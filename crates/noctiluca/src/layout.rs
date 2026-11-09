//! The page as lines of cells, two ways:
//!
//! - [`reader`]: reader mode, the blocks reflowed into one readable column,
//!   with media regions and controls as pieces of their own between the
//!   text;
//! - [`projection`]: Chrome's own geometry snapped to cells, so columns
//!   stay side by side. The first projection (0.1): each box goes at its
//!   snapped place, its text rewrapped to its width, and a skyline per
//!   column pushes a box down below what is already in its columns. So
//!   text never shares a cell with text, and a column that grew pushes
//!   what is under it, not what is beside it. Controls show as faces and
//!   media as framed placeholders with their alt text. Fixed and sticky
//!   layers go on a pinned strip above the page, a docked strip under it,
//!   or their static place in it ([`Place`]); a block an opaque box above
//!   covers is not drawn, and a short text over a picture is drawn in the
//!   picture's cells ([`Projection::overlays`]).
//!
//! Every string here comes from the [model](crate::model), which has
//! cleaned it already.

use std::collections::HashMap;

use rich::Text;
use rich::cells::cell_len;

use crate::adapt::{Adapt, Rule};
use crate::engine::NodeId;
use crate::model::{
    Block, BlockKind, Check, Control, Log, MediaKind, Page, Place, Role, Run, SourcePage, Table,
};

mod source;

/// A run of text on a line with one style (a rich style definition).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub text: String,
    pub style: String,
    /// The control it belongs to.
    pub control: Option<usize>,
    /// A small picture drawn over its cells (an emote, a badge): its
    /// source, a key of [`Page::inline_pictures`]. The text stands in
    /// where pictures are not drawn.
    pub picture: Option<String>,
}

impl Span {
    pub fn new(text: impl Into<String>, style: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            style: style.into(),
            control: None,
            picture: None,
        }
    }
}

/// A line of spans.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Line {
    pub spans: Vec<Span>,
}

impl Line {
    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn width(&self) -> usize {
        self.spans.iter().map(|s| cell_len(&s.text)).sum()
    }
}

/// The control the cell at `column` of `line` belongs to, if any: what a
/// click there uses.
pub fn control_at(line: &Line, column: usize) -> Option<usize> {
    let mut at = 0;
    for span in &line.spans {
        let width = cell_len(&span.text);
        if column < at + width {
            return span.control;
        }
        at += width;
    }
    None
}

/// What the views are asked to mark.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Marks {
    /// Link hint labels, by control.
    pub hints: HashMap<usize, String>,
    /// Text to highlight (find in page), compared without case.
    pub find: Option<String>,
    /// The match find is on, highlighted more: (line, nth match on it).
    pub current: Option<(usize, usize)>,
    /// The terminal adaptations to apply (the defaults, or none for the
    /// faithful projection).
    pub adapt: Adapt,
    /// Where the user has scrolled each live log; a log not here follows
    /// its newest entries.
    pub logs: HashMap<NodeId, LogScroll>,
}

/// Where a live log is scrolled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LogScroll {
    /// At its newest entries, following them as they come.
    #[default]
    Follow,
    /// Scrolled up: this entry is the last in view, whatever comes after.
    Paused(u64),
}

/// `state` moved by `by` entries (up when negative) in `log`: moving down
/// to its newest entry follows it again.
pub fn scroll_log(log: &Log, state: LogScroll, by: isize) -> LogScroll {
    let Some(last) = log.entries.len().checked_sub(1) else {
        return LogScroll::Follow;
    };
    let at = match state {
        LogScroll::Follow => last,
        LogScroll::Paused(key) => log.entries.iter().position(|e| e.key == key).unwrap_or(0),
    };
    let to = (at as isize + by).clamp(0, last as isize) as usize;
    if to == last {
        LogScroll::Follow
    } else {
        LogScroll::Paused(log.entries[to].key)
    }
}

#[cfg(test)]
thread_local! { static TEST_LOG_ENTRY_WRAPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn entry_lines(entry: &crate::model::Entry, width: usize, page: &Page, marks: &Marks) -> Vec<Line> {
    #[cfg(test)]
    TEST_LOG_ENTRY_WRAPS.with(|n| n.set(n.get() + 1));
    wrap(&spans(&entry.runs, "", page, marks), width, &[], &[])
}

/// The `rows` lines of `log` in view at `width` cells: its newest (or,
/// scrolled up, those up to the entry it is paused on) with the
/// [live tail](Rule::LiveTail); without it, from the entry Chrome shows
/// first. Padded to `rows`.
/// Entries outside that window do not need text layout.
pub fn log_lines(log: &Log, width: usize, rows: usize, page: &Page, marks: &Marks) -> Vec<Line> {
    if rows == 0 {
        return Vec::new();
    }
    let position = |key| log.entries.iter().position(|entry| entry.key == key);
    let (first, end, reverse) = if marks.adapt.on(Rule::LiveTail) {
        match marks.logs.get(&log.node).copied().unwrap_or_default() {
            LogScroll::Follow => (0, log.entries.len(), true),
            LogScroll::Paused(key) => match position(key) {
                Some(at) => (0, at + 1, true),
                None => (0, log.entries.len(), false),
            },
        }
    } else {
        (
            log.first_shown.and_then(position).unwrap_or(0),
            log.entries.len(),
            false,
        )
    };
    let mut lines = Vec::new();
    for offset in 0..end - first {
        let index = if reverse {
            end - 1 - offset
        } else {
            first + offset
        };
        let entry = entry_lines(&log.entries[index], width, page, marks);
        if reverse {
            lines.extend(entry.into_iter().rev());
        } else {
            lines.extend(entry);
        }
        if lines.len() >= rows {
            break;
        }
    }
    lines.truncate(rows);
    if reverse {
        lines.reverse();
    }
    lines.resize(rows, Line::default());
    lines
}

/// Lines as rich text, one per row, no wrapping.
pub fn to_text(lines: &[Line], marks: &Marks, first_row: usize) -> Text {
    let mut text = Text::new("").no_wrap(true);
    for (n, line) in lines.iter().enumerate() {
        if n > 0 {
            text.append("\n", None);
        }
        // Text's spans are byte offsets; matches are char offsets.
        let start = text.plain().len();
        for span in &line.spans {
            let style = (!span.style.is_empty()).then(|| span.style.as_str().into());
            text.append(&span.text, style);
        }
        if let Some(query) = marks.find.as_deref().filter(|q| !q.is_empty()) {
            let plain = line.plain();
            let byte = |c: usize| plain.char_indices().nth(c).map_or(plain.len(), |(b, _)| b);
            for (k, (at, len)) in matches(&plain, query).into_iter().enumerate() {
                let current = marks.current == Some((first_row + n, k));
                let style = if current {
                    "bold black on bright_yellow"
                } else {
                    "black on yellow"
                };
                text.stylize(style, start + byte(at), start + byte(at + len));
            }
        }
    }
    text
}

/// Where `query` is in `line`, without case: (char start, char length).
pub fn matches(line: &str, query: &str) -> Vec<(usize, usize)> {
    let hay: Vec<char> = line.chars().flat_map(char::to_lowercase).collect();
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    // Lowercasing can change a char count; only exact one-to-one cases
    // highlight.
    if needle.is_empty() || hay.len() != line.chars().count() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut at = 0;
    while at + needle.len() <= hay.len() {
        if hay[at..at + needle.len()] == needle[..] {
            out.push((at, needle.len()));
            at += needle.len();
        } else {
            at += 1;
        }
    }
    out
}

/// The style of a run in a block.
fn run_style(run: &Run, base: &str, page: &Page, adapt: Adapt) -> String {
    let mut parts: Vec<&str> = Vec::new();
    if !base.is_empty() {
        parts.push(base);
    }
    if run.bold {
        parts.push("bold");
    }
    if run.italic {
        parts.push("italic");
    }
    if run.code {
        parts.push("cyan");
    }
    let background = crate::adapt::background();
    // Adapted, saturated colours stay, made readable; off, the page's own
    // colours, lightness inverted on a dark terminal.
    let raw = !adapt.on(Rule::PageColors);
    let color = run.color.as_deref().and_then(|css| {
        if raw {
            crate::adapt::raw_color(css, background)
        } else if run.control.is_none() {
            crate::adapt::page_color(css, background)
        } else {
            None
        }
    });
    if let Some(color) = &color {
        parts.push(color);
    }
    if run.picture.is_some() {
        parts.push("italic bright_black");
    }
    if let Some(control) = run.control.and_then(|i| page.controls.get(i))
        && control.role == Role::Link
    {
        parts.push(if raw && color.is_some() {
            "underline"
        } else {
            "underline bright_blue"
        });
    }
    parts.join(" ")
}

/// The spans of `runs`, a hint label in front of each control's first run.
fn spans(runs: &[Run], base: &str, page: &Page, marks: &Marks) -> Vec<Span> {
    let mut out = Vec::new();
    let mut last = None;
    for run in runs {
        // Of a pair of screen-reader twins, one is drawn: the spoken one,
        // adapted; the shown parts, faithful.
        let spoken = marks.adapt.on(Rule::ScreenReaderTwins);
        match run.twin {
            Some(crate::model::Twin::Shown) if spoken => continue,
            Some(crate::model::Twin::Spoken) if !spoken => continue,
            _ => {}
        }
        // A decorative picture in a line is dropped, adapted.
        if run
            .picture
            .as_ref()
            .is_some_and(|p| p.decorative && marks.adapt.on(Rule::DecorativeImages))
        {
            continue;
        }
        if run.control.is_some()
            && run.control != last
            && let Some(label) = run.control.and_then(|c| marks.hints.get(&c))
        {
            out.push(Span {
                text: format!("[{label}]"),
                style: "bold black on yellow".into(),
                control: run.control,
                picture: None,
            });
        }
        last = run.control;
        // A small picture in the line keeps its cells, its alt text
        // standing in for it, in one piece.
        let picture = run
            .picture
            .as_ref()
            .filter(|p| !p.src.is_empty() && marks.adapt.on(Rule::InlinePictures));
        out.push(Span {
            text: match picture {
                Some(p) => inline_cells(&p.alt, p.width),
                None => run.text.clone(),
            },
            style: run_style(run, base, page, marks.adapt),
            control: run.control,
            picture: picture.map(|p| p.src.clone()),
        });
    }
    out
}

/// Wrap `spans` to `width` cells: words kept whole where they fit, `\n`
/// breaking a line, `first` in front of the first line and `rest` in
/// front of the others.
pub fn wrap(spans: &[Span], width: usize, first: &[Span], rest: &[Span]) -> Vec<Line> {
    let width = width.max(1);
    let prefix_width = |p: &[Span]| p.iter().map(|s| cell_len(&s.text)).sum::<usize>();
    let mut lines = Vec::new();
    let mut line = Line {
        spans: first.to_vec(),
    };
    let mut used = prefix_width(first);
    let avail = |prefix: usize| width.saturating_sub(prefix).max(1);
    // Words: (text, span index), spaces as their own words.
    let mut words: Vec<(String, usize)> = Vec::new();
    for (i, span) in spans.iter().enumerate() {
        let mut word = String::new();
        for c in span.text.chars() {
            if c == ' ' || c == '\n' {
                if !word.is_empty() {
                    words.push((std::mem::take(&mut word), i));
                }
                words.push((c.to_string(), i));
            } else {
                word.push(c);
            }
        }
        if !word.is_empty() {
            words.push((word, i));
        }
    }
    let push = |line: &mut Line, text: &str, i: usize| {
        let span = &spans[i];
        match line.spans.last_mut() {
            Some(last)
                if last.style == span.style
                    && last.control == span.control
                    && last.picture.is_none()
                    && span.picture.is_none()
                    && !last.text.is_empty() =>
            {
                last.text.push_str(text)
            }
            _ => line.spans.push(Span {
                text: text.to_string(),
                style: span.style.clone(),
                control: span.control,
                picture: span.picture.clone(),
            }),
        }
    };
    let mut start_of_line = true;
    for (word, i) in words {
        if word == "\n" {
            lines.push(std::mem::replace(
                &mut line,
                Line {
                    spans: rest.to_vec(),
                },
            ));
            used = prefix_width(rest);
            start_of_line = true;
            continue;
        }
        if word == " " {
            if start_of_line {
                continue;
            }
            if used < width {
                push(&mut line, " ", i);
                used += 1;
            }
            continue;
        }
        let w = cell_len(&word);
        if used + w > width && !start_of_line {
            // Drop the trailing space before breaking.
            if let Some(last) = line.spans.last_mut()
                && last.text.ends_with(' ')
            {
                last.text.pop();
            }
            lines.push(std::mem::replace(
                &mut line,
                Line {
                    spans: rest.to_vec(),
                },
            ));
            used = prefix_width(rest);
        }
        if w > avail(used.min(width)) && used + w > width {
            // A word longer than a line: break it by characters.
            let mut chunk = String::new();
            for c in word.chars() {
                let cw = cell_len(&c.to_string());
                if used + cell_len(&chunk) + cw > width && !chunk.is_empty() {
                    push(&mut line, &chunk, i);
                    chunk.clear();
                    lines.push(std::mem::replace(
                        &mut line,
                        Line {
                            spans: rest.to_vec(),
                        },
                    ));
                    used = prefix_width(rest);
                }
                chunk.push(c);
            }
            used += cell_len(&chunk);
            push(&mut line, &chunk, i);
        } else {
            push(&mut line, &word, i);
            used += w;
        }
        start_of_line = false;
    }
    if let Some(last) = line.spans.last_mut() {
        while last.text.ends_with(' ') {
            last.text.pop();
        }
    }
    lines.push(line);
    lines
}

/// A piece of the reader view.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(Vec<Line>),
    /// A media region: an index into [`Page::media`], `rows` tall.
    Media {
        index: usize,
        rows: u16,
    },
    /// Controls drawn as widgets: one control, or a group of radios.
    Controls(Vec<usize>),
    /// A video's or an audio's controls: an index into [`Page::media`].
    Player(usize),
}

impl Piece {
    /// How many rows it takes.
    pub fn rows(&self) -> usize {
        match self {
            Piece::Text(lines) => lines.len(),
            Piece::Media { rows, .. } => *rows as usize,
            Piece::Controls(controls) => controls.len().max(1),
            Piece::Player(_) => 1,
        }
    }
}

/// How many rows a media region takes in reader mode.
fn media_rows(height_px: f64) -> u16 {
    ((height_px / 20.0).ceil() as u16).clamp(2, 16)
}

/// A table as lines: columns sized to their text, shrunk to fit `width`
/// (the widest first), cells wrapped, a rule under a header row.
fn table_lines(table: &Table, width: usize, page: &Page, marks: &Marks) -> Vec<Line> {
    let mut lines = Vec::new();
    if !table.caption.is_empty() {
        lines.push(Line {
            spans: vec![Span::new(table.caption.clone(), "bold italic")],
        });
    }
    let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
    if columns == 0 {
        return lines;
    }
    let mut widths = vec![1usize; columns];
    for row in &table.rows {
        for (c, cell) in row.iter().enumerate() {
            widths[c] = widths[c].max(cell_len(&cell.text()).min(60));
        }
    }
    let separators = 3 * (columns - 1);
    let budget = width.saturating_sub(separators).max(columns);
    while widths.iter().sum::<usize>() > budget {
        let (widest, _) = widths
            .iter()
            .enumerate()
            .max_by_key(|(_, w)| **w)
            .unwrap_or((0, &1));
        if widths[widest] <= 4 {
            break;
        }
        widths[widest] -= 1;
    }
    for (r, row) in table.rows.iter().enumerate() {
        let cells: Vec<Vec<Line>> = (0..columns)
            .map(|c| match row.get(c) {
                Some(cell) => {
                    let base = if cell.header { "bold" } else { "" };
                    wrap(&spans(&cell.runs, base, page, marks), widths[c], &[], &[])
                }
                None => vec![Line::default()],
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1);
        for k in 0..height {
            let mut line = Line::default();
            for (c, cell) in cells.iter().enumerate() {
                if c > 0 {
                    line.spans.push(Span::new(" │ ", "dim"));
                }
                let part = cell.get(k).cloned().unwrap_or_default();
                let pad = widths[c].saturating_sub(part.width());
                line.spans.extend(part.spans);
                if c + 1 < columns {
                    line.spans.push(Span::new(" ".repeat(pad), ""));
                }
            }
            lines.push(line);
        }
        if r == 0 && row.iter().all(|c| c.header) {
            let rule = widths
                .iter()
                .map(|w| "─".repeat(*w))
                .collect::<Vec<_>>()
                .join("─┼─");
            lines.push(Line {
                spans: vec![Span::new(rule, "dim")],
            });
        }
    }
    lines
}

/// The page in reader mode at `width` cells: pieces of text, media and
/// controls, in reading order.
pub fn reader(page: &Page, width: u16, marks: &Marks) -> Vec<Piece> {
    #[cfg(test)]
    crate::app::TEST_READER_LAYOUTS.with(|n| n.set(n.get() + 1));
    let width = width.max(10) as usize;
    let mut pieces: Vec<Piece> = Vec::new();
    let mut text: Vec<Line> = Vec::new();
    let mut previous: Option<&BlockKind> = None;
    let flush = |text: &mut Vec<Line>, pieces: &mut Vec<Piece>| {
        while text.last().is_some_and(|l| l.plain().is_empty()) {
            text.pop();
        }
        if !text.is_empty() {
            pieces.push(Piece::Text(std::mem::take(text)));
        }
    };
    let mut b = 0;
    while b < page.blocks.len() {
        let block = &page.blocks[b];
        let tight = matches!(
            (previous, &block.kind),
            (Some(BlockKind::ListItem { .. }), BlockKind::ListItem { .. })
                | (Some(BlockKind::Control(_)), BlockKind::Control(_))
        );
        if previous.is_some()
            && !tight
            && !matches!(block.kind, BlockKind::Control(_) | BlockKind::Media(_))
        {
            text.push(Line::default());
        }
        let quote: Vec<Span> = (0..block.quote).map(|_| Span::new("│ ", "dim")).collect();
        let inner = width.saturating_sub(2 * block.quote as usize).max(8);
        match &block.kind {
            BlockKind::Heading(level) => {
                let base = match level {
                    1 => "bold underline",
                    2 => "bold",
                    _ => "bold",
                };
                let mut runs = block.runs.clone();
                for run in &mut runs {
                    run.text = run.text.replace('\n', " ");
                }
                text.extend(wrap(
                    &spans(&runs, base, page, marks),
                    width,
                    &quote,
                    &quote,
                ));
            }
            BlockKind::Paragraph => {
                let base = if block.quote > 0 { "italic" } else { "" };
                let _ = inner;
                text.extend(wrap(
                    &spans(&block.runs, base, page, marks),
                    width,
                    &quote,
                    &quote,
                ));
            }
            BlockKind::ListItem { depth, marker } => {
                let indent = "  ".repeat(depth.saturating_sub(1) as usize);
                let marker = if marker.is_empty() {
                    String::new()
                } else {
                    format!("{marker} ")
                };
                let hang = " ".repeat(cell_len(&marker).max(2));
                let mut first = quote.clone();
                first.push(Span::new(format!("{indent}{marker}"), "bright_black"));
                if marker.is_empty() {
                    first.push(Span::new(hang.clone(), ""));
                }
                let mut rest = quote.clone();
                rest.push(Span::new(format!("{indent}{hang}"), ""));
                text.extend(wrap(
                    &spans(&block.runs, "", page, marks),
                    width,
                    &first,
                    &rest,
                ));
            }
            BlockKind::Code => {
                let code = crate::model::plain(&block.runs);
                for row in code.split('\n') {
                    let mut first = quote.clone();
                    first.push(Span::new("▏", "bright_black"));
                    let rest = first.clone();
                    let span = Span::new(row, "cyan");
                    // Code is cut by characters, never by words.
                    let mut lines = wrap_chars(&span, width, &first, &rest);
                    text.append(&mut lines);
                }
            }
            BlockKind::Table(table) => {
                text.extend(table_lines(table, width, page, marks));
            }
            BlockKind::Media(index)
                if marks.adapt.on(Rule::DecorativeImages)
                    && crate::adapt::decorative(&page.media[*index]) => {}
            BlockKind::Media(index) if crate::indicator::instead(page, *index, marks.adapt) => {}
            BlockKind::Indicator(index) => {
                if let Some(lines) =
                    crate::indicator::reader_lines(page, *index, width, marks.adapt)
                {
                    text.extend(lines);
                }
            }
            BlockKind::Log(index) => {
                for entry in &page.logs[*index].entries {
                    text.extend(wrap(
                        &spans(&entry.runs, "", page, marks),
                        width,
                        &quote,
                        &quote,
                    ));
                }
            }
            BlockKind::Media(index) => {
                let media = &page.media[*index];
                let kind = match media.kind {
                    MediaKind::Image => "image",
                    MediaKind::Video => "video",
                    MediaKind::Audio => "audio",
                    MediaKind::Canvas => "canvas",
                    MediaKind::Svg => "drawing",
                    MediaKind::Frame => "frame",
                };
                let alt = if media.alt.is_empty() {
                    kind.to_string()
                } else {
                    format!("{kind}: {}", media.alt)
                };
                if previous.is_some() {
                    text.push(Line::default());
                }
                text.extend(wrap(
                    &[Span::new(alt, "italic bright_black")],
                    width,
                    &quote,
                    &quote,
                ));
                // A drawing's own words (a dial's number), as it reads now.
                if !media.text.is_empty() && media.text != media.alt {
                    text.extend(wrap(
                        &[Span::new(media.text.clone(), "")],
                        width,
                        &quote,
                        &quote,
                    ));
                }
                flush(&mut text, &mut pieces);
                // A video's region shows its poster, then its frames while
                // it plays, over its controls.
                if matches!(media.kind, MediaKind::Image | MediaKind::Video) {
                    pieces.push(Piece::Media {
                        index: *index,
                        rows: media_rows(media.rect.height),
                    });
                }
                if matches!(media.kind, MediaKind::Video | MediaKind::Audio) {
                    pieces.push(Piece::Player(*index));
                }
            }
            BlockKind::Control(index) => {
                if previous.is_some() && !tight {
                    text.push(Line::default());
                }
                flush(&mut text, &mut pieces);
                // Radios of one group, one after another, are one widget.
                let mut group = vec![*index];
                let control = &page.controls[*index];
                if control.role == Role::Radio && control.group.is_some() {
                    while let Some(BlockKind::Control(next)) =
                        page.blocks.get(b + 1).map(|b| &b.kind)
                    {
                        let other = &page.controls[*next];
                        if other.role == Role::Radio && other.group == control.group {
                            group.push(*next);
                            b += 1;
                        } else {
                            break;
                        }
                    }
                }
                pieces.push(Piece::Controls(group));
            }
        }
        previous = Some(&page.blocks[b].kind);
        b += 1;
    }
    flush(&mut text, &mut pieces);
    pieces
}

/// One span cut into lines of `width` cells by characters.
fn wrap_chars(span: &Span, width: usize, first: &[Span], rest: &[Span]) -> Vec<Line> {
    let prefix = |p: &[Span]| p.iter().map(|s| cell_len(&s.text)).sum::<usize>();
    let mut lines = Vec::new();
    let mut line = Line {
        spans: first.to_vec(),
    };
    let mut used = prefix(first);
    let mut chunk = String::new();
    for c in span.text.chars() {
        let w = cell_len(&c.to_string());
        if used + w > width && !chunk.is_empty() {
            line.spans
                .push(Span::new(std::mem::take(&mut chunk), span.style.clone()));
            lines.push(std::mem::replace(
                &mut line,
                Line {
                    spans: rest.to_vec(),
                },
            ));
            used = prefix(rest);
        }
        chunk.push(c);
        used += w;
    }
    if !chunk.is_empty() {
        line.spans.push(Span::new(chunk, span.style.clone()));
    }
    lines.push(line);
    lines
}

/// A control's face as the projection draws it in `cells` cells: with
/// [icon buttons](Rule::IconButtons) off, an icon-only button shows as
/// its picture would, a shaded box.
pub fn face_in(control: &Control, adapt: Adapt, cells: usize) -> String {
    if control.icon_only && control.role == Role::Button && !adapt.on(Rule::IconButtons) {
        return "░".repeat(cells.max(1));
    }
    face(control)
}

/// `first` then `spans` on one line of at most `width` cells, its last
/// cell a `…` when cut.
fn cut_line(first: &[Span], spans: &[Span], width: usize) -> Line {
    let mut line = Line::default();
    let mut used = 0;
    let all: Vec<&Span> = first.iter().chain(spans).collect();
    let total: usize = all.iter().map(|s| cell_len(&s.text)).sum();
    for span in all {
        let mut text = String::new();
        for c in span.text.chars() {
            let w = cell_len(&c.to_string());
            if total > width && used + w + 1 > width {
                text.push('…');
                used = width;
                break;
            }
            text.push(c);
            used += w;
        }
        line.spans.push(Span {
            text,
            style: span.style.clone(),
            control: span.control,
            picture: span.picture.clone(),
        });
        if used >= width {
            break;
        }
    }
    line
}

/// The cells a small picture in a line takes, as text: its alt text (or
/// `▫`) cut or padded to its width in cells (a cell is 8 CSS pixels, one
/// to twelve of them), in one piece (no-break spaces), so wrapping never
/// splits it. The picture is drawn over them; where it is not, the text
/// stands in.
pub fn inline_cells(alt: &str, width_px: f64) -> String {
    let cells = ((width_px / 8.0).round() as usize).clamp(1, 12);
    let alt = if alt.trim().is_empty() {
        "▫"
    } else {
        alt.trim()
    };
    let mut out = String::new();
    let mut used = 0;
    let total = cell_len(alt);
    for c in alt.chars() {
        let c = if c == ' ' { '\u{a0}' } else { c };
        let w = cell_len(&c.to_string());
        if used + w > cells || (total > cells && used + w + 1 > cells) {
            if total > cells && used < cells {
                out.push('…');
                used += 1;
            }
            break;
        }
        out.push(c);
        used += w;
    }
    while used < cells {
        out.push('\u{a0}');
        used += 1;
    }
    out
}

/// A face without the spaces inside its brackets: `[ Send ]` as `[Send]`.
fn compact_face(face: &str) -> String {
    match face.strip_prefix("[ ").and_then(|f| f.strip_suffix(" ]")) {
        Some(name) => format!("[{name}]"),
        None => face.to_string(),
    }
}

/// A control's face in text: what it is and what it holds.
pub fn face(control: &Control) -> String {
    let value = control.value.as_deref().unwrap_or("");
    let name = &control.name;
    match control.role {
        Role::Button | Role::MenuItem | Role::Tab => match &control.glyph {
            // An icon button's glyph; its name is in its hint and lists.
            Some(glyph) => format!("[{glyph}]"),
            None => format!("[ {name} ]"),
        },
        Role::Link => name.clone(),
        Role::CheckBox | Role::Switch => {
            let mark = match control.checked {
                Some(Check::On) => "x",
                Some(Check::Mixed) => "-",
                _ => " ",
            };
            format!("[{mark}] {name}")
        }
        Role::Radio => {
            let mark = if control.checked == Some(Check::On) {
                "•"
            } else {
                " "
            };
            format!("({mark}) {name}")
        }
        Role::Password => format!("{name} [{}]", "•".repeat(8)),
        Role::ComboBox | Role::ListBox => format!("{name} ‹{value} ▾›"),
        Role::Slider => format!("{name} ◀ {value} ▶"),
        Role::TextBox | Role::SearchBox | Role::SpinButton => format!("{name} [{value}_]"),
    }
}

/// The most blank rows the projection keeps between two boxes.
const MAX_GAP_ROWS: usize = 200;
/// The play mark on a video that is not playing, as the PoC drew it: a
/// light box with a `▶` at the region's bottom left, the rest of the row
/// as it was. `None` for other media, a playing video, or no room.
pub fn play_mark(media: &crate::model::Media, width: usize) -> Option<Line> {
    if media.kind != MediaKind::Video || media.playback.is_some_and(|p| p.playing()) || width < 3 {
        return None;
    }
    Some(Line {
        spans: vec![
            Span::new(" ▶ ", "bold black on white"),
            Span::new("░".repeat(width - 3), "bright_black"),
        ],
    })
}

/// Pictures at least this many rows tall get an `Image:` line.
const CAPTIONED_ROWS: usize = 3;
/// The most rows a media region takes in the projection.
const MAX_MEDIA_ROWS: usize = 200;

/// The page projected by Chrome's geometry into a canvas `columns` wide,
/// with cells of `cell` CSS pixels: the lines, one per row.
pub fn projection(page: &Page, columns: u16, cell: (f64, f64), marks: &Marks) -> Vec<Line> {
    project(page, columns, cell, marks).lines
}

/// Where a media region landed in the projection: its first row and
/// column, its size in cells, and which of [`Page::media`] it shows. A
/// picture is drawn over these cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Region {
    pub row: usize,
    pub column: usize,
    pub cols: usize,
    pub rows: usize,
    pub media: usize,
}

impl Region {
    /// Whether it covers row `row`.
    pub fn on(&self, row: usize) -> bool {
        (self.row..self.row + self.rows).contains(&row)
    }
}

/// The [`projection`], with what each row is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Projection {
    /// The lines, one per row.
    pub lines: Vec<Line>,
    /// Each row's key, stable across readings of the page: the node of the
    /// first box on it (its `backendNodeId`) and which of the box's lines
    /// it is. A scroll view anchors on it, so the row in view stays when
    /// the page grows above it.
    pub keys: Vec<u64>,
    /// Where each row is on the page, in CSS pixels from its top: its
    /// box's top plus its line's offset. Chrome's scroll follows it.
    pub y: Vec<f64>,
    /// Each key's row.
    pub rows: HashMap<u64, usize>,
    /// Each live log's place: its node, its first row and its rows.
    pub logs: Vec<(NodeId, usize, usize)>,
    /// The media regions, in the order they were placed.
    pub regions: Vec<Region>,
    /// The small pictures in lines, one row each, with their sources (keys
    /// of [`Page::inline_pictures`]); their `media` is `usize::MAX`.
    pub inline: Vec<(Region, String)>,
    /// Short texts drawn over a picture's cells (a duration badge on a
    /// thumbnail), as (row, column, line): already in [`lines`](Self::lines),
    /// and drawn again over the picture once it shows.
    pub overlays: Vec<(usize, usize, Line)>,
    /// The page's top bars ([`Place::Pinned`]), projected on their own:
    /// drawn above the page, never scrolled.
    pub pinned: Option<Box<Projection>>,
    /// Its bottom bars ([`Place::Docked`]: cookie banners, sign-up bars),
    /// projected on their own: drawn under the page, above the status bar.
    pub docked: Option<Box<Projection>>,
}

/// Which part of the page a projection holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Part {
    Flow,
    Pinned,
    Docked,
    Overlay(usize),
    Foreground(NodeId),
}

/// How far down `part` moves `block`'s boxes, in CSS pixels; `None` when
/// it is not in `part`. `docked` is the top of the highest docked layer.
fn shift(page: &Page, block: &Block, part: Part, pinned: f64, docked: f64) -> Option<f64> {
    let layer_index = block.stack.layer;
    if let Part::Foreground(node) = part {
        return (block.node == node).then_some(-block.rect.y);
    }
    if let Part::Overlay(index) = part {
        let dialog = page.overlays.get(index)?;
        return (layer_index == Some(dialog.layer)).then(|| -page.layers[dialog.layer].rect.y);
    }
    if layer_index.is_some_and(|index| page.overlays.iter().any(|d| d.layer == index)) {
        return None;
    }
    let Some(layer) = layer_index.and_then(|l| page.layers.get(l)) else {
        return (part == Part::Flow).then_some(0.0);
    };
    let base = match (layer.place, part) {
        (Place::Flow, Part::Flow) => 0.0,
        (Place::Pinned, Part::Pinned) => pinned,
        (Place::Docked, Part::Docked) => docked,
        _ => return None,
    };
    Some(layer.top - base - layer.rect.y)
}

impl Projection {
    /// The page's y, in CSS pixels, of row `row` (past the end, the last
    /// row's).
    pub fn y_of(&self, row: usize) -> f64 {
        self.y
            .get(row)
            .or(self.y.last())
            .copied()
            .unwrap_or_default()
    }
}

/// The [`projection`] with its rows' keys and page positions, and the
/// page's pinned and docked strips.
pub fn project(page: &Page, columns: u16, cell: (f64, f64), marks: &Marks) -> Projection {
    project_with_logs(page, columns, cell, marks, &mut Vec::new())
}

/// Project one source-declared dialog independently of the page flow.
/// Its control indices continue to refer to the original page.
pub fn project_overlay(
    page: &Page,
    index: usize,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
) -> Option<Projection> {
    page.overlays.get(index)?;
    Some(project_part(
        page,
        None,
        columns,
        cell,
        marks,
        Part::Overlay(index),
        &mut Vec::new(),
    ))
}

pub(crate) fn project_foreground(
    page: &Page,
    node: NodeId,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
) -> Projection {
    project_part(
        page,
        None,
        columns,
        cell,
        marks,
        Part::Foreground(node),
        &mut Vec::new(),
    )
}

pub(crate) fn project_source_overlay(
    source: &SourcePage,
    index: usize,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
) -> Option<Projection> {
    source.page().overlays.get(index)?;
    Some(project_part(
        source.page(),
        Some(source),
        columns,
        cell,
        marks,
        Part::Overlay(index),
        &mut Vec::new(),
    ))
}

pub(crate) fn project_source_foreground(
    source: &SourcePage,
    node: NodeId,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
) -> Projection {
    project_part(
        source.page(),
        Some(source),
        columns,
        cell,
        marks,
        Part::Foreground(node),
        &mut Vec::new(),
    )
}

#[derive(Clone, Debug)]
pub(crate) struct LogSlot {
    pub node: NodeId,
    pub region: Region,
    pub part: Part,
}

pub(crate) fn project_with_logs(
    page: &Page,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
    slots: &mut Vec<LogSlot>,
) -> Projection {
    project_pair(page, None, columns, cell, marks, slots)
}

/// Project the exact source relationships paired with this page.
pub fn project_source(
    source: &SourcePage,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
) -> Projection {
    project_source_with_logs(source, columns, cell, marks, &mut Vec::new())
}

pub(crate) fn project_source_with_logs(
    source: &SourcePage,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
    slots: &mut Vec<LogSlot>,
) -> Projection {
    project_pair(source.page(), Some(source), columns, cell, marks, slots)
}

fn project_pair(
    page: &Page,
    source: Option<&SourcePage>,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
    slots: &mut Vec<LogSlot>,
) -> Projection {
    let mut main = project_part(page, source, columns, cell, marks, Part::Flow, slots);
    for (place, part) in [(Place::Pinned, Part::Pinned), (Place::Docked, Part::Docked)] {
        if !page.layers.iter().any(|l| l.place == place) {
            continue;
        }
        let strip = project_part(page, source, columns, cell, marks, part, slots);
        if strip.lines.is_empty() {
            continue;
        }
        let strip = Some(Box::new(strip));
        match part {
            Part::Pinned => main.pinned = strip,
            _ => main.docked = strip,
        }
    }
    main
}

/// Cells a run of controls at the right edge keeps for each control when
/// it has to shorten them: `[`, a letter, `…` and `]`.
const MIN_FACE: usize = 4;

/// `face` (a control's face, `[Sign Up]`) cut to `width` cells with `…`,
/// its closing bracket kept.
fn cut_face(face: &str, width: usize) -> String {
    if cell_len(face) <= width {
        return face.to_string();
    }
    let (inner, close) = match face.strip_suffix(']') {
        Some(inner) if width >= 3 => (inner, "]"),
        _ => (face, ""),
    };
    let room = width.saturating_sub(cell_len(close) + 1);
    let mut out = String::new();
    let mut used = 0;
    for c in inner.chars() {
        let w = cell_len(&c.to_string());
        if used + w > room {
            break;
        }
        out.push(c);
        used += w;
    }
    format!("{}…{close}", out.trim_end())
}

/// A box to place: where Chrome put it, and what it holds.
struct Item {
    source: Option<usize>,
    anchor: Option<f64>,
    allocation: Option<(usize, usize)>,
    top: f64,
    bottom: f64,
    left: f64,
    /// Unclipped source edges for a source-container transform. Terminal
    /// clipping must not erase a pane to the right before it is stacked.
    source_x: (usize, usize),
    x0: usize,
    x1: usize,
    /// Text to wrap to the box's width, with a first-line prefix; or
    /// lines kept as they are (code, media).
    wrap: Option<(Vec<Span>, Vec<Span>)>,
    lines: Vec<Line>,
    /// The node it shows.
    node: crate::engine::NodeId,
    /// The live log it is.
    log: Option<NodeId>,
    /// A media region's index and its picture's rows (the lines after
    /// are its caption).
    media: Option<(usize, usize, usize)>,
    /// Kept to one line, cut with `…` (a control's face).
    one_line: bool,
    /// A table cell's row (the table's node, the row's index): the
    /// cells of a row start on one terminal row.
    row: Option<(crate::engine::NodeId, usize)>,
    /// A picture's `Image:` caption, wrapped under it to the box's
    /// width once the box has its final columns (a snapped grid can
    /// narrow it).
    caption: Option<Vec<Span>>,
}

/// One part of the page projected ([`project`]).
fn project_part(
    page: &Page,
    source: Option<&SourcePage>,
    columns: u16,
    cell: (f64, f64),
    marks: &Marks,
    part: Part,
    slots: &mut Vec<LogSlot>,
) -> Projection {
    let context = source.map(|source| source::Context::new(source, marks.adapt));
    let source = context.as_ref();
    let columns = columns.max(10) as usize;
    let (cw, ch) = (cell.0.max(1.0), cell.1.max(1.0));
    let origin_x = match part {
        Part::Foreground(node) => page
            .blocks
            .iter()
            .find(|b| b.node == node)
            .map_or(0.0, |b| b.rect.x),
        Part::Overlay(index) => page
            .overlays
            .get(index)
            .and_then(|d| page.layers.get(d.layer))
            .map_or(0.0, |layer| layer.rect.x),
        _ => 0.0,
    };
    let mut items: Vec<Item> = Vec::new();
    // The tops of the highest pinned and docked layers: each strip starts
    // at its first.
    let top_of = |place: Place| {
        page.layers
            .iter()
            .filter(|l| l.place == place)
            .map(|l| l.top)
            .fold(f64::INFINITY, f64::min)
    };
    let (pinned_top, docked_top) = (top_of(Place::Pinned).max(0.0), top_of(Place::Docked));
    let in_part = |block: &Block| shift(page, block, part, pinned_top, docked_top);
    // The pictures this part draws: a box over one of them is in its
    // pixels, or (a short text) drawn in its cells.
    let adapt = marks.adapt;
    let pictured: std::collections::HashSet<usize> = page
        .blocks
        .iter()
        .filter(|b| !b.stack.covered && in_part(b).is_some())
        .filter_map(|b| match b.kind {
            BlockKind::Media(m) => Some(m),
            _ => None,
        })
        .filter(|&m| {
            let media = &page.media[m];
            !(adapt.on(Rule::DecorativeImages) && crate::adapt::decorative(media))
                && !(adapt.on(Rule::TextOverPictures)
                    && crate::adapt::picture_of_text(media, cell.1.max(1.0)))
        })
        .collect();
    // Short texts over a picture: (the picture, the box), placed in its
    // cells once it has its place.
    let mut overlaid: Vec<(usize, Item)> = Vec::new();
    let source_snap = |left: f64, right: f64| -> (usize, usize) {
        let start = (((left - origin_x) / cw).round().max(0.0) as usize).min(usize::MAX - 1);
        (
            start,
            (((right - origin_x) / cw).round().max(0.0) as usize).max(start + 1),
        )
    };
    let snap = |left: f64, right: f64| -> (usize, usize) {
        // Both edges round, so boxes that touch in Chrome touch here and
        // never share a column.
        let x0 = (((left - origin_x) / cw).round().max(0.0) as usize).min(columns - 1);
        let x1 = (((right - origin_x) / cw).round() as usize).clamp(x0 + 1, columns);
        (x0, x1)
    };
    let text_item = |node: crate::engine::NodeId,
                     rect: crate::engine::Rect,
                     left: f64,
                     right: f64,
                     spans: Vec<Span>,
                     first: Vec<Span>| {
        let (x0, x1) = snap(left, right);
        Item {
            source: None,
            anchor: None,
            allocation: None,
            top: rect.y,
            bottom: rect.bottom(),
            left,
            source_x: source_snap(left, right),
            x0,
            x1,
            wrap: Some((spans, first)),
            lines: Vec::new(),
            node,
            log: None,
            media: None,
            one_line: false,
            row: None,
            caption: None,
        }
    };
    // Landmarks that collapse: on a narrow terminal, with the rule, in the
    // page's flow (not in a top or bottom bar).
    let collapsed: Vec<bool> = page
        .regions
        .iter()
        .enumerate()
        .map(|(n, r)| {
            part == Part::Flow
                && adapt.on(Rule::CollapseNav)
                && columns < crate::adapt::NARROW_COLUMNS as usize
                && r.collapsible
                && r.rect.height > 1.5 * ch
                && page
                    .blocks
                    .iter()
                    .filter(|b| b.region == Some(n))
                    .all(|b| in_part(b) == Some(0.0))
        })
        .collect();
    // Boxes that shrank: (item, its height before in CSS pixels, its
    // columns before), whose saved rows move what is under them up.
    let mut shrunk: Vec<(usize, f64, usize, usize)> = Vec::new();
    // Columns freed by a collapsed sidebar: (top, bottom, first, end), for
    // the boxes beside it to move into.
    let mut freed: Vec<(f64, f64, usize, usize)> = Vec::new();
    // Controls: one row each, cut rather than wrapped.
    let mut one_row: Vec<usize> = Vec::new();
    // The one-row indicators among them (a bar, steps, stars).
    let mut indicator_items: Vec<usize> = Vec::new();
    for (r, region) in page.regions.iter().enumerate() {
        if !collapsed[r] {
            continue;
        }
        // One row: its name, then its links, as many as fit.
        let name = if region.label.is_empty() {
            match region.kind {
                crate::model::RegionKind::Navigation => "Navigation",
                crate::model::RegionKind::Complementary => "Aside",
            }
            .to_string()
        } else {
            region.label.clone()
        };
        let mut row = vec![Span::new(format!("≡ {name}:"), "bold")];
        let mut seen = std::collections::HashSet::new();
        for block in page.blocks.iter().filter(|b| b.region == Some(r)) {
            let links = block.runs.iter().filter_map(|run| run.control);
            let controls = match block.kind {
                BlockKind::Control(c) => vec![c],
                _ => links.collect(),
            };
            for c in controls {
                if seen.insert(c) {
                    let mut first = Vec::new();
                    if let Some(label) = marks.hints.get(&c) {
                        first.push(Span {
                            text: format!("[{label}]"),
                            style: "bold black on yellow".into(),
                            control: Some(c),
                            picture: None,
                        });
                    }
                    row.push(Span::new(" ", ""));
                    row.extend(first);
                    row.push(Span {
                        text: page.controls[c].name.clone(),
                        style: "underline bright_blue".into(),
                        control: Some(c),
                        picture: None,
                    });
                }
            }
        }
        let (x0, mut x1) = snap(region.rect.x, region.rect.right());
        // A sidebar (taller than a few rows) gives its columns to what is
        // beside it, and its row spans the page.
        let side = region.rect.height > 3.0 * ch;
        if side {
            freed.push((region.rect.y, region.rect.bottom(), x0, x1));
            x1 = columns;
        }
        let mut line = wrap(&row, x1 - x0, &[], &[]).swap_remove(0);
        if row.len() > line.spans.len() {
            line.spans.push(Span::new(" …", "dim"));
        }
        let mut rect = region.rect;
        rect.height = ch;
        items.push(Item {
            source: None,
            anchor: None,
            allocation: None,
            top: rect.y,
            bottom: rect.y + ch,
            left: rect.x,
            source_x: source_snap(rect.x, rect.right()),
            x0,
            x1,
            wrap: None,
            lines: vec![line],
            node: region.node,
            log: None,
            media: None,
            one_line: false,
            row: None,
            caption: None,
        });
        if !side {
            shrunk.push((items.len() - 1, region.rect.height, x0, x1));
        }
    }
    // A shelf's strip, under its box: what it shows of its items, and
    // the arrows that page it.
    if part == Part::Flow && adapt.on(Rule::Shelves) {
        for shelf in &page.shelves {
            let (x0, x1) = snap(shelf.rect.x, shelf.rect.right());
            let top = shelf.rect.bottom();
            items.push(Item {
                source: None,
                anchor: None,
                allocation: None,
                top,
                bottom: top + ch,
                left: shelf.rect.x,
                source_x: source_snap(shelf.rect.x, shelf.rect.right()),
                x0,
                x1,
                wrap: None,
                lines: vec![crate::shelf::strip(
                    &shelf.clipped(columns as f64 * cw),
                    x1 - x0,
                )],
                node: shelf.node,
                log: None,
                media: None,
                one_line: false,
                row: None,
                caption: None,
            });
        }
    }
    for (block_index, block) in page.blocks.iter().enumerate() {
        if block.region.is_some_and(|r| collapsed[r]) || block.stack.covered {
            continue;
        }
        let Some(dy) = in_part(block) else {
            continue;
        };
        // Art over a picture is in the picture's pixels: it takes no rows.
        let over = block.stack.over.filter(|m| pictured.contains(m));
        if over.is_some() && matches!(block.kind, BlockKind::Media(_)) {
            continue;
        }
        let first_item = items.len();
        // Wholly above or left of the page: text moved off screen for
        // screen readers only (Wikipedia's reference back-links sit
        // 100,000 pixels up), never shown.
        if (block.rect.bottom() < 0.0 || block.rect.right() < 0.0)
            && !source.is_some_and(|s| source::has_alias(s, block_index, marks))
        {
            continue;
        }
        // Scrolled out of a shelf's box sideways: Chrome shows it only
        // once the shelf is paged to it.
        if block
            .stack
            .shelf
            .and_then(|k| page.shelves.get(k))
            .is_some_and(|s| s.clipped(columns as f64 * cw).hides(&block.rect))
        {
            continue;
        }
        // A block's text wraps within the box that holds it; but text that
        // sits well inside that box (after an avatar in a flex row) keeps
        // to where it is, and one line keeps to its own end, so what is
        // beside it keeps its columns.
        let framed = block.frame.width > 0.0;
        let left = if framed && block.rect.x - block.frame.x <= 2.0 * cw {
            block.rect.x.min(block.frame.x)
        } else {
            block.rect.x
        };
        let one_line = block.rect.height <= 1.5 * ch;
        let right = if framed && !(one_line && block.frame.right() - block.rect.right() > 2.0 * cw)
        {
            block.frame.right().max(block.rect.right())
        } else {
            block.rect.right()
        };
        match &block.kind {
            BlockKind::Table(table) => {
                if !table.caption.is_empty() {
                    let caption = vec![Span::new(table.caption.clone(), "bold italic")];
                    let mut rect = block.rect;
                    rect.height = ch;
                    items.push(text_item(
                        block.node,
                        rect,
                        left,
                        right,
                        caption,
                        Vec::new(),
                    ));
                }
                for (r, row) in table.rows.iter().enumerate() {
                    for (c, cell) in row.iter().enumerate() {
                        let base = if cell.header { "bold" } else { "" };
                        let groups = match source {
                            Some(source) => {
                                source::cell_groups(source, block_index, r, c, &cell.runs, marks)
                            }
                            None => vec![source::TextGroup {
                                runs: cell.runs.clone(),
                                owner: None,
                                rect: None,
                            }],
                        };
                        for group in groups {
                            let rect = group.rect.unwrap_or(cell.rect);
                            if rect.bottom() < 0.0 || rect.right() < 0.0 {
                                continue;
                            }
                            let spans = spans(&group.runs, base, page, marks);
                            if spans.iter().all(|s| s.text.trim().is_empty()) {
                                continue;
                            }
                            let mut item = text_item(
                                block.node,
                                rect,
                                rect.x,
                                rect.right(),
                                spans,
                                Vec::new(),
                            );
                            item.row = Some((block.node, r));
                            item.source = group.owner;
                            items.push(item);
                        }
                    }
                }
            }
            BlockKind::Log(index) => {
                let log = &page.logs[*index];
                let (x0, x1) = snap(log.rect.x, log.rect.right());
                let rows = ((log.rect.height / ch).round() as usize).clamp(1, MAX_MEDIA_ROWS);
                items.push(Item {
                    source: None,
                    anchor: None,
                    allocation: None,
                    top: log.rect.y,
                    bottom: log.rect.bottom(),
                    left: log.rect.x,
                    source_x: source_snap(log.rect.x, log.rect.right()),
                    x0,
                    x1,
                    wrap: None,
                    // Reserve height now; source reflow determines the usable width.
                    lines: vec![Line::default(); rows],
                    node: log.node,
                    log: Some(log.node),
                    media: None,
                    one_line: false,
                    row: None,
                    caption: None,
                });
            }
            BlockKind::Media(index)
                if adapt.on(Rule::DecorativeImages)
                    && crate::adapt::decorative(&page.media[*index]) => {}
            BlockKind::Media(index) if crate::indicator::instead(page, *index, adapt) => {}
            BlockKind::Indicator(index) => {
                // A piece of its own, placed as Chrome placed the box: one
                // row (a bar, steps, stars) grows like a control's face;
                // a dial keeps its lines.
                let indicator = &page.indicators[*index];
                if indicator.media.is_some() && !adapt.on(Rule::Indicators) {
                    continue;
                }
                let (i0, i1) = snap(indicator.rect.x, indicator.rect.right());
                let rows = ((indicator.rect.height / ch).round() as usize).max(1);
                let mut lines = crate::indicator::lines(indicator, i1 - i0, rows, (cw, ch));
                if lines.len() == 1 {
                    one_row.push(items.len());
                    indicator_items.push(items.len());
                    let mut item = text_item(
                        indicator.node,
                        indicator.rect,
                        indicator.rect.x,
                        indicator.rect.right(),
                        lines.remove(0).spans,
                        Vec::new(),
                    );
                    item.one_line = true;
                    items.push(item);
                } else {
                    let full = lines.len();
                    items.push(Item {
                        source: None,
                        anchor: None,
                        allocation: None,
                        top: indicator.rect.y,
                        bottom: indicator.rect.y + full as f64 * ch,
                        left: indicator.rect.x,
                        source_x: source_snap(indicator.rect.x, indicator.rect.right()),
                        x0: i0,
                        x1: (i0 + lines.first().map_or(1, Line::width))
                            .min(columns)
                            .max(i0 + 1),
                        wrap: None,
                        lines,
                        node: indicator.node,
                        log: None,
                        media: None,
                        one_line: false,
                        row: None,
                        caption: None,
                    });
                    if (full as f64) < (indicator.rect.height / ch).round() {
                        shrunk.push((items.len() - 1, indicator.rect.height, i0, i1));
                    }
                }
            }
            BlockKind::Media(index)
                if adapt.on(Rule::TextOverPictures)
                    && crate::adapt::picture_of_text(&page.media[*index], ch) =>
            {
                // Its words, where it was, one row tall.
                let media = &page.media[*index];
                let mut rect = media.rect;
                rect.height = ch;
                items.push(text_item(
                    media.node,
                    rect,
                    media.rect.x,
                    media.rect.right(),
                    vec![Span::new(media.alt.clone(), "bold")],
                    Vec::new(),
                ));
                let (t0, t1) = snap(media.rect.x, media.rect.right());
                shrunk.push((items.len() - 1, media.rect.height, t0, t1));
            }
            BlockKind::Media(index) => {
                let media = &page.media[*index];
                let (m0, m1) = snap(media.rect.x, media.rect.right());
                let full = ((media.rect.height / ch).round() as usize).clamp(1, MAX_MEDIA_ROWS);
                let (rows, w) =
                    crate::adapt::picture_cells(media.rect, m1 - m0, ch, adapt, MAX_MEDIA_ROWS);
                let wide = m1;
                let m1 = m0 + w;
                // Captioned, the box keeps its whole width, so a capped
                // (narrowed) picture's caption wraps to the page's width
                // for it, not the picture's.
                let captioned = adapt.on(Rule::Captions)
                    && media.kind == MediaKind::Image
                    && !media.captioned
                    && !media.alt.is_empty()
                    && rows >= CAPTIONED_ROWS;
                let mut lines: Vec<Line> = (0..rows)
                    .map(|r| {
                        // The alt text once: in the box until the picture
                        // comes, unless the caption under it says it.
                        let label = if r == rows / 2 && !captioned {
                            crate::clean::truncate(&media.alt, w)
                        } else {
                            String::new()
                        };
                        let pad = w.saturating_sub(cell_len(&label));
                        Line {
                            spans: vec![
                                Span::new(label, "italic"),
                                Span::new("░".repeat(pad), "bright_black"),
                            ],
                        }
                    })
                    .collect();
                // A picture the page does not caption gets its alt text
                // under it, as the PoC showed it; not a small one (an
                // avatar, an icon), whose alt text is noise.
                if let Some(mark) = play_mark(media, w)
                    && let Some(last) = lines.last_mut()
                {
                    *last = mark;
                }
                let x1 = if captioned { wide.max(m1) } else { m1 };
                let caption =
                    captioned.then(|| vec![Span::new(format!("Image: {}", media.alt), "italic")]);
                items.push(Item {
                    source: None,
                    anchor: None,
                    allocation: None,
                    top: media.rect.y,
                    bottom: media.rect.y + rows as f64 * ch,
                    left: media.rect.x,
                    source_x: source_snap(media.rect.x, media.rect.right()),
                    x0: m0,
                    x1,
                    wrap: None,
                    lines,
                    node: media.node,
                    log: None,
                    media: Some((*index, rows, w)),
                    one_line: false,
                    row: None,
                    caption,
                });
                if rows < full {
                    let (o0, o1) = snap(media.rect.x, media.rect.right());
                    shrunk.push((items.len() - 1, media.rect.height, o0, o1));
                }
            }
            BlockKind::Control(index) => {
                let control = &page.controls[*index];
                let mut first = Vec::new();
                if let Some(label) = marks.hints.get(index) {
                    first.push(Span {
                        text: format!("[{label}]"),
                        style: "bold black on yellow".into(),
                        control: Some(*index),
                        picture: None,
                    });
                }
                let style = if control.disabled { "dim" } else { "reverse" };
                // A control's face is one line: a narrow box gets its
                // compact form (`[Subscribe]`), cut with `…` if still too
                // wide, rather than its name wrapped over rows.
                let (c0, c1) = snap(control.rect.x, control.rect.right());
                let room = (c1 - c0).max(3);
                let mut text = face_in(control, adapt, c1 - c0);
                if cell_len(&text) > room {
                    text = compact_face(&text);
                }
                let span = Span {
                    text,
                    style: style.into(),
                    control: Some(*index),
                    picture: None,
                };
                one_row.push(items.len());
                let mut item = text_item(
                    control.node,
                    control.rect,
                    control.rect.x,
                    control.rect.right(),
                    vec![span],
                    first,
                );
                item.one_line = true;
                items.push(item);
            }
            BlockKind::Code => {
                let (x0, x1) = snap(left, right);
                let code = crate::model::plain(&block.runs);
                let lines = code
                    .split('\n')
                    .flat_map(|row| wrap_chars(&Span::new(row, "cyan"), x1 - x0, &[], &[]))
                    .collect();
                items.push(Item {
                    source: None,
                    anchor: None,
                    allocation: None,
                    top: block.rect.y,
                    bottom: block.rect.bottom(),
                    left,
                    source_x: source_snap(left, right),
                    x0,
                    x1,
                    wrap: None,
                    lines,
                    node: block.node,
                    log: None,
                    media: None,
                    one_line: false,
                    row: None,
                    caption: None,
                });
            }
            kind => {
                let base = match kind {
                    BlockKind::Heading(1) => "bold underline",
                    BlockKind::Heading(_) => "bold",
                    _ if block.quote > 0 => "italic",
                    _ => "",
                };
                let mut first = Vec::new();
                if let BlockKind::ListItem { marker, .. } = kind
                    && !marker.is_empty()
                {
                    first.push(Span::new(format!("{marker} "), "bright_black"));
                }
                if let Some(source) = source {
                    for group in source::text_groups(source, block_index, marks) {
                        let (rect, l, r) = match group.rect {
                            Some(rect) => (rect, rect.x, rect.right()),
                            None => (block.rect, left, right),
                        };
                        if rect.bottom() < 0.0 || rect.right() < 0.0 {
                            continue;
                        }
                        let content = spans(&group.runs, base, page, marks);
                        if content.iter().all(|s| s.text.is_empty()) {
                            continue;
                        }
                        let mut item = text_item(block.node, rect, l, r, content, first.clone());
                        item.source = group.owner;
                        item.anchor = Some(rect.y);
                        items.push(item);
                    }
                } else {
                    items.push(text_item(
                        block.node,
                        block.rect,
                        left,
                        right,
                        spans(&block.runs, base, page, marks),
                        first,
                    ));
                }
            }
        }
        for item in &mut items[first_item..] {
            if let Some(source) = source {
                item.source = item.source.or_else(|| source.owner(block_index));
                item.anchor.get_or_insert(item.top);
                if let Some((media, _, _)) = item.media
                    && page.media[media].kind == MediaKind::Image
                    && let Some(control) = source.relations.media_controls[media]
                {
                    for span in item.lines.iter_mut().flat_map(|l| &mut l.spans) {
                        span.control = Some(control);
                    }
                }
            }
            item.top += dy;
            item.bottom += dy;
        }
        // A short text (one line, at most a third of the picture's width)
        // over a picture is drawn in its cells, not under it.
        if let Some(m) = over
            && items.len() == first_item + 1
            && block.rect.height <= 1.5 * ch
            && block.rect.width <= page.media[m].rect.width / 3.0
            && items[first_item].wrap.is_some()
        {
            let item = items.pop().expect("one item");
            one_row.retain(|&n| n != first_item);
            overlaid.push((m, item));
        }
    }
    // A grid snaps first, on Chrome's own places.
    if let Some(source) = source {
        if part == Part::Flow {
            source::allocate(source, &mut items, columns, (cw, ch), marks);
        }
    } else if adapt.on(Rule::SnapGrids) {
        for grid in &page.grids {
            // Named source panes need room for controls and log labels; their
            // state remains independently keyed by the original browser nodes.
            let inside = |outer: &crate::engine::Rect, inner: &crate::engine::Rect| {
                inner.x >= outer.x
                    && inner.y >= outer.y
                    && inner.right() <= outer.right() + 0.5
                    && inner.bottom() <= outer.bottom() + 0.5
            };
            let paired = grid.items.len() == 2
                && grid.items.iter().any(|r| {
                    page.media
                        .iter()
                        .any(|m| m.kind == MediaKind::Video && inside(r, &m.rect))
                })
                && grid
                    .items
                    .iter()
                    .any(|r| page.logs.iter().any(|l| inside(r, &l.rect)));
            let minimum = if paired {
                32
            } else {
                crate::adapt::MIN_GRID_CELLS
            };
            snap_grid(
                grid,
                &mut items,
                columns,
                (cw, ch),
                &snap,
                minimum,
                |item| {
                    (
                        &mut item.top,
                        &mut item.bottom,
                        &mut item.x0,
                        &mut item.x1,
                        item.left,
                        item.source_x,
                    )
                },
            );
        }
    }
    // What shrank moves what is under it, in its columns, up by the rows
    // it saved; top to bottom, so the moves add up.
    shrunk.sort_by(|a, b| items[a.0].top.total_cmp(&items[b.0].top));
    for (at, before, x0, x1) in shrunk {
        let top = items[at].top;
        let rows = |px: f64| (px / ch).round();
        let saved = (rows(before) - rows(items[at].bottom - top)).max(0.0) * ch;
        if saved <= 0.0 {
            continue;
        }
        let from = top + before - 0.5;
        // Its band: its columns, widened by the boxes that start just
        // under it (the rest of its column, which is wider than it).
        let (mut b0, mut b1) = (x0, x1);
        for item in items.iter() {
            if (item.top - (from + 0.5)).abs() < 2.0 * ch && item.x0 < x1 && x0 < item.x1 {
                b0 = b0.min(item.x0);
                b1 = b1.max(item.x1);
            }
        }
        for (n, item) in items.iter_mut().enumerate() {
            if n != at && item.top >= from && item.x0 < b1 && b0 < item.x1 {
                item.top -= saved;
                item.bottom -= saved;
            }
        }
    }
    // What is beside a collapsed sidebar moves into its columns.
    for (top, bottom, f0, f1) in freed {
        let width = f1 - f0;
        for item in items.iter_mut() {
            if item.x0 >= f1 && item.top < bottom - 0.5 && item.bottom > top + 0.5 {
                item.x0 -= width;
                item.x1 -= width;
            }
        }
    }
    // A control whose face needs more cells than its box pushes the
    // controls after it on its row to the right.
    let mut rows_of: HashMap<i64, Vec<usize>> = HashMap::new();
    for &n in &one_row {
        rows_of
            .entry((items[n].top / ch).round() as i64)
            .or_default()
            .push(n);
    }
    let need_of = |item: &Item| {
        item.wrap.as_ref().map_or(0, |(spans, first)| {
            first
                .iter()
                .chain(spans.iter())
                .map(|s| cell_len(&s.text))
                .sum::<usize>()
        })
    };
    for (_, mut row) in rows_of {
        row.sort_by_key(|&n| items[n].x0);
        let mut push = 0;
        // Where each goes, past the screen's edge if it must.
        let mut at: Vec<(usize, usize)> = Vec::with_capacity(row.len());
        for &n in &row {
            let item = &items[n];
            let (x0, x1) = (item.x0 + push, item.x1 + push);
            let grown = x1.max(x0 + need_of(item));
            push += grown - x1;
            at.push((x0, grown));
        }
        // A run past the right edge (a top bar's icon buttons with long
        // names) moves left into cells nothing beside it claims, its gaps
        // a cell at most; still too wide, its faces shorten with `…`
        // rather than being cut by the screen.
        if at.last().is_some_and(|&(_, x1)| x1 > columns) {
            let (top, bottom, first) = (items[row[0]].top, items[row[0]].bottom, at[0].0);
            let floor = items
                .iter()
                .enumerate()
                .filter(|(m, it)| {
                    !row.contains(m)
                        && it.top < bottom - 0.5
                        && top < it.bottom - 0.5
                        && it.x1 <= first
                })
                .map(|(_, it)| it.x1 + 1)
                .max()
                .unwrap_or(0)
                .min(first);
            let gaps: Vec<usize> = (0..at.len())
                .map(|k| match k {
                    0 => 0,
                    _ => at[k].0.saturating_sub(at[k - 1].1).min(1),
                })
                .collect();
            let mut widths: Vec<usize> = at.iter().map(|(a, b)| b - a).collect();
            let total = |w: &[usize]| w.iter().sum::<usize>() + gaps.iter().sum::<usize>();
            let room = columns.saturating_sub(floor);
            if total(&widths) > room {
                // The widest give way first, none under a few cells.
                let mut cap = widths.iter().copied().max().unwrap_or(0);
                while cap > MIN_FACE
                    && total(&widths.iter().map(|&w| w.min(cap)).collect::<Vec<_>>()) > room
                {
                    cap -= 1;
                }
                for w in &mut widths {
                    *w = (*w).min(cap);
                }
            }
            let mut x = columns.saturating_sub(total(&widths)).max(floor).min(first);
            for (k, &n) in row.iter().enumerate() {
                x += gaps[k];
                let item = &mut items[n];
                item.x0 = x.min(columns - 1);
                item.x1 = (x + widths[k]).clamp(item.x0 + 1, columns);
                x = item.x1;
            }
        } else {
            for (&n, &(x0, x1)) in row.iter().zip(&at) {
                items[n].x0 = x0;
                items[n].x1 = x1;
            }
        }
    }
    // Two boxes side by side that touch in cells, the left one's text
    // filling its box ("16" views and "13y ago"), get a cell between them,
    // or their words run together.
    let touching: Vec<usize> = (0..items.len())
        .filter(|&b| {
            let right = &items[b];
            right.wrap.is_some()
                && right.row.is_none()
                && right.x1 > right.x0 + 1
                && items.iter().any(|left| {
                    left.wrap.is_some()
                        && left.row.is_none()
                        && left.x1 == right.x0
                        && left.bottom - left.top <= 1.5 * ch
                        && (left.top / ch).round() == (right.top / ch).round()
                        && need_of(left) >= left.x1 - left.x0
                })
        })
        .collect();
    for b in touching {
        items[b].x0 += 1;
    }
    // A one-row indicator drawn wider than its box (a rating's stars and
    // number in a box Chrome sized for its picture) moves what follows it
    // on its row right, a cell past its end, so the two do not run
    // together ("4.512,408") or cut it ("4…"); in a grid's cell too.
    for &n in &indicator_items {
        let (top, bottom, x0) = (items[n].top, items[n].bottom, items[n].x0);
        let end = x0 + need_of(&items[n]);
        if end > columns {
            continue;
        }
        for (m, item) in items.iter_mut().enumerate() {
            if m == n
                || item.wrap.is_none()
                || item.x0 <= x0
                || item.x0 > end
                || item.bottom - item.top > 1.5 * ch
                || item.top >= bottom - 0.5
                || top >= item.bottom - 0.5
            {
                continue;
            }
            let shift = end + 1 - item.x0;
            if item.x1 + shift <= columns {
                item.x0 += shift;
                item.x1 += shift;
            } else {
                item.x0 = (end + 1).min(item.x1.saturating_sub(1));
            }
        }
        items[n].x1 = items[n].x1.max(end);
    }
    // Terminal glyphs are not Chrome's: a box whose text needs more cells
    // than it has grows to the right, into cells no box beside it (on the
    // rows it spans) claims, before its text wraps.
    let snapshot: Vec<(f64, f64, usize)> = items.iter().map(|i| (i.top, i.bottom, i.x0)).collect();
    for (n, item) in items.iter_mut().enumerate() {
        let Some((spans, first)) = &item.wrap else {
            continue;
        };
        let needed = item.x0
            + first
                .iter()
                .chain(spans.iter())
                .map(|s| cell_len(&s.text))
                .sum::<usize>();
        if needed > item.x1 {
            let limit = snapshot
                .iter()
                .enumerate()
                .filter(|(m, (top, bottom, x0))| {
                    *m != n
                        && *top < item.bottom - 0.5
                        && item.top < *bottom - 0.5
                        && *x0 >= item.x1
                })
                .map(|(_, (_, _, x0))| x0.saturating_sub(1))
                .min()
                .unwrap_or(columns);
            let limit = item.allocation.map_or(limit, |(_, end)| end.min(limit));
            item.x1 = needed.min(limit).max(item.x1);
        }
    }
    // Still narrower than its longest word (a right-aligned "10." in a
    // narrow column, a box at the page's right edge), a box grows to the
    // left too, into cells no box beside it on its rows claims, so a word
    // is never broken over rows when there is room for it.
    let mut placed: Vec<(f64, f64, usize, usize)> = items
        .iter()
        .map(|i| (i.top, i.bottom, i.x0, i.x1))
        .collect();
    for n in 0..items.len() {
        let Some((spans, first)) = &items[n].wrap else {
            continue;
        };
        let want = min_content(first, spans).min(columns);
        let (top, bottom, x0, x1) = placed[n];
        if x1 - x0 >= want {
            continue;
        }
        let limit = placed
            .iter()
            .enumerate()
            .filter(|(m, (t, b, _, end))| {
                *m != n && *t < bottom - 0.5 && top < *b - 0.5 && *end <= x0
            })
            .map(|(_, (_, _, _, end))| end + 1)
            .max()
            .unwrap_or(0);
        let limit = items[n]
            .allocation
            .map_or(limit, |(start, _)| start.max(limit));
        let start = x1.saturating_sub(want).max(limit).min(x0);
        items[n].x0 = start;
        placed[n].2 = start;
    }
    for item in items.iter_mut() {
        if let Some(node) = item.log {
            let log = page
                .logs
                .iter()
                .find(|log| log.node == node)
                .expect("source log");
            item.lines = log_lines(log, item.x1 - item.x0, item.lines.len(), page, marks);
        }
        if let Some(caption) = &item.caption {
            item.lines
                .extend(wrap(caption, (item.x1 - item.x0).max(1), &[], &[]));
        }
        let Some((spans, first)) = &item.wrap else {
            continue;
        };
        item.lines = wrap(spans, item.x1 - item.x0, first, &[]);
        if item.one_line && item.lines.len() > 1 {
            // A face keeps its closing bracket: `[Sign…]`.
            let width = item.x1 - item.x0;
            let hint: usize = first.iter().map(|s| cell_len(&s.text)).sum();
            item.lines = vec![match spans.as_slice() {
                [face] if face.control.is_some() && width >= hint + 3 => {
                    let mut line = Line {
                        spans: first.clone(),
                    };
                    line.spans.push(Span {
                        text: cut_face(&face.text, width - hint),
                        ..face.clone()
                    });
                    line
                }
                _ => cut_line(first, spans, width),
            }];
        }
    }
    if let Some(source) = source
        && part == Part::Flow
    {
        source::stack_figures(source, &mut items, columns, (cw, ch), marks);
    }
    // Reading order: top, then left. A table's rows keep their order, even
    // where rounding puts a row and the one under it (Hacker News's subtext
    // and the next story) on the same terminal row: each table row sorts at
    // the terminal row of its highest cell, never above the row before it,
    // and then by its index. One key per item, so the order is total.
    let terminal_row = |top: f64| (top / ch).round() as i64;
    let mut table_rows_at: HashMap<crate::engine::NodeId, Vec<(usize, i64)>> = HashMap::new();
    for item in &items {
        if let Some((table, index)) = item.row {
            let rows = table_rows_at.entry(table).or_default();
            let r = terminal_row(item.top);
            match rows.iter_mut().find(|(i, _)| *i == index) {
                Some((_, at)) => *at = (*at).min(r),
                None => rows.push((index, r)),
            }
        }
    }
    let mut row_at: HashMap<(crate::engine::NodeId, usize), i64> = HashMap::new();
    for (table, mut rows) in table_rows_at {
        rows.sort_by_key(|(index, _)| *index);
        let mut floor = i64::MIN;
        for (index, r) in rows {
            floor = floor.max(r);
            row_at.insert((table, index), floor);
        }
    }
    let key = |item: &Item| match item.row {
        Some(row) => (row_at[&row], row.1, item.left),
        None => (terminal_row(item.top), 0, item.left),
    };
    items.sort_by(|a, b| {
        let (ra, ia, la) = key(a);
        let (rb, ib, lb) = key(b);
        ra.cmp(&rb).then(ia.cmp(&ib)).then(la.total_cmp(&lb))
    });
    let mut logs = Vec::new();
    type Placed = (
        f64,
        f64,
        f64,
        usize,
        usize,
        Vec<Line>,
        crate::engine::NodeId,
        Option<(usize, usize, usize)>,
        Option<(crate::engine::NodeId, usize)>,
    );
    // Each table row's cells' columns, so its first cell placed starts the
    // row below every cell's column, and the rest start there too.
    let mut table_rows: HashMap<(crate::engine::NodeId, usize), Vec<(usize, usize)>> =
        HashMap::new();
    for item in &items {
        if let Some(row) = item.row {
            table_rows.entry(row).or_default().push((item.x0, item.x1));
        }
    }
    let mut row_tops: HashMap<(crate::engine::NodeId, usize), usize> = HashMap::new();
    let boxes: Vec<Placed> = items
        .into_iter()
        .map(|i| {
            if let Some(log) = i.log {
                logs.push((log, i.lines.len()));
            }
            (
                i.top,
                i.anchor.unwrap_or(i.top),
                i.left,
                i.x0,
                i.x1,
                i.lines,
                i.node,
                i.media,
                i.row,
            )
        })
        .collect();
    let mut placed_logs = Vec::new();
    let slot_start = slots.len();
    let mut occupied: Vec<(NodeId, Region)> = Vec::new();
    let mut regions = Vec::new();
    let mut inline: Vec<(Region, String)> = Vec::new();
    // Each row's first box: its node, which of its lines, and that line's
    // place on the page.
    let mut owner: Vec<Option<(crate::engine::NodeId, usize, f64)>> = Vec::new();
    let mut skyline = vec![0usize; columns];
    // Each cell holds its grapheme (a character and any zero-width marks
    // after it); an empty string is the second cell of a wide character.
    let mut grid: Vec<Vec<Option<(String, usize)>>> = Vec::new();
    let mut styles: Vec<(String, Option<usize>)> = Vec::new();
    // Rows taken out so far by collapsed gaps: every box after one moves
    // up by them, so what is beside it keeps its place.
    let collapse = adapt.on(Rule::CollapseGaps);
    let mut lifted = 0usize;
    for (top, anchor, _, x0, x1, lines, node, media, table_row) in boxes {
        // A page can place a block millions of pixels down: blank rows
        // past a few screens tell the reader nothing, so the gap is capped
        // and the grid grows with the content, not the coordinates.
        let mut y0 = ((top / ch).round().max(0.0) as usize)
            .saturating_sub(lifted)
            .min(grid.len() + MAX_GAP_ROWS);
        // Rows blank in every column (no box drew on them: the place of
        // an empty slot, a hidden menu, a spacer) collapse to one, none at
        // the top of the page.
        let keep = grid.len() + usize::from(!grid.is_empty());
        if collapse && y0 > keep {
            lifted += y0 - keep;
            y0 = keep;
        }
        let mut y = skyline[x0..x1].iter().copied().max().unwrap_or(0).max(y0);
        // A table row's cells start together: a cell that wrapped onto two
        // rows pushes the next row's cells down, all of them, not only the
        // ones in its column.
        if let Some(key) = table_row {
            let row_y = *row_tops.entry(key).or_insert_with(|| {
                table_rows[&key]
                    .iter()
                    .flat_map(|&(a, b)| skyline[a..b].iter().copied())
                    .max()
                    .unwrap_or(0)
                    .max(y0)
            });
            y = y.max(row_y);
        }
        let rectangle = Region {
            row: y,
            column: x0,
            cols: x1 - x0,
            rows: lines.len(),
            media: usize::MAX,
        };
        occupied.push((node, rectangle));
        if let Some(at) = logs.iter().position(|(log, _)| *log == node) {
            slots.push(LogSlot {
                node,
                region: rectangle,
                part,
            });
            let (log, rows) = logs.remove(at);
            placed_logs.push((log, y, rows));
        }
        if let Some((index, rows, cols)) = media {
            regions.push(Region {
                row: y,
                column: x0,
                cols: cols.min(x1 - x0),
                rows,
                media: index,
            });
        }
        for (k, line) in lines.iter().enumerate() {
            let row = y + k;
            while grid.len() <= row {
                grid.push(vec![None; columns]);
                owner.push(None);
            }
            owner[row].get_or_insert((node, k, anchor + k as f64 * ch));
            let mut x = x0;
            for span in &line.spans {
                let start = x;
                let style = styles
                    .iter()
                    .position(|(s, c)| *s == span.style && *c == span.control)
                    .unwrap_or_else(|| {
                        styles.push((span.style.clone(), span.control));
                        styles.len() - 1
                    });
                for c in span.text.chars() {
                    let w = cell_len(&c.to_string());
                    if w == 0 {
                        // A combining mark joins the grapheme before it, in
                        // that grapheme's cell; with none before it in the
                        // box, it is dropped.
                        let base = (x0..x)
                            .rev()
                            .find(|&at| grid[row][at].as_ref().is_some_and(|(g, _)| !g.is_empty()));
                        if let Some(Some((g, _))) = base.map(|at| &mut grid[row][at]) {
                            g.push(c);
                        }
                        continue;
                    }
                    // Never past the box: what is beside it keeps its cells.
                    if x + w > x1 {
                        break;
                    }
                    grid[row][x] = Some((c.to_string(), style));
                    for extra in 1..w {
                        grid[row][x + extra] = Some((String::new(), style));
                    }
                    x += w;
                }
                if let Some(src) = &span.picture
                    && x > start
                {
                    inline.push((
                        Region {
                            row,
                            column: start,
                            cols: x - start,
                            rows: 1,
                            media: usize::MAX,
                        },
                        src.clone(),
                    ));
                }
            }
        }
        for slot in &mut skyline[x0..x1] {
            *slot = y + lines.len();
        }
    }
    // Texts over a picture, in its cells where they are in its box; at
    // its right edge when they would run past it.
    let mut overlays = Vec::new();
    for (m, item) in overlaid {
        let (Some(region), Some((spans, first))) =
            (regions.iter().find(|r| r.media == m), item.wrap.as_ref())
        else {
            continue;
        };
        let media = &page.media[m];
        let line = cut_line(first, spans, region.cols);
        let width = line.width();
        let row = region.row
            + (((item.top - media.rect.y) / ch).round().max(0.0) as usize)
                .min(region.rows.saturating_sub(1));
        let column = region.column
            + (((item.left - media.rect.x) / cw).round().max(0.0) as usize)
                .min(region.cols.saturating_sub(width));
        let Some(cells) = grid.get_mut(row) else {
            continue;
        };
        let mut x = column;
        for span in &line.spans {
            let style = styles
                .iter()
                .position(|(s, c)| *s == span.style && *c == span.control)
                .unwrap_or_else(|| {
                    styles.push((span.style.clone(), span.control));
                    styles.len() - 1
                });
            for c in span.text.chars() {
                let w = cell_len(&c.to_string());
                if w == 0 || x + w > (region.column + region.cols).min(columns) {
                    continue;
                }
                cells[x] = Some((c.to_string(), style));
                for extra in 1..w {
                    cells[x + extra] = Some((String::new(), style));
                }
                x += w;
            }
        }
        occupied.push((
            i64::MIN,
            Region {
                row,
                column,
                cols: line.width(),
                rows: 1,
                media: usize::MAX,
            },
        ));
        overlays.push((row, column, line));
    }
    // Only exclusive rectangles can be patched; adjacent video/text is
    // safe, but overlapping contributors retain the full composition path.
    let safe: Vec<_> = slots
        .drain(slot_start..)
        .filter(|slot| {
            occupied
                .iter()
                .filter(|(_, r)| rectangles_overlap(*r, slot.region))
                .count()
                == 1
        })
        .collect();
    slots.extend(safe);
    // Keys: a row with no box keeps to the row above it; a key met twice
    // (a table's caption and its cells share the table's node) is told
    // apart by how often it came.
    let (mut keys, mut y) = (
        Vec::with_capacity(owner.len()),
        Vec::with_capacity(owner.len()),
    );
    let mut seen: HashMap<u64, u64> = HashMap::new();
    let mut last = (0i64, 0usize, 0.0f64);
    let mut gap = 0usize;
    for row in &owner {
        let (node, k, at) = match row {
            Some(found) => {
                last = *found;
                gap = 0;
                *found
            }
            None => {
                gap += 1;
                (last.0, last.1, last.2 + gap as f64 * ch)
            }
        };
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&(node, k, gap), &mut hash);
        let key = std::hash::Hasher::finish(&hash);
        let count = seen.entry(key).or_default();
        keys.push(key ^ count.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        *count += 1;
        y.push(at);
    }
    let lines = grid
        .into_iter()
        .map(|row| {
            let mut line = Line::default();
            for cell in row {
                let (c, style) = match cell {
                    Some((g, _)) if g.is_empty() => continue,
                    Some((g, s)) => (g, Some(s)),
                    None => (" ".to_string(), None),
                };
                let (style, control) = style.map(|s| styles[s].clone()).unwrap_or_default();
                match line.spans.last_mut() {
                    Some(last) if last.style == style && last.control == control => {
                        last.text.push_str(&c)
                    }
                    _ => line.spans.push(Span {
                        text: c,
                        style,
                        control,
                        picture: None,
                    }),
                }
            }
            if let Some(last) = line.spans.last_mut()
                && last.style.is_empty()
            {
                let trimmed = last.text.trim_end().len();
                last.text.truncate(trimmed);
            }
            line.spans.retain(|s| !s.text.is_empty());
            line
        })
        .collect();
    let rows = keys
        .iter()
        .enumerate()
        .map(|(row, key)| (*key, row))
        .collect();
    Projection {
        lines,
        keys,
        y,
        rows,
        logs: placed_logs,
        regions,
        inline,
        overlays,
        ..Projection::default()
    }
}

/// [Snap a grid](Rule::SnapGrids): its items get equal cell widths that
/// tile its width, and when a column would be narrower than
/// [`MIN_GRID_CELLS`](crate::adapt::MIN_GRID_CELLS), fewer columns, its
/// items flowing into more rows. Every box inside an item moves with it;
/// what is under the grid moves by the rows it grew or saved. `parts`
/// gives a box's top, bottom, first and end columns, and left edge.
fn snap_grid<T>(
    grid: &crate::model::Grid,
    items: &mut [T],
    columns: usize,
    cell: (f64, f64),
    snap: &dyn Fn(f64, f64) -> (usize, usize),
    minimum: usize,
    parts: impl Fn(
        &mut T,
    ) -> (
        &mut f64,
        &mut f64,
        &mut usize,
        &mut usize,
        f64,
        (usize, usize),
    ),
) {
    let (cw, ch) = cell;
    let (g0, g1) = snap(grid.rect.x, grid.rect.right());
    let width = g1.saturating_sub(g0);
    // Chrome's rows of items: by their tops.
    let mut chrome_rows: Vec<Vec<usize>> = Vec::new();
    for (n, item) in grid.items.iter().enumerate() {
        match chrome_rows.last_mut() {
            Some(row) if (grid.items[row[0]].y - item.y).abs() < ch / 2.0 => row.push(n),
            _ => chrome_rows.push(vec![n]),
        }
    }
    let count = chrome_rows.iter().map(Vec::len).max().unwrap_or(0);
    if count < 2 || width < 2 || columns == 0 {
        return;
    }
    let fit = rich_intuituive::layout::adaptive_columns(
        width.min(u16::MAX as usize) as u16,
        minimum as u16,
        1,
        count,
    );
    let cell = (width + 1) / fit - 1;
    let gap = match chrome_rows.as_slice() {
        [a, b, ..] => {
            let bottom = a
                .iter()
                .map(|&n| grid.items[n].bottom())
                .fold(0.0, f64::max);
            (grid.items[b[0]].y - bottom).max(0.0)
        }
        _ => 0.0,
    };
    // Each item's new place: (dy in pixels, first column, end column).
    let mut places = Vec::with_capacity(grid.items.len());
    let mut top = grid.items[0].y;
    for row in grid.items.chunks(fit) {
        let height = row.iter().map(|r| r.height).fold(0.0, f64::max);
        for (col, rect) in row.iter().enumerate() {
            let x0 = g0 + col * (cell + 1);
            places.push((top - rect.y, x0, (x0 + cell).min(g1)));
        }
        top += height + gap;
    }
    let old_bottom = grid.items.iter().map(|r| r.bottom()).fold(0.0, f64::max);
    let grown = ((top - gap - old_bottom) / ch).round() * ch;
    for item in items.iter_mut() {
        let (item_top, item_bottom, x0, x1, left, source_x) = parts(item);
        let inside = grid.items.iter().position(|r| {
            *item_top >= r.y - 0.5
                && *item_top < r.bottom()
                && left >= r.x - 0.5
                && left < r.right()
        });
        if let Some(n) = inside {
            let (dy, n0, n1) = places[n];
            let o0 = (grid.items[n].x / cw).round().max(0.0) as usize;
            let o1 = (grid.items[n].right() / cw).round().max(0.0) as usize;
            let dy = (dy / ch).round() * ch;
            *item_top += dy;
            *item_bottom += dy;
            // Its place in the item, scaled to the item's new width: an
            // item that narrows keeps what is at its right inside it
            // (Wikipedia's account links, MDN's "Learn more"), rather than
            // each box clamped to the item's last column, one letter wide.
            let (old, new) = ((o1 - o0).max(1), n1.saturating_sub(n0).max(1));
            let scale = |x: usize| n0 + (x.saturating_sub(o0) * new).div_ceil(old).min(new);
            let scale = |x: usize| {
                if new >= old {
                    n0 + x.saturating_sub(o0)
                } else {
                    scale(x)
                }
            };
            let start = scale(source_x.0).min(n1.saturating_sub(1));
            let end = scale(source_x.1).clamp(start + 1, n1.max(start + 1));
            *x0 = start;
            *x1 = end;
        } else if *item_top >= old_bottom - 0.5 && *x0 < g1 && g0 < *x1 && grown != 0.0 {
            *item_top += grown;
            *item_bottom += grown;
        }
    }
}

/// A box's min-content width in cells: its longest word, the first one
/// with the first line's prefix (a hint label, a list marker).
fn min_content(first: &[Span], spans: &[Span]) -> usize {
    let text: String = spans.iter().map(|s| s.text.as_str()).collect();
    let prefix: usize = first.iter().map(|s| cell_len(&s.text)).sum();
    let mut words = text.split_whitespace().map(cell_len);
    let lead = prefix + words.next().unwrap_or(0);
    words.fold(lead, usize::max)
}

/// Labels for `count` link hints: home-row letters, all the same length.
pub fn hint_labels(count: usize) -> Vec<String> {
    const KEYS: &[char] = &[
        'a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l', 'e', 'w', 'r', 'u', 'i', 'o',
    ];
    let mut length = 1;
    while KEYS.len().pow(length as u32) < count {
        length += 1;
    }
    (0..count)
        .map(|mut n| {
            let mut label = vec![' '; length];
            for slot in label.iter_mut().rev() {
                *slot = KEYS[n % KEYS.len()];
                n /= KEYS.len();
            }
            label.into_iter().collect()
        })
        .collect()
}

/// Where a click lands in a media box `width` by `height` CSS pixels drawn
/// in `cols` by `rows` cells: `at` is the pointer in fractional cells from
/// the drawing's top left (a terminal reporting pixels gives the fraction,
/// one reporting cells the cell's centre; see
/// [`Mouse::precise`](rich_intuituive::interact::Mouse::precise)). In CSS
/// pixels from the box's top left, kept inside it (#819).
pub fn point_in(width: f64, height: f64, cols: usize, rows: usize, at: (f32, f32)) -> (f64, f64) {
    let along = |at: f32, cells: usize, size: f64| {
        if cells == 0 || size <= 0.0 {
            return 0.0;
        }
        (f64::from(at) / cells as f64).clamp(0.0, 1.0) * size
    };
    inside(
        0.0,
        0.0,
        width,
        height,
        along(at.0, cols, width),
        along(at.1, rows, height),
    )
}

/// `left + x`, `top + y` kept inside the box from `left`, `top` to
/// `right`, `bottom` (half a pixel in from its edges, so a click on the
/// last pixel is still the box's).
pub fn inside(left: f64, top: f64, right: f64, bottom: f64, x: f64, y: f64) -> (f64, f64) {
    let keep = |from: f64, to: f64, at: f64| {
        if to - from < 1.0 {
            return (from + to) / 2.0;
        }
        (from + at).clamp(from + 0.5, to - 0.5)
    };
    (keep(left, right, x), keep(top, bottom, y))
}

/// Cell rectangle overlap, including rows shared by adjacent boxes.
pub(crate) fn rectangles_overlap(a: Region, b: Region) -> bool {
    a.row < b.row + b.rows
        && b.row < a.row + a.rows
        && a.column < b.column + b.cols
        && b.column < a.column + a.cols
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log_fixture() -> Page {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots/stream-with-log.json");
        crate::model::build(
            &crate::testing::snapshot_file(&path).unwrap(),
            &mut crate::model::Revisions::new(),
        )
    }
    #[test]
    fn fixed_log_wraps_only_entries_contributing_visible_rows() {
        let page = log_fixture();
        let mut log = page.logs[0].clone();
        log.entries = (0..100)
            .map(|key| crate::model::Entry {
                key,
                runs: vec![Run::plain("x")],
            })
            .collect();
        let mut marks = Marks::default();
        for state in [
            LogScroll::Follow,
            LogScroll::Paused(50),
            LogScroll::Paused(999),
        ] {
            marks.logs.insert(log.node, state);
            TEST_LOG_ENTRY_WRAPS.with(|n| n.set(0));
            assert_eq!(log_lines(&log, 20, 3, &page, &marks).len(), 3);
            assert_eq!(TEST_LOG_ENTRY_WRAPS.with(|n| n.get()), 3);
        }
        marks.adapt = Adapt::none();
        log.first_shown = Some(80);
        TEST_LOG_ENTRY_WRAPS.with(|n| n.set(0));
        log_lines(&log, 20, 3, &page, &marks);
        assert_eq!(TEST_LOG_ENTRY_WRAPS.with(|n| n.get()), 3);
        TEST_LOG_ENTRY_WRAPS.with(|n| n.set(0));
        assert!(log_lines(&log, 20, 0, &page, &marks).is_empty());
        assert_eq!(TEST_LOG_ENTRY_WRAPS.with(|n| n.get()), 0);
    }
    #[test]
    fn bounded_log_lines_match_full_wrapping_with_inline_styles_and_unicode() {
        let page = log_fixture();
        let mut log = page.logs[0].clone();
        let template = log
            .entries
            .iter()
            .find(|e| e.runs.iter().any(|r| r.picture.is_some()))
            .unwrap()
            .clone();
        log.entries = (0..60)
            .map(|key| {
                let mut e = template.clone();
                e.key = key;
                let mut run = Run::plain(format!(
                    "row {key} 界 e\u{301} 👩\u{200d}💻\nlongwordabcdefghijklmnopqrstuvwxyz "
                ));
                run.bold = true;
                run.control = Some(0);
                e.runs.insert(0, run);
                e
            })
            .collect();
        for width in [1, 8, 40] {
            for rows in [0, 1, 5, 18] {
                for first in [None, Some(10), Some(999)] {
                    for state in [
                        LogScroll::Follow,
                        LogScroll::Paused(30),
                        LogScroll::Paused(999),
                    ] {
                        for adapted in [false, true] {
                            log.first_shown = first;
                            let mut marks = Marks::default();
                            if !adapted {
                                marks.adapt = Adapt::none();
                            }
                            marks.logs.insert(log.node, state);
                            marks.hints.insert(0, "ab".into());
                            assert_eq!(
                                log_lines(&log, width, rows, &page, &marks),
                                all_entry_log_reference(&log, width, rows, &page, &marks),
                                "{width}/{rows}/{first:?}/{state:?}/{adapted}"
                            );
                        }
                    }
                }
            }
        }
    }
    fn all_entry_log_reference(
        log: &Log,
        width: usize,
        rows: usize,
        page: &Page,
        marks: &Marks,
    ) -> Vec<Line> {
        let mut lines = Vec::new();
        // Each entry's first line.
        let mut starts = Vec::with_capacity(log.entries.len());
        for entry in &log.entries {
            starts.push(lines.len());
            lines.extend(wrap(&spans(&entry.runs, "", page, marks), width, &[], &[]));
        }
        let line_of = |key: u64| log.entries.iter().position(|e| e.key == key);
        let (start, end) = if marks.adapt.on(Rule::LiveTail) {
            let end = match marks.logs.get(&log.node).copied().unwrap_or_default() {
                LogScroll::Follow => lines.len(),
                LogScroll::Paused(key) => match line_of(key) {
                    Some(n) => starts.get(n + 1).copied().unwrap_or(lines.len()),
                    None => rows.min(lines.len()),
                },
            };
            (end.saturating_sub(rows), end)
        } else {
            let start = log.first_shown.and_then(line_of).map_or(0, |n| starts[n]);
            (start, (start + rows).min(lines.len()))
        };
        let mut out: Vec<Line> = lines[start..end].to_vec();
        out.resize(rows, Line::default());
        out
    }

    fn s(text: &str) -> Span {
        Span::new(text, "")
    }

    #[test]
    fn words_wrap_and_long_words_break() {
        let lines = wrap(&[s("the quick brown fox")], 10, &[], &[]);
        assert_eq!(
            lines.iter().map(Line::plain).collect::<Vec<_>>(),
            ["the quick", "brown fox"]
        );
        let lines = wrap(&[s("abcdefghijkl")], 5, &[], &[]);
        assert_eq!(
            lines.iter().map(Line::plain).collect::<Vec<_>>(),
            ["abcde", "fghij", "kl"]
        );
        let lines = wrap(&[s("a\nb")], 10, &[s("> ")], &[s("  ")]);
        assert_eq!(
            lines.iter().map(Line::plain).collect::<Vec<_>>(),
            ["> a", "  b"]
        );
    }

    #[test]
    fn a_match_after_wide_characters_is_highlighted_where_it_is() {
        // A code block's gutter (three bytes, one char) before the match:
        // char offsets taken as bytes cut inside it and panicked.
        let lines = [
            Line {
                spans: vec![Span::new("é", "")],
            },
            Line {
                spans: vec![Span::new(" │ ", "dim"), Span::new("const numbers", "")],
            },
        ];
        let marks = Marks {
            find: Some("numbers".into()),
            ..Marks::default()
        };
        let text = to_text(&lines, &marks, 0);
        let span = text.spans().last().unwrap();
        assert_eq!(&text.plain()[span.start..span.end], "numbers");
    }

    #[test]
    fn text_moved_off_the_page_for_screen_readers_is_not_drawn() {
        // Wikipedia's reference back-links sit 100,000 pixels above the
        // page; they were drawn on its first rows.
        use crate::engine::Rect;
        use crate::model::{Block, Page, Run};
        let block = |text: &str, rect: Rect| Block {
            kind: BlockKind::Paragraph,
            runs: vec![Run::plain(text)],
            quote: 0,
            rect,
            frame: rect,
            node: 1,
            region: None,
            stack: Default::default(),
        };
        let page = Page {
            blocks: vec![
                block("Jump up to:", Rect::new(87.0, -99_829.0, 12.0, 203.0)),
                block("The article", Rect::new(8.0, 40.0, 200.0, 18.0)),
            ],
            ..Page::default()
        };
        let lines: Vec<String> = projection(&page, 80, (8.0, 18.0), &Marks::default())
            .iter()
            .map(Line::plain)
            .collect();
        assert!(!lines.iter().any(|l| l.contains("Jump up")), "{lines:#?}");
        assert!(
            lines.iter().any(|l| l.contains("The article")),
            "{lines:#?}"
        );
    }

    #[test]
    fn find_matches_without_case() {
        assert_eq!(matches("Glow and GLOW", "glow"), [(0, 4), (9, 4)]);
        assert!(matches("abc", "").is_empty());
    }

    #[test]
    fn hint_labels_are_unique_and_even() {
        let labels = hint_labels(40);
        assert_eq!(labels.len(), 40);
        assert!(labels.iter().all(|l| l.len() == 2));
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), 40);
        assert_eq!(hint_labels(3), ["a", "s", "d"]);
    }

    #[test]
    fn a_click_lands_where_the_pointer_was_in_the_box() {
        // A 300 by 150 canvas drawn in 30 by 8 cells.
        assert_eq!(point_in(300.0, 150.0, 30, 8, (15.0, 4.0)), (150.0, 75.0));
        // A cell's centre: column 2, row 0.
        assert_eq!(point_in(300.0, 150.0, 30, 8, (2.5, 0.5)), (25.0, 9.375));
        // Outside the drawing: kept half a pixel inside the box.
        assert_eq!(point_in(300.0, 150.0, 30, 8, (31.0, -1.0)), (299.5, 0.5));
        // An empty drawing or box: its middle, or its corner.
        assert_eq!(point_in(300.0, 150.0, 0, 0, (3.0, 3.0)), (0.5, 0.5));
        assert_eq!(point_in(0.0, 0.0, 30, 8, (3.0, 3.0)), (0.0, 0.0));
        assert_eq!(inside(10.0, 20.0, 110.0, 70.0, 50.0, 25.0), (60.0, 45.0));
    }
}

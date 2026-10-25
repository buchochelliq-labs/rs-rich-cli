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
//!   media as framed placeholders with their alt text; fixed and sticky
//!   layers, paint order and per-region pixels come in phase 0.3.
//!
//! Every string here comes from the [model](crate::model), which has
//! cleaned it already.

use std::collections::HashMap;

use rich::cells::cell_len;
use rich::Text;

use crate::adapt::{Adapt, Rule};
use crate::engine::NodeId;
use crate::model::{BlockKind, Check, Control, Log, MediaKind, Page, Role, Run, Table};

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

/// What the views are asked to mark.
#[derive(Clone, Debug, Default)]
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

/// The `rows` lines of `log` in view at `width` cells: its newest (or,
/// scrolled up, those up to the entry it is paused on) with the
/// [live tail](Rule::LiveTail); without it, from the entry Chrome shows
/// first. Padded to `rows`.
pub fn log_lines(log: &Log, width: usize, rows: usize, page: &Page, marks: &Marks) -> Vec<Line> {
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

/// Lines as rich text, one per row, no wrapping.
pub fn to_text(lines: &[Line], marks: &Marks, first_row: usize) -> Text {
    let mut text = Text::new("").no_wrap(true);
    for (n, line) in lines.iter().enumerate() {
        if n > 0 {
            text.append("\n", None);
        }
        let start = text.plain().chars().count();
        for span in &line.spans {
            let style = (!span.style.is_empty()).then(|| span.style.as_str().into());
            text.append(&span.text, style);
        }
        if let Some(query) = marks.find.as_deref().filter(|q| !q.is_empty()) {
            for (k, (at, len)) in matches(&line.plain(), query).into_iter().enumerate() {
                let current = marks.current == Some((first_row + n, k));
                let style = if current {
                    "bold black on bright_yellow"
                } else {
                    "black on yellow"
                };
                text.stylize(style, start + at, start + at + len);
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
    if let Some(control) = run.control.and_then(|i| page.controls.get(i)) {
        if control.role == Role::Link {
            parts.push(if raw && color.is_some() {
                "underline"
            } else {
                "underline bright_blue"
            });
        }
    }
    parts.join(" ")
}

/// The spans of `runs`, a hint label in front of each control's first run.
fn spans(runs: &[Run], base: &str, page: &Page, marks: &Marks) -> Vec<Span> {
    let mut out = Vec::new();
    let mut last = None;
    for run in runs {
        // A decorative picture in a line is dropped, adapted.
        if run
            .picture
            .as_ref()
            .is_some_and(|p| p.decorative && marks.adapt.on(Rule::DecorativeImages))
        {
            continue;
        }
        if run.control.is_some() && run.control != last {
            if let Some(label) = run.control.and_then(|c| marks.hints.get(&c)) {
                out.push(Span {
                    text: format!("[{label}]"),
                    style: "bold black on yellow".into(),
                    control: run.control,
                    picture: None,
                });
            }
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
            if let Some(last) = line.spans.last_mut() {
                if last.text.ends_with(' ') {
                    last.text.pop();
                }
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
                    1 => "bold underline bright_white",
                    2 => "bold bright_white",
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
        Role::Button | Role::MenuItem | Role::Tab => format!("[ {name} ]"),
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

/// The [`projection`] with its rows' keys and page positions.
pub fn project(page: &Page, columns: u16, cell: (f64, f64), marks: &Marks) -> Projection {
    let columns = columns.max(10) as usize;
    let (cw, ch) = (cell.0.max(1.0), cell.1.max(1.0));
    /// A box to place: where Chrome put it, and what it holds.
    struct Item {
        top: f64,
        bottom: f64,
        left: f64,
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
    }
    let mut items: Vec<Item> = Vec::new();
    let snap = |left: f64, right: f64| -> (usize, usize) {
        // Both edges round, so boxes that touch in Chrome touch here and
        // never share a column.
        let x0 = ((left / cw).round().max(0.0) as usize).min(columns - 1);
        let x1 = ((right / cw).round() as usize).clamp(x0 + 1, columns);
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
            top: rect.y,
            bottom: rect.bottom(),
            left,
            x0,
            x1,
            wrap: Some((spans, first)),
            lines: Vec::new(),
            node,
            log: None,
            media: None,
            one_line: false,
        }
    };
    let adapt = marks.adapt;
    // Landmarks that collapse: on a narrow terminal, with the rule.
    let collapsed: Vec<bool> = page
        .regions
        .iter()
        .map(|r| {
            adapt.on(Rule::CollapseNav)
                && columns < crate::adapt::NARROW_COLUMNS as usize
                && r.collapsible
                && r.rect.height > 1.5 * ch
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
            top: rect.y,
            bottom: rect.y + ch,
            left: rect.x,
            x0,
            x1,
            wrap: None,
            lines: vec![line],
            node: region.node,
            log: None,
            media: None,
            one_line: false,
        });
        if !side {
            shrunk.push((items.len() - 1, region.rect.height, x0, x1));
        }
    }
    for block in &page.blocks {
        if block.region.is_some_and(|r| collapsed[r]) {
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
                for row in &table.rows {
                    for cell in row {
                        let base = if cell.header { "bold" } else { "" };
                        let spans = spans(&cell.runs, base, page, marks);
                        items.push(text_item(
                            block.node,
                            cell.rect,
                            cell.rect.x,
                            cell.rect.right(),
                            spans,
                            Vec::new(),
                        ));
                    }
                }
            }
            BlockKind::Log(index) => {
                let log = &page.logs[*index];
                let (x0, x1) = snap(log.rect.x, log.rect.right());
                let rows = ((log.rect.height / ch).round() as usize).clamp(1, MAX_MEDIA_ROWS);
                items.push(Item {
                    top: log.rect.y,
                    bottom: log.rect.bottom(),
                    left: log.rect.x,
                    x0,
                    x1,
                    wrap: None,
                    lines: log_lines(log, x1 - x0, rows, page, marks),
                    node: log.node,
                    log: Some(log.node),
                    media: None,
                    one_line: false,
                });
            }
            BlockKind::Media(index)
                if adapt.on(Rule::DecorativeImages)
                    && crate::adapt::decorative(&page.media[*index]) => {}
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
                let mut lines: Vec<Line> = (0..rows)
                    .map(|r| {
                        let label = if r == rows / 2 {
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
                if let Some(mark) = play_mark(media, w) {
                    if let Some(last) = lines.last_mut() {
                        *last = mark;
                    }
                }
                // Captioned, the box keeps its whole width, so a capped
                // (narrowed) picture's caption wraps to the page's width
                // for it, not the picture's.
                let captioned = adapt.on(Rule::Captions)
                    && media.kind == MediaKind::Image
                    && !media.captioned
                    && !media.alt.is_empty()
                    && rows >= CAPTIONED_ROWS;
                let x1 = if captioned { wide.max(m1) } else { m1 };
                if captioned {
                    lines.extend(wrap(
                        &[Span::new(format!("Image: {}", media.alt), "italic")],
                        (x1 - m0).max(1),
                        &[],
                        &[],
                    ));
                }
                items.push(Item {
                    top: media.rect.y,
                    bottom: media.rect.y + rows as f64 * ch,
                    left: media.rect.x,
                    x0: m0,
                    x1,
                    wrap: None,
                    lines,
                    node: media.node,
                    log: None,
                    media: Some((*index, rows, w)),
                    one_line: false,
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
                    top: block.rect.y,
                    bottom: block.rect.bottom(),
                    left,
                    x0,
                    x1,
                    wrap: None,
                    lines,
                    node: block.node,
                    log: None,
                    media: None,
                    one_line: false,
                });
            }
            kind => {
                let base = match kind {
                    BlockKind::Heading(1) => "bold underline bright_white",
                    BlockKind::Heading(_) => "bold bright_white",
                    _ if block.quote > 0 => "italic",
                    _ => "",
                };
                let mut first = Vec::new();
                if let BlockKind::ListItem { marker, .. } = kind {
                    if !marker.is_empty() {
                        first.push(Span::new(format!("{marker} "), "bright_black"));
                    }
                }
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
    // A grid snaps first, on Chrome's own places.
    if adapt.on(Rule::SnapGrids) {
        for grid in &page.grids {
            snap_grid(grid, &mut items, columns, ch, &snap, |item| {
                (
                    &mut item.top,
                    &mut item.bottom,
                    &mut item.x0,
                    &mut item.x1,
                    item.left,
                )
            });
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
    for (_, mut row) in rows_of {
        row.sort_by_key(|&n| items[n].x0);
        let mut push = 0;
        for n in row {
            let item = &mut items[n];
            item.x0 = (item.x0 + push).min(columns - 1);
            item.x1 = (item.x1 + push).clamp(item.x0 + 1, columns);
            let need = item.wrap.as_ref().map_or(0, |(spans, first)| {
                first
                    .iter()
                    .chain(spans.iter())
                    .map(|s| cell_len(&s.text))
                    .sum::<usize>()
            });
            if item.x0 + need > item.x1 {
                let grown = (item.x0 + need).min(columns);
                push += grown - item.x1;
                item.x1 = grown;
            }
        }
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
            item.x1 = needed.min(limit).max(item.x1);
        }
        item.lines = wrap(spans, item.x1 - item.x0, first, &[]);
        if item.one_line && item.lines.len() > 1 {
            item.lines = vec![cut_line(first, spans, item.x1 - item.x0)];
        }
    }
    // Reading order: top, then left.
    items.sort_by(|a, b| {
        let ra = (a.top / ch).round() as i64;
        let rb = (b.top / ch).round() as i64;
        ra.cmp(&rb).then(a.left.total_cmp(&b.left))
    });
    let mut logs = Vec::new();
    type Placed = (
        f64,
        f64,
        usize,
        usize,
        Vec<Line>,
        crate::engine::NodeId,
        Option<(usize, usize, usize)>,
    );
    let boxes: Vec<Placed> = items
        .into_iter()
        .map(|i| {
            if let Some(log) = i.log {
                logs.push((log, i.lines.len()));
            }
            (i.top, i.left, i.x0, i.x1, i.lines, i.node, i.media)
        })
        .collect();
    let mut placed_logs = Vec::new();
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
    for (top, _, x0, x1, lines, node, media) in boxes {
        // A page can place a block millions of pixels down: blank rows
        // past a few screens tell the reader nothing, so the gap is capped
        // and the grid grows with the content, not the coordinates.
        let y0 = ((top / ch).round().max(0.0) as usize).min(grid.len() + MAX_GAP_ROWS);
        let y = skyline[x0..x1].iter().copied().max().unwrap_or(0).max(y0);
        if let Some(at) = logs.iter().position(|(log, _)| *log == node) {
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
            owner[row].get_or_insert((node, k, top + k as f64 * ch));
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
                if let Some(src) = &span.picture {
                    if x > start {
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
        }
        for slot in &mut skyline[x0..x1] {
            *slot = y + lines.len();
        }
    }
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
            if let Some(last) = line.spans.last_mut() {
                if last.style.is_empty() {
                    let trimmed = last.text.trim_end().len();
                    last.text.truncate(trimmed);
                }
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
    ch: f64,
    snap: &dyn Fn(f64, f64) -> (usize, usize),
    parts: impl Fn(&mut T) -> (&mut f64, &mut f64, &mut usize, &mut usize, f64),
) {
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
    let min = crate::adapt::MIN_GRID_CELLS;
    let fit = ((width + 1) / (min + 1)).clamp(1, count);
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
        let (item_top, item_bottom, x0, x1, left) = parts(item);
        let inside = grid.items.iter().position(|r| {
            *item_top >= r.y - 0.5
                && *item_top < r.bottom()
                && left >= r.x - 0.5
                && left < r.right()
        });
        if let Some(n) = inside {
            let (dy, n0, n1) = places[n];
            let (o0, _) = snap(grid.items[n].x, grid.items[n].right());
            let dy = (dy / ch).round() * ch;
            *item_top += dy;
            *item_bottom += dy;
            let start = (n0 + x0.saturating_sub(o0)).min(n1.saturating_sub(1));
            let end = (n0 + x1.saturating_sub(o0)).clamp(start + 1, n1.max(start + 1));
            *x0 = start;
            *x1 = end;
        } else if *item_top >= old_bottom - 0.5 && *x0 < g1 && g0 < *x1 && grown != 0.0 {
            *item_top += grown;
            *item_bottom += grown;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}

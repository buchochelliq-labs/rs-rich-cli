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

use crate::model::{BlockKind, Check, Control, MediaKind, Page, Role, Run, Table};

/// A run of text on a line with one style (a rich style definition).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    pub text: String,
    pub style: String,
    /// The control it belongs to.
    pub control: Option<usize>,
}

impl Span {
    pub fn new(text: impl Into<String>, style: impl Into<String>) -> Span {
        Span {
            text: text.into(),
            style: style.into(),
            control: None,
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
fn run_style(run: &Run, base: &str, page: &Page) -> String {
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
    if let Some(control) = run.control.and_then(|i| page.controls.get(i)) {
        if control.role == Role::Link {
            parts.push("underline bright_blue");
        }
    }
    parts.join(" ")
}

/// The spans of `runs`, a hint label in front of each control's first run.
fn spans(runs: &[Run], base: &str, page: &Page, marks: &Marks) -> Vec<Span> {
    let mut out = Vec::new();
    let mut last = None;
    for run in runs {
        if run.control.is_some() && run.control != last {
            if let Some(label) = run.control.and_then(|c| marks.hints.get(&c)) {
                out.push(Span {
                    text: format!("[{label}]"),
                    style: "bold black on yellow".into(),
                    control: run.control,
                });
            }
        }
        last = run.control;
        out.push(Span {
            text: run.text.clone(),
            style: run_style(run, base, page),
            control: run.control,
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
                    && !last.text.is_empty() =>
            {
                last.text.push_str(text)
            }
            _ => line.spans.push(Span {
                text: text.to_string(),
                style: span.style.clone(),
                control: span.control,
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
}

impl Piece {
    /// How many rows it takes.
    pub fn rows(&self) -> usize {
        match self {
            Piece::Text(lines) => lines.len(),
            Piece::Media { rows, .. } => *rows as usize,
            Piece::Controls(controls) => controls.len().max(1),
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
                if media.kind == MediaKind::Image {
                    pieces.push(Piece::Media {
                        index: *index,
                        rows: media_rows(media.rect.height),
                    });
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

/// The page projected by Chrome's geometry into a canvas `columns` wide,
/// with cells of `cell` CSS pixels: the lines, one per row.
pub fn projection(page: &Page, columns: u16, cell: (f64, f64), marks: &Marks) -> Vec<Line> {
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
    }
    let mut items: Vec<Item> = Vec::new();
    let snap = |left: f64, right: f64| -> (usize, usize) {
        // Both edges round, so boxes that touch in Chrome touch here and
        // never share a column.
        let x0 = ((left / cw).round().max(0.0) as usize).min(columns - 1);
        let x1 = ((right / cw).round() as usize).clamp(x0 + 1, columns);
        (x0, x1)
    };
    let text_item =
        |rect: crate::engine::Rect, left: f64, right: f64, spans: Vec<Span>, first: Vec<Span>| {
            let (x0, x1) = snap(left, right);
            Item {
                top: rect.y,
                bottom: rect.bottom(),
                left,
                x0,
                x1,
                wrap: Some((spans, first)),
                lines: Vec::new(),
            }
        };
    for block in &page.blocks {
        let left = block.rect.x.min(if block.frame.width > 0.0 {
            block.frame.x
        } else {
            block.rect.x
        });
        let right = block.frame.right().max(block.rect.right());
        match &block.kind {
            BlockKind::Table(table) => {
                if !table.caption.is_empty() {
                    let caption = vec![Span::new(table.caption.clone(), "bold italic")];
                    let mut rect = block.rect;
                    rect.height = ch;
                    items.push(text_item(rect, left, right, caption, Vec::new()));
                }
                for row in &table.rows {
                    for cell in row {
                        let base = if cell.header { "bold" } else { "" };
                        let spans = spans(&cell.runs, base, page, marks);
                        items.push(text_item(
                            cell.rect,
                            cell.rect.x,
                            cell.rect.right(),
                            spans,
                            Vec::new(),
                        ));
                    }
                }
            }
            BlockKind::Media(index) => {
                let media = &page.media[*index];
                let (m0, m1) = snap(media.rect.x, media.rect.right());
                let rows = ((media.rect.height / ch).round() as usize).max(1);
                let w = m1 - m0;
                let lines = (0..rows)
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
                items.push(Item {
                    top: media.rect.y,
                    bottom: media.rect.bottom(),
                    left: media.rect.x,
                    x0: m0,
                    x1: m1,
                    wrap: None,
                    lines,
                });
            }
            BlockKind::Control(index) => {
                let control = &page.controls[*index];
                let mut first = Vec::new();
                if let Some(label) = marks.hints.get(index) {
                    first.push(Span {
                        text: format!("[{label}]"),
                        style: "bold black on yellow".into(),
                        control: Some(*index),
                    });
                }
                let style = if control.disabled { "dim" } else { "reverse" };
                let span = Span {
                    text: face(control),
                    style: style.into(),
                    control: Some(*index),
                };
                items.push(text_item(
                    control.rect,
                    control.rect.x,
                    control.rect.right(),
                    vec![span],
                    first,
                ));
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
                    block.rect,
                    left,
                    right,
                    spans(&block.runs, base, page, marks),
                    first,
                ));
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
    }
    // Reading order: top, then left.
    items.sort_by(|a, b| {
        let ra = (a.top / ch).round() as i64;
        let rb = (b.top / ch).round() as i64;
        ra.cmp(&rb).then(a.left.total_cmp(&b.left))
    });
    let boxes: Vec<(f64, f64, usize, usize, Vec<Line>)> = items
        .into_iter()
        .map(|i| (i.top, i.left, i.x0, i.x1, i.lines))
        .collect();
    let mut skyline = vec![0usize; columns];
    let mut grid: Vec<Vec<Option<(char, usize)>>> = Vec::new();
    let mut styles: Vec<(String, Option<usize>)> = Vec::new();
    for (top, _, x0, x1, lines) in boxes {
        let y0 = (top / ch).round().max(0.0) as usize;
        let y = skyline[x0..x1].iter().copied().max().unwrap_or(0).max(y0);
        for (k, line) in lines.iter().enumerate() {
            let row = y + k;
            while grid.len() <= row {
                grid.push(vec![None; columns]);
            }
            let mut x = x0;
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
                    // Never past the box: what is beside it keeps its cells.
                    if x + w > x1 {
                        break;
                    }
                    grid[row][x] = Some((c, style));
                    for extra in 1..w {
                        grid[row][x + extra] = Some(('\0', style));
                    }
                    x += w;
                }
            }
        }
        for slot in &mut skyline[x0..x1] {
            *slot = y + lines.len();
        }
    }
    grid.into_iter()
        .map(|row| {
            let mut line = Line::default();
            for cell in row {
                let (c, style) = match cell {
                    Some(('\0', _)) => continue,
                    Some((c, s)) => (c, Some(s)),
                    None => (' ', None),
                };
                let (style, control) = style.map(|s| styles[s].clone()).unwrap_or_default();
                match line.spans.last_mut() {
                    Some(last) if last.style == style && last.control == control => {
                        last.text.push(c)
                    }
                    _ => line.spans.push(Span {
                        text: c.to_string(),
                        style,
                        control,
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
        .collect()
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

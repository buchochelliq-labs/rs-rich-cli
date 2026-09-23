//! Semantic text: renderables as linear, undecorated text for screen readers
//! and logs. Logical order is kept, labels are spelled out, links read as
//! `text <url>`, and decorative layout (borders, guides, padding) is dropped.
//!
//! # Core API gaps
//!
//! Core [`Table`], [`Tree`], [`Panel`] and [`Rule`] keep their contents
//! private (a faithful port has no getters upstream doesn't), so their
//! implementations here recover structure from a plain Unicode render:
//!
//! * `Table` — columns are found from the junctions of the first border line,
//!   or, for borderless boxes, from gaps of two or more blank cells shared by
//!   every row. With a top border, lines above it are the title and lines
//!   below the bottom border the caption; without one, a title reads as a
//!   header. The first bordered block is the header when more blocks follow.
//!   With row separators each block is a row; without them each line is one
//!   (the table is rendered wide so cells do not wrap). A cell containing a
//!   border character, a table with `show_header(false)` and
//!   `show_lines(true)`, or a `Table::grid()` (no padding, so no gap between
//!   columns) can be misread.
//! * `Tree` — depth comes from the four-cell guides (`├── `, `└── `, `│   `).
//! * `Panel` — title from the top border, subtitle from the bottom border,
//!   content with the side borders and common indentation removed.
//! * `Rule` — the title with the rule characters trimmed.
//!
//! Accessors on those core types would make this exact; until then the
//! heuristics are documented and tested here.

use crate::diagnostic::Diagnostic;
use rich::cells::char_cell_width;
use rich::{Console, Panel, Renderable, Rule, StyleType, Table, Text, Theme, Tree};

/// Linear text a screen reader can read.
pub trait AccessibleText {
    /// The content as plain lines, laid out for `width` where layout matters.
    fn accessible_text(&self, width: usize) -> String;
}

fn plain_console(width: usize) -> Console {
    Console::builder()
        .width(width.max(1))
        .height(10_000)
        .no_color(true)
        .color_system(None)
        .force_terminal(false)
        .highlight(false)
        .emoji(false)
        .legacy_windows(false)
        .build()
}

/// Render `value` plainly and split it into lines (trailing spaces kept).
fn plain_lines(value: &dyn Renderable, width: usize) -> Vec<String> {
    let console = plain_console(width);
    let text: String = value
        .rich_render(&console, &console.options())
        .iter()
        .filter(|s| !s.control)
        .map(|s| s.text.as_str())
        .collect();
    let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
    if lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn is_box(c: char) -> bool {
    ('\u{2500}'..='\u{257f}').contains(&c)
}

fn is_decoration(c: char) -> bool {
    is_box(c) || ('\u{2580}'..='\u{259f}').contains(&c)
}

fn is_vertical(c: char) -> bool {
    matches!(c, '│' | '┃' | '║' | '|' | '╎' | '╏' | '┆' | '┇' | '┊' | '┋')
}

fn is_horizontal(c: char) -> bool {
    matches!(
        c,
        '─' | '━'
            | '═'
            | '-'
            | '='
            | '╌'
            | '╍'
            | '┄'
            | '┅'
            | '┈'
            | '┉'
            | '╴'
            | '╶'
            | '╸'
            | '╺'
    )
}

/// A line made only of border characters (at least one).
fn is_structural(line: &str) -> bool {
    let mut border = false;
    for c in line.chars() {
        if is_horizontal(c) || is_box(c) || is_vertical(c) || c == '+' {
            border = true;
        } else if c != ' ' {
            return false;
        }
    }
    border
}

/// `(char, starting cell)` for each char of `line`.
fn cells(line: &str) -> Vec<(char, usize)> {
    let mut at = 0;
    line.chars()
        .map(|c| {
            let start = at;
            at += char_cell_width(c);
            (c, start)
        })
        .collect()
}

fn slice_cells(line: &str, start: usize, end: usize) -> String {
    cells(line)
        .into_iter()
        .filter(|(_, at)| *at >= start && *at < end)
        .map(|(c, _)| c)
        .collect()
}

fn width_of(line: &str) -> usize {
    rich::cells::cell_len(line)
}

/// Collapse whitespace runs and trim.
fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Column `[start, end)` cell ranges for a table render.
fn table_columns(structural: Option<&str>, content: &[&str]) -> Vec<(usize, usize)> {
    let width = content
        .iter()
        .chain(structural.iter())
        .map(|l| width_of(l))
        .max()
        .unwrap_or(0);
    if let Some(line) = structural {
        let junctions: Vec<usize> = cells(line)
            .into_iter()
            .filter(|(c, _)| *c != ' ' && !is_horizontal(*c))
            .map(|(_, at)| at)
            .collect();
        if !junctions.is_empty() {
            let mut bounds = Vec::new();
            let mut previous = 0;
            for j in junctions.iter().copied().chain([width]) {
                if j > previous {
                    bounds.push((previous, j));
                }
                previous = j + 1;
            }
            return bounds;
        }
    }
    // Borderless: a gap is a run of 2+ cells blank (or vertical) in every row.
    let mut blank = vec![true; width];
    for line in content {
        let mut filled = vec![false; width];
        for (c, at) in cells(line) {
            if !(c == ' ' || is_vertical(c)) {
                for cell in filled.iter_mut().skip(at).take(char_cell_width(c).max(1)) {
                    *cell = true;
                }
            }
        }
        for (b, f) in blank.iter_mut().zip(filled) {
            *b &= !f;
        }
    }
    let mut bounds = Vec::new();
    let mut i = 0;
    while i < width {
        if blank[i] {
            i += 1;
            continue;
        }
        let start = i;
        loop {
            while i < width && !blank[i] {
                i += 1;
            }
            let gap = blank[i..].iter().take_while(|b| **b).count();
            if gap == 1 && i + 1 < width {
                i += 1;
                continue;
            }
            break;
        }
        bounds.push((start, i));
    }
    bounds
}

fn row_cells(lines: &[&str], bounds: &[(usize, usize)]) -> Vec<String> {
    bounds
        .iter()
        .map(|(a, b)| {
            let parts: Vec<String> = lines
                .iter()
                .map(|l| {
                    slice_cells(l, *a, *b)
                        .chars()
                        .map(|c| if is_vertical(c) || is_box(c) { ' ' } else { c })
                        .collect::<String>()
                })
                .collect();
            squash(&parts.join(" "))
        })
        .collect()
}

impl AccessibleText for Table {
    fn accessible_text(&self, width: usize) -> String {
        let lines = plain_lines(self, width.max(4096));
        let lines: Vec<&str> = lines.iter().map(|l| l.trim_end()).collect();
        let border = |l: &&str| is_structural(l) || l.trim().is_empty();
        let first = lines.iter().position(border);
        let last = lines.iter().rposition(border);
        let (title, body, caption) = match (first, last) {
            (Some(f), Some(l)) => (&lines[..f], &lines[f..=l], &lines[l + 1..]),
            _ => (&lines[..0], &lines[..], &lines[..0]),
        };
        // Blocks of content lines separated by border lines.
        let mut blocks: Vec<Vec<&str>> = vec![Vec::new()];
        for line in body {
            if border(line) {
                if !blocks.last().is_some_and(Vec::is_empty) {
                    blocks.push(Vec::new());
                }
            } else {
                blocks.last_mut().expect("one block").push(line);
            }
        }
        blocks.retain(|b| !b.is_empty());
        // The border line with the most junctions: ASCII boxes draw no
        // junctions into their top edge.
        let junctions = |l: &str| {
            l.chars()
                .filter(|c| *c != ' ' && !is_horizontal(*c))
                .count()
        };
        let structural = body
            .iter()
            .filter(|l| is_structural(l) && junctions(l) > 0)
            .fold(None::<&str>, |best, l| match best {
                Some(b) if junctions(b) >= junctions(l) => Some(b),
                _ => Some(l),
            });
        let content: Vec<&str> = blocks.iter().flatten().copied().collect();
        let bounds = table_columns(structural, &content);

        let (headers, rows): (Vec<String>, Vec<Vec<String>>) = if blocks.len() >= 2 {
            let headers = row_cells(&blocks[0], &bounds);
            let rows = if blocks.len() > 2 {
                blocks[1..].iter().map(|b| row_cells(b, &bounds)).collect()
            } else {
                blocks[1].iter().map(|l| row_cells(&[l], &bounds)).collect()
            };
            (headers, rows)
        } else {
            let rows = content.iter().map(|l| row_cells(&[l], &bounds)).collect();
            (Vec::new(), rows)
        };

        let mut out: Vec<String> = title
            .iter()
            .map(|l| squash(l))
            .filter(|l| !l.is_empty())
            .collect();
        let count = rows.len();
        out.push(format!(
            "Table with {count} row{}{}",
            if count == 1 { "" } else { "s" },
            if headers.is_empty() {
                String::new()
            } else {
                format!(", columns: {}", headers.join(", "))
            }
        ));
        for (index, row) in rows.iter().enumerate() {
            let cells: Vec<String> = row
                .iter()
                .enumerate()
                .filter(|(_, v)| !v.is_empty())
                .map(|(i, v)| match headers.get(i).filter(|h| !h.is_empty()) {
                    Some(h) => format!("{h}: {v}"),
                    None => v.clone(),
                })
                .collect();
            out.push(format!("Row {}: {}", index + 1, cells.join("; ")));
        }
        out.extend(caption.iter().map(|l| squash(l)).filter(|l| !l.is_empty()));
        out.join("\n")
    }
}

impl AccessibleText for Tree {
    fn accessible_text(&self, width: usize) -> String {
        const GUIDES: [&str; 4] = ["    ", "│   ", "├── ", "└── "];
        let lines = plain_lines(self, width.max(4096));
        let mut items: Vec<(usize, String)> = Vec::new();
        for line in lines.iter().map(|l| l.trim_end()) {
            let mut rest = line;
            let mut depth = 0;
            let mut branch = false;
            while let Some(g) = GUIDES.iter().find(|g| rest.starts_with(**g)) {
                rest = &rest[g.len()..];
                depth += 1;
                branch = g.contains('─');
                if branch {
                    break;
                }
            }
            match items.last_mut() {
                Some(last) if !branch => {
                    last.1.push(' ');
                    last.1.push_str(rest.trim());
                }
                _ => items.push((depth, rest.trim().to_owned())),
            }
        }
        items
            .into_iter()
            .map(|(depth, label)| format!("{}- {}", "  ".repeat(depth), label))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

impl AccessibleText for Panel {
    fn accessible_text(&self, width: usize) -> String {
        let lines = plain_lines(self, width);
        if lines.len() < 2 {
            return clean_lines(&lines, false);
        }
        let border_text = |l: &str| {
            squash(
                &l.chars()
                    .map(|c| if is_box(c) { ' ' } else { c })
                    .collect::<String>(),
            )
        };
        let title = border_text(&lines[0]);
        let subtitle = border_text(&lines[lines.len() - 1]);
        let inner: Vec<String> = lines[1..lines.len() - 1]
            .iter()
            .map(|l| {
                let w = width_of(l);
                slice_cells(l, 1, w.saturating_sub(1)).trim_end().to_owned()
            })
            .collect();
        let mut out = Vec::new();
        if !title.is_empty() {
            out.push(title);
        }
        let content = clean_lines(&inner, true);
        if !content.is_empty() {
            out.push(content);
        }
        if !subtitle.is_empty() {
            out.push(subtitle);
        }
        out.join("\n")
    }
}

impl AccessibleText for Rule {
    fn accessible_text(&self, width: usize) -> String {
        let lines = plain_lines(self, width.max(20));
        let text = lines.join(" ");
        squash(
            text.trim_matches(|c: char| {
                is_decoration(c) || c.is_whitespace() || c == '-' || c == '='
            }),
        )
    }
}

fn span_link(style: &StyleType, theme: &Theme) -> Option<String> {
    match style {
        StyleType::Style(s) => s.link().map(str::to_owned),
        StyleType::Name(_) => theme.get_style_or_null(style).link().map(str::to_owned),
    }
}

impl AccessibleText for Text {
    fn accessible_text(&self, _width: usize) -> String {
        let theme = Theme::default_shared();
        let plain = self.plain();
        // Each link is read once, after the last character of its run.
        let mut links: Vec<(usize, usize, String)> = self
            .spans()
            .iter()
            .filter_map(|s| {
                span_link(&s.style, theme).map(|url| (s.start, s.end.min(plain.len()), url))
            })
            .collect();
        links.sort_by_key(|(start, end, _)| (*end, *start));
        links.dedup_by(|a, b| a.1 == b.1 && a.2 == b.2);
        let mut out = String::new();
        let mut at = 0;
        for (_, end, url) in links {
            if end < at || !plain.is_char_boundary(end) {
                continue;
            }
            out.push_str(&plain[at..end]);
            out.push_str(&format!(" <{url}>"));
            at = end;
        }
        out.push_str(&plain[at..]);
        out
    }
}

/// Markup, read as [`Text`]; unparsable markup reads as written.
impl AccessibleText for str {
    fn accessible_text(&self, width: usize) -> String {
        match Text::from_markup(self) {
            Ok(text) => text.accessible_text(width),
            Err(_) => self.to_owned(),
        }
    }
}

impl AccessibleText for Diagnostic {
    fn accessible_text(&self, _width: usize) -> String {
        let mut out = Vec::new();
        let code = self
            .get_code()
            .map(|c| format!("[{c}]"))
            .unwrap_or_default();
        match self.get_level() {
            Some(level) => out.push(format!("{}{code}: {}", level.name(), self.message())),
            None if code.is_empty() => out.push(self.message().to_owned()),
            None => out.push(format!("{code} {}", self.message())),
        }
        if let Some(url) = self.get_code_url() {
            out.push(format!("documentation: <{url}>"));
        }
        if let Some(l) = self.get_location() {
            let mut at = l.path.clone();
            if let Some(line) = l.line {
                at.push_str(&format!(":{line}"));
                if let Some(column) = l.column {
                    at.push_str(&format!(":{column}"));
                }
            }
            out.push(format!("location: {at}"));
        }
        out.extend(self.causes().iter().map(|c| format!("caused by: {c}")));
        out.extend(self.labels().iter().map(|l| format!("label: {l}")));
        out.extend(self.notes().iter().map(|n| format!("note: {n}")));
        out.extend(self.help_messages().iter().map(|h| format!("help: {h}")));
        out.extend(
            self.suggestions()
                .iter()
                .map(|s| format!("suggestion: {}", s.message())),
        );
        out.join("\n")
    }
}

/// Drop decoration from rendered lines: border lines (any horizontal border
/// character) vanish, other decoration characters become spaces, internal
/// whitespace runs collapse, and blank lines (including rows that are only
/// side borders) collapse to one. With `keep_indent`, the indentation common
/// to every line is removed but relative indentation stays.
fn clean_lines(lines: &[String], keep_indent: bool) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in lines {
        let stripped: String = line
            .chars()
            .map(|c| if is_decoration(c) { ' ' } else { c })
            .collect();
        let body = squash(&stripped);
        if body.is_empty() {
            let border = line.chars().any(|c| is_box(c) && is_horizontal(c));
            if !border && out.last().is_some_and(|l| !l.is_empty()) {
                out.push(String::new());
            }
            continue;
        }
        let indent = if keep_indent {
            stripped.len() - stripped.trim_start().len()
        } else {
            0
        };
        out.push(format!("{}{body}", " ".repeat(indent)));
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    let common = out
        .iter()
        .filter(|l| !l.is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    out.iter()
        .map(|l| l.get(common..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Any renderable as semantic text: rendered plainly at `width`, then
/// decoration dropped (see [`AccessibleText`] for the typed versions, which
/// keep more structure).
pub fn semantic_text(value: &dyn Renderable, width: usize) -> String {
    clean_lines(&plain_lines(value, width), false)
}

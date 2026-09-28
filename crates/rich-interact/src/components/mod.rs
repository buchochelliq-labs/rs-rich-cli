//! Ready-made components (0.0.13 workstream 3), built from the crate's
//! primitives: [`Item`](crate::Item), [`Viewport`](crate::Viewport) and the
//! [fuzzy](crate::fuzzy) matcher. Each works under both drivers, degrades to
//! a line-based prompt without a terminal, and is styled by one [`Theme`].

mod asset;
mod color;
mod confirm;
mod file;
mod form;
mod input;
mod pager;
mod select;
mod textarea;
mod views;

pub use asset::{emoji, AssetKind, AssetPicker, BOX_STYLES};
pub use color::{ColorFormat, ColorPicker};
pub use confirm::{Choice, Confirm};
pub use file::{display_name, display_path, FileMode, FilePicker};
pub use form::{Answers, Form, Value};
pub use input::{Input, Provider, Suggestion};
pub use pager::Pager;
pub use select::{MultiSelect, PreviewLayout, Select};
pub use textarea::TextArea;
pub use views::{TableSelect, TreeSelect};

use rich::{Segment, Style};

/// The styles and symbols every component draws with.
#[derive(Clone, Debug)]
pub struct Theme {
    /// The mark before a question (`?`).
    pub question: String,
    pub question_style: Style,
    pub prompt: Style,
    /// The focused row's pointer (`❯`).
    pub pointer: String,
    pub pointer_style: Style,
    pub focused: Style,
    /// Characters a filter matched.
    pub matched: Style,
    pub checked: String,
    pub unchecked: String,
    pub checked_style: Style,
    /// Hints, counts and descriptions.
    pub hint: Style,
    pub error: Style,
    /// A finished component's answer.
    pub answer: Style,
    pub border: Style,
}

fn style(definition: &str) -> Style {
    Style::parse(definition).expect("a built-in style")
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            question: "?".into(),
            question_style: style("bold green"),
            prompt: style("bold"),
            pointer: "❯".into(),
            pointer_style: style("bold magenta"),
            focused: style("bold"),
            matched: style("bold magenta"),
            checked: "◉".into(),
            unchecked: "○".into(),
            checked_style: style("green"),
            hint: style("dim"),
            error: style("red"),
            answer: style("cyan"),
            border: style("dim"),
        }
    }
}

pub(crate) fn text(text: impl Into<String>, style: &Style) -> Segment {
    Segment::new(text, Some(style.clone()))
}

pub(crate) fn plain(text: impl Into<String>) -> Segment {
    Segment::new(text, None)
}

/// The width of a line in cells.
pub(crate) fn width(line: &[Segment]) -> usize {
    line.iter().map(Segment::cell_length).sum()
}

/// Crop a line to `columns` cells.
pub(crate) fn fit(line: Vec<Segment>, columns: usize) -> Vec<Segment> {
    if width(&line) > columns {
        Segment::adjust_line_length(&line, columns, None)
    } else {
        line
    }
}

/// Crop or pad a line to exactly `columns` cells.
pub(crate) fn pad(line: Vec<Segment>, columns: usize) -> Vec<Segment> {
    Segment::adjust_line_length(&line, columns, None)
}

/// `label` with the characters at `positions` in `matched`, the rest in
/// `base`.
pub(crate) fn highlight(
    label: &str,
    positions: &[usize],
    base: Option<&Style>,
    matched: &Style,
) -> Vec<Segment> {
    let hit = match base {
        Some(base) => base.combine(matched),
        None => matched.clone(),
    };
    let mut segments: Vec<Segment> = Vec::new();
    let mut run = String::new();
    let mut run_hit = false;
    let mut next = positions.iter().peekable();
    for (index, c) in label.chars().enumerate() {
        let is_hit = next.peek() == Some(&&index);
        if is_hit {
            next.next();
        }
        if is_hit != run_hit && !run.is_empty() {
            let style = if run_hit {
                Some(hit.clone())
            } else {
                base.cloned()
            };
            segments.push(Segment::new(std::mem::take(&mut run), style));
        }
        run_hit = is_hit;
        run.push(c);
    }
    if !run.is_empty() {
        let style = if run_hit { Some(hit) } else { base.cloned() };
        segments.push(Segment::new(run, style));
    }
    segments
}

/// Pasted text for a one-line field: line breaks become `newline`, a tab a
/// space, and other terminal controls (C0, DEL, C1) are dropped, so a paste
/// cannot carry an escape sequence into the answer or onto the screen.
pub(crate) fn pasted(text: &str, newline: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\r' => out.push_str(newline),
            '\t' => out.push(' '),
            '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {}
            _ => out.push(c),
        }
    }
    out
}

/// `text` with its terminal controls as the characters a view paints them
/// as, one for one, so cells line up (a table's columns) before painting.
pub(crate) fn shown(text: &str) -> String {
    text.chars()
        .map(|c| crate::paint::visible(c).unwrap_or(c))
        .collect()
}

/// The question line every component starts with: `? prompt › `.
pub(crate) fn question(theme: &Theme, prompt: &str) -> Vec<Segment> {
    vec![
        text(theme.question.clone(), &theme.question_style),
        plain(" "),
        text(prompt, &theme.prompt),
        plain(" "),
        text("›", &theme.hint),
        plain(" "),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_matched_characters() {
        let matched = Style::parse("bold").unwrap();
        let segments = highlight("main.rs", &[0, 1, 5], None, &matched);
        let texts: Vec<(&str, bool)> = segments
            .iter()
            .map(|s| (s.text.as_str(), s.style.is_some()))
            .collect();
        assert_eq!(
            texts,
            [("ma", true), ("in.", false), ("r", true), ("s", false)]
        );
    }

    #[test]
    fn pastes_drop_terminal_controls() {
        assert_eq!(
            pasted("X\x1bcY\u{9b}2JZ\x07\x08W\tV\r\nU", " "),
            "XcY2JZW V  U"
        );
        assert_eq!(pasted("a\nb", ""), "ab");
    }

    #[test]
    fn fits_and_pads_lines() {
        let line = vec![plain("hello "), plain("world")];
        assert_eq!(width(&fit(line.clone(), 7)), 7);
        assert_eq!(width(&fit(line.clone(), 20)), 11);
        assert_eq!(width(&pad(line, 20)), 20);
    }
}

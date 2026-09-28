//! A multi-line editor (#493, `rich write`).
//!
//! Enter starts a new line; Ctrl+D (or the key the caller chooses) submits
//! and Escape cancels. Lines longer than the space wrap, and the text
//! scrolls to keep the caret in view. The arrows, Home/End, Ctrl+A/E,
//! Ctrl+U/K/W, Backspace and Delete edit as in [`Input`](crate::Input), the
//! caret moving by grapheme cluster; Up and Down move between lines. A
//! character limit counts line breaks too. Pasted text keeps its line
//! breaks; other terminal controls are dropped.

use std::cell::Cell;

use rich::cells::{cell_len, split_graphemes};
use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::event::{Event, Key, KeyCode};
use crate::policy::{LineIo, NotInteractive};

/// One row on screen: a piece of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    line: usize,
    /// Byte range of the line's text.
    start: usize,
    end: usize,
    /// Whether it is the line's first row (the one numbered).
    first: bool,
}

/// Multi-line text. Returns the text, lines joined with `\n`.
pub struct TextArea {
    prompt: String,
    lines: Vec<String>,
    /// The caret: a line, and a byte offset into it on a grapheme boundary.
    line: usize,
    at: usize,
    placeholder: Option<String>,
    limit: Option<usize>,
    submit: Key,
    height: usize,
    line_numbers: bool,
    theme: Theme,
    /// The first row shown. Moved in `handle`, which knows the width, and
    /// read in `render`.
    top: Cell<usize>,
    answer: Option<Option<String>>,
}

impl TextArea {
    pub fn new(prompt: impl Into<String>) -> TextArea {
        TextArea {
            prompt: prompt.into(),
            lines: vec![String::new()],
            line: 0,
            at: 0,
            placeholder: None,
            limit: None,
            submit: Key::ctrl('d'),
            height: 5,
            line_numbers: false,
            theme: Theme::default(),
            top: Cell::new(0),
            answer: None,
        }
    }

    /// Start with this text, the caret at its end.
    pub fn value(mut self, value: impl AsRef<str>) -> Self {
        let mut value = clean(value.as_ref());
        if let Some(limit) = self.limit {
            value = value.chars().take(limit).collect();
        }
        self.lines = value.split('\n').map(str::to_string).collect();
        self.line = self.lines.len() - 1;
        self.at = self.lines[self.line].len();
        self
    }

    /// Dim text shown while the area is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// At most `chars` characters, line breaks included. Text already
    /// there beyond it is cut.
    pub fn char_limit(mut self, chars: usize) -> Self {
        self.limit = Some(chars);
        let text: String = self.text().chars().take(chars).collect();
        self.value(text)
    }

    /// The key that submits (default Ctrl+D).
    pub fn submit_key(mut self, key: Key) -> Self {
        self.submit = key;
        self
    }

    /// Rows of text shown (default 5).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    /// Number the lines.
    pub fn line_numbers(mut self, on: bool) -> Self {
        self.line_numbers = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// The text so far.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// The caret: (line, character column).
    pub fn caret(&self) -> (usize, usize) {
        (self.line, self.lines[self.line][..self.at].chars().count())
    }

    fn count(&self) -> usize {
        self.lines
            .iter()
            .map(|line| line.chars().count())
            .sum::<usize>()
            + self.lines.len()
            - 1
    }

    fn room(&self) -> usize {
        self.limit
            .map_or(usize::MAX, |limit| limit.saturating_sub(self.count()))
    }

    /// Byte offsets where the caret may stop in `line`: grapheme starts and
    /// the end.
    fn stops(line: &str) -> Vec<usize> {
        let (spans, _) = split_graphemes(line);
        let mut stops: Vec<usize> = spans.iter().map(|(start, _, _)| *start).collect();
        stops.push(line.len());
        stops
    }

    fn insert(&mut self, text: &str) {
        let text: String = text.chars().take(self.room()).collect();
        if text.is_empty() {
            return;
        }
        let rest = self.lines[self.line].split_off(self.at);
        let mut pieces = text.split('\n');
        let first = pieces.next().unwrap_or("");
        self.lines[self.line].push_str(first);
        let mut line = self.line;
        for piece in pieces {
            line += 1;
            self.lines.insert(line, piece.to_string());
        }
        self.line = line;
        self.at = self.lines[line].len();
        self.lines[line].push_str(&rest);
    }

    fn backspace(&mut self) {
        if self.at == 0 {
            if self.line > 0 {
                let line = self.lines.remove(self.line);
                self.line -= 1;
                self.at = self.lines[self.line].len();
                self.lines[self.line].push_str(&line);
            }
            return;
        }
        let stops = Self::stops(&self.lines[self.line]);
        let previous = stops
            .iter()
            .rev()
            .copied()
            .find(|&s| s < self.at)
            .unwrap_or(0);
        self.lines[self.line].replace_range(previous..self.at, "");
        self.at = previous;
    }

    fn delete(&mut self) {
        let length = self.lines[self.line].len();
        if self.at == length {
            if self.line + 1 < self.lines.len() {
                let next = self.lines.remove(self.line + 1);
                self.lines[self.line].push_str(&next);
            }
            return;
        }
        let stops = Self::stops(&self.lines[self.line]);
        let next = stops
            .iter()
            .copied()
            .find(|&s| s > self.at)
            .unwrap_or(length);
        self.lines[self.line].replace_range(self.at..next, "");
    }

    fn left(&mut self) {
        if self.at == 0 {
            if self.line > 0 {
                self.line -= 1;
                self.at = self.lines[self.line].len();
            }
            return;
        }
        let stops = Self::stops(&self.lines[self.line]);
        self.at = stops
            .iter()
            .rev()
            .copied()
            .find(|&s| s < self.at)
            .unwrap_or(0);
    }

    fn right(&mut self) {
        let length = self.lines[self.line].len();
        if self.at == length {
            if self.line + 1 < self.lines.len() {
                self.line += 1;
                self.at = 0;
            }
            return;
        }
        let stops = Self::stops(&self.lines[self.line]);
        self.at = stops
            .iter()
            .copied()
            .find(|&s| s > self.at)
            .unwrap_or(length);
    }

    /// Move `delta` lines, keeping the column in cells as near as it goes.
    fn vertical(&mut self, delta: isize) {
        let target = self
            .line
            .saturating_add_signed(delta)
            .min(self.lines.len() - 1);
        if target == self.line {
            return;
        }
        let column = cell_len(&self.lines[self.line][..self.at]);
        self.line = target;
        let line = &self.lines[target];
        self.at = Self::stops(line)
            .into_iter()
            .take_while(|&stop| cell_len(&line[..stop]) <= column)
            .last()
            .unwrap_or(0);
    }

    fn kill_word(&mut self) {
        let line = &self.lines[self.line];
        let before = &line[..self.at];
        let trimmed = before.trim_end_matches(' ');
        let start = trimmed.rfind(' ').map_or(0, |space| space + 1);
        self.lines[self.line].replace_range(start..self.at, "");
        self.at = start;
    }

    fn gutter(&self) -> usize {
        if self.line_numbers {
            self.lines.len().to_string().len() + 3
        } else {
            2
        }
    }

    /// The rows at `width`, wrapped one cell short so the caret fits after
    /// the last character.
    fn rows(&self, width: usize) -> Vec<Row> {
        let room = width.saturating_sub(self.gutter() + 1).max(1);
        let mut rows = Vec::new();
        for (index, line) in self.lines.iter().enumerate() {
            let (spans, _) = split_graphemes(line);
            let mut start = 0;
            let mut cells = 0;
            let mut first = true;
            for (at, _, width) in spans {
                if cells + width > room && at > start {
                    rows.push(Row {
                        line: index,
                        start,
                        end: at,
                        first,
                    });
                    first = false;
                    start = at;
                    cells = 0;
                }
                cells += width;
            }
            rows.push(Row {
                line: index,
                start,
                end: line.len(),
                first,
            });
        }
        rows
    }

    /// The caret's row and column among `rows`.
    fn caret_at(&self, rows: &[Row]) -> (usize, usize) {
        for (index, row) in rows.iter().enumerate() {
            let next_same_line = rows
                .get(index + 1)
                .is_some_and(|next| next.line == row.line);
            let within = self.at >= row.start && (self.at < row.end || !next_same_line);
            if row.line == self.line && within {
                let line = &self.lines[row.line];
                return (index, cell_len(&line[row.start..self.at]));
            }
        }
        (0, 0)
    }

    /// Scroll so the caret is shown.
    fn follow(&self, width: usize) {
        let rows = self.rows(width);
        let (row, _) = self.caret_at(&rows);
        let top = self.top.get();
        if row < top {
            self.top.set(row);
        } else if row >= top + self.height {
            self.top.set(row + 1 - self.height);
        }
    }
}

/// Text for the area: `\r\n` and `\r` as `\n`, a tab as four spaces, and
/// other terminal controls (C0, DEL, C1) dropped.
fn clean(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {}
            _ => out.push(c),
        }
    }
    out
}

impl Component for TextArea {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        if let Event::Paste(pasted) = event {
            self.insert(&clean(pasted));
            self.follow(context.width);
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            if let Event::Resize { .. } = event {
                self.follow(context.width);
            }
            return Flow::Continue;
        };
        if key == self.submit {
            let text = self.text();
            self.answer = Some(Some(text.clone()));
            return Flow::Done(text);
        }
        let ctrl = key.modifiers.ctrl;
        match key.code {
            KeyCode::Escape => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            KeyCode::Enter => self.insert("\n"),
            KeyCode::Tab => self.insert("    "),
            KeyCode::Backspace => self.backspace(),
            KeyCode::Delete => self.delete(),
            KeyCode::Left => self.left(),
            KeyCode::Right => self.right(),
            KeyCode::Up => self.vertical(-1),
            KeyCode::Down => self.vertical(1),
            KeyCode::PageUp => self.vertical(-(self.height as isize)),
            KeyCode::PageDown => self.vertical(self.height as isize),
            KeyCode::Home if ctrl => (self.line, self.at) = (0, 0),
            KeyCode::End if ctrl => {
                self.line = self.lines.len() - 1;
                self.at = self.lines[self.line].len();
            }
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.at = self.lines[self.line].len(),
            KeyCode::Char('a') if ctrl => self.at = 0,
            KeyCode::Char('e') if ctrl => self.at = self.lines[self.line].len(),
            KeyCode::Char('u') if ctrl => {
                self.lines[self.line].replace_range(..self.at, "");
                self.at = 0;
            }
            KeyCode::Char('k') if ctrl => {
                let at = self.at;
                self.lines[self.line].truncate(at);
            }
            KeyCode::Char('w') if ctrl => self.kill_word(),
            KeyCode::Char(c) if !ctrl && !key.modifiers.alt => {
                let mut buffer = [0; 4];
                self.insert(c.encode_utf8(&mut buffer));
            }
            _ => {}
        }
        self.follow(context.width);
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            header.push(match answer {
                Some(answer) => {
                    let mut lines = answer.lines();
                    let first = lines.next().unwrap_or("").to_string();
                    let more = lines.count();
                    if more > 0 {
                        text(format!("{first} … (+{more} lines)"), &theme.answer)
                    } else {
                        text(first, &theme.answer)
                    }
                }
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(header, width)]);
        }
        let mut lines = vec![fit(header, width)];
        let rows = self.rows(width);
        let (caret_row, caret_column) = self.caret_at(&rows);
        let top = self.top.get().min(rows.len().saturating_sub(1));
        let gutter = self.gutter();
        let empty = self.lines.len() == 1 && self.lines[0].is_empty();
        for index in top..top + self.height {
            let mut line: Vec<Segment> = Vec::new();
            match rows.get(index) {
                Some(row) => {
                    let number = if self.line_numbers && row.first {
                        format!("{:>w$} │ ", row.line + 1, w = gutter - 3)
                    } else if self.line_numbers {
                        format!("{} │ ", " ".repeat(gutter - 3))
                    } else {
                        "│ ".to_string()
                    };
                    line.push(text(number, &theme.border));
                    if empty {
                        if let Some(placeholder) = &self.placeholder {
                            line.push(text(placeholder.clone(), &theme.hint));
                        }
                    } else {
                        line.push(plain(self.lines[row.line][row.start..row.end].to_string()));
                    }
                }
                None => line.push(text(format!("{}~", " ".repeat(gutter - 2)), &theme.border)),
            }
            lines.push(fit(line, width));
        }
        let mut hint = String::from("  ");
        if let Some(limit) = self.limit {
            hint.push_str(&format!("{}/{limit} · ", self.count()));
        }
        hint.push_str(&format!(
            "enter new line · {} submit · esc cancel",
            self.submit
        ));
        lines.push(fit(vec![text(hint, &theme.hint)], width));
        let view = View::new(lines);
        if caret_row >= top && caret_row < top + self.height {
            view.with_cursor(
                1 + caret_row - top,
                (gutter + caret_column).min(width.saturating_sub(1)),
            )
        } else {
            view
        }
    }

    fn default_value(&self) -> Option<String> {
        Some(self.text()).filter(|text| !text.is_empty())
    }

    /// Without a terminal: every line up to the end of input is the text,
    /// cut to the character limit. No lines at all answer with the text
    /// the area started with.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        io.write(&format!("{} (end with Ctrl+D):\n", self.prompt));
        let mut read: Vec<String> = Vec::new();
        let mut total = 0usize;
        while let Some(line) = io.read_line() {
            total += line.chars().count() + 1;
            read.push(clean(&line));
            if self.limit.is_some_and(|limit| total > limit) {
                break;
            }
        }
        if read.is_empty() {
            return Ok(Some(self.text()));
        }
        let text = read.join("\n");
        Ok(Some(match self.limit {
            Some(limit) => text.chars().take(limit).collect(),
            None => text,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_long_lines_one_cell_short() {
        let area = TextArea::new("Notes").value("abcdefghij\nxy");
        // 12 columns, a 2-cell gutter, one cell kept for the caret: 9.
        let rows = area.rows(12);
        let pieces: Vec<&str> = rows
            .iter()
            .map(|row| &area.lines[row.line][row.start..row.end])
            .collect();
        assert_eq!(pieces, ["abcdefghi", "j", "xy"]);
        assert!(rows[0].first && !rows[1].first && rows[2].first);
        assert_eq!(area.caret_at(&rows), (2, 2));
    }

    #[test]
    fn cleans_pasted_text() {
        assert_eq!(clean("a\r\nb\rc\td\x1b[2Je\u{9b}"), "a\nb\nc    d[2Je");
    }
}

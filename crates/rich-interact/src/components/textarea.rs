//! A multi-line editor (#493, `rich write`).
//!
//! Enter starts a new line; Ctrl+D (or the key the caller chooses) submits
//! and Escape cancels. Lines longer than the space wrap, and the text
//! scrolls to keep the caret in view. The arrows, Home/End, Ctrl+A/E,
//! Ctrl+U/K/W, Backspace and Delete edit as in [`Input`](crate::Input), the
//! caret moving by grapheme cluster; Up and Down move between lines. A
//! character limit counts grapheme clusters, line breaks too. Pasted text
//! keeps its line breaks and tabs (a tab shows as [`TAB`] spaces); other
//! terminal controls are dropped.

use std::cell::Cell;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::edit::{cells, clean, graphemes, length, Buffer, Pos, TAB};
use crate::event::{Event, Key, KeyCode};
use crate::keymap::{keys, Keymap};
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
#[must_use = "a component does nothing until it is run or mounted"]
pub struct TextArea {
    prompt: String,
    /// The text, the caret and the limit: the editing shared with
    /// `rs-rich-intuituive`'s `text_area`.
    buffer: Buffer,
    placeholder: Option<String>,
    submit: Key,
    height: usize,
    line_numbers: bool,
    theme: Theme,
    /// The first row shown. Moved in `handle`, which knows the width, and
    /// read in `render`.
    top: Cell<usize>,
    answer: Option<Option<String>>,
    /// Keys rebound on this area ([`TextArea::rebind`]).
    rebound: Keymap,
}

impl TextArea {
    pub fn new(prompt: impl Into<String>) -> TextArea {
        TextArea {
            prompt: prompt.into(),
            buffer: Buffer::new(""),
            placeholder: None,
            submit: Key::ctrl('d'),
            rebound: Keymap::new("textarea"),
            height: 5,
            line_numbers: false,
            theme: Theme::default(),
            top: Cell::new(0),
            answer: None,
        }
    }

    /// Start with this text, the caret at its end.
    pub fn value(mut self, value: impl AsRef<str>) -> Self {
        self.buffer.set_text(value.as_ref());
        self.buffer.bottom(false);
        self
    }

    /// Dim text shown while the area is empty.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// At most `chars` characters (grapheme clusters, so `👨‍👩‍👧` or `e`
    /// and a combining accent is one), line breaks included. Text already
    /// there beyond it is cut.
    pub fn char_limit(mut self, chars: usize) -> Self {
        self.buffer.set_limit(Some(chars));
        self.buffer.bottom(false);
        self
    }

    /// The first `chars` characters of `text` as the character limit
    /// counts them: grapheme clusters, a line break one. Never cuts a
    /// cluster.
    pub fn truncate(text: &str, chars: usize) -> &str {
        crate::edit::truncate(text, chars)
    }

    /// Make `keys` do `action` (in context `textarea`; see
    /// [`keymap`](Component::keymap)) on this area only; no keys unbinds
    /// it.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.rebound.rebind(action, keys);
        self
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
        self.buffer.text()
    }

    /// The caret: (line, character column).
    pub fn caret(&self) -> (usize, usize) {
        self.buffer.caret_column()
    }

    fn count(&self) -> usize {
        self.buffer.count()
    }

    /// Rows of text shown in a space `rows` high: the height asked for, cut
    /// to what fits beside the question and the hint.
    fn shown_rows(&self, rows: usize) -> usize {
        self.height.min(rows.saturating_sub(2)).max(1)
    }

    fn gutter(&self) -> usize {
        if self.line_numbers {
            self.buffer.line_count().to_string().len() + 3
        } else {
            2
        }
    }

    /// The rows at `width`, wrapped one cell short so the caret fits after
    /// the last character.
    fn rows(&self, width: usize) -> Vec<Row> {
        let room = width.saturating_sub(self.gutter() + 1).max(1);
        let mut rows = Vec::new();
        for (index, line) in self.buffer.lines().iter().enumerate() {
            let spans = graphemes(line);
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
            let Pos { line, at } = self.buffer.caret();
            let within = at >= row.start && (at < row.end || !next_same_line);
            if row.line == line && within {
                let text = self.buffer.line(row.line);
                return (index, cells(&text[row.start..at]));
            }
        }
        (0, 0)
    }

    /// Scroll so the caret is shown, `height` rows of text on screen.
    fn follow(&self, width: usize, height: usize) {
        let rows = self.rows(width);
        let (row, _) = self.caret_at(&rows);
        let top = self.top.get();
        if row < top {
            self.top.set(row);
        } else if row >= top + height {
            self.top.set(row + 1 - height);
        }
    }
}

impl Component for TextArea {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        let height = self.shown_rows(context.height);
        if let Event::Paste(pasted) = event {
            self.buffer.insert(pasted);
            self.follow(context.width, height);
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            if let Event::Resize { .. } = event {
                self.follow(context.width, height);
            }
            return Flow::Continue;
        };
        let keymap = self.keymap();
        // A key with modifiers that is not bound as it is does what it does
        // without them, unless it types.
        let action = keymap.action(key).or_else(|| {
            let typing = matches!(key.code, KeyCode::Char(_));
            (!typing && key.modifiers != crate::event::Modifiers::NONE)
                .then(|| keymap.action(Key::new(key.code)))
                .flatten()
        });
        match action.map(str::to_string).as_deref() {
            Some("submit") => {
                let text = self.text();
                self.answer = Some(Some(text.clone()));
                return Flow::Done(text);
            }
            Some("cancel") => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            Some("newline") => self.buffer.insert("\n"),
            Some("indent") => self.buffer.insert("    "),
            Some("backspace") => self.buffer.backspace(),
            Some("delete") => self.buffer.delete(),
            Some("left") => self.buffer.left(false),
            Some("right") => self.buffer.right(false),
            Some("up") => self.buffer.vertical(-1, false),
            Some("down") => self.buffer.vertical(1, false),
            Some("page-up") => self.buffer.vertical(-(height as isize), false),
            Some("page-down") => self.buffer.vertical(height as isize, false),
            Some("top") => self.buffer.top(false),
            Some("bottom") => self.buffer.bottom(false),
            Some("home") => self.buffer.home(false),
            Some("end") => self.buffer.end(false),
            Some("delete-to-start") => self.buffer.delete_to_start(),
            Some("delete-to-end") => self.buffer.delete_to_end(),
            Some("delete-word") => self.buffer.delete_word(),
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    let mut buffer = [0; 4];
                    self.buffer.insert(c.encode_utf8(&mut buffer));
                }
                _ => return Flow::Ignored,
            },
        }
        self.follow(context.width, height);
        Flow::Continue
    }

    fn keymap(&self) -> Keymap {
        let mut keymap = Keymap::new("textarea")
            .bind("submit", [self.submit], "submit")
            .bind("cancel", keys("escape"), "cancel")
            .bind("newline", keys("enter"), "new line")
            .bind("indent", keys("tab"), "indent")
            .bind("up", keys("up"), "line up")
            .bind("down", keys("down"), "line down")
            .bind("left", keys("left"), "move left")
            .bind("right", keys("right"), "move right")
            .bind("page-up", keys("pageup"), "page up")
            .bind("page-down", keys("pagedown"), "page down")
            .bind("home", keys("home ctrl+a"), "start of the line")
            .bind("end", keys("end ctrl+e"), "end of the line")
            .bind("top", keys("ctrl+home"), "start of the text")
            .bind("bottom", keys("ctrl+end"), "end of the text")
            .bind(
                "delete-to-start",
                keys("ctrl+u"),
                "delete to the line's start",
            )
            .bind("delete-to-end", keys("ctrl+k"), "delete to the line's end")
            .bind("delete-word", keys("ctrl+w"), "delete a word")
            .bind("backspace", keys("backspace"), "delete back")
            .bind("delete", keys("delete"), "delete forward");
        keymap.extend(self.rebound.clone());
        keymap
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
        let height = self.shown_rows(context.height);
        let top = self.top.get().min(rows.len().saturating_sub(1));
        let gutter = self.gutter();
        let empty = self.buffer.is_empty();
        for index in top..top + height {
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
                        let piece = &self.buffer.line(row.line)[row.start..row.end];
                        line.push(plain(piece.replace('\t', &" ".repeat(TAB))));
                    }
                }
                None => line.push(text(format!("{}~", " ".repeat(gutter - 2)), &theme.border)),
            }
            lines.push(fit(line, width));
        }
        let mut hint = String::from("  ");
        if let Some(limit) = self.buffer.limit() {
            hint.push_str(&format!("{}/{limit} · ", self.count()));
        }
        hint.push_str(&format!(
            "enter new line · {} submit · esc cancel",
            self.keymap()
                .key("submit")
                .map_or_else(|| "-".to_string(), |key| key.to_string())
        ));
        lines.push(fit(vec![text(hint, &theme.hint)], width));
        let view = View::new(lines);
        if caret_row >= top && caret_row < top + height {
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
            total += length(&line) + 1;
            read.push(clean(&line));
            if self.buffer.limit().is_some_and(|limit| total > limit) {
                break;
            }
        }
        if read.is_empty() {
            return Ok(Some(self.text()));
        }
        let text = read.join("\n");
        Ok(Some(match self.buffer.limit() {
            Some(limit) => TextArea::truncate(&text, limit).to_string(),
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
            .map(|row| &area.buffer.line(row.line)[row.start..row.end])
            .collect();
        assert_eq!(pieces, ["abcdefghi", "j", "xy"]);
        assert!(rows[0].first && !rows[1].first && rows[2].first);
        assert_eq!(area.caret_at(&rows), (2, 2));
    }
}

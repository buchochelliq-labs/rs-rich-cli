//! A Vim preset for a text editor: modes, motions, operators and counts,
//! as a [`KeyMode`] over an [`edit::Buffer`](crate::edit::Buffer). Off by
//! default, behind the `vim` feature.
//!
//! - **Modes:** normal, insert (`i a I A o O s S C`, `c` with a motion),
//!   visual (`v`) and visual line (`V`); Esc goes back to normal.
//! - **Motions:** `h j k l` and the arrows, `w b e`, `0 ^ $`, `gg G`,
//!   `{ }`, `f t F T` with a character, `n N` (the buffer's
//!   [search](crate::edit::Buffer::set_search)), Ctrl+D/U (half a page)
//!   and Ctrl+F/B (a page).
//! - **Operators** `d c y > <` with a motion, doubled for whole lines
//!   (`dd`, `cc`, `yy`, `>>`), or on a visual selection; `x X D C s S J
//!   r p P u` and Ctrl+R (redo).
//! - **Counts** before a command or a motion (`3dw`, `d3w`, `5j`).
//! - **`.`** repeats the last change, typing included.
//!
//! Deleted and yanked text goes on the buffer's kill ring, which `p` puts
//! back: one register. `:` and `/` and keys it does not know go on to the
//! app, which can open a command line or a search bar on them.
//!
//! ```
//! use rich_interact::edit::Buffer;
//! use rich_interact::edit_keys::{KeyMode, ModeCx};
//! use rich_interact::vim::Vim;
//! use rich_interact::Key;
//!
//! let mut vim = Vim::new();
//! let mut buffer = Buffer::new("one two three");
//! for key in "wdwP$x".chars() {
//!     let mut cx = ModeCx { buffer: &mut buffer, editable: true, single_line: false, page: 10 };
//!     vim.key(Key::char(key), &mut cx);
//! }
//! assert_eq!(buffer.text(), "one two thre");
//! assert_eq!(vim.label().as_deref(), Some("NORMAL"));
//! ```

use crate::caret::CaretShape;
use crate::edit::{Buffer, Pos};
use crate::edit_keys::{KeyMode, ModeCx, ModeKey};
use crate::event::{Key, KeyCode};

/// Which mode a [`Vim`] is in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
    Visual,
    VisualLine,
}

/// A key waiting for the next one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    /// `g`, for `gg`.
    G,
    /// `f`, `t`, `F` or `T`: the character to find.
    Find(char),
    /// `r`: the character to put.
    Replace,
}

/// How a motion's range is taken by an operator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Shape {
    /// Whole lines (`j`, `gg`, `dd`).
    linewise: bool,
    /// The character the motion ends on is included (`e`, `$`, `f`).
    inclusive: bool,
}

/// Vim's keys over a buffer: see the [module](self).
#[derive(Clone, Debug, Default)]
pub struct Vim {
    mode: VimMode,
    count: Option<usize>,
    /// A count was typed (`5G` is not `G`).
    count_given: bool,
    /// An operator waiting for its motion, with the count typed before it.
    operator: Option<(char, Option<usize>)>,
    pending: Option<Pending>,
    /// The visual selection's fixed end, and the cursor.
    visual: Option<(Pos, Pos)>,
    /// The register holds whole lines.
    linewise: bool,
    /// The keys of the change being made, for `.`.
    recording: Option<Vec<Key>>,
    /// Digits typed before a command, which a recording starts with.
    count_keys: Vec<Key>,
    last_change: Vec<Key>,
    replaying: bool,
}

impl Vim {
    /// In normal mode.
    pub fn new() -> Vim {
        Vim::default()
    }

    /// In insert mode: a field that starts out typing.
    pub fn inserting() -> Vim {
        Vim {
            mode: VimMode::Insert,
            ..Vim::default()
        }
    }

    pub fn mode(&self) -> VimMode {
        self.mode
    }

    /// The count typed, at least 1.
    fn take_count(&mut self) -> usize {
        self.count.take().unwrap_or(1).max(1)
    }

    fn normal(&mut self, cx: &mut ModeCx<'_>) {
        // Out of insert mode the caret steps back onto the last character
        // typed.
        if self.mode == VimMode::Insert && cx.buffer.caret().at > 0 {
            cx.buffer.left(false);
        }
        self.mode = VimMode::Normal;
        self.operator = None;
        self.pending = None;
        if let Some((_, cursor)) = self.visual.take() {
            cx.buffer.set_caret(cursor, false);
        }
        on_char(cx.buffer);
    }

    /// Start insert mode at the caret.
    fn insert(&mut self, cx: &mut ModeCx<'_>) {
        self.mode = VimMode::Insert;
        let caret = cx.buffer.caret();
        cx.buffer.set_caret(caret, false);
    }

    /// Whether `c` starts a change, for `.`.
    fn changes(c: char) -> bool {
        "dcxXDCsSpPJoOiaIAr><".contains(c)
    }

    /// The key in insert mode while replaying: typed into the buffer, as
    /// the widget would have.
    fn replay_insert(key: Key, cx: &mut ModeCx<'_>) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                let mut bytes = [0; 4];
                cx.buffer.insert(c.encode_utf8(&mut bytes));
            }
            KeyCode::Enter => cx.buffer.insert("\n"),
            KeyCode::Backspace => cx.buffer.backspace(),
            KeyCode::Delete => cx.buffer.delete(),
            KeyCode::Tab => cx.buffer.indent(),
            KeyCode::Left => cx.buffer.left(false),
            KeyCode::Right => cx.buffer.right(false),
            _ => {}
        }
    }

    /// Move the caret by the motion `key`, `count` times: its shape, or
    /// `None` when `key` is not a motion. `operator`: an operator waits for
    /// it, so `l` and `$` may reach the line's end.
    fn motion(
        &mut self,
        key: Key,
        count: usize,
        operator: Option<char>,
        cx: &mut ModeCx<'_>,
    ) -> Option<Shape> {
        let b = &mut *cx.buffer;
        let ctrl = key.modifiers.ctrl;
        let exclusive = Shape::default();
        let inclusive = Shape {
            inclusive: true,
            ..Shape::default()
        };
        let lines = Shape {
            linewise: true,
            ..Shape::default()
        };
        let half = (cx.page / 2).max(1) as isize;
        let c = match key.code {
            KeyCode::Char(c) if ctrl => {
                let delta = match c {
                    'd' => half,
                    'u' => -half,
                    'f' => cx.page.max(1) as isize,
                    'b' => -(cx.page.max(1) as isize),
                    _ => return None,
                };
                b.vertical(delta * count as isize, false);
                return Some(lines);
            }
            KeyCode::Char(_) if key.modifiers.alt => return None,
            KeyCode::Char(c) => c,
            KeyCode::Left => 'h',
            KeyCode::Right => 'l',
            KeyCode::Up => 'k',
            KeyCode::Down => 'j',
            KeyCode::Home => '0',
            KeyCode::End => '$',
            _ => return None,
        };
        if let Some(Pending::Find(kind)) = self.pending {
            self.pending = None;
            let caret = b.caret();
            let line = b.line(caret.line).to_string();
            let mut at = caret.at;
            for _ in 0..count {
                let found = match kind {
                    'f' | 't' => {
                        let from = b.after(Pos::new(caret.line, at));
                        let from = if from.line == caret.line {
                            from.at
                        } else {
                            line.len()
                        };
                        line[from..].find(c).map(|i| from + i)
                    }
                    _ => line[..at].rfind(c),
                };
                match found {
                    Some(i) => at = i,
                    None => return Some(exclusive),
                }
            }
            let mut to = Pos::new(caret.line, at);
            match kind {
                't' => to = b.before(to),
                'T' => to = b.after(to),
                _ => {}
            }
            b.set_caret(to, false);
            return Some(if kind == 'f' || kind == 't' {
                inclusive
            } else {
                exclusive
            });
        }
        if self.pending == Some(Pending::G) {
            self.pending = None;
            if c != 'g' {
                return None;
            }
            let line = self.count_line(count, b, 0);
            b.set_caret(first_blank(b, line), false);
            return Some(lines);
        }
        match c {
            'h' => {
                for _ in 0..count {
                    if b.caret().at > 0 {
                        b.left(false);
                    }
                }
                Some(exclusive)
            }
            'l' | ' ' => {
                for _ in 0..count {
                    let caret = b.caret();
                    let next = b.after(caret);
                    let end = b.line(caret.line).len();
                    if next.line == caret.line && (next.at < end || operator.is_some()) {
                        b.right(false);
                    }
                }
                Some(exclusive)
            }
            'j' | 'k' => {
                let delta = if c == 'j' { 1 } else { -1 };
                b.vertical(delta * count as isize, false);
                Some(lines)
            }
            'w' => {
                // `cw` on a word changes to its end, as `ce` does.
                let on_word = class(b, b.caret()) != Class::Space;
                if operator == Some('c') && on_word {
                    for i in 0..count {
                        let to = word_end(b, b.caret(), i == 0);
                        b.set_caret(to, false);
                    }
                    return Some(inclusive);
                }
                let start = b.caret();
                for _ in 0..count {
                    let to = word_start(b, b.caret());
                    b.set_caret(to, false);
                }
                // An operator's `w` stops at the end of its line.
                let to = b.caret();
                if operator.is_some() && to.line > start.line {
                    let end = Pos::new(start.line, b.line(start.line).len());
                    if end > start {
                        b.set_caret(end, false);
                    }
                }
                Some(exclusive)
            }
            'e' => {
                for _ in 0..count {
                    let to = word_end(b, b.caret(), false);
                    b.set_caret(to, false);
                }
                Some(inclusive)
            }
            'b' => {
                for _ in 0..count {
                    let to = word_back(b, b.caret());
                    b.set_caret(to, false);
                }
                Some(exclusive)
            }
            '0' => {
                b.home(false);
                Some(exclusive)
            }
            '^' => {
                let line = b.caret().line;
                b.set_caret(first_blank(b, line), false);
                Some(exclusive)
            }
            '$' => {
                if count > 1 {
                    b.vertical(count as isize - 1, false);
                }
                b.end(false);
                if b.caret().at > 0 {
                    let last = b.before(b.caret());
                    b.set_caret(last, false);
                    return Some(inclusive);
                }
                Some(exclusive)
            }
            'G' => {
                let last = b.line_count() - 1;
                let line = self.count_line(count, b, last);
                b.set_caret(first_blank(b, line), false);
                Some(lines)
            }
            'g' => {
                self.pending = Some(Pending::G);
                Some(exclusive)
            }
            '}' => {
                for _ in 0..count {
                    b.paragraph_down(false);
                }
                Some(exclusive)
            }
            '{' => {
                for _ in 0..count {
                    b.paragraph_up(false);
                }
                Some(exclusive)
            }
            'n' | 'N' => {
                for _ in 0..count {
                    if c == 'n' {
                        b.search_forward(false);
                    } else {
                        b.search_back(false);
                    }
                }
                Some(exclusive)
            }
            'f' | 't' | 'F' | 'T' => {
                self.pending = Some(Pending::Find(c));
                Some(exclusive)
            }
            _ => None,
        }
    }

    /// The line a count names (`5G`: the fifth), else `default`.
    fn count_line(&self, count: usize, b: &Buffer, default: usize) -> usize {
        if self.count_given {
            (count.max(1) - 1).min(b.line_count() - 1)
        } else {
            default
        }
    }

    /// Do `operator` from `start` to where the caret is now.
    fn operate(&mut self, operator: char, start: Pos, shape: Shape, cx: &mut ModeCx<'_>) {
        let b = &mut *cx.buffer;
        let target = b.caret();
        let (mut from, mut to) = if start <= target {
            (start, target)
        } else {
            (target, start)
        };
        if shape.linewise {
            self.operate_lines(operator, from.line, to.line, cx);
            return;
        }
        if shape.inclusive && to.at < b.line(to.line).len() {
            to = b.after(to);
        }
        if !cx.editable && operator != 'y' {
            b.set_caret(start, false);
            return;
        }
        match operator {
            'y' => {
                let text = b.slice(from, to);
                b.push_kill(&text);
                self.linewise = false;
                b.set_caret(from, false);
            }
            'd' | 'c' => {
                if from != to {
                    let text = b.slice(from, to);
                    b.push_kill(&text);
                    self.linewise = false;
                    b.replace(from, to, "");
                }
                b.set_caret(from, false);
                if operator == 'c' {
                    self.insert(cx);
                } else {
                    on_char(cx.buffer);
                }
            }
            '>' | '<' => {
                from.at = 0;
                self.operate_lines(operator, from.line, to.line, cx);
            }
            _ => {}
        }
    }

    /// Do `operator` on lines `first` to `last`.
    fn operate_lines(&mut self, operator: char, first: usize, last: usize, cx: &mut ModeCx<'_>) {
        let b = &mut *cx.buffer;
        let count = b.line_count();
        let register = (first..=last)
            .map(|i| b.line(i).to_string())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        if operator == 'y' {
            b.push_kill(&register);
            self.linewise = true;
            let caret = b.caret();
            if caret.line != first {
                b.set_caret(Pos::new(first, caret.at), false);
            }
            return;
        }
        if !cx.editable {
            return;
        }
        let end_of = |b: &Buffer, line: usize| Pos::new(line, b.line(line).len());
        match operator {
            'd' => {
                b.push_kill(&register);
                self.linewise = true;
                if last + 1 < count {
                    b.replace(Pos::new(first, 0), Pos::new(last + 1, 0), "");
                    b.set_caret(first_blank(b, first), false);
                } else if first > 0 {
                    b.replace(end_of(b, first - 1), end_of(b, last), "");
                    b.set_caret(first_blank(b, first - 1), false);
                } else {
                    b.replace(Pos::new(0, 0), end_of(b, last), "");
                }
            }
            'c' => {
                b.push_kill(&register);
                self.linewise = true;
                let indent = first_blank(b, first);
                b.replace(indent, end_of(b, last), "");
                self.insert(cx);
            }
            '>' | '<' => {
                if operator == '>' {
                    b.indent_lines(first, last);
                } else {
                    b.outdent_lines(first, last);
                }
                b.set_caret(first_blank(b, first), false);
            }
            _ => {}
        }
    }

    /// Put the register after the caret (`p`) or before it (`P`).
    fn put(&mut self, after: bool, count: usize, cx: &mut ModeCx<'_>) {
        let b = &mut *cx.buffer;
        let Some(text) = b.kills().last().cloned() else {
            return;
        };
        let caret = b.caret();
        if self.linewise {
            let body = text.strip_suffix('\n').unwrap_or(&text);
            let body = vec![body; count].join("\n");
            if after {
                let at = Pos::new(caret.line, b.line(caret.line).len());
                b.replace(at, at, &format!("\n{body}"));
                b.set_caret(first_blank(b, caret.line + 1), false);
            } else {
                let at = Pos::new(caret.line, 0);
                b.replace(at, at, &format!("{body}\n"));
                b.set_caret(first_blank(b, caret.line), false);
            }
            return;
        }
        let text = text.repeat(count);
        let at = if after && caret.at < b.line(caret.line).len() {
            b.after(caret)
        } else {
            caret
        };
        b.replace(at, at, &text);
        let end = b.caret();
        b.set_caret(b.before(end), false);
    }

    /// A key in normal or visual mode.
    fn command(&mut self, key: Key, cx: &mut ModeCx<'_>) -> ModeKey {
        let plain = !key.modifiers.ctrl && !key.modifiers.alt;
        let char = match key.code {
            KeyCode::Char(c) if plain => Some(c),
            _ => None,
        };
        // `r` and a character.
        if self.pending == Some(Pending::Replace) {
            self.pending = None;
            let count = self.take_count();
            if let (Some(c), true) = (char, cx.editable) {
                let b = &mut *cx.buffer;
                let caret = b.caret();
                let line = b.line(caret.line);
                let room = crate::edit::stops(line)
                    .iter()
                    .filter(|&&s| s >= caret.at)
                    .count()
                    .saturating_sub(1);
                if room >= count {
                    let mut end = caret;
                    for _ in 0..count {
                        end = b.after(end);
                    }
                    b.replace(caret, end, &c.to_string().repeat(count));
                    let at = b.before(b.caret());
                    b.set_caret(at, false);
                }
            }
            return ModeKey::Used;
        }
        if key.code == KeyCode::Escape {
            let busy = self.count.is_some()
                || self.operator.is_some()
                || self.pending.is_some()
                || self.visual.is_some();
            self.count = None;
            self.count_given = false;
            self.normal(cx);
            return if busy { ModeKey::Used } else { ModeKey::Unused };
        }
        // A count.
        if let Some(d @ '0'..='9') = char {
            if (d != '0' || self.count.is_some()) && self.pending.is_none() {
                let n = d.to_digit(10).unwrap_or(0) as usize;
                self.count = Some(self.count.unwrap_or(0).saturating_mul(10).saturating_add(n));
                self.count_given = true;
                if self.operator.is_none() {
                    self.count_keys.push(key);
                }
                return ModeKey::Used;
            }
        }
        if key == Key::ctrl('r') {
            if cx.editable {
                for _ in 0..self.take_count() {
                    cx.buffer.redo();
                }
            }
            self.count_given = false;
            return ModeKey::Used;
        }
        if self.visual.is_some() {
            return self.visual_key(key, char, cx);
        }
        // An operator waits for its motion, or for itself doubled.
        if let Some((operator, before)) = self.operator {
            if char == Some(operator) {
                self.operator = None;
                let count = before.unwrap_or(1) * self.take_count();
                self.count_given = false;
                let line = cx.buffer.caret().line;
                let last = (line + count - 1).min(cx.buffer.line_count() - 1);
                self.operate_lines(operator, line, last, cx);
                return ModeKey::Used;
            }
            let count = before.unwrap_or(1) * self.count.unwrap_or(1).max(1);
            let start = cx.buffer.caret();
            let given = self.count_given || before.is_some();
            self.count_given = given;
            match self.motion(key, count, Some(operator), cx) {
                Some(_) if self.pending.is_some() => {}
                Some(shape) => {
                    self.operator = None;
                    self.count = None;
                    self.count_given = false;
                    self.operate(operator, start, shape, cx);
                }
                None => {
                    self.operator = None;
                    self.count = None;
                    self.count_given = false;
                    cx.buffer.set_caret(start, false);
                }
            }
            return ModeKey::Used;
        }
        if self.pending.is_some() {
            let count = self.count.unwrap_or(1).max(1);
            if self.motion(key, count, None, cx).is_some() && self.pending.is_none() {
                self.count = None;
                self.count_given = false;
                on_char(cx.buffer);
            }
            return ModeKey::Used;
        }
        let Some(c) = char else {
            // Ctrl+D and the like, and the arrows, are motions; any other
            // key does what it would without the mode.
            let count = self.count.unwrap_or(1).max(1);
            if self.motion(key, count, None, cx).is_some() {
                self.count = None;
                self.count_given = false;
                on_char(cx.buffer);
                return ModeKey::Used;
            }
            return ModeKey::Pass;
        };
        let edits = Vim::changes(c);
        if edits && !cx.editable {
            self.count = None;
            self.count_given = false;
            return ModeKey::Used;
        }
        if edits && !self.replaying {
            let mut keys = std::mem::take(&mut self.count_keys);
            keys.push(key);
            self.recording = Some(keys);
        }
        let count = self.count.unwrap_or(1).max(1);
        let b = &mut *cx.buffer;
        match c {
            'i' => self.insert(cx),
            'a' => {
                let caret = b.caret();
                if caret.at < b.line(caret.line).len() {
                    b.right(false);
                }
                self.insert(cx);
            }
            'I' => {
                let line = b.caret().line;
                b.set_caret(first_blank(b, line), false);
                self.insert(cx);
            }
            'A' => {
                b.end(false);
                self.insert(cx);
            }
            'o' | 'O' => {
                let line = b.caret().line;
                if c == 'o' {
                    b.set_caret(Pos::new(line, b.line(line).len()), false);
                    b.insert("\n");
                } else {
                    b.set_caret(Pos::new(line, 0), false);
                    b.insert("\n");
                    b.set_caret(Pos::new(line, 0), false);
                }
                self.insert(cx);
            }
            'x' | 'X' | 's' | 'D' | 'C' | 'S' => {
                let (operator, motion) = match c {
                    'x' => ('d', 'l'),
                    'X' => ('d', 'h'),
                    's' => ('c', 'l'),
                    'D' => ('d', '$'),
                    'C' => ('c', '$'),
                    _ => ('c', 'c'),
                };
                if motion == 'c' {
                    let line = b.caret().line;
                    let last = (line + count - 1).min(b.line_count() - 1);
                    self.operate_lines('c', line, last, cx);
                } else {
                    let start = b.caret();
                    let shape = self.motion(Key::char(motion), count, Some(operator), cx);
                    self.operate(operator, start, shape.unwrap_or_default(), cx);
                }
            }
            'd' | 'c' | 'y' | '>' | '<' => {
                self.operator = Some((c, self.count.take()));
                return ModeKey::Used;
            }
            'p' | 'P' => self.put(c == 'p', count, cx),
            'J' => {
                for _ in 0..count.max(2) - 1 {
                    let line = b.caret().line;
                    if line + 1 >= b.line_count() {
                        break;
                    }
                    let end = Pos::new(line, b.line(line).len());
                    let next = first_blank(b, line + 1);
                    let glue = if b.line(line + 1).trim().is_empty() {
                        ""
                    } else {
                        " "
                    };
                    b.replace(end, next, glue);
                    b.set_caret(end, false);
                }
            }
            'r' => {
                self.pending = Some(Pending::Replace);
                return ModeKey::Used;
            }
            'u' => {
                for _ in 0..count {
                    b.undo();
                }
                on_char(b);
            }
            'v' | 'V' => {
                let caret = b.caret();
                self.visual = Some((caret, caret));
                self.mode = if c == 'v' {
                    VimMode::Visual
                } else {
                    VimMode::VisualLine
                };
                self.show_visual(cx.buffer);
            }
            '.' => {
                let keys = self.last_change.clone();
                self.count = None;
                self.count_given = false;
                self.replaying = true;
                for key in keys {
                    let _ = self.key(key, cx);
                }
                self.replaying = false;
                self.count_keys.clear();
                return ModeKey::Used;
            }
            _ => {
                if self.motion(key, count, None, cx).is_none() {
                    self.count = None;
                    self.count_given = false;
                    self.count_keys.clear();
                    return ModeKey::Unused;
                }
                if self.pending.is_some() {
                    return ModeKey::Used;
                }
                on_char(cx.buffer);
            }
        }
        self.count = None;
        self.count_given = false;
        self.count_keys.clear();
        ModeKey::Used
    }

    /// A key in visual mode.
    fn visual_key(&mut self, key: Key, char: Option<char>, cx: &mut ModeCx<'_>) -> ModeKey {
        let Some((anchor, cursor)) = self.visual else {
            return ModeKey::Unused;
        };
        let line_mode = self.mode == VimMode::VisualLine;
        let (from, to) = if anchor <= cursor {
            (anchor, cursor)
        } else {
            (cursor, anchor)
        };
        let shape = Shape {
            linewise: line_mode,
            inclusive: true,
        };
        match char {
            Some(c @ ('d' | 'x' | 'y' | 'c' | '>' | '<' | 'X' | 'D' | 'Y')) => {
                let operator = match c {
                    'x' | 'X' | 'D' => 'd',
                    'Y' => 'y',
                    c => c,
                };
                let linewise = line_mode || "XDY".contains(c);
                self.visual = None;
                self.mode = VimMode::Normal;
                self.count = None;
                self.count_given = false;
                cx.buffer.set_caret(to, false);
                self.operate(operator, from, Shape { linewise, ..shape }, cx);
                if operator != 'c' {
                    self.mode = VimMode::Normal;
                }
                return ModeKey::Used;
            }
            Some('o') => {
                self.visual = Some((cursor, anchor));
                cx.buffer.set_caret(anchor, false);
                self.show_visual(cx.buffer);
                return ModeKey::Used;
            }
            Some(c @ ('v' | 'V')) => {
                let wanted = if c == 'v' {
                    VimMode::Visual
                } else {
                    VimMode::VisualLine
                };
                if self.mode == wanted {
                    self.normal(cx);
                } else {
                    self.mode = wanted;
                    self.show_visual(cx.buffer);
                }
                return ModeKey::Used;
            }
            _ => {}
        }
        let count = self.count.unwrap_or(1).max(1);
        cx.buffer.set_caret(cursor, false);
        match self.motion(key, count, None, cx) {
            Some(_) => {
                if self.pending.is_none() {
                    self.count = None;
                    self.count_given = false;
                }
                self.visual = Some((anchor, cx.buffer.caret()));
                self.show_visual(cx.buffer);
                ModeKey::Used
            }
            None => {
                self.show_visual(cx.buffer);
                if char.is_some() {
                    ModeKey::Unused
                } else {
                    ModeKey::Pass
                }
            }
        }
    }

    /// Select what the visual mode covers: the characters from one end to
    /// the other, both included, or their whole lines.
    fn show_visual(&self, b: &mut Buffer) {
        let Some((anchor, cursor)) = self.visual else {
            return;
        };
        if self.mode == VimMode::VisualLine {
            let end_of = |b: &Buffer, line: usize| Pos::new(line, b.line(line).len());
            if anchor <= cursor {
                b.select(Pos::new(anchor.line, 0), end_of(b, cursor.line));
            } else {
                b.select(end_of(b, anchor.line), Pos::new(cursor.line, 0));
            }
            return;
        }
        let past = |b: &Buffer, pos: Pos| {
            if pos.at < b.line(pos.line).len() {
                b.after(pos)
            } else {
                pos
            }
        };
        if anchor <= cursor {
            let end = past(b, cursor);
            b.select(anchor, end);
        } else {
            let start = past(b, anchor);
            b.select(start, cursor);
        }
    }
}

impl KeyMode for Vim {
    fn key(&mut self, key: Key, cx: &mut ModeCx<'_>) -> ModeKey {
        let recording = self.recording.is_some() && !self.replaying;
        let out = if self.mode == VimMode::Insert {
            if key.code == KeyCode::Escape {
                self.normal(cx);
                ModeKey::Used
            } else if self.replaying {
                Vim::replay_insert(key, cx);
                ModeKey::Used
            } else {
                ModeKey::Pass
            }
        } else {
            self.command(key, cx)
        };
        if recording {
            if let Some(keys) = self.recording.as_mut() {
                keys.push(key);
            }
        }
        // A change ends back in normal mode with nothing waiting.
        if self.mode == VimMode::Normal
            && self.operator.is_none()
            && self.pending.is_none()
            && !self.replaying
        {
            if let Some(keys) = self.recording.take() {
                self.last_change = keys;
            }
        }
        out
    }

    fn caret_shape(&self) -> Option<CaretShape> {
        Some(match self.mode {
            VimMode::Insert => CaretShape::Bar,
            _ => CaretShape::Block,
        })
    }

    fn label(&self) -> Option<String> {
        Some(
            match self.mode {
                VimMode::Normal => "NORMAL",
                VimMode::Insert => "INSERT",
                VimMode::Visual => "VISUAL",
                VimMode::VisualLine => "V-LINE",
            }
            .to_string(),
        )
    }

    fn pending(&self) -> Option<String> {
        let mut out = String::new();
        if let Some((operator, count)) = self.operator {
            if let Some(n) = count {
                out.push_str(&n.to_string());
            }
            out.push(operator);
        }
        if let Some(n) = self.count {
            out.push_str(&n.to_string());
        }
        match self.pending {
            Some(Pending::G) => out.push('g'),
            Some(Pending::Find(c)) => out.push(c),
            Some(Pending::Replace) => out.push('r'),
            None => {}
        }
        (!out.is_empty()).then_some(out)
    }
}

/// What a character is, for word moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    /// Spaces, and the end of a line.
    Space,
    Word,
    Punctuation,
}

fn class(b: &Buffer, pos: Pos) -> Class {
    match b.line(pos.line)[pos.at..].chars().next() {
        None => Class::Space,
        Some(c) if c.is_whitespace() => Class::Space,
        Some(c) if c.is_alphanumeric() || c == '_' => Class::Word,
        Some(_) => Class::Punctuation,
    }
}

/// `w`: the start of the next word, or an empty line.
fn word_start(b: &Buffer, from: Pos) -> Pos {
    let mut pos = from;
    let here = class(b, pos);
    if here != Class::Space {
        while class(b, pos) == here {
            let next = b.after(pos);
            if next == pos || next.line != pos.line {
                pos = next;
                break;
            }
            pos = next;
        }
    }
    loop {
        if class(b, pos) != Class::Space {
            return pos;
        }
        let next = b.after(pos);
        if next == pos {
            return pos;
        }
        if next.line != pos.line && b.line(next.line).is_empty() {
            return next;
        }
        pos = next;
    }
}

/// `e`: the last character of this word or the next. `here`: the word the
/// caret is in counts (`cw`).
fn word_end(b: &Buffer, from: Pos, here: bool) -> Pos {
    let on_end = |pos: Pos| {
        let next = b.after(pos);
        next == pos || next.line != pos.line || class(b, next) != class(b, pos)
    };
    let mut pos = from;
    if !(here && class(b, pos) != Class::Space) {
        // At least one on, then over spaces.
        let next = b.after(pos);
        if next == pos {
            return pos;
        }
        pos = next;
        while class(b, pos) == Class::Space {
            let next = b.after(pos);
            if next == pos {
                return pos;
            }
            pos = next;
        }
    }
    while !on_end(pos) {
        pos = b.after(pos);
    }
    pos
}

/// `b`: the start of this word or the one before.
fn word_back(b: &Buffer, from: Pos) -> Pos {
    let mut pos = b.before(from);
    if pos == from {
        return pos;
    }
    while class(b, pos) == Class::Space {
        let before = b.before(pos);
        if before == pos || b.line(pos.line).is_empty() && pos.line != from.line {
            return pos;
        }
        pos = before;
    }
    let kind = class(b, pos);
    loop {
        let before = b.before(pos);
        if before == pos || before.line != pos.line || class(b, before) != kind {
            return pos;
        }
        pos = before;
    }
}

/// The first character of `line` that is not a space.
fn first_blank(b: &Buffer, line: usize) -> Pos {
    let text = b.line(line);
    Pos::new(line, text.len() - text.trim_start().len())
}

/// In normal mode the caret is on a character: never past a line's last.
fn on_char(b: &mut Buffer) {
    let caret = b.caret();
    if caret.at > 0 && caret.at >= b.line(caret.line).len() {
        let last = b.before(caret);
        b.set_caret(last, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `keys` typed into a Vim over `text`: `<esc>`, `<enter>`, `<bs>`
    /// and `<c-r>` name keys; a key it passes is typed, as a widget would.
    fn run(text: &str, keys: &str) -> (Buffer, Vim) {
        let mut buffer = Buffer::new(text);
        let mut vim = Vim::new();
        type_keys(&mut buffer, &mut vim, keys);
        (buffer, vim)
    }

    fn type_keys(buffer: &mut Buffer, vim: &mut Vim, keys: &str) {
        let mut rest = keys;
        while let Some(c) = rest.chars().next() {
            let key = if c == '<' {
                let end = rest.find('>').expect("a closing >");
                let name = &rest[1..end];
                rest = &rest[end + 1..];
                match name {
                    "esc" => Key::new(KeyCode::Escape),
                    "enter" => Key::new(KeyCode::Enter),
                    "bs" => Key::new(KeyCode::Backspace),
                    "c-r" => Key::ctrl('r'),
                    "c-d" => Key::ctrl('d'),
                    "lt" => Key::char('<'),
                    other => panic!("{other}"),
                }
            } else {
                rest = &rest[c.len_utf8()..];
                Key::char(c)
            };
            let mut cx = ModeCx {
                buffer: &mut *buffer,
                editable: true,
                single_line: false,
                page: 10,
            };
            if vim.key(key, &mut cx) == ModeKey::Pass {
                Vim::replay_insert(key, &mut cx);
            }
        }
    }

    fn caret(b: &Buffer) -> (usize, usize) {
        let pos = b.caret();
        (pos.line, pos.at)
    }

    #[test]
    fn word_motions() {
        let (b, _) = run("foo.bar baz\n\nqux", "w");
        assert_eq!(caret(&b), (0, 3));
        let (b, _) = run("foo.bar baz\n\nqux", "ww");
        assert_eq!(caret(&b), (0, 4));
        let (b, _) = run("foo.bar baz\n\nqux", "wwww");
        assert_eq!(caret(&b), (1, 0), "an empty line is a word");
        let (b, _) = run("foo.bar baz\n\nqux", "e");
        assert_eq!(caret(&b), (0, 2));
        let (b, _) = run("foo.bar baz\n\nqux", "ee");
        assert_eq!(caret(&b), (0, 3));
        let (b, _) = run("foo.bar baz\n\nqux", "Gb");
        assert_eq!(caret(&b), (1, 0));
        let (b, _) = run("foo.bar baz", "$bb");
        assert_eq!(caret(&b), (0, 4));
        let (b, _) = run("  indented", "$^");
        assert_eq!(caret(&b), (0, 2));
        let (b, _) = run("abc", "$");
        assert_eq!(caret(&b), (0, 2), "on the last character");
        let (b, _) = run("abc", "lllll");
        assert_eq!(caret(&b), (0, 2));
        let (b, _) = run("a,b,c,d", "2f,");
        assert_eq!(caret(&b), (0, 3));
        let (b, _) = run("a,b,c,d", "t,");
        assert_eq!(caret(&b), (0, 0));
        let (b, _) = run("a,b,c,d", "$F,");
        assert_eq!(caret(&b), (0, 5));
    }

    #[test]
    fn operators_with_motions_and_counts() {
        let (b, _) = run("one two three four", "dw");
        assert_eq!(b.text(), "two three four");
        let (b, _) = run("one two three four", "2dw");
        assert_eq!(b.text(), "three four");
        let (b, _) = run("one two three four", "d2w");
        assert_eq!(b.text(), "three four");
        let (b, _) = run("one two", "wdw");
        assert_eq!(b.text(), "one ", "dw stops at the line's end");
        let (b, _) = run("one two", "de");
        assert_eq!(b.text(), " two");
        let (b, _) = run("one two", "wd$");
        assert_eq!(b.text(), "one ");
        let (b, _) = run("a(b)c", "df)");
        assert_eq!(b.text(), "c");
        let (b, _) = run("a(b)c", "dt)");
        assert_eq!(b.text(), ")c");
        let (b, _) = run("abcdef", "3x");
        assert_eq!(b.text(), "def");
        let (b, _) = run("abcdef", "$X");
        assert_eq!(b.text(), "abcdf");
    }

    #[test]
    fn lines_delete_yank_and_put() {
        let (b, _) = run("1\n2\n3\n4", "jddp");
        assert_eq!(b.text(), "1\n3\n2\n4");
        assert_eq!(caret(&b), (2, 0));
        let (b, _) = run("1\n2\n3\n4", "2yyGp");
        assert_eq!(b.text(), "1\n2\n3\n4\n1\n2");
        let (b, _) = run("1\n2\n3\n4", "Gdd");
        assert_eq!(b.text(), "1\n2\n3");
        assert_eq!(caret(&b), (2, 0));
        let (b, _) = run("1\n2\n3\n4", "dj");
        assert_eq!(b.text(), "3\n4");
        let (b, _) = run("1\n2\n3\n4", "jdG");
        assert_eq!(b.text(), "1");
        let (b, _) = run("1\n2\n3\n4", "Gdgg");
        assert_eq!(b.text(), "");
        let (b, _) = run("1\n2", "yyP");
        assert_eq!(b.text(), "1\n1\n2");
        let (b, _) = run("ab", "ylp");
        assert_eq!(b.text(), "aab");
        let (b, _) = run("a\n  b\nc", "3J");
        assert_eq!(b.text(), "a b c");
        let (b, _) = run("a\nb", ">>j>>");
        assert_eq!(b.text(), "    a\n    b");
        let (b, _) = run("    a\nb", "<lt>j");
        assert_eq!(b.text(), "a\nb");
    }

    #[test]
    fn inserting_and_changing() {
        let (b, vim) = run("world", "ihello <esc>");
        assert_eq!(b.text(), "hello world");
        assert_eq!(caret(&b), (0, 5), "Esc steps back onto the last typed");
        assert_eq!(vim.mode(), VimMode::Normal);
        let (b, _) = run("abc", "Ad<esc>0ix<esc>");
        assert_eq!(b.text(), "xabcd");
        let (b, _) = run("one two", "cwnew<esc>");
        assert_eq!(b.text(), "new two");
        let (b, _) = run("one two", "wC2<esc>");
        assert_eq!(b.text(), "one 2");
        let (b, _) = run("  one\ntwo", "ccx<esc>");
        assert_eq!(b.text(), "  x\ntwo");
        let (b, _) = run("a\nc", "ob<esc>");
        assert_eq!(b.text(), "a\nb\nc");
        let (b, _) = run("b", "Oa<esc>");
        assert_eq!(b.text(), "a\nb");
        let (b, _) = run("abc", "rx");
        assert_eq!(b.text(), "xbc");
        let (b, _) = run("abc", "2rx");
        assert_eq!(b.text(), "xxc");
        let (b, _) = run("abc", "sz<esc>");
        assert_eq!(b.text(), "zbc");
    }

    #[test]
    fn undo_redo_and_repeat() {
        let (b, _) = run("abc", "xxu");
        assert_eq!(b.text(), "bc");
        let (b, _) = run("abc", "xxuu<c-r>");
        assert_eq!(b.text(), "bc");
        let (b, _) = run("a b c d", "dw..");
        assert_eq!(b.text(), "d");
        let (b, _) = run("x\ny\nz", "A!<esc>j.j.");
        assert_eq!(b.text(), "x!\ny!\nz!");
        let (b, _) = run("1 2 3 4 5 6", "2dw.");
        assert_eq!(b.text(), "5 6");
    }

    #[test]
    fn visual_modes() {
        let (b, vim) = run("hello world", "vll");
        assert_eq!(vim.mode(), VimMode::Visual);
        assert_eq!(b.selected_text().as_deref(), Some("hel"));
        let (b, _) = run("hello world", "vlld");
        assert_eq!(b.text(), "lo world");
        let (b, _) = run("hello world", "wvey0P");
        assert_eq!(b.text(), "worldhello world");
        let (b, _) = run("1\n2\n3", "Vjd");
        assert_eq!(b.text(), "3");
        let (b, vim) = run("1\n2\n3", "Vj<esc>");
        assert_eq!(vim.mode(), VimMode::Normal);
        assert!(b.selection().is_none());
        let (b, _) = run("hello", "$vhhc!<esc>");
        assert_eq!(b.text(), "he!");
    }

    #[test]
    fn shapes_labels_and_keys_for_the_app() {
        let mut buffer = Buffer::new("x");
        let mut vim = Vim::new();
        assert_eq!(vim.caret_shape(), Some(CaretShape::Block));
        let mut cx = ModeCx {
            buffer: &mut buffer,
            editable: true,
            single_line: false,
            page: 10,
        };
        assert_eq!(vim.key(Key::char(':'), &mut cx), ModeKey::Unused);
        assert_eq!(vim.key(Key::char('/'), &mut cx), ModeKey::Unused);
        assert_eq!(
            vim.key(Key::new(KeyCode::Escape), &mut cx),
            ModeKey::Unused,
            "Esc with nothing to cancel closes a dialog"
        );
        vim.key(Key::char('2'), &mut cx);
        vim.key(Key::char('d'), &mut cx);
        assert_eq!(vim.pending().as_deref(), Some("2d"));
        assert_eq!(vim.key(Key::new(KeyCode::Escape), &mut cx), ModeKey::Used);
        vim.key(Key::char('i'), &mut cx);
        assert_eq!(vim.caret_shape(), Some(CaretShape::Bar));
        assert_eq!(vim.label().as_deref(), Some("INSERT"));
        assert_eq!(vim.key(Key::char('q'), &mut cx), ModeKey::Pass);
        let mut read_only = ModeCx {
            buffer: &mut buffer,
            editable: false,
            single_line: false,
            page: 10,
        };
        let mut vim = Vim::new();
        vim.key(Key::char('x'), &mut read_only);
        vim.key(Key::char('d'), &mut read_only);
        vim.key(Key::char('d'), &mut read_only);
        assert_eq!(buffer.text(), "x", "read-only");
    }

    #[test]
    fn search_moves_with_n() {
        let mut buffer = Buffer::new("a x b x c x");
        buffer.set_search(Some(crate::edit::Search::plain("x")));
        let mut vim = Vim::new();
        type_keys(&mut buffer, &mut vim, "nnN");
        assert_eq!(caret(&buffer), (0, 2));
        type_keys(&mut buffer, &mut vim, "dn");
        assert_eq!(buffer.text(), "a x c x");
    }
}

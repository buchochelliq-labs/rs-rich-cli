//! A multi-line editor: [`text_area`].
//!
//! The editing (the caret moving by grapheme cluster, selection, undo and
//! redo, the character limit, how pasted text is cleaned) is
//! [`rich_interact::edit::Buffer`], which `rich_interact`'s own `TextArea`
//! uses too. This widget draws it: highlighted through the console's code
//! highlighter, with line numbers, wrapped or scrolled sideways, and with
//! the mouse.
//!
//! Only the lines in view are wrapped, highlighted and drawn, so a file of a
//! hundred thousand lines scrolls as fast as a short one. Highlighting
//! starts at the first line in view, so a construct that began above it (a
//! long comment, a multi-line string) is not known to the lines it covers.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::time::{Duration, Instant};

use rich::protocol::ConsoleCodeHighlighting;
use rich::{Console, Segment, Style};
use rich_interact::caret::CaretShape;
use rich_interact::edit::{graphemes, wrap_line, Buffer, Pos, WrapMode};
use rich_interact::edit_keys::{self, Act, ModeCx, ModeKey, Outcome};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::scrollbar::Bar;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// Two clicks this close together, on the same place, are a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A multi-line editor of `text`: what is typed is set on the signal, and
/// text set on the signal from elsewhere replaces what the area holds.
///
/// Its own options are builders on the node: [`Node::language`] highlights
/// it (through the console's code highlighter: syntect, or lumis when
/// registered), [`Node::line_numbers`], [`Node::soft_wrap`] (on by default;
/// off, long lines scroll sideways), [`Node::read_only`] and
/// [`Node::placeholder`].
///
/// Keys: the arrows, Home/End and PageUp/PageDown move, with Shift to
/// select and Ctrl to move by word (Ctrl+Home/End to the start and end);
/// Tab and Shift+Tab indent and outdent the selected lines (after Escape,
/// they move the focus on, as they always do read-only); Ctrl+A selects
/// everything; Ctrl+C copies the selection, Ctrl+X cuts it (both through
/// the app's clipboard, OSC 52); pasting goes through the terminal's
/// bracketed paste; Ctrl+Z undoes and Ctrl+Y or Ctrl+Shift+Z redoes, typed
/// runs as one step; Ctrl+W, Ctrl+U and Ctrl+K delete a word, to the line's
/// start and to its end. The mouse places the caret, drags a selection, and
/// a double or triple click selects a word or a line.
pub fn text_area(text: Signal<String>) -> Node {
    let area = TextArea::new(
        text,
        Options {
            soft_wrap: true,
            ..Options::default()
        },
    );
    widget(area).what("text_area")
}

/// What the node's builders set.
#[derive(Clone, Debug, Default)]
pub(crate) struct Options {
    pub language: Option<String>,
    pub line_numbers: bool,
    pub soft_wrap: bool,
    pub read_only: bool,
    pub placeholder: Option<String>,
    /// One line ([`input`](crate::input)): Enter submits, Tab moves on,
    /// pasted lines join with spaces.
    pub single_line: bool,
    /// Each character shown as this one (a password).
    pub mask: Option<char>,
    /// Where the caret goes when the text is set: [`Node::caret`]; `None`
    /// is the end for an input and the start for a text area.
    pub caret: Option<Caret>,
    /// What is selected when the text is set: [`Node::select`].
    pub select: Option<Range<usize>>,
    /// Wrapping, rows break at a space: [`Node::wrap_mode`].
    pub word_wrap: bool,
    /// The keys, the search, the caret and the rest:
    /// [`text_edit`](crate::text_edit).
    pub editing: crate::text_edit::Editing,
}

/// Where the caret of an [`input`](crate::input) or a
/// [`text_area`](crate::text_area) goes when its text is set (when it is
/// made, and when the signal is set from elsewhere): [`Node::caret`].
/// Positions count characters (grapheme clusters), a line break as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Caret {
    /// Before the first character: a text area's default.
    Start,
    /// After the last character: an input's default, as most toolkits
    /// have it.
    End,
    /// Before character `n` (after the last when `n` is past it).
    At(usize),
}

/// What an [`input`](crate::input) calls on Enter.
type Submit = Box<dyn FnMut(&str, &mut crate::app::Ctx)>;

/// One row on screen: a piece of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Row {
    line: usize,
    /// The byte range of the line it shows (all of it, unwrapped).
    start: usize,
    end: usize,
    /// Whether it is the line's first row (the one numbered).
    first: bool,
}

/// The highlighted lines last drawn: which lines, at which revision.
/// A line's highlighted spans: byte ranges and their styles.
type Spans = Vec<(Range<usize>, Style)>;

/// The styles a row is drawn with beyond the code's, and the search's
/// matches in view.
struct Looks {
    selection: Style,
    search: Style,
    current: Style,
    /// The matches on the lines in view, in order.
    matches: Vec<(Pos, Pos)>,
}

struct Highlight {
    key: (usize, usize, u64),
    lines: Vec<Spans>,
    default: Style,
}

pub(crate) struct TextArea {
    buffer: Buffer,
    /// The signal's value as last read or set, so a change made elsewhere
    /// is told from the area's own.
    seen: String,
    text: Signal<String>,
    pub(crate) options: Options,
    /// The first line shown, and its first row shown when wrapped.
    top: Cell<(usize, usize)>,
    /// Unwrapped: the first cell of each line shown.
    left: Cell<usize>,
    /// The caret moved: scroll it into view when next drawn.
    follow: Cell<bool>,
    size: Cell<(u16, u16)>,
    /// The rows last drawn, for placing clicks.
    rows: RefCell<Vec<Row>>,
    caret: Cell<Option<(u16, u16)>>,
    highlight: RefCell<Option<Highlight>>,
    /// The last press: when, where and how many in a row.
    click: Cell<Option<(Instant, Pos, u8)>>,
    dragging: Cell<bool>,
    /// Escape was the last key, so Tab leaves rather than indents.
    escaped: bool,
    /// Called with the text on Enter, in single-line mode.
    pub(crate) on_submit: Option<Submit>,
    pub(crate) bar: Bar,
    /// Where the bar was drawn, the lines it showed and the view's height;
    /// the column the text starts at.
    bar_at: Cell<Option<(Rect, usize, usize)>>,
    origin: Cell<u16>,
    /// An input's completion and history.
    pub(crate) assist: crate::complete::Assist,
}

impl TextArea {
    pub(crate) fn new(text: Signal<String>, options: Options) -> TextArea {
        let mut initial = text.get_untracked();
        if options.single_line {
            initial = one_line(&initial);
        }
        let mut area = TextArea {
            buffer: Buffer::new(&initial),
            seen: text.get_untracked(),
            text,
            options,
            top: Cell::new((0, 0)),
            left: Cell::new(0),
            follow: Cell::new(true),
            size: Cell::new((0, 0)),
            rows: RefCell::new(Vec::new()),
            caret: Cell::new(None),
            highlight: RefCell::new(None),
            click: Cell::new(None),
            dragging: Cell::new(false),
            escaped: false,
            on_submit: None,
            bar: Bar::default(),
            bar_at: Cell::new(None),
            origin: Cell::new(0),
            assist: Default::default(),
        };
        area.place_caret();
        area
    }

    /// The text was set: put the caret, or select, where the options say.
    fn place_caret(&mut self) {
        let pos = |buffer: &Buffer, n: usize| pos_of(buffer, n);
        if let Some(range) = self.options.select.clone() {
            let (start, end) = (pos(&self.buffer, range.start), pos(&self.buffer, range.end));
            self.buffer.select(start, end);
        } else {
            let caret = match self.options.caret {
                Some(caret) => caret,
                None if self.options.single_line => Caret::End,
                None => Caret::Start,
            };
            let at = match caret {
                Caret::Start => Pos::default(),
                Caret::End => pos(&self.buffer, usize::MAX),
                Caret::At(n) => pos(&self.buffer, n),
            };
            self.buffer.set_caret(at, false);
        }
        self.follow.set(true);
    }

    /// The cells `text` takes as shown: one a character when masked.
    fn shown_cells(&self, text: &str) -> usize {
        match self.options.mask {
            Some(_) => self.buffer.graphemes(text).len(),
            None => self.buffer.cells(text),
        }
    }

    pub(crate) fn buffer_ref(&self) -> &Buffer {
        &self.buffer
    }

    pub(crate) fn buffer_mut(&mut self) -> &mut Buffer {
        self.follow.set(true);
        &mut self.buffer
    }

    /// The signal it edits.
    pub(crate) fn text_signal(&self) -> Signal<String> {
        self.text
    }

    /// An option changed: show the caret again when next drawn.
    pub(crate) fn touch(&mut self) {
        self.follow.set(true);
    }

    pub(crate) fn set_wrap(&mut self, mode: WrapMode) {
        self.options.soft_wrap = mode != WrapMode::None;
        self.options.word_wrap = mode == WrapMode::Word;
        self.follow.set(true);
    }

    fn wrap(&self) -> WrapMode {
        match (self.options.soft_wrap, self.options.word_wrap) {
            (false, _) => WrapMode::None,
            (true, false) => WrapMode::Grapheme,
            (true, true) => WrapMode::Word,
        }
    }

    /// Write the key mode's name and pending keys to its status signal.
    pub(crate) fn tell_mode(&self) {
        let editing = &self.options.editing;
        let (Some(status), Some(mode)) = (editing.mode_status, editing.mode.as_ref()) else {
            return;
        };
        let mode = mode.borrow();
        let text = [mode.label(), mode.pending()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
        status.set(text);
    }

    /// Columns the line numbers take.
    fn gutter(&self) -> u16 {
        if self.options.line_numbers {
            self.buffer.line_count().to_string().len() as u16 + 3
        } else {
            0
        }
    }

    /// Cells a wrapped row may fill: one short of the text's width, so the
    /// caret fits after the last character.
    fn room(&self, width: u16) -> usize {
        (width.saturating_sub(self.gutter()) as usize)
            .saturating_sub(1)
            .max(1)
    }

    /// The rows line `index` takes, `room` cells wide when wrapping.
    fn line_rows(&self, index: usize, room: usize) -> Vec<Row> {
        let line = self.buffer.line(index);
        wrap_line(line, room, self.buffer.tab_width(), self.wrap())
            .into_iter()
            .enumerate()
            .map(|(i, range)| Row {
                line: index,
                start: range.start,
                end: range.end,
                first: i == 0,
            })
            .collect()
    }

    /// Which of `rows` (a line's) holds `at`.
    fn row_of(rows: &[Row], at: usize) -> usize {
        rows.iter()
            .position(|row| at >= row.start && at < row.end)
            .unwrap_or(rows.len() - 1)
    }

    /// Scroll so the caret is in view, `height` rows tall.
    fn follow_caret(&self, width: u16, height: usize) {
        let caret = self.buffer.caret();
        if !self.options.soft_wrap {
            let (top, _) = self.top.get();
            let top = if caret.line < top {
                caret.line
            } else if caret.line >= top + height {
                caret.line + 1 - height
            } else {
                top
            };
            self.top.set((top, 0));
            let text = (width.saturating_sub(self.gutter()) as usize).max(1);
            let column = self.shown_cells(&self.buffer.line(caret.line)[..caret.at]);
            let left = self.left.get();
            if column < left {
                self.left.set(column);
            } else if column >= left + text {
                self.left.set(column + 1 - text);
            }
            return;
        }
        let room = self.room(width);
        let rows = self.line_rows(caret.line, room);
        let here = (caret.line, Self::row_of(&rows, caret.at));
        let top = self.top.get();
        if here < top {
            self.top.set(here);
            return;
        }
        // Rows from the top to the caret's, stopping once past the height.
        let mut between = 0;
        let (mut line, mut sub) = top;
        // Each line wrapped once, not once a row: a long line has many.
        let mut count = self.line_rows(line, room).len();
        while (line, sub) < here && between < height {
            between += 1;
            sub += 1;
            if sub >= count {
                (line, sub) = (line + 1, 0);
                if line < self.buffer.line_count() {
                    count = self.line_rows(line, room).len();
                }
            }
        }
        if between < height {
            return;
        }
        // Back from the caret's row, height - 1 rows.
        let (mut line, mut sub) = here;
        for _ in 1..height {
            if sub > 0 {
                sub -= 1;
            } else if line > 0 {
                line -= 1;
                sub = self.line_rows(line, room).len() - 1;
            } else {
                break;
            }
        }
        self.top.set((line, sub));
    }

    /// The rows in view, from the top, at most `height`.
    fn view(&self, width: u16, height: usize) -> Vec<Row> {
        let room = self.room(width);
        let (mut line, sub) = self.top.get();
        line = line.min(self.buffer.line_count() - 1);
        let first = self.line_rows(line, room);
        // A sub-row of the width the line was wrapped at before: wider now,
        // it may have fewer rows.
        let sub = sub.min(first.len().saturating_sub(1));
        self.top.set((line, sub));
        let mut rows: Vec<Row> = first.into_iter().skip(sub).collect();
        while rows.len() < height && line + 1 < self.buffer.line_count() {
            line += 1;
            rows.extend(self.line_rows(line, room));
        }
        rows.truncate(height);
        rows
    }

    /// Each line of `lines` highlighted: byte ranges and their styles, and
    /// the style for what no range covers. `None` without a language.
    fn highlighted(&self, console: &Console, lines: Range<usize>) -> Option<()> {
        let language = self.options.language.as_deref()?;
        let key = (lines.start, lines.end, self.buffer.revision());
        if self
            .highlight
            .borrow()
            .as_ref()
            .is_some_and(|h| h.key == key)
        {
            return Some(());
        }
        let code = self.buffer.lines()[lines.clone()].join("\n");
        let (engine, theme) = match console.code_highlighting() {
            Some(chosen) => (chosen.highlighter.clone(), chosen.theme.clone()),
            None => (rich::SyntectHighlighter::shared(), None),
        };
        let theme = theme.unwrap_or_else(|| engine.default_theme().to_string());
        let out = engine.highlight(&code, Some(language), &theme).ok()?;
        *self.highlight.borrow_mut() = Some(Highlight {
            key,
            lines: out
                .lines
                .into_iter()
                .map(|line| {
                    line.spans
                        .into_iter()
                        .map(|span| (span.range, span.style))
                        .collect()
                })
                .collect(),
            default: out.default_style,
        });
        Some(())
    }

    /// As wide as the widest line, a cell for the caret, and the gutter, at
    /// most `u16::MAX`.
    fn natural_width(&self) -> u16 {
        let widest = self
            .buffer
            .lines()
            .iter()
            .map(|line| self.shown_cells(line))
            .max()
            .unwrap_or(0);
        (widest + 1 + usize::from(self.gutter())).min(u16::MAX as usize) as u16
    }

    /// The segments of `row`, from cell `left` (unwrapped), at most `width`
    /// cells: its text in the highlighter's styles, tabs as spaces, the
    /// selection reversed.
    fn row_segments(
        &self,
        row: Row,
        first_line: usize,
        left: usize,
        width: usize,
        looks: &Looks,
    ) -> Vec<Segment> {
        let selection = &looks.selection;
        let line = self.buffer.line(row.line);
        let highlight = self.highlight.borrow();
        let spans: Option<(&Spans, &Style)> = highlight
            .as_ref()
            .and_then(|h| Some((h.lines.get(row.line - first_line)?, &h.default)));
        let selected = self.buffer.selection().and_then(|(start, end)| {
            (start.line <= row.line && row.line <= end.line).then(|| {
                let from = if start.line == row.line { start.at } else { 0 };
                let to = if end.line == row.line {
                    end.at
                } else {
                    line.len() + 1
                };
                from..to
            })
        });
        // The selection is one colour, over the code's default style rather
        // than each token's.
        let selected_style = match spans {
            Some((_, default)) => default.combine(selection),
            None => selection.clone(),
        };
        // The search's matches on this row's line, and which is selected.
        let current = self.buffer.selection();
        let matches: Vec<(Range<usize>, bool)> = looks
            .matches
            .iter()
            .filter(|(start, _)| start.line == row.line)
            .map(|&(start, end)| (start.at..end.at, Some((start, end)) == current))
            .collect();
        let over = |base: Option<Style>, look: &Style| match base {
            Some(base) => base.combine(look),
            None => look.clone(),
        };
        let mut out: Vec<Segment> = Vec::new();
        let mut column = 0;
        let mut shown = 0;
        let mask = self.options.mask.map(String::from);
        for (start, end, cell_width) in self.buffer.graphemes(&line[row.start..row.end]) {
            let cell_width = if mask.is_some() { 1 } else { cell_width };
            let (start, end) = (row.start + start, row.start + end);
            if column + cell_width <= left {
                column += cell_width;
                continue;
            }
            if shown + cell_width > width {
                break;
            }
            let mut text = &line[start..end];
            let padded;
            if let Some(mask) = &mask {
                text = mask;
            } else if text == "\t" || column < left {
                // A tab, or a wide character cut by the left edge.
                padded = " ".repeat(cell_width.min(column + cell_width - left));
                text = &padded;
            }
            let mut style = spans.and_then(|(spans, default)| {
                spans
                    .iter()
                    .find(|(range, _)| range.contains(&start))
                    .map(|(_, style)| style.clone())
                    .or_else(|| Some(default.clone()))
            });
            if let Some((_, now)) = matches.iter().find(|(range, _)| range.contains(&start)) {
                let look = if *now { &looks.current } else { &looks.search };
                style = Some(over(style, look));
            }
            if selected
                .as_ref()
                .is_some_and(|range| range.contains(&start))
                && !matches
                    .iter()
                    .any(|(range, now)| *now && range.contains(&start))
            {
                style = Some(selected_style.clone());
            }
            let width = self.buffer.cells(text).max(if text.trim().is_empty() {
                text.len()
            } else {
                0
            });
            match out.last_mut() {
                Some(last) if last.style == style => last.text.push_str(text),
                _ => out.push(Segment::new(text.to_string(), style)),
            }
            column += cell_width;
            shown += width.max(cell_width.min(width.max(1)));
        }
        // A selection that runs on past the line's end shows one cell of it.
        if selected
            .as_ref()
            .is_some_and(|range| range.contains(&line.len()) && range.end > line.len())
            && row.end == line.len()
            && shown < width
        {
            out.push(Segment::new(" ", Some(selected_style)));
        }
        out
    }

    /// Where a click at `column`, `row` (the widget's own) lands.
    fn pos_at(&self, column: u16, row: u16) -> Pos {
        let rows = self.rows.borrow();
        let Some(&shown) = rows.get(row as usize).or_else(|| rows.last()) else {
            return self.buffer.caret();
        };
        if row as usize >= rows.len() {
            return Pos::new(shown.line, shown.end);
        }
        let line = self.buffer.line(shown.line);
        let left = if self.options.soft_wrap {
            0
        } else {
            self.left.get()
        };
        let target = (column
            .saturating_sub(self.origin.get())
            .saturating_sub(self.gutter()) as usize)
            + left;
        let mut at = shown.start;
        let mut used = 0;
        for (start, end, width) in self.buffer.graphemes(&line[shown.start..shown.end]) {
            let width = if self.options.mask.is_some() {
                1
            } else {
                width
            };
            // Past the middle of a character: after it.
            if used + width.div_ceil(2) > target {
                break;
            }
            used += width;
            at = shown.start + end;
            let _ = start;
        }
        Pos::new(shown.line, at)
    }

    /// Tell the signal what the area holds, if it changed.
    fn publish(&mut self, revision: u64) {
        if self.buffer.revision() != revision {
            let text = self.buffer.text();
            self.seen = text.clone();
            self.text.set(text);
        }
        self.follow.set(true);
    }

    fn page(&self) -> isize {
        self.size.get().1.max(1) as isize
    }

    /// A key: whether it was used.
    fn key(&mut self, cx: &mut EventCx, key: Key) -> bool {
        let (ctrl, alt) = (key.modifiers.ctrl, key.modifiers.alt);
        // Escape, then Tab or Shift+Tab, moves the focus on, so the area is
        // never a trap; read-only, Tab always does.
        let escaped = std::mem::replace(&mut self.escaped, key.code == KeyCode::Escape);
        let edit = !self.options.read_only;
        if self.options.single_line {
            if let Some(used) = self.assist_key(key) {
                return used;
            }
            match key.code {
                // Enter submits, with a handler; Tab moves on, and up, down
                // and the page keys go to the app (a form).
                KeyCode::Enter if !ctrl && !alt => {
                    if let Some(history) = self.assist.history() {
                        history.push(&self.buffer.text());
                    }
                    let Some(submit) = self.on_submit.as_mut() else {
                        return false;
                    };
                    submit(&self.buffer.text(), cx.app());
                    return true;
                }
                KeyCode::Enter
                | KeyCode::Tab
                | KeyCode::BackTab
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::PageUp
                | KeyCode::PageDown => return false,
                _ => {}
            }
        }
        // A modal preset sees the key first.
        if let Some(mode) = self.options.editing.mode.clone() {
            let page = self.page().max(1) as usize;
            let out = mode.borrow_mut().key(
                key,
                &mut ModeCx {
                    buffer: &mut self.buffer,
                    editable: edit,
                    single_line: self.options.single_line,
                    page,
                },
            );
            match out {
                ModeKey::Used => return true,
                ModeKey::Unused => return false,
                ModeKey::Copy(text) => {
                    if self.options.mask.is_none() {
                        cx.app().copy(text);
                    }
                    return true;
                }
                ModeKey::Pass => {}
            }
        }
        // Masked, nothing is copied out.
        let masked = self.options.mask.is_some();
        let Some((action, select)) = edit_keys::resolve(&self.options.editing.keymap, key) else {
            return match key.code {
                KeyCode::Char(c) if !ctrl && !alt && edit => {
                    let mut bytes = [0; 4];
                    self.buffer.insert(c.encode_utf8(&mut bytes));
                    true
                }
                _ => false,
            };
        };
        let b = &mut self.buffer;
        match action.as_str() {
            // Masked, a selection takes Ctrl+C and Ctrl+X and gives nothing.
            "copy" | "cut" if masked => return b.selection().is_some(),
            "copy" => match b.selected_text() {
                Some(text) => {
                    b.push_kill(&text);
                    cx.app().copy(text);
                    // Copying ends the mark (Emacs's Alt+W); a plain
                    // selection stays.
                    if b.marking() {
                        b.clear_mark();
                    }
                }
                // Nothing selected: Ctrl+C is the app's (it quits).
                None => return false,
            },
            "cut" if edit => match b.selected_text() {
                Some(text) => {
                    cx.app().copy(text);
                    b.kill_selection();
                }
                None => return false,
            },
            // After Escape, Tab and Shift+Tab move the focus on.
            "indent" | "outdent" if escaped => return false,
            action => {
                let act = Act {
                    select,
                    editable: edit,
                    single_line: self.options.single_line,
                    page: self.page().max(1) as usize,
                };
                if edit_keys::perform(&mut self.buffer, action, act) != Outcome::Used {
                    return false;
                }
            }
        }
        true
    }
}

impl TextArea {
    /// The selection, if any.
    pub(crate) fn buffer_selection(&self) -> Option<(Pos, Pos)> {
        self.buffer.selection()
    }

    /// The caret's byte in the (single-line) text.
    fn caret_byte(&self) -> usize {
        self.buffer.caret().at
    }

    /// The candidates for what is typed now.
    fn candidates(&self) -> Vec<crate::complete::Completion> {
        (self.assist).candidates(&self.buffer.text(), self.caret_byte())
    }

    /// Put `completion` in, the caret after it.
    fn take(&mut self, completion: &crate::complete::Completion) {
        let range = &completion.replace;
        self.buffer.set_caret(Pos::new(0, range.start), false);
        if range.end > range.start {
            self.buffer.set_caret(Pos::new(0, range.end), true);
        }
        self.buffer.insert(&completion.text);
        self.assist.popup = None;
    }

    /// The candidate the ghost and → take: the selected one in the pop-up,
    /// else the top one; with its rest, when it continues what is typed up
    /// to the caret at the end.
    fn ghost(&mut self) -> Option<(crate::complete::Completion, String)> {
        if !self.assist.options.ghost || self.buffer.selection().is_some() {
            return None;
        }
        let text = self.buffer.text();
        if self.caret_byte() != text.len() {
            return None;
        }
        let revision = self.buffer.revision();
        let candidate = match &self.assist.popup {
            Some(popup) => popup.items.get(popup.selected).cloned(),
            None => {
                if self.assist.top.as_ref().map(|t| t.0) != Some(revision) {
                    let top = self.candidates().into_iter().next();
                    self.assist.top = Some((revision, top));
                }
                self.assist.top.as_ref().and_then(|t| t.1.clone())
            }
        }?;
        if candidate.replace.end != text.len() {
            return None;
        }
        let typed = &text[candidate.replace.clone()];
        let rest = candidate
            .text
            .get(typed.len()..)
            .filter(|_| {
                candidate
                    .text
                    .to_lowercase()
                    .starts_with(&typed.to_lowercase())
            })?
            .to_string();
        (!rest.is_empty()).then_some((candidate, rest))
    }

    /// A key for the completion or the history: `Some` when one used it
    /// (or wants it unused).
    fn assist_key(&mut self, key: Key) -> Option<bool> {
        let plain = !key.modifiers.ctrl && !key.modifiers.alt;
        // Any key but ↑ and ↓ ends browsing the history, where it is.
        if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
            if let (Some(_), Some(history)) = (self.assist.browse.take(), self.assist.history()) {
                history.set_status(None);
            }
        }
        let rows = self.assist.options.rows as usize;
        if let Some(popup) = self.assist.popup.as_mut() {
            match key.code {
                KeyCode::Tab | KeyCode::Down => popup.step(1, rows),
                KeyCode::BackTab | KeyCode::Up => popup.step(-1, rows),
                KeyCode::PageDown => popup.step(rows as isize, rows),
                KeyCode::PageUp => popup.step(-(rows as isize), rows),
                KeyCode::Enter if plain => {
                    let chosen = popup.items.get(popup.selected).cloned();
                    match chosen {
                        Some(chosen) => self.take(&chosen),
                        None => self.assist.popup = None,
                    }
                }
                KeyCode::Escape => self.assist.popup = None,
                _ => return None,
            }
            return Some(true);
        }
        if self.assist.completer.is_some() && plain && !self.options.read_only {
            match key.code {
                KeyCode::Tab if !key.modifiers.shift => {
                    let mut items = self.candidates();
                    match items.len() {
                        0 => return None,
                        1 => self.take(&items.remove(0)),
                        _ => {
                            self.assist.popup = Some(crate::complete::Popup {
                                items,
                                selected: 0,
                                top: 0,
                            })
                        }
                    }
                    return Some(true);
                }
                KeyCode::Right | KeyCode::End if !key.modifiers.shift => {
                    if let Some((chosen, _)) = self.ghost() {
                        self.take(&chosen);
                        return Some(true);
                    }
                }
                _ => {}
            }
        }
        if let (Some(history), true) = (self.assist.history(), plain) {
            let older = match key.code {
                KeyCode::Up => true,
                KeyCode::Down if self.assist.browse.is_some() => false,
                _ => return None,
            };
            let (at, typed) = self
                .assist
                .browse
                .clone()
                .unwrap_or_else(|| (0, self.buffer.text()));
            let found = history.matching(&typed);
            if found.is_empty() {
                return None;
            }
            let next = if older {
                (at + 1).min(found.len())
            } else {
                at.saturating_sub(1)
            };
            let shown = match next {
                0 => typed.clone(),
                n => found[n - 1].clone(),
            };
            if shown != self.buffer.text() {
                self.buffer.select_all();
                self.buffer.insert(&shown);
            }
            if next == 0 {
                self.assist.browse = None;
                history.set_status(None);
            } else {
                self.assist.browse = Some((next, typed));
                history.set_status(Some((next, found.len())));
            }
            return Some(true);
        }
        None
    }

    /// What is typed changed: the pop-up follows it (closing when nothing
    /// is left), or opens as you type.
    fn edited(&mut self) {
        if self.assist.completer.is_none() {
            return;
        }
        if self.assist.popup.is_some() || self.assist.options.auto {
            let items = self.candidates();
            self.assist.popup = (!items.is_empty()).then_some(crate::complete::Popup {
                items,
                selected: 0,
                top: 0,
            });
        }
    }

    /// An input's validation message: under the text, or with one row,
    /// at the right of it where it fits.
    fn draw_message(&self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let Some(validate) = &self.options.editing.validate else {
            return;
        };
        let Some(message) = self.text.with(|text| validate(text)) else {
            return;
        };
        let style = cx.style("input.error", "red");
        let width = canvas.width() as usize;
        if canvas.height() >= 2 {
            let shown = rich::cells::set_cell_size(&message, width);
            canvas.print(0, 1, shown.trim_end(), Some(&style));
            return;
        }
        let used = self.shown_cells(&self.buffer.text()) + 1;
        let size = rich::cells::cell_len(&message);
        if used + 1 + size <= width {
            canvas.print((width - size) as u16, 0, &message, Some(&style));
        }
    }

    /// Draw the ghost after the caret, and set the pop-up's float.
    fn draw_assist(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if !self.options.single_line || self.assist.completer.is_none() {
            return;
        }
        let focused = cx.focused();
        if !focused {
            self.assist.popup = None;
        }
        if let (true, Some((x, y)), Some((_, rest))) = (focused, self.caret.get(), self.ghost()) {
            let style = cx.style("input.ghost", "dim");
            canvas.print(x, y, &rest, Some(&style));
        }
        let Some(popup) = self.assist.popup.clone() else {
            if std::mem::take(&mut self.assist.floated) {
                cx.float(None);
            }
            return;
        };
        let rows = self.assist.options.rows as usize;
        let cells = rich::cells::cell_len;
        let widest = popup
            .items
            .iter()
            .map(|c| cells(&c.text) + c.detail.as_deref().map_or(0, |d| cells(d) + 2))
            .max()
            .unwrap_or(0)
            .min(46);
        let width = widest + 2;
        let base = cx.style("completion", "on grey23");
        let selected = cx.style("completion.selected", "reverse");
        let detail = base.combine(&cx.style("completion.detail", "dim"));
        let mut lines = Vec::new();
        for (i, item) in popup.items.iter().enumerate().skip(popup.top).take(rows) {
            let look = if i == popup.selected {
                &selected
            } else {
                &base
            };
            let mut line = vec![Segment::new(" ", Some(look.clone()))];
            let text_room = widest
                - item
                    .detail
                    .as_deref()
                    .map_or(0, |d| cells(d) + 2)
                    .min(widest);
            let shown = rich::cells::set_cell_size(
                &item.text,
                text_room.max(cells(&item.text).min(widest)),
            );
            let used = cells(&shown);
            line.push(Segment::new(shown, Some(look.clone())));
            if let Some(d) = &item.detail {
                let room = widest.saturating_sub(used);
                let d = rich::cells::set_cell_size(&format!("  {d}"), room);
                let look = if i == popup.selected {
                    selected.clone()
                } else {
                    detail.clone()
                };
                line.push(Segment::new(d, Some(look)));
            } else {
                line.push(Segment::new(
                    " ".repeat(widest - used.min(widest)),
                    Some(look.clone()),
                ));
            }
            line.push(Segment::new(" ", Some(look.clone())));
            lines.push(line);
        }
        // Under the start of the text it replaces.
        let start = popup.items[popup.selected.min(popup.items.len() - 1)]
            .replace
            .start;
        let text = self.buffer.text();
        let left = if self.options.soft_wrap {
            0
        } else {
            self.left.get()
        };
        let column = self
            .shown_cells(&text[..start.min(text.len())])
            .saturating_sub(left)
            .min(canvas.width() as usize) as u16;
        let row = self.caret.get().map_or(0, |(_, y)| y);
        let (x, y) = cx.to_screen(self.origin.get() + column, row);
        let (ax, _) = cx.to_screen(0, row);
        cx.float(Some(crate::float::Float {
            anchor: Rect::new(ax, y, canvas.width(), 1),
            x: x.saturating_sub(1),
            width: width.min(u16::MAX as usize) as u16,
            lines,
        }));
        self.assist.floated = true;
    }
}

impl Widget for TextArea {
    fn name(&self) -> &'static str {
        "text_area"
    }

    fn describe(&self) -> Option<String> {
        let lines = self.buffer.line_count();
        Some(match &self.options.language {
            Some(language) => format!("{lines} lines, {language}"),
            None => format!("{lines} lines"),
        })
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => self.natural_width(),
            // An input that validates keeps a row for its message.
            Axis::Vertical if self.options.single_line => {
                1 + u16::from(self.options.editing.validate.is_some())
            }
            Axis::Vertical => {
                let room = self.room(width.max(1));
                (0..self.buffer.line_count())
                    .map(|i| self.line_rows(i, room).len())
                    .sum::<usize>()
                    .min(u16::MAX as usize) as u16
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        // Text set on the signal from elsewhere replaces the area's.
        let changed = self.text.with(|text| *text != self.seen);
        if changed {
            self.seen = self.text.get_untracked();
            if self.options.single_line {
                self.buffer.set_text(&one_line(&self.seen));
            } else {
                self.buffer.set_text(&self.seen);
            }
            self.top.set((0, 0));
            self.left.set(0);
            self.place_caret();
            self.assist.popup = None;
            self.assist.browse = None;
        }
        // The search the app drives: a new pattern or a move selects.
        if crate::text_edit::apply_search(self) {
            self.follow.set(true);
        }
        let (mut width, height) = (canvas.width(), canvas.height());
        // The bar keeps a column while the lines do not all fit.
        let look = (self.bar.look.clone())
            .filter(|look| look.wanted(self.buffer.line_count(), height as usize));
        let mut bar = None;
        let mut origin = 0;
        if let Some(look) = &look {
            let (rest, at) = look.split(width, height);
            (origin, width) = (rest.x, rest.width);
            bar = Some(at);
        }
        self.origin.set(origin);
        self.size.set((width, height));
        if self.follow.replace(false) {
            self.follow_caret(width, height as usize);
        }
        let rows = self.view(width, height as usize);
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            if self
                .highlighted(cx.console(), first.line..last.line + 1)
                .is_none()
            {
                *self.highlight.borrow_mut() = None;
            }
        }
        let first_line = rows.first().map_or(0, |row| row.line);
        let gutter = self.gutter();
        let text_width = width.saturating_sub(gutter) as usize;
        let left = if self.options.soft_wrap {
            0
        } else {
            self.left.get()
        };
        let number = cx.style("text_area.line_number", "dim");
        let hint = cx.style("text_area.placeholder", "dim");
        let looks = Looks {
            selection: cx.style("text_area.selection", "reverse"),
            search: cx.style("text_area.search", "black on yellow"),
            current: cx.style("text_area.search.current", "bold black on bright_yellow"),
            matches: match (rows.first(), rows.last()) {
                (Some(first), Some(last)) if self.buffer.search().is_some() => {
                    self.buffer.matches(first.line..last.line + 1)
                }
                _ => Vec::new(),
            },
        };
        let cursor_line = (self.options.editing.cursor_line)
            .then(|| cx.style("text_area.cursor_line", "underline"));
        // Highlighted, every row is filled with the code's own background.
        let fill = self.highlight.borrow().as_ref().map(|h| h.default.clone());
        let blank_gutter = (gutter > 0).then(|| {
            Segment::new(
                format!("{} │ ", " ".repeat(gutter as usize - 3)),
                Some(number.clone()),
            )
        });
        let caret = self.buffer.caret();
        let mut caret_cell = None;
        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(rows.len());
        for (y, row) in rows.iter().enumerate() {
            let mut line = Vec::new();
            if gutter > 0 {
                let digits = gutter as usize - 3;
                let label = if row.first {
                    format!("{:>digits$} │ ", row.line + 1)
                } else {
                    format!("{} │ ", " ".repeat(digits))
                };
                line.push(Segment::new(label, Some(number.clone())));
            }
            if self.buffer.is_empty() {
                if let Some(placeholder) = &self.options.placeholder {
                    line.push(Segment::new(placeholder.clone(), Some(hint.clone())));
                }
            } else {
                line.extend(self.row_segments(*row, first_line, left, text_width, &looks));
            }
            let used = |line: &[Segment]| -> usize {
                line.iter().map(|s| rich::cells::cell_len(&s.text)).sum()
            };
            if let Some(fill) = &fill {
                let room = width as usize - used(&line).min(width as usize);
                line.push(Segment::new(" ".repeat(room), Some(fill.clone())));
            }
            if let (Some(look), true) = (&cursor_line, row.line == caret.line) {
                for segment in line.iter_mut().skip(usize::from(gutter > 0)) {
                    segment.style = Some(match &segment.style {
                        Some(style) => style.combine(look),
                        None => look.clone(),
                    });
                }
                let room = width as usize - used(&line).min(width as usize);
                line.push(Segment::new(" ".repeat(room), Some(look.clone())));
            }
            if row.line == caret.line {
                let last_of_line = row.end == self.buffer.line(row.line).len();
                let here = caret.at >= row.start && (caret.at < row.end || last_of_line);
                if here {
                    let column = self.shown_cells(&self.buffer.line(row.line)[row.start..caret.at]);
                    let column = column.saturating_sub(left);
                    if column <= text_width {
                        let x = origin + gutter + column.min(u16::MAX as usize) as u16;
                        caret_cell = Some((x, y as u16));
                    }
                }
            }
            lines.push(line);
        }
        if let Some(fill) = &fill {
            for _ in lines.len()..height as usize {
                let mut line: Vec<Segment> = blank_gutter.iter().cloned().collect();
                line.push(Segment::new(" ".repeat(text_width), Some(fill.clone())));
                lines.push(line);
            }
        }
        match (look, bar) {
            (Some(look), Some(at)) => {
                canvas.lines_at(origin, 0, width, height, &lines);
                let (top, _) = self.top.get();
                let (count, view) = (self.buffer.line_count(), height as usize);
                look.paint(cx.console(), canvas, at, top, count, view);
                self.bar_at.set(Some((at, count, view)));
            }
            _ => {
                canvas.lines(&lines);
                self.bar_at.set(None);
            }
        }
        // The caret drawn in the cells, rather than the terminal's.
        let drawn = self.options.editing.draw_caret;
        if let (true, true, Some((x, y))) = (drawn, cx.focused(), caret_cell) {
            let fallback = match self.caret_shape() {
                Some(CaretShape::Underline) => "underline",
                _ => "reverse",
            };
            let look = cx.style("text_area.caret", fallback);
            let (symbol, style) = match canvas.cell(x, y) {
                Some(cell) if !cell.symbol().is_empty() && !cell.is_continuation() => {
                    (cell.symbol().to_string(), cell.style().cloned())
                }
                _ => (" ".to_string(), None),
            };
            let style = match style {
                Some(style) => style.combine(&look),
                None => look,
            };
            canvas.set_cell(x, y, &symbol, Some(&style));
        }
        self.caret.set(caret_cell.filter(|_| !drawn));
        *self.rows.borrow_mut() = rows;
        self.draw_message(cx, canvas);
        self.draw_assist(cx, canvas);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let revision = self.buffer.revision();
        // A press or a drag on the bar scrolls the view by lines.
        if let (WidgetEvent::Mouse(mouse), Some((rect, count, view))) = (event, self.bar_at.get()) {
            let (top, _) = self.top.get();
            match self.bar.mouse(cx, mouse, rect, top, count, view) {
                Some(Some(to)) => {
                    self.top.set((to, 0));
                    cx.redraw();
                    return Used::Yes;
                }
                Some(None) => return Used::Yes,
                None => {}
            }
        }
        match event {
            WidgetEvent::Key(key) => {
                let used = self.key(cx, *key);
                self.tell_mode();
                if !used {
                    return Used::No;
                }
                if self.buffer.revision() != revision && self.assist.browse.is_none() {
                    self.edited();
                }
            }
            WidgetEvent::Focus(false) if self.assist.popup.is_some() => {
                self.assist.popup = None;
                cx.redraw();
                return Used::No;
            }
            WidgetEvent::Paste(text) if !self.options.read_only => {
                if self.options.single_line {
                    self.buffer.insert(&one_line(text));
                } else {
                    self.buffer.insert(text);
                }
            }
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(Button::Left) => {
                    let pos = self.pos_at(mouse.column, mouse.row);
                    let now = Instant::now();
                    let count = match self.click.get() {
                        Some((at, last, count))
                            if last == pos && now.duration_since(at) < DOUBLE_CLICK =>
                        {
                            count % 3 + 1
                        }
                        _ => 1,
                    };
                    self.click.set(Some((now, pos, count)));
                    match count {
                        2 => {
                            let (start, end) = self.buffer.word_at(pos);
                            self.buffer.select(start, end);
                        }
                        3 => {
                            let last = self.buffer.line_count() - 1;
                            let end = if pos.line < last {
                                Pos::new(pos.line + 1, 0)
                            } else {
                                Pos::new(pos.line, self.buffer.line(pos.line).len())
                            };
                            self.buffer.select(Pos::new(pos.line, 0), end);
                        }
                        _ => {
                            self.buffer.set_caret(pos, mouse.modifiers.shift);
                            self.dragging.set(true);
                            cx.capture_mouse();
                        }
                    }
                }
                MouseKind::Drag(Button::Left) if self.dragging.get() => {
                    let pos = self.pos_at(mouse.column, mouse.row);
                    self.buffer.set_caret(pos, true);
                }
                MouseKind::Up(_) if self.dragging.get() => {
                    self.dragging.set(false);
                    cx.release_mouse();
                }
                MouseKind::ScrollUp | MouseKind::ScrollDown => {
                    let down = mouse.kind == MouseKind::ScrollDown;
                    let (top, _) = self.top.get();
                    let last = self.buffer.line_count() - 1;
                    let line = if down {
                        (top + 3).min(last)
                    } else {
                        top.saturating_sub(3)
                    };
                    // Down from inside the last line stays where it is,
                    // rather than going back to the line's first row.
                    if !(down && line == top) {
                        self.top.set((line, 0));
                    }
                    cx.redraw();
                    return Used::Yes;
                }
                _ => return Used::No,
            },
            _ => return Used::No,
        }
        self.publish(revision);
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }

    fn claims_key(&self, key: Key) -> bool {
        // Ctrl+Z undoes here, rather than suspending the app; Ctrl+C copies
        // a selection, and with none, quits as it would anywhere.
        (key == Key::ctrl('z') && !self.options.read_only)
            || (key == Key::ctrl('c') && self.buffer.selection().is_some())
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.caret.get()
    }

    fn caret_shape(&self) -> Option<CaretShape> {
        self.options
            .editing
            .mode
            .as_ref()
            .and_then(|mode| mode.borrow().caret_shape())
            .or(self.options.editing.caret_shape)
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    /// The caret's row: in accessible mode the terminal's cursor goes there,
    /// and in linear mode it is the line read out.
    fn cursor(&self) -> Option<Rect> {
        let (_, y) = self.caret.get()?;
        Some(Rect::new(self.origin.get(), y, self.size.get().0, 1))
    }
}

/// Where character `n` of `buffer`'s text is (grapheme clusters, a line
/// break counting one); the end when `n` is past it.
fn pos_of(buffer: &Buffer, mut n: usize) -> Pos {
    let last = buffer.line_count() - 1;
    for index in 0..=last {
        let line = buffer.line(index);
        let stops = graphemes(line);
        if n <= stops.len() {
            let at = stops.get(n).map_or(line.len(), |(start, _, _)| *start);
            return Pos::new(index, at);
        }
        if index == last {
            return Pos::new(index, line.len());
        }
        n -= stops.len() + 1;
    }
    Pos::default()
}

/// `text` on one line: line breaks become spaces.
fn one_line(text: &str) -> String {
    text.replace("\r\n", " ").replace(['\n', '\r'], " ")
}

/// A [`text_area`]'s options. On any other node they do nothing.
impl Node {
    /// Highlight it as `language` (a name or file extension the console's
    /// code highlighter knows: `"rust"`, `"py"`).
    pub fn language(self, language: &str) -> Node {
        self.text_area_options(|options| options.language = Some(language.to_string()))
    }

    /// Number its lines in a gutter on the left.
    pub fn line_numbers(self) -> Node {
        self.text_area_options(|options| options.line_numbers = true)
    }

    /// Wrap long lines (the default), or with `false` scroll sideways to
    /// the caret.
    pub fn soft_wrap(self, on: bool) -> Node {
        self.text_area_options(|options| options.soft_wrap = on)
    }

    /// Let the caret move and text be selected and copied, but not edited.
    pub fn read_only(self) -> Node {
        self.text_area_options(|options| options.read_only = true)
    }

    /// Show `text`, dimmed, while it is empty.
    pub fn placeholder(self, text: &str) -> Node {
        self.text_area_options(|options| options.placeholder = Some(text.to_string()))
    }

    /// Show each character as `mask` (`'•'`, a password), while the signal
    /// holds the text. Nothing is copied out of it.
    pub fn mask(self, mask: char) -> Node {
        self.text_area_options(|options| options.mask = Some(mask))
    }

    /// Take at most `chars` characters; typing and pasting stop there.
    pub fn max_length(self, chars: usize) -> Node {
        self.widget_mut(|area: &mut TextArea| area.buffer.set_limit(Some(chars)));
        self
    }

    /// For an [`input`](crate::input): call `submit` with the text on Enter,
    /// which it then uses, so Enter does not bubble to a binding. Without
    /// one, Enter goes on to the app (a form's own binding).
    pub fn on_submit(self, submit: impl FnMut(&str, &mut crate::app::Ctx) + 'static) -> Node {
        self.widget_mut(|area: &mut TextArea| area.on_submit = Some(Box::new(submit)));
        self
    }

    /// Where the caret goes when the text is set, as it is made and when
    /// the signal is set from elsewhere: [`Caret::End`] (an input's
    /// default), [`Caret::Start`] (a text area's) or [`Caret::At`] a
    /// character.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::Caret;
    ///
    /// let app = App::new(|| input(signal("report.txt".to_string())).caret(Caret::At(6)));
    /// let mut driver = app.driver(20, 1);
    /// driver.type_text("-v2");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "report-v2.txt");
    /// ```
    pub fn caret(self, caret: Caret) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            area.options.caret = Some(caret);
            area.place_caret();
        });
        self
    }

    /// Select characters `range` (grapheme clusters, a line break counting
    /// one) when the text is set, the caret at its end: typing replaces
    /// them. A rename that selects the name before the extension:
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| input(signal("report.txt".to_string())).select(0..6));
    /// let mut driver = app.driver(20, 1);
    /// driver.type_text("notes");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "notes.txt");
    /// ```
    pub fn select(self, range: Range<usize>) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            area.options.select = Some(range);
            area.place_caret();
        });
        self
    }

    fn text_area_options(self, f: impl FnOnce(&mut Options)) -> Node {
        self.widget_mut(|area: &mut TextArea| {
            f(&mut area.options);
            area.follow.set(true);
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(text: &str) -> TextArea {
        TextArea {
            buffer: Buffer::new(text),
            seen: text.to_string(),
            text: crate::reactive::Runtime::new()
                .enter(|| crate::reactive::signal(text.to_string())),
            options: Options {
                soft_wrap: true,
                ..Options::default()
            },
            top: Cell::new((0, 0)),
            left: Cell::new(0),
            follow: Cell::new(true),
            size: Cell::new((0, 0)),
            rows: RefCell::new(Vec::new()),
            caret: Cell::new(None),
            highlight: RefCell::new(None),
            click: Cell::new(None),
            dragging: Cell::new(false),
            escaped: false,
            on_submit: None,
            bar: Bar::default(),
            bar_at: Cell::new(None),
            origin: Cell::new(0),
            assist: Default::default(),
        }
    }

    #[test]
    fn a_wider_view_clamps_a_stale_wrapped_row() {
        let area = area(&"x".repeat(40));
        // Scrolled to the fourth row of the line wrapped at 10 columns...
        area.top.set((0, 3));
        // ...then the terminal widens: the line is one row, and it shows.
        let rows = area.view(100, 5);
        assert_eq!(rows.len(), 1);
        assert_eq!(area.top.get(), (0, 0));
    }

    #[test]
    fn the_natural_width_counts_the_gutter_before_clamping() {
        let mut area = area(&"x".repeat(70_000));
        area.options.line_numbers = true;
        assert_eq!(area.natural_width(), u16::MAX);
        let mut short = super::tests::area("abc");
        short.options.line_numbers = true;
        assert_eq!(short.natural_width(), 4 + short.gutter());
    }

    #[test]
    fn long_lines_wrap_one_cell_short() {
        let area = area("abcdefghij\nxy");
        // 10 columns, one kept for the caret: 9.
        let rows = area.view(10, 5);
        let pieces: Vec<(usize, usize, usize)> =
            rows.iter().map(|r| (r.line, r.start, r.end)).collect();
        assert_eq!(pieces, [(0, 0, 9), (0, 9, 10), (1, 0, 2)]);
    }

    #[test]
    fn only_the_rows_in_view_are_wrapped() {
        let text = vec!["x"; 100_000].join("\n");
        let area = area(&text);
        area.top.set((50_000, 0));
        let started = Instant::now();
        let rows = area.view(40, 20);
        assert_eq!(rows.len(), 20);
        assert_eq!(rows[0].line, 50_000);
        assert!(started.elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn the_caret_scrolls_into_view_from_either_side() {
        let text = (0..100)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut area = area(&text);
        area.buffer.set_caret(Pos::new(60, 0), false);
        area.follow_caret(20, 10);
        assert_eq!(area.top.get(), (51, 0));
        area.buffer.set_caret(Pos::new(5, 0), false);
        area.follow_caret(20, 10);
        assert_eq!(area.top.get(), (5, 0));
    }

    #[test]
    fn unwrapped_lines_scroll_sideways_to_the_caret() {
        let mut area = area(&"x".repeat(50));
        area.options.soft_wrap = false;
        area.buffer.end(false);
        area.follow_caret(20, 3);
        assert_eq!(area.left.get(), 31);
    }
}

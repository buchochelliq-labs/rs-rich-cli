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
use rich_interact::edit::{cells, graphemes, Buffer, Pos};
use rich_interact::{Button, Key, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
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
    let initial = text.get_untracked();
    let area = TextArea {
        buffer: Buffer::new(&initial),
        seen: initial,
        text,
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
    };
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
}

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
}

impl TextArea {
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
        if !self.options.soft_wrap {
            return vec![Row {
                line: index,
                start: 0,
                end: line.len(),
                first: true,
            }];
        }
        let mut rows = Vec::new();
        let (mut start, mut used, mut first) = (0, 0, true);
        for (at, _, width) in graphemes(line) {
            if used + width > room && at > start {
                rows.push(Row {
                    line: index,
                    start,
                    end: at,
                    first,
                });
                (start, used, first) = (at, 0, false);
            }
            used += width;
        }
        rows.push(Row {
            line: index,
            start,
            end: line.len(),
            first,
        });
        rows
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
            let column = cells(&self.buffer.line(caret.line)[..caret.at]);
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
            .map(|line| cells(line))
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
        selection: &Style,
    ) -> Vec<Segment> {
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
        let mut out: Vec<Segment> = Vec::new();
        let mut column = 0;
        let mut shown = 0;
        for (start, end, cell_width) in graphemes(&line[row.start..row.end]) {
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
            if text == "\t" || column < left {
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
            if selected
                .as_ref()
                .is_some_and(|range| range.contains(&start))
            {
                style = Some(selected_style.clone());
            }
            let width = cells(text).max(if text.trim().is_empty() {
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
        let target = (column.saturating_sub(self.gutter()) as usize) + left;
        let mut at = shown.start;
        let mut used = 0;
        for (start, end, width) in graphemes(&line[shown.start..shown.end]) {
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
        let (ctrl, alt, shift) = (key.modifiers.ctrl, key.modifiers.alt, key.modifiers.shift);
        // Escape, then Tab or Shift+Tab, moves the focus on, so the area is
        // never a trap; read-only, Tab always does.
        let escaped = std::mem::replace(&mut self.escaped, key.code == KeyCode::Escape);
        let edit = !self.options.read_only;
        let b = &mut self.buffer;
        match key.code {
            KeyCode::Char(c) if !ctrl && !alt => {
                if !edit {
                    return false;
                }
                let mut bytes = [0; 4];
                b.insert(c.encode_utf8(&mut bytes));
            }
            KeyCode::Char('a') if ctrl => b.select_all(),
            KeyCode::Char('c') if ctrl => match b.selected_text() {
                Some(text) => cx.app().copy(text),
                // Nothing selected: Ctrl+C is the app's (it quits).
                None => return false,
            },
            KeyCode::Char('x') if ctrl && edit => match b.selected_text() {
                Some(text) => {
                    cx.app().copy(text);
                    b.backspace();
                }
                None => return false,
            },
            KeyCode::Char('z') if ctrl && edit => {
                if shift {
                    b.redo();
                } else {
                    b.undo();
                }
            }
            KeyCode::Char('y') if ctrl && edit => {
                b.redo();
            }
            KeyCode::Char('w') if ctrl && edit => b.delete_word(),
            KeyCode::Char('u') if ctrl && edit => b.delete_to_start(),
            KeyCode::Char('k') if ctrl && edit => b.delete_to_end(),
            KeyCode::Enter if edit => b.insert("\n"),
            KeyCode::Tab if edit && !ctrl && !escaped => b.indent(),
            KeyCode::BackTab if edit && !escaped => b.outdent(),
            KeyCode::Backspace if edit => b.backspace(),
            KeyCode::Delete if edit => b.delete(),
            KeyCode::Left if ctrl => b.word_left(shift),
            KeyCode::Right if ctrl => b.word_right(shift),
            KeyCode::Left => b.left(shift),
            KeyCode::Right => b.right(shift),
            KeyCode::Up => b.vertical(-1, shift),
            KeyCode::Down => b.vertical(1, shift),
            KeyCode::PageUp => {
                let page = self.page();
                self.buffer.vertical(-page, shift);
            }
            KeyCode::PageDown => {
                let page = self.page();
                self.buffer.vertical(page, shift);
            }
            KeyCode::Home if ctrl => b.top(shift),
            KeyCode::End if ctrl => b.bottom(shift),
            KeyCode::Home => b.home(shift),
            KeyCode::End => b.end(shift),
            // With nothing selected it goes on to the app (a modal closes).
            KeyCode::Escape if b.selection().is_some() => {
                let caret = b.caret();
                b.set_caret(caret, false);
            }
            _ => return false,
        }
        true
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
            self.buffer.set_text(&self.seen);
            self.top.set((0, 0));
            self.left.set(0);
        }
        let (width, height) = (canvas.width(), canvas.height());
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
        let selection = cx.style("text_area.selection", "reverse");
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
                line.extend(self.row_segments(*row, first_line, left, text_width, &selection));
            }
            if let Some(fill) = &fill {
                let used: usize = line.iter().map(|s| cells(&s.text)).sum();
                let room = width as usize - used.min(width as usize);
                line.push(Segment::new(" ".repeat(room), Some(fill.clone())));
            }
            if row.line == caret.line {
                let last_of_line = row.end == self.buffer.line(row.line).len();
                let here = caret.at >= row.start && (caret.at < row.end || last_of_line);
                if here {
                    let column = cells(&self.buffer.line(row.line)[row.start..caret.at]);
                    let column = column.saturating_sub(left);
                    if column <= text_width {
                        caret_cell =
                            Some((gutter + column.min(u16::MAX as usize) as u16, y as u16));
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
        canvas.lines(&lines);
        self.caret.set(caret_cell);
        *self.rows.borrow_mut() = rows;
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let revision = self.buffer.revision();
        match event {
            WidgetEvent::Key(key) => {
                if !self.key(cx, *key) {
                    return Used::No;
                }
            }
            WidgetEvent::Paste(text) if !self.options.read_only => self.buffer.insert(text),
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

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::TextBox
    }

    /// The caret's row: in accessible mode the terminal's cursor goes there,
    /// and in linear mode it is the line read out.
    fn cursor(&self) -> Option<Rect> {
        let (_, y) = self.caret.get()?;
        Some(Rect::new(0, y, self.size.get().0, 1))
    }
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

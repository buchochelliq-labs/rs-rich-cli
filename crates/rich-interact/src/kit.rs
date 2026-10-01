//! The building-block kit: the pieces the built-in components are made of,
//! public so your own components can use them too (0.0.14 workstream 1).
//!
//! Two kinds of piece live here.
//!
//! **Line helpers** build and measure the lines of a [`View`](crate::View):
//! [`text`] and [`plain`] make segments, [`width`] measures a line,
//! [`fit`] crops one and [`pad`] crops or pads it to an exact width,
//! [`highlight`] styles the characters a filter matched, and [`question`]
//! is the `? prompt › ` line every built-in starts with. [`slice`](fn@slice) cuts
//! cells out of a line, [`overlay`] draws one line over another and
//! [`restyle`] puts a style over a line (a backdrop); [`frame`] boxes
//! lines and [`place`] draws a box over a view, as a modal is drawn.
//! [`pasted`] and
//! [`shown`] make untrusted text safe for a one-line field or a table cell.
//!
//! **State types** hold what a component remembers between events, with the
//! editing and movement rules the built-ins share, and no rendering of
//! their own:
//!
//! - [`ListState`]: a cursor over rows, the first row shown, and a
//!   selection (what [`Select`](crate::Select) moves and marks with);
//! - [`ScrollState`]: an offset into lines (what a
//!   [`Viewport`](crate::Viewport) scrolls with);
//! - [`FilterState`]: a query, and the candidates that match it, ranked by
//!   the [fuzzy](crate::fuzzy) matcher, with the characters to highlight;
//! - [`TextBuffer`]: one line of text and a caret that moves by grapheme
//!   cluster, with the shell editing keys' operations and an undo point
//!   (what [`Input`](crate::Input) edits);
//! - [`Divider`]: a border between two panes that the mouse drags or the
//!   keyboard nudges (what [`Split`](crate::compose::Split) and
//!   `Select`'s preview resize with);
//! - [`ActionMenu`]: the Ctrl+K menu of [`Action`]s for one target.
//!
//! ```
//! use rich_interact::kit::{FilterState, ListState};
//!
//! let mut filter = FilterState::new(["main.rs", "lib.rs", "README.md"]);
//! filter.set_query("rs");
//! assert_eq!(filter.len(), 2);
//! let mut list = ListState::new();
//! list.set_len(filter.len());
//! list.step(1, 10);
//! let second = filter.index(list.cursor()).unwrap();
//! assert!(filter.candidates()[second].ends_with(".rs"));
//! ```

use std::ops::Range;

use rich::cells::{cell_len, split_graphemes};
use rich::{Segment, Style};

pub use crate::components::Theme;
use crate::event::{Event, Key, KeyCode, Mouse, MouseKind};
use crate::fuzzy::rank;
use crate::item::Action;

// ---------------------------------------------------------------------------
// Line helpers.

/// A segment of `text` in `style`.
pub fn text(text: impl Into<String>, style: &Style) -> Segment {
    Segment::new(text, Some(style.clone()))
}

/// The first line of `text` as segments, unwrapped, its styles and style
/// metadata kept: an icon for a list row, a crumb or a status item. A micro
/// asset's placeholder (`rich_micro::placeholder`) stays tagged, so the
/// painter's graphics can draw the asset over its cells.
pub fn icon(text: &rich::Text) -> Vec<Segment> {
    let console = rich::Console::builder().width(4096).build();
    let options = console.options().update_width(4096);
    let mut text = text.clone();
    text.set_no_wrap(Some(true));
    console
        .render_lines(&text, &options, false)
        .into_iter()
        .next()
        .unwrap_or_default()
        .into_iter()
        .filter(|segment| !segment.text.is_empty())
        .collect()
}

/// A segment of unstyled `text`.
pub fn plain(text: impl Into<String>) -> Segment {
    Segment::new(text, None)
}

/// The width of a line in cells.
pub fn width(line: &[Segment]) -> usize {
    line.iter().map(Segment::cell_length).sum()
}

/// Crop a line to `columns` cells; a shorter line is left as it is.
pub fn fit(line: Vec<Segment>, columns: usize) -> Vec<Segment> {
    if width(&line) > columns {
        Segment::adjust_line_length(&line, columns, None)
    } else {
        line
    }
}

/// Crop or pad a line to exactly `columns` cells.
pub fn pad(line: Vec<Segment>, columns: usize) -> Vec<Segment> {
    Segment::adjust_line_length(&line, columns, None)
}

/// `label` with the characters at `positions` (character indices, in
/// order) in `matched`, the rest in `base`: how a filter's matches show.
pub fn highlight(
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

/// The cells `from..to` of a line. A wide character cut by either end
/// becomes spaces in its style, so the result is exactly the cells asked
/// for (fewer when the line is shorter). Control segments are dropped.
pub fn slice(line: &[Segment], from: usize, to: usize) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    let mut at = 0;
    for segment in line.iter().filter(|segment| !segment.control) {
        if at >= to {
            break;
        }
        let length = segment.cell_length();
        if at >= from && at + length <= to {
            out.push(segment.clone());
            at += length;
            continue;
        }
        let (spans, _) = split_graphemes(&segment.text);
        let mut piece = String::new();
        for (start, end, cells) in spans {
            let (left, right) = (at, at + cells);
            if left >= from && right <= to {
                piece.push_str(&segment.text[start..end]);
            } else if right > from && left < to {
                let covered = right.min(to) - left.max(from);
                piece.push_str(&" ".repeat(covered));
            }
            at = right;
        }
        if !piece.is_empty() {
            out.push(Segment::new(piece, segment.style.clone()));
        }
    }
    out
}

/// `top` drawn over `base` from cell `x`: `base`'s cells before `x` (padded
/// out to `x`), then `top`, then what of `base` is right of it.
pub fn overlay(base: &[Segment], x: usize, top: &[Segment]) -> Vec<Segment> {
    let end = x + width(top);
    let mut line = pad(slice(base, 0, x), x);
    line.extend(top.iter().cloned());
    let base_width = width(base);
    if base_width > end {
        line.extend(slice(base, end, base_width));
    }
    line
}

/// `line` with `style` over each segment's own: a backdrop's dimming.
pub fn restyle(line: &[Segment], style: &Style) -> Vec<Segment> {
    line.iter()
        .map(|segment| {
            if segment.control {
                return segment.clone();
            }
            let styled = match &segment.style {
                Some(own) => own.combine(style),
                None => style.clone(),
            };
            Segment::new(segment.text.clone(), Some(styled))
        })
        .collect()
}

/// `lines` in a rounded box `inner` cells wide inside, each padded to that
/// width, with `title` in the top border: how a modal or a popover is
/// drawn, by [`Layers`](crate::compose::Layers) and the action menu alike.
pub fn frame(
    lines: Vec<Vec<Segment>>,
    inner: usize,
    title: Option<&str>,
    style: &Style,
) -> Vec<Vec<Segment>> {
    let mut top = vec![text("╭", style)];
    match title {
        Some(title) if inner >= 4 => {
            let title = fit(vec![text(format!(" {title} "), style)], inner - 1);
            let used = width(&title) + 1;
            top.push(text("─", style));
            top.extend(title);
            top.push(text("─".repeat(inner.saturating_sub(used)), style));
        }
        _ => top.push(text("─".repeat(inner), style)),
    }
    top.push(text("╮", style));
    let mut boxed = vec![top];
    for line in lines {
        let mut row = vec![text("│", style)];
        row.extend(pad(line, inner));
        row.push(text("│", style));
        boxed.push(row);
    }
    boxed.push(vec![
        text("╰", style),
        text("─".repeat(inner), style),
        text("╯", style),
    ]);
    boxed
}

/// Draw `top`'s lines over `base` from cell `x` of row `y`, adding rows
/// to `base` where `top` reaches below it; with a `backdrop`, first put
/// that style over every line of `base` (a modal's dimming).
pub fn place(
    base: &mut Vec<Vec<Segment>>,
    x: usize,
    y: usize,
    top: &[Vec<Segment>],
    backdrop: Option<&Style>,
) {
    if let Some(style) = backdrop {
        for line in base.iter_mut() {
            *line = restyle(line, style);
        }
    }
    if base.len() < y + top.len() {
        base.resize_with(y + top.len(), Vec::new);
    }
    for (row, line) in top.iter().enumerate() {
        let under = &mut base[y + row];
        *under = overlay(under, x, line);
    }
}

/// Pasted text for a one-line field: line breaks become `newline`, a tab a
/// space, and other terminal controls (C0, DEL, C1) are dropped, so a paste
/// cannot carry an escape sequence into the answer or onto the screen.
pub fn pasted(text: &str, newline: &str) -> String {
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
pub fn shown(text: &str) -> String {
    text.chars()
        .map(|c| crate::paint::visible(c).unwrap_or(c))
        .collect()
}

/// The question line every built-in component starts with: `? prompt › `.
pub fn question(theme: &Theme, prompt: &str) -> Vec<Segment> {
    vec![
        text(theme.question.clone(), &theme.question_style),
        plain(" "),
        text(prompt, &theme.prompt),
        plain(" "),
        text("›", &theme.hint),
        plain(" "),
    ]
}

// ---------------------------------------------------------------------------
// ListState.

/// A cursor over a list of rows, the first row shown, and a selection.
///
/// Rows are positions in what is listed (after filtering, say); the
/// selection is kept by *index*, whatever the caller's indices are (an
/// item's index before filtering), so marks survive a change of filter.
/// Movement takes the page, the rows on screen, since that can change with
/// every render.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ListState {
    len: usize,
    cursor: usize,
    offset: usize,
    selected: Vec<bool>,
}

impl ListState {
    pub fn new() -> ListState {
        ListState::default()
    }

    /// How many rows there are.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Set the number of rows. The cursor is left where it is: call
    /// [`reset`](Self::reset) or [`set_cursor`](Self::set_cursor) to put it
    /// on a row that exists.
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
    }

    /// The row the cursor is on.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Put the cursor on `row`, without scrolling (see
    /// [`follow`](Self::follow)).
    pub fn set_cursor(&mut self, row: usize) {
        self.cursor = row;
    }

    /// The first row shown.
    pub fn offset(&self) -> usize {
        self.offset
    }

    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    /// Put the cursor on `row` and the first row shown back at the top,
    /// then scroll just enough to show the cursor: after the rows changed.
    pub fn reset(&mut self, row: usize, page: usize) {
        self.cursor = row;
        self.offset = 0;
        self.follow(page);
    }

    /// Scroll just enough that the cursor's row is among the `page` shown.
    pub fn follow(&mut self, page: usize) {
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + page {
            self.offset = self.cursor + 1 - page;
        }
    }

    /// Move the cursor by `delta` rows (negative: up), stopping at the
    /// ends, and scroll to follow it. Nothing happens with no rows.
    pub fn step(&mut self, delta: isize, page: usize) {
        if self.len == 0 {
            return;
        }
        let last = self.len - 1;
        self.cursor = self.cursor.saturating_add_signed(delta).min(last);
        self.follow(page);
    }

    /// Move to `row` (clamped to the last) and scroll to show it.
    pub fn select_row(&mut self, row: usize, page: usize) {
        if self.len == 0 {
            return;
        }
        self.cursor = row.min(self.len - 1);
        self.follow(page);
    }

    /// The rows shown on a page of `page` rows.
    pub fn visible(&self, page: usize) -> Range<usize> {
        let end = (self.offset + page).min(self.len);
        self.offset.min(end)..end
    }

    /// Make room to select `count` indices, all unselected.
    pub fn clear_selection(&mut self, count: usize) {
        self.selected = vec![false; count];
    }

    pub fn is_selected(&self, index: usize) -> bool {
        self.selected.get(index).copied().unwrap_or(false)
    }

    /// Select or unselect `index`; indices past the room made are grown to.
    pub fn set_selected(&mut self, index: usize, on: bool) {
        if index >= self.selected.len() {
            self.selected.resize(index + 1, false);
        }
        self.selected[index] = on;
    }

    /// Flip whether `index` is selected.
    pub fn toggle(&mut self, index: usize) {
        let on = !self.is_selected(index);
        self.set_selected(index, on);
    }

    /// The selected indices, in order.
    pub fn selected(&self) -> Vec<usize> {
        (0..self.selected.len())
            .filter(|&index| self.selected[index])
            .collect()
    }

    /// How many indices are selected.
    pub fn selected_count(&self) -> usize {
        self.selected.iter().filter(|on| **on).count()
    }
}

// ---------------------------------------------------------------------------
// ScrollState.

/// An offset into `len` lines, a page at a time: the scroll position of a
/// pager or a preview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollState {
    len: usize,
    offset: usize,
}

impl ScrollState {
    pub fn new(len: usize) -> ScrollState {
        ScrollState { len, offset: 0 }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Set the number of lines, keeping the offset where it still fits.
    pub fn set_len(&mut self, len: usize) {
        self.len = len;
        self.offset = self.offset.min(len.saturating_sub(1));
    }

    /// The first line shown.
    pub fn offset(&self) -> usize {
        self.offset
    }

    fn last_offset(&self, page: usize) -> usize {
        self.len.saturating_sub(page)
    }

    /// Scroll by `delta` lines (negative: up), within the lines. Returns
    /// whether the offset changed.
    pub fn scroll(&mut self, delta: isize, page: usize) -> bool {
        let target = self
            .offset
            .saturating_add_signed(delta)
            .min(self.last_offset(page));
        let changed = target != self.offset;
        self.offset = target;
        changed
    }

    /// Scroll to put `line` first, or as near as the last page allows.
    pub fn scroll_to(&mut self, line: usize, page: usize) -> bool {
        let target = line.min(self.last_offset(page));
        let changed = target != self.offset;
        self.offset = target;
        changed
    }

    /// Scroll just enough that `line` is shown.
    pub fn show(&mut self, line: usize, page: usize) -> bool {
        if line < self.offset {
            self.scroll_to(line, page)
        } else if line >= self.offset + page {
            self.scroll_to(line + 1 - page, page)
        } else {
            false
        }
    }

    /// The lines shown on a page of `page` lines.
    pub fn visible(&self, page: usize) -> Range<usize> {
        let end = (self.offset + page).min(self.len);
        self.offset.min(end)..end
    }

    /// The keys [`handle`](Self::handle) scrolls with, in `context`, for a
    /// component's [`keymap`](crate::Component::keymap).
    pub fn keymap(context: &str) -> crate::keymap::Keymap {
        use crate::keymap::keys;
        crate::keymap::Keymap::new(context)
            .bind("scroll-up", keys("up k"), "scroll up")
            .bind("scroll-down", keys("down j"), "scroll down")
            .bind("page-up", keys("pageup b"), "page up")
            .bind("page-down", keys("pagedown space"), "page down")
            .bind("top", keys("home g"), "to the top")
            .bind("bottom", keys("end G"), "to the bottom")
    }

    /// Do one of [`keymap`](Self::keymap)'s actions (`scroll-up`,
    /// `page-down`, `top`, ...): how a component that looks its keys up in
    /// a keymap, and so honours rebinding, scrolls. Returns whether
    /// `action` is a scroll action (moved or not).
    pub fn act(&mut self, action: &str, page: usize) -> bool {
        let page_step = page.max(1) as isize;
        match action {
            "scroll-up" => self.scroll(-1, page),
            "scroll-down" => self.scroll(1, page),
            "page-up" => self.scroll(-page_step, page),
            "page-down" => self.scroll(page_step, page),
            "top" => self.scroll_to(0, page),
            "bottom" => self.scroll_to(usize::MAX, page),
            _ => return false,
        };
        true
    }

    /// Move with the usual keys and the mouse wheel: arrows and `j`/`k` by a
    /// line, PageUp/PageDown, `b` and Space by a page, Home/End and `g`/`G`
    /// to the ends, and the wheel by three lines. Returns whether the event
    /// was a scroll key (moved or not). The keys are fixed; see
    /// [`act`](Self::act) for keys from a keymap.
    pub fn handle(&mut self, event: &Event, page: usize) -> bool {
        let page_step = page.max(1) as isize;
        match event {
            Event::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.scroll(-1, page),
                    KeyCode::Down | KeyCode::Char('j') => self.scroll(1, page),
                    KeyCode::PageUp | KeyCode::Char('b') => self.scroll(-page_step, page),
                    KeyCode::PageDown | KeyCode::Char(' ') => self.scroll(page_step, page),
                    KeyCode::Home | KeyCode::Char('g') => self.scroll_to(0, page),
                    KeyCode::End | KeyCode::Char('G') => self.scroll_to(usize::MAX, page),
                    _ => return false,
                };
                true
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::ScrollUp => {
                    self.scroll(-3, page);
                    true
                }
                MouseKind::ScrollDown => {
                    self.scroll(3, page);
                    true
                }
                _ => false,
            },
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// FilterState.

/// A query and the candidates that match it, best first.
///
/// Candidates are strings to search; a match is a candidate's index and the
/// character positions that matched, for [`highlight`]. With an empty query
/// every candidate matches, in order, except those hidden
/// ([`set_hidden`](Self::set_hidden)): a collapsed tree node's children
/// stay out of the list until something is typed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FilterState {
    query: String,
    candidates: Vec<String>,
    hidden: Vec<bool>,
    limits: Vec<usize>,
    matches: Vec<(usize, Vec<usize>)>,
    /// Each candidate's parent, for a tree: matches then keep their
    /// ancestors, in tree order ([`set_tree`](Self::set_tree)).
    parents: Option<Vec<Option<usize>>>,
    /// Which matches are only there as a match's ancestor.
    context: Vec<bool>,
    /// The position of the best match.
    best: Option<usize>,
}

impl FilterState {
    /// Filter `candidates`, with nothing typed yet.
    pub fn new<I, S>(candidates: I) -> FilterState
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut filter = FilterState::default();
        filter.set_candidates(candidates);
        filter
    }

    /// Replace the candidates and filter them again.
    pub fn set_candidates<I, S>(&mut self, candidates: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.candidates = candidates.into_iter().map(Into::into).collect();
        self.refilter();
    }

    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }

    /// Leave these candidates out while the query is empty (`true` hides).
    /// Takes effect on the next [`refilter`](Self::refilter).
    pub fn set_hidden(&mut self, hidden: Vec<bool>) {
        self.hidden = hidden;
    }

    /// Filter as a tree (#428): `parents[i]` is candidate `i`'s parent,
    /// and candidates come in tree order, each parent before its children.
    /// While a query is typed, what matches keeps its ancestors, so a match
    /// shows where it is: the list is the matches and their ancestors in
    /// tree order, the ancestors marked as [context](Self::is_context) with
    /// nothing highlighted, and [`best`](Self::best) is where the best match
    /// is. `None` goes back to a flat ranking. Takes effect on the next
    /// [`refilter`](Self::refilter).
    pub fn set_tree(&mut self, parents: Option<Vec<Option<usize>>>) {
        self.parents = parents;
    }

    /// Highlight at most the first `limits[i]` characters of candidate `i`:
    /// for a candidate searched with more than its label (a description),
    /// so only the label highlights. Takes effect on the next
    /// [`refilter`](Self::refilter).
    pub fn set_highlight_limits(&mut self, limits: Vec<usize>) {
        self.limits = limits;
    }

    /// What is typed.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The query, to edit in place; call [`refilter`](Self::refilter)
    /// after.
    pub fn query_mut(&mut self) -> &mut String {
        &mut self.query
    }

    pub fn is_filtering(&self) -> bool {
        !self.query.is_empty()
    }

    /// Replace the query and filter again.
    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query = query.into();
        self.refilter();
    }

    /// Type `c`.
    pub fn push(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    /// Type `text` (a paste: see [`pasted`]).
    pub fn push_str(&mut self, text: &str) {
        self.query.push_str(text);
        self.refilter();
    }

    /// Delete the last character; `false` when there was none.
    pub fn pop(&mut self) -> bool {
        let popped = self.query.pop().is_some();
        if popped {
            self.refilter();
        }
        popped
    }

    /// Clear the query.
    pub fn clear(&mut self) {
        self.query.clear();
        self.refilter();
    }

    /// Match the candidates against the query again.
    pub fn refilter(&mut self) {
        let filtering = !self.query.is_empty();
        let hidden = &self.hidden;
        let limits = &self.limits;
        let candidates = &self.candidates;
        let ranked: Vec<(usize, Vec<usize>)> =
            rank(&self.query, candidates.iter().map(String::as_str))
                .into_iter()
                .filter(|(index, _)| filtering || !hidden.get(*index).copied().unwrap_or(false))
                .map(|(index, found)| {
                    let mut positions = found.positions;
                    if let Some(limit) = limits.get(index) {
                        positions.retain(|position| position < limit);
                    }
                    (index, positions)
                })
                .collect();
        match &self.parents {
            Some(parents) if filtering => {
                let best = ranked.first().map(|(index, _)| *index);
                let (matches, context) = keep_ancestors(ranked, parents);
                self.best = best.and_then(|best| matches.iter().position(|(i, _)| *i == best));
                self.matches = matches;
                self.context = context;
            }
            _ => {
                self.best = (!ranked.is_empty()).then_some(0);
                self.context = vec![false; ranked.len()];
                self.matches = ranked;
            }
        }
    }

    /// Whether the match at `position` is only there as an ancestor of a
    /// match ([`set_tree`](Self::set_tree)): drawn dim, not picked first.
    pub fn is_context(&self, position: usize) -> bool {
        self.context.get(position).copied().unwrap_or(false)
    }

    /// The position of the best match: the first, unless a tree keeps
    /// ancestors above it.
    pub fn best(&self) -> Option<usize> {
        self.best
    }

    /// The matches, best first: candidate indices and the positions to
    /// highlight.
    pub fn matches(&self) -> &[(usize, Vec<usize>)] {
        &self.matches
    }

    /// How many candidates match.
    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.matches.is_empty()
    }

    /// The candidate at `position` in the matches.
    pub fn index(&self, position: usize) -> Option<usize> {
        self.matches.get(position).map(|(index, _)| *index)
    }

    /// Where candidate `index` is in the matches, if it matches.
    pub fn position_of(&self, index: usize) -> Option<usize> {
        self.matches.iter().position(|(i, _)| *i == index)
    }
}

/// `index`'s ancestors in a tree given by `parents`, nearest first.
///
/// ```
/// use rich_interact::kit::ancestors;
///
/// // 0 ─ 1 ─ 2, and 3 under 0.
/// let parents = [None, Some(0), Some(1), Some(0)];
/// assert_eq!(ancestors(&parents, 2).collect::<Vec<_>>(), [1, 0]);
/// ```
pub fn ancestors(parents: &[Option<usize>], index: usize) -> impl Iterator<Item = usize> + '_ {
    let mut at = parents.get(index).copied().flatten();
    // A malformed parent list (a cycle) stops after every node once.
    let mut left = parents.len();
    std::iter::from_fn(move || {
        let current = at.filter(|_| left > 0)?;
        left -= 1;
        at = parents.get(current).copied().flatten();
        Some(current)
    })
}

/// Tree filtering that keeps ancestors (#428): `matches` (candidate
/// indices with the positions to highlight, in any order) plus every
/// ancestor of each, in tree order (index order, parents before
/// children). Returns the list and, for each entry, whether it is only
/// there as an ancestor (context), with nothing highlighted. A parent
/// index past the list is not a node: the walk up stops there.
///
/// ```
/// use rich_interact::kit::keep_ancestors;
///
/// //  0 config
/// //  1 ├── server
/// //  2 │   └── port
/// //  3 └── debug
/// let parents = [None, Some(0), Some(1), Some(0)];
/// let (list, context) = keep_ancestors(vec![(2, vec![0, 1])], &parents);
/// assert_eq!(list, [(0, vec![]), (1, vec![]), (2, vec![0, 1])]);
/// assert_eq!(context, [true, true, false]);
/// ```
pub fn keep_ancestors(
    matches: Vec<(usize, Vec<usize>)>,
    parents: &[Option<usize>],
) -> (Vec<(usize, Vec<usize>)>, Vec<bool>) {
    let count = parents
        .len()
        .max(matches.iter().map(|(i, _)| i + 1).max().unwrap_or(0));
    let mut found: Vec<Option<Vec<usize>>> = vec![None; count];
    let mut kept = vec![false; count];
    for (index, positions) in matches {
        // A parent past the list (a malformed parent list) ends the walk.
        for ancestor in ancestors(parents, index) {
            if kept.get(ancestor).is_none_or(|&kept| kept) {
                break;
            }
            kept[ancestor] = true;
        }
        kept[index] = true;
        found[index] = Some(positions);
    }
    let mut list = Vec::new();
    let mut context = Vec::new();
    for (index, positions) in found.into_iter().enumerate() {
        if !kept[index] {
            continue;
        }
        context.push(positions.is_none());
        list.push((index, positions.unwrap_or_default()));
    }
    (list, context)
}

// ---------------------------------------------------------------------------
// TextBuffer.

/// One line of text and a caret, for a text field.
///
/// The caret is a character index and moves by grapheme cluster, so an
/// accent is never split from its letter. The operations are the ones a
/// shell line has (and [`Input`](crate::Input) binds): insert, Backspace,
/// Delete, Ctrl+U (delete to the start), Ctrl+W (delete a word), and moving
/// by grapheme or to the ends. [`checkpoint`](Self::checkpoint) keeps an
/// undo point and [`undo`](Self::undo) goes back to the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBuffer {
    value: String,
    caret: usize,
    undo: Vec<(String, usize)>,
}

impl TextBuffer {
    pub fn new() -> TextBuffer {
        TextBuffer::default()
    }

    /// A buffer holding `text`, the caret at its end.
    pub fn with_text(text: impl Into<String>) -> TextBuffer {
        let mut buffer = TextBuffer::new();
        buffer.set_text(text);
        buffer
    }

    pub fn text(&self) -> &str {
        &self.value
    }

    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// The length in characters.
    pub fn len(&self) -> usize {
        self.value.chars().count()
    }

    /// Replace the text, the caret at its end.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.value = text.into();
        self.caret = self.len();
    }

    /// The caret, in characters.
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Put the caret at character `caret` (clamped to the end), or at the
    /// start of the grapheme `caret` falls inside.
    pub fn set_caret(&mut self, caret: usize) {
        self.caret = caret.min(self.len());
        self.snap(false);
    }

    /// Move the caret onto a grapheme boundary: on to the end of the
    /// grapheme it is inside (`forward`) or back to its start. Typing a
    /// base character in front of a combining mark or U+FE0F, or deleting
    /// what kept two clusters apart, can leave it inside one.
    fn snap(&mut self, forward: bool) {
        let stops = self.stops();
        self.caret = if forward {
            stops.iter().copied().find(|&stop| stop >= self.caret)
        } else {
            stops.iter().rev().copied().find(|&stop| stop <= self.caret)
        }
        .unwrap_or(self.caret);
    }

    /// The byte offset of character `caret`.
    pub fn byte(&self, caret: usize) -> usize {
        self.value
            .char_indices()
            .nth(caret)
            .map_or(self.value.len(), |(index, _)| index)
    }

    /// The text before the caret.
    pub fn before_caret(&self) -> &str {
        &self.value[..self.byte(self.caret)]
    }

    /// The character indices where graphemes start, and the end: where
    /// the caret may stop.
    pub fn stops(&self) -> Vec<usize> {
        let (spans, _) = split_graphemes(&self.value);
        let mut stops = Vec::with_capacity(spans.len() + 1);
        let (mut chars, mut byte) = (0, 0);
        for (start, _, _) in spans {
            chars += self.value[byte..start].chars().count();
            byte = start;
            stops.push(chars);
        }
        stops.push(self.value.chars().count());
        stops
    }

    /// The caret one grapheme back.
    pub fn previous_stop(&self) -> usize {
        self.stops()
            .iter()
            .rev()
            .copied()
            .find(|&stop| stop < self.caret)
            .unwrap_or(0)
    }

    /// The caret one grapheme on.
    pub fn next_stop(&self) -> usize {
        self.stops()
            .iter()
            .copied()
            .find(|&stop| stop > self.caret)
            .unwrap_or(self.caret)
    }

    /// Insert `text` at the caret, and move past it.
    pub fn insert(&mut self, text: &str) {
        let at = self.byte(self.caret);
        self.value.insert_str(at, text);
        self.caret += text.chars().count();
        self.snap(true);
    }

    /// Move the caret one grapheme left.
    pub fn left(&mut self) {
        self.caret = self.previous_stop();
    }

    /// Move the caret one grapheme right.
    pub fn right(&mut self) {
        self.caret = self.next_stop().min(self.len());
    }

    pub fn home(&mut self) {
        self.caret = 0;
    }

    pub fn end(&mut self) {
        self.caret = self.len();
    }

    /// Delete the grapheme before the caret; `false` at the start.
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let previous = self.previous_stop();
        let (from, to) = (self.byte(previous), self.byte(self.caret));
        self.value.replace_range(from..to, "");
        self.caret = previous;
        self.snap(false);
        true
    }

    /// Delete the grapheme after the caret; `false` at the end.
    pub fn delete(&mut self) -> bool {
        if self.caret >= self.len() {
            return false;
        }
        let (from, to) = (self.byte(self.caret), self.byte(self.next_stop()));
        self.value.replace_range(from..to, "");
        self.snap(false);
        true
    }

    /// Delete everything before the caret (Ctrl+U).
    pub fn delete_to_start(&mut self) {
        let at = self.byte(self.caret);
        self.value.replace_range(..at, "");
        self.caret = 0;
    }

    /// Delete the word before the caret, and the spaces after it (Ctrl+W).
    pub fn delete_word(&mut self) {
        let chars: Vec<char> = self.value.chars().collect();
        let mut start = self.caret;
        while start > 0 && chars[start - 1] == ' ' {
            start -= 1;
        }
        while start > 0 && chars[start - 1] != ' ' {
            start -= 1;
        }
        let (from, to) = (self.byte(start), self.byte(self.caret));
        self.value.replace_range(from..to, "");
        self.caret = start;
        self.snap(false);
    }

    /// Keep the text and caret as an undo point.
    pub fn checkpoint(&mut self) {
        let point = (self.value.clone(), self.caret);
        if self.undo.last() != Some(&point) {
            self.undo.push(point);
        }
    }

    /// Go back to the last undo point; `false` when there is none.
    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some((value, caret)) => {
                self.value = value;
                self.caret = caret;
                true
            }
            None => false,
        }
    }

    /// `text` as shown: every character as `mask` when there is one.
    pub fn masked(text: &str, mask: Option<char>) -> String {
        match mask {
            Some(mask) => std::iter::repeat_n(mask, text.chars().count()).collect(),
            None => text.to_string(),
        }
    }

    /// The text as shown (masked or not), cut to `available` cells so the
    /// caret stays in view: a line longer than the space scrolls. Returns
    /// what to show and the caret's column in it.
    pub fn window(&self, available: usize, mask: Option<char>) -> (String, usize) {
        let shown = Self::masked(&self.value, mask);
        // Masking keeps one character for each, so the caret is the same
        // character index in what is shown.
        let caret = shown
            .char_indices()
            .nth(self.caret)
            .map_or(shown.len(), |(index, _)| index);
        // The cells before the caret and the cells dropped are both counted
        // grapheme by grapheme: `cell_len` of the whole prefix can differ
        // from that (a leading U+200D joins what follows it).
        let (spans, _) = split_graphemes(&shown);
        let before: usize = spans
            .iter()
            .filter(|(start, _, _)| *start < caret)
            .map(|(_, _, cells)| cells)
            .sum();
        // One cell for the caret after the last character.
        let skip = (before + 1).saturating_sub(available.max(1));
        if skip == 0 {
            return (shown, before);
        }
        let mut dropped = 0;
        let mut from = shown.len();
        for (start, _, cells) in spans {
            if dropped >= skip || start >= caret {
                from = start;
                break;
            }
            dropped += cells;
        }
        (shown[from..].to_string(), before.saturating_sub(dropped))
    }
}

// ---------------------------------------------------------------------------
// Divider.

/// A border between two panes along one axis, where the mouse can grab and
/// drag it and the keyboard can nudge it.
///
/// Positions are cells along the axis: the first pane's size, then
/// `thickness` cells of border, then the second pane. Both panes keep at
/// least their minimum while the total has room for that.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Divider {
    position: Option<usize>,
    dragging: bool,
    /// The fewest cells the first pane takes.
    pub min_before: usize,
    /// The fewest cells the second pane takes.
    pub min_after: usize,
    /// The cells the border takes.
    pub thickness: usize,
}

impl Default for Divider {
    fn default() -> Self {
        Divider::new(1)
    }
}

impl Divider {
    /// A border `thickness` cells thick, with no minimum pane size.
    pub fn new(thickness: usize) -> Divider {
        Divider {
            position: None,
            dragging: false,
            min_before: 0,
            min_after: 0,
            thickness,
        }
    }

    /// Both panes at least `cells`.
    pub fn min(mut self, cells: usize) -> Divider {
        self.min_before = cells;
        self.min_after = cells;
        self
    }

    /// Put the border at `position` (the first pane's size), until dragged.
    pub fn at(mut self, position: usize) -> Divider {
        self.position = Some(position);
        self
    }

    /// Where the border was put or dragged to, before clamping.
    pub fn position(&self) -> Option<usize> {
        self.position
    }

    pub fn set_position(&mut self, position: Option<usize>) {
        self.position = position;
    }

    /// Whether a drag is under way.
    pub fn dragging(&self) -> bool {
        self.dragging
    }

    /// The first pane's size for a border at `position` in `total` cells:
    /// both panes at least their minimum when there is room for that.
    pub fn clamp(&self, position: usize, total: usize) -> usize {
        let most = total
            .saturating_sub(self.min_after + self.thickness)
            .max(self.min_before.min(total));
        position.clamp(self.min_before.min(most), most)
    }

    /// The first pane's size in `total` cells: where the border was put or
    /// dragged, clamped, or `default` when it has not been.
    pub fn resolve(&self, total: usize, default: usize) -> usize {
        match self.position {
            Some(position) => self.clamp(position, total),
            None => default,
        }
    }

    /// Whether `at` (a cell along the axis) is on the border when the first
    /// pane is `first` cells.
    pub fn hit(&self, at: usize, first: usize) -> bool {
        (first..first + self.thickness.max(1)).contains(&at)
    }

    /// A press at `at`: grab the border if it is there. Returns whether it
    /// was grabbed.
    pub fn press(&mut self, at: usize, first: usize) -> bool {
        self.dragging = self.hit(at, first);
        self.dragging
    }

    /// A drag to `at` in `total` cells: move the border there if grabbed.
    /// Returns whether it moved.
    pub fn drag(&mut self, at: usize, total: usize) -> bool {
        if !self.dragging {
            return false;
        }
        self.position = Some(self.clamp(at, total));
        true
    }

    /// Let go of the border.
    pub fn release(&mut self) {
        self.dragging = false;
    }

    /// Press, drag and release from one mouse event along the axis, `at`
    /// being the event's cell along it. Returns whether the divider used
    /// the event.
    pub fn handle_mouse(&mut self, mouse: &Mouse, at: usize, first: usize, total: usize) -> bool {
        match mouse.kind {
            MouseKind::Down(crate::event::Button::Left) => self.press(at, first),
            MouseKind::Drag(crate::event::Button::Left) => self.drag(at, total),
            MouseKind::Up(_) if self.dragging => {
                self.release();
                true
            }
            _ => false,
        }
    }

    /// Move the border by `delta` cells from `first`, within `total`.
    pub fn nudge(&mut self, delta: isize, first: usize, total: usize) {
        self.position = Some(self.clamp(first.saturating_add_signed(delta), total));
    }
}

// ---------------------------------------------------------------------------
// ActionMenu.

/// The hints under an [`ActionMenu`]'s rows.
const MENU_HINTS: &str = "  ↑↓ move · enter run · esc close";

/// What an [`ActionMenu`] did with an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuReply {
    /// Nothing to do (moved, or not its event).
    Stay,
    /// Closed without running anything.
    Close,
    /// Run this action.
    Run(Action),
}

/// A menu of [`Action`]s for one target, as Ctrl+K opens in
/// [`Select`](crate::Select): the arrows move, Enter runs, an action's own
/// key runs it, Escape or the key that opened it closes it, and a click
/// runs the action under it or closes the menu.
#[derive(Clone, Debug)]
pub struct ActionMenu {
    pub actions: Vec<Action>,
    pub focus: usize,
    /// The key that opened it, which closes it again.
    pub key: Key,
}

impl ActionMenu {
    pub fn new(actions: Vec<Action>, key: Key) -> ActionMenu {
        ActionMenu {
            actions,
            focus: 0,
            key,
        }
    }

    /// Handle `event`, the menu's first row being `first` in the view the
    /// mouse's rows count from.
    pub fn handle(&mut self, event: &Event, first: usize) -> MenuReply {
        let count = self.actions.len();
        if let Some(mouse) = event.mouse() {
            if mouse.is_click() {
                let row = mouse.row as usize;
                if row >= first && row < first + count {
                    return MenuReply::Run(self.actions[row - first].clone());
                }
                return MenuReply::Close;
            }
            return MenuReply::Stay;
        }
        let Some(key) = event.key() else {
            return MenuReply::Stay;
        };
        match key.code {
            KeyCode::Escape => return MenuReply::Close,
            _ if key == self.key => return MenuReply::Close,
            KeyCode::Up => self.focus = (self.focus + count - 1) % count.max(1),
            KeyCode::Down | KeyCode::Tab => self.focus = (self.focus + 1) % count.max(1),
            KeyCode::Enter => return MenuReply::Run(self.actions[self.focus].clone()),
            _ => {
                if let Some(action) = self.actions.iter().find(|a| a.key == Some(key)) {
                    return MenuReply::Run(action.clone());
                }
            }
        }
        MenuReply::Stay
    }

    /// The width and rows [`render`](Self::render) needs: the widest row,
    /// and a row per action and one for the hints.
    pub fn size(&self) -> (usize, usize) {
        let label_width = self
            .actions
            .iter()
            .map(|action| cell_len(&action.label))
            .max()
            .unwrap_or(0);
        let key_width = self
            .actions
            .iter()
            .filter_map(|action| action.key.map(|key| cell_len(&key.to_string()) + 2))
            .max()
            .unwrap_or(0);
        let hints = cell_len(MENU_HINTS);
        (
            (4 + label_width + key_width).max(hints) + 2,
            self.actions.len() + 1,
        )
    }

    /// The menu's rows at `width`, and a line of hints under them.
    pub fn render(&self, theme: &Theme, width: usize) -> Vec<Vec<Segment>> {
        let label_width = self
            .actions
            .iter()
            .map(|action| cell_len(&action.label))
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        for (index, action) in self.actions.iter().enumerate() {
            let focused = index == self.focus;
            let mut line = if focused {
                vec![text(format!("  {} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("    ")]
            };
            let padding = label_width - cell_len(&action.label);
            line.push(if focused {
                text(action.label.clone(), &theme.focused)
            } else {
                plain(action.label.clone())
            });
            if let Some(key) = action.key {
                line.push(text(format!("{}  {key}", " ".repeat(padding)), &theme.hint));
            }
            lines.push(fit(line, width));
        }
        lines.push(fit(vec![text(MENU_HINTS.to_string(), &theme.hint)], width));
        lines
    }
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

    #[test]
    fn slices_and_overlays_by_cell() {
        let base = vec![plain("ab"), plain("日本"), plain("cd")];
        let cut: String = slice(&base, 3, 7).iter().map(|s| s.text.as_str()).collect();
        assert_eq!(cut, " 本c", "half a wide character is a space");
        let over = overlay(&base, 1, &[plain("XY")]);
        let text: String = over.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "aXY 本cd");
        let past = overlay(&[plain("ab")], 4, &[plain("Z")]);
        assert_eq!(width(&past), 5);
    }

    #[test]
    fn a_list_follows_its_cursor() {
        let mut list = ListState::new();
        list.set_len(20);
        list.step(7, 5);
        assert_eq!((list.cursor(), list.offset()), (7, 3));
        assert_eq!(list.visible(5), 3..8);
        list.step(-100, 5);
        assert_eq!((list.cursor(), list.offset()), (0, 0));
        list.step(isize::MAX / 2, 5);
        assert_eq!(list.cursor(), 19);
        list.toggle(4);
        list.toggle(2);
        assert_eq!(list.selected(), [2, 4]);
        list.toggle(4);
        assert_eq!(list.selected_count(), 1);
    }

    #[test]
    fn a_filter_ranks_and_hides() {
        let mut filter = FilterState::new(["alpha", "beta", "gamma"]);
        filter.set_hidden(vec![false, true, false]);
        filter.refilter();
        assert_eq!(filter.len(), 2, "hidden while nothing is typed");
        filter.set_query("ta");
        assert_eq!(filter.index(0), Some(1), "hidden ones still match a query");
        assert!(filter.pop());
        filter.clear();
        assert!(!filter.is_filtering());
        filter.set_highlight_limits(vec![2, 2, 2]);
        filter.set_query("ma");
        assert!(filter.matches()[0].1.iter().all(|p| *p < 2));
    }

    #[test]
    fn a_text_buffer_edits_by_grapheme() {
        let mut buffer = TextBuffer::with_text("cafe\u{301} au lait");
        buffer.checkpoint();
        buffer.delete_word();
        assert_eq!(buffer.text(), "cafe\u{301} au ");
        buffer.home();
        buffer.right();
        buffer.right();
        buffer.right();
        buffer.right();
        assert_eq!(buffer.caret(), 5, "the accent moves with its letter");
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "caf au ");
        buffer.delete_to_start();
        assert_eq!(buffer.text(), " au ");
        assert!(buffer.undo());
        assert_eq!(buffer.text(), "cafe\u{301} au lait");
        let (shown, column) = TextBuffer::with_text("abcdefgh").window(4, Some('*'));
        assert_eq!((shown.as_str(), column), ("***", 3));
    }

    #[test]
    fn a_divider_drags_within_its_minimums() {
        let mut divider = Divider::new(3).min(12);
        assert_eq!(divider.clamp(5, 80), 12);
        assert_eq!(divider.clamp(79, 80), 80 - 12 - 3);
        assert_eq!(divider.resolve(80, 36), 36);
        assert!(!divider.press(10, 36));
        assert!(divider.press(37, 36));
        assert!(divider.drag(50, 80));
        divider.release();
        assert!(!divider.drag(60, 80));
        assert_eq!(divider.resolve(80, 36), 50);
        divider.nudge(-100, 50, 80);
        assert_eq!(divider.resolve(80, 36), 12);
    }
}

//! Fuzzy selection (#287), single and multiple, with a preview pane
//! (#454).
//!
//! Typing filters the items with the [fuzzy](crate::fuzzy) matcher and
//! highlights what matched; the arrows move; Enter picks. A multi-select
//! marks items with Tab. When the focused item has a
//! [`Preview`](crate::Preview), a pane shows it beside the list on a wide
//! terminal and below it on a narrow one. An item's
//! [`Action`](crate::Action) keys pick it and record which action was asked
//! for; Ctrl+K opens a menu of every action for the focused item, its own
//! and the view's ([`Actions`], #491).
//!
//! With the mouse on (#476), a click focuses a row and a second click on it
//! picks it, the wheel moves, and the border between the list and a preview
//! beside it can be dragged to resize the panes.

use std::cell::Cell;
use std::time::Duration;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, pad, plain, question, text, Theme};
use crate::event::{Button, Event, Key, KeyCode, Mouse, MouseKind};
use crate::fuzzy::rank;
use crate::item::{Action, ActionTarget, Actions, Item, TargetKind};
use crate::policy::{LineIo, NotInteractive};

/// Where the preview pane goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewLayout {
    /// Beside the list from 72 columns, below it when narrower.
    #[default]
    Auto,
    Right,
    Below,
    Hidden,
}

/// The narrowest a pane gets when the border between them is dragged.
const MIN_PANE: usize = 12;

/// The action menu, while it is open.
struct Menu {
    actions: Vec<Action>,
    focus: usize,
}

/// A fuzzy single choice among items. Enter returns the focused item's
/// value; Escape cancels.
pub struct Select<T> {
    prompt: String,
    items: Vec<Item<T>>,
    query: String,
    /// Item indices that match the query, best first, with the label
    /// characters to highlight.
    matches: Vec<(usize, Vec<usize>)>,
    focus: usize,
    offset: usize,
    height: usize,
    multi: bool,
    marked: Vec<bool>,
    preview: PreviewLayout,
    preview_height: usize,
    theme: Theme,
    action: Option<String>,
    default: Option<usize>,
    /// Repaint at least this often, for previews that change by themselves.
    repaint: Option<Duration>,
    /// Once finished: the answer shown in place of the list (`None` inside
    /// means cancelled).
    answer: Option<Option<String>>,
    /// What an action is done to here, and the actions offered on every
    /// item (#491).
    kind: TargetKind,
    actions: Actions,
    menu_key: Key,
    menu: Option<Menu>,
    /// Each item's identity for an action ([`ActionTarget::value`]); the
    /// label when empty.
    values: Vec<String>,
    /// A line above the list, under the question: a table's headings.
    pub(crate) heading: Option<Vec<Segment>>,
    /// Text before each label, not searched: a tree's guides, an emoji.
    pub(crate) prefixes: Vec<String>,
    /// Prefixes drawn as they are rather than dimmed.
    pub(crate) prefix_plain: bool,
    /// Items left out while nothing is typed: a collapsed node's children.
    pub(crate) hidden: Vec<bool>,
    /// A line of extra key hints for the footer.
    pub(crate) hints: Option<String>,
    /// Always take `height` rows, however few items there are: for lists
    /// that change under the user, such as a directory's entries.
    pub(crate) steady: bool,
    mouse: bool,
    /// The list position the last click focused, until a key: a click on
    /// it again picks it.
    clicked: Option<usize>,
    /// Rows on screen, from the last context: the list is cut to fit.
    space: Cell<usize>,
    /// The list's width beside the preview, once the border is dragged.
    split: Option<usize>,
    dragging: bool,
}

impl<T> Select<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> Select<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let items: Vec<Item<T>> = items.into_iter().map(Into::into).collect();
        let mut select = Select {
            prompt: prompt.into(),
            marked: vec![false; items.len()],
            items,
            query: String::new(),
            matches: Vec::new(),
            focus: 0,
            offset: 0,
            height: 10,
            multi: false,
            preview: PreviewLayout::Auto,
            preview_height: 10,
            theme: Theme::default(),
            action: None,
            default: None,
            repaint: None,
            answer: None,
            kind: TargetKind::Item,
            actions: Actions::new(),
            menu_key: Key::ctrl('k'),
            menu: None,
            values: Vec::new(),
            heading: None,
            prefixes: Vec::new(),
            prefix_plain: false,
            hidden: Vec::new(),
            hints: None,
            steady: false,
            mouse: false,
            clicked: None,
            space: Cell::new(usize::MAX),
            split: None,
            dragging: false,
        };
        select.refilter();
        select
    }

    /// Show at most `rows` items at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.preview = layout;
        self
    }

    /// Rows the preview takes when it is below the list (default 10).
    pub fn preview_height(mut self, rows: usize) -> Self {
        self.preview_height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Start with this filter text.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self.refilter();
        self
    }

    /// Render again at least every `interval`, even with no key pressed:
    /// for previews that fill in by themselves, such as a command's output
    /// computed on another thread. An unchanged view still writes nothing.
    pub fn repaint_every(mut self, interval: Duration) -> Self {
        self.repaint = Some(interval);
        self
    }

    /// The item returned without a terminal when the policy asks for
    /// defaults, and focused first.
    pub fn default(mut self, index: usize) -> Self {
        if index < self.items.len() {
            self.default = Some(index);
            if let Some(position) = self.matches.iter().position(|(i, _)| *i == index) {
                self.focus = position;
            }
        }
        self
    }

    /// Actions offered on every item, beside each item's own (#491): in
    /// the action menu, and on their keys.
    pub fn actions(mut self, actions: Actions) -> Self {
        self.actions = actions;
        self
    }

    /// The key that opens the action menu (default Ctrl+K).
    pub fn menu_key(mut self, key: Key) -> Self {
        self.menu_key = key;
        self
    }

    /// Report the mouse (#476): a click focuses a row, a second click picks
    /// it, and the border beside a preview drags.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// The list's width beside a preview, in columns: where the border is
    /// until it is dragged.
    pub fn split(mut self, columns: usize) -> Self {
        self.split = Some(columns);
        self
    }

    /// The id of the [`Action`] that picked the item, from its key or the
    /// action menu, if one did.
    pub fn action(&self) -> Option<&str> {
        self.action.as_deref()
    }

    pub fn items(&self) -> &[Item<T>] {
        &self.items
    }

    /// The focused item's index, if any item matches.
    pub fn focused(&self) -> Option<usize> {
        self.matches.get(self.focus).map(|(index, _)| *index)
    }

    /// The list's width beside a preview, once dragged.
    pub fn split_width(&self) -> Option<usize> {
        self.split
    }

    pub(crate) fn set_kind(&mut self, kind: TargetKind) {
        self.kind = kind;
    }

    pub(crate) fn set_values(&mut self, values: Vec<String>) {
        self.values = values;
    }

    /// Replace the items, keeping the settings; the query is cleared.
    pub(crate) fn replace_items(&mut self, items: Vec<Item<T>>) {
        self.marked = vec![false; items.len()];
        self.items = items;
        self.values.clear();
        self.prefixes.clear();
        self.hidden.clear();
        self.query.clear();
        self.default = None;
        self.menu = None;
        self.focus = 0;
        // A click on the old items is not the first of a pair on the new
        // ones, which may have another item at the same position.
        self.clicked = None;
        self.refilter();
    }

    pub(crate) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    pub(crate) fn query_text(&self) -> &str {
        &self.query
    }

    pub(crate) fn set_answer(&mut self, answer: Option<String>) {
        self.answer = Some(answer);
    }

    /// Carry on after an Enter that did not finish (a directory opened).
    pub(crate) fn reopen(&mut self) {
        self.answer = None;
        self.action = None;
    }

    /// Focus item `index` if it is listed.
    pub(crate) fn focus_item(&mut self, index: usize) {
        if let Some(position) = self.matches.iter().position(|(i, _)| *i == index) {
            self.focus = position;
            self.scroll();
        }
    }

    /// The target an action on item `index` is done to.
    pub fn target(&self, index: usize) -> ActionTarget {
        let label = self.items[index].label.clone();
        ActionTarget {
            kind: self.kind,
            index,
            value: self
                .values
                .get(index)
                .cloned()
                .unwrap_or_else(|| label.clone()),
            label,
        }
    }

    /// Every action for item `index`: its own, then the view's that apply.
    pub fn actions_for(&self, index: usize) -> Vec<Action> {
        self.actions
            .for_target(&self.target(index), &self.items[index].actions)
    }

    pub(crate) fn refilter(&mut self) {
        let focused = self.focused();
        let texts: Vec<String> = self.items.iter().map(Item::search_text).collect();
        let filtering = !self.query.is_empty();
        self.matches = rank(&self.query, texts.iter().map(String::as_str))
            .into_iter()
            .filter(|(index, _)| filtering || !self.hidden.get(*index).copied().unwrap_or(false))
            .map(|(index, found)| {
                let label = self.items[index].label.chars().count();
                let mut positions = found.positions;
                positions.retain(|position| *position < label);
                (index, positions)
            })
            .collect();
        // Keep the focused item when it still matches, else the best match.
        self.focus = focused
            .and_then(|index| self.matches.iter().position(|(i, _)| *i == index))
            .filter(|_| self.query.is_empty())
            .unwrap_or(0);
        self.offset = 0;
        self.scroll();
    }

    fn scroll(&mut self) {
        if self.focus < self.offset {
            self.offset = self.focus;
        } else if self.focus >= self.offset + self.shown() {
            self.offset = self.focus + 1 - self.shown();
        }
    }

    fn step(&mut self, delta: isize) {
        if self.matches.is_empty() {
            return;
        }
        let last = self.matches.len() - 1;
        self.focus = self.focus.saturating_add_signed(delta).min(last);
        self.scroll();
    }

    /// Handle an event; `Some` finishes with item indices, and the view
    /// collapses to the answer.
    pub(crate) fn event(
        &mut self,
        event: &Event,
        context: &Context<'_>,
    ) -> Option<Flow<Vec<usize>>> {
        self.space.set(context.height);
        let flow = self.event_inner(event, context.width);
        match &flow {
            Some(Flow::Done(indices)) => {
                let labels: Vec<&str> = indices
                    .iter()
                    .map(|&index| self.items[index].label.as_str())
                    .collect();
                self.answer = Some(Some(labels.join(", ")));
            }
            Some(Flow::Cancel) => self.answer = Some(None),
            _ => {}
        }
        flow
    }

    /// Pick `index` with `action`.
    fn run_action(&mut self, index: usize, action: &Action) -> Option<Flow<Vec<usize>>> {
        self.menu = None;
        self.action = Some(action.id.clone());
        Some(Flow::Done(vec![index]))
    }

    fn pick_focused(&mut self) -> Option<Flow<Vec<usize>>> {
        let index = self.focused()?;
        let marked: Vec<usize> = (0..self.items.len()).filter(|&i| self.marked[i]).collect();
        Some(Flow::Done(if self.multi && !marked.is_empty() {
            marked
        } else {
            vec![index]
        }))
    }

    fn menu_event(&mut self, event: &Event) -> Option<Flow<Vec<usize>>> {
        let index = self.focused()?;
        // The menu's rows follow the list's: see `render_menu`.
        let first = self.menu_top();
        let menu = self.menu.as_mut()?;
        let count = menu.actions.len();
        if let Some(mouse) = event.mouse() {
            if mouse.is_click() {
                let row = mouse.row as usize;
                if row >= first && row < first + count {
                    let action = menu.actions[row - first].clone();
                    return self.run_action(index, &action);
                }
                self.menu = None;
            }
            return None;
        }
        let key = event.key()?;
        match key.code {
            KeyCode::Escape => self.menu = None,
            _ if key == self.menu_key => self.menu = None,
            KeyCode::Up => menu.focus = (menu.focus + count - 1) % count.max(1),
            KeyCode::Down | KeyCode::Tab => menu.focus = (menu.focus + 1) % count.max(1),
            KeyCode::Enter => {
                let action = menu.actions[menu.focus].clone();
                return self.run_action(index, &action);
            }
            _ => {
                let found = menu
                    .actions
                    .iter()
                    .find(|action| action.key == Some(key))
                    .cloned();
                if let Some(action) = found {
                    return self.run_action(index, &action);
                }
            }
        }
        None
    }

    fn event_inner(&mut self, event: &Event, width: usize) -> Option<Flow<Vec<usize>>> {
        if self.menu.is_some() {
            return self.menu_event(event);
        }
        if let Event::Mouse(mouse) = event {
            return self.mouse_event(*mouse, width);
        }
        if let Event::Paste(text) = event {
            self.clicked = None;
            self.query.push_str(&crate::components::pasted(text, " "));
            self.refilter();
            return None;
        }
        let key = event.key()?;
        // A click after a key is a first click again.
        self.clicked = None;
        if let Some(index) = self.focused() {
            let actions = self.actions_for(index);
            if key == self.menu_key && !actions.is_empty() {
                self.menu = Some(Menu { actions, focus: 0 });
                return None;
            }
            if let Some(action) = actions.iter().find(|action| action.key == Some(key)) {
                let action = action.clone();
                return self.run_action(index, &action);
            }
        }
        let ctrl = key.modifiers.ctrl;
        match key.code {
            KeyCode::Enter => return self.pick_focused(),
            KeyCode::Escape => return Some(Flow::Cancel),
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::Char('p') if ctrl => self.step(-1),
            KeyCode::Char('n') if ctrl => self.step(1),
            KeyCode::PageUp => self.step(-(self.shown() as isize)),
            KeyCode::PageDown => self.step(self.shown() as isize),
            KeyCode::Home => self.step(isize::MIN / 2),
            KeyCode::End => self.step(isize::MAX / 2),
            KeyCode::Tab | KeyCode::BackTab if self.multi => {
                if let Some(index) = self.focused() {
                    self.marked[index] = !self.marked[index];
                    self.step(if key.code == KeyCode::Tab { 1 } else { -1 });
                }
            }
            KeyCode::Char('a') if ctrl && self.multi => {
                let all = self.matches.iter().all(|(i, _)| self.marked[*i]);
                for (index, _) in &self.matches {
                    self.marked[*index] = !all;
                }
            }
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Backspace => {
                if self.query.pop().is_some() {
                    self.refilter();
                }
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.alt => {
                self.query.push(c);
                self.refilter();
            }
            _ => {}
        }
        None
    }

    /// Rows above the list: the question, and the heading if any.
    fn top(&self) -> usize {
        1 + usize::from(self.heading.is_some())
    }

    /// The most rows the list takes: the height asked for, cut to what
    /// fits on screen beside the question, the heading and the footer.
    fn shown(&self) -> usize {
        self.height
            .min(self.space.get().saturating_sub(self.top() + 1))
            .max(1)
    }

    /// Rows the list takes.
    fn rows(&self) -> usize {
        if self.steady {
            return self.shown();
        }
        self.shown().min(self.items.len()).max(1)
    }

    /// The first row of the action menu: under the list, where the footer
    /// was.
    fn menu_top(&self) -> usize {
        self.top() + self.rows()
    }

    /// The preview's layout at `width` for the focused item, and the
    /// list's width when the preview is beside it.
    fn layout(&self, width: usize) -> (PreviewLayout, usize) {
        let preview = self
            .focused()
            .is_some_and(|index| self.items[index].preview.is_some())
            && self.preview != PreviewLayout::Hidden;
        if !preview {
            return (PreviewLayout::Hidden, width);
        }
        let layout = match self.preview {
            PreviewLayout::Auto if width >= 72 => PreviewLayout::Right,
            PreviewLayout::Auto => PreviewLayout::Below,
            layout => layout,
        };
        let left = match self.split {
            Some(split) => clamp_split(split, width),
            None => (width * 45 / 100).max(20).min(width),
        };
        (layout, left)
    }

    fn mouse_event(&mut self, mouse: Mouse, width: usize) -> Option<Flow<Vec<usize>>> {
        let (layout, left) = self.layout(width);
        let (row, column) = (mouse.row as usize, mouse.column as usize);
        let top = self.top();
        let in_list = row >= top && row < top + self.rows();
        match mouse.kind {
            MouseKind::ScrollUp => self.step(-1),
            MouseKind::ScrollDown => self.step(1),
            MouseKind::Down(Button::Left) => {
                let border = layout == PreviewLayout::Right && (left..left + 3).contains(&column);
                if border && in_list {
                    self.dragging = true;
                } else if in_list && (layout != PreviewLayout::Right || column < left) {
                    let position = self.offset + row - top;
                    if position < self.matches.len() {
                        if self.clicked == Some(position) && position == self.focus {
                            return self.pick_focused();
                        }
                        self.focus = position;
                        self.clicked = Some(position);
                    }
                }
            }
            MouseKind::Drag(Button::Left) if self.dragging => {
                self.split = Some(clamp_split(column, width));
            }
            MouseKind::Up(_) => self.dragging = false,
            _ => {}
        }
        None
    }

    fn row(&self, position: usize, width: usize) -> Vec<Segment> {
        let (index, positions) = &self.matches[position];
        let item = &self.items[*index];
        let theme = &self.theme;
        let focused = position == self.focus;
        let mut line = if focused {
            vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
        } else {
            vec![plain("  ")]
        };
        if self.multi {
            line.push(if self.marked[*index] {
                text(format!("{} ", theme.checked), &theme.checked_style)
            } else {
                text(format!("{} ", theme.unchecked), &theme.hint)
            });
        }
        if let Some(prefix) = self.prefixes.get(*index).filter(|p| !p.is_empty()) {
            line.push(if self.prefix_plain {
                plain(prefix.clone())
            } else {
                text(prefix.clone(), &theme.hint)
            });
        }
        let base = focused.then_some(&theme.focused);
        line.extend(highlight(&item.label, positions, base, &theme.matched));
        if let Some(description) = &item.description {
            line.push(text(format!("  {description}"), &theme.hint));
        }
        fit(line, width)
    }

    fn list(&self, width: usize) -> Vec<Vec<Segment>> {
        let rows = self.rows();
        let mut lines: Vec<Vec<Segment>> = (self.offset..self.matches.len())
            .take(rows)
            .map(|position| self.row(position, width))
            .collect();
        if self.matches.is_empty() {
            lines.push(vec![text("  no matches", &self.theme.hint)]);
        }
        // A steady height, so the footer does not jump as the list filters.
        lines.resize_with(rows, Vec::new);
        lines
    }

    fn footer(&self, width: usize) -> Vec<Segment> {
        let mut hint = format!("{}/{}", self.matches.len(), self.items.len());
        if self.multi {
            let marked = self.marked.iter().filter(|m| **m).count();
            hint.push_str(&format!(" · {marked} marked · tab mark"));
        }
        hint.push_str(" · ↑↓ move · enter pick");
        if let Some(extra) = &self.hints {
            hint.push_str(&format!(" · {extra}"));
        }
        if self
            .focused()
            .is_some_and(|index| !self.actions_for(index).is_empty())
        {
            hint.push_str(&format!(" · {} actions", self.menu_key));
        }
        hint.push_str(" · esc cancel");
        fit(vec![text(format!("  {hint}"), &self.theme.hint)], width)
    }

    /// The action menu, in place of the footer.
    fn render_menu(&self, menu: &Menu, width: usize) -> Vec<Vec<Segment>> {
        let theme = &self.theme;
        let label_width = menu
            .actions
            .iter()
            .map(|action| rich::cells::cell_len(&action.label))
            .max()
            .unwrap_or(0);
        let mut lines = Vec::new();
        for (index, action) in menu.actions.iter().enumerate() {
            let focused = index == menu.focus;
            let mut line = if focused {
                vec![text(format!("  {} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("    ")]
            };
            let padding = label_width - rich::cells::cell_len(&action.label);
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
        lines.push(fit(
            vec![text(
                "  ↑↓ move · enter run · esc close".to_string(),
                &theme.hint,
            )],
            width,
        ));
        lines
    }

    fn render_view(&self, context: &Context<'_>) -> View {
        self.space.set(context.height);
        let width = context.width;
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            header.push(match answer {
                Some(answer) => text(answer.clone(), &theme.answer),
                None => text("cancelled", &theme.hint),
            });
            return View::new(vec![fit(header, width)]);
        }
        let column = crate::components::width(&header) + rich::cells::cell_len(&self.query);
        header.push(plain(self.query.clone()));
        let mut lines = vec![fit(header, width)];
        if let Some(heading) = &self.heading {
            let mut line = vec![plain(if self.multi { "    " } else { "  " })];
            line.extend(heading.iter().cloned());
            lines.push(fit(line, width));
        }
        let footer = |this: &Self| match &this.menu {
            Some(menu) => this.render_menu(menu, width),
            None => vec![this.footer(width)],
        };
        let preview = self
            .focused()
            .and_then(|index| self.items[index].preview.as_ref());
        match (preview, self.layout(width)) {
            (Some(preview), (PreviewLayout::Right, left)) => {
                let right = width.saturating_sub(left + 3).max(1);
                let list = self.list(left);
                let shown = context.lines_at(&*preview.renderable(), right);
                for (row, line) in list.into_iter().enumerate() {
                    let mut line = pad(line, left);
                    line.push(text(" │ ", &theme.border));
                    if let Some(preview) = shown.get(row) {
                        line.extend(fit(preview.clone(), right));
                    }
                    lines.push(line);
                }
                lines.extend(footer(self));
            }
            (Some(preview), (PreviewLayout::Below, _)) => {
                lines.extend(self.list(width));
                lines.extend(footer(self));
                lines.push(vec![text("─".repeat(width), &theme.border)]);
                let shown = context.lines_at(&*preview.renderable(), width);
                lines.extend(
                    shown
                        .into_iter()
                        .take(self.preview_height)
                        .map(|line| fit(line, width)),
                );
            }
            _ => {
                lines.extend(self.list(width));
                lines.extend(footer(self));
            }
        }
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    /// Ask without a terminal: list the items, read a number or text.
    fn ask(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<usize>>, NotInteractive> {
        io.write(&format!("{}\n", self.prompt));
        for (index, item) in self.items.iter().enumerate() {
            let mut row = format!("  {}) {}", index + 1, item.label);
            if let Some(description) = &item.description {
                row.push_str(&format!(" — {description}"));
            }
            io.write(&format!("{row}\n"));
        }
        io.write(if self.multi {
            "Numbers or names, separated by commas: "
        } else {
            "Number or name: "
        });
        // At the end of input, or on an empty line, the default answers:
        // the default item, or with `multi` the marked ones.
        let default: Vec<usize> = if self.multi {
            (0..self.items.len()).filter(|&i| self.marked[i]).collect()
        } else {
            self.default.into_iter().collect()
        };
        let Some(line) = io.read_line() else {
            if default.is_empty() {
                return Err(NotInteractive::Ended);
            }
            return Ok(Some(default));
        };
        if line.trim().is_empty() && !default.is_empty() {
            return Ok(Some(default));
        }
        let answers: Vec<&str> = if self.multi {
            line.split(',')
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .collect()
        } else {
            vec![line.trim()]
        };
        let texts: Vec<String> = self.items.iter().map(Item::search_text).collect();
        let mut chosen = Vec::new();
        for answer in answers {
            let index = match answer.parse::<usize>() {
                Ok(number) if (1..=self.items.len()).contains(&number) => number - 1,
                _ => rank(answer, texts.iter().map(String::as_str))
                    .first()
                    .map(|(index, _)| *index)
                    .filter(|_| !answer.is_empty())
                    .ok_or_else(|| {
                        NotInteractive::Invalid(format!("no item matches {answer:?}"))
                    })?,
            };
            if !chosen.contains(&index) {
                chosen.push(index);
            }
        }
        if chosen.is_empty() {
            return Err(NotInteractive::Invalid("nothing chosen".into()));
        }
        Ok(Some(chosen))
    }
}

/// The list's width beside a preview when the border is at `column`: both
/// panes at least [`MIN_PANE`] wide when there is room for that.
fn clamp_split(column: usize, width: usize) -> usize {
    let most = width.saturating_sub(MIN_PANE + 3).max(MIN_PANE.min(width));
    column.clamp(MIN_PANE.min(most), most)
}

impl<T: Clone> Component for Select<T> {
    type Output = T;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
        match self.event(event, context) {
            Some(Flow::Done(indices)) => Flow::Done(self.items[indices[0]].value.clone()),
            Some(Flow::Cancel) => Flow::Cancel,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.render_view(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.repaint
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn default_value(&self) -> Option<T> {
        self.default.map(|index| self.items[index].value.clone())
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<T>, NotInteractive> {
        Ok(self
            .ask(io)?
            .map(|indices| self.items[indices[0]].value.clone()))
    }
}

/// A fuzzy multiple choice: Tab marks items (Shift+Tab marks and moves up,
/// Ctrl+A marks every match), Enter returns the marked items in list
/// order, or the focused one when none is marked.
pub struct MultiSelect<T>(Select<T>);

impl<T> MultiSelect<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> MultiSelect<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let mut select = Select::new(prompt, items);
        select.multi = true;
        MultiSelect(select)
    }

    pub fn height(self, rows: usize) -> Self {
        MultiSelect(self.0.height(rows))
    }

    pub fn preview(self, layout: PreviewLayout) -> Self {
        MultiSelect(self.0.preview(layout))
    }

    pub fn theme(self, theme: Theme) -> Self {
        MultiSelect(self.0.theme(theme))
    }

    pub fn query(self, query: impl Into<String>) -> Self {
        MultiSelect(self.0.query(query))
    }

    /// See [`Select::repaint_every`].
    pub fn repaint_every(self, interval: Duration) -> Self {
        MultiSelect(self.0.repaint_every(interval))
    }

    /// See [`Select::actions`].
    pub fn actions(self, actions: Actions) -> Self {
        MultiSelect(self.0.actions(actions))
    }

    /// See [`Select::with_mouse`].
    pub fn with_mouse(self, on: bool) -> Self {
        MultiSelect(self.0.with_mouse(on))
    }

    /// Mark these items to begin with; they are also the default without a
    /// terminal.
    pub fn marked(mut self, indices: impl IntoIterator<Item = usize>) -> Self {
        for index in indices {
            if let Some(mark) = self.0.marked.get_mut(index) {
                *mark = true;
            }
        }
        self
    }

    pub fn action(&self) -> Option<&str> {
        self.0.action()
    }

    pub fn items(&self) -> &[Item<T>] {
        self.0.items()
    }
}

impl<T: Clone> Component for MultiSelect<T> {
    type Output = Vec<T>;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Vec<T>> {
        match self.0.event(event, context) {
            Some(Flow::Done(indices)) => Flow::Done(
                indices
                    .into_iter()
                    .map(|index| self.0.items[index].value.clone())
                    .collect(),
            ),
            Some(Flow::Cancel) => Flow::Cancel,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.0.render_view(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.0.repaint
    }

    fn mouse(&self) -> bool {
        self.0.mouse
    }

    fn default_value(&self) -> Option<Vec<T>> {
        Some(
            (0..self.0.items.len())
                .filter(|&index| self.0.marked[index])
                .map(|index| self.0.items[index].value.clone())
                .collect(),
        )
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Vec<T>>, NotInteractive> {
        Ok(self.0.ask(io)?.map(|indices| {
            indices
                .into_iter()
                .map(|index| self.0.items[index].value.clone())
                .collect()
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dragged_split_keeps_both_panes_usable() {
        assert_eq!(clamp_split(5, 80), MIN_PANE);
        assert_eq!(clamp_split(40, 80), 40);
        assert_eq!(clamp_split(79, 80), 80 - MIN_PANE - 3);
        // Too narrow for both minimums: the list keeps what it can.
        assert!(clamp_split(3, 20) <= 20);
    }
}

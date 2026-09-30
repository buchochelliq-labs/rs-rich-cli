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
//!
//! Since 0.0.14 a select is made of the public [kit](crate::kit): a
//! [`FilterState`] ranks the items against the query, a [`ListState`] holds
//! the cursor, the scroll and the marks, a [`Divider`] is the preview's
//! border and an [`ActionMenu`] is the Ctrl+K menu. Its keys are declared in
//! a [`Keymap`] (context `select`), so they can be rebound and listed.

use std::cell::Cell;
use std::time::Duration;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, pad, plain, question, text, Theme};
use crate::event::{Button, Event, Key, KeyCode, Modifiers, Mouse, MouseKind};
use crate::fuzzy::rank;
use crate::item::{Action, ActionTarget, Actions, Item, TargetKind};
use crate::keymap::{keys, Keymap};
use crate::kit::{ActionMenu, Divider, FilterState, ListState, MenuReply};
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

/// The keys of a [`Select`] (and of the views built on one), in context
/// `select`. The `mark` actions apply to a [`MultiSelect`].
pub fn select_keymap() -> Keymap {
    Keymap::new("select")
        .bind("pick", keys("enter"), "pick")
        .bind("cancel", keys("escape"), "cancel")
        .bind("up", keys("up ctrl+p"), "move up")
        .bind("down", keys("down ctrl+n"), "move down")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("first", keys("home"), "first")
        .bind("last", keys("end"), "last")
        .bind("mark", keys("tab"), "mark and move down")
        .bind("mark-up", keys("shift+tab"), "mark and move up")
        .bind("mark-all", keys("ctrl+a"), "mark every match")
        .bind("clear", keys("ctrl+u"), "clear the filter")
        .bind("delete", keys("backspace"), "delete a character")
        .bind("actions", keys("ctrl+k"), "actions")
}

/// Actions only a multi-select has.
const MARKING: [&str; 3] = ["mark", "mark-up", "mark-all"];

/// A fuzzy single choice among items. Enter returns the focused item's
/// value; Escape cancels.
pub struct Select<T> {
    prompt: String,
    items: Vec<Item<T>>,
    /// The query, and the item indices that match it, best first, with the
    /// label characters to highlight.
    filter: FilterState,
    /// The cursor over the matches, the first shown, and the marked items
    /// (by item index).
    list: ListState,
    height: usize,
    multi: bool,
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
    menu: Option<ActionMenu>,
    keymap: Keymap,
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
    /// The border between the list and a preview beside it.
    divider: Divider,
}

impl<T> Select<T> {
    pub fn new<I, V>(prompt: impl Into<String>, items: I) -> Select<T>
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        let items: Vec<Item<T>> = items.into_iter().map(Into::into).collect();
        let mut list = ListState::new();
        list.clear_selection(items.len());
        let mut select = Select {
            prompt: prompt.into(),
            items,
            filter: FilterState::default(),
            list,
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
            keymap: select_keymap(),
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
            divider: Divider::new(3).min(MIN_PANE),
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
        *self.filter.query_mut() = query.into();
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
            if let Some(position) = self.filter.position_of(index) {
                self.list.set_cursor(position);
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
        self.keymap.rebind("actions", [key]);
        self
    }

    /// Make `keys` do `action` (see [`select_keymap`]) on this select only;
    /// no keys unbinds it.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
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
        self.divider.set_position(Some(columns));
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
        self.filter.index(self.list.cursor())
    }

    /// The list's width beside a preview, once dragged.
    pub fn split_width(&self) -> Option<usize> {
        self.divider.position()
    }

    /// The query and what matches it.
    pub fn filter(&self) -> &FilterState {
        &self.filter
    }

    /// The cursor, the scroll and the marks.
    pub fn list(&self) -> &ListState {
        &self.list
    }

    /// The border beside the preview.
    pub fn divider(&self) -> &Divider {
        &self.divider
    }

    /// The action menu, while it is open.
    pub fn menu(&self) -> Option<&ActionMenu> {
        self.menu.as_ref()
    }

    /// The keys, with any rebinding.
    pub fn select_keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// Whether this is the list of a [`MultiSelect`].
    pub fn is_multi(&self) -> bool {
        self.multi
    }

    /// What an action is done to: an item, a row, a node, a file.
    pub fn set_kind(&mut self, kind: TargetKind) {
        self.kind = kind;
    }

    /// Each item's identity for an action ([`ActionTarget::value`]), by
    /// index; the label where missing.
    pub fn set_values(&mut self, values: Vec<String>) {
        self.values = values;
    }

    /// A line above the list, under the question: a table's headings.
    pub fn set_heading(&mut self, heading: Option<Vec<Segment>>) {
        self.heading = heading;
    }

    /// Text before each label, by index, not searched: a tree's guides, an
    /// emoji. Dimmed, unless `plain`. Call [`refilter`](Self::refilter)
    /// after.
    pub fn set_prefixes(&mut self, prefixes: Vec<String>, plain: bool) {
        self.prefixes = prefixes;
        self.prefix_plain = plain;
    }

    /// Items left out while nothing is typed, by index: a collapsed node's
    /// children. Call [`refilter`](Self::refilter) after.
    pub fn set_hidden(&mut self, hidden: Vec<bool>) {
        self.hidden = hidden;
    }

    /// A line of extra key hints for the footer.
    pub fn set_hints(&mut self, hints: Option<String>) {
        self.hints = hints;
    }

    /// Always take the full height, however few items there are: for lists
    /// that change under the user, such as a directory's entries.
    pub fn set_steady(&mut self, steady: bool) {
        self.steady = steady;
    }

    /// Replace the items, keeping the settings; the query is cleared.
    pub fn replace_items(&mut self, items: Vec<Item<T>>) {
        self.list.clear_selection(items.len());
        self.items = items;
        self.values.clear();
        self.prefixes.clear();
        self.hidden.clear();
        self.filter.query_mut().clear();
        self.default = None;
        self.menu = None;
        self.list.set_cursor(0);
        // A click on the old items is not the first of a pair on the new
        // ones, which may have another item at the same position.
        self.clicked = None;
        self.refilter();
    }

    /// Whether the action menu is open.
    pub fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// What is typed.
    pub fn query_text(&self) -> &str {
        self.filter.query()
    }

    /// Show `answer` in place of the list (`None`: cancelled), as a
    /// finished select does.
    pub fn set_answer(&mut self, answer: Option<String>) {
        self.answer = Some(answer);
    }

    /// Carry on after an Enter that did not finish (a directory opened).
    pub fn reopen(&mut self) {
        self.answer = None;
        self.action = None;
    }

    /// Focus item `index` if it is listed.
    pub fn focus_item(&mut self, index: usize) {
        if let Some(position) = self.filter.position_of(index) {
            self.list.set_cursor(position);
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

    /// Match the items against the query again, after the items, their
    /// prefixes or what is hidden changed.
    pub fn refilter(&mut self) {
        let focused = self.focused();
        self.filter.set_hidden(self.hidden.clone());
        self.filter.set_highlight_limits(
            self.items
                .iter()
                .map(|item| item.label.chars().count())
                .collect(),
        );
        self.filter
            .set_candidates(self.items.iter().map(Item::search_text));
        // Keep the focused item when it still matches, else the best match.
        let cursor = focused
            .and_then(|index| self.filter.position_of(index))
            .filter(|_| !self.filter.is_filtering())
            .unwrap_or(0);
        self.list.set_len(self.filter.len());
        self.list.reset(cursor, self.shown());
    }

    fn scroll(&mut self) {
        self.list.follow(self.shown());
    }

    fn step(&mut self, delta: isize) {
        self.list.step(delta, self.shown());
    }

    /// Handle an event as the select does, for a component built round
    /// one: `Some(Flow::Done)` finishes with item indices (the view then
    /// collapses to the answer), `Some(Flow::Cancel)` cancels,
    /// `Some(Flow::Ignored)` means the event was not the select's, and
    /// `None` carries on.
    pub fn event(&mut self, event: &Event, context: &Context<'_>) -> Option<Flow<Vec<usize>>> {
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
        let marked = self.list.selected();
        Some(Flow::Done(if self.multi && !marked.is_empty() {
            marked
        } else {
            vec![index]
        }))
    }

    fn menu_event(&mut self, event: &Event) -> Option<Flow<Vec<usize>>> {
        let index = self.focused()?;
        // The menu's rows follow the list's: see `render_view`.
        let first = self.menu_top();
        match self.menu.as_mut()?.handle(event, first) {
            MenuReply::Stay => None,
            MenuReply::Close => {
                self.menu = None;
                None
            }
            MenuReply::Run(action) => self.run_action(index, &action),
        }
    }

    /// The action `key` triggers here. A key with modifiers that is not
    /// bound as it is does what it does without them, unless it types.
    fn key_action(&self, key: Key) -> Option<&str> {
        let action = self.keymap.action(key).or_else(|| {
            let typing = matches!(key.code, KeyCode::Char(_));
            (!typing && key.modifiers != Modifiers::NONE)
                .then(|| self.keymap.action(Key::new(key.code)))
                .flatten()
        })?;
        (self.multi || !MARKING.contains(&action)).then_some(action)
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
            self.filter
                .query_mut()
                .push_str(&crate::components::pasted(text, " "));
            self.refilter();
            return None;
        }
        let key = event.key()?;
        // A click after a key is a first click again.
        self.clicked = None;
        if let Some(index) = self.focused() {
            let actions = self.actions_for(index);
            if self.keymap.is(key, "actions") && !actions.is_empty() {
                self.menu = Some(ActionMenu::new(actions, key));
                return None;
            }
            if let Some(action) = actions.iter().find(|action| action.key == Some(key)) {
                let action = action.clone();
                return self.run_action(index, &action);
            }
        }
        match self.key_action(key) {
            Some("pick") => return self.pick_focused(),
            Some("cancel") => return Some(Flow::Cancel),
            Some("up") => self.step(-1),
            Some("down") => self.step(1),
            Some("page-up") => self.step(-(self.shown() as isize)),
            Some("page-down") => self.step(self.shown() as isize),
            Some("first") => self.step(isize::MIN / 2),
            Some("last") => self.step(isize::MAX / 2),
            Some(mark @ ("mark" | "mark-up")) => {
                let down = mark == "mark";
                if let Some(index) = self.focused() {
                    self.list.toggle(index);
                    self.step(if down { 1 } else { -1 });
                }
            }
            Some("mark-all") => {
                let matches: Vec<usize> = self.filter.matches().iter().map(|(i, _)| *i).collect();
                let all = matches.iter().all(|&index| self.list.is_selected(index));
                for index in matches {
                    self.list.set_selected(index, !all);
                }
            }
            Some("clear") => {
                self.filter.query_mut().clear();
                self.refilter();
            }
            Some("delete") => {
                if self.filter.query_mut().pop().is_some() {
                    self.refilter();
                }
            }
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    self.filter.query_mut().push(c);
                    self.refilter();
                }
                _ => return Some(Flow::Ignored),
            },
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
        let left = self
            .divider
            .resolve(width, (width * 45 / 100).max(20).min(width));
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
                let border = layout == PreviewLayout::Right && self.divider.hit(column, left);
                if border && in_list {
                    self.divider.press(column, left);
                } else if in_list && (layout != PreviewLayout::Right || column < left) {
                    let position = self.list.offset() + row - top;
                    if position < self.filter.len() {
                        if self.clicked == Some(position) && position == self.list.cursor() {
                            return self.pick_focused();
                        }
                        self.list.set_cursor(position);
                        self.clicked = Some(position);
                    }
                }
            }
            MouseKind::Drag(Button::Left) if self.divider.dragging() => {
                self.divider.drag(column, width);
            }
            MouseKind::Up(_) => self.divider.release(),
            _ => {}
        }
        None
    }

    fn row(&self, position: usize, width: usize) -> Vec<Segment> {
        let (index, positions) = &self.filter.matches()[position];
        let item = &self.items[*index];
        let theme = &self.theme;
        let focused = position == self.list.cursor();
        let mut line = if focused {
            vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
        } else {
            vec![plain("  ")]
        };
        if self.multi {
            line.push(if self.list.is_selected(*index) {
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

    /// The list's rows at `width`: the matches shown, or "no matches", kept
    /// at a steady height.
    pub fn list_lines(&self, width: usize) -> Vec<Vec<Segment>> {
        let rows = self.rows();
        let mut lines: Vec<Vec<Segment>> = (self.list.offset()..self.filter.len())
            .take(rows)
            .map(|position| self.row(position, width))
            .collect();
        if self.filter.is_empty() {
            lines.push(vec![text("  no matches", &self.theme.hint)]);
        }
        // A steady height, so the footer does not jump as the list filters.
        lines.resize_with(rows, Vec::new);
        lines
    }

    /// The footer: counts and key hints.
    pub fn footer(&self, width: usize) -> Vec<Segment> {
        let mut hint = format!("{}/{}", self.filter.len(), self.items.len());
        if self.multi {
            let marked = self.list.selected_count();
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
        let query = self.filter.query();
        let column = crate::components::width(&header) + rich::cells::cell_len(query);
        header.push(plain(query.to_string()));
        let mut lines = vec![fit(header, width)];
        if let Some(heading) = &self.heading {
            let mut line = vec![plain(if self.multi { "    " } else { "  " })];
            line.extend(heading.iter().cloned());
            lines.push(fit(line, width));
        }
        let footer = |this: &Self| match &this.menu {
            Some(menu) => menu.render(&this.theme, width),
            None => vec![this.footer(width)],
        };
        let preview = self
            .focused()
            .and_then(|index| self.items[index].preview.as_ref());
        match (preview, self.layout(width)) {
            (Some(preview), (PreviewLayout::Right, left)) => {
                let right = width.saturating_sub(left + 3).max(1);
                let list = self.list_lines(left);
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
                lines.extend(self.list_lines(width));
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
                lines.extend(self.list_lines(width));
                lines.extend(footer(self));
            }
        }
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    /// The keys that apply now: no marking for a single choice, and the
    /// action menu's while it is open.
    pub fn visible_keymap(&self) -> Keymap {
        if self.menu.is_some() {
            return Keymap::new("menu")
                .bind("up", keys("up"), "move up")
                .bind("down", keys("down tab"), "move down")
                .bind("run", keys("enter"), "run")
                .bind("close", keys("escape"), "close");
        }
        let mut keymap = Keymap::new("select");
        for binding in self.keymap.bindings() {
            if self.multi || !MARKING.contains(&binding.action.as_str()) {
                keymap.add(binding);
            }
        }
        keymap
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
            self.list.selected()
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
#[cfg(test)]
fn clamp_split(column: usize, width: usize) -> usize {
    Divider::new(3).min(MIN_PANE).clamp(column, width)
}

impl<T: Clone> Component for Select<T> {
    type Output = T;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<T> {
        match self.event(event, context) {
            Some(Flow::Done(indices)) => Flow::Done(self.items[indices[0]].value.clone()),
            Some(Flow::Cancel) => Flow::Cancel,
            Some(Flow::Ignored) => Flow::Ignored,
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

    fn keymap(&self) -> Keymap {
        self.visible_keymap()
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
            if index < self.0.items.len() {
                self.0.list.set_selected(index, true);
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
            Some(Flow::Ignored) => Flow::Ignored,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.0.render_view(context)
    }

    fn keymap(&self) -> Keymap {
        self.0.visible_keymap()
    }

    fn tick(&self) -> Option<Duration> {
        self.0.repaint
    }

    fn mouse(&self) -> bool {
        self.0.mouse
    }

    fn default_value(&self) -> Option<Vec<T>> {
        Some(
            self.0
                .list
                .selected()
                .into_iter()
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

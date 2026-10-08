//! Overlays: a command palette, a help overlay, a shortcut sheet and an
//! action menu, all read from the keymap, and a wrapper that puts them over
//! any component (0.0.14 workstream 2: #453, #473, #474, #475).
//!
//! Each overlay is a [`Component`] of its own, so it runs alone, sits in a
//! [`Layer`] of your own [`Layers`], or goes anywhere else a component
//! goes:
//!
//! - [`Palette`]: [`Command`]s by category, with their shortcuts as the
//!   keymap has them now, the ones for contexts that are not active left
//!   out, found by fuzzy search. It answers the command picked.
//! - [`Help`]: every binding, grouped by context and searchable as you
//!   type.
//! - [`Shortcuts`]: every binding at a glance, in columns; any key closes
//!   it.
//! - [`Menu`]: the actions for one [`ActionTarget`], an item's or a
//!   region's ([`TargetKind::Region`]), as Ctrl+K opens them.
//!
//! [`Overlays`] wires them into any host: it wraps a component, opens the
//! palette on Ctrl+O, the help on F1 and the shortcuts on `?` or F2 when
//! the component does not use the key, reads what they list from the
//! component's [`keymap`](Component::keymap) at that moment, and runs what
//! is picked: a binding's command by sending the component its key, a
//! command of yours by calling its handler. It also carries a region's
//! actions on Ctrl+K, and a [`StatusBar`] and [`Breadcrumbs`] round the
//! component.
//!
//! ```
//! use rich_interact::overlay::Overlays;
//! use rich_interact::{headless, Outcome, Select};
//!
//! // Ctrl+O, then "down" finds "move down"; Enter runs it; Enter picks.
//! let app = Overlays::new(Select::new("Fruit", ["apple", "banana"]));
//! let script = headless::Script::new().keys("ctrl+o").text("move down").keys("enter enter");
//! let (outcome, record) = headless::run(app, script, 60, 16);
//! assert_eq!(outcome.unwrap(), Outcome::Done("banana"));
//! assert!(record.frames.iter().any(|frame| frame.contains("Commands")));
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use rich::cells::cell_len;
use rich::{Segment, Style};

use crate::chrome::{key_hint, keys_hint, Breadcrumbs, StatusBar};
use crate::component::{Component, Context, Flow, View};
use crate::compose::{ComponentExt, Layer, LayerHandle, Layers, Rect};
use crate::event::{Event, Key, KeyCode, MouseKind};
use crate::item::{Action, ActionTarget, Actions, TargetKind};
use crate::keymap::{keys, Binding, Keymap};
use crate::kit::{self, ActionMenu, FilterState, ListState, MenuReply, ScrollState, Theme};
use crate::policy::{LineIo, NotInteractive};

/// The action that opens the [`Palette`] in [`Overlays`] (Ctrl+O).
pub const PALETTE: &str = "palette";
/// The action that opens the [`Help`] overlay (F1).
pub const HELP: &str = "help";
/// The action that opens the [`Shortcuts`] sheet (`?`, F2).
pub const SHORTCUTS: &str = "shortcuts";
/// The action that opens a region's [`Menu`] (Ctrl+K).
pub const ACTIONS: &str = "actions";

fn style(definition: &str) -> Style {
    Style::parse(definition).expect("a built-in style")
}

/// A typed key: a character without Ctrl or Alt.
fn typed(key: Key) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => Some(c),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Commands and the palette.

/// Something the [`Palette`] offers: a binding of the keymap
/// ([`from_binding`](Self::from_binding)) or a command of yours.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// What identifies it: `context.action` for a binding.
    pub id: String,
    /// What it does, for people.
    pub label: String,
    /// The group it is listed under: a binding's context.
    pub category: String,
    /// The keys that run it, shown as its shortcut; the first runs it
    /// under [`Overlays`] when it has no handler.
    pub keys: Vec<Key>,
    /// The context it belongs to: listed only while that context is
    /// active ([`Palette::contexts`]). `None`: always listed.
    pub context: Option<String>,
}

impl Command {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Command {
        Command {
            id: id.into(),
            label: label.into(),
            category: String::new(),
            keys: Vec::new(),
            context: None,
        }
    }

    pub fn category(mut self, category: impl Into<String>) -> Self {
        self.category = category.into();
        self
    }

    /// Its shortcut, when the keymap does not say (see
    /// [`Palette::hints`]).
    pub fn keys(mut self, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keys = keys.into_iter().collect();
        self
    }

    /// List it only while `context` is active.
    pub fn context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }

    /// The command a binding makes: its description, under its context,
    /// with its keys.
    pub fn from_binding(binding: &Binding) -> Command {
        Command {
            id: binding.id(),
            label: binding.description.clone(),
            category: binding.context.clone(),
            keys: binding.keys.clone(),
            context: Some(binding.context.clone()),
        }
    }

    /// The keys as a hint shows them: `↑/ctrl+p`.
    pub fn keys_label(&self) -> String {
        keys_hint(&self.keys)
    }

    /// What fuzzy search looks in: the category, then the label.
    fn search_text(&self) -> String {
        format!("{} {}", self.category, self.label)
    }
}

/// The palette's keys, in context `palette`. Every other character types.
pub fn palette_keymap() -> Keymap {
    Keymap::new("palette")
        .bind("up", keys("up ctrl+p"), "move up")
        .bind("down", keys("down ctrl+n"), "move down")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("run", keys("enter"), "run the command")
        .bind("close", keys("escape"), "close")
        .bind("delete", keys("backspace"), "delete a character")
        .bind("clear", keys("ctrl+u"), "clear the search")
}

/// A command palette (#453): [`Command`]s in categories, each with its
/// shortcut, found by fuzzy search over category and label. Commands of a
/// context that is not active are left out ([`contexts`](Self::contexts)).
/// Enter answers the command focused; Escape cancels.
///
/// [`Overlays`] builds one from the host's keymap each time it opens, so
/// what it lists is what the host can do then.
pub struct Palette {
    commands: Vec<Command>,
    /// An icon before a category's name, by category.
    icons: Vec<(String, Vec<Segment>)>,
    /// Indices into `commands` of those listed: in an active context.
    listed: Vec<usize>,
    active: Option<Vec<String>>,
    filter: FilterState,
    list: ListState,
    prompt: String,
    theme: Theme,
    keymap: Keymap,
    page: Cell<usize>,
}

impl Palette {
    pub fn new(commands: impl IntoIterator<Item = Command>) -> Palette {
        let mut palette = Palette {
            commands: commands.into_iter().collect(),
            icons: Vec::new(),
            listed: Vec::new(),
            active: None,
            filter: FilterState::default(),
            list: ListState::new(),
            prompt: "Command".to_string(),
            theme: Theme::default(),
            keymap: palette_keymap(),
            page: Cell::new(10),
        };
        palette.relist();
        palette
    }

    /// A command for every binding of `keymap` that has keys.
    pub fn from_keymap(keymap: &Keymap) -> Palette {
        Palette::new(
            keymap
                .bindings()
                .iter()
                .filter(|binding| !binding.keys.is_empty())
                .map(Command::from_binding),
        )
    }

    /// Add a command.
    /// Show `icon` (one line of text: a micro asset's placeholder, say)
    /// before `category`'s name on each of its commands.
    pub fn category_icon(mut self, category: impl Into<String>, icon: &rich::Text) -> Self {
        let category = category.into();
        self.icons.retain(|(known, _)| *known != category);
        self.icons.push((category, kit::icon(icon)));
        self
    }

    fn icon(&self, category: &str) -> Option<&[Segment]> {
        self.icons
            .iter()
            .find(|(known, _)| known == category)
            .map(|(_, icon)| icon.as_slice())
    }

    /// A category's width: its icon and a space, then its name.
    fn category_cells(&self, category: &str) -> usize {
        cell_len(category) + self.icon(category).map_or(0, |icon| kit::width(icon) + 1)
    }

    pub fn command(mut self, command: Command) -> Self {
        self.commands.push(command);
        self.relist();
        self
    }

    /// Add commands.
    pub fn commands(mut self, commands: impl IntoIterator<Item = Command>) -> Self {
        self.commands.extend(commands);
        self.relist();
        self
    }

    /// Only these contexts are active: a command in another is left out.
    pub fn contexts<I, S>(mut self, contexts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.active = Some(contexts.into_iter().map(Into::into).collect());
        self.relist();
        self
    }

    /// Read each command's shortcut from `keymap`: a command whose id is a
    /// binding's `context.action` shows the keys that do it now, rebinding
    /// included.
    pub fn hints(mut self, keymap: &Keymap) -> Self {
        let bindings = keymap.bindings();
        for command in &mut self.commands {
            if let Some(binding) = bindings.iter().find(|b| b.id() == command.id) {
                command.keys = binding.keys.clone();
            }
        }
        self
    }

    /// The prompt before the search (default `Command`).
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Search for `query` from the start.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.filter.set_query(query);
        self.reset();
        self
    }

    /// Make `keys` do `action` (see [`palette_keymap`]) on this palette.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// The commands listed now, best match first.
    pub fn matches(&self) -> Vec<&Command> {
        self.filter
            .matches()
            .iter()
            .map(|(index, _)| &self.commands[self.listed[*index]])
            .collect()
    }

    /// The command focused, if any matches.
    pub fn focused(&self) -> Option<&Command> {
        self.filter
            .index(self.list.cursor())
            .map(|index| &self.commands[self.listed[index]])
    }

    /// The search.
    pub fn filter(&self) -> &FilterState {
        &self.filter
    }

    fn relist(&mut self) {
        let active = self.active.as_ref();
        self.listed = (0..self.commands.len())
            .filter(|&index| match (&self.commands[index].context, active) {
                (Some(context), Some(active)) => active.iter().any(|a| a == context),
                _ => true,
            })
            .collect();
        let candidates: Vec<String> = self
            .listed
            .iter()
            .map(|&index| self.commands[index].search_text())
            .collect();
        self.filter.set_candidates(candidates);
        self.reset();
    }

    fn reset(&mut self) {
        self.list.set_len(self.filter.len());
        self.list.reset(0, self.page.get());
    }

    fn row(&self, position: usize, category_width: usize, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        let (index, positions) = &self.filter.matches()[position];
        let command = &self.commands[self.listed[*index]];
        let focused = position == self.list.cursor();
        let mut line = if focused {
            vec![kit::text(
                format!("{} ", theme.pointer),
                &theme.pointer_style,
            )]
        } else {
            vec![kit::plain("  ")]
        };
        // Positions count characters of `category label`.
        let split = command.category.chars().count() + 1;
        let (in_category, in_label): (Vec<usize>, Vec<usize>) =
            positions.iter().partition(|&&p| p < split);
        let in_label: Vec<usize> = in_label.iter().map(|p| p - split).collect();
        let mut category = match self.icon(&command.category) {
            Some(icon) => {
                let mut cells = icon.to_vec();
                cells.push(kit::plain(" "));
                cells
            }
            None => Vec::new(),
        };
        category.extend(kit::highlight(
            &command.category,
            &in_category,
            Some(&theme.hint),
            &theme.matched,
        ));
        category.push(kit::plain(" "));
        line.extend(kit::pad(category, category_width + 1));
        let base = focused.then_some(&theme.focused);
        line.extend(kit::highlight(
            &command.label,
            &in_label,
            base,
            &theme.matched,
        ));
        let shortcut = command.keys_label();
        if !shortcut.is_empty() {
            let used = kit::width(&line);
            let wanted = cell_len(&shortcut) + 1;
            if used + wanted < width {
                line = kit::pad(line, width - wanted);
                line.push(kit::text(format!("{shortcut} "), &theme.hint));
            }
        }
        kit::fit(line, width)
    }

    fn move_by(&mut self, delta: isize) {
        self.list.step(delta, self.page.get());
    }
}

impl Component for Palette {
    type Output = Command;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Command> {
        let page = self.page.get();
        match event {
            Event::Paste(text) => {
                self.filter.push_str(&kit::pasted(text, " "));
                self.reset();
                return Flow::Continue;
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseKind::ScrollUp => self.move_by(-3),
                    MouseKind::ScrollDown => self.move_by(3),
                    _ if mouse.is_click() => {
                        let row = mouse.row as usize;
                        let position = self.list.offset() + row.wrapping_sub(1);
                        if row >= 1 && row <= page && position < self.filter.len() {
                            self.list.select_row(position, page);
                            if let Some(command) = self.focused() {
                                return Flow::Done(command.clone());
                            }
                        }
                    }
                    _ => {}
                }
                return Flow::Continue;
            }
            _ => {}
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        match self.keymap.action(key) {
            Some("up") => self.move_by(-1),
            Some("down") => self.move_by(1),
            Some("page-up") => self.move_by(-(page as isize)),
            Some("page-down") => self.move_by(page as isize),
            Some("run") => {
                return match self.focused() {
                    Some(command) => Flow::Done(command.clone()),
                    None => Flow::Continue,
                }
            }
            Some("close") => return Flow::Cancel,
            Some("delete") => {
                if self.filter.pop() {
                    self.reset();
                }
            }
            Some("clear") => {
                self.filter.clear();
                self.reset();
            }
            _ => match typed(key) {
                Some(c) => {
                    self.filter.push(c);
                    self.reset();
                }
                None => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        // The search, the commands, and a line of hints.
        let page = context.height.saturating_sub(2).max(1);
        self.page.set(page);
        let mut header = kit::question(theme, &self.prompt);
        let column = kit::width(&header) + cell_len(self.filter.query());
        header.push(kit::plain(self.filter.query().to_string()));
        let mut lines = vec![kit::fit(header, width)];
        let category_width = self
            .listed
            .iter()
            .map(|&index| self.category_cells(&self.commands[index].category))
            .max()
            .unwrap_or(0)
            .min(width / 3);
        for position in self.list.visible(page) {
            lines.push(self.row(position, category_width, width));
        }
        if self.filter.is_empty() {
            lines.push(vec![kit::text("  no matching commands", &theme.hint)]);
        }
        lines.resize_with(page + 1, Vec::new);
        let hint = format!(
            "  {}/{} · ↑↓ move · enter run · esc close",
            self.filter.len(),
            self.listed.len()
        );
        lines.push(kit::fit(vec![kit::text(hint, &theme.hint)], width));
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }
}

// ---------------------------------------------------------------------------
// Help.

/// A searchable help overlay (#473): every binding of a keymap, grouped by
/// context, with its keys and what it does. Typing searches keys,
/// descriptions and contexts; the arrows and PageUp/PageDown scroll;
/// Escape clears the search, then closes; Enter closes.
pub struct Help {
    bindings: Vec<Binding>,
    filter: FilterState,
    scroll: ScrollState,
    prompt: String,
    theme: Theme,
    page: Cell<usize>,
}

impl Help {
    /// Help for every binding of `keymap` that has keys.
    pub fn new(keymap: &Keymap) -> Help {
        Help::from_bindings(keymap.bindings())
    }

    pub fn from_bindings(bindings: impl IntoIterator<Item = Binding>) -> Help {
        let bindings: Vec<Binding> = bindings
            .into_iter()
            .filter(|binding| !binding.keys.is_empty())
            .collect();
        let candidates: Vec<String> = bindings
            .iter()
            .map(|b| {
                let hint = keys_hint(&b.keys);
                format!("{hint} {} {} {}", b.keys_label(), b.description, b.context)
            })
            .collect();
        let mut help = Help {
            bindings,
            filter: FilterState::new(candidates),
            scroll: ScrollState::new(0),
            prompt: "Search keys".to_string(),
            theme: Theme::default(),
            page: Cell::new(10),
        };
        help.scroll.set_len(help.rows(false).len());
        help
    }

    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Search for `query` from the start.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.filter.set_query(query);
        self.refilter();
        self
    }

    /// The bindings that match the search, in keymap order.
    pub fn matches(&self) -> Vec<&Binding> {
        let mut indices: Vec<usize> = self.filter.matches().iter().map(|(i, _)| *i).collect();
        indices.sort_unstable();
        indices.iter().map(|&index| &self.bindings[index]).collect()
    }

    fn refilter(&mut self) {
        self.scroll = ScrollState::new(self.rows(false).len());
    }

    /// The body's rows: a heading per context, then its bindings.
    fn rows(&self, styled: bool) -> Vec<Vec<Segment>> {
        let theme = &self.theme;
        let matches = self.matches();
        let key_width = matches
            .iter()
            .map(|b| cell_len(&keys_hint(&b.keys)))
            .max()
            .unwrap_or(0)
            .min(24);
        let mut rows = Vec::new();
        let mut contexts: Vec<&str> = Vec::new();
        for binding in &matches {
            if !contexts.contains(&binding.context.as_str()) {
                contexts.push(&binding.context);
            }
        }
        for context in contexts {
            rows.push(vec![kit::text(context.to_string(), &style("bold"))]);
            for binding in matches.iter().filter(|b| b.context == context) {
                if !styled {
                    rows.push(Vec::new());
                    continue;
                }
                let keys = kit::pad(
                    vec![kit::text(keys_hint(&binding.keys), &theme.prompt)],
                    key_width,
                );
                let mut line = vec![kit::plain("  ")];
                line.extend(keys);
                line.push(kit::plain("  "));
                line.push(kit::plain(binding.description.clone()));
                rows.push(line);
            }
        }
        rows
    }
}

impl Component for Help {
    type Output = ();

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<()> {
        let page = self.page.get();
        match event {
            Event::Paste(text) => {
                self.filter.push_str(&kit::pasted(text, " "));
                self.refilter();
                return Flow::Continue;
            }
            Event::Mouse(_) => {
                self.scroll.handle(event, page);
                return Flow::Continue;
            }
            _ => {}
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        match key.code {
            KeyCode::Escape if self.filter.is_filtering() => {
                self.filter.clear();
                self.refilter();
            }
            KeyCode::Escape => return Flow::Cancel,
            KeyCode::Enter => return Flow::Done(()),
            KeyCode::Backspace => {
                if self.filter.pop() {
                    self.refilter();
                }
            }
            KeyCode::Char('u') if key.modifiers.ctrl => {
                self.filter.clear();
                self.refilter();
            }
            KeyCode::Up => {
                self.scroll.scroll(-1, page);
            }
            KeyCode::Down => {
                self.scroll.scroll(1, page);
            }
            KeyCode::PageUp => {
                self.scroll.scroll(-(page as isize), page);
            }
            KeyCode::PageDown => {
                self.scroll.scroll(page as isize, page);
            }
            KeyCode::Home => {
                self.scroll.scroll_to(0, page);
            }
            KeyCode::End => {
                self.scroll.scroll_to(usize::MAX, page);
            }
            _ => match typed(key) {
                Some(c) => {
                    self.filter.push(c);
                    self.refilter();
                }
                None => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let page = context.height.saturating_sub(2).max(1);
        self.page.set(page);
        let mut header = kit::question(theme, &self.prompt);
        let column = kit::width(&header) + cell_len(self.filter.query());
        header.push(kit::plain(self.filter.query().to_string()));
        let mut lines = vec![kit::fit(header, width)];
        let rows = self.rows(true);
        let shown = self.scroll.visible(page);
        let more = shown.end < rows.len();
        lines.extend(rows[shown].iter().map(|row| kit::fit(row.clone(), width)));
        if rows.is_empty() {
            lines.push(vec![kit::text("  no matching keys", &theme.hint)]);
        }
        lines.resize_with(page + 1, Vec::new);
        let count = self.matches().len();
        let hint = format!(
            "  {count} {}{} · type to search · ↑↓ scroll · esc close",
            if count == 1 { "key" } else { "keys" },
            if more { " · more below" } else { "" }
        );
        lines.push(kit::fit(vec![kit::text(hint, &theme.hint)], width));
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        Keymap::new("help")
            .bind("scroll-up", keys("up"), "scroll up")
            .bind("scroll-down", keys("down"), "scroll down")
            .bind("page-up", keys("pageup"), "page up")
            .bind("page-down", keys("pagedown"), "page down")
            .bind("close", keys("escape enter"), "close")
            .bind("clear", keys("ctrl+u"), "clear the search")
    }
}

// ---------------------------------------------------------------------------
// Shortcuts.

/// A shortcut sheet (#475): every binding of a keymap at a glance, its
/// first key and what it does, in as many columns as fit. Any key or a
/// click closes it.
pub struct Shortcuts {
    bindings: Vec<Binding>,
    theme: Theme,
}

impl Shortcuts {
    pub fn new(keymap: &Keymap) -> Shortcuts {
        Shortcuts::from_bindings(keymap.bindings())
    }

    pub fn from_bindings(bindings: impl IntoIterator<Item = Binding>) -> Shortcuts {
        Shortcuts {
            bindings: bindings
                .into_iter()
                .filter(|binding| !binding.keys.is_empty())
                .collect(),
            theme: Theme::default(),
        }
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    fn widths(&self) -> (usize, usize) {
        let key = self
            .bindings
            .iter()
            .map(|b| cell_len(&key_hint(b.keys[0])))
            .max()
            .unwrap_or(0);
        let entry = self
            .bindings
            .iter()
            .map(|b| cell_len(&b.description))
            .max()
            .unwrap_or(0);
        (key, (key + 2 + entry).min(36))
    }

    /// How many columns fit in `width`, and the rows they take.
    fn grid(&self, width: usize) -> (usize, usize) {
        let (_, entry) = self.widths();
        let columns = ((width + 3) / (entry + 3)).min(self.bindings.len()).max(1);
        (columns, self.bindings.len().div_ceil(columns))
    }

    /// The size of a box that shows it all, border included, in at most
    /// `width` × `height` cells.
    pub fn size(&self, width: usize, height: usize) -> (usize, usize) {
        let inner = width.saturating_sub(4);
        let (columns, rows) = self.grid(inner);
        let (_, entry) = self.widths();
        let used = columns * (entry + 3) - 3;
        ((used + 4).min(width), (rows + 4).min(height))
    }
}

impl Component for Shortcuts {
    type Output = ();

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<()> {
        match event {
            Event::Key(_) => Flow::Done(()),
            Event::Mouse(mouse) if mouse.is_click() => Flow::Done(()),
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let theme = &self.theme;
        let width = context.width.saturating_sub(2);
        let (key_width, entry) = self.widths();
        let (columns, rows) = self.grid(width);
        let mut lines = Vec::with_capacity(rows + 2);
        for row in 0..rows {
            let mut line = vec![kit::plain(" ")];
            for column in 0..columns {
                let Some(binding) = self.bindings.get(column * rows + row) else {
                    break;
                };
                if column > 0 {
                    line.push(kit::plain("   "));
                }
                let mut cell = kit::pad(
                    vec![kit::text(key_hint(binding.keys[0]), &theme.prompt)],
                    key_width,
                );
                cell.push(kit::plain("  "));
                cell.push(kit::text(binding.description.clone(), &Style::default()));
                line.extend(kit::pad(cell, entry));
            }
            lines.push(kit::fit(line, context.width));
        }
        lines.push(Vec::new());
        lines.push(kit::fit(
            vec![kit::text(" any key closes", &theme.hint)],
            context.width,
        ));
        View::new(lines)
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        Keymap::new("shortcuts").bind("close", keys("escape"), "close (any key)")
    }
}

// ---------------------------------------------------------------------------
// Menu.

/// The actions for one [`ActionTarget`], as a component (#474): an
/// [`ActionMenu`] to show in a modal. The arrows move, Enter or an
/// action's own key runs one (the menu answers it), Escape or the key that
/// opened it cancels. [`Select`](crate::Select) opens one over the focused
/// item; [`Overlays`] opens one for a region.
pub struct Menu {
    menu: ActionMenu,
    target: ActionTarget,
    theme: Theme,
}

impl Menu {
    pub fn new(actions: Vec<Action>, target: ActionTarget) -> Menu {
        Menu {
            menu: ActionMenu::new(actions, Key::ctrl('k')),
            target,
            theme: Theme::default(),
        }
    }

    /// The key that opened it, which closes it again (default Ctrl+K).
    pub fn key(mut self, key: Key) -> Self {
        self.menu.key = key;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    pub fn target(&self) -> &ActionTarget {
        &self.target
    }

    pub fn menu(&self) -> &ActionMenu {
        &self.menu
    }

    /// The size of a box that shows it all, border included.
    pub fn size(&self) -> (usize, usize) {
        let (width, rows) = self.menu.size();
        (width + 2, rows + 2)
    }
}

impl Component for Menu {
    type Output = Action;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Action> {
        if !matches!(event, Event::Key(_) | Event::Mouse(_)) {
            return Flow::Ignored;
        }
        match self.menu.handle(event, 0) {
            MenuReply::Stay => Flow::Continue,
            MenuReply::Close => Flow::Cancel,
            MenuReply::Run(action) => Flow::Done(action),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(self.menu.render(&self.theme, context.width))
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        Keymap::new("menu")
            .bind("up", keys("up"), "move up")
            .bind("down", keys("down tab"), "move down")
            .bind("run", keys("enter"), "run")
            .bind("close", keys("escape"), "close")
    }
}

// ---------------------------------------------------------------------------
// Overlays.

/// The keys [`Overlays`] opens its overlays with, in context `overlays`.
pub fn overlays_keymap() -> Keymap {
    Keymap::new("overlays")
        .bind(PALETTE, keys("ctrl+o"), "commands")
        .bind(HELP, keys("f1"), "help")
        .bind(SHORTCUTS, keys("? f2"), "shortcuts")
}

/// What an overlay picked, for the host to run.
enum Picked {
    Command(Command),
    Action(Action),
}

type Handler<'a, M> = Box<dyn FnMut() -> Flow<M> + 'a>;
type ActionHandler<'a, M> = Box<dyn FnMut(&Action, &ActionTarget) -> Flow<M> + 'a>;

/// Overlays and chrome for any component: wrap it, and it gets a command
/// palette (Ctrl+O), a help overlay (F1) and a shortcut sheet (`?` or F2),
/// each read from its keymap when opened, a region's action menu (Ctrl+K)
/// when given [`actions`](Self::actions), and a [`StatusBar`] under it and
/// [`Breadcrumbs`] over it when given them.
///
/// The keys reach the component first, and open an overlay only when it
/// does not use them ([`Flow::Ignored`]), so `?` still types in a search.
/// They are rebound with [`rebind`](Self::rebind) or, for the whole
/// process, under context `overlays` ([`keymap::install`](crate::keymap::install)).
///
/// A command picked in the palette runs: one of yours through its handler,
/// a binding by sending the component the binding's first key, exactly as
/// if it had been pressed. The overlays are modal [`Layer`]s of a
/// [`Layers`] host round the component ([`layers`](Self::layers)), so
/// layers of your own open there too.
pub struct Overlays<'a, M> {
    layers: Layers<'a, M>,
    keymap: Keymap,
    commands: Vec<Command>,
    category_icons: Vec<(String, rich::Text)>,
    handlers: Vec<(String, Handler<'a, M>)>,
    picked: Rc<RefCell<Option<Picked>>>,
    region: Option<ActionTarget>,
    actions: Actions,
    on_action: Option<ActionHandler<'a, M>>,
    ran: Option<(String, ActionTarget)>,
    status: Option<StatusBar<M>>,
    crumbs: Option<Breadcrumbs<'a, M>>,
    theme: Theme,
}

impl<'a, M: 'a> Overlays<'a, M> {
    pub fn new(component: impl Component<Output = M> + 'a) -> Overlays<'a, M> {
        Overlays {
            layers: Layers::new(component),
            keymap: overlays_keymap(),
            commands: Vec::new(),
            category_icons: Vec::new(),
            handlers: Vec::new(),
            picked: Rc::new(RefCell::new(None)),
            region: None,
            actions: Actions::new(),
            on_action: None,
            ran: None,
            status: None,
            crumbs: None,
            theme: Theme::default(),
        }
    }

    /// Make `keys` open overlay `action` ([`PALETTE`], [`HELP`],
    /// [`SHORTCUTS`], [`ACTIONS`]); no keys turns it off.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// Show `icon` before `category` in the palette: see
    /// [`Palette::category_icon`].
    pub fn category_icon(mut self, category: impl Into<String>, icon: rich::Text) -> Self {
        let category = category.into();
        self.category_icons.retain(|(known, _)| *known != category);
        self.category_icons.push((category, icon));
        self
    }

    /// Offer `command` in the palette; picking it runs `handler`.
    pub fn command(mut self, command: Command, handler: impl FnMut() -> Flow<M> + 'a) -> Self {
        self.handlers.retain(|(id, _)| *id != command.id);
        self.handlers.push((command.id.clone(), Box::new(handler)));
        self.commands.retain(|known| known.id != command.id);
        self.commands.push(command);
        self
    }

    /// Name the component as a region, `label` shown and `value` given to
    /// actions ([`TargetKind::Region`]), for [`actions`](Self::actions).
    pub fn region(mut self, label: impl Into<String>, value: impl Into<String>) -> Self {
        self.region = Some(ActionTarget {
            kind: TargetKind::Region,
            index: 0,
            label: label.into(),
            value: value.into(),
        });
        self
    }

    /// Actions on the region (#474), opened in a modal [`Menu`] on Ctrl+K
    /// when the component does not use the key: when the item focused
    /// has no actions of its own, or there is no item. A region with no
    /// name ([`region`](Self::region)) is called `region`.
    pub fn actions(mut self, actions: Actions) -> Self {
        self.actions = actions;
        if !self.actions.is_empty() {
            self.keymap = std::mem::take(&mut self.keymap).bind(
                ACTIONS,
                keys("ctrl+k"),
                "actions for this region",
            );
        }
        self
    }

    /// What running a region's action does (default: record it, see
    /// [`action`](Self::action), and carry on).
    pub fn on_action(
        mut self,
        handler: impl FnMut(&Action, &ActionTarget) -> Flow<M> + 'a,
    ) -> Self {
        self.on_action = Some(Box::new(handler));
        self
    }

    /// A status bar under the component; its [`Hints`](crate::chrome::StatusItem::Hints)
    /// read the keymap of what has the keys: the component, or the
    /// overlay open over it.
    pub fn status_bar(mut self, bar: StatusBar<M>) -> Self {
        self.status = Some(bar);
        self
    }

    /// Breadcrumbs over the component.
    pub fn breadcrumbs(mut self, crumbs: Breadcrumbs<'a, M>) -> Self {
        self.crumbs = Some(crumbs);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// The overlays' own bindings.
    pub fn own_keymap(&self) -> &Keymap {
        &self.keymap
    }

    /// The layer host round the component.
    pub fn layers(&self) -> &Layers<'a, M> {
        &self.layers
    }

    pub fn layers_mut(&mut self) -> &mut Layers<'a, M> {
        &mut self.layers
    }

    /// A handle that opens layers of your own over the component.
    pub fn layer_handle(&self) -> LayerHandle<'a, M> {
        self.layers.handle()
    }

    /// The component wrapped.
    pub fn component(&self) -> &(dyn Component<Output = M> + 'a) {
        self.layers.base()
    }

    pub fn status(&self) -> Option<&StatusBar<M>> {
        self.status.as_ref()
    }

    /// The id of the region action run last, if one was.
    pub fn action(&self) -> Option<&str> {
        self.ran.as_ref().map(|(id, _)| id.as_str())
    }

    /// The region's target, as actions see it.
    pub fn target(&self) -> ActionTarget {
        self.region.clone().unwrap_or(ActionTarget {
            kind: TargetKind::Region,
            index: 0,
            label: "region".to_string(),
            value: "region".to_string(),
        })
    }

    /// What the overlays list: the component's bindings, then the
    /// overlays' own.
    pub fn host_keymap(&self) -> Keymap {
        let mut keymap = self.layers.base().keymap();
        keymap.extend(self.keymap.clone());
        keymap
    }

    /// The palette as it would open now: the host's bindings and your
    /// commands, in the contexts active now, with shortcuts from the
    /// keymap.
    pub fn palette(&self) -> Palette {
        let keymap = self.host_keymap();
        let mut active: Vec<String> = Vec::new();
        for binding in keymap.bindings() {
            if !active.contains(&binding.context) {
                active.push(binding.context);
            }
        }
        let bindings = keymap
            .bindings()
            .into_iter()
            .filter(|b| !b.keys.is_empty() && b.id() != format!("overlays.{PALETTE}"));
        // Your commands first: they are what the host is for.
        let mut palette = Palette::new(self.commands.iter().cloned())
            .commands(bindings.map(|b| Command::from_binding(&b)))
            .contexts(active)
            .hints(&keymap)
            .theme(self.theme.clone());
        for (category, icon) in &self.category_icons {
            palette = palette.category_icon(category.clone(), icon);
        }
        palette
    }

    /// The area the component and its layers take: all but the chrome.
    fn body(&self, context: &Context<'_>) -> Rect {
        let top = usize::from(self.crumbs.is_some());
        let chrome = top + usize::from(self.status.is_some());
        Rect::new(
            0,
            top,
            context.width,
            context.height.saturating_sub(chrome).max(1),
        )
    }

    fn open(&mut self, action: &str, body: &Context<'_>) -> bool {
        let (width, height) = (body.width, body.height);
        let slot = Rc::clone(&self.picked);
        let layer = match action {
            PALETTE => {
                let palette = self.palette().map(move |command| {
                    *slot.borrow_mut() = Some(Picked::Command(command));
                    Flow::Continue
                });
                Layer::modal(palette)
                    .title("Commands")
                    .size(width.min(72), height.min(18))
            }
            HELP => {
                let help = Help::new(&self.host_keymap())
                    .theme(self.theme.clone())
                    .map(|()| Flow::Continue);
                Layer::modal(help)
                    .title("Keys")
                    .size(width.min(72), height.min(20))
            }
            SHORTCUTS => {
                let sheet = Shortcuts::new(&self.host_keymap()).theme(self.theme.clone());
                let (w, h) = sheet.size(width, height);
                Layer::modal(sheet.map(|()| Flow::Continue))
                    .title("Shortcuts")
                    .size(w, h)
            }
            ACTIONS => {
                let target = self.target();
                let actions = self.actions.for_target(&target, &[]);
                if actions.is_empty() {
                    return false;
                }
                let menu = Menu::new(actions, target.clone())
                    .key(self.keymap.key(ACTIONS).unwrap_or(Key::ctrl('k')))
                    .theme(self.theme.clone());
                let (w, h) = menu.size();
                let menu = menu.map(move |action| {
                    *slot.borrow_mut() = Some(Picked::Action(action));
                    Flow::Continue
                });
                Layer::modal(menu)
                    .title(format!("Actions · {}", target.label))
                    .size(w.min(width), h.min(height))
            }
            _ => return false,
        };
        self.layers.open(layer);
        true
    }

    /// Run what an overlay picked.
    fn run(&mut self, picked: Picked, context: &Context<'_>) -> Flow<M> {
        match picked {
            Picked::Command(command) => {
                if let Some((_, handler)) =
                    self.handlers.iter_mut().find(|(id, _)| *id == command.id)
                {
                    return handler();
                }
                match command.keys.first() {
                    Some(key) => match Component::handle(self, &Event::Key(*key), context) {
                        Flow::Ignored => Flow::Continue,
                        flow => flow,
                    },
                    None => Flow::Continue,
                }
            }
            Picked::Action(action) => {
                let target = self.target();
                self.ran = Some((action.id.clone(), target.clone()));
                match &mut self.on_action {
                    Some(handler) => handler(&action, &target),
                    None => Flow::Continue,
                }
            }
        }
    }

    /// The overlay `key` opens, if any: `?` arrives with Shift from some
    /// terminals, and counts without it.
    fn overlay_for(&self, key: Key) -> Option<String> {
        let action = self.keymap.action(key).or_else(|| match key.code {
            KeyCode::Char(c) if key.modifiers.shift && !c.is_alphabetic() => {
                let mut plain = key;
                plain.modifiers.shift = false;
                self.keymap.action(plain)
            }
            _ => None,
        })?;
        Some(action.to_string())
    }

    fn status_keymap(&self) -> Keymap {
        self.keymap()
    }
}

impl<'a, M: 'a> Component for Overlays<'a, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let body = self.body(context);
        self.layers.start(&body.context(context))
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let body = self.body(context);
        let inner = body.context(context);
        let flow = match event {
            Event::Mouse(mouse) => {
                let row = mouse.row as usize;
                if let Some(crumbs) = self.crumbs.as_mut().filter(|_| row == 0) {
                    if !crumbs.mouse() {
                        return Flow::Ignored;
                    }
                    let line = Rect::new(0, 0, context.width, 1);
                    return crumbs.handle(event, &line.context(context));
                }
                if row < body.y || row >= body.y + body.height {
                    return Flow::Ignored;
                }
                Component::handle(&mut self.layers, &Event::Mouse(body.local(*mouse)), &inner)
            }
            Event::Tick => {
                let flow = if self.layers.tick().is_some() {
                    Component::handle(&mut self.layers, event, &inner)
                } else {
                    Flow::Ignored
                };
                if self.status.as_ref().is_some_and(|s| s.tick().is_some()) {
                    // Repainted with the spinner's next frame.
                    return match flow {
                        Flow::Ignored => Flow::Continue,
                        flow => flow,
                    };
                }
                flow
            }
            _ => Component::handle(&mut self.layers, event, &inner),
        };
        let picked = self.picked.borrow_mut().take();
        if let Some(picked) = picked {
            return match flow {
                Flow::Continue | Flow::Ignored => self.run(picked, context),
                flow => flow,
            };
        }
        if !matches!(flow, Flow::Ignored) || self.layers.depth() > 0 {
            return flow;
        }
        let Some(action) = event.key().and_then(|key| self.overlay_for(key)) else {
            return flow;
        };
        if self.open(&action, &inner) {
            Flow::Continue
        } else {
            flow
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let body = self.body(context);
        let mut lines = Vec::new();
        if let Some(crumbs) = &self.crumbs {
            let line = Rect::new(0, 0, context.width, 1);
            lines.extend(
                crumbs
                    .render(&line.context(context))
                    .lines
                    .into_iter()
                    .take(1),
            );
            lines.resize_with(1, Vec::new);
        }
        let view = self.layers.render(&body.context(context));
        let cursor = view.cursor.map(|(row, column)| (row + body.y, column));
        lines.extend(view.lines.into_iter().take(body.height));
        if let Some(status) = &self.status {
            let line = Rect::new(0, 0, context.width, 1);
            lines.push(status.line(&line.context(context), Some(&self.status_keymap())));
        }
        View { lines, cursor }
    }

    fn tick(&self) -> Option<Duration> {
        let status = self.status.as_ref().and_then(|status| status.tick());
        self.layers.tick().into_iter().chain(status).min()
    }

    fn mouse(&self) -> bool {
        self.layers.mouse() || self.crumbs.as_ref().is_some_and(|crumbs| crumbs.mouse())
    }

    fn keymap(&self) -> Keymap {
        let mut keymap = self.layers.keymap();
        if self.layers.depth() == 0 {
            keymap.extend(self.keymap.clone());
        }
        keymap
    }

    fn focusable(&self) -> bool {
        self.layers.focusable()
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        self.layers.focus_step(forward)
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        self.layers.focus_enter(forward)
    }

    fn default_value(&self) -> Option<M> {
        self.layers.default_value()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<M>, NotInteractive> {
        self.layers.prompt(io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keymap() -> Keymap {
        Keymap::new("demo")
            .bind("save", keys("ctrl+s"), "save the file")
            .bind("quit", keys("q"), "quit")
    }

    #[test]
    fn a_palette_filters_by_context_and_reads_hints_from_the_keymap() {
        let keymap = keymap().rebound("save", keys("ctrl+w"));
        let palette = Palette::from_keymap(&keymap)
            .command(Command::new("demo.save", "save").context("demo"))
            .command(Command::new("other", "elsewhere").context("elsewhere"))
            .command(Command::new("always", "always there"))
            .contexts(["demo"])
            .hints(&keymap);
        let labels: Vec<&str> = palette.matches().iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["save the file", "quit", "save", "always there"]);
        assert_eq!(palette.matches()[2].keys_label(), "ctrl+w");
        let palette = palette.query("qt");
        assert_eq!(palette.focused().unwrap().id, "demo.quit");
    }

    #[test]
    fn help_groups_and_searches_bindings() {
        let mut keymap = keymap();
        keymap.extend(Keymap::new("tabs").bind("next", keys("alt+right"), "next tab"));
        let help = Help::new(&keymap);
        assert_eq!(help.matches().len(), 3);
        let help = help.query("tab");
        let found: Vec<String> = help.matches().iter().map(|b| b.id()).collect();
        assert_eq!(found, ["tabs.next"]);
    }

    #[test]
    fn palette_categories_show_their_icons() {
        let palette = Palette::new([
            Command::new("a", "deploy").category("ship"),
            Command::new("b", "open").category("file"),
        ])
        .category_icon("ship", &rich::Text::new("^^"));
        let console = rich::Console::new();
        let context = Context {
            console: &console,
            width: 40,
            height: 6,
        };
        let view = palette.render(&context);
        let rows: Vec<String> = view
            .lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .collect();
        assert!(
            rows.iter().any(|r| r.contains("^^ ship deploy")),
            "{rows:?}"
        );
        // Categories line up: the other one is padded past the icon.
        assert!(rows.iter().any(|r| r.contains("file    open")), "{rows:?}");
    }

    #[test]
    fn a_shortcut_sheet_packs_columns() {
        let sheet = Shortcuts::new(&keymap());
        assert_eq!(sheet.grid(80), (2, 1));
        assert_eq!(sheet.grid(10), (1, 2));
    }
}

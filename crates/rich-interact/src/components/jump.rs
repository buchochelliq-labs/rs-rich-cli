//! A jump-to-anything navigator (#484): one fuzzy box over several
//! sources at once (files, headings, symbols, commands, recent places),
//! with the results grouped by source and a preview of the focused one.
//!
//! A [`JumpIndex`] is the searchable index: [`JumpSource`]s of
//! [`Item`]s, each with an id, a title and an optional prefix character.
//! [`JumpIndex::search`] ranks every source's items with the
//! [fuzzy](crate::fuzzy) matcher and returns [`JumpGroup`]s, so the index
//! works without the component too (a command palette of your own, a
//! test). [`Jump`] is the component over it: type to search, ↑/↓ move
//! across the groups, Alt+↑/↓ jump between groups, Tab narrows the search
//! to one source and Shift+Tab goes back, and Enter returns a
//! [`Jumped`]: the source's id and the item's value. A query that starts
//! with a source's prefix (`>` for commands, `@` for symbols, say)
//! searches only that source. Without a narrowed source, each group shows
//! its best few (its [`limit`](JumpSource::limit)) and says how many more
//! there are.
//!
//! Group titles are drawn in the console theme's `jump.group` style, and
//! the count of more results in `jump.more` (bold and dim unless set).
//!
//! With the mouse on, a click focuses a result and a second click picks
//! it, a click on a group's title narrows to it, and the wheel moves.
//!
//! Without a terminal it reads one line and answers with the best match
//! for it; asked for a default, it answers with the best match for the
//! query it started with, if any.
//!
//! ```
//! use rich_interact::components::{Jump, JumpSource};
//! use rich_interact::{headless, Item, Outcome};
//!
//! let jump = Jump::new("Jump")
//!     .source(JumpSource::new("files", "Files").items(["src/main.rs".to_string(), "README.md".into()]))
//!     .source(
//!         JumpSource::new("commands", "Commands")
//!             .prefix('>')
//!             .item(Item::new("cargo build".to_string(), "Build")),
//!     );
//! let (outcome, _) = headless::run(jump, headless::Script::new().text(">bu").keys("enter"), 80, 12);
//! let Outcome::Done(jumped) = outcome.unwrap() else { panic!() };
//! assert_eq!((jumped.source.as_str(), jumped.value.as_str()), ("commands", "cargo build"));
//! ```

use std::cell::Cell;

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, pad, pasted, plain, question, text, PreviewLayout, Theme};
use crate::event::{Button, Event, KeyCode, Modifiers, MouseKind};
use crate::fuzzy::rank;
use crate::item::Item;
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// One source of a [`JumpIndex`]: files, headings, commands.
#[derive(Clone, Debug)]
pub struct JumpSource<T> {
    /// What the caller matches on: `files`.
    pub id: String,
    /// What the group is titled: `Files`.
    pub title: String,
    /// A character that, typed first, searches only this source.
    pub prefix: Option<char>,
    /// The most results the group shows while the search is over every
    /// source (default 5); narrowed to this source, all show.
    pub limit: usize,
    pub items: Vec<Item<T>>,
}

impl<T> JumpSource<T> {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> JumpSource<T> {
        JumpSource {
            id: id.into(),
            title: title.into(),
            prefix: None,
            limit: 5,
            items: Vec::new(),
        }
    }

    /// Typed first, `prefix` searches only this source.
    #[must_use]
    pub fn prefix(mut self, prefix: char) -> Self {
        self.prefix = Some(prefix);
        self
    }

    /// Show at most `results` while every source is searched.
    #[must_use]
    pub fn limit(mut self, results: usize) -> Self {
        self.limit = results.max(1);
        self
    }

    #[must_use]
    pub fn item(mut self, item: impl Into<Item<T>>) -> Self {
        self.items.push(item.into());
        self
    }

    #[must_use]
    pub fn items<I, V>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<Item<T>>,
    {
        self.items.extend(items.into_iter().map(Into::into));
        self
    }
}

/// One result: an item of a source, how well it matched, and the label
/// characters to highlight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpHit {
    pub item: usize,
    pub score: i64,
    pub positions: Vec<usize>,
}

/// One source's results, best first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JumpGroup {
    /// The source's position in the index.
    pub source: usize,
    /// The results shown.
    pub hits: Vec<JumpHit>,
    /// How many more matched than are shown.
    pub more: usize,
}

/// A searchable index over several sources: what [`Jump`] searches.
#[derive(Clone, Debug)]
pub struct JumpIndex<T> {
    pub sources: Vec<JumpSource<T>>,
}

impl<T> Default for JumpIndex<T> {
    fn default() -> Self {
        JumpIndex {
            sources: Vec::new(),
        }
    }
}

impl<T> JumpIndex<T> {
    pub fn new() -> JumpIndex<T> {
        JumpIndex::default()
    }

    #[must_use]
    pub fn source(mut self, source: JumpSource<T>) -> Self {
        self.sources.push(source);
        self
    }

    /// The source a query names with its first character, and the rest of
    /// the query: `>bu` is `bu` in the source whose prefix is `>`.
    pub fn scope<'q>(&self, query: &'q str) -> (Option<usize>, &'q str) {
        let mut chars = query.chars();
        if let Some(first) = chars.next() {
            if let Some(at) = self.sources.iter().position(|s| s.prefix == Some(first)) {
                return (Some(at), chars.as_str().trim_start());
            }
        }
        (None, query)
    }

    /// Search for `query` in every source, or in `only` (as a typed
    /// prefix does too): the groups that match, in the sources' order, each
    /// cut to its limit unless one source is searched.
    pub fn search(&self, query: &str, only: Option<usize>) -> Vec<JumpGroup> {
        let (named, query) = self.scope(query);
        let only = named.or(only);
        let mut groups = Vec::new();
        for (at, source) in self.sources.iter().enumerate() {
            if only.is_some_and(|only| only != at) {
                continue;
            }
            let texts: Vec<String> = source.items.iter().map(Item::search_text).collect();
            let mut hits: Vec<JumpHit> = rank(query, texts.iter().map(String::as_str))
                .into_iter()
                .map(|(item, found)| {
                    let limit = source.items[item].label.chars().count();
                    JumpHit {
                        item,
                        score: found.score,
                        positions: found.positions.into_iter().filter(|p| *p < limit).collect(),
                    }
                })
                .collect();
            if hits.is_empty() {
                continue;
            }
            let shown = if only.is_some() {
                hits.len()
            } else {
                source.limit.min(hits.len())
            };
            let more = hits.len() - shown;
            hits.truncate(shown);
            groups.push(JumpGroup {
                source: at,
                hits,
                more,
            });
        }
        groups
    }

    /// The best match for `query` over every source: `(source, item)`.
    pub fn best(&self, query: &str) -> Option<(usize, usize)> {
        self.search(query, None)
            .iter()
            .flat_map(|group| group.hits.iter().map(move |hit| (group.source, hit)))
            .max_by(|a, b| a.1.score.cmp(&b.1.score).then(b.0.cmp(&a.0)))
            .map(|(source, hit)| (source, hit.item))
    }
}

/// What [`Jump`] returns: where to go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Jumped<T> {
    /// The source's id.
    pub source: String,
    /// The item's index in its source.
    pub index: usize,
    pub label: String,
    pub value: T,
}

/// The keys of a [`Jump`], in context `jump`.
pub fn jump_keymap() -> Keymap {
    Keymap::new("jump")
        .bind("pick", keys("enter"), "jump")
        .bind("cancel", keys("escape"), "cancel")
        .bind("up", keys("up ctrl+p"), "move up")
        .bind("down", keys("down ctrl+n"), "move down")
        .bind("page-up", keys("pageup"), "page up")
        .bind("page-down", keys("pagedown"), "page down")
        .bind("first", keys("home"), "first result")
        .bind("last", keys("end"), "last result")
        .bind("previous-group", keys("alt+up"), "previous group")
        .bind("next-group", keys("alt+down"), "next group")
        .bind("narrow", keys("tab"), "search the next source only")
        .bind(
            "widen",
            keys("shift+tab"),
            "search the previous source, or all",
        )
        .bind("clear", keys("ctrl+u"), "clear the query")
        .bind("delete", keys("backspace"), "delete a character")
}

/// A row of the result list.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Row {
    /// A group's title: its source, and how many more there are.
    Title(usize, usize),
    /// A result: source, item, positions to highlight.
    Hit(usize, usize, Vec<usize>),
}

/// One fuzzy box over several sources, the results grouped by source.
/// Returns a [`Jumped`].
#[must_use = "a component does nothing until it is run or mounted"]
pub struct Jump<T> {
    prompt: String,
    index: JumpIndex<T>,
    query: String,
    /// Tab's narrowing: one source searched.
    only: Option<usize>,
    rows: Vec<Row>,
    /// The cursor, a row that is a result.
    cursor: usize,
    offset: usize,
    height: usize,
    preview: PreviewLayout,
    preview_height: usize,
    theme: Theme,
    mouse: bool,
    keymap: Keymap,
    clicked: Option<usize>,
    answer: Option<Option<String>>,
    space: Cell<usize>,
}

impl<T> Jump<T> {
    pub fn new(prompt: impl Into<String>) -> Jump<T> {
        Jump {
            prompt: prompt.into(),
            index: JumpIndex::new(),
            query: String::new(),
            only: None,
            rows: Vec::new(),
            cursor: 0,
            offset: 0,
            height: 12,
            preview: PreviewLayout::Auto,
            preview_height: 8,
            theme: Theme::default(),
            mouse: false,
            keymap: jump_keymap(),
            clicked: None,
            answer: None,
            space: Cell::new(usize::MAX),
        }
    }

    /// Search `index`.
    pub fn index(mut self, index: JumpIndex<T>) -> Self {
        self.index = index;
        self.only = None;
        self.refresh();
        self
    }

    /// Add `source` to the index.
    pub fn source(mut self, source: JumpSource<T>) -> Self {
        self.index.sources.push(source);
        self.refresh();
        self
    }

    /// Start with `query` typed.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.query = query.into();
        self.refresh();
        self
    }

    /// Show at most `rows` rows of results, titles included (default 12).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.preview = layout;
        self
    }

    /// Rows the preview takes when it is below the list (default 8).
    pub fn preview_height(mut self, rows: usize) -> Self {
        self.preview_height = rows.max(1);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Report the mouse (#476): a click focuses a result and a second one
    /// picks it, a click on a group's title narrows to it.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// Make `keys` do `action` (see [`jump_keymap`]) here only.
    pub fn rebind(
        mut self,
        action: &str,
        keys: impl IntoIterator<Item = crate::event::Key>,
    ) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    pub fn search_index(&self) -> &JumpIndex<T> {
        &self.index
    }

    /// What is typed.
    pub fn query_text(&self) -> &str {
        &self.query
    }

    /// The source searched alone, by a prefix typed or by Tab.
    pub fn narrowed(&self) -> Option<usize> {
        self.index.scope(&self.query).0.or(self.only)
    }

    /// The focused result: `(source, item)`.
    pub fn focused(&self) -> Option<(usize, usize)> {
        match self.rows.get(self.cursor)? {
            Row::Hit(source, item, _) => Some((*source, *item)),
            Row::Title(..) => None,
        }
    }

    /// How many results are listed.
    pub fn result_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|row| matches!(row, Row::Hit(..)))
            .count()
    }

    /// Search again and put the cursor on the best result.
    fn refresh(&mut self) {
        let groups = self.index.search(&self.query, self.only);
        let mut best: Option<(i64, usize)> = None;
        self.rows.clear();
        for group in groups {
            self.rows.push(Row::Title(group.source, group.more));
            for hit in group.hits {
                if best.is_none_or(|(score, _)| hit.score > score) {
                    best = Some((hit.score, self.rows.len()));
                }
                self.rows
                    .push(Row::Hit(group.source, hit.item, hit.positions));
            }
        }
        self.cursor = best.map_or(0, |(_, row)| row);
        // With nothing typed, the first result is the place to start.
        if self.query.is_empty() {
            self.cursor = self.first_hit_from(0, 1).unwrap_or(0);
        }
        self.offset = 0;
        self.clicked = None;
        self.follow();
    }

    /// The first result row from `row` going `direction` (±1).
    fn first_hit_from(&self, row: usize, direction: isize) -> Option<usize> {
        let mut at = row as isize;
        while at >= 0 && (at as usize) < self.rows.len() {
            if matches!(self.rows[at as usize], Row::Hit(..)) {
                return Some(at as usize);
            }
            at += direction;
        }
        None
    }

    /// Rows for results: the height asked for, cut to the terminal beside
    /// the question and the footer.
    fn page(&self) -> usize {
        self.height.min(self.space.get().saturating_sub(2)).max(1)
    }

    fn follow(&mut self) {
        let page = self.page();
        // A group's title shows above its first result.
        let top = match self.cursor.checked_sub(1).map(|row| &self.rows[row]) {
            Some(Row::Title(..)) => self.cursor - 1,
            _ => self.cursor,
        };
        if top < self.offset {
            self.offset = top;
        } else if self.cursor >= self.offset + page {
            self.offset = self.cursor + 1 - page;
        }
    }

    /// Move by `delta` results, stopping at the ends.
    fn step(&mut self, delta: isize) {
        let hits: Vec<usize> = (0..self.rows.len())
            .filter(|&row| matches!(self.rows[row], Row::Hit(..)))
            .collect();
        let Some(at) = hits.iter().position(|&row| row == self.cursor) else {
            return;
        };
        let to = at.saturating_add_signed(delta).min(hits.len() - 1);
        self.cursor = hits[to];
        self.follow();
    }

    /// To the first result of the next (or previous) group.
    fn jump_group(&mut self, forward: bool) {
        let titles: Vec<usize> = (0..self.rows.len())
            .filter(|&row| matches!(self.rows[row], Row::Title(..)))
            .collect();
        let current = titles.iter().rposition(|&title| title < self.cursor);
        let target = match (current, forward) {
            (Some(at), true) => titles.get(at + 1),
            (Some(at), false) if at > 0 => titles.get(at - 1),
            _ => None,
        };
        if let Some(&title) = target {
            if let Some(hit) = self.first_hit_from(title, 1) {
                self.cursor = hit;
                self.follow();
            }
        }
    }

    /// Narrow to the next source (`forward`) or the previous, through
    /// "every source" at either end.
    fn narrow(&mut self, forward: bool) {
        let count = self.index.sources.len();
        if count == 0 {
            return;
        }
        // A typed prefix gives way to Tab's choice.
        if let (Some(_), rest) = self.index.scope(&self.query) {
            self.query = rest.to_string();
        }
        self.only = match (self.only, forward) {
            (None, true) => Some(0),
            (None, false) => Some(count - 1),
            (Some(at), true) => (at + 1 < count).then_some(at + 1),
            (Some(at), false) => at.checked_sub(1),
        };
        self.refresh();
    }

    fn pick(&mut self) -> Option<(usize, usize)> {
        let (source, item) = self.focused()?;
        let label = &self.index.sources[source].items[item].label;
        self.answer = Some(Some(label.clone()));
        Some((source, item))
    }

    fn mouse_event(
        &mut self,
        kind: MouseKind,
        column: usize,
        row: usize,
        width: usize,
    ) -> Flow<(usize, usize)> {
        let (layout, left) = self.layout(width);
        match kind {
            MouseKind::ScrollUp => self.step(-1),
            MouseKind::ScrollDown => self.step(1),
            MouseKind::Down(Button::Left) => {
                if row == 0
                    || row > self.page()
                    || (layout == PreviewLayout::Right && column >= left)
                {
                    return Flow::Continue;
                }
                let at = self.offset + row - 1;
                match self.rows.get(at) {
                    Some(Row::Hit(..)) => {
                        if self.clicked == Some(at) && self.cursor == at {
                            return self.pick().map_or(Flow::Continue, Flow::Done);
                        }
                        self.cursor = at;
                        self.clicked = Some(at);
                    }
                    Some(Row::Title(source, _)) => {
                        let source = *source;
                        if let (Some(_), rest) = self.index.scope(&self.query) {
                            self.query = rest.to_string();
                        }
                        self.only = if self.only == Some(source) {
                            None
                        } else {
                            Some(source)
                        };
                        self.refresh();
                    }
                    None => {}
                }
            }
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn event(&mut self, event: &Event, context: &Context<'_>) -> Flow<(usize, usize)> {
        self.space.set(context.height);
        if let Some(mouse) = event.mouse() {
            return self.mouse_event(
                mouse.kind,
                mouse.column as usize,
                mouse.row as usize,
                context.width,
            );
        }
        if let Event::Paste(text) = event {
            self.query.push_str(&pasted(text, " "));
            self.refresh();
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Ignored;
        };
        self.clicked = None;
        let action = self.keymap.action(key).or_else(|| {
            let typing = matches!(key.code, KeyCode::Char(_));
            (!typing && key.modifiers != Modifiers::NONE)
                .then(|| self.keymap.action(crate::event::Key::new(key.code)))
                .flatten()
        });
        let page = self.page() as isize;
        match action {
            Some("pick") => {
                return self.pick().map_or(Flow::Continue, Flow::Done);
            }
            Some("cancel") => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            Some("up") => self.step(-1),
            Some("down") => self.step(1),
            Some("page-up") => self.step(-page),
            Some("page-down") => self.step(page),
            Some("first") => self.step(isize::MIN / 2),
            Some("last") => self.step(isize::MAX / 2),
            Some("previous-group") => self.jump_group(false),
            Some("next-group") => self.jump_group(true),
            Some("narrow") => self.narrow(true),
            Some("widen") => self.narrow(false),
            Some("clear") => {
                self.query.clear();
                self.refresh();
            }
            Some("delete") => {
                if self.query.pop().is_some() {
                    self.refresh();
                }
            }
            _ => match key.code {
                KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => {
                    self.query.push(c);
                    self.refresh();
                }
                _ => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn layout(&self, width: usize) -> (PreviewLayout, usize) {
        let preview = self
            .focused()
            .is_some_and(|(source, item)| self.index.sources[source].items[item].preview.is_some())
            && self.preview != PreviewLayout::Hidden;
        if !preview {
            return (PreviewLayout::Hidden, width);
        }
        let layout = match self.preview {
            PreviewLayout::Auto if width >= 72 => PreviewLayout::Right,
            PreviewLayout::Auto => PreviewLayout::Below,
            layout => layout,
        };
        let left = (width * 45 / 100).max(20).min(width);
        (
            layout,
            if layout == PreviewLayout::Right {
                left
            } else {
                width
            },
        )
    }

    fn theme_style(context: &Context<'_>, key: &str, default: &str) -> Style {
        context
            .console
            .theme()
            .get(key)
            .cloned()
            .unwrap_or_else(|| Style::parse(default).expect("a built-in style"))
    }

    fn row(&self, context: &Context<'_>, at: usize, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        match &self.rows[at] {
            Row::Title(source, more) => {
                let source = &self.index.sources[*source];
                let mut line = vec![text(
                    source.title.clone(),
                    &Self::theme_style(context, "jump.group", "bold"),
                )];
                if let Some(prefix) = source.prefix {
                    line.push(text(format!(" {prefix}"), &theme.hint));
                }
                if *more > 0 {
                    line.push(text(
                        format!("  +{more} more · tab"),
                        &Self::theme_style(context, "jump.more", "dim"),
                    ));
                }
                fit(line, width)
            }
            Row::Hit(source, item, positions) => {
                let item = &self.index.sources[*source].items[*item];
                let focused = at == self.cursor;
                let mut line = if focused {
                    vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
                } else {
                    vec![plain("  ")]
                };
                line.extend(highlight(
                    &item.label,
                    positions,
                    focused.then_some(&theme.focused),
                    &theme.matched,
                ));
                if let Some(description) = &item.description {
                    line.push(text(format!("  {description}"), &theme.hint));
                }
                fit(line, width)
            }
        }
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
        if let Some(only) = self.only {
            header.push(text(
                format!("[{}] ", self.index.sources[only].title),
                &theme.checked_style,
            ));
        }
        let column = crate::components::width(&header) + rich::cells::cell_len(&self.query);
        header.push(plain(self.query.clone()));
        let mut lines = vec![fit(header, width)];
        let page = self.page();
        let (layout, left) = self.layout(width);
        let mut list: Vec<Vec<Segment>> = (self.offset..self.rows.len())
            .take(page)
            .map(|at| self.row(context, at, left))
            .collect();
        if self.rows.is_empty() {
            list.push(vec![text("  no matches", &theme.hint)]);
        }
        let shown = page.min(self.rows.len().max(1));
        list.resize_with(shown, Vec::new);
        let narrowed = self.narrowed();
        let mut hint = format!("  {} results", self.result_count());
        hint.push_str(match narrowed {
            Some(_) => " · shift+tab widen",
            None => " · tab narrow",
        });
        hint.push_str(" · ↑↓ move · alt+↑↓ group · enter jump · esc cancel");
        let prefixes: Vec<String> = self
            .index
            .sources
            .iter()
            .filter_map(|s| s.prefix.map(|p| format!("{p} {}", s.title.to_lowercase())))
            .collect();
        if narrowed.is_none() && !prefixes.is_empty() {
            hint.push_str(&format!(" · {}", prefixes.join(", ")));
        }
        let footer = fit(vec![text(hint, &theme.hint)], width);
        let preview = self
            .focused()
            .and_then(|(source, item)| self.index.sources[source].items[item].preview.as_ref());
        match (preview, layout) {
            (Some(preview), PreviewLayout::Right) => {
                let right = width.saturating_sub(left + 3).max(1);
                let shown = context.lines_at(&*preview.renderable(), right);
                for (row, line) in list.into_iter().enumerate() {
                    let mut line = pad(line, left);
                    line.push(text(" │ ", &theme.border));
                    if let Some(preview) = shown.get(row) {
                        line.extend(fit(preview.clone(), right));
                    }
                    lines.push(line);
                }
                lines.push(footer);
            }
            (Some(preview), PreviewLayout::Below) => {
                lines.extend(list);
                lines.push(footer);
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
                lines.extend(list);
                lines.push(footer);
            }
        }
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }
}

impl<T: Clone> Jump<T> {
    fn jumped(&self, (source, item): (usize, usize)) -> Jumped<T> {
        let entry = &self.index.sources[source].items[item];
        Jumped {
            source: self.index.sources[source].id.clone(),
            index: item,
            label: entry.label.clone(),
            value: entry.value.clone(),
        }
    }
}

impl<T: Clone> Component for Jump<T> {
    type Output = Jumped<T>;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Jumped<T>> {
        match self.event(event, context) {
            Flow::Done(at) => Flow::Done(self.jumped(at)),
            Flow::Cancel => Flow::Cancel,
            Flow::Ignored => Flow::Ignored,
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.render_view(context)
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn keymap(&self) -> Keymap {
        self.keymap.clone()
    }

    /// The best match for the query it started with; none with nothing
    /// typed.
    fn default_value(&self) -> Option<Jumped<T>> {
        if self.query.trim().is_empty() {
            return None;
        }
        self.index.best(&self.query).map(|at| self.jumped(at))
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<Jumped<T>>, NotInteractive> {
        io.write(&format!("{}: ", self.prompt));
        let line = match io.read_line() {
            Some(line) if !line.trim().is_empty() => line,
            // At the end of input or on an empty line, the default answers.
            _ => return self.default_value().map(Some).ok_or(NotInteractive::Ended),
        };
        match self.index.best(line.trim()) {
            Some(at) => Ok(Some(self.jumped(at))),
            None => Err(NotInteractive::Invalid(format!(
                "nothing matches {:?}",
                line.trim()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> JumpIndex<&'static str> {
        JumpIndex::new()
            .source(
                JumpSource::new("files", "Files")
                    .items(["src/main.rs", "src/lib.rs", "README.md"])
                    .limit(2),
            )
            .source(
                JumpSource::new("commands", "Commands")
                    .prefix('>')
                    .items(["build", "test"]),
            )
    }

    #[test]
    fn searches_group_by_source_and_cut_to_limits() {
        let groups = index().search("", None);
        assert_eq!(groups.len(), 2);
        assert_eq!((groups[0].hits.len(), groups[0].more), (2, 1));
        let narrowed = index().search("", Some(0));
        assert_eq!((narrowed.len(), narrowed[0].hits.len()), (1, 3));
        let commands = index().search(">te", None);
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].source, 1);
        assert_eq!(index().best("lib"), Some((0, 1)));
        assert_eq!(index().best("zzz"), None);
    }
}

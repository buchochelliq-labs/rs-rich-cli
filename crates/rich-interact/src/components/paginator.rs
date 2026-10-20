//! A paginator (0.0.21): which page of several is showing, as dots
//! (`● ○ ○`) or a fraction (`3/10`), moved with the arrow keys, Page Up
//! and Page Down, Home and End, the wheel, or a click on a dot.
//!
//! It is a small piece of state with a one-line indicator, to embed
//! anywhere a long list or a multi-page form needs one ([`Form`]'s groups
//! use it), and a [`Component`] of its own: on its own it shows a list of
//! items a page at a time, and Enter returns the page shown.
//!
//! [`Form`]: crate::Form

use std::ops::Range;

use rich::Segment;

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, question, text, Theme};
use crate::event::{Event, Key, MouseKind};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// How a [`Paginator`] shows the page.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PaginatorStyle {
    /// A dot a page, the current one filled: `○ ● ○`. Falls back to
    /// [`Fraction`](PaginatorStyle::Fraction) when the dots do not fit.
    #[default]
    Dots,
    /// The page and the count: `2/3`.
    Fraction,
}

/// A paginator's keys: Left and Right (and `h`, `l`, Page Up, Page Down)
/// move a page, Home and End go to the ends; standalone, Enter picks the
/// page and Escape cancels.
pub fn paginator_keymap() -> Keymap {
    Keymap::new("paginator")
        .bind("next-page", keys("right l pagedown"), "next page")
        .bind("previous-page", keys("left h pageup"), "previous page")
        .bind("first-page", keys("home"), "first page")
        .bind("last-page", keys("end"), "last page")
        .bind("choose", keys("enter"), "choose this page")
        .bind("cancel", keys("escape q"), "cancel")
}

/// Which page of several is showing.
///
/// ```
/// use rich_interact::headless::{self, Script};
/// use rich_interact::{Paginator, PaginatorStyle};
///
/// // Embedded: state and an indicator line.
/// let mut pages = Paginator::new(3);
/// assert!(pages.next_page());
/// assert_eq!(pages.page(), 1);
/// assert_eq!(pages.indicator_text(80), "○ ● ○");
/// assert_eq!(pages.style(PaginatorStyle::Fraction).indicator_text(80), "2/3");
///
/// // Standalone: a list shown a page at a time; Enter returns the page.
/// let letters = ["a", "b", "c", "d", "e"];
/// let paginator = Paginator::over("Letters", letters, 2);
/// let (outcome, record) = headless::run(paginator, Script::new().keys("right enter"), 40, 8);
/// assert_eq!(outcome.unwrap().value(), Some(1));
/// assert_eq!(record.last_frame(), "? Letters › page 2 of 3");
/// ```
#[must_use = "a component does nothing until it is run or mounted"]
pub struct Paginator {
    pages: usize,
    page: usize,
    style: PaginatorStyle,
    active: String,
    inactive: String,
    title: Option<String>,
    items: Vec<String>,
    per_page: usize,
    theme: Theme,
    mouse: bool,
    rebound: Vec<(String, Vec<Key>)>,
    /// `Some(true)` once a page was chosen, `Some(false)` once cancelled.
    answer: Option<bool>,
}

impl Paginator {
    /// A paginator over `pages` pages (at least one), on the first.
    pub fn new(pages: usize) -> Paginator {
        Paginator {
            pages: pages.max(1),
            page: 0,
            style: PaginatorStyle::Dots,
            active: "●".into(),
            inactive: "○".into(),
            title: None,
            items: Vec::new(),
            per_page: 1,
            theme: Theme::default(),
            mouse: false,
            rebound: Vec::new(),
            answer: None,
        }
    }

    /// A paginator over `items`, `per_page` at a time, titled `title`: the
    /// standalone component shows the current page's items above the
    /// indicator.
    pub fn over<I, S>(title: impl Into<String>, items: I, per_page: usize) -> Paginator
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let items: Vec<String> = items.into_iter().map(Into::into).collect();
        let per_page = per_page.max(1);
        let mut paginator = Paginator::new(items.len().div_ceil(per_page));
        paginator.title = Some(title.into());
        paginator.items = items;
        paginator.per_page = per_page;
        paginator
    }

    /// Dots or a fraction (default: dots).
    pub fn style(mut self, style: PaginatorStyle) -> Self {
        self.style = style;
        self
    }

    /// The dots for the current page and the others (default `●` and `○`).
    pub fn dots(mut self, active: impl Into<String>, inactive: impl Into<String>) -> Self {
        self.active = active.into();
        self.inactive = inactive.into();
        self
    }

    /// Start on `page` (0-based; past the last means the last).
    pub fn start_at(mut self, page: usize) -> Self {
        self.set_page(page);
        self
    }

    /// The current dot is drawn in the theme's `checked_style`, the others
    /// and the fraction's count in `hint`.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Report the mouse: a click on a dot goes to its page, and the wheel
    /// moves a page.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// Make `keys` do `action` on this paginator only (see
    /// [`paginator_keymap`]); no keys unbinds it.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.rebound
            .push((action.to_string(), keys.into_iter().collect()));
        self
    }

    /// The number of pages.
    pub fn pages(&self) -> usize {
        self.pages
    }

    /// The current page, from 0.
    pub fn page(&self) -> usize {
        self.page
    }

    /// Change the number of pages (at least one), keeping the current page
    /// if it is still there.
    pub fn set_pages(&mut self, pages: usize) {
        self.pages = pages.max(1);
        self.page = self.page.min(self.pages - 1);
    }

    /// Go to `page`; past the last means the last.
    pub fn set_page(&mut self, page: usize) {
        self.page = page.min(self.pages - 1);
    }

    /// Move to the next page; `false` on the last.
    pub fn next_page(&mut self) -> bool {
        let moved = !self.on_last();
        self.page = (self.page + 1).min(self.pages - 1);
        moved
    }

    /// Move to the previous page; `false` on the first.
    pub fn previous_page(&mut self) -> bool {
        let moved = !self.on_first();
        self.page = self.page.saturating_sub(1);
        moved
    }

    pub fn on_first(&self) -> bool {
        self.page == 0
    }

    pub fn on_last(&self) -> bool {
        self.page + 1 == self.pages
    }

    /// The range of a list of `len` items on the current page, at
    /// `per_page` a page: what to slice for it.
    pub fn bounds(&self, len: usize, per_page: usize) -> Range<usize> {
        let start = (self.page * per_page.max(1)).min(len);
        start..(start + per_page.max(1)).min(len)
    }

    /// The keys, with this paginator's rebinding.
    pub fn keys(&self) -> Keymap {
        let mut keymap = paginator_keymap();
        for (action, keys) in &self.rebound {
            keymap.rebind(action, keys.clone());
        }
        keymap
    }

    /// Move for `key` if it is one of the moving keys (next, previous,
    /// first, last page): `true` when it was, whether or not the page
    /// changed. For a container that embeds a paginator.
    pub fn handle_key(&mut self, key: Key) -> bool {
        match self.keys().action(key) {
            Some("next-page") => self.next_page(),
            Some("previous-page") => self.previous_page(),
            Some("first-page") => {
                self.page = 0;
                true
            }
            Some("last-page") => {
                self.page = self.pages - 1;
                true
            }
            _ => return false,
        };
        true
    }

    /// Whether the dots of every page fit in `width` columns.
    fn dots_fit(&self, width: usize) -> bool {
        let dot = rich::cells::cell_len(&self.active).max(rich::cells::cell_len(&self.inactive));
        self.pages * (dot + 1) <= width + 1
    }

    fn uses_dots(&self, width: usize) -> bool {
        self.style == PaginatorStyle::Dots && self.dots_fit(width)
    }

    /// The indicator line, at most `width` columns.
    pub fn indicator(&self, width: usize) -> Vec<Segment> {
        if self.uses_dots(width) {
            let mut line = Vec::new();
            for page in 0..self.pages {
                if page > 0 {
                    line.push(plain(" "));
                }
                line.push(if page == self.page {
                    text(self.active.clone(), &self.theme.checked_style)
                } else {
                    text(self.inactive.clone(), &self.theme.hint)
                });
            }
            fit(line, width)
        } else {
            fit(
                vec![
                    text(format!("{}", self.page + 1), &self.theme.focused),
                    text(format!("/{}", self.pages), &self.theme.hint),
                ],
                width,
            )
        }
    }

    /// The indicator as plain text: what a screen reader or a log reads.
    pub fn indicator_text(&self, width: usize) -> String {
        self.indicator(width)
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    /// The page a click at `column` of the indicator (drawn at `width`)
    /// lands on: a dot's page, or `None` off the dots.
    pub fn page_at(&self, width: usize, column: usize) -> Option<usize> {
        if !self.uses_dots(width) {
            return None;
        }
        let mut at = 0;
        for page in 0..self.pages {
            let dot = if page == self.page {
                &self.active
            } else {
                &self.inactive
            };
            let end = at + rich::cells::cell_len(dot);
            if (at..end).contains(&column) {
                return Some(page);
            }
            at = end + 1;
        }
        None
    }

    /// Where the standalone view's indicator row is.
    fn indicator_row(&self) -> usize {
        usize::from(self.title.is_some()) + self.bounds(self.items.len(), self.per_page).len()
    }
}

impl Component for Paginator {
    type Output = usize;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<usize> {
        if let Some(mouse) = event.mouse() {
            match mouse.kind {
                MouseKind::ScrollDown => {
                    self.next_page();
                }
                MouseKind::ScrollUp => {
                    self.previous_page();
                }
                _ if mouse.is_click() && mouse.row as usize == self.indicator_row() => {
                    let width = context.width.saturating_sub(2);
                    if let Some(page) = (mouse.column as usize)
                        .checked_sub(2)
                        .and_then(|column| self.page_at(width, column))
                    {
                        self.page = page;
                    }
                }
                _ => {}
            }
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Continue;
        };
        if self.handle_key(key) {
            return Flow::Continue;
        }
        match self.keys().action(key) {
            Some("choose") => {
                self.answer = Some(true);
                Flow::Done(self.page)
            }
            Some("cancel") => {
                self.answer = Some(false);
                Flow::Cancel
            }
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let theme = &self.theme;
        let mut lines = Vec::new();
        let mut title = self.title.as_ref().map(|title| {
            let mut line = question(theme, title);
            line.truncate(line.len() - 3);
            line
        });
        match (self.answer, title.as_mut()) {
            (Some(chosen), Some(line)) => {
                let answer = if chosen {
                    format!("page {} of {}", self.page + 1, self.pages)
                } else {
                    "cancelled".into()
                };
                line.push(text(" › ", &theme.hint));
                line.push(text(
                    answer,
                    if chosen { &theme.answer } else { &theme.hint },
                ));
                return View::new(vec![fit(line.clone(), width)]);
            }
            (_, Some(line)) => lines.push(fit(line.clone(), width)),
            (_, None) => {}
        }
        for item in &self.items[self.bounds(self.items.len(), self.per_page)] {
            lines.push(fit(vec![plain("  "), plain(item.clone())], width));
        }
        let mut indicator = vec![plain("  ")];
        indicator.extend(self.indicator(width.saturating_sub(2)));
        lines.push(indicator);
        lines.push(fit(
            vec![text("  ←/→ page · enter choose · esc cancel", &theme.hint)],
            width,
        ));
        View::new(lines)
    }

    fn keymap(&self) -> Keymap {
        self.keys()
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn default_value(&self) -> Option<usize> {
        Some(self.page)
    }

    /// Without a terminal: every item, then the page asked for by number.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<usize>, NotInteractive> {
        if let Some(title) = &self.title {
            io.write(&format!("{title}\n"));
        }
        for item in &self.items {
            io.write(&format!("  {item}\n"));
        }
        io.write(&format!("Page (1-{}) [{}]: ", self.pages, self.page + 1));
        let line = io.read_line().unwrap_or_default();
        let line = line.trim();
        if line.is_empty() {
            return Ok(Some(self.page));
        }
        match line.parse::<usize>() {
            Ok(page) if (1..=self.pages).contains(&page) => {
                self.page = page - 1;
                Ok(Some(self.page))
            }
            _ => Err(NotInteractive::Invalid(format!(
                "not a page from 1 to {}: {line:?}",
                self.pages
            ))),
        }
    }
}

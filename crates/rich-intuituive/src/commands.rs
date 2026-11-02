//! Command palette providers (#803), as Textual's `Provider` classes feed
//! its command palette: a [`Provider`] offers [`Hit`]s for a search, each
//! with a score, the characters it matched and what it runs. The palette
//! ([`Ctx::palette`](crate::Ctx::palette)) lists the focused path's
//! described bindings and the app's [`SystemCommands`] at once, and merges
//! in each registered provider's hits ([`App::provider`](crate::App::provider))
//! as they arrive, best score first.
//!
//! A provider's search runs on the app's thread; one that is slow (a walk
//! of a directory, a symbol index) moves its work to a thread of its own
//! with [`HitSink::background`] and sends its hits from there. A new query
//! cancels the searches still running: their sink reports
//! [`cancelled`](HitSink::cancelled), and what they send is dropped.
//!
//! [`Themes`] is the built-in provider of the theme picker that the
//! palette's *Theme* command opens.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rich::Segment;
use rich::cells::cell_len;
use rich_interact::kit::{self, ListState, Theme};
use rich_interact::overlay::palette_keymap;
use rich_interact::{Component, Context, Event, Flow, Key, KeyCode, Keymap, MouseKind, View};

use crate::app::Ctx;
use crate::reactive::{Proxy, Runtime, Signal, signal, untracked};

/// What a [`Hit`] runs when it is chosen.
type Run = Arc<dyn Fn(&mut Ctx) + Send + Sync>;

/// One result of a palette search: what identifies it, what it says, its
/// help line, its shortcut, its score (higher first) and the characters of
/// its label that matched (highlighted), and what choosing it does.
///
/// ```
/// use intuituive::Hit;
///
/// let hit = Hit::new("open", "Open a file", |_| {}).matching("opf").unwrap();
/// assert_eq!(hit.positions, vec![0, 1, 7]);
/// assert!(Hit::new("open", "Open a file", |_| {}).matching("xyz").is_none());
/// ```
#[derive(Clone)]
pub struct Hit {
    pub id: String,
    pub label: String,
    /// A line of help, shown dim after the label.
    pub help: String,
    /// The group it is listed under: its provider's
    /// [`name`](Provider::name), unless set.
    pub group: String,
    /// Its shortcut, shown at the right.
    pub keys: Vec<Key>,
    /// Its keys are pressed one after another (`g g`).
    pub sequence: bool,
    /// Why it cannot run now: listed greyed, and not run.
    pub unavailable: Option<String>,
    /// Higher is listed first. [`matching`](Self::matching) sets it.
    pub score: i64,
    /// The characters of `label` that matched, highlighted.
    pub positions: Vec<usize>,
    run: Run,
}

impl fmt::Debug for Hit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hit")
            .field("id", &self.id)
            .field("label", &self.label)
            .field("group", &self.group)
            .field("score", &self.score)
            .field("positions", &self.positions)
            .finish_non_exhaustive()
    }
}

impl Hit {
    /// A hit that calls `run` when chosen (after the palette closes).
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        run: impl Fn(&mut Ctx) + Send + Sync + 'static,
    ) -> Hit {
        Hit::with_run(id, label, Arc::new(run))
    }

    pub(crate) fn with_run(id: impl Into<String>, label: impl Into<String>, run: Run) -> Hit {
        Hit {
            id: id.into(),
            label: label.into(),
            help: String::new(),
            group: String::new(),
            keys: Vec::new(),
            sequence: false,
            unavailable: None,
            score: 0,
            positions: Vec::new(),
            run,
        }
    }

    #[must_use]
    pub fn help(mut self, help: impl Into<String>) -> Hit {
        self.help = help.into();
        self
    }

    #[must_use]
    pub fn group(mut self, group: impl Into<String>) -> Hit {
        self.group = group.into();
        self
    }

    /// Its shortcut: space-separated key names (`"ctrl+s"`).
    ///
    /// # Panics
    /// On a name that is not a key.
    #[must_use]
    pub fn keys(mut self, keys: &str) -> Hit {
        self.keys = crate::node::parse_keys(keys);
        self
    }

    #[must_use]
    pub fn unavailable(mut self, reason: impl Into<String>) -> Hit {
        self.unavailable = Some(reason.into());
        self
    }

    #[must_use]
    pub fn score(mut self, score: i64) -> Hit {
        self.score = score;
        self
    }

    /// Highlight these characters of the label.
    #[must_use]
    pub fn positions(mut self, positions: Vec<usize>) -> Hit {
        self.positions = positions;
        self
    }

    /// The hit scored and highlighted by the palette's fuzzy match of
    /// `query` against its label (rs-rich-interact's `fuzzy`), or `None`
    /// when it does not match. An empty query matches with score 0.
    #[must_use]
    pub fn matching(mut self, query: &str) -> Option<Hit> {
        let found = rich_interact::fuzzy::fuzzy(query, &self.label)?;
        self.score = found.score;
        self.positions = found.positions;
        Some(self)
    }

    /// Run it, as the palette does when it is chosen.
    pub fn run(&self, cx: &mut Ctx) {
        (self.run)(cx);
    }
}

/// Something that offers the command palette [`Hit`]s: Textual's
/// `Provider`. Register one with [`App::provider`](crate::App::provider),
/// or open a palette of it alone with
/// [`Ctx::palette_of`](crate::Ctx::palette_of).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{Hit, HitSink, Provider};
///
/// struct Colours;
/// impl Provider for Colours {
///     fn name(&self) -> &str {
///         "colour"
///     }
///     fn search(&self, query: &str, sink: &HitSink) {
///         for name in ["red", "green", "blue"] {
///             let hit = Hit::new(name, name, move |_| {});
///             if let Some(hit) = hit.matching(query) {
///                 sink.send(hit);
///             }
///         }
///     }
/// }
///
/// let app = App::new(|| label("hi")).provider(Colours).palette_key("ctrl+p");
/// let mut driver = app.driver(40, 8);
/// driver.press("ctrl+p g r e");
/// assert!(driver.screen().plain().join("\n").contains("colour green"));
/// ```
pub trait Provider: 'static {
    /// What its hits are listed under, unless a hit says otherwise.
    fn name(&self) -> &str {
        ""
    }

    /// The hits shown before anything is typed.
    fn discover(&self) -> Vec<Hit> {
        Vec::new()
    }

    /// Send the hits for `query` to `sink`, now or from a thread of its
    /// own ([`HitSink::background`]).
    fn search(&self, query: &str, sink: &HitSink);
}

/// What the palette shares with the searches it started.
struct Shared {
    /// The search the palette shows now; older ones are cancelled.
    generation: AtomicU64,
    /// Every hit sent, with the search it was sent for.
    hits: Mutex<Vec<(u64, Hit)>>,
    /// Background searches still running.
    pending: AtomicUsize,
}

impl Shared {
    fn hits(&self) -> std::sync::MutexGuard<'_, Vec<(u64, Hit)>> {
        self.hits.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Where a [`Provider`] sends its hits for one search. It may be cloned
/// and sent to another thread.
#[derive(Clone)]
pub struct HitSink {
    shared: Arc<Shared>,
    generation: u64,
    group: Arc<str>,
    /// Bumped to draw the palette again when a hit arrives.
    changed: Signal<u64>,
    /// Set off the app's thread: what bumps `changed` there.
    proxy: Option<Proxy>,
}

impl HitSink {
    /// Add `hit` to the palette; `false`, and nothing added, once the
    /// search was cancelled.
    pub fn send(&self, mut hit: Hit) -> bool {
        if self.cancelled() {
            return false;
        }
        if hit.group.is_empty() {
            hit.group = self.group.to_string();
        }
        self.shared.hits().push((self.generation, hit));
        if let Some(proxy) = &self.proxy {
            let changed = self.changed;
            proxy.run(move || changed.update(|n| *n += 1));
        }
        true
    }

    /// Whether the query changed (or the palette closed) since this search
    /// began: stop then.
    pub fn cancelled(&self) -> bool {
        self.shared.generation.load(Ordering::SeqCst) != self.generation
    }

    /// Run `work` on a thread of its own with a sink of this search, its
    /// hits merged in as they arrive. It counts as a
    /// [task](crate::spawn_stream) until it returns, so an app that
    /// [waits for its tasks](crate::App::wait_for_tasks) sees all of it.
    ///
    /// # Panics
    /// Outside a running app.
    pub fn background(&self, work: impl FnOnce(HitSink) + Send + 'static) {
        let runtime = Runtime::current_for("HitSink::background()");
        let proxy = runtime.proxy();
        let sink = HitSink {
            proxy: Some(proxy.clone()),
            ..self.clone()
        };
        let shared = self.shared.clone();
        shared.pending.fetch_add(1, Ordering::SeqCst);
        let changed = self.changed;
        crate::task::spawn_stream(move |_| {
            // Done however the work ends: the palette stops saying it
            // searches.
            struct Done(Arc<Shared>, Proxy, Signal<u64>);
            impl Drop for Done {
                fn drop(&mut self) {
                    self.0.pending.fetch_sub(1, Ordering::SeqCst);
                    let changed = self.2;
                    self.1.run(move || changed.update(|n| *n += 1));
                }
            }
            let _done = Done(shared, proxy, changed);
            work(sink);
        });
    }
}

/// The built-in themes ([`Theme::builtin_names`](crate::Theme::builtin_names))
/// as a provider: choosing one switches the app to it. The palette's
/// *Theme* command opens a palette of it.
#[derive(Clone, Copy, Debug, Default)]
pub struct Themes;

impl Themes {
    fn hit(name: &'static str) -> Hit {
        Hit::new(format!("theme.{name}"), name, move |cx| {
            cx.set_theme_named(name);
        })
    }
}

impl Provider for Themes {
    fn name(&self) -> &str {
        "theme"
    }

    fn discover(&self) -> Vec<Hit> {
        crate::Theme::builtin_names()
            .into_iter()
            .map(Themes::hit)
            .collect()
    }

    fn search(&self, query: &str, sink: &HitSink) {
        for name in crate::Theme::builtin_names() {
            if let Some(hit) = Themes::hit(name).matching(query) {
                sink.send(hit);
            }
        }
    }
}

/// The commands the palette offers on its own, after Textual's
/// `App.get_system_commands`: *Theme* (a picker of [`Themes`]), *Keys*
/// (shows or hides the [`key_panel`](crate::key_panel), or opens the help
/// when there is none), *Maximize* or *Minimize* (the focused pane),
/// *Screenshot* and *Quit*. All are on unless turned off:
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::SystemCommands;
///
/// let app = App::new(|| label("hi"))
///     .system_commands(SystemCommands::default().without("quit"))
///     .palette_key("ctrl+p");
/// let mut driver = app.driver(40, 12);
/// driver.press("ctrl+p");
/// let shown = driver.screen().plain().join("\n");
/// assert!(shown.contains("Theme") && !shown.contains("Quit"));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemCommands {
    on: Vec<&'static str>,
}

impl Default for SystemCommands {
    fn default() -> Self {
        SystemCommands {
            on: SystemCommands::NAMES.to_vec(),
        }
    }
}

impl SystemCommands {
    /// Every system command's name, in the order the palette lists them.
    pub const NAMES: [&'static str; 6] =
        ["theme", "keys", "maximize", "minimize", "screenshot", "quit"];

    /// None of them.
    pub fn none() -> SystemCommands {
        SystemCommands { on: Vec::new() }
    }

    /// Turn `name` off.
    #[must_use]
    pub fn without(mut self, name: &str) -> SystemCommands {
        self.on.retain(|on| *on != name);
        self
    }

    /// Turn `name` (one of [`NAMES`](Self::NAMES)) on; an unknown name
    /// does nothing.
    #[must_use]
    pub fn with(mut self, name: &str) -> SystemCommands {
        if let Some(known) = SystemCommands::NAMES.iter().find(|n| **n == name)
            && !self.on.contains(known)
        {
            self.on.push(known);
            self.on.sort_by_key(|on| {
                SystemCommands::NAMES
                    .iter()
                    .position(|n| n == on)
                    .unwrap_or(usize::MAX)
            });
        }
        self
    }

    /// Whether `name` is on.
    pub fn is_on(&self, name: &str) -> bool {
        self.on.contains(&name)
    }

    /// The hits of those on, as the palette lists them now: Maximize while
    /// nothing is maximized, Minimize while something is.
    pub(crate) fn hits(&self, maximized: bool) -> Vec<Hit> {
        let mut hits = Vec::new();
        for name in &self.on {
            let hit = match *name {
                "theme" => Hit::new("app.theme", "Theme", |cx| cx.palette_of(Themes))
                    .help("Change the theme"),
                "keys" => Hit::new("app.keys", "Keys", |cx| {
                    if Runtime::current().key_panel.get().is_some() {
                        cx.toggle_key_panel();
                    } else {
                        cx.help();
                    }
                })
                .help("Show or hide the keys"),
                "maximize" if !maximized => {
                    Hit::new("app.maximize", "Maximize", |cx| cx.maximize())
                        .help("Show the focused pane alone")
                }
                "minimize" if maximized => {
                    Hit::new("app.minimize", "Minimize", |cx| cx.minimize())
                        .help("Restore the layout")
                }
                "screenshot" => Hit::new("app.screenshot", "Screenshot", |cx| {
                    cx.screenshot_to_dir();
                })
                .help("Save an SVG of the screen"),
                "quit" => Hit::new("app.quit", "Quit", |cx| cx.quit()).help("Quit the app"),
                _ => continue,
            };
            hits.push(hit.group("app"));
        }
        hits
    }
}

/// A palette over live hits (the bindings and system commands, read again
/// as it is drawn) and providers (searched as the query changes).
pub(crate) struct ProviderPalette {
    prompt: String,
    query: String,
    live: Box<dyn Fn() -> Vec<Hit>>,
    providers: Vec<Rc<dyn Provider>>,
    shared: Arc<Shared>,
    changed: Signal<u64>,
    /// What is listed now, best first.
    shown: RefCell<Vec<Hit>>,
    list: RefCell<ListState>,
    page: Cell<usize>,
    theme: Theme,
    keymap: Keymap,
}

impl ProviderPalette {
    pub(crate) fn new(
        prompt: impl Into<String>,
        live: Box<dyn Fn() -> Vec<Hit>>,
        providers: Vec<Rc<dyn Provider>>,
    ) -> ProviderPalette {
        let palette = ProviderPalette {
            prompt: prompt.into(),
            query: String::new(),
            live,
            providers,
            shared: Arc::new(Shared {
                generation: AtomicU64::new(0),
                hits: Mutex::new(Vec::new()),
                pending: AtomicUsize::new(0),
            }),
            changed: signal(0u64),
            shown: RefCell::new(Vec::new()),
            list: RefCell::new(ListState::new()),
            page: Cell::new(10),
            theme: Theme::default(),
            keymap: palette_keymap(),
        };
        untracked(|| palette.search());
        palette
    }

    /// Start a search for the query: the older ones are cancelled.
    fn search(&self) {
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.shared.hits().retain(|(at, _)| *at == generation);
        let blank = self.query.trim().is_empty();
        for provider in &self.providers {
            let sink = HitSink {
                shared: self.shared.clone(),
                generation,
                group: Arc::from(provider.name()),
                changed: self.changed,
                proxy: None,
            };
            if blank {
                for hit in provider.discover() {
                    sink.send(hit);
                }
            } else {
                provider.search(&self.query, &sink);
            }
        }
        self.refresh();
        let mut list = self.list.borrow_mut();
        list.set_len(self.shown.borrow().len());
        list.reset(0, self.page.get());
    }

    /// List the hits as they are now, the focus staying on the same id.
    fn refresh(&self) {
        let focused = self.focused().map(|hit| hit.id);
        let generation = self.shared.generation.load(Ordering::SeqCst);
        let mut hits: Vec<Hit> = (self.live)()
            .into_iter()
            .filter_map(|hit| hit.matching(&self.query))
            .collect();
        hits.extend(
            self.shared
                .hits()
                .iter()
                .filter(|(at, _)| *at == generation)
                .map(|(_, hit)| hit.clone()),
        );
        // Best first; equal scores keep their order (bindings first).
        hits.sort_by_key(|hit| std::cmp::Reverse(hit.score));
        let at = focused.and_then(|id| hits.iter().position(|hit| hit.id == id));
        let len = hits.len();
        *self.shown.borrow_mut() = hits;
        let mut list = self.list.borrow_mut();
        let cursor = list.cursor();
        list.set_len(len);
        if len > 0 {
            list.select_row(at.unwrap_or(cursor.min(len - 1)), self.page.get());
        }
    }

    fn focused(&self) -> Option<Hit> {
        self.shown.borrow().get(self.list.borrow().cursor()).cloned()
    }

    fn chosen(&self) -> Flow<Hit> {
        match self.focused().filter(|hit| hit.unavailable.is_none()) {
            Some(hit) => Flow::Done(hit),
            None => Flow::Continue,
        }
    }

    fn row(&self, hit: &Hit, focused: bool, group_width: usize, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        let mut line = if focused {
            vec![kit::text(
                format!("{} ", theme.pointer),
                &theme.pointer_style,
            )]
        } else {
            vec![kit::plain("  ")]
        };
        if group_width > 0 {
            line.extend(kit::pad(
                vec![kit::text(&hit.group, &theme.hint)],
                group_width + 1,
            ));
        }
        let base = match (&hit.unavailable, focused) {
            (Some(_), true) => Some(theme.focused.combine(&theme.hint)),
            (Some(_), false) => Some(theme.hint.clone()),
            (None, true) => Some(theme.focused.clone()),
            (None, false) => None,
        };
        line.extend(kit::highlight(
            &hit.label,
            &hit.positions,
            base.as_ref(),
            &theme.matched,
        ));
        if let Some(reason) = hit.unavailable.as_ref().filter(|r| !r.is_empty()) {
            line.push(kit::text(format!(" ({reason})"), &theme.hint));
        }
        if !hit.help.is_empty() {
            line.push(kit::text(format!("  {}", hit.help), &theme.hint));
        }
        let shortcut = rich_interact::chrome::keys_hint_of(&hit.keys, hit.sequence);
        if !shortcut.is_empty() {
            let wanted = cell_len(&shortcut) + 1;
            if kit::width(&line) + wanted < width {
                line = kit::pad(line, width - wanted);
                line.push(kit::text(format!("{shortcut} "), &theme.hint));
            } else if width > wanted + 8 {
                line = kit::pad(line, width - wanted - 2);
                line.push(kit::plain("… "));
                line.push(kit::text(format!("{shortcut} "), &theme.hint));
            }
        }
        kit::fit(line, width)
    }
}

impl Drop for ProviderPalette {
    fn drop(&mut self) {
        // Closing cancels what is still running.
        self.shared.generation.fetch_add(1, Ordering::SeqCst);
    }
}

impl Component for ProviderPalette {
    type Output = Hit;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<Hit> {
        untracked(|| self.refresh());
        let page = self.page.get();
        let typed = |key: Key| match key.code {
            KeyCode::Char(c) if !key.modifiers.ctrl && !key.modifiers.alt => Some(c),
            _ => None,
        };
        match event {
            Event::Paste(text) => {
                self.query.push_str(&kit::pasted(text, " "));
                untracked(|| self.search());
                return Flow::Continue;
            }
            Event::Mouse(mouse) => {
                match mouse.kind {
                    MouseKind::ScrollUp => self.list.get_mut().step(-3, page),
                    MouseKind::ScrollDown => self.list.get_mut().step(3, page),
                    _ if mouse.is_click() => {
                        let row = mouse.row as usize;
                        let position = self.list.get_mut().offset() + row.wrapping_sub(1);
                        if row >= 1 && row <= page && position < self.shown.get_mut().len() {
                            self.list.get_mut().select_row(position, page);
                            return self.chosen();
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
            Some("up") => self.list.get_mut().step(-1, page),
            Some("down") => self.list.get_mut().step(1, page),
            Some("page-up") => self.list.get_mut().step(-(page as isize), page),
            Some("page-down") => self.list.get_mut().step(page as isize, page),
            Some("run") => return self.chosen(),
            Some("close") => return Flow::Cancel,
            Some("delete") => {
                if self.query.pop().is_some() {
                    untracked(|| self.search());
                }
            }
            Some("clear") => {
                self.query.clear();
                untracked(|| self.search());
            }
            _ => match typed(key) {
                Some(c) => {
                    self.query.push(c);
                    untracked(|| self.search());
                }
                None => return Flow::Ignored,
            },
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        // Drawn again when a hit arrives from another thread.
        self.changed.get();
        self.refresh();
        let width = context.width;
        let theme = &self.theme;
        let page = context.height.saturating_sub(2).max(1);
        self.page.set(page);
        let mut header = kit::question(theme, &self.prompt);
        let column = kit::width(&header) + cell_len(&self.query);
        header.push(kit::plain(self.query.clone()));
        let mut lines = vec![kit::fit(header, width)];
        let shown = self.shown.borrow();
        let group_width = shown
            .iter()
            .map(|hit| cell_len(&hit.group))
            .max()
            .unwrap_or(0)
            .min(width / 3);
        let list = self.list.borrow();
        for position in list.visible(page) {
            if let Some(hit) = shown.get(position) {
                lines.push(self.row(hit, position == list.cursor(), group_width, width));
            }
        }
        let searching = self.shared.pending.load(Ordering::SeqCst) > 0;
        if shown.is_empty() && !searching {
            lines.push(vec![kit::text("  no matching commands", &theme.hint)]);
        }
        lines.resize_with(page + 1, Vec::new);
        let mut hint = format!(
            "  {} · ↑↓ move · enter run · esc close",
            shown.len()
        );
        if searching {
            hint.push_str(" · searching…");
        }
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

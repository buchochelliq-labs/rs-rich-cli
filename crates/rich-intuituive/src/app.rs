//! The app: the tree, its runtime, and the loop that runs them.
//!
//! [`App::run`] takes the terminal (the alternate screen, raw mode, the
//! mouse), and gives it back on every way out, including a panic. Each turn
//! of the loop it runs work other threads sent through a
//! [`Proxy`], fires due timers, draws what changed and reads
//! one event:
//!
//! - **keys** go to the focused node, then bubble to each ancestor until a
//!   binding uses them; Tab and Shift+Tab move the focus through the
//!   [focusable](crate::Node::focusable) nodes; Ctrl+C ends the app;
//! - **clicks** go to the deepest clickable node under the pointer, found
//!   from the rectangles the last frame laid out, and focus it;
//! - **resizes** lay everything out again.
//!
//! An app is a stack of screens. The first is the one `App::new` builds;
//! a handler opens more with [`Ctx::push`] (a full screen) or [`Ctx::modal`]
//! (a box over the screen below it) and closes them with [`Ctx::pop`]. Keys
//! and clicks go to the top screen only, and each screen keeps its own
//! focus.
//!
//! [`App::inline`] runs in the normal screen instead, in a region of a
//! few rows below the cursor that stays in the scrollback when the app
//! ends.
//!
//! [`App::run_on`] runs on any [`Backend`], including `rich-interact`'s
//! headless driver, so tests script keys and read frames without a
//! terminal.

use std::io;
use std::rc::Rc;
use std::time::Duration;

use rich::{ColorSystem, Console, Style};
use rich_interact::{Backend, Button, Event, Key, KeyCode, MouseKind, Session, SessionOptions};

use crate::layout::Size;
use crate::node::{with_node, Axis, FrameState, Node};
use crate::reactive::{signal, NodeId, Proxy, Runtime, Signal};
use crate::screen::{Painter, Rect, Screen};

/// What a key or click handler can do.
pub struct Ctx {
    quit: bool,
    focus: Option<FocusMove>,
    proxy: Proxy,
    nav: Vec<Nav>,
    theme: Option<Theme>,
}

enum FocusMove {
    Next,
    Previous,
    To(NodeId),
}

type Build = Box<dyn FnOnce() -> Node>;

/// A change to the stack of screens.
enum Nav {
    Push(Build),
    Modal(Size, Size, Build),
    Pop,
    Replace(Build),
}

impl Ctx {
    pub(crate) fn new(proxy: Proxy) -> Ctx {
        Ctx {
            quit: false,
            focus: None,
            proxy,
            nav: Vec::new(),
            theme: None,
        }
    }

    /// Open a screen built by `build` over the current one, which is kept
    /// (its state, its focus) until this one is [popped](Self::pop). Like
    /// `App::new`'s closure, `build` may create signals and call
    /// [`every`]; the screen's timers stop when it closes.
    pub fn push(&mut self, build: impl FnOnce() -> Node + 'static) {
        self.nav.push(Nav::Push(Box::new(build)));
    }

    /// Open a modal: `build`'s node in a box `width` x `height` centred
    /// over the current screen, which stays visible below it. Sizes are
    /// cells ([`Size::Fixed`]), a share of the screen ([`Size::Percent`]),
    /// the content's ([`Size::Auto`]), or the whole screen
    /// ([`Size::Flex`]). Wrap the node in a [`panel`](crate::Node::panel)
    /// for a border, and bind Esc to [`pop`](Self::pop) to close it.
    pub fn modal(&mut self, width: Size, height: Size, build: impl FnOnce() -> Node + 'static) {
        self.nav.push(Nav::Modal(width, height, Box::new(build)));
    }

    /// Close the top screen or modal, going back to the one below with the
    /// focus it had. Does nothing on the first screen.
    pub fn pop(&mut self) {
        self.nav.push(Nav::Pop);
    }

    /// Replace the top screen with the one `build` makes.
    pub fn replace(&mut self, build: impl FnOnce() -> Node + 'static) {
        self.nav.push(Nav::Replace(Box::new(build)));
    }

    /// Switch the app to `theme`; everything is drawn again in it.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = Some(theme);
    }

    /// End the app after this event.
    pub fn quit(&mut self) {
        self.quit = true;
    }

    /// Move the focus to the next focusable node.
    pub fn focus_next(&mut self) {
        self.focus = Some(FocusMove::Next);
    }

    /// Move the focus to the previous focusable node.
    pub fn focus_previous(&mut self) {
        self.focus = Some(FocusMove::Previous);
    }

    /// Focus the node with `id`.
    pub fn focus(&mut self, id: NodeId) {
        self.focus = Some(FocusMove::To(id));
    }

    /// A handle for other threads (see [`Proxy`]).
    pub fn proxy(&self) -> Proxy {
        self.proxy.clone()
    }
}

/// The app's look: the styles of the parts the framework draws (borders,
/// the focused border, panel titles), and named styles for markup.
///
/// Every named style works in console markup: `[accent]…[/]`. The presets
/// define `accent`, `muted`, `good`, `warn` and `bad`, and the framework's
/// own as `border`, `border.focused` and `title`, so an app restyles by
/// name and switches themes at run time with [`Ctx::set_theme`].
#[derive(Clone, Debug)]
pub struct Theme {
    pub border: Style,
    pub border_focused: Style,
    pub title: Style,
    /// Named styles for markup, in addition to rich's defaults.
    pub styles: Vec<(String, Style)>,
}

impl Default for Theme {
    fn default() -> Self {
        Theme::dark()
    }
}

fn style(definition: &str) -> Style {
    Style::parse(definition).expect("a built-in style parses")
}

impl Theme {
    fn preset(border: &str, focused: &str, names: [(&str, &str); 5]) -> Theme {
        Theme {
            border: style(border),
            border_focused: style(focused),
            title: style("bold"),
            styles: names
                .iter()
                .map(|(name, definition)| (name.to_string(), style(definition)))
                .collect(),
        }
    }

    /// For dark terminals (the default).
    pub fn dark() -> Theme {
        Theme::preset(
            "blue",
            "bright_cyan",
            [
                ("accent", "bright_cyan"),
                ("muted", "dim"),
                ("good", "green"),
                ("warn", "yellow"),
                ("bad", "bold red"),
            ],
        )
    }

    /// For light terminals.
    pub fn light() -> Theme {
        Theme::preset(
            "grey50",
            "blue",
            [
                ("accent", "blue"),
                ("muted", "grey42"),
                ("good", "green4"),
                ("warn", "dark_orange3"),
                ("bad", "bold red3"),
            ],
        )
    }

    /// Without colour: emphasis only.
    pub fn mono() -> Theme {
        Theme::preset(
            "dim",
            "bold",
            [
                ("accent", "bold"),
                ("muted", "dim"),
                ("good", "bold"),
                ("warn", "underline"),
                ("bad", "bold reverse"),
            ],
        )
    }

    /// Add (or replace) the named style `name`, for markup: `[name]…[/]`.
    pub fn style(mut self, name: &str, style: Style) -> Theme {
        self.styles.retain(|(n, _)| n != name);
        self.styles.push((name.to_string(), style));
        self
    }

    pub(crate) fn border(&self, focused: bool) -> Style {
        if focused {
            self.border_focused.clone()
        } else {
            self.border.clone()
        }
    }

    /// The theme as rich styles, for the console's markup.
    fn rich(&self) -> rich::Theme {
        let own = [
            ("border".to_string(), self.border.clone()),
            ("border.focused".to_string(), self.border_focused.clone()),
            ("title".to_string(), self.title.clone()),
        ];
        rich::Theme::from_styles(own.into_iter().chain(self.styles.iter().cloned()), false)
            .expect("styles are already parsed")
    }
}

struct Timer {
    every: Duration,
    next: Duration,
    tick: Box<dyn FnMut(&mut Ctx)>,
}

impl Timer {
    /// A zero interval would never move the timer on (and spin the loop),
    /// so the shortest interval is one millisecond.
    fn new(interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) -> Timer {
        let every = interval.max(Duration::from_millis(1));
        Timer {
            every,
            next: every,
            tick: Box::new(tick),
        }
    }
}

/// One screen of the stack.
struct Layer {
    root: Node,
    focus: Option<NodeId>,
    /// A modal's width and height; `None` for a full screen.
    modal: Option<(Size, Size)>,
    /// Where a modal was last drawn.
    drawn: Option<Rect>,
    timers: Vec<Timer>,
}

thread_local! {
    /// Timers [`every`] made while an app is being built.
    static BUILDING: std::cell::RefCell<Option<Vec<Timer>>> = const { std::cell::RefCell::new(None) };
}

/// Call `tick` every `interval` while the app runs: a clock, a poll, an
/// animation. Call it inside [`App::new`]'s closure (or a screen's, see
/// [`Ctx::push`]), next to the signals the timer writes:
///
/// ```
/// use intuituive::prelude::*;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let seconds = signal(0);
///     every(Duration::from_secs(1), move |_| seconds.update(|s| *s += 1));
///     text!("up {seconds}s")
/// });
/// ```
///
/// # Panics
/// Outside `App::new`'s closure or a screen's.
pub fn every(interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) {
    BUILDING.with(|building| {
        building
            .borrow_mut()
            .as_mut()
            .expect("every() is called inside App::new's closure or a screen's")
            .push(Timer::new(interval, tick))
    });
}

/// Counters from the last frame, for tests and benchmarks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Nodes that drew.
    pub drawn: usize,
    /// Bytes written to the terminal.
    pub bytes: usize,
}

/// An intuiTUIve app.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let count = signal(0);
///     column([
///         text!("[b]Count:[/] {count}"),
///         label("[dim]+ adds one · q quits"),
///     ])
///     .on_key("+", move |_| count.update(|c| *c += 1))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["+", "+", "q"], 30, 2).unwrap();
/// assert_eq!(screen[0].trim_end(), "Count: 2");
/// ```
pub struct App {
    runtime: Rc<Runtime>,
    /// The screens, the first at the bottom; never empty.
    layers: Vec<Layer>,
    /// The stack changed: everything draws again.
    restack: bool,
    /// The backend's clock at the top of this turn of the loop.
    now: Duration,
    focus_path: Signal<Vec<NodeId>>,
    theme: Theme,
    console: Option<Console>,
    /// The console the last frame used, for components handling events.
    last_console: Option<Console>,
    stats: FrameStats,
    /// Hand each frame's plain text to the backend (the headless driver
    /// records it); off in a real terminal, where nobody reads it.
    text_frames: bool,
    caret_shown: bool,
    /// The theme changed: a new console, and everything draws again.
    restyled: bool,
    /// Rows of an inline region; `None` for the alternate screen.
    inline: Option<u16>,
    wait_for_tasks: bool,
}

impl App {
    /// Build the app: `build` runs once, creates the app's signals and
    /// returns the root node.
    pub fn new(build: impl FnOnce() -> Node) -> App {
        let runtime = Runtime::new();
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        let (root, focus_path) = runtime.enter(|| {
            let focus_path = signal(Vec::new());
            (build(), focus_path)
        });
        let timers = BUILDING
            .with(|building| building.borrow_mut().take())
            .unwrap_or_default();
        App {
            runtime,
            layers: vec![Layer {
                root,
                focus: None,
                modal: None,
                drawn: None,
                timers,
            }],
            restack: false,
            now: Duration::ZERO,
            focus_path,
            theme: Theme::default(),
            console: None,
            last_console: None,
            stats: FrameStats::default(),
            text_frames: true,
            caret_shown: false,
            restyled: false,
            inline: None,
            wait_for_tasks: false,
        }
    }

    /// Call `tick` every `interval` (a clock, a poll, an animation).
    pub fn every(mut self, interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) -> App {
        self.layers[0].timers.push(Timer::new(interval, tick));
        self
    }

    /// Run inline: in `height` rows below the cursor (fewer if the
    /// terminal is shorter), rather than on the alternate screen. What the
    /// app last showed stays in the scrollback when it ends, like a
    /// command's output. The mouse is left to the terminal, so its
    /// scrollback and selection keep working.
    pub fn inline(mut self, height: u16) -> App {
        self.inline = Some(height.max(1));
        self
    }

    /// Before each event, wait for the [tasks](crate::spawn) in flight to
    /// deliver their results. For tests, which then see a task's result on
    /// the next frame however fast the machine is; off by default.
    pub fn wait_for_tasks(mut self, on: bool) -> App {
        self.wait_for_tasks = on;
        self
    }

    /// Use `theme` (see [`Theme`]).
    pub fn theme(mut self, theme: Theme) -> App {
        self.theme = theme;
        self
    }

    /// Render with `console` (its colour system and options) instead of
    /// one detected from the terminal.
    pub fn console(mut self, console: Console) -> App {
        self.console = Some(console);
        self
    }

    /// Whether to hand each frame's plain text to the backend, as the
    /// headless driver records it (on by default; [`run`](Self::run) turns
    /// it off).
    pub fn text_frames(mut self, on: bool) -> App {
        self.text_frames = on;
        self
    }

    /// A handle other threads use to change the app's state.
    pub fn proxy(&self) -> Proxy {
        self.runtime.proxy()
    }

    /// Counters from the last frame.
    pub fn stats(&self) -> FrameStats {
        self.stats
    }

    /// Run in the terminal, on the alternate screen (or
    /// [inline](Self::inline)), until a handler calls [`Ctx::quit`] or
    /// Ctrl+C is pressed.
    pub fn run(self) -> io::Result<()> {
        let full = self.inline.is_none();
        let mut session = Session::start(SessionOptions {
            alternate_screen: full,
            mouse: full,
            bracketed_paste: true,
            output: Default::default(),
        })?;
        let mut app = self;
        app.text_frames = false;
        app.run_on(&mut session)
    }

    /// Run on `backend`: a terminal session, or the headless driver.
    pub fn run_on(mut self, backend: &mut impl Backend) -> io::Result<()> {
        let mut painter = Painter::new(None);
        if self.inline.is_some() {
            painter = painter.inline();
        }
        let result = self.run_loop(backend, &mut painter);
        // Inline, the cursor goes below the region, where the shell's
        // prompt follows the app's last frame.
        let _ = backend.write(&format!("{}\x1b[0m\x1b[?25h", painter.finish()));
        self.runtime.close();
        result
    }

    /// Run headless at `width` x `height`, pressing `keys` (key names,
    /// one per step), and return the last screen's rows as plain text.
    pub fn render_with(self, keys: &[&str], width: u16, height: u16) -> io::Result<Vec<String>> {
        let mut script = rich_interact::headless::Script::new();
        for key in keys {
            script = script.keys(key);
        }
        let mut backend = rich_interact::headless::Headless::new(script, width, height);
        let record = backend.record();
        self.run_on(&mut backend)?;
        let last = record.borrow().last_frame().to_string();
        Ok(last.lines().map(str::to_string).collect())
    }

    fn console_for(&mut self, width: u16) -> Console {
        let mut console = match &self.console {
            Some(console) => console.clone(),
            None => Console::builder()
                .width(width as usize)
                .color_system(Some(ColorSystem::Truecolor))
                .force_terminal(true)
                .build(),
        };
        console.push_theme(self.theme.rich(), true);
        console
    }

    /// The rows the app draws in: the terminal's, or the inline region's.
    fn region(&self, rows: u16) -> u16 {
        self.inline.map_or(rows, |height| height.min(rows))
    }

    fn top(&self) -> &Layer {
        self.layers.last().expect("an app always has a screen")
    }

    fn top_mut(&mut self) -> &mut Layer {
        self.layers.last_mut().expect("an app always has a screen")
    }

    fn focus(&self) -> Option<NodeId> {
        self.top().focus
    }

    fn run_loop(&mut self, backend: &mut impl Backend, painter: &mut Painter) -> io::Result<()> {
        let (mut width, mut rows) = backend.size();
        let mut console = self.console_for(width);
        self.last_console = Some(console.clone());
        let mut screen = Screen::new(width, self.region(rows));
        painter.set_color_system(console.color_system());
        let mut first = true;
        backend.write("\x1b[?25l")?;
        // Focus what can be focused before the first frame (keyed children
        // appear only once drawn; `keep_focus` catches them after it).
        self.focus_first();
        loop {
            self.now = backend.elapsed();
            let mut cx = self.ctx();
            let runtime = self.runtime.clone();
            runtime.enter(|| {
                // Waiting for tasks, wait again for any a result started.
                loop {
                    if self.wait_for_tasks {
                        self.settle_tasks();
                    }
                    let ran = runtime.run_inbox(&mut cx);
                    let busy = runtime.tasks.load(std::sync::atomic::Ordering::SeqCst) > 0;
                    if !self.wait_for_tasks || !(ran || busy) {
                        break;
                    }
                }
                let now = self.now;
                for layer in &mut self.layers {
                    for timer in &mut layer.timers {
                        if now >= timer.next {
                            (timer.tick)(&mut cx);
                            while timer.next <= now {
                                timer.next += timer.every;
                            }
                        }
                    }
                }
            });
            if self.apply(cx) {
                break;
            }
            if self.restyled {
                self.restyled = false;
                console = self.console_for(width);
                self.last_console = Some(console.clone());
                first = true;
            }
            if first || self.restack || self.runtime.has_dirty() {
                self.paint(backend, painter, &console, &mut screen, first)?;
                first = false;
                // Keyed children exist once drawn: focus is chosen after a
                // frame, and chosen again if the focused node has gone.
                self.keep_focus();
            }
            let wait = self
                .layers
                .iter()
                .flat_map(|layer| &layer.timers)
                .map(|t| t.next.saturating_sub(backend.elapsed()))
                .min()
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(50));
            let Some(event) = backend.read(Some(wait))? else {
                continue;
            };
            match event {
                Event::Key(key) if key == Key::ctrl('c') => break,
                Event::Resize {
                    columns,
                    rows: new_rows,
                } => {
                    (width, rows) = (columns, new_rows);
                    console = self.console_for(width);
                    self.last_console = Some(console.clone());
                    screen = Screen::new(width, self.region(rows));
                    painter.invalidate();
                    first = true;
                }
                Event::Key(key) => {
                    if self.key(key) {
                        break;
                    }
                }
                Event::Paste(_) => {
                    if let Some(true) = self.give_to_host(&event) {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    // Inline, the region starts where the session began,
                    // or higher if it had to scroll to fit.
                    let top = if self.inline.is_some() {
                        backend
                            .origin()
                            .min(rows.saturating_sub(screen.area().height))
                    } else {
                        0
                    };
                    if let (MouseKind::Down(button), Some(row)) =
                        (mouse.kind, mouse.row.checked_sub(top))
                    {
                        if self.click(mouse.column, row, button) {
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        // Inline, the last frame stays in the scrollback: show what the
        // last handler changed before leaving.
        if self.inline.is_some() && (self.restack || self.runtime.has_dirty()) {
            self.paint(backend, painter, &console, &mut screen, false)?;
        }
        Ok(())
    }

    /// Draw what changed and send it.
    fn paint(
        &mut self,
        backend: &mut impl Backend,
        painter: &mut Painter,
        console: &Console,
        screen: &mut Screen,
        full: bool,
    ) -> io::Result<()> {
        let damage = self.frame(console, screen, full);
        let mut out = painter.paint(screen, &damage);
        out.push_str(&self.caret(painter));
        self.stats.bytes = out.len();
        if !out.is_empty() {
            backend.write(&out)?;
        }
        if self.text_frames {
            backend.painted(&screen.plain().join("\n"));
        }
        Ok(())
    }

    /// Wait (a while at most) until every task in flight has sent its
    /// result.
    fn settle_tasks(&self) {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while self.runtime.tasks.load(std::sync::atomic::Ordering::SeqCst) > 0
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Show the focused component's text caret, or keep the cursor hidden.
    fn caret(&mut self, painter: &mut Painter) -> String {
        let caret = self.focus().and_then(|id| {
            let mut caret = None;
            with_node(&self.top().root, id, &mut |node| caret = node.caret.get());
            caret
        });
        let out = match caret {
            Some((x, y)) => format!("{}\x1b[?25h", painter.move_to(x, y)),
            None if self.caret_shown => "\x1b[?25l".to_string(),
            None => String::new(),
        };
        self.caret_shown = caret.is_some();
        out
    }

    /// Draw what changed; returns the damage.
    fn frame(&mut self, console: &Console, screen: &mut Screen, full: bool) -> Vec<Rect> {
        let area = screen.area();
        // Draw from the topmost full screen up: the modals over it, each
        // drawn again whenever what is below it changed under it.
        let base = self
            .layers
            .iter()
            .rposition(|layer| layer.modal.is_none())
            .unwrap_or(0);
        let runtime = self.runtime.clone();
        let rects: Vec<Rect> = runtime.enter(|| {
            self.layers[base..]
                .iter()
                .map(|layer| match layer.modal {
                    None => area,
                    Some((width, height)) => modal_rect(console, &layer.root, area, width, height),
                })
                .collect()
        });
        // A modal that moved or changed size uncovered cells only the
        // screens below can draw: everything draws again.
        let moved = self.layers[base..]
            .iter()
            .zip(&rects)
            .any(|(layer, rect)| layer.modal.is_some() && layer.drawn.is_some_and(|d| d != *rect));
        let full = full || moved || std::mem::take(&mut self.restack);
        let mut frame = FrameState {
            console,
            runtime: &self.runtime,
            dirty: self.runtime.take_dirty(),
            damage: Vec::new(),
            focus_path: self.focus_path,
            theme: &self.theme,
            drawn: 0,
        };
        if full {
            // A screen opened or closed: nothing on screen can be trusted.
            screen.clear(area);
            frame.damage.push(area);
        }
        runtime.enter(|| {
            for (layer, rect) in self.layers[base..].iter_mut().zip(&rects) {
                let force = match layer.modal {
                    None => full,
                    Some(_) => {
                        let under = frame
                            .damage
                            .iter()
                            .any(|d| !d.intersection(*rect).is_empty());
                        if under {
                            screen.clear(*rect);
                            frame.damage.push(*rect);
                        }
                        full || under
                    }
                };
                layer.root.draw(&mut frame, *rect, screen, force);
                layer.drawn = Some(*rect);
            }
        });
        self.stats.drawn = frame.drawn;
        frame.damage
    }

    fn ctx(&self) -> Ctx {
        Ctx::new(self.runtime.proxy())
    }

    /// Apply what a handler asked for; whether to quit.
    fn apply(&mut self, cx: Ctx) -> bool {
        for nav in cx.nav {
            self.navigate(nav);
        }
        if let Some(theme) = cx.theme {
            self.theme = theme;
            self.restyled = true;
        }
        match cx.focus {
            Some(FocusMove::Next) => self.move_focus(true),
            Some(FocusMove::Previous) => self.move_focus(false),
            Some(FocusMove::To(id)) => self.set_focus(Some(id)),
            None => {}
        }
        cx.quit
    }

    fn navigate(&mut self, nav: Nav) {
        match nav {
            Nav::Push(build) => self.open(build, None),
            Nav::Modal(width, height, build) => self.open(build, Some((width, height))),
            Nav::Pop => {
                if self.layers.len() > 1 {
                    self.close();
                    let focus = self.focus();
                    self.set_focus(focus);
                }
            }
            Nav::Replace(build) => {
                let modal = self.top().modal;
                if self.layers.len() > 1 {
                    self.close();
                    self.open(build, modal);
                } else {
                    // The first screen: its timers go with it.
                    let old = self.layers.pop().expect("an app always has a screen");
                    old.root.forget(&self.runtime);
                    self.open(build, None);
                }
            }
        }
    }

    /// Build a screen and put it on top, focusing its first focusable node.
    fn open(&mut self, build: Build, modal: Option<(Size, Size)>) {
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        let runtime = self.runtime.clone();
        let root = runtime.enter(build);
        let mut timers = BUILDING
            .with(|building| building.borrow_mut().take())
            .unwrap_or_default();
        // A screen's timers start counting when it opens.
        for timer in &mut timers {
            timer.next = self.now + timer.every;
        }
        self.layers.push(Layer {
            root,
            focus: None,
            modal,
            drawn: None,
            timers,
        });
        self.restack = true;
        self.set_focus(None);
        self.focus_first();
    }

    /// Take the top screen off, forgetting its subscriptions.
    fn close(&mut self) {
        let layer = self.layers.pop().expect("an app always has a screen");
        layer.root.forget(&self.runtime);
        self.restack = true;
    }

    /// The path from the root to the focused node.
    fn path_to(&self, target: NodeId) -> Vec<NodeId> {
        let mut found = Vec::new();
        self.top().root.walk(&mut |node, path| {
            if node.id == target {
                found = path.to_vec();
            }
        });
        found
    }

    fn focusable(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.top().root.walk(&mut |node, _| {
            if node.focusable {
                ids.push(node.id);
            }
        });
        ids
    }

    /// Focus the first focusable node if nothing has the focus, or if the
    /// focused node left the tree (a keyed child that was removed), so keys
    /// always have a path to the root.
    fn keep_focus(&mut self) {
        if let Some(id) = self.focus() {
            if self.path_to(id).is_empty() {
                self.set_focus(None);
            }
        }
        if self.focus().is_none() {
            self.focus_first();
        }
    }

    fn focus_first(&mut self) {
        if let Some(&first) = self.focusable().first() {
            self.set_focus(Some(first));
        }
    }

    fn set_focus(&mut self, id: Option<NodeId>) {
        self.top_mut().focus = id;
        let path = id.map(|id| self.path_to(id)).unwrap_or_default();
        let runtime = self.runtime.clone();
        runtime.enter(|| self.focus_path.set(path));
    }

    fn move_focus(&mut self, forward: bool) {
        let ids = self.focusable();
        if ids.is_empty() {
            return;
        }
        let at = self
            .focus()
            .and_then(|f| ids.iter().position(|id| *id == f));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => ids.len() - 1,
            (Some(i), true) => (i + 1) % ids.len(),
            (Some(i), false) => (i + ids.len() - 1) % ids.len(),
        };
        self.set_focus(Some(ids[next]));
    }

    /// Route a key: from the focused node up through its ancestors to the
    /// root; Tab and Shift+Tab move the focus if no binding used them.
    /// Whether to quit.
    fn key(&mut self, key: Key) -> bool {
        // A focused component sees the key first.
        if let Some(quit) = self.give_to_host(&Event::Key(key)) {
            return quit;
        }
        let path = match self.focus().map(|id| self.path_to(id)) {
            Some(path) if !path.is_empty() => path,
            _ => vec![self.top().root.id],
        };
        for &id in path.iter().rev() {
            let mut cx = self.ctx();
            let mut used = false;
            let runtime = self.runtime.clone();
            runtime.enter(|| {
                with_node(&self.top().root, id, &mut |node| {
                    let mut keys = node.keys.borrow_mut();
                    if let Some((_, _, handler)) =
                        keys.iter_mut().find(|(keys, _, _)| keys.contains(&key))
                    {
                        handler(&mut cx);
                        used = true;
                    }
                });
            });
            if used {
                return self.apply(cx);
            }
        }
        match key.code {
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            _ => {}
        }
        false
    }

    /// Give `event` to the focused node if it hosts a component. `Some`
    /// (whether to quit) if the component used it.
    fn give_to_host(&mut self, event: &Event) -> Option<bool> {
        let id = self.focus()?;
        let console = self.last_console.clone()?;
        let mut cx = self.ctx();
        let mut used = false;
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                if let crate::node::Kind::Host(host) = &mut *node.kind.borrow_mut() {
                    let rect = node.rect();
                    let context = rich_interact::Context {
                        console: &console,
                        width: rect.width as usize,
                        height: rect.height as usize,
                    };
                    used = matches!(
                        host.handle(event, &context, &mut cx),
                        crate::node::Used::Yes
                    );
                }
            });
        });
        if !used {
            return None;
        }
        self.runtime.mark_dirty(id);
        Some(self.apply(cx))
    }

    /// Route a button press to the deepest node under the pointer that
    /// takes it: a component gets every button, in its own coordinates; an
    /// [`on_click`](crate::Node::on_click) handler only the left one.
    fn click(&mut self, column: u16, row: u16, button: Button) -> bool {
        let left = button == Button::Left;
        let mut target = None;
        self.top().root.walk(&mut |node, _| {
            let host = matches!(&*node.kind.borrow(), crate::node::Kind::Host(_));
            let clickable = left && node.click.borrow().is_some();
            if (host || clickable) && node.rect().contains(column, row) {
                target = Some((node.id, node.rect()));
            }
        });
        let Some((id, rect)) = target else {
            return false;
        };
        self.set_focus(Some(id));
        // A component gets the click in its own coordinates.
        let local =
            rich_interact::Mouse::new(MouseKind::Down(button), column - rect.x, row - rect.y);
        if let Some(quit) = self.give_to_host(&Event::Mouse(local)) {
            return quit;
        }
        if !left {
            return false;
        }
        let mut cx = self.ctx();
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                if let Some(handler) = node.click.borrow_mut().as_mut() {
                    handler(&mut cx);
                }
            });
        });
        self.apply(cx)
    }
}

/// Where a modal `width` x `height` goes: centred in `area`.
fn modal_rect(console: &Console, root: &Node, area: Rect, width: Size, height: Size) -> Rect {
    let resolve = |size: Size, total: u16, measure: &dyn Fn() -> u16| -> u16 {
        match size {
            Size::Fixed(n) => n,
            Size::Percent(p) => (total as u32 * p.min(100) as u32 / 100) as u16,
            Size::Auto => measure(),
            Size::Flex(_) => total,
        }
        .min(total)
    };
    let w = resolve(width, area.width, &|| {
        root.measure(console, Axis::Horizontal, area.width, area.height)
    });
    let h = resolve(height, area.height, &|| {
        root.measure(console, Axis::Vertical, w, 0)
    });
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

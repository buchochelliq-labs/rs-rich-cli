//! The app: the tree, its runtime, and the loop that runs them.
//!
//! [`App::run`] takes the terminal (the alternate screen, raw mode, the
//! mouse), and gives it back on every way out, including a panic. Each turn
//! of the loop it runs work other threads sent through a
//! [`Proxy`](crate::Proxy), fires due timers, draws what changed and reads
//! one event:
//!
//! - **keys** go to the focused node, then bubble to each ancestor until a
//!   binding uses them; Tab and Shift+Tab move the focus through the
//!   [focusable](crate::Node::focusable) nodes; Ctrl+C ends the app;
//! - **clicks** go to the deepest clickable node under the pointer, found
//!   from the rectangles the last frame laid out, and focus it;
//! - **resizes** lay everything out again.
//!
//! [`App::run_on`] runs on any [`Backend`], including `rich-interact`'s
//! headless driver, so tests script keys and read frames without a
//! terminal.

use std::io;
use std::rc::Rc;
use std::time::Duration;

use rich::{ColorSystem, Console, Style};
use rich_interact::{Backend, Event, Key, KeyCode, MouseKind, Session, SessionOptions};

use crate::node::{with_node, FrameState, Node};
use crate::reactive::{signal, NodeId, Proxy, Runtime, Signal};
use crate::screen::{Painter, Screen};

/// What a key or click handler can do.
pub struct Ctx {
    quit: bool,
    focus: Option<FocusMove>,
    proxy: Proxy,
}

enum FocusMove {
    Next,
    Previous,
    To(NodeId),
}

impl Ctx {
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

/// The app's look, for the parts the framework draws itself.
#[derive(Clone, Debug)]
pub struct Theme {
    pub border: Style,
    pub border_focused: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            border: Style::parse("blue").unwrap(),
            border_focused: Style::parse("bright_cyan").unwrap(),
        }
    }
}

impl Theme {
    pub(crate) fn border(&self, focused: bool) -> Style {
        if focused {
            self.border_focused.clone()
        } else {
            self.border.clone()
        }
    }
}

struct Timer {
    every: Duration,
    next: Duration,
    tick: Box<dyn FnMut(&mut Ctx)>,
}

thread_local! {
    /// Timers [`every`] made while an app is being built.
    static BUILDING: std::cell::RefCell<Option<Vec<Timer>>> = const { std::cell::RefCell::new(None) };
}

/// Call `tick` every `interval` while the app runs: a clock, a poll, an
/// animation. Call it inside [`App::new`]'s closure, next to the signals
/// the timer writes:
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
/// Outside `App::new`'s closure.
pub fn every(interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) {
    BUILDING.with(|building| {
        building
            .borrow_mut()
            .as_mut()
            .expect("every() is called inside App::new's closure")
            .push(Timer {
                every: interval,
                next: interval,
                tick: Box::new(tick),
            })
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
    root: Node,
    focus: Option<NodeId>,
    focus_path: Signal<Vec<NodeId>>,
    theme: Theme,
    timers: Vec<Timer>,
    console: Option<Console>,
    /// The console the last frame used, for components handling events.
    last_console: Option<Console>,
    stats: FrameStats,
    /// Hand each frame's plain text to the backend (the headless driver
    /// records it); off in a real terminal, where nobody reads it.
    text_frames: bool,
    caret_shown: bool,
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
            root,
            focus: None,
            focus_path,
            theme: Theme::default(),
            timers,
            console: None,
            last_console: None,
            stats: FrameStats::default(),
            text_frames: true,
            caret_shown: false,
        }
    }

    /// Call `tick` every `interval` (a clock, a poll, an animation).
    pub fn every(mut self, interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) -> App {
        self.timers.push(Timer {
            every: interval,
            next: interval,
            tick: Box::new(tick),
        });
        self
    }

    /// Use `theme` for borders and focus.
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

    /// Run in the terminal, on the alternate screen, until a handler calls
    /// [`Ctx::quit`] or Ctrl+C is pressed.
    pub fn run(self) -> io::Result<()> {
        let mut session = Session::start(SessionOptions {
            alternate_screen: true,
            mouse: true,
            bracketed_paste: true,
            output: Default::default(),
        })?;
        let mut app = self;
        app.text_frames = false;
        app.run_on(&mut session)
    }

    /// Run on `backend`: a terminal session, or the headless driver.
    pub fn run_on(mut self, backend: &mut impl Backend) -> io::Result<()> {
        let result = self.run_loop(backend);
        let _ = backend.write("\x1b[0m\x1b[?25h");
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
        match &self.console {
            Some(console) => console.clone(),
            None => Console::builder()
                .width(width as usize)
                .color_system(Some(ColorSystem::Truecolor))
                .force_terminal(true)
                .build(),
        }
    }

    fn run_loop(&mut self, backend: &mut impl Backend) -> io::Result<()> {
        let (mut width, mut height) = backend.size();
        let mut console = self.console_for(width);
        self.last_console = Some(console.clone());
        let mut screen = Screen::new(width, height);
        let mut painter = Painter::new(console.color_system());
        let mut first = true;
        backend.write("\x1b[?25l")?;
        self.focus_first();
        loop {
            let mut cx = self.ctx();
            let runtime = self.runtime.clone();
            runtime.enter(|| {
                runtime.run_inbox();
                let now = backend.elapsed();
                for timer in &mut self.timers {
                    if now >= timer.next {
                        (timer.tick)(&mut cx);
                        while timer.next <= now {
                            timer.next += timer.every;
                        }
                    }
                }
            });
            if self.apply(cx) {
                return Ok(());
            }
            if first || self.runtime.has_dirty() {
                let damage = self.frame(&console, &mut screen, first);
                let mut out = painter.paint(&screen, &damage);
                out.push_str(&self.caret());
                self.stats.bytes = out.len();
                if !out.is_empty() {
                    backend.write(&out)?;
                }
                if self.text_frames {
                    backend.painted(&screen.plain().join("\n"));
                }
                first = false;
            }
            let wait = self
                .timers
                .iter()
                .map(|t| t.next.saturating_sub(backend.elapsed()))
                .min()
                .unwrap_or(Duration::from_millis(50))
                .min(Duration::from_millis(50));
            let Some(event) = backend.read(Some(wait))? else {
                continue;
            };
            match event {
                Event::Key(key) if key == Key::ctrl('c') => return Ok(()),
                Event::Resize { columns, rows } => {
                    (width, height) = (columns, rows);
                    console = self.console_for(width);
                    self.last_console = Some(console.clone());
                    screen = Screen::new(width, height);
                    painter.invalidate();
                    first = true;
                }
                Event::Key(key) => {
                    if self.key(key) {
                        return Ok(());
                    }
                }
                Event::Paste(_) => {
                    if let Some(true) = self.to_host(&event) {
                        return Ok(());
                    }
                }
                Event::Mouse(mouse) => {
                    if let MouseKind::Down(_) = mouse.kind {
                        if self.click(mouse.column, mouse.row) {
                            return Ok(());
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Show the focused component's text caret, or keep the cursor hidden.
    fn caret(&mut self) -> String {
        let caret = self.focus.and_then(|id| {
            let mut caret = None;
            with_node(&self.root, id, &mut |node| caret = node.caret.get());
            caret
        });
        let out = match caret {
            Some((x, y)) => format!("\x1b[{};{}H\x1b[?25h", y + 1, x + 1),
            None if self.caret_shown => "\x1b[?25l".to_string(),
            None => String::new(),
        };
        self.caret_shown = caret.is_some();
        out
    }

    /// Draw what changed; returns the damage.
    fn frame(
        &mut self,
        console: &Console,
        screen: &mut Screen,
        full: bool,
    ) -> Vec<crate::screen::Rect> {
        let mut frame = FrameState {
            console,
            runtime: &self.runtime,
            dirty: self.runtime.take_dirty(),
            damage: Vec::new(),
            focus_path: self.focus_path,
            theme: &self.theme,
            drawn: 0,
        };
        let area = screen.area();
        let runtime = self.runtime.clone();
        runtime.enter(|| self.root.draw(&mut frame, area, screen, full));
        self.stats.drawn = frame.drawn;
        frame.damage
    }

    fn ctx(&self) -> Ctx {
        Ctx {
            quit: false,
            focus: None,
            proxy: self.runtime.proxy(),
        }
    }

    /// Apply what a handler asked for; whether to quit.
    fn apply(&mut self, cx: Ctx) -> bool {
        match cx.focus {
            Some(FocusMove::Next) => self.move_focus(true),
            Some(FocusMove::Previous) => self.move_focus(false),
            Some(FocusMove::To(id)) => self.set_focus(Some(id)),
            None => {}
        }
        cx.quit
    }

    /// The path from the root to the focused node.
    fn path_to(&self, target: NodeId) -> Vec<NodeId> {
        let mut found = Vec::new();
        self.root.walk(&mut |node, path| {
            if node.id == target {
                found = path.to_vec();
            }
        });
        found
    }

    fn focusable(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.root.walk(&mut |node, _| {
            if node.focusable {
                ids.push(node.id);
            }
        });
        ids
    }

    fn focus_first(&mut self) {
        if let Some(&first) = self.focusable().first() {
            self.set_focus(Some(first));
        }
    }

    fn set_focus(&mut self, id: Option<NodeId>) {
        self.focus = id;
        let path = id.map(|id| self.path_to(id)).unwrap_or_default();
        let runtime = self.runtime.clone();
        runtime.enter(|| self.focus_path.set(path));
    }

    fn move_focus(&mut self, forward: bool) {
        let ids = self.focusable();
        if ids.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| ids.iter().position(|id| *id == f));
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
        if let Some(quit) = self.to_host(&Event::Key(key)) {
            return quit;
        }
        let path = match self.focus {
            Some(id) => self.path_to(id),
            None => vec![self.root.id],
        };
        for &id in path.iter().rev() {
            let mut cx = self.ctx();
            let mut used = false;
            let runtime = self.runtime.clone();
            runtime.enter(|| {
                with_node(&self.root, id, &mut |node| {
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
    fn to_host(&mut self, event: &Event) -> Option<bool> {
        let id = self.focus?;
        let console = self.last_console.clone()?;
        let mut cx = self.ctx();
        let mut used = false;
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.root, id, &mut |node| {
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

    /// Route a click to the deepest clickable node under the pointer.
    fn click(&mut self, column: u16, row: u16) -> bool {
        let mut target = None;
        self.root.walk(&mut |node, _| {
            let host = matches!(&*node.kind.borrow(), crate::node::Kind::Host(_));
            if (host || node.click.borrow().is_some()) && node.rect().contains(column, row) {
                target = Some((node.id, node.rect()));
            }
        });
        let Some((id, rect)) = target else {
            return false;
        };
        self.set_focus(Some(id));
        // A component gets the click in its own coordinates.
        let local = rich_interact::Mouse::new(
            rich_interact::MouseKind::Down(rich_interact::Button::Left),
            column - rect.x,
            row - rect.y,
        );
        if let Some(quit) = self.to_host(&Event::Mouse(local)) {
            return quit;
        }
        let mut cx = self.ctx();
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.root, id, &mut |node| {
                if let Some(handler) = node.click.borrow_mut().as_mut() {
                    handler(&mut cx);
                }
            });
        });
        self.apply(cx)
    }
}

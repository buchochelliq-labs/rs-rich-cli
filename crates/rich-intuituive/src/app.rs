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

use std::collections::HashMap;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use rich::{ColorSystem, Console, Style};
use rich_interact::{Backend, Button, Event, Key, KeyCode, MouseKind, Session, SessionOptions};

use crate::inspect::Inspector;
use crate::layout::Size;
use crate::node::{with_node, Axis, FrameState, Node};
use crate::reactive::{signal, NodeId, Proxy, Runtime, Signal};
use crate::screen::{Painter, Rect, Screen};
use crate::widget::{Used, WidgetEvent};

/// What a key or click handler can do.
pub struct Ctx {
    quit: bool,
    focus: Option<FocusMove>,
    proxy: Proxy,
    nav: Vec<Nav>,
    theme: Option<Theme>,
    toasts: Vec<(String, Duration)>,
    animations: Vec<(Signal<f64>, f64, Duration, Easing)>,
    copies: Vec<String>,
    /// Where the mouse event being handled happened, on the screen.
    pub(crate) pointer: Option<(u16, u16)>,
}

/// What a [pop-up](Ctx::popup) is placed next to: a node (by its
/// [`id`](crate::Node::id)) or a rectangle of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    Node(NodeId),
    Rect(Rect),
}

impl From<NodeId> for Anchor {
    fn from(id: NodeId) -> Anchor {
        Anchor::Node(id)
    }
}

impl From<Rect> for Anchor {
    fn from(rect: Rect) -> Anchor {
        Anchor::Rect(rect)
    }
}

/// How an [animation](Ctx::animate) moves between its two values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    #[default]
    EaseInOut,
}

impl Easing {
    /// The share of the way at time `t` (0 to 1).
    pub fn at(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => 1.0 - (1.0 - t).powi(3),
            Easing::EaseInOut if t < 0.5 => 4.0 * t * t * t,
            Easing::EaseInOut => 1.0 - (-2.0 * t + 2.0).powi(3) / 2.0,
        }
    }
}

enum FocusMove {
    Next,
    Previous,
    To(NodeId),
}

type Build = Box<dyn FnOnce() -> Node>;

/// Where a [pop-up](Ctx::popup) goes, next to its anchor. It flips to the
/// other side when there is no room.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    Below,
    Above,
    Right,
    Left,
}

/// A change to the stack of screens.
enum Nav {
    Push(Build),
    Modal(Size, Size, Build),
    Popup(Anchor, Placement, Size, Size, Build),
    Pop,
    Replace(Build),
    /// The command palette, or the help, from the focused node's bindings.
    Palette,
    Help,
    /// Run binding `index` of node `id`, as its key would.
    Run(NodeId, usize),
}

impl Ctx {
    pub(crate) fn new(proxy: Proxy) -> Ctx {
        Ctx {
            quit: false,
            toasts: Vec::new(),
            animations: Vec::new(),
            copies: Vec::new(),
            pointer: None,
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

    /// Open a pop-up: `build`'s node in a box `width` x `height` next to the
    /// node `anchor` (a dropdown under a field, a menu beside a button),
    /// flipped to the other side when it would leave the screen. It takes
    /// the keys like a modal; Esc that nothing used, or a press outside it,
    /// closes it.
    pub fn popup(
        &mut self,
        anchor: impl Into<Anchor>,
        placement: Placement,
        width: Size,
        height: Size,
        build: impl FnOnce() -> Node + 'static,
    ) {
        self.nav.push(Nav::Popup(
            anchor.into(),
            placement,
            width,
            height,
            Box::new(build),
        ));
    }

    /// Open the command palette: every binding made with
    /// [`bind`](crate::Node::bind) (with a description) on the focused node
    /// and its ancestors, searched by name; the one picked runs as its key
    /// would. [`App::palette_key`] opens it from a key.
    pub fn palette(&mut self) {
        self.nav.push(Nav::Palette);
    }

    /// The old name of [`palette`](Self::palette).
    #[deprecated(since = "0.0.3", note = "renamed to `palette`, to pair with `help`")]
    pub fn command_palette(&mut self) {
        self.palette();
    }

    /// Open the help: the same bindings, with their keys, searchable.
    /// [`App::help_key`] opens it from a key.
    pub fn help(&mut self) {
        self.nav.push(Nav::Help);
    }

    /// Put `text` on the clipboard, where the terminal lets an app (OSC 52)
    /// or the system's clipboard is reachable; a toast says so once it is
    /// there. A loop of your own gets it from
    /// [`Driver::take_copies`](crate::Driver::take_copies).
    pub fn copy(&mut self, text: impl Into<String>) {
        self.copies.push(text.into());
    }

    /// Show `markup` in a toast at the bottom right for three seconds.
    pub fn toast(&mut self, markup: impl Into<String>) {
        self.toast_for(markup, Duration::from_secs(3));
    }

    /// Show `markup` in a toast for `duration`.
    pub fn toast_for(&mut self, markup: impl Into<String>, duration: Duration) {
        self.toasts.push((markup.into(), duration));
    }

    /// Move `value` to `to` over `duration`, eased: the signal is set on
    /// every frame until it gets there, so whatever reads it moves. A new
    /// animation of the same signal takes over from where it is.
    pub fn animate(&mut self, value: Signal<f64>, to: f64, duration: Duration, easing: Easing) {
        self.animations.push((value, to, duration, easing));
    }

    /// Where the mouse event being handled happened, on the screen: for a
    /// context menu or a pop-up at the pointer. `None` outside a mouse
    /// handler.
    pub fn pointer(&self) -> Option<(u16, u16)> {
        self.pointer
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
/// define `accent`, `muted`, `good`, `warn`, `bad` and `selected` (a
/// [`list`](crate::list)'s selected row), and the framework's
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
    fn preset(border: &str, focused: &str, names: [(&str, &str); 6]) -> Theme {
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
                ("selected", "reverse"),
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
                ("selected", "reverse"),
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
                ("selected", "reverse"),
            ],
        )
    }

    /// Add (or replace) the named style `name`, for markup: `[name]…[/]`.
    pub fn style(mut self, name: &str, style: Style) -> Theme {
        self.styles.retain(|(n, _)| n != name);
        self.styles.push((name.to_string(), style));
        self
    }

    /// This theme with the styles of a theme file's contents over it: rich's
    /// theme format, a `[styles]` section of `name = style` lines. `border`,
    /// `border.focused` and `title` set the framework's parts; other names
    /// are added (or replaced) for markup.
    ///
    /// ```
    /// use intuituive::Theme;
    ///
    /// let theme = Theme::dark()
    ///     .with_config("[styles]\naccent = bold magenta\nborder = green\n")
    ///     .unwrap();
    /// assert_eq!(theme.border, rich::Style::parse("green").unwrap());
    /// assert!(Theme::dark().with_config("[styles]\nbad = not-a-colour\n").is_err());
    /// ```
    pub fn with_config(mut self, config: &str) -> Result<Theme, String> {
        let parsed = rich::Theme::from_file(config, false).map_err(|e| e.to_string())?;
        let mut names: Vec<&str> = parsed.names().collect();
        names.sort_unstable();
        for name in names {
            let style = parsed.get(name).cloned().unwrap_or_default();
            match name {
                "border" => self.border = style,
                "border.focused" => self.border_focused = style,
                "title" => self.title = style,
                _ => self = self.style(name, style),
            }
        }
        Ok(self)
    }

    /// The dark theme with a theme file's styles over it (see
    /// [`with_config`](Self::with_config)).
    pub fn load(path: impl AsRef<std::path::Path>) -> Result<Theme, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        Theme::dark().with_config(&text)
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

/// An animation running (see [`Ctx::animate`]).
struct Animation {
    value: Signal<f64>,
    from: f64,
    to: f64,
    start: Duration,
    duration: Duration,
    easing: Easing,
}

/// One screen of the stack.
struct Layer {
    root: Node,
    focus: Option<NodeId>,
    /// A modal's width and height; `None` for a full screen.
    modal: Option<(Size, Size)>,
    /// Where a modal was last drawn.
    drawn: Option<Rect>,
    /// For a pop-up: what it is next to, and on which side.
    anchor: Option<(Anchor, Placement)>,
    timers: Vec<Timer>,
    /// Its [watches](crate::watch), stopped when it closes.
    watches: Vec<usize>,
    /// Its focusable nodes, in Tab order, at the last frame: where the
    /// focus goes when the focused node leaves the tree.
    order: Vec<NodeId>,
    /// The focus went to the first focusable node because no node asked
    /// for it; an `autofocus` node built later takes it over.
    fallback: bool,
}

thread_local! {
    /// Timers [`every`] made while an app is being built.
    static BUILDING: std::cell::RefCell<Option<Vec<Timer>>> = const { std::cell::RefCell::new(None) };
    /// Watches [`watch`](crate::watch) made while a screen is being built.
    static WATCHES: std::cell::RefCell<Option<Vec<usize>>> = const { std::cell::RefCell::new(None) };
}

/// Note a watch made while a screen is built, so it stops with the screen.
pub(crate) fn building_watch(id: usize) {
    WATCHES.with(|watches| {
        if let Some(watches) = watches.borrow_mut().as_mut() {
            watches.push(id);
        }
    });
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
    /// Nodes left dirty by their own drawing, and for how many frames in a
    /// row: one that keeps at it is quietened (see `Driver::render`).
    restless: HashMap<NodeId, u32>,
    /// The backend's clock at the top of this turn of the loop.
    now: Duration,
    focus_path: Signal<Vec<NodeId>>,
    /// The nodes under the mouse pointer, outermost first.
    hover_path: Signal<Vec<NodeId>>,
    /// The node that captured the mouse (a drag in progress).
    capture: Option<(NodeId, (i32, i32))>,
    /// Pointer movement reports: (wanted by a widget, turned on).
    motion: (bool, bool),
    /// Toasts showing, with when each goes.
    toasts: Vec<(String, Duration)>,
    /// Animations running: the signal, from, to, start, length, easing.
    animations: Vec<Animation>,
    /// Keys that open the command palette and the help, if any.
    palette_key: Vec<Key>,
    help_key: Vec<Key>,
    /// Read keys as a legacy terminal sends them, even where the kitty
    /// keyboard protocol could be on.
    legacy_keys: bool,
    /// Selecting text with the mouse (on unless turned off): where the
    /// press was and where the drag is now.
    selectable: bool,
    selection: Option<((u16, u16), (u16, u16))>,
    /// A drag is selecting.
    selecting: bool,
    /// Where the mouse event being routed happened.
    pointer: Option<(u16, u16)>,
    /// Where the pointer last was, for widgets that ask.
    last_pointer: Option<(u16, u16)>,
    /// The node last told it has the focus, on whichever screen.
    focused: Option<NodeId>,
    /// Focus, hover and resize events waiting to be told to their widgets.
    lifecycle: Vec<(NodeId, WidgetEvent)>,
    /// Whether text selected with the mouse can be copied, and what was
    /// copied since the loop last asked.
    clipboard: bool,
    copies: Vec<String>,
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
    /// `Some` when the inspector can be shown (F12 toggles it).
    inspector: Option<Inspector>,
    /// The theme set in code, before any theme file's styles.
    theme_base: Theme,
    theme_file: Option<ThemeFile>,
    /// Draw a frame even if no node is dirty (the inspector has news).
    poke: bool,
}

/// A theme file the app reloads when it changes.
struct ThemeFile {
    path: std::path::PathBuf,
    /// The modification time and length last read.
    stamp: Option<(std::time::SystemTime, u64)>,
    /// The last contents that parsed.
    good: Option<String>,
    /// The contents last read, parsed or not.
    read: Option<String>,
    error: Option<String>,
}

impl App {
    /// Build the app: `build` runs once, creates the app's signals and
    /// returns the root node.
    pub fn new(build: impl FnOnce() -> Node) -> App {
        let runtime = Runtime::new();
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        WATCHES.with(|watches| *watches.borrow_mut() = Some(Vec::new()));
        let (root, focus_path, hover_path) = runtime.enter(|| {
            let focus_path = signal(Vec::new());
            let hover_path = signal(Vec::new());
            (build(), focus_path, hover_path)
        });
        let timers = BUILDING
            .with(|building| building.borrow_mut().take())
            .unwrap_or_default();
        let watches = WATCHES
            .with(|watches| watches.borrow_mut().take())
            .unwrap_or_default();
        App {
            runtime,
            layers: vec![Layer {
                root,
                focus: None,
                modal: None,
                drawn: None,
                anchor: None,
                timers,
                watches,
                order: Vec::new(),
                fallback: false,
            }],
            restack: false,
            restless: HashMap::new(),
            now: Duration::ZERO,
            focus_path,
            hover_path,
            capture: None,
            motion: (false, false),
            last_pointer: None,
            focused: None,
            lifecycle: Vec::new(),
            clipboard: false,
            copies: Vec::new(),
            toasts: Vec::new(),
            animations: Vec::new(),
            palette_key: Vec::new(),
            help_key: Vec::new(),
            legacy_keys: false,
            selectable: true,
            selection: None,
            selecting: false,
            pointer: None,
            theme: Theme::default(),
            console: None,
            last_console: None,
            stats: FrameStats::default(),
            text_frames: true,
            caret_shown: false,
            restyled: false,
            inline: None,
            wait_for_tasks: false,
            inspector: std::env::var("INTUITUIVE_INSPECT")
                .ok()
                .filter(|v| !v.is_empty() && v != "0")
                .map(|_| Inspector::new(true)),
            theme_base: Theme::default(),
            theme_file: None,
            poke: false,
        }
    }

    /// Dock the [inspector](crate::inspect) on the right: the node tree,
    /// what drew in the last frame, the focus and the frame's cost. F12
    /// shows and hides it. Setting `INTUITUIVE_INSPECT=1` turns it on for
    /// any app without a code change.
    pub fn inspector(mut self, on: bool) -> App {
        self.inspector = on.then(|| Inspector::new(true));
        self
    }

    /// Load styles from a theme file, and load them again whenever it
    /// changes while the app runs: edit a colour, save, and the app redraws
    /// in it. The file is rich's theme format, a `[styles]` section of
    /// `name = style` lines; `border`, `border.focused` and `title` restyle
    /// the framework's parts, and every name works in markup. Its styles go
    /// over the app's [theme](Self::theme). A file that does not parse (or
    /// is missing) leaves the last good styles in place; the inspector
    /// shows why.
    ///
    /// ```text
    /// [styles]
    /// accent = bold magenta
    /// border.focused = bright_green
    /// ```
    pub fn theme_file(mut self, path: impl Into<std::path::PathBuf>) -> App {
        self.theme_file = Some(ThemeFile {
            path: path.into(),
            stamp: None,
            good: None,
            read: None,
            error: None,
        });
        self
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
        self.theme_base = theme.clone();
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

    /// Open the [command palette](Ctx::command_palette) with `keys` (say
    /// `"ctrl+p"`), when no binding of the app used them.
    pub fn palette_key(mut self, keys: &str) -> App {
        self.palette_key = crate::node::parse_keys(keys);
        self
    }

    /// Open the [help](Ctx::help) with `keys` (say `"f1"`), when no binding
    /// of the app used them.
    pub fn help_key(mut self, keys: &str) -> App {
        self.help_key = crate::node::parse_keys(keys);
        self
    }

    /// Whether [`run`](Self::run) reads keys as a legacy terminal sends
    /// them (off by default). Off, a terminal with the kitty keyboard
    /// protocol has it turned on: Tab and Ctrl+I are told apart, and widgets
    /// get [`WidgetEvent::KeyUp`].
    pub fn legacy_keys(mut self, on: bool) -> App {
        self.legacy_keys = on;
        self
    }

    /// Whether a drag with the mouse that no node uses selects text, which
    /// is copied to the clipboard when the button is released (on by
    /// default). With the mouse captured, the terminal cannot select by
    /// itself.
    pub fn selectable(mut self, on: bool) -> App {
        self.selectable = on;
        self
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
            legacy_keys: self.legacy_keys,
        })?;
        let mut app = self;
        app.text_frames = false;
        app.run_on(&mut session)
    }

    /// Run on `backend`: a terminal session, or the headless driver. The
    /// app owns the loop: it waits for events, runs timers and draws. To
    /// own the loop yourself, use [`driver`](Self::driver), which this is
    /// built on.
    pub fn run_on(self, backend: &mut impl Backend) -> io::Result<()> {
        let (width, height) = backend.size();
        let inline = self.inline.is_some();
        let text_frames = self.text_frames;
        let mut driver = self.driver(width, height);
        driver.set_clipboard(backend.clipboard().is_ok());
        // Text a handler copied, or the mouse selected, onto the clipboard:
        // after every step that runs handlers (timers, tasks and watches in
        // `update`, lifecycle events in `render`, input in `event`), so a
        // copy is neither late nor lost to a quit.
        fn copy_all(driver: &mut Driver, backend: &mut impl Backend) {
            for text in driver.take_copies() {
                if backend.copy(&text).is_ok() {
                    driver.copied(&text);
                }
            }
        }
        let result = (|| -> io::Result<()> {
            loop {
                driver.update(backend.elapsed());
                copy_all(&mut driver, backend);
                if driver.is_done() {
                    return Ok(());
                }
                if let Some(out) = driver.render() {
                    if !out.is_empty() {
                        backend.write(&out)?;
                    }
                    if text_frames {
                        backend.painted(&driver.screen().plain().join("\n"));
                    }
                }
                copy_all(&mut driver, backend);
                let wait = driver.timeout(backend.elapsed());
                let Some(event) = backend.read(Some(wait))? else {
                    continue;
                };
                if inline {
                    driver.set_origin(backend.origin());
                }
                driver.event(event);
                copy_all(&mut driver, backend);
                if driver.is_done() {
                    return Ok(());
                }
            }
        })();
        let _ = backend.write(&driver.finish());
        result
    }

    /// Drive the app from a loop of your own, in a terminal `width` x
    /// `height` cells: you read events (from crossterm, termion, a socket,
    /// a test), and the [`Driver`] turns each into a frame's worth of
    /// bytes. [`run`](Self::run) and [`run_on`](Self::run_on) are this,
    /// with the loop written for you.
    ///
    /// ```
    /// use std::time::Duration;
    /// use intuituive::interact::{Event, Key};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let count = signal(0);
    ///     text!("count {count}")
    ///         .on_key("+", move |_| count.update(|n| *n += 1))
    ///         .on_key("q", |cx| cx.quit())
    /// });
    /// let mut driver = app.driver(20, 2);
    /// let mut out = String::new();
    /// for key in ["+", "+", "q"] {
    ///     driver.update(Duration::ZERO);
    ///     out += &driver.render().unwrap_or_default();
    ///     driver.event(Event::Key(Key::parse(key).unwrap()));
    ///     if driver.is_done() {
    ///         break;
    ///     }
    /// }
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "count 2");
    /// out += &driver.finish();
    /// assert!(out.contains("count"));
    /// ```
    pub fn driver(mut self, width: u16, height: u16) -> Driver {
        let console = self.console_for(width);
        self.last_console = Some(console.clone());
        let mut painter = Painter::new(None);
        if self.inline.is_some() {
            painter = painter.inline();
        }
        painter.set_color_system(console.color_system());
        let screen = Screen::new(width, self.region(height));
        // Focus what can be focused before the first frame (keyed children
        // appear only once drawn; `keep_focus` catches them after it).
        self.focus_first();
        Driver {
            app: self,
            painter,
            console,
            screen,
            rows: height,
            first: true,
            started: false,
            origin: 0,
            done: false,
        }
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
            // Inside a scroll, a node's coordinates are its own: move the
            // caret onto the screen, and hide it when scrolled out of view.
            crate::node::walk_screen(&self.top().root, &mut |node, _, shift, clip| {
                if node.id() == id {
                    caret = node.caret.get().and_then(|(x, y)| {
                        let at = crate::node::translate(Rect::new(x, y, 1, 1), shift);
                        clip.contains(at.x, at.y).then_some((at.x, at.y))
                    });
                }
            });
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
        let whole = screen.area();
        // The inspector, when shown, takes the right of the screen.
        let panel = self
            .inspector
            .as_ref()
            .and_then(|inspector| inspector.width(whole.width))
            .map(|w| Rect::new(whole.right() - w, whole.y, w, whole.height));
        let area = match panel {
            Some(panel) => Rect::new(whole.x, whole.y, whole.width - panel.width, whole.height),
            None => whole,
        };
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
                .enumerate()
                .map(|(i, layer)| match (layer.modal, layer.anchor) {
                    (None, _) => area,
                    (Some((width, height)), Some((anchor, placement))) => {
                        // The anchor is on a screen below this one.
                        let below = &self.layers[..base + i];
                        let at = match anchor {
                            Anchor::Rect(rect) => rect,
                            Anchor::Node(id) => below
                                .iter()
                                .rev()
                                .find_map(|layer| anchor_rect(&layer.root, id))
                                .unwrap_or_default(),
                        };
                        popup_rect(console, &layer.root, area, at, placement, width, height)
                    }
                    (Some((width, height)), None) => {
                        modal_rect(console, &layer.root, area, width, height)
                    }
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
            hover_path: self.hover_path,
            wants_hover: std::cell::Cell::new(false),
            watchers: &self.runtime.watchers,
            pointer: self.last_pointer,
            shift: (0, 0),
            resized: Vec::new(),
            theme: &self.theme,
            drawn: 0,
            drawn_ids: panel.map(|_| Vec::new()),
        };
        let dirty = frame.dirty.len();
        if full {
            // A screen opened or closed: nothing on screen can be trusted.
            screen.clear(whole);
            frame.damage.push(whole);
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
        // Toasts, newest at the bottom right, over everything.
        let mut bottom = area.bottom();
        for (markup, _) in self.toasts.iter().rev() {
            let text =
                rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.clone()));
            let w = (text.cell_len() as u16 + 4).min(area.width.saturating_sub(2));
            if w < 5 || bottom < area.y + 3 {
                // No room for a box: the newest toast on the bottom row,
                // reversed, so it is still seen.
                if bottom == area.bottom() && area.width > 0 && area.height > 0 {
                    let rect = Rect::new(area.x, area.bottom() - 1, area.width, 1);
                    let mut options = console.options().update_width(rect.width as usize);
                    options.no_wrap = Some(true);
                    options.overflow = Some(rich::Overflow::Ellipsis);
                    let line = console.render_lines(&text, &options, false);
                    let reverse = Style::parse("reverse").unwrap_or_default();
                    screen.write_lines(rect, &line);
                    for x in rect.x..rect.right() {
                        screen.restyle(x, rect.y, &reverse);
                    }
                    frame.damage.push(rect);
                }
                break;
            }
            let rect = Rect::new(area.right().saturating_sub(w + 1), bottom - 3, w, 3);
            let style = self.theme.border(false);
            let lines = crate::node::border("", &style, &style, rect);
            screen.write_lines(rect, &lines);
            let mut options = console.options().update_width(rect.width as usize - 4);
            options.no_wrap = Some(true);
            options.overflow = Some(rich::Overflow::Ellipsis);
            let inner = console.render_lines(&text, &options, false);
            screen.write_lines(Rect::new(rect.x + 2, rect.y + 1, rect.width - 4, 1), &inner);
            frame.damage.push(rect);
            bottom -= 3;
        }
        if frame.wants_hover.get() {
            self.motion.0 = true;
        }
        for id in std::mem::take(&mut frame.resized) {
            let mut size = (0, 0);
            for layer in &self.layers[base..] {
                with_node(&layer.root, id, &mut |node| {
                    let rect = node.rect();
                    size = (rect.width, rect.height);
                });
            }
            self.lifecycle.push((
                id,
                WidgetEvent::Resize {
                    width: size.0,
                    height: size.1,
                },
            ));
        }
        let mut damage = frame.damage;
        if let (Some(panel), Some(drawn)) = (panel, frame.drawn_ids) {
            let inspector = self.inspector.as_mut().expect("a panel means an inspector");
            inspector.frames += 1;
            let report = crate::inspect::Report {
                layers: self.layers[base..]
                    .iter()
                    .map(|layer| (&layer.root, layer.modal.is_some()))
                    .collect(),
                drawn: &drawn,
                dirty,
                damage: &damage,
                focus: self.layers.last().and_then(|layer| layer.focus),
                stats: self.stats,
                frame: inspector.frames,
                theme: self.theme_status(),
            };
            let lines = crate::inspect::render(console, &report, panel.width, panel.height);
            screen.write_lines(panel, &lines);
            damage.push(panel);
        }
        damage
    }

    fn ctx(&self) -> Ctx {
        let mut cx = Ctx::new(self.runtime.proxy());
        cx.pointer = self.pointer;
        cx
    }

    /// Apply what a handler asked for; whether to quit.
    fn apply(&mut self, cx: Ctx) -> bool {
        let mut quit = cx.quit;
        self.copies.extend(cx.copies);
        for (markup, duration) in cx.toasts {
            self.toasts.push((markup, self.now + duration));
            self.poke = true;
        }
        for (value, to, duration, easing) in cx.animations {
            // A new animation of a signal replaces the one running.
            self.animations.retain(|a| a.value != value);
            let from = self.runtime.enter(|| value.get_untracked());
            self.animations.push(Animation {
                value,
                from,
                to,
                start: self.now,
                duration,
                easing,
            });
        }
        for nav in cx.nav {
            quit |= self.navigate(nav);
        }
        if let Some(theme) = cx.theme {
            self.theme_base = theme;
            self.retheme();
        }
        match cx.focus {
            Some(FocusMove::Next) => self.move_focus(true),
            Some(FocusMove::Previous) => self.move_focus(false),
            Some(FocusMove::To(id)) => self.set_focus(Some(id)),
            None => {}
        }
        quit
    }

    /// Whether to quit (a command run from the palette may quit).
    fn navigate(&mut self, nav: Nav) -> bool {
        match nav {
            Nav::Push(build) => self.open(build, None),
            Nav::Modal(width, height, build) => self.open(build, Some((width, height))),
            Nav::Popup(anchor, placement, width, height, build) => {
                self.open(build, Some((width, height)));
                self.top_mut().anchor = Some((anchor, placement));
            }
            Nav::Pop => {
                if self.layers.len() > 1 {
                    self.close();
                    let focus = self.focus();
                    self.set_focus(focus);
                }
            }
            Nav::Replace(build) => {
                let (modal, anchor) = (self.top().modal, self.top().anchor);
                if self.layers.len() > 1 {
                    self.close();
                    self.open(build, modal);
                    self.top_mut().anchor = anchor;
                } else {
                    // The first screen: its timers go with it.
                    let old = self.layers.pop().expect("an app always has a screen");
                    old.root.forget(&self.runtime);
                    for watch in old.watches {
                        self.runtime.drop_watch(watch);
                    }
                    self.open(build, None);
                }
            }
            Nav::Palette => self.open_palette(false),
            Nav::Help => self.open_palette(true),
            Nav::Run(id, index) => return self.run_binding(id, index),
        }
        false
    }

    /// The described bindings of the focused node and its ancestors,
    /// innermost first, each with its node and index.
    fn bindings(&self) -> Vec<(NodeId, usize, rich_interact::keymap::Binding)> {
        let path = self
            .focus()
            .map(|id| self.path_to(id))
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| vec![self.top().root.id()]);
        let mut out = Vec::new();
        for &id in path.iter().rev() {
            with_node(&self.top().root, id, &mut |node| {
                let context = node.label();
                for (index, (keys, description, _)) in node.keys.borrow().iter().enumerate() {
                    if description.is_empty() {
                        continue;
                    }
                    let binding = rich_interact::keymap::Binding::new(
                        context.clone(),
                        format!("{id}:{index}"),
                        keys.clone(),
                        description.clone(),
                    );
                    out.push((id, index, binding));
                }
            });
        }
        out
    }

    /// The command palette (or the help, if `help`) over the top screen.
    fn open_palette(&mut self, help: bool) {
        let bindings = self.bindings();
        if help {
            let all: Vec<_> = bindings.into_iter().map(|(_, _, b)| b).collect();
            self.open(
                Box::new(move || {
                    let help = rich_interact::overlay::Help::from_bindings(all);
                    crate::node::component(help, |_, cx| cx.pop())
                        .on_cancel(|cx| cx.pop())
                        .panel("Help")
                }),
                Some((Size::Percent(70), Size::Percent(70))),
            );
            return;
        }
        let commands: Vec<rich_interact::overlay::Command> = bindings
            .iter()
            .map(|(_, _, binding)| rich_interact::overlay::Command::from_binding(binding))
            .collect();
        let targets: Vec<(String, NodeId, usize)> = bindings
            .iter()
            .map(|(id, index, binding)| (binding.id(), *id, *index))
            .collect();
        self.open(
            Box::new(move || {
                let palette = rich_interact::overlay::Palette::new(commands);
                crate::node::component(palette, move |command, cx| {
                    cx.pop();
                    if let Some((_, id, index)) = targets.iter().find(|t| t.0 == command.id) {
                        cx.nav.push(Nav::Run(*id, *index));
                    }
                })
                .on_cancel(|cx| cx.pop())
                .panel("Commands")
            }),
            Some((Size::Percent(60), Size::Percent(60))),
        );
    }

    /// Run binding `index` of node `id` on the top screen; whether to quit.
    fn run_binding(&mut self, id: NodeId, index: usize) -> bool {
        let mut cx = self.ctx();
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                if let Some((_, _, handler)) = node.keys.borrow_mut().get_mut(index) {
                    handler(&mut cx);
                }
            });
        });
        self.apply(cx)
    }

    /// Build a screen and put it on top, focusing its first focusable node.
    fn open(&mut self, build: Build, modal: Option<(Size, Size)>) {
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        WATCHES.with(|watches| *watches.borrow_mut() = Some(Vec::new()));
        let runtime = self.runtime.clone();
        let root = runtime.enter(build);
        let mut timers = BUILDING
            .with(|building| building.borrow_mut().take())
            .unwrap_or_default();
        let watches = WATCHES
            .with(|watches| watches.borrow_mut().take())
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
            anchor: None,
            timers,
            watches,
            order: Vec::new(),
            fallback: false,
        });
        self.restack = true;
        self.set_focus(None);
        self.focus_first();
    }

    /// Take the top screen off, forgetting its subscriptions.
    fn close(&mut self) {
        let layer = self.layers.pop().expect("an app always has a screen");
        layer.root.forget(&self.runtime);
        for watch in layer.watches {
            self.runtime.drop_watch(watch);
        }
        self.restack = true;
    }

    /// The theme in code with the theme file's last good styles over it.
    fn retheme(&mut self) {
        let good = self.theme_file.as_ref().and_then(|file| file.good.clone());
        self.theme = match good {
            Some(text) => self
                .theme_base
                .clone()
                .with_config(&text)
                .unwrap_or_else(|_| self.theme_base.clone()),
            None => self.theme_base.clone(),
        };
        self.restyled = true;
    }

    /// Read the theme file again if it changed since it was last read.
    fn reload_theme(&mut self) {
        let Some(file) = &mut self.theme_file else {
            return;
        };
        let stamp = std::fs::metadata(&file.path)
            .ok()
            .map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
        if stamp == file.stamp && (stamp.is_some() || file.error.is_some()) {
            // A file system with coarse timestamps can hide a same-length
            // edit made within one tick: while the file is that fresh, look
            // at what it says.
            let fresh = stamp.is_some_and(|(modified, _)| {
                modified
                    .elapsed()
                    .map_or(true, |age| age < Duration::from_secs(2))
            });
            if !fresh {
                return;
            }
            let now = std::fs::read_to_string(&file.path).ok();
            if now.is_none() || now == file.read {
                return;
            }
        }
        file.stamp = stamp;
        let read = std::fs::read_to_string(&file.path).map_err(|e| e.to_string());
        file.read = read.as_ref().ok().cloned();
        match read.and_then(|text| self.theme_base.clone().with_config(&text).map(|_| text)) {
            Ok(text) => {
                file.good = Some(text);
                file.error = None;
                self.retheme();
            }
            Err(error) => {
                file.error = Some(error);
                self.poke = true;
            }
        }
    }

    /// The theme file's state, for the inspector.
    fn theme_status(&self) -> Option<String> {
        let file = self.theme_file.as_ref()?;
        let name = file.path.file_name().map_or_else(
            || file.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        Some(match &file.error {
            Some(error) => format!("{name}: {error}"),
            None => format!("{name}: loaded"),
        })
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

    /// Keep a focused node, so keys always have a path to the root. When
    /// the focused node left the tree (a keyed row that was removed), the
    /// focus goes to the node after it in the last frame's Tab order, or
    /// the one before it if it was last; with nothing focused, to the
    /// first focusable node.
    fn keep_focus(&mut self) {
        let ids = self.focusable();
        if let Some(id) = self.focus() {
            if self.path_to(id).is_empty() {
                let order = &self.top().order;
                let next = order.iter().position(|o| *o == id).and_then(|at| {
                    order[at + 1..]
                        .iter()
                        .chain(order[..at].iter().rev())
                        .find(|o| ids.contains(o))
                        .copied()
                });
                self.set_focus(next);
            }
        }
        if self.focus().is_none() {
            self.focus_first();
        } else if self.top().fallback {
            // An `autofocus` node that a lazy `each` or `switch` built
            // after the first frame still takes the focus, as long as
            // only the fallback has held it.
            if let Some(id) = self.autofocused() {
                self.set_focus(Some(id));
            }
        }
        self.top_mut().order = ids;
    }

    /// Nodes still dirty after a frame were made dirty by their own
    /// drawing (they wrote a signal they read). Once is fine (a scroll
    /// that moved to keep the focus in view); a node that does it frame
    /// after frame would draw forever, so its own writes stop drawing it
    /// again, and a toast says which node it is.
    fn calm_restless(&mut self) {
        let dirty = self.runtime.dirty_now();
        self.restless.retain(|id, _| dirty.contains(id));
        for id in dirty {
            let frames = self.restless.entry(id).or_insert(0);
            *frames += 1;
            if *frames < RESTLESS_FRAMES {
                continue;
            }
            self.restless.remove(&id);
            self.runtime.quiet(id);
            let mut what = String::from("a node");
            for layer in &self.layers {
                with_node(&layer.root, id, &mut |node| what = node.describe());
            }
            let what = rich::markup::escape(&what);
            self.toasts.push((
                format!(
                    "[bold red]{what} draws itself again and again[/]: it writes a \
                     signal it reads while it draws. Write it in a handler or a watch."
                ),
                self.now + Duration::from_secs(8),
            ));
            self.restack = true;
        }
    }

    /// Focus the screen's first node that asked for it with
    /// [`autofocus`](crate::Node::autofocus), else its first focusable one.
    fn focus_first(&mut self) {
        let chosen = self.autofocused();
        if let Some(first) = chosen.or_else(|| self.focusable().first().copied()) {
            self.set_focus(Some(first));
            self.top_mut().fallback = chosen.is_none();
        }
    }

    /// The screen's first node that asked for the focus.
    fn autofocused(&self) -> Option<NodeId> {
        let mut chosen = None;
        self.top().root.walk(&mut |node, _| {
            if node.autofocus && chosen.is_none() {
                chosen = Some(node.id);
            }
        });
        chosen
    }

    fn set_focus(&mut self, id: Option<NodeId>) {
        // Against the node last told, whichever screen it is on: opening a
        // modal takes the focus from the screen below, and closing it gives
        // the focus back.
        let old = self.focused;
        if old != id {
            self.lifecycle
                .extend(old.map(|old| (old, WidgetEvent::Focus(false))));
            self.lifecycle
                .extend(id.map(|id| (id, WidgetEvent::Focus(true))));
            self.focused = id;
        }
        let top = self.top_mut();
        top.focus = id;
        top.fallback = false;
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
    /// Whether a node on the path keys take binds `key`.
    fn binds(&self, key: Key) -> bool {
        let mut bound = false;
        for id in self.key_path() {
            with_node(&self.top().root, id, &mut |node| {
                bound |= node
                    .keys
                    .borrow()
                    .iter()
                    .any(|(keys, _, _)| key.matches_any(keys));
            });
        }
        bound
    }

    /// The nodes a key goes through, root first: to the focused node, or
    /// with nothing focused, to the node under the pointer.
    fn key_path(&self) -> Vec<NodeId> {
        match self.focus().map(|id| self.path_to(id)) {
            Some(path) if !path.is_empty() => path,
            _ => {
                let hovered = self.hover_path.get_untracked();
                if hovered.first() == Some(&self.top().root.id) {
                    hovered
                } else {
                    vec![self.top().root.id]
                }
            }
        }
    }

    fn key(&mut self, key: Key) -> bool {
        if let (Some(inspector), KeyCode::F(12)) = (&mut self.inspector, key.code) {
            inspector.open = !inspector.open;
            self.restack = true;
            return false;
        }
        // From the focused node up; with nothing focused, from the node
        // under the pointer, so a container that cannot take the focus
        // still gets keys while the mouse is over it.
        let path = self.key_path();
        // Containers that asked see the key first, outermost first.
        for &id in &path[..path.len().saturating_sub(1)] {
            let mut previews = false;
            with_node(&self.top().root, id, &mut |node| {
                previews = node.body.borrow().widget.previews_keys();
            });
            if previews {
                if let Some(quit) = self.give_to_widget(id, &WidgetEvent::Preview(key)) {
                    return quit;
                }
            }
        }
        for &id in path.iter().rev() {
            if let Some(quit) = self.give_to_widget(id, &WidgetEvent::Key(key)) {
                return quit;
            }
            let mut cx = self.ctx();
            let mut used = false;
            let runtime = self.runtime.clone();
            runtime.enter(|| {
                with_node(&self.top().root, id, &mut |node| {
                    let mut keys = node.keys.borrow_mut();
                    // A binding that names the key before one it could be
                    // (a Tab from a legacy terminal: `tab`, then `ctrl+i`).
                    let picked = key.pick(keys.iter().map(|(keys, _, _)| keys.as_slice()));
                    if let Some((_, _, handler)) = picked.and_then(|index| keys.get_mut(index)) {
                        handler(&mut cx);
                        used = true;
                    }
                });
            });
            if used {
                return self.apply(cx);
            }
        }
        if key.matches_any(&self.palette_key) {
            return self.navigate(Nav::Palette);
        }
        if key.matches_any(&self.help_key) {
            return self.navigate(Nav::Help);
        }
        match key.code {
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Escape if self.top().anchor.is_some() => return self.navigate(Nav::Pop),
            _ => {}
        }
        false
    }

    /// Route a key's release to the widgets a press goes to, focused node
    /// first, until one uses it. Bindings are for presses only. Whether to
    /// quit.
    fn key_up(&mut self, key: Key) -> bool {
        for id in self.key_path().into_iter().rev() {
            if let Some(quit) = self.give_to_widget(id, &WidgetEvent::KeyUp(key)) {
                return quit;
            }
        }
        false
    }

    /// Offer `event` to the focused node's widget and then its
    /// ancestors' until one uses it. Whether to quit.
    fn bubble(&mut self, event: &WidgetEvent) -> bool {
        let path = self.focus().map(|id| self.path_to(id)).unwrap_or_default();
        for &id in path.iter().rev() {
            if let Some(quit) = self.give_to_widget(id, event) {
                return quit;
            }
        }
        false
    }

    /// Tell widgets the focus, hover and resize events waiting for them.
    /// Whether to quit.
    fn tell_lifecycle(&mut self) -> bool {
        // A handler may move the focus again: what that sets off waits for
        // the next turn, a few rounds at most.
        for _ in 0..4 {
            let events = std::mem::take(&mut self.lifecycle);
            if events.is_empty() {
                break;
            }
            for (id, event) in events {
                if let Some(true) = self.give_to_widget(id, &event) {
                    return true;
                }
            }
        }
        false
    }

    /// Route a mouse event. Movement updates what is hovered; a captured
    /// mouse goes to the node that captured it; anything else goes to the
    /// deepest node under the pointer and bubbles up through its ancestors
    /// until one uses it. A press first focuses the deepest focusable node
    /// under the pointer. Whether to quit.
    fn mouse(&mut self, mouse: rich_interact::Mouse) -> Option<bool> {
        let hits = crate::node::hit_path(&self.top().root, mouse.column, mouse.row);
        let path: Vec<NodeId> = hits.iter().map(|(id, _)| *id).collect();
        let old = self.hover_path.get_untracked();
        self.last_pointer = Some((mouse.column, mouse.row));
        // Widgets that read the pointer draw again when it moves over them
        // or leaves them.
        for id in self.runtime.watchers.pointer.borrow().iter() {
            if old.contains(id) || path.contains(id) {
                self.runtime.mark_dirty(*id);
            }
        }
        if old != path {
            let left: Vec<NodeId> = old
                .iter()
                .filter(|id| !path.contains(id))
                .copied()
                .collect();
            let came: Vec<NodeId> = path
                .iter()
                .filter(|id| !old.contains(id))
                .copied()
                .collect();
            for id in left.iter().chain(&came) {
                if self.runtime.watchers.hover.borrow().contains(id) {
                    self.runtime.mark_dirty(*id);
                }
            }
            self.lifecycle
                .extend(left.into_iter().map(|id| (id, WidgetEvent::Hover(false))));
            self.lifecycle
                .extend(came.into_iter().map(|id| (id, WidgetEvent::Hover(true))));
            let runtime = self.runtime.clone();
            let hover = self.hover_path;
            let path = path.clone();
            runtime.enter(|| hover.set(path));
        }
        if let Some((id, shift)) = self.capture {
            if matches!(mouse.kind, MouseKind::Drag(_) | MouseKind::Up(_)) {
                if matches!(mouse.kind, MouseKind::Up(_)) {
                    self.capture = None;
                }
                return Some(self.send_mouse(id, shift, mouse).unwrap_or(false));
            }
        }
        if matches!(mouse.kind, MouseKind::Down(_))
            && self.top().anchor.is_some()
            && !self
                .top()
                .drawn
                .is_some_and(|r| r.contains(mouse.column, mouse.row))
        {
            // A press outside a pop-up closes it.
            return Some(self.navigate(Nav::Pop));
        }
        if matches!(mouse.kind, MouseKind::Down(_)) {
            let focusable = path.iter().rev().copied().find(|id| {
                let mut yes = false;
                with_node(&self.top().root, *id, &mut |node| yes = node.focusable);
                yes
            });
            if let Some(id) = focusable {
                self.set_focus(Some(id));
            }
        }
        for &(id, shift) in hits.iter().rev() {
            if let Some(quit) = self.send_mouse(id, shift, mouse) {
                return Some(quit);
            }
        }
        None
    }

    /// A drag selecting text: move its end, and on release copy what it
    /// covers. Whether the event was the selection's.
    fn select(&mut self, mouse: rich_interact::Mouse, screen: &Screen) -> bool {
        let Some((start, _)) = self.selection else {
            self.selecting = false;
            return false;
        };
        let at = (mouse.column, mouse.row);
        match mouse.kind {
            MouseKind::Drag(Button::Left) => {
                self.selection = Some((start, at));
                self.restack = true;
                true
            }
            MouseKind::Up(Button::Left) => {
                self.selecting = false;
                self.selection = Some((start, at));
                if start == at {
                    // A click, not a drag.
                    self.selection = None;
                    return true;
                }
                let text = selected_text(screen, start, at);
                if !text.is_empty() && self.clipboard {
                    self.copies.push(text);
                }
                self.restack = true;
                true
            }
            _ => false,
        }
    }

    /// Offer `mouse` (in screen coordinates) to node `id`, in its own
    /// coordinates: a component gets every button, a widget and an
    /// [`on_mouse`](crate::Node::on_mouse) handler every event, and an
    /// [`on_click`](crate::Node::on_click) handler a left press. `Some`
    /// (whether to quit) if one used it.
    fn send_mouse(
        &mut self,
        id: NodeId,
        shift: (i32, i32),
        mouse: rich_interact::Mouse,
    ) -> Option<bool> {
        let mut rect = Rect::default();
        with_node(&self.top().root, id, &mut |node| rect = node.rect());
        // Into the node's coordinates (they differ inside a scroll), then
        // relative to its rectangle.
        let at = |value: u16, by: i32, from: u16| {
            (value as i32 + by - from as i32).clamp(0, u16::MAX as i32) as u16
        };
        let local = rich_interact::Mouse::new(
            mouse.kind,
            at(mouse.column, shift.0, rect.x),
            at(mouse.row, shift.1, rect.y),
        );
        if let Some(quit) = self.give_to_widget_at(id, shift, &WidgetEvent::Mouse(local)) {
            return Some(quit);
        }
        let mut cx = self.ctx();
        let mut used = false;
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                if let Some(handler) = node.mouse.borrow_mut().as_mut() {
                    used = handler(&mut cx, local);
                }
                if !used && mouse.kind == MouseKind::Down(Button::Left) {
                    if let Some(handler) = node.click.borrow_mut().as_mut() {
                        handler(&mut cx);
                        used = true;
                    }
                }
            });
        });
        used.then(|| self.apply(cx))
    }

    /// Offer `event` to node `id` if it is a [widget](crate::widget). `Some`
    /// (whether to quit) if it used it.
    fn give_to_widget(&mut self, id: NodeId, event: &WidgetEvent) -> Option<bool> {
        let shift = self.shift_of(id);
        self.give_to_widget_at(id, shift, event)
    }

    /// How far a node's coordinates are from the screen's: inside a
    /// scroll, by the scroll.
    fn shift_of(&self, id: NodeId) -> (i32, i32) {
        for layer in &self.layers {
            for (hit, at) in crate::node::shifts(&layer.root) {
                if hit == id {
                    return at;
                }
            }
        }
        (0, 0)
    }

    fn give_to_widget_at(
        &mut self,
        id: NodeId,
        shift: (i32, i32),
        event: &WidgetEvent,
    ) -> Option<bool> {
        let mut cx = self.ctx();
        let mut used = false;
        let mut redraw = false;
        let mut capture = None;
        let fallback;
        let console = match &self.last_console {
            Some(console) => console,
            None => {
                fallback = Console::builder().build();
                &fallback
            }
        };
        let focused = self.focus() == Some(id);
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            let mut found = false;
            for layer in self.layers.iter().rev() {
                if found {
                    break;
                }
                with_node(&layer.root, id, &mut |node| {
                    found = true;
                    let rect = node.rect();
                    let mut ecx = crate::widget::EventCx {
                        ctx: &mut cx,
                        console,
                        size: (rect.width, rect.height),
                        rect: crate::node::translate(rect, (-shift.0, -shift.1)),
                        focused,
                        redraw: false,
                        capture: None,
                    };
                    used = node.body.borrow_mut().widget.event(&mut ecx, event) == Used::Yes;
                    redraw = ecx.redraw;
                    capture = ecx.capture;
                });
            }
        });
        if redraw {
            self.runtime.mark_dirty(id);
        }
        match capture {
            Some(true) => self.capture = Some((id, shift)),
            Some(false) if self.capture.is_some_and(|(c, _)| c == id) => self.capture = None,
            _ => {}
        }
        used.then(|| self.apply(cx))
    }
}

/// A running app driven by a loop of yours: made by [`App::driver`].
///
/// Each turn of the loop:
///
/// 1. [`update`](Self::update) with the time since the start: timers,
///    animations, results from other threads, watches, toasts;
/// 2. [`render`](Self::render), and write what it returns to the
///    terminal: only what changed, as escape sequences;
/// 3. wait at most [`timeout`](Self::timeout) for an event, and give it to
///    [`event`](Self::event);
///
/// until [`is_done`](Self::is_done). Then write [`finish`](Self::finish)'s
/// bytes, which leave the terminal as it was. Setting the terminal up
/// (raw mode, the alternate screen, mouse reporting) is yours, so is
/// copying [`take_copies`](Self::take_copies) to the clipboard.
///
/// [`screen`](Self::screen) is the frame as cells, for a loop that shows
/// it some other way (inside another program's frame, in a test).
pub struct Driver {
    app: App,
    painter: Painter,
    console: Console,
    screen: Screen,
    /// The terminal's rows.
    rows: u16,
    /// Draw everything next time.
    first: bool,
    /// The cursor was hidden by the first frame.
    started: bool,
    /// Inline: the terminal row the region starts on.
    origin: u16,
    done: bool,
}

/// How many frames in a row a node may leave itself dirty before its own
/// writes stop drawing it again.
const RESTLESS_FRAMES: u32 = 4;

impl Driver {
    /// Bring the app up to `now` (the time since it started): run the
    /// timers that are due, move animations, deliver results from other
    /// threads and run the watches they set off, and drop toasts that are
    /// over.
    pub fn update(&mut self, now: Duration) {
        if self.done {
            return;
        }
        let app = &mut self.app;
        app.reload_theme();
        app.now = now;
        let mut cx = app.ctx();
        let runtime = app.runtime.clone();
        runtime.enter(|| {
            for layer in &mut app.layers {
                for timer in &mut layer.timers {
                    if now >= timer.next {
                        (timer.tick)(&mut cx);
                        // Once, however far behind (the clock jumped, the
                        // laptop slept), and on to the next tick in step.
                        let every = timer.every.as_nanos().max(1);
                        let behind = (now - timer.next).as_nanos() % every;
                        timer.next = now + Duration::from_nanos((every - behind) as u64);
                    }
                }
            }
            // Animations: each one's value for now; finished ones go.
            app.animations.retain(|a| {
                let t = if a.duration.is_zero() {
                    1.0
                } else {
                    now.saturating_sub(a.start).as_secs_f64() / a.duration.as_secs_f64()
                };
                a.value.set(a.from + (a.to - a.from) * a.easing.at(t));
                t < 1.0
            });
            // Results from other threads, then the watches they (or the
            // last event) set off. Waiting for tasks, this repeats until
            // none is in flight: a watch may start one, and its result may
            // start another.
            loop {
                if app.wait_for_tasks {
                    app.settle_tasks();
                }
                let delivered = runtime.run_inbox(&mut cx);
                let watched = runtime.run_watches(&mut cx);
                let busy = runtime.tasks.load(std::sync::atomic::Ordering::SeqCst) > 0;
                if !app.wait_for_tasks || !(delivered || watched || busy) {
                    break;
                }
            }
        });
        if app.apply(cx) {
            self.done = true;
            return;
        }
        // Toasts that are over: what was under them draws again.
        let shown = app.toasts.len();
        app.toasts.retain(|(_, until)| *until > now);
        if app.toasts.len() != shown {
            app.restack = true;
        }
        if app.restyled {
            app.restyled = false;
            self.console = app.console_for(self.screen.area().width);
            app.last_console = Some(self.console.clone());
            self.first = true;
        }
    }

    /// Whether anything changed since the last frame.
    pub fn needs_render(&self) -> bool {
        self.first || self.app.restack || self.app.poke || self.app.runtime.has_dirty()
    }

    /// Draw what changed and return the bytes that show it (they may be
    /// empty); `None` when nothing changed. Then tell widgets of focus,
    /// hover and size changes, now that they are laid out.
    pub fn render(&mut self) -> Option<String> {
        if self.done {
            return None;
        }
        let out = self.needs_render().then(|| {
            let mut out = String::new();
            if !self.started {
                self.started = true;
                out.push_str("\x1b[?25l");
            }
            out.push_str(&self.paint(self.first));
            self.first = false;
            // A frame can leave nodes to draw again (a scroll that moved to
            // keep the focus in view, pointer readers inside it): once more,
            // so what is sent is settled.
            if self.app.runtime.has_dirty() {
                out.push_str(&self.paint(false));
            }
            self.app.calm_restless();
            // Keyed children exist once drawn: focus is chosen after a
            // frame, and chosen again if the focused node has gone.
            self.app.keep_focus();
            out
        });
        // Focus, hover and resize events, once what they are about is laid
        // out: a widget told it has the focus knows where it is.
        if self.app.tell_lifecycle() {
            self.done = true;
        }
        out
    }

    /// How long the loop may wait for an event before calling
    /// [`update`](Self::update) again: not at all while a frame or a
    /// widget's event waits; else until the next timer, a frame's time
    /// while something animates, until the next toast goes, and at most
    /// 50 ms (results from other threads and theme files are picked up by
    /// `update`).
    pub fn timeout(&self, now: Duration) -> Duration {
        let app = &self.app;
        if self.needs_render() || !app.lifecycle.is_empty() {
            // Something is waiting to be drawn or told: no waiting.
            return Duration::ZERO;
        }
        app.layers
            .iter()
            .flat_map(|layer| &layer.timers)
            .map(|t| t.next.saturating_sub(now))
            .min()
            .unwrap_or(Duration::from_millis(50))
            .min(Duration::from_millis(50))
            .min(if app.animations.is_empty() {
                Duration::MAX
            } else {
                Duration::from_millis(16)
            })
            .min(
                app.toasts
                    .iter()
                    .map(|(_, until)| until.saturating_sub(now))
                    .min()
                    .unwrap_or(Duration::MAX),
            )
    }

    /// Handle an event: a key pressed or let go, the mouse, pasted text, or
    /// the terminal's new size. Ctrl+C quits, unless a node on the focused
    /// path binds it ([`on_key`](crate::Node::on_key)); then the binding
    /// runs.
    pub fn event(&mut self, event: Event) {
        if self.done {
            return;
        }
        let quit = match event {
            Event::Key(key) if key == Key::ctrl('c') && !self.app.binds(key) => true,
            Event::Resize { columns, rows } => {
                self.resize(columns, rows);
                false
            }
            Event::Key(key) => self.app.key(key),
            Event::KeyUp(key) => self.app.key_up(key),
            Event::Paste(text) => self.app.bubble(&WidgetEvent::Paste(text)),
            Event::Mouse(mouse) => self.mouse(mouse),
            _ => false,
        };
        if quit {
            self.done = true;
        }
    }

    /// The terminal is now `columns` x `rows`: everything draws again.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.rows = rows;
        self.console = self.app.console_for(columns);
        self.app.last_console = Some(self.console.clone());
        self.screen = Screen::new(columns, self.app.region(rows));
        self.painter.invalidate();
        self.first = true;
    }

    fn mouse(&mut self, mouse: rich_interact::Mouse) -> bool {
        let app = &mut self.app;
        // Inline, the region starts where the session began, or higher if
        // it had to scroll to fit.
        let top = if app.inline.is_some() {
            self.origin
                .min(self.rows.saturating_sub(self.screen.area().height))
        } else {
            0
        };
        let Some(row) = mouse.row.checked_sub(top) else {
            return false;
        };
        let mouse = rich_interact::Mouse::new(mouse.kind, mouse.column, row);
        if app.selecting && app.select(mouse, &self.screen) {
            return false;
        }
        app.pointer = Some((mouse.column, row));
        let routed = app.mouse(mouse);
        app.pointer = None;
        match routed {
            Some(quit) => quit,
            // Nothing used a press: it may start a selection.
            None => {
                if app.selection.take().is_some() {
                    app.restack = true;
                }
                if app.selectable && mouse.kind == MouseKind::Down(Button::Left) {
                    let at = (mouse.column, row);
                    app.selection = Some((at, at));
                    app.selecting = true;
                }
                false
            }
        }
    }

    /// Whether the app quit (a handler called [`Ctx::quit`], or Ctrl+C).
    pub fn is_done(&self) -> bool {
        self.done
    }

    /// The frame as drawn, cell by cell.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// The last frame's numbers.
    pub fn stats(&self) -> FrameStats {
        self.app.stats
    }

    /// Whether text selected with the mouse can be put on the clipboard
    /// (off by default; [`run_on`](App::run_on) asks the backend). When it
    /// can, a selection shows a toast and waits in
    /// [`take_copies`](Self::take_copies).
    pub fn set_clipboard(&mut self, on: bool) {
        self.app.clipboard = on;
    }

    /// Text selected with the mouse, or [copied](Ctx::copy) by a handler,
    /// since the last call, for the loop to put on the clipboard (with OSC
    /// 52, or the system's); tell
    /// [`copied`](Self::copied) when it is there.
    pub fn take_copies(&mut self) -> Vec<String> {
        std::mem::take(&mut self.app.copies)
    }

    /// `text` is on the clipboard: a toast says so.
    pub fn copied(&mut self, text: &str) {
        let n = text.chars().count();
        self.app.toasts.push((
            format!("Copied {n} character{}", if n == 1 { "" } else { "s" }),
            self.app.now + Duration::from_secs(2),
        ));
        self.app.restack = true;
    }

    /// Inline: the terminal row the app's region starts on, for placing
    /// mouse events.
    pub fn set_origin(&mut self, row: u16) {
        self.origin = row;
    }

    /// Stop: the bytes that show the last change and leave the terminal
    /// as it was (styles reset, the cursor shown; inline, below the
    /// region, where the shell's prompt follows the app's last frame).
    pub fn finish(mut self) -> String {
        let mut out = String::new();
        // Inline, the last frame stays in the scrollback: show what the
        // last handler changed before leaving.
        if self.app.inline.is_some() && (self.app.restack || self.app.runtime.has_dirty()) {
            out.push_str(&self.paint(false));
        }
        out.push_str(&self.painter.finish());
        out.push_str("\x1b[0m\x1b[?25h");
        self.app.runtime.close();
        out
    }

    /// Draw what changed: the bytes to send.
    fn paint(&mut self, full: bool) -> String {
        let app = &mut self.app;
        app.poke = false;
        let screen = &mut self.screen;
        let mut damage = app.frame(&self.console, screen, full);
        if let Some((start, end)) = app.selection {
            let style = Style::parse("reverse").expect("a built-in style parses");
            for (x, y) in selected_cells(screen.area(), start, end) {
                screen.restyle(x, y, &style);
            }
            damage.push(screen.area());
        }
        let mut out = self.painter.paint(screen, &damage);
        if app.motion.0 && !app.motion.1 && app.inline.is_none() {
            // A widget reads hover: report the pointer's movement too. The
            // session turns it off with the rest of the mouse on the way out.
            out.push_str("\x1b[?1003h");
            app.motion.1 = true;
        }
        out.push_str(&app.caret(&mut self.painter));
        app.stats.bytes = out.len();
        out
    }
}

/// The cells between `start` and `end` in reading order: the rest of the
/// first row, the rows between, and the last row up to the end.
fn selected_cells(area: Rect, start: (u16, u16), end: (u16, u16)) -> Vec<(u16, u16)> {
    let (a, b) = if (start.1, start.0) <= (end.1, end.0) {
        (start, end)
    } else {
        (end, start)
    };
    let mut cells = Vec::new();
    for y in a.1..=b.1.min(area.bottom().saturating_sub(1)) {
        let from = if y == a.1 { a.0 } else { 0 };
        let to = if y == b.1 {
            b.0
        } else {
            area.right().saturating_sub(1)
        };
        for x in from..=to.min(area.right().saturating_sub(1)) {
            cells.push((x, y));
        }
    }
    cells
}

/// The text of the cells between `start` and `end`, rows joined by
/// newlines and each row's trailing spaces dropped.
fn selected_text(screen: &Screen, start: (u16, u16), end: (u16, u16)) -> String {
    let mut rows: Vec<String> = Vec::new();
    let mut row = None;
    for (x, y) in selected_cells(screen.area(), start, end) {
        if row != Some(y) {
            rows.push(String::new());
            row = Some(y);
        }
        let cell = screen.cell(x, y);
        if !cell.is_continuation() {
            rows.last_mut().expect("a row").push_str(&cell.text);
        }
    }
    rows.iter()
        .map(|r| r.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Where node `id` is on the screen, if it is in `root`'s tree and shown.
fn anchor_rect(root: &Node, id: NodeId) -> Option<Rect> {
    let mut found = None;
    crate::node::walk_screen(root, &mut |node, _, shift, clip| {
        if node.id() == id {
            found = Some(crate::node::translate(node.rect(), shift).intersection(clip));
        }
    });
    found.filter(|r| !r.is_empty())
}

/// Where a pop-up `width` x `height` goes: on the `placement` side of
/// `anchor`, or the other side when that has more room, kept inside
/// `area`.
fn popup_rect(
    console: &Console,
    root: &Node,
    area: Rect,
    anchor: Rect,
    placement: Placement,
    width: Size,
    height: Size,
) -> Rect {
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
    let (above, below) = (
        anchor.y.saturating_sub(area.y),
        area.bottom().saturating_sub(anchor.bottom()),
    );
    let (left, right) = (
        anchor.x.saturating_sub(area.x),
        area.right().saturating_sub(anchor.right()),
    );
    let (x, y) = match placement {
        Placement::Below | Placement::Above => {
            let down = match placement {
                Placement::Below => below >= h || below >= above,
                _ => !(above >= h || above >= below),
            };
            let y = if down {
                anchor.bottom()
            } else {
                anchor.y.saturating_sub(h)
            };
            (anchor.x, y)
        }
        Placement::Right | Placement::Left => {
            let to_right = match placement {
                Placement::Right => right >= w || right >= left,
                _ => !(left >= w || left >= right),
            };
            let x = if to_right {
                anchor.right()
            } else {
                anchor.x.saturating_sub(w)
            };
            (x, anchor.y)
        }
    };
    // Inside the area, sliding back from its right and bottom edges.
    let x = x.min(area.right().saturating_sub(w)).max(area.x);
    let y = y.min(area.bottom().saturating_sub(h)).max(area.y);
    Rect::new(x, y, w, h)
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rich_interact::{Event, Key, Mouse, MouseKind};

    use crate::prelude::*;
    use crate::widget::{widget, Canvas, DrawCx, Widget};

    struct Hoverable;

    impl Widget for Hoverable {
        fn draw(&mut self, cx: &mut DrawCx, _canvas: &mut Canvas) {
            cx.hovered();
            cx.pointer();
        }
    }

    #[test]
    fn widgets_that_left_the_tree_stop_being_watched() {
        let rows = std::rc::Rc::new(std::cell::Cell::new(None));
        let set = rows.clone();
        let app = App::new(move || {
            let keys = signal((0..10).collect::<Vec<u32>>());
            set.set(Some(keys));
            each(move || keys.get(), |_| widget(Hoverable))
        });
        let mut driver = app.driver(10, 10);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        let watched = |driver: &crate::Driver| {
            let watchers = &driver.app.runtime.watchers;
            (
                watchers.hover.borrow().len(),
                watchers.pointer.borrow().len(),
            )
        };
        assert_eq!(watched(&driver), (10, 10));
        let keys = rows.get().expect("the app was built");
        driver.app.runtime.enter(|| keys.set(vec![0, 1]));
        driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 0, 0)));
        driver.update(Duration::ZERO);
        let _ = driver.render();
        assert_eq!(watched(&driver), (2, 2));
        driver.event(Event::Key(Key::ctrl('c')));
    }
}

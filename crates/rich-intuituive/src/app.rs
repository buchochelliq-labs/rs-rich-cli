//! The app: the tree, its runtime, and the loop that runs them.
//!
//! [`App::run`] takes the terminal (the alternate screen, raw mode, the
//! mouse), and gives it back on every way out, including a panic. It drives
//! the terminal with crossterm; [`App::run_with`] picks termion or termwiz
//! instead, behind the crate's features of those names. Each turn
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
use std::panic::Location;
use std::rc::Rc;
use std::time::Duration;

use rich::{ColorSystem, Console, Style};
use rich_interact::{
    Backend, BackendKind, Button, Event, Key, KeyCode, MouseKind, Session, SessionOptions,
};

use crate::inspect::{Handled, Inspector, Names, NodeTiming, Probe, TraceEntry, Traced};
use crate::layout::Size;
use crate::node::{with_node, Axis, FrameState, Node};
use crate::notify::{Corner, Level, Note};
use crate::place::{Offset, Origin};
use crate::reactive::{signal, NodeId, Proxy, Runtime, Signal};
use crate::screen::{Painter, Rect, Screen};
use crate::widget::{Used, WidgetEvent};

/// The theme style a pop-up's title is drawn in: the palette's, the
/// help's, a modal's or pop-up's panel, a [confirm](Ctx::confirm) dialog's.
/// The theme's `title` when it does not name it.
pub(crate) const POPUP_TITLE: &str = "popup.title";

/// What a key or click handler can do.
pub struct Ctx {
    quit: bool,
    focus: Option<FocusMove>,
    proxy: Proxy,
    nav: Vec<Nav>,
    theme: Option<Theme>,
    toasts: Vec<(Note, Duration)>,
    animations: Vec<(Signal<f64>, f64, Duration, Easing)>,
    copies: Vec<String>,
    announcements: Vec<crate::a11y::Announcement>,
    /// Where the mouse event being handled happened, on the screen.
    pub(crate) pointer: Option<(u16, u16)>,
    /// What the app ends with: a value, or an error.
    exit: Option<Box<dyn std::any::Any>>,
    error: Option<io::Error>,
    /// Lines to print above an inline app.
    printed: Vec<String>,
    /// Programs to give the terminal to, in turn.
    handoffs: Vec<Handoff>,
    /// One-shot timers made with [`Ctx::after`].
    afters: Vec<After>,
    /// Screens open when the handler was called.
    layers: usize,
}

/// A one-shot timer waiting to be put on the app's clock: how long it
/// waits, what it runs, and where it was made.
pub(crate) struct After {
    pub wait: Duration,
    pub run: Box<dyn FnOnce(&mut Ctx)>,
    pub at: &'static Location<'static>,
}

/// A one-shot timer on the app's clock: when it is due.
struct Timeout {
    due: Duration,
    run: Box<dyn FnOnce(&mut Ctx)>,
    at: &'static Location<'static>,
}

/// What [`handoff_then`](Ctx::handoff_then) runs once the program is done.
type HandedBack = Box<dyn FnOnce(io::Result<Option<i32>>, &mut Ctx)>;

/// A program the app gives the terminal to ([`Ctx::handoff`]): what a loop
/// of your own takes with [`Driver::take_handoff`], runs with the terminal
/// as it was before the app, and gives back with
/// [`Driver::handed_back`].
pub struct Handoff {
    /// The program, its arguments, its directory and environment.
    pub command: std::process::Command,
    then: Option<HandedBack>,
}

impl std::fmt::Debug for Handoff {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handoff")
            .field("command", &self.command)
            .finish_non_exhaustive()
    }
}

/// How a [modal](Ctx::modal_with) or a [pop-up](Ctx::popup_with) looks
/// over the screen below it: a shadow, and a backdrop. Styles are console
/// styles or theme style names (`"on grey11"`, `"dim"`, `"muted"`).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::ModalOptions;
///
/// let app = App::new(|| {
///     label("below").on_key("o", |cx| {
///         let look = ModalOptions::new().shadow("on grey11").backdrop("dim");
///         cx.modal_with(Size::Fixed(8), Size::Fixed(3), look, || label("hi").panel(""))
///     })
/// });
/// let mut driver = app.driver(20, 6);
/// driver.press("o");
/// assert!(driver.screen().plain()[2].contains("hi"));
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModalOptions {
    shadow: Option<String>,
    backdrop: Option<String>,
}

impl ModalOptions {
    pub fn new() -> ModalOptions {
        ModalOptions::default()
    }

    /// A shadow one cell right and down of the box, restyling the screen
    /// below in `style`: ratatui's `Shadow::overlay().style(..)`.
    pub fn shadow(mut self, style: &str) -> ModalOptions {
        self.shadow = Some(style.to_string());
        self
    }

    /// The whole screen below restyled in `style` while the box is open
    /// (`"dim"`).
    pub fn backdrop(mut self, style: &str) -> ModalOptions {
        self.backdrop = Some(style.to_string());
        self
    }
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

impl Layer {
    /// Whether it is a pop-up: Esc that nothing used, or a press outside
    /// it, closes it.
    fn is_popup(&self) -> bool {
        self.anchor.is_some() || self.origin.is_some()
    }
}

/// A change to the stack of screens.
enum Nav {
    Push(Build),
    Modal(Size, Size, ModalOptions, Build),
    Popup(Anchor, Placement, Size, Size, ModalOptions, Build),
    /// A pop-up placed from a point of the screen.
    PopupAt(Origin, Offset, ModalOptions, Build),
    Pop,
    Replace(Build),
    /// The command palette, or the help, from the focused node's bindings.
    Palette,
    Help(crate::help::HelpOptions),
    /// Run binding `index` of node `id` (a key sequence's, if `true`), as
    /// its key would.
    Run(NodeId, usize, bool),
}

/// Whether a screen is open over the first one, as a binding's condition
/// reads it ([`Ctx::is_modal`] in a handler).
pub(crate) fn over_first() -> bool {
    Runtime::current().layers.get() > 1
}

impl Ctx {
    pub(crate) fn new(proxy: Proxy) -> Ctx {
        Ctx {
            quit: false,
            toasts: Vec::new(),
            animations: Vec::new(),
            copies: Vec::new(),
            announcements: Vec::new(),
            pointer: None,
            focus: None,
            proxy,
            nav: Vec::new(),
            theme: None,
            exit: None,
            error: None,
            printed: Vec::new(),
            handoffs: Vec::new(),
            afters: Vec::new(),
            layers: 1,
        }
    }

    /// Whether the handler runs on a screen, modal or pop-up opened over
    /// the first screen (one [`pop`](Self::pop) would close): a component
    /// that closes itself on Esc, such as a
    /// [`fuzzy_list`](crate::finder::fuzzy_list), leaves Esc to the app on
    /// the first screen.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let said = signal(String::new());
    ///     text!("{said}").on_key("m", move |cx| {
    ///         said.set(format!("modal: {}", cx.is_modal()));
    ///         cx.modal(Size::Fixed(10), Size::Fixed(3), move || {
    ///             label("over").on_key("m", move |cx| said.set(format!("modal: {}", cx.is_modal())))
    ///         });
    ///     })
    /// });
    /// let mut driver = app.driver(20, 5);
    /// driver.press("m");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "modal: false");
    /// driver.press("m");
    /// assert!(driver.screen().plain()[0].starts_with("modal: true"));
    /// ```
    pub fn is_modal(&self) -> bool {
        self.layers > 1
    }

    /// Run `then` once, `wait` from now on the app's clock: [`after`] from
    /// a handler.
    #[track_caller]
    pub fn after(&mut self, wait: Duration, then: impl FnOnce(&mut Ctx) + 'static) {
        self.afters.push(After {
            wait,
            run: Box::new(then),
            at: Location::caller(),
        });
    }

    /// Give the terminal to `command` (an editor, a pager, a shell) once
    /// this handler returns: the app leaves the alternate screen and raw
    /// mode, runs it and waits for it, then takes the terminal back and
    /// draws everything again. Yazi's Enter on a file. Under the headless
    /// driver the program is recorded, not run. A loop of your own takes it
    /// with [`Driver::take_handoff`].
    ///
    /// ```
    /// use intuituive::interact::headless::{Headless, Script};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("notes.txt")
    ///         .on_key("enter", |cx| cx.handoff(std::process::Command::new("vi")))
    ///         .on_key("q", |cx| cx.quit())
    /// });
    /// let mut backend = Headless::new(Script::new().keys("enter q"), 20, 2);
    /// let record = backend.record();
    /// app.run_on(&mut backend).unwrap();
    /// assert_eq!(record.borrow().handoffs, ["vi"]);
    /// ```
    pub fn handoff(&mut self, command: std::process::Command) {
        self.handoffs.push(Handoff {
            command,
            then: None,
        });
    }

    /// [`handoff`](Self::handoff), then run `then` with how the program
    /// ended (its exit code, `None` when a signal ended it, or why it could
    /// not run), back on the app's thread with the terminal the app's again.
    pub fn handoff_then(
        &mut self,
        command: std::process::Command,
        then: impl FnOnce(io::Result<Option<i32>>, &mut Ctx) + 'static,
    ) {
        self.handoffs.push(Handoff {
            command,
            then: Some(Box::new(then)),
        });
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
        self.modal_with(width, height, ModalOptions::default(), build);
    }

    /// A [`modal`](Self::modal) with a shadow or a backdrop, as `options`
    /// says.
    pub fn modal_with(
        &mut self,
        width: Size,
        height: Size,
        options: ModalOptions,
        build: impl FnOnce() -> Node + 'static,
    ) {
        self.nav
            .push(Nav::Modal(width, height, options, Box::new(build)));
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
        self.popup_with(
            anchor,
            placement,
            width,
            height,
            ModalOptions::default(),
            build,
        );
    }

    /// A [`popup`](Self::popup) with a shadow or a backdrop, as `options`
    /// says.
    pub fn popup_with(
        &mut self,
        anchor: impl Into<Anchor>,
        placement: Placement,
        width: Size,
        height: Size,
        options: ModalOptions,
        build: impl FnOnce() -> Node + 'static,
    ) {
        self.nav.push(Nav::Popup(
            anchor.into(),
            placement,
            width,
            height,
            options,
            Box::new(build),
        ));
    }

    /// Open a pop-up placed from a point of the screen rather than next to
    /// a node: `origin` (a corner, an edge's middle, the centre, or below a
    /// rectangle), moved and sized by `offset`, and kept on the screen
    /// however large or far the offset (moved back inside, made smaller).
    /// It takes the keys like a modal; Esc that nothing used, or a press
    /// outside it, closes it. Yazi's input is at the top centre:
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{Offset, Origin};
    ///
    /// let app = App::new(|| {
    ///     label("files").on_key("r", |cx| {
    ///         let name = signal("notes.txt".to_string());
    ///         cx.popup_at(Origin::TopCenter, Offset::new(0, 2, 20, 3), move || {
    ///             input(name).panel("Rename:")
    ///         })
    ///     })
    /// });
    /// let mut driver = app.driver(40, 8);
    /// driver.press("r");
    /// let top: String = driver.screen().plain()[2].chars().skip(10).take(20).collect();
    /// assert_eq!(top, "╭─ Rename: ────────╮");
    /// ```
    pub fn popup_at(
        &mut self,
        origin: Origin,
        offset: Offset,
        build: impl FnOnce() -> Node + 'static,
    ) {
        self.popup_at_with(origin, offset, ModalOptions::default(), build);
    }

    /// A [`popup_at`](Self::popup_at) with a shadow or a backdrop, as
    /// `options` says.
    pub fn popup_at_with(
        &mut self,
        origin: Origin,
        offset: Offset,
        options: ModalOptions,
        build: impl FnOnce() -> Node + 'static,
    ) {
        self.nav
            .push(Nav::PopupAt(origin, offset, options, Box::new(build)));
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
    /// [`App::help_key`] opens it from a key. Only bindings that apply now
    /// are listed ([`bind_when`](crate::Node::bind_when)); a row too long
    /// for the help wraps.
    pub fn help(&mut self) {
        self.help_with(crate::help::HelpOptions::new());
    }

    /// [`help`](Self::help) shown the way `options` say: its title, its
    /// key notation, its layout.
    pub fn help_with(&mut self, options: crate::help::HelpOptions) {
        self.nav.push(Nav::Help(options));
    }

    /// Put `text` on the clipboard, where the terminal lets an app (OSC 52)
    /// or the system's clipboard is reachable; a toast says so once it is
    /// there. A loop of your own gets it from
    /// [`Driver::take_copies`](crate::Driver::take_copies).
    pub fn copy(&mut self, text: impl Into<String>) {
        self.copies.push(text.into());
    }

    /// Show `markup` in a toast for three seconds, at the bottom right (or
    /// in the corner [`App::toasts_at`] names).
    pub fn toast(&mut self, markup: impl Into<String>) {
        self.toast_for(markup, Duration::from_secs(3));
    }

    /// Show `markup` in a toast for `duration`.
    pub fn toast_for(&mut self, markup: impl Into<String>, duration: Duration) {
        self.toasts.push((Note::plain(markup.into()), duration));
    }

    /// Show a notification for five seconds: a box with `title` (markup)
    /// on its top edge and `body` (markup, wrapped) inside, its border and
    /// title in the `level`'s style (the theme's `notify.info`,
    /// `notify.warn` or `notify.error`, else `accent`, `warn` or `bad`). It
    /// stacks with the toasts, in the corner [`App::toasts_at`] names, and
    /// is announced as `Warning: title: body`.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{Corner, Level};
    ///
    /// let app = App::new(|| {
    ///     label("files").on_key("d", |cx| cx.notify(Level::Warn, "Trash", "3 files moved"))
    /// })
    /// .toasts_at(Corner::TopRight);
    /// let mut driver = app.driver(40, 6);
    /// driver.press("d");
    /// let rows = driver.screen().plain();
    /// assert!(rows[0].ends_with("╭─ Trash ───────╮ "));
    /// assert!(rows[1].ends_with("│ 3 files moved │ "));
    /// ```
    pub fn notify(&mut self, level: Level, title: impl Into<String>, body: impl Into<String>) {
        self.notify_for(level, title, body, Duration::from_secs(5));
    }

    /// [`notify`](Self::notify), shown for `duration`.
    pub fn notify_for(
        &mut self,
        level: Level,
        title: impl Into<String>,
        body: impl Into<String>,
        duration: Duration,
    ) {
        let note = Note {
            markup: body.into(),
            notice: Some((level, title.into())),
        };
        self.toasts.push((note, duration));
    }

    /// Say `text` to assistive technology (a screen reader through the
    /// app's [`Announcer`](crate::a11y::Announcer), a browser's live
    /// region) without showing anything: when it is idle, or at once if
    /// `urgent`. Toasts are announced already.
    pub fn announce(&mut self, text: impl Into<String>, urgent: bool) {
        self.announcements.push(crate::a11y::Announcement {
            text: text.into(),
            urgent,
        });
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

    /// End the app after this event with `value`, which
    /// [`App::run_for`] returns (and [`Driver::take_exit`] gives a loop of
    /// your own): the choice a picker was opened for.
    pub fn quit_with<T: 'static>(&mut self, value: T) {
        self.exit = Some(Box::new(value));
        self.quit = true;
    }

    /// End the app after this event with `error`, which [`App::run`] (and
    /// its kin) then returns, once the terminal is given back. An
    /// [`io::Error`] keeps its kind; anything else is
    /// [`io::ErrorKind::Other`].
    pub fn fail(&mut self, error: impl Into<Box<dyn std::error::Error + Send + Sync>>) {
        let error = match error.into().downcast::<io::Error>() {
            Ok(error) => *error,
            Err(other) => io::Error::other(other),
        };
        self.error = Some(error);
        self.quit = true;
    }

    /// Print console `markup` above the app: inline, into the terminal's
    /// scrollback above the app's rows, which move down (ratatui's
    /// `insert_before`); full-screen, once the app ends and the terminal
    /// is back. A loop of your own takes them with
    /// [`Driver::take_printed`].
    pub fn print_above(&mut self, markup: impl Into<String>) {
        self.printed.push(markup.into());
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
/// own as `border`, `border.focused` and `title` (and `tooltip` and
/// `drop.target`, reverse unless a theme sets them), so an app restyles by
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

    /// The named style `name`, or `fallback` when the theme has none.
    pub(crate) fn named(&self, name: &str, fallback: &str) -> Style {
        self.styles
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, style)| style.clone())
            .unwrap_or_else(|| style(fallback))
    }

    /// The style for a node in [error](crate::Node::error_when): the
    /// theme's style named `name` (`error`, or `border.error` for a
    /// panel's border), else its `bad` style, else bold red.
    pub(crate) fn error_style(&self, name: &str) -> Style {
        self.styles
            .iter()
            .find(|(n, _)| n == name)
            .or_else(|| self.styles.iter().find(|(n, _)| n == "bad"))
            .map(|(_, style)| style.clone())
            .unwrap_or_else(|| style("bold red"))
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
    /// Where it was made, for the inspector's trace.
    at: &'static Location<'static>,
}

impl Timer {
    /// A zero interval would never move the timer on (and spin the loop),
    /// so the shortest interval is one millisecond.
    fn new(
        interval: Duration,
        tick: impl FnMut(&mut Ctx) + 'static,
        at: &'static Location<'static>,
    ) -> Timer {
        let every = interval.max(Duration::from_millis(1));
        Timer {
            every,
            next: every,
            tick: Box::new(tick),
            at,
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
    /// A modal's shadow and backdrop.
    look: ModalOptions,
    /// Where a modal was last drawn.
    drawn: Option<Rect>,
    /// For a pop-up: what it is next to, and on which side.
    anchor: Option<(Anchor, Placement)>,
    /// For a pop-up placed from a point of the screen: where.
    origin: Option<(Origin, Offset)>,
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
#[track_caller]
pub fn every(interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) {
    let at = Location::caller();
    BUILDING.with(|building| {
        building
            .borrow_mut()
            .as_mut()
            .expect("every() is called inside App::new's closure or a screen's")
            .push(Timer::new(interval, tick, at))
    });
}

/// Call `then` once, `wait` from now on the app's clock (the time a
/// [`Driver`] is given, so a test steps it exactly): clear a message after
/// a while, retry later, give up on something slow. Call it while the app
/// is built or running: in [`App::new`]'s closure, a handler, a watch.
/// [`Ctx::after`] does the same from a handler's `cx`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::after;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let message = signal("Saved");
///     after(Duration::from_secs(2), move |_| message.set(""));
///     text!("{message}")
/// });
/// let mut driver = app.driver(10, 1);
/// driver.advance(Duration::from_millis(1900));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Saved");
/// driver.advance(Duration::from_millis(100));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn after(wait: Duration, then: impl FnOnce(&mut Ctx) + 'static) {
    let at = Location::caller();
    Runtime::current_for("after()")
        .afters
        .borrow_mut()
        .push(After {
            wait,
            run: Box::new(then),
            at,
        });
}

/// How many times a second `source` is written, counted over each second
/// of the app's clock (and 0 for the first): a frame-rate meter. Each
/// write counts, an equal value too, as [`on_write`](crate::on_write)
/// sees them. Call it inside [`App::new`]'s closure (or a screen's), as
/// [`every`].
///
/// ```
/// use intuituive::prelude::*;
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let frame = signal(());
///     every(Duration::from_millis(100), move |_| frame.update(|_| {}));
///     let fps = rate(frame);
///     text!("{fps} fps")
/// });
/// let mut driver = app.driver(10, 1);
/// for tenth in 1..=10 {
///     driver.update(Duration::from_millis(100 * tenth));
/// }
/// driver.render();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "10 fps");
/// ```
///
/// # Panics
/// Outside `App::new`'s closure or a screen's, or for a signal of another
/// app.
#[track_caller]
pub fn rate<T: 'static>(source: Signal<T>) -> Signal<u32> {
    let shown = signal(0u32);
    let count = crate::reactive::count_writes(source);
    every(Duration::from_secs(1), move |_| shown.set(count.replace(0)));
    shown
}

/// Counters from the last frame, for tests and benchmarks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameStats {
    /// Nodes that drew.
    pub drawn: usize,
    /// Bytes written to the terminal.
    pub bytes: usize,
    /// Frames drawn since the app started, this one included: ratatui's
    /// `Frame::count`.
    pub frames: u64,
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
    /// Toasts showing, with when each goes, and the corner they stack in.
    toasts: Vec<(Note, Duration)>,
    toast_corner: Corner,
    /// The widest a notification grows.
    notify_width: Size,
    /// Animations running: the signal, from, to, start, length, easing.
    animations: Vec<Animation>,
    /// One-shot timers ([`after`]), with when each is due.
    timeouts: Vec<Timeout>,
    /// Terminal graphics placed on the cells ([`App::graphics`]), and the
    /// placements the last frame drew.
    graphics: Option<std::sync::Arc<dyn rich_ext::frame::PlacementSource>>,
    placements: Vec<rich_ext::frame::Placement>,
    /// Keys that open the command palette and the help, if any.
    palette_key: Vec<Key>,
    help_key: Vec<Key>,
    /// Read keys as a legacy terminal sends them, even where the kitty
    /// keyboard protocol could be on.
    legacy_keys: bool,
    /// Synchronized output forced on or off; `None` detects it.
    synchronized_output: Option<bool>,
    /// Selecting text with the mouse (on unless turned off): where the
    /// press was and where the drag is now.
    selectable: bool,
    selection: Option<((u16, u16), (u16, u16))>,
    /// A drag is selecting.
    selecting: bool,
    /// Where the mouse event being routed happened.
    pointer: Option<(u16, u16)>,
    /// Whether [`run`](Self::run) captures the mouse; `None` is the rule
    /// (full-screen yes, inline no).
    mouse: Option<bool>,
    /// What the app ended with, a value or an error, and lines printed
    /// above it not yet written.
    exit: Option<Box<dyn std::any::Any>>,
    error: Option<io::Error>,
    printed: Vec<String>,
    /// Where the pointer last was, for widgets that ask.
    last_pointer: Option<(u16, u16)>,
    /// The tooltip waiting for the pointer to rest, or showing.
    tip: Option<Tip>,
    /// A press on a draggable node, or the drag it became.
    drag: Option<Drag>,
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
    sheet: Option<SheetSource>,
    /// For assistive technology: whether to draw for it, or write lines
    /// for it (linear mode), where announcements go, those not yet taken,
    /// those linear mode has still to write, and whether a screen just
    /// opened.
    accessible: bool,
    linear: bool,
    announcer: Option<Box<dyn crate::a11y::Announcer>>,
    announcements: Vec<crate::a11y::Announcement>,
    unwritten: Vec<String>,
    opened: bool,
    /// Accessible: where the cursor was parked by the last frame.
    caret_at: Option<(u16, u16)>,
    /// Draw a frame even if no node is dirty (the inspector has news).
    poke: bool,
    /// The stylesheet generation the last frame matched nodes against.
    sheet_drawn: Option<u64>,
    /// Programs handlers asked to give the terminal to, not yet run.
    handoffs: Vec<Handoff>,
    /// A key sequence under way, the which-key pop-up's delay (`None`:
    /// off), and where it was last drawn.
    pending: Option<crate::sequence::Pending>,
    which_key: Option<Duration>,
    which_key_at: Option<Rect>,
    /// While the inspector traces: the node that took the event being
    /// routed, and how.
    used: crate::inspect::Used,
}

/// A theme file the app reloads when it changes.
/// The app's stylesheet, and the file it comes from when it is read live.
struct SheetSource {
    sheet: crate::Stylesheet,
    /// A new number each time it changes, so nodes match it again.
    generation: u64,
    file: Option<ThemeFile>,
    /// With `@media` rules: the generation and media key the number nodes
    /// match against was made for, and that number. A viewport that
    /// crosses a rule's bounds makes a new one.
    media: Option<(u64, Vec<bool>, u64)>,
}

impl SheetSource {
    fn new(sheet: crate::Stylesheet, file: Option<ThemeFile>) -> SheetSource {
        SheetSource {
            sheet,
            generation: sheet_generation(),
            file,
            media: None,
        }
    }

    /// The number nodes match against at `viewport`: the sheet's
    /// generation, or one for the `@media` blocks that hold there.
    fn generation_at(&mut self, viewport: (u16, u16)) -> u64 {
        if !self.sheet.has_media() {
            return self.generation;
        }
        let key = self.sheet.media_key(viewport);
        match &self.media {
            Some((base, held, generation)) if *base == self.generation && *held == key => {
                *generation
            }
            _ => {
                let generation = sheet_generation();
                self.media = Some((self.generation, key, generation));
                generation
            }
        }
    }
}

/// A number no stylesheet had before.
fn sheet_generation() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

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

/// An app dropped without [`Driver::finish`] (a test that stops early, a
/// driver let go) closes its runtime all the same: `latest` threads asleep
/// on its clock wake and end, and its signals go.
impl Drop for App {
    fn drop(&mut self) {
        self.runtime.close();
    }
}

impl App {
    /// Build the app: `build` runs once, creates the app's signals and
    /// returns the root node.
    pub fn new(build: impl FnOnce() -> Node) -> App {
        let runtime = Runtime::new();
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        WATCHES.with(|watches| *watches.borrow_mut() = Some(Vec::new()));
        let (root, focus_path, hover_path) = runtime.enter(|| {
            // The trace says when the focus moves and the pointer acts.
            let focus_path = signal(Vec::new()).untraced();
            let hover_path = signal(Vec::new()).untraced();
            // Made now, so a node closure reading it later makes nothing.
            crate::reactive::viewport();
            (build(), focus_path, hover_path)
        });
        let timers = BUILDING
            .with(|building| building.borrow_mut().take())
            .unwrap_or_default();
        let watches = WATCHES
            .with(|watches| watches.borrow_mut().take())
            .unwrap_or_default();
        let (accessible, linear) = crate::a11y::from_env();
        App {
            runtime,
            layers: vec![Layer {
                root,
                focus: None,
                modal: None,
                look: ModalOptions::default(),
                drawn: None,
                anchor: None,
                origin: None,
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
            tip: None,
            drag: None,
            focused: None,
            lifecycle: Vec::new(),
            clipboard: false,
            copies: Vec::new(),
            toasts: Vec::new(),
            toast_corner: Corner::default(),
            notify_width: Size::Fixed(crate::notify::NOTICE_WIDTH),
            animations: Vec::new(),
            timeouts: Vec::new(),
            graphics: None,
            placements: Vec::new(),
            palette_key: Vec::new(),
            help_key: Vec::new(),
            legacy_keys: false,
            synchronized_output: None,
            selectable: true,
            selection: None,
            selecting: false,
            pointer: None,
            mouse: None,
            exit: None,
            error: None,
            printed: Vec::new(),
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
            sheet: None,
            accessible,
            linear,
            announcer: None,
            announcements: Vec::new(),
            unwritten: Vec::new(),
            opened: false,
            caret_at: None,
            poke: false,
            sheet_drawn: None,
            handoffs: Vec::new(),
            pending: None,
            which_key: None,
            which_key_at: None,
            used: None,
        }
    }

    /// [`new`](Self::new), with a `build` that can fail: a sound card that
    /// is not there, a file that does not open. The error is returned here,
    /// before anything runs, so the terminal is never taken: `main` reports
    /// it as any other, with `?`. Signals, watches and tasks `build` made
    /// before it failed are dropped with the app; `latest` threads, which
    /// start when the app runs, never start.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let opened = App::try_new(|| {
    ///     let text = std::fs::read_to_string("/no/such/file")?;
    ///     Ok::<_, std::io::Error>(label(text))
    /// });
    /// assert!(opened.is_err());
    /// let app = App::try_new(|| Ok::<_, std::io::Error>(label("fine"))).unwrap();
    /// assert_eq!(app.render_with(&["ctrl+c"], 4, 1).unwrap()[0], "fine");
    /// ```
    pub fn try_new<E>(build: impl FnOnce() -> Result<Node, E>) -> Result<App, E> {
        let mut failed = None;
        let app = App::new(|| match build() {
            Ok(root) => root,
            Err(error) => {
                failed = Some(error);
                crate::node::label("")
            }
        });
        match failed {
            Some(error) => {
                app.runtime.close();
                Err(error)
            }
            None => Ok(app),
        }
    }

    /// Dock the [inspector](crate::inspect) on the right: the node tree,
    /// what drew in the last frame, the focus and the frame's cost, a
    /// selected node's layout and states, each node's timings and a trace
    /// of what happened. F12 shows and hides it; Shift+F12 gives it the
    /// keys. Setting `INTUITUIVE_INSPECT=1` turns it on for any app
    /// without a code change.
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

    /// Style and lay out the app's nodes with a stylesheet: a CSS subset
    /// (see [`Stylesheet`](crate::Stylesheet)). Builders in code win over
    /// it, as an inline style beats CSS. A sheet that does not parse is
    /// reported in a toast, and the app runs without it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     column([label("Status").name("bar"), label("body")]).on_key("q", |cx| cx.quit())
    /// })
    /// .stylesheet("#bar { dock: bottom; size: 1; }");
    /// let screen = app.render_with(&["q"], 10, 3).unwrap();
    /// assert_eq!(screen[2].trim_end(), "Status");
    /// ```
    pub fn stylesheet(mut self, css: &str) -> App {
        match crate::Stylesheet::parse(css) {
            Ok(sheet) => {
                self.sheet = Some(SheetSource::new(sheet, None));
            }
            Err(error) => self.sheet_error(&error.to_string()),
        }
        self
    }

    /// [`stylesheet`](Self::stylesheet) from a file, read again whenever it
    /// changes while the app runs. A change that does not parse is
    /// reported in a toast, and the last good sheet stays.
    pub fn stylesheet_file(mut self, path: impl Into<std::path::PathBuf>) -> App {
        self.sheet = Some(SheetSource::new(
            crate::Stylesheet::default(),
            Some(ThemeFile {
                path: path.into(),
                stamp: None,
                good: None,
                read: None,
                error: None,
            }),
        ));
        self.reload_sheet();
        self
    }

    /// Say in a toast that the stylesheet does not parse.
    fn sheet_error(&mut self, error: &str) {
        let markup = format!("[bad]stylesheet:[/] {}", rich::markup::escape(error));
        self.push_toast(Note::plain(markup), self.now + Duration::from_secs(6));
    }

    /// Show a toast until `until`, and announce it.
    fn push_toast(&mut self, note: Note, until: Duration) {
        let text = note.spoken();
        self.toasts.push((note, until));
        self.poke = true;
        self.announce(
            crate::a11y::Announcement {
                text,
                urgent: false,
            },
            true,
        );
    }

    /// Hand `announcement` to the announcer and keep it for
    /// [`Driver::take_announcements`] (the last 64). In linear mode it is
    /// also written, if `write`: a live node's change and a screen opening
    /// are not, as their lines say the same.
    fn announce(&mut self, announcement: crate::a11y::Announcement, write: bool) {
        if let Some(announcer) = &mut self.announcer {
            announcer.announce(&announcement);
        }
        if self.linear && write && !announcement.text.is_empty() {
            self.unwritten.push(announcement.text.clone());
        }
        if self.announcements.len() >= 64 {
            self.announcements.remove(0);
        }
        self.announcements.push(announcement);
    }

    /// Draw for assistive technology: the terminal's cursor on what has the
    /// focus (its caret, else its selected item, else its corner), boxes
    /// as blanks, no colour, a `>` on selected items, no animation. The
    /// cursor is hidden while a frame is written and shown once it is back
    /// on the focus; a frame that changes nothing does not move it. On by
    /// default when `INTUITUIVE_ACCESSIBLE` is set (to anything but `0`;
    /// `linear` turns on [linear mode](Self::linear) instead), or rs-rich's
    /// `RICH_A11Y` names `screen-reader`.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let picked = signal(1usize);
    ///     list(|| vec!["one".into(), "two".into()], picked).on_key("q", |cx| cx.quit())
    /// })
    /// .accessible(true);
    /// let screen = app.render_with(&["q"], 10, 2).unwrap();
    /// assert_eq!(screen[1].trim_end(), "> two");
    /// ```
    pub fn accessible(mut self, on: bool) -> App {
        self.accessible = on;
        self
    }

    /// Linear mode, for screen reader users: no screen is drawn. The
    /// accessibility tree is written as lines of plain text, in reading
    /// order ([`AccessNode::describe`](crate::a11y::AccessNode::describe)),
    /// and after each event only the lines of what changed, like a
    /// transcript, which a screen reader reads as it arrives. The focus
    /// moving is written as `→ ` and the line of what has it; toasts and
    /// [`Ctx::announce`] as their text. There is no cursor addressing, no
    /// alternate screen, no mouse, no colour and no synchronized output;
    /// keys work as usual. On by default when `INTUITUIVE_ACCESSIBLE` is
    /// `linear`.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let picked = signal(0usize);
    ///     list(|| vec!["one".into(), "two".into()], picked)
    ///         .label("Numbers")
    ///         .on_key("q", |cx| cx.quit())
    /// })
    /// .linear(true);
    /// let mut driver = app.driver(20, 4);
    /// driver.update(std::time::Duration::ZERO);
    /// let out = driver.render().unwrap();
    /// assert_eq!(out, "→ Numbers, list, 1 of 2: one, selected\r\n");
    /// ```
    pub fn linear(mut self, on: bool) -> App {
        self.linear = on;
        self
    }

    /// Whether frames are drawn as text for assistive technology: no boxes,
    /// no colour, markers on selected items, no animation.
    fn text_mode(&self) -> bool {
        self.accessible || self.linear
    }

    /// Where announcements go as they happen: toasts, a screen or dialog
    /// opening, [live](crate::Node::live) nodes changing, and
    /// [`Ctx::announce`]. [`Driver::take_announcements`] has them too.
    pub fn announcer(mut self, announcer: impl crate::a11y::Announcer + 'static) -> App {
        self.announcer = Some(Box::new(announcer));
        self
    }

    /// Read the stylesheet file again if it changed since it was last read.
    fn reload_sheet(&mut self) {
        let Some(SheetSource {
            file: Some(file), ..
        }) = &mut self.sheet
        else {
            return;
        };
        let stamp = std::fs::metadata(&file.path)
            .ok()
            .map(|m| (m.modified().unwrap_or(std::time::UNIX_EPOCH), m.len()));
        if stamp == file.stamp && (stamp.is_some() || file.error.is_some()) {
            // As for the theme file: a fresh file is looked at, in case a
            // same-length edit landed within one tick.
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
        let parsed = read.and_then(|text| {
            crate::Stylesheet::parse(&text)
                .map(|sheet| (sheet, text))
                .map_err(|e| e.to_string())
        });
        match parsed {
            Ok((sheet, text)) => {
                file.good = Some(text);
                file.error = None;
                let source = self.sheet.as_mut().expect("a stylesheet");
                source.sheet = sheet;
                source.generation = sheet_generation();
                self.restack = true;
            }
            Err(error) => {
                let repeated = file.error.as_ref() == Some(&error);
                file.error = Some(error.clone());
                if !repeated {
                    self.sheet_error(&error);
                }
            }
        }
    }

    /// Call `tick` every `interval` (a clock, a poll, an animation).
    #[track_caller]
    pub fn every(mut self, interval: Duration, tick: impl FnMut(&mut Ctx) + 'static) -> App {
        let at = Location::caller();
        self.layers[0].timers.push(Timer::new(interval, tick, at));
        self
    }

    /// Run inline: in `height` rows below the cursor (fewer if the
    /// terminal is shorter), rather than on the alternate screen. What the
    /// app last showed stays in the scrollback when it ends, like a
    /// command's output. The mouse is left to the terminal, so its
    /// scrollback and selection keep working (see [`mouse`](Self::mouse)).
    /// [`Ctx::print_above`] prints lines into the scrollback above it.
    pub fn inline(mut self, height: u16) -> App {
        self.inline = Some(height.max(1));
        self
    }

    /// Whether [`run`](Self::run) captures the mouse: `Some(true)` even
    /// inline (clicks, the wheel and hover reach the app, and the
    /// terminal's own selection needs Shift), `Some(false)` never, even
    /// full-screen (the terminal keeps its selection and scrollback).
    /// `None`, the default, captures it full-screen and leaves it to the
    /// terminal inline.
    pub fn mouse(mut self, capture: Option<bool>) -> App {
        self.mouse = capture;
        self
    }

    /// Whether the app captures the mouse, by [`mouse`](Self::mouse) and
    /// the rule.
    fn captures_mouse(&self) -> bool {
        self.mouse.unwrap_or(self.inline.is_none() && !self.linear)
    }

    /// The terminal options [`run_with`](Self::run_with) starts its
    /// session with on `backend`: the alternate screen unless inline or
    /// linear, the mouse as [`mouse`](Self::mouse) says, bracketed paste,
    /// the keyboard and synchronized output as set. For a loop of your own
    /// that starts its own [`Session`].
    pub fn session_options(&self, backend: BackendKind) -> SessionOptions {
        // Linear, lines are written where the terminal's cursor is.
        let full = self.inline.is_none() && !self.linear;
        SessionOptions {
            alternate_screen: full,
            mouse: self.captures_mouse(),
            bracketed_paste: true,
            output: Default::default(),
            legacy_keys: self.legacy_keys,
            backend,
            // Linear, lines are appended as they come: nothing to hold.
            synchronized_output: if self.linear {
                Some(false)
            } else {
                self.synchronized_output
            },
        }
    }

    /// Before each event, wait for the [tasks](crate::spawn) in flight to
    /// deliver their results. For tests, which then see a task's result on
    /// the next frame however fast the machine is; off by default. It also
    /// puts [`latest`](crate::latest) threads that keep a beat with
    /// [`Latest::sleep`](crate::Latest::sleep) (and
    /// [`latest_every`](crate::latest_every)) on the app's clock, and
    /// waits each turn until those due have sent their values and sleep
    /// again: a thread-fed app is the same frame by frame on every run.
    /// A `latest` thread that waits on something else (a pipe) holds each
    /// turn up to 5 seconds.
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

    /// Stack toasts and [notifications](Ctx::notify) in `corner`, newest
    /// nearest it: [`Corner::BottomRight`] by default, Yazi's
    /// [`Corner::TopRight`].
    pub fn toasts_at(mut self, corner: Corner) -> App {
        self.toast_corner = corner;
        self
    }

    /// Draw terminal graphics (Kitty, iTerm2 and Sixel images) beside the
    /// cells: after each frame's cells are written, `source` finds the
    /// [placements](rich_ext::frame::Placement) on the screen's cells, and
    /// those that are new, moved, on another animation frame or wiped by
    /// cells written over them are drawn again (rs-rich-ext's
    /// [`plan`](rich_ext::frame::plan), as its `Surface::graphics` does);
    /// an overlay that went away has its cells repainted. The cells under a
    /// placement are what a terminal without graphics shows, so the frame
    /// stays exact either way. When the app ends, `source` releases what
    /// the terminal holds. Choosing a protocol the terminal supports is the
    /// source's.
    pub fn graphics(mut self, source: std::sync::Arc<dyn rich_ext::frame::PlacementSource>) -> App {
        self.graphics = Some(source);
        self
    }

    /// The widest a [notification](Ctx::notify) grows: 48 cells by
    /// default, or a share of the screen ([`Size::Percent`]); never wider
    /// than the screen. A notification is as wide as its body's longest
    /// line and its title within that, and wraps beyond it.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::{Corner, Level};
    ///
    /// let error = "Failed to rename notes.txt: permission denied (os error 13)";
    /// let app = App::new(move || label("").on_key("e", move |cx| cx.notify(Level::Error, "Rename", error)))
    ///     .toasts_at(Corner::TopRight)
    ///     .notify_width(Size::Percent(90));
    /// let mut driver = app.driver(80, 6);
    /// driver.press("e");
    /// // One line: the box's body row, then its bottom edge.
    /// assert!(driver.screen().plain()[1].contains(error));
    /// assert!(driver.screen().plain()[2].contains("╰───"));
    /// ```
    pub fn notify_width(mut self, width: Size) -> App {
        self.notify_width = width;
        self
    }

    /// Show the which-key pop-up while a [key
    /// sequence](crate::Node::bind_sequence) is under way: what can follow
    /// the keys pressed so far, with their descriptions, in up to three
    /// columns in a box at the bottom of the screen (Yazi's). It shows at
    /// once; [`which_key_delay`](Self::which_key_delay) waits first. Off by
    /// default. Its keys are in the theme's `which_key.key` style (else
    /// `accent`), the two cells after them in `which_key.sep`, and the
    /// descriptions in `which_key.desc`; a description too long for its
    /// column ends in `…`, two cells short of the next column's keys.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     label("files")
    ///         .bind_sequence("g g", "Go to the top", |_| {})
    ///         .bind_sequence("g h", "Go home", |_| {})
    /// })
    /// .which_key(true);
    /// let mut driver = app.driver(44, 6);
    /// driver.press("g");
    /// let rows = driver.screen().plain();
    /// assert!(rows[3].contains("g  Go to the top") && rows[3].contains("h  Go home"));
    /// ```
    pub fn which_key(mut self, on: bool) -> App {
        self.which_key = on.then_some(Duration::ZERO);
        self
    }

    /// Turn the [which-key](Self::which_key) pop-up on, shown once a
    /// sequence has waited `delay` for its next key (which-key.nvim's
    /// habit: a sequence typed quickly shows nothing).
    pub fn which_key_delay(mut self, delay: Duration) -> App {
        self.which_key = Some(delay);
        self
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

    /// Whether [`run`](Self::run) writes each frame as one synchronized
    /// update (DEC private mode 2026), which the terminal shows whole:
    /// `Some(true)` always, `Some(false)` never. `None`, the default, leaves
    /// it to `RICH_SYNC_OUTPUT` (`0` off, `1` on) and then to the terminal,
    /// asked at start-up with the kitty keyboard query (not asked with
    /// [`legacy_keys`](Self::legacy_keys)). See
    /// [`SessionOptions::synchronized_output`].
    pub fn synchronized_output(mut self, on: Option<bool>) -> App {
        self.synchronized_output = on;
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
    /// Ctrl+C is pressed. The terminal is driven with crossterm; see
    /// [`run_with`](Self::run_with) for the others.
    pub fn run(self) -> io::Result<()> {
        self.run_with(BackendKind::default())
    }

    /// [`run`](Self::run), with the terminal driven by `backend`: crossterm
    /// (what `run` uses), or termion or termwiz with this crate's features
    /// of those names.
    ///
    /// ```no_run
    /// use intuituive::interact::BackendKind;
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("hello").on_key("q", |cx| cx.quit()));
    /// app.run_with(BackendKind::Crossterm)?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn run_with(self, backend: BackendKind) -> io::Result<()> {
        self.run_terminal(backend).map(|_| ())
    }

    /// [`run`](Self::run), returning the value a handler ended the app with
    /// ([`Ctx::quit_with`]): `None` when it ended otherwise, or with a value
    /// that is not a `T`.
    ///
    /// ```no_run
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let selected = signal(0usize);
    ///     let items = || vec!["red".to_string(), "green".into(), "blue".into()];
    ///     list(items, selected).on_key("enter", move |cx| cx.quit_with(selected.get()))
    /// });
    /// let picked: Option<usize> = app.run_for()?;
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn run_for<T: 'static>(self) -> io::Result<Option<T>> {
        let exit = self.run_terminal(BackendKind::default())?;
        Ok(exit.and_then(|value| value.downcast::<T>().ok().map(|value| *value)))
    }

    /// Run in a terminal session on `backend`; what the app ended with.
    fn run_terminal(self, backend: BackendKind) -> io::Result<Option<Box<dyn std::any::Any>>> {
        let mut session = Session::start(self.session_options(backend))?;
        if self.linear {
            // The cursor stays at the end of what was written.
            session.write("\x1b[?25h")?;
        }
        let mut app = self;
        app.text_frames = false;
        let ended = app.run_loop(&mut session, false);
        // Lines printed above a full-screen app go to the terminal once it
        // is back.
        drop(session);
        if !ended.printed.is_empty() {
            use std::io::Write;
            let mut out = io::stdout().lock();
            for row in &ended.printed {
                let _ = writeln!(out, "{row}\x1b[0m");
            }
            let _ = out.flush();
        }
        ended.result
    }

    /// Run on `backend`: a terminal session, or the headless driver. The
    /// app owns the loop: it waits for events, runs timers and draws. To
    /// own the loop yourself, use [`driver`](Self::driver), which this is
    /// built on.
    ///
    /// On a backend that [can suspend](Backend::can_suspend) (a terminal
    /// on Unix), Ctrl+Z that no binding on the focused path uses suspends
    /// the app as the shell expects, and `fg` draws it again.
    pub fn run_on(self, backend: &mut impl Backend) -> io::Result<()> {
        self.run_on_for::<()>(backend).map(|_| ())
    }

    /// [`run_on`](Self::run_on), returning the value a handler ended the
    /// app with ([`Ctx::quit_with`]), if it is a `T`. Lines printed above a
    /// full-screen app ([`Ctx::print_above`]) are written to `backend`
    /// after the app's last frame.
    ///
    /// ```
    /// use intuituive::interact::headless::{Headless, Script};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("pick").on_key("y", |cx| cx.quit_with("yes")));
    /// let mut backend = Headless::new(Script::new().keys("y"), 10, 1);
    /// assert_eq!(app.run_on_for::<&str>(&mut backend).unwrap(), Some("yes"));
    /// ```
    pub fn run_on_for<T: 'static>(self, backend: &mut impl Backend) -> io::Result<Option<T>> {
        let ended = self.run_loop(backend, true);
        let exit = ended.result?;
        Ok(exit.and_then(|value| value.downcast::<T>().ok().map(|value| *value)))
    }

    /// The loop of [`run_on`](Self::run_on): what the app ended with, and
    /// the lines printed above a full-screen app, written to `backend`
    /// after the last frame when `write_printed` (else for the caller to
    /// write once the terminal is back).
    fn run_loop(self, backend: &mut impl Backend, write_printed: bool) -> Ended {
        let (width, height) = backend.size();
        let inline = self.inline.is_some();
        let text_frames = self.text_frames;
        let linear = self.linear;
        let mut driver = self.driver(width, height);
        driver.set_clipboard(backend.clipboard().is_ok());
        // A value from another thread ends the wait on the terminal.
        if let Some(waker) = backend.waker() {
            driver.set_waker(move || waker.wake());
        }
        // A frame as one synchronized update, where the backend writes
        // them; linear lines as they are, since they are appended and
        // there is no frame to hold. Nothing for output that is empty.
        fn put(backend: &mut impl Backend, linear: bool, out: &str) -> io::Result<()> {
            if out.is_empty() {
                Ok(())
            } else if linear {
                backend.write(out)
            } else {
                backend.write_frame(out)
            }
        }
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
        // Programs handlers gave the terminal to: run each with the
        // terminal as it was, then draw everything again.
        fn hand_off(
            driver: &mut Driver,
            backend: &mut impl Backend,
            linear: bool,
            inline: bool,
        ) -> io::Result<()> {
            while let Some(mut handoff) = driver.take_handoff() {
                put(backend, linear, &driver.suspend())?;
                let result = backend.handoff(&mut handoff.command);
                if linear {
                    backend.write("\x1b[?25h")?;
                }
                let (columns, rows) = backend.size();
                if inline {
                    driver.set_origin(backend.origin());
                }
                driver.resize(columns, rows);
                driver.handed_back(handoff, result);
            }
            Ok(())
        }
        let result = (|| -> io::Result<()> {
            loop {
                driver.update(backend.elapsed());
                copy_all(&mut driver, backend);
                hand_off(&mut driver, backend, linear, inline)?;
                if driver.is_done() {
                    return Ok(());
                }
                if let Some(out) = driver.render() {
                    put(backend, linear, &out)?;
                    if text_frames {
                        backend.painted(&driver.screen().plain().join("\n"));
                    }
                }
                copy_all(&mut driver, backend);
                let wait = driver.timeout(backend.elapsed());
                let Some(event) = backend.read(Some(wait))? else {
                    continue;
                };
                match &event {
                    // Raw mode turns off the terminal's own Ctrl+Z, so it
                    // arrives as a key: suspend as the terminal would have,
                    // unless the app binds it. On `fg` everything draws
                    // again (inline, in a new region below the shell's
                    // lines).
                    Event::Key(key)
                        if *key == Key::ctrl('z')
                            && backend.can_suspend()
                            && !driver.app.binds(*key) =>
                    {
                        put(backend, linear, &driver.suspend())?;
                        backend.suspend()?;
                        if linear {
                            // The session hides the cursor on the way back;
                            // linear mode reads from where it is.
                            backend.write("\x1b[?25h")?;
                        }
                        let (columns, rows) = backend.size();
                        if inline {
                            driver.set_origin(backend.origin());
                        }
                        driver.resize(columns, rows);
                        continue;
                    }
                    // Back from a suspend started outside (SIGTSTP): what
                    // is on the screen is the shell's, so the resize this
                    // is draws everything, in a new region when inline.
                    Event::Resize { .. } if backend.take_resumed() => {
                        let _ = driver.suspend();
                        if linear {
                            backend.write("\x1b[?25h")?;
                        }
                    }
                    _ => {}
                }
                if inline {
                    driver.set_origin(backend.origin());
                }
                driver.event(event);
                copy_all(&mut driver, backend);
                hand_off(&mut driver, backend, linear, inline)?;
                if driver.is_done() {
                    return Ok(());
                }
            }
        })();
        // What the app ended with: an error from a handler wins over the
        // loop's own end.
        let failed = driver.take_error();
        let exit = driver.app.exit.take();
        // Inline, `finish` prints what is left above the region itself.
        let printed = if inline {
            Vec::new()
        } else {
            driver.take_printed()
        };
        let rows = driver.printed_rows(&printed);
        let _ = put(backend, linear, &driver.finish());
        let mut printed = Vec::new();
        if write_printed {
            let text: String = rows.iter().map(|row| format!("{row}\x1b[0m\r\n")).collect();
            if !text.is_empty() {
                let _ = backend.write(&text);
            }
        } else {
            printed = rows;
        }
        let result = match failed {
            Some(error) => Err(error),
            None => result.map(|()| exit),
        };
        Ended { result, printed }
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
        self.sync_tracer();
        let console = self.console_for(self.columns(width));
        self.last_console = Some(console.clone());
        let mut painter = Painter::new(None);
        if self.inline.is_some() {
            painter = painter.inline();
        }
        // Accessible: no colour, so no meaning rests on it.
        painter.set_color_system(if self.text_mode() {
            None
        } else {
            console.color_system()
        });
        let screen = Screen::new(self.columns(width), self.region(height));
        self.set_viewport(screen.area(), height);
        // Focus what can be focused before the first frame (keyed children
        // appear only once drawn; `keep_focus` catches them after it).
        self.focus_first();
        // `latest` threads start with the app, on its clock in a test.
        self.runtime.start_threads(self.wait_for_tasks);
        Driver {
            app: self,
            painter,
            console,
            screen,
            rows: height,
            first: true,
            started: false,
            origin: 0,
            shifted: 0,
            done: false,
            written: Vec::new(),
            written_focus: None,
        }
    }

    /// Run headless at `width` x `height`, pressing `keys` (key names,
    /// one per step, or several separated by spaces), and return the last
    /// screen's rows as plain text. The app need not quit: once the keys
    /// are pressed, it settles (watches set off by the first frames run,
    /// a [`RectHandle`](crate::RectHandle)'s readers load) and the screen
    /// is read. Keys after a quit are dropped; a handed-off program is
    /// taken to have ended with 0. An error the app
    /// [failed](Ctx::fail) with is returned.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let n = signal(0);
    ///     text!("{n}").on_key("+", move |_| n.update(|n| *n += 1))
    /// });
    /// assert_eq!(app.render_with(&["+ +"], 5, 1).unwrap()[0].trim_end(), "2");
    /// ```
    pub fn render_with(self, keys: &[&str], width: u16, height: u16) -> io::Result<Vec<String>> {
        let mut driver = self.driver(width, height);
        let hand_back = |driver: &mut Driver| {
            while let Some(handoff) = driver.take_handoff() {
                driver.handed_back(handoff, Ok(Some(0)));
                driver.settle();
            }
        };
        driver.settle();
        hand_back(&mut driver);
        for key in keys {
            driver.press(key);
            hand_back(&mut driver);
        }
        driver.settle();
        if let Some(error) = driver.take_error() {
            return Err(error);
        }
        Ok(driver.screen().plain())
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

    /// Tell nodes the app's [`viewport`](crate::viewport()): `screen` (its
    /// columns less the inspector's, while shown) and, in linear mode,
    /// which lays out as tall as its content, the terminal's `rows`.
    fn set_viewport(&self, screen: Rect, rows: u16) {
        let panel = self.panel_width(screen.width).unwrap_or(0);
        let height = if self.linear { rows } else { screen.height };
        self.runtime
            .viewport()
            .set((screen.width.saturating_sub(panel), height));
    }

    /// The columns the inspector's panel takes from a screen `columns`
    /// wide: none while it is closed, or the screen too narrow, or in
    /// linear mode, which draws nothing for it to sit beside.
    fn panel_width(&self, columns: u16) -> Option<u16> {
        if self.linear {
            return None;
        }
        self.inspector
            .as_ref()
            .and_then(|inspector| inspector.width(columns))
    }

    /// Trace while the inspector is open and its trace not paused; off,
    /// the runtime notes nothing.
    fn sync_tracer(&self) {
        let on = !self.linear && self.inspector.as_ref().is_some_and(Inspector::tracing);
        self.runtime.tracer.set_on(on);
    }

    /// Whether the inspector's trace, shown, has news: draw a frame.
    fn trace_news(&self) -> bool {
        self.inspector.as_ref().is_some_and(|inspector| {
            inspector.open && inspector.shown && inspector.pane == crate::inspect::Pane::Trace
        }) && self.runtime.tracer.has_pending()
    }

    /// The node that took the event being routed, and how, for the
    /// inspector's trace.
    fn note_used(&mut self, id: NodeId, how: &'static str) {
        if self.used.is_none() && self.runtime.tracer.on() {
            self.used = Some((id, how));
        }
    }

    /// Whether `event` is the inspector's: F12 and Shift+F12, and every key
    /// and paste while it has the keys. The mouse is sorted in
    /// [`inspector_mouse`](Self::inspector_mouse).
    fn inspector_owns(&self, event: &Event) -> bool {
        let Some(inspector) = self.inspector.as_ref().filter(|_| !self.linear) else {
            return false;
        };
        let keys = inspector.open && (inspector.keys || inspector.pick.is_some());
        match event {
            Event::Key(key) => key.code == KeyCode::F(12) || keys,
            Event::KeyUp(_) | Event::Paste(_) => keys,
            _ => false,
        }
    }

    /// A key while the inspector has the keys.
    fn inspector_key(&mut self, key: Key) {
        let layers = &self.layers;
        let focus = layers.last().and_then(|layer| layer.focus);
        let Some(inspector) = self.inspector.as_mut() else {
            return;
        };
        let hit = |x: u16, y: u16| node_at(layers, x, y);
        let rect = |id: NodeId| node_rect(layers, id);
        let probe = Probe {
            focus,
            area: inspector.area,
            hit: &hit,
            rect: &rect,
        };
        let handled = inspector.key(key, &probe);
        self.after_inspector(handled);
    }

    /// Offer `mouse` to the inspector, while it is shown: picking, or on its
    /// panel. Whether it was the inspector's.
    fn inspector_mouse(&mut self, mouse: rich_interact::Mouse) -> bool {
        if self.linear {
            return false;
        }
        let layers = &self.layers;
        let focus = layers.last().and_then(|layer| layer.focus);
        let Some(inspector) = self.inspector.as_mut().filter(|i| i.open && i.shown) else {
            return false;
        };
        let hit = |x: u16, y: u16| node_at(layers, x, y);
        let rect = |id: NodeId| node_rect(layers, id);
        let probe = Probe {
            focus,
            area: inspector.area,
            hit: &hit,
            rect: &rect,
        };
        let panel = inspector.panel;
        let handled = inspector.mouse(mouse, panel, &probe);
        self.after_inspector(handled);
        handled.used
    }

    /// What the inspector asked for after an event.
    fn after_inspector(&mut self, handled: Handled) {
        if handled.redraw {
            self.poke = true;
        }
        if handled.motion {
            // Picking follows the pointer.
            self.motion.0 = true;
        }
        if handled.retrace {
            self.sync_tracer();
        }
    }

    /// Note `mouse` in the inspector's trace (not the pointer moving), with
    /// the node under it; its number, to say later who took it.
    fn trace_mouse(&self, mouse: rich_interact::Mouse) -> Option<u64> {
        if !self.runtime.tracer.on() || mouse.kind == MouseKind::Moved {
            return None;
        }
        let target = node_at(&self.layers, mouse.column, mouse.row);
        self.runtime.tracer.push(Traced::Mouse {
            mouse,
            target,
            used: None,
        })
    }

    /// The columns the app lays out in. Linear mode draws nothing, so a
    /// terminal that reports no size (a new pseudo-terminal) still gets
    /// 80.
    fn columns(&self, columns: u16) -> u16 {
        if self.linear && columns == 0 {
            80
        } else {
            columns
        }
    }

    /// Linear mode: the rows to lay out in, as many as the top screen's
    /// content wants (at least the terminal's, at most [`LINEAR_ROWS`]),
    /// so every node has a place to be read from however few rows the
    /// terminal has.
    fn linear_rows(&self, console: &Console, columns: u16, rows: u16) -> u16 {
        let root = &self.top().root;
        let wanted = self
            .runtime
            .enter(|| root.measure(console, Axis::Vertical, columns, 0));
        wanted.clamp(rows.clamp(1, LINEAR_ROWS), LINEAR_ROWS)
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

    /// The accessibility tree of the top screen (see
    /// [`Driver::accessibility`]).
    fn accessibility(&self, screen: &Screen) -> Vec<crate::a11y::AccessNode> {
        use crate::a11y::{screen_text, AccessNode, Role};
        let focus = self.focus();
        let top = self.top();
        let text_mode = self.text_mode();
        let hidden = self.hidden_rects();
        // A layer's text leaves out the layers over it.
        let covered = crate::node::covered(&top.root);
        let mut out = Vec::new();
        let mut included: Vec<NodeId> = Vec::new();
        // Nodes whose insides are left out: hidden ones, and ones named by
        // what they show, which says what is inside already (a live row).
        let mut closed: Vec<NodeId> = Vec::new();
        crate::node::walk_screen(&top.root, &mut |node, path, shift, clip| {
            let id = node.id();
            if node.is_access_hidden() || path.iter().any(|id| closed.contains(id)) {
                closed.push(id);
                return;
            }
            let mut role = node.access_role();
            if id == top.root.id() && top.modal.is_some() {
                role = Role::Dialog;
            }
            let label = node.access_label();
            if role == Role::Group && label.is_none() {
                return;
            }
            let rect = crate::node::translate(node.rect(), shift).intersection(clip);
            if rect.is_empty() {
                return;
            }
            let depth = path[..path.len() - 1]
                .iter()
                .filter(|id| included.contains(id))
                .count();
            let shown = || match covered.get(&id) {
                Some(over) => screen_text(screen, rect, &[&hidden[..], &over[..]].concat()),
                None => screen_text(screen, rect, &hidden),
            };
            let named_by_text = label.is_none()
                && match role {
                    Role::Text
                    | Role::Button
                    | Role::TextBox
                    | Role::Status
                    | Role::Cell
                    | Role::ListItem
                    | Role::Tab
                    | Role::MenuItem
                    | Role::TreeItem
                    | Role::CheckBox
                    | Role::Switch => true,
                    // A dialog that is one line of text says it.
                    Role::Dialog => !node.has_children(),
                    _ => false,
                };
            let name = match label {
                Some(label) => label,
                None if named_by_text => {
                    closed.push(id);
                    shown()
                }
                None => String::new(),
            };
            // A multi-line text box's value is its caret's line, as an
            // item's is the selected one.
            let value = if role.has_items() || (role == Role::TextBox && node.cursor().is_some()) {
                node.cursor().map(|at| {
                    let inner = crate::node::translate(node.inner.get(), shift);
                    let item = Rect::new(
                        inner.x.saturating_add(at.x),
                        inner.y.saturating_add(at.y),
                        at.width,
                        at.height,
                    );
                    let text = screen_text(screen, item.intersection(rect), &hidden);
                    // In text mode, without the marker on the selection.
                    match text.strip_prefix('>').filter(|_| text_mode) {
                        Some(rest) => rest.trim_start().to_string(),
                        None => text,
                    }
                })
            } else if role == Role::TextBox {
                Some(shown())
            } else {
                None
            };
            included.push(id);
            out.push(AccessNode {
                depth,
                id,
                role,
                name,
                value,
                focused: focus == Some(id),
                disabled: node.disabled(),
                state: node.access_state(),
                rect,
            });
        });
        out
    }

    /// Where the top screen's [hidden](crate::Node::access_hidden) nodes
    /// are, on the screen.
    fn hidden_rects(&self) -> Vec<Rect> {
        let mut out = Vec::new();
        crate::node::walk_screen(&self.top().root, &mut |node, _, shift, clip| {
            if node.is_access_hidden() {
                out.push(crate::node::translate(node.rect(), shift).intersection(clip));
            }
        });
        out
    }

    /// Show the focused component's text caret, or keep the cursor hidden.
    /// Accessible, the cursor goes on whatever has the focus.
    fn caret(&mut self, painter: &mut Painter) -> String {
        let accessible = self.accessible;
        let caret = self.focus().and_then(|id| {
            let mut caret = None;
            // Inside a scroll, a node's coordinates are its own: move the
            // caret onto the screen, and hide it when scrolled out of view.
            crate::node::walk_screen(&self.top().root, &mut |node, _, shift, clip| {
                if node.id() == id {
                    let point = node.caret.get().or_else(|| {
                        accessible.then(|| {
                            let inner = node.inner.get();
                            let at = node.cursor().unwrap_or_default();
                            (inner.x.saturating_add(at.x), inner.y.saturating_add(at.y))
                        })
                    });
                    caret = point.and_then(|(x, y)| {
                        let at = crate::node::translate(Rect::new(x, y, 1, 1), shift);
                        clip.contains(at.x, at.y).then_some((at.x, at.y))
                    });
                }
            });
            caret
        });
        // Accessible, a cursor already shown where it goes stays put: a
        // screen reader following it reads nothing new.
        let parked = accessible && self.caret_shown && caret == self.caret_at;
        let out = match caret {
            Some(_) if parked => String::new(),
            Some((x, y)) => format!("{}\x1b[?25h", painter.move_to(x, y)),
            None if self.caret_shown => "\x1b[?25l".to_string(),
            None => String::new(),
        };
        self.caret_shown = caret.is_some();
        self.caret_at = caret;
        out
    }

    /// Draw what changed; returns the damage.
    fn frame(&mut self, console: &Console, screen: &mut Screen, full: bool) -> Vec<Rect> {
        crate::a11y::set_text_mode(self.text_mode());
        // What the inspector drew over the app comes off first, so the
        // app's cells are its own again before anything draws.
        let restored = match &mut self.inspector {
            Some(inspector) => inspector.restore(screen),
            None => Vec::new(),
        };
        let whole = screen.area();
        // The inspector, when shown, takes the right of the screen.
        let panel = self
            .panel_width(whole.width)
            .map(|w| Rect::new(whole.right() - w, whole.y, w, whole.height));
        // Only while it is shown is the frame timed.
        let started = panel.map(|_| std::time::Instant::now());
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
                    (Some(_), _) if layer.origin.is_some() => {
                        let (origin, offset) = layer.origin.expect("a placed pop-up");
                        crate::place::place(console, Some(&layer.root), area, origin, offset)
                    }
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
        let mut full = full || moved || std::mem::take(&mut self.restack);
        // The stylesheet as it applies at this size: crossing an `@media`
        // rule's bounds matches every node again and draws everything.
        let viewport = self.runtime.viewport().get_untracked();
        let generation = self
            .sheet
            .as_mut()
            .map(|source| source.generation_at(viewport));
        if generation != self.sheet_drawn {
            full = true;
            self.sheet_drawn = generation;
        }
        // `$name` in the stylesheet: the theme's style of that name.
        let theme = &self.theme;
        let vars = |name: &str| -> Option<Style> {
            match name {
                "border" => Some(theme.border.clone()),
                "border-focused" | "border.focused" => Some(theme.border_focused.clone()),
                "title" => Some(theme.title.clone()),
                _ => theme
                    .styles
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, style)| style.clone()),
            }
        };
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
            sheet: self.sheet.as_ref().map(|source| crate::node::SheetCx {
                sheet: &source.sheet,
                generation: generation.unwrap_or(source.generation),
                vars: &vars,
                viewport,
            }),
            chain: Vec::new(),
            announcements: Vec::new(),
            hidden: 0,
            hidden_rects: Vec::new(),
            timings: panel.map(|_| Vec::new()),
            time_stack: Vec::new(),
            scoped: Vec::new(),
        };
        if full {
            // A screen opened or closed: nothing on screen can be trusted.
            screen.clear(whole);
            frame.damage.push(whole);
        }
        runtime.enter(|| {
            for (layer, rect) in self.layers[base..].iter_mut().zip(&rects) {
                // A modal's backdrop and shadow restyle what is below it,
                // where it was drawn this frame (all of it in a full one).
                if layer.modal.is_some() && layer.look != ModalOptions::default() {
                    let damaged: Vec<Rect> = if full {
                        vec![area]
                    } else {
                        frame
                            .damage
                            .iter()
                            .map(|d| d.intersection(area))
                            .filter(|d| !d.is_empty())
                            .collect()
                    };
                    let style = |name: &String| crate::widget::theme_style(console, name, name);
                    if let Some(backdrop) = layer.look.backdrop.as_ref().map(style) {
                        for part in &damaged {
                            restyle_rect(screen, *part, &backdrop);
                        }
                    }
                    if let Some(shadow) = layer.look.shadow.as_ref().map(style) {
                        let strips = [
                            Rect::new(rect.right(), rect.y.saturating_add(1), 1, rect.height),
                            Rect::new(rect.x.saturating_add(1), rect.bottom(), rect.width, 1),
                        ];
                        for strip in strips {
                            for part in &damaged {
                                let part = strip.intersection(*part);
                                if !part.is_empty() {
                                    restyle_rect(screen, part, &shadow);
                                }
                            }
                        }
                    }
                }
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
                if let Some(sheet) = &frame.sheet {
                    layer.root.resolve(sheet, &mut frame.chain);
                }
                layer.root.draw(&mut frame, *rect, screen, force);
                layer.drawn = Some(*rect);
            }
            // Where nodes with a rect handle are on the screen now.
            if crate::place::any_handles() {
                // Nowhere on the screen: whatever a full screen covers,
                // and whatever is hidden.
                for layer in &self.layers[..base] {
                    clear_rect_handles(&layer.root);
                }
                for layer in &self.layers[base..] {
                    layer.root.walk_all(&mut |node, _, hidden| {
                        if hidden {
                            clear_rect_handles(node);
                        }
                    });
                }
                for layer in &self.layers[base..] {
                    crate::node::walk_screen(&layer.root, &mut |node, _, shift, clip| {
                        if let Some(handle) = *node.rect_slot.borrow() {
                            let rect =
                                crate::node::translate(node.rect(), shift).intersection(clip);
                            let rect = (!rect.is_empty()).then_some(rect);
                            if handle.get_untracked() != rect {
                                handle.set(rect);
                            }
                        }
                    });
                }
            }
        });
        let took = started.map(|start| start.elapsed()).unwrap_or_default();
        self.stats.drawn = frame.drawn;
        // A drag: its source dimmed, the drop target under the pointer lit.
        if let Some(drag) = self.drag.as_ref().filter(|drag| drag.active) {
            let root = &self.top().root;
            if let Some(rect) = anchor_rect(root, drag.source) {
                restyle_rect(screen, rect, &style("dim"));
                frame.damage.push(rect);
            }
            if let Some(rect) = drag.target.and_then(|id| anchor_rect(root, id)) {
                restyle_rect(screen, rect, &self.theme.named("drop.target", "reverse"));
                frame.damage.push(rect);
            }
        }
        // Toasts, newest nearest their corner, over everything. Linear,
        // they are written as lines instead, and nothing covers the nodes
        // whose names are read from the screen.
        if !self.linear {
            let notes: Vec<&Note> = self.toasts.iter().map(|(note, _)| note).collect();
            let corner = self.toast_corner;
            let widest = crate::place::resolve(self.notify_width, area.width, &|| area.width);
            let look = (&self.theme, corner, widest);
            let drawn = crate::notify::draw(console, screen, area, &notes, look);
            frame.damage.extend(drawn);
        }
        // The which-key pop-up, once its delay is over.
        self.which_key_at = None;
        let shown = self.layers.len();
        let due = match (&self.which_key, &self.pending) {
            (Some(delay), Some(pending)) => {
                pending.depth == shown && self.now >= pending.since + *delay
            }
            _ => false,
        };
        if due && !self.linear {
            let mut entries = Vec::new();
            if let (Some(pending), Some(top)) = (&self.pending, self.layers.last()) {
                with_node(&top.root, pending.node, &mut |node| {
                    entries = crate::sequence::continuations(node, &pending.keys);
                });
            }
            let drawn =
                crate::sequence::draw_which_key(console, screen, area, &entries, &self.theme);
            if let Some(rect) = drawn {
                frame.damage.push(rect);
            }
            self.which_key_at = drawn;
        }
        if let Some(pending) = self.pending.as_mut().filter(|_| due) {
            pending.shown = true;
        }
        // A tooltip, over the toasts too.
        if let Some(tip) = self.tip.as_ref().filter(|tip| tip.shown && !self.linear) {
            let look = self.theme.named("tooltip", "reverse");
            frame
                .damage
                .push(draw_tip(console, screen, area, tip, &look));
        }
        if frame.wants_hover.get() {
            self.motion.0 = true;
        }
        let live = std::mem::take(&mut frame.announcements);
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
        // While the inspector is open its trace takes in what happened,
        // whether its panel is shown or the terminal is too narrow for it.
        if let Some(inspector) = self.inspector.as_mut().filter(|i| i.open) {
            inspector.shown = panel.is_some();
            if panel.is_some() {
                inspector.frames += 1;
            } else {
                // No room for the panel: the keys are the app's.
                inspector.keys = false;
                inspector.pick = None;
            }
            let (events, gone) = self.runtime.tracer.take();
            if !events.is_empty() {
                // Named from every screen on the stack, and those that
                // closed since: an event may be about any of them.
                let names = Names::of(self.layers.iter().map(|layer| &layer.root)).with(&gone);
                inspector.take_trace(events, &names);
            }
        }
        if let (Some(panel), Some(drawn)) = (panel, frame.drawn_ids) {
            let theme = self.theme_status();
            let looks = crate::inspect::Looks {
                selected: self.theme.named("inspector.selected", "reverse magenta"),
                cursor: self.theme.named("inspector.cursor", "reverse yellow"),
                flash: self.theme.named("inspector.flash", "on dark_green"),
            };
            let focus_path = self.focus_path.get_untracked();
            let hover = self.hover_path.get_untracked();
            let viewport = self.runtime.viewport().get_untracked();
            let all = &self.layers;
            let shown: Vec<(&Node, bool)> = all[base..]
                .iter()
                .map(|layer| (&layer.root, layer.modal.is_some()))
                .collect();
            let rect = |id: NodeId| node_rect(all, id);
            // A frame drawn for the inspector alone (its selection moving)
            // is not the app's: its numbers stay those of the app's last.
            let fresh = full || !drawn.is_empty() || !frame.dirty.is_empty() || !damage.is_empty();
            let inspector = self.inspector.as_mut().expect("a panel means an inspector");
            inspector.area = area;
            inspector.panel = panel;
            if fresh {
                inspector.record(frame.timings.as_deref().unwrap_or_default(), took);
            }
            let report = crate::inspect::Report {
                layers: shown,
                fresh,
                drawn: &drawn,
                dirty: &frame.dirty,
                damage: &damage,
                focus: all.last().and_then(|layer| layer.focus),
                focus_path: &focus_path,
                hover: &hover,
                stats: self.stats,
                frame: inspector.frames,
                theme,
                viewport,
                rect: &rect,
            };
            // The node pane reads conditions (`class_when`, `disabled_when`),
            // which may read signals: in the app, subscribing nobody.
            let runtime = self.runtime.clone();
            let lines = runtime.enter(|| {
                runtime.untracked(|| inspector.render(console, &report, panel.width, panel.height))
            });
            screen.write_lines(panel, &lines);
            // Over the app: the redraw flash, the selected node's outline,
            // the picking cursor. They come off at the next frame.
            let over = inspector.overlay(screen, area, &damage, self.now, &looks, &rect);
            damage.push(panel);
            damage.extend(over);
        }
        damage.extend(restored);
        for announcement in live {
            self.announce(announcement, false);
        }
        // A screen or dialog that opened says what it shows, once drawn.
        if std::mem::take(&mut self.opened) && !self.top().is_popup() {
            let rect = self.top().drawn.unwrap_or(whole);
            let hidden = self.hidden_rects();
            let mut text = crate::a11y::screen_text(screen, rect, &hidden);
            if text.chars().count() > 200 {
                text = text.chars().take(200).collect::<String>() + "…";
            }
            let text = match self.top().modal {
                Some(_) => format!("Dialog: {text}"),
                None => text,
            };
            self.announce(crate::a11y::Announcement { text, urgent: true }, false);
        }
        damage
    }

    /// Put the one-shot timers [`after`] made since the last turn on the
    /// app's clock, from now.
    fn take_afters(&mut self) {
        let made = std::mem::take(&mut *self.runtime.afters.borrow_mut());
        for after in made {
            self.timeouts.push(Timeout {
                due: self.now + after.wait,
                run: after.run,
                at: after.at,
            });
        }
    }

    fn ctx(&self) -> Ctx {
        let mut cx = Ctx::new(self.runtime.proxy());
        cx.pointer = self.pointer;
        cx.layers = self.layers.len();
        cx
    }

    /// Apply what a handler asked for; whether to quit.
    fn apply(&mut self, cx: Ctx) -> bool {
        let mut quit = cx.quit;
        for after in cx.afters {
            self.timeouts.push(Timeout {
                due: self.now + after.wait,
                run: after.run,
                at: after.at,
            });
        }
        self.take_afters();
        if let Some(exit) = cx.exit {
            self.exit = Some(exit);
        }
        if let Some(error) = cx.error {
            self.error = Some(error);
        }
        if self.linear {
            // Linear, they are lines like any other.
            for markup in cx.printed {
                let plain = rich::Text::from_markup(&markup)
                    .map(|text| text.plain().to_string())
                    .unwrap_or(markup);
                self.unwritten.extend(plain.lines().map(str::to_string));
            }
        } else {
            self.printed.extend(cx.printed);
        }
        self.copies.extend(cx.copies);
        self.handoffs.extend(cx.handoffs);
        for (note, duration) in cx.toasts {
            self.push_toast(note, self.now + duration);
        }
        for announcement in cx.announcements {
            self.announce(announcement, true);
        }
        for (value, to, duration, easing) in cx.animations {
            // A new animation of a signal replaces the one running.
            self.animations.retain(|a| a.value != value);
            let from = self.runtime.enter(|| value.get_untracked());
            // Accessible: no motion, the value goes straight to its end.
            let duration = if self.text_mode() {
                Duration::ZERO
            } else {
                duration
            };
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
        let what = match &nav {
            Nav::Push(_) => Some("screen opened"),
            Nav::Modal(..) => Some("modal opened"),
            Nav::Popup(..) | Nav::PopupAt(..) => Some("pop-up opened"),
            Nav::Pop => (self.layers.len() > 1).then_some("screen closed"),
            Nav::Replace(_) => Some("screen replaced"),
            Nav::Palette => Some("command palette opened"),
            Nav::Help(_) => Some("help opened"),
            Nav::Run(..) => None,
        };
        if let Some(what) = what {
            self.runtime.tracer.push(Traced::Screen(what));
        }
        match nav {
            Nav::Push(build) => self.open(build, None),
            Nav::Modal(width, height, look, build) => {
                self.open(build, Some((width, height)));
                self.top_mut().look = look;
            }
            Nav::Popup(anchor, placement, width, height, look, build) => {
                self.open(build, Some((width, height)));
                let top = self.top_mut();
                top.anchor = Some((anchor, placement));
                top.look = look;
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
                let origin = self.top().origin;
                let look = self.top().look.clone();
                if self.layers.len() > 1 {
                    self.close();
                    self.open(build, modal);
                    self.top_mut().anchor = anchor;
                    self.top_mut().origin = origin;
                    self.top_mut().look = look;
                } else {
                    // The first screen: its timers go with it.
                    let old = self.layers.pop().expect("an app always has a screen");
                    self.runtime.tracer.farewell(&old.root);
                    old.root.forget(&self.runtime);
                    for watch in old.watches {
                        self.runtime.drop_watch(watch);
                    }
                    self.open(build, None);
                }
            }
            Nav::Palette => self.open_palette(),
            Nav::Help(options) => self.open_help(options),
            Nav::PopupAt(origin, offset, look, build) => {
                self.open(build, Some((offset.w, offset.h)));
                let top = self.top_mut();
                top.origin = Some((origin, offset));
                top.look = look;
            }
            Nav::Run(id, index, sequence) => return self.run_binding(id, index, sequence),
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
                let name = node.group_name();
                // A binding that does not apply now is not listed.
                for (index, bound) in node.keys.borrow().iter().enumerate() {
                    if bound.description.is_empty() || !bound.active() {
                        continue;
                    }
                    let binding = rich_interact::keymap::Binding::new(
                        bound.section.clone().unwrap_or_else(|| name.clone()),
                        format!("{id}:{index}"),
                        bound.shown.clone().unwrap_or_else(|| bound.keys.clone()),
                        bound.description.clone(),
                    );
                    out.push((id, index, binding));
                }
                for (index, bound) in node.sequences.borrow().iter().enumerate() {
                    if bound.description.is_empty() || !bound.active() {
                        continue;
                    }
                    let binding = rich_interact::keymap::Binding::sequence(
                        bound.section.clone().unwrap_or_else(|| name.clone()),
                        format!("{id}:s{index}"),
                        bound.keys.clone(),
                        bound.description.clone(),
                    );
                    out.push((id, index, binding));
                }
            });
        }
        out
    }

    /// What a [`key_hints`](crate::widgets::key_hints) footer shows: the
    /// described bindings (not sequences) of the focused node and its
    /// ancestors, innermost first, each keys once. Nothing without one.
    fn refresh_hints(&self) {
        let Some(hints) = self.runtime.hints.get() else {
            return;
        };
        let mut shown: Vec<(String, String)> = Vec::new();
        for (_, _, binding) in self.bindings() {
            let keys = binding.hint();
            if binding.sequence || shown.iter().any(|(k, _)| *k == keys) {
                continue;
            }
            shown.push((keys, binding.description));
        }
        self.runtime.enter(|| hints.set(shown));
    }

    /// The help over the top screen, as `options` say.
    fn open_help(&mut self, options: crate::help::HelpOptions) {
        let all: Vec<_> = self.bindings().into_iter().map(|(_, _, b)| b).collect();
        self.open(
            Box::new(move || {
                let format = options.key_format;
                let help = rich_interact::overlay::Help::from_bindings(all)
                    .wrap(true)
                    .flat(options.filter_first)
                    .filter_key(options.filter_key)
                    .key_format(move |binding| format.keys(&binding.keys, binding.sequence));
                crate::node::component(help, |_, cx| cx.pop())
                    .on_cancel(|cx| cx.pop())
                    .panel(&options.title)
            }),
            Some((Size::Percent(70), Size::Percent(70))),
        );
    }

    /// The command palette over the top screen.
    fn open_palette(&mut self) {
        let bindings = self.bindings();
        let commands: Vec<rich_interact::overlay::Command> = bindings
            .iter()
            .map(|(_, _, binding)| rich_interact::overlay::Command::from_binding(binding))
            .collect();
        let targets: Vec<(String, NodeId, usize, bool)> = bindings
            .iter()
            .map(|(id, index, binding)| (binding.id(), *id, *index, binding.sequence))
            .collect();
        self.open(
            Box::new(move || {
                let palette = rich_interact::overlay::Palette::new(commands);
                crate::node::component(palette, move |command, cx| {
                    cx.pop();
                    if let Some((_, id, index, sequence)) =
                        targets.iter().find(|t| t.0 == command.id)
                    {
                        cx.nav.push(Nav::Run(*id, *index, *sequence));
                    }
                })
                .on_cancel(|cx| cx.pop())
                .panel("Commands")
            }),
            Some((Size::Percent(60), Size::Percent(60))),
        );
    }

    /// Run binding `index` of node `id` (its key sequence `index`, if
    /// `sequence`) on the top screen; whether to quit.
    fn run_binding(&mut self, id: NodeId, index: usize, sequence: bool) -> bool {
        let mut cx = self.ctx();
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                let mut list = match sequence {
                    true => node.sequences.borrow_mut(),
                    false => node.keys.borrow_mut(),
                };
                if let Some(bound) = list.get_mut(index) {
                    // As its first key would.
                    let key = bound.keys[0];
                    (bound.handler)(&mut cx, key);
                }
            });
        });
        self.note_used(id, "on_key");
        self.apply(cx)
    }

    /// Build a screen and put it on top, focusing its first focusable node.
    fn open(&mut self, build: Build, modal: Option<(Size, Size)>) {
        BUILDING.with(|building| *building.borrow_mut() = Some(Vec::new()));
        WATCHES.with(|watches| *watches.borrow_mut() = Some(Vec::new()));
        let runtime = self.runtime.clone();
        let root = runtime.enter(build);
        // A modal's or pop-up's panel titles itself in the theme's
        // `popup.title`, unless code said otherwise.
        if modal.is_some() {
            root.widget_mut(|panel: &mut crate::builtin::Panel| {
                panel
                    .code
                    .title_style
                    .get_or_insert_with(|| POPUP_TITLE.to_string());
            });
        }
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
        self.pending = None;
        self.layers.push(Layer {
            root,
            focus: None,
            modal,
            look: ModalOptions::default(),
            drawn: None,
            anchor: None,
            origin: None,
            timers,
            watches,
            order: Vec::new(),
            fallback: false,
        });
        self.restack = true;
        self.runtime.layers.set(self.layers.len());
        // Once drawn, what opened is announced.
        self.opened = true;
        self.set_focus(None);
        self.focus_first();
    }

    /// Take the top screen off, forgetting its subscriptions.
    fn close(&mut self) {
        self.pending = None;
        let layer = self.layers.pop().expect("an app always has a screen");
        self.runtime.tracer.farewell(&layer.root);
        layer.root.forget(&self.runtime);
        for watch in layer.watches {
            self.runtime.drop_watch(watch);
        }
        self.restack = true;
        self.runtime.layers.set(self.layers.len());
    }

    /// The theme in code with the theme file's last good styles over it.
    fn retheme(&mut self) {
        // `$name` in the stylesheet means the new theme's styles now.
        if let Some(source) = &mut self.sheet {
            source.generation = sheet_generation();
        }
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

    /// The Tab order: nodes that can take the focus, less those
    /// [hidden](crate::Node::access_hidden) from assistive technology.
    fn focusable(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        self.top().root.walk_accessible(&mut |node, _| {
            // A wrapper whose focus passes inside is not a stop of its own.
            if node.takes_focus() && node.focus_target().is_none() {
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
            // A node that was disabled lets the focus go as one that left.
            let mut disabled = false;
            with_node(&self.top().root, id, &mut |node| disabled = node.disabled());
            if disabled || self.path_to(id).is_empty() {
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
        // A wrapper given the focus hands it to what it holds, once that
        // is built (a `switch`'s list).
        if let Some(id) = self.focus() {
            let mut target = None;
            with_node(&self.top().root, id, &mut |node| {
                target = node.focus_target()
            });
            if target.is_some() {
                let fallback = self.top().fallback;
                self.set_focus(target);
                self.top_mut().fallback = fallback;
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
                Note::plain(format!(
                    "[bold red]{what} draws itself again and again[/]: it writes a \
                     signal it reads while it draws. Write it in a handler or a watch."
                )),
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

    /// The screen's first node that asked for the focus and is neither
    /// [disabled](crate::Node::disabled_when) nor
    /// [hidden](crate::Node::access_hidden) from assistive technology.
    fn autofocused(&self) -> Option<NodeId> {
        let mut chosen = None;
        self.top().root.walk_accessible(&mut |node, _| {
            if node.autofocus && chosen.is_none() && !node.disabled() {
                chosen = Some(node.focus_target().unwrap_or(node.id));
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
            self.runtime.tracer.push(Traced::Focus(id));
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
    /// Whether a node on the path keys take binds `key`, or its widget
    /// [claims](crate::Widget::claims_key) it.
    fn binds(&self, key: Key) -> bool {
        let mut bound = false;
        for id in self.key_path() {
            with_node(&self.top().root, id, &mut |node| {
                bound |= node
                    .keys
                    .borrow()
                    .iter()
                    .any(|bound| key.matches_any(bound.live_keys()))
                    || node
                        .sequences
                        .borrow()
                        .iter()
                        .any(|bound| crate::sequence::starts(bound.live_keys(), &[key]))
                    || node.body.borrow().widget.claims_key(key);
            });
        }
        bound
    }

    /// The nodes a key goes through, root first: to the focused node, or
    /// with nothing focused, to the node under the pointer, else down from
    /// the root through each node that holds one child (a panel round a
    /// pop-up's content, its padding), so the bindings of what a wrapper
    /// holds work too.
    fn key_path(&self) -> Vec<NodeId> {
        match self.focus().map(|id| self.path_to(id)) {
            Some(path) if !path.is_empty() => path,
            _ => {
                let hovered = self.hover_path.get_untracked();
                if hovered.first() == Some(&self.top().root.id) {
                    return hovered;
                }
                let mut path = Vec::new();
                fn down(node: &Node, path: &mut Vec<NodeId>) {
                    path.push(node.id);
                    let body = node.body.borrow();
                    let shown: Vec<&Node> = body
                        .widget
                        .children()
                        .iter()
                        .filter(|child| !child.hidden())
                        .collect();
                    if let [only] = shown[..] {
                        down(only, path);
                    }
                }
                down(&self.top().root, &mut path);
                path
            }
        }
    }

    fn key(&mut self, key: Key) -> bool {
        self.hide_tip();
        if key.code == KeyCode::Escape && self.drag.as_ref().is_some_and(|drag| drag.active) {
            // Esc cancels a drag.
            self.drag = None;
            self.restack = true;
            return false;
        }
        if let (Some(inspector), false) = (&mut self.inspector, self.linear) {
            if key.code == KeyCode::F(12) {
                if !key.modifiers.shift {
                    // F12 shows and hides it; hidden, it gives the keys back.
                    let open = !inspector.open;
                    inspector.set_open(open);
                    self.restack = true;
                } else if inspector.open {
                    // Shift+F12 gives it the keys, or gives them back.
                    inspector.toggle_keys();
                    self.poke = true;
                } else {
                    inspector.set_open(true);
                    inspector.keys = true;
                    self.restack = true;
                }
                self.sync_tracer();
                return false;
            }
            if inspector.open && (inspector.keys || inspector.pick.is_some()) {
                // Every key is the inspector's while it has them.
                self.inspector_key(key);
                return false;
            }
        }
        // A focused node disabled since the last frame (nothing need have
        // drawn since) gives the focus up before the key is routed.
        if let Some(id) = self.focus() {
            let mut disabled = false;
            with_node(&self.top().root, id, &mut |node| disabled = node.disabled());
            if disabled {
                self.keep_focus();
            }
        }
        // From the focused node up; with nothing focused, from the node
        // under the pointer, so a container that cannot take the focus
        // still gets keys while the mouse is over it.
        let path = self.key_path();
        // A key sequence under way takes a key that goes on with it; Esc
        // cancels it; any other key cancels it and is routed as usual.
        if let Some(pending) = self.pending.take() {
            self.which_key_changed();
            if let Some(quit) = self.continue_sequence(pending, key, &path) {
                return quit;
            }
            if key.code == KeyCode::Escape {
                return false;
            }
        }
        // Containers that asked see the key first, outermost first: their
        // widgets, and nodes' own bindings ([`Node::previews_keys`]). A
        // disabled node sees none: neither its widget nor its handlers.
        for &id in &path[..path.len().saturating_sub(1)] {
            let mut previews = (false, false);
            with_node(&self.top().root, id, &mut |node| {
                if !node.disabled() {
                    previews = (node.body.borrow().widget.previews_keys(), node.previews);
                }
            });
            if previews.0 {
                if let Some(quit) = self.give_to_widget(id, &WidgetEvent::Preview(key)) {
                    return quit;
                }
            }
            if previews.1 {
                if let Some(quit) = self.node_keys(id, key) {
                    return quit;
                }
            }
        }
        let focused = path.last().copied();
        for &id in path.iter().rev() {
            // Disabled; or a container that saw the key first already.
            let mut skip = (false, false);
            with_node(&self.top().root, id, &mut |node| {
                skip = (node.disabled(), node.previews && Some(id) != focused);
            });
            if skip.0 {
                continue;
            }
            if let Some(quit) = self.give_to_widget(id, &WidgetEvent::Key(key)) {
                return quit;
            }
            if !skip.1 {
                if let Some(quit) = self.node_keys(id, key) {
                    return quit;
                }
            }
        }
        if key == Key::new(KeyCode::F(1)) && self.show_focus_tip() {
            return false;
        }
        if key.matches_any(&self.palette_key) {
            return self.navigate(Nav::Palette);
        }
        if key.matches_any(&self.help_key) {
            return self.navigate(Nav::Help(crate::help::HelpOptions::new()));
        }
        match key.code {
            KeyCode::Tab => self.move_focus(true),
            KeyCode::BackTab => self.move_focus(false),
            KeyCode::Escape if self.top().is_popup() => return self.navigate(Nav::Pop),
            _ => {}
        }
        false
    }

    /// Offer `key` to node `id`'s own handlers: its bindings, then the
    /// first key of its sequences (which starts one), then its
    /// [`on_any_key`](crate::Node::on_any_key). `Some` (whether to quit)
    /// when one took it.
    fn node_keys(&mut self, id: NodeId, key: Key) -> Option<bool> {
        let mut cx = self.ctx();
        let mut used = None;
        let mut starts = false;
        let runtime = self.runtime.clone();
        runtime.enter(|| {
            with_node(&self.top().root, id, &mut |node| {
                if node.disabled() {
                    return;
                }
                {
                    let mut keys = node.keys.borrow_mut();
                    // A binding that names the key before one it could be
                    // (a Tab from a legacy terminal: `tab`, then `ctrl+i`).
                    // One that does not apply now has no keys (another of
                    // the key may: two under different conditions).
                    let picked = key.pick(keys.iter().map(|bound| bound.live_keys()));
                    if let Some(bound) = picked.and_then(|index| keys.get_mut(index)) {
                        (bound.handler)(&mut cx, key);
                        used = Some("on_key");
                        return;
                    }
                }
                let sequences = node.sequences.borrow();
                if sequences
                    .iter()
                    .any(|bound| crate::sequence::starts(bound.live_keys(), &[key]))
                {
                    starts = true;
                    return;
                }
                drop(sequences);
                if let Some(handler) = node.any_key.borrow_mut().as_mut() {
                    if handler(&mut cx, key) {
                        used = Some("on_any_key");
                    }
                }
            });
        });
        if starts {
            self.pending = Some(crate::sequence::Pending {
                node: id,
                keys: vec![key],
                since: self.now,
                depth: self.layers.len(),
                shown: false,
            });
            self.which_key_changed();
            self.note_used(id, "sequence");
            return Some(false);
        }
        let how = used?;
        self.note_used(id, how);
        Some(self.apply(cx))
    }

    /// Go on with the key sequence `pending` with `key`: run the sequence
    /// it ends, or wait for the next key. `None` when no sequence of the
    /// node goes on with it (or the node is no longer on the key's path).
    fn continue_sequence(
        &mut self,
        pending: crate::sequence::Pending,
        key: Key,
        path: &[NodeId],
    ) -> Option<bool> {
        if pending.depth != self.layers.len() || !path.contains(&pending.node) {
            return None;
        }
        let mut typed = pending.keys.clone();
        typed.push(key);
        let (mut exact, mut longer) = (None, false);
        with_node(&self.top().root, pending.node, &mut |node| {
            for (index, bound) in node.sequences.borrow().iter().enumerate() {
                if crate::sequence::starts(bound.live_keys(), &typed) {
                    if bound.keys.len() == typed.len() {
                        exact.get_or_insert(index);
                    } else {
                        longer = true;
                    }
                }
            }
        });
        if let Some(index) = exact {
            return Some(self.run_binding(pending.node, index, true));
        }
        if !longer {
            return None;
        }
        self.pending = Some(crate::sequence::Pending {
            keys: typed,
            since: self.now,
            shown: false,
            ..pending
        });
        self.which_key_changed();
        Some(false)
    }

    /// A key sequence started, went on or ended: the which-key pop-up
    /// changes.
    fn which_key_changed(&mut self) {
        if self.which_key.is_some() {
            self.restack = true;
        }
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

    /// The tooltip of the deepest node in `path` that has one.
    fn tip_of(&self, path: &[NodeId]) -> Option<(NodeId, String)> {
        path.iter().rev().find_map(|&id| {
            let mut tip = None;
            with_node(&self.top().root, id, &mut |node| tip = node.tooltip.clone());
            tip.map(|markup| (id, markup))
        })
    }

    /// Hide the tooltip; what was under it draws again.
    fn hide_tip(&mut self) {
        if self.tip.take().is_some_and(|tip| tip.shown) {
            self.restack = true;
        }
    }

    /// Follow the pointer for tooltips: resting on a node with one starts
    /// its wait (moving on it starts the wait again), going to another node
    /// or pressing a button hides it.
    fn track_tip(&mut self, path: &[NodeId], mouse: rich_interact::Mouse) {
        let owner = match mouse.kind {
            MouseKind::Moved => self.tip_of(path),
            _ => None,
        };
        match (owner, &mut self.tip) {
            (Some((id, _)), Some(tip)) if tip.node == id && tip.shown => {}
            (Some((id, _)), Some(tip)) if tip.node == id => {
                tip.at = (mouse.column, mouse.row.saturating_add(1));
                tip.above = mouse.row;
                tip.since = self.now;
            }
            (owner, _) => {
                self.hide_tip();
                self.tip = owner.map(|(node, markup)| Tip {
                    node,
                    markup,
                    at: (mouse.column, mouse.row.saturating_add(1)),
                    above: mouse.row,
                    since: self.now,
                    shown: false,
                });
            }
        }
    }

    /// Show the focused node's tooltip (or its nearest ancestor's) below
    /// it, at once. Whether there was one.
    fn show_focus_tip(&mut self) -> bool {
        let path = self.focus().map(|id| self.path_to(id)).unwrap_or_default();
        let Some((node, markup)) = self.tip_of(&path) else {
            return false;
        };
        let Some(rect) = anchor_rect(&self.top().root, node) else {
            return false;
        };
        self.tip = Some(Tip {
            node,
            markup,
            at: (rect.x, rect.bottom()),
            above: rect.y,
            since: self.now,
            shown: true,
        });
        self.restack = true;
        true
    }

    /// Route a mouse event. Movement updates what is hovered; a captured
    /// mouse goes to the node that captured it; anything else goes to the
    /// deepest node under the pointer and bubbles up through its ancestors
    /// until one uses it. A press first focuses the deepest focusable node
    /// under the pointer. Whether to quit.
    fn mouse_event(&mut self, mouse: rich_interact::Mouse) -> Option<bool> {
        // A press ends a key sequence under way.
        if matches!(mouse.kind, MouseKind::Down(_)) && self.pending.take().is_some() {
            self.which_key_changed();
        }
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
        self.track_tip(&path, mouse);
        if let Some(quit) = self.drag_mouse(&path, mouse) {
            return Some(quit);
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
            && self.top().is_popup()
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
                with_node(&self.top().root, *id, &mut |node| yes = node.takes_focus());
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
        // A press that may start a drag is used: it starts no selection.
        (self.drag.is_some() && matches!(mouse.kind, MouseKind::Down(_))).then_some(false)
    }

    /// The deepest node in `path` with a value to drag.
    fn drag_source(&self, path: &[NodeId]) -> Option<(NodeId, std::rc::Rc<dyn std::any::Any>)> {
        path.iter().rev().find_map(|&id| {
            let mut value = None;
            with_node(&self.top().root, id, &mut |node| value = node.drag.clone());
            value.map(|value| (id, value))
        })
    }

    /// The deepest node in `path`, other than the drag's source, that takes
    /// what it carries.
    fn drop_target(&self, path: &[NodeId], drag: &Drag) -> Option<NodeId> {
        path.iter().rev().copied().find(|&id| {
            let mut takes = false;
            if id != drag.source {
                with_node(&self.top().root, id, &mut |node| {
                    takes = node
                        .drop
                        .as_ref()
                        .is_some_and(|drop| (drop.accepts)(&*drag.value));
                });
            }
            takes
        })
    }

    /// Drag-and-drop: a left press on a draggable node arms a drag, moving
    /// a cell starts it, letting go drops it on the target under the
    /// pointer. `Some` (whether to quit) when the event was the drag's.
    fn drag_mouse(&mut self, path: &[NodeId], mouse: rich_interact::Mouse) -> Option<bool> {
        if self.capture.is_some() {
            // A widget took the mouse: no drag.
            self.drag = None;
            return None;
        }
        let at = (mouse.column, mouse.row);
        match mouse.kind {
            MouseKind::Down(Button::Left) => {
                if self.drag.take().is_some_and(|drag| drag.active) {
                    self.restack = true;
                }
                self.drag = self.drag_source(path).map(|(source, value)| Drag {
                    source,
                    value,
                    from: at,
                    active: false,
                    target: None,
                });
                None
            }
            MouseKind::Drag(Button::Left) => {
                let drag = self.drag.as_ref()?;
                if !drag.active && at == drag.from {
                    return Some(false);
                }
                let target = self.drop_target(path, drag);
                let drag = self.drag.as_mut()?;
                if !drag.active || drag.target != target {
                    self.restack = true;
                }
                drag.active = true;
                drag.target = target;
                Some(false)
            }
            MouseKind::Up(Button::Left) => {
                if !self.drag.as_ref()?.active {
                    self.drag = None;
                    return None;
                }
                let drag = self.drag.take()?;
                self.restack = true;
                let Some(target) = self.drop_target(path, &drag) else {
                    return Some(false);
                };
                let mut cx = self.ctx();
                let runtime = self.runtime.clone();
                runtime.enter(|| {
                    with_node(&self.top().root, target, &mut |node| {
                        if let Some(drop) = &node.drop {
                            (drop.handler.borrow_mut())(&*drag.value, &mut cx);
                        }
                    });
                });
                self.note_used(target, "on_drop");
                Some(self.apply(cx))
            }
            _ => None,
        }
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
        // Relative to where its widget draws, inside a stylesheet's border
        // and padding.
        let mut rect = Rect::default();
        let mut disabled = false;
        with_node(&self.top().root, id, &mut |node| {
            rect = node.inner.get();
            disabled = node.disabled();
        });
        if disabled {
            return None;
        }
        // Into the node's coordinates (they differ inside a scroll), then
        // relative to its rectangle.
        let at = |value: u16, by: i32, from: u16| {
            (value as i32 + by - from as i32).clamp(0, u16::MAX as i32) as u16
        };
        // Keeping its modifiers: Shift+wheel scrolls across.
        let local = rich_interact::Mouse {
            column: at(mouse.column, shift.0, rect.x),
            row: at(mouse.row, shift.1, rect.y),
            ..mouse
        };
        if let Some(quit) = self.give_to_widget_at(id, shift, &WidgetEvent::Mouse(local)) {
            return Some(quit);
        }
        let mut cx = self.ctx();
        let mut used = false;
        let mut how = "on_mouse";
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
                        how = "on_click";
                    }
                }
            });
        });
        if used {
            self.note_used(id, how);
        }
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
        if used {
            self.note_used(id, "widget");
        }
        used.then(|| self.apply(cx))
    }
}

/// After a frame's cells: the graphics `source` places on `screen` that
/// moved, went away or had cells written under them, as rs-rich-ext's
/// `Surface` draws them. `placements` is what the last frame drew.
fn draw_graphics(
    source: &dyn rich_ext::frame::PlacementSource,
    placements: &mut Vec<rich_ext::frame::Placement>,
    screen: &Screen,
    painter: &mut Painter,
) -> String {
    use rich::Segment;
    use rich_ext::frame::{Change, Frame};
    let mut segments = Vec::new();
    for line in screen.lines() {
        segments.extend(line);
        segments.push(Segment::line());
    }
    let frame = Frame::from_segments(&segments);
    let height = frame.height();
    let next: Vec<_> = source
        .placements(&frame)
        .into_iter()
        .filter(|p| p.row + p.rows <= height)
        .collect();
    let changes: Vec<Change> = painter
        .written()
        .iter()
        .flat_map(|rect| {
            (rect.y..rect.bottom()).map(move |row| Change {
                row: row as usize,
                columns: rect.x as usize..rect.right() as usize,
            })
        })
        .collect();
    let plan = rich_ext::frame::plan_graphics(placements, &next, &changes);
    let mut out = String::new();
    let repaint: Vec<Rect> = plan
        .repaint
        .iter()
        .map(|(row, columns)| {
            Rect::new(
                columns.start as u16,
                *row as u16,
                (columns.end - columns.start) as u16,
                1,
            )
        })
        .collect();
    if !repaint.is_empty() {
        out.push_str(&painter.repaint(screen, &repaint));
    }
    for placement in &plan.draw {
        out.push_str(&painter.move_to(placement.column as u16, placement.row as u16));
        out.push_str(&placement.graphic.draw(placement.frame));
    }
    *placements = next;
    out
}

/// What [`App::run_on`]'s loop ended with: the app's value or an error,
/// and the rows printed above a full-screen app, left to write.
struct Ended {
    result: io::Result<Option<Box<dyn std::any::Any>>>,
    printed: Vec<String>,
}

impl Proxy {
    /// [`Ctx::print_above`] from any thread: print console `markup` above
    /// the app (inline, into the scrollback above it). Returns `false` if
    /// the app has finished.
    #[track_caller]
    pub fn print_above(&self, markup: impl Into<String>) -> bool {
        let markup = markup.into();
        self.run_with(move |cx| cx.print_above(markup))
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
    /// Inline: the terminal row the region starts on, and the rows lines
    /// printed above it moved it down since.
    origin: u16,
    shifted: u16,
    done: bool,
    /// Linear mode: each node's line as last written, a map for each
    /// screen on the stack, and what had the focus.
    written: Vec<HashMap<NodeId, String>>,
    written_focus: Option<NodeId>,
}

/// How many frames in a row a node may leave itself dirty before its own
/// writes stop drawing it again.
const RESTLESS_FRAMES: u32 = 4;

/// The most rows linear mode lays out in.
const LINEAR_ROWS: u16 = 4096;

/// How many times a frame runs the watches its layout set off, and draws
/// what they changed, before it is sent.
const LAYOUT_ROUNDS: usize = 4;

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
        app.reload_sheet();
        app.now = now;
        app.runtime.tracer.set_now(now);
        app.runtime.now.set(now);
        app.take_afters();
        let mut cx = app.ctx();
        let runtime = app.runtime.clone();
        // One-shot timers that are due, in the order they fall due.
        let mut due = Vec::new();
        let mut i = 0;
        while i < app.timeouts.len() {
            if app.timeouts[i].due <= now {
                due.push(app.timeouts.remove(i));
            } else {
                i += 1;
            }
        }
        due.sort_by_key(|timeout| timeout.due);
        runtime.enter(|| {
            for timeout in due {
                if runtime.tracer.on() {
                    runtime.tracer.push(Traced::Timer {
                        every: Duration::ZERO,
                        at: timeout.at,
                    });
                }
                (timeout.run)(&mut cx);
            }
            for layer in &mut app.layers {
                for timer in &mut layer.timers {
                    if now >= timer.next {
                        if runtime.tracer.on() {
                            runtime.tracer.push(Traced::Timer {
                                every: timer.every,
                                at: timer.at,
                            });
                        }
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
            // Waiting for tasks, `latest` threads keep to the app's clock:
            // those due run until they sleep again (or end).
            if app.wait_for_tasks {
                runtime.clock.advance(now);
                runtime.clock.settle();
            }
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
        if let Some(tip) = app.tip.as_mut().filter(|tip| !tip.shown) {
            if now >= tip.since + TOOLTIP_DELAY {
                tip.shown = true;
                app.restack = true;
            }
        }
        // A key sequence that waited long enough shows its which-key.
        if let (Some(delay), Some(pending)) = (app.which_key, &app.pending) {
            if !pending.shown && now >= pending.since + delay {
                app.restack = true;
            }
        }
        // The inspector's redraw flash, over: it comes off the app.
        if app
            .inspector
            .as_ref()
            .and_then(Inspector::flash_ends)
            .is_some_and(|ends| now >= ends)
        {
            app.poke = true;
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
        self.first
            || (self.app.inline.is_some() && !self.app.printed.is_empty())
            || self.app.restack
            || self.app.poke
            || self.app.runtime.has_dirty()
            || !self.app.unwritten.is_empty()
            || self.app.trace_news()
    }

    /// Draw what changed and return the bytes that show it (they may be
    /// empty); `None` when nothing changed. Then tell widgets of focus,
    /// hover and size changes, now that they are laid out. Watches the
    /// layout set off (one reading a [`RectHandle`](crate::RectHandle):
    /// what loads once its pane has a width) run before it returns, and,
    /// in an app that [waits for its tasks](App::wait_for_tasks), the
    /// tasks they start finish, and what they change is drawn too: the
    /// frame returned is settled, and the next event finds it so.
    pub fn render(&mut self) -> Option<String> {
        let mut out = self.render_once();
        // A few rounds: a watch's result may lay out anew, and set off
        // another.
        for _ in 0..LAYOUT_ROUNDS {
            if self.done || !self.app.runtime.has_pending_watches() {
                break;
            }
            self.update(self.app.now);
            if let Some(more) = self.render_once() {
                out.get_or_insert_with(String::new).push_str(&more);
            }
        }
        out
    }

    /// [`render`](Self::render), once: what changed drawn, and the
    /// lifecycle events told.
    fn render_once(&mut self) -> Option<String> {
        if self.done {
            return None;
        }
        self.app.refresh_hints();
        let out = self.needs_render().then(|| {
            let mut out = String::new();
            if !self.started {
                self.started = true;
                // Linear, only text is written.
                if !self.app.linear {
                    out.push_str("\x1b[?25l");
                }
            }
            self.app.stats.frames += 1;
            // Inline, lines printed above go into the scrollback before the
            // region draws again below them.
            if self.app.inline.is_some() && !self.app.printed.is_empty() {
                out.push_str(&self.print_inline());
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
    /// while something animates, until the next toast goes or a tooltip
    /// shows, until the next [`latest`](crate::latest) thread asleep on
    /// the app's clock wakes (in an app that
    /// [waits for its tasks](App::wait_for_tasks)), and at most 50 ms
    /// (theme files are looked at by `update`). A value another thread
    /// sends ends the wait early when the loop has a
    /// [waker](Self::set_waker), as [`App::run`]'s has; without one, it
    /// waits for the timeout.
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
            .min(
                app.timeouts
                    .iter()
                    .map(|t| t.due.saturating_sub(now))
                    .min()
                    .unwrap_or(Duration::MAX),
            )
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
            .min(
                app.tip
                    .as_ref()
                    .filter(|tip| !tip.shown)
                    .map(|tip| (tip.since + TOOLTIP_DELAY).saturating_sub(now))
                    .unwrap_or(Duration::MAX),
            )
            .min(match (app.which_key, &app.pending) {
                (Some(delay), Some(pending)) if !pending.shown => {
                    (pending.since + delay).saturating_sub(now)
                }
                _ => Duration::MAX,
            })
            .min(
                app.inspector
                    .as_ref()
                    .and_then(Inspector::flash_ends)
                    .map(|ends| ends.saturating_sub(now))
                    .unwrap_or(Duration::MAX),
            )
            .min(
                app.runtime
                    .clock
                    .next_due()
                    .map(|due| due.saturating_sub(now))
                    .unwrap_or(Duration::MAX),
            )
    }

    /// Have every [`Proxy`] send (a [`latest`](crate::latest) value, a
    /// task's result, [`Proxy::run`]) call `wake`, on the thread that
    /// sends: a loop of your own that waits on something (a socket, a
    /// channel, the terminal) ends that wait, so the value shows at once
    /// rather than at the next [`timeout`](Self::timeout).
    /// [`App::run_on`] sets the backend's
    /// [`Waker`](rich_interact::Waker) here, when it has one.
    ///
    /// ```
    /// use intuituive::interact::Waker;
    /// use intuituive::prelude::*;
    /// use std::time::Duration;
    ///
    /// let app = App::new(|| {
    ///     let status = signal("waiting");
    ///     text!("{status}")
    /// });
    /// let proxy = app.proxy();
    /// let mut driver = app.driver(10, 1);
    /// let waker = Waker::new();
    /// let wakes = waker.clone();
    /// driver.set_waker(move || wakes.wake());
    /// std::thread::spawn(move || proxy.run(|| {}));
    /// // The send ended the wait at once, not at its timeout.
    /// assert!(waker.wait(Duration::from_secs(10)));
    /// ```
    pub fn set_waker(&mut self, wake: impl Fn() + Send + Sync + 'static) {
        self.app.runtime.wake.set(Some(std::sync::Arc::new(wake)));
    }

    /// Handle an event: a key pressed or let go, the mouse, pasted text, or
    /// the terminal's new size. Ctrl+C quits, unless a node on the focused
    /// path binds it ([`on_key`](crate::Node::on_key)); then the binding
    /// runs.
    pub fn event(&mut self, event: Event) {
        if self.done {
            return;
        }
        // While the inspector is open, the trace says what came, and who
        // took it (the mouse is noted in `mouse`); not what was its own.
        let owned = self.app.inspector_owns(&event);
        let tracer = &self.app.runtime.tracer;
        let seq = match &event {
            _ if owned || !tracer.on() => None,
            Event::Key(key) => tracer.push(Traced::Key {
                key: *key,
                used: None,
            }),
            Event::Paste(text) => tracer.push(Traced::Paste {
                chars: text.chars().count(),
                used: None,
            }),
            Event::Resize { columns, rows } => tracer.push(Traced::Resize(*columns, *rows)),
            _ => None,
        };
        self.app.used = None;
        let quit = match event {
            Event::Key(key) if key == Key::ctrl('c') && !self.app.binds(key) => true,
            Event::Resize { columns, rows } => {
                self.resize(columns, rows);
                false
            }
            Event::Key(key) => self.app.key(key),
            // While the inspector has the keys, none reaches the app.
            Event::KeyUp(_) | Event::Paste(_) if owned => false,
            Event::KeyUp(key) => self.app.key_up(key),
            Event::Paste(text) => self.app.bubble(&WidgetEvent::Paste(text)),
            Event::Mouse(mouse) => self.mouse(mouse),
            _ => false,
        };
        if let Some(seq) = seq {
            let used = self.app.used.take();
            self.app
                .runtime
                .tracer
                .amend(seq, |event| event.set_used(used));
        }
        if quit {
            self.done = true;
        }
    }

    /// The terminal is now `columns` x `rows`: everything draws again.
    pub fn resize(&mut self, columns: u16, rows: u16) {
        self.rows = rows;
        self.console = self.app.console_for(self.app.columns(columns));
        self.app.last_console = Some(self.console.clone());
        self.screen = Screen::new(self.app.columns(columns), self.app.region(rows));
        self.app.set_viewport(self.screen.area(), rows);
        if let Some(inspector) = &mut self.app.inspector {
            inspector.forget_overlay();
        }
        self.painter.invalidate();
        self.first = true;
    }

    fn mouse(&mut self, mouse: rich_interact::Mouse) -> bool {
        // Inline, the region starts where the session began, or higher if
        // it had to scroll to fit.
        let top = if self.app.inline.is_some() {
            self.origin
                .saturating_add(self.shifted)
                .min(self.rows.saturating_sub(self.screen.area().height))
        } else {
            0
        };
        let Some(row) = mouse.row.checked_sub(top) else {
            return false;
        };
        let mouse = rich_interact::Mouse { row, ..mouse };
        // Picking, or on the inspector's panel: the inspector's.
        if self.app.inspector_mouse(mouse) {
            return false;
        }
        let seq = self.app.trace_mouse(mouse);
        self.app.used = None;
        let quit = self.route_mouse(mouse);
        if let Some(seq) = seq {
            let used = self.app.used.take();
            self.app
                .runtime
                .tracer
                .amend(seq, |event| event.set_used(used));
        }
        quit
    }

    /// Route a mouse event in the app's rows: to a selection being made,
    /// or to the nodes under it; a press nothing used may start a
    /// selection. Whether to quit.
    fn route_mouse(&mut self, mouse: rich_interact::Mouse) -> bool {
        let app = &mut self.app;
        let row = mouse.row;
        if app.selecting && app.select(mouse, &self.screen) {
            return false;
        }
        app.pointer = Some((mouse.column, row));
        let routed = app.mouse_event(mouse);
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

    /// Bring the app up to date at the time of the last
    /// [`update`](Self::update) and draw until nothing is left to draw: a
    /// test's step between events, where the bytes do not matter. It does
    /// not advance the clock.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| text!("ready"));
    /// let mut driver = app.driver(10, 1);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "ready");
    /// ```
    pub fn settle(&mut self) {
        // A frame can leave work for the next (a watch set off by a
        // lifecycle event); a few rounds settle any sane app.
        for _ in 0..8 {
            if self.done {
                return;
            }
            self.update(self.app.now);
            let drew = self.render().is_some();
            if !drew && !self.needs_render() && self.app.lifecycle.is_empty() {
                return;
            }
        }
    }

    /// Move the app's clock on by `by`, a step at a time, and draw after
    /// each step: every timer, [`after`], animation and toast in between
    /// runs at its own time, as it would with a real clock. A step goes
    /// to the next thing due, 50 ms at most; in an app that
    /// [waits for its tasks](App::wait_for_tasks), a
    /// [`latest`](crate::latest) thread asleep on the app's clock
    /// ([`Latest::sleep`](crate::Latest::sleep),
    /// [`latest_every`](crate::latest_every)) is a thing due, so each of
    /// its values is seen. A test of something timed.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use std::time::Duration;
    ///
    /// let app = App::new(|| {
    ///     let ticks = signal(0);
    ///     every(Duration::from_millis(100), move |_| ticks.update(|t| *t += 1));
    ///     text!("{ticks}")
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.advance(Duration::from_secs(1));
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "10");
    /// ```
    pub fn advance(&mut self, by: Duration) {
        let end = self.app.now + by;
        self.settle();
        while !self.done && self.app.now < end {
            let step = self.timeout(self.app.now).max(Duration::from_millis(1));
            let next = (self.app.now + step).min(end);
            self.update(next);
            self.settle();
        }
    }

    /// Press keys by name, separated by spaces (`"down down enter"`,
    /// `"ctrl+s"`; the space key is `space`), each followed by
    /// [`settle`](Self::settle). Keys after the app quits are dropped.
    ///
    /// # Panics
    /// On a name that is not a key.
    pub fn press(&mut self, keys: &str) {
        self.settle();
        for key in rich_interact::keymap::keys(keys) {
            if self.done {
                return;
            }
            self.event(Event::Key(key));
            self.settle();
        }
    }

    /// Type `text`, one key a character, then [`settle`](Self::settle):
    /// what a test does to fill in a text box.
    ///
    /// ```
    /// use intuituive::interact::Input;
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let name = signal(String::new());
    ///     column([
    ///         component(Input::new("Name"), move |answer, _| name.set(answer)),
    ///         text!("Hello, {name}"),
    ///     ])
    /// });
    /// let mut driver = app.driver(30, 4);
    /// driver.type_text("Ada");
    /// driver.press("enter");
    /// assert!(driver.screen().plain().iter().any(|row| row.contains("Hello, Ada")));
    /// ```
    pub fn type_text(&mut self, text: &str) {
        self.settle();
        for c in text.chars() {
            if self.done {
                return;
            }
            self.event(Event::Key(Key::char(c)));
        }
        self.settle();
    }

    /// The node that has the focus on the top screen, if any.
    pub fn focused(&self) -> Option<NodeId> {
        self.app.focus()
    }

    /// The focused node's [name](crate::Node::name), else its accessible
    /// name (its [label](crate::Node::label), the text it shows, a panel's
    /// title), if any.
    pub fn focused_name(&self) -> Option<String> {
        let id = self.focused()?;
        let mut name = None;
        with_node(&self.app.top().root, id, &mut |node| {
            name = node.given_name().map(str::to_string);
        });
        name.or_else(|| {
            self.accessibility()
                .into_iter()
                .find(|node| node.id == id)
                .map(|node| node.name)
                .filter(|name| !name.is_empty())
        })
    }

    /// Assert that the focus is on the node called `name`: its
    /// [name](crate::Node::name), or its accessible name (see
    /// [`focused_name`](Self::focused_name)).
    ///
    /// # Panics
    /// When the focus is elsewhere, or nowhere, saying where it is.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     column([
    ///         label("Save").name("save").focusable(),
    ///         label("Quit").name("quit").focusable(),
    ///     ])
    /// });
    /// let mut driver = app.driver(10, 2);
    /// driver.settle();
    /// driver.assert_focused("save");
    /// driver.press("tab");
    /// driver.assert_focused("quit");
    /// ```
    #[track_caller]
    pub fn assert_focused(&self, name: &str) {
        if let Err(message) = self.check_focused(name) {
            panic!("{message}");
        }
    }

    /// [`assert_focused`](Self::assert_focused) as a result: the message
    /// it would panic with when the focus is not on `name`.
    pub fn check_focused(&self, name: &str) -> Result<(), String> {
        let Some(id) = self.focused() else {
            return Err(format!(
                "expected the focus on `{name}`, but nothing has the focus"
            ));
        };
        let mut given = None;
        with_node(&self.app.top().root, id, &mut |node| {
            given = node.given_name().map(str::to_string);
        });
        let tree = self.accessibility();
        let access = tree.iter().find(|node| node.id == id);
        if given.as_deref() == Some(name) || access.is_some_and(|node| node.name == name) {
            return Ok(());
        }
        let what = match (given, access) {
            (Some(given), Some(node)) => format!("`{given}` ({})", node.describe()),
            (Some(given), None) => format!("`{given}`"),
            (None, Some(node)) if !node.describe().is_empty() => node.describe(),
            _ => format!("node {id}"),
        };
        Err(format!(
            "expected the focus on `{name}`, but it is on {what}"
        ))
    }

    /// The frame as drawn, cell by cell.
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// Where the last frame left the terminal's cursor: on the focused text
    /// box's caret (in [accessible](App::accessible) mode, on whatever has
    /// the focus), or `None` while it is hidden. A renderer that draws the
    /// [screen](Self::screen) itself, such as a browser's DOM, draws the
    /// caret here.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.app.caret_at
    }

    /// The last frame's numbers.
    pub fn stats(&self) -> FrameStats {
        self.app.stats
    }

    /// The node selected in the [inspector](crate::inspect), if any.
    pub fn inspector_selected(&self) -> Option<NodeId> {
        self.app.inspector.as_ref().and_then(|i| i.selected)
    }

    /// The inspector's event trace, oldest first: what happened while it
    /// was open (nothing is traced while it is closed or its trace is
    /// paused), the last [`TRACE_LIMIT`](crate::inspect::TRACE_LIMIT)
    /// events at most. Events since the last frame are in it, numbered for
    /// the next. Empty without an inspector.
    ///
    /// ```
    /// use intuituive::inspect::TraceKind;
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let count = signal(0);
    ///     text!("{count}").name("count").on_key("+", move |_| count.update(|n| *n += 1))
    /// })
    /// .inspector(true);
    /// let mut driver = app.driver(80, 10);
    /// driver.press("+");
    /// let trace = driver.inspector_trace();
    /// assert!(trace.iter().any(|e| e.kind == TraceKind::Key && e.text == "key + → count on_key"));
    /// assert!(trace.iter().any(|e| e.kind == TraceKind::Signal && e.text.contains("→ count")));
    /// ```
    pub fn inspector_trace(&self) -> Vec<TraceEntry> {
        let Some(inspector) = &self.app.inspector else {
            return Vec::new();
        };
        let tracer = &self.app.runtime.tracer;
        let pending = if tracer.has_pending() {
            tracer.peek(&self.names(), inspector.frames + 1)
        } else {
            Vec::new()
        };
        inspector.trace_entries(pending)
    }

    /// What the inspector measured of each node while it was open, in the
    /// order its timings pane shows them (by default, the most time first).
    /// Empty without an inspector.
    pub fn inspector_timings(&self) -> Vec<NodeTiming> {
        match &self.app.inspector {
            Some(inspector) => inspector.node_timings(&self.names()),
            None => Vec::new(),
        }
    }

    /// The nodes on every screen of the stack, named as the inspector
    /// names them.
    fn names(&self) -> Names {
        Names::of(self.app.layers.iter().map(|layer| &layer.root))
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

    /// What the app shows, for assistive technology: the nodes on the top
    /// screen with a role other than layout (or a
    /// [label](crate::Node::label)), outermost first, each with its depth,
    /// its name, what is selected in it and its states; nodes
    /// [hidden](crate::Node::access_hidden) from assistive technology, and
    /// what is inside them, are left out. A screen reader bridge or a
    /// browser's DOM renders this.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::a11y::Role;
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     column([
    ///         label("Files").label("Heading"),
    ///         list(|| vec!["a.txt".into(), "b.txt".into()], signal(1)),
    ///     ])
    /// });
    /// let mut driver = app.driver(20, 3);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// let tree = driver.accessibility();
    /// assert_eq!((tree[0].role, tree[0].name.as_str()), (Role::Text, "Heading"));
    /// assert_eq!((tree[1].role, tree[1].value.as_deref()), (Role::List, Some("b.txt")));
    /// assert!(tree[1].focused);
    /// ```
    pub fn accessibility(&self) -> Vec<crate::a11y::AccessNode> {
        self.app.accessibility(&self.screen)
    }

    /// The announcements since the last call (the last 64): toasts, a
    /// screen or dialog opening, [live](crate::Node::live) nodes changing,
    /// [`Ctx::announce`]. A browser puts them in an ARIA live region.
    pub fn take_announcements(&mut self) -> Vec<crate::a11y::Announcement> {
        std::mem::take(&mut self.app.announcements)
    }

    /// Whether the app draws for assistive technology
    /// ([`App::accessible`]).
    pub fn is_accessible(&self) -> bool {
        self.app.accessible
    }

    /// Whether the app writes lines of text instead of drawing
    /// ([`App::linear`]): [`render`](Self::render) then returns them.
    pub fn is_linear(&self) -> bool {
        self.app.linear
    }

    /// `text` is on the clipboard: a toast says so.
    pub fn copied(&mut self, text: &str) {
        let n = text.chars().count();
        let until = self.app.now + Duration::from_secs(2);
        self.app.push_toast(
            Note::plain(format!(
                "Copied {n} character{}",
                if n == 1 { "" } else { "s" }
            )),
            until,
        );
        self.app.restack = true;
    }

    /// The terminal is about to be given back for a while (Ctrl+Z, a
    /// command run in it): the bytes that end the frame (inline, the cursor
    /// goes below the region). Once the terminal is back, call
    /// [`resize`](Self::resize) with its size, and the next
    /// [`render`](Self::render) draws everything again (inline, in a new
    /// region where the cursor is then).
    pub fn suspend(&mut self) -> String {
        let mut out = self.painter.finish();
        out.push_str("\x1b[0m\x1b[?25h");
        self.painter.invalidate();
        self.first = true;
        self.started = false;
        // Back, an inline region starts where the cursor is then.
        self.shifted = 0;
        // The terminal's modes come back without pointer movement reports:
        // asked for again once drawn.
        self.app.motion.1 = false;
        self.app.caret_shown = false;
        out
    }

    /// The next program a handler asked to give the terminal to
    /// ([`Ctx::handoff`]), taken. A loop of your own writes
    /// [`suspend`](Self::suspend)'s bytes, runs the command with the
    /// terminal as it was before the app (out of raw mode, off the
    /// alternate screen: rs-rich-interact's [`Backend::handoff`]), takes the
    /// terminal back, calls [`resize`](Self::resize) with its size, then
    /// [`handed_back`](Self::handed_back). [`App::run`] does all this.
    pub fn take_handoff(&mut self) -> Option<Handoff> {
        (!self.app.handoffs.is_empty()).then(|| self.app.handoffs.remove(0))
    }

    /// `handoff`'s program ended with `result`: its
    /// [`handoff_then`](Ctx::handoff_then) runs, and everything draws
    /// again.
    pub fn handed_back(&mut self, handoff: Handoff, result: io::Result<Option<i32>>) {
        self.first = true;
        self.painter.invalidate();
        let Some(then) = handoff.then else {
            return;
        };
        let app = &mut self.app;
        let mut cx = app.ctx();
        let runtime = app.runtime.clone();
        runtime.enter(|| then(result, &mut cx));
        if app.apply(cx) {
            self.done = true;
        }
    }

    /// Inline: the terminal row the app's region starts on, for placing
    /// mouse events.
    pub fn set_origin(&mut self, row: u16) {
        self.origin = row;
    }

    /// The value a handler ended the app with ([`Ctx::quit_with`]), if it
    /// is a `T`; taken, so a second call returns `None`.
    pub fn take_exit<T: 'static>(&mut self) -> Option<T> {
        let value = self.app.exit.take()?;
        match value.downcast::<T>() {
            Ok(value) => Some(*value),
            Err(value) => {
                self.app.exit = Some(value);
                None
            }
        }
    }

    /// The error a handler ended the app with ([`Ctx::fail`]), taken.
    pub fn take_error(&mut self) -> Option<io::Error> {
        self.app.error.take()
    }

    /// The console markup handlers printed above the app
    /// ([`Ctx::print_above`]) that is waiting to be printed, taken: in a
    /// full-screen app, to print once the terminal is given back. Inline,
    /// [`render`](Self::render) writes them above the region itself, so
    /// none wait.
    pub fn take_printed(&mut self) -> Vec<String> {
        std::mem::take(&mut self.app.printed)
    }

    /// `markup` lines as the terminal shows them: rendered at the app's
    /// width, wrapped, one string of text and escape sequences a row.
    fn printed_rows(&self, markup: &[String]) -> Vec<String> {
        let system = self.console.color_system();
        let width = self.screen.area().width.max(1) as usize;
        let options = self.console.options().update_width(width);
        let mut rows = Vec::new();
        for markup in markup {
            let text =
                rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup.clone()));
            for line in self.console.render_lines(&text, &options, false) {
                let mut row = String::new();
                for segment in line {
                    let mut text = [segment];
                    rich_interact::paint::sanitize_line(&mut text);
                    let [segment] = text;
                    match &segment.style {
                        Some(style) => row.push_str(&style.render(&segment.text, system)),
                        None => row.push_str(&segment.text),
                    }
                }
                rows.push(row);
            }
        }
        rows
    }

    /// Inline: write the printed lines into the scrollback above the
    /// region, which then starts below them; the next paint draws it all.
    fn print_inline(&mut self) -> String {
        let markup = std::mem::take(&mut self.app.printed);
        let rows = self.printed_rows(&markup);
        // From the region's first row, cleared to the end of the screen,
        // the lines push the region down (scrolling the terminal when it
        // is at the bottom).
        let mut out = String::from("\x1b[?25l");
        out.push_str(&self.painter.move_to(0, 0));
        out.push_str("\x1b[0m\r\x1b[J");
        for row in &rows {
            out.push_str(row);
            out.push_str("\x1b[0m\r\n");
        }
        self.app.caret_shown = false;
        self.painter.invalidate();
        self.first = true;
        self.shifted = self
            .shifted
            .saturating_add(rows.len().min(u16::MAX as usize) as u16);
        out
    }

    /// Stop: the bytes that show the last change and leave the terminal
    /// as it was (styles reset, the cursor shown; inline, below the
    /// region, where the shell's prompt follows the app's last frame).
    pub fn finish(mut self) -> String {
        let mut out = String::new();
        if self.app.linear {
            // Nothing was drawn, so there is nothing to put back.
            self.app.runtime.close();
            return out;
        }
        // Inline, the last frame stays in the scrollback: show what the
        // last handler changed (and printed) before leaving.
        if self.app.inline.is_some() && !self.app.printed.is_empty() {
            out.push_str(&self.print_inline());
            out.push_str(&self.paint(true));
        } else if self.app.inline.is_some() && (self.app.restack || self.app.runtime.has_dirty()) {
            out.push_str(&self.paint(false));
        }
        if let Some(source) = &self.app.graphics {
            // Inline, the last frame stays, with its graphics.
            let retained = match self.app.inline {
                Some(_) => self.app.placements.clone(),
                None => Vec::new(),
            };
            out.push_str(&source.release(&retained));
        }
        out.push_str(&self.painter.finish());
        out.push_str("\x1b[0m\x1b[?25h");
        self.app.runtime.close();
        out
    }

    /// Draw what changed: the bytes to send.
    fn paint(&mut self, mut full: bool) -> String {
        let app = &mut self.app;
        app.poke = false;
        let screen = &mut self.screen;
        // The inspector may have opened or closed since the last frame.
        app.set_viewport(screen.area(), self.rows);
        if app.linear {
            let columns = screen.area().width;
            let rows = app.linear_rows(&self.console, columns, self.rows);
            if rows != screen.area().height {
                *screen = Screen::new(columns, rows);
                full = true;
            }
        }
        let mut damage = app.frame(&self.console, screen, full);
        if app.linear {
            let out = self.lines();
            self.app.stats.bytes = out.len();
            return out;
        }
        if let Some((start, end)) = app.selection {
            let style = Style::parse("reverse").expect("a built-in style parses");
            for (x, y) in selected_cells(screen.area(), start, end) {
                screen.restyle(x, y, &style);
            }
            damage.push(screen.area());
        }
        let painted = self.painter.paint(screen, &damage);
        let mut out = String::new();
        // Accessible: the cursor is hidden while the frame is written, so a
        // screen reader following it reads none of the cells it passes, and
        // shown again once parked on the focus.
        if app.accessible && app.caret_shown && !painted.is_empty() {
            out.push_str("\x1b[?25l");
            app.caret_shown = false;
        }
        out.push_str(&painted);
        if let Some(source) = app.graphics.clone() {
            out.push_str(&draw_graphics(
                &*source,
                &mut app.placements,
                screen,
                &mut self.painter,
            ));
        }
        if app.motion.0 && !app.motion.1 && app.captures_mouse() {
            // A widget reads hover: report the pointer's movement too. The
            // session turns it off with the rest of the mouse on the way out.
            out.push_str("\x1b[?1003h");
            app.motion.1 = true;
        }
        out.push_str(&app.caret(&mut self.painter));
        app.stats.bytes = out.len();
        out
    }

    /// Linear mode: the lines of the nodes whose line changed since they
    /// were last written (all of them the first time), in reading order,
    /// the focus with `→ ` when it moved, then what was announced. Each
    /// ends with `\r\n`: the terminal is in raw mode. A screen that
    /// closes leaves the one below as it was last written, so going back
    /// says only what changed there, and where the focus is.
    fn lines(&mut self) -> String {
        let tree = self.app.accessibility(&self.screen);
        let screens = self.app.layers.len();
        self.written.resize_with(screens, HashMap::new);
        let before = &self.written[screens - 1];
        let focus = tree.iter().find(|node| node.focused).map(|node| node.id);
        let moved = focus != self.written_focus;
        let mut lines = Vec::new();
        let mut written = HashMap::new();
        for node in &tree {
            let line = node.describe();
            if !line.is_empty() {
                if node.focused && moved {
                    lines.push(format!("→ {line}"));
                } else if before.get(&node.id) != Some(&line) {
                    lines.push(line.clone());
                }
            }
            written.insert(node.id, line);
        }
        self.written[screens - 1] = written;
        self.written_focus = focus;
        lines.append(&mut self.app.unwritten);
        // Names, values and announcements are the app's text, so terminal
        // controls in them become visible symbols, as on the cell screen.
        lines
            .into_iter()
            .map(|line| {
                let mut line = [rich::Segment::new(line, None)];
                rich_interact::paint::sanitize_line(&mut line);
                format!("{}\r\n", line[0].text)
            })
            .collect()
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

/// How long the pointer rests on a node before its tooltip shows.
const TOOLTIP_DELAY: Duration = Duration::from_millis(600);

/// A node's tooltip, waiting or showing.
struct Tip {
    node: NodeId,
    markup: String,
    /// The box's top left, and the row its bottom goes above when there is
    /// no room below.
    at: (u16, u16),
    above: u16,
    /// When the pointer came to rest.
    since: Duration,
    shown: bool,
}

/// A drag from a [`draggable`](crate::Node::draggable) node.
struct Drag {
    source: NodeId,
    value: std::rc::Rc<dyn std::any::Any>,
    /// Where the press was: the drag starts once the pointer leaves it.
    from: (u16, u16),
    active: bool,
    /// The drop target under the pointer that takes the value.
    target: Option<NodeId>,
}

/// Lay `style` over every cell of `rect`.
fn restyle_rect(screen: &mut Screen, rect: Rect, style: &Style) {
    for row in rect.y..rect.bottom() {
        for column in rect.x..rect.right() {
            screen.restyle(column, row, style);
        }
    }
}

/// Draw `tip` in `area`, by where it asked to be; where it went.
fn draw_tip(console: &Console, screen: &mut Screen, area: Rect, tip: &Tip, style: &Style) -> Rect {
    if area.width < 3 || area.height == 0 {
        return Rect::default();
    }
    let text = rich::Text::from_markup(&tip.markup)
        .unwrap_or_else(|_| rich::Text::new(tip.markup.clone()));
    let widest = text
        .split("\n", false, true)
        .iter()
        .map(|line| line.cell_len())
        .max()
        .unwrap_or(0) as u16;
    let inner = widest.clamp(1, area.width.saturating_sub(2).min(48));
    let options = console.options().update_width(inner as usize);
    let lines = console.render_lines(&text, &options, false);
    let height = (lines.len() as u16).clamp(1, area.height);
    let width = inner + 2;
    let y = if tip.at.1.saturating_add(height) <= area.bottom() {
        tip.at.1
    } else {
        tip.above.saturating_sub(height).max(area.y)
    };
    let x = tip.at.0.min(area.right().saturating_sub(width)).max(area.x);
    let rect = Rect::new(x, y, width, height);
    screen.clear(rect);
    screen.write_lines(
        Rect::new(x + 1, y, inner, height),
        &lines[..height as usize],
    );
    restyle_rect(screen, rect, style);
    rect
}

/// The screens drawn: the topmost full screen and the modals over it.
fn shown_layers(layers: &[Layer]) -> &[Layer] {
    let base = layers
        .iter()
        .rposition(|layer| layer.modal.is_none())
        .unwrap_or(0);
    &layers[base..]
}

/// The deepest node shown at `x`, `y`, on the topmost screen there (a
/// point outside a modal is on the screen below it).
fn node_at(layers: &[Layer], x: u16, y: u16) -> Option<NodeId> {
    shown_layers(layers).iter().rev().find_map(|layer| {
        crate::node::hit_path(&layer.root, x, y)
            .last()
            .map(|(id, _)| *id)
    })
}

/// Where node `id` is on the screen, on whichever screen drawn holds it.
fn node_rect(layers: &[Layer], id: NodeId) -> Option<Rect> {
    shown_layers(layers)
        .iter()
        .rev()
        .find_map(|layer| anchor_rect(&layer.root, id))
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
            Size::Ratio(a, b) => crate::layout::ratio_of(total as u32, a, b) as u16,
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
            Size::Ratio(a, b) => crate::layout::ratio_of(total as u32, a, b) as u16,
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

/// Set every rect handle in `node`, and in everything inside it, to `None`.
fn clear_rect_handles(node: &crate::node::Node) {
    if let Some(handle) = *node.rect_slot.borrow() {
        if handle.get_untracked().is_some() {
            handle.set(None);
        }
    }
    node.each_child(&mut |child| clear_rect_handles(child));
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

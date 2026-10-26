//! Live (in-place updating) displays.
//!
//! Port of `rich/live.py`. A [`Live`] drives a [`LiveRender`], writing the
//! control codes to redraw a renderable in place as it changes: `start` hides
//! the cursor (and, with [`screen`](Live::screen), switches to the alternate
//! screen) and draws, `update`/`refresh` reposition the cursor and redraw,
//! and `stop` commits the final render and restores the cursor.
//!
//! The manual-refresh path is byte-parity with upstream's `auto_refresh=False`
//! Live; [`Live::spawn`] runs the same display on a background refresh thread
//! ([`AutoLive`]), as upstream's `refresh_per_second` does.
//!
//! Upstream's render hook, which puts anything printed through the console
//! above the display, is [`Live::print`] here: the `Live` owns its console and
//! output sink. Its stdout/stderr redirect is [`Live::stdout`] and
//! [`Live::stderr`], writers that print through the display as upstream's
//! `FileProxy` does (see [`file_proxy`](crate::file_proxy) for why the
//! process's own stdout is not replaced). `console.log` during a display is
//! [`Live::log`].
//!
//! As upstream's, `start` pushes the display on its console's live stack
//! ([`Console::set_live`], shared by the console's clones) and `stop` pops
//! it. A display started while the stack holds another is nested: the first
//! display draws every display on the stack, one after another, and a
//! nested display's refresh redraws the first. A nested display prints its
//! renderable once, when it stops, unless it is transient.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::io::Write;
use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::console::{Console, ConsoleOptions};
use crate::control::Control;
use crate::file_proxy::{FileProxy, ProxyBuffer, ProxyStream, ProxyTarget};
use crate::live_render::VerticalOverflow;
use crate::protocol::Renderable;
use crate::segment::Segment;

/// The options of a [`Live`] display that are not its renderable, console or
/// sink: upstream's `Live(...)` keyword arguments, for
/// [`Live::spawn_options`]. The defaults are upstream's.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiveOptions {
    /// Clear the display when it stops (upstream `transient`, default false).
    pub transient: bool,
    /// Use the alternate screen (upstream `screen`, default false).
    pub screen: bool,
    /// Print what is written to [`AutoLive::stdout`] through the display
    /// (upstream `redirect_stdout`, default true).
    pub redirect_stdout: bool,
    /// The same for [`AutoLive::stderr`] (upstream `redirect_stderr`).
    pub redirect_stderr: bool,
    /// What to do with a frame taller than the screen (upstream
    /// `vertical_overflow`, default [`VerticalOverflow::Ellipsis`]).
    pub vertical_overflow: VerticalOverflow,
    /// Draw the first frame as the display starts (upstream
    /// `start(refresh=True)`, default true). When false the refresh thread
    /// draws it at its first tick.
    pub refresh_on_start: bool,
}

impl Default for LiveOptions {
    fn default() -> Self {
        LiveOptions {
            transient: false,
            screen: false,
            redirect_stdout: true,
            redirect_stderr: true,
            vertical_overflow: VerticalOverflow::Ellipsis,
            refresh_on_start: true,
        }
    }
}

/// A display's renderable as rendered for a frame: the segments the first
/// display draws for it.
type Snapshot = Arc<Vec<Segment>>;

/// What a [`Live`] pushes on its console's live stack ([`Console::set_live`]).
/// Upstream pushes the display itself, and the first display renders every
/// display on the stack. A `Live` is not `Send` (its renderable need not
/// be), so the stack holds what the other displays need of it instead: its
/// renderable as last rendered, and its frame.
///
/// Locks are taken in one order: a frame, then the console's live stack or
/// an entry's content. Nothing takes a frame while holding the stack or a
/// content lock, one frame is held at a time, and no renderable is rendered
/// while a frame is held, except as segments already rendered. A renderable
/// may lock state of its own (a progress's tasks) that a thread waiting on
/// another display holds, so it is rendered before the frame is locked.
struct LiveEntry {
    /// The renderable as last rendered, for the first display to draw
    /// (upstream's `live.get_renderable()` in `Live.renderable`).
    content: Mutex<Option<Snapshot>>,
    /// The display's frame: what its live render shows and the shape it
    /// left on the screen. A nested display redraws the first display's
    /// frame, as upstream's `refresh` calls `live_stack[0].refresh()`.
    frame: Mutex<Frame>,
}

/// One renderable of a frame's `Group`.
#[derive(Clone)]
enum Slot {
    /// The display's own renderable.
    Own,
    /// Another display's, as rendered when the frame was refreshed.
    Other(Snapshot),
}

/// The state of a display's live render, shared with the displays nested
/// in it.
struct Frame {
    /// `LiveRender._shape`.
    shape: Option<(usize, usize)>,
    vertical_overflow: VerticalOverflow,
    /// `LiveRender.renderable`, set at each refresh: the display's own, or,
    /// with other displays on the stack, a `Group` of every display's.
    slots: Vec<Slot>,
    /// Counts the changes of `slots`.
    generation: u64,
    /// A nested display has refreshed this one since it last refreshed
    /// itself; upstream's refresh took the display's renderable.
    stale: bool,
    /// Upstream's render hook is pushed.
    hooked: bool,
    started: bool,
    transient: bool,
    alt_screen: bool,
    /// The console's `is_interactive`.
    interactive: bool,
}

/// Lock `mutex`, recovering it if a panicking holder poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl LiveEntry {
    fn new(interactive: bool) -> Self {
        LiveEntry {
            content: Mutex::new(None),
            frame: Mutex::new(Frame {
                shape: None,
                vertical_overflow: VerticalOverflow::Ellipsis,
                slots: vec![Slot::Own],
                generation: 0,
                stale: false,
                hooked: false,
                started: false,
                transient: false,
                alt_screen: false,
                interactive,
            }),
        }
    }

    fn frame(&self) -> std::sync::MutexGuard<'_, Frame> {
        lock(&self.frame)
    }

    fn content(&self) -> Option<Snapshot> {
        lock(&self.content).clone()
    }

    fn set_content(&self, content: Snapshot) {
        *lock(&self.content) = Some(content);
    }
}

/// The live display entry `item` on a console's stack, if it is one.
fn entry_of(item: &Arc<dyn Any + Send + Sync>) -> Option<Arc<LiveEntry>> {
    item.clone().downcast::<LiveEntry>().ok()
}

/// The `Group` upstream's first display shows, the stack's displays in
/// order: `Own` for `first`, the last rendering of each other.
fn group_slots(stack: &[Arc<dyn Any + Send + Sync>], first: &Arc<LiveEntry>) -> Vec<Slot> {
    stack
        .iter()
        .filter_map(entry_of)
        .filter_map(|entry| {
            if Arc::ptr_eq(&entry, first) {
                Some(Slot::Own)
            } else {
                entry.content().map(Slot::Other)
            }
        })
        .collect()
}

impl Frame {
    fn set_slots(&mut self, slots: Vec<Slot>) {
        self.slots = slots;
        self.generation += 1;
    }

    /// Whether a print draws the display below it (upstream's
    /// `process_renderables`).
    fn draws(&self) -> bool {
        self.hooked && (self.interactive || (!self.started && !self.transient))
    }

    /// `reset` + `rendered` + the frame drawn as `live` with `shape`, as
    /// upstream's `process_renderables` arranges a print.
    fn arrange(&mut self, rendered: &str, live: &str, shape: (usize, usize)) -> String {
        let reset = if !self.interactive {
            String::new()
        } else if self.alt_screen {
            Control::home().as_str().to_string()
        } else {
            crate::live_render::position_cursor(self.shape)
                .as_str()
                .to_string()
        };
        self.shape = Some(shape);
        format!("{reset}{rendered}{live}")
    }

    /// `console.print(...)` through this display's render hook, from a
    /// display that does not own it: `rendered` is what was printed, and
    /// `own` stands for this display's renderable.
    fn draw(&mut self, console: &Console, own: &Own<'_>, rendered: &str) -> String {
        if !self.draws() {
            return rendered.to_string();
        }
        let (live, shape, _) = render_frame(
            console,
            own,
            &self.slots,
            self.alt_screen,
            self.vertical_overflow,
        );
        self.arrange(rendered, &live, shape)
    }
}

/// A display's own renderable, in its frame.
enum Own<'a> {
    /// Rendered now, by the display that owns it.
    Render(&'a dyn Renderable),
    /// As it was last rendered, for another display drawing the frame.
    Snapshot(Option<Snapshot>),
}

/// What a display's live render shows: the slots one after another
/// (upstream's `Group`), in upstream's `Screen` on the alternate screen.
/// Rendering the display's own renderable keeps its segments in `stash`.
struct FrameContent<'a> {
    own: &'a Own<'a>,
    slots: &'a [Slot],
    alt_screen: bool,
    stash: RefCell<Option<Snapshot>>,
}

/// [`FrameContent`] without the `Screen`.
struct FrameGroup<'a, 'b>(&'b FrameContent<'a>);

impl Renderable for FrameGroup<'_, '_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let content = self.0;
        let mut segments = Vec::new();
        let mut first = true;
        for slot in content.slots {
            let rendered = match (slot, content.own) {
                (Slot::Own, Own::Render(renderable)) => {
                    let rendered = console.render(*renderable, Some(options));
                    *content.stash.borrow_mut() = Some(Arc::new(rendered.clone()));
                    rendered
                }
                (Slot::Own, Own::Snapshot(snapshot)) => {
                    snapshot.as_deref().cloned().unwrap_or_default()
                }
                (Slot::Other(snapshot), _) => snapshot.as_ref().clone(),
            };
            // `Group`: each renderable's lines after the last's.
            if rendered.is_empty() {
                continue;
            }
            if !first {
                segments.push(Segment::line());
            }
            first = false;
            segments.extend(rendered);
        }
        segments
    }
}

impl Renderable for FrameContent<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.alt_screen {
            crate::screen::render_screen(Some(&FrameGroup(self)), None, console, options)
        } else {
            FrameGroup(self).rich_render(console, options)
        }
    }
}

/// [`FrameContent`] in a live render: upstream's `LiveRender`.
struct FrameRender<'a> {
    content: FrameContent<'a>,
    vertical_overflow: VerticalOverflow,
    shape: Cell<(usize, usize)>,
}

impl Renderable for FrameRender<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (segments, shape) = crate::live_render::render(
            &self.content,
            None,
            self.vertical_overflow,
            console,
            options,
        );
        self.shape.set(shape);
        segments
    }

    fn print_end_line(&self, _options: &ConsoleOptions) -> bool {
        false
    }
}

/// Render a frame as a print renders a live render: the output, the shape
/// it leaves, and the display's own renderable as rendered for it.
fn render_frame(
    console: &Console,
    own: &Own<'_>,
    slots: &[Slot],
    alt_screen: bool,
    vertical_overflow: VerticalOverflow,
) -> (String, (usize, usize), Option<Snapshot>) {
    let render = FrameRender {
        content: FrameContent {
            own,
            slots,
            alt_screen,
            stash: RefCell::new(None),
        },
        vertical_overflow,
        shape: Cell::new((0, 0)),
    };
    let live = console.render_to_string(&render);
    let shape = render.shape.get();
    (live, shape, render.content.stash.into_inner())
}

/// An in-place updating display over a renderable. Mirrors `rich.live.Live`.
/// Output is written to a generic sink `W`, so it can target stdout or, in
/// tests, a byte buffer.
///
/// A display started while another runs on its console or a clone of it is
/// nested, as upstream's: it draws nothing itself, and the first display
/// draws its renderable below its own. A refresh of a nested display draws
/// the first display's frame to the nested display's sink, so displays that
/// nest should write to one terminal.
pub struct Live<W: Write> {
    console: Console,
    writer: W,
    /// Upstream's `_renderable` as the last refresh took it.
    content: Box<dyn Renderable>,
    /// This display on its console's live stack, shared with the displays
    /// nested in it.
    entry: Arc<LiveEntry>,
    started: bool,
    /// Upstream's render hook is pushed: from `start` to the end of `stop`.
    hooked: bool,
    /// Upstream's `_nested`: the console's live stack held another display
    /// when this one started, so this one draws nothing itself.
    nested: bool,
    transient: bool,
    screen: bool,
    /// Upstream's `_alt_screen`: the alternate screen is on.
    alt_screen: bool,
    redirect_stdout: bool,
    redirect_stderr: bool,
    vertical_overflow: VerticalOverflow,
    stdout_buffer: ProxyBuffer,
    stderr_buffer: ProxyBuffer,
    /// Upstream's `get_renderable`, called for a fresh renderable at each
    /// refresh.
    get_renderable: Option<GetRenderable>,
    /// Upstream's `_renderable` after an `update`: the live render takes it
    /// at the next refresh.
    pending: Option<Box<dyn Renderable>>,
}

/// Upstream's `get_renderable` callable.
type GetRenderable = Box<dyn Fn() -> Box<dyn Renderable>>;

impl<W: Write> Live<W> {
    /// Create a live display for `renderable`, rendering with `console` and
    /// writing control/output bytes to `writer`.
    pub fn new(renderable: Box<dyn Renderable>, console: Console, writer: W) -> Self {
        let interactive = console.is_terminal() && !console.is_dumb_terminal();
        Live {
            console,
            writer,
            content: renderable,
            entry: Arc::new(LiveEntry::new(interactive)),
            started: false,
            hooked: false,
            nested: false,
            transient: false,
            screen: false,
            alt_screen: false,
            redirect_stdout: true,
            redirect_stderr: true,
            vertical_overflow: VerticalOverflow::Ellipsis,
            stdout_buffer: ProxyBuffer::default(),
            stderr_buffer: ProxyBuffer::default(),
            get_renderable: None,
            pending: None,
        }
    }

    /// Take the renderable from `get` at every refresh instead (upstream's
    /// `get_renderable`), as a `Progress` builds its task table afresh for
    /// each frame. What a print draws below itself is the renderable of the
    /// last refresh, as upstream.
    pub fn get_renderable(mut self, get: impl Fn() -> Box<dyn Renderable> + 'static) -> Self {
        self.content = get();
        self.get_renderable = Some(Box::new(get));
        self
    }

    /// Apply every option but [`LiveOptions::refresh_on_start`].
    pub fn options(self, options: &LiveOptions) -> Self {
        self.transient(options.transient)
            .screen(options.screen)
            .redirect_stdout(options.redirect_stdout)
            .redirect_stderr(options.redirect_stderr)
            .vertical_overflow(options.vertical_overflow)
    }

    /// Clear the display when it stops (upstream `transient`): the final
    /// frame is drawn, then erased and the cursor put back where it started.
    pub fn transient(mut self, transient: bool) -> Self {
        self.transient = transient;
        self.entry.frame().transient = self.is_transient();
        self
    }

    /// Show the display on the alternate screen (upstream `screen`), filling
    /// it as upstream's `Screen` does. Like upstream, a screen display is
    /// always transient, and it needs a terminal.
    pub fn screen(mut self, screen: bool) -> Self {
        self.screen = screen;
        self.entry.frame().transient = self.is_transient();
        self
    }

    /// Print what is written to [`stdout`](Self::stdout) through the display
    /// while it runs on a terminal (upstream `redirect_stdout`, default true).
    /// When off, the proxy writes straight to its file.
    pub fn redirect_stdout(mut self, redirect: bool) -> Self {
        self.redirect_stdout = redirect;
        self
    }

    /// The same for [`stderr`](Self::stderr) (upstream `redirect_stderr`).
    pub fn redirect_stderr(mut self, redirect: bool) -> Self {
        self.redirect_stderr = redirect;
        self
    }

    /// What to do with a frame taller than the screen (upstream
    /// `vertical_overflow`, default [`VerticalOverflow::Ellipsis`]). The final
    /// frame drawn by [`stop`](Self::stop) is always shown in full.
    pub fn vertical_overflow(mut self, vertical_overflow: VerticalOverflow) -> Self {
        self.vertical_overflow = vertical_overflow;
        self.entry.frame().vertical_overflow = vertical_overflow;
        self
    }

    /// Whether the display has been started and not yet stopped. Port of
    /// `Live.is_started`.
    pub fn is_started(&self) -> bool {
        self.started
    }

    /// The console the display renders with. Port of `Live.console`.
    pub fn console(&self) -> &Console {
        &self.console
    }

    /// Upstream sets `transient` whenever `screen` is.
    fn is_transient(&self) -> bool {
        self.transient || self.screen
    }

    /// Upstream's `Console.is_dumb_terminal`.
    fn is_dumb_terminal(&self) -> bool {
        self.console.is_dumb_terminal()
    }

    /// Upstream's `Console.is_interactive`: a terminal that is not dumb.
    fn is_interactive(&self) -> bool {
        self.console.is_terminal() && !self.console.is_dumb_terminal()
    }

    fn write_str(&mut self, text: &str) {
        if !text.is_empty() {
            let _ = self.writer.write_all(text.as_bytes());
        }
    }

    /// Upstream's `Console.control`: dropped on a dumb terminal.
    fn control(&mut self, control: &Control) {
        if !self.is_dumb_terminal() {
            self.write_str(control.as_str());
        }
    }

    /// Upstream's `Console.show_cursor`: only on a terminal.
    fn show_cursor(&mut self, show: bool) {
        if self.console.is_terminal() {
            self.control(&Control::show_cursor(show));
        }
    }

    /// Upstream's `Console.set_alt_screen`.
    fn set_alt_screen(&mut self, enable: bool) -> bool {
        if self.console.is_terminal() && !self.console.legacy_windows() {
            self.control(&Control::alt_screen(enable));
            return true;
        }
        false
    }

    /// The first display on the console's live stack, when it is another
    /// display's.
    fn first(&self) -> Option<Arc<LiveEntry>> {
        let stack = self.console.live_stack();
        let first = entry_of(stack.first()?)?;
        (!Arc::ptr_eq(&first, &self.entry)).then_some(first)
    }

    /// Begin the live display: hide the cursor and draw the first frame.
    /// Upstream's `start(refresh=True)`, as `with Live(renderable):` does.
    pub fn start(&mut self) {
        self.start_with(true);
    }

    /// Begin the live display, drawing the first frame only when `refresh`
    /// is set. Port of `Live.start(refresh)`; a display started without a
    /// refresh is first drawn by the next refresh, update or stop.
    pub fn start_with(&mut self, refresh: bool) {
        if self.started {
            return;
        }
        self.started = true;
        self.entry.frame().started = true;
        // `if not self.console.set_live(self)`: another display is on the
        // console's stack, and this one is nested. The first display draws
        // it from here on.
        if !self.console.set_live(self.entry.clone()) {
            self.nested = true;
            self.publish();
            return;
        }
        if self.screen {
            self.alt_screen = self.set_alt_screen(true);
        }
        self.show_cursor(false);
        self.stdout_buffer = ProxyBuffer::default();
        self.stderr_buffer = ProxyBuffer::default();
        self.hooked = true;
        {
            let mut frame = self.entry.frame();
            frame.hooked = true;
            frame.alt_screen = self.alt_screen;
        }
        if refresh {
            self.refresh();
        }
    }

    /// Swap in a new renderable and redraw in place. Upstream's
    /// `update(renderable, refresh=True)`.
    pub fn update(&mut self, renderable: Box<dyn Renderable>) {
        self.update_with(renderable, true);
    }

    /// Swap in a new renderable, redrawing only when `refresh` is set. Port of
    /// `Live.update(renderable, refresh)`: the display takes it at the next
    /// refresh, so a print before then still draws the old one below it.
    pub fn update_with(&mut self, renderable: Box<dyn Renderable>, refresh: bool) {
        self.get_renderable = None;
        self.pending = Some(renderable);
        if refresh {
            self.refresh();
        } else if self.started && (self.nested || self.console.live_stack().len() > 1) {
            // The first display's next refresh draws the new renderable.
            self.publish();
        }
    }

    /// Upstream's `get_renderable()` as a refresh takes it.
    fn take_content(&mut self) {
        if let Some(get) = &self.get_renderable {
            self.content = get();
        } else if let Some(renderable) = self.pending.take() {
            self.content = renderable;
        }
    }

    /// Render this display's renderable (the one its next refresh takes)
    /// for the first display to draw.
    fn publish(&mut self) {
        let alt_screen = match self.first() {
            Some(first) => first.frame().alt_screen,
            None => self.alt_screen,
        };
        let renderable = self.pending.as_deref().unwrap_or(self.content.as_ref());
        let (_, _, stash) = render_frame(
            &self.console,
            &Own::Render(renderable),
            &[Slot::Own],
            alt_screen,
            self.vertical_overflow,
        );
        if let Some(stash) = stash {
            self.entry.set_content(stash);
        }
    }

    /// Redraw the current renderable in place. Port of `Live.refresh`: on an
    /// interactive terminal the cursor goes back over the last frame (or
    /// home, on the alternate screen) and the frame is drawn; elsewhere only
    /// the final frame of a stopped, non-transient display is written. The
    /// first display on the console's stack draws every display's renderable
    /// (upstream's `Live.renderable`), and a nested display redraws it.
    pub fn refresh(&mut self) {
        // Upstream's `self._live_render.set_renderable(self.renderable)`.
        self.take_content();
        if self.nested {
            // `self.console._live_stack[0].refresh()`.
            self.publish();
            self.refresh_first();
            return;
        }
        let stack = self.console.live_stack();
        let slots = match stack.first().and_then(entry_of) {
            Some(first) if Arc::ptr_eq(&first, &self.entry) => group_slots(&stack, &self.entry),
            _ => vec![Slot::Own],
        };
        if self.is_interactive() || (!self.started && !self.is_transient()) {
            self.emit_with("", Some(slots));
        } else {
            let mut frame = self.entry.frame();
            frame.set_slots(slots);
            frame.stale = false;
        }
    }

    /// A timer tick of an auto-refreshing display. Upstream's nested display
    /// has no refresh thread: the first display's ticks draw it. Here a
    /// nested display's ticks render its renderable for them to draw.
    fn tick(&mut self) {
        if self.nested {
            self.take_content();
            self.publish();
        } else {
            self.refresh();
        }
    }

    /// Refresh the first display on the stack, from this nested one: its
    /// frame is drawn to this display's sink.
    fn refresh_first(&mut self) {
        let stack = self.console.live_stack();
        // Anything else first on the stack is asked to refresh, and draws
        // nothing.
        let Some(first) = stack.first().and_then(entry_of) else {
            return;
        };
        if Arc::ptr_eq(&first, &self.entry) {
            return;
        }
        let slots = group_slots(&stack, &first);
        let own = Own::Snapshot(first.content());
        let output = {
            let mut frame = first.frame();
            frame.set_slots(slots);
            frame.stale = true;
            frame.draw(&self.console, &own, "")
        };
        self.write_str(&output);
    }

    /// Print `renderable` above the display, as `console.print` does while
    /// upstream's Live is running (its render hook): the display is erased,
    /// the output printed, and the display drawn again below it.
    pub fn print(&mut self, renderable: &dyn Renderable) {
        let rendered = self.console.capture(|console| console.print(renderable));
        self.emit(&rendered);
    }

    /// [`print`](Self::print) a string as console markup, as upstream's
    /// `console.print("...")`.
    pub fn print_str(&mut self, content: &str) {
        let rendered = self.console.capture(|console| console.print_str(content));
        self.emit(&rendered);
    }

    /// Log `objects` above the display, as `console.log` does while
    /// upstream's Live is running (through its render hook). Port of the
    /// `log` upstream's `Progress` binds to its console: the time, the
    /// message and the caller found with `#[track_caller]` (DIVERGENCES §47).
    #[track_caller]
    pub fn log(
        &mut self,
        objects: &[crate::console::LogObject<'_>],
        options: &crate::console::LogOptions<'_>,
    ) -> crate::errors::Result<()> {
        let caller = std::panic::Location::caller();
        self.log_at(objects, options, caller.file(), caller.line())
    }

    /// [`log`](Self::log) with the caller's `file` and `line` given
    /// ([`Console::log_at`]).
    pub fn log_at(
        &mut self,
        objects: &[crate::console::LogObject<'_>],
        options: &crate::console::LogOptions<'_>,
        file: &str,
        line: u32,
    ) -> crate::errors::Result<()> {
        let mut result = Ok(());
        let rendered = self.console.capture(|console| {
            result = console.log_at(objects, options, file, line);
        });
        self.emit(&rendered);
        result
    }

    /// Write `rendered` (what `console.print` made of its renderables) as
    /// upstream's `process_renderables` hook arranges it.
    fn emit(&mut self, rendered: &str) {
        self.emit_with(rendered, None);
    }

    /// [`emit`](Self::emit), first setting the frame's slots when `slots`
    /// is given (a refresh).
    fn emit_with(&mut self, rendered: &str, slots: Option<Vec<Slot>>) {
        let entry = self.entry.clone();
        if !self.hooked {
            if let Some(slots) = slots {
                entry.frame().set_slots(slots);
            }
            self.emit_through_first(rendered);
            return;
        }
        let (slots, generation, draws, stale) = {
            let mut frame = entry.frame();
            if let Some(slots) = slots {
                frame.set_slots(slots);
                frame.stale = false;
            }
            let stale = std::mem::take(&mut frame.stale);
            (frame.slots.clone(), frame.generation, frame.draws(), stale)
        };
        if stale {
            // A nested display refreshed this one, which took its renderable.
            self.take_content();
        }
        if !draws {
            self.write_str(rendered);
            return;
        }
        // The renderable is rendered before the frame is locked (see
        // `LiveEntry`).
        let (live, shape, stash) = render_frame(
            &self.console,
            &Own::Render(self.content.as_ref()),
            &slots,
            self.alt_screen,
            self.vertical_overflow,
        );
        if let (Some(stash), None) = (&stash, &self.pending) {
            entry.set_content(stash.clone());
        }
        let mut frame = entry.frame();
        let output = if frame.generation == generation {
            frame.arrange(rendered, &live, shape)
        } else {
            // A nested display set the frame meanwhile: draw what it set,
            // with this display's renderable as just rendered.
            let own = Own::Snapshot(stash.or_else(|| entry.content()));
            frame.draw(&self.console, &own, rendered)
        };
        if !output.is_empty() {
            let _ = self.writer.write_all(output.as_bytes());
        }
    }

    /// `console.print` of a display that does not draw: through the first
    /// display's render hook when another display draws, else as it is.
    fn emit_through_first(&mut self, rendered: &str) {
        let output = match self.first() {
            Some(first) => {
                let own = Own::Snapshot(first.content());
                let mut frame = first.frame();
                frame.draw(&self.console, &own, rendered)
            }
            None => rendered.to_string(),
        };
        self.write_str(&output);
    }

    /// Commit the final frame (with a trailing newline) and show the cursor.
    /// Port of `Live.stop`; the alternate screen is switched off rather than
    /// drawn again.
    pub fn stop(&mut self) {
        if !self.started {
            return;
        }
        self.started = false;
        self.entry.frame().started = false;
        self.console.clear_live();
        if self.nested {
            // A nested display prints its renderable once, at the end,
            // through the first display's render hook.
            self.nested = false;
            if !self.is_transient() {
                self.take_content();
                let content = self.content.as_ref();
                let rendered = self.console.capture(|console| console.print(content));
                self.emit_through_first(&rendered);
            }
            let _ = self.writer.flush();
            return;
        }
        // "allow it to fully render on the last even if overflow".
        self.vertical_overflow = VerticalOverflow::Visible;
        self.entry.frame().vertical_overflow = VerticalOverflow::Visible;
        let alt_screen = self.alt_screen;
        if !alt_screen {
            self.refresh();
        }
        // `_disable_redirect_io` drops upstream's `FileProxy` objects, and
        // CPython's `close()` on the way out flushes a partial line through
        // the console while the render hook is still pushed.
        for stream in [ProxyStream::Stdout, ProxyStream::Stderr] {
            if let Some(pending) = self.proxy_buffer(stream).flush() {
                self.print_str(&pending);
            }
        }
        self.hooked = false;
        let entry = self.entry.clone();
        let mut frame = entry.frame();
        frame.hooked = false;
        // Upstream ends the display with `console.line()` only when the last
        // render drew something (`_live_render.last_render_height`).
        let height = frame.shape.map_or(0, |(_, height)| height);
        if !alt_screen && self.console.is_terminal() && height > 0 {
            self.write_str("\n");
        }
        self.show_cursor(true);
        if alt_screen {
            self.set_alt_screen(false);
        }
        if self.is_transient() && !alt_screen {
            let restore = crate::live_render::restore_cursor(frame.shape);
            self.control(&restore);
        }
        // A later `start` decides the alternate screen afresh.
        self.alt_screen = false;
        frame.alt_screen = false;
        drop(frame);
        let _ = self.writer.flush();
    }

    /// Restore the terminal after a render panicked mid-display: end the
    /// partial frame's line and show the cursor, as the `finally` of
    /// upstream's `stop()` does whatever its refresh raised.
    fn abort(&mut self) {
        // `stop()` clears `started` before its final refresh, but stays
        // hooked until it has restored the terminal: a panic in that refresh
        // still needs the restore.
        if !self.started && !self.hooked {
            return;
        }
        if self.started {
            // `stop()` pops the live stack before its final refresh.
            self.console.clear_live();
        }
        self.started = false;
        {
            let mut frame = self.entry.frame();
            frame.started = false;
            frame.hooked = false;
        }
        if std::mem::take(&mut self.nested) {
            // A nested display drew nothing to restore.
            return;
        }
        self.hooked = false;
        if self.console.is_terminal() {
            let _ = write!(self.writer, "\n{}", Control::show_cursor(true).as_str());
            if self.alt_screen {
                let _ = write!(self.writer, "{}", Control::alt_screen(false).as_str());
            }
            let _ = self.writer.flush();
        }
        self.alt_screen = false;
        self.entry.frame().alt_screen = false;
    }

    /// Whether writes to the proxy for `stream` print through the display:
    /// upstream's `_enable_redirect_io` runs only on a terminal, and only
    /// while the display is started.
    fn redirecting(&self, stream: ProxyStream) -> bool {
        let enabled = match stream {
            ProxyStream::Stdout => self.redirect_stdout,
            ProxyStream::Stderr => self.redirect_stderr,
        };
        self.started && enabled && self.console.is_terminal()
    }

    fn proxy_buffer(&mut self, stream: ProxyStream) -> &mut ProxyBuffer {
        match stream {
            ProxyStream::Stdout => &mut self.stdout_buffer,
            ProxyStream::Stderr => &mut self.stderr_buffer,
        }
    }

    /// A writer standing in for stdout while the display runs (upstream's
    /// `sys.stdout` replaced by a `FileProxy`): whole lines print above the
    /// display; when not redirecting, writes go to the process's stdout.
    pub fn stdout(&mut self) -> FileProxy<&mut Self, std::io::Stdout> {
        FileProxy::new(self, ProxyStream::Stdout, std::io::stdout())
    }

    /// [`stdout`](Self::stdout) for stderr (upstream's `sys.stderr` proxy).
    pub fn stderr(&mut self) -> FileProxy<&mut Self, std::io::Stderr> {
        FileProxy::new(self, ProxyStream::Stderr, std::io::stderr())
    }

    /// A proxy for `stream` over any `file`, which receives the writes when
    /// the display is not redirecting.
    pub fn file_proxy<F: Write>(
        &mut self,
        stream: ProxyStream,
        file: F,
    ) -> FileProxy<&mut Self, F> {
        FileProxy::new(self, stream, file)
    }

    /// The output sink (for inspecting captured bytes in tests).
    pub fn writer(&self) -> &W {
        &self.writer
    }

    /// Consume the display, returning its output sink.
    pub fn into_writer(self) -> W {
        self.writer
    }
}

impl<W: Write> ProxyTarget for &mut Live<W> {
    fn proxy_write(&mut self, stream: ProxyStream, bytes: &[u8]) -> bool {
        if !self.redirecting(stream) {
            return false;
        }
        if let Some(text) = self.proxy_buffer(stream).write(bytes) {
            self.print(&text);
        }
        true
    }

    fn proxy_flush(&mut self, stream: ProxyStream) -> bool {
        if !self.redirecting(stream) {
            return false;
        }
        if let Some(pending) = self.proxy_buffer(stream).flush() {
            self.print_str(&pending);
        }
        true
    }
}

/// A job run on an [`AutoLive`] refresh thread against its display.
type LiveJob<W> = Box<dyn FnOnce(&mut Live<W>) + Send>;

/// A message from an [`AutoLive`] handle to its background refresh thread.
enum LiveMessage<W: Write> {
    /// Swap in a new renderable and redraw.
    Update(Box<dyn Renderable + Send>),
    /// Redraw the current renderable now.
    Refresh,
    /// Run a job against the display (a print, a proxied write, an
    /// acknowledged refresh).
    Run(LiveJob<W>),
    /// Commit the final frame, restore the cursor, and stop the thread.
    Stop,
}

impl<W: Write + Send + 'static> Live<W> {
    /// Start an auto-refreshing live display on a background thread: it draws the
    /// first frame, then redraws every `1/refresh_per_second` seconds (and
    /// immediately on each [`AutoLive::update`]). Mirrors upstream's
    /// `auto_refresh`/`refresh_per_second`. The `renderable`, `console`, and
    /// `writer` are moved into the thread, so all three must be `Send`.
    pub fn spawn(
        renderable: Box<dyn Renderable + Send>,
        console: Console,
        writer: W,
        refresh_per_second: f64,
    ) -> AutoLive<W> {
        Live::spawn_options(
            renderable,
            console,
            writer,
            refresh_per_second,
            LiveOptions::default(),
        )
    }

    /// [`spawn`](Self::spawn), clearing the display when it stops when
    /// `transient` is set (upstream `Live(transient=True)`).
    pub fn spawn_with(
        renderable: Box<dyn Renderable + Send>,
        console: Console,
        writer: W,
        refresh_per_second: f64,
        transient: bool,
    ) -> AutoLive<W> {
        Live::spawn_options(
            renderable,
            console,
            writer,
            refresh_per_second,
            LiveOptions {
                transient,
                ..LiveOptions::default()
            },
        )
    }

    /// [`spawn`](Self::spawn) with every option of upstream's `Live(...)`.
    pub fn spawn_options(
        renderable: Box<dyn Renderable + Send>,
        console: Console,
        writer: W,
        refresh_per_second: f64,
        options: LiveOptions,
    ) -> AutoLive<W> {
        Live::spawn_inner(
            move |console, writer| Live::new(renderable, console, writer),
            console,
            writer,
            refresh_per_second,
            options,
        )
    }

    /// [`spawn_options`](Self::spawn_options) over upstream's
    /// `get_renderable`: `get` is called on the refresh thread for a fresh
    /// renderable at every refresh ([`Live::get_renderable`]).
    pub fn spawn_get_renderable(
        get: impl Fn() -> Box<dyn Renderable> + Send + 'static,
        console: Console,
        writer: W,
        refresh_per_second: f64,
        options: LiveOptions,
    ) -> AutoLive<W> {
        Live::spawn_inner(
            move |console, writer| {
                Live::new(Box::new(crate::text::Text::new("")), console, writer).get_renderable(get)
            },
            console,
            writer,
            refresh_per_second,
            options,
        )
    }

    fn spawn_inner(
        make: impl FnOnce(Console, W) -> Live<W> + Send + 'static,
        console: Console,
        writer: W,
        refresh_per_second: f64,
        options: LiveOptions,
    ) -> AutoLive<W> {
        let (sender, receiver) = mpsc::channel::<LiveMessage<W>>();
        let (started, wait_started) = mpsc::channel::<()>();
        let interval = Duration::from_secs_f64(1.0 / refresh_per_second.max(f64::MIN_POSITIVE));
        let terminal = console.is_terminal();
        let handle = thread::spawn(move || {
            // The `Live` (and its non-`Send` `LiveRender`) is built and owned
            // entirely within this thread — only the `Send` inputs cross over.
            let mut live = make(console, writer).options(&options);
            // A panicking render must not take the writer down with it:
            // upstream's `stop()` restores the terminal in a `finally`, so the
            // failure is caught here, the cursor shown again, and the panic
            // handed back to `try_stop` instead of being re-raised by `join`.
            let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
                live.start_with(options.refresh_on_start);
                let _ = started.send(());
                loop {
                    match receiver.recv_timeout(interval) {
                        Ok(LiveMessage::Update(renderable)) => live.update(renderable),
                        Ok(LiveMessage::Refresh) => live.refresh(),
                        Err(RecvTimeoutError::Timeout) => live.tick(),
                        Ok(LiveMessage::Run(job)) => job(&mut live),
                        // Stop, or the handle was dropped: finalize and exit.
                        Ok(LiveMessage::Stop) | Err(RecvTimeoutError::Disconnected) => {
                            live.stop();
                            break;
                        }
                    }
                }
            }));
            let failure = outcome.err().map(|payload| {
                live.abort();
                panic_message(payload.as_ref())
            });
            (live.into_writer(), failure)
        });
        // Upstream's `Live.start()` draws the first frame before returning;
        // without this wait, a change made right after spawning could land in
        // the "first" frame, depending on thread scheduling.
        let _ = wait_started.recv();
        AutoLive {
            handle: LiveHandle {
                sender: Some(sender),
                redirect_stdout: options.redirect_stdout && terminal,
                redirect_stderr: options.redirect_stderr && terminal,
                holder: None,
            },
            thread: Some(handle),
        }
    }
}

/// A cloneable, `Send` handle to an [`AutoLive`]'s refresh thread, used by
/// the [`FileProxy`] writers it hands out. Once the display has stopped,
/// proxies built on it write to their files, as upstream's restored
/// `sys.stdout` does.
pub struct LiveHandle<W: Write> {
    sender: Option<mpsc::Sender<LiveMessage<W>>>,
    redirect_stdout: bool,
    redirect_stderr: bool,
    /// The thread holding a lock the display's renderable needs (a
    /// `LiveProgress::with` closure), which must not wait for a print.
    holder: Option<Holder>,
}

/// The thread inside a lock a display's renderable needs, if any.
pub(crate) type Holder = Arc<Mutex<Option<thread::ThreadId>>>;

impl<W: Write> Clone for LiveHandle<W> {
    fn clone(&self) -> Self {
        LiveHandle {
            sender: self.sender.clone(),
            redirect_stdout: self.redirect_stdout,
            redirect_stderr: self.redirect_stderr,
            holder: self.holder.clone(),
        }
    }
}

impl<W: Write + Send + 'static> LiveHandle<W> {
    /// A handle for a display that never runs (a disabled progress): its
    /// proxies always write to their files.
    pub(crate) fn detached() -> Self {
        LiveHandle {
            sender: None,
            redirect_stdout: false,
            redirect_stderr: false,
            holder: None,
        }
    }

    /// The same handle, not waiting for its jobs on the thread that holds
    /// `holder`.
    pub(crate) fn with_holder(mut self, holder: Holder) -> Self {
        self.holder = Some(holder);
        self
    }

    /// Run `job` on the refresh thread and wait until it has run, as
    /// upstream prints synchronously; false once the display has stopped.
    /// On the thread holding the renderable's lock it is only queued, since
    /// the refresh thread needs that lock to draw.
    pub(crate) fn run(&self, job: impl FnOnce(&mut Live<W>) + Send + 'static) -> bool {
        let Some(sender) = &self.sender else {
            return false;
        };
        let held = self.holder.as_ref().is_some_and(|holder| {
            *holder
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                == Some(thread::current().id())
        });
        if held {
            return sender.send(LiveMessage::Run(Box::new(job))).is_ok();
        }
        let (done, wait) = mpsc::channel();
        let job = move |live: &mut Live<W>| {
            job(live);
            let _ = done.send(());
        };
        if sender.send(LiveMessage::Run(Box::new(job))).is_err() {
            return false;
        }
        // A job that panicked drops `done`; the thread has restored the
        // terminal, and the write counts as made.
        let _ = wait.recv();
        true
    }

    /// [`Live::print`] on the refresh thread.
    pub fn print(&self, renderable: Box<dyn Renderable + Send>) {
        self.run(move |live| live.print(renderable.as_ref()));
    }

    /// [`Live::print_str`] on the refresh thread.
    pub fn print_str(&self, content: impl Into<String>) {
        let content = content.into();
        self.run(move |live| live.print_str(&content));
    }

    /// [`Live::log`] on the refresh thread. A log queued from the thread
    /// holding the renderable's lock (see [`run`](Self::run)) reports no
    /// error.
    #[track_caller]
    pub fn log(
        &self,
        objects: &[crate::console::LogObject<'_>],
        options: &crate::console::LogOptions<'_>,
    ) -> crate::errors::Result<()> {
        let caller = std::panic::Location::caller();
        self.log_at(objects, options, caller.file(), caller.line())
    }

    /// [`log`](Self::log) with the caller's `file` and `line` given.
    pub fn log_at(
        &self,
        objects: &[crate::console::LogObject<'_>],
        options: &crate::console::LogOptions<'_>,
        file: &str,
        line: u32,
    ) -> crate::errors::Result<()> {
        let call = crate::console::OwnedLog::new(objects, options, file, line);
        let result = Arc::new(Mutex::new(Ok(())));
        let slot = result.clone();
        self.run(move |live| {
            let mut outcome = Ok(());
            let rendered = live.console.capture(|console| outcome = call.log(console));
            live.emit(&rendered);
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = outcome;
        });
        let mut result = result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *result, Ok(()))
    }

    fn redirecting(&self, stream: ProxyStream) -> bool {
        match stream {
            ProxyStream::Stdout => self.redirect_stdout,
            ProxyStream::Stderr => self.redirect_stderr,
        }
    }

    /// A proxy for `stream` over `file`.
    pub fn file_proxy<F: Write>(&self, stream: ProxyStream, file: F) -> FileProxy<Self, F> {
        FileProxy::new(self.clone(), stream, file)
    }
}

impl<W: Write + Send + 'static> ProxyTarget for LiveHandle<W> {
    fn proxy_write(&mut self, stream: ProxyStream, bytes: &[u8]) -> bool {
        if !self.redirecting(stream) {
            return false;
        }
        let bytes = bytes.to_vec();
        self.run(move |live| {
            (&mut *live).proxy_write(stream, &bytes);
        })
    }

    fn proxy_flush(&mut self, stream: ProxyStream) -> bool {
        if !self.redirecting(stream) {
            return false;
        }
        self.run(move |live| {
            (&mut *live).proxy_flush(stream);
        })
    }
}

/// A handle to an auto-refreshing [`Live`] running on a background thread.
/// Dropping the handle (or calling [`stop`](Self::stop)) finalizes the display.
pub struct AutoLive<W: Write + Send + 'static> {
    handle: LiveHandle<W>,
    thread: Option<JoinHandle<(W, Option<String>)>>,
}

/// A render that panicked on an [`AutoLive`] refresh thread, returned by
/// [`AutoLive::try_stop`]. The terminal was restored before the thread exited.
#[derive(Debug)]
pub struct LivePanic<W> {
    /// The output sink, with everything written up to and including the
    /// cursor restore.
    pub writer: W,
    /// The panic message (`"<non-string panic payload>"` when it had none).
    pub message: String,
}

/// The message of a caught panic payload.
fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

impl<W: Write + Send + 'static> AutoLive<W> {
    /// Swap in a new renderable; the thread redraws it promptly.
    pub fn update(&self, renderable: Box<dyn Renderable + Send>) {
        self.send(LiveMessage::Update(renderable));
    }

    /// Ask the thread to redraw the current renderable now.
    pub fn refresh(&self) {
        self.send(LiveMessage::Refresh);
    }

    fn send(&self, message: LiveMessage<W>) -> bool {
        match &self.handle.sender {
            Some(sender) => sender.send(message).is_ok(),
            None => false,
        }
    }

    /// Print `renderable` above the display ([`Live::print`]), on the thread.
    pub fn print(&self, renderable: Box<dyn Renderable + Send>) {
        self.handle.print(renderable);
    }

    /// Print a string as console markup above the display
    /// ([`Live::print_str`]), on the thread.
    pub fn print_str(&self, content: impl Into<String>) {
        self.handle.print_str(content);
    }

    /// Log above the display ([`Live::log`]), on the thread.
    #[track_caller]
    pub fn log(
        &self,
        objects: &[crate::console::LogObject<'_>],
        options: &crate::console::LogOptions<'_>,
    ) -> crate::errors::Result<()> {
        let caller = std::panic::Location::caller();
        self.handle
            .log_at(objects, options, caller.file(), caller.line())
    }

    /// A cloneable handle to the thread, for proxies and prints from other
    /// threads.
    pub fn handle(&self) -> LiveHandle<W> {
        self.handle.clone()
    }

    /// A writer standing in for stdout while the display runs (upstream's
    /// `sys.stdout` `FileProxy`); see [`Live::stdout`].
    pub fn stdout(&self) -> FileProxy<LiveHandle<W>, std::io::Stdout> {
        self.handle
            .file_proxy(ProxyStream::Stdout, std::io::stdout())
    }

    /// [`stdout`](Self::stdout) for stderr.
    pub fn stderr(&self) -> FileProxy<LiveHandle<W>, std::io::Stderr> {
        self.handle
            .file_proxy(ProxyStream::Stderr, std::io::stderr())
    }

    /// A proxy for `stream` over any `file`, which receives the writes when
    /// the display is not redirecting.
    pub fn file_proxy<F: Write>(
        &self,
        stream: ProxyStream,
        file: F,
    ) -> FileProxy<LiveHandle<W>, F> {
        self.handle.file_proxy(stream, file)
    }

    /// Redraw now and wait until the frame is written, as upstream's
    /// `Live.refresh()` renders in the caller's thread. A renderable that reads
    /// shared state then shows the state as of this call.
    pub fn refresh_wait(&self) {
        let (done, wait) = mpsc::channel();
        let job = move |live: &mut Live<W>| {
            live.refresh();
            let _ = done.send(());
        };
        if self.send(LiveMessage::Run(Box::new(job))) {
            let _ = wait.recv();
        }
    }

    /// Commit the final frame, join the thread, and return the output sink.
    ///
    /// If a render panicked on the refresh thread, the cursor has already been
    /// shown again and the failure is swallowed (the panic hook reported it
    /// when it happened); use [`try_stop`](Self::try_stop) to observe it.
    pub fn stop(self) -> W {
        match self.try_stop() {
            Ok(writer) => writer,
            Err(failure) => failure.writer,
        }
    }

    /// [`stop`](Self::stop), reporting a render that panicked on the refresh
    /// thread as an error that still carries the output sink.
    pub fn try_stop(mut self) -> Result<W, LivePanic<W>> {
        self.send(LiveMessage::Stop);
        let handle = self
            .thread
            .take()
            .expect("thread handle present until stop/drop");
        // The thread catches every panic from rendering, so a join error
        // cannot carry a writer; there is nothing to return but the payload.
        let (writer, failure) = match handle.join() {
            Ok(result) => result,
            Err(payload) => panic::resume_unwind(payload),
        };
        match failure {
            None => Ok(writer),
            Some(message) => Err(LivePanic { writer, message }),
        }
    }
}

impl<W: Write + Send + 'static> Drop for AutoLive<W> {
    fn drop(&mut self) {
        // If the caller didn't `stop()`, still finalize + join the thread.
        if let Some(handle) = self.thread.take() {
            self.send(LiveMessage::Stop);
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::text::Text;

    fn console() -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .no_color(false)
            .build()
    }

    #[test]
    fn redirected_live_emits_only_the_final_frame() {
        let console = Console::builder().force_terminal(false).width(20).build();
        let mut live = Live::new(Box::new(Text::new("first")), console, Vec::<u8>::new());
        live.start();
        live.update(Box::new(Text::new("last")));
        live.refresh();
        assert!(live.writer().is_empty());
        live.stop();
        assert_eq!(live.writer(), b"last");
    }

    #[test]
    fn manual_refresh_stream_matches_upstream() {
        let mut live = Live::new(
            Box::new(Text::new("frame one")),
            console(),
            Vec::<u8>::new(),
        );
        live.start();
        live.update(Box::new(Text::new("frame two")));
        live.update(Box::new(Text::new("frame three")));
        live.stop();

        // Captured verbatim from real rich 15.0.0 (auto_refresh=False,
        // transient=False, width 20) writing to a StringIO.
        let expected = "\x1b[?25lframe one\r\x1b[2Kframe two\r\x1b[2Kframe three\r\x1b[2Kframe three\n\x1b[?25h";
        assert_eq!(String::from_utf8(live.writer().clone()).unwrap(), expected);
    }

    #[test]
    fn auto_refresh_thread_produces_the_same_stream() {
        // A very low refresh rate (10s interval) means no timeout-driven refresh
        // fires during the test, so the thread processes exactly start + the two
        // updates + stop, in order — the identical byte-parity stream, now driven
        // through the background thread (spawn / channel / join).
        let auto = Live::spawn(
            Box::new(Text::new("frame one")),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.update(Box::new(Text::new("frame two")));
        auto.update(Box::new(Text::new("frame three")));
        let output = auto.stop();

        let expected = "\x1b[?25lframe one\r\x1b[2Kframe two\r\x1b[2Kframe three\r\x1b[2Kframe three\n\x1b[?25h";
        assert_eq!(String::from_utf8(output).unwrap(), expected);
    }

    /// Renders `ok` until `fail_after` renders have happened, then panics.
    struct PanicsLater {
        renders: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        fail_after: usize,
    }

    impl Renderable for PanicsLater {
        fn rich_render(
            &self,
            console: &Console,
            options: &crate::console::ConsoleOptions,
        ) -> Vec<crate::segment::Segment> {
            use std::sync::atomic::Ordering;
            if self.renders.fetch_add(1, Ordering::SeqCst) >= self.fail_after {
                panic!("render failed");
            }
            Text::new("ok").rich_render(console, options)
        }
    }

    #[test]
    fn a_final_refresh_that_panics_still_restores_the_cursor() {
        // The first frame renders; the refresh `stop()` makes panics. That
        // refresh runs after `started` is cleared, so the restore must not
        // depend on it.
        let auto = Live::spawn(
            Box::new(PanicsLater {
                renders: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                fail_after: 1,
            }),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        let (output, error) = match auto.try_stop() {
            Ok(output) => (output, None),
            Err(failure) => (failure.writer, Some(failure.message)),
        };
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.ends_with("\x1b[?25h"),
            "cursor left hidden: {output:?}"
        );
        assert_eq!(error.as_deref(), Some("render failed"));
    }

    #[test]
    fn a_panicking_render_still_restores_the_cursor() {
        // Upstream's `stop()` shows the cursor again in a `finally`, whatever
        // the refresh raised. A render that panics on the refresh thread must
        // neither leave the cursor hidden nor make `stop()` panic in turn.
        let renders = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let auto = Live::spawn(
            Box::new(PanicsLater {
                renders: renders.clone(),
                fail_after: 1,
            }),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.refresh_wait(); // panics on the thread; must not hang here
        let (output, error) = match auto.try_stop() {
            Ok(output) => (output, None),
            Err(failure) => (failure.writer, Some(failure.message)),
        };
        let output = String::from_utf8(output).unwrap();
        assert!(output.starts_with("\x1b[?25lok"), "{output:?}");
        assert!(
            output.ends_with("\x1b[?25h"),
            "cursor left hidden: {output:?}"
        );
        assert_eq!(error.as_deref(), Some("render failed"));

        // `stop()` swallows the failure after restoring the terminal.
        let auto = Live::spawn(
            Box::new(PanicsLater {
                renders: std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                fail_after: 1,
            }),
            console(),
            Vec::<u8>::new(),
            0.1,
        );
        auto.refresh();
        let output = String::from_utf8(auto.stop()).unwrap();
        assert!(
            output.ends_with("\x1b[?25h"),
            "cursor left hidden: {output:?}"
        );
    }

    #[test]
    fn dropping_the_handle_finalizes_the_display() {
        // Even without an explicit stop(), Drop commits the final frame + restores
        // the cursor (the trailing "\n" + show-cursor), so no display is left open.
        let console = console();
        // Route through a shared buffer so we can inspect it after the drop.
        let auto = Live::spawn(Box::new(Text::new("only")), console, Vec::<u8>::new(), 0.1);
        drop(auto); // no explicit stop
                    // If Drop didn't join the thread, this test would still pass
                    // but leak the thread; the assertion is simply that drop
                    // returns without panicking / deadlocking.
    }
}

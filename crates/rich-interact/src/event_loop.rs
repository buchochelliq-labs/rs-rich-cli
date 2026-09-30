//! The event loop, and the blocking [`run`] built on it.
//!
//! An [`EventLoop`] runs one or more mounted components at once: it renders
//! them stacked, paints only what changed, waits for the next event, tick or
//! timer, and routes keys to the first component still running and mouse
//! events to the running component under the pointer. It is kept minimal on
//! purpose (events, timers, repaint on change) so the intuiTUIve track can
//! grow a component tree, reactive state and focus routing on top of it
//! rather than beside it. [`run`] is the loop with one component, so a
//! component behaves the same under both.

use std::cell::RefCell;
use std::fmt;
use std::io;
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use rich::{ColorSystem, Console, Segment};

use crate::component::{Component, Context, Flow, View};
use crate::event::{Button, Event, Key, Mouse, MouseKind};
use crate::paint::Painter;
use crate::policy::{Fallback, LineIo, NotInteractive, Policy, StdLineIo};
use crate::session::{Backend, Session, SessionOptions};

/// How a component ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome<T> {
    Done(T),
    /// The component cancelled (Escape, `q`).
    Cancelled,
    /// Ctrl+C.
    Interrupted,
}

impl<T> Outcome<T> {
    /// The value, if the component finished with one.
    pub fn value(self) -> Option<T> {
        match self {
            Outcome::Done(value) => Some(value),
            _ => None,
        }
    }
}

/// A component's outcome and whether it finished. Kept apart so taking the
/// outcome does not make a finished component look like it is running.
struct Shared<T> {
    outcome: Option<Outcome<T>>,
    finished: bool,
}

impl<T> Shared<T> {
    fn finish(&mut self, outcome: Outcome<T>) {
        if !self.finished {
            self.outcome = Some(outcome);
            self.finished = true;
        }
    }
}

/// A component's result, readable once the loop has run.
pub struct Handle<T>(Rc<RefCell<Shared<T>>>);

impl<T> Handle<T> {
    /// The outcome, once; `None` while the component is running or after
    /// it was taken.
    pub fn take(&self) -> Option<Outcome<T>> {
        self.0.borrow_mut().outcome.take()
    }

    /// Whether the component finished, even after its outcome was taken.
    pub fn is_finished(&self) -> bool {
        self.0.borrow().finished
    }
}

/// How an [`EventLoop`] paints.
#[derive(Clone, Debug)]
pub struct LoopOptions {
    /// Rows the inline region may take (default: the terminal's height).
    pub height: Option<usize>,
    /// Clear the region when the loop ends, instead of leaving the last
    /// view on screen.
    pub transient: bool,
    /// The colours to paint with. `None`: the headless driver paints
    /// truecolor, a terminal what rich detects.
    pub color_system: Option<Option<ColorSystem>>,
    pub no_color: bool,
}

impl Default for LoopOptions {
    fn default() -> Self {
        LoopOptions {
            height: None,
            transient: false,
            color_system: None,
            no_color: std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
        }
    }
}

/// What a mounted component did with an event.
enum Step {
    Continue,
    Finished,
    Handoff(Command),
}

/// A component with its result slot, behind one object-safe face.
trait Mounted {
    /// `None` once started.
    fn start(&mut self, context: &Context<'_>) -> Option<Step>;
    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Step;
    fn render(&self, context: &Context<'_>) -> View;
    fn tick(&self) -> Option<Duration>;
    fn finished(&self) -> bool;
    fn interrupt(&mut self);
}

struct Slot<C: Component> {
    component: C,
    result: Rc<RefCell<Shared<C::Output>>>,
    started: bool,
}

impl<C: Component> Slot<C> {
    fn step(&mut self, flow: Flow<C::Output>) -> Step {
        let outcome = match flow {
            Flow::Continue | Flow::Ignored => return Step::Continue,
            Flow::Handoff(command) => return Step::Handoff(command),
            Flow::Done(value) => Outcome::Done(value),
            Flow::Cancel => Outcome::Cancelled,
        };
        self.result.borrow_mut().finish(outcome);
        Step::Finished
    }
}

impl<C: Component> Mounted for Slot<C> {
    fn start(&mut self, context: &Context<'_>) -> Option<Step> {
        if std::mem::replace(&mut self.started, true) {
            return None;
        }
        let flow = self.component.start(context);
        Some(self.step(flow))
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Step {
        let flow = self.component.handle(event, context);
        // A focus key nothing inside used ran off the end of the focus
        // order: at the top, focus wraps round to the other end.
        if let (Flow::Ignored, Some(key)) = (&flow, event.key()) {
            let keymap = self.component.keymap();
            let forward = keymap.is(key, crate::compose::FOCUS_NEXT);
            if forward || keymap.is(key, crate::compose::FOCUS_PREVIOUS) {
                self.component.focus_enter(forward);
            }
        }
        self.step(flow)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.component.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        self.component.tick()
    }

    fn finished(&self) -> bool {
        self.result.borrow().finished
    }

    fn interrupt(&mut self) {
        self.result.borrow_mut().finish(Outcome::Interrupted);
    }
}

struct Timer<'a> {
    interval: Duration,
    next: Duration,
    callback: Box<dyn FnMut() -> bool + 'a>,
}

/// Runs mounted components until every one has finished. See the
/// [module docs](self).
pub struct EventLoop<'a> {
    backend: Box<dyn Backend + 'a>,
    options: LoopOptions,
    console: Console,
    painter: Painter,
    mounted: Vec<(Box<dyn Mounted + 'a>, Option<Duration>)>,
    timers: Vec<Timer<'a>>,
    size: (u16, u16),
    /// The first row of each mounted component's view in the last paint,
    /// and the painted lines, so a mouse event can be put in a component's
    /// own coordinates and a click matched to a link.
    rows: Vec<usize>,
    shown: Vec<Vec<Segment>>,
    /// The component a button press went to: its moves and release follow
    /// it there (a drag), wherever the pointer goes.
    grab: Option<usize>,
}

impl<'a> EventLoop<'a> {
    pub fn new(backend: impl Backend + 'a, options: LoopOptions) -> EventLoop<'a> {
        let size = backend.size();
        // Pinned unless the caller says otherwise, so headless output is
        // byte-exact anywhere; `terminal` passes what rich detects.
        let system = options.color_system.unwrap_or(Some(ColorSystem::Truecolor));
        let console = Self::console(size, system, options.no_color);
        EventLoop {
            painter: Painter::new(system, options.no_color),
            backend: Box::new(backend),
            options,
            console,
            mounted: Vec::new(),
            timers: Vec::new(),
            size,
            rows: Vec::new(),
            shown: Vec::new(),
            grab: None,
        }
    }

    /// A loop on the real terminal, starting a [`Session`].
    pub fn terminal(session: SessionOptions, options: LoopOptions) -> io::Result<EventLoop<'a>> {
        let session = Session::start(session)?;
        let system = options.color_system.unwrap_or_else(|| {
            Console::builder()
                .force_terminal(true)
                .build()
                .color_system()
        });
        Ok(EventLoop::new(
            session,
            LoopOptions {
                color_system: Some(system),
                ..options
            },
        ))
    }

    fn console(size: (u16, u16), system: Option<ColorSystem>, no_color: bool) -> Console {
        Console::builder()
            .width(size.0.max(1) as usize)
            .height(size.1.max(1) as usize)
            .force_terminal(true)
            .color_system(system)
            .no_color(no_color)
            .build()
    }

    /// Add a component. Its outcome is in the handle once it finishes.
    pub fn mount<C: Component + 'a>(&mut self, component: C) -> Handle<C::Output> {
        let result = Rc::new(RefCell::new(Shared {
            outcome: None,
            finished: false,
        }));
        let tick = component.tick();
        let next = tick.map(|interval| self.backend.elapsed() + interval);
        self.mounted.push((
            Box::new(Slot {
                component,
                result: Rc::clone(&result),
                started: false,
            }),
            next,
        ));
        Handle(result)
    }

    /// Draw the graphics `source` places on the views' cells (micro assets
    /// through Kitty, iTerm2 or Sixel). The loop wakes when an animation
    /// among them shows its next frame. Give it only on a terminal that
    /// showed it can draw them.
    pub fn graphics(&mut self, source: std::sync::Arc<dyn rich_ext::frame::PlacementSource>) {
        self.painter.set_graphics(Some(source));
    }

    /// Call `callback` every `interval` while the loop runs, until it
    /// returns false.
    pub fn every(&mut self, interval: Duration, callback: impl FnMut() -> bool + 'a) {
        self.timers.push(Timer {
            interval,
            next: self.backend.elapsed() + interval,
            callback: Box::new(callback),
        });
    }

    /// (width, height) components render for.
    fn space(&self) -> (usize, usize) {
        let rows = self.size.1.max(1) as usize;
        (
            self.size.0.max(1) as usize,
            self.options.height.map_or(rows, |height| height.min(rows)),
        )
    }

    fn context(&self) -> Context<'_> {
        let (width, height) = self.space();
        Context {
            console: &self.console,
            width,
            height,
        }
    }

    fn paint(&mut self) -> io::Result<()> {
        let context = self.context();
        let max_rows = context.height;
        let mut view = View::default();
        let mut rows = Vec::with_capacity(self.mounted.len());
        for (component, _) in &self.mounted {
            rows.push(view.lines.len());
            view.push(component.render(&context));
        }
        // The cursor was placed on the rendered cells; shown controls take
        // one each, so it moves with them.
        if let Some((row, column)) = view.cursor {
            if let Some(line) = view.lines.get(row) {
                let shown = crate::paint::sanitized_column(line, column);
                let last = context.width.saturating_sub(1).max(column);
                view.cursor = Some((row, shown.min(last)));
            }
        }
        // A line wider than the terminal would wrap and shift every row
        // below it: crop, once controls are shown as the characters the
        // painter will write.
        for line in &mut view.lines {
            crate::paint::sanitize_line(line);
            if line.iter().map(Segment::cell_length).sum::<usize>() > context.width {
                *line = Segment::adjust_line_length(line, context.width, None);
            }
        }
        let out = self.painter.paint(&view, max_rows);
        view.lines.truncate(max_rows);
        if !out.is_empty() {
            let plain = view
                .lines
                .iter()
                .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n");
            self.backend.painted(&plain);
            self.backend.write(&out)?;
        }
        self.rows = rows;
        self.shown = view.lines;
        Ok(())
    }

    /// `mouse`, from terminal cells, in the coordinates of mounted
    /// component `index`'s view: `None` for a press outside it. A press of
    /// the left button on a hyperlink is [`Event::Link`] instead.
    fn locate(&self, index: usize, mouse: Mouse) -> Option<Event> {
        let region_row = self.region_row(mouse);
        let (start, end) = self.extent(index);
        let inside = region_row.filter(|row| (start..end).contains(row));
        if let (MouseKind::Down(_), None) = (mouse.kind, inside) {
            return None;
        }
        if let (MouseKind::Down(Button::Left), Some(row)) = (mouse.kind, inside) {
            if let Some(url) = link_at(&self.shown[row], mouse.column as usize) {
                return Some(Event::Link(url));
            }
        }
        let row = region_row.map_or(0, |row| row.saturating_sub(start));
        Some(Event::Mouse(Mouse {
            row: u16::try_from(row).unwrap_or(u16::MAX),
            ..mouse
        }))
    }

    /// `mouse`'s row in the painted region, `None` above it.
    fn region_row(&self, mouse: Mouse) -> Option<usize> {
        let top = if self.backend.alternate_screen() {
            0
        } else {
            // An inline region that grew past the bottom of the screen
            // scrolled up with it.
            let rows = self.space().1;
            (self.backend.origin() as usize).min(rows.saturating_sub(self.painter.extent()))
        };
        (mouse.row as usize).checked_sub(top)
    }

    /// The rows mounted component `index`'s view took in the last paint.
    fn extent(&self, index: usize) -> (usize, usize) {
        let start = self.rows.get(index).copied().unwrap_or(0);
        let end = self
            .rows
            .get(index + 1)
            .copied()
            .unwrap_or(self.shown.len());
        (start, end)
    }

    /// The running component a mouse event goes to. A press goes to the
    /// one under the pointer (none, outside every view) and holds it until
    /// the release, so a drag stays where it started; anything else goes
    /// to the one under the pointer, or the first running.
    fn mouse_target(&mut self, mouse: Mouse) -> Option<usize> {
        let running = |index: &usize| !self.mounted[*index].0.finished();
        let grabbed = self.grab.filter(running);
        let under = self.region_row(mouse).and_then(|row| {
            (0..self.mounted.len()).filter(running).find(|&index| {
                let (start, end) = self.extent(index);
                (start..end).contains(&row)
            })
        });
        let first = (0..self.mounted.len()).find(running);
        match mouse.kind {
            MouseKind::Down(_) => {
                self.grab = under;
                under
            }
            MouseKind::Drag(_) => grabbed.or(under).or(first),
            MouseKind::Up(_) => {
                self.grab = None;
                grabbed.or(under).or(first)
            }
            _ => under.or(first),
        }
    }

    fn running(&self) -> bool {
        self.mounted
            .iter()
            .any(|(component, _)| !component.finished())
    }

    /// The next moment a tick or timer is due.
    fn deadline(&self) -> Option<Duration> {
        let ticks = self
            .mounted
            .iter()
            .filter(|(component, _)| !component.finished())
            .filter_map(|(_, next)| *next);
        let timers = self.timers.iter().map(|timer| timer.next);
        let animation = self
            .painter
            .graphics()
            .and_then(|source| source.next_change())
            .map(|wait| self.backend.elapsed() + wait);
        ticks.chain(timers).chain(animation).min()
    }

    fn fire_due(&mut self) -> io::Result<()> {
        let now = self.backend.elapsed();
        let mut timers = std::mem::take(&mut self.timers);
        timers.retain_mut(|timer| {
            if timer.next > now {
                return true;
            }
            timer.next = now + timer.interval;
            (timer.callback)()
        });
        timers.append(&mut self.timers);
        self.timers = timers;
        for index in 0..self.mounted.len() {
            let due = self.mounted[index].1.is_some_and(|next| next <= now);
            if due && !self.mounted[index].0.finished() {
                let interval = self.mounted[index].0.tick();
                self.mounted[index].1 = interval.map(|interval| now + interval);
                self.deliver(index, &Event::Tick)?;
            }
        }
        Ok(())
    }

    /// Start every component not started yet, before it is painted.
    fn start(&mut self) -> io::Result<()> {
        for index in 0..self.mounted.len() {
            if self.mounted[index].0.finished() {
                continue;
            }
            let (width, height) = self.space();
            let context = Context {
                console: &self.console,
                width,
                height,
            };
            if let Some(step) = self.mounted[index].0.start(&context) {
                self.apply(index, step)?;
            }
        }
        Ok(())
    }

    fn deliver(&mut self, index: usize, event: &Event) -> io::Result<()> {
        let (width, height) = self.space();
        // Field by field, so the context can borrow the console while the
        // component is borrowed mutably.
        let context = Context {
            console: &self.console,
            width,
            height,
        };
        // Copies made while handling go out once the handler returns
        // (#488): the component cannot write to the terminal itself.
        crate::clipboard::open(self.backend.clipboard().err());
        let step = self.mounted[index].0.handle(event, &context);
        for text in crate::clipboard::close() {
            self.backend.copy(&text)?;
        }
        self.apply(index, step)
    }

    /// Carry out what a component asked for: for a hand-off, run the
    /// command and deliver [`Event::Returned`].
    fn apply(&mut self, index: usize, step: Step) -> io::Result<()> {
        if let Step::Handoff(mut command) = step {
            let finish = self.painter.finish(false);
            self.backend.write(&finish)?;
            let code = self.backend.handoff(&mut command)?;
            self.painter.reset();
            self.deliver(index, &Event::Returned(code))?;
        }
        Ok(())
    }

    /// The terminal is `columns` x `rows` (again): repaint every row, and
    /// tell the components.
    fn resized(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.size = (columns, rows);
        self.console = Self::console(
            self.size,
            self.console.color_system(),
            self.options.no_color,
        );
        self.painter.invalidate();
        let event = Event::Resize { columns, rows };
        for index in 0..self.mounted.len() {
            if !self.mounted[index].0.finished() {
                self.deliver(index, &event)?;
            }
        }
        Ok(())
    }

    /// Run until every mounted component has finished. Ctrl+C finishes all
    /// of them as interrupted.
    pub fn run(&mut self) -> io::Result<()> {
        let result = self.run_inner();
        let finish = self.painter.finish(self.options.transient);
        let written = self.backend.write(&finish);
        result.and(written)
    }

    fn run_inner(&mut self) -> io::Result<()> {
        loop {
            self.start()?;
            self.paint()?;
            if !self.running() {
                return Ok(());
            }
            let timeout = self
                .deadline()
                .map(|deadline| deadline.saturating_sub(self.backend.elapsed()));
            match self.backend.read(timeout)? {
                None => {}
                Some(Event::Key(key)) if key.is_interrupt() => {
                    for (component, _) in &mut self.mounted {
                        component.interrupt();
                    }
                }
                // Raw mode turns off the terminal's own Ctrl+Z, so it
                // arrives as a key: suspend as the terminal would have. The
                // region is finished first, as for a hand-off, so the
                // shell's "Stopped" goes below it.
                Some(Event::Key(key)) if key == Key::ctrl('z') && self.backend.can_suspend() => {
                    let finish = self.painter.finish(false);
                    self.backend.write(&finish)?;
                    self.backend.suspend()?;
                    self.painter.reset();
                    let (columns, rows) = self.backend.size();
                    self.resized(columns, rows)?;
                }
                Some(Event::Resize { columns, rows }) => {
                    // Back from a suspend started outside: what is on the
                    // screen is the shell's, not the region.
                    if self.backend.take_resumed() {
                        self.painter.reset();
                    }
                    self.resized(columns, rows)?;
                }
                Some(Event::Mouse(mouse)) => {
                    if let Some(index) = self.mouse_target(mouse) {
                        if let Some(event) = self.locate(index, mouse) {
                            self.deliver(index, &event)?;
                        }
                    }
                }
                Some(event) => {
                    if let Some(index) = self
                        .mounted
                        .iter()
                        .position(|(component, _)| !component.finished())
                    {
                        self.deliver(index, &event)?;
                    }
                }
            }
            self.fire_due()?;
        }
    }
}

/// Why [`run`] returned no outcome.
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    NotInteractive(NotInteractive),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(error) => write!(f, "{error}"),
            Error::NotInteractive(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Error {
        Error::Io(error)
    }
}

/// Options for [`run`].
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
    pub policy: Policy,
    pub session: SessionOptions,
    pub paint: LoopOptions,
}

/// Run one component on the terminal until it finishes: the blocking
/// driver. When the terminal is not interactive (see [`Policy`]), no
/// session starts and the policy's fallback decides the result.
pub fn run<C: Component>(component: C, options: &RunOptions) -> Result<Outcome<C::Output>, Error> {
    let mut component = component;
    if let Err(reason) = options.policy.detect_for(options.session.output) {
        return degrade(
            &mut component,
            options.policy.fallback,
            reason,
            &mut StdLineIo,
        );
    }
    let mut session = options.session;
    if component.mouse() {
        session.mouse = true;
        // Clicks are placed from the top of the region, which is known on
        // the alternate screen and, inline, only by asking the terminal on
        // standard output: painting on standard error, take the screen.
        if session.output == crate::session::Output::Stderr {
            session.alternate_screen = true;
        }
    }
    let mut event_loop = EventLoop::terminal(session, options.paint.clone())?;
    let handle = event_loop.mount(component);
    event_loop.run()?;
    drop(event_loop);
    Ok(handle.take().unwrap_or(Outcome::Interrupted))
}

/// What [`run`] does without a terminal: follow `fallback`, asking through
/// `io` when it says to prompt, with terminal controls in what is written
/// shown as text. A prompt that ends with
/// [`NotInteractive::Ended`] (input ran out, no default) fails with
/// [`NotInteractive::NoDefault`] and the reason; one that ends with
/// [`NotInteractive::Interrupted`] is [`Outcome::Interrupted`].
pub fn degrade<C: Component>(
    component: &mut C,
    fallback: Fallback,
    reason: crate::policy::Reason,
    io: &mut dyn LineIo,
) -> Result<Outcome<C::Output>, Error> {
    let default = |component: &C| {
        component
            .default_value()
            .map(Outcome::Done)
            .ok_or(Error::NotInteractive(NotInteractive::NoDefault(reason)))
    };
    match fallback {
        Fallback::Error => Err(Error::NotInteractive(NotInteractive::Terminal(reason))),
        Fallback::Default => default(component),
        Fallback::Prompt => match component.prompt(&mut crate::policy::Shown(io)) {
            Ok(Some(value)) => Ok(Outcome::Done(value)),
            Ok(None) => Ok(Outcome::Cancelled),
            Err(NotInteractive::NoPrompt) => default(component),
            Err(NotInteractive::Ended) => {
                Err(Error::NotInteractive(NotInteractive::NoDefault(reason)))
            }
            Err(NotInteractive::Interrupted) => Ok(Outcome::Interrupted),
            Err(error) => Err(Error::NotInteractive(error)),
        },
    }
}

/// The URL of the hyperlink at `column` of `line`, if one is there.
fn link_at(line: &[Segment], column: usize) -> Option<String> {
    let mut at = 0;
    for segment in line.iter().filter(|segment| !segment.control) {
        let width = segment.cell_length();
        if column < at + width {
            return segment
                .style
                .as_ref()
                .and_then(|style| style.link())
                .filter(|url| !url.is_empty())
                .map(str::to_string);
        }
        at += width;
    }
    None
}

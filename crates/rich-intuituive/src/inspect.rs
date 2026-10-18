//! The inspector: a panel docked on the right of a running app that shows
//! the node tree as it is, which nodes drew in the last frame, where the
//! focus is, what each frame and each node cost, and what happened in the
//! app.
//!
//! Turn it on with [`App::inspector`](crate::App::inspector) or by setting
//! `INTUITUIVE_INSPECT=1` before starting any app; F12 shows and hides it.
//! The app is laid out in the space to its left, so everything stays
//! visible. While it is shown the app keeps the keys: Shift+F12, or a
//! click on the panel, gives them to the inspector, and Esc (or Shift+F12)
//! gives them back. Its panes, chosen with Tab, `1` to `4` or a click on
//! their names:
//!
//! - **tree**: each line one node: its [name](crate::Node::name), or the
//!   builder that made it, what it holds (a panel's title, a list's
//!   length, a grid's shape), and its size, or `hidden`; yellow if it drew
//!   in the last frame, reversed if it has the focus. ↑ and ↓ select a
//!   node, ← and → go to its parent and its first child, Enter shows its
//!   details; while the inspector has the keys, the selected node's
//!   rectangle is outlined on the app.
//! - **node**: the selected node: where it is, its size and its limits,
//!   its alignment, grid place, padding and border, whether it takes the
//!   focus, the rules that show or hide it, its classes and states, whether
//!   it was dirty and drew in the last frame, and its timings.
//! - **timings**: the frames' times, and each node's draws and time,
//!   cumulative and in the last frame it drew, slowest first (`s` sorts
//!   another way, `c` starts again).
//! - **trace**: what happened, newest last: keys with their modifiers,
//!   presses, releases, drags and the wheel with the node under the pointer,
//!   and for each the handler that took it (`on_key`, `on_click`, a widget)
//!   and its node; resizes, pastes, the focus moving, screens opening and
//!   closing, signal writes (named by where the signal was made and where
//!   it was written) with the nodes they made dirty, timers, watches, tasks
//!   done and jobs from other threads. Space pauses it, `c` clears it, and
//!   it keeps the last [`TRACE_LIMIT`] events.
//!
//! `p` picks a node from the app: the pointer, or a cursor moved with the
//! arrows, selects what is under it; a click or Enter keeps it, Esc goes
//! back. `f` turns on the redraw flash, which marks what each frame drew on
//! the app for a moment.
//!
//! Nothing is measured or traced while the inspector is closed, and the
//! app draws as it does without one. In linear mode, which draws nothing,
//! the inspector is off.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::panic::Location;
use std::time::Duration;

use rich::{Console, Segment, Style};
use rich_interact::{Button, Key, KeyCode, Mouse, MouseKind};

use crate::a11y::Role;
use crate::app::FrameStats;
use crate::layout::{Align, AlignContent, Justify, Size, StackLayout};
use crate::node::{Axis, Node};
use crate::reactive::NodeId;
use crate::screen::{Rect, Screen};

/// How many events the trace keeps; the oldest go first.
pub const TRACE_LIMIT: usize = 1000;

/// How long the redraw flash marks a region.
const FLASH: Duration = Duration::from_millis(300);

/// The most regions the flash marks at once.
const FLASH_REGIONS: usize = 256;

/// How many frames' times the timings average over.
const FRAME_TIMES: usize = 120;

/// What a line of the [trace](TraceEntry) is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TraceKind {
    /// A key pressed, and the handler that took it.
    Key,
    /// A press, release, drag or turn of the wheel, and what took it.
    Mouse,
    /// Pasted text.
    Paste,
    /// The terminal's new size.
    Resize,
    /// The focus moving.
    Focus,
    /// A screen, modal, pop-up, palette or help opening, or one closing.
    Screen,
    /// A signal written, and the nodes it made dirty.
    Signal,
    /// A [task](crate::spawn) done, its result back on the app's thread.
    Task,
    /// A [timer](crate::every) firing.
    Timer,
    /// A [watch](crate::watch) running.
    Watch,
    /// A job another thread sent through a [`Proxy`](crate::Proxy).
    Proxy,
}

impl TraceKind {
    /// Its name: `key`, `mouse`, `signal`, …
    pub fn name(self) -> &'static str {
        match self {
            TraceKind::Key => "key",
            TraceKind::Mouse => "mouse",
            TraceKind::Paste => "paste",
            TraceKind::Resize => "resize",
            TraceKind::Focus => "focus",
            TraceKind::Screen => "screen",
            TraceKind::Signal => "signal",
            TraceKind::Task => "task",
            TraceKind::Timer => "timer",
            TraceKind::Watch => "watch",
            TraceKind::Proxy => "proxy",
        }
    }
}

/// One line of the inspector's event trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceEntry {
    /// The frame that drew after it: the number the inspector's header
    /// shows.
    pub frame: u64,
    /// The app's clock when it happened.
    pub at: Duration,
    pub kind: TraceKind,
    /// What happened, as the trace shows it: `key ctrl+s → editor on_key`.
    /// Nodes are named by their [name](crate::Node::name), else by their
    /// builder and [id](crate::Node::id) (`text#7`).
    pub text: String,
}

/// What the inspector measured of one node since it opened (or since its
/// timings were cleared).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeTiming {
    pub id: NodeId,
    /// Its name, else its builder and id (`text#7`); `#7` once it has left
    /// the tree.
    pub name: String,
    /// The frames it drew in.
    pub draws: u64,
    /// The time it took to lay out and draw itself, its children's not
    /// counted, over all its draws.
    pub total: Duration,
    /// The same, the last time it drew.
    pub last: Duration,
}

// ---------------------------------------------------------------------------
// The tracer: what the runtime and the app note while the inspector is open.

/// Who used an event: the node and how (`on_key`, `on_click`, `widget`).
pub(crate) type Used = Option<(NodeId, &'static str)>;

/// A signal write, as the runtime notes it.
pub(crate) struct Write {
    pub slot: usize,
    pub ty: &'static str,
    /// Where the signal was made, and where it was written.
    pub made: Option<&'static Location<'static>>,
    pub at: &'static Location<'static>,
    /// Whether it was the outermost update: the nodes reached are its.
    pub outer: bool,
}

/// An event, as it was noted; it is named once the inspector takes it.
pub(crate) enum Traced {
    Key {
        key: Key,
        used: Used,
    },
    Mouse {
        mouse: Mouse,
        target: Option<NodeId>,
        used: Used,
    },
    Paste {
        chars: usize,
        used: Used,
    },
    Resize(u16, u16),
    Focus(Option<NodeId>),
    Screen(&'static str),
    Signal {
        ty: &'static str,
        made: Option<&'static Location<'static>>,
        at: &'static Location<'static>,
        /// The nodes it made dirty; `None` for a write inside another
        /// update, whose nodes the outer one lists.
        readers: Option<Vec<NodeId>>,
    },
    Task {
        at: &'static Location<'static>,
        dropped: bool,
    },
    Proxy(&'static Location<'static>),
    Timer {
        every: Duration,
        at: &'static Location<'static>,
    },
    Watch(Option<&'static Location<'static>>),
}

impl Traced {
    /// Say who used the event this is.
    pub(crate) fn set_used(&mut self, by: Used) {
        match self {
            Traced::Key { used, .. } | Traced::Mouse { used, .. } | Traced::Paste { used, .. } => {
                *used = by
            }
            _ => {}
        }
    }
}

/// The events an app's runtime notes while its inspector is open, in the
/// order they happen, until the inspector takes them at the next frame.
/// Off, each hook costs one flag's check.
#[derive(Default)]
pub(crate) struct Tracer {
    on: Cell<bool>,
    /// The app's clock, for each event's time.
    now: Cell<Duration>,
    pending: RefCell<VecDeque<(u64, Duration, Traced)>>,
    next: Cell<u64>,
    /// The nodes the write being made reached.
    reached: RefCell<Vec<NodeId>>,
    /// Slots whose writes are left out: the framework's own.
    quiet: RefCell<HashSet<usize>>,
    /// The names of nodes on screens that closed since the events were
    /// last taken, which events may name.
    gone: RefCell<HashMap<NodeId, String>>,
}

impl Tracer {
    pub(crate) fn on(&self) -> bool {
        self.on.get()
    }

    pub(crate) fn set_on(&self, on: bool) {
        self.on.set(on);
        if !on {
            self.reached.borrow_mut().clear();
        }
    }

    pub(crate) fn set_now(&self, now: Duration) {
        self.now.set(now);
    }

    /// Leave writes to `slot` out.
    pub(crate) fn quiet(&self, slot: usize) {
        self.quiet.borrow_mut().insert(slot);
    }

    /// Note `event`; its number, to [amend](Self::amend) it, while on.
    pub(crate) fn push(&self, event: Traced) -> Option<u64> {
        if !self.on() {
            return None;
        }
        let seq = self.next.get();
        self.next.set(seq + 1);
        let mut pending = self.pending.borrow_mut();
        if pending.len() >= TRACE_LIMIT {
            pending.pop_front();
        }
        pending.push_back((seq, self.now.get(), event));
        Some(seq)
    }

    /// Change event `seq`, if it was not taken yet.
    pub(crate) fn amend(&self, seq: u64, f: impl FnOnce(&mut Traced)) {
        let mut pending = self.pending.borrow_mut();
        if let Some((_, _, event)) = pending.iter_mut().rev().find(|(s, _, _)| *s == seq) {
            f(event);
        }
    }

    /// A write reached `node`, a reader of what it changed.
    pub(crate) fn reached(&self, node: NodeId) {
        if self.on() {
            let mut reached = self.reached.borrow_mut();
            if reached.len() < 64 && !reached.contains(&node) {
                reached.push(node);
            }
        }
    }

    /// A write starts: an outermost one starts counting the nodes it
    /// reaches.
    pub(crate) fn start_write(&self, outer: bool) -> bool {
        if outer {
            self.reached.borrow_mut().clear();
        }
        outer
    }

    /// A write ends.
    pub(crate) fn end_write(&self, write: Write) {
        let readers = write
            .outer
            .then(|| std::mem::take(&mut *self.reached.borrow_mut()));
        if self.quiet.borrow().contains(&write.slot) {
            return;
        }
        self.push(Traced::Signal {
            ty: write.ty,
            made: write.made,
            at: write.at,
            readers,
        });
    }

    /// A screen whose root is `root` closes: keep its nodes' names for
    /// the events that name them.
    pub(crate) fn farewell(&self, root: &Node) {
        if self.on() {
            let mut gone = self.gone.borrow_mut();
            root.walk_all(&mut |node, _, _| {
                gone.insert(node.id(), node.short());
            });
        }
    }

    /// The events noted since the last call, oldest first, and the names
    /// of the nodes that left with their screens meanwhile.
    pub(crate) fn take(&self) -> (Vec<(Duration, Traced)>, HashMap<NodeId, String>) {
        let events = self
            .pending
            .borrow_mut()
            .drain(..)
            .map(|(_, at, event)| (at, event))
            .collect();
        (events, std::mem::take(&mut *self.gone.borrow_mut()))
    }

    /// Whether events wait to be taken.
    pub(crate) fn has_pending(&self) -> bool {
        !self.pending.borrow().is_empty()
    }

    /// The events waiting to be taken, as the trace will show them at
    /// `frame`.
    pub(crate) fn peek(&self, names: &Names, frame: u64) -> Vec<TraceEntry> {
        let gone = self.gone.borrow();
        let names = &names.with(&gone);
        self.pending
            .borrow()
            .iter()
            .map(|(_, at, event)| {
                let (kind, text) = describe(event, names);
                TraceEntry {
                    frame,
                    at: *at,
                    kind,
                    text,
                }
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// What the node pane reads of a node.

/// A grid's columns and rows, and its gaps between rows and between
/// columns.
pub(crate) type GridFacts = (Vec<Size>, Vec<Size>, (u16, u16));

/// A node's layout, focus, rules and states, as [`Node::facts`] reads
/// them.
pub(crate) struct Facts {
    /// Its size along its parent's axis, and who set it (`code`,
    /// `stylesheet`; neither: the default).
    pub size: Size,
    pub size_from: Option<&'static str>,
    pub min: u16,
    pub max: u16,
    pub span: (u16, u16),
    pub align_self: Option<Align>,
    pub dock: Option<&'static str>,
    /// A stylesheet's border round its widget, and its padding.
    pub border: Option<&'static str>,
    pub padding: [u16; 4],
    /// For a column or row: its axis and flow.
    pub flow: Option<(Axis, StackLayout)>,
    /// For a grid: its columns, rows and gaps (between rows, columns).
    pub grid: Option<GridFacts>,
    /// For a padding node, its edges; for a panel, its padding.
    pub edges: Option<[u16; 4]>,
    pub focusable: bool,
    pub autofocus: bool,
    pub disabled: bool,
    /// Its responsive rules, each with whether it holds (shows it) now.
    pub rules: Vec<(String, bool)>,
    /// Hidden by a stylesheet's `display: none`.
    pub sheet_hidden: bool,
    pub access_hidden: bool,
    pub classes: Vec<String>,
    /// Its conditional classes: those that hold now, those that do not.
    pub active: Vec<String>,
    pub inactive: Vec<String>,
    pub selected: bool,
    pub invalid: bool,
    /// Whether a stylesheet rule watches its `:focus` and its `:hover`.
    pub watches: (bool, bool),
    pub role: Role,
    /// Its key bindings, each binding's keys joined by `/`.
    pub keys: Vec<String>,
    pub handlers: Vec<&'static str>,
    pub focus_style: Option<String>,
    pub hover_style: Option<String>,
}

// ---------------------------------------------------------------------------
// The inspector.

/// The inspector's panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Pane {
    #[default]
    Tree,
    Node,
    Timings,
    Trace,
}

impl Pane {
    const ALL: [Pane; 4] = [Pane::Tree, Pane::Node, Pane::Timings, Pane::Trace];

    fn name(self) -> &'static str {
        match self {
            Pane::Tree => "tree",
            Pane::Node => "node",
            Pane::Timings => "timings",
            Pane::Trace => "trace",
        }
    }

    fn index(self) -> usize {
        Pane::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// How the timings pane orders the nodes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Total,
    Last,
    Draws,
}

/// Picking a node from the app.
pub(crate) struct Pick {
    /// The cell under the cursor (the pointer, or the arrows').
    pub at: (u16, u16),
    /// The selection and the pane before, which Esc puts back.
    before: Option<NodeId>,
    pane: Pane,
}

/// A line of the tree: a node, how deep it is, and whether it is hidden.
#[derive(Clone, Copy)]
struct Row {
    id: NodeId,
    depth: usize,
}

#[derive(Clone, Copy, Default)]
struct Timing {
    draws: u64,
    total: Duration,
    last: Duration,
}

/// The last frame the app drew in: a frame drawn for the inspector alone
/// (its selection moving) leaves it, so the header, the tree's colours
/// and the node pane keep saying what the app did.
struct Summary {
    frame: u64,
    drawn: Vec<NodeId>,
    dirty: HashSet<NodeId>,
    rects: usize,
    cells: u32,
    /// The frame before's bytes: this one's are not known yet.
    bytes: usize,
}

/// The cells the inspector drew over: whole rows of the app's part of the
/// screen, as they were before.
struct Saved {
    x: u16,
    width: u16,
    rows: Vec<u16>,
    cells: Screen,
}

/// Where a click on the panel lands.
#[derive(Clone, Copy)]
enum Hit {
    Tab(Pane),
    Row(usize),
}

/// What the app does after the inspector had a key or a mouse event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Handled {
    /// The event was the inspector's: the app does not see it.
    pub used: bool,
    /// The inspector changed: draw a frame.
    pub redraw: bool,
    /// Picking started: the pointer's movement must be reported.
    pub motion: bool,
    /// The trace was paused or resumed.
    pub retrace: bool,
}

impl Handled {
    fn used() -> Handled {
        Handled {
            used: true,
            redraw: true,
            ..Handled::default()
        }
    }
}

/// What the inspector asks the app while it handles an event.
pub(crate) struct Probe<'a> {
    /// The node with the focus on the top screen.
    pub focus: Option<NodeId>,
    /// The app's part of the screen.
    pub area: Rect,
    /// The deepest node shown at a cell, on the topmost screen there.
    pub hit: &'a dyn Fn(u16, u16) -> Option<NodeId>,
    /// Where a node is on the screen, if it is shown.
    pub rect: &'a dyn Fn(NodeId) -> Option<Rect>,
}

/// The styles the inspector draws over the app with.
pub(crate) struct Looks {
    /// The selected node's outline.
    pub selected: Style,
    /// The picking cursor.
    pub cursor: Style,
    /// The redraw flash.
    pub flash: Style,
}

/// The inspector's state.
pub(crate) struct Inspector {
    pub open: bool,
    /// Frames drawn while it was shown.
    pub frames: u64,
    /// Keys go to the inspector, not the app.
    pub keys: bool,
    pub pane: Pane,
    pub selected: Option<NodeId>,
    pub pick: Option<Pick>,
    pub flash: bool,
    /// The trace takes nothing in while paused.
    pub paused: bool,
    /// The app's part of the screen, and the panel's, at the last frame,
    /// and whether the panel was shown (the screen may be too narrow).
    pub area: Rect,
    pub panel: Rect,
    pub shown: bool,
    last: Option<Summary>,
    /// The tree's rows at the last frame: what the arrows move through.
    rows: Vec<Row>,
    /// The first line shown of the tree, and of the timings' list.
    tree_top: usize,
    list_top: usize,
    trace: VecDeque<TraceEntry>,
    /// How many entries back from the newest the trace's view ends: 0
    /// follows the newest.
    trace_back: usize,
    timings: HashMap<NodeId, Timing>,
    /// The time each of the last frames took to draw its nodes.
    frame_times: VecDeque<Duration>,
    /// Nodes that drew in the last frame.
    last_drew: usize,
    sort: Sort,
    /// Regions the flash marks, and until when.
    flashes: Vec<(Rect, Duration)>,
    saved: Option<Saved>,
    /// Where clicks on the panel land: line, first and last column (from
    /// the panel's top left), and what is there.
    hits: Vec<(u16, u16, u16, Hit)>,
    /// How many lines the pane last showed: a page.
    page: usize,
}

impl Inspector {
    pub fn new(open: bool) -> Inspector {
        Inspector {
            open,
            frames: 0,
            keys: false,
            pane: Pane::Tree,
            selected: None,
            pick: None,
            flash: false,
            paused: false,
            area: Rect::default(),
            panel: Rect::default(),
            shown: false,
            last: None,
            rows: Vec::new(),
            tree_top: 0,
            list_top: 0,
            trace: VecDeque::new(),
            trace_back: 0,
            timings: HashMap::new(),
            frame_times: VecDeque::new(),
            last_drew: 0,
            sort: Sort::Total,
            flashes: Vec::new(),
            saved: None,
            hits: Vec::new(),
            page: 1,
        }
    }

    /// The columns the panel takes from a screen `width` wide, or `None`
    /// if the screen is too narrow to share.
    pub fn width(&self, width: u16) -> Option<u16> {
        (self.open && width >= 40).then(|| (width * 2 / 5).clamp(24, 56))
    }

    /// Show or hide it; hidden, it gives the keys back, and nothing it drew
    /// stays (the next frame draws everything).
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        if !open {
            self.keys = false;
            self.pick = None;
            self.flashes.clear();
        }
    }

    /// Whether the trace takes events in: open and not paused.
    pub fn tracing(&self) -> bool {
        self.open && !self.paused
    }

    /// Take or give back the keys (Shift+F12).
    pub fn toggle_keys(&mut self) {
        self.keys = !self.keys;
        self.pick = None;
    }

    // -- Keys and the mouse ------------------------------------------------

    /// A key, while the inspector has the keys. Every key is its.
    pub fn key(&mut self, key: Key, probe: &Probe) -> Handled {
        let mut out = Handled::used();
        if self.pick.is_some() {
            self.pick_key(key, probe);
            return out;
        }
        if key.modifiers.ctrl || key.modifiers.alt {
            return out;
        }
        match key.code {
            KeyCode::Escape => self.keys = false,
            KeyCode::Tab => self.pane = Pane::ALL[(self.pane.index() + 1) % Pane::ALL.len()],
            KeyCode::BackTab => {
                self.pane = Pane::ALL[(self.pane.index() + Pane::ALL.len() - 1) % Pane::ALL.len()]
            }
            KeyCode::Char(c @ '1'..='4') => self.pane = Pane::ALL[c as usize - '1' as usize],
            KeyCode::Char('p') => {
                self.start_pick(probe);
                out.motion = true;
            }
            KeyCode::Char('f') => {
                self.flash = !self.flash;
                self.flashes.clear();
            }
            _ => match self.pane {
                Pane::Tree | Pane::Node => self.tree_key(key, probe),
                Pane::Timings => self.timings_key(key),
                Pane::Trace => out.retrace = self.trace_key(key),
            },
        }
        out
    }

    /// Moving through the tree.
    fn tree_key(&mut self, key: Key, probe: &Probe) {
        if self.rows.is_empty() {
            return;
        }
        let nav = matches!(
            key.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Enter
                | KeyCode::Char('k' | 'j' | 'h' | 'l' | 'g' | 'G')
        );
        let at = self
            .selected
            .and_then(|id| self.rows.iter().position(|row| row.id == id));
        let Some(at) = at else {
            // Nothing selected yet: the first key selects the focused
            // node, or the first.
            if nav {
                let start = probe
                    .focus
                    .and_then(|focus| self.rows.iter().position(|row| row.id == focus))
                    .unwrap_or(0);
                self.selected = Some(self.rows[start].id);
            }
            return;
        };
        let last = self.rows.len() - 1;
        let page = self.page.max(1);
        let depth = self.rows[at].depth;
        let to = match key.code {
            KeyCode::Up | KeyCode::Char('k') => at.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => (at + 1).min(last),
            KeyCode::PageUp => at.saturating_sub(page),
            KeyCode::PageDown => (at + page).min(last),
            KeyCode::Home | KeyCode::Char('g') => 0,
            KeyCode::End | KeyCode::Char('G') => last,
            // The parent: the nearest row above that is less deep.
            KeyCode::Left | KeyCode::Char('h') => self.rows[..at]
                .iter()
                .rposition(|row| row.depth < depth)
                .unwrap_or(at),
            // The first child: the next row, if it is one deeper.
            KeyCode::Right | KeyCode::Char('l') => match self.rows.get(at + 1) {
                Some(row) if row.depth == depth + 1 => at + 1,
                _ => at,
            },
            KeyCode::Enter => {
                self.pane = match self.pane {
                    Pane::Tree => Pane::Node,
                    _ => Pane::Tree,
                };
                return;
            }
            _ => return,
        };
        self.selected = Some(self.rows[to].id);
    }

    fn timings_key(&mut self, key: Key) {
        let page = self.page.max(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.list_top = self.list_top.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => self.list_top += 1,
            KeyCode::PageUp => self.list_top = self.list_top.saturating_sub(page),
            KeyCode::PageDown => self.list_top += page,
            KeyCode::Home | KeyCode::Char('g') => self.list_top = 0,
            KeyCode::End | KeyCode::Char('G') => self.list_top = usize::MAX / 2,
            KeyCode::Char('s') => {
                self.sort = match self.sort {
                    Sort::Total => Sort::Last,
                    Sort::Last => Sort::Draws,
                    Sort::Draws => Sort::Total,
                }
            }
            KeyCode::Char('c') => {
                self.timings.clear();
                self.frame_times.clear();
                self.list_top = 0;
            }
            _ => {}
        }
    }

    /// Scrolling, pausing and clearing the trace; whether it was paused or
    /// resumed.
    fn trace_key(&mut self, key: Key) -> bool {
        let oldest = self.trace.len().saturating_sub(1);
        let page = self.page.max(1);
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.trace_back = (self.trace_back + 1).min(oldest),
            KeyCode::Down | KeyCode::Char('j') => {
                self.trace_back = self.trace_back.saturating_sub(1)
            }
            KeyCode::PageUp => self.trace_back = (self.trace_back + page).min(oldest),
            KeyCode::PageDown => self.trace_back = self.trace_back.saturating_sub(page),
            KeyCode::Home | KeyCode::Char('g') => self.trace_back = oldest,
            KeyCode::End | KeyCode::Char('G') => self.trace_back = 0,
            KeyCode::Char(' ') => {
                self.paused = !self.paused;
                return true;
            }
            KeyCode::Char('c') => {
                self.trace.clear();
                self.trace_back = 0;
            }
            _ => {}
        }
        false
    }

    /// Start picking, the cursor on the selected node's corner (or the
    /// middle of the app), the node pane showing what is under it.
    fn start_pick(&mut self, probe: &Probe) {
        let area = probe.area;
        let at = self
            .selected
            .and_then(probe.rect)
            .filter(|rect| !rect.is_empty())
            .map(|rect| (rect.x, rect.y))
            .unwrap_or((area.x + area.width / 2, area.y + area.height / 2));
        self.pick = Some(Pick {
            at,
            before: self.selected,
            pane: self.pane,
        });
        self.pane = Pane::Node;
    }

    /// The cursor is at `at`: select what is under it.
    fn pick_at(&mut self, at: (u16, u16), probe: &Probe) {
        if let Some(pick) = &mut self.pick {
            pick.at = at;
        }
        if let Some(id) = (probe.hit)(at.0, at.1) {
            self.selected = Some(id);
        }
    }

    /// A key while picking: the arrows move the cursor (Shift, five
    /// cells), Enter keeps what it is on, Esc puts the selection back.
    fn pick_key(&mut self, key: Key, probe: &Probe) {
        let Some(pick) = &self.pick else {
            return;
        };
        let step = if key.modifiers.shift { 5 } else { 1 };
        let (x, y) = pick.at;
        let at = match key.code {
            KeyCode::Escape => {
                self.selected = pick.before;
                self.pane = pick.pane;
                self.pick = None;
                return;
            }
            KeyCode::Enter | KeyCode::Char('p') => {
                self.pick = None;
                self.pane = Pane::Node;
                return;
            }
            KeyCode::Up | KeyCode::Char('k') => (x, y.saturating_sub(step)),
            KeyCode::Down | KeyCode::Char('j') => (x, y.saturating_add(step)),
            KeyCode::Left | KeyCode::Char('h') => (x.saturating_sub(step), y),
            KeyCode::Right | KeyCode::Char('l') => (x.saturating_add(step), y),
            _ => return,
        };
        let area = probe.area;
        if area.is_empty() {
            return;
        }
        let at = (
            at.0.clamp(area.x, area.right() - 1),
            at.1.clamp(area.y, area.bottom() - 1),
        );
        self.pick_at(at, probe);
    }

    /// A mouse event, while the inspector is shown, at a cell of the
    /// screen. `panel` is where it is.
    pub fn mouse(&mut self, mouse: Mouse, panel: Rect, probe: &Probe) -> Handled {
        let (x, y) = (mouse.column, mouse.row);
        if self.pick.is_some() && probe.area.contains(x, y) {
            match mouse.kind {
                MouseKind::Moved | MouseKind::Drag(_) => self.pick_at((x, y), probe),
                MouseKind::Down(Button::Left) => {
                    self.pick_at((x, y), probe);
                    self.pick = None;
                    self.pane = Pane::Node;
                    self.keys = true;
                }
                _ => {}
            }
            return Handled::used();
        }
        if !panel.contains(x, y) {
            // A press on the app gives it the keys back, and is its.
            if (self.keys || self.pick.is_some()) && matches!(mouse.kind, MouseKind::Down(_)) {
                self.keys = false;
                self.pick = None;
                return Handled {
                    redraw: true,
                    ..Handled::default()
                };
            }
            return Handled::default();
        }
        match mouse.kind {
            MouseKind::Down(Button::Left) => {
                self.keys = true;
                self.pick = None;
                let (line, column) = (y - panel.y, x - panel.x);
                let hit = self
                    .hits
                    .iter()
                    .find(|(l, from, to, _)| *l == line && column >= *from && column < *to)
                    .map(|(.., hit)| *hit);
                match hit {
                    Some(Hit::Tab(pane)) => self.pane = pane,
                    Some(Hit::Row(row)) => {
                        if let Some(row) = self.rows.get(row) {
                            self.selected = Some(row.id);
                        }
                    }
                    None => {}
                }
            }
            MouseKind::ScrollUp | MouseKind::ScrollDown => {
                let up = mouse.kind == MouseKind::ScrollUp;
                let code = if up { KeyCode::Up } else { KeyCode::Down };
                for _ in 0..3 {
                    match self.pane {
                        Pane::Tree | Pane::Node => self.tree_key(Key::new(code), probe),
                        Pane::Timings => self.timings_key(Key::new(code)),
                        Pane::Trace => {
                            self.trace_key(Key::new(code));
                        }
                    }
                }
            }
            // The pointer moving over the panel, a button let go there: the
            // panel's, and nothing changes.
            _ => {
                return Handled {
                    used: true,
                    ..Handled::default()
                }
            }
        }
        Handled::used()
    }

    // -- What a frame tells it ---------------------------------------------

    /// A frame drew: each node that drew and its own time, and the time the
    /// frame's nodes took.
    pub fn record(&mut self, times: &[(NodeId, Duration)], took: Duration) {
        for (id, time) in times {
            let timing = self.timings.entry(*id).or_default();
            timing.draws += 1;
            timing.total += *time;
            timing.last = *time;
        }
        self.last_drew = times.len();
        if self.frame_times.len() >= FRAME_TIMES {
            self.frame_times.pop_front();
        }
        self.frame_times.push_back(took);
    }

    /// Take in the events noted since the last frame, named with `names`.
    pub fn take_trace(&mut self, events: Vec<(Duration, Traced)>, names: &Names) {
        if self.paused {
            return;
        }
        for (at, event) in events {
            let (kind, text) = describe(&event, names);
            self.trace.push_back(TraceEntry {
                frame: self.frames,
                at,
                kind,
                text,
            });
            if self.trace.len() > TRACE_LIMIT {
                self.trace.pop_front();
            }
            if self.trace_back > 0 {
                // Scrolled back: the view stays on what it shows.
                self.trace_back = (self.trace_back + 1).min(self.trace.len().saturating_sub(1));
            }
        }
    }

    /// The trace, oldest first, with `pending`, the events the next frame
    /// will take in, after it; the last [`TRACE_LIMIT`] of them.
    pub fn trace_entries(&self, pending: Vec<TraceEntry>) -> Vec<TraceEntry> {
        let mut out: Vec<TraceEntry> = self.trace.iter().cloned().collect();
        if !self.paused {
            out.extend(pending);
        }
        let over = out.len().saturating_sub(TRACE_LIMIT);
        out.drain(..over);
        out
    }

    /// Each node measured, as the timings pane orders them.
    pub fn node_timings(&self, names: &Names) -> Vec<NodeTiming> {
        let mut out: Vec<NodeTiming> = self
            .timings
            .iter()
            .map(|(id, timing)| NodeTiming {
                id: *id,
                name: names.get(*id),
                draws: timing.draws,
                total: timing.total,
                last: timing.last,
            })
            .collect();
        match self.sort {
            Sort::Total => out.sort_by(|a, b| b.total.cmp(&a.total).then(a.id.cmp(&b.id))),
            Sort::Last => out.sort_by(|a, b| b.last.cmp(&a.last).then(a.id.cmp(&b.id))),
            Sort::Draws => out.sort_by(|a, b| b.draws.cmp(&a.draws).then(a.id.cmp(&b.id))),
        }
        out
    }

    // -- Drawing over the app ----------------------------------------------

    /// Put back what the last frame's outline, cursor and flash covered:
    /// before anything draws, so the app's cells are its own again. The
    /// rows put back, to send again where they differ.
    pub fn restore(&mut self, screen: &mut Screen) -> Vec<Rect> {
        let Some(saved) = self.saved.take() else {
            return Vec::new();
        };
        let area = screen.area();
        let mut rows = Vec::new();
        for (i, &y) in saved.rows.iter().enumerate() {
            let row = Rect::new(saved.x, y, saved.width, 1);
            // The screen changed size since: it draws in full anyway.
            if row.intersection(area) != row {
                continue;
            }
            screen.blit(
                &saved.cells,
                Rect::new(0, i as u16, saved.width, 1),
                saved.x,
                y,
            );
            rows.push(row);
        }
        rows
    }

    /// The screen was made again (a resize): nothing to put back.
    pub fn forget_overlay(&mut self) {
        self.saved = None;
        self.flashes.clear();
    }

    /// Draw over the app's `area` of the screen: the redraw flash over
    /// what this frame `drew`, the selected node's outline while the
    /// inspector has the keys (or picks), and the picking cursor. The
    /// cells covered are kept to put back. The rows drawn over.
    pub fn overlay(
        &mut self,
        screen: &mut Screen,
        area: Rect,
        drew: &[Rect],
        now: Duration,
        looks: &Looks,
        rect: &dyn Fn(NodeId) -> Option<Rect>,
    ) -> Vec<Rect> {
        if self.flash {
            self.flashes.retain(|(_, until)| *until > now);
            for region in drew {
                let region = region.intersection(area);
                if !region.is_empty() {
                    self.flashes.push((region, now + FLASH));
                }
            }
            let over = self.flashes.len().saturating_sub(FLASH_REGIONS);
            self.flashes.drain(..over);
        } else {
            self.flashes.clear();
        }
        let outline = (self.keys || self.pick.is_some())
            .then(|| self.selected.and_then(rect))
            .flatten()
            .map(|r| r.intersection(area))
            .filter(|r| !r.is_empty());
        let cursor = self
            .pick
            .as_ref()
            .map(|pick| pick.at)
            .filter(|(x, y)| area.contains(*x, *y));
        if self.flashes.is_empty() && outline.is_none() && cursor.is_none() {
            return Vec::new();
        }
        let mut rows: Vec<u16> = Vec::new();
        for (region, _) in &self.flashes {
            rows.extend(region.y..region.bottom());
        }
        if let Some(r) = outline {
            rows.extend(r.y..r.bottom());
        }
        if let Some((_, y)) = cursor {
            rows.push(y);
        }
        rows.sort_unstable();
        rows.dedup();
        let mut cells = Screen::new(area.width, rows.len() as u16);
        for (i, &y) in rows.iter().enumerate() {
            cells.blit(screen, Rect::new(area.x, y, area.width, 1), 0, i as u16);
        }
        for (region, _) in &self.flashes {
            restyle(screen, *region, &looks.flash);
        }
        if let Some(r) = outline {
            // Its edges only: what is inside still shows.
            for edge in crate::node::edges(r) {
                restyle(screen, edge, &looks.selected);
            }
        }
        if let Some((x, y)) = cursor {
            screen.restyle(x, y, &looks.cursor);
        }
        let damage = rows
            .iter()
            .map(|&y| Rect::new(area.x, y, area.width, 1))
            .collect();
        self.saved = Some(Saved {
            x: area.x,
            width: area.width,
            rows,
            cells,
        });
        damage
    }

    /// When the next flash goes, if one is showing.
    pub fn flash_ends(&self) -> Option<Duration> {
        self.flashes.iter().map(|(_, until)| *until).min()
    }

    // -- The panel ---------------------------------------------------------

    /// The panel's lines, rendered at `width` x `height` with its border.
    pub fn render(
        &mut self,
        console: &Console,
        report: &Report,
        width: u16,
        height: u16,
    ) -> Vec<Vec<Segment>> {
        let rows_inside = (height as usize).saturating_sub(2);
        self.hits.clear();
        if report.fresh || self.last.is_none() {
            self.last = Some(Summary {
                frame: report.frame,
                drawn: report.drawn.to_vec(),
                dirty: report.dirty.clone(),
                rects: report.damage.len(),
                cells: report
                    .damage
                    .iter()
                    .map(|r| r.width as u32 * r.height as u32)
                    .sum(),
                bytes: report.stats.bytes,
            });
        }
        // The tree's rows, which the arrows move through on any pane.
        let tree = self.tree(report);
        if let Some(id) = self.selected {
            if !self.rows.iter().any(|row| row.id == id) {
                // It left the tree.
                self.selected = None;
            }
        }
        let title = if self.pick.is_some() {
            "[b]inspector[/] [reverse] pick [/]".to_string()
        } else if self.keys {
            "[b]inspector[/] [reverse] keys [/] [dim]· esc returns them".to_string()
        } else {
            "[b]inspector[/] [dim]· F12 hides".to_string()
        };
        let mut lines = vec![title, self.tabs()];
        let last = self.last.as_ref().expect("a frame was summed up");
        lines.push(format!(
            "frame {} · [yellow]drew {}[/] of {} dirty",
            last.frame,
            last.drawn.len(),
            last.dirty.len()
        ));
        lines.push(format!("damage {} rects, {} cells", last.rects, last.cells));
        lines.push(format!("sent {} B last frame", last.bytes));
        if let Some(theme) = &report.theme {
            lines.push(format!("theme {}", rich::markup::escape(theme)));
        }
        lines.push(String::new());
        let help = self.help();
        let room = rows_inside.saturating_sub(lines.len() + help.len());
        self.page = room.saturating_sub(2).max(1);
        let first = lines.len();
        let body = match self.pane {
            Pane::Tree => self.tree_lines(tree, room, first),
            Pane::Node => self.node_lines(report),
            Pane::Timings => {
                let names = Names::of(report.layers.iter().map(|(root, _)| *root));
                self.timing_lines(room, &names)
            }
            Pane::Trace => self.trace_lines(room),
        };
        if body.len() > room && room > 0 {
            let more = body.len() - (room - 1);
            lines.extend(body.into_iter().take(room - 1));
            lines.push(format!("[dim]… {more} more"));
        } else {
            lines.extend(body.into_iter().take(room));
        }
        // The help, on the last lines.
        while lines.len() + help.len() < rows_inside {
            lines.push(String::new());
        }
        lines.extend(help.iter().map(|line| format!("[dim]{line}")));
        boxed(console, &lines, width, height)
    }

    /// The panes' names, the shown one reversed, and the modes that are on.
    fn tabs(&mut self) -> String {
        let mut out = String::new();
        let mut column = 1u16;
        for (i, pane) in Pane::ALL.iter().enumerate() {
            if i > 0 {
                out.push(' ');
                column += 1;
            }
            let name = pane.name();
            let width = name.len() as u16;
            // The second line inside the border.
            self.hits.push((2, column, column + width, Hit::Tab(*pane)));
            column += width;
            if *pane == self.pane {
                out.push_str(&format!("[reverse]{name}[/]"));
            } else {
                out.push_str(&format!("[dim]{name}[/]"));
            }
        }
        if self.flash {
            out.push_str(" · [green]flash[/]");
        }
        if self.paused {
            out.push_str(" · [yellow]paused[/]");
        }
        out
    }

    /// The help lines: how to give the inspector the keys, or what its
    /// keys do on the pane shown.
    fn help(&self) -> Vec<&'static str> {
        if self.pick.is_some() {
            return vec!["arrows or the pointer move", "enter or a click keeps · esc"];
        }
        if !self.keys {
            return vec!["⇧F12 or a click to inspect"];
        }
        let pane = match self.pane {
            Pane::Tree => "↑↓ select · ← parent → child · enter",
            Pane::Node => "↑↓ select · enter tree",
            Pane::Timings => "↑↓ scroll · s sort · c clear",
            Pane::Trace => "↑↓ scroll · space pause · c clear",
        };
        vec![pane, "tab panes · p pick · f flash · esc"]
    }

    /// Walk the shown screens for the tree's rows, keeping them for the
    /// arrows; each line's markup, with its row.
    fn tree(&mut self, report: &Report) -> Vec<(Option<usize>, String)> {
        let mut rows = Vec::new();
        let mut lines = Vec::new();
        let drawn: &[NodeId] = self.last.as_ref().map_or(&[], |last| &last.drawn);
        for (index, (root, modal)) in report.layers.iter().enumerate() {
            let kind = if *modal { "modal" } else { "screen" };
            lines.push((None, format!("[dim]{kind} {}[/]", index + 1)));
            root.walk_all(&mut |node, path, hidden| {
                let id = node.id();
                let depth = path.len();
                let rect = node.rect();
                let size = if hidden || rect.is_empty() {
                    "[dim]hidden[/]".to_string()
                } else {
                    format!("[dim]{}x{}[/]", rect.width, rect.height)
                };
                let what = rich::markup::escape(&node.describe());
                let what = if report.focus == Some(id) {
                    format!("[reverse]{what}[/]")
                } else if drawn.contains(&id) {
                    format!("[yellow]{what}[/]")
                } else {
                    what
                };
                let indent = "  ".repeat(depth.min(12));
                let line = if self.selected == Some(id) {
                    format!("[on magenta]›{}{what} {size}[/]", &indent[1..])
                } else {
                    format!("{indent}{what} {size}")
                };
                lines.push((Some(rows.len()), line));
                rows.push(Row { id, depth });
            });
        }
        self.rows = rows;
        lines
    }

    /// The tree pane: the lines that fit, scrolled to the selected node.
    fn tree_lines(
        &mut self,
        lines: Vec<(Option<usize>, String)>,
        room: usize,
        first: usize,
    ) -> Vec<String> {
        let selected = self
            .selected
            .and_then(|id| self.rows.iter().position(|row| row.id == id));
        let keep = selected.and_then(|row| lines.iter().position(|(r, _)| *r == Some(row)));
        let (start, end) = window(lines.len(), room, &mut self.tree_top, keep);
        let mut out = Vec::new();
        if start > 0 {
            out.push(format!("[dim]… {start} above"));
        }
        for (row, line) in &lines[start..end] {
            if let Some(row) = row {
                // Inside the border, below the header lines.
                let at = (1 + first + out.len()) as u16;
                self.hits.push((at, 1, u16::MAX, Hit::Row(*row)));
            }
            out.push(line.clone());
        }
        if end < lines.len() {
            out.push(format!("[dim]… {} more", lines.len() - end));
        }
        out
    }

    /// The node pane: the selected node.
    fn node_lines(&self, report: &Report) -> Vec<String> {
        let Some(id) = self.selected else {
            return vec![
                "[dim]nothing selected".to_string(),
                "[dim]↑↓ select one, or p picks it".to_string(),
            ];
        };
        let mut found = None;
        for (index, (root, _)) in report.layers.iter().enumerate() {
            root.walk_all(&mut |node, path, hidden| {
                if node.id() == id {
                    let parent = path.len().checked_sub(2).map(|at| path[at]);
                    found = Some((
                        index,
                        parent,
                        hidden,
                        node.describe(),
                        node.facts(),
                        node.rect(),
                        node.inner.get(),
                    ));
                }
            });
        }
        let Some((layer, parent, hidden, what, facts, rect, inner)) = found else {
            return vec!["[dim]not in the tree".to_string()];
        };
        let place = parent.and_then(|parent| {
            let mut place = None;
            report.layers[layer].0.walk_all(&mut |node, _, _| {
                if node.id() == parent {
                    place = node.grid_place(id);
                }
            });
            place
        });
        let mut lines = vec![format!(
            "[b]{}[/] [dim]#{id} · {} {}",
            rich::markup::escape(&what),
            if report.layers[layer].1 {
                "modal"
            } else {
                "screen"
            },
            layer + 1
        )];
        // Where it is.
        match (report.rect)(id) {
            Some(shown) if !hidden => {
                lines.push(format!(
                    "at {},{} · {}x{}",
                    shown.x, shown.y, shown.width, shown.height
                ));
                if shown != rect && !rect.is_empty() {
                    lines.push(format!(
                        "[dim]laid out at {},{} · {}x{} (scrolled)",
                        rect.x, rect.y, rect.width, rect.height
                    ));
                }
                if inner != rect && !inner.is_empty() {
                    lines.push(format!(
                        "inside {},{} · {}x{}",
                        inner.x, inner.y, inner.width, inner.height
                    ));
                }
            }
            _ => lines.push("[dim]not on the screen".to_string()),
        }
        // Its size and limits.
        let mut size = format!("size {}", size_name(facts.size));
        match facts.size_from {
            Some(from) => size.push_str(&format!(" ({from})")),
            None => size.push_str(" (default)"),
        }
        if facts.min > 0 {
            size.push_str(&format!(" · min {}", facts.min));
        }
        if facts.max < u16::MAX {
            size.push_str(&format!(" · max {}", facts.max));
        }
        lines.push(size);
        // Its container's settings, and its place in its parent.
        if let Some((axis, flow)) = facts.flow {
            lines.push(flow_line(axis, flow));
        }
        if let Some((columns, rows, gap)) = &facts.grid {
            let sizes = |sizes: &[Size]| {
                if sizes.is_empty() {
                    "even".to_string()
                } else {
                    sizes
                        .iter()
                        .map(|s| size_name(*s))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            };
            let mut line = format!("grid columns {} · rows {}", sizes(columns), sizes(rows));
            if *gap != (0, 0) {
                line.push_str(&format!(" · gap {} {}", gap.0, gap.1));
            }
            lines.push(line);
        }
        let mut place_line = Vec::new();
        if let Some(place) = place {
            place_line.push(format!(
                "grid cell column {} row {}",
                place.column + 1,
                place.row + 1
            ));
        }
        if facts.span != (1, 1) {
            place_line.push(format!("span {}x{}", facts.span.0, facts.span.1));
        }
        if let Some(align) = facts.align_self {
            place_line.push(format!("align-self {}", crate::builtin::css_align(align)));
        }
        if let Some(dock) = facts.dock {
            place_line.push(format!("dock {dock}"));
        }
        if !place_line.is_empty() {
            lines.push(place_line.join(" · "));
        }
        let mut boxing = Vec::new();
        if facts.padding != [0; 4] {
            boxing.push(format!("padding {}", edges_name(facts.padding)));
        }
        if let Some(edges) = facts.edges.filter(|e| *e != [0; 4]) {
            boxing.push(format!("padding {}", edges_name(edges)));
        }
        if let Some(border) = facts.border {
            boxing.push(format!("border {border}"));
        }
        if !boxing.is_empty() {
            lines.push(boxing.join(" · "));
        }
        // The focus.
        let mut focus = Vec::new();
        if facts.focusable {
            focus.push("focusable");
        } else {
            focus.push("not focusable");
        }
        if facts.autofocus {
            focus.push("autofocus");
        }
        if report.focus == Some(id) {
            focus.push("[reverse]focused[/]");
        } else if report.focus_path.contains(&id) {
            focus.push("focus inside");
        }
        if facts.access_hidden {
            focus.push("hidden from assistive technology");
        }
        lines.push(focus.join(" · "));
        // Shown or hidden, and why.
        let failing: Vec<&str> = facts
            .rules
            .iter()
            .filter(|(_, holds)| !holds)
            .map(|(rule, _)| rule.as_str())
            .collect();
        let (columns, rows) = report.viewport;
        if !failing.is_empty() {
            lines.push(format!(
                "[yellow]hidden[/] by {} (app {columns}x{rows})",
                failing.join(", ")
            ));
        } else if facts.sheet_hidden {
            lines.push("[yellow]hidden[/] by the stylesheet (display: none)".to_string());
        } else if rect.is_empty() {
            lines.push("[yellow]no room[/]: laid out 0 cells".to_string());
        } else if !facts.rules.is_empty() {
            let rules: Vec<&str> = facts.rules.iter().map(|(rule, _)| rule.as_str()).collect();
            lines.push(format!(
                "shown: {} (app {columns}x{rows})",
                rules.join(", ")
            ));
        } else {
            lines.push("shown".to_string());
        }
        // Classes and states, as a stylesheet sees them.
        let mut classes: Vec<String> = facts.classes.iter().map(|c| format!(".{c}")).collect();
        classes.extend(facts.active.iter().map(|c| format!("[green].{c}[/]")));
        classes.extend(facts.inactive.iter().map(|c| format!("[dim].{c}[/]")));
        if !classes.is_empty() {
            lines.push(format!("classes {}", classes.join(" ")));
        }
        let mut states = Vec::new();
        if report.focus == Some(id) {
            states.push(":focus");
        }
        if report.hover.contains(&id) {
            states.push(":hover");
        }
        if facts.disabled {
            states.push(":disabled");
        }
        if facts.invalid {
            states.push(":error");
        }
        if facts.selected {
            states.push(":selected");
        }
        let mut line = if states.is_empty() {
            "states none".to_string()
        } else {
            format!("states {}", states.join(" "))
        };
        match facts.watches {
            (true, true) => line.push_str(" [dim](sheet: :focus :hover)"),
            (true, false) => line.push_str(" [dim](sheet: :focus)"),
            (false, true) => line.push_str(" [dim](sheet: :hover)"),
            (false, false) => {}
        }
        lines.push(line);
        if facts.focus_style.is_some() || facts.hover_style.is_some() {
            let mut looks = Vec::new();
            if let Some(style) = &facts.focus_style {
                looks.push(format!("focus {}", rich::markup::escape(style)));
            }
            if let Some(style) = &facts.hover_style {
                looks.push(format!("hover {}", rich::markup::escape(style)));
            }
            lines.push(format!("styles {}", looks.join(" · ")));
        }
        // The last frame the app drew in.
        let (dirty, drew) = self.last.as_ref().map_or((false, false), |last| {
            (last.dirty.contains(&id), last.drawn.contains(&id))
        });
        lines.push(
            match (dirty, drew) {
                (true, true) => "[yellow]dirty, drew[/] in the last frame",
                (true, false) => "[yellow]dirty[/], drew nothing in the last frame",
                (false, true) => "[yellow]drew[/] in the last frame (moved or uncovered)",
                (false, false) => "did not draw in the last frame",
            }
            .to_string(),
        );
        if let Some(timing) = self.timings.get(&id) {
            lines.push(format!(
                "draws {} · all {} · last {}",
                timing.draws,
                millis(timing.total).trim_start(),
                millis(timing.last).trim_start()
            ));
        }
        // What it is, and what it answers.
        lines.push(format!("role {}", facts.role.name()));
        if !facts.keys.is_empty() {
            lines.push(format!(
                "keys {}",
                rich::markup::escape(&facts.keys.join(" · "))
            ));
        }
        if !facts.handlers.is_empty() {
            lines.push(format!("has {}", facts.handlers.join(" · ")));
        }
        lines
    }

    /// The timings pane: the frames, then each node, slowest first.
    fn timing_lines(&mut self, room: usize, names: &Names) -> Vec<String> {
        let frames = self.frame_times.len();
        let last = self.frame_times.back().copied().unwrap_or_default();
        let average = if frames == 0 {
            Duration::ZERO
        } else {
            self.frame_times.iter().sum::<Duration>() / frames as u32
        };
        let slowest = self.frame_times.iter().max().copied().unwrap_or_default();
        let mut lines = vec![
            format!(
                "last frame {} · {} nodes drew",
                millis(last).trim_start(),
                self.last_drew
            ),
            format!(
                "avg {} · max {} · {frames} frames",
                millis(average).trim_start(),
                millis(slowest).trim_start()
            ),
            format!(
                "[dim]{:>7} {:>7} {:>5}  node · by {}",
                "all",
                "last",
                "draws",
                match self.sort {
                    Sort::Total => "all",
                    Sort::Last => "last",
                    Sort::Draws => "draws",
                }
            ),
        ];
        let nodes = self.node_timings(names);
        let room = room.saturating_sub(lines.len());
        let (start, end) = window(nodes.len(), room, &mut self.list_top, None);
        if start > 0 {
            lines.push(format!("[dim]… {start} above"));
        }
        for node in &nodes[start..end] {
            lines.push(format!(
                "{} {} {:>5}  {}",
                millis(node.total),
                millis(node.last),
                node.draws,
                rich::markup::escape(&node.name)
            ));
        }
        if end < nodes.len() {
            lines.push(format!("[dim]… {} more", nodes.len() - end));
        }
        if nodes.is_empty() {
            lines.push("[dim]nothing drew yet".to_string());
        }
        lines
    }

    /// The trace pane: the newest events that fit, or those scrolled to.
    fn trace_lines(&self, room: usize) -> Vec<String> {
        let len = self.trace.len();
        let state = if self.paused {
            "[yellow]paused[/]".to_string()
        } else if self.trace_back > 0 {
            format!("{} back", self.trace_back)
        } else {
            "newest last".to_string()
        };
        let mut lines = vec![format!("{len} events · {state}")];
        let room = room.saturating_sub(1);
        let end = len.saturating_sub(self.trace_back);
        let start = end.saturating_sub(room);
        for entry in self.trace.range(start..end) {
            lines.push(format!(
                "[dim]{}[/] {}",
                entry.frame,
                rich::markup::escape(&entry.text)
            ));
        }
        if len == 0 {
            lines.push("[dim]nothing yet: press a key in the app".to_string());
        }
        lines
    }
}

/// What the inspector is shown of the last frame.
pub(crate) struct Report<'a> {
    /// The screens drawn, bottom first, with whether each is a modal.
    pub layers: Vec<(&'a Node, bool)>,
    /// Whether the app drew in this frame (rather than the inspector
    /// alone): its nodes, what was dirty and the damage to the app's
    /// part of the screen.
    pub fresh: bool,
    pub drawn: &'a [NodeId],
    pub dirty: &'a HashSet<NodeId>,
    pub damage: &'a [Rect],
    pub focus: Option<NodeId>,
    pub focus_path: &'a [NodeId],
    /// The nodes under the pointer, outermost first.
    pub hover: &'a [NodeId],
    /// The frame before this one (this one's bytes are not known yet).
    pub stats: FrameStats,
    pub frame: u64,
    pub theme: Option<String>,
    /// The app's [`viewport`](crate::viewport()).
    pub viewport: (u16, u16),
    /// Where a node is on the screen, if it is shown.
    pub rect: &'a dyn Fn(NodeId) -> Option<Rect>,
}

/// How the trace and the timings name nodes: [`Node::short`] for each
/// node in the tree, `#id` for one that left it.
pub(crate) struct Names(HashMap<NodeId, String>);

impl Names {
    /// The nodes under each of `roots`.
    pub(crate) fn of<'a>(roots: impl IntoIterator<Item = &'a Node>) -> Names {
        let mut names = HashMap::new();
        for root in roots {
            root.walk_all(&mut |node, _, _| {
                names.insert(node.id(), node.short());
            });
        }
        Names(names)
    }

    /// These names and `more`.
    pub(crate) fn with(&self, more: &HashMap<NodeId, String>) -> Names {
        let mut names = self.0.clone();
        for (id, name) in more {
            names.entry(*id).or_insert_with(|| name.clone());
        }
        Names(names)
    }

    pub(crate) fn get(&self, id: NodeId) -> String {
        self.0.get(&id).cloned().unwrap_or_else(|| format!("#{id}"))
    }
}

/// A trace line's kind and text for `event`.
fn describe(event: &Traced, names: &Names) -> (TraceKind, String) {
    let used = |used: &Used| match used {
        Some((id, how)) => format!(" → {} {how}", names.get(*id)),
        None => " (unused)".to_string(),
    };
    match event {
        Traced::Key { key, used: by } => (TraceKind::Key, format!("key {key}{}", used(by))),
        Traced::Mouse {
            mouse,
            target,
            used: by,
        } => {
            let button = |b: &Button| match b {
                Button::Left => "left",
                Button::Right => "right",
                Button::Middle => "middle",
            };
            let what = match &mouse.kind {
                MouseKind::Down(b) => format!("press {}", button(b)),
                MouseKind::Up(b) => format!("release {}", button(b)),
                MouseKind::Drag(b) => format!("drag {}", button(b)),
                MouseKind::Moved => "move".to_string(),
                MouseKind::ScrollUp => "wheel up".to_string(),
                MouseKind::ScrollDown => "wheel down".to_string(),
            };
            let mut held = String::new();
            for (on, name) in [
                (mouse.modifiers.ctrl, "ctrl+"),
                (mouse.modifiers.alt, "alt+"),
                (mouse.modifiers.shift, "shift+"),
            ] {
                if on {
                    held.push_str(name);
                }
            }
            let on = target.map_or(String::new(), |id| format!(" on {}", names.get(id)));
            // A release or a drag nobody takes is no news.
            let by = match (by, &mouse.kind) {
                (None, MouseKind::Up(_) | MouseKind::Drag(_)) => String::new(),
                _ => used(by),
            };
            (
                TraceKind::Mouse,
                format!("{held}{what} {},{}{on}{by}", mouse.column, mouse.row),
            )
        }
        Traced::Paste { chars, used: by } => (
            TraceKind::Paste,
            format!(
                "paste {chars} character{}{}",
                if *chars == 1 { "" } else { "s" },
                used(by)
            ),
        ),
        Traced::Resize(columns, rows) => (TraceKind::Resize, format!("resize {columns}x{rows}")),
        Traced::Focus(id) => (
            TraceKind::Focus,
            match id {
                Some(id) => format!("focus → {}", names.get(*id)),
                None => "focus → nothing".to_string(),
            },
        ),
        Traced::Screen(what) => (TraceKind::Screen, what.to_string()),
        Traced::Signal {
            ty,
            made,
            at,
            readers,
        } => {
            let made = made.map_or("signal".to_string(), |m| format!("signal {}", site(m)));
            let readers = match readers {
                None => "(with the update round it)".to_string(),
                Some(readers) if readers.is_empty() => "→ nothing".to_string(),
                Some(readers) => {
                    let mut named: Vec<String> =
                        readers.iter().take(3).map(|id| names.get(*id)).collect();
                    if readers.len() > 3 {
                        named.push(format!("+{}", readers.len() - 3));
                    }
                    format!("→ {}", named.join(", "))
                }
            };
            (
                TraceKind::Signal,
                format!("{made} {readers} ({}, by {})", short_type(ty), site(at)),
            )
        }
        Traced::Task { at, dropped } => (
            TraceKind::Task,
            format!(
                "task from {} done{}",
                site(at),
                if *dropped { ", cancelled" } else { "" }
            ),
        ),
        Traced::Proxy(at) => (
            TraceKind::Proxy,
            format!("job from another thread ({})", site(at)),
        ),
        Traced::Timer { every, at } => (
            TraceKind::Timer,
            format!("timer every {} ({})", interval(*every), site(at)),
        ),
        Traced::Watch(at) => (
            TraceKind::Watch,
            match at {
                Some(at) => format!("watch ({})", site(at)),
                None => "watch".to_string(),
            },
        ),
    }
}

/// A source location as `file.rs:12`.
fn site(at: &Location) -> String {
    let file = at.file();
    let name = file.rsplit(['/', '\\']).next().unwrap_or(file);
    format!("{name}:{}", at.line())
}

/// A type's name without its paths: `Vec<String>` for
/// `alloc::vec::Vec<alloc::string::String>`.
fn short_type(ty: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let mut chars = ty.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':' && chars.peek() == Some(&':') {
            chars.next();
            word.clear();
        } else if c.is_alphanumeric() || c == '_' {
            word.push(c);
        } else {
            out.push_str(&word);
            word.clear();
            out.push(c);
        }
    }
    out.push_str(&word);
    out
}

/// A timer's interval: `1s`, `250ms`.
fn interval(every: Duration) -> String {
    let ms = every.as_millis();
    if ms >= 1000 && ms.is_multiple_of(1000) {
        format!("{}s", ms / 1000)
    } else {
        format!("{ms}ms")
    }
}

/// A duration in milliseconds, six columns wide: `0.042ms`, `12.5ms`.
fn millis(time: Duration) -> String {
    let ms = time.as_secs_f64() * 1000.0;
    if ms < 10.0 {
        format!("{ms:.3}ms")
    } else if ms < 1000.0 {
        format!("{ms:>5.1}ms")
    } else {
        format!("{:>6.2}s", ms / 1000.0)
    }
}

/// A size as the node pane says it.
fn size_name(size: Size) -> String {
    match size {
        Size::Fixed(n) => format!("fixed {n}"),
        Size::Percent(p) => format!("{p}%"),
        Size::Flex(w) => format!("flex {w}"),
        Size::Auto => "auto".to_string(),
    }
}

/// Four edges, top first, as CSS writes them.
fn edges_name([top, right, bottom, left]: [u16; 4]) -> String {
    format!("{top} {right} {bottom} {left}")
}

/// A column's or row's settings: its axis, gap, and what differs from the
/// defaults.
fn flow_line(axis: Axis, flow: StackLayout) -> String {
    let mut parts = vec![match axis {
        Axis::Vertical => "column".to_string(),
        Axis::Horizontal => "row".to_string(),
    }];
    if flow.gap > 0 {
        parts.push(format!("gap {}", flow.gap));
    }
    if flow.align_items != Align::Stretch {
        parts.push(format!(
            "align-items {}",
            crate::builtin::css_align(flow.align_items)
        ));
    }
    if flow.justify != Justify::Start {
        parts.push(format!("justify {}", justify_name(flow.justify)));
    }
    if flow.wrap {
        parts.push("wrap".to_string());
        if flow.align_content != AlignContent::Stretch {
            parts.push(format!(
                "align-content {}",
                align_content_name(flow.align_content)
            ));
        }
    }
    parts.join(" · ")
}

fn justify_name(justify: Justify) -> &'static str {
    match justify {
        Justify::Start => "flex-start",
        Justify::Center => "center",
        Justify::End => "flex-end",
        Justify::SpaceBetween => "space-between",
        Justify::SpaceAround => "space-around",
        Justify::SpaceEvenly => "space-evenly",
    }
}

fn align_content_name(align: AlignContent) -> &'static str {
    match align {
        AlignContent::Start => "flex-start",
        AlignContent::Center => "center",
        AlignContent::End => "flex-end",
        AlignContent::SpaceBetween => "space-between",
        AlignContent::SpaceAround => "space-around",
        AlignContent::SpaceEvenly => "space-evenly",
        AlignContent::Stretch => "stretch",
    }
}

/// Which of `total` lines show in `room`: from `top`, moved so that line
/// `keep` shows. Lines above the window are said in a line of their own,
/// as are lines below it. Returns the first line shown and the one after
/// the last, and keeps the first in `top`.
fn window(total: usize, room: usize, top: &mut usize, keep: Option<usize>) -> (usize, usize) {
    if total <= room {
        *top = 0;
        return (0, total);
    }
    if room < 3 {
        let start = keep.unwrap_or(*top).min(total - room.max(1));
        *top = start;
        return (start, (start + room).min(total));
    }
    // The lines shown from `start`: the room less the lines saying what
    // is above and below.
    let shown = |start: usize| {
        let mut n = room - usize::from(start > 0);
        if start + n < total {
            n -= 1;
        }
        n
    };
    let mut start = (*top).min(total - 1);
    if let Some(keep) = keep {
        if keep < start {
            start = keep;
        }
        while keep >= start + shown(start) {
            start += 1;
        }
    }
    // Not past the end: the last line shows at the bottom.
    while start > 0 && start - 1 + shown(start - 1) >= total {
        start -= 1;
    }
    *top = start;
    (start, start + shown(start))
}

/// Lay `style` over every cell of `rect`.
fn restyle(screen: &mut Screen, rect: Rect, style: &Style) {
    for y in rect.y..rect.bottom() {
        for x in rect.x..rect.right() {
            screen.restyle(x, y, style);
        }
    }
}

/// `lines` of markup in a magenta box `width` x `height`, each cut at its
/// width.
fn boxed(console: &Console, lines: &[String], width: u16, height: u16) -> Vec<Vec<Segment>> {
    let inner = width.saturating_sub(2).max(1) as usize;
    let text = rich::Text::from_markup(&lines.join("\n"))
        .unwrap_or_else(|_| rich::Text::new(lines.join("\n")));
    let mut options = console.options().update_width(inner);
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
    let body = console.render_lines(&text, &options, true);
    let edge = Some(Style::parse("magenta").expect("a built-in style parses"));
    let mut out = Vec::with_capacity(height as usize);
    out.push(vec![Segment::new(
        format!("╭{}╮", "─".repeat(inner)),
        edge.clone(),
    )]);
    for row in 0..(height as usize).saturating_sub(2) {
        let mut line = vec![Segment::new("│", edge.clone())];
        match body.get(row) {
            Some(cells) => line.extend(cells.iter().cloned()),
            None => line.push(Segment::new(" ".repeat(inner), None)),
        }
        line.push(Segment::new("│", edge.clone()));
        out.push(line);
    }
    out.push(vec![Segment::new(format!("╰{}╯", "─".repeat(inner)), edge)]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_keeps_the_line_in_view_and_says_what_is_cut() {
        let mut top = 0;
        // Everything fits.
        assert_eq!(window(5, 5, &mut top, Some(4)), (0, 5));
        // From the top: four lines and "more".
        assert_eq!(window(20, 5, &mut top, None), (0, 4));
        assert_eq!(window(20, 5, &mut top, Some(3)), (0, 4));
        // Line 10: "above", three lines, "more".
        assert_eq!(window(20, 5, &mut top, Some(10)), (8, 11));
        assert_eq!(top, 8);
        // Back up to line 9, still in view: nothing moves.
        assert_eq!(window(20, 5, &mut top, Some(9)), (8, 11));
        // The last line: "above" and four lines.
        assert_eq!(window(20, 5, &mut top, Some(19)), (16, 20));
        // Back to the top.
        assert_eq!(window(20, 5, &mut top, Some(0)), (0, 4));
    }

    #[test]
    fn type_names_lose_their_paths() {
        assert_eq!(short_type("alloc::string::String"), "String");
        assert_eq!(
            short_type("alloc::vec::Vec<core::option::Option<u64>>"),
            "Vec<Option<u64>>"
        );
        assert_eq!(short_type("(u16, u16)"), "(u16, u16)");
    }

    #[test]
    fn the_tracer_keeps_a_bounded_order_and_nothing_while_off() {
        let tracer = Tracer::default();
        assert_eq!(tracer.push(Traced::Resize(1, 1)), None, "off: nothing");
        tracer.set_on(true);
        for n in 0..(TRACE_LIMIT as u16 + 10) {
            tracer.push(Traced::Resize(n, 1));
        }
        let (taken, _) = tracer.take();
        assert_eq!(taken.len(), TRACE_LIMIT);
        assert!(matches!(taken[0].1, Traced::Resize(10, 1)));
        assert!(!tracer.has_pending());
    }
}

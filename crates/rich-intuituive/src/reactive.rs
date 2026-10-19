//! Reactive state: signals, memos, and the per-app runtime behind them.
//!
//! A [`Signal`] is a value that knows who reads it. While a node draws, the
//! runtime records every signal (and memo) it reads; writing one of them
//! marks that node dirty, and only dirty nodes draw again. Subscriptions are
//! recorded afresh on every draw, so a branch that stops reading a value
//! stops depending on it.
//!
//! A [`Memo`] is a value derived from others. It recomputes when one of its
//! inputs changes, and notifies its own readers only if the result is
//! different: twenty list rows that read `memo(move || selected.get() == i)`
//! do not all redraw when the selection moves, only the two whose answer
//! changed.
//!
//! Signals are `Copy` handles, so closures capture them freely. Each
//! [`App`](crate::App) has its own runtime; [`signal`] and [`memo`] create
//! in the runtime of the app being built or run. From another thread, use
//! a [`Proxy`] to run a closure on the app's thread, where signals live.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::marker::PhantomData;
use std::panic::Location;
use std::rc::Rc;
use std::sync::atomic::AtomicUsize;
use std::sync::{mpsc, Arc};

use crate::app::Ctx;

/// A node's identity in the tree, as the runtime sees it.
pub type NodeId = u64;

/// Who is reading: a node drawing, or a memo computing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Observer {
    Node(NodeId),
    Memo(usize),
    /// A [`watch`]: its callback runs when what it read changes.
    Watch(usize),
    /// Reads that subscribe nobody.
    Untracked,
}

type Compute = Rc<dyn Fn() -> Box<dyn Any>>;
type Equal = fn(&dyn Any, &dyn Any) -> bool;

struct MemoDef {
    compute: Compute,
    equal: Equal,
}

#[derive(Default)]
struct Graph {
    /// Readers of each slot.
    subscribers: Vec<HashSet<Observer>>,
    /// The slots each observer read last time, to unsubscribe it.
    reads: HashMap<Observer, Vec<usize>>,
    /// Who is reading now.
    observing: Vec<Observer>,
    /// Nodes made dirty since the tree last took them.
    dirty: HashSet<NodeId>,
    memos: HashMap<usize, MemoDef>,
    /// Watches whose source changed, to run between frames.
    pending: Vec<usize>,
    /// Readers of each slot in other apps' runtimes (an app reading a
    /// signal another app made).
    foreign: HashMap<usize, HashSet<(u32, Observer)>>,
    /// The slots of other runtimes each observer read last time.
    foreign_reads: HashMap<Observer, Vec<(u32, usize)>>,
    /// Nodes whose own writes while they draw no longer draw them again:
    /// they kept doing it, frame after frame (see `Driver::render`).
    quiet: HashSet<NodeId>,
}

/// What a key-hint footer shows: each binding's keys and description.
pub(crate) type Hints = Vec<(String, String)>;

/// One app's reactive state.
pub(crate) struct Runtime {
    id: u32,
    values: RefCell<Vec<Box<dyn Any>>>,
    graph: RefCell<Graph>,
    /// Closures sent from other threads, run on the app's thread.
    inbox: mpsc::Receiver<Job>,
    outbox: mpsc::Sender<Job>,
    /// What wakes the loop's wait when another thread sends a closure.
    pub(crate) wake: Arc<WakeSlot>,
    /// Tasks started and not yet delivered.
    pub(crate) tasks: Arc<AtomicUsize>,
    /// The clock [`latest`](crate::latest) threads keep to, and the
    /// threads waiting for the app to start running (`None` once it has).
    pub(crate) clock: Arc<crate::task::ThreadClock>,
    threads: RefCell<Option<Vec<StartThread>>>,
    /// Each watch's step: read its source, and call back if it changed.
    watches: RefCell<HashMap<usize, WatchStep>>,
    /// The [`on_write`] watches of each slot, with what keeps each write
    /// for its watch.
    writes: RefCell<HashMap<usize, Vec<(usize, Keep)>>>,
    next_watch: Cell<usize>,
    /// How deep in [`Signal::update`]s (or passing changes on) this is;
    /// changes made meanwhile wait in `queued`, so no reader computes
    /// while a signal's value is out of its slot.
    batch: Cell<usize>,
    queued: RefCell<Vec<usize>>,
    /// The nodes that asked about hover or the pointer; a node leaves when
    /// it is forgotten.
    pub(crate) watchers: crate::widget::Watchers,
    /// The slot of the app's [`viewport`], once made.
    viewport: Cell<Option<usize>>,
    /// Where each slot's signal was made, for the inspector's trace.
    sites: RefCell<Vec<Option<&'static Location<'static>>>>,
    /// Where each watch was made.
    watch_sites: RefCell<HashMap<usize, &'static Location<'static>>>,
    /// What the inspector traces while it is open.
    pub(crate) tracer: crate::inspect::Tracer,
    /// One-shot timers [`after`](crate::after) made, with how long each
    /// waits, until the app takes them onto its clock.
    pub(crate) afters: RefCell<Vec<crate::app::After>>,
    /// The app's clock at the top of its last turn.
    pub(crate) now: Cell<std::time::Duration>,
    /// What a [`key_hints`](crate::widgets::key_hints) footer shows, once
    /// one is made: each binding's keys and description.
    pub(crate) hints: Cell<Option<Signal<Hints>>>,
    /// [`spawn_then`](crate::spawn_then)'s continuations, by number, kept
    /// on the app's thread while their work runs.
    pub(crate) locals: RefCell<HashMap<u64, Box<dyn Any>>>,
    pub(crate) next_local: Cell<u64>,
}

thread_local! {
    static RUNTIMES: RefCell<Vec<Option<Rc<Runtime>>>> = const { RefCell::new(Vec::new()) };
    /// The runtime `signal()` and `memo()` create in.
    static CURRENT: RefCell<Vec<Rc<Runtime>>> = const { RefCell::new(Vec::new()) };
}

impl Runtime {
    pub(crate) fn new() -> Rc<Runtime> {
        let (outbox, inbox) = mpsc::channel();
        RUNTIMES.with(|all| {
            let mut all = all.borrow_mut();
            let runtime = Rc::new(Runtime {
                id: all.len() as u32,
                values: RefCell::new(Vec::new()),
                graph: RefCell::new(Graph::default()),
                inbox,
                outbox,
                wake: Arc::default(),
                tasks: Arc::new(AtomicUsize::new(0)),
                clock: crate::task::ThreadClock::new(),
                threads: RefCell::new(Some(Vec::new())),
                watches: RefCell::new(HashMap::new()),
                writes: RefCell::new(HashMap::new()),
                next_watch: Cell::new(0),
                batch: Cell::new(0),
                queued: RefCell::new(Vec::new()),
                watchers: Default::default(),
                viewport: Cell::new(None),
                sites: RefCell::new(Vec::new()),
                watch_sites: RefCell::new(HashMap::new()),
                tracer: Default::default(),
                afters: RefCell::new(Vec::new()),
                now: Cell::new(std::time::Duration::ZERO),
                hints: Cell::new(None),
                locals: RefCell::new(HashMap::new()),
                next_local: Cell::new(0),
            });
            all.push(Some(runtime.clone()));
            runtime
        })
    }

    fn get(id: u32) -> Rc<Runtime> {
        RUNTIMES.with(|all| {
            all.borrow()
                .get(id as usize)
                .and_then(Clone::clone)
                .expect("a signal used after its app was dropped")
        })
    }

    /// Runtime `id`, if its app is still there.
    fn try_get(id: u32) -> Option<Rc<Runtime>> {
        RUNTIMES.with(|all| all.borrow().get(id as usize).and_then(Clone::clone))
    }

    pub(crate) fn current() -> Rc<Runtime> {
        Runtime::current_for("this")
    }

    /// The current runtime, or a panic that names `what` was called
    /// outside an app.
    pub(crate) fn current_for(what: &str) -> Rc<Runtime> {
        CURRENT.with(|current| {
            current.borrow().last().cloned().unwrap_or_else(|| {
                panic!(
                    "{what} is called while an app is built or running: inside \
                     App::new's closure, a node's closure or an event handler"
                )
            })
        })
    }

    /// The signal behind [`viewport`]: the cells the app lays out in, as
    /// its driver last set them ((0, 0) until it has a size).
    pub(crate) fn viewport(&self) -> Signal<(u16, u16)> {
        let slot = match self.viewport.get() {
            Some(slot) => slot,
            None => {
                let slot = self.insert(Box::new((0u16, 0u16)), None);
                self.viewport.set(Some(slot));
                // A resize says so in the trace already.
                self.tracer.quiet(slot);
                slot
            }
        };
        Signal {
            runtime: self.id,
            slot,
            _type: PhantomData,
        }
    }

    /// Run `f` with this runtime current.
    pub(crate) fn enter<R>(self: &Rc<Self>, f: impl FnOnce() -> R) -> R {
        CURRENT.with(|current| current.borrow_mut().push(self.clone()));
        let result = f();
        CURRENT.with(|current| current.borrow_mut().pop());
        result
    }

    /// Start a thread with `start` once the app runs, or now if it does.
    pub(crate) fn start_thread(&self, start: StartThread) {
        let mut threads = self.threads.borrow_mut();
        match threads.as_mut() {
            Some(waiting) => waiting.push(start),
            None => {
                drop(threads);
                start();
            }
        }
    }

    /// The app runs: start the threads that waited for it, keeping to the
    /// app's clock if `manual`.
    pub(crate) fn start_threads(&self, manual: bool) {
        self.clock.set_manual(manual);
        let waiting = self.threads.borrow_mut().take();
        for start in waiting.into_iter().flatten() {
            start();
        }
    }

    /// Drop the runtime from the registry (the app is finished).
    pub(crate) fn close(&self) {
        self.clock.close();
        RUNTIMES.with(|all| {
            if let Some(slot) = all.borrow_mut().get_mut(self.id as usize) {
                *slot = None;
            }
        });
    }

    fn insert(&self, value: Box<dyn Any>, site: Option<&'static Location<'static>>) -> usize {
        let mut values = self.values.borrow_mut();
        values.push(value);
        self.sites.borrow_mut().push(site);
        self.graph.borrow_mut().subscribers.push(HashSet::new());
        values.len() - 1
    }

    /// Where the signal in `slot` was made, if [`signal`] made it.
    pub(crate) fn site(&self, slot: usize) -> Option<&'static Location<'static>> {
        self.sites.borrow().get(slot).copied().flatten()
    }

    fn subscribe(&self, slot: usize) {
        // Read from another app (a signal shared by two apps of one
        // program): that app's reader is recorded here, and told when the
        // value changes.
        let reader = CURRENT.with(|current| current.borrow().last().cloned());
        if let Some(reader) = reader.filter(|r| r.id != self.id) {
            let observer = reader.graph.borrow().observing.last().copied();
            if let Some(observer) = observer.filter(|o| *o != Observer::Untracked) {
                let mut graph = self.graph.borrow_mut();
                graph
                    .foreign
                    .entry(slot)
                    .or_default()
                    .insert((reader.id, observer));
                drop(graph);
                let mut graph = reader.graph.borrow_mut();
                graph
                    .foreign_reads
                    .entry(observer)
                    .or_default()
                    .push((self.id, slot));
            }
            return;
        }
        let mut graph = self.graph.borrow_mut();
        if let Some(&observer) = graph.observing.last() {
            if observer == Observer::Untracked {
                return;
            }
            graph.subscribers[slot].insert(observer);
            graph.reads.entry(observer).or_default().push(slot);
        }
    }

    /// Unsubscribe `observer`, of the runtime `id` whose graph this is,
    /// from all it read, here and in other apps.
    fn unsubscribe(graph: &mut Graph, id: u32, observer: Observer) {
        if let Some(old) = graph.reads.remove(&observer) {
            for slot in old {
                graph.subscribers[slot].remove(&observer);
            }
        }
        if let Some(old) = graph.foreign_reads.remove(&observer) {
            for (other, slot) in old {
                if let Some(other) = Runtime::try_get(other) {
                    if let Some(readers) = other.graph.borrow_mut().foreign.get_mut(&slot) {
                        readers.remove(&(id, observer));
                    }
                }
            }
        }
    }

    /// Run `f` as `observer`, so it ends up subscribed to exactly what it
    /// read.
    fn observe<R>(&self, observer: Observer, f: impl FnOnce() -> R) -> R {
        {
            let mut graph = self.graph.borrow_mut();
            Runtime::unsubscribe(&mut graph, self.id, observer);
            graph.observing.push(observer);
        }
        let result = f();
        self.graph.borrow_mut().observing.pop();
        result
    }

    /// Run `f` without subscribing anyone to what it reads.
    pub(crate) fn untracked<R>(&self, f: impl FnOnce() -> R) -> R {
        self.graph.borrow_mut().observing.push(Observer::Untracked);
        let result = f();
        self.graph.borrow_mut().observing.pop();
        result
    }

    pub(crate) fn observe_node<R>(&self, node: NodeId, f: impl FnOnce() -> R) -> R {
        self.observe(Observer::Node(node), f)
    }

    /// [`observe_node`](Self::observe_node), adding to what the node
    /// already reads rather than starting afresh.
    pub(crate) fn observe_node_more<R>(&self, node: NodeId, f: impl FnOnce() -> R) -> R {
        self.graph.borrow_mut().observing.push(Observer::Node(node));
        let result = f();
        self.graph.borrow_mut().observing.pop();
        result
    }

    /// Slot `slot` changed: mark its node readers dirty and bring its memo
    /// readers up to date, following on from the memos whose value changed.
    /// Inside an update the change waits until the outermost one is done,
    /// so no reader computes while a signal's value is out of its slot.
    fn changed(&self, slot: usize) {
        self.queued.borrow_mut().push(slot);
        self.pass_changes_on();
    }

    /// Reach the readers of the slots changed meanwhile, unless inside an
    /// update (whose end does it).
    fn pass_changes_on(&self) {
        if self.batch.get() > 0 {
            return;
        }
        self.batch.set(1);
        loop {
            let next = self.queued.borrow_mut().pop();
            let Some(slot) = next else { break };
            let readers: Vec<Observer> = self.graph.borrow().subscribers[slot]
                .iter()
                .copied()
                .collect();
            for reader in readers {
                self.reached(reader);
            }
            // Readers in other apps, each in its own runtime.
            let foreign: Vec<(u32, Observer)> = self
                .graph
                .borrow()
                .foreign
                .get(&slot)
                .map(|readers| readers.iter().copied().collect())
                .unwrap_or_default();
            for (other, reader) in foreign {
                if let Some(other) = Runtime::try_get(other) {
                    other.enter(|| other.reached(reader));
                }
            }
        }
        self.batch.set(0);
    }

    /// A value `reader` read changed.
    fn reached(&self, reader: Observer) {
        match reader {
            Observer::Node(node) => {
                let mut graph = self.graph.borrow_mut();
                // A node that kept changing what it reads while it draws
                // is not drawn again for its own writes.
                let drawing = graph.observing.contains(&reader);
                if !(drawing && graph.quiet.contains(&node)) {
                    graph.dirty.insert(node);
                }
                self.tracer.reached(node);
            }
            Observer::Memo(memo) => {
                if self.recompute(memo) {
                    self.changed(memo);
                }
            }
            Observer::Watch(watch) => {
                let mut graph = self.graph.borrow_mut();
                if !graph.pending.contains(&watch) {
                    graph.pending.push(watch);
                }
            }
            Observer::Untracked => {}
        }
    }

    /// Writing a signal is not for a memo: it computes a value from others,
    /// and is recomputed whenever they change, so a write there would set
    /// it off again without end.
    fn check_write(&self) {
        let computing = |runtime: &Runtime| {
            runtime
                .graph
                .borrow()
                .observing
                .iter()
                .any(|o| matches!(o, Observer::Memo(_)))
        };
        let current = CURRENT.with(|current| current.borrow().last().cloned());
        if computing(self) || current.is_some_and(|c| computing(&c)) {
            panic!(
                "a signal was written while a memo computes: a memo (or a watch's \
                 source) only computes a value from others. Write signals in an \
                 event handler, a watch's callback or a task"
            );
        }
    }

    /// Recompute memo `slot`; whether its value changed.
    fn recompute(&self, slot: usize) -> bool {
        let (compute, equal) = {
            let graph = self.graph.borrow();
            let def = &graph.memos[&slot];
            (def.compute.clone(), def.equal)
        };
        let value = self.observe(Observer::Memo(slot), || compute());
        let mut values = self.values.borrow_mut();
        if equal(&*values[slot], &*value) {
            return false;
        }
        values[slot] = value;
        true
    }

    /// Run the watches whose sources changed, and the ones they set off in
    /// turn; whether any ran.
    pub(crate) fn run_watches(&self, cx: &mut Ctx) -> bool {
        let mut ran = false;
        // A watch that keeps setting itself off stops after this many
        // rounds, so a cycle cannot hang the app; what is still pending is
        // dropped (not run next time), and a toast says so.
        for round in 0..=WATCH_ROUNDS {
            if round == WATCH_ROUNDS {
                let stuck = std::mem::take(&mut self.graph.borrow_mut().pending);
                if !stuck.is_empty() {
                    cx.toast_for(
                        format!(
                            "[bold red]A watch kept setting itself off[/]: stopped after \
                             {WATCH_ROUNDS} rounds. Its callback changes what its source reads."
                        ),
                        std::time::Duration::from_secs(8),
                    );
                }
                break;
            }
            let pending = std::mem::take(&mut self.graph.borrow_mut().pending);
            if pending.is_empty() {
                break;
            }
            for id in pending {
                let watch = self.watches.borrow_mut().remove(&id);
                if let Some(mut watch) = watch {
                    if self.tracer.on() {
                        let at = self.watch_sites.borrow().get(&id).copied();
                        self.tracer.push(crate::inspect::Traced::Watch(at));
                    }
                    self.observe(Observer::Watch(id), || watch(cx));
                    self.watches.borrow_mut().insert(id, watch);
                    ran = true;
                }
            }
        }
        ran
    }

    /// Slot `slot` was written: keep the value for its [`on_write`]
    /// watches, and set them off.
    fn wrote(&self, slot: usize) {
        let keeps = self.writes.borrow().get(&slot).cloned();
        let Some(keeps) = keeps else { return };
        {
            let values = self.values.borrow();
            for (_, keep) in &keeps {
                keep(&*values[slot]);
            }
        }
        let mut graph = self.graph.borrow_mut();
        for (id, _) in keeps {
            if !graph.pending.contains(&id) {
                graph.pending.push(id);
            }
        }
    }

    /// Whether watches wait to run: their sources changed since they last
    /// ran (a frame's layout moved a rect they read).
    pub(crate) fn has_pending_watches(&self) -> bool {
        !self.graph.borrow().pending.is_empty()
    }

    /// Stop the watch `id`.
    pub(crate) fn drop_watch(&self, id: usize) {
        self.watches.borrow_mut().remove(&id);
        for keeps in self.writes.borrow_mut().values_mut() {
            keeps.retain(|(watch, _)| *watch != id);
        }
        self.watch_sites.borrow_mut().remove(&id);
        let mut graph = self.graph.borrow_mut();
        Runtime::unsubscribe(&mut graph, self.id, Observer::Watch(id));
        graph.pending.retain(|w| *w != id);
    }

    /// Take the nodes made dirty since the last call.
    pub(crate) fn take_dirty(&self) -> HashSet<NodeId> {
        std::mem::take(&mut self.graph.borrow_mut().dirty)
    }

    pub(crate) fn has_dirty(&self) -> bool {
        !self.graph.borrow().dirty.is_empty()
    }

    /// Mark a node dirty by hand (a hosted component handled an event).
    pub(crate) fn mark_dirty(&self, node: NodeId) {
        self.graph.borrow_mut().dirty.insert(node);
    }

    /// The nodes dirty now.
    pub(crate) fn dirty_now(&self) -> Vec<NodeId> {
        self.graph.borrow().dirty.iter().copied().collect()
    }

    /// Stop drawing `node` again for its own writes while it draws.
    pub(crate) fn quiet(&self, node: NodeId) {
        let mut graph = self.graph.borrow_mut();
        graph.dirty.remove(&node);
        graph.quiet.insert(node);
    }

    /// Forget a node that left the tree.
    pub(crate) fn forget(&self, node: NodeId) {
        let mut graph = self.graph.borrow_mut();
        Runtime::unsubscribe(&mut graph, self.id, Observer::Node(node));
        graph.quiet.remove(&node);
        graph.dirty.remove(&node);
        self.watchers.hover.borrow_mut().remove(&node);
        self.watchers.pointer.borrow_mut().remove(&node);
    }

    /// Run the closures other threads sent; whether there were any.
    pub(crate) fn run_inbox(&self, cx: &mut Ctx) -> bool {
        let mut ran = false;
        while let Ok(job) = self.inbox.try_recv() {
            job(cx);
            ran = true;
        }
        ran
    }

    pub(crate) fn proxy(&self) -> Proxy {
        Proxy {
            outbox: self.outbox.clone(),
            wake: self.wake.clone(),
        }
    }
}

/// A reactive value. `Copy`, so move it into as many closures as you like.
///
/// A signal is a handle; its value lives on the app's thread. A handle may
/// travel to another thread inside a [`Proxy`] closure, which runs back on
/// the app's thread. Read or write it there, never on the other thread.
pub struct Signal<T> {
    runtime: u32,
    slot: usize,
    _type: PhantomData<fn() -> T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

impl<T> PartialEq for Signal<T> {
    /// The same signal.
    fn eq(&self, other: &Self) -> bool {
        (self.runtime, self.slot) == (other.runtime, other.slot)
    }
}

impl<T> Eq for Signal<T> {}

impl<T> std::fmt::Debug for Signal<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Signal({}:{})", self.runtime, self.slot)
    }
}

/// A new signal holding `value`, in the current app. The
/// [inspector](crate::inspect)'s trace names it by where it was made.
#[track_caller]
pub fn signal<T: 'static>(value: T) -> Signal<T> {
    let runtime = Runtime::current_for("signal()");
    Signal {
        runtime: runtime.id,
        slot: runtime.insert(Box::new(value), Some(Location::caller())),
        _type: PhantomData,
    }
}

impl<T: 'static> Signal<T> {
    fn runtime(&self) -> Rc<Runtime> {
        Runtime::get(self.runtime)
    }

    /// Leave this signal's writes out of the inspector's trace: one the
    /// framework writes for itself (the focus, the hover), whose changes
    /// the trace says otherwise.
    pub(crate) fn untraced(self) -> Signal<T> {
        self.runtime().tracer.quiet(self.slot);
        self
    }

    /// Read the value through `f`. A node drawing (or a memo computing)
    /// subscribes to it. `f` must not write signals.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let runtime = self.runtime();
        runtime.subscribe(self.slot);
        let values = runtime.values.borrow();
        f(values[self.slot].downcast_ref::<T>().expect(SELF_READ))
    }

    /// Read without subscribing.
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let runtime = self.runtime();
        let values = runtime.values.borrow();
        f(values[self.slot].downcast_ref::<T>().expect(SELF_READ))
    }

    /// Change the value in place; every reader updates. `f` may read and
    /// write other signals (not read this one, whose value it holds);
    /// readers see all the changes once the outermost update is done.
    ///
    /// # Panics
    /// While a memo computes: a memo only computes a value from others.
    #[track_caller]
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let runtime = self.runtime();
        runtime.check_write();
        // While the inspector traces: the nodes this write reaches. An
        // update inside another's closure reaches them with the outer one.
        let traced = runtime
            .tracer
            .on()
            .then(|| runtime.tracer.start_write(runtime.batch.get() == 0));
        runtime.batch.set(runtime.batch.get() + 1);
        // Out of the table while `f` runs, so the table is not borrowed.
        let mut value = std::mem::replace(
            &mut runtime.values.borrow_mut()[self.slot],
            Box::new(Updating),
        );
        f(value.downcast_mut::<T>().expect("signal type"));
        runtime.values.borrow_mut()[self.slot] = value;
        runtime.batch.set(runtime.batch.get() - 1);
        runtime.wrote(self.slot);
        runtime.changed(self.slot);
        if let Some(outer) = traced {
            runtime.tracer.end_write(crate::inspect::Write {
                slot: self.slot,
                ty: std::any::type_name::<T>(),
                made: runtime.site(self.slot),
                at: Location::caller(),
                outer,
            });
        }
    }
}

impl<T: 'static> Signal<T> {
    /// Lend the value to `f`, which may write other signals meanwhile (not
    /// this one, nor read it): it is out of the table while `f` runs, and
    /// what `f` changes reaches its readers after.
    pub(crate) fn lend<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let runtime = self.runtime();
        runtime.batch.set(runtime.batch.get() + 1);
        let value = std::mem::replace(
            &mut runtime.values.borrow_mut()[self.slot],
            Box::new(Updating),
        );
        let result = f(value.downcast_ref::<T>().expect("signal type"));
        runtime.values.borrow_mut()[self.slot] = value;
        runtime.batch.set(runtime.batch.get() - 1);
        runtime.pass_changes_on();
        result
    }

    /// Replace the value and return the old one. Every reader updates,
    /// whether the value changed or not: [`set`](Self::set) for a type
    /// without `PartialEq` (a handle, a closure, a list of watchers).
    ///
    /// # Panics
    /// While a memo computes, as [`update`](Self::update).
    #[track_caller]
    pub fn replace(&self, value: T) -> T {
        let mut new = Some(value);
        let mut old = None;
        self.update(|slot| old = new.take().map(|new| std::mem::replace(slot, new)));
        old.expect("the update ran")
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// The value (a clone), subscribing the reader.
    pub fn get(&self) -> T {
        self.with(T::clone)
    }

    /// The value (a clone), without subscribing.
    pub fn get_untracked(&self) -> T {
        self.with_untracked(T::clone)
    }
}

impl<T: PartialEq + 'static> Signal<T> {
    /// Replace the value. Readers update only if it changed.
    #[track_caller]
    pub fn set(&self, value: T) {
        if self.with_untracked(|old| *old != value) {
            self.update(|slot| *slot = value);
        }
    }
}

/// How many rounds of watches setting off watches run before the rest are
/// dropped.
const WATCH_ROUNDS: usize = 100;

/// The panic for a signal read while its own update holds its value.
const SELF_READ: &str = "a signal was read inside its own update(): read it before \
     the update, or use the value the update's closure is given";

/// Stands in a signal's slot while [`Signal::update`] holds its value.
struct Updating;

/// A value derived from signals (or other memos), recomputed when they
/// change. Readers update only when the result is different.
pub struct Memo<T> {
    inner: Signal<T>,
}

impl<T> Clone for Memo<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Memo<T> {}

/// The width and height, in cells, that the current app lays out in: the
/// terminal's columns, less the [inspector](crate::App::inspector) while it
/// is shown, and its rows (an [inline](crate::App::inline) app's region).
/// It changes when the terminal is resized, and a node that reads it draws
/// again then, so a closure can reflow on it:
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::viewport;
///
/// let app = App::new(|| {
///     let size = viewport();
///     text(move || {
///         let (columns, _) = size.get();
///         if columns < 40 { "narrow".into() } else { "wide".into() }
///     })
///     .on_key("q", |cx| cx.quit())
/// });
/// assert_eq!(app.render_with(&["q"], 30, 1).unwrap()[0].trim_end(), "narrow");
/// ```
///
/// [`Node::hide_below`](crate::Node::hide_below) and a stylesheet's
/// `@media` rules read it too.
pub fn viewport() -> Memo<(u16, u16)> {
    Memo {
        inner: Runtime::current_for("viewport()").viewport(),
    }
}

/// A memo computing `f`, in the current app.
pub fn memo<T: PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T> {
    let runtime = Runtime::current_for("memo()");
    let compute: Compute = Rc::new(move || Box::new(f()) as Box<dyn Any>);
    let equal: Equal = |a, b| a.downcast_ref::<T>() == b.downcast_ref::<T>();
    // Compute first with a placeholder slot, so the memo's reads are
    // recorded against its own slot.
    let slot = runtime.insert(Box::new(()), None);
    runtime.graph.borrow_mut().memos.insert(
        slot,
        MemoDef {
            compute: compute.clone(),
            equal,
        },
    );
    let value = runtime.observe(Observer::Memo(slot), || compute());
    runtime.values.borrow_mut()[slot] = value;
    Memo {
        inner: Signal {
            runtime: runtime.id,
            slot,
            _type: PhantomData,
        },
    }
}

/// Call `on_change` with the value of `source` whenever it changes, and
/// once at the start: load a preview when the selection moves, save when
/// the document changes, follow a signal with a task. `source` reads
/// signals (it is tracked like a node's drawing); `on_change` runs on the
/// app's thread between frames, with a [`Ctx`], and may write signals,
/// open screens or start tasks.
///
/// A watch made while a screen is built (in `App::new`'s closure or a
/// `push`ed screen's) stops when that screen closes; one made elsewhere
/// lasts as long as the app.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let count = signal(0);
///     let parity = signal("even");
///     watch(move || count.get() % 2, move |odd, _| {
///         parity.set(if odd == 1 { "odd" } else { "even" })
///     });
///     text!("{count} is {parity}")
///         .on_key("+", move |_| count.update(|c| *c += 1))
///         .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["+", "q"], 20, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "1 is odd");
/// ```
#[track_caller]
pub fn watch<T: PartialEq + Clone + 'static>(
    source: impl Fn() -> T + 'static,
    mut on_change: impl FnMut(T, &mut Ctx) + 'static,
) {
    let runtime = Runtime::current_for("watch()");
    let value = memo(source);
    let id = runtime.next_watch.get();
    runtime.next_watch.set(id + 1);
    runtime
        .watch_sites
        .borrow_mut()
        .insert(id, Location::caller());
    let mut last: Option<T> = None;
    runtime.watches.borrow_mut().insert(
        id,
        Box::new(move |cx: &mut Ctx| {
            // Reading the memo subscribes the watch to it.
            let now = value.get();
            if last.as_ref() != Some(&now) {
                last = Some(now.clone());
                on_change(now, cx);
            }
        }),
    );
    // The first run, with the first value, comes on the app's first turn.
    runtime.graph.borrow_mut().pending.push(id);
    crate::app::building_watch(id);
}

/// Call `on_write` with the value of `signal` after every write to it, in
/// turn: [`update`](Signal::update), [`replace`](Signal::replace), a
/// [`set`](Signal::set) that changed it, a [`latest`](crate::latest)
/// thread's delivery. Where [`watch`] runs when a value changes, and sees
/// two equal values as one, this sees each write: counting the frames a
/// thread sends when two of them can be equal (the silence of a sound
/// card). Not called at the start; like a watch's, the callback runs on
/// the app's thread between frames, with a [`Ctx`], and stops with the
/// screen it was made for.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let frame = signal(0);
///     let (changes, writes) = (signal(0), signal(0));
///     watch(move || frame.get(), move |_, _| changes.update(|n| *n += 1));
///     on_write(frame, move |_, _| writes.update(|n| *n += 1));
///     // Three writes of an equal value: one change (the start), three writes.
///     text!("{changes} {writes}").on_key("w", move |_| frame.update(|f| *f = 0))
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("w w w");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "1 3");
/// ```
///
/// # Panics
/// Outside a running or building app, or for a signal of another app.
#[track_caller]
pub fn on_write<T: Clone + 'static>(
    signal: Signal<T>,
    mut on_write: impl FnMut(T, &mut Ctx) + 'static,
) {
    let runtime = Runtime::current_for("on_write()");
    assert_eq!(
        signal.runtime, runtime.id,
        "on_write() follows a signal of the app it is called in"
    );
    let id = runtime.next_watch.get();
    runtime.next_watch.set(id + 1);
    runtime
        .watch_sites
        .borrow_mut()
        .insert(id, Location::caller());
    let written: Rc<RefCell<VecDeque<T>>> = Rc::default();
    let kept = written.clone();
    let keep: Keep = Rc::new(move |value: &dyn Any| {
        let value = value.downcast_ref::<T>().expect("signal type");
        kept.borrow_mut().push_back(value.clone());
    });
    runtime
        .writes
        .borrow_mut()
        .entry(signal.slot)
        .or_default()
        .push((id, keep));
    runtime.watches.borrow_mut().insert(
        id,
        Box::new(move |cx: &mut Ctx| loop {
            // Taken one at a time: the callback may write the signal again.
            let next = written.borrow_mut().pop_front();
            match next {
                Some(value) => on_write(value, cx),
                None => break,
            }
        }),
    );
    crate::app::building_watch(id);
}

/// [`on_write`] without cloning: `on_write` gets a borrow of `signal`'s
/// value, between frames, after a turn that wrote it (an update, a
/// [`latest`](crate::latest) delivery). The value is the signal's as it is
/// then: a turn that wrote it twice (two updates in one handler) calls back
/// once, with the last, where [`on_write`] keeps a clone of each. A feed
/// writes once a turn, so a big value (a buffer, a frame) is seen each
/// time it comes, and never copied. Not called at the start.
///
/// While `on_write` runs, the value is lent out of the signal: it may read
/// and write other signals, not this one.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::on_write_ref;
///
/// let app = App::new(|| {
///     let frame = signal(vec![0u8; 4096]);
///     let sum = signal(0u32);
///     on_write_ref(frame, move |bytes: &Vec<u8>, _| {
///         sum.set(bytes.iter().map(|b| *b as u32).sum())
///     });
///     text!("{sum}").on_key("w", move |_| frame.update(|f| f.fill(1)))
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("w");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "4096");
/// ```
///
/// # Panics
/// Outside a running or building app, or for a signal of another app.
#[track_caller]
pub fn on_write_ref<T: 'static>(
    signal: Signal<T>,
    mut on_write: impl FnMut(&T, &mut Ctx) + 'static,
) {
    let runtime = Runtime::current_for("on_write_ref()");
    assert_eq!(
        signal.runtime, runtime.id,
        "on_write_ref() follows a signal of the app it is called in"
    );
    let id = runtime.next_watch.get();
    runtime.next_watch.set(id + 1);
    runtime
        .watch_sites
        .borrow_mut()
        .insert(id, Location::caller());
    let written = Rc::new(Cell::new(false));
    let marked = written.clone();
    let keep: Keep = Rc::new(move |_: &dyn Any| marked.set(true));
    runtime
        .writes
        .borrow_mut()
        .entry(signal.slot)
        .or_default()
        .push((id, keep));
    runtime.watches.borrow_mut().insert(
        id,
        Box::new(move |cx: &mut Ctx| {
            if written.replace(false) {
                signal.lend(|value| on_write(value, cx));
            }
        }),
    );
    crate::app::building_watch(id);
}

/// A counter of the writes to `signal` (as [`on_write`] sees them), for
/// [`rate`](crate::rate): nothing is cloned. It stops counting with the
/// screen it was made for.
pub(crate) fn count_writes<T: 'static>(signal: Signal<T>) -> Rc<Cell<u32>> {
    let runtime = Runtime::current_for("rate()");
    assert_eq!(
        signal.runtime, runtime.id,
        "rate() follows a signal of the app it is called in"
    );
    let id = runtime.next_watch.get();
    runtime.next_watch.set(id + 1);
    let count = Rc::new(Cell::new(0u32));
    let counted = count.clone();
    let keep: Keep = Rc::new(move |_: &dyn Any| counted.set(counted.get().saturating_add(1)));
    runtime
        .writes
        .borrow_mut()
        .entry(signal.slot)
        .or_default()
        .push((id, keep));
    // Nothing to run between frames; a watch all the same, so it stops
    // with its screen.
    runtime
        .watches
        .borrow_mut()
        .insert(id, Box::new(|_: &mut Ctx| {}));
    crate::app::building_watch(id);
    count
}

impl<T: 'static> Memo<T> {
    /// `signal`, read only: a value the crate writes and its readers only
    /// read.
    pub(crate) fn of(signal: Signal<T>) -> Memo<T> {
        Memo { inner: signal }
    }

    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.inner.with(f)
    }

    /// Read without subscribing (in a handler, say).
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        self.inner.with_untracked(f)
    }
}

impl<T: Clone + 'static> Memo<T> {
    pub fn get(&self) -> T {
        self.inner.get()
    }

    /// The value (a clone), without subscribing.
    pub fn get_untracked(&self) -> T {
        self.inner.get_untracked()
    }
}

/// A handle another thread can use to change the app's state: the closure
/// runs on the app's thread, between frames, where it may write signals.
///
/// ```no_run
/// # use intuituive::prelude::*;
/// # let proxy: Proxy = unimplemented!();
/// # let status: Signal<String> = unimplemented!();
/// std::thread::spawn(move || {
///     let body = "fetched".to_string(); // slow work off the UI thread
///     proxy.run(move || status.set(body));
/// });
/// ```
#[derive(Clone)]
pub struct Proxy {
    outbox: mpsc::Sender<Job>,
    wake: Arc<WakeSlot>,
}

/// What wakes the app's loop from a wait on the terminal (the backend's
/// waker, set by [`Driver::set_waker`](crate::Driver::set_waker)), shared
/// by every [`Proxy`].
#[derive(Default)]
pub(crate) struct WakeSlot(std::sync::Mutex<Option<Waking>>);

/// Wakes the app's loop.
pub(crate) type Waking = Arc<dyn Fn() + Send + Sync>;

impl WakeSlot {
    pub(crate) fn set(&self, wake: Option<Waking>) {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = wake;
    }

    fn wake(&self) {
        let wake = self.0.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(wake) = wake {
            wake();
        }
    }
}

/// One run of a watch: read its source, call back if it changed.
type WatchStep = Box<dyn FnMut(&mut Ctx)>;

/// Starts a [`latest`](crate::latest) thread, once the app runs.
pub(crate) type StartThread = Box<dyn FnOnce()>;

/// Keeps a write's value (a clone) for an [`on_write`] watch.
type Keep = Rc<dyn Fn(&dyn Any)>;

/// A closure another thread sent to run on the app's thread.
pub(crate) type Job = Box<dyn FnOnce(&mut Ctx) + Send>;

impl Proxy {
    /// Run `f` on the app's thread. Returns `false` if the app has finished.
    #[track_caller]
    pub fn run(&self, f: impl FnOnce() + Send + 'static) -> bool {
        let at = Location::caller();
        self.send(Box::new(move |_: &mut Ctx| {
            traced(|| crate::inspect::Traced::Proxy(at));
            f()
        }))
    }

    /// Run `f` on the app's thread with a [`Ctx`], so it can also move the
    /// focus, open a screen or quit. Returns `false` if the app has
    /// finished.
    #[track_caller]
    pub fn run_with(&self, f: impl FnOnce(&mut Ctx) + Send + 'static) -> bool {
        let at = Location::caller();
        self.send(Box::new(move |cx: &mut Ctx| {
            traced(|| crate::inspect::Traced::Proxy(at));
            f(cx)
        }))
    }

    /// Send `job` as it is: a task's delivery, which traces itself. The
    /// loop's wait on the terminal ends, so it runs at once.
    pub(crate) fn send(&self, job: Job) -> bool {
        let sent = self.outbox.send(job).is_ok();
        if sent {
            self.wake.wake();
        }
        sent
    }
}

/// Run `f` subscribing no one to what it reads in the app running on this
/// thread (or as it is, outside one).
pub(crate) fn untracked<R>(f: impl FnOnce() -> R) -> R {
    match CURRENT.with(|current| current.borrow().last().cloned()) {
        Some(runtime) => runtime.untracked(f),
        None => f(),
    }
}

/// Note an event in the trace of the app running on this thread, while
/// its inspector is open; `event` is only made then.
pub(crate) fn traced(event: impl FnOnce() -> crate::inspect::Traced) {
    CURRENT.with(|current| {
        if let Some(runtime) = current.borrow().last().filter(|r| r.tracer.on()) {
            runtime.tracer.push(event());
        }
    });
}

thread_local! {
    static NEXT_NODE: Cell<NodeId> = const { Cell::new(1) };
}

/// A fresh node id.
pub(crate) fn next_node() -> NodeId {
    NEXT_NODE.with(|next| {
        let id = next.get();
        next.set(id + 1);
        id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_runtime<R>(f: impl FnOnce(&Rc<Runtime>) -> R) -> R {
        let runtime = Runtime::new();
        let result = runtime.enter(|| f(&runtime));
        runtime.close();
        result
    }

    #[test]
    fn an_update_may_read_other_signals() {
        with_runtime(|_| {
            let index = signal(1usize);
            let list = signal(vec![10, 20, 30]);
            // Reading `index` (and a memo over `list`'s neighbour) while
            // `list` is being written.
            list.update(|items| items[index.get()] += 1);
            index.update(|i| *i = list.with(|items| items.len()) - 1);
            assert_eq!(list.get(), [10, 21, 30]);
            assert_eq!(index.get(), 2);
        });
    }

    #[test]
    fn a_write_marks_exactly_its_readers_dirty() {
        with_runtime(|rt| {
            let a = signal(1);
            let b = signal("x".to_string());
            rt.observe_node(10, || a.get());
            rt.observe_node(11, || b.get());
            rt.observe_node(12, || (a.get(), b.get()));
            a.set(2);
            assert_eq!(rt.take_dirty(), HashSet::from([10, 12]));
            a.set(2);
            assert!(rt.take_dirty().is_empty(), "an equal value changes nothing");
        });
    }

    #[test]
    fn a_reader_that_stops_reading_stops_depending() {
        with_runtime(|rt| {
            let show = signal(true);
            let detail = signal(0);
            let draw = || {
                if show.get() {
                    detail.get();
                }
            };
            rt.observe_node(20, draw);
            show.set(false);
            rt.observe_node(20, draw);
            rt.take_dirty();
            detail.set(5);
            assert!(rt.take_dirty().is_empty());
        });
    }

    #[test]
    fn memos_notify_only_when_their_value_changes() {
        with_runtime(|rt| {
            let selected = signal(0usize);
            let rows: Vec<Memo<bool>> =
                (0..20).map(|i| memo(move || selected.get() == i)).collect();
            for (i, row) in rows.iter().enumerate() {
                rt.observe_node(100 + i as u64, || row.get());
            }
            selected.set(3);
            // Rows 0 (no longer selected) and 3 (now selected), not twenty.
            assert_eq!(rt.take_dirty(), HashSet::from([100, 103]));
            assert!(rows[3].get() && !rows[0].get());
        });
    }

    #[test]
    fn memos_chain() {
        with_runtime(|rt| {
            let n = signal(2);
            let square = memo(move || n.get() * n.get());
            let parity = memo(move || square.get() % 2 == 0);
            rt.observe_node(1, || parity.get());
            n.set(4); // 16: still even
            assert!(rt.take_dirty().is_empty());
            n.set(3); // 9: odd
            assert_eq!(rt.take_dirty(), HashSet::from([1]));
        });
    }

    #[test]
    fn a_proxy_runs_on_the_apps_thread() {
        with_runtime(|rt| {
            let status = signal(String::new());
            rt.observe_node(7, || status.get());
            let proxy = rt.proxy();
            std::thread::spawn(move || {
                let body = "fetched".to_string(); // slow work, off the UI thread
                proxy.run(move || status.set(body));
            })
            .join()
            .unwrap();
            assert!(
                rt.take_dirty().is_empty(),
                "nothing runs until the app drains"
            );
            assert!(rt.run_inbox(&mut crate::app::Ctx::new(rt.proxy())));
            assert_eq!(status.get_untracked(), "fetched");
            assert_eq!(rt.take_dirty(), HashSet::from([7]));
        });
    }
}

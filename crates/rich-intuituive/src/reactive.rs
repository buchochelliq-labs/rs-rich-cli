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
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
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
}

/// One app's reactive state.
pub(crate) struct Runtime {
    id: u32,
    values: RefCell<Vec<Box<dyn Any>>>,
    graph: RefCell<Graph>,
    /// Closures sent from other threads, run on the app's thread.
    inbox: mpsc::Receiver<Job>,
    outbox: mpsc::Sender<Job>,
    /// Tasks started and not yet delivered.
    pub(crate) tasks: Arc<AtomicUsize>,
    /// Each watch's step: read its source, and call back if it changed.
    watches: RefCell<HashMap<usize, WatchStep>>,
    next_watch: Cell<usize>,
    /// The nodes that asked about hover or the pointer; a node leaves when
    /// it is forgotten.
    pub(crate) watchers: crate::widget::Watchers,
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
                tasks: Arc::new(AtomicUsize::new(0)),
                watches: RefCell::new(HashMap::new()),
                next_watch: Cell::new(0),
                watchers: Default::default(),
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

    pub(crate) fn current() -> Rc<Runtime> {
        CURRENT.with(|current| {
            current.borrow().last().cloned().expect(
                "signal() and memo() are called while an app is built or running: \
                 inside App::new's closure, a node's closure or an event handler",
            )
        })
    }

    /// Run `f` with this runtime current.
    pub(crate) fn enter<R>(self: &Rc<Self>, f: impl FnOnce() -> R) -> R {
        CURRENT.with(|current| current.borrow_mut().push(self.clone()));
        let result = f();
        CURRENT.with(|current| current.borrow_mut().pop());
        result
    }

    /// Drop the runtime from the registry (the app is finished).
    pub(crate) fn close(&self) {
        RUNTIMES.with(|all| {
            if let Some(slot) = all.borrow_mut().get_mut(self.id as usize) {
                *slot = None;
            }
        });
    }

    fn insert(&self, value: Box<dyn Any>) -> usize {
        let mut values = self.values.borrow_mut();
        values.push(value);
        self.graph.borrow_mut().subscribers.push(HashSet::new());
        values.len() - 1
    }

    fn subscribe(&self, slot: usize) {
        let mut graph = self.graph.borrow_mut();
        if let Some(&observer) = graph.observing.last() {
            if observer == Observer::Untracked {
                return;
            }
            graph.subscribers[slot].insert(observer);
            graph.reads.entry(observer).or_default().push(slot);
        }
    }

    fn unsubscribe(graph: &mut Graph, observer: Observer) {
        if let Some(old) = graph.reads.remove(&observer) {
            for slot in old {
                graph.subscribers[slot].remove(&observer);
            }
        }
    }

    /// Run `f` as `observer`, so it ends up subscribed to exactly what it
    /// read.
    fn observe<R>(&self, observer: Observer, f: impl FnOnce() -> R) -> R {
        {
            let mut graph = self.graph.borrow_mut();
            Runtime::unsubscribe(&mut graph, observer);
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
    fn changed(&self, slot: usize) {
        let mut pending = vec![slot];
        while let Some(slot) = pending.pop() {
            let readers: Vec<Observer> = self.graph.borrow().subscribers[slot]
                .iter()
                .copied()
                .collect();
            for reader in readers {
                match reader {
                    Observer::Node(node) => {
                        self.graph.borrow_mut().dirty.insert(node);
                    }
                    Observer::Memo(memo) => {
                        if self.recompute(memo) {
                            pending.push(memo);
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
        // rounds, so a cycle cannot hang the app.
        for _ in 0..100 {
            let pending = std::mem::take(&mut self.graph.borrow_mut().pending);
            if pending.is_empty() {
                break;
            }
            for id in pending {
                let watch = self.watches.borrow_mut().remove(&id);
                if let Some(mut watch) = watch {
                    self.observe(Observer::Watch(id), || watch(cx));
                    self.watches.borrow_mut().insert(id, watch);
                    ran = true;
                }
            }
        }
        ran
    }

    /// Stop the watch `id`.
    pub(crate) fn drop_watch(&self, id: usize) {
        self.watches.borrow_mut().remove(&id);
        let mut graph = self.graph.borrow_mut();
        Runtime::unsubscribe(&mut graph, Observer::Watch(id));
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

    /// Forget a node that left the tree.
    pub(crate) fn forget(&self, node: NodeId) {
        let mut graph = self.graph.borrow_mut();
        Runtime::unsubscribe(&mut graph, Observer::Node(node));
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
        Proxy(self.outbox.clone())
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

/// A new signal holding `value`, in the current app.
pub fn signal<T: 'static>(value: T) -> Signal<T> {
    let runtime = Runtime::current();
    Signal {
        runtime: runtime.id,
        slot: runtime.insert(Box::new(value)),
        _type: PhantomData,
    }
}

impl<T: 'static> Signal<T> {
    fn runtime(&self) -> Rc<Runtime> {
        Runtime::get(self.runtime)
    }

    /// Read the value through `f`. A node drawing (or a memo computing)
    /// subscribes to it. `f` must not write signals.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let runtime = self.runtime();
        runtime.subscribe(self.slot);
        let values = runtime.values.borrow();
        f(values[self.slot]
            .downcast_ref::<T>()
            .expect("a signal read inside its own update"))
    }

    /// Read without subscribing.
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let runtime = self.runtime();
        let values = runtime.values.borrow();
        f(values[self.slot]
            .downcast_ref::<T>()
            .expect("a signal read inside its own update"))
    }

    /// Change the value in place; every reader updates. `f` may read other
    /// signals (not this one, whose value it holds).
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let runtime = self.runtime();
        // Out of the table while `f` runs, so the table is not borrowed.
        let mut value = std::mem::replace(
            &mut runtime.values.borrow_mut()[self.slot],
            Box::new(Updating),
        );
        f(value.downcast_mut::<T>().expect("signal type"));
        runtime.values.borrow_mut()[self.slot] = value;
        runtime.changed(self.slot);
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
    pub fn set(&self, value: T) {
        if self.with_untracked(|old| *old != value) {
            self.update(|slot| *slot = value);
        }
    }
}

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

/// A memo computing `f`, in the current app.
pub fn memo<T: PartialEq + 'static>(f: impl Fn() -> T + 'static) -> Memo<T> {
    let runtime = Runtime::current();
    let compute: Compute = Rc::new(move || Box::new(f()) as Box<dyn Any>);
    let equal: Equal = |a, b| a.downcast_ref::<T>() == b.downcast_ref::<T>();
    // Compute first with a placeholder slot, so the memo's reads are
    // recorded against its own slot.
    let slot = runtime.insert(Box::new(()));
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
pub fn watch<T: PartialEq + Clone + 'static>(
    source: impl Fn() -> T + 'static,
    mut on_change: impl FnMut(T, &mut Ctx) + 'static,
) {
    let runtime = Runtime::current();
    let value = memo(source);
    let id = runtime.next_watch.get();
    runtime.next_watch.set(id + 1);
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

impl<T: 'static> Memo<T> {
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
pub struct Proxy(mpsc::Sender<Job>);

/// One run of a watch: read its source, call back if it changed.
type WatchStep = Box<dyn FnMut(&mut Ctx)>;

/// A closure another thread sent to run on the app's thread.
type Job = Box<dyn FnOnce(&mut Ctx) + Send>;

impl Proxy {
    /// Run `f` on the app's thread. Returns `false` if the app has finished.
    pub fn run(&self, f: impl FnOnce() + Send + 'static) -> bool {
        self.0.send(Box::new(move |_: &mut Ctx| f())).is_ok()
    }

    /// Run `f` on the app's thread with a [`Ctx`], so it can also move the
    /// focus, open a screen or quit. Returns `false` if the app has
    /// finished.
    pub fn run_with(&self, f: impl FnOnce(&mut Ctx) + Send + 'static) -> bool {
        self.0.send(Box::new(f)).is_ok()
    }
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

//! Fine-grained reactive state: signals that know which tree nodes read them.
//!
//! A [`Signal`] is a copyable handle to a value in a thread-local store.
//! While a node renders, the tree sets it as the *observer*; every signal
//! the node reads with [`Signal::get`] or [`Signal::with`] records the node
//! as a subscriber. Writing the signal ([`Signal::set`], [`Signal::update`])
//! marks each subscriber dirty, and only dirty nodes render again. A node's
//! subscriptions are cleared and re-recorded on every render, so a branch
//! that stops reading a signal stops depending on it.
//!
//! This is the model of SolidJS and Leptos, cut down: no memos, no effects,
//! no scopes. It is enough to test whether invalidation by dependency pays
//! for itself against re-rendering everything (today's `rich-interact`) and
//! redrawing everything (ratatui).

use std::any::Any;
use std::cell::RefCell;
use std::collections::HashSet;
use std::marker::PhantomData;

/// A tree node's identity, as the reactive store sees it.
pub type NodeId = usize;

struct Slot {
    value: Box<dyn Any>,
    subscribers: HashSet<NodeId>,
}

#[derive(Default)]
struct Runtime {
    slots: Vec<Slot>,
    /// The node rendering now, whose reads are recorded.
    observer: Vec<NodeId>,
    /// Nodes a write invalidated since the tree last took them.
    dirty: HashSet<NodeId>,
    /// Which slots each node read in its last render, to unsubscribe it.
    reads: std::collections::HashMap<NodeId, Vec<usize>>,
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

/// A reactive value. Copy it into closures freely: it is an index.
pub struct Signal<T> {
    index: usize,
    _type: PhantomData<fn() -> T>,
}

impl<T> Clone for Signal<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Signal<T> {}

/// A new signal holding `value`.
pub fn signal<T: 'static>(value: T) -> Signal<T> {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        rt.slots.push(Slot {
            value: Box::new(value),
            subscribers: HashSet::new(),
        });
        Signal {
            index: rt.slots.len() - 1,
            _type: PhantomData,
        }
    })
}

impl<T: 'static> Signal<T> {
    /// Read the value through `f`, subscribing the rendering node.
    pub fn with<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            if let Some(&node) = rt.observer.last() {
                rt.slots[self.index].subscribers.insert(node);
                rt.reads.entry(node).or_default().push(self.index);
            }
            let value = rt.slots[self.index]
                .value
                .downcast_ref::<T>()
                .expect("signal type");
            // `f` must not touch signals: the store is borrowed. Node
            // renders only read, and they read through `with` one at a time.
            f(value)
        })
    }

    /// Read without subscribing: for event handlers, which react to input
    /// rather than to state.
    pub fn with_untracked<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        RUNTIME.with(|rt| {
            let rt = rt.borrow();
            f(rt.slots[self.index]
                .value
                .downcast_ref::<T>()
                .expect("signal type"))
        })
    }

    /// Change the value in place and invalidate every reader.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            let rt = &mut *rt;
            let slot = &mut rt.slots[self.index];
            f(slot.value.downcast_mut::<T>().expect("signal type"));
            rt.dirty.extend(slot.subscribers.iter().copied());
        })
    }
}

impl<T: Clone + 'static> Signal<T> {
    /// The value, subscribing the rendering node.
    pub fn get(&self) -> T {
        self.with(T::clone)
    }

    pub fn get_untracked(&self) -> T {
        self.with_untracked(T::clone)
    }
}

impl<T: PartialEq + 'static> Signal<T> {
    /// Replace the value; readers are invalidated only if it changed.
    pub fn set(&self, value: T) {
        let changed = self.with_untracked(|old| *old != value);
        if changed {
            self.update(|slot| *slot = value);
        }
    }
}

/// Run `render` with `node` as the observer, after dropping the node's old
/// subscriptions, so it ends up subscribed to exactly what it read.
pub fn observe<R>(node: NodeId, render: impl FnOnce() -> R) -> R {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        if let Some(old) = rt.reads.remove(&node) {
            for index in old {
                rt.slots[index].subscribers.remove(&node);
            }
        }
        rt.observer.push(node);
    });
    let result = render();
    RUNTIME.with(|rt| rt.borrow_mut().observer.pop());
    result
}

/// Take the set of nodes invalidated since the last call.
pub fn take_dirty() -> HashSet<NodeId> {
    RUNTIME.with(|rt| std::mem::take(&mut rt.borrow_mut().dirty))
}

/// Forget a node: drop its subscriptions (a keyed child that was removed).
pub fn forget(node: NodeId) {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        if let Some(old) = rt.reads.remove(&node) {
            for index in old {
                rt.slots[index].subscribers.remove(&node);
            }
        }
        rt.dirty.remove(&node);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_write_invalidates_exactly_the_readers() {
        let a = signal(1);
        let b = signal("x".to_string());
        observe(10, || a.get());
        observe(11, || b.get());
        observe(12, || (a.get(), b.get()));
        take_dirty();
        a.set(2);
        let dirty = take_dirty();
        assert_eq!(dirty, HashSet::from([10, 12]));
        // An equal value changes nothing.
        a.set(2);
        assert!(take_dirty().is_empty());
    }

    #[test]
    fn a_node_that_stops_reading_stops_depending() {
        let show = signal(true);
        let detail = signal(0);
        let render = || {
            if show.get() {
                detail.get();
            }
        };
        observe(20, render);
        show.set(false);
        take_dirty();
        observe(20, render);
        detail.set(5);
        assert!(take_dirty().is_empty());
    }
}

//! A set of tabs' states with one active: [`tab_set`], the model a
//! [`tabs`](crate::widgets::tabs) strip shows.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::node::Node;
use crate::reactive::{signal, Signal};

/// What makes a new tab's state.
type Make<T> = Rc<dyn Fn() -> T>;

/// Tabs' states, one per tab, with one active: a browser's tabs, an
/// editor's buffers, a file manager's directories (Bubble Tea apps and
/// Textual's `TabbedContent` keep a model like it). `Copy`: any handler
/// takes one. There is always one tab at least. Made by [`tab_set`]; a
/// [`tabs`](crate::widgets::tabs) strip shows it with [`tabs_for`].
///
/// A tab's state is anything: signals made for it (a cwd and its
/// history), or plain values changed with [`update`](Self::update).
pub struct TabSet<T: 'static> {
    tabs: Signal<Vec<T>>,
    active: Signal<usize>,
    make: Signal<Make<T>>,
    /// Each tab's identity, in the tabs' order: what [`scoped`](TabSet::scoped)
    /// values are kept by, through opens, closes and swaps.
    ids: Signal<Vec<u64>>,
    next_id: Signal<u64>,
}

impl<T> Clone for TabSet<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for TabSet<T> {}

/// A [`TabSet`] of one tab, its state made by `make`, which also makes
/// each tab [`open`](TabSet::open) adds. Call it while the app is built.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{tab_set, tabs_for};
///
/// let app = App::new(|| {
///     let pages = tab_set(|| String::from("new"));
///     column([
///         tabs_for(&pages, |page| page.clone()).fixed(1),
///         text(move || pages.with_active(|page| format!("on {page}"))),
///     ])
///     .on_key("t", move |_| {
///         pages.open();
///         pages.update_active(|page| *page = String::from("docs"));
///     })
///     .on_key("x", move |_| {
///         pages.close_active();
///     })
/// });
/// let mut driver = app.driver(20, 2);
/// driver.press("t");
/// assert_eq!(driver.screen().plain()[0].trim_end(), " new │ docs");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "on docs");
/// driver.press("x");
/// assert_eq!(driver.screen().plain()[1].trim_end(), "on new");
/// ```
pub fn tab_set<T: 'static>(make: impl Fn() -> T + 'static) -> TabSet<T> {
    let first = make();
    let make: Make<T> = Rc::new(make);
    TabSet {
        tabs: signal(vec![first]),
        active: signal(0),
        make: signal(make),
        ids: signal(vec![0]),
        next_id: signal(1),
    }
}

impl<T: 'static> TabSet<T> {
    /// The active tab's index, a signal: read it, or give it to a
    /// [`tabs`](crate::widgets::tabs) strip. Set it with
    /// [`switch`](Self::switch), which keeps it among the tabs.
    pub fn active(&self) -> Signal<usize> {
        self.active
    }

    /// Every tab's state, a signal.
    pub fn tabs(&self) -> Signal<Vec<T>> {
        self.tabs
    }

    /// How many tabs there are, subscribing.
    pub fn len(&self) -> usize {
        self.tabs.with(Vec::len)
    }

    /// Never: a set has one tab at least.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Open a tab after the active one, made by the set's `make`, and make
    /// it active; its index.
    pub fn open(&self) -> usize {
        let make = self.make.with_untracked(Rc::clone);
        self.open_with(make())
    }

    /// Open a tab of `state` after the active one, and make it active; its
    /// index.
    pub fn open_with(&self, state: T) -> usize {
        let at = (self.active.get_untracked() + 1).min(self.tabs.with_untracked(Vec::len));
        let id = self.next_id.get_untracked();
        self.next_id.set(id + 1);
        self.ids.update(|ids| ids.insert(at, id));
        self.tabs.update(|tabs| tabs.insert(at, state));
        self.active.set(at);
        at
    }

    /// Close tab `i`; the tab after it becomes active if it was (the one
    /// before, for the last). The last tab left is not closed: whether it
    /// was.
    pub fn close(&self, i: usize) -> bool {
        let len = self.tabs.with_untracked(Vec::len);
        if len <= 1 || i >= len {
            return false;
        }
        self.ids.update(|ids| {
            ids.remove(i);
        });
        self.tabs.update(|tabs| {
            tabs.remove(i);
        });
        let active = self.active.get_untracked();
        if active > i || active == len - 1 {
            self.active.set(active.saturating_sub(1));
        }
        true
    }

    /// Close the active tab, as [`close`](Self::close).
    pub fn close_active(&self) -> bool {
        self.close(self.active.get_untracked())
    }

    /// Make tab `to` active (kept among the tabs).
    pub fn switch(&self, to: usize) {
        let last = self.tabs.with_untracked(Vec::len) - 1;
        self.active.set(to.min(last));
    }

    /// Make the tab `by` places away active, wrapping round the ends:
    /// `cycle(1)` is the next tab, `cycle(-1)` the previous.
    pub fn cycle(&self, by: isize) {
        let len = self.tabs.with_untracked(Vec::len) as isize;
        let to = (self.active.get_untracked() as isize + by).rem_euclid(len);
        self.active.set(to as usize);
    }

    /// Move the active tab `by` places (left for less than 0), staying
    /// active, kept among the tabs (Yazi's tab swap).
    pub fn swap(&self, by: isize) {
        let last = self.tabs.with_untracked(Vec::len) as isize - 1;
        let from = self.active.get_untracked();
        let to = (from as isize + by).clamp(0, last) as usize;
        if to == from {
            return;
        }
        self.ids.update(|ids| {
            let id = ids.remove(from);
            ids.insert(to, id);
        });
        self.tabs.update(|tabs| {
            let tab = tabs.remove(from);
            tabs.insert(to, tab);
        });
        self.active.set(to);
    }

    /// Read the active tab's state through `f`, subscribing.
    pub fn with_active<R>(&self, f: impl FnOnce(&T) -> R) -> R {
        let at = self.active.get();
        self.tabs.with(|tabs| f(&tabs[at.min(tabs.len() - 1)]))
    }

    /// Change the active tab's state in place.
    pub fn update_active(&self, f: impl FnOnce(&mut T)) {
        let at = self.active.get_untracked();
        self.tabs.update(|tabs| {
            let last = tabs.len() - 1;
            f(&mut tabs[at.min(last)]);
        });
    }

    /// Change tab `i`'s state in place (nothing past the end).
    pub fn update(&self, i: usize, f: impl FnOnce(&mut T)) {
        self.tabs.update(|tabs| {
            if let Some(tab) = tabs.get_mut(i) {
                f(tab);
            }
        });
    }

    /// The active tab's identity, subscribing: a number that stays with the
    /// tab through opens, closes and swaps (unlike its index), and is never
    /// given to another tab. What [`scoped`](Self::scoped) values are kept
    /// by.
    pub fn active_id(&self) -> u64 {
        let at = self.active.get();
        self.ids.with(|ids| ids[at.min(ids.len() - 1)])
    }

    /// Every tab's identity, in the tabs' order, subscribing.
    pub fn ids(&self) -> Vec<u64> {
        self.ids.get()
    }

    /// A value per tab, beside the tabs' own states: the active tab's is
    /// [`Scoped::get`], and switching tabs switches it. `make` makes a tab's
    /// value the first time it is asked for (the tabs already open
    /// included), so it may make signals: a selection per tab, a scroll
    /// position, a draft. A closed tab's value goes. For a value made of
    /// signals, a `Copy` handle such as a
    /// [`MultiSelect`](crate::MultiSelect), `get` returns the active tab's
    /// handle; to keep one handle and switch what it holds instead, see
    /// [`MultiSelect::scoped`](crate::MultiSelect::scoped).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::tab_set;
    ///
    /// let app = App::new(|| {
    ///     let pages = tab_set(|| ());
    ///     // A draft per tab.
    ///     let draft = pages.scoped(|| signal(String::new()));
    ///     text(move || draft.get().get())
    ///         .on_key("a", move |_| draft.get().update(|d| d.push('a')))
    ///         .on_key("t", move |_| {
    ///             pages.open();
    ///         })
    ///         .on_key("1", move |_| pages.switch(0))
    /// });
    /// let mut driver = app.driver(10, 1);
    /// driver.press("a a t a");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "a");
    /// driver.press("1");
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "aa");
    /// ```
    pub fn scoped<S: Clone + 'static>(&self, make: impl Fn() -> S + 'static) -> Scoped<S> {
        Scoped {
            values: signal(Rc::new(RefCell::new(HashMap::new()))),
            version: signal(0),
            ids: self.ids,
            active: self.active,
            make: signal(Rc::new(make) as Make<S>),
        }
    }

    /// The tabs' titles, `title` of each state, as a closure for a
    /// [`tabs`](crate::widgets::tabs) strip; it reads the set (and whatever
    /// `title` reads).
    pub fn titles(&self, title: impl Fn(&T) -> String + 'static) -> impl Fn() -> Vec<String> {
        let tabs = self.tabs;
        move || tabs.with(|tabs| tabs.iter().map(&title).collect())
    }
}

/// A value per tab of a [`TabSet`], switched with the active tab: made by
/// [`TabSet::scoped`]. `Copy`.
pub struct Scoped<S: 'static> {
    /// The values by tab identity: not a signal of its own, so a value can
    /// be made while a node draws or a memo computes.
    values: Signal<Rc<RefCell<HashMap<u64, S>>>>,
    /// Bumped by [`update`](Scoped::update), for readers of a plain value.
    version: Signal<u64>,
    ids: Signal<Vec<u64>>,
    active: Signal<usize>,
    make: Signal<Make<S>>,
}

impl<S> Clone for Scoped<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Scoped<S> {}

impl<S: Clone + 'static> Scoped<S> {
    /// The active tab's value (a clone), made now if it has none,
    /// subscribing to which tab is active and to [`update`](Self::update)s.
    pub fn get(&self) -> S {
        let at = self.active.get();
        let id = self.ids.with(|ids| ids[at.min(ids.len() - 1)]);
        self.version.with(|_| ());
        self.of_id(id)
    }

    /// Tab `i`'s value (a clone), made now if it has none; `None` past the
    /// last tab.
    pub fn of(&self, i: usize) -> Option<S> {
        let id = self.ids.with(|ids| ids.get(i).copied())?;
        self.version.with(|_| ());
        Some(self.of_id(id))
    }

    /// Change the active tab's value in place (in an event handler or a
    /// watch: readers of [`get`](Self::get) update).
    pub fn update(&self, f: impl FnOnce(&mut S)) {
        let at = self.active.get_untracked();
        let id = self.ids.with_untracked(|ids| ids[at.min(ids.len() - 1)]);
        self.of_id(id);
        let values = self.values.with_untracked(Rc::clone);
        if let Some(value) = values.borrow_mut().get_mut(&id) {
            f(value);
        }
        self.version.update(|v| *v += 1);
    }

    /// The value of the tab whose identity is `id`, made if it has none;
    /// the values of tabs that have closed are dropped meanwhile.
    fn of_id(&self, id: u64) -> S {
        let values = self.values.with_untracked(Rc::clone);
        if let Some(value) = values.borrow().get(&id).cloned() {
            return value;
        }
        let make = self.make.with_untracked(Rc::clone);
        let value = crate::reactive::untracked(|| make());
        let open = self.ids.get_untracked();
        let mut values = values.borrow_mut();
        values.retain(|id, _| open.contains(id));
        values.insert(id, value.clone());
        value
    }
}

impl<T: Clone + 'static> TabSet<T> {
    /// The active tab's state (a clone), subscribing.
    pub fn current(&self) -> T {
        self.with_active(T::clone)
    }
}

/// A [`tabs`](crate::widgets::tabs) strip over `set`: a title for each tab
/// from `title`, the active one selected, and a click or a key on the strip
/// switching to its tab. It takes every option the strip does
/// ([`fit`](Node::fit), [`auto_hide`](Node::auto_hide), …).
pub fn tabs_for<T: 'static>(set: &TabSet<T>, title: impl Fn(&T) -> String + 'static) -> Node {
    crate::widgets::tabs(set.titles(title), set.active)
}

/// [`tabs_for`] with each tab's index (from 0) given to `title` too:
/// numbered tabs, as Yazi's ` 1 src `.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{tab_set, tabs_for_indexed};
///
/// let app = App::new(|| {
///     let set = tab_set(|| "src".to_string());
///     set.open_with("docs".to_string());
///     tabs_for_indexed(&set, |i, name| format!(" {} {name} ", i + 1))
/// });
/// let mut driver = app.driver(30, 1);
/// driver.settle();
/// let strip = driver.screen().plain()[0].clone();
/// assert!(strip.contains("1 src") && strip.contains("2 docs"), "{strip}");
/// ```
pub fn tabs_for_indexed<T: 'static>(
    set: &TabSet<T>,
    title: impl Fn(usize, &T) -> String + 'static,
) -> Node {
    let tabs = set.tabs;
    let titles = move || {
        tabs.with(|tabs| {
            tabs.iter()
                .enumerate()
                .map(|(i, tab)| title(i, tab))
                .collect()
        })
    };
    crate::widgets::tabs(titles, set.active)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` in an app's runtime, as a handler would.
    fn in_app(f: impl Fn() + 'static) {
        let app = crate::App::new(move || {
            f();
            crate::label("")
        });
        app.render_with(&[], 4, 1).unwrap();
    }

    #[test]
    fn closing_keeps_one_tab_and_an_active_one_in_range() {
        in_app(|| {
            let set = tab_set(|| 0);
            assert!(!set.close(0));
            set.open_with(1);
            set.open_with(2);
            assert_eq!(set.tabs().get_untracked(), [0, 1, 2]);
            assert_eq!(set.active().get_untracked(), 2);
            assert!(set.close(2));
            assert_eq!(set.active().get_untracked(), 1);
            set.switch(0);
            assert!(set.close(1));
            assert_eq!(set.active().get_untracked(), 0);
            assert_eq!(set.tabs().get_untracked(), [0]);
        });
    }

    #[test]
    fn swap_moves_the_active_tab_and_cycle_wraps() {
        in_app(|| {
            let set = tab_set(|| 'a');
            set.open_with('b');
            set.open_with('c');
            set.switch(0);
            set.swap(1);
            assert_eq!(set.tabs().get_untracked(), ['b', 'a', 'c']);
            assert_eq!(set.active().get_untracked(), 1);
            set.swap(-5);
            assert_eq!(set.tabs().get_untracked(), ['a', 'b', 'c']);
            set.cycle(-1);
            assert_eq!(set.active().get_untracked(), 2);
            set.cycle(1);
            assert_eq!(set.active().get_untracked(), 0);
            // A new tab goes after the active one.
            set.open_with('z');
            assert_eq!(set.tabs().get_untracked(), ['a', 'z', 'b', 'c']);
        });
    }
}

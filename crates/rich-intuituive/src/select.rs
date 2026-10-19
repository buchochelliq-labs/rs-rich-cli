//! Picking items of a list: [`multi_select`], a selection by key with a
//! visual range, and [`list_search`], find next and previous over a list's
//! items.

use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use crate::node::Node;
use crate::reactive::{signal, Signal};

/// The keys of a list's items, in order.
type Keys<K> = Rc<dyn Fn() -> Vec<K>>;

/// A visual range: where it started, and whether it unselects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Visual {
    anchor: usize,
    unset: bool,
}

/// The keys chosen, in the order they were, with a set to look them up.
struct Chosen<K> {
    order: Vec<K>,
    set: HashSet<K>,
}

impl<K: Clone + Eq + Hash> Chosen<K> {
    fn insert(&mut self, key: K) {
        if self.set.insert(key.clone()) {
            self.order.push(key);
        }
    }

    fn remove(&mut self, key: &K) {
        if self.set.remove(key) {
            self.order.retain(|k| k != key);
        }
    }
}

/// How an item stands in a [`MultiSelect`], for its marker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mark {
    /// Not selected.
    None,
    /// Selected.
    Selected,
    /// In a visual range that selects.
    Visual,
    /// In a visual range that unselects (it was selected).
    Unset,
}

/// Items of a list picked by key, as a file manager's selection or a mail
/// client's: a set of keys that outlives the list's order (a re-sort, a
/// filter), and a visual range from an anchor to the list's cursor, as
/// vim's and Yazi's visual mode. `Copy`: any handler takes one. Made by
/// [`multi_select`].
///
/// - Picking: [`toggle`](Self::toggle), [`select`](Self::select),
///   [`unselect`](Self::unselect), [`clear`](Self::clear); over the
///   items ([`over`](Self::over)) [`select_all`](Self::select_all) and
///   [`invert`](Self::invert).
/// - A visual range ([`range_from`](Self::range_from) a cursor):
///   [`visual`](Self::visual) anchors it at the cursor, it follows the
///   cursor, and [`end_visual`](Self::end_visual) keeps what it covers
///   (or [`cancel_visual`](Self::cancel_visual) drops it).
/// - Reading: [`selected`](Self::selected), [`targets`](Self::targets)
///   (the selection, else the hovered item: what an action acts on),
///   [`mark`](Self::mark) for one key; a list draws the marks with
///   [`Node::selection_markers`].
pub struct MultiSelect<K: 'static> {
    chosen: Signal<Chosen<K>>,
    visual: Signal<Option<Visual>>,
    keys: Signal<Option<Keys<K>>>,
    cursor: Signal<Option<Signal<usize>>>,
}

impl<K> Clone for MultiSelect<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for MultiSelect<K> {}

/// A [`MultiSelect`] with nothing selected, over no items yet: give it
/// the list's keys with [`over`](MultiSelect::over) and its cursor with
/// [`range_from`](MultiSelect::range_from). Call it while the app is
/// built.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::multi_select;
///
/// let app = App::new(|| {
///     let names = signal(vec!["a.txt", "b.txt", "c.txt", "d.txt"]);
///     let cursor = signal(0usize);
///     let picked = multi_select::<String>()
///         .over(move || names.get().iter().map(|n| n.to_string()).collect())
///         .range_from(cursor);
///     column([
///         list(move || names.get().iter().map(|n| n.to_string()).collect(), cursor)
///             .selection_markers(picked, move |i| names.with(|n| n[i].to_string()))
///             .fixed(4),
///         text(move || picked.selected().join(" ")),
///     ])
///     .on_key("space", move |_| {
///         picked.toggle(names.with(|n| n[cursor.get_untracked()].to_string()))
///     })
///     .on_key("v", move |_| picked.visual(false))
///     .on_key("escape", move |_| picked.end_visual())
/// });
/// let mut driver = app.driver(20, 5);
/// driver.press("space");
/// driver.press("j");
/// driver.press("j");
/// driver.press("v");
/// driver.press("j");
/// driver.press("escape");
/// assert_eq!(driver.screen().plain()[4].trim_end(), "a.txt c.txt d.txt");
/// ```
pub fn multi_select<K: Clone + Eq + Hash + 'static>() -> MultiSelect<K> {
    MultiSelect {
        chosen: signal(Chosen {
            order: Vec::new(),
            set: HashSet::new(),
        }),
        visual: signal(None),
        keys: signal(None),
        cursor: signal(None),
    }
}

impl<K: Clone + Eq + Hash + 'static> MultiSelect<K> {
    /// The list's items, by key, in the list's order (may read signals):
    /// what [`select_all`](Self::select_all), [`invert`](Self::invert) and
    /// a visual range go over.
    pub fn over(self, keys: impl Fn() -> Vec<K> + 'static) -> Self {
        let keys: Keys<K> = Rc::new(keys);
        self.keys.replace(Some(keys));
        self
    }

    /// The cursor a visual range runs to: the list's selected index.
    pub fn range_from(self, cursor: Signal<usize>) -> Self {
        self.cursor.set(Some(cursor));
        self
    }

    /// The items' keys now, subscribing; none without [`over`](Self::over).
    fn keys(&self) -> Vec<K> {
        let keys = self.keys.with(Clone::clone);
        keys.map(|keys| keys()).unwrap_or_default()
    }

    /// Select `key`, or unselect it if it is selected.
    pub fn toggle(&self, key: K) {
        self.chosen.update(|chosen| {
            if chosen.set.contains(&key) {
                chosen.remove(&key);
            } else {
                chosen.insert(key);
            }
        });
    }

    /// Select `key`.
    pub fn select(&self, key: K) {
        if !self.chosen.with_untracked(|c| c.set.contains(&key)) {
            self.chosen.update(|chosen| chosen.insert(key));
        }
    }

    /// Unselect `key`.
    pub fn unselect(&self, key: &K) {
        if self.chosen.with_untracked(|c| c.set.contains(key)) {
            self.chosen.update(|chosen| chosen.remove(key));
        }
    }

    /// Select every item.
    pub fn select_all(&self) {
        let keys = self.untracked_keys();
        self.chosen
            .update(|chosen| keys.into_iter().for_each(|k| chosen.insert(k)));
    }

    /// Select the items not selected, and unselect the rest. Keys selected
    /// that are not among the items stay selected.
    pub fn invert(&self) {
        let keys = self.untracked_keys();
        self.chosen.update(|chosen| {
            for key in keys {
                if chosen.set.contains(&key) {
                    chosen.remove(&key);
                } else {
                    chosen.insert(key);
                }
            }
        });
    }

    /// Unselect everything, and end a visual range without keeping it.
    pub fn clear(&self) {
        self.visual.set(None);
        self.chosen.update(|chosen| {
            chosen.order.clear();
            chosen.set.clear();
        });
    }

    fn untracked_keys(&self) -> Vec<K> {
        crate::reactive::Runtime::current().untracked(|| self.keys())
    }

    /// Start a visual range at the cursor ([`range_from`](Self::range_from)):
    /// the items from there to wherever the cursor goes are selected, or,
    /// with `unset`, unselected (Yazi's `v` and `V`). Without a cursor it
    /// does nothing.
    pub fn visual(&self, unset: bool) {
        if let Some(cursor) = self.cursor.get_untracked() {
            let anchor = cursor.get_untracked();
            self.visual.set(Some(Visual { anchor, unset }));
        }
    }

    /// Whether a visual range is going on, subscribing.
    pub fn in_visual(&self) -> bool {
        self.visual.with(Option::is_some)
    }

    /// Whether the visual range going on unselects.
    pub fn visual_unsets(&self) -> bool {
        self.visual.with(|v| v.is_some_and(|v| v.unset))
    }

    /// End the visual range, keeping what it did: its items selected (or
    /// unselected).
    pub fn end_visual(&self) {
        let Some(visual) = self.visual.get_untracked() else {
            return;
        };
        let range = crate::reactive::Runtime::current().untracked(|| self.range());
        self.visual.set(None);
        self.chosen.update(|chosen| {
            for key in range {
                if visual.unset {
                    chosen.remove(&key);
                } else {
                    chosen.insert(key);
                }
            }
        });
    }

    /// End the visual range, leaving the selection as it was before it.
    pub fn cancel_visual(&self) {
        self.visual.set(None);
    }

    /// The keys the visual range covers now, in the items' order (none
    /// outside one), subscribing.
    pub fn range(&self) -> Vec<K> {
        let Some(visual) = self.visual.get() else {
            return Vec::new();
        };
        let Some(cursor) = self.cursor.get() else {
            return Vec::new();
        };
        let keys = self.keys();
        if keys.is_empty() {
            return Vec::new();
        }
        let last = keys.len() - 1;
        let (a, b) = (visual.anchor.min(last), cursor.get().min(last));
        keys[a.min(b)..=a.max(b)].to_vec()
    }

    /// How `key` stands, subscribing: for a marker.
    pub fn mark(&self, key: &K) -> Mark {
        let in_range = self.visual.with(Option::is_some) && self.range().contains(key);
        self.mark_with(key, in_range)
    }

    /// How item `i`, whose key is `key`, stands, subscribing: the range
    /// by index, without reading every key (a marker for each row drawn).
    fn mark_at(&self, i: usize, key: &K) -> Mark {
        let in_range = match (self.visual.get(), self.cursor.get()) {
            (Some(visual), Some(cursor)) => {
                let at = cursor.get();
                (visual.anchor.min(at)..=visual.anchor.max(at)).contains(&i)
            }
            _ => false,
        };
        self.mark_with(key, in_range)
    }

    fn mark_with(&self, key: &K, in_range: bool) -> Mark {
        let chosen = self.chosen.with(|c| c.set.contains(key));
        let visual = self.visual.get();
        match (visual, in_range, chosen) {
            (Some(v), true, true) if v.unset => Mark::Unset,
            (Some(v), true, false) if !v.unset => Mark::Visual,
            (_, _, true) => Mark::Selected,
            _ => Mark::None,
        }
    }

    /// Whether `key` is selected now, a visual range counted, subscribing.
    pub fn contains(&self, key: &K) -> bool {
        matches!(self.mark(key), Mark::Selected | Mark::Visual)
    }

    /// The keys selected now, a visual range counted, subscribing: those
    /// among the items in their order, then the others in the order they
    /// were selected.
    pub fn selected(&self) -> Vec<K> {
        let range: HashSet<K> = self.range().into_iter().collect();
        let unset = self.visual.with(|v| v.is_some_and(|v| v.unset));
        let keys = self.keys();
        self.chosen.with(|chosen| {
            let shown: HashSet<&K> = keys.iter().collect();
            let selected = |key: &K| {
                let ranged = range.contains(key);
                (chosen.set.contains(key) && !(unset && ranged)) || (ranged && !unset)
            };
            let mut out: Vec<K> = keys.iter().filter(|k| selected(k)).cloned().collect();
            out.extend(chosen.order.iter().filter(|k| !shown.contains(k)).cloned());
            out
        })
    }

    /// How many keys are selected now, subscribing.
    pub fn len(&self) -> usize {
        self.selected().len()
    }

    /// Whether nothing is selected now, subscribing.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// What an action acts on: the selection, or else the `hovered` item
    /// alone (none without one). Yazi's yank, cut and delete.
    pub fn targets(&self, hovered: Option<K>) -> Vec<K> {
        let selected = self.selected();
        if selected.is_empty() {
            hovered.into_iter().collect()
        } else {
            selected
        }
    }
}

impl Node {
    /// On a [`list`](crate::list): draw `selection`'s marks in the
    /// one-cell marker column, item `i` being `key(i)`: a selected item in
    /// the theme's `marker.selected` style (yellow), one a visual range
    /// covers in `marker.visual` (cyan), and a selected one a range
    /// unselects in `marker.unset` (red). A [`marker`](Self::marker) shows
    /// where an item has no mark (Yazi's copied and cut markers).
    pub fn selection_markers<K: Clone + Eq + Hash + 'static>(
        self,
        selection: MultiSelect<K>,
        key: impl Fn(usize) -> K + 'static,
    ) -> Node {
        let marks: crate::list::Marks = Rc::new(move |i| match selection.mark_at(i, &key(i)) {
            Mark::None => None,
            Mark::Selected => Some(("marker.selected", "on yellow")),
            Mark::Visual => Some(("marker.visual", "on cyan")),
            Mark::Unset => Some(("marker.unset", "on red")),
        });
        self.list_options("selection_markers", move |options| {
            options.marks = Some(marks.clone())
        })
    }

    /// [`selection_markers`](Self::selection_markers) for a list that is
    /// not the one `selection` is over: its items found in the selection
    /// by key alone, with no cursor of their own. A parent column showing
    /// which of its items are selected, a second view of the same items,
    /// a search's results.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::multi_select;
    ///
    /// let app = App::new(|| {
    ///     let picked = multi_select::<String>();
    ///     picked.select("b".to_string());
    ///     // Another list, in another order, with a cursor of its own.
    ///     let other = signal(vec!["b".to_string(), "a".to_string()]);
    ///     list(move || other.get(), signal(1))
    ///         .marks_by(picked, move |i| other.with(|o| o[i].clone()))
    /// });
    /// let mut driver = app.driver(6, 2);
    /// driver.settle();
    /// let styled = intuituive::testing::styled(&driver);
    /// assert!(styled.lines().next().unwrap().contains("on yellow"), "{styled}");
    /// ```
    pub fn marks_by<K: Clone + Eq + Hash + 'static>(
        self,
        selection: MultiSelect<K>,
        key: impl Fn(usize) -> K + 'static,
    ) -> Node {
        let marks: crate::list::Marks = Rc::new(move |i| match selection.mark(&key(i)) {
            Mark::None => None,
            Mark::Selected => Some(("marker.selected", "on yellow")),
            Mark::Visual => Some(("marker.visual", "on cyan")),
            Mark::Unset => Some(("marker.unset", "on red")),
        });
        self.list_options("marks_by", move |options| {
            options.marks = Some(marks.clone())
        })
    }
}

/// Find next and previous over a list's items: [`list_search`].
#[derive(Clone)]
pub struct ListSearch {
    query: Rc<dyn Fn() -> String>,
    items: Rc<dyn Fn() -> Vec<String>>,
    cursor: Signal<usize>,
}

/// Find over a list: `query` (which may read signals; empty finds
/// nothing) against the plain text of `items`, moving `cursor`, the list's
/// selected index, as vim's and Yazi's `n` and `N` do. The match is a
/// substring, in smart case: a query in lower case matches either case,
/// one with a capital matches exactly. [`next`](ListSearch::next) and
/// [`previous`](ListSearch::previous) wrap at the ends;
/// [`place`](ListSearch::place) says which match the cursor is on.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::list_search;
///
/// let app = App::new(|| {
///     let items = || ["main.rs", "lib.rs", "README.md", "Cargo.toml"].map(String::from).to_vec();
///     let query = signal(String::from("r"));
///     let cursor = signal(0usize);
///     let find = list_search(move || query.get(), items, cursor);
///     let shown = find.clone();
///     column([
///         list(items, cursor).fixed(4),
///         text(move || {
///             let (n, of) = shown.place();
///             format!("{n}/{of}")
///         }),
///     ])
///     .on_key("n", move |_| {
///         find.next();
///     })
/// });
/// let mut driver = app.driver(12, 5);
/// driver.press("n");
/// // From main.rs, the next match is lib.rs: the second of four.
/// assert_eq!(driver.screen().plain()[4].trim_end(), "2/4");
/// ```
pub fn list_search(
    query: impl Fn() -> String + 'static,
    items: impl Fn() -> Vec<String> + 'static,
    cursor: Signal<usize>,
) -> ListSearch {
    ListSearch {
        query: Rc::new(query),
        items: Rc::new(items),
        cursor,
    }
}

/// Whether `text` holds `query`, in smart case.
fn finds(query: &str, text: &str) -> bool {
    if query.chars().any(char::is_uppercase) {
        text.contains(query)
    } else {
        text.to_lowercase().contains(&query.to_lowercase())
    }
}

impl ListSearch {
    /// The indices of the items that match, subscribing.
    pub fn matches(&self) -> Vec<usize> {
        let query = (self.query)();
        if query.is_empty() {
            return Vec::new();
        }
        (self.items)()
            .iter()
            .enumerate()
            .filter(|(_, item)| finds(&query, &plain(item)))
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether item `i` matches, subscribing.
    pub fn is_match(&self, i: usize) -> bool {
        let query = (self.query)();
        !query.is_empty()
            && (self.items)()
                .get(i)
                .is_some_and(|item| finds(&query, &plain(item)))
    }

    fn untracked_matches(&self) -> Vec<usize> {
        crate::reactive::Runtime::current().untracked(|| self.matches())
    }

    /// Move the cursor to the next match after it, wrapping to the first;
    /// the index it went to, or `None` with no match.
    pub fn next(&self) -> Option<usize> {
        let matches = self.untracked_matches();
        let at = self.cursor.get_untracked();
        let to = matches
            .iter()
            .copied()
            .find(|i| *i > at)
            .or_else(|| matches.first().copied())?;
        self.cursor.set(to);
        Some(to)
    }

    /// [`next`](Self::next), staying on the item the cursor is on if
    /// that matches and `here` is true: find as you type, which keeps a
    /// hovered match where it is (Yazi's `/`).
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::list_search;
    ///
    /// let app = App::new(|| {
    ///     let items = || ["lib.rs", "main.rs", "mod.rs"].map(String::from).to_vec();
    ///     let cursor = signal(1usize);
    ///     let find = list_search(|| "m".to_string(), items, cursor);
    ///     text!("{cursor}").on_key("n", move |_| {
    ///         find.next_from(true);
    ///     })
    /// });
    /// let mut driver = app.driver(5, 1);
    /// driver.press("n");
    /// // main.rs matches: the cursor stays on it.
    /// assert_eq!(driver.screen().plain()[0].trim_end(), "1");
    /// ```
    pub fn next_from(&self, here: bool) -> Option<usize> {
        let at = self.cursor.get_untracked();
        if here && crate::reactive::untracked(|| self.is_match(at)) {
            return Some(at);
        }
        self.next()
    }

    /// [`previous`](Self::previous), staying on the item the cursor is on
    /// if that matches and `here` is true.
    pub fn previous_from(&self, here: bool) -> Option<usize> {
        let at = self.cursor.get_untracked();
        if here && crate::reactive::untracked(|| self.is_match(at)) {
            return Some(at);
        }
        self.previous()
    }

    /// Move the cursor to the previous match before it, wrapping to the
    /// last; the index it went to, or `None` with no match.
    pub fn previous(&self) -> Option<usize> {
        let matches = self.untracked_matches();
        let at = self.cursor.get_untracked();
        let to = matches
            .iter()
            .rev()
            .copied()
            .find(|i| *i < at)
            .or_else(|| matches.last().copied())?;
        self.cursor.set(to);
        Some(to)
    }

    /// Which match the cursor is on, from 1, and how many there are,
    /// subscribing: `(2, 5)`. On an item that does not match, the first
    /// number is 0.
    pub fn place(&self) -> (usize, usize) {
        let matches = self.matches();
        let at = self.cursor.get();
        let n = matches.iter().position(|i| *i == at).map_or(0, |n| n + 1);
        (n, matches.len())
    }
}

/// An item's plain text: its markup's, or itself where that does not
/// parse.
fn plain(item: &str) -> String {
    rich::Text::from_markup(item)
        .map(|text| text.plain().to_string())
        .unwrap_or_else(|_| item.to_string())
}

#[cfg(test)]
mod tests {
    use super::finds;

    #[test]
    fn smart_case_ignores_case_until_the_query_has_a_capital() {
        assert!(finds("readme", "README.md"));
        assert!(finds("READ", "README.md"));
        assert!(!finds("Read", "README.md"));
        assert!(!finds("x", "README.md"));
    }
}

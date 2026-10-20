//! Rows from a source: [`TableSource`], what a table asks for its rows by
//! index (a file indexed in the background, a database cursor, a log), a
//! [`source_table`] over one, and [`view_of`], a source filtered, sorted
//! and searched on a background task, whose result is a source itself.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::ops::Range;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering as Atomic};
use std::sync::Arc;

use crate::node::Node;
use crate::reactive::{signal, watch, Signal};
use crate::widgets::{virtual_table_with, Column, Order, TableOptions};

/// How many rows a [`TableSource`] has: all of them, or as many as it has
/// found so far (a file still being indexed).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RowCount {
    Exact(usize),
    AtLeast(usize),
}

impl RowCount {
    /// The rows there are now.
    pub fn count(self) -> usize {
        match self {
            RowCount::Exact(n) | RowCount::AtLeast(n) => n,
        }
    }

    /// Whether the count is final.
    pub fn is_exact(self) -> bool {
        matches!(self, RowCount::Exact(_))
    }
}

/// `1000`, or `1000+` while it is not final: csvlens's row count.
impl fmt::Display for RowCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RowCount::Exact(n) => write!(f, "{n}"),
            RowCount::AtLeast(n) => write!(f, "{n}+"),
        }
    }
}

/// Rows by index, as a table asks for them: how many there are (perhaps
/// not all found yet) and one row's cells.
///
/// A [`source_table`] calls both on the app's thread while it draws, so a
/// `len` that reads a signal (the count a background indexer has reached)
/// draws the table again as it grows. A source given to [`view_of`] is also
/// read by `row` on a worker thread, so it is `Send + Sync` and its `row`
/// reads no signals.
pub trait TableSource {
    /// How many rows there are.
    fn len(&self) -> RowCount;

    /// Row `i`'s cells, as markup.
    fn row(&self, i: usize) -> Vec<String>;

    /// The rows in `range`, in order: each one's index and cells given to
    /// `each`, until it returns `false`. What a [`view_of`] pass reads the
    /// source with. By default [`row`](Self::row) a row; a source that can
    /// do better in bulk (a file read in one sequential sweep rather than
    /// a seek a row, a database cursor) overrides it.
    ///
    /// ```
    /// use std::ops::Range;
    /// use intuituive::widgets::{RowCount, TableSource};
    ///
    /// /// Lines of one string, handed out a sweep at a time.
    /// struct Lines(String);
    /// impl TableSource for Lines {
    ///     fn len(&self) -> RowCount {
    ///         RowCount::Exact(self.0.lines().count())
    ///     }
    ///     fn row(&self, i: usize) -> Vec<String> {
    ///         self.0.lines().nth(i).map(|l| vec![l.to_string()]).unwrap_or_default()
    ///     }
    ///     fn rows(&self, range: Range<usize>, each: &mut dyn FnMut(usize, Vec<String>) -> bool) {
    ///         let lines = self.0.lines().enumerate().skip(range.start).take(range.len());
    ///         for (i, line) in lines {
    ///             if !each(i, vec![line.to_string()]) {
    ///                 return;
    ///             }
    ///         }
    ///     }
    /// }
    /// let mut seen = Vec::new();
    /// Lines("a\nb\nc".into()).rows(1..3, &mut |i, cells| {
    ///     seen.push((i, cells[0].clone()));
    ///     true
    /// });
    /// assert_eq!(seen, [(1, "b".to_string()), (2, "c".to_string())]);
    /// ```
    fn rows(&self, range: Range<usize>, each: &mut dyn FnMut(usize, Vec<String>) -> bool) {
        for i in range {
            if !each(i, self.row(i)) {
                return;
            }
        }
    }

    /// Whether there are no rows yet.
    fn is_empty(&self) -> bool {
        self.len().count() == 0
    }
}

impl<T: TableSource + ?Sized> TableSource for Rc<T> {
    fn len(&self) -> RowCount {
        (**self).len()
    }

    fn row(&self, i: usize) -> Vec<String> {
        (**self).row(i)
    }

    fn rows(&self, range: Range<usize>, each: &mut dyn FnMut(usize, Vec<String>) -> bool) {
        (**self).rows(range, each)
    }
}

impl<T: TableSource + ?Sized> TableSource for Arc<T> {
    fn len(&self) -> RowCount {
        (**self).len()
    }

    fn row(&self, i: usize) -> Vec<String> {
        (**self).row(i)
    }

    fn rows(&self, range: Range<usize>, each: &mut dyn FnMut(usize, Vec<String>) -> bool) {
        (**self).rows(range, each)
    }
}

/// Rows in hand as a source: every row, exact.
impl TableSource for Vec<Vec<String>> {
    fn len(&self) -> RowCount {
        RowCount::Exact(self.as_slice().len())
    }

    fn row(&self, i: usize) -> Vec<String> {
        self.get(i).cloned().unwrap_or_default()
    }
}

/// A [`virtual_table_with`] whose rows come from `source`: only the rows
/// in view are asked for, and the count is read at each draw.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{source_table, Column, RowCount, TableOptions, TableSource};
///
/// struct Squares;
/// impl TableSource for Squares {
///     fn len(&self) -> RowCount {
///         RowCount::AtLeast(1000)
///     }
///     fn row(&self, i: usize) -> Vec<String> {
///         vec![i.to_string(), (i * i).to_string()]
///     }
/// }
/// let app = App::new(|| {
///     let columns = vec![Column::new("n", Size::Auto), Column::new("n²", Size::Auto)];
///     source_table(Squares, columns, signal(0), TableOptions::default())
/// });
/// let mut driver = app.driver(10, 3);
/// driver.press("end");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "999 998001");
/// ```
pub fn source_table(
    source: impl TableSource + 'static,
    columns: Vec<Column>,
    selected: Signal<usize>,
    options: TableOptions,
) -> Node {
    let source = Rc::new(source);
    let rows = source.clone();
    virtual_table_with(
        columns,
        move || source.len().count(),
        move |i| rows.row(i),
        selected,
        options,
    )
}

/// A test of a row's cells, for [`ViewQuery::keep`] and
/// [`ViewQuery::find`]: run on a worker thread.
pub type RowTest = Arc<dyn Fn(&[String]) -> bool + Send + Sync>;

/// How two cells compare, for [`ViewQuery::compare`].
pub type CellOrder = Arc<dyn Fn(&str, &str) -> Ordering + Send + Sync>;

/// What a [`view_of`] does to its source: keep the rows a test passes,
/// sort them by a column, and find the rows another test passes. Each part
/// is off by default.
#[derive(Clone, Default)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct ViewQuery {
    pub keep: Option<RowTest>,
    pub sort: Option<(usize, Order)>,
    /// How the sort compares two cells; by default as numbers when both
    /// are, else as text without case or markup.
    pub compare: Option<CellOrder>,
    pub find: Option<RowTest>,
}

impl ViewQuery {
    /// Keep the rows `test` passes.
    pub fn keep(mut self, test: impl Fn(&[String]) -> bool + Send + Sync + 'static) -> ViewQuery {
        self.keep = Some(Arc::new(test));
        self
    }

    /// Sort by `column`.
    pub fn sort(mut self, column: usize, order: Order) -> ViewQuery {
        self.sort = Some((column, order));
        self
    }

    /// Compare cells with `compare` when sorting.
    pub fn compare(
        mut self,
        compare: impl Fn(&str, &str) -> Ordering + Send + Sync + 'static,
    ) -> ViewQuery {
        self.compare = Some(Arc::new(compare));
        self
    }

    /// Find the rows `test` passes.
    pub fn find(mut self, test: impl Fn(&[String]) -> bool + Send + Sync + 'static) -> ViewQuery {
        self.find = Some(Arc::new(test));
        self
    }
}

/// Queries are the same when they hold the same tests (the same `Arc`s)
/// and the same sort.
impl PartialEq for ViewQuery {
    fn eq(&self, other: &ViewQuery) -> bool {
        fn same<T: ?Sized>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
            match (a, b) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            }
        }
        same(&self.keep, &other.keep)
            && self.sort == other.sort
            && same(&self.compare, &other.compare)
            && same(&self.find, &other.find)
    }
}

/// What a pass over the source gave.
#[derive(Clone, Default)]
struct Pass {
    /// The source's rows in the view's order; `None` is every row in its
    /// own order.
    rows: Option<Arc<Vec<usize>>>,
    /// The places in the view of the rows found.
    found: Arc<Vec<usize>>,
    /// Whether the source's count was not final when the pass ran.
    partial: bool,
}

/// A source seen through a [`ViewQuery`]: the rows kept, in their order,
/// and the rows found. Itself a [`TableSource`] (give it to a
/// [`source_table`]); `Copy`, so any handler takes one. Made by
/// [`view_of`].
pub struct View<S: 'static> {
    source: Signal<Arc<S>>,
    pass: Signal<Pass>,
    busy: Signal<bool>,
}

impl<S: 'static> Clone for View<S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: 'static> Copy for View<S> {}

/// `source` filtered, sorted and searched as `query` (which may read
/// signals) says, on a worker thread: each row is read once a pass (with
/// [`TableSource::rows`], a thousand at a time), and a
/// new query (or the source's count becoming final) starts a new pass,
/// dropping the one running. Until the first pass ends the view is the
/// source as it is. csvlens's whole-file filter, find and sort over a file
/// read lazily.
///
/// The table's selection is a place in the view: keep it on its row across
/// passes with [`Node::cursor_by`] on [`View::source_row`].
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::{source_table, view_of, Column, TableOptions, ViewQuery};
///
/// let app = App::new(|| {
///     let rows: Vec<Vec<String>> = (0..100).map(|i| vec![i.to_string()]).collect();
///     let pattern = signal(String::from("7"));
///     let view = view_of(rows, move || {
///         let pattern = pattern.get();
///         ViewQuery::default().keep(move |cells| cells[0].contains(&pattern))
///     });
///     let cursor = signal(0);
///     column([
///         source_table(view, vec![Column::new("n", Size::Auto)], cursor, TableOptions::default())
///             .fixed(4),
///         text(move || format!("{} rows", view.len())),
///     ])
/// });
/// let mut driver = app.wait_for_tasks(true).driver(10, 5);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1..4], ["7", "17", "27"].map(|r| format!("{r:<10}")));
/// assert_eq!(driver.screen().plain()[4].trim_end(), "19 rows");
/// ```
pub fn view_of<S: TableSource + Send + Sync + 'static>(
    source: S,
    query: impl Fn() -> ViewQuery + 'static,
) -> View<S> {
    let source = Arc::new(source);
    let view = View {
        source: signal(source.clone()),
        pass: signal(Pass::default()),
        busy: signal(false),
    };
    let generation = Arc::new(AtomicU64::new(0));
    let running: Rc<RefCell<Option<crate::task::Task>>> = Rc::new(RefCell::new(None));
    let counted = source.clone();
    watch(
        move || {
            let len = counted.len();
            (query(), len.is_exact().then(|| len.count()))
        },
        move |(query, _), _| {
            let mine = generation.fetch_add(1, Atomic::SeqCst) + 1;
            if let Some(task) = running.borrow_mut().take() {
                task.cancel();
            }
            let identity = query.keep.is_none() && query.sort.is_none() && query.find.is_none();
            if identity {
                view.pass.replace(Pass::default());
                view.busy.set(false);
                return;
            }
            let len = source.len();
            let (source, current) = (source.clone(), generation.clone());
            view.busy.set(true);
            let task = crate::task::spawn(
                move || {
                    run(&*source, len, &query, || {
                        current.load(Atomic::SeqCst) != mine
                    })
                },
                move |pass, _| {
                    if let Some(pass) = pass {
                        view.pass.replace(pass);
                        view.busy.set(false);
                    }
                },
            );
            *running.borrow_mut() = Some(task);
        },
    );
    view
}

/// A sort key: a cell's number, else its plain text without case.
enum Key {
    Number(f64),
    Text(String),
}

impl Key {
    fn of(cell: &str) -> Key {
        let plain = rich::Text::from_markup(cell)
            .map(|text| text.plain().to_string())
            .unwrap_or_else(|_| cell.to_string());
        match plain.trim().parse::<f64>() {
            Ok(n) if !n.is_nan() => Key::Number(n),
            _ => Key::Text(plain.to_lowercase()),
        }
    }

    /// Numbers before text when a column mixes them, as both compare among
    /// themselves.
    fn cmp(&self, other: &Key) -> Ordering {
        match (self, other) {
            (Key::Number(a), Key::Number(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (Key::Text(a), Key::Text(b)) => a.cmp(b),
            (Key::Number(_), Key::Text(_)) => Ordering::Less,
            (Key::Text(_), Key::Number(_)) => Ordering::Greater,
        }
    }
}

/// One pass over `source`'s first `len` rows; `None` once `stale` says a
/// newer query has started.
fn run<S: TableSource + ?Sized>(
    source: &S,
    len: RowCount,
    query: &ViewQuery,
    stale: impl Fn() -> bool,
) -> Option<Pass> {
    let n = len.count();
    let mut kept: Vec<usize> = Vec::new();
    let mut cells: Vec<String> = Vec::new();
    let mut hits: HashSet<usize> = HashSet::new();
    // In bulk, a thousand rows at a time, looking between them for a
    // newer query.
    for start in (0..n).step_by(1024) {
        if stale() {
            return None;
        }
        source.rows(start..(start + 1024).min(n), &mut |i, mut row| {
            if let Some(keep) = &query.keep {
                if !keep(&row) {
                    return true;
                }
            }
            if let Some(find) = &query.find {
                if find(&row) {
                    hits.insert(i);
                }
            }
            if let Some((column, _)) = query.sort {
                let cell = match column < row.len() {
                    true => row.swap_remove(column),
                    false => String::new(),
                };
                cells.push(cell);
            }
            kept.push(i);
            true
        });
    }
    if let Some((_, order)) = query.sort {
        let mut places: Vec<usize> = (0..kept.len()).collect();
        match &query.compare {
            Some(compare) => places.sort_by(|&a, &b| compare(&cells[a], &cells[b])),
            None => {
                let keys: Vec<Key> = cells.iter().map(|cell| Key::of(cell)).collect();
                places.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
            }
        }
        if order == Order::Descending {
            places.reverse();
        }
        kept = places.into_iter().map(|p| kept[p]).collect();
    }
    if stale() {
        return None;
    }
    let found = kept
        .iter()
        .enumerate()
        .filter(|(_, i)| hits.contains(i))
        .map(|(at, _)| at)
        .collect();
    let all = query.keep.is_none() && query.sort.is_none();
    Some(Pass {
        rows: (!all).then(|| Arc::new(kept)),
        found: Arc::new(found),
        partial: !len.is_exact(),
    })
}

impl<S: TableSource + 'static> View<S> {
    /// The rows in the view now, subscribing: the source's count until a
    /// pass has filtered or sorted it; `AtLeast` while the pass ran over a
    /// count that was not final.
    pub fn len(&self) -> RowCount {
        self.pass.with(|pass| match &pass.rows {
            Some(rows) if pass.partial => RowCount::AtLeast(rows.len()),
            Some(rows) => RowCount::Exact(rows.len()),
            None => self.source.with(|source| source.len()),
        })
    }

    /// Whether the view has no rows now.
    pub fn is_empty(&self) -> bool {
        self.len().count() == 0
    }

    /// The source's row at place `at` in the view, subscribing.
    pub fn source_row(&self, at: usize) -> Option<usize> {
        self.pass.with(|pass| match &pass.rows {
            Some(rows) => rows.get(at).copied(),
            None => Some(at),
        })
    }

    /// The places in the view of the rows found, subscribing.
    pub fn found(&self) -> Arc<Vec<usize>> {
        self.pass.with(|pass| pass.found.clone())
    }

    /// The first row found after place `at`, round to the first at the
    /// end: `n`.
    pub fn next_found(&self, at: usize) -> Option<usize> {
        let found = self.pass.with_untracked(|pass| pass.found.clone());
        let next = found.partition_point(|&p| p <= at);
        found.get(next).or(found.first()).copied()
    }

    /// The last row found before place `at`, round to the last at the
    /// start: `N`.
    pub fn previous_found(&self, at: usize) -> Option<usize> {
        let found = self.pass.with_untracked(|pass| pass.found.clone());
        let before = found.partition_point(|&p| p < at);
        match before {
            0 => found.last().copied(),
            n => Some(found[n - 1]),
        }
    }

    /// Which row found place `at` is (from 1; 0 when it is none) and how
    /// many there are, subscribing: csvlens's `[Find "re": 2/4]`.
    pub fn place(&self, at: usize) -> (usize, usize) {
        self.pass.with(|pass| {
            let n = pass.found.binary_search(&at).map_or(0, |i| i + 1);
            (n, pass.found.len())
        })
    }

    /// Whether a pass is running, subscribing: `Finding…`.
    pub fn is_busy(&self) -> bool {
        self.busy.get()
    }
}

/// The view's rows, by place: what a [`source_table`] over it shows.
impl<S: TableSource + 'static> TableSource for View<S> {
    fn len(&self) -> RowCount {
        View::len(self)
    }

    fn row(&self, i: usize) -> Vec<String> {
        match self.source_row(i) {
            Some(at) => self.source.with_untracked(|source| source.row(at)),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(cells: &[&str]) -> Vec<Vec<String>> {
        cells.iter().map(|c| vec![c.to_string()]).collect()
    }

    #[test]
    fn a_pass_keeps_sorts_and_finds() {
        let source = rows(&["10", "9", "apple", "100", "Banana"]);
        let query = ViewQuery::default()
            .keep(|row| row[0] != "9")
            .sort(0, Order::Ascending)
            .find(|row| row[0].contains('a'));
        let pass = run(&source, TableSource::len(&source), &query, || false).unwrap();
        // 10, 100, apple, Banana: numbers first.
        assert_eq!(pass.rows.unwrap().as_slice(), [0, 3, 2, 4]);
        assert_eq!(pass.found.as_slice(), [2, 3]);
        assert!(!pass.partial);
    }

    #[test]
    fn a_stale_pass_gives_nothing() {
        let source = rows(&["a"]);
        let query = ViewQuery::default().keep(|_| true);
        assert!(run(&source, TableSource::len(&source), &query, || true).is_none());
    }

    /// Reads only in bulk: `row` is not called by a pass.
    struct Bulk {
        sweeps: std::sync::Mutex<Vec<Range<usize>>>,
    }

    impl TableSource for Bulk {
        fn len(&self) -> RowCount {
            RowCount::Exact(2500)
        }
        fn row(&self, _: usize) -> Vec<String> {
            panic!("a pass reads in bulk")
        }
        fn rows(&self, range: Range<usize>, each: &mut dyn FnMut(usize, Vec<String>) -> bool) {
            self.sweeps.lock().unwrap().push(range.clone());
            for i in range {
                if !each(i, vec![i.to_string()]) {
                    return;
                }
            }
        }
    }

    #[test]
    fn a_pass_reads_the_source_in_bulk() {
        let source = Bulk {
            sweeps: Default::default(),
        };
        let query = ViewQuery::default().keep(|row| row[0].ends_with("99"));
        let pass = run(&source, source.len(), &query, || false).unwrap();
        assert_eq!(pass.rows.unwrap().len(), 25);
        assert_eq!(
            *source.sweeps.lock().unwrap(),
            [0..1024, 1024..2048, 2048..2500]
        );
    }

    #[test]
    fn counts_show_whether_they_are_final() {
        assert_eq!(RowCount::Exact(5).to_string(), "5");
        assert_eq!(RowCount::AtLeast(5).to_string(), "5+");
    }
}

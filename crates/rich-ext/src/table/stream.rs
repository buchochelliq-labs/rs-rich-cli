//! A keyed table for append/update workloads under a live display.
//!
//! [`StreamingTable`] holds rows under stable keys: [`upsert`] appends a new
//! key and replaces an existing one in place, [`update_cell`] changes one
//! cell, [`remove`] drops a row. Rows keep their insertion order unless a
//! sort is set. A [`Window`] shows only the first or last N rows with an
//! `… N more rows` line, and [`capacity`] evicts the oldest rows for
//! log-like streams.
//!
//! # Only changed rows are re-rendered
//!
//! Each row caches its formatted cells, their widths and its rendered lines.
//! A frame re-formats only rows written since the last frame, and re-lays out
//! only those rows — unless the column widths change (a new row widens a
//! column, the widest row goes away, the width available changes), in which
//! case every visible row is laid out again. [`stats`] counts both, so the
//! claim is testable.
//!
//! The rendering itself is the core [`Table`]'s, never a copy of it: widths
//! come from the core's column sizing, which depends only on each column's
//! widest cell. Each changed row is rendered by a core table holding one
//! *proxy* row of those widest cells plus the row itself, so the row lays
//! out at exactly the widths the whole table would give it. The output is
//! byte-for-byte what [`to_table`] renders (plus the window's indicator).
//!
//! Row separators (`show_lines`) are not supported; use [`TableData`] for a
//! static table that needs them.
//!
//! ```
//! use rich::{Console, Justify};
//! use rich_ext::table::{Column, StreamingTable, Value};
//!
//! let mut jobs = StreamingTable::new([
//!     Column::new("job"),
//!     Column::new("state"),
//!     Column::new("done").justify(Justify::Right),
//! ]);
//! jobs.upsert("build", ["build".into(), "running".into(), Value::Int(40)]);
//! jobs.upsert("test", ["test".into(), "queued".into(), Value::Int(0)]);
//!
//! let console = Console::builder().width(40).build();
//! console.render_export(&jobs); // first frame: both rows render
//! jobs.update_cell(&"build", 2, Value::Int(80));
//! let out = console.render_export(&jobs); // only `build` renders again
//! assert!(out.contains("│ build │ running │   80 │"), "{out}");
//! assert_eq!(jobs.stats().rows_rendered, 3);
//! ```
//!
//! Under a [`LiveCoordinator`](crate::live::LiveCoordinator), render the
//! table into its region after each batch of changes:
//!
//! ```
//! use rich::protocol::{Support, TargetCapabilities};
//! use rich::Theme;
//! use rich_ext::live::LiveCoordinator;
//! use rich_ext::table::{Column, StreamingTable, Window};
//! use rich_ext::target::{RenderTarget, TargetKind};
//!
//! let capabilities = TargetCapabilities {
//!     width: 40,
//!     height: 10,
//!     color_system: None,
//!     interactive: false,
//!     unicode: true,
//!     hyperlinks: false,
//!     sixel: Support::Unsupported,
//! };
//! let target = RenderTarget::new(TargetKind::Custom, capabilities, Theme::default_theme());
//! let mut log = StreamingTable::new([Column::new("#"), Column::new("event")])
//!     .window(Window::Tail(2));
//!
//! let mut out = Vec::new();
//! let mut live = LiveCoordinator::new(&mut out, target.clone());
//! let region = live.add(target.segments(&log)).unwrap();
//! for (n, event) in ["start", "fetch", "build"].into_iter().enumerate() {
//!     log.upsert(n, [n.into(), event.into()]);
//!     live.update(region.clone(), target.segments(&log)).unwrap();
//!     live.refresh().unwrap();
//! }
//! live.finish().unwrap();
//! drop(live);
//! let out = String::from_utf8(out).unwrap();
//! assert!(out.starts_with("… 1 earlier row\n"), "{out}");
//! assert!(out.contains("│ 2 │ build │"), "{out}");
//! ```
//!
//! [`upsert`]: StreamingTable::upsert
//! [`update_cell`]: StreamingTable::update_cell
//! [`remove`]: StreamingTable::remove
//! [`capacity`]: StreamingTable::capacity
//! [`stats`]: StreamingTable::stats
//! [`to_table`]: StreamingTable::to_table
//! [`Table`]: rich::Table
//! [`TableData`]: super::TableData

use std::collections::{BTreeMap, HashMap};
use std::convert::Infallible;
use std::fmt;
use std::hash::Hash;
use std::sync::{Mutex, PoisonError};

use rich::{Console, ConsoleOptions, LineRenderable, Renderable, Segment, Table, Text};

use super::data::{normalize, TableData};
use super::sort::{compare_rows, SortKey};
use super::{frame_builders, headers, style, Column, Frame, Value};

/// Which rows a [`StreamingTable`] shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Window {
    /// Every row.
    #[default]
    All,
    /// The first N rows in display order, then `… N more rows`.
    Head(usize),
    /// The last N rows in display order, after `… N earlier rows` — for
    /// log-like appends.
    Tail(usize),
}

/// Counters for a [`StreamingTable`]'s render cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderStats {
    /// Frames rendered.
    pub frames: u64,
    /// Rows whose cells were formatted and measured (new or changed rows).
    pub rows_prepared: u64,
    /// Rows laid out by the core table (changed rows, or every visible row
    /// after a relayout).
    pub rows_rendered: u64,
    /// Frames where the column widths or the available width changed, so
    /// every cached row was laid out again.
    pub relayouts: u64,
}

struct Entry<K> {
    key: K,
    values: Vec<Value>,
    version: u64,
}

/// One row's cached work.
struct RowCache {
    version: u64,
    cells: Vec<Text>,
    widths: Vec<usize>,
    lines: Option<Vec<Vec<Segment>>>,
}

/// What a row's rendered lines depend on besides its own cells.
#[derive(Clone, Debug, PartialEq, Eq)]
struct LayoutKey {
    width: usize,
    maxima: Vec<usize>,
    ascii_only: bool,
    safe_box: bool,
    legacy_windows: bool,
}

#[derive(Default)]
struct Cache {
    rows: HashMap<u64, RowCache>,
    layout: Option<LayoutKey>,
    stats: RenderStats,
}

/// Keyed rows rendered incrementally; see the [module docs](self).
///
/// `K` is the stable row key (a job id, a path, a sequence number). Cells are
/// [`Value`]s shown through each [`Column`]'s formatter; the frame (title,
/// caption, box, edges, expand, border style) is set with the same builders
/// as [`TableData`].
pub struct StreamingTable<K> {
    columns: Vec<Column>,
    frame: Frame,
    window: Window,
    capacity: Option<usize>,
    sort: Vec<SortKey>,
    /// Rows by insertion sequence number, so iteration is insertion order.
    entries: BTreeMap<u64, Entry<K>>,
    index: HashMap<K, u64>,
    next_seq: u64,
    next_version: u64,
    evicted: u64,
    cache: Mutex<Cache>,
}

impl<K> fmt::Debug for StreamingTable<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StreamingTable")
            .field("columns", &self.columns)
            .field("rows", &self.entries.len())
            .field("window", &self.window)
            .field("capacity", &self.capacity)
            .field("sort", &self.sort)
            .field("evicted", &self.evicted)
            .finish()
    }
}

frame_builders!([K] StreamingTable<K>);

impl<K: Eq + Hash + Clone> StreamingTable<K> {
    /// An empty table under `columns`.
    pub fn new(columns: impl IntoIterator<Item = Column>) -> Self {
        StreamingTable {
            columns: columns.into_iter().collect(),
            frame: Frame::default(),
            window: Window::All,
            capacity: None,
            sort: Vec::new(),
            entries: BTreeMap::new(),
            index: HashMap::new(),
            next_seq: 0,
            next_version: 0,
            evicted: 0,
            cache: Mutex::new(Cache::default()),
        }
    }

    /// Show only part of the rows.
    pub fn window(mut self, window: Window) -> Self {
        self.window = window;
        self
    }

    /// Change the window.
    pub fn set_window(&mut self, window: Window) {
        self.window = window;
    }

    /// Keep at most `rows` rows: inserting beyond that evicts the oldest
    /// (first inserted). Evicted rows count as earlier rows in the indicator.
    pub fn capacity(mut self, rows: usize) -> Self {
        self.capacity = Some(rows);
        self.evict();
        self
    }

    /// Show rows sorted by `keys` (stable over insertion order, empty cells
    /// last), with header indicators; an empty list restores insertion order.
    /// Sorting reorders cached rows without re-rendering them.
    pub fn set_sort(&mut self, keys: impl IntoIterator<Item = SortKey>) {
        self.sort = keys.into_iter().collect();
    }

    /// Builder form of [`set_sort`](Self::set_sort).
    pub fn sort_by(mut self, keys: impl IntoIterator<Item = SortKey>) -> Self {
        self.set_sort(keys);
        self
    }

    fn version(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    fn evict(&mut self) {
        let Some(capacity) = self.capacity else {
            return;
        };
        while self.entries.len() > capacity {
            let Some((_, entry)) = self.entries.pop_first() else {
                break;
            };
            self.index.remove(&entry.key);
            self.evicted += 1;
        }
    }

    /// Insert a row, or replace the row under `key` in place (it keeps its
    /// position). Returns `true` when the key is new. Missing cells are
    /// `Null`; extra cells are dropped. Writing identical values is not a
    /// change and does not re-render the row.
    pub fn upsert(&mut self, key: K, row: impl IntoIterator<Item = Value>) -> bool {
        let values = normalize(row, self.columns.len());
        if let Some(&seq) = self.index.get(&key) {
            let entry = self.entries.get(&seq).expect("indexed rows exist");
            if entry.values != values {
                let version = self.version();
                let entry = self.entries.get_mut(&seq).expect("indexed rows exist");
                entry.values = values;
                entry.version = version;
            }
            return false;
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        let version = self.version();
        self.index.insert(key.clone(), seq);
        self.entries.insert(
            seq,
            Entry {
                key,
                values,
                version,
            },
        );
        self.evict();
        true
    }

    /// Set one cell of the row under `key`. Returns `false` when there is no
    /// such row or column.
    pub fn update_cell(&mut self, key: &K, column: usize, value: impl Into<Value>) -> bool {
        let value = value.into();
        let Some(&seq) = self.index.get(key) else {
            return false;
        };
        if column >= self.columns.len() {
            return false;
        }
        if self.entries[&seq].values[column] != value {
            let version = self.version();
            let entry = self.entries.get_mut(&seq).expect("indexed rows exist");
            entry.values[column] = value;
            entry.version = version;
        }
        true
    }

    /// Remove the row under `key`, returning its cells.
    pub fn remove(&mut self, key: &K) -> Option<Vec<Value>> {
        let seq = self.index.remove(key)?;
        self.entries.remove(&seq).map(|entry| entry.values)
    }

    /// Remove every row (the evicted count is kept).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
    }

    /// The cells of the row under `key`.
    pub fn get(&self, key: &K) -> Option<&[Value]> {
        let seq = self.index.get(key)?;
        self.entries.get(seq).map(|entry| entry.values.as_slice())
    }

    /// Whether a row is stored under `key`.
    pub fn contains_key(&self, key: &K) -> bool {
        self.index.contains_key(key)
    }

    /// The number of stored rows (shown or not).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether no rows are stored.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// How many rows [`capacity`](Self::capacity) has evicted.
    pub fn evicted(&self) -> u64 {
        self.evicted
    }

    /// The columns.
    pub fn columns(&self) -> &[Column] {
        &self.columns
    }

    /// Stored rows in insertion order.
    pub fn rows(&self) -> impl Iterator<Item = (&K, &[Value])> + '_ {
        self.entries
            .values()
            .map(|entry| (&entry.key, entry.values.as_slice()))
    }

    /// Render-cache counters since creation (or [`reset_stats`](Self::reset_stats)).
    pub fn stats(&self) -> RenderStats {
        self.lock().stats
    }

    /// Zero the counters.
    pub fn reset_stats(&self) {
        self.lock().stats = RenderStats::default();
    }

    /// Drop every cached row so the next frame renders from scratch. Needed
    /// only when rendering to a console with a different theme, since cached
    /// lines keep the styles they were rendered with.
    pub fn invalidate(&self) {
        let mut cache = self.lock();
        cache.rows.clear();
        cache.layout = None;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sequence numbers in display order.
    fn order(&self) -> Vec<u64> {
        let mut order: Vec<u64> = self.entries.keys().copied().collect();
        if !self.sort.is_empty() {
            order.sort_by(|a, b| {
                compare_rows(&self.entries[a].values, &self.entries[b].values, &self.sort)
            });
        }
        order
    }

    /// The shown rows' sequence numbers, and the counts of earlier and later
    /// rows not shown.
    fn visible(&self) -> (Vec<u64>, u64, u64) {
        let order = self.order();
        let total = order.len();
        match self.window {
            Window::All => (order, self.evicted, 0),
            Window::Head(n) => {
                let shown = n.min(total);
                (
                    order[..shown].to_vec(),
                    self.evicted,
                    (total - shown) as u64,
                )
            }
            Window::Tail(n) => {
                let start = total.saturating_sub(n);
                (order[start..].to_vec(), self.evicted + start as u64, 0)
            }
        }
    }

    /// A snapshot of every stored row, in display order, with the same
    /// columns and sort — for grouping and aggregates.
    pub fn to_data(&self) -> TableData {
        let mut data = TableData::new(self.columns.clone());
        data.frame = self.frame.clone();
        for seq in self.order() {
            data.push(self.entries[&seq].values.iter().cloned());
        }
        data.sort_by(self.sort.iter().copied())
    }

    /// The shown rows as one core table, rendered from scratch (without the
    /// window's indicator lines).
    pub fn to_table(&self, console: &Console) -> Table {
        let headers = headers(console, &self.columns, &self.sort);
        let mut table = self.frame.table(&self.columns, &headers, true, true);
        for seq in self.visible().0 {
            let values = &self.entries[&seq].values;
            table.add_row_text(
                self.columns
                    .iter()
                    .zip(values)
                    .map(|(column, value)| column.cell(value))
                    .collect(),
            );
        }
        table
    }

    fn indicator(
        console: &Console,
        options: &ConsoleOptions,
        count: u64,
        what: &str,
    ) -> Vec<Vec<Segment>> {
        let ellipsis = if console.ascii_only() { "..." } else { "…" };
        let plural = if count == 1 { "" } else { "s" };
        let text = Text::styled(
            format!("{ellipsis} {count} {what} row{plural}"),
            style(console, "table.more"),
        );
        // One line per indicator, whatever height the frame is given.
        let mut options = options.clone();
        options.height = None;
        console.render_lines(&text, &options, false)
    }

    /// Render the frame as lines, reusing every cached row that is still valid.
    fn render_lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
        let mut cache = self.lock();
        let cache = &mut *cache;
        cache.stats.frames += 1;
        let (visible, earlier, later) = self.visible();
        let headers = headers(console, &self.columns, &self.sort);

        let mut out = Vec::new();
        if earlier > 0 {
            out.extend(Self::indicator(console, options, earlier, "earlier"));
        }
        // Drop cached rows that are gone or out of the window.
        let shown: std::collections::HashSet<u64> = visible.iter().copied().collect();
        cache.rows.retain(|seq, _| shown.contains(seq));

        if self.columns.is_empty() || visible.is_empty() {
            let table = self.frame.table(&self.columns, &headers, true, true);
            out.extend(table_lines(&table, console, options));
        } else {
            self.render_rows(console, options, cache, &visible, &headers, &mut out);
        }
        if later > 0 {
            out.extend(Self::indicator(console, options, later, "more"));
        }
        out
    }

    fn render_rows(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        cache: &mut Cache,
        visible: &[u64],
        headers: &[Text],
        out: &mut Vec<Vec<Segment>>,
    ) {
        // Format and measure new or changed rows.
        for seq in visible {
            let entry = &self.entries[seq];
            if cache
                .rows
                .get(seq)
                .is_some_and(|row| row.version == entry.version)
            {
                continue;
            }
            let cells: Vec<Text> = self
                .columns
                .iter()
                .zip(&entry.values)
                .map(|(column, value)| column.cell(value))
                .collect();
            let widths = cells.iter().map(|cell| cell.measurement().1).collect();
            cache.rows.insert(
                *seq,
                RowCache {
                    version: entry.version,
                    cells,
                    widths,
                    lines: None,
                },
            );
            cache.stats.rows_prepared += 1;
        }

        // The widest cell of each column, header included: the core sizes a
        // column from its cells' maximum widths only, so a table of these
        // cells gets the same widths as the whole table.
        let mut widest: Vec<(usize, Option<u64>)> = headers
            .iter()
            .map(|header| (header.measurement().1, None))
            .collect();
        for seq in visible {
            for (best, &width) in widest.iter_mut().zip(&cache.rows[seq].widths) {
                if width > best.0 {
                    *best = (width, Some(*seq));
                }
            }
        }
        let proxy: Vec<Text> = widest
            .iter()
            .enumerate()
            .map(|(column, (_, seq))| match seq {
                Some(seq) => cache.rows[seq].cells[column].clone(),
                None => headers[column].clone(),
            })
            .collect();
        let key = LayoutKey {
            width: options.max_width,
            maxima: widest.iter().map(|(width, _)| *width).collect(),
            ascii_only: console.ascii_only(),
            safe_box: console.safe_box(),
            legacy_windows: console.legacy_windows(),
        };
        if cache.layout.as_ref() != Some(&key) {
            for row in cache.rows.values_mut() {
                row.lines = None;
            }
            cache.layout = Some(key);
            cache.stats.relayouts += 1;
        }

        // How many lines the proxy row and the box edges take.
        let edge = self.frame.edge_lines();
        let render = |frame: &Frame, rows: &[&[Text]], show_header: bool| {
            let mut table = frame.table(&self.columns, headers, show_header, true);
            for row in rows {
                table.add_row_text(row.to_vec());
            }
            table_lines(&table, console, options)
        };
        let bare = Frame {
            title: None,
            caption: None,
            ..self.frame.clone()
        };
        let proxy_height = render(&bare, &[&proxy], false).len() - 2 * edge;

        // Title, top edge, header and header separator.
        let head = Frame {
            caption: None,
            ..self.frame.clone()
        };
        let head = render(&head, &[&proxy], true);
        out.extend_from_slice(&head[..head.len() - proxy_height - edge]);

        for seq in visible {
            let row = cache.rows.get_mut(seq).expect("prepared above");
            if row.lines.is_none() {
                let lines = render(&bare, &[&proxy, &row.cells], false);
                row.lines = Some(lines[edge + proxy_height..lines.len() - edge].to_vec());
                cache.stats.rows_rendered += 1;
            }
            out.extend(row.lines.iter().flatten().cloned());
        }

        // Bottom edge and caption.
        let foot = Frame {
            title: None,
            ..self.frame.clone()
        };
        let foot = render(&foot, &[&proxy], false);
        out.extend_from_slice(&foot[edge + proxy_height..]);
    }
}

/// A core table's lines, as it streams them.
fn table_lines(table: &Table, console: &Console, options: &ConsoleOptions) -> Vec<Vec<Segment>> {
    let mut lines = Vec::new();
    let result: Result<(), Infallible> = table.try_for_each_line(console, options, |line| {
        lines.push(line);
        Ok(())
    });
    match result {
        Ok(()) => lines,
        Err(never) => match never {},
    }
}

impl<K: Eq + Hash + Clone> Renderable for StreamingTable<K> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        crate::event::flatten(self.render_lines(console, options))
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        self.to_table(console).measure(console, options)
    }
}

impl<K: Eq + Hash + Clone> crate::a11y::AccessibleText for StreamingTable<K> {
    fn accessible_text(&self, width: usize) -> String {
        let console = Console::builder().width(width.max(1)).build();
        self.to_table(&console).accessible_text(width)
    }
}

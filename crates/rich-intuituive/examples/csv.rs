//! A CSV viewer in the style of [csvlens](https://github.com/YS-L/csvlens),
//! rebuilt on intuiTUIve: a table over a CSV or TSV file (or stdin), read
//! lazily, with csvlens's keys, its status line and its look.
//!
//! Ported from csvlens (<https://github.com/YS-L/csvlens>), by YS-L and its
//! contributors, MIT licence. Its layout, keys, colours and behaviour are
//! theirs; this rebuild reuses none of its code.
//!
//!     cargo run -p rs-rich-intuituive --example csv -- FILE.csv
//!     some-command | cargo run -p rs-rich-intuituive --example csv
//!
//! Options, as csvlens's: `-d CHAR` (or `-d auto`, `-d '\t'`), `-t` for
//! tabs, `--no-headers`, `-i` to ignore case, `--filter RE`, `--find RE`,
//! `--columns RE`. Without `-d` the delimiter is sniffed from the first
//! lines (`,` `\t` `;` `|`).
//!
//! The file is opened lazily, as csvlens opens it: the first 64 KiB are
//! indexed before the first frame, a background task indexes the rest
//! (the row count shows `N+` until it is done), and only the rows in view
//! are read and parsed, a seek and a read each. Finding, filtering and
//! sorting read the whole file, on a background task: the file is a
//! `TableSource` and what the query asks of it a `view_of` it. The table
//! follows state as it draws (`columns_with`, `freeze_columns_with`,
//! `multiline_when`, `TableLayout`); the prompts are a `command_line`,
//! find and filter sharing a history.
//!
//! csvlens's keys: hjkl or the arrows, Ctrl+F/B (PgDn/PgUp) a window,
//! Ctrl+D/U (d/u) half one, G (End) and g (Home), `<n>G` (or `:<n>`) a
//! line, `/` finds and n/N go to the next and previous match, `&` filters
//! rows, `*` filters columns, Tab cycles row, column and cell selection,
//! `>` `<` resize the selected column, J (Shift+↓) sorts by it (Ctrl+J
//! naturally), `#` and `@` find and filter like the selected cell, `y`
//! copies, Enter (cell mode) prints the cell and exits, `-S`/`-W` wrap
//! lines, `f<n>` freezes columns, `m`/`M` mark rows, Ctrl+E prints the
//! marked rows and exits, `r` resets, H or ? shows the help, q quits.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering as Atomic};
use std::sync::{Arc, Mutex, RwLock};

use intuituive::command_line::{command_line, CommandLine, Mode as Line};
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::{Style, Text};
use intuituive::widgets::{
    view_of, virtual_table_with, Column, Order, RowCount, Scrollbar, Show, TableLayout,
    TableOptions, TableSource, View, ViewQuery,
};
use intuituive::{rule, spawn_stream, HelpOptions};
use rich_ext::transform::{HighlightMatches, KeepLines, TextTransform};

// ---------------------------------------------------------------------
// The file: an index of record boundaries, and rows read on demand.
// ---------------------------------------------------------------------

/// How much of the file is indexed before the first frame.
const FIRST_CHUNK: usize = 64 * 1024;
/// How much the background index reads at a time.
const CHUNK: usize = 1024 * 1024;

/// Push the offset after each line break outside quotes in `bytes`
/// (which starts at `base` in the file) onto `out`. `quoted` carries the
/// quoting state from one chunk to the next: a doubled quote toggles it
/// twice, so `""` inside a quoted field needs no special case.
fn scan(bytes: &[u8], base: u64, quoted: &mut bool, out: &mut Vec<u64>) {
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => *quoted = !*quoted,
            b'\n' if !*quoted => out.push(base + i as u64 + 1),
            _ => {}
        }
    }
}

/// One record's fields, its line break dropped.
pub fn parse_record(bytes: &[u8], delimiter: u8) -> Vec<String> {
    let mut s = bytes;
    if let Some(rest) = s.strip_suffix(b"\n") {
        s = rest;
    }
    if let Some(rest) = s.strip_suffix(b"\r") {
        s = rest;
    }
    let mut fields = Vec::new();
    let mut field = Vec::new();
    let (mut quoted, mut i) = (false, 0);
    while i < s.len() {
        let c = s[i];
        if quoted {
            if c == b'"' {
                if s.get(i + 1) == Some(&b'"') {
                    field.push(b'"');
                    i += 1;
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
        } else if c == b'"' && field.is_empty() {
            quoted = true;
        } else if c == delimiter {
            fields.push(String::from_utf8_lossy(&field).into_owned());
            field.clear();
        } else {
            field.push(c);
        }
        i += 1;
    }
    fields.push(String::from_utf8_lossy(&field).into_owned());
    fields
}

/// The delimiter the first records agree on: of `,` tab `;` `|`, the one
/// found the same number of times (and at least once) on every record,
/// the most often; else the most frequent; else a comma.
pub fn sniff(bytes: &[u8]) -> u8 {
    let mut bounds = vec![0];
    scan(bytes, 0, &mut false, &mut bounds);
    let records: Vec<&[u8]> = bounds
        .windows(2)
        .take(20)
        .map(|w| &bytes[w[0] as usize..w[1] as usize])
        .filter(|r| !r.trim_ascii().is_empty())
        .collect();
    let count = |record: &[u8], d: u8| {
        let mut quoted = false;
        record
            .iter()
            .filter(|&&b| {
                if b == b'"' {
                    quoted = !quoted;
                }
                b == d && !quoted
            })
            .count()
    };
    let mut best: Option<(bool, usize, u8)> = None;
    for &d in b",\t;|" {
        let counts: Vec<usize> = records.iter().map(|r| count(r, d)).collect();
        let total: usize = counts.iter().sum();
        if total == 0 {
            continue;
        }
        let steady = counts.iter().all(|&c| c == counts[0]);
        let score = (steady, if steady { counts[0] } else { total }, d);
        if best.is_none_or(|b| (score.0, score.1) > (b.0, b.1)) {
            best = Some(score);
        }
    }
    best.map_or(b',', |b| b.2)
}

/// Escape a regular expression's special characters: csvlens's `#` and
/// `@` find a cell's value as it is.
fn regex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A CSV file opened for viewing: where each record starts (indexed in the
/// background), and the records in view read from disk as they are asked
/// for.
pub struct Source {
    pub path: PathBuf,
    pub delimiter: u8,
    pub headers: bool,
    /// The column names: the header record's, or 1, 2, 3 without one.
    pub names: Vec<String>,
    /// Record boundaries: record `k` is `bounds[k]..bounds[k + 1]`.
    bounds: Arc<RwLock<Vec<u64>>>,
    /// Where the background index goes on from, and whether it is needed.
    resume: u64,
    complete: bool,
    file: Mutex<File>,
    cache: Mutex<HashMap<usize, Arc<Vec<String>>>>,
    /// How many records were read and parsed for the view: the test's
    /// measure of "only the window is read".
    pub parsed: Arc<AtomicUsize>,
}

impl Source {
    /// Open `path`, sniffing the delimiter unless given, and index the
    /// first [`FIRST_CHUNK`] bytes.
    pub fn open(path: &Path, delimiter: Option<u8>, no_headers: bool) -> io::Result<Source> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let mut head = vec![0; FIRST_CHUNK.min(len as usize)];
        file.read_exact(&mut head)?;
        let tsv = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tsv") || e.eq_ignore_ascii_case("tab"));
        let delimiter = delimiter.unwrap_or_else(|| if tsv { b'\t' } else { sniff(&head) });
        let mut bounds = vec![0];
        scan(&head, 0, &mut false, &mut bounds);
        let complete = head.len() as u64 == len;
        if complete && *bounds.last().unwrap() < len {
            bounds.push(len);
        }
        let resume = *bounds.last().unwrap();
        let first = match bounds.get(1) {
            Some(&end) => parse_record(&head[..end as usize], delimiter),
            None => parse_record(&head, delimiter),
        };
        let names = if no_headers {
            (1..=first.len()).map(|n| n.to_string()).collect()
        } else {
            first
        };
        Ok(Source {
            path: path.to_path_buf(),
            delimiter,
            headers: !no_headers,
            names,
            bounds: Arc::new(RwLock::new(bounds)),
            resume,
            complete,
            file: Mutex::new(file),
            cache: Mutex::new(HashMap::new()),
            parsed: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// The data rows indexed so far.
    pub fn rows(&self) -> usize {
        let records = self.bounds.read().unwrap().len().saturating_sub(1);
        records.saturating_sub(self.headers as usize)
    }

    /// Data row `row` (0-based, after the header), read from disk unless
    /// it was read lately.
    pub fn row(&self, row: usize) -> Arc<Vec<String>> {
        if let Some(cached) = self.cache.lock().unwrap().get(&row) {
            return cached.clone();
        }
        let record = row + self.headers as usize;
        let span = {
            let bounds = self.bounds.read().unwrap();
            bounds
                .get(record)
                .copied()
                .zip(bounds.get(record + 1).copied())
        };
        let fields = match span {
            Some((from, to)) => {
                let mut bytes = vec![0; (to - from) as usize];
                let mut file = self.file.lock().unwrap();
                let read = file
                    .seek(SeekFrom::Start(from))
                    .and_then(|_| file.read_exact(&mut bytes));
                self.parsed.fetch_add(1, Atomic::Relaxed);
                match read {
                    Ok(()) => parse_record(&bytes, self.delimiter),
                    Err(_) => Vec::new(),
                }
            }
            None => Vec::new(),
        };
        let fields = Arc::new(fields);
        let mut cache = self.cache.lock().unwrap();
        if cache.len() > 2048 {
            cache.clear();
        }
        cache.insert(row, fields.clone());
        fields
    }

    /// Index the rest of the file on a background task, telling
    /// `indexed` (data rows, done) as it goes.
    fn index_in_background(&self, indexed: Signal<(usize, bool)>) {
        let headers = self.headers as usize;
        let rows = move |n: usize| n.saturating_sub(1).saturating_sub(headers);
        if self.complete {
            indexed.set((self.rows(), true));
            return;
        }
        indexed.set((self.rows(), false));
        let (path, bounds, resume) = (self.path.clone(), self.bounds.clone(), self.resume);
        spawn_stream(move |stream| {
            let Ok(mut file) = File::open(&path) else {
                return;
            };
            let len = file.metadata().map(|m| m.len()).unwrap_or(0);
            if file.seek(SeekFrom::Start(resume)).is_err() {
                return;
            }
            let (mut at, mut quoted) = (resume, false);
            let mut buffer = vec![0; CHUNK];
            let mut found = Vec::new();
            loop {
                if stream.cancelled() {
                    return;
                }
                let n = match file.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                found.clear();
                scan(&buffer[..n], at, &mut quoted, &mut found);
                at += n as u64;
                let count = {
                    let mut bounds = bounds.write().unwrap();
                    bounds.extend_from_slice(&found);
                    bounds.len()
                };
                stream
                    .proxy()
                    .run(move || indexed.set((rows(count), false)));
            }
            let count = {
                let mut bounds = bounds.write().unwrap();
                if *bounds.last().unwrap() < len {
                    bounds.push(len);
                }
                bounds.len()
            };
            stream.proxy().run(move || indexed.set((rows(count), true)));
        });
    }
}

/// The rows indexed so far, as a table and a view read them: `N+` until
/// the background index is done.
pub struct Indexed {
    src: Arc<Source>,
    indexed: Signal<(usize, bool)>,
}

impl TableSource for Indexed {
    fn len(&self) -> RowCount {
        match self.indexed.get() {
            (n, true) => RowCount::Exact(n),
            (n, false) => RowCount::AtLeast(n),
        }
    }

    fn row(&self, i: usize) -> Vec<String> {
        self.src.row(i).to_vec()
    }
}

// ---------------------------------------------------------------------
// Finding, filtering and sorting: a view of the file, on a worker.
// ---------------------------------------------------------------------

/// How the rows are sorted: by a data column, which way, and whether
/// naturally ("file2" before "file10").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sort {
    pub column: usize,
    pub descending: bool,
    pub natural: bool,
}

/// What the view asks of the whole file: rows kept by a regex (`&`), rows
/// found by one (`/`), and a sort. Find and filter are one finder in
/// csvlens, so at most one of them is set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Query {
    pub filter: Option<String>,
    pub find: Option<String>,
    pub sort: Option<Sort>,
}

/// The pattern as csvlens matches it: case ignored with `-i` unless the
/// pattern has a capital letter.
fn pattern(re: &str, ignore_case: bool) -> String {
    if ignore_case && !re.chars().any(char::is_uppercase) {
        format!("(?i){re}")
    } else {
        re.to_string()
    }
}

/// A row test for the view: a match of `re` in a cell.
fn row_test(re: &str, ignore_case: bool) -> Option<impl Fn(&[String]) -> bool + Send + Sync> {
    let keep = KeepLines::new(&pattern(re, ignore_case)).ok()?;
    Some(move |cells: &[String]| {
        // A cell a line, so a match never runs from one into the next.
        let lines: Vec<String> = cells.iter().map(|c| c.replace(['\n', '\r'], " ")).collect();
        keep.transform(Text::new(lines.join("\n")))
            .is_ok_and(|t| !t.plain().is_empty())
    })
}

/// The view's query for the app's.
fn view_query(query: &Query, ignore_case: bool) -> ViewQuery {
    let mut view = ViewQuery::default();
    if let Some(test) = query
        .filter
        .as_deref()
        .and_then(|re| row_test(re, ignore_case))
    {
        view = view.keep(test);
    }
    if let Some(test) = query
        .find
        .as_deref()
        .and_then(|re| row_test(re, ignore_case))
    {
        view = view.find(test);
    }
    if let Some(sort) = query.sort {
        let order = match sort.descending {
            true => Order::Descending,
            false => Order::Ascending,
        };
        view = view.sort(sort.column, order);
        if sort.natural {
            view = view.compare(natural_cmp);
        }
    }
    view
}

/// "file10" after "file2": digit runs compare as numbers.
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let run = |s: &[u8]| s.iter().take_while(|c| c.is_ascii_digit()).count();
                let (ra, rb) = (run(a), run(b));
                let na = std::str::from_utf8(&a[..ra])
                    .unwrap()
                    .trim_start_matches('0');
                let nb = std::str::from_utf8(&b[..rb])
                    .unwrap()
                    .trim_start_matches('0');
                let by = na.len().cmp(&nb.len()).then_with(|| na.cmp(nb));
                if by.is_ne() {
                    return by;
                }
                a = &a[ra..];
                b = &b[rb..];
            }
            (Some(x), Some(y)) => {
                let by = x.cmp(y);
                if by.is_ne() {
                    return by;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

// ---------------------------------------------------------------------
// The state.
// ---------------------------------------------------------------------

/// csvlens's selection modes, cycled by Tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Row,
    Column,
    Cell,
}

/// csvlens's dark theme.
const GUTTER: &str = "rgb(131,148,150)";
const SELECTED: &str = "bold rgb(192,192,192) on rgb(62,61,50)";
const MARKED: &str = "rgb(220,230,255) on rgb(40,50,80)";
const FOUND: &str = "rgb(200,0,0)";

/// What the app is started with: the command line's options.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub path: PathBuf,
    /// What the status line calls it: the file name, or `stdin`.
    pub name: String,
    pub delimiter: Option<u8>,
    pub no_headers: bool,
    pub ignore_case: bool,
    pub filter: Option<String>,
    pub find: Option<String>,
    pub columns: Option<String>,
    /// A counter of the rows read and parsed for the view, to share with
    /// the caller (a test's measure); the source keeps its own without.
    pub parsed: Option<Arc<AtomicUsize>>,
}

/// The finder's pattern, compiled once for the cells in view (`None`
/// when it does not compile).
type Highlighter = (String, Option<HighlightMatches>);

/// The app's state, every piece a signal but the file itself.
#[derive(Clone)]
struct Lens {
    src: Arc<Source>,
    name: Rc<str>,
    ignore_case: bool,
    /// Data rows indexed so far, and whether that is all of them.
    indexed: Signal<(usize, bool)>,
    query: Signal<Query>,
    /// The file through the query: the rows kept, in their order, and
    /// those found.
    view: View<Indexed>,
    /// The column filter (`*`): its pattern and the data columns shown.
    columns: Signal<Option<(String, Rc<Vec<usize>>)>>,
    freeze: Signal<usize>,
    mode: Signal<Mode>,
    /// Widths set with `>` and `<`, by data column.
    widths: Signal<Rc<HashMap<usize, u16>>>,
    /// `-S`/`-W`: rows as tall as their cells.
    wrap: Signal<bool>,
    marks: Signal<Rc<BTreeSet<usize>>>,
    message: Signal<Option<String>>,
    selected: Signal<usize>,
    offset: Signal<usize>,
    /// The cell cursor's table column (the gutter is column 0).
    cursor: Signal<usize>,
    /// What the table drew: its columns' widths and rows in view.
    layout: TableLayout,
    /// The prompts, in place of the status line.
    cmd: CommandLine,
    highlighter: Rc<RefCell<Option<Highlighter>>>,
}

impl Lens {
    fn new(src: Source, options: &Options) -> Lens {
        let names = src.names.len();
        let src = Arc::new(src);
        let indexed = signal((0, false));
        let query = signal(Query {
            filter: options.filter.clone(),
            find: options.find.clone().filter(|_| options.filter.is_none()),
            sort: None,
        });
        let ignore_case = options.ignore_case;
        let asked = memo(move || view_query(&query.get(), ignore_case));
        let source = Indexed {
            src: src.clone(),
            indexed,
        };
        let lens = Lens {
            src,
            name: options.name.clone().into(),
            ignore_case,
            indexed,
            query,
            view: view_of(source, move || asked.get()),
            columns: signal(None),
            freeze: signal(0),
            mode: signal(Mode::Row),
            widths: signal(Rc::new(HashMap::new())),
            wrap: signal(false),
            marks: signal(Rc::new(BTreeSet::new())),
            message: signal(None),
            selected: signal(0),
            offset: signal(0),
            cursor: signal(1.min(names)),
            layout: TableLayout::new(),
            cmd: CommandLine::new(),
            highlighter: Rc::new(RefCell::new(None)),
        };
        if let Some(re) = &options.columns {
            lens.filter_columns(re);
        }
        lens
    }

    /// Rows in view.
    fn len(&self) -> usize {
        self.view.len().count()
    }

    /// The record (data row in the file) at position `at` in view.
    fn record(&self, at: usize) -> usize {
        self.view.source_row(at).unwrap_or(at)
    }

    fn shown(&self) -> Rc<Vec<usize>> {
        self.columns
            .get()
            .map(|(_, shown)| shown)
            .unwrap_or_else(|| {
                let all: Vec<usize> = (0..self.src.names.len()).collect();
                Rc::new(all)
            })
    }

    /// The data column under the cell cursor, outside row mode.
    fn selected_column(&self) -> Option<usize> {
        if self.mode.get_untracked() == Mode::Row {
            return None;
        }
        let at = self.cursor.get_untracked().checked_sub(1)?;
        self.shown().get(at).copied()
    }

    /// The table's rows in view.
    fn page(&self) -> usize {
        self.layout.rows_in_view().max(1)
    }

    fn select(&self, at: usize) {
        let last = self.len().saturating_sub(1);
        self.selected.set(at.min(last));
    }

    fn step(&self, by: isize) {
        let now = self.selected.get_untracked() as isize;
        self.select((now + by).max(0) as usize);
    }

    /// The pattern the cells' matches are drawn for: the finder's.
    fn highlight_pattern(&self) -> Option<String> {
        self.query
            .with(|q| q.find.clone().or_else(|| q.filter.clone()))
            .map(|re| pattern(&re, self.ignore_case))
    }

    /// `cell` as markup, the finder's matches in red.
    fn cell_markup(&self, cell: &str) -> String {
        let cell = cell.replace(['\n', '\r'], " ");
        let Some(re) = self.highlight_pattern() else {
            return escape(&cell);
        };
        let mut slot = self.highlighter.borrow_mut();
        if slot.as_ref().is_none_or(|(p, _)| *p != re) {
            let style = Style::parse(FOUND).expect("a style");
            *slot = Some((re.clone(), HighlightMatches::new(&re, style).ok()));
        }
        match slot.as_ref() {
            Some((_, Some(highlight))) => highlight
                .transform(Text::new(&cell))
                .map_or_else(|_| escape(&cell), |text| text.markup()),
            _ => escape(&cell),
        }
    }

    /// The table row at `at` in view: the row number, then the cells shown.
    fn table_row(&self, at: usize, shown: &[usize], digits: usize) -> Vec<String> {
        let record = self.record(at);
        let fields = self.src.row(record);
        let number = format!("{:<digits$}", record + 1);
        let number = if self.selected.get() == at {
            format!("[bold underline]{number}[/]")
        } else {
            number
        };
        let mut cells = vec![format!("[{GUTTER}]{number}  │[/]")];
        for &c in shown {
            let cell = fields.get(c).map(String::as_str).unwrap_or("");
            // Two cells of padding and the table's two between columns:
            // csvlens's four.
            cells.push(format!("{}  ", self.cell_markup(cell)));
        }
        cells
    }

    /// The table's columns: the gutter, then the columns shown, fitted to
    /// their titles and the cells in view up to csvlens's cap of 30%, or
    /// as wide as `>`/`<` set them; the sorted one's title with its arrow.
    fn columns(&self) -> Vec<Column> {
        let sort = self.query.with(|q| q.sort);
        let widths = self.widths.get();
        let mut columns = vec![Column::new("", Size::Auto)];
        for &c in self.shown().iter() {
            let mut title = self.src.names[c].clone();
            if let Some(sort) = sort.filter(|s| s.column == c) {
                let arrow = if sort.descending { "▾" } else { "▴" };
                let natural = if sort.natural { "N" } else { "" };
                title = format!("{title} [{arrow}{natural}]");
            }
            let title = format!("{}  ", escape(&title));
            columns.push(match widths.get(&c) {
                Some(&w) => Column::new(&title, Size::Fixed(w)),
                None => Column::new(&title, Size::Auto).max_percent(30),
            });
        }
        columns
    }

    fn filter_columns(&self, re: &str) {
        if re.is_empty() {
            self.columns.set(None);
            return;
        }
        let shown: Vec<usize> = match KeepLines::new(&pattern(re, self.ignore_case)) {
            Ok(keep) => (0..self.src.names.len())
                .filter(|&c| {
                    let name = Text::new(&self.src.names[c]);
                    keep.transform(name).is_ok_and(|t| !t.plain().is_empty())
                })
                .collect(),
            Err(_) => {
                self.message.set(Some(format!("Invalid regex: {re}")));
                return;
            }
        };
        let shown = if shown.is_empty() {
            (0..self.src.names.len()).collect()
        } else {
            shown
        };
        self.columns.set(Some((re.to_string(), Rc::new(shown))));
        self.cursor.set(1);
    }

    /// The status line, csvlens's: the file, the row and column, then the
    /// finder's, the column filter's and the sort's state.
    fn status(&self) -> String {
        if let Some(message) = self.message.get() {
            return escape(&message);
        }
        let (rows, done) = self.indexed.get();
        let len = self.len();
        let at = self.selected.get();
        let row = if len == 0 {
            "-".to_string()
        } else {
            (self.record(at) + 1).to_string()
        };
        let plus = if done { "" } else { "+" };
        let shown = self.shown();
        let col = match self.mode.get() {
            Mode::Row => 1,
            _ => self.cursor.get().max(1),
        };
        let mut line = format!(
            "{} [Row {row}/{rows}{plus}, Col {col}/{}]",
            self.name,
            shown.len()
        );
        let query = self.query.get();
        let busy = self.view.is_busy();
        if let Some(re) = &query.find {
            let state = match self.view.place(at) {
                _ if busy => "Finding...".to_string(),
                (_, 0) => "Not found".to_string(),
                (0, n) => format!("-/{n}"),
                (i, n) => format!("{i}/{n}"),
            };
            line += &format!(" [Find \"{re}\": {state}]");
        }
        if let Some(re) = &query.filter {
            let state = if busy {
                "Finding...".to_string()
            } else if len == 0 {
                "Not found".to_string()
            } else {
                format!("{}/{len}", at + 1)
            };
            line += &format!(" [Filter \"{re}\": {state}]");
        }
        if let Some((re, shown)) = self.columns.get() {
            line += &format!(
                " [Filter \"{re}\": {}/{} cols]",
                shown.len(),
                self.src.names.len()
            );
        }
        if let (Some(sort), true) = (query.sort, busy) {
            let kind = if sort.natural { "natural" } else { "auto" };
            line += &format!(" [Sorting by {} ({kind})...]", self.src.names[sort.column]);
        }
        if self.ignore_case {
            line += " [ignore-case]";
        }
        escape(&line)
    }
}

// ---------------------------------------------------------------------
// The view.
// ---------------------------------------------------------------------

/// The table in `mode`: the row numbers as a frozen first column, then the
/// columns shown. Everything else it reads from state as it draws.
fn table(l: &Lens, mode: Mode) -> Node {
    let columns = {
        let l = l.clone();
        move || l.columns()
    };
    let (freeze, wrap) = (l.freeze, l.wrap);
    let mut options = TableOptions::default()
        .columns_with(columns)
        .freeze_columns_with(move || 1 + freeze.get())
        .multiline_when(move || wrap.get())
        .column_spacing(2)
        .offset(l.offset)
        .layout(&l.layout)
        .row_style({
            let l = l.clone();
            move |at| {
                let record = l.record(at);
                l.marks
                    .with(|m| m.contains(&record))
                    .then(|| MARKED.to_string())
            }
        });
    if mode != Mode::Row {
        options = options.cells(l.cursor);
    }
    if mode == Mode::Column {
        options = options.column_highlight();
    }
    let rows = {
        let l = l.clone();
        move |at: usize| {
            // csvlens sizes the gutter to the largest row number in view.
            let first = l.offset.get();
            let last = (first + l.page()).min(l.len());
            let widest = (first..last).map(|at| l.record(at) + 1).max().unwrap_or(1);
            l.table_row(at, &l.shown(), widest.to_string().len())
        }
    };
    let len = {
        let l = l.clone();
        move || l.len()
    };
    let (row_selected, cell_selected) = match mode {
        Mode::Row => (SELECTED, "none"),
        _ => ("none", SELECTED),
    };
    virtual_table_with(Vec::new(), len, rows, l.selected, options)
        .scrollbar(Scrollbar::new().show(Show::Never))
        .theme_styles([
            ("table.header", "bold"),
            ("selected", row_selected),
            ("table.selected", row_selected),
            ("selected.cell", cell_selected),
            ("selected.column", cell_selected),
        ])
        .name("table")
}

/// Where a prompt in the status line leads.
#[derive(Clone, Copy, PartialEq)]
enum Prompt {
    Find,
    Filter,
    Columns,
    Goto,
}

/// csvlens's input, in the status line: a prefix and the text typed. Find
/// and filter share a history, as csvlens's do.
fn prompt(l: &Lens, cx: &mut Ctx, kind: Prompt, initial: &str) {
    let prefix = match kind {
        Prompt::Find => "Find: ",
        Prompt::Filter => "Filter: ",
        Prompt::Columns => "Columns regex: ",
        Prompt::Goto => "Go to line: ",
    };
    let submit = l.clone();
    let mut line = Line::new(prefix)
        .initial(initial)
        .on_submit(move |value, _| {
            submitted(&submit, kind, value.trim());
            Ok(())
        });
    if matches!(kind, Prompt::Find | Prompt::Filter) {
        line = line.history("finder");
    }
    if kind == Prompt::Goto {
        // `<n>G`: G (or g) ends the number as Enter does.
        let ends = l.clone();
        line = line.on_change(move |value, cx| {
            if let Some(number) = value.strip_suffix(['G', 'g']) {
                let number = number.trim().to_string();
                ends.cmd.close(cx);
                submitted(&ends, Prompt::Goto, &number);
            }
        });
    }
    l.cmd.open(cx, line);
}

fn submitted(l: &Lens, kind: Prompt, value: &str) {
    match kind {
        Prompt::Goto => {
            if let Ok(n) = value.parse::<usize>() {
                let record = n.saturating_sub(1);
                let at = (0..l.len()).find(|&at| l.record(at) == record);
                match at {
                    Some(at) => l.select(at),
                    None => l.message.set(Some(format!("Line {n} is not in view"))),
                }
            }
        }
        Prompt::Columns => l.filter_columns(value),
        Prompt::Find | Prompt::Filter => {
            if value.is_empty() {
                return;
            }
            if KeepLines::new(&pattern(value, l.ignore_case)).is_err() {
                l.message.set(Some(format!("Invalid regex: {value}")));
                return;
            }
            l.query.update(|q| {
                let re = Some(value.to_string());
                if kind == Prompt::Find {
                    (q.find, q.filter) = (re, None);
                } else {
                    (q.filter, q.find) = (re, None);
                }
            });
        }
    }
}

/// n and N: the next or the previous row found.
fn next_found(l: &Lens, forward: bool) {
    let now = l.selected.get_untracked();
    let to = match forward {
        true => l.view.next_found(now),
        false => l.view.previous_found(now),
    };
    if let Some(to) = to {
        l.select(to);
    }
}

fn sort(l: &Lens, natural: bool) {
    let Some(column) = l.selected_column() else {
        l.message
            .set(Some("Press TAB and select a column before sorting".into()));
        return;
    };
    if !l.indexed.get_untracked().1 {
        l.message.set(Some(
            "Sorting is not supported when data is still streaming".into(),
        ));
        return;
    }
    l.query.update(|q| {
        q.sort = match q.sort {
            Some(s) if s.column == column && s.natural == natural => Some(Sort {
                descending: !s.descending,
                ..s
            }),
            _ => Some(Sort {
                column,
                descending: false,
                natural,
            }),
        }
    });
}

/// The selected cell's value, in cell mode.
fn selected_cell(l: &Lens) -> Option<String> {
    if l.mode.get_untracked() != Mode::Cell {
        return None;
    }
    let column = l.selected_column()?;
    let fields = l.src.row(l.record(l.selected.get_untracked()));
    fields.get(column).cloned()
}

/// The marked rows, with the header, as CSV: what Ctrl+E prints.
fn marked_rows(l: &Lens) -> String {
    let d = (l.src.delimiter as char).to_string();
    let quote = |f: &String| {
        if f.contains([l.src.delimiter as char, '"', '\n']) {
            format!("\"{}\"", f.replace('"', "\"\""))
        } else {
            f.clone()
        }
    };
    let line = |fields: &[String]| fields.iter().map(quote).collect::<Vec<_>>().join(&d);
    let mut out = Vec::new();
    if l.src.headers {
        out.push(line(&l.src.names));
    }
    for &record in l.marks.get_untracked().iter() {
        out.push(line(&l.src.row(record)));
    }
    out.join("\n")
}
/// What a key does.
#[derive(Clone, Copy)]
enum Act {
    Down(Step),
    Up(Step),
    /// The cell cursor across: by columns, or to the ends.
    Across(isize),
    Bottom,
    Top,
    Goto,
    Find,
    Next,
    Previous,
    Filter,
    FilterColumns,
    Mode,
    Wider,
    Narrower,
    Sort,
    SortNatural,
    FindLike,
    FilterLike,
    Copy,
    Select,
    Wrap,
    Reset,
    Help,
    Mark,
    ClearMarks,
    PrintMarks,
    Quit,
}

#[derive(Clone, Copy)]
enum Step {
    Row,
    Page,
    HalfPage,
}

/// csvlens's keys, by its help's sections: the section, the keys, what
/// the help says, and the action. Two keys in a row (`minus S`) are a
/// sequence.
#[rustfmt::skip]
const KEYS: &[(&str, &str, &str, Act)] = &[
    ("Moving", "j down", "Scroll one row down", Act::Down(Step::Row)),
    ("Moving", "k up", "Scroll one row up", Act::Up(Step::Row)),
    ("Moving", "ctrl+f pagedown", "Scroll one window down", Act::Down(Step::Page)),
    ("Moving", "ctrl+b pageup", "Scroll one window up", Act::Up(Step::Page)),
    ("Moving", "ctrl+d d", "Scroll half a window down", Act::Down(Step::HalfPage)),
    ("Moving", "ctrl+u u", "Scroll half a window up", Act::Up(Step::HalfPage)),
    ("Moving", "ctrl+h", "Scroll one window left", Act::Across(-4)),
    ("Moving", "ctrl+l", "Scroll one window right", Act::Across(4)),
    ("Moving", "ctrl+left", "Scroll left to first column", Act::Across(isize::MIN)),
    ("Moving", "ctrl+right", "Scroll right to last column", Act::Across(isize::MAX)),
    ("Moving", "G end", "Go to bottom", Act::Bottom),
    ("Moving", "g home", "Go to top", Act::Top),
    ("Moving", "colon", "Go to line n (or <n>G)", Act::Goto),
    ("Search", "slash", "Find content matching regex and highlight matches", Act::Find),
    ("Search", "n", "Jump to next result", Act::Next),
    ("Search", "N", "Jump to previous result", Act::Previous),
    ("Search", "ampersand", "Filter rows using regex (show only matches)", Act::Filter),
    ("Search", "asterisk", "Filter columns using regex (show only matches)", Act::FilterColumns),
    ("Selection modes", "tab", "Toggle between row, column or cell selection modes", Act::Mode),
    ("Selection modes", "gt", "Increase selected column's width", Act::Wider),
    ("Selection modes", "lt", "Decrease selected column's width", Act::Narrower),
    ("Selection modes", "J shift+down", "Sort rows by the selected column", Act::Sort),
    ("Selection modes", "ctrl+j", "Sort rows by the selected column (natural)", Act::SortNatural),
    ("Selection modes", "hash", "Find and highlight rows like the selected cell", Act::FindLike),
    ("Selection modes", "at", "Filter rows like the selected cell", Act::FilterLike),
    ("Selection modes", "y", "Copy the selected row or cell to clipboard", Act::Copy),
    ("Selection modes", "enter", "Print the selected cell to stdout and exit", Act::Select),
    ("Other options", "minus S", "Toggle line wrapping", Act::Wrap),
    ("Other options", "minus W", "Toggle line wrapping by words", Act::Wrap),
    ("Other options", "r", "Reset to default view", Act::Reset),
    ("Other options", "H question", "Display this help", Act::Help),
    ("Other options", "m", "Mark / unmark the selected row visually", Act::Mark),
    ("Other options", "M", "Clear all row marks", Act::ClearMarks),
    ("Other options", "ctrl+e", "Print the marked rows (with header) to stdout and exit", Act::PrintMarks),
    ("Other options", "q", "Exit", Act::Quit),
];

fn run(l: &Lens, act: Act, cx: &mut Ctx) {
    l.message.set(None);
    let page = l.page() as isize;
    let rows = |step| match step {
        Step::Row => 1,
        Step::Page => page,
        Step::HalfPage => page / 2,
    };
    match act {
        Act::Down(step) => l.step(rows(step)),
        Act::Up(step) => l.step(-rows(step)),
        Act::Across(by) => {
            let last = l.shown().len() as isize;
            l.cursor
                .update(|c| *c = (*c as isize).saturating_add(by).clamp(1, last.max(1)) as usize)
        }
        Act::Bottom => l.select(usize::MAX),
        Act::Top => l.select(0),
        Act::Goto => prompt(l, cx, Prompt::Goto, ""),
        Act::Find => prompt(l, cx, Prompt::Find, ""),
        Act::Next => next_found(l, true),
        Act::Previous => next_found(l, false),
        Act::Filter => prompt(l, cx, Prompt::Filter, ""),
        Act::FilterColumns => prompt(l, cx, Prompt::Columns, ""),
        Act::Mode => l.mode.update(|m| {
            *m = match m {
                Mode::Row => Mode::Column,
                Mode::Column => Mode::Cell,
                Mode::Cell => Mode::Row,
            }
        }),
        Act::Wider => resize(l, 4),
        Act::Narrower => resize(l, -4),
        Act::Sort => sort(l, false),
        Act::SortNatural => sort(l, true),
        Act::FindLike => like_cell(l, false),
        Act::FilterLike => like_cell(l, true),
        Act::Copy => {
            let text = selected_cell(l).unwrap_or_else(|| {
                let row = l.src.row(l.record(l.selected.get_untracked()));
                row.join(&(l.src.delimiter as char).to_string())
            });
            l.message.set(Some(format!(
                "Copied {} to clipboard",
                text.replace('\n', "")
            )));
            cx.copy(text);
        }
        Act::Select => {
            if let Some(cell) = selected_cell(l) {
                cx.quit_with(cell);
            }
        }
        Act::Wrap => l.wrap.update(|w| *w = !*w),
        Act::Reset => {
            l.widths.set(Rc::new(HashMap::new()));
            l.columns.set(None);
            l.query.set(Query::default());
        }
        Act::Help => cx.help_with(HelpOptions::new().title("csvlens")),
        Act::Mark => {
            let record = l.record(l.selected.get_untracked());
            l.marks.update(|m| {
                let m = Rc::make_mut(m);
                if !m.remove(&record) {
                    m.insert(record);
                }
            });
        }
        Act::ClearMarks => l.marks.set(Rc::new(BTreeSet::new())),
        Act::PrintMarks => cx.quit_with(marked_rows(l)),
        Act::Quit => cx.quit(),
    }
}

/// The keys on `root`, in csvlens's help sections. The keys that move the
/// cell cursor apply outside row mode only: in row mode the table's own
/// h/l scroll it across.
fn keymap(l: &Lens, mut root: Node) -> Node {
    let mode = l.mode;
    let cell_modes = move || mode.get_untracked() != Mode::Row;
    for &(section, keys, description, act) in KEYS {
        let l = l.clone();
        let handler = move |cx: &mut Ctx| run(&l, act, cx);
        root = match act {
            Act::Across(_) => root.section(section, |n| {
                n.bind_when(cell_modes, keys, description, handler)
            }),
            _ if keys.starts_with("minus ") => {
                root.bind_sequence_in(section, keys, description, handler)
            }
            _ => root.section(section, |n| n.bind(keys, description, handler)),
        };
    }
    // f<n>: freeze n columns, the first sequence listed for all ten.
    for n in 0..=9usize {
        let l = l.clone();
        let freeze = move |_: &mut Ctx| {
            l.message.set(None);
            l.freeze.set(n.min(l.shown().len().saturating_sub(1)));
        };
        root = match n {
            1 => root.bind_sequence_in(
                "Other options",
                "f 1",
                "Freeze this number of columns from the left (f<n>)",
                freeze,
            ),
            _ => root.on_sequence(&format!("f {n}"), freeze),
        };
    }
    // <n>G: a digit starts the line number.
    for n in 0..=9 {
        let l = l.clone();
        root = root.on_key(&n.to_string(), move |cx| {
            l.message.set(None);
            prompt(&l, cx, Prompt::Goto, &n.to_string())
        });
    }
    root
}

fn resize(l: &Lens, by: i32) {
    let Some(column) = l.selected_column() else {
        return;
    };
    let at = l.cursor.get_untracked();
    let Some(now) = l.layout.width_of(at) else {
        return;
    };
    let to = now as i32 + by;
    if to > 0 {
        l.widths.update(|w| {
            Rc::make_mut(w).insert(column, to as u16);
        });
    }
}

fn like_cell(l: &Lens, filter: bool) {
    let Some(value) = selected_cell(l) else {
        l.message.set(Some(
            "Select a cell first before finding (#) or filtering (@) rows like it".into(),
        ));
        return;
    };
    let re = Some(format!("^{}$", regex_escape(&value)));
    l.query.update(|q| {
        if filter {
            (q.filter, q.find) = (re, None);
        } else {
            (q.find, q.filter) = (re, None);
        }
    });
}

/// csvlens's screen: a rule, the table, a rule and the status line (or
/// the prompt in its place).
fn view(l: &Lens) -> Node {
    let tables = {
        let (l, mode) = (l.clone(), l.mode);
        keyed(move || mode.get(), move |mode| table(&l, mode))
    };
    let cmd = l.cmd;
    let status = {
        let l = l.clone();
        text(move || l.status())
            .no_wrap()
            .style(GUTTER)
            .fixed(1)
            .hidden_when(move || cmd.is_open())
    };
    let line = command_line(cmd)
        .style(GUTTER)
        .fixed(1)
        .shown_when(move || cmd.is_open());
    let body = column([rule(), tables.flex(1), rule(), status, line]).previews_keys();
    keymap(l, body)
}

fn theme() -> Theme {
    let mut theme = Theme::dark();
    theme.border = Style::parse(GUTTER).expect("a style");
    theme.border_focused = theme.border.clone();
    theme
}

/// The app over `options.path`, or why the file would not open.
pub fn csv_app(options: Options) -> io::Result<App> {
    let app = App::try_new(move || -> io::Result<Node> {
        let mut src = Source::open(&options.path, options.delimiter, options.no_headers)?;
        if let Some(parsed) = &options.parsed {
            src.parsed = parsed.clone();
        }
        let l = Lens::new(src, &options);
        l.src.index_in_background(l.indexed);
        // After each pass over the file: a filter or a sort starts at the
        // top, and csvlens goes to the first row found from where it is.
        {
            let l2 = l.clone();
            watch(
                move || l2.view.is_busy(),
                move |busy, _| {
                    if busy {
                        return;
                    }
                    let query = l.query.get_untracked();
                    if query.filter.is_some() || query.sort.is_some() {
                        l.selected.set(0);
                    }
                    let now = l.selected.get_untracked();
                    let found = l.view.found();
                    if let Some(&first) = found.iter().find(|&&at| at >= now).or(found.first()) {
                        l.selected.set(first);
                    }
                },
            );
        }
        // Keep the cell cursor among the columns shown.
        {
            let (cursor, l2) = (l.cursor, l.clone());
            watch(
                move || l2.shown().len(),
                move |n, _| cursor.update(|c| *c = (*c).clamp(1, n.max(1))),
            );
        }
        Ok(view(&l))
    })?;
    Ok(app.theme(theme()))
}

/// Stdin, saved to a temporary file csvlens-style, so it can be read
/// lazily like any other.
fn stdin_to_file() -> io::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("csvlens-stdin-{}.csv", std::process::id()));
    let mut out = File::create(&path)?;
    io::copy(&mut io::stdin().lock(), &mut out)?;
    Ok(path)
}

#[allow(dead_code)]
fn main() -> io::Result<()> {
    use std::io::IsTerminal;
    let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidInput, e);
    let mut options = Options::default();
    let mut path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .ok_or_else(|| invalid(format!("{name} takes a value")))
        };
        match arg.as_str() {
            "-d" | "--delimiter" => {
                let d = value("-d")?;
                options.delimiter = match d.as_str() {
                    "auto" => None,
                    "\\t" | "t" => Some(b'\t'),
                    d if d.len() == 1 => Some(d.as_bytes()[0]),
                    _ => return Err(invalid(format!("bad delimiter {d:?}"))),
                };
            }
            "-t" | "--tab-separated" => options.delimiter = Some(b'\t'),
            "--no-headers" => options.no_headers = true,
            "-i" | "--ignore-case" => options.ignore_case = true,
            "--filter" => options.filter = Some(value("--filter")?),
            "--find" => options.find = Some(value("--find")?),
            "--columns" => options.columns = Some(value("--columns")?),
            _ => path = Some(PathBuf::from(arg)),
        }
    }
    let stdin = path.is_none() && !io::stdin().is_terminal();
    let path = match path {
        Some(path) => path,
        None if stdin => stdin_to_file()?,
        None => {
            return Err(invalid(
                "usage: csv [OPTIONS] FILE (or pipe a CSV in)".into(),
            ))
        }
    };
    options.name = if stdin {
        "stdin".into()
    } else {
        path.display().to_string()
    };
    options.path = path.clone();
    let printed = csv_app(options)?.run_for::<String>();
    if stdin {
        let _ = std::fs::remove_file(&path);
    }
    if let Some(out) = printed? {
        println!("{out}");
    }
    Ok(())
}

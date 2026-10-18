//! Nested profiles of JSON records (#705): every path to a value, how many
//! records hold it, and the mix of JSON types found there.
//!
//! A [`JsonProfiler`] walks each record (one JSON value per line, as
//! [`jsonl`](crate::jsonl) reads them) and profiles each **path** as a
//! column: `user.id` for a key inside an object, `tags[]` for the items of an
//! array and `items[].sku` for a key of the objects in one. A record that is
//! not an object is the path `value`, as in a flat profile.
//!
//! **Path syntax.** A key is written as it is unless it is empty, has
//! whitespace at either end, or holds `.`, `[`, `]`, `"`, `\` or a control
//! character; then it is a JSON string in brackets, as JSONPath and jq write
//! it: `meta["k8s.io/name"]`, `["a.b"].c`, `[""]`. [`path_key`] writes one.
//!
//! **Bounded.** A path has at most [`ProfileOptions::depth`] steps (a key or
//! `[]`; default [`DEFAULT_DEPTH`]): an object or array at the last step is
//! not walked into, and its compact JSON text is its value, as in a flat
//! profile. At most [`ProfileOptions::max_paths`] paths are profiled
//! (default [`DEFAULT_MAX_PATHS`]), in the order they are first seen; paths
//! seen after that are named (the first hundred) and noted, and nothing
//! below them is walked. Each path keeps at most [`ProfileOptions::sample`]
//! of its values, a fixed-seed reservoir as [`Profiler`](super::Profiler)
//! keeps rows. Presence, type counts, nulls and the missing-value map count
//! every record. Time is linear in the input; memory is the paths and their
//! samples, however long it is.
//!
//! **Presence and types.** For each path the profile counts the records
//! that hold it ([`ColumnProfile::present`](super::ColumnProfile::present))
//! apart from those that do not
//! ([`absent`](super::ColumnProfile::absent)); an explicit `null` is
//! present, and counted in `nulls`. It counts each value's JSON type
//! ([`TypeCounts`]: string, number, boolean, object, array, null), so a
//! field that is sometimes a number and sometimes a string says so instead
//! of being one inferred type; such a path is **mixed**. Only JSON's `null`
//! is null: the null tokens of a flat profile (`NA`, an empty string) are
//! strings here.
//!
//! ```
//! use rich_data::profile::{Profile, ProfileOptions};
//!
//! let text = r#"{"id": 1, "user": {"name": "ada"}, "tags": ["a", "b"]}
//! {"id": "2", "user": null}
//! not json
//! {"id": 3, "tags": []}
//! "#;
//! let profile = Profile::from_json_lines(text.as_bytes(), ProfileOptions::default()).unwrap();
//! assert!(profile.is_nested());
//! let names: Vec<&str> = profile.columns().iter().map(|c| c.name.as_str()).collect();
//! assert_eq!(names, ["id", "user", "user.name", "tags", "tags[]"]);
//!
//! let id = profile.column("id").unwrap();
//! assert_eq!(id.types.unwrap().mix(), "num 67% · str 33%");
//! assert_eq!(id.mixed, Some(true));
//! let user = profile.column("user").unwrap();
//! assert_eq!((user.present, user.absent, user.nulls), (Some(2), Some(1), 1));
//! assert_eq!(profile.column("tags[]").unwrap().values, Some(2));
//! assert_eq!((profile.malformed(), profile.malformed_lines()), (1, &[3][..]));
//! ```

use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap};
use std::io::BufRead;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use super::{
    describe, grouped, percent, plural, MissingBucket, MissingMap, Profile, ProfileOptions, Rules,
};
use crate::infer::Inferrer;
use crate::jsonl::Lines;
use crate::record::cell;
use crate::stats::{Stats, StatsOptions};
use crate::{DataError, Rows, Value};

/// The deepest path a nested profile describes by default, in steps.
pub const DEFAULT_DEPTH: usize = 4;
/// The most paths a nested profile describes by default.
pub const DEFAULT_MAX_PATHS: usize = 200;
/// The most malformed line numbers a profile keeps.
pub const MAX_MALFORMED_LINES: usize = 10;
/// The most paths past the cap a profile names.
const MAX_DROPPED: usize = crate::record::MAX_UNKNOWN_KEYS;
/// The most names a note lists.
const NOTE_NAMES: usize = 10;

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// How many values at a path were each JSON type. Serialised with only the
/// types seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeCounts {
    /// Strings.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub string: u64,
    /// Numbers.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub number: u64,
    /// `true` and `false`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub boolean: u64,
    /// Objects.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub object: u64,
    /// Arrays.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub array: u64,
    /// `null`.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub null: u64,
}

impl TypeCounts {
    /// Count one more value.
    pub fn add(&mut self, value: &Json) {
        let slot = match value {
            Json::String(_) => &mut self.string,
            Json::Number(_) => &mut self.number,
            Json::Bool(_) => &mut self.boolean,
            Json::Object(_) => &mut self.object,
            Json::Array(_) => &mut self.array,
            Json::Null => &mut self.null,
        };
        *slot += 1;
    }

    /// Each type's name (as JSON's: `string`, `number`, `boolean`, `object`,
    /// `array`, `null`), its short name for the mix (`str`, `num`, `bool`,
    /// `obj`, `arr`, `null`) and its count, in that order.
    pub fn counts(&self) -> [(&'static str, &'static str, u64); 6] {
        [
            ("string", "str", self.string),
            ("number", "num", self.number),
            ("boolean", "bool", self.boolean),
            ("object", "obj", self.object),
            ("array", "arr", self.array),
            ("null", "null", self.null),
        ]
    }

    /// Every value counted.
    pub fn total(&self) -> u64 {
        self.counts().iter().map(|(_, _, n)| n).sum()
    }

    /// How many types were seen, `null` included.
    pub fn kinds(&self) -> usize {
        self.counts().iter().filter(|(_, _, n)| *n > 0).count()
    }

    /// Whether more than one type besides `null` was seen.
    pub fn mixed(&self) -> bool {
        self.counts()
            .iter()
            .filter(|(name, _, n)| *n > 0 && *name != "null")
            .count()
            > 1
    }

    /// The mix in short: the one type's short name (`str`), or each type
    /// seen with its share, most common first (`str 90% · null 10%`); empty
    /// when nothing was counted.
    pub fn mix(&self) -> String {
        let total = self.total();
        let mut seen: Vec<(&str, u64)> = self
            .counts()
            .iter()
            .filter(|(_, _, n)| *n > 0)
            .map(|(_, short, n)| (*short, *n))
            .collect();
        if seen.len() == 1 {
            return seen[0].0.to_string();
        }
        // Stable: ties keep the fixed order.
        seen.sort_by(|a, b| b.1.cmp(&a.1));
        seen.iter()
            .map(|(short, n)| format!("{short} {}", percent(*n as f64 / total as f64)))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// A nested profile's limits, and what met them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathSummary {
    /// The deepest path described, in steps ([`ProfileOptions::depth`]).
    pub depth: usize,
    /// The most paths described ([`ProfileOptions::max_paths`]).
    pub max_paths: usize,
    /// Whether any path kept fewer of its values than it had.
    pub sampled: bool,
    /// Whether paths were seen past `max_paths`.
    pub capped: bool,
    /// The first paths seen past it (at most a hundred), sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dropped: Vec<String>,
    /// Whether more were seen than `dropped` names.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub more_dropped: bool,
}

/// A key as a path writes it: as it is, or as a JSON string in brackets
/// (`["k8s.io/name"]`) when it is empty, has whitespace at either end, or
/// holds `.`, `[`, `]`, `"`, `\` or a control character. See the
/// [module docs](self).
///
/// ```
/// use rich_data::profile::path_key;
///
/// assert_eq!(path_key("user"), "user");
/// assert_eq!(path_key("first name"), "first name");
/// assert_eq!(path_key("k8s.io/name"), r#"["k8s.io/name"]"#);
/// assert_eq!(path_key(""), r#"[""]"#);
/// ```
pub fn path_key(key: &str) -> Cow<'_, str> {
    let bare = !key.is_empty()
        && key.trim() == key
        && !key.contains(|c: char| matches!(c, '.' | '[' | ']' | '"' | '\\') || c.is_control());
    if bare {
        Cow::Borrowed(key)
    } else {
        let quoted = serde_json::to_string(key).unwrap_or_else(|_| format!("{key:?}"));
        Cow::Owned(format!("[{quoted}]"))
    }
}

/// One step down a path.
#[derive(Clone, Copy)]
enum Step<'a> {
    Key(&'a str),
    Item,
}

/// `parent`'s path with `step` after it.
fn join(parent: Option<&str>, step: Step<'_>) -> String {
    let parent = parent.unwrap_or("");
    match step {
        Step::Item => format!("{parent}[]"),
        Step::Key(key) => match path_key(key) {
            Cow::Borrowed(bare) if parent.is_empty() => bare.to_string(),
            Cow::Borrowed(bare) => format!("{parent}.{bare}"),
            Cow::Owned(quoted) => format!("{parent}{quoted}"),
        },
    }
}

/// What a [`JsonProfiler`] knows of one path.
#[derive(Clone, Debug)]
struct Node {
    name: String,
    parent: Option<usize>,
    /// The paths one key further down.
    keys: HashMap<String, usize>,
    /// The path of this one's array items.
    item: Option<usize>,
    /// Records that hold the path.
    present: u64,
    /// The last record counted in `present`.
    last_present: u64,
    /// The last record with a value here that is not `null`.
    last_value: u64,
    /// Values seen, `null` included.
    values: u64,
    types: TypeCounts,
    /// Values offered to the reservoir.
    seen: u64,
    reservoir: Vec<Value>,
}

impl Node {
    fn new(name: String, parent: Option<usize>) -> Self {
        Node {
            name,
            parent,
            keys: HashMap::new(),
            item: None,
            present: 0,
            last_present: 0,
            last_value: 0,
            values: 0,
            types: TypeCounts::default(),
            seen: 0,
            reservoir: Vec::new(),
        }
    }
}

/// Profiles JSON records path by path in bounded memory. See the
/// [module docs](self).
///
/// ```
/// use rich_data::profile::{JsonProfiler, ProfileOptions};
/// use serde_json::json;
///
/// let options = ProfileOptions { depth: 1, ..Default::default() };
/// let mut profiler = JsonProfiler::new(options);
/// profiler.push(&json!({"a": {"b": 1}}));
/// profiler.push(&json!({"a": "x"}));
/// let profile = profiler.finish().unwrap();
/// let a = profile.column("a").unwrap();
/// // One step deep: the object is a value of `a`, as its JSON text.
/// assert_eq!(a.types.unwrap().mix(), "str 50% · obj 50%");
/// assert_eq!(a.distinct, 2);
/// ```
#[derive(Clone, Debug)]
pub struct JsonProfiler {
    options: ProfileOptions,
    rows: u64,
    nodes: Vec<Node>,
    /// The top-level paths, by key.
    top: HashMap<String, usize>,
    rng: u64,
    bucket_rows: u64,
    /// Per bucket: its records, and per path the records missing it.
    buckets: Vec<(u64, Vec<u64>)>,
    dropped: BTreeSet<String>,
    more_dropped: bool,
    malformed: u64,
    malformed_lines: Vec<usize>,
    first_malformed: Option<(usize, String)>,
}

impl JsonProfiler {
    /// A profiler with `options` (`depth`, `max_paths`, `sample`, `top`,
    /// `bins`, `buckets` and `columns`, which names paths).
    pub fn new(options: ProfileOptions) -> Self {
        JsonProfiler {
            options,
            rows: 0,
            nodes: Vec::new(),
            top: HashMap::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
            bucket_rows: 1,
            buckets: Vec::new(),
            dropped: BTreeSet::new(),
            more_dropped: false,
            malformed: 0,
            malformed_lines: Vec::new(),
            first_malformed: None,
        }
    }

    /// Add a record.
    pub fn push(&mut self, record: &Json) {
        self.rows += 1;
        let depth = self.options.depth.max(1);
        // Depth first, in document order, with a stack rather than
        // recursion: a record built in code may nest deeper than any parser
        // allows.
        let mut stack: Vec<(Option<usize>, Step<'_>, &Json, usize)> = match record {
            Json::Object(map) => map
                .iter()
                .rev()
                .map(|(key, value)| (None, Step::Key(key.as_str()), value, 1))
                .collect(),
            other => vec![(None, Step::Key("value"), other, 1)],
        };
        while let Some((parent, step, value, level)) = stack.pop() {
            let Some(id) = self.node(parent, step) else {
                continue;
            };
            let expand = level < depth;
            self.observe(id, value, expand);
            if !expand {
                continue;
            }
            match value {
                Json::Object(map) => stack.extend(
                    map.iter()
                        .rev()
                        .map(|(key, value)| (Some(id), Step::Key(key.as_str()), value, level + 1)),
                ),
                Json::Array(items) => stack.extend(
                    items
                        .iter()
                        .rev()
                        .map(|item| (Some(id), Step::Item, item, level + 1)),
                ),
                _ => {}
            }
        }
        self.end_record();
    }

    /// Count a line that could not be read as JSON (it is not a record):
    /// its 1-based number and why.
    pub fn skip_malformed(&mut self, line: usize, message: impl Into<String>) {
        self.malformed += 1;
        if self.malformed_lines.len() < MAX_MALFORMED_LINES {
            self.malformed_lines.push(line);
        }
        if self.first_malformed.is_none() {
            self.first_malformed = Some((line, message.into()));
        }
    }

    /// The path `step` below `parent`, added when new and there is room;
    /// `None` past the cap, after noting its name.
    fn node(&mut self, parent: Option<usize>, step: Step<'_>) -> Option<usize> {
        let found = match (parent, step) {
            (None, Step::Key(key)) => self.top.get(key).copied(),
            (Some(parent), Step::Key(key)) => self.nodes[parent].keys.get(key).copied(),
            (Some(parent), Step::Item) => self.nodes[parent].item,
            (None, Step::Item) => return None,
        };
        if found.is_some() {
            return found;
        }
        let name = join(parent.map(|p| self.nodes[p].name.as_str()), step);
        if self.nodes.len() >= self.options.max_paths.max(1) {
            if !self.dropped.contains(&name) {
                if self.dropped.len() < MAX_DROPPED {
                    self.dropped.insert(name);
                } else {
                    self.more_dropped = true;
                }
            }
            return None;
        }
        let id = self.nodes.len();
        // Every record read so far lacked it.
        for (rows, missing) in &mut self.buckets {
            missing.push(*rows);
        }
        self.nodes.push(Node::new(name, parent));
        match (parent, step) {
            (None, Step::Key(key)) => {
                self.top.insert(key.to_string(), id);
            }
            (Some(parent), Step::Key(key)) => {
                self.nodes[parent].keys.insert(key.to_string(), id);
            }
            (Some(parent), Step::Item) => self.nodes[parent].item = Some(id),
            (None, Step::Item) => {}
        }
        Some(id)
    }

    /// Count `value` at path `id`; an object or array that is `expand`ed has
    /// no value of its own, its paths below holding them.
    fn observe(&mut self, id: usize, value: &Json, expand: bool) {
        let record = self.rows;
        let node = &mut self.nodes[id];
        node.values += 1;
        node.types.add(value);
        if node.last_present != record {
            node.last_present = record;
            node.present += 1;
        }
        if value.is_null() {
            return;
        }
        node.last_value = record;
        if expand && matches!(value, Json::Object(_) | Json::Array(_)) {
            return;
        }
        // Algorithm R over the path's values, choosing the slot before
        // building the cell (an object's JSON text costs its size).
        node.seen += 1;
        let sample = self.options.sample.max(1);
        let (seen, kept) = (node.seen, node.reservoir.len());
        if kept < sample {
            self.nodes[id].reservoir.push(cell(value));
            return;
        }
        let slot = self.next_random() % seen;
        if let Some(kept) = self.nodes[id].reservoir.get_mut(slot as usize) {
            *kept = cell(value);
        }
    }

    /// The missing-value map for the record just walked: a path is missing
    /// when the record lacks it or holds only `null` there.
    fn end_record(&mut self) {
        let record = self.rows;
        if self
            .buckets
            .last()
            .is_none_or(|(rows, _)| *rows >= self.bucket_rows)
        {
            if self.buckets.len() >= self.options.buckets.max(1) {
                self.merge_buckets();
            }
            if self
                .buckets
                .last()
                .is_none_or(|(rows, _)| *rows >= self.bucket_rows)
            {
                self.buckets.push((0, vec![0; self.nodes.len()]));
            }
        }
        let bucket = self.buckets.last_mut().expect("a bucket was just added");
        bucket.0 += 1;
        for (missing, node) in bucket.1.iter_mut().zip(&self.nodes) {
            if node.last_value != record {
                *missing += 1;
            }
        }
    }

    /// As [`Profiler`](super::Profiler)'s: neighbouring buckets merged,
    /// doubling the records per bucket.
    fn merge_buckets(&mut self) {
        let width = self.nodes.len();
        self.buckets = self
            .buckets
            .chunks(2)
            .map(|pair| {
                let mut rows = 0;
                let mut missing = vec![0; width];
                for (r, m) in pair {
                    rows += r;
                    for (total, n) in missing.iter_mut().zip(m) {
                        *total += n;
                    }
                }
                (rows, missing)
            })
            .collect();
        self.bucket_rows *= 2;
    }

    /// SplitMix64, as the row profiler's.
    fn next_random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The paths in tree order: each after its parent, siblings in the
    /// order first seen.
    fn tree_order(&self) -> Vec<usize> {
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); self.nodes.len()];
        let mut stack: Vec<usize> = Vec::new();
        for (id, node) in self.nodes.iter().enumerate() {
            match node.parent {
                Some(parent) => children[parent].push(id),
                None => stack.push(id),
            }
        }
        stack.reverse();
        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(id) = stack.pop() {
            order.push(id);
            stack.extend(children[id].iter().rev());
        }
        order
    }

    /// The profile of every record pushed. Fails when
    /// [`ProfileOptions::columns`] names a path that was not profiled.
    pub fn finish(mut self) -> Result<Profile, DataError> {
        let order = match &self.options.columns {
            None => self.tree_order(),
            Some(wanted) => wanted
                .iter()
                .map(|name| {
                    self.nodes
                        .iter()
                        .position(|node| &node.name == name)
                        .ok_or_else(|| self.unknown_path(name))
                })
                .collect::<Result<_, _>>()?,
        };
        let rows = self.rows;
        let sample = self.options.sample.max(1);
        let sampled = self.nodes.iter().any(|n| n.seen > n.reservoir.len() as u64);
        let stats_options = StatsOptions {
            quantiles: vec![0.25, 0.75],
            top: self.options.top,
        };
        let mut columns = Vec::with_capacity(order.len());
        for &id in &order {
            let node = &mut self.nodes[id];
            let mut values = Rows::new([node.name.clone()]);
            for value in std::mem::take(&mut node.reservoir) {
                values.push([value]);
            }
            // JSON has its own null: no text is a null token here.
            let inference = Inferrer::new()
                .null_tokens(std::iter::empty::<String>())
                .infer(&values);
            inference.apply(&mut values);
            let stats = Stats::with_options(&values, &stats_options);
            let s = &stats.columns()[0];
            let types = node.types;
            // A path that only ever held objects or arrays (walked into).
            let data_type = if s.count == 0 && types.object + types.array > 0 {
                if types.object >= types.array {
                    "object"
                } else {
                    "array"
                }
            } else {
                inference.columns()[0].data_type().name()
            };
            let mut column = describe(&values, 0, s, data_type, self.options.bins);
            column.nulls = types.null;
            column.null_rate = if node.values == 0 {
                0.0
            } else {
                types.null as f64 / node.values as f64
            };
            column.present = Some(node.present);
            column.absent = Some(rows - node.present);
            column.values = Some(node.values);
            column.types = Some(types);
            column.mixed = Some(types.mixed());
            columns.push(column);
        }
        let mut first = 1;
        let buckets = self
            .buckets
            .iter()
            .map(|(records, missing)| {
                let bucket = MissingBucket {
                    first,
                    rows: *records,
                    nulls: order.iter().map(|&id| missing[id]).collect(),
                };
                first += records;
                bucket
            })
            .collect();
        let mut notes = Vec::new();
        let mixed: Vec<&str> = columns
            .iter()
            .filter(|c| c.mixed == Some(true))
            .map(|c| c.name.as_str())
            .collect();
        if !mixed.is_empty() {
            let count = mixed.len() as u64;
            notes.push(format!(
                "! marks {count} {} holding more than one JSON type besides null: {}",
                plural(count, "path", "paths"),
                listed(&mixed, count as usize)
            ));
        }
        if !self.dropped.is_empty() {
            let names: Vec<&str> = self.dropped.iter().map(String::as_str).collect();
            let count = names.len() as u64;
            let shown = if self.more_dropped {
                format!("{}+", grouped(count))
            } else {
                grouped(count)
            };
            notes.push(format!(
                "max_paths ({}) was reached: {shown} more {} not profiled: {}",
                grouped(self.options.max_paths.max(1) as u64),
                plural(count, "path is", "paths are"),
                listed(&names, if self.more_dropped { usize::MAX } else { names.len() })
            ));
        }
        if self.malformed > 0 {
            let mut lines: Vec<String> =
                self.malformed_lines.iter().map(usize::to_string).collect();
            if self.malformed as usize > lines.len() {
                lines.push("…".into());
            }
            let mut note = format!(
                "{} {} not JSON and {} skipped: {} {}",
                grouped(self.malformed),
                plural(self.malformed, "line is", "lines are"),
                plural(self.malformed, "was", "were"),
                plural(self.malformed, "line", "lines"),
                lines.join(", "),
            );
            if let Some((line, message)) = &self.first_malformed {
                // serde_json's position is within the line: the line
                // number says where.
                let why = message.strip_prefix("not JSON: ").unwrap_or(message);
                let why = why.split(" at line ").next().unwrap_or(why);
                if self.malformed == 1 {
                    note.push_str(&format!(" ({why})"));
                } else {
                    note.push_str(&format!(" (line {line}: {why})"));
                }
            }
            notes.push(note);
        }
        Ok(Profile {
            name: None,
            rows,
            sample,
            columns,
            missing: MissingMap {
                bucket_rows: self.bucket_rows,
                buckets,
            },
            notes,
            paths: Some(PathSummary {
                depth: self.options.depth.max(1),
                max_paths: self.options.max_paths.max(1),
                sampled,
                capped: !self.dropped.is_empty(),
                dropped: self.dropped.into_iter().collect(),
                more_dropped: self.more_dropped,
            }),
            malformed: self.malformed,
            malformed_lines: self.malformed_lines,
            rules: Rules::default(),
        })
    }

    /// The error for a path `columns` names that was not profiled.
    fn unknown_path(&self, name: &str) -> DataError {
        let names: Vec<&str> = self.nodes.iter().map(|n| n.name.as_str()).collect();
        let mut message = format!(
            "no path {:?}; the paths are {}",
            rich_ext::sanitize::sanitize_single_line(name),
            listed(&names, names.len())
        );
        if !self.dropped.is_empty() {
            message.push_str(&format!(
                " (max_paths {} was reached; a path seen later is not profiled)",
                self.options.max_paths.max(1)
            ));
        }
        DataError::new(message)
    }
}

/// The first [`NOTE_NAMES`] of `names`, then `…` when `total` is more.
fn listed(names: &[&str], total: usize) -> String {
    let mut shown: Vec<String> = names
        .iter()
        .take(NOTE_NAMES)
        .map(|n| rich_ext::sanitize::sanitize_single_line(n))
        .collect();
    if total > shown.len() {
        shown.push("…".into());
    }
    shown.join(", ")
}

impl Profile {
    /// A nested profile of JSON Lines from `reader`, streamed: a line that
    /// is not JSON is counted and skipped ([`malformed`](Self::malformed)),
    /// while a read error or a line longer than
    /// [`jsonl::MAX_LINE`](crate::jsonl::MAX_LINE) fails. See
    /// [`JsonProfiler`].
    pub fn from_json_lines(reader: impl BufRead, options: ProfileOptions) -> Result<Self, DataError> {
        let mut profiler = JsonProfiler::new(options);
        let mut lines = Lines::new(reader);
        while let Some(record) = lines.next() {
            match record {
                Ok(record) => profiler.push(&record),
                Err(error) if !lines.stopped() => {
                    profiler.skip_malformed(error.line().unwrap_or(0), error.message())
                }
                Err(error) => return Err(error),
            }
        }
        profiler.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::Console;
    use serde_json::json;

    fn nested(text: &str, options: ProfileOptions) -> Profile {
        Profile::from_json_lines(text.as_bytes(), options).unwrap()
    }

    fn names(profile: &Profile) -> Vec<&str> {
        profile.columns().iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn paths_follow_objects_and_arrays_in_tree_order() {
        let p = nested(
            "{\"id\": 1, \"items\": [{\"sku\": \"a\", \"qty\": 2}]}\n\
             {\"id\": 2, \"user\": {\"name\": \"x\"}, \"items\": [{\"sku\": \"b\", \"note\": \"n\"}]}\n",
            ProfileOptions::default(),
        );
        // `items[].note`, first seen in the second record, sits with its
        // siblings rather than at the end.
        assert_eq!(
            names(&p),
            [
                "id",
                "items",
                "items[]",
                "items[].sku",
                "items[].qty",
                "items[].note",
                "user",
                "user.name"
            ]
        );
        assert_eq!(p.heading(), "2 records, 8 paths");
        let sku = p.column("items[].sku").unwrap();
        assert_eq!((sku.present, sku.absent, sku.values), (Some(2), Some(0), Some(2)));
        let items = p.column("items").unwrap();
        assert_eq!(items.data_type, "array");
        assert_eq!(p.column("items[]").unwrap().data_type, "object");
    }

    #[test]
    fn array_items_count_every_value_and_presence_counts_records() {
        let p = nested(
            "{\"tags\": [\"a\", \"b\", \"a\"]}\n{\"tags\": []}\n{}\n",
            ProfileOptions::default(),
        );
        let tags = p.column("tags[]").unwrap();
        assert_eq!((tags.values, tags.present, tags.absent), (Some(3), Some(1), Some(2)));
        assert_eq!((tags.count, tags.distinct), (3, 2));
        let tags = p.column("tags").unwrap();
        assert_eq!((tags.present, tags.absent), (Some(2), Some(1)));
    }

    #[test]
    fn absent_keys_are_counted_apart_from_null() {
        let p = nested(
            "{\"a\": 1, \"b\": null}\n{\"a\": null}\n{\"a\": 2, \"b\": \"x\"}\n{}\n",
            ProfileOptions::default(),
        );
        let a = p.column("a").unwrap();
        assert_eq!((a.present, a.absent, a.nulls), (Some(3), Some(1), 1));
        assert_eq!(a.null_rate, 1.0 / 3.0);
        let b = p.column("b").unwrap();
        assert_eq!((b.present, b.absent, b.nulls), (Some(2), Some(2), 1));
        assert_eq!(b.types.unwrap().mix(), "str 50% · null 50%");
        assert_eq!(b.mixed, Some(false));
        // The map: a record misses `b` when it lacks it or holds null.
        let map = p.missing();
        let b_missing: Vec<u64> = map.buckets.iter().map(|bucket| bucket.nulls[1]).collect();
        assert_eq!(b_missing, [1, 1, 0, 1]);
        let json = p.to_json();
        assert_eq!(json["columns"][1]["absent"], 2);
        assert_eq!(json["columns"][1]["types"], json!({"string": 1, "null": 1}));
    }

    #[test]
    fn type_mixes_count_each_json_type() {
        let p = nested(
            "{\"v\": 1}\n{\"v\": \"1\"}\n{\"v\": true}\n{\"v\": [1]}\n{\"v\": {\"k\": 1}}\n\
             {\"v\": null}\n{\"v\": 2.5}\n",
            ProfileOptions::default(),
        );
        let v = p.column("v").unwrap();
        let types = v.types.unwrap();
        assert_eq!(
            (types.string, types.number, types.boolean, types.object, types.array, types.null),
            (1, 2, 1, 1, 1, 1)
        );
        assert_eq!(types.kinds(), 6);
        assert!(types.mixed());
        assert_eq!(types.mix(), "num 29% · str 14% · bool 14% · obj 14% · arr 14% · null 14%");
        // A single type is its name alone.
        let only = TypeCounts {
            number: 3,
            ..Default::default()
        };
        assert_eq!((only.mix(), only.mixed()), ("num".to_string(), false));
        assert_eq!(TypeCounts::default().mix(), "");
        // The table flags the mixed path.
        let table = Console::builder()
            .width(160)
            .color_system(None)
            .build()
            .render_export(&p.to_table());
        assert!(table.contains("! num 29%"), "{table}");
        assert!(p.notes()[0].starts_with("! marks 1 path holding more than one JSON type"));
    }

    #[test]
    fn depth_bounds_the_walk_and_unwalked_values_are_json_text() {
        let text = "{\"a\": {\"b\": {\"c\": {\"d\": {\"e\": 1}}}}}\n";
        let p = nested(text, ProfileOptions::default());
        assert_eq!(names(&p), ["a", "a.b", "a.b.c", "a.b.c.d"]);
        let deepest = p.column("a.b.c.d").unwrap();
        assert_eq!(deepest.max.as_deref(), Some("{\"e\":1}"));
        let shallow = nested(
            text,
            ProfileOptions {
                depth: 1,
                ..Default::default()
            },
        );
        assert_eq!(names(&shallow), ["a"]);
        assert_eq!(shallow.paths().unwrap().depth, 1);
        // A record nested far deeper than any parser allows does not
        // overflow the stack.
        let mut deep = json!(1);
        for _ in 0..100_000 {
            deep = Json::Array(vec![deep]);
        }
        let mut profiler = JsonProfiler::new(ProfileOptions {
            depth: usize::MAX,
            max_paths: 10,
            ..Default::default()
        });
        profiler.push(&deep);
        let p = profiler.finish().unwrap();
        assert_eq!(p.columns().len(), 10);
        assert!(p.paths().unwrap().capped);
        // Dropping the value iteratively too.
        let mut value = deep;
        while let Json::Array(mut items) = value {
            value = items.pop().unwrap_or(Json::Null);
        }
    }

    #[test]
    fn the_path_cap_names_what_it_dropped_and_stays_bounded() {
        let text: String = (0..1_000)
            .map(|i| format!("{{\"id\": {i}, \"k{i:04}\": 1}}\n"))
            .collect();
        let p = nested(
            &text,
            ProfileOptions {
                max_paths: 5,
                ..Default::default()
            },
        );
        assert_eq!(names(&p), ["id", "k0000", "k0001", "k0002", "k0003"]);
        let paths = p.paths().unwrap();
        assert!(paths.capped && paths.more_dropped);
        assert_eq!(paths.dropped.len(), 100);
        assert_eq!(paths.dropped[0], "k0004");
        let note = p.notes().iter().find(|n| n.starts_with("max_paths")).unwrap();
        assert!(
            note.starts_with("max_paths (5) was reached: 100+ more paths are not profiled: k0004, "),
            "{note}"
        );
        assert!(note.ends_with(", …"), "{note}");
        let table = Console::builder()
            .width(160)
            .color_system(None)
            .build()
            .render_export(&p.to_table());
        assert!(table.contains("100+ more paths not profiled (max_paths 5)"), "{table}");
        let json = p.to_json();
        assert_eq!(json["paths"]["capped"], true);
        assert_eq!(json["paths"]["max_paths"], 5);
        // A later path in `columns` that the cap kept out says so.
        let error = Profile::from_json_lines(
            text.as_bytes(),
            ProfileOptions {
                max_paths: 5,
                columns: Some(vec!["k0500".into()]),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.message().starts_with("no path \"k0500\"; the paths are id, k0000"));
        assert!(error.message().contains("max_paths 5 was reached"), "{error}");
    }

    #[test]
    fn keys_that_look_like_paths_are_quoted() {
        let p = nested(
            "{\"a.b\": {\"c\": 1}, \"x[0]\": [true], \"\": 1, \" pad\": 2, \"q\\\"\": {\"k8s.io/n\": 3}}\n",
            ProfileOptions::default(),
        );
        assert_eq!(
            names(&p),
            [
                "[\"a.b\"]",
                "[\"a.b\"].c",
                "[\"x[0]\"]",
                "[\"x[0]\"][]",
                "[\"\"]",
                "[\" pad\"]",
                "[\"q\\\"\"]",
                "[\"q\\\"\"][\"k8s.io/n\"]"
            ]
        );
        // `columns` names paths as written.
        let p = nested(
            "{\"a.b\": {\"c\": 1}}\n",
            ProfileOptions {
                columns: Some(vec!["[\"a.b\"].c".into()]),
                ..Default::default()
            },
        );
        assert_eq!(names(&p), ["[\"a.b\"].c"]);
    }

    #[test]
    fn malformed_lines_are_counted_and_never_fatal() {
        let mut text = String::from("{\"a\": 1}\n{\"a\": \nnot json\n\u{0}\u{1}\n[1, 2\n");
        for i in 0..20 {
            text.push_str(&format!("{{oops {i}\n"));
        }
        text.push_str("{\"a\": 2}\n\"just a string\"\n7\n");
        // Invalid UTF-8 too.
        let mut bytes = text.into_bytes();
        bytes.extend_from_slice(b"{\"a\": \"\xff\"}\n");
        let p = Profile::from_json_lines(&bytes[..], ProfileOptions::default()).unwrap();
        assert_eq!(p.rows(), 4);
        assert_eq!(p.malformed(), 25);
        assert_eq!(p.malformed_lines(), [2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        let note = p.notes().last().unwrap();
        assert!(
            note.starts_with("25 lines are not JSON and were skipped: lines 2, 3,"),
            "{note}"
        );
        assert!(
            note.ends_with(", 11, … (line 2: EOF while parsing a value)"),
            "{note}"
        );
        // Scalar records are the `value` path, as in a flat profile.
        assert_eq!(names(&p), ["a", "value"]);
        assert_eq!(p.column("value").unwrap().types.unwrap().mix(), "str 50% · num 50%");
        let json = p.to_json();
        assert_eq!(json["malformed"], 25);
        assert_eq!(json["malformed_lines"][0], 2);
        // Nothing but malformed lines: an empty profile, still no panic.
        let p = nested("nope\n{\n", ProfileOptions::default());
        assert_eq!((p.rows(), p.columns().len(), p.malformed()), (0, 0, 2));
        let console = Console::builder().width(40).color_system(None).build();
        console.render_export(&p);
        // A read error still fails: what follows it is unknown.
        struct Broken(bool);
        impl std::io::Read for Broken {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if std::mem::replace(&mut self.0, true) {
                    return Err(std::io::Error::other("disk on fire"));
                }
                let line = b"{\"a\": 1}\n";
                buf[..line.len()].copy_from_slice(line);
                Ok(line.len())
            }
        }
        let error = Profile::from_json_lines(
            std::io::BufReader::new(Broken(false)),
            ProfileOptions::default(),
        )
        .unwrap_err();
        assert_eq!((error.line(), error.message()), (Some(2), "disk on fire"));
    }

    #[test]
    fn a_long_stream_is_sampled_per_path_with_exact_counts() {
        let options = ProfileOptions {
            sample: 100,
            buckets: 8,
            ..Default::default()
        };
        let mut profiler = JsonProfiler::new(options);
        for i in 0..20_000i64 {
            let record = if i % 4 == 0 {
                json!({"n": i, "xs": [i, i + 1]})
            } else {
                json!({"n": i, "s": null})
            };
            profiler.push(&record);
        }
        let p = profiler.finish().unwrap();
        assert!(p.sampled());
        assert_eq!(p.sample_size(), 100);
        assert_eq!(
            p.heading(),
            "20,000 records, 4 paths, sampled to 100 values a path"
        );
        let n = p.column("n").unwrap();
        assert_eq!((n.values, n.count), (Some(20_000), 100));
        let xs = p.column("xs[]").unwrap();
        assert_eq!((xs.values, xs.present, xs.absent), (Some(10_000), Some(5_000), Some(15_000)));
        let s = p.column("s").unwrap();
        assert_eq!((s.nulls, s.null_rate, s.absent), (15_000, 1.0, Some(5_000)));
        // The map stays at its bucket limit and counts every record.
        assert!(p.missing().buckets.len() <= 8);
        let records: u64 = p.missing().buckets.iter().map(|b| b.rows).sum();
        assert_eq!(records, 20_000);
        let s_missing: u64 = p.missing().buckets.iter().map(|b| b.nulls[3]).sum();
        assert_eq!(s_missing, 20_000);
        // Reproducible.
        let again = {
            let mut profiler = JsonProfiler::new(ProfileOptions {
                sample: 100,
                buckets: 8,
                ..Default::default()
            });
            for i in 0..20_000i64 {
                profiler.push(&if i % 4 == 0 {
                    json!({"n": i, "xs": [i, i + 1]})
                } else {
                    json!({"n": i, "s": null})
                });
            }
            profiler.finish().unwrap()
        };
        assert_eq!(again, p);
    }

    #[test]
    fn the_nested_model_round_trips_and_renders_narrow() {
        let p = nested(
            "{\"a\": {\"b\": [1, \"x\"]}, \"c\": null}\n{\"a\": 3}\nnope\n",
            ProfileOptions::default(),
        )
        .with_name("n.jsonl");
        let json = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        for width in [40, 50, 80] {
            let console = Console::builder().width(width).color_system(None).build();
            let segments = console.render(&p, None);
            for line in rich::Segment::split_lines(&segments) {
                let cells: usize = line.iter().map(rich::Segment::cell_length).sum();
                assert!(cells <= width, "{width}: {line:?}");
            }
        }
    }
}

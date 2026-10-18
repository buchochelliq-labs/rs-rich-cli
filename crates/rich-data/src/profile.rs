//! Data profiles (#343, #344, #345, #346): what every column of a CSV, JSON
//! Lines or any other [`RowSource`] holds, in one bounded pass.
//!
//! A [`Profile`] gives each column its inferred type ([`infer`](crate::infer)),
//! null count and rate, distinct values, statistics ([`stats`](crate::stats))
//! and a distribution: a histogram (from `rich_ext`'s
//! [`chart::Histogram`](rich_ext::chart::Histogram)) for a numeric column, the
//! most common values for any other. A missing-value map shows where the
//! nulls are: the rows in order, bucketed, against the columns, drawn as a
//! [`chart::Heatmap`](rich_ext::chart::Heatmap). The model serialises to JSON
//! (`serde`), and renders as a heading, a table of the columns, each
//! distribution and the map.
//!
//! **Bounded.** A [`Profiler`] reads rows one at a time and keeps at most
//! [`ProfileOptions::sample`] of them (default [`DEFAULT_SAMPLE`]): a uniform
//! reservoir sample (Algorithm R) drawn with a fixed seed, so the same input
//! always gives the same profile. Types, distinct values, statistics and
//! distributions describe the sample; the row count, every null count and
//! the missing-value map count every row (the map holds at most
//! [`ProfileOptions::buckets`] buckets, merging neighbours as rows arrive).
//! [`Profile::sampled`] says whether the sample is all of it, and the
//! heading prints the sample size. Memory is the sample plus the map, however
//! long the input.
//!
//! Nulls are [`Value::Null`] and the null tokens (by default
//! [`DEFAULT_NULL_TOKENS`]: an empty cell, `null`, `NULL`, `NA`, `N/A`).
//!
//! ```
//! use rich::Console;
//! use rich_data::profile::Profile;
//! use rich_data::{csv::CsvReader, RowSource};
//!
//! let text = "service,p99\nweb,120\napi,35\ndb,\nweb,80\n";
//! let profile = Profile::from_source(
//!     CsvReader::new().header(true).source(text.as_bytes()).unwrap(),
//!     Default::default(),
//! )
//! .unwrap();
//! assert_eq!(profile.rows(), 4);
//! assert!(!profile.sampled());
//! let p99 = profile.column("p99").unwrap();
//! assert_eq!((p99.data_type.as_str(), p99.nulls), ("integer", 1));
//! assert_eq!(p99.mean, Some(78.33333333333333));
//! let service = profile.column("service").unwrap();
//! assert_eq!(service.distinct, 3);
//!
//! let console = Console::builder().width(60).color_system(None).build();
//! let out = console.render_export(&profile);
//! assert!(out.starts_with("4 rows, 2 columns\n"));
//! assert!(out.contains("p99 · integer · 1 null (25%)"));
//! let json = profile.to_json();
//! assert_eq!(json["columns"][1]["distribution"]["kind"], "histogram");
//! ```
//!
//! [`DEFAULT_NULL_TOKENS`]: crate::infer::DEFAULT_NULL_TOKENS

use rich::{
    Cell, ColumnOptions, Console, ConsoleOptions, Justify, Renderable, Segment, Style, Table, Text,
};
use rich_ext::chart::{BarChart, Heatmap, Histogram};
use rich_ext::sanitize::sanitize_single_line;
use rich_ext::table::StyleRules;
use serde::{Deserialize, Serialize};

use crate::infer::{parse_float, Inferrer, DEFAULT_NULL_TOKENS};
use crate::stats::{number, ColumnStats, Stats, StatsOptions};
use crate::{DataError, Row, RowSource, Rows, Value};

mod nested;

pub use nested::{
    path_key, JsonProfiler, PathSummary, TypeCounts, DEFAULT_DEPTH, DEFAULT_MAX_PATHS,
    MAX_MALFORMED_LINES,
};

/// The rows a profile keeps by default.
pub const DEFAULT_SAMPLE: usize = 10_000;
/// The most common values a categorical column shows by default.
pub const DEFAULT_TOP: usize = 5;
/// Histogram bins by default.
pub const DEFAULT_BINS: usize = 10;
/// Rows of the missing-value map by default.
pub const DEFAULT_BUCKETS: usize = 20;
/// The longest value label a distribution writes, in characters.
const LABEL_CHARS: usize = 24;
/// Cells for the longest bar of a distribution.
const BAR_WIDTH: usize = 24;

/// The theme keys a profile draws with, and their defaults. A console theme
/// that defines a key wins.
pub const STYLES: &[(&str, &str)] = &[
    ("profile.title", "bold"),
    ("profile.column", "bold"),
    ("profile.type", "cyan"),
    ("profile.note", "dim"),
    ("profile.null", "yellow"),
    ("profile.mixed", "magenta"),
];

/// The console theme's style for `key`, else its default from [`STYLES`].
pub(crate) fn style(console: &Console, key: &str, styles: &[(&str, &str)]) -> Style {
    if let Some(style) = console.theme().get(key) {
        return style.clone();
    }
    styles
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, spec)| Style::parse(spec).ok())
        .unwrap_or_default()
}

/// What a [`Profiler`] keeps and shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ProfileOptions {
    /// The most rows kept for types, statistics and distributions (default
    /// [`DEFAULT_SAMPLE`], at least 1).
    pub sample: usize,
    /// The most common values a categorical column keeps (default
    /// [`DEFAULT_TOP`]).
    pub top: usize,
    /// Histogram bins for a numeric column (default [`DEFAULT_BINS`]).
    pub bins: usize,
    /// The most rows of the missing-value map (default [`DEFAULT_BUCKETS`],
    /// at least 1).
    pub buckets: usize,
    /// Only these columns, by name, in this order (default every column).
    pub columns: Option<Vec<String>>,
    /// The cells that count as null, compared after trimming (default
    /// [`DEFAULT_NULL_TOKENS`]). A nested profile ([`JsonProfiler`]) has
    /// JSON's own `null` and ignores them.
    pub nulls: Vec<String>,
    /// For a nested profile ([`JsonProfiler`]), the deepest path profiled,
    /// in steps (a key or `[]`; default [`DEFAULT_DEPTH`], at least 1).
    pub depth: usize,
    /// For a nested profile, the most paths profiled (default
    /// [`DEFAULT_MAX_PATHS`], at least 1).
    pub max_paths: usize,
}

impl Default for ProfileOptions {
    fn default() -> Self {
        ProfileOptions {
            sample: DEFAULT_SAMPLE,
            top: DEFAULT_TOP,
            bins: DEFAULT_BINS,
            buckets: DEFAULT_BUCKETS,
            columns: None,
            nulls: DEFAULT_NULL_TOKENS.map(String::from).to_vec(),
            depth: DEFAULT_DEPTH,
            max_paths: DEFAULT_MAX_PATHS,
        }
    }
}

/// One column of a [`Profile`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColumnProfile {
    /// The column name.
    pub name: String,
    /// The inferred type's name (`integer`, `float`, `boolean`, `date`,
    /// `timestamp`, `text`, or `null` when every sampled cell is null).
    #[serde(rename = "type")]
    pub data_type: String,
    /// Null cells, over every row.
    pub nulls: u64,
    /// `nulls` over the row count, `0.0` for no rows.
    pub null_rate: f64,
    /// Non-null cells in the sample.
    pub count: usize,
    /// Distinct non-null values in the sample.
    pub distinct: usize,
    /// The smallest value in the sample (numerically for a number column).
    pub min: Option<String>,
    /// The largest value in the sample.
    pub max: Option<String>,
    /// The mean, for a number column.
    pub mean: Option<f64>,
    /// The median, for a number column.
    pub median: Option<f64>,
    /// The first quartile, for a number column.
    pub p25: Option<f64>,
    /// The third quartile, for a number column.
    pub p75: Option<f64>,
    /// How the sampled values are spread.
    pub distribution: Distribution,
    /// For a nested profile: the records that hold the path at least once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub present: Option<u64>,
    /// For a nested profile: the records that do not hold the path (an
    /// explicit `null` is present, and counted in `nulls`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absent: Option<u64>,
    /// For a nested profile: the values seen at the path, `null`s included
    /// (more than `present` inside arrays).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<u64>,
    /// For a nested profile: how many of those values were each JSON type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub types: Option<TypeCounts>,
    /// For a nested profile: whether the values were more than one JSON
    /// type besides `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixed: Option<bool>,
}

impl ColumnProfile {
    /// `absent` over the row count, for a nested profile.
    pub fn absent_rate(&self, rows: u64) -> Option<f64> {
        self.absent
            .map(|absent| if rows == 0 { 0.0 } else { absent as f64 / rows as f64 })
    }
}

/// How a column's values are spread.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Distribution {
    /// A number column: values counted into bins.
    Histogram {
        /// The bins, low to high.
        bins: Vec<Bin>,
    },
    /// Any other column: its most common values.
    Top {
        /// The most common values, most common first (ties in text order).
        values: Vec<TopValue>,
        /// Non-null cells holding any other value.
        other: usize,
    },
    /// No non-null values.
    Empty,
}

/// One histogram bin: `[low, high)`, the last `[low, high]`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bin {
    /// `[low, high)` as the chart writes it.
    pub label: String,
    /// The low edge.
    pub low: f64,
    /// The high edge.
    pub high: f64,
    /// Values in the bin.
    pub count: usize,
}

/// One of a column's most common values.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TopValue {
    /// The value as text.
    pub value: String,
    /// How many sampled cells hold it.
    pub count: usize,
}

/// Where the nulls are: the rows in order, in buckets of
/// [`bucket_rows`](Self::bucket_rows), each with its null count per column.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MissingMap {
    /// Rows per bucket (the last may hold fewer).
    pub bucket_rows: u64,
    /// The buckets, first rows first.
    pub buckets: Vec<MissingBucket>,
}

/// One bucket of a [`MissingMap`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MissingBucket {
    /// The first row, counted from 1.
    pub first: u64,
    /// Rows in the bucket.
    pub rows: u64,
    /// Null cells per column, in the profile's column order.
    pub nulls: Vec<u64>,
}

/// A profile of rows. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// What was profiled (a file name), for the heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    rows: u64,
    sample: usize,
    columns: Vec<ColumnProfile>,
    missing: MissingMap,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<String>,
    /// Set for a nested profile ([`JsonProfiler`]): its limits and the paths
    /// past them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    paths: Option<PathSummary>,
    /// Lines that were not JSON, skipped (a nested profile only).
    #[serde(default, skip_serializing_if = "is_zero")]
    malformed: u64,
    /// The first of them, by line number.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    malformed_lines: Vec<usize>,
    /// Conditional styles for the table; not part of the model.
    #[serde(skip)]
    rules: Rules,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// [`StyleRules`] held by a [`Profile`]: presentation, so two profiles with
/// different rules are still equal.
#[derive(Clone, Debug, Default)]
struct Rules(StyleRules);

impl PartialEq for Rules {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Profile {
    /// A profile of rows in memory, with the default options.
    pub fn of(rows: &Rows) -> Self {
        let mut profiler = Profiler::new(rows.columns(), ProfileOptions::default())
            .expect("every column of the rows exists");
        for row in rows.rows() {
            profiler.push(row.clone());
        }
        profiler.finish()
    }

    /// Profile every row of `source`, stopping at the first that cannot be
    /// read.
    pub fn from_source(
        mut source: impl RowSource,
        options: ProfileOptions,
    ) -> Result<Self, DataError> {
        let mut profiler = Profiler::new(source.columns(), options)?;
        while let Some(row) = source.next_row() {
            profiler.push(row?);
        }
        Ok(profiler.finish())
    }

    /// Name what was profiled, for the heading.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Add a note shown under the heading (rows cut to fit, keys seen late).
    pub fn note(&mut self, note: impl Into<String>) {
        self.notes.push(note.into());
    }

    /// Every row read.
    pub fn rows(&self) -> u64 {
        self.rows
    }

    /// The rows the types, statistics and distributions describe.
    pub fn sample_size(&self) -> usize {
        self.sample
    }

    /// Whether the sample is fewer rows than were read; for a nested
    /// profile, whether any path kept fewer of its values than it had (then
    /// [`sample_size`](Self::sample_size) is the most values a path keeps).
    pub fn sampled(&self) -> bool {
        match &self.paths {
            Some(paths) => paths.sampled,
            None => (self.sample as u64) < self.rows,
        }
    }

    /// One entry per column, in order.
    pub fn columns(&self) -> &[ColumnProfile] {
        &self.columns
    }

    /// The column called `name`.
    pub fn column(&self, name: &str) -> Option<&ColumnProfile> {
        self.columns.iter().find(|c| c.name == name)
    }

    /// Where the nulls are.
    pub fn missing(&self) -> &MissingMap {
        &self.missing
    }

    /// The notes, in order.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Whether this is a nested profile of JSON paths ([`JsonProfiler`]):
    /// its columns are paths, each with its presence and type mix.
    pub fn is_nested(&self) -> bool {
        self.paths.is_some()
    }

    /// A nested profile's limits, and the paths past its cap; `None` for a
    /// profile of rows.
    pub fn paths(&self) -> Option<&PathSummary> {
        self.paths.as_ref()
    }

    /// Lines skipped because they were not JSON (a nested profile only).
    pub fn malformed(&self) -> u64 {
        self.malformed
    }

    /// The first of them by line number (at most
    /// [`MAX_MALFORMED_LINES`]).
    pub fn malformed_lines(&self) -> &[usize] {
        &self.malformed_lines
    }

    /// Style the table of columns by value (`rich_ext`'s
    /// [`table::rules`](rich_ext::table::rules)). A rule names a heading
    /// ([`table_headers`](Self::table_headers)) and compares the model's
    /// value, not the cell's text: `nulls`, `absent` and `distinct` are
    /// counts, `mean` a number, the rest text.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_data::profile::Profile;
    /// use rich_data::Rows;
    /// use rich_ext::table::{StyleRule, StyleRules};
    ///
    /// let mut rows = Rows::new(["a", "b"]);
    /// rows.push(["1".into(), "".into()]);
    /// let rules = StyleRules::new().rule(StyleRule::parse("nulls>0:row:bold red").unwrap());
    /// let profile = Profile::of(&rows).style_rules(rules);
    /// let console = Console::builder().width(60).force_terminal(true).build();
    /// let out = console.render_export(&profile.to_table());
    /// assert!(out.contains("\x1b[1;31mb"), "{out:?}");
    /// ```
    pub fn style_rules(mut self, rules: StyleRules) -> Self {
        self.rules = Rules(rules);
        self
    }

    /// The style rules set with [`style_rules`](Self::style_rules).
    pub fn rules(&self) -> &StyleRules {
        &self.rules.0
    }

    /// The headings of the table of columns, which style rules name:
    /// `column`, `type`, `nulls`, `distinct`, `min`, `max` and `mean`; for
    /// a nested profile `path`, `types` (the mix, `null` among them),
    /// `absent`, `distinct`, `min`, `max` and `mean`.
    pub fn table_headers(&self) -> &'static [&'static str] {
        if self.is_nested() {
            &["path", "types", "absent", "distinct", "min", "max", "mean"]
        } else {
            &["column", "type", "nulls", "distinct", "min", "max", "mean"]
        }
    }

    /// One row of the table as style rules compare it, in
    /// [`table_headers`](Self::table_headers) order.
    fn rule_values(&self, c: &ColumnProfile) -> Vec<Value> {
        let text = |v: &Option<String>| v.clone().map_or(Value::Null, Value::Str);
        let count = |n: u64| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
        let mut values = vec![Value::Str(c.name.clone())];
        if self.is_nested() {
            values.push(Value::Str(c.types.map(|t| t.mix()).unwrap_or_default()));
            values.push(count(c.absent.unwrap_or(0)));
        } else {
            values.push(Value::Str(c.data_type.clone()));
            values.push(count(c.nulls));
        }
        values.extend([
            count(c.distinct as u64),
            text(&c.min),
            text(&c.max),
            c.mean.map_or(Value::Null, Value::Float),
        ]);
        values
    }

    /// The profile as JSON: `rows`, `sample`, `sampled`, `columns`,
    /// `missing`, and `name` and `notes` when set. A nested profile adds
    /// `paths` (its `depth`, `max_paths`, whether it was `capped` and the
    /// paths `dropped`), `malformed` and `malformed_lines` when lines were
    /// skipped, and per column `present`, `absent`, `values`, `types` and
    /// `mixed`.
    pub fn to_json(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).unwrap_or_default();
        if let Some(object) = value.as_object_mut() {
            object.insert("sampled".into(), self.sampled().into());
        }
        value
    }

    /// The heading: the name, the rows (and the sample, when smaller) and
    /// the columns.
    pub fn heading(&self) -> String {
        let count = self.columns.len() as u64;
        let body = if self.is_nested() {
            // Records and paths; each path is sampled on its own.
            let mut body = format!(
                "{} {}, {} {}",
                grouped(self.rows),
                plural(self.rows, "record", "records"),
                grouped(count),
                plural(count, "path", "paths")
            );
            if self.sampled() {
                body.push_str(&format!(
                    ", sampled to {} values a path",
                    grouped(self.sample as u64)
                ));
            }
            body
        } else {
            let rows = if self.sampled() {
                format!(
                    "sampled {} of {} rows",
                    grouped(self.sample as u64),
                    grouped(self.rows)
                )
            } else {
                format!(
                    "{} {}",
                    grouped(self.rows),
                    plural(self.rows, "row", "rows")
                )
            };
            format!("{rows}, {count} {}", plural(count, "column", "columns"))
        };
        match &self.name {
            Some(name) => format!("{}: {body}", sanitize_single_line(name)),
            None => body,
        }
    }

    /// The table of columns: type, nulls, distinct, min, max and mean (the
    /// median and quartiles are in the model and under each numeric
    /// column's heading).
    ///
    /// A nested profile's table has each path's type mix (`str 90% · null
    /// 10%`, with `!` before the mix of a path holding more than one type
    /// besides `null`) and the records that lack it (`absent`), in place of
    /// the inferred type and the nulls (the mix has their share).
    pub fn to_table(&self) -> Table {
        self.table(None, Hold::NamesAndMixes)
    }

    /// [`to_table`](Self::to_table), its columns held as `hold` says, and
    /// mixed type mixes styled from `console`'s theme when there is one.
    fn table(&self, console: Option<&Console>, hold: Hold) -> Table {
        let names_first = hold != Hold::Nothing;
        let mut table = Table::new();
        let headers = self.table_headers();
        // Names give way last.
        let widest = self
            .columns
            .iter()
            .map(|c| c.name.chars().count())
            .max()
            .unwrap_or(0);
        // A nested profile's paths are held whole (up to 24) and not
        // narrowed before the other columns: paths differ at their ends.
        let nested_names = names_first && self.is_nested();
        table.add_column_with(
            Text::new(headers[0]),
            ColumnOptions {
                min_width: names_first.then(|| widest.clamp(6, if nested_names { 24 } else { 16 })),
                no_wrap: nested_names,
                ..ColumnOptions::default()
            },
        );
        // A nested profile's type mixes give way after the names.
        let mixes = hold == Hold::NamesAndMixes && self.is_nested();
        table.add_column_with(
            Text::new(headers[1]),
            ColumnOptions {
                no_wrap: mixes,
                ..ColumnOptions::default()
            },
        );
        for heading in &headers[2..] {
            table.add_column_justify(*heading, Justify::Right);
        }
        let mixed = console.map(|console| style(console, "profile.mixed", STYLES));
        let rules = &self.rules.0;
        let values: Vec<Vec<Value>> = if rules.is_empty() {
            Vec::new()
        } else {
            self.columns.iter().map(|c| self.rule_values(c)).collect()
        };
        let resolved = (!rules.is_empty()).then(|| rules.resolve(headers, &values));
        for (index, c) in self.columns.iter().enumerate() {
            let optional = |v: Option<f64>| v.map(number).unwrap_or_default();
            let text = |v: &Option<String>| v.as_deref().map(label).unwrap_or_default();
            let mut cells = vec![Text::new(sanitize_single_line(&c.name))];
            if self.is_nested() {
                let mix = c.types.unwrap_or_default().mix();
                cells.push(match (c.mixed == Some(true), &mixed) {
                    (true, Some(style)) => Text::styled(format!("! {mix}"), style.clone()),
                    (true, None) => Text::new(format!("! {mix}")),
                    (false, _) => Text::new(mix),
                });
                cells.push(Text::new(nulls(
                    c.absent.unwrap_or(0),
                    c.absent_rate(self.rows).unwrap_or(0.0),
                )));
            } else {
                cells.push(Text::new(c.data_type.clone()));
                // A nested path's nulls are in its type mix.
                cells.push(Text::new(nulls(c.nulls, c.null_rate)));
            }
            cells.extend(
                [
                    c.distinct.to_string(),
                    text(&c.min),
                    text(&c.max),
                    optional(c.mean),
                ]
                .into_iter()
                .map(Text::new),
            );
            let (Some(resolved), Some(row)) = (&resolved, values.get(index)) else {
                table.add_row_text(cells);
                continue;
            };
            let cells = cells
                .into_iter()
                .enumerate()
                .map(|(column, mut text)| {
                    if let Some(style) = resolved.cell_style(row, column) {
                        let len = text.plain().len();
                        text.stylize(style, 0, len);
                    }
                    Cell::Text(text)
                })
                .collect();
            table.add_row_with(cells, resolved.row_style(row).map(Into::into), false);
        }
        // A note row when a nested profile met its cap.
        if let Some(paths) = self.paths.as_ref().filter(|p| p.capped) {
            let more = paths.dropped.len() as u64;
            let count = if paths.more_dropped {
                format!("{}+", grouped(more))
            } else {
                grouped(more)
            };
            let note = format!(
                "{count} more {} not profiled (max_paths {})",
                plural(more, "path", "paths"),
                grouped(paths.max_paths as u64)
            );
            let dim = console.map(|console| style(console, "profile.note", STYLES));
            let cell = |text: String| match &dim {
                Some(style) => Text::styled(text, style.clone()),
                None => Text::new(text),
            };
            table.add_row_text(vec![cell("…".into()), cell(note)]);
        }
        table
    }

    /// The missing-value map as a heatmap: a row per bucket, a column per
    /// column, each cell the percentage of the bucket's cells that are null.
    /// `None` when nothing is null.
    ///
    /// A nested profile counts a record as missing a path when it lacks the
    /// path or holds only `null` there, and maps only the paths something is
    /// missing from.
    pub fn missing_heatmap(&self) -> Option<Heatmap> {
        if !self.is_nested() && self.columns.iter().all(|c| c.nulls == 0) {
            return None;
        }
        let missing = |i: usize| {
            self.missing
                .buckets
                .iter()
                .any(|b| b.nulls.get(i).is_some_and(|&n| n > 0))
        };
        // Every column, or for a nested profile those with anything missing.
        let shown: Vec<usize> = (0..self.columns.len())
            .filter(|&i| !self.is_nested() || missing(i))
            .collect();
        if shown.is_empty() {
            return None;
        }
        let widest = shown
            .iter()
            .map(|&i| self.columns[i].name.chars().count())
            .max()
            .unwrap_or(1);
        let mut map = Heatmap::new()
            .columns(
                shown
                    .iter()
                    .map(|&i| sanitize_single_line(&self.columns[i].name)),
            )
            .range(0.0, 100.0)
            .cell_width((widest + 1).clamp(2, 12));
        for bucket in &self.missing.buckets {
            let last = bucket.first + bucket.rows.saturating_sub(1);
            let label = if bucket.rows <= 1 {
                bucket.first.to_string()
            } else {
                format!("{}-{last}", bucket.first)
            };
            let rows = bucket.rows.max(1) as f64;
            let share = |&i: &usize| bucket.nulls.get(i).copied().unwrap_or(0) as f64 * 100.0 / rows;
            map = map.row(label, shown.iter().map(share));
        }
        Some(map)
    }
}

/// Which columns of the table of columns keep their width when it is tight.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Hold {
    /// The names (up to 16) and a nested profile's type mixes (up to 20).
    NamesAndMixes,
    /// The names alone.
    Names,
    /// Neither.
    Nothing,
}

/// A column's distribution as bars: the histogram's bins, or the most
/// common values.
fn distribution_chart(distribution: &Distribution) -> Option<BarChart> {
    let bars: Vec<(String, f64)> = match distribution {
        Distribution::Histogram { bins } => bins
            .iter()
            .map(|b| (b.label.clone(), b.count as f64))
            .collect(),
        Distribution::Top { values, .. } => values
            .iter()
            .map(|v| (label(&v.value), v.count as f64))
            .collect(),
        Distribution::Empty => return None,
    };
    Some(BarChart::from_pairs(bars).bar_width(BAR_WIDTH))
}

impl Renderable for Profile {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        if width == 0 {
            return Vec::new();
        }
        let options = options.reset_height();
        let note = style(console, "profile.note", STYLES);
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        let text = |text: Text, indent: usize, lines: &mut Vec<Vec<Segment>>| {
            block(console, &options, &text, indent, lines);
        };

        text(
            Text::styled(self.heading(), style(console, "profile.title", STYLES)),
            0,
            &mut lines,
        );
        if self.sampled() {
            let says = if self.is_nested() {
                "types, distinct values, statistics and distributions are of a uniform sample \
                 of each path's values; presence, type mixes, nulls and the missing-value map \
                 count every record"
            } else {
                "types, distinct values, statistics and distributions are of a uniform sample; \
                 nulls and the missing-value map count every row"
            };
            text(Text::styled(says, note.clone()), 0, &mut lines);
        }
        for line in &self.notes {
            text(
                Text::styled(sanitize_single_line(line), note.clone()),
                0,
                &mut lines,
            );
        }
        if self.columns.is_empty() {
            return join(lines);
        }
        lines.push(Vec::new());
        // A name column (and a nested profile's type mixes) held wide can
        // push the table past a narrow width; then the mixes give way, then
        // the names too.
        let fits = |table: &Table| {
            let drawn = console.render(table, Some(&options));
            Segment::split_lines(&drawn).iter().all(|line| {
                line.iter().map(Segment::cell_length).sum::<usize>() <= options.max_width
            })
        };
        let mut table = self.table(Some(console), Hold::NamesAndMixes);
        if self.is_nested() && !fits(&table) {
            table = self.table(Some(console), Hold::Names);
        }
        if !fits(&table) {
            table = self.table(Some(console), Hold::Nothing);
        }
        block(console, &options, &table, 0, &mut lines);

        for c in &self.columns {
            lines.push(Vec::new());
            let mut title = Text::styled(
                sanitize_single_line(&c.name),
                style(console, "profile.column", STYLES),
            );
            title.append(" · ", None);
            title.append(
                &c.data_type,
                Some(style(console, "profile.type", STYLES).into()),
            );
            // A nested path: its type mix when it holds more than one type
            // besides null, and the records that lack it.
            let types = c.types.unwrap_or_default();
            if c.mixed == Some(true) {
                title.append(" · ", None);
                title.append(
                    &format!("mixed types: {}", types.mix()),
                    Some(style(console, "profile.mixed", STYLES).into()),
                );
            }
            if let Some(absent) = c.absent.filter(|&n| n > 0) {
                title.append(" · ", None);
                title.append(
                    &format!(
                        "{} absent ({})",
                        grouped(absent),
                        percent(c.absent_rate(self.rows).unwrap_or(0.0))
                    ),
                    Some(style(console, "profile.null", STYLES).into()),
                );
            }
            if c.nulls > 0 {
                title.append(" · ", None);
                title.append(
                    &format!(
                        "{} {} ({})",
                        grouped(c.nulls),
                        plural(c.nulls, "null", "nulls"),
                        percent(c.null_rate)
                    ),
                    Some(style(console, "profile.null", STYLES).into()),
                );
            }
            // A nested object or array whose own paths are profiled has no
            // values of its own to show.
            let container = c.count == 0 && types.object + types.array > 0;
            if matches!(c.distribution, Distribution::Histogram { .. }) {
                let quartiles = [c.p25, c.median, c.p75].map(|q| q.map(number));
                if let [Some(p25), Some(median), Some(p75)] = quartiles {
                    title.append(&format!(" · median {median} (p25 {p25}, p75 {p75})"), None);
                }
            } else if !container {
                title.append(&format!(" · {} distinct", grouped(c.distinct as u64)), None);
            }
            text(title, 0, &mut lines);
            match distribution_chart(&c.distribution) {
                Some(chart) => block(console, &options, &chart, 2, &mut lines),
                None if container => {}
                None => text(Text::styled("no values", note.clone()), 2, &mut lines),
            }
            if let Distribution::Top { values, other } = &c.distribution {
                if *other > 0 {
                    let more = c.distinct.saturating_sub(values.len()) as u64;
                    text(
                        Text::styled(
                            format!(
                                "+ {} other {} ({} {})",
                                grouped(more),
                                plural(more, "value", "values"),
                                grouped(*other as u64),
                                plural(*other as u64, "cell", "cells"),
                            ),
                            note.clone(),
                        ),
                        2,
                        &mut lines,
                    );
                }
            }
        }

        lines.push(Vec::new());
        match self.missing_heatmap() {
            None => text(
                Text::styled("no missing values", note.clone()),
                0,
                &mut lines,
            ),
            Some(map) => {
                let rows = self.missing.bucket_rows;
                let (what, row, rows_name) = if self.is_nested() {
                    ("% of records absent or null", "a record", "records")
                } else {
                    ("% null", "a row", "rows")
                };
                let per = if rows <= 1 {
                    format!("{row} each")
                } else {
                    format!("{} {rows_name} each", grouped(rows))
                };
                text(
                    Text::styled(
                        format!("missing values ({what}; {rows_name} top to bottom, {per})"),
                        style(console, "profile.column", STYLES),
                    ),
                    0,
                    &mut lines,
                );
                block(console, &options, &map, 0, &mut lines);
            }
        }
        join(lines)
    }

    /// As wide as its widest line at the width given.
    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let widest = Segment::split_lines(&self.rich_render(console, options))
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        rich::measure::Measurement::new(widest, widest)
    }
}

/// Render `renderable` `indent` cells in, adding its lines to `lines`.
pub(crate) fn block(
    console: &Console,
    options: &ConsoleOptions,
    renderable: &dyn Renderable,
    indent: usize,
    lines: &mut Vec<Vec<Segment>>,
) {
    let width = options.max_width.saturating_sub(indent).max(1);
    let rendered = console.render(renderable, Some(&options.update_width(width)));
    for line in Segment::split_lines(&rendered) {
        let mut row = Vec::with_capacity(line.len() + 1);
        if indent > 0 {
            row.push(Segment::new(" ".repeat(indent), None));
        }
        row.extend(line);
        lines.push(row);
    }
}

/// Lines separated by newlines.
pub(crate) fn join(lines: Vec<Vec<Segment>>) -> Vec<Segment> {
    let mut out = Vec::new();
    for (i, line) in lines.into_iter().enumerate() {
        if i > 0 {
            out.push(Segment::line());
        }
        out.extend(line);
    }
    out
}

/// Profiles rows one at a time in bounded memory. See the
/// [module docs](self).
///
/// ```
/// use rich_data::profile::{ProfileOptions, Profiler};
/// use rich_data::Value;
///
/// let options = ProfileOptions { sample: 100, ..Default::default() };
/// let mut profiler = Profiler::new(&["n".to_string()], options).unwrap();
/// for n in 0..10_000 {
///     profiler.push(vec![Value::Int(n)]);
/// }
/// let profile = profiler.finish();
/// assert_eq!((profile.rows(), profile.sample_size()), (10_000, 100));
/// assert!(profile.sampled());
/// assert!(profile.heading().starts_with("sampled 100 of 10,000 rows"));
/// ```
#[derive(Clone, Debug)]
pub struct Profiler {
    options: ProfileOptions,
    names: Vec<String>,
    /// The source column of each profiled column.
    indices: Vec<usize>,
    rows: u64,
    reservoir: Vec<Row>,
    rng: u64,
    nulls: Vec<u64>,
    bucket_rows: u64,
    buckets: Vec<(u64, Vec<u64>)>,
}

impl Profiler {
    /// A profiler for rows with `columns`. Fails when
    /// [`ProfileOptions::columns`] names a column there is not.
    pub fn new(columns: &[String], options: ProfileOptions) -> Result<Self, DataError> {
        let indices: Vec<usize> = match &options.columns {
            None => (0..columns.len()).collect(),
            Some(wanted) => wanted
                .iter()
                .map(|name| {
                    columns.iter().position(|c| c == name).ok_or_else(|| {
                        DataError::new(format!(
                            "no column {:?}; the columns are {}",
                            sanitize_single_line(name),
                            columns
                                .iter()
                                .map(|c| sanitize_single_line(c))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ))
                    })
                })
                .collect::<Result<_, _>>()?,
        };
        Ok(Profiler {
            names: indices.iter().map(|&i| columns[i].clone()).collect(),
            nulls: vec![0; indices.len()],
            indices,
            rows: 0,
            reservoir: Vec::new(),
            rng: 0x9E37_79B9_7F4A_7C15,
            bucket_rows: 1,
            buckets: Vec::new(),
            options,
        })
    }

    /// Add a row (missing cells are null; extra cells are ignored).
    pub fn push(&mut self, row: Row) {
        let mut cells: Row = self
            .indices
            .iter()
            .map(|&i| row.get(i).cloned().unwrap_or(Value::Null))
            .collect();
        // The missing-value map: a new bucket when the last is full, and
        // neighbours merged when there would be too many.
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
                self.buckets.push((0, vec![0; self.indices.len()]));
            }
        }
        let bucket = self.buckets.last_mut().expect("a bucket was just added");
        bucket.0 += 1;
        // Null tokens become `Null` in the sample too, so a text column does
        // not count `NA` as a value.
        for (column, cell) in cells.iter_mut().enumerate() {
            if is_null(&self.options.nulls, cell) {
                self.nulls[column] += 1;
                bucket.1[column] += 1;
                *cell = Value::Null;
            }
        }
        // Algorithm R: keep the first `sample` rows, then replace a kept row
        // with probability sample / rows seen.
        let sample = self.options.sample.max(1);
        self.rows += 1;
        if self.reservoir.len() < sample {
            self.reservoir.push(cells);
        } else {
            let slot = self.next_random() % self.rows;
            if let Some(kept) = self.reservoir.get_mut(slot as usize) {
                *kept = cells;
            }
        }
    }

    /// The profile of every row pushed.
    pub fn finish(self) -> Profile {
        let mut sample = Rows::new(self.names.clone());
        let size = self.reservoir.len();
        for row in self.reservoir {
            sample.push(row);
        }
        let inference = Inferrer::new()
            .null_tokens(self.options.nulls.iter().cloned())
            .infer(&sample);
        inference.apply(&mut sample);
        let stats = Stats::with_options(
            &sample,
            &StatsOptions {
                quantiles: vec![0.25, 0.75],
                top: self.options.top,
            },
        );
        let rows = self.rows;
        let columns = stats
            .columns()
            .iter()
            .zip(inference.columns())
            .enumerate()
            .map(|(index, (s, inferred))| {
                let mut column = describe(
                    &sample,
                    index,
                    s,
                    inferred.data_type().name(),
                    self.options.bins,
                );
                column.nulls = self.nulls[index];
                column.null_rate = if rows == 0 {
                    0.0
                } else {
                    self.nulls[index] as f64 / rows as f64
                };
                column
            })
            .collect();
        let mut first = 1;
        let buckets = self
            .buckets
            .into_iter()
            .map(|(rows, nulls)| {
                let bucket = MissingBucket { first, rows, nulls };
                first += rows;
                bucket
            })
            .collect();
        Profile {
            name: None,
            rows,
            sample: size,
            columns,
            missing: MissingMap {
                bucket_rows: self.bucket_rows,
                buckets,
            },
            notes: Vec::new(),
            paths: None,
            malformed: 0,
            malformed_lines: Vec::new(),
            rules: Rules::default(),
        }
    }

    /// Merge neighbouring buckets, doubling the rows per bucket. Every
    /// bucket is full, so each new one covers twice the rows, in order; an
    /// odd last one carries on filling.
    fn merge_buckets(&mut self) {
        let merged = self
            .buckets
            .chunks(2)
            .map(|pair| {
                let mut rows = 0;
                let mut nulls = vec![0; self.indices.len()];
                for (r, n) in pair {
                    rows += r;
                    for (total, n) in nulls.iter_mut().zip(n) {
                        *total += n;
                    }
                }
                (rows, nulls)
            })
            .collect();
        self.buckets = merged;
        self.bucket_rows *= 2;
    }

    /// SplitMix64: a fixed sequence, so a profile is reproducible.
    fn next_random(&mut self) -> u64 {
        self.rng = self.rng.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Column `index` of `sample` described from its statistics `s`: its type,
/// statistics and distribution, with no nulls counted yet (the caller
/// counts them over every row, not the sample).
fn describe(
    sample: &Rows,
    index: usize,
    s: &ColumnStats,
    data_type: &str,
    bins: usize,
) -> ColumnProfile {
    let distribution = if s.count == 0 {
        Distribution::Empty
    } else if s.numeric {
        let values = sample.column(index).filter_map(|v| {
            v.as_f64()
                .or_else(|| parse_float(v.plain().trim()))
                .filter(|n| n.is_finite())
        });
        histogram(Histogram::new(values).bins(bins.max(1)))
    } else {
        let shown: usize = s.top.iter().map(|(_, n)| n).sum();
        Distribution::Top {
            values: s
                .top
                .iter()
                .map(|(value, count)| TopValue {
                    value: value.clone(),
                    count: *count,
                })
                .collect(),
            other: s.count - shown,
        }
    };
    let quantile = |q: f64| s.quantiles.iter().find(|(at, _)| *at == q).map(|v| v.1);
    ColumnProfile {
        name: s.name.clone(),
        data_type: data_type.to_string(),
        nulls: 0,
        null_rate: 0.0,
        count: s.count,
        distinct: s.distinct,
        min: s.min.clone(),
        max: s.max.clone(),
        mean: s.mean,
        median: s.median,
        p25: quantile(0.25),
        p75: quantile(0.75),
        distribution,
        present: None,
        absent: None,
        values: None,
        types: None,
        mixed: None,
    }
}

/// Whether `value` is null: [`Value::Null`] or one of the `tokens`.
fn is_null(tokens: &[String], value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Int(_) | Value::Float(_) => false,
        Value::Str(text) => tokens.iter().any(|t| t == text.trim()),
        Value::Text(text) => {
            let plain = text.plain();
            tokens.iter().any(|t| t == plain.trim())
        }
    }
}

/// A histogram's bins, labelled as its chart labels them.
fn histogram(histogram: Histogram) -> Distribution {
    let edges = histogram.edges();
    let counts = histogram.counts();
    let chart = histogram.to_bar_chart();
    let bins = chart
        .bars()
        .iter()
        .zip(counts)
        .enumerate()
        .map(|(i, (bar, count))| Bin {
            label: bar.label.clone(),
            low: edges[i],
            high: edges[i + 1],
            count,
        })
        .collect();
    Distribution::Histogram { bins }
}

/// A value as a label: one line, at most [`LABEL_CHARS`] characters.
fn label(value: &str) -> String {
    let line = sanitize_single_line(value);
    if line.chars().count() <= LABEL_CHARS {
        return line;
    }
    let mut cut: String = line.chars().take(LABEL_CHARS - 1).collect();
    cut.push('…');
    cut
}

/// `3 (25%)`, or `0`.
fn nulls(count: u64, rate: f64) -> String {
    if count == 0 {
        "0".to_string()
    } else {
        format!("{} ({})", grouped(count), percent(rate))
    }
}

/// A rate as a percentage: whole numbers from 1%, one decimal below, and
/// never `0%` or `100%` unless exactly so.
fn percent(rate: f64) -> String {
    let p = rate * 100.0;
    if (99.5..100.0).contains(&p) {
        return ">99%".to_string();
    }
    if p > 0.0 && p < 0.1 {
        return "<0.1%".to_string();
    }
    if p < 1.0 {
        format!("{}%", number((p * 10.0).round() / 10.0))
    } else {
        format!("{}%", p.round())
    }
}

/// `1234567` as `1,234,567`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: u64, one: &'static str, many: &'static str) -> &'static str {
    if n == 1 {
        one
    } else {
        many
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(columns: &[&str], rows: &[&[&str]], options: ProfileOptions) -> Profile {
        let names: Vec<String> = columns.iter().map(|c| c.to_string()).collect();
        let mut profiler = Profiler::new(&names, options).unwrap();
        for row in rows {
            profiler.push(row.iter().map(|c| Value::from(*c)).collect());
        }
        profiler.finish()
    }

    #[test]
    fn numbers_group_and_rates_round() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1_000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(percent(0.25), "25%");
        assert_eq!(percent(0.004), "0.4%");
        assert_eq!(percent(0.000_01), "<0.1%");
        assert_eq!(percent(0.999), ">99%");
        assert_eq!(percent(1.0), "100%");
    }

    #[test]
    fn the_map_merges_buckets_in_order() {
        let options = ProfileOptions {
            buckets: 4,
            ..Default::default()
        };
        let rows: Vec<Vec<&str>> = (0..10)
            .map(|i| vec![if i % 3 == 0 { "" } else { "x" }])
            .collect();
        let rows: Vec<&[&str]> = rows.iter().map(Vec::as_slice).collect();
        let p = profile(&["a"], &rows, options);
        let map = p.missing();
        assert_eq!(map.bucket_rows, 4);
        let shape: Vec<(u64, u64, Vec<u64>)> = map
            .buckets
            .iter()
            .map(|b| (b.first, b.rows, b.nulls.clone()))
            .collect();
        // Nulls at rows 1, 4, 7 and 10.
        assert_eq!(shape, [(1, 4, vec![2]), (5, 4, vec![1]), (9, 2, vec![1])]);
        assert_eq!(p.columns()[0].nulls, 4);
    }

    #[test]
    fn a_large_input_is_sampled_with_exact_nulls() {
        let options = ProfileOptions {
            sample: 50,
            ..Default::default()
        };
        let names = vec!["n".to_string(), "s".to_string()];
        let mut profiler = Profiler::new(&names, options).unwrap();
        for i in 0..5_000i64 {
            let s = if i % 10 == 0 {
                Value::Null
            } else {
                Value::from("x")
            };
            profiler.push(vec![Value::Int(i), s]);
        }
        let p = profiler.finish();
        assert_eq!((p.rows(), p.sample_size(), p.sampled()), (5_000, 50, true));
        assert_eq!(p.columns()[1].nulls, 500);
        assert_eq!(p.columns()[1].null_rate, 0.1);
        // The sample spreads over the input, not just its start.
        let max: f64 = p.columns()[0].max.as_deref().unwrap().parse().unwrap();
        assert!(max > 2_500.0, "{max}");
        assert_eq!(p.to_json()["sampled"], true);
        assert_eq!(p.to_json()["sample"], 50);
    }

    #[test]
    fn ragged_and_empty_input_never_panic() {
        let names = vec!["a".to_string(), "b".to_string()];
        let mut profiler = Profiler::new(&names, ProfileOptions::default()).unwrap();
        profiler.push(vec![]);
        profiler.push(vec![Value::from("1"), Value::from("2"), Value::from("3")]);
        let p = profiler.finish();
        assert_eq!(p.columns()[0].nulls, 1);
        let console = Console::builder().width(30).color_system(None).build();
        console.render_export(&p);
        let empty = Profiler::new(&[], ProfileOptions::default())
            .unwrap()
            .finish();
        assert_eq!(console.render_export(&empty), "0 rows, 0 columns\n");
        let none = Profiler::new(&names, ProfileOptions::default())
            .unwrap()
            .finish();
        assert!(console.render_export(&none).contains("no values"));
    }

    #[test]
    fn unknown_columns_are_named() {
        let options = ProfileOptions {
            columns: Some(vec!["b".into(), "z".into()]),
            ..Default::default()
        };
        let error = Profiler::new(&["a".into(), "b".into()], options).unwrap_err();
        assert_eq!(error.to_string(), "no column \"z\"; the columns are a, b");
    }

    #[test]
    fn the_model_round_trips_through_json() {
        let p = profile(
            &["k", "v"],
            &[&["a", "1"], &["b", "2.5"], &["a", ""]],
            ProfileOptions::default(),
        )
        .with_name("x.csv");
        let json = serde_json::to_string(&p).unwrap();
        let back: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
        assert_eq!(p.columns()[1].data_type, "float");
        match &p.columns()[0].distribution {
            Distribution::Top { values, other } => {
                assert_eq!(values[0].value, "a");
                assert_eq!((values[0].count, *other), (2, 0));
            }
            other => panic!("{other:?}"),
        }
    }
}

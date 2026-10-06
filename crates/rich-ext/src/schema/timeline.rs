//! A schema's evolution over versions (#347), on a
//! [`Timeline`](crate::chart::Timeline).

use rich::{Console, ConsoleOptions, Renderable, Segment, Text, Tree};
use serde_json::Value;

use super::{json, theme_style, ChangeKind, Schema, SchemaDiff, STYLES};
use crate::chart::{Charset, Span, Timeline};

/// The most field rows drawn; past this the rest are counted on a last
/// line.
pub const MAX_ROWS: usize = 200;

/// One version: a JSON Schema or a model schema.
#[derive(Clone, Debug)]
enum Source {
    Json(Value),
    Model(Schema),
}

#[derive(Clone, Debug)]
struct Version {
    label: String,
    at: Option<f64>,
    source: Source,
}

impl Version {
    /// The model schema, for the field rows.
    fn model(&self) -> Schema {
        match &self.source {
            Source::Json(value) => json::to_model(value),
            Source::Model(schema) => schema.clone(),
        }
    }
}

/// A series of schema versions on a [`Timeline`]: a row per field (a
/// table's columns as `table.column`) spanning the versions it is in, and a
/// milestone per version with what changed in it. A span is drawn in the
/// added, changed or breaking style in the version its field was added or
/// changed, and the [details](SchemaTimeline::details) under the timeline
/// list every change, so neither the timeline nor the list reads colour
/// alone.
///
/// Consecutive versions are compared with [`SchemaDiff`]: two JSON Schemas
/// as JSON Schema, anything else through the model.
///
/// ```
/// use rich::Console;
/// use rich_ext::schema::{sql, SchemaTimeline};
///
/// let ddl = |columns: &str| sql::parse(&format!("CREATE TABLE t ({columns});")).unwrap().schema;
/// let timeline = SchemaTimeline::new()
///     .push("v1", ddl("id INT"))
///     .push("v2", ddl("id INT, name TEXT"))
///     .push("v3", ddl("id BIGINT, name TEXT"))
///     .details(false)
///     .charset(rich_ext::chart::Charset::Ascii);
/// let console = Console::builder().width(60).color_system(None).build();
/// assert_eq!(
///     console.render_to_string(&timeline),
///     concat!(
///         "t.id   ################================#################    \n",
///         "t.name                 ################=================    \n",
///         "       * v1            * v2: +1        * v3: ~1, 1 breaking \n",
///         "       +-------+-------+-------+-------+-------+-------+----\n",
///         "       0      0.5      1      1.5      2      2.5      3    ",
///     )
/// );
/// ```
#[derive(Clone, Debug)]
pub struct SchemaTimeline {
    versions: Vec<Version>,
    details: bool,
    charset: Charset,
}

impl Default for SchemaTimeline {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaTimeline {
    /// No versions yet; add them in order with [`push`](Self::push).
    pub fn new() -> Self {
        SchemaTimeline {
            versions: Vec::new(),
            details: true,
            charset: Charset::Auto,
        }
    }

    /// Add a model schema as the next version.
    pub fn push(mut self, label: impl Into<String>, schema: Schema) -> Self {
        self.versions.push(Version {
            label: label.into(),
            at: None,
            source: Source::Model(schema),
        });
        self
    }

    /// Add a JSON Schema as the next version.
    pub fn push_json(mut self, label: impl Into<String>, schema: Value) -> Self {
        self.versions.push(Version {
            label: label.into(),
            at: None,
            source: Source::Json(schema),
        });
        self
    }

    /// Place the last version added at `at` on the scale (a date as days,
    /// say). By default each version is at its index.
    pub fn at(mut self, at: f64) -> Self {
        if let Some(last) = self.versions.last_mut() {
            last.at = Some(at);
        }
        self
    }

    /// List every change under the timeline (default on).
    pub fn details(mut self, details: bool) -> Self {
        self.details = details;
        self
    }

    /// Glyphs to draw the timeline with.
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// The version labels, in order.
    pub fn labels(&self) -> Vec<&str> {
        self.versions.iter().map(|v| v.label.as_str()).collect()
    }

    /// What changed into each version after the first: its label and the
    /// diff from the one before.
    pub fn changes(&self) -> Vec<(&str, SchemaDiff)> {
        self.versions
            .windows(2)
            .map(|pair| {
                let diff = match (&pair[0].source, &pair[1].source) {
                    (Source::Json(a), Source::Json(b)) => SchemaDiff::new(a, b),
                    _ => SchemaDiff::models(&pair[0].model(), &pair[1].model()),
                };
                let diff = diff.names(pair[0].label.clone(), pair[1].label.clone());
                (pair[1].label.as_str(), diff)
            })
            .collect()
    }

    fn position(&self, index: usize) -> f64 {
        self.versions[index]
            .at
            .filter(|at| at.is_finite())
            .unwrap_or(index as f64)
    }

    /// The [`Timeline`] drawn, its styles resolved through `console`'s
    /// theme: at most [`MAX_ROWS`] rows (rendering counts the rest on a
    /// line under it).
    pub fn timeline(&self, console: &Console) -> Timeline {
        let mut timeline = Timeline::new()
            .durations(false)
            .compress(false)
            .charset(self.charset);
        if self.versions.is_empty() {
            return timeline;
        }
        let spec = |key: &str| -> String {
            if console.theme().get(key).is_some() {
                key.to_string()
            } else {
                STYLES
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map_or_else(String::new, |(_, spec)| spec.to_string())
            }
        };
        let models: Vec<Schema> = self.versions.iter().map(Version::model).collect();
        let changes = self.changes();
        let (rows, _) = rows(&models);
        let n = self.versions.len();
        let step = if n > 1 {
            ((self.position(n - 1) - self.position(0)) / (n - 1) as f64).max(f64::EPSILON)
        } else {
            1.0
        };
        let present: Vec<std::collections::HashSet<String>> = models
            .iter()
            .map(|model| row_names(model).into_iter().collect())
            .collect();
        for row in &rows {
            for (index, names) in present.iter().enumerate() {
                if !names.contains(row) {
                    continue;
                }
                let start = self.position(index);
                let end = if index + 1 < n {
                    self.position(index + 1)
                } else {
                    start + step
                };
                let mut span = Span::new(row.clone(), start, end);
                let key = index
                    .checked_sub(1)
                    .and_then(|i| row_change(row, &changes[i].1));
                if let Some(key) = key {
                    span = span.style(spec(key));
                } else {
                    span = span.style("dim");
                }
                timeline = timeline.push(span);
            }
        }
        for (index, version) in self.versions.iter().enumerate() {
            let label = match index.checked_sub(1) {
                None => version.label.clone(),
                Some(i) => format!("{}: {}", version.label, counts(&changes[i].1)),
            };
            timeline = timeline.milestone(label, self.position(index));
        }
        timeline
    }

    /// `… N more fields` under the timeline, when there are more rows than
    /// it draws.
    fn hidden_note(&self, console: &Console) -> Option<Text> {
        let models: Vec<Schema> = self.versions.iter().map(Version::model).collect();
        let (_, hidden) = rows(&models);
        (hidden > 0).then(|| {
            Text::styled(
                format!("… {hidden} more fields"),
                theme_style(console, "schema.description"),
            )
        })
    }

    /// The change list drawn under the timeline.
    fn listing(&self, console: &Console) -> Vec<Tree> {
        let mut trees = Vec::new();
        for (_, diff) in self.changes() {
            let mut tree = Tree::new(Text::styled(
                diff.summary(),
                theme_style(console, "schema.name"),
            ));
            for change in diff.changes() {
                let mut line = Text::new("");
                let style = theme_style(console, change.kind.style_key());
                line.append(change.kind.marker(), Some(style.clone().into()));
                line.append(" ", None);
                line.append(
                    &change.path,
                    Some(theme_style(console, "schema.name").into()),
                );
                line.append("  ", None);
                line.append(&change.detail, Some(style.into()));
                if change.breaking {
                    line.append("  ", None);
                    line.append(
                        "breaking",
                        Some(theme_style(console, "schema.breaking").into()),
                    );
                }
                tree.add(line);
            }
            trees.push(tree);
        }
        trees
    }
}

/// The rows drawn, in the order fields first appear, and how many more
/// there are past [`MAX_ROWS`].
fn rows(models: &[Schema]) -> (Vec<String>, usize) {
    let mut rows: Vec<String> = Vec::new();
    let mut listed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for model in models {
        for row in row_names(model) {
            if listed.insert(row.clone()) {
                rows.push(row);
            }
        }
    }
    let hidden = rows.len().saturating_sub(MAX_ROWS);
    rows.truncate(MAX_ROWS);
    (rows, hidden)
}

/// The rows a schema has: its fields, then each table's as `table.column`.
fn row_names(schema: &Schema) -> Vec<String> {
    let mut rows: Vec<String> = schema
        .fields()
        .iter()
        .filter(|f| f.kind() == super::FieldKind::Field)
        .map(|f| f.name().to_string())
        .collect();
    for table in schema.tables() {
        let name = table.name().unwrap_or_default();
        rows.extend(
            table
                .fields()
                .iter()
                .map(|f| format!("{name}.{}", f.name())),
        );
    }
    rows
}

/// The style a row's span takes in a version: the strongest of its
/// changes there, if it changed.
fn row_change(row: &str, diff: &SchemaDiff) -> Option<&'static str> {
    let mut found: Option<&'static str> = None;
    for change in diff.changes() {
        let path = change.path.as_str();
        let under = path == row
            || path
                .strip_prefix(row)
                .is_some_and(|rest| rest.starts_with('.') || rest.starts_with('['));
        if !under {
            continue;
        }
        if change.breaking {
            return Some("schema.breaking");
        }
        let key = if path == row && change.kind == ChangeKind::Added {
            "schema.added"
        } else {
            "schema.changed"
        };
        if found != Some("schema.added") {
            found = Some(key);
        }
    }
    found
}

/// `+2 -1 ~3, 1 breaking`, or `no changes`.
fn counts(diff: &SchemaDiff) -> String {
    let count = |kind: ChangeKind| diff.changes().iter().filter(|c| c.kind == kind).count();
    let mut parts = Vec::new();
    for (kind, marker) in [
        (ChangeKind::Added, "+"),
        (ChangeKind::Removed, "-"),
        (ChangeKind::Changed, "~"),
    ] {
        let n = count(kind);
        if n > 0 {
            parts.push(format!("{marker}{n}"));
        }
    }
    if parts.is_empty() {
        return "no changes".into();
    }
    let mut text = parts.join(" ");
    if diff.breaking() > 0 {
        text.push_str(&format!(", {} breaking", diff.breaking()));
    }
    text
}

impl Renderable for SchemaTimeline {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = self.timeline(console).rich_render(console, options);
        if let Some(note) = self.hidden_note(console) {
            if !segments.last().is_some_and(|s| s.text.ends_with('\n')) {
                segments.push(Segment::line());
            }
            segments.extend(note.rich_render(console, options));
        }
        if self.details {
            for tree in self.listing(console) {
                if !segments.last().is_some_and(|s| s.text.ends_with('\n')) {
                    segments.push(Segment::line());
                }
                segments.extend(tree.rich_render(console, options));
            }
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let mut measurement = self.timeline(console).measure(console, options);
        if let Some(note) = self.hidden_note(console) {
            let m = note.measure(console, options);
            measurement = rich::measure::Measurement::new(
                measurement.minimum.max(m.minimum),
                measurement.maximum.max(m.maximum),
            );
        }
        if self.details {
            for tree in self.listing(console) {
                let m = tree.measure(console, options);
                measurement = rich::measure::Measurement::new(
                    measurement.minimum.max(m.minimum),
                    measurement.maximum.max(m.maximum),
                );
            }
        }
        measurement
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{DataType, Field};

    /// Each row was looked up among all those before it: 200,000 columns
    /// took minutes.
    #[test]
    fn many_rows_are_gathered_in_linear_time() {
        let schema = Schema::of_tables((0..50).map(|t| {
            Schema::new((0..4_000).map(|i| Field::new(format!("c{i}"), DataType::Integer)))
                .named(format!("t{t}"))
        }));
        let timeline = SchemaTimeline::new()
            .push("v1", schema.clone())
            .push("v2", schema)
            .details(false)
            .charset(Charset::Ascii);
        let console = Console::builder().width(60).color_system(None).build();
        let started = std::time::Instant::now();
        let out = console.render_to_string(&timeline);
        assert!(started.elapsed() < std::time::Duration::from_secs(20));
        assert!(out.starts_with("t0.c0 "), "{out}");
        assert!(
            out.ends_with(
                "
… 199800 more fields"
            ),
            "{out}"
        );
    }

    /// The rows past [`MAX_ROWS`] were counted in a milestone at the first
    /// version's place, which its label always took: never shown.
    #[test]
    fn rows_not_drawn_are_counted() {
        let schema =
            Schema::new((0..MAX_ROWS + 5).map(|i| Field::new(format!("f{i}"), DataType::Integer)));
        let timeline = SchemaTimeline::new()
            .push("v1", schema)
            .charset(Charset::Ascii);
        let console = Console::builder().width(40).color_system(None).build();
        let out = console.render_to_string(&timeline);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), MAX_ROWS + 4, "{out}");
        assert!(lines[MAX_ROWS - 1].starts_with("f199 "));
        assert!(lines[MAX_ROWS].contains("* v1"));
        assert_eq!(lines[MAX_ROWS + 3], "… 5 more fields");
    }
}

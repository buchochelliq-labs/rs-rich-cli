//! Build times (#421): what `cargo build --timings` measured, as bars and a
//! table.
//!
//! [`Timings::parse`] reads any of:
//!
//! - the HTML report (`target/cargo-timings/cargo-timing*.html`), whose
//!   script holds every unit as JSON in `const UNIT_DATA = [...]`;
//! - that array on its own;
//! - the JSON lines `cargo build --timings=json -Zunstable-options` wrote
//!   on older nightly toolchains (`"reason": "timing-info"` messages; other
//!   messages and lines that are not JSON objects are skipped).
//!
//! Each [`Unit`] is one compilation: a crate's library, a binary, a build
//! script or a build script's run. Its time splits into the frontend (until
//! the `.rmeta` metadata was ready, which is what lets dependents start) and
//! codegen: from the report's `sections` where Cargo records them, else from
//! `rmeta_time`.
//!
//! [`TimingsReport`] draws the slowest units as bars, then a table of them,
//! with the totals above. Running `cargo` is the caller's business.
//!
//! ```
//! use rich::Console;
//! use rich_ext::deps::timings::{Timings, TimingsReport};
//!
//! let report = r#"const UNIT_DATA = [
//!   {"i": 0, "name": "syn", "version": "2.0.79", "mode": "todo", "target": "",
//!    "start": 0.0, "duration": 4.0, "rmeta_time": 1.5},
//!   {"i": 1, "name": "app", "version": "0.1.0", "mode": "todo", "target": " app \"bin\"",
//!    "start": 4.0, "duration": 1.0, "rmeta_time": null}
//! ];"#;
//! let timings = Timings::parse(report).unwrap();
//! assert_eq!(timings.units().len(), 2);
//! assert_eq!(timings.wall_clock(), Some(5.0));
//! let console = Console::builder().width(60).color_system(None).build();
//! let out = console.render_to_string(&TimingsReport::new(timings));
//! assert!(out.starts_with("2 units: 5.00s of compile time, 5.00s wall clock"));
//! ```

use rich::table::Table;
use rich::{Console, ConsoleOptions, Justify, Renderable, Segment, Text};
use serde_json::Value;

use super::{check_count, check_size, clean, stack, stack_measure, theme_style, DepsError};
use crate::chart::{BarChart, ValueFormat};

/// The slowest units [`TimingsReport`] shows by default.
pub const DEFAULT_LIMIT: usize = 20;

/// One compilation unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Unit {
    pub name: String,
    pub version: String,
    /// What was built, as Cargo writes it: empty for a library,
    /// `build-script`, `build-script (run)`, `app "bin"`, ….
    pub target: String,
    /// Cargo's mode: `todo` (a build), `run-custom-build`, `test`, ….
    pub mode: String,
    /// Seconds from the start of the build, when the report says.
    pub start: Option<f64>,
    /// Seconds it took.
    pub duration: f64,
    /// Seconds until its metadata (`.rmeta`) was ready: the frontend.
    pub rmeta: Option<f64>,
    /// Seconds of code generation after that.
    pub codegen: Option<f64>,
}

impl Unit {
    /// `name` and the target, as the bars label it: `syn`, `app build-script`.
    pub fn label(&self) -> String {
        if self.target.is_empty() {
            self.name.clone()
        } else {
            format!("{} {}", self.name, self.target)
        }
    }
}

/// The units of one build. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Timings {
    units: Vec<Unit>,
}

fn seconds(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s >= 0.0)
}

impl Timings {
    /// Units from any input the [module docs](self) list.
    pub fn parse(input: &str) -> Result<Self, DepsError> {
        check_size(input, "the timings report")?;
        let trimmed = input.trim_start_matches('\u{feff}').trim_start();
        if trimmed.starts_with('[') {
            let value: Value = serde_json::from_str(trimmed)
                .map_err(|e| DepsError::new(format!("not a timings unit list: {e}")))?;
            return Self::from_unit_data(&value);
        }
        // The first `UNIT_DATA =`: the name also appears in prose and in
        // the script that reads it.
        let assigned = input
            .match_indices("UNIT_DATA")
            .find_map(|(at, name)| input[at + name.len()..].trim_start().strip_prefix('='));
        if let Some(rest) = assigned {
            let mut values =
                serde_json::Deserializer::from_str(rest.trim_start()).into_iter::<Value>();
            let value = values
                .next()
                .ok_or_else(|| DepsError::new("the timings report's UNIT_DATA is empty"))?
                .map_err(|e| DepsError::new(format!("the timings report's UNIT_DATA: {e}")))?;
            return Self::from_unit_data(&value);
        }
        Self::from_json_lines(input)
    }

    /// Units from the report's `UNIT_DATA` array.
    pub fn from_unit_data(value: &Value) -> Result<Self, DepsError> {
        let list = value
            .as_array()
            .ok_or_else(|| DepsError::new("UNIT_DATA is not a list"))?;
        check_count(list.len(), "timing units")?;
        let mut units = Vec::with_capacity(list.len());
        for unit in list {
            let field = |key: &str| unit.get(key).and_then(Value::as_str);
            let (Some(name), Some(version), Some(duration)) = (
                field("name"),
                field("version"),
                seconds(unit.get("duration")),
            ) else {
                return Err(DepsError::new(
                    "a timing unit has no name, version or duration",
                ));
            };
            let section = |which: &str| {
                unit.get("sections")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|s| s.get(0).and_then(Value::as_str) == Some(which))
                    .and_then(|s| s.get(1))
                    .map(|span| (seconds(span.get("start")), seconds(span.get("end"))))
            };
            let (rmeta, codegen) = match (section("frontend"), section("codegen")) {
                (Some((_, Some(end))), codegen) => (
                    Some(end),
                    codegen
                        .and_then(|(start, end)| Some(end? - start?))
                        .filter(|s| *s >= 0.0),
                ),
                _ => {
                    let rmeta = seconds(unit.get("rmeta_time"));
                    (rmeta, rmeta.map(|r| (duration - r).max(0.0)))
                }
            };
            units.push(Unit {
                name: name.to_string(),
                version: version.to_string(),
                target: field("target").unwrap_or_default().trim().to_string(),
                mode: field("mode").unwrap_or("todo").to_string(),
                start: seconds(unit.get("start")),
                duration,
                rmeta,
                codegen,
            });
        }
        Ok(Timings { units })
    }

    /// Units from `timing-info` JSON lines.
    pub fn from_json_lines(input: &str) -> Result<Self, DepsError> {
        let mut units = Vec::new();
        for (number, line) in input.lines().enumerate() {
            let line = line.trim();
            if !line.starts_with('{') {
                continue;
            }
            let value: Value = serde_json::from_str(line)
                .map_err(|e| DepsError::new(format!("line {}: not JSON: {e}", number + 1)))?;
            if value.get("reason").and_then(Value::as_str) != Some("timing-info") {
                continue;
            }
            check_count(units.len() + 1, "timing units")?;
            let id = value
                .get("package_id")
                .and_then(Value::as_str)
                .ok_or_else(|| DepsError::new(format!("line {}: no package_id", number + 1)))?;
            let (name, version) = package_name(id);
            let duration = seconds(value.get("duration"))
                .ok_or_else(|| DepsError::new(format!("line {}: no duration", number + 1)))?;
            let mode = value
                .get("mode")
                .and_then(Value::as_str)
                .unwrap_or("build")
                .to_string();
            let kind = value
                .pointer("/target/kind/0")
                .and_then(Value::as_str)
                .unwrap_or("lib");
            let target_name = value
                .pointer("/target/name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let target = match (kind, mode.as_str()) {
                (_, "run-custom-build") => "build-script (run)".to_string(),
                ("custom-build", _) => "build-script".to_string(),
                ("lib" | "rlib" | "proc-macro", _) => String::new(),
                (kind, _) => format!("{target_name} \"{kind}\""),
            };
            let rmeta = seconds(value.get("rmeta_time"));
            units.push(Unit {
                name,
                version,
                target,
                mode,
                start: None,
                duration,
                rmeta,
                codegen: rmeta.map(|r| (duration - r).max(0.0)),
            });
        }
        if units.is_empty() {
            return Err(DepsError::new(
                "no timing data: expected a cargo-timing HTML report, its UNIT_DATA, \
                 or timing-info JSON lines",
            ));
        }
        Ok(Timings { units })
    }

    /// Timings of these units.
    pub fn new(units: Vec<Unit>) -> Self {
        Timings { units }
    }

    /// The units, in the report's order.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// The seconds every unit took, added up (more than the wall clock when
    /// units ran in parallel).
    pub fn total(&self) -> f64 {
        self.units.iter().map(|u| u.duration).sum()
    }

    /// The seconds from the first unit's start to the last one's end, when
    /// the report records starts.
    pub fn wall_clock(&self) -> Option<f64> {
        if self.units.is_empty() || self.units.iter().any(|u| u.start.is_none()) {
            return None;
        }
        let first = self
            .units
            .iter()
            .filter_map(|u| u.start)
            .fold(f64::INFINITY, f64::min);
        let last = self
            .units
            .iter()
            .filter_map(|u| Some(u.start? + u.duration))
            .fold(0.0, f64::max);
        Some(last - first.min(last))
    }

    /// The units, slowest first (by name on a tie).
    pub fn slowest(&self) -> Vec<&Unit> {
        let mut units: Vec<&Unit> = self.units.iter().collect();
        units.sort_by(|a, b| {
            b.duration
                .total_cmp(&a.duration)
                .then_with(|| a.label().cmp(&b.label()))
        });
        units
    }
}

/// The name and version in a Cargo package id: `name 1.0.0 (source)`, or
/// `source#name@1.0.0`, or `path+file:///…/name#1.0.0`.
fn package_name(id: &str) -> (String, String) {
    if let Some((url, fragment)) = id.rsplit_once('#') {
        if let Some((name, version)) = fragment.split_once('@') {
            return (name.to_string(), version.to_string());
        }
        let url = url.split('?').next().unwrap_or(url);
        let name = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
        return (name.to_string(), fragment.to_string());
    }
    let mut words = id.split_whitespace();
    let name = words.next().unwrap_or(id).to_string();
    let version = words.next().unwrap_or_default().to_string();
    (name, version)
}

fn secs(value: f64) -> String {
    format!("{value:.2}s")
}

/// The totals, the slowest units as bars, and a table of them. See the
/// [module docs](self).
#[derive(Clone, Debug)]
pub struct TimingsReport {
    timings: Timings,
    limit: usize,
}

impl TimingsReport {
    pub fn new(timings: Timings) -> Self {
        TimingsReport {
            timings,
            limit: DEFAULT_LIMIT,
        }
    }

    /// Show the `limit` slowest units ([`DEFAULT_LIMIT`] by default); the
    /// rest are counted underneath.
    pub fn limit(mut self, limit: usize) -> Self {
        self.limit = limit.max(1);
        self
    }

    pub fn timings(&self) -> &Timings {
        &self.timings
    }

    fn heading(&self, console: &Console) -> Text {
        let count = self.timings.units.len();
        let mut text = Text::new(format!(
            "{count} unit{}: {} of compile time",
            if count == 1 { "" } else { "s" },
            secs(self.timings.total())
        ));
        if let Some(wall) = self.timings.wall_clock() {
            text.append(&format!(", {} wall clock", secs(wall)), None);
        }
        let end = text.plain().len();
        text.stylize(theme_style(console, "deps.root"), 0, end);
        text
    }

    /// Bars labelled by [`Unit::label`], with the version for a crate built
    /// at several.
    fn bars(&self, shown: &[&Unit]) -> BarChart {
        let several = |unit: &Unit| {
            self.timings
                .units
                .iter()
                .any(|u| u.name == unit.name && u.version != unit.version)
        };
        BarChart::from_pairs(shown.iter().map(|u| {
            let label = if several(u) {
                let mut label = format!("{} v{}", u.name, u.version);
                if !u.target.is_empty() {
                    label.push(' ');
                    label.push_str(&u.target);
                }
                label
            } else {
                u.label()
            };
            (clean(&label).into_owned(), u.duration)
        }))
        .format(ValueFormat::Fixed(2))
    }

    fn table(&self, shown: &[&Unit], rest: &[&Unit], console: &Console) -> Table {
        let mut table = Table::new();
        table.add_column("Unit");
        table.add_column("Version");
        for header in ["Time", "Frontend", "Codegen"] {
            table.add_column_justify(header, Justify::Right);
        }
        let none = || Text::styled("–", theme_style(console, "deps.off"));
        for unit in shown {
            table.add_row_text(vec![
                Text::new(clean(&unit.label()).into_owned()),
                Text::styled(
                    format!("v{}", clean(&unit.version)),
                    theme_style(console, "deps.version"),
                ),
                Text::new(secs(unit.duration)),
                unit.rmeta.map_or_else(none, |s| Text::new(secs(s))),
                unit.codegen.map_or_else(none, |s| Text::new(secs(s))),
            ]);
        }
        if !rest.is_empty() {
            let time: f64 = rest.iter().map(|u| u.duration).sum();
            table = table
                .caption(format!(
                    "and {} more unit{}, {} in all",
                    rest.len(),
                    if rest.len() == 1 { "" } else { "s" },
                    secs(time)
                ))
                .caption_justify(Justify::Left);
        }
        table
    }

    fn parts(&self, console: &Console) -> (Text, BarChart, Table) {
        let slowest = self.timings.slowest();
        let (shown, rest) = slowest.split_at(self.limit.min(slowest.len()));
        (
            self.heading(console),
            self.bars(shown),
            self.table(shown, rest, console),
        )
    }
}

impl Renderable for TimingsReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let (heading, bars, table) = self.parts(console);
        stack(&[&heading, &bars, &table], console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let (heading, bars, table) = self.parts(console);
        stack_measure(&[&heading, &bars, &table], console, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_split_frontend_and_codegen() {
        let data = serde_json::json!([
            {"name": "lib1", "version": "0.1.0", "mode": "todo", "target": "",
             "start": 0.01, "duration": 0.03,
             "sections": [["frontend", {"start": 0.0, "end": 0.02}],
                          ["codegen", {"start": 0.02, "end": 0.03}]]},
            {"name": "app", "version": "0.1.0", "mode": "run-custom-build",
             "target": " build-script (run)", "start": 0.0, "duration": 0.0, "sections": null}
        ]);
        let timings = Timings::from_unit_data(&data).unwrap();
        let lib = &timings.units()[0];
        assert_eq!(lib.rmeta, Some(0.02));
        assert!((lib.codegen.unwrap() - 0.01).abs() < 1e-9);
        assert_eq!(timings.units()[1].label(), "app build-script (run)");
        assert_eq!(timings.units()[1].rmeta, None);
    }

    #[test]
    fn json_lines_are_read() {
        let lines = concat!(
            "   Compiling foo\n",
            r#"{"reason":"compiler-artifact","package_id":"x"}"#,
            "\n",
            r#"{"reason":"timing-info","package_id":"registry+https://github.com/rust-lang/crates.io-index#syn@2.0.79","target":{"kind":["lib"],"name":"syn"},"mode":"build","duration":2.5,"rmeta_time":1.0}"#,
            "\n",
            r#"{"reason":"timing-info","package_id":"app 0.1.0 (path+file:///w/app)","target":{"kind":["bin"],"name":"app"},"mode":"build","duration":0.5,"rmeta_time":null}"#,
            "\n",
            r#"{"reason":"timing-info","package_id":"path+file:///w/demo#0.3.0","target":{"kind":["custom-build"],"name":"build-script-build"},"mode":"build","duration":0.25}"#,
        );
        let timings = Timings::parse(lines).unwrap();
        let units = timings.units();
        assert_eq!(units.len(), 3);
        assert_eq!(
            (units[0].name.as_str(), units[0].version.as_str()),
            ("syn", "2.0.79")
        );
        assert_eq!(units[0].codegen, Some(1.5));
        assert_eq!(units[1].label(), "app app \"bin\"");
        assert_eq!(units[2].label(), "demo build-script");
        assert_eq!(timings.wall_clock(), None);
    }

    #[test]
    fn malformed_input_is_an_error() {
        assert!(Timings::parse("hello").is_err());
        assert!(Timings::parse("const UNIT_DATA = [{\"name\": 1}];").is_err());
        assert!(Timings::parse("const UNIT_DATA = [").is_err());
        assert!(Timings::parse("const UNIT_DATA").is_err());
        assert!(
            Timings::parse("UNIT_DATA holds units; const UNIT_DATA = [];")
                .unwrap()
                .units()
                .is_empty()
        );
        assert!(Timings::parse("{\"reason\": \"timing-info\"").is_err());
        assert!(Timings::parse("{\"reason\": \"timing-info\"}").is_err());
        let deep = format!("[{}{}]", "[".repeat(1000), "]".repeat(1000));
        assert!(Timings::parse(&deep).is_err());
    }

    #[test]
    fn the_report_limits_and_counts_the_rest() {
        let units = (0..5)
            .map(|i| Unit {
                name: format!("c{i}"),
                version: "1.0.0".into(),
                target: String::new(),
                mode: "todo".into(),
                start: Some(0.0),
                duration: f64::from(i + 1),
                rmeta: None,
                codegen: None,
            })
            .collect();
        let report = TimingsReport::new(Timings::new(units)).limit(2);
        let console = Console::builder().width(60).color_system(None).build();
        let out = console.render_to_string(&report);
        assert!(
            out.starts_with("5 units: 15.00s of compile time, 5.00s wall clock"),
            "{out}"
        );
        assert!(out.contains("c4"), "{out}");
        assert!(!out.contains("c2"), "{out}");
        assert!(out.contains("and 3 more units, 6.00s in all"), "{out}");
    }
}

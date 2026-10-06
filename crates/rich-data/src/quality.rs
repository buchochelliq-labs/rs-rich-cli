//! Data quality results (#269): a check-result model and a report that
//! renders it the way `rich_ext`'s test report renders test runs.
//!
//! This is not a check engine. A [`CheckResult`] is whatever a caller's
//! checks found: the check's name, the column it looked at, a [`Status`]
//! (pass, warn, fail or error), what it observed and expected, a message, and
//! a sample of the failing rows. [`not_null`] and [`unique`] are two small
//! built-in checks over [`Rows`], for the common cases.
//!
//! [`QualityReport`] shows the failures, errors and warnings first, each
//! with its observed and expected values and its failing rows as a table,
//! then a table of every check and a summary line. Names, messages and cells
//! are shown with terminal controls made visible, so data cannot move the
//! cursor or recolour the terminal. Statuses take the `quality.*` theme keys
//! in [`STYLES`].
//!
//! ```
//! use rich::Console;
//! use rich_data::quality::{self, CheckResult, QualityReport, Status};
//! use rich_data::{Rows, Value};
//!
//! let mut rows = Rows::new(["id", "email"]);
//! rows.push([Value::Int(1), "ada@example.com".into()]);
//! rows.push([Value::Int(2), Value::Null]);
//! rows.push([Value::Int(2), "alan@example.com".into()]);
//!
//! let report = QualityReport::new([
//!     quality::not_null(&rows, "email").unwrap(),
//!     quality::unique(&rows, "id").unwrap(),
//!     CheckResult::new("row_count", Status::Warn)
//!         .observed("3")
//!         .expected(">= 100"),
//! ]);
//! assert_eq!(report.totals().failed, 2);
//! assert!(!report.is_success());
//!
//! let console = Console::builder().width(60).color_system(None).build();
//! let out = console.render_export(&report);
//! assert!(out.starts_with("FAIL not_null › email\n  observed 1 null · expected 0\n"));
//! assert!(out.ends_with("0 passed, 1 warned, 2 failed\n"));
//! ```

use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};
use rich_ext::sanitize::{sanitize_single_line, sanitize_terminal_and_bidi_controls};
use serde::{Deserialize, Serialize};

use crate::profile::{block, join, style};
use crate::{DataError, Rows, Value};

/// The theme keys a quality report draws with, and their defaults. A console
/// theme that defines a key wins.
pub const STYLES: &[(&str, &str)] = &[
    ("quality.pass", "green"),
    ("quality.warn", "yellow"),
    ("quality.fail", "bold red"),
    ("quality.error", "bold magenta"),
    ("quality.check", "bold"),
    ("quality.dim", "dim"),
];

/// How many failing rows a built-in check keeps.
pub const FAILING_ROWS: usize = 5;

/// How a check ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The data met the expectation.
    Pass,
    /// Worth a look, but not a failure.
    Warn,
    /// The data did not meet the expectation.
    Fail,
    /// The check could not run.
    Error,
}

impl Status {
    /// The label a report writes: `PASS`, `WARN`, `FAIL` or `ERROR`.
    pub fn label(self) -> &'static str {
        match self {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Error => "ERROR",
        }
    }

    /// The theme key it is drawn in.
    pub fn key(self) -> &'static str {
        match self {
            Status::Pass => "quality.pass",
            Status::Warn => "quality.warn",
            Status::Fail => "quality.fail",
            Status::Error => "quality.error",
        }
    }
}

/// Some of the rows a check failed on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FailingRows {
    /// The column headings.
    pub columns: Vec<String>,
    /// The rows, as text.
    pub rows: Vec<Vec<String>>,
    /// How many rows failed in all, when more than `rows` holds.
    pub total: Option<u64>,
}

/// One check's result.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CheckResult {
    /// The check's name (`not_null`, `unique`, `row_count`, …).
    pub check: String,
    /// The column it looked at, if one.
    pub column: Option<String>,
    /// How it ended.
    pub status: Status,
    /// What it found.
    pub observed: Option<String>,
    /// What it wanted.
    pub expected: Option<String>,
    /// Anything else to say (an error's cause).
    pub message: Option<String>,
    /// A sample of the rows it failed on.
    pub failing_rows: Option<FailingRows>,
}

impl CheckResult {
    /// A result with nothing observed or expected yet.
    pub fn new(check: impl Into<String>, status: Status) -> Self {
        CheckResult {
            check: check.into(),
            column: None,
            status,
            observed: None,
            expected: None,
            message: None,
            failing_rows: None,
        }
    }

    /// The column checked.
    pub fn column(mut self, column: impl Into<String>) -> Self {
        self.column = Some(column.into());
        self
    }

    /// What the check found.
    pub fn observed(mut self, observed: impl Into<String>) -> Self {
        self.observed = Some(observed.into());
        self
    }

    /// What the check wanted.
    pub fn expected(mut self, expected: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self
    }

    /// A message.
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    /// A sample of the failing rows.
    pub fn failing_rows(mut self, rows: FailingRows) -> Self {
        self.failing_rows = Some(rows);
        self
    }

    /// `check › column`, or the check alone.
    pub fn title(&self) -> String {
        match &self.column {
            Some(column) => format!("{} › {}", self.check, column),
            None => self.check.clone(),
        }
    }
}

/// How many checks ended each way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// Passed.
    pub passed: usize,
    /// Warned.
    pub warned: usize,
    /// Failed.
    pub failed: usize,
    /// Could not run.
    pub errored: usize,
}

/// Results as a report: failures first, a table and a summary. See the
/// [module docs](self).
#[derive(Clone, Debug, PartialEq)]
pub struct QualityReport {
    results: Vec<CheckResult>,
    show_rows: usize,
}

impl QualityReport {
    /// A report of `results`, in order.
    pub fn new(results: impl IntoIterator<Item = CheckResult>) -> Self {
        QualityReport {
            results: results.into_iter().collect(),
            show_rows: FAILING_ROWS,
        }
    }

    /// Add a result.
    pub fn push(&mut self, result: CheckResult) -> &mut Self {
        self.results.push(result);
        self
    }

    /// The most failing rows shown per check (default [`FAILING_ROWS`]).
    pub fn show_rows(mut self, rows: usize) -> Self {
        self.show_rows = rows;
        self
    }

    /// The results, in order.
    pub fn results(&self) -> &[CheckResult] {
        &self.results
    }

    /// How many ended each way.
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for r in &self.results {
            match r.status {
                Status::Pass => t.passed += 1,
                Status::Warn => t.warned += 1,
                Status::Fail => t.failed += 1,
                Status::Error => t.errored += 1,
            }
        }
        t
    }

    /// Whether nothing failed or errored (warnings pass).
    pub fn is_success(&self) -> bool {
        let t = self.totals();
        t.failed == 0 && t.errored == 0
    }

    /// The summary: `3 passed, 1 warned, 2 failed`, with errors when any.
    pub fn summary(&self) -> String {
        let t = self.totals();
        let mut parts = vec![
            format!("{} passed", t.passed),
            format!("{} warned", t.warned),
            format!("{} failed", t.failed),
        ];
        if t.errored > 0 {
            parts.push(format!("{} errored", t.errored));
        }
        parts.join(", ")
    }

    /// The results as JSON: `results` and `totals`.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "results": self.results,
            "totals": self.totals(),
            "ok": self.is_success(),
        })
    }

    /// A table of every check: status, check, column, observed, expected.
    pub fn to_table(&self, console: &Console) -> Table {
        let mut table = Table::new();
        for heading in ["status", "check", "column", "observed", "expected"] {
            table.add_column(heading);
        }
        for r in &self.results {
            let line =
                |v: &Option<String>| Text::new(v.as_deref().map(one_line).unwrap_or_default());
            table.add_row_text(vec![
                Text::styled(r.status.label(), style(console, r.status.key(), STYLES)),
                Text::new(one_line(&r.check)),
                line(&r.column),
                line(&r.observed),
                line(&r.expected),
            ]);
        }
        table
    }

    fn details(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        r: &CheckResult,
    ) -> Vec<Vec<Segment>> {
        let mut lines = Vec::new();
        let mut title = Text::styled(
            format!("{} ", r.status.label()),
            style(console, r.status.key(), STYLES),
        );
        title.append(
            &one_line(&r.title()),
            Some(style(console, "quality.check", STYLES).into()),
        );
        block(console, options, &title, 0, &mut lines);
        let mut found = Vec::new();
        if let Some(observed) = &r.observed {
            found.push(format!("observed {}", one_line(observed)));
        }
        if let Some(expected) = &r.expected {
            found.push(format!("expected {}", one_line(expected)));
        }
        if !found.is_empty() {
            block(
                console,
                options,
                &Text::new(found.join(" · ")),
                2,
                &mut lines,
            );
        }
        if let Some(message) = &r.message {
            let message = sanitize_terminal_and_bidi_controls(message.trim_end());
            block(console, options, &Text::new(message), 2, &mut lines);
        }
        if let Some(failing) = r.failing_rows.as_ref().filter(|f| !f.rows.is_empty()) {
            let mut table = Table::new();
            for heading in &failing.columns {
                table.add_column(one_line(heading));
            }
            let width = failing.columns.len().max(1);
            if failing.columns.is_empty() {
                table.add_column("");
            }
            for row in failing.rows.iter().take(self.show_rows) {
                let mut cells: Vec<Text> = row
                    .iter()
                    .take(width)
                    .map(|c| Text::new(one_line(c)))
                    .collect();
                cells.resize_with(width, || Text::new(""));
                table.add_row_text(cells);
            }
            block(console, options, &table, 2, &mut lines);
            let shown = failing.rows.len().min(self.show_rows) as u64;
            let total = failing
                .total
                .unwrap_or(failing.rows.len() as u64)
                .max(shown);
            if total > shown {
                let more = Text::styled(
                    format!("{shown} of {total} failing rows shown"),
                    style(console, "quality.dim", STYLES),
                );
                block(console, options, &more, 2, &mut lines);
            }
        }
        lines
    }
}

impl Renderable for QualityReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if options.max_width == 0 {
            return Vec::new();
        }
        let options = options.reset_height();
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        // Failures and errors first, then warnings.
        for wanted in [[Status::Fail, Status::Error], [Status::Warn, Status::Warn]] {
            for r in self.results.iter().filter(|r| wanted.contains(&r.status)) {
                lines.extend(self.details(console, &options, r));
                lines.push(Vec::new());
            }
        }
        if !self.results.is_empty() {
            block(console, &options, &self.to_table(console), 0, &mut lines);
        }
        let t = self.totals();
        let mut total = Text::new("");
        let mut part = |n: usize, label: &str, status: Status, always: bool| {
            if n == 0 && !always {
                return;
            }
            if !total.is_empty() {
                total.append(", ", None);
            }
            let st = if n > 0 {
                Some(style(console, status.key(), STYLES).into())
            } else {
                None
            };
            total.append(&format!("{n} {label}"), st);
        };
        part(t.passed, "passed", Status::Pass, true);
        part(t.warned, "warned", Status::Warn, true);
        part(t.failed, "failed", Status::Fail, true);
        part(t.errored, "errored", Status::Error, false);
        block(console, &options, &total, 0, &mut lines);
        join(lines)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let widest = Segment::split_lines(&self.rich_render(console, options))
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0);
        rich::measure::Measurement::new(widest, widest)
    }
}

/// Data shown on one line, controls visible.
fn one_line(text: &str) -> String {
    sanitize_single_line(text)
}

/// `not_null`: every cell of `column` has a value. Nulls are
/// [`Value::Null`] and empty text. Fails with the first null rows (`row` is
/// counted from 1); an unknown column is an error.
pub fn not_null(rows: &Rows, column: &str) -> Result<CheckResult, DataError> {
    let index = column_index(rows, column)?;
    let failing: Vec<usize> = rows
        .rows()
        .iter()
        .enumerate()
        .filter(|(_, row)| row.get(index).is_none_or(Value::is_empty))
        .map(|(i, _)| i)
        .collect();
    let count = failing.len() as u64;
    let result = CheckResult::new("not_null", status(count))
        .column(column)
        .observed(format!(
            "{count} {}",
            if count == 1 { "null" } else { "nulls" }
        ))
        .expected("0");
    Ok(with_rows(result, rows, &failing))
}

/// `unique`: no two non-null cells of `column` hold the same value (by
/// text). Fails with the rows holding a repeated value, after its first;
/// an unknown column is an error.
pub fn unique(rows: &Rows, column: &str) -> Result<CheckResult, DataError> {
    let index = column_index(rows, column)?;
    let mut seen = std::collections::HashSet::new();
    let mut failing = Vec::new();
    for (i, row) in rows.rows().iter().enumerate() {
        let Some(value) = row.get(index).filter(|v| !v.is_empty()) else {
            continue;
        };
        if !seen.insert(value.plain()) {
            failing.push(i);
        }
    }
    let count = failing.len() as u64;
    let result = CheckResult::new("unique", status(count))
        .column(column)
        .observed(format!(
            "{count} {}",
            if count == 1 {
                "duplicate"
            } else {
                "duplicates"
            }
        ))
        .expected("0");
    Ok(with_rows(result, rows, &failing))
}

fn column_index(rows: &Rows, column: &str) -> Result<usize, DataError> {
    rows.column_index(column).ok_or_else(|| {
        DataError::new(format!(
            "no column {:?}; the columns are {}",
            one_line(column),
            rows.columns()
                .iter()
                .map(|c| one_line(c))
                .collect::<Vec<_>>()
                .join(", ")
        ))
    })
}

fn status(failures: u64) -> Status {
    if failures == 0 {
        Status::Pass
    } else {
        Status::Fail
    }
}

/// Attach the first [`FAILING_ROWS`] of `failing` (indices into `rows`),
/// each with its row number.
fn with_rows(result: CheckResult, rows: &Rows, failing: &[usize]) -> CheckResult {
    if failing.is_empty() {
        return result;
    }
    let mut columns = vec!["row".to_string()];
    columns.extend(rows.columns().iter().cloned());
    let sample = failing
        .iter()
        .take(FAILING_ROWS)
        .map(|&i| {
            let mut cells = vec![(i + 1).to_string()];
            cells.extend(rows.rows()[i].iter().map(Value::plain));
            cells
        })
        .collect();
    result.failing_rows(FailingRows {
        columns,
        rows: sample,
        total: Some(failing.len() as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn people() -> Rows {
        let mut rows = Rows::new(["id", "name"]);
        for (id, name) in [(1, "ada"), (2, ""), (2, "alan"), (3, "grace"), (3, "")] {
            rows.push([Value::Int(id), name.into()]);
        }
        rows
    }

    #[test]
    fn built_in_checks_find_their_rows() {
        let rows = people();
        let nulls = not_null(&rows, "name").unwrap();
        assert_eq!(nulls.status, Status::Fail);
        assert_eq!(nulls.observed.as_deref(), Some("2 nulls"));
        let failing = nulls.failing_rows.unwrap();
        assert_eq!(failing.columns, ["row", "id", "name"]);
        assert_eq!(failing.rows, [["2", "2", ""], ["5", "3", ""]]);
        let dupes = unique(&rows, "id").unwrap();
        assert_eq!(dupes.observed.as_deref(), Some("2 duplicates"));
        assert_eq!(dupes.failing_rows.unwrap().rows[0][0], "3");
        let ok = unique(&rows, "name").unwrap();
        assert_eq!(ok.status, Status::Pass);
        assert!(ok.failing_rows.is_none());
        let error = not_null(&rows, "nope").unwrap_err();
        assert_eq!(
            error.to_string(),
            "no column \"nope\"; the columns are id, name"
        );
    }

    #[test]
    fn the_report_shows_failures_first_then_a_table_and_a_summary() {
        let rows = people();
        let report = QualityReport::new([
            CheckResult::new("row_count", Status::Pass)
                .observed("5")
                .expected(">= 1"),
            not_null(&rows, "name").unwrap(),
            CheckResult::new("freshness", Status::Error).message("no \x1b[31mclock\x1b[0m"),
            CheckResult::new("range", Status::Warn)
                .column("id")
                .observed("max 3"),
        ])
        .show_rows(1);
        let console = Console::builder().width(60).color_system(None).build();
        let out = console.render_export(&report);
        let expected = concat!(
            "FAIL not_null › name\n",
            "  observed 2 nulls · expected 0\n",
            "  ┏━━━━━┳━━━━┳━━━━━━┓\n",
            "  ┃ row ┃ id ┃ name ┃\n",
            "  ┡━━━━━╇━━━━╇━━━━━━┩\n",
            "  │ 2   │ 2  │      │\n",
            "  └─────┴────┴──────┘\n",
            "  1 of 2 failing rows shown\n",
            "\n",
            "ERROR freshness\n",
            "  no ␛[31mclock␛[0m\n",
            "\n",
            "WARN range › id\n",
            "  observed max 3\n",
            "\n",
            "┏━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━┳━━━━━━━━━━┳━━━━━━━━━━┓\n",
            "┃ status ┃ check     ┃ column ┃ observed ┃ expected ┃\n",
            "┡━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━╇━━━━━━━━━━╇━━━━━━━━━━┩\n",
            "│ PASS   │ row_count │        │ 5        │ >= 1     │\n",
            "│ FAIL   │ not_null  │ name   │ 2 nulls  │ 0        │\n",
            "│ ERROR  │ freshness │        │          │          │\n",
            "│ WARN   │ range     │ id     │ max 3    │          │\n",
            "└────────┴───────────┴────────┴──────────┴──────────┘\n",
            "1 passed, 1 warned, 1 failed, 1 errored\n",
        );
        assert_eq!(out, expected);
        assert_eq!(report.to_json()["totals"]["errored"], 1);
        assert_eq!(report.to_json()["results"][1]["status"], "fail");
    }

    #[test]
    fn status_colours_come_from_the_theme_keys() {
        let report = QualityReport::new([CheckResult::new("x", Status::Fail)]);
        let console = Console::builder()
            .width(40)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        let out = console.render_export(&report);
        assert!(out.contains("\x1b[1;31mFAIL"), "{out:?}");
    }
}

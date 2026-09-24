//! Test results: a format-neutral model, JUnit XML and libtest JSON
//! adapters, a renderable [`TestReport`] and JUnit export.
//!
//! [`TestReport`] shows failures first — each with its message, captured
//! output and, when both are known, a [`DiffView`] of
//! expected against actual — then a per-suite summary table and a total
//! line. It is an ordinary renderable, so console HTML and SVG export apply;
//! [`TestRun::to_junit_xml`] writes normalized JUnit for CI.
//!
//! The parsers decode escapes (JUnit's `&#x1b;`, JSON's `\u001b`), so a
//! [`TestRun`] holds names, messages and output exactly as the report gave
//! them, control characters included. [`TestReport`] shows every such field
//! with terminal and bidi controls made visible (`␛[31m`), so a report
//! cannot move the cursor, recolour or reorder the terminal it is shown on.

pub mod junit;
pub mod libtest;

use std::fmt;
use std::time::Duration;

use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Table, Text};
use serde::{Deserialize, Serialize};

use super::render::{banner, join, trim_end, wrap_text};
use super::{style, DiffView};
use crate::sanitize::{sanitize_single_line, sanitize_terminal_and_bidi_controls};

/// Multi-line report text (a message, a trace, captured output), inert.
fn inert(text: &str) -> String {
    sanitize_terminal_and_bidi_controls(text)
}

/// How a test case ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Status {
    Passed,
    Failed,
    Skipped,
    /// An error outside the assertion (JUnit `<error>`).
    Errored,
}

impl Status {
    fn key(self) -> &'static str {
        match self {
            Status::Passed => "test.passed",
            Status::Failed => "test.failed",
            Status::Skipped => "test.skipped",
            Status::Errored => "test.errored",
        }
    }
    fn label(self) -> &'static str {
        match self {
            Status::Passed => "PASSED",
            Status::Failed => "FAILED",
            Status::Skipped => "SKIPPED",
            Status::Errored => "ERROR",
        }
    }
}

/// One test case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Case {
    pub name: String,
    pub classname: String,
    pub status: Status,
    pub duration: Option<Duration>,
    /// The failure, error or skip message.
    pub message: Option<String>,
    /// The failure body (a stack trace or assertion detail).
    pub details: Option<String>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub expected: Option<String>,
    pub actual: Option<String>,
}

impl Case {
    /// A case with no message, output or duration.
    pub fn new(name: impl Into<String>, classname: impl Into<String>, status: Status) -> Self {
        Case {
            name: name.into(),
            classname: classname.into(),
            status,
            duration: None,
            message: None,
            details: None,
            stdout: None,
            stderr: None,
            expected: None,
            actual: None,
        }
    }
    /// `classname.name`, or the name alone when it already includes the
    /// class (libtest paths, jest-junit titles).
    pub fn full_name(&self) -> String {
        if self.classname.is_empty() || self.name.contains(&self.classname) {
            self.name.clone()
        } else {
            format!("{}.{}", self.classname, self.name)
        }
    }
    /// Whether this case failed or errored.
    pub fn is_failure(&self) -> bool {
        matches!(self.status, Status::Failed | Status::Errored)
    }
}

/// A suite of cases.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suite {
    pub name: String,
    pub cases: Vec<Case>,
    pub duration: Option<Duration>,
}

/// Counts of each status.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Totals {
    pub passed: usize,
    pub failed: usize,
    pub errored: usize,
    pub skipped: usize,
}

impl Totals {
    fn add(&mut self, status: Status) {
        match status {
            Status::Passed => self.passed += 1,
            Status::Failed => self.failed += 1,
            Status::Errored => self.errored += 1,
            Status::Skipped => self.skipped += 1,
        }
    }
    /// All cases.
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.errored + self.skipped
    }
}

impl Suite {
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for case in &self.cases {
            t.add(case.status);
        }
        t
    }
    /// The suite's duration, else the sum of its cases'.
    pub fn time(&self) -> Option<Duration> {
        self.duration.or_else(|| {
            let times: Vec<Duration> = self.cases.iter().filter_map(|c| c.duration).collect();
            (!times.is_empty()).then(|| times.iter().sum())
        })
    }
}

/// A whole test run.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestRun {
    pub suites: Vec<Suite>,
    pub duration: Option<Duration>,
}

/// Why test results did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestParseError {
    pub format: &'static str,
    pub message: String,
}

impl fmt::Display for TestParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.format, self.message)
    }
}
impl std::error::Error for TestParseError {}

/// A test-result format. Implement it to feed [`TestReport`] from another
/// runner.
pub trait TestAdapter {
    /// The format's name.
    fn name(&self) -> &'static str;
    /// Parse a whole report.
    fn parse(&self, input: &str) -> Result<TestRun, TestParseError>;
}

impl TestRun {
    pub fn totals(&self) -> Totals {
        let mut t = Totals::default();
        for case in self.suites.iter().flat_map(|s| &s.cases) {
            t.add(case.status);
        }
        t
    }
    /// The run's duration, else the sum of its suites'.
    pub fn time(&self) -> Option<Duration> {
        self.duration.or_else(|| {
            let times: Vec<Duration> = self.suites.iter().filter_map(Suite::time).collect();
            (!times.is_empty()).then(|| times.iter().sum())
        })
    }
    /// Whether nothing failed or errored.
    pub fn is_success(&self) -> bool {
        let t = self.totals();
        t.failed == 0 && t.errored == 0
    }
    /// Normalized JUnit XML, which [`junit::parse`] reads back.
    pub fn to_junit_xml(&self) -> String {
        junit::write(self)
    }
}

/// A duration from seconds, rounded to the nanosecond; `None` when negative
/// or not finite.
pub(crate) fn duration(secs: f64) -> Option<Duration> {
    (0.0..1e9)
        .contains(&secs)
        .then(|| Duration::from_nanos((secs * 1e9).round() as u64))
}

/// Seconds with millisecond precision, as JUnit writes them.
pub(crate) fn seconds(d: Duration) -> String {
    format!("{:.3}", d.as_secs_f64())
}

/// `expected` and `actual` from common assertion messages: libtest's
/// `left`/`right`, JUnit 4's `expected:<a> but was:<b>` and Jest's
/// `Expected:`/`Received:` lines.
pub(crate) fn expected_actual(text: &str) -> Option<(String, String)> {
    if let Some(found) = libtest::left_right(text) {
        return Some(found);
    }
    if let Some(rest) = text.split("expected:<").nth(1) {
        if let Some((expected, rest)) = rest.split_once("> but was:<") {
            if let Some(actual) = rest.rfind('>').map(|i| &rest[..i]) {
                return Some((expected.to_string(), actual.to_string()));
            }
        }
    }
    let mut expected = None;
    let mut actual = None;
    for line in text.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("Expected:") {
            expected.get_or_insert_with(|| v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("Received:") {
            actual.get_or_insert_with(|| v.trim().to_string());
        }
    }
    expected.zip(actual)
}

/// A renderable test report: failures first, a per-suite summary table and
/// a total line.
#[derive(Clone, Debug)]
pub struct TestReport {
    run: TestRun,
    show_passed: bool,
    show_output: bool,
    diff_context: usize,
}

impl TestReport {
    pub fn new(run: TestRun) -> Self {
        TestReport {
            run,
            show_passed: false,
            show_output: true,
            diff_context: 3,
        }
    }
    /// List passing and skipped cases after the failures (default off).
    pub fn show_passed(mut self, show: bool) -> Self {
        self.show_passed = show;
        self
    }
    /// Show captured output under failures (default on).
    pub fn show_output(mut self, show: bool) -> Self {
        self.show_output = show;
        self
    }
    /// Context lines in expected/actual diffs (default 3).
    pub fn diff_context(mut self, lines: usize) -> Self {
        self.diff_context = lines;
        self
    }
    /// The run shown.
    pub fn run(&self) -> &TestRun {
        &self.run
    }

    fn case_title(&self, console: &Console, suite: &Suite, case: &Case) -> Text {
        let mut text = Text::new("");
        text.append(
            &format!("{} ", case.status.label()),
            Some(style(console, case.status.key()).into()),
        );
        if !suite.name.is_empty() {
            text.append(
                &format!("{} > ", sanitize_single_line(&suite.name)),
                Some(style(console, "diff.line_number").into()),
            );
        }
        text.append(
            &sanitize_single_line(&case.full_name()),
            Some(style(console, "diff.header").into()),
        );
        if let Some(d) = case.duration {
            text.append(
                &format!(" ({}s)", seconds(d)),
                Some(style(console, "diff.line_number").into()),
            );
        }
        text
    }

    fn indented(console: &Console, text: &Text, width: usize, indent: usize) -> Vec<Vec<Segment>> {
        let pad = Segment::new(" ".repeat(indent), None);
        wrap_text(
            console,
            text,
            &Style::new(),
            width.saturating_sub(indent).max(1),
            true,
        )
        .into_iter()
        .map(|line| {
            let mut row = vec![pad.clone()];
            row.extend(line);
            trim_end(row)
        })
        .collect()
    }

    fn failure_rows(
        &self,
        console: &Console,
        suite: &Suite,
        case: &Case,
        width: usize,
    ) -> Vec<Vec<Segment>> {
        let mut rows = Self::indented(console, &self.case_title(console, suite, case), width, 0);
        let dim = style(console, "diff.line_number");
        if let Some(message) = &case.message {
            rows.extend(Self::indented(
                console,
                &Text::new(inert(message.trim_end())),
                width,
                2,
            ));
        }
        if let (Some(expected), Some(actual)) = (&case.expected, &case.actual) {
            let mut e = inert(expected);
            let mut a = inert(actual);
            e.push('\n');
            a.push('\n');
            let view = DiffView::new(&e, &a)
                .titles("expected", "actual")
                .context(self.diff_context);
            let opts = console
                .options()
                .update_width(width.saturating_sub(2).max(1));
            let pad = Segment::new("  ", None);
            for row in Segment::split_lines(&view.rich_render(console, &opts)) {
                let mut r = vec![pad.clone()];
                r.extend(row);
                rows.push(trim_end(r));
            }
        } else if let Some(details) = &case.details {
            if case.message.as_deref().map(str::trim) != Some(details.trim()) {
                rows.extend(Self::indented(
                    console,
                    &Text::new(inert(details.trim_end())),
                    width,
                    2,
                ));
            }
        }
        if self.show_output {
            for (label, output) in [("stdout", &case.stdout), ("stderr", &case.stderr)] {
                let Some(output) = output.as_deref().filter(|o| !o.trim().is_empty()) else {
                    continue;
                };
                rows.extend(banner(&format!("  captured {label}:"), dim.clone(), width));
                rows.extend(Self::indented(
                    console,
                    &Text::new(inert(output.trim_end())),
                    width,
                    4,
                ));
            }
        }
        rows
    }

    fn table(&self) -> Table {
        let mut table = Table::new();
        for h in ["Suite", "Passed", "Failed", "Skipped", "Time"] {
            table.add_column(h);
        }
        for suite in &self.run.suites {
            let t = suite.totals();
            let time = suite
                .time()
                .map(|d| format!("{}s", seconds(d)))
                .unwrap_or_default();
            // Suite names are data: pytest ids such as `test_x[a]` stay literal.
            table.add_row_text(vec![
                Text::new(sanitize_single_line(&suite.name)),
                Text::new(t.passed.to_string()),
                Text::new((t.failed + t.errored).to_string()),
                Text::new(t.skipped.to_string()),
                Text::new(time),
            ]);
        }
        table
    }
}

impl Renderable for TestReport {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        if width == 0 || options.height == Some(0) {
            return Vec::new();
        }
        let mut rows: Vec<Vec<Segment>> = Vec::new();
        for suite in &self.run.suites {
            for case in suite.cases.iter().filter(|c| c.is_failure()) {
                rows.extend(self.failure_rows(console, suite, case, width));
                rows.push(Vec::new());
            }
        }
        if self.show_passed {
            let mut any = false;
            for suite in &self.run.suites {
                for case in suite.cases.iter().filter(|c| !c.is_failure()) {
                    rows.extend(Self::indented(
                        console,
                        &self.case_title(console, suite, case),
                        width,
                        0,
                    ));
                    any = true;
                }
            }
            if any {
                rows.push(Vec::new());
            }
        }
        if !self.run.suites.is_empty() {
            let table = self.table();
            let rendered = table.rich_render(console, &options.update_width(width));
            rows.extend(Segment::split_lines(&rendered).into_iter().map(trim_end));
        }
        let t = self.run.totals();
        let mut total = Text::new("");
        let mut part = |n: usize, label: &str, key: &str, always: bool| {
            if n == 0 && !always {
                return;
            }
            if !total.is_empty() {
                total.append(", ", None);
            }
            let st = if n > 0 {
                style(console, key)
            } else {
                Style::new()
            };
            total.append(&format!("{n} {label}"), Some(st.into()));
        };
        part(t.passed, "passed", "test.passed", true);
        part(t.failed, "failed", "test.failed", true);
        part(t.errored, "errored", "test.errored", false);
        part(t.skipped, "skipped", "test.skipped", false);
        if let Some(d) = self.run.time() {
            total.append(&format!(" in {}s", seconds(d)), None);
        }
        rows.extend(Self::indented(console, &total, width, 0));
        join(rows, options.height)
    }
}

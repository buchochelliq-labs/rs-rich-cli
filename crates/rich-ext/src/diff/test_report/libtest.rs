//! libtest's JSON event stream (`cargo test -- -Z unstable-options
//! --format json`, nightly).
//!
//! Reads `suite` events (`started`, `ok`, `failed`) and `test` events
//! (`started`, `ok`, `failed`, `ignored`, `bench`) with `exec_time` and
//! `stdout`. Each `suite started` opens a new suite; a preceding cargo
//! `Running <target> (…)` line, when the stream includes stderr, names it.
//! Other lines — cargo's `--message-format json` records, plain text — are
//! skipped. A failure's `assertion `left == right` failed` block becomes its
//! expected (`right`) and actual (`left`) values.

use std::collections::HashMap;

use serde_json::Value;

use super::{duration, Case, Status, Suite, TestAdapter, TestParseError, TestRun};

/// The libtest JSON adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct LibTest;

impl TestAdapter for LibTest {
    fn name(&self) -> &'static str {
        "libtest"
    }
    fn parse(&self, input: &str) -> Result<TestRun, TestParseError> {
        parse(input)
    }
}

/// Strip the old format's backticks and trailing comma: `` `1`, ``.
fn clean(value: &str) -> String {
    let v = value.trim_end();
    let v = v.strip_suffix(',').unwrap_or(v);
    let v = v
        .strip_prefix('`')
        .and_then(|v| v.strip_suffix('`'))
        .unwrap_or(v);
    v.to_string()
}

/// `(expected, actual)` from an `assert_eq!` panic's `left:`/`right:` block:
/// `right` as expected and `left` as actual, the `assert_eq!(actual,
/// expected)` convention.
pub(crate) fn left_right(text: &str) -> Option<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.trim_start().starts_with("left:"))?;
    let mut left = vec![lines[start].trim_start()["left:".len()..].trim_start()];
    let mut i = start + 1;
    while i < lines.len() && !lines[i].trim_start().starts_with("right:") {
        left.push(lines[i]);
        i += 1;
    }
    let right_first = lines.get(i)?.trim_start()["right:".len()..].trim_start();
    let mut right = vec![right_first];
    i += 1;
    while i < lines.len() {
        let l = lines[i];
        if l.is_empty()
            || l.starts_with("note:")
            || l.starts_with("stack backtrace:")
            || l.starts_with("thread '")
        {
            break;
        }
        right.push(l);
        i += 1;
    }
    Some((clean(&right.join("\n")), clean(&left.join("\n"))))
}

/// The panic message from captured output: the lines after `panicked at`,
/// up to the `left:`/`right:` block or the backtrace note.
fn panic_message(stdout: &str) -> Option<String> {
    let lines: Vec<&str> = stdout.lines().collect();
    let at = lines.iter().position(|l| l.contains("panicked at"))?;
    let header = lines[at];
    let mut message: Vec<&str> = Vec::new();
    // Old format: `panicked at 'message', src/lib.rs:1:2`.
    if let Some(rest) = header.split("panicked at '").nth(1) {
        if let Some(end) = rest.rfind("', ") {
            message.push(&rest[..end]);
        }
    }
    for l in &lines[at + 1..] {
        let t = l.trim_start();
        if t.starts_with("left:") || l.starts_with("note:") || l.starts_with("stack backtrace:") {
            break;
        }
        message.push(l);
    }
    let message = message.join("\n").trim().to_string();
    (!message.is_empty()).then_some(message)
}

fn secs(event: &Value) -> Option<std::time::Duration> {
    event
        .get("exec_time")
        .and_then(Value::as_f64)
        .and_then(duration)
}

/// Parse a libtest JSON event stream.
pub fn parse(input: &str) -> Result<TestRun, TestParseError> {
    let mut run = TestRun::default();
    let mut current: Option<Suite> = None;
    // The current suite's cases by name, so each event finds its case in
    // constant time; cases stay in order of first appearance.
    let mut by_name: HashMap<String, usize> = HashMap::new();
    let mut next_name: Option<String> = None;
    let mut events = 0usize;
    for (index, line) in input.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Running ") {
            // cargo's stderr: `Running unittests src/lib.rs (target/…)`.
            let name = rest.split(" (").next().unwrap_or(rest).trim();
            next_name = Some(name.to_string());
            continue;
        }
        if !trimmed.starts_with('{') {
            continue;
        }
        let event: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                return Err(TestParseError {
                    format: "libtest",
                    message: format!("line {}: {e}", index + 1),
                })
            }
        };
        let kind = event.get("type").and_then(Value::as_str);
        let name = event.get("event").and_then(Value::as_str).unwrap_or("");
        match kind {
            Some("suite") => {
                events += 1;
                match name {
                    "started" => {
                        if let Some(suite) = current.take() {
                            run.suites.push(suite);
                        }
                        by_name.clear();
                        let n = run.suites.len() + 1;
                        current = Some(Suite {
                            name: next_name.take().unwrap_or_else(|| format!("suite {n}")),
                            cases: Vec::new(),
                            duration: None,
                        });
                    }
                    _ => {
                        let mut suite = current.take().unwrap_or_default();
                        by_name.clear();
                        suite.duration = secs(&event);
                        run.suites.push(suite);
                    }
                }
            }
            Some("test") => {
                events += 1;
                let Some(test) = event.get("name").and_then(Value::as_str) else {
                    continue;
                };
                let suite = current.get_or_insert_with(|| Suite {
                    name: next_name.take().unwrap_or_else(|| "suite 1".into()),
                    cases: Vec::new(),
                    duration: None,
                });
                let classname = test.rsplit_once("::").map_or("", |(m, _)| m);
                let slot = match by_name.get(test) {
                    Some(&i) => i,
                    None => {
                        suite.cases.push(Case::new(test, classname, Status::Passed));
                        by_name.insert(test.to_string(), suite.cases.len() - 1);
                        suite.cases.len() - 1
                    }
                };
                let case = &mut suite.cases[slot];
                // Captured output is kept without its final newline, as JUnit
                // readers normalize it.
                let stdout = event
                    .get("stdout")
                    .and_then(Value::as_str)
                    .map(|s| s.trim_end_matches('\n').to_string())
                    .filter(|s| !s.is_empty());
                match name {
                    "started" | "timeout" => {}
                    "ok" | "bench" => {
                        case.status = Status::Passed;
                        case.duration = secs(&event);
                        case.stdout = stdout;
                    }
                    "ignored" => {
                        case.status = Status::Skipped;
                        case.message = event
                            .get("message")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                    }
                    _ => {
                        // "failed", and anything else that ends a test.
                        case.status = Status::Failed;
                        case.duration = secs(&event);
                        let reason = event.get("reason").and_then(Value::as_str);
                        case.message = stdout
                            .as_deref()
                            .and_then(panic_message)
                            .or_else(|| reason.map(str::to_string));
                        if let Some((expected, actual)) = stdout.as_deref().and_then(left_right) {
                            case.expected = Some(expected);
                            case.actual = Some(actual);
                        }
                        case.stdout = stdout;
                    }
                }
            }
            _ => {}
        }
    }
    if let Some(suite) = current.take() {
        run.suites.push(suite);
    }
    if events == 0 {
        return Err(TestParseError {
            format: "libtest",
            message: "no libtest JSON events".into(),
        });
    }
    Ok(run)
}

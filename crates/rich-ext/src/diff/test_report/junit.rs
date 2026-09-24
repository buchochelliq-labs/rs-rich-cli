//! JUnit XML: `testsuites`/`testsuite`/`testcase` with `failure`, `error`,
//! `skipped`, `system-out` and `system-err`, and `time` attributes.
//!
//! Tolerates the common dialects: Maven Surefire (a bare `testsuite` root,
//! `properties`, JUnit 4 `expected:<a> but was:<b>` messages), pytest
//! (`skipped` with a message, `system-out` per case) and jest-junit (failure
//! text without a `message` attribute, `Expected:`/`Received:` lines).
//! Nested suites are flattened. Case-level `expected`/`actual` properties,
//! which [`write()`] emits, are read back.
//!
//! A file that ends while elements are still open — a report cut short by a
//! crashed or killed runner — is an error, not a partial run: the failing
//! case being written when it stopped would otherwise be lost, and the
//! report would read as a success.

use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{expected_actual, Case, Status, Suite, TestAdapter, TestParseError, TestRun};

/// The JUnit XML adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct JUnit;

impl TestAdapter for JUnit {
    fn name(&self) -> &'static str {
        "junit"
    }
    fn parse(&self, input: &str) -> Result<TestRun, TestParseError> {
        parse(input)
    }
}

fn error(message: impl Into<String>) -> TestParseError {
    TestParseError {
        format: "junit",
        message: message.into(),
    }
}

fn attrs(start: &BytesStart<'_>) -> Result<Vec<(String, String)>, TestParseError> {
    let mut out = Vec::new();
    for attribute in start.attributes() {
        let attribute = attribute.map_err(|e| error(e.to_string()))?;
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| error(e.to_string()))?;
        out.push((attribute.key.as_ref().to_string(), value.into_owned()));
    }
    Ok(out)
}

fn get<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v.as_str())
}

/// A `time` attribute in seconds. A comma is a thousands separator next to a
/// decimal point (`1,234.5`) and a decimal comma otherwise (`0,002`, from
/// locale-formatting runners).
fn time(attrs: &[(String, String)]) -> Option<Duration> {
    let raw = get(attrs, "time")?.trim();
    let raw = if raw.contains('.') {
        raw.replace(',', "")
    } else {
        raw.replace(',', ".")
    };
    super::duration(raw.parse().ok()?)
}

/// Exact seconds: nanosecond precision, trailing zeros trimmed to three
/// decimals, so a written report reads back to the same durations.
fn exact_seconds(d: Duration) -> String {
    let mut s = format!("{}.{:09}", d.as_secs(), d.subsec_nanos());
    while s.ends_with('0') && s.len() - s.find('.').unwrap_or(0) > 4 {
        s.pop();
    }
    s
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Capture {
    None,
    Failure,
    Stdout,
    Stderr,
    Skipped,
}

/// Parse JUnit XML.
pub fn parse(input: &str) -> Result<TestRun, TestParseError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let mut reader = Reader::from_str(input);
    let mut run = TestRun::default();
    let mut suites: Vec<Suite> = Vec::new();
    let mut case: Option<Case> = None;
    let mut capture = Capture::None;
    let mut text = String::new();
    let mut saw_root = false;
    // Elements opened and not yet closed, for truncation.
    let mut open: Vec<String> = Vec::new();

    fn start_element(
        name: &str,
        a: Vec<(String, String)>,
        run: &mut TestRun,
        suites: &mut Vec<Suite>,
        case: &mut Option<Case>,
        capture: &mut Capture,
        text: &mut String,
    ) {
        match name {
            "testsuites" => run.duration = time(&a),
            "testsuite" => suites.push(Suite {
                name: get(&a, "name").unwrap_or_default().to_string(),
                cases: Vec::new(),
                duration: time(&a),
            }),
            "testcase" => {
                let mut c = Case::new(
                    get(&a, "name").unwrap_or_default(),
                    get(&a, "classname")
                        .or(get(&a, "class"))
                        .unwrap_or_default(),
                    Status::Passed,
                );
                c.duration = time(&a);
                *case = Some(c);
            }
            "failure" | "error" => {
                if let Some(c) = case.as_mut() {
                    c.status = if name == "failure" {
                        Status::Failed
                    } else {
                        Status::Errored
                    };
                    c.message = get(&a, "message").map(str::to_string);
                }
                *capture = Capture::Failure;
                text.clear();
            }
            "skipped" => {
                if let Some(c) = case.as_mut() {
                    c.status = Status::Skipped;
                    c.message = get(&a, "message").map(str::to_string);
                }
                *capture = Capture::Skipped;
                text.clear();
            }
            "system-out" => {
                *capture = Capture::Stdout;
                text.clear();
            }
            "system-err" => {
                *capture = Capture::Stderr;
                text.clear();
            }
            "property" => {
                if let Some(c) = case.as_mut() {
                    let value = get(&a, "value").map(str::to_string);
                    match get(&a, "name") {
                        Some("expected") => c.expected = value,
                        Some("actual") => c.actual = value,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn end_element(
        name: &str,
        run: &mut TestRun,
        suites: &mut Vec<Suite>,
        case: &mut Option<Case>,
        capture: &mut Capture,
        text: &mut String,
    ) {
        let body = std::mem::take(text);
        let body = body.trim_matches('\n');
        let body = (!body.trim().is_empty()).then(|| body.to_string());
        match name {
            "failure" | "error" => {
                if let Some(c) = case.as_mut() {
                    match (&c.message, body) {
                        (None, Some(body)) => {
                            // jest-junit: the first line is the message.
                            let first = body
                                .lines()
                                .find(|l| !l.trim().is_empty())
                                .unwrap_or("")
                                .trim();
                            c.message = Some(first.to_string());
                            c.details = Some(body);
                        }
                        (_, body) => c.details = body,
                    }
                }
                *capture = Capture::None;
            }
            "skipped" => {
                if let Some(c) = case.as_mut() {
                    if c.message.is_none() {
                        c.message = body;
                    }
                }
                *capture = Capture::None;
            }
            "system-out" | "system-err" => {
                let is_out = name == "system-out";
                match case.as_mut() {
                    Some(c) if is_out => c.stdout = body,
                    Some(c) => c.stderr = body,
                    None => {
                        // Suite-level output goes to cases without their own.
                        if let (Some(suite), Some(body)) = (suites.last_mut(), body) {
                            for c in &mut suite.cases {
                                let slot = if is_out { &mut c.stdout } else { &mut c.stderr };
                                if slot.is_none() && c.status != Status::Passed {
                                    *slot = Some(body.clone());
                                }
                            }
                        }
                    }
                }
                *capture = Capture::None;
            }
            "testcase" => {
                if let Some(mut c) = case.take() {
                    if c.is_failure() && (c.expected.is_none() || c.actual.is_none()) {
                        let source = format!(
                            "{}\n{}",
                            c.message.as_deref().unwrap_or(""),
                            c.details.as_deref().unwrap_or("")
                        );
                        if let Some((e, a)) = expected_actual(&source) {
                            c.expected = Some(e);
                            c.actual = Some(a);
                        }
                    }
                    match suites.last_mut() {
                        Some(suite) => suite.cases.push(c),
                        None => run.suites.push(Suite {
                            name: String::new(),
                            cases: vec![c],
                            duration: None,
                        }),
                    }
                }
            }
            "testsuite" => {
                if let Some(suite) = suites.pop() {
                    run.suites.push(suite);
                }
            }
            _ => {}
        }
    }

    loop {
        let event = reader
            .read_event()
            .map_err(|e| error(format!("at byte {}: {e}", reader.error_position())))?;
        match event {
            Event::Start(start) => {
                let name = start.local_name().as_ref().to_string();
                saw_root |= matches!(name.as_str(), "testsuites" | "testsuite");
                let a = attrs(&start)?;
                open.push(name.clone());
                start_element(
                    &name,
                    a,
                    &mut run,
                    &mut suites,
                    &mut case,
                    &mut capture,
                    &mut text,
                );
            }
            Event::Empty(start) => {
                let name = start.local_name().as_ref().to_string();
                saw_root |= matches!(name.as_str(), "testsuites" | "testsuite");
                let a = attrs(&start)?;
                start_element(
                    &name,
                    a,
                    &mut run,
                    &mut suites,
                    &mut case,
                    &mut capture,
                    &mut text,
                );
                end_element(
                    &name,
                    &mut run,
                    &mut suites,
                    &mut case,
                    &mut capture,
                    &mut text,
                );
            }
            Event::End(end) => {
                let name = end.local_name().as_ref().to_string();
                open.pop();
                end_element(
                    &name,
                    &mut run,
                    &mut suites,
                    &mut case,
                    &mut capture,
                    &mut text,
                );
            }
            Event::Text(t) => {
                if capture != Capture::None {
                    text.push_str(&t.xml10_content());
                }
            }
            Event::CData(data) => {
                if capture != Capture::None {
                    text.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                if capture != Capture::None {
                    let resolved = match reference.resolve_char_ref() {
                        Ok(Some(c)) => c.to_string(),
                        _ => quick_xml::escape::unescape(&format!("&{};", &*reference))
                            .map(|s| s.into_owned())
                            .unwrap_or_default(),
                    };
                    text.push_str(&resolved);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !saw_root {
        return Err(error("no <testsuites> or <testsuite> element"));
    }
    if let Some(innermost) = open.last() {
        return Err(error(format!(
            "truncated: the file ends inside <{innermost}> ({} element{} left open)",
            open.len(),
            if open.len() == 1 { "" } else { "s" }
        )));
    }
    run.suites.extend(suites);
    Ok(run)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

/// Element text: attribute escaping, but newlines kept literal.
fn escape_text(text: &str) -> String {
    escape(text).replace("&#10;", "\n").replace("&#9;", "\t")
}

/// Normalized JUnit XML for `run`.
pub fn write(run: &TestRun) -> String {
    let t = run.totals();
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str(&format!(
        "<testsuites tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\"",
        t.total(),
        t.failed,
        t.errored,
        t.skipped
    ));
    if let Some(d) = run.duration {
        out.push_str(&format!(" time=\"{}\"", exact_seconds(d)));
    }
    out.push_str(">\n");
    for suite in &run.suites {
        let s = suite.totals();
        out.push_str(&format!(
            "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\"",
            escape(&suite.name),
            s.total(),
            s.failed,
            s.errored,
            s.skipped
        ));
        if let Some(d) = suite.duration {
            out.push_str(&format!(" time=\"{}\"", exact_seconds(d)));
        }
        out.push_str(">\n");
        for case in &suite.cases {
            out.push_str(&format!(
                "    <testcase name=\"{}\" classname=\"{}\"",
                escape(&case.name),
                escape(&case.classname)
            ));
            if let Some(d) = case.duration {
                out.push_str(&format!(" time=\"{}\"", exact_seconds(d)));
            }
            out.push_str(">\n");
            if case.expected.is_some() || case.actual.is_some() {
                out.push_str("      <properties>\n");
                for (name, value) in [("expected", &case.expected), ("actual", &case.actual)] {
                    if let Some(value) = value {
                        out.push_str(&format!(
                            "        <property name=\"{name}\" value=\"{}\"/>\n",
                            escape(value)
                        ));
                    }
                }
                out.push_str("      </properties>\n");
            }
            let tag = match case.status {
                Status::Passed => None,
                Status::Failed => Some("failure"),
                Status::Errored => Some("error"),
                Status::Skipped => Some("skipped"),
            };
            if let Some(tag) = tag {
                out.push_str(&format!("      <{tag}"));
                if let Some(message) = &case.message {
                    out.push_str(&format!(" message=\"{}\"", escape(message)));
                }
                match &case.details {
                    Some(details) => out.push_str(&format!(">{}</{tag}>\n", escape_text(details))),
                    None => out.push_str("/>\n"),
                }
            }
            for (tag, output) in [("system-out", &case.stdout), ("system-err", &case.stderr)] {
                if let Some(output) = output {
                    out.push_str(&format!("      <{tag}>{}</{tag}>\n", escape_text(output)));
                }
            }
            out.push_str("    </testcase>\n");
        }
        out.push_str("  </testsuite>\n");
    }
    out.push_str("</testsuites>\n");
    out
}

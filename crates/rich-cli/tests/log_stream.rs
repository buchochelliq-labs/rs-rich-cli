//! `rich log`'s filters, follow mode, groups, parsers and JSON output
//! (#724), end to end through the built binary.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

fn bin() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command.env_remove("NO_COLOR").env("COLUMNS", "200");
    command
}

/// `rich --no-config log ARGS…` on `stdin`: stdout, stderr and the status.
fn log(args: &[&str], stdin: &str) -> (String, String, ExitStatus) {
    let mut child = bin()
        .args(["--no-config", "--no-color", "-w", "200", "log"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rich");
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_string();
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(stdin.as_bytes());
    });
    let output = child.wait_with_output().expect("wait rich");
    writer.join().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status,
    )
}

/// [`log`] reading `-`, expecting success: stdout.
fn ok(args: &[&str], stdin: &str) -> String {
    let mut all = args.to_vec();
    all.push("-");
    let (out, err, status) = log(&all, stdin);
    assert!(status.success(), "rich log {args:?}: {status:?}\n{err}");
    out
}

/// [`log`] expecting a usage error (exit 2): stderr.
fn usage(args: &[&str]) -> String {
    let (_, err, status) = log(args, "");
    assert_eq!(status.code(), Some(2), "rich log {args:?}: {err}");
    err
}

const APP: &str = r#"{"time":"2026-10-18T10:00:00.000Z","level":"info","msg":"request started","service":"api","trace_id":"t1","latency_ms":12}
{"time":"2026-10-18T10:00:00.250Z","level":"info","msg":"request started","service":"web","trace_id":"t2","latency_ms":3}
{"time":"2026-10-18T10:00:01.100Z","level":"warn","msg":"upstream slow","service":"api","trace_id":"t1","latency_ms":812}
{"time":"2026-10-18T10:00:01.900Z","level":"error","msg":"upstream timed out","service":"api","trace_id":"t1","latency_ms":3000}
Traceback (most recent call last):
  File "app.py", line 12, in handle
TimeoutError: upstream timed out
{"time":"2026-10-18T10:00:02.000Z","level":"info","msg":"health check","service":"web"}
{"time":"2026-10-18T10:00:03.000Z","level":"info","msg":"health check","service":"web"}
{"time":"2026-10-18T10:00:04.000Z","level":"info","msg":"health check","service":"web"}
{"time":"2026-10-18T10:00:05.000Z","level":"error","msg":"connection refused","service":"web","trace_id":"t2"}
"#;

#[test]
fn json_records_print_as_before() {
    let out = ok(
        &[],
        r#"{"timestamp":"2026-09-13T20:00:00Z","level":"info","message":"started","request_id":"abc"}
{"time":1690000000000,"level":30,"msg":"started","pid":42}
[1,2]
"#,
    );
    assert_eq!(
        out,
        "2026-09-13T20:00:00Z INFO started {\"request_id\":\"abc\"}\n\
         1690000000000 30 started {\"pid\":42}\n[1,2]\n"
    );
    let rich = ok(
        &["--log-presentation", "rich"],
        r#"{"level":"warn","message":"[red]literal[/red]","count":3}"#,
    );
    assert_eq!(rich, "warn [red]literal[/red] count=3\n");
}

#[test]
fn lines_that_are_not_json_are_parsed_or_shown_never_dropped() {
    let out = ok(
        &[],
        "booting, before any record\n\
         time=2026-10-18T10:00:00Z level=info msg=\"cache warm\" keys=12\n\
         2026-10-18T10:00:01Z WARN app::db: pool low size=2\n\
         127.0.0.1 - - [18/Oct/2026:10:00:02 +0000] \"GET /missing HTTP/1.1\" 404 0 \"-\" \"curl\"\n\
         ERROR request failed\n\
         \x20 at handler (app.js:10)\n\
         \n\
         not parsed, after an error: it joins it\n\
         \x1b[32mINFO\x1b[0m coloured\n\
         \u{fffd}\u{0} odd bytes\n",
    );
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "booting, before any record");
    assert_eq!(
        lines[1],
        r#"2026-10-18T10:00:00Z INFO cache warm {"keys":"12"}"#
    );
    assert_eq!(
        lines[2],
        r#"2026-10-18T10:00:01Z WARN pool low {"target":"app::db","size":"2"}"#
    );
    assert!(
        lines[3].starts_with("18/Oct/2026:10:00:02 +0000 WARN GET /missing 404 {\"remote_addr\""),
        "{}",
        lines[3]
    );
    assert_eq!(lines[4], "ERROR request failed");
    assert_eq!(lines[5], "    at handler (app.js:10)");
    assert_eq!(lines[6], "  not parsed, after an error: it joins it");
    assert_eq!(lines[7], "INFO coloured");
    assert!(lines[8].contains("odd bytes"), "{out}");
    assert_eq!(lines.len(), 9, "{out}");
}

#[test]
fn invalid_utf8_and_broken_json_never_fail() {
    let mut child = bin()
        .args(["--no-config", "log", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let mut bytes = b"{\"level\":\n\xff\xfe broken \xc3\n{\"a\":}\n".to_vec();
        bytes.extend(std::iter::repeat_n(b'x', 200_000));
        bytes.extend(b"\n\x00\x01\x02\n");
        let _ = input.write_all(&bytes);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(out.contains("{\"level\":"), "{out}");
    assert!(out.contains("broken"), "{out}");
    assert!(out.contains("{\"a\":}"), "{out}");
}

#[test]
fn level_filters_keep_a_level_and_those_above() {
    let levels = |out: &str| -> Vec<String> {
        out.lines()
            .filter_map(|line| line.split(' ').nth(1).map(str::to_string))
            .filter(|word| word.chars().all(|c| c.is_ascii_uppercase()) && !word.is_empty())
            .collect()
    };
    assert_eq!(
        levels(&ok(&["--level", "warn+"], APP)),
        ["WARN", "ERROR", "ERROR"]
    );
    assert_eq!(levels(&ok(&["--level", "error"], APP)), ["ERROR", "ERROR"]);
    assert_eq!(
        levels(&ok(&["--level", "warn", "--level", "error"], APP)),
        ["WARN", "ERROR", "ERROR"]
    );
    assert_eq!(levels(&ok(&["--level", "info,error"], APP)).len(), 7);
    // A record's continuation lines go with it.
    let errors = ok(&["--level", "error"], APP);
    assert!(
        errors.contains("  TimeoutError: upstream timed out"),
        "{errors}"
    );
    assert!(usage(&["--level", "loud", "-"]).contains("unknown level \"loud\""));
    assert!(usage(&["--level"]).contains("--level requires a value"));
}

#[test]
fn where_predicates_test_fields_and_are_anded() {
    let messages = |args: &[&str]| -> Vec<String> {
        ok(args, APP)
            .lines()
            .filter(|line| line.starts_with("2026"))
            .map(|line| {
                line.split(' ')
                    .skip(2)
                    .take(2)
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    };
    assert_eq!(
        messages(&["--where", "latency_ms>500"]),
        ["upstream slow", "upstream timed"]
    );
    assert_eq!(
        messages(&["--where", "service=api", "--where", "latency_ms<=812"]),
        ["request started", "upstream slow"]
    );
    assert_eq!(
        messages(&["--where", "msg~timed|refused"]),
        ["upstream timed", "connection refused"]
    );
    assert_eq!(messages(&["--where", "!trace_id"]).len(), 3);
    assert_eq!(
        messages(&["--where", "trace_id=t2", "--where", "level>=warn"]).len(),
        1
    );
    assert_eq!(messages(&["--where", "service!=api"]).len(), 5);
    assert_eq!(
        messages(&["--where", "time>=2026-10-18T10:00:04Z"]),
        ["health check", "connection refused"]
    );
    assert!(usage(&["--where", "=api", "-"]).contains("no field name"));
    assert!(usage(&["--where", "msg~(", "-"]).contains("invalid pattern"));
}

#[test]
fn repeats_collapse_on_request_and_by_default_only_when_following() {
    let plain = ok(&[], APP);
    assert_eq!(plain.matches("health check").count(), 3);
    let folded = ok(&["--collapse"], APP);
    assert_eq!(folded.matches("health check").count(), 1, "{folded}");
    assert!(
        folded.contains("  × 3 (last 2026-10-18T10:00:04.000Z)"),
        "{folded}"
    );
    let untimed = ok(&["--collapse", "--time", "none"], APP);
    assert!(untimed.contains("\n  × 3\n"), "{untimed}");
    let kept = ok(&["--no-collapse"], APP);
    assert_eq!(kept, plain);
}

#[test]
fn groups_gather_records_under_headers_with_a_summary() {
    let out = ok(&["--group", "trace"], APP);
    let expected_start = "▸ trace t1 · 3 records · 1 error · 1 warning · 1.900s\n\
         \x20 2026-10-18T10:00:00.000Z INFO request started";
    assert!(out.starts_with(expected_start), "{out}");
    assert!(
        out.contains("\n▸ trace t2 · 2 records · 1 error · 4.750s\n"),
        "{out}"
    );
    assert!(
        out.contains("\n▸ no trace id · 3 records · 2.000s\n"),
        "{out}"
    );
    assert!(
        out.contains("\n    TimeoutError: upstream timed out\n"),
        "{out}"
    );
    assert!(
        out.ends_with(
            "\nSummary: 8 records\n  error 2 · warn 1 · info 5\n  2 traces with errors: t1 \
             (1 error), t2 (1 error)\n"
        ),
        "{out}"
    );
    let quiet = ok(&["--group", "trace", "--no-summary"], APP);
    assert!(!quiet.contains("Summary"), "{quiet}");
    let requests = ok(
        &["--group", "correlation"],
        "{\"msg\":\"a\",\"request_id\":\"r1\"}\n{\"msg\":\"b\",\"correlationId\":\"r2\"}\n",
    );
    assert!(requests.contains("▸ correlation r1"), "{requests}");
    assert!(requests.contains("▸ correlation r2"), "{requests}");
    let summary = ok(&["--summary", "--level", "warn+"], APP);
    assert!(
        summary.ends_with("\nSummary: 3 of 8 records shown\n  error 2 · warn 1\n"),
        "{summary}"
    );
    assert!(usage(&["--group", "host", "-"]).contains("unknown grouping"));
    assert!(usage(&["--group", "4", "-"]).contains("trace, span or correlation"));
}

#[test]
fn fields_and_times_change_how_a_record_shows() {
    let first = APP.lines().next().unwrap();
    let fields = ok(&["--fields"], first);
    assert_eq!(
        fields,
        "2026-10-18T10:00:00.000Z INFO request started\n    service: \"api\"\n    trace_id: \
         \"t1\"\n    latency_ms: 12\n"
    );
    let relative = ok(&["--time", "relative"], APP);
    let starts: Vec<&str> = relative
        .lines()
        .filter_map(|line| line.split(' ').next())
        .filter(|word| word.starts_with('+'))
        .collect();
    assert_eq!(
        starts,
        ["+0.000s", "+0.250s", "+1.100s", "+1.900s", "+2.000s", "+3.000s", "+4.000s", "+5.000s"]
    );
    let none = ok(&["--time", "none"], first);
    assert_eq!(
        none,
        "INFO request started {\"service\":\"api\",\"trace_id\":\"t1\",\"latency_ms\":12}\n"
    );
    let rich = ok(&["--log-presentation", "rich", "--time", "relative"], first);
    assert!(rich.starts_with("+0.000s info request started"), "{rich}");
}

#[test]
fn a_parser_can_be_chosen() {
    let input = "level=info msg=hi n=1\nWARN low disk\n";
    let auto = ok(&[], input);
    assert_eq!(auto, "INFO hi {\"n\":\"1\"}\nWARN low disk\n");
    let logfmt = ok(&["--parser", "logfmt"], input);
    assert_eq!(logfmt, "INFO hi {\"n\":\"1\"}\n  WARN low disk\n");
    let text = ok(&["--parser", "text"], input);
    assert_eq!(text, "level=info msg=hi n=1\nWARN low disk\n");
    assert!(usage(&["--parser", "xml", "-"]).contains("unknown --parser"));
}

#[test]
fn jsonl_output_writes_records_that_read_back_the_same() {
    let jsonl = ok(&["--output", "jsonl"], APP);
    let records: Vec<serde_json::Value> = jsonl
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{line}: {e}")))
        .collect();
    assert_eq!(records.len(), 8);
    assert_eq!(
        records[3]["continuation"][2],
        "TimeoutError: upstream timed out"
    );
    // Written again, nothing changes.
    assert_eq!(ok(&["--output", "jsonl"], &jsonl), jsonl);
    // Read back, every record shows as it did, continuation lines and all.
    assert_eq!(ok(&[], &jsonl), ok(&[], APP));
    // Text lines become objects; filters apply; the summary goes to stderr.
    let (out, err, status) = log(
        &["--output", "jsonl", "--level", "error", "--summary", "-"],
        "ERROR boom\nINFO fine\n",
    );
    assert!(status.success());
    assert_eq!(out, "{\"level\":\"ERROR\",\"message\":\"boom\"}\n");
    assert!(err.contains("Summary: 1 of 2 records shown"), "{err}");
    assert!(usage(&["--output", "jsonl", "--fields", "-"]).contains("--output jsonl"));
}

#[test]
fn continuation_after_a_quiet_spell_reads_back_under_its_record() {
    // The record is written when stdin goes quiet; a line under it that
    // comes later is an object of its own, folded back on reading.
    let mut child = bin()
        .args(["--no-config", "--no-color", "log", "--output", "jsonl", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        input
            .write_all(b"{\"level\":\"error\",\"msg\":\"boom\"}\n  at one\n")
            .unwrap();
        input.flush().unwrap();
        std::thread::sleep(Duration::from_millis(800));
        input
            .write_all(b"  at two\n{\"level\":\"info\",\"msg\":\"next\"}\n")
            .unwrap();
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    let jsonl = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        jsonl,
        "{\"level\":\"error\",\"msg\":\"boom\",\"continuation\":[\"  at one\"]}\n\
         {\"continuation\":[\"  at two\"]}\n\
         {\"level\":\"info\",\"msg\":\"next\"}\n"
    );
    // Read back: one record with both lines, the way it was read.
    let again = ok(&["--output", "jsonl"], &jsonl);
    assert_eq!(
        again,
        "{\"level\":\"error\",\"msg\":\"boom\",\"continuation\":[\"  at one\",\"  at two\"]}\n\
         {\"level\":\"info\",\"msg\":\"next\"}\n"
    );
    let shown = ok(&[], &jsonl);
    assert!(!shown.contains("continuation"), "{shown}");
    assert!(
        shown.contains("at one\n") && shown.contains("at two\n"),
        "{shown}"
    );
    // A continuation object with no record before it: its lines are read
    // as lines.
    let out = ok(&["--output", "jsonl"], "{\"continuation\":[\"orphan\"]}\n");
    assert_eq!(out, "{\"message\":\"orphan\"}\n");
}

#[test]
fn untrusted_controls_are_shown_not_sent_by_default() {
    let input = "{\"msg\":\"hi\",\"note\":\"\\u001b]52;c;aGk=\\u0007\",\"\\u001b[2Jkey\":1,\"nested\":{\"\\u001b]0;title\\u0007\":2}}\n  \u{1b}]52;c;aGk=\u{7}\n";
    for output in [&[][..], &["--output", "jsonl"], &["--fields"]] {
        let out = ok(output, input);
        assert!(!out.contains('\u{1b}'), "{output:?}: {out:?}");
        assert!(!out.contains('\u{7}'), "{output:?}: {out:?}");
        assert!(out.contains("52;c;aGk="), "{output:?}: {out:?}");
    }
    // `--no-sanitize` lets them through.
    let out = ok(&["--no-sanitize"], input);
    assert!(out.contains("\u{1b}]52;c;aGk="), "{out:?}");
}

#[test]
fn redaction_masks_secrets_in_fields_messages_and_lines() {
    let input = "{\"msg\":\"login token=ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\",\"password\":\"hunter 2\",\"user\":\"ann\"}\n  Authorization: Bearer abcdef0123456789xyz\n";
    let out = ok(&["--redact"], input);
    assert_eq!(
        out,
        "login token=******** {\"password\":\"********\",\"user\":\"ann\"}\n    \
         Authorization: Bearer ********\n"
    );
    let jsonl = ok(&["--redact", "--output", "jsonl"], input);
    assert!(jsonl.contains("\"password\":\"********\""), "{jsonl}");
    let pattern = ok(&["--redact-pattern", "ann"], input);
    assert!(pattern.contains("\"user\":\"********\""), "{pattern}");
    assert!(pattern.contains("hunter 2"), "{pattern}");
}

#[test]
fn options_only_apply_to_rich_log() {
    let (_, err, status) = bin()
        .args(["--no-config", "--level", "error", "notes.txt"])
        .output()
        .map(|o| {
            (
                String::new(),
                String::from_utf8_lossy(&o.stderr).into_owned(),
                o.status,
            )
        })
        .unwrap();
    assert_eq!(status.code(), Some(2));
    assert!(
        err.contains("--level only has an effect with `rich log`"),
        "{err}"
    );
    let hex = bin()
        .args(["--no-config", "hex", "--group", "trace", "x"])
        .output()
        .unwrap();
    assert_eq!(hex.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&hex.stderr).contains("only has an effect with `rich log`"));
    assert!(usage(&["--follow"]).contains("--follow needs a FILE"));
    assert!(usage(&["-f", "-"]).contains("--follow needs a FILE"));
    assert!(usage(&["-i", "--group", "trace", "-"]).contains("--interactive"));
}

#[cfg(feature = "interact")]
#[test]
fn interactive_without_a_terminal_streams_as_usual() {
    let out = ok(&["-i"], APP);
    assert_eq!(out.matches("health check").count(), 1, "{out}");
    assert!(out.contains("× 3"), "{out}");
    let plain = ok(&["-i", "--no-collapse"], APP);
    assert_eq!(plain, ok(&[], APP));
}

/// Lines of a running `rich log`, read on a thread.
struct Running {
    child: std::process::Child,
    lines: mpsc::Receiver<String>,
    seen: Vec<String>,
}

impl Running {
    fn start(args: &[&str], file: &Path) -> Running {
        let mut child = bin()
            .args(["--no-config", "--no-color", "-w", "200", "log"])
            .args(args)
            .arg(file)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn rich");
        let (sender, lines) = mpsc::channel();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        for stream in [
            Box::new(stdout) as Box<dyn std::io::Read + Send>,
            Box::new(stderr),
        ] {
            let sender = sender.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        Running {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    /// Wait, at most 20 s, for a line that holds `text`.
    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.seen.iter().any(|line| line.contains(text)) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.lines.recv_timeout(left) {
                Ok(line) => self.seen.push(line),
                Err(_) => {
                    let _ = self.child.kill();
                    panic!("no line with {text:?} in time; saw {:#?}", self.seen);
                }
            }
        }
    }

    fn stop(mut self) -> (Option<i32>, Vec<String>) {
        #[cfg(unix)]
        {
            // Ctrl+C: the summary is written and the exit is 130.
            unsafe_free_interrupt(self.child.id());
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                if let Some(status) = self.child.try_wait().unwrap() {
                    std::thread::sleep(Duration::from_millis(100));
                    self.seen.extend(self.lines.try_iter());
                    return (status.code(), self.seen);
                }
                if Instant::now() > deadline {
                    let _ = self.child.kill();
                    panic!("rich log --follow did not stop on SIGINT");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        #[cfg(not(unix))]
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
            (None, self.seen)
        }
    }
}

/// Send SIGINT to `pid` with the `kill` program, which every Unix has.
#[cfg(unix)]
fn unsafe_free_interrupt(pid: u32) {
    let status = Command::new("kill")
        .args(["-INT", &pid.to_string()])
        .status()
        .expect("run kill");
    assert!(status.success());
}

fn append(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

#[test]
fn follow_reads_appended_lines_through_rotation_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.log");
    std::fs::write(&path, "INFO first\n").unwrap();
    let mut running = Running::start(&["--follow", "--summary"], &path);
    running.wait_for("INFO first");
    append(&path, "WARN appended\nERROR partial ");
    running.wait_for("WARN appended");
    append(&path, "line finished\n");
    running.wait_for("ERROR partial line finished");
    // Rotation: renamed away and a new file in its place.
    append(&path, "INFO last of the old file\n");
    std::fs::rename(&path, dir.path().join("app.log.1")).unwrap();
    std::fs::write(&path, "INFO in the new file\n").unwrap();
    running.wait_for("INFO in the new file");
    running.wait_for("was replaced; following the new file");
    // Truncation, as copytruncate rotates.
    std::fs::write(&path, "").unwrap();
    // A longer file written before the truncation is seen would read as an
    // append, so wait for it.
    running.wait_for("was truncated; reading it from the start");
    std::fs::write(&path, "INFO after truncation\n").unwrap();
    running.wait_for("INFO after truncation");
    // Repeats fold while following; piped, the count is written when the
    // run ends.
    append(&path, "WARN again\nWARN again\nWARN again\nINFO done\n");
    running.wait_for("INFO done");
    running.wait_for("× 3");
    let (code, seen) = running.stop();
    assert!(
        seen.iter().any(|line| line == "INFO last of the old file"),
        "{seen:#?}"
    );
    assert_eq!(
        seen.iter().filter(|line| *line == "WARN again").count(),
        1,
        "{seen:#?}"
    );
    if cfg!(unix) {
        assert_eq!(code, Some(130), "{seen:#?}");
        assert!(
            seen.iter()
                .any(|line| line == "Summary: 10 records, 2 repeats folded"),
            "{seen:#?}"
        );
    }
}

#[test]
fn follow_with_groups_writes_a_header_when_the_group_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("app.jsonl");
    std::fs::write(
        &path,
        "{\"msg\":\"a\",\"trace_id\":\"t1\"}\n{\"msg\":\"b\",\"trace_id\":\"t1\"}\n",
    )
    .unwrap();
    let mut running = Running::start(&["-f", "--group", "trace"], &path);
    running.wait_for("  b {\"trace_id\":\"t1\"}");
    append(&path, "{\"msg\":\"c\",\"trace_id\":\"t2\"}\n");
    running.wait_for("▸ trace t2");
    let (_, seen) = running.stop();
    assert_eq!(
        seen.iter()
            .filter(|line| line.starts_with("▸ trace t1"))
            .count(),
        1
    );
}

/// 100,000 records through the static path: well under a second in a
/// release build (`cargo test --release -p rs-rich-cli --test log_stream --
/// --ignored`).
#[test]
#[ignore]
fn a_hundred_thousand_records_stream_quickly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.jsonl");
    let mut text = String::new();
    for n in 0..100_000 {
        text.push_str(&format!(
            "{{\"time\":\"2026-10-18T10:00:00.{:03}Z\",\"level\":\"{}\",\"msg\":\"request {}\",\"service\":\"api\",\"latency_ms\":{}}}\n",
            n % 1000,
            ["info", "warn", "error"][n % 3],
            n % 50,
            n % 997
        ));
    }
    std::fs::write(&path, text).unwrap();
    for args in [
        &[][..],
        &["--level", "warn+", "--where", "latency_ms>100"][..],
    ] {
        let start = Instant::now();
        let output = bin()
            .args(["--no-config", "log"])
            .args(args)
            .arg(&path)
            .output()
            .unwrap();
        let took = start.elapsed();
        assert!(output.status.success());
        eprintln!("{args:?}: {took:?}");
        assert!(took < Duration::from_secs(1), "{args:?} took {took:?}");
    }
}

/// The live view in a real terminal: `rich log -i` run by `sh` in a PTY.
#[cfg(all(unix, feature = "interact"))]
mod live {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    struct Pty {
        output: Arc<Mutex<Vec<u8>>>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn portable_pty::Child + Send + Sync>,
    }

    impl Pty {
        /// Run `script` in `sh`, `rich` meaning the built binary.
        fn start(script: &str) -> Pty {
            let pty = native_pty_system()
                .openpty(PtySize {
                    rows: 24,
                    cols: 100,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let rich = format!("'{}' --no-config", env!("CARGO_BIN_EXE_rich"));
            let script = format!(
                "{}; stty -a; echo STTY-DONE",
                script.replace("rich ", &format!("{rich} "))
            );
            let mut command = CommandBuilder::new("sh");
            command.args(["-c", &script]);
            command.env("TERM", "xterm-256color");
            command.env_remove("CI");
            command.env_remove("NO_COLOR");
            let child = pty.slave.spawn_command(command).unwrap();
            drop(pty.slave);
            let mut reader = pty.master.try_clone_reader().unwrap();
            let output = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&output);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while let Ok(read) = reader.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    sink.lock().unwrap().extend_from_slice(&buffer[..read]);
                }
            });
            let writer = pty.master.take_writer().unwrap();
            std::mem::forget(pty.master);
            Pty {
                output,
                writer,
                child,
            }
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
        }

        fn screen(&self) -> String {
            let mut parser = vt100::Parser::new(24, 100, 0);
            parser.process(&self.output.lock().unwrap());
            parser.screen().contents()
        }

        fn wait_for(&self, needle: &str) {
            let end = Instant::now() + Duration::from_secs(30);
            while Instant::now() < end {
                if self.screen().contains(needle) || self.text().contains(needle) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!(
                "timed out waiting for {needle:?}; screen:\n{}",
                self.screen()
            );
        }

        fn send(&mut self, keys: &str) {
            self.writer.write_all(keys.as_bytes()).unwrap();
            self.writer.flush().unwrap();
        }

        /// The whole output, once the script is done; the terminal must be
        /// back in cooked mode.
        fn finish(mut self) -> String {
            self.wait_for("STTY-DONE");
            let _ = self.child.wait();
            let text = self.text();
            let stty = &text[text.rfind("speed").expect("stty output")..];
            let flags: Vec<&str> = stty.split_whitespace().collect();
            for flag in ["icanon", "echo", "isig"] {
                assert!(flags.contains(&flag), "{flag:?} not restored:\n{stty}");
            }
            text
        }
    }

    #[test]
    fn the_live_view_follows_filters_expands_and_pauses() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        std::fs::write(
            &path,
            "level=info msg=\"request started\" service=api\n\
             level=error msg=\"upstream timed out\" service=api trace_id=t1\n",
        )
        .unwrap();
        let mut pty = Pty::start(&format!(
            "rich log -i -f '{}'; echo \"code=$?\"",
            path.display()
        ));
        pty.wait_for("upstream timed out");
        pty.wait_for("following");
        // Appended while following: it shows.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"level=warn msg=\"disk low\" service=db\n")
            .unwrap();
        drop(file);
        pty.wait_for("disk low");
        // The filter line keeps what matches.
        pty.send("/");
        pty.send("service=api upstream");
        pty.wait_for("1 matching");
        pty.send("\r");
        // Enter shows the record's fields.
        pty.send("\r");
        pty.wait_for("trace_id: t1");
        // Space pauses.
        pty.send(" ");
        pty.wait_for("paused");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
    }
}

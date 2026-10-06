//! `rich profile` (0.0.16 workstream 3): a profile of CSV, TSV or JSON Lines
//! from a file or stdin, `--report json`, sampling, and the errors.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const ORDERS: &str = include_str!("../../rich-data/tests/fixtures/orders.csv");
const EVENTS: &str = include_str!("../../rich-data/tests/fixtures/events.jsonl");
/// The library's snapshots: the command draws exactly what `Profile` does.
const ORDERS_SNAPSHOT: &str = include_str!("../../rich-data/tests/snapshots/profile_orders.txt");
const EVENTS_SNAPSHOT: &str = include_str!("../../rich-data/tests/snapshots/profile_events.txt");

/// Run `rich` in a fresh directory holding `files`, with `stdin` piped in,
/// plain output 80 columns wide.
fn run_with(files: &[(&str, &str)], stdin: &str, args: &[&str]) -> Output {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(home.join(".config/rich")).unwrap();
    for (name, content) in files {
        std::fs::write(work.join(name), content).unwrap();
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(&work)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "80")
        .env("TERM", "xterm-256color")
        .env_remove("COLORTERM")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let stdin = stdin.to_string();
    // A large input is written from a thread, so the child can stream it.
    let writer = std::thread::spawn(move || {
        let _ = pipe.write_all(stdin.as_bytes());
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    output
}

fn run(args: &[&str]) -> Output {
    run_with(
        &[("orders.csv", ORDERS), ("events.jsonl", EVENTS)],
        "",
        args,
    )
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn failure(out: &Output) -> (Option<i32>, String) {
    assert!(out.stdout.is_empty(), "drew something on failure");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_csv_file_is_profiled_as_the_library_draws_it() {
    assert_eq!(stdout(&run(&["profile", "orders.csv"])), ORDERS_SNAPSHOT);
}

#[test]
fn a_jsonl_file_is_profiled_as_the_library_draws_it() {
    assert_eq!(stdout(&run(&["profile", "events.jsonl"])), EVENTS_SNAPSHOT);
}

#[test]
fn stdin_is_read_as_csv_or_by_its_first_character_as_json_lines() {
    let out = stdout(&run_with(&[], ORDERS, &["profile"]));
    assert!(out.starts_with("<stdin>: 24 rows, 6 columns\n"), "{out}");
    let out = stdout(&run_with(&[], EVENTS, &["profile", "-"]));
    assert!(out.starts_with("<stdin>: 12 rows, 5 columns\n"), "{out}");
    assert!(out.contains("ts · timestamp"), "{out}");
    // A header is kept even when the sniffer would doubt it.
    let out = stdout(&run_with(&[], "service,p99\nweb,120\ndb,\n", &["profile"]));
    assert!(out.contains("│ p99     │ integer │"), "{out}");
}

#[test]
fn columns_and_top_choose_what_is_shown() {
    let out = stdout(&run(&[
        "profile",
        "orders.csv",
        "--columns",
        "status,amount",
        "--top",
        "2",
    ]));
    assert!(out.starts_with("orders.csv: 24 rows, 2 columns\n"), "{out}");
    let status = out.find("status · text").unwrap();
    assert!(status < out.find("amount · float").unwrap(), "{out}");
    assert!(out.contains("  + 2 other values (4 cells)\n"), "{out}");
    assert!(!out.contains("region"), "{out}");
}

#[test]
fn a_large_input_is_sampled_and_the_sample_size_printed() {
    let mut csv = String::from("id,group,value\n");
    for i in 0..30_000u32 {
        let value = if i % 5 == 0 {
            String::new()
        } else {
            (i % 97).to_string()
        };
        csv.push_str(&format!("{i},g{},{value}\n", i % 3));
    }
    let out = stdout(&run_with(&[], &csv, &["profile", "--sample", "500"]));
    assert!(
        out.starts_with("<stdin>: sampled 500 of 30,000 rows, 3 columns\n"),
        "{out}"
    );
    // Nulls count every row.
    assert!(out.contains("value · integer · 6,000 nulls (20%)"), "{out}");
    let json = stdout(&run_with(
        &[("big.csv", &csv)],
        "",
        &["profile", "big.csv", "--sample", "500", "--report", "json"],
    ));
    let json: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(json["rows"], 30_000);
    assert_eq!(json["sample"], 500);
    assert_eq!(json["sampled"], true);
}

#[test]
fn report_json_writes_the_profile_to_stdout() {
    let out = run(&["profile", "orders.csv", "--report", "json"]);
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["name"], "orders.csv");
    assert_eq!(json["rows"], 24);
    assert_eq!(json["sampled"], false);
    let amount = &json["columns"][2];
    assert_eq!(amount["name"], "amount");
    assert_eq!(amount["type"], "float");
    assert_eq!(amount["nulls"], 4);
    assert_eq!(amount["distribution"]["kind"], "histogram");
    assert_eq!(json["columns"][1]["distribution"]["kind"], "top");
    assert_eq!(json["missing"]["bucket_rows"], 2);
    // A failure is the usual envelope on stderr.
    let out = run(&[
        "profile",
        "orders.csv",
        "--report",
        "json",
        "--columns",
        "nope",
    ]);
    let (code, stderr) = failure(&out);
    assert_eq!(code, Some(4));
    let envelope: serde_json::Value = serde_json::from_str(stderr.trim()).unwrap();
    assert_eq!(envelope["code"], "data");
}

#[test]
fn ragged_rows_are_noted_not_fatal() {
    let mut csv = String::from("a,b\n1\n");
    csv.push_str(&"5,6\n".repeat(300));
    csv.push_str("7,8,9\n");
    let out = stdout(&run_with(&[("r.csv", &csv)], "", &["profile", "r.csv"]));
    assert!(
        out.contains("1 row has more cells than the header names; the extra cells are not"),
        "{out}"
    );
    assert!(out.contains("b · integer · 1 null"), "{out}");
}

#[test]
fn bad_input_and_options_are_refused() {
    let (code, stderr) = failure(&run(&["profile", "orders.csv", "--columns", "nope"]));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("orders.csv: no column \"nope\"; the columns are order_id, region"),
        "{stderr}"
    );
    let (code, stderr) = failure(&run(&["profile", "missing.csv"]));
    assert_eq!(code, Some(3));
    assert!(stderr.contains("cannot read missing.csv"), "{stderr}");
    let (code, stderr) = failure(&run_with(&[], "", &["profile"]));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("<stdin>: no rows to profile"), "{stderr}");
    let (code, stderr) = failure(&run_with(
        &[("bad.jsonl", "{\"a\": 1}\n{\"a\": \n")],
        "",
        &["profile", "bad.jsonl"],
    ));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("bad.jsonl: line 2: not JSON"), "{stderr}");
    let (code, stderr) = failure(&run(&["profile", "orders.csv", "--sample", "0"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--sample requires a positive integer"),
        "{stderr}"
    );
    let (code, stderr) = failure(&run(&["orders.csv", "--top", "3"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--top only has an effect with `rich profile`"),
        "{stderr}"
    );
}

#[test]
fn colour_goes_through_the_profile_theme() {
    let out = stdout(&run(&["profile", "orders.csv", "--force-terminal"]));
    // Under the bold heading, `profile.type` is cyan and `profile.null` yellow.
    assert!(out.contains("\x1b[1;36minteger"), "{out:?}");
    assert!(out.contains("\x1b[1;33m4 nulls (17%)"), "{out:?}");
}

#[test]
fn profile_is_in_help() {
    let out = stdout(&run(&["profile", "--help"]));
    for option in ["--sample", "--columns", "--top", "--report"] {
        assert!(out.contains(option), "{option}: {out}");
    }
    let root = stdout(&run(&["--help"]));
    assert!(root.contains("profile"), "{root}");
}

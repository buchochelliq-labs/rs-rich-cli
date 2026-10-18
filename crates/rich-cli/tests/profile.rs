//! `rich profile` (0.0.16 workstream 3): a profile of CSV, TSV or JSON Lines
//! from a file or stdin, `--report json`, sampling, and the errors.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const ORDERS: &str = include_str!("../../rich-data/tests/fixtures/orders.csv");
const EVENTS: &str = include_str!("../../rich-data/tests/fixtures/events.jsonl");
/// The library's snapshots: the command draws exactly what `Profile` does.
const ORDERS_SNAPSHOT: &str = include_str!("../../rich-data/tests/snapshots/profile_orders.txt");
const EVENTS_SNAPSHOT: &str = include_str!("../../rich-data/tests/snapshots/profile_events.txt");
const REQUESTS: &str = include_str!("../../rich-data/tests/fixtures/requests.jsonl");
const REQUESTS_SNAPSHOT: &str =
    include_str!("../../rich-data/tests/snapshots/profile_requests.txt");

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
    // `--flat`: rows of the top-level keys, as before 0.0.19.
    assert_eq!(
        stdout(&run(&["profile", "events.jsonl", "--flat"])),
        EVENTS_SNAPSHOT
    );
    // By default, path by path.
    assert_eq!(
        stdout(&run_with(
            &[("requests.jsonl", REQUESTS)],
            "",
            &["profile", "requests.jsonl"]
        )),
        REQUESTS_SNAPSHOT
    );
}

#[test]
fn stdin_is_read_as_csv_or_by_its_first_character_as_json_lines() {
    let out = stdout(&run_with(&[], ORDERS, &["profile"]));
    assert!(out.starts_with("<stdin>: 24 rows, 6 columns\n"), "{out}");
    let out = stdout(&run_with(&[], EVENTS, &["profile", "-"]));
    assert!(out.starts_with("<stdin>: 12 records, 6 paths\n"), "{out}");
    assert!(out.contains("ts · timestamp"), "{out}");
    assert!(out.contains("user.id · integer"), "{out}");
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
    // `--flat` reads rows, and a line that is not JSON stops it.
    let (code, stderr) = failure(&run_with(
        &[("bad.jsonl", "{\"a\": 1}\n{\"a\": \n")],
        "",
        &["profile", "bad.jsonl", "--flat"],
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
fn keys_first_seen_after_the_sample_are_noted_up_to_a_bound() {
    // Every record past the first 1,000 has a key of its own: the note
    // names the first 100 and ends `…`, not all 2,000 of them.
    let input: String = (0..3_000)
        .map(|i| {
            if i < 1_000 {
                format!("{{\"id\": {i}}}\n")
            } else {
                format!("{{\"id\": {i}, \"k{i}\": 1}}\n")
            }
        })
        .collect();
    let out = stdout(&run_with(
        &[],
        &input,
        &["profile", "--flat", "--report", "json"],
    ));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    let note = json["notes"][0].as_str().unwrap();
    assert!(
        note.starts_with("keys first seen after record 1000 are not profiled: k1000, "),
        "{note}"
    );
    assert!(note.ends_with(", k1099, …"), "{note}");
    assert_eq!(note.matches(", ").count(), 100, "{note}");
}

#[test]
fn json_lines_after_a_long_run_of_blank_lines_is_still_json_lines() {
    // More whitespace than one read buffer holds (8 KiB) before the first
    // record: the format is decided by the first byte that is not.
    let input = format!("{}{EVENTS}", "\n".repeat(20_000));
    let out = stdout(&run_with(&[], &input, &["profile"]));
    assert!(out.starts_with("<stdin>: 12 records, 6 paths\n"), "{out}");
    let out = stdout(&run_with(&[("events", &input)], "", &["profile", "events"]));
    assert!(out.starts_with("events: 12 records, 6 paths\n"), "{out}");
    assert!(out.contains("ts · timestamp"), "{out}");
    // Whitespace before CSV reaches the reader unchanged: blank lines give
    // no rows (the sniffer, like `--csv`'s, reads the first 1,024
    // characters, so they must leave it some data).
    let csv = format!("{}{ORDERS}", "\n".repeat(200));
    let out = stdout(&run_with(&[], &csv, &["profile"]));
    assert!(out.starts_with("<stdin>: 24 rows, 6 columns\n"), "{out}");
    let out = stdout(&run_with(&[], ORDERS, &["profile"]));
    let blank = stdout(&run_with(&[], &csv, &["profile"]));
    assert_eq!(out, blank);
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
    for option in [
        "--sample",
        "--columns",
        "--top",
        "--depth",
        "--max-paths",
        "--flat",
        "--style-rule",
        "--rules",
        "--report",
    ] {
        assert!(out.contains(option), "{option}: {out}");
    }
    let root = stdout(&run(&["--help"]));
    assert!(root.contains("profile"), "{root}");
}

fn requests(args: &[&str]) -> Output {
    let mut all = vec!["profile", "requests.jsonl"];
    all.extend(args);
    run_with(&[("requests.jsonl", REQUESTS)], "", &all)
}

/// #705: JSON Lines path by path, with presence, type mixes and the
/// malformed line counted; `--report json` carries all of it.
#[test]
fn nested_json_lines_report_presence_types_and_paths() {
    let out = stdout(&requests(&["--report", "json"]));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["rows"], 12);
    assert_eq!(json["malformed"], 1);
    assert_eq!(json["malformed_lines"], serde_json::json!([13]));
    assert_eq!(json["paths"]["depth"], 4);
    assert_eq!(json["paths"]["max_paths"], 200);
    let column = |name: &str| {
        json["columns"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("no {name}"))
            .clone()
    };
    let plan = column("user.plan");
    assert_eq!(
        (
            plan["present"].clone(),
            plan["absent"].clone(),
            plan["nulls"].clone()
        ),
        (9.into(), 3.into(), 1.into())
    );
    assert_eq!(plan["types"], serde_json::json!({"string": 8, "null": 1}));
    let status = column("[\"http.status\"]");
    assert_eq!(status["mixed"], true);
    assert_eq!(
        status["types"],
        serde_json::json!({"string": 2, "number": 9})
    );
    assert_eq!(column("items[].qty")["values"], 4);
    // The table: the mix, `!` for a mixed path, and the absent records.
    let out = stdout(&requests(&["--width", "100"]));
    assert!(
        out.starts_with("requests.jsonl: 12 records, 17 paths\n"),
        "{out}"
    );
    assert!(
        out.contains("│ [\"http.status\"] │ ! num 82% · str 18% │   1 (8%) │"),
        "{out}"
    );
    assert!(
        out.contains("1 line is not JSON and was skipped: line 13"),
        "{out}"
    );
    assert!(
        out.contains("user.plan · text · 3 absent (25%) · 1 null (11%)"),
        "{out}"
    );
}

#[test]
fn depth_max_paths_columns_and_flat_shape_the_paths() {
    let json = |args: &[&str]| -> serde_json::Value {
        let mut all = args.to_vec();
        all.extend(["--report", "json"]);
        serde_json::from_str(&stdout(&requests(&all))).unwrap()
    };
    let names = |json: &serde_json::Value| -> Vec<String> {
        json["columns"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["name"].as_str().unwrap().to_string())
            .collect()
    };
    // One step: the top-level keys, an object as its JSON text.
    let shallow = json(&["--depth", "1"]);
    assert_eq!(shallow["paths"]["depth"], 1);
    assert_eq!(names(&shallow).len(), 9);
    assert!(names(&shallow)
        .iter()
        .all(|n| !n.contains('.') || n.starts_with("[\"")));
    // The cap: the first three paths, and a note naming the rest.
    let capped = json(&["--max-paths", "3"]);
    assert_eq!(names(&capped), ["id", "ts", "level"]);
    assert_eq!(capped["paths"]["capped"], true);
    assert_eq!(capped["paths"]["dropped"][0], "[\"http.status\"]");
    let out = stdout(&requests(&["--max-paths", "3", "--width", "100"]));
    assert!(
        out.contains("more paths not profiled (max_paths 3)"),
        "{out}"
    );
    // `--columns` names paths, quoted keys as written.
    let picked = json(&["--columns", "items[].qty,[\"http.status\"]"]);
    assert_eq!(names(&picked), ["items[].qty", "[\"http.status\"]"]);
    let (code, stderr) = failure(&requests(&["--columns", "user.nope"]));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("no path \"user.nope\"; the paths are id, ts"),
        "{stderr}"
    );
    // `--flat`: rows of the top-level keys; the malformed line is an error.
    let (code, stderr) = failure(&requests(&["--flat"]));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("line 13: not JSON"), "{stderr}");
    // `--depth` is still `rich deps`'s elsewhere, and must be positive.
    let (code, stderr) = failure(&requests(&["--depth", "0"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--depth requires a positive integer"),
        "{stderr}"
    );
    let (code, stderr) = failure(&run(&["orders.csv", "--depth", "2"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--depth only has an effect with `rich deps`"),
        "{stderr}"
    );
    // CSV has no paths.
    let (code, stderr) = failure(&run(&["profile", "orders.csv", "--depth", "2", "--flat"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--depth, --flat only apply to JSON Lines; orders.csv is read as CSV"),
        "{stderr}"
    );
}

#[test]
fn malformed_lines_are_counted_not_fatal() {
    let out = stdout(&run_with(
        &[("bad.jsonl", "{\"a\": 1}\n{\"a\": \nnope\n{\"a\": \"x\"}\n")],
        "",
        &["profile", "bad.jsonl", "--report", "json"],
    ));
    let json: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        (json["rows"].clone(), json["malformed"].clone()),
        (2.into(), 2.into())
    );
    assert_eq!(json["columns"][0]["mixed"], true);
    // Nothing but malformed lines: nothing to profile, said with why.
    let (code, stderr) = failure(&run_with(
        &[("bad.jsonl", "{oops\nnope\n")],
        "",
        &["profile", "bad.jsonl"],
    ));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("bad.jsonl: no records to profile (2 lines are not JSON"),
        "{stderr}"
    );
}

/// `--style-rule` on `rich profile`'s table compares the model's values.
#[test]
fn style_rules_style_the_profile_table() {
    let out = stdout(&run(&[
        "profile",
        "orders.csv",
        "--force-terminal",
        "--style-rule",
        "nulls>3:row:reverse",
        "--style-rule",
        "column=region:bold blue",
    ]));
    // `amount` has 4 nulls; its row is reversed, `region`'s name blue.
    let amount = out
        .lines()
        .find(|l| l.contains("amount") && l.contains("float"))
        .unwrap();
    assert!(amount.contains("\x1b[7m"), "{amount:?}");
    let region = out
        .lines()
        .find(|l| l.contains("region") && l.contains("text"))
        .unwrap();
    assert!(region.contains("\x1b[1;34mregion"), "{region:?}");
    // A nested profile's headings: `absent`, `types`.
    let out = stdout(&requests(&[
        "--force-terminal",
        "--width",
        "100",
        "--style-rule",
        "absent>=9:dim",
    ]));
    let error = out.lines().find(|l| l.contains("error.code")).unwrap();
    assert!(error.contains("\x1b[2m"), "{error:?}");
    // A rule for a heading the table lacks is warned about.
    let out = run(&["profile", "orders.csv", "--style-rule", "latency>1:red"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("warning: style rule \"latency>1:red\" names no column of this table"),
        "{stderr}"
    );
}

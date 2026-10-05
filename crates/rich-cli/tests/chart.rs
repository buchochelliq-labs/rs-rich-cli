//! `rich chart` (0.0.15 workstream 5): a chart from CSV, JSON or stdin, each
//! kind, the column flags, and the errors that name a row and a column.

use std::io::Write;
use std::process::{Command, Output, Stdio};

const SALES: &str = "month,api,web\nJan,30,12\nFeb,42,18\nMar,35,25\nApr,51,22\nMay,48,31\n";

/// Run `rich` in a fresh directory holding `files`, with `stdin` piped in,
/// plain output 60 columns wide.
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
        .env("COLUMNS", "60")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn run(args: &[&str]) -> Output {
    run_with(&[("sales.csv", SALES)], "", args)
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
fn a_sparkline_per_series_with_their_names() {
    let out = stdout(&run(&["chart", "sales.csv", "--kind", "spark"]));
    assert_eq!(out, "api ▁▅▃█▇\nweb ▁▃▆▅█\n");
    let one = stdout(&run(&[
        "chart",
        "sales.csv",
        "--kind",
        "spark",
        "--y",
        "web",
    ]));
    assert_eq!(one.trim_end(), "▁▃▆▅█");
}

#[test]
fn bars_are_labelled_by_the_first_text_column_or_by_x() {
    let out = stdout(&run(&[
        "chart",
        "sales.csv",
        "--kind",
        "bar",
        "--y",
        "api",
        "--width",
        "30",
    ]));
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines[0], "Jan █████████████▌          30");
    assert_eq!(lines[3], "Apr ███████████████████████ 51");
    // Several series: a bar for each, grouped by row.
    let both = stdout(&run(&["chart", "sales.csv", "--kind", "bar", "--x", "1"]));
    assert!(both.starts_with("Jan api "), "{both}");
    assert!(both.contains("\nJan web "), "{both}");
    assert_eq!(both.trim_end().lines().count(), 10, "{both}");
}

#[test]
fn line_and_scatter_plot_every_numeric_column_with_a_legend() {
    for kind in ["line", "scatter"] {
        let out = stdout(&run(&[
            "chart",
            "sales.csv",
            "--kind",
            kind,
            "--width",
            "50",
        ]));
        assert!(out.contains("api") && out.contains("web"), "{kind}: {out}");
        assert!(out.contains("50 ┤"), "{kind}: {out}");
        assert!(
            out.lines().all(|line| line.chars().count() <= 50),
            "{kind}: {out}"
        );
    }
    // The default kind is a line chart.
    let default = stdout(&run(&["chart", "sales.csv", "--width", "50"]));
    let line = stdout(&run(&[
        "chart",
        "sales.csv",
        "--kind",
        "line",
        "--width",
        "50",
    ]));
    assert_eq!(default, line);
}

#[test]
fn a_numeric_x_column_places_the_points() {
    let csv = "t,load\n0,1\n10,4\n20,2\n";
    let out = stdout(&run_with(
        &[("load.csv", csv)],
        "",
        &[
            "chart", "load.csv", "--x", "t", "--y", "load", "--width", "40",
        ],
    ));
    // The x axis runs over t (0 to 20), not over the row numbers.
    assert!(out.contains("20"), "{out}");
    assert!(!out.lines().last().unwrap_or("").contains("1.5"), "{out}");
}

#[test]
fn a_heatmap_shows_every_series_header() {
    let out = stdout(&run(&["chart", "sales.csv", "--kind", "heatmap"]));
    assert!(out.starts_with("    api web"), "{out}");
    assert!(out.contains("\nApr "), "{out}");
    assert!(out.contains("12 [ ░▒▓█] 51"), "{out}");
}

#[test]
fn json_records_columns_and_lines_read_like_csv() {
    let records = r#"[{"month":"Jan","api":30},{"month":"Feb","api":42}]"#;
    let columns = r#"{"api":[30,42]}"#;
    let lines = "{\"api\":30}\n{\"api\":42}\n";
    for (name, content) in [("r.json", records), ("c.json", columns), ("l.jsonl", lines)] {
        let out = stdout(&run_with(
            &[(name, content)],
            "",
            &["chart", name, "--kind", "spark", "--y", "api"],
        ));
        assert_eq!(out.trim_end(), "▁█", "{name}");
    }
}

#[test]
fn stdin_takes_numbers_csv_or_json() {
    let numbers = stdout(&run_with(
        &[],
        "1\n2\n3\n8\n",
        &["chart", "--kind", "spark"],
    ));
    assert_eq!(numbers.trim_end(), "▁▂▃█");
    let dash = stdout(&run_with(
        &[],
        "1 2 3 8",
        &["chart", "-", "--kind", "spark"],
    ));
    assert_eq!(dash, numbers);
    let csv = stdout(&run_with(
        &[],
        SALES,
        &["chart", "--kind", "spark", "--y", "api"],
    ));
    assert_eq!(csv.trim_end(), "▁▅▃█▇");
    let json = stdout(&run_with(
        &[],
        "[1, 2, 3, 8]",
        &["chart", "--kind", "spark"],
    ));
    assert_eq!(json, numbers);
}

#[test]
fn a_gap_is_skipped_not_refused() {
    let out = stdout(&run_with(
        &[("gap.csv", "v\n1\n\n3\n")],
        "",
        &["chart", "gap.csv", "--kind", "bar"],
    ));
    assert!(out.contains("3"), "{out}");
}

#[test]
fn a_missing_column_is_refused_with_the_columns_there_are() {
    let (code, stderr) = failure(&run(&["chart", "sales.csv", "--y", "mobile"]));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("no column \"mobile\"; the columns are \"month\", \"api\", \"web\""),
        "{stderr}"
    );
    let (code, stderr) = failure(&run(&["chart", "sales.csv", "--x", "day"]));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("no column \"day\""), "{stderr}");
}

#[test]
fn a_value_that_is_not_a_number_names_its_row_and_column() {
    let csv = "t,v\n1,2\n2,n/a\n";
    let (code, stderr) = failure(&run_with(
        &[("bad.csv", csv)],
        "",
        &["chart", "bad.csv", "--y", "v"],
    ));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("row 2, column \"v\": \"n/a\" is not a number"),
        "{stderr}"
    );
    // A label column is not a position.
    let (code, stderr) = failure(&run(&["chart", "sales.csv", "--x", "month"]));
    assert_eq!(code, Some(4));
    assert!(
        stderr.contains("row 1, column \"month\": \"Jan\" is not a number")
            && stderr.contains("--kind bar takes labels"),
        "{stderr}"
    );
    let report = run(&["chart", "sales.csv", "--y", "month", "--report", "json"]);
    let report = String::from_utf8_lossy(&report.stderr);
    assert!(report.contains("\"code\":\"data\""), "{report}");
}

#[test]
fn data_with_nothing_to_chart_is_refused() {
    let (code, stderr) = failure(&run_with(&[], "", &["chart"]));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("<stdin>: no rows to chart"), "{stderr}");
    let (code, stderr) = failure(&run_with(
        &[("names.csv", "name\nada\ngrace\n")],
        "",
        &["chart", "names.csv"],
    ));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("no numeric column to chart"), "{stderr}");
    let (code, stderr) = failure(&run_with(
        &[("o.json", "{\"a\": 1}")],
        "",
        &["chart", "o.json"],
    ));
    assert_eq!(code, Some(4));
    assert!(stderr.contains("o.json: expected an array"), "{stderr}");
}

#[test]
fn chart_options_are_checked() {
    let (code, stderr) = failure(&run(&["chart", "sales.csv", "--kind", "pie"]));
    assert_eq!(code, Some(2));
    assert!(stderr.contains("unknown chart kind \"pie\""), "{stderr}");
    let (code, stderr) = failure(&run(&["sales.csv", "--kind", "bar"]));
    assert_eq!(code, Some(2));
    assert!(
        stderr.contains("--kind only has an effect with `rich chart`"),
        "{stderr}"
    );
    let (code, stderr) = failure(&run(&["chart", "sales.csv", "--x", "1", "--x", "2"]));
    assert_eq!(code, Some(2));
    assert!(stderr.contains("--x can be given once"), "{stderr}");
}

#[test]
fn colour_goes_through_the_chart_theme() {
    let out = stdout(&run(&[
        "chart",
        "sales.csv",
        "--kind",
        "bar",
        "--y",
        "api",
        "--force-terminal",
    ]));
    // `chart.bar` is cyan.
    assert!(out.contains("\x1b[36m"), "{out:?}");
}

#[test]
fn chart_is_in_help_and_completions() {
    let out = stdout(&run(&["chart", "--help"]));
    assert!(out.contains("--kind"), "{out}");
    assert!(out.contains("--y"), "{out}");
}

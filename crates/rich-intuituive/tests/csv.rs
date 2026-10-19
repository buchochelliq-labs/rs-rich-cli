//! The csvlens-style CSV viewer (examples/csv.rs), on CSV files written
//! for each test: a small one of cities, a wide one, and one of 200,000
//! rows for the lazy reading.

#[path = "../examples/csv.rs"]
#[allow(dead_code)]
mod csv;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use csv::{csv_app, parse_record, sniff, Options, Source};
use intuituive::rich::Style;
use intuituive::{App, Driver};

/// A file in the temporary directory, named for the test.
fn file(name: &str, body: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("intuituive-csv-{}-{name}", std::process::id()));
    std::fs::write(&path, body).unwrap();
    path
}

fn options(path: &Path) -> Options {
    Options {
        path: path.to_path_buf(),
        name: "cities.csv".into(),
        ..Options::default()
    }
}

fn app_with(options: Options) -> App {
    csv_app(options).unwrap().wait_for_tasks(true)
}

fn app(path: &Path) -> App {
    app_with(options(path))
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn status(driver: &Driver) -> String {
    rows(driver).last().unwrap().clone()
}

/// The row numbers down the gutter, top to bottom.
fn numbers(driver: &Driver) -> Vec<String> {
    rows(driver)
        .iter()
        .filter_map(|r| r.split_once("  │").map(|(n, _)| n.trim().to_string()))
        .collect()
}

const CITIES: &str = "City,State,Population,Note\n\
Ravenna,OH,11000,small\n\
Reading,PA,95000,\"quoted, with comma\"\n\
Red Bluff,CA,14000,\"has \"\"quotes\"\"\"\n\
Regina,SA,226000,capital\n\
Reno,NV,264000,biggest little city\n";

/// Ten columns, `colN` over `rRcN` cells.
fn wide(rows: usize) -> String {
    let mut out = (0..10)
        .map(|c| format!("col{c}"))
        .collect::<Vec<_>>()
        .join(",");
    for r in 0..rows {
        out.push('\n');
        let cells: Vec<String> = (0..10).map(|c| format!("r{r}c{c}-xxxxxxxx")).collect();
        out.push_str(&cells.join(","));
    }
    out.push('\n');
    out
}

#[test]
fn it_shows_the_table_as_csvlens_does() {
    let path = file("look", CITIES);
    let mut d = app(&path).driver(60, 10);
    d.settle();
    assert_eq!(
        rows(&d),
        [
            "─".repeat(60).as_str(),
            "      City         State    Population    Note",
            "1  │  Ravenna      OH       11000         small",
            "2  │  Reading      PA       95000         quoted, with comm…",
            "3  │  Red Bluff    CA       14000         has \"quotes\"",
            "4  │  Regina       SA       226000        capital",
            "5  │  Reno         NV       264000        biggest little ci…",
            "",
            "─".repeat(60).as_str(),
            "cities.csv [Row 1/5, Col 1/4]",
        ]
    );
    // csvlens's selected row: bold light grey on dark olive.
    let selected = Style::parse("bold rgb(192,192,192) on rgb(62,61,50)").unwrap();
    let cell = d.screen().cell(8, 2).style;
    assert_eq!(d.screen().style(cell), Some(&selected));
}

#[test]
fn it_parses_quotes_and_sniffs_the_delimiter() {
    assert_eq!(
        parse_record(b"a,\"b, c\",\"say \"\"hi\"\"\"\r\n", b','),
        ["a", "b, c", "say \"hi\""]
    );
    assert_eq!(sniff(b"a\tb\tc\n1\t2\t3\n"), b'\t');
    assert_eq!(sniff(b"a;b;c\n1;2;3\n"), b';');
    assert_eq!(sniff(b"a|b\n\"x|y\"|2\n"), b'|');
    assert_eq!(sniff(b"name\nalpha\n"), b',');
    // A quoted line break stays in its record.
    let path = file("multi.csv", "a,b\n\"one\ntwo\",2\nlast,3\n");
    let src = Source::open(&path, None, false).unwrap();
    assert_eq!(src.rows(), 2);
    assert_eq!(*src.row(0), ["one\ntwo", "2"]);
    assert_eq!(*src.row(1), ["last", "3"]);
}

#[test]
fn it_reads_tsv_and_files_without_headers() {
    let path = file("t.tsv", "x\ty\n1\t2\n");
    let mut d = app(&path).driver(40, 6);
    d.settle();
    assert_eq!(rows(&d)[1], "      x    y");
    assert_eq!(rows(&d)[2], "1  │  1    2");

    let path = file("nohead.csv", "1,2\n3,4\n");
    let mut options = options(&path);
    options.no_headers = true;
    let mut d = app_with(options).driver(40, 6);
    d.settle();
    assert_eq!(rows(&d)[1], "      1    2");
    assert_eq!(rows(&d)[2], "1  │  1    2");
    assert!(status(&d).contains("[Row 1/2, Col 1/2]"));
}

#[test]
fn it_moves_by_rows_pages_and_lines() {
    let path = file("move.csv", &wide(100));
    let mut d = app(&path).driver(60, 12);
    d.press("j j");
    assert!(status(&d).contains("[Row 3/100,"));
    d.press("G");
    assert!(status(&d).contains("[Row 100/100,"));
    assert_eq!(numbers(&d).last().unwrap(), "100");
    d.press("g");
    assert!(status(&d).contains("[Row 1/100,"));
    // A window is the table's rows: 12 less 3 lines of chrome, less the
    // header.
    d.press("ctrl+f");
    assert!(status(&d).contains("[Row 9/100,"));
    d.press("ctrl+d");
    assert!(status(&d).contains("[Row 13/100,"));
    d.press("u");
    assert!(status(&d).contains("[Row 9/100,"));
    // <n>G: the digits show in the status line as they are typed.
    d.press("4 2");
    assert_eq!(status(&d), "Go to line: 42");
    d.press("G");
    assert!(status(&d).contains("[Row 42/100,"));
    d.press("colon");
    d.type_text("7");
    d.press("enter");
    assert!(status(&d).contains("[Row 7/100,"));
}

#[test]
fn it_scrolls_across_with_frozen_row_numbers_and_columns() {
    let path = file("wide.csv", &wide(3));
    let mut d = app(&path).driver(60, 8);
    d.settle();
    assert!(rows(&d)[1].starts_with("      col0"));
    // Row mode: l scrolls a column across; the row numbers stay.
    d.press("l l");
    assert!(rows(&d)[2].starts_with("1  │"));
    assert!(!rows(&d)[1].contains("col0"));
    assert!(!rows(&d)[1].contains("col1 "));
    // f1 freezes the first column: it stays while the rest scroll.
    d.press("f 1");
    d.press("l l l");
    assert!(rows(&d)[1].contains("col0"));
    assert!(!rows(&d)[1].contains("col1 "));
    assert!(rows(&d)[2].starts_with("1  │  r0c0-xxxxxxxx"));
}

#[test]
fn it_cycles_selection_modes_and_moves_the_cell_cursor() {
    let path = file("modes.csv", &wide(3));
    let mut d = app(&path).driver(60, 8);
    let selected = Style::parse("bold rgb(192,192,192) on rgb(62,61,50)").unwrap();
    d.press("tab");
    assert!(status(&d).contains("Col 1/10"));
    d.press("l l");
    assert!(status(&d).contains("Col 3/10"));
    // Column mode: the column is highlighted down the rows, the row is not.
    let x = rows(&d)[1].find("col2").unwrap() as u16;
    for y in [2, 3, 4] {
        let cell = d.screen().cell(x, y).style;
        assert_eq!(d.screen().style(cell), Some(&selected), "row {y}");
    }
    let cell = d.screen().cell(8, 2).style;
    assert_ne!(d.screen().style(cell), Some(&selected));
    // Cell mode: only the cell.
    d.press("tab");
    let cell = d.screen().cell(x, 3).style;
    assert_ne!(d.screen().style(cell), Some(&selected));
    let cell = d.screen().cell(x, 2).style;
    assert_eq!(d.screen().style(cell), Some(&selected));
    // Enter prints the cell and exits.
    d.press("enter");
    assert_eq!(d.take_exit::<String>().as_deref(), Some("r0c2-xxxxxxxx"));
}

#[test]
fn it_finds_highlights_and_jumps_between_matches() {
    let path = file("find.csv", CITIES);
    let mut d = app(&path).driver(60, 10);
    d.press("slash");
    d.type_text("Re");
    assert_eq!(status(&d), "Find: Re");
    d.press("enter");
    // The first match from the top: Reading, the second row.
    assert!(status(&d).ends_with("[Row 2/5, Col 1/4] [Find \"Re\": 1/4]"));
    // The matches are drawn in csvlens's red (the selected row's are
    // covered by the selection), the rest of the cell plain.
    let red = Style::parse("rgb(200,0,0)").unwrap();
    for (x, y) in [(6, 4), (7, 4), (6, 5), (6, 6)] {
        let cell = d.screen().cell(x, y).style;
        assert_eq!(d.screen().style(cell), Some(&red), "({x}, {y})");
    }
    let cell = d.screen().cell(8, 4).style;
    assert_ne!(d.screen().style(cell), Some(&red));
    d.press("n n");
    assert!(status(&d).ends_with("[Row 4/5, Col 1/4] [Find \"Re\": 3/4]"));
    d.press("N");
    assert!(status(&d).ends_with("[Row 3/5, Col 1/4] [Find \"Re\": 2/4]"));
    d.press("slash");
    d.type_text("nowhere");
    d.press("enter");
    assert!(status(&d).ends_with("[Find \"nowhere\": Not found]"));
    d.press("slash");
    d.type_text("(");
    d.press("enter");
    assert_eq!(status(&d), "Invalid regex: (");
}

#[test]
fn it_filters_rows_and_columns_and_resets() {
    let path = file("filter.csv", CITIES);
    let mut d = app(&path).driver(90, 10);
    d.press("ampersand");
    d.type_text("OH|CA");
    d.press("enter");
    assert_eq!(numbers(&d), ["1", "3"]);
    assert!(status(&d).ends_with("[Row 1/5, Col 1/4] [Filter \"OH|CA\": 1/2]"));
    d.press("j");
    assert!(status(&d).ends_with("[Row 3/5, Col 1/4] [Filter \"OH|CA\": 2/2]"));
    d.press("asterisk");
    d.type_text("Ci|Pop");
    d.press("enter");
    assert_eq!(rows(&d)[1], "      City         Population");
    assert!(status(&d).ends_with("[Filter \"Ci|Pop\": 2/4 cols]"));
    d.press("r");
    assert_eq!(numbers(&d), ["1", "2", "3", "4", "5"]);
    assert_eq!(
        rows(&d)[1],
        "      City         State    Population    Note"
    );
    // The selection stays where it was.
    assert_eq!(status(&d), "cities.csv [Row 2/5, Col 1/4]");
    // `-i` ignores case.
    let mut options = options(&path);
    options.ignore_case = true;
    let mut d = app_with(options).driver(90, 10);
    d.press("ampersand");
    d.type_text("reno");
    d.press("enter");
    assert_eq!(numbers(&d), ["5"]);
    assert!(status(&d).ends_with("[ignore-case]"));
}

#[test]
fn it_sorts_by_the_selected_column() {
    let path = file("sort.csv", CITIES);
    let mut d = app(&path).driver(70, 10);
    d.press("J");
    assert_eq!(status(&d), "Press TAB and select a column before sorting");
    // Population: numbers sort as numbers.
    d.press("tab l l J");
    assert!(rows(&d)[1].contains("Population [▴]"));
    assert_eq!(numbers(&d), ["1", "3", "2", "4", "5"]);
    d.press("J");
    assert!(rows(&d)[1].contains("Population [▾]"));
    assert_eq!(numbers(&d), ["5", "4", "2", "3", "1"]);
    // Text sorts as text; `#` finds rows like the selected cell.
    d.press("h J");
    assert!(rows(&d)[1].contains("State [▴]"));
    assert_eq!(numbers(&d), ["3", "5", "1", "2", "4"]);
    d.press("tab j j hash");
    assert!(status(&d).ends_with("[Find \"^OH$\": 1/1]"));
}

#[test]
fn it_resizes_wraps_and_marks() {
    let path = file("resize.csv", CITIES);
    let mut d = app(&path).driver(90, 12);
    d.settle();
    let before = rows(&d)[1].find("Population").unwrap();
    // `>` widens the selected column by csvlens's four cells, `<` narrows it.
    d.press("tab l gt");
    assert_eq!(rows(&d)[1].find("Population").unwrap(), before + 4);
    d.press("lt lt");
    assert!(rows(&d)[1].find("Population").unwrap() < before);
    assert!(rows(&d)[1].contains("St…"));
    // -S: long cells wrap onto more lines.
    let mut d = app(&path).driver(60, 12);
    d.press("minus S");
    let screen = rows(&d);
    let at = screen
        .iter()
        .position(|r| r.contains("biggest little"))
        .unwrap();
    assert!(screen[at + 1].trim().starts_with("city"), "{screen:#?}");
    d.press("minus S");
    assert!(rows(&d).iter().any(|r| r.ends_with("biggest little ci…")));
    // m marks rows; Ctrl+E prints them, with the header, and exits.
    d.press("tab tab j m j j m");
    let marked = Style::parse("rgb(220,230,255) on rgb(40,50,80)").unwrap();
    let cell = d.screen().cell(8, 3).style;
    assert_eq!(d.screen().style(cell), Some(&marked));
    d.press("ctrl+e");
    assert_eq!(
        d.take_exit::<String>().as_deref(),
        Some("City,State,Population,Note\nReading,PA,95000,\"quoted, with comma\"\nRegina,SA,226000,capital")
    );
}

#[test]
fn it_copies_and_shows_the_help() {
    let path = file("help.csv", CITIES);
    let mut d = app(&path).driver(80, 30);
    d.press("y");
    assert_eq!(d.take_copies(), ["Ravenna,OH,11000,small"]);
    assert_eq!(status(&d), "Copied Ravenna,OH,11000,small to clipboard");
    d.press("j");
    assert!(status(&d).starts_with("cities.csv [Row 2/5"));
    d.press("question");
    let screen = rows(&d).join("\n");
    for line in [
        "csvlens",
        "Moving",
        "j/↓",
        "Scroll one row down",
        "Search",
        "/ ",
    ] {
        assert!(screen.contains(line), "{line} in {screen}");
    }
    // The help searches as csvlens's keys are typed.
    d.type_text("sort");
    let screen = rows(&d).join("\n");
    assert!(screen.contains("Selection modes"), "{screen}");
    assert!(
        screen.contains("Sort rows by the selected column"),
        "{screen}"
    );
}

/// 200,000 rows: the first frame reads only the rows in view, and is
/// quick, however long the file.
#[test]
fn it_opens_a_large_file_reading_only_the_window() {
    let mut body = String::from("id,name,value,comment\n");
    for i in 0..200_000 {
        body.push_str(&format!("{i},name{i},{},\"note, {i}\"\n", i * 7 % 1000));
    }
    let path = file("large.csv", &body);

    // Before the background index: only the first 64 KiB are indexed.
    let src = Source::open(&path, None, false).unwrap();
    assert!(src.rows() < 5_000, "{}", src.rows());

    let parsed = Arc::new(AtomicUsize::new(0));
    let mut opened = options(&path);
    opened.parsed = Some(parsed.clone());
    let app = csv_app(opened).unwrap();
    let started = Instant::now();
    let mut d = app.driver(100, 40);
    d.settle();
    let first = started.elapsed();
    let screen = rows(&d);
    assert_eq!(screen[2], "1   │  0     name0     0        note, 0");
    assert!(first < Duration::from_secs(2), "first frame took {first:?}");
    // The table's 35 rows in view, each read once.
    let read = parsed.load(Ordering::Relaxed);
    assert!(read <= 40, "{read} rows read for the first frame");
    eprintln!("first frame of 200,000 rows: {first:?}, {read} rows read");

    // With the whole index, the end of the file: still only a window read.
    let parsed = Arc::new(AtomicUsize::new(0));
    let mut opened = options(&path);
    opened.parsed = Some(parsed.clone());
    let mut d = app_with(opened).driver(100, 40);
    d.settle();
    assert!(status(&d).contains("[Row 1/200000, Col 1/4]"));
    let before = parsed.load(Ordering::Relaxed);
    let started = Instant::now();
    d.press("G");
    let took = started.elapsed();
    let read = parsed.load(Ordering::Relaxed) - before;
    eprintln!("G on 200,000 rows: {took:?}, {read} rows read");
    assert!(read <= 40, "{read} rows read for G");
    assert!(status(&d).contains("[Row 200000/200000, Col 1/4]"));
    assert_eq!(numbers(&d).last().unwrap(), "200000");
    let started = Instant::now();
    d.press("pageup pageup pageup");
    eprintln!("three pages up: {:?}", started.elapsed());
    assert!(status(&d).contains("[Row 199892/200000"), "{}", status(&d));
}

#[test]
fn it_counts_only_the_rows_in_view() {
    let path = file("count.csv", &wide(50_000));
    let src = Source::open(&path, None, false).unwrap();
    let parsed = src.parsed.clone();
    for i in 0..30 {
        src.row(i);
    }
    src.row(0);
    assert_eq!(parsed.load(Ordering::Relaxed), 30);
}

/// The numbers in the gap report: a million rows (about 40 MB), run by
/// hand with `--ignored --nocapture`.
#[test]
#[ignore]
fn measure_a_million_rows() {
    let mut body = String::from("id,name,value,comment\n");
    for i in 0..1_000_000 {
        body.push_str(&format!("{i},name{i},{},\"note, {i}\"\n", i * 7 % 1000));
    }
    let path = file("million.csv", &body);
    drop(body);
    let parsed = Arc::new(AtomicUsize::new(0));
    let mut opened = options(&path);
    opened.parsed = Some(parsed.clone());
    let started = Instant::now();
    let mut d = csv_app(opened).unwrap().driver(120, 40);
    d.settle();
    eprintln!(
        "first frame: {:?}, {} rows read",
        started.elapsed(),
        parsed.load(Ordering::Relaxed)
    );
    let started = Instant::now();
    for _ in 0..100 {
        d.press("j");
    }
    eprintln!(
        "100 x j: {:?} ({:?} a key)",
        started.elapsed(),
        started.elapsed() / 100
    );
    let started = Instant::now();
    for _ in 0..20 {
        d.press("pagedown");
    }
    eprintln!(
        "20 x PgDn: {:?}, {} rows read in all",
        started.elapsed(),
        parsed.load(Ordering::Relaxed)
    );
    // The whole index, then G, then a filter (a pass over the file).
    let mut opened = options(&path);
    opened.parsed = Some(parsed.clone());
    let started = Instant::now();
    let mut d = app_with(opened).driver(120, 40);
    d.settle();
    eprintln!("indexed in {:?}: {}", started.elapsed(), status(&d));
    let started = Instant::now();
    d.press("G");
    eprintln!("G: {:?}: {}", started.elapsed(), status(&d));
    d.press("ampersand");
    d.type_text("name99");
    let started = Instant::now();
    d.press("enter");
    eprintln!("filter: {:?}: {}", started.elapsed(), status(&d));
    let _ = std::fs::remove_file(&path);
    // The same keys on 50 rows: what a key costs whatever the length.
    let path = file("fifty.csv", &wide(50));
    let mut d = app(&path).driver(120, 40);
    d.settle();
    let started = Instant::now();
    for _ in 0..30 {
        d.press("j");
    }
    eprintln!("30 x j on 50 rows: {:?} a key", started.elapsed() / 30);
}

#[test]
fn it_takes_csvlens_options_for_the_first_view() {
    let path = file("opts.csv", CITIES);
    let mut opened = options(&path);
    opened.filter = Some("PA|NV".into());
    opened.columns = Some("City|Note".into());
    let mut d = app_with(opened).driver(100, 10);
    d.settle();
    assert_eq!(rows(&d)[1], "      City       Note");
    assert_eq!(numbers(&d), ["2", "5"]);
    assert!(status(&d)
        .ends_with("[Row 2/5, Col 1/2] [Filter \"PA|NV\": 1/2] [Filter \"City|Note\": 2/4 cols]"));
    let mut opened = options(&path);
    opened.find = Some("capital".into());
    let mut d = app_with(opened).driver(100, 10);
    d.settle();
    assert!(status(&d).ends_with("[Row 4/5, Col 1/4] [Find \"capital\": 1/1]"));
}

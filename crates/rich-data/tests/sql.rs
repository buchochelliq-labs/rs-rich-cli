//! SQL-shaped result sets (#235) and windows of large sources (#260), from
//! every adapter.

use std::time::{Duration, Instant};

use rich::{ColorSystem, Console};
use rich_data::csv::CsvReader;
use rich_data::infer::Inferrer;
use rich_data::sql::ResultSet;
use rich_data::window::RowWindow;
use rich_data::{jsonl, DataError, Row, RowSource, Value};

const CSV: &str = include_str!("fixtures/services.csv");
const JSONL: &str = include_str!("fixtures/services.jsonl");

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

const TYPED: &str = "\
┏━━━━━━━━━┳━━━━━━━━━━┳━━━━━━━┳━━━━━━━━━┳━━━━━━━━━━━━┓
┃ service ┃ replicas ┃   p99 ┃ healthy ┃ deployed   ┃
┡━━━━━━━━━╇━━━━━━━━━━╇━━━━━━━╇━━━━━━━━━╇━━━━━━━━━━━━┩
│ web     │        3 │ 120.5 │  true   │ 2026-10-01 │
│ api     │        2 │    35 │  false  │ 2026-10-02 │
│ db      │        1 │  NULL │  true   │ 2026-09-30 │
└─────────┴──────────┴───────┴─────────┴────────────┘
";

#[test]
fn inferred_csv_renders_typed_with_nulls_and_a_count() {
    let mut rows = CsvReader::new().read(CSV).unwrap();
    Inferrer::new().infer(&rows).apply(&mut rows);
    let result = ResultSet::new(rows).elapsed(Duration::from_micros(1_500));
    let out = plain(60).render_export(&result);
    assert_eq!(out, format!("{TYPED}(3 rows, 1ms)\n"));
}

#[test]
fn a_jsonl_stream_reads_into_the_same_result() {
    let source = jsonl::source(JSONL.as_bytes()).unwrap();
    let result = ResultSet::read(source, 0, 100).unwrap();
    let out = plain(60).render_export(&result);
    // JSON's 35 is an integer, so p99 stays as written; the schema still
    // types the column as a number.
    assert!(
        out.contains("│ api     │        2 │    35 │  false  │ 2026-10-02 │"),
        "{out}"
    );
    assert!(out.contains("│  NULL │"), "{out}");
    assert!(out.ends_with("(3 rows)\n"), "{out}");
}

#[test]
fn null_is_never_mistaken_for_empty_text() {
    let mut rows = rich_data::Rows::new(["note"]);
    rows.push([Value::Null]);
    rows.push([Value::from("")]);
    let console = Console::builder()
        .width(30)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let out = console.render_export(&ResultSet::new(rows));
    assert!(out.contains("\u{1b}[2;3mNULL\u{1b}[0m"), "{out:?}");
    assert_eq!(out.matches("NULL").count(), 1, "{out:?}");
}

/// Ten million synthetic rows, made one at a time and never kept.
struct Synthetic {
    columns: Vec<String>,
    next: usize,
    len: usize,
}

impl RowSource for Synthetic {
    fn columns(&self) -> &[String] {
        &self.columns
    }

    fn next_row(&mut self) -> Option<Result<Row, DataError>> {
        if self.next == self.len {
            return None;
        }
        let n = self.next;
        self.next += 1;
        Some(Ok(vec![
            Value::from(n),
            if n.is_multiple_of(7) {
                Value::Null
            } else {
                Value::Float(n as f64 / 8.0)
            },
        ]))
    }
}

#[test]
fn a_window_of_a_long_forward_source_holds_only_the_window() {
    let source = Synthetic {
        columns: vec!["id".into(), "value".into()],
        next: 0,
        len: 10_000_000,
    };
    let started = Instant::now();
    // Without counting, reading stops one row past the window.
    let window = RowWindow::reader()
        .offset(9_999_996)
        .len(3)
        .count(false)
        .read(source)
        .unwrap();
    assert_eq!(window.rows().len(), 3);
    assert_eq!(window.total(), None);
    let result = ResultSet::window(window);
    let out = plain(60).render_export(&result);
    assert!(started.elapsed() < Duration::from_secs(60));
    assert!(
        out.ends_with(
            "\
│ 9999996 │  1249999.5 │
│ 9999997 │       NULL │
│ 9999998 │ 1249999.75 │
└─────────┴────────────┘
rows 9,999,997–9,999,999 of 10,000,000+
(10,000,000+ rows)
"
        ),
        "{out}"
    );
}

#[test]
fn a_counted_window_reports_the_total() {
    let source = Synthetic {
        columns: vec!["id".into(), "value".into()],
        next: 0,
        len: 100_000,
    };
    let result = ResultSet::read(source, 70, 2).unwrap();
    assert_eq!(result.row_count(), Some(100_000));
    let out = plain(60).render_export(&result);
    assert!(out.contains("│ 70 │   NULL │\n│ 71 │  8.875 │"), "{out}");
    assert!(
        out.ends_with("rows 71–72 of 100,000\n(100,000 rows)\n"),
        "{out}"
    );
}

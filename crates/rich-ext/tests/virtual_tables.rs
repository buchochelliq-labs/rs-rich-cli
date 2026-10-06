//! Virtualised tables (#260): one window of a large row source.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use rich::{ColorSystem, Console, Justify};
use rich_ext::a11y::AccessibleText;
use rich_ext::table::{Column, FnRows, Value, VirtualRows, VirtualTable};

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

#[test]
fn a_window_of_ten_million_rows_reads_only_what_it_shows() {
    let calls = AtomicUsize::new(0);
    let rows = FnRows::new(Some(10_000_000), |i| {
        calls.fetch_add(1, Ordering::Relaxed);
        Some(vec![
            Value::from(i),
            format!("host-{:03}", i % 1000).into(),
            Value::Float((i % 97) as f64 / 4.0),
        ])
    });
    let table = VirtualTable::new(
        [
            Column::new("seq").justify(Justify::Right),
            Column::new("host"),
            Column::new("load").justify(Justify::Right),
        ],
        &rows,
    )
    .widths([8])
    .offset(5_000_000)
    .height(4);

    let started = Instant::now();
    let out = plain(60).render_export(&table);
    assert!(started.elapsed().as_secs() < 5, "{:?}", started.elapsed());
    assert_eq!(
        out,
        "\
┏━━━━━━━━━━┳━━━━━━━━━━┳━━━━━━━┓
┃      seq ┃ host     ┃  load ┃
┡━━━━━━━━━━╇━━━━━━━━━━╇━━━━━━━┩
│  5000000 │ host-000 │   9.5 │
│  5000001 │ host-001 │  9.75 │
│  5000002 │ host-002 │    10 │
│  5000003 │ host-003 │ 10.25 │
└──────────┴──────────┴───────┘
rows 5,000,001–5,000,004 of 10,000,000
"
    );
    // The width sample (100 rows) and the window (4 rows), nothing else.
    assert_eq!(calls.load(Ordering::Relaxed), 104);

    // Widths are sampled once: scrolling reads only the new window.
    let mut table = table;
    table.scroll_by(1_000);
    plain(60).render_export(&table);
    assert_eq!(calls.load(Ordering::Relaxed), 108);
}

#[test]
fn the_last_window_and_its_position() {
    let rows: Vec<Vec<Value>> = (1..=12)
        .map(|n| vec![Value::from(n), Value::from(format!("row {n}"))])
        .collect();
    let mut table = VirtualTable::new(
        [
            Column::new("n").justify(Justify::Right),
            Column::new("label"),
        ],
        rows,
    )
    .height(5)
    .row_numbers(true);
    table.scroll_by(100);
    assert_eq!(table.window_offset(), 7);
    assert_eq!(
        plain(40).render_export(&table),
        "\
┏━━━━┳━━━━┳━━━━━━━━┓
┃  # ┃  n ┃ label  ┃
┡━━━━╇━━━━╇━━━━━━━━┩
│  8 │  8 │ row 8  │
│  9 │  9 │ row 9  │
│ 10 │ 10 │ row 10 │
│ 11 │ 11 │ row 11 │
│ 12 │ 12 │ row 12 │
└────┴────┴────────┘
rows 8–12 of 12
"
    );
}

#[test]
fn an_uncounted_stream_says_how_many_rows_it_has_seen() {
    let rows = FnRows::new(None, |i| (i < 1_500).then(|| vec![Value::from(i)]));
    let mut table = VirtualTable::new([Column::new("i")], rows)
        .height(2)
        .offset(1_000);
    let out = plain(30).render_export(&table);
    assert!(out.ends_with("rows 1,001–1,002 of 1,003+\n"), "{out}");
    table.set_offset(1_499);
    let out = plain(30).render_export(&table);
    assert!(out.ends_with("rows 1,500–1,500 of 1,500\n"), "{out}");
}

#[test]
fn ascii_consoles_get_ascii_positions() {
    let rows = vec![vec![Value::from(1)], vec![Value::from(2)]];
    let table = VirtualTable::new([Column::new("n")], rows).height(1);
    let console = Console::builder().width(20).color_system(None).build();
    let out = console.render_export(&table);
    assert!(out.ends_with("rows 1–1 of 2\n"), "{out}");
    let page = table.page();
    assert_eq!(page.position(true), "rows 1-1 of 2");
}

#[test]
fn a_viewport_of_n_lines_holds_n_minus_chrome_rows() {
    let rows = FnRows::new(Some(1_000), |i| Some(vec![Value::from(i)]));
    let mut table = VirtualTable::new([Column::new("i")], rows).title("events");
    let console = plain(30);
    let lines = 12;
    table.set_height(lines - table.chrome_lines(&console, 30));
    assert_eq!(console.render_export(&table).lines().count(), lines);
}

#[test]
fn wide_cells_are_cut_short_unless_wrapping() {
    let rows = vec![vec![Value::from("a fairly long cell value")]];
    let table = VirtualTable::new([Column::new("v")], rows.clone())
        .widths([10])
        .show_position(false);
    let out = plain(40).render_export(&table);
    assert!(out.contains("│ a fairly … │"), "{out}");
    let table = VirtualTable::new([Column::new("v")], rows)
        .widths([10])
        .wrap(true)
        .show_position(false);
    assert_eq!(plain(40).render_export(&table).lines().count(), 7);
}

#[test]
fn nulls_are_styled_apart_from_empty_strings() {
    let rows = vec![vec![Value::Null, Value::from("")]];
    let table = VirtualTable::new([Column::new("a"), Column::new("b")], rows)
        .null_marker("NULL")
        .show_position(false);
    let console = Console::builder()
        .width(30)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let out = console.render_export(&table);
    assert!(out.contains("\u{1b}[2;3mNULL\u{1b}[0m"), "{out:?}");
}

#[test]
fn custom_sources_can_fetch_ranges_themselves() {
    struct Ranged;
    impl VirtualRows for Ranged {
        fn row_count(&self) -> Option<usize> {
            Some(usize::MAX)
        }
        fn row(&self, index: usize) -> Option<Vec<Value>> {
            Some(vec![Value::from(index)])
        }
        fn rows(&self, start: usize, len: usize) -> Vec<Vec<Value>> {
            // Returns more than asked: the table keeps only its window.
            (start..start.saturating_add(len + 5))
                .map(|i| vec![Value::from(i)])
                .collect()
        }
    }
    let table = VirtualTable::new([Column::new("i")], Ranged)
        .offset(usize::MAX)
        .height(2)
        .widths([30]);
    let page = table.page();
    assert_eq!(page.rows.len(), 2);
    assert_eq!(page.start, usize::MAX - 2);
    let text = table.accessible_text(40);
    assert!(text.starts_with("Table with 2 rows, columns: i"), "{text}");
}

#[test]
fn the_measurement_covers_the_position_line_and_footnote() {
    let rows = FnRows::new(Some(1_000_000), |i| Some(vec![Value::from(i % 10)]));
    let table = VirtualTable::new([Column::new("n")], rows)
        .offset(500_000)
        .height(1)
        .footnote("a footnote longer than the position line");
    let console = plain(80);
    let measured = rich::measure::Measurement::get(&console, &console.options(), &table);
    assert_eq!(
        measured.maximum,
        "a footnote longer than the position line".len()
    );
    // A fitted panel sizes itself from the measurement: nothing is cut short.
    let out = console.render_export(&rich::Panel::fit(Box::new(table)));
    assert!(out.contains("rows 500,001–500,001 of 1,000,000"), "{out}");
    assert!(
        out.contains("a footnote longer than the position line"),
        "{out}"
    );
    assert!(!out.contains('…'), "{out}");
}

#[test]
fn short_rows_sample_the_null_marker_width() {
    let rows = vec![
        vec![Value::from("a")],
        vec![Value::from("b"), Value::from(2)],
    ];
    let table = VirtualTable::new([Column::new("x"), Column::new("y")], rows)
        .null_marker("NULL")
        .show_position(false);
    assert_eq!(table.column_widths(), [1, 4]);
    let out = plain(30).render_export(&table);
    assert!(out.contains("│ a │ NULL │"), "{out}");
}

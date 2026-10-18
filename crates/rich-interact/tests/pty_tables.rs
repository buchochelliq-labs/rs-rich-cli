//! A long table paged in a real PTY keeps its header on top (#716), and a
//! wide one scrolls across (#717). See `support` for the harness.
#![cfg(unix)]

mod support;

use rich_interact::policy::Policy;
use rich_interact::{run, Pager, RunOptions, TableSelect};
use support::{assert_restored, Pty};

#[test]
fn child() {
    let Some(mode) = std::env::var_os("INTERACT_CHILD") else {
        return;
    };
    let options = RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        ..RunOptions::default()
    };
    let outcome = match mode.to_str().unwrap() {
        "table-pager" => {
            let mut table = rich::Table::new();
            table.add_column("Name");
            table.add_column("Size");
            for row in 1..=200 {
                table.add_row(&[&format!("file {row}"), &format!("{row}K")]);
            }
            format!(
                "{:?}",
                run(Pager::new(table).sticky_table_header(true), &options)
            )
        }
        "wide-table" => {
            let cells = |row: usize| {
                (0..12)
                    .map(|column| format!("r{row}c{column}-value"))
                    .collect::<Vec<_>>()
            };
            let headers: Vec<String> = (0..12).map(|column| format!("column-{column}")).collect();
            let rows = (0..5).map(|row| (row, cells(row)));
            let table = TableSelect::new("Pick ready", headers, rows)
                .scroll_x(true)
                .freeze_columns(1);
            format!("{:?}", run(table, &options))
        }
        other => panic!("unknown child {other}"),
    };
    println!("OUTCOME {outcome}");
}

/// The screen as the terminal shows it now.
fn screen(pty: &Pty) -> Vec<String> {
    let mut parser = vt100::Parser::new(24, 80, 0);
    parser.process(pty.text().as_bytes());
    parser
        .screen()
        .rows(0, 80)
        .map(|row| row.trim_end().to_string())
        .collect()
}

/// Wait until a row of the screen holds `needle`.
fn wait_for_row(pty: &Pty, needle: &str) -> Vec<String> {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let rows = screen(pty);
        if rows.iter().any(|row| row.contains(needle)) {
            return rows;
        }
        assert!(
            std::time::Instant::now() < end,
            "timed out waiting for {needle:?}; screen:\n{}",
            rows.join("\n")
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_long_table_paged_past_its_first_screen_keeps_its_header() {
    let mut pty = Pty::start("table-pager");
    pty.wait_for("file 1");
    // Page Down three times, then a line more.
    for key in ["\x1b[6~", "\x1b[6~", "\x1b[6~", "j"] {
        pty.send(key);
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    // 23 rows of content: three of header, twenty of rows a page.
    let rows = wait_for_row(&pty, "file 62 ");
    let top = rows
        .iter()
        .position(|row| row.contains("Name"))
        .unwrap_or_else(|| panic!("no header on screen:\n{}", rows.join("\n")));
    assert_eq!(top, 1, "the header on top:\n{}", rows.join("\n"));
    assert!(rows[0].starts_with('┏'), "{}", rows.join("\n"));
    assert!(rows[2].starts_with('┡'), "{}", rows.join("\n"));
    assert!(rows[3].contains("file 62 "), "{}", rows.join("\n"));
    assert!(!rows.iter().any(|row| row.contains("file 1 ")));
    pty.send("q");
    let (output, parser) = pty.finish();
    assert!(output.contains("OUTCOME Ok(Done(()))"), "{output}");
    assert_restored(&output, &parser);
}

#[test]
fn a_wide_table_scrolls_across_with_its_first_column_frozen() {
    let mut pty = Pty::start("wide-table");
    pty.wait_for("ready");
    let rows = wait_for_row(&pty, "column-0");
    assert!(
        rows.iter().any(|row| row.ends_with('›')),
        "{}",
        rows.join("\n")
    );
    // Shift+End: the last column, after the frozen first one.
    pty.send("\x1b[1;2F");
    let rows = wait_for_row(&pty, "column-11");
    let heading = rows.iter().find(|row| row.contains("column-11")).unwrap();
    assert!(heading.starts_with("‹ column-0 "), "{}", rows.join("\n"));
    let row = rows.iter().find(|row| row.contains("r0c11")).unwrap();
    assert!(row.contains("r0c0-value"), "{}", rows.join("\n"));
    assert_eq!(
        heading
            .find("column-11")
            .map(|at| heading[..at].chars().count()),
        row.find("r0c11").map(|at| row[..at].chars().count()),
        "the heading over its column"
    );
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(output.contains("OUTCOME Ok(Done(0))"), "{output}");
    assert_restored(&output, &parser);
}

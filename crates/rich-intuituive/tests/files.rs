//! The Yazi-style file manager (examples/files.rs), on a directory made for
//! each test.

mod common;

#[path = "../examples/files.rs"]
#[allow(dead_code)]
mod files;

use std::path::{Path, PathBuf};

use common::{row_of, run, run_open, screen};
use files::files_app;
use intuituive::interact::{Button, MouseKind};
use intuituive::App;
use rich_interact::headless::Script;

/// root/
///   alpha/a.txt  beta/  .hidden  notes.md  data.bin  z.txt
fn tree(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("intuituive-files-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("alpha")).unwrap();
    std::fs::create_dir_all(root.join("beta")).unwrap();
    std::fs::write(root.join("alpha/a.txt"), "in alpha").unwrap();
    std::fs::write(root.join(".hidden"), "secret").unwrap();
    std::fs::write(root.join("notes.md"), "# Title\n\nhello notes\n").unwrap();
    std::fs::write(root.join("data.bin"), [0u8, 1, 2, 3, 0, 9]).unwrap();
    std::fs::write(root.join("z.txt"), "zzz ".repeat(300)).unwrap();
    root.canonicalize().unwrap()
}

fn app(root: &Path) -> App {
    files_app(root.to_path_buf()).wait_for_tasks(true)
}

/// The names in the middle column (a table): rows between its header row
/// and the status line, without the size column.
fn middle(rows: &[String], from: usize, to: usize) -> Vec<String> {
    rows[2..rows.len() - 1]
        .iter()
        .map(|r| {
            let cell: String = r.chars().skip(from).take(to - from).collect();
            cell.split("  ").next().unwrap_or("").trim_end().to_string()
        })
        .collect()
}

#[test]
fn it_lists_directories_first_and_previews_the_selection() {
    let root = tree("list");
    let rows = screen(&run(app(&root), Script::new().keys("q"), 80, 10));
    // Columns in split panes: parent 10, divider, current 39, divider,
    // preview 29.
    let current = middle(&rows, 11, 50);
    assert_eq!(
        &current[..5],
        ["alpha/", "beta/", "data.bin", "notes.md", "z.txt"]
    );
    assert!(!rows.join("\n").contains(".hidden"));
    // alpha is selected: its contents are previewed.
    assert!(rows[1].ends_with("a.txt"), "{rows:?}");
    assert!(rows.last().unwrap().contains("1/5"), "{rows:?}");
}

#[test]
fn moving_previews_text_with_highlighting_and_binary_as_a_note() {
    let root = tree("preview");
    let rows = screen(&run(app(&root), Script::new().keys("j j j q"), 80, 10));
    assert!(row_of(&rows, "# Title").is_some(), "notes.md: {rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("j j q"), 80, 10));
    assert!(
        row_of(&rows, "binary file, 6B").is_some(),
        "data.bin: {rows:?}"
    );
}

#[test]
fn opening_and_going_up_keep_the_place() {
    let root = tree("nav");
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    // Into alpha: the header shows it, the parent column lists root.
    let rows = screen(&run(app(&root), Script::new().keys("l q"), 80, 10));
    assert!(rows[0].ends_with(&format!("{name}/alpha")), "{rows:?}");
    assert!(middle(&rows, 11, 50)[0].starts_with("a.txt"), "{rows:?}");
    assert!(rows[1].starts_with("alpha/"), "parent column: {rows:?}");
    // Back up: alpha is selected again.
    let rows = screen(&run(app(&root), Script::new().keys("j l h q"), 80, 10));
    assert!(
        rows.last().unwrap().contains("2/5"),
        "beta stays selected: {rows:?}"
    );
}

#[test]
fn a_long_path_keeps_the_header_to_one_row() {
    let root = tree("long-path-to-show-that-the-header-stays-on-one-row-however-deep");
    let rows = screen(&run(app(&root), Script::new().keys("q"), 40, 8));
    assert!(rows[0].starts_with('…'), "{rows:?}");
    assert!(
        rows[0].ends_with("however-deep-") || rows[0].contains("however-deep"),
        "{rows:?}"
    );
    // The listing starts on the second row, under the table's header.
    assert!(rows[1].contains("Name"), "{rows:?}");
    assert!(rows[2].contains("alpha/"), "{rows:?}");
}

#[test]
fn hidden_files_sorting_and_filtering() {
    let root = tree("options");
    let rows = screen(&run(app(&root), Script::new().keys(". q"), 80, 10));
    assert!(row_of(&rows, ".hidden").is_some(), "{rows:?}");
    assert!(rows.last().unwrap().contains("hidden shown"), "{rows:?}");

    // By size: z.txt (1200 bytes) before notes.md and data.bin.
    let rows = screen(&run(app(&root), Script::new().keys("s q"), 80, 10));
    let current = middle(&rows, 11, 50);
    assert_eq!(
        &current[2..5],
        ["z.txt", "notes.md", "data.bin"],
        "{rows:?}"
    );

    let script = Script::new().keys("/").text("not").keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    let current = middle(&rows, 11, 50);
    assert_eq!(current[0], "notes.md", "{rows:?}");
    assert!(current[1].is_empty(), "{rows:?}");
    assert!(rows.last().unwrap().contains("filter “not”"), "{rows:?}");
}

#[test]
fn tabs_open_switch_and_close() {
    let root = tree("tabs");
    // A tab in alpha: the strip shows both, numbered.
    let rows = screen(&run(app(&root), Script::new().keys("t l q"), 80, 10));
    assert!(
        rows[1].contains("1 ") && rows[1].contains("2 alpha"),
        "{rows:?}"
    );
    assert!(rows[0].ends_with("/alpha"), "{rows:?}");
    // Back to the first tab: its own directory.
    let rows = screen(&run(app(&root), Script::new().keys("t l 1 q"), 80, 10));
    assert!(!rows[0].ends_with("/alpha"), "{rows:?}");
    // Closing the second leaves one tab and no strip.
    let rows = screen(&run(app(&root), Script::new().keys("t l ctrl+w q"), 80, 10));
    assert!(!rows[0].ends_with("/alpha"), "{rows:?}");
    assert!(rows[1].contains("Name"), "no strip: {rows:?}");
}

#[test]
fn the_preview_scrolls_and_the_filter_opens_above_the_status_line() {
    let root = tree("scrolling");
    let long: String = (0..60).map(|i| format!("line {i}\n")).collect();
    std::fs::write(root.join("z.txt"), long).unwrap();
    // z.txt is last: select it, then scroll its preview by 5 twice.
    let rows = screen(&run(app(&root), Script::new().keys("G J J q"), 80, 10));
    assert!(row_of(&rows, "line 10").is_some(), "{rows:?}");
    assert!(row_of(&rows, "line 0 ").is_none(), "{rows:?}");

    let rows = screen(&common::run_open(
        app(&root),
        Script::new().keys("/"),
        80,
        10,
    ));
    // The filter box ends on the row above the status line.
    assert!(rows[8].contains('╰'), "{rows:?}");
    assert!(rows[6].contains("Filter"), "{rows:?}");
}

#[test]
fn a_click_selects_a_row() {
    let root = tree("click");
    // Rows: header 0, table header 1, alpha 2, beta 3, data.bin 4.
    let script = Script::new().click(20, 4).keys("q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(rows.last().unwrap().contains("3/5"), "{rows:?}");
}

#[test]
fn y_yanks_the_path_and_a_toast_says_so() {
    let root = tree("yank");
    let record = run(app(&root), Script::new().keys("y q"), 80, 10);
    assert_eq!(record.copies, [root.join("alpha").display().to_string()]);
    let rows = screen(&record);
    assert!(row_of(&rows, "Copied").is_some(), "{rows:?}");
}

#[test]
fn a_right_click_opens_a_menu_of_what_can_be_done_here() {
    let root = tree("menu");
    // On beta's row of the current column.
    let script = Script::new().mouse(MouseKind::Down(Button::Right), 20, 3);
    let rows = screen(&run_open(app(&root), script, 80, 14));
    for item in ["Open", "Yank path", "Show hidden files", "New tab here"] {
        assert!(row_of(&rows, item).is_some(), "{item}: {rows:?}");
    }
    // Right-clicking selected the row first.
    assert!(rows.last().unwrap().contains("2/5"), "{rows:?}");
}

#[test]
fn the_help_is_made_from_the_bindings() {
    let root = tree("help");
    let app = app(&root).help_key("?");
    let rows = screen(&run_open(app, Script::new().keys("?"), 80, 24));
    for line in [
        "yank the path to the clipboard",
        "open a tab here",
        "go home",
    ] {
        assert!(row_of(&rows, line).is_some(), "{line}: {rows:?}");
    }
}

//! The Yazi-style file manager (examples/files.rs), on a directory made for
//! each test.

mod common;

#[path = "../examples/files.rs"]
#[allow(dead_code)]
mod files;

use std::path::{Path, PathBuf};

use common::{row_of, run, screen};
use files::files_app;
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

/// The middle column: rows between the header and the status line.
fn middle(rows: &[String], from: usize, to: usize) -> Vec<String> {
    rows[1..rows.len() - 1]
        .iter()
        .map(|r| {
            r.chars()
                .skip(from)
                .take(to - from)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn it_lists_directories_first_and_previews_the_selection() {
    let root = tree("list");
    let rows = screen(&run(app(&root), Script::new().keys("q"), 80, 10));
    // Columns: parent 9, gap, current 36, gap, preview 27 (1:4:3 of 78).
    let current = middle(&rows, 10, 46);
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
    assert!(middle(&rows, 10, 46)[0].starts_with("a.txt"), "{rows:?}");
    assert!(rows[1].starts_with("alpha/"), "parent column: {rows:?}");
    // Back up: alpha is selected again.
    let rows = screen(&run(app(&root), Script::new().keys("j l h q"), 80, 10));
    assert!(
        rows.last().unwrap().contains("2/5"),
        "beta stays selected: {rows:?}"
    );
}

#[test]
fn hidden_files_sorting_and_filtering() {
    let root = tree("options");
    let rows = screen(&run(app(&root), Script::new().keys(". q"), 80, 10));
    assert!(row_of(&rows, ".hidden").is_some(), "{rows:?}");
    assert!(rows.last().unwrap().contains("hidden shown"), "{rows:?}");

    // By size: z.txt (1200 bytes) before notes.md and data.bin.
    let rows = screen(&run(app(&root), Script::new().keys("s q"), 80, 10));
    let current = middle(&rows, 10, 46);
    assert_eq!(
        &current[2..5],
        ["z.txt", "notes.md", "data.bin"],
        "{rows:?}"
    );

    let script = Script::new().keys("/").text("not").keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    let current = middle(&rows, 10, 46);
    assert_eq!(current[0], "notes.md", "{rows:?}");
    assert!(current[1].is_empty(), "{rows:?}");
    assert!(rows.last().unwrap().contains("filter “not”"), "{rows:?}");
}

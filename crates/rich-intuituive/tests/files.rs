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
    let full = root.canonicalize().unwrap();
    match full.to_string_lossy().strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => full,
    }
}

fn app(root: &Path) -> App {
    files_app(root.to_path_buf()).wait_for_tasks(true)
}

/// The current column's names (columns 11 to 49 at 80 wide), without the
/// line mode at their right.
fn current(rows: &[String]) -> Vec<String> {
    rows[1..rows.len() - 1]
        .iter()
        .map(|r| {
            let cell: String = r.chars().skip(11).take(39).collect();
            cell.trim().split("  ").next().unwrap_or("").to_string()
        })
        .collect()
}

fn status(rows: &[String]) -> &str {
    rows.last().unwrap()
}

#[test]
fn it_lists_directories_first_with_yazis_status_line() {
    let root = tree("list");
    let rows = screen(&run(app(&root), Script::new().keys("q"), 80, 10));
    assert_eq!(
        &current(&rows)[..5],
        ["alpha", "beta", "data.bin", "notes.md", "z.txt"]
    );
    assert!(!rows.join("\n").contains(".hidden"));
    // The rails between the columns.
    assert_eq!(rows[1].chars().nth(10), Some('│'), "{rows:?}");
    assert_eq!(rows[1].chars().nth(50), Some('│'), "{rows:?}");
    // alpha is hovered: its contents are previewed.
    assert!(rows[1].ends_with("a.txt"), "{rows:?}");
    // Mode, size, name; permissions, place, position.
    let line = status(&rows);
    assert!(line.starts_with(" NOR "), "{line:?}");
    assert!(line.contains(" alpha"), "{line:?}");
    assert!(
        line.contains("Top") && line.trim_end().ends_with("1/5"),
        "{line:?}"
    );
    #[cfg(unix)]
    assert!(line.contains("drwx"), "{line:?}");
}

#[test]
fn moving_previews_text_highlighted_and_binary_as_a_note() {
    let root = tree("preview");
    let rows = screen(&run(app(&root), Script::new().keys("j j j q"), 80, 10));
    assert!(row_of(&rows, "# Title").is_some(), "notes.md: {rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("j j q"), 80, 10));
    assert!(row_of(&rows, "Binary data, 6B").is_some(), "{rows:?}");
    // G is the last, gg (a chord) the first again.
    let rows = screen(&run(app(&root), Script::new().keys("G q"), 80, 10));
    assert!(
        status(&rows).contains("Bot") && status(&rows).contains("5/5"),
        "{rows:?}"
    );
    let rows = screen(&run(app(&root), Script::new().keys("G g g q"), 80, 10));
    assert!(status(&rows).contains("1/5"), "{rows:?}");
}

#[test]
fn a_chord_shows_what_can_follow_it() {
    let root = tree("which");
    let rows = screen(&run_open(app(&root), Script::new().text(","), 100, 14));
    for cand in ["Sort by size", "Sort naturally", "Sort randomly"] {
        assert!(row_of(&rows, cand).is_some(), "{cand}: {rows:?}");
    }
    // `,s` sorts by size, smallest first, and shows the sizes.
    let rows = screen(&run(app(&root), Script::new().text(",s").keys("q"), 80, 10));
    assert_eq!(
        &current(&rows)[2..5],
        ["data.bin", "notes.md", "z.txt"],
        "{rows:?}"
    );
    assert!(rows[5].contains("1.2K│"), "{rows:?}");
}

#[test]
fn entering_and_leaving_keep_the_place() {
    let root = tree("nav");
    let name = root.file_name().unwrap().to_string_lossy().into_owned();
    let sep = std::path::MAIN_SEPARATOR;
    let rows = screen(&run(app(&root), Script::new().keys("l q"), 80, 10));
    assert!(rows[0].ends_with(&format!("{name}{sep}alpha")), "{rows:?}");
    assert!(!rows[0].contains(r"\\?\"), "{rows:?}");
    assert_eq!(current(&rows)[0], "a.txt", "{rows:?}");
    assert!(rows[1].starts_with("  alpha"), "parent column: {rows:?}");
    // Back up: beta is hovered again; H and L walk the history.
    let rows = screen(&run(app(&root), Script::new().keys("j l h q"), 80, 10));
    assert!(status(&rows).contains("2/5"), "{rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("l H q"), 80, 10));
    assert!(rows[0].ends_with(&name), "{rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("l H L q"), 80, 10));
    assert!(rows[0].ends_with("alpha"), "{rows:?}");
}

#[test]
fn a_long_path_keeps_the_header_to_one_row() {
    let root = tree("long-path-to-show-that-the-header-stays-on-one-row-however-deep");
    let rows = screen(&run(app(&root), Script::new().keys("q"), 40, 8));
    assert!(rows[0].starts_with('…'), "{rows:?}");
    assert!(rows[0].contains("however-deep"), "{rows:?}");
    assert!(rows[1].contains("alpha"), "{rows:?}");
}

#[test]
fn hidden_files_and_the_filter() {
    let root = tree("options");
    let rows = screen(&run(app(&root), Script::new().keys(". q"), 80, 10));
    assert!(row_of(&rows, ".hidden").is_some(), "{rows:?}");

    // The filter applies as it is typed; the header says so.
    let script = Script::new().keys("f").text("not").keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert_eq!(current(&rows)[0], "notes.md", "{rows:?}");
    assert!(current(&rows)[1].is_empty(), "{rows:?}");
    assert!(rows[0].ends_with("(filter: not)"), "{rows:?}");
    // Esc clears it.
    let script = Script::new().keys("f").text("not").keys("enter esc q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert_eq!(current(&rows)[1], "beta", "{rows:?}");
    // Nothing left: a centred note.
    let script = Script::new()
        .keys("f")
        .text("nothing-like-it")
        .keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(row_of(&rows, "No filter results").is_some(), "{rows:?}");
}

#[test]
fn find_jumps_as_it_is_typed_and_n_goes_on() {
    let root = tree("find");
    // "e" is in beta and notes.md: the first from the cursor, with its
    // place among the matches.
    let script = Script::new().keys("/").text("e").keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(status(&rows).contains("beta"), "{rows:?}");
    assert!(rows[0].ends_with("(find: e)"), "{rows:?}");
    assert!(rows[2].contains("beta  [1/2]"), "{rows:?}");
    let script = Script::new().keys("/").text("e").keys("enter n q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(status(&rows).contains("notes.md"), "{rows:?}");
    let script = Script::new().keys("/").text("e").keys("enter n n q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(status(&rows).contains("beta"), "wraps: {rows:?}");
    let script = Script::new().keys("/").text("e").keys("enter N q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(
        status(&rows).contains("notes.md"),
        "back, wrapping: {rows:?}"
    );
}

#[test]
fn tabs_open_switch_and_close() {
    let root = tree("tabs");
    let alpha = format!("{}alpha", std::path::MAIN_SEPARATOR);
    // `t t` opens a tab: the strip shows both, numbered.
    let rows = screen(&run(app(&root), Script::new().keys("t t l q"), 80, 10));
    assert!(
        rows[1].contains(" 1 ") && rows[1].contains(" 2 alpha"),
        "{rows:?}"
    );
    assert!(rows[0].ends_with(&alpha), "{rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("t t l 1 q"), 80, 10));
    assert!(!rows[0].ends_with(&alpha), "{rows:?}");
    // Ctrl+C closes a tab, and quits on the last.
    let rows = screen(&run(
        app(&root),
        Script::new().keys("t t l ctrl+c q"),
        80,
        10,
    ));
    assert!(!rows[0].ends_with(&alpha), "{rows:?}");
    assert!(rows[1].contains("alpha"), "no strip: {rows:?}");
    run(app(&root), Script::new().keys("ctrl+c"), 80, 10);
}

#[test]
fn selecting_marks_rows_and_the_header_counts_them() {
    let root = tree("select");
    let rows = screen(&run(
        app(&root),
        Script::new().keys("space space q"),
        80,
        10,
    ));
    assert!(rows[0].trim_end().ends_with(" 2"), "{rows:?}");
    // Visual mode: SEL, and its range counts once it ends.
    let rows = screen(&run(app(&root), Script::new().keys("v j j q"), 80, 10));
    assert!(status(&rows).starts_with(" SEL "), "{rows:?}");
    let rows = screen(&run(app(&root), Script::new().keys("v j j esc q"), 80, 10));
    assert!(status(&rows).starts_with(" NOR "), "{rows:?}");
    assert!(rows[0].trim_end().ends_with(" 3"), "{rows:?}");
    // Esc again clears the selection.
    let rows = screen(&run(
        app(&root),
        Script::new().keys("v j j esc esc q"),
        80,
        10,
    ));
    assert!(!rows[0].trim_end().ends_with(" 3"), "{rows:?}");
}

#[test]
fn visual_mode_ends_cleanly_when_a_filter_leaves_nothing() {
    let root = tree("visual-empty");
    let script = Script::new()
        .keys("v j j f")
        .text("nothing-like-it")
        .keys("enter esc q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(status(&rows).starts_with(" NOR "), "{rows:?}");
}

#[test]
fn a_tab_click_switches_as_the_keys_do() {
    let root = tree("tab-click");
    let rows = screen(&run(app(&root), Script::new().keys("t t q"), 80, 10));
    let x = rows[1].find(" 1 ").expect("tab 1 in the strip") as u16 + 1;
    // In visual mode in tab 2, a click on tab 1 ends it, as `1` would.
    let script = Script::new()
        .keys("t t v j")
        .mouse(MouseKind::Down(Button::Left), x, 1)
        .mouse(MouseKind::Up(Button::Left), x, 1)
        .keys("q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(status(&rows).starts_with(" NOR "), "{rows:?}");
}

#[test]
fn copying_paths_puts_them_on_the_clipboard() {
    let root = tree("copy");
    let record = run(app(&root), Script::new().keys("c c c f q"), 80, 10);
    assert_eq!(
        record.copies,
        [
            root.join("alpha").display().to_string(),
            "alpha".to_string()
        ]
    );
}

#[test]
fn yank_and_paste_copy_under_a_free_name() {
    let root = tree("paste");
    // notes.md yanked, pasted in alpha and here (as notes_1.md).
    let script = Script::new().keys("j j j y k k k l p h p q");
    run(app(&root), script, 80, 10);
    assert!(root.join("alpha/notes.md").exists());
    assert!(root.join("notes_1.md").exists());
    assert!(root.join("notes.md").exists());
    // Cut and paste moves it.
    let script = Script::new().keys("G x g g l p q");
    run(app(&root), script, 80, 10);
    assert!(root.join("alpha/z.txt").exists());
    assert!(!root.join("z.txt").exists());
}

#[test]
fn trash_and_delete_ask_first() {
    let root = tree("remove");
    // d asks; n keeps the file.
    let rows = screen(&run_open(app(&root), Script::new().keys("G d"), 80, 24));
    assert!(
        row_of(&rows, "Trash 1 selected file?").is_some(),
        "{rows:?}"
    );
    assert!(row_of(&rows, "[Y]es").is_some(), "{rows:?}");
    run(app(&root), Script::new().keys("G d n q"), 80, 24);
    assert!(root.join("z.txt").exists());
    // y moves it to .trash in the directory the app started in.
    run(app(&root), Script::new().keys("G d y q"), 80, 24);
    assert!(!root.join("z.txt").exists());
    assert!(root.join(".trash/z.txt").exists());
    // D deletes for good, a directory and all in it too.
    let rows = screen(&run_open(app(&root), Script::new().keys("D"), 80, 24));
    assert!(
        row_of(&rows, "Permanently delete 1 selected file?").is_some(),
        "{rows:?}"
    );
    run(app(&root), Script::new().keys("D y q"), 80, 24);
    assert!(!root.join("alpha").exists());
    // The trash itself is never moved into itself.
    let script = Script::new().keys(". f").text(".trash").keys("enter d y q");
    run(app(&root), script, 80, 24);
    assert!(root.join(".trash/z.txt").exists());
    assert!(!root.join(".trash/.trash").exists());
}

#[test]
fn create_and_rename_stay_inside_the_directory() {
    let root = tree("create");
    let rows = screen(&run_open(app(&root), Script::new().keys("a"), 80, 10));
    assert!(row_of(&rows, "Create:").is_some(), "{rows:?}");
    run(
        app(&root),
        Script::new().keys("a").text("new.txt").keys("enter q"),
        80,
        10,
    );
    assert!(root.join("new.txt").is_file());
    run(
        app(&root),
        Script::new().keys("a").text("made/").keys("enter q"),
        80,
        10,
    );
    assert!(root.join("made").is_dir());
    // Out of it is refused.
    let script = Script::new()
        .keys("a")
        .text("../escaped.txt")
        .keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(!root.parent().unwrap().join("escaped.txt").exists());
    assert!(row_of(&rows, "inside this directory").is_some(), "{rows:?}");
    // r renames the hovered entry, starting from its name (the prompt's
    // caret starts at its start).
    let script = Script::new()
        .keys("r end backspace backspace backspace backspace backspace")
        .text("gamma")
        .keys("enter q");
    let rows = screen(&run(app(&root), script, 80, 10));
    assert!(root.join("gamma").is_dir(), "{rows:?}");
    assert!(!root.join("alpha").exists());
}

#[test]
fn clicks_hover_and_the_parent_column_goes_there() {
    let root = tree("click");
    // Rows: header 0, then alpha 1, beta 2, data.bin 3.
    let rows = screen(&run(
        app(&root),
        Script::new().click(20, 3).keys("q"),
        80,
        10,
    ));
    assert!(status(&rows).contains("3/5"), "{rows:?}");
    // In alpha, a click on its parent's beta goes to the root, beta hovered.
    let rows = screen(&run(
        app(&root),
        Script::new().keys("l").click(3, 2).keys("q"),
        80,
        10,
    ));
    assert!(status(&rows).contains("beta"), "{rows:?}");
    // A click on an entry of a directory's preview enters it.
    let rows = screen(&run(
        app(&root),
        Script::new().click(60, 1).keys("q"),
        80,
        10,
    ));
    assert!(rows[0].ends_with("alpha"), "{rows:?}");
}

#[test]
fn a_right_click_opens_a_menu_of_what_can_be_done_here() {
    let root = tree("menu");
    let script = Script::new().mouse(MouseKind::Down(Button::Right), 20, 2);
    let rows = screen(&run_open(app(&root), script, 80, 20));
    for item in ["Open", "Yank", "Copy path", "Trash", "Show hidden files"] {
        assert!(row_of(&rows, item).is_some(), "{item}: {rows:?}");
    }
    // It hovered the row first.
    assert!(status(&rows).contains("2/5"), "{rows:?}");
    // Below the rows it opens nothing.
    let script = Script::new().mouse(MouseKind::Down(Button::Right), 20, 8);
    let rows = screen(&run_open(app(&root), script, 80, 20));
    assert!(row_of(&rows, "Copy path").is_none(), "{rows:?}");
}

#[test]
fn the_help_lists_every_key_and_filters() {
    let root = tree("help");
    let rows = screen(&run_open(app(&root), Script::new().keys("~"), 100, 30));
    assert!(row_of(&rows, "mgr.help").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Previous file").is_some(), "{rows:?}");
    let script = Script::new().keys("f1 /").text("tab");
    let rows = screen(&run_open(app(&root), script, 100, 30));
    assert!(
        row_of(&rows, "Create a new tab in CWD").is_some(),
        "{rows:?}"
    );
    assert!(row_of(&rows, "Previous file").is_none(), "{rows:?}");
}

#[test]
fn it_fits_small_and_large_screens() {
    let root = tree("sizes");
    for (w, h) in [(80, 24), (100, 32), (200, 60), (40, 10)] {
        let rows = screen(&run(app(&root), Script::new().keys("q"), w, h));
        assert_eq!(rows.len(), h as usize, "{w}x{h}");
        assert!(status(&rows).starts_with(" NOR "), "{w}x{h}: {rows:?}");
        assert!(rows.iter().all(|r| r.chars().count() <= w as usize));
    }
}

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
    // The line mode ends with Yazi's padding cell.
    assert!(rows[5].contains("1.2K │"), "{rows:?}");
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
    assert!(row_of(&rows, "must stay inside this").is_some(), "{rows:?}");
    // r renames the hovered entry, starting from its name, the caret at
    // its end (a directory's name has no extension).
    let script = Script::new()
        .keys("r backspace backspace backspace backspace backspace")
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

#[test]
fn rename_puts_the_caret_before_the_extension() {
    let root = tree("rename-ext");
    // notes.md: typing goes before ".md", as Yazi's `--cursor=before_ext`.
    let script = Script::new().keys("j j j r").text("-v2").keys("enter q");
    run(app(&root), script, 80, 10);
    assert!(root.join("notes-v2.md").is_file());
    assert!(!root.join("notes.md").exists());
}

#[test]
fn the_rename_prompt_opens_below_the_hovered_row() {
    let root = tree("rename-at");
    // beta is on row 2: the prompt's box starts on row 3.
    let rows = screen(&run_open(app(&root), Script::new().keys("j r"), 100, 14));
    assert_eq!(row_of(&rows, "Rename:"), Some(3), "{rows:?}");
}

/// The program `$EDITOR` names, as the example reads it.
fn editor() -> String {
    files::editor().get_program().to_string_lossy().into_owned()
}

#[test]
fn enter_hands_the_terminal_to_the_editor_as_yazi_does() {
    let root = tree("open");
    // notes.md is text: Enter (and o) open it in $EDITOR.
    let record = run(app(&root), Script::new().keys("j j j enter o q"), 80, 10);
    assert_eq!(record.handoffs, [editor(), editor()]);
    // Yazi's rules give a directory to the editor too.
    let record = run(app(&root), Script::new().keys("enter q"), 80, 10);
    assert_eq!(record.handoffs, [editor()]);
}

#[test]
fn open_interactively_offers_the_openers_of_the_files_type() {
    let root = tree("open-with");
    let rows = screen(&run_open(app(&root), Script::new().keys("j j j O"), 80, 14));
    let at = row_of(&rows, "Open with:").expect("the pick");
    // Below notes.md (row 4).
    assert_eq!(at, 5, "{rows:?}");
    assert!(rows[at + 1].contains("$EDITOR"), "{rows:?}");
    assert!(rows[at + 2].contains("Reveal"), "{rows:?}");
    let record = run(app(&root), Script::new().keys("j j j O enter q"), 80, 14);
    assert_eq!(record.handoffs, [editor()]);
    assert_eq!(files::openers_for("image/png")[0], files::Opener::Open);
    assert_eq!(files::openers_for("video/mp4")[0], files::Opener::Play);
}

#[test]
fn line_modes_show_local_times_and_owner_names() {
    let root = tree("linemode");
    let mtime = std::fs::metadata(root.join("alpha"))
        .unwrap()
        .modified()
        .unwrap();
    let rows = screen(&run(app(&root), Script::new().keys("m m q"), 80, 10));
    let shown = rich_ext::fs::short_local_time(mtime);
    assert!(rows[1].contains(&format!("{shown} │")), "{shown}: {rows:?}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(root.join("alpha")).unwrap();
        let user = rich_ext::fs::user_name(meta.uid()).unwrap_or(meta.uid().to_string());
        let rows = screen(&run(app(&root), Script::new().keys("m o q"), 80, 10));
        assert!(rows[1].contains(&format!("{user}:")), "{user}: {rows:?}");
    }
}

#[test]
fn errors_are_notifications_at_the_top_right() {
    let root = tree("notify");
    let script = Script::new().keys("a").text("../out").keys("enter");
    let rows = screen(&run_open(app(&root), script, 100, 14));
    let at = row_of(&rows, "must stay").expect("the notification");
    assert!(at <= 2, "{rows:?}");
    let title = row_of(&rows, "Create").expect("its title");
    assert!(rows[title].find("Create").unwrap() > 50, "{rows:?}");
}

#[test]
fn a_sequence_shows_what_can_follow_and_esc_cancels_it() {
    let root = tree("sequence");
    let rows = screen(&run_open(app(&root), Script::new().keys("g"), 100, 14));
    assert!(row_of(&rows, "Go home").is_some(), "{rows:?}");
    // Esc cancels: j then moves as usual.
    let rows = screen(&run(app(&root), Script::new().keys("g esc j q"), 80, 10));
    assert!(status(&rows).contains("2/5"), "{rows:?}");
    assert!(row_of(&rows, "Go home").is_none(), "{rows:?}");
}

#[test]
fn overwriting_asks_first() {
    let root = tree("overwrite");
    std::fs::write(root.join("alpha/notes.md"), "old").unwrap();
    // notes.md yanked, P in alpha, where one has the name.
    let rows = screen(&run_open(
        app(&root),
        Script::new().keys("j j j y k k k l P"),
        80,
        24,
    ));
    assert!(row_of(&rows, "Overwrite file?").is_some(), "{rows:?}");
    assert!(
        row_of(&rows, "Will overwrite the following file:").is_some(),
        "{rows:?}"
    );
    run(
        app(&root),
        Script::new().keys("j j j y k k k l P n q"),
        80,
        24,
    );
    assert_eq!(
        std::fs::read_to_string(root.join("alpha/notes.md")).unwrap(),
        "old"
    );
    run(
        app(&root),
        Script::new().keys("j j j y k k k l P y q"),
        80,
        24,
    );
    assert!(std::fs::read_to_string(root.join("alpha/notes.md"))
        .unwrap()
        .contains("hello notes"));
}

#[cfg(feature = "watch")]
#[test]
fn it_follows_the_disk() {
    let root = tree("watch");
    let mut driver = app(&root).driver(80, 10);
    driver.settle();
    std::fs::write(root.join("added.txt"), "new").unwrap();
    let mut seen = false;
    for _ in 0..200 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        driver.settle();
        if driver
            .screen()
            .plain()
            .iter()
            .any(|r| r.contains("added.txt"))
        {
            seen = true;
            break;
        }
    }
    assert!(seen, "{:?}", driver.screen().plain());
}

#[cfg(feature = "art")]
#[test]
fn images_are_drawn_in_the_preview() {
    let root = tree("image");
    rich_art::image::RgbImage::from_pixel(8, 8, rich_art::image::Rgb([255, 0, 0]))
        .save(root.join("a.png"))
        .unwrap();
    // a.png is after the directories: alpha, beta, a.png. It is decoded
    // on a worker, a task the app waits for like the preview's own.
    let mut driver = app(&root).driver(80, 10);
    driver.press("j j");
    let rows = driver.screen().plain();
    assert!(status(&rows).contains("a.png"), "{rows:?}");
    assert!(rows[1].contains('▀'), "{rows:?}");
}

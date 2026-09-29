//! TextArea, FilePicker, ColorPicker and AssetPicker (#493), headless.

use std::path::{Path, PathBuf};

use rich_interact::headless::{self, Script};
use rich_interact::policy::{Fallback, Reason, ScriptedLineIo};
use rich_interact::{
    degrade, AssetKind, AssetPicker, ColorFormat, ColorPicker, Component, Confirm, Context, Error,
    Event, FileMode, FilePicker, Input, Key, NotInteractive, Outcome, PreviewLayout, Select,
    TextArea,
};

/// The last view before the component collapsed to its answer.
fn before_answer(record: &headless::Record) -> &str {
    &record.frames[record.frames.len() - 2]
}

// ---- TextArea ----

#[test]
fn text_area_takes_lines_and_submits_with_ctrl_d() {
    let script = Script::new()
        .text("first")
        .keys("enter")
        .text("second")
        .keys("ctrl+d");
    let (outcome, record) = headless::run(TextArea::new("Notes"), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("first\nsecond".into()));
    let editing = before_answer(&record);
    assert!(editing.contains("│ first\n│ second"), "{editing}");
    assert!(editing.contains("ctrl+d submit"), "{editing}");
    assert_eq!(record.last_frame(), "? Notes › first … (+1 lines)");
}

#[test]
fn text_area_edits_across_lines() {
    // "ab|" + enter + "cd", then up, end, backspace, down, home, x.
    let script = Script::new()
        .text("ab")
        .keys("enter")
        .text("cd")
        .keys("up end backspace down home")
        .text("x")
        .keys("ctrl+d");
    let (outcome, _) = headless::run(TextArea::new("Notes"), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("a\nxcd".into()));
    // Backspace at the start of a line joins it to the one before; Delete
    // at the end joins the next.
    let script = Script::new()
        .text("ab")
        .keys("enter")
        .text("cd")
        .keys("home backspace")
        .keys("ctrl+home end delete ctrl+d");
    let (outcome, _) = headless::run(TextArea::new("Notes"), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("abcd".into()));
}

#[test]
fn text_area_limits_characters_and_shows_a_placeholder() {
    let area = TextArea::new("Bio")
        .placeholder("Say something")
        .char_limit(5);
    let script = Script::new()
        .text("abc")
        .keys("enter")
        .text("defg")
        .keys("ctrl+d");
    let (outcome, record) = headless::run(area, script, 40, 12);
    // Five characters: "abc", the line break, "d".
    assert_eq!(outcome.unwrap(), Outcome::Done("abc\nd".into()));
    assert!(
        record.frames[0].contains("│ Say something"),
        "{}",
        record.frames[0]
    );
    assert!(record.frames[0].contains("0/5"), "{}", record.frames[0]);
    assert!(before_answer(&record).contains("5/5"));
}

#[test]
fn text_area_wraps_and_scrolls_to_the_caret() {
    let area = TextArea::new("Notes").height(2).line_numbers(true);
    let script = Script::new()
        .text("one")
        .keys("enter")
        .text("two")
        .keys("enter")
        .text("three")
        .keys("ctrl+d");
    let (_, record) = headless::run(area, script, 40, 12);
    let last = before_answer(&record);
    assert!(last.contains("2 │ two\n3 │ three"), "{last}");
    assert!(
        !last.contains("1 │ one"),
        "scrolled past the first line:\n{last}"
    );
    // A line longer than the space wraps, numbered once.
    let area = TextArea::new("N")
        .line_numbers(true)
        .value("abcdefghijklmnop");
    let (_, record) = headless::run(area, Script::new().keys("ctrl+d"), 14, 12);
    let first = &record.frames[0];
    // 14 columns: a 4-cell gutter and a cell for the caret leave 9.
    assert!(first.contains("1 │ abcdefghi\n  │ jklmnop"), "{first}");
}

#[test]
fn text_area_submit_key_escape_and_paste() {
    let area = TextArea::new("Msg").submit_key(Key::parse("ctrl+s").unwrap());
    let script = Script::new()
        .event(Event::Paste("a\r\nb\x1b[2J\tc".into()))
        .keys("ctrl+s");
    let (outcome, _) = headless::run(area, script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("a\nb[2J\tc".into()));
    let (outcome, record) =
        headless::run(TextArea::new("Msg"), Script::new().keys("x esc"), 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    assert_eq!(record.last_frame(), "? Msg › cancelled");
}

#[test]
fn text_area_degrades_to_every_line_of_input() {
    let mut io = ScriptedLineIo::new(["one", "two"]);
    let mut area = TextArea::new("Notes");
    let outcome = degrade(
        &mut area,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("one\ntwo".into()));
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let mut area = TextArea::new("Notes").value("kept");
    let outcome = degrade(
        &mut area,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("kept".into()));
}

// ---- FilePicker ----

/// A directory tree to browse:
/// `a.rs`, `b.txt`, `.hidden`, `src/lib.rs`, `src/deep/x.md`.
fn tree() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("a.rs"), "fn main() {}\n").unwrap();
    std::fs::write(root.join("b.txt"), "hello\x1b[2J world\n").unwrap();
    std::fs::write(root.join(".hidden"), "secret").unwrap();
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    std::fs::write(root.join("src/deep/x.md"), "# x\n").unwrap();
    dir
}

#[test]
fn file_picker_lists_directories_first_and_picks_a_file() {
    let dir = tree();
    let picker = FilePicker::new("File", dir.path());
    assert_eq!(picker.labels(), ["..", "src/", "a.rs", "b.txt"]);
    // The first entry is focused, not `..`: Enter opens src/; then lib.rs.
    let script = Script::new().keys("enter").text("lib").keys("enter");
    let (outcome, record) = headless::run(picker, script, 100, 16);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(dir.path().join("src").join("lib.rs"))
    );
    let frames = record.frames.join("\n");
    assert!(frames.contains("deep/"), "{frames}");
    let inside = before_answer(&record);
    // The file's preview is beside it.
    assert!(inside.contains("pub fn f() {}"), "{inside}");
}

#[test]
fn file_picker_goes_up_and_toggles_hidden_files() {
    let dir = tree();
    // Into src/, then left back up: src/ is focused again.
    let script = Script::new().keys("right left enter");
    let picker = FilePicker::new("File", dir.path()).mode(FileMode::Both);
    let (outcome, _) = headless::run(picker, script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join("src")));
    let script = Script::new().keys("ctrl+t").text("hidden").keys("enter");
    let (outcome, _) = headless::run(FilePicker::new("File", dir.path()), script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join(".hidden")));
    let picker = FilePicker::new("File", dir.path()).show_hidden(true);
    assert!(picker.labels().contains(&".hidden".to_string()));
}

#[test]
fn file_picker_modes_and_extensions() {
    let dir = tree();
    let dirs = FilePicker::new("Dir", dir.path()).mode(FileMode::Directory);
    assert_eq!(dirs.labels(), ["..", "src/"]);
    let rust = FilePicker::new("Rust", dir.path()).extensions([".RS"]);
    assert_eq!(rust.labels(), ["..", "src/", "a.rs"]);
    // Directory mode picks a directory on Enter.
    let (outcome, _) = headless::run(dirs, Script::new().keys("enter"), 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join("src")));
}

#[test]
fn file_picker_shows_controls_in_names_and_content_as_text() {
    let dir = tree();
    let script = Script::new().text("b.txt").keys("esc");
    let (_, record) = headless::run(FilePicker::new("File", dir.path()), script, 100, 16);
    let frames = record.frames.join("\n");
    assert!(frames.contains("hello␛[2J world"), "{frames}");
    assert!(!record.output().contains("\x1b[2J"));
}

#[test]
fn a_root_of_dot_gives_relative_paths() {
    // Tests run in the crate's directory.
    let script = Script::new().text("Cargo.toml").keys("enter");
    let (outcome, _) = headless::run(FilePicker::new("File", "."), script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(PathBuf::from("Cargo.toml")));
    let script = Script::new()
        .text("src")
        .keys("right")
        .text("lib.rs")
        .keys("enter");
    let (outcome, _) = headless::run(FilePicker::new("File", "."), script, 80, 16);
    assert_eq!(
        outcome.unwrap(),
        Outcome::Done(Path::new("src").join("lib.rs"))
    );
}

#[cfg(unix)]
#[test]
fn a_jailed_picker_never_leaves_its_root() {
    let dir = tree();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("passwd"), "root:x:0:0").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("passwd"), dir.path().join("link")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("a.rs"), dir.path().join("inside")).unwrap();
    let jailed = FilePicker::new("File", dir.path()).jail(true);
    // No `..` at the root, no link out; the link within stays.
    assert_eq!(jailed.labels(), ["src/", "a.rs", "b.txt", "inside"]);
    // Left at the root stays there.
    let jailed = jailed.mode(FileMode::Both);
    let (outcome, _) = headless::run(jailed, Script::new().keys("left left enter"), 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join("src")));
    // Unjailed, the links are listed.
    let free = FilePicker::new("File", dir.path());
    assert!(free.labels().contains(&"escape/".to_string()));
}

#[cfg(target_os = "linux")]
#[test]
fn names_that_are_not_utf8_show_escaped_and_return_the_real_path() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    std::fs::write(dir.path().join(name), "x").unwrap();
    let picker = FilePicker::new("File", dir.path());
    assert_eq!(picker.labels(), ["..", "caf\\xE9.txt"]);
    let (outcome, record) = headless::run(picker, Script::new().keys("enter"), 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done(dir.path().join(name)));
    assert!(
        record.last_frame().ends_with("caf\\xE9.txt"),
        "{}",
        record.last_frame()
    );
}

#[cfg(unix)]
#[test]
fn a_fifo_is_listed_but_never_read_for_its_preview() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|status| status.success()) {
        return;
    }
    let script = Script::new().text("pipe").keys("esc");
    let (_, record) = headless::run(FilePicker::new("File", dir.path()), script, 100, 12);
    assert!(record.frames.join("\n").contains("not a regular file"));
}

#[test]
fn without_a_terminal_the_file_picker_answers_its_default_or_nothing() {
    let dir = tree();
    let mut picker = FilePicker::new("File", dir.path()).default("a.rs");
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(PathBuf::from("a.rs")));
    let mut picker = FilePicker::new("File", dir.path());
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(matches!(
        outcome,
        Err(Error::NotInteractive(NotInteractive::NoDefault(_)))
    ));
}

// ---- ColorPicker ----

#[test]
fn color_picker_filters_names_and_shows_a_swatch() {
    let script = Script::new().text("dark_orange").keys("enter");
    let picker = ColorPicker::new("Colour").format(ColorFormat::Name);
    let (outcome, record) = headless::run(picker, script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done("dark_orange".into()));
    let shown = before_answer(&record);
    assert!(shown.contains("#ff8700"), "{shown}");
    // The swatch is painted with the colour as its background.
    assert!(
        record.output().contains("48;2;255;135;0"),
        "{:?}",
        record.output()
    );
}

#[test]
fn color_picker_takes_hex_and_rgb_typed_in() {
    let script = Script::new().text("#FF8800").keys("enter");
    let (outcome, _) = headless::run(
        ColorPicker::new("Colour").format(ColorFormat::Rgb),
        script,
        80,
        16,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("rgb(255,136,0)".into()));
    let script = Script::new().text("rgb(1,2,3)").keys("enter");
    let (outcome, _) = headless::run(ColorPicker::new("Colour"), script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done("#010203".into()));
}

#[test]
fn color_picker_palette_grid() {
    // Tab to the grid, right twice and down once: 16 + 2 = color(18).
    let script = Script::new().keys("tab right right down enter");
    let picker = ColorPicker::new("Colour").format(ColorFormat::Name);
    let (outcome, record) = headless::run(picker, script, 80, 24);
    assert_eq!(outcome.unwrap(), Outcome::Done("dark_blue".into()));
    assert!(before_answer(&record).contains("color(18)"));
}

#[test]
fn color_picker_degrades_to_a_line() {
    let mut io = ScriptedLineIo::new(["#00ff00"]);
    let mut picker = ColorPicker::new("Colour").format(ColorFormat::Name);
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("#00ff00".into()));
    let mut io = ScriptedLineIo::new([""]);
    let mut picker = ColorPicker::new("Colour").default("red");
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("#800000".into()));
    let mut io = ScriptedLineIo::new(["nope"]);
    let outcome = degrade(
        &mut ColorPicker::new("Colour"),
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert!(outcome.is_err());
}

fn degrade_color(
    picker: ColorPicker,
    lines: &[&str],
    fallback: Fallback,
) -> Result<Outcome<String>, Error> {
    let mut io = ScriptedLineIo::new(lines.iter().map(|line| line.to_string()));
    let mut picker = picker;
    degrade(&mut picker, fallback, Reason::StdinNotTerminal, &mut io)
}

#[test]
fn color_picker_rejects_a_default_that_is_not_a_colour() {
    assert!(ColorPicker::is_color("red") && ColorPicker::is_color("#ff8800"));
    assert!(!ColorPicker::is_color("definitely-not-a-color") && !ColorPicker::is_color(""));
    // Input ended, an empty line, or a real answer: the bad default is
    // reported before anything is asked.
    for lines in [&[][..], &[""], &["blue"]] {
        let bad = ColorPicker::new("Colour").default("definitely-not-a-color");
        match degrade_color(bad, lines, Fallback::Prompt) {
            Err(Error::NotInteractive(NotInteractive::Invalid(message))) => {
                assert!(message.contains("definitely-not-a-color"), "{message}")
            }
            other => panic!("{lines:?}: {other:?}"),
        }
    }
    let empty = ColorPicker::new("Colour").default("");
    assert!(matches!(
        degrade_color(empty, &[""], Fallback::Prompt),
        Err(Error::NotInteractive(NotInteractive::Invalid(_)))
    ));
    // Asked for the default outright, a bad one is no default.
    let bad = ColorPicker::new("Colour").default("nope");
    assert!(Component::default_value(&bad).is_none());
    assert!(matches!(
        degrade_color(bad, &[], Fallback::Default),
        Err(Error::NotInteractive(NotInteractive::NoDefault(_)))
    ));
}

#[test]
fn an_empty_colour_line_is_the_default_or_no_answer() {
    let red = ColorPicker::new("Colour").default("red");
    assert_eq!(
        degrade_color(red, &[""], Fallback::Prompt).unwrap(),
        Outcome::Done("#800000".into())
    );
    // No default: no answer, as when input ends.
    assert!(matches!(
        degrade_color(ColorPicker::new("Colour"), &[""], Fallback::Prompt),
        Err(Error::NotInteractive(NotInteractive::NoDefault(_)))
    ));
}

// ---- AssetPicker ----

#[test]
fn asset_picker_finds_emoji_by_name() {
    let script = Script::new().text("thumbs_up").keys("enter");
    let (outcome, record) =
        headless::run(AssetPicker::new("Emoji", AssetKind::Emoji), script, 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done("👍".into()));
    assert!(before_answer(&record).contains("👍 thumbs_up"));
}

#[test]
fn asset_picker_previews_box_styles_and_spinners() {
    let script = Script::new().text("double_edge").keys("enter");
    let (outcome, record) = headless::run(AssetPicker::new("Box", AssetKind::Box), script, 100, 16);
    assert_eq!(outcome.unwrap(), Outcome::Done("double_edge".into()));
    assert!(
        before_answer(&record).contains('╔'),
        "{}",
        before_answer(&record)
    );
    let script = Script::new().text("dots").keys("enter");
    let (outcome, _) = headless::run(
        AssetPicker::new("Spinner", AssetKind::Spinner),
        script,
        100,
        16,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("dots".into()));
}

#[test]
fn asset_picker_degrades_to_a_name() {
    let mut io = ScriptedLineIo::new([":rocket:"]);
    let mut picker = AssetPicker::new("Emoji", AssetKind::Emoji);
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("🚀".into()));
    let mut io = ScriptedLineIo::new(Vec::<String>::new());
    let mut picker = AssetPicker::new("Box", AssetKind::Box).default("heavy");
    let outcome = degrade(
        &mut picker,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("heavy".into()));
}

// ---- Bounds, graphemes and line prompts ----

/// The rows `component` renders in a terminal 40 by `rows`.
fn rendered_rows<C: Component>(component: &C, rows: usize) -> usize {
    let console = rich::Console::new();
    let context = Context {
        console: &console,
        width: 40,
        height: rows,
    };
    component.render(&context).lines.len()
}

#[test]
fn a_height_beyond_the_terminal_is_cut_to_it() {
    let tall = 1_000;
    assert!(rendered_rows(&TextArea::new("Notes").height(tall), 12) <= 12);
    assert!(rendered_rows(&ColorPicker::new("Colour").height(tall), 12) <= 12);
    let dir = tree();
    let files = FilePicker::new("File", dir.path())
        .preview(PreviewLayout::Hidden)
        .height(tall);
    assert!(rendered_rows(&files, 12) <= 12);
    // Huge heights cost nothing: nothing is drawn beyond the screen.
    let huge = 100_000_000;
    let (outcome, record) = headless::run(
        TextArea::new("Notes").height(huge),
        Script::new().text("a").keys("pagedown enter ctrl+d"),
        40,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("a\n".into()));
    assert!(before_answer(&record).contains("ctrl+d submit"));
    let (outcome, _) = headless::run(
        ColorPicker::new("Colour").height(huge),
        Script::new().keys("pagedown esc"),
        40,
        12,
    );
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let (_, record) = headless::run(
        FilePicker::new("File", dir.path()).height(huge),
        Script::new().keys("pagedown esc"),
        40,
        12,
    );
    assert!(before_answer(&record).contains("↑↓ move"));
}

#[test]
fn the_character_limit_counts_grapheme_clusters() {
    let family = "👨\u{200d}👩\u{200d}👧";
    let area = TextArea::new("N").char_limit(3).value(family.repeat(4));
    assert_eq!(area.text(), family.repeat(3));
    let accent = "e\u{301}";
    let area = TextArea::new("N").char_limit(1).value(format!("{accent}x"));
    assert_eq!(area.text(), accent);
    // Typed: an accent joins the character before it, even at the limit.
    let script = Script::new().text("ab\u{301}c").keys("ctrl+d");
    let (outcome, _) = headless::run(TextArea::new("N").char_limit(2), script, 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("ab\u{301}".into()));
    // A line break is one.
    assert_eq!(
        TextArea::truncate(&format!("{family}\n{family}"), 2),
        format!("{family}\n")
    );
    assert_eq!(TextArea::truncate("ab\ncd", 4), "ab\nc");
    // Without a terminal, likewise.
    let mut io = ScriptedLineIo::new([format!("{family}{family}")]);
    let mut area = TextArea::new("N").char_limit(1);
    let outcome = degrade(
        &mut area,
        Fallback::Prompt,
        Reason::StdinNotTerminal,
        &mut io,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done(family.to_string()));
}

#[test]
fn text_area_keeps_tabs_and_shows_them_as_spaces() {
    let area = TextArea::new("N").value("a\tb");
    assert_eq!(area.text(), "a\tb");
    let (outcome, record) = headless::run(area, Script::new().keys("left x ctrl+d"), 40, 12);
    assert_eq!(outcome.unwrap(), Outcome::Done("a\txb".into()));
    assert!(
        record.frames[0].contains("│ a    b"),
        "{}",
        record.frames[0]
    );
}

#[test]
fn an_extension_matches_the_end_of_the_name() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.tar.gz", "b.gz", "c.tar", ".tar.gz"] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    let picker = FilePicker::new("File", dir.path())
        .show_hidden(true)
        .extensions(["tar.gz"]);
    assert_eq!(picker.labels(), ["..", "a.tar.gz"]);
    let picker = FilePicker::new("File", dir.path()).extensions(["GZ"]);
    assert_eq!(picker.labels(), ["..", "a.tar.gz", "b.gz"]);
}

#[test]
fn a_second_click_on_a_colour_picks_it_not_the_first() {
    // Row 2 is the first colour, focused from the start.
    let picker = ColorPicker::new("Colour").with_mouse(true);
    let (outcome, _) = headless::run(picker, Script::new().click(4, 2).keys("esc"), 80, 16);
    assert_eq!(outcome.unwrap(), Outcome::Cancelled);
    let picker = ColorPicker::new("Colour")
        .format(ColorFormat::Name)
        .with_mouse(true);
    let (outcome, _) = headless::run(picker, Script::new().click(4, 3).click(4, 3), 80, 16);
    assert!(matches!(outcome.unwrap(), Outcome::Done(_)));
}

#[test]
fn line_prompts_show_terminal_controls_as_text() {
    let header = "H\x1b]0;PWNED\x07\x1b[2J\u{9b}";
    let reason = Reason::StdinNotTerminal;
    let mut written = Vec::new();
    let mut io = ScriptedLineIo::new(["red"]);
    degrade(
        &mut ColorPicker::new(header),
        Fallback::Prompt,
        reason,
        &mut io,
    )
    .unwrap();
    written.push(io.written);
    let mut io = ScriptedLineIo::new(["x"]);
    degrade(
        &mut TextArea::new(header),
        Fallback::Prompt,
        reason,
        &mut io,
    )
    .unwrap();
    written.push(io.written);
    let mut io = ScriptedLineIo::new(["x"]);
    degrade(&mut Input::new(header), Fallback::Prompt, reason, &mut io).unwrap();
    written.push(io.written);
    let mut io = ScriptedLineIo::new(["y"]);
    degrade(&mut Confirm::new(header), Fallback::Prompt, reason, &mut io).unwrap();
    written.push(io.written);
    let mut io = ScriptedLineIo::new(["1"]);
    let items = [header.to_string(), "b".to_string()];
    degrade(
        &mut Select::new(header, items),
        Fallback::Prompt,
        reason,
        &mut io,
    )
    .unwrap();
    written.push(io.written);
    for written in written {
        assert!(written.contains("H␛]0;PWNED␇␛[2J"), "{written:?}");
        assert!(
            !written.chars().any(|c| c.is_control() && c != '\n'),
            "{written:?}"
        );
    }
}

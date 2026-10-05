//! The process-wide overrides (`keymap::install`, what the CLI's
//! configuration sets) rebind `Confirm`, `Pager`, `TextArea`,
//! `ColorPicker`, `Form` and `FilePicker` (0.0.15 workstream 6). One test,
//! alone in its binary, since the overrides are global.

use rich::Segment;
use rich_interact::headless::{self, Script};
use rich_interact::keymap::{self, Overrides};
use rich_interact::{ColorPicker, Confirm, FilePicker, Form, Input, Pager, TextArea};

#[test]
fn installed_overrides_rebind_every_built_in() {
    keymap::install(
        Overrides::parse(
            "confirm.choose-yes = j\n\
             pager.quit = x\n\
             textarea.submit = ctrl+s\n\
             color.pick = ctrl+o\n\
             form.next = ctrl+n\n\
             file.open = ctrl+o\n",
        )
        .unwrap(),
    );
    let mut failed = Vec::new();

    let (outcome, _) = headless::run(Confirm::new("Go?"), Script::new().keys("j"), 40, 8);
    if outcome.ok().and_then(|o| o.value()).as_deref() != Some("yes") {
        failed.push("confirm");
    }

    let lines = vec![vec![Segment::new("x", None)]];
    let (outcome, _) = headless::run(Pager::lines(lines), Script::new().keys("x"), 40, 6);
    if outcome.ok().and_then(|o| o.value()).is_none() {
        failed.push("pager");
    }

    let script = Script::new()
        .text("a")
        .keys("ctrl+d")
        .text("b")
        .keys("ctrl+s");
    let (outcome, _) = headless::run(TextArea::new("Notes"), script, 40, 10);
    if outcome.ok().and_then(|o| o.value()).as_deref() != Some("ab") {
        failed.push("textarea");
    }

    let script = Script::new().text("#00ff00").keys("ctrl+o");
    let (outcome, _) = headless::run(ColorPicker::new("Colour"), script, 60, 14);
    if outcome.ok().and_then(|o| o.value()).as_deref() != Some("#00ff00") {
        failed.push("color");
    }

    let form = Form::new("F")
        .input("a", Input::new("A"))
        .input("b", Input::new("B"));
    let script = Script::new()
        .text("1")
        .keys("ctrl+n")
        .text("2")
        .keys("ctrl+s");
    let (outcome, _) = headless::run(form, script, 60, 10);
    let answers = outcome.ok().and_then(|o| o.value());
    let fields = answers.as_ref().map(|answers| {
        (
            answers.text("a").map(str::to_string),
            answers.text("b").map(str::to_string),
        )
    });
    if fields != Some((Some("1".into()), Some("2".into()))) {
        failed.push("form");
    }

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub").join("in.txt"), "x").unwrap();
    let script = Script::new().keys("ctrl+o enter");
    let (outcome, _) = headless::run(FilePicker::new("File", dir.path()), script, 60, 10);
    if outcome.ok().and_then(|o| o.value()) != Some(dir.path().join("sub").join("in.txt")) {
        failed.push("file");
    }

    keymap::install(Overrides::new());
    assert!(failed.is_empty(), "not rebound: {failed:?}");
}

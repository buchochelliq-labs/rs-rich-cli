//! Rebinding the components that matched keys inline until 0.0.15
//! (workstream 6): `Confirm`, `Pager`, `TextArea`, `ColorPicker`, `Form`
//! and `FilePicker` look their keys up in their keymaps, so a rebound key
//! does the action and the old key no longer does.

use rich::Segment;
use rich_interact::headless::{self, Script};
use rich_interact::keymap::keys;
use rich_interact::{
    Choice, ColorPicker, Component, Confirm, FilePicker, Form, Input, Outcome, Pager, TextArea,
};

#[test]
fn confirm_choices_and_moves_rebind() {
    let confirm = || Confirm::new("Deploy?").rebind("choose-yes", keys("j"));
    // `y` no longer picks yes; `n` still picks no.
    let (outcome, _) = headless::run(confirm(), Script::new().keys("y n"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("no".to_string()));
    let (outcome, _) = headless::run(confirm(), Script::new().keys("j"), 40, 8);
    assert_eq!(outcome.unwrap(), Outcome::Done("yes".to_string()));
    // Moving and picking the focused choice.
    let confirm = Confirm::new("Apply?")
        .choices([
            Choice::new("a", "Apply", 'a'),
            Choice::new("s", "Skip", 's'),
        ])
        .rebind("next", keys("ctrl+l"))
        .rebind("pick", keys("ctrl+o"));
    let (outcome, _) = headless::run(
        confirm,
        Script::new().keys("right ctrl+l enter ctrl+o"),
        40,
        8,
    );
    assert_eq!(outcome.unwrap(), Outcome::Done("s".to_string()));
    // The keymap lists the keys that do each action now.
    let confirm = Confirm::new("Deploy?").rebind("choose-yes", keys("j"));
    assert_eq!(confirm.keymap().keys("choose-yes"), keys("j"));
}

#[test]
fn pager_keys_rebind() {
    let lines: Vec<Vec<Segment>> = (1..=40)
        .map(|n| vec![Segment::new(format!("line {n}"), None)])
        .collect();
    let pager = Pager::lines(lines)
        .rebind("scroll-down", keys("ctrl+j"))
        .rebind("quit", keys("x"));
    // `j` and `q` do nothing now; Ctrl+J scrolls and `x` quits.
    let script = Script::new().keys("j q ctrl+j ctrl+j x");
    let (outcome, record) = headless::run(pager, script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    let frames = record.frames.join("\n---\n");
    assert!(frames.contains("lines 3–"), "{frames}");
    assert!(!frames.contains("lines 4–"), "{frames}");
}

#[test]
fn text_area_keys_rebind() {
    let area = TextArea::new("Notes").rebind("submit", keys("ctrl+s"));
    // Ctrl+D no longer submits.
    let script = Script::new()
        .text("hi")
        .keys("ctrl+d")
        .text("!")
        .keys("ctrl+s");
    let (outcome, record) = headless::run(area, script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("hi!".to_string()));
    // The hint names the key that submits now.
    let frames = record.frames.join("\n");
    assert!(frames.contains("ctrl+s submit"), "{frames}");
    let area = TextArea::new("Notes").rebind("newline", keys("ctrl+j"));
    let script = Script::new()
        .text("a")
        .keys("enter ctrl+j")
        .text("b")
        .keys("ctrl+d");
    let (outcome, _) = headless::run(area, script, 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done("a\nb".to_string()));
}

#[test]
fn color_picker_keys_rebind() {
    let picker = ColorPicker::new("Colour")
        .rebind("pick", keys("ctrl+o"))
        .rebind("clear", keys("ctrl+x"));
    // Enter no longer picks, Ctrl+U no longer clears; Ctrl+X clears.
    let script = Script::new()
        .text("blue")
        .keys("enter ctrl+u ctrl+x")
        .text("#ff0000")
        .keys("ctrl+o");
    let (outcome, _) = headless::run(picker, script, 60, 14);
    assert_eq!(outcome.unwrap(), Outcome::Done("#ff0000".to_string()));
}

#[test]
fn form_keys_rebind() {
    let form = Form::new("Service")
        .input("name", Input::new("Name"))
        .toggle("tls", "TLS", false)
        .rebind("next", keys("ctrl+n"))
        .rebind("yes", keys("j"));
    // Tab no longer moves on (and a text field leaves it alone); Ctrl+N
    // does; `j` turns the toggle on; Ctrl+S submits as before.
    let script = Script::new()
        .text("api")
        .keys("tab ctrl+n")
        .keys("y j ctrl+s");
    let (outcome, _) = headless::run(form, script, 60, 12);
    let answers = outcome.unwrap().value().unwrap();
    assert_eq!(answers.text("name"), Some("api"));
    assert_eq!(answers.flag("tls"), Some(true));
}

#[test]
fn file_picker_keys_rebind() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    std::fs::write(dir.path().join("sub").join("inner.txt"), "x").unwrap();
    std::fs::write(dir.path().join("top.txt"), "x").unwrap();
    let picker = FilePicker::new("File", dir.path())
        .rebind("open", keys("ctrl+o"))
        .rebind("down", keys("ctrl+j"));
    // Right no longer opens `sub`; Ctrl+O does, and the list's own keys
    // rebind through the picker too.
    let script = Script::new().keys("right ctrl+o enter");
    let (outcome, _) = headless::run(picker, script, 60, 12);
    let picked = outcome.unwrap().value().unwrap();
    assert_eq!(picked, dir.path().join("sub").join("inner.txt"));
    let picker = FilePicker::new("File", dir.path()).rebind("down", keys("ctrl+j"));
    let (outcome, _) = headless::run(picker, Script::new().keys("down ctrl+j enter"), 60, 12);
    assert_eq!(
        outcome.unwrap().value().unwrap(),
        dir.path().join("top.txt")
    );
}

#[test]
fn file_picker_up_rebinds_the_parent_key_only() {
    // `up` is the picker's go-to-parent; rebinding it leaves the list's
    // cursor-up alone. `select.up` is the list's.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "x").unwrap();
    std::fs::write(dir.path().join("b.txt"), "x").unwrap();
    let picker = FilePicker::new("File", dir.path()).rebind("up", keys("ctrl+h"));
    let (outcome, _) = headless::run(picker, Script::new().keys("down up enter"), 60, 12);
    assert_eq!(outcome.unwrap().value().unwrap(), dir.path().join("a.txt"));
    let picker = FilePicker::new("File", dir.path()).rebind("select.up", keys("ctrl+p"));
    let (outcome, _) = headless::run(picker, Script::new().keys("down up enter"), 60, 12);
    assert_eq!(outcome.unwrap().value().unwrap(), dir.path().join("b.txt"));
    let picker = FilePicker::new("File", dir.path()).rebind("select.up", keys("ctrl+p"));
    let (outcome, _) = headless::run(picker, Script::new().keys("down ctrl+p enter"), 60, 12);
    assert_eq!(outcome.unwrap().value().unwrap(), dir.path().join("a.txt"));
}

#[test]
fn confirm_choice_keys_answer_either_case() {
    // As in 0.0.14: an uppercase choice key answers to the lowercase press,
    // and a lowercase one to the uppercase press.
    let confirm = || {
        Confirm::new("Go?").choices([
            Choice::new("all", "Apply all", 'A'),
            Choice::new("skip", "Skip", 's'),
        ])
    };
    for (press, want) in [("a", "all"), ("A", "all"), ("s", "skip"), ("S", "skip")] {
        let (outcome, _) = headless::run(confirm(), Script::new().keys(press), 40, 8);
        assert_eq!(outcome.unwrap(), Outcome::Done(want.to_string()), "{press}");
    }
}

#[test]
fn rebinding_onto_an_earlier_actions_key_takes_it() {
    // `n` is declared for next-match, earlier than scroll-down: rebinding
    // scroll-down onto it makes `n` scroll.
    let lines: Vec<Vec<Segment>> = (1..=40)
        .map(|n| vec![Segment::new(format!("line {n}"), None)])
        .collect();
    let pager = Pager::lines(lines).rebind("scroll-down", keys("n"));
    let (outcome, record) = headless::run(pager, Script::new().keys("n n q"), 40, 10);
    assert_eq!(outcome.unwrap(), Outcome::Done(()));
    let frames = record.frames.join("\n---\n");
    assert!(frames.contains("line 3"), "{frames}");
    assert!(
        !record.frames.last().unwrap().contains("line 1\n"),
        "{frames}"
    );
}

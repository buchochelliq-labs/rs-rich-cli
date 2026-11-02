//! The interactive diff view (#799): hunks, folds, decisions, layouts.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::interact::headless::Script;
use intuituive::prelude::*;
use intuituive::widgets::{Decision, DiffOptions, accepted_text, diff_view_with};
use intuituive::{assert_screen, assert_screen_styled};
use rich_ext::diff::Layout;

/// Twenty numbered lines, and the same with three hunks: a change near the
/// top, one in the middle, and a line added at the end.
fn texts() -> (String, String) {
    let old: String = (1..=20).map(|i| format!("line {i}\n")).collect();
    let new = old
        .replace("line 2\n", "line two\n")
        .replace("line 10\n", "line ten\nline ten and a half\n")
        + "line 21\n";
    (old, new)
}

/// Signals made inside an app, read by the test after.
type Out<T> = Rc<Cell<Option<T>>>;

fn out<T>() -> Out<T> {
    Rc::new(Cell::new(None))
}

fn app_with(options: impl Fn() -> DiffOptions + 'static) -> App {
    App::new(move || {
        let (old, new) = texts();
        diff_view_with(signal(old), signal(new), options())
    })
}

#[test]
fn three_hunks_unified() {
    let mut driver = app_with(|| DiffOptions::new().context(2)).driver(44, 22);
    driver.settle();
    assert_screen_styled!(driver, "diff_view_unified");
}

#[test]
fn three_hunks_side_by_side() {
    let mut driver =
        app_with(|| DiffOptions::new().context(2).layout(Layout::SideBySide)).driver(64, 18);
    driver.settle();
    assert_screen!(driver, "diff_view_side_by_side");
}

#[test]
fn side_by_side_by_width() {
    let app = app_with(|| DiffOptions::new().side_by_side_at(60));
    let mut driver = app.driver(70, 10);
    driver.settle();
    assert!(
        driver.screen().plain()[3].contains(" │ "),
        "{:?}",
        driver.screen().plain()
    );
    driver.resize(50, 10);
    driver.settle();
    assert!(!driver.screen().plain().join("\n").contains(" │ "));
}

#[test]
fn keys_decide_hunks_and_status_counts_them() {
    let seen = out();
    let app = App::new({
        let seen = seen.clone();
        move || {
            let (old, new) = texts();
            let decisions = signal(Vec::<Decision>::new());
            seen.set(Some(decisions));
            diff_view_with(
                signal(old),
                signal(new),
                DiffOptions::new().decide(decisions),
            )
        }
    });
    let mut driver = app.driver(60, 12);
    driver.press("] ] a [ r");
    let decisions = seen.get().unwrap();
    assert_eq!(
        decisions.get(),
        [Decision::Rejected, Decision::Accepted, Decision::Pending]
    );
    let screen = driver.screen().plain();
    let status = screen.last().unwrap().trim_end();
    assert_eq!(
        status,
        "hunk 1 of 3 · 2 of 3 decided · 1 accepted · 1 rejected"
    );
    // The cursor's hunk is marked, and decided hunks say so.
    assert!(
        screen[0].starts_with("▌@@ -2 +2 @@  ✗ rejected"),
        "{screen:?}"
    );
    driver.press("A");
    assert_eq!(decisions.get(), [Decision::Accepted; 3]);
    driver.press("R");
    assert_eq!(decisions.get(), [Decision::Rejected; 3]);
}

#[test]
fn on_done_runs_when_the_last_hunk_is_decided() {
    let seen = out();
    let app = App::new({
        let seen = seen.clone();
        move || {
            let (old, new) = texts();
            let done = signal(String::new());
            seen.set(Some(done));
            let options = DiffOptions::new()
                .decide(signal(Vec::new()))
                .on_done(move |decisions, _| done.set(format!("{decisions:?}")));
            diff_view_with(signal(old), signal(new), options)
        }
    });
    let mut driver = app.driver(50, 12);
    driver.press("] a ] a");
    let done = seen.get().unwrap();
    assert_eq!(done.get(), "");
    driver.press("] r");
    assert_eq!(done.get(), "[Accepted, Accepted, Rejected]");
}

#[test]
fn accepted_text_applies_the_accepted_hunks() {
    let (old, new) = texts();
    use Decision::*;
    assert_eq!(accepted_text(&old, &new, &[Accepted; 3]), new);
    assert_eq!(accepted_text(&old, &new, &[Rejected; 3]), old);
    assert_eq!(accepted_text(&old, &new, &[]), old);
    let mixed = accepted_text(&old, &new, &[Accepted, Pending, Accepted]);
    assert!(mixed.starts_with("line 1\nline two\nline 3\n"));
    assert!(mixed.contains("line 10\nline 11\n"));
    assert!(mixed.ends_with("line 20\nline 21\n"));
    // The first and last lines of the file.
    let (old, new) = ("first\nmid\nlast", "FIRST\nmid\nLAST\n");
    assert_eq!(
        accepted_text(old, new, &[Accepted, Rejected]),
        "FIRST\nmid\nlast"
    );
    assert_eq!(
        accepted_text(old, new, &[Rejected, Accepted]),
        "first\nmid\nLAST\n"
    );
}

#[test]
fn enter_opens_the_folds_around_the_hunk_and_a_click_opens_one() {
    let app = app_with(|| DiffOptions::new().context(1));
    let mut driver = app.driver(40, 40);
    driver.settle();
    let folds = |driver: &intuituive::Driver| {
        driver
            .screen()
            .plain()
            .iter()
            .filter(|row| row.contains("unchanged"))
            .count()
    };
    // Lines 3 to 9 and 11 to 20 fold; line 1 is context of the first hunk.
    assert_eq!(folds(&driver), 2);
    driver.press("] ] enter");
    assert_eq!(folds(&driver), 0);
    driver.press("enter");
    assert_eq!(folds(&driver), 2);
    let row = driver
        .screen()
        .plain()
        .iter()
        .position(|row| row.contains("unchanged"))
        .unwrap();
    driver.click(5, row as u16);
    assert_eq!(folds(&driver), 1);
}

#[test]
fn a_click_moves_the_cursor_and_the_wheel_scrolls() {
    let seen = out();
    let app = App::new({
        let seen = seen.clone();
        move || {
            let (old, new) = texts();
            let cursor = signal(None);
            seen.set(Some(cursor));
            diff_view_with(signal(old), signal(new), DiffOptions::new().cursor(cursor))
        }
    });
    let mut driver = app.driver(40, 6);
    driver.settle();
    let cursor = seen.get().unwrap();
    let first = driver.screen().plain()[0].clone();
    driver.click(5, 2);
    assert_eq!(cursor.get(), Some(0));
    driver.wheel(5, 2, 1);
    assert_ne!(driver.screen().plain()[0], first);
    driver.press("home");
    assert_eq!(driver.screen().plain()[0], first.replacen(' ', "▌", 1));
}

#[test]
fn moving_to_a_hunk_scrolls_it_into_view() {
    let app = app_with(DiffOptions::new);
    let mut driver = app.driver(40, 5);
    driver.press("] ] ]");
    let screen = driver.screen().plain();
    assert!(screen[1].starts_with("▌@@ -20,0 +22 @@"), "{screen:?}");
}

#[test]
fn a_new_text_is_diffed_again() {
    let seen = out();
    let app = App::new({
        let seen = seen.clone();
        move || {
            let new = signal("a\n".to_string());
            seen.set(Some(new));
            diff_view_with(signal("a\n".to_string()), new, DiffOptions::new())
        }
    });
    let mut driver = app.driver(30, 3);
    driver.settle();
    let new = seen.get().unwrap();
    assert_eq!(driver.screen().plain()[0].trim_end(), "no differences");
    new.set("b\n".to_string());
    driver.settle();
    assert_eq!(driver.screen().plain()[0].trim_end(), " @@ -1 +1 @@");
}

#[test]
fn a_new_text_sets_the_decisions_back_to_pending() {
    let seen = out();
    let app = App::new({
        let seen = seen.clone();
        move || {
            let (old, new) = texts();
            let (old, new) = (signal(old), signal(new));
            let decisions = signal(Vec::<Decision>::new());
            seen.set(Some((old, new, decisions)));
            diff_view_with(old, new, DiffOptions::new().decide(decisions))
        }
    });
    let mut driver = app.driver(60, 12);
    driver.press("A");
    let (old, new, decisions) = seen.get().unwrap();
    assert_eq!(decisions.get(), [Decision::Accepted; 3]);
    // A hunk joins the others: no decision moves onto it, or onto any.
    new.set(new.get().replacen("line 6\n", "line six\n", 1));
    driver.settle();
    assert_eq!(decisions.get(), [Decision::Pending; 4]);
    assert_eq!(
        accepted_text(&old.get(), &new.get(), &decisions.get()),
        old.get()
    );
    let screen = driver.screen().plain();
    assert_eq!(
        screen.last().unwrap().trim_end(),
        "4 hunks · 0 of 4 decided"
    );
}

#[test]
fn the_keys_are_in_the_help() {
    let app = App::new(|| {
        let (old, new) = texts();
        diff_view_with(
            signal(old),
            signal(new),
            DiffOptions::new().decide(signal(vec![])),
        )
        .on_key("?", |cx| cx.help())
    });
    let mut driver = app.driver(70, 24);
    driver.press("?");
    let help = driver.screen().plain().join("\n");
    for what in [
        "Next hunk",
        "Previous hunk",
        "Accept the hunk",
        "Reject every hunk",
    ] {
        assert!(help.contains(what), "{what}: {help}");
    }
}

#[cfg(feature = "syntax")]
#[test]
fn a_language_highlights_both_sides() {
    let app = App::new(|| {
        diff_view_with(
            signal("fn main() {}\n".to_string()),
            signal("fn main() { run(); }\n".to_string()),
            DiffOptions::new().language("rust"),
        )
    });
    let mut driver = app.driver(40, 4);
    driver.settle();
    let styled = intuituive::testing::styled(&driver);
    // `main` has a colour of its own, apart from the line's.
    assert!(styled.contains("]main[/]"), "{styled}");
    assert!(driver.screen().plain()[1].contains("- fn main() {}"));
}

/// Ten thousand lines with a change every hundred: scrolling draws only
/// the rows in view, so a frame costs what a short diff's does.
#[test]
fn a_long_diff_scrolls_drawing_only_the_rows_in_view() {
    let old: String = (0..10_000).map(|i| format!("line {i}\n")).collect();
    let new: String = (0..10_000)
        .map(|i| match i % 100 {
            50 => format!("changed {i}\n"),
            _ => format!("line {i}\n"),
        })
        .collect();
    let app = App::new(move || {
        diff_view_with(
            signal(old.clone()),
            signal(new.clone()),
            DiffOptions::new().decide(signal(Vec::new())),
        )
    });
    let script = Script::new()
        .keys(&"pagedown ".repeat(60))
        .keys(&"] ".repeat(50));
    let demo = app.timedemo(script, 80, 30);
    assert!(demo.frames.len() > 60, "{demo}");
    // Each frame after the first renders 30 rows; even unoptimised it
    // stays well under a frame at 60 Hz.
    let worst = demo.frames[1..].iter().map(|f| f.draw).max().unwrap();
    assert!(worst.as_millis() < 100, "{demo}");
}

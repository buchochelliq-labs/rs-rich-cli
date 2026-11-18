//! `steps`: a checklist's states and their styles, a stepper across, and
//! a wizard moved with the keys, clicks and its own buttons.

use intuituive::prelude::*;
use intuituive::{Driver, StepState, button, steps};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

#[test]
fn states_follow_signals_and_take_their_styles() {
    let app = App::new(|| {
        let done = signal(0usize);
        let failed = signal(false);
        steps(
            || vec!["Fetch".into(), "Build".into(), "Test".into()],
            move |i| match (i.cmp(&done.get()), failed.get()) {
                (std::cmp::Ordering::Less, _) => StepState::Done,
                (std::cmp::Ordering::Equal, true) => StepState::Failed,
                (std::cmp::Ordering::Equal, false) => StepState::Current,
                _ => StepState::Pending,
            },
        )
        .on_key("n", move |_| done.update(|d| *d += 1))
        .on_key("f", move |_| failed.set(true))
        .focusable()
    });
    let mut driver = app.driver(10, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["▸ Fetch", "○ Build", "○ Test"]);
    driver.press("n");
    assert_eq!(rows(&driver), ["✓ Fetch", "▸ Build", "○ Test"]);
    driver.press("f");
    assert_eq!(rows(&driver), ["✓ Fetch", "✗ Build", "○ Test"]);
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[green]✓ [/]Fetch"), "{styled}");
    assert!(styled.contains("[bold red]✗ [/]Build"), "{styled}");
    assert!(styled.contains("[dim]○ Test"), "{styled}");
}

#[test]
fn a_wizard_moves_with_its_buttons_keys_and_clicks() {
    let app = App::new(|| {
        let page = signal(0usize);
        let titles = ["Account", "Profile", "Confirm"];
        column([
            steps(
                move || titles.iter().map(|t| t.to_string()).collect(),
                move |i| StepState::at(i, page.get()),
            )
            .steps_across(" ─ ")
            .step_nav(page),
            text(move || format!("page: {}", titles[page.get()])).fixed(1),
            row([
                button("Back", move |_| page.update(|p| *p = p.saturating_sub(1))).fixed(8),
                button("Next", move |_| page.update(|p| *p = (*p + 1).min(2))).fixed(8),
            ])
            .fixed(1),
        ])
    });
    let mut driver = app.driver(40, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0], "▸ Account ─ ○ Profile ─ ○ Confirm");
    assert!(driver.click_text("Next"));
    assert_eq!(rows(&driver)[0], "✓ Account ─ ▸ Profile ─ ○ Confirm");
    assert_eq!(rows(&driver)[1], "page: Profile");
    // The stepper itself: a click on a step, then the keys.
    assert!(driver.click_text("Confirm"));
    assert_eq!(rows(&driver)[1], "page: Confirm");
    driver.press("left");
    assert_eq!(rows(&driver)[1], "page: Profile");
    driver.press("home");
    assert_eq!(rows(&driver)[0], "▸ Account ─ ○ Profile ─ ○ Confirm");
    // Focused, the current step is reversed.
    let styled = intuituive::testing::styled(&driver);
    assert!(styled.contains("[bold reverse]▸ Account[/]"), "{styled}");
    driver.press("end");
    assert_eq!(rows(&driver)[1], "page: Confirm");
}

#[test]
fn text_mode_marks_are_ascii() {
    let app =
        App::new(|| steps(|| vec!["a".into(), "b".into()], |i| StepState::at(i, 1))).linear(true);
    let mut driver = app.driver(10, 2);
    driver.settle();
    let shown = rows(&driver).join("\n");
    assert!(
        shown.contains("[x] a") && shown.contains("[>] b"),
        "{shown}"
    );
}

#[test]
fn a_long_step_wraps_under_its_text_and_keeps_its_line_breaks() {
    // A plan entry longer than the width, and one with a line break of its
    // own: every word shows, each wrapped line under the step's text.
    let app = App::new(|| {
        column([
            steps(
                || {
                    vec![
                        "Read the configuration and the tests".into(),
                        "Run them\nthen report".into(),
                        "Done".into(),
                    ]
                },
                |i| [StepState::Done, StepState::Current, StepState::Pending][i],
            ),
            label("after"),
        ])
    });
    let mut driver = app.driver(16, 8);
    driver.settle();
    assert_eq!(
        rows(&driver)[..7],
        [
            "✓ Read the",
            "  configuration",
            "  and the tests",
            "▸ Run them",
            "  then report",
            "○ Done",
            "after",
        ]
    );
    // A screen reader hears the whole step.
    let read: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|n| n.describe())
        .collect();
    assert!(
        read.iter()
            .any(|d| d.contains("Run them\nthen report current")
                || d.contains("Run them then report current")),
        "{read:?}"
    );
}

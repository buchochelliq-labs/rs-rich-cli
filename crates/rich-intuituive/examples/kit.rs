//! The widgets after Textual's and other kits' in one screen: a notice
//! bar, a wizard's stepper and a plan's checklist, button variants,
//! chips and a tag input, an accordion, a pager and a range slider.
//!
//!     cargo run -p rs-rich-intuituive --example kit
//!
//! Tab moves between them. On the stepper ← and → move the page; t shows
//! a toast with an Undo button. Ctrl+Q quits.

use intuituive::prelude::*;
use intuituive::{
    ButtonVariant, Level, Notification, StepState, accordion, button, chip, notice_bar, pager,
    range_slider, steps, tag_input,
};

const PAGES: [&str; 4] = ["Account", "Plan", "Payment", "Done"];

fn main() -> std::io::Result<()> {
    App::new(|| {
        let shown = signal(true);
        let page = signal(1usize);
        let tags = signal(vec!["rust".to_string(), "tui".to_string()]);
        let open = signal(Some(0usize));
        let results = signal(0usize);
        let (low, high) = (signal(20.0), signal(80.0));
        let plan = ["Fetch", "Build", "Test", "Deploy"];
        let plan_states = [
            StepState::Done,
            StepState::Done,
            StepState::Current,
            StepState::Pending,
        ];
        column([
            notice_bar(
                shown,
                || "Changed on disk".into(),
                || "notes.md changed since you opened it; reload it, or keep your edits".into(),
            )
            .notice_level(Level::Warn)
            .notice_action("Reload", |_| {})
            .notice_action("Keep mine", |_| {})
            .notice_closes(true),
            label(""),
            label("[b]Wizard[/]"),
            steps(
                || PAGES.iter().map(|p| p.to_string()).collect(),
                move |i| StepState::at(i, page.get()),
            )
            .steps_across(" ─ ")
            .step_nav(page)
            .name("wizard"),
            label(""),
            row([
                column([
                    label("[b]Plan[/]"),
                    steps(
                        move || plan.iter().map(|p| p.to_string()).collect(),
                        move |i| plan_states[i],
                    ),
                ])
                .flex(1),
                column([
                    label("[b]Buttons[/]"),
                    row([
                        button("Save", |_| {}).variant(ButtonVariant::Success),
                        button("Retry", |_| {}).variant(ButtonVariant::Warning),
                        button("Delete", |_| {}).variant(ButtonVariant::Danger),
                    ])
                    .gap(1),
                ])
                .flex(2),
            ])
            .fixed(5),
            label(""),
            row([
                label("[b]Tags[/] "),
                chip("beta"),
                label(" "),
                tag_input(tags).flex(1),
            ])
            .fixed(1),
            label(""),
            accordion(
                open,
                [
                    ("General", label("Name: Memo")),
                    ("Display", label("Theme: dark")),
                    ("Keys", label("Keymap: default")),
                ],
            ),
            label(""),
            row([
                label("[b]Results[/] "),
                pager(results, || 5).name("results"),
                label("   [b]Price[/] "),
                range_slider(low, high, 0.0..=100.0, 5.0).flex(1),
            ])
            .fixed(1),
        ])
        .on_key("t", |cx| {
            cx.notify_with(
                Notification::new(Level::Success, "Saved", "notes.md is saved")
                    .action("Undo", |_| {})
                    .timeout(std::time::Duration::from_secs(600)),
            )
        })
    })
    .run()
}

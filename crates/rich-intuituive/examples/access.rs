//! Accessibility in one app: roles, states (selected, expanded, checked,
//! busy), a decoration hidden from screen readers, a live status, a toast
//! and a dialog. The screen reader checklist
//! (docs/guide/intuituive/screen-readers.md) walks through it.
//!
//!     cargo run -p rs-rich-intuituive --example access
//!     cargo run -p rs-rich-intuituive --example access -- --accessible
//!     cargo run -p rs-rich-intuituive --example access -- --linear
//!
//! `--accessible` (or `INTUITUIVE_ACCESSIBLE=1`) draws in text mode with
//! the cursor on the focus; `--linear` (or `INTUITUIVE_ACCESSIBLE=linear`)
//! writes lines of text instead of a screen.
//!
//! Tab moves between the parts. ←/→ switch tabs; ↑/↓ move in the tree and
//! the list, → opens a folder; Space ticks a setting; s saves (a toast); d
//! asks before deleting (a dialog); r reloads (busy, then the status
//! changes); q quits.

use std::time::Duration;

use intuituive::a11y::Role;
use intuituive::prelude::*;
use intuituive::widgets::{tabs, tree, TreeItem};

const SPINNER: [&str; 4] = ["-", "\\", "|", "/"];

/// A setting that Space ticks: a check box, or a switch.
fn setting(name: &'static str, on: Signal<bool>, role: Role) -> Node {
    text(move || format!("{} {name}", if on.get() { "[x]" } else { "[ ]" }))
        .label(name)
        .role(role)
        .checked_when(move || on.get())
        .focusable()
        .focus_style("reverse")
        .on_key("space", move |_| on.update(|on| *on = !*on))
        .fixed(1)
}

/// A clickable label that Enter presses too.
fn button(name: &'static str, press: impl Fn(&mut Ctx) + Copy + 'static) -> Node {
    label(format!("[ {name} ]"))
        .label(name)
        .focusable()
        .focus_style("reverse")
        .on_click(press)
        .on_key("enter", press)
        .fixed(name.len() as u16 + 4)
}

fn delete(cx: &mut Ctx) {
    cx.modal(Size::Auto, Size::Auto, || {
        label("Delete notes-1.md? [b]y[/] / [b]n[/]")
            .padding(0, 1)
            .panel("Delete")
            .on_key("y", |cx| {
                cx.pop();
                cx.toast("Deleted notes-1.md");
            })
            .on_key("n esc", |cx| cx.pop())
    })
}

/// The app.
pub fn access_app() -> App {
    App::new(|| {
        let tab = signal(0usize);
        let folder = signal(vec![0usize]);
        let file = signal(0usize);
        let wrap = signal(true);
        let dark = signal(false);
        let loading = signal(false);
        let loads = signal(1u32);
        let turn = signal(0usize);
        every(Duration::from_millis(150), move |_| {
            if loading.get_untracked() {
                turn.update(|t| *t += 1);
            }
        });
        let reload = move || {
            loading.set(true);
            spawn(
                || std::thread::sleep(Duration::from_millis(1500)),
                move |_, _| {
                    loading.set(false);
                    loads.update(|n| *n += 1);
                },
            );
        };

        let folders = || {
            vec![
                TreeItem::new("src").children([TreeItem::new("main.rs"), TreeItem::new("lib.rs")]),
                TreeItem::new("docs").child(TreeItem::new("guide.md")),
                TreeItem::new("README.md"),
            ]
        };
        let files = || (1..=10).map(|n| format!("notes-{n}.md")).collect();
        let body = switch(
            move || tab.get(),
            move |tab| match tab {
                0 => row([
                    tree(folders, folder).label("Folders").panel("Folders"),
                    list(files, file).label("Files").panel("Files"),
                ]),
                _ => column([
                    setting("Wrap lines", wrap, Role::CheckBox),
                    setting("Dark theme", dark, Role::Switch),
                ])
                .panel("Settings"),
            },
        );
        // The spinner's glyph is decoration: the status says the same.
        let spinner = text(move || match loading.get() {
            true => SPINNER[turn.get() % SPINNER.len()].to_string(),
            false => " ".into(),
        })
        .access_hidden(true)
        .fixed(2);
        let status = row([
            spinner,
            text(move || match (loading.get(), loads.get()) {
                (true, _) => "Loading…".into(),
                (false, 1) => "10 files, loaded once".into(),
                (false, n) => format!("10 files, loaded {n} times"),
            }),
        ])
        .live()
        .busy_when(move || loading.get());

        column([
            label("[b]Notes[/]").fixed(1),
            label("[dim]≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈≈[/]")
                .access_hidden(true)
                .fixed(1),
            tabs(|| vec!["Files".into(), "Settings".into()], tab)
                .label("Sections")
                .fixed(1),
            body,
            status.fixed(1),
            row([
                button("Save", |cx| cx.toast("Saved")),
                button("Delete", delete),
                button("Reload", move |_| reload()),
            ])
            .gap(1)
            .fixed(1),
            label("[dim]tab moves · space ticks · s saves · d deletes · r reloads · q quits")
                .fixed(1),
        ])
        .on_key("s", |cx| cx.toast("Saved"))
        .on_key("d", delete)
        .on_key("r", move |_| reload())
        .on_key("q", |cx| cx.quit())
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|arg| arg == name);
    let mut app = access_app();
    if flag("--accessible") {
        app = app.accessible(true);
    }
    if flag("--linear") {
        app = app.linear(true);
    }
    app.run()
}

//! Overlays and chrome round a built-in component (0.0.14 workstream 2):
//! `cargo run -p rs-rich-interact --example overlays`.
//!
//! A [`Select`] of files, wrapped in [`Overlays`]:
//!
//! - **Ctrl+O** opens the command palette: every key the list has, by
//!   category, with its shortcut, found by fuzzy search, and two commands
//!   of the example's own. Enter runs what is picked.
//! - **F1** opens the help overlay, searchable as you type; **F2** (or `?`
//!   when the list is not filtering) the shortcut sheet.
//! - **Ctrl+K** opens the actions for the *region*, the file list as a
//!   whole, in a modal menu: there are no item actions here.
//! - Breadcrumbs above, and a status bar below: a mode badge, a spinner,
//!   a note the commands change, and key hints read from the keymap.
//!
//! The integration tests (`tests/overlays.rs`) include this file and drive
//! [`app`] headless and in a PTY; the tapes `docs/tapes/palette.tape`,
//! `help.tape` and `statusbar.tape` record it.

use rich_interact::chrome::{Breadcrumbs, StatusBar, StatusItem};
use rich_interact::overlay::{Command, Overlays};
use rich_interact::{Action, Actions, Flow, Outcome, RunOptions, Select};

/// The files listed.
pub const FILES: [&str; 6] = [
    "src/lib.rs",
    "src/overlay.rs",
    "src/chrome.rs",
    "src/compose.rs",
    "README.md",
    "Cargo.toml",
];

/// The whole example: a file list with overlays, breadcrumbs and a status
/// bar.
pub fn app<'a>() -> Overlays<'a, &'static str> {
    let files = Select::new("Open", FILES).height(6);
    let bar = StatusBar::new()
        .left("mode", StatusItem::badge("FILES", "bold black on cyan"))
        .left("work", StatusItem::spinner("dots", "indexing"))
        .left("note", StatusItem::text(format!("{} files", FILES.len())))
        .right("keys", StatusItem::hints(2))
        .right(
            "overlays",
            StatusItem::text("[bold]ctrl+o[/] [dim]commands[/] · [bold]f1[/] [dim]help[/]"),
        );
    let status = bar.handle();
    let crumbs = Breadcrumbs::new(["rs-rich-cli", "crates", "rich-interact"]);
    let noted = status.clone();
    let region = Actions::new()
        .action(Action::menu("refresh", "Refresh the list"))
        .action(Action::menu("hidden", "Show hidden files"));
    Overlays::new(files)
        .breadcrumbs(crumbs)
        .status_bar(bar)
        .region("Files", "rich-interact")
        .actions(region)
        .on_action(move |action, target| {
            noted.set(
                "note",
                StatusItem::text(format!("[green]{}[/] · {}", action.label, target.label)),
            );
            Flow::Continue
        })
        .command(
            Command::new("files.refresh", "Refresh the list").category("files"),
            {
                let status = status.clone();
                move || {
                    status.set("note", StatusItem::text("[green]refreshed[/]"));
                    status.remove("work");
                    Flow::Continue
                }
            },
        )
        .command(
            Command::new("files.quit", "Quit without opening").category("files"),
            || Flow::Cancel,
        )
}

#[allow(dead_code)]
fn main() {
    match rich_interact::run(app(), &RunOptions::default()) {
        Ok(Outcome::Done(file)) => println!("opened {file}"),
        Ok(other) => println!("{other:?}"),
        Err(error) => eprintln!("{error}"),
    }
}

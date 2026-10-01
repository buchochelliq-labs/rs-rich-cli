//! Micro assets in the interactive chrome (0.0.14 workstream 7):
//! `cargo run -p rs-rich-interact --features micro --example micro_showcase`.
//!
//! A list of deploy steps, wrapped in [`Overlays`], with rs-rich-micro's
//! built-in assets everywhere the chrome takes an icon:
//!
//! - the **status bar** shows `status/loading` (animated) beside "deploying"
//!   and `status/success` beside the count ([`StatusItem::micro`]);
//! - the **breadcrumbs** put `dev/package` before the project
//!   ([`Breadcrumbs::icons`]);
//! - each **row** has its step's icon ([`Select::set_icons`]);
//! - the **command palette** (Ctrl+O) shows an icon before each category
//!   ([`Overlays::category_icon`]).
//!
//! The painter draws the assets through [`MicroGraphics`]: Kitty, iTerm2 or
//! Sixel images where the terminal has them, coloured half-blocks on any
//! other colour terminal, the emoji or text fallback elsewhere. The tape
//! `docs/tapes/micro-chrome.tape` records it in a PTY, which has no image
//! protocol: it shows the half-block fallback.

use std::sync::Arc;

use rich_interact::chrome::{Breadcrumbs, StatusBar, StatusItem};
use rich_interact::overlay::{Command, Overlays};
use rich_interact::{EventLoop, Flow, LoopOptions, Outcome, Select, SessionOptions};
use rich_micro::{FallbackPreference, MicroGraphics, MicroRegistry};

/// The steps listed, each with its icon.
pub const STEPS: [(&str, &str); 5] = [
    ("Run the tests", "status/success"),
    ("Fix the flaky one", "dev/bug"),
    ("Merge the branch", "dev/branch"),
    ("Build the package", "dev/package"),
    ("Ship it", "fun/heart"),
];

/// The whole example, drawing assets from `registry`.
pub fn app<'a>(registry: &MicroRegistry) -> Overlays<'a, &'static str> {
    let icon = |name: &str| {
        rich_micro::placeholder(
            registry.resolve(name).expect("a built-in asset"),
            FallbackPreference::Emoji,
        )
    };
    let mut steps = Select::new("Next step", STEPS.map(|(step, _)| step)).height(5);
    steps.set_icons(STEPS.iter().map(|(_, name)| Some(icon(name))).collect());
    let asset = |name: &str| registry.resolve(name).expect("a built-in asset");
    let bar = StatusBar::new()
        .left(
            "work",
            StatusItem::micro(asset("status/loading"), "deploying"),
        )
        .left(
            "done",
            StatusItem::micro(asset("status/success"), "3 checks passed"),
        )
        .right("keys", StatusItem::hints(2))
        .right(
            "overlays",
            StatusItem::text("[bold]ctrl+o[/] [dim]commands[/]"),
        );
    let status = bar.handle();
    let package = icon("dev/package");
    let crumbs = Breadcrumbs::new(["rs-rich-cli", "release", "0.0.14"])
        .icons(move |index, _| (index == 0).then(|| package.clone()));
    Overlays::new(steps)
        .breadcrumbs(crumbs)
        .status_bar(bar)
        .category_icon("deploy", icon("status/loading"))
        .category_icon("debug", icon("dev/bug"))
        .category_icon("select", icon("dev/terminal"))
        .command(
            Command::new("deploy.finish", "Mark the deploy done").category("deploy"),
            {
                let status = status.clone();
                move || {
                    status.remove("work");
                    Flow::Continue
                }
            },
        )
        .command(
            Command::new("debug.quit", "Quit without a step").category("debug"),
            || Flow::Cancel,
        )
}

fn main() {
    let registry = Arc::new(MicroRegistry::builtin());
    let graphics = MicroGraphics::detect(Arc::clone(&registry));
    let mut event_loop =
        match EventLoop::terminal(SessionOptions::default(), LoopOptions::default()) {
            Ok(event_loop) => event_loop,
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        };
    event_loop.graphics(graphics.source());
    let handle = event_loop.mount(app(&registry));
    if let Err(error) = event_loop.run() {
        eprintln!("{error}");
    }
    drop(event_loop);
    print!("{}", graphics.close());
    match handle.take() {
        Some(Outcome::Done(step)) => println!("next: {step}"),
        other => println!("{other:?}"),
    }
}

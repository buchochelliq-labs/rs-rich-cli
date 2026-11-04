//! Indicators: a speed test's two dials, a score in stars you set, and a
//! shelf of items paged sideways, built from intuiTUIve's `dial`,
//! `rating`, `rating_input` and `carousel`.
//!
//!     cargo run -p rs-rich-intuituive --example indicators [-- --frozen]
//!
//! Space starts the test again · Tab moves between your rating and the
//! shelf · ←/→ set the stars or turn the shelf's page · `[` and `]` turn
//! it from anywhere · a click on a star or on `‹ ›` · q quits. `--frozen`
//! shows the test finished, for screenshots that do not move.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{carousel, dial, rating, rating_input};

/// What the test reaches, in Mbps: download, upload.
const RESULT: (f64, f64) = (287.4, 41.8);

/// The items on the shelf: a name and its score out of 5.
const SHELF: [(&str, f64); 9] = [
    ("Lantern", 4.5),
    ("Compass", 4.0),
    ("Kettle", 3.5),
    ("Tent", 4.8),
    ("Rope", 4.2),
    ("Stove", 3.9),
    ("Map", 4.6),
    ("Torch", 2.5),
    ("Flask", 4.1),
];

/// One item's card: its name and its stars.
fn card(index: usize) -> Node {
    let (name, score) = SHELF[index];
    column([
        label(format!("[b]{name}[/]")).fixed(1),
        rating(move || score, 5).fixed(1),
    ])
    .panel("")
    .focusable()
    .name(name)
}

/// The board; `frozen` shows the test finished and stops the clock.
pub fn indicators_app(frozen: bool) -> App {
    App::new(move || {
        // Where the test is, from 0 to 1.
        let progress = signal(if frozen { 1.0f64 } else { 0.0 });
        let at = move |target: f64| {
            // Up fast, then settles: a speed test's sweep.
            let p = progress.get();
            target * (1.0 - (1.0 - p).powi(3))
        };
        if !frozen {
            every(Duration::from_millis(100), move |_| {
                progress.update(|p| *p = (*p + 0.04).min(1.0));
            });
        }
        let mine = signal(4.0);
        let first = signal(0usize);
        let dials = row([
            dial(move || at(RESULT.0), 0.0..=1000.0)
                .unit("Mbps")
                .caption("Download")
                .zones([(100.0, "warn"), (1000.0, "good")]),
            dial(move || at(RESULT.1), 0.0..=100.0)
                .readout(move || format!("{:.1}", at(RESULT.1)))
                .unit("Mbps")
                .caption("Upload")
                .bands(10.0, 50.0, 80.0),
        ])
        .gap(2);
        column([
            label("[b]Speed test[/] [dim]· Space runs it again · q quits").fixed(1),
            dials,
            row([
                label("Your rating ").fixed(12),
                rating_input(mine, 5).label("Your rating").auto(),
            ])
            .fixed(1),
            carousel(|| SHELF.len(), first, card)
                .item_width(14)
                .label("Customers also viewed")
                .fixed(6),
        ])
        .gap(1)
        .on_key("space", move |_| progress.set(0.0))
        .on_key("q", |cx| cx.quit())
    })
}

fn main() -> std::io::Result<()> {
    let frozen = std::env::args().any(|a| a == "--frozen");
    indicators_app(frozen).run()
}

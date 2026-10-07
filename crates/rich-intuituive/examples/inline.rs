//! An inline app: a few rows under the prompt, left in the scrollback when
//! it ends.
//!
//!     cargo run -p rs-rich-intuituive --example inline
//!
//! It counts to 20 with a progress bar, then exits; q stops early.

use std::time::Duration;

use intuituive::prelude::*;

fn main() -> std::io::Result<()> {
    App::new(|| {
        let done = signal(0u32);
        every(Duration::from_millis(100), move |cx| {
            done.update(|d| *d += 1);
            if done.get_untracked() >= 20 {
                cx.quit();
            }
        });
        column([
            text(move || {
                let n = done.get() as usize;
                format!(
                    "[accent]{}[/][muted]{}[/] {n}/20",
                    "━".repeat(n),
                    "━".repeat(20 - n)
                )
            })
            .fixed(1),
            text(move || {
                if done.get() >= 20 {
                    "[good]✓ finished".into()
                } else {
                    "[muted]working… q stops".into()
                }
            })
            .fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
    .inline(2)
    .run()
}

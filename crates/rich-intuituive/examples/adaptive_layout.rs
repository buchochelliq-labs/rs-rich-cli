//! A retained adaptive dashboard, independent of browser and art features.
//! `cargo run -p rs-rich-intuituive --no-default-features --example adaptive_layout`
//! Add `-- --record DIRECTORY` to save deterministic Driver SVGs and text.
use intuituive::prelude::*;
use intuituive::{button, scroll_virtual_with, transport};
use std::time::Duration;

fn dashboard() -> App {
    App::new(|| {
        let draft = signal(String::new());
        let count = signal(20usize);
        let offset = signal(0u64);
        let following = signal(true);
        let clock = transport(Some(Duration::from_secs(120)));
        let cards = grid(
            [],
            [
                column([
                    label("[bold]Draft 海の光[/]").auto(),
                    input(draft).name("draft").fixed(1),
                ]),
                column([
                    label("[bold]Replay timeline[/]").auto(),
                    button("Play / pause", move |_| clock.toggle()).fixed(1),
                    text(move || format!("{} seconds", clock.position().as_secs())).auto(),
                ]),
                column([
                    label("[bold]Long Unicode label: 海と光の記録[/]").auto(),
                    button("Append event", move |_| count.update(|n| *n += 1)).fixed(1),
                ]),
            ],
        )
        .adaptive_columns(24, 3)
        .rows([Size::Auto])
        .gap(1)
        .auto();
        let history = scroll_virtual_with(
            move || count.get(),
            |_| 1,
            |i| label(format!("event {i:03} · retained history")),
            offset,
        )
        .item_key(|i| i as u64)
        .follow_end_with(following)
        .name("history");
        column([
            cards,
            history,
            text(move || {
                format!(
                    "following: {} · Tab controls · End resumes history",
                    following.get()
                )
            })
            .fixed(1),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}
fn main() -> std::io::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "--record") {
        let dir = std::path::PathBuf::from(args.get(2).expect("record directory"));
        std::fs::create_dir_all(&dir)?;
        let mut d = dashboard().driver(160, 50);
        d.settle();
        d.click_on("draft");
        d.type_text("retained draft");
        let focused = d.focused();
        for (w, h) in [(80, 24), (120, 40), (160, 50), (40, 24), (160, 50)] {
            d.resize(w, h);
            d.settle();
            assert_eq!(d.focused(), focused);
            assert!(d.screen().plain().join("\n").contains("retained draft"));
            std::fs::write(
                dir.join(format!("adaptive-{w}x{h}.svg")),
                d.screen().to_svg(&Default::default()),
            )?;
            std::fs::write(
                dir.join(format!("adaptive-{w}x{h}.txt")),
                d.screen().plain().join("\n"),
            )?;
        }
        Ok(())
    } else {
        dashboard().run()
    }
}

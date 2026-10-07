//! A small app with screens, a modal, a grid of cards, background loading
//! and a theme switch.
//!
//!     cargo run -p rs-rich-intuituive --example screens
//!
//! Tab moves between cards, Enter opens a card's detail screen (Esc goes
//! back), r reloads, t switches theme, q asks before quitting.

use std::time::Duration;

use intuituive::prelude::*;

const CARDS: [&str; 6] = ["api", "web", "db", "cache", "queue", "search"];

fn detail(name: &'static str) -> Node {
    // Loads in the background; the screen shows it when it arrives.
    let info = resource(move || {
        std::thread::sleep(Duration::from_millis(400));
        Ok::<_, String>(format!("{name}: 3 replicas, p99 41 ms"))
    });
    column([
        text(move || match info.get() {
            Load::Loading => "[muted]loading…".into(),
            Load::Ready(line) => format!("[good]{line}"),
            Load::Failed(error) => format!("[bad]{error}"),
        })
        .auto()
        .padding(1, 2)
        .panel(name),
        label("[muted]r reloads · esc goes back").fixed(1),
    ])
    .on_key("r", move |_| info.reload())
    .on_key("esc", |cx| cx.pop())
}

fn confirm_quit() -> Node {
    label("Quit? [b]y[/] / [b]n[/]")
        .padding(0, 1)
        .panel("Quit")
        .on_key("y", |cx| cx.quit())
        .on_key("n esc", |cx| cx.pop())
}

fn main() -> std::io::Result<()> {
    App::new(|| {
        let dark = signal(true);
        let cards = CARDS.map(|name| {
            label(format!("[accent]{name}[/]\n[muted]healthy"))
                .padding(0, 1)
                .panel(name)
                .focusable()
                .on_key("enter", move |cx| cx.push(move || detail(name)))
        });
        column([
            label("[b]services[/] · tab moves · enter opens · t theme · q quits").auto(),
            grid([Size::Flex(1); 3], cards)
                .rows([Size::Fixed(4)])
                .gap(1),
        ])
        .on_key("t", move |cx| {
            dark.update(|d| *d = !*d);
            cx.set_theme(if dark.get_untracked() {
                Theme::dark()
            } else {
                Theme::light()
            });
        })
        .on_key("q", |cx| cx.modal(Size::Auto, Size::Auto, confirm_quit))
    })
    .run()
}

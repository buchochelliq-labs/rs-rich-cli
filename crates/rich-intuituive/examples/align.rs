//! Flex alignment: a row of cards placed by `align_items`, `justify`,
//! `wrap` and `align_content`, each changed with a key.
//!
//!     cargo run -p rs-rich-intuituive --example align
//!
//! a cycles align-items · j cycles justify · w turns wrapping on and off ·
//! c cycles align-content (while wrapping) · + and - add and remove cards ·
//! q quits. The test card keeps to the end of its line with `align_self`,
//! whatever align-items says.

use intuituive::prelude::*;
use intuituive::AlignContent;

const ALIGNS: [Align; 4] = [Align::Stretch, Align::Start, Align::Center, Align::End];
const JUSTIFIES: [Justify; 6] = [
    Justify::Start,
    Justify::Center,
    Justify::End,
    Justify::SpaceBetween,
    Justify::SpaceAround,
    Justify::SpaceEvenly,
];
const CONTENTS: [AlignContent; 4] = [
    AlignContent::Stretch,
    AlignContent::Start,
    AlignContent::Center,
    AlignContent::SpaceBetween,
];

// --8<-- [start:cards]
/// The first `count` cards: different widths and heights, so alignment
/// shows. The test card keeps to the end of its line with `align_self`,
/// whatever the row's `align_items` says.
fn cards(count: usize) -> Vec<Node> {
    [
        ("build", "41s", 14),
        ("test", "2m 03s", 16),
        ("lint", "9s", 12),
        ("docs", "1m 12s", 16),
        ("bench", "4m 40s", 12),
        ("deploy", "queued", 14),
    ]
    .into_iter()
    .take(count)
    .enumerate()
    .map(|(i, (name, time, width))| {
        let lines = if i % 2 == 0 { 1 } else { 3 };
        let body = format!("{time}{}", "\n·".repeat(lines - 1));
        let card = label(body).panel(name).fixed(width);
        if name == "test" {
            card.align_self(Align::End)
        } else {
            card
        }
    })
    .collect()
}
// --8<-- [end:cards]

fn main() -> std::io::Result<()> {
    App::new(|| {
        let align = signal(0usize);
        let justify = signal(0usize);
        let wrap = signal(false);
        let content = signal(0usize);
        let count = signal(3usize);
        // --8<-- [start:board]
        let board = move || {
            let mut cards = row(cards(count.get()))
                .gap(1)
                .align_items(ALIGNS[align.get()])
                .justify(JUSTIFIES[justify.get()]);
            if wrap.get() {
                cards = cards.wrap().align_content(CONTENTS[content.get()]);
            }
            cards.panel("row")
        };
        // --8<-- [end:board]
        column([
            switch(
                move || {
                    (
                        align.get(),
                        justify.get(),
                        wrap.get(),
                        content.get(),
                        count.get(),
                    )
                },
                move |_| board(),
            ),
            text!(
                "[b]items[/] {:?} · [b]justify[/] {:?} · [b]wrap[/] {} · [b]lines[/] {:?}",
                ALIGNS[align.get()],
                JUSTIFIES[justify.get()],
                wrap.get(),
                CONTENTS[content.get()]
            )
            .fixed(1),
            label("[muted]a items · j justify · w wrap · c lines · + - cards · q quits").fixed(1),
        ])
        .on_key("a", move |_| align.update(|i| *i = (*i + 1) % ALIGNS.len()))
        .on_key("j", move |_| {
            justify.update(|i| *i = (*i + 1) % JUSTIFIES.len())
        })
        .on_key("w", move |_| wrap.update(|on| *on = !*on))
        .on_key("c", move |_| {
            content.update(|i| *i = (*i + 1) % CONTENTS.len())
        })
        .on_key("+", move |_| count.update(|n| *n = (*n + 1).min(6)))
        .on_key("-", move |_| {
            count.update(|n| *n = n.saturating_sub(1).max(1))
        })
        .on_key("q", |cx| cx.quit())
    })
    .run()
}

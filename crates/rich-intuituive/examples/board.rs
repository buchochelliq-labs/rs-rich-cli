//! A sprint board: past sprints in a tree that loads each one when it is
//! opened, and this sprint's cards in three lanes, dragged from one to
//! another, each with a tooltip.
//!
//!     cargo run -p rs-rich-intuituive --example board
//!
//! In the tree, → opens a sprint (its tickets load on another thread, as
//! from a tracker over the network; the oldest sprint's archive is
//! offline). Drag a card to another lane to move it; Esc cancels a drag.
//! Rest the pointer on a card, or press F1 on the focused one, for its
//! tooltip. Tab moves between the tree and the cards · q quits.
//!
//! Built on `tree_lazy` (with `LazyItem`), `.draggable(value)` and
//! `.on_drop(|value: &T, cx| …)`, and `.tooltip(markup)`.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::widgets::{tree_lazy, LazyItem};

/// A card on the board.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub id: u32,
    pub title: &'static str,
    pub owner: &'static str,
    pub note: &'static str,
}

/// What a drag carries: the card's id. Only the lanes take it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CardId(pub u32);

/// The lanes, left to right.
pub const LANES: [&str; 3] = ["To do", "Doing", "Done"];

const fn card(id: u32, title: &'static str, owner: &'static str, note: &'static str) -> Card {
    Card {
        id,
        title,
        owner,
        note,
    }
}

/// The board this sprint starts with: a list of cards per lane.
pub fn sprint() -> Vec<Vec<Card>> {
    vec![
        vec![
            card(
                1,
                "Add Apple Pay",
                "Priya",
                "Behind the payments flag until legal signs off.",
            ),
            card(
                2,
                "Load test the cart",
                "Sam",
                "Target 2,000 checkouts a minute.",
            ),
            card(
                3,
                "Rotate API keys",
                "Lena",
                "Stripe and the shipping provider.",
            ),
        ],
        vec![
            card(
                4,
                "Fix checkout timeout",
                "Sam",
                "Seen when the cart has over 40 items.",
            ),
            card(
                5,
                "Postgres 17 upgrade",
                "Lena",
                "Replica first, then a failover.",
            ),
        ],
        vec![card(
            6,
            "Cache product images",
            "Priya",
            "Served from the CDN since Monday.",
        )],
    ]
}

/// A past sprint's tickets, as the tracker would send them; the oldest
/// sprint's archive is offline. Called on a worker thread.
pub fn tickets(sprint: &str) -> Result<Vec<LazyItem>, String> {
    let titles: &[&str] = match sprint {
        "Sprint 41" => &["Search by SKU", "Gift cards", "Faster product pages"],
        "Sprint 40" => &["Order history export", "Two-factor sign-in"],
        "Sprint 39" => return Err("archive offline".into()),
        _ => &[],
    };
    Ok(titles
        .iter()
        .map(|title| LazyItem::leaf(format!("{sprint} / {title}"), format!("✓ {title}")))
        .collect())
}

/// The app; `delay` is how long the tracker takes to answer.
pub fn board_app(delay: Duration) -> App {
    App::new(move || {
        let lanes = signal(sprint());
        let selected = signal(None::<String>);
        let last = signal(String::from("drag a card to another lane"));

        let roots = || {
            vec![
                LazyItem::branch("Sprint 41", "Sprint 41"),
                LazyItem::branch("Sprint 40", "Sprint 40"),
                LazyItem::branch("Sprint 39", "Sprint 39"),
            ]
        };
        // --8<-- [start:lazy]
        let history = tree_lazy(
            roots,
            move |key: String| {
                std::thread::sleep(delay);
                tickets(&key)
            },
            selected,
        )
        .autofocus()
        .panel("Past sprints");
        // --8<-- [end:lazy]

        // --8<-- [start:lanes]
        let lane = move |i: usize| {
            each(
                move || lanes.with(|l| l[i].iter().map(|c| c.id).collect()),
                move |id| card_node(lanes, id),
            )
            .panel(LANES[i])
            .flex(1)
            .on_drop(move |&CardId(id): &CardId, _| {
                let Some((from, at)) = find(&lanes.get_untracked(), id) else {
                    return;
                };
                if from == i {
                    return;
                }
                let title = lanes.with_untracked(|l| l[from][at].title);
                lanes.update(|l| {
                    let card = l[from].remove(at);
                    l[i].push(card);
                });
                last.set(format!("[b]{}[/] moved to {}", escape(title), LANES[i]));
            })
        };
        // --8<-- [end:lanes]

        let status = text(move || {
            let picked = selected.get().unwrap_or_else(|| "nothing".into());
            format!(
                "{}  [muted]tree: {}[/]  [dim]F1 tooltip · q quits[/]",
                last.get(),
                escape(&picked)
            )
        });

        column([
            row([history.fixed(28), lane(0), lane(1), lane(2)]).flex(1),
            status.fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    })
}

/// Which lane holds card `id`, and where in it.
fn find(lanes: &[Vec<Card>], id: u32) -> Option<(usize, usize)> {
    lanes
        .iter()
        .enumerate()
        .find_map(|(lane, cards)| cards.iter().position(|c| c.id == id).map(|at| (lane, at)))
}

// --8<-- [start:card]
/// One card: dragged by the mouse, with its note as a tooltip.
fn card_node(lanes: Signal<Vec<Vec<Card>>>, id: u32) -> Node {
    let card = lanes.with_untracked(|l| {
        let (lane, at) = find(l, id).expect("a card on the board");
        l[lane][at].clone()
    });
    label(format!("• {}", escape(card.title)))
        .draggable(CardId(id))
        .tooltip(format!(
            "[b]{}[/]\n{}\n[i]{}[/]",
            escape(card.title),
            escape(card.note),
            card.owner
        ))
        .focusable()
        .focus_style("reverse")
        .fixed(1)
}
// --8<-- [end:card]

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    board_app(Duration::from_millis(1200)).run()
}

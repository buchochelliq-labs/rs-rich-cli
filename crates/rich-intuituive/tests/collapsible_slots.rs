//! `collapsible_with`: a title that follows state, a status beside it and
//! a summary while closed (#804), the parts of a tool call's card.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::{Status, collapsible, collapsible_with};

#[derive(Clone, Copy)]
struct Card {
    command: Signal<String>,
    done: Signal<Option<i32>>,
    last: Signal<String>,
    open: Signal<bool>,
}

fn card() -> (App, Rc<Cell<Option<Card>>>) {
    let out = Rc::new(Cell::new(None));
    let seen = out.clone();
    let app = App::new(move || {
        let card = Card {
            command: signal("ls".to_string()),
            done: signal(None),
            last: signal("b.txt".to_string()),
            open: signal(false),
        };
        seen.set(Some(card));
        column([
            collapsible_with(
                move || format!("[b]bash[/] {}", card.command.get()),
                card.open,
                label("a.txt\nb.txt"),
            )
            .status(move || match card.done.get() {
                None => Status::Spinner("line"),
                Some(0) => Status::Markup("✓".into()),
                Some(code) => Status::Markup(format!("✗ {code}")),
            })
            .summary(move || card.last.get())
            .symbols("▶", "▼"),
            label("below"),
        ])
    });
    (app, out)
}

fn plain(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect()
}

#[test]
fn the_title_follows_a_signal_and_only_the_header_draws_again() {
    let (app, card) = card();
    let mut driver = app.driver(20, 4);
    driver.settle();
    let card = card.get().unwrap();
    assert_eq!(driver.screen().plain()[0], "▶ bash ls          -");
    let seen = driver.stats().frames;
    card.command.set("cat a.txt".into());
    driver.settle();
    assert!(plain(&driver)[0].starts_with("▶ bash cat a.txt"));
    let damage = driver.damage_since(seen).unwrap();
    assert!(!damage.is_empty());
    assert!(
        damage.iter().all(|rect| rect.y == 0 && rect.height == 1),
        "{damage:?}"
    );
}

#[test]
fn the_spinner_turns_on_the_app_clock_then_becomes_a_mark() {
    let (app, card) = card();
    let mut driver = app.driver(20, 4);
    driver.settle();
    let card = card.get().unwrap();
    let last = |driver: &intuituive::Driver| driver.screen().plain()[0].chars().last().unwrap();
    assert_eq!(last(&driver), '-');
    // rich's `line` turns every 130 ms: - \ | /
    driver.advance(Duration::from_millis(130));
    assert_eq!(last(&driver), '\\');
    driver.advance(Duration::from_millis(130));
    assert_eq!(last(&driver), '|');
    assert!(driver.accessibility()[0].state.busy);
    card.done.set(Some(0));
    driver.settle();
    assert_eq!(last(&driver), '✓');
    assert!(!driver.accessibility()[0].state.busy);
    card.done.set(Some(2));
    driver.settle();
    assert!(driver.screen().plain()[0].ends_with("✗ 2"));
    assert!(
        driver.accessibility()[0]
            .describe()
            .starts_with("bash ls, ✗ 2"),
        "{}",
        driver.accessibility()[0].describe()
    );
}

#[test]
fn the_summary_shows_while_closed() {
    let (app, card) = card();
    let mut driver = app.driver(20, 4);
    driver.settle();
    let card = card.get().unwrap();
    assert_eq!(plain(&driver)[1], "  b.txt");
    assert_eq!(plain(&driver)[2], "below");
    card.last.set("c.txt\nmore".into());
    driver.settle();
    assert_eq!(plain(&driver)[1], "  c.txt");
    // Enter opens it: the child in place of the summary.
    driver.press("enter");
    assert!(card.open.get_untracked());
    assert!(plain(&driver)[0].starts_with("▼ bash ls"));
    assert_eq!(plain(&driver)[1], "a.txt");
    assert_eq!(plain(&driver)[2], "b.txt");
    assert_eq!(plain(&driver)[3], "below");
    assert!(driver.accessibility()[0].describe().contains("expanded"));
    // Space closes it.
    driver.press("space");
    assert_eq!(plain(&driver)[1], "  c.txt");
    assert!(driver.accessibility()[0].describe().contains("collapsed"));
}

#[test]
fn a_plain_collapsible_is_as_it_was() {
    let app = App::new(|| collapsible("Advanced", signal(false), label("inside")));
    let mut driver = app.driver(20, 2);
    driver.settle();
    assert_eq!(plain(&driver), ["▸ Advanced", ""]);
    driver.press("enter");
    assert_eq!(plain(&driver), ["▾ Advanced", "inside"]);
}

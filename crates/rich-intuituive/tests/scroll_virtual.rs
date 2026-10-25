//! `scroll_virtual`: a scroll view that builds only the items near the
//! view, on the headless driver.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::interact::{Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::scroll_virtual_with;

/// The text of each row, without the bar's column.
fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|line| {
            let mut line: Vec<char> = line.chars().collect();
            line.pop();
            line.into_iter().collect::<String>().trim_end().to_string()
        })
        .collect()
}

/// The bar's column.
fn bar(driver: &intuituive::Driver) -> String {
    driver
        .screen()
        .plain()
        .iter()
        .map(|line| line.chars().last().unwrap_or(' '))
        .collect()
}

#[test]
fn a_million_items_cost_a_screenful() {
    let builds = Rc::new(Cell::new(0usize));
    let count = builds.clone();
    let app = App::new(move || {
        let count = count.clone();
        scroll_virtual(
            || 1_000_000,
            |_| 1,
            move |i| {
                count.set(count.get() + 1);
                label(format!("item {i}"))
            },
        )
    });
    let mut driver = app.driver(20, 10);
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 0");
    assert_eq!(rows(&driver)[9], "item 9");
    // The view and a screen below it.
    assert!(builds.get() <= 21, "{} built", builds.get());
    // A row down is within what is built: nothing new.
    let before = builds.get();
    driver.press("down");
    assert_eq!(rows(&driver)[0], "item 1");
    assert_eq!(builds.get(), before);
    driver.press("end");
    assert_eq!(rows(&driver)[9], "item 999999");
    assert_eq!(rows(&driver)[0], "item 999990");
    assert!(
        builds.get() - before <= 21,
        "{} built",
        builds.get() - before
    );
    // Past what `scroll` reaches (4000 rows), and back.
    driver.press("home");
    assert_eq!(rows(&driver)[0], "item 0");
}

#[test]
fn keys_and_the_wheel_scroll_it() {
    let offset = Rc::new(Cell::new(None));
    let seen = offset.clone();
    let app = App::new(move || {
        let at = signal(0u64);
        seen.set(Some(at));
        scroll_virtual_with(|| 100, |_| 1, |i| label(format!("item {i}")), at)
    });
    let mut driver = app.driver(20, 10);
    driver.settle();
    let at = offset.get().expect("the offset");
    driver.press("down");
    assert_eq!(rows(&driver)[0], "item 1");
    driver.press("pagedown");
    assert_eq!(rows(&driver)[0], "item 10");
    driver.send(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 3, 3)));
    assert_eq!(rows(&driver)[0], "item 13");
    assert_eq!(at.get_untracked(), 13);
    driver.send(Event::Mouse(Mouse::new(MouseKind::ScrollUp, 3, 3)));
    assert_eq!(rows(&driver)[0], "item 10");
    driver.press("up");
    assert_eq!(rows(&driver)[0], "item 9");
    // Set from code: past the end is the end.
    at.set(u64::MAX);
    driver.settle();
    assert_eq!(rows(&driver)[9], "item 99");
    assert_eq!(at.get_untracked(), 90);
    driver.press("home");
    assert_eq!(rows(&driver)[0], "item 0");
}

#[test]
fn the_bar_shows_where_the_view_is() {
    let app = App::new(|| scroll_virtual(|| 100, |_| 1, |i| label(format!("item {i}"))));
    let mut driver = app.driver(20, 10);
    driver.settle();
    let thumb = |driver: &intuituive::Driver| -> Vec<usize> {
        bar(driver)
            .chars()
            .enumerate()
            .filter(|(_, c)| *c == '┃')
            .map(|(i, _)| i)
            .collect()
    };
    assert_eq!(thumb(&driver), [0], "{:?}", bar(&driver));
    driver.press("end");
    assert_eq!(thumb(&driver), [9], "{:?}", bar(&driver));
    // Halfway: the thumb is in the middle.
    driver.press("home");
    for _ in 0..5 {
        driver.press("pagedown");
    }
    let middle = thumb(&driver);
    assert!(
        !middle.is_empty() && middle.iter().all(|&i| (3..=6).contains(&i)),
        "{:?}",
        bar(&driver)
    );
    // Content that fits has no bar.
    let app = App::new(|| scroll_virtual(|| 3, |_| 1, |i| label(format!("item {i}"))));
    let mut driver = app.driver(20, 10);
    driver.settle();
    assert_eq!(bar(&driver).trim(), "");
}

#[test]
fn estimates_are_corrected_once_items_are_drawn() {
    // Each item is two rows, estimated at one.
    let app = App::new(|| scroll_virtual(|| 50, |_| 1, |i| label(format!("item {i}\n  more {i}"))));
    let mut driver = app.driver(20, 10);
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 0");
    assert_eq!(rows(&driver)[1], "  more 0");
    assert_eq!(rows(&driver)[8], "item 4");
    driver.press("end");
    assert_eq!(rows(&driver)[9], "  more 49");
    assert_eq!(rows(&driver)[8], "item 49");
}

#[test]
fn the_top_item_keeps_its_row_when_items_are_inserted_above() {
    let items = Rc::new(Cell::new(None));
    let seen = items.clone();
    let app = App::new(move || {
        let list = signal((0..100u64).collect::<Vec<_>>());
        seen.set(Some(list));
        scroll_virtual(
            move || list.with(Vec::len),
            |_| 1,
            move |i| label(format!("item {}", list.with(|l| l[i]))),
        )
        .item_key(move |i| list.with_untracked(|l| l[i]))
    });
    let mut driver = app.driver(20, 10);
    driver.settle();
    let list = items.get().expect("the list");
    driver.press("pagedown pagedown pagedown");
    assert_eq!(rows(&driver)[0], "item 27");
    // Five new items at the front: the view stays on item 27.
    list.update(|l| {
        for k in 0..5 {
            l.insert(0, 1000 + k);
        }
    });
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 27");
    assert_eq!(rows(&driver)[9], "item 36");
    // And one removed above.
    list.update(|l| {
        l.remove(0);
    });
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 27");
}

#[test]
fn the_top_item_keeps_its_row_when_items_above_grow() {
    let tall = Rc::new(Cell::new(None));
    let seen = tall.clone();
    let app = App::new(move || {
        let tall = signal(false);
        seen.set(Some(tall));
        scroll_virtual(
            || 100,
            |_| 1,
            move |i| {
                if i == 25 {
                    text(move || {
                        if tall.get() {
                            "item 25\nmore\nmore".to_string()
                        } else {
                            "item 25".to_string()
                        }
                    })
                } else {
                    label(format!("item {i}"))
                }
            },
        )
    });
    let mut driver = app.driver(20, 10);
    driver.settle();
    driver.press("pagedown pagedown pagedown");
    assert_eq!(rows(&driver)[0], "item 27");
    tall.get().expect("the switch").set(true);
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 27");
    driver.press("up up");
    assert_eq!(rows(&driver)[0], "more");
}

#[test]
fn near_the_end_fires_once_until_the_length_grows() {
    let calls = Rc::new(Cell::new(0));
    let len = Rc::new(Cell::new(None));
    let (count, seen) = (calls.clone(), len.clone());
    let app = App::new(move || {
        let n = signal(30usize);
        seen.set(Some(n));
        let count = count.clone();
        scroll_virtual(move || n.get(), |_| 1, |i| label(format!("item {i}")))
            .on_near_end(5, move |_| count.set(count.get() + 1))
    });
    let mut driver = app.driver(20, 10);
    driver.settle();
    assert_eq!(calls.get(), 0);
    driver.press("pagedown pagedown");
    assert_eq!(calls.get(), 1);
    // No more while the load is in flight.
    driver.press("pageup end down up end");
    assert_eq!(calls.get(), 1);
    // The load lands: the view is no longer near the end.
    len.get().expect("the length").set(60);
    driver.settle();
    assert_eq!(calls.get(), 1);
    driver.press("end");
    assert_eq!(calls.get(), 2);
    assert_eq!(rows(&driver)[9], "item 59");
}

#[test]
fn a_feed_loads_more_as_it_scrolls() {
    let app = App::new(|| {
        let n = signal(20usize);
        scroll_virtual(move || n.get(), |_| 1, |i| label(format!("post {i}")))
            .on_near_end(3, move |_| n.update(|n| *n += 20))
    });
    let mut driver = app.driver(20, 5);
    driver.settle();
    for _ in 0..20 {
        driver.press("pagedown");
    }
    // Twenty pages of four rows: the feed grew ahead of the view.
    let last: usize = rows(&driver)[4]
        .trim_start_matches("post ")
        .parse()
        .expect("a post");
    assert!(last >= 80, "{last}");
}

#[test]
fn stand_ins_while_scrolling_fast_and_full_items_at_rest() {
    let app = App::new(|| {
        scroll_virtual(|| 1000, |_| 1, |i| label(format!("item {i}"))).stand_in(|_| label("░░░░"))
    });
    let mut driver = app.driver(20, 5);
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 0");
    // A slow move builds full items.
    driver.press("down");
    assert_eq!(rows(&driver)[4], "item 5");
    // Pages in quick succession: stand-ins for the items not built yet,
    // while those built already stay.
    driver.press("pagedown pagedown pagedown pagedown");
    assert_eq!(rows(&driver)[0], "item 17");
    assert_eq!(rows(&driver)[4], "░░░░");
    // At rest, full items.
    driver.advance(Duration::from_millis(200));
    assert_eq!(rows(&driver)[4], "item 21");
}

#[test]
fn an_empty_list_draws_nothing_and_asks_for_more() {
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let app = App::new(move || {
        let count = count.clone();
        scroll_virtual(|| 0, |_| 1, |i| label(format!("item {i}")))
            .on_near_end(1, move |_| count.set(count.get() + 1))
    });
    let mut driver = app.driver(20, 3);
    driver.settle();
    assert_eq!(rows(&driver).join(""), "");
    assert_eq!(calls.get(), 1);
}

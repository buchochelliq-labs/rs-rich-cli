//! The generic core components the domain plugins build on: Miller columns
//! over any tree, a job list with progress, a streaming fuzzy finder over
//! any items, and a preview pane keyed by any value. Each is driven here
//! with no file in sight.

use std::time::{Duration, Instant};

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::miller::MillerSource;
use intuituive::prelude::*;
use intuituive::{
    finder_of, jobs, jobs_panel, miller_of, preview_of, preview_of_with, Driver, PreviewOfOptions,
};

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

/// Settle until `done` holds of the screen, or panic after a few seconds.
fn until(driver: &mut Driver, done: impl Fn(&[String]) -> bool) -> Vec<String> {
    let start = Instant::now();
    loop {
        driver.settle();
        let screen = rows(driver);
        if done(&screen) {
            return screen;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{screen:#?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A small category tree, keyed by a path of names joined by `/`.
struct Shop;

impl MillerSource for Shop {
    type Key = String;

    fn items(&self, at: &String) -> Vec<String> {
        let names: &[&str] = match at.as_str() {
            "" => &["books", "music", "tools"],
            "books" => &["fiction", "poetry"],
            "books/fiction" => &["novels", "stories"],
            "music" => &["jazz"],
            _ => &[],
        };
        names.iter().map(|n| n.to_string()).collect()
    }

    fn child(&self, at: &String, name: &str) -> Option<String> {
        let key = if at.is_empty() {
            name.to_string()
        } else {
            format!("{at}/{name}")
        };
        // A level with items opens; the rest are leaves.
        (!self.items(&key).is_empty()).then_some(key)
    }

    fn parent(&self, at: &String) -> Option<(String, String)> {
        if at.is_empty() {
            return None;
        }
        Some(match at.rsplit_once('/') {
            Some((up, name)) => (up.to_string(), name.to_string()),
            None => (String::new(), at.clone()),
        })
    }
}

fn shop() -> App {
    App::new(|| {
        let columns = miller_of(Shop, String::new());
        columns.navigation(column([
            columns.view().flex(1),
            text(move || {
                let hovered = columns.hovered.get().unwrap_or_default();
                format!("{} | {hovered}", columns.at.get())
            })
            .fixed(1),
        ]))
    })
}

fn status(driver: &Driver) -> String {
    rows(driver).last().cloned().unwrap_or_default()
}

#[test]
fn miller_columns_browse_any_tree_and_remember() {
    let mut driver = shop().driver(48, 4);
    driver.settle();
    // Parent (none) | current | the hovered `books` previewed.
    let screen = rows(&driver);
    assert!(
        screen[0].contains("books") && screen[0].contains("fiction"),
        "{screen:?}"
    );
    assert_eq!(status(&driver), " | books");
    driver.press("l l");
    assert_eq!(status(&driver), "books/fiction | novels");
    driver.press("j h");
    assert_eq!(status(&driver), "books | fiction");
    // Back in: the item last hovered there is hovered again.
    driver.press("l");
    assert_eq!(status(&driver), "books/fiction | stories");
    driver.press("h h G");
    assert_eq!(status(&driver), " | tools");
    // A leaf: `l` stays.
    driver.press("l");
    assert_eq!(status(&driver), " | tools");
}

#[test]
fn miller_clicks_in_the_parent_and_the_preview_navigate() {
    let mut driver = shop().driver(48, 6);
    driver.settle();
    driver.press("l");
    assert_eq!(status(&driver), "books | fiction");
    // The parent column (6 wide at 48) lists the top: books, music, tools.
    // A click on `music` goes into it.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        1,
        1,
    )));
    driver.settle();
    assert_eq!(status(&driver), "music | jazz");
    // Below the parent's items: up a level, hovering where we were.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        1,
        3,
    )));
    driver.settle();
    assert_eq!(status(&driver), " | music");
    // A click in the preview enters the hovered item there.
    driver.press("k");
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        40,
        1,
    )));
    driver.settle();
    assert_eq!(status(&driver), "books | poetry");
}

#[test]
fn jobs_report_progress_and_failures_stay_listed() {
    let app = App::new(|| {
        let list = jobs::<f64>();
        let said = signal(String::new());
        column([jobs_panel(list).flex(1), text!("{said}").fixed(1)])
            .on_key("o", move |_| {
                list.run(
                    "Upload",
                    |_, progress| {
                        progress(&0.5);
                        Ok::<_, String>("uploaded")
                    },
                    move |done, _| said.set(done.unwrap().to_string()),
                );
            })
            .on_key("f", move |_| {
                list.run(
                    "Export",
                    |_, _| Err::<(), _>("disk full".to_string()),
                    move |_, _| said.set("failed".into()),
                );
            })
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(50, 3);
    driver.settle();
    assert_eq!(rows(&driver)[0].trim(), "No tasks");
    // A clean job leaves the list.
    driver.press("o");
    let screen = until(&mut driver, |s| s[2] == "uploaded");
    assert_eq!(screen[0].trim(), "No tasks");
    // A failed one stays, with why; `c` clears it.
    driver.press("f");
    let screen = until(&mut driver, |s| s[2] == "failed");
    assert_eq!(screen[0], "failed  Export: disk full");
    driver.press("c");
    assert_eq!(rows(&driver)[0].trim(), "No tasks");
}

#[test]
fn a_running_job_shows_its_bar_and_cancels() {
    let app = App::new(|| {
        let list = jobs::<f64>();
        let said = signal(String::new());
        column([jobs_panel(list).flex(1), text!("{said}").fixed(1)]).on_key("w", move |_| {
            list.run(
                "Wait",
                |cancel, progress| {
                    progress(&0.25);
                    while !cancel.is_cancelled() {
                        std::thread::sleep(Duration::from_millis(2));
                    }
                },
                move |_, _| said.set("cancelled".into()),
            );
        })
    });
    let mut driver = app.driver(50, 2);
    driver.press("w");
    let screen = until(&mut driver, |s| s[0].contains("25%"));
    assert_eq!(screen[0], "━━━━━━━━━━━━━━━━  25%  Wait");
    driver.press("x");
    let screen = until(&mut driver, |s| s[1] == "cancelled");
    assert_eq!(screen[0].trim(), "No tasks");
}

#[test]
fn the_finder_streams_any_items_and_picks_one() {
    #[derive(Clone)]
    struct Contact {
        name: String,
        id: u32,
    }
    let app = App::new(|| {
        let picked = signal(0u32);
        let contacts = (0..500).map(|id| Contact {
            name: format!("person {id}"),
            id,
        });
        column([
            finder_of(contacts, |c| c.name.clone(), move |c, _| picked.set(c.id)).flex(1),
            text!("picked {picked}").fixed(1),
        ])
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(30, 6);
    until(&mut driver, |s| s[1].trim() == "500/500");
    driver.type_text("son 42");
    let screen = rows(&driver);
    assert!(screen[2].ends_with("person 42"), "{screen:?}");
    driver.press("enter");
    assert_eq!(rows(&driver)[5], "picked 42");
}

#[test]
fn a_preview_drops_late_results_and_clears_without_a_key() {
    let app = App::new(|| {
        let key = signal(Some(1u32));
        preview_of_with(
            move |_| key.get(),
            |n| {
                // The first key takes a while.
                if *n == 1 {
                    std::thread::sleep(Duration::from_millis(300));
                }
                format!("value {n}")
            },
            |shown| text(move || shown.get()),
            PreviewOfOptions::new(),
        )
        .on_key("n", move |_| key.set(Some(2)))
        .on_key("x", move |_| key.set(None))
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    driver.press("n");
    until(&mut driver, |s| s[0] == "value 2");
    // The slow first value arrives later and is dropped.
    std::thread::sleep(Duration::from_millis(400));
    driver.settle();
    assert_eq!(rows(&driver)[0], "value 2");
    driver.press("x");
    assert_eq!(rows(&driver)[0], "");
}

#[test]
fn a_preview_loads_for_its_width() {
    let app = App::new(|| {
        preview_of(
            Some,
            |width| "#".repeat(*width as usize),
            |shown| text(move || shown.get()),
        )
    })
    .wait_for_tasks(true);
    let mut driver = app.driver(7, 1);
    driver.settle();
    assert_eq!(rows(&driver)[0], "#######");
}

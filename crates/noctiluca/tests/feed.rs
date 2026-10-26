//! Long pages and infinite feeds on intuiTUIve's driver, over the scripted
//! engine: the view builds only rows near it, Chrome's scroll follows
//! ours, the page is asked for more near its end, and the row in view
//! keeps its place when the page grows above it. The feed is a generated
//! snapshot standing for `tests/fixtures/feed.html`, whose
//! IntersectionObserver sentinel appends posts (the live test reads the
//! real page).

use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, View};
use noctiluca::engine::{EngineEvent, PageAction};
use noctiluca::testing::{feed_snapshot, feed_snapshot_of, FakeEngine, FakeState};
use rich_intuituive::Driver;

const URL: &str = "https://fixture.example/feed.html";

/// A feed of `posts` posts; each scroll to the bottom appends 30 more, as
/// the fixture's sentinel does, and pings.
fn feed(posts: usize) -> FakeEngine {
    let engine = FakeEngine::from_dir(&std::env::temp_dir().join("noctiluca-no-snapshots"))
        .unwrap_or_else(|_| empty())
        .on_act(|action, state: &mut FakeState| {
            let bottom = (state.content_height() - state.viewport_height).max(0.0);
            if matches!(action, PageAction::ScrollTo(_) | PageAction::Wheel(_))
                && state.scroll_y >= bottom - 1.0
            {
                let posts = state.pages["feed"].dom["documents"][0]["contentHeight"]
                    .as_f64()
                    .map_or(0, |h| ((h - 60.0) / 36.0) as usize);
                state
                    .pages
                    .insert("feed".into(), feed_snapshot(URL, posts + 30));
                state.events.push(EngineEvent::Changed);
            }
        });
    engine
        .state
        .lock()
        .unwrap()
        .pages
        .insert("feed".into(), feed_snapshot(URL, posts));
    engine
}

fn empty() -> FakeEngine {
    let dir = std::env::temp_dir().join(format!("noctiluca-feed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    FakeEngine::from_dir(&dir).unwrap()
}

fn options() -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some(URL.to_string()),
        view: Some(View::Projection),
        images: Some(noctiluca::config::Images::Ansi),
    }
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// Step the app's clock until `ok` holds for the screen, for up to ten
/// seconds of real time (the tab runs on a thread of its own).
fn wait_until(driver: &mut Driver, what: &str, ok: impl Fn(&str) -> bool) {
    let start = Instant::now();
    loop {
        driver.advance(Duration::from_millis(40));
        if ok(&screen(driver)) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{what} never showed:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn scrolling_to_the_end_of_a_feed_loads_more() {
    let engine = feed(30);
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options()).driver(60, 20);
    wait_until(&mut driver, "the feed", |s| s.contains("Post 1"));
    // Past the first thirty posts: the view asks the page for more, the
    // page appends them and pings, and they are read.
    for last in ["Post 60", "Post 90"] {
        let start = Instant::now();
        while !screen(&driver).contains(last) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{last} never showed:\n{}",
                screen(&driver)
            );
            driver.press("G");
            driver.advance(Duration::from_millis(40));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let actions = state.lock().unwrap().actions.clone();
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, PageAction::ScrollTo(y) if *y == f64::MAX)),
        "{actions:?}"
    );
}

/// Step the app's clock until the engine was asked for an action `ok`
/// accepts (the tab runs it on its own thread).
fn wait_for_action(
    driver: &mut Driver,
    state: &std::sync::Arc<std::sync::Mutex<FakeState>>,
    what: &str,
    ok: impl Fn(&PageAction) -> bool,
) {
    let start = Instant::now();
    loop {
        driver.advance(Duration::from_millis(40));
        let actions = state.lock().unwrap().actions.clone();
        if actions.iter().any(&ok) {
            state.lock().unwrap().actions.clear();
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no {what}: {actions:?}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn chromes_scroll_follows_ours() {
    let engine = feed(200);
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options()).driver(60, 20);
    wait_until(&mut driver, "the feed", |s| s.contains("Post 1"));
    // A few rows down: a wheel turn, once the view rests.
    driver.press("j j j j");
    wait_for_action(
        &mut driver,
        &state,
        "wheel turn",
        |a| matches!(a, PageAction::Wheel(d) if *d > 0.0),
    );
    // The end: Chrome goes to its bottom (asked for more there); then
    // back to the top, a jump.
    driver.press("G");
    wait_for_action(
        &mut driver,
        &state,
        "jump down",
        |a| matches!(a, PageAction::ScrollTo(y) if *y > 1000.0),
    );
    driver.press("home");
    wait_for_action(&mut driver, &state, "jump up", |a| {
        *a == PageAction::ScrollTo(0.0)
    });
}

#[test]
fn the_row_in_view_keeps_its_place_when_the_page_grows_above_it() {
    let engine = feed(60);
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options()).driver(60, 20);
    wait_until(&mut driver, "the feed", |s| s.contains("Post 1"));
    // Down until Post 20 is the first row.
    for _ in 0..200 {
        if screen(&driver)
            .lines()
            .nth(1)
            .is_some_and(|l| l.contains("Post 20"))
        {
            break;
        }
        driver.press("j");
    }
    assert!(
        screen(&driver).lines().nth(1).unwrap().contains("Post 20"),
        "{}",
        screen(&driver)
    );
    // Five posts appear above it.
    {
        let mut state = state.lock().unwrap();
        let mut grown = feed_snapshot_of(URL, -4, 60);
        // A new title, to tell when the app has read it.
        grown.dom["strings"][1] = "Feed grown".into();
        state.pages.insert("feed".into(), grown);
        state.events.push(EngineEvent::Changed);
    }
    wait_until(&mut driver, "the grown feed", |s| s.contains("Feed grown"));
    let first = screen(&driver)
        .lines()
        .nth(1)
        .unwrap_or_default()
        .to_string();
    assert!(first.contains("Post 20"), "{}", screen(&driver));
    // And the new posts are there above.
    driver.press("g g");
    wait_until(&mut driver, "post -4", |s| s.contains("Post -4"));
}

#[test]
fn a_long_page_costs_a_screenful_and_has_no_row_cap() {
    // 3,000 posts: 6,000 rows, past the old 4,000-row cap.
    let engine = feed(3000);
    let mut driver = app(Box::new(engine), options()).driver(60, 20);
    wait_until(&mut driver, "the feed", |s| s.contains("Post 1"));
    driver.press("G");
    wait_until(&mut driver, "the last post", |s| s.contains("Post 3000"));
}

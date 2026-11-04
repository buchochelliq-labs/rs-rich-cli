//! Shelves: boxes that scroll sideways over alike items ("Customers also
//! viewed", a row of videos). Chrome's snapshot of `product-grid.html`,
//! pages built by [`PageBuilder`], and the app over the scripted engine:
//! no browser needed, and nothing here names a site. `tests/live_shelf.rs`
//! pages the fixture's shelf in a real Chrome.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::adapt::{Adapt, Rule};
use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, View};
use noctiluca::engine::{PageAction, Rect};
use noctiluca::layout::{Line, Marks, project};
use noctiluca::model::{Page, Revisions, build};
use noctiluca::shelf::page_by;
use noctiluca::testing::{FakeEngine, PageBuilder, snapshot_file};

fn snapshots() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn snapshot(name: &str) -> Page {
    let path = snapshots().join(format!("{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn lines(page: &Page, columns: u16, adapt: Adapt) -> Vec<String> {
    let marks = Marks {
        adapt,
        ..Marks::default()
    };
    project(page, columns, (8.0, 18.0), &marks)
        .lines
        .iter()
        .map(Line::plain)
        .collect()
}

#[test]
fn a_sideways_shelf_shows_the_items_in_its_box_and_a_strip_that_pages_it() {
    let page = snapshot("product-grid");
    assert_eq!(page.shelves.len(), 1, "{:#?}", page.shelves);
    let shelf = &page.shelves[0];
    assert_eq!(shelf.label, "Customers also viewed");
    assert_eq!(shelf.items.len(), 6);
    assert!(shelf.scroll_width > shelf.client_width);
    assert_eq!(shelf.can_page(), (false, true));
    let shown = lines(&page, 100, Adapt::default());
    let text = shown.join("\n");
    // Three items fit in what a 100-column screen shows of its box; the
    // rest are scrolled out of it, as Chrome shows them, not squeezed in
    // a column at the screen's edge.
    for item in ["Kettle descaler", "Tea tin with seal", "Mug set of 4"] {
        assert!(text.contains(item), "{item}: {text}");
    }
    for item in ["Travel kettle", "Water filter jug", "Milk frother"] {
        assert!(!text.contains(item), "{item}: {text}");
    }
    let strip = shown
        .iter()
        .find(|l| l.contains("of 6"))
        .unwrap_or_else(|| panic!("no strip: {text}"));
    assert!(strip.contains("1–3 of 6"), "{strip:?}");
    assert!(strip.trim_start().starts_with('‹') && strip.trim_end().ends_with('›'));
    // A page on is three items' stride, so the fourth comes first.
    let right = 100.0 * 8.0;
    assert_eq!(page_by(&shelf.clipped(right), true), 3.0 * 172.0);
    assert_eq!(page_by(&shelf.clipped(right), false), -3.0 * 172.0);

    // Off, no strip; the items are still those in its box.
    let off = lines(&page, 100, Adapt::default().with(Rule::Shelves, false)).join("\n");
    assert!(!off.contains("of 6"), "{off}");
    assert!(!off.contains("Milk frother"), "{off}");
}

/// A shelf of six alike cards, 160 px wide 12 px apart, in a 400 px box
/// scrolled `left` px: Chrome lays the cards out moved left by it.
fn shelf_page(left: f64, overflow: &str) -> Page {
    let mut p = PageBuilder::new("https://shop.example/", "Shop", 800.0, 600.0);
    let body = p.body();
    let row = p.element(
        body,
        "UL",
        &[("aria-label", "Recommended")],
        &[("display", "flex"), ("overflow-x", overflow)],
        Some(Rect::new(0.0, 100.0, 400.0, 60.0)),
    );
    p.scroll_box(row, left, 6.0 * 172.0 - 12.0, 400.0);
    for k in 0..6 {
        let x = k as f64 * 172.0 - left;
        let card = p.element(
            row,
            "LI",
            &[],
            &[("display", "list-item")],
            Some(Rect::new(x, 100.0, 160.0, 60.0)),
        );
        p.text(
            card,
            &format!("Item {}", k + 1),
            Rect::new(x, 100.0, 50.0, 18.0),
        );
    }
    build(&p.snapshot(), &mut Revisions::new())
}

/// Text beside a shelf is not the shelf's: a sidebar paragraph level with
/// a shelf in the right column keeps its place, and so does a second
/// shelf at the same height.
#[test]
fn a_shelf_clips_only_what_is_in_it() {
    let mut p = PageBuilder::new("https://shop.example/", "Shop", 800.0, 600.0);
    let body = p.body();
    let side = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 100.0, 180.0, 60.0)),
    );
    p.text(side, "Sidebar note", Rect::new(0.0, 100.0, 100.0, 18.0));
    let shelf = |p: &mut PageBuilder, x0: f64, name: &str| {
        let row = p.element(
            body,
            "UL",
            &[("aria-label", name)],
            &[("display", "flex"), ("overflow-x", "auto")],
            Some(Rect::new(x0, 100.0, 300.0, 60.0)),
        );
        p.scroll_box(row, 0.0, 6.0 * 120.0 - 12.0, 300.0);
        for k in 0..6 {
            let x = x0 + k as f64 * 120.0;
            let card = p.element(
                row,
                "LI",
                &[],
                &[("display", "list-item")],
                Some(Rect::new(x, 100.0, 108.0, 60.0)),
            );
            p.text(
                card,
                &format!("{name} {}", k + 1),
                Rect::new(x, 100.0, 60.0, 18.0),
            );
        }
    };
    shelf(&mut p, 200.0, "Left");
    shelf(&mut p, 500.0, "Right");
    let page = build(&p.snapshot(), &mut Revisions::new());
    assert_eq!(page.shelves.len(), 2, "{:#?}", page.shelves);
    let text = lines(&page, 100, Adapt::default()).join("\n");
    assert!(text.contains("Sidebar note"), "{text}");
    assert!(
        text.contains("Left 1") && text.contains("Right 1"),
        "{text}"
    );
    assert!(
        !text.contains("Left 4") && !text.contains("Right 4"),
        "{text}"
    );
}

#[test]
fn a_shelf_paged_on_shows_its_later_items() {
    let page = shelf_page(3.0 * 172.0, "auto");
    let shelf = &page.shelves[0];
    assert_eq!(shelf.label, "Recommended");
    assert_eq!(shelf.shown(), (3, 2), "two whole cards in 400 px");
    assert_eq!(shelf.can_page(), (true, true));
    let text = lines(&page, 100, Adapt::default()).join("\n");
    assert!(text.contains("Item 4") && text.contains("Item 5"), "{text}");
    assert!(
        !text.contains("Item 3") && !text.contains("Item 6"),
        "{text}"
    );
    assert!(text.contains("4–5 of 6"), "{text}");
}

#[test]
fn a_box_that_does_not_scroll_sideways_or_holds_unlike_children_is_no_shelf() {
    assert!(shelf_page(0.0, "visible").shelves.is_empty());
    assert!(shelf_page(0.0, "hidden").shelves.is_empty());
    assert_eq!(shelf_page(0.0, "scroll").shelves.len(), 1);
    // Two items are not a shelf.
    let mut p = PageBuilder::new("https://shop.example/", "Shop", 800.0, 600.0);
    let body = p.body();
    let row = p.element(
        body,
        "DIV",
        &[],
        &[("display", "flex"), ("overflow-x", "auto")],
        Some(Rect::new(0.0, 0.0, 300.0, 60.0)),
    );
    p.scroll_box(row, 0.0, 600.0, 300.0);
    for k in 0..2 {
        p.element(
            row,
            "DIV",
            &[],
            &[],
            Some(Rect::new(k as f64 * 300.0, 0.0, 300.0, 60.0)),
        );
    }
    assert!(
        build(&p.snapshot(), &mut Revisions::new())
            .shelves
            .is_empty()
    );
}

fn options(url: &str) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some(url.to_string()),
        view: Some(View::Projection),
        images: Some(noctiluca::config::Images::Ansi),
        tunnel: None,
        control: None,
    }
}

#[test]
fn the_shelf_keys_scroll_the_pages_box_sideways() {
    let engine = FakeEngine::from_dir(&snapshots()).unwrap();
    let state = engine.state.clone();
    let mut driver = app(Box::new(engine), options("product-grid.html")).driver(100, 60);
    let start = Instant::now();
    let shown = |driver: &rich_intuituive::Driver| driver.screen().plain().join("\n");
    while !shown(&driver).contains("1–3 of 6") {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{}",
            shown(&driver)
        );
        driver.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.press(">");
    let start = Instant::now();
    loop {
        driver.settle();
        let by = state.lock().unwrap().actions.iter().find_map(|a| match a {
            PageAction::ScrollBox { by, .. } => Some(*by),
            _ => None,
        });
        if let Some(by) = by {
            assert_eq!(by, 3.0 * 172.0);
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "no sideways scroll"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    // At its start, back says so and scrolls nothing.
    driver.press("<");
    driver.settle();
    assert!(
        shown(&driver).contains("shelf: at its start"),
        "{}",
        shown(&driver)
    );
}

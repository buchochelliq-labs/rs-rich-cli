//! `follow_end`: a scroll view that keeps to its end as content arrives,
//! until the reader scrolls up (#798), on `scroll` and `scroll_virtual`.

use std::cell::Cell;
use std::rc::Rc;

use intuituive::prelude::*;

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

/// The signals a test drives: the items, the last item's extra lines, and
/// whether the view follows.
#[derive(Clone, Copy)]
struct Knobs {
    len: Signal<usize>,
    extra: Signal<usize>,
    follow: Signal<bool>,
}

/// Item `i` of `len`: one line, and the last has `extra` more.
fn item(i: usize, knobs: Knobs) -> String {
    let mut text = format!("item {i}");
    if i + 1 == knobs.len.get() {
        for k in 0..knobs.extra.get() {
            text.push_str(&format!("\nitem {i} more {k}"));
        }
    }
    text
}

/// What the driven views are built over.
#[derive(Clone, Copy)]
enum View {
    Scroll,
    Virtual,
    Keyed,
}

fn app(view: View) -> (App, Rc<Cell<Option<Knobs>>>) {
    let out = Rc::new(Cell::new(None));
    let seen = out.clone();
    let app = App::new(move || {
        let knobs = Knobs {
            len: signal(3usize),
            extra: signal(0usize),
            follow: signal(true),
        };
        seen.set(Some(knobs));
        let node = match view {
            View::Scroll => scroll(text(move || {
                (0..knobs.len.get())
                    .map(|i| item(i, knobs))
                    .collect::<Vec<_>>()
                    .join("\n")
            })),
            View::Virtual | View::Keyed => {
                let node = scroll_virtual(
                    move || knobs.len.get(),
                    |_| 1,
                    move |i| text(move || item(i, knobs)),
                );
                if matches!(view, View::Keyed) {
                    node.item_key(|i| i as u64 * 7 + 1)
                } else {
                    node
                }
            }
        };
        node.follow_end_with(knobs.follow)
    });
    (app, out)
}

fn follows(view: View) {
    let (app, knobs) = app(view);
    let mut driver = app.driver(24, 5);
    driver.settle();
    let knobs = knobs.get().expect("the knobs");
    // It fits: the first item is on the first row.
    assert_eq!(rows(&driver)[0], "item 0", "{view:?}");
    // Items arrive: the last stays on the bottom row.
    for n in 5..=12 {
        knobs.len.set(n);
        driver.settle();
        assert_eq!(rows(&driver)[4], format!("item {}", n - 1), "{view:?}");
    }
    assert!(knobs.follow.get_untracked());
    // The last item grows: its last line is still on the bottom row.
    for extra in 1..=3 {
        knobs.extra.set(extra);
        driver.settle();
        assert_eq!(
            rows(&driver)[4],
            format!("item 11 more {}", extra - 1),
            "{view:?}"
        );
    }
    // A row up: it stops following, and new items do not move the view.
    driver.press("up");
    let before = rows(&driver);
    assert!(!knobs.follow.get_untracked(), "{view:?}");
    knobs.extra.set(0);
    knobs.len.set(20);
    driver.settle();
    assert_eq!(rows(&driver)[0], before[0], "{view:?}");
    assert!(!knobs.follow.get_untracked(), "{view:?}");
    // End follows again.
    driver.press("end");
    assert!(knobs.follow.get_untracked(), "{view:?}");
    knobs.len.set(21);
    driver.settle();
    assert_eq!(rows(&driver)[4], "item 20", "{view:?}");
    // The wheel up, then back down to the end, follows again.
    driver.wheel(2, 2, -1);
    assert!(!knobs.follow.get_untracked(), "{view:?}");
    knobs.len.set(22);
    driver.settle();
    assert_ne!(rows(&driver)[4], "item 21", "{view:?}");
    driver.wheel(2, 2, 5);
    assert!(knobs.follow.get_untracked(), "{view:?}");
    knobs.len.set(23);
    driver.settle();
    assert_eq!(rows(&driver)[4], "item 22", "{view:?}");
    // Setting the signal jumps back.
    driver.press("home");
    assert!(!knobs.follow.get_untracked(), "{view:?}");
    assert_eq!(rows(&driver)[0], "item 0", "{view:?}");
    knobs.follow.set(true);
    driver.settle();
    assert_eq!(rows(&driver)[4], "item 22", "{view:?}");
}

impl std::fmt::Debug for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            View::Scroll => "scroll",
            View::Virtual => "scroll_virtual",
            View::Keyed => "scroll_virtual with item_key",
        })
    }
}

#[test]
fn a_scroll_follows_its_end() {
    follows(View::Scroll);
}

#[test]
fn a_virtual_scroll_follows_its_end() {
    follows(View::Virtual);
}

#[test]
fn a_keyed_virtual_scroll_follows_its_end() {
    follows(View::Keyed);
}

#[test]
fn without_follow_end_nothing_changes() {
    let out = Rc::new(Cell::new(None));
    let seen = out.clone();
    let app = App::new(move || {
        let len = signal(3usize);
        seen.set(Some(len));
        scroll_virtual(move || len.get(), |_| 1, |i| label(format!("item {i}")))
    });
    let mut driver = app.driver(24, 5);
    driver.settle();
    let len = out.get().unwrap();
    len.set(30);
    driver.settle();
    assert_eq!(rows(&driver)[0], "item 0");
}

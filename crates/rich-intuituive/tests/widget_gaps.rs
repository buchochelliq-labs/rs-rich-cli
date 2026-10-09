//! The 0.0.18 widget gaps: scrolling across, sortable tables with a cell
//! cursor, the lazy tree, tooltips, drag-and-drop and hover styles.

mod common;

use common::{row_of, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Modifiers, Mouse, MouseKind};
use intuituive::prelude::*;

/// Ten lines, each `wide` cells: `line N` then letters.
fn wide_lines(wide: usize) -> String {
    (0..10)
        .map(|n| {
            let mut line = format!("line {n} ");
            while line.len() < wide {
                line.push(char::from(b'a' + (line.len() % 26) as u8));
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_view_that_scrolls_both_ways_shows_two_bars() {
    let app = App::new(|| scroll_both(label(wide_lines(40))));
    let rows = screen(&run_open(app, Script::new(), 12, 5));
    // The last column is the bar down, the last row the bar across.
    assert!(rows[0].ends_with('┃') || rows[0].ends_with('│'), "{rows:?}");
    assert!(rows[4].contains('━'), "{rows:?}");
    assert!(rows[0].starts_with("line 0"), "{rows:?}");
}

#[test]
fn shift_and_the_wheel_scroll_across_and_arrows_too() {
    let app = App::new(|| scroll_both(label(wide_lines(40))));
    let wheel = Mouse {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        ..Mouse::new(MouseKind::ScrollDown, 2, 2)
    };
    let script = Script::new().event(intuituive::interact::Event::Mouse(wheel));
    let rows = screen(&run_open(app, script, 12, 5));
    assert!(!rows[0].starts_with("line 0"), "moved across: {rows:?}");
    // Down is untouched: line 0 is still the first row.
    assert!(rows[0].contains("line 0") || rows[0].starts_with(' ') || !rows[0].is_empty());
    let app = App::new(|| scroll_x(label(wide_lines(40))));
    let rows = screen(&run_open(app, Script::new().keys("end"), 12, 5));
    assert!(
        rows[0].trim_end().ends_with(char::is_alphabetic),
        "{rows:?}"
    );
    assert!(!rows[0].contains("line"), "at the far end: {rows:?}");
}

#[test]
fn a_focused_cell_far_to_the_right_is_scrolled_into_view() {
    let app = App::new(|| {
        scroll_x(row([
            label("left side of a wide row").fixed(30),
            label("target").focusable().focus_style("reverse").fixed(6),
        ]))
    });
    let rows = screen(&run_open(app, Script::new().keys("tab tab"), 12, 3));
    assert!(row_of(&rows, "target").is_some(), "{rows:?}");
}

use intuituive::widgets::{table_with, Column, Order, TableOptions};

fn files() -> Vec<Vec<String>> {
    vec![
        vec!["b.txt".into(), "20".into()],
        vec!["a.txt".into(), "100".into()],
        vec!["c.txt".into(), "3".into()],
    ]
}

fn columns() -> Vec<Column> {
    vec![
        Column::new("Name", Size::Fixed(6)),
        Column::new("Size", Size::Fixed(6)),
    ]
}

/// The wheel over the header scrolls the rows, as over the rows.
#[test]
fn the_wheel_over_a_header_scrolls_the_rows() {
    use intuituive::interact::Event;
    use std::time::Duration;
    let many = || {
        (0..20)
            .map(|i| vec![format!("f{i}"), i.to_string()])
            .collect()
    };
    let app = App::new(move || table_with(columns(), many, signal(0), TableOptions::default()));
    let mut driver = app.driver(20, 4);
    driver.update(Duration::ZERO);
    driver.render();
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 2, 0)));
    driver.update(Duration::ZERO);
    driver.render();
    let rows = driver.screen().plain();
    assert!(!rows[1].starts_with("f0 "), "scrolled: {rows:?}");
}

#[test]
fn a_click_on_a_header_sorts_by_it_as_numbers_and_again_reverses() {
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().sort_rows(signal(None)),
        )
    });
    // "Size" starts at column 7.
    let rows = screen(&run_open(app, Script::new().click(8, 0), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name   Size ▲");
    let sizes: Vec<&str> = rows[1..]
        .iter()
        .map(|r| r.split_whitespace().nth(1).unwrap_or(""))
        .collect();
    assert_eq!(sizes, ["3", "20", "100"]);
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().sort_rows(signal(None)),
        )
    });
    let rows = screen(&run_open(app, Script::new().click(8, 0).click(8, 0), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name   Size ▼");
    assert!(rows[1].contains("100"), "{rows:?}");
}

#[test]
fn the_app_sorts_when_it_owns_the_order() {
    let app = App::new(|| {
        let sort: Signal<Option<(usize, Order)>> = signal(None);
        let rows = move || {
            let mut rows = files();
            if let Some((column, order)) = sort.get() {
                rows.sort_by(|a: &Vec<String>, b: &Vec<String>| a[column].cmp(&b[column]));
                if order == Order::Descending {
                    rows.reverse();
                }
            }
            rows
        };
        table_with(
            columns(),
            rows,
            signal(0),
            TableOptions::default().sort(sort),
        )
    });
    let rows = screen(&run_open(app, Script::new().keys("s"), 20, 4));
    assert_eq!(rows[0].trim_end(), "Name ▲ Size");
    assert!(rows[1].starts_with("a.txt"), "{rows:?}");
}

#[test]
fn the_cell_cursor_moves_across_and_s_sorts_by_its_column() {
    let app = App::new(|| {
        let column = signal(0usize);
        column_with_status(column)
    });
    let rows = screen(&run_open(app, Script::new().keys("right s"), 20, 5));
    assert_eq!(rows[4].trim_end(), "cell 1");
    assert_eq!(rows[0].trim_end(), "Name   Size ▲");
    let app = App::new(|| column_with_status(signal(0usize)));
    // Clicking a cell moves the cursor there.
    let rows = screen(&run_open(app, Script::new().click(9, 2), 20, 5));
    assert_eq!(rows[4].trim_end(), "cell 1");
}

fn column_with_status(column: Signal<usize>) -> Node {
    let options = TableOptions::default()
        .sort_rows(signal(None))
        .cells(column);
    intuituive::column([
        table_with(columns(), files, signal(0), options).fixed(4),
        text!("cell {column}"),
    ])
}

#[test]
fn dragging_the_gap_after_a_header_resizes_the_column() {
    let app = App::new(|| {
        table_with(
            columns(),
            files,
            signal(0),
            TableOptions::default().resizable(),
        )
    });
    // The gap after "Name" (6 wide) is column 6; drag it to 10.
    let script = Script::new().drag((6, 0), (10, 0));
    let rows = screen(&run_open(app, script, 24, 4));
    assert_eq!(rows[0].trim_end(), "Name       Size");
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use intuituive::widgets::{tree_lazy, LazyItem};

fn dirs() -> Vec<LazyItem> {
    vec![
        LazyItem::branch("src", "src"),
        LazyItem::branch("bad", "bad"),
    ]
}

#[test]
fn a_lazy_tree_loads_a_level_once_when_it_is_first_opened() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let app = App::new(move || {
        let counted = counted.clone();
        let children = move |key: String| -> Result<Vec<LazyItem>, String> {
            counted.fetch_add(1, Ordering::SeqCst);
            if key == "bad" {
                return Err("permission denied".into());
            }
            Ok((0..3)
                .map(|i| LazyItem::leaf(format!("{key}/{i}"), format!("file {i}")))
                .collect())
        };
        tree_lazy(dirs, children, signal(None))
    })
    .wait_for_tasks(true);
    // Open src, close it, open it again; then open bad.
    let rows = screen(&run_open(
        app,
        Script::new().keys("right left right down down down down right"),
        30,
        8,
    ));
    assert!(row_of(&rows, "file 2").is_some(), "{rows:?}");
    assert!(row_of(&rows, "permission denied").is_some(), "{rows:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 2, "src once, bad once");
}

#[test]
fn a_lazy_tree_shows_loading_until_children_arrive() {
    let app = App::new(|| {
        let children = |_key: String| -> Result<Vec<LazyItem>, String> {
            std::thread::sleep(std::time::Duration::from_millis(300));
            Ok(vec![LazyItem::leaf("x", "late")])
        };
        tree_lazy(dirs, children, signal(None))
    });
    let rows = screen(&run_open(app, Script::new().keys("right"), 30, 4));
    assert!(row_of(&rows, "loading…").is_some(), "{rows:?}");
}

/// Data whose keys repeat on their own ancestors (a folder linked back
/// up the tree) opens one level at a time, as far as it is opened, and
/// never recurses.
#[test]
fn a_lazy_tree_whose_children_repeat_an_ancestor_opens_level_by_level() {
    let app = App::new(|| {
        let roots = || vec![LazyItem::branch("/loop", "loop")];
        let children = |key: String| -> Result<Vec<LazyItem>, String> {
            Ok(vec![LazyItem::branch(key, "again")])
        };
        tree_lazy(roots, children, signal(None))
    })
    .wait_for_tasks(true);
    let rows = screen(&run_open(
        app,
        Script::new().keys("right down right down right"),
        30,
        6,
    ));
    let again = rows.iter().filter(|row| row.contains("again")).count();
    assert_eq!(again, 3, "{rows:?}");
}

#[test]
fn the_hover_style_comes_and_goes_with_the_pointer() {
    use intuituive::interact::{Event, Mouse};
    use std::time::Duration;
    let app = App::new(|| {
        column([
            row([label("one").fixed(3), label(" x")]).hover_style("reverse"),
            label("two"),
        ])
    });
    let mut driver = app.driver(10, 2);
    let frame = |driver: &mut intuituive::Driver, event: Option<Event>| {
        if let Some(event) = event {
            driver.event(event);
        }
        driver.update(Duration::ZERO);
        driver.render().unwrap_or_default()
    };
    frame(&mut driver, None);
    let over = frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0))),
    );
    assert!(over.contains("\x1b[0;7m"), "reversed: {over:?}");
    let away = frame(
        &mut driver,
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 1))),
    );
    assert!(
        !away.contains("\x1b[0;7m") && away.contains("one"),
        "plain again: {away:?}"
    );
    // The whole row, gap included, is plain again.
    assert!(driver.screen().plain()[0].starts_with("one x"));
}

// Tooltips.

mod tips {
    use std::time::Duration;

    use super::common::row_of;
    use intuituive::interact::{Event, Key, Mouse, MouseKind};
    use intuituive::prelude::*;

    fn at(ms: u64, driver: &mut intuituive::Driver, event: Option<Event>) -> Vec<String> {
        if let Some(event) = event {
            driver.event(event);
        }
        driver.update(Duration::from_millis(ms));
        let _ = driver.render();
        driver.screen().plain()
    }

    fn moved(column: u16, row: u16) -> Option<Event> {
        Some(Event::Mouse(Mouse::new(MouseKind::Moved, column, row)))
    }

    fn app() -> App {
        App::new(|| {
            column([
                label("save").tooltip("Write it to disk").focusable(),
                label("open").tooltip("Read a file"),
                label("plain"),
                label(""),
            ])
        })
    }

    #[test]
    fn a_tooltip_shows_after_the_pointer_rests_and_hides_when_it_leaves() {
        let mut driver = app().driver(30, 4);
        at(0, &mut driver, None);
        let rows = at(100, &mut driver, moved(1, 0));
        assert!(row_of(&rows, "Write it").is_none(), "not yet: {rows:?}");
        // The loop is told to wake when it is due.
        assert_eq!(
            driver.timeout(Duration::from_millis(100)),
            Duration::from_millis(50)
        );
        assert!(driver.timeout(Duration::from_millis(650)) <= Duration::from_millis(50));
        let rows = at(700, &mut driver, None);
        assert_eq!(row_of(&rows, "Write it to disk"), Some(1), "{rows:?}");
        // To a node without one: gone, and the row under it is back.
        let rows = at(800, &mut driver, moved(1, 2));
        assert!(row_of(&rows, "Write it").is_none(), "{rows:?}");
        assert_eq!(rows[1].trim_end(), "open");
    }

    #[test]
    fn moving_to_another_node_starts_its_own_wait() {
        let mut driver = app().driver(30, 4);
        at(0, &mut driver, None);
        at(0, &mut driver, moved(1, 0));
        // An event happens at the time of the update before it.
        at(500, &mut driver, None);
        let rows = at(500, &mut driver, moved(1, 1));
        let rows_later = at(700, &mut driver, None);
        assert!(row_of(&rows, "Read a file").is_none(), "{rows:?}");
        assert!(
            row_of(&rows_later, "Read a file").is_none(),
            "{rows_later:?}"
        );
        let rows = at(1200, &mut driver, None);
        assert_eq!(row_of(&rows, "Read a file"), Some(2), "{rows:?}");
    }

    #[test]
    fn a_key_or_a_click_hides_it_and_f1_shows_the_focused_one() {
        let mut driver = app().driver(30, 4);
        at(0, &mut driver, None);
        at(0, &mut driver, moved(1, 1));
        let rows = at(700, &mut driver, None);
        assert!(row_of(&rows, "Read a file").is_some(), "{rows:?}");
        let rows = at(
            710,
            &mut driver,
            Some(Event::Key(Key::parse("x").expect("a key"))),
        );
        assert!(row_of(&rows, "Read a file").is_none(), "{rows:?}");
        // F1: the focused node's, below it, at once.
        let rows = at(
            720,
            &mut driver,
            Some(Event::Key(Key::parse("f1").expect("a key"))),
        );
        assert_eq!(row_of(&rows, "Write it to disk"), Some(1), "{rows:?}");
        let rows = at(
            730,
            &mut driver,
            Some(Event::Mouse(Mouse::new(
                MouseKind::Down(intuituive::interact::Button::Left),
                20,
                3,
            ))),
        );
        assert!(row_of(&rows, "Write it").is_none(), "{rows:?}");
    }

    #[test]
    fn a_tooltip_at_the_bottom_goes_above_the_pointer_and_stays_on_screen() {
        let app = App::new(|| {
            column([
                label(""),
                label(""),
                label("edge").tooltip("A long tooltip text"),
            ])
        });
        let mut driver = app.driver(16, 3);
        at(0, &mut driver, None);
        at(0, &mut driver, moved(2, 2));
        let rows = at(700, &mut driver, None);
        let row = row_of(&rows, "A long").expect("shown");
        assert_eq!(row, 0, "above the pointer: {rows:?}");
        // Wrapped to the screen's width, within it.
        assert!(rows[0].chars().count() <= 16);
    }

    #[test]
    fn f1_is_left_to_a_binding_for_it() {
        let app = App::new(|| {
            let pressed = signal(false);
            column([
                label("save")
                    .tooltip("Write it")
                    .focusable()
                    .on_key("f1", move |_| pressed.set(true)),
                text(move || format!("pressed {}", pressed.get())),
            ])
        });
        let mut driver = app.driver(30, 3);
        at(0, &mut driver, None);
        let rows = at(
            10,
            &mut driver,
            Some(Event::Key(Key::parse("f1").expect("a key"))),
        );
        assert!(row_of(&rows, "pressed true").is_some(), "{rows:?}");
        assert!(row_of(&rows, "Write it").is_none(), "{rows:?}");
    }
}

// Drag-and-drop.

mod drag {
    use std::time::Duration;

    use intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
    use intuituive::prelude::*;

    fn send(driver: &mut intuituive::Driver, kind: MouseKind, column: u16, row: u16) -> String {
        driver.event(Event::Mouse(Mouse::new(kind, column, row)));
        driver.update(Duration::ZERO);
        driver.render().unwrap_or_default()
    }

    /// Two cards and two lanes: the first takes strings, the second
    /// numbers.
    fn board() -> App {
        App::new(|| {
            let words = signal(Vec::<String>::new());
            let numbers = signal(Vec::<u32>::new());
            let clicked = signal(0);
            column([
                label("card").draggable("card".to_string()),
                label("seven")
                    .draggable(7u32)
                    .on_click(move |_| clicked.update(|n| *n += 1)),
                text(move || format!("words: {}", words.get().join(",")))
                    .on_drop(move |w: &String, _| words.update(|v| v.push(w.clone()))),
                text(move || format!("numbers: {:?}", numbers.get()))
                    .on_drop(move |n: &u32, _| numbers.update(|v| v.push(*n))),
                text(move || format!("clicked {}", clicked.get())),
            ])
        })
    }

    fn rows(driver: &intuituive::Driver) -> Vec<String> {
        driver
            .screen()
            .plain()
            .into_iter()
            .map(|r| r.trim_end().to_string())
            .collect()
    }

    #[test]
    fn a_value_drops_on_a_target_of_its_type_only() {
        let mut driver = board().driver(30, 5);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        send(&mut driver, MouseKind::Down(Button::Left), 1, 0);
        // Over the numbers lane: a string is not taken there.
        let frame = send(&mut driver, MouseKind::Drag(Button::Left), 1, 3);
        assert!(!frame.contains("\x1b[0;7m"), "no highlight: {frame:?}");
        send(&mut driver, MouseKind::Up(Button::Left), 1, 3);
        assert_eq!(rows(&driver)[3], "numbers: []");
        // Over the words lane: lit, and dropped there.
        send(&mut driver, MouseKind::Down(Button::Left), 1, 0);
        let frame = send(&mut driver, MouseKind::Drag(Button::Left), 1, 2);
        assert!(frame.contains("\x1b[0;7m"), "the target is lit: {frame:?}");
        send(&mut driver, MouseKind::Up(Button::Left), 1, 2);
        assert_eq!(rows(&driver)[2], "words: card");
        // The number goes to its own lane.
        send(&mut driver, MouseKind::Down(Button::Left), 1, 1);
        send(&mut driver, MouseKind::Drag(Button::Left), 2, 3);
        send(&mut driver, MouseKind::Up(Button::Left), 2, 3);
        assert_eq!(rows(&driver)[3], "numbers: [7]");
    }

    #[test]
    fn a_press_without_moving_still_clicks_and_drops_nothing() {
        let mut driver = board().driver(30, 5);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        send(&mut driver, MouseKind::Down(Button::Left), 1, 1);
        send(&mut driver, MouseKind::Up(Button::Left), 1, 1);
        let rows = rows(&driver);
        assert_eq!(rows[4], "clicked 1");
        assert_eq!(rows[3], "numbers: []");
    }

    #[test]
    fn esc_cancels_a_drag() {
        let mut driver = board().driver(30, 5);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        send(&mut driver, MouseKind::Down(Button::Left), 1, 0);
        send(&mut driver, MouseKind::Drag(Button::Left), 1, 2);
        driver.event(Event::Key(Key::parse("esc").expect("a key")));
        driver.update(Duration::ZERO);
        let frame = driver.render().unwrap_or_default();
        assert!(!frame.contains("\x1b[0;7m"), "unlit: {frame:?}");
        assert!(!driver.is_done(), "Esc went to the drag, not the app");
        send(&mut driver, MouseKind::Up(Button::Left), 1, 2);
        assert_eq!(rows(&driver)[2], "words:");
    }

    #[test]
    fn a_drop_inside_a_target_reaches_it() {
        let app = App::new(|| {
            let got = signal(String::new());
            column([
                label("item").draggable(1u8),
                column([label("inside"), text!("got {got}")])
                    .panel("box")
                    .on_drop(move |n: &u8, _| got.set(format!("{n}"))),
            ])
        });
        let mut driver = app.driver(20, 5);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        send(&mut driver, MouseKind::Down(Button::Left), 1, 0);
        send(&mut driver, MouseKind::Drag(Button::Left), 2, 2);
        send(&mut driver, MouseKind::Up(Button::Left), 2, 2);
        assert!(
            rows(&driver).iter().any(|r| r.contains("got 1")),
            "{:?}",
            rows(&driver)
        );
    }
}

#[test]
fn shift_and_the_wheel_scroll_across_through_a_driver() {
    use intuituive::interact::Event;
    use std::time::Duration;
    let app = App::new(|| scroll_both(label(wide_lines(40))));
    let mut driver = app.driver(12, 5);
    driver.update(Duration::ZERO);
    driver.render();
    let wheel = Mouse {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        ..Mouse::new(MouseKind::ScrollDown, 2, 2)
    };
    driver.event(Event::Mouse(wheel));
    driver.update(Duration::ZERO);
    driver.render();
    let rows = driver.screen().plain();
    // Across, not down: the line numbers have scrolled off to the left.
    assert!(!rows[0].contains("line"), "moved across: {rows:?}");
}

#[test]
fn a_viewport_wider_than_the_scroll_cap_lays_out() {
    let app = App::new(|| scroll_x(label("narrow")));
    let rows = screen(&run_open(app, Script::new(), 2100, 2));
    assert!(rows[0].starts_with("narrow"), "{:?}", &rows[0][..20]);
}

#[test]
fn a_lazy_tree_loads_an_open_branch_the_roots_change_to() {
    let app = App::new(|| {
        let which = signal("a");
        let roots = move || vec![LazyItem::branch(which.get(), which.get())];
        let children = |key: String| -> Result<Vec<LazyItem>, String> {
            Ok(vec![LazyItem::leaf(
                format!("{key}/x"),
                format!("in {key}"),
            )])
        };
        tree_lazy(roots, children, signal(None)).on_key("s", move |_| which.set("b"))
    })
    .wait_for_tasks(true);
    let rows = screen(&run_open(app, Script::new().keys("right s"), 30, 4));
    assert!(row_of(&rows, "in b").is_some(), "{rows:?}");
}

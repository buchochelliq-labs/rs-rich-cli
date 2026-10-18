//! The layout explorer (examples/layout.rs), the port of ratatui's
//! `constraint-explorer`, `constraints` and `flex`: each tab at 100x30 as a
//! snapshot, the keys and the mouse, and a key that redraws only the
//! explorer.
//!
//! The file is `layout_explorer.rs` because `tests/layout.rs` already
//! tests the layout itself.

#[path = "../examples/layout.rs"]
#[allow(dead_code)]
mod layout;

use intuituive::interact::{Button, Event, Key, KeyCode, Mouse, MouseKind};
use intuituive::layout::split;
use intuituive::node::Axis;
use intuituive::screen::Rect;
use intuituive::{assert_screen, Driver, StackLayout};
use layout::{layout_app, size_examples, Block, Kind, Options, BLOCKS, TABS};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 30;

fn driver_at(tab: usize, width: u16, height: u16) -> Driver {
    let options = Options {
        tab,
        ..Options::default()
    };
    let mut driver = layout_app(options).driver(width, height);
    driver.settle();
    driver
}

fn driver(tab: usize) -> Driver {
    driver_at(tab, WIDTH, HEIGHT)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn row_of(rows: &[String], needle: &str) -> Option<usize> {
    rows.iter().position(|r| r.contains(needle))
}

/// The widths `blocks` get in `width` cells with `gap` between them.
fn widths(blocks: &[Block], width: u16, gap: u16) -> Vec<u16> {
    let tracks: Vec<_> = blocks.iter().map(|b| b.track()).collect();
    let flow = StackLayout {
        gap,
        ..StackLayout::default()
    };
    split(Rect::new(0, 0, width, 3), Axis::Horizontal, &tracks, flow)
        .iter()
        .map(|r| r.width)
        .collect()
}

#[test]
fn layout_explorer() {
    let driver = driver(0);
    let rows = rows(&driver);
    assert!(
        rows[1].starts_with("Blocks:  Fixed 16   Percent 20%"),
        "{rows:?}"
    );
    // Each block shows the width it got, as the layout gives it.
    assert_eq!(widths(&BLOCKS, WIDTH, 1), [16, 19, 24, 12]);
    for mode in [
        "Start",
        "Center",
        "End",
        "SpaceBetween",
        "SpaceAround",
        "SpaceEvenly",
    ] {
        let at = row_of(&rows, mode).unwrap_or_else(|| panic!("{mode}: {rows:?}"));
        let bottom = &rows[at + 3];
        for width in ["16", "19", "24", "12"] {
            assert!(bottom.contains(&format!("└ {width} ")), "{mode}: {bottom}");
        }
    }
    // Start packs them at the left, End at the right, with the spacers
    // between.
    let start = row_of(&rows, "Start").unwrap();
    assert!(rows[start + 1].starts_with("┌ Fixed 16"), "{rows:?}");
    assert!(rows[start + 2].ends_with("····"), "{rows:?}");
    let end = row_of(&rows, "End").unwrap();
    assert!(rows[end + 2].starts_with("····"), "{rows:?}");
    assert!(rows[end + 1].ends_with('┐'), "{rows:?}");
    assert_screen!(driver, "layout_explorer");
}

#[test]
fn layout_explorer_gap() {
    let mut driver = driver(0);
    driver.press("+ + right");
    let rows = rows(&driver);
    assert!(rows[1].ends_with("gap 3"), "{rows:?}");
    assert_eq!(widths(&BLOCKS, WIDTH, 3), [16, 18, 22, 12]);
    let start = row_of(&rows, "Start").unwrap();
    assert!(
        rows[start + 2].starts_with("│              │···│▒▒▒"),
        "{rows:?}"
    );
    assert_screen!(driver, "layout_explorer_gap");
    // - narrows it again, down to none.
    driver.press("- - - - -");
    assert!(self::rows(&driver)[1].ends_with("gap 0"));
}

#[test]
fn the_explorers_keys_change_the_blocks() {
    let mut driver = driver(0);
    // The second block, made flexible: it takes the room, and every mode
    // looks the same.
    driver.press("right 4");
    let rows = rows(&driver);
    assert!(rows[1].contains("Flex 1"), "{rows:?}");
    let blocks = [BLOCKS[0], Block::new(Kind::Flex, 1), BLOCKS[2], BLOCKS[3]];
    let got = widths(&blocks, WIDTH, 1);
    assert_eq!(got.iter().sum::<u16>() + 3, WIDTH);
    assert!(
        row_of(&rows, &format!("└ {} ", got[1])).is_some(),
        "{rows:?}"
    );
    // ↑ makes it a share of 2; 1 makes it fixed, and ↑/↓ step by two.
    driver.press("up");
    assert!(self::rows(&driver)[1].contains("Flex 2"));
    driver.press("1 up up down");
    assert!(self::rows(&driver)[1].contains("Fixed 14"));
    // a adds a block and picks it; x removes it.
    driver.press("a");
    assert!(self::rows(&driver)[1].contains("Max 12   Fixed 10"));
    driver.press("x x");
    let rows = self::rows(&driver);
    assert!(rows[1].contains("Fixed 14   Ratio 1/4"), "{rows:?}");
    assert!(!rows[1].contains("Max 12"), "{rows:?}");
    // Every kind, from the keys.
    for (key, name) in [
        ("2", "Percent 20%"),
        ("3", "Ratio 1/4"),
        ("5", "Min 10"),
        ("6", "Max 10"),
    ] {
        driver.press(key);
        assert!(self::rows(&driver)[1].contains(name), "{key}");
    }
}

#[test]
fn a_key_redraws_only_the_explorer() {
    let mut driver = layout_app(Options::default())
        .inspector(true)
        .driver(140, HEIGHT);
    driver.settle();
    let draws = |driver: &Driver, name: &str| {
        driver
            .inspector_timings()
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.draws)
            .unwrap_or_else(|| panic!("no node called {name}"))
    };
    let before: Vec<u64> = ["tabs", "footer", "explorer"]
        .iter()
        .map(|name| draws(&driver, name))
        .collect();
    driver.event(Event::Key(Key::new(KeyCode::Up)));
    driver.update(std::time::Duration::ZERO);
    let frame = driver.render().expect("the key draws");
    assert_eq!(driver.stats().drawn, 1, "only the explorer drew");
    assert_eq!(draws(&driver, "tabs"), before[0]);
    assert_eq!(draws(&driver, "footer"), before[1]);
    assert_eq!(draws(&driver, "explorer"), before[2] + 1);
    assert!(!frame.contains("Shift+Tab"), "the footer was not sent");
}

#[test]
fn layout_sizes_ratio() {
    let mut driver = driver(1);
    driver.press("right right");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "Four quarters fill the row exactly").is_some(),
        "{rows:?}"
    );
    // Four quarters of 100: 25 each.
    let at = row_of(&rows, "Four quarters").unwrap();
    assert_eq!(rows[at + 2].matches("25 wide").count(), 4, "{rows:?}");
    assert_screen!(driver, "layout_sizes_ratio");
}

#[test]
fn every_kind_of_size_has_its_rows() {
    let mut driver = driver(1);
    for kind in [
        Kind::Fixed,
        Kind::Percent,
        Kind::Ratio,
        Kind::Flex,
        Kind::Min,
        Kind::Max,
    ] {
        let rows = rows(&driver);
        for (about, _) in size_examples(kind) {
            assert!(
                row_of(&rows, about).is_some(),
                "{kind:?}: {about}: {rows:?}"
            );
        }
        driver.press("right");
    }
    // And round again.
    assert!(row_of(&rows(&driver), "Two fixed blocks").is_some());
    driver.press("left");
    assert!(row_of(&rows(&driver), "At most 20").is_some());
}

#[test]
fn layout_flex() {
    let mut driver = driver(2);
    let rows = rows(&driver);
    assert!(rows[1].starts_with("Start, gap 0"), "{rows:?}");
    let end = row_of(&rows, "End, gap 0").unwrap();
    // Against the scrollbar.
    assert!(rows[end + 1].ends_with("╮┃"), "{rows:?}");
    assert_screen!(driver, "layout_flex");
    // It scrolls, with the keys and the wheel, to the cases with a gap.
    driver.press("end");
    assert!(row_of(&self::rows(&driver), "SpaceEvenly, gap 2").is_some());
    driver.press("home");
    driver.event(Event::Mouse(Mouse::new(MouseKind::ScrollDown, 10, 10)));
    driver.settle();
    assert!(!self::rows(&driver)[1].starts_with("Start, gap 0"));
}

#[test]
fn layout_beyond() {
    let driver = driver(3);
    let rows = rows(&driver);
    assert!(row_of(&rows, "align_items(Center)").is_some(), "{rows:?}");
    assert!(row_of(&rows, "card 9").is_some(), "{rows:?}");
    assert_screen!(driver, "layout_beyond");
}

#[test]
fn tab_and_clicks_change_tabs() {
    let mut driver = driver(0);
    driver.press("tab");
    assert!(row_of(&rows(&driver), "Fixed │ Percent").is_some());
    driver.press("shift+tab shift+tab");
    assert!(row_of(&rows(&driver), "align_items").is_some());
    // " Explorer │ Sizes │ Flex": a click on Flex.
    for kind in [MouseKind::Down(Button::Left), MouseKind::Up(Button::Left)] {
        driver.event(Event::Mouse(Mouse::new(kind, 21, 0)));
        driver.settle();
    }
    assert!(row_of(&rows(&driver), "Start, gap 0").is_some());
}

#[test]
fn the_help_lists_the_keys() {
    let mut driver = driver(0);
    driver.press("?");
    let rows = rows(&driver);
    assert!(row_of(&rows, "a wider gap").is_some(), "{rows:?}");
    driver.press("esc ctrl+p");
    assert!(row_of(&self::rows(&driver), "make it").is_some());
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    assert_eq!(parse("--tab 3").unwrap().tab, 2);
    assert!(parse("--tab=4 --linear").unwrap().linear);
    assert!(parse("--tab 5").is_err());
    assert!(parse("--gap").is_err());
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        for tab in 0..TABS.len() {
            let mut driver = driver_at(tab, width, height);
            driver.press("a a a a a a + + + + right 6 up down x end");
        }
    }
}

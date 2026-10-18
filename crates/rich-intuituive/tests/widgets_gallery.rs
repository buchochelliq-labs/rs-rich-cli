//! The widget gallery (examples/widgets.rs), the port of ratatui's widget
//! examples: each tab at 80x24 as a snapshot, the keys and the mouse, and
//! a key that redraws only what it changes.
//!
//! The file is `widgets_gallery.rs` because `tests/widgets.rs` already
//! tests the `Widget` trait.

#[path = "../examples/widgets.rs"]
#[allow(dead_code)]
mod widgets;

use intuituive::interact::{Button, Event, Key, KeyCode, Mouse, MouseKind};
use intuituive::{assert_screen, assert_screen_styled, Driver};
use widgets::{widgets_app, Options, LINK, SECRET, TABS};

const WIDTH: u16 = 80;
const HEIGHT: u16 = 24;

fn driver_at(tab: usize, width: u16, height: u16) -> Driver {
    let options = Options {
        tab,
        ..Options::default()
    };
    let mut driver = widgets_app(options).driver(width, height);
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

/// The cell column where `needle` starts in `row` (cells, not bytes).
fn column_of(row: &str, needle: &str) -> u16 {
    let at = row.find(needle).expect("the text is in the row");
    rich::cells::cell_len(&row[..at]) as u16
}

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

fn click(driver: &mut Driver, column: u16, row: u16) {
    mouse(driver, MouseKind::Down(Button::Left), column, row);
    mouse(driver, MouseKind::Up(Button::Left), column, row);
}

/// The definition of the style at a cell.
fn style_at(driver: &Driver, column: u16, row: u16) -> String {
    let screen = driver.screen();
    screen
        .style(screen.cell(column, row).style)
        .map(|s| s.definition())
        .unwrap_or_default()
}

#[test]
fn widgets_block() {
    let driver = driver(0);
    let rows = rows(&driver);
    assert!(rows[1].starts_with("┌─ Bordered"), "{rows:?}");
    assert!(row_of(&rows, "┏━ Thick").is_some(), "{rows:?}");
    assert!(row_of(&rows, "╔═══").is_some(), "{rows:?}");
    // The shadow, along the bottom.
    assert!(rows[22].ends_with("▒▒▒▒"), "{rows:?}");
    // The styled block's inside: bold italic blue on black.
    let style = style_at(&driver, 30, 4);
    for part in ["bold", "italic", "blue", "on black"] {
        assert!(style.contains(part), "{style}");
    }
    assert_screen!(driver, "widgets_block");
}

#[test]
fn widgets_text() {
    let mut driver = driver(1);
    let masked = "•".repeat(SECRET.chars().count());
    assert!(row_of(&rows(&driver), &masked).is_some());
    assert!(row_of(&rows(&driver), "A line too long for its p…").is_some());
    assert_screen!(driver, "widgets_text");
    // m shows the secret, and m masks it again.
    driver.press("m");
    assert!(row_of(&rows(&driver), SECRET).is_some());
    driver.press("m");
    assert!(row_of(&rows(&driver), &masked).is_some());
}

#[test]
fn the_links_cells_carry_its_url() {
    let driver = driver(1);
    let rows = rows(&driver);
    let text = "rs-rich on GitHub";
    let at = row_of(&rows, text).expect("the link shows");
    let column = column_of(&rows[at], text);
    let at = at as u16;
    let screen = driver.screen();
    let link = |x: u16| {
        screen
            .style(screen.cell(x, at).style)
            .and_then(|s| s.link().map(str::to_string))
    };
    for x in column..column + text.len() as u16 {
        assert_eq!(link(x).as_deref(), Some(LINK), "{x}");
    }
    // The text round it is not a link.
    assert_eq!(link(column - 1), None);
}

#[test]
fn widgets_list() {
    let mut driver = driver(2);
    // ↓ moves the first list; Tab goes to the chat, where ↑ goes up the
    // screen to the older messages.
    driver.press("down down tab up up");
    let rows = rows(&driver);
    assert!(row_of(&rows, "> Carrots").is_some(), "{rows:?}");
    let chat = row_of(&rows, ">> Ana: Did you book").expect("the chat's selection");
    assert!(chat > 10, "{rows:?}");
    assert_screen!(driver, "widgets_list");
    // A multi-line item repeats its symbol on each line.
    driver.press("up");
    let rows = self::rows(&driver);
    let at = row_of(&rows, ">> Ben: The new place").expect("the long message");
    assert!(rows[at + 1].contains(">> menu and short"), "{rows:?}");
}

#[test]
fn widgets_table() {
    let mut driver = driver(3);
    driver.press("down down right");
    let rows = rows(&driver);
    assert!(row_of(&rows, "🍴 Mushroom risotto").is_some(), "{rows:?}");
    assert!(row_of(&rows, "8 dishes").is_some(), "the footer: {rows:?}");
    // The cell cursor is in the second column of the third row.
    let at = row_of(&rows, "Mushroom").unwrap();
    let column = column_of(&rows[at], "Main") + 1;
    let style = style_at(&driver, column, at as u16);
    assert!(style.contains("on yellow"), "{style}");
    assert_screen_styled!(driver, "widgets_table");
}

#[test]
fn widgets_table_app() {
    let mut driver = driver(3);
    driver.press("v down c");
    let rows = rows(&driver);
    assert!(row_of(&rows, "Table app · emerald").is_some(), "{rows:?}");
    assert!(row_of(&rows, " █ Ben Okafor").is_some(), "{rows:?}");
    // Rows of three lines and a gap.
    let ben = row_of(&rows, "Ben Okafor").unwrap();
    assert!(rows[ben + 2].contains("LF9 1AA"), "{rows:?}");
    assert!(rows[ben + 4].contains("Chloe Ruiz"), "{rows:?}");
    assert_screen!(driver, "widgets_table_app");
    // c goes round the palettes; v goes back to the first table.
    driver.press("c c c v");
    assert!(row_of(&self::rows(&driver), "🍴 Lentil soup").is_some());
}

#[test]
fn widgets_tabs() {
    let mut driver = driver(4);
    driver.press("n n");
    let rows = rows(&driver);
    assert_eq!(rows[16].trim_matches(|c| c == '│' || c == ' '), "Metrics");
    assert_screen!(driver, "widgets_tabs");
    // A click on an inner title picks it: " Overview │ Logs", Logs at 13.
    click(&mut driver, 14, 2);
    let rows = self::rows(&driver);
    assert_eq!(rows[16].trim_matches(|c| c == '│' || c == ' '), "Logs");
    driver.press("p");
    let rows = self::rows(&driver);
    assert_eq!(rows[16].trim_matches(|c| c == '│' || c == ' '), "Overview");
}

#[test]
fn widgets_scrollbar() {
    let mut driver = driver(5);
    driver.press("j j l l");
    let rows = rows(&driver);
    assert!(rows[2].starts_with("│Line  3"), "{rows:?}");
    assert!(rows[12].starts_with("│ the quick"), "{rows:?}");
    assert_screen!(driver, "widgets_scrollbar");
    // Back to the start, and no further.
    driver.press("k k k h h h");
    let rows = self::rows(&driver);
    assert!(rows[2].starts_with("│Line  1"), "{rows:?}");
    assert!(rows[12].starts_with("│Line  1"), "{rows:?}");
}

#[test]
fn widgets_calendar() {
    let mut driver = driver(6);
    let rows = rows(&driver);
    assert!(row_of(&rows, "October 2026").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Su Mo Tu We Th Fr Sa").is_some(), "{rows:?}");
    // The days around the month, and its events.
    assert!(row_of(&rows, "28 29 30  1  2  3  4").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Harvest fair").is_some(), "{rows:?}");
    assert_screen_styled!(driver, "widgets_calendar");
    driver.press("n n");
    let rows = self::rows(&driver);
    assert!(row_of(&rows, "December 2026").is_some(), "{rows:?}");
    assert!(row_of(&rows, "Midwinter feast").is_some(), "{rows:?}");
}

#[test]
fn widgets_popup() {
    let mut driver = driver(7);
    driver.press("p");
    let rows = rows(&driver);
    let top = row_of(&rows, "─ Popup ─").expect("the pop-up opens");
    // 60% of 80 wide, centred.
    assert_eq!(column_of(&rows[top], "╭"), 16, "{rows:?}");
    assert_screen_styled!(driver, "widgets_popup");
    driver.press("esc");
    assert_eq!(row_of(&self::rows(&driver), "─ Popup ─"), None);
    driver.press("p p");
    assert_eq!(row_of(&self::rows(&driver), "─ Popup ─"), None);
}

#[test]
fn widgets_borders() {
    let mut driver = driver(8);
    // The first pane has the focus, and is thick.
    assert!(rows(&driver)[1].starts_with("┏━ Top left"));
    driver.press("down down");
    let rows = rows(&driver);
    assert!(rows[11].starts_with("┢━ Bottom left"), "{rows:?}");
    assert_screen!(driver, "widgets_borders");
    // A click picks a pane.
    click(&mut driver, 60, 5);
    assert!(self::rows(&driver)[1].contains("┲━ Top right"));
}

#[test]
fn widgets_custom() {
    let mut driver = driver(9);
    driver.press("enter");
    assert!(row_of(&rows(&driver), "Red pressed, 1 time.").is_some());
    driver.press("down space");
    assert!(row_of(&rows(&driver), "Green pressed, 1 time.").is_some());
    assert_screen_styled!(driver, "widgets_custom");
}

#[test]
fn a_click_presses_a_button() {
    let mut driver = driver(9);
    let rows = rows(&driver);
    let at = row_of(&rows, "Green").unwrap();
    let x = column_of(&rows[at], "Green");
    let at = at as u16;
    mouse(&mut driver, MouseKind::Down(Button::Left), x, at);
    // Held: pressed, and nothing counted yet.
    assert!(row_of(&self::rows(&driver), "[Green]").is_some());
    assert!(row_of(&self::rows(&driver), "Press a button").is_some());
    mouse(&mut driver, MouseKind::Up(Button::Left), x, at);
    assert!(row_of(&self::rows(&driver), "Green pressed, 1 time.").is_some());
    assert!(row_of(&self::rows(&driver), "[Green]").is_none());
    click(&mut driver, x, at);
    assert!(row_of(&self::rows(&driver), "Green pressed, 2 times.").is_some());
    // Let go elsewhere: no press.
    mouse(&mut driver, MouseKind::Down(Button::Left), x, at);
    mouse(&mut driver, MouseKind::Up(Button::Left), x, at + 6);
    assert!(row_of(&self::rows(&driver), "Green pressed, 2 times.").is_some());
}

#[test]
fn the_keys_and_clicks_change_tabs() {
    let mut driver = driver(0);
    driver.press("right");
    assert!(row_of(&rows(&driver), "─ Centred ─").is_some());
    driver.press("left left");
    assert!(row_of(&rows(&driver), "Press a button").is_some());
    for (i, key) in ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
        .iter()
        .enumerate()
    {
        driver.press(key);
        let rows = rows(&driver);
        // The selected title is drawn black on cyan.
        let at = column_of(&rows[0], &format!(" {}", TABS[i])) + 1;
        assert!(style_at(&driver, at, 0).contains("on cyan"), "{key}");
    }
    // " Block  Text  List": a click on List.
    click(&mut driver, 15, 0);
    assert!(row_of(&rows(&driver), "> Apples").is_some());
}

#[test]
fn a_key_redraws_only_what_it_changes() {
    let mut driver = widgets_app(Options {
        tab: 2,
        ..Options::default()
    })
    .inspector(true)
    .driver(120, HEIGHT);
    driver.settle();
    let draws = |driver: &Driver, name: &str| {
        driver
            .inspector_timings()
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.draws)
            .unwrap_or_else(|| panic!("no node called {name}"))
    };
    let before: Vec<u64> = ["tabs", "footer", "chat", "groceries"]
        .iter()
        .map(|name| draws(&driver, name))
        .collect();
    driver.event(Event::Key(Key::new(KeyCode::Down)));
    driver.update(std::time::Duration::ZERO);
    let frame = driver.render().expect("the key draws");
    let stats = driver.stats();
    assert!(stats.drawn <= 2, "{} nodes drew", stats.drawn);
    assert_eq!(
        draws(&driver, "tabs"),
        before[0],
        "the tab strip did not draw"
    );
    assert_eq!(
        draws(&driver, "footer"),
        before[1],
        "the footer did not draw"
    );
    assert_eq!(draws(&driver, "chat"), before[2], "the chat did not draw");
    assert_eq!(draws(&driver, "groceries"), before[3] + 1, "the list drew");
    assert!(!frame.contains("Ana"), "the chat was not sent: {frame:?}");
}

#[test]
fn the_help_and_the_palette_list_the_keys() {
    let mut driver = driver(1);
    driver.press("?");
    let rows = rows(&driver);
    assert!(
        row_of(&rows, "show or mask the secret").is_some(),
        "{rows:?}"
    );
    assert!(row_of(&rows, "the Block tab").is_some(), "{rows:?}");
    driver.press("esc ctrl+p");
    assert!(row_of(&self::rows(&driver), "the next tab").is_some());
}

#[test]
fn linear_mode_reads_the_labels() {
    let options = Options {
        linear: true,
        tab: 2,
    };
    let mut driver = widgets_app(options).driver(WIDTH, HEIGHT);
    driver.settle();
    let names: Vec<String> = driver
        .accessibility()
        .iter()
        .map(|node| node.name.clone())
        .collect();
    for label in ["Widget families", "Groceries", "Chat, newest at the bottom"] {
        assert!(names.iter().any(|n| n == label), "{label}: {names:?}");
    }
}

#[test]
fn the_command_line() {
    let parse = |line: &str| Options::parse(line.split_whitespace().map(String::from));
    assert_eq!(parse("--tab 4").unwrap().tab, 3);
    let options = parse("--tab=10 --linear").unwrap();
    assert_eq!(
        options,
        Options {
            tab: 9,
            linear: true
        }
    );
    assert!(parse("--tab 11").is_err());
    assert!(parse("--tab").is_err());
    assert!(parse("--frozen").is_err());
}

#[test]
fn every_tab_draws_at_small_and_large_sizes() {
    for (width, height) in [(40, 12), (200, 60), (12, 5), (1, 1)] {
        for tab in 0..TABS.len() {
            let mut driver = driver_at(tab, width, height);
            driver.press("down tab enter j l n v p");
            driver.press("esc");
        }
    }
}

//! The colour picker: its grid, hue strip and hex field by keys and the
//! mouse, its look on fewer colours and in text mode, and what a screen
//! reader is told.

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{Driver, color_picker};
use rich::Color;
use rich::color::ColorSystem;

/// A 24 x 9 picker over `start`, with the colour's hex on the row under
/// it; and the signal.
fn picker(start: &str, app: impl FnOnce(App) -> App) -> (Driver, Signal<Color>) {
    picker_of(start, 24, app)
}

fn picker_of(start: &str, width: u16, app: impl FnOnce(App) -> App) -> (Driver, Signal<Color>) {
    let out = std::rc::Rc::new(std::cell::Cell::new(None));
    let set = out.clone();
    let start = start.to_string();
    let built = App::new(move || {
        let colour = signal(Color::parse(&start).unwrap());
        set.set(Some(colour));
        column([
            color_picker(colour),
            text(move || colour.get().name.to_string()),
        ])
    });
    let mut driver = app(built).driver(width, 10);
    driver.settle();
    (driver, out.get().unwrap())
}

fn hex(colour: Signal<Color>) -> String {
    colour.get_untracked().get_truecolor().unwrap().hex()
}

fn row(driver: &Driver, y: usize) -> String {
    driver.screen().plain()[y].trim_end().to_string()
}

#[test]
fn the_grid_and_strip_are_half_blocks_with_a_swatch_and_the_hex() {
    let (driver, _) = picker("#ff0000", |app| app);
    // The grid (21 wide), a column, the strip (2), then the swatch row.
    // The mark at the top right (full saturation and value), ◆ while the
    // grid has the keys; the hue's pointer before the strip.
    assert_eq!(row(&driver, 0), format!("{}◆▸▀▀", "▀".repeat(20)));
    assert_eq!(row(&driver, 8), "     #ff0000 red");
    let screen = driver.screen();
    let cell = screen.cell(0, 0).style;
    // Top left: white over the next value down, both unsaturated.
    let style = screen.style(cell).unwrap();
    assert_eq!(style.color().unwrap().name, "#ffffff");
    assert_eq!(style.bgcolor().unwrap().name, "#eeeeee");
    // The swatch is the colour.
    let swatch = screen.style(screen.cell(0, 8).style).unwrap();
    assert_eq!(swatch.bgcolor().unwrap().name, "#ff0000");
}

#[test]
fn arrows_move_in_the_grid_tab_to_the_hue_and_the_field() {
    let (mut driver, colour) = picker("#ff0000", |app| app);
    driver.press("left");
    assert_eq!(hex(colour), "#ff0d0d");
    driver.press("end pagedown");
    assert_eq!(hex(colour), "#000000");
    // A black keeps its hue: back up, still red.
    driver.press("pageup");
    assert_eq!(hex(colour), "#ff0000");
    // Tab: the hue strip; down turns the hue.
    driver.press("tab down");
    assert_eq!(hex(colour), "#ff6000");
    // Tab: the hex field; the first digit replaces it.
    driver.press("tab");
    driver.type_text("3a7b");
    assert_eq!(row(&driver, 8), "     #3a7b__");
    driver.type_text("d5");
    assert_eq!(hex(colour), "#3a7bd5");
    assert_eq!(row(&driver, 9), "#3a7bd5");
    // Esc puts back what was being typed.
    driver.type_text("12");
    driver.press("escape");
    assert_eq!(row(&driver, 8), "     #3a7bd5");
    // Tab from the field moves the focus on; Shift+Tab comes back to it.
    driver.press("tab");
    assert!(driver.focused_name().is_none_or(|n| n != "color_picker"));
}

#[test]
fn a_click_or_a_drag_picks_in_the_grid_and_the_strip() {
    let (mut driver, colour) = picker("#ff0000", |app| app);
    // The bottom left of the grid: black.
    driver.click(0, 7);
    assert_eq!(hex(colour), "#111111");
    // A drag to the top right: the full colour.
    driver.send(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        0,
        7,
    )));
    driver.send(Event::Mouse(Mouse::new(
        MouseKind::Drag(Button::Left),
        30,
        0,
    )));
    driver.send(Event::Mouse(Mouse::new(MouseKind::Up(Button::Left), 30, 0)));
    assert_eq!(hex(colour), "#ff0000");
    // Half way down the strip: cyan.
    driver.click(22, 4);
    assert_eq!(hex(colour), "#00ffff");
    // A click on the field types into it.
    driver.click(7, 8);
    driver.type_text("0f0");
    driver.press("enter");
    assert_eq!(hex(colour), "#00ff00");
}

#[test]
fn the_app_setting_the_value_moves_the_picker() {
    let (mut driver, colour) = picker("#ff0000", |app| app);
    colour.set(Color::from_rgb(0, 0, 128));
    driver.settle();
    assert_eq!(row(&driver, 8), "     #000080 navy");
}

#[test]
fn on_256_colours_the_nearest_is_shown_and_the_value_kept() {
    let console = rich::Console::builder()
        .width(32)
        .color_system(Some(ColorSystem::EightBit))
        .force_terminal(true)
        .build();
    let (mut driver, colour) = picker_of("#3a7bd5", 32, |app| app.console(console));
    assert_eq!(row(&driver, 8), "     #3a7bd5 ≈ color(68)");
    driver.press("tab tab");
    driver.type_text("ff0000");
    assert_eq!(hex(colour), "#ff0000");
    assert_eq!(row(&driver, 8), "     #ff0000 red ≈ color(196)");
}

#[test]
fn in_text_mode_it_is_the_field_and_a_screen_reader_hears_the_hex_and_name() {
    let (mut driver, colour) = picker("#ffa500", |app| app.accessible(true));
    assert_eq!(row(&driver, 0), "#ffa500 orange");
    driver.type_text("00f");
    driver.press("enter");
    assert_eq!(hex(colour), "#0000ff");
    let node = &driver.accessibility()[0];
    assert_eq!(node.describe(), "Colour, slider: #0000ff blue");
}

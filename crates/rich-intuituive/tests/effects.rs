//! Drawing over children, reading cells, `effect` and `fill` (0.0.19 gap
//! A7).

use std::time::Duration;

use intuituive::interact::{Event, Key};
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{Canvas, DrawCx, MeasureCx};
use intuituive::{assert_screen, Driver};

fn shown(app: App, width: u16, height: u16) -> Driver {
    let mut driver = app.driver(width, height);
    driver.settle();
    driver
}

fn style(definition: &str) -> rich::Style {
    rich::Style::parse(definition).unwrap()
}

fn style_at(driver: &Driver, x: u16, y: u16) -> Option<rich::Style> {
    let screen = driver.screen();
    screen.style(screen.cell(x, y).style).cloned()
}

/// Its child, every cell reversed once the child has drawn.
struct Reverse(Node);

impl Widget for Reverse {
    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.0)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn draw_over(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        canvas.restyle(0, 0, width, height, &style("reverse"));
    }

    fn retained(&self) -> bool {
        true
    }
}

#[test]
fn a_draw_over_reverses_its_childrens_cells_and_follows_their_changes() {
    let app = App::new(|| {
        let word = signal("before");
        widget(Reverse(text!("{word} [red]red[/]"))).on_key("x", move |_| word.set("after!"))
    });
    let mut driver = shown(app, 14, 1);
    assert_eq!(style_at(&driver, 0, 0), Some(style("reverse")));
    assert_eq!(style_at(&driver, 7, 0), Some(style("red reverse")));
    // Nothing drew: nothing is sent.
    driver.update(Duration::ZERO);
    assert!(driver.render().is_none());
    // The child changes; the overlay runs again over its new cells.
    driver.event(Event::Key(Key::char('x')));
    driver.update(Duration::ZERO);
    let frame = driver.render().expect("the change draws");
    // Only the cells that changed are sent, reversed.
    assert!(frame.contains("\x1b[0;7mafte"), "{frame:?}");
    assert!(!frame.contains("red"), "{frame:?}");
    assert_eq!(driver.screen().plain()[0].trim_end(), "after! red");
    assert_eq!(style_at(&driver, 0, 0), Some(style("reverse")));
}

/// What its child drew at the top left, written to its right.
struct Echo(Node);

impl Widget for Echo {
    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.0)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![Rect::new(rect.x, rect.y, 2, 1)]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn draw_over(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let cell = canvas.cell(0, 0).expect("on the canvas");
        let said = format!(
            "={}:{}:{}",
            cell.symbol(),
            cell.width(),
            cell.style()
                .map(rich::Style::definition)
                .unwrap_or_default()
        );
        canvas.print(2, 0, &said, None);
        // A wide grapheme's second column reads as a continuation.
        assert!(canvas.cell(1, 0).is_some_and(|c| c.symbol() != "x"));
        assert!(canvas.cell(canvas.width(), 0).is_none());
    }
}

#[test]
fn cell_returns_what_a_child_wrote() {
    let app = App::new(|| widget(Echo(label("[bold green]x"))));
    let driver = shown(app, 20, 1);
    assert_eq!(driver.screen().plain()[0].trim_end(), "x =x:1:bold green");
    let app = App::new(|| widget(Echo(label("界"))));
    let driver = shown(app, 20, 1);
    assert_eq!(driver.screen().plain()[0].trim_end(), "界=界:2:");
}

#[test]
fn effect_drip() {
    let app = App::new(|| {
        let phase = signal(0u16);
        every(Duration::from_millis(16), move |_| {
            phase.update(|p| *p += 1);
        });
        effect(
            label("ABCDEFGH\nabcdefgh\n12345678\n\n\n"),
            move |canvas, _cx| {
                // Column x has dripped (phase × (x % 3 + 1)) / 2 rows.
                let phase = phase.get();
                let (width, height) = (canvas.width(), canvas.height());
                for x in 0..width {
                    let drop = (phase * (x % 3 + 1) / 2).min(height);
                    let column: Vec<(String, Option<rich::Style>)> = (0..height)
                        .map(|y| {
                            let cell = canvas.cell(x, y).expect("on the canvas");
                            (cell.symbol().to_string(), cell.style().cloned())
                        })
                        .collect();
                    for y in 0..height {
                        let (symbol, style) = match y.checked_sub(drop) {
                            Some(from) => column[from as usize].clone(),
                            None => (" ".to_string(), None),
                        };
                        canvas.set_cell(x, y, &symbol, style.as_ref());
                    }
                }
            },
        )
    });
    let mut driver = app.driver(8, 6);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    for frame in 1..=3 {
        driver.update(Duration::from_millis(16 * frame));
        let _ = driver.render();
    }
    assert_screen!(driver, "effect_drip");
}

#[test]
fn an_effect_sees_its_child_afresh_when_only_the_child_changed() {
    let app = App::new(|| {
        let word = signal("ab");
        // Moves every row one cell right: run twice over its own output,
        // a row would move two.
        effect(column([text!("{word}"), label("xy")]), |canvas, _cx| {
            for y in 0..canvas.height() {
                for x in (1..canvas.width()).rev() {
                    let symbol = canvas.cell(x - 1, y).map(|c| c.symbol().to_string());
                    canvas.set_cell(x, y, &symbol.unwrap_or_default(), None);
                }
                canvas.set_cell(0, y, ">", None);
            }
        })
        .on_key("x", move |_| word.set("cd"))
    });
    let mut driver = shown(app, 5, 2);
    assert_eq!(driver.screen().plain(), [">ab  ", ">xy  "]);
    // Only the first row's text changed; the second is shifted once still.
    driver.press("x");
    assert_eq!(driver.screen().plain(), [">cd  ", ">xy  "]);
}

#[test]
fn fill_dots() {
    let app = App::new(|| {
        column([
            fill("·", "bright_black").fixed(2),
            row([fill("界", "").fixed(5), fill("=", "accent")]).fixed(1),
        ])
    });
    let driver = shown(app, 9, 3);
    assert_screen!(driver, "fill_dots");
    let rows = driver.screen().plain();
    assert_eq!(rows[0], "·········");
    // A wide glyph that does not fit at a row's end leaves a space.
    assert_eq!(rows[2], "界界 ====");
    assert_eq!(style_at(&driver, 0, 0), Some(style("bright_black")));
    assert_eq!(style_at(&driver, 6, 2), Some(style("bright_cyan")));
}

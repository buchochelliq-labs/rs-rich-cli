//! A click on a canvas reaches the page where the pointer was (#819): to
//! the pixel from a terminal that reports the mouse in pixels
//! (SGR-Pixels), at the cell's centre from one that reports cells. On
//! intuiTUIve's driver over the scripted engine, in both views.

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, Images, View};
use noctiluca::engine::{PageAction, Rect};
use noctiluca::testing::{FakeEngine, PageBuilder};
use rich_intuituive::Driver;
use std::time::{Duration, Instant};

/// A page with a line of text and, under it, a 240 by 108 canvas (in the
/// projection, 30 by 6 of Noctiluca's 8 by 18 pixel cells) or picture (in
/// reader mode, which draws pictures but names canvases: 30 by 6 too).
fn engine(view: View) -> FakeEngine {
    let mut p = PageBuilder::new(
        "file:///noctiluca/fixtures/canvas.html",
        "Canvas fixture",
        800.0,
        400.0,
    );
    let body = p.body();
    let para = p.element(body, "P", &[], &[], Some(Rect::new(0.0, 0.0, 800.0, 18.0)));
    p.text(para, "Above the canvas", Rect::new(0.0, 0.0, 128.0, 18.0));
    let rect = Rect::new(0.0, 36.0, 240.0, 108.0);
    match view {
        View::Reader => {
            p.image(
                body,
                "file:///noctiluca/fixtures/media/field.jpg",
                "Field",
                rect,
            );
        }
        _ => {
            p.element(
                body,
                "CANVAS",
                &[("width", "240"), ("height", "108")],
                &[("display", "inline")],
                Some(rect),
            );
        }
    }
    let para = p.element(
        body,
        "P",
        &[],
        &[],
        Some(Rect::new(0.0, 162.0, 800.0, 18.0)),
    );
    p.text(para, "Below the canvas", Rect::new(0.0, 162.0, 128.0, 18.0));
    let dir = std::env::temp_dir().join(format!("noctiluca-canvas-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let engine = FakeEngine::from_dir(&dir).unwrap();
    engine
        .state
        .lock()
        .unwrap()
        .pages
        .insert("canvas".into(), p.snapshot());
    engine
}

fn options(view: View) -> Options {
    Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some("canvas.html".to_string()),
        view: Some(view),
        images: Some(Images::Ansi),
        tunnel: None,
        control: None,
    }
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

fn wait(driver: &mut Driver, what: &str, ok: impl Fn(&mut Driver) -> bool) {
    let start = Instant::now();
    loop {
        driver.settle();
        if ok(driver) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "{what} never happened:\n{}",
            screen(driver)
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Where `text` first shows below the header: its column and row.
fn find(driver: &Driver, text: &str) -> (u16, u16) {
    driver
        .screen()
        .plain()
        .iter()
        .enumerate()
        .skip(2)
        .find_map(|(row, line)| {
            let at = line.find(text)?;
            Some((rich::cells::cell_len(&line[..at]) as u16, row as u16))
        })
        .unwrap_or_else(|| panic!("no {text:?} on the page:\n{}", screen(driver)))
}

/// The last point click the page was sent.
fn last_point(
    state: &std::sync::Arc<std::sync::Mutex<noctiluca::testing::FakeState>>,
) -> Option<(f64, f64)> {
    state
        .lock()
        .unwrap()
        .actions
        .iter()
        .rev()
        .find_map(|a| match a {
            PageAction::ClickAt { x, y, .. } => Some((*x, *y)),
            _ => None,
        })
}

fn near(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 0.01 && (a.1 - b.1).abs() < 0.01
}

#[test]
fn a_click_on_a_canvas_reaches_the_page_where_the_pointer_was() {
    for view in [View::Projection, View::Reader] {
        let engine = engine(view);
        let state = engine.state.clone();
        let mut driver = app(Box::new(engine), options(view)).driver(80, 30);
        // The projection draws the canvas where Chrome lays it out (its
        // still of Chrome's pixels, in half blocks at `--images ansi`);
        // reader mode names a picture, then draws it in the rows under
        // its name.
        let (column, top) = if view == View::Reader {
            wait(&mut driver, "the picture", |d| {
                screen(d).contains("image: Field")
            });
            let (column, name) = find(&driver, "image: Field");
            (column, name + 1)
        } else {
            wait(&mut driver, "the canvas", |d| screen(d).contains('▀'));
            find(&driver, "▀")
        };
        let (_, above) = find(&driver, "Above the canvas");
        // A terminal with cells of 10 by 20 pixels: 4 and 9 pixels into
        // the canvas's column 15, row 3.
        let (x, y) = (u32::from(column + 15) * 10 + 4, u32::from(top + 3) * 20 + 9);
        driver.click_pixel(x, y, (10, 20));
        let expected = ((15.0 + 0.4) / 30.0 * 240.0, (3.0 + 0.45) / 6.0 * 108.0);
        wait(&mut driver, "the point click", |_| {
            last_point(&state).is_some_and(|at| near(at, expected))
        });
        // From a terminal that reports cells: the cell's centre.
        driver.click(column + 15, top + 3);
        let expected = ((15.0 + 0.5) / 30.0 * 240.0, (3.0 + 0.5) / 6.0 * 108.0);
        wait(&mut driver, "the cell click", |_| {
            last_point(&state).is_some_and(|at| near(at, expected))
        });
        // Text takes no point click.
        let before = state.lock().unwrap().actions.len();
        driver.click(column + 1, above);
        driver.settle();
        let after = state.lock().unwrap().actions.clone();
        assert!(
            !after[before..]
                .iter()
                .any(|a| matches!(a, PageAction::ClickAt { .. })),
            "{view:?}: {after:?}"
        );
    }
}

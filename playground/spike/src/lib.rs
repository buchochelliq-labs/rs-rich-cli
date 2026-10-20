//! The docs playground spike: rs-rich rendering in the browser from wasm.
//!
//! Tier 1 (always): core renderables to ANSI for xterm.js, or to HTML
//! through the console's export. Tier 3 (the `app` feature): the
//! intuiTUIve tutorial's to-do app, unchanged, on a `Driver` fed by the
//! bytes xterm.js produces for keys and the mouse.

use rich::{ColorSystem, Console, Panel, Table, Text};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn error(message: &str);
}

/// Panics go to the browser's console (they abort: `panic = "abort"`).
#[wasm_bindgen(start)]
fn start() {
    std::panic::set_hook(Box::new(|info| error(&info.to_string())));
}

/// A console for a terminal `width` cells wide that takes truecolor, as
/// xterm.js does. The clock is pinned: `std::time::Instant` panics on
/// `wasm32-unknown-unknown`, and nothing here needs the time.
fn console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width.max(20))
        .get_time(|| 0.0)
        .build()
}

/// Console markup (`console.print("[bold]…")`), rendered to ANSI.
#[wasm_bindgen]
pub fn render_markup(markup: &str, width: usize) -> String {
    let console = console(width);
    console.capture(|c| c.print_str(markup))
}

/// Markup inside a Panel titled `title`, rendered to ANSI.
#[wasm_bindgen]
pub fn render_panel(markup: &str, title: &str, width: usize) -> String {
    let console = console(width);
    let body = Text::from_markup(markup).unwrap_or_else(|e| Text::new(e.to_string()));
    let panel = Panel::fit(Box::new(body))
        .title(title)
        .border_style("cyan");
    console.capture(|c| c.print(&panel))
}

/// Comma-separated lines (the first is the header) as a Table, to ANSI.
#[wasm_bindgen]
pub fn render_table(csv: &str, title: &str, width: usize) -> String {
    let console = console(width);
    console.capture(|c| c.print(&table(csv, title)))
}

/// The same Table, exported as an HTML document with inline styles.
#[wasm_bindgen]
pub fn table_html(csv: &str, title: &str, width: usize) -> String {
    let console = console(width);
    console.export_html(|c| c.print(&table(csv, title)))
}

/// Markdown, rendered to ANSI.
#[cfg(feature = "syntax")]
#[wasm_bindgen]
pub fn render_markdown(source: &str, width: usize) -> String {
    let console = console(width);
    console.capture(|c| c.print(&rich::markdown::Markdown::new(source)))
}

fn table(csv: &str, title: &str) -> Table {
    let mut lines = csv.lines().filter(|line| !line.trim().is_empty());
    let mut table = Table::new().title(title);
    for header in lines.next().unwrap_or_default().split(',') {
        table.add_column(header.trim());
    }
    for line in lines {
        let cells: Vec<&str> = line.split(',').map(str::trim).collect();
        table.add_row(&cells);
    }
    table
}

/// The build's crate versions, for the page's footer.
#[wasm_bindgen]
pub fn versions() -> String {
    format!("rs-rich-playground-spike, features: {}", features())
}

fn features() -> &'static str {
    match (cfg!(feature = "syntax"), cfg!(feature = "app")) {
        (true, true) => "syntax, app",
        (true, false) => "syntax",
        (false, true) => "app",
        (false, false) => "none",
    }
}

// rs-rich-web's decoder of xterm.js input, used as it is: it depends on
// nothing but rs-rich-interact's event types.
#[cfg(feature = "app")]
#[path = "../../../crates/rich-web/src/input.rs"]
#[allow(dead_code)]
mod input;

// The intuiTUIve tutorial's app, unchanged.
#[cfg(feature = "app")]
#[path = "../../../crates/rich-intuituive/examples/todo.rs"]
#[allow(dead_code)]
mod todo;

#[cfg(feature = "app")]
mod app {
    //! Tier 3: an intuiTUIve app on a `Driver`, a push loop the page runs:
    //! input in, frames out, the time from `performance.now()`.

    use std::cell::RefCell;
    use std::time::Duration;

    use intuituive::interact::Event;
    use intuituive::Driver;
    use wasm_bindgen::prelude::*;

    use crate::{input, todo};

    thread_local! {
        static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
    }

    fn with<T>(f: impl FnOnce(&mut Driver) -> T) -> Option<T> {
        DRIVER.with(|cell| cell.borrow_mut().as_mut().map(f))
    }

    /// Start the app in a `columns` x `rows` terminal; returns its first
    /// frame.
    #[wasm_bindgen]
    pub fn app_start(columns: u16, rows: u16) -> String {
        let app = todo::todo_app(vec![
            todo::Todo::new(1, "Read the intuiTUIve tutorial"),
            todo::Todo::new(2, "Run it in a browser"),
        ]);
        let mut driver = app.driver(columns, rows);
        driver.update(Duration::ZERO);
        let frame = driver.render().unwrap_or_default();
        DRIVER.with(|cell| *cell.borrow_mut() = Some(driver));
        // The alternate screen, the cursor hidden, mouse reporting on, as
        // rs-rich-web's session writes them.
        format!("\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1006h{frame}")
    }

    /// Bytes xterm.js produced (`onData`) at `now_ms`; returns what to
    /// draw. Empty once the app has quit.
    #[wasm_bindgen]
    pub fn app_input(data: &str, now_ms: f64) -> String {
        let events = input::decode(data);
        step(now_ms, |driver| {
            for event in events {
                driver.event(event);
            }
        })
    }

    /// The terminal is now `columns` x `rows`.
    #[wasm_bindgen]
    pub fn app_resize(columns: u16, rows: u16, now_ms: f64) -> String {
        step(now_ms, |driver| driver.event(Event::Resize { columns, rows }))
    }

    /// Timers and animations up to `now_ms`, for a `setTimeout` loop;
    /// returns what to draw.
    #[wasm_bindgen]
    pub fn app_tick(now_ms: f64) -> String {
        step(now_ms, |_| {})
    }

    /// Milliseconds until the app next needs a tick, if it is running.
    #[wasm_bindgen]
    pub fn app_timeout(now_ms: f64) -> f64 {
        with(|driver| driver.timeout(at(now_ms)).as_secs_f64() * 1000.0).unwrap_or(-1.0)
    }

    #[wasm_bindgen]
    pub fn app_done() -> bool {
        with(|driver| driver.is_done()).unwrap_or(true)
    }

    fn at(now_ms: f64) -> Duration {
        Duration::from_secs_f64(now_ms.max(0.0) / 1000.0)
    }

    fn step(now_ms: f64, f: impl FnOnce(&mut Driver)) -> String {
        DRIVER.with(|cell| {
            let mut slot = cell.borrow_mut();
            let Some(driver) = slot.as_mut() else {
                return String::new();
            };
            driver.update(at(now_ms));
            f(driver);
            driver.update(at(now_ms));
            let out = driver.render().unwrap_or_default();
            if driver.is_done() {
                let driver = slot.take().expect("running");
                return out + &driver.finish() + "\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[?1049l";
            }
            out
        })
    }
}

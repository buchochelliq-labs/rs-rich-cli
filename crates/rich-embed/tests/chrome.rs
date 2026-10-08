//! ChromeEngine against a real browser. Runs only when `RICH_EMBED_CHROME`
//! names a Chrome or Chromium binary, so CI needs none; without it the test
//! says it skipped and passes. The browser keeps its sandbox on, so it must
//! be able to start one (as root it cannot).

#![cfg(feature = "chrome")]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use rich::Color;
use rich_embed::{web_view_with, ChromeEngine, WebHandle};
use rich_intuituive::prelude::*;
use rich_intuituive::Driver;

#[test]
fn a_page_arrives_as_a_screencast_and_draws_as_half_blocks() {
    let Some(binary) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let url = "data:text/html,<title>red</title><body style='margin:0;background:%23ff0000'>";
    let handle: Rc<RefCell<Option<WebHandle>>> = Rc::default();
    let keep = handle.clone();
    let app = App::new(move || {
        let page = web_view_with(ChromeEngine::new().binary(binary), url);
        *keep.borrow_mut() = Some(page.handle());
        page.node()
    });
    let mut driver = app.driver(20, 6);
    let start = Instant::now();
    let red = |driver: &Driver| {
        let screen = driver.screen();
        let cell = screen.cell(10, 3);
        // Red, give or take what JPEG does to it.
        let colour = screen
            .style(cell.style)
            .and_then(|s| s.color().and_then(Color::get_truecolor));
        cell.text == "▀" && colour.is_some_and(|c| c.red > 200 && c.green < 60 && c.blue < 60)
    };
    while !red(&driver) && start.elapsed() < Duration::from_secs(60) {
        driver.update(start.elapsed());
        driver.render();
        std::thread::sleep(Duration::from_millis(20));
    }
    let h = handle.borrow().unwrap();
    assert!(
        red(&driver),
        "no red frame: {:?}, error {:?}",
        driver.screen().plain(),
        h.error().get_untracked()
    );
    assert!(h.address().get_untracked().starts_with("data:text/html"));
}

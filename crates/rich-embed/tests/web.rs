//! The web view against a fake engine: the address, back, forward and
//! loading signals, cells drawn, a pixel frame drawn as half blocks, input
//! and the size forwarded; and ProgramEngine over a host that plays bytes.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;
use std::time::Duration;

use rich::{Color, Segment};
use rich_embed::{
    web_view_with, Notify, PageState, Pixels, ProgramEngine, PtyHost, ReplayHandle, ReplayHost,
    WebEngine, WebFrame, WebHandle, WebInput,
};
use rich_intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use rich_intuituive::prelude::*;
use rich_intuituive::Driver;

/// What the fake engine was asked, and what it will answer.
#[derive(Default)]
struct Fake {
    calls: Vec<String>,
    history: Vec<String>,
    at: usize,
    loading: bool,
    frame: Option<WebFrame>,
    notify: Option<Notify>,
}

impl Fake {
    fn changed(&self) {
        if let Some(notify) = &self.notify {
            notify();
        }
    }
}

#[derive(Clone, Default)]
struct FakeEngine(Rc<RefCell<Fake>>);

impl FakeEngine {
    /// The page finishes loading with `frame`.
    fn finish(&self, frame: WebFrame) {
        let mut fake = self.0.borrow_mut();
        fake.loading = false;
        fake.frame = Some(frame);
        fake.changed();
    }

    fn calls(&self) -> Vec<String> {
        std::mem::take(&mut self.0.borrow_mut().calls)
    }
}

impl WebEngine for FakeEngine {
    fn open(&mut self, url: &str) -> io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.calls.push(format!("open {url}"));
        let keep = (fake.at + 1).min(fake.history.len());
        fake.history.truncate(keep);
        fake.history.push(url.to_string());
        fake.at = fake.history.len() - 1;
        fake.loading = true;
        fake.changed();
        Ok(())
    }

    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.0
            .borrow_mut()
            .calls
            .push(format!("resize {columns}x{rows}"));
        Ok(())
    }

    fn input(&mut self, input: WebInput) -> io::Result<()> {
        let call = match input {
            WebInput::Key(key) => format!("key {key}"),
            WebInput::Mouse(mouse) => {
                format!("mouse {:?} {},{}", mouse.kind, mouse.column, mouse.row)
            }
            WebInput::Paste(text) => format!("paste {text}"),
        };
        self.0.borrow_mut().calls.push(call);
        Ok(())
    }

    fn back(&mut self) -> io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.calls.push("back".into());
        fake.at = fake.at.saturating_sub(1);
        fake.changed();
        Ok(())
    }

    fn forward(&mut self) -> io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.calls.push("forward".into());
        if fake.at + 1 < fake.history.len() {
            fake.at += 1;
        }
        fake.changed();
        Ok(())
    }

    fn reload(&mut self) -> io::Result<()> {
        let mut fake = self.0.borrow_mut();
        fake.calls.push("reload".into());
        fake.loading = true;
        fake.changed();
        Ok(())
    }

    fn poll(&mut self) -> Option<WebFrame> {
        self.0.borrow_mut().frame.take()
    }

    fn state(&self) -> PageState {
        let fake = self.0.borrow();
        PageState {
            url: fake.history.get(fake.at).cloned().unwrap_or_default(),
            title: "fake".into(),
            loading: fake.loading,
            can_go_back: fake.at > 0,
            can_go_forward: fake.at + 1 < fake.history.len(),
            error: None,
        }
    }

    fn set_notify(&mut self, notify: Notify) {
        self.0.borrow_mut().notify = Some(notify);
    }
}

fn turn(driver: &mut Driver) {
    for _ in 0..3 {
        driver.update(Duration::ZERO);
        driver.render();
    }
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn key(driver: &mut Driver, name: &str) {
    driver.event(Event::Key(Key::parse(name).unwrap()));
    turn(driver);
}

fn cells(lines: &[&str]) -> WebFrame {
    WebFrame::Cells {
        lines: lines
            .iter()
            .map(|line| vec![Segment::new(line.to_string(), None)])
            .collect(),
        cursor: None,
    }
}

/// An app with a web view over `engine`, its handle, and a status line
/// of its signals under it.
fn view(engine: FakeEngine, width: u16, height: u16) -> (Driver, Rc<RefCell<Option<WebHandle>>>) {
    let handle: Rc<RefCell<Option<WebHandle>>> = Rc::default();
    let keep = handle.clone();
    let app = App::new(move || {
        let page = web_view_with(engine, "https://one.example");
        let h = page.handle();
        *keep.borrow_mut() = Some(h);
        column([
            page.node(),
            text(move || {
                format!(
                    "{} back={} fwd={} loading={}",
                    h.address().get(),
                    h.can_go_back().get(),
                    h.can_go_forward().get(),
                    h.loading().get()
                )
            })
            .fixed(1),
        ])
    });
    let mut driver = app.driver(width, height);
    turn(&mut driver);
    (driver, handle)
}

#[test]
fn the_page_state_is_signals_and_the_bar_drives_it() {
    let engine = FakeEngine::default();
    let (mut driver, handle) = view(engine.clone(), 60, 6);
    // The engine got the page's size (the bar's row and the status line
    // are not the page's) before the first address.
    assert_eq!(engine.calls(), ["resize 60x4", "open https://one.example"]);
    let screen = rows(&driver);
    assert!(screen[0].contains("https://one.example"), "{screen:?}");
    assert!(screen[0].ends_with('…'), "loading: {screen:?}");
    assert_eq!(
        screen[5],
        "https://one.example back=false fwd=false loading=true"
    );

    engine.finish(cells(&["Hello, page"]));
    turn(&mut driver);
    let screen = rows(&driver);
    assert_eq!(screen[1], "Hello, page");
    assert!(screen[5].ends_with("loading=false"));

    // The app opens another address by setting the signal.
    let h = handle.borrow().unwrap();
    h.open("https://two.example");
    turn(&mut driver);
    assert_eq!(engine.calls(), ["open https://two.example"]);
    assert_eq!(
        rows(&driver)[5],
        "https://two.example back=true fwd=false loading=true"
    );

    // Back and forward, from the bar's arrows and from the keys.
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        1,
        0,
    )));
    turn(&mut driver);
    assert_eq!(engine.calls(), ["back"]);
    assert!(rows(&driver)[5].starts_with("https://one.example back=false fwd=true"));
    key(&mut driver, "alt+right");
    assert_eq!(engine.calls(), ["forward"]);
    assert!(rows(&driver)[5].starts_with("https://two.example back=true fwd=false"));
    key(&mut driver, "f5");
    assert_eq!(engine.calls(), ["reload"]);
}

#[test]
fn the_address_bar_takes_a_new_address() {
    let engine = FakeEngine::default();
    let (mut driver, _) = view(engine.clone(), 40, 6);
    engine.calls();
    key(&mut driver, "ctrl+l");
    for _ in 0.."https://one.example".len() {
        key(&mut driver, "backspace");
    }
    for c in "three.example".chars() {
        driver.event(Event::Key(Key::char(c)));
    }
    turn(&mut driver);
    assert!(rows(&driver)[0].contains("three.example"));
    key(&mut driver, "enter");
    assert_eq!(engine.calls(), ["open https://three.example"]);
}

#[test]
fn input_goes_to_the_page_in_its_own_cells() {
    let engine = FakeEngine::default();
    let (mut driver, _) = view(engine.clone(), 40, 6);
    engine.calls();
    key(&mut driver, "j");
    driver.event(Event::Mouse(Mouse::new(
        MouseKind::Down(Button::Left),
        5,
        3,
    )));
    driver.event(Event::Paste("text".into()));
    turn(&mut driver);
    assert_eq!(
        engine.calls(),
        ["key j", "mouse Down(Left) 5,2", "paste text"]
    );
}

#[test]
fn a_pixel_frame_draws_as_half_blocks() {
    let engine = FakeEngine::default();
    let (mut driver, _) = view(engine.clone(), 2, 3);
    // 2 x 2 pixels into 2 x 1 cells: red over blue, green over white.
    let rgb = vec![
        255, 0, 0, 0, 255, 0, //
        0, 0, 255, 255, 255, 255,
    ];
    engine.finish(WebFrame::Pixels(Pixels::new(2, 2, rgb).unwrap()));
    turn(&mut driver);
    let screen = driver.screen();
    let check = |x: u16, top: Color, bottom: Color| {
        let cell = screen.cell(x, 1);
        assert_eq!(cell.text, "▀");
        let style = screen.style(cell.style).unwrap();
        assert_eq!(style.color(), Some(&top));
        assert_eq!(style.bgcolor(), Some(&bottom));
    };
    check(0, Color::from_rgb(255, 0, 0), Color::from_rgb(0, 0, 255));
    check(
        1,
        Color::from_rgb(0, 255, 0),
        Color::from_rgb(255, 255, 255),
    );
}

/// A ProgramEngine whose "browser" for each address is a replayed host.
#[test]
fn a_program_engine_shows_the_browser_screen_and_keeps_a_history() {
    let handles: Rc<RefCell<Vec<(String, ReplayHandle)>>> = Rc::default();
    let made = handles.clone();
    let engine = ProgramEngine::with_hosts(move |url| {
        let host = ReplayHost::new().output(format!("page {url}"));
        made.borrow_mut().push((url.to_string(), host.handle()));
        Box::new(host) as Box<dyn PtyHost>
    });
    let app = App::new(move || {
        let page = web_view_with(engine, "a.example");
        let h = page.handle();
        page.release_keys("f2")
            .node()
            .on_key("f2", move |_| h.open("b.example"))
    });
    let mut driver = app.driver(30, 4);
    turn(&mut driver);
    assert_eq!(rows(&driver)[1], "page a.example");
    // Keys go to the browser.
    key(&mut driver, "down");
    assert_eq!(handles.borrow()[0].1.written(), b"\x1b[B".to_vec());
    // A new address starts the browser again there; back returns.
    key(&mut driver, "f2");
    assert_eq!(rows(&driver)[1], "page b.example");
    assert!(handles.borrow()[0].1.killed());
    key(&mut driver, "alt+left");
    assert_eq!(rows(&driver)[1], "page a.example");
    let urls: Vec<String> = handles.borrow().iter().map(|(u, _)| u.clone()).collect();
    assert_eq!(urls, ["a.example", "b.example", "a.example"]);
    // The first started at the page's size: the bar takes a row.
    assert_eq!(handles.borrow()[0].1.started(), Some((30, 3)));
}

/// An engine with no browser: every call fails at once, and it never
/// calls its `notify`.
struct NoBrowser;

impl WebEngine for NoBrowser {
    fn open(&mut self, _: &str) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::NotFound, "no browser found"))
    }
    fn resize(&mut self, _: u16, _: u16) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::NotFound, "no browser found"))
    }
    fn input(&mut self, _: WebInput) -> io::Result<()> {
        Ok(())
    }
    fn back(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn forward(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn reload(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn poll(&mut self) -> Option<WebFrame> {
        None
    }
    fn state(&self) -> PageState {
        PageState::default()
    }
    fn set_notify(&mut self, _: Notify) {}
}

#[test]
fn a_first_open_that_fails_reaches_the_error_signal() {
    let app = App::new(|| {
        let page = web_view_with(NoBrowser, "https://one.example");
        let h = page.handle();
        column([
            page.node(),
            text(move || format!("error={:?}", h.error().get())).fixed(1),
        ])
    });
    let mut driver = app.driver(40, 5);
    turn(&mut driver);
    turn(&mut driver);
    let last = rows(&driver).last().cloned().unwrap_or_default();
    assert!(last.contains("no browser found"), "{:?}", rows(&driver));
}

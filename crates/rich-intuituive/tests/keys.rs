//! Keys from a legacy terminal and from one with the kitty keyboard
//! protocol: which bindings each fires, and releases reaching widgets.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::interact::headless::{Headless, Script};
use intuituive::interact::{Event, Key};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

fn key(name: &str) -> Key {
    Key::parse(name).expect("a key name")
}

fn frame(driver: &mut intuituive::Driver) -> String {
    driver.update(Duration::ZERO);
    let _ = driver.render();
    driver.screen().plain()[0].trim_end().to_string()
}

/// Shows which of its bindings ran last.
fn bindings(names: &'static [&'static str]) -> App {
    App::new(move || {
        let last = signal(String::from("-"));
        let mut node = text!("{last}");
        for name in names {
            node = node.on_key(name, move |_| last.set(name.to_string()));
        }
        node
    })
}

#[test]
fn a_legacy_key_fires_a_binding_for_any_key_it_could_be() {
    let mut driver = bindings(&["ctrl+i", "ctrl+m", "ctrl+["]).driver(20, 1);
    frame(&mut driver);
    // Keys built in code read as a legacy terminal sends them.
    driver.event(Event::Key(key("tab")));
    assert_eq!(frame(&mut driver), "ctrl+i");
    driver.event(Event::Key(key("enter")));
    assert_eq!(frame(&mut driver), "ctrl+m");
    driver.event(Event::Key(key("esc")));
    assert_eq!(frame(&mut driver), "ctrl+[");
}

#[test]
fn an_exact_key_fires_only_its_own_binding() {
    let mut driver = bindings(&["ctrl+i", "enter"]).driver(20, 1);
    frame(&mut driver);
    driver.event(Event::Key(key("tab").exact()));
    assert_eq!(frame(&mut driver), "-");
    driver.event(Event::Key(key("ctrl+m").exact()));
    assert_eq!(frame(&mut driver), "-");
    driver.event(Event::Key(key("ctrl+i").exact()));
    assert_eq!(frame(&mut driver), "ctrl+i");
    driver.event(Event::Key(key("enter").exact()));
    assert_eq!(frame(&mut driver), "enter");
}

#[test]
fn a_binding_that_names_the_key_wins_over_one_it_could_be() {
    let mut driver = bindings(&["ctrl+i", "tab"]).driver(20, 1);
    frame(&mut driver);
    driver.event(Event::Key(key("tab")));
    assert_eq!(frame(&mut driver), "tab");
    driver.event(Event::Key(key("ctrl+i").exact()));
    assert_eq!(frame(&mut driver), "ctrl+i");
}

#[test]
fn the_headless_terminal_sends_keys_both_ways() {
    // Legacy (the default): a scripted ctrl+i arrives as Tab, which fires
    // the `tab` binding; exact, it is Ctrl+I, which fires nothing.
    let run = |exact: bool| {
        let app = bindings(&["tab"]);
        let mut backend = Headless::new(Script::new().keys("ctrl+i ctrl+c"), 20, 1);
        backend.exact_keys = exact;
        let record = backend.record();
        app.run_on(&mut backend).expect("the app runs");
        let record = record.borrow();
        record.last_frame().trim_end().to_string()
    };
    assert_eq!(run(false), "tab");
    assert_eq!(run(true), "-");
}

/// Records the presses and releases it gets, and uses all but `q`'s.
struct Keys(Rc<RefCell<Vec<String>>>);

impl Widget for Keys {
    fn measure(&mut self, _cx: &MeasureCx, _axis: Axis, _width: u16, _height: u16) -> u16 {
        1
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn event(&mut self, _cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let (seen, key) = match event {
            WidgetEvent::Key(key) => (format!("down {key}"), key),
            WidgetEvent::KeyUp(key) => (format!("up {key}"), key),
            _ => return Used::No,
        };
        self.0.borrow_mut().push(seen);
        // `q` goes on to the bindings above.
        if *key == Key::char('q') {
            Used::No
        } else {
            Used::Yes
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn releases_reach_widgets_and_never_bindings() {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let fired = Rc::new(RefCell::new(0));
    let (keys, count) = (seen.clone(), fired.clone());
    let app = App::new(move || {
        let count = count.clone();
        column([widget(Keys(keys.clone()))]).on_key("q", move |_| *count.borrow_mut() += 1)
    });
    let mut driver = app.driver(20, 2);
    frame(&mut driver);
    driver.event(Event::Key(key("x").exact()));
    driver.event(Event::KeyUp(key("x").exact()));
    // A release the widget leaves goes on up, and fires no binding; the
    // press does.
    driver.event(Event::KeyUp(key("q").exact()));
    assert_eq!(*seen.borrow(), ["down x", "up x", "up q"]);
    assert_eq!(*fired.borrow(), 0);
    driver.event(Event::Key(key("q").exact()));
    assert_eq!(*fired.borrow(), 1);
}

/// Ctrl+Z suspends an app on a backend that can suspend, unless a binding
/// on the focused path takes it; elsewhere it reaches the app as a key.
#[test]
fn ctrl_z_suspends_unless_bound() {
    let run = |bound: bool, suspendable: bool| {
        let app = App::new(move || {
            let last = signal(String::from("-"));
            let node = text!("{last}").on_key("x", move |_| last.set("x".into()));
            if bound {
                node.on_key("ctrl+z", move |_| last.set("undo".into()))
            } else {
                node
            }
        });
        let mut backend = Headless::new(Script::new().keys("ctrl+z ctrl+c"), 20, 1);
        backend.suspendable = suspendable;
        let record = backend.record();
        app.run_on(&mut backend).expect("the app runs");
        let record = record.borrow();
        (record.suspends, record.last_frame().trim_end().to_string())
    };
    // Suspended, then drawn again: the frame after it is whole.
    assert_eq!(run(false, true), (1, "-".to_string()));
    // Bound, the binding runs instead.
    assert_eq!(run(true, true), (0, "undo".to_string()));
    // A backend that cannot suspend hands it to the app.
    assert_eq!(run(true, false), (0, "undo".to_string()));
    assert_eq!(run(false, false), (0, "-".to_string()));
}

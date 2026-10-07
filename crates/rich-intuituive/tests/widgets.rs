//! The widget trait: custom controls with children, events in their own
//! coordinates, mouse capture, hover and the caret.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{run, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Button, Key, MouseKind};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{
    widget, Canvas, DrawCx, EventCx, MeasureCx, MouseExt, Used, Widget, WidgetEvent,
};

/// Records the events it gets, and shows the last one.
struct Probe {
    seen: Rc<RefCell<Vec<String>>>,
    capture: bool,
}

impl Widget for Probe {
    fn name(&self) -> &'static str {
        "probe"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 2,
            Axis::Horizontal => width.min(12),
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let last = self.seen.borrow().last().cloned().unwrap_or_default();
        let hover = if cx.hovered() { "hover" } else { "-" };
        canvas.print(0, 0, &last, None);
        canvas.print(0, 1, hover, None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let text = match event {
            WidgetEvent::Key(key) if *key == Key::char('x') => "key x".to_string(),
            WidgetEvent::Key(_) => return Used::No,
            WidgetEvent::Mouse(mouse) => {
                if self.capture && mouse.is_press() {
                    cx.capture_mouse();
                }
                format!("{:?} {},{}", mouse.kind, mouse.column, mouse.row)
            }
        };
        self.seen.borrow_mut().push(text);
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        Some((3, 1))
    }
}

fn probe(capture: bool) -> (Node, Rc<RefCell<Vec<String>>>) {
    let seen = Rc::new(RefCell::new(Vec::new()));
    let node = widget(Probe {
        seen: seen.clone(),
        capture,
    });
    (node, seen)
}

#[test]
fn a_widget_gets_keys_and_mouse_events_in_its_own_coordinates() {
    let (node, seen) = probe(false);
    let app = App::new(move || {
        column([label("top").fixed(2), node.fixed(2)]).on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .keys("x")
        .mouse(MouseKind::Down(Button::Left), 5, 3)
        .keys("q");
    let rows = screen(&run(app, script, 20, 5));
    // The press was at column 5, row 3: row 1 of the widget, which starts
    // on row 2.
    assert_eq!(*seen.borrow(), ["key x", "Down(Left) 5,1"]);
    assert_eq!(rows[2], "Down(Left) 5,1");
}

#[test]
fn unused_keys_bubble_to_the_ancestors_bindings() {
    let (node, seen) = probe(false);
    let hits = signal_counter();
    let counter = hits.clone();
    let app = App::new(move || {
        column([node])
            .on_key("y", move |_| *counter.borrow_mut() += 1)
            .on_key("q", |cx| cx.quit())
    });
    run(app, Script::new().keys("y y q"), 20, 3);
    assert_eq!(*hits.borrow(), 2);
    assert!(seen.borrow().is_empty());
}

fn signal_counter() -> Rc<RefCell<u32>> {
    Rc::new(RefCell::new(0))
}

#[test]
fn a_captured_mouse_keeps_its_drags_outside_the_widget() {
    let (node, seen) = probe(true);
    let other = signal_counter();
    let counter = other.clone();
    let app = App::new(move || {
        column([
            node.fixed(2),
            label("below").fixed(2).on_mouse(move |_, _| {
                *counter.borrow_mut() += 1;
                true
            }),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 1, 0)
        .mouse(MouseKind::Drag(Button::Left), 4, 3)
        .mouse(MouseKind::Up(Button::Left), 4, 3)
        // Released: the next press below goes to the label.
        .mouse(MouseKind::Down(Button::Left), 4, 3)
        .keys("q");
    run(app, script, 20, 4);
    assert_eq!(
        *seen.borrow(),
        ["Down(Left) 1,0", "Drag(Left) 4,3", "Up(Left) 4,3"]
    );
    assert_eq!(*other.borrow(), 1);
}

#[test]
fn the_wheel_bubbles_to_an_ancestor_that_uses_it() {
    let wheel = Rc::new(RefCell::new(0i32));
    let total = wheel.clone();
    let app = App::new(move || {
        column([label("a"), label("b")])
            .on_mouse(move |_, mouse| {
                *total.borrow_mut() += mouse.wheel();
                mouse.wheel() != 0
            })
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .scroll(true, 0, 1)
        .scroll(true, 0, 1)
        .scroll(false, 0, 0)
        .keys("q");
    run(app, script, 10, 2);
    assert_eq!(*wheel.borrow(), 1);
}

#[test]
fn hover_redraws_the_widget_the_pointer_enters_and_leaves() {
    let (node, _) = probe(false);
    let app = App::new(move || column([node.fixed(2), label("rest")]).on_key("q", |cx| cx.quit()));
    let over = Script::new().mouse(MouseKind::Moved, 2, 1).keys("q");
    let rows = screen(&run(app, over, 20, 4));
    assert_eq!(rows[1], "hover");

    let (node, _) = probe(false);
    let app = App::new(move || column([node.fixed(2), label("rest")]).on_key("q", |cx| cx.quit()));
    let left = Script::new()
        .mouse(MouseKind::Moved, 2, 1)
        .mouse(MouseKind::Moved, 2, 3)
        .keys("q");
    let record = run(app, left, 20, 4);
    assert_eq!(screen(&record)[1], "-");
    // Pointer movement reports were turned on once a widget asked.
    assert!(record.output().contains("\x1b[?1003h"));
}

#[test]
fn a_press_focuses_the_deepest_focusable_node_under_the_pointer() {
    let app = App::new(|| {
        let which = signal(String::new());
        column([
            label("one").focus_style("reverse").fixed(1),
            label("two").focus_style("reverse").fixed(1),
            text(move || which.get()).fixed(1),
        ])
        .on_key("enter", move |_| which.set("entered".into()))
        .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .mouse(MouseKind::Down(Button::Left), 1, 1)
        .keys("tab enter q");
    // The press focused "two", so Tab wrapped round to "one".
    let record = run(app, script, 10, 3);
    assert_eq!(screen(&record)[2], "entered");
}

/// Two children side by side, and a title row above them.
struct Split {
    children: Vec<Node>,
}

impl Widget for Split {
    fn name(&self) -> &'static str {
        "split"
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let half = rect.width / 2;
        vec![
            Rect::new(rect.x, rect.y + 1, half, rect.height - 1),
            Rect::new(
                rect.x + half,
                rect.y + 1,
                rect.width - half,
                rect.height - 1,
            ),
        ]
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let title = if cx.focus_within() { "split*" } else { "split" };
        canvas.print(0, 0, title, None);
    }
}

#[test]
fn a_widget_lays_out_children_that_take_the_focus() {
    let app = App::new(|| {
        let n = signal(0);
        widget(Split {
            children: vec![
                text!("left {n}").focus_style("bold"),
                label("right").focus_style("bold"),
            ],
        })
        .on_key("+", move |_| n.update(|n| *n += 1))
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("+ + q"), 20, 2));
    assert_eq!(rows, ["split*", "left 2    right"]);
}

/// Counts its draws; `x` changes its state.
struct Counted {
    draws: Rc<RefCell<u32>>,
    n: u32,
}

impl Widget for Counted {
    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        *self.draws.borrow_mut() += 1;
        canvas.print(0, 0, &self.n.to_string(), None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if *event != WidgetEvent::Key(Key::char('x')) {
            return Used::No;
        }
        self.n += 1;
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn only_the_widget_whose_state_changed_draws() {
    let (a, b) = (signal_counter(), signal_counter());
    let (da, db) = (a.clone(), b.clone());
    let app = App::new(move || {
        column([
            widget(Counted { draws: da, n: 0 }).fixed(1),
            widget(Counted { draws: db, n: 0 }).fixed(1),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("x x q"), 10, 2));
    assert_eq!(rows, ["2", "0"]);
    // The first frame, then once per key for the focused one only.
    assert_eq!((*a.borrow(), *b.borrow()), (3, 1));
}

#[test]
fn the_caret_goes_where_the_focused_widget_puts_it() {
    let (node, _) = probe(false);
    let app = App::new(move || column([label("top").fixed(1), node]).on_key("q", |cx| cx.quit()));
    let record = run(app, Script::new().keys("q"), 20, 4);
    // Row 1 of the widget, which starts on row 1: screen row 2, column 3
    // (1-based in the escape).
    assert!(
        record.output().contains("\x1b[3;4H\x1b[?25h"),
        "{:?}",
        record.output()
    );
}

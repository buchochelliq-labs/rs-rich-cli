//! What the widget trait gained when every built-in node moved onto it:
//! focus, hover, resize and paste events, hover and pointer tracking that
//! redraws only the widgets concerned, keys with nothing focused,
//! retained drawing, viewports, and the measuring and drawing helpers.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{run, run_open, screen};
use intuituive::interact::headless::Script;
use intuituive::interact::{Event, Key, MouseKind};
use intuituive::node::Axis;
use intuituive::prelude::*;
use intuituive::screen::Rect;
use intuituive::widget::{
    markup_width, widget, Canvas, DrawCx, EventCx, MeasureCx, ScrollCx, Used, Widget, WidgetEvent,
};

type Log = Rc<RefCell<Vec<String>>>;

fn log() -> Log {
    Rc::new(RefCell::new(Vec::new()))
}

/// Records its lifecycle events and its draws, under its name.
struct Watch {
    name: &'static str,
    log: Log,
    hover: bool,
    pointer: bool,
    focusable: bool,
}

impl Watch {
    fn new(name: &'static str, log: &Log) -> Watch {
        Watch {
            name,
            log: log.clone(),
            hover: false,
            pointer: false,
            focusable: true,
        }
    }
}

impl Widget for Watch {
    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let mut line = format!("{} draw", self.name);
        if self.hover {
            line += if cx.hovered() { " hovered" } else { " plain" };
        }
        if self.pointer {
            line += &format!(" at {:?}", cx.pointer());
        }
        canvas.print(0, 0, self.name, None);
        self.log.borrow_mut().push(line);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let what = match event {
            WidgetEvent::Focus(on) => format!("focus {on}"),
            WidgetEvent::Hover(on) => format!("hover {on}"),
            WidgetEvent::Resize { width, height } => format!("resize {width}x{height}"),
            WidgetEvent::Paste(text) => format!("paste {text}"),
            WidgetEvent::Key(key) if *key == Key::char('k') => "key k".to_string(),
            _ => return Used::No,
        };
        self.log.borrow_mut().push(format!("{} {what}", self.name));
        if matches!(event, WidgetEvent::Key(_) | WidgetEvent::Paste(_)) {
            cx.redraw();
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        self.focusable
    }
}

fn entries(log: &Log, prefix: &str) -> Vec<String> {
    log.borrow()
        .iter()
        .filter(|line| line.starts_with(prefix))
        .cloned()
        .collect()
}

#[test]
fn widgets_are_told_when_the_focus_comes_and_goes() {
    let seen = log();
    let (a, b) = (Watch::new("a", &seen), Watch::new("b", &seen));
    let app = App::new(move || {
        column([widget(a).fixed(1), widget(b).fixed(1)]).on_key("q", |cx| cx.quit())
    });
    run(app, Script::new().keys("tab q"), 10, 2);
    let events: Vec<String> = seen
        .borrow()
        .iter()
        .filter(|line| line.contains("focus"))
        .cloned()
        .collect();
    assert_eq!(events, ["a focus true", "a focus false", "b focus true"]);
}

#[test]
fn hover_redraws_only_the_widget_entered_or_left() {
    let seen = log();
    let mut a = Watch::new("a", &seen);
    a.hover = true;
    let mut b = Watch::new("b", &seen);
    b.hover = true;
    let c = Watch::new("c", &seen);
    let app = App::new(move || {
        row([widget(a).fixed(3), widget(b).fixed(3), widget(c).fixed(3)])
            .on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .mouse(MouseKind::Moved, 0, 0)
        .mouse(MouseKind::Moved, 1, 0)
        .mouse(MouseKind::Moved, 4, 0)
        .mouse(MouseKind::Moved, 7, 0)
        .keys("q");
    run(app, script, 9, 1);
    // `a` drew when the pointer came and when it left, not when it moved
    // inside; `c`, which never asked, drew only once.
    assert_eq!(
        entries(&seen, "a draw"),
        ["a draw plain", "a draw hovered", "a draw plain"]
    );
    assert_eq!(
        entries(&seen, "b draw"),
        ["b draw plain", "b draw hovered", "b draw plain"]
    );
    assert_eq!(entries(&seen, "c draw"), ["c draw"]);
    let hovers: Vec<String> = seen
        .borrow()
        .iter()
        .filter(|line| line.contains("hover "))
        .cloned()
        .collect();
    assert_eq!(
        hovers,
        [
            "a hover true",
            "a hover false",
            "b hover true",
            "b hover false",
            "c hover true"
        ]
    );
}

#[test]
fn a_widget_can_follow_the_pointer_over_its_own_cells() {
    let seen = log();
    let mut a = Watch::new("a", &seen);
    a.pointer = true;
    let app = App::new(move || {
        row([label("x").fixed(2), widget(a).fixed(4)]).on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .mouse(MouseKind::Moved, 3, 0)
        .mouse(MouseKind::Moved, 5, 0)
        .mouse(MouseKind::Moved, 0, 0)
        .keys("q");
    run(app, script, 6, 1);
    assert_eq!(
        entries(&seen, "a draw"),
        [
            "a draw at None",
            "a draw at Some((1, 0))",
            "a draw at Some((3, 0))",
            "a draw at None"
        ]
    );
}

#[test]
fn a_resize_and_a_paste_reach_the_widget() {
    let seen = log();
    let a = Watch::new("a", &seen);
    let app = App::new(move || widget(a).on_key("q", |cx| cx.quit()));
    let script = Script::new()
        .resize(12, 3)
        .event(Event::Paste("hello".into()))
        .keys("q");
    run(app, script, 10, 2);
    assert!(
        seen.borrow().contains(&"a resize 12x3".to_string()),
        "{:?}",
        seen.borrow()
    );
    assert!(seen.borrow().contains(&"a paste hello".to_string()));
}

#[test]
fn with_nothing_focused_keys_go_to_the_node_under_the_pointer() {
    let seen = log();
    let mut a = Watch::new("a", &seen);
    a.focusable = false;
    let mut b = Watch::new("b", &seen);
    b.focusable = false;
    let app =
        App::new(move || row([widget(a).fixed(3), widget(b).fixed(3)]).on_key("q", |cx| cx.quit()));
    let script = Script::new()
        .mouse(MouseKind::Moved, 4, 0)
        .keys("k")
        .mouse(MouseKind::Moved, 1, 0)
        .keys("k q");
    run(app, script, 6, 1);
    let keys: Vec<String> = seen
        .borrow()
        .iter()
        .filter(|line| line.ends_with("key k"))
        .cloned()
        .collect();
    assert_eq!(keys, ["b key k", "a key k"]);
}

/// A retained container: a status mark in its first cell, and one child
/// below it.
struct Marked {
    child: Node,
    mark: char,
}

impl Widget for Marked {
    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![Rect::new(rect.x, rect.y + 1, rect.width, rect.height - 1)]
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        canvas.set(0, 0, self.mark, None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if *event == WidgetEvent::Key(Key::char('m')) {
            self.mark = '*';
            cx.redraw();
            return Used::Yes;
        }
        Used::No
    }

    fn retained(&self) -> bool {
        true
    }
}

#[test]
fn a_retained_widget_redraws_its_own_cells_and_leaves_its_children() {
    let seen = log();
    let child = Watch::new("c", &seen);
    let app = App::new(move || {
        widget(Marked {
            child: widget(child),
            mark: '-',
        })
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("m q"), 4, 2));
    assert_eq!(rows, ["*", "c"]);
    // The child drew once: the mark changing did not draw over it.
    assert_eq!(entries(&seen, "c draw"), ["c draw"]);
}

/// A viewport of its own: a window two rows tall onto a column, scrolled
/// by `j`.
struct Window {
    child: Node,
    top: u16,
}

impl Widget for Window {
    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn viewport(&self) -> bool {
        true
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let height = cx.measure(&self.child, Axis::Vertical, rect.width, 0);
        vec![Rect::new(0, 0, rect.width - 1, height)]
    }

    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        let (width, _) = cx.content();
        (Rect::new(1, 0, width, 2), (0, self.top))
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        canvas.print(0, 0, ">", None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if *event == WidgetEvent::Key(Key::char('j')) {
            self.top += 1;
            cx.redraw();
            return Used::Yes;
        }
        Used::No
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn a_widget_of_your_own_can_scroll_its_children() {
    let clicked = Rc::new(RefCell::new(None));
    let seen = clicked.clone();
    let make = move || {
        let seen = seen.clone();
        App::new(move || {
            let rows = (0..6).map(|i| {
                let seen = seen.clone();
                label(format!("row {i}"))
                    .fixed(1)
                    .on_click(move |_| *seen.borrow_mut() = Some(i))
            });
            widget(Window {
                child: column(rows),
                top: 0,
            })
            .on_key("q", |cx| cx.quit())
        })
    };
    let rows = screen(&run(make(), Script::new().keys("j j q"), 8, 3));
    assert_eq!(rows, [">row 2", " row 3", ""]);
    // A click in the window reaches the row drawn there.
    run(make(), Script::new().keys("j").click(2, 1).keys("q"), 8, 3);
    assert_eq!(*clicked.borrow(), Some(2));
}

/// A container laid out with the helpers: its children in a row, with a
/// bordered title over them.
struct Titled {
    title: String,
    children: Vec<Node>,
}

impl Widget for Titled {
    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        match axis {
            Axis::Vertical => {
                2 + self
                    .children
                    .iter()
                    .map(|c| cx.measure(c, Axis::Vertical, width, height))
                    .max()
                    .unwrap_or(0)
            }
            Axis::Horizontal => width,
        }
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        cx.stack(Axis::Horizontal, 1, &self.children, rect.inner(1))
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let style = cx.style("panel.border", "");
        canvas.border("", &style, &style);
        let width = markup_width(&self.title);
        canvas.markup(cx.console(), 2, 0, width, &self.title, None);
    }
}

#[test]
fn the_helpers_lay_out_and_draw_like_the_built_in_nodes() {
    let app = App::new(|| {
        column([
            widget(Titled {
                title: "[b]Pair[/]".into(),
                children: vec![label("left").fixed(4), label("right")],
            })
            .auto(),
            label("below"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 14, 5));
    assert_eq!(rows[0], "╭─Pair───────╮");
    assert_eq!(rows[1], "│left right  │");
    assert_eq!(rows[2], "╰────────────╯");
    assert_eq!(rows[3], "below");
}

#[test]
fn built_in_nodes_are_widgets_too() {
    // A panel round a log round nothing special: the same nodes as before,
    // now each a widget, still drawing as they did.
    let app = App::new(|| {
        let log = intuituive::Log::new(10);
        for i in 0..5 {
            log.push(format!("line {i}"));
        }
        column([
            log.view().panel("Log").fixed(5),
            scroll(column((0..10).map(|i| label(format!("item {i}")).fixed(1)))),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run_open(app, Script::new(), 12, 7));
    assert_eq!(rows[0], "╭─ Log ────╮");
    assert_eq!(rows[1], "│line 2    │");
    assert_eq!(rows[3], "│line 4    │");
    assert_eq!(rows[5], "item 0     ┃");
}

#[test]
fn a_modal_takes_the_focus_from_the_screen_below_and_gives_it_back() {
    let seen = log();
    let (a, b) = (Watch::new("a", &seen), seen.clone());
    let app = App::new(move || {
        widget(a)
            .on_key("o", move |cx| {
                let b = Watch::new("b", &b);
                cx.modal(Size::Fixed(6), Size::Fixed(3), move || {
                    widget(b).on_key("x", |cx| cx.pop())
                })
            })
            .on_key("q", |cx| cx.quit())
    });
    run(app, Script::new().keys("o x q"), 20, 6);
    let events: Vec<String> = seen
        .borrow()
        .iter()
        .filter(|line| line.contains("focus"))
        .cloned()
        .collect();
    assert_eq!(
        events,
        [
            "a focus true",
            "a focus false",
            "b focus true",
            "a focus true"
        ]
    );
}

/// Notes its rectangle when it gets the focus.
struct Placed(Rc<RefCell<Option<Rect>>>);

impl Widget for Placed {
    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        if *event == WidgetEvent::Focus(true) {
            *self.0.borrow_mut() = Some(cx.rect());
        }
        Used::No
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[test]
fn a_widget_told_it_has_the_focus_is_already_laid_out() {
    let at = Rc::new(RefCell::new(None));
    let seen = at.clone();
    let app = App::new(move || {
        column([label("top").fixed(1), widget(Placed(seen)).fixed(2)]).on_key("q", |cx| cx.quit())
    });
    run(app, Script::new().keys("q"), 10, 4);
    assert_eq!(*at.borrow(), Some(Rect::new(0, 1, 10, 2)));
}

/// Ten rows; the one under the pointer is marked.
struct Rows;

impl Widget for Rows {
    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical => 10,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let under = cx.pointer().map(|(_, y)| y);
        for y in 0..canvas.height() {
            let mark = if under == Some(y) { ">" } else { " " };
            canvas.print(0, y, &format!("{mark}row{y}"), None);
        }
    }
}

#[test]
fn the_pointer_inside_a_scroll_follows_the_scroll() {
    let app = App::new(|| {
        column([label("head").fixed(1), scroll(widget(Rows))]).on_key("q", |cx| cx.quit())
    });
    let script = Script::new()
        .mouse(MouseKind::Moved, 1, 3)
        .scroll(true, 1, 3)
        .keys("q");
    let rows = screen(&run(app, script, 12, 6));
    // Scrolled three rows down, the pointer is over row 5.
    assert_eq!(rows[3], ">row5      ┃", "{rows:?}");
    assert!(!rows.iter().any(|r| r.starts_with(">row2")), "{rows:?}");
}

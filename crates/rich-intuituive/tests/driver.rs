//! Driving an app from a loop of your own: time, events, frames, the
//! clipboard and the frame as cells.

use std::time::Duration;

use intuituive::interact::{Button, Event, Key, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::Easing;

fn press(key: &str) -> Event {
    Event::Key(Key::parse(key).expect("a key name"))
}

#[test]
fn the_caller_feeds_events_and_writes_what_render_returns() {
    let app = App::new(|| {
        let count = signal(0);
        text!("count {count}")
            .on_key("+", move |_| count.update(|n| *n += 1))
            .on_key("q", |cx| cx.quit())
    });
    let mut driver = app.driver(12, 1);
    driver.update(Duration::ZERO);
    let first = driver.render().expect("the first frame draws");
    assert!(first.starts_with("\x1b[?25l") && first.contains("count 0"));
    // Nothing changed: nothing to draw.
    assert_eq!(driver.render(), None);
    driver.event(press("+"));
    driver.update(Duration::ZERO);
    let second = driver.render().expect("the count changed");
    // Only the digit is sent.
    assert!(
        second.ends_with('1') && !second.contains("count"),
        "{second:?}"
    );
    assert_eq!(driver.screen().plain()[0].trim_end(), "count 1");
    driver.event(press("q"));
    assert!(driver.is_done());
    assert!(driver.finish().ends_with("\x1b[?25h"));
}

#[test]
fn time_is_the_callers_timers_and_animations_follow_it() {
    let app = App::new(|| {
        let ticks = signal(0);
        text!("ticks {ticks}")
    })
    .every(Duration::from_millis(100), |_| {});
    let mut driver = app.driver(12, 1);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    // The next timer is due in 100 ms; the loop may wait that long.
    assert_eq!(
        driver.timeout(Duration::from_millis(60)),
        Duration::from_millis(40)
    );

    let app = App::new(|| {
        let x = signal(0.0f64);
        text!("x {:.0}", x.get()).on_key("a", move |cx| {
            cx.animate(x, 100.0, Duration::from_millis(200), Easing::Linear)
        })
    });
    let mut driver = app.driver(8, 1);
    driver.update(Duration::ZERO);
    driver.event(press("a"));
    for ms in [0, 100] {
        driver.update(Duration::from_millis(ms));
        let _ = driver.render();
    }
    assert_eq!(driver.screen().plain()[0].trim_end(), "x 50");
    // While it animates, the loop comes back within a frame.
    assert_eq!(
        driver.timeout(Duration::from_millis(100)),
        Duration::from_millis(16)
    );
    driver.update(Duration::from_millis(300));
    let _ = driver.render();
    assert_eq!(driver.screen().plain()[0].trim_end(), "x 100");
}

#[test]
fn a_resize_lays_the_app_out_again() {
    let app = App::new(|| label("a long line of text"));
    let mut driver = app.driver(30, 2);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    assert_eq!(driver.screen().plain()[0].trim_end(), "a long line of text");
    driver.resize(10, 3);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    assert_eq!(driver.screen().area().width, 10);
    assert_eq!(driver.screen().plain()[1].trim_end(), "line of");
    assert_eq!(driver.screen().plain()[2].trim_end(), "text");
}

#[test]
fn a_selection_is_handed_to_the_caller_for_the_clipboard() {
    let app = App::new(|| label("hello world"));
    let mut driver = app.driver(12, 1);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    let mouse = |kind, column| Event::Mouse(Mouse::new(kind, column, 0));
    // Without a clipboard, nothing is copied.
    driver.event(mouse(MouseKind::Down(Button::Left), 0));
    driver.event(mouse(MouseKind::Drag(Button::Left), 4));
    driver.event(mouse(MouseKind::Up(Button::Left), 4));
    assert!(driver.take_copies().is_empty());
    driver.set_clipboard(true);
    driver.event(mouse(MouseKind::Down(Button::Left), 6));
    driver.event(mouse(MouseKind::Drag(Button::Left), 10));
    driver.event(mouse(MouseKind::Up(Button::Left), 10));
    assert_eq!(driver.take_copies(), ["world"]);
}

#[test]
fn the_frame_comes_out_as_styled_lines_for_another_renderer() {
    let app = App::new(|| label("[bold]hi[/] there"));
    let mut driver = app.driver(10, 1);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    let lines = driver.screen().lines();
    let text: String = lines[0].iter().map(|s| s.text.as_str()).collect();
    assert_eq!(text, "hi there  ");
    assert_eq!(lines[0][0].text, "hi");
    let bold = lines[0][0].style.as_ref().map(|s| s.definition());
    assert_eq!(bold.as_deref(), Some("bold"));
}

#[test]
fn an_app_runs_inside_a_ratatui_buffer() {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;

    let app = App::new(|| {
        let count = signal(1);
        text!("count {count}")
            .panel("app")
            .on_key("+", move |_| count.update(|n| *n += 1))
    });
    let mut driver = app.driver(12, 3);
    driver.update(Duration::ZERO);
    driver.event(press("+"));
    driver.update(Duration::ZERO);
    let _ = driver.render();
    // ratatui's frame: the app in its right half.
    let mut buffer = Buffer::empty(Rect::new(0, 0, 24, 3));
    rich_ratatui::lines_to_buffer(
        &driver.screen().lines(),
        Rect::new(12, 0, 12, 3),
        &mut buffer,
    );
    let row = |y: u16| -> String {
        (0..24)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect()
    };
    assert_eq!(row(0), format!("{}╭─ app ────╮", " ".repeat(12)));
    assert_eq!(row(1), format!("{}│count 2   │", " ".repeat(12)));
}

#[test]
fn the_copy_toast_waits_for_the_caller_to_say_the_copy_worked() {
    let app = App::new(|| label("hello world"));
    let mut driver = app.driver(30, 3);
    driver.set_clipboard(true);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    let mouse = |kind, column| Event::Mouse(Mouse::new(kind, column, 0));
    driver.event(mouse(MouseKind::Down(Button::Left), 0));
    driver.event(mouse(MouseKind::Drag(Button::Left), 4));
    driver.event(mouse(MouseKind::Up(Button::Left), 4));
    let shown = |driver: &mut intuituive::Driver| {
        driver.update(Duration::from_millis(10));
        let _ = driver.render();
        driver.screen().plain().join("\n")
    };
    // The copy failed: no toast.
    assert_eq!(driver.take_copies(), ["hello"]);
    assert!(!shown(&mut driver).contains("Copied"));
    // It worked.
    driver.event(mouse(MouseKind::Down(Button::Left), 6));
    driver.event(mouse(MouseKind::Drag(Button::Left), 10));
    driver.event(mouse(MouseKind::Up(Button::Left), 10));
    for text in driver.take_copies() {
        driver.copied(&text);
    }
    assert!(shown(&mut driver).contains("Copied 5 characters"));
}

#[test]
fn a_containers_focus_style_goes_over_its_children_and_off_again() {
    let app = App::new(|| {
        let n = signal(0);
        column([
            row([text!("n{n}"), label("cd")])
                .focus_style("reverse")
                .on_key("+", move |_| n.update(|n| *n += 1))
                .fixed(1),
            label("other").focus_style("bold").fixed(1),
        ])
    });
    let mut driver = app.driver(10, 2);
    let reversed = |driver: &intuituive::Driver, x: u16| {
        let screen = driver.screen();
        screen
            .style(screen.cell(x, 0).style)
            .is_some_and(|style| style.definition().contains("reverse"))
    };
    let step = |driver: &mut intuituive::Driver, key: Option<&str>| {
        if let Some(key) = key {
            driver.event(press(key));
        }
        driver.update(Duration::ZERO);
        let _ = driver.render();
    };
    step(&mut driver, None);
    assert!(reversed(&driver, 0), "the row has the focus first");
    assert!(reversed(&driver, 9), "over both children");
    // A child drawing again keeps the style.
    step(&mut driver, Some("+"));
    assert_eq!(driver.screen().plain()[0].trim_end(), "n1   cd");
    assert!(reversed(&driver, 0) && reversed(&driver, 9));
    // The focus moves on: the style goes.
    step(&mut driver, Some("tab"));
    assert!(!reversed(&driver, 0) && !reversed(&driver, 9));
}

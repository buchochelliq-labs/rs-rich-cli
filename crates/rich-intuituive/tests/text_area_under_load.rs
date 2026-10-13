//! The text area under load and at its edges (the 0.0.19 release test).

mod common;

use std::time::{Duration, Instant};

use intuituive::interact::{Event, Key, Mouse, MouseKind};
use intuituive::prelude::*;

fn press(key: &str) -> Event {
    Event::Key(Key::parse(key).expect("a key name"))
}

fn rows(driver: &intuituive::Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|row| row.trim_end().to_string())
        .collect()
}

fn step(driver: &mut intuituive::Driver, event: Event) -> Duration {
    let t = Instant::now();
    driver.event(event);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    t.elapsed()
}

fn area_app(text: String, f: fn(Node) -> Node) -> App {
    App::new(move || {
        let value = signal(text.clone());
        column([f(text_area(value).autofocus())]).on_key("ctrl+q", |cx| cx.quit())
    })
}

#[test]
fn hundred_thousand_lines_keystroke_and_render() {
    let text: String = (0..100_000)
        .map(|i| format!("let line_{i} = {i} + 1; // some text"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut driver = area_app(text, |n| n.line_numbers()).driver(80, 24);
    let t = Instant::now();
    driver.update(Duration::ZERO);
    let _ = driver.render();
    eprintln!("first render: {:?}", t.elapsed());
    eprintln!("ctrl+end: {:?}", step(&mut driver, press("ctrl+end")));
    eprintln!(
        "type x: {:?}",
        step(&mut driver, Event::Key(Key::char('x')))
    );
    eprintln!(
        "type y: {:?}",
        step(&mut driver, Event::Key(Key::char('y')))
    );
    eprintln!("page up: {:?}", step(&mut driver, press("pageup")));
    eprintln!("ctrl+z: {:?}", step(&mut driver, press("ctrl+z")));
    let shown = rows(&driver);
    eprintln!("{:#?}", &shown[..3]);
}

#[test]
fn hundred_thousand_lines_with_limit_keystroke() {
    // Not reachable from text_area's builders? check only Buffer for limit.
}

/// Scrolling down with the wheel inside the last (long, wrapped) line
/// jumps back to its first row.
#[test]
fn wheel_down_in_a_long_last_line_does_not_scroll_up() {
    let text = (0..200)
        .map(|i| char::from(b'a' + (i % 26) as u8))
        .collect::<String>();
    let mut driver = area_app(text, |n| n).driver(21, 3);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    step(&mut driver, press("ctrl+end"));
    let before = rows(&driver);
    let wheel = Event::Mouse(Mouse {
        kind: MouseKind::ScrollDown,
        column: 1,
        row: 1,
        modifiers: Default::default(),
    });
    step(&mut driver, wheel);
    let after = rows(&driver);
    eprintln!("before {before:?}\nafter  {after:?}");
    assert_eq!(after, before, "wheel down moved the view back up");
}

#[test]
fn paste_with_controls_and_read_only_and_placeholder() {
    let mut driver = area_app(String::new(), |n| {
        n.placeholder("type\x1b[2J here").line_numbers()
    })
    .driver(30, 3);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    eprintln!("placeholder: {:?}", rows(&driver));
    step(
        &mut driver,
        Event::Paste("a\x1b[31mb\x07c\r\nd\u{9b}e\tf".into()),
    );
    let shown = rows(&driver);
    eprintln!("pasted: {shown:?}");
    assert!(!shown.join("").contains('\x1b'));
    step(&mut driver, press("ctrl+z"));
    eprintln!("undone: {:?}", rows(&driver));
    step(&mut driver, press("ctrl+y"));
    eprintln!("redone: {:?}", rows(&driver));

    let mut ro = area_app("abc".into(), |n| n.read_only()).driver(30, 3);
    ro.update(Duration::ZERO);
    let _ = ro.render();
    step(&mut ro, Event::Paste("zzz".into()));
    step(&mut ro, Event::Key(Key::char('q')));
    step(&mut ro, press("enter"));
    step(&mut ro, press("backspace"));
    assert_eq!(rows(&ro)[0], "abc");
}

/// Wide and combining characters wrap and place the caret.
#[test]
fn wide_and_combining() {
    let text = "日本語のテキスト e\u{301}e\u{301} 👨‍👩‍👧👨‍👩‍👧 end".to_string();
    let mut driver = area_app(text, |n| n.line_numbers()).driver(12, 6);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    step(&mut driver, press("end"));
    eprintln!("{:#?}", rows(&driver));
    for _ in 0..30 {
        step(&mut driver, press("left"));
    }
    step(&mut driver, press("shift+end"));
    eprintln!("{:#?}", rows(&driver));
}

/// One very long line: a keystroke's cost, wrapped and not.
#[test]
fn a_very_long_line_keystroke() {
    for wrap in [true, false] {
        let text = "abcdefghij".repeat(20_000);
        let mut driver = if wrap {
            area_app(text, |n| n)
        } else {
            area_app(text, |n| n.soft_wrap(false))
        }
        .driver(80, 24);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        eprintln!(
            "wrap {wrap}: ctrl+end {:?}",
            step(&mut driver, press("ctrl+end"))
        );
        eprintln!(
            "wrap {wrap}: type x {:?}",
            step(&mut driver, Event::Key(Key::char('x')))
        );
        eprintln!("wrap {wrap}: left {:?}", step(&mut driver, press("left")));
        eprintln!("wrap {wrap}: home {:?}", step(&mut driver, press("home")));
    }
}

/// A keystroke at the end of one very long wrapped line costs the same
/// whatever the height: following the caret wraps each line once, not once
/// a row (it took four times as long at 48 rows as at 12).
#[test]
fn a_very_long_wrapped_line_keystroke_does_not_scale_with_height() {
    let mut took = Vec::new();
    for height in [12u16, 48] {
        let text = "abcdefghij".repeat(20_000);
        let mut driver = area_app(text, |n| n).driver(80, height);
        driver.update(Duration::ZERO);
        let _ = driver.render();
        step(&mut driver, press("ctrl+end"));
        let e = step(&mut driver, Event::Key(Key::char('x')));
        eprintln!("height {height}: type x {e:?}");
        took.push(e);
    }
    assert!(
        took[1] < took[0] * 5 / 2 + Duration::from_millis(100),
        "{took:?}"
    );
}

/// Which cells of each row are drawn reversed (the selection).
fn reversed(driver: &intuituive::Driver) -> Vec<String> {
    let screen = driver.screen();
    let area = screen.area();
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| {
                    let cell = screen.cell(x, y);
                    let rev = screen
                        .style(cell.style)
                        .is_some_and(|s| s.attr(6) == Some(true));
                    if rev {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect()
        })
        .collect()
}

#[test]
fn selection_over_wrapped_rows() {
    let mut driver = area_app("abcdefghijklmnop\nxy".into(), |n| n).driver(8, 4);
    driver.update(Duration::ZERO);
    let _ = driver.render();
    // Caret to (0, 3), select to (1, 1).
    for k in ["right", "right", "right", "shift+down", "shift+right"] {
        step(&mut driver, press(k));
    }
    eprintln!("{:#?}\n{:#?}", rows(&driver), reversed(&driver));
}

//! Maximizing a pane over the whole screen, and restoring it (#807).

use intuituive::prelude::*;

#[test]
fn a_maximized_pane_fills_the_screen_and_esc_restores_it() {
    let app = App::new(|| {
        row([
            label("left").fixed(10).name("left").focusable(),
            column([label("diff").name("diff").focusable()])
                .panel("Diff")
                .name("pane")
                .maximizable(),
        ])
    })
    .maximize_key("ctrl+m");
    let mut driver = app.driver(80, 24);
    driver.settle();
    driver.press("tab");
    driver.assert_focused("diff");
    driver.press("ctrl+m");
    driver.settle();
    let lines = driver.screen().plain();
    // The panel's border runs from the first column to the last, top to
    // bottom.
    assert!(lines[0].starts_with('╭'), "{:?}", lines[0]);
    assert!(lines[0].ends_with('╮'), "{:?}", lines[0]);
    assert!(lines[23].starts_with('╰'), "{:?}", lines[23]);
    assert!(!lines.iter().any(|line| line.contains("left")));
    driver.assert_focused("diff");
    // Tab stays inside the maximized pane.
    driver.press("tab");
    driver.assert_focused("diff");
    let said: Vec<String> = driver
        .take_announcements()
        .into_iter()
        .map(|a| a.text)
        .collect();
    assert!(said.contains(&"maximized".to_string()), "{said:?}");
    driver.press("esc");
    driver.settle();
    let lines = driver.screen().plain();
    assert!(lines[0].starts_with("left"), "{:?}", lines[0]);
    driver.assert_focused("diff");
}

#[test]
fn a_child_that_cannot_be_maximized_maximizes_its_nearest_ancestor() {
    let app = App::new(|| {
        row([
            label("other").fixed(10),
            column([
                label("title"),
                label("inner")
                    .name("inner")
                    .focusable()
                    .bind("z", "Zoom", |cx| cx.maximize()),
            ])
            .name("outer")
            .maximizable(),
        ])
    });
    let mut driver = app.driver(30, 4);
    driver.settle();
    driver.press("z");
    driver.settle();
    let lines = driver.screen().plain();
    assert_eq!(lines[0].trim_end(), "title");
    assert!(lines.iter().any(|line| line.trim_end() == "inner"));
    assert!(!lines.iter().any(|line| line.contains("other")));
}

#[test]
fn signals_written_while_maximized_redraw_hidden_panes_after_restore() {
    let app = App::new(move || {
        let count = signal(0u32);
        row([
            text!("count {count}").fixed(10),
            label("pane")
                .name("pane")
                .focusable()
                .maximizable()
                .bind("ctrl+m", "Zoom", |cx| {
                    if cx.is_maximized() {
                        cx.minimize()
                    } else {
                        cx.maximize()
                    }
                }),
        ])
        .bind("+", "More", move |_| count.set(5))
    });
    let mut driver = app.driver(20, 1);
    driver.settle();
    assert!(driver.screen().plain()[0].starts_with("count 0"));
    driver.press("ctrl+m");
    driver.settle();
    driver.press("+");
    driver.settle();
    // Hidden: not drawn while the pane is maximized.
    assert_eq!(driver.screen().plain()[0].trim_end(), "pane");
    driver.press("ctrl+m");
    driver.settle();
    assert!(driver.screen().plain()[0].starts_with("count 5"));
}

#[test]
fn delayed_outside_autofocus_keeps_focus_and_keys_inside_a_maximized_pane() {
    use std::{cell::Cell, rc::Rc};

    let actions = Rc::new(Cell::new((0, 0)));
    let seen = actions.clone();
    let app = App::new(move || {
        let inside = actions.clone();
        let outside = actions.clone();
        let show_outside = signal(false);
        row([
            column([label("inside")
                .name("inside")
                .focusable()
                .on_key("x", move |_| {
                    let (a, b) = inside.get();
                    inside.set((a + 1, b));
                })])
            .maximizable(),
            label("outside")
                .name("outside")
                .focusable()
                .autofocus()
                .shown_when(move || show_outside.get())
                .on_key("x", move |_| {
                    let (a, b) = outside.get();
                    outside.set((a, b + 1));
                }),
        ])
        .on_key("s", move |_| show_outside.set(true))
    })
    .maximize_key("ctrl+m");
    let mut driver = app.driver(30, 3);
    driver.settle();
    driver.assert_focused("inside");
    driver.press("ctrl+m");
    driver.assert_focused("inside");
    driver.press("s x");
    assert!(
        !driver
            .screen()
            .plain()
            .iter()
            .any(|line| line.contains("outside"))
    );
    assert_eq!(seen.get(), (1, 0), "keys escaped the maximized pane");
    driver.assert_focused("inside");

    // Restoring removes the boundary, so delayed autofocus can take over.
    driver.press("esc");
    driver.assert_focused("outside");
    driver.press("x");
    assert_eq!(seen.get(), (1, 1));
}

#[test]
fn clicks_reach_only_the_maximized_pane() {
    let hits = std::rc::Rc::new(std::cell::Cell::new(0u32));
    let seen = hits.clone();
    let app = App::new(move || {
        let hits = hits.clone();
        row([
            label("left")
                .fixed(10)
                .on_click(move |_| hits.set(hits.get() + 1)),
            label("pane").name("pane").focusable().maximizable(),
        ])
    })
    .maximize_key("ctrl+m");
    let mut driver = app.driver(20, 1);
    driver.settle();
    driver.press("tab");
    driver.press("ctrl+m");
    driver.settle();
    driver.click(2, 0);
    driver.settle();
    assert_eq!(seen.get(), 0);
}

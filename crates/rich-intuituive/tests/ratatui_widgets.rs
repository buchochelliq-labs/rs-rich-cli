//! ratatui widgets inside an intuiTUIve app, through rs-rich-ratatui's
//! `RatatuiComponent`: the porting guide's "keep your widgets" step.

mod common;

use common::{run, screen};
use intuituive::prelude::*;
use ratatui::widgets::{Block, Gauge, Paragraph, Widget};
use rich_interact::headless::Script;
use rich_ratatui::RatatuiComponent;

#[test]
fn a_ratatui_widget_draws_app_state_and_follows_it() {
    let app = App::new(|| {
        let done = signal(1u16);
        let gauge = RatatuiComponent::<(), ()>::new(move |area, buf| {
            // An unchanged ratatui widget, reading a signal.
            Gauge::default()
                .block(Block::bordered().title("Progress"))
                .percent(done.get() * 10)
                .render(area, buf);
        });
        column([
            component(gauge, |_, _| {}).no_focus().fixed(3),
            text!("done {done}/10").auto(),
        ])
        .on_key("+", move |_| done.update(|d| *d += 1))
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("+ + q"), 30, 5));
    assert!(rows[0].contains("Progress"), "{rows:?}");
    assert!(
        rows[1].contains("30%"),
        "the gauge follows the signal: {rows:?}"
    );
    assert_eq!(rows[3], "done 3/10");
}

#[test]
fn a_ratatui_widget_sits_beside_native_nodes() {
    let app = App::new(|| {
        let legacy = RatatuiComponent::<(), ()>::new(|area, buf| {
            Paragraph::new("from ratatui")
                .block(Block::bordered().title("Old"))
                .render(area, buf);
        });
        row([
            component(legacy, |_, _| {}).no_focus(),
            label("from intuiTUIve").panel("New"),
        ])
        .on_key("q", |cx| cx.quit())
    });
    let rows = screen(&run(app, Script::new().keys("q"), 50, 3));
    assert!(
        rows[1].contains("from ratatui") && rows[1].contains("from intuiTUIve"),
        "{rows:?}"
    );
}

//! A measurement, run by hand (`cargo test --test scroll_virtual_measure --
//! --ignored --nocapture`): frame time on a held PageDown, with and without
//! stand-ins, for cards (a panel of styled text) and one-line labels.
use intuituive::prelude::*;
use std::time::Instant;

fn item(i: usize) -> Node {
    column([
        label(format!(
            "[bold]Item {i}[/] · [dim]{} words of body text[/]",
            i % 97
        ))
        .fixed(1),
        text(move || format!("[italic]{}[/]", "lorem ipsum dolor sit amet ".repeat(6))).fixed(2),
    ])
    .panel(&format!("Card {i}"))
    .fixed(5)
}

fn run(stand_in: bool, cheap: bool) -> (f64, u64) {
    let app = App::new(move || {
        let view = if cheap {
            scroll_virtual(|| 100_000, |_| 5, |i| label(format!("item {i}")).fixed(5))
        } else {
            scroll_virtual(|| 100_000, |_| 5, item)
        };
        if stand_in {
            view.stand_in(|_| label("░░░░░░").fixed(5))
        } else {
            view
        }
    });
    let mut driver = app.driver(80, 40);
    driver.settle();
    let start = Instant::now();
    // A held PageDown: 200 pages with no time passing between them.
    for _ in 0..200 {
        driver.press("pagedown");
    }
    let fast = start.elapsed().as_secs_f64() * 1000.0;
    driver.advance(std::time::Duration::from_millis(200));
    (fast, driver.stats().frames)
}

#[test]
#[ignore]
fn stand_ins_cut_frame_time_on_a_held_page_down() {
    for cheap in [false, true] {
        let (full, f1) = run(false, cheap);
        let (lod, f2) = run(true, cheap);
        eprintln!("cheap {cheap}: full items: {full:.1} ms ({f1} frames); stand-ins: {lod:.1} ms ({f2} frames)");
    }
}

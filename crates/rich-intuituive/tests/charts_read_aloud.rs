//! Charts that read aloud (#726): a `described(..)` node draws a chart and
//! is named by the chart's accessible text, in the accessibility tree and
//! in linear mode, and says it again when the data changes.

use std::time::Duration;

use intuituive::a11y::Role;
use intuituive::interact::{Event, Key};
use intuituive::prelude::*;
use rich_ext::chart::{Gauge, LineChart, Series, Sparkline};

fn frame(driver: &mut intuituive::Driver, key: Option<&str>) -> String {
    if let Some(key) = key {
        driver.event(Event::Key(Key::parse(key).expect("a key")));
    }
    driver.update(Duration::ZERO);
    driver.render().unwrap_or_default()
}

fn app() -> App {
    App::new(|| {
        let load = signal(vec![2.0, 4.0, 3.0, 9.0]);
        column([
            described(move || Sparkline::new(load.get()))
                .label("Load")
                .fixed(1),
            described(|| {
                LineChart::new()
                    .series(Series::from_values("p50", [10.0, 12.0, 11.0]))
                    .height(3)
            })
            .fixed(6),
            described(|| Gauge::new("disk", 42.0).range(0.0, 100.0).unit("%")).fixed(1),
        ])
        .on_key("n", move |_| load.update(|v| v.push(1.0)))
    })
}

#[test]
fn a_described_chart_is_named_by_what_it_shows() {
    let mut driver = app().driver(40, 8);
    frame(&mut driver, None);
    let tree: Vec<(Role, String)> = driver
        .accessibility()
        .into_iter()
        .map(|n| (n.role, n.name))
        .collect();
    assert_eq!(
        tree,
        vec![
            (
                Role::Text,
                "Load: Sparkline, 4 values: min 2, max 9, last 9, trend up".to_string()
            ),
            (
                Role::Text,
                "Line chart, 1 series, x from 0 to 2, y from 10 to 12. \
                 p50, line: 3 points, min 10, max 12, last 11, trend up"
                    .to_string()
            ),
            (
                Role::Text,
                "Gauge disk: 42%, 42 percent of 0% to 100%".to_string()
            ),
        ]
    );
    // The glyphs are still drawn.
    assert!(driver.screen().plain()[0].contains('█'));
}

#[test]
fn linear_mode_reads_the_description_and_its_changes() {
    let mut driver = app().linear(true).driver(40, 8);
    let first = frame(&mut driver, None);
    assert!(
        first.contains("Load: Sparkline, 4 values: min 2, max 9, last 9, trend up\r\n"),
        "{first:?}"
    );
    assert!(!first.contains('▁'), "{first:?}");
    // New data: only the line that changed is written again.
    let next = frame(&mut driver, Some("n"));
    assert_eq!(
        next,
        "Load: Sparkline, 5 values: min 1, max 9, last 1, trend up\r\n"
    );
}

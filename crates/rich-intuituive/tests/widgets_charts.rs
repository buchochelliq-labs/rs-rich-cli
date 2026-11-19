//! The `viz` plugin's chart nodes: a line chart, a bar chart and a
//! heatmap over signals, their cursor by the keys and the mouse, its
//! readout, a rolling window over streaming data, and what a screen
//! reader is told.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::interact::{Event, Mouse, MouseKind};
use intuituive::prelude::*;
use intuituive::{Driver, HeatGrid, Window, bar_chart, heatmap, line_chart};
use rich_ext::chart::Series;

fn last_row(driver: &Driver) -> String {
    let rows = driver.screen().plain();
    rows[rows.len() - 1].trim_end().to_string()
}

#[test]
fn a_line_charts_cursor_moves_by_keys_and_the_pointer_and_reads_each_series() {
    let app = App::new(|| {
        let load = signal(vec![1.0, 3.0, 2.0, 5.0, 4.0]);
        let cpu = signal(vec![2.0, 2.0, 4.0, 1.0, 0.0]);
        line_chart(move || {
            vec![
                Series::from_values("load", load.get()),
                Series::from_values("cpu", cpu.get()),
            ]
        })
    });
    let mut driver = app.driver(30, 12);
    driver.settle();
    // No cursor: the newest x.
    assert_eq!(last_row(&driver), "x 4 · load 4 · cpu 0");
    driver.press("home");
    assert_eq!(last_row(&driver), "x 0 · load 1 · cpu 2");
    // The cursor's column is reversed, down the plot.
    let screen = driver.screen();
    let reverse = rich::Style::parse("reverse").unwrap();
    let left = (0..30u16)
        .find(|&x| {
            screen
                .style(screen.cell(x, 0).style)
                .is_some_and(|s| s.reverse() == reverse.reverse())
        })
        .expect("a reversed column");
    // The pointer over the plot's last column: the last point.
    driver.send(Event::Mouse(Mouse::new(MouseKind::Moved, 29, 3)));
    assert_eq!(last_row(&driver), "x 4 · load 4 · cpu 0");
    driver.send(Event::Mouse(Mouse::new(MouseKind::Moved, left, 3)));
    assert_eq!(last_row(&driver), "x 0 · load 1 · cpu 2");
    // Esc takes the cursor away.
    driver.press("escape");
    assert_eq!(last_row(&driver), "x 4 · load 4 · cpu 0");
    let node = &driver.accessibility()[0];
    assert_eq!(
        node.describe(),
        "Line chart: load, cpu, image: x 4 · load 4 · cpu 0"
    );
}

#[test]
fn a_rolling_window_keeps_the_newest_points_and_draws_only_when_they_change() {
    let out: Rc<Cell<Option<Signal<Vec<f64>>>>> = Rc::default();
    let set = out.clone();
    let app = App::new(move || {
        let history = signal((0..10).map(f64::from).collect::<Vec<_>>());
        set.set(Some(history));
        line_chart(move || vec![Series::from_values("n", history.get())])
            .chart_window(Window::Last(3))
    });
    let mut driver = app.driver(30, 12);
    driver.settle();
    let history = out.get().unwrap();
    assert_eq!(last_row(&driver), "x 9 · n 9");
    driver.press("home");
    // The window's first point: x 7.
    assert_eq!(last_row(&driver), "x 7 · n 7");
    // A change before the window draws nothing.
    let frames = driver.stats().frames;
    history.update(|h| h[0] = 100.0);
    driver.settle();
    assert_eq!(driver.stats().frames, frames);
    // A new point moves the window on.
    history.update(|h| h.push(10.0));
    driver.settle();
    assert_eq!(last_row(&driver), "x 8 · n 8");
    assert!(driver.stats().frames > frames);
}

#[test]
fn a_span_window_keeps_the_points_within_it_of_the_newest() {
    let app = App::new(|| {
        let points: Vec<(f64, f64)> = (0..20).map(|t| (t as f64, (t * 2) as f64)).collect();
        line_chart(move || vec![Series::line("v", points.clone())])
            .chart_window(Window::Span(Duration::from_secs(5)))
    });
    let mut driver = app.driver(30, 12);
    driver.press("home");
    assert_eq!(last_row(&driver), "x 14 · v 28");
}

#[test]
fn a_bar_charts_cursor_goes_from_bar_to_bar_and_a_click_picks_one() {
    let app = App::new(|| {
        bar_chart(|| {
            vec![
                ("api".to_string(), 4.0),
                ("web".to_string(), 2.0),
                ("db".to_string(), 7.0),
            ]
        })
    });
    let mut driver = app.driver(24, 4);
    driver.settle();
    assert_eq!(last_row(&driver), "db 7");
    // The first key puts the cursor on the last bar.
    driver.press("up up up");
    assert_eq!(last_row(&driver), "api 4");
    let styled = intuituive::testing::styled(&driver);
    assert!(
        styled.lines().next().unwrap().contains("reverse"),
        "{styled}"
    );
    driver.click(2, 1);
    assert_eq!(last_row(&driver), "web 2");
    assert_eq!(
        driver.accessibility()[0].describe(),
        "Bar chart: api, web, db, image: web 2"
    );
}

#[test]
fn a_heatmaps_cursor_reads_a_column_and_the_window_keeps_the_newest() {
    let app = App::new(|| {
        let grid = HeatGrid::new()
            .columns(["mon", "tue", "wed", "thu"])
            .row("api", [1.0, 4.0, 9.0, 2.0])
            .row("web", [0.0, 6.0, 8.0, f64::NAN]);
        column([
            heatmap({
                let grid = grid.clone();
                move || grid.clone()
            }),
            heatmap(move || grid.clone()).chart_window(Window::Last(2)),
        ])
    });
    let mut driver = app.driver(30, 12);
    driver.settle();
    let rows = driver.screen().plain();
    let readouts: Vec<&str> = rows
        .iter()
        .map(|r| r.trim_end())
        .filter(|r| r.contains(" · api"))
        .collect();
    assert_eq!(readouts, ["thu · api 2 · web -", "thu · api 2 · web -"]);
    driver.press("home");
    let rows = driver.screen().plain();
    assert!(
        rows.iter().any(|r| r.trim_end() == "mon · api 1 · web 0"),
        "{rows:#?}"
    );
    // The windowed one starts at wed.
    driver.press("tab home");
    let rows = driver.screen().plain();
    assert!(
        rows.iter().any(|r| r.trim_end() == "wed · api 9 · web 8"),
        "{rows:#?}"
    );
}

#[test]
fn a_line_chart_sized_to_itself_keeps_its_axis_and_legend() {
    // In a column with its default size, the chart is measured at its own
    // height, not cut to one plot row and the readout.
    let app = App::new(|| {
        column([
            line_chart(|| vec![Series::from_values("load", [1.0, 3.0, 2.0, 5.0, 4.0])]),
            label("after"),
        ])
    });
    let mut driver = app.driver(30, 30);
    driver.settle();
    let rows: Vec<String> = driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect();
    let after = rows.iter().position(|r| r == "after").expect("the label");
    assert!(after > 4, "{rows:#?}");
    assert!(
        rows[..after].iter().any(|r| r.contains("load")),
        "{rows:#?}"
    );
}

//! Charts fill the node they are in (0.0.19 gap batch C0): `renderable(..)`
//! passes the node's height, and a `Chart`, a `Canvas`, a vertical
//! `BarChart`, a `Sparkline` and a `FillGauge` given no height of their own
//! take it, as ratatui's widgets fill their area.

mod common;

use common::{run, screen};
use intuituive::interact::headless::Script;
use intuituive::prelude::*;
use rich_ext::chart::{
    Axis, BarChart, Canvas, Chart, Dataset, FillGauge, GraphType, Labels, Marker, Orientation,
    Sparkline,
};

/// `chart` in a node `rows` tall, over a one-row footer, at `width`.
fn in_a_node(chart: impl Fn() -> Node + 'static, width: u16, rows: u16) -> Vec<String> {
    let app = App::new(move || {
        column([chart().fixed(rows), label("footer").fixed(1)]).on_key("q", |cx| cx.quit())
    });
    screen(&run(app, Script::new().keys("q"), width, rows + 1))
}

fn line_chart() -> Chart {
    Chart::new(vec![Dataset::new([(0.0, 0.0), (1.0, 1.0)])
        .graph_type(GraphType::Line)
        .marker(Marker::Ascii('*'))])
    .x_axis(
        Axis::default()
            .bounds(0.0, 1.0)
            .labels(Labels::Values(vec![0.0, 1.0])),
    )
    .y_axis(
        Axis::default()
            .bounds(0.0, 1.0)
            .labels(Labels::Values(vec![0.0, 1.0])),
    )
}

#[test]
fn a_chart_fills_its_node() {
    for rows in [4u16, 7, 12] {
        let screen = in_a_node(|| renderable(line_chart), 20, rows);
        let rows = rows as usize;
        // The x labels on the node's last row, the axis line above them,
        // and the y axis's top label on its first row.
        assert!(screen[rows - 1].contains('0'), "{screen:?}");
        assert!(screen[rows - 2].contains('└'), "{screen:?}");
        assert!(screen[0].starts_with('1'), "{screen:?}");
        assert_eq!(screen[rows], "footer");
    }
    // Its own height wins: three rows, then blank ones.
    let screen = in_a_node(|| renderable(|| line_chart().height(3)), 20, 7);
    assert!(screen[1].contains('└') && screen[3..7].iter().all(String::is_empty));
}

#[test]
fn a_canvas_fills_its_node() {
    let canvas = || {
        Canvas::new().marker(Marker::Ascii('#')).paint(|p| {
            p.line(0.0, 0.0, 0.0, 1.0, "");
        })
    };
    let screen = in_a_node(move || renderable(canvas), 6, 5);
    assert_eq!(screen[..5], ["#", "#", "#", "#", "#"]);
}

#[test]
fn vertical_bars_fill_their_node() {
    let bars = || {
        BarChart::from_pairs([("a", 4.0), ("b", 2.0)])
            .orientation(Orientation::Vertical)
            .show_values(false)
    };
    // Six rows: five of bars and the labels.
    let screen = in_a_node(move || renderable(bars), 10, 6);
    assert_eq!(screen[..6], ["█", "█", "█ ▄", "█ █", "█ █", "a b"]);
}

#[test]
fn a_sparkline_fills_its_node() {
    let spark = || Sparkline::new([1.0, 2.0, 3.0, 4.0]);
    let screen = in_a_node(move || renderable(spark), 10, 2);
    assert_eq!(screen[..2], ["  ▃█", "▁▆██"]);
    // In a row of its own, one row as before.
    let screen = in_a_node(move || renderable(spark), 10, 1);
    assert_eq!(screen[0], "▁▃▆█");
}

#[test]
fn a_fill_gauge_fills_its_node() {
    let gauge = || FillGauge::new(0.5);
    let screen = in_a_node(move || renderable(gauge), 10, 3);
    assert_eq!(screen[..3], ["█████", "███50%", "█████"]);
}

#[test]
fn a_flexible_node_gives_a_chart_what_is_left() {
    let app = || {
        App::new(|| {
            column([
                label("title").fixed(1),
                renderable(line_chart).flex(1),
                label("footer").fixed(1),
            ])
            .on_key("q", |cx| cx.quit())
        })
    };
    for height in [8u16, 14] {
        let screen = screen(&run(app(), Script::new().keys("q"), 20, height));
        let h = height as usize;
        // The chart takes the rows between the title and the footer.
        assert!(screen[1].starts_with('1'), "{screen:?}");
        assert!(screen[h - 3].contains('└'), "{screen:?}");
        assert_eq!(screen[h - 1], "footer");
    }
}

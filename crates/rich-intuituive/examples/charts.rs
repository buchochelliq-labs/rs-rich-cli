//! A port of ratatui's chart examples, as one gallery rebuilt on
//! intuiTUIve: the `gauge`, `line-gauge`, `sparkline`, `barchart`,
//! `barchart-grouped`, `chart` and `canvas` widget examples
//! (<https://github.com/ratatui/ratatui/tree/main/ratatui-widgets/examples>),
//! and the `gauge`, `weather` and `chart` example apps
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps>). It
//! reuses none of ratatui's code: it rebuilds the behaviour and the look
//! with intuiTUIve's API and rs-rich-ext's charts.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The
//! numbers, the companies and the weather are our own, and the map is
//! Natural Earth's, in the public domain.
//!
//!     cargo run -p rs-rich-intuituive --example charts
//!     cargo run -p rs-rich-intuituive --example charts -- --tick-rate 100
//!     cargo run -p rs-rich-intuituive --example charts -- --frozen
//!
//! Tabs: Gauge, Sparkline, Bars, Groups, Chart and Canvas. ←/→ (or h/l) or
//! 1 to 6 change tabs, and a click picks one; m changes the canvas's
//! marker; Space stops and starts the clock; ? lists the keys, Ctrl+P
//! finds a command, q quits.
//!
//! `--tick-rate MS` sets how often the data moves (200 ms by default).
//! `--frozen` moves the data on [`FROZEN_TICKS`] ticks (the first gauge at
//! 42%) and stops the clock, for screenshots that do not move. `--linear`
//! runs it in linear mode.
//!
//! What intuiTUIve adds to the originals:
//!
//! - **Only what moves redraws.** The gauges, the sparklines and the moving
//!   chart read the clock; a tick draws them and nothing else
//!   (`tests/charts.rs` checks it).
//! - **The charts have labels**, so linear mode reads them as words rather
//!   than Braille.
//! - **The keys have descriptions**, so `?` lists them and Ctrl+P finds
//!   them; the mouse picks the tabs.

use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::{Console, ConsoleOptions};
use intuituive::widgets::tabs;
use rich_ext::chart::{
    Axis, Bar, BarChart, BarGroup, Canvas, Charset, Chart, Dataset, Direction, FillGauge,
    GraphType, Labels, LegendPosition, LineGauge, LineSet, MapResolution, Marker, Orientation,
    Sparkline,
};

/// The tabs, in order.
pub const TABS: [&str; 6] = ["Gauge", "Sparkline", "Bars", "Groups", "Chart", "Canvas"];

/// How the gallery runs: its command line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// How often the data moves.
    pub tick_rate: Duration,
    /// Move the data on [`FROZEN_TICKS`] ticks, then stop the clock.
    pub frozen: bool,
    /// The seed of the random values.
    pub seed: u64,
    /// The tab it opens on.
    pub tab: usize,
    /// Run in linear mode.
    pub linear: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tick_rate: Duration::from_millis(200),
            frozen: false,
            seed: 0x5eed_c4a7,
            tab: 0,
            linear: false,
        }
    }
}

/// How far `--frozen` moves the data before it stops the clock: the first
/// gauge then shows 42%.
pub const FROZEN_TICKS: u64 = 42;

impl Options {
    /// `--tick-rate MS`, `--frozen`, `--tab N` (1 to 6) and `--linear`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let (name, inline) = match arg.split_once('=') {
                Some((name, value)) => (name.to_string(), Some(value.to_string())),
                None => (arg.clone(), None),
            };
            let mut value = |what: &str| {
                inline
                    .clone()
                    .or_else(|| args.next())
                    .ok_or_else(|| format!("{name} takes {what}"))
            };
            match name.as_str() {
                "--tick-rate" | "-t" => {
                    let ms: u64 = value("a number of milliseconds")?
                        .parse()
                        .map_err(|_| format!("{name} takes a number of milliseconds"))?;
                    options.tick_rate = Duration::from_millis(ms.max(1));
                }
                "--tab" => {
                    let n: usize = value("a number from 1 to 6")?
                        .parse()
                        .ok()
                        .filter(|n| (1..=TABS.len()).contains(n))
                        .ok_or("--tab takes a number from 1 to 6")?;
                    options.tab = n - 1;
                }
                "--frozen" => options.frozen = true,
                "--linear" => options.linear = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// A seeded xorshift: the same numbers every run (the crate has no `rand`).
#[derive(Clone, Copy, Debug)]
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> XorShift {
        XorShift(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
}

/// How many random values the sparklines draw from; their window moves a
/// value each tick and wraps round.
pub const SAMPLES: usize = 512;

/// The options a leaf renders with, at its width and height (a height of
/// 0 asks how tall it would like to be), as `renderable(..)` does.
fn sized(console: &Console, width: u16, height: u16) -> ConsoleOptions {
    let options = console.options().update_width(width.max(1) as usize);
    if height == 0 {
        options
    } else {
        options.update_height(height as usize)
    }
}

/// `count` of `values` from `from`, going round.
fn window(values: &[u64], from: usize, count: usize) -> Vec<u64> {
    (0..count)
        .map(|i| values[(from + i) % values.len()])
        .collect()
}

/// The gauges' ratios after `tick` ticks: each fills at its own speed and
/// starts again when full.
pub fn gauge_ratios(tick: u64) -> [f64; 4] {
    let at = |per_tick: u64| (tick * per_tick % 1000) as f64 / 1000.0;
    [at(10), at(7), at(13), at(5)]
}

/// Gauge: ratatui's `gauge` app and `line-gauge` example.
fn gauges(ticks: Signal<u64>) -> Node {
    let gauge = |i: usize, title: &str, make: fn(f64) -> FillGauge| {
        renderable(move || make(gauge_ratios(ticks.get())[i]))
            .name(&format!("gauge-{}", i + 1))
            .label(format!("{title}, a gauge"))
            .panel(title)
            .flex(1)
            .min_size(3)
    };
    let line = |ratio: f64, look: LineSet, label: &str, style: &str| {
        LineGauge::new(ratio)
            .label(format!("{label}: {:>3.0}%", ratio * 100.0))
            .line(look)
            .filled_style(style)
            .unfilled_style("bright_black")
    };
    let lines = leaf(move |console, width, height| {
        let [a, b, c, d] = gauge_ratios(ticks.get());
        let gauges = [
            line(a, LineSet::Thin, "Thin  ", "green"),
            line(b, LineSet::Thick, "Thick ", "magenta"),
            line(c, LineSet::Double, "Double", "cyan"),
            line(d, LineSet::Thin, "Shapes", "yellow").symbols('▰', '▱'),
        ];
        let options = sized(console, width, 1);
        gauges
            .iter()
            .take(height.max(4) as usize)
            .flat_map(|g| console.render_lines(g, &options, false))
            .collect()
    })
    .name("line-gauges")
    .label("Four line gauges")
    .panel("Line gauges")
    .fixed(6);
    column([
        gauge(0, "Percentage", |r| FillGauge::new(r).style("red on black")),
        gauge(1, "Ratio, with a label of its own", |r| {
            FillGauge::new(r)
                .label(format!("{:.0} of 1000 MB", r * 1000.0))
                .style("green on black")
                .label_style("bold")
        }),
        gauge(2, "Whole cells", |r| {
            FillGauge::new(r).style("blue on black").unicode(false)
        }),
        gauge(3, "Unicode: eighths of a cell", |r| {
            FillGauge::new(r)
                .label(format!("{:.2}%", r * 100.0))
                .style("yellow on black italic")
                .unicode(true)
        }),
        lines,
    ])
}

/// Sparkline: left to right; right to left with gaps; Braille; and the
/// same values as bars beside them.
fn sparklines(ticks: Signal<u64>, samples: std::rc::Rc<Vec<u64>>) -> Node {
    let spark = |i: usize| {
        let samples = samples.clone();
        leaf(move |console, width, height| {
            let from = ticks.get() as usize;
            let line = match i {
                0 => Sparkline::new(
                    window(&samples, from, width as usize)
                        .into_iter()
                        .map(|v| v as f64),
                )
                .min(0.0)
                .style("green"),
                1 => Sparkline::optional(
                    window(&samples, from + 100, width as usize)
                        .into_iter()
                        .enumerate()
                        // Every seventh value is missing.
                        .map(|(k, v)| (!(from + k).is_multiple_of(7)).then_some(v as f64)),
                )
                .min(0.0)
                .direction(Direction::RightToLeft)
                .absent("×", "red")
                .style("yellow"),
                _ => Sparkline::new(
                    window(&samples, from + 200, width as usize * 2)
                        .into_iter()
                        .map(|v| v as f64),
                )
                .min(0.0)
                .charset(Charset::Braille)
                .style("cyan"),
            };
            console.render_lines(&line, &sized(console, width, height), false)
        })
    };
    let bars = {
        let samples = samples.clone();
        leaf(move |console, width, height| {
            let from = ticks.get() as usize;
            let fit = (width as usize + 1) / 3;
            let values = window(&samples, from, fit);
            let chart = values.iter().fold(
                BarChart::new()
                    .orientation(Orientation::Vertical)
                    .thickness(2)
                    .bar_gap(1)
                    .show_values(false)
                    .range(0.0, 100.0)
                    .style("green"),
                |chart, v| chart.bar("", *v as f64),
            );
            console.render_lines(&chart, &sized(console, width, height), false)
        })
    };
    row([
        column([
            spark(0)
                .name("spark-ltr")
                .label("Random values, left to right")
                .panel("Left to right")
                .flex(1),
            spark(1)
                .name("spark-rtl")
                .label("Random values, right to left, some missing")
                .panel("Right to left, gaps marked ×")
                .flex(1),
            spark(2)
                .name("spark-braille")
                .label("Random values in Braille, two to a cell")
                .panel("Braille, two values a cell")
                .flex(1),
        ])
        .percent(60),
        bars.name("spark-bars")
            .label("The first sparkline's values as bars")
            .panel("The same values, as bars")
            .flex(1),
    ])
}

/// The day's temperatures, an hour apart: our own, a warm autumn day.
pub fn temperatures() -> [i32; 24] {
    let mut out = [0; 24];
    for (hour, t) in out.iter_mut().enumerate() {
        let x = (hour as f64 - 15.0) / 24.0 * std::f64::consts::TAU;
        *t = (17.0 + 9.0 * x.cos()).round() as i32;
    }
    out
}

/// A temperature's colour: warm red, mild yellow, cool cyan.
fn temperature_style(t: i32) -> &'static str {
    match t {
        22.. => "red",
        15..=21 => "yellow",
        _ => "cyan",
    }
}

/// Commits a day, our own, for the plain bar charts.
pub const COMMITS: [(&str, f64); 7] = [
    ("Mon", 12.0),
    ("Tue", 19.0),
    ("Wed", 8.0),
    ("Thu", 23.0),
    ("Fri", 15.0),
    ("Sat", 3.0),
    ("Sun", 5.0),
];

/// Bars: ratatui's `weather` app, and the `barchart` example's vertical and
/// horizontal charts.
fn bars() -> Node {
    let weather = renderable(|| {
        temperatures().iter().enumerate().fold(
            BarChart::new()
                .orientation(Orientation::Vertical)
                .thickness(3)
                .bar_gap(1)
                .range(0.0, 30.0)
                .label_style("bright_black"),
            |chart, (hour, t)| {
                let style = temperature_style(*t);
                chart.push(
                    Bar::new(format!("{hour:02}"), *t as f64)
                        .text(format!("{t}°"))
                        .style(style)
                        .value_style(style),
                )
            },
        )
    })
    .name("weather")
    .label("The day's temperatures by the hour, a bar chart")
    .panel("Weather · hourly °C")
    .fixed(13);
    let vertical = renderable(|| {
        COMMITS.iter().fold(
            BarChart::new()
                .orientation(Orientation::Vertical)
                .thickness(4)
                .bar_gap(2)
                .style("magenta")
                .value_style("black on magenta")
                .values_inside(true),
            |chart, (day, n)| chart.bar(*day, *n),
        )
    })
    .name("bars-vertical")
    .label("Commits a day, vertical bars")
    .panel("Vertical");
    let horizontal = renderable(|| {
        COMMITS.iter().fold(
            BarChart::new().style("blue").bar_width(40),
            |chart, (day, n)| chart.push(Bar::new(*day, *n).text(format!("{n} commits"))),
        )
    })
    .name("bars-horizontal")
    .label("Commits a day, horizontal bars")
    .panel("Horizontal");
    column([
        weather,
        row([vertical.percent(50), horizontal.flex(1)]).flex(1),
    ])
}

/// The companies, their colours and their revenue each quarter, in
/// millions: our own.
pub const REVENUE: [(&str, &str, [f64; 4]); 3] = [
    ("Fern", "#60a5fa", [8.2, 9.1, 7.4, 10.6]),
    ("Moss", "#34d399", [5.5, 6.8, 8.9, 9.3]),
    ("Reed", "#f472b6", [6.1, 4.9, 5.8, 7.2]),
];

/// The grouped chart: a group a quarter, a bar a company. Vertical bars
/// are three cells thick, too narrow for the companies' names: the colours
/// say which is which, as the legend above shows.
fn revenue(orientation: Orientation) -> BarChart {
    let vertical = orientation == Orientation::Vertical;
    let chart = BarChart::new()
        .orientation(orientation)
        .range(0.0, 12.0)
        .label_style("bold");
    let chart = if vertical {
        chart
            .thickness(3)
            .bar_gap(1)
            .values_inside(true)
            .value_style("bold black")
    } else {
        chart.bar_width(30)
    };
    (0..4).fold(chart, |chart, q| {
        chart.group(
            BarGroup::new(format!("Q{}", q + 1)).bars(REVENUE.iter().map(
                |(company, colour, values)| {
                    let (name, text) = if vertical {
                        ("", format!("{:.1}", values[q]))
                    } else {
                        (*company, format!("{:.1}M", values[q]))
                    };
                    Bar::new(name, values[q]).text(text).style(*colour)
                },
            )),
        )
    })
}

/// Groups: ratatui's `barchart-grouped`, vertical and horizontal.
fn groups() -> Node {
    let legend = REVENUE
        .iter()
        .map(|(company, colour, _)| format!("[{colour}]■[/] {company}"))
        .collect::<Vec<_>>()
        .join("   ");
    column([
        label(format!("Revenue by quarter, in millions:   {legend}")).fixed(1),
        row([
            renderable(|| revenue(Orientation::Vertical))
                .name("groups-vertical")
                .label("Revenue by quarter for each company, vertical bars")
                .panel("Vertical")
                .percent(60),
            renderable(|| revenue(Orientation::Horizontal))
                .name("groups-horizontal")
                .label("Revenue by quarter for each company, horizontal bars")
                .panel("Horizontal")
                .flex(1),
        ])
        .flex(1),
    ])
}

/// The moving chart's window: 20 units wide, moving a quarter a tick.
pub fn chart_window(tick: u64) -> (f64, f64) {
    let start = tick as f64 * 0.25;
    (start, start + 20.0)
}

/// Chart: a moving window, an area chart, bars, a line with its legend top
/// left, and a scatter of every marker.
fn charts(ticks: Signal<u64>) -> Node {
    let moving = renderable(move || {
        let (low, high) = chart_window(ticks.get());
        let points = |step: f64, f: fn(f64) -> f64| -> Vec<(f64, f64)> {
            let n = ((high - low) / step) as usize;
            (0..=n)
                .map(|i| {
                    let x = low + i as f64 * step;
                    (x, f(x))
                })
                .collect()
        };
        Chart::new(vec![
            Dataset::new(points(0.2, |x| (x / 2.0).sin() * 8.0))
                .name("sin")
                .marker(Marker::Dot)
                .style("cyan"),
            Dataset::new(points(0.1, |x| (x / 3.0).cos() * 5.0))
                .name("cos")
                .marker(Marker::Braille)
                .graph_type(GraphType::Line)
                .style("yellow"),
        ])
        .x_axis(
            Axis::default()
                .title("Time")
                .style("bright_black")
                .bounds(low, high)
                .labels(Labels::Markup(vec![
                    format!("[bold]{low:.1}[/]"),
                    format!("{:.1}", (low + high) / 2.0),
                    format!("[bold]{high:.1}[/]"),
                ])),
        )
        .y_axis(
            Axis::default()
                .title("Signal")
                .style("bright_black")
                .bounds(-10.0, 10.0)
                .labels(Labels::Markup(vec![
                    "[bold]-10[/]".into(),
                    "0".into(),
                    "[bold]10[/]".into(),
                ])),
        )
        .hidden_legend_fraction(0.3, 0.4)
    })
    .name("chart-moving")
    .label("A sine and a cosine in a moving window, a chart")
    .panel("A moving window");
    let area = renderable(|| {
        let rain: Vec<(f64, f64)> = (0..=24)
            .map(|h| {
                let x = h as f64;
                (x, 4.0 + 3.0 * ((x - 6.0) / 4.0).sin().max(-0.6))
            })
            .collect();
        Chart::new(vec![Dataset::new(rain)
            .name("rain")
            .graph_type(GraphType::Area)
            .marker(Marker::HalfBlock)
            .style("blue")])
        .x_axis(
            Axis::default()
                .bounds(0.0, 24.0)
                .labels(Labels::Values(vec![0.0, 6.0, 12.0, 18.0, 24.0]))
                .title("hour"),
        )
        .y_axis(
            Axis::default()
                .bounds(0.0, 8.0)
                .labels(Labels::Auto(3))
                .labels_align(rich::HorizontalAlign::Right)
                .title("mm"),
        )
        .legend(LegendPosition::None)
    })
    .name("chart-area")
    .label("Rain through the day, an area chart")
    .panel("Area");
    let bars = renderable(|| {
        let points: Vec<(f64, f64)> = [3.0, 5.0, 2.0, 7.0, 6.0, 8.0, 4.0]
            .iter()
            .enumerate()
            .map(|(i, v)| (i as f64 + 1.0, *v))
            .collect();
        Chart::new(vec![Dataset::new(points)
            .name("load")
            .graph_type(GraphType::Bar)
            .marker(Marker::Block)
            .style("green")])
        .x_axis(
            Axis::default()
                .bounds(0.0, 8.0)
                .labels(Labels::Text(vec!["0".into(), "4".into(), "8".into()]))
                .labels_align(rich::HorizontalAlign::Center),
        )
        .y_axis(
            Axis::default()
                .bounds(0.0, 8.0)
                .labels(Labels::Text(vec!["0".into(), "8".into()])),
        )
        .legend(LegendPosition::None)
    })
    .name("chart-bars")
    .label("Load a day, a chart of bars")
    .panel("Bar");
    let line = renderable(|| {
        let steps = |f: fn(f64) -> f64| -> Vec<(f64, f64)> {
            (0..=10).map(|i| (i as f64, f(i as f64))).collect()
        };
        Chart::new(vec![
            Dataset::new(steps(|x| x * 0.8))
                .name("plan")
                .graph_type(GraphType::Line)
                .style("bright_black"),
            Dataset::new(steps(|x| (x * 0.9 - (x / 2.0).sin()).max(0.0)))
                .name("done")
                .graph_type(GraphType::Line)
                .style("magenta"),
        ])
        .x_axis(
            Axis::default()
                .bounds(0.0, 10.0)
                .labels(Labels::Auto(3))
                .labels_align(rich::HorizontalAlign::Left),
        )
        .y_axis(Axis::default().bounds(0.0, 10.0).labels(Labels::Auto(3)))
        .legend(LegendPosition::TopLeft)
        .hidden_legend_fraction(0.6, 0.6)
    })
    .name("chart-line")
    .label("Planned and done, a line chart")
    .panel("Line, legend top left");
    let scatter = renderable(|| {
        let markers = [
            ("dot", Marker::Dot, "cyan"),
            ("x", Marker::Ascii('x'), "yellow"),
            ("block", Marker::Block, "green"),
            ("braille", Marker::Braille, "magenta"),
        ];
        let mut random = XorShift::new(0x5ca7);
        Chart::new(
            markers
                .iter()
                .enumerate()
                .map(|(k, (name, marker, style))| {
                    let points: Vec<(f64, f64)> = (0..8)
                        .map(|_| {
                            let x = random.below(100) as f64 / 10.0;
                            let y = k as f64 * 2.5 + random.below(20) as f64 / 10.0;
                            (x, y)
                        })
                        .collect();
                    Dataset::new(points)
                        .name(*name)
                        .marker(*marker)
                        .style(*style)
                }),
        )
        .x_axis(Axis::default().bounds(0.0, 10.0).labels(Labels::Auto(2)))
        .y_axis(Axis::default().bounds(0.0, 10.0).labels(Labels::Auto(2)))
        .legend(LegendPosition::BottomRight)
        .hidden_legend_fraction(0.6, 0.8)
    })
    .name("chart-scatter")
    .label("Scattered points in four markers, a scatter chart")
    .panel("Scatter, four markers");
    column([
        row([moving.percent(60), area.flex(1)]).fixed(13),
        row([bars.ratio(1, 3), line.ratio(1, 3), scatter.ratio(1, 3)]).flex(1),
    ])
}

/// The canvas's markers, cycled with `m`.
pub const MARKERS: [(&str, Marker); 9] = [
    ("Braille", Marker::Braille),
    ("HalfBlock", Marker::HalfBlock),
    ("Block", Marker::Block),
    ("Bar", Marker::Bar),
    ("Quadrant", Marker::Quadrant),
    ("Sextant", Marker::Sextant),
    ("Octant", Marker::Octant),
    ("Dot", Marker::Dot),
    ("Ascii", Marker::Ascii('*')),
];

/// Canvas: the world map, and shapes, in the marker `m` picks.
fn canvases(marker: Signal<usize>) -> Node {
    let map = renderable(move || {
        let (_, marker) = MARKERS[marker.get() % MARKERS.len()];
        Canvas::new()
            .x_bounds(-180.0, 180.0)
            .y_bounds(-90.0, 90.0)
            .marker(marker)
            .paint(|p| {
                p.map(MapResolution::High, "green");
            })
            .paint(|p| {
                // Reykjavík, and a ring round it.
                p.circle(-21.9, 64.1, 8.0, "yellow");
                p.print(-21.9, 64.1, "You are here", "bold yellow");
            })
    })
    .name("canvas-map")
    .label("A map of the world")
    .panel("World");
    let shapes = renderable(move || {
        let (_, marker) = MARKERS[marker.get() % MARKERS.len()];
        Canvas::new()
            .x_bounds(0.0, 100.0)
            .y_bounds(0.0, 100.0)
            .marker(marker)
            .paint(|p| {
                p.rect(5.0, 5.0, 40.0, 35.0, "red");
                p.circle(70.0, 30.0, 22.0, "cyan");
                p.line(5.0, 95.0, 95.0, 55.0, "yellow");
                p.line(5.0, 55.0, 95.0, 95.0, "yellow");
                let dots: Vec<(f64, f64)> = (0..20)
                    .map(|i| (10.0 + i as f64 * 4.0, 50.0 + (i as f64 / 2.0).sin() * 4.0))
                    .collect();
                p.points(&dots, "magenta");
                p.print(10.0, 30.0, "a rectangle", "red");
                p.print(55.0, 30.0, "a circle", "cyan");
            })
    })
    .name("canvas-shapes")
    .label("Shapes: a rectangle, a circle, lines and points")
    .panel("Shapes");
    let title = text(move || {
        let (name, _) = MARKERS[marker.get() % MARKERS.len()];
        format!("Marker: [bold]{name}[/]   [dim]m changes it[/]")
    })
    .name("marker")
    .fixed(1);
    column([title, row([map.percent(60), shapes.flex(1)]).flex(1)])
}

/// The gallery, as `options` says.
pub fn charts_app(options: Options) -> App {
    App::new(move || {
        let tab = signal(options.tab.min(TABS.len() - 1));
        let ticks = signal(0u64);
        let running = signal(!options.frozen);
        let marker = signal(0usize);
        let samples = {
            let mut random = XorShift::new(options.seed);
            std::rc::Rc::new((0..SAMPLES).map(|_| random.below(100)).collect::<Vec<u64>>())
        };
        if options.frozen {
            ticks.set(FROZEN_TICKS);
        } else {
            every(options.tick_rate, move |_| {
                if running.get_untracked() {
                    ticks.update(|t| *t += 1);
                }
            });
        }
        let strip = tabs(|| TABS.iter().map(|t| t.to_string()).collect(), tab)
            .theme_styles([
                ("tabs.selected", "bold black on yellow"),
                ("tabs.selected.blur", "bold black on yellow"),
            ])
            .no_focus()
            .name("tabs")
            .label("Chart families")
            .fixed(1);
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => gauges(ticks),
                1 => sparklines(ticks, samples.clone()),
                2 => bars(),
                3 => groups(),
                4 => charts(ticks),
                _ => canvases(marker),
            },
        )
        .name("page");
        let footer = text(move || {
            let clock = if running.get() { "stop" } else { "start" };
            format!(
                "[dim]←/→ or 1-6 tabs · m marker · Space {clock} the clock · ? keys · Ctrl+P commands · q quit[/]"
            )
        })
        .no_wrap()
        .name("footer")
        .fixed(1);
        let mut root = column([strip, body.flex(1), footer])
            .bind("left h", "the tab before", move |_| {
                tab.update(|t| *t = (*t + TABS.len() - 1) % TABS.len())
            })
            .bind("right l", "the next tab", move |_| {
                tab.update(|t| *t = (*t + 1) % TABS.len())
            })
            .bind("space", "stop or start the clock", move |_| {
                running.update(|r| *r = !*r)
            })
            .bind("q", "quit", |cx| cx.quit());
        for (i, name) in TABS.iter().enumerate() {
            root = root.bind(&(i + 1).to_string(), &format!("the {name} tab"), move |_| {
                tab.set(i)
            });
        }
        // The canvas's marker is bound here, so it works from any tab.
        root.bind("m", "the canvas's next marker", move |_| {
            marker.update(|m| *m = (*m + 1) % MARKERS.len())
        })
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    charts_app(options).run()
}

//! A port of ratatui's `demo` example
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps/demo>),
//! rebuilt on intuiTUIve. It reuses none of ratatui's code: it rebuilds
//! the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The
//! tasks, the events, the servers and the text are our own.
//!
//!     cargo run -p rs-rich-intuituive --example demo
//!     cargo run -p rs-rich-intuituive --example demo -- --tick-rate 100 --unicode false
//!     cargo run -p rs-rich-intuituive --example demo -- --frozen
//!
//! ←/→ (or h/l) change tabs · ↑/↓ (or k/j) move through the tasks · t
//! shows or hides the chart · ? lists the keys · Ctrl+P finds a command ·
//! q quits. A click picks a tab, a task or a server, and the wheel moves
//! through the tasks.
//!
//! `--tick-rate MS` sets how often the data moves (250 ms by default), and
//! `--unicode false` draws with the three-level block sets and dots
//! instead of eighths and Braille. `--frozen` moves the data on a little
//! and stops the clock, for screenshots that do not move.
//!
//! What intuiTUIve adds to the original:
//!
//! - **Only what moves redraws.** A tick redraws the gauges, the
//!   sparkline, the log, the bars and the chart, each reading a signal of
//!   its own; the tasks, the footer and the tab strip stay as they were
//!   (`tests/demo.rs` checks it, and with `INTUITUIVE_INSPECT=1` the
//!   inspector shows it).
//! - **The mouse** picks tabs, tasks and servers.
//! - **The keys have descriptions**, so `?` lists them and Ctrl+P finds
//!   them.
//! - **The charts have labels**, so linear mode (`--linear`) reads them as
//!   words rather than Braille.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::{Console, ConsoleOptions};
use intuituive::screen::Rect;
use intuituive::widget::{Canvas as WidgetCanvas, DrawCx, MeasureCx};
use intuituive::widgets::{table_optional, tabs, Column, TableOptions};
use rich_ext::chart::{
    Axis, BarChart, Canvas, Chart, Dataset, FillGauge, Labels, Levels, LineGauge, LineSet,
    MapResolution, Marker, Orientation, Sparkline,
};

/// The panel round the tab strip is titled with the app's name.
pub const TITLE: &str = "intuiTUIve demo";

/// The tabs, in order.
pub const TABS: [&str; 3] = ["Graphs", "World", "Colours"];

/// How the demo runs: its command line.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// How often the data moves.
    pub tick_rate: Duration,
    /// Eighths of a cell and Braille (`true`), or the three-level sets and
    /// dots.
    pub unicode: bool,
    /// Move the data on [`FROZEN_TICKS`] ticks, then stop the clock.
    pub frozen: bool,
    /// The seed of the random values.
    pub seed: u64,
    /// Run in linear mode: the screen as lines of text, for a screen
    /// reader.
    pub linear: bool,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            tick_rate: Duration::from_millis(250),
            unicode: true,
            frozen: false,
            seed: 0x5eed_de30,
            linear: false,
        }
    }
}

/// How far `--frozen` moves the data before it stops the clock.
pub const FROZEN_TICKS: u64 = 120;

impl Options {
    /// `--tick-rate MS`, `--unicode BOOL`, `--frozen` and `--linear`, as
    /// the command line gives them.
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
                "--unicode" | "-u" => {
                    options.unicode = match value("true or false")?.as_str() {
                        "true" | "yes" | "1" => true,
                        "false" | "no" | "0" => false,
                        other => return Err(format!("--unicode takes true or false, not {other}")),
                    }
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

/// The tasks in the first list.
pub const TASKS: [&str; 24] = [
    "Rotate the API keys",
    "Renew the certificates",
    "Patch the build hosts",
    "Archive last year's logs",
    "Resize the cache cluster",
    "Review the alert rules",
    "Test the backups",
    "Move DNS to the new zone",
    "Retire the old mirror",
    "Tune the queue workers",
    "Update the runbooks",
    "Audit the firewall",
    "Upgrade the database",
    "Clean up stale branches",
    "Pin the base images",
    "Check the disk quotas",
    "Replace the load balancer",
    "Split the monolith's jobs",
    "Profile the search API",
    "Add a canary stage",
    "Shorten the deploy",
    "Write the incident review",
    "Tag the cost centres",
    "Plan the next drill",
];

/// How serious a log line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
    Critical,
}

impl Level {
    pub fn name(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warning => "WARNING",
            Level::Error => "ERROR",
            Level::Critical => "CRITICAL",
        }
    }

    /// Its colour, as ratatui's demo has them.
    pub fn style(self) -> &'static str {
        match self {
            Level::Info => "blue",
            Level::Warning => "yellow",
            Level::Error => "magenta",
            Level::Critical => "red",
        }
    }
}

/// The events in the log, which turns a line every tick.
pub const LOGS: [(&str, Level); 26] = [
    ("Deploy 412 started", Level::Info),
    ("Cache warmed in 3.2 s", Level::Info),
    ("Primary database unreachable", Level::Critical),
    ("Payment webhook failed", Level::Error),
    ("Worker pool scaled to 12", Level::Info),
    ("Nightly backup done", Level::Info),
    ("Disk on build-3 at 91%", Level::Warning),
    ("Certificate renewed", Level::Info),
    ("Search index rebuilt", Level::Info),
    ("Queue drained", Level::Info),
    ("Replica lag over 30 s", Level::Critical),
    ("Feature flag flipped", Level::Info),
    ("New region online", Level::Info),
    ("Config reloaded", Level::Info),
    ("User import finished", Level::Info),
    ("Rate limit raised", Level::Info),
    ("Image upload timed out", Level::Error),
    ("Mail relay refused", Level::Error),
    ("Canary healthy", Level::Info),
    ("Deploy 412 finished", Level::Info),
    ("Memory at 85% on api-2", Level::Warning),
    ("Health check passed", Level::Info),
    ("Logs rotated", Level::Info),
    ("Slow query on orders", Level::Warning),
    ("Cron jobs rescheduled", Level::Info),
    ("Metrics flushed", Level::Info),
];

/// A server on the World tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Server {
    pub name: &'static str,
    pub location: &'static str,
    /// Latitude and longitude, in degrees.
    pub lat: f64,
    pub lon: f64,
    pub up: bool,
}

impl Server {
    pub fn status(&self) -> &'static str {
        if self.up {
            "Up"
        } else {
            "Failure"
        }
    }
}

/// The servers, one of them down.
pub const SERVERS: [Server; 4] = [
    Server {
        name: "NorthAmerica-1",
        location: "Toronto",
        lat: 43.65,
        lon: -79.38,
        up: true,
    },
    Server {
        name: "Europe-1",
        location: "Lisbon",
        lat: 38.72,
        lon: -9.14,
        up: false,
    },
    Server {
        name: "SouthAmerica-1",
        location: "Buenos Aires",
        lat: -34.60,
        lon: -58.38,
        up: true,
    },
    Server {
        name: "Asia-1",
        location: "Tokyo",
        lat: 35.68,
        lon: 139.69,
        up: true,
    },
];

/// The named colours of the Colours tab, and the terminal's own.
pub const COLOURS: [&str; 17] = [
    "default",
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// The footer's paragraph: colours, every modifier, and a character wider
/// than ASCII, wrapped to the panel.
pub const FOOTER: &str = "A paragraph of several lines, wrapped to fit its panel, and styled \
however you like.\n\n\
For example: [red]under[/] [green]the[/] [blue]rainbow[/].\n\
Or with every modifier: [bold]bold[/], [dim]dim[/], [italic]italic[/], \
[underline]underlined[/], [reverse]reversed[/], [strike]struck through[/] and \
[blink]blinking[/].\n\
Text beyond ASCII shows as well: a coffee here costs 10€.";

/// How many values the sparkline keeps; it shows as many as fit, oldest
/// first, as ratatui's does.
pub const SPARK_LEN: usize = 300;

/// What a tick moves, each in a signal of its own, so that a tick redraws
/// only the nodes that show it.
#[derive(Clone, Copy)]
pub struct Feeds {
    /// Ticks so far: the chart's window.
    pub ticks: Signal<u64>,
    /// The gauges, 0 to 1, a thousandth a tick.
    pub progress: Signal<f64>,
    /// The sparkline's values, 0 to 99, oldest first.
    pub spark: Signal<Vec<u64>>,
    /// How far the log and the bars have turned.
    pub turned: Signal<usize>,
}

/// The `i`th of `len` items after `turned` turns, the last moving to the
/// front each turn.
fn turned_index(i: usize, len: usize, turned: usize) -> usize {
    (i + len - turned % len) % len
}

/// The chart's first sine: 100 points a fifth apart, five new a tick.
fn first_sine(tick: u64) -> Vec<(f64, f64)> {
    (0..100)
        .map(|i| {
            let x = (tick * 5 + i) as f64 * 0.2;
            (x, (x / 3.0).sin() * 18.0)
        })
        .collect()
}

/// The chart's second sine: 200 points a tenth apart, ten new a tick.
fn second_sine(tick: u64) -> Vec<(f64, f64)> {
    (0..200)
        .map(|i| {
            let x = (tick * 10 + i) as f64 * 0.1;
            (x, (x / 2.0).sin() * 10.0)
        })
        .collect()
}

/// How thick the bars are, and how far apart.
const BAR_THICKNESS: usize = 3;
const BAR_GAP: usize = 2;

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

/// Two children side by side, or the first alone while `both` is false:
/// `t` hides the chart, and the lists and bars take the width.
struct Halves {
    children: Vec<Node>,
    both: Signal<bool>,
}

impl Widget for Halves {
    fn name(&self) -> &'static str {
        "halves"
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        if self.both.get() {
            let half = rect.width / 2;
            vec![
                Rect::new(rect.x, rect.y, half, rect.height),
                Rect::new(rect.x + half, rect.y, rect.width - half, rect.height),
            ]
        } else {
            vec![rect, Rect::default()]
        }
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut WidgetCanvas) {}
}

/// A tick: the gauges fill a little, the sparkline takes a new value, the
/// chart's window moves on, and the log and the bars turn.
fn tick(feeds: Feeds, rng: &Cell<XorShift>) {
    feeds.progress.update(|p| {
        *p += 0.001;
        if *p > 1.0 {
            *p = 0.0;
        }
    });
    let mut random = rng.get();
    let value = random.below(100);
    rng.set(random);
    feeds.spark.update(|values| {
        values.remove(0);
        values.push(value);
    });
    feeds.turned.update(|t| *t += 1);
    feeds.ticks.update(|t| *t += 1);
}

/// The Graphs tab: gauges, lists, bars, a chart and a paragraph.
fn graphs(
    feeds: Feeds,
    bars: Rc<Vec<(String, f64)>>,
    options: Options,
    chart_on: Signal<bool>,
    task: Signal<Option<usize>>,
) -> Node {
    let Feeds {
        ticks,
        progress,
        spark,
        turned,
    } = feeds;
    let unicode = options.unicode;

    let gauge = renderable(move || {
        let ratio = progress.get();
        FillGauge::new(ratio)
            .label(format!("{:.2}%", ratio * 100.0))
            .style("magenta on black italic bold")
            .unicode(unicode)
    })
    .name("gauge");
    // As many values as fit, oldest first, scaled from zero: ratatui's.
    let sparkline = leaf(move |console, width, height| {
        let values: Vec<f64> =
            spark.with(|v| v.iter().take(width as usize).map(|v| *v as f64).collect());
        let levels = if unicode { Levels::Nine } else { Levels::Three };
        let line = Sparkline::new(values)
            .min(0.0)
            .style("green")
            .levels(levels);
        console.render_lines(&line, &sized(console, width, height), false)
    })
    .name("sparkline")
    .label("Random values, a sparkline");
    let line_gauge = renderable(move || {
        LineGauge::new(progress.get())
            .filled_style("magenta")
            .line(if unicode {
                LineSet::Thick
            } else {
                LineSet::Thin
            })
    })
    .name("line-gauge");
    let gauges = column([
        gauge.panel("Gauge:").border(Border::None).fixed(2),
        sparkline.panel("Sparkline:").border(Border::None).fixed(4),
        line_gauge.panel("LineGauge:").border(Border::None).fixed(2),
    ])
    .panel("Graphs")
    .fixed(10);

    let tasks = list_optional(|| TASKS.iter().map(|t| t.to_string()).collect(), task)
        .symbol("> ")
        .theme_styles([("selected", "bold")])
        .no_focus()
        .name("tasks")
        .panel("List")
        .label("Tasks");
    let logs = list_optional(
        move || {
            let turned = turned.get();
            (0..LOGS.len())
                .map(|i| {
                    let (event, level) = LOGS[turned_index(i, LOGS.len(), turned)];
                    format!("[{}]{:<9}[/]{event}", level.style(), level.name())
                })
                .collect()
        },
        signal(None),
    )
    .no_focus()
    .name("logs")
    .panel("List")
    .label("Logs");
    // The bars that fit, three cells thick and two apart, as ratatui
    // draws them: a `BarChart` given more bars than fit would make them
    // thinner and close the gaps instead.
    let bar_chart = leaf(move |console, width, height| {
        let turned = turned.get();
        let fit = (width as usize + BAR_GAP) / (BAR_THICKNESS + BAR_GAP);
        let chart = BarChart::new()
            .orientation(Orientation::Vertical)
            .thickness(BAR_THICKNESS)
            .bar_gap(BAR_GAP)
            .style("green")
            .label_style("yellow")
            .value_style("black on green italic")
            .values_inside(true);
        let chart = (0..bars.len().min(fit)).fold(chart, |chart, i| {
            let (label, value) = &bars[turned_index(i, bars.len(), turned)];
            chart.bar(label.clone(), *value)
        });
        console.render_lines(&chart, &sized(console, width, height), false)
    })
    .name("bars")
    .label("Events by hour, a bar chart")
    .panel("Bar chart");
    let chart = renderable(move || {
        let tick = ticks.get();
        let (low, high) = (tick as f64, tick as f64 + 20.0);
        let middle = (low + high) / 2.0;
        Chart::new(vec![
            Dataset::new(first_sine(tick))
                .name("data2")
                .marker(Marker::Dot)
                .style("cyan"),
            Dataset::new(second_sine(tick))
                .name("data3")
                .marker(if unicode {
                    Marker::Braille
                } else {
                    Marker::Dot
                })
                .style("yellow"),
        ])
        .x_axis(
            Axis::default()
                .title("X Axis")
                .style("white")
                .bounds(low, high)
                .labels(Labels::Markup(vec![
                    format!("[bold]{low}[/]"),
                    format!("{middle}"),
                    format!("[bold]{high}[/]"),
                ])),
        )
        .y_axis(
            Axis::default()
                .title("Y Axis")
                .style("white")
                .bounds(-20.0, 20.0)
                .labels(Labels::Markup(vec![
                    "[bold]-20[/]".into(),
                    "0".into(),
                    "[bold]20[/]".into(),
                ])),
        )
    })
    .name("chart")
    .label("Two sine waves, a chart")
    .panel("[bold cyan]Chart[/]");

    let left = column([
        row([tasks.percent(50), logs.percent(50)]).percent(50),
        bar_chart.percent(50),
    ]);
    let middle = widget(Halves {
        children: vec![left, chart],
        both: chart_on,
    });
    let footer = label(FOOTER)
        .name("footer")
        .panel("[bold magenta]Footer[/]")
        .fixed(7);
    column([gauges, middle.flex(1).min_size(8), footer])
}

/// The World tab: the servers, and where they are.
fn world(unicode: bool, server: Signal<Option<usize>>) -> Node {
    let columns = vec![
        Column::new("Server", Size::Fixed(15)),
        Column::new("Location", Size::Fixed(15)),
        Column::new("Status", Size::Fixed(10)),
    ];
    let rows = || {
        SERVERS
            .iter()
            .map(|s| {
                vec![
                    s.name.to_string(),
                    s.location.to_string(),
                    s.status().to_string(),
                ]
            })
            .collect()
    };
    let options = TableOptions::default().header_gap(1).row_style(|i| {
        let up = SERVERS.get(i).is_some_and(|s| s.up);
        Some(if up { "green" } else { "red blink strike" }.to_string())
    });
    let table = table_optional(columns, rows, server, options)
        .theme_styles([("table.header", "yellow")])
        .name("servers")
        .panel("Servers");
    let map = renderable(move || {
        Canvas::new()
            .x_bounds(-180.0, 180.0)
            .y_bounds(-90.0, 90.0)
            .marker(if unicode {
                Marker::Braille
            } else {
                Marker::Dot
            })
            .paint(|p| {
                p.map(MapResolution::High, "white");
            })
            .paint(|p| {
                p.rect(10.0, -10.0, 20.0, 20.0, "yellow");
                let hub = SERVERS[2];
                p.circle(hub.lon, hub.lat, 10.0, "green");
                for (i, a) in SERVERS.iter().enumerate() {
                    for b in &SERVERS[i + 1..] {
                        p.line(a.lon, a.lat, b.lon, b.lat, "yellow");
                    }
                }
                for s in &SERVERS {
                    p.print(s.lon, s.lat, "X", if s.up { "green" } else { "red" });
                }
            })
    })
    .name("map")
    .label("The servers on a map of the world")
    .panel("World");
    row([table.percent(30), map.percent(70)])
}

/// The Colours tab: each named colour as a foreground and a background,
/// in three thirds of the left half.
fn colours() -> Node {
    let column_of = |cell: fn(&str) -> String| {
        label(
            COLOURS
                .iter()
                .map(|c| cell(c))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .no_wrap()
        .ratio(1, 3)
    };
    let table = row([
        column_of(|c| format!("{c}:")),
        column_of(|c| format!("[{c}]Foreground[/]")),
        column_of(|c| format!("[on {c}]Background[/]")),
    ])
    .name("colours")
    .panel("Colours");
    row([table.ratio(1, 2), label("").ratio(1, 2)])
}

/// The demo, as `options` says.
pub fn demo_app(options: Options) -> App {
    App::new(move || {
        let tab = signal(0usize);
        let chart_on = signal(true);
        let task = signal(None::<usize>);
        let server = signal(None::<usize>);

        // Random values: the sparkline's, and the bars'.
        let rng = Rc::new(Cell::new(XorShift::new(options.seed)));
        let mut random = rng.get();
        let spark: Vec<u64> = (0..SPARK_LEN).map(|_| random.below(100)).collect();
        let bars: Rc<Vec<(String, f64)>> = Rc::new(
            (1..=24)
                .map(|n| (format!("B{n}"), random.below(16) as f64))
                .collect(),
        );
        rng.set(random);
        let feeds = Feeds {
            ticks: signal(0),
            progress: signal(0.0),
            spark: signal(spark),
            turned: signal(0),
        };
        if options.frozen {
            for _ in 0..FROZEN_TICKS {
                tick(feeds, &rng);
            }
        } else {
            let rng = rng.clone();
            every(options.tick_rate, move |_| tick(feeds, &rng));
        }

        let strip = tabs(
            || TABS.iter().map(|t| format!("[green]{t}[/]")).collect(),
            tab,
        )
        .theme_styles([
            ("tabs.selected", "yellow"),
            ("tabs.selected.blur", "yellow"),
        ])
        .no_focus()
        .name("tabs")
        .panel(TITLE)
        .fixed(3);
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => graphs(feeds, bars.clone(), options, chart_on, task),
                1 => world(options.unicode, server),
                _ => colours(),
            },
        );
        let step = |n: usize, by: isize| {
            move |at: Option<usize>| -> Option<usize> {
                Some(match at {
                    None => 0,
                    Some(at) => (at as isize + by).rem_euclid(n as isize) as usize,
                })
            }
        };
        let (up, down) = (step(TASKS.len(), -1), step(TASKS.len(), 1));
        column([strip, body.flex(1)])
            .bind("left h", "the tab before", move |_| {
                tab.update(|t| *t = (*t + TABS.len() - 1) % TABS.len())
            })
            .bind("right l", "the next tab", move |_| {
                tab.update(|t| *t = (*t + 1) % TABS.len())
            })
            .bind("up k", "the task before", move |_| {
                task.update(|t| *t = up(*t))
            })
            .bind("down j", "the next task", move |_| {
                task.update(|t| *t = down(*t))
            })
            .bind("t", "show or hide the chart", move |_| {
                chart_on.update(|on| *on = !*on)
            })
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    demo_app(options).run()
}

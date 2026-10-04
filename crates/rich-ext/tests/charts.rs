//! Charts (0.0.15 workstream 1): sparklines, bars, histograms, and line and
//! scatter plots, in every glyph set, with colour on and off.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Cell, ColorSystem, Console, Live, Panel, Renderable, Table};
use rich_ext::chart::{
    Bar, BarChart, Charset, Histogram, LineChart, Series, SeriesKind, Sparkline, ValueFormat,
};

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

fn ascii(width: usize) -> Console {
    Console::builder()
        .width(width)
        .color_system(None)
        .ascii_only(true)
        .build()
}

fn color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .theme(rich_ext::extended_theme())
        .build()
}

fn no_color(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .no_color(true)
        .theme(rich_ext::extended_theme())
        .build()
}

fn render(console: &Console, r: &dyn Renderable) -> String {
    console.segments_to_string(&r.rich_render(console, &console.options()))
}

/// `s` without CSI escape sequences.
fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

const VALUES: [f64; 12] = [
    3.0, 5.0, 4.0, 8.0, 12.0, 9.0, 7.0, 11.0, 15.0, 13.0, 10.0, 6.0,
];

fn line_chart() -> LineChart {
    LineChart::new()
        .series(Series::from_values(
            "rx",
            [1.0, 4.0, 2.0, 6.0, 5.0, 8.0, 7.0],
        ))
        .series(Series::from_values(
            "tx",
            [6.0, 5.0, 5.0, 3.0, 2.0, 2.0, 1.0],
        ))
        .height(4)
}

// ------------------------------------------------------------- sparklines

#[test]
fn sparkline_snapshots() {
    let s = Sparkline::new(VALUES);
    assert_eq!(render(&plain(40), &s), "▁▂▂▄▆▅▃▆█▇▅▃\n");
    assert_eq!(
        render(&plain(40), &s.clone().charset(Charset::Braille)),
        "⣠⣠⣶⣴⣿⣦\n"
    );
    assert_eq!(
        render(&plain(40), &s.clone().charset(Charset::Ascii)),
        "_..:+=-+#*=-\n"
    );
    // An ASCII console gets ASCII whatever was asked for.
    assert_eq!(
        render(&ascii(40), &s.clone().charset(Charset::Braille)),
        "_..:+=-+#*=-\n"
    );
    assert_eq!(
        render(&plain(40), &s.clone().min_max(true).threshold(12.0)),
        "▁▂▂▄▆▅▃▆█▇▅▃ min 3 max 15 2 > 12\n"
    );
}

#[test]
fn sparkline_colour_on_and_off() {
    let s = Sparkline::new([1.0, 5.0, 3.0]).min_max(true);
    assert_eq!(
        render(&color(40), &s),
        "\x1b[34m▁\x1b[0m\x1b[1;32m█\x1b[0m\x1b[36m▅\x1b[0m \x1b[34mmin 1\x1b[0m \x1b[1;32mmax 5\x1b[0m\n"
    );
    // Without colour the words carry the extremes.
    assert_eq!(render(&no_color(40), &s), "▁█▅ min 1 max 5\n");
    let t = Sparkline::new([1.0, 5.0, 3.0]).threshold(4.0);
    assert_eq!(
        render(&color(40), &t),
        "\x1b[36m▁\x1b[0m\x1b[1;31m█\x1b[0m\x1b[36m▅\x1b[0m \x1b[1;31m1 > 4\x1b[0m\n"
    );
    assert_eq!(render(&no_color(40), &t), "▁█▅ 1 > 4\n");
}

#[test]
fn sparkline_measures_one_cell_per_value() {
    let console = plain(80);
    let m = |s: &Sparkline| {
        let m = s.measure(&console, &console.options());
        (m.minimum, m.maximum)
    };
    assert_eq!(m(&Sparkline::new(VALUES)), (4, 12));
    assert_eq!(m(&Sparkline::new(VALUES).charset(Charset::Braille)), (4, 6));
    assert_eq!(m(&Sparkline::new(VALUES).min_max(true)), (4, 12 + 13));
    assert_eq!(m(&Sparkline::new([1.0, 2.0])), (2, 2));
}

#[test]
fn sparkline_resamples_by_bucket_mean() {
    // Twelve values in six cells: each cell is the mean of two.
    let s = Sparkline::new(VALUES);
    let out = render(&plain(6), &s);
    let means: Vec<f64> = VALUES.chunks(2).map(|c| (c[0] + c[1]) / 2.0).collect();
    let expected = render(&plain(40), &Sparkline::new(means.clone()).range(3.0, 15.0));
    assert_eq!(out, expected);
    assert_eq!(cell_len(out.trim_end()), 6);
    // The summary goes before any data does.
    let with = Sparkline::new(VALUES).min_max(true);
    assert_eq!(render(&plain(12), &with), "▁▂▂▄▆▅▃▆█▇▅▃\n");
    assert_eq!(render(&plain(25), &with), "▁▂▂▄▆▅▃▆█▇▅▃ min 3 max 15\n");
}

#[test]
fn sparkline_degenerate_data() {
    // All equal sits mid-height; all zero at the bottom of 0..1.
    assert_eq!(render(&plain(10), &Sparkline::new([5.0; 4])), "▅▅▅▅\n");
    assert_eq!(render(&plain(10), &Sparkline::new([0.0; 4])), "▁▁▁▁\n");
    // NaN and infinities are gaps.
    assert_eq!(
        render(
            &plain(10),
            &Sparkline::new([1.0, f64::NAN, 3.0, f64::INFINITY, 2.0])
        ),
        "▁ █ ▅\n"
    );
    assert_eq!(render(&plain(10), &Sparkline::new([])), "\n");
    // Negative values.
    assert_eq!(
        render(&plain(10), &Sparkline::new([-4.0, 0.0, 4.0])),
        "▁▅█\n"
    );
    // An explicit range clamps.
    assert_eq!(
        render(&plain(10), &Sparkline::new([5.0, 50.0]).range(0.0, 10.0)),
        "▅█\n"
    );
}

// ------------------------------------------------------------------- bars

#[test]
fn bar_chart_snapshots() {
    let chart = BarChart::new()
        .bar("disk", 75.0)
        .bar("cpu", 33.3)
        .bar("net", 0.5)
        .max(100.0)
        .bar_width(10);
    assert_eq!(
        render(&plain(40), &chart),
        "disk ███████▌     75\ncpu  ███▍       33.3\nnet  ▏           0.5\n"
    );
    assert_eq!(
        render(&plain(40), &chart.clone().charset(Charset::Braille)),
        "disk ⣿⣿⣿⣿⣿⣿⣿⡇     75\ncpu  ⣿⣿⣿⡇       33.3\nnet  ⡇           0.5\n"
    );
    assert_eq!(
        render(&plain(40), &chart.clone().charset(Charset::Ascii)),
        "disk #######=     75\ncpu  ###=       33.3\nnet  =           0.5\n"
    );
    assert_eq!(
        render(&ascii(40), &chart),
        render(&plain(40), &chart.clone().charset(Charset::Ascii))
    );
}

#[test]
fn bar_chart_colour_on_and_off() {
    let chart = BarChart::new()
        .bar("up", 4.0)
        .push(Bar::new("down", -2.0))
        .push(Bar::new("hot", 3.0).style("chart.over"))
        .bar_width(6);
    assert_eq!(
        render(&color(40), &chart),
        "up     \x1b[36m████\x1b[0m  4\n\
         down \x1b[35m██\x1b[0m     -2\n\
         hot    \x1b[1;31m███\x1b[0m   3\n"
    );
    // Without colour, position and sign carry the meaning.
    assert_eq!(
        render(&no_color(40), &chart),
        "up     ████  4\ndown ██     -2\nhot    ███   3\n"
    );
}

#[test]
fn bar_chart_shrinks_values_then_labels() {
    let chart = BarChart::new()
        .bar("requests", 100.0)
        .bar("errors", 25.0)
        .bar_width(20);
    assert_eq!(
        render(&plain(40), &chart),
        "requests ████████████████████ 100\nerrors   █████                 25\n"
    );
    // Bar shrinks first.
    assert_eq!(
        render(&plain(20), &chart),
        "requests ███████ 100\nerrors   █▊       25\n"
    );
    // Then values go.
    assert_eq!(render(&plain(13), &chart), "requests ████\nerrors   █   \n");
    // Then labels are cut.
    assert_eq!(render(&plain(9), &chart), "req… ████\nerr… █   \n");
    assert_eq!(render(&plain(3), &chart), "███\n▊  \n");
    assert_eq!(render(&plain(1), &chart), "█\n▎\n");
}

#[test]
fn bar_chart_measures_to_content() {
    let console = plain(80);
    let chart = BarChart::new().bar("a", 1.0).bar("bb", 2.0).bar_width(10);
    let m = chart.measure(&console, &console.options());
    assert_eq!((m.minimum, m.maximum), (4, 2 + 1 + 10 + 1 + 1));
    let empty = BarChart::new();
    assert_eq!(render(&plain(40), &empty), "no data\n");
    assert_eq!(render(&plain(4), &empty), "no …\n");
    assert_eq!(render(&ascii(4), &empty), "no .\n");
}

// -------------------------------------------------------------- histogram

#[test]
fn histogram_bins_values() {
    let values = [1.0, 2.0, 2.5, 3.0, 7.0, 8.0, 9.5, f64::NAN, 4.0];
    let h = Histogram::new(values).bins(4).bar_width(8);
    // 8.5 / 4 bins rounds up to a bin width of 2.5.
    assert_eq!(h.edges(), vec![0.0, 2.5, 5.0, 7.5, 10.0]);
    assert_eq!(h.counts().iter().sum::<usize>(), 8);
    assert_eq!(
        render(&plain(40), &h),
        "[0, 2.5)  █████▍   2\n[2.5, 5)  ████████ 3\n[5, 7.5)  ██▋      1\n[7.5, 10] █████▍   2\n"
    );
    assert_eq!(
        render(&plain(40), &h.clone().charset(Charset::Ascii)),
        "[0, 2.5)  #####=   2\n[2.5, 5)  ######## 3\n[5, 7.5)  ##=      1\n[7.5, 10] #####=   2\n"
    );
    assert_eq!(
        render(&plain(40), &h.clone().charset(Charset::Braille)),
        "[0, 2.5)  ⣿⣿⣿⣿⣿⡇   2\n[2.5, 5)  ⣿⣿⣿⣿⣿⣿⣿⣿ 3\n[5, 7.5)  ⣿⣿⡇      1\n[7.5, 10] ⣿⣿⣿⣿⣿⡇   2\n"
    );
    let ranged = Histogram::new(values).bins(2).range(0.0, 10.0).bar_width(4);
    assert_eq!(ranged.counts(), vec![5, 3]);
    assert_eq!(
        render(&no_color(40), &ranged),
        "[0, 5)  ████ 5\n[5, 10] ██▍  3\n"
    );
    assert!(render(&color(40), &ranged).contains("\x1b[36m████\x1b[0m"));
    assert_eq!(render(&plain(40), &Histogram::new([])), "no data\n");
    assert_eq!(
        Histogram::new([5.0; 3])
            .bins(2)
            .counts()
            .iter()
            .sum::<usize>(),
        3
    );
}

// ---------------------------------------------------------- line, scatter

#[test]
fn line_chart_braille_in_colour() {
    let out = render(&color(30), &line_chart());
    assert_eq!(
        strip_ansi(&out),
        concat!(
            "10 ┤                     ⢀⡀   \n",
            "   │⠤⢄⣀⡀        ⣀⠤⣀⣀ ⢀⡠⠔⠊⠁⠈⠉⠑⠒\n",
            " 5 ┤  ⢀⡨⠝⠫⢍⣉⠉⣑⠖⠮⢄⣀⣀ ⠉⠁        \n",
            " 0 ┤⠔⠊⠁     ⠉      ⠉⠉⠉⠉⠉⠉⠉⠑⠒⠢⠤\n",
            "   └┬───┬───┬────┬───┬───┬───┬\n",
            "    0   1   2    3   4   5   6\n",
            "● rx  ◆ tx                    \n",
        )
    );
    // Each series is drawn in its own colour, and the legend matches.
    assert!(out.contains("\x1b[36m●\x1b[0m rx"));
    assert!(out.contains("\x1b[35m◆\x1b[0m tx"));
}

#[test]
fn line_chart_without_colour_uses_markers() {
    // Braille dots cannot tell two series apart without colour.
    assert_eq!(
        render(&no_color(30), &line_chart()),
        concat!(
            "10 ┤                          \n",
            "   │◆◆         ●●●●    ●●●●●●●\n",
            " 5 ┤  ◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆◆   \n",
            " 0 ┤●●                     ◆◆◆\n",
            "   └┬───┬───┬────┬───┬───┬───┬\n",
            "    0   1   2    3   4   5   6\n",
            "● rx  ◆ tx                    \n",
        )
    );
    assert_eq!(
        render(&plain(30), &line_chart()),
        render(&no_color(30), &line_chart())
    );
    // One series stays in Braille.
    let one = LineChart::new()
        .series(Series::from_values("rx", [1.0, 4.0, 2.0, 6.0]))
        .height(2)
        .legend(false);
    assert_eq!(
        render(&plain(16), &one),
        concat!(
            "10 ┤           ⣀\n",
            " 0 ┤⠤⠒⠒⠉⠉⠒⠒⠤⠔⠊⠉ \n",
            "   └┬──────┬────\n",
            "    0      2    \n",
        )
    );
}

#[test]
fn line_chart_ascii_and_blocks() {
    assert_eq!(
        render(&ascii(30), &line_chart()),
        concat!(
            "10 +                          \n",
            "   |++         ****    *******\n",
            " 5 +  +++++++++++++++++++++   \n",
            " 0 +**                     +++\n",
            "   ++---+---+----+---+---+---+\n",
            "    0   1   2    3   4   5   6\n",
            "* rx  + tx                    \n",
        )
    );
    assert_eq!(
        strip_ansi(&render(&color(30), &line_chart().charset(Charset::Blocks))),
        render(&no_color(30), &line_chart())
    );
}

#[test]
fn scatter_chart() {
    let chart = LineChart::new()
        .series(Series::scatter(
            "a",
            [(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0)],
        ))
        .series(
            Series::line("b", [(0.0, 9.0), (3.0, 0.0)])
                .kind(SeriesKind::Scatter)
                .marker('#'),
        )
        .height(3)
        .y_format(ValueFormat::Fixed(1));
    assert_eq!(
        render(&ascii(24), &chart),
        concat!(
            "10.0 +#                *\n",
            "     |           *      \n",
            " 0.0 +*     *          #\n",
            "     ++-----+----+-----+\n",
            "      0     1    2     3\n",
            "* a  # b                \n",
        )
    );
    // Braille scatter: one dot per point, each series in its colour.
    let braille = render(&color(24), &chart.clone().charset(Charset::Braille));
    assert!(braille.contains("\x1b[36m⡀\x1b[0m"), "{braille:?}");
    assert!(braille.contains("\x1b[35m⠂\x1b[0m"), "{braille:?}");
}

#[test]
fn line_chart_degenerate_data() {
    let empty = LineChart::new().series(Series::from_values("x", []));
    assert_eq!(render(&plain(20), &empty), "no data\n");
    let nan = LineChart::new().series(Series::from_values("x", [f64::NAN, f64::INFINITY]));
    assert_eq!(render(&plain(20), &nan), "no data\n");
    let flat = LineChart::new()
        .series(Series::from_values("zero", [0.0, 0.0, 0.0]))
        .height(2)
        .charset(Charset::Ascii);
    assert_eq!(
        render(&plain(16), &flat),
        concat!(
            "1 +             \n",
            "0 +*************\n",
            "  ++-----+-----+\n",
            "   0     1     2\n",
            "* zero          \n",
        )
    );
    let single = LineChart::new()
        .series(Series::from_values("one", [-3.0]))
        .height(2)
        .charset(Charset::Ascii);
    assert_eq!(
        render(&plain(16), &single),
        concat!(
            "  0 +*          \n",
            "-10 +           \n",
            "    ++---------+\n",
            "     0         1\n",
            "* one           \n",
        )
    );
    // A NaN breaks a line.
    let gap = LineChart::new()
        .series(Series::from_values("gap", [1.0, 2.0, f64::NAN, 2.0, 1.0]))
        .height(2)
        .legend(false)
        .charset(Charset::Ascii);
    assert_eq!(
        render(&plain(16), &gap),
        concat!(
            "2 +  **     **  \n",
            "1 +**         **\n",
            "  ++-----+-----+\n",
            "   0     2     4\n",
        )
    );
}

// ------------------------------------------------------------ properties

fn every_chart() -> Vec<(&'static str, Box<dyn Renderable>)> {
    let long: Vec<f64> = (0..300).map(|i| ((i as f64) / 7.0).sin() * 100.0).collect();
    let mut out: Vec<(&'static str, Box<dyn Renderable>)> = Vec::new();
    for charset in [
        Charset::Auto,
        Charset::Blocks,
        Charset::Braille,
        Charset::Ascii,
    ] {
        out.push((
            "sparkline",
            Box::new(
                Sparkline::new(long.clone())
                    .charset(charset)
                    .min_max(true)
                    .threshold(50.0),
            ),
        ));
        out.push((
            "sparkline-empty",
            Box::new(Sparkline::new([]).charset(charset)),
        ));
        out.push((
            "bars",
            Box::new(
                BarChart::new()
                    .bar("a rather long label", 1234.5)
                    .bar("日本語", -300.0)
                    .bar("nan", f64::NAN)
                    .bar("zero", 0.0)
                    .charset(charset),
            ),
        ));
        out.push(("bars-empty", Box::new(BarChart::new().charset(charset))));
        out.push((
            "histogram",
            Box::new(Histogram::new(long.clone()).bins(12).charset(charset)),
        ));
        out.push((
            "line",
            Box::new(
                LineChart::new()
                    .series(Series::from_values(
                        "sine wave with a long name",
                        long.clone(),
                    ))
                    .series(Series::scatter("points", [(10.0, 5.0), (200.0, -80.0)]))
                    .series(Series::from_values("系列", [1e6, -1e6]))
                    .height(5)
                    .charset(charset),
            ),
        ));
        out.push(("line-empty", Box::new(LineChart::new().charset(charset))));
    }
    out
}

#[test]
fn no_line_is_wider_than_the_width_given() {
    let charts = every_chart();
    for width in 1..=200 {
        for console in [plain(width), ascii(width), color(width), no_color(width)] {
            for (name, chart) in &charts {
                let out = strip_ansi(&render(&console, chart.as_ref()));
                for line in out.lines() {
                    assert!(
                        cell_len(line) <= width,
                        "{name} at width {width}: {line:?} is {} cells",
                        cell_len(line)
                    );
                }
                if console.ascii_only() {
                    assert!(
                        out.chars().all(|c| (c as u32) < 0x80),
                        "{name} at width {width} is not ASCII: {out:?}"
                    );
                }
                let m = chart.measure(&console, &console.options());
                assert!(
                    m.minimum <= m.maximum && m.maximum <= width,
                    "{name} at {width}: {m:?}"
                );
            }
        }
    }
}

#[test]
fn ascii_charset_on_a_unicode_console_is_ascii() {
    for (name, chart) in every_chart().into_iter().skip(21) {
        let out = render(&plain(60), chart.as_ref());
        assert!(out.is_ascii(), "{name}: {out:?}");
    }
}

// ------------------------------------------------------------- containers

#[test]
fn charts_sit_in_table_cells_and_panels() {
    let charts: Vec<(&str, Arc<dyn Renderable + Send + Sync>)> = vec![
        ("spark", Arc::new(Sparkline::new(VALUES))),
        (
            "bars",
            Arc::new(BarChart::new().bar("a", 3.0).bar("b", 1.0).bar_width(8)),
        ),
        (
            "hist",
            Arc::new(Histogram::new(VALUES).bins(3).bar_width(8)),
        ),
        ("line", Arc::new(line_chart().width(24))),
    ];
    for (name, chart) in &charts {
        let mut table = Table::new();
        table.add_column("name");
        table.add_column("chart");
        table.add_row_cells(vec![
            Cell::Markup((*name).into()),
            Cell::Renderable(chart.clone()),
        ]);
        for width in [12, 30, 80] {
            let out = render(&plain(width), &table);
            assert!(
                out.lines().all(|l| cell_len(l) <= width),
                "{name} table {width}:\n{out}"
            );
        }
        // Wide enough that the table itself does not cut "name" with `…`.
        for width in [30, 80] {
            let out = render(&ascii(width), &table);
            assert!(out.is_ascii(), "{name} ascii table:\n{out}");
        }
        let panel = Panel::new(Box::new(ArcChart(chart.clone()))).title(*name);
        for width in [8, 30, 80] {
            let out = render(&plain(width), &panel);
            assert!(
                out.lines().all(|l| cell_len(l) <= width),
                "{name} panel {width}:\n{out}"
            );
        }
    }
    // A sparkline cell keeps its width: one cell per value.
    let mut table = Table::new();
    table.add_column("trend");
    table.add_row_cells(vec![Cell::Renderable(charts[0].1.clone())]);
    assert_eq!(
        render(&plain(80), &table),
        "┏━━━━━━━━━━━━━━┓\n┃ trend        ┃\n┡━━━━━━━━━━━━━━┩\n│ ▁▂▂▄▆▅▃▆█▇▅▃ │\n└──────────────┘"
    );
    let panel = Panel::new(Box::new(
        BarChart::new().bar("a", 3.0).bar("b", 1.0).bar_width(6),
    ))
    .title("bars")
    .expand(false);
    assert_eq!(
        render(&plain(80), &panel),
        "╭─── bars ───╮\n│ a ██████ 3 │\n│ b ██     1 │\n╰────────────╯"
    );
}

struct ArcChart(Arc<dyn Renderable + Send + Sync>);

impl Renderable for ArcChart {
    fn rich_render(&self, c: &Console, o: &rich::ConsoleOptions) -> Vec<rich::Segment> {
        self.0.rich_render(c, o)
    }
    fn measure(&self, c: &Console, o: &rich::ConsoleOptions) -> rich::measure::Measurement {
        self.0.measure(c, o)
    }
}

#[test]
fn charts_update_in_live() {
    let console = Console::builder()
        .width(30)
        .force_terminal(true)
        .color_system(None)
        .build();
    let mut out = Vec::new();
    let mut live = Live::new(Box::new(Sparkline::new([1.0, 2.0])), console, &mut out);
    live.start();
    live.update(Box::new(Sparkline::new([1.0, 2.0, 3.0])));
    live.update(Box::new(line_chart()));
    live.stop();
    drop(live);
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("▁█"), "{out:?}");
    assert!(out.contains("▁▅█"), "{out:?}");
    assert!(out.contains("● rx  ◆ tx"), "{out:?}");
}

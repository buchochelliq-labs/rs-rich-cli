//! Charts (0.0.15 workstream 1): sparklines, bars, histograms, and line and
//! scatter plots, in every glyph set, with colour on and off.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Cell, ColorSystem, Console, Live, Panel, Renderable, Table};
use rich_ext::chart::{
    Bar, BarChart, Charset, Histogram, LineChart, Orientation, Series, SeriesKind, Sparkline,
    ValueFormat,
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
        // Explicit, so an inherited NO_COLOR cannot turn colour off here.
        .no_color(false)
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
        .height(5)
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

// ---------------------------------------------------------- vertical bars

fn weekly() -> BarChart {
    BarChart::from_pairs([("mon", 3.0), ("tue", 7.25), ("wed", 5.5), ("thu", 0.0)])
        .orientation(Orientation::Vertical)
        .range(0.0, 8.0)
        .bar_width(4)
}

#[test]
fn vertical_bar_snapshots() {
    // Two units a row: 7.25 is three rows and five eighths.
    assert_eq!(
        render(&plain(40), &weekly()),
        concat!(
            "     7.25          \n",
            "     ▅▅▅  5.5      \n",
            " 3   ███  ▆▆▆      \n",
            "▄▄▄  ███  ███      \n",
            "███  ███  ███   0  \n",
            "mon  tue  wed  thu \n",
        )
    );
    assert_eq!(
        render(&plain(40), &weekly().charset(Charset::Braille)),
        concat!(
            "     7.25          \n",
            "     ⣤⣤⣤  5.5      \n",
            " 3   ⣿⣿⣿  ⣿⣿⣿      \n",
            "⣤⣤⣤  ⣿⣿⣿  ⣿⣿⣿      \n",
            "⣿⣿⣿  ⣿⣿⣿  ⣿⣿⣿   0  \n",
            "mon  tue  wed  thu \n",
        )
    );
    assert_eq!(
        render(&plain(40), &weekly().charset(Charset::Ascii)),
        concat!(
            "     7.25          \n",
            "     ...  5.5      \n",
            " 3   ###  ###      \n",
            "...  ###  ###      \n",
            "###  ###  ###   0  \n",
            "mon  tue  wed  thu \n",
        )
    );
    assert_eq!(
        render(&ascii(40), &weekly()),
        render(&plain(40), &weekly().charset(Charset::Ascii))
    );
}

#[test]
fn vertical_bars_colour_on_and_off() {
    let chart = BarChart::new()
        .bar("up", 4.0)
        .push(Bar::new("down", -2.0))
        .push(Bar::new("hot", 3.0).style("chart.over"))
        .orientation(Orientation::Vertical)
        .bar_width(3);
    assert_eq!(
        render(&color(40), &chart),
        concat!(
            " 4         3  \n",
            "\x1b[36m███\x1b[0m       \x1b[1;31m▄▄▄\x1b[0m \n",
            "\x1b[36m███\x1b[0m       \x1b[1;31m███\x1b[0m \n",
            "     \x1b[35m███\x1b[0m      \n",
            "      -2      \n",
            " up  down hot \n",
        )
    );
    // Without colour, height, the side of zero and the written value
    // carry it.
    assert_eq!(
        render(&no_color(40), &chart),
        concat!(
            " 4         3  \n",
            "███       ▄▄▄ \n",
            "███       ███ \n",
            "     ███      \n",
            "      -2      \n",
            " up  down hot \n",
        )
    );
}

#[test]
fn vertical_bars_shrink_values_labels_then_gaps() {
    let chart = BarChart::from_pairs([("requests", 100.0), ("errors", 25.0), ("drops", 12345.0)])
        .orientation(Orientation::Vertical)
        .bar_width(2);
    // Slots as wide as the widest label or value.
    let full = concat!(
        "                   12.3k  \n",
        "  100       25      ███   \n",
        "  ▁▁▁      ▁▁▁      ███   \n",
        "requests  errors   drops  \n",
    );
    assert_eq!(render(&plain(40), &chart), full);
    assert_eq!(render(&plain(26), &chart), full);
    // Values go first, then labels are cut, then dropped.
    assert_eq!(
        render(&plain(20), &chart),
        concat!(
            "               ███  \n",
            " ▁▁▁    ▁▁▁    ███  \n",
            "reque… errors drops \n",
        )
    );
    assert_eq!(render(&plain(8), &chart), "      ██\n▁▁ ▁▁ ██\nr… e… d…\n");
    assert_eq!(render(&plain(5), &chart), "    █\n▁ ▁ █\n");
    // Then the gaps, then the bars that do not fit.
    assert_eq!(render(&plain(3), &chart), "  █\n▁▁█\n");
    assert_eq!(render(&plain(2), &chart), "  \n▁▁\n");
    let m = chart.measure(&plain(80), &plain(80).options());
    assert_eq!((m.minimum, m.maximum), (3, 3 * 8 + 2));
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
            "8 ┤                   ⢀⡠⢄⣀⡀   \n",
            "6 ┤⠠⢄⣀⡀        ⡠⢄⣀⡀ ⡠⠔⠁   ⠈⠉  \n",
            "4 ┤   ⢈⡩⣉⠉⠉⠉⠒⡤⣊   ⠈⠉          \n",
            "2 ┤ ⡠⠔⠁  ⠉⠒⠤⠊  ⠉⠑⠒⠢⠤⠤⠤⠤⠤⢄⣀⡀   \n",
            "0 ┤⠈                      ⠈⠉  \n",
            "  └┬───┬───┬───┬───┬───┬───┬──\n",
            "   0   1   2   3   4   5   6  \n",
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
            "8 ┤                   ●●●     \n",
            "6 ┤◆◆         ●●●   ●●   ●●●  \n",
            "4 ┤  ◆◆◆◆◆◆◆◆●   ●●●          \n",
            "2 ┤ ●●   ●●● ◆◆◆◆◆◆◆◆◆◆◆◆     \n",
            "0 ┤●                     ◆◆◆  \n",
            "  └┬───┬───┬───┬───┬───┬───┬──\n",
            "   0   1   2   3   4   5   6  \n",
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
            "10 ┤          ⢀⡀\n",
            " 0 ┤⠐⠒⠊⠉⠉⠉⠒⠒⠊⠉⠁ \n",
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
            "8 +                   ***     \n",
            "6 +++         ***   **   ***  \n",
            "4 +  ++++++++*   ***          \n",
            "2 + **   *** ++++++++++++     \n",
            "0 +*                     +++  \n",
            "  ++---+---+---+---+---+---+--\n",
            "   0   1   2   3   4   5   6  \n",
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
            "10.0 +#              *  \n",
            "     |          *       \n",
            " 0.0 +*    *         #  \n",
            "     ++----+----+----+--\n",
            "      0    1    2    3  \n",
            "* a  # b                \n",
        )
    );
    // Braille scatter: one dot per point, each series in its colour.
    let braille = render(&color(24), &chart.clone().charset(Charset::Braille));
    assert!(braille.contains("\x1b[36m⠂\x1b[0m"), "{braille:?}");
    assert!(braille.contains("\x1b[35m⠠\x1b[0m"), "{braille:?}");
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

/// The value a chart label writes (`1.2k` is 1200).
fn read_label(label: &str) -> f64 {
    let (number, scale) = match label.chars().last() {
        Some('k') => (&label[..label.len() - 1], 1e3),
        Some('M') => (&label[..label.len() - 1], 1e6),
        _ => (label, 1.0),
    };
    number.parse::<f64>().expect(label) * scale
}

/// Read a rendered line chart's axes: `(row, value)` of each y label, the
/// columns of the x ticks, and the x labels.
fn axis_labels(out: &str) -> (Vec<(usize, f64)>, Vec<usize>, Vec<String>) {
    let lines: Vec<Vec<char>> = out.lines().map(|l| l.chars().collect()).collect();
    let rule = lines
        .iter()
        .position(|l| {
            l.contains(&'└') || l.iter().collect::<String>().trim_start().starts_with("++")
        })
        .expect("an x axis");
    let corner = lines[rule].iter().position(|c| *c != ' ').unwrap();
    let mut rows = Vec::new();
    for (row, line) in lines[..rule].iter().enumerate() {
        let label: String = line[..corner].iter().collect();
        if label.trim().is_empty() {
            assert!(matches!(line[corner], '│' | '|'), "{out}");
        } else {
            assert!(matches!(line[corner], '┤' | '+'), "{out}");
            rows.push((row, read_label(label.trim())));
        }
    }
    let ticks: Vec<usize> = lines[rule]
        .iter()
        .enumerate()
        .skip(corner + 1)
        .filter(|(_, c)| matches!(c, '┬' | '+'))
        .map(|(i, _)| i)
        .collect();
    let labels: Vec<String> = lines[rule + 1]
        .iter()
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    (rows, ticks, labels)
}

fn all_equal(gaps: &[usize]) -> bool {
    gaps.windows(2).all(|w| w[0] == w[1])
}

fn progression(values: &[f64]) -> bool {
    let steps: Vec<f64> = values.windows(2).map(|w| w[1] - w[0]).collect();
    steps
        .iter()
        .all(|s| (s - steps[0]).abs() <= 1e-9 * steps[0].abs().max(1.0))
}

#[test]
fn axis_labels_are_evenly_spaced_and_exact() {
    let wave: Vec<(f64, f64)> = (0..90)
        .map(|i| (i as f64 * 0.7, 37.0 + 41.0 * (i as f64 / 9.0).sin()))
        .collect();
    let datasets: Vec<(&str, Vec<(f64, f64)>)> = vec![
        ("wave", wave),
        ("small", vec![(0.0, 0.012), (1.0, 0.04), (2.0, 0.027)]),
        ("negative", vec![(-5.0, -30.0), (5.0, 12.5), (15.0, -2.0)]),
        ("large", vec![(1990.0, 1_200.0), (2025.0, 86_000.0)]),
        ("flat", vec![(0.0, 4.0), (1.0, 4.0)]),
    ];
    // With a fixed y range every row has a known value.
    let fixed = [
        None,
        Some((0.0, 100.0)),
        Some((-1.0, 1.0)),
        Some((0.0, 3.0)),
    ];
    for (name, points) in &datasets {
        for range in fixed {
            for height in 4..=20 {
                for (console, charset) in [
                    (plain(60), Charset::Braille),
                    (ascii(60), Charset::Ascii),
                    (plain(37), Charset::Braille),
                ] {
                    let mut chart = LineChart::new()
                        .series(Series::line(*name, points.clone()))
                        .height(height)
                        .charset(charset);
                    if let Some((lo, hi)) = range {
                        chart = chart.y_range(lo, hi);
                    }
                    let out = render(&console, &chart);
                    let (rows, ticks, labels) = axis_labels(&out);
                    let context = format!("{name} {range:?} height {height}\n{out}");

                    // Y: two labels or more, one on the bottom row, on
                    // evenly spaced rows and evenly spaced in value.
                    assert!(rows.len() >= 2, "{context}");
                    assert_eq!(rows.last().unwrap().0, height - 1, "{context}");
                    let gaps: Vec<usize> = rows.windows(2).map(|w| w[1].0 - w[0].0).collect();
                    assert!(all_equal(&gaps), "uneven y labels: {context}");
                    let values: Vec<f64> = rows.iter().map(|r| r.1).collect();
                    assert!(progression(&values), "y labels: {context}");
                    // Each label is exactly the value of its row.
                    if let Some((lo, hi)) = range {
                        let unit = (hi - lo) / (height - 1) as f64;
                        for (row, value) in &rows {
                            let own = lo + (height - 1 - row) as f64 * unit;
                            assert!((value - own).abs() < 1e-9, "row {row}: {context}");
                        }
                    }

                    // X: every tick labelled, on evenly spaced columns.
                    assert!(ticks.len() >= 2, "{context}");
                    assert_eq!(ticks.len(), labels.len(), "{context}");
                    let gaps: Vec<usize> = ticks.windows(2).map(|w| w[1] - w[0]).collect();
                    assert!(all_equal(&gaps), "uneven x labels: {context}");
                    let values: Vec<f64> = labels.iter().map(|l| read_label(l)).collect();
                    assert!(progression(&values), "x labels: {context}");
                }
            }
        }
    }
}

#[test]
fn default_height_fits_its_labels() {
    // Without `.height`, 0..100 takes 9 rows: a label every 25, two rows
    // apart.
    let chart = LineChart::new()
        .series(Series::from_values("p", [0.0, 50.0, 100.0]))
        .y_range(0.0, 100.0)
        .legend(false)
        .charset(Charset::Ascii);
    let out = render(&plain(30), &chart);
    let (rows, _, _) = axis_labels(&out);
    assert_eq!(
        rows,
        [(0, 100.0), (2, 75.0), (4, 50.0), (6, 25.0), (8, 0.0)],
        "{out}"
    );
}

// ------------------------------------------------------------ properties

fn every_chart() -> Vec<(&'static str, Box<dyn Renderable>)> {
    [
        Charset::Auto,
        Charset::Blocks,
        Charset::Braille,
        Charset::Ascii,
    ]
    .into_iter()
    .flat_map(charts_for)
    .collect()
}

fn charts_for(charset: Charset) -> Vec<(&'static str, Box<dyn Renderable>)> {
    let long: Vec<f64> = (0..300).map(|i| ((i as f64) / 7.0).sin() * 100.0).collect();
    let mut out: Vec<(&'static str, Box<dyn Renderable>)> = Vec::new();
    {
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
            "vertical-bars",
            Box::new(
                BarChart::new()
                    .bar("a rather long label", 1234.5)
                    .bar("日本語", -300.0)
                    .bar("nan", f64::NAN)
                    .bar("zero", 0.0)
                    .bar("tiny", 0.01)
                    .orientation(Orientation::Vertical)
                    .charset(charset),
            ),
        ));
        out.push((
            "vertical-histogram",
            Box::new(
                Histogram::new(long.clone())
                    .bins(12)
                    .orientation(Orientation::Vertical)
                    .charset(charset),
            ),
        ));
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
    for (name, chart) in charts_for(Charset::Ascii) {
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

//! Pie and donut charts, stacked bars and areas (0.0.21's new renderables),
//! in every glyph set, with colour on and off; and gradient text.

use rich::cells::cell_len;
use rich::{Color, ColorSystem, Console, Renderable, Text};
use rich_ext::a11y::AccessibleText;
use rich_ext::chart::{
    Axis, Bar, BarChart, BarGroup, Charset, Chart, Dataset, GraphType, LegendPosition, Marker,
    Orientation, PieChart, Slice,
};
use rich_ext::gradient::{GradientDirection, GradientText};

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

fn colour_with(width: usize, system: ColorSystem) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(system))
        // Explicit, so an inherited NO_COLOR cannot turn colour off here.
        .no_color(false)
        .theme(rich_ext::extended_theme())
        .build()
}

fn color(width: usize) -> Console {
    colour_with(width, ColorSystem::Standard)
}

fn render(console: &Console, r: &dyn Renderable) -> String {
    console.render_export(r)
}

fn pie() -> PieChart {
    PieChart::from_pairs([("api", 50.0), ("web", 30.0), ("cron", 20.0)]).height(3)
}

#[test]
fn pie_snapshots() {
    assert_eq!(
        render(&plain(40), &pie()),
        concat!(
            " ▓▓██   █ api   50.0%\n",
            "░░░███  ░ web   30.0%\n",
            " ░░██   ▓ cron  20.0%\n",
        )
    );
    assert_eq!(
        render(&plain(40), &pie().charset(Charset::Braille)),
        concat!(
            "⠠⣶⡇⢸⣶⡄  ⣿ api   50.0%\n",
            "⣦⣤⡄⢸⣿⣿  ⣿ web   30.0%\n",
            "⠘⠿⡇⢸⠿⠃  ⣿ cron  20.0%\n",
        )
    );
    // An ASCII console gets ASCII whatever was asked for.
    assert_eq!(
        render(&ascii(40), &pie().charset(Charset::Braille)),
        render(&plain(40), &pie().charset(Charset::Ascii))
    );
    assert!(render(&ascii(40), &pie()).is_ascii());
}

#[test]
fn pie_colour_draws_half_blocks_two_slices_to_a_cell() {
    assert_eq!(
        render(&color(40), &pie()),
        concat!(
            "\x1b[33m▄██\x1b[0m\x1b[36m██▄\x1b[0m  \x1b[36m■\x1b[0m api   50.0%\n",
            "\x1b[35m█\x1b[0m\x1b[33;45m▀▀\x1b[0m\x1b[36m███\x1b[0m  \x1b[35m■\x1b[0m web   30.0%\n",
            "\x1b[35m▀██\x1b[0m\x1b[36m██▀\x1b[0m  \x1b[33m■\x1b[0m cron  20.0%\n",
        )
    );
    // A slice's own style wins over its series style.
    let styled = PieChart::new()
        .push(Slice::new("ok", 1.0).style("green"))
        .height(1)
        .legend(false);
    assert_eq!(render(&color(40), &styled), "\x1b[32m██\x1b[0m\n");
}

#[test]
fn pie_legend_goes_under_a_narrow_disc_and_the_disc_shrinks() {
    assert_eq!(
        render(&plain(8), &pie()),
        concat!(
            " ▓▓██   \n",
            "░░░███  \n",
            " ░░██   \n",
            "█ api   \n",
            "░ web   \n",
            "▓ cron  \n",
        )
    );
    let narrow = render(&plain(4), &pie().legend(false));
    assert_eq!(narrow.lines().count(), 2);
    assert!(narrow.lines().all(|l| cell_len(l) <= 4));
}

#[test]
fn donuts_have_a_hole_and_text_in_it() {
    let donut = pie().height(6).donut(0.5).centre_text("100").legend(false);
    assert_eq!(
        render(&plain(40), &donut),
        concat!(
            "   ▓▓▓███   \n",
            " ▓▓▓▓▓█████ \n",
            "░░░ 100  ███\n",
            "░░░      ███\n",
            " ░░░░░█████ \n",
            "   ░░░███   \n",
        )
    );
    // Text too wide for the hole is left out.
    let wide = donut.clone().centre_text("a long total");
    assert!(!render(&plain(40), &wide).contains("long"));
}

#[test]
fn pie_values_without_a_share_stay_in_the_legend() {
    let chart = pie().show_values(true).slice("idle", -1.0);
    assert_eq!(
        render(&plain(40), &chart),
        concat!(
            " ▓▓██   █ api   50.0%  50\n",
            "░░░███  ░ web   30.0%  30\n",
            " ░░██   ▓ cron  20.0%  20\n",
            "        ▒ idle      -  -1\n",
        )
    );
    assert_eq!(render(&plain(40), &PieChart::new()), "no data\n");
    assert_eq!(
        render(&plain(40), &PieChart::new().slice("none", 0.0)),
        "no data\n"
    );
}

#[test]
fn pie_fills_the_height_it_is_given() {
    let console = plain(60);
    let options = console.options().update_height(5);
    let rows = |r: &dyn Renderable| {
        let segments = r.rich_render(&console, &options);
        console.segments_to_string(&segments).lines().count()
    };
    // `.height(3)` wins over the height given.
    assert_eq!(rows(&pie()), 3);
    let free = PieChart::from_pairs([("a", 1.0), ("b", 2.0)]);
    assert_eq!(rows(&free), 5);
    // With the legend under it, the disc leaves it room.
    let narrow = plain(12);
    let segments = free.rich_render(&narrow, &narrow.options().update_height(5));
    assert_eq!(narrow.segments_to_string(&segments).lines().count(), 5);
}

#[test]
fn pie_accessible_text_lists_shares() {
    assert_eq!(
        pie().accessible_text(80),
        "Pie chart, 3 slices, total 100\napi 50 (50.0%), web 30 (30.0%), cron 20 (20.0%)"
    );
    assert_eq!(
        pie().donut(0.4).slice("idle", f64::NAN).accessible_text(80),
        "Donut chart, 4 slices, total 100, 1 without a share\n\
         api 50 (50.0%), web 30 (30.0%), cron 20 (20.0%), idle no share"
    );
}

fn quarters() -> BarChart {
    BarChart::new()
        .group(
            BarGroup::new("Q1")
                .stacked()
                .bar(Bar::new("a", 4.0))
                .bar(Bar::new("b", 2.0)),
        )
        .group(
            BarGroup::new("Q2")
                .stacked()
                .bar(Bar::new("a", 3.0))
                .bar(Bar::new("b", 3.0)),
        )
        .bar_width(12)
}

#[test]
fn stacked_bar_snapshots() {
    assert_eq!(
        render(&plain(40), &quarters()),
        "Q1 ████████▓▓▓▓ 6\nQ2 ██████▓▓▓▓▓▓ 6\n█ a  ▓ b         \n"
    );
    assert_eq!(
        render(&ascii(40), &quarters()),
        "Q1 ########==== 6\nQ2 ######====== 6\n# a  = b         \n"
    );
    assert_eq!(
        render(&color(40), &quarters()),
        "Q1 \x1b[36m████████\x1b[0m\x1b[35m████\x1b[0m 6\n\
         Q2 \x1b[36m██████\x1b[0m\x1b[35m██████\x1b[0m 6\n\
         \x1b[36m■\x1b[0m a  \x1b[35m■\x1b[0m b         \n"
    );
    assert_eq!(
        render(
            &plain(40),
            &quarters().orientation(Orientation::Vertical).bar_width(4)
        ),
        "6  6    \n▓▓ ▓▓   \n██ ▓▓   \n██ ██   \n██ ██   \nQ1 Q2   \n█ a  ▓ b\n"
    );
    // Given less room the bar shrinks, then the values go.
    assert_eq!(
        render(&plain(10), &quarters()),
        "Q1 ███▓▓ 6\nQ2 ███▓▓ 6\n█ a  ▓ b  \n"
    );
    assert_eq!(
        render(&plain(40), &quarters().legend(false)),
        "Q1 ████████▓▓▓▓ 6\nQ2 ██████▓▓▓▓▓▓ 6\n"
    );
}

#[test]
fn stacked_parts_below_zero_stack_down_from_zero() {
    let chart = BarChart::new()
        .group(
            BarGroup::new("net")
                .stacked()
                .bar(Bar::new("in", 4.0))
                .bar(Bar::new("out", -2.0))
                .bar(Bar::new("fees", 2.0)),
        )
        .bar_width(12);
    // The scale runs from -2 to 6: `out` left of zero, `in` and `fees`
    // right of it.
    assert_eq!(
        render(&plain(40), &chart),
        "net ▓▓▓██████░░░ 4 \n█ in  ▓ out  ░ fees\n"
    );
}

#[test]
fn stacked_colour_ends_in_a_partial_cell() {
    let chart = BarChart::new()
        .group(
            BarGroup::new("x")
                .stacked()
                .bar(Bar::new("a", 1.0))
                .bar(Bar::new("b", 1.5)),
        )
        .range(0.0, 4.0)
        .bar_width(8)
        .legend(false);
    assert_eq!(
        render(&color(40), &chart),
        "x \x1b[36m██\x1b[0m\x1b[35m███\x1b[0m    2.5\n"
    );
}

#[test]
fn stacked_and_plain_bars_mix() {
    let chart = quarters().bar("Q3", 5.0);
    let out = render(&plain(40), &chart);
    assert!(out.contains("Q3 ██████████   5"), "{out}");
}

#[test]
fn stacked_accessible_text_names_totals_and_parts() {
    assert_eq!(
        quarters().accessible_text(80),
        "Stacked bar chart, 2 stacks of 2 parts, totals at 6\n\
         Q1 total 6 (a 4, b 2), Q2 total 6 (a 3, b 3)"
    );
}

fn layers() -> Chart {
    let area = |points: [(f64, f64); 2], c: char| {
        Dataset::new(points)
            .graph_type(GraphType::Area)
            .marker(Marker::Ascii(c))
    };
    Chart::new(vec![
        area([(0.0, 1.0), (7.0, 1.0)], '#').name("a"),
        area([(0.0, 1.0), (7.0, 2.0)], '+').name("b"),
    ])
    .x_axis(Axis::default().bounds(0.0, 7.0))
    .legend(LegendPosition::None)
    .stacked(true)
    .width(8)
    .height(4)
}

#[test]
fn stacked_areas_sit_on_the_ones_below() {
    // The y axis runs from 0 (the fill level) to the top total, 3.
    assert_eq!(
        render(&plain(8), &layers()),
        "    ++++\n++++++++\n########\n########\n"
    );
    // Unstacked, the axis fits the values and the second area covers the
    // first.
    assert_eq!(
        render(&plain(8), &layers().stacked(false)),
        "      ++\n    ++++\n  ++++++\n++++++++\n"
    );
}

#[test]
fn stacked_areas_keep_their_own_values_in_words() {
    let text = layers().accessible_text(80);
    assert!(
        text.starts_with("Chart, 2 datasets, x from 0 to 7, y from 0 to 3, areas stacked"),
        "{text}"
    );
    assert!(text.contains("a, area: 2 points, min 1, max 1"), "{text}");
}

fn stops() -> [Color; 2] {
    ["#ff0000", "#0000ff"].map(|c| Color::parse(c).unwrap())
}

#[test]
fn gradient_text_degrades_by_colour_system() {
    let text = GradientText::new(Text::new("abcdef"), stops());
    assert_eq!(
        render(&colour_with(40, ColorSystem::EightBit), &text),
        "\x1b[38;5;196ma\x1b[0m\x1b[38;5;161mb\x1b[0m\x1b[38;5;89mc\x1b[0m\
         \x1b[38;5;54md\x1b[0m\x1b[38;5;56me\x1b[0m\x1b[38;5;21mf\x1b[0m\n"
    );
    // On 16 colours, neighbours that come out the same are one run.
    assert_eq!(
        render(&color(40), &text),
        "\x1b[31mab\x1b[0m\x1b[35mcd\x1b[0m\x1b[34mef\x1b[0m\n"
    );
    assert_eq!(render(&plain(40), &text), "abcdef\n");
    let no_color = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .no_color(true)
        .build();
    assert_eq!(render(&no_color, &text), "abcdef\n");
}

#[test]
fn gradient_text_runs_across_down_and_corner_to_corner() {
    let console = colour_with(40, ColorSystem::Truecolor);
    let two = Text::new("ab\ncd");
    let across = GradientText::new(two.clone(), stops());
    assert_eq!(
        render(&console, &across),
        "\x1b[38;2;255;0;0ma\x1b[0m\x1b[38;2;0;0;255mb\x1b[0m\n\
         \x1b[38;2;255;0;0mc\x1b[0m\x1b[38;2;0;0;255md\x1b[0m\n"
    );
    let down = GradientText::new(two.clone(), stops()).direction(GradientDirection::Vertical);
    assert_eq!(
        render(&console, &down),
        "\x1b[38;2;255;0;0mab\x1b[0m\n\x1b[38;2;0;0;255mcd\x1b[0m\n"
    );
    let corner = GradientText::new(two, stops()).direction(GradientDirection::Diagonal);
    assert_eq!(
        render(&console, &corner),
        "\x1b[38;2;255;0;0ma\x1b[0m\x1b[38;2;128;0;128mb\x1b[0m\n\
         \x1b[38;2;128;0;128mc\x1b[0m\x1b[38;2;0;0;255md\x1b[0m\n"
    );
}

#[test]
fn gradient_text_keeps_attributes_and_can_paint_the_background() {
    let console = colour_with(40, ColorSystem::Truecolor);
    let bold = GradientText::new(Text::from_markup("[bold]a[/]b").unwrap(), stops());
    assert_eq!(
        render(&console, &bold),
        "\x1b[1;38;2;255;0;0ma\x1b[0m\x1b[38;2;0;0;255mb\x1b[0m\n"
    );
    let behind = GradientText::new(Text::new("ab"), stops()).background(true);
    assert_eq!(
        render(&console, &behind),
        "\x1b[48;2;255;0;0ma\x1b[0m\x1b[48;2;0;0;255mb\x1b[0m\n"
    );
    assert_eq!(behind.accessible_text(80), "ab");
}

#[test]
fn nothing_is_wider_than_its_width() {
    let renderables: Vec<Box<dyn Renderable>> = vec![
        Box::new(pie()),
        Box::new(pie().height(10).donut(0.5).centre_text("total 100")),
        Box::new(pie().charset(Charset::Braille)),
        Box::new(quarters()),
        Box::new(quarters().orientation(Orientation::Vertical)),
        Box::new(layers()),
        Box::new(GradientText::new(
            Text::new("a gradient over a long line of text that wraps"),
            stops(),
        )),
    ];
    for width in 1..=60 {
        for console in [plain(width), ascii(width), color(width)] {
            for r in &renderables {
                let out = console.render_to_string(r.as_ref());
                for line in out.lines() {
                    let line = strip_ansi(line);
                    assert!(cell_len(&line) <= width, "width {width}: {line:?}");
                }
            }
        }
    }
}

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

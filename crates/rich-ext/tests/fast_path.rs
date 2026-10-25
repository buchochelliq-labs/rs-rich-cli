//! rs-rich-ext's fast path ([`Draw`]) against rich's path
//! ([`draw_rendered`]): every chart with a `Draw` writes the same cells,
//! text, width and style, as rendering it and writing its lines does. At
//! several sizes, into a rectangle inside a screen that already holds wide
//! characters across its edges, with colour and without.

use rich::{Color, Console, Segment, Style};
use rich_ext::chart::{
    Axis, Bar, BarChart, BarGroup, BarSpectrum, Canvas, Charset, Chart, Dataset, FillGauge,
    GraphType, Heatmap, KpiCard, Labels, LineChart, LineGauge, Marker, PieChart, Series, Sparkline,
    Spectrogram, StatusMatrix, Timeline,
};
use rich_ext::surface::{draw_rendered, Draw, Rect, Screen, TextRun};

/// A screen already drawn on: text, a wide character across the
/// rectangle's left and right edges, and a style.
fn backdrop(width: u16, height: u16, rect: Rect) -> Screen {
    let mut screen = Screen::new(width, height);
    let red = Style::from_color(Some(Color::parse("red").unwrap()), None);
    let row = "日本語のテキスト ".repeat(width as usize);
    let lines: Vec<Vec<Segment>> = (0..height)
        .map(|_| vec![Segment::new(row.clone(), Some(red.clone()))])
        .collect();
    screen.write_lines(screen.area(), &lines);
    // A wide character starting just left of the rectangle.
    if rect.x > 0 {
        screen.write_lines(
            Rect::new(rect.x - 1, rect.y, 2, 1),
            &[vec![Segment::new("字", None)]],
        );
    }
    screen
}

/// Every cell of `a` and `b`, with its style resolved.
fn cells(screen: &Screen) -> Vec<(String, u8, Option<Style>)> {
    let area = screen.area();
    let mut out = Vec::new();
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = screen.cell(x, y);
            out.push((
                cell.text().to_string(),
                cell.width,
                screen.style(cell.style).cloned(),
            ));
        }
    }
    out
}

fn consoles() -> Vec<(&'static str, Console)> {
    vec![
        ("truecolor", Console::builder().width(80).build()),
        (
            "no colour",
            Console::builder().width(80).color_system(None).build(),
        ),
    ]
}

/// `drawable` both ways, at each size, compared cell by cell.
fn same(name: &str, drawable: &dyn Draw) {
    for (label, console) in consoles() {
        for (w, h) in [(1, 1), (7, 3), (24, 6), (40, 12), (80, 24)] {
            for (x, y) in [(0, 0), (3, 1)] {
                let rect = Rect::new(x, y, w, h);
                let (sw, sh) = (w + x + 4, h + y + 2);
                let options = console
                    .options()
                    .update_width(w as usize)
                    .update_height(h as usize);
                let mut fast = backdrop(sw, sh, rect);
                drawable.draw(&console, &options, &mut fast, rect);
                let mut slow = backdrop(sw, sh, rect);
                draw_rendered(&console, drawable, &options, &mut slow, rect);
                let (f, s) = (cells(&fast), cells(&slow));
                if f != s {
                    let at = f.iter().zip(&s).position(|(a, b)| a != b).unwrap();
                    panic!(
                        "{name}, {label}, {w}x{h} at {x},{y}: cell {} ({}, {}) differs:\n fast {:?}\n rich {:?}\nfast {:#?}\nrich {:#?}",
                        at,
                        at % sw as usize,
                        at / sw as usize,
                        f[at],
                        s[at],
                        fast.plain(),
                        slow.plain()
                    );
                }
            }
        }
    }
}

fn wave(n: usize, phase: f64) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let x = i as f64 / 10.0;
            (x, (x + phase).sin())
        })
        .collect()
}

#[test]
fn charts_draw_the_cells_rich_renders() {
    let chart = Chart::new([Dataset::new(wave(200, 0.4))
        .marker(Marker::Braille)
        .graph_type(GraphType::Line)
        .style("cyan")])
    .x_axis(Axis::default().bounds(0.0, 20.0))
    .y_axis(Axis::default().bounds(-1.0, 1.0));
    same("chart", &chart);

    let labelled = Chart::new([
        Dataset::new(wave(50, 0.0)).name("sin").style("green"),
        Dataset::new(wave(50, 1.5))
            .name("日本")
            .marker(Marker::HalfBlock)
            .graph_type(GraphType::Line)
            .style("bold magenta on blue"),
    ])
    .x_axis(
        Axis::default()
            .bounds(0.0, 5.0)
            .title("time")
            .labels(Labels::Text(vec!["0".into(), "2.5".into(), "5".into()])),
    )
    .y_axis(
        Axis::default()
            .bounds(-1.0, 1.0)
            .title("值")
            .labels(Labels::Markup(vec![
                "[b]-1[/]".into(),
                "0".into(),
                "1".into(),
            ])),
    )
    .style("on grey11");
    same("labelled chart", &labelled);

    for marker in [
        Marker::Dot,
        Marker::Block,
        Marker::Bar,
        Marker::Quadrant,
        Marker::Sextant,
        Marker::Octant,
        Marker::HalfBlock,
        Marker::Ascii('*'),
    ] {
        let chart = Chart::new([Dataset::new(wave(80, 0.2)).marker(marker).style("yellow")]);
        same(&format!("chart {marker:?}"), &chart);
    }

    let canvas = Canvas::new()
        .x_bounds(-1.0, 1.0)
        .y_bounds(-1.0, 1.0)
        .paint(|p| {
            p.circle(0.0, 0.0, 0.8, "cyan");
            p.line(-1.0, -1.0, 1.0, 1.0, "red");
            p.layer();
            p.print(-0.2, 0.0, "hi 世界", "bold");
        })
        .background("on navy_blue");
    same("canvas", &canvas);
}

#[test]
fn line_charts_draw_the_cells_rich_renders() {
    same(
        "sparkline",
        &Sparkline::new([1.0, 3.0, 2.0, 8.0, 5.0, 0.0, 9.0]).style("green"),
    );
    same(
        "sparkline braille",
        &Sparkline::new((0..40).map(|i| (i as f64 / 3.0).sin())).charset(Charset::Braille),
    );
    same(
        "bar chart",
        &BarChart::new()
            .group(
                BarGroup::new("Q1")
                    .bar(Bar::new("a", 3.0))
                    .bar(Bar::new("b", 5.0)),
            )
            .group(BarGroup::new("Q2").bars([Bar::new("a", 4.0), Bar::new("b", 2.0)])),
    );
    same("fill gauge", &FillGauge::new(0.42).height(3));
    same(
        "line gauge",
        &LineGauge::new(0.42).label("Downloading: 42%"),
    );
    same(
        "timeline",
        &Timeline::new()
            .span("fetch", 0.0, 4.0)
            .span("compile", 4.0, 26.0)
            .span("test", 12.0, 30.0)
            .milestone("ship", 36.0)
            .unit("s"),
    );
    same(
        "heatmap",
        &Heatmap::new()
            .columns(["mon", "tue", "wed", "thu"])
            .row("api", [1.0, 4.0, 9.0, 2.0])
            .row("web", [0.0, 6.0, 8.0, f64::NAN]),
    );
    same(
        "kpi",
        &KpiCard::new("Errors", 42.0)
            .previous(56.0)
            .caption("vs yesterday"),
    );
    same(
        "line chart",
        &LineChart::new()
            .series(Series::from_values("load", [1.0, 3.0, 2.0, 5.0, 4.0]))
            .height(6),
    );
    same(
        "pie",
        &PieChart::new()
            .slice("a", 3.0)
            .slice("b", 1.0)
            .slice("c", 2.0),
    );
    same(
        "bar spectrum",
        &BarSpectrum::new([1.0, 0.5, 0.25, 0.8]).range(1.0).height(2),
    );
    same(
        "status matrix",
        &StatusMatrix::new()
            .columns(["linux", "macos", "win"])
            .row("unit", ["pass", "pass", "fail"])
            .row("e2e", ["pass", "flaky", "skip"]),
    );
    let mut waterfall = Spectrogram::new(3).range(0.0, 9.0);
    waterfall.push([9.0, 0.0, 0.0, 0.0]);
    waterfall.push([0.0, 9.0, 0.0, 4.5]);
    same("spectrogram", &waterfall);
}

#[test]
fn runs_write_what_segments_write() {
    // Text the cell path takes, the grapheme path, controls, a null style
    // and a link: runs and segments give the same cells.
    let texts = [
        "plain ascii",
        "日本語 wide",
        "cafe\u{301} e\u{301}",
        "tab\there\u{7}",
        "\u{301}leading mark",
        "👨\u{200d}👩\u{200d}👧 family",
    ];
    let styles = [
        None,
        Some(Style::parse("bold red").unwrap()),
        Some(Style::null()),
        Some(Style::parse("link https://example.com").unwrap()),
    ];
    for text in texts {
        for style in &styles {
            for width in [3u16, 8, 30] {
                let rect = Rect::new(2, 1, width, 2);
                let mut slow = backdrop(width + 6, 4, rect);
                let line = vec![Segment::new(text, style.clone()), Segment::new("!", None)];
                slow.write_lines(rect, &[line.clone(), line]);
                let mut fast = backdrop(width + 6, 4, rect);
                let id = fast.intern(style.as_ref());
                let none = fast.intern(None);
                let runs = vec![TextRun::new(text, id), TextRun::new("!", none)];
                fast.write_runs(rect, &[runs.clone(), runs]);
                assert_eq!(
                    cells(&fast),
                    cells(&slow),
                    "{text:?} in {style:?} at width {width}"
                );
            }
        }
    }
}

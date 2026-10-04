//! KPI and status renderables (0.0.15 workstream 2): gauges and bullet
//! charts, heatmaps, status matrices, KPI cards and timelines, in blocks,
//! Braille and ASCII, with colour on and off. The width sweep and the
//! ASCII-only checks for them are in `charts.rs`, with the other charts.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Cell, ColorSystem, Columns, Console, Layout, Live, Panel, Renderable, Table};
use rich_ext::chart::{
    Band, BulletChart, Charset, Gauge, Heatmap, KpiCard, Span, Sparkline, State, Status,
    StatusMatrix, Timeline, ValueFormat,
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

fn measure(console: &Console, r: &dyn Renderable) -> (usize, usize) {
    let m = r.measure(console, &console.options());
    (m.minimum, m.maximum)
}

fn cpu() -> Gauge {
    Gauge::new("cpu", 72.0)
        .range(0.0, 100.0)
        .target(80.0)
        .band(Band::new(60.0, "ok").style("chart.ok"))
        .band(Band::new(85.0, "high").style("chart.warning"))
        .band(Band::new(100.0, "critical").style("chart.critical"))
        .unit("%")
        .bar_width(20)
}

fn ci() -> StatusMatrix {
    StatusMatrix::new()
        .columns(["linux", "macos", "windows"])
        .row("unit", ["pass", "pass", "fail"])
        .row("integration", ["pass", "flaky", "skip"])
        .row("docs", ["pass", "pending"])
}

fn heat() -> Heatmap {
    Heatmap::new()
        .columns(["mon", "tue", "wed", "thu", "fri"])
        .row("api", [1.0, 4.0, 9.0, 2.0, 5.0])
        .row("web", [0.0, 6.0, 8.0, f64::NAN, 3.0])
        .row("db", [2.0, 2.0, 7.0, 3.0, 1.0])
}

const TREND: [f64; 12] = [
    3.0, 5.0, 4.0, 8.0, 12.0, 9.0, 7.0, 11.0, 15.0, 13.0, 10.0, 6.0,
];

fn card() -> KpiCard {
    KpiCard::new("Requests", 12_400.0)
        .unit("/s")
        .previous(11_430.0)
        .caption("vs last week")
        .trend(TREND)
        .status(Status::Ok)
}

fn errors() -> KpiCard {
    KpiCard::new("Errors", 42.0)
        .delta(-14.0)
        .higher_is_better(false)
        .status(Status::Warning)
}

fn build() -> Timeline {
    Timeline::new()
        .span("fetch", 0.0, 4.0)
        .span("compile", 4.0, 26.0)
        .span("test", 12.0, 30.0)
        .span("test", 20.0, 34.0)
        .milestone("ship", 36.0)
        .unit("s")
}

fn jobs() -> Timeline {
    Timeline::new()
        .span("a", 0.0, 3.0)
        .span("a", 3.0, 5.0)
        .span("b", 2.0, 6.0)
        .span("a", 600.0, 604.0)
        .span("b", 602.0, 610.0)
        .milestone("deploy", 611.0)
        .unit("s")
}

// ----------------------------------------------------------------- gauges

#[test]
fn gauge_snapshots() {
    assert_eq!(
        render(&plain(40), &cpu()),
        "cpu ██████████████▍▒│░░░ 72% high\n"
    );
    assert_eq!(
        render(&plain(40), &cpu().charset(Charset::Braille)),
        "cpu ⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⣿⡇▒│░░░ 72% high\n"
    );
    assert_eq!(
        render(&plain(40), &cpu().charset(Charset::Ascii)),
        "cpu ##############=:|... 72% high\n"
    );
    // An ASCII console gets ASCII whatever was asked for.
    assert_eq!(
        render(&ascii(40), &cpu().charset(Charset::Blocks)),
        "cpu ##############=:|... 72% high\n"
    );
    assert_eq!(
        render(
            &plain(40),
            &Gauge::new("disk", 0.42).range(0.0, 1.0).bar_width(10)
        ),
        "disk ████▎░░░░░ 0.42\n"
    );
    // Over the range: a full bar, and the value says by how much.
    assert_eq!(
        render(
            &plain(40),
            &Gauge::new("x", 150.0).range(0.0, 100.0).bar_width(10)
        ),
        "x ██████████ 150\n"
    );
    assert_eq!(
        render(&plain(40), &Gauge::new("x", f64::NAN).bar_width(10)),
        "x ░░░░░░░░░░ -\n"
    );
}

#[test]
fn gauge_full_width() {
    assert_eq!(
        render(&plain(40), &cpu().full_width(true)),
        concat!(
            "cpu ███████████████████▌▒│▒░░░░ 72% high\n",
            "    0%             60% 80% 100%         \n",
            "░ ok up to 60%  ▒ high up to 85%        \n",
            "░ critical up to 100%  │ target 80%     \n",
        )
    );
    assert_eq!(
        render(&ascii(40), &cpu().full_width(true)),
        concat!(
            "cpu ###################=:|:.... 72% high\n",
            "    0%             60% 80% 100%         \n",
            ". ok up to 60%  : high up to 85%        \n",
            ". critical up to 100%  | target 80%     \n",
        )
    );
    assert_eq!(measure(&plain(40), &cpu().full_width(true)), (4, 40));
}

#[test]
fn gauge_colour_on_and_off() {
    // The bar takes the style of the band the value is in.
    assert_eq!(
        render(&color(40), &cpu()),
        "cpu \x1b[33m██████████████▍\x1b[0m\x1b[90m▒\x1b[0m\x1b[1m│\x1b[0m\x1b[90m░░░\x1b[0m 72% \x1b[33mhigh\x1b[0m\n"
    );
    // Without colour the band's name says the same.
    assert_eq!(
        render(&no_color(40), &cpu()),
        "cpu ██████████████▍▒│░░░ 72% high\n"
    );
}

#[test]
fn gauge_shrinks_bar_then_band_then_value_then_label() {
    let at = |w: usize| render(&plain(w), &cpu());
    assert_eq!(at(30), "cpu ████████████▎│░░░ 72% high\n");
    assert_eq!(at(20), "cpu █████│░ 72% high\n");
    assert_eq!(at(12), "cpu ██▉│ 72%\n");
    // A label cut to one cell would be only the `…`: the bar takes it all.
    assert_eq!(at(6), "████│░\n");
    assert_eq!(measure(&plain(80), &cpu()), (4, 33));
}

#[test]
fn gauge_scale_and_bands() {
    let g = Gauge::new("q", 3.0).band(Band::new(10.0, "low"));
    // The scale runs from zero to the largest of value, target and bands.
    assert_eq!(g.current_band().map(|b| b.label.as_str()), Some("low"));
    assert_eq!(
        render(&plain(40), &g.clone().bar_width(10)),
        "q ███░░░░░░░ 3 low\n"
    );
    // Bands may be added in any order.
    let g = Gauge::new("q", 50.0)
        .band(Band::new(100.0, "b"))
        .band(Band::new(40.0, "a"));
    assert_eq!(g.current_band().map(|b| b.label.as_str()), Some("b"));
    assert!(Gauge::new("q", f64::NAN).current_band().is_none());
}

#[test]
fn bullet_chart_aligns_its_columns() {
    let chart = BulletChart::new()
        .gauge(Gauge::new("revenue", 270.0).range(0.0, 300.0).target(250.0))
        .gauge(cpu())
        .bar_width(12);
    assert_eq!(
        render(&plain(40), &chart),
        "revenue ██████████│░ 270     \ncpu     ████████▋│░░ 72% high\n"
    );
    assert_eq!(measure(&plain(80), &chart), (4, 29));
    assert_eq!(render(&plain(40), &BulletChart::new()), "no data\n");
}

// --------------------------------------------------------------- matrices

#[test]
fn status_matrix_snapshots() {
    assert_eq!(
        render(&plain(40), &ci()),
        concat!(
            "             linux   macos  windows    \n",
            "unit           ✓       ✓       ✗       \n",
            "integration    ✓       ≈       ○       \n",
            "docs           ✓       ?               \n",
            "✓ pass 4  ✗ fail 1  ○ skip 1  ≈ flaky 1\n",
            "? pending 1                            \n",
        )
    );
    assert_eq!(
        render(&ascii(40), &ci()),
        concat!(
            "             linux   macos  windows    \n",
            "unit           +       +       X       \n",
            "integration    +       ~       -       \n",
            "docs           +       ?               \n",
            "+ pass 4  X fail 1  - skip 1  ~ flaky 1\n",
            "? pending 1                            \n",
        )
    );
    assert_eq!(
        ci().counts(),
        [
            ("pass", 4),
            ("fail", 1),
            ("skip", 1),
            ("flaky", 1),
            ("pending", 1)
        ]
        .map(|(n, c)| (n.to_string(), c))
    );
}

#[test]
fn status_matrix_colour_on_and_off() {
    let m = StatusMatrix::new()
        .columns(["a", "b"])
        .row("x", ["pass", "fail"]);
    assert_eq!(
        render(&color(40), &m),
        concat!(
            "  a b             \n",
            "x \x1b[32m✓\x1b[0m \x1b[1;31m✗\x1b[0m             \n",
            "\x1b[32m✓\x1b[0m pass 1  \x1b[1;31m✗\x1b[0m fail 1\n",
        )
    );
    // Symbols, not colours, tell the states apart.
    assert_eq!(
        render(&no_color(40), &m),
        "  a b             \nx ✓ ✗             \n✓ pass 1  ✗ fail 1\n"
    );
}

#[test]
fn status_matrix_own_states() {
    let m = StatusMatrix::new()
        .state(State::new("blocked", '⊘', '#', "bold magenta"))
        .state(State::new("pass", '●', 'o', "green"))
        .columns(["a", "b"])
        .row("x", ["pass", "blocked"]);
    assert_eq!(
        render(&plain(40), &m),
        "  a b                \nx ● ⊘                \n● pass 1  ⊘ blocked 1\n"
    );
    assert_eq!(
        render(&ascii(40), &m),
        "  a b                \nx o #                \no pass 1  # blocked 1\n"
    );
    // A wide symbol would break the grid: its ASCII form is used.
    assert_eq!(State::new("wide", '日', 'w', "red").symbol(false), 'w');
    assert_eq!(State::new("x", 'x', 'é', "red").symbol(true), '?');
}

#[test]
fn status_matrix_shrinks_headers_then_labels() {
    assert_eq!(
        render(&plain(24), &ci().legend(false)),
        concat!(
            "            li… ma… wi…\n",
            "unit         ✓   ✓   ✗ \n",
            "integration  ✓   ≈   ○ \n",
            "docs         ✓   ?     \n",
        )
    );
    // Too narrow for headers: they go; then the labels are cut.
    assert_eq!(
        render(&plain(12), &ci().legend(false)),
        "unit   ✓ ✓ ✗\ninteg… ✓ ≈ ○\ndocs   ✓ ?  \n"
    );
    // At most the legend on one line; it wraps when narrower.
    assert_eq!(measure(&plain(80), &ci()), (5, 52));
    assert_eq!(render(&plain(40), &StatusMatrix::new()), "no data\n");
}

// --------------------------------------------------------------- heatmaps

#[test]
fn heatmap_snapshots() {
    assert_eq!(
        render(&plain(40), &heat()),
        concat!(
            "    mon wed           \n",
            "api   ▒▒██░░▒▒        \n",
            "web   ▓▓██··░░        \n",
            "db  ░░░░▓▓░░          \n",
            "0 [ ░▒▓█] 9  · no data\n",
        )
    );
    // Ten shades in ASCII, so it reads in black and white.
    assert_eq!(
        render(&ascii(40), &heat()),
        concat!(
            "    mon wed                \n",
            "api ..==@@::++             \n",
            "web   **%%??--             \n",
            "db  ::::##--..             \n",
            "0 [ .:-=+*#%@] 9  ? no data\n",
        )
    );
    // Wider cells leave room for every header.
    assert_eq!(
        render(&plain(40), &heat().cell_width(4).legend(false)),
        concat!(
            "    mon tue wed thu fri \n",
            "api     ▒▒▒▒████░░░░▒▒▒▒\n",
            "web     ▓▓▓▓████····░░░░\n",
            "db  ░░░░░░░░▓▓▓▓░░░░    \n",
        )
    );
    assert_eq!(heat().scale().max(), 9.0);
}

#[test]
fn heatmap_colour_on_and_off() {
    let m = Heatmap::new().row("r", [0.0, 5.0, 10.0]).legend(false);
    assert_eq!(
        render(&color(20), &m),
        "r \x1b[34m  \x1b[0m\x1b[32m▒▒\x1b[0m\x1b[31m██\x1b[0m\n"
    );
    assert_eq!(render(&no_color(20), &m), "r   ▒▒██\n");
}

#[test]
fn heatmap_narrows_then_merges_columns() {
    assert_eq!(
        render(&plain(12), &heat().legend(false)),
        "    mon  \napi  ▒█░▒\nweb  ▓█·░\ndb  ░░▓░ \n"
    );
    // Fewer cells than columns: neighbours are merged by their mean.
    assert_eq!(
        render(&plain(7), &heat().legend(false)),
        "    mon\napi  ▓░\nweb  ▓░\ndb  ░▒░\n"
    );
    assert_eq!(measure(&plain(80), &heat()), (5, 22));
    assert_eq!(render(&plain(40), &Heatmap::new()), "no data\n");
}

#[test]
fn heatmap_range_clamps() {
    let m = Heatmap::new()
        .row("r", [-5.0, 50.0, 500.0])
        .range(0.0, 100.0)
        .charset(Charset::Ascii)
        .cell_width(1);
    assert_eq!(
        render(&plain(40), &m),
        "r  +@             \n0 [ .:-=+*#%@] 100\n"
    );
}

// ------------------------------------------------------------------ cards

#[test]
fn kpi_card_snapshots() {
    assert_eq!(
        render(&plain(40), &card()),
        concat!(
            "╭───────────────────────╮\n",
            "│ Requests         ✓ ok │\n",
            "│ 12.4k/s               │\n",
            "│ ▲ +8.49% vs last week │\n",
            "│ ▁▂▂▄▆▅▃▆█▇▅▃          │\n",
            "╰───────────────────────╯\n",
        )
    );
    assert_eq!(
        render(&ascii(40), &card()),
        concat!(
            "+-----------------------+\n",
            "| Requests         + ok |\n",
            "| 12.4k/s               |\n",
            "| ^ +8.49% vs last week |\n",
            "| _..:+=-+#*=-          |\n",
            "+-----------------------+\n",
        )
    );
    let braille = card().sparkline(Sparkline::new([1.0, 5.0, 3.0, 8.0]).charset(Charset::Braille));
    assert!(render(&plain(40), &braille).contains("│ ⣰⣼                    │"));
    assert_eq!(
        render(&plain(40), &card().border(false)),
        concat!(
            "Requests         ✓ ok\n",
            "12.4k/s              \n",
            "▲ +8.49% vs last week\n",
            "▁▂▂▄▆▅▃▆█▇▅▃         \n",
        )
    );
    assert_eq!(
        render(&plain(40), &KpiCard::new("Queue", 0.0).delta(0.0)),
        "╭───────╮\n│ Queue │\n│ 0     │\n│ = 0   │\n╰───────╯\n"
    );
}

#[test]
fn kpi_card_colour_on_and_off() {
    // Down is good news for errors: green, and the arrow says down.
    assert_eq!(
        render(&color(40), &errors()),
        concat!(
            "\x1b[90m╭──────────────────╮\x1b[0m\n",
            "\x1b[90m│ \x1b[0mErrors \x1b[33m! warning\x1b[0m\x1b[90m │\x1b[0m\n",
            "\x1b[90m│ \x1b[0m\x1b[1m42\x1b[0m              \x1b[90m │\x1b[0m\n",
            "\x1b[90m│ \x1b[0m\x1b[32m▼ -14\x1b[0m           \x1b[90m │\x1b[0m\n",
            "\x1b[90m╰──────────────────╯\x1b[0m\n",
        )
    );
    assert_eq!(
        render(&no_color(40), &errors()),
        concat!(
            "╭──────────────────╮\n",
            "│ Errors ! warning │\n",
            "│ 42               │\n",
            "│ ▼ -14            │\n",
            "╰──────────────────╯\n",
        )
    );
    // A rise in errors is bad news.
    let worse = KpiCard::new("Errors", 42.0)
        .delta(3.0)
        .higher_is_better(false)
        .border(false);
    assert!(render(&color(40), &worse).contains("\x1b[31m▲ +3\x1b[0m"));
}

#[test]
fn kpi_card_sizes() {
    assert_eq!(
        render(&plain(40), &card().width(18)),
        concat!(
            "╭────────────────╮\n",
            "│ Requests  ✓ ok │\n",
            "│ 12.4k/s        │\n",
            "│ ▲ +8.49% vs l… │\n",
            "│ ▁▂▂▄▆▅▃▆█▇▅▃   │\n",
            "╰────────────────╯\n",
        )
    );
    assert_eq!(
        render(
            &plain(30),
            &KpiCard::new("Uptime", 99.95).unit("%").expand(true)
        ),
        concat!(
            "╭────────────────────────────╮\n",
            "│ Uptime                     │\n",
            "│ 99.95%                     │\n",
            "╰────────────────────────────╯\n",
        )
    );
    // Below five cells the border goes; the sparkline resamples.
    assert_eq!(render(&plain(4), &card()), "✓ ok\n12.…\n▲ +…\n▂▅▆▅\n");
    assert_eq!(measure(&plain(80), &card()), (8, 25));
    assert_eq!(measure(&plain(80), &card().width(18)), (18, 18));
    assert_eq!(measure(&plain(30), &card().expand(true)), (8, 30));
    // The same parts make the same height, so a row of cards lines up.
    let lines = |c: &KpiCard| render(&plain(40), c).lines().count();
    assert_eq!(lines(&card()), lines(&card().caption("x").trend([1.0])));
    // Percentages and amounts.
    let pct = KpiCard::new("p", 50.0).delta_percent(-12.5).border(false);
    assert!(render(&plain(20), &pct).contains("▼ -12.5%"));
    let from_zero = KpiCard::new("p", 5.0).previous(0.0).border(false);
    assert!(render(&plain(20), &from_zero).contains("▲ +5"));
    let fixed = KpiCard::new("p", 2.0)
        .format(ValueFormat::Fixed(2))
        .border(false);
    assert!(render(&plain(20), &fixed).starts_with("p   \n2.00\n"));
}

// -------------------------------------------------------------- timelines

#[test]
fn timeline_snapshots() {
    assert_eq!(
        render(&plain(40), &build()),
        concat!(
            "fetch   ████ 4s                         \n",
            "compile     █████████████████ 22s       \n",
            "test              ███████████████ 18s   \n",
            "                        ████████████ 14s\n",
            "                                ship ◆  \n",
            "        ┬───────┬───────┬───────┬───────\n",
            "        0s     10s     20s     30s      \n",
        )
    );
    assert_eq!(
        render(&ascii(40), &build()),
        concat!(
            "fetch   #### 4s                         \n",
            "compile     ################# 22s       \n",
            "test              ############### 18s   \n",
            "                        ############ 14s\n",
            "                                ship *  \n",
            "        +-------+-------+-------+-------\n",
            "        0s     10s     20s     30s      \n",
        )
    );
    // A fixed range: the bounds label the ends.
    assert_eq!(
        render(&plain(40), &build().range(0.0, 60.0)),
        concat!(
            "fetch   ███ 4s                          \n",
            "compile    ███████████ 22s              \n",
            "test           █████████ 18s            \n",
            "                   ███████ 14s          \n",
            "                           ◆ ship       \n",
            "        ┬──────────────────────────────┬\n",
            "        0s                           60s\n",
        )
    );
    assert_eq!(build().rows(), ["fetch", "compile", "test"]);
}

#[test]
fn timeline_colour_on_and_off() {
    let t = Timeline::new()
        .span("a", 0.0, 2.0)
        .span("b", 1.0, 4.0)
        .milestone("go", 4.0);
    assert_eq!(
        render(&color(30), &t),
        concat!(
            "a \x1b[36m██████████████\x1b[0m 2            \n",
            "b        \x1b[35m████████████████████\x1b[0m \n",
            "                         go \x1b[1;33m◆\x1b[0m \n",
            "  \x1b[90m┬────────────┬────────────┬─\x1b[0m\n",
            "  0            2            4 \n",
        )
    );
    assert_eq!(
        render(&no_color(30), &t),
        concat!(
            "a ██████████████ 2            \n",
            "b        ████████████████████ \n",
            "                         go ◆ \n",
            "  ┬────────────┬────────────┬─\n",
            "  0            2            4 \n",
        )
    );
    let styled = Timeline::new().push(Span::new("a", 0.0, 1.0).style("bold red"));
    assert!(render(&color(20), &styled).contains("\x1b[1;31m"));
}

#[test]
fn timeline_stacks_overlaps_and_alternates_touching_ranges() {
    // `a` has back-to-back ranges: the second is `▓` so both show.
    let out = render(&plain(48), &jobs());
    assert!(out.starts_with("a ████████▓▓▓▓▓ 2s"), "{out}");
    let t = Timeline::new()
        .span("r", 0.0, 10.0)
        .span("r", 2.0, 8.0)
        .span("r", 4.0, 6.0)
        .durations(false);
    // Three overlapping ranges on one row: three lines, labelled once.
    assert_eq!(
        render(&plain(24), &t),
        concat!(
            "r ██████████████████████\n",
            "       ████████████     \n",
            "           ████         \n",
            "  ┬────────────────────┬\n",
            "  0                   10\n",
        )
    );
}

#[test]
fn timeline_compresses_idle_gaps() {
    assert_eq!(
        render(&plain(48), &jobs()),
        concat!(
            "a ████████▓▓▓▓▓ 2s   ██████████ 4s              \n",
            "b      ███████████ 4s     ███████████████████ 8s\n",
            "                                        deploy ◆\n",
            "  ┬──────────────┬ ≈ ┬─────────────────────────┬\n",
            "  0s            6s   600s                   611s\n",
        )
    );
    // Without compression the short ranges shrink to a cell.
    assert_eq!(
        render(&plain(48), &jobs().compress(false)),
        concat!(
            "a ▓ 2s                                      █ 4s\n",
            "b █ 4s                                      █ 8s\n",
            "                                      deploy ◆  \n",
            "  ┬──────┬──────┬──────┬──────┬──────┬──────┬───\n",
            "  0s   100s   200s   300s   400s   500s   600s  \n",
        )
    );
    // In ASCII the cut is `~`.
    assert!(render(&ascii(48), &jobs()).contains("+ ~ +"));
}

#[test]
fn timeline_narrow_and_empty() {
    assert_eq!(
        render(&plain(12), &build()),
        concat!(
            "fe… █ 4s    \n",
            "co…  ████   \n",
            "te…   ███   \n",
            "        ██  \n",
            "     ship ◆ \n",
            "    ┬───┬───\n",
            "    0s 25s  \n",
        )
    );
    assert_eq!(render(&plain(40), &Timeline::new()), "no data\n");
    assert_eq!(measure(&plain(40), &build()), (12, 40));
    assert_eq!(measure(&plain(80), &build().width(30)), (12, 30));
}

// ------------------------------------------------------------- containers

fn every() -> Vec<(&'static str, Arc<dyn Renderable + Send + Sync>)> {
    vec![
        ("gauge", Arc::new(cpu())),
        (
            "bullet",
            Arc::new(BulletChart::new().gauge(cpu()).bar_width(8)),
        ),
        ("matrix", Arc::new(ci())),
        ("heatmap", Arc::new(heat())),
        ("card", Arc::new(card())),
        ("timeline", Arc::new(build().width(30))),
    ]
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
fn renderables_sit_in_table_cells_and_panels() {
    for (name, chart) in every() {
        let mut table = Table::new();
        table.add_column("name");
        table.add_column("chart");
        table.add_row_cells(vec![
            Cell::Markup(name.into()),
            Cell::Renderable(chart.clone()),
        ]);
        for width in [12, 30, 80] {
            let out = render(&plain(width), &table);
            assert!(
                out.lines().all(|l| cell_len(l) <= width),
                "{name} table {width}:\n{out}"
            );
        }
        for width in [30, 80] {
            let out = render(&ascii(width), &table);
            assert!(out.is_ascii(), "{name} ascii table:\n{out}");
        }
        let panel = Panel::new(Box::new(ArcChart(chart.clone()))).title(name);
        for width in [8, 30, 80] {
            let out = render(&plain(width), &panel);
            assert!(
                out.lines().all(|l| cell_len(l) <= width),
                "{name} panel {width}:\n{out}"
            );
        }
    }
}

#[test]
fn cards_sit_side_by_side_in_columns() {
    let cards: Vec<Cell> = [card(), errors(), KpiCard::new("Latency", 182.0).unit(" ms")]
        .into_iter()
        .map(|c| Cell::Renderable(Arc::new(c.width(22))))
        .collect();
    let columns = Columns::from_cells(cards);
    let out = render(&plain(72), &columns);
    let lines: Vec<&str> = out.lines().collect();
    // Three cards across, each 22 cells wide.
    assert!(lines[0].starts_with("╭────────────────────╮ ╭"), "{out}");
    assert_eq!(lines[0].matches('╭').count(), 3, "{out}");
    assert!(out.lines().all(|l| cell_len(l) <= 72));
    let out = render(&plain(30), &columns);
    assert_eq!(out.lines().next().unwrap().matches('╭').count(), 1, "{out}");
}

#[test]
fn dashboard_in_a_layout_updates_in_live() {
    let frame = |requests: f64| {
        let mut layout = Layout::new();
        let mut cards = Layout::new().size(6);
        cards.split_row(vec![
            Layout::with_renderable(Box::new(card().expand(true))),
            Layout::with_renderable(Box::new(KpiCard::new("Requests", requests).expand(true))),
        ]);
        layout.split_column(vec![
            cards,
            Layout::with_renderable(Box::new(ci().legend(false))).size(4),
            Layout::with_renderable(Box::new(build())),
        ]);
        layout
    };
    let console = Console::builder()
        .width(60)
        .height(18)
        .force_terminal(true)
        .color_system(None)
        .build();
    let out = render(&console, &frame(1.0));
    assert!(out.lines().all(|l| cell_len(l) <= 60), "{out}");
    assert!(out.contains("integration"), "{out}");
    assert!(out.contains("compile"), "{out}");
    let mut buffer = Vec::new();
    let mut live = Live::new(Box::new(frame(1.0)), console, &mut buffer);
    live.start();
    live.update(Box::new(frame(2.0)));
    live.update(Box::new(frame(3_500.0)));
    live.stop();
    drop(live);
    let out = String::from_utf8(buffer).unwrap();
    assert!(out.contains("│ 2 "), "{out:?}");
    assert!(out.contains("│ 3.5k "), "{out:?}");
}

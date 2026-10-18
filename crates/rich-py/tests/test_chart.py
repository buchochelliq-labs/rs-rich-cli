"""``rs_rich.chart``: ``rich_ext::chart`` from Python (0.0.15 workstream 5).

Rich has no charts, so each expected drawing is what the Rust crate draws for
the same chart: the cases are ``rich_ext::chart``'s own doc examples, which
its doc tests assert byte for byte. A chart leaves its last newline to
``Console.print``, like rich's renderables (see ``drawn``).
"""

from __future__ import annotations

import io

import pytest

from rs_rich import _native, chart
from rs_rich.console import Console
from rs_rich.panel import Panel
from rs_rich.table import Table


def drawn(renderable, width: int = 40) -> str:
    """What `print` writes for `renderable` at `width` without colour: the
    chart's lines and the one newline `print` ends them with (the chart
    adds none of its own, so no blank line follows it)."""
    out = io.StringIO()
    Console(file=out, width=width, color_system=None).print(renderable)
    text = out.getvalue()
    assert not text.endswith("\n\n"), repr(text)
    return text


def coloured(renderable, width: int = 40) -> str:
    out = io.StringIO()
    console = Console(file=out, width=width, force_terminal=True, color_system="standard", no_color=False)
    console.print(renderable)
    return out.getvalue()


def test_the_module_exports_native_objects_and_the_rust_names():
    for name in chart.__all__:
        assert getattr(chart, name) is getattr(_native, name)
    assert chart.Bar is chart.ChartBar
    assert chart.State is chart.ChartState
    assert chart.Span is chart.TimelineSpan
    assert ("chart.bar", "cyan") in chart.STYLES


def test_sparkline():
    assert drawn(chart.Sparkline([1.0, 3.0, 2.0, 8.0, 5.0])) == "▁▃▂█▅\n"
    assert chart.Sparkline((1, 2)).values == [1.0, 2.0]
    # A gap is None; it is skipped, not refused.
    assert "▁" in drawn(chart.Sparkline([1, None, 3]))
    assert drawn(chart.Sparkline([1, 3, 2, 8, 5], charset="ascii")).strip() != ""


def test_bar_chart_from_pairs_a_mapping_or_bars():
    expected = "api    ████████   31\nweb    ███▎     12.5\nworker █           4\n"
    pairs = [("api", 31.0), ("web", 12.5), ("worker", 4.0)]
    assert drawn(chart.BarChart(pairs, bar_width=8)) == expected
    assert drawn(chart.BarChart(dict(pairs), bar_width=8)) == expected
    bars = [chart.ChartBar(label, value) for label, value in pairs]
    assert drawn(chart.BarChart(bars, bar_width=8)) == expected
    assert drawn(chart.BarChart(pairs, bar_width=8, charset="ascii")) == (
        "api    ########   31\nweb    ###      12.5\nworker #           4\n"
    )
    assert len(chart.BarChart(pairs)) == 3


def test_vertical_bars():
    bars = chart.BarChart([("mon", 4.0), ("tue", 7.5), ("wed", -2.0)], orientation="vertical", bar_width=4)
    assert drawn(bars) == (
        "    7.5    \n"
        " 4  ███    \n"
        "▅▅▅ ███    \n"
        "███ ███    \n"
        "        ███\n"
        "        -2 \n"
        "mon tue wed\n"
    )


def test_sparklines_after_ratatui():
    assert drawn(chart.Sparkline([1, 4, 7, 10], rows=3)) == "   █\n ▁██\n▁███\n"
    assert drawn(chart.Sparkline([1, 4, 7, 10], direction="right_to_left"), 10) == "      █▆▃▁\n"
    assert drawn(chart.Sparkline([1, None, 3], absent="·"), 10) == "▁·█\n"
    assert drawn(chart.Sparkline([1, None, 3], absent=("x", "red")), 10) == "▁x█\n"
    assert drawn(chart.Sparkline([0, 1, 2, 3, 4], levels="three"), 10) == " ▄▄▄█\n"
    # One style a value; None keeps the sparkline's own (`chart.bar`, cyan).
    assert coloured(chart.Sparkline([1, 2], styles=["red", None]), 10) == "\x1b[31m▁\x1b[0m\x1b[36m█\x1b[0m\n"
    with pytest.raises(ValueError, match="rows"):
        chart.Sparkline([1], rows=0)
    with pytest.raises(ValueError, match="direction"):
        chart.Sparkline([1], direction="up")
    with pytest.raises(ValueError, match="levels"):
        chart.Sparkline([1], levels="two")


def test_grouped_bars():
    q1 = chart.BarGroup("Q1", [chart.ChartBar("a", 4.0), chart.ChartBar("b", 2.0)])
    q2 = chart.BarGroup("Q2", [("a", 3.0), ("b", 1.0)])
    assert (q1.label, len(q1)) == ("Q1", 2)
    assert repr(q2) == "<BarGroup 'Q2' with 2 bars>"
    assert drawn(chart.BarChart(groups=[q1, q2], bar_width=8), 30) == (
        "a ████████ 4\n"
        "b ████     2\n"
        "  Q1        \n"
        "            \n"
        "a ██████   3\n"
        "b ██       1\n"
        "  Q2        \n"
    )
    vertical = chart.BarChart(groups=[q1, q2], orientation="vertical", bar_width=4, thickness=2)
    assert drawn(vertical, 30) == (
        "4           \n"
        "██     3    \n"
        "██ 2   ██   \n"
        "██ ██  ██ 1 \n"
        "██ ██  ██ ██\n"
        "a  b   a  b \n"
        " Q1     Q2  \n"
    )
    with pytest.raises(TypeError, match="BarGroup"):
        chart.BarChart(groups=[1])


def test_thick_bars_text_values_and_values_inside():
    assert drawn(chart.BarChart([("a", 4.0), ("b", 2.0)], bar_width=8, thickness=3, bar_gap=1), 30) == (
        "  ████████  \n"
        "a ████████ 4\n"
        "  ████████  \n"
        "            \n"
        "  ████      \n"
        "b ████     2\n"
        "  ████      \n"
    )
    text = chart.BarChart([chart.ChartBar("a", 4.0, text="four"), chart.ChartBar("b", 2.0)], bar_width=8)
    assert drawn(text, 30) == "a ████████ four\nb ████        2\n"
    assert chart.ChartBar("a", 1.0, text="x").text == "x"
    assert chart.ChartBar("a", 1.0).text is None
    inside = chart.BarChart([("spring", 4.0), ("summer", 2.0)], bar_width=10, values_inside=True)
    assert drawn(inside, 30) == "spring 4█████████\nsummer 2████     \n"
    styled = chart.BarChart([chart.ChartBar("a", 4.0, value_style="red", label_style="blue")], bar_width=4)
    assert coloured(styled, 20) == "\x1b[34ma\x1b[0m \x1b[36m██\x1b[0m   \x1b[31m4\x1b[0m\n"
    with pytest.raises(ValueError, match="thickness"):
        chart.BarChart([("a", 1)], thickness=0)


def test_fill_gauge():
    assert drawn(chart.FillGauge(0.42, height=3), 20) == (
        "████████            \n████████42%         \n████████            \n"
    )
    assert drawn(chart.FillGauge(0.42, label="42.00%", unicode=True), 20) == "███████42.00%       \n"
    # Where the bar runs under the label, the label's colours swap.
    assert coloured(chart.FillGauge(0.5, style="green"), 10) == (
        "\x1b[32m███\x1b[0m\x1b[39;42m50\x1b[0m\x1b[32m%    \x1b[0m\n"
    )
    assert (chart.FillGauge(7).ratio, chart.FillGauge(-1).ratio) == (1.0, 0.0)
    with pytest.raises(ValueError, match="NaN"):
        chart.FillGauge(float("nan"))


def test_line_gauge():
    assert drawn(chart.LineGauge(0.42, label="Downloading: 42%", line="thick"), 30) == (
        "Downloading: 42% ━━━━━━━━━━━━━\n"
    )
    assert drawn(chart.LineGauge(0.5, width=12)) == " 50% ───────\n"
    assert drawn(chart.LineGauge(0.99, label="cpu", symbols=("▰", "▱")), 20) == "cpu ▰▰▰▰▰▰▰▰▰▰▰▰▰▰▰▱\n"
    assert coloured(chart.LineGauge(0.5, filled_style="red", unfilled_style="blue", width=10), 10) == (
        " 50% \x1b[31m──\x1b[0m\x1b[34m───\x1b[0m\n"
    )
    assert chart.LineGauge(0.25).ratio == 0.25
    with pytest.raises(ValueError, match="line"):
        chart.LineGauge(0.5, line="dotted")
    with pytest.raises(ValueError, match="one character"):
        chart.LineGauge(0.5, symbols=("ab", "c"))


def test_histogram():
    latencies = [12.0, 14.0, 15.0, 21.0, 22.0, 23.0, 24.0, 38.0]
    histogram = chart.Histogram(latencies, bins=3, range=(10.0, 40.0), bar_width=8)
    assert drawn(histogram) == "[10, 20) ██████   3\n[20, 30) ████████ 4\n[30, 40] ██       1\n"
    assert histogram.counts() == [3, 4, 1]
    assert histogram.edges() == [10.0, 20.0, 30.0, 40.0]
    with pytest.raises(ValueError):
        chart.Histogram([1.0], bins=0)


def test_line_chart_in_ascii():
    line = chart.LineChart([chart.Series("load", [1.0, 3.0, 2.0, 5.0, 4.0])], height=6, charset="ascii")
    assert drawn(line, width=24) == (
        "6 +                     \n"
        "5 +               ***   \n"
        "4 +             **   ***\n"
        "3 +    ****   **        \n"
        "2 +  **    ***          \n"
        "1 +**                   \n"
        "  ++----+----+----+----+\n"
        "   0    1    2    3    4\n"
        "* load                  \n"
    )


def test_series_take_values_or_points():
    assert chart.Series("a", [1, 2]).points == [(0.0, 1.0), (1.0, 2.0)]
    # A generator is read once: the item looked at to tell values from
    # points is kept.
    assert chart.Series("g", (v for v in [1, 2])).points == [(0.0, 1.0), (1.0, 2.0)]
    assert chart.Series("p", iter([(0, 1)])).points == [(0.0, 1.0)]
    scatter = chart.Series("b", [(0, 1), (2, 3)], kind="scatter", marker="x")
    assert scatter.points == [(0.0, 1.0), (2.0, 3.0)]
    assert scatter.kind == "scatter"
    assert scatter.name == "b"
    with pytest.raises(ValueError, match="series kind"):
        chart.Series("c", [1], kind="area")
    with pytest.raises(TypeError, match="LineChart takes Series"):
        chart.LineChart([[1, 2]])


def test_gauge_and_bullet_chart():
    bands = [chart.Band(60.0, "ok"), chart.Band(85.0, "high"), chart.Band(100.0, "critical")]
    gauge = chart.Gauge("cpu", 72.0, range=(0.0, 100.0), target=80.0, bands=bands, unit="%", bar_width=20)
    assert drawn(gauge) == "cpu ██████████████▍▒│░░░ 72% high\n"
    assert gauge.band == "high"
    assert gauge.value == 72.0
    ascii = chart.Gauge(
        "cpu", 72.0, range=(0.0, 100.0), target=80.0, bands=bands, unit="%", bar_width=20, charset="ascii"
    )
    assert drawn(ascii) == "cpu ##############=:|... 72% high\n"
    bullet = chart.BulletChart(
        [
            chart.Gauge("revenue", 270.0, range=(0.0, 300.0), target=250.0),
            chart.Gauge("profit", 22.5, range=(0.0, 30.0), target=26.0),
        ],
        bar_width=12,
    )
    assert drawn(bullet) == "revenue ██████████│░  270\nprofit  █████████░│░ 22.5\n"
    assert len(bullet) == 2


def test_heatmap():
    heatmap = chart.Heatmap(
        [("api", [1.0, 4.0, 9.0, 2.0]), ("web", [0.0, 6.0, 8.0, None])],
        columns=["mon", "tue", "wed", "thu"],
        cell_width=4,
        charset="ascii",
    )
    assert drawn(heatmap) == (
        "    mon tue wed thu        \n"
        "api ....====@@@@::::       \n"
        "web     ****%%%%????       \n"
        "0 [ .:-=+*#%@] 9  ? no data\n"
    )
    assert heatmap.scale == (0.0, 9.0)


def test_status_matrix():
    matrix = chart.StatusMatrix(
        {"unit": ["pass", "pass", "fail"], "e2e": ["pass", "flaky", "skip"]},
        columns=["linux", "macos", "win"],
    )
    assert drawn(matrix) == (
        "     linux macos  win                  \n"
        "unit   ✓     ✓     ✗                   \n"
        "e2e    ✓     ≈     ○                   \n"
        "✓ pass 3  ✗ fail 1  ○ skip 1  ≈ flaky 1\n"
    )
    assert dict(matrix.counts()) == {"pass": 3, "fail": 1, "skip": 1, "flaky": 1}
    blocked = chart.ChartState("blocked", "⊘", "#", "bold magenta")
    assert (blocked.symbol, blocked.ascii) == ("⊘", "#")
    assert chart.ChartState.pass_().name == "pass"
    custom = chart.StatusMatrix([("ci", ["blocked"])], states=[blocked])
    assert "⊘ blocked 1" in drawn(custom)


def test_kpi_card():
    card = chart.KpiCard("Requests", 1234, delta_percent=4.2, trend=[1, 2, 3, 2, 5], status="ok", width=24)
    assert drawn(card) == (
        "╭──────────────────────╮\n"
        "│ Requests        ✓ ok │\n"
        "│ 1.2k                 │\n"
        "│ ▲ +4.2%              │\n"
        "│ ▁▃▅▃█                │\n"
        "╰──────────────────────╯\n"
    )
    with pytest.raises(ValueError, match="at most one"):
        chart.KpiCard("x", 1, delta=1, previous=2)
    with pytest.raises(ValueError, match="status"):
        chart.KpiCard("x", 1, status="green")


def test_timeline():
    build = chart.Timeline(
        [("fetch", 0.0, 4.0), ("compile", 4.0, 26.0), chart.TimelineSpan("test", 12.0, 30.0), ("test", 20.0, 34.0)],
        milestones=[("ship", 36.0)],
        unit="s",
        width=40,
    )
    assert drawn(build) == (
        "fetch   ████ 4s                         \n"
        "compile     █████████████████ 22s       \n"
        "test              ███████████████ 18s   \n"
        "                        ████████████ 14s\n"
        "                                ship ◆  \n"
        "        ┬───────┬───────┬───────┬───────\n"
        "        0s     10s     20s     30s      \n"
    )
    assert build.rows == ["fetch", "compile", "test"]


def test_formats_and_bad_arguments():
    assert chart.chart_format(1234.0) == "1.2k"
    assert chart.chart_format(2.0, 1) == "2.0"
    assert chart.chart_format(2.0, "compact") == "2"
    with pytest.raises(ValueError, match="charset"):
        chart.Sparkline([1], charset="emoji")
    with pytest.raises(ValueError, match="format"):
        chart.Sparkline([1], format="long")
    with pytest.raises(TypeError, match="string"):
        chart.Sparkline(["1"])
    with pytest.raises(TypeError, match="label, value"):
        chart.BarChart([1, 2])


def test_fixed_decimals_out_of_range_are_refused():
    assert chart.chart_format(1.5, 17) == "1.50000000000000000"
    for decimals in (18, 70000, 10**6, -1):
        with pytest.raises(ValueError, match="decimals"):
            chart.chart_format(1.5, decimals)
    with pytest.raises(ValueError, match="decimals"):
        chart.Gauge("a", 1, format=70000)


def test_any_iterable_where_a_list_is_taken():
    gauges = chart.BulletChart(g for g in [chart.Gauge("a", 1), chart.Gauge("b", 2)])
    assert len(gauges) == 2
    banded = chart.Gauge("a", 1, range=(0, 2), bands=(b for b in [chart.Band(1, "low"), chart.Band(2, "high")]))
    assert banded.band == "low"
    timeline = chart.Timeline([("a", 0, 1)], milestones=(m for m in [("x", 1)]))
    assert "x" in drawn(timeline)
    matrix = chart.StatusMatrix(
        [("api", (s for s in ["pass", "flaky"]))],
        columns=(c for c in ["mon", "tue"]),
        states=(s for s in [chart.ChartState("flaky", "~", "~", "yellow")]),
    )
    assert dict(matrix.counts())["flaky"] == 1
    heat = chart.Heatmap({"a": [1, 2]}, columns=iter(["x", "y"]))
    assert "x" in drawn(heat)
    with pytest.raises(TypeError, match="columns"):
        chart.Heatmap({"a": [1]}, columns="ab")
    with pytest.raises(TypeError, match="Gauge"):
        chart.BulletChart([1])


def test_series_points_are_any_pair():
    for points in ([[0, 1], [1, 2]], [(0, 1), (1, 2)], ((x, y) for x, y in [(0, 1), (1, 2)])):
        assert chart.Series("a", points).points == [(0.0, 1.0), (1.0, 2.0)]
    with pytest.raises(TypeError, match="pair"):
        chart.Series("a", [(0, 1), [1, 2, 3]])


def test_a_timeline_none_is_a_gap():
    with_gap = chart.Timeline([("a", None, 1), ("b", 0, 2)], milestones=[("m", None)], width=30)
    assert with_gap.rows == ["a", "b"]
    out = drawn(with_gap, 30)
    lines = out.splitlines()
    assert lines[0].strip() == "a" and lines[1].startswith("b █"), out
    assert "◆" not in out, out
    with pytest.raises(TypeError, match="row, start, end"):
        chart.Timeline([("a", 1)])


def test_charts_compose_with_tables_and_panels():
    table = Table.grid(padding=(0, 1))
    table.add_column()
    table.add_column()
    table.add_row("api", chart.Sparkline([1, 3, 2, 8, 5]))
    out = io.StringIO()
    Console(file=out, width=40, color_system=None).print(Panel(table, title="load"))
    assert "api ▁▃▂█▅" in out.getvalue()


def test_colour_comes_from_the_chart_theme():
    # `chart.bar` is cyan.
    assert "\x1b[36m" in coloured(chart.BarChart([("a", 1.0)], bar_width=4))


@pytest.mark.parametrize("width", [1, 2, 5, 13, 40])
def test_no_chart_draws_past_its_width(width):
    for renderable in [
        chart.Sparkline(range(30)),
        chart.BarChart([("label", 10.0), ("other", 3.0)]),
        chart.LineChart([chart.Series("s", [1, 5, 2])]),
        chart.Heatmap([("r", [1, 2, 3])], columns=["a", "b", "c"]),
        chart.KpiCard("k", 1.0, trend=[1, 2]),
        chart.Timeline([("t", 0, 10)]),
        chart.Sparkline(range(30), rows=3, direction="right_to_left"),
        chart.BarChart(groups=[chart.BarGroup("g", [("a", 1.0), ("b", 2.0)])], thickness=2, values_inside=True),
        chart.BarChart([("a", 1.0), ("b", 2.0)], orientation="vertical", thickness=3),
        chart.FillGauge(0.5, label="half way there"),
        chart.LineGauge(0.5, label="half way there"),
    ]:
        out = io.StringIO()
        Console(file=out, width=width, color_system=None).print(renderable)
        assert all(len(line) <= width for line in out.getvalue().splitlines()), (renderable, width)


# ---------------------------------------------------------------------------
# Charts that read aloud, distributions and small multiples (0.0.19, #726)


def test_every_chart_describes_itself():
    gauge = chart.Gauge("cpu", 72, range=(0, 100), target=80, unit="%")
    assert chart.describe(gauge) == "Gauge cpu: 72%, 72 percent of 0% to 100%, target 80%"
    assert chart.describe(chart.Sparkline([2, 4, 3, 9, 6, 5])) == (
        "Sparkline, 6 values: min 2, max 9, last 5, trend up"
    )
    assert chart.describe(chart.BarChart([("api", 31.0), ("web", 12.5)])) == (
        "Bar chart, 2 bars, values from 12.5 to 31\napi 31, web 12.5"
    )
    # rs_rich.ext.a11y reads charts the same way.
    from rs_rich.ext import a11y

    assert a11y.accessible_text(gauge) == chart.describe(gauge)
    with pytest.raises(TypeError):
        chart.describe("not a chart")


def test_quantiles_and_summaries_are_numpys_linear():
    assert chart.quantile([3, 1, 2, 4], 0.5) == 2.5
    assert chart.quantile([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 0.25) == 3.25
    assert chart.quantile([], 0.5) is None
    s = chart.summary([1, 2, 3, 4, None, float("nan")])
    assert (s["count"], s["ignored"], s["p50"], s["mean"]) == (4, 2, 2.5, 2.5)
    assert chart.summary([])["min"] is None


def test_a_distribution_draws_a_strip_and_its_numbers():
    d = chart.Distribution(range(1, 21), label="x", strip_width=12)
    assert drawn(d, 80) == (
        "x ├──▒▒▒█▒▒▒─┤ n 20 min 1 p25 5.75 p50 10.5 p75 15.25 p95 19.05 max 20 mean 10.5\n"
    )
    assert drawn(chart.Distribution(range(1, 21), strip_width=12, numbers=False, charset="ascii")) == (
        "|--===#===-|\n"
    )
    assert d.summary["p25"] == 5.75
    assert d.quantile(0.5) == 10.5
    assert d.values[:3] == [1.0, 2.0, 3.0]
    assert chart.describe(d) == (
        "Distribution x: 20 values, min 1, p25 5.75, median 10.5, p75 15.25, p95 19.05, max 20, mean 10.5"
    )


def test_a_distribution_set_shares_its_scale():
    s = chart.DistributionSet(
        {"a": [0, 2, 4, 6, 8], "b": [4, 5, 6]}, strip_width=9, numbers=False, charset="ascii"
    )
    assert drawn(s) == "a |-==#==-|\nb     |#=  \n  0       8\n"
    assert s.scale == (0.0, 8.0)
    assert s.labels == ["a", "b"]
    assert chart.describe(s).splitlines()[0] == "Distributions, 2 on a shared scale from 0 to 8"


def test_small_multiples_share_a_scale_and_fit_the_width():
    grid = chart.SmallMultiples(
        [("low", chart.Sparkline([1, 2, 3])), ("high", chart.Sparkline([7, 8, 9]))],
        min_cell_width=10,
    )
    assert drawn(grid, 30) == (
        "low             high          \n"
        "▁▂▃             ▆▇█           \n"
        "scale 1 to 9 in every chart   \n"
    )
    assert grid.scale == (1.0, 9.0)
    assert grid.columns_at(30) == 2 and grid.columns_at(10) == 1
    assert grid.titles == ["low", "high"]
    assert chart.describe(grid).splitlines()[1] == "low: Sparkline, 3 values: min 1, max 3, last 3, trend up"
    with pytest.raises(TypeError):
        chart.SmallMultiples([("x", chart.Gauge("g", 1))])

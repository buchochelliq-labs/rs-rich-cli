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
    ]:
        out = io.StringIO()
        Console(file=out, width=width, color_system=None).print(renderable)
        assert all(len(line) <= width for line in out.getvalue().splitlines()), (renderable, width)

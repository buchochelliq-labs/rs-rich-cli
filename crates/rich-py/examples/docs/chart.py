"""The pictures on docs/python/chart.md: run it, or scripts/capture_python_docs.py."""

from _shots import Shots

from rs_rich import chart


def bars(console):
    # --8<-- [start:bars]
    console.print(chart.Sparkline([12, 15, 14, 18, 25, 31, 28, 22, 35, 41]))
    console.print(chart.Sparkline([12, 15, 14, 18, 25, 31, 28, 22, 35, 41], min_max=True, threshold=30))
    console.print(chart.BarChart({"api": 412, "web": 268.5, "worker": 97}, bar_width=20))
    console.print(chart.Histogram([12, 14, 15, 21, 22, 23, 24, 38], bins=3, range=(10, 40), bar_width=8))
    # --8<-- [end:bars]


def ratatui_bars(console):
    # --8<-- [start:ratatui-bars]
    console.print(chart.Sparkline([3, 5, 2, 8, 6, 9, 4, 7], rows=3))
    console.print(chart.Sparkline([3, None, 2, 8, 6, None, 4, 7], direction="right_to_left", absent="·"))
    console.print(chart.BarChart(groups=[
        chart.BarGroup("Q1", [("north", 31), chart.ChartBar("south", 18, text="18 (low)")]),
        chart.BarGroup("Q2", [("north", 27), ("south", 24)]),
    ], bar_width=16))
    # --8<-- [end:ratatui-bars]


def ratatui_gauges(console):
    # --8<-- [start:ratatui-gauges]
    console.print(chart.FillGauge(0.42, height=3, style="green"))
    console.print(chart.LineGauge(0.42, label="Downloading", line="thick", filled_style="cyan"))
    console.print(chart.LineGauge(0.8, label="cpu", symbols=("▰", "▱")))
    # --8<-- [end:ratatui-gauges]


def line(console):
    # --8<-- [start:line]
    load = chart.Series("load", [1, 3, 2, 5, 4])
    console.print(chart.LineChart([load], height=6, charset="ascii"), width=24)
    # --8<-- [end:line]


def gauges(console):
    # --8<-- [start:gauges]
    bands = [chart.Band(60, "ok"), chart.Band(85, "high"), chart.Band(100, "critical")]
    cpu = chart.Gauge("cpu", 72, range=(0, 100), target=80, bands=bands, unit="%", bar_width=20)
    console.print(cpu)
    print(cpu.band)
    console.print(chart.BulletChart([
        chart.Gauge("revenue", 270, range=(0, 300), target=250),
        chart.Gauge("profit", 22.5, range=(0, 30), target=26),
    ], bar_width=12))
    # --8<-- [end:gauges]


def heatmap(console):
    # --8<-- [start:heatmap]
    console.print(chart.Heatmap(
        {"api": [1, 4, 9, 2], "web": [0, 6, 8, None]},
        columns=["mon", "tue", "wed", "thu"],
        cell_width=4,
        charset="ascii",
    ))
    blocked = chart.ChartState("blocked", "⊘", "#", "bold magenta")
    console.print(chart.StatusMatrix(
        {"unit": ["pass", "pass", "fail"], "e2e": ["pass", "blocked", "skip"]},
        columns=["linux", "macos", "win"],
        states=[blocked],
    ))
    # --8<-- [end:heatmap]


def kpi(console):
    # --8<-- [start:kpi]
    console.print(chart.KpiCard("Requests", 1234, delta_percent=4.2, trend=[1, 2, 3, 2, 5], status="ok", width=24))
    # --8<-- [end:kpi]


def timeline(console):
    # --8<-- [start:timeline]
    console.print(chart.Timeline(
        [("fetch", 0, 4), ("compile", 4, 26), ("test", 12, 30), ("test", 20, 34)],
        milestones=[("ship", 36)],
        unit="s",
        width=40,
    ))
    # --8<-- [end:timeline]


def distribution(console):
    # --8<-- [start:distribution]
    latency = [12, 15, 14, 18, 25, 31, 28, 22, 35, 41, 38, 30, 26, 33, 47, 52, 140, None]
    console.print(chart.Distribution(latency, label="latency", unit="ms", format=0))
    console.print(chart.DistributionSet(
        {"api": latency, "web": [v * 0.6 for v in latency[:-1]], "worker": [v * 1.5 for v in latency[:-1]]},
        strip_width=24,
        numbers=False,
    ))
    print(round(chart.quantile(latency, 0.95), 2), chart.summary(latency)["ignored"])
    # --8<-- [end:distribution]


def multiples(console):
    # --8<-- [start:multiples]
    requests = [12, 15, 14, 18, 25, 31, 28, 22, 35, 41, 38, 30, 26, 33, 47, 52]
    services = {"api": 1.0, "web": 0.6, "worker": 0.3, "cron": 0.1}
    grid = chart.SmallMultiples(
        {name: chart.Sparkline([v * share for v in requests]) for name, share in services.items()},
        min_cell_width=18,
    )
    console.print(grid)
    print(chart.describe(grid))
    # --8<-- [end:multiples]


shots = Shots("chart")
shots.shot("bars", bars, width=40)
shots.shot("ratatui-bars", ratatui_bars, width=40)
shots.shot("line", line, width=80)
shots.shot("gauges", gauges, width=80)
shots.shot("ratatui-gauges", ratatui_gauges, width=40)
shots.shot("heatmap", heatmap, width=80)
shots.shot("kpi", kpi, width=80)
shots.shot("timeline", timeline, width=80)
shots.shot("distribution", distribution, width=100)
shots.shot("multiples", multiples, width=80)
shots.finish()

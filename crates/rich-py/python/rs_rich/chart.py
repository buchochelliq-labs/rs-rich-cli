"""``rs_rich.chart``: charts drawn with text (``rich_ext::chart``; no Rich counterpart).

Sparklines, bars and histograms, Braille line and scatter charts, gauges and
bullet charts, heatmaps, status matrices, KPI cards and timelines, ratatui's
fill and line gauges, distributions and small multiples. Each is a
renderable: print it, or put it in a ``Table`` cell, a ``Panel`` or a
``Live`` display. None writes a line wider than the width it gets, and none
needs colour to be read. ``describe(chart)`` gives what it says to a screen
reader.

Settings are keyword arguments; ``charset`` is ``"auto"``, ``"blocks"``,
``"braille"`` or ``"ascii"``, and ``format`` is ``"compact"`` (``1.2k``) or a
number of decimals. ``None`` in a list of values is a gap.
"""

from ._native import (
    CHART_STYLES,
    Band,
    BarChart,
    BarGroup,
    BulletChart,
    ChartBar,
    ChartState,
    Distribution,
    DistributionSet,
    FillGauge,
    Gauge,
    Heatmap,
    Histogram,
    KpiCard,
    LineChart,
    LineGauge,
    Series,
    SmallMultiples,
    Sparkline,
    StatusMatrix,
    Timeline,
    TimelineSpan,
    chart_describe,
    chart_format,
    chart_quantile,
    chart_summary,
)

# The Rust names, where the flat native module needed a longer one.
Bar = ChartBar
State = ChartState
Span = TimelineSpan
STYLES = CHART_STYLES
format_value = chart_format
quantile = chart_quantile
summary = chart_summary
describe = chart_describe

__all__ = [
    "Band",
    "BarChart",
    "BarGroup",
    "BulletChart",
    "CHART_STYLES",
    "ChartBar",
    "ChartState",
    "Distribution",
    "DistributionSet",
    "FillGauge",
    "Gauge",
    "Heatmap",
    "Histogram",
    "KpiCard",
    "LineChart",
    "LineGauge",
    "Series",
    "SmallMultiples",
    "Sparkline",
    "StatusMatrix",
    "Timeline",
    "TimelineSpan",
    "chart_describe",
    "chart_format",
    "chart_quantile",
    "chart_summary",
]

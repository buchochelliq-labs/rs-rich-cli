"""``rs_rich.chart``: charts drawn with text (``rich_ext::chart``; no Rich counterpart).

Sparklines, bars and histograms, Braille line and scatter charts, gauges and
bullet charts, heatmaps, status matrices, KPI cards and timelines. Each is a
renderable: print it, or put it in a ``Table`` cell, a ``Panel`` or a
``Live`` display. None writes a line wider than the width it gets, and none
needs colour to be read.

Settings are keyword arguments; ``charset`` is ``"auto"``, ``"blocks"``,
``"braille"`` or ``"ascii"``, and ``format`` is ``"compact"`` (``1.2k``) or a
number of decimals. ``None`` in a list of values is a gap.
"""

from ._native import (
    CHART_STYLES,
    Band,
    BarChart,
    BulletChart,
    ChartBar,
    ChartState,
    Gauge,
    Heatmap,
    Histogram,
    KpiCard,
    LineChart,
    Series,
    Sparkline,
    StatusMatrix,
    Timeline,
    TimelineSpan,
    chart_format,
)

# The Rust names, where the flat native module needed a longer one.
Bar = ChartBar
State = ChartState
Span = TimelineSpan
STYLES = CHART_STYLES
format_value = chart_format

__all__ = [
    "Band",
    "BarChart",
    "BulletChart",
    "CHART_STYLES",
    "ChartBar",
    "ChartState",
    "Gauge",
    "Heatmap",
    "Histogram",
    "KpiCard",
    "LineChart",
    "Series",
    "Sparkline",
    "StatusMatrix",
    "Timeline",
    "TimelineSpan",
    "chart_format",
]

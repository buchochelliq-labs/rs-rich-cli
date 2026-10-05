# Charts

```python
from rs_rich import chart
```

`rs_rich.chart` is `rich_ext::chart` from Python: charts drawn with text.
Rich has no counterpart. Every chart is a renderable, so it prints, sits in a
`Table` cell or a `Panel`, and updates in a `Live` display. None writes a line
wider than the width it gets: at small widths it drops labels, values and
axes before it drops data. None needs colour to be read: lengths and heights
carry the values, numbers are written out, and series have their own
markers.

| Class | Draws |
|---|---|
| `Sparkline` | one line, one cell per value (two in Braille) |
| `BarChart`, `ChartBar` | a label, a bar and a value per row, or columns side by side |
| `Histogram` | raw values counted into bins, drawn as bars |
| `LineChart`, `Series` | one or more series as lines or points, with axes and a legend |
| `Gauge`, `Band`, `BulletChart` | a value against a range, with a target and threshold bands; several aligned |
| `Heatmap` | a labelled grid of values as shades, with a legend |
| `StatusMatrix`, `ChartState` | rows and columns of states (pass, fail, skip, flaky, your own) |
| `KpiCard` | a label, a value, a delta, a sparkline and a status |
| `Timeline`, `TimelineSpan` | labelled ranges on a scale, overlaps stacked, milestones |

Settings are keyword arguments. Three are shared:

- `charset`: `"auto"` (the chart's natural glyphs: blocks, or Braille for
  line charts), `"blocks"`, `"braille"` or `"ascii"`. A console that is not
  UTF-8 always gets ASCII.
- `format`: how values are written: `"compact"` (`1.2k`, `3.4M`; the
  default) or a number of decimals. `chart_format(value, format)` writes one.
- `None` in a list of values is a gap: it is skipped, never drawn as zero.

`ChartBar`, `ChartState` and `TimelineSpan` are also exported as `Bar`,
`State` and `Span`, their Rust names. Styles come from the theme keys in
`chart.STYLES` (`chart.bar`, `chart.series.1`, ...).

From the shell, `rich chart` draws the same charts from CSV, JSON or stdin
([`rich chart`](https://buchochelliq-labs.github.io/rs-rich-cli/guide/ext/charts/#from-the-shell-rich-chart)).

## Sparklines, bars and histograms

```text
Sparkline(values, *, range=None, min=None, max=None, charset="auto", style=None,
          min_max=False, threshold=None, format=None)
BarChart(bars=None, *, range=None, max=None, charset="auto", orientation="horizontal",
         bar_width=None, show_values=True, format=None, style=None)
Histogram(values, *, bins=None, range=None, charset="auto", orientation="horizontal",
          bar_width=None, show_values=True, style=None)
```

`bars` is `(label, value)` pairs, a `{label: value}` mapping, or `ChartBar`s
(`ChartBar(label, value, *, style=None)`, to style one bar). `min_max` names
a sparkline's extremes after it, and `threshold` counts the values above
it. `Histogram.counts()` and `Histogram.edges()` give the bins.

```python
from rs_rich.console import Console
from rs_rich import chart

console = Console(width=40, color_system=None)
console.print(chart.Sparkline([12, 15, 14, 18, 25, 31, 28, 22, 35, 41]))
console.print(chart.Sparkline([12, 15, 14, 18, 25, 31, 28, 22, 35, 41], min_max=True, threshold=30))
console.print(chart.BarChart({"api": 412, "web": 268.5, "worker": 97}, bar_width=20))
console.print(chart.Histogram([12, 14, 15, 21, 22, 23, 24, 38], bins=3, range=(10, 40), bar_width=8))
```

```text
▁▂▁▂▄▆▅▃▇█
▁▂▁▂▄▆▅▃▇█ min 12 max 41 3 > 30
api    ████████████████████   412
web    █████████████        268.5
worker ████▊                   97
[10, 20) ██████   3
[20, 30) ████████ 4
[30, 40] ██       1
```

## Line and scatter charts

```text
Series(name, points, *, kind="line", style=None, marker=None)
LineChart(series=None, *, height=None, width=None, x_range=None, y_range=None,
          charset="auto", legend=True, x_format=None, y_format=None)
```

`points` is `(x, y)` pairs, or plain numbers at x = 0, 1, 2, ... `kind` is
`"line"` or `"scatter"`. In Braille each cell holds 2×4 points; in ASCII the
chart plots with markers at cell resolution.

```python
load = chart.Series("load", [1, 3, 2, 5, 4])
console.print(chart.LineChart([load], height=6, charset="ascii"), width=24)
```

```text
6 +                     
5 +               ***   
4 +             **   ***
3 +    ****   **        
2 +  **    ***          
1 +**                   
  ++----+----+----+----+
   0    1    2    3    4
* load                  
```

## Gauges and bullet charts

```text
Band(upto, label, *, style=None)
Gauge(label, value, *, range=None, target=None, bands=None, charset="auto",
      bar_width=None, full_width=False, format=None, unit=None, style=None)
BulletChart(gauges=None, *, bar_width=None, full_width=False, charset="auto")
```

A band covers the values up to `upto`; the gauge names the one its value is
in (`Gauge.band`).

```python
bands = [chart.Band(60, "ok"), chart.Band(85, "high"), chart.Band(100, "critical")]
cpu = chart.Gauge("cpu", 72, range=(0, 100), target=80, bands=bands, unit="%", bar_width=20)
console.print(cpu)
print(cpu.band)
console.print(chart.BulletChart([
    chart.Gauge("revenue", 270, range=(0, 300), target=250),
    chart.Gauge("profit", 22.5, range=(0, 30), target=26),
], bar_width=12))
```

```text
cpu ██████████████▍▒│░░░ 72% high
high
revenue ██████████│░  270
profit  █████████░│░ 22.5
```

## Heatmaps and status matrices

```text
Heatmap(rows=None, *, columns=None, range=None, charset="auto", cell_width=None,
        legend=True, format=None)
ChartState(name, symbol, ascii, style)
StatusMatrix(rows=None, *, columns=None, states=None, legend=True, charset="auto")
```

Rows are `(label, values)` pairs or a `{label: values}` mapping. A matrix's
values are state names: `pass`, `fail`, `skip`, `flaky`, or a `ChartState`
you add with `states=`. `StatusMatrix.counts()` counts each state.

```python
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
```

```text
    mon tue wed thu        
api ....====@@@@::::       
web     ****%%%%????       
0 [ .:-=+*#%@] 9  ? no data
     linux macos  win       
unit   ✓     ✓     ✗        
e2e    ✓     ⊘     ○        
✓ pass 3  ✗ fail 1  ○ skip 1
⊘ blocked 1                 
```

## KPI cards

```text
KpiCard(label, value, *, format=None, unit=None, delta=None, delta_percent=None,
        previous=None, caption=None, higher_is_better=True, trend=None, status=None,
        width=None, expand=False, border=True, charset="auto")
```

Give at most one of `delta`, `delta_percent` and `previous` (the delta is
then worked out). `trend` is the card's sparkline; `status` is `"ok"`,
`"warning"`, `"critical"` or `"unknown"`, drawn as a symbol and a word.

```python
console.print(chart.KpiCard("Requests", 1234, delta_percent=4.2, trend=[1, 2, 3, 2, 5], status="ok", width=24))
```

```text
╭──────────────────────╮
│ Requests        ✓ ok │
│ 1.2k                 │
│ ▲ +4.2%              │
│ ▁▃▅▃█                │
╰──────────────────────╯
```

## Timelines

```text
TimelineSpan(row, start, end, *, style=None)
Timeline(spans=None, *, milestones=None, range=None, charset="auto", format=None,
         unit=None, compress=True, durations=True, width=None)
```

Spans are `TimelineSpan`s or `(row, start, end)` triples; spans on one row
that overlap stack. Milestones are `(label, at)` pairs.

```python
console.print(chart.Timeline(
    [("fetch", 0, 4), ("compile", 4, 26), ("test", 12, 30), ("test", 20, 34)],
    milestones=[("ship", 36)],
    unit="s",
    width=40,
))
```

```text
fetch   ████ 4s                         
compile     █████████████████ 22s       
test              ███████████████ 18s   
                        ████████████ 14s
                                ship ◆  
        ┬───────┬───────┬───────┬───────
        0s     10s     20s     30s      
```

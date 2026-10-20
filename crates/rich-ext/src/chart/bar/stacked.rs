//! Stacked bars: a [`BarGroup::stacked`](super::BarGroup::stacked) group
//! drawn as one bar of parts, horizontal or vertical, with a legend.

use rich::{Console, Style};

use super::super::{series_key, wrap_entries};
use super::*;

/// Part fills without colour, by position in the stack (they cycle).
const PATTERNS: [char; 4] = ['█', '▓', '░', '▒'];
const BRAILLE_PATTERNS: [char; 4] = ['⣿', '⣤', '⠿', '⠶'];
const ASCII_PATTERNS: [char; 4] = ['#', '=', '+', ':'];
/// The legend's marker when colour tells the parts apart.
const MARKER: &str = "■";

/// One bar of a stacked chart: a label and the indices of its parts.
pub(super) struct Stack {
    label: String,
    parts: Vec<usize>,
    /// Parts take `chart.series.N` by position (a stacked group), rather
    /// than the chart's bar style (a bar on its own).
    series: bool,
}

/// A part's cells along the bar: `[from, to)` in whole cells, and the
/// fraction past `to` the last part above zero draws as a partial glyph.
struct Run {
    from: usize,
    to: usize,
    partial: f64,
    style: Option<Style>,
    fill: char,
}

impl BarChart {
    /// Whether any group is [stacked](BarGroup::stacked).
    pub(super) fn is_stacked(&self) -> bool {
        self.groups.iter().any(|g| g.stacked)
    }

    /// The bars as stacks: a stacked group is one, labelled with the
    /// group's label; any other bar is a stack of its own.
    pub(super) fn stacks(&self) -> Vec<Stack> {
        let mut out = Vec::new();
        let mut at = 0;
        let push_single = |out: &mut Vec<Stack>, i: usize| {
            out.push(Stack {
                label: self.bars[i].label.clone(),
                parts: vec![i],
                series: false,
            });
        };
        for group in &self.groups {
            let end = (at + group.len).min(self.bars.len());
            if group.stacked {
                if end > at {
                    out.push(Stack {
                        label: group.label.clone(),
                        parts: (at..end).collect(),
                        series: true,
                    });
                }
            } else {
                for i in at..end {
                    push_single(&mut out, i);
                }
            }
            at = end;
        }
        for i in at..self.bars.len() {
            push_single(&mut out, i);
        }
        out
    }

    /// A stack's parts above zero and below it, summed.
    fn sums(&self, stack: &Stack) -> (f64, f64) {
        let finite = stack
            .parts
            .iter()
            .map(|&i| self.bars[i].value)
            .filter(|v| v.is_finite());
        finite.fold((0.0, 0.0), |(up, down), v| {
            if v >= 0.0 {
                (up + v, down)
            } else {
                (up, down + v)
            }
        })
    }

    /// The scale: every stack's sums, zero included, fixed bounds kept.
    pub(super) fn stacked_scale(&self) -> Scale {
        let sums = self.stacks().into_iter().flat_map(|s| {
            let (up, down) = self.sums(&s);
            [up, down]
        });
        Scale::from_values(sums)
            .include_zero()
            .bounds(self.min, self.max)
    }

    /// A chart of one bar per stack, valued at its total: the layout the
    /// stacked chart takes its widths and columns from.
    pub(super) fn totals(&self) -> BarChart {
        let bars = self
            .stacks()
            .iter()
            .map(|stack| {
                let (up, down) = self.sums(stack);
                let any = stack.parts.iter().any(|&i| self.bars[i].value.is_finite());
                let mut bar = Bar::new(stack.label.clone(), if any { up + down } else { f64::NAN });
                if let [only] = stack.parts[..] {
                    bar.text = self.bars[only].text.clone();
                }
                bar
            })
            .collect();
        BarChart {
            bars,
            groups: Vec::new(),
            values_inside: false,
            ..self.clone()
        }
    }

    /// The style and fill of part `nth` of `stack` (bar `index`).
    fn part_look(
        &self,
        console: &Console,
        charset: Charset,
        colour: bool,
        stack: &Stack,
        (nth, index): (usize, usize),
    ) -> (Option<Style>, char) {
        let bar = &self.bars[index];
        let style = colour.then(|| match (&bar.style, stack.series) {
            (Some(spec), _) => user_style(console, spec),
            (None, true) => user_style(console, &series_key(nth)),
            (None, false) => self.bar_style(console, bar, bar.value < 0.0),
        });
        let fills: &[char] = match charset {
            Charset::Ascii => &ASCII_PATTERNS,
            Charset::Braille => &BRAILLE_PATTERNS,
            _ => &PATTERNS,
        };
        let fill = if colour {
            fills[0]
        } else {
            fills[nth % fills.len()]
        };
        (style, fill)
    }

    /// A stack's runs along `len` cells, from the start of the bar.
    fn runs(
        &self,
        console: &Console,
        charset: Charset,
        colour: bool,
        stack: &Stack,
        (scale, len): (&Scale, usize),
    ) -> Vec<Run> {
        let at = |v: f64| scale.normalize(v).unwrap_or(0.0) * len as f64;
        let mut runs = Vec::new();
        let (mut up, mut down) = (0.0, 0.0);
        let last_up = stack
            .parts
            .iter()
            .rposition(|&i| self.bars[i].value.is_finite() && self.bars[i].value > 0.0);
        for (nth, &index) in stack.parts.iter().enumerate() {
            let value = self.bars[index].value;
            if !value.is_finite() || value == 0.0 {
                continue;
            }
            let (style, fill) = self.part_look(console, charset, colour, stack, (nth, index));
            let (from, to) = if value > 0.0 {
                let from = at(up);
                up += value;
                (from, at(up))
            } else {
                let to = at(down);
                down += value;
                (at(down), to)
            };
            let start = from.round() as usize;
            let mut run = Run {
                from: start,
                to: (to.round() as usize).max(start),
                partial: 0.0,
                style,
                fill,
            };
            // The last part above zero ends in a partial glyph, when colour
            // tells the parts apart and the end is not on a cell edge.
            if Some(nth) == last_up && colour {
                run.to = (to.floor() as usize).max(start);
                run.partial = to - run.to as f64;
                if run.to == start && run.partial < 1.0 / 16.0 {
                    // A sliver, so a value above zero always shows.
                    run.partial = 1.0 / 8.0;
                }
            }
            runs.push(run);
        }
        runs
    }

    /// The legend's entries: one per position in the stacked groups, named
    /// after the first stacked group's bar there.
    fn legend_entries(
        &self,
        console: &Console,
        charset: Charset,
        colour: bool,
    ) -> Vec<Vec<(String, Option<Style>)>> {
        if !self.legend {
            return Vec::new();
        }
        let ascii = charset == Charset::Ascii;
        let stacks: Vec<Stack> = self.stacks().into_iter().filter(|s| s.series).collect();
        let depth = stacks.iter().map(|s| s.parts.len()).max().unwrap_or(0);
        let mut out = Vec::new();
        for nth in 0..depth {
            let Some(stack) = stacks.iter().find(|s| s.parts.len() > nth) else {
                continue;
            };
            let index = stack.parts[nth];
            let name = &self.bars[index].label;
            if name.is_empty() {
                continue;
            }
            let (style, fill) = self.part_look(console, charset, colour, stack, (nth, index));
            let marker = if colour {
                if ascii { "#" } else { MARKER }.to_string()
            } else {
                fill.to_string()
            };
            out.push(vec![
                (marker, style),
                (" ".to_string(), None),
                (
                    truncate(name, usize::MAX, ascii),
                    colour.then(|| theme_style(console, "chart.label")),
                ),
            ]);
        }
        out
    }

    pub(super) fn stacked_lines(
        &self,
        console: &Console,
        width: usize,
        charset: Charset,
        height: Option<usize>,
    ) -> Vec<Line> {
        let colour = has_colour(console);
        let entries = self.legend_entries(console, charset, colour);
        let legend = if entries.is_empty() {
            Vec::new()
        } else {
            wrap_entries(entries, width)
        };
        let mut out = match self.orientation {
            Orientation::Horizontal => self.stacked_horizontal(console, width, charset, colour),
            Orientation::Vertical => {
                let fit =
                    |rows: usize| self.stacked_vertical(console, width, charset, colour, rows);
                match (self.bar_width, height) {
                    (Some(rows), _) => fit(rows.min(MAX_COLUMN)),
                    (None, None) => fit(DEFAULT_COLUMN),
                    (None, Some(total)) => {
                        let total = total.saturating_sub(legend.len()).clamp(1, MAX_COLUMN);
                        let mut rows = total;
                        loop {
                            let lines = fit(rows);
                            if lines.len() <= total || rows == 1 {
                                break lines;
                            }
                            rows -= (lines.len() - total).min(rows - 1);
                        }
                    }
                }
            }
        };
        out.extend(legend);
        out
    }

    fn stacked_horizontal(
        &self,
        console: &Console,
        width: usize,
        charset: Charset,
        colour: bool,
    ) -> Vec<Line> {
        let ascii = charset == Charset::Ascii;
        let totals = self.totals();
        let (label_w, bar_w, value_w) = totals.layout(width);
        let scale = self.stacked_scale();
        let thickness = self.thickness.unwrap_or(1);
        let gap = self.gap();
        let mut out = Vec::new();
        for (n, (stack, total)) in self.stacks().iter().zip(&totals.bars).enumerate() {
            if n > 0 {
                out.extend((0..gap).map(|_| Line::new()));
            }
            let mut row = vec![(" ".to_string(), None); bar_w];
            for run in self.runs(console, charset, colour, stack, (&scale, bar_w)) {
                for cell in row.iter_mut().take(run.to.min(bar_w)).skip(run.from) {
                    *cell = (run.fill.to_string(), run.style.clone());
                }
                if run.partial > 0.0 && run.to < bar_w {
                    let glyph = Self::glyphs(charset, run.partial, true);
                    row[run.to] = (glyph, run.style.clone());
                }
            }
            for r in 0..thickness {
                let here = r == thickness / 2;
                let mut line = Line::new();
                if label_w > 0 {
                    if here {
                        let label = truncate(&stack.label, label_w, ascii);
                        line.push(&label, colour.then(|| self.group_style(console)));
                        line.pad(label_w - cells(&label) + 1);
                    } else {
                        line.pad(label_w + 1);
                    }
                }
                for (cell, style) in &row {
                    line.push(cell, style.clone());
                }
                if value_w > 0 {
                    if here && total.value.is_finite() {
                        let value = truncate(&totals.value_text(total), value_w, ascii);
                        line.pad(1 + value_w - cells(&value));
                        line.push(&value, colour.then(|| self.value_style_of(console, total)));
                    } else {
                        line.pad(1 + value_w);
                    }
                }
                out.push(line);
            }
        }
        out
    }

    fn stacked_vertical(
        &self,
        console: &Console,
        width: usize,
        charset: Charset,
        colour: bool,
        height: usize,
    ) -> Vec<Line> {
        let ascii = charset == Charset::Ascii;
        let totals = self.totals();
        let layout = totals.columns(width);
        let stacks = self.stacks();
        let lefts: Vec<usize> = (0..layout.shown)
            .map(|i| i * (layout.slot + layout.gap))
            .collect();
        let width = lefts.last().map_or(0, |l| l + layout.slot).min(width);
        let scale = self.stacked_scale();
        // Rows from the bottom of the plot, with one above and one below
        // for the totals.
        let (bottom, top) = (-1i64, height as i64);
        let mut grid: Vec<Vec<(String, Option<Style>)>> =
            vec![vec![(" ".to_string(), None); width]; (top - bottom + 1) as usize];
        let mut put = |row: i64, col: usize, cell: &str, style: Option<Style>| {
            if (bottom..=top).contains(&row) && col < width {
                grid[(top - row) as usize][col] = (cell.to_string(), style);
            }
        };
        let zero_row = (scale.normalize(0.0).unwrap_or(0.0) * height as f64).round() as i64;
        let mut used = (0i64, height as i64 - 1);
        for (i, stack) in stacks.iter().take(layout.shown).enumerate() {
            let bar_left = lefts[i] + (layout.slot - layout.thickness) / 2;
            let (mut high, mut low) = (zero_row, zero_row);
            for run in self.runs(console, charset, colour, stack, (&scale, height)) {
                for row in run.from..run.to {
                    for col in bar_left..bar_left + layout.thickness {
                        put(row as i64, col, &run.fill.to_string(), run.style.clone());
                    }
                }
                let mut end = run.to as i64;
                if run.partial > 0.0 && run.to < height {
                    let glyphs = Self::column_glyphs(charset, run.partial, true);
                    if let Some(glyph) = glyphs.first() {
                        for col in bar_left..bar_left + layout.thickness {
                            put(run.to as i64, col, &glyph.to_string(), run.style.clone());
                        }
                        end += 1;
                    }
                }
                if (run.from as i64) < zero_row {
                    low = low.min(run.from as i64);
                } else {
                    high = high.max(end);
                }
            }
            if !layout.values {
                continue;
            }
            let total = &totals.bars[i];
            if !total.value.is_finite() {
                continue;
            }
            let value = truncate(&totals.value_text(total), usize::MAX, ascii);
            let row = if total.value < 0.0 { low - 1 } else { high };
            let start = lefts[i] + (layout.slot.saturating_sub(cells(&value))) / 2;
            let style = colour.then(|| self.value_style_of(console, total));
            for (k, unit) in cell_units(&value).iter().enumerate() {
                put(row, start + k, unit, style.clone());
            }
            used = (used.0.min(row), used.1.max(row));
        }
        let mut out: Vec<Line> = grid
            .into_iter()
            .enumerate()
            .filter(|(i, _)| (used.0..=used.1).contains(&(top - *i as i64)))
            .map(|(_, row)| {
                let mut line = Line::new();
                for (cell, style) in row {
                    line.push(&cell, style);
                }
                line
            })
            .collect();
        if layout.labels {
            let style = colour.then(|| self.group_style(console));
            let mut line = Line::new();
            for (i, stack) in stacks.iter().take(layout.shown).enumerate() {
                line.pad(lefts[i].saturating_sub(line.width()));
                let label = truncate(&stack.label, layout.slot, ascii);
                let pad = layout.slot - cells(&label);
                line.pad(pad / 2);
                line.push(&label, style.clone());
                line.pad(pad - pad / 2);
            }
            line.pad(width.saturating_sub(line.width()));
            out.push(line);
        }
        out
    }

    /// `Stacked bar chart, 2 stacks of 3 parts, totals from 9 to 12`, then
    /// each stack's total and parts (up to six stacks), or the three
    /// largest totals and the smallest.
    pub(super) fn stacked_facts(&self) -> String {
        use super::super::describe::{counted, extent, range, ranked};
        let stacks = self.stacks();
        let totals = self.totals();
        let most = stacks.iter().map(|s| s.parts.len()).max().unwrap_or(0);
        let fewest = stacks.iter().map(|s| s.parts.len()).min().unwrap_or(0);
        let parts = if most == fewest {
            counted(most, "part", "parts")
        } else {
            format!("{fewest} to {most} parts")
        };
        let mut head = format!(
            "Stacked bar chart, {} of {parts}",
            counted(stacks.len(), "stack", "stacks")
        );
        if let Some((lo, hi)) = extent(totals.bars.iter().map(|b| b.value)) {
            head.push_str(&format!(", totals {}", range(lo, hi, self.format)));
        }
        let entries: Vec<(String, String, f64)> = stacks
            .iter()
            .zip(&totals.bars)
            .map(|(stack, total)| {
                let mut text = if total.value.is_finite() {
                    format!("total {}", totals.value_text(total))
                } else {
                    "no value".to_string()
                };
                if stack.series {
                    let parts: Vec<String> = stack
                        .parts
                        .iter()
                        .map(|&i| {
                            let bar = &self.bars[i];
                            let value = if bar.value.is_finite() {
                                self.value_text(bar)
                            } else {
                                "no value".to_string()
                            };
                            format!("{} {value}", bar.label).trim_start().to_string()
                        })
                        .collect();
                    text.push_str(&format!(" ({})", parts.join(", ")));
                }
                (stack.label.clone(), text, total.value)
            })
            .collect();
        let mut out = vec![head];
        out.extend(ranked(&entries, 6));
        out.join("\n")
    }
}

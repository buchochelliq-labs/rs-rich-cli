//! Timelines and Gantt strips: labelled ranges on a numeric scale.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::axis::{exact_label, fit, AxisRequest};
use super::{
    cells, has_colour, lines_to_segments, series_key, theme_style, truncate, user_style, Charset,
    Line, Scale, ValueFormat,
};

/// Columns an elided gap takes.
const BREAK: usize = 3;
/// The narrowest plot that keeps the full row labels.
const MIN_PLOT: usize = 8;

/// A range on a [`Timeline`]: the row it goes on, where it starts and where
/// it ends.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    /// The row's label. Spans with the same label share a row.
    pub row: String,
    /// Where the range starts.
    pub start: f64,
    /// Where the range ends (at or after `start`).
    pub end: f64,
    /// A theme key or style definition, instead of the row's
    /// `chart.series.N`.
    pub style: Option<String>,
}

impl Span {
    /// A range on row `row` from `start` to `end` (swapped if reversed).
    pub fn new(row: impl Into<String>, start: f64, end: f64) -> Self {
        let (start, end) = if end < start {
            (end, start)
        } else {
            (start, end)
        };
        Span {
            row: row.into(),
            start,
            end,
            style: None,
        }
    }

    /// This range's style: a theme key or a definition.
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    fn finite(&self) -> bool {
        self.start.is_finite() && self.end.is_finite()
    }
}

/// A point on a [`Timeline`], drawn `◆` (`*` in ASCII) on its own line
/// under the ranges, with its label beside it when there is room.
#[derive(Clone, Debug, PartialEq)]
pub struct Milestone {
    /// Written after the marker.
    pub label: String,
    /// Where it is.
    pub at: f64,
}

/// Columns of the plot that run at one rate: column `col0 + k` stands for
/// `lo + k * unit`, at its centre.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    lo: f64,
    unit: f64,
    col0: usize,
    cols: usize,
}

impl Piece {
    fn hi(&self) -> f64 {
        self.lo + self.cols.saturating_sub(1) as f64 * self.unit
    }

    /// The columns whose centre falls in `start..=end`.
    fn columns(&self, start: f64, end: f64) -> Option<(usize, usize)> {
        if self.unit <= 0.0 {
            return (start <= self.lo && self.lo <= end).then_some((self.col0, self.col0));
        }
        let first = ((start - self.lo) / self.unit - 1e-9).ceil().max(0.0);
        let last = ((end - self.lo) / self.unit + 1e-9)
            .floor()
            .min(self.cols as f64 - 1.0);
        (first <= last).then(|| (self.col0 + first as usize, self.col0 + last as usize))
    }

    /// The column nearest `value`, and how far it is in columns.
    fn nearest(&self, value: f64) -> (usize, f64) {
        if self.unit <= 0.0 {
            return (self.col0, (value - self.lo).abs());
        }
        let k = ((value - self.lo) / self.unit).round();
        let k = k.clamp(0.0, self.cols as f64 - 1.0);
        let off = ((value - self.lo) / self.unit - k).abs();
        (self.col0 + k as usize, off)
    }
}

/// Where an axis label sits against its column.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    /// Starting at the column: a stretch's first value.
    Start,
    /// Centred on it.
    Centre,
    /// Ending at it: a stretch's last value.
    End,
}

/// How the plot's columns map to values: one piece, or several with the
/// idle gaps between them elided.
struct Mapping {
    pieces: Vec<Piece>,
    /// `(column, label, anchor)` for the axis.
    labels: Vec<(usize, String, Anchor)>,
}

impl Mapping {
    /// The columns a range covers: every column whose centre is inside it,
    /// or the nearest one.
    fn span(&self, start: f64, end: f64) -> (usize, usize) {
        let mut covered: Option<(usize, usize)> = None;
        for piece in &self.pieces {
            if let Some((a, b)) = piece.columns(start, end) {
                covered = Some(match covered {
                    None => (a, b),
                    Some((x, y)) => (x.min(a), y.max(b)),
                });
            }
        }
        covered.unwrap_or_else(|| {
            let c = self.point((start + end) / 2.0);
            (c, c)
        })
    }

    /// The column nearest `value`.
    fn point(&self, value: f64) -> usize {
        self.pieces
            .iter()
            .map(|p| p.nearest(value))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map_or(0, |(c, _)| c)
    }

    /// Whether `col` is in an elided gap.
    fn is_break(&self, col: usize) -> bool {
        self.pieces.len() > 1
            && !self
                .pieces
                .iter()
                .any(|p| (p.col0..p.col0 + p.cols).contains(&col))
    }
}

/// Labelled ranges on a numeric scale, such as the steps of a build in
/// seconds: a Gantt strip when every range has its own row, a timeline of
/// lanes when ranges share a row.
///
/// - Each row is labelled on the left. Ranges on the same row that overlap
///   are **stacked** onto extra lines under it, so none hides another.
/// - Ranges are drawn `█`, or `▓` when one follows another on the same
///   line (`#` and `=` in ASCII), in their row's `chart.series.N` colour. A
///   range covers every column whose centre value is inside it, and at
///   least one.
/// - Each range's length is written after it when there is room before
///   the next one ([`durations`](Self::durations), on by default), so the
///   chart reads without the axis.
/// - [`milestone`](Self::milestone) marks a point with `◆` (`*`) on a line
///   under the ranges, its label beside it when it fits.
/// - The axis runs along the bottom. Its labels sit on evenly spaced
///   columns and name each column's exact value, as on a
///   [`LineChart`](super::LineChart); [`unit`](Self::unit) is written after
///   each value (`20s`).
/// - **Compression:** when the range is wider than the plot, so that the
///   shortest range would get less than a column of its own, idle gaps
///   longer than four times the shortest range are cut out
///   ([`compress`](Self::compress), on by default). Each cut is three
///   columns with `≈` (`~`) on the axis, and the axis labels each stretch
///   at its start and end.
/// - It fills the width given, or [`width`](Self::width). Given less, the
///   row labels are cut so the plot keeps 8 columns.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::Timeline;
///
/// let console = Console::builder().width(40).color_system(None).build();
/// let build = Timeline::new()
///     .span("fetch", 0.0, 4.0)
///     .span("compile", 4.0, 26.0)
///     .span("test", 12.0, 30.0)
///     .span("test", 20.0, 34.0)
///     .milestone("ship", 36.0)
///     .unit("s")
///     .width(40);
/// assert_eq!(
///     console.render_to_string(&build),
///     concat!(
///         "fetch   ████ 4s                         \n",
///         "compile     █████████████████ 22s       \n",
///         "test              ███████████████ 18s   \n",
///         "                        ████████████ 14s\n",
///         "                                ship ◆  \n",
///         "        ┬───────┬───────┬───────┬───────\n",
///         "        0s     10s     20s     30s      \n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    spans: Vec<Span>,
    milestones: Vec<Milestone>,
    min: Option<f64>,
    max: Option<f64>,
    charset: Charset,
    format: ValueFormat,
    unit: String,
    compress: bool,
    durations: bool,
    width: Option<usize>,
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// An empty timeline; add ranges with [`span`](Self::span).
    pub fn new() -> Self {
        Timeline {
            spans: Vec::new(),
            milestones: Vec::new(),
            min: None,
            max: None,
            charset: Charset::Auto,
            format: ValueFormat::Compact,
            unit: String::new(),
            compress: true,
            durations: true,
            width: None,
        }
    }

    /// Add a range on row `row` from `start` to `end`.
    pub fn span(self, row: impl Into<String>, start: f64, end: f64) -> Self {
        self.push(Span::new(row, start, end))
    }

    /// Add a [`Span`] built with its own style.
    pub fn push(mut self, span: Span) -> Self {
        self.spans.push(span);
        self
    }

    /// Add a milestone at `at`.
    pub fn milestone(mut self, label: impl Into<String>, at: f64) -> Self {
        self.milestones.push(Milestone {
            label: label.into(),
            at,
        });
        self
    }

    /// Fix the scale instead of fitting it to the ranges (no compression).
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = Some(min);
        self.max = Some(max);
        self
    }

    /// Glyphs to draw with: [`Charset::Ascii`] asks for ASCII; anything else
    /// draws blocks (ASCII on an ASCII-only console).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// How values are written (default [`ValueFormat::Compact`]).
    pub fn format(mut self, format: ValueFormat) -> Self {
        self.format = format;
        self
    }

    /// Written after every value on the axis and every length, such as `s`.
    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }

    /// Cut long idle gaps when the range is wider than the plot (default
    /// on).
    pub fn compress(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Write each range's length after it (default on).
    pub fn durations(mut self, show: bool) -> Self {
        self.durations = show;
        self
    }

    /// Fix the width (default: the width given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.max(1));
        self
    }

    /// The row labels, in the order they first appear.
    pub fn rows(&self) -> Vec<&str> {
        let mut rows: Vec<&str> = Vec::new();
        for span in &self.spans {
            if !rows.contains(&span.row.as_str()) {
                rows.push(&span.row);
            }
        }
        rows
    }

    /// The spans of `row` packed into lines: each line's spans in order,
    /// none overlapping another on its line.
    fn lanes(&self, row: &str) -> Vec<Vec<&Span>> {
        let mut spans: Vec<&Span> = self
            .spans
            .iter()
            .filter(|s| s.row == row && s.finite())
            .collect();
        spans.sort_by(|a, b| a.start.total_cmp(&b.start));
        let mut lanes: Vec<Vec<&Span>> = Vec::new();
        for span in spans {
            match lanes
                .iter_mut()
                .find(|lane| lane.last().is_some_and(|last| last.end <= span.start))
            {
                Some(lane) => lane.push(span),
                None => lanes.push(vec![span]),
            }
        }
        if lanes.is_empty() {
            lanes.push(Vec::new());
        }
        lanes
    }

    fn data(&self) -> Option<(f64, f64)> {
        let points = self
            .spans
            .iter()
            .filter(|s| s.finite())
            .flat_map(|s| [s.start, s.end])
            .chain(
                self.milestones
                    .iter()
                    .map(|m| m.at)
                    .filter(|v| v.is_finite()),
            );
        let mut range: Option<(f64, f64)> = None;
        for v in points {
            range = Some(match range {
                None => (v, v),
                Some((lo, hi)) => (lo.min(v), hi.max(v)),
            });
        }
        range
    }

    fn label(&self, value: f64) -> String {
        format!("{}{}", exact_label(self.format, value), self.unit)
    }

    fn text(&self, value: f64) -> String {
        format!("{}{}", self.format.format(value), self.unit)
    }

    /// The plot's mapping for `plot` columns.
    fn mapping(&self, plot: usize) -> Mapping {
        let fixed = self.min.is_some() || self.max.is_some();
        let (lo, hi) = self.data().unwrap_or((0.0, 1.0));
        let data = Scale::new(lo, hi).bounds(self.min, self.max);
        if !fixed && self.compress {
            if let Some(mapping) = self.compressed(data, plot) {
                return mapping;
            }
        }
        let cells = plot.saturating_sub(1);
        let axis = fit(&AxisRequest {
            data,
            fixed,
            cells: &[cells],
            wanted: (plot / 8).max(2),
            label_room: true,
            format: self.format,
        });
        let labels = axis
            .labels
            .iter()
            .map(|(p, _)| {
                let value = axis.lo + *p as f64 * axis.unit;
                (*p, self.label(value), Anchor::Centre)
            })
            .collect();
        Mapping {
            pieces: vec![Piece {
                lo: axis.lo,
                unit: axis.unit,
                col0: 0,
                cols: plot.max(1),
            }],
            labels,
        }
    }

    /// The mapping with long idle gaps cut out, when the shortest range
    /// would get less than a column and there are gaps worth cutting.
    fn compressed(&self, data: Scale, plot: usize) -> Option<Mapping> {
        let spans: Vec<(f64, f64)> = self
            .spans
            .iter()
            .filter(|s| s.finite())
            .map(|s| (s.start, s.end))
            .chain(
                self.milestones
                    .iter()
                    .filter(|m| m.at.is_finite())
                    .map(|m| (m.at, m.at)),
            )
            .collect();
        let shortest = self
            .spans
            .iter()
            .filter(|s| s.finite() && s.end > s.start)
            .map(|s| s.end - s.start)
            .fold(f64::INFINITY, f64::min);
        if !shortest.is_finite() || plot < 2 * BREAK + 4 {
            return None;
        }
        let per_column = data.span() / (plot - 1) as f64;
        if shortest >= per_column {
            return None;
        }
        // Busy stretches: ranges merged where they touch or overlap.
        let mut sorted = spans;
        sorted.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut busy: Vec<(f64, f64)> = Vec::new();
        let min_gap = (shortest * 4.0).max(per_column * BREAK as f64);
        for (start, end) in sorted {
            match busy.last_mut() {
                Some(last) if start - last.1 <= min_gap => last.1 = last.1.max(end),
                _ => busy.push((start, end)),
            }
        }
        if busy.len() < 2 {
            return None;
        }
        let breaks = busy.len() - 1;
        let room = plot.checked_sub(breaks * BREAK)?;
        if room < busy.len() * 2 {
            return None;
        }
        // Share the columns by length, each stretch at least two.
        let total: f64 = busy.iter().map(|(a, b)| b - a).sum();
        let spare = room - busy.len() * 2;
        let mut cols: Vec<usize> = busy
            .iter()
            .map(|(a, b)| {
                let share = if total > 0.0 {
                    (b - a) / total * spare as f64
                } else {
                    0.0
                };
                2 + share.floor() as usize
            })
            .collect();
        let mut left = room - cols.iter().sum::<usize>();
        let mut order: Vec<usize> = (0..busy.len()).collect();
        order.sort_by(|&i, &j| {
            let frac = |k: usize| {
                let (a, b) = busy[k];
                let share = if total > 0.0 {
                    (b - a) / total * spare as f64
                } else {
                    0.0
                };
                share - share.floor()
            };
            frac(j).total_cmp(&frac(i))
        });
        for i in order.into_iter().cycle() {
            if left == 0 {
                break;
            }
            cols[i] += 1;
            left -= 1;
        }
        let mut pieces = Vec::new();
        let mut labels = Vec::new();
        let mut col0 = 0;
        for ((start, end), n) in busy.iter().zip(cols) {
            let unit = (end - start) / (n - 1) as f64;
            let piece = Piece {
                lo: *start,
                unit,
                col0,
                cols: n,
            };
            labels.push((col0, self.label(piece.lo), Anchor::Start));
            labels.push((col0 + n - 1, self.label(piece.hi()), Anchor::End));
            pieces.push(piece);
            col0 += n + BREAK;
        }
        Some(Mapping { pieces, labels })
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let given = self
            .width
            .unwrap_or(options.max_width)
            .min(options.max_width);
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let colour = has_colour(console);
        if self.data().is_none() {
            let mut line = Line::new();
            line.push(&truncate("no data", given, ascii), None);
            return vec![line];
        }
        let rows = self.rows();
        let natural = rows.iter().map(|r| cells(r)).max().unwrap_or(0);
        let part = |l: usize| l + usize::from(l > 0);
        let mut label_w = natural;
        if given < part(label_w) + MIN_PLOT {
            label_w = given.saturating_sub(MIN_PLOT + 1).min(natural);
            label_w = label_w.max(natural.min(3));
            if given < part(label_w) + 2 {
                label_w = 0;
            }
        }
        let plot = given.saturating_sub(part(label_w)).max(1);
        let mapping = self.mapping(plot);
        let label_style = colour.then(|| theme_style(console, "chart.label"));
        let value_style = colour.then(|| theme_style(console, "chart.value"));
        let axis_style = colour.then(|| theme_style(console, "chart.axis"));
        let (full, alt) = if ascii { ('#', '=') } else { ('█', '▓') };
        let mut out = Vec::new();

        for (index, row) in rows.iter().enumerate() {
            let row_style = colour.then(|| theme_style(console, &series_key(index)));
            for (lane_no, lane) in self.lanes(row).into_iter().enumerate() {
                let mut cells_row: Vec<(char, Option<Style>)> = vec![(' ', None); plot];
                let placed: Vec<(usize, usize)> =
                    lane.iter().map(|s| mapping.span(s.start, s.end)).collect();
                let mut glyph = full;
                for (k, span) in lane.iter().enumerate() {
                    let (a, b) = placed[k];
                    // Touching the one before: the other glyph.
                    let touching = k > 0 && placed[k - 1].1 + 1 >= a;
                    glyph = match (touching, glyph == full) {
                        (true, true) => alt,
                        _ => full,
                    };
                    let style = colour.then(|| match &span.style {
                        Some(s) => user_style(console, s),
                        None => row_style.clone().unwrap_or_default(),
                    });
                    for cell in cells_row.iter_mut().take(b + 1).skip(a) {
                        *cell = (glyph, style.clone());
                    }
                }
                if self.durations {
                    for (k, span) in lane.iter().enumerate() {
                        let text = self.text(span.end - span.start);
                        let text = truncate(&text, usize::MAX, ascii);
                        let start = placed[k].1 + 2;
                        let limit = placed.get(k + 1).map_or(plot, |n| n.0.saturating_sub(1));
                        if start + cells(&text) <= limit {
                            for (i, c) in text.chars().enumerate() {
                                cells_row[start + i] = (c, value_style.clone());
                            }
                        }
                    }
                }
                let mut line = Line::new();
                if label_w > 0 {
                    let text = if lane_no == 0 {
                        truncate(row, label_w, ascii)
                    } else {
                        String::new()
                    };
                    let pad = label_w - cells(&text);
                    line.push(&text, label_style.clone());
                    line.pad(pad + 1);
                }
                for (c, style) in cells_row {
                    line.push(&c.to_string(), style);
                }
                out.push(line);
            }
        }

        // Milestones, each with its label when it fits before the next.
        let mut marks: Vec<(usize, &Milestone)> = self
            .milestones
            .iter()
            .filter(|m| m.at.is_finite())
            .map(|m| (mapping.point(m.at), m))
            .collect();
        if !marks.is_empty() {
            marks.sort_by_key(|(c, _)| *c);
            let style = colour.then(|| theme_style(console, "chart.milestone"));
            let mut row: Vec<(char, Option<Style>)> = vec![(' ', None); plot];
            let marker = if ascii { '*' } else { '◆' };
            for (col, _) in &marks {
                row[*col] = (marker, style.clone());
            }
            // After the marker when it fits, else before it, else cut.
            let mut free = 0;
            for (k, (col, mark)) in marks.iter().enumerate() {
                let limit = marks.get(k + 1).map_or(plot, |n| n.0.saturating_sub(1));
                let after = limit.saturating_sub(col + 2);
                let before = col.saturating_sub(free + 1);
                let len = cells(&truncate(&mark.label, usize::MAX, ascii));
                let (start, room) = if len <= after || after >= before {
                    (col + 2, after)
                } else {
                    (col - 1 - len.min(before), before)
                };
                if room == 0 {
                    free = col + 2;
                    continue;
                }
                let text = truncate(&mark.label, room, ascii);
                for (i, c) in text.chars().enumerate() {
                    row[start + i] = (c, label_style.clone());
                }
                free = (start + cells(&text)).max(col + 1) + 1;
            }
            let mut line = Line::new();
            line.pad(part(label_w));
            for (c, style) in row {
                line.push(&c.to_string(), style);
            }
            out.push(line);
        }

        // The axis and its labels.
        let (rule, tick, gap) = if ascii {
            ('-', '+', '~')
        } else {
            ('─', '┬', '≈')
        };
        let mut axis: Vec<char> = (0..plot)
            .map(|c| {
                if mapping.is_break(c) {
                    if mapping.is_break(c.wrapping_sub(1)) && mapping.is_break(c + 1) {
                        gap
                    } else {
                        ' '
                    }
                } else {
                    rule
                }
            })
            .collect();
        let mut labels: Vec<char> = vec![' '; plot];
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for (col, text, anchor) in &mapping.labels {
            let text = truncate(text, plot, ascii);
            let len = cells(&text);
            // Against its column, kept inside the plot.
            let start = match anchor {
                Anchor::Start => *col,
                Anchor::Centre => col.saturating_sub(len / 2),
                Anchor::End => (col + 1).saturating_sub(len),
            }
            .min(plot.saturating_sub(len));
            let end = start + len;
            if taken.iter().any(|&(a, b)| start < b + 1 && a < end + 1) {
                continue;
            }
            for (i, c) in text.chars().enumerate() {
                labels[start + i] = c;
            }
            taken.push((start, end));
            if *col < plot {
                axis[*col] = tick;
            }
        }
        let mut axis_line = Line::new();
        axis_line.pad(part(label_w));
        axis_line.push(&axis.into_iter().collect::<String>(), axis_style);
        out.push(axis_line);
        let mut label_line = Line::new();
        label_line.pad(part(label_w));
        label_line.push(&labels.into_iter().collect::<String>(), label_style);
        out.push(label_line);
        out
    }
}

impl Renderable for Timeline {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.lines(console, options), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = self.width.unwrap_or(options.max_width);
        Measurement::new(max.min(12), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

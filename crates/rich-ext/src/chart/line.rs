//! Line and scatter charts on a Braille dot canvas.

use rich::cells::char_cell_width;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::canvas::bresenham;
use super::{
    cells, has_colour, lines_to_segments, series_key, theme_style, truncate, user_style, Charset,
    DotCanvas, Line, Scale, ValueFormat,
};

/// Markers per series at cell resolution, in order; they cycle.
const UNICODE_MARKERS: [char; 5] = ['●', '◆', '▲', '■', '○'];
const ASCII_MARKERS: [char; 5] = ['*', '+', 'o', 'x', '.'];

/// The narrowest plot area drawn before the y labels give way.
const MIN_PLOT: usize = 4;

/// How a [`Series`] is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SeriesKind {
    /// Points joined by straight lines; a NaN or infinite value breaks the
    /// line.
    #[default]
    Line,
    /// Points only.
    Scatter,
}

/// One named set of `(x, y)` points in a [`LineChart`].
#[derive(Clone, Debug, PartialEq)]
pub struct Series {
    name: String,
    points: Vec<(f64, f64)>,
    kind: SeriesKind,
    style: Option<String>,
    marker: Option<char>,
}

impl Series {
    /// A line through `points`, in the order given.
    pub fn line(name: impl Into<String>, points: impl IntoIterator<Item = (f64, f64)>) -> Self {
        Series {
            name: name.into(),
            points: points.into_iter().collect(),
            kind: SeriesKind::Line,
            style: None,
            marker: None,
        }
    }

    /// Unjoined points.
    pub fn scatter(name: impl Into<String>, points: impl IntoIterator<Item = (f64, f64)>) -> Self {
        Series {
            kind: SeriesKind::Scatter,
            ..Series::line(name, points)
        }
    }

    /// A line through `values` at x = 0, 1, 2, …
    pub fn from_values(name: impl Into<String>, values: impl IntoIterator<Item = f64>) -> Self {
        Series::line(
            name,
            values.into_iter().enumerate().map(|(i, y)| (i as f64, y)),
        )
    }

    /// Draw as a line or as points.
    pub fn kind(mut self, kind: SeriesKind) -> Self {
        self.kind = kind;
        self
    }

    /// The series' colour: a theme key or a definition (default
    /// `chart.series.N`, by position).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// The glyph marking this series at cell resolution and in the legend
    /// (default by position: `●◆▲■○`, or `*+ox.` in ASCII). A marker that is
    /// not one cell wide, or not ASCII on an ASCII console, is replaced by
    /// the default.
    pub fn marker(mut self, marker: char) -> Self {
        self.marker = Some(marker);
        self
    }

    /// The name shown in the legend.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The points.
    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }

    fn finite(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.points
            .iter()
            .copied()
            .filter(|(x, y)| x.is_finite() && y.is_finite())
    }
}

/// A line or scatter chart: one or more [`Series`] with a y axis, x axis
/// labels and a legend.
///
/// - [`Charset::Braille`] (the default) plots 2×4 dots per cell.
///   [`Charset::Blocks`] and [`Charset::Ascii`] plot at cell resolution,
///   each series with its own marker (`●◆▲■○`, or `*`, `+`, `o`, `x`, `.`),
///   lines drawn as a run of markers.
/// - Series are told apart by colour (`chart.series.1` to `.5`) **and** by
///   marker: the legend shows each series' marker in its colour. Braille
///   dots carry no shape, so when the console shows no colour and there is
///   more than one series, the chart plots at cell resolution with markers
///   instead.
/// - The y scale is rounded out to whole ticks unless you fix it with
///   [`y_range`](Self::y_range); the x scale runs from the first to the last
///   x value unless you fix it with [`x_range`](Self::x_range).
/// - It is [`height`](Self::height) rows of plot (8 by default) plus an
///   axis line, a row of x labels and the legend. It fills the width it is
///   given (or [`width`](Self::width)); when that is short it drops the y
///   labels, then the axes, and the x labels and legend are cut to fit.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Charset, LineChart, Series};
///
/// let console = Console::builder().width(24).color_system(None).build();
/// let chart = LineChart::new()
///     .series(Series::from_values("load", [1.0, 3.0, 2.0, 5.0, 4.0]))
///     .height(4)
///     .charset(Charset::Ascii);
/// assert_eq!(
///     console.render_to_string(&chart),
///     concat!(
///         "6 +              ****   \n",
///         "4 +            **    ***\n",
///         "2 +   *********         \n",
///         "0 +***                  \n",
///         "  ++----+----+----+----+\n",
///         "   0    1    2    3    4\n",
///         "* load                  \n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct LineChart {
    series: Vec<Series>,
    height: usize,
    width: Option<usize>,
    x_min: Option<f64>,
    x_max: Option<f64>,
    y_min: Option<f64>,
    y_max: Option<f64>,
    charset: Charset,
    legend: bool,
    y_format: ValueFormat,
    x_format: ValueFormat,
}

impl Default for LineChart {
    fn default() -> Self {
        Self::new()
    }
}

impl LineChart {
    /// An empty chart; add series with [`series`](Self::series).
    pub fn new() -> Self {
        LineChart {
            series: Vec::new(),
            height: 8,
            width: None,
            x_min: None,
            x_max: None,
            y_min: None,
            y_max: None,
            charset: Charset::Auto,
            legend: true,
            y_format: ValueFormat::Compact,
            x_format: ValueFormat::Compact,
        }
    }

    /// Add a series.
    pub fn series(mut self, series: Series) -> Self {
        self.series.push(series);
        self
    }

    /// The series, in legend order.
    pub fn all_series(&self) -> &[Series] {
        &self.series
    }

    /// Rows of plot, not counting the axis, x labels and legend (default 8,
    /// at least 1).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.max(1));
        self
    }

    /// Fix the x scale; points outside are clamped to its edges.
    pub fn x_range(mut self, min: f64, max: f64) -> Self {
        self.x_min = Some(min);
        self.x_max = Some(max);
        self
    }

    /// Fix the y scale; points outside are clamped to its edges.
    pub fn y_range(mut self, min: f64, max: f64) -> Self {
        self.y_min = Some(min);
        self.y_max = Some(max);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: Braille).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Show the legend (default on, when any series has a name).
    pub fn legend(mut self, show: bool) -> Self {
        self.legend = show;
        self
    }

    /// How the y axis writes values (default [`ValueFormat::Compact`]).
    pub fn y_format(mut self, format: ValueFormat) -> Self {
        self.y_format = format;
        self
    }

    /// How the x axis writes values (default [`ValueFormat::Compact`]).
    pub fn x_format(mut self, format: ValueFormat) -> Self {
        self.x_format = format;
        self
    }

    fn y_ticks_wanted(&self) -> usize {
        (self.height / 2 + 1).clamp(2, 6)
    }

    /// The x scale, the y scale and the y tick step.
    fn scales(&self) -> (Scale, Scale, f64) {
        let xs = self.series.iter().flat_map(|s| s.finite().map(|p| p.0));
        let ys = self.series.iter().flat_map(|s| s.finite().map(|p| p.1));
        let x = Scale::from_values(xs).bounds(self.x_min, self.x_max);
        let y = Scale::from_values(ys);
        let wanted = self.y_ticks_wanted();
        if self.y_min.is_none() && self.y_max.is_none() {
            (x, y.nice(wanted), y.step(wanted))
        } else {
            let y = y.bounds(self.y_min, self.y_max);
            (x, y, y.step(wanted))
        }
    }

    fn marker(&self, index: usize, ascii: bool) -> char {
        let set = if ascii {
            &ASCII_MARKERS
        } else {
            &UNICODE_MARKERS
        };
        self.series[index]
            .marker
            .filter(|m| char_cell_width(*m) == 1 && (!ascii || m.is_ascii()))
            .unwrap_or(set[index % set.len()])
    }

    fn series_style(&self, console: &Console, index: usize) -> Style {
        match &self.series[index].style {
            Some(spec) => user_style(console, spec),
            None => theme_style(console, &series_key(index)),
        }
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Line> {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        let mut charset = self.charset.resolve(console, options, Charset::Braille);
        let colour = has_colour(console);
        if charset == Charset::Braille && !colour && self.series.len() > 1 {
            charset = Charset::Blocks;
        }
        let ascii = charset == Charset::Ascii;
        if self.series.iter().all(|s| s.finite().next().is_none()) {
            let mut line = Line::new();
            line.push(&truncate("no data", width, ascii), None);
            return vec![line];
        }
        let (x_scale, y_scale, y_step) = self.scales();
        let rows = self.height;
        let axis_style = colour.then(|| theme_style(console, "chart.axis"));
        let label_style = colour.then(|| theme_style(console, "chart.label"));

        // Y ticks and the space their labels need.
        let y_ticks: Vec<(f64, String)> = y_scale
            .ticks_every(y_step)
            .into_iter()
            .map(|t| (t, self.y_format.format(t)))
            .collect();
        let label_w = y_ticks.iter().map(|(_, l)| cells(l)).max().unwrap_or(0);
        let (labels, axis) = if width >= label_w + 2 + MIN_PLOT {
            (true, true)
        } else if width >= 2 {
            (false, true)
        } else {
            (false, false)
        };
        let prefix = if labels { label_w + 1 } else { 0 } + usize::from(axis);
        let plot_w = width.saturating_sub(prefix).max(1);

        // Plot.
        let braille = charset == Charset::Braille;
        let (dot_w, dot_h) = if braille {
            (plot_w * 2, rows * 4)
        } else {
            (plot_w, rows)
        };
        let to_dot = |x: f64, y: f64| -> (i64, i64) {
            let nx = x_scale.normalize(x).unwrap_or(0.0);
            let ny = y_scale.normalize(y).unwrap_or(0.0);
            (
                (nx * (dot_w - 1) as f64).round() as i64,
                ((1.0 - ny) * (dot_h - 1) as f64).round() as i64,
            )
        };
        let mut canvas = DotCanvas::new(plot_w, rows);
        let mut grid: Vec<Option<(char, usize)>> = vec![None; plot_w * rows];
        for (index, series) in self.series.iter().enumerate() {
            let marker = self.marker(index, ascii);
            let mut plot = |(x, y): (i64, i64)| {
                if braille {
                    canvas.set(x, y, index);
                } else if x >= 0 && y >= 0 && (x as usize) < plot_w && (y as usize) < rows {
                    grid[y as usize * plot_w + x as usize] = Some((marker, index));
                }
            };
            let mut previous: Option<(i64, i64)> = None;
            for &(x, y) in &series.points {
                if !(x.is_finite() && y.is_finite()) {
                    previous = None;
                    continue;
                }
                let point = to_dot(x, y);
                match (series.kind, previous) {
                    (SeriesKind::Line, Some(from)) => {
                        for p in bresenham(from, point) {
                            plot(p);
                        }
                    }
                    _ => plot(point),
                }
                previous = Some(point);
            }
        }

        // Which plot row each y tick lands on (the first tick wins a row).
        let mut tick_rows: Vec<Option<&str>> = vec![None; rows];
        for (t, label) in &y_ticks {
            let (_, dy) = to_dot(x_scale.min(), *t);
            let row = (dy.max(0) as usize / if braille { 4 } else { 1 }).min(rows - 1);
            tick_rows[row].get_or_insert(label.as_str());
        }

        let styles: Vec<Option<Style>> = (0..self.series.len())
            .map(|i| colour.then(|| self.series_style(console, i)))
            .collect();
        let (v_axis, v_tick, corner, h_axis, h_tick) = if ascii {
            ('|', '+', '+', '-', '+')
        } else {
            ('│', '┤', '└', '─', '┬')
        };
        let mut out = Vec::new();
        for (row, tick) in tick_rows.iter().enumerate() {
            let mut line = Line::new();
            if labels {
                let label = tick.unwrap_or("");
                line.pad(label_w - cells(label));
                line.push(label, label_style.clone());
                line.pad(1);
            }
            if axis {
                let glyph = if tick.is_some() { v_tick } else { v_axis };
                line.push(&glyph.to_string(), axis_style.clone());
            }
            for col in 0..plot_w {
                let (glyph, layer) = if braille {
                    if canvas.is_set(col, row) {
                        let (c, layer) = canvas.cell(col, row);
                        (c, layer)
                    } else {
                        (' ', None)
                    }
                } else {
                    match grid[row * plot_w + col] {
                        Some((c, layer)) => (c, Some(layer)),
                        None => (' ', None),
                    }
                };
                let style = layer.and_then(|l| styles[l].clone());
                line.push(&glyph.to_string(), style);
            }
            out.push(line);
        }

        // X axis and labels.
        if axis {
            // As many ticks as labels of the bounds' width fit, with gaps.
            let label_guess = cells(&self.x_format.format(x_scale.min()))
                .max(cells(&self.x_format.format(x_scale.max())));
            let x_ticks: Vec<(usize, String)> = x_scale
                .ticks((plot_w / (label_guess + 3)).max(2))
                .into_iter()
                .map(|t| {
                    let col = (x_scale.normalize(t).unwrap_or(0.0) * (plot_w - 1) as f64).round();
                    (col as usize, self.x_format.format(t))
                })
                .collect();
            let mut rule = Line::new();
            rule.pad(prefix - 1);
            let mut glyphs: Vec<char> = vec![h_axis; plot_w];
            for (col, _) in &x_ticks {
                glyphs[*col] = h_tick;
            }
            rule.push(&corner.to_string(), axis_style.clone());
            rule.push(&glyphs.into_iter().collect::<String>(), axis_style.clone());
            out.push(rule);

            let mut row = vec![' '; plot_w];
            let mut free_from = 0usize;
            for (col, label) in &x_ticks {
                let len = label.chars().count();
                if len > plot_w {
                    continue;
                }
                let start = col.saturating_sub(len / 2).min(plot_w - len);
                if start < free_from {
                    continue;
                }
                for (i, c) in label.chars().enumerate() {
                    row[start + i] = c;
                }
                free_from = start + len + 1;
            }
            let mut labels_line = Line::new();
            labels_line.pad(prefix);
            labels_line.push(&row.into_iter().collect::<String>(), label_style.clone());
            out.push(labels_line);
        }

        // Legend: marker and name, in series order, wrapped to the width.
        if self.legend && self.series.iter().any(|s| !s.name.is_empty()) {
            let mut line = Line::new();
            for (index, series) in self.series.iter().enumerate() {
                let name = truncate(&series.name, width.saturating_sub(2), ascii);
                let entry_w = 2 + cells(&name);
                if line.width() > 0 && line.width() + 2 + entry_w > width {
                    out.push(std::mem::take(&mut line));
                }
                if line.width() > 0 {
                    line.pad(2);
                }
                line.push(
                    &self.marker(index, ascii).to_string(),
                    styles[index].clone(),
                );
                line.pad(1);
                line.push(&name, label_style.clone());
            }
            out.push(line);
        }
        out
    }
}

impl Renderable for LineChart {
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

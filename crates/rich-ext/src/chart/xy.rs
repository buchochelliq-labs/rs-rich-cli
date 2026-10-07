//! An x–y chart laid out as ratatui's `Chart` widget: datasets drawn on a
//! [`Canvas`](super::Canvas), two axes with titles and labels, and a boxed
//! legend.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::plot_canvas::{is_ascii, raster, spec_style, CellGrid, Marker, Op, View};
use super::{cell_units, cells, lines_to_segments, series_key, Scale, ValueFormat};
use crate::fidelity::ascii_text;

/// Rows a [`Chart`] takes unless [`Chart::height`] says otherwise.
const DEFAULT_HEIGHT: usize = 15;
/// The tallest chart drawn, in rows.
const MAX_HEIGHT: usize = u16::MAX as usize;

/// How an [`Axis`] maps values onto its length.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AxisScale {
    /// Evenly.
    #[default]
    Linear,
    /// By their base-10 logarithm: each power of ten takes the same length.
    /// Zero, negative and non-finite values are skipped.
    Log,
}

/// The labels along an [`Axis`].
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Labels {
    /// No labels, and no axis line (as in ratatui, where an axis without
    /// labels draws neither).
    #[default]
    None,
    /// About this many round values inside the bounds, each at its place.
    /// On a [`Log`](AxisScale::Log) axis: the powers of ten, with 2 and 5
    /// times them when fewer than two powers fit.
    Auto(usize),
    /// These values, each at its place; values outside the bounds are left
    /// out.
    Values(Vec<f64>),
    /// Text spread evenly along the axis, as ratatui places its labels: the
    /// first at the start, the last at the end.
    Text(Vec<String>),
}

/// One axis of a [`Chart`]: its bounds, title, labels and scale.
///
/// ```
/// use rich_ext::chart::{Axis, AxisScale, Labels};
///
/// let axis = Axis::default()
///     .bounds(1.0, 1000.0)
///     .scale(AxisScale::Log)
///     .labels(Labels::Auto(4))
///     .title("Hz");
/// assert_eq!(axis.label_values(), vec![1.0, 10.0, 100.0, 1000.0]);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Axis {
    bounds: Option<[f64; 2]>,
    title: Option<String>,
    title_style: Option<String>,
    labels: Labels,
    scale: AxisScale,
    style: Option<String>,
    label_format: ValueFormat,
}

impl Axis {
    /// The values at either end, in either order. Without bounds, the axis
    /// spans the chart's data.
    pub fn bounds(mut self, min: f64, max: f64) -> Self {
        self.bounds = Some([min, max]);
        self
    }

    /// The axis title: the y axis's above its top left, the x axis's at the
    /// right end of the plot's bottom row.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// The title's style: a theme key or a definition (default
    /// `chart.label`).
    pub fn title_style(mut self, style: impl Into<String>) -> Self {
        self.title_style = Some(style.into());
        self
    }

    /// The labels (default [`Labels::None`]: no labels and no axis line).
    pub fn labels(mut self, labels: Labels) -> Self {
        self.labels = labels;
        self
    }

    /// Linear (the default) or logarithmic.
    pub fn scale(mut self, scale: AxisScale) -> Self {
        self.scale = scale;
        self
    }

    /// The style of the axis line and labels: a theme key or a definition
    /// (default `chart.axis`).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// How [`Labels::Auto`] and [`Labels::Values`] are written (default
    /// [`ValueFormat::Compact`]).
    pub fn label_format(mut self, format: ValueFormat) -> Self {
        self.label_format = format;
        self
    }

    /// The values labelled with [`Labels::Auto`] or [`Labels::Values`],
    /// when the axis spans its bounds (or `0..1` without them).
    pub fn label_values(&self) -> Vec<f64> {
        let scale = self.span(std::iter::empty());
        self.values(&scale)
    }

    /// `value` where the axis measures it: itself, or its logarithm.
    fn map(&self, value: f64) -> Option<f64> {
        match self.scale {
            _ if !value.is_finite() => None,
            AxisScale::Linear => Some(value),
            AxisScale::Log if value > 0.0 => Some(value.log10()),
            AxisScale::Log => None,
        }
    }

    /// The mapped span, from the bounds or else from `data`.
    fn span(&self, data: impl Iterator<Item = f64>) -> Scale {
        let given = self.bounds.and_then(|[a, b]| {
            let (a, b) = (self.map(a)?, self.map(b)?);
            Some(Scale::new(a, b))
        });
        match given {
            Some(scale) => scale,
            None => Scale::from_values(data.filter_map(|v| self.map(v))),
        }
    }

    /// The values [`Labels::Auto`] or [`Labels::Values`] put on `scale`
    /// (mapped), in value terms.
    fn values(&self, scale: &Scale) -> Vec<f64> {
        let inside = |m: f64| {
            let slack = scale.span() * 1e-9;
            m >= scale.min() - slack && m <= scale.max() + slack
        };
        match (&self.labels, self.scale) {
            (Labels::Values(values), _) => values
                .iter()
                .copied()
                .filter(|v| self.map(*v).is_some_and(inside))
                .collect(),
            (Labels::Auto(0), _) => Vec::new(),
            (Labels::Auto(n), AxisScale::Linear) => {
                scale.ticks(*n).into_iter().filter(|v| inside(*v)).collect()
            }
            (Labels::Auto(n), AxisScale::Log) => log_ticks(scale, *n),
            _ => Vec::new(),
        }
    }

    fn resolve(&self, data: impl Iterator<Item = f64>, ascii: bool) -> Resolved {
        let scale = self.span(data);
        let text = |s: &str| {
            if ascii && !s.is_ascii() {
                ascii_text(s)
            } else {
                s.to_string()
            }
        };
        let labels = match &self.labels {
            Labels::None => Placed::None,
            Labels::Text(labels) => Placed::Even(labels.iter().map(|l| text(l)).collect()),
            _ => Placed::At(
                self.values(&scale)
                    .into_iter()
                    .filter_map(|v| Some((self.map(v)?, text(&self.label_format.format(v)))))
                    .collect(),
            ),
        };
        Resolved {
            scale,
            labels,
            title: self.title.as_deref().map(text).filter(|t| !t.is_empty()),
        }
    }
}

/// Labels on a log axis spanning `scale` (in log10 terms): the powers of
/// ten inside, with 2 and 5 times them when fewer than two fit, thinned to
/// about `count`.
fn log_ticks(scale: &Scale, count: usize) -> Vec<f64> {
    let (lo, hi) = (scale.min(), scale.max());
    let slack = (hi - lo) * 1e-9;
    let inside = |v: f64| (lo - slack..=hi + slack).contains(&v.log10());
    // Bounds are logs of finite values, so these stay within ±400.
    let (first, last) = (lo.floor() as i32, hi.ceil() as i32);
    let mut values: Vec<f64> = (first..=last)
        .map(|k| 10f64.powi(k))
        .filter(|v| inside(*v))
        .collect();
    if values.len() < 2 {
        values = (first..=last)
            .flat_map(|k| [1.0, 2.0, 5.0].map(|m| m * 10f64.powi(k)))
            .filter(|v| inside(*v))
            .collect();
    }
    let count = count.max(2);
    if values.len() > count {
        let step = values.len().div_ceil(count);
        values = values.into_iter().step_by(step).collect();
    }
    values
}

/// An axis ready to draw: its mapped span, labels and title.
struct Resolved {
    scale: Scale,
    labels: Placed,
    title: Option<String>,
}

enum Placed {
    None,
    /// Labels at mapped values.
    At(Vec<(f64, String)>),
    /// Labels spread evenly.
    Even(Vec<String>),
}

impl Resolved {
    fn has_labels(&self) -> bool {
        !matches!(self.labels, Placed::None)
    }

    fn texts(&self) -> Vec<&str> {
        match &self.labels {
            Placed::None => Vec::new(),
            Placed::At(labels) => labels.iter().map(|(_, l)| l.as_str()).collect(),
            Placed::Even(labels) => labels.iter().map(String::as_str).collect(),
        }
    }

    /// The cell, of `cells`, that a mapped value falls in, with `per` dots
    /// to a cell, counted from the low end.
    fn cell(&self, mapped: f64, cells: usize, per: usize) -> usize {
        let t = self.scale.normalize(mapped).unwrap_or(0.0);
        let dots = (cells * per).saturating_sub(1);
        ((t * dots as f64) as usize / per).min(cells.saturating_sub(1))
    }
}

/// How a [`Dataset`] joins its points.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphType {
    /// Points only (ratatui's default).
    #[default]
    Scatter,
    /// Points joined by straight lines; a point the axes cannot show (NaN,
    /// infinite, or not positive on a log axis) breaks the line.
    Line,
    /// A vertical line from zero (the bottom, on a log axis) to each point.
    Bar,
}

/// One named set of `(x, y)` points in a [`Chart`].
#[derive(Clone, Debug, PartialEq)]
pub struct Dataset {
    name: String,
    points: Vec<(f64, f64)>,
    graph_type: GraphType,
    marker: Marker,
    style: Option<String>,
}

impl Dataset {
    /// Unnamed points, drawn as a scatter of [`Marker::Dot`]s (ratatui's
    /// defaults).
    pub fn new(points: impl IntoIterator<Item = (f64, f64)>) -> Self {
        Dataset {
            name: String::new(),
            points: points.into_iter().collect(),
            graph_type: GraphType::Scatter,
            marker: Marker::Dot,
            style: None,
        }
    }

    /// The name shown in the legend; a dataset without one is left out.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Points, lines or bars.
    pub fn graph_type(mut self, graph_type: GraphType) -> Self {
        self.graph_type = graph_type;
        self
    }

    /// The marker (default [`Marker::Dot`]).
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// The style of the points and the legend entry: a theme key or a
    /// definition (default `chart.series.N`, by position).
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    /// The points.
    pub fn points(&self) -> &[(f64, f64)] {
        &self.points
    }
}

/// Where a [`Chart`] puts its legend, inside the plot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LegendPosition {
    /// The top right corner (ratatui's default).
    #[default]
    TopRight,
    /// Centred along the top.
    Top,
    /// The top left corner.
    TopLeft,
    /// Centred on the left.
    Left,
    /// Centred on the right.
    Right,
    /// The bottom left corner.
    BottomLeft,
    /// Centred along the bottom.
    Bottom,
    /// The bottom right corner.
    BottomRight,
    /// No legend.
    None,
}

/// Datasets on two axes, laid out as ratatui's `Chart`, so a ratatui app
/// ports with the same look.
///
/// - The x axis's labels take the bottom row and its line (`─`) the row
///   above; the y axis's labels take the left (at most a third of the
///   width) and its line (`│`) the column after, meeting at `└`. An axis
///   draws its line only when it has [`Labels`].
/// - The datasets are drawn in order on a [`Canvas`](super::Canvas) filling
///   the rest, each with its marker; a later one wins a cell.
/// - The y axis's title sits on the plot's top row, from its left; the x
///   axis's at the right end of the plot's bottom row.
/// - The legend is a box listing the named datasets, each in its style, in
///   a corner of the plot ([`LegendPosition::TopRight`] by default). It
///   hides itself when it would take more than a share of the plot: a
///   quarter of the width and a quarter of the height by default.
/// - It is [`height`](Self::height) rows (15 by default) by the width it is
///   given (or [`width`](Self::width)). As in ratatui, a short chart gives
///   up the x axis line, then the x labels; a narrow one cuts the y labels
///   to a third of the width; the titles and the legend show only when the
///   plot has room for them.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Axis, Chart, Dataset, GraphType, Labels, Marker};
///
/// let console = Console::builder().width(24).color_system(None).build();
/// let cpu = Dataset::new([(0.0, 0.0), (5.0, 4.0), (10.0, 1.0)])
///     .name("cpu")
///     .graph_type(GraphType::Line)
///     .marker(Marker::Ascii('*'));
/// let chart = Chart::new(vec![cpu])
///     .x_axis(Axis::default().bounds(0.0, 10.0).title("t").labels(Labels::Auto(3)))
///     .y_axis(Axis::default().bounds(0.0, 4.0).title("%").labels(Labels::Auto(3)))
///     .hidden_legend_fraction(0.5, 0.5)
///     .height(8);
/// assert_eq!(
///     console.render_export(&chart),
///     concat!(
///         "4│%        ***     ┌───┐\n",
///         " │       **   **** │cpu│\n",
///         " │     **         *└───┘\n",
///         "2│   **               **\n",
///         " │ **                   \n",
///         "0│*                    t\n",
///         " └──────────────────────\n",
///         "  0         5         10\n",
///     )
/// );
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Chart {
    datasets: Vec<Dataset>,
    x_axis: Axis,
    y_axis: Axis,
    legend: LegendPosition,
    hidden_legend: (f64, f64),
    width: Option<usize>,
    height: usize,
}

impl Default for Chart {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Chart {
    /// A chart of `datasets`, drawn in order.
    pub fn new(datasets: impl IntoIterator<Item = Dataset>) -> Self {
        Chart {
            datasets: datasets.into_iter().collect(),
            x_axis: Axis::default(),
            y_axis: Axis::default(),
            legend: LegendPosition::TopRight,
            hidden_legend: (0.25, 0.25),
            width: None,
            height: DEFAULT_HEIGHT,
        }
    }

    /// Add a dataset, drawn after the others.
    pub fn dataset(mut self, dataset: Dataset) -> Self {
        self.datasets.push(dataset);
        self
    }

    /// The datasets, in drawing order.
    pub fn datasets(&self) -> &[Dataset] {
        &self.datasets
    }

    /// The x axis.
    pub fn x_axis(mut self, axis: Axis) -> Self {
        self.x_axis = axis;
        self
    }

    /// The y axis.
    pub fn y_axis(mut self, axis: Axis) -> Self {
        self.y_axis = axis;
        self
    }

    /// Where the legend goes (default [`LegendPosition::TopRight`]).
    pub fn legend(mut self, position: LegendPosition) -> Self {
        self.legend = position;
        self
    }

    /// The largest share of the plot's width and height the legend may
    /// take before it hides (default a quarter of each). Each is clamped
    /// to `0..=1`; NaN keeps the default.
    pub fn hidden_legend_fraction(mut self, width: f64, height: f64) -> Self {
        let share = |v: f64| if v.is_nan() { 0.25 } else { v.clamp(0.0, 1.0) };
        self.hidden_legend = (share(width), share(height));
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Height in rows, axes and labels included (default 15, at most
    /// 65535).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.min(MAX_HEIGHT);
        self
    }

    fn grid(&self, console: &Console, options: &ConsoleOptions) -> CellGrid {
        let w = self
            .width
            .map_or(options.max_width, |v| v.min(options.max_width))
            .min(MAX_HEIGHT);
        let h = self.height;
        let mut grid = CellGrid::new(w, h);
        if w == 0 || h == 0 {
            return grid;
        }
        let ascii = is_ascii(console, options);
        let points = || self.datasets.iter().flat_map(|d| d.points.iter());
        let x = self.x_axis.resolve(points().map(|p| p.0), ascii);
        let y = self.y_axis.resolve(points().map(|p| p.1), ascii);
        let style_of = |spec: &Option<String>, default: &str| {
            spec_style(console, spec.as_deref().unwrap_or(default))
        };
        let x_style = style_of(&self.x_axis.style, "chart.axis");
        let y_style = style_of(&self.y_axis.style, "chart.axis");
        let (h_line, v_line, corner) = if ascii {
            ('-', '|', '+')
        } else {
            ('─', '│', '└')
        };

        // The layout, as ratatui's: from the bottom row up and the left
        // column across.
        let mut bottom = h - 1;
        // A label row only when there are labels to put in it.
        let label_x = (!x.texts().is_empty() && bottom > 0).then(|| {
            bottom -= 1;
            bottom + 1
        });
        let mut left = left_width(w, &x, &y);
        let axis_x = (x.has_labels() && bottom > 0).then(|| {
            bottom -= 1;
            bottom + 1
        });
        let axis_y = (y.has_labels() && left + 1 < w).then(|| {
            left += 1;
            left - 1
        });
        let (gx, gw, gh) = (left, w - left, bottom + 1);
        let marker = self
            .datasets
            .first()
            .map_or(Marker::Dot, |d| d.marker)
            .resolve(ascii);
        let (per_x, per_y) = marker.per_cell();

        // Labels.
        if let Some(row) = label_x {
            match &x.labels {
                Placed::At(labels) => {
                    let mut free = 0;
                    for (value, label) in labels {
                        let len = cells(label);
                        if len > w {
                            continue;
                        }
                        let centre = gx + x.cell(*value, gw, per_x);
                        let start = centre.saturating_sub(len / 2).min(w - len);
                        if start < free {
                            continue;
                        }
                        grid.put_text(row, start, label, x_style.clone(), w);
                        free = start + len + 1;
                    }
                }
                Placed::Even(labels) if labels.len() >= 2 => {
                    let n = labels.len();
                    let between = gw / n;
                    let put = |grid: &mut CellGrid, col, width, text: &str, align| {
                        put_aligned(grid, row, col, width, text, x_style.clone(), align);
                    };
                    put(&mut grid, 0, gx, &labels[0], Align::Right);
                    for (i, label) in labels[1..n - 1].iter().enumerate() {
                        let col = gx + (i + 1) * between + 1;
                        put(
                            &mut grid,
                            col,
                            between.saturating_sub(1),
                            label,
                            Align::Centre,
                        );
                    }
                    put(
                        &mut grid,
                        w - between,
                        between,
                        &labels[n - 1],
                        Align::Right,
                    );
                }
                _ => {}
            }
        }
        if y.has_labels() {
            let width = gx.saturating_sub(1);
            match &y.labels {
                Placed::At(labels) => {
                    let mut used = vec![false; gh];
                    for (value, label) in labels {
                        let row = gh - 1 - y.cell(*value, gh, per_y);
                        if !std::mem::replace(&mut used[row], true) {
                            put_aligned(
                                &mut grid,
                                row,
                                0,
                                width,
                                label,
                                y_style.clone(),
                                Align::Right,
                            );
                        }
                    }
                }
                Placed::Even(labels) => {
                    let n = labels.len();
                    for (i, label) in labels.iter().enumerate() {
                        let dy = if n > 1 { i * (gh - 1) / (n - 1) } else { 0 };
                        if dy < gh {
                            let row = gh - 1 - dy;
                            put_aligned(
                                &mut grid,
                                row,
                                0,
                                width,
                                label,
                                y_style.clone(),
                                Align::Left,
                            );
                        }
                    }
                }
                Placed::None => {}
            }
        }

        // Axis lines.
        if let Some(row) = axis_x {
            for col in gx..w {
                grid.set(row, col, h_line.to_string(), x_style.clone());
            }
        }
        if let Some(col) = axis_y {
            for row in 0..gh {
                grid.set(row, col, v_line.to_string(), y_style.clone());
            }
            if let Some(row) = axis_x {
                grid.set(row, col, corner.to_string(), x_style.clone());
            }
        }

        // The datasets.
        let view = View {
            x: [x.scale.min(), x.scale.max()],
            y: [y.scale.min(), y.scale.max()],
            width: gw,
            height: gh,
            marker,
            ascii,
        };
        let plot = raster(&self.ops(&y), &view, console);
        grid.blit(&plot, 0, gx);

        // Titles.
        let x_title = x.title.as_ref().map(|t| (t, cells(t)));
        let x_title = x_title
            .filter(|(_, tw)| *tw < gw && gh > 2)
            .map(|(title, tw)| {
                let col = gx + gw - tw;
                let style = style_of(&self.x_axis.title_style, "chart.label");
                grid.put_text(gh - 1, col, title, style, w);
                (gh - 1, col, col + tw)
            });
        let y_title = y.title.as_ref().map(|t| (t, cells(t)));
        let y_title = y_title
            .filter(|(_, tw)| tw + 1 < gw && gh > 2)
            .map(|(title, tw)| {
                let style = style_of(&self.y_axis.title_style, "chart.label");
                grid.put_text(0, gx, title, style, w);
                (0, gx, gx + tw)
            });

        // The legend.
        let named: Vec<(String, Option<Style>)> = self
            .datasets
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.name.is_empty())
            .map(|(i, d)| {
                let name = if ascii && !d.name.is_ascii() {
                    ascii_text(&d.name)
                } else {
                    d.name.clone()
                };
                (name, self.dataset_style(console, i))
            })
            .collect();
        let inner = named.iter().map(|(n, _)| cells(n)).max().unwrap_or(0);
        let (lw, lh) = (inner + 2, named.len() + 2);
        let max_w = (gw as f64 * self.hidden_legend.0).round() as usize;
        let max_h = (gh as f64 * self.hidden_legend.1).round() as usize;
        if inner > 0 && lw <= max_w && lh <= max_h {
            let area = Area { gx, gw, gh };
            if let Some((top, col)) = area.legend(self.legend, lw, lh, x_title, y_title) {
                let border = if ascii {
                    ['+', '+', '+', '+', '-', '|']
                } else {
                    ['┌', '┐', '└', '┘', '─', '│']
                };
                draw_legend(
                    &mut grid,
                    (top, col),
                    inner,
                    &named,
                    border,
                    x_style.clone(),
                );
            }
        }
        grid
    }

    /// The style of dataset `index`.
    fn dataset_style(&self, console: &Console, index: usize) -> Option<Style> {
        spec_style(console, &self.dataset_spec(index))
    }

    fn dataset_spec(&self, index: usize) -> String {
        self.datasets[index]
            .style
            .clone()
            .unwrap_or_else(|| series_key(index))
    }

    /// The datasets as canvas steps, in the axes' mapped terms.
    fn ops(&self, y: &Resolved) -> Vec<Op> {
        let mut ops = Vec::new();
        for (index, dataset) in self.datasets.iter().enumerate() {
            let style = self.dataset_spec(index);
            let mapped: Vec<Option<(f64, f64)>> = dataset
                .points
                .iter()
                .map(|&(px, py)| Some((self.x_axis.map(px)?, self.y_axis.map(py)?)))
                .collect();
            let shown = || mapped.iter().flatten().copied();
            ops.push(Op::Marker(dataset.marker));
            ops.push(Op::Points(shown().collect(), style.clone()));
            match dataset.graph_type {
                GraphType::Scatter => {}
                GraphType::Line => {
                    for pair in mapped.windows(2) {
                        if let [Some(a), Some(b)] = pair {
                            ops.push(Op::Line(*a, *b, style.clone()));
                        }
                    }
                }
                GraphType::Bar => {
                    let base = match self.y_axis.scale {
                        AxisScale::Linear => 0.0,
                        AxisScale::Log => y.scale.min(),
                    };
                    for (px, py) in shown() {
                        ops.push(Op::Line((px, base), (px, py), style.clone()));
                    }
                }
            }
        }
        ops
    }
}

/// How wide the labels left of the y axis line are, as ratatui works it
/// out: the widest y label, or as much of the first x label as hangs left
/// of the plot, whichever is wider, and at most a third of `width`.
fn left_width(width: usize, x: &Resolved, y: &Resolved) -> usize {
    let mut widest = y.texts().iter().map(|l| cells(l)).max().unwrap_or(0);
    if let Some(first) = x.texts().first() {
        let first = cells(first);
        let hangs = match x.labels {
            // The label ends under the y axis line, when there is one.
            Placed::Even(_) => first.saturating_sub(usize::from(y.has_labels())),
            _ => first / 2,
        };
        widest = widest.max(hangs);
    }
    widest.min(width / 3)
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Centre,
    Right,
}

/// `text` in the `width` cells from `col` of `row`, aligned; when it is
/// wider it is cut on the right, or on the left when right-aligned.
fn put_aligned(
    grid: &mut CellGrid,
    row: usize,
    col: usize,
    width: usize,
    text: &str,
    style: Option<Style>,
    align: Align,
) {
    if width == 0 {
        return;
    }
    let len = cells(text);
    let end = col + width;
    if len <= width {
        let offset = match align {
            Align::Left => 0,
            Align::Centre => (width - len) / 2,
            Align::Right => width - len,
        };
        grid.put_text(row, col + offset, text, style, end);
    } else if let Align::Right = align {
        let units = cell_units(text);
        let kept: String = units[len - width..]
            .iter()
            .enumerate()
            .map(|(i, u)| if i == 0 && u.is_empty() { " " } else { u })
            .collect();
        grid.put_text(row, col, &kept, style, end);
    } else {
        grid.put_text(row, col, text, style, end);
    }
}

/// The plot's place in the chart: its left column, width and height.
struct Area {
    gx: usize,
    gw: usize,
    gh: usize,
}

/// A title's row and the columns it covers.
type TitleSpan = (usize, usize, usize);

impl Area {
    /// The top left cell of a `lw` × `lh` legend at `position`, moved a row
    /// off a title it would cover; `None` when there is no room.
    fn legend(
        &self,
        position: LegendPosition,
        lw: usize,
        lh: usize,
        x_title: Option<TitleSpan>,
        y_title: Option<TitleSpan>,
    ) -> Option<(usize, usize)> {
        // Room for the legend and a row for each title, or none.
        self.gh
            .checked_sub(lh + usize::from(x_title.is_some()) + usize::from(y_title.is_some()))?;
        if lw > self.gw {
            return None;
        }
        let (right, centre) = (self.gx + self.gw - lw, self.gx + (self.gw - lw) / 2);
        let (middle, bottom) = ((self.gh - lh) / 2, self.gh - lh);
        let (mut row, col) = match position {
            LegendPosition::TopLeft => (0, self.gx),
            LegendPosition::Top => (0, centre),
            LegendPosition::TopRight => (0, right),
            LegendPosition::Left => (middle, self.gx),
            LegendPosition::Right => (middle, right),
            LegendPosition::BottomLeft => (bottom, self.gx),
            LegendPosition::Bottom => (bottom, centre),
            LegendPosition::BottomRight => (bottom, right),
            LegendPosition::None => return None,
        };
        let covers = |row: usize, (r, c0, c1): TitleSpan| {
            row <= r && r < row + lh && col < c1 && c0 < col + lw
        };
        if y_title.is_some_and(|t| covers(row, t)) {
            row += 1;
        }
        if x_title.is_some_and(|t| covers(row, t)) {
            row = row.saturating_sub(1);
        }
        Some((row, col))
    }
}

/// A box at `at` listing `named`, each name in its style.
fn draw_legend(
    grid: &mut CellGrid,
    (top, left): (usize, usize),
    inner: usize,
    named: &[(String, Option<Style>)],
    [tl, tr, bl, br, h, v]: [char; 6],
    border: Option<Style>,
) {
    let edge = |a: char, b: char| format!("{a}{}{b}", h.to_string().repeat(inner));
    let right = left + inner + 2;
    grid.put_text(top, left, &edge(tl, tr), border.clone(), right);
    for (i, (name, style)) in named.iter().enumerate() {
        let row = top + 1 + i;
        grid.set(row, left, v.to_string(), border.clone());
        for col in left + 1..right - 1 {
            grid.set(row, col, " ".to_string(), None);
        }
        grid.put_text(row, left + 1, name, style.clone(), right - 1);
        grid.set(row, right - 1, v.to_string(), border.clone());
    }
    grid.put_text(top + named.len() + 1, left, &edge(bl, br), border, right);
}

impl Renderable for Chart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.grid(console, options).into_lines(), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = self.width.unwrap_or(options.max_width);
        Measurement::new(max.min(12), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(chart: &Chart, width: usize) -> Vec<String> {
        let console = Console::builder().width(width).color_system(None).build();
        console
            .render_export(chart)
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn dot(points: &[(f64, f64)]) -> Dataset {
        Dataset::new(points.to_vec()).marker(Marker::Ascii('o'))
    }

    #[test]
    fn log_axes_map_by_powers_of_ten() {
        let axis = Axis::default().bounds(1.0, 1000.0).scale(AxisScale::Log);
        assert_eq!(axis.map(100.0), Some(2.0));
        assert_eq!(axis.map(0.0), None);
        assert_eq!(axis.map(-5.0), None);
        assert_eq!(axis.map(f64::NAN), None);
        let labels = |axis: Axis| axis.label_values();
        assert_eq!(
            labels(axis.clone().labels(Labels::Auto(4))),
            vec![1.0, 10.0, 100.0, 1000.0]
        );
        // Thinned to about the count asked for.
        assert_eq!(
            labels(axis.clone().labels(Labels::Auto(2))),
            vec![1.0, 100.0]
        );
        // Fewer than two powers of ten: 2 and 5 times them too.
        let narrow = Axis::default()
            .bounds(2.0, 60.0)
            .scale(AxisScale::Log)
            .labels(Labels::Auto(6));
        assert_eq!(labels(narrow), vec![2.0, 5.0, 10.0, 20.0, 50.0]);
        // Values outside the bounds, or not positive, are left out.
        let given = axis.labels(Labels::Values(vec![-1.0, 0.0, 5.0, 5000.0]));
        assert_eq!(labels(given), vec![5.0]);
    }

    #[test]
    fn a_log_axis_places_points_by_decade() {
        let chart = Chart::new(vec![dot(&[
            (1.0, 0.0),
            (10.0, 0.0),
            (100.0, 0.0),
            (0.0, 0.0),
        ])])
        .x_axis(
            Axis::default()
                .bounds(1.0, 100.0)
                .scale(AxisScale::Log)
                .labels(Labels::Auto(3)),
        )
        .y_axis(Axis::default().bounds(0.0, 1.0))
        .height(3);
        assert_eq!(
            rows(&chart, 11),
            vec!["           ", "o    o    o", "───────────", "1   10  100"][1..].to_vec()
        );
    }

    #[test]
    fn text_labels_are_spread_as_ratatui_spreads_them() {
        let chart = Chart::new(Vec::new())
            .x_axis(Axis::default().labels(Labels::Text(vec!["a".into(), "b".into(), "c".into()])))
            .y_axis(Axis::default().labels(Labels::Text(vec!["lo".into(), "hi".into()])))
            .height(5);
        assert_eq!(
            rows(&chart, 14),
            vec![
                "hi│           ",
                "  │           ",
                "lo│           ",
                "  └───────────",
                "  a    b     c",
            ]
        );
    }

    #[test]
    fn bars_rise_from_zero() {
        let chart = Chart::new(vec![
            dot(&[(0.0, 2.0), (2.0, 4.0), (4.0, -1.0)]).graph_type(GraphType::Bar)
        ])
        .x_axis(Axis::default().bounds(0.0, 4.0))
        .y_axis(Axis::default().bounds(-1.0, 4.0))
        .height(6)
        .width(5);
        assert_eq!(
            rows(&chart, 5),
            vec!["  o  ", "  o  ", "o o  ", "o o  ", "o o o", "    o"]
        );
    }

    #[test]
    fn bars_on_a_log_axis_rise_from_the_bottom() {
        let chart = Chart::new(vec![dot(&[(1.0, 100.0)]).graph_type(GraphType::Bar)])
            .x_axis(Axis::default().bounds(0.0, 2.0))
            .y_axis(Axis::default().bounds(1.0, 1000.0).scale(AxisScale::Log))
            .height(4)
            .width(3);
        assert_eq!(rows(&chart, 3), vec!["   ", " o ", " o ", " o "]);
    }

    #[test]
    fn lines_break_at_points_the_axes_cannot_show() {
        let chart = Chart::new(vec![dot(&[
            (0.0, 0.0),
            (1.0, 0.0),
            (f64::NAN, 0.0),
            (3.0, f64::INFINITY),
            (4.0, 0.0),
            (5.0, 0.0),
        ])
        .graph_type(GraphType::Line)])
        .x_axis(Axis::default().bounds(0.0, 5.0))
        .y_axis(Axis::default().bounds(0.0, 1.0))
        .height(1)
        .width(6);
        assert_eq!(rows(&chart, 6), vec!["oo  oo"]);
    }

    fn legend_chart(position: LegendPosition) -> Chart {
        Chart::new(vec![
            dot(&[]).name("one").style("red"),
            dot(&[]).name("two").style("blue"),
            dot(&[]),
        ])
        .legend(position)
        .hidden_legend_fraction(1.0, 1.0)
        .width(9)
        .height(6)
    }

    #[test]
    fn legends_go_in_the_corner_asked_for() {
        assert_eq!(
            rows(&legend_chart(LegendPosition::TopRight), 9),
            vec![
                "    ┌───┐",
                "    │one│",
                "    │two│",
                "    └───┘",
                "         ",
                "         ",
            ]
        );
        assert_eq!(
            rows(&legend_chart(LegendPosition::BottomLeft), 9)[2..],
            ["┌───┐    ", "│one│    ", "│two│    ", "└───┘    "]
        );
        assert_eq!(
            rows(&legend_chart(LegendPosition::Right), 9)[1],
            "    ┌───┐"
        );
        assert!(rows(&legend_chart(LegendPosition::None), 9)
            .iter()
            .all(|r| r.trim().is_empty()));
    }

    #[test]
    fn legends_keep_clear_of_titles() {
        let chart = legend_chart(LegendPosition::TopLeft)
            .y_axis(Axis::default().title("y"))
            .x_axis(Axis::default().title("x"));
        assert_eq!(
            rows(&chart, 9),
            vec![
                "y        ",
                "┌───┐    ",
                "│one│    ",
                "│two│    ",
                "└───┘    ",
                "        x",
            ]
        );
        // No room for both titles and the box: no legend.
        assert!(rows(&chart.height(5), 9).iter().all(|r| !r.contains('┌')));
    }

    #[test]
    fn legends_hide_when_they_would_take_too_much() {
        let shown = |w: f64, h: f64| {
            let chart = legend_chart(LegendPosition::TopRight).hidden_legend_fraction(w, h);
            rows(&chart, 9).concat().contains("one")
        };
        assert!(shown(1.0, 1.0));
        // Five of nine columns, four of six rows.
        assert!(shown(0.56, 0.67));
        assert!(!shown(0.4, 1.0));
        assert!(!shown(1.0, 0.5));
        // The default, a quarter each way, hides it here.
        let default = legend_chart(LegendPosition::TopRight);
        let default = Chart {
            hidden_legend: (0.25, 0.25),
            ..default
        };
        assert!(!rows(&default, 9).concat().contains("one"));
    }

    #[test]
    fn the_legend_names_take_their_dataset_styles() {
        let console = Console::builder().width(9).build();
        let options = console.options();
        let grid = legend_chart(LegendPosition::TopRight).grid(&console, &options);
        assert_eq!(grid.style_at(1, 5), Style::parse("red").ok());
        assert_eq!(grid.style_at(2, 5), Style::parse("blue").ok());
    }

    #[test]
    fn later_datasets_win_a_cell() {
        let chart = Chart::new(vec![
            Dataset::new([(0.0, 0.0)]).style("red"),
            Dataset::new([(0.0, 0.0)]).style("blue"),
        ])
        .x_axis(Axis::default().bounds(0.0, 1.0))
        .y_axis(Axis::default().bounds(0.0, 1.0))
        .height(2)
        .width(2);
        let console = Console::builder().width(2).build();
        let grid = chart.grid(&console, &console.options());
        assert_eq!(grid.row_string(1), "• ");
        assert_eq!(grid.style_at(1, 0), Style::parse("blue").ok());
    }

    #[test]
    fn bounds_default_to_the_data() {
        let chart = Chart::new(vec![dot(&[(10.0, 5.0), (20.0, 7.0)])])
            .height(2)
            .width(3);
        assert_eq!(rows(&chart, 3), vec!["  o", "o  "]);
    }

    #[test]
    fn tiny_charts_drop_labels_then_titles() {
        let chart = Chart::new(vec![dot(&[(0.0, 0.0), (1.0, 1.0)])
            .name("series")
            .graph_type(GraphType::Line)])
        .x_axis(
            Axis::default()
                .bounds(0.0, 1.0)
                .title("time")
                .labels(Labels::Auto(5)),
        )
        .y_axis(
            Axis::default()
                .bounds(0.0, 1.0)
                .title("value")
                .labels(Labels::Auto(5)),
        );
        for width in 0..12 {
            for height in 0..6 {
                let rendered = rows(&chart.clone().width(width).height(height), 12);
                assert!(rendered.len() <= height);
                assert!(rendered.iter().all(|r| rich::cells::cell_len(r) <= width));
            }
        }
        // Two rows: as in ratatui, the x labels keep their row and the x
        // axis line goes; the y labels are cut to a third of the width.
        assert_eq!(
            rows(&chart.clone().width(8).height(2), 8),
            vec![" 0│ooooo", "   0 0.8"]
        );
        // Two columns: no room for the y labels.
        assert_eq!(
            rows(&chart.clone().width(2).height(3), 2),
            vec!["│o", "└─", " 0"]
        );
        // Three rows of plot fit the titles; two do not.
        assert_eq!(
            rows(&chart.clone().width(12).height(5), 12),
            vec![
                "  1│value oo",
                "0.6│  oooo  ",
                "  0│oo  time",
                "   └────────",
                "    0  0.6 1",
            ]
        );
        let short = rows(&chart.width(12).height(4), 12).concat();
        assert!(!short.contains("time") && !short.contains("value"));
    }

    #[test]
    fn ascii_consoles_draw_ascii_axes_and_legends() {
        let console = Console::builder()
            .width(10)
            .color_system(None)
            .ascii_only(true)
            .build();
        let chart = Chart::new(vec![Dataset::new([(0.0, 0.0), (1.0, 1.0)])
            .name("é")
            .graph_type(GraphType::Line)])
        .x_axis(
            Axis::default()
                .bounds(0.0, 1.0)
                .labels(Labels::Values(vec![])),
        )
        .y_axis(
            Axis::default()
                .bounds(0.0, 1.0)
                .labels(Labels::Values(vec![])),
        )
        .hidden_legend_fraction(1.0, 1.0)
        .height(6);
        assert_eq!(
            console.render_export(&chart),
            concat!(
                "|      +-+\n",
                "|     *|?|\n",
                "|   ** +-+\n",
                "| **      \n",
                "|*        \n",
                "+---------\n",
            )
        );
    }

    #[test]
    fn axes_without_labels_reserve_no_room_for_them() {
        let chart = Chart::new(vec![dot(&[(0.0, 0.0)])])
            .x_axis(
                Axis::default()
                    .bounds(0.0, 1.0)
                    .labels(Labels::Values(vec![])),
            )
            .y_axis(
                Axis::default()
                    .bounds(0.0, 1.0)
                    .labels(Labels::Text(vec![])),
            )
            .width(10)
            .height(4);
        assert_eq!(
            rows(&chart, 10),
            vec!["│         ", "│         ", "│o        ", "└─────────"]
        );
    }
}

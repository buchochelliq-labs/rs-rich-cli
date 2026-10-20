//! An x–y chart laid out as ratatui's `Chart` widget: datasets drawn on a
//! [`Canvas`](super::Canvas), two axes with titles and labels, and a boxed
//! legend.

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, HorizontalAlign, Renderable, Segment, Style};

use super::plot_canvas::{is_ascii, raster, spec_style, CellGrid, Marker, Op, View};
use super::{cell_units, cells, lines_to_segments, series_key, Scale, ValueFormat};
use crate::fidelity::ascii_text;

mod stacked;

/// Rows a [`Chart`] takes when it is given no height and [`Chart::height`]
/// is not set.
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
    /// Console markup spread evenly, as [`Text`](Self::Text) is:
    /// `"[bold]0[/]"` is a bold label, as a ratatui label of styled spans
    /// is. The markup's styles go over the axis style.
    Markup(Vec<String>),
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
    labels_align: Option<HorizontalAlign>,
    scale: AxisScale,
    style: Option<String>,
    label_format: ValueFormat,
    /// Behind one pointer, as a [`Facet`](super::Facet) holds a chart and
    /// its two axes, and must stay small.
    grid: Option<Box<GridLines>>,
}

/// An [`Axis`]'s grid lines: their values, style and marker.
#[derive(Clone, Debug, Default, PartialEq)]
struct GridLines {
    values: Vec<f64>,
    style: Option<String>,
    marker: Option<Marker>,
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

    /// How the labels sit, as ratatui's `labels_alignment`.
    ///
    /// - **On the y axis**, in the column left of the axis line. By
    ///   default [`Labels::Auto`] and [`Labels::Values`] are right-aligned
    ///   and [`Labels::Text`] and [`Labels::Markup`] left-aligned, as in
    ///   ratatui.
    /// - **On the x axis**, the first text label against the y axis line:
    ///   `Left` (the default) ends it under the line, `Center` centres it
    ///   on the line, `Right` starts it there. A label placed at a value
    ///   ([`Labels::Auto`], [`Labels::Values`]) starts at its value
    ///   (`Left`), is centred on it (the default) or ends at it (`Right`).
    pub fn labels_align(mut self, align: HorizontalAlign) -> Self {
        self.labels_align = Some(align);
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

    /// Grid lines across the plot at these values, in the axis's
    /// [`grid_style`](Self::grid_style) (else its [`style`](Self::style)),
    /// drawn under the datasets with the axis's
    /// [`grid_marker`](Self::grid_marker) (else the chart's
    /// [`marker`](Chart::marker), else the first dataset's, else Braille):
    /// the x axis's run up the plot, the y axis's along it. Values the axis
    /// cannot show (outside its bounds, or not positive on a log axis) are
    /// left out. With [`Layering::PerDataset`] each line is a layer of its
    /// own, the y axis's first, and the datasets are drawn over them, so a
    /// line crossing another and a trace crossing a line break what they
    /// cross, as datasets drawn for the purpose do in ratatui (scope-tui's
    /// reference lines).
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, Marker};
    ///
    /// let console = Console::builder().width(9).color_system(None).build();
    /// let chart = Chart::new(vec![Dataset::new([(2.0, 1.0)]).marker(Marker::Ascii('o'))])
    ///     .x_axis(Axis::default().bounds(0.0, 8.0).grid([4.0]))
    ///     .y_axis(Axis::default().bounds(0.0, 2.0).grid([0.0]))
    ///     .width(9)
    ///     .height(3);
    /// // The point at (2, 1); the grid at x = 4 and y = 0, in the axis style.
    /// assert_eq!(console.render_export(&chart), "    o    \n  o o    \nooooooooo\n");
    /// ```
    pub fn grid(mut self, values: impl IntoIterator<Item = f64>) -> Self {
        self.grid_lines().values = values.into_iter().collect();
        self
    }

    /// The style of the [`grid`](Self::grid) lines: a theme key or a
    /// definition (default: the axis's [`style`](Self::style), else
    /// `chart.axis`), so the lines can be fainter than the axis line and
    /// labels.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, Labels, Marker};
    ///
    /// let console = Console::builder().width(5).force_terminal(true).build();
    /// let chart = Chart::new(Vec::<Dataset>::new())
    ///     .marker(Marker::Ascii('.'))
    ///     .x_axis(Axis::default().bounds(0.0, 4.0))
    ///     .y_axis(
    ///         Axis::default()
    ///             .bounds(0.0, 1.0)
    ///             .labels(Labels::Text(vec!["0".into(), "1".into()]))
    ///             .style("bold")
    ///             .grid([0.0])
    ///             .grid_style("dim"),
    ///     )
    ///     .width(5)
    ///     .height(2);
    /// // The labels and the axis line bold, the grid line dim.
    /// assert_eq!(
    ///     console.render_export(&chart),
    ///     "\u{1b}[1m1│\u{1b}[0m   \n\u{1b}[1m0│\u{1b}[0m\u{1b}[2m...\u{1b}[0m\n"
    /// );
    /// ```
    pub fn grid_style(mut self, style: impl Into<String>) -> Self {
        self.grid_lines().style = Some(style.into());
        self
    }

    /// The marker the [`grid`](Self::grid) lines are drawn with (default:
    /// the chart's [`marker`](Chart::marker), else the first dataset's,
    /// else [`Marker::Braille`]).
    pub fn grid_marker(mut self, marker: Marker) -> Self {
        self.grid_lines().marker = Some(marker);
        self
    }

    fn grid_lines(&mut self) -> &mut GridLines {
        self.grid.get_or_insert_with(Default::default)
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

    fn resolve(&self, data: impl Iterator<Item = f64>, console: &Console, ascii: bool) -> Resolved {
        let scale = self.span(data);
        let text = |s: &str| {
            if ascii && !s.is_ascii() {
                ascii_text(s)
            } else {
                s.to_string()
            }
        };
        let plain = |s: &str| Label::plain(text(s));
        let labels = match &self.labels {
            Labels::None => Placed::None,
            Labels::Text(labels) => Placed::Even(labels.iter().map(|l| plain(l)).collect()),
            Labels::Markup(labels) => Placed::Even(
                labels
                    .iter()
                    .map(|l| Label::markup(console, l, ascii))
                    .collect(),
            ),
            _ => Placed::At(
                self.values(&scale)
                    .into_iter()
                    .filter_map(|v| Some((self.map(v)?, plain(&self.label_format.format(v)))))
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
    At(Vec<(f64, Label)>),
    /// Labels spread evenly.
    Even(Vec<Label>),
}

/// An axis label: runs of text, each in its own style over the axis style
/// (`None`: the axis style alone).
#[derive(Clone, Debug, Default)]
struct Label {
    runs: Vec<(String, Option<Style>)>,
}

impl Label {
    fn plain(text: String) -> Label {
        Label {
            runs: vec![(text, None)],
        }
    }

    /// The first line of console `markup`, as styled runs; markup that does
    /// not parse is plain text.
    fn markup(console: &Console, markup: &str, ascii: bool) -> Label {
        let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
        let mut options = console.options().update_width(cells(text.plain()).max(1));
        options.no_wrap = Some(true);
        let mut runs = Vec::new();
        for segment in console.render(&text, Some(&options)) {
            if segment.control || segment.text.is_empty() {
                continue;
            }
            if segment.text.contains('\n') {
                break;
            }
            let run = if ascii && !segment.text.is_ascii() {
                ascii_text(&segment.text)
            } else {
                segment.text
            };
            runs.push((run, segment.style));
        }
        Label { runs }
    }

    fn width(&self) -> usize {
        self.runs.iter().map(|(text, _)| cells(text)).sum()
    }
}

/// A run's style over the axis style.
fn over(base: &Option<Style>, own: &Option<Style>) -> Option<Style> {
    match (base, own) {
        (_, None) => base.clone(),
        (None, Some(own)) => Some(own.clone()),
        (Some(base), Some(own)) => Some(base.combine(own)),
    }
}

impl Resolved {
    /// `axis`'s grid values that this span shows, mapped.
    fn grid(&self, axis: &Axis) -> Vec<f64> {
        let (lo, hi) = (self.scale.min(), self.scale.max());
        let slack = (hi - lo).abs() * 1e-9;
        axis.grid
            .iter()
            .flat_map(|lines| lines.values.iter())
            .filter_map(|v| axis.map(*v))
            .filter(|m| lo.min(hi) - slack <= *m && *m <= lo.max(hi) + slack)
            .collect()
    }

    fn has_labels(&self) -> bool {
        !matches!(self.labels, Placed::None)
    }

    fn texts(&self) -> Vec<&Label> {
        match &self.labels {
            Placed::None => Vec::new(),
            Placed::At(labels) => labels.iter().map(|(_, l)| l).collect(),
            Placed::Even(labels) => labels.iter().collect(),
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
    /// Points joined by lines, as [`Line`](Self::Line), with the area
    /// between the lines and [`Dataset::fill_to`] filled in (ratatui's
    /// `GraphType::Area`, 0.30.1). A level the axis cannot show (not
    /// positive on a log axis) fills to the bottom.
    Area,
}

/// One named set of `(x, y)` points in a [`Chart`].
#[derive(Clone, Debug, PartialEq)]
pub struct Dataset {
    name: String,
    points: Vec<(f64, f64)>,
    graph_type: GraphType,
    marker: Marker,
    style: Option<String>,
    fill_to: f64,
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
            fill_to: 0.0,
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

    /// The level a [`GraphType::Area`] fills to (default 0): below the
    /// points that are above it, above those below it. ratatui calls it
    /// `fill_to_y`.
    pub fn fill_to(mut self, y: f64) -> Self {
        self.fill_to = y;
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

/// What a [`Chart`] does with a line, bar or area segment that has an end
/// outside the axes' bounds.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Axis, Chart, Dataset, GraphType, Marker, OutOfBounds};
///
/// let console = Console::builder().width(5).color_system(None).build();
/// // From (0, 0) to (8, 2): the second end is past the right edge.
/// let line = Dataset::new([(0.0, 0.0), (8.0, 2.0)])
///     .graph_type(GraphType::Line)
///     .marker(Marker::Ascii('*'));
/// let chart = Chart::new(vec![line])
///     .x_axis(Axis::default().bounds(0.0, 4.0))
///     .y_axis(Axis::default().bounds(0.0, 2.0))
///     .width(5)
///     .height(3);
/// assert_eq!(console.render_export(&chart), "     \n  ***\n**   \n");
/// // ratatui 0.29 drops it; only the point inside is drawn.
/// let dropped = chart.out_of_bounds(OutOfBounds::Drop);
/// assert_eq!(console.render_export(&dropped), "     \n     \n*    \n");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OutOfBounds {
    /// Draw the part inside the bounds (the default; ratatui's since
    /// 0.30).
    #[default]
    Clip,
    /// Leave the whole segment out, as ratatui 0.29 does. Points outside
    /// are never drawn either way.
    Drop,
}

/// How a [`Chart`] puts its datasets together where they share a cell.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Axis, Chart, Dataset, Layering, Marker};
///
/// let console = Console::builder().width(1).color_system(None).build();
/// // Two Braille dots in one cell, from two datasets.
/// let chart = Chart::new(vec![
///     Dataset::new([(0.0, 1.0)]).marker(Marker::Braille),
///     Dataset::new([(0.0, 0.0)]).marker(Marker::Braille),
/// ])
/// .x_axis(Axis::default().bounds(0.0, 1.0))
/// .y_axis(Axis::default().bounds(0.0, 1.0))
/// .width(1)
/// .height(1);
/// assert_eq!(console.render_export(&chart), "⡁\n");
/// let layered = chart.layering(Layering::PerDataset);
/// assert_eq!(console.render_export(&layered), "⡀\n");
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layering {
    /// Datasets with the same marker share the dots of a cell, which takes
    /// the style of the last dataset to set one (the default).
    #[default]
    Merge,
    /// Each dataset is drawn over the ones before it, as ratatui 0.29
    /// draws a canvas per dataset: a cell a later dataset draws in shows
    /// only its dots, in its style.
    PerDataset,
}

/// Datasets on two axes, laid out as ratatui's `Chart`, so a ratatui app
/// ports with the same look.
///
/// - The x axis's labels take the bottom row and its line (`─`) the row
///   above; the y axis's labels take the left (at most a third of the
///   width) and its line (`│`) the column after, meeting at `└`. An axis
///   draws its line only when it has [`Labels`].
/// - The datasets are drawn in order on a [`Canvas`](super::Canvas) filling
///   the rest, each with its marker. Datasets with the same marker share
///   the dots of a cell, which takes the later one's style;
///   [`Layering::PerDataset`] draws each over the ones before. A line
///   leaving the bounds is clipped at them ([`OutOfBounds`]).
/// - The y axis's title sits on the plot's top row, from its left; the x
///   axis's at the right end of the plot's bottom row.
/// - The legend is a box listing the named datasets, each in its style, in
///   a corner of the plot ([`LegendPosition::TopRight`] by default). It
///   hides itself when it would take more than a share of the plot: a
///   quarter of the width and a quarter of the height by default. Its box
///   is in the chart's [`style`](Self::style), or else the x axis's, unless
///   [`legend_style`](Self::legend_style) says otherwise, and it clears the
///   plot under it unless [`legend_clear`](Self::legend_clear) is false.
/// - It fills the height it is given, as ratatui's fills its area: a
///   `Layout` region, a `Panel` with a height, an intuiTUIve node through
///   `renderable(..)`. [`height`](Self::height) fixes it instead; given no
///   height, it is 15 rows. It is as wide as the width it is given (or
///   [`width`](Self::width)). As in ratatui, a short chart gives up the x
///   axis line, then the x labels; a narrow one cuts the y labels to a
///   third of the width; the titles and the legend show only when the
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
    // Boxed strings keep a chart small enough for a `Facet`.
    legend_style: Option<Box<str>>,
    style: Option<Box<str>>,
    legend_clear: bool,
    out_of_bounds: OutOfBounds,
    layering: Layering,
    marker: Option<Marker>,
    width: Option<usize>,
    height: Option<usize>,
    stacked: bool,
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
            legend_style: None,
            style: None,
            legend_clear: true,
            out_of_bounds: OutOfBounds::Clip,
            layering: Layering::Merge,
            marker: None,
            width: None,
            height: None,
            stacked: false,
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

    /// The chart's own style, as ratatui's `Chart::style`: a theme key or a
    /// definition put under every cell of the chart (a background fills
    /// it), and the style of the legend's box unless
    /// [`legend_style`](Self::legend_style) gives one. Without it the chart
    /// has no style of its own and the legend's box takes the x axis's.
    /// `"none"` is the terminal's own colours, so the legend's box is
    /// unstyled too: [`legend_unstyled`](Self::legend_unstyled) says the
    /// same by name.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, Marker};
    ///
    /// let console = Console::builder().width(6).force_terminal(true).build();
    /// let chart = Chart::new(vec![Dataset::new([]).name("a")])
    ///     .x_axis(Axis::default().bounds(0.0, 1.0))
    ///     .y_axis(Axis::default().bounds(0.0, 1.0))
    ///     .hidden_legend_fraction(1.0, 1.0)
    ///     .style("on blue")
    ///     .width(6)
    ///     .height(3);
    /// let out = console.render_export(&chart);
    /// // Every cell on blue, the legend's box too.
    /// assert!(out.starts_with("\u{1b}[44m"));
    /// ```
    pub fn style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into().into_boxed_str());
        self
    }

    /// The style of the legend's box: a theme key or a definition (default:
    /// the chart's [`style`](Self::style) when it has one, else the x
    /// axis's style). The names keep their datasets' styles.
    ///
    /// ```
    /// use rich_ext::chart::{Chart, Dataset};
    ///
    /// let chart = Chart::new(vec![Dataset::new([(0.0, 1.0)]).name("left")])
    ///     .legend_style("dim");
    /// ```
    pub fn legend_style(mut self, style: impl Into<String>) -> Self {
        self.legend_style = Some(style.into().into_boxed_str());
        self
    }

    /// The legend's box in the terminal's own colours, not the x axis's
    /// style: a ratatui chart whose legend style is the default (scope-tui
    /// sets `legend_style(Style::default())`). The same as
    /// [`legend_style`](Self::legend_style)`("none")`, by name; the names
    /// keep their datasets' styles.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset};
    ///
    /// let console = Console::builder().width(6).force_terminal(true).build();
    /// let chart = Chart::new(vec![Dataset::new([]).name("a").style("none")])
    ///     .x_axis(Axis::default().bounds(0.0, 1.0).style("red"))
    ///     .y_axis(Axis::default().bounds(0.0, 1.0))
    ///     .hidden_legend_fraction(1.0, 1.0)
    ///     .width(6)
    ///     .height(3);
    /// // The box in the x axis's red, then in no colour at all.
    /// assert!(console.render_export(&chart).contains("\u{1b}[31m┌"));
    /// assert!(!console.render_export(&chart.legend_unstyled()).contains('\u{1b}'));
    /// ```
    pub fn legend_unstyled(self) -> Self {
        self.legend_style("none")
    }

    /// Whether the legend clears the plot under it (default true). When
    /// false, as in ratatui, what the datasets drew inside the box stays,
    /// unstyled, around the names.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, Marker};
    ///
    /// let console = Console::builder().width(8).color_system(None).build();
    /// let wall = (0..8).flat_map(|x| (0..4).map(move |y| (f64::from(x), f64::from(y))));
    /// let dots = Dataset::new(wall).name("a").marker(Marker::Ascii('.'));
    /// let chart = Chart::new(vec![dots, Dataset::new([]).name("bb")])
    ///     .x_axis(Axis::default().bounds(0.0, 7.0))
    ///     .y_axis(Axis::default().bounds(0.0, 3.0))
    ///     .hidden_legend_fraction(1.0, 1.0)
    ///     .width(8)
    ///     .height(4);
    /// assert_eq!(
    ///     console.render_export(&chart),
    ///     "....┌──┐\n....│a │\n....│bb│\n....└──┘\n"
    /// );
    /// assert_eq!(
    ///     console.render_export(&chart.legend_clear(false)),
    ///     "....┌──┐\n....│a.│\n....│bb│\n....└──┘\n"
    /// );
    /// ```
    pub fn legend_clear(mut self, clear: bool) -> Self {
        self.legend_clear = clear;
        self
    }

    /// What to do with a segment that has an end outside the bounds
    /// (default [`OutOfBounds::Clip`]).
    pub fn out_of_bounds(mut self, out_of_bounds: OutOfBounds) -> Self {
        self.out_of_bounds = out_of_bounds;
        self
    }

    /// How datasets that share a cell are put together (default
    /// [`Layering::Merge`]).
    pub fn layering(mut self, layering: Layering) -> Self {
        self.layering = layering;
        self
    }

    /// The marker the axes' [grid lines](Axis::grid) are drawn with when
    /// an axis gives none ([`Axis::grid_marker`]). Without it, the first
    /// dataset's marker, or [`Marker::Braille`] when there is no dataset,
    /// so a chart's grid looks the same before its first data as after.
    ///
    /// ```
    /// use rich::Console;
    /// use rich_ext::chart::{Axis, Chart, Dataset, Marker};
    ///
    /// let console = Console::builder().width(4).color_system(None).build();
    /// let empty = Chart::new(Vec::<Dataset>::new())
    ///     .x_axis(Axis::default().bounds(0.0, 1.0))
    ///     .y_axis(Axis::default().bounds(-1.0, 1.0).grid([0.0]))
    ///     .width(4)
    ///     .height(1);
    /// // Braille with no datasets, not dots.
    /// assert_eq!(console.render_export(&empty), "⠒⠒⠒⠒\n");
    /// let ascii = empty.marker(Marker::Ascii('-'));
    /// assert_eq!(console.render_export(&ascii), "----\n");
    /// ```
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = Some(marker);
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Height in rows, axes and labels included (at most 65535), whatever
    /// height it is given. Without it, the chart fills the height it is
    /// given, or takes 15 rows.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows.min(MAX_HEIGHT));
        self
    }

    fn grid(&self, console: &Console, options: &ConsoleOptions) -> CellGrid {
        let w = self
            .width
            .map_or(options.max_width, |v| v.min(options.max_width))
            .min(MAX_HEIGHT);
        let h = self
            .height
            .or(options.height)
            .unwrap_or(DEFAULT_HEIGHT)
            .min(MAX_HEIGHT);
        let mut grid = CellGrid::new(w, h);
        if w == 0 || h == 0 {
            return grid;
        }
        let ascii = is_ascii(console, options);
        let points = || self.datasets.iter().flat_map(|d| d.points.iter());
        let x = self.x_axis.resolve(points().map(|p| p.0), console, ascii);
        let y = self.y_axis.resolve(points().map(|p| p.1), console, ascii);
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
        let x_align = self.x_axis.labels_align;
        let mut left = left_width(w, &x, &y, x_align);
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
                        let len = label.width();
                        if len > w {
                            continue;
                        }
                        let tick = gx + x.cell(*value, gw, per_x);
                        let start = match x_align {
                            Some(HorizontalAlign::Left) => tick,
                            Some(HorizontalAlign::Right) => (tick + 1).saturating_sub(len),
                            _ => tick.saturating_sub(len / 2),
                        }
                        .min(w - len);
                        if start < free {
                            continue;
                        }
                        put_runs(&mut grid, row, start, w, label, &x_style);
                        free = start + len + 1;
                    }
                }
                Placed::Even(labels) if labels.len() >= 2 => {
                    let n = labels.len();
                    let between = gw / n;
                    let put = |grid: &mut CellGrid, col, width, label: &Label, align| {
                        put_aligned(grid, row, col, width, label, &x_style, align);
                    };
                    // The first label against the y axis line, as ratatui
                    // places it for each alignment.
                    let (col, width, align) = match x_align {
                        Some(HorizontalAlign::Center) => {
                            (0, gx + between.min(labels[0].width()), Align::Centre)
                        }
                        Some(HorizontalAlign::Right) => {
                            (gx.saturating_sub(1), between + 1, Align::Left)
                        }
                        _ => (0, gx, Align::Right),
                    };
                    put(&mut grid, col, width, &labels[0], align);
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
            let align = self.y_axis.labels_align.map(Align::from);
            match &y.labels {
                Placed::At(labels) => {
                    let mut used = vec![false; gh];
                    for (value, label) in labels {
                        let row = gh - 1 - y.cell(*value, gh, per_y);
                        if !std::mem::replace(&mut used[row], true) {
                            let align = align.unwrap_or(Align::Right);
                            put_aligned(&mut grid, row, 0, width, label, &y_style, align);
                        }
                    }
                }
                Placed::Even(labels) => {
                    let n = labels.len();
                    for (i, label) in labels.iter().enumerate() {
                        let dy = if n > 1 { i * (gh - 1) / (n - 1) } else { 0 };
                        if dy < gh {
                            let row = gh - 1 - dy;
                            let align = align.unwrap_or(Align::Left);
                            put_aligned(&mut grid, row, 0, width, label, &y_style, align);
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
        let plot = raster(&self.ops(&x, &y), &view, console);
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
                let style = match (self.legend_style.as_deref(), self.style.as_deref()) {
                    (Some(spec), _) | (None, Some(spec)) => spec_style(console, spec),
                    (None, None) => x_style.clone(),
                };
                let box_ = LegendBox {
                    at: (top, col),
                    inner,
                    border,
                    style,
                    clear: self.legend_clear,
                };
                box_.draw(&mut grid, &named);
            }
        }
        if let Some(style) = self.style.as_deref().and_then(|s| spec_style(console, s)) {
            grid.underlay(&style);
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
    fn ops(&self, x: &Resolved, y: &Resolved) -> Vec<Op> {
        let within = |v: f64, s: &Scale| {
            let (a, b) = (s.min(), s.max());
            a.min(b) <= v && v <= a.max(b)
        };
        let inside = |(px, py): (f64, f64)| within(px, &x.scale) && within(py, &y.scale);
        // Whether a segment from `a` to `b` is drawn (clipped) at all.
        let keep = |a: (f64, f64), b: (f64, f64)| {
            self.out_of_bounds == OutOfBounds::Clip || (inside(a) && inside(b))
        };
        let mut ops = Vec::new();
        // The grid lines first, under the datasets: the y axis's, then the
        // x axis's, each its own layer when the datasets are (scope-tui
        // lists its reference lines in that order).
        let fallback = self
            .marker
            .or(self.datasets.first().map(|d| d.marker))
            .unwrap_or(Marker::Braille);
        let lines = |axis: &Axis| {
            let grid = axis.grid.as_deref();
            let spec = grid
                .and_then(|g| g.style.clone())
                .or_else(|| axis.style.clone())
                .unwrap_or_else(|| "chart.axis".into());
            let marker = grid.and_then(|g| g.marker).unwrap_or(fallback);
            (spec, marker)
        };
        let (left, right) = (x.scale.min(), x.scale.max());
        let (bottom, top) = (y.scale.min(), y.scale.max());
        let (y_spec, y_marker) = lines(&self.y_axis);
        let (x_spec, x_marker) = lines(&self.x_axis);
        let across = y.grid(&self.y_axis).into_iter().map(|at| {
            let line = Op::Line((left, at), (right, at), y_spec.clone());
            (line, y_marker)
        });
        let up = x.grid(&self.x_axis).into_iter().map(|at| {
            let line = Op::Line((at, bottom), (at, top), x_spec.clone());
            (line, x_marker)
        });
        let mut marker = None;
        for (line, line_marker) in across.chain(up) {
            if marker != Some(line_marker) {
                ops.push(Op::Marker(line_marker));
                marker = Some(line_marker);
            }
            ops.push(line);
            if self.layering == Layering::PerDataset {
                ops.push(Op::Layer);
            }
        }
        for (nth, index) in self.draw_order().into_iter().enumerate() {
            let dataset = &self.datasets[index];
            if nth > 0 && self.layering == Layering::PerDataset {
                ops.push(Op::Layer);
            }
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
                            if keep(*a, *b) {
                                ops.push(Op::Line(*a, *b, style.clone()));
                            }
                        }
                    }
                }
                GraphType::Bar => {
                    let base = match self.y_axis.scale {
                        AxisScale::Linear => 0.0,
                        AxisScale::Log => y.scale.min(),
                    };
                    for (px, py) in shown().filter(|&p| keep((p.0, base), p)) {
                        ops.push(Op::Line((px, base), (px, py), style.clone()));
                    }
                }
                GraphType::Area => {
                    let level = self.y_axis.map(dataset.fill_to).unwrap_or(y.scale.min());
                    for pair in mapped.windows(2) {
                        if let [Some(a), Some(b)] = pair {
                            if keep(*a, *b) {
                                ops.push(Op::Area(*a, *b, level, style.clone()));
                            }
                        }
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
fn left_width(width: usize, x: &Resolved, y: &Resolved, x_align: Option<HorizontalAlign>) -> usize {
    let mut widest = y.texts().iter().map(|l| l.width()).max().unwrap_or(0);
    if let Some(first) = x.texts().first() {
        let first = first.width();
        let hangs = match (&x.labels, x_align) {
            // The label ends under the y axis line, when there is one.
            (Placed::Even(_), None | Some(HorizontalAlign::Left)) => {
                first.saturating_sub(usize::from(y.has_labels()))
            }
            (Placed::Even(_), Some(HorizontalAlign::Right)) => 0,
            (_, Some(HorizontalAlign::Left)) => 0,
            (_, Some(HorizontalAlign::Right)) => first.saturating_sub(1),
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

impl From<HorizontalAlign> for Align {
    fn from(align: HorizontalAlign) -> Align {
        match align {
            HorizontalAlign::Left => Align::Left,
            HorizontalAlign::Center => Align::Centre,
            HorizontalAlign::Right => Align::Right,
        }
    }
}

/// `label`'s runs from `col` of `row`, over `base`, stopping before `end`.
fn put_runs(
    grid: &mut CellGrid,
    row: usize,
    col: usize,
    end: usize,
    label: &Label,
    base: &Option<Style>,
) {
    let mut at = col;
    for (text, style) in &label.runs {
        if at >= end {
            break;
        }
        grid.put_text(row, at, text, over(base, style), end);
        at += cells(text);
    }
}

/// `label` in the `width` cells from `col` of `row`, aligned; when it is
/// wider it is cut on the right, or on the left when right-aligned.
fn put_aligned(
    grid: &mut CellGrid,
    row: usize,
    col: usize,
    width: usize,
    label: &Label,
    base: &Option<Style>,
    align: Align,
) {
    if width == 0 {
        return;
    }
    let len = label.width();
    let end = col + width;
    if len <= width {
        let offset = match align {
            Align::Left => 0,
            Align::Centre => (width - len) / 2,
            Align::Right => width - len,
        };
        put_runs(grid, row, col + offset, end, label, base);
    } else if let Align::Right = align {
        // The last `width` cells, each in its run's style.
        let units: Vec<(String, Option<Style>)> = label
            .runs
            .iter()
            .flat_map(|(text, style)| {
                let style = over(base, style);
                cell_units(text)
                    .into_iter()
                    .map(move |u| (u, style.clone()))
            })
            .collect();
        for (i, (unit, style)) in units[len - width..].iter().enumerate() {
            let unit = if i == 0 && unit.is_empty() { " " } else { unit };
            grid.set(row, col + i, unit.to_string(), style.clone());
        }
    } else {
        put_runs(grid, row, col, end, label, base);
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

/// The legend's box: its top left cell, the width inside it, its border
/// glyphs and style, and whether it clears what is under it.
struct LegendBox {
    at: (usize, usize),
    inner: usize,
    border: [char; 6],
    style: Option<Style>,
    clear: bool,
}

impl LegendBox {
    /// The box, listing `named`, each name in its style.
    fn draw(&self, grid: &mut CellGrid, named: &[(String, Option<Style>)]) {
        let ((top, left), inner, border) = (self.at, self.inner, &self.style);
        let [tl, tr, bl, br, h, v] = self.border;
        let edge = |a: char, b: char| format!("{a}{}{b}", h.to_string().repeat(inner));
        let right = left + inner + 2;
        grid.put_text(top, left, &edge(tl, tr), border.clone(), right);
        for (i, (name, style)) in named.iter().enumerate() {
            let row = top + 1 + i;
            grid.set(row, left, v.to_string(), border.clone());
            for col in left + 1..right - 1 {
                if self.clear {
                    grid.set(row, col, " ".to_string(), None);
                } else {
                    grid.restyle(row, col, None);
                }
            }
            grid.put_text(row, left + 1, name, style.clone(), right - 1);
            grid.set(row, right - 1, v.to_string(), border.clone());
        }
        let bottom = top + named.len() + 1;
        grid.put_text(bottom, left, &edge(bl, br), border.clone(), right);
    }
}

impl Axis {
    /// `x (t) from 0 to 10, log scale`: the axis's title and range (its
    /// bounds, else the data's), for accessible text.
    fn describe(&self, name: &str, data: impl Iterator<Item = f64>) -> String {
        let mut out = name.to_string();
        if let Some(title) = self.title.as_deref().filter(|t| !t.is_empty()) {
            out.push_str(&format!(" ({title})"));
        }
        let shown = data.filter(|v| self.map(*v).is_some());
        let span = match self.bounds {
            Some([a, b]) => Some((a.min(b), a.max(b))),
            None => super::describe::extent(shown),
        };
        if let Some((lo, hi)) = span {
            out.push(' ');
            out.push_str(&super::describe::range(lo, hi, self.label_format));
        }
        if self.scale == AxisScale::Log {
            out.push_str(", log scale");
        }
        out
    }
}

/// `Chart, 2 datasets, x (t) from 0 to 10, y (%) from 0 to 4`, then a line
/// per dataset: its name (`dataset 2` without one), how it is drawn, and
/// its facts (`3 points, min 0, max 4, last 1, trend up`).
impl crate::a11y::AccessibleText for Chart {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::{counted, series};
        if self.datasets.is_empty() {
            return "Chart, no datasets".to_string();
        }
        let points = || self.datasets.iter().flat_map(|d| d.points.iter());
        let view = self.stacked_view();
        let stacked_points = || view.datasets.iter().flat_map(|d| d.points.iter());
        let mut out = vec![format!(
            "Chart, {}, {}, {}{}",
            counted(self.datasets.len(), "dataset", "datasets"),
            self.x_axis.describe("x", points().map(|p| p.0)),
            self.y_axis.describe("y", stacked_points().map(|p| p.1)),
            if self.stacks() { ", areas stacked" } else { "" },
        )];
        for (index, d) in self.datasets.iter().enumerate() {
            let name = if d.name.is_empty() {
                format!("dataset {}", index + 1)
            } else {
                d.name.clone()
            };
            let kind = match d.graph_type {
                GraphType::Scatter => "points",
                GraphType::Line => "line",
                GraphType::Bar => "bars",
                GraphType::Area => "area",
            };
            let facts = series(&d.points, self.y_axis.label_format, ("point", "points"));
            out.push(format!("{name}, {kind}: {facts}"));
        }
        out.join("\n")
    }
}

impl Chart {
    /// The y axis's bounds, else the finite y values' range: what a shared
    /// scale unifies.
    pub(crate) fn y_extent(&self) -> Option<(f64, f64)> {
        if let Some([a, b]) = self.y_axis.bounds {
            return Some((a.min(b), a.max(b)));
        }
        let view = self.stacked_view();
        let ys = view
            .datasets
            .iter()
            .flat_map(|d| d.points.iter().map(|p| p.1));
        super::describe::extent(ys.filter(|v| self.y_axis.map(*v).is_some()))
    }

    /// This chart with its y axis's bounds set.
    pub(crate) fn with_y_bounds(mut self, min: f64, max: f64) -> Self {
        self.y_axis.bounds = Some([min, max]);
        self
    }
}

impl Renderable for Chart {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let view = self.stacked_view();
        lines_to_segments(view.grid(console, options).into_lines(), options.max_width)
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

    #[test]
    fn accessible_text_names_the_axes_and_each_dataset() {
        use crate::a11y::AccessibleText;
        let cpu = Dataset::new([(0.0, 0.0), (5.0, 4.0), (10.0, 1.0)])
            .name("cpu")
            .graph_type(GraphType::Line);
        let chart = Chart::new(vec![cpu, Dataset::new([(1.0, 2.0)])])
            .x_axis(Axis::default().bounds(0.0, 10.0).title("t"))
            .y_axis(Axis::default().bounds(0.0, 4.0).title("%"));
        assert_eq!(
            chart.accessible_text(80),
            "Chart, 2 datasets, x (t) from 0 to 10, y (%) from 0 to 4\n\
             cpu, line: 3 points, min 0, max 4, last 1, trend up\n\
             dataset 2, points: 1 point, min 2, max 2, last 2"
        );
        // Without bounds, the data's range; a log axis says so.
        let log = Chart::new(vec![Dataset::new([(1.0, 1.0), (2.0, 100.0)])
            .name("hz")
            .graph_type(GraphType::Area)])
        .y_axis(Axis::default().scale(AxisScale::Log));
        assert_eq!(
            log.accessible_text(80),
            "Chart, 1 dataset, x from 1 to 2, y from 1 to 100, log scale\n\
             hz, area: 2 points, min 1, max 100, last 100, trend up"
        );
        assert_eq!(Chart::default().accessible_text(80), "Chart, no datasets");
    }

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

    /// A 5 × 3 chart on 0..=4 × 0..=2, one glyph a cell.
    fn unit_chart(datasets: Vec<Dataset>) -> Chart {
        Chart::new(datasets)
            .x_axis(Axis::default().bounds(0.0, 4.0))
            .y_axis(Axis::default().bounds(0.0, 2.0))
            .width(5)
            .height(3)
    }

    #[test]
    fn segments_leaving_the_bounds_are_clipped_by_default() {
        let line = dot(&[(0.0, 0.0), (8.0, 2.0)]).graph_type(GraphType::Line);
        let chart = unit_chart(vec![line]);
        assert_eq!(chart.out_of_bounds, OutOfBounds::Clip);
        assert_eq!(rows(&chart, 5), vec!["     ", "  ooo", "oo   "]);
        let explicit = chart.clone().out_of_bounds(OutOfBounds::Clip);
        assert_eq!(rows(&explicit, 5), rows(&chart, 5));
    }

    #[test]
    fn dropped_segments_leave_out_any_with_an_end_outside() {
        let drop = |d: Dataset| rows(&unit_chart(vec![d]).out_of_bounds(OutOfBounds::Drop), 5);
        // One end outside: only the point inside is drawn.
        let line = dot(&[(0.0, 0.0), (8.0, 2.0)]).graph_type(GraphType::Line);
        assert_eq!(drop(line), vec!["     ", "     ", "o    "]);
        // Both ends inside: drawn whole; the next segment, leaving, is not.
        let line = dot(&[(0.0, 0.0), (4.0, 2.0), (4.0, 9.0)]).graph_type(GraphType::Line);
        assert_eq!(drop(line), vec!["   oo", " oo  ", "o    "]);
        // A segment that crosses the plot with both ends outside.
        let across = dot(&[(-1.0, 1.0), (5.0, 1.0)]).graph_type(GraphType::Line);
        assert_eq!(drop(across.clone()), vec!["     "; 3]);
        assert_eq!(
            rows(&unit_chart(vec![across]), 5),
            vec!["     ", "ooooo", "     "]
        );
        // A bar from zero, when zero is below the bounds.
        let bar = |d: Dataset| {
            let chart = unit_chart(vec![d.graph_type(GraphType::Bar)])
                .y_axis(Axis::default().bounds(1.0, 3.0));
            (
                rows(&chart.clone(), 5),
                rows(&chart.out_of_bounds(OutOfBounds::Drop), 5),
            )
        };
        let (clipped, dropped) = bar(dot(&[(2.0, 3.0)]));
        assert_eq!(clipped, vec!["  o  "; 3]);
        assert_eq!(dropped, vec!["  o  ", "     ", "     "]);
        // An area segment, as a line.
        let area = dot(&[(0.0, 2.0), (8.0, 2.0)]).graph_type(GraphType::Area);
        assert_eq!(drop(area), vec!["o    ", "     ", "     "]);
    }

    /// A Braille reference line along y = 1 and a trace crossing it.
    fn crossing() -> Chart {
        Chart::new(vec![
            Dataset::new([(0.0, 1.0), (4.0, 1.0)])
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
                .style("red"),
            Dataset::new([(2.0, 0.0), (2.0, 2.0)])
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
                .style("blue"),
        ])
        .x_axis(Axis::default().bounds(0.0, 4.0))
        .y_axis(Axis::default().bounds(0.0, 2.0))
        .width(3)
        .height(1)
    }

    #[test]
    fn datasets_merge_their_dots_by_default() {
        let console = Console::builder().width(3).build();
        let chart = crossing();
        assert_eq!(chart.layering, Layering::Merge);
        let grid = chart.grid(&console, &console.options());
        // The trace's column in the middle cell, over the line's row.
        assert_eq!(grid.row_string(0), "⠒⡗⠒");
        assert_eq!(grid.style_at(0, 1), Style::parse("blue").ok());
        assert_eq!(grid.style_at(0, 0), Style::parse("red").ok());
    }

    #[test]
    fn per_dataset_layering_draws_each_over_the_ones_before() {
        let console = Console::builder().width(3).build();
        let chart = crossing().layering(Layering::PerDataset);
        let grid = chart.grid(&console, &console.options());
        // The trace's cell shows only its dots: the line breaks there.
        assert_eq!(grid.row_string(0), "⠒⡇⠒");
        assert_eq!(grid.style_at(0, 1), Style::parse("blue").ok());
        assert_eq!(grid.style_at(0, 2), Style::parse("red").ok());
        // Cells only one dataset draws in are as merged.
        let merged = crossing().grid(&console, &console.options());
        assert_eq!(grid.row_string(0)[..3], merged.row_string(0)[..3]);
    }

    #[test]
    fn the_legend_box_takes_the_x_axis_style_unless_given_its_own() {
        let console = Console::builder().width(9).build();
        let options = console.options();
        let chart = legend_chart(LegendPosition::TopRight).x_axis(Axis::default().style("magenta"));
        let grid = chart.grid(&console, &options);
        assert_eq!(grid.style_at(0, 4), Style::parse("magenta").ok());
        assert_eq!(grid.style_at(1, 4), Style::parse("magenta").ok());
        let own = chart.legend_style("green").grid(&console, &options);
        for (row, col) in [(0, 4), (0, 8), (1, 4), (1, 8), (3, 6)] {
            assert_eq!(own.style_at(row, col), Style::parse("green").ok());
        }
        // The names keep their datasets' styles.
        assert_eq!(own.style_at(1, 5), Style::parse("red").ok());
        assert_eq!(own.style_at(2, 5), Style::parse("blue").ok());
        // The text is the same either way.
        let text = |g: &CellGrid| (0..6).map(|r| g.row_string(r)).collect::<Vec<_>>();
        assert_eq!(text(&own), text(&grid));
    }

    /// An 8 × 4 wall of red dots with a legend of "a" and "bb" over it.
    fn walled() -> Chart {
        let wall = (0..8).flat_map(|x| (0..4).map(move |y| (f64::from(x), f64::from(y))));
        let dots = Dataset::new(wall)
            .name("a")
            .marker(Marker::Ascii('.'))
            .style("red");
        Chart::new(vec![dots, Dataset::new([]).name("bb").style("blue")])
            .x_axis(Axis::default().bounds(0.0, 7.0))
            .y_axis(Axis::default().bounds(0.0, 3.0))
            .hidden_legend_fraction(1.0, 1.0)
            .width(8)
            .height(4)
    }

    #[test]
    fn the_legend_clears_the_plot_under_it_by_default() {
        let chart = walled();
        assert!(chart.legend_clear);
        assert_eq!(
            rows(&chart, 8),
            vec!["....┌──┐", "....│a │", "....│bb│", "....└──┘"]
        );
        assert_eq!(rows(&chart.clone().legend_clear(true), 8), rows(&chart, 8));
    }

    #[test]
    fn an_uncleared_legend_leaves_the_plot_unstyled_around_the_names() {
        let chart = walled().legend_clear(false);
        assert_eq!(
            rows(&chart, 8),
            vec!["....┌──┐", "....│a.│", "....│bb│", "....└──┘"]
        );
        let console = Console::builder().width(8).build();
        let grid = chart.grid(&console, &console.options());
        // The dot after "a" keeps its glyph, not its style.
        assert_eq!(grid.style_at(1, 6), None);
        assert_eq!(grid.style_at(1, 5), Style::parse("red").ok());
        assert_eq!(grid.style_at(1, 2), Style::parse("red").ok());
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
    fn area_charts_fill_to_their_level() {
        let points = [(0.0, 1.0), (2.0, 3.0), (4.0, 2.0), (6.0, 4.0)];
        let area = |fill: f64| {
            Chart::new(vec![Dataset::new(points)
                .graph_type(GraphType::Area)
                .marker(Marker::Ascii('#'))
                .fill_to(fill)])
            .x_axis(Axis::default().bounds(0.0, 6.0))
            .y_axis(Axis::default().bounds(0.0, 4.0))
            .height(5)
            .width(7)
        };
        assert_eq!(
            rows(&area(0.0), 7),
            vec!["      #", "  #  ##", " ######", "#######", "#######"]
        );
        // Filled to a level above some points: the strips between the line
        // and the level, on either side of it.
        assert_eq!(
            rows(&area(2.0), 7),
            vec!["      #", "  #  ##", "#######", "#      ", "       "]
        );
        // On a log axis, a level it cannot show fills to the bottom.
        let log = Chart::new(vec![Dataset::new([(0.0, 10.0), (2.0, 10.0)])
            .graph_type(GraphType::Area)
            .marker(Marker::Ascii('#'))])
        .x_axis(Axis::default().bounds(0.0, 2.0))
        .y_axis(Axis::default().bounds(1.0, 100.0).scale(AxisScale::Log))
        .height(3)
        .width(3);
        assert_eq!(rows(&log, 3), vec!["   ", "###", "###"]);
    }

    #[test]
    fn markup_labels_keep_their_styles() {
        let chart = Chart::new(Vec::new())
            .x_axis(Axis::default().style("blue").labels(Labels::Markup(vec![
                "[bold]0[/]".into(),
                "5".into(),
                "[bold]10[/]".into(),
            ])))
            .height(3)
            .width(12);
        assert_eq!(
            rows(&chart, 12),
            vec!["            ", " ───────────", "0    5    10"]
        );
        let console = Console::builder().width(12).build();
        let grid = chart.grid(&console, &console.options());
        let bold_blue = Style::parse("bold blue").ok();
        assert_eq!(grid.style_at(2, 0), bold_blue);
        assert_eq!(grid.style_at(2, 5), Style::parse("blue").ok());
        assert_eq!(grid.style_at(2, 10), bold_blue);
        assert_eq!(grid.style_at(2, 11), bold_blue);
        // Markup that does not parse is written as it is.
        let broken = Chart::new(Vec::new())
            .y_axis(Axis::default().labels(Labels::Markup(vec!["[/x]".into(), "b".into()])))
            .height(2)
            .width(8);
        assert_eq!(rows(&broken, 8), vec!["b │     ", "[/│     "]);
    }

    #[test]
    fn labels_align_as_asked() {
        let chart = |align: Option<HorizontalAlign>| {
            let mut y = Axis::default()
                .bounds(0.0, 100.0)
                .labels(Labels::Values(vec![0.0, 50.0, 100.0]));
            if let Some(align) = align {
                y = y.labels_align(align);
            }
            Chart::new(Vec::new())
                .y_axis(y.labels(Labels::Text(vec!["0".into(), "fifty".into(), "100".into()])))
                .height(3)
                .width(15)
        };
        // Text labels are left-aligned by default, as in ratatui.
        assert_eq!(
            rows(&chart(None), 15),
            vec!["100  │         ", "fifty│         ", "0    │         "]
        );
        assert_eq!(
            rows(&chart(Some(HorizontalAlign::Right)), 15),
            vec!["  100│         ", "fifty│         ", "    0│         "]
        );
        assert_eq!(
            rows(&chart(Some(HorizontalAlign::Center)), 15),
            vec![" 100 │         ", "fifty│         ", "  0  │         "]
        );
        // Values are right-aligned by default; left on asking.
        let values = |align: Option<HorizontalAlign>| {
            let mut y = Axis::default()
                .bounds(0.0, 100.0)
                .labels(Labels::Values(vec![0.0, 100.0]));
            if let Some(align) = align {
                y = y.labels_align(align);
            }
            rows(&Chart::new(Vec::new()).y_axis(y).height(2).width(12), 12)
        };
        assert_eq!(values(None), vec!["100│        ", "  0│        "]);
        assert_eq!(
            values(Some(HorizontalAlign::Left)),
            vec!["100│        ", "0  │        "]
        );
    }

    #[test]
    fn the_first_x_label_sits_against_the_y_axis_as_asked() {
        let chart = |align: Option<HorizontalAlign>| {
            let mut x = Axis::default().labels(Labels::Text(vec!["start".into(), "end".into()]));
            if let Some(align) = align {
                x = x.labels_align(align);
            }
            let y = Axis::default().labels(Labels::Text(vec!["abc".into(), "d".into()]));
            rows(
                &Chart::new(Vec::new())
                    .x_axis(x)
                    .y_axis(y)
                    .height(4)
                    .width(16),
                16,
            )
        };
        // Left (the default): it ends under the y axis line.
        assert_eq!(chart(None)[2], "    └───────────");
        assert_eq!(chart(None)[3], "start        end");
        // Centre: about the line; right: it starts there.
        assert_eq!(chart(Some(HorizontalAlign::Center))[3], "  start      end");
        assert_eq!(chart(Some(HorizontalAlign::Right))[2], "   └────────────");
        assert_eq!(chart(Some(HorizontalAlign::Right))[3], "   start     end");
    }

    #[test]
    fn a_chart_fills_the_height_it_is_given() {
        let chart = Chart::new(vec![
            dot(&[(0.0, 0.0), (1.0, 1.0)]).graph_type(GraphType::Line)
        ])
        .x_axis(Axis::default().bounds(0.0, 1.0).labels(Labels::Auto(2)))
        .y_axis(Axis::default().bounds(0.0, 1.0).labels(Labels::Auto(2)));
        let console = Console::builder().width(20).color_system(None).build();
        let lines = console.render_lines(&chart, &console.options().update_height(7), false);
        assert_eq!(lines.len(), 7);
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.iter().map(|s| s.text.as_str()).collect())
            .collect();
        // The axes take the last two rows, the plot the five above.
        assert!(
            text[5].starts_with(" └") || text[5].contains('└'),
            "{text:?}"
        );
        assert!(text[0].contains('1') && text[4].contains('0'), "{text:?}");
        // An explicit height wins; no height at all is 15 rows.
        let fixed = chart.clone().height(4);
        let lines = console.render_lines(&fixed, &console.options().update_height(7), false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 4);
        assert_eq!(console.render_export(&chart).lines().count(), 15);
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

    #[test]
    fn grid_lines_go_under_the_datasets_and_within_the_bounds() {
        let trace = Dataset::new([(0.0, 1.0), (4.0, 1.0)])
            .graph_type(GraphType::Line)
            .marker(Marker::Ascii('o'));
        let chart = Chart::new(vec![trace])
            .x_axis(Axis::default().bounds(0.0, 4.0).grid([2.0, 9.0]))
            .y_axis(Axis::default().bounds(0.0, 2.0).grid([-1.0]))
            .layering(Layering::PerDataset)
            .width(5)
            .height(3);
        assert_eq!(rows(&chart, 5), ["  o  ", "ooooo", "  o  "]);
        // On a log axis, only positive values.
        let log = Chart::new(vec![Dataset::new([]).marker(Marker::Ascii('+'))])
            .x_axis(
                Axis::default()
                    .bounds(1.0, 100.0)
                    .scale(AxisScale::Log)
                    .grid([0.0, 10.0]),
            )
            .y_axis(Axis::default().bounds(0.0, 1.0))
            .width(5)
            .height(2);
        assert_eq!(rows(&log, 5), ["  +  ", "  +  "]);
    }

    #[test]
    fn grid_lines_take_the_axis_style() {
        let console = Console::builder().width(3).build();
        let chart = Chart::new(vec![Dataset::new([]).marker(Marker::Ascii('.'))])
            .x_axis(Axis::default().bounds(0.0, 2.0).grid([1.0]).style("red"))
            .y_axis(Axis::default().bounds(0.0, 1.0))
            .width(3)
            .height(1);
        let grid = chart.grid(&console, &console.options());
        assert_eq!(grid.row_string(0), " . ");
        assert_eq!(grid.style_at(0, 1), Style::parse("red").ok());
    }

    #[test]
    fn the_chart_style_is_under_everything_and_the_legends() {
        let console = Console::builder().width(8).build();
        let chart = Chart::new(vec![Dataset::new([]).name("a")])
            .x_axis(Axis::default().bounds(0.0, 1.0).style("red"))
            .y_axis(Axis::default().bounds(0.0, 1.0))
            .hidden_legend_fraction(1.0, 1.0)
            .width(8)
            .height(3);
        // No chart style: the legend's box in the x axis's.
        let grid = chart.grid(&console, &console.options());
        assert_eq!(grid.style_at(0, 5), Style::parse("red").ok());
        assert_eq!(grid.style_at(2, 0), None);
        let styled = chart.clone().style("on blue");
        let grid = styled.grid(&console, &console.options());
        assert_eq!(grid.style_at(0, 5), Style::parse("on blue").ok());
        assert_eq!(grid.style_at(2, 0), Style::parse("on blue").ok());
        // legend_style still wins.
        let grid = styled
            .legend_style("green")
            .grid(&console, &console.options());
        assert_eq!(grid.style_at(0, 5), Style::parse("on blue green").ok());
    }
    #[test]
    fn crossing_grid_lines_are_layers_of_their_own_the_y_axis_first() {
        // A cross at the centre of a 3 × 1 Braille plot: the x line (drawn
        // last) over the y line in the cell they share, as two datasets.
        let cross = |layering| {
            Chart::new(Vec::<Dataset>::new())
                .x_axis(Axis::default().bounds(-1.0, 1.0).grid([0.0]))
                .y_axis(Axis::default().bounds(-1.0, 1.0).grid([0.0]))
                .layering(layering)
                .width(3)
                .height(1)
        };
        let line = |points: [(f64, f64); 2]| {
            Dataset::new(points)
                .graph_type(GraphType::Line)
                .marker(Marker::Braille)
        };
        let datasets = Chart::new(vec![
            line([(-1.0, 0.0), (1.0, 0.0)]),
            line([(0.0, -1.0), (0.0, 1.0)]),
        ])
        .x_axis(Axis::default().bounds(-1.0, 1.0))
        .y_axis(Axis::default().bounds(-1.0, 1.0))
        .layering(Layering::PerDataset)
        .width(3)
        .height(1);
        let layered = rows(&cross(Layering::PerDataset), 3);
        assert_eq!(layered, rows(&datasets, 3));
        assert_eq!(layered, ["⠒⡇⠒"]);
        // Merged, the two lines share the cell's dots.
        assert_eq!(rows(&cross(Layering::Merge), 3), ["⠒⡗⠒"]);
    }

    #[test]
    fn a_grid_with_no_datasets_is_braille_unless_told() {
        let empty = Chart::new(Vec::<Dataset>::new())
            .x_axis(Axis::default().bounds(0.0, 1.0))
            .y_axis(Axis::default().bounds(-1.0, 1.0).grid([0.0]))
            .width(3)
            .height(1);
        assert_eq!(rows(&empty, 3), ["⠒⠒⠒"]);
        // The axis's marker wins over the chart's, the chart's over the
        // first dataset's.
        let dotted = Chart::new(vec![Dataset::new([]).marker(Marker::Dot)])
            .y_axis(Axis::default().bounds(-1.0, 1.0).grid([0.0]))
            .x_axis(Axis::default().bounds(0.0, 1.0))
            .width(3)
            .height(1);
        assert_eq!(rows(&dotted, 3), ["•••"]);
        assert_eq!(rows(&dotted.clone().marker(Marker::Ascii('=')), 3), ["==="]);
        let own = dotted.marker(Marker::Ascii('=')).y_axis(
            Axis::default()
                .bounds(-1.0, 1.0)
                .grid([0.0])
                .grid_marker(Marker::Ascii('~')),
        );
        assert_eq!(rows(&own, 3), ["~~~"]);
    }

    #[test]
    fn grid_lines_take_their_own_style_over_the_axis_style() {
        let console = Console::builder().width(3).build();
        let chart = Chart::new(vec![Dataset::new([]).marker(Marker::Ascii('.'))])
            .x_axis(
                Axis::default()
                    .bounds(0.0, 2.0)
                    .grid([1.0])
                    .style("red")
                    .grid_style("blue"),
            )
            .y_axis(Axis::default().bounds(0.0, 1.0))
            .width(3)
            .height(1);
        let grid = chart.grid(&console, &console.options());
        assert_eq!(grid.row_string(0), " . ");
        assert_eq!(grid.style_at(0, 1), Style::parse("blue").ok());
        // A style or a marker alone draws no line.
        let none = Chart::new(Vec::<Dataset>::new())
            .x_axis(Axis::default().bounds(0.0, 2.0).grid_style("blue"))
            .y_axis(Axis::default().bounds(0.0, 1.0))
            .width(3)
            .height(1);
        assert_eq!(rows(&none, 3), ["   "]);
    }

    #[test]
    fn an_unstyled_legend_is_the_none_style_by_name() {
        let console = Console::builder().width(9).build();
        let options = console.options();
        let chart = legend_chart(LegendPosition::TopRight).x_axis(Axis::default().style("magenta"));
        let plain = chart.clone().legend_unstyled().grid(&console, &options);
        let none = chart.style("none").grid(&console, &options);
        for (row, col) in [(0, 4), (1, 4), (3, 6)] {
            assert_eq!(
                plain.style_at(row, col).unwrap_or_default(),
                Style::default()
            );
            assert_eq!(plain.style_at(row, col), none.style_at(row, col));
        }
    }
}

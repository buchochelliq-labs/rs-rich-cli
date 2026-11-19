//! The `viz` plugin's charts as nodes: [`line_chart`], [`bar_chart`] and
//! [`heatmap`], each drawn by one of rs-rich-ext's charts from a closure
//! that reads signals, with a cursor the keys and the mouse move along the
//! x axis, a readout of the values under it, and a rolling
//! [`Window`] for data that streams in.

use std::rc::Rc;
use std::time::Duration;

use rich::{Console, Segment};
use rich_ext::chart::{Bar, BarChart, Heatmap, LineChart, Series, ValueFormat};
use rich_interact::{KeyCode, MouseKind};

use crate::a11y::Role;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::{Memo, memo};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// What of a chart's data shows, for data that streams in: the newest
/// points (a line chart's, per series), bars or columns, or the points
/// whose x is within a span of the newest (x read as seconds).
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Window {
    /// The last `n` points of each series, bars or columns.
    Last(usize),
    /// The points whose x is no more than this before the newest x, the
    /// x axis read as seconds. Only a line chart's points have an x; bars
    /// and columns keep all.
    Span(Duration),
}

/// A heatmap's data for [`heatmap`]: the columns' headers and the rows,
/// each a label and a value a column.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
#[must_use = "builder methods return a new value; it does nothing until a heatmap shows it"]
pub struct HeatGrid {
    pub columns: Vec<String>,
    pub rows: Vec<(String, Vec<f64>)>,
}

impl HeatGrid {
    /// An empty grid.
    pub fn new() -> HeatGrid {
        HeatGrid::default()
    }

    /// The columns' headers.
    pub fn columns<S: Into<String>>(mut self, headers: impl IntoIterator<Item = S>) -> HeatGrid {
        self.columns = headers.into_iter().map(Into::into).collect();
        self
    }

    /// A row called `label`, a value a column.
    pub fn row(
        mut self,
        label: impl Into<String>,
        values: impl IntoIterator<Item = f64>,
    ) -> HeatGrid {
        self.rows.push((label.into(), values.into_iter().collect()));
        self
    }

    /// The last `n` columns.
    fn last(mut self, n: usize) -> HeatGrid {
        let count = self
            .rows
            .iter()
            .map(|(_, v)| v.len())
            .max()
            .unwrap_or(0)
            .max(self.columns.len());
        let from = count.saturating_sub(n);
        if from > 0 {
            self.columns = self.columns.into_iter().skip(from).collect();
            for (_, values) in &mut self.rows {
                *values = values.iter().skip(from).copied().collect();
            }
        }
        self
    }
}

type Configure<T> = Rc<dyn Fn(T) -> T>;

/// What a chart node shows, and its data in view.
enum Data {
    Line {
        source: Rc<dyn Fn() -> Vec<Series>>,
        view: Memo<Vec<Series>>,
        configure: Configure<LineChart>,
    },
    Bars {
        source: Rc<dyn Fn() -> Vec<(String, f64)>>,
        view: Memo<Vec<(String, f64)>>,
        configure: Configure<BarChart>,
    },
    Heat {
        source: Rc<dyn Fn() -> HeatGrid>,
        view: Memo<HeatGrid>,
        configure: Configure<Heatmap>,
    },
}

/// `series` cut to `window`.
fn window_series(series: Vec<Series>, window: Option<Window>) -> Vec<Series> {
    match window {
        None => series,
        Some(Window::Last(n)) => series
            .into_iter()
            .map(|s| {
                let from = s.points().len().saturating_sub(n);
                let points = s.points()[from..].to_vec();
                s.with_points(points)
            })
            .collect(),
        Some(Window::Span(span)) => {
            let newest = series
                .iter()
                .flat_map(|s| s.points().iter().map(|p| p.0))
                .filter(|x| x.is_finite())
                .fold(f64::NEG_INFINITY, f64::max);
            let from = newest - span.as_secs_f64();
            series
                .into_iter()
                .map(|s| {
                    let points: Vec<(f64, f64)> =
                        s.points().iter().copied().filter(|p| p.0 >= from).collect();
                    s.with_points(points)
                })
                .collect()
        }
    }
}

impl Data {
    /// The view of the data, again, for `window`.
    fn rewindow(&mut self, window: Option<Window>) {
        match self {
            Data::Line { source, view, .. } => {
                let source = source.clone();
                *view = memo(move || window_series(source(), window));
            }
            Data::Bars { source, view, .. } => {
                let source = source.clone();
                *view = memo(move || {
                    let mut bars = source();
                    if let Some(Window::Last(n)) = window {
                        bars.drain(..bars.len().saturating_sub(n));
                    }
                    bars
                });
            }
            Data::Heat { source, view, .. } => {
                let source = source.clone();
                *view = memo(move || match window {
                    Some(Window::Last(n)) => source().last(n),
                    _ => source(),
                });
            }
        }
    }
}

/// A [`line_chart`]'s, [`bar_chart`]'s or [`heatmap`]'s widget.
struct ChartNode {
    data: Data,
    /// Which column (a line chart's plot column, a bar, a heatmap column)
    /// the cursor is on.
    cursor: Option<usize>,
    /// Where the last draw put the cursor's places: each one's first cell
    /// and width, and the rows they take.
    places: Vec<(u16, u16)>,
    rows: u16,
    /// The readout the last draw wrote.
    readout: String,
    /// What the chart is, for a screen reader: its kind and its series' or
    /// rows' names.
    about: String,
}

fn chart_node(data: Data) -> Node {
    let mut node = widget(ChartNode {
        data,
        cursor: None,
        places: Vec::new(),
        rows: 0,
        readout: String::new(),
        about: String::new(),
    })
    .what("chart")
    .focusable();
    node.size = Size::Auto;
    node
}

/// A line chart of `series`, a closure that may read signals and returns
/// rs-rich-ext's [`Series`] (`Series::from_values("load", history)`,
/// `Series::line("p95", points)`): drawn by its [`LineChart`] (Braille
/// dots, a y axis, x labels and a legend) as tall as the node, less a row
/// under it for the readout.
///
/// With the focus ← and → move a cursor along the x axis (Home and End to
/// its ends, Esc takes it away), and the pointer moving over the plot puts
/// it there; the cursor's column is drawn in the theme's `chart.cursor`
/// style (`reverse`) and the readout says the x there and each series'
/// nearest value (`x 3 · load 5 · cpu 2`). Without a cursor it reads the
/// newest x. A [`window`](Node::chart_window) keeps the newest points of a
/// stream; the chart draws again only when what is in view changes. Its
/// role is an image, its name the chart's kind and series, its value the
/// readout.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::line_chart;
/// use rich_ext::chart::Series;
///
/// let app = App::new(|| {
///     let load = signal(vec![1.0, 3.0, 2.0, 5.0, 4.0]);
///     line_chart(move || vec![Series::from_values("load", load.get())])
/// });
/// let mut driver = app.driver(30, 12);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[11].trim_end(), "x 4 · load 4");
/// driver.press("home right");
/// // A column on: x 0.15, nearest the first point.
/// assert_eq!(driver.screen().plain()[11].trim_end(), "x 0.15 · load 1");
/// ```
pub fn line_chart(series: impl Fn() -> Vec<Series> + 'static) -> Node {
    line_chart_with(series, |chart| chart)
}

/// A [`line_chart`] whose [`LineChart`] `configure` sets up (`.y_range(0.0,
/// 100.0)`, `.legend(false)`, `.charset(Charset::Blocks)`): it is given one
/// with the series and the height, and returns it changed.
pub fn line_chart_with(
    series: impl Fn() -> Vec<Series> + 'static,
    configure: impl Fn(LineChart) -> LineChart + 'static,
) -> Node {
    let source: Rc<dyn Fn() -> Vec<Series>> = Rc::new(series);
    let view = {
        let source = source.clone();
        memo(move || source())
    };
    chart_node(Data::Line {
        source,
        view,
        configure: Rc::new(configure),
    })
}

/// A bar chart of `bars`, a closure that may read signals and returns
/// `(label, value)` pairs: drawn by rs-rich-ext's [`BarChart`], a row a
/// bar, its label, the bar and its value, then a readout row. ← and →
/// (or ↑ and ↓) move a cursor from bar to bar, its row in the theme's
/// `chart.cursor` style, and the readout says its label and value
/// (`b 4`); a click on a bar's row puts it there. Without a cursor it reads
/// the last bar. A [`Window::Last`] keeps the newest bars.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::bar_chart;
///
/// let app = App::new(|| bar_chart(|| vec![("a".into(), 4.0), ("b".into(), 2.0)]));
/// let mut driver = app.driver(20, 3);
/// // The first ← puts the cursor on the last bar; the next goes up.
/// driver.press("left left");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "a 4");
/// ```
pub fn bar_chart(bars: impl Fn() -> Vec<(String, f64)> + 'static) -> Node {
    bar_chart_with(bars, |chart| chart)
}

/// A [`bar_chart`] whose [`BarChart`] `configure` sets up (`.range(0.0,
/// 10.0)`, `.charset(..)`). Its bars stay one a row: the cursor follows
/// rows.
pub fn bar_chart_with(
    bars: impl Fn() -> Vec<(String, f64)> + 'static,
    configure: impl Fn(BarChart) -> BarChart + 'static,
) -> Node {
    let source: Rc<dyn Fn() -> Vec<(String, f64)>> = Rc::new(bars);
    let view = {
        let source = source.clone();
        memo(move || source())
    };
    chart_node(Data::Bars {
        source,
        view,
        configure: Rc::new(configure),
    })
}

/// A heatmap of `grid`, a closure that may read signals and returns a
/// [`HeatGrid`] (the columns' headers, and rows of values): drawn by
/// rs-rich-ext's [`Heatmap`], each value a shade, then a readout row. ←
/// and → move a cursor from column to column, drawn in the theme's
/// `chart.cursor` style, and the readout says the column's header and
/// each row's value (`tue · api 4 · web 6`); the pointer over a column
/// puts it there. A [`Window::Last`] keeps the newest columns.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{heatmap, HeatGrid};
///
/// let app = App::new(|| {
///     heatmap(|| HeatGrid::new().columns(["mon", "tue"]).row("api", [1.0, 4.0]))
/// });
/// let mut driver = app.driver(30, 5);
/// driver.press("home");
/// assert_eq!(driver.screen().plain()[4].trim_end(), "mon · api 1");
/// ```
pub fn heatmap(grid: impl Fn() -> HeatGrid + 'static) -> Node {
    heatmap_with(grid, |map| map)
}

/// A [`heatmap`] whose [`Heatmap`] `configure` sets up (`.range(0.0,
/// 1.0)`, `.cell_width(3)`, `.legend(false)`).
pub fn heatmap_with(
    grid: impl Fn() -> HeatGrid + 'static,
    configure: impl Fn(Heatmap) -> Heatmap + 'static,
) -> Node {
    let source: Rc<dyn Fn() -> HeatGrid> = Rc::new(grid);
    let view = {
        let source = source.clone();
        memo(move || source())
    };
    chart_node(Data::Heat {
        source,
        view,
        configure: Rc::new(configure),
    })
}

impl Node {
    /// For a [`line_chart`], [`bar_chart`] or [`heatmap`]: show only the
    /// data `window` keeps, the newest of a stream (`Window::Last(60)`,
    /// `Window::Span(Duration::from_secs(30))`). Ignored on other nodes.
    pub fn chart_window(self, window: Window) -> Node {
        self.widget_mut(|chart: &mut ChartNode| chart.data.rewindow(Some(window)));
        self
    }
}

fn number(value: f64) -> String {
    ValueFormat::Compact.format(value)
}

/// The point of `series` nearest `x`.
fn nearest(series: &Series, x: f64) -> Option<(f64, f64)> {
    series
        .points()
        .iter()
        .copied()
        .filter(|(px, py)| px.is_finite() && py.is_finite())
        .min_by(|a, b| (a.0 - x).abs().total_cmp(&(b.0 - x).abs()))
}

fn rendered(console: &Console, value: &impl rich::Renderable, width: u16) -> Vec<Vec<Segment>> {
    let options = console.options().update_width(width.max(1) as usize);
    console.render_lines(value, &options, false)
}

impl ChartNode {
    /// The chart's lines in `width` x `height` (0: as tall as it likes),
    /// with the readout; and where the cursor's places are.
    fn render(&mut self, console: &Console, width: u16, height: u16) -> Vec<Vec<Segment>> {
        let rows = height.saturating_sub(1);
        match &self.data {
            Data::Line {
                view, configure, ..
            } => {
                let series = view.get();
                let mut chart = LineChart::new();
                for s in &series {
                    chart = chart.series(s.clone());
                }
                if height > 0 {
                    // The plot, less its axis, its x labels and a legend.
                    chart = chart.height(rows.saturating_sub(3).max(1) as usize);
                }
                let chart = configure(chart);
                let names: Vec<&str> = series.iter().map(Series::name).collect();
                self.about = format!("Line chart: {}", names.join(", "));
                let (left, xs) = chart
                    .plot_columns(width.max(1) as usize)
                    .unwrap_or((0, Vec::new()));
                self.places = (0..xs.len())
                    .map(|c| ((left + c).min(u16::MAX as usize) as u16, 1))
                    .collect();
                let mut lines = rendered(console, &chart, width);
                // The plot's rows: those above the axis, x labels and legend.
                self.rows = lines.len().saturating_sub(
                    2 + usize::from(chart.all_series().iter().any(|s| !s.name().is_empty())),
                ) as u16;
                if let Some(c) = self.cursor.map(|c| c.min(xs.len().saturating_sub(1))) {
                    self.cursor = Some(c);
                }
                let x = match self.cursor {
                    Some(c) => xs.get(c).copied(),
                    None => series
                        .iter()
                        .filter_map(|s| {
                            s.points()
                                .iter()
                                .rev()
                                .find(|p| p.0.is_finite())
                                .map(|p| p.0)
                        })
                        .reduce(f64::max),
                };
                self.readout = match x {
                    Some(x) => {
                        let mut parts = vec![format!("x {}", number(x))];
                        for s in &series {
                            if let Some((_, y)) = nearest(s, x) {
                                parts
                                    .push(format!("{} {}", s.name(), number(y)).trim().to_string());
                            }
                        }
                        parts.join(" · ")
                    }
                    None => String::new(),
                };
                if height > 0 {
                    lines.truncate(rows.max(1) as usize);
                }
                lines
            }
            Data::Bars {
                view, configure, ..
            } => {
                let bars = view.get();
                if let Some(c) = self.cursor {
                    self.cursor = Some(c.min(bars.len().saturating_sub(1)));
                }
                let mut chart = BarChart::new();
                for (label, value) in &bars {
                    chart = chart.push(Bar::new(label.clone(), *value));
                }
                let chart = configure(chart);
                let labels: Vec<&str> = bars.iter().map(|(l, _)| l.as_str()).collect();
                self.about = format!("Bar chart: {}", labels.join(", "));
                self.places = (0..bars.len())
                    .map(|i| (i.min(u16::MAX as usize) as u16, 1))
                    .collect();
                self.rows = bars.len().min(u16::MAX as usize) as u16;
                let at = self.cursor.or(bars.len().checked_sub(1));
                self.readout = at
                    .and_then(|i| bars.get(i))
                    .map(|(label, value)| format!("{label} {}", number(*value)))
                    .unwrap_or_default();
                let mut lines = rendered(console, &chart, width);
                if height > 0 {
                    lines.truncate(rows.max(1) as usize);
                }
                lines
            }
            Data::Heat {
                view, configure, ..
            } => {
                let grid = view.get();
                let mut map = Heatmap::new().columns(grid.columns.clone());
                for (label, values) in &grid.rows {
                    map = map.row(label.clone(), values.iter().copied());
                }
                let map = configure(map);
                let labels: Vec<&str> = grid.rows.iter().map(|(l, _)| l.as_str()).collect();
                self.about = format!("Heatmap: {}", labels.join(", "));
                let cells = map.column_cells(width.max(1) as usize);
                if let Some(c) = self.cursor {
                    self.cursor = Some(c.min(cells.len().saturating_sub(1)));
                }
                self.places = cells
                    .iter()
                    .map(|(at, _)| {
                        let x = at.start.min(u16::MAX as usize) as u16;
                        (x, at.len().min(u16::MAX as usize) as u16)
                    })
                    .collect();
                self.rows = rendered(console, &map.clone().legend(false), width).len() as u16;
                let at = self.cursor.or(cells.len().checked_sub(1));
                self.readout = at
                    .and_then(|i| cells.get(i))
                    .map(|(_, columns)| {
                        let header = grid.columns.get(columns.start).cloned().unwrap_or_default();
                        let mut parts = vec![header];
                        for (label, values) in &grid.rows {
                            let shown: Vec<f64> = values
                                .get(columns.clone())
                                .unwrap_or_default()
                                .iter()
                                .copied()
                                .filter(|v| v.is_finite())
                                .collect();
                            let value = if shown.is_empty() {
                                "-".to_string()
                            } else {
                                number(shown.iter().sum::<f64>() / shown.len() as f64)
                            };
                            parts.push(format!("{label} {value}"));
                        }
                        parts.join(" · ")
                    })
                    .unwrap_or_default();
                let mut lines = rendered(console, &map, width);
                if height > 0 {
                    lines.truncate(rows.max(1) as usize);
                }
                lines
            }
        }
    }

    /// The cursor's place under column `x`, if any.
    fn place_at(&self, x: u16) -> Option<usize> {
        self.places
            .iter()
            .position(|&(at, w)| x >= at && x < at.saturating_add(w))
    }
}

impl Widget for ChartNode {
    fn name(&self) -> &'static str {
        "chart"
    }

    fn role(&self) -> Role {
        Role::Img
    }

    fn access_text(&self) -> Option<String> {
        Some(self.about.clone())
    }

    fn access_value(&self) -> Option<String> {
        (!self.readout.is_empty()).then(|| self.readout.clone())
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Horizontal => width,
            Axis::Vertical => {
                let lines = self.render(cx.console(), width, 0);
                (lines.len() + 1).min(u16::MAX as usize) as u16
            }
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let cursor = cx.style("chart.cursor", "reverse");
        let (width, height) = (canvas.width(), canvas.height());
        let lines = self.render(cx.console(), width, height);
        let readout_row = height.saturating_sub(1);
        canvas.lines(&lines);
        if let Some(&(x, w)) = self.cursor.and_then(|c| self.places.get(c)) {
            match self.data {
                // A bar is a row.
                Data::Bars { .. } => canvas.restyle(0, x, width, 1, &cursor),
                _ => canvas.restyle(x, 0, w, self.rows.min(readout_row), &cursor),
            }
        }
        canvas.print(0, readout_row, &self.readout, None);
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let last = self.places.len().saturating_sub(1);
        let bars = matches!(self.data, Data::Bars { .. });
        let now = self.cursor;
        let to = match event {
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => match key.code {
                KeyCode::Left => Some(now.map_or(last, |c| c.saturating_sub(1))),
                KeyCode::Right => Some(now.map_or(last, |c| (c + 1).min(last))),
                KeyCode::Up if bars => Some(now.map_or(last, |c| c.saturating_sub(1))),
                KeyCode::Down if bars => Some(now.map_or(last, |c| (c + 1).min(last))),
                KeyCode::Home => Some(0),
                KeyCode::End => Some(last),
                KeyCode::Escape if now.is_some() => None,
                _ => return Used::No,
            },
            WidgetEvent::Mouse(mouse)
                if matches!(
                    mouse.kind,
                    MouseKind::Moved | MouseKind::Down(_) | MouseKind::Drag(_)
                ) =>
            {
                let at = if bars {
                    (mouse.row < self.rows).then_some(mouse.row as usize)
                } else {
                    self.place_at(mouse.column)
                };
                match at {
                    Some(at) => Some(at),
                    None => return Used::No,
                }
            }
            _ => return Used::No,
        };
        if self.places.is_empty() {
            return Used::No;
        }
        self.cursor = to;
        cx.redraw();
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

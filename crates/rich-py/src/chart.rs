//! `rs_rich.chart`: `rich_ext::chart` from Python (0.0.15 workstream 5).
//!
//! Owner: the chart area. Every class is frozen and takes its settings as
//! keyword arguments, building the `rich-ext` chart once; it renders in Rust
//! when printed. Names the flat native module already had (`Bar`, `Status`'s
//! `State`, `Span`) are prefixed (`ChartBar`, `ChartState`, `TimelineSpan`);
//! `rs_rich.chart` also exports them under the Rust names.
//!
//! | Python | Rust |
//! |---|---|
//! | `Sparkline` | `rich_ext::chart::Sparkline` |
//! | `ChartBar`, `BarGroup`, `BarChart`, `Histogram` | `Bar`, `BarGroup`, `BarChart`, `Histogram` |
//! | `Series`, `LineChart` | `Series`, `LineChart` |
//! | `Band`, `Gauge`, `BulletChart` | `Band`, `Gauge`, `BulletChart` |
//! | `FillGauge`, `LineGauge` | `FillGauge`, `LineGauge` (0.0.19) |
//! | `Heatmap` | `Heatmap` |
//! | `ChartState`, `StatusMatrix` | `State`, `StatusMatrix` |
//! | `KpiCard` | `KpiCard` (its `status` by name) |
//! | `TimelineSpan`, `Timeline` | `Span`, `Timeline` |
//! | `Distribution`, `DistributionSet` | `Distribution`, `DistributionSet` (0.0.19) |
//! | `SmallMultiples` | `SmallMultiples` of `Sparkline`s, `BarChart`s and `LineChart`s (0.0.19) |
//! | `chart_quantile`, `chart_summary` | `quantile`, `Summary::of` (0.0.19) |
//! | `chart_describe` | each chart's `a11y::AccessibleText` (0.0.19) |

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PySequence, PyString};

use rich::protocol::Renderable;
use rich_ext::chart::{
    Band as CoreBand, Bar as CoreBar, BarChart as CoreBarChart, BarGroup as CoreBarGroup,
    BulletChart as CoreBulletChart, Charset, Direction, Distribution as CoreDistribution,
    DistributionSet as CoreDistributionSet, Facet, FillGauge as CoreFillGauge, Gauge as CoreGauge,
    Heatmap as CoreHeatmap, Histogram as CoreHistogram, KpiCard as CoreKpiCard, Levels,
    LineChart as CoreLineChart, LineGauge as CoreLineGauge, LineSet, Orientation,
    Series as CoreSeries, SeriesKind, SmallMultiples as CoreSmallMultiples, Span as CoreSpan,
    Sparkline as CoreSparkline, State as CoreState, Status, StatusMatrix as CoreStatusMatrix,
    Summary as CoreSummary, Timeline as CoreTimeline, ValueFormat,
};

use crate::art::repr_str;
use crate::ext::common::names;
use crate::renderable::{self, AsRenderable};

names!(charset, charset_name, Charset, "charset", {
    "auto" => Charset::Auto,
    "blocks" => Charset::Blocks,
    "braille" => Charset::Braille,
    "ascii" => Charset::Ascii,
});

names!(orientation, orientation_name, Orientation, "orientation", {
    "horizontal" => Orientation::Horizontal,
    "vertical" => Orientation::Vertical,
});

names!(series_kind, series_kind_name, SeriesKind, "series kind", {
    "line" => SeriesKind::Line,
    "scatter" => SeriesKind::Scatter,
});

names!(direction, direction_name, Direction, "direction", {
    "left_to_right" => Direction::LeftToRight,
    "right_to_left" => Direction::RightToLeft,
});

names!(levels, levels_name, Levels, "levels", {
    "nine" => Levels::Nine,
    "three" => Levels::Three,
});

names!(line_set, line_set_name, LineSet, "line", {
    "thin" => LineSet::Thin,
    "thick" => LineSet::Thick,
    "double" => LineSet::Double,
});

names!(status, status_name, Status, "status", {
    "ok" => Status::Ok,
    "warning" => Status::Warning,
    "critical" => Status::Critical,
    "unknown" => Status::Unknown,
});

/// `format`: `None` or `"compact"` for short forms (`1.2k`), or a number of
/// decimals.
fn value_format(format: Option<&Bound<'_, PyAny>>) -> PyResult<ValueFormat> {
    let Some(format) = format.filter(|f| !f.is_none()) else {
        return Ok(ValueFormat::Compact);
    };
    if let Ok(text) = format.extract::<String>() {
        if text.eq_ignore_ascii_case("compact") {
            return Ok(ValueFormat::Compact);
        }
        return Err(PyValueError::new_err(format!(
            "invalid format {}; expected \"compact\" or a number of decimals",
            repr_str(&text)
        )));
    }
    if format.is_instance_of::<pyo3::types::PyBool>() {
        return Err(PyTypeError::new_err(
            "format must be \"compact\" or a number of decimals",
        ));
    }
    let decimals: i64 = format
        .extract()
        .map_err(|_| PyTypeError::new_err("format must be \"compact\" or a number of decimals"))
        .or_else(|err| {
            // An int too large for i64 is a number of decimals out of range.
            if format.is_instance_of::<pyo3::types::PyInt>() {
                Ok(i64::MAX)
            } else {
                Err(err)
            }
        })?;
    match usize::try_from(decimals) {
        Ok(decimals) if decimals <= ValueFormat::MAX_DECIMALS => Ok(ValueFormat::Fixed(decimals)),
        _ => Err(PyValueError::new_err(format!(
            "format: {} decimals is out of range; expected 0 to {}",
            format.repr()?,
            ValueFormat::MAX_DECIMALS
        ))),
    }
}

/// A number, with `None` as a gap (NaN).
fn number(value: &Bound<'_, PyAny>) -> PyResult<f64> {
    if value.is_none() {
        return Ok(f64::NAN);
    }
    if value.is_instance_of::<pyo3::types::PyString>() {
        return Err(PyTypeError::new_err(format!(
            "expected a number, got the string {}",
            value.repr()?
        )));
    }
    value.extract()
}

/// Any iterable of numbers (`None` a gap).
fn numbers(values: &Bound<'_, PyAny>) -> PyResult<Vec<f64>> {
    values.try_iter()?.map(|v| number(&v?)).collect()
}

/// The two items of a tuple, list or other sequence of length 2 (not a
/// string), or `None`.
fn pair<'py>(value: &Bound<'py, PyAny>) -> Option<(Bound<'py, PyAny>, Bound<'py, PyAny>)> {
    if value.is_instance_of::<PyString>() {
        return None;
    }
    let sequence = value.cast::<PySequence>().ok()?;
    if sequence.len().ok()? != 2 {
        return None;
    }
    Some((sequence.get_item(0).ok()?, sequence.get_item(1).ok()?))
}

/// Any iterable of `(x, y)` pairs: tuples, lists or other sequences of two.
fn points(values: &Bound<'_, PyAny>) -> PyResult<Vec<(f64, f64)>> {
    values
        .try_iter()?
        .map(|item| {
            let (x, y) =
                pair(&item?).ok_or_else(|| PyTypeError::new_err("a point is an (x, y) pair"))?;
            Ok((number(&x)?, number(&y)?))
        })
        .collect()
}

/// Any iterable (not a string) as a list, each item read by `read`; `None`
/// is empty. `what` names the argument in the error.
fn each<'py, T>(
    values: Option<&Bound<'py, PyAny>>,
    what: &str,
    read: impl Fn(&Bound<'py, PyAny>) -> PyResult<T>,
) -> PyResult<Vec<T>> {
    let Some(values) = values.filter(|v| !v.is_none()) else {
        return Ok(Vec::new());
    };
    if values.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(format!(
            "{what} must be an iterable, not a string"
        )));
    }
    values
        .try_iter()
        .map_err(|_| PyTypeError::new_err(format!("{what} must be an iterable")))?
        .map(|item| read(&item?))
        .collect()
}

/// Any iterable of strings, such as column names.
fn strings(values: Option<&Bound<'_, PyAny>>, what: &str) -> PyResult<Vec<String>> {
    each(values, what, |item| {
        item.extract::<String>()
            .map_err(|_| PyTypeError::new_err(format!("{what} must be strings")))
    })
}

/// Items of an iterable, or the `(key, value)` items of a mapping.
fn items<'py>(values: &Bound<'py, PyAny>) -> PyResult<Vec<Bound<'py, PyAny>>> {
    if let Ok(dict) = values.cast::<PyDict>() {
        return Ok(dict.items().into_iter().collect());
    }
    values.try_iter()?.collect()
}

macro_rules! renders {
    ($ty:ident) => {
        impl AsRenderable for $ty {
            fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
                Ok(Box::new(self.inner.clone()))
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Sparkline

/// One line, one cell per value (two in Braille) (`rich_ext::chart::Sparkline`).
#[pyclass(name = "Sparkline", module = "rs_rich.chart", frozen)]
pub(crate) struct Sparkline {
    inner: CoreSparkline,
}
renders!(Sparkline);

#[pymethods]
impl Sparkline {
    /// `values`: numbers, `None` for a gap. `range`: fix the scale as
    /// `(min, max)`. `min_max`: name the extremes after the line.
    /// `threshold`: count (and mark) the values above it. `rows`: how many
    /// rows tall (default: the height it is given, or one). `direction`:
    /// `"left_to_right"` or `"right_to_left"`. `absent`: draw gaps with a
    /// symbol, or a `(symbol, style)` pair. `styles`: a style (or `None`)
    /// for each value. `levels`: `"nine"` or `"three"` block heights.
    #[new]
    #[pyo3(signature = (values, *, range=None, min=None, max=None, charset="auto", style=None, min_max=false, threshold=None, format=None, rows=None, direction="left_to_right", absent=None, styles=None, levels="nine"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        values: &Bound<'_, PyAny>,
        range: Option<(f64, f64)>,
        min: Option<f64>,
        max: Option<f64>,
        charset: &str,
        style: Option<String>,
        min_max: bool,
        threshold: Option<f64>,
        format: Option<&Bound<'_, PyAny>>,
        rows: Option<usize>,
        direction: &str,
        absent: Option<&Bound<'_, PyAny>>,
        styles: Option<&Bound<'_, PyAny>>,
        levels: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreSparkline::new(numbers(values)?)
            .charset(self::charset(charset)?)
            .min_max(min_max)
            .format(value_format(format)?)
            .direction(self::direction(direction)?)
            .levels(self::levels(levels)?);
        if let Some(rows) = rows {
            if rows == 0 {
                return Err(PyValueError::new_err("rows must be at least 1"));
            }
            inner = inner.rows(rows);
        }
        if let Some(absent) = absent.filter(|a| !a.is_none()) {
            let (symbol, style) = if let Ok(symbol) = absent.extract::<String>() {
                (symbol, String::new())
            } else {
                absent
                    .extract::<(String, Option<String>)>()
                    .map(|(symbol, style)| (symbol, style.unwrap_or_default()))
                    .map_err(|_| {
                        PyTypeError::new_err("absent is a symbol, or a (symbol, style) pair")
                    })?
            };
            inner = inner.absent(symbol, style);
        }
        let styles = each(styles, "styles", |item| {
            if item.is_none() {
                return Ok(None);
            }
            item.extract::<String>()
                .map(Some)
                .map_err(|_| PyTypeError::new_err("styles must be strings or None"))
        })?;
        if !styles.is_empty() {
            inner = inner.styles(styles);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(min) = min {
            inner = inner.min(min);
        }
        if let Some(max) = max {
            inner = inner.max(max);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        if let Some(threshold) = threshold {
            inner = inner.threshold(threshold);
        }
        Ok(Sparkline { inner })
    }

    #[getter]
    fn values(&self) -> Vec<f64> {
        self.inner.values().to_vec()
    }

    fn __repr__(&self) -> String {
        format!("<Sparkline {} values>", self.inner.values().len())
    }
}

// ---------------------------------------------------------------------------
// Bars and histograms

/// One labelled bar of a `BarChart` (`rich_ext::chart::Bar`).
#[pyclass(
    name = "ChartBar",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ChartBar {
    inner: CoreBar,
    label: String,
    value: f64,
    style: Option<String>,
}

#[pymethods]
impl ChartBar {
    /// `text`: written instead of the value (`"76°"`). `value_style` and
    /// `label_style`: styles over the chart's for this bar.
    #[new]
    #[pyo3(signature = (label, value, *, style=None, text=None, value_style=None, label_style=None))]
    fn new(
        label: String,
        value: &Bound<'_, PyAny>,
        style: Option<String>,
        text: Option<String>,
        value_style: Option<String>,
        label_style: Option<String>,
    ) -> PyResult<Self> {
        let value = number(value)?;
        let mut inner = CoreBar::new(label.clone(), value);
        if let Some(style) = &style {
            inner = inner.style(style.clone());
        }
        if let Some(text) = text {
            inner = inner.text(text);
        }
        if let Some(style) = value_style {
            inner = inner.value_style(style);
        }
        if let Some(style) = label_style {
            inner = inner.label_style(style);
        }
        Ok(ChartBar {
            inner,
            label,
            value,
            style,
        })
    }

    /// What is written instead of the value, if anything.
    #[getter]
    fn text(&self) -> Option<&str> {
        self.inner.text.as_deref()
    }

    #[getter]
    fn label(&self) -> &str {
        &self.label
    }

    #[getter]
    fn value(&self) -> f64 {
        self.value
    }

    #[getter]
    fn style(&self) -> Option<&str> {
        self.style.as_deref()
    }

    fn __repr__(&self) -> String {
        format!("ChartBar({}, {})", repr_str(&self.label), self.value)
    }
}

/// A bar from a `ChartBar` or a `(label, value)` pair.
fn bar(item: &Bound<'_, PyAny>) -> PyResult<CoreBar> {
    if let Ok(bar) = item.cast::<ChartBar>() {
        return Ok(bar.get().inner.clone());
    }
    let (label, value): (String, Bound<'_, PyAny>) = item
        .extract()
        .map_err(|_| PyTypeError::new_err("a bar is a ChartBar or a (label, value) pair"))?;
    Ok(CoreBar::new(label, number(&value)?))
}

/// Bars with a label, set apart from the others in a `BarChart`
/// (`rich_ext::chart::BarGroup`).
#[pyclass(
    name = "BarGroup",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct BarGroup {
    inner: CoreBarGroup,
    label: String,
    len: usize,
}

#[pymethods]
impl BarGroup {
    /// `label`: under (or after) the group; empty for none. `bars`:
    /// `ChartBar`s, `(label, value)` pairs or a `{label: value}` mapping.
    #[new]
    #[pyo3(signature = (label, bars=None))]
    fn new(label: String, bars: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let mut inner = CoreBarGroup::new(label.clone());
        let mut len = 0;
        if let Some(bars) = bars.filter(|b| !b.is_none()) {
            for item in items(bars)? {
                inner = inner.bar(bar(&item)?);
                len += 1;
            }
        }
        Ok(BarGroup { inner, label, len })
    }

    #[getter]
    fn label(&self) -> &str {
        &self.label
    }

    fn __len__(&self) -> usize {
        self.len
    }

    fn __repr__(&self) -> String {
        format!(
            "<BarGroup {} with {} bars>",
            repr_str(&self.label),
            self.len
        )
    }
}

/// A label, a bar and a value per row, or columns side by side
/// (`rich_ext::chart::BarChart`).
#[pyclass(name = "BarChart", module = "rs_rich.chart", frozen)]
pub(crate) struct BarChart {
    inner: CoreBarChart,
}
renders!(BarChart);

#[pymethods]
impl BarChart {
    /// `bars`: `ChartBar`s, `(label, value)` pairs or a `{label: value}`
    /// mapping; `groups`: `BarGroup`s, after them. `orientation`:
    /// `"horizontal"` or `"vertical"`. `thickness` is ratatui's
    /// `bar_width` (cells across, or rows down); `bar_width` is the longest
    /// bar's length, or the tallest column's height. `values_inside`
    /// writes each value on its bar.
    #[new]
    #[pyo3(signature = (bars=None, *, groups=None, range=None, max=None, charset="auto", orientation="horizontal", bar_width=None, show_values=true, format=None, style=None, thickness=None, bar_gap=None, group_gap=None, label_style=None, value_style=None, values_inside=false))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        bars: Option<&Bound<'_, PyAny>>,
        groups: Option<&Bound<'_, PyAny>>,
        range: Option<(f64, f64)>,
        max: Option<f64>,
        charset: &str,
        orientation: &str,
        bar_width: Option<usize>,
        show_values: bool,
        format: Option<&Bound<'_, PyAny>>,
        style: Option<String>,
        thickness: Option<usize>,
        bar_gap: Option<usize>,
        group_gap: Option<usize>,
        label_style: Option<String>,
        value_style: Option<String>,
        values_inside: bool,
    ) -> PyResult<Self> {
        let mut inner = CoreBarChart::new()
            .charset(self::charset(charset)?)
            .orientation(self::orientation(orientation)?)
            .show_values(show_values)
            .values_inside(values_inside)
            .format(value_format(format)?);
        if let Some(bars) = bars.filter(|b| !b.is_none()) {
            for item in items(bars)? {
                inner = inner.push(bar(&item)?);
            }
        }
        let groups = each(groups, "groups", |item| {
            item.cast::<BarGroup>()
                .map(|group| group.get().inner.clone())
                .map_err(|_| PyTypeError::new_err("groups must be BarGroups"))
        })?;
        for group in groups {
            inner = inner.group(group);
        }
        if let Some(thickness) = thickness {
            if thickness == 0 {
                return Err(PyValueError::new_err("thickness must be at least 1"));
            }
            inner = inner.thickness(thickness);
        }
        if let Some(gap) = bar_gap {
            inner = inner.bar_gap(gap);
        }
        if let Some(gap) = group_gap {
            inner = inner.group_gap(gap);
        }
        if let Some(style) = label_style {
            inner = inner.label_style(style);
        }
        if let Some(style) = value_style {
            inner = inner.value_style(style);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(max) = max {
            inner = inner.max(max);
        }
        if let Some(width) = bar_width {
            inner = inner.bar_width(width);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        Ok(BarChart { inner })
    }

    fn __len__(&self) -> usize {
        self.inner.bars().len()
    }

    fn __repr__(&self) -> String {
        format!("<BarChart {} bars>", self.inner.bars().len())
    }
}

/// Raw values counted into bins, drawn as a `BarChart`
/// (`rich_ext::chart::Histogram`).
#[pyclass(name = "Histogram", module = "rs_rich.chart", frozen)]
pub(crate) struct Histogram {
    inner: CoreHistogram,
}
renders!(Histogram);

#[pymethods]
impl Histogram {
    #[new]
    #[pyo3(signature = (values, *, bins=None, range=None, charset="auto", orientation="horizontal", bar_width=None, show_values=true, style=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        values: &Bound<'_, PyAny>,
        bins: Option<usize>,
        range: Option<(f64, f64)>,
        charset: &str,
        orientation: &str,
        bar_width: Option<usize>,
        show_values: bool,
        style: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreHistogram::new(numbers(values)?)
            .charset(self::charset(charset)?)
            .orientation(self::orientation(orientation)?)
            .show_values(show_values);
        if let Some(bins) = bins {
            if bins == 0 {
                return Err(PyValueError::new_err("bins must be at least 1"));
            }
            inner = inner.bins(bins);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(width) = bar_width {
            inner = inner.bar_width(width);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        Ok(Histogram { inner })
    }

    /// The bin edges, one more than the bins.
    fn edges(&self) -> Vec<f64> {
        self.inner.edges()
    }

    /// How many values fell in each bin.
    fn counts(&self) -> Vec<usize> {
        self.inner.counts()
    }

    fn __repr__(&self) -> String {
        format!("<Histogram {} bins>", self.inner.counts().len())
    }
}

// ---------------------------------------------------------------------------
// Line and scatter charts

/// One named set of points in a `LineChart` (`rich_ext::chart::Series`).
#[pyclass(name = "Series", module = "rs_rich.chart", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Series {
    inner: CoreSeries,
    kind: SeriesKind,
}

#[pymethods]
impl Series {
    /// `points`: `(x, y)` pairs, or plain numbers at x = 0, 1, 2, …
    /// `kind`: `"line"` or `"scatter"`. `marker`: one character.
    #[new]
    #[pyo3(signature = (name, points, *, kind="line", style=None, marker=None))]
    fn new(
        name: String,
        points: &Bound<'_, PyAny>,
        kind: &str,
        style: Option<String>,
        marker: Option<char>,
    ) -> PyResult<Self> {
        let kind = series_kind(kind)?;
        // Read once into a list: a generator would lose the item looked at.
        let points = PyList::new(
            points.py(),
            points.try_iter()?.collect::<PyResult<Vec<_>>>()?,
        )?;
        let points = points.as_any();
        let first = points.try_iter()?.next().transpose()?;
        let pairs = match first {
            Some(first) if pair(&first).is_some() => self::points(points)?,
            _ => numbers(points)?
                .into_iter()
                .enumerate()
                .map(|(i, y)| (i as f64, y))
                .collect(),
        };
        let mut inner = CoreSeries::line(name, pairs).kind(kind);
        if let Some(style) = style {
            inner = inner.style(style);
        }
        if let Some(marker) = marker {
            inner = inner.marker(marker);
        }
        Ok(Series { inner, kind })
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn points(&self) -> Vec<(f64, f64)> {
        self.inner.points().to_vec()
    }

    #[getter]
    fn kind(&self) -> &'static str {
        series_kind_name(self.kind)
    }

    fn __repr__(&self) -> String {
        format!(
            "<Series {} {} points>",
            repr_str(self.inner.name()),
            self.inner.points().len()
        )
    }
}

/// One or more `Series` as lines or points, with axes and a legend
/// (`rich_ext::chart::LineChart`).
#[pyclass(name = "LineChart", module = "rs_rich.chart", frozen)]
pub(crate) struct LineChart {
    inner: CoreLineChart,
}
renders!(LineChart);

#[pymethods]
impl LineChart {
    #[new]
    #[pyo3(signature = (series=None, *, height=None, width=None, x_range=None, y_range=None, charset="auto", legend=true, x_format=None, y_format=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        series: Option<&Bound<'_, PyAny>>,
        height: Option<usize>,
        width: Option<usize>,
        x_range: Option<(f64, f64)>,
        y_range: Option<(f64, f64)>,
        charset: &str,
        legend: bool,
        x_format: Option<&Bound<'_, PyAny>>,
        y_format: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreLineChart::new()
            .charset(self::charset(charset)?)
            .legend(legend)
            .x_format(value_format(x_format)?)
            .y_format(value_format(y_format)?);
        if let Some(series) = series.filter(|s| !s.is_none()) {
            for item in series.try_iter()? {
                let item = item?;
                let series = item
                    .cast::<Series>()
                    .map_err(|_| PyTypeError::new_err("LineChart takes Series"))?;
                inner = inner.series(series.get().inner.clone());
            }
        }
        if let Some(height) = height {
            inner = inner.height(height);
        }
        if let Some(width) = width {
            inner = inner.width(width);
        }
        if let Some((lo, hi)) = x_range {
            inner = inner.x_range(lo, hi);
        }
        if let Some((lo, hi)) = y_range {
            inner = inner.y_range(lo, hi);
        }
        Ok(LineChart { inner })
    }

    fn __len__(&self) -> usize {
        self.inner.all_series().len()
    }

    fn __repr__(&self) -> String {
        format!("<LineChart {} series>", self.inner.all_series().len())
    }
}

// ---------------------------------------------------------------------------
// Gauges

/// A threshold band of a `Gauge`: values up to `upto` are in it
/// (`rich_ext::chart::Band`).
#[pyclass(name = "Band", module = "rs_rich.chart", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Band {
    inner: CoreBand,
}

#[pymethods]
impl Band {
    #[new]
    #[pyo3(signature = (upto, label, *, style=None))]
    fn new(upto: f64, label: String, style: Option<String>) -> Self {
        let mut inner = CoreBand::new(upto, label);
        if let Some(style) = style {
            inner = inner.style(style);
        }
        Band { inner }
    }

    #[getter]
    fn upto(&self) -> f64 {
        self.inner.upto
    }

    #[getter]
    fn label(&self) -> &str {
        &self.inner.label
    }

    #[getter]
    fn style(&self) -> Option<&str> {
        self.inner.style.as_deref()
    }

    fn __repr__(&self) -> String {
        format!("Band({}, {})", self.inner.upto, repr_str(&self.inner.label))
    }
}

/// A value against a range, with a target and threshold bands
/// (`rich_ext::chart::Gauge`).
#[pyclass(name = "Gauge", module = "rs_rich.chart", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Gauge {
    inner: CoreGauge,
}
renders!(Gauge);

#[pymethods]
impl Gauge {
    #[new]
    #[pyo3(signature = (label, value, *, range=None, target=None, bands=None, charset="auto", bar_width=None, full_width=false, format=None, unit=None, style=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        label: String,
        value: f64,
        range: Option<(f64, f64)>,
        target: Option<f64>,
        bands: Option<&Bound<'_, PyAny>>,
        charset: &str,
        bar_width: Option<usize>,
        full_width: bool,
        format: Option<&Bound<'_, PyAny>>,
        unit: Option<String>,
        style: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreGauge::new(label, value)
            .charset(self::charset(charset)?)
            .full_width(full_width)
            .format(value_format(format)?);
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(target) = target {
            inner = inner.target(target);
        }
        let bands = each(bands, "bands", |item| {
            item.cast::<Band>()
                .map(|band| band.get().inner.clone())
                .map_err(|_| PyTypeError::new_err("bands must be Bands"))
        })?;
        for band in bands {
            inner = inner.band(band);
        }
        if let Some(width) = bar_width {
            inner = inner.bar_width(width);
        }
        if let Some(unit) = unit {
            inner = inner.unit(unit);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        Ok(Gauge { inner })
    }

    #[getter]
    fn value(&self) -> f64 {
        self.inner.value()
    }

    /// The label of the band the value is in, if any.
    #[getter]
    fn band(&self) -> Option<String> {
        self.inner.current_band().map(|band| band.label.clone())
    }

    fn __repr__(&self) -> String {
        format!("<Gauge {}>", self.inner.value())
    }
}

/// A ratio from 0 to 1, clamped; NaN is refused.
fn ratio(value: f64) -> PyResult<f64> {
    if value.is_nan() {
        return Err(PyValueError::new_err(
            "a gauge's ratio must be a number, not NaN",
        ));
    }
    Ok(value.clamp(0.0, 1.0))
}

/// ratatui's `Gauge`: a bar that fills its area, the label on the middle
/// row (`rich_ext::chart::FillGauge`).
#[pyclass(
    name = "FillGauge",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct FillGauge {
    inner: CoreFillGauge,
}
renders!(FillGauge);

#[pymethods]
impl FillGauge {
    /// `ratio`: from 0 to 1 (clamped). `label`: the text on the middle
    /// row (default the percentage). `style`: ratatui's `gauge_style`, its
    /// foreground the bar's colour and its background the rest's.
    /// `unicode`: eighths of a cell at the bar's edge. `height`: rows
    /// (default: the height given, or one).
    #[new]
    #[pyo3(signature = (ratio, *, label=None, label_style=None, style=None, unicode=false, width=None, height=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        ratio: f64,
        label: Option<String>,
        label_style: Option<String>,
        style: Option<String>,
        unicode: bool,
        width: Option<usize>,
        height: Option<usize>,
    ) -> PyResult<Self> {
        let mut inner = CoreFillGauge::new(self::ratio(ratio)?).unicode(unicode);
        if let Some(label) = label {
            inner = inner.label(label);
        }
        if let Some(style) = label_style {
            inner = inner.label_style(style);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        if let Some(width) = width {
            inner = inner.width(width);
        }
        if let Some(height) = height {
            inner = inner.height(height);
        }
        Ok(FillGauge { inner })
    }

    #[getter]
    fn ratio(&self) -> f64 {
        self.inner.ratio()
    }

    fn __repr__(&self) -> String {
        format!("<FillGauge {}>", self.inner.ratio())
    }
}

/// ratatui's `LineGauge`: a label and a line in two styles
/// (`rich_ext::chart::LineGauge`).
#[pyclass(
    name = "LineGauge",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct LineGauge {
    inner: CoreLineGauge,
}
renders!(LineGauge);

/// One character, for a gauge's symbols.
fn glyph(text: &str, what: &str) -> PyResult<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(PyValueError::new_err(format!(
            "{what} must be one character, got {}",
            repr_str(text)
        ))),
    }
}

#[pymethods]
impl LineGauge {
    /// `ratio`: from 0 to 1 (clamped). `label`: before the line (default
    /// the percentage, ` 42%`). `line`: `"thin"`, `"thick"` or `"double"`;
    /// `symbols`: a `(filled, unfilled)` pair of characters instead.
    #[new]
    #[pyo3(signature = (ratio, *, label=None, label_style=None, filled_style=None, unfilled_style=None, line="thin", symbols=None, width=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        ratio: f64,
        label: Option<String>,
        label_style: Option<String>,
        filled_style: Option<String>,
        unfilled_style: Option<String>,
        line: &str,
        symbols: Option<(String, String)>,
        width: Option<usize>,
    ) -> PyResult<Self> {
        let mut inner = CoreLineGauge::new(self::ratio(ratio)?).line(line_set(line)?);
        if let Some(label) = label {
            inner = inner.label(label);
        }
        if let Some(style) = label_style {
            inner = inner.label_style(style);
        }
        if let Some(style) = filled_style {
            inner = inner.filled_style(style);
        }
        if let Some(style) = unfilled_style {
            inner = inner.unfilled_style(style);
        }
        if let Some((filled, unfilled)) = symbols {
            inner = inner.symbols(glyph(&filled, "symbols")?, glyph(&unfilled, "symbols")?);
        }
        if let Some(width) = width {
            inner = inner.width(width);
        }
        Ok(LineGauge { inner })
    }

    #[getter]
    fn ratio(&self) -> f64 {
        self.inner.ratio()
    }

    fn __repr__(&self) -> String {
        format!("<LineGauge {}>", self.inner.ratio())
    }
}

/// Several gauges, one per line, their columns aligned
/// (`rich_ext::chart::BulletChart`).
#[pyclass(name = "BulletChart", module = "rs_rich.chart", frozen)]
pub(crate) struct BulletChart {
    inner: CoreBulletChart,
    count: usize,
}
renders!(BulletChart);

#[pymethods]
impl BulletChart {
    #[new]
    #[pyo3(signature = (gauges=None, *, bar_width=None, full_width=false, charset="auto"))]
    fn new(
        gauges: Option<&Bound<'_, PyAny>>,
        bar_width: Option<usize>,
        full_width: bool,
        charset: &str,
    ) -> PyResult<Self> {
        let gauges = each(gauges, "gauges", |item| {
            item.cast::<Gauge>()
                .map(|gauge| gauge.get().inner.clone())
                .map_err(|_| PyTypeError::new_err("BulletChart takes Gauges"))
        })?;
        let count = gauges.len();
        let mut inner = CoreBulletChart::new()
            .full_width(full_width)
            .charset(self::charset(charset)?);
        for gauge in gauges {
            inner = inner.gauge(gauge);
        }
        if let Some(width) = bar_width {
            inner = inner.bar_width(width);
        }
        Ok(BulletChart { inner, count })
    }

    fn __len__(&self) -> usize {
        self.count
    }

    fn __repr__(&self) -> String {
        format!("<BulletChart {} gauges>", self.count)
    }
}

// ---------------------------------------------------------------------------
// Heatmaps and status matrices

/// A labelled grid of values as shades, with a legend
/// (`rich_ext::chart::Heatmap`).
#[pyclass(name = "Heatmap", module = "rs_rich.chart", frozen)]
pub(crate) struct Heatmap {
    inner: CoreHeatmap,
}
renders!(Heatmap);

#[pymethods]
impl Heatmap {
    /// `rows`: `(label, values)` pairs or a `{label: values}` mapping;
    /// `None` in values is no data.
    #[new]
    #[pyo3(signature = (rows=None, *, columns=None, range=None, charset="auto", cell_width=None, legend=true, format=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        rows: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        range: Option<(f64, f64)>,
        charset: &str,
        cell_width: Option<usize>,
        legend: bool,
        format: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreHeatmap::new()
            .columns(strings(columns, "columns")?)
            .charset(self::charset(charset)?)
            .legend(legend)
            .format(value_format(format)?);
        if let Some(rows) = rows.filter(|r| !r.is_none()) {
            for item in items(rows)? {
                let (label, values): (String, Bound<'_, PyAny>) = item
                    .extract()
                    .map_err(|_| PyTypeError::new_err("a heatmap row is a (label, values) pair"))?;
                inner = inner.row(label, numbers(&values)?);
            }
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(width) = cell_width {
            inner = inner.cell_width(width);
        }
        Ok(Heatmap { inner })
    }

    /// The scale the shades are read on, as `(min, max)`.
    #[getter]
    fn scale(&self) -> (f64, f64) {
        let scale = self.inner.scale();
        (scale.min(), scale.max())
    }

    fn __repr__(&self) -> String {
        "<Heatmap>".to_string()
    }
}

/// A state a `StatusMatrix` cell can be in (`rich_ext::chart::State`).
#[pyclass(
    name = "ChartState",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ChartState {
    inner: CoreState,
}

#[pymethods]
impl ChartState {
    /// A state called `name`, drawn as `symbol` (`ascii` on an ASCII
    /// console) in `style`.
    #[new]
    #[pyo3(signature = (name, symbol, ascii, style))]
    fn new(name: String, symbol: char, ascii: char, style: String) -> Self {
        ChartState {
            inner: CoreState::new(name, symbol, ascii, style),
        }
    }

    #[staticmethod]
    #[pyo3(name = "pass_")]
    fn pass_() -> Self {
        ChartState {
            inner: CoreState::pass(),
        }
    }

    #[staticmethod]
    fn fail() -> Self {
        ChartState {
            inner: CoreState::fail(),
        }
    }

    #[staticmethod]
    fn skip() -> Self {
        ChartState {
            inner: CoreState::skip(),
        }
    }

    #[staticmethod]
    fn flaky() -> Self {
        ChartState {
            inner: CoreState::flaky(),
        }
    }

    #[getter]
    fn name(&self) -> &str {
        self.inner.name()
    }

    #[getter]
    fn symbol(&self) -> char {
        self.inner.symbol(false)
    }

    #[getter]
    fn ascii(&self) -> char {
        self.inner.symbol(true)
    }

    #[getter]
    fn style(&self) -> &str {
        self.inner.style()
    }

    fn __repr__(&self) -> String {
        format!(
            "ChartState({}, {}, {}, {})",
            repr_str(self.inner.name()),
            repr_str(&self.inner.symbol(false).to_string()),
            repr_str(&self.inner.symbol(true).to_string()),
            repr_str(self.inner.style())
        )
    }
}

/// Rows and columns of states (pass, fail, skip, flaky, your own), each a
/// symbol and a colour (`rich_ext::chart::StatusMatrix`).
#[pyclass(name = "StatusMatrix", module = "rs_rich.chart", frozen)]
pub(crate) struct StatusMatrix {
    inner: CoreStatusMatrix,
}
renders!(StatusMatrix);

#[pymethods]
impl StatusMatrix {
    /// `rows`: `(label, state names)` pairs or a `{label: state names}`
    /// mapping. `states`: `ChartState`s to add or replace.
    #[new]
    #[pyo3(signature = (rows=None, *, columns=None, states=None, legend=true, charset="auto"))]
    fn new(
        rows: Option<&Bound<'_, PyAny>>,
        columns: Option<&Bound<'_, PyAny>>,
        states: Option<&Bound<'_, PyAny>>,
        legend: bool,
        charset: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreStatusMatrix::new()
            .columns(strings(columns, "columns")?)
            .legend(legend)
            .charset(self::charset(charset)?);
        let states = each(states, "states", |item| {
            item.cast::<ChartState>()
                .map(|state| state.get().inner.clone())
                .map_err(|_| PyTypeError::new_err("states must be ChartStates"))
        })?;
        for state in states {
            inner = inner.state(state);
        }
        if let Some(rows) = rows.filter(|r| !r.is_none()) {
            let bad_row =
                || PyTypeError::new_err("a status matrix row is a (label, [state names]) pair");
            for item in items(rows)? {
                let (label, cells) = pair(&item).ok_or_else(bad_row)?;
                let label: String = label.extract().map_err(|_| bad_row())?;
                let cells = strings(Some(&cells), "a row's state names").map_err(|_| bad_row())?;
                inner = inner.row(label, cells);
            }
        }
        Ok(StatusMatrix { inner })
    }

    /// How many cells are in each state, as `(name, count)` pairs.
    fn counts(&self) -> Vec<(String, usize)> {
        self.inner.counts()
    }

    fn __repr__(&self) -> String {
        "<StatusMatrix>".to_string()
    }
}

// ---------------------------------------------------------------------------
// KPI cards

/// A label, a value, a delta with its direction, a sparkline and a status
/// (`rich_ext::chart::KpiCard`).
#[pyclass(name = "KpiCard", module = "rs_rich.chart", frozen)]
pub(crate) struct KpiCard {
    inner: CoreKpiCard,
}
renders!(KpiCard);

#[pymethods]
impl KpiCard {
    /// Give at most one of `delta`, `delta_percent` and `previous`.
    /// `trend`: numbers for the card's sparkline. `status`: `"ok"`,
    /// `"warning"`, `"critical"` or `"unknown"`.
    #[new]
    #[pyo3(signature = (label, value, *, format=None, unit=None, delta=None, delta_percent=None, previous=None, caption=None, higher_is_better=true, trend=None, status=None, width=None, expand=false, border=true, charset="auto"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        label: String,
        value: f64,
        format: Option<&Bound<'_, PyAny>>,
        unit: Option<String>,
        delta: Option<f64>,
        delta_percent: Option<f64>,
        previous: Option<f64>,
        caption: Option<String>,
        higher_is_better: bool,
        trend: Option<&Bound<'_, PyAny>>,
        status: Option<&str>,
        width: Option<usize>,
        expand: bool,
        border: bool,
        charset: &str,
    ) -> PyResult<Self> {
        if [delta.is_some(), delta_percent.is_some(), previous.is_some()]
            .iter()
            .filter(|given| **given)
            .count()
            > 1
        {
            return Err(PyValueError::new_err(
                "give at most one of delta, delta_percent and previous",
            ));
        }
        let mut inner = CoreKpiCard::new(label, value)
            .format(value_format(format)?)
            .higher_is_better(higher_is_better)
            .expand(expand)
            .border(border)
            .charset(self::charset(charset)?);
        if let Some(unit) = unit {
            inner = inner.unit(unit);
        }
        if let Some(delta) = delta {
            inner = inner.delta(delta);
        }
        if let Some(percent) = delta_percent {
            inner = inner.delta_percent(percent);
        }
        if let Some(previous) = previous {
            inner = inner.previous(previous);
        }
        if let Some(caption) = caption {
            inner = inner.caption(caption);
        }
        if let Some(trend) = trend.filter(|t| !t.is_none()) {
            inner = inner.trend(numbers(trend)?);
        }
        if let Some(status) = status {
            inner = inner.status(self::status(status)?);
        }
        if let Some(width) = width {
            inner = inner.width(width);
        }
        Ok(KpiCard { inner })
    }

    fn __repr__(&self) -> String {
        "<KpiCard>".to_string()
    }
}

// ---------------------------------------------------------------------------
// Timelines

/// A labelled range on a `Timeline` row (`rich_ext::chart::Span`).
#[pyclass(
    name = "TimelineSpan",
    module = "rs_rich.chart",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct TimelineSpan {
    inner: CoreSpan,
    row: String,
    start: f64,
    end: f64,
}

#[pymethods]
impl TimelineSpan {
    #[new]
    #[pyo3(signature = (row, start, end, *, style=None))]
    fn new(row: String, start: f64, end: f64, style: Option<String>) -> Self {
        let mut inner = CoreSpan::new(row.clone(), start, end);
        if let Some(style) = style {
            inner = inner.style(style);
        }
        TimelineSpan {
            inner,
            row,
            start,
            end,
        }
    }

    #[getter]
    fn row(&self) -> &str {
        &self.row
    }

    #[getter]
    fn start(&self) -> f64 {
        self.start
    }

    #[getter]
    fn end(&self) -> f64 {
        self.end
    }

    fn __repr__(&self) -> String {
        format!(
            "TimelineSpan({}, {}, {})",
            repr_str(&self.row),
            self.start,
            self.end
        )
    }
}

/// Labelled ranges on a numeric or seconds scale, overlaps stacked, with
/// milestones (`rich_ext::chart::Timeline`).
#[pyclass(name = "Timeline", module = "rs_rich.chart", frozen)]
pub(crate) struct Timeline {
    inner: CoreTimeline,
}
renders!(Timeline);

#[pymethods]
impl Timeline {
    /// `spans`: `TimelineSpan`s or `(row, start, end)` triples.
    /// `milestones`: `(label, at)` pairs.
    #[new]
    #[pyo3(signature = (spans=None, *, milestones=None, range=None, charset="auto", format=None, unit=None, compress=true, durations=true, width=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        spans: Option<&Bound<'_, PyAny>>,
        milestones: Option<&Bound<'_, PyAny>>,
        range: Option<(f64, f64)>,
        charset: &str,
        format: Option<&Bound<'_, PyAny>>,
        unit: Option<String>,
        compress: bool,
        durations: bool,
        width: Option<usize>,
    ) -> PyResult<Self> {
        let mut inner = CoreTimeline::new()
            .charset(self::charset(charset)?)
            .format(value_format(format)?)
            .compress(compress)
            .durations(durations);
        if let Some(spans) = spans.filter(|s| !s.is_none()) {
            for item in spans.try_iter()? {
                let item = item?;
                let span = if let Ok(span) = item.cast::<TimelineSpan>() {
                    span.get().inner.clone()
                } else {
                    let (row, start, end): (String, Bound<'_, PyAny>, Bound<'_, PyAny>) =
                        item.extract().map_err(|_| {
                            PyTypeError::new_err(
                                "a span is a TimelineSpan or a (row, start, end) triple",
                            )
                        })?;
                    // `None` is a gap, as in every chart: the span keeps
                    // its row but is not drawn.
                    CoreSpan::new(row, number(&start)?, number(&end)?)
                };
                inner = inner.push(span);
            }
        }
        let milestones = each(milestones, "milestones", |item| {
            let bad = || PyTypeError::new_err("a milestone is a (label, at) pair");
            let (label, at) = pair(item).ok_or_else(bad)?;
            Ok((label.extract::<String>().map_err(|_| bad())?, number(&at)?))
        })?;
        for (label, at) in milestones {
            inner = inner.milestone(label, at);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(unit) = unit {
            inner = inner.unit(unit);
        }
        if let Some(width) = width {
            inner = inner.width(width);
        }
        Ok(Timeline { inner })
    }

    /// The row labels, in order.
    #[getter]
    fn rows(&self) -> Vec<String> {
        self.inner.rows().into_iter().map(str::to_string).collect()
    }

    fn __repr__(&self) -> String {
        format!("<Timeline {} rows>", self.inner.rows().len())
    }
}

// ---------------------------------------------------------------------------
// Distributions, small multiples and accessible text (0.0.19 workstream 14)

/// A `Summary` as a dict: `count`, `ignored`, and `min`, `p25`, `p50`,
/// `p75`, `p95`, `max` and `mean` (`None` when there are no values).
fn summary_dict<'py>(py: Python<'py>, s: &CoreSummary) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);
    out.set_item("count", s.count)?;
    out.set_item("ignored", s.ignored)?;
    for (key, value) in [
        ("min", s.min),
        ("p25", s.p25),
        ("p50", s.p50),
        ("p75", s.p75),
        ("p95", s.p95),
        ("max", s.max),
        ("mean", s.mean),
    ] {
        out.set_item(key, (!s.is_empty()).then_some(value))?;
    }
    Ok(out)
}

/// The count, quantiles and mean of values, drawn as a box-and-whisker
/// strip and the numbers on one line (`rich_ext::chart::Distribution`).
#[pyclass(name = "Distribution", module = "rs_rich.chart", frozen)]
pub(crate) struct Distribution {
    inner: CoreDistribution,
}
renders!(Distribution);

#[pymethods]
impl Distribution {
    /// `values`: numbers; `None`, NaN and infinities are left out and
    /// counted. `range`: fix the strip's scale as `(min, max)`.
    /// `strip_width`: cells for the strip (default 20). `unit`: written
    /// after every value. `numbers`: write the numbers after the strip.
    #[new]
    #[pyo3(signature = (values, *, label=None, range=None, strip_width=None, format=None, unit=None, charset="auto", style=None, numbers=true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        values: &Bound<'_, PyAny>,
        label: Option<String>,
        range: Option<(f64, f64)>,
        strip_width: Option<usize>,
        format: Option<&Bound<'_, PyAny>>,
        unit: Option<String>,
        charset: &str,
        style: Option<String>,
        numbers: bool,
    ) -> PyResult<Self> {
        let mut inner = CoreDistribution::new(self::numbers(values)?)
            .format(value_format(format)?)
            .charset(self::charset(charset)?)
            .numbers(numbers);
        if let Some(label) = label {
            inner = inner.label(label);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(width) = strip_width {
            if width == 0 {
                return Err(PyValueError::new_err("strip_width must be at least 1"));
            }
            inner = inner.strip_width(width);
        }
        if let Some(unit) = unit {
            inner = inner.unit(unit);
        }
        if let Some(style) = style {
            inner = inner.style(style);
        }
        Ok(Distribution { inner })
    }

    /// `count`, `ignored`, `min`, `p25`, `p50`, `p75`, `p95`, `max` and
    /// `mean` (`None` without values).
    #[getter]
    fn summary<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        summary_dict(py, &self.inner.summary())
    }

    /// The finite values, sorted.
    #[getter]
    fn values(&self) -> Vec<f64> {
        self.inner.values().to_vec()
    }

    #[getter]
    fn label(&self) -> &str {
        self.inner.label_text()
    }

    /// The `q` quantile (0 to 1), interpolated linearly (numpy's default);
    /// `None` without values.
    fn quantile(&self, q: f64) -> Option<f64> {
        self.inner.quantile(q)
    }

    fn __repr__(&self) -> String {
        format!("<Distribution {} values>", self.inner.summary().count)
    }
}

/// Distributions stacked on one scale, their columns aligned
/// (`rich_ext::chart::DistributionSet`).
#[pyclass(name = "DistributionSet", module = "rs_rich.chart", frozen)]
pub(crate) struct DistributionSet {
    inner: CoreDistributionSet,
}
renders!(DistributionSet);

#[pymethods]
impl DistributionSet {
    /// `distributions`: `Distribution`s, `(label, values)` pairs, or a
    /// dict of label to values. `range`: fix the shared scale.
    /// `shared_scale`: draw every row on one scale with a scale line.
    #[new]
    #[pyo3(signature = (distributions=None, *, range=None, strip_width=None, numbers=true, charset="auto", shared_scale=true))]
    fn new(
        distributions: Option<&Bound<'_, PyAny>>,
        range: Option<(f64, f64)>,
        strip_width: Option<usize>,
        numbers: bool,
        charset: &str,
        shared_scale: bool,
    ) -> PyResult<Self> {
        let mut inner = CoreDistributionSet::new()
            .numbers(numbers)
            .charset(self::charset(charset)?)
            .shared_scale(shared_scale);
        if let Some(distributions) = distributions.filter(|d| !d.is_none()) {
            for item in items(distributions)? {
                let row = if let Ok(d) = item.cast::<Distribution>() {
                    d.get().inner.clone()
                } else {
                    let bad = || {
                        PyTypeError::new_err(
                            "a distribution is a Distribution or a (label, values) pair",
                        )
                    };
                    let (label, values) = pair(&item).ok_or_else(bad)?;
                    let label: String = label.extract().map_err(|_| bad())?;
                    CoreDistribution::new(self::numbers(&values)?).label(label)
                };
                inner = inner.push(row);
            }
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(width) = strip_width {
            if width == 0 {
                return Err(PyValueError::new_err("strip_width must be at least 1"));
            }
            inner = inner.strip_width(width);
        }
        Ok(DistributionSet { inner })
    }

    /// The shared scale, `(min, max)`.
    #[getter]
    fn scale(&self) -> (f64, f64) {
        let scale = self.inner.scale();
        (scale.min(), scale.max())
    }

    /// The rows' labels.
    #[getter]
    fn labels(&self) -> Vec<String> {
        self.inner
            .distributions()
            .iter()
            .map(|d| d.label_text().to_string())
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "<DistributionSet {} distributions>",
            self.inner.distributions().len()
        )
    }
}

/// A chart a `SmallMultiples` grid can hold: a `Sparkline`, a `BarChart`
/// or a `LineChart`.
fn facet(chart: &Bound<'_, PyAny>) -> PyResult<Facet> {
    if let Ok(c) = chart.cast::<Sparkline>() {
        return Ok(c.get().inner.clone().into());
    }
    if let Ok(c) = chart.cast::<BarChart>() {
        return Ok(c.get().inner.clone().into());
    }
    if let Ok(c) = chart.cast::<LineChart>() {
        return Ok(c.get().inner.clone().into());
    }
    Err(PyTypeError::new_err(
        "a small multiple is a Sparkline, a BarChart or a LineChart",
    ))
}

/// A grid of titled charts on one value scale, fitted to the width
/// (`rich_ext::chart::SmallMultiples`).
#[pyclass(name = "SmallMultiples", module = "rs_rich.chart", frozen)]
pub(crate) struct SmallMultiples {
    inner: CoreSmallMultiples,
}
renders!(SmallMultiples);

#[pymethods]
impl SmallMultiples {
    /// `items`: `(title, chart)` pairs or a dict of title to chart, each a
    /// `Sparkline`, `BarChart` or `LineChart`. `columns`: how many columns
    /// (default: as many cells of `min_cell_width` as fit). `range`: fix
    /// the shared scale. `height`: each chart's rows. `note`: the scale and
    /// the one legend under the grid.
    #[new]
    #[pyo3(signature = (items, *, columns=None, min_cell_width=24, gap=2, shared_scale=true, range=None, height=None, note=true, charset="auto"))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        items: &Bound<'_, PyAny>,
        columns: Option<usize>,
        min_cell_width: usize,
        gap: usize,
        shared_scale: bool,
        range: Option<(f64, f64)>,
        height: Option<usize>,
        note: bool,
        charset: &str,
    ) -> PyResult<Self> {
        let mut pairs: Vec<(String, Facet)> = Vec::new();
        for item in self::items(items)? {
            let bad = || PyTypeError::new_err("an item is a (title, chart) pair");
            let (title, chart) = pair(&item).ok_or_else(bad)?;
            pairs.push((title.extract().map_err(|_| bad())?, facet(&chart)?));
        }
        let mut inner = CoreSmallMultiples::new(pairs)
            .min_cell_width(min_cell_width)
            .gap(gap)
            .shared_scale(shared_scale)
            .note(note)
            .charset(self::charset(charset)?);
        if let Some(columns) = columns {
            if columns == 0 {
                return Err(PyValueError::new_err("columns must be at least 1"));
            }
            inner = inner.columns(columns);
        }
        if let Some((lo, hi)) = range {
            inner = inner.range(lo, hi);
        }
        if let Some(height) = height {
            if height == 0 {
                return Err(PyValueError::new_err("height must be at least 1"));
            }
            inner = inner.height(height);
        }
        Ok(SmallMultiples { inner })
    }

    /// How many columns the grid has at `width` cells.
    fn columns_at(&self, width: usize) -> usize {
        self.inner.columns_at(width)
    }

    /// The shared scale, `(min, max)`, or `None` when not shared.
    #[getter]
    fn scale(&self) -> Option<(f64, f64)> {
        self.inner.scale()
    }

    /// The titles, in order.
    #[getter]
    fn titles(&self) -> Vec<String> {
        self.inner.items().iter().map(|(t, _)| t.clone()).collect()
    }

    fn __repr__(&self) -> String {
        format!("<SmallMultiples {} charts>", self.inner.items().len())
    }
}

/// The `q` quantile (0 to 1) of `values` (`None`, NaN and infinities left
/// out), interpolated linearly as numpy's default does; `None` without
/// values.
#[pyfunction]
fn chart_quantile(values: &Bound<'_, PyAny>, q: f64) -> PyResult<Option<f64>> {
    let mut sorted: Vec<f64> = numbers(values)?
        .into_iter()
        .filter(|v| v.is_finite())
        .collect();
    sorted.sort_by(f64::total_cmp);
    Ok(rich_ext::chart::quantile(&sorted, q))
}

/// The summary of `values`: `count`, `ignored`, `min`, `p25`, `p50`,
/// `p75`, `p95`, `max` and `mean`.
#[pyfunction]
fn chart_summary<'py>(py: Python<'py>, values: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    summary_dict(py, &CoreSummary::of(numbers(values)?))
}

/// What a chart of this module says to a screen reader, or `None` for
/// anything else.
pub(crate) fn accessible_text_of(chart: &Bound<'_, PyAny>, width: usize) -> Option<String> {
    use rich_ext::a11y::AccessibleText;
    macro_rules! each {
        ($($ty:ty),*) => {$(
            if let Ok(c) = chart.cast::<$ty>() {
                return Some(c.get().inner.accessible_text(width));
            }
        )*};
    }
    each!(
        Sparkline,
        BarChart,
        Histogram,
        LineChart,
        Gauge,
        BulletChart,
        FillGauge,
        LineGauge,
        Heatmap,
        StatusMatrix,
        KpiCard,
        Timeline,
        Distribution,
        DistributionSet,
        SmallMultiples
    );
    None
}

/// A chart's accessible text: a few lines of plain words for a screen
/// reader (what it is, its ranges and key values).
#[pyfunction]
#[pyo3(signature = (chart, width=80))]
fn chart_describe(chart: &Bound<'_, PyAny>, width: usize) -> PyResult<String> {
    accessible_text_of(chart, width)
        .ok_or_else(|| PyTypeError::new_err("chart_describe takes a chart of rs_rich.chart"))
}

/// `value` as a chart writes it: `format` as for the charts.
#[pyfunction]
#[pyo3(signature = (value, format=None))]
fn chart_format(value: f64, format: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    Ok(value_format(format)?.format(value))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Sparkline>(m)?;
    m.add_class::<ChartBar>()?;
    m.add_class::<BarGroup>()?;
    renderable::add_renderable_class::<BarChart>(m)?;
    renderable::add_renderable_class::<FillGauge>(m)?;
    renderable::add_renderable_class::<LineGauge>(m)?;
    renderable::add_renderable_class::<Histogram>(m)?;
    m.add_class::<Series>()?;
    renderable::add_renderable_class::<LineChart>(m)?;
    m.add_class::<Band>()?;
    renderable::add_renderable_class::<Gauge>(m)?;
    renderable::add_renderable_class::<BulletChart>(m)?;
    renderable::add_renderable_class::<Heatmap>(m)?;
    m.add_class::<ChartState>()?;
    renderable::add_renderable_class::<StatusMatrix>(m)?;
    renderable::add_renderable_class::<KpiCard>(m)?;
    m.add_class::<TimelineSpan>()?;
    renderable::add_renderable_class::<Timeline>(m)?;
    renderable::add_renderable_class::<Distribution>(m)?;
    renderable::add_renderable_class::<DistributionSet>(m)?;
    renderable::add_renderable_class::<SmallMultiples>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(chart_quantile, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chart_summary, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chart_describe, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(chart_format, m)?)?;
    m.add(
        "CHART_STYLES",
        rich_ext::chart::STYLES
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}

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
//! | `ChartBar`, `BarChart`, `Histogram` | `Bar`, `BarChart`, `Histogram` |
//! | `Series`, `LineChart` | `Series`, `LineChart` |
//! | `Band`, `Gauge`, `BulletChart` | `Band`, `Gauge`, `BulletChart` |
//! | `Heatmap` | `Heatmap` |
//! | `ChartState`, `StatusMatrix` | `State`, `StatusMatrix` |
//! | `KpiCard` | `KpiCard` (its `status` by name) |
//! | `TimelineSpan`, `Timeline` | `Span`, `Timeline` |

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use rich::protocol::Renderable;
use rich_ext::chart::{
    Band as CoreBand, Bar as CoreBar, BarChart as CoreBarChart, BulletChart as CoreBulletChart,
    Charset, Gauge as CoreGauge, Heatmap as CoreHeatmap, Histogram as CoreHistogram,
    KpiCard as CoreKpiCard, LineChart as CoreLineChart, Orientation, Series as CoreSeries,
    SeriesKind, Span as CoreSpan, Sparkline as CoreSparkline, State as CoreState, Status,
    StatusMatrix as CoreStatusMatrix, Timeline as CoreTimeline, ValueFormat,
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
    let decimals: usize = format
        .extract()
        .map_err(|_| PyTypeError::new_err("format must be \"compact\" or a number of decimals"))?;
    Ok(ValueFormat::Fixed(decimals))
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

/// Any iterable of `(x, y)` pairs.
fn points(values: &Bound<'_, PyAny>) -> PyResult<Vec<(f64, f64)>> {
    values
        .try_iter()?
        .map(|pair| {
            let pair = pair?;
            let (x, y): (Bound<'_, PyAny>, Bound<'_, PyAny>) = pair
                .extract()
                .map_err(|_| PyTypeError::new_err("a point is an (x, y) pair"))?;
            Ok((number(&x)?, number(&y)?))
        })
        .collect()
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
    /// `threshold`: count (and mark) the values above it.
    #[new]
    #[pyo3(signature = (values, *, range=None, min=None, max=None, charset="auto", style=None, min_max=false, threshold=None, format=None))]
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
    ) -> PyResult<Self> {
        let mut inner = CoreSparkline::new(numbers(values)?)
            .charset(self::charset(charset)?)
            .min_max(min_max)
            .format(value_format(format)?);
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
    #[new]
    #[pyo3(signature = (label, value, *, style=None))]
    fn new(label: String, value: &Bound<'_, PyAny>, style: Option<String>) -> PyResult<Self> {
        let value = number(value)?;
        let mut inner = CoreBar::new(label.clone(), value);
        if let Some(style) = &style {
            inner = inner.style(style.clone());
        }
        Ok(ChartBar {
            inner,
            label,
            value,
            style,
        })
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
    /// mapping. `orientation`: `"horizontal"` or `"vertical"`.
    #[new]
    #[pyo3(signature = (bars=None, *, range=None, max=None, charset="auto", orientation="horizontal", bar_width=None, show_values=true, format=None, style=None))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        bars: Option<&Bound<'_, PyAny>>,
        range: Option<(f64, f64)>,
        max: Option<f64>,
        charset: &str,
        orientation: &str,
        bar_width: Option<usize>,
        show_values: bool,
        format: Option<&Bound<'_, PyAny>>,
        style: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreBarChart::new()
            .charset(self::charset(charset)?)
            .orientation(self::orientation(orientation)?)
            .show_values(show_values)
            .format(value_format(format)?);
        if let Some(bars) = bars.filter(|b| !b.is_none()) {
            for item in items(bars)? {
                inner = inner.push(bar(&item)?);
            }
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
        let first = points.try_iter()?.next().transpose()?;
        let pairs = match first {
            Some(first)
                if first
                    .extract::<(Bound<'_, PyAny>, Bound<'_, PyAny>)>()
                    .is_ok() =>
            {
                self::points(points)?
            }
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
        bands: Option<Vec<PyRef<'_, Band>>>,
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
        for band in bands.unwrap_or_default() {
            inner = inner.band(band.inner.clone());
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
        gauges: Option<Vec<PyRef<'_, Gauge>>>,
        bar_width: Option<usize>,
        full_width: bool,
        charset: &str,
    ) -> PyResult<Self> {
        let gauges = gauges.unwrap_or_default();
        let count = gauges.len();
        let mut inner = CoreBulletChart::new()
            .full_width(full_width)
            .charset(self::charset(charset)?);
        for gauge in gauges {
            inner = inner.gauge(gauge.inner.clone());
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
        columns: Option<Vec<String>>,
        range: Option<(f64, f64)>,
        charset: &str,
        cell_width: Option<usize>,
        legend: bool,
        format: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreHeatmap::new()
            .columns(columns.unwrap_or_default())
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
        columns: Option<Vec<String>>,
        states: Option<Vec<PyRef<'_, ChartState>>>,
        legend: bool,
        charset: &str,
    ) -> PyResult<Self> {
        let mut inner = CoreStatusMatrix::new()
            .columns(columns.unwrap_or_default())
            .legend(legend)
            .charset(self::charset(charset)?);
        for state in states.unwrap_or_default() {
            inner = inner.state(state.inner.clone());
        }
        if let Some(rows) = rows.filter(|r| !r.is_none()) {
            for item in items(rows)? {
                let (label, cells): (String, Vec<String>) = item.extract().map_err(|_| {
                    PyTypeError::new_err("a status matrix row is a (label, [state names]) pair")
                })?;
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
        milestones: Option<Vec<(String, f64)>>,
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
                    let (row, start, end): (String, f64, f64) = item.extract().map_err(|_| {
                        PyTypeError::new_err(
                            "a span is a TimelineSpan or a (row, start, end) triple",
                        )
                    })?;
                    CoreSpan::new(row, start, end)
                };
                inner = inner.push(span);
            }
        }
        for (label, at) in milestones.unwrap_or_default() {
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

/// `value` as a chart writes it: `format` as for the charts.
#[pyfunction]
#[pyo3(signature = (value, format=None))]
fn chart_format(value: f64, format: Option<&Bound<'_, PyAny>>) -> PyResult<String> {
    Ok(value_format(format)?.format(value))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Sparkline>(m)?;
    m.add_class::<ChartBar>()?;
    renderable::add_renderable_class::<BarChart>(m)?;
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

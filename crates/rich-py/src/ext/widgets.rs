//! `rs_rich.ext.badge` (status, label, link and metadata chips),
//! `.size_bar` (a size against a total or limit), `.format` (sizes, rates,
//! durations, times and numbers as people read them) and `.redact`
//! (masking secrets in text, ANSI text and rendered output).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyList;

use rich::protocol::Renderable;
use rich_ext::badge::{Badge as CoreBadge, Badges as CoreBadges};
use rich_ext::format as fmt;
use rich_ext::redact::{Detector, Redacted as CoreRedacted, Redactor as CoreRedactor};
use rich_ext::size_bar::{SizeBar as CoreSizeBar, Units};

use super::common::{self, names, RedactPatternError};
use super::terminal::{self, symbol_set};
use crate::renderable::{self, AsRenderable};

names!(units, units_name, Units, "units", {
    "decimal" => Units::Decimal,
    "binary" => Units::Binary,
});

names!(detector, detector_name, Detector, "detector", {
    "key_value" => Detector::KeyValue,
    "bearer" => Detector::Bearer,
    "token" => Detector::TokenPrefix,
    "aws_access_key" => Detector::AwsAccessKey,
    "jwt" => Detector::Jwt,
    "url_credentials" => Detector::UrlCredentials,
});

// ---------------------------------------------------------------------------
// Badges

/// A small chip: `Badge.status("ok", "passing")`, `Badge.label("beta")`,
/// `Badge.link("docs", url)` or `Badge.meta("version", "1.2")`.
#[pyclass(
    name = "Badge",
    module = "rs_rich.ext.badge",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Badge {
    pub(crate) inner: CoreBadge,
}

fn configure(
    mut badge: CoreBadge,
    style: Option<String>,
    symbols: Option<&str>,
    show_url: Option<bool>,
) -> PyResult<CoreBadge> {
    if let Some(style) = style {
        badge = badge.style(style);
    }
    if let Some(symbols) = symbols {
        badge = badge.symbols(symbol_set(symbols)?);
    }
    if let Some(show) = show_url {
        badge = badge.show_url(show);
    }
    Ok(badge)
}

impl AsRenderable for Badge {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Badge {
    /// A plain label chip.
    #[staticmethod]
    #[pyo3(signature = (text, *, style=None, symbols=None))]
    fn label(text: String, style: Option<String>, symbols: Option<&str>) -> PyResult<Self> {
        Ok(Badge {
            inner: configure(CoreBadge::label(text), style, symbols, None)?,
        })
    }

    /// A status chip (`ok`, `warning`, `error`, `info`, `pending`, `skipped`).
    #[staticmethod]
    #[pyo3(signature = (status, message, *, style=None, symbols=None))]
    fn status(
        status: &str,
        message: String,
        style: Option<String>,
        symbols: Option<&str>,
    ) -> PyResult<Self> {
        Ok(Badge {
            inner: configure(
                CoreBadge::status(terminal::status(status)?, message),
                style,
                symbols,
                None,
            )?,
        })
    }

    /// A linked chip; `show_url` prints the URL where links do not work.
    #[staticmethod]
    #[pyo3(signature = (text, url, *, style=None, symbols=None, show_url=None))]
    fn link(
        text: String,
        url: String,
        style: Option<String>,
        symbols: Option<&str>,
        show_url: Option<bool>,
    ) -> PyResult<Self> {
        Ok(Badge {
            inner: configure(CoreBadge::link(text, url), style, symbols, show_url)?,
        })
    }

    /// A `key value` metadata chip.
    #[staticmethod]
    #[pyo3(signature = (key, value, *, style=None, symbols=None))]
    fn meta(
        key: String,
        value: String,
        style: Option<String>,
        symbols: Option<&str>,
    ) -> PyResult<Self> {
        Ok(Badge {
            inner: configure(CoreBadge::meta(key, value), style, symbols, None)?,
        })
    }

    /// The badge's status, for a status badge.
    #[getter]
    fn get_status(&self) -> Option<&'static str> {
        self.inner.status_of().map(terminal::status_name)
    }

    /// The badge as plain ASCII text (`[OK passing]`).
    #[getter]
    fn plain(&self) -> String {
        self.inner.plain()
    }
}

/// `Badges(badges=(), *, separator=" ")`: badges on one line, wrapping
/// between badges.
#[pyclass(name = "Badges", module = "rs_rich.ext.badge", frozen)]
pub(crate) struct Badges {
    pub(crate) inner: CoreBadges,
}

impl AsRenderable for Badges {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl Badges {
    #[new]
    #[pyo3(signature = (badges=None, *, separator=None))]
    fn new(badges: Option<&Bound<'_, PyAny>>, separator: Option<String>) -> PyResult<Self> {
        let mut items = Vec::new();
        if let Some(badges) = badges {
            for badge in badges.try_iter()? {
                items.push(badge?.extract::<PyRef<'_, Badge>>()?.inner.clone());
            }
        }
        let mut inner = CoreBadges::new(items);
        if let Some(separator) = separator {
            inner = inner.separator(separator);
        }
        Ok(Badges { inner })
    }

    #[getter]
    fn plain(&self) -> String {
        self.inner.plain()
    }

    fn __len__(&self) -> usize {
        self.inner.badges().len()
    }
}

// ---------------------------------------------------------------------------
// Size bars

/// `SizeBar(used, total, *, label=None, units="decimal", bar_width=20,
/// warn_at=None, show_sizes=True, show_percent=True)`: `disk ███░░ 3.2 GB
/// / 8.0 GB 40%`. `SizeBar.limit(used, limit)` warns at 90%.
#[pyclass(name = "SizeBar", module = "rs_rich.ext.size_bar", frozen)]
pub(crate) struct SizeBar {
    inner: CoreSizeBar,
}

impl AsRenderable for SizeBar {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[allow(clippy::too_many_arguments)]
fn size_bar(
    mut bar: CoreSizeBar,
    label: Option<String>,
    units: &str,
    bar_width: usize,
    warn_at: Option<f64>,
    show_sizes: bool,
    show_percent: bool,
) -> PyResult<CoreSizeBar> {
    bar = bar
        .units(self::units(units)?)
        .bar_width(bar_width)
        .show_sizes(show_sizes)
        .show_percent(show_percent);
    if let Some(label) = label {
        bar = bar.label(label);
    }
    if let Some(ratio) = warn_at {
        bar = bar.warn_at(ratio);
    }
    Ok(bar)
}

#[pymethods]
impl SizeBar {
    #[new]
    #[pyo3(signature = (used, total, *, label=None, units="decimal", bar_width=20, warn_at=None, show_sizes=true, show_percent=true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        used: u64,
        total: u64,
        label: Option<String>,
        units: &str,
        bar_width: usize,
        warn_at: Option<f64>,
        show_sizes: bool,
        show_percent: bool,
    ) -> PyResult<Self> {
        Ok(SizeBar {
            inner: size_bar(
                CoreSizeBar::new(used, total),
                label,
                units,
                bar_width,
                warn_at,
                show_sizes,
                show_percent,
            )?,
        })
    }

    /// A bar against a limit, warning from 90%.
    #[staticmethod]
    #[pyo3(signature = (used, limit, *, label=None, units="decimal", bar_width=20, warn_at=None, show_sizes=true, show_percent=true))]
    #[allow(clippy::too_many_arguments)]
    fn limit(
        used: u64,
        limit: u64,
        label: Option<String>,
        units: &str,
        bar_width: usize,
        warn_at: Option<f64>,
        show_sizes: bool,
        show_percent: bool,
    ) -> PyResult<Self> {
        Ok(SizeBar {
            inner: size_bar(
                CoreSizeBar::limit(used, limit),
                label,
                units,
                bar_width,
                warn_at,
                show_sizes,
                show_percent,
            )?,
        })
    }

    /// Used over total (infinite over a zero total).
    #[getter]
    fn ratio(&self) -> f64 {
        self.inner.ratio()
    }

    /// Whether used exceeds the total.
    #[getter]
    fn is_over(&self) -> bool {
        self.inner.is_over()
    }

    /// Whether the ratio passed `warn_at` (and is not over).
    #[getter]
    fn is_high(&self) -> bool {
        self.inner.is_high()
    }
}

/// `format_size(n, *, units="decimal")`: `"1.5 kB"` or `"1.5 KiB"`.
#[pyfunction]
#[pyo3(signature = (n, *, units="decimal"))]
fn format_size(n: u64, units: &str) -> PyResult<String> {
    Ok(self::units(units)?.format(n))
}

/// `format_rate(bytes_per_second)`: `"1.2 MB/s"`.
#[pyfunction]
fn format_rate(bytes_per_second: f64) -> String {
    fmt::rate(bytes_per_second)
}

/// `format_duration(seconds, *, ascii=False)`: `"1m 05s"`, `"350ms"`, `"12µs"`.
#[pyfunction]
#[pyo3(signature = (seconds, *, ascii=false))]
fn format_duration(seconds: &Bound<'_, PyAny>, ascii: bool) -> PyResult<String> {
    Ok(fmt::duration_with(common::seconds(seconds)?, ascii))
}

/// `format_clock(seconds)`: `"1:02:03"`.
#[pyfunction]
fn format_clock(seconds: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(fmt::clock(common::seconds(seconds)?))
}

/// A POSIX timestamp (seconds; or a `datetime`) as a `SystemTime`.
fn system_time(value: &Bound<'_, PyAny>) -> PyResult<SystemTime> {
    let secs: f64 = match value.getattr_opt("timestamp")? {
        Some(method) if value.hasattr("isoformat")? => method.call0()?.extract()?,
        _ => value.extract()?,
    };
    if !secs.is_finite() {
        return Err(PyValueError::new_err("a time must be finite"));
    }
    let offset = Duration::try_from_secs_f64(secs.abs())
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    Ok(if secs >= 0.0 {
        UNIX_EPOCH + offset
    } else {
        UNIX_EPOCH - offset
    })
}

/// `format_relative(then, now)`: `"3 minutes ago"`, `"in 2 hours"` (times
/// are POSIX seconds or `datetime`s).
#[pyfunction]
fn format_relative(then: &Bound<'_, PyAny>, now: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(fmt::relative(system_time(then)?, system_time(now)?))
}

/// `format_timestamp(t)`: `"2026-09-25 12:00:00 UTC"`.
#[pyfunction]
fn format_timestamp(t: &Bound<'_, PyAny>) -> PyResult<String> {
    Ok(fmt::timestamp(system_time(t)?))
}

/// `format_percent(ratio, decimals=0)`: `"42%"`.
#[pyfunction]
#[pyo3(signature = (ratio, decimals=0))]
fn format_percent(ratio: f64, decimals: usize) -> String {
    fmt::percent(ratio, decimals)
}

/// `format_number(n)`: `"1,234,567"`.
#[pyfunction]
fn format_number(n: i64) -> String {
    fmt::number(n)
}

/// `format_compact(n)`: `"1.2k"`, `"3.4M"`.
#[pyfunction]
fn format_compact(n: f64) -> String {
    fmt::compact(n)
}

// ---------------------------------------------------------------------------
// Redaction

/// `Redactor(*, secrets=False, detectors=(), patterns=(), mask="********",
/// preserve_width=False)`: masks secrets in strings, ANSI text and rendered
/// output (**experimental**, best effort: check its output). `secrets=True`
/// turns on every detector; `patterns` are regular expressions (or
/// `(name, pattern)` pairs).
#[pyclass(
    name = "Redactor",
    module = "rs_rich.ext.redact",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Redactor {
    pub(crate) inner: CoreRedactor,
}

/// One secret `Redactor.find` located: character offsets and its kind.
#[pyclass(name = "RedactMatch", module = "rs_rich.ext.redact", frozen)]
pub(crate) struct RedactMatch {
    #[pyo3(get)]
    start: usize,
    #[pyo3(get)]
    end: usize,
    #[pyo3(get)]
    kind: String,
}

#[pymethods]
impl RedactMatch {
    fn __repr__(&self) -> String {
        format!(
            "RedactMatch(start={}, end={}, kind={:?})",
            self.start, self.end, self.kind
        )
    }
}

fn pattern_error(error: rich_ext::redact::PatternError) -> PyErr {
    RedactPatternError::new_err(error.to_string())
}

impl Redactor {
    fn render<T>(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
        width: usize,
        color: bool,
        f: impl FnOnce(&CoreRedactor, &rich::Console, &dyn Renderable) -> T,
    ) -> PyResult<T> {
        let console = common::plain_console(width, color);
        common::scoped(py, width, 25, color, || {
            let value = renderable::to_renderable(renderable, None)?;
            Ok(f(&self.inner, &console, value.as_ref()))
        })
    }
}

#[pymethods]
impl Redactor {
    #[new]
    #[pyo3(signature = (*, secrets=false, detectors=None, patterns=None, mask=None, preserve_width=false))]
    fn new(
        secrets: bool,
        detectors: Option<&Bound<'_, PyAny>>,
        patterns: Option<&Bound<'_, PyAny>>,
        mask: Option<String>,
        preserve_width: bool,
    ) -> PyResult<Self> {
        let mut inner = if secrets {
            CoreRedactor::secrets()
        } else {
            CoreRedactor::new()
        };
        if let Some(detectors) = detectors {
            for name in common::strings(detectors)? {
                inner = inner.detector(detector(&name)?);
            }
        }
        if let Some(patterns) = patterns {
            for item in patterns.try_iter()? {
                let item = item?;
                inner = match item.extract::<String>() {
                    Ok(pattern) => inner.pattern(&pattern).map_err(pattern_error)?,
                    Err(_) => {
                        let (name, pattern): (String, String) = item.extract()?;
                        inner.named_pattern(name, &pattern).map_err(pattern_error)?
                    }
                };
            }
        }
        if let Some(mask) = mask {
            inner = inner.mask(mask);
        }
        Ok(Redactor {
            inner: inner.preserve_width(preserve_width),
        })
    }

    /// Every built-in detector.
    #[staticmethod]
    fn secrets() -> Self {
        Redactor {
            inner: CoreRedactor::secrets(),
        }
    }

    /// Whether no rule is set.
    #[getter]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// The secrets in one line, with character offsets.
    fn find(&self, line: &str) -> Vec<RedactMatch> {
        let index = common::CharIndex::new(line);
        self.inner
            .find(line)
            .into_iter()
            .map(|m| RedactMatch {
                start: index.get(m.start),
                end: index.get(m.end),
                kind: m.kind,
            })
            .collect()
    }

    /// `text` with its secrets masked.
    fn redact(&self, text: &str) -> String {
        self.inner.redact_str(text)
    }

    /// ANSI text with its secrets masked (escape sequences kept).
    fn redact_ansi(&self, text: &str) -> String {
        self.inner.redact_ansi(text)
    }

    /// Chunks of a stream, masking secrets split across chunks.
    fn redact_chunks(&self, chunks: Vec<String>) -> Vec<String> {
        self.inner.redact_chunks(&chunks)
    }

    /// Byte chunks of a stream (`bytes` objects).
    fn redact_byte_chunks(&self, chunks: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
        self.inner.redact_byte_chunks(&chunks)
    }

    /// Command-line arguments with secret values (`--token X`) masked.
    fn redact_args(&self, args: Vec<String>) -> Vec<String> {
        self.inner.redact_args(&args)
    }

    /// `Segment`s with their secrets masked.
    fn redact_segments<'py>(
        &self,
        py: Python<'py>,
        segments: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let segments: Vec<_> = segments
            .try_iter()?
            .map(|s| {
                Ok(s?
                    .extract::<PyRef<'_, crate::segment::Segment>>()?
                    .to_core())
            })
            .collect::<PyResult<_>>()?;
        crate::segment::to_python(py, &self.inner.redact_segments(&segments))
    }

    /// Render `renderable` and return it as ANSI text, redacted.
    #[pyo3(signature = (renderable, *, width=80, color=true))]
    fn capture(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
        width: usize,
        color: bool,
    ) -> PyResult<String> {
        self.render(py, renderable, width, color, |r, c, v| {
            r.capture(c, |c| c.print(v))
        })
    }

    /// Render `renderable` as plain text, redacted.
    #[pyo3(signature = (renderable, *, width=80))]
    fn export_text(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
        width: usize,
    ) -> PyResult<String> {
        self.render(py, renderable, width, true, |r, c, v| {
            r.export_text(c, |c| c.print(v))
        })
    }

    /// Render `renderable` as HTML with inline styles, redacted.
    #[pyo3(signature = (renderable, *, width=80, inline_styles=true))]
    fn export_html(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
        width: usize,
        inline_styles: bool,
    ) -> PyResult<String> {
        self.render(py, renderable, width, true, |r, c, v| {
            if inline_styles {
                r.export_html(c, |c| c.print(v))
            } else {
                r.export_html_classes(c, |c| c.print(v))
            }
        })
    }

    /// Render `renderable` as SVG, redacted.
    #[pyo3(signature = (renderable, *, width=80, title="Rich", unique_id="rich"))]
    fn export_svg(
        &self,
        py: Python<'_>,
        renderable: &Bound<'_, PyAny>,
        width: usize,
        title: &str,
        unique_id: &str,
    ) -> PyResult<String> {
        self.render(py, renderable, width, true, |r, c, v| {
            r.export_svg(c, title, unique_id, |c| c.print(v))
        })
    }
}

/// `Redacted(renderable, redactor=None)`: any renderable with its secrets
/// masked as it renders (default: `Redactor.secrets()`).
#[pyclass(name = "Redacted", module = "rs_rich.ext.redact", frozen)]
pub(crate) struct Redacted {
    renderable: Py<PyAny>,
    redactor: CoreRedactor,
}

impl AsRenderable for Redacted {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let inner = renderable::to_renderable(self.renderable.bind(py), None)?;
        Ok(Box::new(CoreRedacted::new(
            common::Boxed(inner),
            self.redactor.clone(),
        )))
    }
}

#[pymethods]
impl Redacted {
    #[new]
    #[pyo3(signature = (renderable, redactor=None))]
    fn new(renderable: Py<PyAny>, redactor: Option<PyRef<'_, Redactor>>) -> Self {
        Redacted {
            renderable,
            redactor: redactor.map_or_else(CoreRedactor::secrets, |r| r.inner.clone()),
        }
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.renderable)
    }
}

/// `is_secret_key(key)`: whether a key names a secret (`password`, `token`).
#[pyfunction]
fn is_secret_key(key: &str) -> bool {
    rich_ext::redact::is_secret_key(key)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Badge>(m)?;
    renderable::add_renderable_class::<Badges>(m)?;
    renderable::add_renderable_class::<SizeBar>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(format_size, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_rate, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_duration, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_clock, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_relative, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_timestamp, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_percent, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_number, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(format_compact, m)?)?;
    m.add_class::<Redactor>()?;
    m.add_class::<RedactMatch>()?;
    renderable::add_renderable_class::<Redacted>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(is_secret_key, m)?)?;
    m.add(
        "REDACT_DETECTORS",
        Detector::ALL
            .iter()
            .map(|d| detector_name(*d))
            .collect::<Vec<_>>(),
    )?;
    m.add("BADGE_STYLES", rich_ext::badge::STYLES.to_vec())?;
    m.add("SIZE_BAR_STYLES", rich_ext::size_bar::STYLES.to_vec())?;
    Ok(())
}

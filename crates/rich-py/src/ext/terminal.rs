//! Terminals: `rs_rich.ext.capabilities` (what a terminal supports, and
//! why), `.fidelity` (degrading any renderable), `.a11y` (accessibility
//! policy, contrast checks, semantic text), `.ansi_explain`, `.sanitize`,
//! `.encoding` and `.target` (explicit render targets).

use std::collections::BTreeMap;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyDict, PyList};

use rich::color::ColorTriplet;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::{Renderable, Support, TargetCapabilities};
use rich::segment::Segment as CoreSegment;
use rich::theme::Theme as CoreTheme;
use rich::ColorSystem;
use rich_ext::a11y::contrast::{self, CheckOptions, Deficiency, Finding, FindingKind};
use rich_ext::a11y::policy::{AccessibilityPolicy as CorePolicy, Status, SymbolSet};
use rich_ext::ansi_explain::{self as ansi, Explanation as CoreExplanation, Token, ViewMode};
use rich_ext::capabilities::{
    Capabilities, ColorDepth, Environment, Graphics, MapEnvironment, Origin, Overrides,
    Report as CoreReport, SystemEnvironment,
};
use rich_ext::encoding::Encoding;
use rich_ext::fidelity::{self, Fidelity, FidelityFacts, Policy};
use rich_ext::target::{
    resolve_capabilities, CapabilityOrigin, RenderTarget as CoreTarget, TargetKind,
    TargetObservations, TargetOverrides,
};

use super::common::{self, names, EncodingError};
use crate::renderable::{self, AsRenderable};
use crate::segment::Segment;

names!(symbol_set, symbol_set_name, SymbolSet, "symbols", {
    "unicode" => SymbolSet::Unicode,
    "ascii" => SymbolSet::Ascii,
    "words" => SymbolSet::Words,
});

names!(status, status_name, Status, "status", {
    "ok" => Status::Ok,
    "warning" => Status::Warning,
    "error" => Status::Error,
    "info" => Status::Info,
    "pending" => Status::Pending,
    "skipped" => Status::Skipped,
});

names!(fidelity_level, fidelity_name, Fidelity, "fidelity", {
    "ascii" => Fidelity::Ascii,
    "plain" => Fidelity::Plain,
    "styled" => Fidelity::Styled,
    "rich" => Fidelity::Rich,
    "animated" => Fidelity::Animated,
});

names!(deficiency, deficiency_name, Deficiency, "deficiency", {
    "protan" => Deficiency::Protan,
    "deutan" => Deficiency::Deutan,
    "tritan" => Deficiency::Tritan,
});

names!(target_kind, target_kind_name, TargetKind, "target kind", {
    "terminal" => TargetKind::Terminal,
    "plain_stream" => TargetKind::PlainStream,
    "capture" => TargetKind::Capture,
    "html" => TargetKind::Html,
    "svg" => TargetKind::Svg,
    "custom" => TargetKind::Custom,
});

names!(support, support_name, Support, "support", {
    "unsupported" => Support::Unsupported,
    "inferred" => Support::Inferred,
    "confirmed" => Support::Confirmed,
});

/// `"truecolor"`, `"256"`, `"standard"`, `"windows"` or `None`.
fn color_system(value: Option<&str>) -> PyResult<Option<ColorSystem>> {
    Ok(match value {
        None | Some("none") => None,
        Some("truecolor") => Some(ColorSystem::Truecolor),
        Some("256") => Some(ColorSystem::EightBit),
        Some("standard") => Some(ColorSystem::Standard),
        Some("windows") => Some(ColorSystem::Windows),
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "invalid color_system {other:?}; expected truecolor, 256, standard, windows or None"
            )))
        }
    })
}

fn color_system_name(value: Option<ColorSystem>) -> Option<&'static str> {
    value.map(|system| match system {
        ColorSystem::Truecolor => "truecolor",
        ColorSystem::EightBit => "256",
        ColorSystem::Standard => "standard",
        ColorSystem::Windows => "windows",
    })
}

/// An environment: the process's own, or `env` (a dict of variables) with
/// the given terminal facts.
fn environment(
    env: Option<&Bound<'_, PyAny>>,
    terminal: Option<bool>,
    size: Option<(usize, usize)>,
    windows: bool,
) -> PyResult<Box<dyn Environment>> {
    let Some(env) = env.filter(|v| !v.is_none()) else {
        if terminal.is_none() && size.is_none() && !windows {
            return Ok(Box::new(SystemEnvironment));
        }
        let mut map = MapEnvironment::new().windows(windows);
        for (key, value) in std::env::vars() {
            map = map.var(&key, &value);
        }
        map = map.terminal(terminal.unwrap_or_else(|| SystemEnvironment.is_terminal()));
        if let Some((width, height)) = size {
            map = map.size(width, height);
        }
        return Ok(Box::new(map));
    };
    let mut map = MapEnvironment::new()
        .terminal(terminal.unwrap_or(false))
        .windows(windows);
    for (key, value) in common::pairs(env)? {
        map = map.var(&key, &value.str()?.to_string());
    }
    if let Some((width, height)) = size {
        map = map.size(width, height);
    }
    Ok(Box::new(map))
}

// ---------------------------------------------------------------------------
// Accessibility

/// `AccessibilityPolicy(*, compact=False, screen_reader=False, ...)`: how
/// output adapts for screen readers, reduced motion, high contrast and
/// monochrome terminals.
#[pyclass(
    name = "AccessibilityPolicy",
    module = "rs_rich.ext.a11y",
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct AccessibilityPolicy {
    pub(crate) inner: CorePolicy,
}

pub(crate) fn policy_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CorePolicy>> {
    value
        .filter(|v| !v.is_none())
        .map(|v| Ok(v.extract::<PyRef<'_, AccessibilityPolicy>>()?.inner.clone()))
        .transpose()
}

#[pymethods]
impl AccessibilityPolicy {
    #[new]
    #[pyo3(signature = (
        *, compact=false, screen_reader=false, no_animation=false, reduced_motion=false,
        high_contrast=false, monochrome=false, status_symbols="unicode"
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        compact: bool,
        screen_reader: bool,
        no_animation: bool,
        reduced_motion: bool,
        high_contrast: bool,
        monochrome: bool,
        status_symbols: &str,
    ) -> PyResult<Self> {
        Ok(AccessibilityPolicy {
            inner: CorePolicy {
                compact,
                screen_reader,
                no_animation,
                reduced_motion,
                high_contrast,
                monochrome,
                status_symbols: symbol_set(status_symbols)?,
                warnings: Vec::new(),
            },
        })
    }

    /// Compact, words for symbols, no animation.
    #[staticmethod]
    fn for_screen_reader() -> Self {
        AccessibilityPolicy {
            inner: CorePolicy::screen_reader(),
        }
    }

    #[staticmethod]
    fn for_reduced_motion() -> Self {
        AccessibilityPolicy {
            inner: CorePolicy::reduced_motion(),
        }
    }

    #[staticmethod]
    fn for_high_contrast() -> Self {
        AccessibilityPolicy {
            inner: CorePolicy::high_contrast(),
        }
    }

    #[staticmethod]
    fn for_monochrome() -> Self {
        AccessibilityPolicy {
            inner: CorePolicy::monochrome(),
        }
    }

    /// From `NO_COLOR` and `RICH_A11Y` (`screen-reader,reduced-motion,...`)
    /// in `env` (a dict), or the process environment.
    #[staticmethod]
    #[pyo3(signature = (env=None))]
    fn from_env(env: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let env = environment(env, None, None, false)?;
        Ok(AccessibilityPolicy {
            inner: CorePolicy::from_env(env.as_ref()),
        })
    }

    #[getter]
    fn compact(&self) -> bool {
        self.inner.compact
    }
    #[getter]
    fn screen_reader(&self) -> bool {
        self.inner.screen_reader
    }
    #[getter]
    fn no_animation(&self) -> bool {
        self.inner.no_animation
    }
    #[getter]
    fn reduced_motion(&self) -> bool {
        self.inner.reduced_motion
    }
    #[getter]
    fn high_contrast(&self) -> bool {
        self.inner.high_contrast
    }
    #[getter]
    fn monochrome(&self) -> bool {
        self.inner.monochrome
    }
    #[getter]
    fn status_symbols(&self) -> &'static str {
        symbol_set_name(self.inner.status_symbols)
    }
    #[getter]
    fn warnings(&self) -> Vec<String> {
        self.inner.warnings.clone()
    }

    /// The richest fidelity this policy allows.
    fn fidelity_ceiling(&self) -> &'static str {
        fidelity_name(self.inner.fidelity_ceiling())
    }

    /// The marker for a status (`"✔ ok"`, `"[OK]"` or `"ok:"`).
    fn status(&self, status: &str) -> PyResult<&'static str> {
        Ok(self.inner.status(self::status(status)?))
    }

    /// `theme` (default: the default theme) adapted: high contrast, no colour.
    #[pyo3(signature = (theme=None))]
    fn theme(&self, py: Python<'_>, theme: Option<&Bound<'_, PyAny>>) -> PyResult<Py<PyAny>> {
        let theme = theme_arg(theme)?;
        py_theme(py, &self.inner.theme(&theme))
    }

    fn __repr__(&self) -> String {
        format!("{:?}", self.inner)
    }
}

/// `status_marker(status, symbols="unicode")`: `"✔ ok"`, `"[OK]"` or `"ok:"`.
#[pyfunction]
#[pyo3(signature = (status, symbols="unicode"))]
fn status_marker(status: &str, symbols: &str) -> PyResult<&'static str> {
    Ok(self::status(status)?.symbol(symbol_set(symbols)?))
}

pub(crate) fn theme_arg(value: Option<&Bound<'_, PyAny>>) -> PyResult<CoreTheme> {
    match value.filter(|v| !v.is_none()) {
        None => Ok(CoreTheme::default_theme()),
        Some(value) => Ok(value
            .extract::<PyRef<'_, crate::theme::Theme>>()?
            .inner
            .clone()),
    }
}

/// A core theme as a Python `rs_rich.theme.Theme`.
pub(crate) fn py_theme(py: Python<'_>, theme: &CoreTheme) -> PyResult<Py<PyAny>> {
    let styles = PyDict::new(py);
    let mut names: Vec<&str> = theme.names().collect();
    names.sort_unstable();
    for name in names {
        if let Some(style) = theme.get(name) {
            styles.set_item(name, style.definition())?;
        }
    }
    let kwargs = PyDict::new(py);
    kwargs.set_item("inherit", false)?;
    Ok(py
        .import("rs_rich.theme")?
        .getattr("Theme")?
        .call((styles,), Some(&kwargs))?
        .unbind())
}

type Rgb = (u8, u8, u8);

fn triplet(value: &Bound<'_, PyAny>) -> PyResult<ColorTriplet> {
    if let Ok((red, green, blue)) = value.extract::<Rgb>() {
        return Ok(ColorTriplet { red, green, blue });
    }
    let text: String = value.extract()?;
    let color = rich::Color::parse(&text).map_err(|e| PyValueError::new_err(e.to_string()))?;
    color
        .get_truecolor()
        .ok_or_else(|| PyValueError::new_err(format!("{text:?} has no RGB value")))
}

fn rgb(c: ColorTriplet) -> Rgb {
    (c.red, c.green, c.blue)
}

/// `contrast_ratio(a, b)`: WCAG contrast of two colours (`(r, g, b)` or a
/// colour name / `#rrggbb`).
#[pyfunction]
fn contrast_ratio(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<f64> {
    Ok(contrast::contrast_ratio(triplet(a)?, triplet(b)?))
}

/// `relative_luminance(color)`: WCAG relative luminance.
#[pyfunction]
fn relative_luminance(color: &Bound<'_, PyAny>) -> PyResult<f64> {
    Ok(contrast::relative_luminance(triplet(color)?))
}

/// `delta_e(a, b)`: CIEDE2000 colour difference.
#[pyfunction]
fn delta_e(a: &Bound<'_, PyAny>, b: &Bound<'_, PyAny>) -> PyResult<f64> {
    Ok(contrast::delta_e(triplet(a)?, triplet(b)?))
}

/// `to_lab(color)`: CIE L*a*b*.
#[pyfunction]
fn to_lab(color: &Bound<'_, PyAny>) -> PyResult<[f64; 3]> {
    Ok(contrast::to_lab(triplet(color)?))
}

/// `simulate_deficiency(color, deficiency)`: the colour as seen with
/// `protan`, `deutan` or `tritan` colour vision deficiency.
#[pyfunction]
fn simulate_deficiency(color: &Bound<'_, PyAny>, deficiency: &str) -> PyResult<Rgb> {
    Ok(rgb(contrast::simulate(
        triplet(color)?,
        self::deficiency(deficiency)?,
    )))
}

/// `suggest_color(fg, bg, min_ratio=4.5)`: the nearest foreground reaching
/// `min_ratio` against `bg`, or `None`.
#[pyfunction]
#[pyo3(signature = (fg, bg, min_ratio=4.5))]
fn suggest_color(
    fg: &Bound<'_, PyAny>,
    bg: &Bound<'_, PyAny>,
    min_ratio: f64,
) -> PyResult<Option<Rgb>> {
    Ok(contrast::suggest_color(triplet(fg)?, triplet(bg)?, min_ratio).map(rgb))
}

/// One problem `check_theme` found.
#[pyclass(
    name = "ContrastFinding",
    module = "rs_rich.ext.a11y",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct ContrastFinding {
    inner: Finding,
}

#[pymethods]
impl ContrastFinding {
    #[getter]
    fn style_name(&self) -> String {
        self.inner.style_name.clone()
    }
    /// `low_contrast`, `color_only_distinction` or `color_blind_confusable`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner.kind {
            FindingKind::LowContrast { .. } => "low_contrast",
            FindingKind::ColorOnlyDistinction { .. } => "color_only_distinction",
            FindingKind::ColorBlindConfusable { .. } => "color_blind_confusable",
        }
    }
    #[getter]
    fn severity(&self) -> &'static str {
        match self.inner.severity {
            contrast::Severity::Error => "error",
            contrast::Severity::Warning => "warning",
            contrast::Severity::Info => "info",
        }
    }
    #[getter]
    fn suggestion(&self) -> String {
        self.inner.suggestion.clone()
    }
    /// The contrast ratio, for a `low_contrast` finding.
    #[getter]
    fn ratio(&self) -> Option<f64> {
        match &self.inner.kind {
            FindingKind::LowContrast { ratio, .. } => Some(*ratio),
            _ => None,
        }
    }
    /// A one-line description.
    fn describe(&self) -> String {
        self.inner.describe()
    }
    fn __repr__(&self) -> String {
        format!(
            "<ContrastFinding {}: {}>",
            self.inner.style_name,
            self.inner.describe()
        )
    }
}

/// `check_theme(theme=None, *, backgrounds=None, min_ratio=4.5,
/// error_ratio=3.0, cvd_threshold=10.0, groups=None)`: low contrast,
/// colour-only distinctions and colour-blind confusions in a theme, against
/// terminal backgrounds (`TerminalTheme`s; default and Monokai).
#[pyfunction]
#[pyo3(signature = (theme=None, *, backgrounds=None, min_ratio=4.5, error_ratio=3.0, cvd_threshold=10.0, groups=None))]
fn check_theme(
    theme: Option<&Bound<'_, PyAny>>,
    backgrounds: Option<&Bound<'_, PyAny>>,
    min_ratio: f64,
    error_ratio: f64,
    cvd_threshold: f64,
    groups: Option<&Bound<'_, PyAny>>,
) -> PyResult<Vec<ContrastFinding>> {
    let theme = theme_arg(theme)?;
    let mut options = CheckOptions {
        min_ratio,
        error_ratio,
        cvd_threshold,
        ..CheckOptions::default()
    };
    if let Some(backgrounds) = backgrounds {
        options.backgrounds = backgrounds
            .try_iter()?
            .map(|t| {
                Ok(t?
                    .extract::<PyRef<'_, crate::terminal_theme::TerminalTheme>>()?
                    .inner
                    .clone())
            })
            .collect::<PyResult<_>>()?;
    }
    if let Some(groups) = groups {
        options.groups = groups
            .try_iter()?
            .map(|g| common::strings(&g?))
            .collect::<PyResult<_>>()?;
    }
    Ok(contrast::check_theme(&theme, &options)
        .into_iter()
        .map(|inner| ContrastFinding { inner })
        .collect())
}

/// `ContrastReport(findings)`: a table of findings, then a summary line.
#[pyclass(name = "ContrastReport", module = "rs_rich.ext.a11y", frozen)]
pub(crate) struct ContrastReport {
    findings: Vec<Finding>,
}

struct OwnedContrast(Vec<Finding>);

impl Renderable for OwnedContrast {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        contrast::ContrastReport::new(&self.0).rich_render(console, options)
    }
}

impl AsRenderable for ContrastReport {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(OwnedContrast(self.findings.clone())))
    }
}

#[pymethods]
impl ContrastReport {
    #[new]
    fn new(findings: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(ContrastReport {
            findings: findings
                .try_iter()?
                .map(|f| Ok(f?.extract::<PyRef<'_, ContrastFinding>>()?.inner.clone()))
                .collect::<PyResult<_>>()?,
        })
    }
}

/// `semantic_text(renderable, width=80)`: any renderable as plain,
/// linear text for a screen reader (decoration dropped).
#[pyfunction]
#[pyo3(signature = (renderable, width=80))]
fn semantic_text(py: Python<'_>, renderable: &Bound<'_, PyAny>, width: usize) -> PyResult<String> {
    common::scoped(py, width, 10_000, false, || {
        let value = renderable::to_renderable(renderable, None)?;
        Ok(rich_ext::a11y::semantic_text(value.as_ref(), width))
    })
}

// ---------------------------------------------------------------------------
// Capabilities

/// What a terminal supports, each fact with where it came from.
#[pyclass(name = "CapabilityReport", module = "rs_rich.ext.capabilities", frozen)]
pub(crate) struct CapabilityReport {
    inner: CoreReport,
}

fn origin_name(origin: &Origin) -> String {
    origin.to_string()
}

impl AsRenderable for CapabilityReport {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(OwnedReport(self.inner.clone())))
    }
}

struct OwnedReport(CoreReport);

impl Renderable for OwnedReport {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        rich_ext::capabilities::CapabilityReport::new(&self.0).rich_render(console, options)
    }
}

#[pymethods]
impl CapabilityReport {
    /// `color` depth: `none`, `16`, `256` or `truecolor`.
    #[getter]
    fn color(&self) -> &'static str {
        self.inner.color.value.name()
    }
    /// The console `color_system` it implies.
    #[getter]
    fn color_system(&self) -> Option<&'static str> {
        color_system_name(self.inner.color.value.color_system())
    }
    #[getter]
    fn unicode(&self) -> bool {
        self.inner.unicode.value
    }
    #[getter]
    fn hyperlinks(&self) -> bool {
        self.inner.hyperlinks.value
    }
    /// `none`, `sixel`, `kitty` or `iterm`.
    #[getter]
    fn graphics(&self) -> &'static str {
        self.inner.graphics.value.name()
    }
    #[getter]
    fn sixel(&self) -> bool {
        self.inner.sixel.value
    }
    #[getter]
    fn width(&self) -> usize {
        self.inner.width.value
    }
    #[getter]
    fn height(&self) -> usize {
        self.inner.height.value
    }
    #[getter]
    fn interactive(&self) -> bool {
        self.inner.interactive.value
    }
    #[getter]
    fn animation(&self) -> bool {
        self.inner.animation.value
    }
    /// The terminal program, when known.
    #[getter]
    fn terminal(&self) -> Option<String> {
        self.inner.terminal.clone()
    }
    /// The CI system, when running in one.
    #[getter]
    fn ci(&self) -> Option<String> {
        self.inner.ci.clone()
    }
    #[getter]
    fn warnings(&self) -> Vec<String> {
        self.inner.warnings.clone()
    }
    /// `(name, value, origin, reason)` for each fact, in report order.
    fn rows(&self) -> Vec<(&'static str, String, String, String)> {
        self.inner
            .rows()
            .into_iter()
            .map(|(name, value, origin, reason)| {
                (name, value, origin_name(origin), reason.to_string())
            })
            .collect()
    }
}

/// `detect_capabilities(env=None, *, terminal=None, size=None,
/// windows=False, color=None, unicode=None, ...)`: what the terminal
/// supports, from `env` (a dict; default: the process environment) and
/// explicit overrides.
#[pyfunction]
#[pyo3(signature = (
    env=None, *, terminal=None, size=None, windows=false, color=None, unicode=None,
    hyperlinks=None, graphics=None, sixel=None, animation=None, interactive=None, width=None,
    height=None
))]
#[allow(clippy::too_many_arguments)]
fn detect_capabilities(
    env: Option<&Bound<'_, PyAny>>,
    terminal: Option<bool>,
    size: Option<(usize, usize)>,
    windows: bool,
    color: Option<&str>,
    unicode: Option<bool>,
    hyperlinks: Option<bool>,
    graphics: Option<&str>,
    sixel: Option<bool>,
    animation: Option<bool>,
    interactive: Option<bool>,
    width: Option<usize>,
    height: Option<usize>,
) -> PyResult<CapabilityReport> {
    let env = environment(env, terminal, size, windows)?;
    let overrides = Overrides {
        color: color
            .map(|c| {
                ColorDepth::parse(c)
                    .ok_or_else(|| PyValueError::new_err(format!("invalid color depth {c:?}")))
            })
            .transpose()?,
        unicode,
        hyperlinks,
        graphics: graphics
            .map(|g| {
                Graphics::parse(g)
                    .ok_or_else(|| PyValueError::new_err(format!("invalid graphics {g:?}")))
            })
            .transpose()?,
        sixel,
        animation,
        interactive,
        width,
        height,
    };
    Ok(CapabilityReport {
        inner: Capabilities::detect_with(env.as_ref(), &overrides),
    })
}

// ---------------------------------------------------------------------------
// Fidelity

/// `select_fidelity(*, unicode=True, color=True, interactive=True,
/// animation=True, ceiling=None, floor=None, allow_animation=True)`: the
/// richest level these facts and limits allow.
#[pyfunction]
#[pyo3(signature = (*, unicode=true, color=true, interactive=true, animation=true, ceiling=None, floor=None, allow_animation=true))]
#[allow(clippy::too_many_arguments)]
fn select_fidelity(
    unicode: bool,
    color: bool,
    interactive: bool,
    animation: bool,
    ceiling: Option<&str>,
    floor: Option<&str>,
    allow_animation: bool,
) -> PyResult<&'static str> {
    let policy = fidelity_policy(ceiling, floor, allow_animation)?;
    let facts = FidelityFacts {
        unicode,
        color,
        interactive,
        animation,
    };
    Ok(fidelity_name(Fidelity::select(&facts, &policy)))
}

fn fidelity_policy(
    ceiling: Option<&str>,
    floor: Option<&str>,
    allow_animation: bool,
) -> PyResult<Policy> {
    Ok(Policy {
        ceiling: ceiling.map(fidelity_level).transpose()?,
        floor: floor.map(fidelity_level).transpose()?,
        allow_animation,
    })
}

/// `Degrade(renderable, *, level=None, ceiling=None, floor=None,
/// allow_animation=True, policy=None)`: render anything at a lower
/// fidelity: `styled` drops colour, `plain` all styles, `ascii` non-ASCII
/// characters too. Without `level`, the console's capabilities decide.
#[pyclass(name = "Degrade", module = "rs_rich.ext.fidelity")]
pub(crate) struct Degrade {
    renderable: Py<PyAny>,
    level: Option<Fidelity>,
    policy: Policy,
}

impl AsRenderable for Degrade {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let inner = renderable::to_renderable(self.renderable.bind(py), None)?;
        let mut degrade = fidelity::Degrade::new(common::Boxed(inner)).policy(self.policy);
        if let Some(level) = self.level {
            degrade = degrade.level(level);
        }
        Ok(Box::new(degrade))
    }
}

#[pymethods]
impl Degrade {
    #[new]
    #[pyo3(signature = (renderable, *, level=None, ceiling=None, floor=None, allow_animation=true, policy=None))]
    fn new(
        renderable: Py<PyAny>,
        level: Option<&str>,
        ceiling: Option<&str>,
        floor: Option<&str>,
        allow_animation: bool,
        policy: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let policy = match policy_arg(policy)? {
            Some(policy) => policy.fidelity_policy(),
            None => fidelity_policy(ceiling, floor, allow_animation)?,
        };
        Ok(Degrade {
            renderable,
            level: level.map(fidelity_level).transpose()?,
            policy,
        })
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.renderable)
    }
}

fn segments_arg(value: &Bound<'_, PyAny>) -> PyResult<Vec<CoreSegment>> {
    value
        .try_iter()?
        .map(|s| Ok(s?.extract::<PyRef<'_, Segment>>()?.to_core()))
        .collect()
}

/// `degrade_segments(segments, level)`: `Segment`s at a fidelity level.
#[pyfunction]
fn degrade_segments<'py>(
    py: Python<'py>,
    segments: &Bound<'py, PyAny>,
    level: &str,
) -> PyResult<Bound<'py, PyList>> {
    let out = fidelity::degrade_segments(segments_arg(segments)?, fidelity_level(level)?);
    crate::segment::to_python(py, &out)
}

/// `ascii_text(text)`: `text` with non-ASCII characters replaced by ASCII
/// look-alikes (`─` → `-`) or `?`.
#[pyfunction]
fn ascii_text(text: &str) -> String {
    fidelity::ascii_text(text)
}

// ---------------------------------------------------------------------------
// ANSI explained

/// One token of an explained string.
#[pyclass(name = "AnsiToken", module = "rs_rich.ext.ansi_explain", frozen)]
pub(crate) struct AnsiToken {
    /// Character offset in the input.
    #[pyo3(get)]
    offset: usize,
    /// Length in characters.
    #[pyo3(get)]
    length: usize,
    /// `text`, `SGR`, `CSI`, `OSC`, `ESC`, `DCS`, `APC`, `PM`, `SOS`,
    /// `control` or `invalid`.
    #[pyo3(get)]
    kind: &'static str,
    #[pyo3(get)]
    raw: String,
    #[pyo3(get)]
    meaning: String,
    /// For SGR: `(code, description)` per effect.
    #[pyo3(get)]
    effects: Vec<(String, String)>,
}

#[pymethods]
impl AnsiToken {
    fn __repr__(&self) -> String {
        format!(
            "AnsiToken({}, {:?}, {:?})",
            self.kind, self.raw, self.meaning
        )
    }
}

/// `explain(text)`: the tokens of a string with escape sequences, and the
/// text a terminal would show. Renders as a table (see `ExplanationView`).
#[pyclass(name = "Explanation", module = "rs_rich.ext.ansi_explain", frozen)]
pub(crate) struct Explanation {
    inner: CoreExplanation,
    source: String,
}

impl AsRenderable for Explanation {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(OwnedExplanationView {
            explanation: self.inner.clone(),
            mode: ViewMode::Table,
            escapes_only: false,
            show_visible: true,
            raw_width: 40,
        }))
    }
}

fn token(source: &str, spanned: &ansi::Spanned) -> AnsiToken {
    let start = common::char_index(source, spanned.offset);
    let end = common::char_index(source, spanned.offset + spanned.len);
    AnsiToken {
        offset: start,
        length: end - start,
        kind: spanned.token.kind(),
        raw: spanned.token.raw(),
        meaning: spanned.token.meaning(),
        effects: match &spanned.token {
            Token::Sgr { effects, .. } => effects
                .iter()
                .map(|e| (e.code.clone(), e.description.clone()))
                .collect(),
            _ => Vec::new(),
        },
    }
}

#[pymethods]
impl Explanation {
    #[getter]
    fn tokens(&self) -> Vec<AnsiToken> {
        self.inner
            .tokens
            .iter()
            .map(|t| token(&self.source, t))
            .collect()
    }
    /// What a terminal would show.
    #[getter]
    fn visible_text(&self) -> String {
        self.inner.visible_text.clone()
    }
    /// The tokens other than text.
    fn escapes(&self) -> Vec<AnsiToken> {
        self.inner
            .escapes()
            .map(|t| token(&self.source, t))
            .collect()
    }
    /// The invalid tokens.
    fn invalid(&self) -> Vec<AnsiToken> {
        self.inner
            .invalid()
            .map(|t| token(&self.source, t))
            .collect()
    }
}

/// `explain(text)`: decode escape sequences into words. `bytes` input
/// decodes 8-bit C1 controls too.
#[pyfunction]
fn explain(text: &Bound<'_, PyAny>) -> PyResult<Explanation> {
    if let Ok(bytes) = text.cast::<PyBytes>() {
        let bytes = bytes.as_bytes();
        return Ok(Explanation {
            inner: ansi::explain_bytes(bytes),
            source: ansi::decode_bytes(bytes),
        });
    }
    let source: String = text.extract()?;
    Ok(Explanation {
        inner: ansi::explain(&source),
        source,
    })
}

struct OwnedExplanationView {
    explanation: CoreExplanation,
    mode: ViewMode,
    escapes_only: bool,
    show_visible: bool,
    raw_width: usize,
}

impl OwnedExplanationView {
    fn view(&self) -> ansi::ExplanationView<'_> {
        ansi::ExplanationView::new(&self.explanation)
            .mode(self.mode)
            .escapes_only(self.escapes_only)
            .show_visible(self.show_visible)
            .raw_width(self.raw_width)
    }
}

impl Renderable for OwnedExplanationView {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.view().rich_render(console, options)
    }
}

/// `ExplanationView(explanation, *, mode="table", escapes_only=False,
/// show_visible=True, raw_width=40)`: an explanation as a table, or inline
/// (`inline` mode puts each sequence's meaning in the text).
#[pyclass(name = "ExplanationView", module = "rs_rich.ext.ansi_explain", frozen)]
pub(crate) struct ExplanationView {
    explanation: CoreExplanation,
    mode: ViewMode,
    escapes_only: bool,
    show_visible: bool,
    raw_width: usize,
}

impl ExplanationView {
    fn owned(&self) -> OwnedExplanationView {
        OwnedExplanationView {
            explanation: self.explanation.clone(),
            mode: self.mode,
            escapes_only: self.escapes_only,
            show_visible: self.show_visible,
            raw_width: self.raw_width,
        }
    }
}

impl AsRenderable for ExplanationView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.owned()))
    }
}

#[pymethods]
impl ExplanationView {
    #[new]
    #[pyo3(signature = (explanation, *, mode="table", escapes_only=false, show_visible=true, raw_width=40))]
    fn new(
        explanation: &Bound<'_, PyAny>,
        mode: &str,
        escapes_only: bool,
        show_visible: bool,
        raw_width: usize,
    ) -> PyResult<Self> {
        let explanation = match explanation.extract::<PyRef<'_, Explanation>>() {
            Ok(e) => e.inner.clone(),
            Err(_) => explain(explanation)?.inner,
        };
        let mode = match mode {
            "table" => ViewMode::Table,
            "inline" => ViewMode::Inline,
            other => {
                return Err(PyValueError::new_err(format!(
                    "invalid mode {other:?}; expected table or inline"
                )))
            }
        };
        Ok(ExplanationView {
            explanation,
            mode,
            escapes_only,
            show_visible,
            raw_width,
        })
    }

    /// The text with each sequence replaced by `⟨its meaning⟩` (`<…>` with
    /// `ascii=True`).
    #[pyo3(signature = (ascii=false))]
    fn inline_text(&self, ascii: bool) -> String {
        self.owned().view().inline_text(ascii)
    }
}

/// `sgr_effects(params)`: what SGR parameters (`"1;31"`) do, as
/// `(code, description)` pairs.
#[pyfunction]
fn sgr_effects(params: &str) -> Vec<(String, String)> {
    ansi::sgr_effects(params)
        .into_iter()
        .map(|e| (e.code, e.description))
        .collect()
}

/// `csi_meaning(params, intermediates, final)`: what a CSI sequence does.
#[pyfunction]
fn csi_meaning(params: &str, intermediates: &str, r#final: char) -> String {
    ansi::csi_meaning(params, intermediates, r#final)
}

/// `osc_meaning(data)`: what an OSC string (`"8;;https://…"`) does.
#[pyfunction]
fn osc_meaning(data: &str) -> String {
    ansi::osc_meaning(data)
}

/// `control_name(byte)`: the name of a C0/C1 control (`"ESC"`).
#[pyfunction]
fn control_name(byte: u8) -> &'static str {
    ansi::control_name(byte)
}

/// `escape_visible(raw)`: an escape sequence with its controls made visible.
#[pyfunction]
fn escape_visible(raw: &str) -> String {
    ansi::escape_visible(raw)
}

// ---------------------------------------------------------------------------
// Sanitizing and decoding

/// `sanitize_terminal_controls(text)`: control characters as inert
/// symbols (`␛`), so untrusted text cannot drive the terminal.
#[pyfunction]
fn sanitize_terminal_controls(text: &str) -> String {
    rich_ext::sanitize::sanitize_terminal_controls(text)
}

/// `sanitize_terminal_and_bidi_controls(text)`: also bidirectional
/// overrides (Trojan Source).
#[pyfunction]
fn sanitize_terminal_and_bidi_controls(text: &str) -> String {
    rich_ext::sanitize::sanitize_terminal_and_bidi_controls(text)
}

/// `sanitize_single_line(text)`: sanitized, with newlines made inert too.
#[pyfunction]
fn sanitize_single_line(text: &str) -> String {
    rich_ext::sanitize::sanitize_single_line(text)
}

/// `is_bidi_control(char)`: whether a character is a bidi control.
#[pyfunction]
fn is_bidi_control(char: char) -> bool {
    rich_ext::sanitize::is_bidi_control(char)
}

/// `decode_text(data, encoding)`: strict decoding (`utf-8`, `utf-16` with a
/// BOM, `utf-16le`, `utf-16be`); never guesses.
#[pyfunction]
fn decode_text(data: &[u8], encoding: &str) -> PyResult<String> {
    let encoding: Encoding = encoding.parse().map_err(PyValueError::new_err)?;
    encoding
        .decode(data)
        .map_err(|e| EncodingError::new_err(e.to_string()))
}

/// `has_utf16_bom(data)`: whether `data` starts with a UTF-16 BOM.
#[pyfunction]
fn has_utf16_bom(data: &[u8]) -> bool {
    rich_ext::encoding::has_utf16_bom(data)
}

// ---------------------------------------------------------------------------
// Render targets

/// `RenderTarget(kind="capture", *, width=80, height=25,
/// color_system="truecolor", interactive=False, unicode=True,
/// hyperlinks=True, sixel="unsupported", theme=None)`: an explicit,
/// deterministic destination. `text(renderable)` renders anything for it.
#[pyclass(
    name = "RenderTarget",
    module = "rs_rich.ext.target",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct RenderTarget {
    pub(crate) inner: CoreTarget,
}

#[pymethods]
impl RenderTarget {
    #[new]
    #[pyo3(signature = (
        kind="capture", *, width=80, height=25, color_system=Some("truecolor"), interactive=false,
        unicode=true, hyperlinks=true, sixel="unsupported", theme=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        kind: &str,
        width: usize,
        height: usize,
        color_system: Option<&str>,
        interactive: bool,
        unicode: bool,
        hyperlinks: bool,
        sixel: &str,
        theme: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let caps = TargetCapabilities {
            width: width.min(crate::limits::MAX_CONSOLE_WIDTH),
            height,
            color_system: self::color_system(color_system)?,
            interactive,
            unicode,
            hyperlinks,
            sixel: support(sixel)?,
        };
        let inner = CoreTarget::new(target_kind(kind)?, caps, theme_arg(theme)?);
        Ok(RenderTarget { inner })
    }

    #[getter]
    fn kind(&self) -> &'static str {
        target_kind_name(self.inner.kind())
    }

    /// The capabilities after the kind's rules (a plain stream has no colour).
    #[getter]
    fn capabilities<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let c = rich::protocol::RenderEnvironment::capabilities(&self.inner);
        capabilities_dict(py, &c)
    }

    /// Render any renderable to a string for this target.
    fn text(&self, py: Python<'_>, renderable: &Bound<'_, PyAny>) -> PyResult<String> {
        let c = rich::protocol::RenderEnvironment::capabilities(&self.inner);
        common::scoped(py, c.width, c.height, c.interactive, || {
            let value = renderable::to_renderable(renderable, None)?;
            Ok(self.inner.text(value.as_ref()))
        })
    }

    /// Render any renderable to `Segment`s for this target.
    fn segments<'py>(
        &self,
        py: Python<'py>,
        renderable: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyList>> {
        let c = rich::protocol::RenderEnvironment::capabilities(&self.inner);
        let segments = common::scoped(py, c.width, c.height, c.interactive, || {
            let value = renderable::to_renderable(renderable, None)?;
            Ok(self.inner.segments(value.as_ref()))
        })?;
        crate::segment::to_python(py, &segments)
    }
}

fn capabilities_dict<'py>(py: Python<'py>, c: &TargetCapabilities) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    dict.set_item("width", c.width)?;
    dict.set_item("height", c.height)?;
    dict.set_item("color_system", color_system_name(c.color_system))?;
    dict.set_item("interactive", c.interactive)?;
    dict.set_item("unicode", c.unicode)?;
    dict.set_item("hyperlinks", c.hyperlinks)?;
    dict.set_item("sixel", support_name(c.sixel))?;
    Ok(dict)
}

/// `resolve_capabilities(*, width=None, height=None, is_terminal=False,
/// color_system=None, unicode=True, hyperlinks=False, sixel="unsupported",
/// overrides=None)`: observed facts plus configured overrides (a dict of
/// the same names) as `(capabilities, origins)`, each origin `configured`,
/// `detected`, `inferred` or `default`.
#[pyfunction]
#[pyo3(signature = (*, width=None, height=None, is_terminal=false, color_system=None, unicode=true, hyperlinks=false, sixel="unsupported", overrides=None))]
#[allow(clippy::too_many_arguments)]
fn resolve_target_capabilities<'py>(
    py: Python<'py>,
    width: Option<usize>,
    height: Option<usize>,
    is_terminal: bool,
    color_system: Option<&str>,
    unicode: bool,
    hyperlinks: bool,
    sixel: &str,
    overrides: Option<&Bound<'py, PyDict>>,
) -> PyResult<(Bound<'py, PyDict>, BTreeMap<String, &'static str>)> {
    let observations = TargetObservations {
        width,
        height,
        is_terminal,
        color_system: self::color_system(color_system)?,
        unicode,
        hyperlinks,
        sixel_hint: support(sixel)?,
    };
    let mut o = TargetOverrides::default();
    if let Some(overrides) = overrides {
        for (key, value) in overrides.iter() {
            let key: String = key.extract()?;
            match key.as_str() {
                "width" => o.width = Some(value.extract()?),
                "height" => o.height = Some(value.extract()?),
                "interactive" => o.interactive = Some(value.extract()?),
                "color_system" => {
                    o.color_system = Some(self::color_system(
                        value.extract::<Option<String>>()?.as_deref(),
                    )?)
                }
                "unicode" => o.unicode = Some(value.extract()?),
                "hyperlinks" => o.hyperlinks = Some(value.extract()?),
                "sixel" => o.sixel = Some(support(&value.extract::<String>()?)?),
                other => return Err(PyTypeError::new_err(format!("unknown override {other:?}"))),
            }
        }
    }
    let detected = resolve_capabilities(observations, o);
    let origins = detected
        .origins
        .iter()
        .map(|(name, origin)| {
            let origin = match origin {
                CapabilityOrigin::Configured => "configured",
                CapabilityOrigin::Detected => "detected",
                CapabilityOrigin::Inferred => "inferred",
                CapabilityOrigin::Default => "default",
            };
            (name.clone(), origin)
        })
        .collect();
    Ok((capabilities_dict(py, &detected.capabilities)?, origins))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AccessibilityPolicy>()?;
    m.add_function(pyo3::wrap_pyfunction!(status_marker, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(contrast_ratio, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(relative_luminance, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(delta_e, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(to_lab, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(simulate_deficiency, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(suggest_color, m)?)?;
    m.add_class::<ContrastFinding>()?;
    m.add_function(pyo3::wrap_pyfunction!(check_theme, m)?)?;
    renderable::add_renderable_class::<ContrastReport>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(semantic_text, m)?)?;
    renderable::add_renderable_class::<CapabilityReport>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_capabilities, m)?)?;
    m.add(
        "CAPABILITY_OVERRIDE_VARS",
        rich_ext::capabilities::OVERRIDE_VARS.to_vec(),
    )?;
    m.add_function(pyo3::wrap_pyfunction!(select_fidelity, m)?)?;
    renderable::add_renderable_class::<Degrade>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(degrade_segments, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(ascii_text, m)?)?;
    m.add(
        "FIDELITY_LEVELS",
        Fidelity::ALL
            .iter()
            .map(|f| fidelity_name(*f))
            .collect::<Vec<_>>(),
    )?;
    m.add_class::<AnsiToken>()?;
    renderable::add_renderable_class::<Explanation>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(explain, m)?)?;
    renderable::add_renderable_class::<ExplanationView>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(sgr_effects, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(csi_meaning, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(osc_meaning, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(control_name, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(escape_visible, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(sanitize_terminal_controls, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(
        sanitize_terminal_and_bidi_controls,
        m
    )?)?;
    m.add_function(pyo3::wrap_pyfunction!(sanitize_single_line, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(is_bidi_control, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(decode_text, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(has_utf16_bom, m)?)?;
    m.add_class::<RenderTarget>()?;
    m.add_function(pyo3::wrap_pyfunction!(resolve_target_capabilities, m)?)?;
    Ok(())
}

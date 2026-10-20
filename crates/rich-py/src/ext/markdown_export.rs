//! `rs_rich.ext.markdown_export`: GitHub-flavoured Markdown from renderables
//! (`rich_ext::markdown_export`), for pull request comments, issue bodies and
//! job summaries.
//!
//! The Rust exporter reads core renderables through their accessors; here
//! each Python renderable is converted to the core value it prints as, then
//! exported: a `Table` becomes a GFM table, a `Tree` nested lists, a `Panel`
//! a blockquote or `<details>` block, a `Rule` `---`, `Text` and markup
//! inline Markdown, a `Syntax` a fenced block, a `Markdown` its source. A
//! `Group` (or `Renderables`) writes each item as its own block by the same
//! rules; anything else is its plain rendering in a ```` ```text ```` fence.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;

use rich::protocol::Renderable;
use rich::Text as CoreText;
use rich_ext::markdown_export::{
    self as gfm, GfmDocument as CoreDocument, GfmExporter as CoreExporter,
    GfmOptions as CoreOptions, MarkdownSource, PanelStyle as CorePanelStyle, ToGfm,
};

use super::common;
use crate::art::repr_str;
use crate::limits::MAX_CONSOLE_WIDTH;
use crate::renderable::{self, Nesting};
use crate::text::Text;

// ---------------------------------------------------------------------------
// Options

/// How a `Panel` is written (`rich_ext::markdown_export::PanelStyle`): a
/// blockquote (`"blockquote"`, the default) or a collapsible `<details>`
/// block (`"details"`, closed unless `open`).
#[pyclass(
    name = "PanelStyle",
    module = "rs_rich.ext.markdown_export",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct PanelStyle {
    inner: CorePanelStyle,
}

fn panel_style(kind: &str, open: bool) -> PyResult<CorePanelStyle> {
    match kind {
        "blockquote" if !open => Ok(CorePanelStyle::Blockquote),
        "blockquote" => Err(PyValueError::new_err(
            "open applies to the details style only",
        )),
        "details" => Ok(CorePanelStyle::Details { open }),
        other => Err(PyValueError::new_err(format!(
            "invalid panel style {other:?}; expected blockquote or details"
        ))),
    }
}

#[pymethods]
impl PanelStyle {
    #[new]
    #[pyo3(signature = (kind="blockquote", *, open=false))]
    fn new(kind: &str, open: bool) -> PyResult<Self> {
        Ok(PanelStyle {
            inner: panel_style(kind, open)?,
        })
    }

    /// A blockquote, the title in bold on its first line.
    #[staticmethod]
    fn blockquote() -> Self {
        PanelStyle {
            inner: CorePanelStyle::Blockquote,
        }
    }

    /// A `<details>` block with the title as its summary.
    #[staticmethod]
    #[pyo3(signature = (open=false))]
    fn details(open: bool) -> Self {
        PanelStyle {
            inner: CorePanelStyle::Details { open },
        }
    }

    /// `"blockquote"` or `"details"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner {
            CorePanelStyle::Blockquote => "blockquote",
            CorePanelStyle::Details { .. } => "details",
        }
    }

    /// Whether a details block starts open.
    #[getter]
    fn open(&self) -> bool {
        matches!(self.inner, CorePanelStyle::Details { open: true })
    }

    fn __eq__(&self, other: &Bound<'_, PyAny>) -> bool {
        other
            .extract::<PyRef<'_, PanelStyle>>()
            .is_ok_and(|other| other.inner == self.inner)
    }

    fn __hash__(&self) -> u64 {
        match self.inner {
            CorePanelStyle::Blockquote => 0,
            CorePanelStyle::Details { open } => 1 + u64::from(open),
        }
    }

    fn __repr__(&self) -> String {
        match self.inner {
            CorePanelStyle::Blockquote => "PanelStyle('blockquote')".into(),
            CorePanelStyle::Details { open } => format!(
                "PanelStyle('details', open={})",
                if open { "True" } else { "False" }
            ),
        }
    }
}

/// The default code style names (`GfmOptions.code_styles`).
const CODE_STYLES: [&str; 3] = ["code", "markdown.code", "inline_code"];

/// Options for the exporter (`rich_ext::markdown_export::GfmOptions`).
/// `width`: the width plain renderings are made at. `panel`: a
/// `PanelStyle`, or `"blockquote"` / `"details"`. `code_styles`: style names
/// written as inline code. `untitled_summary`: the summary of a details
/// block for a panel with no title.
#[pyclass(
    name = "GfmOptions",
    module = "rs_rich.ext.markdown_export",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct GfmOptions {
    #[pyo3(get)]
    width: usize,
    panel: CorePanelStyle,
    #[pyo3(get)]
    code_styles: Vec<String>,
    #[pyo3(get)]
    untitled_summary: String,
}

impl Default for GfmOptions {
    fn default() -> Self {
        GfmOptions {
            width: 100,
            panel: CorePanelStyle::Blockquote,
            code_styles: CODE_STYLES.iter().map(|s| s.to_string()).collect(),
            untitled_summary: "Details".into(),
        }
    }
}

impl GfmOptions {
    fn to_core(&self) -> CoreOptions {
        CoreOptions::new()
            .width(self.width)
            .panel(self.panel)
            .code_styles(self.code_styles.iter().cloned())
            .untitled_summary(self.untitled_summary.clone())
    }

    fn exporter(&self) -> CoreExporter {
        CoreExporter::with_options(self.to_core())
    }
}

#[pymethods]
impl GfmOptions {
    #[new]
    #[pyo3(signature = (*, width=100, panel=None, code_styles=None, untitled_summary="Details".to_string()))]
    fn new(
        width: usize,
        panel: Option<&Bound<'_, PyAny>>,
        code_styles: Option<Vec<String>>,
        untitled_summary: String,
    ) -> PyResult<Self> {
        if width == 0 || width > MAX_CONSOLE_WIDTH {
            return Err(PyValueError::new_err(format!(
                "width must be from 1 to {MAX_CONSOLE_WIDTH}, got {width}"
            )));
        }
        let panel = match panel.filter(|value| !value.is_none()) {
            None => CorePanelStyle::Blockquote,
            Some(value) => {
                if let Ok(style) = value.extract::<PyRef<'_, PanelStyle>>() {
                    style.inner
                } else if let Ok(kind) = value.cast::<PyString>() {
                    panel_style(kind.to_cow()?.as_ref(), false)?
                } else {
                    return Err(PyTypeError::new_err(
                        "panel must be a PanelStyle, \"blockquote\" or \"details\"",
                    ));
                }
            }
        };
        Ok(GfmOptions {
            width,
            panel,
            code_styles: code_styles
                .unwrap_or_else(|| CODE_STYLES.iter().map(|s| s.to_string()).collect()),
            untitled_summary,
        })
    }

    #[getter]
    fn panel(&self) -> PanelStyle {
        PanelStyle { inner: self.panel }
    }

    fn __repr__(&self) -> String {
        format!(
            "GfmOptions(width={}, panel={}, code_styles=[{}], untitled_summary={})",
            self.width,
            PanelStyle { inner: self.panel }.__repr__(),
            self.code_styles
                .iter()
                .map(|name| repr_str(name))
                .collect::<Vec<_>>()
                .join(", "),
            repr_str(&self.untitled_summary)
        )
    }
}

fn options_arg(options: Option<PyRef<'_, GfmOptions>>) -> GfmOptions {
    options.map(|o| o.clone()).unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Python renderables to Markdown

/// `value` as a Markdown block, in a render scope (Python renderables may
/// render to be read back).
fn export(py: Python<'_>, exporter: &CoreExporter, value: &Bound<'_, PyAny>) -> PyResult<String> {
    let width = exporter.options_width();
    common::scoped(py, width, 25, false, || block(py, exporter, value))
}

/// The Markdown of one value; see the module docs for what becomes what.
fn block(py: Python<'_>, exporter: &CoreExporter, value: &Bound<'_, PyAny>) -> PyResult<String> {
    let _nesting = Nesting::enter()?;
    let value = renderable::rich_cast(value)?;
    if let Ok(markup) = value.cast::<PyString>() {
        let markup = markup.to_cow()?;
        // Bad markup raises, as printing it does.
        renderable::render_str_markup(markup.to_string())?;
        return Ok(exporter.markup(&markup));
    }
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(exporter.text(&text.inner));
    }
    if let Ok(table) = value.extract::<PyRef<'_, crate::table::Table>>() {
        return Ok(table.build(py)?.to_gfm(exporter));
    }
    if let Ok(tree) = value.extract::<PyRef<'_, crate::renderables::Tree>>() {
        return Ok(tree.build(py, true)?.to_gfm(exporter));
    }
    if let Ok(panel) = value.extract::<PyRef<'_, crate::panel::Panel>>() {
        let child = renderable::rich_cast(&panel.child(py))?;
        // A `str` or `Text` body is read as text; anything else is rendered
        // and read back by the exporter.
        let body: Option<Box<dyn Renderable>> = if let Ok(markup) = child.cast::<PyString>() {
            let markup = renderable::render_str_markup(markup.to_cow()?.into_owned())?;
            Some(Box::new(
                exporter.console().render_str(&markup, Some(false)),
            ))
        } else if let Ok(text) = child.extract::<PyRef<'_, Text>>() {
            Some(Box::new(text.inner.clone()))
        } else {
            None
        };
        return Ok(panel.build(py, body)?.to_gfm(exporter));
    }
    if let Ok(rule) = value.extract::<PyRef<'_, crate::renderables::Rule>>() {
        return Ok(rule.build(py)?.to_gfm(exporter));
    }
    if let Ok(syntax) = value.extract::<PyRef<'_, crate::code::Syntax>>() {
        return Ok(syntax.core_syntax().to_gfm(exporter));
    }
    if let Ok(markdown) = value.extract::<PyRef<'_, crate::code::Markdown>>() {
        return Ok(MarkdownSource(markdown.source()).to_gfm(exporter));
    }
    let items = if let Ok(group) = value.extract::<PyRef<'_, crate::renderables::Group>>() {
        Some(group.items(py))
    } else if let Ok(list) = value.extract::<PyRef<'_, crate::renderables::Renderables>>() {
        Some(list.items(py))
    } else {
        None
    };
    if let Some(items) = items {
        let mut blocks = Vec::new();
        for item in items {
            let written = block(py, exporter, &item)?;
            if !written.is_empty() {
                blocks.push(written);
            }
        }
        return Ok(blocks.join("\n\n"));
    }
    let renderable = renderable::to_renderable(&value, None)?;
    Ok(exporter.plain(renderable.as_ref()))
}

/// The width an exporter renders at.
trait OptionsWidth {
    fn options_width(&self) -> usize;
}

impl OptionsWidth for CoreExporter {
    fn options_width(&self) -> usize {
        self.console().width()
    }
}

/// `value` (a renderable, `str` markup or `Text`) as GitHub-flavoured
/// Markdown, without a trailing newline.
#[pyfunction]
#[pyo3(signature = (value, options=None))]
fn to_gfm(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
    options: Option<PyRef<'_, GfmOptions>>,
) -> PyResult<String> {
    export(py, &options_arg(options).exporter(), value)
}

/// Writes renderables as Markdown blocks
/// (`rich_ext::markdown_export::GfmExporter`).
#[pyclass(name = "GfmExporter", module = "rs_rich.ext.markdown_export", frozen)]
pub(crate) struct GfmExporter {
    options: GfmOptions,
}

#[pymethods]
impl GfmExporter {
    #[new]
    #[pyo3(signature = (options=None))]
    fn new(options: Option<PyRef<'_, GfmOptions>>) -> Self {
        GfmExporter {
            options: options_arg(options),
        }
    }

    #[getter]
    fn options(&self) -> GfmOptions {
        self.options.clone()
    }

    /// `value` as a Markdown block.
    fn export(&self, py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<String> {
        export(py, &self.options.exporter(), value)
    }

    /// Console markup as inline Markdown; malformed markup is taken
    /// literally.
    fn markup(&self, markup: &str) -> String {
        self.options.exporter().markup(markup)
    }

    /// A `Text` (or a `str`, taken literally) as Markdown paragraphs.
    fn text(&self, text: &Bound<'_, PyAny>) -> PyResult<String> {
        let text = text_arg(text)?;
        Ok(self.options.exporter().text(&text))
    }

    /// A fenced code block; `language` is its info string.
    #[pyo3(signature = (code, language=""))]
    fn code(&self, code: &str, language: &str) -> String {
        self.options.exporter().code(code, language)
    }

    /// Any renderable's plain rendering in a ```` ```text ```` fence.
    fn plain(&self, py: Python<'_>, renderable: &Bound<'_, PyAny>) -> PyResult<String> {
        let exporter = self.options.exporter();
        common::scoped(py, self.options.width, 25, false, || {
            let renderable = renderable::to_renderable(renderable, None)?;
            Ok(exporter.plain(renderable.as_ref()))
        })
    }

    fn __repr__(&self) -> String {
        format!("GfmExporter({})", self.options.__repr__())
    }
}

/// A `Text` argument, or a `str` taken literally.
fn text_arg(value: &Bound<'_, PyAny>) -> PyResult<CoreText> {
    if let Ok(text) = value.extract::<PyRef<'_, Text>>() {
        return Ok(text.inner.clone());
    }
    match value.cast::<PyString>() {
        Ok(string) => Ok(CoreText::new(string.to_cow()?.into_owned())),
        Err(_) => Err(PyTypeError::new_err("expected a Text or a str")),
    }
}

// ---------------------------------------------------------------------------
// Documents

/// A Markdown document built block by block
/// (`rich_ext::markdown_export::GfmDocument`). Each `push` returns the
/// document, so calls chain; `finish()` (or `str()`) is the Markdown.
#[pyclass(name = "GfmDocument", module = "rs_rich.ext.markdown_export")]
pub(crate) struct GfmDocument {
    options: GfmOptions,
    blocks: Vec<String>,
}

impl GfmDocument {
    /// The core document holding these blocks, to finish.
    fn core(&self) -> CoreDocument {
        let mut document = CoreDocument::with_options(self.options.to_core());
        for block in &self.blocks {
            document.push_markdown(block);
        }
        document
    }

    fn add(slf: &mut PyRefMut<'_, Self>, block: String) {
        if !block.is_empty() {
            slf.blocks.push(block);
        }
    }
}

#[pymethods]
impl GfmDocument {
    #[new]
    #[pyo3(signature = (options=None))]
    fn new(options: Option<PyRef<'_, GfmOptions>>) -> Self {
        GfmDocument {
            options: options_arg(options),
            blocks: Vec::new(),
        }
    }

    #[getter]
    fn options(&self) -> GfmOptions {
        self.options.clone()
    }

    /// Append `value` (a renderable, `str` markup or `Text`) as a block.
    fn push<'py>(
        mut slf: PyRefMut<'py, Self>,
        value: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let block = export(slf.py(), &slf.options.exporter(), value)?;
        Self::add(&mut slf, block);
        Ok(slf)
    }

    /// Append any renderable's plain rendering in a ```` ```text ```` fence.
    fn push_plain<'py>(
        mut slf: PyRefMut<'py, Self>,
        renderable: &Bound<'py, PyAny>,
    ) -> PyResult<PyRefMut<'py, Self>> {
        let exporter = slf.options.exporter();
        let width = slf.options.width;
        let block = common::scoped(slf.py(), width, 25, false, || {
            let renderable = renderable::to_renderable(renderable, None)?;
            Ok(exporter.plain(renderable.as_ref()))
        })?;
        Self::add(&mut slf, block);
        Ok(slf)
    }

    /// Append Markdown as it is.
    fn push_markdown<'py>(mut slf: PyRefMut<'py, Self>, source: &str) -> PyRefMut<'py, Self> {
        let block = MarkdownSource(source).to_gfm(&slf.options.exporter());
        Self::add(&mut slf, block);
        slf
    }

    /// Append a fenced code block.
    #[pyo3(signature = (code, language=""))]
    fn push_code<'py>(
        mut slf: PyRefMut<'py, Self>,
        code: &str,
        language: &str,
    ) -> PyRefMut<'py, Self> {
        let block = slf.options.exporter().code(code, language);
        Self::add(&mut slf, block);
        slf
    }

    /// Append an ATX heading (`level` 1 to 6) whose text is console markup.
    fn push_heading<'py>(
        mut slf: PyRefMut<'py, Self>,
        level: usize,
        markup: &str,
    ) -> PyRefMut<'py, Self> {
        let mut document = CoreDocument::with_options(slf.options.to_core());
        document.push_heading(level, markup);
        let block = document.blocks().concat();
        // A heading is kept even when its text is empty, as Rust's is.
        slf.blocks.push(block);
        slf
    }

    /// The blocks written so far.
    #[getter]
    fn blocks(&self) -> Vec<String> {
        self.blocks.clone()
    }

    /// The document: blocks separated by blank lines, ending in a newline
    /// (empty when there are none).
    fn finish(&self) -> String {
        self.core().finish()
    }

    fn __str__(&self) -> String {
        self.finish()
    }

    fn __len__(&self) -> usize {
        self.blocks.len()
    }

    fn __repr__(&self) -> String {
        format!("<GfmDocument blocks={}>", self.blocks.len())
    }
}

// ---------------------------------------------------------------------------
// GitHub's limit

/// `markdown` cut to fit a GitHub comment (`GITHUB_COMMENT_LIMIT`
/// characters), with a note saying so.
#[pyfunction]
fn fit_github_comment(markdown: &str) -> String {
    gfm::fit_github_comment(markdown)
}

/// `markdown` cut to at most `limit` characters at a line break, closing a
/// code fence or `<details>` block left open, with a note giving how much
/// was kept; text that fits is returned unchanged.
#[pyfunction]
fn truncate_markdown(markdown: &str, limit: usize) -> String {
    gfm::truncate_markdown(markdown, limit)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PanelStyle>()?;
    m.add_class::<GfmOptions>()?;
    m.add_class::<GfmExporter>()?;
    m.add_class::<GfmDocument>()?;
    m.add_function(pyo3::wrap_pyfunction!(to_gfm, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(fit_github_comment, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(truncate_markdown, m)?)?;
    m.add("GITHUB_COMMENT_LIMIT", gfm::GITHUB_COMMENT_LIMIT)?;
    Ok(())
}

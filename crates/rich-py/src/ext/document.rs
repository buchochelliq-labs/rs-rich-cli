//! `rs_rich.ext.document`: Markdown, reStructuredText, Org, AsciiDoc and
//! man pages behind one interface (`rich_ext::document`): rendering by
//! format name or detection, the diagnostics, content sniffing and the
//! format list.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use rich::protocol::Renderable;
use rich_ext::document::{
    DetectedBy, Diagnostic as CoreDiagnostic, DocumentRegistry, RenderOptions, Severity, CERTAIN,
    LIKELY,
};

use crate::art::{repr_float, repr_str};
use crate::renderable::{self, AsRenderable};

fn registry() -> DocumentRegistry {
    DocumentRegistry::with_builtins()
}

/// A confidence as Python shows it: `0.6`, not `0.6000000238418579`.
fn decimal(value: f32) -> f64 {
    (f64::from(value) * 10_000.0).round() / 10_000.0
}

/// The id of the format named `name` (an id or an alias), or a
/// `ValueError` listing the known ones.
fn format_id(registry: &DocumentRegistry, name: &str) -> PyResult<String> {
    registry
        .get(name)
        .map(|format| format.id().to_string())
        .ok_or_else(|| {
            PyValueError::new_err(format!(
                "unknown document format {}; use one of: {}",
                repr_str(name),
                registry.ids().join(", ")
            ))
        })
}

/// A document to print: `DocumentView(source, "org")`, or with no format,
/// the one `path` and the content say (a `ValueError` when neither tells).
#[pyclass(
    name = "DocumentView",
    module = "rs_rich.ext.document",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DocumentView {
    source: String,
    format: String,
    hyperlinks: bool,
    code_theme: Option<String>,
    default_language: Option<String>,
}

impl DocumentView {
    fn options(&self) -> RenderOptions {
        let mut options = RenderOptions::new().hyperlinks(self.hyperlinks);
        if let Some(theme) = &self.code_theme {
            options = options.code_theme(theme.as_str());
        }
        if let Some(language) = &self.default_language {
            options = options.default_language(language.as_str());
        }
        options
    }

    fn render(&self) -> PyResult<rich_ext::document::Rendered> {
        registry()
            .render(&self.source, &self.format, &self.options())
            .map_err(|err| PyValueError::new_err(err.to_string()))
    }
}

impl AsRenderable for DocumentView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(self.render()?.renderable)
    }
}

#[pymethods]
impl DocumentView {
    #[new]
    #[pyo3(signature = (source, format=None, *, path=None, hyperlinks=true, code_theme=None, default_language=None))]
    fn new(
        source: String,
        format: Option<&str>,
        path: Option<&str>,
        hyperlinks: bool,
        code_theme: Option<String>,
        default_language: Option<String>,
    ) -> PyResult<Self> {
        let registry = registry();
        let format = match format {
            Some(name) => format_id(&registry, name)?,
            None => registry
                .detect(path, &source, LIKELY)
                .map(|detection| detection.format)
                .ok_or_else(|| {
                    PyValueError::new_err(format!(
                        "cannot tell the document's format; pass format= (one of: {})",
                        registry.ids().join(", ")
                    ))
                })?,
        };
        Ok(DocumentView {
            source,
            format,
            hyperlinks,
            code_theme,
            default_language,
        })
    }

    /// The format's id (`"org"`, `"asciidoc"`, `"man"`, `"markdown"`, `"rst"`).
    #[getter]
    fn format(&self) -> &str {
        &self.format
    }

    #[getter]
    fn source(&self) -> &str {
        &self.source
    }

    #[getter]
    fn hyperlinks(&self) -> bool {
        self.hyperlinks
    }

    /// What the parser could not show as written, as `DocumentDiagnostic`s.
    #[getter]
    fn diagnostics(&self) -> PyResult<Vec<DocumentDiagnostic>> {
        Ok(self
            .render()?
            .diagnostics
            .iter()
            .map(DocumentDiagnostic::from)
            .collect())
    }

    fn __repr__(&self) -> String {
        format!(
            "DocumentView(<{} characters>, format={})",
            self.source.chars().count(),
            repr_str(&self.format)
        )
    }
}

/// `render_document(source, format=None, *, path=None, ...)`: a
/// `DocumentView`, as the constructor makes one.
#[pyfunction]
#[pyo3(signature = (source, format=None, *, path=None, hyperlinks=true, code_theme=None, default_language=None))]
fn render_document(
    source: String,
    format: Option<&str>,
    path: Option<&str>,
    hyperlinks: bool,
    code_theme: Option<String>,
    default_language: Option<String>,
) -> PyResult<DocumentView> {
    DocumentView::new(
        source,
        format,
        path,
        hyperlinks,
        code_theme,
        default_language,
    )
}

/// Something a parser could not show as written: its 1-based `line` (or
/// `None`), `severity` (`"warning"` when content is not shown, `"info"`
/// otherwise) and `message`.
#[pyclass(
    name = "DocumentDiagnostic",
    module = "rs_rich.ext.document",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DocumentDiagnostic {
    #[pyo3(get)]
    line: Option<usize>,
    #[pyo3(get)]
    severity: &'static str,
    #[pyo3(get)]
    message: String,
}

impl From<&CoreDiagnostic> for DocumentDiagnostic {
    fn from(diagnostic: &CoreDiagnostic) -> Self {
        DocumentDiagnostic {
            line: diagnostic.line,
            severity: match diagnostic.severity {
                Severity::Info => "info",
                Severity::Warning => "warning",
            },
            message: diagnostic.message.clone(),
        }
    }
}

#[pymethods]
impl DocumentDiagnostic {
    fn __str__(&self) -> String {
        match self.line {
            Some(line) => format!("line {line}: {}", self.message),
            None => self.message.clone(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "DocumentDiagnostic(line={}, severity={}, message={})",
            self.line
                .map_or_else(|| "None".to_string(), |line| line.to_string()),
            repr_str(self.severity),
            repr_str(&self.message)
        )
    }
}

/// A detected format: its `format` id, `confidence` (0 to 1) and `by`
/// (`"name"`, `"name_and_content"` or `"content"`).
#[pyclass(
    name = "DocumentDetection",
    module = "rs_rich.ext.document",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DocumentDetection {
    #[pyo3(get)]
    format: String,
    #[pyo3(get)]
    confidence: f64,
    #[pyo3(get)]
    by: &'static str,
}

#[pymethods]
impl DocumentDetection {
    fn __repr__(&self) -> String {
        format!(
            "DocumentDetection(format={}, confidence={}, by={})",
            repr_str(&self.format),
            repr_float(self.confidence),
            repr_str(self.by)
        )
    }
}

/// The format of a document from its text and, when known, its `path`:
/// a format's own extension wins, a "maybe" extension (`.1`, `.asc`) needs
/// the content to agree, any other extension is never a document, and
/// with no extension the content decides at `threshold` (`CERTAIN` by
/// default; `LIKELY` is what `rich --format auto` uses).
#[pyfunction]
#[pyo3(signature = (text, path=None, *, threshold=CERTAIN))]
fn detect_document(text: &str, path: Option<&str>, threshold: f32) -> Option<DocumentDetection> {
    let detection = registry().detect(path, text, threshold)?;
    Some(DocumentDetection {
        format: detection.format,
        confidence: decimal(detection.confidence),
        by: match detection.by {
            DetectedBy::Name => "name",
            DetectedBy::NameAndContent => "name_and_content",
            DetectedBy::Content => "content",
        },
    })
}

/// Every format's confidence that `text` is in it, most confident first.
#[pyfunction]
fn sniff_document(text: &str) -> Vec<(String, f64)> {
    registry()
        .sniff(text)
        .into_iter()
        .map(|(format, confidence)| (format, decimal(confidence)))
        .collect()
}

/// A built-in format: `id`, `name`, `aliases`, `extensions` and `features`.
#[pyclass(
    name = "DocumentFormatInfo",
    module = "rs_rich.ext.document",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct DocumentFormatInfo {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    name: String,
    #[pyo3(get)]
    aliases: Vec<String>,
    #[pyo3(get)]
    extensions: Vec<String>,
    #[pyo3(get)]
    features: Vec<String>,
}

#[pymethods]
impl DocumentFormatInfo {
    fn __repr__(&self) -> String {
        format!(
            "DocumentFormatInfo(id={}, name={})",
            repr_str(&self.id),
            repr_str(&self.name)
        )
    }
}

/// The built-in formats, in detection order.
#[pyfunction]
fn document_formats() -> Vec<DocumentFormatInfo> {
    registry()
        .formats()
        .iter()
        .map(|format| DocumentFormatInfo {
            id: format.id().to_string(),
            name: format.name().to_string(),
            aliases: format.aliases(),
            extensions: format.extensions(),
            features: format.features(),
        })
        .collect()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<DocumentView>(m)?;
    m.add_class::<DocumentDiagnostic>()?;
    m.add_class::<DocumentDetection>()?;
    m.add_class::<DocumentFormatInfo>()?;
    m.add_function(pyo3::wrap_pyfunction!(render_document, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(detect_document, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(sniff_document, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(document_formats, m)?)?;
    m.add("DOCUMENT_LIKELY", decimal(LIKELY))?;
    m.add("DOCUMENT_CERTAIN", decimal(CERTAIN))?;
    m.add("DOCUMENT_STYLES", rich_ext::document::STYLES.to_vec())?;
    Ok(())
}

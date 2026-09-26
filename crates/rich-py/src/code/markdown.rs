//! `rich.markdown`: `Markdown`, rendered by core's `Markdown`.
//!
//! The port's own additions: `highlighter=`, the code highlighter for code
//! blocks and highlighted inline code (a name from the `rich-ext` registry,
//! or a plugin code highlighter), and `fences=`, renderers that draw fenced
//! blocks of their languages (`MermaidFences`, or a plugin fence renderer)
//! instead of highlighting them.

use std::sync::Arc;

use pyo3::prelude::*;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::{CodeHighlighter, FenceRenderer, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::Justify;

use super::syntax::code_highlighter_value;
use crate::convert;
use crate::renderable::{self, AsRenderable};
use crate::style::style_type;

/// What a render needs; the core document is built when it renders, so the
/// root style can be a theme name, as upstream resolves it then.
#[derive(Clone)]
struct Spec {
    markup: String,
    code_theme: String,
    justify: Option<Justify>,
    style: Option<StyleType>,
    hyperlinks: bool,
    inline_code_lexer: Option<String>,
    inline_code_theme: Option<String>,
    highlighter: Option<Arc<dyn CodeHighlighter>>,
    fences: Vec<Arc<dyn FenceRenderer>>,
}

impl Spec {
    fn document(&self, console: &CoreConsole) -> rich::markdown::Markdown {
        let mut markdown = rich::markdown::Markdown::new(&self.markup)
            .hyperlinks(self.hyperlinks)
            .code_theme(self.code_theme.as_str());
        if let Some(justify) = self.justify {
            markdown = markdown.justify(justify);
        }
        if let Some(style) = &self.style {
            if let Ok(style) = console.get_style(style) {
                markdown = markdown.style(style);
            }
        }
        if let Some(lexer) = &self.inline_code_lexer {
            markdown = markdown.inline_code_lexer(lexer.as_str());
        }
        if let Some(theme) = &self.inline_code_theme {
            markdown = markdown.inline_code_theme(theme.as_str());
        }
        if let Some(highlighter) = &self.highlighter {
            markdown = markdown.highlighter(highlighter.clone());
        }
        for fence in &self.fences {
            markdown = markdown.fence_renderer(fence.clone());
        }
        markdown
    }
}

struct Render(Spec);

impl Renderable for Render {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.document(console).rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        self.0.document(console).measure(console, options)
    }
}

/// `rich.markdown.Markdown`: a Markdown document.
#[pyclass(name = "Markdown", module = "rs_rich.markdown")]
pub(crate) struct Markdown {
    spec: Spec,
    #[pyo3(get)]
    justify: Option<String>,
    style: Py<PyAny>,
    highlighter: Option<Py<PyAny>>,
    fences: Vec<Py<PyAny>>,
}

/// A `fences=` item as a core fence renderer: `MermaidFences` natively,
/// anything else through the plugin API's adapter.
fn fence_renderer(value: &Bound<'_, PyAny>) -> PyResult<Arc<dyn FenceRenderer>> {
    if let Ok(mermaid) = value.extract::<PyRef<'_, crate::art::mermaid::MermaidFences>>() {
        return Ok(mermaid.fence_renderer());
    }
    crate::plugins::fence_renderer_arg(value)
}

impl AsRenderable for Markdown {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Render(self.spec.clone())))
    }
}

#[pymethods]
impl Markdown {
    #[new]
    #[pyo3(signature = (
        markup, code_theme="monokai".to_string(), justify=None, style=None, hyperlinks=true,
        inline_code_lexer=None, inline_code_theme=None, *, highlighter=None, fences=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        markup: String,
        code_theme: String,
        justify: Option<String>,
        style: Option<Py<PyAny>>,
        hyperlinks: bool,
        inline_code_lexer: Option<String>,
        inline_code_theme: Option<String>,
        highlighter: Option<Py<PyAny>>,
        fences: Option<Vec<Py<PyAny>>>,
    ) -> PyResult<Self> {
        let fences = fences.unwrap_or_default();
        let style = style.unwrap_or_else(|| {
            "none"
                .into_pyobject(py)
                .map(|s| s.into_any().unbind())
                .expect("str")
        });
        let spec = Spec {
            markup,
            inline_code_theme: inline_code_theme.or_else(|| Some(code_theme.clone())),
            code_theme,
            justify: match justify.as_deref() {
                None => None,
                name => Some(convert::justify(name)?),
            },
            style: style_type(Some(style.bind(py)))?,
            hyperlinks,
            inline_code_lexer,
            highlighter: code_highlighter_value(highlighter.as_ref().map(|h| h.bind(py)))?,
            fences: fences
                .iter()
                .map(|fence| fence_renderer(fence.bind(py)))
                .collect::<PyResult<_>>()?,
        };
        Ok(Markdown {
            spec,
            justify,
            style,
            highlighter,
            fences,
        })
    }

    #[getter]
    fn markup(&self) -> &str {
        &self.spec.markup
    }
    #[getter]
    fn code_theme(&self) -> &str {
        &self.spec.code_theme
    }
    #[getter]
    fn style(&self, py: Python<'_>) -> Py<PyAny> {
        self.style.clone_ref(py)
    }
    #[getter]
    fn hyperlinks(&self) -> bool {
        self.spec.hyperlinks
    }
    #[getter]
    fn inline_code_lexer(&self) -> Option<&str> {
        self.spec.inline_code_lexer.as_deref()
    }
    #[getter]
    fn inline_code_theme(&self) -> Option<&str> {
        self.spec.inline_code_theme.as_deref()
    }

    /// The code highlighter given (a name or a plugin highlighter). Not in
    /// Rich.
    #[getter]
    fn highlighter(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.highlighter.as_ref().map(|h| h.clone_ref(py))
    }

    /// The fence renderers given. Not in Rich.
    #[getter]
    fn fences(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.fences.iter().map(|f| f.clone_ref(py)).collect()
    }

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.style)?;
        if let Some(highlighter) = &self.highlighter {
            visit.call(highlighter)?;
        }
        for fence in &self.fences {
            visit.call(fence)?;
        }
        Ok(())
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Markdown>(m)
}

//! `rich.markdown`: `Markdown`, rendered by core's `Markdown`.
//!
//! The port's own addition is `highlighter=`: the code highlighter (by
//! name, from the `rich-ext` registry) for code blocks and highlighted
//! inline code.

use std::sync::Arc;

use pyo3::prelude::*;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::{CodeHighlighter, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::style::StyleType;
use rich::Justify;

use super::syntax::code_highlighter;
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
    #[pyo3(get)]
    highlighter: Option<String>,
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
        inline_code_lexer=None, inline_code_theme=None, *, highlighter=None
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
        highlighter: Option<String>,
    ) -> PyResult<Self> {
        let style = style.unwrap_or_else(|| "none".into_pyobject(py).map(|s| s.into_any().unbind()).expect("str"));
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
            highlighter: code_highlighter(highlighter.as_deref())?,
        };
        Ok(Markdown {
            spec,
            justify,
            style,
            highlighter,
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

    fn __traverse__(&self, visit: pyo3::PyVisit<'_>) -> Result<(), pyo3::PyTraverseError> {
        visit.call(&self.style)
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<Markdown>(m)
}

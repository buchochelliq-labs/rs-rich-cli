//! Python objects behind Rust's plugin traits (highlighters, text
//! transforms, source renderers, fence renderers), and the Python handles
//! of Rust ones (`TextTransform`, `TextPipeline`, `SourceRenderer`,
//! `FenceRenderer`).
//!
//! Every adapter calls Python with the GIL (`Python::attach`); what it
//! renders renders through the bridge (`renderable::render_object`,
//! `PyRenderable`), inside the scope of the print that reached it.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pyo3::exceptions::{PyNotImplementedError, PyTypeError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple, PyType};

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::{FenceRenderer as CoreFenceRenderer, Highlighter, Renderable};
use rich::segment::Segment as CoreSegment;
use rich::Text as CoreText;
use rich_ext::transform::Pipeline;
use rich_plugin_api::{
    HighlighterFactory, PluginError as CorePluginError, SourceRenderer as CoreSourceRenderer,
    TextTransform as CoreTextTransform,
};

use super::errors::{self, callback_failed};
use crate::protocol::ConsoleOptions;
use crate::renderable::{self, AsRenderable, PyRenderable};
use crate::segment::Segment;
use crate::text::Text;

/// A Python `Text` holding `inner`, built through the class so it stays
/// valid whatever fields the text area adds.
pub(crate) fn py_text(py: Python<'_>, inner: CoreText) -> PyResult<Bound<'_, Text>> {
    let text = py.get_type::<Text>().call0()?.cast_into::<Text>()?;
    text.borrow_mut().inner = inner;
    Ok(text)
}

// ---------------------------------------------------------------------------
// Highlighters (`rich.highlighter.Highlighter`-like objects)

/// A Python highlighter as a Rust one: calls `highlight(text)` on a Python
/// `Text` copy, which it styles in place (as Rich's `Highlighter.highlight`
/// does), or which it replaces by returning a `Text`.
struct PyHighlighter {
    object: Py<PyAny>,
}

impl Highlighter for PyHighlighter {
    fn highlight(&self, text: &mut CoreText) {
        Python::attach(|py| {
            let result = (|| -> PyResult<CoreText> {
                let copy = py_text(py, text.clone())?;
                let returned = self.object.bind(py).call_method1("highlight", (&copy,))?;
                if let Ok(returned) = returned.extract::<PyRef<'_, Text>>() {
                    return Ok(returned.inner.clone());
                }
                let styled = copy.borrow().inner.clone();
                Ok(styled)
            })();
            match result {
                Ok(styled) => *text = styled,
                Err(error) => {
                    callback_failed(py, error);
                }
            }
        })
    }
}

/// Does nothing: stands in for a highlighter whose factory failed.
struct Inert;

impl Highlighter for Inert {
    fn highlight(&self, _text: &mut CoreText) {}
}

/// A highlighter argument as a factory: an object with `highlight(text)`
/// is shared by every console it is installed onto; a class is
/// instantiated once per console, as Rust's factories are called.
pub(crate) fn highlighter_factory_arg(value: &Bound<'_, PyAny>) -> PyResult<HighlighterFactory> {
    let object = value.clone().unbind();
    if value.is_instance_of::<PyType>() {
        return Ok(Box::new(move || {
            Python::attach(|py| match object.bind(py).call0() {
                Ok(instance) => Box::new(PyHighlighter {
                    object: instance.unbind(),
                }) as Box<dyn Highlighter + Send>,
                Err(error) => {
                    callback_failed(py, error);
                    Box::new(Inert)
                }
            })
        }));
    }
    if !value
        .getattr_opt("highlight")?
        .is_some_and(|m| m.is_callable())
    {
        return Err(PyTypeError::new_err(format!(
            "a highlighter needs a highlight(text) method; got {}",
            value.repr()?
        )));
    }
    Ok(Box::new(move || {
        Python::attach(|py| {
            Box::new(PyHighlighter {
                object: object.clone_ref(py),
            }) as Box<dyn Highlighter + Send>
        })
    }))
}

// ---------------------------------------------------------------------------
// Text transforms

/// A Python transform: a callable `f(text) -> Text`, or an object with
/// `transform(text)`. Returning `None` keeps the (possibly styled in place)
/// text it was given.
struct PyTextTransform {
    object: Py<PyAny>,
}

impl CoreTextTransform for PyTextTransform {
    fn transform(&self, text: CoreText) -> Result<CoreText, CorePluginError> {
        Python::attach(|py| {
            let result = (|| -> PyResult<CoreText> {
                let copy = py_text(py, text)?;
                let object = self.object.bind(py);
                let returned = match object.getattr_opt("transform")? {
                    Some(method) => method.call1((&copy,))?,
                    None => object.call1((&copy,))?,
                };
                if returned.is_none() {
                    return Ok(copy.borrow().inner.clone());
                }
                let returned = returned.extract::<PyRef<'_, Text>>().map_err(|_| {
                    PyTypeError::new_err(format!(
                        "a transform must return a Text (or None), not {}",
                        returned
                            .get_type()
                            .name()
                            .map(|n| n.to_string())
                            .unwrap_or_default()
                    ))
                })?;
                Ok(returned.inner.clone())
            })();
            result.map_err(|error| CorePluginError::Other(callback_failed(py, error)))
        })
    }
}

/// A transform argument as Rust's: a handle's own, or a Python one.
pub(crate) fn transform_arg(value: &Bound<'_, PyAny>) -> PyResult<Arc<dyn CoreTextTransform>> {
    if let Ok(handle) = value.extract::<PyRef<'_, TextTransform>>() {
        if let Some(inner) = &handle.inner {
            return Ok(inner.clone());
        }
    }
    if let Ok(pipeline) = value.extract::<PyRef<'_, TextPipeline>>() {
        return Ok(Arc::new(PipelineTransform(pipeline.inner.clone())));
    }
    let has_method = value
        .getattr_opt("transform")?
        .is_some_and(|m| m.is_callable());
    if !has_method && !value.is_callable() {
        return Err(PyTypeError::new_err(format!(
            "a transform must be callable or have a transform(text) method; got {}",
            value.repr()?
        )));
    }
    Ok(Arc::new(PyTextTransform {
        object: value.clone().unbind(),
    }))
}

/// A pipeline registered as one transform.
struct PipelineTransform(Arc<Pipeline<CoreText>>);

impl CoreTextTransform for PipelineTransform {
    fn transform(&self, text: CoreText) -> Result<CoreText, CorePluginError> {
        self.0
            .apply(text)
            .map_err(|error| CorePluginError::Other(error.to_string()))
    }
}

/// A text transform: `transform(text)` returns the rewritten `Text`
/// (calling the object does the same). Subclass it and override
/// `transform` for a Python one; `ExtensionRegistry.transform(name)`
/// returns ones wrapping a registered transform.
#[pyclass(
    name = "TextTransform",
    module = "rs_rich.plugins",
    subclass,
    frozen,
    skip_from_py_object
)]
pub(crate) struct TextTransform {
    pub(crate) inner: Option<Arc<dyn CoreTextTransform>>,
}

#[pymethods]
impl TextTransform {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        TextTransform { inner: None }
    }

    fn transform<'py>(
        &self,
        py: Python<'py>,
        text: PyRef<'py, Text>,
    ) -> PyResult<Bound<'py, Text>> {
        let Some(inner) = self.inner.clone() else {
            return Err(PyNotImplementedError::new_err(
                "a TextTransform subclass must implement transform(text)",
            ));
        };
        let input = text.inner.clone();
        drop(text);
        match errors::direct(|| inner.transform(input)) {
            Ok(output) => py_text(py, output),
            Err(error) => Err(errors::plugin_error(py, &error)),
        }
    }

    fn __call__<'py>(
        slf: &Bound<'py, Self>,
        text: &Bound<'py, PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        slf.call_method1("transform", (text,))
    }
}

/// Registered transforms chained by name (`ExtensionRegistry.text_pipeline`):
/// `apply(text)` runs each in order. A failing stage raises `PluginError`
/// with `kind == "pipeline"` and the stage's name in `stage`.
#[pyclass(name = "TextPipeline", module = "rs_rich.plugins", frozen)]
pub(crate) struct TextPipeline {
    pub(crate) inner: Arc<Pipeline<CoreText>>,
}

#[pymethods]
impl TextPipeline {
    fn names(&self) -> Vec<String> {
        self.inner.names().into_iter().map(str::to_string).collect()
    }

    fn __len__(&self) -> usize {
        self.inner.len()
    }

    fn apply<'py>(&self, py: Python<'py>, text: PyRef<'py, Text>) -> PyResult<Bound<'py, Text>> {
        let input = text.inner.clone();
        drop(text);
        match errors::direct(|| self.inner.apply(input)) {
            Ok(output) => py_text(py, output),
            Err(error) => {
                let raised = errors::plugin_error(py, &CorePluginError::Other(error.to_string()));
                let value = raised.value(py);
                value.setattr("kind", "pipeline")?;
                value.setattr("stage", error.stage.clone())?;
                value.setattr("message", error.error.message().to_string())?;
                Err(raised)
            }
        }
    }

    fn __call__<'py>(&self, py: Python<'py>, text: PyRef<'py, Text>) -> PyResult<Bound<'py, Text>> {
        self.apply(py, text)
    }

    fn __repr__(&self) -> String {
        format!("<TextPipeline {:?}>", self.inner.names())
    }
}

// ---------------------------------------------------------------------------
// Source renderers

/// A Python renderer: a callable `f(source)`, or an object with
/// `render(source)`, returning any renderable.
struct PySourceRenderer {
    object: Py<PyAny>,
}

impl CoreSourceRenderer for PySourceRenderer {
    fn render(&self, source: &str) -> Result<Box<dyn Renderable + Send + Sync>, CorePluginError> {
        Python::attach(|py| {
            let result = (|| -> PyResult<Py<PyAny>> {
                let object = self.object.bind(py);
                let returned = match object.getattr_opt("render")? {
                    Some(method) => method.call1((source,))?,
                    None => object.call1((source,))?,
                };
                if !renderable::is_renderable(&returned)? {
                    return Err(crate::errors::NotRenderableError::new_err(format!(
                        "a renderer must return a renderable, not {}",
                        returned.repr()?
                    )));
                }
                Ok(returned.unbind())
            })();
            match result {
                Ok(object) => {
                    Ok(Box::new(PyRenderable::new(object)) as Box<dyn Renderable + Send + Sync>)
                }
                Err(error) => Err(CorePluginError::Other(callback_failed(py, error))),
            }
        })
    }
}

/// A renderer argument as Rust's: a handle's own, or a Python one.
pub(crate) fn renderer_arg(value: &Bound<'_, PyAny>) -> PyResult<Arc<dyn CoreSourceRenderer>> {
    if let Ok(handle) = value.extract::<PyRef<'_, SourceRenderer>>() {
        if let Some(inner) = &handle.inner {
            return Ok(inner.clone());
        }
    }
    let has_method = value
        .getattr_opt("render")?
        .is_some_and(|m| m.is_callable());
    if !has_method && !value.is_callable() {
        return Err(PyTypeError::new_err(format!(
            "a renderer must be callable or have a render(source) method; got {}",
            value.repr()?
        )));
    }
    Ok(Arc::new(PySourceRenderer {
        object: value.clone().unbind(),
    }))
}

/// Turns source text into a renderable: `render(source)`. Subclass it and
/// override `render` for a Python one; `ExtensionRegistry.renderer(name)`
/// returns ones wrapping a registered renderer (Mermaid's, say).
#[pyclass(
    name = "SourceRenderer",
    module = "rs_rich.plugins",
    subclass,
    frozen,
    skip_from_py_object
)]
pub(crate) struct SourceRenderer {
    pub(crate) inner: Option<Arc<dyn CoreSourceRenderer>>,
}

#[pymethods]
impl SourceRenderer {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        SourceRenderer { inner: None }
    }

    /// The renderable for `source` (it renders when printed).
    fn render(&self, py: Python<'_>, source: &str) -> PyResult<Rendered> {
        let Some(inner) = self.inner.clone() else {
            return Err(PyNotImplementedError::new_err(
                "a SourceRenderer subclass must implement render(source)",
            ));
        };
        match errors::direct(|| inner.render(source)) {
            Ok(rendered) => Ok(Rendered {
                inner: Arc::from(rendered),
            }),
            Err(error) => Err(errors::plugin_error(py, &error)),
        }
    }

    fn __call__(&self, py: Python<'_>, source: &str) -> PyResult<Rendered> {
        self.render(py, source)
    }
}

/// What a `SourceRenderer` returned: a renderable.
#[pyclass(name = "Rendered", module = "rs_rich.plugins", frozen)]
pub(crate) struct Rendered {
    inner: Arc<dyn Renderable + Send + Sync>,
}

struct Shared(Arc<dyn Renderable + Send + Sync>);

impl Renderable for Shared {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

impl AsRenderable for Rendered {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(self.inner.clone())))
    }
}

#[pymethods]
impl Rendered {
    fn __repr__(&self) -> &'static str {
        "<Rendered>"
    }
}

// ---------------------------------------------------------------------------
// Fence renderers

/// A Python fence renderer: an object with
/// `render_fence(language, code, console, options)`, or a callable
/// `f(language, code)`. It returns `None` to decline (the fence is then
/// highlighted as code), a list of `Segment`s, or any renderable.
struct PyFenceRenderer {
    object: Py<PyAny>,
}

impl CoreFenceRenderer for PyFenceRenderer {
    fn render_fence(
        &self,
        language: &str,
        code: &str,
        console: &CoreConsole,
        options: &CoreOptions,
    ) -> Option<Vec<CoreSegment>> {
        Python::attach(|py| {
            let result = (|| -> PyResult<Option<Vec<CoreSegment>>> {
                let object = self.object.bind(py);
                let returned = match object.getattr_opt("render_fence")? {
                    Some(method) => {
                        let ambient = renderable::ambient()?;
                        let python_options = ConsoleOptions::from_core(options, &ambient.base);
                        method.call1((language, code, ambient.console.bind(py), python_options))?
                    }
                    None => object.call1((language, code))?,
                };
                if returned.is_none() {
                    return Ok(None);
                }
                if let Ok(items) = returned.cast::<pyo3::types::PyList>() {
                    let segments: Option<Vec<CoreSegment>> = items
                        .iter()
                        .map(|item| {
                            item.extract::<PyRef<'_, Segment>>()
                                .ok()
                                .map(|s| s.to_core())
                        })
                        .collect();
                    if let Some(segments) = segments {
                        return Ok(Some(renderable::unterminated(segments)));
                    }
                }
                let segments = renderable::render_object(&returned, console, options)?;
                Ok(Some(renderable::unterminated(segments)))
            })();
            result.unwrap_or_else(|error| {
                callback_failed(py, error);
                Some(Vec::new())
            })
        })
    }
}

/// A fence renderer argument as Rust's: a handle's own, or a Python one.
pub(crate) fn fence_renderer_arg(value: &Bound<'_, PyAny>) -> PyResult<Arc<dyn CoreFenceRenderer>> {
    if let Ok(handle) = value.extract::<PyRef<'_, FenceRenderer>>() {
        if let Some(inner) = &handle.inner {
            return Ok(inner.clone());
        }
    }
    let has_method = value
        .getattr_opt("render_fence")?
        .is_some_and(|m| m.is_callable());
    if !has_method && !value.is_callable() {
        return Err(PyTypeError::new_err(format!(
            "a fence renderer must be callable as f(language, code) or have \
             render_fence(language, code, console, options); got {}",
            value.repr()?
        )));
    }
    Ok(Arc::new(PyFenceRenderer {
        object: value.clone().unbind(),
    }))
}

/// Draws Markdown fences of some languages in place of highlighting them:
/// `render_fence(language, code, console, options)` returns the segments
/// (every line ending in `"\n"`, as `Console.render` yields them), or
/// `None` to decline. Subclass it and override `render_fence` for a Python
/// one; `ExtensionRegistry.fences()` and `fence_renderer(language)` return
/// ones wrapping registered renderers.
#[pyclass(
    name = "FenceRenderer",
    module = "rs_rich.plugins",
    subclass,
    frozen,
    skip_from_py_object
)]
pub(crate) struct FenceRenderer {
    pub(crate) inner: Option<Arc<dyn CoreFenceRenderer>>,
}

#[pymethods]
impl FenceRenderer {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> Self {
        FenceRenderer { inner: None }
    }

    #[pyo3(signature = (language, code, console, options=None))]
    fn render_fence<'py>(
        &self,
        py: Python<'py>,
        language: String,
        code: String,
        console: &Bound<'py, PyAny>,
        options: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Option<Bound<'py, PyAny>>> {
        let Some(inner) = self.inner.clone() else {
            return Err(PyNotImplementedError::new_err(
                "a FenceRenderer subclass must implement render_fence(language, code, console, options)",
            ));
        };
        // Render through the console's own pipeline, which has the core
        // console and the scope Python callbacks need.
        let declined = Arc::new(AtomicBool::new(false));
        let fence = Bound::new(
            py,
            Fence {
                inner,
                language,
                code,
                declined: declined.clone(),
            },
        )?;
        let segments = errors::direct(|| console.call_method1("render", (fence, options)))?;
        let segments =
            pyo3::types::PyList::new(py, segments.try_iter()?.collect::<PyResult<Vec<_>>>()?)?;
        if declined.load(Ordering::SeqCst) {
            return Ok(None);
        }
        Ok(Some(segments.into_any()))
    }
}

/// One fence rendered by a Rust fence renderer, as a renderable.
#[pyclass(module = "rs_rich.plugins", frozen)]
pub(crate) struct Fence {
    inner: Arc<dyn CoreFenceRenderer>,
    language: String,
    code: String,
    declined: Arc<AtomicBool>,
}

struct FenceNow {
    inner: Arc<dyn CoreFenceRenderer>,
    language: String,
    code: String,
    declined: Arc<AtomicBool>,
}

impl Renderable for FenceNow {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        match self
            .inner
            .render_fence(&self.language, &self.code, console, options)
        {
            Some(segments) => segments,
            None => {
                self.declined.store(true, Ordering::SeqCst);
                Vec::new()
            }
        }
    }
}

impl AsRenderable for Fence {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(FenceNow {
            inner: self.inner.clone(),
            language: self.language.clone(),
            code: self.code.clone(),
            declined: self.declined.clone(),
        }))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<TextTransform>()?;
    m.add_class::<TextPipeline>()?;
    m.add_class::<SourceRenderer>()?;
    m.add_class::<FenceRenderer>()?;
    renderable::add_renderable_class::<Rendered>(m)?;
    renderable::register_renderable::<Fence>(m.py());
    Ok(())
}

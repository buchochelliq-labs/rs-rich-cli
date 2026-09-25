//! `rich.prompt`: `PromptBase`, `Prompt`, `IntPrompt`, `FloatPrompt` and
//! `Confirm` (`InvalidResponse` and `PromptError` are in the glue module).
//!
//! Attributes live in the instance dict and class attributes, as upstream's
//! do, so a subclass can override `response_type`, `choices`, the messages
//! or any method (`process_response`, `render_default`, ...).

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple, PyType};

use rich::Text as CoreText;

use super::util::{self, Arg};
use crate::renderable;

fn ellipsis(py: Python<'_>) -> Py<PyAny> {
    py.Ellipsis()
}

fn is_ellipsis(value: &Bound<'_, PyAny>) -> bool {
    value.is(value.py().Ellipsis())
}

/// `rich.prompt.PromptBase`: ask for a value, validate it, ask again.
#[pyclass(name = "PromptBase", module = "rs_rich.prompt", subclass, dict, frozen)]
pub(crate) struct PromptBase;

#[pymethods]
impl PromptBase {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(_args: &Bound<'_, PyTuple>, _kwargs: Option<&Bound<'_, PyDict>>) -> PromptBase {
        PromptBase
    }

    #[pyo3(signature = (
        prompt=None, *, console=None, password=false, choices=None, case_sensitive=true,
        show_default=true, show_choices=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn __init__(
        slf: &Bound<'_, Self>,
        prompt: Option<Bound<'_, PyAny>>,
        console: Option<Bound<'_, PyAny>>,
        password: bool,
        choices: Option<Py<PyAny>>,
        case_sensitive: bool,
        show_default: bool,
        show_choices: bool,
    ) -> PyResult<()> {
        let py = slf.py();
        slf.setattr("console", util::console_or_global(py, console)?)?;
        let prompt = match prompt {
            None => util::new_text(py, CoreText::styled("", "prompt"))?,
            Some(prompt) if util::is_str(&prompt) => {
                let mut text =
                    renderable::render_str(&util::to_str(&prompt)?, true, true, false)?;
                text.set_base_style("prompt");
                util::new_text(py, text)?
            }
            Some(prompt) => prompt.unbind(),
        };
        slf.setattr("prompt", prompt)?;
        slf.setattr("password", password)?;
        if let Some(choices) = choices.filter(|c| !c.is_none(py)) {
            slf.setattr("choices", choices)?;
        }
        slf.setattr("case_sensitive", case_sensitive)?;
        slf.setattr("show_default", show_default)?;
        slf.setattr("show_choices", show_choices)?;
        Ok(())
    }

    #[classattr]
    fn response_type(py: Python<'_>) -> Py<PyType> {
        py.get_type::<PyString>().unbind()
    }

    #[classattr]
    fn validate_error_message() -> &'static str {
        "[prompt.invalid]Please enter a valid value"
    }

    #[classattr]
    fn illegal_choice_message() -> &'static str {
        "[prompt.invalid.choice]Please select one of the available options"
    }

    #[classattr]
    fn prompt_suffix() -> &'static str {
        ": "
    }

    #[classattr]
    fn choices(py: Python<'_>) -> Py<PyAny> {
        py.None()
    }

    /// Build a prompt and ask it: `Prompt.ask("Name", default="x")`.
    #[classmethod]
    #[pyo3(signature = (
        prompt=None, *, console=None, password=false, choices=None, case_sensitive=true,
        show_default=true, show_choices=true, default=Arg::Missing, stream=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn ask(
        cls: &Bound<'_, PyType>,
        prompt: Option<Py<PyAny>>,
        console: Option<Py<PyAny>>,
        password: bool,
        choices: Option<Py<PyAny>>,
        case_sensitive: bool,
        show_default: bool,
        show_choices: bool,
        default: Arg,
        stream: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = cls.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item("console", console)?;
        kwargs.set_item("password", password)?;
        kwargs.set_item("choices", choices)?;
        kwargs.set_item("case_sensitive", case_sensitive)?;
        kwargs.set_item("show_default", show_default)?;
        kwargs.set_item("show_choices", show_choices)?;
        let prompt = prompt.unwrap_or_else(|| PyString::new(py, "").into_any().unbind());
        let instance = cls.call((prompt,), Some(&kwargs))?;
        let call_kwargs = PyDict::new(py);
        call_kwargs.set_item("default", default.or_else(|| ellipsis(py)))?;
        call_kwargs.set_item("stream", stream)?;
        Ok(instance.call((), Some(&call_kwargs))?.unbind())
    }

    /// The default as shown in the prompt: `(default)`.
    fn render_default(&self, py: Python<'_>, default: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        util::new_text(
            py,
            CoreText::styled(format!("({})", util::to_str(default)?), "prompt.default"),
        )
    }

    /// The prompt text: the prompt, the choices, the default and the suffix.
    fn make_prompt(slf: &Bound<'_, Self>, default: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let prompt = slf.getattr("prompt")?;
        let mut text = match util::core_text(&prompt) {
            Some(text) => text,
            None => CoreText::new(util::to_str(&prompt)?),
        };
        let choices = slf.getattr("choices")?;
        if slf.getattr("show_choices")?.is_truthy()? && choices.is_truthy()? {
            let names: Vec<String> = choices
                .try_iter()?
                .map(|choice| choice.and_then(|c| util::to_str(&c)))
                .collect::<PyResult<_>>()?;
            text.append(" ", None);
            text.append(&format!("[{}]", names.join("/")), Some("prompt.choices".into()));
        }
        let accepted = PyTuple::new(
            py,
            [
                py.get_type::<PyString>().into_any(),
                slf.getattr("response_type")?,
            ],
        )?;
        if !is_ellipsis(default)
            && slf.getattr("show_default")?.is_truthy()?
            && default.is_instance(&accepted)?
        {
            text.append(" ", None);
            let rendered = slf.call_method1("render_default", (default,))?;
            match util::core_text(&rendered) {
                Some(rendered) => text = text.append_text(&rendered),
                None => text.append(&util::to_str(&rendered)?, None),
            }
        }
        text.append(&util::to_str(&slf.getattr("prompt_suffix")?)?, None);
        util::new_text(py, text)
    }

    /// Read the input: `console.input(prompt, password=..., stream=...)`.
    #[classmethod]
    #[pyo3(signature = (console, prompt, password, stream=None))]
    fn get_input(
        _cls: &Bound<'_, PyType>,
        console: &Bound<'_, PyAny>,
        prompt: &Bound<'_, PyAny>,
        password: bool,
        stream: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = console.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item("password", password)?;
        kwargs.set_item("stream", stream)?;
        Ok(console
            .call_method("input", (prompt,), Some(&kwargs))?
            .unbind())
    }

    /// Whether `value` is one of the choices.
    fn check_choice(slf: &Bound<'_, Self>, value: &str) -> PyResult<bool> {
        let value = value.trim();
        let case_sensitive = slf.getattr("case_sensitive")?.is_truthy()?;
        for choice in slf.getattr("choices")?.try_iter()? {
            let choice = util::to_str(&choice?)?;
            if case_sensitive {
                if choice == value {
                    return Ok(true);
                }
            } else if lower(&choice) == lower(value) {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Convert and check the response, or raise `InvalidResponse`.
    fn process_response(slf: &Bound<'_, Self>, value: &str) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let value = value.trim();
        let response_type = slf.getattr("response_type")?;
        let invalid = |message: &str| -> PyResult<PyErr> {
            let message = slf.getattr(message)?;
            let error = super::glue_module(py)?
                .getattr("InvalidResponse")?
                .call1((message,))?;
            Ok(PyErr::from_value(error))
        };
        let return_value = match response_type.call1((value,)) {
            Ok(converted) => converted,
            Err(error) if error.is_instance_of::<PyValueError>(py) => {
                return Err(invalid("validate_error_message")?)
            }
            Err(error) => return Err(error),
        };
        let choices = slf.getattr("choices")?;
        if choices.is_none() {
            return Ok(return_value.unbind());
        }
        if !slf.call_method1("check_choice", (value,))?.is_truthy()? {
            return Err(invalid("illegal_choice_message")?);
        }
        if !slf.getattr("case_sensitive")?.is_truthy()? {
            for choice in choices.try_iter()? {
                let choice = choice?;
                if lower(&util::to_str(&choice)?) == lower(value) {
                    return Ok(response_type.call1((choice,))?.unbind());
                }
            }
        }
        Ok(return_value.unbind())
    }

    /// Show the error message of an invalid response.
    fn on_validate_error(
        slf: &Bound<'_, Self>,
        _value: &Bound<'_, PyAny>,
        error: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let py = slf.py();
        let kwargs = PyDict::new(py);
        kwargs.set_item("markup", true)?;
        slf.getattr("console")?
            .call_method("print", (error,), Some(&kwargs))?;
        Ok(())
    }

    /// Called before each prompt (nothing, unless overridden).
    fn pre_prompt(&self) {}

    /// Ask until the response is valid; an empty response returns `default`.
    #[pyo3(signature = (*, default=Arg::Missing, stream=None))]
    fn __call__(
        slf: &Bound<'_, Self>,
        default: Arg,
        stream: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let default = default.or_else(|| ellipsis(py)).into_bound(py);
        let invalid_response = super::glue_module(py)?.getattr("InvalidResponse")?;
        loop {
            slf.call_method0("pre_prompt")?;
            let prompt = slf.call_method1("make_prompt", (&default,))?;
            let kwargs = PyDict::new(py);
            kwargs.set_item("stream", stream.as_ref())?;
            let value = slf.call_method(
                "get_input",
                (slf.getattr("console")?, prompt, slf.getattr("password")?),
                Some(&kwargs),
            )?;
            if value.eq("")? && !is_ellipsis(&default) {
                return Ok(default.unbind());
            }
            match slf.call_method1("process_response", (&value,)) {
                Ok(result) => return Ok(result.unbind()),
                Err(error) if error.matches(py, &invalid_response)? => {
                    slf.call_method1("on_validate_error", (value, error.value(py)))?;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// Python's `str.lower()` for the comparisons upstream makes.
fn lower(value: &str) -> String {
    value.to_lowercase()
}

/// `rich.prompt.Prompt`: a prompt returning a `str`.
#[pyclass(name = "Prompt", module = "rs_rich.prompt", extends = PromptBase, subclass, frozen)]
pub(crate) struct Prompt;

#[pymethods]
impl Prompt {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Prompt> {
        PyClassInitializer::from(PromptBase).add_subclass(Prompt)
    }

    #[classattr]
    fn response_type(py: Python<'_>) -> Py<PyType> {
        py.get_type::<PyString>().unbind()
    }
}

/// `rich.prompt.IntPrompt`: a prompt returning an `int`.
#[pyclass(name = "IntPrompt", module = "rs_rich.prompt", extends = PromptBase, subclass, frozen)]
pub(crate) struct IntPrompt;

#[pymethods]
impl IntPrompt {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<IntPrompt> {
        PyClassInitializer::from(PromptBase).add_subclass(IntPrompt)
    }

    #[classattr]
    fn response_type(py: Python<'_>) -> Py<PyType> {
        py.get_type::<PyInt>().unbind()
    }

    #[classattr]
    fn validate_error_message() -> &'static str {
        "[prompt.invalid]Please enter a valid integer number"
    }
}

/// `rich.prompt.FloatPrompt`: a prompt returning a `float`.
#[pyclass(name = "FloatPrompt", module = "rs_rich.prompt", extends = PromptBase, subclass, frozen)]
pub(crate) struct FloatPrompt;

#[pymethods]
impl FloatPrompt {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<FloatPrompt> {
        PyClassInitializer::from(PromptBase).add_subclass(FloatPrompt)
    }

    #[classattr]
    fn response_type(py: Python<'_>) -> Py<PyType> {
        py.get_type::<PyFloat>().unbind()
    }

    #[classattr]
    fn validate_error_message() -> &'static str {
        "[prompt.invalid]Please enter a number"
    }
}

/// `rich.prompt.Confirm`: a yes / no prompt returning a `bool`.
#[pyclass(name = "Confirm", module = "rs_rich.prompt", extends = PromptBase, subclass, frozen)]
pub(crate) struct Confirm;

#[pymethods]
impl Confirm {
    #[new]
    #[pyo3(signature = (*_args, **_kwargs))]
    fn new(
        _args: &Bound<'_, PyTuple>,
        _kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyClassInitializer<Confirm> {
        PyClassInitializer::from(PromptBase).add_subclass(Confirm)
    }

    #[classattr]
    fn response_type(py: Python<'_>) -> Py<PyType> {
        py.get_type::<PyBool>().unbind()
    }

    #[classattr]
    fn validate_error_message() -> &'static str {
        "[prompt.invalid]Please enter Y or N"
    }

    #[classattr]
    fn choices(py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(PyList::new(py, ["y", "n"])?.into_any().unbind())
    }

    /// `(y)` or `(n)`.
    fn render_default(slf: &Bound<'_, Self>, default: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
        let py = slf.py();
        let choices = slf.getattr("choices")?;
        let yes = util::to_str(&choices.get_item(0)?)?;
        let no = util::to_str(&choices.get_item(1)?)?;
        let shown = if default.is_truthy()? { yes } else { no };
        util::new_text(py, CoreText::styled(format!("({shown})"), "prompt.default"))
    }

    /// `True` for the first choice, `False` for the second.
    fn process_response(slf: &Bound<'_, Self>, value: &str) -> PyResult<bool> {
        let py = slf.py();
        let value = lower(value.trim());
        let choices = slf.getattr("choices")?;
        let names: Vec<String> = choices
            .try_iter()?
            .map(|choice| choice.and_then(|c| util::to_str(&c)))
            .collect::<PyResult<_>>()?;
        if !names.contains(&value) {
            let message = slf.getattr("validate_error_message")?;
            let error = super::glue_module(py)?
                .getattr("InvalidResponse")?
                .call1((message,))?;
            return Err(PyErr::from_value(error));
        }
        Ok(names.first() == Some(&value))
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PromptBase>()?;
    m.add_class::<Prompt>()?;
    m.add_class::<IntPrompt>()?;
    m.add_class::<FloatPrompt>()?;
    m.add_class::<Confirm>()?;
    Ok(())
}

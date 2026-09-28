//! The component classes of `rs_rich.interact` and their items, each a
//! configuration built into a fresh `rich_interact` component per run (see
//! the parent module), and [`drive`], which runs any of them.

use std::sync::Arc;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyString, PyTuple};
use pyo3::{PyTraverseError, PyVisit};

use rich::Renderable;
use rich_interact::{
    Answers, Choice as CoreChoice, Component, Confirm as CoreConfirm, Context, Event as CoreEvent,
    Flow, Form as CoreForm, Input as CoreInput, Item as CoreItem, Key, MultiSelect as CoreMulti,
    Pager as CorePager, Preview, PreviewLayout, Select as CoreSelect, Suggestion, Value, View,
};

use super::{execute, record, Build, Mode, Record};
use crate::renderable::{self, PyRenderable};

type Shared = Arc<dyn Renderable + Send + Sync>;

fn preview_layout(name: &str) -> PyResult<PreviewLayout> {
    Ok(match name {
        "auto" => PreviewLayout::Auto,
        "right" => PreviewLayout::Right,
        "below" => PreviewLayout::Below,
        "hidden" => PreviewLayout::Hidden,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid preview {other:?}; expected auto, right, below or hidden"
            )))
        }
    })
}

fn key(name: &str) -> PyResult<Key> {
    Key::parse(name).ok_or_else(|| PyValueError::new_err(format!("unknown key name {name:?}")))
}

fn one_char(value: &str, what: &str) -> PyResult<char> {
    let mut chars = value.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Ok(c),
        _ => Err(PyValueError::new_err(format!(
            "{what} must be one character, got {value:?}"
        ))),
    }
}

/// A renderable argument, rendered when the component renders.
fn shared(value: &Bound<'_, PyAny>) -> Shared {
    PyRenderable::shared(value.clone().unbind(), None)
}

// ---------------------------------------------------------------------------
// Items

/// `Action(id, label, key)`: something done to an item, bound to a key; the
/// key picks the item and the outcome's `action` is `id`.
#[pyclass(
    name = "Action",
    module = "rs_rich.interact",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Action {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    key: String,
    parsed: Key,
}

#[pymethods]
impl Action {
    #[new]
    fn new(id: String, label: String, key: String) -> PyResult<Self> {
        let parsed = self::key(&key)?;
        Ok(Action {
            id,
            label,
            key,
            parsed,
        })
    }

    fn __repr__(&self) -> String {
        format!("Action({:?}, {:?}, {:?})", self.id, self.label, self.key)
    }
}

/// `Item(value, label=None, *, description=None, preview=None,
/// metadata=None, keywords=(), actions=())`: one choosable thing. `value`
/// is any Python object (returned when chosen); `label` defaults to
/// `str(value)`. `preview` is console markup (a `str`) or any renderable.
#[pyclass(name = "Item", module = "rs_rich.interact", frozen)]
pub(crate) struct Item {
    value: Py<PyAny>,
    #[pyo3(get)]
    label: String,
    #[pyo3(get)]
    description: Option<String>,
    #[pyo3(get)]
    metadata: Vec<(String, String)>,
    preview: Option<Py<PyAny>>,
    #[pyo3(get)]
    keywords: Vec<String>,
    actions: Vec<Action>,
}

#[pymethods]
impl Item {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.value)?;
        if let Some(preview) = &self.preview {
            visit.call(preview)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (value, label=None, *, description=None, preview=None, metadata=None, keywords=Vec::new(), actions=Vec::new()))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        value: &Bound<'_, PyAny>,
        label: Option<String>,
        description: Option<String>,
        preview: Option<&Bound<'_, PyAny>>,
        metadata: Option<&Bound<'_, PyAny>>,
        keywords: Vec<String>,
        actions: Vec<PyRef<'_, Action>>,
    ) -> PyResult<Self> {
        let label = match label {
            Some(label) => label,
            None => value.str()?.to_cow()?.into_owned(),
        };
        let metadata = match metadata {
            Some(metadata) if !metadata.is_none() => crate::ext::common::pairs(metadata)?
                .into_iter()
                .map(|(key, value)| Ok((key, value.str()?.to_cow()?.into_owned())))
                .collect::<PyResult<Vec<_>>>()?,
            _ => Vec::new(),
        };
        Ok(Item {
            value: value.clone().unbind(),
            label,
            description,
            metadata,
            preview: preview.filter(|p| !p.is_none()).map(|p| p.clone().unbind()),
            keywords,
            actions: actions.iter().map(|action| (**action).clone()).collect(),
        })
    }

    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.value.clone_ref(py)
    }

    #[getter]
    fn preview(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.preview.as_ref().map(|p| p.clone_ref(py))
    }

    #[getter]
    fn actions(&self) -> Vec<Action> {
        self.actions.clone()
    }

    fn __repr__(&self, py: Python<'_>) -> PyResult<String> {
        Ok(format!(
            "Item({}, {:?})",
            self.value.bind(py).repr()?,
            self.label
        ))
    }
}

impl Item {
    /// The Rust item, its value the index `index`.
    fn core(&self, py: Python<'_>, index: usize) -> PyResult<CoreItem<usize>> {
        let mut item = CoreItem::new(index, self.label.clone());
        if let Some(description) = &self.description {
            item = item.description(description.clone());
        }
        for (key, value) in &self.metadata {
            item = item.meta(key.clone(), value.clone());
        }
        for keyword in &self.keywords {
            item = item.keyword(keyword.clone());
        }
        for action in &self.actions {
            item = item.action(rich_interact::Action::new(
                action.id.clone(),
                action.label.clone(),
                action.parsed,
            ));
        }
        if let Some(preview) = &self.preview {
            let preview = preview.bind(py);
            item = item.preview(match preview.cast::<PyString>() {
                Ok(markup) => Preview::Markup(markup.to_cow()?.into_owned()),
                Err(_) => Preview::Renderable(shared(preview)),
            });
        }
        Ok(item)
    }
}

/// The items of a picker: `Item`s, or any objects (labelled with `str`).
fn items(py: Python<'_>, values: &Bound<'_, PyAny>) -> PyResult<Vec<Py<Item>>> {
    if values.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "items must be an iterable of items, not a str",
        ));
    }
    values
        .try_iter()?
        .map(|value| {
            let value = value?;
            if let Ok(item) = value.cast::<Item>() {
                return Ok(item.clone().unbind());
            }
            let label = value.str()?.to_cow()?.into_owned();
            Py::new(
                py,
                Item {
                    value: value.unbind(),
                    label,
                    description: None,
                    metadata: Vec::new(),
                    preview: None,
                    keywords: Vec::new(),
                    actions: Vec::new(),
                },
            )
        })
        .collect()
}

fn core_items(py: Python<'_>, items: &[Py<Item>]) -> PyResult<Vec<CoreItem<usize>>> {
    items
        .iter()
        .enumerate()
        .map(|(index, item)| item.get().core(py, index))
        .collect()
}

fn item_values<'py>(py: Python<'py>, items: &[Py<Item>]) -> PyResult<Bound<'py, PyList>> {
    PyList::new(py, items.iter().map(|item| item.get().value.clone_ref(py)))
}

// ---------------------------------------------------------------------------
// Select and MultiSelect

/// `Select(prompt, items, *, default=None, query="", height=10,
/// preview="auto", preview_height=10)`: a fuzzy single choice. Typing
/// filters, the arrows move, Enter returns the focused item's value.
/// `default` is an item index: focused first, and the answer without a
/// terminal when the fallback is `"default"`.
#[pyclass(name = "Select", module = "rs_rich.interact", frozen)]
pub(crate) struct Select {
    #[pyo3(get)]
    prompt: String,
    items: Vec<Py<Item>>,
    #[pyo3(get)]
    default: Option<usize>,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    height: usize,
    preview: PreviewLayout,
    #[pyo3(get)]
    preview_height: usize,
}

struct SelectBuild {
    prompt: String,
    items: Vec<CoreItem<usize>>,
    default: Option<usize>,
    query: String,
    height: usize,
    preview: PreviewLayout,
    preview_height: usize,
}

impl Build for SelectBuild {
    type C = CoreSelect<usize>;

    fn build(self) -> CoreSelect<usize> {
        let mut select = CoreSelect::new(self.prompt, self.items)
            .height(self.height)
            .preview(self.preview)
            .preview_height(self.preview_height);
        if !self.query.is_empty() {
            select = select.query(self.query);
        }
        if let Some(default) = self.default {
            select = select.default(default);
        }
        select
    }

    fn action(component: &CoreSelect<usize>) -> Option<String> {
        component.action().map(str::to_string)
    }
}

#[pymethods]
impl Select {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (prompt, items, *, default=None, query=String::new(), height=10, preview="auto", preview_height=10))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        prompt: String,
        items: &Bound<'_, PyAny>,
        default: Option<usize>,
        query: String,
        height: usize,
        preview: &str,
        preview_height: usize,
    ) -> PyResult<Self> {
        let items = self::items(py, items)?;
        if let Some(index) = default {
            if index >= items.len() {
                return Err(PyValueError::new_err(format!(
                    "default {index} is out of range for {} items",
                    items.len()
                )));
            }
        }
        Ok(Select {
            prompt,
            items,
            default,
            query,
            height,
            preview: preview_layout(preview)?,
            preview_height,
        })
    }

    #[getter]
    fn items(&self, py: Python<'_>) -> Vec<Py<Item>> {
        self.items.iter().map(|item| item.clone_ref(py)).collect()
    }

    /// The items' values, in order.
    #[getter]
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        item_values(py, &self.items)
    }

    /// `ask(**options)`: `rs_rich.interact.ask(self, **options)`.
    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    /// `headless(script=None, *, width=80, height=24)`:
    /// `rs_rich.interact.headless(self, ...)`.
    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl Select {
    fn prepare(&self, py: Python<'_>) -> PyResult<SelectBuild> {
        Ok(SelectBuild {
            prompt: self.prompt.clone(),
            items: core_items(py, &self.items)?,
            default: self.default,
            query: self.query.clone(),
            height: self.height,
            preview: self.preview,
            preview_height: self.preview_height,
        })
    }
}

/// `MultiSelect(prompt, items, *, marked=(), query="", height=10,
/// preview="auto")`: a fuzzy multiple choice. Tab marks (Shift+Tab marks
/// and moves up, Ctrl+A marks every match); Enter returns the marked
/// values in list order, or the focused one when none is marked. `marked`
/// (item indices) start marked, and are the answer without a terminal
/// when the fallback is `"default"`.
#[pyclass(name = "MultiSelect", module = "rs_rich.interact", frozen)]
pub(crate) struct MultiSelect {
    #[pyo3(get)]
    prompt: String,
    items: Vec<Py<Item>>,
    #[pyo3(get)]
    marked: Vec<usize>,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    height: usize,
    preview: PreviewLayout,
}

struct MultiBuild {
    prompt: String,
    items: Vec<CoreItem<usize>>,
    marked: Vec<usize>,
    query: String,
    height: usize,
    preview: PreviewLayout,
}

impl Build for MultiBuild {
    type C = CoreMulti<usize>;

    fn build(self) -> CoreMulti<usize> {
        let mut select = CoreMulti::new(self.prompt, self.items)
            .height(self.height)
            .preview(self.preview)
            .marked(self.marked);
        if !self.query.is_empty() {
            select = select.query(self.query);
        }
        select
    }

    fn action(component: &CoreMulti<usize>) -> Option<String> {
        component.action().map(str::to_string)
    }
}

#[pymethods]
impl MultiSelect {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (prompt, items, *, marked=Vec::new(), query=String::new(), height=10, preview="auto"))]
    fn new(
        py: Python<'_>,
        prompt: String,
        items: &Bound<'_, PyAny>,
        marked: Vec<usize>,
        query: String,
        height: usize,
        preview: &str,
    ) -> PyResult<Self> {
        let items = self::items(py, items)?;
        if let Some(index) = marked.iter().find(|&&index| index >= items.len()) {
            return Err(PyValueError::new_err(format!(
                "marked index {index} is out of range for {} items",
                items.len()
            )));
        }
        Ok(MultiSelect {
            prompt,
            items,
            marked,
            query,
            height,
            preview: preview_layout(preview)?,
        })
    }

    #[getter]
    fn items(&self, py: Python<'_>) -> Vec<Py<Item>> {
        self.items.iter().map(|item| item.clone_ref(py)).collect()
    }

    #[getter]
    fn values<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyList>> {
        item_values(py, &self.items)
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl MultiSelect {
    fn prepare(&self, py: Python<'_>) -> PyResult<MultiBuild> {
        Ok(MultiBuild {
            prompt: self.prompt.clone(),
            items: core_items(py, &self.items)?,
            marked: self.marked.clone(),
            query: self.query.clone(),
            height: self.height,
            preview: self.preview,
        })
    }
}

// ---------------------------------------------------------------------------
// Input

/// A validator's verdict on `text`: `None` or `True` passes, `False` fails,
/// a `str` is the message, and a raised `ValueError` is its message. Any
/// other exception is raised from the run.
fn validate(validator: &Py<PyAny>, text: &str) -> Result<(), String> {
    Python::attach(|py| match validator.bind(py).call1((text,)) {
        Ok(verdict) => {
            if verdict.is_none() {
                return Ok(());
            }
            if let Ok(message) = verdict.cast::<PyString>() {
                return Err(message.to_string());
            }
            match verdict.is_truthy() {
                Ok(true) => Ok(()),
                Ok(false) => Err("invalid value".to_string()),
                Err(error) => {
                    let message = error.to_string();
                    renderable::report_error(py, error);
                    Err(message)
                }
            }
        }
        Err(error) if error.is_instance_of::<PyValueError>(py) => Err(error
            .value(py)
            .str()
            .map_or_else(|_| "invalid value".to_string(), |s| s.to_string())),
        Err(error) => {
            let message = error.to_string();
            renderable::report_error(py, error);
            Err(message)
        }
    })
}

/// `Input(prompt, *, value="", placeholder=None, default=None, help=None,
/// password=False, mask=None, validate=None, history=(), suggestions=(),
/// limit=5)`: one line of text with shell-like editing. `password=True`
/// masks what is typed with `•` (`mask` picks another character).
/// `validate(text)` returns `None`/`True` to accept, or a message (or
/// `False`, or raises `ValueError`) to refuse. `suggestions` are `str`s or
/// `(value, description)` pairs, filtered as you type; Tab accepts one.
#[pyclass(name = "Input", module = "rs_rich.interact", frozen)]
pub(crate) struct Input {
    #[pyo3(get)]
    prompt: String,
    #[pyo3(get)]
    value: String,
    #[pyo3(get)]
    placeholder: Option<String>,
    #[pyo3(get)]
    default: Option<String>,
    #[pyo3(get)]
    help: Option<String>,
    mask: Option<char>,
    validator: Option<Py<PyAny>>,
    #[pyo3(get)]
    history: Vec<String>,
    suggestions: Vec<(String, Option<String>)>,
    #[pyo3(get)]
    limit: usize,
}

struct InputBuild {
    prompt: String,
    value: String,
    placeholder: Option<String>,
    default: Option<String>,
    help: Option<String>,
    mask: Option<char>,
    validator: Option<Py<PyAny>>,
    history: Vec<String>,
    suggestions: Vec<(String, Option<String>)>,
    limit: usize,
}

impl InputBuild {
    fn input(self) -> CoreInput {
        let mut input = CoreInput::new(self.prompt).limit(self.limit);
        if let Some(mask) = self.mask {
            input = input.mask(mask);
        }
        if !self.value.is_empty() {
            input = input.value(self.value);
        }
        if let Some(placeholder) = self.placeholder {
            input = input.placeholder(placeholder);
        }
        if let Some(default) = self.default {
            input = input.default(default);
        }
        if let Some(help) = self.help {
            input = input.help(help);
        }
        if let Some(validator) = self.validator {
            input = input.validate(move |text| validate(&validator, text));
        }
        if !self.history.is_empty() {
            input = input.history(self.history);
        }
        if !self.suggestions.is_empty() {
            input = input.suggestions(self.suggestions.into_iter().map(|(value, description)| {
                let suggestion = Suggestion::new(value);
                match description {
                    Some(description) => suggestion.description(description),
                    None => suggestion,
                }
            }));
        }
        input
    }
}

impl Build for InputBuild {
    type C = CoreInput;

    fn build(self) -> CoreInput {
        self.input()
    }
}

fn suggestions(value: &Bound<'_, PyAny>) -> PyResult<Vec<(String, Option<String>)>> {
    if value.is_instance_of::<PyString>() {
        return Err(PyTypeError::new_err(
            "suggestions must be an iterable, not a str",
        ));
    }
    value
        .try_iter()?
        .map(|entry| {
            let entry = entry?;
            if let Ok(text) = entry.cast::<PyString>() {
                return Ok((text.to_cow()?.into_owned(), None));
            }
            let (value, description): (String, Option<String>) = entry.extract()?;
            Ok((value, description))
        })
        .collect()
}

#[pymethods]
impl Input {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        if let Some(validator) = &self.validator {
            visit.call(validator)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (
        prompt, *, value=String::new(), placeholder=None, default=None, help=None, password=false,
        mask=None, validate=None, history=Vec::new(), suggestions=None, limit=5
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        prompt: String,
        value: String,
        placeholder: Option<String>,
        default: Option<String>,
        help: Option<String>,
        password: bool,
        mask: Option<&str>,
        validate: Option<&Bound<'_, PyAny>>,
        history: Vec<String>,
        suggestions: Option<&Bound<'_, PyAny>>,
        limit: usize,
    ) -> PyResult<Self> {
        let mask = match mask {
            Some(mask) => Some(one_char(mask, "mask")?),
            None if password => Some('•'),
            None => None,
        };
        let validator = match validate.filter(|v| !v.is_none()) {
            Some(validator) if !validator.is_callable() => {
                return Err(PyTypeError::new_err("validate must be callable"))
            }
            other => other.map(|v| v.clone().unbind()),
        };
        Ok(Input {
            prompt,
            value,
            placeholder,
            default,
            help,
            mask,
            validator,
            history,
            suggestions: match suggestions.filter(|s| !s.is_none()) {
                Some(suggestions) => self::suggestions(suggestions)?,
                None => Vec::new(),
            },
            limit,
        })
    }

    /// Whether what is typed is masked.
    #[getter]
    fn password(&self) -> bool {
        self.mask.is_some()
    }

    #[getter]
    fn mask(&self) -> Option<String> {
        self.mask.map(String::from)
    }

    #[getter]
    fn suggestions(&self) -> Vec<(String, Option<String>)> {
        self.suggestions.clone()
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl Input {
    fn prepare(&self, py: Python<'_>) -> InputBuild {
        InputBuild {
            prompt: self.prompt.clone(),
            value: self.value.clone(),
            placeholder: self.placeholder.clone(),
            default: self.default.clone(),
            help: self.help.clone(),
            mask: self.mask,
            validator: self.validator.as_ref().map(|v| v.clone_ref(py)),
            history: self.history.clone(),
            suggestions: self.suggestions.clone(),
            limit: self.limit,
        }
    }
}

// ---------------------------------------------------------------------------
// Confirm

/// `Choice(id, label, key)`: one answer of a `Confirm`; `key` (one
/// character) picks it directly.
#[pyclass(
    name = "Choice",
    module = "rs_rich.interact",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Choice {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    label: String,
    key: char,
}

#[pymethods]
impl Choice {
    #[new]
    fn new(id: String, label: String, key: &str) -> PyResult<Self> {
        Ok(Choice {
            id,
            label,
            key: one_char(key, "a choice's key")?,
        })
    }

    #[getter]
    fn key(&self) -> String {
        self.key.to_string()
    }

    fn __repr__(&self) -> String {
        format!("Choice({:?}, {:?}, {:?})", self.id, self.label, self.key)
    }
}

/// `Confirm(title="Are you sure?", *, body=None, warnings=(), choices=None,
/// default=None, body_height=None)`: a question with an optional body
/// (a renderable or a list of them, scrollable), warnings under it, and
/// choices (default: `Choice("yes", "Yes", "y")` and `Choice("no", "No",
/// "n")`). The answer is the chosen choice's `id`. `default` (an id) is
/// focused first, and the answer without a terminal when the fallback is
/// `"default"`.
#[pyclass(name = "Confirm", module = "rs_rich.interact", frozen)]
pub(crate) struct Confirm {
    #[pyo3(get)]
    title: String,
    body: Vec<Py<PyAny>>,
    #[pyo3(get)]
    warnings: Vec<String>,
    choices: Option<Vec<Choice>>,
    #[pyo3(get)]
    default: Option<String>,
    #[pyo3(get)]
    body_height: Option<usize>,
}

struct ConfirmBuild {
    title: String,
    body: Vec<Shared>,
    warnings: Vec<String>,
    choices: Option<Vec<CoreChoice>>,
    default: Option<String>,
    body_height: Option<usize>,
}

/// An `Arc`'d renderable as a renderable.
struct ArcRenderable(Shared);

impl Renderable for ArcRenderable {
    fn rich_render(
        &self,
        console: &rich::Console,
        options: &rich::console::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        self.0.rich_render(console, options)
    }

    fn measure(
        &self,
        console: &rich::Console,
        options: &rich::console::ConsoleOptions,
    ) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

impl Build for ConfirmBuild {
    type C = CoreConfirm;

    fn build(self) -> CoreConfirm {
        let mut confirm = CoreConfirm::new(self.title);
        for renderable in self.body {
            confirm = confirm.body(ArcRenderable(renderable));
        }
        for warning in self.warnings {
            confirm = confirm.warning(warning);
        }
        if let Some(choices) = self.choices {
            confirm = confirm.choices(choices);
        }
        if let Some(default) = self.default {
            confirm = confirm.default(&default);
        }
        if let Some(rows) = self.body_height {
            confirm = confirm.body_height(rows);
        }
        confirm
    }
}

#[pymethods]
impl Confirm {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for part in &self.body {
            visit.call(part)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (title=String::from("Are you sure?"), *, body=None, warnings=Vec::new(), choices=None, default=None, body_height=None))]
    fn new(
        title: String,
        body: Option<&Bound<'_, PyAny>>,
        warnings: Vec<String>,
        choices: Option<&Bound<'_, PyAny>>,
        default: Option<String>,
        body_height: Option<usize>,
    ) -> PyResult<Self> {
        let body = match body.filter(|b| !b.is_none()) {
            None => Vec::new(),
            Some(body) if body.is_instance_of::<PyList>() || body.is_instance_of::<PyTuple>() => {
                body.try_iter()?
                    .map(|item| Ok(item?.unbind()))
                    .collect::<PyResult<Vec<_>>>()?
            }
            Some(body) => vec![body.clone().unbind()],
        };
        let choices = match choices.filter(|c| !c.is_none()) {
            None => None,
            Some(choices) => Some(
                choices
                    .try_iter()?
                    .map(|choice| {
                        let choice = choice?;
                        if let Ok(choice) = choice.cast::<Choice>() {
                            return Ok(choice.get().clone());
                        }
                        let (id, label, key): (String, String, String) = choice.extract()?;
                        Choice::new(id, label, &key)
                    })
                    .collect::<PyResult<Vec<_>>>()?,
            ),
        };
        if let Some(choices) = &choices {
            if choices.is_empty() {
                return Err(PyValueError::new_err("a Confirm needs at least one choice"));
            }
        }
        if let Some(default) = &default {
            let known = match &choices {
                Some(choices) => choices.iter().any(|choice| &choice.id == default),
                None => default == "yes" || default == "no",
            };
            if !known {
                return Err(PyValueError::new_err(format!(
                    "default {default:?} is not one of the choices' ids"
                )));
            }
        }
        Ok(Confirm {
            title,
            body,
            warnings,
            choices,
            default,
            body_height,
        })
    }

    #[getter]
    fn body(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        self.body.iter().map(|b| b.clone_ref(py)).collect()
    }

    /// The choices (the yes/no pair when none were given).
    #[getter]
    fn choices(&self) -> Vec<Choice> {
        self.choices.clone().unwrap_or_else(|| {
            vec![
                Choice {
                    id: "yes".into(),
                    label: "Yes".into(),
                    key: 'y',
                },
                Choice {
                    id: "no".into(),
                    label: "No".into(),
                    key: 'n',
                },
            ]
        })
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

impl Confirm {
    fn prepare(&self, py: Python<'_>) -> ConfirmBuild {
        ConfirmBuild {
            title: self.title.clone(),
            body: self.body.iter().map(|b| shared(b.bind(py))).collect(),
            warnings: self.warnings.clone(),
            choices: self.choices.as_ref().map(|choices| {
                choices
                    .iter()
                    .map(|c| CoreChoice::new(c.id.clone(), c.label.clone(), c.key))
                    .collect()
            }),
            default: self.default.clone(),
            body_height: self.body_height,
        }
    }
}

// ---------------------------------------------------------------------------
// Form

enum Field {
    Input(Py<Input>),
    Choice(String, Vec<String>),
    Toggle(String, bool),
}

enum FieldBuild {
    Input(InputBuild),
    Choice(String, Vec<String>),
    Toggle(String, bool),
}

/// `Form(title, *, theme=None)`: named fields answered together. Add fields
/// with `text`, `masked`, `input`, `choice` and `toggle` (each returns the
/// form, so calls chain). Tab and the arrows move between fields; Enter on
/// the last submits, after every field's validation passes. The answer is
/// a `dict` of field name to `str` (text and choices) or `bool` (toggles).
#[pyclass(name = "Form", module = "rs_rich.interact", frozen)]
pub(crate) struct Form {
    #[pyo3(get)]
    title: String,
    fields: std::sync::Mutex<Vec<(String, Field)>>,
}

struct FormBuild {
    title: String,
    fields: Vec<(String, FieldBuild)>,
}

impl Build for FormBuild {
    type C = CoreForm;

    fn build(self) -> CoreForm {
        let mut form = CoreForm::new(self.title);
        for (name, field) in self.fields {
            form = match field {
                FieldBuild::Input(input) => form.input(name, input.input()),
                FieldBuild::Choice(label, options) => form.choice(name, label, options),
                FieldBuild::Toggle(label, on) => form.toggle(name, label, on),
            };
        }
        form
    }
}

impl Form {
    fn push<'py>(slf: &Bound<'py, Self>, name: String, field: Field) -> PyResult<Bound<'py, Self>> {
        let this = slf.get();
        let mut fields = this.fields.lock().unwrap_or_else(|p| p.into_inner());
        if fields.iter().any(|(existing, _)| *existing == name) {
            return Err(PyValueError::new_err(format!(
                "the form already has a field named {name:?}"
            )));
        }
        fields.push((name, field));
        Ok(slf.clone())
    }

    fn prepare(&self, py: Python<'_>) -> FormBuild {
        let fields = self.fields.lock().unwrap_or_else(|p| p.into_inner());
        FormBuild {
            title: self.title.clone(),
            fields: fields
                .iter()
                .map(|(name, field)| {
                    let field = match field {
                        Field::Input(input) => FieldBuild::Input(input.get().prepare(py)),
                        Field::Choice(label, options) => {
                            FieldBuild::Choice(label.clone(), options.clone())
                        }
                        Field::Toggle(label, on) => FieldBuild::Toggle(label.clone(), *on),
                    };
                    (name.clone(), field)
                })
                .collect(),
        }
    }
}

#[pymethods]
impl Form {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        // Never block in the collector: a form being changed is alive.
        if let Ok(fields) = self.fields.try_lock() {
            for (_, field) in fields.iter() {
                if let Field::Input(input) = field {
                    visit.call(input)?;
                }
            }
        }
        Ok(())
    }

    #[new]
    fn new(title: String) -> Self {
        Form {
            title,
            fields: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// A text field; the keywords are `Input`'s.
    #[pyo3(signature = (name, label, *, value=String::new(), placeholder=None, default=None, help=None, validate=None))]
    #[allow(clippy::too_many_arguments)]
    fn text<'py>(
        slf: &Bound<'py, Self>,
        name: String,
        label: String,
        value: String,
        placeholder: Option<String>,
        default: Option<String>,
        help: Option<String>,
        validate: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Self>> {
        let input = Input::new(
            label,
            value,
            placeholder,
            default,
            help,
            false,
            None,
            validate,
            Vec::new(),
            None,
            5,
        )?;
        Form::push(slf, name, Field::Input(Py::new(slf.py(), input)?))
    }

    /// A masked text field, for passwords and tokens.
    #[pyo3(signature = (name, label, *, default=None, validate=None))]
    fn masked<'py>(
        slf: &Bound<'py, Self>,
        name: String,
        label: String,
        default: Option<String>,
        validate: Option<&Bound<'py, PyAny>>,
    ) -> PyResult<Bound<'py, Self>> {
        let input = Input::new(
            label,
            String::new(),
            None,
            default,
            None,
            true,
            None,
            validate,
            Vec::new(),
            None,
            5,
        )?;
        Form::push(slf, name, Field::Input(Py::new(slf.py(), input)?))
    }

    /// A text field from a configured `Input`; its prompt is the label.
    fn input<'py>(
        slf: &Bound<'py, Self>,
        name: String,
        input: Bound<'py, Input>,
    ) -> PyResult<Bound<'py, Self>> {
        Form::push(slf, name, Field::Input(input.unbind()))
    }

    /// One of `options`, changed with Left, Right or Space.
    fn choice<'py>(
        slf: &Bound<'py, Self>,
        name: String,
        label: String,
        options: Vec<String>,
    ) -> PyResult<Bound<'py, Self>> {
        if options.is_empty() {
            return Err(PyValueError::new_err("a choice needs at least one option"));
        }
        Form::push(slf, name, Field::Choice(label, options))
    }

    /// Yes or no, changed with Space, Left, Right, `y` and `n`.
    #[pyo3(signature = (name, label, on=false))]
    fn toggle<'py>(
        slf: &Bound<'py, Self>,
        name: String,
        label: String,
        on: bool,
    ) -> PyResult<Bound<'py, Self>> {
        Form::push(slf, name, Field::Toggle(label, on))
    }

    /// The field names, in order.
    #[getter]
    fn fields(&self) -> Vec<String> {
        let fields = self.fields.lock().unwrap_or_else(|p| p.into_inner());
        fields.iter().map(|(name, _)| name.clone()).collect()
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

fn answers(py: Python<'_>, answers: Answers) -> PyResult<Py<PyAny>> {
    let dict = PyDict::new(py);
    for (name, value) in answers.0 {
        match value {
            Value::Text(text) => dict.set_item(name, text)?,
            Value::Flag(flag) => dict.set_item(name, flag)?,
        }
    }
    Ok(dict.into_any().unbind())
}

// ---------------------------------------------------------------------------
// Pager

/// `Pager(renderable, *, search=None)`: page any renderable, rendered at
/// the terminal's width. The arrows, Space and PageUp/PageDown scroll; `/`
/// searches, `n` and `N` move between matches; `q` or Escape closes.
/// Without a terminal the content is written out. The answer is `None`.
#[pyclass(name = "Pager", module = "rs_rich.interact", frozen)]
pub(crate) struct Pager {
    renderable: Py<PyAny>,
    #[pyo3(get)]
    search: Option<String>,
}

struct PagerBuild {
    renderable: Shared,
    search: Option<String>,
}

impl Build for PagerBuild {
    type C = CorePager;

    fn build(self) -> CorePager {
        let pager = CorePager::new(ArcRenderable(self.renderable));
        match self.search {
            Some(query) => pager.search(query),
            None => pager,
        }
    }
}

#[pymethods]
impl Pager {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.renderable)?;
        Ok(())
    }

    #[new]
    #[pyo3(signature = (renderable, *, search=None))]
    fn new(renderable: &Bound<'_, PyAny>, search: Option<String>) -> Self {
        Pager {
            renderable: renderable.clone().unbind(),
            search,
        }
    }

    #[getter]
    fn renderable(&self, py: Python<'_>) -> Py<PyAny> {
        self.renderable.clone_ref(py)
    }

    #[pyo3(signature = (**options))]
    fn ask(slf: &Bound<'_, Self>, options: Option<&Bound<'_, PyDict>>) -> PyResult<Py<PyAny>> {
        super::interact_ask(slf.py(), slf.as_any(), options)
    }

    #[pyo3(signature = (script=None, *, width=80, height=24))]
    fn headless(
        slf: &Bound<'_, Self>,
        script: Option<&Bound<'_, PyAny>>,
        width: u16,
        height: u16,
    ) -> PyResult<Record> {
        super::interact_headless(slf.py(), slf.as_any(), script, width, height)
    }
}

// ---------------------------------------------------------------------------
// Components written in Python

/// An event given to a Python component's `handle`: `kind` is `"key"`,
/// `"paste"`, `"resize"`, `"mouse"`, `"tick"` or `"returned"`.
#[pyclass(name = "Event", module = "rs_rich.interact", frozen)]
pub(crate) struct Event {
    #[pyo3(get)]
    kind: &'static str,
    /// The key's name for a key event (`"enter"`, `"down"`, `"ctrl+c"`,
    /// `"a"`, `"space"`).
    #[pyo3(get)]
    key: Option<String>,
    /// The pasted text.
    #[pyo3(get)]
    text: Option<String>,
    /// The new size, for a resize.
    #[pyo3(get)]
    columns: Option<u16>,
    #[pyo3(get)]
    rows: Option<u16>,
    /// A hand-off's exit code, for `returned`.
    #[pyo3(get)]
    code: Option<i32>,
}

#[pymethods]
impl Event {
    fn __repr__(&self) -> String {
        match (&self.key, &self.text) {
            (Some(key), _) => format!("Event(key={key:?})"),
            (_, Some(text)) => format!("Event(paste={text:?})"),
            _ => format!("Event({})", self.kind),
        }
    }
}

fn event(value: &CoreEvent) -> Event {
    let mut event = Event {
        kind: "tick",
        key: None,
        text: None,
        columns: None,
        rows: None,
        code: None,
    };
    match value {
        CoreEvent::Key(key) => {
            event.kind = "key";
            event.key = Some(key.to_string());
        }
        CoreEvent::Mouse(_) => event.kind = "mouse",
        CoreEvent::Paste(text) => {
            event.kind = "paste";
            event.text = Some(text.clone());
        }
        CoreEvent::Resize { columns, rows } => {
            event.kind = "resize";
            event.columns = Some(*columns);
            event.rows = Some(*rows);
        }
        CoreEvent::Tick => {}
        CoreEvent::Returned(code) => {
            event.kind = "returned";
            event.code = *code;
        }
    }
    event
}

/// `Done(value)`: what a Python component's `handle` returns to finish.
#[pyclass(name = "Done", module = "rs_rich.interact", frozen)]
pub(crate) struct Done {
    value: Py<PyAny>,
}

#[pymethods]
impl Done {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        visit.call(&self.value)?;
        Ok(())
    }

    #[new]
    #[pyo3(signature = (value=None))]
    fn new(py: Python<'_>, value: Option<Py<PyAny>>) -> Self {
        Done {
            value: value.unwrap_or_else(|| py.None()),
        }
    }

    #[getter]
    fn value(&self, py: Python<'_>) -> Py<PyAny> {
        self.value.clone_ref(py)
    }
}

/// `Cancel()`: what a Python component's `handle` returns to cancel.
#[pyclass(name = "Cancel", module = "rs_rich.interact", frozen)]
pub(crate) struct Cancel;

#[pymethods]
impl Cancel {
    #[new]
    fn new() -> Self {
        Cancel
    }
}

/// A Python object as a component: `render(width, height)` returns a
/// renderable; `handle(event)` returns `None` to continue, `Done(value)`
/// or `Cancel()`; an optional `default_value()` is the answer without a
/// terminal. An exception in either ends the run and is raised from it.
struct PyComponent {
    object: Py<PyAny>,
    failed: bool,
}

impl Build for PyComponent {
    type C = PyComponent;

    fn build(self) -> PyComponent {
        self
    }
}

impl Component for PyComponent {
    type Output = Py<PyAny>;

    fn handle(&mut self, value: &CoreEvent, _: &Context<'_>) -> Flow<Py<PyAny>> {
        // An exception from `render` (kept for the scope) ends it too.
        if self.failed || renderable::has_pending() {
            return Flow::Cancel;
        }
        Python::attach(|py| {
            let flow = self
                .object
                .bind(py)
                .call_method1("handle", (event(value),))
                .and_then(|result| {
                    if result.is_none() {
                        Ok(Flow::Continue)
                    } else if let Ok(done) = result.cast::<Done>() {
                        Ok(Flow::Done(done.get().value.clone_ref(py)))
                    } else if result.is_instance_of::<Cancel>()
                        || result.is(py.get_type::<Cancel>().as_any())
                    {
                        Ok(Flow::Cancel)
                    } else {
                        Err(PyTypeError::new_err(format!(
                            "handle() must return None, Done(value) or Cancel(), got {}",
                            result.repr()?
                        )))
                    }
                });
            flow.unwrap_or_else(|error| {
                self.failed = true;
                renderable::report_error(py, error);
                Flow::Cancel
            })
        })
    }

    fn render(&self, context: &Context<'_>) -> View {
        if self.failed || renderable::has_pending() {
            return View::default();
        }
        let rendered = Python::attach(|py| {
            self.object
                .bind(py)
                .call_method1("render", (context.width, context.height))
                .map(|renderable| renderable.unbind())
                .map_err(|error| renderable::report_error(py, error))
                .ok()
        });
        match rendered {
            Some(renderable) => View::new(context.lines(&PyRenderable::new(renderable))),
            None => View::default(),
        }
    }

    fn default_value(&self) -> Option<Py<PyAny>> {
        Python::attach(|py| {
            let object = self.object.bind(py);
            let method = object.getattr_opt("default_value").ok()??;
            match method.call0() {
                Ok(value) if value.is_none() => None,
                Ok(value) => Some(value.unbind()),
                Err(error) => {
                    renderable::report_error(py, error);
                    None
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Dispatch

/// Run `component` (one of this module's classes, or a Python component)
/// in `mode`.
pub(crate) fn drive(py: Python<'_>, component: &Bound<'_, PyAny>, mode: Mode) -> PyResult<Record> {
    if let Ok(select) = component.cast::<Select>() {
        let select = select.get();
        let ran = execute(py, select.prepare(py)?, mode)?;
        return record(py, ran, |index| {
            Ok(select.items[index].get().value.clone_ref(py))
        });
    }
    if let Ok(multi) = component.cast::<MultiSelect>() {
        let multi = multi.get();
        let ran = execute(py, multi.prepare(py)?, mode)?;
        return record(py, ran, |indices| {
            let values = indices
                .into_iter()
                .map(|index| multi.items[index].get().value.clone_ref(py));
            Ok(PyList::new(py, values)?.into_any().unbind())
        });
    }
    if let Ok(input) = component.cast::<Input>() {
        let ran = execute(py, input.get().prepare(py), mode)?;
        return record(py, ran, |text| {
            Ok(PyString::new(py, &text).into_any().unbind())
        });
    }
    if let Ok(confirm) = component.cast::<Confirm>() {
        let ran = execute(py, confirm.get().prepare(py), mode)?;
        return record(py, ran, |id| Ok(PyString::new(py, &id).into_any().unbind()));
    }
    if let Ok(form) = component.cast::<Form>() {
        let ran = execute(py, form.get().prepare(py), mode)?;
        return record(py, ran, |value| answers(py, value));
    }
    if let Ok(pager) = component.cast::<Pager>() {
        let pager = pager.get();
        let build = PagerBuild {
            renderable: shared(pager.renderable.bind(py)),
            search: pager.search.clone(),
        };
        let ran = execute(py, build, mode)?;
        return record(py, ran, |()| Ok(py.None()));
    }
    if component.hasattr("handle")? && component.hasattr("render")? {
        let build = PyComponent {
            object: component.clone().unbind(),
            failed: false,
        };
        let ran = execute(py, build, mode)?;
        return record(py, ran, Ok);
    }
    Err(PyTypeError::new_err(format!(
        "expected an interactive component (Select, MultiSelect, Input, Confirm, Form, Pager, \
         or an object with handle() and render()), got {}",
        component.get_type().name()?
    )))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    // Names other areas already use in the flat native module carry an
    // `Interact` prefix there.
    for (name, class) in [
        ("InteractItem", py.get_type::<Item>()),
        ("InteractAction", py.get_type::<Action>()),
        ("InteractSelect", py.get_type::<Select>()),
        ("InteractMultiSelect", py.get_type::<MultiSelect>()),
        ("InteractInput", py.get_type::<Input>()),
        ("InteractChoice", py.get_type::<Choice>()),
        ("InteractConfirm", py.get_type::<Confirm>()),
        ("InteractForm", py.get_type::<Form>()),
        ("InteractPager", py.get_type::<Pager>()),
        ("InteractEvent", py.get_type::<Event>()),
        ("InteractDone", py.get_type::<Done>()),
        ("InteractCancel", py.get_type::<Cancel>()),
    ] {
        m.add(name, class)?;
    }
    Ok(())
}

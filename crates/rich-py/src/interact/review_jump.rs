//! `ApprovalQueue` (#471) and `Jump` with `JumpSource` (#484) for
//! `rs_rich.interact`: configurations built into a fresh `rich_interact`
//! component per run, like the other component classes. A review's answer
//! is a list of dicts, one per item; a jump's is a dict naming the source
//! and the item.

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};
use pyo3::{PyTraverseError, PyVisit};

use rich_interact::components::{
    ApprovalQueue as CoreQueue, ApprovalReport, Jump as CoreJump, JumpSource as CoreSource, Jumped,
    Verdict,
};
use rich_interact::{Item as CoreItem, PreviewLayout};

use super::components::{core_items, item_values, items, preview_layout, Item};
use super::compose::{self, Node};
use super::{execute, record, Build, Mode, Record};

// ---------------------------------------------------------------------------
// ApprovalQueue

/// `ApprovalQueue(prompt, items, *, height=10, preview="auto",
/// require_reason=False, auto_submit=False, advance=True, mouse=False)`:
/// review the items one by one or in bulk. `a`, `r` and `s` approve, reject
/// and skip the focused item (`A`, `R`, `S` ask for a reason first), Space
/// marks items for a decision on all of them, `u` takes one back, and
/// Enter submits. The answer is a list of dicts, one per item in order:
/// `index`, `value`, `label`, `decision` (`"approved"`, `"rejected"`,
/// `"skipped"` or `"pending"`) and `reason`. `require_reason` asks for a
/// reason on every rejection. Without a terminal it asks item by item.
#[pyclass(name = "ApprovalQueue", module = "rs_rich.interact", frozen)]
pub(crate) struct ApprovalQueue {
    #[pyo3(get)]
    prompt: String,
    items: Vec<Py<Item>>,
    #[pyo3(get)]
    height: usize,
    preview: PreviewLayout,
    #[pyo3(get)]
    require_reason: bool,
    #[pyo3(get)]
    auto_submit: bool,
    #[pyo3(get)]
    advance: bool,
    #[pyo3(get)]
    mouse: bool,
}

struct QueueBuild {
    prompt: String,
    items: Vec<CoreItem<usize>>,
    height: usize,
    preview: PreviewLayout,
    require_reason: bool,
    auto_submit: bool,
    advance: bool,
    mouse: bool,
}

impl Build for QueueBuild {
    type C = CoreQueue<usize>;

    fn build(self) -> CoreQueue<usize> {
        let queue = CoreQueue::new(self.prompt, self.items)
            .height(self.height)
            .preview(self.preview)
            .auto_submit(self.auto_submit)
            .advance(self.advance)
            .with_mouse(self.mouse);
        if self.require_reason {
            queue.require_reason(Verdict::Rejected)
        } else {
            queue
        }
    }
}

#[pymethods]
impl ApprovalQueue {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (
        prompt, items, *, height=10, preview="auto", require_reason=false, auto_submit=false,
        advance=true, mouse=false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        prompt: String,
        items: &Bound<'_, PyAny>,
        height: usize,
        preview: &str,
        require_reason: bool,
        auto_submit: bool,
        advance: bool,
        mouse: bool,
    ) -> PyResult<Self> {
        if height == 0 {
            return Err(PyValueError::new_err("height must be at least 1"));
        }
        Ok(ApprovalQueue {
            prompt,
            items: self::items(py, items)?,
            height,
            preview: preview_layout(preview)?,
            require_reason,
            auto_submit,
            advance,
            mouse,
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

impl ApprovalQueue {
    fn prepare(&self, py: Python<'_>) -> PyResult<QueueBuild> {
        Ok(QueueBuild {
            prompt: self.prompt.clone(),
            items: core_items(py, &self.items)?,
            height: self.height,
            preview: self.preview,
            require_reason: self.require_reason,
            auto_submit: self.auto_submit,
            advance: self.advance,
            mouse: self.mouse,
        })
    }
}

/// A report as a list of dicts, `values` giving each item's Python value.
fn report_list(
    py: Python<'_>,
    values: &[Py<PyAny>],
    report: ApprovalReport<usize>,
) -> PyResult<Py<PyAny>> {
    let list = PyList::empty(py);
    for decision in report.decisions {
        let entry = PyDict::new(py);
        entry.set_item("index", decision.index)?;
        entry.set_item("value", values[decision.value].clone_ref(py))?;
        entry.set_item("label", decision.label)?;
        entry.set_item("decision", decision.verdict.as_str())?;
        entry.set_item("reason", decision.reason)?;
        list.append(entry)?;
    }
    Ok(list.into_any().unbind())
}

// ---------------------------------------------------------------------------
// Jump and JumpSource

/// `JumpSource(id, title, items, *, prefix=None, limit=5)`: one source of a
/// `Jump`: files, headings, commands. `prefix`, a character typed first,
/// searches only this source; `limit` is how many results it shows while
/// every source is searched.
#[pyclass(name = "JumpSource", module = "rs_rich.interact", frozen)]
pub(crate) struct JumpSource {
    #[pyo3(get)]
    id: String,
    #[pyo3(get)]
    title: String,
    items: Vec<Py<Item>>,
    #[pyo3(get)]
    prefix: Option<char>,
    #[pyo3(get)]
    limit: usize,
}

#[pymethods]
impl JumpSource {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for item in &self.items {
            visit.call(item)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (id, title, items, *, prefix=None, limit=5))]
    fn new(
        py: Python<'_>,
        id: String,
        title: String,
        items: &Bound<'_, PyAny>,
        prefix: Option<String>,
        limit: usize,
    ) -> PyResult<Self> {
        let prefix = match prefix.as_deref() {
            None | Some("") => None,
            Some(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Some(c),
                    _ => return Err(PyValueError::new_err("prefix must be one character")),
                }
            }
        };
        if limit == 0 {
            return Err(PyValueError::new_err("limit must be at least 1"));
        }
        Ok(JumpSource {
            id,
            title,
            items: self::items(py, items)?,
            prefix,
            limit,
        })
    }

    #[getter]
    fn items(&self, py: Python<'_>) -> Vec<Py<Item>> {
        self.items.iter().map(|item| item.clone_ref(py)).collect()
    }

    fn __len__(&self) -> usize {
        self.items.len()
    }
}

/// `Jump(sources, prompt="Jump", *, query="", height=12, preview="auto",
/// mouse=False)`: one fuzzy box over several `JumpSource`s, the results
/// grouped by source. Typing searches every source, a source's prefix
/// typed first searches only it, Tab narrows to one source and Shift+Tab
/// widens, Alt+↑/↓ jump between groups. The answer is a dict: `source`
/// (its id), `index` (the item's in its source), `label` and `value`.
/// Without a terminal it answers with the best match for a line read, or,
/// asked for a default, for `query`.
#[pyclass(name = "Jump", module = "rs_rich.interact", frozen)]
pub(crate) struct Jump {
    sources: Vec<Py<JumpSource>>,
    #[pyo3(get)]
    prompt: String,
    #[pyo3(get)]
    query: String,
    #[pyo3(get)]
    height: usize,
    preview: PreviewLayout,
    #[pyo3(get)]
    mouse: bool,
}

struct JumpBuild {
    prompt: String,
    sources: Vec<CoreSource<usize>>,
    query: String,
    height: usize,
    preview: PreviewLayout,
    mouse: bool,
}

impl Build for JumpBuild {
    type C = CoreJump<usize>;

    fn build(self) -> CoreJump<usize> {
        let mut jump = CoreJump::new(self.prompt)
            .height(self.height)
            .preview(self.preview)
            .with_mouse(self.mouse);
        for source in self.sources {
            jump = jump.source(source);
        }
        if self.query.is_empty() {
            jump
        } else {
            jump.query(self.query)
        }
    }
}

#[pymethods]
impl Jump {
    fn __traverse__(&self, visit: PyVisit<'_>) -> Result<(), PyTraverseError> {
        for source in &self.sources {
            visit.call(source)?;
        }
        Ok(())
    }

    #[new]
    #[pyo3(signature = (sources, prompt="Jump".to_string(), *, query=String::new(), height=12, preview="auto", mouse=false))]
    fn new(
        sources: &Bound<'_, PyAny>,
        prompt: String,
        query: String,
        height: usize,
        preview: &str,
        mouse: bool,
    ) -> PyResult<Self> {
        if height == 0 {
            return Err(PyValueError::new_err("height must be at least 1"));
        }
        let mut kept = Vec::new();
        for source in sources.try_iter()? {
            let source = source?;
            let source = source
                .cast::<JumpSource>()
                .map_err(|_| PyTypeError::new_err("sources must be JumpSource objects"))?;
            kept.push(source.clone().unbind());
        }
        Ok(Jump {
            sources: kept,
            prompt,
            query,
            height,
            preview: preview_layout(preview)?,
            mouse,
        })
    }

    #[getter]
    fn sources(&self, py: Python<'_>) -> Vec<Py<JumpSource>> {
        self.sources.iter().map(|s| s.clone_ref(py)).collect()
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

impl Jump {
    /// The build, and each source's items' Python values.
    fn prepare(&self, py: Python<'_>) -> PyResult<(JumpBuild, Vec<Vec<Py<PyAny>>>)> {
        let mut sources = Vec::new();
        let mut values = Vec::new();
        for source in &self.sources {
            let source = source.get();
            let mut core = CoreSource::new(source.id.clone(), source.title.clone())
                .limit(source.limit)
                .items(core_items(py, &source.items)?);
            core.prefix = source.prefix;
            sources.push(core);
            values.push(
                source
                    .items
                    .iter()
                    .map(|item| item.get().value.clone_ref(py))
                    .collect(),
            );
        }
        Ok((
            JumpBuild {
                prompt: self.prompt.clone(),
                sources,
                query: self.query.clone(),
                height: self.height,
                preview: self.preview,
                mouse: self.mouse,
            },
            values,
        ))
    }
}

/// A jump's answer as a dict; `values` by source, then item.
fn jumped_dict(
    py: Python<'_>,
    ids: &[String],
    values: &[Vec<Py<PyAny>>],
    jumped: Jumped<usize>,
) -> PyResult<Py<PyAny>> {
    let source = ids.iter().position(|id| *id == jumped.source).unwrap_or(0);
    let entry = PyDict::new(py);
    entry.set_item("source", jumped.source)?;
    entry.set_item("index", jumped.index)?;
    entry.set_item("label", jumped.label)?;
    entry.set_item("value", values[source][jumped.value].clone_ref(py))?;
    Ok(entry.into_any().unbind())
}

// ---------------------------------------------------------------------------
// Dispatch

/// Run `component` if it is one of these classes; otherwise give `mode`
/// back for the next kind to try.
pub(crate) fn drive(
    py: Python<'_>,
    component: &Bound<'_, PyAny>,
    mode: Mode,
) -> PyResult<Result<Record, Mode>> {
    if let Ok(queue) = component.cast::<ApprovalQueue>() {
        let queue = queue.get();
        let values: Vec<Py<PyAny>> = queue
            .items
            .iter()
            .map(|i| i.get().value.clone_ref(py))
            .collect();
        let ran = execute(py, queue.prepare(py)?, mode)?;
        return record(py, ran, |report| report_list(py, &values, report)).map(Ok);
    }
    if let Ok(jump) = component.cast::<Jump>() {
        let jump = jump.get();
        let ids: Vec<String> = jump.sources.iter().map(|s| s.get().id.clone()).collect();
        let (build, values) = jump.prepare(py)?;
        let ran = execute(py, build, mode)?;
        return record(py, ran, |jumped| jumped_dict(py, &ids, &values, jumped)).map(Ok);
    }
    Ok(Err(mode))
}

/// The node for one of these as a child of a container (see
/// `compose::leaf`), if `component` is one.
pub(super) fn leaf(component: &Bound<'_, PyAny>) -> PyResult<Option<Node>> {
    let py = component.py();
    if let Ok(queue) = component.cast::<ApprovalQueue>() {
        let queue = queue.get();
        let values: Vec<Py<PyAny>> = queue
            .items
            .iter()
            .map(|i| i.get().value.clone_ref(py))
            .collect();
        let build = queue.prepare(py)?;
        return Ok(Some(compose::leaf(build, move |py, report| {
            report_list(py, &values, report)
        })));
    }
    if let Ok(jump) = component.cast::<Jump>() {
        let jump = jump.get();
        let ids: Vec<String> = jump.sources.iter().map(|s| s.get().id.clone()).collect();
        let (build, values) = jump.prepare(py)?;
        return Ok(Some(compose::leaf(build, move |py, jumped| {
            jumped_dict(py, &ids, &values, jumped)
        })));
    }
    Ok(None)
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("InteractApprovalQueue", py.get_type::<ApprovalQueue>())?;
    m.add("InteractJump", py.get_type::<Jump>())?;
    m.add("InteractJumpSource", py.get_type::<JumpSource>())?;
    Ok(())
}

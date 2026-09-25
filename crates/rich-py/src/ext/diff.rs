//! `rs_rich.ext.diff`: the diff engine (Myers, line, word and character
//! diffs, hunks, `diff -u`), `DiffView`, `SourceDiff`, git patches with
//! `PatchView`, and JUnit / libtest test reports.
//!
//! Word and character diffs report character offsets (Rust's are bytes).

use std::collections::BTreeMap;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyType};

use rich::protocol::Renderable;
use rich_ext::diff::git::{
    parse_unified, Annotation as CoreAnnotation, FilePatch as CoreFilePatch, FileStatus, LineKind,
    LinkProvider, Patch as CorePatch, PatchView as CorePatchView,
    TemplateLinks as CoreTemplateLinks,
};
use rich_ext::diff::test_report::{
    self as reports, Case as CoreCase, Status, Suite as CoreSuite, TestReport as CoreReport,
    TestRun as CoreRun,
};
use rich_ext::diff::transform::KeepFiles as CoreKeepFiles;
use rich_ext::diff::{
    self as core, DiffView as CoreDiffView, Layout, Op, SourceDiff as CoreSourceDiff,
    TextDiff as CoreTextDiff,
};
use rich_ext::transform::Transform;

use super::common::{self, names, PatchParseError, TestParseError, TransformError};
use super::diagnostic::{self, hyperlinker_arg};
use crate::renderable::{self, AsRenderable};

names!(layout, layout_name, Layout, "layout", {
    "unified" => Layout::Unified,
    "side_by_side" => Layout::SideBySide,
});

names!(status, status_name, Status, "status", {
    "passed" => Status::Passed,
    "failed" => Status::Failed,
    "skipped" => Status::Skipped,
    "errored" => Status::Errored,
});

type Range = (usize, usize);

/// An edit-script run as `(tag, (old_start, old_end), (new_start, new_end))`,
/// `tag` one of `equal`, `delete`, `insert`.
fn op_tuple(op: &Op, map: impl Fn(bool, usize) -> usize) -> (&'static str, Range, Range) {
    let (old, new) = (op.old(), op.new_range());
    let tag = match op {
        Op::Equal { .. } => "equal",
        Op::Delete { .. } => "delete",
        Op::Insert { .. } => "insert",
    };
    (
        tag,
        (map(true, old.start), map(true, old.end)),
        (map(false, new.start), map(false, new.end)),
    )
}

fn plain_op(op: &Op) -> (&'static str, Range, Range) {
    op_tuple(op, |_, i| i)
}

fn text_ops(old: &str, new: &str, ops: Vec<Op>) -> Vec<(&'static str, Range, Range)> {
    ops.iter()
        .map(|op| {
            op_tuple(op, |is_old, offset| {
                common::char_index(if is_old { old } else { new }, offset)
            })
        })
        .collect()
}

fn op_arg(value: &Bound<'_, PyAny>) -> PyResult<Op> {
    let (tag, old, new): (String, Range, Range) = value.extract()?;
    let (old, new) = (old.0..old.1, new.0..new.1);
    Ok(match tag.as_str() {
        "equal" => Op::Equal { old, new },
        "delete" => Op::Delete { old, new },
        "insert" => Op::Insert { old, new },
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid op tag {other:?}; expected equal, delete or insert"
            )))
        }
    })
}

/// `diff_sequences(old, new)`: Myers' diff of two sequences of hashable
/// Python values, as `(tag, old_range, new_range)` runs.
#[pyfunction]
fn diff_sequences(
    py: Python<'_>,
    old: &Bound<'_, PyAny>,
    new: &Bound<'_, PyAny>,
) -> PyResult<Vec<(&'static str, Range, Range)>> {
    // Intern each value by Python equality, so any hashable values diff.
    let ids = PyDict::new(py);
    let mut intern = |value: PyResult<Bound<'_, PyAny>>| -> PyResult<usize> {
        let value = value?;
        if let Some(id) = ids.get_item(&value)? {
            return id.extract();
        }
        let id = ids.len();
        ids.set_item(&value, id)?;
        Ok(id)
    };
    let a: Vec<usize> = old.try_iter()?.map(&mut intern).collect::<PyResult<_>>()?;
    let b: Vec<usize> = new.try_iter()?.map(&mut intern).collect::<PyResult<_>>()?;
    Ok(core::diff_slices(&a, &b).iter().map(plain_op).collect())
}

/// `diff_lines(old, new)`: a line diff of two lists of lines.
#[pyfunction]
fn diff_lines(old: Vec<String>, new: Vec<String>) -> Vec<(&'static str, Range, Range)> {
    let a: Vec<&str> = old.iter().map(String::as_str).collect();
    let b: Vec<&str> = new.iter().map(String::as_str).collect();
    core::diff_lines(&a, &b).iter().map(plain_op).collect()
}

/// `diff_words(old, new)`: a word diff; ranges are character offsets.
#[pyfunction]
fn diff_words(old: &str, new: &str) -> Vec<(&'static str, Range, Range)> {
    text_ops(old, new, core::diff_words(old, new))
}

/// `diff_chars(old, new)`: a character diff; ranges are character offsets.
#[pyfunction]
fn diff_chars(old: &str, new: &str) -> Vec<(&'static str, Range, Range)> {
    text_ops(old, new, core::diff_chars(old, new))
}

/// `tokenize(text)`: word, whitespace and punctuation tokens as
/// `(start, end)` character offsets.
#[pyfunction]
fn tokenize(text: &str) -> Vec<Range> {
    core::tokenize(text)
        .into_iter()
        .map(|r| {
            (
                common::char_index(text, r.start),
                common::char_index(text, r.end),
            )
        })
        .collect()
}

/// `hunk_header(old_range, new_range)`: `@@ -a,b +c,d @@` for 0-based ranges.
#[pyfunction]
fn hunk_header(old: Range, new: Range) -> String {
    core::hunk_header(old.0..old.1, new.0..new.1)
}

/// A group of changes with context, as `diff -u` prints them.
#[pyclass(name = "Hunk", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct Hunk {
    inner: core::Hunk,
}

#[pymethods]
impl Hunk {
    #[getter]
    fn ops(&self) -> Vec<(&'static str, Range, Range)> {
        self.inner.ops.iter().map(plain_op).collect()
    }
    #[getter]
    fn old_range(&self) -> Range {
        let r = self.inner.old_range();
        (r.start, r.end)
    }
    #[getter]
    fn new_range(&self) -> Range {
        let r = self.inner.new_range();
        (r.start, r.end)
    }
    #[getter]
    fn header(&self) -> String {
        self.inner.header()
    }
    fn __repr__(&self) -> String {
        format!("<Hunk {}>", self.inner.header())
    }
}

/// `group_hunks(ops, context=3)`: runs grouped into hunks.
#[pyfunction]
#[pyo3(signature = (ops, context=3))]
fn group_hunks(ops: &Bound<'_, PyAny>, context: usize) -> PyResult<Vec<Hunk>> {
    let ops: Vec<Op> = ops
        .try_iter()?
        .map(|op| op_arg(&op?))
        .collect::<PyResult<_>>()?;
    Ok(core::group_hunks(&ops, context)
        .into_iter()
        .map(|inner| Hunk { inner })
        .collect())
}

/// `TextDiff(old, new, *, context=3)`: a line diff of two strings, with
/// hunks, stats and `diff -u` output.
#[pyclass(name = "TextDiff", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct TextDiff {
    inner: CoreTextDiff,
}

#[pymethods]
impl TextDiff {
    #[new]
    #[pyo3(signature = (old, new, *, context=3))]
    fn new(old: &str, new: &str, context: usize) -> Self {
        TextDiff {
            inner: CoreTextDiff::new(old, new).context(context),
        }
    }
    #[getter]
    fn context(&self) -> usize {
        self.inner.context_lines()
    }
    #[getter]
    fn ops(&self) -> Vec<(&'static str, Range, Range)> {
        self.inner.ops().iter().map(plain_op).collect()
    }
    #[getter]
    fn old_lines(&self) -> Vec<String> {
        self.inner.old_lines().to_vec()
    }
    #[getter]
    fn new_lines(&self) -> Vec<String> {
        self.inner.new_lines().to_vec()
    }
    fn hunks(&self) -> Vec<Hunk> {
        self.inner
            .hunks()
            .into_iter()
            .map(|inner| Hunk { inner })
            .collect()
    }
    #[getter]
    fn is_equal(&self) -> bool {
        self.inner.is_equal()
    }
    /// `(added, removed)` line counts.
    fn stats(&self) -> (usize, usize) {
        self.inner.stats()
    }
    /// The diff as `diff -u` prints it (without timestamps).
    #[pyo3(signature = (old_name="a", new_name="b"))]
    fn unified(&self, old_name: &str, new_name: &str) -> String {
        self.inner.unified(old_name, new_name)
    }
}

/// `DiffView(old, new, *, ansi=False, layout="unified", line_numbers=True,
/// wrap=True, context=3, titles=None, emphasis=True)`: a line diff as a
/// renderable. `ansi=True` reads both sides as ANSI text and also reports
/// lines whose styling alone changed (`~`).
#[pyclass(name = "DiffView", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct DiffView {
    inner: CoreDiffView,
}

impl AsRenderable for DiffView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

fn configure(
    mut view: CoreDiffView,
    layout: &str,
    line_numbers: bool,
    wrap: bool,
    context: usize,
    titles: Option<(String, String)>,
    emphasis: bool,
) -> PyResult<CoreDiffView> {
    view = view
        .layout(self::layout(layout)?)
        .line_numbers(line_numbers)
        .wrap(wrap)
        .context(context)
        .emphasis(emphasis);
    if let Some((old, new)) = titles {
        view = view.titles(old, new);
    }
    Ok(view)
}

#[pymethods]
impl DiffView {
    #[new]
    #[pyo3(signature = (old, new, *, ansi=false, layout="unified", line_numbers=true, wrap=true, context=3, titles=None, emphasis=true))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        old: &str,
        new: &str,
        ansi: bool,
        layout: &str,
        line_numbers: bool,
        wrap: bool,
        context: usize,
        titles: Option<(String, String)>,
        emphasis: bool,
    ) -> PyResult<Self> {
        let view = if ansi {
            CoreDiffView::ansi(old, new)
        } else {
            CoreDiffView::new(old, new)
        };
        Ok(DiffView {
            inner: configure(view, layout, line_numbers, wrap, context, titles, emphasis)?,
        })
    }

    /// A view of a `TextDiff` (keeping its context).
    #[classmethod]
    #[pyo3(signature = (diff, *, layout="unified", line_numbers=true, wrap=true, titles=None, emphasis=true))]
    fn from_diff(
        _cls: &Bound<'_, PyType>,
        diff: PyRef<'_, TextDiff>,
        layout: &str,
        line_numbers: bool,
        wrap: bool,
        titles: Option<(String, String)>,
        emphasis: bool,
    ) -> PyResult<Self> {
        let context = diff.inner.context_lines();
        Ok(DiffView {
            inner: configure(
                CoreDiffView::from_diff(&diff.inner),
                layout,
                line_numbers,
                wrap,
                context,
                titles,
                emphasis,
            )?,
        })
    }

    /// Whether both sides render identically.
    #[getter]
    fn is_equal(&self) -> bool {
        self.inner.is_equal()
    }

    /// `(added, removed, restyled)` line counts.
    fn stats(&self) -> (usize, usize, usize) {
        self.inner.stats()
    }

    /// The 1-based new-side lines whose text matches but styling differs.
    fn style_changed_lines(&self) -> Vec<usize> {
        self.inner.style_changed_lines()
    }
}

/// `SourceDiff(old, new, *, language=None, path=None, paths=None, ...)`: a
/// syntax-highlighted source diff, with optional line links.
#[pyclass(name = "SourceDiff", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct SourceDiff {
    inner: CoreSourceDiff,
}

impl AsRenderable for SourceDiff {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl SourceDiff {
    #[new]
    #[pyo3(signature = (
        old, new, *, language=None, path=None, paths=None, layout="unified", line_numbers=true,
        wrap=true, context=3, titles=true, link_template=None, hyperlinker=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        old: String,
        new: String,
        language: Option<String>,
        path: Option<String>,
        paths: Option<(String, String)>,
        layout: &str,
        line_numbers: bool,
        wrap: bool,
        context: usize,
        titles: bool,
        link_template: Option<String>,
        hyperlinker: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut inner = CoreSourceDiff::new(old, new)
            .layout(self::layout(layout)?)
            .line_numbers(line_numbers)
            .wrap(wrap)
            .context(context)
            .titles(titles);
        if let Some(language) = language {
            inner = inner.language(language);
        }
        if let Some(path) = path {
            inner = inner.path(path);
        }
        if let Some((old, new)) = paths {
            inner = inner.paths(old, new);
        }
        if let Some(template) = link_template {
            inner = inner.link_template(template);
        }
        if let Some(linker) = hyperlinker_arg(hyperlinker)? {
            inner = inner.hyperlinker(linker);
        }
        Ok(SourceDiff { inner })
    }

    /// The highlighted diff as a `DiffView`.
    fn view(&self) -> DiffView {
        DiffView {
            inner: self.inner.view(),
        }
    }
}

// ---------------------------------------------------------------------------
// Git patches

/// One file of a parsed patch.
#[pyclass(name = "FilePatch", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct FilePatch {
    inner: CoreFilePatch,
}

fn file_status_name(status: FileStatus) -> &'static str {
    match status {
        FileStatus::Added => "added",
        FileStatus::Deleted => "deleted",
        FileStatus::Modified => "modified",
        FileStatus::Renamed => "renamed",
        FileStatus::Copied => "copied",
        FileStatus::Binary => "binary",
    }
}

#[pymethods]
impl FilePatch {
    /// The new path, or the old one for a deleted file.
    #[getter]
    fn path(&self) -> String {
        self.inner.path().to_string()
    }
    #[getter]
    fn old_path(&self) -> Option<String> {
        self.inner.old_path.clone()
    }
    #[getter]
    fn new_path(&self) -> Option<String> {
        self.inner.new_path.clone()
    }
    #[getter]
    fn status(&self) -> &'static str {
        file_status_name(self.inner.status)
    }
    #[getter]
    fn binary(&self) -> bool {
        self.inner.binary
    }
    #[getter]
    fn old_mode(&self) -> Option<String> {
        self.inner.old_mode.clone()
    }
    #[getter]
    fn new_mode(&self) -> Option<String> {
        self.inner.new_mode.clone()
    }
    #[getter]
    fn mode_changed(&self) -> bool {
        self.inner.mode_changed()
    }
    #[getter]
    fn similarity(&self) -> Option<u8> {
        self.inner.similarity
    }
    #[getter]
    fn additions(&self) -> usize {
        self.inner.additions
    }
    #[getter]
    fn deletions(&self) -> usize {
        self.inner.deletions
    }
    /// The hunks as `(header, [(kind, text, old_line, new_line), ...])`,
    /// `kind` one of `context`, `added`, `removed`.
    #[allow(clippy::type_complexity)]
    #[getter]
    fn hunks(
        &self,
    ) -> Vec<(
        String,
        Vec<(&'static str, String, Option<usize>, Option<usize>)>,
    )> {
        self.inner
            .hunks
            .iter()
            .map(|hunk| {
                let lines = hunk
                    .lines
                    .iter()
                    .map(|line| {
                        let kind = match line.kind {
                            LineKind::Context => "context",
                            LineKind::Added => "added",
                            LineKind::Removed => "removed",
                        };
                        (kind, line.text.clone(), line.old_line, line.new_line)
                    })
                    .collect();
                (hunk.header(), lines)
            })
            .collect()
    }
    fn __repr__(&self) -> String {
        format!(
            "<FilePatch {} {} +{} -{}>",
            file_status_name(self.inner.status),
            self.inner.path(),
            self.inner.additions,
            self.inner.deletions
        )
    }
}

/// A parsed `git diff`: `parse_patch(text)`.
#[pyclass(
    name = "Patch",
    module = "rs_rich.ext.diff",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Patch {
    inner: CorePatch,
}

#[pymethods]
impl Patch {
    #[getter]
    fn files(&self) -> Vec<FilePatch> {
        self.inner
            .files
            .iter()
            .map(|f| FilePatch { inner: f.clone() })
            .collect()
    }
    /// Total `(additions, deletions)`.
    fn stats(&self) -> (usize, usize) {
        self.inner.stats()
    }
    fn __len__(&self) -> usize {
        self.inner.files.len()
    }
}

/// `parse_patch(text)`: parse unified `git diff` output.
#[pyfunction]
fn parse_patch(py: Python<'_>, text: &str) -> PyResult<Patch> {
    parse_unified(text)
        .map(|inner| Patch { inner })
        .map_err(|e| {
            let err = PatchParseError::new_err(format!("line {}: {}", e.line, e.message));
            let _ = err.value(py).setattr("line", e.line);
            err
        })
}

/// `Annotation(path, line, message, *, level="warning")`: a note under a
/// new-side line of a `PatchView`.
#[pyclass(name = "Annotation", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct Annotation {
    inner: CoreAnnotation,
}

#[pymethods]
impl Annotation {
    #[new]
    #[pyo3(signature = (path, line, message, *, level="warning"))]
    fn new(path: String, line: usize, message: String, level: &str) -> PyResult<Self> {
        Ok(Annotation {
            inner: CoreAnnotation::new(path, line, diagnostic::level(level)?, message),
        })
    }
}

/// `TemplateLinks(line_template, *, file_template=None, **vars)`: file and
/// line links from URL templates with `{path}`, `{line}` and your `{name}`s.
#[pyclass(name = "TemplateLinks", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct TemplateLinks {
    inner: CoreTemplateLinks,
}

#[pymethods]
impl TemplateLinks {
    #[new]
    #[pyo3(signature = (line_template, *, file_template=None, **vars))]
    fn new(
        line_template: String,
        file_template: Option<String>,
        vars: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Self> {
        let mut inner = CoreTemplateLinks::new(line_template);
        if let Some(template) = file_template {
            inner = inner.file_template(template);
        }
        if let Some(vars) = vars {
            for (name, value) in vars.iter() {
                inner = inner.var(name.extract::<String>()?, value.str()?.to_string());
            }
        }
        Ok(TemplateLinks { inner })
    }

    fn file_url(&self, path: &str) -> Option<String> {
        LinkProvider::file_url(&self.inner, path)
    }

    fn line_url(&self, path: &str, line: usize) -> Option<String> {
        LinkProvider::line_url(&self.inner, path, line)
    }
}

enum Links {
    Template(CoreTemplateLinks),
    Linker(rich_ext::hyperlink::Hyperlinker),
}

/// `PatchView(patch, *, annotations=(), links=None, layout="unified", ...)`:
/// a review-style patch with a file tree, annotations and links.
#[pyclass(name = "PatchView", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct PatchView {
    patch: CorePatch,
    annotations: Vec<CoreAnnotation>,
    links: Option<Links>,
    layout: Layout,
    line_numbers: bool,
    wrap: bool,
    highlight: bool,
    tree: bool,
    emphasis: bool,
}

impl AsRenderable for PatchView {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let mut view = CorePatchView::new(self.patch.clone())
            .annotations(self.annotations.iter().cloned())
            .layout(self.layout)
            .line_numbers(self.line_numbers)
            .wrap(self.wrap)
            .highlight(self.highlight)
            .tree(self.tree)
            .emphasis(self.emphasis);
        match &self.links {
            Some(Links::Template(links)) => view = view.links(links.clone()),
            Some(Links::Linker(linker)) => view = view.links(linker.clone()),
            None => {}
        }
        Ok(Box::new(view))
    }
}

fn patch_arg(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<CorePatch> {
    if let Ok(text) = value.extract::<String>() {
        return Ok(parse_patch(py, &text)?.inner);
    }
    Ok(value.extract::<PyRef<'_, Patch>>()?.inner.clone())
}

#[pymethods]
impl PatchView {
    #[new]
    #[pyo3(signature = (
        patch, *, annotations=None, links=None, layout="unified", line_numbers=true, wrap=true,
        highlight=true, tree=true, emphasis=true
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        patch: &Bound<'_, PyAny>,
        annotations: Option<&Bound<'_, PyAny>>,
        links: Option<&Bound<'_, PyAny>>,
        layout: &str,
        line_numbers: bool,
        wrap: bool,
        highlight: bool,
        tree: bool,
        emphasis: bool,
    ) -> PyResult<Self> {
        let mut notes = Vec::new();
        if let Some(items) = annotations {
            for item in items.try_iter()? {
                notes.push(item?.extract::<PyRef<'_, Annotation>>()?.inner.clone());
            }
        }
        let links = match links.filter(|v| !v.is_none()) {
            None => None,
            Some(value) => match value.extract::<PyRef<'_, TemplateLinks>>() {
                Ok(links) => Some(Links::Template(links.inner.clone())),
                Err(_) => hyperlinker_arg(Some(value))?.map(Links::Linker),
            },
        };
        Ok(PatchView {
            patch: patch_arg(py, patch)?,
            annotations: notes,
            links,
            layout: self::layout(layout)?,
            line_numbers,
            wrap,
            highlight,
            tree,
            emphasis,
        })
    }

    #[getter]
    fn patch(&self) -> Patch {
        Patch {
            inner: self.patch.clone(),
        }
    }
}

/// `KeepFiles(patterns)`: keep the files of a patch whose path matches a
/// pattern (a case-insensitive substring, or a glob with `*` and `?`).
#[pyclass(name = "KeepFiles", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct KeepFiles {
    patterns: Vec<String>,
}

#[pymethods]
impl KeepFiles {
    #[new]
    fn new(patterns: &Bound<'_, PyAny>) -> PyResult<Self> {
        Ok(KeepFiles {
            patterns: common::strings(patterns)?,
        })
    }

    fn apply(&self, py: Python<'_>, patch: &Bound<'_, PyAny>) -> PyResult<Patch> {
        let patch = patch_arg(py, patch)?;
        CoreKeepFiles(self.patterns.clone())
            .apply(patch)
            .map(|inner| Patch { inner })
            .map_err(|e| TransformError::new_err(e.message().to_string()))
    }

    fn __call__(&self, py: Python<'_>, patch: &Bound<'_, PyAny>) -> PyResult<Patch> {
        self.apply(py, patch)
    }
}

// ---------------------------------------------------------------------------
// Test reports

/// One test case.
#[pyclass(name = "TestCase", module = "rs_rich.ext.diff", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct TestCase {
    inner: CoreCase,
}

#[pymethods]
impl TestCase {
    /// Not a pytest test class.
    #[classattr]
    #[allow(non_upper_case_globals)]
    const __test__: bool = false;

    #[new]
    #[pyo3(signature = (
        name, classname=String::new(), status="passed", *, duration=None, message=None, details=None,
        stdout=None, stderr=None, expected=None, actual=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        name: String,
        classname: String,
        status: &str,
        duration: Option<&Bound<'_, PyAny>>,
        message: Option<String>,
        details: Option<String>,
        stdout: Option<String>,
        stderr: Option<String>,
        expected: Option<String>,
        actual: Option<String>,
    ) -> PyResult<Self> {
        let mut inner = CoreCase::new(name, classname, self::status(status)?);
        inner.duration = common::opt_seconds(duration)?;
        inner.message = message;
        inner.details = details;
        inner.stdout = stdout;
        inner.stderr = stderr;
        inner.expected = expected;
        inner.actual = actual;
        Ok(TestCase { inner })
    }
    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    fn classname(&self) -> String {
        self.inner.classname.clone()
    }
    #[getter]
    fn status(&self) -> &'static str {
        status_name(self.inner.status)
    }
    #[getter]
    fn duration(&self) -> Option<f64> {
        self.inner.duration.map(|d| d.as_secs_f64())
    }
    #[getter]
    fn message(&self) -> Option<String> {
        self.inner.message.clone()
    }
    #[getter]
    fn details(&self) -> Option<String> {
        self.inner.details.clone()
    }
    #[getter]
    fn stdout(&self) -> Option<String> {
        self.inner.stdout.clone()
    }
    #[getter]
    fn stderr(&self) -> Option<String> {
        self.inner.stderr.clone()
    }
    #[getter]
    fn expected(&self) -> Option<String> {
        self.inner.expected.clone()
    }
    #[getter]
    fn actual(&self) -> Option<String> {
        self.inner.actual.clone()
    }
    /// `classname.name` (or the name alone).
    #[getter]
    fn full_name(&self) -> String {
        self.inner.full_name()
    }
    #[getter]
    fn is_failure(&self) -> bool {
        self.inner.is_failure()
    }
    fn __repr__(&self) -> String {
        format!(
            "<TestCase {} {}>",
            self.inner.full_name(),
            status_name(self.inner.status)
        )
    }
}

/// A suite of test cases.
#[pyclass(name = "TestSuite", module = "rs_rich.ext.diff", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct TestSuite {
    inner: CoreSuite,
}

fn totals_dict(totals: reports::Totals) -> BTreeMap<&'static str, usize> {
    BTreeMap::from([
        ("passed", totals.passed),
        ("failed", totals.failed),
        ("errored", totals.errored),
        ("skipped", totals.skipped),
        ("total", totals.total()),
    ])
}

#[pymethods]
impl TestSuite {
    /// Not a pytest test class.
    #[classattr]
    #[allow(non_upper_case_globals)]
    const __test__: bool = false;

    #[new]
    #[pyo3(signature = (name, cases=None, *, duration=None))]
    fn new(
        name: String,
        cases: Option<&Bound<'_, PyAny>>,
        duration: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut suite = CoreSuite {
            name,
            duration: common::opt_seconds(duration)?,
            ..CoreSuite::default()
        };
        if let Some(cases) = cases {
            for case in cases.try_iter()? {
                suite
                    .cases
                    .push(case?.extract::<PyRef<'_, TestCase>>()?.inner.clone());
            }
        }
        Ok(TestSuite { inner: suite })
    }
    #[getter]
    fn name(&self) -> String {
        self.inner.name.clone()
    }
    #[getter]
    fn cases(&self) -> Vec<TestCase> {
        self.inner
            .cases
            .iter()
            .map(|c| TestCase { inner: c.clone() })
            .collect()
    }
    #[getter]
    fn duration(&self) -> Option<f64> {
        self.inner.duration.map(|d| d.as_secs_f64())
    }
    /// `{"passed": n, "failed": n, "errored": n, "skipped": n, "total": n}`.
    fn totals(&self) -> BTreeMap<&'static str, usize> {
        totals_dict(self.inner.totals())
    }
    /// The suite's time: its own, or the sum of its cases'.
    fn time(&self) -> Option<f64> {
        self.inner.time().map(|d| d.as_secs_f64())
    }
}

/// A test run: suites of cases, from JUnit XML or libtest JSON.
#[pyclass(name = "TestRun", module = "rs_rich.ext.diff", skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct TestRun {
    inner: CoreRun,
}

fn parse_error(error: reports::TestParseError) -> PyErr {
    TestParseError::new_err(format!("{}: {}", error.format, error.message))
}

#[pymethods]
impl TestRun {
    /// Not a pytest test class.
    #[classattr]
    #[allow(non_upper_case_globals)]
    const __test__: bool = false;

    #[new]
    #[pyo3(signature = (suites=None, *, duration=None))]
    fn new(
        suites: Option<&Bound<'_, PyAny>>,
        duration: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let mut run = CoreRun {
            duration: common::opt_seconds(duration)?,
            ..CoreRun::default()
        };
        if let Some(suites) = suites {
            for suite in suites.try_iter()? {
                run.suites
                    .push(suite?.extract::<PyRef<'_, TestSuite>>()?.inner.clone());
            }
        }
        Ok(TestRun { inner: run })
    }

    /// Parse JUnit XML.
    #[staticmethod]
    fn from_junit(xml: &str) -> PyResult<Self> {
        reports::junit::parse(xml)
            .map(|inner| TestRun { inner })
            .map_err(parse_error)
    }

    /// Parse libtest JSON (`cargo test -- -Z unstable-options --format json`).
    #[staticmethod]
    fn from_libtest(json: &str) -> PyResult<Self> {
        reports::libtest::parse(json)
            .map(|inner| TestRun { inner })
            .map_err(parse_error)
    }

    #[getter]
    fn suites(&self) -> Vec<TestSuite> {
        self.inner
            .suites
            .iter()
            .map(|s| TestSuite { inner: s.clone() })
            .collect()
    }
    #[getter]
    fn duration(&self) -> Option<f64> {
        self.inner.duration.map(|d| d.as_secs_f64())
    }
    fn totals(&self) -> BTreeMap<&'static str, usize> {
        totals_dict(self.inner.totals())
    }
    fn time(&self) -> Option<f64> {
        self.inner.time().map(|d| d.as_secs_f64())
    }
    #[getter]
    fn is_success(&self) -> bool {
        self.inner.is_success()
    }
    /// The run as JUnit XML.
    fn to_junit_xml(&self) -> String {
        self.inner.to_junit_xml()
    }
}

/// `TestReport(run, *, show_passed=False, show_output=True,
/// diff_context=3)`: failures with expected/actual diffs, then totals. `run`
/// may be a `TestRun` or JUnit XML / libtest JSON text.
#[pyclass(name = "TestReport", module = "rs_rich.ext.diff", frozen)]
pub(crate) struct TestReport {
    inner: CoreReport,
}

impl AsRenderable for TestReport {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(self.inner.clone()))
    }
}

#[pymethods]
impl TestReport {
    /// Not a pytest test class.
    #[classattr]
    #[allow(non_upper_case_globals)]
    const __test__: bool = false;

    #[new]
    #[pyo3(signature = (run, *, show_passed=false, show_output=true, diff_context=3))]
    fn new(
        run: &Bound<'_, PyAny>,
        show_passed: bool,
        show_output: bool,
        diff_context: usize,
    ) -> PyResult<Self> {
        let run = match run.extract::<String>() {
            Ok(text) if text.trim_start().starts_with('<') => TestRun::from_junit(&text)?.inner,
            Ok(text) => TestRun::from_libtest(&text)?.inner,
            Err(_) => run.extract::<PyRef<'_, TestRun>>()?.inner.clone(),
        };
        Ok(TestReport {
            inner: CoreReport::new(run)
                .show_passed(show_passed)
                .show_output(show_output)
                .diff_context(diff_context),
        })
    }

    #[getter]
    fn run(&self) -> TestRun {
        TestRun {
            inner: self.inner.run().clone(),
        }
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(pyo3::wrap_pyfunction!(diff_sequences, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(diff_lines, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(diff_words, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(diff_chars, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(tokenize, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(hunk_header, m)?)?;
    m.add_function(pyo3::wrap_pyfunction!(group_hunks, m)?)?;
    m.add_class::<Hunk>()?;
    m.add_class::<TextDiff>()?;
    renderable::add_renderable_class::<DiffView>(m)?;
    renderable::add_renderable_class::<SourceDiff>(m)?;
    m.add_class::<FilePatch>()?;
    m.add_class::<Patch>()?;
    m.add_function(pyo3::wrap_pyfunction!(parse_patch, m)?)?;
    m.add_class::<Annotation>()?;
    m.add_class::<TemplateLinks>()?;
    renderable::add_renderable_class::<PatchView>(m)?;
    m.add_class::<KeepFiles>()?;
    m.add_class::<TestCase>()?;
    m.add_class::<TestSuite>()?;
    m.add_class::<TestRun>()?;
    renderable::add_renderable_class::<TestReport>(m)?;
    m.add("DIFF_STYLES", core::STYLES.to_vec())?;
    Ok(())
}

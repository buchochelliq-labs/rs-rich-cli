//! `rs_rich.ext.frame`: `rich_ext::frame` (0.0.13 workstream 1), a rendered
//! result as rows of styled runs, with its cells and a cell-level diff.
//!
//! A `Frame` is built from segments (`Frame.from_segments`), from a
//! `Console`'s render (`Frame.from_console`) or by a `RenderTarget`
//! (`RenderTarget.frame`). The frame is Rust's; this module only converts
//! its rows, cells and changes to Python values.

use pyo3::prelude::*;
use pyo3::types::{PyList, PyRange, PyTuple};

use rich_ext::frame::Frame as CoreFrame;

use super::terminal::{color_system, segments_arg};
use crate::style::Style;

/// `Frame`: rows of styled runs. Build one with `Frame.from_segments`,
/// `Frame.from_console` or `RenderTarget.frame`.
#[pyclass(
    name = "Frame",
    module = "rs_rich.ext.frame",
    frozen,
    skip_from_py_object
)]
#[derive(Clone)]
pub(crate) struct Frame {
    pub(crate) inner: CoreFrame,
}

impl Frame {
    pub(crate) fn from_core(inner: CoreFrame) -> Frame {
        Frame { inner }
    }

    fn check_row(&self, row: usize) -> PyResult<()> {
        if row >= self.inner.height() {
            return Err(pyo3::exceptions::PyIndexError::new_err(format!(
                "row {row} out of range for a frame of {} rows",
                self.inner.height()
            )));
        }
        Ok(())
    }
}

fn style_object(py: Python<'_>, style: Option<&rich::Style>) -> PyResult<Option<Py<Style>>> {
    style
        .map(|style| Py::new(py, Style::from_core(style.clone())))
        .transpose()
}

#[pymethods]
impl Frame {
    /// A frame from `Segment`s; control segments are dropped.
    #[staticmethod]
    fn from_segments(segments: &Bound<'_, PyAny>) -> PyResult<Frame> {
        Ok(Frame::from_core(CoreFrame::from_segments(&segments_arg(
            segments,
        )?)))
    }

    /// A frame of what `console.render(renderable, options)` yields.
    #[staticmethod]
    #[pyo3(signature = (console, renderable, options=None))]
    fn from_console(
        console: &Bound<'_, PyAny>,
        renderable: &Bound<'_, PyAny>,
        options: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Frame> {
        let rendered = console.call_method1("render", (renderable, options))?;
        Frame::from_segments(&rendered)
    }

    /// The number of rows (a final line break starts none).
    #[getter]
    fn height(&self) -> usize {
        self.inner.height()
    }

    /// The widest row, in cells.
    #[getter]
    fn width(&self) -> usize {
        self.inner.width()
    }

    /// Whether the rendered stream ended with a line break.
    #[getter]
    fn ends_with_newline(&self) -> bool {
        self.inner.ends_with_newline()
    }

    /// The total number of runs.
    #[getter]
    fn run_count(&self) -> usize {
        self.inner.run_count()
    }

    /// The number of distinct styles (no style counts as one).
    #[getter]
    fn style_count(&self) -> usize {
        self.inner.styles().len()
    }

    /// The width of row `row` in cells.
    fn row_width(&self, row: usize) -> PyResult<usize> {
        self.check_row(row)?;
        Ok(self.inner.row_width(row))
    }

    /// Row `row`'s runs as `(text, cells, style)`.
    fn row<'py>(&self, py: Python<'py>, row: usize) -> PyResult<Bound<'py, PyList>> {
        self.check_row(row)?;
        let styles = self.inner.styles();
        let runs = self
            .inner
            .row(row)
            .iter()
            .map(|run| {
                Ok((
                    self.inner.run_text(run),
                    run.cells(),
                    style_object(py, styles.get(run.style()))?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, runs)
    }

    /// Row `row` as one cell per column: `(text, width, style)`. The cell
    /// after a wide character is a continuation, `("", 0, style)`.
    fn cells<'py>(&self, py: Python<'py>, row: usize) -> PyResult<Bound<'py, PyList>> {
        self.check_row(row)?;
        let styles = self.inner.styles();
        let cells = self
            .inner
            .cells(row)
            .into_iter()
            .map(|cell| {
                Ok((
                    cell.text,
                    cell.width,
                    style_object(py, styles.get(cell.style))?,
                ))
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, cells)
    }

    /// The text without styles, rows joined by line breaks.
    fn plain(&self) -> String {
        self.inner.plain()
    }

    /// The same bytes `console` writes for the segments the frame was built
    /// from: its colour system and `no_color`.
    fn to_ansi(&self, console: &Bound<'_, PyAny>) -> PyResult<String> {
        let (system, no_color) = console_colors(console)?;
        Ok(self.inner.encode(system, no_color))
    }

    /// `to_ansi` after merging adjacent runs of one style: fewer bytes, not
    /// Rich's.
    fn to_ansi_merged(&self, console: &Bound<'_, PyAny>) -> PyResult<String> {
        let (system, no_color) = console_colors(console)?;
        Ok(self.inner.merged().encode(system, no_color))
    }

    /// Encode with a colour system (`"truecolor"`, `"256"`, `"standard"`,
    /// `"windows"` or `None` for no styles).
    #[pyo3(signature = (color_system="truecolor", no_color=false))]
    fn encode(&self, color_system: Option<&str>, no_color: bool) -> PyResult<String> {
        Ok(self
            .inner
            .encode(self::color_system(color_system)?, no_color))
    }

    /// Encode columns `start` to `end` of row `row`: what a cell-level
    /// repaint writes for one change.
    #[pyo3(signature = (row, start, end, color_system="truecolor", no_color=false))]
    fn encode_span(
        &self,
        row: usize,
        start: usize,
        end: usize,
        color_system: Option<&str>,
        no_color: bool,
    ) -> PyResult<String> {
        self.check_row(row)?;
        Ok(self.inner.encode_span(
            row,
            start..end.max(start),
            self::color_system(color_system)?,
            no_color,
        ))
    }

    /// A copy with adjacent runs of one style merged.
    fn merged(&self) -> Frame {
        Frame::from_core(self.inner.merged())
    }

    /// The cells that differ from `previous`, as `(row, range(start, end))`
    /// in row order.
    fn diff<'py>(
        &self,
        py: Python<'py>,
        previous: PyRef<'_, Frame>,
    ) -> PyResult<Bound<'py, PyList>> {
        let changes = self
            .inner
            .diff(&previous.inner)
            .into_iter()
            .map(|change| {
                let range = PyRange::new(
                    py,
                    change.columns.start as isize,
                    change.columns.end as isize,
                )?;
                PyTuple::new(
                    py,
                    [change.row.into_pyobject(py)?.into_any(), range.into_any()],
                )
            })
            .collect::<PyResult<Vec<_>>>()?;
        PyList::new(py, changes)
    }

    fn __str__(&self) -> String {
        self.inner.plain()
    }

    fn __len__(&self) -> usize {
        self.inner.height()
    }

    fn __repr__(&self) -> String {
        format!(
            "<Frame {}x{} runs={}>",
            self.inner.width(),
            self.inner.height(),
            self.inner.run_count()
        )
    }
}

/// A Python console's colour system and `no_color`.
fn console_colors(console: &Bound<'_, PyAny>) -> PyResult<(Option<rich::ColorSystem>, bool)> {
    let system: Option<String> = console.getattr("color_system")?.extract()?;
    let no_color: bool = console.getattr("no_color")?.is_truthy()?;
    Ok((color_system(system.as_deref())?, no_color))
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // `Frame` is Rich's traceback frame in the flat native module.
    m.add("RenderFrame", m.py().get_type::<Frame>())?;
    Ok(())
}

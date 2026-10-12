//! `rs_rich.ext.surface`: `rich_ext::surface` (0.0.19 workstream 4),
//! positioned drawing that writes only the cells that changed.
//!
//! `Surface.draw(fn)` calls `fn` with a `Drawing`; what it renders is kept
//! until `fn` returns, then drawn on the Rust surface in one frame, and the
//! bytes are written to the surface's file (`sys.stdout` by default, looked
//! up at each write, so a redirected stdout gets them).

use std::cell::RefCell;

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use rich::{Console as CoreConsole, Segment as CoreSegment};
use rich_ext::surface::{Rect, Surface as CoreSurface, Viewport};

use super::terminal::color_system;
use crate::tui::{node::draw_object, rect_arg, PyRect};

/// What a `Drawing` was asked to do, in order.
enum Op {
    Lines(Rect, Vec<Vec<CoreSegment>>),
    Clear(Rect),
    Cursor(u16, u16),
}

/// `Surface(width=None, height=None, *, inline=None, file=None,
/// color_system="truecolor", synchronized=False)`: a drawing handle on the
/// terminal. Without a size it takes the terminal's; `inline=rows` draws
/// that many rows below the cursor instead of on the alternate screen.
#[pyclass(name = "Surface", module = "rs_rich.ext.surface", unsendable)]
pub(crate) struct Surface {
    inner: Option<CoreSurface<Vec<u8>>>,
    console: CoreConsole,
    file: Option<Py<PyAny>>,
}

impl Surface {
    fn inner(&mut self) -> PyResult<&mut CoreSurface<Vec<u8>>> {
        self.inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("the surface is finished"))
    }

    /// Write what the Rust surface produced to the file.
    fn flush(&mut self, py: Python<'_>) -> PyResult<()> {
        let Some(inner) = self.inner.as_mut() else {
            return Ok(());
        };
        let bytes = std::mem::take(inner.writer_mut());
        if bytes.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let file = match &self.file {
            Some(file) => file.bind(py).clone(),
            None => py.import("sys")?.getattr("stdout")?,
        };
        file.call_method1("write", (text,))?;
        if file.hasattr("flush")? {
            file.call_method0("flush")?;
        }
        Ok(())
    }
}

#[pymethods]
impl Surface {
    #[new]
    #[pyo3(signature = (width=None, height=None, *, inline=None, file=None, color_system="truecolor", synchronized=false))]
    fn new(
        width: Option<u16>,
        height: Option<u16>,
        inline: Option<u16>,
        file: Option<Py<PyAny>>,
        color_system: Option<&str>,
        synchronized: bool,
    ) -> PyResult<Surface> {
        let system = self::color_system(color_system)?;
        let size = CoreConsole::new().size();
        let width = width.unwrap_or(size.width.clamp(1, u16::MAX as usize) as u16);
        let height = height.unwrap_or(size.height.clamp(1, u16::MAX as usize) as u16);
        let build = || {
            CoreConsole::builder()
                .width(width.max(1) as usize)
                .height(height.max(1) as usize)
                .force_terminal(true)
                .color_system(system)
                .build()
        };
        let viewport = match inline {
            Some(rows) => Viewport::Inline(rows),
            None => Viewport::Fullscreen,
        };
        let inner = CoreSurface::new(Vec::new(), width, height)
            .viewport(viewport)
            .console(build())
            .synchronized(synchronized);
        Ok(Surface {
            inner: Some(inner),
            console: build(),
            file,
        })
    }

    /// The whole surface, as a `Rect` at `(0, 0)`.
    #[getter]
    fn area(&mut self) -> PyResult<PyRect> {
        Ok(PyRect(self.inner()?.area()))
    }

    /// Call `fn(drawing)`, then write what changed in the rectangles it drew.
    fn draw(&mut self, py: Python<'_>, f: &Bound<'_, PyAny>) -> PyResult<()> {
        let area = self.inner()?.area();
        let drawing = Py::new(
            py,
            Drawing {
                area,
                console: self.console.clone(),
                ops: RefCell::new(Vec::new()),
            },
        )?;
        f.call1((drawing.clone_ref(py),))?;
        let ops = std::mem::take(&mut *drawing.borrow(py).ops.borrow_mut());
        self.inner()?
            .draw(|s| {
                for op in ops {
                    match op {
                        Op::Lines(rect, lines) => s.write_lines(rect, lines),
                        Op::Clear(rect) => s.clear(rect),
                        Op::Cursor(x, y) => s.set_cursor(x, y),
                    }
                }
            })
            .map_err(|e| pyo3::exceptions::PyOSError::new_err(e.to_string()))?;
        self.flush(py)
    }

    /// The terminal changed size: keep what was drawn, and write everything
    /// on the next `draw`.
    fn resize(&mut self, width: u16, height: u16) -> PyResult<()> {
        self.inner()?.resize(width, height);
        Ok(())
    }

    /// Blank the surface; the next `draw` writes everything.
    fn clear(&mut self) -> PyResult<()> {
        self.inner()?.clear();
        Ok(())
    }

    /// Each row as plain text: what the surface shows.
    fn plain(&mut self) -> PyResult<Vec<String>> {
        Ok(self.inner()?.screen().plain())
    }

    /// Give the terminal back: leave the alternate screen (or go below the
    /// inline region), reset styles and show the cursor.
    fn finish(&mut self, py: Python<'_>) -> PyResult<()> {
        if let Some(inner) = self.inner.as_mut() {
            inner
                .finish()
                .map_err(|e| pyo3::exceptions::PyOSError::new_err(e.to_string()))?;
        }
        self.flush(py)?;
        self.inner = None;
        Ok(())
    }

    fn __enter__(slf: Py<Self>) -> Py<Self> {
        slf
    }

    #[pyo3(signature = (*_args))]
    fn __exit__(
        &mut self,
        py: Python<'_>,
        _args: &Bound<'_, pyo3::types::PyTuple>,
    ) -> PyResult<bool> {
        self.finish(py)?;
        Ok(false)
    }
}

impl Drop for Surface {
    /// A surface dropped without `finish` still gives the terminal back.
    fn drop(&mut self) {
        if self.inner.is_some() {
            Python::attach(|py| {
                let _ = self.finish(py);
            });
        }
    }
}

/// One frame being drawn; see `Surface.draw`.
#[pyclass(name = "Drawing", module = "rs_rich.ext.surface", unsendable)]
pub(crate) struct Drawing {
    area: Rect,
    console: CoreConsole,
    ops: RefCell<Vec<Op>>,
}

#[pymethods]
impl Drawing {
    /// The whole surface, as a `Rect` at `(0, 0)`.
    #[getter]
    fn area(&self) -> PyRect {
        PyRect(self.area)
    }

    /// Render `renderable` (anything `print` takes, or a list of lines of
    /// `Segment`s) into `rect`, replacing what was there.
    fn render(&self, rect: &Bound<'_, PyAny>, renderable: &Bound<'_, PyAny>) -> PyResult<()> {
        let rect = rect_arg(rect)?;
        let lines = draw_object(renderable, &self.console, rect.width, rect.height.max(1))?;
        self.ops.borrow_mut().push(Op::Lines(rect, lines));
        Ok(())
    }

    /// Clear `rect` to blank cells.
    fn clear(&self, rect: &Bound<'_, PyAny>) -> PyResult<()> {
        self.ops.borrow_mut().push(Op::Clear(rect_arg(rect)?));
        Ok(())
    }

    /// Show the cursor at `x`, `y` once the frame is written.
    fn set_cursor(&self, x: u16, y: u16) {
        self.ops.borrow_mut().push(Op::Cursor(x, y));
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Surface>()?;
    m.add_class::<Drawing>()?;
    Ok(())
}

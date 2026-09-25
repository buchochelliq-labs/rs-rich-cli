//! `rs_rich.art` and `rs_rich.mermaid`: the `rich-art` crate (images in
//! every mode and fit, FIGlet, GIFs, image diffs) and `rich-mermaid` diagrams.
//!
//! Owner: the art area. Neither crate has a Rich counterpart, so the Python
//! API follows the Rust one: each builder method becomes a keyword argument,
//! each enum a lowercase name (the `rich` CLI's spelling). Everything renders
//! in Rust; this module only converts arguments.
//!
//! | Submodule | Python classes |
//! |---|---|
//! | [`image`] | `ArtImage`, `ImageArt`, `ImageOptions`, `RenderCapabilities`, `AsciiArt`, `BlockArt`, `BrailleArt`, `QuadrantArt`, `SixelArt` |
//! | [`figlet`] | `Figlet`, `FigletFont` |
//! | [`gif`] | `AnimatedArt`, `GifFrame`, `Stage` |
//! | [`diff`] | `image_diff`, `DiffSettings`, `DiffReport`, `DiffRegion` |
//! | [`mermaid`] | `Mermaid`, `MermaidFences`, `MermaidPlugin`, `MmdcOptions`, `Flowchart`, ... |

use std::io::Write;
use std::sync::{Arc, Mutex};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyByteArray, PyBytes, PyMemoryView, PyString};

use rich::color::ColorSystem;
use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;

use crate::renderable::PyRenderable;

mod diff;
mod figlet;
mod gif;
mod image;
pub(crate) mod mermaid;

create_exception!(_native, ArtError, PyException);
create_exception!(_native, ImageArtError, ArtError);
create_exception!(_native, ImageDecodeError, ArtError);
create_exception!(_native, FigletFontError, ArtError);
create_exception!(_native, ImageDiffError, ArtError);

/// A core renderable shared between a Python object and each print of it.
pub(crate) struct Shared<T: Renderable + Send + Sync>(pub(crate) Arc<T>);

impl<T: Renderable + Send + Sync> Renderable for Shared<T> {
    fn rich_render(&self, console: &CoreConsole, options: &CoreOptions) -> Vec<CoreSegment> {
        self.0.rich_render(console, options)
    }

    fn measure(&self, console: &CoreConsole, options: &CoreOptions) -> rich::measure::Measurement {
        self.0.measure(console, options)
    }
}

/// Raises its exception when rendered: how a core render that failed
/// reports a Python exception to the print that ran it (through the
/// foundation's `PyRenderable`, which keeps the first error of a render).
#[pyclass(module = "rs_rich.art")]
struct RenderFailure {
    error: Mutex<Option<PyErr>>,
}

#[pymethods]
impl RenderFailure {
    fn __rich_console__(&self, _console: Py<PyAny>, _options: Py<PyAny>) -> PyResult<()> {
        let error = self
            .error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        Err(error.unwrap_or_else(|| ArtError::new_err("render failed")))
    }
}

/// Report `error` from inside a core render: the enclosing print raises it.
pub(crate) fn fail_render(error: PyErr, console: &CoreConsole, options: &CoreOptions) {
    Python::attach(|py| {
        let failure = RenderFailure {
            error: Mutex::new(Some(error)),
        };
        match Py::new(py, failure) {
            Ok(object) => {
                PyRenderable::new(object.into_any()).rich_render(console, options);
            }
            Err(error) => error.write_unraisable(py, None),
        }
    });
}

/// An exception with a machine-readable `kind` attribute.
pub(crate) fn kinded<E: pyo3::PyTypeInfo>(py: Python<'_>, message: String, kind: &str) -> PyErr {
    let error = PyErr::from_type(py.get_type::<E>(), message);
    // Setting an attribute on a fresh exception instance cannot fail.
    let _ = error.value(py).setattr("kind", kind);
    error
}

/// Bytes from `bytes`, `bytearray` or `memoryview`, or `None` for anything else.
pub(crate) fn buffer_bytes(value: &Bound<'_, PyAny>) -> PyResult<Option<Vec<u8>>> {
    if let Ok(bytes) = value.cast::<PyBytes>() {
        return Ok(Some(bytes.as_bytes().to_vec()));
    }
    if let Ok(array) = value.cast::<PyByteArray>() {
        return Ok(Some(array.to_vec()));
    }
    if value.cast::<PyMemoryView>().is_ok() {
        let bytes = value.py().get_type::<PyBytes>().call1((value,))?;
        return Ok(Some(bytes.cast::<PyBytes>()?.as_bytes().to_vec()));
    }
    Ok(None)
}

/// A `str` or `os.PathLike` as a path, or `None` for anything else.
pub(crate) fn path_arg(value: &Bound<'_, PyAny>) -> PyResult<Option<std::path::PathBuf>> {
    if value.is_instance_of::<PyString>() || value.hasattr("__fspath__")? {
        let fspath = value.py().import("os")?.call_method1("fspath", (value,))?;
        if fspath.is_instance_of::<PyString>() {
            return Ok(Some(fspath.extract()?));
        }
        return Err(PyTypeError::new_err(
            "bytes paths are not supported; pass a str path",
        ));
    }
    Ok(None)
}

/// Read a file, raising `OSError` (`FileNotFoundError`, ...) with `errno`,
/// `strerror` and `filename`, as Python's own `open` does.
pub(crate) fn read_file(path: &std::path::Path) -> PyResult<Vec<u8>> {
    std::fs::read(path).map_err(|error| {
        Python::attach(|py| {
            let Some(code) = error.raw_os_error() else {
                return PyErr::from(error);
            };
            let raised = (|| {
                let class = PyErr::from(std::io::Error::from(error.kind())).get_type(py);
                let strerror = py.import("os")?.call_method1("strerror", (code,))?;
                let filename = path.to_string_lossy().into_owned();
                Ok::<_, PyErr>(PyErr::from_value(class.call1((code, strerror, filename))?))
            })();
            raised.unwrap_or_else(|failure| failure)
        })
    })
}

/// A lowercase name with `_` and `-` treated alike.
pub(crate) fn normalized(name: &str) -> String {
    name.trim().to_ascii_lowercase().replace('_', "-")
}

/// Raise `ValueError` naming what was expected.
pub(crate) fn bad_choice(what: &str, value: &str, expected: &str) -> PyErr {
    PyValueError::new_err(format!(
        "invalid {what} {}; expected {expected}",
        repr_str(value)
    ))
}

/// Python's `repr` of a string, for messages and reprs.
pub(crate) fn repr_str(value: &str) -> String {
    Python::attach(|py| {
        PyString::new(py, value)
            .repr()
            .map(|r| r.to_string())
            .unwrap_or_else(|_| format!("{value:?}"))
    })
}

/// Python's `repr` of a float.
pub(crate) fn repr_float(value: f64) -> String {
    Python::attach(|py| {
        pyo3::types::PyFloat::new(py, value)
            .repr()
            .map(|r| r.to_string())
            .unwrap_or_else(|_| format!("{value:?}"))
    })
}

/// Python's `repr` of an optional string.
pub(crate) fn repr_opt(value: Option<&str>) -> String {
    value.map_or_else(|| "None".to_string(), repr_str)
}

/// A core console with a Python `Console`'s settings, for what renders
/// outside a print (playing a GIF, reading capabilities). Mirrors the
/// console a print renders with.
pub(crate) fn core_console(console: &Bound<'_, PyAny>) -> PyResult<CoreConsole> {
    let color_system = match console
        .getattr("color_system")?
        .extract::<Option<String>>()?
    {
        None => None,
        Some(name) => Some(match name.as_str() {
            "standard" => ColorSystem::Standard,
            "256" => ColorSystem::EightBit,
            "truecolor" => ColorSystem::Truecolor,
            "windows" => ColorSystem::Windows,
            other => {
                return Err(bad_choice(
                    "color system",
                    other,
                    "a Console's color system",
                ))
            }
        }),
    };
    Ok(CoreConsole::builder()
        .force_terminal(console.getattr("is_terminal")?.extract()?)
        .color_system(color_system)
        .width(console.getattr("width")?.extract()?)
        .height(console.getattr("height")?.extract()?)
        .no_color(console.getattr("no_color")?.extract()?)
        .legacy_windows(console.getattr("legacy_windows")?.extract()?)
        .safe_box(console.getattr("safe_box")?.extract()?)
        .build())
}

/// A new `rs_rich.console.Console()`, for methods whose console is optional.
pub(crate) fn default_console(py: Python<'_>) -> PyResult<Bound<'_, PyAny>> {
    py.import("rs_rich.console")?.getattr("Console")?.call0()
}

/// Unwinds out of a playback loop once its writer has failed (see
/// [`FileWriter`]); raised with `resume_unwind`, so no panic message prints.
struct StopPlayback;

/// Writes to a Python console's file from code running without the GIL.
///
/// Text is decoded as UTF-8 at character boundaries. Each write checks for
/// signals: core's playback loops ignore write errors, so on Ctrl-C (or a
/// failing `file.write`) the writer keeps the exception and unwinds out of
/// the loop; [`play_to_console`] catches that and raises the exception.
pub(crate) struct FileWriter {
    file: Py<PyAny>,
    pending: Vec<u8>,
    error: Option<PyErr>,
}

impl FileWriter {
    fn new(file: Py<PyAny>) -> FileWriter {
        FileWriter {
            file,
            pending: Vec::new(),
            error: None,
        }
    }

    /// Hand the complete characters written so far to `file.write`.
    fn send(&mut self, py: Python<'_>) -> PyResult<()> {
        let valid = match std::str::from_utf8(&self.pending) {
            Ok(text) => text.len(),
            Err(error) => error.valid_up_to(),
        };
        if valid == 0 {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.pending[..valid]).into_owned();
        self.pending.drain(..valid);
        self.file.bind(py).call_method1("write", (text,))?;
        Ok(())
    }

    fn stop(&mut self, error: PyErr) -> ! {
        self.error = Some(error);
        std::panic::resume_unwind(Box::new(StopPlayback))
    }
}

impl Write for FileWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.pending.extend_from_slice(buf);
        let result = Python::attach(|py| {
            py.check_signals()?;
            self.send(py)
        });
        match result {
            Ok(()) => Ok(buf.len()),
            Err(error) => self.stop(error),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let result = Python::attach(|py| {
            self.send(py)?;
            let file = self.file.bind(py);
            if file.hasattr("flush")? {
                file.call_method0("flush")?;
            }
            Ok(())
        });
        match result {
            Ok(()) => Ok(()),
            Err(error) => self.stop(error),
        }
    }
}

/// Run a core playback loop (`AnimatedArt::play`, `Stage::play`) against a
/// Python console (default: a new `Console()`), without the GIL, writing to
/// the console's file. Ctrl-C stops it with `KeyboardInterrupt`, and the
/// cursor is shown again.
pub(crate) fn play_to_console<F>(
    py: Python<'_>,
    console: Option<&Bound<'_, PyAny>>,
    play: F,
) -> PyResult<()>
where
    F: FnOnce(CoreConsole, &mut FileWriter) -> std::io::Result<()> + Send,
{
    let console = match console {
        Some(console) if !console.is_none() => console.clone(),
        _ => default_console(py)?,
    };
    let core = core_console(&console)?;
    let terminal = core.is_terminal();
    let file = console.getattr("file")?;
    let mut writer = FileWriter::new(file.clone().unbind());
    let outcome = py.detach(|| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| play(core, &mut writer)))
    });
    match outcome {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(PyErr::from(error)),
        Err(payload) if payload.is::<StopPlayback>() => {
            if terminal {
                // The loop stopped with the cursor hidden; show it again.
                let _ = file.call_method1("write", (rich_art::gif::show_cursor_sequence(),));
                if file.hasattr("flush").unwrap_or(false) {
                    let _ = file.call_method0("flush");
                }
            }
            Err(writer
                .error
                .take()
                .unwrap_or_else(|| ArtError::new_err("playback stopped")))
        }
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let py = m.py();
    m.add("ArtError", py.get_type::<ArtError>())?;
    m.add("ImageArtError", py.get_type::<ImageArtError>())?;
    m.add("ImageDecodeError", py.get_type::<ImageDecodeError>())?;
    m.add("FigletFontError", py.get_type::<FigletFontError>())?;
    m.add("ImageDiffError", py.get_type::<ImageDiffError>())?;
    image::register(m)?;
    figlet::register(m)?;
    gif::register(m)?;
    diff::register(m)?;
    mermaid::register(m)?;
    Ok(())
}

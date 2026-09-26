//! Animated GIFs: `AnimatedArt`, its frames (`GifFrame`), and `Stage` for
//! several at once.

use std::sync::Arc;
use std::time::Duration;

use pyo3::exceptions::{PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyString;

use rich::protocol::Renderable;
use rich_art::gif::{AnimatedArt as CoreAnimated, GifFrame as CoreFrame, Repeat};
use rich_art::stage::{Stage as CoreStage, Until};

use super::image::{
    color_distance, color_distance_name, color_mode, color_mode_name, decode_error, dither,
    dither_name, AsciiArt,
};
use super::{bad_choice, buffer_bytes, path_arg, play_to_console, read_file, Shared};
use crate::renderable::{self, AsRenderable};

fn repeat(value: &Bound<'_, PyAny>) -> PyResult<Repeat> {
    if let Ok(name) = value.cast::<PyString>() {
        return match name.to_cow()?.as_ref() {
            "once" => Ok(Repeat::Once),
            "forever" => Ok(Repeat::Forever),
            other => Err(bad_choice(
                "repeat",
                other,
                "a number of passes, 'once' or 'forever'",
            )),
        };
    }
    if value.is_none() {
        return Ok(Repeat::Forever);
    }
    match value.extract::<usize>()? {
        1 => Ok(Repeat::Once),
        times => Ok(Repeat::Times(times)),
    }
}

fn seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

/// An animated GIF (`rich_art::AnimatedArt`). It prints as its first
/// frame; `play()` animates it in place, and `frames()` gives each frame
/// and its delay, for driving a `Live` display yourself.
#[pyclass(name = "AnimatedArt", module = "rs_rich.art", frozen)]
pub(crate) struct AnimatedArt {
    inner: Arc<CoreAnimated>,
    repeat: Repeat,
    blocks: bool,
    color: bool,
    color_mode: rich_art::ImageColorMode,
    dither: rich_art::Dither,
    distance: rich_art::ColorDistance,
}

impl AsRenderable for AnimatedArt {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

impl AnimatedArt {
    fn frame_at(&self, index: usize) -> Option<GifFrame> {
        self.inner.render_frame(index).map(|frame| GifFrame {
            inner: Arc::new(frame),
            index,
        })
    }
}

#[pymethods]
impl AnimatedArt {
    /// The most bytes of decoded frames a GIF may expand to.
    #[classattr]
    const MAX_DECODED_BYTES: usize = CoreAnimated::MAX_DECODED_BYTES;

    /// `AnimatedArt(source, ...)`: a GIF file's path, or its bytes.
    #[new]
    #[pyo3(signature = (
        source, *, width=None, height=None, ramp=None, invert=false, color=false, blocks=false,
        color_mode="truecolor", dither=None, color_distance="rgb", repeat=None, max_fps=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        py: Python<'_>,
        source: &Bound<'_, PyAny>,
        width: Option<usize>,
        height: Option<usize>,
        ramp: Option<String>,
        invert: bool,
        color: bool,
        blocks: bool,
        color_mode: &str,
        dither: Option<&str>,
        color_distance: &str,
        repeat: Option<&Bound<'_, PyAny>>,
        max_fps: Option<f64>,
    ) -> PyResult<Self> {
        let bytes = match path_arg(source)? {
            Some(path) => read_file(&path)?,
            None => buffer_bytes(source)?.ok_or_else(|| {
                PyTypeError::new_err("expected a GIF: a path or the file's bytes")
            })?,
        };
        let repeat = match repeat {
            Some(value) => self::repeat(value)?,
            None => Repeat::Once,
        };
        let color_mode = self::color_mode(color_mode)?;
        let dither = self::dither(dither)?;
        let distance = self::color_distance(color_distance)?;
        let decoded = py
            .detach(|| CoreAnimated::from_bytes(&bytes))
            .map_err(|error| decode_error(&error))?;
        let mut art = decoded
            .invert(invert)
            .color(color)
            .blocks(blocks)
            .color_mode(color_mode)
            .dither(dither)
            .color_distance(distance)
            .repeat(repeat);
        if let Some(width) = width {
            art = art.width(width);
        }
        if let Some(height) = height {
            art = art.height(height);
        }
        if let Some(ramp) = ramp {
            art = art.ramp(ramp);
        }
        if let Some(fps) = max_fps {
            art = art.max_fps(fps);
        }
        Ok(AnimatedArt {
            inner: Arc::new(art),
            repeat,
            blocks,
            color,
            color_mode,
            dither,
            distance,
        })
    }

    /// Number of decoded frames.
    #[getter]
    fn frame_count(&self) -> usize {
        self.inner.frame_count()
    }

    fn __len__(&self) -> usize {
        self.inner.frame_count()
    }

    /// Seconds one pass takes, after any `max_fps` cap.
    #[getter]
    fn duration(&self) -> f64 {
        seconds(self.inner.duration())
    }

    /// How many passes `play()` makes: a number, or `"forever"`.
    #[getter]
    fn repeat(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        Ok(match self.repeat {
            Repeat::Once => 1usize.into_pyobject(py)?.into_any().unbind(),
            Repeat::Times(n) => n.into_pyobject(py)?.into_any().unbind(),
            Repeat::Forever => "forever".into_pyobject(py)?.into_any().unbind(),
        })
    }

    #[getter]
    fn color(&self) -> bool {
        self.color
    }

    #[getter]
    fn blocks(&self) -> bool {
        self.blocks
    }

    #[getter]
    fn color_mode(&self) -> &'static str {
        color_mode_name(self.color_mode)
    }

    #[getter]
    fn dither(&self) -> Option<&'static str> {
        dither_name(self.dither)
    }

    #[getter]
    fn color_distance(&self) -> &'static str {
        color_distance_name(self.distance)
    }

    /// Seconds frame `index` shows for (after any cap), or `None` past the
    /// last frame.
    fn frame_delay(&self, index: usize) -> Option<f64> {
        self.inner.frame_delay(index).map(seconds)
    }

    /// Frame `index` as `AsciiArt` (whatever `blocks` says), or `None`.
    fn frame(&self, index: usize) -> Option<AsciiArt> {
        self.inner.frame(index).map(|art| AsciiArt {
            inner: Arc::new(art),
        })
    }

    /// Frame `index` as it plays: half-blocks when `blocks=True` on a
    /// colour terminal, else ASCII. `None` past the last frame.
    fn render_frame(&self, index: usize) -> Option<GifFrame> {
        self.frame_at(index)
    }

    /// Every frame of one pass, with the seconds it shows for:
    /// `[(GifFrame, delay), ...]`.
    fn frames(&self) -> Vec<(GifFrame, f64)> {
        (0..self.inner.frame_count())
            .filter_map(|index| {
                let delay = self.inner.frame_delay(index)?;
                Some((self.frame_at(index)?, seconds(delay)))
            })
            .collect()
    }

    /// Play in place on `console` (default: a new `Console()`), blocking
    /// until done; on a file, print the first frame once. Ctrl-C stops it.
    #[pyo3(signature = (console=None))]
    fn play(&self, py: Python<'_>, console: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let art = Arc::clone(&self.inner);
        play_to_console(py, console, move |core, writer| art.play(core, writer))
    }

    fn __repr__(&self) -> String {
        format!("<AnimatedArt frames={}>", self.inner.frame_count())
    }
}

/// One frame of an `AnimatedArt`, as it plays (`rich_art::gif::GifFrame`).
#[pyclass(name = "GifFrame", module = "rs_rich.art", frozen)]
pub(crate) struct GifFrame {
    inner: Arc<CoreFrame>,
    #[pyo3(get)]
    index: usize,
}

impl AsRenderable for GifFrame {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::clone(&self.inner))))
    }
}

#[pymethods]
impl GifFrame {
    fn __repr__(&self) -> String {
        format!("<GifFrame {}>", self.index)
    }
}

/// Several `AnimatedArt`s playing side by side, each on its own clock
/// (`rich_art::Stage`).
#[pyclass(name = "Stage", module = "rs_rich.art")]
pub(crate) struct Stage {
    items: Vec<Arc<CoreAnimated>>,
    #[pyo3(get, set)]
    gap: usize,
    until: Option<f64>,
}

#[pymethods]
impl Stage {
    /// `Stage(*arts, gap=2, until=None)`: `until` is a number of seconds to
    /// play for; `None` plays until every animation has finished.
    #[new]
    #[pyo3(signature = (*arts, gap=2, until=None))]
    fn new(arts: Vec<PyRef<'_, AnimatedArt>>, gap: usize, until: Option<f64>) -> PyResult<Self> {
        check_until(until)?;
        Ok(Stage {
            items: arts.iter().map(|art| Arc::clone(&art.inner)).collect(),
            gap,
            until,
        })
    }

    /// Add an animation to the right of the others (`Stage::with`).
    fn add<'py>(mut slf: PyRefMut<'py, Self>, art: PyRef<'_, AnimatedArt>) -> PyRefMut<'py, Self> {
        slf.items.push(Arc::clone(&art.inner));
        slf
    }

    #[getter]
    fn get_until(&self) -> Option<f64> {
        self.until
    }

    #[setter]
    fn set_until(&mut self, until: Option<f64>) -> PyResult<()> {
        check_until(until)?;
        self.until = until;
        Ok(())
    }

    fn __len__(&self) -> usize {
        self.items.len()
    }

    /// Play every animation at once on `console` (default: a new
    /// `Console()`); on a file, print the first composed frame once.
    #[pyo3(signature = (console=None))]
    fn play(&self, py: Python<'_>, console: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        let mut stage = CoreStage::new().gap(self.gap);
        for art in &self.items {
            stage = stage.with((**art).clone());
        }
        if let Some(limit) = self.until {
            stage = stage.until(Until::Elapsed(Duration::from_secs_f64(limit)));
        }
        play_to_console(py, console, move |core, writer| stage.play(core, writer))
    }

    fn __repr__(&self) -> String {
        format!("<Stage animations={}>", self.items.len())
    }
}

fn check_until(until: Option<f64>) -> PyResult<()> {
    match until {
        Some(limit) if !(limit.is_finite() && limit >= 0.0) => Err(PyValueError::new_err(
            "until must be a finite, non-negative number of seconds",
        )),
        _ => Ok(()),
    }
}

/// The control sequence that shows the cursor again, for a signal handler
/// that interrupted playback (`rich_art::gif::show_cursor_sequence`).
#[pyfunction]
fn show_cursor_sequence() -> String {
    rich_art::gif::show_cursor_sequence()
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    renderable::add_renderable_class::<AnimatedArt>(m)?;
    renderable::add_renderable_class::<GifFrame>(m)?;
    m.add_class::<Stage>()?;
    m.add_function(pyo3::wrap_pyfunction!(show_cursor_sequence, m)?)?;
    Ok(())
}

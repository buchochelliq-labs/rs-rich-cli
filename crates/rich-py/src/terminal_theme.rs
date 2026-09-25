//! `rich.terminal_theme`: the palettes `export_html` and `export_svg` use.
//!
//! Owner: the foundation.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use rich::color::ColorTriplet;
use rich::terminal_theme::{self as core_themes, TerminalTheme as CoreTheme};

type Rgb = (u8, u8, u8);

fn triplet((red, green, blue): Rgb) -> ColorTriplet {
    ColorTriplet { red, green, blue }
}

/// `rich.terminal_theme.TerminalTheme(background, foreground, normal,
/// bright=None)`: colours as `(r, g, b)` tuples, 8 normal and 8 bright ANSI
/// colours (`bright` defaults to `normal`).
#[pyclass(name = "TerminalTheme", module = "rs_rich.terminal_theme", frozen)]
pub(crate) struct TerminalTheme {
    pub(crate) inner: CoreTheme,
}

#[pymethods]
impl TerminalTheme {
    #[new]
    #[pyo3(signature = (background, foreground, normal, bright=None))]
    fn new(
        background: Rgb,
        foreground: Rgb,
        normal: Vec<Rgb>,
        bright: Option<Vec<Rgb>>,
    ) -> PyResult<Self> {
        let bright = bright.unwrap_or_else(|| normal.clone());
        if normal.len() != 8 || bright.len() != 8 {
            return Err(PyValueError::new_err(
                "normal and bright must each hold 8 (r, g, b) colours",
            ));
        }
        let mut ansi = [triplet((0, 0, 0)); 16];
        for (slot, colour) in ansi.iter_mut().zip(normal.into_iter().chain(bright)) {
            *slot = triplet(colour);
        }
        Ok(TerminalTheme {
            inner: CoreTheme {
                background: triplet(background),
                foreground: triplet(foreground),
                ansi,
            },
        })
    }

    #[getter]
    fn background_color(&self) -> Rgb {
        let c = self.inner.background;
        (c.red, c.green, c.blue)
    }

    #[getter]
    fn foreground_color(&self) -> Rgb {
        let c = self.inner.foreground;
        (c.red, c.green, c.blue)
    }

    /// The 16 ANSI colours, normal then bright.
    #[getter]
    fn ansi_colors(&self) -> Vec<Rgb> {
        self.inner
            .ansi
            .iter()
            .map(|c| (c.red, c.green, c.blue))
            .collect()
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<TerminalTheme>()?;
    for (name, theme) in [
        (
            "DEFAULT_TERMINAL_THEME",
            core_themes::DEFAULT_TERMINAL_THEME,
        ),
        ("SVG_EXPORT_THEME", core_themes::SVG_EXPORT_THEME),
        ("MONOKAI", core_themes::MONOKAI),
        ("DIMMED_MONOKAI", core_themes::DIMMED_MONOKAI),
        ("NIGHT_OWLISH", core_themes::NIGHT_OWLISH),
    ] {
        m.add(name, TerminalTheme { inner: theme })?;
    }
    Ok(())
}

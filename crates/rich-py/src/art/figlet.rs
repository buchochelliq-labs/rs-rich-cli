//! FIGlet banners: `Figlet`, `FigletFont` and `figlet_render`.

use std::sync::Arc;

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

use rich::protocol::Renderable;
use rich::style::Style as CoreStyle;
use rich_art::figlet::{FigletFont as CoreFont, FontError, Justify};

use super::{bad_choice, path_arg, read_file, repr_str, FigletFontError, Shared};
use crate::renderable::{self, AsRenderable};
use crate::style::resolved_style;

fn justify(name: &str) -> PyResult<Justify> {
    Ok(match name {
        "left" | "default" => Justify::Left,
        "center" => Justify::Center,
        "right" => Justify::Right,
        other => return Err(bad_choice("justify", other, "left, center or right")),
    })
}

fn justify_name(justify: Justify) -> &'static str {
    match justify {
        Justify::Left => "left",
        Justify::Center => "center",
        Justify::Right => "right",
    }
}

fn font_error(error: FontError) -> PyErr {
    FigletFontError::new_err(error.to_string())
}

/// A parsed FIGfont (`.flf`); `FigletFont()` is the bundled `standard` font.
#[pyclass(name = "FigletFont", module = "rs_rich.art", frozen)]
pub(crate) struct FigletFont {
    inner: Arc<CoreFont>,
}

#[pymethods]
impl FigletFont {
    /// Parse a font from the text of a `.flf` file, or the bundled
    /// `standard` font when `source` is `None`.
    #[new]
    #[pyo3(signature = (source=None))]
    fn new(source: Option<&str>) -> PyResult<Self> {
        let font = match source {
            Some(source) => CoreFont::parse(source).map_err(font_error)?,
            None => CoreFont::standard(),
        };
        Ok(FigletFont {
            inner: Arc::new(font),
        })
    }

    /// Parse a font from the text of a `.flf` file.
    #[staticmethod]
    fn parse(source: &str) -> PyResult<Self> {
        FigletFont::new(Some(source))
    }

    /// The bundled `standard` font.
    #[staticmethod]
    fn standard() -> Self {
        FigletFont {
            inner: Arc::new(CoreFont::standard()),
        }
    }

    /// Read and parse a `.flf` file.
    #[staticmethod]
    fn from_path(path: &Bound<'_, PyAny>) -> PyResult<Self> {
        let path = path_arg(path)?.ok_or_else(|| PyTypeError::new_err("expected a path"))?;
        let bytes = read_file(&path)?;
        // FIGfonts are Latin-1 as often as UTF-8; read either.
        let source = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => error.into_bytes().iter().map(|&b| char::from(b)).collect(),
        };
        FigletFont::new(Some(&source))
    }

    /// Rows in every character.
    #[getter]
    fn height(&self) -> usize {
        self.inner.height()
    }

    /// The font's hard blank (drawn as a space).
    #[getter]
    fn hard_blank(&self) -> char {
        self.inner.hard_blank()
    }

    fn __repr__(&self) -> String {
        format!("<FigletFont height={}>", self.inner.height())
    }
}

/// A FIGlet banner (`rich_art::Figlet`): lays out to the console width (or
/// `width`), wrapping onto more banner rows as `figlet` does.
#[pyclass(name = "Figlet", module = "rs_rich.art", frozen)]
pub(crate) struct Figlet {
    text: String,
    font: Arc<CoreFont>,
    justify: Justify,
    style: Option<CoreStyle>,
    width: Option<usize>,
}

impl Figlet {
    fn build(&self) -> rich_art::Figlet {
        let mut banner = rich_art::Figlet::new(self.text.clone())
            .font((*self.font).clone())
            .justify(self.justify);
        if let Some(style) = &self.style {
            banner = banner.style(style.clone());
        }
        if let Some(width) = self.width {
            banner = banner.width(width);
        }
        banner
    }
}

impl AsRenderable for Figlet {
    fn to_renderable(&self, _py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        Ok(Box::new(Shared(Arc::new(self.build()))))
    }
}

#[pymethods]
impl Figlet {
    #[new]
    #[pyo3(signature = (text, *, font=None, justify="left", style=None, width=None))]
    fn new(
        text: String,
        font: Option<PyRef<'_, FigletFont>>,
        justify: &str,
        style: Option<&Bound<'_, PyAny>>,
        width: Option<usize>,
    ) -> PyResult<Self> {
        Ok(Figlet {
            text,
            font: match font {
                Some(font) => Arc::clone(&font.inner),
                None => Arc::new(CoreFont::standard()),
            },
            justify: self::justify(justify)?,
            style: resolved_style(style)?,
            width,
        })
    }

    /// The banner as plain text (`figlet`'s output) for `width` columns:
    /// by default the banner's own width, else 80.
    #[pyo3(signature = (width=None))]
    fn to_text(&self, width: Option<usize>) -> String {
        self.build().to_text(width.or(self.width).unwrap_or(80))
    }

    #[getter]
    fn text(&self) -> &str {
        &self.text
    }

    #[getter]
    fn font(&self) -> FigletFont {
        FigletFont {
            inner: Arc::clone(&self.font),
        }
    }

    #[getter]
    fn justify(&self) -> &'static str {
        justify_name(self.justify)
    }

    #[getter]
    fn width(&self) -> Option<usize> {
        self.width
    }

    fn __repr__(&self) -> String {
        format!("<Figlet {}>", repr_str(&self.text))
    }
}

/// `rich_art::figlet::render`: `text` as a FIGlet banner, as plain text.
#[pyfunction]
#[pyo3(signature = (text, font=None, width=80, justify="left"))]
fn figlet_render(
    text: &str,
    font: Option<PyRef<'_, FigletFont>>,
    width: usize,
    justify: &str,
) -> PyResult<String> {
    let justify = self::justify(justify)?;
    Ok(match font {
        Some(font) => rich_art::figlet::render(text, &font.inner, width, justify),
        None => rich_art::figlet::render(text, &CoreFont::standard(), width, justify),
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<FigletFont>()?;
    renderable::add_renderable_class::<Figlet>(m)?;
    m.add_function(pyo3::wrap_pyfunction!(figlet_render, m)?)?;
    m.add("STANDARD_FONT", rich_art::figlet::STANDARD_FONT)?;
    Ok(())
}

//! `rich.emoji`: `Emoji`, `NoEmoji` and emoji-code replacement.

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyType;

use rich::console::{Console as CoreConsole, ConsoleOptions as CoreOptions};
use rich::emoji::EmojiVariant;
use rich::measure::Measurement as CoreMeasurement;
use rich::protocol::Renderable;
use rich::segment::Segment as CoreSegment;
use rich::StyleType;

use crate::renderable::{self, AsRenderable};

create_exception!(_native, NoEmoji, PyException);

/// Rich's `EmojiVariant` from its name; `None` is no variant.
pub(crate) fn variant(name: Option<&str>) -> PyResult<Option<EmojiVariant>> {
    match name {
        None => Ok(None),
        Some(name) => EmojiVariant::parse(name).map(Some).ok_or_else(|| {
            PyValueError::new_err(format!(
                "invalid emoji variant {name:?}; expected 'emoji' or 'text'"
            ))
        }),
    }
}

/// `_emoji_replace(text, default_variant)`: replace `:code:`s in `text`.
pub(crate) fn emoji_replace(text: &str, default_variant: Option<&str>) -> PyResult<String> {
    Ok(rich::emoji::replace_with_variant(
        text,
        variant(default_variant)?,
    ))
}

/// The glyph Rich's `EMOJI[name]` holds, if any. The table's names are all
/// lower case and carry no variant suffix, so a name that is not in that
/// form is missing there, even where core's case-folding lookup finds it.
fn lookup(name: &str) -> Option<String> {
    let plain_name = name == name.to_lowercase()
        && !name.ends_with("-emoji")
        && !name.ends_with("-text")
        && !name.contains(':')
        && !name.chars().any(char::is_whitespace);
    if !plain_name || name.is_empty() {
        return None;
    }
    let code = format!(":{name}:");
    let replaced = rich::emoji::replace(&code);
    (replaced != code).then_some(replaced)
}

/// `rich.emoji.Emoji(name, style="none", variant=None)`: one emoji.
#[pyclass(name = "Emoji", module = "rs_rich.emoji", skip_from_py_object)]
pub(crate) struct Emoji {
    #[pyo3(get, set)]
    name: String,
    #[pyo3(get, set)]
    style: Py<PyAny>,
    #[pyo3(get, set)]
    variant: Option<String>,
    character: String,
}

/// An emoji as a core renderable: one segment in its style.
struct EmojiSegment {
    character: String,
    style: Option<StyleType>,
}

impl Renderable for EmojiSegment {
    fn rich_render(&self, console: &CoreConsole, _options: &CoreOptions) -> Vec<CoreSegment> {
        let style = self
            .style
            .as_ref()
            .map(|style| console.get_style(style).unwrap_or_default());
        vec![CoreSegment::new(&self.character, style)]
    }

    fn measure(&self, _console: &CoreConsole, options: &CoreOptions) -> CoreMeasurement {
        // Rich's `Emoji` has no `__rich_measure__`: it may take any width.
        CoreMeasurement::new(0, options.max_width)
    }
}

impl AsRenderable for Emoji {
    fn to_renderable(&self, py: Python<'_>) -> PyResult<Box<dyn Renderable>> {
        let style = crate::style::style_type(Some(self.style.bind(py)))?;
        Ok(Box::new(EmojiSegment {
            character: self.character.clone(),
            style,
        }))
    }
}

#[pymethods]
impl Emoji {
    #[classattr]
    #[allow(non_snake_case)]
    fn VARIANTS() -> std::collections::HashMap<&'static str, &'static str> {
        [("text", "\u{fe0e}"), ("emoji", "\u{fe0f}")]
            .into_iter()
            .collect()
    }

    #[new]
    #[pyo3(signature = (name, style=None, variant=None))]
    fn new(
        py: Python<'_>,
        name: String,
        style: Option<Py<PyAny>>,
        variant: Option<String>,
    ) -> PyResult<Self> {
        let Some(mut character) = lookup(&name) else {
            return Err(NoEmoji::new_err(format!(
                "No emoji called {}",
                super::python_repr(&name)
            )));
        };
        match variant.as_deref() {
            Some("text") => character.push('\u{fe0e}'),
            Some("emoji") => character.push('\u{fe0f}'),
            _ => {}
        }
        let style =
            style.unwrap_or_else(|| pyo3::types::PyString::new(py, "none").into_any().unbind());
        Ok(Emoji {
            name,
            style,
            variant,
            character,
        })
    }

    /// Replace emoji codes (`:smiley:`) in `text` with their characters.
    #[classmethod]
    fn replace(_cls: &Bound<'_, PyType>, text: &str) -> String {
        rich::emoji::replace(text)
    }

    fn __repr__(&self) -> String {
        format!("<emoji {}>", super::python_repr(&self.name))
    }

    fn __str__(&self) -> String {
        self.character.clone()
    }
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("NoEmoji", m.py().get_type::<NoEmoji>())?;
    renderable::add_renderable_class::<Emoji>(m)
}

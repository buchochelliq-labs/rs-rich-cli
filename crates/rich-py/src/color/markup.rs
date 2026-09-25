//! `rich.markup`: `render` and `Tag` (`escape` is the foundation's).

use pyo3::prelude::*;

use rich::{RichError, Text as CoreText};

use crate::errors::MarkupError;

/// A core markup error as Rich's `MarkupError`, with Rich's message.
pub(crate) fn markup_error(error: RichError) -> PyErr {
    match error {
        RichError::Markup(message) => MarkupError::new_err(message),
        other => MarkupError::new_err(other.to_string()),
    }
}

/// `rich.markup.render(markup, emoji=..., emoji_variant=...)` without the
/// base style: console markup to a core `Text`. Emoji codes are replaced
/// before the markup is parsed, as `Console.render_str` does.
pub(crate) fn render(markup: &str, emoji: bool, emoji_variant: Option<&str>) -> PyResult<CoreText> {
    let content = if emoji {
        super::emoji_replace(markup, emoji_variant)?
    } else {
        super::emoji::variant(emoji_variant)?;
        markup.to_string()
    };
    if !markup.contains('[') {
        return Ok(CoreText::new(content));
    }
    CoreText::from_markup(&content).map_err(markup_error)
}

/// `rich.markup.render(markup, style="", emoji=True, emoji_variant=None)`:
/// console markup to a `Text`. Exposed as `rs_rich.markup.render`.
#[pyfunction]
#[pyo3(signature = (markup, style=None, emoji=true, emoji_variant=None))]
fn render_markup(
    markup: &str,
    style: Option<&Bound<'_, PyAny>>,
    emoji: bool,
    emoji_variant: Option<&str>,
) -> PyResult<crate::text::Text> {
    let mut inner = render(markup, emoji, emoji_variant)?;
    inner.set_base_style(crate::text::base_style(style)?);
    Ok(crate::text::Text { inner })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(render_markup, m)?)
}

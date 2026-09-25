//! `rich.style`: the `Style` class and the `style=` argument conversions.
//!
//! Owner: the text/style area (with `text.rs`, `theme.rs` and `color.rs`).

use pyo3::exceptions::PyTypeError;
use pyo3::prelude::*;

use rich::{Style as CoreStyle, StyleType};

use crate::errors::StyleSyntaxError;

/// `rich.style.Style`: attributes, colours and a link.
#[pyclass(name = "Style", module = "rs_rich.style", frozen, skip_from_py_object)]
#[derive(Clone)]
pub(crate) struct Style {
    pub(crate) definition: String,
    pub(crate) inner: CoreStyle,
}

impl Style {
    /// Wrap a core style; its definition is core's normalised one.
    pub(crate) fn from_core(inner: CoreStyle) -> Style {
        Style {
            definition: if inner.is_null() {
                String::new()
            } else {
                inner.definition()
            },
            inner,
        }
    }
}

pub(crate) fn parse_style(definition: &str) -> PyResult<CoreStyle> {
    CoreStyle::parse(definition).map_err(|e| StyleSyntaxError::new_err(e.to_string()))
}

#[pymethods]
impl Style {
    #[new]
    #[pyo3(signature = (
        *, color=None, bgcolor=None, bold=None, dim=None, italic=None, underline=None,
        blink=None, reverse=None, conceal=None, strike=None, link=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn new(
        color: Option<&str>,
        bgcolor: Option<&str>,
        bold: Option<bool>,
        dim: Option<bool>,
        italic: Option<bool>,
        underline: Option<bool>,
        blink: Option<bool>,
        reverse: Option<bool>,
        conceal: Option<bool>,
        strike: Option<bool>,
        link: Option<&str>,
    ) -> PyResult<Self> {
        // Rich's own style grammar: `bold not italic red on blue link URL`.
        let mut words: Vec<String> = Vec::new();
        for (name, value) in [
            ("bold", bold),
            ("dim", dim),
            ("italic", italic),
            ("underline", underline),
            ("blink", blink),
            ("reverse", reverse),
            ("conceal", conceal),
            ("strike", strike),
        ] {
            match value {
                Some(true) => words.push(name.into()),
                Some(false) => words.push(format!("not {name}")),
                None => {}
            }
        }
        if let Some(color) = color {
            words.push(color.into());
        }
        if let Some(bgcolor) = bgcolor {
            words.push(format!("on {bgcolor}"));
        }
        if let Some(link) = link {
            words.push(format!("link {link}"));
        }
        let definition = words.join(" ");
        let inner = parse_style(&definition)?;
        Ok(Style { definition, inner })
    }

    /// `Style.parse("bold red on white")`.
    #[staticmethod]
    fn parse(definition: &str) -> PyResult<Self> {
        Ok(Style {
            inner: parse_style(definition)?,
            definition: definition.to_string(),
        })
    }

    /// Combine: `other`'s attributes and colours win where it sets them.
    fn __add__(&self, other: &Style) -> Style {
        Style {
            definition: format!("{} {}", self.definition, other.definition)
                .trim()
                .to_string(),
            inner: self.inner.combine(&other.inner),
        }
    }

    fn __eq__(&self, other: &Style) -> bool {
        self.inner == other.inner
    }

    /// Hashable, as upstream's `Style` is: equal styles hash alike, because
    /// both compare the parsed style, never the definition string.
    fn __hash__(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.inner).hash(&mut hasher);
        hasher.finish()
    }

    fn __str__(&self) -> String {
        if self.definition.is_empty() {
            "none".into()
        } else {
            self.definition.clone()
        }
    }

    fn __repr__(&self) -> String {
        format!("Style.parse({:?})", self.__str__())
    }
}

/// A `style=` argument: a string (a theme name or a definition) or a `Style`.
pub(crate) fn style_type(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<StyleType>> {
    let Some(value) = value.filter(|v| !v.is_none()) else {
        return Ok(None);
    };
    if let Ok(style) = value.extract::<PyRef<'_, Style>>() {
        return Ok(Some(StyleType::Style(style.inner.clone())));
    }
    if let Ok(name) = value.extract::<String>() {
        return Ok(Some(StyleType::Name(name)));
    }
    Err(PyTypeError::new_err("style must be a str or a Style"))
}

/// A `style=` argument that core wants resolved now (table columns, borders).
pub(crate) fn resolved_style(value: Option<&Bound<'_, PyAny>>) -> PyResult<Option<CoreStyle>> {
    Ok(match style_type(value)? {
        Some(StyleType::Style(style)) => Some(style),
        Some(StyleType::Name(name)) => Some(parse_style(&name)?),
        None => None,
    })
}

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Style>()
}

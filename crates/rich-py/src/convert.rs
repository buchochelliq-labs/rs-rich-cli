//! Argument conversions shared by every area: Rich's `justify`, `overflow`
//! and `align` names, Python `int` indices, and padding tuples.
//!
//! Owner: the foundation. Add a shared conversion here only when two areas
//! need it; area-specific ones stay in the area's own module.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyInt;

use rich::align::HorizontalAlign;
use rich::{Justify, Overflow};

use crate::limits::MAX_PADDING;

/// Rich's `JustifyMethod`; `None` and `"default"` are core's default.
pub(crate) fn justify(value: Option<&str>) -> PyResult<Justify> {
    Ok(match value {
        None | Some("default") => Justify::Default,
        Some("left") => Justify::Left,
        Some("center") => Justify::Center,
        Some("right") => Justify::Right,
        Some("full") => Justify::Full,
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "invalid justify {other:?}; expected left, center, right, full or default"
            )))
        }
    })
}

/// The name Rich uses for a core justify (`None` for the default).
pub(crate) fn justify_name(value: Justify) -> Option<&'static str> {
    match value {
        Justify::Default => None,
        Justify::Left => Some("left"),
        Justify::Center => Some("center"),
        Justify::Right => Some("right"),
        Justify::Full => Some("full"),
    }
}

/// Rich's `OverflowMethod`.
pub(crate) fn overflow(value: &str) -> PyResult<Overflow> {
    Ok(match value {
        "fold" => Overflow::Fold,
        "crop" => Overflow::Crop,
        "ellipsis" => Overflow::Ellipsis,
        "ignore" => Overflow::Ignore,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid overflow {other:?}; expected fold, crop, ellipsis or ignore"
            )))
        }
    })
}

/// The name Rich uses for a core overflow.
pub(crate) fn overflow_name(value: Overflow) -> &'static str {
    match value {
        Overflow::Fold => "fold",
        Overflow::Crop => "crop",
        Overflow::Ellipsis => "ellipsis",
        Overflow::Ignore => "ignore",
    }
}

/// Rich's `AlignMethod`.
pub(crate) fn align(value: &str) -> PyResult<HorizontalAlign> {
    Ok(match value {
        "left" => HorizontalAlign::Left,
        "center" => HorizontalAlign::Center,
        "right" => HorizontalAlign::Right,
        other => {
            return Err(PyValueError::new_err(format!(
                "invalid align {other:?}; expected left, center or right"
            )))
        }
    })
}

/// A Python `int` index: one too large for an `isize` saturates, which is
/// the same as clamping to the text (no text is that long).
pub(crate) struct Index(pub(crate) isize);

impl<'a, 'py> FromPyObject<'a, 'py> for Index {
    type Error = PyErr;

    fn extract(value: pyo3::Borrowed<'a, 'py, PyAny>) -> PyResult<Self> {
        match value.extract::<isize>() {
            Ok(index) => Ok(Index(index)),
            Err(_) if value.is_instance_of::<PyInt>() => {
                Ok(Index(if value.lt(0)? { isize::MIN } else { isize::MAX }))
            }
            Err(error) => Err(error),
        }
    }
}

/// Rich's `PaddingDimensions`: 1, 2 or 4 integers, each at most `MAX_PADDING`,
/// as `(top, right, bottom, left)`.
pub(crate) fn padding(value: &Bound<'_, PyAny>) -> PyResult<(usize, usize, usize, usize)> {
    let sides = if let Ok(all) = value.extract::<Index>() {
        [all.0; 4]
    } else if let Ok((vertical, horizontal)) = value.extract::<(Index, Index)>() {
        [vertical.0, horizontal.0, vertical.0, horizontal.0]
    } else if let Ok((top, right, bottom, left)) = value.extract::<(Index, Index, Index, Index)>() {
        [top.0, right.0, bottom.0, left.0]
    } else {
        return Err(PyValueError::new_err("padding must be 1, 2 or 4 integers"));
    };
    let mut result = [0usize; 4];
    for (side, value) in result.iter_mut().zip(sides) {
        *side = usize::try_from(value)
            .ok()
            .filter(|value| *value <= MAX_PADDING)
            .ok_or_else(|| {
                PyValueError::new_err(format!(
                    "padding must be between 0 and {MAX_PADDING}, got {value}"
                ))
            })?;
    }
    let [top, right, bottom, left] = result;
    Ok((top, right, bottom, left))
}

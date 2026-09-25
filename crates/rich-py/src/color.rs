//! `rich.color`, `rich.emoji` and the rest of the text/style area that has
//! no module yet: `Color`, `ColorTriplet`, `ColorSystem`, `Emoji`
//! (`rs_rich.color`, `rs_rich.emoji`).
//!
//! Owner: the text/style area (with `text.rs`, `style.rs` and `theme.rs`).
//! Placeholder: nothing is registered yet.

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

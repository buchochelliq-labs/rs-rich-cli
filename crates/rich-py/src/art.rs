//! `rs_rich.art` and `rs_rich.mermaid`: the `rich-art` crate (images in
//! every mode and fit, FIGlet, GIFs) and `rich-mermaid` diagrams.
//!
//! Owner: the art area. Placeholder: nothing is registered yet. The crates
//! are not dependencies yet; the art area adds them to Cargo.toml.
//! Register classes in [`register`] (submodules under `art/` are fine).

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

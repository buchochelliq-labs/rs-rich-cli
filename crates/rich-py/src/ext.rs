//! `rs_rich.ext`: the `rich-ext` crate's renderables and tools
//! (diagnostics, structured data, diffs, transforms, workflow renderables,
//! terminal capabilities, inspectors and the extension registry).
//!
//! Owner: the ext area. Each submodule wraps one group of `rich-ext`
//! modules; the Python package `rs_rich.ext` has one submodule per Rust
//! module (`rs_rich.ext.diagnostic`, `rs_rich.ext.data`, ...). Everything
//! renders in Rust: a class here stores what it was given and builds the
//! `rich-ext` value when printed.

use pyo3::prelude::*;

mod common;
mod data;
mod diagnostic;
mod transform;

pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    common::register(m)?;
    diagnostic::register(m)?;
    data::register(m)?;
    transform::register(m)?;
    Ok(())
}

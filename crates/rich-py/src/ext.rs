//! `rs_rich.ext`: the `rich-ext` crate's renderables and tools
//! (diagnostics, data inspection, diffs, transforms, workflow renderables,
//! the plugin host).
//!
//! Owner: the ext area. Placeholder: nothing is registered yet. The crate
//! is not a dependency yet; the ext area adds `rich-ext` to Cargo.toml.
//! Register classes in [`register`] (submodules under `ext/` are fine),
//! with `module = "rs_rich.ext"` (or `rs_rich.ext.<name>`) on each pyclass.

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

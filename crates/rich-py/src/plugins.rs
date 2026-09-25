//! `rs_rich.plugins`: the `rs-rich-plugin-api` contract from Python: Python
//! classes acting as highlighters, themes, renderers, fence renderers and
//! transforms, and registering them with the ext plugin host.
//!
//! Owner: the plugins area. Placeholder: nothing is registered yet.
//! Register classes and functions in [`register`] (submodules under
//! `plugins/` are fine).

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

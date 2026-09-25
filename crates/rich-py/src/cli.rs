//! Running the `rich` CLI from Python: `python -m rs_rich` and the console
//! script (`rs_rich/__main__.py` calls into this module).
//!
//! Owner: the CLI area. Placeholder: nothing is registered yet. Register the
//! entry point (for example a `_cli_main(argv)` function) in [`register`].

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

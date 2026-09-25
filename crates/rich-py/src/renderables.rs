//! Static renderables: `Rule`, `Padding`, `Align`, `Columns`, `Group`,
//! `Constrain`, `Tree`, `Layout`, `Bar`, `Spinner`, `Styled` (Python modules
//! `rs_rich.rule`, `rs_rich.padding`, `rs_rich.align`, `rs_rich.columns`,
//! `rs_rich.console.Group`, `rs_rich.constrain`, `rs_rich.tree`,
//! `rs_rich.layout`, `rs_rich.bar`, `rs_rich.spinner`, `rs_rich.styled`),
//! plus the rest of `Segment`'s class methods (in `segment.rs`).
//!
//! Owner: the static-renderables area. Placeholder: nothing is registered
//! yet. Add each class here (or in `renderables/<name>.rs` submodules
//! declared from this file) and register it in [`register`] with
//! `renderable::add_renderable_class::<T>(m)`; see `renderable.rs`.

use pyo3::prelude::*;

pub(crate) fn register(_m: &Bound<'_, PyModule>) -> PyResult<()> {
    Ok(())
}

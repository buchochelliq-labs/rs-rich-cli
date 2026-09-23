//! Bounded extension layouts.
mod constraints;
pub use constraints::{allocate, Allocation, Constraint, ConstraintError};
mod node;
mod overflow;
pub use node::{Alignment, Axis, LayoutNode};
pub use overflow::{fit_segments, OverflowPolicy};

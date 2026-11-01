//! Bounded extension layouts.
mod constraints;
pub use constraints::{Allocation, Constraint, ConstraintError, allocate};
mod node;
mod overflow;
mod overflowing;
pub use node::{Alignment, Axis, LayoutNode};
pub use overflow::{OverflowPolicy, fit_segments};
pub use overflowing::Overflowing;

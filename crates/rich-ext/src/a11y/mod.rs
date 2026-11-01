//! Accessibility: semantic text for screen readers ([`semantic`]), user
//! presentation preferences ([`policy`]) and colour checks ([`contrast`]).

pub mod contrast;
pub mod policy;
pub mod semantic;

pub use contrast::{CheckOptions, ContrastReport, Finding, check_theme};
pub use policy::{AccessibilityPolicy, Status, SymbolSet};
pub use semantic::{AccessibleText, semantic_text};

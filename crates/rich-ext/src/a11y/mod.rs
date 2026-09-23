//! Accessibility: semantic text for screen readers ([`semantic`]), user
//! presentation preferences ([`policy`]) and colour checks ([`contrast`]).

pub mod contrast;
pub mod policy;
pub mod semantic;

pub use contrast::{check_theme, CheckOptions, ContrastReport, Finding};
pub use policy::{AccessibilityPolicy, Status, SymbolSet};
pub use semantic::{semantic_text, AccessibleText};

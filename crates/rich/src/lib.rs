//! # rich
//!
//! A **faithful** Rust port of the Python [`rich`](https://github.com/Textualize/rich)
//! terminal-rendering library. This crate mirrors upstream module-for-module and
//! its version tracks the upstream release it reflects (see `UPSTREAM.toml`).
//!
//! Local features and the plugin registry live in the separate `rich-ext` crate
//! — do **not** add non-upstream behavior here. See `AGENTS.md`.
//!
//! ## Ported so far (the first vertical slice)
//!
//! [`color`] · [`style`] · [`cells`] · [`segment`] · [`markup`] · [`text`] ·
//! [`theme`] · [`console`] · [`protocol`] (extension points) · [`measure`] ·
//! [`errors`]
//!
//! The remaining modules are tracked as roadmap issues; see `docs/PORTING.md`
//! for the module map and per-module parity status.

pub mod align;
pub mod r#box;
pub mod cells;
pub mod color;
pub mod columns;
pub mod console;
pub mod constrain;
pub mod errors;
pub mod filesize;
pub mod markup;
pub mod measure;
pub mod padding;
pub mod panel;
pub mod protocol;
pub mod rule;
pub mod segment;
pub mod style;
pub mod table;
pub mod text;
pub mod theme;
pub mod tree;
pub mod wrap;

// A small, curated prelude mirroring the most-used names from `rich`'s top level.
pub use crate::align::{Align, HorizontalAlign};
pub use crate::color::{Color, ColorSystem, ColorTriplet};
pub use crate::columns::Columns;
pub use crate::console::{Console, ConsoleOptions, Justify};
pub use crate::constrain::Constrain;
pub use crate::errors::{Result, RichError};
pub use crate::padding::Padding;
pub use crate::panel::Panel;
pub use crate::protocol::{Highlighter, Renderable};
pub use crate::rule::Rule;
pub use crate::segment::Segment;
pub use crate::style::Style;
pub use crate::table::Table;
pub use crate::text::Text;
pub use crate::theme::Theme;
pub use crate::tree::Tree;

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
pub mod ansi;
pub mod bar;
pub mod r#box;
pub mod cells;
pub mod color;
mod color_names;
pub mod columns;
pub mod console;
pub mod constrain;
pub mod control;
pub mod emoji;
mod emoji_codes;
pub mod errors;
pub mod export;
pub mod filesize;
pub mod highlighter;
pub mod json;
pub mod layout;
pub mod live;
pub mod live_render;
pub mod log_render;
pub mod markdown;
pub mod markup;
pub mod measure;
pub mod padding;
pub mod pager;
pub mod panel;
pub mod pretty;
pub mod progress;
pub mod progress_bar;
pub mod prompt;
pub mod protocol;
pub mod ratio;
mod repr_patterns;
pub mod rule;
pub mod screen;
pub mod segment;
pub mod spinner;
mod spinner_data;
pub mod status;
pub mod style;
pub mod styled;
pub mod svg;
pub mod syntax;
pub mod table;
pub mod terminal_theme;
pub mod text;
pub mod theme;
pub mod traceback;
pub mod tree;
pub mod wrap;

// A small, curated prelude mirroring the most-used names from `rich`'s top level.
pub use crate::align::{Align, HorizontalAlign};
pub use crate::ansi::AnsiDecoder;
pub use crate::bar::Bar;
pub use crate::color::{Color, ColorSystem, ColorTriplet};
pub use crate::columns::Columns;
pub use crate::console::{Console, ConsoleOptions, Justify, Overflow};
pub use crate::constrain::Constrain;
pub use crate::control::{Control, ControlType};
pub use crate::errors::{Result, RichError};
pub use crate::highlighter::{ISO8601Highlighter, RegexHighlighter, ReprHighlighter};
pub use crate::json::Json;
pub use crate::layout::Layout;
pub use crate::live::{AutoLive, Live};
pub use crate::live_render::LiveRender;
pub use crate::log_render::{LogLevel, LogRender};
pub use crate::padding::Padding;
pub use crate::pager::{Pager, SystemPager};
pub use crate::panel::Panel;
pub use crate::pretty::Pretty;
pub use crate::progress::{Progress, ProgressColumn};
pub use crate::progress_bar::ProgressBar;
pub use crate::protocol::{Highlighter, Renderable};
pub use crate::rule::Rule;
pub use crate::screen::Screen;
pub use crate::segment::Segment;
pub use crate::spinner::Spinner;
pub use crate::status::Status;
pub use crate::style::{Style, StyleType};
pub use crate::styled::Styled;
pub use crate::syntax::Syntax;
pub use crate::table::Table;
pub use crate::terminal_theme::{
    TerminalTheme, DEFAULT_TERMINAL_THEME, DIMMED_MONOKAI, MONOKAI, NIGHT_OWLISH, SVG_EXPORT_THEME,
};
pub use crate::text::Text;
pub use crate::theme::Theme;
pub use crate::traceback::Traceback;
pub use crate::tree::Tree;

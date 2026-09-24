//! # rich
//!
//! A **faithful** Rust port of the Python [`rich`](https://github.com/Textualize/rich)
//! terminal-rendering library. This crate mirrors upstream module-for-module.
//! Its version is independent SemVer; the upstream release it reflects
//! (currently `rich` 15.0.0) is recorded in `UPSTREAM.toml`.
//!
//! Local features and the plugin registry live in the separate `rich-ext` crate
//! — do **not** add non-upstream behavior here. See `AGENTS.md`.
//!
//! ## What is ported
//!
//! - Console and styling: [`console`], [`style`], [`color`], [`segment`],
//!   [`markup`], [`theme`], [`control`], [`screen`], [`export`].
//! - Text: [`text`], [`cells`], [`wrap`], [`emoji`], [`highlighter`], [`ansi`].
//! - Renderables: [`panel`], [`table`], [`tree`], [`layout`], [`columns`],
//!   [`align`], [`padding`], [`constrain`], [`rule`], [`bar`], [`markdown`],
//!   [`syntax`], [`json`], [`pretty`], [`traceback`], [`log_render`].
//! - Live output: [`live`], [`progress`] (with [`track`]), [`progress_bar`],
//!   [`spinner`], [`status`].
//! - Input and paging: [`prompt`], [`pager`].
//! - Extension points: [`protocol`], [`measure`].
//!
//! Most modules are partial ports of their upstream counterpart; the per-module
//! status and parity evidence are in `docs/PORTING.md`.

pub mod align;
pub mod ansi;
pub mod bar;
pub mod r#box;
mod cell_widths;
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
mod markdown_url;
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
pub mod pyformat;
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
pub use crate::console::{Console, ConsoleOptions, Justify, Overflow, ThemeContext};
pub use crate::constrain::Constrain;
pub use crate::control::{Control, ControlType};
pub use crate::errors::{Result, RichError};
pub use crate::highlighter::{ISO8601Highlighter, RegexHighlighter, ReprHighlighter};
pub use crate::json::Json;
pub use crate::layout::Layout;
pub use crate::live::{AutoLive, Live, LivePanic};
pub use crate::live_render::LiveRender;
pub use crate::log_render::{level_text, LogLevel, LogRecord, LogRender};
pub use crate::padding::Padding;
pub use crate::pager::{Pager, SystemPager};
pub use crate::panel::Panel;
pub use crate::pretty::Pretty;
pub use crate::progress::{
    track, BarColumn, LiveProgress, Progress, ProgressColumn, ProgressReader, SpinnerColumn, Task,
    TaskId, TaskUpdate, TextColumn, TimeRemainingColumn, Track, TrackStdout,
};
pub use crate::progress_bar::ProgressBar;
pub use crate::protocol::{Highlighter, LineRenderable, Renderable};
pub use crate::rule::Rule;
pub use crate::screen::Screen;
pub use crate::segment::Segment;
pub use crate::spinner::Spinner;
pub use crate::status::Status;
pub use crate::style::{Style, StyleType};
pub use crate::styled::Styled;
pub use crate::syntax::Syntax;
pub use crate::table::{Cell, ColumnOptions, Table};
pub use crate::terminal_theme::{
    TerminalTheme, DEFAULT_TERMINAL_THEME, DIMMED_MONOKAI, MONOKAI, NIGHT_OWLISH, SVG_EXPORT_THEME,
};
pub use crate::text::Text;
pub use crate::theme::Theme;
pub use crate::traceback::Traceback;
pub use crate::tree::Tree;

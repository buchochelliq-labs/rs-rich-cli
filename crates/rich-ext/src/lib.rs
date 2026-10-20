//! # rich-ext
//!
//! Extensions layered on top of the faithful [`rich`] core, plus the internal
//! plugin registry. **Anything that is not a faithful port of upstream `rich`
//! belongs here**, so the core stays a clean, easily-synced mirror.
//!
//! This crate carries its own independent SemVer (it does not track any upstream
//! version). See `AGENTS.md` → Versioning and `docs/PLUGINS.md`.
//!
//! ## Modules
//!
//! - Errors and logs: [`diagnostic`], [`dashboard`], [`stacktrace`],
//!   [`hyperlink`], [`log_handler`] (`RichHandler`, `SpanView`), [`adapters`]
//!   (`EventSink`; `LogAdapter` and `EventLayer` with the `log` / `tracing`
//!   features), [`event`], [`log_stream`] (log records parsed, filtered,
//!   collapsed and grouped, and [`ChannelSink`](log_stream::ChannelSink)).
//! - Data and authoring: `data`, `deps` (Cargo dependency trees) and `schema`
//!   (JSON Schema trees and diffs, and the format-neutral schema model), all
//!   behind the `data` feature;
//!   [`derive`](mod@derive), [`macros`] (the
//!   `macros` feature re-exports `richf!` and `#[derive(Rich)]`), [`cli_doc`]
//!   (`clap` feature for `clap::Command`).
//! - Diffs and testing: [`diff`], `testing` and `qa` (`testing` feature).
//! - Terminals: [`capabilities`], [`fidelity`], [`a11y`], [`target`],
//!   [`ansi_explain`], [`sanitize`], [`encoding`], [`clipboard`] (OSC 52).
//! - Workflows: [`workflow`], [`transfer`], [`countdown`], [`notify`],
//!   [`cancel`], [`table`], [`badge`], [`size_bar`], [`format`](mod@format), [`redact`]
//!   (experimental).
//! - Charts: [`chart`] (sparklines, bars, histograms, line and scatter plots,
//!   gauges, heatmaps, status matrices, KPI cards and timelines; ratatui's
//!   chart, canvas with a world map, and gauges).
//! - Colour: [`palette`] (Tailwind's and Material's palettes; HSL, HSV,
//!   Okhsv and Oklab colours, blends and gradients).
//! - Inspectors: [`source_view`], [`hex`], [`unicode_inspect`],
//!   [`env_inspect`].
//! - Documents: `document` (Org, AsciiDoc and man pages beside Markdown and
//!   `rst`, with format detection; `syntax` feature).
//! - Layout and live output: [`layout`], [`live`], [`surface`] (positioned
//!   drawing that writes only what changed).
//! - Pipelines: [`transform`], and `data::transform` (`jsonpath` feature).
//! - Files: [`filetype`] (icons, colours, MIME types), [`fs`] (local times,
//!   owners), [`listing`] (entries, sorting, a tree walk), [`open`] (opener
//!   rules), [`file_ops`] (copy, move, delete with progress), [`trash`] (the
//!   FreeDesktop trash), [`git`] (status per file), and `archive` (tar and zip
//!   listing and extraction; `archive` feature).
//!
//! The user guide (<https://buchochelliq-labs.github.io/rs-rich-cli/guide/ext/>)
//! covers each module with compiled examples.
//!
//! ```
//! use rich::{Console, ColorSystem};
//! use rich_ext::ConsoleExt;
//!
//! let mut console = Console::builder()
//!     .force_terminal(true)
//!     .color_system(Some(ColorSystem::Truecolor))
//!     .build();
//! console.install_extensions(); // registers the number highlighter, etc.
//! let out = console.render_str_to_string("count=7");
//! assert!(out.contains("\x1b[1;36m7\x1b[0m"));
//! ```

pub mod a11y;
pub mod ansi_explain;
// Tar, tar.gz and zip listings and extraction.
#[cfg(feature = "archive")]
pub mod archive;
pub mod badge;
pub mod cancel;
pub mod capabilities;
pub mod chart;
pub mod cli;
pub mod clipboard;
// Documentation formats (Markdown, reStructuredText, Org, AsciiDoc, man).
#[cfg(feature = "syntax")]
pub mod document;
// FFT, Hann window and spectrum.
pub mod dsp;
pub mod encoding;
pub mod env_inspect;
pub mod fidelity;
// File-type icons, MIME types and the Nerd Font check.
pub mod filetype;
pub mod format;
pub mod frame;
// Text coloured along a gradient of two or more colours.
pub mod gradient;
// Local times and owner names for file listings.
pub mod fs;
// File operations with progress: copy, move, delete, trash.
pub mod file_ops;
// Git's status of each file, through `git status --porcelain`.
pub mod git;
pub mod graphics;
pub mod hex;
pub mod highlighter;
// Directory entries, sorting, line-mode cells and a lazy tree walk.
pub mod listing;
pub mod markdown_export;
// Which program opens a file: Yazi's opener rules.
pub mod open;
pub mod palette;
pub mod plugin_loading;
// QR codes drawn with half blocks.
#[cfg(feature = "qr")]
pub mod qr;
pub mod redact;
pub mod registry;
#[cfg(feature = "syntax")]
pub mod rst;
pub mod sanitize;
pub mod size_bar;
pub mod source_view;
pub mod surface;
// Warming up syntax highlighting on a worker thread.
#[cfg(feature = "syntax")]
pub mod syntax;
pub mod table;
pub mod target;
pub mod theme;
pub mod transform;
// The FreeDesktop trash: put, list, restore, purge.
pub mod trash;
pub mod unicode_inspect;
pub mod workflow;

pub use highlighter::NumberHighlighter;
pub use registry::{
    install_defaults, BuiltinPlugin, ExtensionRegistry, HighlighterChoiceError, RegisteredPlugin,
};
/// The plugin contract, re-exported so a host needs only `rich_ext`.
pub use rich_plugin_api as plugin;
pub use sanitize::sanitize_terminal_controls;
pub use theme::{extended_theme, EXTRA_STYLES};

use rich::Console;

/// Ergonomic extension methods on the core [`Console`].
///
/// Defined here (not in core) so the core remains free of our additions.
pub trait ConsoleExt {
    /// Install this crate's default extensions onto the console.
    fn install_extensions(&mut self) -> &mut Self;
}

impl ConsoleExt for Console {
    fn install_extensions(&mut self) -> &mut Self {
        install_defaults(self);
        self
    }
}

#[cfg(feature = "testing")]
pub mod testing;

// QA tooling: screenshots, stress, lint, explain, profile, fuzz, matrix, bench.
#[cfg(feature = "testing")]
pub mod qa;

pub mod layout;

pub mod event;

pub mod diagnostic;

pub mod dashboard;

pub mod derive;

pub mod macros;

#[cfg(feature = "macros")]
pub use rich_macros::{markup, richf, style, theme_key, Rich};

/// Paths the macros expand to. Not a public API.
#[doc(hidden)]
pub mod __private {
    pub use rich;
}

pub mod hyperlink;

pub mod stacktrace;

pub mod log_handler;
pub use log_handler::{RichHandler, SpanView};

// Log streams: parsing, filtering, collapsing and grouping records (#724).
pub mod log_stream;

// `EventSink` always; `LogAdapter` and `EventLayer` behind `log` and `tracing`.
pub mod adapters;

pub mod live;

#[cfg(feature = "data")]
pub mod data;

// Real sources drawn as trees: Cargo dependency graphs and JSON Schemas.
#[cfg(feature = "data")]
pub mod deps;
#[cfg(feature = "data")]
pub mod schema;

// CLI authoring: help, errors, completions, docs, config reference and
// precedence from one command description.
pub mod cli_doc;

// Diffs: engine, views, source/patch renderers, test reports and assertions.
pub mod diff;

// Transfers, retry/rate-limit countdowns and transient notifications.
pub mod countdown;
pub mod notify;
pub mod transfer;

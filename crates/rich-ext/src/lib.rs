//! # rich-ext
//!
//! Extensions layered on top of the faithful [`rich`] core, plus the internal
//! plugin registry. **Anything that is not a faithful port of upstream `rich`
//! belongs here**, so the core stays a clean, easily-synced mirror.
//!
//! This crate carries its own independent SemVer (it does not track any upstream
//! version). See `AGENTS.md` → Versioning and `docs/PLUGINS.md`.
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
pub mod cancel;
pub mod capabilities;
pub mod cli;
pub mod encoding;
pub mod env_inspect;
pub mod fidelity;
pub mod format;
pub mod hex;
pub mod highlighter;
pub mod registry;
pub mod sanitize;
pub mod source_view;
pub mod target;
pub mod theme;
pub mod unicode_inspect;

pub use highlighter::NumberHighlighter;
pub use registry::{install_defaults, ExtensionRegistry};
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

#[cfg(any(feature = "log", feature = "tracing"))]
pub mod adapters;

pub mod live;

#[cfg(feature = "data")]
pub mod data;

// CLI authoring: help, errors, completions, docs, config reference and
// precedence from one command description.
pub mod cli_doc;

// Diffs: engine, views, source/patch renderers, test reports and assertions.
pub mod diff;

// Transfers, retry/rate-limit countdowns and transient notifications.
pub mod countdown;
pub mod notify;
pub mod transfer;

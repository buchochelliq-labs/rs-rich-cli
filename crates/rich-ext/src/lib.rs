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

pub mod highlighter;
pub mod registry;
pub mod theme;

pub use highlighter::NumberHighlighter;
pub use registry::{install_defaults, ExtensionRegistry};
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

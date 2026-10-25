//! Noctiluca, a terminal web browser on intuiTUIve.
//!
//! Chrome loads and runs the page. Noctiluca reads what Chrome laid out
//! (one `DOMSnapshot.captureSnapshot` and the accessibility tree, which the
//! page cannot observe) and draws it in cells: text as terminal text,
//! controls as intuiTUIve widgets, images as media regions. The design is
//! `docs/design/browser.md`.
//!
//! - [`engine`]: the [`Engine`](engine::Engine) trait, and [`cdp`] its
//!   Chrome backend over rs-rich-embed.
//! - [`model`]: the page model, rebuilt from each snapshot.
//! - [`tab`]: a tab on a thread of its own: loading, the debounced change
//!   ping, guarded actions and stills of images.
//!
//! Everything the page says is untrusted: control characters are stripped
//! ([`clean`]) before any of it reaches the terminal.

pub mod actions;
pub mod app;
pub mod bookmarks;
pub mod cdp;
pub mod clean;
pub mod config;
pub mod draft;
pub mod engine;
pub mod layout;
pub mod model;
pub mod tab;
pub mod testing;
pub mod url;
pub mod widgets;

pub use clean::clean;
pub use url::file_url;

//! Noctiluca, a terminal web browser on intuiTUIve.
//!
//! Chrome loads and runs the page. Noctiluca reads what Chrome laid out
//! (one `DOMSnapshot.captureSnapshot` and the accessibility tree, which the
//! page cannot observe) and draws it in cells: text as terminal text,
//! controls as intuiTUIve widgets, images as media regions. This crate is
//! the `noctiluca` binary and the library it is built from.
//!
//! The user guide is on the docs site: [Noctiluca][guide], with pages on
//! [getting started][start], [keys][keys], [configuration][config],
//! [privacy and security][privacy] and [using it as a library][library].
//! The design and its phases are in [`docs/design/browser.md`][design].
//!
//! [guide]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/
//! [start]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/getting-started/
//! [keys]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/keys/
//! [config]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/config/
//! [privacy]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/privacy/
//! [library]: https://buchochelliq-labs.github.io/rs-rich-cli/guide/noctiluca/library/
//! [design]: https://github.com/buchochelliq-labs/rs-rich-cli/blob/main/docs/design/browser.md
//!
//! # The pieces
//!
//! - [`engine`]: the [`Engine`](engine::Engine) trait, what Noctiluca
//!   needs from a browser, and [`cdp`] its Chrome backend over
//!   rs-rich-embed ([`ChromeBackend`](cdp::ChromeBackend)).
//! - [`tab`]: a [`Tab`](tab::Tab) runs an engine on a thread of its own:
//!   loading, the debounced change ping, live logs read incrementally,
//!   guarded actions, stills and pictures. It sends
//!   [`Update`](tab::Update)s.
//! - [`model`]: the [`Page`](model::Page) model, rebuilt from each
//!   snapshot: blocks of text, controls and media.
//! - [`layout`]: the page as lines of cells, in reader mode or the
//!   projection; [`adapt`] its terminal adaptations, named generic rules
//!   that can each be switched off; [`pictures`] draws pictures and moving
//!   GIFs over their regions. [`widgets`] gives controls their intuiTUIve widgets.
//! - [`app`]: the browser as an intuiTUIve app; [`actions`] is the one
//!   list of what it can do, with the keys of both presets.
//! - [`config`], [`bookmarks`], [`args`], [`dump`] (`--dump`) and
//!   [`url`]: the rest of the binary, usable on their own.
//! - [`testing`]: [`FakeEngine`](testing::FakeEngine), an engine over
//!   saved snapshots, for tests with no browser.
//!
//! Everything the page says is untrusted: control characters are stripped
//! ([`clean()`]) before any of it reaches the terminal.
//!
//! # Examples
//!
//! The address bar's rules, from a config:
//!
//! ```
//! use noctiluca::config::{Config, Destination};
//!
//! let config = Config::parse(
//!     r#"
//! [shortcuts]
//! wiki = "https://en.wikipedia.org/w/index.php?search={}"
//! "#,
//! )
//! .unwrap();
//! assert!(config.warnings.is_empty());
//! assert_eq!(
//!     config.destination("wiki sea sparkle", "about:blank", &[]),
//!     Destination::Keyword("https://en.wikipedia.org/w/index.php?search=sea+sparkle".into())
//! );
//! assert_eq!(
//!     config.destination("docs.rs/serde", "about:blank", &[]),
//!     Destination::Url("https://docs.rs/serde".into())
//! );
//! ```
//!
//! A page read once through an engine, as `--dump` does. Here the engine
//! is [`FakeEngine`](testing::FakeEngine), over the snapshots of the
//! crate's test pages, so no browser runs:
//!
//! ```
//! use std::path::Path;
//! use std::time::Duration;
//!
//! use noctiluca::testing::FakeEngine;
//!
//! let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
//! let engine = FakeEngine::from_dir(&snapshots)?;
//! let page = noctiluca::dump::read_page(
//!     Box::new(engine),
//!     "https://fixture.example/second.html",
//!     Duration::from_secs(10),
//! )?;
//! assert_eq!(page.title, "Second page");
//! assert!(page.text().contains("You followed the link."));
//! let link = page.find("link", "Back to the start");
//! assert_eq!(link.len(), 1);
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! The same with Chrome (it needs a Chrome or Chromium, and a normal
//! user):
//!
//! ```no_run
//! use std::time::Duration;
//!
//! use noctiluca::cdp::{ChromeBackend, ChromeOptions};
//!
//! let engine = ChromeBackend::start(ChromeOptions {
//!     columns: 100,
//!     rows: 40,
//!     ..ChromeOptions::default()
//! })?;
//! let page = noctiluca::dump::read_page(
//!     Box::new(engine),
//!     "https://example.com",
//!     Duration::from_secs(60),
//! )?;
//! println!("{}", page.text());
//! # Ok::<(), std::io::Error>(())
//! ```

pub mod about;
pub mod actions;
pub mod adapt;
pub mod app;
pub mod args;
pub mod bars;
pub mod bookmarks;
pub mod cdp;
pub mod clean;
pub mod config;
pub mod draft;
pub mod dump;
pub mod engine;
pub mod layout;
pub mod model;
pub mod pictures;
pub mod tab;
pub mod testing;
pub mod url;
pub mod video;
pub mod widgets;

pub use clean::clean;
pub use url::file_url;

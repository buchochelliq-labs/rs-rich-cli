//! Other programs and web pages inside an intuiTUIve app.
//!
//! - [`terminal`] runs any program in a pane of the app: a shell, `htop`,
//!   an editor, a terminal browser. Keys, the mouse, pastes and resizes go
//!   to the program; its screen is followed by rs-rich-record's VT
//!   emulator (vt100, with rich's character widths) and drawn in the pane,
//!   with scrollback; its exit is a signal and a callback.
//! - [`web_view`] shows a web page in a pane, with an address bar, and
//!   back, forward, reload, the address and the loading state as signals.
//!
//! Each sits on a trait, as intuiTUIve's terminals sit on `Backend`: what
//! runs behind it can be swapped for another implementation, including one
//! the app writes itself.
//!
//! - [`PtyHost`] starts a program and carries its bytes and size.
//!   [`LocalPty`] runs one on this machine (a PTY on Unix, ConPTY on
//!   Windows); [`ReplayHost`] plays bytes back, for tests. An SSH session
//!   is a third, passed to [`terminal_with`].
//! - [`WebEngine`] opens pages, takes input and hands back frames, of
//!   cells or of pixels. [`ProgramEngine`] (the default) runs a terminal
//!   browser (Carbonyl, Browsh, Chawan, w3m) in a pane; `ChromeEngine`
//!   (feature `chrome`) drives a headless Chrome or Chromium the user
//!   installed, over the DevTools protocol; `BrowshEngine` (feature
//!   `browsh`) reads pages as text from Browsh's HTTP server. Pixel frames
//!   are drawn as coloured half blocks.
//!
//! No browser ships with this crate, and none is downloaded: each is a
//! program the user chooses and installs.
//!
//! ```no_run
//! # extern crate rich_intuituive as intuituive;
//! use intuituive::prelude::*;
//! use rich_embed::{terminal, web_view_with, ProgramEngine};
//!
//! fn main() -> std::io::Result<()> {
//!     App::new(|| {
//!         let page = web_view_with(ProgramEngine::new("w3m"), "https://example.com");
//!         row([
//!             terminal("bash").on_exit(|_, cx| cx.quit()).node().panel("shell"),
//!             page.node().panel("web"),
//!         ])
//!     })
//!     .run()
//! }
//! ```
//!
//! The panes need a running app: like [`signal`](rich_intuituive::signal),
//! [`terminal`] and [`web_view`] are called inside `App::new`'s closure or
//! a node's.

#[cfg(feature = "browsh")]
pub mod browsh;
#[cfg(feature = "chrome")]
pub mod chrome;
mod host;
pub mod keys;
mod pane;
mod pixels;
mod program;
mod term;
mod wake;
mod web;

#[cfg(feature = "browsh")]
pub use browsh::BrowshEngine;
#[cfg(feature = "chrome")]
pub use chrome::ChromeEngine;
pub use host::{Command, ExitStatus, LocalPty, Notify, PtyHost, ReplayHandle, ReplayHost};
pub use pane::{terminal, terminal_with, TerminalPane, DEFAULT_SCROLLBACK};
pub use pixels::{half_blocks, Pixels};
pub use program::{ProgramEngine, BROWSER_VARIABLE, KNOWN_BROWSERS};
pub use web::{
    web_view, web_view_with, PageState, WebEngine, WebFrame, WebHandle, WebInput, WebView,
};

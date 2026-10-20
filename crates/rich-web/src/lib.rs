//! Serve intuiTUIve apps, and any terminal program, to a web browser.
//!
//! [`serve`] listens on an address and runs one [`App`] per browser tab,
//! each in a process of its own (see [Isolation](#isolation)). The page is
//! [xterm.js](https://xtermjs.org),
//! vendored in this crate and served by it; the app's frames go to it as
//! terminal output over a WebSocket, and its keys, mouse and resizes come
//! back as the same [`Event`](intuituive::interact::Event)s a terminal
//! gives. No async runtime, and nothing fetched from a CDN.
//!
//! The framework is re-exported as [`rich_web::intuituive`](intuituive), so
//! an app served this way needs no other dependency.
//!
//! ```no_run
//! use rich_web::intuituive::prelude::*;
//!
//! fn counter() -> App {
//!     App::new(|| {
//!         let count = signal(0);
//!         text!("[b]Count:[/] {count} (+ adds one)")
//!             .on_key("+", move |_| count.update(|c| *c += 1))
//!     })
//! }
//!
//! fn main() -> std::io::Result<()> {
//!     // Prints http://127.0.0.1:8080/?token=… to open in a browser.
//!     rich_web::serve("127.0.0.1:8080", counter)
//! }
//! ```
//!
//! [`Server`] is the same with options: a session cap, a fixed token, the
//! page's title, origins a proxy serves the page from, and
//! [`spawn`](Server::spawn) to run in the background.
//!
//! # Isolation
//!
//! Each app session runs in a process of its own ([`Isolation::Processes`],
//! the default): the server starts this program again with
//! [`SESSION_ENV`] set, and when that process reaches the same [`serve`] or
//! [`Server::bind`] call, it runs the tab's app there, connected back to
//! the server over loopback, and exits. A crash, a hang or a runaway
//! allocation then ends one tab; the server kills a session's process
//! with what it started (on Unix its process group, on Windows a job
//! object) when the tab closes, the server stops or the server dies, and
//! on Unix can cap its memory and CPU time
//! ([`Server::session_memory`], [`Server::session_cpu`]).
//! [`Server::session_command`] starts it some other way (an interpreter
//! and its script), and [`Isolation::Threads`] runs every session on a
//! thread of the server instead, as 0.0.18 did: for tests, and programs
//! that cannot start themselves again.
//!
//! # Any terminal program
//!
//! [`serve_command`] runs a program on a pseudo-terminal per tab instead
//! (a PTY on Unix, ConPTY on Windows, through rs-rich-embed's
//! [`LocalPty`]) and streams it to the same page: keys, pastes and resizes
//! go to the program, its output to the page, and its exit ends the session
//! with its status shown. [`Server::bind_host`] takes any other
//! [`PtyHost`] (an SSH session, a container, a test host).
//!
//! ```no_run
//! fn main() -> std::io::Result<()> {
//!     rich_web::serve_command("127.0.0.1:8080", ["htop"])
//! }
//! ```
//!
//! A page that falls behind holds the program back rather than the server
//! buffering without end: at most [`Server::max_buffered`] bytes of output
//! wait for the page, and past that the program's writes wait, as they do
//! on a slow terminal. Input is bounded too: with 1 MiB that the program
//! has not read held for it, the server stops reading the page until it
//! reads some, so a flood of pastes waits in the network.
//!
//! # A DOM renderer
//!
//! [`Renderer::Dom`] (or `&renderer=dom` in the page's address) draws an
//! app as a grid of styled spans instead of xterm.js, and carries its
//! [accessibility tree](intuituive::Driver::accessibility) as ARIA: roles,
//! names and states, the focused node focused in the page, and
//! announcements in a live region, so a browser's screen reader reads the
//! app. Its messages are described in [`dom`].
//!
//! # Security
//!
//! Anyone who can open a session can do whatever the app (or the program)
//! lets them, so the server is closed by default:
//!
//! - **It listens where it is told.** Give it a loopback address
//!   (`127.0.0.1`, `[::1]`, `localhost`) to keep it on this computer, as
//!   the examples do. Any other address prints a warning.
//! - **A random token** (128 bits from the operating system) is part of the
//!   URL printed at start; the page and the WebSocket both refuse a request
//!   without it.
//! - **The `Origin` header is checked**: browsers let any web page open a
//!   WebSocket to a localhost port, so the upgrade must come from this
//!   server's own page (its address, which must also be the `Host` asked
//!   for, against DNS rebinding) or from an origin allowed with
//!   [`Server::allow_origin`].
//! - **Sessions are capped** ([`DEFAULT_MAX_SESSIONS`] unless set with
//!   [`Server::max_sessions`]); one more is refused with `503`.
//! - **Requests are timed**: a connection has 10 seconds to send its whole
//!   request, however slowly it trickles in, and with 64 still sending,
//!   one more is answered `503`.
//!
//! There is no other authentication and no TLS. To reach the server from
//! another machine, put it behind a reverse proxy that has both, and allow
//! the proxy's origin.

// The framework, under the name its own crate gives it (the workspace's
// dependency key makes it `rich_intuituive`), and re-exported, so an app needs
// only this crate.
pub extern crate rich_intuituive as intuituive;

pub mod dom;
pub mod input;

#[cfg(feature = "server")]
mod http;
#[cfg(feature = "server")]
mod isolate;
#[cfg(feature = "server")]
mod pty;
#[cfg(feature = "server")]
mod server;
#[cfg(feature = "server")]
mod session;

#[cfg(feature = "server")]
pub use server::*;
#[cfg(feature = "server")]
pub(crate) use server::{random_token, MAX_MESSAGE};

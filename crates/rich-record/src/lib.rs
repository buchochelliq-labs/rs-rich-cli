//! Record scripted terminal sessions: tapes (#599).
//!
//! A tape scripts a session: type, press keys, wait for text on the screen,
//! take screenshots. [`record::record`] runs it in `bash` on a real PTY with a
//! pinned environment, follows the screen with a VT emulator, and returns the
//! screenshots and a timeline. [`record::write`] turns them into files:
//!
//! - per screenshot, a PNG in a window frame, an SVG with selectable text, and
//!   a text grid (through [`rich_ext::frame::Frame`]);
//! - an asciinema v2 cast with input events and the theme in its header;
//! - a GIF with a key overlay, and an MP4 when FFmpeg is installed.
//!
//! [`record::check`] compares a new run's text grids with committed ones, so
//! documentation media cannot drift from the program it shows.
//!
//! ```no_run
//! use rich_record::{record, tape};
//! let source = std::fs::read_to_string("demo.tape")?;
//! let tape = tape::parse(&source)?;
//! let recording = record::record(&tape, "demo", &record::Options::default())?;
//! let fonts = rich_record::render::raster::Fonts::embedded();
//! record::write(&recording, "media/demo".as_ref(), "demo", record::Formats::ALL,
//!               &fonts, &Default::default(), None)?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! Linux and macOS are supported. Windows builds through ConPTY but needs
//! `bash` on `PATH` and is experimental.

pub mod record;
pub mod render;
pub mod screen;
pub mod session;
pub mod tape;

pub use record::{Formats, Options, Problem, Recording};
pub use screen::{Snapshot, Theme};
pub use tape::{Tape, TapeError};

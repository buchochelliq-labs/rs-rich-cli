//! Record scripted terminal sessions: tapes (#599).
//!
//! A tape scripts a session: type, press keys, wait for text on the screen,
//! take screenshots. [`record::record`] runs it in `bash` on a real PTY with a
//! pinned environment, follows the screen with a VT emulator, and returns the
//! screenshots and a timeline. [`record::write_selected`] turns them into the
//! files the tape's `Output` lines ask for ([`record::write`] takes the
//! formats as given):
//!
//! - per screenshot, a PNG in a window frame, an SVG with selectable text
//!   (drawn by rich-ext's frame exporter, [`rich_ext::frame::Frame::to_svg`]),
//!   and a text grid (through [`rich_ext::frame::Frame`]);
//! - an asciinema v2 cast with input events and the theme in its header;
//! - a GIF and an animated PNG (APNG) with a key overlay, and an MP4 when
//!   FFmpeg is installed, at the tape's frame rate (`Set Framerate`), from
//!   every settled screen update;
//! - a self-contained HTML page with a small player and the screenshots.
//!
//! The window frame, a caption and the key overlay can each be switched on
//! or off ([`record::Presentation`]), and a tape's `Output` chooses what is
//! written.
//!
//! [`record::check`] compares a new run's text grids with committed ones, so
//! documentation media cannot drift from the program it shows.
//!
//! The PNG rasteriser also draws output that never ran on a PTY:
//! [`render::png::segments_png`] takes the segments a [`rich::Console`]
//! recorded (rich-cli's `--export-png`), and [`render::png::frame_png`] a
//! rich-ext [`Frame`](rich_ext::frame::Frame).
//!
//! ```no_run
//! use rich_record::{record, tape};
//! let source = std::fs::read_to_string("demo.tape")?;
//! let tape = tape::parse(&source)?;
//! let recording = record::record(&tape, "demo", &record::Options::default())?;
//! let fonts = rich_record::render::raster::Fonts::embedded();
//! // The tape's `Output` formats; `Formats::ALL` puts no further limit on them.
//! let written = record::write_selected(&recording, "media/demo".as_ref(), "demo",
//!                                      record::Formats::ALL, &fonts, &Default::default(), None)?;
//! for skipped in &written.skipped {
//!     eprintln!("skipped {}: {}", skipped.path.display(), skipped.reason);
//! }
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! The emulator follows the images a program draws ([`graphics`]: Kitty,
//! Sixel, iTerm2) on the cell grid, and the kitty keyboard protocol's flags
//! ([`keyboard`]), so a tape can hold keys down (`KeyDown`, `KeyUp`,
//! `Hold`) and a game be recorded as it plays.
//!
//! Linux and macOS are supported. Windows builds through ConPTY but needs
//! `bash` on `PATH` and is experimental.

// A builder method that returns `Self` is `#[must_use]`: dropping its
// result drops the value (CONTRIBUTING.md, "API conventions").
#![warn(clippy::return_self_not_must_use)]

pub mod graphics;
pub mod keyboard;
pub mod record;
pub mod render;
pub mod screen;
pub mod session;
pub mod tape;
pub mod terminal;

pub use record::{Formats, Options, Presentation, Problem, Recording, Skipped, Written};
pub use screen::{Packed, Snapshot, Theme};
pub use tape::{Format, Output, Tape, TapeError};

//! # rich-ratatui
//!
//! [ratatui](https://ratatui.rs) interop for
//! [rs-rich](https://github.com/buchochelliq-labs/rs-rich-cli), the Rust port
//! of Python's `rich`, both ways. Not a port of anything upstream; it builds
//! on core `rich`'s public API and on [`ratatui_core`], the crate ratatui's
//! own types live in, so the `Buffer`, `Rect` and `Widget` here are the ones
//! a `ratatui` 0.30 app already uses.
//!
//! - **rich in ratatui.** [`RichWidget`] is a ratatui
//!   [`Widget`](ratatui_core::widgets::Widget) that draws any
//!   [`rich::Renderable`] (a `Table`, a `Panel`, `Markdown`, `Syntax`, a
//!   `Tree`, markup) into a ratatui [`Buffer`](ratatui_core::buffer::Buffer),
//!   so a ratatui app can adopt rich's renderables one widget at a time.
//! - **ratatui in rich-interact.** `RatatuiComponent` (the off-by-default
//!   `interact` feature) is a `rich_interact::Component` whose view is drawn
//!   by ratatui widgets, so an existing widget runs unchanged inside a
//!   rich-interact event loop, and under its headless driver in tests.
//! - **The conversions underneath**, useful on their own:
//!   [`style`] maps styles and colours both ways; [`buffer`] writes rich
//!   lines into a buffer ([`lines_to_buffer`]), reads a buffer back into
//!   rich lines ([`buffer_to_lines`]), and walks a rectangle of a buffer cell
//!   by cell without allocating ([`cells`]).
//!
//! ## Use rich in your ratatui app
//!
//! ```
//! use ratatui_core::buffer::Buffer;
//! use ratatui_core::layout::Rect;
//! use ratatui_core::widgets::Widget;
//! use rich::Table;
//! use rich_ratatui::RichWidget;
//!
//! let mut table = Table::new().title("Inventory");
//! table.add_column("item");
//! table.add_column("qty");
//! table.add_row(&["apples", "[green]3[/]"]);
//! table.add_row(&["pears", "[bold red]0[/]"]);
//!
//! // In an app: `frame.render_widget(&RichWidget::new(&table), area)`.
//! let area = Rect::new(0, 0, 24, 7);
//! let mut buffer = Buffer::empty(area);
//! RichWidget::new(&table).render(area, &mut buffer);
//!
//! let row: String = (0..area.width).map(|x| buffer[(x, 1)].symbol()).collect();
//! assert_eq!(row.trim_end(), "┏━━━━━━━━┳━━━━━┓");
//! ```
//!
//! ## What converts, and what is lost
//!
//! The mapping is lossless for the common subset: no colour, the terminal
//! default (rich's `default` ⇄ ratatui's `Color::Reset`), the 16 named
//! colours, the 256-colour palette, 24-bit RGB, and nine attributes (bold,
//! dim, italic, underline, blink, rapid blink (`blink2`), reverse, conceal
//! (hidden), strike), each tri-state: on, explicitly off, or unset. A rich
//! `Panel` drawn into a buffer and read back shows the same text and the same
//! SGR codes. It loses:
//!
//! - rich's **colour names**: `grey0` comes back as `color(16)`, `#FF0000`
//!   as `#ff0000`. The colour (kind, number, RGB) is the same; only
//!   `Color::name` differs.
//! - rich's legacy **Windows** colours, which come back as the standard
//!   colour with the same number.
//! - rich's **`underline2`, `frame`, `encircle`, `overline`**: ratatui has
//!   no modifier for them; they are dropped going to ratatui.
//! - rich's **hyperlinks** (OSC 8) and **meta**: a ratatui cell holds a
//!   symbol and a style, nothing else, so links are dropped and their text
//!   kept.
//! - ratatui's **underline colour** (its `underline-color` feature): rich
//!   has no such colour; it is dropped going to rich.
//! - In a *buffer*, `Color::Reset` and "no colour" are the same thing (a
//!   cell always has a colour; a fresh one has `Reset`), so reading a buffer
//!   takes `Reset` as unset, not as rich's `default`.
//! - **Widths:** rich and ratatui measure some characters (some emoji)
//!   differently. Writing, each segment is placed at the column rich
//!   measured, so a disagreement cannot shift the rest of the line; reading,
//!   a wide symbol is measured as ratatui measured it when it was set.
//!
//! ## Features
//!
//! | Feature | Default | What it adds |
//! |---|---|---|
//! | `interact` | no | `RatatuiComponent`, through `rs-rich-interact` (and crossterm) |

pub mod buffer;
#[cfg(feature = "interact")]
mod interact;
pub mod style;
mod widget;

/// The ratatui core crate this one is built against, re-exported so an app
/// can name the same `Buffer`, `Rect` and `Widget` without a version
/// mismatch.
pub use ratatui_core;

pub use buffer::{buffer_area_to_lines, buffer_to_lines, cells, lines_to_buffer};
#[cfg(feature = "interact")]
pub use interact::RatatuiComponent;
pub use style::{to_ratatui_color, to_ratatui_style, to_rich_color, to_rich_style};
pub use widget::RichWidget;

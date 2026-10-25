//! The cell screen: what the terminal shows, kept between frames.
//!
//! A [`Screen`] is a grid of [`Cell`]s, each a grapheme, its width and an
//! interned rich [`Style`](rich::Style) (so hyperlinks and every rich
//! attribute survive, which a ratatui cell cannot hold). Nodes write their
//! rendered lines into their own rectangle; the [`Painter`] compares only
//! the damaged rectangles with what it sent last time and writes the
//! difference.
//!
//! These are [`rich_ext::surface`]'s types: an app draws with the same code
//! a plain [`Surface`](rich_ext::surface::Surface) user gets, and the
//! benchmark gate measures it.

pub use rich_ext::surface::{Cell, PaintStats, Painter, Rect, Screen};

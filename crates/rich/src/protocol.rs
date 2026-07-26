//! Rendering & extension protocols.
//!
//! Port of upstream `rich/protocol.py` + `rich/abc.py` + the highlighter
//! interface. **These traits are the sanctioned extension points of the port.**
//! Extensions in `rich-ext` (and, later, third-party plugins) implement them;
//! the faithful core only ever ships upstream's built-in implementations. See
//! docs/PLUGINS.md.

use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::segment::Segment;
use crate::text::Text;

/// Anything that can be rendered to a stream of [`Segment`]s within a width.
///
/// The Rust equivalent of upstream's `__rich_console__(console, options)`
/// protocol. Implement it to make a custom type printable by [`Console`]. The
/// `options` carry the available width (and, later, height/justify) the
/// renderable must fit into. Newlines between lines are emitted as ordinary
/// segments containing `\n`.
pub trait Renderable {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment>;

    /// The `(minimum, maximum)` cell width this renderable wants. The default
    /// assumes the renderable fills the available width (e.g. `Panel`, `Table`);
    /// `Text` overrides it with its content width so the top-level print path can
    /// shrink to fit. Port of `__rich_measure__` / `Measurement.get`.
    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        Measurement::new(options.max_width, options.max_width)
    }
}

/// A transformer that adds style spans to [`Text`] (e.g. syntax/number/URL
/// highlighting). The Rust equivalent of upstream's `Highlighter` ABC.
///
/// This is the primary *plugin* seam for the first slice: `rich-ext` registers
/// [`Highlighter`]s onto a [`Console`] without the core knowing they exist.
pub trait Highlighter {
    /// Inspect `text` and apply any style spans in place.
    fn highlight(&self, text: &mut Text);
}

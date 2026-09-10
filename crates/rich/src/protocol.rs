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

/// Optional line-streaming extension point for renderables.
///
/// Mirrors the incremental consumption of upstream's rendering generators.
/// Consumers can write each visual line immediately instead of collecting the
/// complete segment stream. Implementations may still retain source data for
/// measurement. This trait keeps streaming hooks out of inherent core APIs.
pub trait LineRenderable: Renderable {
    /// Emit styled visual lines without trailing newlines, stopping immediately
    /// on the callback's first error. An empty segment represents a blank line;
    /// calling the callback zero times represents no output.
    fn try_for_each_line<E>(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        emit: impl FnMut(Vec<Segment>) -> Result<(), E>,
    ) -> Result<(), E>;
}

/// Transfer already-owned table rows without cloning every cell string.
///
/// This extension point changes ownership only. Column definitions, measurement
/// and rendering follow the table's existing rules, including missing/extra cells.
/// Producers that parse into owned strings can release their row collection as
/// they populate a table instead of retaining a second complete copy.
pub trait OwnedTableRows {
    fn extend_owned_rows(&mut self, rows: Vec<Vec<String>>) -> &mut Self;
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

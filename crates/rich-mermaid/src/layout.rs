//! Draw a [`Flowchart`] as text.
//!
//! The layered layout and the box drawing live in `rs-rich-diagram`
//! ([`rich_diagram::layout`]); this module converts the flowchart into a
//! [`rich_diagram::Graph`] and draws it there.

use crate::flowchart::Flowchart;

/// A drawn diagram: lines of text without trailing spaces. The same type as
/// [`rich_diagram::Drawing`].
pub type Diagram = rich_diagram::Drawing;

/// Lay out and draw `chart`. `ascii` restricts the output to ASCII.
pub fn draw(chart: &Flowchart, ascii: bool) -> Result<Diagram, String> {
    rich_diagram::draw(&chart.to_graph(), ascii).map_err(|error| error.to_string())
}

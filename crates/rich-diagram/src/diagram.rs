//! [`Diagram`]: a [`Graph`] as a renderable.

use std::sync::OnceLock;

use rich::console::{Console, ConsoleOptions};
use rich::measure::Measurement;
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;
use rich::text::Text;

use crate::graph::Graph;
use crate::layout::{draw, DrawError, Drawing};

/// A graph drawn with box-drawing characters, or ASCII.
///
/// It measures to the width of its drawing. Given less, it is cropped: each
/// line is cut at the right edge, and rows left empty are dropped, so the
/// output never exceeds the width it is given and is the same every time.
/// [`Diagram::drawing`] gives the whole drawing, uncropped. A graph too large
/// to lay out renders as a one-line dim note saying why.
///
/// ASCII follows [`Console::ascii_only`] (a console whose encoding is not
/// UTF), unless set with [`Diagram::ascii`].
///
/// ```
/// use rich::Console;
/// use rich_diagram::{Diagram, Direction, Graph};
///
/// let graph = Graph::new(Direction::LeftRight)
///     .node("api", "API")
///     .node("db", "DB")
///     .edge("api", "db").label("reads");
/// let console = Console::builder().width(40).color_system(None).build();
/// let out = console.render_to_string(&Diagram::new(graph));
/// assert_eq!(
///     out,
///     "┌─────┐         ┌────┐\n\
///      │ API ├──reads─►│ DB │\n\
///      └─────┘         └────┘\n"
/// );
/// ```
#[derive(Clone, Debug)]
pub struct Diagram {
    graph: Graph,
    ascii: Option<bool>,
    /// The Unicode and ASCII drawings, made on first use.
    drawn: [OnceLock<Result<Drawing, DrawError>>; 2],
}

impl Diagram {
    pub fn new(graph: Graph) -> Self {
        Diagram {
            graph,
            ascii: None,
            drawn: Default::default(),
        }
    }

    /// Draw with ASCII only (`true`) or box drawing (`false`), whatever the
    /// console's encoding.
    pub fn ascii(mut self, ascii: bool) -> Self {
        self.ascii = Some(ascii);
        self
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    /// The whole drawing, uncropped.
    pub fn drawing(&self, ascii: bool) -> Result<&Drawing, &DrawError> {
        self.drawn[usize::from(ascii)]
            .get_or_init(|| draw(&self.graph, ascii))
            .as_ref()
    }

    fn ascii_for(&self, console: &Console) -> bool {
        self.ascii.unwrap_or_else(|| console.ascii_only())
    }
}

impl From<Graph> for Diagram {
    fn from(graph: Graph) -> Self {
        Diagram::new(graph)
    }
}

impl Renderable for Diagram {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let drawing = match self.drawing(self.ascii_for(console)) {
            Ok(drawing) => drawing,
            Err(error) => {
                let style = Style::parse("dim italic").expect("valid style");
                let mut segments =
                    Text::styled(format!("Diagram: too large to draw: {error}"), style)
                        .rich_render(console, options);
                if segments
                    .iter()
                    .rev()
                    .find(|segment| !segment.text.is_empty())
                    .is_some_and(|segment| !segment.text.ends_with('\n'))
                {
                    segments.push(Segment::line());
                }
                return segments;
            }
        };
        let mut segments = Vec::new();
        for line in drawing.cropped(options.max_width) {
            segments.push(Segment::new(line, None));
            segments.push(Segment::line());
        }
        segments
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        match self.drawing(self.ascii_for(console)) {
            Ok(drawing) => Measurement::new(drawing.width, drawing.width),
            Err(_) => Measurement::new(options.max_width, options.max_width),
        }
    }
}

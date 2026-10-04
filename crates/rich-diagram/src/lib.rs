//! Graph diagrams for the `rich` Rust port: a graph model, a layered layout,
//! and a renderable that draws it with box-drawing characters.
//!
//! Not a port of anything upstream: an rs-rich addition. It depends on core
//! `rich` only, so any crate (Mermaid flowcharts among them) can draw graphs
//! without pulling in `rs-rich-ext`.
//!
//! - [`Graph`]: nodes with labels and [`Shape`]s, edges with labels,
//!   [`Stroke`]s and [`Head`]s, flowing in a [`Direction`]. Build one in code
//!   with its chaining builder, or from parts as a parser does.
//! - [`draw`]: the layered (Sugiyama-style) layout, giving a [`Drawing`]:
//!   plain lines of text. See [`layout`] for how it works.
//! - [`Diagram`]: the drawing as a renderable that measures itself and crops
//!   to the width it is given, in Unicode box drawing or ASCII.
//!
//! ```
//! use rich::Console;
//! use rich_diagram::{Diagram, Direction, Graph, Shape};
//!
//! let graph = Graph::new(Direction::TopDown)
//!     .node("start", "Start").shape(Shape::Round)
//!     .node("ok", "Tests pass?").shape(Shape::Rhombus)
//!     .edge("start", "ok")
//!     .edge("ok", "ship").label("yes");
//! let console = Console::builder().width(40).color_system(None).build();
//! let out = console.render_to_string(&Diagram::new(graph));
//! assert!(out.contains("Tests pass?"), "{out}");
//! assert!(out.lines().all(|line| rich::cells::cell_len(line) <= 40));
//! ```
//!
//! Clusters (subgraph frames) are not drawn yet: the layout lays every node
//! out in one graph.

pub mod diagram;
pub mod graph;
pub mod layout;

pub use diagram::Diagram;
pub use graph::{Direction, Edge, Graph, Head, Node, Shape, Stroke, MAX_EDGE_LENGTH};
pub use layout::{draw, DrawError, Drawing};

//! The `Diagram` renderable and the builder.
//!
//! The layout came out of rs-rich-mermaid unchanged, so graphs built here to
//! match Mermaid's snapshot sources must draw exactly Mermaid's snapshots
//! (`crates/rich-mermaid/tests/snapshots`). rs-rich-mermaid's own
//! `tests/diagram.rs` checks the same from the other side, on random graphs.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::panel::Panel;
use rich::table::{Cell, Table};
use rich::Console;
use rich_diagram::{
    draw, Diagram, Direction, Edge, Graph, Head, Node, Shape, Stroke, MAX_EDGE_LENGTH,
};

fn console(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

fn render(graph: &Graph, width: usize, ascii: bool) -> String {
    console(width).render_to_string(&Diagram::new(graph.clone()).ascii(ascii))
}

fn snapshot(name: &str) -> String {
    let path = format!(
        "{}/../rich-mermaid/tests/snapshots/{name}.txt",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// `graph TD` with Christmas, from Mermaid's `labels_td` snapshot.
fn christmas() -> Graph {
    Graph::new(Direction::TopDown)
        .node("A", "Christmas")
        .node("B", "Go shopping")
        .shape(Shape::Round)
        .edge("A", "B")
        .label("Get money")
        .node("C", "Let me think")
        .shape(Shape::Rhombus)
        .edge("B", "C")
        .node("D", "Laptop")
        .edge("C", "D")
        .label("One")
        .node("E", "iPhone")
        .edge("C", "E")
        .label("Two")
        .node("F", "Car")
        .edge("C", "F")
        .label("Three")
}

/// Every Mermaid shape, from the `shapes` snapshots.
fn shapes() -> Graph {
    let chains: [&[(&str, &str, Shape)]; 4] = [
        &[
            ("a", "rect", Shape::Rect),
            ("b", "round", Shape::Round),
            ("c", "stadium", Shape::Stadium),
            ("d", "subroutine", Shape::Subroutine),
        ],
        &[
            ("e", "cylinder", Shape::Cylinder),
            ("f", "circle", Shape::Circle),
            ("g", "double", Shape::DoubleCircle),
            ("h", "flag", Shape::Asymmetric),
        ],
        &[
            ("i", "decision", Shape::Rhombus),
            ("j", "hexagon", Shape::Hexagon),
            ("k", "in", Shape::Parallelogram),
            ("l", "out", Shape::ParallelogramAlt),
        ],
        &[
            ("m", "top", Shape::Trapezoid),
            ("n", "bottom", Shape::TrapezoidAlt),
        ],
    ];
    let mut graph = Graph::new(Direction::TopDown);
    for chain in chains {
        for (id, label, shape) in chain {
            graph = graph.node(id, *label).shape(*shape);
        }
        for pair in chain.windows(2) {
            graph = graph.edge(pair[0].0, pair[1].0);
        }
    }
    graph
}

#[test]
fn builder_graphs_draw_mermaids_snapshots() {
    assert_eq!(render(&christmas(), 80, false), snapshot("labels_td"));
    assert_eq!(render(&shapes(), 100, false), snapshot("shapes"));
    assert_eq!(render(&shapes(), 100, true), snapshot("shapes_ascii"));

    let cycle = Graph::new(Direction::TopDown)
        .node("A", "Draft")
        .node("B", "Review")
        .edge("A", "B")
        .edge("B", "A")
        .label("changes")
        .node("C", "Merged")
        .edge("B", "C")
        .edge("C", "C");
    assert_eq!(render(&cycle, 80, false), snapshot("cycle"));

    let disconnected = Graph::new(Direction::LeftRight)
        .edge("A", "B")
        .edge("C", "D")
        .edge("D", "E")
        .node("lonely", "On its own");
    assert_eq!(render(&disconnected, 80, false), snapshot("disconnected"));
}

#[test]
fn every_direction_draws_mermaids_snapshots() {
    for (name, direction) in [
        ("dir_td", Direction::TopDown),
        ("dir_bt", Direction::BottomUp),
        ("dir_lr", Direction::LeftRight),
        ("dir_rl", Direction::RightLeft),
    ] {
        let graph = Graph::new(direction)
            .node("A", "Start")
            .node("B", "Middle")
            .shape(Shape::Round)
            .edge("A", "B")
            .node("C", "End")
            .shape(Shape::Stadium)
            .edge("B", "C")
            .edge("A", "C")
            .label("skip");
        assert_eq!(render(&graph, 80, false), snapshot(name), "{name}");
    }
}

#[test]
fn strokes_and_heads_draw_mermaids_snapshot() {
    let graph = Graph::new(Direction::LeftRight)
        .edge("A", "B")
        .edge("A", "C")
        .stroke(Stroke::Thick)
        .edge("A", "D")
        .stroke(Stroke::Dotted)
        .link("A", "E")
        .edge("A", "F")
        .heads(Head::Arrow, Head::Arrow)
        .edge("A", "G")
        .heads(Head::None, Head::Circle)
        .edge("A", "H")
        .heads(Head::None, Head::Cross)
        .link("A", "I")
        .stroke(Stroke::Invisible)
        .edge("A", "J")
        .stroke(Stroke::Dotted)
        .label("dotted");
    assert_eq!(render(&graph, 80, false), snapshot("strokes"));
}

/// The same graph assembled from parts, as a parser would.
#[test]
fn from_parts_draws_the_same_as_the_builder() {
    let nodes = vec![
        Node::new("A", "Draft"),
        Node::new("B", "Review"),
        Node::new("C", "Merged"),
    ];
    let mut back = Edge::new(1, 0);
    back.label = Some("changes".into());
    let edges = vec![Edge::new(0, 1), back, Edge::new(1, 2), Edge::new(2, 2)];
    let parts = Graph::from_parts(Direction::TopDown, nodes, edges);
    assert_eq!(render(&parts, 80, false), snapshot("cycle"));
}

#[test]
fn ascii_output_is_ascii_and_follows_the_console() {
    let graph = christmas();
    let out = render(&graph, 80, true);
    assert!(out.is_ascii(), "{out}");
    assert!(out.contains("| Christmas |"), "{out}");
    // Unset, it follows the console: ASCII when the console says so.
    let ascii_console = Console::builder()
        .width(80)
        .color_system(None)
        .ascii_only(true)
        .build();
    assert!(ascii_console.ascii_only());
    assert_eq!(
        ascii_console.render_to_string(&Diagram::new(graph.clone())),
        out
    );
    assert_eq!(
        console(80).render_to_string(&Diagram::new(graph)),
        snapshot("labels_td")
    );
}

/// The layout has no width constraint: a drawing wider than the console is
/// cropped at the right edge, the same way every time.
#[test]
fn never_wider_than_the_width_it_is_given() {
    let mut wide = Graph::new(Direction::TopDown);
    for i in 0..12 {
        wide = wide
            .node(format!("n{i}"), format!("node number {i}"))
            .edge("root", format!("n{i}"));
    }
    let full = draw(&wide, false).unwrap();
    assert_eq!(full.width, 230);
    for graph in [&wide, &christmas(), &shapes()] {
        for ascii in [false, true] {
            for width in 1..=200 {
                let out = render(graph, width, ascii);
                for line in out.lines() {
                    assert!(cell_len(line) <= width, "{width}: {line:?}");
                }
                assert_eq!(out, render(graph, width, ascii), "not deterministic");
            }
        }
    }
    // The crop is Mermaid's: the same lines, without Mermaid's note.
    let mermaid = snapshot("cropped");
    let drawing: Vec<&str> = mermaid
        .lines()
        .filter(|l| !l.starts_with("Mermaid:"))
        .collect();
    let out = render(&wide, 60, false);
    assert_eq!(out.lines().collect::<Vec<_>>(), drawing);
    assert_eq!(full.cropped(60), drawing);
}

#[test]
fn measures_to_its_drawing() {
    let diagram = Diagram::new(christmas());
    let console = console(120);
    let width = diagram.drawing(false).unwrap().width;
    assert_eq!(width, 33);
    assert_eq!(
        Measurement::get(&console, &console.options(), &diagram),
        Measurement::new(width, width)
    );
    let empty = Diagram::new(Graph::new(Direction::TopDown));
    assert_eq!(console.render_to_string(&empty), "");
    assert_eq!(
        Measurement::get(&console, &console.options(), &empty),
        Measurement::new(0, 0)
    );
}

#[test]
fn sits_in_a_panel_and_a_table() {
    for width in [12, 30, 60] {
        let console = console(width);
        let panel = Panel::new(Box::new(Diagram::new(christmas())));
        let out = console.render_to_string(&panel);
        assert!(out.lines().all(|line| cell_len(line) <= width), "{out}");
        let mut table = Table::new();
        table.add_column("graph");
        table.add_row_cells(vec![Cell::Renderable(Arc::new(Diagram::new(christmas())))]);
        let out = console.render_to_string(&table);
        assert!(out.lines().all(|line| cell_len(line) <= width), "{out}");
    }
    // With room, the panel holds the whole drawing.
    let out = console(80).render_to_string(&Panel::fit(Box::new(Diagram::new(christmas()))));
    assert!(out.contains("│ Christmas │"), "{out}");
    assert!(out.contains("│ Laptop  │  │ iPhone  │  │ Car │"), "{out}");
}

#[test]
fn cycles_are_drawn_with_the_back_edge_reversed() {
    let graph = Graph::new(Direction::LeftRight)
        .edge("a", "b")
        .edge("b", "c")
        .edge("c", "a")
        .label("again");
    let out = render(&graph, 80, false);
    assert!(out.contains("again"), "{out}");
    // Three nodes in three ranks, the back edge's arrow pointing at `a`.
    assert_eq!(out.matches("┌───┐").count(), 3, "{out}");
    assert_eq!(
        out.matches('►').count() + out.matches('◄').count(),
        3,
        "{out}"
    );
}

#[test]
fn long_edges_span_ranks_and_are_capped() {
    // `a -> d` skips two ranks: it runs past `b` and `c`.
    let graph = Graph::new(Direction::TopDown)
        .edge("a", "b")
        .edge("b", "c")
        .edge("c", "d")
        .edge("a", "d")
        .label("shortcut");
    let out = render(&graph, 80, false);
    assert!(out.contains("shortcut"), "{out}");
    assert_eq!(out.matches('▼').count(), 4, "{out}");

    // A requested length places the target that many ranks down.
    let near = draw(&Graph::new(Direction::TopDown).edge("a", "b"), false).unwrap();
    let far = draw(
        &Graph::new(Direction::TopDown).edge("a", "b").min_length(3),
        false,
    )
    .unwrap();
    assert!(far.lines.len() > near.lines.len());
    let capped = Graph::new(Direction::TopDown)
        .edge("a", "b")
        .min_length(10_000);
    let longest = Graph::new(Direction::TopDown)
        .edge("a", "b")
        .min_length(MAX_EDGE_LENGTH);
    assert_eq!(draw(&capped, false), draw(&longest, false));
}

#[test]
fn labels_cannot_carry_escape_sequences() {
    let graph = Graph::new(Direction::LeftRight)
        .node("a", "\u{1b}[31mred\u{1b}[0m")
        .edge("a", "b")
        .label("\u{1b}]0;title\u{7}\tbold");
    let out = render(&graph, 80, false);
    assert!(out.contains("[31mred[0m"), "{out}");
    assert!(out.contains("]0;title bold"), "{out}");
    assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
}

#[test]
fn multi_line_labels() {
    let graph = Graph::new(Direction::TopDown)
        .node("a", "one\ntwo")
        .edge("a", "b");
    let lines = draw(&graph, false).unwrap().lines;
    assert_eq!(&lines[..4], ["┌─────┐", "│ one │", "│ two │", "└──┬──┘"]);
}

#[test]
fn too_large_graphs_render_a_note() {
    let mut graph = Graph::new(Direction::TopDown);
    for a in 0..45 {
        for b in 0..44 {
            graph = graph
                .edge(format!("a{a}"), format!("b{b}"))
                .min_length(MAX_EDGE_LENGTH);
        }
    }
    let error = draw(&graph, false).unwrap_err();
    assert!(error.to_string().contains("rank positions"), "{error}");
    let out = render(&graph, 60, false);
    assert!(out.starts_with("Diagram: too large to draw: "), "{out}");
    assert!(out.lines().all(|line| cell_len(line) <= 60), "{out}");
}

#[test]
fn edges_to_missing_nodes_are_refused() {
    let graph = Graph::from_parts(
        Direction::TopDown,
        vec![Node::new("a", "a")],
        vec![Edge::new(0, 3)],
    );
    let error = draw(&graph, false).unwrap_err();
    assert_eq!(
        error.to_string(),
        "edge 0 -> 3 names a node that does not exist (1 nodes)"
    );
}

#[test]
fn edges_without_any_nodes_are_refused() {
    // An empty graph draws as nothing, but not when its edges name nodes.
    let graph = Graph::from_parts(Direction::TopDown, Vec::new(), vec![Edge::new(0, 1)]);
    let error = draw(&graph, false).unwrap_err();
    assert_eq!(
        error.to_string(),
        "edge 0 -> 1 names a node that does not exist (0 nodes)"
    );
    let empty = Graph::from_parts(Direction::TopDown, Vec::new(), Vec::new());
    assert!(draw(&empty, false).unwrap().lines.is_empty());
}

//! Cluster frames and same-rank groups (0.0.16, #240).

use rich::cells::cell_len;
use rich::Console;
use rich_diagram::{draw, Cluster, Diagram, Direction, Dot, Graph};

fn lines(graph: &Graph, ascii: bool) -> Vec<String> {
    draw(graph, ascii).unwrap().lines
}

#[test]
fn a_cluster_is_framed_with_its_label_in_a_border() {
    let graph = Graph::new(Direction::LeftRight)
        .edge("web", "api")
        .edge("api", "db")
        .cluster("backend", "Backend", ["api", "db"]);
    assert_eq!(
        lines(&graph, false),
        [
            "         ┌╌ Backend ╌╌╌╌╌╌┐",
            "┌─────┐  ╎ ┌─────┐  ┌────┐╎",
            "│ web ├───►│ api ├─►│ db │╎",
            "└─────┘  ╎ └─────┘  └────┘╎",
            "         └╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌┘",
        ]
    );
    assert_eq!(
        lines(&graph, true),
        [
            "         +- Backend ------+",
            "+-----+  : +-----+  +----+:",
            "| web +--->| api +->| db |:",
            "+-----+  : +-----+  +----+:",
            "         +----------------+",
        ]
    );
}

#[test]
fn an_edge_crossing_the_top_border_moves_the_label_to_the_bottom() {
    let graph = Graph::new(Direction::TopDown)
        .edge("web", "api")
        .edge("api", "db")
        .cluster("backend", "Backend", ["api", "db"]);
    let lines = lines(&graph, false);
    assert_eq!(lines[5], "┌╌╌╌╌│╌╌╌╌╌╌┐", "{lines:#?}");
    assert_eq!(lines.last().unwrap(), "└╌ Backend ╌┘", "{lines:#?}");
}

#[test]
fn nested_clusters_draw_nested_frames() {
    let mut graph = Graph::new(Direction::LeftRight)
        .edge("web", "api")
        .edge("api", "db")
        .edge("api", "cache");
    let backend = graph.add_cluster(Cluster::new("backend").label("Backend").nodes([1]));
    graph.add_cluster(
        Cluster::new("storage")
            .label("Storage")
            .nodes([2, 3])
            .parent(backend),
    );
    let drawn = lines(&graph, false);
    let text = drawn.join("\n");
    assert!(text.contains("┌╌ Backend ╌"), "{text}");
    assert!(text.contains("┌╌ Storage ╌"), "{text}");
    // The inner frame starts below and right of the outer one, and ends
    // above it.
    let outer = drawn.iter().position(|l| l.contains("Backend")).unwrap();
    let inner = drawn.iter().position(|l| l.contains("Storage")).unwrap();
    assert!(inner > outer, "{text}");
    let column = |row: usize| cell_len(drawn[row].split("┌╌").next().unwrap());
    assert!(column(inner) > column(outer), "{text}");
    let bottoms: Vec<usize> = (0..drawn.len())
        .filter(|&row| drawn[row].contains("└╌"))
        .collect();
    assert_eq!(bottoms.len(), 2, "{text}");
    assert_eq!(text.matches("┌╌").count(), 2, "{text}");
}

#[test]
fn nodes_outside_a_cluster_stay_outside_its_frame() {
    // `x` ranks between the cluster's two nodes, so it must be pushed out
    // of the frame across the ranks.
    let graph = Graph::new(Direction::TopDown)
        .edge("a", "x")
        .edge("x", "c")
        .edge("a", "c")
        .cluster("k", "K", ["a", "c"]);
    let drawn = lines(&graph, false);
    let row = drawn.iter().find(|l| l.contains("│ x │")).unwrap();
    let frame = row.find('╎').expect("the frame's side runs past x");
    let node = row.find("│ x │").unwrap();
    assert!(node < frame, "{}", drawn.join("\n"));
}

#[test]
fn same_rank_groups_share_a_rank() {
    let ranked = |graph: &Graph| {
        let drawn = lines(graph, false);
        let row = |name: &str| {
            drawn
                .iter()
                .position(|l| l.contains(&format!("│ {name} │")))
                .unwrap()
        };
        (row("b"), row("d"))
    };
    let graph = Graph::new(Direction::TopDown)
        .edge("a", "b")
        .edge("a", "c")
        .edge("c", "d");
    let (b, d) = ranked(&graph);
    assert!(b < d);
    let (b, d) = ranked(&graph.clone().same_rank(["b", "d"]));
    assert_eq!(b, d);
    assert!(draw(&graph.same_rank(["b", "d"]), false)
        .unwrap()
        .notes
        .is_empty());
}

#[test]
fn a_same_rank_group_joined_by_an_edge_is_dropped_with_a_note() {
    let graph = Graph::new(Direction::TopDown)
        .edge("a", "b")
        .same_rank(["a", "b"]);
    let drawing = draw(&graph, false).unwrap();
    assert_eq!(
        drawing.notes,
        ["a same-rank group is not applied, as an edge joins two of its nodes: a, b"]
    );
    let plain = draw(&Graph::new(Direction::TopDown).edge("a", "b"), false).unwrap();
    assert_eq!(drawing.lines, plain.lines);
}

#[test]
fn a_node_in_two_clusters_that_do_not_nest_is_noted() {
    let mut graph = Graph::new(Direction::LeftRight).edge("a", "b");
    graph.add_cluster(Cluster::new("one").label("One").nodes([0]));
    graph.add_cluster(Cluster::new("two").label("Two").nodes([0, 1]));
    let drawing = draw(&graph, false).unwrap();
    assert_eq!(
        drawing.notes,
        ["node `a` is in clusters that do not nest (One and Two); it is framed in One"]
    );
}

#[test]
fn empty_clusters_and_bad_parents_draw_without_frames() {
    let plain = Graph::new(Direction::TopDown).edge("a", "b");
    let mut graph = plain.clone();
    graph.add_cluster(Cluster::new("empty").label("Empty"));
    graph.add_cluster(Cluster::new("out_of_range").nodes([7]));
    assert_eq!(lines(&graph, false), lines(&plain, false));
    // A parent that comes later is ignored: the cluster is drawn at the top
    // level, not refused.
    let mut graph = plain.clone();
    graph.add_cluster(Cluster::new("a").nodes([0]).parent(1));
    graph.add_cluster(Cluster::new("b").nodes([1]));
    let text = lines(&graph, false).join("\n");
    assert_eq!(text.matches("┌╌").count(), 2, "{text}");
}

#[test]
fn cluster_labels_cannot_carry_escape_sequences() {
    let graph = Graph::new(Direction::LeftRight).cluster("k", "\x1b[31mred\x07", ["a"]);
    let text = lines(&graph, false).join("\n");
    assert!(!text.contains('\x1b') && !text.contains('\x07'), "{text:?}");
    assert!(text.contains("[31mred"), "{text}");
}

#[test]
fn every_direction_frames_the_cluster() {
    for direction in [
        Direction::TopDown,
        Direction::BottomUp,
        Direction::LeftRight,
        Direction::RightLeft,
    ] {
        let graph = Graph::new(direction)
            .edge("a", "b")
            .edge("b", "c")
            .cluster("k", "Kay", ["b"]);
        let text = lines(&graph, false).join("\n");
        assert!(text.contains(" Kay "), "{direction:?}:\n{text}");
        assert_eq!(text.matches('┌').count(), 4, "{direction:?}:\n{text}");
    }
}

#[test]
fn dot_draws_clusters_and_rank_same() {
    let console = Console::builder().width(80).color_system(None).build();
    let out = console.render_export(&Dot::new(
        "digraph { subgraph cluster_x { label=\"X\"; a -> b } { rank=same; b; c } a -> c }",
    ));
    assert!(out.contains(" X "), "{out}");
    let row = out.lines().find(|l| l.contains("│ b │")).unwrap();
    assert!(row.contains("│ c │"), "{out}");
    assert!(!out.contains("DOT:"), "{out}");
}

#[test]
fn diagram_measures_the_frames_too() {
    let graph =
        Graph::new(Direction::LeftRight)
            .edge("a", "b")
            .cluster("k", "A long cluster label", ["b"]);
    let drawing = draw(&graph, false).unwrap();
    assert_eq!(
        drawing.width,
        drawing.lines.iter().map(|l| cell_len(l)).max().unwrap()
    );
    assert!(drawing.lines[0].contains("A long cluster label"));
    let console = Console::builder().width(20).color_system(None).build();
    let out = console.render_export(&Diagram::new(graph));
    assert!(out.lines().all(|l| cell_len(l) <= 20), "{out}");
}

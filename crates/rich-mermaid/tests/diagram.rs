//! Flowcharts draw through rs-rich-diagram: a graph built in code with
//! `rich_diagram`'s builder renders exactly as the equivalent Mermaid source.

use rich::Console;
use rich_diagram::{Diagram, Direction, Graph, Head, Stroke};
use rich_mermaid::{parse, Mermaid};

/// A small deterministic generator (xorshift), so failures reproduce.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Mermaid link syntax and the builder calls that make the same edge.
const LINKS: &[&str] = &[
    "-->", "---", "-.->", "==>", "-->|yes|", "--->", "<-->", "--o", "--x", "~~~",
];

fn add_link(graph: Graph, link: &str, from: &str, to: &str) -> Graph {
    let graph = graph.edge(from, to);
    match link {
        "-->" => graph,
        "---" => graph.heads(Head::None, Head::None),
        "-.->" => graph.stroke(Stroke::Dotted),
        "==>" => graph.stroke(Stroke::Thick),
        "-->|yes|" => graph.label("yes"),
        "--->" => graph.min_length(2),
        "<-->" => graph.heads(Head::Arrow, Head::Arrow),
        "--o" => graph.heads(Head::None, Head::Circle),
        "--x" => graph.heads(Head::None, Head::Cross),
        "~~~" => graph
            .stroke(Stroke::Invisible)
            .heads(Head::None, Head::None),
        other => unreachable!("{other}"),
    }
}

#[test]
fn builder_graphs_render_as_their_mermaid_source() {
    let mut rng = Rng(0x5eed_cafe_f00d_0001);
    for case in 0..300 {
        let (keyword, direction) = [
            ("TD", Direction::TopDown),
            ("BT", Direction::BottomUp),
            ("LR", Direction::LeftRight),
            ("RL", Direction::RightLeft),
        ][(rng.next() % 4) as usize];
        let nodes = 1 + rng.next() % 10;
        let edges = 1 + rng.next() % 16;
        let mut source = format!("graph {keyword}\n");
        let mut graph = Graph::new(direction);
        for _ in 0..edges {
            let a = rng.next() % nodes;
            let b = rng.next() % nodes;
            let link = LINKS[(rng.next() % LINKS.len() as u64) as usize];
            source.push_str(&format!("  n{a}[Node {a}] {link} n{b}[Node {b}]\n"));
            let (from, to) = (format!("n{a}"), format!("n{b}"));
            graph = graph
                .node(&from, format!("Node {a}"))
                .node(&to, format!("Node {b}"));
            graph = add_link(graph, link, &from, &to);
        }
        assert_eq!(
            parse(&source).unwrap().to_graph(),
            graph,
            "case {case}:\n{source}"
        );
        for (width, ascii) in [(120, false), (50, true)] {
            let console = Console::builder().width(width).color_system(None).build();
            let mermaid = console.render_to_string(&Mermaid::new(source.clone()).ascii(ascii));
            let built = console.render_to_string(&Diagram::new(graph.clone()).ascii(ascii));
            // Mermaid adds a note when it crops; the drawing above it is the same.
            let drawing: String = mermaid
                .split_inclusive('\n')
                .filter(|line| !line.starts_with("Mermaid:"))
                .collect();
            assert_eq!(built, drawing, "case {case} at {width}:\n{source}");
        }
    }
}

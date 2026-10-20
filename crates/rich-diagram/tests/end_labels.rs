//! Texts at an edge's ends (`Edge::start_label`, `Edge::end_label`): beside
//! the line where it leaves and reaches a node, in every direction, clear of
//! the other lines.

use rich_diagram::{dot, draw, Direction, Graph};

fn lines(graph: &Graph, ascii: bool) -> String {
    let mut out = draw(graph, ascii).unwrap().lines.join("\n");
    out.push('\n');
    out
}

/// A customer with orders and addresses, and an edge back that makes a
/// cycle: its texts swap ends with it.
fn shop(direction: Direction) -> Graph {
    Graph::new(direction)
        .link("customer", "order")
        .end_labels(Some("1"), Some("0..N"))
        .label("places")
        .link("customer", "address")
        .end_labels(Some("1..N"), Some("0..1"))
        .link("order", "customer")
        .end_labels(Some("x"), Some("y"))
}

#[test]
fn left_right_texts_sit_on_the_first_and_last_runs() {
    assert_eq!(
        lines(&shop(Direction::LeftRight), false),
        "                                  ┌───────┐
┌──────────┐      ┌──places──0..N─┤ order │
│          ├─1────┘┌────────────x─┤       │
│ customer ├─y─────┘              └───────┘
│          ├─1..N─┐
└──────────┘      │               ┌─────────┐
                  └──────────0..1─┤ address │
                                  └─────────┘
"
    );
}

#[test]
fn top_down_texts_sit_beside_the_line_clear_of_the_others() {
    assert_eq!(
        lines(&shop(Direction::TopDown), false),
        "      ┌───────────┐
      │ customer  │
      └───┬─┬─┬───┘
          │ │ │
        1 │ │ y
          │ │ │ 1..N
       ┌──┘ │ └─────┐
       │ ┌──┘       │
places │ │          │
  0..N │ │ x        │ 0..1
       │ │          │
    ┌──┴─┴──┐  ┌────┴────┐
    │ order │  │ address │
    └───────┘  └─────────┘
"
    );
}

#[test]
fn mirrored_directions_keep_texts_at_their_ends() {
    let rl = lines(&shop(Direction::RightLeft), false);
    assert!(rl.contains("│ order ├─0..N──places──┐"), "{rl}");
    assert!(rl.contains("└────1─┤          │"), "{rl}");
    let bt = lines(&shop(Direction::BottomUp), false);
    let rows: Vec<&str> = bt.lines().collect();
    assert_eq!(rows[4], "  0..N │ │ x        │ 0..1", "{bt}");
    assert_eq!(rows[9], "        1 │ │ y", "{bt}");
}

#[test]
fn a_straight_edge_holds_both_texts() {
    let graph = Graph::new(Direction::LeftRight)
        .link("a", "b")
        .end_labels(Some("1"), Some("0..N"));
    assert_eq!(
        lines(&graph, true),
        "+---+         +---+\n| a +-1--0..N-+ b |\n+---+         +---+\n"
    );
    // Empty texts take no room.
    let plain = Graph::new(Direction::LeftRight).link("a", "b");
    let empty = plain.clone().end_labels(Some(""), None);
    assert_eq!(lines(&empty, false), lines(&plain, false));
}

#[test]
fn dot_tail_and_head_labels() {
    let parsed =
        dot::parse("digraph { a -> b [taillabel=\"t\", headlabel=\"h\", label=mid] a -> c }")
            .unwrap();
    let edge = &parsed.graph.edges()[0];
    assert_eq!(
        (edge.start_label.as_deref(), edge.end_label.as_deref()),
        (Some("t"), Some("h"))
    );
    assert_eq!(
        lines(&parsed.graph, true),
        "    .---.
    | a |
    '+-+'
     | |
   t | |
  +--+ +-+
 mid     |
  | h    |
  v      v
.---.  .---.
| b |  | c |
'---'  '---'
"
    );
}

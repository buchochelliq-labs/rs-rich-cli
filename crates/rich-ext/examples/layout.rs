use rich::{Console, Text};
use rich_ext::layout::{Axis, Constraint, LayoutNode};
fn main() {
    let width = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(40);
    let layout = LayoutNode::split(
        Axis::Horizontal,
        vec![
            LayoutNode::leaf(Box::new(Text::styled("Sidebar", "bold cyan")))
                .width(Constraint::fixed(10)),
            LayoutNode::leaf(Box::new(Text::new(
                "Content wraps within the remaining bounded cells.",
            ))),
        ],
    );
    let console = Console::builder().width(width).height(4).build();
    console.print(&layout);
}

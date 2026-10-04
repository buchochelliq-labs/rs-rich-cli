//! Draw a small service graph built in code:
//! `cargo run -p rs-rich-diagram --example services -- [WIDTH] [ascii]`.
use rich::Console;
use rich_diagram::{Diagram, Direction, Graph, Shape, Stroke};

fn main() {
    let mut args = std::env::args().skip(1);
    let width = args.next().and_then(|w| w.parse().ok()).unwrap_or(80);
    let ascii = args.next().as_deref() == Some("ascii");

    let graph = Graph::new(Direction::TopDown)
        .node("web", "Browser")
        .shape(Shape::Round)
        .node("api", "API")
        .node("db", "Postgres")
        .shape(Shape::Cylinder)
        .node("jobs", "Job queue")
        .node("worker", "Worker")
        .shape(Shape::Subroutine)
        .edge("web", "api")
        .label("HTTPS")
        .edge("api", "db")
        .label("reads")
        .edge("api", "jobs")
        .stroke(Stroke::Dotted)
        .edge("jobs", "worker")
        .edge("worker", "db")
        .stroke(Stroke::Thick)
        .label("writes");

    let console = Console::builder().width(width).color_system(None).build();
    print!(
        "{}",
        console.render_to_string(&Diagram::new(graph).ascii(ascii))
    );
}

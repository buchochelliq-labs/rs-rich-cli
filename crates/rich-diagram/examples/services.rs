//! Draw a small service graph built in code:
//! `cargo run -p rs-rich-diagram --example services -- [WIDTH] [ascii]`.
//!
//! `-- --svg DIR` writes the guide's screenshots instead:
//! `DIR/guide_diagram-services.svg` (this graph), `-ascii.svg` (the same in
//! ASCII) and `-dot.svg` (a DOT source drawn by `Dot`).
use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_diagram::{Diagram, Direction, Dot, Graph, Shape, Stroke};

fn services() -> Graph {
    Graph::new(Direction::TopDown)
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
        .label("writes")
}

const PIPELINE: &str = "digraph pipeline {
  rankdir=LR
  node [style=rounded]
  checkout -> build -> test -> deploy
  test -> { lint docs } [style=dotted]
  deploy [label=\"Deploy\\nto prod\", shape=hexagon]
}";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--svg") {
        let dir = PathBuf::from(args.get(i + 1).expect("--svg takes a directory"));
        return screenshots(&dir);
    }
    let width = args.first().and_then(|w| w.parse().ok()).unwrap_or(80);
    let ascii = args.get(1).map(String::as_str) == Some("ascii");

    let console = Console::builder().width(width).color_system(None).build();
    print!(
        "{}",
        console.render_to_string(&Diagram::new(services()).ascii(ascii))
    );
}

/// The diagram guide's screenshots, as SVG.
fn screenshots(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).expect("create the SVG directory");
    let shots: [(&str, &str, usize, Box<dyn Fn(&Console)>); 3] = [
        (
            "services",
            "Diagram",
            40,
            Box::new(|c: &Console| c.print(&Diagram::new(services()))),
        ),
        (
            "ascii",
            "Diagram, ASCII",
            40,
            Box::new(|c: &Console| c.print(&Diagram::new(services()).ascii(true))),
        ),
        (
            "dot",
            "Dot",
            72,
            Box::new(|c: &Console| c.print(&Dot::new(PIPELINE))),
        ),
    ];
    for (name, title, width, draw) in shots {
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        let id = format!("guide_diagram-{name}");
        let svg = console.export_svg(title, &id, |c| draw(c));
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}

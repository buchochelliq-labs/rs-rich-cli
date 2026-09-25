//! Draw a Mermaid file as text: `cargo run -p rs-rich-mermaid --example draw -- FILE [WIDTH] [ascii]`.
use rich::Console;
use rich_mermaid::Mermaid;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: draw FILE [WIDTH]");
    let width = args.next().and_then(|w| w.parse().ok()).unwrap_or(100);
    let ascii = args.next().as_deref() == Some("ascii");
    let source = std::fs::read_to_string(path).expect("readable file");
    let console = Console::builder().width(width).color_system(None).build();
    print!(
        "{}",
        console.render_to_string(&Mermaid::new(source).ascii(ascii))
    );
}

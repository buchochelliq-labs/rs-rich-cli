//! Guide: Inspect — run: cargo run -p rs-rich-ext --example guide_inspect --features macros [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/inspect.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_inspect-<shot>.svg`.

use std::fmt;
use std::path::PathBuf;

use rich::{ColorSystem, Console};
use rich_ext::{Inspect, inspect};

/// Where shots go: the terminal, or one SVG per shot.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let dir = args
            .iter()
            .position(|arg| arg == "--svg")
            .and_then(|i| args.get(i + 1))
            .map(PathBuf::from);
        Shots { dir }
    }

    fn shot(&self, name: &str, width: usize, body: impl FnOnce(&Console)) {
        match &self.dir {
            None => {
                let console = Console::new();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .build();
                let id = format!("guide_inspect-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:point]
/// A point in the plane.
///
/// Only `help` shows this paragraph.
#[derive(Inspect)]
#[inspect(methods)] // list the methods `#[inspect]` records below
pub struct Point {
    pub x: i32,
    pub y: f64,
    pub label: String,
    secret: u8, // private: shown with `private`
}

#[inspect]
impl Point {
    /// The length, scaled.
    pub fn norm(&self, scale: f64) -> f64 {
        f64::from(self.x).hypot(self.y) * scale
    }
}
// --8<-- [end:point]

// --8<-- [start:fields]
/// A request, as a server sees it.
#[derive(Inspect)]
pub enum Request {
    /// Fetch a page.
    Get { path: String, retries: u8 },
    /// Store a body.
    Put(String, #[inspect(with = hex)] u32),
    /// Is the server up?
    Ping,
}

/// Show a checksum in hex, as `Debug::fmt` would be written.
fn hex(checksum: &u32, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{checksum:#010x}")
}

/// A queue of requests.
#[derive(Inspect)]
pub struct Queue<T> {
    /// What runs next.
    #[inspect(nested)]
    pub head: Option<T>,
    pub waiting: usize,
    #[inspect(skip)] // not Debug, and noise anyway
    pub lock: std::sync::Mutex<()>,
}
// --8<-- [end:fields]

fn main() {
    let shots = Shots::from_args();
    let point = Point {
        x: 3,
        y: 4.0,
        label: "origin".to_string(),
        secret: 7,
    };
    shots.shot("default", 64, |console| {
        // --8<-- [start:default]
        console.print(&Inspect::new(&point));
        // --8<-- [end:default]
    });
    shots.shot("methods", 72, |console| {
        // --8<-- [start:methods]
        console.print(&Inspect::new(&point).methods(true).private(true));
        // --8<-- [end:methods]
    });
    let queue = Queue {
        head: Some(Request::Put("/notes".into(), 0xc0ffee)),
        waiting: 4,
        lock: std::sync::Mutex::new(()),
    };
    shots.shot("nested", 72, |console| {
        // --8<-- [start:nested]
        console.print(&Inspect::new(&queue).types(true));
        // --8<-- [end:nested]
    });
    if shots.dir.is_none() {
        // --8<-- [start:print]
        // Print straight to standard output, as Python's `inspect(obj)` does.
        inspect(&point, rich_ext::inspect::InspectOptions::new().all(true));
        // The macro puts the expression in the title, and takes options.
        rich_ext::rich_inspect!(point, value = false, methods = true);
        // --8<-- [end:print]
        let _ = point.secret;
    }
}

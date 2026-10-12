//! Guide: Explorers, copying and live lists — run: cargo run -p rs-rich-interact --example guide_explorers --features data [-- --svg docs/media/guide]
//!
//! The pictures on `docs/guide/interact/explorers.md` come from this file.
//! Each is one frame: the component is started, given the keys of a
//! headless `Script`, and its view printed as the event loop would paint it.

use std::path::PathBuf;

use rich::{ColorSystem, Console, ConsoleOptions, Renderable, Segment, Style, Theme};
use rich_ext::data::{parse, Format};
use rich_interact::headless::{Script, Step};
use rich_interact::{Component, Context, DataExplorer, TableSelect, ThemePicker, TreeSelect};

const CONFIG: &str = "\
server:
  host: 0.0.0.0
  port: 8080
  tls:
    cert: /etc/app/cert.pem
    port: 8443
database:
  url: postgres://db.internal/app
  pool: 16
features: [search, export, audit]
";

// --8<-- [start:explorer]
fn explorer() -> DataExplorer {
    let document = parse(Format::Yaml, CONFIG).expect("valid YAML");
    DataExplorer::new("config.yaml", document)
}
// --8<-- [end:explorer]

// --8<-- [start:tree]
fn tree() -> TreeSelect<String> {
    let nodes = [
        (0, "rs-rich-cli"),
        (1, "crates"),
        (2, "rich"),
        (3, "src"),
        (4, "console.rs"),
        (4, "table.rs"),
        (2, "rich-ext"),
        (3, "src"),
        (4, "table"),
        (5, "rules.rs"),
        (5, "virtual.rs"),
        (1, "docs"),
        (2, "guide"),
        (3, "tables.md"),
    ];
    TreeSelect::new("File", nodes.map(|(depth, name)| (depth, name.to_string())))
        .breadcrumbs(true)
        .height(12)
}
// --8<-- [end:tree]

// --8<-- [start:table]
fn table() -> TableSelect<String> {
    let crates = [
        ("rs-rich", "0.0.9", "MIT", "the faithful port"),
        ("rs-rich-ext", "0.0.12", "MIT", "extensions"),
        ("rs-rich-data", "0.0.3", "MIT", "tabular data"),
        ("rs-rich-interact", "0.0.4", "MIT", "interactive components"),
    ];
    let rows = crates.map(|(name, version, licence, about)| {
        let cells = [name, version, licence, about].map(String::from).to_vec();
        (name.to_string(), cells)
    });
    TableSelect::new("Crate", ["crate", "version", "licence", "what"], rows)
}
// --8<-- [end:table]

// --8<-- [start:theme]
fn theme_picker() -> ThemePicker {
    let mut night = Theme::new();
    night.insert("info", Style::parse("bold blue").unwrap());
    night.insert("repr.number", Style::parse("bold magenta").unwrap());
    ThemePicker::new("Theme", [("default", Theme::new()), ("night", night)])
}
// --8<-- [end:theme]

fn main() {
    let shots = Shots::from_args("guide_explorers");
    shots.shot("explorer-search", 72, |c| {
        paint(c, explorer(), Script::new().text("port"), 14)
    });
    shots.shot("tree", 60, |c| {
        paint(c, tree(), Script::new().text("table"), 16)
    });
    shots.shot("table", 72, |c| {
        paint(
            c,
            table(),
            Script::new().keys("down ctrl+right ctrl+right"),
            8,
        )
    });
    shots.shot("theme", 80, |c| {
        paint(c, theme_picker(), Script::new().keys("down"), 16)
    });
}

/// Start `component`, give it the events of `script`, and print the view it
/// then paints at the console's width and `height` rows.
fn paint<C: Component>(console: &Console, mut component: C, script: Script, height: usize) {
    let context = Context {
        console,
        width: console.width(),
        height,
    };
    component.start(&context);
    for step in script.0 {
        if let Step::Event(event) = step {
            component.handle(&event, &context);
        }
    }
    console.print(&Lines(component.render(&context).lines));
}

/// A view's lines as a renderable.
struct Lines(Vec<Vec<Segment>>);

impl Renderable for Lines {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        let mut segments = Vec::new();
        for line in &self.0 {
            segments.extend(line.iter().cloned());
            segments.push(Segment::line());
        }
        segments
    }
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal at the shot's width.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    fn shot(&self, name: &str, width: usize, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            return f(&Console::builder().width(width).build());
        };
        let console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
}

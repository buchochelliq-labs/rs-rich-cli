//! Guide: Micro assets — run: cargo run -p rs-rich-micro --example guide_micro [-- --svg docs/media/guide]
//!
//! The pictures on `docs/guide/micro/index.md` come from this file: the same
//! markup with the built-in library, as the emoji fallback (what a pipe or a
//! log gets) and drawn as half-block cells (what a colour terminal without
//! an image protocol gets); and the two-row assets, in text and in a table.

use std::path::PathBuf;
use std::sync::Arc;

use rich::table::Cell;
use rich::{ColorSystem, Console, Table};
use rich_micro::{
    render_markup, CellPixels, FallbackPreference, MicroAssetRef, MicroGraphics, MicroMode,
    MicroRegistry, MicroRows, Selection,
};

// --8<-- [start:table]
/// A deploy's steps, each with a built-in asset before it.
fn steps(console: &Console, registry: &MicroRegistry) -> Table {
    let mut table = Table::new().title("Deploy v1.4.0");
    table.add_column("step");
    table.add_column("status");
    let rows = [
        ("Run the tests", ":micro:status/success: passed"),
        ("Lint", ":micro:status/warning: 2 warnings"),
        ("Fix the flaky one", ":micro:dev/bug: retried twice"),
        ("Build the package", ":micro:dev/package: 4.2 MB"),
        ("Sign the release", ":micro:status/error: key expired"),
    ];
    for (step, status) in rows {
        let (text, diagnostics) =
            render_markup(console, status, registry, FallbackPreference::Emoji);
        assert!(diagnostics.is_empty());
        table.add_row_text(vec![rich::Text::new(step), text]);
    }
    table
}
// --8<-- [end:table]

// --8<-- [start:fallback]
fn fallback(console: &Console) {
    let registry = MicroRegistry::builtin();
    // Printed as it is, each asset is its fallback, at exactly its width.
    console.print(&steps(console, &registry));
}
// --8<-- [end:fallback]

// --8<-- [start:blocks]
fn blocks(console: &Console) {
    let registry = Arc::new(MicroRegistry::builtin());
    // `MicroGraphics::detect` chooses for the terminal; a fixed mode here,
    // so the picture is the same on every machine.
    let selection = Selection::forced(MicroMode::Blocks, CellPixels::DEFAULT);
    let graphics = MicroGraphics::new(Arc::clone(&registry), selection);
    console.print(&graphics.view(steps(console, &registry)));
}
// --8<-- [end:blocks]

// --8<-- [start:rows]
fn rows(console: &Console) {
    let registry = Arc::new(MicroRegistry::builtin());
    // fun/trophy is 2x2 and fun/campfire 3x2: `MicroRows` reserves the row
    // below the line each one sits on.
    let (text, _) = render_markup(
        console,
        "Release :micro:fun/trophy: shipped, then the team at the :micro:fun/campfire:",
        &registry,
        FallbackPreference::Emoji,
    );
    // `MicroAssetRef` reserves its own rows: a table row grows to hold them.
    let mut table = Table::new();
    table.add_column("asset");
    table.add_column("size");
    for name in ["fun/trophy", "fun/campfire"] {
        let asset = MicroAssetRef::from_registry(&registry, name).expect("built in");
        let size = asset.asset().size().to_string();
        table.add_row_cells(vec![
            Cell::Renderable(Arc::new(asset)),
            Cell::Text(rich::Text::new(size)),
        ]);
    }
    // The fallback first, then half-blocks over both rows.
    console.print(&MicroRows::new(text.clone()));
    let selection = Selection::forced(MicroMode::Blocks, CellPixels::DEFAULT);
    let graphics = MicroGraphics::new(Arc::clone(&registry), selection);
    console.print(&graphics.view(MicroRows::new(text)));
    console.print(&graphics.view(table));
}
// --8<-- [end:rows]

fn main() {
    let shots = Shots::from_args("guide_micro");
    shots.shot("fallback", 48, fallback);
    shots.shot("blocks", 48, blocks);
    shots.shot("rows", 64, rows);
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
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
            return f(&Console::new());
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

//! Guide: Sorting, grouping and streaming tables — run: cargo run -p rs-rich-ext --example guide_tables_ext [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/tables.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_tables_ext-<shot>.svg`;
//! the live shot is one representative frame, rendered from the same table.

use std::path::PathBuf;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Justify, Text, Theme};
use rich_ext::live::{LiveCoordinator, LiveError};
use rich_ext::table::{
    Aggregate, Column, GroupBy, SortKey, StreamingTable, TableData, Value, Window,
};
use rich_ext::target::{RenderTarget, TargetKind};

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
        self.shot_with(name, width, false, body);
    }

    /// A shot on a console that can only render ASCII.
    fn shot_with(&self, name: &str, width: usize, ascii: bool, body: impl FnOnce(&Console)) {
        match &self.dir {
            None => {
                let console = Console::builder().ascii_only(ascii).build();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .ascii_only(ascii)
                    .build();
                let id = format!("guide_tables_ext-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:data]
fn services() -> TableData {
    let latency = |value: &Value| match value.as_f64() {
        Some(ms) => Text::new(format!("{ms:.0} ms")),
        None => Text::styled("-", "dim"),
    };
    let mut data = TableData::new([
        Column::new("service"),
        Column::new("region"),
        Column::new("errors").justify(Justify::Right),
        Column::new("p99").justify(Justify::Right).format(latency),
    ]);
    data.push([
        "api".into(),
        "eu".into(),
        Value::Int(3),
        Value::Float(120.0),
    ]);
    data.push(["web".into(), "us".into(), Value::Int(0), Value::Float(80.0)]);
    data.push(["db".into(), "eu".into(), Value::Int(7), Value::Null]);
    data.push([
        "cache".into(),
        "us".into(),
        Value::Int(1),
        Value::Float(4.0),
    ]);
    data.push([
        "worker-10".into(),
        "eu".into(),
        Value::Int(3),
        Value::Float(95.0),
    ]);
    data.push([
        "worker-9".into(),
        Value::Null,
        Value::Int(0),
        Value::Float(60.0),
    ]);
    data
}
// --8<-- [end:data]

// --8<-- [start:stream]
fn jobs() -> StreamingTable<&'static str> {
    let mut jobs = StreamingTable::new([
        Column::new("job"),
        Column::new("state"),
        Column::new("done")
            .justify(Justify::Right)
            .format(|v| match v {
                Value::Int(n) => Text::new(format!("{n}%")),
                _ => Text::new(""),
            }),
    ])
    .title("pipeline")
    .window(Window::Tail(4)); // log-like: keep the newest rows on screen

    // Keys are stable: an upsert of a known key updates its row in place.
    for job in ["fetch", "build", "test", "lint", "docs"] {
        jobs.upsert(job, [job.into(), "queued".into(), Value::Int(0)]);
    }
    jobs.update_cell(&"build", 1, "running");
    jobs.update_cell(&"build", 2, Value::Int(45));
    jobs.upsert("fetch", ["fetch".into(), "done".into(), Value::Int(100)]);
    jobs
}
// --8<-- [end:stream]

// --8<-- [start:live]
fn run_live(target: RenderTarget) -> Result<(), LiveError> {
    let mut jobs = jobs();
    let mut live = LiveCoordinator::new(std::io::stdout(), target.clone());
    let region = live.add(target.segments(&jobs))?;
    live.refresh()?;

    for done in [60, 80, 100] {
        jobs.update_cell(&"build", 2, Value::Int(done));
        // Only the `build` row is laid out again; the rest come from the cache.
        live.update(region.clone(), target.segments(&jobs))?;
        live.refresh()?;
    }
    jobs.update_cell(&"build", 1, "done");
    live.update(region, target.segments(&jobs))?;
    live.finish()?;

    let stats = jobs.stats();
    eprintln!(
        "{} frames, {} row renders, {} relayouts",
        stats.frames, stats.rows_rendered, stats.relayouts
    );
    Ok(())
}
// --8<-- [end:live]

fn main() {
    let shots = Shots::from_args();

    shots.shot("sort", 60, |console| {
        // --8<-- [start:sort]
        // Region ascending, then errors descending; the null region sorts last.
        let data = services().sort_by([SortKey::asc(1), SortKey::desc(2)]);
        console.print(&data);
        // --8<-- [end:sort]
    });

    shots.shot("natural", 60, |console| {
        // --8<-- [start:natural]
        // Natural order: `worker-9` before `worker-10`.
        console.print(&services().sort_by([SortKey::asc(0)]));
        // --8<-- [end:natural]
    });

    shots.shot("group", 60, |console| {
        // --8<-- [start:group]
        let data = services()
            .sort_by([SortKey::asc(1), SortKey::asc(0)])
            .group_by(
                GroupBy::new(1)
                    .aggregate(Aggregate::sum(2))
                    .aggregate(Aggregate::max(3)),
            )
            .totals(
                "all",
                [Aggregate::count(0), Aggregate::sum(2), Aggregate::mean(3)],
            );
        console.print(&data);
        // --8<-- [end:group]
    });

    shots.shot_with("ascii", 60, true, |console| {
        // --8<-- [start:ascii]
        // On an ASCII-only console (`Console::builder().ascii_only(true)`) the
        // indicators read `^`/`v` and the core swaps in an ASCII box.
        console.print(&services().sort_by([SortKey::desc(3)]));
        // --8<-- [end:ascii]
    });

    match &shots.dir {
        None => {
            // --8<-- [start:live-run]
            use std::io::IsTerminal;

            let interactive = std::io::stdout().is_terminal();
            let capabilities = TargetCapabilities {
                width: 60,
                height: 12,
                color_system: Some(ColorSystem::Standard),
                interactive,
                unicode: true,
                hyperlinks: false,
                sixel: Support::Unsupported,
            };
            let kind = if interactive {
                TargetKind::Terminal
            } else {
                TargetKind::PlainStream
            };
            let target = RenderTarget::new(kind, capabilities, Theme::default_theme());
            run_live(target).expect("live output");
            // --8<-- [end:live-run]
        }
        Some(_) => shots.shot("stream", 60, |console| {
            console.print(&jobs());
        }),
    }
}

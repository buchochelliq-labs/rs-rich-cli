//! Guide: Live regions and bounded layout — run: cargo run -p rs-rich-ext --example guide_live_layout [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/live-and-layout.md` come from this file.
//! With `--svg DIR` every shot is written as `DIR/guide_live_layout-<shot>.svg`;
//! the live shot is one representative frame, rendered from the same content.

use std::path::PathBuf;

use rich::protocol::{RenderEnvironment, Support, TargetCapabilities};
use rich::{ColorSystem, Console, Panel, Renderable, Syntax, Table, Text, Theme};
use rich_ext::layout::{
    allocate, Alignment, Axis, Constraint, LayoutNode, OverflowPolicy, Overflowing,
};
use rich_ext::live::{LiveCoordinator, LiveError};
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
                let id = format!("guide_live_layout-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

// --8<-- [start:dashboard-layout]
fn dashboard() -> LayoutNode {
    let header = LayoutNode::leaf(Box::new(Text::styled("acme deploy", "bold white on blue")))
        .height(Constraint::fixed(1));
    let sidebar = LayoutNode::leaf(Box::new(Text::new("api\nworker\nweb\ncron")))
        .width(Constraint {
            min: 6,
            max: Some(12),
            ..Constraint::default()
        })
        .content_width(); // as wide as its longest line, within min..=max
    let body = LayoutNode::leaf(Box::new(
        Panel::new(Box::new(Text::new(
            "v2.4.1 on 3 of 4 services\nweb: waiting for health check",
        )))
        .title("status"),
    ));
    let footer = LayoutNode::leaf(Box::new(Text::styled("q quit · r retry", "dim")))
        .height(Constraint::fixed(1))
        .align(Alignment::End, Alignment::Start); // right-aligned

    LayoutNode::split(
        Axis::Vertical,
        vec![
            header,
            LayoutNode::split(Axis::Horizontal, vec![sidebar, body]),
            footer,
        ],
    )
}
// --8<-- [end:dashboard-layout]

// --8<-- [start:target]
/// A destination described in full by the caller: nothing is detected.
fn target(kind: TargetKind, width: usize, height: usize) -> RenderTarget {
    let capabilities = TargetCapabilities {
        width,
        height,
        color_system: Some(ColorSystem::Standard),
        interactive: true,
        unicode: true,
        hyperlinks: true,
        sixel: Support::Unsupported,
    };
    // Policy is applied here: a PlainStream drops colour and links, anything
    // but a Terminal or Custom target is non-interactive.
    RenderTarget::new(kind, capabilities, Theme::default_theme())
}
// --8<-- [end:target]

/// One status row: a label and a coloured state.
fn status(label: &str, state: &str, style: &str) -> Text {
    let mut text = Text::new(format!("{label:<8}"));
    text.append(state, Some(style.into()));
    text
}

// --8<-- [start:live]
fn run_live(target: RenderTarget) -> Result<(), LiveError> {
    let mut live = LiveCoordinator::new(std::io::stdout(), target.clone());

    // Regions are drawn top to bottom, in the order they were added.
    let build = live.add(target.segments(&status("build", "running", "yellow")))?;
    let tests = live.add(target.segments(&status("tests", "queued", "dim")))?;
    live.refresh()?;

    // Ordinary output goes through the coordinator, above the regions.
    live.print(&target.segments(&Text::new("compiled 12 crates")))?;
    live.update(build, target.segments(&status("build", "done", "green")))?; // takes the id by value
    live.update(
        tests.clone(),
        target.segments(&status("tests", "running", "yellow")),
    )?;
    live.refresh()?;

    live.print(&target.segments(&Text::new("148 tests passed")))?;
    live.update(tests, target.segments(&status("tests", "done", "green")))?;
    live.refresh()?;

    // Restores the cursor; on a non-interactive target, writes the final state.
    live.finish()
}
// --8<-- [end:live]

fn main() {
    let shots = Shots::from_args();

    shots.shot("layout", 60, |console| {
        // --8<-- [start:layout]
        let layout = dashboard();
        layout.validate().expect("constraints are consistent");
        // A layout fills the height it is given: pass one explicitly.
        console.print_with(&layout, &console.options().update_dimensions(60, 8));
        // --8<-- [end:layout]
    });

    shots.shot("layout-narrow", 30, |console| {
        console.print_with(&dashboard(), &console.options().update_dimensions(30, 8));
    });

    shots.shot("allocate", 60, |console| {
        // --8<-- [start:allocate]
        let constraints = [
            Constraint::fixed(20), // wants exactly 20
            Constraint {
                min: 10,
                max: Some(30),
                ..Constraint::default()
            }, // flex 1, 10..=30
            Constraint {
                min: 5,
                flex: 2,
                ..Constraint::default()
            }, // flex 2, at least 5
        ];
        let mut table = Table::new();
        for heading in ["total", "sizes", "padding", "relaxed"] {
            table.add_column(heading);
        }
        for total in [100, 60, 30, 12] {
            let a = allocate(total, &constraints).expect("valid constraints");
            table.add_row(&[
                &total.to_string(),
                &format!("{:?}", a.sizes),
                &a.padding.to_string(),
                &format!("{:?}", a.relaxed),
            ]);
        }
        console.print(&table);
        // --8<-- [end:allocate]
    });

    shots.shot("overflow", 40, |console| {
        // --8<-- [start:overflow]
        let code = "let answer = compute_the_answer(universe, everything, 42);";
        for policy in [
            OverflowPolicy::Fold,
            OverflowPolicy::Crop,
            OverflowPolicy::Ellipsis,
        ] {
            console.print_str(&format!("[dim]{policy:?}[/]"));
            let syntax: Box<dyn Renderable> = Box::new(Syntax::new(code, "rust"));
            console.print(&Overflowing::new(syntax, policy));
        }
        // --8<-- [end:overflow]
    });

    shots.shot("targets", 100, |console| {
        // --8<-- [start:targets]
        let mut text = Text::styled("docs", "bold red");
        text.stylize(
            rich::Style::new().with_link("https://acme.dev".to_string()),
            0,
            4,
        );

        for kind in [
            TargetKind::Terminal,
            TargetKind::PlainStream,
            TargetKind::Capture,
        ] {
            let target = target(kind, 20, 1);
            let interactive = target.capabilities().interactive;
            let out = target.text(&text);
            console.print(&Text::new(format!(
                "{kind:?} (interactive: {interactive}): {out:?}"
            )));
        }
        // A zero-sized destination renders nothing at all.
        assert!(target(TargetKind::Terminal, 0, 1).text(&text).is_empty());
        // --8<-- [end:targets]
    });

    match &shots.dir {
        None => {
            // --8<-- [start:live-run]
            use std::io::IsTerminal;

            // Observe the process once, at the edge; everything below is explicit.
            let interactive = std::io::stdout().is_terminal();
            let kind = if interactive {
                TargetKind::Terminal
            } else {
                TargetKind::PlainStream
            };
            run_live(target(kind, 60, 10)).expect("live output");
            // --8<-- [end:live-run]
        }
        Some(_) => shots.shot("live", 60, |console| {
            // The frame after the second refresh: printed lines above, regions below.
            console.print(&Text::new("compiled 12 crates"));
            console.print(&status("build", "done", "green"));
            console.print(&status("tests", "running", "yellow"));
        }),
    }
}

//! Guide: Workflows — run: cargo run -p rs-rich-ext --example guide_workflow [-- --svg docs/media/guide]
//!
//! The snippets on `docs/guide/ext/workflows.md` come from this file. With
//! `--svg DIR` every shot is written as `DIR/guide_workflow-<shot>.svg`.
//! Every duration is fixed (hand-built records and a `ManualClock`) so the
//! screenshots are the same on every run; the real runner is shown in the
//! terminal only.

use std::path::PathBuf;
use std::time::Duration;

use rich::{ColorSystem, Console};
use rich_ext::theme::extended_theme;
use rich_ext::workflow::{CommandRecord, CommandStatus, CompletionSummary, ManualClock, TaskTree};

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
                let console = Console::builder().theme(extended_theme()).build();
                console.print_str(&format!("[dim]── {name} ──[/]"));
                body(&console);
            }
            Some(dir) => {
                let console = Console::builder()
                    .width(width)
                    .force_terminal(true)
                    .color_system(Some(ColorSystem::Truecolor))
                    .theme(extended_theme())
                    .build();
                let id = format!("guide_workflow-{name}");
                let svg = console.export_svg("rich-ext", &id, body);
                std::fs::create_dir_all(dir).expect("create the SVG directory");
                let path = dir.join(format!("{id}.svg"));
                std::fs::write(&path, svg).expect("write the SVG");
                eprintln!("wrote {}", path.display());
            }
        }
    }
}

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

// --8<-- [start:runner]
fn run_for_real(console: &Console) {
    use rich_ext::cancel::CancelToken;
    use rich_ext::workflow::CommandRunner;
    use std::process::Command;

    let cancel = CancelToken::new(); // cancel() it from anywhere to kill the child
    let record = CommandRunner::new()
        .cancel(cancel.clone())
        .on_update(|so_far| {
            // Redraw a live region here: so_far.view() shows a spinner.
            let _ = so_far.lines.len();
        })
        .run(Command::new("rustc").arg("--version"));
    console.print(&record);
}
// --8<-- [end:runner]

/// The deploy tree the task shots share, stopped mid-upload.
fn deploy() -> (TaskTree, ManualClock) {
    // --8<-- [start:tree]
    let clock = ManualClock::new(); // TaskTree::new() uses the wall clock
    let mut tree = TaskTree::with_clock(clock.clone()).title("Deploy");
    let build = tree.add(None, "build");
    let compile = tree.add(Some(build), "compile");
    let link = tree.add(Some(build), "link");
    let upload = tree.add(None, "upload");
    let assets = tree.add(Some(upload), "assets");
    let images = tree.add(Some(upload), "images");
    tree.add(None, "verify");

    tree.start(compile);
    clock.advance(ms(9_200));
    tree.succeed(compile).start(link);
    clock.advance(ms(3_200));
    tree.succeed(link).start(assets);
    clock.advance(ms(1_100));
    tree.warn(assets, "2 files unchanged").start(images);
    tree.progress(images, 21, Some(50));
    clock.advance(ms(1_900));
    // --8<-- [end:tree]
    (tree, clock)
}

fn main() {
    let shots = Shots::from_args();

    shots.shot("command", 70, |console| {
        // --8<-- [start:command]
        let record = CommandRecord::new("cargo", ["build", "--release"])
            .stdout("   Compiling rs-rich v0.0.11\n   Compiling rs-rich-ext v0.0.10")
            .stderr("warning: unused variable: `width`")
            .stdout("    Finished `release` profile [optimized] target(s)")
            .status(CommandStatus::Exited(0))
            .duration(ms(41_300));
        console.print(&record.view().tail(3));
        // --8<-- [end:command]
    });

    shots.shot("failure", 70, |console| {
        // --8<-- [start:failure]
        let record = CommandRecord::new("cargo", ["test", "-p", "demo"])
            .cwd("crates/demo")
            .stdout("running 3 tests\ntest parse::empty ... ok\ntest parse::quoted ... FAILED")
            .stderr("error: test failed, to rerun pass `--lib`")
            .status(CommandStatus::Exited(101))
            .duration(ms(4_250));
        console.print(
            &record
                .view()
                .help("rerun one test with `cargo test parse::quoted`"),
        );
        // --8<-- [end:failure]
    });

    shots.shot("running", 70, |console| {
        // --8<-- [start:running]
        use rich_ext::a11y::AccessibilityPolicy;

        let record = CommandRecord::new("npm", ["install"])
            .stdout("added 212 packages")
            .duration(ms(3_400)); // still running: the status defaults to Running
        console.print(&record); // a spinner frame chosen by the elapsed time
        console.print(&record.view().policy(&AccessibilityPolicy::reduced_motion()));
        // --8<-- [end:running]
    });

    shots.shot("tree", 70, |console| {
        let (tree, _) = deploy();
        // --8<-- [start:tree-print]
        console.print(&tree);
        console.print(&tree.view().collapse_finished(true).animate(false));
        // --8<-- [end:tree-print]
    });

    shots.shot("cancel", 70, |console| {
        let (mut tree, _) = deploy();
        // --8<-- [start:cancel]
        let upload = tree.roots()[1];
        let images = tree.children(upload)[1];
        let worker = tree.token(images); // hand this to the upload thread
        tree.cancel(upload);
        assert!(worker.is_cancelled());
        console.print(&tree.view().collapse_finished(true));
        // --8<-- [end:cancel]
    });

    shots.shot("summary", 70, |console| {
        let (mut tree, clock) = deploy();
        let upload = tree.roots()[1];
        let images = tree.children(upload)[1];
        let verify = tree.roots()[2];
        // --8<-- [start:summary]
        clock.advance(ms(2_600));
        tree.fail(images, "403 Forbidden")
            .skip(verify, Some("upload failed"));

        let summary = CompletionSummary::from(&tree)
            .next_step("check the bucket policy")
            .next_step("rerun with `deploy --resume`");
        console.print(&summary);
        // --8<-- [end:summary]
    });

    shots.shot("summary-plain", 70, |console| {
        // --8<-- [start:summary-plain]
        use rich_ext::a11y::SymbolSet;
        use rich_ext::workflow::State;

        let summary = CompletionSummary::new("Lint")
            .count(State::Succeeded, 118)
            .count(State::Warning, 2)
            .duration(ms(6_800))
            .next_step("run `lint --fix`")
            .symbols(SymbolSet::Ascii);
        console.print(&summary);
        // --8<-- [end:summary-plain]
    });

    if shots.dir.is_none() {
        run_for_real(&Console::builder().theme(extended_theme()).build());
    }
}

//! A live task tree, a real command and a completion summary.
//!
//! Run: `cargo run -p rs-rich-ext --example workflow`
//!
//! A simulated deploy advances on a timer while a `LiveCoordinator` region
//! redraws the tree; `rustc --version` runs through `CommandRunner` in the
//! same region. Everything honours `RICH_A11Y` (try
//! `RICH_A11Y=reduced-motion` or `RICH_A11Y=screen-reader`) and `NO_COLOR`.
//! Pass `--fail` to make the upload fail and see the summary's next steps.

use std::io::Stdout;
use std::process::Command;
use std::time::Duration;

use rich::{Console, Renderable, Text};
use rich_ext::a11y::AccessibilityPolicy;
use rich_ext::capabilities::{Capabilities, SystemEnvironment};
use rich_ext::live::{LiveCoordinator, LiveError, RegionId};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::theme::extended_theme;
use rich_ext::workflow::{CommandRunner, CompletionSummary, State, TaskTree};

/// The live display: one region redrawn from whatever is current.
struct Screen {
    live: LiveCoordinator<Stdout>,
    target: RenderTarget,
    region: RegionId,
    policy: AccessibilityPolicy,
}

impl Screen {
    fn show(&mut self, content: &dyn Renderable) -> Result<(), LiveError> {
        let segments = self.target.segments(content);
        self.live.update(self.region.clone(), segments)?;
        self.live.refresh()
    }

    /// Redraw `tree` every 100 ms for `ticks` ticks, so spinners move.
    fn wait(&mut self, tree: &TaskTree, ticks: u32) -> Result<(), LiveError> {
        for _ in 0..ticks {
            std::thread::sleep(Duration::from_millis(100));
            let view = tree.view().policy(&self.policy);
            self.show(&view)?;
        }
        Ok(())
    }

    fn print(&mut self, content: &dyn Renderable) -> Result<(), LiveError> {
        let segments = self.target.segments(content);
        self.live.print(&segments)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let fail = std::env::args().any(|arg| arg == "--fail");
    let policy = AccessibilityPolicy::from_env(&SystemEnvironment);
    let target = RenderTarget::new(
        TargetKind::Terminal,
        Capabilities::system().to_target_capabilities(),
        policy.theme(&extended_theme()),
    );
    let mut live = LiveCoordinator::new(std::io::stdout(), target.clone());
    let region = live.add(Vec::new())?;
    let mut screen = Screen {
        live,
        target,
        region,
        policy: policy.clone(),
    };

    let mut tree = TaskTree::new().title("Deploy");
    let build = tree.add(None, "build");
    let compile = tree.add(Some(build), "compile");
    let link = tree.add(Some(build), "link");
    let upload = tree.add(None, "upload");
    let assets = tree.add(Some(upload), "assets");
    let images = tree.add(Some(upload), "images");
    let verify = tree.add(None, "verify");

    for step in [compile, link] {
        tree.start(step);
        screen.wait(&tree, 8)?;
        tree.succeed(step);
    }
    screen.print(&Text::new("build finished"))?;
    tree.start(assets);
    screen.wait(&tree, 6)?;
    tree.succeed(assets).start(images);
    for done in (0..=40).step_by(4) {
        tree.progress(images, done, Some(40));
        if fail && done == 24 {
            tree.fail(images, "403 Forbidden");
            break;
        }
        screen.wait(&tree, 1)?;
    }
    if tree.state(images) == State::Running {
        tree.succeed(images);
    }

    if tree.state(upload) == State::Failed {
        tree.skip(verify, Some("upload failed"));
    } else {
        tree.start(verify);
        let token = tree.token(verify);
        let command = CommandRunner::new()
            .cancel(token)
            .on_update(|record| {
                let view = record.view().policy(&policy);
                let _ = screen.show(&view);
            })
            .run(Command::new("rustc").arg("--version"));
        screen.print(&command.view().policy(&policy))?;
        match command.state() {
            State::Succeeded => tree.succeed(verify),
            _ => tree.fail(verify, command.status.detail()),
        };
    }
    screen.live.remove(screen.region.clone())?;
    screen.live.finish()?;

    let console = policy
        .console_builder_with_theme(Console::builder(), &extended_theme())
        .build();
    console.print(&tree.view().policy(&policy).collapse_finished(true));
    let mut summary = CompletionSummary::from(&tree).policy(&policy);
    if tree.overall() == State::Failed {
        summary = summary
            .next_step("check the bucket policy")
            .next_step("rerun with --resume");
    }
    console.print(&summary);
    Ok(())
}

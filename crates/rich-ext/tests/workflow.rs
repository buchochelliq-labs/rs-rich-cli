//! Workflow renderables: command runs (#388), task trees (#389) and
//! completion summaries (#393).
//!
//! Rendering tests use hand-built records and a `ManualClock`, so every
//! duration and spinner frame is exact. The runner tests spawn this test
//! binary itself (`helper_process`, switched on by an environment variable),
//! which exists on every platform CI runs on — no shell, `echo` or `sleep`.
use std::io::Write;
use std::process::Command;
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console};
use rich_ext::a11y::{AccessibilityPolicy, SymbolSet};
use rich_ext::cancel::CancelToken;
use rich_ext::live::LiveCoordinator;
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::theme::extended_theme;
use rich_ext::workflow::{
    CommandRecord, CommandRunner, CommandStatus, CompletionSummary, ManualClock, OutputLine, State,
    Stream, SummaryItem, TaskTree,
};

const HELPER: &str = "RICH_EXT_WORKFLOW_HELPER";

fn plain(width: usize, renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(width)
        .build()
        .render_to_string(renderable)
}

fn colour(width: usize, renderable: &dyn rich::Renderable) -> String {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .theme(extended_theme())
        .build()
        .render_to_string(renderable)
}

fn secs(s: f64) -> Duration {
    Duration::from_secs_f64(s)
}

// ── commands ────────────────────────────────────────────────────────────

fn numbered(count: usize) -> CommandRecord {
    let mut record = CommandRecord::new("cargo", ["test"]);
    for n in 1..=count {
        record.push(Stream::Stdout, &format!("line {n}"));
    }
    record
}

#[test]
fn command_success_header_and_output() {
    let record = CommandRecord::new("cargo", ["build", "--release"])
        .stdout("Compiling demo v0.1.0\nFinished release")
        .stderr("warning: unused import")
        .status(CommandStatus::Exited(0))
        .duration(secs(2.5));
    assert_eq!(
        plain(60, &record),
        "✔ ok $ cargo build --release  2.5s\n\
         \x20 │ Compiling demo v0.1.0\n\
         \x20 │ Finished release\n\
         \x20 ! warning: unused import"
    );
    assert!(record.diagnostic().is_none());
    assert_eq!(
        record.output(Stream::Stdout),
        "Compiling demo v0.1.0\nFinished release"
    );
    assert_eq!(record.output(Stream::Stderr), "warning: unused import");
}

#[test]
fn command_output_folds_to_the_tail() {
    let record = numbered(5)
        .status(CommandStatus::Exited(0))
        .duration(secs(1.0));
    assert_eq!(
        plain(40, &record.view().tail(2)),
        "✔ ok $ cargo test  1.0s\n  … 3 lines hidden\n  │ line 4\n  │ line 5"
    );
    assert_eq!(
        plain(40, &record.view().tail(4)),
        "✔ ok $ cargo test  1.0s\n  … 1 line hidden\n  │ line 2\n  │ line 3\n  │ line 4\n  │ line 5"
    );
    assert_eq!(
        plain(40, &record.view().tail(0)),
        "✔ ok $ cargo test  1.0s\n  … 5 lines hidden"
    );
    // Ten lines by default; `show_all` lifts the limit.
    let long = numbered(12).status(CommandStatus::Exited(0));
    let text = plain(40, &long);
    assert!(text.contains("  … 2 lines hidden\n  │ line 3\n"), "{text}");
    assert!(!plain(40, &long.view().show_all()).contains("hidden"));
}

#[test]
fn failed_command_shows_everything_and_a_diagnostic() {
    let record = numbered(3)
        .stderr("error: test failed, to rerun pass `--lib`\n")
        .status(CommandStatus::Exited(101))
        .duration(secs(4.25));
    assert_eq!(
        plain(60, &record.view().tail(1)),
        "✖ error $ cargo test  exit 101  4.2s\n\
         \x20 │ line 1\n\
         \x20 │ line 2\n\
         \x20 │ line 3\n\
         \x20 ! error: test failed, to rerun pass `--lib`\n\
         error: `cargo test` exited with code 101\n\
         caused by: error: test failed, to rerun pass `--lib`"
    );
    // Folded anyway, with help lines in the diagnostic.
    assert_eq!(
        plain(
            60,
            &record
                .view()
                .tail(1)
                .full_on_failure(false)
                .help("rerun with `cargo test -- --nocapture`")
        ),
        "✖ error $ cargo test  exit 101  4.2s\n\
         \x20 … 3 lines hidden\n\
         \x20 ! error: test failed, to rerun pass `--lib`\n\
         error: `cargo test` exited with code 101\n\
         caused by: error: test failed, to rerun pass `--lib`\n\
         help: rerun with `cargo test -- --nocapture`"
    );
    assert_eq!(
        plain(
            60,
            &record
                .view()
                .show_diagnostic(false)
                .tail(0)
                .full_on_failure(false)
        ),
        "✖ error $ cargo test  exit 101  4.2s\n  … 4 lines hidden"
    );
}

#[test]
fn command_endings_have_their_own_headers() {
    let base = || CommandRecord::new("deploy", ["--env", "prod eu"]).duration(secs(3.0));
    let header = |record: CommandRecord| {
        plain(70, &record.view().show_diagnostic(false))
            .lines()
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        header(base().status(CommandStatus::Signalled(9))),
        "✖ error $ deploy --env 'prod eu'  signal 9  3.0s"
    );
    assert_eq!(
        header(base().status(CommandStatus::Cancelled)),
        "⊘ cancelled $ deploy --env 'prod eu'  3.0s"
    );
    assert_eq!(
        header(base().status(CommandStatus::FailedToStart("not found".into()))),
        "✖ error $ deploy --env 'prod eu'  failed to start"
    );
    let failed = base().status(CommandStatus::FailedToStart("not found".into()));
    assert_eq!(
        plain(70, &failed).lines().nth(1),
        Some("error: `deploy --env 'prod eu'` could not start: not found")
    );
    assert!(base()
        .status(CommandStatus::Cancelled)
        .diagnostic()
        .is_none());
    assert_eq!(
        CommandRecord::new("echo", ["it's", ""]).command_line(),
        r#"echo 'it'\''s' ''"#
    );
}

#[test]
fn running_command_spins_by_elapsed_time_or_stays_still() {
    // `dots` has ten frames 80 ms apart: 0.5 s is frame 6.
    let record = numbered(1).duration(secs(0.5));
    assert_eq!(record.status, CommandStatus::Running);
    assert_eq!(
        plain(40, &record),
        "⠦ running $ cargo test  500ms\n  │ line 1"
    );
    assert!(plain(40, &record.clone().duration(secs(0.0))).starts_with("⠋ running"));
    assert_eq!(
        plain(40, &record.view().animate(false)),
        "▶ running $ cargo test  500ms\n  │ line 1"
    );
    let reduced = AccessibilityPolicy::reduced_motion();
    assert!(plain(40, &record.view().policy(&reduced)).starts_with("▶ running $"));
    assert_eq!(
        plain(40, &record.view().symbols(SymbolSet::Ascii).tail(0)),
        "[RUN] $ cargo test  500ms\n  ... 1 line hidden"
    );
    let words = AccessibilityPolicy::screen_reader();
    assert!(plain(40, &record.view().policy(&words)).starts_with("running: $ cargo test"));
}

#[test]
fn command_cwd_ascii_output_and_ansi_input() {
    let record = CommandRecord::new("make", ["all"])
        .cwd("build")
        .stdout("\x1b[1mbold\x1b[0m done")
        .stderr("oops")
        .status(CommandStatus::Exited(0))
        .duration(Duration::from_micros(250));
    assert_eq!(
        plain(40, &record.view().symbols(SymbolSet::Words)),
        "ok: $ make all  250us\n  in build\n  | bold done\n  ! oops"
    );
    assert_eq!(
        plain(40, &record.view().show_cwd(false).tail(0)),
        "✔ ok $ make all  250µs\n  … 2 lines hidden"
    );
    let styled = colour(40, &record);
    // The output's own SGR survives as a style, stderr gets its theme style.
    assert!(styled.contains("\x1b[1mbold\x1b[0m"), "{styled:?}");
    assert!(
        styled.contains("\x1b[33m!\x1b[0m \x1b[33moops"),
        "{styled:?}"
    );
    assert!(styled.contains("\x1b[1;32m✔ ok\x1b[0m"), "{styled:?}");
}

#[test]
fn a_command_becomes_a_summary_item() {
    let record = CommandRecord::new("cargo", ["test"])
        .status(CommandStatus::Exited(101))
        .duration(secs(2.0));
    assert_eq!(
        SummaryItem::from(&record),
        SummaryItem::new(State::Failed, "cargo test")
            .duration(secs(2.0))
            .detail("exit 101")
    );
}

/// Not a real test: the runner tests spawn this binary with `HELPER` set, and
/// this is what the child does. Without the variable it returns at once.
#[test]
fn helper_process() {
    let Ok(mode) = std::env::var(HELPER) else {
        return;
    };
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    // libtest has printed `test helper_process ... ` without a newline.
    writeln!(out).unwrap();
    match mode.as_str() {
        "output" => {
            writeln!(out, "out one").unwrap();
            out.flush().unwrap();
            writeln!(err, "err one").unwrap();
            err.flush().unwrap();
            writeln!(out, "out two\r").unwrap();
            out.flush().unwrap();
            std::process::exit(3);
        }
        "sleep" => {
            writeln!(out, "ready").unwrap();
            out.flush().unwrap();
            std::thread::sleep(Duration::from_secs(60));
            std::process::exit(0);
        }
        _ => std::process::exit(2),
    }
}

fn helper(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().expect("the test binary"));
    command
        .args([
            "helper_process",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HELPER, mode);
    command
}

fn has(record: &CommandRecord, stream: Stream, text: &str) -> bool {
    record.lines.contains(&OutputLine {
        stream,
        text: text.into(),
    })
}

#[test]
fn runner_captures_both_streams_and_the_exit_code() {
    let mut updates = Vec::new();
    let record = CommandRunner::new()
        .on_update(|record| updates.push(record.status.clone()))
        .run(&mut helper("output"));
    assert_eq!(record.status, CommandStatus::Exited(3));
    assert!(has(&record, Stream::Stdout, "out one"), "{record:?}");
    assert!(has(&record, Stream::Stderr, "err one"), "{record:?}");
    // The `\r` before the newline is a line ending, not content.
    assert!(has(&record, Stream::Stdout, "out two"), "{record:?}");
    let position = |text: &str| record.lines.iter().position(|l| l.text == text);
    assert!(position("out one") < position("out two"));
    assert!(!updates.is_empty());
    assert!(updates
        .iter()
        .all(|status| *status == CommandStatus::Running));
    assert!(record.program.ends_with(
        std::env::current_exe()
            .unwrap()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
    ));
    assert_eq!(record.args[0], "helper_process");
    assert_eq!(record.state(), State::Failed);
}

#[test]
fn runner_cancellation_kills_the_child() {
    let token = CancelToken::new();
    let trigger = token.clone();
    let record = CommandRunner::new()
        .cancel(token)
        .tick(Duration::from_millis(20))
        .on_update(move |record| {
            if record.lines.iter().any(|line| line.text == "ready") {
                trigger.cancel();
            }
        })
        .run(&mut helper("sleep"));
    assert_eq!(record.status, CommandStatus::Cancelled);
    assert!(has(&record, Stream::Stdout, "ready"), "{record:?}");
    // Killed long before the helper's 60 s sleep ends.
    assert!(record.duration < Duration::from_secs(30), "{record:?}");
}

#[test]
fn runner_reports_a_program_that_cannot_start() {
    let record =
        CommandRunner::new().run(Command::new("rich-ext-no-such-program").current_dir("."));
    assert!(matches!(record.status, CommandStatus::FailedToStart(_)));
    assert_eq!(record.cwd.as_deref(), Some(std::path::Path::new(".")));
    let text = plain(80, &record.view().show_cwd(false));
    assert!(
        text.starts_with("✖ error $ rich-ext-no-such-program  failed to start\nerror: `rich-ext-no-such-program` could not start: "),
        "{text}"
    );
}

// ── task trees ──────────────────────────────────────────────────────────

/// Deploy: build (compile, link) done, upload running with progress,
/// verify pending.
fn deploy() -> (TaskTree, ManualClock) {
    let clock = ManualClock::new();
    let mut tree = TaskTree::with_clock(clock.clone()).title("Deploy");
    let build = tree.add(None, "build");
    let compile = tree.add(Some(build), "compile");
    let link = tree.add(Some(build), "link");
    let upload = tree.add(None, "upload");
    let assets = tree.add(Some(upload), "assets");
    let images = tree.add(Some(upload), "images");
    tree.add(None, "verify");
    tree.start(compile);
    clock.advance(secs(1.5));
    tree.succeed(compile).start(link);
    clock.advance(secs(0.5));
    tree.succeed(link).start(assets);
    clock.advance(secs(1.0));
    tree.succeed(assets)
        .start(images)
        .progress(images, 21, Some(50));
    clock.advance(secs(0.5));
    (tree, clock)
}

#[test]
fn a_task_tree_renders_guides_markers_progress_and_durations() {
    let (tree, _) = deploy();
    assert_eq!(
        plain(60, &tree),
        "Deploy\n\
         ├── ✔ ok build  2.0s\n\
         │   ├── ✔ ok compile  1.5s\n\
         │   └── ✔ ok link  500ms\n\
         ├── ⠇ running upload  1.5s\n\
         │   ├── ✔ ok assets  1.0s\n\
         │   └── ⠦ running images  21/50 (42%)  500ms\n\
         └── … pending verify"
    );
    assert_eq!(
        plain(60, &tree.view().collapse_finished(true).animate(false)),
        "Deploy\n\
         ├── ✔ ok build (+2 tasks)  2.0s\n\
         ├── ▶ running upload  1.5s\n\
         │   ├── ✔ ok assets  1.0s\n\
         │   └── ▶ running images  21/50 (42%)  500ms\n\
         └── … pending verify"
    );
    assert_eq!(
        plain(
            60,
            &tree.view().symbols(SymbolSet::Ascii).show_durations(false)
        ),
        "Deploy\n\
         +-- [OK] build\n\
         |   +-- [OK] compile\n\
         |   `-- [OK] link\n\
         +-- [RUN] upload\n\
         |   +-- [OK] assets\n\
         |   `-- [RUN] images  21/50 (42%)\n\
         `-- [PENDING] verify"
    );
}

#[test]
fn untitled_trees_start_at_the_roots_and_long_lines_end_in_an_ellipsis() {
    let clock = ManualClock::new();
    let mut tree = TaskTree::with_clock(clock.clone());
    let a = tree.add(None, "fetch");
    let b = tree.add(Some(a), "a rather long task label that will not fit");
    tree.skip(b, Some("cached"));
    assert_eq!(tree.state(a), State::Skipped);
    assert_eq!(
        plain(30, &tree),
        "↷ skipped fetch\n└── ↷ skipped a rather long t…"
    );
}

#[test]
fn parents_aggregate_their_children() {
    let clock = ManualClock::new();
    let mut tree = TaskTree::with_clock(clock);
    let parent = tree.add(None, "parent");
    let a = tree.add(Some(parent), "a");
    let b = tree.add(Some(parent), "b");
    assert_eq!(tree.state(parent), State::Pending);
    assert_eq!(tree.overall(), State::Pending);
    tree.start(parent);
    assert_eq!(tree.state(parent), State::Running, "a started parent runs");
    tree.succeed(a);
    assert_eq!(
        tree.state(parent),
        State::Running,
        "finished + pending runs"
    );
    tree.skip(b, None);
    assert_eq!(tree.state(parent), State::Succeeded, "skips do not count");
    tree.warn(b, "slow");
    assert_eq!(tree.state(parent), State::Warning);
    tree.fail(a, "boom");
    assert_eq!(tree.state(parent), State::Failed);
    assert_eq!(tree.overall(), State::Failed);
    assert!(tree.is_finished());
    assert_eq!(tree.get_note(a), Some("boom"));
    assert_eq!(
        tree.counts().into_iter().collect::<Vec<_>>(),
        vec![(State::Failed, 1), (State::Warning, 1)]
    );
    assert_eq!(tree.iter().collect::<Vec<_>>(), vec![parent, a, b]);
    assert_eq!(tree.leaves().collect::<Vec<_>>(), vec![a, b]);
    assert_eq!(tree.children(parent), &[a, b]);
    assert_eq!(tree.parent(a), Some(parent));
    assert_eq!(tree.roots(), &[parent]);
}

#[test]
fn cancelling_a_task_cancels_its_subtree_only() {
    let (mut tree, clock) = deploy();
    let upload = tree.roots()[1];
    let verify = tree.roots()[2];
    let images = tree.children(upload)[1];
    let worker = tree.token(images);
    tree.cancel(upload);
    assert!(worker.is_cancelled());
    assert!(!tree.token(verify).is_cancelled());
    assert_eq!(tree.state(images), State::Cancelled);
    assert_eq!(tree.state(upload), State::Cancelled);
    assert_eq!(tree.state(verify), State::Pending);
    clock.advance(secs(10.0));
    // Cancelled tasks stop their clocks.
    assert_eq!(tree.elapsed(upload), Some(secs(1.5)));
    assert_eq!(
        plain(60, &tree.view().collapse_finished(true)),
        "Deploy\n\
         ├── ✔ ok build (+2 tasks)  2.0s\n\
         ├── ⊘ cancelled upload  1.5s\n\
         │   ├── ✔ ok assets  1.0s\n\
         │   └── ⊘ cancelled images  21/50 (42%)  500ms\n\
         └── … pending verify"
    );
}

#[test]
fn cancellation_from_outside_is_picked_up() {
    let (mut tree, _) = deploy();
    tree.root_token().cancel();
    assert_eq!(
        tree.sync_cancelled(),
        3,
        "upload, images and verify; build had finished"
    );
    assert_eq!(tree.overall(), State::Cancelled);
    assert!(tree.is_finished());
    let (mut other, _) = deploy();
    other.cancel_all();
    assert_eq!(other.overall(), State::Cancelled);
}

#[test]
fn task_trees_follow_the_accessibility_policy_and_theme() {
    let (tree, _) = deploy();
    let words = plain(
        60,
        &tree.view().policy(&AccessibilityPolicy::screen_reader()),
    );
    assert!(words.contains("+-- running: upload  1.5s\n"), "{words}");
    assert!(words.contains("`-- pending: verify"), "{words}");
    let styled = colour(60, &tree);
    assert!(styled.contains("\x1b[1;32m✔ ok\x1b[0m"), "{styled:?}");
    assert!(
        styled.contains("\x1b[1;36m⠇ running\x1b[0m \x1b[1mupload"),
        "{styled:?}"
    );
    assert!(styled.contains("\x1b[1mDeploy\x1b[0m"), "{styled:?}");
}

#[test]
fn a_task_tree_fits_a_live_region() {
    let (tree, _) = deploy();
    let target = RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: 60,
            height: 20,
            color_system: None,
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        extended_theme(),
    );
    let mut out = Vec::new();
    {
        let mut live = LiveCoordinator::new(&mut out, target.clone());
        let region = live
            .add(target.segments(&tree.view().animate(false)))
            .unwrap();
        live.update(
            region,
            target.segments(&tree.view().collapse_finished(true).animate(false)),
        )
        .unwrap();
        live.finish().unwrap();
    }
    let text = String::from_utf8(out).unwrap();
    assert!(
        text.starts_with("Deploy\n├── ✔ ok build (+2 tasks)  2.0s\n"),
        "{text}"
    );
}

// ── summaries ───────────────────────────────────────────────────────────

#[test]
fn a_finished_tree_summarises_itself() {
    let (mut tree, clock) = deploy();
    let upload = tree.roots()[1];
    let images = tree.children(upload)[1];
    let verify = tree.roots()[2];
    clock.advance(secs(1.0));
    tree.fail(images, "403 Forbidden")
        .skip(verify, Some("upload failed"));
    let summary = CompletionSummary::from(&tree).next_step("check the bucket policy");
    assert_eq!(summary.overall(), State::Failed);
    assert_eq!(
        plain(60, &summary),
        "✖ error Deploy  4.5s\n\
         \x20 1 failed, 3 succeeded, 1 skipped\n\
         \x20 ✖ error upload / images  1.5s  403 Forbidden\n\
         \x20 ↷ skipped verify  upload failed\n\
         Next steps:\n\
         \x20 → check the bucket policy"
    );
    assert_eq!(
        plain(
            60,
            &summary
                .clone()
                .symbols(SymbolSet::Words)
                .show_all_items(true)
        ),
        "error: Deploy  4.5s\n\
         \x20 1 failed, 3 succeeded, 1 skipped\n\
         \x20 ok: build / compile  1.5s\n\
         \x20 ok: build / link  500ms\n\
         \x20 ok: upload / assets  1.0s\n\
         \x20 error: upload / images  1.5s  403 Forbidden\n\
         \x20 skipped: verify  upload failed\n\
         Next steps:\n\
         \x20 - check the bucket policy"
    );
    let styled = colour(60, &summary);
    assert!(
        styled.contains("\x1b[1;31m✖ error\x1b[0m \x1b[1mDeploy"),
        "{styled:?}"
    );
}

#[test]
fn summaries_by_hand_count_and_derive_their_status() {
    assert_eq!(
        plain(40, &CompletionSummary::new("Nothing to do")),
        "✔ ok Nothing to do"
    );
    let skipped = CompletionSummary::new("Install").item(State::Skipped, "cache");
    assert_eq!(skipped.overall(), State::Skipped);
    let counted = CompletionSummary::new("Lint")
        .count(State::Succeeded, 40)
        .count(State::Warning, 2)
        .duration(secs(75.0))
        .symbols(SymbolSet::Ascii);
    assert_eq!(counted.overall(), State::Warning);
    assert_eq!(
        plain(40, &counted),
        "[WARN] Lint  1m 15s\n  2 warnings, 40 succeeded"
    );
    let forced = CompletionSummary::new("Tests")
        .status(State::Cancelled)
        .push(SummaryItem::new(State::Running, "integration").duration(secs(3.0)))
        .policy(&AccessibilityPolicy::monochrome());
    assert_eq!(
        plain(40, &forced),
        "[CANCEL] Tests\n  1 running\n  [RUN] integration  3.0s"
    );
}

//! Guide: Diffs and test reports — run: cargo run -p rs-rich-ext --example guide_diff --features testing,test-report [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/diffs-and-test-reports.md` comes from
//! this file.
use std::path::PathBuf;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Text, Theme};
use rich_ext::diagnostic::Level;
use rich_ext::diff::git::{parse_unified, Annotation, PatchView, TemplateLinks};
use rich_ext::diff::test_report::{junit, libtest, TestReport};
use rich_ext::diff::{DiffView, Layout, SourceDiff, TextDiff};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::testing::RenderSnapshot;
use rich_ext::ConsoleExt;

const OLD: &str = "\
[server]
host = \"0.0.0.0\"
port = 8080
workers = 4
log = \"info\"
";

const NEW: &str = "\
[server]
host = \"0.0.0.0\"
port = 8443
workers = 4
log = \"debug\"
tls = true
";

// --8<-- [start:text-diff]
fn text_diff() {
    let diff = TextDiff::new(OLD, NEW).context(1);
    let (added, removed) = diff.stats();
    assert_eq!((added, removed), (3, 2));
    // The same hunks `diff -u` prints, minus timestamps.
    print!("{}", diff.unified("a/server.toml", "b/server.toml"));
}
// --8<-- [end:text-diff]

// --8<-- [start:unified]
fn show_unified(console: &Console) {
    let view = DiffView::new(OLD, NEW).titles("server.toml (old)", "server.toml (new)");
    console.print(&view);
}
// --8<-- [end:unified]

// --8<-- [start:side-by-side]
fn show_side_by_side(console: &Console) {
    let view = DiffView::new(OLD, NEW)
        .layout(Layout::SideBySide)
        .titles("before", "after")
        .context(1);
    console.print(&view);
}
// --8<-- [end:side-by-side]

// --8<-- [start:ansi]
fn show_ansi(console: &Console) {
    // Captured terminal output: the text is the same but "FAILED" lost its
    // colour. `DiffView::ansi` reports that as a style-only change (`~`).
    let before = "test parse ... \x1b[32mok\x1b[0m\ntest render ... \x1b[31mFAILED\x1b[0m\n";
    let after = "test parse ... \x1b[32mok\x1b[0m\ntest render ... FAILED\n";
    let view = DiffView::ansi(before, after);
    assert_eq!(view.style_changed_lines(), [2]);
    console.print(&view);
}
// --8<-- [end:ansi]

// --8<-- [start:snapshots]
fn show_snapshots(console: &Console) {
    let target = RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width: 30,
            height: 5,
            color_system: Some(ColorSystem::Truecolor),
            interactive: false,
            unicode: true,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    let old = RenderSnapshot::capture(&target, &Text::styled("Build passed", "bold green"));
    let new = RenderSnapshot::capture(&target, &Text::styled("Build passed", "green"));
    // Same plain text, different styling: only a snapshot diff sees it.
    assert_eq!(old.plain, new.plain);
    console.print(&DiffView::snapshots(&old, &new));
}
// --8<-- [end:snapshots]

// --8<-- [start:source]
fn show_source(console: &Console) {
    let old = "fn area(w: u32, h: u32) -> u32 {\n    w * h\n}\n";
    let new = "fn area(w: u64, h: u64) -> u64 {\n    w.saturating_mul(h)\n}\n";
    let diff = SourceDiff::new(old, new)
        .path("src/geometry.rs") // picks the Rust highlighter
        .link_template("vscode://file/{path}:{line}");
    console.print(&diff);
}
// --8<-- [end:source]

const PATCH: &str = "\
diff --git a/src/lib.rs b/src/lib.rs
index 3b18e51..a2c4f0d 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,5 +1,6 @@
 pub fn total(items: &[u32]) -> u32 {
-    let mut sum = 0;
+    let mut sum: u32 = 0;
+    let unused = items.len();
     for item in items {
         sum += item;
     }
diff --git a/README.md b/README.md
new file mode 100644
index 0000000..e69de29
--- /dev/null
+++ b/README.md
@@ -0,0 +1,2 @@
+# totals
+Adds numbers.
";

// --8<-- [start:patch]
fn show_patch(console: &Console) {
    let patch = parse_unified(PATCH).expect("valid patch");
    assert_eq!(patch.stats(), (4, 1)); // (additions, deletions) across files
    let view = PatchView::new(patch)
        .annotate(Annotation::new(
            "src/lib.rs",
            3,
            Level::Warning,
            "unused variable: `unused`",
        ))
        .links(
            TemplateLinks::new("https://github.com/{owner}/{repo}/blob/{rev}/{path}#L{line}")
                .file_template("https://github.com/{owner}/{repo}/blob/{rev}/{path}")
                .var("owner", "octo")
                .var("repo", "totals")
                .var("rev", "a2c4f0d"),
        );
    console.print(&view);
}
// --8<-- [end:patch]

const LIBTEST: &str = r#"     Running unittests src/lib.rs (target/debug/deps/totals-5e1c0a)
{ "type": "suite", "event": "started", "test_count": 3 }
{ "type": "test", "event": "started", "name": "math::adds" }
{ "type": "test", "name": "math::adds", "event": "ok", "exec_time": 0.000412 }
{ "type": "test", "event": "started", "name": "math::lists" }
{ "type": "test", "name": "math::lists", "event": "failed", "exec_time": 0.001203, "stdout": "thread 'math::lists' panicked at src/math.rs:21:9:\nassertion `left == right` failed\n  left: \"1\\n2\\n3\"\n right: \"1\\n2\\n4\"\n" }
{ "type": "test", "event": "ignored", "name": "math::slow", "message": "takes a minute" }
{ "type": "suite", "event": "failed", "passed": 1, "failed": 1, "ignored": 1, "measured": 0, "filtered_out": 0, "exec_time": 0.002514 }
"#;

const JUNIT: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuite name="api.tests" tests="2" failures="1" time="0.031">
  <testcase classname="api.tests.UserTest" name="creates_user" time="0.012"/>
  <testcase classname="api.tests.UserTest" name="rejects_duplicate" time="0.019">
    <failure message="expected:&lt;409&gt; but was:&lt;500&gt;">at UserTest.java:42</failure>
  </testcase>
</testsuite>
"#;

// --8<-- [start:test-report]
fn show_test_report(console: &Console) {
    // `cargo +nightly test -- -Z unstable-options --format json > results.json 2>&1`
    let run = libtest::parse(LIBTEST).expect("libtest JSON");
    assert!(!run.is_success());
    console.print(&TestReport::new(run).show_passed(true));

    // JUnit XML from Surefire, pytest, jest-junit, …
    let run = junit::parse(JUNIT).expect("JUnit XML");
    let totals = run.totals();
    assert_eq!((totals.passed, totals.failed), (1, 1));
    console.print(&TestReport::new(run));
}
// --8<-- [end:test-report]

// --8<-- [start:assert]
#[derive(serde::Serialize)]
struct Config {
    name: &'static str,
    ports: Vec<u16>,
}

fn assertion_message() -> String {
    let failure = std::panic::catch_unwind(|| {
        // In a test you would just write the assertion.
        rich_ext::assert_rich_json_eq!(
            Config {
                name: "api",
                ports: vec![80, 443]
            },
            Config {
                name: "api",
                ports: vec![80, 8443]
            },
            "config for {}",
            "production"
        );
    })
    .unwrap_err();
    failure
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default()
}
// --8<-- [end:assert]

fn main() {
    // Plain, deterministic assertion output (see RICH_ASSERT_COLOR).
    std::env::set_var("RICH_ASSERT_COLOR", "0");
    let shots = Shots::from_args();
    if !shots.svg() {
        text_diff();
    }
    shots.shot("unified", 60, "DiffView", show_unified);
    shots.shot("side-by-side", 80, "Side by side", show_side_by_side);
    shots.shot("ansi", 60, "DiffView::ansi", show_ansi);
    shots.shot("snapshots", 60, "DiffView::snapshots", show_snapshots);
    shots.shot("source", 72, "SourceDiff", show_source);
    shots.shot("patch", 80, "PatchView", show_patch);
    shots.shot("test-report", 80, "TestReport", show_test_report);

    // Keep the expected panic's default report off stderr.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let message = assertion_message();
    std::panic::set_hook(hook);
    shots.shot("assert", 72, "assert_rich_json_eq!", |c| {
        c.print(&Text::new(message.trim_end()))
    });
}

/// `--svg DIR` writes each shot as `DIR/guide_diff-<shot>.svg`; without it,
/// shots print to the terminal.
struct Shots {
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args() -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { dir }
    }

    fn svg(&self) -> bool {
        self.dir.is_some()
    }

    fn shot(&self, name: &str, width: usize, title: &str, f: impl FnOnce(&Console)) {
        let Some(dir) = &self.dir else {
            let mut console = Console::new();
            console.install_extensions();
            return f(&console);
        };
        let mut console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        console.install_extensions();
        let id = format!("guide_diff-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}

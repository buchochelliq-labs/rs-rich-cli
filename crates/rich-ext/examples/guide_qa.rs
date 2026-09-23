//! Guide: Quality assurance — run: cargo run -p rs-rich-ext --example guide_qa --features testing [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/qa.md` comes from this file. The
//! `#[test]` functions at the bottom compile with
//! `cargo test -p rs-rich-ext --features testing --example guide_qa`.
use std::path::{Path, PathBuf};
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Panel, Renderable, Table, Text, Theme};
use rich_ext::capabilities::ColorDepth;
use rich_ext::qa::bench::{
    bench_renderable, compare, BenchRun, CompareOptions, ComparisonView, Measurement,
};
use rich_ext::qa::explain::{explain, ExplanationView};
use rich_ext::qa::fuzz::{fuzz_with, Case, GenOptions, Invariants, Rendered};
use rich_ext::qa::lint::{lint, lint_markup, LintOptions, LintReport};
use rich_ext::qa::matrix::{self, CapabilityProfile, Fixture};
use rich_ext::qa::profile::{profile, AllocStats, Profile, ProfileOptions, ProfileReport, Timing};
use rich_ext::qa::stress::{stress, StressOptions};
use rich_ext::qa::{Approvals, Matrix, Screenshot};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::ConsoleExt;

// --8<-- [start:allocator]
use rich_ext::qa::profile::CountingAllocator;

// In a test or bench binary, never in a library.
#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::system();
// --8<-- [end:allocator]

// --8<-- [start:fixture]
fn status_table() -> Table {
    let mut table = Table::new().title("Deployments");
    table.add_column("Service");
    table.add_column("Region");
    table.add_column("Status");
    let status = |markup: &str| Text::from_markup(markup).expect("valid markup");
    table.add_row_text(vec![
        Text::new("api"),
        Text::new("eu-west-1"),
        status("[green]✔ ok[/]"),
    ]);
    table.add_row_text(vec![
        Text::new("worker"),
        Text::new("us-east-2"),
        status("[red]✖ failed[/]"),
    ]);
    table
}
// --8<-- [end:fixture]

// --8<-- [start:approvals]
fn approval_workflow(dir: &Path) -> String {
    // Two widths, colour and no colour: four shots.
    let matrix = Matrix::default()
        .widths([30, 60])
        .color([ColorDepth::TrueColor, ColorDepth::None])
        .unicode([true]);
    let approvals = Approvals::new(dir);

    // First run: nothing approved yet, so every shot is "missing" and
    // written as `<key>.new` for review.
    let shots = Screenshot::capture("status-table", &status_table(), &matrix);
    let first = approvals.check(&shots).expect("readable directory");
    assert_eq!(first.missing.len(), 4);

    // Accept them (what RICH_APPROVE=1 does during a check).
    approvals.approve_all().expect("writable directory");

    // The renderable changes: the approved files no longer match.
    let mut changed = status_table();
    changed.add_row(&["cron", "eu-west-1", "paused"]);
    let shots = Screenshot::capture("status-table", &changed, &matrix.clone().widths([60]));
    let outcome = approvals.check(&shots).expect("readable directory");
    assert!(!outcome.is_ok());
    outcome.report() // a summary, then a diff per mismatch
}
// --8<-- [end:approvals]

// --8<-- [start:stress]
fn show_stress(console: &Console) {
    let report = stress(&status_table(), &StressOptions::widths([3, 8, 20, 40, 80]));
    if !report.is_clean() {
        console.print(&report);
    }
}
// --8<-- [end:stress]

// --8<-- [start:lint]
fn show_lint(console: &Console) {
    // Markup is checked against a theme: typos get the nearest name.
    let theme = rich_ext::extended_theme();
    let mut findings = lint_markup("[bold gren]ready[/] [repr.nubmer]42[/]", &theme);

    // A render is checked at each width for a described target.
    let options = LintOptions::default().widths([12, 40]).unicode(false);
    let status =
        Text::from_markup("[green]● ok[/] [red]● failed[/] [link=htps://example.com]docs[/]")
            .unwrap();
    findings.extend(lint(&status, &options));

    let report = LintReport::new(findings);
    let _json = report.to_json(); // for CI annotations
    console.print(&report);
}
// --8<-- [end:lint]

// --8<-- [start:explain]
fn show_explain(console: &Console) {
    let mut table = Table::new();
    table.add_column("Step");
    table.add_column("Result");
    table.add_row_text(vec![
        Text::from_markup("[#ff8700]deploy[/]").unwrap(),
        Text::new("finished in 42s with no errors"),
    ]);
    // A 16-colour, ASCII-only terminal, 24 columns wide.
    let target = RenderTarget::new(
        TargetKind::Terminal,
        TargetCapabilities {
            width: 24,
            height: 24,
            color_system: Some(ColorSystem::Standard),
            interactive: true,
            unicode: false,
            hyperlinks: false,
            sixel: Support::Unsupported,
        },
        Theme::default_theme(),
    );
    let explanation = explain(&table, &target);
    console.print(&ExplanationView::new(&explanation));
}
// --8<-- [end:explain]

// --8<-- [start:profile]
fn run_profile(console: &Console) -> Profile {
    let options = ProfileOptions::default()
        .iterations(50)
        .width(60)
        .frame(60, 10); // also cost a Live-style 60x10 refresh
    let profile = profile(&status_table(), console, &options);
    // Filled in because this binary installs CountingAllocator.
    assert!(profile.allocations.is_some());
    profile
}

fn show_profile(console: &Console, profile: &Profile) {
    console.print(&ProfileReport::new(profile));
}
// --8<-- [end:profile]

/// Timings in screenshots are made up, so the guide does not change with
/// the machine that generated it.
fn synthetic_profile() -> Profile {
    let micros = |us: &[u64]| {
        Timing::from_samples(
            &us.iter()
                .map(|u| Duration::from_micros(*u))
                .collect::<Vec<_>>(),
        )
    };
    Profile {
        width: 60,
        iterations: 50,
        measure_time: micros(&[11, 12, 12, 13, 19]),
        render_time: micros(&[58, 61, 62, 64, 90]),
        segments: 118,
        lines: 9,
        cells: 540,
        bytes: 1_392,
        frame: Some(rich_ext::qa::profile::FrameCost {
            width: 60,
            height: 10,
            time: micros(&[70, 72, 75, 77, 101]),
            bytes: 1_486,
        }),
        allocations: Some(AllocStats {
            allocations: 412,
            deallocations: 409,
            bytes: 38_016,
            live_bytes: 96,
        }),
    }
}

// --8<-- [start:fuzz]
fn show_fuzz(console: &Console) {
    // Every node kind, widths 4 to 40.
    let options = GenOptions {
        min_width: 4,
        max_width: 40,
        ..GenOptions::default()
    };
    // The default invariants, plus one of our own.
    let invariants = Invariants::default().custom("no-tabs", |r: &Rendered<'_>| {
        match r.lines.iter().position(|line| line.contains('\t')) {
            Some(i) => Err(format!("line {} contains a tab", i + 1)),
            None => Ok(()),
        }
    });
    let report = fuzz_with(7, 60, &options, &invariants);
    console.print(&report);
    for failure in &report.failures {
        // Rebuild the exact case from its seed and index...
        let case = Case::generate(failure.seed, failure.case_index, &options);
        assert_eq!(case, failure.case);
        // ...or paste the shrunk reproduction into a test.
        println!("{}", failure.minimized.as_deref().unwrap_or(""));
    }
}
// --8<-- [end:fuzz]

// --8<-- [start:matrix]
fn fixture_table() -> Box<dyn Renderable> {
    Box::new(status_table())
}

fn fixture_panel() -> Box<dyn Renderable> {
    Box::new(Panel::new(Box::new(Text::styled("all systems go", "green"))).title("Status"))
}

const FIXTURES: &[Fixture] = &[("table", fixture_table), ("panel", fixture_panel)];

fn show_matrix(console: &Console) {
    // 12 colour × unicode × link combinations, then dumb, ci,
    // windows-terminal and screen-reader.
    let profiles = CapabilityProfile::standard();
    let report = matrix::regression(FIXTURES, &profiles, 40);
    console.print(&report);
    for cell in report.failures() {
        eprintln!("{} under {}: {:?}", cell.fixture, cell.profile, cell.status);
    }
}
// --8<-- [end:matrix]

// --8<-- [start:bench]
fn record_bench(path: &Path) -> std::io::Result<()> {
    let table = status_table();
    let run = BenchRun::new(vec![
        bench_renderable("status-table@80", &table, 80),
        bench_renderable("status-table@40", &table, 40),
    ]);
    run.save(path) // JSON; `rich bench compare` reads it
}

fn show_comparison(console: &Console, baseline: &BenchRun, candidate: &BenchRun) -> bool {
    let options = CompareOptions {
        threshold_pct: 10.0,
        ..CompareOptions::default()
    };
    let comparison = compare(baseline, candidate, &options);
    console.print(&ComparisonView::new(&comparison));
    comparison.has_regressions()
}
// --8<-- [end:bench]

/// Made-up measurements for the screenshot.
fn synthetic_runs() -> (BenchRun, BenchRun) {
    let m = |name: &str, mean: f64, stddev: f64| Measurement {
        name: name.into(),
        samples: 200,
        mean,
        median: mean,
        stddev,
        p95: mean * 1.08,
        min: mean * 0.93,
        max: mean * 1.2,
        unit: "ns".into(),
    };
    let baseline = BenchRun::new(vec![
        m("table@80", 41_200.0, 900.0),
        m("table@40", 38_900.0, 850.0),
        m("markdown@80", 212_000.0, 4_100.0),
        m("syntax@80", 530_000.0, 9_800.0),
    ]);
    let candidate = BenchRun::new(vec![
        m("table@80", 47_900.0, 950.0),
        m("table@40", 38_400.0, 800.0),
        m("markdown@80", 171_000.0, 3_900.0),
        m("tree@80", 18_300.0, 400.0),
    ]);
    (baseline, candidate)
}

fn main() {
    let shots = Shots::from_args();
    let scratch = std::env::temp_dir().join("rs-rich-guide-qa");
    let _ = std::fs::remove_dir_all(&scratch);

    // Plain, deterministic diffs in the approval report (see RICH_ASSERT_COLOR).
    std::env::set_var("RICH_ASSERT_COLOR", "0");
    let report = approval_workflow(&scratch.join("screenshots"));
    shots.shot("approvals", 72, "Approvals::check", |c| {
        c.print(&Text::new(report.trim_end()))
    });
    shots.shot("stress", 80, "stress", show_stress);
    shots.shot("lint", 90, "lint", show_lint);
    shots.shot("explain", 90, "explain", show_explain);
    if shots.svg() {
        let profile = synthetic_profile();
        shots.shot("profile", 72, "profile", |c| show_profile(c, &profile));
    } else {
        let console = Console::new();
        let profile = run_profile(&console);
        show_profile(&console, &profile);
    }
    shots.shot("fuzz", 90, "fuzz", show_fuzz);
    shots.shot("matrix", 100, "matrix", show_matrix);
    let (baseline, candidate) = if shots.svg() {
        synthetic_runs()
    } else {
        let path = scratch.join("bench.json");
        record_bench(&path).expect("writable scratch directory");
        let run = BenchRun::load(&path).expect("just written");
        (run.clone(), run)
    };
    shots.shot("bench", 90, "bench compare", |c| {
        show_comparison(c, &baseline, &candidate);
    });
    let _ = std::fs::remove_dir_all(&scratch);
}

// --8<-- [start:tests]
#[cfg(test)]
mod tests {
    use super::*;
    use rich_ext::qa::fuzz::NodeKind;
    use rich_ext::qa::screenshot::assert_screenshots;

    #[test]
    fn status_table_screenshots() {
        // Approved files live in tests/screenshots/status-table/; run with
        // RICH_APPROVE=1 to accept new output.
        assert_screenshots("status-table", &status_table());
    }

    #[test]
    fn status_table_survives_common_widths() {
        let report = stress(&status_table(), &StressOptions::widths([20, 40, 80, 120]));
        assert!(
            report.is_clean(),
            "{}",
            Console::new().render_to_string(&report)
        );
    }

    #[test]
    fn status_table_lints_clean() {
        let findings = lint(&status_table(), &LintOptions::default());
        let report = LintReport::new(findings);
        assert!(!report.has_errors(), "{}", report.to_json());
    }

    #[test]
    fn fixtures_hold_across_unicode_terminals() {
        // The status symbols need Unicode: ASCII profiles fail.
        let profiles: Vec<_> = CapabilityProfile::standard()
            .into_iter()
            .filter(|p| p.unicode())
            .collect();
        let report = matrix::regression(FIXTURES, &profiles, 40);
        assert!(report.is_ok());
    }

    #[test]
    fn fuzzing_finds_nothing() {
        let options = GenOptions::default().kinds([NodeKind::Text, NodeKind::Table]);
        let report = fuzz_with(2024, 300, &options, &Invariants::default());
        assert!(report.is_clean(), "{:#?}", report.failures);
    }
}
// --8<-- [end:tests]

/// `--svg DIR` writes each shot as `DIR/guide_qa-<shot>.svg`; without it,
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
        let id = format!("guide_qa-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}

#![cfg(feature = "testing")]
//! QA tooling: screenshots, stress, lint, explain, profile, fuzz, matrix and
//! bench.

use std::path::PathBuf;
use std::time::Duration;

use rich::protocol::{Support, TargetCapabilities};
use rich::{
    ColorSystem, Console, ConsoleOptions, Overflow, Panel, Renderable, Segment, Style, Table, Text,
    Theme,
};
use rich_ext::capabilities::{Capabilities, ColorDepth, MapEnvironment, Overrides};
use rich_ext::qa::bench::{
    bench_renderable_with, compare, Bench, BenchRun, CompareOptions, ComparisonView, Measurement,
    Verdict,
};
use rich_ext::qa::explain::{explain, explain_with_report, EventKind, ExplanationView};
use rich_ext::qa::fuzz::{fuzz, fuzz_with, Case, GenOptions, Invariants, Node, NodeKind, Rendered};
use rich_ext::qa::lint::{lint, lint_markup, LintOptions, LintReport, Rule, Severity};
use rich_ext::qa::matrix::{self, CapabilityProfile, CellStatus, Fixture};
use rich_ext::qa::profile::{profile, CountingAllocator, ProfileOptions, ProfileReport};
use rich_ext::qa::screenshot::{assert_screenshots_with, Encoding};
use rich_ext::qa::stress::{stress, IssueKind, StressOptions};
use rich_ext::qa::{Approvals, Matrix, Screenshot};
use rich_ext::target::{RenderTarget, TargetKind};

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::system();

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-ext-qa-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn render(renderable: &dyn Renderable, width: usize) -> String {
    let console = Console::builder()
        .width(width)
        .no_color(true)
        .force_terminal(false)
        .build();
    console.render_to_string(renderable)
}

fn target(width: usize, color: ColorDepth, unicode: bool, hyperlinks: bool) -> RenderTarget {
    RenderTarget::new(
        TargetKind::Capture,
        TargetCapabilities {
            width,
            height: 25,
            color_system: color.color_system(),
            interactive: false,
            unicode,
            hyperlinks,
            sixel: Support::Unsupported,
        },
        rich_ext::theme::extended_theme(),
    )
}

/// Emits fixed segments whatever the width.
struct Segs(Vec<Segment>);
impl Renderable for Segs {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        self.0.clone()
    }
}

fn styled(text: &str, style: &str) -> Segment {
    Segment::new(text, Some(Style::parse(style).unwrap()))
}

fn linked(text: &str, url: &str) -> Segment {
    Segment::new(text, Some(Style::new().with_link(url)))
}

fn small_table() -> Table {
    let mut table = Table::new();
    table.add_column("Name");
    table.add_column("Value");
    table.add_row(&["alpha", "1"]);
    table.add_row(&["a longer cell here", "22"]);
    table
}

// ---------------------------------------------------------------- screenshots

#[test]
fn screenshot_keys_and_encodings() {
    let text = Text::styled("hello", "bold red");
    let shots = Screenshot::capture("greeting", &text, &Matrix::default());
    let keys: Vec<&str> = shots.iter().map(|s| s.key.as_str()).collect();
    assert_eq!(keys.len(), 12);
    assert_eq!(keys[0], "greeting@40.truecolor.unicode");
    assert_eq!(keys[1], "greeting@40.truecolor.ascii");
    assert_eq!(keys[2], "greeting@40.none.unicode");
    assert!(keys.contains(&"greeting@120.none.ascii"));
    let color = &shots[0];
    assert_eq!(color.encoding, Encoding::Escaped);
    assert!(color.text.contains("\x1b[1;31mhello"), "{:?}", color.text);
    assert!(color.file_contents().starts_with("\\e[1;31mhello\\e[0m"));
    let plain = &shots[2];
    assert_eq!(
        (plain.encoding, plain.text.as_str()),
        (Encoding::Plain, "hello")
    );
    assert_eq!(plain.snapshot.plain, "hello");

    let panel = Panel::new(Box::new(Text::new("hi")));
    let shots = Screenshot::capture("panel", &panel, &Matrix::single(10).unicode([true, false]));
    assert!(shots[0].snapshot.plain.contains('╭'));
    assert!(shots[1].snapshot.plain.starts_with('+'));
    // A Panel does not grow to a screen height.
    assert_eq!(shots[0].snapshot.plain.lines().count(), 3);
}

#[test]
fn escaped_encoding_round_trips() {
    let raw = "a\\b \x1b[31mred\x1b[0m \x07bell\ttab\nline two";
    let encoded = Encoding::Escaped.encode(raw);
    assert_eq!(
        encoded,
        "a\\\\b \\e[31mred\\e[0m \\x07bell\\x09tab\nline two\n"
    );
    assert_eq!(Encoding::Escaped.decode(&encoded), raw);
    assert_eq!(
        Encoding::Plain.decode(&Encoding::Plain.encode("x\ny")),
        "x\ny"
    );
    assert_eq!(Encoding::Plain.decode("x\r\ny\r\n"), "x\ny");
}

#[test]
fn approvals_flow() {
    let dir = temp_dir("approvals");
    let approvals = Approvals::new(&dir).approving(false);
    let matrix = Matrix::single(20).color([ColorDepth::TrueColor, ColorDepth::None]);
    let shots = Screenshot::capture("t", &Text::styled("one two", "green"), &matrix);

    // Nothing approved yet: both missing, `.new` files written.
    let outcome = approvals.check(&shots).unwrap();
    assert_eq!(
        outcome.missing,
        ["t@20.truecolor.unicode", "t@20.none.unicode"]
    );
    assert!(!outcome.is_ok());
    let pending = approvals.pending_path(&shots[0]);
    assert!(
        pending.ends_with("t/t@20.truecolor.unicode.new"),
        "{pending:?}"
    );
    assert!(pending.exists());
    assert!(std::fs::read_to_string(&pending).unwrap().contains("\\e["));

    // Approve them: the same shots now match and the `.new` files are gone.
    let approved = approvals.approve_all().unwrap();
    assert_eq!(approved.len(), 2);
    let outcome = approvals.check(&shots).unwrap();
    assert_eq!(outcome.matched.len(), 2);
    assert!(outcome.is_ok());
    assert!(!pending.exists());
    assert_eq!(
        std::fs::read_to_string(approvals.path(&shots[1])).unwrap(),
        "one two\n"
    );

    // A change is a mismatch with a diff, and writes `.new` again.
    let changed = Screenshot::capture("t", &Text::styled("one three", "red"), &matrix);
    let outcome = approvals.check(&changed).unwrap();
    assert_eq!(outcome.mismatched.len(), 2);
    let plain = outcome
        .mismatched
        .iter()
        .find(|m| m.encoding == Encoding::Plain)
        .unwrap();
    assert_eq!(
        (plain.approved.as_str(), plain.actual.as_str()),
        ("one two", "one three")
    );
    let diff = render(&plain.view().line_numbers(false), 60);
    assert!(
        diff.contains("- one two") && diff.contains("+ one three"),
        "{diff}"
    );
    assert!(outcome.report().contains("mismatch: t@20.none.unicode"));
    assert!(approvals.pending_path(&changed[1]).exists());

    // Approving mode accepts the change.
    let outcome = Approvals::new(&dir)
        .approving(true)
        .check(&changed)
        .unwrap();
    assert_eq!(outcome.approved.len(), 2);
    assert!(approvals.check(&changed).unwrap().is_ok());

    // The assertion helper panics on differences and names the fix.
    let err = std::panic::catch_unwind(|| {
        assert_screenshots_with(&approvals, "t", &Text::new("different"), &matrix)
    })
    .unwrap_err();
    let message = err.downcast_ref::<String>().unwrap();
    assert!(message.contains("RICH_APPROVE=1"), "{message}");
    assert_screenshots_with(&approvals, "t", &Text::styled("one three", "red"), &matrix);
    let _ = std::fs::remove_dir_all(&dir);
}

// --------------------------------------------------------------------- stress

/// Ignores the width entirely.
struct WidthIgnorer;
impl Renderable for WidthIgnorer {
    fn rich_render(&self, _: &Console, _: &ConsoleOptions) -> Vec<Segment> {
        vec![Segment::new("x".repeat(50), None)]
    }
}

/// Panics when squeezed.
struct Fragile;
impl Renderable for Fragile {
    fn rich_render(&self, _: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        assert!(o.max_width >= 5, "too narrow: {}", o.max_width);
        vec![Segment::new("ok", None)]
    }
}

/// Crops silently, claiming it can go down to one cell.
struct Clipper;
impl Renderable for Clipper {
    fn rich_render(&self, _: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let text: String = "abcdefghijklmnopq".chars().take(o.max_width).collect();
        vec![Segment::new(text, None)]
    }
    fn measure(&self, _: &Console, _: &ConsoleOptions) -> rich::measure::Measurement {
        rich::measure::Measurement::new(1, 17)
    }
}

/// The same words on more lines when wider.
struct Growing;
impl Renderable for Growing {
    fn rich_render(&self, _: &Console, o: &ConsoleOptions) -> Vec<Segment> {
        let sep = if o.max_width >= 20 { "\n" } else { " " };
        vec![Segment::new(["a", "b", "c", "d"].join(sep), None)]
    }
}

#[test]
fn stress_finds_broken_renderables() {
    let report = stress(&WidthIgnorer, &StressOptions::default());
    let widths: Vec<usize> = report
        .of(IssueKind::Overflow)
        .filter(|i| i.height.is_none())
        .map(|i| i.width)
        .collect();
    assert_eq!(widths, [1, 2, 3, 4, 10, 20, 40]);
    assert!(report.of(IssueKind::Overflow).any(|i| i.height == Some(5)));
    assert_eq!(report.renders, 30);

    let report = stress(&Fragile, &StressOptions::widths([1, 4, 5, 10]));
    let panics: Vec<usize> = report.of(IssueKind::Panic).map(|i| i.width).collect();
    assert_eq!(panics, [1, 4]);
    assert!(report.issues[0].detail.contains("too narrow"));

    let report = stress(&Clipper, &StressOptions::widths([5, 10, 40]));
    let clipped: Vec<usize> = report.of(IssueKind::Clipping).map(|i| i.width).collect();
    assert_eq!(clipped, [5, 10]);
    assert!(report
        .issues
        .iter()
        .any(|i| i.detail.contains("7 characters lost")));

    let report = stress(&Growing, &StressOptions::widths([10, 20, 40, 80]));
    let unstable: Vec<&str> = report
        .of(IssueKind::UnstableWrapping)
        .map(|i| i.detail.as_str())
        .collect();
    assert_eq!(unstable, ["1 line at width 10, 4 lines at width 20"]);

    let out = render(&report, 80);
    assert!(
        out.contains("unstable wrapping") && out.contains("4 renders, 1 issue"),
        "{out}"
    );
}

#[test]
fn stress_on_core_renderables_is_clean_or_documented() {
    let text = Text::new("The quick brown fox jumps over the lazy dog, twice. 日本語 🙂");
    assert!(stress(&text, &StressOptions::default()).is_clean());

    // Table and Panel borders cannot fit in one to three cells, and leave no
    // room for content up to four; upstream overflows and loses content
    // there too (a top-level print crops). Nothing else, at any height.
    let table = small_table();
    let panel = Panel::new(Box::new(Text::new("hello world, this is a panel"))).title("T");
    for renderable in [&table as &dyn Renderable, &panel] {
        let report = stress(renderable, &StressOptions::default());
        assert!(report.of(IssueKind::Overflow).count() > 0);
        for issue in &report.issues {
            assert!(
                matches!(issue.kind, IssueKind::Overflow | IssueKind::Clipping),
                "{issue:?}"
            );
            assert!(issue.width <= 4, "{issue:?}");
        }
    }
    let report = stress(&small_table(), &StressOptions::widths([10, 20, 40, 80]));
    assert!(report.is_clean(), "{report:?}");
}

// ----------------------------------------------------------------------- lint

fn rules(findings: &[rich_ext::qa::lint::LintFinding]) -> Vec<(Rule, Severity)> {
    let mut r: Vec<_> = findings.iter().map(|f| (f.rule, f.severity)).collect();
    r.sort();
    r
}

#[test]
fn lint_finds_each_rule() {
    let no_layout = LintOptions::default().layout(false);

    let links = Segs(vec![
        linked("a", ""),
        Segment::new(" ", None),
        linked("b", "docs/index.html"),
        Segment::new(" ", None),
        linked("c", "javascript:alert(1)"),
        Segment::new(" ", None),
        linked("  ", "https://example.com/empty"),
        Segment::new(" ", None),
        linked("d", "gopher://example.com"),
        Segment::new(" ", None),
        linked("e", "https://"),
    ]);
    let findings = lint(&links, &LintOptions::capable().layout(false));
    let broken: Vec<Severity> = findings
        .iter()
        .filter(|f| f.rule == Rule::BrokenHyperlink)
        .map(|f| f.severity)
        .collect();
    assert_eq!(
        broken,
        [
            Severity::Error,
            Severity::Warning,
            Severity::Error,
            Severity::Warning,
            Severity::Error
        ]
    );
    assert!(findings.iter().any(|f| f.rule == Rule::EmptyLinkText));

    // Links on a target without OSC 8.
    let hidden = Segs(vec![linked("docs", "https://example.com/docs")]);
    let findings = lint(&hidden, &no_layout);
    assert_eq!(
        rules(&findings),
        [(Rule::HyperlinksUnsupported, Severity::Warning)]
    );
    let visible = Segs(vec![linked("https://example.com", "https://example.com")]);
    assert_eq!(
        rules(&lint(&visible, &no_layout)),
        [(Rule::HyperlinksUnsupported, Severity::Info)]
    );

    let blink = Segs(vec![styled("ALERT", "blink")]);
    assert_eq!(
        rules(&lint(&blink, &no_layout)),
        [(Rule::Blink, Severity::Warning)]
    );

    let dots = Segs(vec![
        styled("●", "red"),
        Segment::new(" api\n", None),
        styled("●", "green"),
        Segment::new(" db", None),
    ]);
    let findings = lint(&dots, &no_layout);
    assert_eq!(
        rules(&findings),
        [(Rule::ColorOnlyDistinction, Severity::Warning)]
    );
    assert!(findings[0].message.contains("●"), "{}", findings[0].message);
    assert_eq!(findings[0].line, Some(1));
    // Different words, or a symbol that also differs in attributes, are fine.
    let words = Segs(vec![styled("ok", "green"), styled(" failed", "red")]);
    assert!(lint(&words, &no_layout).is_empty());
    let marked = Segs(vec![styled("●", "red bold"), styled(" ●", "green")]);
    assert!(lint(&marked, &no_layout).is_empty());

    let deep = Segs(vec![styled("orange", "#ff8700")]);
    let findings = lint(&deep, &no_layout);
    assert_eq!(rules(&findings), [(Rule::ColorDepth, Severity::Info)]);
    assert!(
        findings[0].message.contains("#ff8700 → "),
        "{}",
        findings[0].message
    );
    let collide = Segs(vec![styled("a", "#ff0000"), styled("b", "#ee0000")]);
    assert_eq!(
        rules(&lint(&collide, &no_layout)),
        [(Rule::ColorDepth, Severity::Warning)]
    );
    assert!(lint(&deep, &LintOptions::capable().layout(false)).is_empty());

    let glyphs = Segs(vec![Segment::new("✔ done", None)]);
    let findings = lint(&glyphs, &no_layout.clone().unicode(false));
    assert_eq!(rules(&findings), [(Rule::NonAsciiGlyph, Severity::Warning)]);

    let findings = lint(&WidthIgnorer, &LintOptions::default());
    assert!(findings
        .iter()
        .any(|f| f.rule == Rule::Overflow && f.severity == Severity::Error));
}

#[test]
fn lint_is_quiet_on_clean_input() {
    let text = Text::new("hello world, all is well");
    assert!(lint(&text, &LintOptions::default()).is_empty());
    let panel = Panel::new(Box::new(Text::styled("status: ok", "green")));
    assert!(lint(&panel, &LintOptions::default()).is_empty());
    assert!(lint(&small_table(), &LintOptions::default()).is_empty());
    let theme = Theme::default_theme();
    assert!(lint_markup(
        "[bold]hi[/] [repr.number]1[/] [link=https://x.org]x[/]",
        &theme
    )
    .is_empty());
}

#[test]
fn lint_markup_and_json() {
    let theme = Theme::default_theme();
    let findings = lint_markup("[bold]ok[/] [repr.nubmer]1[/]", &theme);
    assert_eq!(rules(&findings), [(Rule::UnknownStyle, Severity::Error)]);
    assert!(
        findings[0].message.contains("did you mean \"repr.number\""),
        "{}",
        findings[0].message
    );
    assert_eq!(
        rules(&lint_markup("[bold]x[/italic]", &theme)),
        [(Rule::MarkupError, Severity::Error)]
    );
    let findings = lint_markup("[link=/relative]x[/] [blink]y[/]", &theme);
    assert_eq!(
        rules(&findings),
        [
            (Rule::BrokenHyperlink, Severity::Warning),
            (Rule::Blink, Severity::Warning)
        ]
    );

    let report = LintReport::new(findings);
    let json = report.to_json();
    assert!(json.contains("\"rule\": \"broken_hyperlink\""), "{json}");
    assert!(json.contains("\"severity\": \"warning\""));
    assert_eq!(LintReport::from_json(&json).unwrap(), report);
    let out = render(&report, 100);
    assert!(
        out.contains("broken-hyperlink") && out.contains("2 findings"),
        "{out}"
    );
}

// -------------------------------------------------------------------- explain

#[test]
fn explain_reports_wrapping_and_truncation() {
    let text = Text::new("alpha beta gamma delta epsilon zeta eta theta");
    let e = explain(&text, &target(12, ColorDepth::TrueColor, true, true));
    assert_eq!(e.natural_width, 45);
    let wrapped: Vec<&rich_ext::qa::explain::Event> = e.of(EventKind::Wrapped).collect();
    assert_eq!(wrapped.len(), 1);
    assert!(wrapped[0]
        .summary
        .starts_with("source line 1 wrapped onto lines 1–"));
    assert!(wrapped[0].lines.len() >= 4);
    assert!(!e.has(EventKind::Truncated));

    let cut = Text::new("supercalifragilistic word")
        .overflow(Overflow::Ellipsis)
        .no_wrap(true);
    let e = explain(&cut, &target(10, ColorDepth::TrueColor, true, true));
    let t: Vec<_> = e.of(EventKind::Truncated).collect();
    assert_eq!(t.len(), 1, "{e:?}");
    assert_eq!(t[0].lines, [1]);

    let crop = Text::new("supercalifragilistic word")
        .overflow(Overflow::Crop)
        .no_wrap(true);
    let e = explain(&crop, &target(10, ColorDepth::TrueColor, true, true));
    assert!(e.has(EventKind::Cropped), "{e:?}");
    assert!(!e.has(EventKind::Truncated));

    let short = Text::new("fits");
    let e = explain(&short, &target(40, ColorDepth::TrueColor, true, true));
    let kinds: Vec<EventKind> = e.events.iter().map(|e| e.kind).collect();
    assert_eq!(kinds, [EventKind::Fidelity]);
    assert_eq!(e.events[0].summary, "fidelity rich");
}

#[test]
fn explain_reports_colour_unicode_links_and_capabilities() {
    let orange =
        Text::from_markup("[#ff8700]orange[/] [red]red[/] [link=https://x.org]x[/]").unwrap();
    let e = explain(&orange, &target(40, ColorDepth::Ansi16, true, false));
    let chains: Vec<&str> = e
        .of(EventKind::ColorDowngraded)
        .map(|e| e.summary.as_str())
        .collect();
    assert_eq!(chains.len(), 1, "{chains:?}");
    assert!(chains[0].starts_with("#ff8700 → 208 → "), "{}", chains[0]);
    assert!(e.has(EventKind::HyperlinksDropped));

    let e = explain(&orange, &target(40, ColorDepth::Ansi256, true, true));
    let chains: Vec<&str> = e
        .of(EventKind::ColorDowngraded)
        .map(|e| e.summary.as_str())
        .collect();
    assert_eq!(chains, ["#ff8700 → 208"]);
    let e = explain(&orange, &target(40, ColorDepth::None, true, true));
    assert!(e.has(EventKind::ColorRemoved));
    assert_eq!(e.events[0].summary, "fidelity plain");

    let panel = Panel::new(Box::new(Text::new("✔ done")));
    let e = explain(&panel, &target(20, ColorDepth::TrueColor, false, false));
    let fallbacks: Vec<&str> = e
        .of(EventKind::UnicodeFallback)
        .map(|e| e.summary.as_str())
        .collect();
    assert_eq!(fallbacks.len(), 2, "{fallbacks:?}");
    assert!(fallbacks[0].contains("╭ → +"), "{}", fallbacks[0]);
    assert!(fallbacks[1].contains('✔'));
    assert_eq!(e.events[0].summary, "fidelity ascii");

    let report = Capabilities::detect_with(
        &MapEnvironment::tty().var("TERM", "xterm-256color"),
        &Overrides {
            width: Some(30),
            ..Overrides::default()
        },
    );
    let t = RenderTarget::new(
        TargetKind::Capture,
        report.to_target_capabilities(),
        Theme::default_theme(),
    );
    let e = explain_with_report(&panel, &t, Some(&report));
    let caps: Vec<&str> = e
        .of(EventKind::Capability)
        .map(|e| e.summary.as_str())
        .collect();
    assert!(
        caps.iter().any(|c| c.starts_with("color = 256")),
        "{caps:?}"
    );
    assert!(e
        .of(EventKind::Capability)
        .any(|c| c.summary.starts_with("width") && c.reason.contains("override")));

    let out = render(&ExplanationView::new(&e), 120);
    assert!(
        out.contains("capability") && out.contains("events"),
        "{out}"
    );
}

// -------------------------------------------------------------------- profile

#[test]
fn profile_returns_sane_shapes() {
    let console = Console::builder()
        .width(60)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    let text =
        Text::from_markup("[bold]hello[/] world, this wraps a little at forty cells").unwrap();
    let options = ProfileOptions::default()
        .iterations(5)
        .width(40)
        .frame(30, 4);
    let p = profile(&text, &console, &options);
    assert_eq!((p.width, p.iterations), (40, 5));
    assert_eq!(p.render_time.samples, 5);
    assert_eq!(p.measure_time.samples, 5);
    assert!(p.render_time.min <= p.render_time.median && p.render_time.median <= p.render_time.max);
    assert!(p.render_time.p95 <= p.render_time.max);
    assert_eq!(p.lines, 2);
    assert!(p.segments >= 2 && p.cells > 40 && p.bytes > p.cells);
    let frame = p.frame.unwrap();
    assert_eq!((frame.width, frame.height, frame.time.samples), (30, 4, 5));
    assert!(frame.bytes >= 30 * 4);
    // This binary installs CountingAllocator, so allocations are counted.
    let allocs = p.allocations.expect("CountingAllocator is installed");
    assert!(allocs.allocations > 0 && allocs.bytes > 0);

    let out = render(&ProfileReport::new(&p), 100);
    assert!(
        out.contains("render") && out.contains("frame 30x4"),
        "{out}"
    );
    assert!(out.contains("allocations ("), "{out}");
}

// ----------------------------------------------------------------------- fuzz

/// Core renderables without the two kinds that expose known core bugs (see
/// the ignored tests below), at widths where borders can fit.
fn core_options() -> GenOptions {
    GenOptions {
        min_width: 16,
        ..GenOptions::default().without(NodeKind::Columns)
    }
}

#[test]
fn fuzz_is_deterministic_for_a_seed() {
    let a = fuzz(42, 40, &Invariants::default());
    let b = fuzz(42, 40, &Invariants::default());
    assert_eq!(a, b);
    let c1 = Case::generate(42, 7, &GenOptions::default());
    assert_eq!(c1, Case::generate(42, 7, &GenOptions::default()));
    assert_ne!(c1, Case::generate(43, 7, &GenOptions::default()));
    assert!(c1.to_rust().starts_with("// seed 42, case 7, width "));
}

#[test]
fn fuzz_core_renderables_hold_invariants() {
    let start = std::time::Instant::now();
    let report = fuzz_with(2024, 300, &core_options(), &Invariants::default());
    let elapsed = start.elapsed();
    let failures: Vec<String> = report
        .failures
        .iter()
        .map(|f| {
            format!(
                "case {} ({}): {}\n{}",
                f.case_index,
                f.invariant,
                f.description,
                f.minimized.clone().unwrap_or_else(|| f.case.to_rust())
            )
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
    assert_eq!(report.cases, 300);
    assert!(elapsed < Duration::from_secs(30), "took {elapsed:?}");
    assert!(render(&report, 80).contains("300 cases, 0 failures"));
}

#[test]
fn fuzz_minimizes_a_failing_invariant() {
    // Synthetic: no output line may contain an `a`.
    let no_a = Invariants::none().custom("no-a", |r: &Rendered<'_>| {
        match r.lines.iter().position(|l| l.contains('a')) {
            Some(i) => Err(format!("line {} has an a", i + 1)),
            None => Ok(()),
        }
    });
    let report = fuzz_with(9, 30, &GenOptions::default(), &no_a);
    assert!(report.failures.len() >= 5, "{}", report.failures.len());
    let mut shrunk = 0;
    for f in &report.failures {
        assert_eq!(f.invariant, "no-a");
        assert_eq!(
            f.case,
            Case::generate(9, f.case_index, &GenOptions::default())
        );
        if let Some(small) = &f.minimized_case {
            shrunk += 1;
            assert!(small.node.size() <= f.case.node.size());
            assert!(small.width <= f.case.width);
            assert!(small.node.to_rust().len() < f.case.node.to_rust().len());
            // The shrunk case still fails, and its reproduction says so.
            assert!(no_a.check(&small.node, small.width).is_some());
            assert!(f
                .minimized
                .as_ref()
                .unwrap()
                .contains(&small.node.to_rust()));
        }
    }
    assert!(shrunk * 2 >= report.failures.len(), "{shrunk}");
    // Shrinking usually gets to the single character that fails.
    assert!(report.failures.iter().any(|f| f
        .minimized_case
        .as_ref()
        .is_some_and(|m| matches!(&m.node, Node::Text(t) if t.markup() == "a"))));
}

fn widest(renderable: &dyn Renderable, width: usize) -> usize {
    let console = Console::builder().width(width).build();
    let segments = renderable.rich_render(&console, &console.options().update_width(width));
    Segment::split_lines(&segments)
        .iter()
        .map(|l| l.iter().map(Segment::cell_length).sum::<usize>())
        .max()
        .unwrap_or(0)
}

/// Found by `fuzz` with the default generation options: `Columns` sizes a column to its
/// widest item without capping it at the width, and shows only an item's
/// first line. Upstream lays items out in a `Table.grid`, which wraps or
/// ellipsises them: `superca…` here.
#[test]
#[ignore = "core bug: Columns does not fit items wider than the width"]
fn core_bug_columns_overflow() {
    let columns = rich::Columns::new(vec!["supercalifragilistic".to_string()]);
    assert_eq!(widest(&columns, 8), 8);
    let columns = rich::Columns::new(vec!["name name name".to_string()]);
    assert_eq!(widest(&columns, 13), 13);
}

/// Found by `fuzz` with the default generation options: `Tree` draws guides past the width when the
/// width is below the guide depth. Upstream renders nothing for a child
/// label with no room, so it never exceeds the width (`roo`/`t` at width 3).
#[test]
#[ignore = "core bug: Tree guides overflow narrow widths"]
fn core_bug_tree_guides_overflow() {
    let mut tree = rich::Tree::new("root");
    tree.add("child one").add("grand");
    assert!(widest(&tree, 3) <= 3, "{}", widest(&tree, 3));
    assert!(widest(&tree, 4) <= 4, "{}", widest(&tree, 4));
}

// --------------------------------------------------------------------- matrix

fn fixture_text() -> Box<dyn Renderable> {
    Box::new(
        Text::from_markup("[bold]Deploy[/] [red]failed[/]: see [link=https://example.com]docs[/]")
            .unwrap(),
    )
}
fn fixture_table() -> Box<dyn Renderable> {
    Box::new(small_table())
}
fn fixture_panel() -> Box<dyn Renderable> {
    Box::new(Panel::new(Box::new(Text::styled("all systems go", "green"))).title("Status"))
}
fn fixture_check() -> Box<dyn Renderable> {
    Box::new(Text::new("✔ done"))
}

const FIXTURES: &[Fixture] = &[
    ("text", fixture_text),
    ("table", fixture_table),
    ("panel", fixture_panel),
];

#[test]
fn matrix_structural_checks_pass() {
    let profiles = CapabilityProfile::standard();
    assert_eq!(profiles.len(), 16);
    assert_eq!(profiles[0].name, "16-unicode-links");
    assert!(profiles.iter().any(|p| p.name == "screen-reader"));
    let report = matrix::regression(FIXTURES, &profiles, 40);
    let failures: Vec<_> = report.failures().collect();
    assert!(failures.is_empty(), "{failures:#?}");
    assert_eq!(report.cells.len(), 48);

    let out = render(&report, 60);
    assert!(
        out.contains('✔') && out.contains("48 passed, 0 failed"),
        "{out}"
    );
    let ascii = Console::builder()
        .width(60)
        .ascii_only(true)
        .no_color(true)
        .build();
    assert!(ascii.render_to_string(&report).contains("[OK]"));

    // A non-ASCII fixture fails exactly the ASCII profiles.
    let report = matrix::regression(&[("check", fixture_check)], &profiles, 40);
    for cell in &report.cells {
        let ascii = cell.profile.contains("ascii") || cell.profile == "dumb";
        match &cell.status {
            CellStatus::Fail(reasons) => {
                assert!(ascii, "{cell:?}");
                assert!(reasons[0].contains("non-ASCII"));
            }
            _ => assert!(!ascii, "{cell:?}"),
        }
    }
    assert!(render(&report, 80).contains('✖'));
}

#[test]
fn matrix_compares_against_approvals() {
    let dir = temp_dir("matrix");
    let profiles = [
        CapabilityProfile::dumb(),
        CapabilityProfile::windows_terminal(),
    ];
    let approvals = Approvals::new(&dir).approving(false);
    let report = matrix::run(FIXTURES, &profiles, 40, Some(&approvals)).unwrap();
    assert!(report.cells.iter().all(|c| c.status == CellStatus::Missing));
    approvals.approve_all().unwrap();
    let report = matrix::run(FIXTURES, &profiles, 40, Some(&approvals)).unwrap();
    assert!(
        report.is_ok(),
        "{:?}",
        report.failures().collect::<Vec<_>>()
    );
    assert!(dir.join("text/text@40.windows-terminal.txt").exists());
    let approved = std::fs::read_to_string(dir.join("text/text@40.windows-terminal.txt")).unwrap();
    assert!(
        approved.contains("\\e]8;;https://example.com"),
        "{approved}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------------- bench

fn m(name: &str, mean: f64, stddev: f64) -> Measurement {
    Measurement {
        name: name.into(),
        samples: 10,
        mean,
        median: mean,
        stddev,
        p95: mean * 1.1,
        min: mean * 0.9,
        max: mean * 1.2,
        unit: "ns".into(),
    }
}

#[test]
fn bench_compare_verdicts() {
    let baseline = BenchRun::new(vec![
        m("slower", 100.0, 1.0),
        m("faster", 100.0, 1.0),
        m("noisy", 100.0, 50.0),
        m("same", 100.0, 1.0),
        m("gone", 100.0, 1.0),
    ]);
    let candidate = BenchRun::new(vec![
        m("slower", 120.0, 1.0),
        m("faster", 80.0, 1.0),
        m("noisy", 120.0, 50.0),
        m("same", 103.0, 1.0),
        m("fresh", 50.0, 1.0),
    ]);
    let c = compare(&baseline, &candidate, &CompareOptions::default());
    let verdicts: Vec<(&str, Verdict)> = c
        .rows
        .iter()
        .map(|r| (r.name.as_str(), r.verdict))
        .collect();
    assert_eq!(
        verdicts,
        [
            ("slower", Verdict::Regression),
            ("faster", Verdict::Improvement),
            ("noisy", Verdict::Unchanged),
            ("same", Verdict::Unchanged),
            ("gone", Verdict::Removed),
            ("fresh", Verdict::New),
        ]
    );
    assert!((c.row("slower").unwrap().change_pct.unwrap() - 20.0).abs() < 1e-9);
    assert!(c.has_regressions());
    // Without noise discounting the noisy one regresses too.
    let strict = CompareOptions {
        noise: rich_ext::qa::bench::Noise::Ignore,
        ..CompareOptions::default()
    };
    assert_eq!(
        compare(&baseline, &candidate, &strict)
            .row("noisy")
            .unwrap()
            .verdict,
        Verdict::Regression
    );

    let out = render(&ComparisonView::new(&c), 100);
    assert!(
        out.contains("+20.0%") && out.contains("regression"),
        "{out}"
    );
    assert!(
        out.contains('█') && out.contains("1 regression, 1 improvement"),
        "{out}"
    );
    let ascii = render(&ComparisonView::new(&c).ascii(true), 100);
    assert!(!ascii.contains('█') && ascii.contains('#'), "{ascii}");
}

#[test]
fn bench_json_round_trip_and_harness() {
    let run = BenchRun::new(vec![m("a", 1.5, 0.1)]).host("ci");
    assert_eq!(run.schema_version, 1);
    assert_eq!(run.created.len(), 20);
    assert!(run.created.ends_with('Z'));
    let json = run.to_json();
    assert!(json.contains("\"unit\": \"ns\""));
    assert_eq!(BenchRun::from_json(&json).unwrap(), run);
    let dir = temp_dir("bench");
    let path = dir.join("runs/base.json");
    run.save(&path).unwrap();
    assert_eq!(BenchRun::load(&path).unwrap(), run);
    assert!(
        BenchRun::from_json(&json.replace("\"schema_version\": 1", "\"schema_version\": 9"))
            .is_err()
    );

    assert_eq!(
        rich_ext::qa::bench::rfc3339(std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        "2023-11-14T22:13:20Z"
    );

    let bench = Bench::new("sum")
        .warmup(Duration::ZERO)
        .target_time(Duration::ZERO)
        .samples(7, 7);
    let result = bench.run(|| (0..100u64).sum::<u64>());
    assert_eq!((result.name.as_str(), result.samples), ("sum", 7));
    assert!(result.min <= result.median && result.median <= result.max);
    let result = bench_renderable_with(&bench, &small_table(), 40);
    assert_eq!(result.samples, 7);
    assert!(result.mean > 0.0);

    // criterion layout: <dir>/<group>/<bench>/new/{estimates,sample}.json.
    let criterion = dir.join("criterion");
    let new = criterion.join("render").join("table").join("new");
    std::fs::create_dir_all(&new).unwrap();
    std::fs::create_dir_all(criterion.join("report")).unwrap();
    std::fs::write(
        new.join("estimates.json"),
        r#"{"mean":{"point_estimate":1500.0},"median":{"point_estimate":1400.0},"std_dev":{"point_estimate":10.0}}"#,
    )
    .unwrap();
    std::fs::write(
        new.join("sample.json"),
        r#"{"iters":[1.0,2.0],"times":[1000.0,4000.0]}"#,
    )
    .unwrap();
    let run = BenchRun::from_criterion_dir(&criterion).unwrap();
    assert_eq!(run.measurements.len(), 1);
    let t = &run.measurements[0];
    assert_eq!(t.name, "render/table");
    assert_eq!((t.mean, t.median, t.stddev), (1500.0, 1400.0, 10.0));
    assert_eq!((t.samples, t.min, t.max), (2, 1000.0, 2000.0));
    let _ = std::fs::remove_dir_all(&dir);
}

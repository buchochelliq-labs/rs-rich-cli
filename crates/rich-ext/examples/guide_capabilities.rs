//! Guide: Capabilities and fidelity — run: cargo run -p rs-rich-ext --example guide_capabilities --features serde [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/capabilities.md` comes from this file.
//! Screenshots detect from fixed `MapEnvironment`s; without `--svg` the
//! first section also reports the real terminal.
use std::path::PathBuf;

use rich::{
    AnsiDecoder, ColorSystem, Console, ConsoleOptions, Panel, Renderable, Segment, Table, Text,
    Theme,
};
use rich_ext::capabilities::{
    Capabilities, CapabilityReport, ColorDepth, MapEnvironment, Origin, Overrides, Report,
};
use rich_ext::fidelity::{Adaptive, Degradable, Degrade, Fidelity, FidelityFacts, Policy};
use rich_ext::target::{RenderTarget, TargetKind};
use rich_ext::ConsoleExt;

// --8<-- [start:system]
fn system_report(console: &Console) {
    // Reads the real process: std::env, stdout's tty status, terminal size.
    let report: Report = Capabilities::system();
    console.print(&CapabilityReport::new(&report));
}
// --8<-- [end:system]

// --8<-- [start:map]
fn wezterm() -> MapEnvironment {
    MapEnvironment::tty()
        .size(120, 40)
        .var("TERM", "xterm-256color")
        .var("TERM_PROGRAM", "WezTerm")
        .var("COLORTERM", "truecolor")
        .var("LANG", "en_US.UTF-8")
}

fn show_map(console: &Console) {
    let report = Capabilities::detect(&wezterm());
    assert_eq!(report.color.value, ColorDepth::TrueColor);
    assert_eq!(report.color.origin, Origin::Environment("COLORTERM".into()));
    assert!(report.hyperlinks.value);
    console.print(&CapabilityReport::new(&report));
}
// --8<-- [end:map]

// --8<-- [start:ci]
fn show_ci(console: &Console) {
    // Piped output on GitHub Actions, with two RICH_* overrides; one is invalid.
    let env = MapEnvironment::new()
        .var("GITHUB_ACTIONS", "true")
        .var("CI", "true")
        .var("RICH_WIDTH", "100")
        .var("RICH_HYPERLINKS", "maybe");
    let report = Capabilities::detect(&env);
    assert_eq!(report.width.value, 100);
    assert!(!report.interactive.value);
    assert_eq!(report.warnings.len(), 1); // RICH_HYPERLINKS=maybe is ignored
    console.print(&CapabilityReport::new(&report));
}
// --8<-- [end:ci]

// --8<-- [start:overrides]
fn with_overrides() -> Report {
    // What a `--color 256 --ascii` command line might set: applied last.
    let overrides = Overrides {
        color: Some(ColorDepth::Ansi256),
        unicode: Some(false),
        width: Some(40),
        ..Overrides::default()
    };
    let report = Capabilities::detect_with(&wezterm(), &overrides);
    assert_eq!(report.color.origin, Origin::Override);
    for (name, value, origin, reason) in report.rows() {
        println!("{name:<12} {value:<10} {origin} {reason}");
    }
    report
}
// --8<-- [end:overrides]

// --8<-- [start:json]
fn report_json(report: &Report) -> String {
    // Needs the `serde` feature.
    serde_json::to_string_pretty(report).expect("serializable")
}
// --8<-- [end:json]

// --8<-- [start:target]
fn render_for(report: &Report, console: &Console) {
    // Detection feeds an explicit render target: nothing else is detected.
    let target = RenderTarget::new(
        TargetKind::Terminal,
        report.to_target_capabilities(),
        Theme::default_theme(),
    );
    let mut table = Table::new().title("Deploys");
    table.add_column("Service");
    table.add_column("Status");
    table.add_row_text(vec![
        Text::new("api"),
        Text::styled("✔ deployed", "bold #00d75f"),
    ]);
    // The target renders for those capabilities (256 colours, ASCII)...
    let ansi: String = target.text(&table);
    // ...and the result is ordinary ANSI text.
    for line in AnsiDecoder::new().decode(&ansi) {
        console.print(&line);
    }
}
// --8<-- [end:target]

// --8<-- [start:fidelity]
fn show_fidelity(console: &Console) {
    let environments = [
        ("WezTerm", wezterm()),
        ("NO_COLOR", wezterm().var("NO_COLOR", "1")),
        ("piped", wezterm().terminal(false)),
        ("LANG=C.ISO-8859-1", wezterm().var("LANG", "C.ISO-8859-1")),
    ];
    let quiet = Policy::default().ceiling(Fidelity::Rich); // never animate
    for (name, env) in environments {
        let report = Capabilities::detect(&env);
        let level = Fidelity::select(&report, &Policy::default());
        let capped = Fidelity::select(&report, &quiet);
        console.print(&Text::new(format!(
            "{name:<18} {:<9} capped: {}",
            level.name(),
            capped.name()
        )));
    }
    // Facts can also be given directly.
    let facts = FidelityFacts {
        unicode: true,
        color: false,
        interactive: false,
        animation: false,
    };
    assert_eq!(
        Fidelity::select(&facts, &Policy::default()),
        Fidelity::Plain
    );
}
// --8<-- [end:fidelity]

// --8<-- [start:degrade]
fn show_degrade(console: &Console) {
    let panel = Panel::new(Box::new(
        Text::from_markup(
            "[bold green]✔ ok[/]  [red]✖ failed[/]  → [link=https://ci.example/1]log[/]",
        )
        .unwrap(),
    ))
    .title("CI");
    for level in [
        Fidelity::Rich,
        Fidelity::Styled,
        Fidelity::Plain,
        Fidelity::Ascii,
    ] {
        console.print(&Text::new(level.name()));
        // Without `.level(…)`, Degrade selects from the console it renders on.
        console.print(&Degrade::borrowed(&panel).level(level));
    }
}
// --8<-- [end:degrade]

// --8<-- [start:adaptive]
/// A status line with its own plain and ASCII forms.
struct Status {
    ok: usize,
    failed: usize,
}

impl Degradable for Status {
    fn levels(&self) -> &[Fidelity] {
        &[Fidelity::Rich, Fidelity::Plain, Fidelity::Ascii]
    }

    fn render_at(
        &self,
        level: Fidelity,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Vec<Segment> {
        let (ok, failed) = (self.ok, self.failed);
        let text = match level {
            Fidelity::Ascii => Text::new(format!("[OK] {ok} passed, [FAIL] {failed} failed")),
            Fidelity::Plain => Text::new(format!("✔ {ok} passed, ✖ {failed} failed")),
            _ => Text::from_markup(&format!(
                "[green]✔ {ok}[/] passed, [bold red]✖ {failed}[/] failed"
            ))
            .unwrap(),
        };
        text.rich_render(console, options)
    }
}

fn show_adaptive(console: &Console) {
    for level in [Fidelity::Animated, Fidelity::Styled, Fidelity::Ascii] {
        // Styled is not offered, so Plain (the best level below it) renders.
        let status = Adaptive::new(Status { ok: 41, failed: 1 }).level(level);
        let (selected, rendered) = status.resolve(console);
        console.print(&Text::new(format!(
            "{:<8} → {:<6}",
            selected.name(),
            rendered.name()
        )));
        console.print(&status);
    }
}
// --8<-- [end:adaptive]

fn main() {
    let shots = Shots::from_args();
    if !shots.svg() {
        system_report(&Console::new());
    }
    shots.shot("map", 90, "Capabilities::detect", show_map);
    shots.shot("ci", 90, "GitHub Actions", show_ci);
    let report = with_overrides();
    if !shots.svg() {
        println!("{}", report_json(&report));
    }
    shots.shot("target", 40, "to_target_capabilities", |c| {
        render_for(&report, c)
    });
    shots.shot("fidelity", 60, "Fidelity::select", show_fidelity);
    shots.shot("degrade", 50, "Degrade", show_degrade);
    shots.shot("adaptive", 60, "Adaptive", show_adaptive);
}

/// `--svg DIR` writes each shot as `DIR/guide_capabilities-<shot>.svg`;
/// without it, shots print to the terminal.
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
        let id = format!("guide_capabilities-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}

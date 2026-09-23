//! Guide: Accessibility — run: cargo run -p rs-rich-ext --example guide_a11y --features serde [-- --svg docs/media/guide]
//!
//! Every snippet on `docs/guide/ext/accessibility.md` comes from this file.
use std::path::PathBuf;

use rich::{ColorSystem, Console, Panel, Table, Text, Theme, Tree};
use rich_ext::a11y::contrast::{CheckOptions, FindingKind};
use rich_ext::a11y::{
    check_theme, semantic_text, AccessibilityPolicy, AccessibleText, ContrastReport, Status,
    SymbolSet,
};
use rich_ext::capabilities::MapEnvironment;
use rich_ext::diagnostic::{Diagnostic, Location};
use rich_ext::fidelity::Fidelity;
use rich_ext::ConsoleExt;

fn services() -> Table {
    let mut table = Table::new().title("Services");
    table.add_column("Name");
    table.add_column("Region");
    table.add_column("Status");
    table.add_row(&["api", "eu-west-1", "✔ ok"]);
    table.add_row(&["worker", "us-east-2", "✖ failed"]);
    table
}

// --8<-- [start:semantic]
fn show_semantic(console: &Console) {
    let table = services();
    let mut tree = Tree::new("deploy");
    tree.add("build").add("compile");
    tree.add("upload");
    let panel = Panel::new(Box::new(Text::new("All checks passed"))).title("CI");
    let link = Text::from_markup("See the [link=https://ci.example/42]build log[/].").unwrap();
    let diagnostic = Diagnostic::error("mismatched types")
        .code("E0308")
        .location(Location::new("src/main.rs", Some(4), Some(18)))
        .help("change the type to `u64`");

    // The same content, in reading order, without borders or guides.
    for text in [
        table.accessible_text(80),
        tree.accessible_text(80),
        panel.accessible_text(80),
        link.accessible_text(80),
        diagnostic.accessible_text(80),
    ] {
        console.print(&Text::new(text));
        console.print(&Text::new(""));
    }
    // Any other renderable: rendered plainly, decoration dropped.
    let rule = rich::Rule::new("Summary");
    assert_eq!(semantic_text(&rule, 40), "Summary");
}
// --8<-- [end:semantic]

fn show_rendered(console: &Console) {
    console.print(&services());
}

// --8<-- [start:policy]
fn show_policy(console: &Console) {
    let environments = [
        ("(nothing set)", MapEnvironment::new()),
        ("NO_COLOR=1", MapEnvironment::new().var("NO_COLOR", "1")),
        (
            "RICH_A11Y=screen-reader",
            MapEnvironment::new().var("RICH_A11Y", "screen-reader"),
        ),
        (
            "RICH_A11Y=reduced-motion,ascii-symbols",
            MapEnvironment::new().var("RICH_A11Y", "reduced-motion,ascii-symbols"),
        ),
    ];
    let mut table = Table::new();
    for header in ["Environment", "Ceiling", "Ok", "Error", "Warning"] {
        table.add_column(header);
    }
    for (name, env) in environments {
        let policy = AccessibilityPolicy::from_env(&env);
        table.add_row(&[
            name,
            policy.fidelity_ceiling().name(),
            policy.status(Status::Ok),
            policy.status(Status::Error),
            policy.status(Status::Warning),
        ]);
    }
    console.print(&table);

    // Unknown items are kept as warnings rather than failing.
    let policy = AccessibilityPolicy::from_env(&MapEnvironment::new().var("RICH_A11Y", "loud"));
    assert_eq!(policy.warnings, ["ignored RICH_A11Y item \"loud\""]);
}
// --8<-- [end:policy]

// --8<-- [start:apply]
fn build_console() -> Console {
    let policy = AccessibilityPolicy::from_env(&rich_ext::capabilities::SystemEnvironment);
    // A theme without dim/grey (high contrast) or colour (monochrome), and no
    // emoji or highlighting for screen readers.
    let console = policy.console_builder(Console::builder()).build();
    // Cap fidelity-aware renderables (`Degrade`, `Adaptive`) too.
    let _ceiling: Fidelity = policy.fidelity_ceiling();
    let _fidelity_policy = policy.fidelity_policy();
    console
}

fn status_line(policy: &AccessibilityPolicy, ok: bool, message: &str) -> String {
    // Meaning never depends on colour: a symbol and a word, a tag, or a word.
    let status = if ok { Status::Ok } else { Status::Error };
    status.label(policy.status_symbols, message)
}
// --8<-- [end:apply]

fn show_status_lines(console: &Console) {
    for set in [SymbolSet::Unicode, SymbolSet::Ascii, SymbolSet::Words] {
        let policy = AccessibilityPolicy {
            status_symbols: set,
            ..AccessibilityPolicy::default()
        };
        let line = format!(
            "{:<8} {}   {}",
            format!("{set:?}"),
            status_line(&policy, true, "build"),
            status_line(&policy, false, "tests")
        );
        console.print(&Text::new(line));
    }
}

// --8<-- [start:contrast]
fn app_theme() -> Theme {
    Theme::from_styles(
        [
            ("app.title", "bold #1e90ff"),
            ("app.muted", "#9e9e9e"),
            ("app.ok", "#2e8b57"),
            ("app.fail", "#b22222"),
            ("app.link", "underline #6495ed"),
        ],
        false,
    )
    .expect("valid styles")
}

fn show_contrast(console: &Console) {
    let options = CheckOptions {
        // Pairs that must stay distinguishable from each other.
        groups: vec![vec!["app.ok".into(), "app.fail".into()]],
        ..CheckOptions::default()
    };
    let findings = check_theme(&app_theme(), &options);
    for finding in &findings {
        if let FindingKind::LowContrast { ratio, .. } = finding.kind {
            assert!(ratio < options.min_ratio);
        }
    }
    console.print(&ContrastReport::new(&findings));
}
// --8<-- [end:contrast]

// --8<-- [start:json]
fn contrast_json() -> String {
    // Needs the `serde` feature.
    let findings = check_theme(&app_theme(), &CheckOptions::default());
    serde_json::to_string_pretty(&findings).expect("serializable")
}
// --8<-- [end:json]

fn main() {
    let shots = Shots::from_args();
    shots.shot("rendered", 50, "Rendered", show_rendered);
    shots.shot("semantic", 70, "AccessibleText", show_semantic);
    shots.shot("policy", 90, "AccessibilityPolicy", show_policy);
    shots.shot("status", 60, "Status symbols", show_status_lines);
    shots.shot("contrast", 100, "check_theme", show_contrast);
    if !shots.svg() {
        let _ = build_console();
        println!("{}", contrast_json());
    }
}

/// `--svg DIR` writes each shot as `DIR/guide_a11y-<shot>.svg`; without it,
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
        let id = format!("guide_a11y-{name}");
        let svg = console.export_svg(title, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        std::fs::write(dir.join(format!("{id}.svg")), svg).expect("write the SVG");
    }
}

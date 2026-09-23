use rich::r#box::Box as BoxSet;
use rich::{ColorTriplet, Console, Panel, Renderable, Rule, Style, Table, Text, Theme, Tree};
use rich_ext::a11y::contrast::{
    check_theme, ciede2000, contrast_ratio, delta_e, relative_luminance, simulate, suggest_color,
    CheckOptions, ContrastReport, Deficiency, FindingKind, Severity,
};
use rich_ext::a11y::policy::{AccessibilityPolicy, Status, SymbolSet};
use rich_ext::a11y::{semantic_text, AccessibleText};
use rich_ext::capabilities::MapEnvironment;
use rich_ext::diagnostic::{Diagnostic, Location, Suggestion};
use rich_ext::fidelity::{ascii_fallback, ascii_text, Degrade, Fidelity};

fn plain(r: &dyn Renderable, width: usize) -> String {
    let c = Console::builder()
        .width(width)
        .no_color(true)
        .color_system(None)
        .build();
    c.segments_to_string(&r.rich_render(&c, &c.options()))
}

const BOXES: [(&str, BoxSet); 20] = [
    ("ASCII", rich::r#box::ASCII),
    ("ASCII2", rich::r#box::ASCII2),
    ("ASCII_DOUBLE_HEAD", rich::r#box::ASCII_DOUBLE_HEAD),
    ("SQUARE", rich::r#box::SQUARE),
    ("SQUARE_DOUBLE_HEAD", rich::r#box::SQUARE_DOUBLE_HEAD),
    ("MINIMAL", rich::r#box::MINIMAL),
    ("MINIMAL_HEAVY_HEAD", rich::r#box::MINIMAL_HEAVY_HEAD),
    ("MINIMAL_DOUBLE_HEAD", rich::r#box::MINIMAL_DOUBLE_HEAD),
    ("SIMPLE", rich::r#box::SIMPLE),
    ("SIMPLE_HEAD", rich::r#box::SIMPLE_HEAD),
    ("SIMPLE_HEAVY", rich::r#box::SIMPLE_HEAVY),
    ("HORIZONTALS", rich::r#box::HORIZONTALS),
    ("ROUNDED", rich::r#box::ROUNDED),
    ("HEAVY", rich::r#box::HEAVY),
    ("HEAVY_EDGE", rich::r#box::HEAVY_EDGE),
    ("HEAVY_HEAD", rich::r#box::HEAVY_HEAD),
    ("DOUBLE", rich::r#box::DOUBLE),
    ("DOUBLE_EDGE", rich::r#box::DOUBLE_EDGE),
    ("MARKDOWN", rich::r#box::MARKDOWN),
    ("NONE", rich::r#box::NONE),
];

fn people(b: BoxSet) -> Table {
    let mut t = Table::new().box_set(b).title("People").caption("2 people");
    t.add_column("Name");
    t.add_column("Age");
    t.add_row(&["Alice Smith", "30"]);
    t.add_row(&["Bob", "25"]);
    t
}

#[test]
fn ascii_fallback_covers_every_core_box() {
    for (name, b) in BOXES {
        let panel = Panel::new(Box::new(Text::new("● done ✔ → next…"))).box_set(b);
        for r in [&people(b) as &dyn Renderable, &panel] {
            let rich = plain(r, 30);
            let ascii = plain(&Degrade::borrowed(r).level(Fidelity::Ascii), 30);
            assert!(ascii.is_ascii(), "{name}: {ascii}");
            let widths = |s: &str| s.lines().map(rich::cells::cell_len).collect::<Vec<_>>();
            assert_eq!(widths(&rich), widths(&ascii), "{name}: widths change");
        }
    }
    assert_eq!(
        plain(
            &Degrade::new(people(rich::r#box::HEAVY_HEAD)).level(Fidelity::Ascii),
            40
        ),
        concat!(
            "       People        \n",
            "+-------------+-----+\n",
            "| Name        | Age |\n",
            "+-------------+-----+\n",
            "| Alice Smith | 30  |\n",
            "| Bob         | 25  |\n",
            "+-------------+-----+\n",
            "      2 people       "
        )
    );
    assert_eq!(ascii_text("█▓▒░ ⠋ ✗ 表"), "##:. . x ??");
    let segs = ascii_fallback(vec![rich::Segment::control("\x1b]0;✔\x07")]);
    assert_eq!(segs[0].text, "\x1b]0;✔\x07", "controls are left alone");
}

#[test]
fn table_semantic_text() {
    let expected = "\
People
Table with 2 rows, columns: Name, Age
Row 1: Name: Alice Smith; Age: 30
Row 2: Name: Bob; Age: 25
2 people";
    for (name, b) in [
        ("HEAVY_HEAD", rich::r#box::HEAVY_HEAD),
        ("ASCII", rich::r#box::ASCII),
        ("ROUNDED", rich::r#box::ROUNDED),
        ("DOUBLE_EDGE", rich::r#box::DOUBLE_EDGE),
        ("SQUARE_DOUBLE_HEAD", rich::r#box::SQUARE_DOUBLE_HEAD),
    ] {
        assert_eq!(people(b).accessible_text(40), expected, "{name}");
    }
    // MINIMAL has no top border: the title reads as part of the table.
    let minimal = people(rich::r#box::MINIMAL).accessible_text(40);
    assert!(
        minimal.contains("Row 1: Name: Alice Smith; Age: 30"),
        "{minimal}"
    );
    // Row separators group multi-line cells.
    let mut lines = Table::new().show_lines(true);
    lines.add_column("Key");
    lines.add_column("Value");
    lines.add_row(&["a", "one\ntwo"]);
    lines.add_row(&["b", "three"]);
    assert_eq!(
        lines.accessible_text(40),
        "Table with 2 rows, columns: Key, Value\nRow 1: Key: a; Value: one two\nRow 2: Key: b; Value: three"
    );
    // Borderless tables split on shared gaps of two or more cells.
    let mut grid = Table::new().box_set(rich::r#box::SIMPLE).show_header(false);
    grid.add_column("");
    grid.add_column("");
    grid.add_row(&["left side", "right"]);
    grid.add_row(&["x", "y z"]);
    assert_eq!(
        grid.accessible_text(40),
        "Table with 2 rows\nRow 1: left side; right\nRow 2: x; y z"
    );
}

#[test]
fn tree_panel_rule_text_and_diagnostic_semantic_text() {
    let mut tree = Tree::new("root");
    {
        let a = tree.add("src");
        a.add("main.rs");
        a.add("lib.rs");
    }
    tree.add("Cargo.toml");
    assert_eq!(
        tree.accessible_text(40),
        "- root\n  - src\n    - main.rs\n    - lib.rs\n  - Cargo.toml"
    );

    let panel = Panel::new(Box::new(Text::new("hello\n  indented")))
        .title("Title")
        .subtitle("sub");
    assert_eq!(panel.accessible_text(30), "Title\nhello\n  indented\nsub");
    let nested = Panel::new(Box::new(Panel::new(Box::new(Text::new("inner")))));
    assert_eq!(nested.accessible_text(30), "inner");

    assert_eq!(Rule::new("Section").accessible_text(30), "Section");
    assert_eq!(Rule::line().accessible_text(30), "");

    assert_eq!(
        "see [link=https://x.y]the docs[/link] or [bold]not[/bold]".accessible_text(80),
        "see the docs <https://x.y> or not"
    );
    let mut t = Text::new("click here");
    t.stylize(Style::new().with_link("https://a"), 6, 10);
    assert_eq!(t.accessible_text(80), "click here <https://a>");

    let d = Diagnostic::error("bad thing")
        .code("E001")
        .code_url("https://docs/E001")
        .location(Location::new("src/main.rs", Some(3), Some(5)))
        .cause("disk full")
        .note("a note")
        .help("try this")
        .suggestion(Suggestion::new("rename it"));
    assert_eq!(
        d.accessible_text(60),
        "\
error[E001]: bad thing
documentation: <https://docs/E001>
location: src/main.rs:3:5
caused by: disk full
note: a note
help: try this
suggestion: rename it"
    );
    assert_eq!(Diagnostic::new("plain").accessible_text(60), "plain");
}

#[test]
fn generic_semantic_text_drops_decoration() {
    let panel = Panel::new(Box::new(Text::new("a  b\n\nc"))).title("T");
    assert_eq!(semantic_text(&panel, 20), "T\na b\n\nc");
    assert_eq!(
        semantic_text(&people(rich::r#box::HEAVY_HEAD), 40),
        "People\nName Age\nAlice Smith 30\nBob 25\n2 people"
    );
}

#[test]
fn policy_presets_env_and_symbols() {
    let p = AccessibilityPolicy::from_env(&MapEnvironment::new().var("NO_COLOR", "1").var(
        "RICH_A11Y",
        "high-contrast, reduced-motion,bogus,ascii-symbols",
    ));
    assert!(p.monochrome && p.high_contrast && p.reduced_motion && !p.screen_reader);
    assert_eq!(p.status_symbols, SymbolSet::Ascii);
    assert_eq!(p.warnings, ["ignored RICH_A11Y item \"bogus\""]);
    assert_eq!(p.fidelity_ceiling(), Fidelity::Styled);

    let sr =
        AccessibilityPolicy::from_env(&MapEnvironment::new().var("RICH_A11Y", "screen-reader"));
    assert_eq!(sr, AccessibilityPolicy::screen_reader());
    assert_eq!(sr.fidelity_ceiling(), Fidelity::Plain);
    assert_eq!(
        AccessibilityPolicy::reduced_motion().fidelity_ceiling(),
        Fidelity::Rich
    );
    assert_eq!(
        AccessibilityPolicy::default().fidelity_ceiling(),
        Fidelity::Animated
    );
    assert!(
        !AccessibilityPolicy::reduced_motion()
            .fidelity_policy()
            .allow_animation
    );
    let empty = AccessibilityPolicy::from_env(&MapEnvironment::new().var("NO_COLOR", ""));
    assert_eq!(empty, AccessibilityPolicy::default());

    let table: Vec<[&str; 3]> = Status::ALL
        .iter()
        .map(|s| {
            [
                s.symbol(SymbolSet::Unicode),
                s.symbol(SymbolSet::Ascii),
                s.symbol(SymbolSet::Words),
            ]
        })
        .collect();
    assert_eq!(
        table,
        [
            ["✔ ok", "[OK]", "ok:"],
            ["⚠ warning", "[WARN]", "warning:"],
            ["✖ error", "[ERROR]", "error:"],
            ["ℹ info", "[INFO]", "info:"],
            ["… pending", "[PENDING]", "pending:"],
            ["↷ skipped", "[SKIP]", "skipped:"],
        ]
    );
    // Every symbol carries its word: meaning never depends on colour.
    for s in Status::ALL {
        for set in [SymbolSet::Unicode, SymbolSet::Ascii, SymbolSet::Words] {
            let word = s.word().to_ascii_uppercase();
            assert!(
                s.symbol(set)
                    .to_ascii_uppercase()
                    .contains(&word[..4.min(word.len())]),
                "{s:?} {set:?}"
            );
        }
    }
    assert_eq!(Status::Error.label(SymbolSet::Words, "boom"), "error: boom");
}

#[test]
fn policy_theme_transforms() {
    let base = Theme::default_theme();
    let def = |t: &Theme, name: &str| t.get(name).unwrap().definition();

    let mono = AccessibilityPolicy::monochrome().theme(&base);
    assert_eq!(mono.len(), base.len());
    assert_eq!(def(&mono, "logging.level.error"), "bold");
    assert_eq!(def(&mono, "logging.level.critical"), "bold reverse");
    assert_eq!(def(&mono, "repr.bool_true"), "italic");
    assert_eq!(def(&mono, "logging.level.info"), "none");
    assert!(mono.names().all(|n| {
        let s = mono.get(n).unwrap();
        s.color().is_none() && s.bgcolor().is_none()
    }));

    let hc = AccessibilityPolicy::high_contrast().theme(&base);
    assert_eq!(def(&hc, "dim"), "none");
    assert_eq!(def(&hc, "log.time"), "cyan");
    assert_eq!(def(&hc, "bar.back"), "default");
    assert_eq!(def(&hc, "logging.level.info"), "bright_blue");
    assert_eq!(def(&hc, "logging.level.error"), "bold red");
    assert!(hc.names().all(|n| hc.get(n).unwrap().attr(1) != Some(true)));

    let c = AccessibilityPolicy::monochrome()
        .console_builder(Console::builder().width(20).force_terminal(true))
        .build();
    assert!(c.no_color());
    assert_eq!(
        c.theme().get("logging.level.error").unwrap().definition(),
        "bold"
    );
}

fn hex(s: &str) -> ColorTriplet {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap();
    ColorTriplet::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

#[test]
fn wcag_reference_values() {
    let (black, white) = (hex("000000"), hex("ffffff"));
    assert_eq!(contrast_ratio(black, white), 21.0);
    assert_eq!(contrast_ratio(white, white), 1.0);
    assert_eq!(relative_luminance(white), 1.0);
    // WebAIM reference figures.
    let r = |a, b| (contrast_ratio(hex(a), hex(b)) * 100.0).round() / 100.0;
    assert_eq!(r("777777", "ffffff"), 4.48);
    assert_eq!(r("767676", "ffffff"), 4.54);
    assert_eq!(r("0000ff", "ffffff"), 8.59);
    assert_eq!(r("ff0000", "ffffff"), 4.0);
    assert_eq!(r("ffffff", "ff0000"), 4.0, "symmetric");
    let s = suggest_color(hex("777777"), white, 4.5).unwrap();
    assert!(contrast_ratio(s, white) >= 4.5);
    assert_eq!(s, hex("767676"), "the nearest grey that passes");
}

#[test]
fn ciede2000_matches_sharma_reference_pairs() {
    // Sharma, Wu & Dalal (2005), table 1: pairs 1, 7, 17, 25.
    for (a, b, expected) in [
        ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
        ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
        ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
        (
            [60.2574, -34.0099, 36.2677],
            [60.4626, -34.1751, 39.4387],
            1.2644,
        ),
    ] {
        let d = ciede2000(a, b);
        assert!((d - expected).abs() < 1e-4, "{a:?} {b:?}: {d}");
    }
}

#[test]
fn colour_blind_simulation_sanity() {
    let white = hex("ffffff");
    for d in Deficiency::ALL {
        let w = simulate(white, d);
        assert!(delta_e(white, w) < 1.0, "{d:?} keeps white: {w:?}");
        let grey = hex("808080");
        assert!(delta_e(grey, simulate(grey, d)) < 1.0, "{d:?} keeps grey");
    }
    // Viénot dichromats see a single red/green channel.
    let s = simulate(hex("d62728"), Deficiency::Protan);
    assert_eq!(s.red, s.green);
    // An isoluminant red/green pair (L* ≈ 46.85): distinct with normal vision,
    // confusable for deutans, still distinct for tritans.
    let (red, green) = (hex("d62728"), hex("547a20"));
    let normal = delta_e(red, green);
    let sim = |d| delta_e(simulate(red, d), simulate(green, d));
    assert!(normal > 30.0, "normal {normal}");
    assert!(
        sim(Deficiency::Deutan) < 10.0,
        "deutan {}",
        sim(Deficiency::Deutan)
    );
    // Protans also lose red luminance, so the pair splits by lightness again.
    assert!(
        sim(Deficiency::Protan) < normal / 2.0,
        "protan {}",
        sim(Deficiency::Protan)
    );
    assert!(
        sim(Deficiency::Tritan) > 20.0,
        "tritan {}",
        sim(Deficiency::Tritan)
    );
    // Blue/yellow-green confusion is the tritan axis, not the deutan one.
    let (blue, teal) = (hex("0072b2"), hex("009e73"));
    assert!(sim_pair(blue, teal, Deficiency::Tritan) < sim_pair(blue, teal, Deficiency::Deutan));
}

fn sim_pair(a: ColorTriplet, b: ColorTriplet, d: Deficiency) -> f64 {
    delta_e(simulate(a, d), simulate(b, d))
}

#[test]
fn default_theme_check_is_stable() {
    let theme = Theme::default_theme();
    let findings = check_theme(&theme, &CheckOptions::default());
    assert_eq!(findings, check_theme(&theme, &CheckOptions::default()));
    // Recorded findings for upstream's default theme against the white export
    // theme and Monokai: see the table in `ContrastReport` below.
    assert_eq!(findings.len(), 66);
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    assert_eq!(errors, 21);
    let first = &findings[0];
    assert_eq!(first.style_name, "bar.back");
    assert_eq!(
        first.kind,
        FindingKind::LowContrast {
            ratio: 1.72,
            fg: "#3a3a3a".into(),
            bg: "#0c0c0c".into()
        }
    );
    assert_eq!(first.suggestion, "use #7a7a7a (4.56:1) on #0c0c0c");
    let kinds = |name: &str| {
        findings
            .iter()
            .filter(|f| f.style_name == name)
            .map(|f| f.describe())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        kinds("repr.bool_true"),
        [
            "contrast 1.37:1 (#00ff00 on #ffffff)",
            "repr.bool_true and repr.bool_false differ only by colour"
        ]
    );
    assert_eq!(
        kinds("logging.level.debug"),
        [
            "logging.level.debug and logging.level.info differ only by colour",
            "logging.level.debug and logging.level.warning differ only by colour",
            "logging.level.debug ~ logging.level.warning with protanopia (ΔE00 2.8)",
            "logging.level.debug ~ logging.level.warning with deuteranopia (ΔE00 4.4)",
        ]
    );
    assert!(
        findings.iter().all(|f| !matches!(
            &f.kind,
            FindingKind::ColorOnlyDistinction { a, .. } if a == "logging.level.error"
        )),
        "bold keeps error distinct without colour"
    );

    // The extended theme adds error/warning/info/success, which are checked too.
    let extended = check_theme(&rich_ext::theme::extended_theme(), &CheckOptions::default());
    assert!(extended
        .iter()
        .any(|f| f.describe() == "warning and info differ only by colour"));

    let c = Console::builder().width(120).no_color(true).build();
    let out = c.segments_to_string(&ContrastReport::new(&findings).rich_render(&c, &c.options()));
    assert!(out.ends_with("│\n└───────────────────────────────┴──────────┴─────────────────────────────────────────┴─────────────────────────────────────────┘\n66 findings, 21 errors\n")
        || out.ends_with("66 findings, 21 errors\n"), "{out}");
    let empty = c.segments_to_string(&ContrastReport::new(&[]).rich_render(&c, &c.options()));
    assert_eq!(empty, "0 findings, 0 errors\n");
}

#[cfg(feature = "serde")]
#[test]
fn findings_serialize() {
    let findings = check_theme(&Theme::default_theme(), &CheckOptions::default());
    let json = serde_json::to_value(&findings[0]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "style_name": "bar.back",
            "kind": {"kind": "low_contrast", "ratio": 1.72, "fg": "#3a3a3a", "bg": "#0c0c0c"},
            "severity": "error",
            "suggestion": "use #7a7a7a (4.56:1) on #0c0c0c"
        })
    );
}

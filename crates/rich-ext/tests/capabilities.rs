use rich::protocol::{Support, TargetCapabilities};
use rich::{ColorSystem, Console, Panel, Renderable, Text};
use rich_ext::capabilities::{
    Capabilities, CapabilityReport, ColorDepth, Graphics, MapEnvironment, Origin, Overrides,
};
use rich_ext::fidelity::{Adaptive, Degradable, Degrade, Fidelity, FidelityFacts, Policy};
use rich_ext::target::{resolve_capabilities, CapabilityOrigin, TargetOverrides};

fn env(name: &str) -> Origin {
    Origin::Environment(name.into())
}

fn tty(vars: &[(&str, &str)]) -> MapEnvironment {
    vars.iter()
        .fold(MapEnvironment::tty(), |e, (k, v)| e.var(k, v))
}

#[test]
fn colour_detection_table_with_provenance() {
    use ColorDepth::*;
    let cases: Vec<(&str, MapEnvironment, ColorDepth, Origin, &str)> = vec![
        (
            "bare tty",
            tty(&[]),
            Ansi16,
            Origin::Default,
            "terminal with no TERM",
        ),
        (
            "dumb",
            tty(&[("TERM", "dumb")]),
            None,
            env("TERM"),
            "TERM=dumb",
        ),
        (
            "NO_COLOR beats everything",
            tty(&[
                ("NO_COLOR", "1"),
                ("COLORTERM", "truecolor"),
                ("FORCE_COLOR", "3"),
            ]),
            None,
            env("NO_COLOR"),
            "NO_COLOR=1",
        ),
        (
            "empty NO_COLOR is ignored",
            tty(&[("NO_COLOR", ""), ("TERM", "xterm")]),
            Ansi16,
            env("TERM"),
            "TERM=xterm",
        ),
        (
            "FORCE_COLOR on a pipe",
            MapEnvironment::new().var("FORCE_COLOR", "2"),
            Ansi256,
            env("FORCE_COLOR"),
            "FORCE_COLOR=2",
        ),
        (
            "FORCE_COLOR=0",
            tty(&[("FORCE_COLOR", "0"), ("COLORTERM", "truecolor")]),
            None,
            env("FORCE_COLOR"),
            "FORCE_COLOR=0",
        ),
        (
            "FORCE_COLOR without a level keeps the terminal's depth",
            MapEnvironment::new()
                .var("FORCE_COLOR", "true")
                .var("TERM", "xterm-256color"),
            Ansi256,
            env("FORCE_COLOR"),
            "TERM=xterm-256color",
        ),
        (
            "COLORTERM",
            tty(&[("COLORTERM", "truecolor"), ("TERM", "xterm")]),
            TrueColor,
            env("COLORTERM"),
            "COLORTERM=truecolor",
        ),
        (
            "COLORTERM 24bit",
            tty(&[("COLORTERM", "24bit")]),
            TrueColor,
            env("COLORTERM"),
            "COLORTERM=24bit",
        ),
        (
            "256color TERM",
            tty(&[("TERM", "xterm-256color")]),
            Ansi256,
            env("TERM"),
            "TERM=xterm-256color",
        ),
        (
            "Windows Terminal",
            tty(&[("WT_SESSION", "abc")]),
            TrueColor,
            env("WT_SESSION"),
            "Windows Terminal",
        ),
        (
            "legacy Windows console",
            tty(&[]).windows(true),
            TrueColor,
            Origin::Inferred,
            "Windows console",
        ),
        (
            "iTerm",
            tty(&[("TERM_PROGRAM", "iTerm.app"), ("TERM", "xterm-256color")]),
            TrueColor,
            env("TERM_PROGRAM"),
            "TERM_PROGRAM=iTerm.app",
        ),
        (
            "Apple Terminal",
            tty(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm")]),
            Ansi256,
            env("TERM_PROGRAM"),
            "TERM_PROGRAM=Apple_Terminal",
        ),
        (
            "kitty",
            tty(&[("TERM", "xterm-kitty")]),
            TrueColor,
            env("TERM"),
            "TERM=xterm-kitty",
        ),
        (
            "WezTerm",
            tty(&[("TERM_PROGRAM", "WezTerm")]),
            TrueColor,
            env("TERM_PROGRAM"),
            "TERM_PROGRAM=WezTerm",
        ),
        (
            "pipe",
            MapEnvironment::new().var("COLORTERM", "truecolor"),
            None,
            Origin::Inferred,
            "stdout is not a terminal",
        ),
        (
            "GitHub Actions log",
            MapEnvironment::new()
                .var("GITHUB_ACTIONS", "true")
                .var("CI", "true"),
            TrueColor,
            env("GITHUB_ACTIONS"),
            "GitHub Actions renders ANSI logs",
        ),
        (
            "GitLab CI log",
            MapEnvironment::new().var("GITLAB_CI", "true"),
            Ansi16,
            env("GITLAB_CI"),
            "GitLab CI renders ANSI logs",
        ),
        (
            "Jenkins log is plain",
            MapEnvironment::new().var("JENKINS_URL", "http://ci"),
            None,
            Origin::Inferred,
            "stdout is not a terminal",
        ),
    ];
    for (name, e, depth, origin, reason) in cases {
        let r = Capabilities::detect(&e);
        assert_eq!(r.color.value, depth, "{name}");
        assert_eq!(r.color.origin, origin, "{name}");
        assert_eq!(r.color.reason, reason, "{name}");
    }
}

#[test]
fn unicode_hyperlinks_graphics_and_animation() {
    let r = Capabilities::detect(&tty(&[
        ("LANG", "en_US.UTF-8"),
        ("LC_ALL", "de_DE.ISO-8859-1"),
    ]));
    assert!(!r.unicode.value, "LC_ALL wins over LANG");
    assert_eq!(r.unicode.origin, env("LC_ALL"));
    let r = Capabilities::detect(&tty(&[("LC_CTYPE", "C.UTF-8")]));
    assert!(r.unicode.value);
    assert_eq!(r.unicode.reason, "LC_CTYPE=C.UTF-8");
    let r = Capabilities::detect(&tty(&[("LANG", "C")]));
    assert!(r.unicode.value);
    let r = Capabilities::detect(&tty(&[]));
    assert_eq!(
        (r.unicode.value, &r.unicode.origin),
        (true, &Origin::Default)
    );

    for (vars, expected, origin) in [
        (
            vec![("TERM_PROGRAM", "iTerm.app")],
            true,
            env("TERM_PROGRAM"),
        ),
        (vec![("TERM_PROGRAM", "WezTerm")], true, env("TERM_PROGRAM")),
        (vec![("KITTY_WINDOW_ID", "1")], true, env("KITTY_WINDOW_ID")),
        (vec![("WT_SESSION", "x")], true, env("WT_SESSION")),
        (vec![("VTE_VERSION", "6003")], true, env("VTE_VERSION")),
        (vec![("VTE_VERSION", "4803")], false, env("VTE_VERSION")),
        (vec![("TERM", "foot")], true, env("TERM")),
        (vec![("TERM", "alacritty")], false, Origin::Default),
        (
            vec![("TERM_PROGRAM", "iTerm.app"), ("TMUX", "/tmp/t")],
            false,
            env("TMUX"),
        ),
        (vec![("TERM", "dumb")], false, env("TERM")),
    ] {
        let r = Capabilities::detect(&tty(&vars));
        assert_eq!(r.hyperlinks.value, expected, "{vars:?}");
        assert_eq!(r.hyperlinks.origin, origin, "{vars:?}");
    }
    let piped = Capabilities::detect(&MapEnvironment::new().var("TERM_PROGRAM", "iTerm.app"));
    assert!(!piped.hyperlinks.value);
    assert_eq!(piped.hyperlinks.origin, Origin::Inferred);

    for (vars, graphics, sixel) in [
        (vec![("TERM", "xterm-kitty")], Graphics::Kitty, false),
        (vec![("KITTY_WINDOW_ID", "3")], Graphics::Kitty, false),
        (vec![("TERM_PROGRAM", "iTerm.app")], Graphics::Iterm, false),
        (vec![("TERM_PROGRAM", "WezTerm")], Graphics::Iterm, true),
        (vec![("WT_SESSION", "x")], Graphics::Sixel, true),
        (vec![("TERM", "foot-extra")], Graphics::Sixel, true),
        (vec![("TERM", "mlterm")], Graphics::Sixel, true),
        (vec![("TERM_PROGRAM", "mintty")], Graphics::Sixel, true),
        (vec![("TERM", "xterm-256color")], Graphics::None, false),
    ] {
        let r = Capabilities::detect(&tty(&vars));
        assert_eq!(
            (r.graphics.value, r.sixel.value),
            (graphics, sixel),
            "{vars:?}"
        );
    }
    let r = Capabilities::detect(&MapEnvironment::new().var("TERM", "xterm-kitty"));
    assert_eq!(r.graphics.value, Graphics::None, "graphics need a tty");

    let r = Capabilities::detect(&tty(&[("TERM", "xterm")]));
    assert!(r.animation.value && r.interactive.value);
    for (vars, origin) in [
        (vec![("CI", "true")], env("CI")),
        (vec![("GITHUB_ACTIONS", "true")], env("GITHUB_ACTIONS")),
        (vec![("TERM", "dumb")], env("TERM")),
        (
            vec![("RICH_A11Y", "compact,reduced-motion")],
            env("RICH_A11Y"),
        ),
    ] {
        let r = Capabilities::detect(&tty(&vars));
        assert!(!r.animation.value, "{vars:?}");
        assert_eq!(r.animation.origin, origin, "{vars:?}");
    }
    let r = Capabilities::detect(&MapEnvironment::new());
    assert!(!r.animation.value && !r.interactive.value);
    assert_eq!(r.ci, None);
    let r = Capabilities::detect(&tty(&[("BUILDKITE", "true")]));
    assert_eq!(r.ci.as_deref(), Some("Buildkite"));
}

#[test]
fn dimensions_follow_rich_then_columns_then_terminal() {
    let r = Capabilities::detect(&tty(&[]).size(120, 40));
    assert_eq!((r.width.value, r.height.value), (120, 40));
    assert_eq!(r.width.origin, Origin::Inferred);
    let r = Capabilities::detect(&tty(&[("COLUMNS", "100")]).size(120, 40));
    assert_eq!(
        (r.width.value, r.width.origin.clone()),
        (100, env("COLUMNS"))
    );
    let r = Capabilities::detect(&tty(&[("COLUMNS", "100"), ("RICH_WIDTH", "60")]));
    assert_eq!(
        (r.width.value, r.width.origin.clone()),
        (60, env("RICH_WIDTH"))
    );
    let r = Capabilities::detect(&MapEnvironment::new());
    assert_eq!((r.width.value, r.height.value), (80, 25));
    assert_eq!(r.height.origin, Origin::Default);
}

#[test]
fn override_variables_and_struct_overrides_win() {
    let e = tty(&[
        ("TERM", "xterm-kitty"),
        ("RICH_COLOR", "256"),
        ("RICH_UNICODE", "0"),
        ("RICH_HYPERLINKS", "no"),
        ("RICH_GRAPHICS", "sixel"),
        ("RICH_ANIMATION", "off"),
    ]);
    let r = Capabilities::detect(&e);
    assert_eq!(r.color.value, ColorDepth::Ansi256);
    assert_eq!(r.color.origin, env("RICH_COLOR"));
    assert_eq!(r.color.reason, "RICH_COLOR=256");
    assert!(!r.unicode.value && !r.hyperlinks.value && !r.animation.value);
    assert_eq!((r.graphics.value, r.sixel.value), (Graphics::Sixel, true));
    assert_eq!(r.to_target_capabilities().sixel, Support::Confirmed);

    // The CLI's RICH_SIXEL still works, as an alias.
    let r = Capabilities::detect(&tty(&[("RICH_SIXEL", "1")]));
    assert_eq!((r.graphics.value, r.sixel.value), (Graphics::Sixel, true));
    assert_eq!(r.sixel.origin, env("RICH_SIXEL"));
    let r = Capabilities::detect(&tty(&[("RICH_SIXEL", "0"), ("WT_SESSION", "x")]));
    assert!(!r.sixel.value);

    // Invalid values are reported and ignored.
    let r = Capabilities::detect(&tty(&[("RICH_COLOR", "lots"), ("TERM", "xterm-256color")]));
    assert_eq!(r.color.value, ColorDepth::Ansi256);
    assert_eq!(
        r.warnings,
        ["ignored RICH_COLOR=\"lots\": unrecognised value"]
    );

    // Struct overrides beat everything, variables included.
    let overrides = Overrides {
        color: Some(ColorDepth::TrueColor),
        width: Some(33),
        interactive: Some(false),
        graphics: Some(Graphics::Kitty),
        ..Overrides::default()
    };
    let r = Capabilities::detect_with(&e, &overrides);
    assert_eq!(r.color.value, ColorDepth::TrueColor);
    assert_eq!(r.color.origin, Origin::Override);
    assert_eq!(r.width.value, 33);
    assert_eq!((r.graphics.value, r.sixel.value), (Graphics::Kitty, false));
    assert!(!r.interactive.value);
    assert!(
        !r.animation.value,
        "animation follows the overridden tty flag"
    );
}

#[test]
fn report_converts_to_core_capabilities_and_the_older_api() {
    let r = Capabilities::detect(
        &tty(&[("TERM", "xterm-256color"), ("TERM_PROGRAM", "WezTerm")]).size(90, 30),
    );
    let caps = r.to_target_capabilities();
    assert_eq!(
        caps,
        TargetCapabilities {
            width: 90,
            height: 30,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: true,
            sixel: Support::Inferred,
        }
    );
    let detected = r.to_detected();
    assert_eq!(detected.capabilities, caps);
    assert!(detected
        .origins
        .contains(&("color_system".into(), CapabilityOrigin::Detected)));
    assert!(detected
        .origins
        .contains(&("width".into(), CapabilityOrigin::Inferred)));
    let resolved = resolve_capabilities(r.observations(), TargetOverrides::default());
    assert_eq!(resolved.capabilities, caps);
    let target = rich_ext::target::RenderTarget::new(
        rich_ext::target::TargetKind::Terminal,
        caps,
        rich::Theme::default_theme(),
    );
    assert_eq!(target.console().width(), 90);
}

#[test]
fn capability_report_renders_every_row() {
    let r = Capabilities::detect(
        &tty(&[
            ("COLORTERM", "truecolor"),
            ("LANG", "en_US.UTF-8"),
            ("RICH_WIDTH", "x"),
        ])
        .size(100, 30),
    );
    let c = Console::builder().width(90).no_color(true).build();
    let out = c.segments_to_string(&CapabilityReport::new(&r).rich_render(&c, &c.options()));
    let expected = "\
┏━━━━━━━━━━━━━┳━━━━━━━━━━━┳━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ Capability  ┃ Value     ┃ Source                                        ┃
┡━━━━━━━━━━━━━╇━━━━━━━━━━━╇━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┩
│ color       │ truecolor │ environment (COLORTERM): COLORTERM=truecolor  │
│ unicode     │ yes       │ environment (LANG): LANG=en_US.UTF-8          │
│ hyperlinks  │ no        │ default: terminal not known to support OSC 8  │
│ graphics    │ none      │ default: no graphics protocol identified      │
│ sixel       │ no        │ default: no sixel-capable terminal identified │
│ width       │ 100       │ inferred: terminal size                       │
│ height      │ 30        │ inferred: terminal size                       │
│ interactive │ yes       │ inferred: stdout is a terminal                │
│ animation   │ yes       │ inferred: interactive terminal                │
│ warning     │           │ ignored RICH_WIDTH=\"x\": unrecognised value    │
└─────────────┴───────────┴───────────────────────────────────────────────┘";
    assert_eq!(out, expected);
}

#[cfg(feature = "serde")]
#[test]
fn report_serializes() {
    let r = Capabilities::detect(&tty(&[("NO_COLOR", "1")]));
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["color"]["value"], "none");
    assert_eq!(json["color"]["origin"]["environment"], "NO_COLOR");
    assert_eq!(json["interactive"]["origin"], "inferred");
}

#[test]
fn fidelity_selection_matrix() {
    let f = |unicode, color, interactive, animation| FidelityFacts {
        unicode,
        color,
        interactive,
        animation,
    };
    let p = Policy::default();
    for (facts, expected) in [
        (f(false, true, true, true), Fidelity::Ascii),
        (f(true, false, true, true), Fidelity::Styled),
        (f(true, false, false, false), Fidelity::Plain),
        (f(true, true, true, true), Fidelity::Animated),
        (f(true, true, true, false), Fidelity::Rich),
        (f(true, true, false, false), Fidelity::Rich),
    ] {
        assert_eq!(Fidelity::select(&facts, &p), expected, "{facts:?}");
    }
    let top = f(true, true, true, true);
    assert_eq!(
        Fidelity::select(&top, &p.ceiling(Fidelity::Styled)),
        Fidelity::Styled
    );
    let no_anim = Policy {
        allow_animation: false,
        ..p
    };
    assert_eq!(Fidelity::select(&top, &no_anim), Fidelity::Rich);
    let bottom = f(false, false, false, false);
    assert_eq!(
        Fidelity::select(&bottom, &p.floor(Fidelity::Plain)),
        Fidelity::Plain
    );
    assert!(Fidelity::Animated > Fidelity::Rich && Fidelity::Plain > Fidelity::Ascii);

    // From a report and from core capabilities.
    let r = Capabilities::detect(&tty(&[("TERM", "xterm-256color"), ("CI", "1")]));
    assert_eq!(
        Fidelity::select(&r, &p),
        Fidelity::Rich,
        "CI does not animate"
    );
    let r = Capabilities::detect(&tty(&[("NO_COLOR", "1")]));
    assert_eq!(Fidelity::select(&r, &p), Fidelity::Styled);
    let r = Capabilities::detect(&MapEnvironment::new().var("LANG", "en_US.ISO-8859-1"));
    assert_eq!(Fidelity::select(&r, &p), Fidelity::Ascii);
    let caps = Capabilities::detect(&MapEnvironment::new()).to_target_capabilities();
    assert_eq!(Fidelity::select(&caps, &p), Fidelity::Plain);
}

struct TwoLevel;
impl Degradable for TwoLevel {
    fn levels(&self) -> &[Fidelity] {
        &[Fidelity::Rich, Fidelity::Plain]
    }
    fn render_at(
        &self,
        level: Fidelity,
        _: &Console,
        _: &rich::ConsoleOptions,
    ) -> Vec<rich::Segment> {
        let style = rich::Style::parse("bold red").unwrap();
        vec![rich::Segment::new(
            format!("level {} ✔", level.name()),
            Some(style),
        )]
    }
}

#[test]
fn adaptive_picks_the_best_supported_level() {
    let c = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let render = |a: &dyn Renderable| a.rich_render(&c, &c.options());
    let seg = render(&Adaptive::new(TwoLevel).level(Fidelity::Animated));
    assert_eq!(seg[0].text, "level rich ✔");
    let seg = render(&Adaptive::new(TwoLevel).level(Fidelity::Styled));
    assert_eq!(seg[0].text, "level plain ✔");
    let seg = render(&Adaptive::new(TwoLevel).level(Fidelity::Ascii));
    assert_eq!(
        seg[0].text, "level plain v",
        "degraded generically below Plain"
    );
    assert!(seg[0].style.is_none());
    // Derived from the console: a truecolor terminal animates.
    assert_eq!(
        Adaptive::new(TwoLevel).resolve(&c),
        (Fidelity::Animated, Fidelity::Rich)
    );
    let p = Policy::default().ceiling(Fidelity::Plain);
    assert_eq!(
        Adaptive::new(TwoLevel).policy(p).resolve(&c),
        (Fidelity::Plain, Fidelity::Plain)
    );
}

#[test]
fn degrade_strips_colour_then_styles() {
    let c = Console::builder()
        .width(20)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build();
    let text = || Text::from_markup("[bold red]hi[/] [link=https://x]x[/link]").unwrap();
    let out = |level| {
        let d = Degrade::new(text()).level(level);
        c.segments_to_string(&d.rich_render(&c, &c.options()))
    };
    assert!(out(Fidelity::Rich).contains("\x1b[1;31mhi"));
    let styled = out(Fidelity::Styled);
    assert!(
        styled.contains("\x1b[1mhi") && !styled.contains("31"),
        "{styled:?}"
    );
    assert!(styled.contains("\x1b]8;"), "links survive Styled");
    assert_eq!(out(Fidelity::Plain), "hi x");
    let panel = Panel::new(Box::new(Text::new("ok")));
    let d = Degrade::new(panel).level(Fidelity::Ascii);
    let s = c.segments_to_string(&d.rich_render(&c, &c.options().update_width(8)));
    assert_eq!(s, "+------+\n| ok   |\n+------+");
}

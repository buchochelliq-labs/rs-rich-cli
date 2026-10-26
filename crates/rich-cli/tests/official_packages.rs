//! What the official packages added in 0.0.23 do (tests/support/official.rs
//! builds them; official_artifacts.rs pins their bytes).
//!
//! - The themes (`crates/rich-cli/official/themes/`) read as upstream theme
//!   files and pass the marketplace's install check, name only styles of
//!   rich's default theme and override every one of them that has a colour,
//!   meet WCAG contrast against the background each names, and render
//!   through `--theme-file`.
//! - The WASM plugins `textcase` 0.2.0 and `wordplay` 0.1.0
//!   (`crates/rich-cli/official/wasm/`) load with the host's loader and
//!   transform ASCII and non-ASCII text as their sources document, within
//!   the ABI's default limits.

#[path = "support/official.rs"]
mod official;

use rich::theme::DEFAULT_STYLES;
use rich::{ColorSystem, ColorTriplet, Style, Theme};
use std::process::{Command, Stdio};

fn rich(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", dir)
        .env("USERPROFILE", dir)
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .env("RICH_PLUGIN_DIR", dir.join("plugins"))
        .env("COLUMNS", "60")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor")
        .env_remove("NO_COLOR")
        .env_remove("FORCE_COLOR")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// ---------------------------------------------------------------------------
// Themes
// ---------------------------------------------------------------------------

/// The styles that are colours by name; a theme leaves them alone.
const COLOUR_NAMES: [&str; 7] = [
    "black", "red", "green", "yellow", "magenta", "cyan", "white",
];

fn theme_text(id: &str) -> String {
    String::from_utf8(official::theme(id)).unwrap()
}

/// The theme's own styles, as `--theme-file` reads it (not inheriting).
fn theme(id: &str) -> Theme {
    Theme::from_file(&theme_text(id), false).unwrap()
}

fn styles(id: &str) -> Vec<(String, Style)> {
    let theme = theme(id);
    let mut all: Vec<(String, Style)> = theme
        .names()
        .map(|name| (name.to_string(), theme.get(name).unwrap().clone()))
        .collect();
    all.sort_by(|a, b| a.0.cmp(&b.0));
    all
}

/// The background a coloured theme is made for: its `# background: #RRGGBB`
/// line. Mono has none.
fn background(id: &str) -> Option<ColorTriplet> {
    let text = theme_text(id);
    let hex = text
        .lines()
        .find_map(|line| line.strip_prefix("# background: "))?;
    let color = rich::Color::parse(hex.trim()).unwrap();
    Some(color.get_truecolor().unwrap())
}

/// WCAG 2's relative luminance and contrast ratio.
fn luminance(c: ColorTriplet) -> f64 {
    let channel = |v: u8| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(c.red) + 0.7152 * channel(c.green) + 0.0722 * channel(c.blue)
}

fn contrast(a: ColorTriplet, b: ColorTriplet) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Lines and borders, not text: WCAG's non-text contrast, 3:1, applies.
fn is_graphic(name: &str) -> bool {
    name.ends_with("border")
        || name.ends_with(".line")
        || name == "markdown.hr"
        || name.starts_with("bar.")
}

/// Each theme reads as an upstream theme file, as `--theme-file` and the
/// marketplace's install check read it, and fits the CLI's 1 MiB limit.
#[test]
fn every_theme_reads_as_an_upstream_theme_file() {
    for (id, _) in official::THEMES {
        let bytes = official::theme(id);
        assert!(bytes.len() < 1024 * 1024, "{id}");
        rich_marketplace::check_theme(&bytes).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(!styles(id).is_empty(), "{id}");
    }
}

/// A theme names only styles of rich's default theme (a typo would add a
/// style nothing uses), and overrides every default style with a colour, so
/// no default colour shows through it.
#[test]
fn every_theme_covers_the_coloured_default_styles() {
    let known: Vec<&str> = DEFAULT_STYLES.iter().map(|(name, _)| *name).collect();
    let coloured: Vec<&str> = DEFAULT_STYLES
        .iter()
        .filter(|(name, spec)| {
            let style = Style::parse(spec).unwrap();
            let coloured = |c: Option<&rich::Color>| c.is_some_and(|c| !c.is_default());
            !COLOUR_NAMES.contains(name) && (coloured(style.color()) || coloured(style.bgcolor()))
        })
        .map(|(name, _)| *name)
        .collect();
    assert!(coloured.len() > 80, "{}", coloured.len());
    for (id, _) in official::THEMES {
        let names: Vec<String> = styles(id).into_iter().map(|(name, _)| name).collect();
        for name in &names {
            assert!(known.contains(&name.as_str()), "{id}: unknown style {name}");
        }
        for name in &coloured {
            assert!(names.iter().any(|n| n == name), "{id}: {name} is not set");
        }
    }
}

/// `mono` has no colour at all (the terminal's default colour aside), and
/// covers every coloured default style, so with it nothing is coloured but
/// by the colour names themselves.
#[test]
fn mono_has_no_colour() {
    for (name, style) in styles("mono") {
        assert!(style.color().is_none_or(|c| c.is_default()), "{name}");
        assert!(style.bgcolor().is_none_or(|c| c.is_default()), "{name}");
    }
    assert!(background("mono").is_none());
}

/// Every text colour meets WCAG AA (4.5:1) against its background (the
/// style's own, or the one the theme names), and `high-contrast` meets AAA
/// (7:1) and uses no dim text. Lines and borders meet 3:1, and a progress
/// bar's filled part 3:1 against its track. Colours are 24-bit, so the
/// ratios are the ones a truecolor terminal shows.
#[test]
fn every_coloured_theme_meets_wcag_contrast() {
    for (id, _) in official::THEMES {
        let Some(page) = background(id) else {
            continue;
        };
        let strict = id == "high-contrast";
        let text_minimum = if strict { 7.0 } else { 4.5 };
        let all = styles(id);
        let mut checked = 0;
        for (name, style) in &all {
            assert_ne!(style.dim(), Some(true), "{id}: {name} is dim");
            let Some(color) = style.color().filter(|c| !c.is_default()) else {
                continue;
            };
            assert_eq!(color.system(), ColorSystem::Truecolor, "{id}: {name}");
            let behind = style
                .bgcolor()
                .map(|c| c.get_truecolor().unwrap())
                .unwrap_or(page);
            let ratio = contrast(color.get_truecolor().unwrap(), behind);
            if name == "bar.back" {
                continue;
            }
            let minimum = if is_graphic(name) { 3.0 } else { text_minimum };
            assert!(
                ratio >= minimum,
                "{id}: {name} is {ratio:.2}:1, under {minimum}:1"
            );
            checked += 1;
        }
        assert!(checked > 80, "{id}: {checked}");
        let colour = |name: &str| {
            let (_, style) = all.iter().find(|(n, _)| n == name).unwrap();
            style.color().unwrap().get_truecolor().unwrap()
        };
        let bar = contrast(colour("bar.complete"), colour("bar.back"));
        assert!(bar >= 3.0, "{id}: the bar is {bar:.2}:1 against its track");
    }
}

/// The SGR parameters of every escape sequence in `text`.
fn sgr_parameters(text: &str) -> Vec<Vec<u32>> {
    text.split('\u{1b}')
        .filter_map(|part| part.strip_prefix('['))
        .filter_map(|part| part.split_once('m').map(|(params, _)| params))
        .map(|params| {
            params
                .split(';')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse().ok())
                .collect()
        })
        .collect()
}

/// Each theme renders through `--theme-file`: a Markdown sample, printed
/// values (numbers in the theme's `repr.number` colour) and JSON (keys in
/// its `json.key` colour); mono with attributes only. The Markdown is only
/// checked to render: core's `Markdown` does not read `markdown.*` styles
/// from the console's theme yet.
#[test]
fn every_theme_renders_through_theme_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("sample.md"),
        "# Release notes\n\nSome **bold** text and `code`.\n\n- one\n- two\n\n> quoted\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("sample.json"),
        r#"{"name": "rich", "stars": 42, "tags": [true, null]}"#,
    )
    .unwrap();
    let sgr = |c: ColorTriplet| format!("38;2;{};{};{}", c.red, c.green, c.blue);
    for (id, _) in official::THEMES {
        let path = official::theme_path(id);
        let path = path.to_str().unwrap();
        let mut outputs = Vec::new();
        for args in [
            &["-m", "sample.md"][..],
            &["-p", "[1, 2.5, None, True, 'text']"],
            &["--json", "sample.json"],
        ] {
            let mut all = vec!["--theme-file", path, "--force-terminal"];
            all.extend_from_slice(args);
            let out = rich(dir.path(), &all);
            assert!(out.status.success(), "{id} {args:?}: {}", stderr(&out));
            outputs.push(stdout(&out));
        }
        let [markdown, printed, json] = &outputs[..] else {
            unreachable!()
        };
        assert!(markdown.contains("Release notes"), "{id}: {markdown:?}");
        assert!(printed.contains("None"), "{id}: {printed:?}");
        assert!(json.contains("stars"), "{id}: {json:?}");

        if id == "mono" {
            for params in [printed, json].into_iter().flat_map(|o| sgr_parameters(o)) {
                for p in &params {
                    // 39 and 49 are the terminal's own colours.
                    assert!(
                        !matches!(p, 30..=38 | 40..=48 | 90..=107),
                        "{id}: a colour code {p} in {params:?}"
                    );
                }
            }
            assert!(printed.contains("\u{1b}[1"), "{id}: {printed:?}");
            continue;
        }
        let theme = theme(id);
        let colour = |name: &str| {
            sgr(theme
                .get(name)
                .unwrap()
                .color()
                .unwrap()
                .get_truecolor()
                .unwrap())
        };
        assert!(
            printed.contains(&colour("repr.number")),
            "{id}: {printed:?}"
        );
        assert!(json.contains(&colour("json.key")), "{id}: {json:?}");
        assert!(json.contains(&colour("json.number")), "{id}: {json:?}");
    }
}

// ---------------------------------------------------------------------------
// WASM plugins
// ---------------------------------------------------------------------------

#[cfg(feature = "wasm-plugins")]
mod wasm {
    use super::*;
    use rich_ext::plugin_loading::{load_wasm_bytes, RuntimePlugin, WasmLimits};

    fn load(id: &str, version: &str) -> RuntimePlugin {
        let bytes = official::official_wasm(id, version);
        let path = std::path::PathBuf::from(format!("{id}-{version}.wasm"));
        load_wasm_bytes(&bytes, &path, &WasmLimits::default()).unwrap()
    }

    /// Run the capability named `name` on `input`.
    fn run(plugin: &RuntimePlugin, name: &str, input: &str) -> Result<String, String> {
        let index = plugin
            .abi()
            .capabilities
            .iter()
            .position(|c| c.name == name)
            .unwrap_or_else(|| panic!("no capability {name}"));
        plugin.call(index, input, 80)
    }

    fn check(plugin: &RuntimePlugin, name: &str, cases: &[(&str, &str)]) {
        for (input, expected) in cases {
            assert_eq!(
                run(plugin, name, input).as_deref(),
                Ok(*expected),
                "{name}({input:?})"
            );
        }
    }

    /// The manifests are what the listings state: name, version and
    /// capabilities, in this order.
    #[test]
    fn the_manifests_are_the_listings() {
        let caps = |p: &RuntimePlugin| -> Vec<String> {
            p.abi()
                .capabilities
                .iter()
                .map(|c| format!("{} {}", c.kind.as_str(), c.name))
                .collect()
        };
        let textcase = load("textcase", "0.2.0");
        assert_eq!(textcase.abi().name, "textcase");
        assert_eq!(textcase.abi().version, "0.2.0");
        assert_eq!(
            caps(&textcase),
            [
                "transform upcase",
                "transform downcase",
                "fence-ansi loud",
                "transform titlecase",
                "transform snakecase",
                "transform kebabcase",
                "transform camelcase",
            ]
        );
        let wordplay = load("wordplay", "0.1.0");
        assert_eq!(wordplay.abi().name, "wordplay");
        assert_eq!(wordplay.abi().version, "0.1.0");
        assert_eq!(
            caps(&wordplay),
            ["transform rot13", "transform reverse", "transform slug"]
        );
        // A capability number past the manifest's is the plugin's error.
        for (plugin, index) in [(&textcase, 7), (&wordplay, 3)] {
            assert_eq!(
                plugin.call(index, "x", 80),
                Err("unknown capability".to_string())
            );
        }
    }

    /// textcase 0.1.0's capabilities do in 0.2.0 what they did.
    #[test]
    fn textcase_keeps_its_first_capabilities() {
        let plugin = load("textcase", "0.2.0");
        check(&plugin, "upcase", &[("Héllo, world", "HéLLO, WORLD")]);
        check(&plugin, "downcase", &[("QUIET Please", "quiet please")]);
        check(&plugin, "loud", &[("hi ü", "\u{1b}[1mHI ü\u{1b}[0m")]);
    }

    #[test]
    fn titlecase() {
        let plugin = load("textcase", "0.2.0");
        check(
            &plugin,
            "titlecase",
            &[
                ("the QUICK brown fox", "The Quick Brown Fox"),
                ("don't stop 'quoted'", "Don't Stop 'Quoted'"),
                ("3rd place, hello-world", "3rd Place, Hello-World"),
                ("", ""),
                // Non-ASCII passes through, and is part of its word.
                ("élan vital, ÜBER alles", "élan Vital, Über Alles"),
                ("日本 tokyo", "日本 Tokyo"),
            ],
        );
    }

    #[test]
    fn identifier_cases() {
        let plugin = load("textcase", "0.2.0");
        check(
            &plugin,
            "snakecase",
            &[
                ("Hello World", "hello_world"),
                ("parseHTTP2Header", "parse_http2_header"),
                ("HTTPServer error", "http_server_error"),
                ("  already_snake--and-kebab  ", "already_snake_and_kebab"),
                ("one two\nThree Four", "one_two\nthree_four"),
                ("", ""),
                ("Crème Brûlée", "crème_brûlée"),
                ("😀 smile", "😀_smile"),
            ],
        );
        check(
            &plugin,
            "kebabcase",
            &[
                ("Hello World", "hello-world"),
                ("someValue_42", "some-value-42"),
                ("  Crème Brûlée -- recipe  ", "crème-brûlée-recipe"),
            ],
        );
        check(
            &plugin,
            "camelcase",
            &[
                ("hello big_world", "helloBigWorld"),
                ("XML http request", "xmlHttpRequest"),
                ("Already camelCase", "alreadyCamelCase"),
                ("über cool straße", "überCoolStraße"),
                ("a b\nc d", "aB\ncD"),
            ],
        );
    }

    #[test]
    fn rot13() {
        let plugin = load("wordplay", "0.1.0");
        check(
            &plugin,
            "rot13",
            &[
                ("Hello, World!", "Uryyb, Jbeyq!"),
                ("Uryyb, Jbeyq!", "Hello, World!"),
                ("Grüße 123", "Teüßr 123"),
                ("", ""),
            ],
        );
    }

    #[test]
    fn reverse_by_cluster() {
        let plugin = load("wordplay", "0.1.0");
        check(
            &plugin,
            "reverse",
            &[
                ("hello", "olleh"),
                ("", ""),
                // Line by line, the line endings kept.
                ("abc\ndef\r\n", "cba\nfed\r\n"),
                ("\n\nab", "\n\nba"),
                ("日本語", "語本日"),
                // A combining accent stays on its letter.
                ("ae\u{301}x", "xe\u{301}a"),
                // A flag is two regional indicators.
                ("🇫🇷🇩🇪", "🇩🇪🇫🇷"),
                // A ZWJ family, a skin tone, a variation selector.
                ("a👨\u{200d}👩\u{200d}👧b", "b👨\u{200d}👩\u{200d}👧a"),
                ("👍🏽!", "!👍🏽"),
                ("x❤\u{fe0f}y", "y❤\u{fe0f}x"),
            ],
        );
    }

    #[test]
    fn slug() {
        let plugin = load("wordplay", "0.1.0");
        check(
            &plugin,
            "slug",
            &[
                ("Hello, World!", "hello-world"),
                ("  a--b__c  ", "a-b-c"),
                ("One\nTwo Three", "one\ntwo-three"),
                ("!!!", ""),
                ("Crème Brûlée!", "crème-brûlée"),
            ],
        );
    }

    /// Every transform takes 256 KiB of mixed text within the default
    /// fuel and memory limits, and returns UTF-8 (the host refuses
    /// anything else).
    #[test]
    fn every_transform_fits_the_default_limits() {
        let input = "Hello wörld, parseHTTPRequest 👍🏽 e\u{301}!\n".repeat(256 * 1024 / 40);
        for (id, version) in official::WASM {
            let plugin = load(id, version);
            for capability in &plugin.abi().capabilities {
                let out = run(&plugin, &capability.name, &input)
                    .unwrap_or_else(|e| panic!("{id} {}: {e}", capability.name));
                assert!(!out.is_empty(), "{id} {}", capability.name);
            }
        }
    }

    /// Through the CLI: `plugins info` lists the capabilities, and
    /// `--transform` runs them on `--print`.
    #[test]
    fn the_cli_runs_them() {
        let dir = tempfile::tempdir().unwrap();
        let textcase = dir.path().join("textcase.wasm");
        std::fs::write(&textcase, official::official_wasm("textcase", "0.2.0")).unwrap();
        let wordplay = dir.path().join("wordplay.wasm");
        std::fs::write(&wordplay, official::official_wasm("wordplay", "0.1.0")).unwrap();
        let (textcase, wordplay) = (textcase.to_str().unwrap(), wordplay.to_str().unwrap());

        let out = rich(
            dir.path(),
            &["plugins", "info", "wordplay", "--plugin", wordplay],
        );
        assert!(out.status.success(), "{}", stderr(&out));
        for expected in ["0.1.0", "transform \"rot13\"", "transform \"slug\""] {
            assert!(stdout(&out).contains(expected), "{}", stdout(&out));
        }

        for (plugin, transform, input, expected) in [
            (
                textcase,
                "titlecase",
                "the quick brown fox",
                "The Quick Brown Fox",
            ),
            (textcase, "snakecase", "Hello World", "hello_world"),
            (wordplay, "rot13", "Hello, World!", "Uryyb, Jbeyq!"),
            (wordplay, "reverse", "drawer", "reward"),
        ] {
            let out = rich(
                dir.path(),
                &["--plugin", plugin, "--transform", transform, "-p", input],
            );
            assert!(out.status.success(), "{transform}: {}", stderr(&out));
            assert!(
                stdout(&out).contains(expected),
                "{transform}: {}",
                stdout(&out)
            );
        }
    }
}

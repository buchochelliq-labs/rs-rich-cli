//! `LumisHighlighter` against the `CodeHighlighter` contract, and through
//! `Syntax`, Markdown and the plugin host, using public items only.

use std::path::Path;
use std::sync::Arc;

use rich::color::ColorSystem;
use rich::markdown::Markdown;
use rich::protocol::{CodeHighlighter, HighlightError, HighlightedCode};
use rich::syntax::Syntax;
use rich::Console;
use rich_ext::plugin::Capability;
use rich_ext::ExtensionRegistry;
use rich_lumis::{LumisHighlighter, LumisPlugin, DEFAULT_THEME};

const RUST: &str =
    "/// Docs\nfn main() {\n    let s = \"multi\nline\";\n    println!(\"{s}\"); // é 日本\n}\n";

/// The contract: one line per `split('\n')` element; spans sorted, disjoint,
/// inside their line and on character boundaries.
fn check_contract(code: &str, highlighted: &HighlightedCode) {
    let lines: Vec<&str> = code.split('\n').collect();
    assert_eq!(highlighted.lines.len(), lines.len(), "{code:?}");
    for (line, text) in highlighted.lines.iter().zip(&lines) {
        let mut end = 0;
        for span in &line.spans {
            assert!(span.range.start >= end, "overlap in {text:?}");
            assert!(span.range.start < span.range.end, "empty span in {text:?}");
            assert!(span.range.end <= text.len(), "out of range in {text:?}");
            assert!(
                text.is_char_boundary(span.range.start) && text.is_char_boundary(span.range.end)
            );
            end = span.range.end;
        }
    }
}

fn truecolor(width: usize) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .build()
}

#[test]
fn the_contract_holds_for_every_input_shape() {
    let highlighter = LumisHighlighter::new();
    let inputs = [
        "",
        "\n",
        "x",
        "fn x() {}\n\n\n",
        "a\r\nb\r\n",
        RUST,
        "\u{feff}fn é() {}",
        "\"unterminated",
    ];
    for theme in ["monokai", "github_light", "ansi_dark", "ansi_light"] {
        for language in [
            Some("rust"),
            Some("python"),
            Some("rs"),
            Some("no-such"),
            None,
        ] {
            for code in inputs {
                let highlighted = highlighter
                    .highlight(code, language, theme)
                    .unwrap_or_else(|e| panic!("{theme} {language:?} {code:?}: {e}"));
                check_contract(code, &highlighted);
            }
        }
    }
}

#[test]
fn themes_colour_tokens_and_the_background() {
    let highlighter = LumisHighlighter::new();
    let code = highlighter
        .highlight(RUST, Some("rust"), "dracula")
        .unwrap();
    // Dracula's background, and at least one of its keyword colours.
    assert_eq!(
        code.background.as_ref().map(|c| format!("{c:?}")),
        Some(format!(
            "{:?}",
            rich::color::Color::parse("#282a36").unwrap()
        ))
    );
    let out = truecolor(60).render_to_string(
        &Syntax::new(RUST, "rust")
            .highlighter(LumisHighlighter::shared())
            .theme("dracula"),
    );
    assert!(out.contains("48;2;40;42;54"), "dracula background: {out:?}");
    assert!(
        out.contains("38;2;255;121;198") || out.contains("38;2;139;233;253"),
        "{out:?}"
    );
    // Different themes differ.
    let light = truecolor(60).render_to_string(
        &Syntax::new(RUST, "rust")
            .highlighter(LumisHighlighter::shared())
            .theme("github_light"),
    );
    assert_ne!(out, light);
}

#[test]
fn ansi_themes_use_the_terminals_colours() {
    let dark = truecolor(60).render_to_string(
        &Syntax::new("def greet(name):\n    return 42  # hi\n", "python")
            .highlighter(LumisHighlighter::shared())
            .theme("ansi_dark"),
    );
    assert!(
        dark.contains("\x1b[94mdef"),
        "keyword bright_blue: {dark:?}"
    );
    assert!(
        dark.contains("\x1b[92mgreet"),
        "function bright_green: {dark:?}"
    );
    assert!(dark.contains("\x1b[94m42"), "number bright_blue: {dark:?}");
    assert!(dark.contains("\x1b[2m# hi"), "comment dim: {dark:?}");
    assert!(
        !dark.contains("38;2;") && !dark.contains("48;2;"),
        "no RGB: {dark:?}"
    );
    let light = truecolor(60).render_to_string(
        &Syntax::new("def f(): pass\n", "python")
            .highlighter(LumisHighlighter::shared())
            .theme("ansi_light"),
    );
    assert!(light.contains("\x1b[34mdef"), "keyword blue: {light:?}");
}

#[test]
fn unknown_themes_and_languages() {
    let highlighter = LumisHighlighter::new();
    assert_eq!(
        highlighter
            .highlight("x", Some("rust"), "no-such-theme")
            .unwrap_err(),
        HighlightError::UnknownTheme("no-such-theme".into())
    );
    // Syntax falls back to the default theme rather than failing.
    let fallback = truecolor(40).render_to_string(
        &Syntax::new("fn x() {}", "rust")
            .highlighter(LumisHighlighter::shared())
            .theme("no-such-theme"),
    );
    let default = truecolor(40).render_to_string(
        &Syntax::new("fn x() {}", "rust")
            .highlighter(LumisHighlighter::shared())
            .theme(DEFAULT_THEME),
    );
    assert_eq!(fallback, default);
    // An unknown language is plain text: no token styles.
    let plain = highlighter
        .highlight("fn x() {}", Some("no-such-language"), "ansi_dark")
        .unwrap();
    assert!(
        plain.lines.iter().all(|line| line.spans.is_empty()),
        "{plain:?}"
    );
}

#[test]
fn themes_languages_and_paths_are_listed() {
    let highlighter = LumisHighlighter::new();
    let themes = highlighter.themes();
    for name in [
        "monokai",
        "dracula",
        "github_light",
        "ansi_dark",
        "ansi_light",
    ] {
        assert!(themes.iter().any(|t| t == name), "{name}");
    }
    assert!(themes.contains(&highlighter.default_theme().to_string()));
    let languages = highlighter.languages();
    assert!(languages.len() > 50, "{}", languages.len());
    assert!(languages.iter().any(|l| l == "rust"));
    assert_eq!(
        highlighter
            .language_for_path(Path::new("src/main.rs"))
            .as_deref(),
        Some("rust")
    );
    assert_eq!(
        highlighter
            .language_for_path(Path::new("a/b.py"))
            .as_deref(),
        Some("python")
    );
    assert_eq!(
        highlighter.language_for_path(Path::new("notes.unknownext")),
        None
    );
}

#[test]
fn a_multi_line_string_styles_its_line_break() {
    let code = LumisHighlighter::new()
        .highlight("let s = \"a\nb\";", Some("rust"), "ansi_dark")
        .unwrap();
    assert!(code.lines[0].newline_style.is_some(), "{code:?}");
    assert!(code.lines[1].newline_style.is_none());
}

#[test]
fn markdown_code_blocks_and_the_plugin_host() {
    let mut registry = ExtensionRegistry::with_defaults();
    registry.add_plugin(&LumisPlugin).unwrap();
    assert_eq!(
        registry.provided_by(&Capability::CodeHighlighter("lumis".into())),
        Some("lumis")
    );
    assert_eq!(registry.code_highlighter_names(), ["lumis", "syntect"]);
    let lumis = registry.code_highlighter("lumis").unwrap();
    let markdown = Markdown::new("```python\ndef f(): pass\n```")
        .highlighter(lumis.clone())
        .code_theme("ansi_dark");
    let out = truecolor(40).render_to_string(&markdown);
    assert!(out.contains("\x1b[94mdef"), "{out:?}");
    // The same engine behind an `Arc<dyn CodeHighlighter>` highlights alike.
    let direct: Arc<dyn CodeHighlighter> = LumisHighlighter::shared();
    assert_eq!(
        lumis.highlight("x = 1", Some("python"), "monokai").unwrap(),
        direct
            .highlight("x = 1", Some("python"), "monokai")
            .unwrap()
    );
}

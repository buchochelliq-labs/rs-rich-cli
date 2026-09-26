//! The 0.0.12 code-highlighting and fence seams at their edges: layouts that
//! must match rich 15.0.0, narrow widths, fence renderer output, and the
//! console-wide default highlighter reaching inline code. Expected layouts were
//! captured from rich 15.0.0.

use rich::markdown::Markdown;
use rich::r#box::{
    ASCII_DOUBLE_HEAD, HEAVY_HEAD, MINIMAL_DOUBLE_HEAD, MINIMAL_HEAVY_HEAD, ROUNDED,
    SQUARE_DOUBLE_HEAD,
};
use rich::{Console, Syntax, Table};

fn plain(width: usize) -> Console {
    Console::builder().width(width).color_system(None).build()
}

/// Clean check: `show_header(false)` swaps in upstream's plain-headed box.
/// Expected strings captured from rich 15.0.0 (syn.py).
#[test]
fn plain_headed_boxes_match_upstream() {
    let expected = [
        (
            HEAVY_HEAD,
            "┌───┬───┐\n│ 1 │ 2 │\n├───┼───┤\n│ 3 │ 4 │\n└───┴───┘",
        ),
        (
            SQUARE_DOUBLE_HEAD,
            "┌───┬───┐\n│ 1 │ 2 │\n├───┼───┤\n│ 3 │ 4 │\n└───┴───┘",
        ),
        (
            MINIMAL_DOUBLE_HEAD,
            "    ╷    \n  1 │ 2  \n╶───┼───╴\n  3 │ 4  \n    ╵    ",
        ),
        (
            MINIMAL_HEAVY_HEAD,
            "    ╷    \n  1 │ 2  \n╶───┼───╴\n  3 │ 4  \n    ╵    ",
        ),
        (
            ASCII_DOUBLE_HEAD,
            "+---+---+\n| 1 | 2 |\n+---+---+\n| 3 | 4 |\n+---+---+",
        ),
        (
            ROUNDED,
            "╭───┬───╮\n│ 1 │ 2 │\n├───┼───┤\n│ 3 │ 4 │\n╰───┴───╯",
        ),
    ];
    for (b, want) in expected {
        let mut t = Table::new().box_set(b).show_header(false).show_lines(true);
        t.add_column("A");
        t.add_column("B");
        t.add_row(&["1", "2"]);
        t.add_row(&["3", "4"]);
        assert_eq!(plain(20).render_to_string(&t), want);
    }
}

/// Clean check: Syntax layout for edge sources matches rich 15.0.0 (syn.py).
#[test]
fn syntax_edge_layout_matches_upstream() {
    let cases: &[(&str, &str, &str)] = &[
        ("", "            ", "      \n      \n      "),
        (
            "a\n\n",
            "a           \n            \n            ",
            "      \n a    \n      \n      \n      ",
        ),
        (
            "a\r\nb\r\n",
            "a           \nb           \n            ",
            "      \n a    \n b    \n      \n      ",
        ),
        (
            "\tx\ty",
            "    x   y   ",
            "      \n      \n x    \n y    \n      ",
        ),
        ("a\u{8}b\u{7}c", "abc         ", "      \n abc  \n      "),
        (
            "a\u{1b}[31mb",
            "a\u{1b}[31mb      ",
            "      \n a\u{1b}[31 \n mb   \n      ",
        ),
        (
            "a\u{85}b",
            "a\u{85}b          ",
            "      \n a\u{85}b   \n      ",
        ),
    ];
    for (code, flat, wrapped) in cases {
        assert_eq!(
            plain(12).render_to_string(&Syntax::new(*code, "python")),
            *flat,
            "{code:?}"
        );
        assert_eq!(
            plain(6).render_to_string(&Syntax::new(*code, "python").word_wrap(true).padding(1)),
            *wrapped,
            "{code:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Probes (edge input / seams)
// ---------------------------------------------------------------------------

use rich::cells::cell_len;
use rich::protocol::{
    CodeHighlighter, CodeHighlighting, ConsoleCodeHighlighting, FenceRenderer, HighlightError,
    HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::segment::Segment;
use rich::{ConsoleOptions, Style};
use std::sync::Arc;

/// Underlines every non-empty line.
struct Underline;
impl CodeHighlighter for Underline {
    fn highlight(
        &self,
        code: &str,
        _language: Option<&str>,
        _theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        let u = Style::parse("underline").unwrap();
        Ok(HighlightedCode {
            lines: code
                .split('\n')
                .map(|l| HighlightedLine {
                    spans: (!l.is_empty())
                        .then(|| HighlightSpan {
                            range: 0..l.len(),
                            style: u.clone(),
                        })
                        .into_iter()
                        .collect(),
                    newline_style: None,
                })
                .collect(),
            ..Default::default()
        })
    }
    fn default_theme(&self) -> &str {
        "u"
    }
    fn themes(&self) -> Vec<String> {
        vec!["u".into()]
    }
    fn languages(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Returns a fixed (hostile) payload for `lang == "x"`.
struct Echo(String);
impl FenceRenderer for Echo {
    fn render_fence(
        &self,
        language: &str,
        _code: &str,
        _console: &Console,
        _options: &ConsoleOptions,
    ) -> Option<Vec<Segment>> {
        (language == "x").then(|| vec![Segment::new(self.0.clone(), None), Segment::line()])
    }
}

#[test]
fn probe_narrow_widths_do_not_panic_or_hang() {
    for w in 0..4 {
        for code in ["中文字", "a\tb", "", "\n", "abc def ghi"] {
            let _ = plain(w).render_to_string(&Syntax::new(code, "python").word_wrap(true));
            let _ =
                plain(w).render_to_string(&Syntax::new(code, "python").word_wrap(true).padding(2));
            let md = format!("> - ```python\n>   {code}\n>   ```\n");
            let _ = plain(w).render_to_string(&Markdown::new(&md));
        }
    }
}

/// Clean check: an over-wide FenceRenderer result is cropped by the console,
/// and a hostile fence info string never reaches the output.
#[test]
fn fence_renderer_output_is_cropped_and_info_string_not_echoed() {
    let out = plain(20).render_to_string(
        &Markdown::new("> ```x\n> y\n> ```\n\n```\u{1b}]0;pwn\u{7}\nz\n```")
            .fence_renderer(Arc::new(Echo("W".repeat(50)))),
    );
    for line in out.split('\n') {
        assert!(cell_len(line) <= 20, "{line:?}");
    }
    assert!(!out.contains('\u{1b}'), "{out:?}");
}

/// Finding: a console-wide default highlighter reaches Markdown code blocks
/// but not highlighted inline code (`inline_code_lexer`), which is highlighted
/// at parse time with `Syntax::highlight()` (no console) and so always uses
/// syntect's base16 theme — one document, two engines/themes.
#[test]
fn console_default_highlighter_reaches_inline_code() {
    let mut console = Console::builder()
        .width(40)
        .force_terminal(true)
        .color_system(Some(rich::ColorSystem::Truecolor))
        .build();
    console.set_code_highlighting(Some(CodeHighlighting {
        highlighter: Arc::new(Underline),
        theme: None,
    }));
    let md =
        Markdown::new("Call `go()` now.\n\n```rust\nfn main() {}\n```").inline_code_lexer("rust");
    let out = console.render_to_string(&md);
    assert!(out.contains("\u{1b}[4mfn main() {}"), "block: {out:?}");
    assert!(
        out.contains("\u{1b}[4mgo()"),
        "inline code ignored the console highlighter: {out:?}"
    );
}

// ---------------------------------------------------------------------------
// Regression check: default Syntax output vs the 0.0.11 algorithm, re-derived
// here from syntect directly (tokens -> segments, trailing blank row, padded).
// ---------------------------------------------------------------------------

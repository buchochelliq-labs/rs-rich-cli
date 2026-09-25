//! The "Code highlighters" guide: choosing a highlighter, a custom adapter in
//! about 40 lines, and running it through the conformance kit.
//!
//! `cargo run -p rs-rich-ext --example guide_highlighters --features testing`

use std::sync::Arc;

use rich::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::{Console, Style, Syntax, SyntectHighlighter};
use rich_ext::testing::conformance;
use rich_ext::ExtensionRegistry;

// --8<-- [start:adapter]
/// Highlights a handful of Rust keywords, and nothing else.
struct Keywords;

const KEYWORDS: &[&str] = &["fn", "let", "mut", "if", "else", "return", "struct"];

impl CodeHighlighter for Keywords {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        let style = match theme {
            "bold" => Style::parse("bold").unwrap(),
            "blue" => Style::parse("bright_blue").unwrap(),
            other => return Err(HighlightError::UnknownTheme(other.to_string())),
        };
        let rust = matches!(language, Some("rust" | "rs"));
        // One line per `split('\n')` element; spans are byte ranges in the line.
        let lines = code
            .split('\n')
            .map(|line| {
                let mut spans = Vec::new();
                let mut start = 0;
                for word in line.split(|c: char| !c.is_alphanumeric() && c != '_') {
                    if rust && KEYWORDS.contains(&word) {
                        spans.push(HighlightSpan {
                            range: start..start + word.len(),
                            style: style.clone(),
                        });
                    }
                    start += word.len()
                        + line[start + word.len()..]
                            .chars()
                            .next()
                            .map_or(0, char::len_utf8);
                }
                HighlightedLine {
                    spans,
                    newline_style: None,
                }
            })
            .collect();
        Ok(HighlightedCode {
            lines,
            ..Default::default()
        })
    }
    fn default_theme(&self) -> &str {
        "bold"
    }
    fn themes(&self) -> Vec<String> {
        vec!["bold".into(), "blue".into()]
    }
    fn languages(&self) -> Vec<String> {
        vec!["rust".into()]
    }
}
// --8<-- [end:adapter]

fn main() {
    let console = Console::new();

    // --8<-- [start:use]
    // One block of code, with an engine and one of its themes.
    let code = "fn main() {\n    let answer = 42;\n}\n";
    console.print(
        &Syntax::new(code, "rust")
            .highlighter(Arc::new(Keywords))
            .theme("blue"),
    );

    // Or every block a console renders: register it and choose it by name.
    let mut registry = ExtensionRegistry::with_defaults();
    registry
        .register_code_highlighter("keywords", Arc::new(Keywords))
        .unwrap();
    registry
        .set_default_code_highlighter("keywords", None)
        .unwrap();
    let mut console = Console::new();
    registry.install(&mut console);
    console.print(&Syntax::new(code, "rust"));
    // --8<-- [end:use]

    // --8<-- [start:ansi]
    // The ANSI themes use the terminal's own 16 colours, so code follows the
    // user's terminal palette. Both shipped adapters have them.
    console.print(
        &Syntax::new(
            "def greet(name):\n    return f\"hi {name}\"  # ok\n",
            "python",
        )
        .highlighter(SyntectHighlighter::shared())
        .theme("ansi_dark"),
    );
    // --8<-- [end:ansi]

    // --8<-- [start:conformance]
    // The same kit the shipped adapters run in CI.
    conformance::check(Arc::new(Keywords)).expect("Keywords conforms");
    // --8<-- [end:conformance]
    println!("Keywords passes the conformance kit");
}

#[cfg(test)]
mod tests {
    use super::*;

    // --8<-- [start:test]
    #[test]
    fn keywords_conforms() {
        conformance::check(Arc::new(Keywords)).unwrap_or_else(|error| panic!("{error}"));
    }
    // --8<-- [end:test]
}

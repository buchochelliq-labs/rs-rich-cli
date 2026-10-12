//! Guide: Code highlighters — run: cargo run -p rs-rich-ext --example guide_highlighters --features testing,syntax [-- --svg docs/media/guide]
//!
//! The snippets and pictures on `docs/guide/ext/code-highlighters.md` come
//! from this file: choosing a highlighter, a custom adapter in about 40
//! lines, and running it through the conformance kit.

use std::path::PathBuf;
use std::sync::Arc;

use rich::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::{ColorSystem, Console, Style, Syntax, SyntectHighlighter};
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

// --8<-- [start:default]
const CODE: &str =
    "fn main() {\n    let answer = 42;\n    if answer > 40 {\n        return;\n    }\n}\n";

// No highlighter given: syntect, with its default theme.
fn default_engine(console: &Console) {
    console.print(&Syntax::new(CODE, "rust"));
}
// --8<-- [end:default]

// --8<-- [start:use]
// One block of code, with an engine and one of its themes.
fn one_block(console: &Console) {
    console.print(
        &Syntax::new(CODE, "rust")
            .highlighter(Arc::new(Keywords))
            .theme("blue"),
    );
}

// Or every block a console renders: register it, choose it by name and
// install the registry on the console.
fn install_keywords(console: &mut Console) {
    let mut registry = ExtensionRegistry::with_defaults();
    registry
        .register_code_highlighter("keywords", Arc::new(Keywords))
        .unwrap();
    registry
        .set_default_code_highlighter("keywords", None)
        .unwrap();
    registry.install(console);
}

fn every_block(console: &Console) {
    console.print(&Syntax::new(CODE, "rust"));
}
// --8<-- [end:use]

// --8<-- [start:ansi]
// The ANSI themes use the terminal's own 16 colours, so code follows the
// user's terminal palette. Both shipped adapters have them.
fn ansi(console: &Console) {
    console.print(
        &Syntax::new(
            "def greet(name):\n    return f\"hi {name}\"  # ok\n",
            "python",
        )
        .highlighter(SyntectHighlighter::shared())
        .theme("ansi_dark"),
    );
}
// --8<-- [end:ansi]

fn main() {
    let shots = Shots::from_args("guide_highlighters");
    shots.shot("default", 40, |_| {}, default_engine);
    shots.shot("one", 40, |_| {}, one_block);
    shots.shot("every", 40, install_keywords, every_block);
    shots.shot("ansi", 40, |_| {}, ansi);

    // --8<-- [start:conformance]
    // The same kit the shipped adapters run in CI.
    conformance::check(Arc::new(Keywords)).expect("Keywords conforms");
    // --8<-- [end:conformance]
    eprintln!("Keywords passes the conformance kit");
}

/// `--svg DIR` writes each shot as `DIR/<topic>-<shot>.svg` from a pinned
/// console; without it, shots print to the terminal.
struct Shots {
    topic: &'static str,
    dir: Option<PathBuf>,
}

impl Shots {
    fn from_args(topic: &'static str) -> Self {
        let args: Vec<String> = std::env::args().collect();
        let dir = args
            .iter()
            .position(|a| a == "--svg")
            .map(|i| PathBuf::from(args.get(i + 1).expect("--svg takes a directory")));
        Shots { topic, dir }
    }

    /// Run `setup` on the console (to install a registry), then the shot.
    fn shot(
        &self,
        name: &str,
        width: usize,
        setup: impl FnOnce(&mut Console),
        f: impl FnOnce(&Console),
    ) {
        let Some(dir) = &self.dir else {
            let mut console = Console::new();
            setup(&mut console);
            return f(&console);
        };
        let mut console = Console::builder()
            .width(width)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build();
        setup(&mut console);
        let id = format!("{}-{name}", self.topic);
        let svg = console.export_svg(&id, &id, f);
        std::fs::create_dir_all(dir).expect("create the SVG directory");
        let path = dir.join(format!("{id}.svg"));
        std::fs::write(&path, svg).expect("write the SVG");
        eprintln!("wrote {}", path.display());
    }
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

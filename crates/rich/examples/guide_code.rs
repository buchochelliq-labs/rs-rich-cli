//! Guide: Code and data — run: cargo run -p rs-rich --example guide_code [-- --svg docs/media/guide]
//!
//! The snippets in docs/guide/core/code-and-data.md are cut from this file.

#[path = "guide_support/mod.rs"]
mod guide_support;

use std::collections::BTreeMap;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::markdown::Markdown;
use rich::{Console, Json, Pretty, Syntax};
// --8<-- [end:imports]

const CODE: &str = r#"use std::collections::HashMap;

/// Count words, ignoring case.
fn count(text: &str) -> HashMap<String, usize> {
	let mut counts = HashMap::new();
	for word in text.split_whitespace() {
		*counts.entry(word.to_lowercase()).or_insert(0) += 1;
	}
	counts
}
"#;

fn main() {
    let shots = Shots::from_args("guide_code");
    shots.shot("syntax", 64, syntax);
    shots.shot("syntax-options", 48, syntax_options);
    shots.shot("markdown", 64, markdown);
    shots.shot("json", 48, json);
    shots.shot("pretty", 48, pretty);
    highlight_to_text();
    if !shots.is_svg() {
        Console::new().print(&markdown_options());
    }
}

// --8<-- [start:syntax]
fn syntax(console: &Console) {
    // Language by name or file extension: "rust", "rs", "py", "toml", …
    console.print(&Syntax::new(CODE, "rust"));
}
// --8<-- [end:syntax]

// --8<-- [start:syntax_options]
fn syntax_options(console: &Console) {
    let line = "let answer = compute_the_answer(life, universe, everything); // 42";
    let snippet = format!("{line}\n\tindented_with_a_tab();");

    let syntax = Syntax::new(snippet.clone(), "rs")
        .theme("InspiredGitHub") // any syntect theme name
        .padding(1) // background margin on every side
        .tab_size(2) // tabs expand to spaces (default 4)
        .word_wrap(true); // wrap long lines instead of cropping
    console.print(&syntax);

    // The default crops long lines at the width.
    console.print(&Syntax::new(snippet, "rs").theme("Solarized (dark)"));
}
// --8<-- [end:syntax_options]

// --8<-- [start:highlight]
fn highlight_to_text() {
    // Highlight into a Text (no padding or background block), to embed or edit.
    let text = Syntax::new("x = 1", "py").highlight();
    assert_eq!(text.plain(), "x = 1");
    assert!(!text.spans().is_empty());
}
// --8<-- [end:highlight]

// --8<-- [start:markdown]
fn markdown(console: &Console) {
    let source = r#"# Release notes

Markdown renders **bold**, *italic*, `code` and [links](https://docs.rs/rs-rich).

- bullet lists
  - nested
1. and numbered ones

> Block quotes, too.

| Crate | Lib name |
|-------|---------:|
| rs-rich | `rich` |
| rs-rich-ext | `rich_ext` |

```rust
fn main() { println!("fenced code is highlighted"); }
```
"#;
    console.print(&Markdown::new(source));
}
// --8<-- [end:markdown]

// --8<-- [start:markdown_options]
fn markdown_options() -> Markdown {
    use rich::{Justify, Style};

    Markdown::new("Some *text* with a [link](https://example.com).")
        // false: write "text (url)" instead of an OSC 8 hyperlink.
        .hyperlinks(false)
        .justify(Justify::Full)
        .style(Style::parse("grey85").unwrap())
        .code_theme("base16-ocean.dark")
        // Highlight `inline code` as Rust.
        .inline_code_lexer("rust")
}
// --8<-- [end:markdown_options]

// --8<-- [start:json]
fn json(console: &Console) {
    let raw = r#"{"name": "rs-rich", "version": "0.0.7", "tags": ["terminal", "ansi"],
                  "stable": false, "downloads": 1204, "license": null}"#;
    match Json::new(raw) {
        Ok(json) => console.print(&json),
        Err(error) => console.print_str(&format!("[red]invalid JSON:[/] {error}")),
    }
}
// --8<-- [end:json]

// --8<-- [start:pretty]
#[derive(Debug)]
#[allow(dead_code)]
struct Config {
    name: &'static str,
    width: Option<usize>,
    ratio: f64,
    tags: Vec<&'static str>,
    limits: BTreeMap<&'static str, u32>,
}

fn pretty(console: &Console) {
    let config = Config {
        name: "demo",
        width: Some(80),
        ratio: 0.5,
        tags: vec!["a", "b"],
        limits: BTreeMap::from([("cpu", 4), ("mem", 8)]),
    };
    // {:#?} output, highlighted.
    console.print(&Pretty::new(&config));
    // {:?} on one line.
    console.print(&Pretty::compact(&config.tags));
}
// --8<-- [end:pretty]

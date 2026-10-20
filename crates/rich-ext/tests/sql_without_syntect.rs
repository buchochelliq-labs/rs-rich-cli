//! The consumer story for core's `code` feature: `Syntax` and Markdown with
//! SQL highlighted by `SqlHighlighter`, in a build that can leave syntect out.
//!
//! CI runs this with `--no-default-features --features
//! sql,rich/code,rich/markdown`, where `cargo tree` shows no syntect or
//! bincode (the "Lean builds" step checks that). It also runs with the
//! defaults, where syntect is present but a registered highlighter wins.

use rich::markdown::Markdown;
use rich::{ColorSystem, Console, Syntax};
use rich_ext::registry::ExtensionRegistry;
use rich_ext::sql::SqlPlugin;

/// A console with `sql` installed from the registry as the default engine.
fn console() -> Console {
    let mut registry = ExtensionRegistry::new();
    registry.add_plugin(&SqlPlugin).unwrap();
    registry
        .set_default_code_highlighter("sql", Some("ansi_dark"))
        .unwrap();
    let mut console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .width(40)
        .build();
    registry.install(&mut console);
    console
}

#[test]
fn a_syntax_block_is_highlighted_through_the_registry() {
    let out = console().render_to_string(&Syntax::new("SELECT 'x' -- c", "sql"));
    assert!(out.contains("\x1b[94mSELECT"), "{out:?}");
    assert!(out.contains("\x1b[33m'x'"), "{out:?}");
    assert!(out.contains("\x1b[2m-- c"), "{out:?}");
}

#[test]
fn a_sql_markdown_fence_is_highlighted_through_the_registry() {
    let markdown = Markdown::new("Rows:\n\n```sql\nselect 1 from {{ ref('t') }}\n```\n");
    let out = console().render_to_string(&markdown);
    assert!(out.contains("\x1b[94mselect"), "{out:?}");
    assert!(out.contains("\x1b[96m{{ ref('t') }}"), "{out:?}");
}

/// With nothing registered a build without syntect shows the code plain, in
/// the same block; with syntect the default engine colours it.
#[test]
fn without_a_registered_highlighter_the_code_is_still_shown() {
    let console = Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .width(40)
        .build();
    let out = console.render_to_string(&Markdown::new("```sql\nSELECT 1\n```\n"));
    assert!(out.contains("SELECT 1"), "{out:?}");
}

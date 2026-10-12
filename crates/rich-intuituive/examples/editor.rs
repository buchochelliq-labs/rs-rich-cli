//! A text area: Rust source, highlighted, with line numbers, over a notes
//! area that wraps and shows a placeholder while it is empty.
//!
//!     cargo run -p rs-rich-intuituive --example editor
//!
//! Type, select with Shift or the mouse, Ctrl+Z to undo and Ctrl+Y to redo,
//! Tab to indent. Escape then Tab moves to the notes; Ctrl+Q quits.

use intuituive::prelude::*;

const SOURCE: &str = r#"use std::collections::HashMap;

/// Count the words in `text`, most frequent first.
fn count(text: &str) -> Vec<(&str, usize)> {
    let mut counts = HashMap::new();
    for word in text.split_whitespace() {
        *counts.entry(word).or_insert(0) += 1;
    }
    let mut counts: Vec<_> = counts.into_iter().collect();
    counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    counts
}

fn main() {
    for (word, n) in count("the cat and the hat and the bat") {
        println!("{word:>4} {n}");
    }
}
"#;

fn main() -> std::io::Result<()> {
    editor_app().run()
}

/// The app, for the tests too.
pub fn editor_app() -> App {
    App::new(|| {
        // --8<-- [start:areas]
        let source = signal(SOURCE.to_string());
        let notes = signal(String::new());
        let code = text_area(source)
            .language("rust")
            .line_numbers()
            .soft_wrap(false)
            .autofocus()
            .panel("count.rs");
        let jotted = text_area(notes)
            .placeholder("Notes: what to change, what to test…")
            .panel("notes")
            .fixed(6);
        // --8<-- [end:areas]
        column([
            code,
            jotted,
            text!(
                "[b]count.rs[/] {} lines · [b]notes[/] {} words",
                source.with(|s| s.lines().count()),
                notes.with(|s| s.split_whitespace().count())
            )
            .fixed(1),
            label("[muted]esc tab moves on · ctrl+z undo · ctrl+y redo · ctrl+q quits").fixed(1),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}

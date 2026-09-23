//! CLI authoring: describe a command once, then render its help, errors,
//! shell completions, Markdown and man pages, config reference and config
//! precedence through rich.
//!
//! The model ([`CommandSpec`], [`ArgSpec`]) is plain data with no parser and
//! no dependencies, so any argument parser can feed it. With the `clap`
//! feature, `CommandSpec::from_clap` builds one from a `clap::Command` and the
//! `cli_doc::clap` module renders clap's help, version and errors through rich.
//!
//! ```
//! use rich::Console;
//! use rich_ext::cli_doc::{ArgSpec, CommandSpec, HelpView};
//!
//! let spec = CommandSpec::new("rich")
//!     .about("Render files in the terminal")
//!     .arg(ArgSpec::option("width").short('w').value_name("SIZE").help("Output width").heading("Layout"))
//!     .arg(ArgSpec::positional("resource").help("A file path or URL"));
//! let out = Console::builder().width(60).build().render_to_string(&HelpView::new(&spec));
//! assert!(out.starts_with("Usage: rich [OPTIONS] [RESOURCE]"));
//! assert!(out.contains("  -w, --width <SIZE>  Output width"));
//! ```
//!
//! Every renderable reads its styles from the console theme and falls back to
//! [`STYLES`], so they work without configuration; [`extended_theme`]
//! registers them too, which also makes them usable in markup.
//!
//! [`extended_theme`]: crate::theme::extended_theme

mod completion;
mod config;
mod docs;
mod error;
mod help;
mod precedence;
mod spec;

#[cfg(feature = "clap")]
pub mod clap;

pub use completion::{generate, CompletionCatalog, CompletionItem, CompletionKind, Shell};
pub use config::{ConfigEntry, ConfigReference, ConfigSource};
pub use docs::{markdown_view, to_man, to_man_pages, to_markdown};
pub use error::{suggest, CliError, CliErrorKind};
pub use help::{HelpView, STACK_BELOW};
pub use precedence::{Explanation, Layer, Precedence, PrecedenceView, Resolved};
pub use spec::{ArgSpec, Choice, CommandSpec, Example, HeadingNote, Section, ValueHint};

use rich::{Console, Segment, Style, Text};

/// Theme keys used by this module, with their default styles.
pub const STYLES: &[(&str, &str)] = &[
    ("help.usage", "bold"),
    ("help.heading", "bold yellow"),
    ("help.option", "bold cyan"),
    ("help.metavar", "cyan"),
    ("help.command", "bold green"),
    ("help.hint", "dim"),
    ("help.example", "green"),
    ("help.description", "none"),
    ("config.key", "bold"),
    ("config.winner", "bold green"),
    ("config.shadowed", "dim strike"),
    ("config.origin", "dim"),
];

/// The console theme's style for `key`, else its default from [`STYLES`].
pub(crate) fn style(console: &Console, key: &str) -> Style {
    let fallback = STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("none", |(_, spec)| spec);
    crate::event::theme_style(console, key, fallback)
}

/// Split text into paragraphs at blank lines, trimming each.
pub(crate) fn paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                out.push(current.join("\n"));
                current.clear();
            }
        } else {
            current.push(line.trim_end());
        }
    }
    if !current.is_empty() {
        out.push(current.join("\n"));
    }
    out
}

/// Wrap `text` to `width` cells and indent every line by `indent` spaces.
pub(crate) fn wrap_indented(
    console: &Console,
    text: &Text,
    indent: usize,
    width: usize,
) -> Vec<Vec<Segment>> {
    let inner = width.saturating_sub(indent).max(1);
    text.render_lines(console.theme(), &Style::new(), Some(inner))
        .into_iter()
        .map(|line| {
            let mut row = Vec::with_capacity(line.len() + 1);
            if indent > 0 {
                row.push(Segment::new(" ".repeat(indent), None));
            }
            row.extend(line);
            row
        })
        .collect()
}

/// Lines to a segment stream with newlines between them, trailing spaces
/// (that no background colour makes visible) trimmed.
pub(crate) fn join_lines(lines: Vec<Vec<Segment>>) -> Vec<Segment> {
    let trimmed = lines.into_iter().map(|line| {
        let mut line = Segment::simplify(&line);
        while let Some(last) = line.last_mut() {
            if last.control || last.style.as_ref().is_some_and(|s| s.bgcolor().is_some()) {
                break;
            }
            let kept = last.text.trim_end_matches(' ').len();
            last.text.truncate(kept);
            if !last.text.is_empty() {
                break;
            }
            line.pop();
        }
        line
    });
    crate::event::flatten(trimmed.collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_style_parses() {
        for (name, spec) in STYLES {
            assert!(Style::parse(spec).is_ok(), "{name}: {spec}");
        }
    }

    #[test]
    fn paragraphs_split_on_blank_lines() {
        assert_eq!(
            paragraphs("a\nb\n\n  \nc\n"),
            vec!["a\nb".to_string(), "c".to_string()]
        );
    }
}

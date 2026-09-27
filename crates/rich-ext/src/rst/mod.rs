//! reStructuredText, rendered as `rich-rst` 1.3.2 renders it: the
//! `RestructuredText` renderable that rich-cli 1.8.1's `--rst` prints.
//!
//! `rich-rst` walks a docutils document tree and turns each node into rich
//! renderables: section titles in double-bordered panels, admonitions in
//! titled panels, field lists as a table, code in a `Syntax` panel, and
//! paragraphs as text. This module parses the common subset of
//! reStructuredText into the same tree ([`parse`]) and builds the same
//! renderables from it ([`RestructuredText`]), quirks included: a nested
//! bullet list repeats its items, a definition list keeps its trailing
//! indent, only the last footnote reaches the footer.
//!
//! Like `rich-rst`, the parse applies none of docutils' transforms, so a
//! document title is an ordinary title, `name_` references stay unresolved
//! until their target styles them, and substitutions are not replaced.
//! Grid and simple tables render their cells as paragraphs, as the visitor
//! does. What the subset does not cover (directives other than code,
//! admonitions, images, `contents`, `raw`, `math`, `sidebar`, `rubric` and
//! substitution definitions) renders nothing, as docutils' own errors do
//! with `show_errors=False`.

mod entities;
mod inline;
mod parse;
mod render;

pub use parse::{parse, Block, DefinitionItem, Field, Inline, OptionItem};
pub use render::RestructuredText;

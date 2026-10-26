//! reStructuredText, rendered as `rich-rst` 2.2.0 renders it: the
//! `RestructuredText` renderable that rich-cli's `--rst` prints.
//!
//! `rich-rst` builds a docutils document tree, applies docutils' transforms
//! (the document title, substitutions, footnote numbers, hyperlink targets,
//! the table of contents) and turns each node into rich renderables: titles
//! in panels by section level, admonitions in titled panels, field lists as
//! a table, code in a `Syntax` panel, tables as tables (cells that span
//! drawn line by line), and paragraphs as text. This module parses the
//! common subset of reStructuredText into the same tree ([`parse`]), applies
//! the same transforms, and builds the same renderables from it
//! ([`RestructuredText`]), quirks included. Directives docutils knows but
//! this port does not build render nothing; an unknown one shows its source,
//! as upstream's error does.

mod doctree;
pub(crate) mod entities;
mod inline;
mod parse;
mod render;
mod transforms;

pub use doctree::{Element, Node};
pub use parse::parse;
pub use render::RestructuredText;

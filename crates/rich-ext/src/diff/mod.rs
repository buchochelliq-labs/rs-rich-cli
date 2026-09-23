//! Diffs: an engine, terminal views and the inputs they read.
//!
//! - [`engine`]: Myers' O(ND) diff in linear space over any hashable
//!   elements ([`diff_lines`], [`diff_slices`]), word and character diffs for
//!   intra-line emphasis ([`diff_words`], [`diff_chars`]), [`Hunk`] grouping
//!   and [`TextDiff`], whose [`unified`](TextDiff::unified) output matches
//!   `diff -u` minus timestamps.
//! - [`DiffView`]: a renderable diff, unified or side by side, of plain text,
//!   ANSI text ([`DiffView::ansi`], which also reports style-only changes) or,
//!   with the `testing` feature, render snapshots.
//! - [`SourceDiff`]: a syntax-highlighted source diff with optional line links.
//! - [`git`]: a `git diff` parser and [`PatchView`](git::PatchView), a
//!   review-style renderer with a file tree, annotations and links.
//! - `test_report` (feature `test-report`): JUnit XML and libtest JSON test
//!   results and the `TestReport` renderable.
//! - `assert_rich_eq!` and friends (feature `testing`): assertions that
//!   panic with a rendered diff.
//!
//! With colour off every change stays visible: `-`, `+` and `~` (style only)
//! markers lead each changed line.

pub mod engine;
pub mod git;
mod render;
mod source;
mod view;

#[cfg(feature = "testing")]
pub mod assert;
#[cfg(feature = "test-report")]
pub mod test_report;

pub use engine::{
    diff_chars, diff_lines, diff_slices, diff_words, group_hunks, hunk_header, tokenize, Hunk, Op,
    TextDiff,
};
pub use source::SourceDiff;
pub use view::{DiffView, Side};

use rich::{Console, Style};

/// The default styles for diff and test-report keys. [`extended_theme`]
/// includes them; renderers fall back to them when a theme lacks a key.
///
/// [`extended_theme`]: crate::theme::extended_theme
pub const STYLES: &[(&str, &str)] = &[
    ("diff.added", "green"),
    ("diff.removed", "red"),
    ("diff.added.emphasis", "bold underline green"),
    ("diff.removed.emphasis", "bold underline red"),
    ("diff.hunk", "cyan"),
    ("diff.header", "bold"),
    ("diff.line_number", "dim"),
    ("diff.context", "none"),
    ("test.passed", "green"),
    ("test.failed", "bold red"),
    ("test.errored", "bold magenta"),
    ("test.skipped", "yellow"),
];

/// The console theme's style for `key`, else its default from [`STYLES`].
pub(crate) fn style(console: &Console, key: &str) -> Style {
    let fallback = STYLES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or("none", |(_, spec)| spec);
    crate::event::theme_style(console, key, fallback)
}

/// How a diff is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    /// One column, removed lines before added ones (the default).
    #[default]
    Unified,
    /// Old on the left, new on the right, each half the width.
    SideBySide,
}

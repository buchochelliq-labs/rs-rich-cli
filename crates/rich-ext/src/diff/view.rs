//! [`DiffView`]: a renderable line diff.

use std::ops::Range;
use std::sync::Arc;

use rich::measure::Measurement;
use rich::{AnsiDecoder, Console, ConsoleOptions, Renderable, Segment, Style, Text, Theme};

use super::engine::{
    diff_lines, group_ranges, hunk_header, split_lines_inclusive, strip_eol, Op, TextDiff,
};
use super::render::{self, Block, Kind, Options, Row};
use super::{style, Layout};

/// Which side of a diff.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

/// Builds the URL a line number links to.
pub(crate) type LineLinks = Arc<dyn Fn(Side, usize) -> Option<String> + Send + Sync>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tag {
    Equal,
    Delete,
    Insert,
    /// Same text, different styling.
    Style,
}

/// A line diff as a renderable: unified (default) or side by side, with line
/// numbers, hunk headers and word-level emphasis on changed lines.
///
/// ```
/// use rich::Console;
/// use rich_ext::diff::DiffView;
///
/// let console = Console::builder().width(40).no_color(true).build();
/// let out = console.render_to_string(&DiffView::new("a\nb\n", "a\nc\n").line_numbers(false));
/// assert_eq!(out, "@@ -1,2 +1,2 @@\n  a\n- b\n+ c");
/// ```
#[derive(Clone)]
pub struct DiffView {
    old: Vec<Text>,
    new: Vec<Text>,
    chunks: Vec<(Tag, Range<usize>, Range<usize>)>,
    old_no_newline: bool,
    new_no_newline: bool,
    layout: Layout,
    line_numbers: bool,
    wrap: bool,
    context: usize,
    titles: Option<(String, String)>,
    emphasis: bool,
    links: Option<LineLinks>,
}

impl std::fmt::Debug for DiffView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiffView")
            .field("old_lines", &self.old.len())
            .field("new_lines", &self.new.len())
            .field("layout", &self.layout)
            .field("context", &self.context)
            .finish_non_exhaustive()
    }
}

/// Lines of `text` without terminators; a trailing newline adds no line.
pub(crate) fn text_lines(text: &Text) -> Vec<Text> {
    let plain = text.plain();
    let offsets: Vec<usize> = plain
        .match_indices('\n')
        .map(|(i, _)| i + 1)
        .filter(|&i| i < plain.len())
        .collect();
    if plain.is_empty() {
        return Vec::new();
    }
    text.divide(&offsets)
        .into_iter()
        .map(|mut line| {
            if line.plain().ends_with('\n') {
                line.right_crop(1);
            }
            if line.plain().ends_with('\r') {
                line.right_crop(1);
            }
            line
        })
        .collect()
}

/// The visible styling of a line, independent of how its spans are split.
fn signature(text: &Text) -> Vec<Segment> {
    let segments = text.render(&Theme::default_theme(), &Style::new());
    Segment::simplify(
        &segments
            .into_iter()
            .filter(|s| !s.text.is_empty())
            .collect::<Vec<_>>(),
    )
}

impl DiffView {
    /// Diff two plain texts.
    pub fn new(old: &str, new: &str) -> Self {
        let plain = |s: &str| {
            split_lines_inclusive(s)
                .into_iter()
                .map(|l| Text::new(strip_eol(l)))
                .collect()
        };
        Self::from_parts(
            plain(old),
            plain(new),
            &split_lines_inclusive(old),
            &split_lines_inclusive(new),
            false,
        )
    }

    /// View an existing [`TextDiff`], keeping its context setting.
    pub fn from_diff(diff: &TextDiff) -> Self {
        let lines = |ls: &[String]| ls.iter().map(|l| Text::new(strip_eol(l))).collect();
        let old: Vec<&str> = diff.old_lines().iter().map(String::as_str).collect();
        let new: Vec<&str> = diff.new_lines().iter().map(String::as_str).collect();
        let mut view = Self::from_ops(
            lines(diff.old_lines()),
            lines(diff.new_lines()),
            diff.ops(),
            &old,
            &new,
        );
        view.context = diff.context_lines();
        view
    }

    /// Diff two ANSI-styled texts. Lines are compared by their visible text;
    /// lines with the same text but different styling are reported as
    /// style-only changes (`~`). Each side keeps its own styling.
    pub fn ansi(old: &str, new: &str) -> Self {
        let decode = |s: &str| {
            let mut decoder = AnsiDecoder::new();
            let lines = split_lines_inclusive(s);
            let texts: Vec<Text> = lines
                .iter()
                .map(|l| decoder.decode_line(strip_eol(l)))
                .collect();
            // Compare visible text, keeping each line's terminator.
            let keys: Vec<String> = lines
                .iter()
                .zip(&texts)
                .map(|(l, t)| {
                    let mut key = t.plain().to_string();
                    if l.ends_with('\n') {
                        key.push('\n');
                    }
                    key
                })
                .collect();
            (texts, keys)
        };
        let (old_texts, old_keys) = decode(old);
        let (new_texts, new_keys) = decode(new);
        let ok: Vec<&str> = old_keys.iter().map(String::as_str).collect();
        let nk: Vec<&str> = new_keys.iter().map(String::as_str).collect();
        Self::from_parts(old_texts, new_texts, &ok, &nk, true)
    }

    /// Diff two render snapshots through their ANSI output, so styling
    /// regressions show as `~` lines.
    #[cfg(feature = "testing")]
    pub fn snapshots(
        old: &crate::testing::RenderSnapshot,
        new: &crate::testing::RenderSnapshot,
    ) -> Self {
        Self::ansi(&old.ansi, &new.ansi)
    }

    /// Diff styled lines by comparison keys (lines with terminators).
    pub(crate) fn from_parts(
        old: Vec<Text>,
        new: Vec<Text>,
        old_keys: &[&str],
        new_keys: &[&str],
        styles: bool,
    ) -> Self {
        let ops = diff_lines(old_keys, new_keys);
        let mut view = Self::from_ops(old, new, &ops, old_keys, new_keys);
        if styles {
            view.detect_style_changes();
        }
        view
    }

    fn from_ops(
        old: Vec<Text>,
        new: Vec<Text>,
        ops: &[Op],
        old_keys: &[&str],
        new_keys: &[&str],
    ) -> Self {
        let chunks = ops
            .iter()
            .map(|op| {
                let tag = match op {
                    Op::Equal { .. } => Tag::Equal,
                    Op::Delete { .. } => Tag::Delete,
                    Op::Insert { .. } => Tag::Insert,
                };
                (tag, op.old(), op.new_range())
            })
            .collect();
        let lacks = |keys: &[&str]| keys.last().is_some_and(|l| !l.ends_with('\n'));
        DiffView {
            old,
            new,
            chunks,
            old_no_newline: lacks(old_keys),
            new_no_newline: lacks(new_keys),
            layout: Layout::Unified,
            line_numbers: true,
            wrap: true,
            context: 3,
            titles: None,
            emphasis: true,
            links: None,
        }
    }

    /// Split equal runs where the styling differs.
    fn detect_style_changes(&mut self) {
        let mut chunks = Vec::new();
        for (tag, old, new) in std::mem::take(&mut self.chunks) {
            if tag != Tag::Equal {
                chunks.push((tag, old, new));
                continue;
            }
            for (i, j) in old.clone().zip(new.clone()) {
                let t = if signature(&self.old[i]) == signature(&self.new[j]) {
                    Tag::Equal
                } else {
                    Tag::Style
                };
                match chunks.last_mut() {
                    Some((last, a, b)) if *last == t && a.end == i && b.end == j => {
                        a.end = i + 1;
                        b.end = j + 1;
                    }
                    _ => chunks.push((t, i..i + 1, j..j + 1)),
                }
            }
        }
        self.chunks = chunks;
    }

    /// Unified (default) or side by side.
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }
    /// Show line numbers (default on).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }
    /// Wrap long lines (default) or truncate them with an ellipsis.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
    /// Unchanged lines around each change (default 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context = lines;
        self
    }
    /// Titles for the old and new sides (`---`/`+++` when unified, column
    /// headings side by side).
    pub fn titles(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.titles = Some((old.into(), new.into()));
        self
    }
    /// Word-level emphasis on changed lines (default on).
    pub fn emphasis(mut self, on: bool) -> Self {
        self.emphasis = on;
        self
    }
    pub(crate) fn links(mut self, links: Option<LineLinks>) -> Self {
        self.links = links;
        self
    }

    /// Whether the two sides render identically.
    pub fn is_equal(&self) -> bool {
        self.chunks.iter().all(|c| c.0 == Tag::Equal)
    }

    /// `(added, removed, restyled)` line counts.
    pub fn stats(&self) -> (usize, usize, usize) {
        self.chunks
            .iter()
            .fold((0, 0, 0), |(a, r, s), (tag, o, n)| match tag {
                Tag::Insert => (a + n.len(), r, s),
                Tag::Delete => (a, r + o.len(), s),
                Tag::Style => (a, r, s + o.len()),
                Tag::Equal => (a, r, s),
            })
    }

    /// The 1-based new-side lines whose text matches but styling differs.
    pub fn style_changed_lines(&self) -> Vec<usize> {
        self.chunks
            .iter()
            .filter(|c| c.0 == Tag::Style)
            .flat_map(|c| c.2.clone().map(|j| j + 1))
            .collect()
    }

    pub(crate) fn blocks(&self) -> Vec<Block> {
        let runs: Vec<(bool, Range<usize>, Range<usize>)> = self
            .chunks
            .iter()
            .map(|(t, o, n)| (*t == Tag::Equal, o.clone(), n.clone()))
            .collect();
        let link = |side: Side, n: usize| self.links.as_ref().and_then(|f| f(side, n));
        let mut blocks = Vec::new();
        for group in group_ranges(&runs, self.context) {
            let old_range =
                group.first().map_or(0, |g| g.1.start)..group.last().map_or(0, |g| g.1.end);
            let new_range =
                group.first().map_or(0, |g| g.2.start)..group.last().map_or(0, |g| g.2.end);
            let mut block = Block {
                header: Some(hunk_header(old_range, new_range)),
                rows: Vec::new(),
            };
            for (index, old, new) in group {
                let old_row = |i: usize, kind: Kind| {
                    let mut row = Row::new(kind, Some(i + 1), None, self.old[i].clone());
                    row.no_newline = self.old_no_newline && i + 1 == self.old.len();
                    row.link = link(Side::Old, i + 1);
                    row
                };
                let new_row = |j: usize, kind: Kind| {
                    let mut row = Row::new(kind, None, Some(j + 1), self.new[j].clone());
                    row.no_newline = self.new_no_newline && j + 1 == self.new.len();
                    row.link = link(Side::New, j + 1);
                    row
                };
                match self.chunks[index].0 {
                    Tag::Equal => {
                        for (i, j) in old.zip(new) {
                            let mut row = new_row(j, Kind::Context);
                            row.old_no = Some(i + 1);
                            block.rows.push(row);
                        }
                    }
                    Tag::Delete => block.rows.extend(old.map(|i| old_row(i, Kind::Delete))),
                    Tag::Insert => block.rows.extend(new.map(|j| new_row(j, Kind::Insert))),
                    Tag::Style => {
                        for (i, j) in old.zip(new) {
                            block.rows.push(old_row(i, Kind::StyleOld));
                            block.rows.push(new_row(j, Kind::StyleNew));
                        }
                    }
                }
            }
            if self.emphasis {
                render::emphasize(&mut block);
            }
            blocks.push(block);
        }
        blocks
    }

    fn options(&self) -> Options {
        Options {
            layout: self.layout,
            line_numbers: self.line_numbers,
            wrap: self.wrap,
            titles: self.titles.clone(),
        }
    }
}

impl Renderable for DiffView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if options.max_width == 0 || options.height == Some(0) {
            return Vec::new();
        }
        let blocks = self.blocks();
        if blocks.is_empty() {
            let rows = render::banner(
                "no differences",
                style(console, "diff.line_number"),
                options.max_width,
            );
            return render::join(rows, options.height);
        }
        let rows = render::render(console, &blocks, &self.options(), options.max_width);
        render::join(rows, options.height)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let blocks = self.blocks();
        let (min, max) = if blocks.is_empty() {
            (1, "no differences".len())
        } else {
            render::measure(&blocks, &self.options())
        };
        let max = max.min(options.max_width);
        Measurement::new(min.min(max), max)
    }
}

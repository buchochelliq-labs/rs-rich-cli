//! The row model and layout shared by [`DiffView`](super::DiffView),
//! [`SourceDiff`](super::SourceDiff) and [`PatchView`](super::git::PatchView).

use std::ops::Range;

use rich::cells::cell_len;
use rich::{Console, Justify, Overflow, Segment, Style, Text};

use super::engine::diff_words;
use super::{style, Layout};
use crate::diagnostic::Level;

/// What a displayed line is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Context,
    Delete,
    Insert,
    /// Same text as its partner, different styling: the old side.
    StyleOld,
    /// The new side of a style-only change.
    StyleNew,
}

impl Kind {
    fn marker(self) -> char {
        match self {
            Kind::Context => ' ',
            Kind::Delete => '-',
            Kind::Insert => '+',
            Kind::StyleOld | Kind::StyleNew => '~',
        }
    }
    fn style_key(self) -> &'static str {
        match self {
            Kind::Context | Kind::StyleOld | Kind::StyleNew => "diff.context",
            Kind::Delete => "diff.removed",
            Kind::Insert => "diff.added",
        }
    }
    fn marker_key(self) -> &'static str {
        match self {
            Kind::StyleOld | Kind::StyleNew => "diff.header",
            other => other.style_key(),
        }
    }
}

/// One source line as displayed.
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub kind: Kind,
    pub old_no: Option<usize>,
    pub new_no: Option<usize>,
    pub text: Text,
    /// Byte ranges of `text` to emphasise.
    pub emphasis: Vec<Range<usize>>,
    pub no_newline: bool,
    /// Where the line number links to.
    pub link: Option<String>,
    pub notes: Vec<(Level, String)>,
}

impl Row {
    pub fn new(kind: Kind, old_no: Option<usize>, new_no: Option<usize>, text: Text) -> Self {
        Row {
            kind,
            old_no,
            new_no,
            text,
            emphasis: Vec::new(),
            no_newline: false,
            link: None,
            notes: Vec::new(),
        }
    }
}

/// A hunk: its `@@` header and rows.
#[derive(Clone, Debug, Default)]
pub(crate) struct Block {
    pub header: Option<String>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug)]
pub(crate) struct Options {
    pub layout: Layout,
    pub line_numbers: bool,
    pub wrap: bool,
    pub titles: Option<(String, String)>,
}

/// Word-level emphasis on paired deletions and insertions. Pairs that share
/// too little are left whole: emphasising nearly every token says nothing.
pub(crate) fn emphasize(block: &mut Block) {
    let mut i = 0;
    let rows = &mut block.rows;
    while i < rows.len() {
        if rows[i].kind != Kind::Delete {
            i += 1;
            continue;
        }
        let d0 = i;
        while i < rows.len() && rows[i].kind == Kind::Delete {
            i += 1;
        }
        let i0 = i;
        while i < rows.len() && rows[i].kind == Kind::Insert {
            i += 1;
        }
        let pairs = (i0 - d0).min(i - i0);
        for p in 0..pairs {
            let (old, new) = (rows[d0 + p].text.plain(), rows[i0 + p].text.plain());
            let ops = diff_words(old, new);
            let same: usize = ops
                .iter()
                .filter(|op| op.is_equal())
                .map(|op| op.old().len())
                .sum();
            let total = old.len() + new.len();
            if total == 0 || same * 2 * 10 < total * 4 {
                continue;
            }
            let (mut a, mut b) = (Vec::new(), Vec::new());
            for op in ops {
                if op.is_equal() {
                    continue;
                }
                let (o, n) = (op.old(), op.new_range());
                if !o.is_empty() {
                    a.push(o);
                }
                if !n.is_empty() {
                    b.push(n);
                }
            }
            rows[d0 + p].emphasis = a;
            rows[i0 + p].emphasis = b;
        }
    }
}

fn number_width(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .flat_map(|b| &b.rows)
        .flat_map(|r| [r.old_no, r.new_no])
        .flatten()
        .max()
        .unwrap_or(0)
        .max(1)
        .to_string()
        .len()
}

/// The widest line content, in cells.
pub(crate) fn content_width(blocks: &[Block]) -> usize {
    blocks
        .iter()
        .flat_map(|b| &b.rows)
        .map(|r| {
            let mut text = r.text.clone();
            text.expand_tabs(8);
            text.cell_len()
        })
        .max()
        .unwrap_or(0)
}

/// `(minimum, maximum)` width for these blocks.
pub(crate) fn measure(blocks: &[Block], options: &Options) -> (usize, usize) {
    let digits = number_width(blocks);
    let gutter = if options.line_numbers {
        2 * digits + 4
    } else {
        2
    };
    let content = content_width(blocks);
    let headers = blocks
        .iter()
        .filter_map(|b| b.header.as_deref())
        .map(cell_len)
        .max()
        .unwrap_or(0);
    let titles = options
        .titles
        .as_ref()
        .map_or(0, |(a, b)| cell_len(a).max(cell_len(b)) + 4);
    match options.layout {
        Layout::Unified => (
            gutter + 1,
            (gutter + content).max(headers).max(titles).max(1),
        ),
        Layout::SideBySide => {
            let side = if options.line_numbers { digits + 3 } else { 2 };
            (
                2 * (side + 1) + 3,
                (2 * (side + content) + 3).max(headers).max(titles),
            )
        }
    }
}

fn pad_to(mut row: Vec<Segment>, width: usize) -> Vec<Segment> {
    let len: usize = row.iter().map(Segment::cell_length).sum();
    if len < width {
        row.push(Segment::new(" ".repeat(width - len), None));
    }
    row
}

/// Drop trailing whitespace, whatever its style.
pub(crate) fn trim_end(mut row: Vec<Segment>) -> Vec<Segment> {
    while let Some(last) = row.last_mut() {
        if last.control {
            break;
        }
        let trimmed = last.text.trim_end_matches([' ', '\t']).len();
        if trimmed == 0 {
            row.pop();
            continue;
        }
        last.text.truncate(trimmed);
        break;
    }
    Segment::simplify(&row)
}

/// A whole-width line of `text` in `style`, cropped.
pub(crate) fn banner(text: &str, style: Style, width: usize) -> Vec<Vec<Segment>> {
    let text = Text::styled(text, style);
    let lines = text.render_lines_wrapped(
        &rich::Theme::default_theme(),
        &Style::new(),
        Some(width),
        Justify::Default,
        Overflow::Ellipsis,
        true,
    );
    lines.into_iter().map(trim_end).collect()
}

/// Wrapped or truncated lines of `text` at `width`.
pub(crate) fn wrap_text(
    console: &Console,
    text: &Text,
    base: &Style,
    width: usize,
    wrap: bool,
) -> Vec<Vec<Segment>> {
    let (overflow, no_wrap) = if wrap {
        (Overflow::Fold, false)
    } else {
        (Overflow::Ellipsis, true)
    };
    let mut lines = text.render_lines_wrapped(
        console.theme(),
        base,
        Some(width.max(1)),
        Justify::Default,
        overflow,
        no_wrap,
    );
    if lines.is_empty() {
        lines.push(Vec::new());
    }
    lines
}

/// The row's text with emphasis applied, and its base style.
fn styled_content(console: &Console, row: &Row) -> (Text, Style) {
    let mut text = row.text.clone();
    let key = match row.kind {
        Kind::Delete => Some("diff.removed.emphasis"),
        Kind::Insert => Some("diff.added.emphasis"),
        _ => None,
    };
    if let Some(key) = key {
        let emphasis = style(console, key);
        for range in &row.emphasis {
            text.stylize(emphasis.clone(), range.start, range.end);
        }
    }
    (text, style(console, row.kind.style_key()))
}

struct Gutter {
    digits: usize,
    numbers: bool,
    /// Unified shows both numbers; a side-by-side column shows one.
    both: bool,
}

impl Gutter {
    fn width(&self) -> usize {
        match (self.numbers, self.both) {
            (false, _) => 2,
            (true, true) => 2 * self.digits + 4,
            (true, false) => self.digits + 3,
        }
    }

    fn render(
        &self,
        console: &Console,
        row: &Row,
        number: Option<usize>,
        first: bool,
    ) -> Vec<Segment> {
        let mut out = Vec::new();
        if self.numbers {
            let ln = style(console, "diff.line_number");
            let cell = |n: Option<usize>| match (first, n) {
                (true, Some(n)) => format!("{n:>w$}", w = self.digits),
                _ => " ".repeat(self.digits),
            };
            let numbers: Vec<Option<usize>> = if self.both {
                vec![row.old_no, row.new_no]
            } else {
                vec![number]
            };
            let linked = row.new_no.or(row.old_no);
            for n in numbers {
                let text = cell(n);
                let mut st = ln.clone();
                if first && n.is_some() && n == linked {
                    if let Some(url) = &row.link {
                        st = st.with_link(url.clone());
                    }
                }
                out.push(Segment::new(text, Some(st)));
                out.push(Segment::new(" ", None));
            }
        }
        out.push(Segment::new(
            row.kind.marker().to_string(),
            Some(style(console, row.kind.marker_key())),
        ));
        out.push(Segment::new(" ", None));
        out
    }
}

fn row_lines(
    console: &Console,
    row: &Row,
    gutter: &Gutter,
    number: Option<usize>,
    width: usize,
    wrap: bool,
) -> Vec<Vec<Segment>> {
    let (text, base) = styled_content(console, row);
    let content = width.saturating_sub(gutter.width()).max(1);
    wrap_text(console, &text, &base, content, wrap)
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut out = gutter.render(console, row, number, i == 0);
            out.extend(line);
            out
        })
        .collect()
}

fn extras(console: &Console, row: &Row, indent: usize, width: usize) -> Vec<Vec<Segment>> {
    let mut out = Vec::new();
    let pad = Segment::new(" ".repeat(indent), None);
    let avail = width.saturating_sub(indent).max(1);
    if row.no_newline {
        const MARKER: &str = "\\ No newline at end of file";
        // Pull the marker left rather than cut it, when the width allows.
        let indent = indent.min(width.saturating_sub(MARKER.len()));
        for line in banner(
            MARKER,
            style(console, "diff.line_number"),
            width.saturating_sub(indent).max(1),
        ) {
            let mut r = vec![Segment::new(" ".repeat(indent), None)];
            r.extend(line);
            out.push(r);
        }
    }
    for (level, message) in &row.notes {
        let mut text = Text::new("");
        text.append(
            &format!("{}: ", level.name()),
            Some(level.style(console).into()),
        );
        text.append(message, None);
        for line in wrap_text(console, &text, &Style::new(), avail, true) {
            let mut r = vec![pad.clone()];
            r.extend(line);
            out.push(r);
        }
    }
    out
}

fn titles_rows(
    console: &Console,
    options: &Options,
    width: usize,
    columns: Option<(usize, usize, &str)>,
) -> Vec<Vec<Segment>> {
    let Some((old, new)) = &options.titles else {
        return Vec::new();
    };
    let header = style(console, "diff.header");
    match columns {
        None => {
            let mut rows = banner(&format!("--- {old}"), header.clone(), width);
            rows.extend(banner(&format!("+++ {new}"), header, width));
            rows
        }
        Some((left, right, divider)) => {
            let l = banner(old, header.clone(), left)
                .into_iter()
                .next()
                .unwrap_or_default();
            let r = banner(new, header, right)
                .into_iter()
                .next()
                .unwrap_or_default();
            let mut row = pad_to(l, left);
            row.push(Segment::new(
                divider,
                Some(style(console, "diff.line_number")),
            ));
            row.extend(r);
            vec![trim_end(row)]
        }
    }
}

/// Lay the blocks out at `width`.
pub(crate) fn render(
    console: &Console,
    blocks: &[Block],
    options: &Options,
    width: usize,
) -> Vec<Vec<Segment>> {
    if width == 0 {
        return Vec::new();
    }
    let digits = number_width(blocks);
    let hunk = style(console, "diff.hunk");
    let mut out = Vec::new();
    match options.layout {
        Layout::Unified => {
            let mut gutter = Gutter {
                digits,
                numbers: options.line_numbers,
                both: true,
            };
            if gutter.width() + 4 > width {
                gutter.numbers = false;
            }
            out.extend(titles_rows(console, options, width, None));
            for block in blocks {
                if let Some(header) = &block.header {
                    out.extend(banner(header, hunk.clone(), width));
                }
                for row in &block.rows {
                    out.extend(
                        row_lines(console, row, &gutter, None, width, options.wrap)
                            .into_iter()
                            .map(trim_end),
                    );
                    out.extend(
                        extras(console, row, gutter.width(), width)
                            .into_iter()
                            .map(trim_end),
                    );
                }
            }
        }
        Layout::SideBySide => {
            let divider = if console.ascii_only() { " | " } else { " │ " };
            let inner = width.saturating_sub(3);
            let left = inner / 2;
            let right = inner - left;
            let mut gutter = Gutter {
                digits,
                numbers: options.line_numbers,
                both: false,
            };
            if gutter.width() + 2 > right {
                gutter.numbers = false;
            }
            let div_style = style(console, "diff.line_number");
            out.extend(titles_rows(
                console,
                options,
                width,
                Some((left, right, divider)),
            ));
            for block in blocks {
                if let Some(header) = &block.header {
                    out.extend(banner(header, hunk.clone(), width));
                }
                for (l, r) in pair_rows(&block.rows) {
                    let lines_l = l.map_or_else(Vec::new, |row| {
                        row_lines(console, row, &gutter, row.old_no, left, options.wrap)
                    });
                    let lines_r = r.map_or_else(Vec::new, |row| {
                        row_lines(console, row, &gutter, row.new_no, right, options.wrap)
                    });
                    for i in 0..lines_l.len().max(lines_r.len()) {
                        let mut row = pad_to(lines_l.get(i).cloned().unwrap_or_default(), left);
                        row.push(Segment::new(divider, Some(div_style.clone())));
                        row.extend(lines_r.get(i).cloned().unwrap_or_default());
                        out.push(trim_end(row));
                    }
                    // A context row is on both sides; its extras show once.
                    let shared = matches!((l, r), (Some(a), Some(b)) if std::ptr::eq(a, b));
                    if let Some(row) = l.filter(|_| !shared) {
                        out.extend(
                            extras(console, row, gutter.width(), width)
                                .into_iter()
                                .map(trim_end),
                        );
                    }
                    if let Some(row) = r {
                        out.extend(
                            extras(console, row, left + 3 + gutter.width(), width)
                                .into_iter()
                                .map(trim_end),
                        );
                    }
                }
            }
        }
    }
    out
}

/// Pair rows for side-by-side: context with itself, deletions with the
/// insertions that follow them, and style-only halves together.
fn pair_rows(rows: &[Row]) -> Vec<(Option<&Row>, Option<&Row>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        match rows[i].kind {
            Kind::Context => {
                out.push((Some(&rows[i]), Some(&rows[i])));
                i += 1;
            }
            Kind::StyleOld => {
                let partner = rows.get(i + 1).filter(|r| r.kind == Kind::StyleNew);
                out.push((Some(&rows[i]), partner));
                i += 1 + usize::from(partner.is_some());
            }
            Kind::StyleNew => {
                out.push((None, Some(&rows[i])));
                i += 1;
            }
            Kind::Delete | Kind::Insert => {
                let d0 = i;
                while i < rows.len() && rows[i].kind == Kind::Delete {
                    i += 1;
                }
                let i0 = i;
                while i < rows.len() && rows[i].kind == Kind::Insert {
                    i += 1;
                }
                let (dels, ins) = (&rows[d0..i0], &rows[i0..i]);
                for k in 0..dels.len().max(ins.len()) {
                    out.push((dels.get(k), ins.get(k)));
                }
            }
        }
    }
    out
}

/// Rows flattened into one segment stream, newline-separated.
pub(crate) fn join(mut rows: Vec<Vec<Segment>>, height: Option<usize>) -> Vec<Segment> {
    if let Some(height) = height {
        rows.truncate(height);
    }
    crate::event::flatten(rows)
}

//! Inspect text one grapheme cluster at a time.
//!
//! [`UnicodeView`] splits text into grapheme clusters with the core's
//! [`split_graphemes`](rich::cells::split_graphemes) and shows one row per
//! cluster: its byte offset, the cluster itself, its code points, its UTF-8
//! bytes, its cell width, an escape form and a short [`Kind`] label from a
//! small hand-written classifier. Controls are always rows of their own and
//! display as Unicode control pictures (`␊`), or `^J` on ASCII-only consoles.
//! A summary line follows the table. Below [`FULL_LAYOUT_WIDTH`] columns (or
//! whenever the seven-column table would not fit) the code points, bytes,
//! width, escape and kind stack in one `Details` column, so the table still
//! fits at 40 columns.
//!
//! [`UnicodeView::from_bytes`] decodes UTF-8 itself: every invalid sequence
//! becomes an `invalid` row holding the offending bytes, and decoding resumes
//! after it, splitting the input exactly as `String::from_utf8_lossy` does.
//!
//! There is no character-name database; the kind label is all the naming.
//!
//! ```
//! use rich_ext::unicode_inspect::{Kind, UnicodeView};
//!
//! let view = UnicodeView::from_str("e\u{301}!");
//! let clusters = view.clusters();
//! assert_eq!(clusters.len(), 2);
//! assert_eq!(clusters[0].code_points(), "U+0065 U+0301");
//! assert_eq!(clusters[0].width, 1);
//! assert_eq!(clusters[0].kind, Kind::Combining);
//!
//! let bad = UnicodeView::from_bytes(&[0x66, 0xff, 0x6f]);
//! assert_eq!(bad.clusters()[1].kind, Kind::Invalid);
//! assert_eq!(bad.summary().invalid, 1);
//! ```
//!
//! Styles come from the theme keys `unicode.offset`, `unicode.kind`,
//! `unicode.error` and `unicode.summary`, with built-in fallbacks.

use rich::cells::{cell_len, split_graphemes};
use rich::table::ColumnOptions;
use rich::{Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Table, Text};

use crate::event::theme_style;

/// The label of a cluster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A C0 or C1 control, or DEL.
    Control,
    /// Whitespace that is not a control (space, NBSP, the U+2000 spaces…).
    Whitespace,
    /// A cluster carrying a combining mark.
    Combining,
    /// ZWJ, ZWNJ, U+200B, U+2060 or U+FEFF outside an emoji sequence.
    ZeroWidth,
    /// A variation selector outside an emoji sequence.
    VariationSelector,
    /// Contains a code point from the main emoji blocks.
    Emoji,
    /// Two cells wide.
    Wide,
    /// Plain ASCII.
    Ascii,
    /// Anything else.
    Other,
    /// Bytes that are not valid UTF-8.
    Invalid,
}

impl Kind {
    /// The label shown in the table.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Control => "control",
            Kind::Whitespace => "whitespace",
            Kind::Combining => "combining",
            Kind::ZeroWidth => "zero-width",
            Kind::VariationSelector => "variation selector",
            Kind::Emoji => "emoji",
            Kind::Wide => "wide",
            Kind::Ascii => "ascii",
            Kind::Other => "other",
            Kind::Invalid => "invalid",
        }
    }
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

fn is_control(c: char) -> bool {
    matches!(c as u32, 0..=0x1f | 0x7f..=0x9f)
}

fn is_combining(c: char) -> bool {
    matches!(
        c as u32,
        0x0300..=0x036f | 0x1ab0..=0x1aff | 0x1dc0..=0x1dff | 0x20d0..=0x20ff | 0xfe20..=0xfe2f
    )
}

fn is_zero_width(c: char) -> bool {
    matches!(c as u32, 0x200b..=0x200d | 0x2060 | 0xfeff)
}

fn is_variation_selector(c: char) -> bool {
    matches!(c as u32, 0xfe00..=0xfe0f | 0xe0100..=0xe01ef)
}

fn is_emoji(c: char) -> bool {
    matches!(
        c as u32,
        0x1f000..=0x1f2ff // mahjong, dominoes, cards, enclosed alphanumerics
            | 0x1f300..=0x1f5ff // symbols and pictographs
            | 0x1f600..=0x1f64f // emoticons
            | 0x1f680..=0x1f6ff // transport and map
            | 0x1f900..=0x1f9ff // supplemental symbols and pictographs
            | 0x1fa70..=0x1faff // symbols and pictographs extended-A
            | 0x2600..=0x26ff // miscellaneous symbols
            | 0x2700..=0x27bf // dingbats
    )
}

/// Classify a cluster of valid text with its cell width.
pub fn classify(cluster: &str, width: usize) -> Kind {
    let mut chars = cluster.chars();
    let Some(first) = chars.next() else {
        return Kind::Other;
    };
    let single = chars.next().is_none();
    if single && is_control(first) {
        Kind::Control
    } else if cluster.chars().any(is_emoji) {
        Kind::Emoji
    } else if cluster.chars().any(is_combining) {
        Kind::Combining
    } else if cluster.chars().any(is_variation_selector) {
        Kind::VariationSelector
    } else if cluster.chars().any(is_zero_width) {
        Kind::ZeroWidth
    } else if single && first.is_whitespace() {
        Kind::Whitespace
    } else if width == 2 {
        Kind::Wide
    } else if cluster.is_ascii() {
        Kind::Ascii
    } else {
        Kind::Other
    }
}

/// How a control character is shown: its control picture (`␛`, `␡`), or
/// `^[` / `^?` when `ascii`. C1 controls have no picture and show as `\u{9b}`.
/// `None` for anything that is not a control.
pub fn control_picture(c: char, ascii: bool) -> Option<String> {
    let n = c as u32;
    match n {
        0..=0x1f if ascii => Some(format!("^{}", char::from_u32(n + 0x40).expect("ASCII"))),
        0..=0x1f => Some(char::from_u32(0x2400 + n).expect("picture").to_string()),
        0x7f if ascii => Some("^?".into()),
        0x7f => Some("␡".into()),
        0x80..=0x9f => Some(format!("\\u{{{n:x}}}")),
        _ => None,
    }
}

/// One grapheme cluster, or one invalid UTF-8 sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cluster {
    /// Byte offset in the input.
    pub offset: usize,
    /// The input bytes it covers.
    pub bytes: Vec<u8>,
    /// The text, or `None` for an invalid sequence.
    pub text: Option<String>,
    /// Terminal cells (0 for an invalid sequence).
    pub width: usize,
    /// Its label.
    pub kind: Kind,
}

impl Cluster {
    /// `U+1F468 U+200D …`, or `-` for an invalid sequence.
    pub fn code_points(&self) -> String {
        match &self.text {
            Some(text) => text
                .chars()
                .map(|c| format!("U+{:04X}", c as u32))
                .collect::<Vec<_>>()
                .join(" "),
            None => "-".into(),
        }
    }

    /// The bytes in lowercase hex, space separated.
    pub fn hex(&self) -> String {
        self.bytes
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A Rust escape form: `\u{…}` for non-ASCII and controls, `\\` for a
    /// backslash, the character otherwise. Invalid bytes show as `\xNN`.
    pub fn escape(&self) -> String {
        let Some(text) = &self.text else {
            return self.bytes.iter().map(|b| format!("\\x{b:02x}")).collect();
        };
        let mut out = String::new();
        for c in text.chars() {
            if c == '\\' {
                out.push_str("\\\\");
            } else if c.is_ascii() && !is_control(c) {
                out.push(c);
            } else {
                out.push_str(&format!("\\u{{{:x}}}", c as u32));
            }
        }
        out
    }

    /// The cluster as the table shows it. Controls become pictures, a
    /// leading combining mark sits on `◌`, lone zero-width characters show
    /// as nothing. With `ascii`, non-ASCII clusters show as `.` and invalid
    /// ones as `?`.
    pub fn display(&self, ascii: bool) -> String {
        let Some(text) = &self.text else {
            return if ascii { "?" } else { "\u{fffd}" }.into();
        };
        let mut chars = text.chars();
        let first = chars.next().unwrap_or(' ');
        if self.kind == Kind::Control {
            return control_picture(first, ascii).unwrap_or_default();
        }
        if ascii {
            return if text.is_ascii() {
                text.clone()
            } else {
                ".".into()
            };
        }
        if self.width == 0 && is_combining(first) {
            return format!("◌{text}");
        }
        if self.width == 0 {
            return String::new();
        }
        text.clone()
    }
}

/// Totals over the whole input.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Summary {
    pub bytes: usize,
    pub code_points: usize,
    pub graphemes: usize,
    pub cells: usize,
    pub invalid: usize,
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let plural = |n: usize, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        write!(
            f,
            "{}, {}, {}, {}, {}",
            plural(self.bytes, "byte"),
            plural(self.code_points, "code point"),
            plural(self.graphemes, "grapheme"),
            plural(self.cells, "cell"),
            plural(self.invalid, "invalid sequence"),
        )
    }
}

/// Split valid text into clusters, controls on their own.
fn push_clusters(out: &mut Vec<Cluster>, text: &str, base: usize) {
    let (spans, _) = split_graphemes(text);
    for (start, end, _) in spans {
        let span = &text[start..end];
        // The core attaches zero-width characters, controls included, to the
        // grapheme before them; an inspector wants each control on its own.
        let mut piece_start = start;
        for (i, c) in span.char_indices() {
            if !is_control(c) {
                continue;
            }
            let at = start + i;
            if at > piece_start {
                out.push(cluster(text, piece_start, at, base));
            }
            out.push(cluster(text, at, at + c.len_utf8(), base));
            piece_start = at + c.len_utf8();
        }
        if piece_start < end {
            out.push(cluster(text, piece_start, end, base));
        }
    }
}

fn cluster(text: &str, start: usize, end: usize, base: usize) -> Cluster {
    let s = &text[start..end];
    let width = cell_len(s);
    Cluster {
        offset: base + start,
        bytes: s.as_bytes().to_vec(),
        text: Some(s.to_owned()),
        width,
        kind: classify(s, width),
    }
}

/// A renderable table of the grapheme clusters of some text.
#[derive(Clone, Debug)]
pub struct UnicodeView {
    clusters: Vec<Cluster>,
    summary: Summary,
    limit: Option<usize>,
}

impl UnicodeView {
    /// Inspect `text`.
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(text: &str) -> Self {
        let mut clusters = Vec::new();
        push_clusters(&mut clusters, text, 0);
        Self::build(clusters, text.len())
    }

    /// Inspect raw bytes, decoding UTF-8 and reporting each invalid sequence.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let mut clusters = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            let rest = &bytes[at..];
            match std::str::from_utf8(rest) {
                Ok(text) => {
                    push_clusters(&mut clusters, text, at);
                    break;
                }
                Err(e) => {
                    let valid = e.valid_up_to();
                    let text = std::str::from_utf8(&rest[..valid]).expect("valid prefix");
                    push_clusters(&mut clusters, text, at);
                    let bad = e.error_len().unwrap_or(rest.len() - valid);
                    clusters.push(Cluster {
                        offset: at + valid,
                        bytes: rest[valid..valid + bad].to_vec(),
                        text: None,
                        width: 0,
                        kind: Kind::Invalid,
                    });
                    at += valid + bad;
                }
            }
        }
        Self::build(clusters, bytes.len())
    }

    fn build(clusters: Vec<Cluster>, bytes: usize) -> Self {
        let mut summary = Summary {
            bytes,
            ..Summary::default()
        };
        for c in &clusters {
            match &c.text {
                Some(text) => {
                    summary.code_points += text.chars().count();
                    summary.graphemes += 1;
                    summary.cells += c.width;
                }
                None => summary.invalid += 1,
            }
        }
        UnicodeView {
            clusters,
            summary,
            limit: None,
        }
    }

    /// Show at most `n` rows, then `… N more`.
    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    /// Every cluster, in order (the limit does not apply).
    pub fn clusters(&self) -> &[Cluster] {
        &self.clusters
    }

    /// Totals over the whole input.
    pub fn summary(&self) -> Summary {
        self.summary
    }
}

/// Below this width, or when the full table would not fit, [`UnicodeView`]
/// stacks the details of each cluster in one column instead of five.
pub const FULL_LAYOUT_WIDTH: usize = 80;

impl UnicodeView {
    fn table(&self, console: &Console, compact: bool) -> Table {
        let ascii = console.ascii_only();
        let offset_style = theme_style(console, "unicode.offset", "cyan");
        let kind_style = theme_style(console, "unicode.kind", "magenta");
        let error_style = theme_style(console, "unicode.error", "bold red");

        let mut table = Table::new();
        let fold = |justify: Justify| ColumnOptions {
            justify,
            overflow: Overflow::Fold,
            ..ColumnOptions::default()
        };
        // Caps keep long clusters from starving the narrow columns: two code
        // points, four bytes or one supplementary escape per line.
        let headers: &[(&str, Justify, Option<usize>)] = if compact {
            &[
                ("Offset", Justify::Right, None),
                ("Char", Justify::Left, None),
                ("Details", Justify::Left, None),
            ]
        } else {
            &[
                ("Offset", Justify::Right, None),
                ("Char", Justify::Left, None),
                ("Code points", Justify::Left, Some(14)),
                ("UTF-8", Justify::Left, Some(11)),
                ("Width", Justify::Right, None),
                ("Escape", Justify::Left, Some(12)),
                ("Kind", Justify::Left, None),
            ]
        };
        for (header, justify, max_width) in headers {
            let mut options = fold(*justify);
            options.max_width = *max_width;
            table.add_column_with(Text::new(*header), options);
        }
        for c in &self.clusters[..self.shown()] {
            let invalid = c.kind == Kind::Invalid;
            let cell = |s: String| {
                if invalid {
                    Text::styled(s, error_style.clone())
                } else {
                    Text::new(s)
                }
            };
            let kind = Text::styled(
                c.kind.label(),
                if invalid {
                    error_style.clone()
                } else {
                    kind_style.clone()
                },
            );
            let width = if invalid {
                "-".into()
            } else {
                c.width.to_string()
            };
            let offset = Text::styled(c.offset.to_string(), offset_style.clone());
            if compact {
                // One cell of details: kind and width, code points, bytes, escape.
                let mut details = kind;
                let cells = if invalid {
                    String::new()
                } else {
                    format!(", {width} cell{}", if c.width == 1 { "" } else { "s" })
                };
                details.append(&cells, None);
                if !invalid {
                    details.append(&format!("\n{}", c.code_points()), None);
                }
                details.append(&format!("\n{}\n{}", c.hex(), c.escape()), None);
                table.add_row_text(vec![offset, cell(c.display(ascii)), details]);
            } else {
                table.add_row_text(vec![
                    offset,
                    cell(c.display(ascii)),
                    cell(c.code_points()),
                    cell(c.hex()),
                    cell(width),
                    cell(c.escape()),
                    kind,
                ]);
            }
        }
        table
    }

    fn shown(&self) -> usize {
        self.limit.unwrap_or(usize::MAX).min(self.clusters.len())
    }
}

impl Renderable for UnicodeView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let ascii = console.ascii_only();
        let summary_style = theme_style(console, "unicode.summary", "dim");
        let full = (options.max_width >= FULL_LAYOUT_WIDTH)
            .then(|| self.table(console, false).rich_render(console, options))
            .filter(|segments| fits(segments, options.max_width));
        let mut out =
            full.unwrap_or_else(|| self.table(console, true).rich_render(console, options));
        let shown = self.shown();
        let line = |text: Text, out: &mut Vec<Segment>| {
            if out.last().is_some_and(|s| !s.text.ends_with('\n')) {
                out.push(Segment::line());
            }
            out.extend(text.rich_render(console, options));
        };
        let hidden = self.clusters.len() - shown;
        if hidden > 0 {
            let ellipsis = if ascii { "..." } else { "…" };
            line(
                Text::styled(format!("{ellipsis} {hidden} more"), summary_style.clone()),
                &mut out,
            );
        }
        line(
            Text::styled(self.summary.to_string(), summary_style),
            &mut out,
        );
        // The console ends the output with a newline; don't add a blank line.
        while out.last().is_some_and(|s| s.text == "\n") {
            out.pop();
        }
        out
    }
}

/// Whether every line of `segments` fits in `width` cells.
fn fits(segments: &[Segment], width: usize) -> bool {
    let mut line = 0;
    for segment in segments {
        let mut parts = segment.text.split('\n');
        if let Some(first) = parts.next() {
            line += cell_len(first);
        }
        for part in parts {
            if line > width {
                return false;
            }
            line = cell_len(part);
        }
    }
    line <= width
}

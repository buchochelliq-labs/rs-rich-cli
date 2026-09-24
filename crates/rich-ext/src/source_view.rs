//! A line-numbered, searchable view of a source file.
//!
//! [`SourceView`] highlights text with core's [`Syntax`] and lays it out with
//! a line-number gutter, wrapping long lines under a blank gutter so the
//! numbers stay aligned. With [`SourceView::search`], every case-insensitive
//! match is highlighted and its line number marked, and
//! [`SourceView::matches`] reports where they are.
//!
//! Upstream `Syntax` has `line_numbers` and `highlight_lines`; core's port
//! does not yet, and match highlighting is not upstream at all, so this view
//! lives here and leaves core untouched.
//!
//! ```
//! use rich::Console;
//! use rich_ext::source_view::SourceView;
//!
//! let view = SourceView::new("fn main() {\n    println!(\"hi\");\n}\n", "rust").search("println");
//! assert_eq!(view.matches(), vec![(2, 1)]);
//! let out = Console::builder().width(40).color_system(None).build().render_to_string(&view);
//! assert!(out.starts_with("1 │ fn main() {"));
//! ```

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Style, Syntax, Text};

/// Source text with line numbers and optional search highlighting.
#[derive(Clone, Debug)]
pub struct SourceView {
    code: String,
    language: String,
    theme: Option<String>,
    line_numbers: bool,
    start_line: usize,
    search: Option<String>,
    tab_size: usize,
}

impl SourceView {
    /// `code` highlighted as `language` (a lexer name or file extension;
    /// unknown languages render as plain text).
    pub fn new(code: impl Into<String>, language: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            language: language.into(),
            theme: None,
            line_numbers: true,
            start_line: 1,
            search: None,
            tab_size: 4,
        }
    }

    /// Show the line-number gutter (default on).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }

    /// The number of the first line (default 1), for an excerpt.
    pub fn start_line(mut self, line: usize) -> Self {
        self.start_line = line.max(1);
        self
    }

    /// Highlight every case-insensitive (ASCII) occurrence of `pattern`. An
    /// empty pattern highlights nothing.
    pub fn search(mut self, pattern: impl Into<String>) -> Self {
        let pattern = pattern.into();
        self.search = (!pattern.is_empty()).then_some(pattern);
        self
    }

    /// A syntect theme name, as core's `Syntax::theme` takes.
    pub fn theme(mut self, theme: impl Into<String>) -> Self {
        self.theme = Some(theme.into());
        self
    }

    /// Spaces per tab (default 4).
    pub fn tab_size(mut self, size: usize) -> Self {
        self.tab_size = size;
        self
    }

    /// The source lines, without their line endings. A final newline does not
    /// start an empty last line.
    fn source_lines(&self) -> Vec<&str> {
        let code = self.code.strip_suffix('\n').unwrap_or(&self.code);
        let code = code.strip_suffix('\r').unwrap_or(code);
        if code.is_empty() && self.code.is_empty() {
            return Vec::new();
        }
        code.split('\n')
            .map(|line| line.strip_suffix('\r').unwrap_or(line))
            .collect()
    }

    /// The byte ranges of `search` in `line`.
    fn match_ranges(&self, line: &str) -> Vec<(usize, usize)> {
        let Some(pattern) = &self.search else {
            return Vec::new();
        };
        let needle = pattern.to_ascii_lowercase();
        let haystack = line.to_ascii_lowercase();
        let mut ranges = Vec::new();
        let mut from = 0;
        while let Some(found) = haystack[from..].find(&needle) {
            let start = from + found;
            ranges.push((start, start + needle.len()));
            from = start + needle.len();
        }
        ranges
    }

    /// Every line with a match, as `(line number, matches on it)`.
    pub fn matches(&self) -> Vec<(usize, usize)> {
        self.source_lines()
            .iter()
            .enumerate()
            .filter_map(|(index, line)| {
                let count = self.match_ranges(&expand_tabs(line, self.tab_size)).len();
                (count > 0).then_some((self.start_line + index, count))
            })
            .collect()
    }

    fn gutter_width(&self, lines: usize) -> usize {
        if !self.line_numbers {
            return 0;
        }
        let last = self.start_line + lines.saturating_sub(1);
        // The number, a space, the rule and a space: `12 │ `.
        last.to_string().len() + 3
    }

    fn highlighted_lines(&self) -> Vec<Text> {
        let lines = self.source_lines();
        let mut syntax =
            Syntax::new(self.code.clone(), self.language.clone()).tab_size(self.tab_size);
        if let Some(theme) = &self.theme {
            syntax = syntax.theme(theme.clone());
        }
        let highlighted = syntax.highlight();
        let mut texts = highlighted.split("\n", false, true);
        texts.truncate(lines.len());
        // Carriage returns of CRLF files are not content.
        texts
            .into_iter()
            .map(|text| {
                if text.plain().ends_with('\r') {
                    let keep = text.plain().len() - 1;
                    text.divide(&[keep]).into_iter().next().unwrap_or(text)
                } else {
                    text
                }
            })
            .collect()
    }
}

/// Tabs expanded as `Syntax::highlight` expands them, so match offsets agree.
fn expand_tabs(line: &str, tab_size: usize) -> String {
    if !line.contains('\t') || tab_size == 0 {
        return line.to_string();
    }
    let mut out = String::new();
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = tab_size - column % tab_size;
            out.push_str(&" ".repeat(spaces));
            column += spaces;
        } else {
            out.push(c);
            column += rich::cells::char_cell_width(c);
        }
    }
    out
}

impl Renderable for SourceView {
    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let lines = self.source_lines();
        let widest = lines
            .iter()
            .map(|line| cell_len(&expand_tabs(line, self.tab_size)))
            .max()
            .unwrap_or(0);
        let gutter = self.gutter_width(lines.len());
        Measurement::new((gutter + 1).min(options.max_width), gutter + widest)
            .with_maximum(options.max_width)
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let texts = self.highlighted_lines();
        let gutter = self.gutter_width(texts.len());
        let width = options.max_width.saturating_sub(gutter).max(1);
        let number_width = gutter.saturating_sub(3);
        let dim = Style::parse("dim").unwrap_or_default();
        let marked = Style::parse("bold yellow").unwrap_or_default();
        let found = Style::parse("black on yellow").unwrap_or_default();
        let rule = if console.ascii_only() { "|" } else { "│" };
        let mut out = Vec::new();
        for (index, mut text) in texts.into_iter().enumerate() {
            let ranges = self.match_ranges(text.plain());
            for &(start, end) in &ranges {
                text.stylize(found.clone(), start, end);
            }
            let rows = text.render_lines_wrapped(
                console.theme(),
                &Style::new(),
                Some(width),
                Justify::Left,
                Overflow::Fold,
                false,
            );
            let rows = if rows.is_empty() {
                vec![Vec::new()]
            } else {
                rows
            };
            for (row_index, row) in rows.into_iter().enumerate() {
                if !out.is_empty() {
                    out.push(Segment::line());
                }
                if gutter > 0 {
                    let number = if row_index == 0 {
                        format!("{:>number_width$}", self.start_line + index)
                    } else {
                        " ".repeat(number_width)
                    };
                    let style = if ranges.is_empty() { &dim } else { &marked };
                    out.push(Segment::new(number, Some(style.clone())));
                    out.push(Segment::new(format!(" {rule} "), Some(dim.clone())));
                }
                out.extend(row);
            }
        }
        out
    }
}

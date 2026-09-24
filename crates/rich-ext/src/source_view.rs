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
use rich::{
    Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Style, Syntax, Text, Theme,
};

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
                (count > 0).then_some((self.start_line.saturating_add(index), count))
            })
            .collect()
    }

    fn gutter_width(&self, lines: usize) -> usize {
        if !self.line_numbers {
            return 0;
        }
        let last = self.start_line.saturating_add(lines.saturating_sub(1));
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

/// One hard line wrapped to `width`: exactly what core's
/// `text.render_lines_wrapped(theme, &Style::new(), Some(width), Justify::Left,
/// Overflow::Fold, false)` returns, in time linear in the line's spans.
///
/// Core styles every visual line by scanning every span for every segment,
/// which is quadratic in the spans of a line; a minified JSON file is one
/// line holding tens of thousands of highlighted tokens. Here the span
/// boundaries are swept once for the whole line, and each wrapped row takes
/// its run of the resulting pieces.
fn wrap_rows(text: &Text, theme: &Theme, width: usize) -> Vec<Vec<Segment>> {
    let plain = text.plain();
    if plain.contains(['\t', '\n']) || width == 0 {
        // Not a single tab-free line: leave it to core.
        return text.render_lines_wrapped(
            theme,
            &Style::new(),
            Some(width),
            Justify::Left,
            Overflow::Fold,
            false,
        );
    }
    let len = plain.len();
    let spans = text.spans();
    let resolved: Vec<Style> = spans
        .iter()
        .map(|span| theme.get_style_or_null(&span.style))
        .collect();
    // The text's own base style, resolved as core resolves it.
    let base = {
        let mut probe = text.blank_copy();
        probe.append("x", None);
        probe
            .render(theme, &Style::new())
            .into_iter()
            .next()
            .and_then(|segment| segment.style)
            .unwrap_or_default()
    };

    // Row boundaries: the wrap's char offsets as byte offsets.
    let mut cuts = Vec::new();
    cuts.push(0);
    let mut chars = plain.char_indices().map(|(at, _)| at).peekable();
    let mut char_index = 0;
    for offset in rich::wrap::divide_line(plain, width, true) {
        while char_index < offset && chars.next().is_some() {
            char_index += 1;
        }
        cuts.push(chars.peek().copied().unwrap_or(len));
    }
    cuts.push(len);

    // Every span edge and row edge, then the style of each piece between
    // them: the base combined with every covering span, in span order.
    let mut points: Vec<usize> = cuts.clone();
    for span in spans {
        points.push(span.start.min(len));
        points.push(span.end.min(len));
    }
    points.sort_unstable();
    points.dedup();
    let mut by_start: Vec<usize> = (0..spans.len())
        .filter(|&i| spans[i].start < spans[i].end)
        .collect();
    by_start.sort_by_key(|&i| spans[i].start);
    let mut next = 0;
    let mut active: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    let mut changed = true;
    let mut style = base.clone();
    let mut pieces: Vec<(usize, usize, Style)> = Vec::with_capacity(points.len());
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        while next < by_start.len() && spans[by_start[next]].start <= a {
            active.insert(by_start[next]);
            next += 1;
            changed = true;
        }
        // No edge lies inside (a, b), so a span covers the piece exactly
        // when it starts at or before `a` and ends after it.
        let before = active.len();
        active.retain(|&i| spans[i].end > a);
        changed |= active.len() != before;
        if changed {
            style = active
                .iter()
                .fold(base.clone(), |style, &i| style.combine(&resolved[i]));
            changed = false;
        }
        pieces.push((a, b, style.clone()));
    }

    let mut rows = Vec::with_capacity(cuts.len() - 1);
    let mut piece = 0;
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        let mut line = Vec::new();
        while piece < pieces.len() && pieces[piece].1 <= end {
            let (a, b, style) = &pieces[piece];
            if *a >= start {
                line.push(Segment::new(&plain[*a..*b], Some(style.clone())));
            }
            piece += 1;
        }
        rstrip_end(&mut line, width);
        let excess = width.saturating_sub(line.iter().map(Segment::cell_length).sum());
        if excess > 0 {
            line.push(Segment::new(" ".repeat(excess), Some(base.clone())));
        }
        rows.push(fold_to(line, width));
    }
    rows
}

/// Core's `rstrip_end` on a rendered line: drop only as much trailing
/// whitespace as brings its *character* count down to `size`.
fn rstrip_end(line: &mut Vec<Segment>, size: usize) {
    let length: usize = line.iter().map(|s| s.text.chars().count()).sum();
    let Some(excess) = length.checked_sub(size).filter(|excess| *excess > 0) else {
        return;
    };
    let mut whitespace = 0;
    for segment in line.iter().rev() {
        let trimmed = segment.text.trim_end();
        whitespace += segment.text[trimmed.len()..].chars().count();
        if !trimmed.is_empty() {
            break;
        }
    }
    let mut remaining = whitespace.min(excess);
    while remaining > 0 {
        let Some(last) = line.last_mut() else { break };
        let length = last.text.chars().count();
        if length <= remaining {
            remaining -= length;
            line.pop();
        } else {
            let keep = last
                .text
                .char_indices()
                .nth(length - remaining)
                .map_or(last.text.len(), |(at, _)| at);
            last.text.truncate(keep);
            remaining = 0;
        }
    }
}

/// Core's fold truncation of a rendered line to `width` cells.
fn fold_to(line: Vec<Segment>, width: usize) -> Vec<Segment> {
    let plain: String = line.iter().map(|s| s.text.as_str()).collect();
    if cell_len(&plain) <= width {
        return line;
    }
    let kept = rich::cells::set_cell_size(&plain, width);
    let mut out = Vec::new();
    let mut offset = 0;
    for segment in line {
        if offset >= kept.len() {
            break;
        }
        let end = (offset + segment.text.len()).min(kept.len());
        if end > offset {
            out.push(Segment::new(&kept[offset..end], segment.style.clone()));
        }
        if offset + segment.text.len() > kept.len() {
            break;
        }
        offset = end;
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
            let rows = wrap_rows(&text, console.theme(), width);
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
                        format!("{:>number_width$}", self.start_line.saturating_add(index))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Core's wrapping, which `wrap_rows` must reproduce byte for byte.
    fn core_rows(text: &Text, theme: &Theme, width: usize) -> Vec<Vec<Segment>> {
        text.render_lines_wrapped(
            theme,
            &Style::new(),
            Some(width),
            Justify::Left,
            Overflow::Fold,
            false,
        )
    }

    fn check(text: &Text, theme: &Theme) {
        for width in 1..=24 {
            assert_eq!(
                wrap_rows(text, theme, width),
                core_rows(text, theme, width),
                "{:?} at width {width}",
                text.plain()
            );
        }
    }

    #[test]
    fn wrap_rows_matches_core() {
        let theme = Theme::default();
        let plains = [
            "",
            " ",
            "a",
            "hello world",
            "hello   world   ",
            "   leading and trailing   ",
            "averyveryverylongwordthatmustfold and more",
            "宽字符的文本需要折叠 和 空格",
            "emoji 👨‍👩‍👧 family 🇬🇧 flag e\u{301}",
            "zero\u{200d}width\u{2060}joiners and\u{fe0f} selectors",
            r#"{"k0":0,"k1":1,"k2":[true,false,null],"k3":"x y z"}"#,
            "a b c d e f g h i j k l m n o p q r s t u v w x y z",
        ];
        // A small LCG for reproducible, overlapping span layouts.
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |n: usize| {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            (seed >> 33) as usize % n.max(1)
        };
        let styles = [
            "bold",
            "red",
            "on blue",
            "italic underline",
            "repr.number",
            "dim",
        ];
        for plain in plains {
            let boundaries: Vec<usize> = (0..=plain.len())
                .filter(|&i| plain.is_char_boundary(i))
                .collect();
            check(&Text::new(plain), &theme);
            check(&Text::styled(plain, "on green"), &theme);
            for _ in 0..12 {
                let mut text = Text::new(plain);
                if next(3) == 0 {
                    text.set_base_style("yellow");
                }
                for _ in 0..next(12) {
                    let a = boundaries[next(boundaries.len())];
                    let b = boundaries[next(boundaries.len())];
                    text.stylize(styles[next(styles.len())], a.min(b), a.max(b));
                }
                check(&text, &theme);
            }
        }
        // Highlighted source, as the view renders it.
        for (code, language) in [
            (
                r#"fn main() { let x = "hi there"; println!("{x}"); } // done"#,
                "rust",
            ),
            (
                r#"{"a": [1, 2, {"b": "c d e"}], "f": null, "g": 3.5e10}"#,
                "json",
            ),
            ("def f(x):  return x ** 2  # squares", "python"),
        ] {
            let text = Syntax::new(code, language).highlight();
            let mut text = text.split("\n", false, true).remove(0);
            let at = text.plain().find(' ').unwrap_or(0);
            text.stylize("black on yellow", at, at + 3);
            check(&text, &theme);
        }
    }
}

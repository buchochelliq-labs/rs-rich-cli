//! Syntax highlighting.
//!
//! Port of `rich/syntax.py`'s renderable surface. A [`Syntax`] highlights a
//! block of source code for a given language and theme, producing colored
//! [`Segment`]s (a solid block: each line is padded to the render width with
//! the theme background).
//!
//! Highlighting goes through a [`CodeHighlighter`]. The default,
//! [`SyntectHighlighter`], uses the `syntect` crate.
//!
//! **Divergence:** upstream uses Pygments; `syntect` ships different grammars
//! and themes. So the *coloring is functional, not byte-identical* to Python
//! rich — see docs/DIVERGENCES.md. Everything else (the renderable protocol,
//! width handling) matches the port's conventions.

use std::sync::Arc;

#[cfg(feature = "syntax-cache")]
#[path = "syntax_cache.rs"]
mod cache;
#[path = "syntax_syntect.rs"]
mod syntect_adapter;

pub use syntect_adapter::SyntectHighlighter;

use crate::cells::cell_len;
use crate::console::{Console, ConsoleOptions};
use crate::measure::Measurement;
use crate::protocol::{
    CodeHighlighter, HighlightError, HighlightedCode, HighlightedLine, Renderable,
};
use crate::segment::Segment;
use crate::style::Style;
use crate::text::is_control_code;

/// Upstream's `Syntax(tab_size=4)`.
const DEFAULT_TAB_SIZE: usize = 4;

/// A block of syntax-highlighted source code. Mirrors `rich.syntax.Syntax`.
pub struct Syntax {
    code: String,
    language: Option<String>,
    /// `None` means the highlighter's default theme.
    theme: Option<String>,
    word_wrap: bool,
    padding: usize,
    tab_size: usize,
    /// `None` means [`SyntectHighlighter`].
    highlighter: Option<Arc<dyn CodeHighlighter>>,
}

/// Port of Python's `str.expandtabs(tab_size)`, which `Syntax._process_code`
/// runs over the source before highlighting it.
///
/// A tab advances to the next multiple of `tab_size` **counted in characters,
/// not cells** (CPython's `unicode_expandtabs` walks code points), and the
/// column resets at `\n` and `\r`. `tab_size == 0` deletes the tab, matching
/// CPython's `tabsize <= 0` branch.
///
/// Without this the raw U+0009 reached the terminal, where it jumps to the next
/// 8-cell stop while we had measured it as one cell: a block asked to be 30
/// wide rendered 31-32 cells and tore the background panel.
fn expand_tabs(code: &str, tab_size: usize) -> String {
    if !code.contains('\t') {
        return code.to_string();
    }
    let mut out = String::with_capacity(code.len());
    let mut column = 0usize;
    for ch in code.chars() {
        match ch {
            '\t' => {
                if tab_size > 0 {
                    let advance = tab_size - (column % tab_size);
                    out.extend(std::iter::repeat_n(' ', advance));
                    column += advance;
                }
            }
            '\n' | '\r' => {
                out.push(ch);
                column = 0;
            }
            _ => {
                out.push(ch);
                column += 1;
            }
        }
    }
    out
}

impl Syntax {
    /// Wrap lines wider than the render width instead of cropping them.
    ///
    /// Off by default, matching upstream's `Syntax(word_wrap=False)`: a long
    /// line is cut at the width. Upstream's **CLI** turns this on, which is why
    /// `rich --syntax` does too — cropping a source file silently loses code.
    pub fn word_wrap(mut self, wrap: bool) -> Self {
        self.word_wrap = wrap;
        self
    }

    /// Highlight `code` as `language` (a name or file extension, e.g. `"rust"`
    /// or `"rs"`). Pass an empty/unknown language to render as plain text.
    pub fn new(code: impl Into<String>, language: impl Into<String>) -> Self {
        Syntax {
            word_wrap: false,
            padding: 0,
            tab_size: DEFAULT_TAB_SIZE,
            code: code.into(),
            language: Some(language.into()).filter(|l| !l.is_empty()),
            theme: None,
            highlighter: None,
        }
    }

    /// Highlight with `highlighter` instead of the default
    /// [`SyntectHighlighter`]. Theme names are the highlighter's own.
    pub fn highlighter(mut self, highlighter: Arc<dyn CodeHighlighter>) -> Self {
        self.highlighter = Some(highlighter);
        self
    }

    /// How far a tab advances the column, in characters. Upstream's
    /// `Syntax(tab_size=…)`, default 4.
    ///
    /// Tabs are *expanded* to spaces before highlighting (upstream's
    /// `code.expandtabs(self.tab_size)`), so this is the only tab handling in
    /// play — the rendered code contains no U+0009 at all.
    pub fn tab_size(mut self, tab_size: usize) -> Self {
        self.tab_size = tab_size;
        self
    }

    /// Surround the code with `padding` cells of background on every side.
    ///
    /// Upstream's Markdown renders a fenced block as `Syntax(..., padding=1)`,
    /// which is what gives a code block its blank inset row above and below and
    /// its one-column gutter. Without it the code sat flush against the
    /// surrounding text and every document containing a fence diverged.
    pub fn padding(mut self, padding: usize) -> Self {
        self.padding = padding;
        self
    }

    /// Choose the highlighting theme, by the highlighter's name for it. The
    /// default highlighter offers `syntect`'s themes plus upstream's
    /// `ansi_dark` and `ansi_light`. Unknown names fall back to the
    /// highlighter's default theme.
    pub fn theme(mut self, theme: impl Into<String>) -> Self {
        self.theme = Some(theme.into());
        self
    }

    /// Highlight the (already tab-expanded) `code`, falling back to the default
    /// theme for an unknown one and to plain text if the engine fails, then
    /// validate the result against the source (see [`CodeHighlighter`]).
    ///
    /// The engine is this `Syntax`'s own, else `console`'s default (whose theme
    /// then applies unless this `Syntax` names one), else syntect.
    fn highlighted(&self, code: &str, console: Option<&Console>) -> HighlightedCode {
        use crate::protocol::ConsoleCodeHighlighting;
        let default = match &self.highlighter {
            Some(_) => None,
            None => console.and_then(|console| console.code_highlighting()),
        };
        let engine = self
            .highlighter
            .clone()
            .or_else(|| default.map(|d| d.highlighter.clone()))
            .unwrap_or_else(SyntectHighlighter::shared);
        let language = self.language.as_deref();
        let theme = self
            .theme
            .as_deref()
            .or_else(|| default.and_then(|d| d.theme.as_deref()))
            .unwrap_or(engine.default_theme());
        let result = match engine.highlight(code, language, theme) {
            Err(HighlightError::UnknownTheme(_)) => {
                engine.highlight(code, language, engine.default_theme())
            }
            other => other,
        };
        let highlighted = result.unwrap_or_default();
        validate(code, highlighted)
    }
}

/// Make a highlighter's output safe to render against `code`: one line per
/// `code.split('\n')` element, spans sorted, in range, non-overlapping and on
/// character boundaries, and no hyperlinks in adapter styles.
fn validate(code: &str, mut highlighted: HighlightedCode) -> HighlightedCode {
    let sources: Vec<&str> = code.split('\n').collect();
    highlighted
        .lines
        .resize_with(sources.len(), HighlightedLine::default);
    for (line, source) in highlighted.lines.iter_mut().zip(&sources) {
        let mut end = 0usize;
        line.spans.retain(|span| {
            let keep = span.range.start < span.range.end
                && span.range.start >= end
                && span.range.end <= source.len()
                && source.is_char_boundary(span.range.start)
                && source.is_char_boundary(span.range.end);
            if keep {
                end = span.range.end;
            }
            keep
        });
        for span in &mut line.spans {
            span.style = span.style.update_link(None);
        }
        line.newline_style = line.newline_style.as_ref().map(|s| s.update_link(None));
    }
    highlighted.default_style = highlighted.default_style.update_link(None);
    highlighted
}

/// The pieces of one source line: every span, plus the gaps between them in
/// the default style, in order.
fn line_pieces<'a>(
    source: &'a str,
    line: &HighlightedLine,
    default_style: &Style,
) -> Vec<(&'a str, Style)> {
    let mut pieces = Vec::with_capacity(line.spans.len() + 1);
    let mut position = 0usize;
    for span in &line.spans {
        if span.range.start > position {
            pieces.push((&source[position..span.range.start], default_style.clone()));
        }
        pieces.push((&source[span.range.clone()], span.style.clone()));
        position = span.range.end;
    }
    if position < source.len() {
        pieces.push((&source[position..], default_style.clone()));
    }
    pieces
}

impl Syntax {
    /// Highlight the code into a [`Text`](crate::text::Text) rather than a padded block. Port of
    /// `Syntax.highlight`: the theme background is the text's base style and
    /// every token carries its own style. Tabs are expanded first, as
    /// `_process_code` does. Used by `Markdown(inline_code_lexer=…)`.
    pub fn highlight(&self) -> crate::text::Text {
        self.highlight_text(None)
    }

    /// [`highlight`](Self::highlight) with `console`'s default code
    /// highlighter (see [`ConsoleCodeHighlighting`](crate::protocol::ConsoleCodeHighlighting))
    /// when this `Syntax` has none of its own. Not in upstream.
    pub fn highlight_for(&self, console: &Console) -> crate::text::Text {
        self.highlight_text(Some(console))
    }

    fn highlight_text(&self, console: Option<&Console>) -> crate::text::Text {
        let code = expand_tabs(&self.code, self.tab_size);
        let highlighted = self.highlighted(&code, console);
        let mut text = crate::text::Text::new("");
        if let Some(background) = &highlighted.background {
            text.set_base_style(Style::new().with_bgcolor(background.clone()));
        }
        let sources: Vec<&str> = code.split('\n').collect();
        let last = sources.len().saturating_sub(1);
        for (index, (source, line)) in sources.iter().zip(&highlighted.lines).enumerate() {
            let mut pieces: Vec<(String, Style)> =
                line_pieces(source, line, &highlighted.default_style)
                    .into_iter()
                    .map(|(piece, style)| (piece.to_string(), style))
                    .collect();
            if index != last {
                // The engine's style for the line break. When it matches the
                // last piece, the break joins that piece, as one token.
                let style = line.newline_style.clone().unwrap_or_default();
                match pieces.last_mut() {
                    Some((piece, last_style))
                        if line.newline_style.is_some() && *last_style == style =>
                    {
                        piece.push('\n');
                    }
                    _ => pieces.push(("\n".to_string(), style)),
                }
            }
            for (piece, style) in pieces {
                text.append(piece.as_str(), Some(style.into()));
            }
        }
        text
    }
}

/// Port of Python's `str.splitlines()`: every Unicode line boundary ends a
/// line, `\r\n` counts once, and a trailing boundary adds no empty line.
fn python_splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        ) {
            lines.push(&text[start..i]);
            start = i + c.len_utf8();
            if c == '\r' && chars.peek().map(|&(_, n)| n) == Some('\n') {
                chars.next();
                start += 1;
            }
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

impl Renderable for Syntax {
    /// Port of `Syntax.__rich_measure__` (no line numbers or `code_width` in
    /// this port, so the numbers column is zero wide). Like upstream it
    /// measures the raw source, where a tab counts as zero cells.
    fn measure(&self, _console: &Console, _options: &ConsoleOptions) -> Measurement {
        let widest = python_splitlines(&self.code)
            .into_iter()
            .map(cell_len)
            .max()
            .unwrap_or(0);
        Measurement::new(0, self.padding * 2 + widest)
    }

    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // The gutter eats into the space the code itself may occupy.
        let width = options.max_width;
        let code_width = width.saturating_sub(self.padding * 2);

        // `Syntax._process_code`: the source is tab-expanded before it reaches
        // the highlighter, so no U+0009 ever survives into a segment.
        let code = expand_tabs(&self.code, self.tab_size);
        let highlighted = self.highlighted(&code, Some(console));
        let background = highlighted.background.clone();

        // Upstream splits the source with Python's `str.split("\n")`, which keeps
        // the empty element after a trailing newline — so a file ending in `\n`
        // gets one final padded blank row, and an empty source one row. The
        // highlighter returns exactly one line per element of that split.
        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for (source, line) in code.split('\n').zip(&highlighted.lines) {
            let mut row: Vec<Segment> = Vec::new();
            for (text, style) in line_pieces(source, line, &highlighted.default_style) {
                // Upstream's Syntax builds a `Text`, so `strip_control_codes`
                // runs on every token. We emit segments directly, which let BEL,
                // backspace, vertical tab and form feed through to the terminal
                // — a backspace run rewrites what the reader sees.
                let text: String = text.chars().filter(|c| !is_control_code(*c)).collect();
                if text.is_empty() {
                    continue;
                }
                row.push(Segment::new(text, Some(style)));
            }
            lines.push(row);
        }

        // Wrapping happens before padding, so every *visual* row gets the same
        // background treatment rather than only the first.
        if self.word_wrap {
            lines = lines
                .into_iter()
                .flat_map(|row| {
                    // A blank source line has no segments at all, and folding an
                    // empty row yields *zero* rows rather than one empty one — so
                    // wrapping silently deleted every blank line in the file.
                    // `rich -x` on a 2698-line source dropped all 386 of them, and
                    // the loss was baked into HTML exports too.
                    if row.is_empty() {
                        vec![Vec::new()]
                    } else {
                        Segment::split_lines(&Segment::fold_lines_words(&row, code_width))
                    }
                })
                .collect();
        }

        // Left gutter, then the blank inset rows, both in the block background.
        let pad_style = {
            let mut style = Style::new();
            if let Some(bg) = &background {
                style = style.with_bgcolor(bg.clone());
            }
            style
        };
        if self.padding > 0 {
            for row in &mut lines {
                row.insert(
                    0,
                    Segment::new(" ".repeat(self.padding), Some(pad_style.clone())),
                );
            }
            let blank = vec![Segment::new(" ".repeat(width), Some(pad_style.clone()))];
            for _ in 0..self.padding {
                lines.insert(0, blank.clone());
                lines.push(blank.clone());
            }
        }

        // Pad each line to the full width with the theme background, so the
        // block reads as a solid panel of code.
        for row in &mut lines {
            let used: usize = row.iter().map(Segment::cell_length).sum();
            if width > used {
                let mut pad = Style::new();
                if let Some(bg) = &background {
                    pad = pad.with_bgcolor(bg.clone());
                }
                row.push(Segment::new(" ".repeat(width - used), Some(pad)));
            }
        }

        let mut segments = Vec::new();
        let last = lines.len().saturating_sub(1);
        for (index, line) in lines.into_iter().enumerate() {
            segments.extend(line);
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use std::sync::Arc;

    fn render(code: &str, lang: &str, width: usize) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .no_color(false)
            .build()
            .render_to_string(&Syntax::new(code, lang))
    }

    #[test]
    fn measured_syntax_still_prints_at_full_width() {
        // Upstream renders a printed Syntax at the console width (its background
        // pads every row); only str/Text shrink to their measurement.
        let console = Console::builder().width(30).color_system(None).build();
        let syntax = Syntax::new("x = 1", "python");
        assert_eq!(syntax.measure(&console, &console.options()).maximum, 5);
        let out = console.render_to_string(&syntax);
        assert!(!out.contains('\x1b'), "{out:?}");
        assert_eq!(cell_len(out.lines().next().unwrap()), 30, "{out:?}");
    }

    #[test]
    fn splitlines_matches_python() {
        assert_eq!(
            python_splitlines("a\r\nb\rc\u{2028}d\n"),
            ["a", "b", "c", "d"]
        );
        assert_eq!(python_splitlines("\n\n"), ["", ""]);
        assert!(python_splitlines("").is_empty());
    }

    #[test]
    fn highlights_rust_keyword() {
        // Functional (not byte-parity): assert the code text survives and the
        // output is colored (contains SGR sequences).
        let out = render("fn main() {}", "rust", 20);
        assert!(out.contains("fn"));
        assert!(out.contains("main"));
        assert!(out.contains('\x1b'), "expected ANSI color codes");
    }

    #[test]
    fn multiple_lines_are_separated() {
        let out = render("let x = 1;\nlet y = 2;", "rust", 20);
        assert_eq!(out.matches('\n').count(), 1);
        assert!(out.contains("let"));
    }

    #[test]
    fn unknown_language_renders_plain() {
        // No panic, code preserved, still padded/colored to a block.
        let out = render("just some text", "nonsense-lang", 20);
        assert!(out.contains("just some text"));
    }

    #[test]
    fn word_wrap_is_off_by_default_matching_upstream() {
        // Measured against upstream: Syntax(word_wrap=False) at width 80 keeps
        // 80 of 300 characters. The default must not diverge from that.
        let code = "A".repeat(300);
        let out = render(&code, "python", 80);
        assert_eq!(out.matches('A').count(), 80, "default should crop");
    }

    #[test]
    fn word_wrap_keeps_every_character() {
        let code = "A".repeat(300);
        let console = Console::builder().width(80).color_system(None).build();
        let out = console.render_to_string(&Syntax::new(code.as_str(), "python").word_wrap(true));
        assert_eq!(
            out.matches('A').count(),
            300,
            "wrapping must not lose characters:
{out}"
        );
    }

    /// Syntax emits segments directly rather than going through `Text`, so the
    /// shared `strip_control_codes` never ran and `rich -x` leaked backspaces
    /// and BELs that `rich -m` did not.
    #[test]
    fn control_codes_are_stripped_from_highlighted_code() {
        let out = render("let x = 1;\u{7}\u{8}\u{b}\u{c}", "rust", 40);
        for code in ['\u{7}', '\u{8}', '\u{b}', '\u{c}'] {
            assert!(
                !out.contains(code),
                "control code {code:?} reached the output"
            );
        }
        assert!(out.contains("let"), "content lost with the control codes");
    }

    /// A blank source line has no segments, and folding an empty row yielded
    /// zero rows rather than one empty one — so wrapping silently deleted every
    /// blank line in the file, and the loss was baked into exports.
    #[test]
    fn word_wrap_keeps_blank_lines() {
        let console = Console::builder().width(20).color_system(None).build();
        let out =
            console.render_to_string(&Syntax::new("a = 1\n\nb = 2\n", "python").word_wrap(true));
        let rows: Vec<&str> = out.trim_end_matches('\n').split('\n').collect();
        // Four rows, not three: upstream splits with Python's `str.split("\n")`,
        // so the trailing newline contributes a final empty row —
        // `"a = 1\n\nb = 2\n".split("\n") == ["a = 1", "", "b = 2", ""]`, and
        // rich 15.0.0 prints four padded rows for it. This assertion previously
        // said three, pinning our own missing-row bug as the expectation.
        assert_eq!(rows.len(), 4, "blank line lost: {rows:?}");
        assert!(
            rows[1].trim().is_empty(),
            "middle row should be blank: {rows:?}"
        );
        assert!(
            rows[3].trim().is_empty(),
            "trailing row should be blank: {rows:?}"
        );
    }

    /// `Syntax._process_code` runs `code.expandtabs(self.tab_size)` before
    /// anything is highlighted. We emitted the raw U+0009 and measured it as one
    /// cell, so a tabbed line reached the terminal 31-32 cells wide against a
    /// requested 30 and tore the background block.
    ///
    /// Both expectations captured verbatim from real rich 15.0.0.
    #[test]
    fn tabs_are_expanded_before_highlighting() {
        let console = Console::builder().width(30).color_system(None).build();
        let out = console.render_to_string(&Syntax::new(
            "def f():\n\tif x:\n\t\treturn 1\n\treturn 0",
            "python",
        ));
        assert_eq!(
            out.split('\n').collect::<Vec<_>>(),
            [
                "def f():                      ",
                "    if x:                     ",
                "        return 1              ",
                "    return 0                  ",
            ]
        );
        assert!(!out.contains('\t'), "a raw tab survived: {out:?}");
    }

    /// A tab advances to the next multiple of the tab size, so it is *not* a
    /// fixed run of spaces — the width of the text before it decides.
    #[test]
    fn a_tab_advances_to_the_next_tab_stop() {
        let console = Console::builder().width(20).color_system(None).build();
        let out = console.render_to_string(&Syntax::new(
            "a\tb\tc\nab\tcd\tef\nabcd\tefgh\tijkl",
            "python",
        ));
        assert_eq!(
            out.split('\n').collect::<Vec<_>>(),
            [
                "a   b   c           ",
                "ab  cd  ef          ",
                "abcd    efgh    ijkl",
            ]
        );
    }

    /// Every row must occupy exactly the requested width *on screen*.
    ///
    /// Measuring against [`cell_len`] cannot catch this: it counted a raw tab as
    /// one cell and the padding was computed the same way, so the row looked
    /// exactly `width` wide to us while the terminal advanced the tab to the
    /// next 8-cell stop and the block overran by seven.
    #[test]
    fn a_tabbed_line_measures_the_requested_width() {
        /// Width as the *terminal* renders it: a tab jumps to the next 8-cell
        /// stop, which is the only measure that reveals the defect.
        fn screen_width(row: &str) -> usize {
            let mut column = 0usize;
            for ch in row.chars() {
                column += if ch == '\t' {
                    8 - (column % 8)
                } else {
                    cell_len(ch.encode_utf8(&mut [0u8; 4]))
                };
            }
            column
        }

        for width in [10usize, 20, 30, 40] {
            let console = Console::builder().width(width).color_system(None).build();
            let out = console.render_to_string(&Syntax::new("\tvalue = compute(a, b)", "python"));
            for row in out.split('\n') {
                assert_eq!(screen_width(row), width, "row {row:?} at width {width}");
            }
        }
    }

    /// `str.expandtabs` counts *characters*, not cells, and resets its column at
    /// `\n` and `\r`.
    #[test]
    fn expand_tabs_matches_pythons_str_expandtabs() {
        // Left column verified against CPython's `str.expandtabs(4)`.
        for (input, expected) in [
            ("a\tb", "a   b"),
            ("ab\tb", "ab  b"),
            ("abc\tb", "abc b"),
            ("abcd\tb", "abcd    b"),
            ("\t", "    "),
            ("a\nbb\tc", "a\nbb  c"),
            ("a\rbb\tc", "a\rbb  c"),
            // A wide char counts as one column, exactly as in Python.
            ("\u{4e2d}\tx", "\u{4e2d}   x"),
        ] {
            assert_eq!(expand_tabs(input, 4), expected, "input {input:?}");
        }
        // `tabsize <= 0` deletes the tab (CPython's own branch).
        assert_eq!(expand_tabs("a\tb", 0), "ab");
    }

    /// Upstream's word_wrap breaks at word boundaries; we folded wherever the
    /// row filled up, splitting identifiers mid-word.
    #[test]
    fn word_wrap_breaks_between_words() {
        let console = Console::builder().width(30).color_system(None).build();
        // This exact line is the one character-folding splits as `z` / `eta`,
        // which is what makes the assertion discriminating.
        let code = "result = compute_total(alpha, beta, gamma, delta, epsilon, zeta, eta, theta)\n";
        let out = console.render_to_string(&Syntax::new(code, "python").word_wrap(true));
        // Every identifier must survive on a single row. Folding mid-word split
        // `epsilon` across the break as `e` / `psilon`.
        for word in [
            "compute_total",
            "alpha",
            "gamma",
            "epsilon",
            "zeta",
            "theta",
        ] {
            assert!(
                out.split('\n').any(|row| row.contains(word)),
                "{word:?} was split across rows: {out:?}"
            );
        }
    }

    // ---- CodeHighlighter (#522, #523) ---------------------------------------

    use crate::protocol::HighlightSpan;

    /// A highlighter that returns exactly the lines it was built with.
    struct Fixed(Vec<HighlightedLine>);

    impl CodeHighlighter for Fixed {
        fn highlight(
            &self,
            _code: &str,
            _language: Option<&str>,
            _theme: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            Ok(HighlightedCode {
                lines: self.0.clone(),
                ..Default::default()
            })
        }
        fn default_theme(&self) -> &str {
            "fixed"
        }
        fn themes(&self) -> Vec<String> {
            vec!["fixed".into()]
        }
        fn languages(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// Styles every line by theme: `bold` (its default) or `underline`.
    struct ByTheme;

    impl CodeHighlighter for ByTheme {
        fn highlight(
            &self,
            code: &str,
            _language: Option<&str>,
            theme: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            let style = match theme {
                "bold" | "underline" => Style::parse(theme).unwrap(),
                other => return Err(HighlightError::UnknownTheme(other.into())),
            };
            let lines = code
                .split('\n')
                .map(|line| HighlightedLine {
                    spans: (!line.is_empty())
                        .then(|| HighlightSpan {
                            range: 0..line.len(),
                            style: style.clone(),
                        })
                        .into_iter()
                        .collect(),
                    newline_style: None,
                })
                .collect();
            Ok(HighlightedCode {
                lines,
                ..Default::default()
            })
        }
        fn default_theme(&self) -> &str {
            "bold"
        }
        fn themes(&self) -> Vec<String> {
            vec!["bold".into(), "underline".into()]
        }
        fn languages(&self) -> Vec<String> {
            Vec::new()
        }
    }

    /// The console's default highlighter and theme apply to a `Syntax` (and
    /// to Markdown code) without its own; the `Syntax`'s own highlighter or
    /// theme wins; with no default, output is unchanged.
    #[test]
    fn a_console_default_highlighter_applies_where_none_is_given() {
        use crate::markdown::Markdown;
        use crate::protocol::{CodeHighlighting, ConsoleCodeHighlighting};
        let plain = || {
            Console::builder()
                .width(20)
                .force_terminal(true)
                .color_system(Some(crate::color::ColorSystem::Truecolor))
                .build()
        };
        let with = |theme: Option<&str>| {
            let mut console = plain();
            console.set_code_highlighting(Some(CodeHighlighting {
                highlighter: Arc::new(ByTheme),
                theme: theme.map(str::to_string),
            }));
            console
        };
        let syntax = || Syntax::new("x = 1", "python");

        assert!(with(None)
            .render_to_string(&syntax())
            .contains("\x1b[1mx = 1"));
        assert!(with(Some("underline"))
            .render_to_string(&syntax())
            .contains("\x1b[4mx = 1"));
        // The Syntax's own theme beats the console's.
        assert!(with(Some("underline"))
            .render_to_string(&syntax().theme("bold"))
            .contains("\x1b[1mx = 1"));
        // The Syntax's own highlighter beats the console's, and the console's
        // theme (a name of another engine) does not follow it.
        let own = with(Some("underline"))
            .render_to_string(&syntax().highlighter(SyntectHighlighter::shared()));
        assert_eq!(own, plain().render_to_string(&syntax()));
        // Markdown code blocks follow the console.
        let markdown = Markdown::new("```python\nx = 1\n```");
        assert!(with(None)
            .render_to_string(&markdown)
            .contains("\x1b[1mx = 1"));
        // `highlight_for` sees the console; `highlight` does not.
        assert!(!syntax().highlight_for(&with(None)).spans().is_empty());
        assert_eq!(
            syntax().highlight_for(&plain()).spans(),
            syntax().highlight().spans()
        );
        // With no default, nothing changes.
        let mut cleared = with(None);
        cleared.set_code_highlighting(None);
        assert_eq!(
            cleared.render_to_string(&syntax()),
            plain().render_to_string(&syntax())
        );
    }

    /// Always fails with an engine error.
    struct Broken;

    impl CodeHighlighter for Broken {
        fn highlight(
            &self,
            _code: &str,
            _language: Option<&str>,
            _theme: &str,
        ) -> Result<HighlightedCode, HighlightError> {
            Err(HighlightError::Engine("boom".into()))
        }
        fn default_theme(&self) -> &str {
            "x"
        }
        fn themes(&self) -> Vec<String> {
            vec!["x".into()]
        }
        fn languages(&self) -> Vec<String> {
            Vec::new()
        }
    }

    fn span(range: std::ops::Range<usize>, style: &str) -> HighlightSpan {
        HighlightSpan {
            range,
            style: Style::parse(style).unwrap(),
        }
    }

    fn line(spans: Vec<HighlightSpan>) -> HighlightedLine {
        HighlightedLine {
            spans,
            newline_style: None,
        }
    }

    fn render_with(code: &str, highlighter: Arc<dyn CodeHighlighter>, width: usize) -> String {
        Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Standard))
            .width(width)
            .build()
            .render_to_string(&Syntax::new(code, "python").highlighter(highlighter))
    }

    #[test]
    fn a_custom_highlighter_drives_the_rendered_styles() {
        let fixed = Fixed(vec![line(vec![span(0..1, "red"), span(4..5, "bold")])]);
        let out = render_with("x = 1", Arc::new(fixed), 7);
        // `x` red, ` = ` in the default (unstyled) gap, `1` bold, then padding.
        assert_eq!(out, "\u{1b}[31mx\u{1b}[0m = \u{1b}[1m1\u{1b}[0m  ");
    }

    #[test]
    fn invalid_spans_are_dropped_not_rendered() {
        let fixed = Fixed(vec![line(vec![
            span(0..3, "red"),
            span(1..2, "green"), // overlaps the first
            span(3..99, "blue"), // past the end of the line
            span(std::ops::Range { start: 2, end: 1 }, "yellow"), // reversed
        ])]);
        let out = render_with("abcdef", Arc::new(fixed), 6);
        assert_eq!(out, "\u{1b}[31mabc\u{1b}[0mdef");
        // A span ending inside a multi-byte character is dropped too.
        let fixed = Fixed(vec![line(vec![span(0..1, "red")])]);
        let out = render_with("é", Arc::new(fixed), 1);
        assert_eq!(out, "é");
    }

    #[test]
    fn missing_and_extra_lines_are_reconciled_with_the_source() {
        // One line returned for three source lines: the rest render unstyled.
        let fixed = Fixed(vec![line(vec![span(0..1, "red")])]);
        let out = render_with("a\nb\nc", Arc::new(fixed), 1);
        assert_eq!(out, "\u{1b}[31ma\u{1b}[0m\nb\nc");
        // Extra lines beyond the source are ignored.
        let fixed = Fixed(vec![
            line(vec![]),
            line(vec![]),
            line(vec![span(0..1, "red")]),
        ]);
        assert_eq!(render_with("a", Arc::new(fixed), 1), "a");
    }

    #[test]
    fn highlighter_styles_cannot_add_links_or_text() {
        let style = Style::parse("red")
            .unwrap()
            .with_link("https://evil.example/\u{1b}]0;x\u{7}");
        let fixed = Fixed(vec![line(vec![HighlightSpan { range: 0..1, style }])]);
        let out = render_with("a\u{8}", Arc::new(fixed), 1);
        assert!(!out.contains("\u{1b}]8"), "hyperlink escaped: {out:?}");
        assert!(!out.contains("evil"), "{out:?}");
        assert!(!out.contains('\u{8}'), "control code survived: {out:?}");
    }

    #[test]
    fn an_engine_failure_renders_the_source_unstyled() {
        let out = render_with("print(1)", Arc::new(Broken), 8);
        assert_eq!(out, "print(1)");
    }

    #[test]
    fn an_unknown_theme_falls_back_to_the_default_theme() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(20)
            .build();
        let default = console.render_to_string(&Syntax::new("x = 1", "python"));
        let unknown =
            console.render_to_string(&Syntax::new("x = 1", "python").theme("no-such-theme"));
        assert_eq!(default, unknown);
    }

    #[test]
    fn highlight_text_uses_the_same_highlighter() {
        let fixed = Fixed(vec![line(vec![span(0..1, "red")]), line(vec![])]);
        let text = Syntax::new("ab\ncd", "python")
            .highlighter(Arc::new(fixed))
            .highlight();
        assert_eq!(text.plain(), "ab\ncd");
        assert_eq!(text.spans()[0].start, 0);
        assert_eq!(text.spans()[0].end, 1);
    }

    /// Upstream's `ANSI_DARK`/`ANSI_LIGHT` colours, by Pygments token class,
    /// applied through the TextMate scopes syntect reports.
    #[test]
    fn ansi_themes_use_upstream_palette_colours() {
        let code = "# note\n@wraps\ndef greet(name):\n    return f\"hi {name}\" and 42\n";
        let render = |theme: &str, system: ColorSystem| {
            Console::builder()
                .force_terminal(true)
                .color_system(Some(system))
                .width(40)
                .build()
                .render_to_string(&Syntax::new(code, "python").theme(theme))
        };
        let dark = render("ansi_dark", ColorSystem::Truecolor);
        // No RGB colour and no background anywhere: only the 16 palette colours.
        assert!(!dark.contains("38;2;") && !dark.contains("48;"), "{dark:?}");
        assert!(dark.contains("\u{1b}[2m#"), "comment is dim: {dark:?}");
        assert!(
            dark.contains("\u{1b}[1;95m@"),
            "decorator bold bright_magenta: {dark:?}"
        );
        assert!(
            dark.contains("\u{1b}[33mf"),
            "string prefix is part of the string: {dark:?}"
        );
        assert!(
            dark.contains("\u{1b}[94mdef"),
            "keyword bright_blue: {dark:?}"
        );
        assert!(
            dark.contains("\u{1b}[92mgreet"),
            "function bright_green: {dark:?}"
        );
        assert!(
            dark.contains("\u{1b}[94m42"),
            "number bright_blue: {dark:?}"
        );
        assert!(
            dark.contains("\u{1b}[95mand"),
            "operator word bright_magenta: {dark:?}"
        );
        assert!(dark.contains("\u{1b}[33m"), "string yellow: {dark:?}");
        let light = render("ansi_light", ColorSystem::Truecolor);
        assert!(light.contains("\u{1b}[34mdef"), "keyword blue: {light:?}");
        assert!(
            light.contains("\u{1b}[32mgreet"),
            "function green: {light:?}"
        );
        // A 16-colour console gets exactly the same codes.
        assert_eq!(render("ansi_dark", ColorSystem::Standard), dark);
    }
}

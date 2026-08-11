//! Syntax highlighting.
//!
//! Port of `rich/syntax.py`'s renderable surface, powered by the `syntect`
//! crate. A [`Syntax`] highlights a block of source code for a given language
//! and theme, producing colored [`Segment`]s (a solid block: each line is padded
//! to the render width with the theme background).
//!
//! **Divergence:** upstream uses Pygments; we use `syntect`, which ships
//! different grammars and themes. So the *coloring is functional, not
//! byte-identical* to Python rich — see docs/DIVERGENCES.md. Everything else
//! (the renderable protocol, width handling) matches the port's conventions.

use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, FontStyle, Style as SynStyle, Theme, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::cells::cell_len;
use crate::color::Color;
use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::Style;
use crate::text::is_control_code;

/// The default theme (a dark base16 palette shipped with `syntect`).
const DEFAULT_THEME: &str = "base16-ocean.dark";

/// Upstream's `Syntax(tab_size=4)`.
const DEFAULT_TAB_SIZE: usize = 4;

/// A block of syntax-highlighted source code. Mirrors `rich.syntax.Syntax`.
pub struct Syntax {
    code: String,
    language: Option<String>,
    theme: String,
    word_wrap: bool,
    padding: usize,
    tab_size: usize,
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
            theme: DEFAULT_THEME.to_string(),
        }
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

    /// Choose the highlighting theme (a `syntect` theme name). Unknown names fall
    /// back to the default.
    pub fn theme(mut self, theme: impl Into<String>) -> Self {
        self.theme = theme.into();
        self
    }
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// Convert a `syntect` RGBA color to a truecolor [`Color`] (alpha dropped).
fn to_color(c: SynColor) -> Color {
    Color::from_rgb(c.r, c.g, c.b)
}

/// Convert a `syntect` style (fg/bg + font flags) to a rich [`Style`].
fn to_style(s: SynStyle) -> Style {
    let mut style = Style::new()
        .with_color(to_color(s.foreground))
        .with_bgcolor(to_color(s.background));
    if s.font_style.contains(FontStyle::BOLD) {
        style = style.combine(&Style::parse("bold").expect("valid style"));
    }
    if s.font_style.contains(FontStyle::ITALIC) {
        style = style.combine(&Style::parse("italic").expect("valid style"));
    }
    if s.font_style.contains(FontStyle::UNDERLINE) {
        style = style.combine(&Style::parse("underline").expect("valid style"));
    }
    style
}

impl Syntax {
    fn theme_ref<'a>(&self, themes: &'a ThemeSet) -> &'a Theme {
        themes
            .themes
            .get(&self.theme)
            .or_else(|| themes.themes.get(DEFAULT_THEME))
            .expect("default theme present")
    }
}

impl Renderable for Syntax {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let syntaxes = syntax_set();
        let themes = theme_set();
        let theme = self.theme_ref(themes);
        let background = theme.settings.background.map(to_color);

        // Resolve the language by token (name) or extension; else plain text.
        let syntax = self
            .language
            .as_deref()
            .and_then(|lang| {
                syntaxes
                    .find_syntax_by_token(lang)
                    .or_else(|| syntaxes.find_syntax_by_extension(lang))
            })
            .unwrap_or_else(|| syntaxes.find_syntax_plain_text());

        let mut highlighter = HighlightLines::new(syntax, theme);
        // The gutter eats into the space the code itself may occupy.
        let width = options.max_width;
        let code_width = width.saturating_sub(self.padding * 2);

        // `Syntax._process_code`: the source is tab-expanded before it reaches
        // the highlighter, so no U+0009 ever survives into a segment.
        let code = expand_tabs(&self.code, self.tab_size);

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for line in LinesWithEndings::from(&code) {
            let ranges = highlighter
                .highlight_line(line, syntaxes)
                .unwrap_or_default();
            let mut row: Vec<Segment> = Vec::new();
            let mut used = 0usize;
            for (syn_style, text) in ranges {
                let text = text.strip_suffix('\n').unwrap_or(text);
                if text.is_empty() {
                    continue;
                }
                // Upstream's Syntax builds a `Text`, so `strip_control_codes`
                // runs on every token. We emit segments directly, which let BEL,
                // backspace, vertical tab and form feed through to the terminal
                // — a backspace run rewrites what the reader sees.
                let text: String = text.chars().filter(|c| !is_control_code(*c)).collect();
                if text.is_empty() {
                    continue;
                }
                used += cell_len(&text);
                row.push(Segment::new(text, Some(to_style(syn_style))));
            }
            let _ = used;
            lines.push(row);
        }

        // Upstream splits the source with Python's `str.split("\n")`, which keeps
        // the empty element after a trailing newline — so a file ending in `\n`
        // gets one final padded blank row. `LinesWithEndings` yields no such
        // element, so every source (i.e. nearly every real file) rendered one row
        // short of upstream. An empty source splits to `[""]`, one row, too.
        if code.is_empty() || code.ends_with('\n') {
            lines.push(Vec::new());
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
        let console = Console::builder().width(80).no_color(true).build();
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
        let console = Console::builder().width(20).no_color(true).build();
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
        let console = Console::builder().width(30).no_color(true).build();
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
        let console = Console::builder().width(20).no_color(true).build();
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
            let console = Console::builder().width(width).no_color(true).build();
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
        let console = Console::builder().width(30).no_color(true).build();
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
}

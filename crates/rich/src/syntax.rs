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

/// A block of syntax-highlighted source code. Mirrors `rich.syntax.Syntax`.
pub struct Syntax {
    code: String,
    language: Option<String>,
    theme: String,
    word_wrap: bool,
    padding: usize,
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
            code: code.into(),
            language: Some(language.into()).filter(|l| !l.is_empty()),
            theme: DEFAULT_THEME.to_string(),
        }
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

        let mut lines: Vec<Vec<Segment>> = Vec::new();
        for line in LinesWithEndings::from(&self.code) {
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
        assert_eq!(rows.len(), 3, "blank line lost: {rows:?}");
        assert!(
            rows[1].trim().is_empty(),
            "middle row should be blank: {rows:?}"
        );
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

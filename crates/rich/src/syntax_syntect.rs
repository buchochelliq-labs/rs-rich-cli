//! The default [`CodeHighlighter`]: `syntect`, with its bundled grammars and
//! themes, plus upstream's two ANSI themes.
//!
//! Upstream highlights with Pygments, so the token colours are functional, not
//! byte-identical (DIVERGENCES #18). This module is the whole of the port's
//! dependency on `syntect`; everything else talks to the [`CodeHighlighter`]
//! trait.

use std::path::Path;
use std::sync::{Arc, OnceLock};

use syntect::highlighting::{
    Color as SynColor, FontStyle, ScopeSelectors, Style as SynStyle, StyleModifier, Theme,
    ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

#[cfg(not(feature = "syntax-cache"))]
use syntect::easy::HighlightLines;

use crate::color::Color;
use crate::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use crate::style::Style;

/// The default theme (a dark base16 palette shipped with `syntect`).
pub(crate) const DEFAULT_THEME: &str = "base16-ocean.dark";

/// Highlights with `syntect`: its default grammars, its default themes, and
/// upstream's `ansi_dark` and `ansi_light`, which use the terminal's own
/// palette. This is the highlighter `Syntax` and Markdown use unless given
/// another one.
#[derive(Clone, Copy, Debug, Default)]
pub struct SyntectHighlighter;

impl SyntectHighlighter {
    pub fn new() -> Self {
        SyntectHighlighter
    }

    /// The shared instance every `Syntax` uses by default.
    pub fn shared() -> Arc<dyn CodeHighlighter> {
        static SHARED: OnceLock<Arc<dyn CodeHighlighter>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(SyntectHighlighter)).clone()
    }
}

pub(crate) fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

pub(crate) fn theme_set() -> &'static ThemeSet {
    static SET: OnceLock<ThemeSet> = OnceLock::new();
    SET.get_or_init(ThemeSet::load_defaults)
}

/// A language by token (name) or extension; plain text otherwise.
fn resolve(language: Option<&str>) -> &'static SyntaxReference {
    let syntaxes = syntax_set();
    language
        .and_then(|lang| {
            syntaxes
                .find_syntax_by_token(lang)
                .or_else(|| syntaxes.find_syntax_by_extension(lang))
        })
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
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

// ---- ANSI themes ------------------------------------------------------------
//
// Upstream's `ansi_dark`/`ansi_light` (`rich/syntax.py`, `ANSI_DARK`/`ANSI_LIGHT`)
// style Pygments token types with the terminal's 16 colours. syntect matches
// TextMate scopes instead, so each token class below lists the scopes that
// correspond to it (DIVERGENCES #18). syntect does the scope matching against
// an internal theme whose "colours" are only class markers; the marker is then
// mapped to upstream's style for that class. Pygments' `Whitespace` and
// `Generic.Prompt` have no TextMate equivalent and are not mapped.

/// A Pygments token type, in upstream's table order.
#[derive(Clone, Copy)]
enum TokenType {
    Token,
    Comment,
    CommentPreproc,
    Keyword,
    KeywordType,
    Operator,
    OperatorWord,
    NameBuiltin,
    NameFunction,
    NameNamespace,
    NameClass,
    NameException,
    NameDecorator,
    NameVariable,
    NameConstant,
    NameAttribute,
    NameTag,
    String,
    Number,
    GenericDeleted,
    GenericInserted,
    GenericHeading,
    GenericSubheading,
    Error,
}

/// TextMate scope selectors for each class. syntect picks the most specific
/// match, so `keyword.operator` (plain operators) beats `keyword`.
const SCOPES: &[(TokenType, &str)] = &[
    (
        TokenType::Comment,
        "comment, punctuation.definition.comment",
    ),
    (
        TokenType::CommentPreproc,
        "meta.preprocessor, keyword.control.import.include, meta.annotation",
    ),
    (
        TokenType::Keyword,
        "keyword, storage.type, storage.modifier, constant.language",
    ),
    (
        TokenType::KeywordType,
        "support.type, storage.type.primitive, storage.type.numeric, storage.type.builtin",
    ),
    (TokenType::Operator, "keyword.operator"),
    (
        TokenType::OperatorWord,
        "keyword.operator.logical, keyword.operator.word",
    ),
    (
        TokenType::NameBuiltin,
        "support.function.builtin, variable.language",
    ),
    (TokenType::NameFunction, "entity.name.function"),
    (
        TokenType::NameNamespace,
        "entity.name.namespace, entity.name.module",
    ),
    (
        TokenType::NameClass,
        "entity.name.class, entity.name.struct, entity.name.enum, entity.name.union, \
         entity.name.trait, entity.name.type",
    ),
    (TokenType::NameException, "support.type.exception"),
    (
        TokenType::NameDecorator,
        "meta.decorator, meta.annotation.python, entity.name.function.decorator",
    ),
    (
        TokenType::NameVariable,
        "variable.other.readwrite.instance, variable.other.readwrite.class, \
         variable.other.readwrite.global, variable.other.readwrite.shell, \
         variable.other.normal.shell, variable.other.php",
    ),
    (
        TokenType::NameConstant,
        "variable.other.constant, constant.other, entity.name.constant, support.constant",
    ),
    (TokenType::NameAttribute, "entity.other.attribute-name"),
    (TokenType::NameTag, "entity.name.tag"),
    // `storage.type.string` is a string prefix (`f"…"`), Pygments' `String.Affix`.
    (
        TokenType::String,
        "string, punctuation.definition.string, storage.type.string",
    ),
    (TokenType::Number, "constant.numeric"),
    (TokenType::GenericDeleted, "markup.deleted"),
    (TokenType::GenericInserted, "markup.inserted"),
    (
        TokenType::GenericHeading,
        "markup.heading, meta.diff.header",
    ),
    (
        TokenType::GenericSubheading,
        "markup.heading.2, markup.heading.3, markup.heading.4, markup.heading.5, \
         markup.heading.6, meta.diff.range",
    ),
    (TokenType::Error, "invalid"),
];

/// Upstream's `ANSI_DARK` and `ANSI_LIGHT` styles for a class, as markup.
fn ansi_style_markup(class: TokenType, dark: bool) -> &'static str {
    use TokenType::*;
    match (class, dark) {
        (Token | Operator, _) => "",
        (Comment, _) => "dim",
        (CommentPreproc, true) => "bright_cyan",
        (CommentPreproc, false) => "cyan",
        (Keyword, true) => "bright_blue",
        (Keyword, false) => "blue",
        (KeywordType, true) => "bright_cyan",
        (KeywordType, false) => "cyan",
        (OperatorWord, true) => "bright_magenta",
        (OperatorWord, false) => "magenta",
        (NameBuiltin, true) => "bright_cyan",
        (NameBuiltin, false) => "cyan",
        (NameFunction, true) => "bright_green",
        (NameFunction, false) => "green",
        (NameNamespace, true) => "bright_cyan underline",
        (NameNamespace, false) => "cyan underline",
        (NameClass, true) => "bright_green underline",
        (NameClass, false) => "green underline",
        (NameException, true) => "bright_cyan",
        (NameException, false) => "cyan",
        (NameDecorator, true) => "bright_magenta bold",
        (NameDecorator, false) => "magenta bold",
        (NameVariable, true) => "bright_red",
        (NameVariable, false) => "red",
        (NameConstant, true) => "bright_red",
        (NameConstant, false) => "red",
        (NameAttribute, true) => "bright_cyan",
        (NameAttribute, false) => "cyan",
        (NameTag, _) => "bright_blue",
        (String, _) => "yellow",
        (Number, true) => "bright_blue",
        (Number, false) => "blue",
        (GenericDeleted, _) => "bright_red",
        (GenericInserted, true) => "bright_green",
        (GenericInserted, false) => "green",
        (GenericHeading, _) => "bold",
        (GenericSubheading, true) => "bright_magenta bold",
        (GenericSubheading, false) => "magenta bold",
        (Error, _) => "red underline",
    }
}

/// Classes in marker order: marker `n` is `CLASSES[n]`.
const CLASSES: [TokenType; 24] = [
    TokenType::Token,
    TokenType::Comment,
    TokenType::CommentPreproc,
    TokenType::Keyword,
    TokenType::KeywordType,
    TokenType::Operator,
    TokenType::OperatorWord,
    TokenType::NameBuiltin,
    TokenType::NameFunction,
    TokenType::NameNamespace,
    TokenType::NameClass,
    TokenType::NameException,
    TokenType::NameDecorator,
    TokenType::NameVariable,
    TokenType::NameConstant,
    TokenType::NameAttribute,
    TokenType::NameTag,
    TokenType::String,
    TokenType::Number,
    TokenType::GenericDeleted,
    TokenType::GenericInserted,
    TokenType::GenericHeading,
    TokenType::GenericSubheading,
    TokenType::Error,
];

fn marker(class: TokenType) -> SynColor {
    SynColor {
        r: class as u8,
        g: 0x5a,
        b: 0xa5,
        a: 0xff,
    }
}

/// The internal marker theme both ANSI themes share.
fn ansi_marker_theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| Theme {
        name: Some("rich-ansi-markers".into()),
        settings: ThemeSettings {
            foreground: Some(marker(TokenType::Token)),
            background: Some(marker(TokenType::Token)),
            ..ThemeSettings::default()
        },
        scopes: SCOPES
            .iter()
            .map(|&(class, selectors)| ThemeItem {
                scope: selectors
                    .parse::<ScopeSelectors>()
                    .expect("valid scope selectors"),
                style: StyleModifier {
                    foreground: Some(marker(class)),
                    background: None,
                    font_style: None,
                },
            })
            .collect(),
        ..Theme::default()
    })
}

/// Upstream's style for a marker colour.
fn ansi_style(foreground: SynColor, dark: bool) -> Style {
    static DARK: OnceLock<Vec<Style>> = OnceLock::new();
    static LIGHT: OnceLock<Vec<Style>> = OnceLock::new();
    let table = if dark { &DARK } else { &LIGHT };
    let styles = table.get_or_init(|| {
        CLASSES
            .iter()
            .map(|&class| {
                let markup = ansi_style_markup(class, dark);
                if markup.is_empty() {
                    Style::new()
                } else {
                    Style::parse(markup).expect("valid ANSI theme style")
                }
            })
            .collect()
    });
    styles
        .get(foreground.r as usize)
        .cloned()
        .unwrap_or_default()
}

/// A theme resolved for one highlight call.
enum Resolved {
    Syntect(&'static Theme),
    Ansi { dark: bool },
}

fn resolve_theme(name: &str) -> Result<Resolved, HighlightError> {
    match name {
        "ansi_dark" => Ok(Resolved::Ansi { dark: true }),
        "ansi_light" => Ok(Resolved::Ansi { dark: false }),
        _ => theme_set()
            .themes
            .get(name)
            .map(Resolved::Syntect)
            .ok_or_else(|| HighlightError::UnknownTheme(name.to_string())),
    }
}

impl CodeHighlighter for SyntectHighlighter {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        let resolved = resolve_theme(theme)?;
        let (syn_theme, background, default_style) = match &resolved {
            Resolved::Syntect(theme) => {
                let background = theme.settings.background.map(to_color);
                let mut default_style = Style::new();
                if let Some(bg) = &background {
                    default_style = default_style.with_bgcolor(bg.clone());
                }
                (*theme, background, default_style)
            }
            Resolved::Ansi { .. } => (ansi_marker_theme(), None, Style::new()),
        };
        let convert = |style: SynStyle| match resolved {
            Resolved::Syntect(_) => to_style(style),
            Resolved::Ansi { dark } => ansi_style(style.foreground, dark),
        };

        let syntaxes = syntax_set();
        let syntax = resolve(language);
        #[cfg(not(feature = "syntax-cache"))]
        let mut highlighter = HighlightLines::new(syntax, syn_theme);
        #[cfg(feature = "syntax-cache")]
        let mut highlighter = super::cache::CachedHighlighter::new(syntax, syn_theme);

        let mut lines = Vec::new();
        for line in LinesWithEndings::from(code) {
            let body = line.strip_suffix('\n').map_or(line.len(), str::len);
            let mut highlighted = HighlightedLine::default();
            let mut position = 0usize;
            // A line syntect fails on keeps no spans and renders unstyled.
            for (style, token) in highlighter
                .highlight_line(line, syntaxes)
                .unwrap_or_default()
            {
                let style = convert(style);
                let start = position;
                position += token.len();
                if token.ends_with('\n') {
                    highlighted.newline_style = Some(style.clone());
                }
                let end = position.min(body);
                if end > start {
                    highlighted.spans.push(HighlightSpan {
                        range: start..end,
                        style,
                    });
                }
            }
            lines.push(highlighted);
        }
        // `code.split('\n')` has one more element than `LinesWithEndings` yields
        // when the code is empty or ends with a newline.
        if code.is_empty() || code.ends_with('\n') {
            lines.push(HighlightedLine::default());
        }
        Ok(HighlightedCode {
            lines,
            background,
            default_style,
        })
    }

    fn default_theme(&self) -> &str {
        DEFAULT_THEME
    }

    /// Upstream's `ANSI_DARK`/`ANSI_LIGHT` entry for the token; for a syntect
    /// theme, its foreground (`Text`) or its `comment` scope style.
    fn token_style(&self, theme: &str, token: &str) -> Option<Style> {
        match (resolve_theme(theme).ok()?, token) {
            (Resolved::Ansi { .. }, "Comment") => Some(Style::parse("dim").expect("valid style")),
            (Resolved::Ansi { .. }, _) => None,
            (Resolved::Syntect(theme), "Text") => theme
                .settings
                .foreground
                .map(|color| Style::new().with_color(to_color(color))),
            (Resolved::Syntect(theme), "Comment") => {
                let scope = syntect::parsing::Scope::new("comment").ok()?;
                let style =
                    syntect::highlighting::Highlighter::new(theme).style_for_stack(&[scope]);
                Some(
                    to_style(style)
                        .without_color()
                        .combine(&Style::new().with_color(to_color(style.foreground))),
                )
            }
            _ => None,
        }
    }

    fn themes(&self) -> Vec<String> {
        let mut names: Vec<String> = theme_set().themes.keys().cloned().collect();
        names.extend(["ansi_dark".to_string(), "ansi_light".to_string()]);
        names.sort();
        names
    }

    fn languages(&self) -> Vec<String> {
        let mut names: Vec<String> = syntax_set()
            .syntaxes()
            .iter()
            .map(|syntax| syntax.name.clone())
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn language_for_path(&self, path: &Path) -> Option<String> {
        let syntaxes = syntax_set();
        let by_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| syntaxes.find_syntax_by_extension(name));
        let by_extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| syntaxes.find_syntax_by_extension(ext));
        by_name.or(by_extension).map(|syntax| syntax.name.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_ansi_scope_selector_parses_and_every_style_is_valid() {
        let _ = ansi_marker_theme();
        for dark in [true, false] {
            for class in CLASSES {
                let _ = ansi_style(marker(class), dark);
            }
        }
    }

    #[test]
    fn markers_are_in_class_order() {
        for (index, class) in CLASSES.iter().enumerate() {
            assert_eq!(*class as usize, index);
        }
    }

    #[test]
    fn lines_follow_split_semantics() {
        let highlighter = SyntectHighlighter;
        for code in ["", "a", "a\n", "a\nb", "a\n\nb\n", "\n"] {
            let out = highlighter
                .highlight(code, Some("python"), DEFAULT_THEME)
                .unwrap();
            assert_eq!(out.lines.len(), code.split('\n').count(), "{code:?}");
        }
    }

    #[test]
    fn unknown_theme_is_an_error_and_unknown_language_is_plain() {
        let highlighter = SyntectHighlighter;
        assert_eq!(
            highlighter.highlight("x", None, "no-such-theme"),
            Err(HighlightError::UnknownTheme("no-such-theme".into()))
        );
        let plain = highlighter
            .highlight("x = 1", Some("no-such-language"), DEFAULT_THEME)
            .unwrap();
        assert_eq!(plain.lines.len(), 1);
    }

    #[test]
    fn themes_include_the_ansi_pair_and_the_default() {
        let themes = SyntectHighlighter.themes();
        for name in ["ansi_dark", "ansi_light", DEFAULT_THEME] {
            assert!(
                themes.iter().any(|t| t == name),
                "{name} missing: {themes:?}"
            );
        }
    }

    #[test]
    fn language_for_path_uses_extensions() {
        let highlighter = SyntectHighlighter;
        assert_eq!(
            highlighter
                .language_for_path(Path::new("src/main.rs"))
                .as_deref(),
            Some("Rust")
        );
        assert_eq!(
            highlighter.language_for_path(Path::new("notes.unknownext")),
            None
        );
    }
}

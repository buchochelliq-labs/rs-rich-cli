//! The [lumis](https://lumis.sh) syntax highlighter for the `rich` Rust port.
//!
//! [`LumisHighlighter`] is a [`CodeHighlighter`]: tree-sitter grammars for
//! parsing, and lumis's Neovim themes (`monokai` by default, `dracula`,
//! `github_light`, `onedark` and over 250 more) for colour. Like the
//! built-in syntect adapter it also has `ansi_dark` and `ansi_light`, upstream
//! rich's themes in the terminal's own 16 colours.
//!
//! Give it to [`Syntax`](rich::syntax::Syntax) or
//! [`Markdown`](rich::markdown::Markdown) directly, or register [`LumisPlugin`]
//! with a plugin host so it can be chosen by name (`"lumis"`).
//!
//! ```
//! use rich::syntax::Syntax;
//! use rich::Console;
//! use rich_lumis::LumisHighlighter;
//!
//! let console = Console::builder().width(40).force_terminal(true).build();
//! let code = Syntax::new("fn main() {}", "rust")
//!     .highlighter(LumisHighlighter::shared())
//!     .theme("dracula");
//! assert!(console.render_to_string(&code).contains("main"));
//! ```
//!
//! lumis compiles every language it ships by default. For a smaller build,
//! turn default features off and pick a bundle (`bundle-web`, `bundle-system`,
//! `bundle-backend`, `bundle-web-extra`).

use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use lumis::highlight::highlight_iter;
use lumis::languages::{available_languages, language_id_for_filename, Language};
use lumis::themes::{self, Theme, UnderlineStyle};
use rich::color::Color;
use rich::protocol::{
    CodeHighlighter, HighlightError, HighlightSpan, HighlightedCode, HighlightedLine,
};
use rich::style::Style;
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar};

/// The theme used when none is chosen: upstream rich's default, `monokai`.
pub const DEFAULT_THEME: &str = "monokai";

/// Syntax highlighting by lumis.
#[derive(Clone, Copy, Debug, Default)]
pub struct LumisHighlighter;

impl LumisHighlighter {
    pub fn new() -> Self {
        LumisHighlighter
    }

    /// One shared instance.
    pub fn shared() -> Arc<dyn CodeHighlighter> {
        static SHARED: OnceLock<Arc<LumisHighlighter>> = OnceLock::new();
        SHARED.get_or_init(|| Arc::new(LumisHighlighter)).clone()
    }
}

/// Where span colours come from.
enum Palette {
    Theme(Box<Theme>),
    /// Upstream's `ANSI_DARK` (`true`) or `ANSI_LIGHT`.
    Ansi(bool),
}

fn palette(name: &str) -> Result<Palette, HighlightError> {
    match name {
        "ansi_dark" => Ok(Palette::Ansi(true)),
        "ansi_light" => Ok(Palette::Ansi(false)),
        _ => themes::get(name)
            .map(|theme| Palette::Theme(Box::new(theme)))
            .map_err(|_| HighlightError::UnknownTheme(name.to_string())),
    }
}

/// A lumis hex colour as a rich colour; `None` for anything else.
fn color(value: Option<&str>) -> Option<Color> {
    value.and_then(|value| Color::parse(value).ok())
}

/// A lumis token style as a rich style, over the theme's own colours.
fn theme_style(style: &themes::Style, base: &Style) -> Style {
    let mut out = base.clone();
    if let Some(fg) = color(style.fg.as_deref()) {
        out = out.with_color(fg);
    }
    if let Some(bg) = color(style.bg.as_deref()) {
        out = out.with_bgcolor(bg);
    }
    let mut flags = Vec::new();
    if style.bold {
        flags.push("bold");
    }
    if style.italic {
        flags.push("italic");
    }
    if style.text_decoration.underline != UnderlineStyle::None {
        flags.push("underline");
    }
    if style.text_decoration.strikethrough {
        flags.push("strike");
    }
    if !flags.is_empty() {
        out = out.combine(&Style::parse(&flags.join(" ")).expect("valid style"));
    }
    out
}

/// Pygments token types in upstream's ANSI themes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenType {
    Token,
    Comment,
    CommentPreproc,
    Keyword,
    KeywordType,
    OperatorWord,
    NameBuiltin,
    NameFunction,
    NameNamespace,
    NameClass,
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
    Error,
}

/// Tree-sitter capture names (as lumis reports them) to Pygments token types.
/// The longest matching prefix wins, so `keyword.operator` beats `keyword`.
const SCOPES: &[(&str, TokenType)] = &[
    ("comment", TokenType::Comment),
    ("keyword.directive", TokenType::CommentPreproc),
    ("keyword.type", TokenType::KeywordType),
    ("keyword.operator", TokenType::OperatorWord),
    ("keyword", TokenType::Keyword),
    ("boolean", TokenType::Keyword),
    ("constant.builtin", TokenType::Keyword),
    ("type.builtin", TokenType::KeywordType),
    ("type", TokenType::NameClass),
    ("constructor", TokenType::NameClass),
    ("function.builtin", TokenType::NameBuiltin),
    ("variable.builtin", TokenType::NameBuiltin),
    ("module.builtin", TokenType::NameBuiltin),
    ("function", TokenType::NameFunction),
    ("module", TokenType::NameNamespace),
    ("namespace", TokenType::NameNamespace),
    ("attribute", TokenType::NameDecorator),
    ("variable.parameter", TokenType::NameVariable),
    ("constant", TokenType::NameConstant),
    ("property", TokenType::NameAttribute),
    ("variable.member", TokenType::NameAttribute),
    ("tag.attribute", TokenType::NameAttribute),
    ("tag", TokenType::NameTag),
    ("string", TokenType::String),
    ("character", TokenType::String),
    ("number", TokenType::Number),
    ("diff.minus", TokenType::GenericDeleted),
    ("diff.plus", TokenType::GenericInserted),
    ("markup.heading", TokenType::GenericHeading),
    ("error", TokenType::Error),
];

fn token_type(scope: &str) -> TokenType {
    SCOPES
        .iter()
        .filter(|(prefix, _)| {
            scope == *prefix
                || scope
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('.'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or(TokenType::Token, |(_, token)| *token)
}

/// Upstream rich's `ANSI_DARK` / `ANSI_LIGHT`, as the syntect adapter uses them.
fn ansi_markup(token: TokenType, dark: bool) -> &'static str {
    use TokenType::*;
    match (token, dark) {
        (Token, _) => "",
        (Comment, _) => "dim",
        (CommentPreproc, true) => "bright_cyan",
        (CommentPreproc, false) => "cyan",
        (Keyword, true) => "bright_blue",
        (Keyword, false) => "blue",
        (KeywordType | NameBuiltin | NameAttribute, true) => "bright_cyan",
        (KeywordType | NameBuiltin | NameAttribute, false) => "cyan",
        (OperatorWord, true) => "bright_magenta",
        (OperatorWord, false) => "magenta",
        (NameFunction, true) => "bright_green",
        (NameFunction, false) => "green",
        (NameNamespace, true) => "bright_cyan underline",
        (NameNamespace, false) => "cyan underline",
        (NameClass, true) => "bright_green underline",
        (NameClass, false) => "green underline",
        (NameDecorator, true) => "bright_magenta bold",
        (NameDecorator, false) => "magenta bold",
        (NameVariable | NameConstant, true) => "bright_red",
        (NameVariable | NameConstant, false) => "red",
        (NameTag, _) => "bright_blue",
        (String, _) => "yellow",
        (Number, true) => "bright_blue",
        (Number, false) => "blue",
        (GenericDeleted, _) => "bright_red",
        (GenericInserted, true) => "bright_green",
        (GenericInserted, false) => "green",
        (GenericHeading, _) => "bold",
        (Error, _) => "red underline",
    }
}

fn ansi_style(scope: &str, dark: bool) -> Style {
    Style::parse(ansi_markup(token_type(scope), dark)).expect("valid style")
}

/// Collects token styles into per-line spans.
struct Lines<'a> {
    code: &'a str,
    /// Byte offset where each line of `code.split('\n')` starts.
    starts: Vec<usize>,
    lines: Vec<HighlightedLine>,
}

impl<'a> Lines<'a> {
    fn new(code: &'a str) -> Self {
        let mut starts = vec![0];
        starts.extend(code.match_indices('\n').map(|(at, _)| at + 1));
        let lines = vec![HighlightedLine::default(); starts.len()];
        Lines {
            code,
            starts,
            lines,
        }
    }

    /// The line holding byte `at`.
    fn line_of(&self, at: usize) -> usize {
        self.starts.partition_point(|&start| start <= at) - 1
    }

    /// Style `range` of the code, splitting it at line breaks.
    fn push(&mut self, range: Range<usize>, style: &Style) {
        if range.is_empty() || style.is_null() || range.end > self.code.len() {
            return;
        }
        for line in self.line_of(range.start)..=self.line_of(range.end - 1) {
            let begin = self.starts[line];
            // The end of the line's text, before its '\n'.
            let end = self
                .starts
                .get(line + 1)
                .map_or(self.code.len(), |next| next - 1);
            let (from, to) = (range.start.max(begin), range.end.min(end));
            if from < to {
                self.lines[line].spans.push(HighlightSpan {
                    range: from - begin..to - begin,
                    style: style.clone(),
                });
            }
            if end < self.code.len() && range.start <= end && end < range.end {
                self.lines[line].newline_style = Some(style.clone());
            }
        }
    }
}

impl CodeHighlighter for LumisHighlighter {
    fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme: &str,
    ) -> Result<HighlightedCode, HighlightError> {
        let palette = palette(theme)?;
        let language = match language {
            // An unknown name highlights as plain text rather than guessing
            // from the content.
            Some(name) => Language::guess(Some(name), ""),
            None => Language::PlainText,
        };
        let (theme, background, base) = match palette {
            Palette::Theme(theme) => {
                let background = color(theme.bg());
                let mut base = Style::new();
                if let Some(fg) = color(theme.fg()) {
                    base = base.with_color(fg);
                }
                if let Some(bg) = &background {
                    base = base.with_bgcolor(bg.clone());
                }
                (Some(*theme), background, Some(base))
            }
            Palette::Ansi(dark) => {
                let mut lines = Lines::new(code);
                highlight_iter(code, language, None, |_, _, range, scope, _| {
                    lines.push(range, &ansi_style(scope, dark));
                    Ok::<(), std::convert::Infallible>(())
                })
                .map_err(|error| HighlightError::Engine(error.to_string()))?;
                return Ok(HighlightedCode {
                    lines: lines.lines,
                    background: None,
                    default_style: Style::new(),
                });
            }
        };
        let base = base.unwrap_or_default();
        let mut lines = Lines::new(code);
        highlight_iter(code, language, theme, |_, _, range, scope, style| {
            // Unscoped text takes the default style, which already carries the
            // theme's colours.
            if !scope.is_empty() {
                lines.push(range, &theme_style(style, &base));
            }
            Ok::<(), std::convert::Infallible>(())
        })
        .map_err(|error| HighlightError::Engine(error.to_string()))?;
        Ok(HighlightedCode {
            lines: lines.lines,
            background,
            default_style: base,
        })
    }

    fn default_theme(&self) -> &str {
        DEFAULT_THEME
    }

    fn themes(&self) -> Vec<String> {
        let mut names: Vec<String> = themes::available_themes()
            .map(|theme| theme.name.clone())
            .collect();
        names.extend(["ansi_dark".to_string(), "ansi_light".to_string()]);
        names.sort();
        names
    }

    fn languages(&self) -> Vec<String> {
        available_languages()
            .into_iter()
            .map(|language| language.id.to_string())
            .collect()
    }

    fn language_for_path(&self, path: &Path) -> Option<String> {
        language_id_for_filename(&path.to_string_lossy())
            .filter(|id| *id != "plaintext")
            .map(str::to_string)
    }
}

/// Registers [`LumisHighlighter`] as the code highlighter `"lumis"`.
#[derive(Clone, Copy, Debug, Default)]
pub struct LumisPlugin;

impl Plugin for LumisPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new(
            "lumis",
            "lumis syntax highlighter",
            env!("CARGO_PKG_VERSION"),
        )
        .description("tree-sitter highlighting with Neovim themes")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.code_highlighter("lumis", LumisHighlighter::shared());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_map_to_upstream_token_types() {
        assert_eq!(token_type("keyword"), TokenType::Keyword);
        assert_eq!(token_type("keyword.function"), TokenType::Keyword);
        assert_eq!(token_type("keyword.operator"), TokenType::OperatorWord);
        assert_eq!(
            token_type("keyword.directive.define"),
            TokenType::CommentPreproc
        );
        assert_eq!(token_type("comment.documentation"), TokenType::Comment);
        assert_eq!(token_type("function.builtin"), TokenType::NameBuiltin);
        assert_eq!(token_type("function.method.call"), TokenType::NameFunction);
        assert_eq!(token_type("string.escape"), TokenType::String);
        assert_eq!(token_type("punctuation.bracket"), TokenType::Token);
        assert_eq!(token_type("variable"), TokenType::Token);
        // A prefix only matches whole components.
        assert_eq!(token_type("keywordish"), TokenType::Token);
        assert_eq!(token_type(""), TokenType::Token);
    }

    #[test]
    fn spans_split_at_line_breaks() {
        let code = "ab\ncd\n";
        let mut lines = Lines::new(code);
        let bold = Style::parse("bold").unwrap();
        lines.push(1..5, &bold);
        assert_eq!(lines.lines.len(), 3);
        assert_eq!(lines.lines[0].spans[0].range, 1..2);
        assert_eq!(lines.lines[0].newline_style, Some(bold.clone()));
        assert_eq!(lines.lines[1].spans[0].range, 0..2);
        assert_eq!(lines.lines[1].newline_style, None);
        assert!(lines.lines[2].spans.is_empty());
        // Out-of-range and empty ranges are ignored.
        lines.push(4..40, &bold);
        lines.push(2..2, &bold);
        assert_eq!(lines.lines[1].spans.len(), 1);
    }
}

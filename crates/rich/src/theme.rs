//! Themes — named styles.
//!
//! Port of upstream `rich/theme.py` + `rich/default_styles.py`. A [`Theme`] maps
//! style names (e.g. `"repr.number"`, `"markdown.h1"`) to [`Style`]s so that
//! markup tags and highlighters can refer to styles by name.
//!
//! [`DEFAULT_STYLES`] is the complete upstream table — it is the single source
//! of truth for named styles in this crate; nothing else should keep its own
//! name→style map.

use std::collections::HashMap;

use crate::errors::Result;
use crate::style::StyleType;

use crate::style::Style;

/// Upstream's `rich.default_styles.DEFAULT_STYLES`, verbatim.
///
/// Captured from real rich 15.0.0 (`str(style)` for each entry), so the specs
/// are exactly what upstream parses. `theme_covers_upstream` asserts the count,
/// and `every_default_style_parses` asserts we can actually parse all of them —
/// a spec this crate's `Style::parse` cannot handle would otherwise be dropped
/// silently and leave a named style resolving to nothing.
pub const DEFAULT_STYLES: &[(&str, &str)] = &[
    ("none", "none"),
    (
        "reset",
        "not bold not dim not italic not underline not blink not blink2 \
         not reverse not conceal not strike default on default",
    ),
    ("dim", "dim"),
    ("bright", "not dim"),
    ("bold", "bold"),
    ("strong", "bold"),
    ("code", "bold reverse"),
    ("italic", "italic"),
    ("emphasize", "italic"),
    ("underline", "underline"),
    ("blink", "blink"),
    ("blink2", "blink2"),
    ("reverse", "reverse"),
    ("strike", "strike"),
    ("black", "black"),
    ("red", "red"),
    ("green", "green"),
    ("yellow", "yellow"),
    ("magenta", "magenta"),
    ("cyan", "cyan"),
    ("white", "white"),
    ("inspect.attr", "italic yellow"),
    ("inspect.attr.dunder", "dim italic yellow"),
    ("inspect.callable", "bold red"),
    ("inspect.async_def", "italic bright_cyan"),
    ("inspect.def", "italic bright_cyan"),
    ("inspect.class", "italic bright_cyan"),
    ("inspect.error", "bold red"),
    ("inspect.equals", "none"),
    ("inspect.help", "cyan"),
    ("inspect.doc", "dim"),
    ("inspect.value.border", "green"),
    ("live.ellipsis", "bold red"),
    ("layout.tree.row", "not dim red"),
    ("layout.tree.column", "not dim blue"),
    ("logging.keyword", "bold yellow"),
    ("logging.level.notset", "dim"),
    ("logging.level.debug", "green"),
    ("logging.level.info", "blue"),
    ("logging.level.warning", "yellow"),
    ("logging.level.error", "bold red"),
    ("logging.level.critical", "bold reverse red"),
    ("log.level", "none"),
    ("log.time", "dim cyan"),
    ("log.message", "none"),
    ("log.path", "dim"),
    ("repr.ellipsis", "yellow"),
    ("repr.indent", "dim green"),
    ("repr.error", "bold red"),
    ("repr.str", "not bold not italic green"),
    ("repr.brace", "bold"),
    ("repr.comma", "bold"),
    ("repr.ipv4", "bold bright_green"),
    ("repr.ipv6", "bold bright_green"),
    ("repr.eui48", "bold bright_green"),
    ("repr.eui64", "bold bright_green"),
    ("repr.tag_start", "bold"),
    ("repr.tag_name", "bold bright_magenta"),
    ("repr.tag_contents", "default"),
    ("repr.tag_end", "bold"),
    ("repr.attrib_name", "not italic yellow"),
    ("repr.attrib_equal", "bold"),
    ("repr.attrib_value", "not italic magenta"),
    ("repr.number", "bold not italic cyan"),
    ("repr.number_complex", "bold not italic cyan"),
    ("repr.bool_true", "italic bright_green"),
    ("repr.bool_false", "italic bright_red"),
    ("repr.none", "italic magenta"),
    ("repr.url", "not bold not italic underline bright_blue"),
    ("repr.uuid", "not bold bright_yellow"),
    ("repr.call", "bold magenta"),
    ("repr.path", "magenta"),
    ("repr.filename", "bright_magenta"),
    ("rule.line", "bright_green"),
    ("rule.text", "none"),
    ("json.brace", "bold"),
    ("json.bool_true", "italic bright_green"),
    ("json.bool_false", "italic bright_red"),
    ("json.null", "italic magenta"),
    ("json.number", "bold not italic cyan"),
    ("json.str", "not bold not italic green"),
    ("json.key", "bold blue"),
    ("prompt", "none"),
    ("prompt.choices", "bold magenta"),
    ("prompt.default", "bold cyan"),
    ("prompt.invalid", "red"),
    ("prompt.invalid.choice", "red"),
    ("pretty", "none"),
    ("scope.border", "blue"),
    ("scope.key", "italic yellow"),
    ("scope.key.special", "dim italic yellow"),
    ("scope.equals", "red"),
    ("table.header", "bold"),
    ("table.footer", "bold"),
    ("table.cell", "none"),
    ("table.title", "italic"),
    ("table.caption", "dim italic"),
    ("traceback.error", "italic red"),
    ("traceback.border.syntax_error", "bright_red"),
    ("traceback.border", "red"),
    ("traceback.text", "none"),
    ("traceback.title", "bold red"),
    ("traceback.exc_type", "bold bright_red"),
    ("traceback.exc_value", "none"),
    ("traceback.offset", "bold bright_red"),
    ("traceback.error_range", "bold underline"),
    ("traceback.note", "bold green"),
    ("traceback.group.border", "magenta"),
    ("bar.back", "grey23"),
    ("bar.complete", "rgb(249,38,114)"),
    ("bar.finished", "rgb(114,156,31)"),
    ("bar.pulse", "rgb(249,38,114)"),
    ("progress.description", "none"),
    ("progress.filesize", "green"),
    ("progress.filesize.total", "green"),
    ("progress.download", "green"),
    ("progress.elapsed", "yellow"),
    ("progress.percentage", "magenta"),
    ("progress.remaining", "cyan"),
    ("progress.data.speed", "red"),
    ("progress.spinner", "green"),
    ("status.spinner", "green"),
    ("tree", "none"),
    ("tree.line", "none"),
    ("markdown.paragraph", "none"),
    ("markdown.text", "none"),
    ("markdown.em", "italic"),
    ("markdown.emph", "italic"),
    ("markdown.strong", "bold"),
    ("markdown.code", "bold cyan on black"),
    ("markdown.code_block", "cyan on black"),
    ("markdown.block_quote", "magenta"),
    ("markdown.list", "cyan"),
    ("markdown.item", "none"),
    ("markdown.item.bullet", "bold"),
    ("markdown.item.number", "cyan"),
    ("markdown.hr", "dim"),
    ("markdown.h1.border", "none"),
    ("markdown.h1", "bold underline"),
    ("markdown.h2", "underline magenta"),
    ("markdown.h3", "bold magenta"),
    ("markdown.h4", "italic magenta"),
    ("markdown.h5", "italic"),
    ("markdown.h6", "dim"),
    ("markdown.h7", "dim italic"),
    ("markdown.link", "bright_blue"),
    ("markdown.link_url", "underline blue"),
    ("markdown.s", "strike"),
    ("markdown.table.border", "cyan"),
    ("markdown.table.header", "not bold cyan"),
    ("markdown.kbd", "bold bright_yellow"),
    ("iso8601.date", "blue"),
    ("iso8601.time", "magenta"),
    ("iso8601.timezone", "yellow"),
];

/// A named collection of styles. Mirrors `rich.theme.Theme`.
#[derive(Debug, Clone, Default)]
pub struct Theme {
    styles: HashMap<String, Style>,
}

impl Theme {
    pub fn new() -> Self {
        Theme::default()
    }

    /// Look up a style by name.
    pub fn get(&self, name: &str) -> Option<&Style> {
        self.styles.get(name)
    }

    /// Insert or replace a named style.
    pub fn insert(&mut self, name: impl Into<String>, style: Style) {
        self.styles.insert(name.into(), style);
    }

    /// Resolve a [`StyleType`] against this theme. Port of `Console.get_style`.
    ///
    /// An already-resolved style passes straight through. A name is looked up in
    /// the theme **first**, and only then parsed as a style definition — the
    /// order matters, because the default theme itself defines bare words like
    /// `none`, `bold` and `red`, and a custom theme has to be able to shadow
    /// them.
    ///
    /// The lookup is case-sensitive while the parse fallback is not, which
    /// reproduces an asymmetry upstream really has: `"BOLD"` misses the theme but
    /// still parses to bold, whereas a theme key `"Danger"` is never found by a
    /// span naming `"danger"`.
    pub fn get_style(&self, style: &StyleType) -> Result<Style> {
        match style {
            StyleType::Style(style) => Ok(style.clone()),
            StyleType::Name(name) => match self.styles.get(name) {
                Some(style) => Ok(style.clone()),
                None => Style::parse(name),
            },
        }
    }

    /// As [`get_style`](Self::get_style), but an unresolvable name yields the
    /// null style instead of an error. Port of upstream's
    /// `get_style(..., default=Style.null())`, which is what the render path
    /// uses — an unknown name must not blow up a print.
    pub fn get_style_or_null(&self, style: &StyleType) -> Style {
        self.get_style(style).unwrap_or_default()
    }

    /// How many named styles this theme holds.
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Whether this theme holds no styles.
    pub fn is_empty(&self) -> bool {
        self.styles.is_empty()
    }

    /// The complete upstream default theme — every entry of [`DEFAULT_STYLES`].
    pub fn default_theme() -> Self {
        let mut theme = Theme::new();
        for (name, spec) in DEFAULT_STYLES {
            match Style::parse(spec) {
                Ok(style) => theme.insert(*name, style),
                // Unreachable in practice: `every_default_style_parses` fails
                // the build if a spec stops parsing. Skipping keeps a bad spec
                // from poisoning every other named style at runtime.
                Err(_) => continue,
            }
        }
        theme
    }

    /// The shared default theme, for callers that only need to resolve a name
    /// (e.g. the built-in highlighters) and have no `Console` to hand.
    pub fn default_shared() -> &'static Theme {
        static DEFAULT: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        DEFAULT.get_or_init(Theme::default_theme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The theme is consulted *before* the style parser. The default theme
    /// defines bare words like `red` itself, so parse-first would make a custom
    /// theme unable to shadow them.
    ///
    /// Verified against real rich 15.0.0: with `Theme({"red": "blue"})`,
    /// `Console.get_style("red")` returns blue.
    #[test]
    fn theme_lookup_beats_the_style_parser() {
        let mut theme = Theme::default_theme();
        theme.insert("red", Style::parse("blue").unwrap());
        assert_eq!(
            theme.get_style(&StyleType::Name("red".into())).unwrap(),
            Style::parse("blue").unwrap()
        );
    }

    /// Upstream's case handling is asymmetric, and this pins it: the theme
    /// lookup is case-sensitive, but the parse fallback is not. So `"BOLD"`
    /// misses the theme and still parses to bold, while a theme key `"Danger"`
    /// is never found by a span naming `"danger"`.
    #[test]
    fn lookup_is_case_sensitive_but_parsing_is_not() {
        let mut theme = Theme::new();
        theme.insert("Danger", Style::parse("bold red").unwrap());

        assert_eq!(
            theme.get_style(&StyleType::Name("BOLD".into())).unwrap(),
            Style::parse("bold").unwrap()
        );
        // "danger" is not a style definition either, so it does not resolve.
        assert!(theme.get_style(&StyleType::Name("danger".into())).is_err());
        assert_eq!(
            theme.get_style(&StyleType::Name("Danger".into())).unwrap(),
            Style::parse("bold red").unwrap()
        );
    }

    /// The parse fallback inherits `Style::parse`'s single-letter aliases.
    #[test]
    fn parse_fallback_understands_aliases() {
        let theme = Theme::new();
        assert_eq!(
            theme.get_style(&StyleType::Name("b".into())).unwrap(),
            Style::parse("bold").unwrap()
        );
    }

    /// An unknown name is an error from `get_style` and the null style from
    /// `get_style_or_null` — the render path uses the latter so a typo cannot
    /// blow up a print.
    #[test]
    fn unknown_names_error_but_render_null() {
        let theme = Theme::default_theme();
        let unknown = StyleType::Name("repr.nope".into());
        assert!(theme.get_style(&unknown).is_err());
        assert!(theme.get_style_or_null(&unknown).is_null());
    }

    /// An already-resolved style passes through untouched, theme or no theme.
    #[test]
    fn resolved_styles_pass_through() {
        let mut theme = Theme::new();
        theme.insert("bold", Style::parse("red").unwrap());
        let style = Style::parse("bold").unwrap();
        assert_eq!(
            theme.get_style(&StyleType::Style(style.clone())).unwrap(),
            style
        );
    }

    #[test]
    fn theme_covers_upstream() {
        // rich 15.0.0 ships exactly this many named styles.
        assert_eq!(DEFAULT_STYLES.len(), 154);
        // No duplicate names (a duplicate would silently shadow).
        let mut names: Vec<&str> = DEFAULT_STYLES.iter().map(|(n, _)| *n).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(
            names.len(),
            before,
            "duplicate style names in DEFAULT_STYLES"
        );
    }

    /// Every upstream spec must parse. A failure here means `Style::parse` is
    /// missing syntax upstream uses, and that style would silently vanish from
    /// the theme rather than resolving.
    #[test]
    fn every_default_style_parses() {
        let unparsed: Vec<&str> = DEFAULT_STYLES
            .iter()
            .filter(|(_, spec)| Style::parse(spec).is_err())
            .map(|(name, _)| *name)
            .collect();
        assert!(
            unparsed.is_empty(),
            "specs that failed to parse: {unparsed:?}"
        );
        assert_eq!(Theme::default_theme().len(), DEFAULT_STYLES.len());
    }

    #[test]
    fn resolves_a_few_known_styles() {
        let theme = Theme::default_theme();
        assert_eq!(
            theme.get("repr.number"),
            Style::parse("bold not italic cyan").ok().as_ref()
        );
        assert_eq!(
            theme.get("markdown.table.header"),
            Style::parse("not bold cyan").ok().as_ref()
        );
        assert!(theme.get("no.such.style").is_none());
    }
}

//! Extra named styles layered on the upstream theme.
//!
//! The core's [`Theme::default_theme`] is a faithful port of upstream's
//! `DEFAULT_STYLES` and holds *only* those 154 entries. Semantic conveniences
//! like `[error]` are **our** additions, so they live here — see `AGENTS.md`.

use rich::style::Style;
use rich::theme::Theme;

/// Semantic style names this crate adds on top of upstream's.
///
/// Deliberately not in the core: upstream has no `error`/`warning`/`info`
/// styles, and adding them there would make the mirror diverge and every
/// upstream sync noisier.
pub const EXTRA_STYLES: &[(&str, &str)] = &[
    ("error", "bold red"),
    ("warning", "yellow"),
    ("info", "cyan"),
    ("success", "bold green"),
];

/// Upstream's default theme plus [`EXTRA_STYLES`].
///
/// Pass to `Console::builder().theme(..)` to get `[error]`-style markup:
///
/// ```
/// use rich::{ColorSystem, Console};
///
/// let console = Console::builder()
///     .force_terminal(true)
///     .color_system(Some(ColorSystem::Truecolor))
///     .theme(rich_ext::theme::extended_theme())
///     .build();
/// assert_eq!(console.render_str_to_string("[error]boom[/]"), "\u{1b}[1;31mboom\u{1b}[0m");
/// ```
pub fn extended_theme() -> Theme {
    let mut theme = Theme::default_theme();
    for (name, spec) in EXTRA_STYLES {
        if let Ok(style) = Style::parse(spec) {
            theme.insert(*name, style);
        }
    }
    theme
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extends_without_dropping_upstream_styles() {
        let base = Theme::default_theme();
        let extended = extended_theme();
        assert_eq!(extended.len(), base.len() + EXTRA_STYLES.len());
        // Upstream entries survive...
        assert!(extended.get("repr.number").is_some());
        // ...and ours are added.
        assert_eq!(
            extended.get("error"),
            Style::parse("bold red").ok().as_ref()
        );
    }

    #[test]
    fn core_theme_stays_upstream_only() {
        // Guards the governance rule: our conveniences must not leak into the
        // faithful core's theme.
        let base = Theme::default_theme();
        for (name, _) in EXTRA_STYLES {
            assert!(
                base.get(name).is_none(),
                "{name:?} leaked into the core default theme"
            );
        }
    }
}

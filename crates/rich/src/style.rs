//! Text styles.
//!
//! Port of upstream `rich/style.py` (core attributes). A [`Style`] holds an
//! optional foreground/background [`Color`] plus a set of boolean attributes
//! (bold, italic, …). Each attribute is tri-state: `Some(true)` = on,
//! `Some(false)` = explicitly off, `None` = unset — this preserves upstream's
//! `_set_attributes`/`_attributes` bitmask semantics under [`Style::combine`].

use crate::color::{Color, ColorSystem};
use crate::errors::{Result, RichError};

/// The 13 boolean attributes, in the SGR order upstream emits them.
const ATTR_COUNT: usize = 13;

/// SGR codes per attribute index (`rich.style._STYLE_MAP`).
const ATTR_SGR: [&str; ATTR_COUNT] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "21", "51", "52", "53",
];

/// Canonical attribute names per index.
const ATTR_NAMES: [&str; ATTR_COUNT] = [
    "bold",
    "dim",
    "italic",
    "underline",
    "blink",
    "blink2",
    "reverse",
    "conceal",
    "strike",
    "underline2",
    "frame",
    "encircle",
    "overline",
];

/// Map a style word (including upstream's short aliases) to its attribute index.
fn attribute_index(word: &str) -> Option<usize> {
    let canonical = match word {
        "b" => "bold",
        "d" => "dim",
        "i" => "italic",
        "u" => "underline",
        "r" => "reverse",
        "c" => "conceal",
        "s" => "strike",
        "uu" => "underline2",
        "o" => "overline",
        other => other,
    };
    ATTR_NAMES.iter().position(|&n| n == canonical)
}

/// A terminal text style. Mirrors `rich.style.Style`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style {
    color: Option<Color>,
    bgcolor: Option<Color>,
    attrs: [Option<bool>; ATTR_COUNT],
}

impl Style {
    /// The empty (null) style — sets nothing.
    pub fn new() -> Self {
        Style::default()
    }

    pub fn with_color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    pub fn with_bgcolor(mut self, color: Color) -> Self {
        self.bgcolor = Some(color);
        self
    }

    pub fn color(&self) -> Option<&Color> {
        self.color.as_ref()
    }

    pub fn bgcolor(&self) -> Option<&Color> {
        self.bgcolor.as_ref()
    }

    /// True when nothing at all is set (renders as a no-op).
    pub fn is_null(&self) -> bool {
        self.color.is_none() && self.bgcolor.is_none() && self.attrs.iter().all(Option::is_none)
    }

    /// Parse a style definition such as `"bold red on blue"`.
    ///
    /// Port of `Style.parse` covering attributes, `not <attr>`, and
    /// `<color> on <color>`. (`link`/`meta` are deferred — see DIVERGENCES.)
    pub fn parse(definition: &str) -> Result<Self> {
        let mut style = Style::new();
        let mut words = definition.split_whitespace();
        while let Some(raw) = words.next() {
            let word = raw.to_ascii_lowercase();
            match word.as_str() {
                "on" => {
                    let color_word = words.next().ok_or_else(|| {
                        RichError::StyleSyntax("color expected after 'on'".to_string())
                    })?;
                    style.bgcolor = Some(Color::parse(color_word)?);
                }
                "not" => {
                    let attr_word = words.next().ok_or_else(|| {
                        RichError::StyleSyntax("attribute expected after 'not'".to_string())
                    })?;
                    let idx =
                        attribute_index(&attr_word.to_ascii_lowercase()).ok_or_else(|| {
                            RichError::StyleSyntax(format!(
                                "{attr_word:?} is not a recognized attribute"
                            ))
                        })?;
                    style.attrs[idx] = Some(false);
                }
                _ => {
                    if let Some(idx) = attribute_index(&word) {
                        style.attrs[idx] = Some(true);
                    } else {
                        style.color = Some(Color::parse(&word)?);
                    }
                }
            }
        }
        Ok(style)
    }

    /// Combine two styles, `other` winning wherever it sets a value.
    ///
    /// Port of `Style.__add__`.
    pub fn combine(&self, other: &Style) -> Style {
        let mut attrs = self.attrs;
        for (slot, over) in attrs.iter_mut().zip(other.attrs.iter()) {
            if over.is_some() {
                *slot = *over;
            }
        }
        Style {
            color: other.color.clone().or_else(|| self.color.clone()),
            bgcolor: other.bgcolor.clone().or_else(|| self.bgcolor.clone()),
            attrs,
        }
    }

    /// The SGR parameter list (e.g. `"1;31;44"`) for a given color system.
    ///
    /// Port of `Style._make_ansi_codes`.
    pub fn ansi_codes(&self, system: ColorSystem) -> String {
        let mut sgr: Vec<String> = Vec::new();
        for (idx, attr) in self.attrs.iter().enumerate() {
            if *attr == Some(true) {
                sgr.push(ATTR_SGR[idx].to_string());
            }
        }
        if let Some(color) = &self.color {
            sgr.extend(color.downgrade(system).ansi_codes(true));
        }
        if let Some(bgcolor) = &self.bgcolor {
            sgr.extend(bgcolor.downgrade(system).ansi_codes(false));
        }
        sgr.join(";")
    }

    /// Wrap `text` in this style's escape sequence for `system`.
    ///
    /// With `system == None` (no color) or a null style, `text` is returned
    /// unchanged. Port of `Style.render`.
    pub fn render(&self, text: &str, system: Option<ColorSystem>) -> String {
        let Some(system) = system else {
            return text.to_string();
        };
        if text.is_empty() {
            return text.to_string();
        }
        let codes = self.ansi_codes(system);
        if codes.is_empty() {
            text.to_string()
        } else {
            format!("\x1b[{codes}m{text}\x1b[0m")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bold_red() {
        let style = Style::parse("bold red").unwrap();
        assert_eq!(style.ansi_codes(ColorSystem::Truecolor), "1;31");
        assert_eq!(
            style.render("hello", Some(ColorSystem::Truecolor)),
            "\x1b[1;31mhello\x1b[0m"
        );
    }

    #[test]
    fn parse_fg_on_bg() {
        let style = Style::parse("white on blue").unwrap();
        assert_eq!(style.ansi_codes(ColorSystem::Truecolor), "37;44");
    }

    #[test]
    fn combine_overrides() {
        let base = Style::parse("bold red").unwrap();
        let over = Style::parse("blue").unwrap();
        let combined = base.combine(&over);
        // bold retained from base, color replaced by blue (34)
        assert_eq!(combined.ansi_codes(ColorSystem::Truecolor), "1;34");
    }

    #[test]
    fn no_color_system_is_plaintext() {
        let style = Style::parse("bold red").unwrap();
        assert_eq!(style.render("hello", None), "hello");
    }

    #[test]
    fn null_style_does_not_wrap() {
        let style = Style::new();
        assert_eq!(style.render("hello", Some(ColorSystem::Truecolor)), "hello");
    }
}

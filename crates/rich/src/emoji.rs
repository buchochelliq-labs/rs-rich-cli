//! Emoji shortcode replacement.
//!
//! Port of upstream `rich/_emoji_replace.py` + the full `rich/_emoji_codes.py`
//! table (see `emoji_codes.rs`). [`replace`] turns `:name:` shortcodes into
//! emoji, with optional `:name-emoji:` / `:name-text:` variant selectors.
//! Unknown codes are left untouched (matching upstream). [`Emoji`] is
//! upstream's `rich.emoji.Emoji` renderable.

use crate::console::{Console, ConsoleOptions};
use crate::protocol::Renderable;
use crate::segment::Segment;
use crate::style::StyleType;

const VARIANT_EMOJI: &str = "\u{fe0f}";
const VARIANT_TEXT: &str = "\u{fe0e}";

/// An emoji presentation variant. Mirrors `rich.emoji.EmojiVariant`
/// (`"emoji"` or `"text"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmojiVariant {
    /// Emoji presentation: appends U+FE0F.
    Emoji,
    /// Text presentation: appends U+FE0E.
    Text,
}

impl EmojiVariant {
    /// Parse upstream's variant name (`"emoji"` or `"text"`).
    pub fn parse(name: &str) -> Option<EmojiVariant> {
        match name {
            "emoji" => Some(EmojiVariant::Emoji),
            "text" => Some(EmojiVariant::Text),
            _ => None,
        }
    }

    /// The variant's name, as upstream spells it.
    pub fn name(self) -> &'static str {
        match self {
            EmojiVariant::Emoji => "emoji",
            EmojiVariant::Text => "text",
        }
    }

    /// The variation selector this variant appends.
    fn selector(self) -> &'static str {
        match self {
            EmojiVariant::Emoji => VARIANT_EMOJI,
            EmojiVariant::Text => VARIANT_TEXT,
        }
    }
}

/// Replace `:name:` emoji shortcodes in `text`. Port of `_emoji_replace`.
///
/// Mirrors one `re.sub` over `(:(\S*?)(?:(?:\-)(emoji|text))?:)`: every match,
/// including an unknown code or an empty `::`, is consumed, and scanning
/// resumes after its closing colon.
pub fn replace(text: &str) -> String {
    replace_with_variant(text, None)
}

/// [`replace`] with upstream's `default_variant`: the selector appended to a
/// code that names no variant of its own. Port of
/// `_emoji_replace(text, default_variant=…)`.
pub fn replace_with_variant(text: &str, default_variant: Option<EmojiVariant>) -> String {
    // Every code is between colons: text without one comes back as it is,
    // without the scan (docs/PORTING.md, "Performance notes").
    if !text.contains(':') {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;

    while index < len {
        if chars[index] == ':' {
            // The lazy `\S*?` stops at the first ':' that follows only
            // non-whitespace characters.
            let mut end = index + 1;
            while end < len && !is_python_space(chars[end]) && chars[end] != ':' {
                end += 1;
            }
            if end < len && chars[end] == ':' {
                let body: String = chars[index + 1..end].iter().collect();
                match lookup(&body, default_variant) {
                    Some(replacement) => out.push_str(&replacement),
                    None => out.extend(&chars[index..=end]),
                }
                index = end + 1;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// No emoji by that name. Port of `rich.emoji.NoEmoji`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoEmoji(pub String);

impl std::fmt::Display for NoEmoji {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoEmoji {}

/// A single emoji character, by name. Port of `rich.emoji.Emoji`.
#[derive(Debug, Clone)]
pub struct Emoji {
    name: String,
    style: StyleType,
    variant: Option<EmojiVariant>,
    character: String,
}

impl Emoji {
    /// The emoji called `name` (exactly, as upstream's `EMOJI[name]`), or
    /// [`NoEmoji`].
    pub fn new(name: &str) -> Result<Emoji, NoEmoji> {
        let character = emoji_code(name)
            .ok_or_else(|| NoEmoji(format!("No emoji called {}", crate::pretty::str_repr(name))))?;
        Ok(Emoji {
            name: name.to_string(),
            style: StyleType::from("none"),
            variant: None,
            character: character.to_string(),
        })
    }

    /// The style of the emoji (upstream `style`, default `"none"`).
    pub fn style(mut self, style: impl Into<StyleType>) -> Self {
        self.style = style.into();
        self
    }

    /// Append the variant's selector (upstream `variant`).
    pub fn variant(mut self, variant: EmojiVariant) -> Self {
        if self.variant.is_none() {
            self.character.push_str(variant.selector());
        } else {
            let base = emoji_code(&self.name).unwrap_or_default();
            self.character = format!("{base}{}", variant.selector());
        }
        self.variant = Some(variant);
        self
    }

    /// The emoji's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Replace `:name:` codes in `text`. Port of `Emoji.replace`.
    pub fn replace(text: &str) -> String {
        replace(text)
    }
}

impl std::fmt::Display for Emoji {
    /// The emoji character (upstream `__str__`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.character)
    }
}

impl Renderable for Emoji {
    fn rich_render(&self, console: &Console, _options: &ConsoleOptions) -> Vec<Segment> {
        let style = console.get_style(&self.style).unwrap_or_default();
        vec![Segment::new(self.character.clone(), Some(style))]
    }
}

/// Python's `str.isspace()`, which `\s` follows for `str` patterns. Unlike
/// `char::is_whitespace` it includes the separators U+001C..=U+001F.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Resolve a shortcode body (with optional variant suffix) to its replacement.
///
/// The suffix is matched case-sensitively, as upstream's `(emoji|text)` group
/// is; only the name is lowercased (`get_emoji(emoji_name.lower())`).
fn lookup(body: &str, default_variant: Option<EmojiVariant>) -> Option<String> {
    let default = default_variant.map_or("", EmojiVariant::selector);
    let (name, variant) = if let Some(name) = body.strip_suffix("-emoji") {
        (name, VARIANT_EMOJI)
    } else if let Some(name) = body.strip_suffix("-text") {
        (name, VARIANT_TEXT)
    } else {
        (body, default)
    };
    emoji_code(&name.to_lowercase()).map(|code| format!("{code}{variant}"))
}

/// Resolve an emoji name to its glyph(s) from the full vendored table.
fn emoji_code(name: &str) -> Option<&'static str> {
    crate::emoji_codes::emoji_code(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_known_codes() {
        // Captured from real rich 15.0.0 `_emoji_replace`.
        assert_eq!(replace(":rocket:"), "\u{1f680}");
        assert_eq!(replace("hello :thumbs_up: world"), "hello \u{1f44d} world");
        assert_eq!(replace(":rocket: to :moon:"), "\u{1f680} to \u{1f314}");
    }

    #[test]
    fn leaves_unknown_and_bare_colons() {
        assert_eq!(replace(":not_a_real_emoji:"), ":not_a_real_emoji:");
        assert_eq!(replace("no emoji"), "no emoji");
        assert_eq!(replace("ratio 3:4 done"), "ratio 3:4 done");
    }

    #[test]
    fn emoji_renderable_matches_upstream() {
        // Captured from rich 15.0.0: `str(Emoji("rocket", variant="text"))`,
        // `NoEmoji("No emoji called 'nope'")`, and `Emoji` printed in red.
        let rocket = Emoji::new("rocket").unwrap();
        assert_eq!(rocket.to_string(), "\u{1f680}");
        assert_eq!(
            rocket.clone().variant(EmojiVariant::Text).to_string(),
            "\u{1f680}\u{fe0e}"
        );
        assert_eq!(
            Emoji::new("nope").unwrap_err().to_string(),
            "No emoji called 'nope'"
        );
        assert!(Emoji::new("Rocket").is_err());
        let console = crate::Console::builder()
            .force_terminal(true)
            .color_system(Some(crate::ColorSystem::Truecolor))
            .width(10)
            .build();
        // Upstream prints no newline after it; `print` here always ends the
        // line (DIVERGENCES §29), so compare the rendered segment.
        let out = console.render_to_string(&rocket.style("red"));
        assert_eq!(out, "\x1b[31m\u{1f680}\x1b[0m");
    }

    #[test]
    fn variant_selectors() {
        assert_eq!(replace(":rocket-emoji:"), "\u{1f680}\u{fe0f}");
        assert_eq!(replace(":rocket-text:"), "\u{1f680}\u{fe0e}");
    }

    #[test]
    fn default_variant_and_case_sensitive_suffix() {
        // Captured from real rich 15.0.0 `_emoji_replace`.
        assert_eq!(replace(":rocket-EMOJI:"), ":rocket-EMOJI:");
        assert_eq!(replace(":ROCKET-text:"), "\u{1f680}\u{fe0e}");
        assert_eq!(
            replace_with_variant(":rocket: :rocket-text:", Some(EmojiVariant::Emoji)),
            "\u{1f680}\u{fe0f} \u{1f680}\u{fe0e}"
        );
        assert_eq!(
            replace_with_variant(":rocket:", Some(EmojiVariant::Text)),
            "\u{1f680}\u{fe0e}"
        );
    }

    #[test]
    fn scanning_resumes_after_every_match() {
        // Captured from real rich 15.0.0 `_emoji_replace` (#448).
        assert_eq!(replace(" -:界[ba:b: x"), " -:界[ba:b: x");
        assert_eq!(replace("::rocket:"), "::rocket:");
        assert_eq!(replace(":x:rocket:"), "\u{274c}rocket:");
        assert_eq!(replace(":rocket:rocket:"), "\u{1f680}rocket:");
        assert_eq!(replace(":ROCKET:"), "\u{1f680}");
        assert_eq!(replace("a:\u{1c}rocket:"), "a:\u{1c}rocket:");
        assert_eq!(replace(":\u{212a}rocket:"), ":\u{212a}rocket:");
    }
}

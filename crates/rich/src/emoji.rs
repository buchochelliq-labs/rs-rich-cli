//! Emoji shortcode replacement.
//!
//! Port of upstream `rich/_emoji_replace.py` + the full `rich/_emoji_codes.py`
//! table (see `emoji_codes.rs`). [`replace`] turns `:name:` shortcodes into
//! emoji, with optional `:name-emoji:` / `:name-text:` variant selectors.
//! Unknown codes are left untouched (matching upstream).

const VARIANT_EMOJI: &str = "\u{fe0f}";
const VARIANT_TEXT: &str = "\u{fe0e}";

/// Replace `:name:` emoji shortcodes in `text`. Port of `_emoji_replace`.
pub fn replace(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;

    while index < len {
        if chars[index] == ':' {
            // Scan a non-whitespace, colon-free body up to the closing ':'.
            let mut end = index + 1;
            while end < len && !chars[end].is_whitespace() && chars[end] != ':' {
                end += 1;
            }
            if end < len && chars[end] == ':' && end > index + 1 {
                let body: String = chars[index + 1..end].iter().collect();
                if let Some(replacement) = lookup(&body) {
                    out.push_str(&replacement);
                    index = end + 1;
                    continue;
                }
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// Resolve a shortcode body (with optional variant suffix) to its replacement.
fn lookup(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let (name, variant) = if let Some(name) = lower.strip_suffix("-emoji") {
        (name, VARIANT_EMOJI)
    } else if let Some(name) = lower.strip_suffix("-text") {
        (name, VARIANT_TEXT)
    } else {
        (lower.as_str(), "")
    };
    emoji_code(name).map(|code| format!("{code}{variant}"))
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
    fn variant_selectors() {
        assert_eq!(replace(":rocket-emoji:"), "\u{1f680}\u{fe0f}");
        assert_eq!(replace(":rocket-text:"), "\u{1f680}\u{fe0e}");
    }
}

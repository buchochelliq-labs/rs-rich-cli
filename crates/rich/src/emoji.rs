//! Emoji shortcode replacement.
//!
//! Port of upstream `rich/_emoji_replace.py` + a curated subset of
//! `rich/_emoji_codes.py`. [`replace`] turns `:name:` shortcodes into emoji,
//! with optional `:name-emoji:` / `:name-text:` variant selectors.
//!
//! Slice scope: a curated subset of the ~3600 upstream codes. Unknown codes are
//! left untouched (matching upstream). See docs/DIVERGENCES.md.

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

/// A curated subset of `_emoji_codes.EMOJI`. Codepoints captured from real rich.
fn emoji_code(name: &str) -> Option<&'static str> {
    Some(match name {
        "rocket" => "\u{1f680}",
        "fire" => "\u{1f525}",
        "star" => "\u{2b50}",
        "sparkles" => "\u{2728}",
        "heart" => "\u{2764}",
        "thumbs_up" => "\u{1f44d}",
        "thumbs_down" => "\u{1f44e}",
        "warning" => "\u{26a0}",
        "white_check_mark" => "\u{2705}",
        "cross_mark" | "x" => "\u{274c}",
        "tada" => "\u{1f389}",
        "eyes" => "\u{1f440}",
        "bug" => "\u{1f41b}",
        "snake" => "\u{1f40d}",
        "crab" => "\u{1f980}",
        "package" => "\u{1f4e6}",
        "gear" => "\u{2699}",
        "books" => "\u{1f4da}",
        "memo" => "\u{1f4dd}",
        "bulb" => "\u{1f4a1}",
        "zap" => "\u{26a1}",
        "lock" => "\u{1f512}",
        "key" => "\u{1f511}",
        "hammer" => "\u{1f528}",
        "wrench" => "\u{1f527}",
        "rainbow" => "\u{1f308}",
        "sun" => "\u{2600}",
        "moon" => "\u{1f314}",
        "cloud" => "\u{2601}",
        "snowflake" => "\u{2744}",
        "coffee" => "\u{2615}",
        "beer" => "\u{1f37a}",
        "pizza" => "\u{1f355}",
        "apple" => "\u{1f34e}",
        "robot" => "\u{1f916}",
        "alien" => "\u{1f47d}",
        "ghost" => "\u{1f47b}",
        "skull" => "\u{1f480}",
        "100" => "\u{1f4af}",
        _ => return None,
    })
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

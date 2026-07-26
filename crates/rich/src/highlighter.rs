//! Built-in highlighters.
//!
//! Port of upstream `rich/highlighter.py` (`ReprHighlighter`). The patterns
//! (vendored in `repr_patterns.rs`) use lookbehind/alternation, so we compile
//! them with `fancy-regex`.

use std::sync::OnceLock;

use fancy_regex::Regex;

use crate::protocol::Highlighter;
use crate::repr_patterns::REPR_PATTERNS;
use crate::style::Style;
use crate::text::Text;

/// Highlights repr-style output — numbers, strings, bools, `None`, paths, URLs,
/// braces, calls, IP/UUID/EUI, and tags. Mirrors `rich.highlighter.ReprHighlighter`.
#[derive(Default)]
pub struct ReprHighlighter;

impl ReprHighlighter {
    pub fn new() -> Self {
        ReprHighlighter
    }
}

/// Compile the patterns once, globally.
fn patterns() -> &'static [Regex] {
    static COMPILED: OnceLock<Vec<Regex>> = OnceLock::new();
    COMPILED.get_or_init(|| {
        REPR_PATTERNS
            .iter()
            .map(|pattern| Regex::new(pattern).expect("valid repr pattern"))
            .collect()
    })
}

/// The `repr.<group>` style for a capture-group name (from `default_styles.py`).
fn group_style(name: &str) -> Option<Style> {
    let spec = match name {
        "tag_start" | "tag_end" | "brace" => "bold",
        "tag_name" => "bold bright_magenta",
        "tag_contents" => "default",
        "attrib_name" => "not italic yellow",
        "attrib_value" => "not italic magenta",
        "ipv4" | "ipv6" | "eui48" | "eui64" => "bold bright_green",
        "uuid" => "not bold bright_yellow",
        "call" => "bold magenta",
        "bool_true" => "italic bright_green",
        "bool_false" => "italic bright_red",
        "none" => "italic magenta",
        "ellipsis" => "yellow",
        "number" | "number_complex" => "bold not italic cyan",
        "path" => "magenta",
        "filename" => "bright_magenta",
        "str" => "not bold not italic green",
        "url" => "not bold not italic underline bright_blue",
        _ => return None,
    };
    Style::parse(spec).ok()
}

impl Highlighter for ReprHighlighter {
    fn highlight(&self, text: &mut Text) {
        let plain = text.plain().to_string();
        for regex in patterns() {
            let names: Vec<&str> = regex.capture_names().flatten().collect();
            for captures in regex.captures_iter(&plain) {
                let Ok(captures) = captures else { break };
                for &name in &names {
                    if let Some(matched) = captures.name(name) {
                        if let Some(style) = group_style(name) {
                            text.stylize(matched.start(), matched.end(), style);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::console::Console;

    fn highlight(input: &str) -> String {
        let mut text = Text::new(input);
        ReprHighlighter::new().highlight(&mut text);
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .build();
        console.render_to_string(&text)
    }

    #[test]
    fn highlights_numbers_bools_none() {
        // Captured from real rich 15.0.0 Console(highlight=True).
        assert_eq!(
            highlight("value = 42 and 3.14"),
            "value = \x1b[1;36m42\x1b[0m and \x1b[1;36m3.14\x1b[0m"
        );
        assert_eq!(
            highlight("flag is True, x is None"),
            "flag is \x1b[3;92mTrue\x1b[0m, x is \x1b[3;35mNone\x1b[0m"
        );
    }

    #[test]
    fn highlights_paths_strings_urls_and_more() {
        // All captured from real rich 15.0.0.
        assert_eq!(
            highlight("path /usr/bin done"),
            "path \x1b[35m/usr/\x1b[0m\x1b[95mbin\x1b[0m done"
        );
        assert_eq!(
            highlight("s = 'hello world' end"),
            "s = \x1b[32m'hello world'\x1b[0m end"
        );
        assert_eq!(
            highlight("see https://example.com/x now"),
            "see \x1b[4;94mhttps://example.com/x\x1b[0m now"
        );
        assert_eq!(
            highlight("list [1, 2, 3] and (a, b)"),
            "list \x1b[1m[\x1b[0m\x1b[1;36m1\x1b[0m, \x1b[1;36m2\x1b[0m, \x1b[1;36m3\x1b[0m\x1b[1m]\x1b[0m and \x1b[1m(\x1b[0ma, b\x1b[1m)\x1b[0m"
        );
        assert_eq!(
            highlight("call func(x) here"),
            "call \x1b[1;35mfunc\x1b[0m\x1b[1m(\x1b[0mx\x1b[1m)\x1b[0m here"
        );
        assert_eq!(
            highlight("id 12345678-1234-1234-1234-123456789abc x"),
            "id \x1b[93m12345678-1234-1234-1234-123456789abc\x1b[0m x"
        );
        assert_eq!(
            highlight("ip 192.168.0.1 addr"),
            "ip \x1b[1;92m192.168.0.1\x1b[0m addr"
        );
        // "3:4" matches the ipv6 pattern in upstream; "..." is the ellipsis.
        assert_eq!(
            highlight("ratio 3:4 and dots ..."),
            "ratio \x1b[1;92m3:4\x1b[0m and dots \x1b[33m...\x1b[0m"
        );
    }
}

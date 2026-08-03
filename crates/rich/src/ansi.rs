//! Decoding ANSI escape sequences back into styled [`Text`].
//!
//! Port of upstream `rich/ansi.py`. [`AnsiDecoder`] tokenizes a terminal string
//! into plain runs and SGR (Select Graphic Rendition) codes, accumulating a
//! [`Style`] as it goes and emitting one [`Text`] per line. This is the inverse
//! of the [`Console`](crate::console::Console)'s styled output.
//!
//! Scope: SGR styling (attributes, 16/256/truecolor foreground + background) is
//! fully handled, as are **OSC 8 hyperlinks** (`\x1b]8;<params>;<url>\x1b\`): the
//! URL is attached to the running [`Style`] (and cleared by the empty closing
//! sequence). Re-rendering reproduces upstream byte-for-byte except the random
//! `id=` field upstream adds, which we omit for determinism (docs/DIVERGENCES.md
//! #20).

use fancy_regex::Regex;
use std::sync::OnceLock;

use crate::color::Color;
use crate::style::Style;
use crate::text::Text;

/// The SGR parameter → `Style` spec map. Port of `rich.ansi.SGR_STYLE_MAP`.
fn sgr_style(code: u16) -> Option<&'static str> {
    let spec = match code {
        1 => "bold",
        2 => "dim",
        3 => "italic",
        4 => "underline",
        5 => "blink",
        6 => "blink2",
        7 => "reverse",
        8 => "conceal",
        9 => "strike",
        21 => "underline2",
        22 => "not dim not bold",
        23 => "not italic",
        24 => "not underline",
        25 => "not blink",
        26 => "not blink2",
        27 => "not reverse",
        28 => "not conceal",
        29 => "not strike",
        30 => "color(0)",
        31 => "color(1)",
        32 => "color(2)",
        33 => "color(3)",
        34 => "color(4)",
        35 => "color(5)",
        36 => "color(6)",
        37 => "color(7)",
        39 => "default",
        40 => "on color(0)",
        41 => "on color(1)",
        42 => "on color(2)",
        43 => "on color(3)",
        44 => "on color(4)",
        45 => "on color(5)",
        46 => "on color(6)",
        47 => "on color(7)",
        49 => "on default",
        51 => "frame",
        52 => "encircle",
        53 => "overline",
        54 => "not frame not encircle",
        55 => "not overline",
        90 => "color(8)",
        91 => "color(9)",
        92 => "color(10)",
        93 => "color(11)",
        94 => "color(12)",
        95 => "color(13)",
        96 => "color(14)",
        97 => "color(15)",
        100 => "on color(8)",
        101 => "on color(9)",
        102 => "on color(10)",
        103 => "on color(11)",
        104 => "on color(12)",
        105 => "on color(13)",
        106 => "on color(14)",
        107 => "on color(15)",
        _ => return None,
    };
    Some(spec)
}

/// The tokenizer regex (port of `rich.ansi.re_ansi`): an OSC string
/// (`\x1b]…\x1b\`) or an escape sequence (single-char or CSI).
fn re_ansi() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:\x1b\](.*?)\x1b\\)|(?:\x1b([(@-Z\\-_]|\[[0-?]*[ -/]*[@-~]))")
            .expect("valid ansi regex")
    })
}

/// One token from [`tokenize`]: plain text, an SGR parameter string, or the
/// body of an OSC string (`\x1b]<body>\x1b\`).
enum Token {
    Plain(String),
    /// The parameters of an `\x1b[…m` sequence (without the `[` and `m`).
    Sgr(String),
    /// The body of an OSC string (e.g. `8;;https://example.com`).
    Osc(String),
}

/// Tokenize a line into plain runs, SGR parameter strings, and OSC bodies,
/// mirroring `_ansi_tokenize`. Non-SGR CSI sequences are dropped.
fn tokenize(line: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut position = 0;
    for caps in re_ansi().captures_iter(line).flatten() {
        let whole = caps.get(0).expect("group 0 always present");
        let (start, end) = (whole.start(), whole.end());
        if start > position {
            tokens.push(Token::Plain(line[position..start].to_string()));
        }
        match caps.get(2) {
            Some(sgr) => {
                let sgr = sgr.as_str();
                if sgr == "(" {
                    // Charset-select escape consumes the following byte too.
                    position = (end + 1).min(line.len());
                    continue;
                }
                if let Some(params) = sgr.strip_prefix('[').and_then(|s| s.strip_suffix('m')) {
                    tokens.push(Token::Sgr(params.to_string()));
                }
                // Other CSI sequences (e.g. `[2J`) are dropped.
            }
            None => {
                // An OSC string — group 1 is its body (between `\x1b]` and the
                // terminating `\x1b\`).
                if let Some(osc) = caps.get(1) {
                    tokens.push(Token::Osc(osc.as_str().to_string()));
                }
            }
        }
        position = end;
    }
    if position < line.len() {
        tokens.push(Token::Plain(line[position..].to_string()));
    }
    tokens
}

/// Translates ANSI codes into styled [`Text`]. Mirrors `rich.ansi.AnsiDecoder`.
///
/// The decoder is stateful: a style set on one line persists to the next, just
/// like a real terminal (and like upstream).
#[derive(Default)]
pub struct AnsiDecoder {
    style: Style,
}

impl AnsiDecoder {
    pub fn new() -> Self {
        AnsiDecoder {
            style: Style::new(),
        }
    }

    /// Decode a multi-line terminal string into one [`Text`] per line.
    pub fn decode(&mut self, terminal_text: &str) -> Vec<Text> {
        // `str::lines` matches Python's `splitlines` for `\n`/`\r\n` endings.
        terminal_text.lines().map(|l| self.decode_line(l)).collect()
    }

    /// Decode a single line containing ANSI codes.
    pub fn decode_line(&mut self, line: &str) -> Text {
        // A carriage return resets the line: keep only what follows the last one.
        let line = line.rsplit('\r').next().unwrap_or(line);
        let mut text = Text::new("");
        for token in tokenize(line) {
            match token {
                Token::Plain(plain) => {
                    let style = if self.style.is_null() {
                        None
                    } else {
                        Some(self.style.clone().into())
                    };
                    text.append(&plain, style);
                }
                Token::Sgr(params) => self.apply_sgr(&params),
                Token::Osc(osc) => self.apply_osc(&osc),
            }
        }
        text
    }

    /// Apply an OSC body. Only hyperlinks (`8;<params>;<url>`) are meaningful:
    /// the params (e.g. `id=…`) are ignored, and the URL is attached to — or,
    /// when empty, cleared from — the running style. Port of the OSC branch of
    /// `decode_line`.
    fn apply_osc(&mut self, osc: &str) {
        if let Some(rest) = osc.strip_prefix("8;") {
            // partition on the first ';': everything after it is the link.
            if let Some(idx) = rest.find(';') {
                let link = &rest[idx + 1..];
                let link = (!link.is_empty()).then(|| link.to_string());
                self.style = self.style.update_link(link);
            }
        }
    }

    /// Apply an SGR parameter string (e.g. `"1;31"`) to the running style.
    fn apply_sgr(&mut self, params: &str) {
        // Lenient parse: keep digit runs (clamped to 255) and empty fields (0).
        let codes: Vec<u16> = params
            .split(';')
            .filter(|c| c.is_empty() || c.bytes().all(|b| b.is_ascii_digit()))
            .map(|c| c.parse::<u32>().unwrap_or(0).min(255) as u16)
            .collect();

        let mut iter = codes.into_iter();
        while let Some(code) = iter.next() {
            if code == 0 {
                self.style = Style::new();
            } else if let Some(spec) = sgr_style(code) {
                if let Ok(parsed) = Style::parse(spec) {
                    self.style = self.style.combine(&parsed);
                }
            } else if code == 38 {
                if let Some(color) = read_extended_color(&mut iter) {
                    self.style = self.style.combine(&Style::from_color(Some(color), None));
                }
            } else if code == 48 {
                if let Some(color) = read_extended_color(&mut iter) {
                    self.style = self.style.combine(&Style::from_color(None, Some(color)));
                }
            }
        }
    }
}

/// Read the color following a `38`/`48` code: `5;<n>` (8-bit) or `2;<r>;<g>;<b>`
/// (truecolor). Returns `None` if the sequence is truncated (lenient, like
/// upstream's `suppress(StopIteration)`).
fn read_extended_color(iter: &mut impl Iterator<Item = u16>) -> Option<Color> {
    match iter.next()? {
        5 => Some(Color::from_ansi(iter.next()? as u8)),
        2 => {
            let r = iter.next()? as u8;
            let g = iter.next()? as u8;
            let b = iter.next()? as u8;
            Some(Color::from_rgb(r, g, b))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::ColorSystem;
    use crate::console::Console;

    fn round_trip(input: &str) -> String {
        let mut decoder = AnsiDecoder::new();
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(80)
            .build();
        decoder
            .decode(input)
            .iter()
            .map(|t| console.render_to_string(t))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plain_text_has_no_style() {
        assert_eq!(round_trip("hello"), "hello");
    }

    #[test]
    fn bold_red_round_trips() {
        // Both the input and the re-render use rich's own SGR ordering.
        assert_eq!(round_trip("\x1b[1;31mhi\x1b[0m"), "\x1b[1;31mhi\x1b[0m");
    }

    #[test]
    fn eight_bit_and_truecolor() {
        assert_eq!(
            round_trip("\x1b[38;5;214mx\x1b[0m"),
            "\x1b[38;5;214mx\x1b[0m"
        );
        assert_eq!(
            round_trip("\x1b[38;2;255;136;0mx\x1b[0m"),
            "\x1b[38;2;255;136;0mx\x1b[0m"
        );
    }

    #[test]
    fn style_persists_until_reset() {
        // "a" is bold; without a reset, "b" on the next segment stays bold.
        assert_eq!(round_trip("\x1b[1mab"), "\x1b[1mab\x1b[0m");
    }

    #[test]
    fn non_sgr_csi_is_dropped() {
        assert_eq!(round_trip("\x1b[2Jhi"), "hi");
    }

    #[test]
    fn osc8_hyperlink_round_trips() {
        // A styled hyperlink: the URL attaches to the running style, so the
        // re-render wraps the styled text in OSC 8. Matches real rich 15.0.0
        // except upstream's random `id=` field, which we omit (DIVERGENCES #20).
        assert_eq!(
            round_trip("\x1b]8;;https://example.com\x1b\\\x1b[4;34mlink\x1b[0m\x1b]8;;\x1b\\"),
            "\x1b]8;;https://example.com\x1b\\\x1b[4;34mlink\x1b[0m\x1b]8;;\x1b\\"
        );
    }

    #[test]
    fn osc8_link_without_style_and_clear() {
        // Unstyled link text between plain runs; the empty closing OSC clears the
        // link so " after" is plain.
        assert_eq!(
            round_trip("before \x1b]8;;https://x.io\x1b\\here\x1b]8;;\x1b\\ after"),
            "before \x1b]8;;https://x.io\x1b\\here\x1b]8;;\x1b\\ after"
        );
    }

    #[test]
    fn osc8_id_param_is_ignored() {
        // Upstream includes a random `id=`; when decoding we drop the params and
        // keep only the URL, re-emitting without an id.
        assert_eq!(
            round_trip("\x1b]8;id=42;https://x.io\x1b\\a\x1b]8;;\x1b\\"),
            "\x1b]8;;https://x.io\x1b\\a\x1b]8;;\x1b\\"
        );
    }
}

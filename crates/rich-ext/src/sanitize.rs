//! Opt-in input sanitizing for terminal-control bytes.
//!
//! Core `rich` keeps ESC for upstream parity. This extension-owned helper is
//! for CLI boundaries that need to display untrusted text without allowing that
//! text to move the cursor, clear the screen, or open terminal hyperlinks.

/// Replace terminal controls with visible, inert text.
///
/// LF and TAB are preserved because they are content/layout in plain text, CSV
/// and TSV. ESC and C1 controls are rendered as printable text before the core
/// renderer sees them, so the renderer's own ANSI styling remains unaffected.
pub fn sanitize_terminal_controls(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\n' | '\t' => out.push(ch),
            '\u{1b}' => out.push('␛'),
            '\u{7f}' => out.push('␡'),
            '\0'..='\u{1f}' => {
                let picture = char::from_u32(0x2400 + ch as u32).expect("control picture");
                out.push(picture);
            }
            '\u{80}'..='\u{9f}' => {
                out.push_str(&format!("\\u{{{:04X}}}", ch as u32));
            }
            _ => out.push(ch),
        }
    }
    out
}

/// Neutralise terminal controls in ANSI-styled text while keeping what the
/// ANSI decoder (`Text::from_ansi`) turns into styles.
///
/// CSI sequences (`ESC [ … final`) and two-character escapes pass through:
/// the decoder applies SGR colours and drops the rest. OSC strings (titles,
/// clipboard writes, hyperlinks) are removed. Every other control — a lone
/// ESC the decoder would leave in the text, C1 controls such as U+009B, BEL
/// and the other C0 controls — is made visible as `sanitize_terminal_controls`
/// shows it. Newlines, tabs and carriage returns are kept for the decoder's
/// line handling.
pub fn sanitize_ansi_for_decoder(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch != '\u{1b}' {
            match ch {
                '\n' | '\t' | '\r' => out.push(ch),
                '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {
                    out.push_str(&sanitize_terminal_controls(ch.encode_utf8(&mut [0; 4])))
                }
                _ => out.push(ch),
            }
            i += 1;
            continue;
        }
        match chars.get(i + 1).copied() {
            // OSC: drop through its terminator on this line (ST or BEL), or
            // just the introducer when it has none.
            Some(']') => {
                let mut j = i + 2;
                let mut end = None;
                while j < chars.len() && chars[j] != '\n' {
                    if chars[j] == '\u{7}' {
                        end = Some(j + 1);
                        break;
                    }
                    if chars[j] == '\u{1b}' && chars.get(j + 1) == Some(&'\\') {
                        end = Some(j + 2);
                        break;
                    }
                    j += 1;
                }
                i = end.unwrap_or(i + 2);
            }
            Some('[') => {
                // ESC [ params(0x30-0x3f)* intermediates(0x20-0x2f)* final(0x40-0x7e)
                let mut j = i + 2;
                while j < chars.len() && ('\u{30}'..='\u{3f}').contains(&chars[j]) {
                    j += 1;
                }
                while j < chars.len() && ('\u{20}'..='\u{2f}').contains(&chars[j]) {
                    j += 1;
                }
                if j < chars.len() && ('\u{40}'..='\u{7e}').contains(&chars[j]) {
                    out.extend(&chars[i..=j]);
                    i = j + 1;
                } else {
                    out.push('␛');
                    i += 1;
                }
            }
            // Two-character escapes the decoder consumes and drops.
            Some(c) if c == '(' || ('@'..='Z').contains(&c) || ('\\'..='_').contains(&c) => {
                i += 2;
            }
            _ => {
                out.push('␛');
                i += 1;
            }
        }
    }
    out
}

/// Whether `c` is a bidirectional formatting control: ALM (U+061C), LRM and
/// RLM (U+200E, U+200F), the embeddings and overrides U+202A–U+202E and the
/// isolates U+2066–U+2069. They print nothing but reorder the text around
/// them, which lets one string display as another ("Trojan Source").
pub fn is_bidi_control(c: char) -> bool {
    matches!(
        c as u32,
        0x061c | 0x200e | 0x200f | 0x202a..=0x202e | 0x2066..=0x2069
    )
}

/// [`sanitize_terminal_controls`], and bidi controls (see
/// [`is_bidi_control`]) shown as `\u{202E}` escapes too.
///
/// A separate function because [`sanitize_terminal_controls`] promises to
/// leave everything but terminal controls alone.
pub fn sanitize_terminal_and_bidi_controls(input: &str) -> String {
    escape_bidi(&sanitize_terminal_controls(input))
}

/// Text for a one-line label (a file name, a test name): like
/// [`sanitize_terminal_and_bidi_controls`], but LF and TAB become visible
/// too (`␊`, `␉`), so the label cannot break or push the line it sits on.
pub fn sanitize_single_line(input: &str) -> String {
    sanitize_terminal_and_bidi_controls(input)
        .replace('\n', "␊")
        .replace('\t', "␉")
}

fn escape_bidi(input: &str) -> String {
    if !input.chars().any(is_bidi_control) {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len() + 8);
    for ch in input.chars() {
        if is_bidi_control(ch) {
            out.push_str(&format!("\\u{{{:04X}}}", ch as u32));
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{
        sanitize_single_line, sanitize_terminal_and_bidi_controls, sanitize_terminal_controls,
    };

    #[test]
    fn bidi_controls_are_escaped_only_by_the_bidi_variants() {
        let evil = "a\u{202e}b\u{2066}\u{61c}\x1b";
        assert_eq!(
            sanitize_terminal_controls(evil),
            "a\u{202e}b\u{2066}\u{61c}␛"
        );
        assert_eq!(
            sanitize_terminal_and_bidi_controls(evil),
            "a\\u{202E}b\\u{2066}\\u{061C}␛"
        );
        assert_eq!(sanitize_single_line("a\tb\nc\u{200f}"), "a␉b␊c\\u{200F}");
    }

    #[test]
    fn esc_and_csi_are_visible_not_executable() {
        assert_eq!(sanitize_terminal_controls("a\x1b[2Jb"), "a␛[2Jb");
        assert_eq!(sanitize_terminal_controls("a\u{9b}2Jb"), "a\\u{009B}2Jb");
    }

    #[test]
    fn preserves_layout_controls() {
        assert_eq!(sanitize_terminal_controls("a\tb\nc"), "a\tb\nc");
    }

    #[test]
    fn renders_other_c0_controls_as_control_pictures() {
        assert_eq!(sanitize_terminal_controls("a\u{7}b\rc"), "a␇b␍c");
    }
}

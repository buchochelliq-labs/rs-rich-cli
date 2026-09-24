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

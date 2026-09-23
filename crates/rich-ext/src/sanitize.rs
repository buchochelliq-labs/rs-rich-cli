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

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_controls;

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

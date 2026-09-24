//! Terminal-control hygiene and input limits for the commands this binary
//! adds (`view`, text `diff`, `capture`, `hex`, `unicode`, `inspect`) and for
//! its error messages. Not upstream: upstream `rich-cli` has none of these
//! commands, so this is a binary-boundary convenience composing
//! `rich_ext::sanitize_terminal_controls`.
use rich_ext::sanitize_terminal_controls;

// The limits below keep what these commands read, and so what they render,
// bounded: an endless input (`/dev/zero`, `yes`) ends, and rendering stays
// within a few hundred megabytes. Documented in docs/cli.md ("Limits").

/// The most `rich view` reads of a text resource; more is cut off with a
/// notice.
pub(crate) const VIEW_LIMIT: u64 = 8 * 1024 * 1024;
/// The most lines `rich view` shows, and `rich capture` keeps, before
/// cutting off with a notice.
pub(crate) const LINE_LIMIT: usize = 20_000;
/// The most bytes `rich hex` shows without `--length`, `rich view` shows of
/// binary input, and `rich unicode` reads; more is cut off with a notice.
pub(crate) const BYTES_LIMIT: u64 = 64 * 1024;
/// The most `rich inspect` parses from one document; more is an error.
pub(crate) const INSPECT_LIMIT: u64 = 64 * 1024 * 1024;
/// The most `rich capture` keeps of a command's output before it stops the
/// command.
pub(crate) const CAPTURE_LIMIT: usize = 1024 * 1024;
/// The largest theme file read (`--theme-file`, `theme_file`).
pub(crate) const THEME_FILE_LIMIT: u64 = 1024 * 1024;

/// `bytes` as a size such as `64 MiB`.
pub(crate) fn size(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else if bytes >= 1024 && bytes.is_multiple_of(1024) {
        format!("{} KiB", bytes / 1024)
    } else {
        format!("{bytes} bytes")
    }
}

/// Neutralise terminal controls in ANSI-styled text while keeping what the
/// ANSI decoder turns into styles.
///
/// CSI sequences (`ESC [ … final`) and two-character escapes pass through:
/// the decoder applies SGR colours and drops the rest. OSC strings (titles,
/// clipboard writes, hyperlinks) are removed. Every other control — a lone
/// ESC the decoder would leave in the text, C1 controls such as U+009B, BEL
/// and the other C0 controls — is made visible as `sanitize_terminal_controls`
/// shows it. Newlines, tabs and carriage returns are kept for the decoder's
/// line handling.
pub(crate) fn neutralize_for_decoder(input: &str) -> String {
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

/// `path` for an error message, with any terminal controls in it made
/// visible.
pub(crate) fn shown(text: &str) -> String {
    sanitize_terminal_controls(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_survive_and_other_controls_do_not() {
        assert_eq!(
            neutralize_for_decoder("\u{1b}[31mred\u{1b}[0m"),
            "\u{1b}[31mred\u{1b}[0m"
        );
        assert_eq!(neutralize_for_decoder("a\u{1b}]0;T\u{7}b"), "ab");
        assert_eq!(
            neutralize_for_decoder("a\u{1b}]8;;http://x\u{1b}\\l\u{1b}]8;;\u{1b}\\"),
            "al"
        );
        assert_eq!(neutralize_for_decoder("a\u{9b}2J"), "a\\u{009B}2J");
        assert_eq!(neutralize_for_decoder("a\u{1b}7b\u{7}"), "a␛7b␇");
        assert_eq!(neutralize_for_decoder("x\u{1b}[é"), "x␛[é");
        assert_eq!(
            neutralize_for_decoder("x\u{1b}]0;no end\ny"),
            "x0;no end\ny"
        );
        assert_eq!(neutralize_for_decoder("a\r\n\tb"), "a\r\n\tb");
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(size(64 * 1024 * 1024), "64 MiB");
        assert_eq!(size(8192), "8 KiB");
        assert_eq!(size(10), "10 bytes");
    }
}

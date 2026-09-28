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
/// ANSI decoder turns into styles; see
/// [`rich_ext::sanitize::sanitize_ansi_for_decoder`], which runtime plugins'
/// ANSI output goes through too.
pub(crate) fn neutralize_for_decoder(input: &str) -> String {
    rich_ext::sanitize::sanitize_ansi_for_decoder(input)
}

/// `path` for an error message, with any terminal controls in it made
/// visible. A NUL only ever marks the escaped spelling of a path that is not
/// valid Unicode (no argument or path can contain one), so it is dropped and
/// the path shows as its `\xNN` spelling.
pub(crate) fn shown(text: &str) -> String {
    sanitize_terminal_controls(&text.replace('\0', ""))
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

//! rs-rich-record's VT emulator: a program's output fed in chunks, then the
//! screen read back as text and as a snapshot. The first two bytes are the
//! size, the third the chunk length.
//!
//! The emulator catches a panic in vt100 and reports it as an error, but
//! libFuzzer's panic hook aborts first, so a vt100 panic is still found.
//! The size stays within what a tape may ask for (`tape::MIN_COLUMNS` and
//! up): vt100 0.16 panics on a wide character in a one-column screen, a
//! known limit recorded in docs/design/fuzzing.md and tested by the
//! crate. The screen is not resized, for the same reason: vt100 panics on a
//! resize that cuts a wide character.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{take_byte, MAX_PARSE_INPUT};
use rich_record::tape::{MAX_COLUMNS, MAX_ROWS, MIN_COLUMNS, MIN_ROWS};
use rich_record::terminal::Terminal;
use rich_record::Theme;

fuzz_target!(|data: &[u8]| {
    let (rows, rest) = take_byte(data);
    let (columns, rest) = take_byte(rest);
    let (chunk, rest) = take_byte(rest);
    let rest = &rest[..rest.len().min(MAX_PARSE_INPUT)];
    let mut terminal = Terminal::with_scrollback(
        MIN_ROWS + u16::from(rows) % (MAX_ROWS.min(60) - MIN_ROWS + 1),
        MIN_COLUMNS + u16::from(columns) % (MAX_COLUMNS.min(200) - MIN_COLUMNS + 1),
        usize::from(chunk % 4) * 10,
    );
    for piece in rest.chunks(usize::from(chunk) + 1) {
        if terminal.process(piece).is_err() {
            return;
        }
        let _ = terminal.take_replies();
    }
    let _ = terminal.contents();
    let _ = terminal.snapshot(&Theme::default());
});

//! rs-rich-record's VT emulator: a program's output fed in chunks, then the
//! screen read back as text and as a snapshot. The first two bytes are the
//! size, the third the chunk length.
//!
//! The emulator catches a panic in vt100 and reports it as an error, but
//! libFuzzer's panic hook aborts first, so a vt100 panic is still found.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{take_byte, MAX_PARSE_INPUT};
use rich_record::terminal::Terminal;
use rich_record::Theme;

fuzz_target!(|data: &[u8]| {
    let (rows, rest) = take_byte(data);
    let (columns, rest) = take_byte(rest);
    let (chunk, rest) = take_byte(rest);
    let rest = &rest[..rest.len().min(MAX_PARSE_INPUT)];
    let mut terminal = Terminal::with_scrollback(
        u16::from(rows % 60) + 1,
        u16::from(columns % 200) + 1,
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

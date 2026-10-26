//! The ANSI decoder (`AnsiDecoder`, as `Text.from_ansi` uses it), then the
//! decoded lines drawn on a console. The first byte is the width.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich::AnsiDecoder;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};

fuzz_target!(|data: &[u8]| {
    let (width, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    let mut decoder = AnsiDecoder::new();
    for line in decoder.decode(&source) {
        render(width, &line);
    }
    let _ = AnsiDecoder::new().decode_split_newlines(&source);
});

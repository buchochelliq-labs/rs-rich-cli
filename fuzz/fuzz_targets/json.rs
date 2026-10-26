//! `Json`: rich's own JSON reader and its pretty, highlighted layout. The
//! first byte is the width.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich::Json;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};

fuzz_target!(|data: &[u8]| {
    let (width, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    if let Ok(json) = Json::new(&source) {
        render(width, &json);
    }
});

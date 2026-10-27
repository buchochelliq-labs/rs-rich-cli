//! rs-rich-web's input decoder: xterm.js's keys, mouse reports and pastes,
//! as one WebSocket message's text.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{text, MAX_PARSE_INPUT};

fuzz_target!(|data: &[u8]| {
    let _ = rich_web::input::decode(&text(data, MAX_PARSE_INPUT));
});

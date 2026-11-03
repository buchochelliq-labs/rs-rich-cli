//! rs-rich-interact's terminal host client (`host`, #820): a terminal's
//! one-line JSON reply to a request, as `SocketHost` reads it, and the
//! panes in a `listPanes` reply.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{MAX_PARSE_INPUT, text};

fuzz_target!(|data: &[u8]| {
    let line = text(data, MAX_PARSE_INPUT);
    if let Ok(result) = rich_interact::host::parse_reply(&line) {
        let _ = rich_interact::host::panes_of(&result);
    }
});

//! rs-rich-record's tape parser (`rich record`'s script format).
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{MAX_PARSE_INPUT, text};

fuzz_target!(|data: &[u8]| {
    let _ = rich_record::tape::parse(&text(data, MAX_PARSE_INPUT));
});

//! intuiTUIve's stylesheet parser (`Stylesheet::parse`, the `.tcss` reader).
#![no_main]

use intuituive::Stylesheet;
use libfuzzer_sys::fuzz_target;
use rich_fuzz::{MAX_PARSE_INPUT, text};

fuzz_target!(|data: &[u8]| {
    let _ = Stylesheet::parse(&text(data, MAX_PARSE_INPUT));
});

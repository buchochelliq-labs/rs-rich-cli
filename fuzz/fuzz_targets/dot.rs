//! rs-rich-diagram's DOT parser, then the graph laid out and drawn (box
//! drawing or ASCII, by the first byte's low bit; the width is the rest).
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_diagram::Dot;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};

fuzz_target!(|data: &[u8]| {
    let (mode, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    let dot = Dot::new(source).ascii(mode & 1 == 1);
    if dot.parsed().is_ok() {
        render(mode >> 1, &dot);
    }
});

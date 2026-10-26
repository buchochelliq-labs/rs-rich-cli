//! Markdown (pulldown-cmark into rich's blocks), drawn on a console. The
//! first byte is the width.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich::markdown::Markdown;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};

fuzz_target!(|data: &[u8]| {
    let (width, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    render(width, &Markdown::new(&source));
});

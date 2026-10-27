//! rs-rich-mermaid: flowchart, sequence and ER sources parsed and drawn
//! (box drawing or ASCII, by the first byte's low bit; the width is the
//! rest). Text-only: the `mmdc` backend is not built.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};
use rich_mermaid::Mermaid;

fuzz_target!(|data: &[u8]| {
    let (mode, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    let _ = rich_mermaid::flowchart::parse(&source);
    let _ = rich_mermaid::sequence::parse(&source);
    let _ = rich_mermaid::er::parse(&source);
    render(mode >> 1, &Mermaid::new(source).ascii(mode & 1 == 1));
});

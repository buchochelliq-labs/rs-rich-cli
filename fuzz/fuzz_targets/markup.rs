//! Console markup (`[bold red]…[/]`): `Text::from_markup`, `markup::render`
//! and `Style::parse`, then the text drawn on a console. The first byte is
//! the width.
#![no_main]

use libfuzzer_sys::fuzz_target;
use rich::style::Style;
use rich::text::Text;
use rich_fuzz::{render, take_byte, text, MAX_RENDER_INPUT};

fuzz_target!(|data: &[u8]| {
    let (width, rest) = take_byte(data);
    let source = text(rest, MAX_RENDER_INPUT);
    let _ = Style::parse(&source);
    let _ = rich::markup::render(&source);
    if let Ok(parsed) = Text::from_markup(&source) {
        render(width, &parsed);
    }
});

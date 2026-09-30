//! iTerm2's inline images (`OSC 1337 ; File=`), also spoken by WezTerm.
//!
//! [`inline_image`] draws a PNG or an animated GIF at the cursor at an exact
//! size in cells; iTerm2 plays a GIF's frames itself. The terminal moves the
//! cursor past the image, so [`crate::graphics::overlay`] wraps it to draw
//! over cells a line already reserved and leave the cursor right after them.
//! Other animation formats are transcoded to GIF first
//! ([`crate::graphics::encode_gif`]).
//!
//! Requires the `iterm` feature.

use crate::graphics::base64;

/// The escape that draws `bytes` (a PNG, or a GIF, which iTerm2 animates)
/// at the cursor in exactly `cols × rows` cells. The raster is expected to
/// have the cells' shape already; the terminal keeps its aspect ratio
/// either way.
pub fn inline_image(bytes: &[u8], cols: usize, rows: usize) -> String {
    format!(
        "\x1b]1337;File=inline=1;size={};width={cols};height={rows};preserveAspectRatio=1:{}\x07",
        bytes.len(),
        base64(bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_in_cells_and_encodes_the_payload() {
        assert_eq!(
            inline_image(b"foo", 2, 1),
            "\x1b]1337;File=inline=1;size=3;width=2;height=1;preserveAspectRatio=1:Zm9v\x07"
        );
    }
}

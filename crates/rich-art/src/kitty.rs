//! The Kitty graphics protocol, with Unicode placeholders.
//!
//! An image is transmitted once, under an id, and given a *virtual*
//! placement of `cols × rows` cells. It then shows wherever the terminal
//! prints placeholder cells for it: [`PLACEHOLDER`] (U+10EEEE) followed by a
//! diacritic for the row and one for the column, in a foreground colour that
//! encodes the image id. The image therefore lives in the cell grid: it
//! scrolls with the text, survives a redraw that reprints the cells, is
//! overwritten like any other cell, and costs nothing to show again.
//!
//! - [`transmit`] sends a PNG and makes its virtual placement;
//!   [`transmit_animation`] adds frames and starts Kitty's own playback.
//! - [`placeholder_row`] is the text of one row of cells, and
//!   [`id_color`] the colour those cells must be printed in.
//! - [`delete`] frees an image and its data when the view that showed it
//!   closes.
//!
//! Every command carries `q=2`, so the terminal sends no replies back into
//! the program's input.
//!
//! Requires the `kitty` feature.

use std::time::Duration;

use rich::color::Color;

use crate::graphics::{base64, encode_png, AnimationFrame};

/// The placeholder character (Supplementary Private Use Area-B).
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Payload bytes per chunk: the protocol's limit for one escape.
const CHUNK: usize = 4096;

/// Kitty's row and column diacritics, in order: the *n*th encodes *n*.
/// From kitty's `gen/rowcolumn-diacritics.txt`.
#[rustfmt::skip]
pub const DIACRITICS: [char; 297] = [
    '\u{0305}', '\u{030D}', '\u{030E}', '\u{0310}', '\u{0312}', '\u{033D}', '\u{033E}', '\u{033F}',
    '\u{0346}', '\u{034A}', '\u{034B}', '\u{034C}', '\u{0350}', '\u{0351}', '\u{0352}', '\u{0357}',
    '\u{035B}', '\u{0363}', '\u{0364}', '\u{0365}', '\u{0366}', '\u{0367}', '\u{0368}', '\u{0369}',
    '\u{036A}', '\u{036B}', '\u{036C}', '\u{036D}', '\u{036E}', '\u{036F}', '\u{0483}', '\u{0484}',
    '\u{0485}', '\u{0486}', '\u{0487}', '\u{0592}', '\u{0593}', '\u{0594}', '\u{0595}', '\u{0597}',
    '\u{0598}', '\u{0599}', '\u{059C}', '\u{059D}', '\u{059E}', '\u{059F}', '\u{05A0}', '\u{05A1}',
    '\u{05A8}', '\u{05A9}', '\u{05AB}', '\u{05AC}', '\u{05AF}', '\u{05C4}', '\u{0610}', '\u{0611}',
    '\u{0612}', '\u{0613}', '\u{0614}', '\u{0615}', '\u{0616}', '\u{0617}', '\u{0657}', '\u{0658}',
    '\u{0659}', '\u{065A}', '\u{065B}', '\u{065D}', '\u{065E}', '\u{06D6}', '\u{06D7}', '\u{06D8}',
    '\u{06D9}', '\u{06DA}', '\u{06DB}', '\u{06DC}', '\u{06DF}', '\u{06E0}', '\u{06E1}', '\u{06E2}',
    '\u{06E4}', '\u{06E7}', '\u{06E8}', '\u{06EB}', '\u{06EC}', '\u{0730}', '\u{0732}', '\u{0733}',
    '\u{0735}', '\u{0736}', '\u{073A}', '\u{073D}', '\u{073F}', '\u{0740}', '\u{0741}', '\u{0743}',
    '\u{0745}', '\u{0747}', '\u{0749}', '\u{074A}', '\u{07EB}', '\u{07EC}', '\u{07ED}', '\u{07EE}',
    '\u{07EF}', '\u{07F0}', '\u{07F1}', '\u{07F3}', '\u{0816}', '\u{0817}', '\u{0818}', '\u{0819}',
    '\u{081B}', '\u{081C}', '\u{081D}', '\u{081E}', '\u{081F}', '\u{0820}', '\u{0821}', '\u{0822}',
    '\u{0823}', '\u{0825}', '\u{0826}', '\u{0827}', '\u{0829}', '\u{082A}', '\u{082B}', '\u{082C}',
    '\u{082D}', '\u{0951}', '\u{0953}', '\u{0954}', '\u{0F82}', '\u{0F83}', '\u{0F86}', '\u{0F87}',
    '\u{135D}', '\u{135E}', '\u{135F}', '\u{17DD}', '\u{193A}', '\u{1A17}', '\u{1A75}', '\u{1A76}',
    '\u{1A77}', '\u{1A78}', '\u{1A79}', '\u{1A7A}', '\u{1A7B}', '\u{1A7C}', '\u{1B6B}', '\u{1B6D}',
    '\u{1B6E}', '\u{1B6F}', '\u{1B70}', '\u{1B71}', '\u{1B72}', '\u{1B73}', '\u{1CD0}', '\u{1CD1}',
    '\u{1CD2}', '\u{1CDA}', '\u{1CDB}', '\u{1CE0}', '\u{1DC0}', '\u{1DC1}', '\u{1DC3}', '\u{1DC4}',
    '\u{1DC5}', '\u{1DC6}', '\u{1DC7}', '\u{1DC8}', '\u{1DC9}', '\u{1DCB}', '\u{1DCC}', '\u{1DD1}',
    '\u{1DD2}', '\u{1DD3}', '\u{1DD4}', '\u{1DD5}', '\u{1DD6}', '\u{1DD7}', '\u{1DD8}', '\u{1DD9}',
    '\u{1DDA}', '\u{1DDB}', '\u{1DDC}', '\u{1DDD}', '\u{1DDE}', '\u{1DDF}', '\u{1DE0}', '\u{1DE1}',
    '\u{1DE2}', '\u{1DE3}', '\u{1DE4}', '\u{1DE5}', '\u{1DE6}', '\u{1DFE}', '\u{20D0}', '\u{20D1}',
    '\u{20D4}', '\u{20D5}', '\u{20D6}', '\u{20D7}', '\u{20DB}', '\u{20DC}', '\u{20E1}', '\u{20E7}',
    '\u{20E9}', '\u{20F0}', '\u{2CEF}', '\u{2CF0}', '\u{2CF1}', '\u{2DE0}', '\u{2DE1}', '\u{2DE2}',
    '\u{2DE3}', '\u{2DE4}', '\u{2DE5}', '\u{2DE6}', '\u{2DE7}', '\u{2DE8}', '\u{2DE9}', '\u{2DEA}',
    '\u{2DEB}', '\u{2DEC}', '\u{2DED}', '\u{2DEE}', '\u{2DEF}', '\u{2DF0}', '\u{2DF1}', '\u{2DF2}',
    '\u{2DF3}', '\u{2DF4}', '\u{2DF5}', '\u{2DF6}', '\u{2DF7}', '\u{2DF8}', '\u{2DF9}', '\u{2DFA}',
    '\u{2DFB}', '\u{2DFC}', '\u{2DFD}', '\u{2DFE}', '\u{2DFF}', '\u{A66F}', '\u{A67C}', '\u{A67D}',
    '\u{A6F0}', '\u{A6F1}', '\u{A8E0}', '\u{A8E1}', '\u{A8E2}', '\u{A8E3}', '\u{A8E4}', '\u{A8E5}',
    '\u{A8E6}', '\u{A8E7}', '\u{A8E8}', '\u{A8E9}', '\u{A8EA}', '\u{A8EB}', '\u{A8EC}', '\u{A8ED}',
    '\u{A8EE}', '\u{A8EF}', '\u{A8F0}', '\u{A8F1}', '\u{AAB0}', '\u{AAB2}', '\u{AAB3}', '\u{AAB7}',
    '\u{AAB8}', '\u{AABE}', '\u{AABF}', '\u{AAC1}', '\u{FE20}', '\u{FE21}', '\u{FE22}', '\u{FE23}',
    '\u{FE24}', '\u{FE25}', '\u{FE26}', '\u{10A0F}', '\u{10A38}', '\u{1D185}', '\u{1D186}', '\u{1D187}',
    '\u{1D188}', '\u{1D189}', '\u{1D1AA}', '\u{1D1AB}', '\u{1D1AC}', '\u{1D1AD}', '\u{1D242}', '\u{1D243}',
    '\u{1D244}',
];

/// The diacritic that encodes row or column `index`, if there is one.
pub fn diacritic(index: usize) -> Option<char> {
    DIACRITICS.get(index).copied()
}

/// One placeholder cell: row `row`, column `col` of the image.
pub fn placeholder_cell(row: usize, col: usize) -> Option<String> {
    let mut cell = String::with_capacity(12);
    cell.push(PLACEHOLDER);
    cell.push(diacritic(row)?);
    cell.push(diacritic(col)?);
    Some(cell)
}

/// The placeholder cells of row `row` of an image `cols` wide: `cols` cells
/// of text. `None` past the last diacritic (297 rows or columns).
pub fn placeholder_row(row: usize, cols: usize) -> Option<String> {
    (0..cols).map(|col| placeholder_cell(row, col)).collect()
}

/// The foreground colour placeholder cells for image `id` are printed in.
///
/// With `truecolor`, any id below 2²⁴ is an RGB colour. Otherwise only ids
/// 16..=255 can be written, as 256-colour indices (0–15 would be written as
/// the standard colours, which the protocol does not read as an id).
pub fn id_color(id: u32, truecolor: bool) -> Option<Color> {
    if id == 0 || id >= 1 << 24 {
        return None;
    }
    if truecolor {
        Some(Color::from_rgb((id >> 16) as u8, (id >> 8) as u8, id as u8))
    } else if (16..=255).contains(&id) {
        Some(Color::from_ansi(id as u8))
    } else {
        None
    }
}

/// One graphics command, its payload split into protocol-sized chunks.
fn command(keys: &str, payload: &[u8]) -> String {
    let data = base64(payload);
    if data.is_empty() {
        return format!("\x1b_G{keys}\x1b\\");
    }
    // Base64 is ASCII, so any byte boundary is a character boundary.
    let chunks: Vec<&str> = (0..data.len())
        .step_by(CHUNK)
        .map(|start| &data[start..(start + CHUNK).min(data.len())])
        .collect();
    let mut out = String::with_capacity(data.len() + chunks.len() * 16 + keys.len());
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        if index == 0 {
            out.push_str(&format!("\x1b_G{keys},m={more};{chunk}\x1b\\"));
        } else {
            out.push_str(&format!("\x1b_Gm={more};{chunk}\x1b\\"));
        }
    }
    out
}

/// Transmit `png` as image `id` and give it a virtual placement of
/// `cols × rows` cells, for placeholder cells to show.
pub fn transmit(id: u32, png: &[u8], cols: usize, rows: usize) -> String {
    command(
        &format!("a=T,U=1,f=100,t=d,i={id},c={cols},r={rows},q=2"),
        png,
    )
}

/// Transmit `frames` as image `id` (the first is the image, the rest its
/// animation frames), give it a virtual placement of `cols × rows` cells,
/// and start Kitty's playback, looping forever. With one frame this is
/// [`transmit`]. `None` when a frame will not encode.
pub fn transmit_animation(
    id: u32,
    frames: &[AnimationFrame],
    cols: usize,
    rows: usize,
) -> Option<String> {
    let (first, rest) = frames.split_first()?;
    let mut out = transmit(id, &encode_png(&first.image)?, cols, rows);
    if rest.is_empty() {
        return Some(out);
    }
    let millis = |delay: Duration| delay.as_millis().clamp(1, u32::MAX as u128);
    for frame in rest {
        out.push_str(&command(
            &format!("a=f,f=100,t=d,i={id},z={},q=2", millis(frame.delay)),
            &encode_png(&frame.image)?,
        ));
    }
    // The root frame's gap, then play: s=3 runs and loops, v=1 forever.
    out.push_str(&format!(
        "\x1b_Ga=a,i={id},r=1,z={},q=2\x1b\\",
        millis(first.delay)
    ));
    out.push_str(&format!("\x1b_Ga=a,i={id},s=3,v=1,q=2\x1b\\"));
    Some(out)
}

/// Stop image `id`'s animation on its current frame.
pub fn stop_animation(id: u32) -> String {
    format!("\x1b_Ga=a,i={id},s=1,q=2\x1b\\")
}

/// Delete image `id`: its placements and, with the upper-case `I`, its data.
pub fn delete(id: u32) -> String {
    format!("\x1b_Ga=d,d=I,i={id},q=2\x1b\\")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::cells::cell_len;

    #[test]
    fn placeholders_take_one_cell_each() {
        for rows in [0, 1, 5, 296] {
            for cols in [1, 2, 4, 297] {
                let row = placeholder_row(rows, cols).expect("in range");
                assert_eq!(cell_len(&row), cols, "row {rows} cols {cols}");
                assert_eq!(row.chars().filter(|c| *c == PLACEHOLDER).count(), cols);
            }
        }
        assert!(placeholder_row(0, 298).is_none());
        assert_eq!(
            placeholder_row(0, 2).unwrap(),
            "\u{10EEEE}\u{0305}\u{0305}\u{10EEEE}\u{0305}\u{030D}"
        );
    }

    #[test]
    fn id_colours() {
        assert_eq!(id_color(0x01_02_03, true), Some(Color::from_rgb(1, 2, 3)));
        assert_eq!(id_color(42, false), Some(Color::from_ansi(42)));
        assert_eq!(id_color(5, false), None);
        assert_eq!(id_color(300, false), None);
        assert_eq!(id_color(0, true), None);
    }

    #[test]
    fn transmission_is_chunked_and_quiet() {
        let out = transmit(7, &[0u8; 5000], 2, 1);
        assert!(out.starts_with("\x1b_Ga=T,U=1,f=100,t=d,i=7,c=2,r=1,q=2,m=1;"));
        // 5000 bytes are 6668 base64 characters: two chunks.
        assert_eq!(out.matches("\x1b_G").count(), 2);
        assert!(out.contains("\x1b\\\x1b_Gm=0;"));
        assert_eq!(delete(7), "\x1b_Ga=d,d=I,i=7,q=2\x1b\\");
    }

    #[test]
    fn animations_add_frames_and_loop() {
        use image::{Rgba, RgbaImage};
        let frame = |c: u8, ms| AnimationFrame {
            image: RgbaImage::from_pixel(2, 2, Rgba([c, 0, 0, 255])),
            delay: Duration::from_millis(ms),
        };
        let out = transmit_animation(9, &[frame(1, 100), frame(2, 150)], 2, 1).unwrap();
        assert!(out.contains("\x1b_Ga=f,f=100,t=d,i=9,z=150,q=2,m=0;"));
        assert!(out.contains("\x1b_Ga=a,i=9,r=1,z=100,q=2\x1b\\"));
        assert!(out.ends_with("\x1b_Ga=a,i=9,s=3,v=1,q=2\x1b\\"));
    }
}

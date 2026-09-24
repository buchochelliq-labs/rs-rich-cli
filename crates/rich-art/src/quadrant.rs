//! Images rendered with Unicode quadrant blocks: 2×2 pixels per cell.
//!
//! [`BlockArt`](crate::block::BlockArt) splits a cell into two pixels stacked
//! vertically. Quadrant characters (`▘ ▝ ▖ ▗ ▌ ▐ ▚ ▞ ▛ ▜ ▙ ▟ ▀ ▄ █`) split it
//! into four, doubling the horizontal detail. A cell still holds only two
//! colours (foreground and background), so each cell picks the partition of
//! its four pixels into two groups that loses the least:
//!
//! * The eight distinct partitions are tried in a fixed order: all four
//!   together (`█`), then the masks containing the top-left pixel: `▘ ▀ ▌ ▛ ▚ ▜ ▙`.
//! * Each group is painted in its mean colour. The cost is the summed squared
//!   RGB distance of every pixel from its group's mean.
//! * The strictly cheapest partition wins, so exact ties keep the earlier one
//!   and a uniform cell always renders as `█`.
//!
//! Transparency composites onto black, as in `BlockArt`. With an ANSI colour
//! mode the raster is quantized (and dithered) first; each group's mean is then
//! mapped to the nearest palette entry again, because the mean of two palette
//! colours is usually not one.

use crate::image_color::{nearest, preprocess};
use crate::{ColorDistance, Dither, ImageColorMode};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use rich::color::Color;
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;

/// Candidate foreground masks in evaluation order. Bits: top-left 1,
/// top-right 2, bottom-left 4, bottom-right 8.
const MASKS: [u8; 8] = [15, 1, 3, 5, 7, 9, 11, 13];

/// The glyph whose painted (foreground) quadrants are `mask`.
fn glyph(mask: u8) -> char {
    match mask {
        1 => '\u{2598}',  // ▘
        3 => '\u{2580}',  // ▀
        5 => '\u{258C}',  // ▌
        7 => '\u{259B}',  // ▛
        9 => '\u{259A}',  // ▚
        11 => '\u{259C}', // ▜
        13 => '\u{2599}', // ▙
        // The complements only appear when transparent quadrants are kept.
        2 => '\u{259D}',  // ▝
        4 => '\u{2596}',  // ▖
        6 => '\u{259E}',  // ▞
        8 => '\u{2597}',  // ▗
        10 => '\u{2590}', // ▐
        12 => '\u{2584}', // ▄
        14 => '\u{259F}', // ▟
        0 => ' ',
        _ => '\u{2588}', // █
    }
}

/// One chosen cell: glyph plus foreground and background RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct QuadCell {
    pub glyph: char,
    pub fg: [f64; 3],
    pub bg: [f64; 3],
    /// The unpainted quadrants are transparent: leave the cell background to
    /// the terminal. Only set when transparency is kept.
    pub open: bool,
}

/// Pick the cheapest two-colour partition of four pixels (TL, TR, BL, BR).
pub(crate) fn choose(pixels: [[f64; 3]; 4]) -> QuadCell {
    let mean = |mask: u8, inside: bool| -> [f64; 3] {
        let members: Vec<_> = (0..4)
            .filter(|i| (mask >> i & 1 == 1) == inside)
            .map(|i| pixels[i])
            .collect();
        if members.is_empty() {
            return [0.0; 3];
        }
        std::array::from_fn(|c| members.iter().map(|p| p[c]).sum::<f64>() / members.len() as f64)
    };
    let mut best = None::<(f64, QuadCell)>;
    for mask in MASKS {
        let fg = mean(mask, true);
        let bg = if mask == 15 { fg } else { mean(mask, false) };
        let cost: f64 = (0..4)
            .map(|i| {
                let target = if mask >> i & 1 == 1 { fg } else { bg };
                (0..3)
                    .map(|c| (pixels[i][c] - target[c]).powi(2))
                    .sum::<f64>()
            })
            .sum();
        if best.is_none_or(|(b, _)| cost < b) {
            best = Some((
                cost,
                QuadCell {
                    glyph: glyph(mask),
                    fg,
                    bg,
                    open: false,
                },
            ));
        }
    }
    best.expect("MASKS is nonempty").1
}

/// A cell whose `clear` quadrants (same bit order) are transparent: the
/// opaque ones become the foreground glyph in their mean colour and the rest
/// is left unpainted. Fully opaque cells use [`choose`] as usual.
fn choose_with_clear(pixels: [[f64; 3]; 4], clear: u8) -> QuadCell {
    if clear == 0 {
        return choose(pixels);
    }
    let mask = !clear & 15;
    let members: Vec<_> = (0..4).filter(|i| mask >> i & 1 == 1).collect();
    let fg = if members.is_empty() {
        [0.0; 3]
    } else {
        std::array::from_fn(|c| {
            members.iter().map(|&i| pixels[i][c]).sum::<f64>() / members.len() as f64
        })
    };
    QuadCell {
        glyph: glyph(mask),
        fg,
        bg: [0.0; 3],
        open: true,
    }
}

/// An image drawn with quadrant-block characters.
pub struct QuadrantArt {
    image: std::sync::Arc<DynamicImage>,
    width: Option<usize>,
    height: Option<usize>,
    color_mode: ImageColorMode,
    dither: Dither,
    distance: ColorDistance,
    transparent: bool,
}

impl QuadrantArt {
    pub fn new(image: DynamicImage) -> Self {
        Self::from_shared(std::sync::Arc::new(image))
    }

    pub(crate) fn from_shared(image: std::sync::Arc<DynamicImage>) -> Self {
        Self {
            image,
            width: None,
            height: None,
            color_mode: ImageColorMode::default(),
            dither: Dither::default(),
            distance: ColorDistance::default(),
            transparent: false,
        }
    }

    pub(crate) fn color_processing(
        mut self,
        mode: ImageColorMode,
        dither: Dither,
        distance: ColorDistance,
    ) -> Self {
        self.color_mode = mode;
        self.dither = dither;
        self.distance = distance;
        self
    }

    /// Leave quadrants under half opacity unpainted (see
    /// [`BlockArt`](crate::BlockArt)'s equivalent).
    pub(crate) fn keep_transparency(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
        Ok(Self::new(image::open(path)?))
    }

    /// Render into this many columns instead of the console's width.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Cap the number of character rows, shrinking the width to keep the
    /// aspect ratio (as [`BlockArt::height`](crate::BlockArt::height) does).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows);
        self
    }

    /// Columns and character rows: the same grid as `BlockArt`, since a cell
    /// is still two pixels tall; quadrants add the second pixel column.
    pub(crate) fn grid(&self, available: usize) -> (usize, usize) {
        let (iw, ih) = self.image.dimensions();
        if iw == 0 || ih == 0 {
            return (1, 1);
        }
        let mut columns = self.width.unwrap_or(available).max(1);
        let mut rows = ((((ih as f64 * columns as f64) / iw as f64) / 2.0).round() as usize).max(1);
        if let Some(cap) = self.height {
            if rows > cap.max(1) {
                let factor = cap.max(1) as f64 / rows as f64;
                columns = ((columns as f64 * factor).round() as usize).max(1);
                rows = cap.max(1);
            }
        }
        (columns, rows)
    }

    pub(crate) fn cells(&self, available: usize) -> Vec<Vec<QuadCell>> {
        let (columns, rows) = self.grid(available);
        let mut scaled = self
            .image
            .resize_exact(
                (columns * 2) as u32,
                (rows * 2) as u32,
                FilterType::Triangle,
            )
            .to_rgba8();
        let clear = crate::image_art::clear_mask(&mut scaled, self.transparent);
        // Quantization composites alpha onto black itself; truecolor does it here.
        preprocess(
            &mut scaled,
            self.color_mode,
            self.dither,
            self.distance,
            clear.as_deref(),
        );
        let width = scaled.width() as usize;
        let sample = |x: usize, y: usize| -> [f64; 3] {
            let [r, g, b, a] = scaled.get_pixel(x as u32, y as u32).0;
            // Kept transparency shows an opaque-enough pixel's own colour.
            let f = if clear.is_some() {
                1.0
            } else {
                f64::from(a) / 255.0
            };
            [f64::from(r) * f, f64::from(g) * f, f64::from(b) * f]
        };
        let is_clear = |x: usize, y: usize| clear.as_ref().is_some_and(|c| c[y * width + x]);
        (0..rows)
            .map(|row| {
                (0..columns)
                    .map(|col| {
                        let (x, y) = (col * 2, row * 2);
                        let clear = [(x, y), (x + 1, y), (x, y + 1), (x + 1, y + 1)]
                            .iter()
                            .enumerate()
                            .fold(0u8, |bits, (i, &(px, py))| {
                                bits | (u8::from(is_clear(px, py)) << i)
                            });
                        choose_with_clear(
                            [
                                sample(x, y),
                                sample(x + 1, y),
                                sample(x, y + 1),
                                sample(x + 1, y + 1),
                            ],
                            clear,
                        )
                    })
                    .collect()
            })
            .collect()
    }

    fn color(&self, rgb: [f64; 3]) -> Color {
        if self.color_mode == ImageColorMode::TrueColor {
            let [r, g, b] = rgb.map(|v| v.round().clamp(0.0, 255.0) as u8);
            Color::from_rgb(r, g, b)
        } else {
            Color::from_ansi(nearest(rgb, self.color_mode, self.distance).0)
        }
    }
}

impl Renderable for QuadrantArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.cells(options.max_width);
        let mut segments = Vec::new();
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.iter().enumerate() {
            for cell in row {
                let style = match (cell.open, cell.glyph) {
                    (true, ' ') => None,
                    (true, _) => Some(Style::new().with_color(self.color(cell.fg))),
                    (false, _) => Some(
                        Style::new()
                            .with_color(self.color(cell.fg))
                            .with_bgcolor(self.color(cell.bg)),
                    ),
                };
                segments.push(Segment::new(cell.glyph.to_string(), style));
            }
            if index != last {
                segments.push(Segment::line());
            }
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: [f64; 3] = [255.0, 0.0, 0.0];
    const B: [f64; 3] = [0.0, 0.0, 255.0];

    #[test]
    fn uniform_cells_are_full_blocks() {
        let cell = choose([R; 4]);
        assert_eq!(cell.glyph, '█');
        assert_eq!((cell.fg, cell.bg), (R, R));
    }

    #[test]
    fn every_two_colour_split_selects_its_exact_glyph() {
        // (TL, TR, BL, BR) with the top-left pixel always red (foreground).
        let cases = [
            ([R, B, B, B], '▘'),
            ([R, R, B, B], '▀'),
            ([R, B, R, B], '▌'),
            ([R, R, R, B], '▛'),
            ([R, B, B, R], '▚'),
            ([R, R, B, R], '▜'),
            ([R, B, R, R], '▙'),
        ];
        for (pixels, expected) in cases {
            let cell = choose(pixels);
            assert_eq!(cell.glyph, expected, "{pixels:?}");
            assert_eq!((cell.fg, cell.bg), (R, B));
        }
        // The complement paints the other colour as foreground.
        let cell = choose([B, R, R, R]);
        assert_eq!((cell.glyph, cell.fg, cell.bg), ('▘', B, R));
    }

    #[test]
    fn three_colours_keep_the_cheapest_split() {
        // Near-black TL/TR, white BL, grey BR: the grey sits nearer the white.
        let cell = choose([[0.0; 3], [10.0; 3], [255.0; 3], [200.0; 3]]);
        assert_eq!(cell.glyph, '▀');
        assert_eq!(cell.fg, [5.0; 3]);
        assert_eq!(cell.bg, [227.5; 3]);
    }
}

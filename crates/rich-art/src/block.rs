//! Images rendered as half-blocks — full colour, twice the vertical resolution.
//!
//! [`AsciiArt`](crate::ascii::AsciiArt) maps each pixel to a character from a
//! ramp, which is the right answer for line art and the wrong one for anything
//! photographic: a dark pixel becomes a space, so the picture arrives full of
//! holes and its shape is unreadable.
//!
//! This renders every cell as `▀` (upper half block) with the **foreground**
//! set to the upper pixel and the **background** to the lower one. Two
//! consequences follow:
//!
//! * Every cell is fully painted, so there are no gaps.
//! * A terminal cell is roughly twice as tall as it is wide, so packing two
//!   pixel rows into one character row makes the aspect ratio come out right
//!   *and* doubles the vertical detail — a ramp renderer has to throw half the
//!   rows away to avoid a stretched image.
//!
//! It needs a truecolour (or at least 256-colour) terminal; with colour off
//! there is nothing to see, so callers should fall back to `AsciiArt` there.

use image::{imageops::FilterType, DynamicImage, GenericImageView};
use rich::color::Color;
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;

/// The glyph: the top half is painted in the foreground colour, the bottom
/// half is left as background.
const UPPER_HALF: &str = "\u{2580}";

/// An image drawn with half-block characters.
pub struct BlockArt {
    image: DynamicImage,
    width: Option<usize>,
    height: Option<usize>,
}

impl BlockArt {
    pub fn new(image: DynamicImage) -> Self {
        Self {
            image,
            width: None,
            height: None,
        }
    }

    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
        Ok(Self::new(image::open(path)?))
    }

    /// Render into this many columns instead of the console's width.
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Cap the number of character rows. The image keeps its aspect ratio, so
    /// this effectively caps the width too — useful when a picture would
    /// otherwise push everything else off the screen.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows);
        self
    }

    /// Columns and character rows for the available width.
    fn grid(&self, available: usize) -> (usize, usize) {
        let (iw, ih) = self.image.dimensions();
        if iw == 0 || ih == 0 {
            return (1, 1);
        }
        let mut columns = self.width.unwrap_or(available).max(1);
        // Two pixel rows per character row, hence the halving; the cell aspect
        // is already accounted for by that pairing.
        let mut rows = (((ih as f64 * columns as f64) / iw as f64) / 2.0).round() as usize;
        rows = rows.max(1);

        if let Some(cap) = self.height {
            if rows > cap.max(1) {
                // Scale the width down to keep the aspect ratio rather than
                // squashing the image into the cap.
                let factor = cap.max(1) as f64 / rows as f64;
                columns = ((columns as f64 * factor).round() as usize).max(1);
                rows = cap.max(1);
            }
        }
        (columns, rows)
    }

    /// The rendered rows as `(upper, lower)` colour pairs.
    fn cells(&self, available: usize) -> Vec<Vec<(Color, Color)>> {
        let (columns, rows) = self.grid(available);
        let scaled = self
            .image
            .resize_exact(columns as u32, (rows * 2) as u32, FilterType::Triangle)
            .to_rgba8();

        (0..rows)
            .map(|row| {
                (0..columns)
                    .map(|col| {
                        let sample = |y: u32| {
                            let p = scaled.get_pixel(col as u32, y.min(scaled.height() - 1));
                            let [r, g, b, a] = p.0;
                            // Composite onto black so transparency reads as
                            // empty rather than as an opaque colour.
                            let f = f32::from(a) / 255.0;
                            Color::from_rgb(
                                (f32::from(r) * f) as u8,
                                (f32::from(g) * f) as u8,
                                (f32::from(b) * f) as u8,
                            )
                        };
                        (sample((row * 2) as u32), sample((row * 2 + 1) as u32))
                    })
                    .collect()
            })
            .collect()
    }
}

impl Renderable for BlockArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.cells(options.max_width);
        let mut segments = Vec::new();
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.iter().enumerate() {
            for (upper, lower) in row {
                let style = Style::new()
                    .with_color(upper.clone())
                    .with_bgcolor(lower.clone());
                segments.push(Segment::new(UPPER_HALF.to_string(), Some(style)));
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
    use image::{Rgb, RgbImage};
    use rich::color::ColorSystem;

    fn image_of(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> DynamicImage {
        let mut img = RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = Rgb(f(x, y));
        }
        DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn two_pixel_rows_collapse_into_one_character_row() {
        // 8 wide, 8 tall -> 8 columns, 4 character rows.
        let art = BlockArt::new(image_of(8, 8, |_, _| [10, 20, 30])).width(8);
        assert_eq!(art.grid(80), (8, 4));
    }

    #[test]
    fn every_cell_is_painted_with_both_colours() {
        // Top half red, bottom half blue: each cell should carry one as the
        // foreground and the other as the background, with no gaps.
        let art = BlockArt::new(image_of(4, 4, |_, y| {
            if y < 2 {
                [255, 0, 0]
            } else {
                [0, 0, 255]
            }
        }))
        .width(4);
        let rows = art.cells(80);
        assert_eq!(rows.len(), 2);
        // The first character row covers the two red pixel rows.
        for (upper, lower) in &rows[0] {
            assert_eq!(*upper, Color::from_rgb(255, 0, 0));
            assert_eq!(*lower, Color::from_rgb(255, 0, 0));
        }
        for (upper, lower) in &rows[1] {
            assert_eq!(*upper, Color::from_rgb(0, 0, 255));
            assert_eq!(*lower, Color::from_rgb(0, 0, 255));
        }
    }

    #[test]
    fn a_height_cap_preserves_the_aspect_ratio() {
        // A tall image capped to 4 rows must lose width too, not squash.
        let art = BlockArt::new(image_of(40, 200, |_, _| [1, 2, 3]))
            .width(40)
            .height(4);
        let (columns, rows) = art.grid(80);
        assert_eq!(rows, 4);
        assert!(
            columns < 40,
            "width should shrink with the cap, got {columns}"
        );
    }

    #[test]
    fn renders_with_a_background_colour_per_cell() {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(4)
            .no_color(false)
            .build();
        let art = BlockArt::new(image_of(2, 2, |_, y| {
            if y == 0 {
                [255, 0, 0]
            } else {
                [0, 0, 255]
            }
        }))
        .width(2);
        let out = console.render_to_string(&art);
        assert!(
            out.contains('\u{2580}'),
            "expected half blocks, got:\n{out}"
        );
        // Truecolour foreground (38;2) and background (48;2) both present.
        assert!(
            out.contains("38;2;255;0;0"),
            "expected a red fg, got:\n{out}"
        );
        assert!(
            out.contains("48;2;0;0;255"),
            "expected a blue bg, got:\n{out}"
        );
    }
}

//! Images rendered with Unicode Braille cells.

use image::{imageops::FilterType, DynamicImage, GenericImageView};
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;

const DOTS: [[u8; 2]; 4] = [[0, 3], [1, 4], [2, 5], [6, 7]];

/// A monochrome image rendered with two pixels by four pixels per cell.
pub struct BrailleArt {
    image: std::sync::Arc<DynamicImage>,
    width: Option<usize>,
    height: Option<usize>,
    /// Keep transparency (`ImageBackground::TerminalDefault`): a dot needs at
    /// least half opacity and luma 128 of its own, not alpha-darkened luma.
    transparent: bool,
}

impl BrailleArt {
    pub fn new(image: DynamicImage) -> Self {
        Self {
            image: std::sync::Arc::new(image),
            width: None,
            height: None,
            transparent: false,
        }
    }

    pub(crate) fn from_shared(image: std::sync::Arc<DynamicImage>) -> Self {
        Self {
            image,
            width: None,
            height: None,
            transparent: false,
        }
    }

    /// Leave pixels under half opacity dotless and judge the rest by their
    /// own brightness, as [`ImageBackground::TerminalDefault`] promises.
    ///
    /// [`ImageBackground::TerminalDefault`]: crate::ImageBackground::TerminalDefault
    pub(crate) fn keep_transparency(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.max(1));
        self
    }

    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height.max(1));
        self
    }

    /// The grid for the available width, with derived rows also capped by `max_rows`, and the whole
    /// of it by [`MAX_CELLS`](crate::image_art::MAX_CELLS).
    fn grid_within(&self, available: usize, max_rows: Option<usize>) -> (usize, usize) {
        let (iw, ih) = self.image.dimensions();
        let mut columns = self.width.unwrap_or(available).max(1);
        let mut rows = ((ih as f64 * columns as f64) / (iw.max(1) as f64 * 2.0)).ceil() as usize;
        rows = rows.max(1);
        if let Some(cap) = self.height {
            if rows > cap.max(1) {
                let factor = cap.max(1) as f64 / rows as f64;
                columns = ((columns as f64 * factor).round() as usize).max(1);
                rows = cap.max(1);
            }
        }
        crate::image_art::bound_grid(columns, rows, true, max_rows)
    }

    fn rows(&self, available: usize, max_rows: Option<usize>) -> Vec<String> {
        let (columns, rows) = self.grid_within(available, max_rows);
        let mut scaled = self
            .image
            .resize_exact(
                (columns * 2) as u32,
                (rows * 4) as u32,
                FilterType::Triangle,
            )
            .to_rgba8();
        // Kept transparency: clear pixels are marked, the rest made opaque.
        let clear = crate::image_art::clear_mask(&mut scaled, self.transparent);
        let width = scaled.width() as usize;
        (0..rows)
            .map(|row| {
                (0..columns)
                    .map(|column| {
                        let mut bits = 0u8;
                        for (dy, dots) in DOTS.iter().enumerate() {
                            for (dx, dot) in dots.iter().enumerate() {
                                let x = column * 2 + dx;
                                let y = row * 4 + dy;
                                let [red, green, blue, alpha] =
                                    scaled.get_pixel(x as u32, y as u32).0;
                                let luminance = (0.299 * f32::from(red)
                                    + 0.587 * f32::from(green)
                                    + 0.114 * f32::from(blue))
                                    * (f32::from(alpha) / 255.0);
                                let hidden = clear.as_ref().is_some_and(|c| c[y * width + x]);
                                if luminance >= 128.0 && !hidden {
                                    bits |= 1 << dot;
                                }
                            }
                        }
                        char::from_u32(0x2800 + u32::from(bits)).unwrap_or('\u{2800}')
                    })
                    .collect()
            })
            .collect()
    }

    pub fn to_text(&self, width: usize) -> String {
        self.rows(width, None).join("\n")
    }
}

impl Renderable for BrailleArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.rows(options.max_width, options.height);
        let mut out = Vec::new();
        for (index, row) in rows.into_iter().enumerate() {
            if index > 0 {
                out.push(Segment::line());
            }
            out.push(Segment::new(row, None));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn maps_bright_pixels_to_braille_dots() {
        let mut image = RgbImage::from_pixel(2, 4, Rgb([0, 0, 0]));
        image.put_pixel(0, 0, Rgb([255, 255, 255]));
        let art = BrailleArt::new(DynamicImage::ImageRgb8(image))
            .width(1)
            .height(1);
        assert_eq!(art.to_text(1), "\u{2801}");
    }

    #[test]
    fn transparent_white_pixels_are_not_rendered() {
        let image = image::RgbaImage::from_pixel(2, 4, image::Rgba([255, 255, 255, 0]));
        let art = BrailleArt::new(DynamicImage::ImageRgba8(image))
            .width(1)
            .height(1);
        assert_eq!(art.to_text(1), "\u{2800}");
    }
}

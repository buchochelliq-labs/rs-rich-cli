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
}

impl BrailleArt {
    pub fn new(image: DynamicImage) -> Self {
        Self {
            image: std::sync::Arc::new(image),
            width: None,
            height: None,
        }
    }

    pub(crate) fn from_shared(image: std::sync::Arc<DynamicImage>) -> Self {
        Self {
            image,
            width: None,
            height: None,
        }
    }

    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.max(1));
        self
    }

    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height.max(1));
        self
    }

    fn grid(&self, available: usize) -> (usize, usize) {
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
        (columns, rows)
    }

    fn rows(&self, available: usize) -> Vec<String> {
        let (columns, rows) = self.grid(available);
        let scaled = self
            .image
            .resize_exact(
                (columns * 2) as u32,
                (rows * 4) as u32,
                FilterType::Triangle,
            )
            .to_luma8();
        (0..rows)
            .map(|row| {
                (0..columns)
                    .map(|column| {
                        let mut bits = 0u8;
                        for (dy, dots) in DOTS.iter().enumerate() {
                            for (dx, dot) in dots.iter().enumerate() {
                                let x = column * 2 + dx;
                                let y = row * 4 + dy;
                                if scaled.get_pixel(x as u32, y as u32)[0] >= 128 {
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
        self.rows(width).join("\n")
    }
}

impl Renderable for BrailleArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.rows(options.max_width);
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
}

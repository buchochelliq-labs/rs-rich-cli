//! Images rendered as ASCII / ANSI art, in the spirit of `jp2a`.
//!
//! Each output cell samples the image and picks a character from a density
//! ramp by luminance; with [`AsciiArt::color`] the cell also carries the
//! sampled colour, giving ANSI art rather than plain ASCII.
//!
//! Requires the non-default `image` feature.

use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageError, Rgba};

use rich::color::Color;
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;

/// `jp2a`'s default density ramp, darkest first.
pub const DEFAULT_RAMP: &str = "   ...',;:clodxkO0KXNWM";

/// Terminal cells are roughly twice as tall as they are wide, so an image
/// sampled 1:1 would come out stretched vertically. Rows are scaled by this.
const CELL_ASPECT: f64 = 2.0;

/// A foreground-only style for a sampled pixel.
fn fg_style(color: Color) -> Style {
    Style::from_color(Some(color), None)
}

/// An image rendered as ASCII (or ANSI) art.
pub struct AsciiArt {
    image: DynamicImage,
    width: Option<usize>,
    height: Option<usize>,
    ramp: Vec<char>,
    invert: bool,
    color: bool,
}

impl AsciiArt {
    /// Build from an already-decoded image.
    pub fn new(image: DynamicImage) -> Self {
        AsciiArt {
            image,
            width: None,
            height: None,
            ramp: DEFAULT_RAMP.chars().collect(),
            invert: false,
            color: false,
        }
    }

    /// Decode an image from bytes (PNG or JPEG).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ImageError> {
        Ok(AsciiArt::new(image::load_from_memory(bytes)?))
    }

    /// Decode an image from a file.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, ImageError> {
        Ok(AsciiArt::new(image::open(path)?))
    }

    /// Render this many columns (default: the console width).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }

    /// Render this many rows (default: derived from the image's aspect ratio).
    pub fn height(mut self, height: usize) -> Self {
        self.height = Some(height);
        self
    }

    /// Use a custom density ramp, darkest character first.
    pub fn ramp(mut self, ramp: impl Into<String>) -> Self {
        let ramp: String = ramp.into();
        if !ramp.is_empty() {
            self.ramp = ramp.chars().collect();
        }
        self
    }

    /// Swap dark and light — for light-on-dark terminals. Port of `jp2a
    /// --invert`.
    pub fn invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }

    /// Colour each cell with the sampled pixel (ANSI art). Port of `jp2a
    /// --colors`.
    pub fn color(mut self, color: bool) -> Self {
        self.color = color;
        self
    }

    /// The output grid for a given available width.
    fn grid(&self, available: usize) -> (usize, usize) {
        let (image_width, image_height) = self.image.dimensions();
        let columns = self.width.unwrap_or(available).max(1);
        let rows = self.height.unwrap_or_else(|| {
            if image_width == 0 {
                return 1;
            }
            let scaled =
                (image_height as f64 * columns as f64) / (image_width as f64 * CELL_ASPECT);
            (scaled.round() as usize).max(1)
        });
        (columns, rows)
    }

    /// Rec. 601 luma, the same weighting `jp2a` uses to grey-scale a pixel.
    fn luminance(pixel: Rgba<u8>) -> f64 {
        let [r, g, b, a] = pixel.0;
        let luma = 0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64;
        // Treat transparency as background (black), so cut-outs read as empty.
        luma * (a as f64 / 255.0)
    }

    /// Map a 0-255 luminance onto the ramp.
    fn glyph(&self, luma: f64) -> char {
        let normalised = (luma / 255.0).clamp(0.0, 1.0);
        let normalised = if self.invert {
            1.0 - normalised
        } else {
            normalised
        };
        let last = self.ramp.len() - 1;
        let index = (normalised * last as f64).round() as usize;
        self.ramp[index.min(last)]
    }

    /// Render to rows of `(char, colour)` pairs.
    fn cells(&self, available: usize) -> Vec<Vec<(char, Option<Color>)>> {
        let (columns, rows) = self.grid(available);
        // One resize does the sampling; nearest keeps it cheap and predictable.
        let scaled = self
            .image
            .resize_exact(columns as u32, rows as u32, FilterType::Triangle)
            .to_rgba8();

        (0..rows)
            .map(|y| {
                (0..columns)
                    .map(|x| {
                        let pixel = *scaled.get_pixel(x as u32, y as u32);
                        let glyph = self.glyph(Self::luminance(pixel));
                        let colour = if self.color {
                            let [r, g, b, _] = pixel.0;
                            Some(Color::from_rgb(r, g, b))
                        } else {
                            None
                        };
                        (glyph, colour)
                    })
                    .collect()
            })
            .collect()
    }

    /// The art as plain text, without going through a console.
    pub fn to_text(&self, width: usize) -> String {
        let mut out = String::new();
        for (index, row) in self.cells(width).iter().enumerate() {
            if index > 0 {
                out.push('\n');
            }
            out.extend(row.iter().map(|(glyph, _)| *glyph));
        }
        out
    }
}

impl Renderable for AsciiArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let rows = self.cells(options.max_width);
        let mut segments = Vec::new();
        let last = rows.len().saturating_sub(1);
        for (index, row) in rows.into_iter().enumerate() {
            if self.color {
                // Coalesce runs of the same colour into one segment.
                let mut run = String::new();
                let mut run_colour: Option<Color> = None;
                let mut started = false;
                for (glyph, colour) in row {
                    if started && colour != run_colour {
                        segments.push(Segment::new(
                            std::mem::take(&mut run),
                            run_colour.clone().map(fg_style),
                        ));
                    }
                    run_colour = colour;
                    started = true;
                    run.push(glyph);
                }
                if !run.is_empty() {
                    segments.push(Segment::new(run, run_colour.map(fg_style)));
                }
            } else {
                segments.push(Segment::new(
                    row.into_iter().map(|(glyph, _)| glyph).collect::<String>(),
                    None,
                ));
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

    /// A solid image of one colour.
    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb(rgb)))
    }

    #[test]
    fn black_and_white_hit_the_ends_of_the_ramp() {
        let ramp: Vec<char> = DEFAULT_RAMP.chars().collect();
        let dark = AsciiArt::new(solid(8, 8, [0, 0, 0])).width(4).height(2);
        assert_eq!(
            dark.to_text(4),
            format!("{0}{0}{0}{0}\n{0}{0}{0}{0}", ramp[0])
        );

        let light = AsciiArt::new(solid(8, 8, [255, 255, 255]))
            .width(4)
            .height(1);
        let brightest = ramp[ramp.len() - 1];
        assert_eq!(
            light.to_text(4),
            format!("{brightest}{brightest}{brightest}{brightest}")
        );
    }

    #[test]
    fn invert_swaps_the_ends() {
        let ramp: Vec<char> = DEFAULT_RAMP.chars().collect();
        let art = AsciiArt::new(solid(4, 4, [0, 0, 0]))
            .width(2)
            .height(1)
            .invert(true);
        let brightest = ramp[ramp.len() - 1];
        assert_eq!(art.to_text(2), format!("{brightest}{brightest}"));
    }

    #[test]
    fn grid_corrects_for_cell_aspect() {
        // A square image at 20 columns should be ~10 rows, not 20 — terminal
        // cells are about twice as tall as wide.
        let art = AsciiArt::new(solid(100, 100, [128, 128, 128]));
        assert_eq!(art.grid(20), (20, 10));
        // A 2:1 landscape image halves again.
        let wide = AsciiArt::new(solid(100, 50, [128, 128, 128]));
        assert_eq!(wide.grid(20), (20, 5));
    }

    #[test]
    fn explicit_height_wins() {
        let art = AsciiArt::new(solid(100, 100, [0, 0, 0])).height(3);
        assert_eq!(art.grid(20), (20, 3));
        assert_eq!(art.to_text(20).lines().count(), 3);
    }

    #[test]
    fn a_custom_ramp_is_used() {
        let art = AsciiArt::new(solid(4, 4, [255, 255, 255]))
            .width(3)
            .height(1)
            .ramp(".#");
        assert_eq!(art.to_text(3), "###");
    }

    #[test]
    fn colored_art_emits_styled_segments() {
        use rich::color::ColorSystem;
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(4)
            .no_color(false)
            .build();
        let art = AsciiArt::new(solid(4, 4, [255, 0, 0]))
            .width(4)
            .height(1)
            .color(true);
        let out = console.render_to_string(&art);
        assert!(out.contains("38;2;255;0;0"), "expected red fg, got {out:?}");
    }

    #[test]
    fn transparent_pixels_read_as_dark() {
        let ramp: Vec<char> = DEFAULT_RAMP.chars().collect();
        let mut rgba = image::RgbaImage::from_pixel(4, 4, Rgba([255, 255, 255, 255]));
        for pixel in rgba.pixels_mut() {
            *pixel = Rgba([255, 255, 255, 0]); // fully transparent white
        }
        let art = AsciiArt::new(DynamicImage::ImageRgba8(rgba))
            .width(2)
            .height(1);
        assert_eq!(art.to_text(2), format!("{0}{0}", ramp[0]));
    }
}

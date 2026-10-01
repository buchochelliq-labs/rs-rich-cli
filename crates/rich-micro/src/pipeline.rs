//! The image and animation pipeline (#571, #572): from any picture to the
//! images a package holds, fitted to its cells.
//!
//! A source image (PNG, APNG, GIF or JPEG) is decoded under hard limits,
//! then every frame goes through the same steps, all of them `rich-art`'s
//! image code: the adjustments (brightness, contrast, gamma, grayscale),
//! fitting to exactly `cols × cell width` by `rows × cell height` pixels
//! (contain, cover with an anchor, or stretch), an optional unsharp mask,
//! transparency (kept, cut at a threshold, flattened onto a colour, or made
//! from a key colour), and optionally a reduced palette with dithering. An
//! animation is then deduplicated and rate-limited.
//!
//! The result, [`Processed`], is what a package stores ([`crate::create`]),
//! and what [`preview`](Processed::preview) shows magnified: the asset at
//! the cell size it will be drawn at, one image pixel per half-block.
//!
//! ```
//! use rich_art::image::{DynamicImage, Rgba, RgbaImage};
//! use rich_micro::pipeline::Pipeline;
//! use rich_micro::CellSize;
//!
//! let source = DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 64, Rgba([0, 160, 0, 255])));
//! let processed = Pipeline::new(CellSize::default()).process_image(&source);
//! // 2x1 cells of 8x16 pixels: a 16x16 image.
//! assert_eq!(processed.still().dimensions(), (16, 16));
//! assert!(!processed.animated());
//! ```

use std::io::Cursor;

use rich::{Console, ConsoleOptions, Renderable, Segment};
use rich_art::graphics::{
    adjust, decode_animation, encode_gif, encode_png, fill_pixels, reduce_colors, resample,
    sharpen, threshold_alpha, AnimationFrame, CellPixels, FrameBudget,
};
use rich_art::image::{self, DynamicImage, ImageReader, RgbaImage};
use rich_art::{
    BlockArt, ColorDistance, Dither, ImageAnchor, ImageColorMode, ImageFit, ImageTransforms,
};

use crate::error::MicroError;
use crate::model::CellSize;

/// The largest source file [`Pipeline::process_bytes`] reads: authoring
/// inputs may be much larger than a package's images.
pub const MAX_SOURCE_BYTES: usize = 32 << 20;
/// The largest source image side, in pixels.
pub const MAX_SOURCE_DIMENSION: u32 = 8192;

/// What happens to transparency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transparency {
    /// Keep the alpha channel as it is (soft edges stay soft in the PNG;
    /// the GIF animation still cuts at half).
    Keep,
    /// Every pixel opaque or transparent, cut at this alpha (the default,
    /// 128): what GIF and most terminal protocols draw.
    Threshold(u8),
    /// Composite onto this colour: no transparency left.
    Flatten([u8; 3]),
    /// Pixels within a small distance of this colour become transparent
    /// (for sources with a plain background and no alpha), then cut at half.
    Key([u8; 3]),
}

impl Default for Transparency {
    fn default() -> Self {
        Transparency::Threshold(128)
    }
}

/// How far (per channel) a pixel may be from [`Transparency::Key`] and
/// still count as the key.
const KEY_TOLERANCE: u8 = 24;

/// The pipeline's settings. [`Pipeline::new`] gives the defaults: contain,
/// no adjustment, no sharpening, alpha cut at half, true colour, frames at
/// most 25 a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pipeline {
    /// The asset's size in cells.
    pub size: CellSize,
    /// Pixels per cell the images are made for. The default, 8×16, is the
    /// size terminals draw at when they do not say; a package made for it
    /// is scaled by the terminal (or the cache) for other cell sizes.
    pub cell: CellPixels,
    pub fit: ImageFit,
    /// What [`ImageFit::Cover`] keeps.
    pub anchor: ImageAnchor,
    /// Brightness, contrast, gamma, grayscale, rotation and flips, applied
    /// before fitting.
    pub transforms: ImageTransforms,
    /// An unsharp mask of this radius in pixels, after fitting.
    pub sharpen: Option<f32>,
    pub transparency: Transparency,
    /// A reduced palette (with `dither`), for assets that should look the
    /// same in 256 or 16 colours.
    pub colors: ImageColorMode,
    pub dither: Dither,
    /// Frames kept, their decoded size, and the shortest frame time.
    pub budget: FrameBudget,
}

impl Pipeline {
    pub fn new(size: CellSize) -> Pipeline {
        Pipeline {
            size,
            cell: CellPixels::DEFAULT,
            fit: ImageFit::Contain,
            anchor: ImageAnchor::Center,
            transforms: ImageTransforms::default(),
            sharpen: None,
            transparency: Transparency::default(),
            colors: ImageColorMode::TrueColor,
            dither: Dither::None,
            budget: FrameBudget {
                max_bytes: 256 << 20,
                max_frames: 240,
                ..FrameBudget::default()
            },
        }
    }

    /// The canvas every frame is fitted to, in pixels.
    pub fn canvas(&self) -> (u32, u32) {
        self.cell.span(self.size.cols(), self.size.rows())
    }

    /// Check the settings: adjustments in range and a positive sharpening
    /// radius.
    pub fn validate(&self) -> Result<(), MicroError> {
        if !self.transforms.adjustments_valid() {
            return Err(MicroError::Image(
                "brightness and contrast must be finite and not negative, gamma finite and \
                 positive"
                    .into(),
            ));
        }
        if let Some(sigma) = self.sharpen {
            if !sigma.is_finite() || sigma <= 0.0 || sigma > 10.0 {
                return Err(MicroError::Image(format!(
                    "sharpen radius {sigma} is not between 0 and 10 pixels"
                )));
            }
        }
        Ok(())
    }

    /// Decode `bytes` (PNG, APNG, GIF or JPEG) and run every frame through
    /// the pipeline. A still source gives one frame.
    pub fn process_bytes(&self, bytes: &[u8]) -> Result<Processed, MicroError> {
        self.validate()?;
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err(MicroError::Limit(format!(
                "the source image is larger than {MAX_SOURCE_BYTES} bytes"
            )));
        }
        let frames = decode(bytes, self.budget)?;
        Ok(self.process_frames(&frames))
    }

    /// Run decoded frames through the pipeline.
    pub fn process_frames(&self, frames: &[AnimationFrame]) -> Processed {
        let fitted: Vec<AnimationFrame> = frames
            .iter()
            .take(self.budget.max_frames.max(1))
            .map(|frame| AnimationFrame {
                image: self.frame(&DynamicImage::ImageRgba8(frame.image.clone())),
                delay: frame.delay,
            })
            .collect();
        let (width, height) = self.canvas();
        // Already at the canvas size: this only deduplicates and
        // rate-limits.
        let frames = if fitted.len() > 1 {
            resample(&fitted, width, height, self.budget)
        } else {
            fitted
        };
        Processed {
            frames,
            size: self.size,
            cell: self.cell,
        }
    }

    /// Run one still image through the pipeline.
    pub fn process_image(&self, image: &DynamicImage) -> Processed {
        Processed {
            frames: vec![AnimationFrame {
                image: self.frame(image),
                delay: std::time::Duration::from_millis(100),
            }],
            size: self.size,
            cell: self.cell,
        }
    }

    /// One frame: adjust, fit, sharpen, transparency, palette.
    fn frame(&self, image: &DynamicImage) -> RgbaImage {
        let adjusted = if self.transforms == ImageTransforms::default() {
            image.clone()
        } else {
            adjust(image, self.transforms)
        };
        let adjusted = match self.transparency {
            Transparency::Key(key) => DynamicImage::ImageRgba8(key_out(&adjusted.to_rgba8(), key)),
            _ => adjusted,
        };
        let (width, height) = self.canvas();
        let mut fitted = fill_pixels(&adjusted, width, height, self.fit, self.anchor);
        if let Some(sigma) = self.sharpen {
            fitted = sharpen(&fitted, sigma, 1);
        }
        match self.transparency {
            Transparency::Keep => {}
            Transparency::Threshold(cutoff) => threshold_alpha(&mut fitted, cutoff),
            Transparency::Key(_) => threshold_alpha(&mut fitted, 128),
            Transparency::Flatten(background) => flatten(&mut fitted, background),
        }
        reduce_colors(&mut fitted, self.colors, self.dither, ColorDistance::Rgb);
        fitted
    }
}

/// Decode a source image under `budget` and the source limits.
fn decode(bytes: &[u8], budget: FrameBudget) -> Result<Vec<AnimationFrame>, MicroError> {
    let bad = |e: image::ImageError| MicroError::Image(format!("cannot decode the image: {e}"));
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| MicroError::Image(format!("cannot read the image: {e}")))?;
    let (width, height) = reader.into_dimensions().map_err(bad)?;
    if width > MAX_SOURCE_DIMENSION || height > MAX_SOURCE_DIMENSION {
        return Err(MicroError::Limit(format!(
            "the source image is {width}x{height} pixels; the limit is \
             {MAX_SOURCE_DIMENSION} on each side"
        )));
    }
    if bytes.starts_with(b"GIF8") || bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let frames = decode_animation(bytes, budget).map_err(bad)?;
        if frames.is_empty() {
            return Err(MicroError::Image("the image has no frames".into()));
        }
        return Ok(frames);
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| MicroError::Image(format!("cannot read the image: {e}")))?;
    reader.limits(image::Limits::default());
    let image = reader.decode().map_err(bad)?;
    Ok(vec![AnimationFrame {
        image: image.to_rgba8(),
        delay: std::time::Duration::from_millis(100),
    }])
}

/// Pixels close to `key` made transparent.
fn key_out(image: &RgbaImage, key: [u8; 3]) -> RgbaImage {
    let mut out = image.clone();
    for pixel in out.pixels_mut() {
        let near = (0..3).all(|c| pixel.0[c].abs_diff(key[c]) <= KEY_TOLERANCE);
        if near {
            pixel.0 = [0, 0, 0, 0];
        }
    }
    out
}

/// Composite onto `background`: every pixel opaque.
fn flatten(image: &mut RgbaImage, background: [u8; 3]) {
    for pixel in image.pixels_mut() {
        composite(&mut pixel.0, background);
    }
}

/// One RGBA pixel over an opaque `under`: opaque.
fn composite(pixel: &mut [u8; 4], under: [u8; 3]) {
    let alpha = u32::from(pixel[3]);
    for (channel, under) in pixel.iter_mut().zip(under) {
        *channel =
            ((u32::from(*channel) * alpha + u32::from(under) * (255 - alpha) + 127) / 255) as u8;
    }
    pixel[3] = 255;
}

/// What the pipeline made: frames of exactly the asset's canvas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Processed {
    pub frames: Vec<AnimationFrame>,
    pub size: CellSize,
    pub cell: CellPixels,
}

impl Processed {
    /// Whether it moves.
    pub fn animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// The still image: the first frame.
    pub fn still(&self) -> &RgbaImage {
        &self.frames[0].image
    }

    /// The still image as PNG bytes.
    pub fn png(&self) -> Result<Vec<u8>, MicroError> {
        encode_png(self.still()).ok_or_else(|| MicroError::Image("cannot encode a PNG".into()))
    }

    /// The animation as a looping GIF, when there is one.
    pub fn gif(&self) -> Result<Option<Vec<u8>>, MicroError> {
        if !self.animated() {
            return Ok(None);
        }
        encode_gif(&self.frames)
            .map(Some)
            .ok_or_else(|| MicroError::Image("cannot encode a GIF".into()))
    }

    /// The still image magnified: one image pixel per half-block cell, so a
    /// 16×16 asset is 16 columns by 8 rows. Needs a colour terminal to show
    /// anything but blocks.
    pub fn preview(&self) -> Magnified {
        Magnified(self.still().clone())
    }
}

/// A fitted image drawn one pixel per half-block: see
/// [`Processed::preview`].
#[derive(Clone, Debug)]
pub struct Magnified(pub RgbaImage);

impl Magnified {
    /// Columns it takes: one per pixel.
    pub fn cols(&self) -> usize {
        self.0.width() as usize
    }
}

impl Renderable for Magnified {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        // Transparent pixels over a two-tone checkerboard, as image editors
        // show them, so the asset's outline is visible on any background.
        let mut board = self.0.clone();
        for (x, y, pixel) in board.enumerate_pixels_mut() {
            let under: [u8; 3] = if (x / 2 + y / 2) % 2 == 0 {
                [52, 52, 52]
            } else {
                [36, 36, 36]
            };
            composite(&mut pixel.0, under);
        }
        BlockArt::new(DynamicImage::ImageRgba8(board))
            .width(self.cols().min(options.max_width))
            .rich_render(console, options)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> rich::measure::Measurement {
        let cols = self.cols().min(options.max_width);
        rich::measure::Measurement::new(cols, cols)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_art::image::Rgba;
    use std::time::Duration;

    fn frame(color: [u8; 4], ms: u64) -> AnimationFrame {
        AnimationFrame {
            image: RgbaImage::from_pixel(32, 32, Rgba(color)),
            delay: Duration::from_millis(ms),
        }
    }

    #[test]
    fn fits_every_frame_to_the_canvas_and_dedupes() {
        let pipeline = Pipeline::new(CellSize::default());
        let out = pipeline.process_frames(&[
            frame([255, 0, 0, 255], 100),
            frame([255, 0, 0, 255], 100),
            frame([0, 0, 255, 255], 100),
        ]);
        assert_eq!(out.frames.len(), 2);
        assert_eq!(out.frames[0].delay, Duration::from_millis(200));
        assert!(out.frames.iter().all(|f| f.image.dimensions() == (16, 16)));
        let gif = out.gif().unwrap().unwrap();
        assert!(gif.starts_with(b"GIF89a"));
        let one = Pipeline::new(CellSize::new(1, 1).unwrap()).process_frames(&[frame([1; 4], 1)]);
        assert_eq!(one.still().dimensions(), (8, 16));
    }

    #[test]
    fn transparency_modes() {
        let mut source = RgbaImage::from_pixel(16, 16, Rgba([255, 255, 255, 255]));
        source.put_pixel(8, 8, Rgba([200, 0, 0, 255]));
        let source = DynamicImage::ImageRgba8(source);
        let mut pipeline = Pipeline::new(CellSize::default());
        pipeline.transparency = Transparency::Key([255, 255, 255]);
        let keyed = pipeline.process_image(&source);
        assert_eq!(keyed.still().get_pixel(0, 0).0[3], 0);
        assert_eq!(keyed.still().get_pixel(8, 8).0, [200, 0, 0, 255]);
        pipeline.transparency = Transparency::Flatten([0, 0, 0]);
        let half = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 128])));
        let flat = pipeline.process_image(&half);
        assert_eq!(flat.still().get_pixel(3, 3).0[3], 255);
        assert!(flat.still().get_pixel(3, 3).0[0] < 200);
    }

    #[test]
    fn decodes_png_and_rejects_nonsense() {
        let png = encode_png(&RgbaImage::from_pixel(40, 20, Rgba([0, 200, 0, 255]))).unwrap();
        let mut pipeline = Pipeline::new(CellSize::default());
        pipeline.fit = ImageFit::Cover;
        pipeline.sharpen = Some(0.8);
        pipeline.transforms.contrast = 1.2;
        let out = pipeline.process_bytes(&png).unwrap();
        assert_eq!(out.still().dimensions(), (16, 16));
        assert!(out.still().pixels().all(|p| p.0[3] == 255));
        assert!(pipeline.process_bytes(b"not an image").is_err());
        pipeline.sharpen = Some(-1.0);
        assert!(pipeline.process_bytes(&png).is_err());
    }

    #[test]
    fn preview_is_one_column_per_pixel() {
        let processed = Pipeline::new(CellSize::default()).process_image(
            &DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]))),
        );
        let console = Console::builder().width(40).build();
        let lines = console.render_lines(&processed.preview(), &console.options(), false);
        assert_eq!(lines.len(), 8);
        assert!(lines
            .iter()
            .all(|line| line.iter().map(Segment::cell_length).sum::<usize>() == 16));
    }
}

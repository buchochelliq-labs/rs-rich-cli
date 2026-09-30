//! Shared pieces of the terminal graphics protocols: images fitted to an
//! exact number of cells, animation frames resampled for them, and the
//! overlay that draws an image over cells a line already holds.
//!
//! Kitty ([`crate::kitty`]), iTerm2 ([`crate::iterm`]) and inline Sixel
//! ([`crate::sixel::SixelArt::encode_inline`]) all start here: a protocol
//! image that must take exactly `cols × rows` cells is drawn from a raster of
//! exactly `cols × cell width` by `rows × cell height` pixels, so the
//! terminal never has to scale it and never lets it spill into the next cell.
//!
//! Requires the `image` feature.

use std::io::Cursor;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::imageops::FilterType;
use image::{AnimationDecoder, DynamicImage, Frame, ImageDecoder, ImageError, RgbaImage};

/// The size of one terminal cell in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellPixels {
    pub width: u32,
    pub height: u32,
}

impl CellPixels {
    /// The conventional guess when the terminal did not say: 8×16, the classic
    /// VGA cell (the same guess [`crate::sixel::DEFAULT_CELL_PX`] makes).
    pub const DEFAULT: CellPixels = CellPixels {
        width: 8,
        height: 16,
    };

    /// A cell size, each side at least 1 and at most 512 pixels.
    pub fn new(width: u32, height: u32) -> CellPixels {
        CellPixels {
            width: width.clamp(1, 512),
            height: height.clamp(1, 512),
        }
    }

    /// The pixel size of `cols × rows` cells.
    pub fn span(self, cols: usize, rows: usize) -> (u32, u32) {
        let cols = u32::try_from(cols.clamp(1, 4096)).unwrap_or(4096);
        let rows = u32::try_from(rows.clamp(1, 4096)).unwrap_or(4096);
        (self.width * cols, self.height * rows)
    }
}

impl Default for CellPixels {
    fn default() -> Self {
        CellPixels::DEFAULT
    }
}

/// `image` scaled to fit `cols × rows` cells of `cell` pixels, keeping its
/// aspect ratio, centred on a transparent canvas of exactly that size.
pub fn fit_to_cells(image: &DynamicImage, cols: usize, rows: usize, cell: CellPixels) -> RgbaImage {
    let (width, height) = cell.span(cols, rows);
    fit_to_pixels(image, width, height)
}

/// `image` scaled to fit `width × height` pixels, keeping its aspect ratio,
/// centred on a transparent canvas of exactly that size.
pub fn fit_to_pixels(image: &DynamicImage, width: u32, height: u32) -> RgbaImage {
    let (iw, ih) = (image.width().max(1), image.height().max(1));
    // The largest size of the same shape that fits.
    let scale = f64::min(width as f64 / iw as f64, height as f64 / ih as f64);
    let fw = ((iw as f64 * scale).round() as u32).clamp(1, width);
    let fh = ((ih as f64 * scale).round() as u32).clamp(1, height);
    let filter = if scale < 1.0 {
        FilterType::Lanczos3
    } else {
        // Upscaling a tiny icon: keep its pixels crisp.
        FilterType::Nearest
    };
    let scaled = image.resize_exact(fw, fh, filter).to_rgba8();
    let mut canvas = RgbaImage::new(width, height);
    let (x, y) = ((width - fw) / 2, (height - fh) / 2);
    image::imageops::overlay(&mut canvas, &scaled, x as i64, y as i64);
    canvas
}

/// One frame of an animation, as a full canvas, and how long it shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationFrame {
    pub image: RgbaImage,
    pub delay: Duration,
}

/// Bounds on decoding and resampling an animation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameBudget {
    /// Decoded bytes all frames may take together, at canvas size.
    pub max_bytes: usize,
    /// Frames kept at most; later frames are dropped.
    pub max_frames: usize,
    /// The shortest time a frame shows: faster frames are merged into the
    /// next, so an animation never repaints faster than this.
    pub min_delay: Duration,
}

impl Default for FrameBudget {
    fn default() -> Self {
        FrameBudget {
            max_bytes: 64 * 1024 * 1024,
            max_frames: 256,
            // 25 frames a second.
            min_delay: Duration::from_millis(40),
        }
    }
}

/// A browser's substitute for a GIF's "as fast as you can" delay of zero.
const ZERO_DELAY: Duration = Duration::from_millis(100);

fn over_budget() -> ImageError {
    ImageError::Limits(image::error::LimitError::from_kind(
        image::error::LimitErrorKind::InsufficientMemory,
    ))
}

/// Decode an animated GIF or APNG into full-canvas frames under `budget`.
/// A still PNG decodes to one frame. Frames past `budget.max_frames` are
/// dropped; decoding stops with an error when the frames so far take more
/// than `budget.max_bytes`.
pub fn decode_animation(bytes: &[u8], budget: FrameBudget) -> Result<Vec<AnimationFrame>, ImageError> {
    let frames: Box<dyn Iterator<Item = Result<Frame, ImageError>>> = if bytes.starts_with(b"GIF8")
    {
        let mut decoder = GifDecoder::new(Cursor::new(bytes))?;
        decoder.set_limits(image::Limits::default())?;
        Box::new(decoder.into_frames())
    } else {
        let mut decoder = PngDecoder::new(Cursor::new(bytes))?;
        decoder.set_limits(image::Limits::default())?;
        if decoder.is_apng()? {
            Box::new(decoder.apng()?.into_frames())
        } else {
            let image = DynamicImage::from_decoder(decoder)?.to_rgba8();
            return Ok(vec![AnimationFrame {
                image,
                delay: ZERO_DELAY,
            }]);
        }
    };
    let mut used = 0usize;
    let mut out = Vec::new();
    for frame in frames.take(budget.max_frames.max(1)) {
        let frame = frame?;
        let (numer, denom) = frame.delay().numer_denom_ms();
        let millis = if denom == 0 { 0 } else { numer / denom };
        let buffer = frame.into_buffer();
        let size = (buffer.width() as usize)
            .checked_mul(buffer.height() as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(over_budget)?;
        used = used.checked_add(size).ok_or_else(over_budget)?;
        if used > budget.max_bytes {
            return Err(over_budget());
        }
        out.push(AnimationFrame {
            image: buffer,
            delay: if millis == 0 {
                ZERO_DELAY
            } else {
                Duration::from_millis(u64::from(millis))
            },
        });
    }
    Ok(out)
}

/// `frames` fitted to `width × height` pixels, then deduplicated (a frame
/// equal to the one before it only lengthens that one) and rate-limited (a
/// frame shorter than `budget.min_delay` is merged into the next). The
/// result always holds at least one frame when `frames` does.
pub fn resample(
    frames: &[AnimationFrame],
    width: u32,
    height: u32,
    budget: FrameBudget,
) -> Vec<AnimationFrame> {
    let mut out: Vec<AnimationFrame> = Vec::new();
    // Time owed by frames that were too short to show on their own.
    let mut carried = Duration::ZERO;
    for (index, frame) in frames.iter().enumerate() {
        let delay = frame.delay + carried;
        let last = index + 1 == frames.len();
        if delay < budget.min_delay && !last {
            carried = delay;
            continue;
        }
        carried = Duration::ZERO;
        let image = fit_to_pixels(&DynamicImage::ImageRgba8(frame.image.clone()), width, height);
        match out.last_mut() {
            Some(previous) if previous.image == image => previous.delay += delay,
            _ => out.push(AnimationFrame { image, delay }),
        }
    }
    out
}

/// `image` as PNG bytes.
pub fn encode_png(image: &RgbaImage) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// `frames` as an animated GIF that loops forever, or `None` when there is
/// nothing to encode.
pub fn encode_gif(frames: &[AnimationFrame]) -> Option<Vec<u8>> {
    use image::codecs::gif::{GifEncoder, Repeat};
    if frames.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    {
        let mut encoder = GifEncoder::new_with_speed(&mut out, 10);
        encoder.set_repeat(Repeat::Infinite).ok()?;
        let frames = frames.iter().map(|frame| {
            let millis = u32::try_from(frame.delay.as_millis()).unwrap_or(u32::MAX);
            Frame::from_parts(
                frame.image.clone(),
                0,
                0,
                image::Delay::from_numer_denom_ms(millis, 1),
            )
        });
        encoder.encode_frames(frames).ok()?;
    }
    Some(out)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding, as every graphics protocol wants it.
pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(BASE64[(n >> 18) as usize & 63] as char);
        out.push(BASE64[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            BASE64[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Save the cursor (DECSC).
pub const SAVE_CURSOR: &str = "\x1b7";
/// Restore the cursor (DECRC).
pub const RESTORE_CURSOR: &str = "\x1b8";

/// The two escapes that draw `image` (an escape that draws at the cursor and
/// may move it) over `cols` cells a line is about to print: write the first,
/// then the cells, then the second. The cursor is saved before the cells,
/// restored to draw the image over them, and restored again then moved
/// forward past them, so wherever the protocol leaves the cursor the line
/// continues right after the cells.
pub fn overlay(image: &str, cols: usize) -> (String, String) {
    let mut after = String::with_capacity(image.len() + 16);
    after.push_str(RESTORE_CURSOR);
    after.push_str(image);
    after.push_str(RESTORE_CURSOR);
    if cols > 0 {
        after.push_str(&format!("\x1b[{cols}C"));
    }
    (SAVE_CURSOR.to_string(), after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(input.as_bytes()), expected);
        }
    }

    #[test]
    fn fitting_gives_the_exact_canvas_and_centres() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])));
        let fitted = fit_to_cells(&image, 2, 1, CellPixels::new(8, 16));
        assert_eq!(fitted.dimensions(), (16, 16));
        let wide = fit_to_pixels(&image, 32, 16);
        assert_eq!(wide.dimensions(), (32, 16));
        // A square in a 2:1 box: transparent margins left and right.
        assert_eq!(wide.get_pixel(0, 8).0[3], 0);
        assert_eq!(wide.get_pixel(16, 8).0, [255, 0, 0, 255]);
    }

    #[test]
    fn resampling_dedupes_and_rate_limits() {
        let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
        let frame = |image: &RgbaImage, ms| AnimationFrame {
            image: image.clone(),
            delay: Duration::from_millis(ms),
        };
        let frames = [
            frame(&red, 100),
            frame(&red, 100),
            frame(&blue, 10),
            frame(&red, 50),
        ];
        let out = resample(&frames, 4, 4, FrameBudget::default());
        // red 200ms; blue's 10ms is carried into the red that follows it.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].delay, Duration::from_millis(260));
        let frames = [frame(&red, 100), frame(&blue, 100)];
        assert_eq!(resample(&frames, 4, 4, FrameBudget::default()).len(), 2);
    }

    #[test]
    fn overlay_saves_restores_and_steps_past_the_cells() {
        let (before, after) = overlay("IMG", 2);
        assert_eq!(before, "\x1b7");
        assert_eq!(after, "\x1b8IMG\x1b8\x1b[2C");
    }

    #[test]
    fn gif_round_trips_through_the_decoder() {
        let red = RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 255]));
        let blue = RgbaImage::from_pixel(2, 2, Rgba([0, 0, 255, 255]));
        let frames = vec![
            AnimationFrame {
                image: red,
                delay: Duration::from_millis(100),
            },
            AnimationFrame {
                image: blue,
                delay: Duration::from_millis(200),
            },
        ];
        let gif = encode_gif(&frames).expect("encodes");
        let decoded = decode_animation(&gif, FrameBudget::default()).expect("decodes");
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[1].delay, Duration::from_millis(200));
        let png = encode_png(&frames[0].image).expect("png");
        assert_eq!(decode_animation(&png, FrameBudget::default()).unwrap().len(), 1);
    }
}

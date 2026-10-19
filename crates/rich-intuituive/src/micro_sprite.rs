//! rs-rich-micro's micro assets as sprites: an emoji-sized image or
//! animation, fitted to its cells in half blocks, for a
//! [`stage`](crate::sprite::stage). Behind the off-by-default `micro`
//! feature.
//!
//! An asset with images is decoded and fitted by rs-rich-micro's
//! [`Pipeline`] at two pixels a cell (one across, two down), so a 2x1
//! asset is a 2x2 pixel sprite: an animation keeps its frames and their
//! times. An asset with no images is its fallback (the emoji or text),
//! as a terminal without graphics shows it.
//!
//! ```
//! use intuituive::micro_sprite::micro_sprite;
//! use rich_micro::MicroRegistry;
//!
//! let registry = MicroRegistry::builtin();
//! let loading = registry.require("status/loading").unwrap();
//! let sprite = micro_sprite(&loading).unwrap();
//! assert_eq!(sprite.size(), (loading.cols() as u16, loading.rows() as u16));
//! ```

use rich_micro::pipeline::Pipeline;
use rich_micro::{fallback_cells, CellPixels, FallbackPreference, Limits, MicroAsset, MicroError};

use crate::pixels::{rgba_pixels, PixelSprite};
use crate::sprite::Sprite;

/// `asset` as a sprite, its images read under the default [`Limits`].
pub fn micro_sprite(asset: &MicroAsset) -> Result<Sprite, MicroError> {
    micro_sprite_with(asset, &Limits::default())
}

/// `asset` as a sprite, its images read under `limits`.
pub fn micro_sprite_with(asset: &MicroAsset, limits: &Limits) -> Result<Sprite, MicroError> {
    match micro_pixels(asset, limits)? {
        Some(pixels) => Ok(Sprite::from_pixels(&pixels)),
        None => {
            let cells = fallback_cells(asset, FallbackPreference::Emoji);
            // The fallback is plain text: escape markup's brackets.
            let line = cells.replace('[', "\\[");
            Ok(Sprite::from_markup(&[line.as_str()], '\u{0}'))
        }
    }
}

/// How much of a pixel a shape must cover to be drawn: an eighth. An
/// icon fitted to two pixels a cell is mostly thin strokes averaged with
/// the transparent background; half (the usual cut) loses them.
const COVERED: u8 = 32;

/// A fitted pixel drawn opaque in its shape's colour when the shape
/// covers enough of it (the averaging with transparent black undone), or
/// transparent.
fn covered(pixel: [u8; 4]) -> [u8; 4] {
    let a = pixel[3];
    if a < COVERED {
        return [0, 0, 0, 0];
    }
    let undo = |c: u8| ((c as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8;
    [undo(pixel[0]), undo(pixel[1]), undo(pixel[2]), 255]
}

/// `asset`'s images fitted to its cells at two pixels a cell (its columns
/// by twice its rows): the animation, else the still image, else its
/// frames one after another; `None` when it has no images. A pixel an
/// eighth covered or more is opaque, in the colour of what covers it.
pub fn micro_pixels(
    asset: &MicroAsset,
    limits: &Limits,
) -> Result<Option<PixelSprite>, MicroError> {
    let mut pipeline = Pipeline::new(asset.size());
    pipeline.cell = CellPixels::new(1, 2);
    pipeline.transparency = rich_micro::pipeline::Transparency::Keep;
    let variants = asset.variants();
    let sources: Vec<_> = match (&variants.animation, &variants.static_image) {
        (Some(animation), _) => vec![animation],
        (None, Some(still)) => vec![still],
        (None, None) => variants.frames.iter().collect(),
    };
    let mut sprite: Option<PixelSprite> = None;
    let mut durations = Vec::new();
    for image in sources {
        let bytes = asset.read_variant(image, limits)?;
        let processed = pipeline.process_bytes(&bytes)?;
        for frame in &processed.frames {
            let (width, height) = (frame.image.width() as u16, frame.image.height() as u16);
            let pixels = rgba_pixels(frame.image.as_raw())
                .into_iter()
                .map(covered)
                .collect();
            sprite = Some(match sprite {
                None => PixelSprite::new(width, height, pixels),
                Some(sprite) => sprite.with_frame(pixels),
            });
            durations.push(frame.delay);
        }
    }
    Ok(sprite.map(|sprite| sprite.durations(&durations)))
}

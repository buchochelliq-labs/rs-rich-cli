//! Sprite sheets: an image cut into tiles, each a [`PixelSprite`] (for a
//! [`PixelBuf`](crate::pixels::PixelBuf)) or a half-block [`Sprite`] (for
//! a [`stage`](crate::sprite::stage)). Behind the off-by-default `art`
//! feature: the image is decoded by rs-rich-art's `image`.
//!
//! ```
//! use intuituive::sprite_sheet::sprite_sheet_bytes;
//! use std::time::Duration;
//!
//! // A 4x2 sheet of two 2x2 tiles: red, then blue.
//! let mut png = Vec::new();
//! let sheet = rich_art::image::RgbaImage::from_fn(4, 2, |x, _| {
//!     rich_art::image::Rgba(if x < 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] })
//! });
//! sheet
//!     .write_to(&mut std::io::Cursor::new(&mut png), rich_art::image::ImageFormat::Png)
//!     .unwrap();
//! let sheet = sprite_sheet_bytes(&png, 2, 2).unwrap();
//! assert_eq!((sheet.columns(), sheet.rows(), sheet.len()), (2, 1, 2));
//! assert_eq!(sheet.tile(1).frame(0)[0], [0, 0, 255, 255]);
//! // Both tiles as an animation, a frame each 100 ms.
//! let walk = sheet.animation(&[0, 1]).period(Duration::from_millis(100));
//! assert_eq!(walk.frame_at(Duration::from_millis(150)), 1);
//! assert_eq!(sheet.sprite(0).size(), (2, 1));
//! ```

use std::io;
use std::path::Path;

use rich_art::image::{DynamicImage, GenericImageView};

use crate::pixels::{rgba_pixels, PixelSprite};
use crate::sprite::Sprite;

/// The largest side of a sheet decoded, in pixels: a guard against a
/// file that would take gigabytes decoded.
const MAX_SIDE: u32 = 8192;

/// An image cut into equal tiles, left to right, top to bottom. A part
/// at the right or bottom edge too small for a whole tile is left out.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteSheet {
    columns: usize,
    rows: usize,
    tiles: Vec<PixelSprite>,
}

/// The image file at `path` cut into tiles of `tile_width` x
/// `tile_height` pixels.
pub fn sprite_sheet(
    path: impl AsRef<Path>,
    tile_width: u32,
    tile_height: u32,
) -> io::Result<SpriteSheet> {
    let bytes = std::fs::read(path)?;
    sprite_sheet_bytes(&bytes, tile_width, tile_height)
}

/// An image's bytes (PNG, GIF, JPEG: what rs-rich-art decodes) cut into
/// tiles of `tile_width` x `tile_height` pixels.
pub fn sprite_sheet_bytes(
    bytes: &[u8],
    tile_width: u32,
    tile_height: u32,
) -> io::Result<SpriteSheet> {
    let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
    let image = rich_art::image::load_from_memory(bytes).map_err(|e| invalid(e.to_string()))?;
    let (width, height) = image.dimensions();
    if width > MAX_SIDE || height > MAX_SIDE {
        return Err(invalid(format!(
            "a {width}x{height} sheet is larger than {MAX_SIDE} pixels a side"
        )));
    }
    Ok(SpriteSheet::from_image(&image, tile_width, tile_height))
}

impl SpriteSheet {
    /// A decoded image cut into tiles of `tile_width` x `tile_height`
    /// pixels (at least one each, at most 65535).
    pub fn from_image(image: &DynamicImage, tile_width: u32, tile_height: u32) -> SpriteSheet {
        let (tw, th) = (tile_width.clamp(1, 65535), tile_height.clamp(1, 65535));
        let rgba = image.to_rgba8();
        let (columns, rows) = ((rgba.width() / tw) as usize, (rgba.height() / th) as usize);
        let mut tiles = Vec::with_capacity(columns * rows);
        for row in 0..rows as u32 {
            for column in 0..columns as u32 {
                let tile =
                    rich_art::image::imageops::crop_imm(&rgba, column * tw, row * th, tw, th)
                        .to_image();
                tiles.push(PixelSprite::new(
                    tw as u16,
                    th as u16,
                    rgba_pixels(tile.as_raw()),
                ));
            }
        }
        SpriteSheet {
            columns,
            rows,
            tiles,
        }
    }

    /// Tiles across.
    pub fn columns(&self) -> usize {
        self.columns
    }

    /// Tiles down.
    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Every tile, row by row.
    pub fn tiles(&self) -> &[PixelSprite] {
        &self.tiles
    }

    /// Tile `index`, counted row by row.
    ///
    /// # Panics
    /// Past the last tile.
    pub fn tile(&self, index: usize) -> &PixelSprite {
        &self.tiles[index]
    }

    /// The tile at `column`, `row`.
    pub fn tile_at(&self, column: usize, row: usize) -> Option<&PixelSprite> {
        (column < self.columns)
            .then(|| self.tiles.get(row * self.columns + column))
            .flatten()
    }

    /// The tiles `frames` (indices, in order; repeats allowed) as one
    /// animated [`PixelSprite`], 100 ms a frame until
    /// [`period`](PixelSprite::period) says otherwise.
    ///
    /// # Panics
    /// With no frames, or an index past the last tile.
    pub fn animation(&self, frames: &[usize]) -> PixelSprite {
        let (first, rest) = frames.split_first().expect("an animation needs a frame");
        rest.iter().fold(self.tiles[*first].clone(), |sprite, i| {
            sprite.with_frame(self.tiles[*i].frame(0).to_vec())
        })
    }

    /// Tile `index` as half-block cell art, for a stage.
    pub fn sprite(&self, index: usize) -> Sprite {
        Sprite::from_pixels(&self.tiles[index])
    }

    /// The tiles `frames` as an animated half-block [`Sprite`].
    pub fn sprite_animation(&self, frames: &[usize]) -> Sprite {
        Sprite::from_pixels(&self.animation(frames))
    }
}

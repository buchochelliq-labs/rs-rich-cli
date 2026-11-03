//! A whole frame as tiles (#818): each frame (a browser's screencast, an
//! emulator's screen) is fitted to the cells it is drawn in and cut into
//! tiles a few cells a side, each a [`Picture`] of its own. A tile whose
//! pixels did not change keeps its picture, id and version, so a painter
//! that sends a picture once per version (intuiTUIve's `image_graphics`,
//! Kitty's placeholders) sends the terminal only the tiles that changed.
//!
//! What it saves depends on what moves: a caret blinking or a spinner
//! turning changes one tile of hundreds; a page scrolling moves every row,
//! so nearly every tile changes and it costs about what a whole frame
//! does. Behind the `frames` feature.
//!
//! ```
//! use rich_art::image::{Rgba, RgbaImage};
//! use rich_art::tiles::Tiler;
//!
//! // 16 x 8 cells in tiles of 8 x 4 cells: four tiles.
//! let mut tiler = Tiler::new(8, 4);
//! let mut frame = RgbaImage::from_pixel(160, 80, Rgba([255, 255, 255, 255]));
//! assert_eq!(tiler.update(&frame, 16, 8).len(), 4);
//! // The same frame again: nothing changed.
//! assert!(tiler.update(&frame, 16, 8).is_empty());
//! // One pixel in the bottom right changes one tile.
//! frame.put_pixel(150, 70, Rgba([0, 0, 0, 255]));
//! assert_eq!(tiler.update(&frame, 16, 8), [3]);
//! ```

use std::hash::{Hash, Hasher};

use image::{DynamicImage, RgbaImage, imageops::FilterType};

use crate::graphics::CellPixels;
use crate::picture::{Pacer, Picture, Pixels};

/// The tile size the issue proposes, in cells of [`CellPixels::DEFAULT`]:
/// 8 x 4 cells, 64 x 64 pixels.
pub const TILE_CELLS: (usize, usize) = (8, 4);

/// One tile: where it is, in cells, and its picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Its first column and row in the frame's cells.
    pub col: usize,
    pub row: usize,
    /// Its size in cells (the last column and row of tiles may be
    /// smaller).
    pub cols: usize,
    pub rows: usize,
    /// Its pixels, fitted to its cells: a new version when they change.
    pub picture: Picture,
    hash: u64,
}

/// Cuts frames into [`Tile`]s and tells which changed.
#[derive(Clone, Debug)]
pub struct Tiler {
    tile_cols: usize,
    tile_rows: usize,
    /// The frame's size in cells, as last updated.
    cols: usize,
    rows: usize,
    tiles: Vec<Tile>,
}

impl Default for Tiler {
    fn default() -> Tiler {
        Tiler::new(TILE_CELLS.0, TILE_CELLS.1)
    }
}

impl Tiler {
    /// Tiles of `tile_cols` x `tile_rows` cells (each at least 1).
    pub fn new(tile_cols: usize, tile_rows: usize) -> Tiler {
        Tiler {
            tile_cols: tile_cols.max(1),
            tile_rows: tile_rows.max(1),
            cols: 0,
            rows: 0,
            tiles: Vec::new(),
        }
    }

    /// The tiles, row by row.
    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// The frame's size in cells, as last updated.
    pub fn size(&self) -> (usize, usize) {
        (self.cols, self.rows)
    }

    /// Take `frame`, drawn in `cols` x `rows` cells: it is stretched to
    /// those cells (at [`CellPixels::DEFAULT`]) and cut into tiles. The
    /// indices of the tiles that changed, in order: all of them when the
    /// size in cells changed.
    pub fn update(&mut self, frame: &RgbaImage, cols: usize, rows: usize) -> Vec<usize> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        let cell = CellPixels::DEFAULT;
        let (width, height) = cell.span(cols, rows);
        let fitted;
        let frame = if frame.dimensions() == (width, height) {
            frame
        } else {
            fitted = DynamicImage::ImageRgba8(frame.clone())
                .resize_exact(width, height, FilterType::Triangle)
                .to_rgba8();
            &fitted
        };
        let resized = (cols, rows) != (self.cols, self.rows);
        if resized {
            self.tiles.clear();
            self.cols = cols;
            self.rows = rows;
        }
        let mut changed = Vec::new();
        let mut index = 0;
        for row in (0..rows).step_by(self.tile_rows) {
            for col in (0..cols).step_by(self.tile_cols) {
                let (tile_cols, tile_rows) = (
                    self.tile_cols.min(cols - col),
                    self.tile_rows.min(rows - row),
                );
                let (x, y) = (col as u32 * cell.width, row as u32 * cell.height);
                let (w, h) = cell.span(tile_cols, tile_rows);
                let pixels = image::imageops::crop_imm(frame, x, y, w, h).to_image();
                let hash = hash_of(&pixels);
                match self.tiles.get_mut(index) {
                    Some(tile) if tile.hash == hash => {}
                    Some(tile) => {
                        if let Some(next) = tile.picture.next_rgba(w, h, pixels.into_raw()) {
                            tile.picture = next;
                            tile.hash = hash;
                            changed.push(index);
                        }
                    }
                    None => {
                        if let Some(picture) = Picture::from_rgba(w, h, pixels.into_raw()) {
                            self.tiles.push(Tile {
                                col,
                                row,
                                cols: tile_cols,
                                rows: tile_rows,
                                picture,
                                hash,
                            });
                            changed.push(index);
                        }
                    }
                }
                index += 1;
            }
        }
        changed
    }

    /// What sending the tiles `changed` is likely to cost through
    /// `pixels`, before anything is measured ([`Pacer::estimate`]): what
    /// a byte budget weighs before it lets a frame through.
    pub fn estimate(&self, changed: &[usize], pixels: Pixels) -> u64 {
        changed
            .iter()
            .filter_map(|&i| self.tiles.get(i))
            .map(|tile| Pacer::estimate(pixels, tile.cols, tile.rows))
            .sum()
    }

    /// The escapes that send the tiles `changed` through `pixels`, each
    /// drawn in its own cells (Kitty's upload for its placeholders): what
    /// a painter writes for them, for measuring.
    pub fn encode(&self, changed: &[usize], pixels: Pixels) -> Vec<String> {
        changed
            .iter()
            .filter_map(|&i| self.tiles.get(i))
            .filter_map(|tile| tile.picture.encode(pixels, tile.cols, tile.rows))
            .collect()
    }
}

/// A hash of a tile's pixels.
fn hash_of(pixels: &RgbaImage) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    pixels.dimensions().hash(&mut hasher);
    pixels.as_raw().hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn white(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba([255, 255, 255, 255]))
    }

    #[test]
    fn a_frame_is_cut_into_tiles_and_the_edges_are_smaller() {
        let mut tiler = Tiler::new(8, 4);
        // 20 x 10 cells: three columns of tiles (8, 8, 4) and three rows
        // (4, 4, 2).
        let changed = tiler.update(&white(160, 160), 20, 10);
        assert_eq!(changed.len(), 9);
        let last = &tiler.tiles()[8];
        assert_eq!((last.col, last.row, last.cols, last.rows), (16, 8, 4, 2));
    }

    #[test]
    fn one_changed_tile_is_one_new_picture_and_one_upload() {
        let mut tiler = Tiler::new(8, 4);
        let mut frame = white(128, 128);
        tiler.update(&frame, 16, 8);
        let before: Vec<(u32, u32)> = tiler
            .tiles()
            .iter()
            .map(|t| (t.picture.id(), t.picture.version()))
            .collect();
        // Fitted to 16 x 8 cells of 8 x 16 pixels: 128 x 128 is already so.
        for x in 70..74 {
            frame.put_pixel(x, 10, Rgba([200, 0, 0, 255]));
        }
        let changed = tiler.update(&frame, 16, 8);
        assert_eq!(changed, [1]);
        let after: Vec<(u32, u32)> = tiler
            .tiles()
            .iter()
            .map(|t| (t.picture.id(), t.picture.version()))
            .collect();
        assert_eq!(before[0], after[0]);
        assert_eq!(before[1].0, after[1].0, "the same id");
        assert_ne!(before[1].1, after[1].1, "a new version");
        let escapes = tiler.encode(&changed, Pixels::Kitty);
        assert_eq!(escapes.len(), 1);
        assert!(escapes[0].contains(&format!("i={}", after[1].0)));
        assert!(tiler.estimate(&changed, Pixels::Kitty) > 0);
    }

    #[test]
    fn a_new_size_sends_every_tile_again() {
        let mut tiler = Tiler::default();
        assert_eq!(tiler.update(&white(64, 64), 8, 4).len(), 1);
        assert_eq!(tiler.update(&white(64, 64), 16, 4).len(), 2);
        assert_eq!(tiler.size(), (16, 4));
    }
}

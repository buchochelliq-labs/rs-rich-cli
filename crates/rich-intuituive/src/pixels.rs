//! Pixel buffers: an RGBA framebuffer drawn in terminal cells.
//!
//! [`pixels`] is a node that hands its closure a [`PixelBuf`] each time it
//! draws, then shows the buffer in half blocks (two pixels a cell, one
//! above the other, in true colour), quadrants (2x2) or Braille (2x4). A
//! game's playfield, a raycaster's view, a plot, a picture made in code:
//! anything drawn a pixel at a time. [`PixelBuf`] fills rectangles, draws
//! lines and [blits](PixelBuf::blit) [`PixelSprite`]s with alpha, scaled
//! and flipped. The closure may read signals: the node draws again when
//! they change.
//!
//! ```
//! use intuituive::prelude::*;
//! use intuituive::pixels::{pixels, PixelSprite};
//!
//! let app = App::new(|| {
//!     let ball = PixelSprite::from_art(&[".o.", "ooo", ".o."], &[('o', [255, 255, 0, 255])]);
//!     pixels(8, 4, move |buf| {
//!         buf.clear([0, 0, 0, 0]);
//!         buf.blit(&ball, 2, 0, 1.0, false);
//!     })
//! });
//! let mut driver = app.driver(8, 2);
//! driver.settle();
//! // Two pixels a cell, one above the other.
//! assert_eq!(driver.screen().plain(), ["  ▄▀▄   ", "   ▀    "]);
//! ```

use std::rc::Rc;
use std::time::Duration;

#[cfg(test)]
use rich::color::Color;
#[cfg(test)]
use rich::{Segment, Style};

use crate::node::{Axis, Node};
use crate::sprite::Timing;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

/// One pixel: red, green, blue and alpha (0 transparent, 255 opaque).
pub type Rgba = [u8; 4];

/// A transparent pixel.
pub const CLEAR: Rgba = [0, 0, 0, 0];

/// `src` over `dst`, by `src`'s alpha.
#[inline]
pub fn blend(dst: Rgba, src: Rgba) -> Rgba {
    match src[3] {
        255 => src,
        0 => dst,
        a => {
            let a = a as u32;
            let mix = |s: u8, d: u8| ((s as u32 * a + d as u32 * (255 - a) + 127) / 255) as u8;
            [
                mix(src[0], dst[0]),
                mix(src[1], dst[1]),
                mix(src[2], dst[2]),
                (a + dst[3] as u32 * (255 - a) / 255).min(255) as u8,
            ]
        }
    }
}

/// An RGBA framebuffer, `width` x `height` pixels, transparent at first.
/// Drawing outside it is clipped.
///
/// ```
/// use intuituive::pixels::PixelBuf;
///
/// let mut buf = PixelBuf::new(4, 4);
/// buf.fill_rect(1, 1, 2, 2, [255, 0, 0, 255]);
/// buf.fill_rect(2, 2, 9, 9, [0, 0, 255, 128]);
/// assert_eq!(buf.get(1, 1), Some([255, 0, 0, 255]));
/// assert_eq!(buf.get(2, 2), Some([127, 0, 128, 255]));
/// assert_eq!(buf.get(0, 0), Some([0, 0, 0, 0]));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PixelBuf {
    width: u16,
    height: u16,
    data: Vec<Rgba>,
}

impl PixelBuf {
    pub fn new(width: u16, height: u16) -> PixelBuf {
        PixelBuf {
            width,
            height,
            data: vec![CLEAR; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Every pixel, row by row.
    pub fn pixels(&self) -> &[Rgba] {
        &self.data
    }

    /// Every pixel, row by row, to write directly (a raycaster's columns).
    pub fn pixels_mut(&mut self) -> &mut [Rgba] {
        &mut self.data
    }

    /// The pixels as RGBA bytes, row by row: for an image encoder.
    pub fn to_rgba8(&self) -> Vec<u8> {
        self.data.iter().flatten().copied().collect()
    }

    /// Change the size; the pixels are cleared.
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.data.clear();
        self.data.resize(width as usize * height as usize, CLEAR);
    }

    #[inline]
    fn index(&self, x: i32, y: i32) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.width as i32 && y < self.height as i32)
            .then(|| y as usize * self.width as usize + x as usize)
    }

    /// The pixel at `x`, `y`; `None` outside the buffer.
    pub fn get(&self, x: i32, y: i32) -> Option<Rgba> {
        self.index(x, y).map(|i| self.data[i])
    }

    /// Set the pixel at `x`, `y` to `rgba`, alpha and all.
    pub fn set(&mut self, x: i32, y: i32, rgba: Rgba) {
        if let Some(i) = self.index(x, y) {
            self.data[i] = rgba;
        }
    }

    /// Lay `rgba` over the pixel at `x`, `y`, by its alpha.
    pub fn plot(&mut self, x: i32, y: i32, rgba: Rgba) {
        if let Some(i) = self.index(x, y) {
            self.data[i] = blend(self.data[i], rgba);
        }
    }

    /// Set every pixel to `rgba`.
    pub fn clear(&mut self, rgba: Rgba) {
        self.data.fill(rgba);
    }

    /// Lay `rgba` over the rectangle at `x`, `y`, `width` x `height`, by
    /// its alpha.
    pub fn fill_rect(&mut self, x: i32, y: i32, width: u16, height: u16, rgba: Rgba) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + width as i32).min(self.width as i32);
        let y1 = (y + height as i32).min(self.height as i32);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let stride = self.width as usize;
        for row in y0..y1 {
            let start = row as usize * stride;
            let line = &mut self.data[start + x0 as usize..start + x1 as usize];
            if rgba[3] == 255 {
                line.fill(rgba);
            } else {
                for pixel in line {
                    *pixel = blend(*pixel, rgba);
                }
            }
        }
    }

    /// A line from `x0`, `y0` to `x1`, `y1`, both ends included
    /// (Bresenham's), laid over by `rgba`'s alpha.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgba: Rgba) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut error) = (x0, y0, dx + dy);
        loop {
            self.plot(x, y, rgba);
            if x == x1 && y == y1 {
                return;
            }
            let twice = 2 * error;
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }

    /// Draw `sprite`'s first frame with its top left at `x`, `y`, `scale`
    /// times its size (nearest neighbour; at least a pixel), mirrored left
    /// to right when `flip`, laid over by each pixel's alpha.
    ///
    /// ```
    /// use intuituive::pixels::{PixelBuf, PixelSprite};
    ///
    /// let arrow = PixelSprite::from_art(&["r."], &[('r', [255, 0, 0, 255])]);
    /// let mut buf = PixelBuf::new(4, 2);
    /// buf.blit(&arrow, 0, 0, 2.0, true);
    /// // Twice the size, mirrored: the red pixel is on the right.
    /// assert_eq!(buf.get(1, 1), Some([0, 0, 0, 0]));
    /// assert_eq!(buf.get(3, 1), Some([255, 0, 0, 255]));
    /// ```
    pub fn blit(&mut self, sprite: &PixelSprite, x: i32, y: i32, scale: f32, flip: bool) {
        self.blit_frame(sprite, 0, x, y, scale, flip);
    }

    /// [`blit`](Self::blit) with frame `frame` of `sprite` (wrapping round
    /// its frames): [`PixelSprite::frame_at`] picks one by the clock.
    pub fn blit_frame(
        &mut self,
        sprite: &PixelSprite,
        frame: usize,
        x: i32,
        y: i32,
        scale: f32,
        flip: bool,
    ) {
        let (sw, sh) = (sprite.width as i32, sprite.height as i32);
        if sw == 0 || sh == 0 || sprite.frames.is_empty() || !scale.is_finite() || scale <= 0.0 {
            return;
        }
        let source = &sprite.frames[frame % sprite.frames.len()];
        let dw = ((sw as f32 * scale).round() as i32).max(1);
        let dh = ((sh as f32 * scale).round() as i32).max(1);
        let cx0 = x.max(0);
        let cy0 = y.max(0);
        let cx1 = (x + dw).min(self.width as i32);
        let cy1 = (y + dh).min(self.height as i32);
        if cx0 >= cx1 || cy0 >= cy1 {
            return;
        }
        let stride = self.width as usize;
        for ty in cy0..cy1 {
            let sy = ((ty - y) as i64 * sh as i64 / dh as i64) as usize;
            let row = &source[sy * sw as usize..(sy + 1) * sw as usize];
            let start = ty as usize * stride;
            for tx in cx0..cx1 {
                let mut sx = ((tx - x) as i64 * sw as i64 / dw as i64) as usize;
                if flip {
                    sx = sw as usize - 1 - sx;
                }
                let src = row[sx];
                if src[3] != 0 {
                    let dst = &mut self.data[start + tx as usize];
                    *dst = blend(*dst, src);
                }
            }
        }
    }
}

/// Pixel art in RGBA: one or more frames of `width` x `height` pixels,
/// with the time each frame shows. [`PixelBuf::blit`] draws one into a
/// buffer; [`Sprite::from_pixels`](crate::sprite::Sprite::from_pixels)
/// makes cell art of it for a [`stage`](crate::sprite::stage).
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct PixelSprite {
    width: u16,
    height: u16,
    frames: Vec<Rc<[Rgba]>>,
    timing: Timing,
}

impl PixelSprite {
    /// One frame of `pixels`, row by row.
    ///
    /// # Panics
    /// When there are not `width * height` pixels.
    pub fn new(width: u16, height: u16, pixels: Vec<Rgba>) -> PixelSprite {
        assert_eq!(
            pixels.len(),
            width as usize * height as usize,
            "a PixelSprite frame has width * height pixels"
        );
        PixelSprite {
            width,
            height,
            frames: vec![pixels.into()],
            timing: Timing::still(),
        }
    }

    /// One frame from RGBA bytes, row by row.
    ///
    /// # Panics
    /// When there are not `width * height * 4` bytes.
    pub fn from_rgba8(width: u16, height: u16, bytes: &[u8]) -> PixelSprite {
        PixelSprite::new(width, height, rgba_pixels(bytes))
    }

    /// One frame drawn in characters: each row a string, each character a
    /// pixel, coloured by `palette`; a character not in it is transparent.
    /// Rows shorter than the longest are transparent past their end.
    pub fn from_art(rows: &[&str], palette: &[(char, Rgba)]) -> PixelSprite {
        let width = rows.iter().map(|r| r.chars().count()).max().unwrap_or(0);
        let mut pixels = vec![CLEAR; width * rows.len()];
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                if let Some((_, rgba)) = palette.iter().find(|(p, _)| *p == c) {
                    pixels[y * width + x] = *rgba;
                }
            }
        }
        PixelSprite::new(width as u16, rows.len() as u16, pixels)
    }

    /// The same with another frame after the last, which shows as long as
    /// the last did.
    ///
    /// # Panics
    /// When the frame is not the sprite's size.
    pub fn with_frame(mut self, pixels: Vec<Rgba>) -> PixelSprite {
        assert_eq!(
            pixels.len(),
            self.width as usize * self.height as usize,
            "a PixelSprite frame has width * height pixels"
        );
        self.frames.push(pixels.into());
        self.timing.push();
        self
    }

    /// Each frame shows for `period`.
    pub fn period(mut self, period: Duration) -> PixelSprite {
        self.timing.set_period(period, self.frames.len());
        self
    }

    /// Each frame shows for its own time (the last given for the rest).
    pub fn durations(mut self, durations: &[Duration]) -> PixelSprite {
        self.timing.set_durations(durations, self.frames.len());
        self
    }

    /// Play the frames once and stay on the last, instead of looping.
    pub fn once(mut self) -> PixelSprite {
        self.timing.looping = false;
        self
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// Frame `i`'s pixels, row by row (wrapping round the frames).
    pub fn frame(&self, i: usize) -> &[Rgba] {
        &self.frames[i % self.frames.len()]
    }

    /// The frame showing `elapsed` after the animation started.
    pub fn frame_at(&self, elapsed: Duration) -> usize {
        self.timing.frame_at(elapsed)
    }

    pub(crate) fn timing(&self) -> &Timing {
        &self.timing
    }

    pub(crate) fn frames(&self) -> &[Rc<[Rgba]>] {
        &self.frames
    }

    /// The pixel at `x`, `y` of frame `frame`, transparent outside.
    pub(crate) fn pixel(frame: &[Rgba], width: u16, x: i32, y: i32, height: u16) -> Rgba {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            CLEAR
        } else {
            frame[y as usize * width as usize + x as usize]
        }
    }
}

/// RGBA bytes as pixels.
///
/// # Panics
/// When the length is not a multiple of four.
pub(crate) fn rgba_pixels(bytes: &[u8]) -> Vec<Rgba> {
    assert_eq!(bytes.len() % 4, 0, "RGBA bytes come in fours");
    bytes.as_chunks::<4>().0.to_vec()
}

/// How a [`pixels`] node puts pixels in cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelMode {
    /// Two pixels a cell, one above the other (`▀` in the top's colour on
    /// the bottom's): every pixel's own colour, in any truecolor or
    /// 256-colour terminal.
    #[default]
    HalfBlocks,
    /// Four pixels a cell, 2x2, in two colours a cell (`▚`, `▙`): twice
    /// the detail across, colours averaged.
    Quadrants,
    /// Eight dots a cell, 2x4 (`⣿`), in two colours a cell: the finest
    /// shapes, the coarsest colour.
    Braille,
}

impl PixelMode {
    /// The pixels one cell holds, across and down.
    pub fn cell(self) -> (u16, u16) {
        match self {
            PixelMode::HalfBlocks => (1, 2),
            PixelMode::Quadrants => (2, 2),
            PixelMode::Braille => (2, 4),
        }
    }
}

/// How a [`pixels`] node colours a quadrant or Braille cell, whose pixels
/// get two colours between them ([`Node::pixel_colors`]). Half blocks show
/// every pixel's own colour either way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelColors {
    /// Each side's pixels averaged: the smoothest picture, in colours the
    /// pixels may not have.
    #[default]
    Average,
    /// Each side's colour is the one of its pixels nearest that side's
    /// average: only colours the cell has, so pixel art and an emulator's
    /// or a 16- or 256-colour picture keep their palette, and fewer
    /// distinct styles reach the screen.
    Nearest,
}

/// Alpha at or above this is opaque when pixels become cells.
const OPAQUE: u8 = 128;

type Rgb = [u8; 3];

#[inline]
fn opaque(p: Rgba) -> Option<Rgb> {
    (p[3] >= OPAQUE).then_some([p[0], p[1], p[2]])
}

/// One cell of half blocks from its `top` and `bottom` pixels: `None`
/// when both are transparent. A cell with one pixel transparent has no
/// background, so what is under it shows there.
#[inline]
pub(crate) fn half_block(top: Rgba, bottom: Rgba) -> Option<(char, Option<Rgb>, Option<Rgb>)> {
    match (opaque(top), opaque(bottom)) {
        (None, None) => None,
        (Some(t), None) => Some(('▀', Some(t), None)),
        (None, Some(b)) => Some(('▄', Some(b), None)),
        (Some(t), Some(b)) => Some(('▀', Some(t), Some(b))),
    }
}

/// Quadrant glyphs by mask: top left 1, top right 2, bottom left 4,
/// bottom right 8.
const QUADRANTS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// A cell of several pixels in two colours: which pixels are lit
/// (`mask`'s bits in the order given), the lit colour and the unlit one
/// (`None` where the unlit pixels are transparent). Lit are the opaque
/// pixels at least as bright as their mean; with transparent pixels, every
/// opaque one. Each colour is as `colors` says.
fn two_colors(pixels: &[Rgba], colors: PixelColors) -> Option<(u32, Rgb, Option<Rgb>)> {
    let luma = |p: &Rgba| 299 * p[0] as u32 + 587 * p[1] as u32 + 114 * p[2] as u32;
    let solid: Vec<(usize, &Rgba)> = pixels
        .iter()
        .enumerate()
        .filter(|(_, p)| p[3] >= OPAQUE)
        .collect();
    if solid.is_empty() {
        return None;
    }
    let lit: u32 = if solid.len() < pixels.len() {
        solid.iter().fold(0, |m, (i, _)| m | 1 << i)
    } else {
        let mean = solid.iter().map(|(_, p)| luma(p)).sum::<u32>() / solid.len() as u32;
        solid
            .iter()
            .filter(|(_, p)| luma(p) >= mean)
            .fold(0, |m, (i, _)| m | 1 << i)
    };
    let average = |want: bool| -> Option<Rgb> {
        let chosen: Vec<&Rgba> = solid
            .iter()
            .filter(|(i, _)| (lit >> i & 1 == 1) == want)
            .map(|(_, p)| *p)
            .collect();
        if chosen.is_empty() {
            return None;
        }
        let n = chosen.len() as u32;
        let sum = |c: usize| (chosen.iter().map(|p| p[c] as u32).sum::<u32>() / n) as u8;
        let mean = [sum(0), sum(1), sum(2)];
        if colors == PixelColors::Average {
            return Some(mean);
        }
        let distance = |p: &&&Rgba| {
            (0..3)
                .map(|c| (i32::from(p[c]) - i32::from(mean[c])).pow(2))
                .sum::<i32>()
        };
        chosen
            .iter()
            .min_by_key(distance)
            .map(|p| [p[0], p[1], p[2]])
    };
    Some((lit, average(true)?, average(false)))
}

/// The cell at `column`, `row` of `buf` in `mode`: its glyph and colours.
/// A space shows only its background, so it has no foreground. `scratch`
/// holds a cell's pixels.
#[inline]
fn cell_at(
    buf: &PixelBuf,
    (mode, colors): (PixelMode, PixelColors),
    column: i32,
    row: i32,
    scratch: &mut Vec<Rgba>,
) -> (char, Option<Rgb>, Option<Rgb>) {
    let (pw, ph) = mode.cell();
    let px = |x: i32, y: i32| buf.get(x, y).unwrap_or(CLEAR);
    let (x, y) = (column * pw as i32, row * ph as i32);
    let (glyph, fg, bg) = match mode {
        PixelMode::HalfBlocks => half_block(px(x, y), px(x, y + 1)).unwrap_or((' ', None, None)),
        PixelMode::Quadrants => {
            scratch.clear();
            scratch.extend([px(x, y), px(x + 1, y), px(x, y + 1), px(x + 1, y + 1)]);
            match two_colors(scratch, colors) {
                None => (' ', None, None),
                Some((mask, fg, bg)) => (QUADRANTS[mask as usize], Some(fg), bg),
            }
        }
        PixelMode::Braille => {
            scratch.clear();
            // In the order of the Braille pattern's bits.
            scratch.extend([
                px(x, y),
                px(x, y + 1),
                px(x, y + 2),
                px(x + 1, y),
                px(x + 1, y + 1),
                px(x + 1, y + 2),
                px(x, y + 3),
                px(x + 1, y + 3),
            ]);
            match two_colors(scratch, colors) {
                None => (' ', None, None),
                Some((mask, fg, bg)) => {
                    let glyph = char::from_u32(0x2800 + mask).unwrap_or(' ');
                    (glyph, Some(fg), bg)
                }
            }
        }
    };
    if glyph == ' ' {
        (glyph, None, bg)
    } else {
        (glyph, fg, bg)
    }
}

/// Draw `buf` on `canvas` in `mode`, a cell at a time with plain colours
/// ([`Canvas::put_rgb`]); the cells past the buffer are cleared.
fn draw_cells(buf: &PixelBuf, (mode, colors): (PixelMode, PixelColors), canvas: &mut Canvas) {
    let (pw, ph) = mode.cell();
    let (width, height) = (canvas.width(), canvas.height());
    let columns = width.min(buf.width.div_ceil(pw));
    let rows = height.min(buf.height.div_ceil(ph));
    let mut scratch = Vec::with_capacity(8);
    for row in 0..rows {
        for column in 0..columns {
            let (glyph, fg, bg) =
                cell_at(buf, (mode, colors), column as i32, row as i32, &mut scratch);
            canvas.put_rgb(column, row, glyph, fg, bg);
        }
    }
    if columns < width {
        canvas.clear(columns, 0, width - columns, rows);
    }
    if rows < height {
        canvas.clear(0, rows, width, height - rows);
    }
}

/// `buf` as rows of cells in `mode`, `columns` x `rows` of them, as
/// rendered segments (runs of one style are one segment): what the node
/// drew before it wrote cells directly, kept to show they agree.
#[cfg(test)]
fn render_lines(buf: &PixelBuf, mode: PixelMode, columns: u16, rows: u16) -> Vec<Vec<Segment>> {
    let style = |(fg, bg): (Option<Rgb>, Option<Rgb>)| {
        (fg.is_some() || bg.is_some()).then(|| {
            Style::from_color(
                fg.map(|[r, g, b]| Color::from_rgb(r, g, b)),
                bg.map(|[r, g, b]| Color::from_rgb(r, g, b)),
            )
        })
    };
    let mut scratch = Vec::with_capacity(8);
    (0..rows as i32)
        .map(|row| {
            let mut line: Vec<Segment> = Vec::new();
            let mut run = String::new();
            let mut run_key = (None, None);
            for column in 0..columns as i32 {
                let (glyph, fg, bg) =
                    cell_at(buf, (mode, PixelColors::Average), column, row, &mut scratch);
                if (fg, bg) != run_key && !run.is_empty() {
                    line.push(Segment::new(std::mem::take(&mut run), style(run_key)));
                }
                run_key = (fg, bg);
                run.push(glyph);
            }
            if !run.is_empty() {
                line.push(Segment::new(run, style(run_key)));
            }
            line
        })
        .collect()
}

type Draw = Box<dyn FnMut(&mut PixelBuf)>;

/// What [`pixels`] makes.
struct Pixels {
    /// The buffer's size, or `None` to follow the node's.
    size: Option<(u16, u16)>,
    mode: PixelMode,
    colors: PixelColors,
    buf: PixelBuf,
    draw: Draw,
}

impl Widget for Pixels {
    fn name(&self) -> &'static str {
        "pixels"
    }

    fn describe(&self) -> Option<String> {
        Some(format!(
            "{}x{} {:?}",
            self.buf.width, self.buf.height, self.mode
        ))
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let (pw, ph) = self.mode.cell();
        match (self.size, axis) {
            (Some((w, _)), Axis::Horizontal) => w.div_ceil(pw),
            (Some((_, h)), Axis::Vertical) => h.div_ceil(ph),
            (None, Axis::Horizontal) => width,
            (None, Axis::Vertical) => height,
        }
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut Canvas) {
        let (pw, ph) = self.mode.cell();
        let (columns, rows) = (canvas.width(), canvas.height());
        let (width, height) = self
            .size
            .unwrap_or((columns.saturating_mul(pw), rows.saturating_mul(ph)));
        if (self.buf.width, self.buf.height) != (width, height) {
            self.buf.resize(width, height);
        }
        (self.draw)(&mut self.buf);
        draw_cells(&self.buf, (self.mode, self.colors), canvas);
    }
}

/// A node showing a `width` x `height` pixel buffer that `draw` paints
/// each time the node draws, in half blocks (two pixels a cell, one above
/// the other): `width` columns, `height / 2` rows (rounded up). The buffer
/// is kept between draws, so `draw` may change only what moved, or
/// [`clear`](PixelBuf::clear) it first. Signals `draw` reads draw it again
/// when they change. [`Node::pixel_mode`] draws it in quadrants or
/// Braille instead. The node is as large as its buffer
/// ([`auto`](Node::auto)); where it is given less, the rest is cut.
pub fn pixels(width: u16, height: u16, draw: impl FnMut(&mut PixelBuf) + 'static) -> Node {
    Node::from_widget(
        Box::new(Pixels {
            size: Some((width, height)),
            mode: PixelMode::HalfBlocks,
            colors: PixelColors::Average,
            buf: PixelBuf::new(width, height),
            draw: Box::new(draw),
        }),
        "pixels",
    )
    .auto()
}

/// [`pixels`] with a buffer as large as the node: its columns by twice its
/// rows in half blocks (by the [mode](Node::pixel_mode)'s cell otherwise),
/// cleared whenever the node's size changes. A raycaster's view that
/// fills the screen.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::pixels::pixels_fit;
///
/// let app = App::new(|| {
///     pixels_fit(|buf| {
///         let (w, h) = (buf.width() as i32, buf.height() as i32);
///         buf.clear([0, 0, 0, 255]);
///         buf.line(0, h - 1, w - 1, h - 1, [255, 255, 255, 255]);
///     })
/// });
/// let mut driver = app.driver(6, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain(), ["▀▀▀▀▀▀", "▀▀▀▀▀▀"]);
/// ```
pub fn pixels_fit(draw: impl FnMut(&mut PixelBuf) + 'static) -> Node {
    Node::from_widget(
        Box::new(Pixels {
            size: None,
            mode: PixelMode::HalfBlocks,
            colors: PixelColors::Average,
            buf: PixelBuf::new(0, 0),
            draw: Box::new(draw),
        }),
        "pixels",
    )
}

impl Node {
    /// For a [`pixels`] node: draw its pixels in `mode`'s cells. A fixed
    /// buffer's node is measured again in the new cells.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::pixels::{pixels, PixelMode};
    ///
    /// let app = App::new(|| {
    ///     pixels(4, 4, |buf| buf.fill_rect(0, 0, 2, 2, [255, 255, 255, 255]))
    ///         .pixel_mode(PixelMode::Quadrants)
    /// });
    /// let mut driver = app.driver(2, 2);
    /// driver.settle();
    /// assert_eq!(driver.screen().plain(), ["█ ", "  "]);
    /// ```
    pub fn pixel_mode(self, mode: PixelMode) -> Node {
        self.widget_mut(|pixels: &mut Pixels| pixels.mode = mode);
        self
    }

    /// For a [`pixels`] node in quadrants or Braille: colour each cell's
    /// two sides as `colors` says. [`PixelColors::Nearest`] keeps to the
    /// colours the pixels have (a palette's); [`PixelColors::Average`],
    /// the default, blends them.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::pixels::{pixels, PixelColors, PixelMode};
    ///
    /// let app = App::new(|| {
    ///     pixels(2, 4, |buf| {
    ///         buf.clear([0, 0, 0, 255]);
    ///         buf.fill_rect(0, 0, 2, 1, [255, 0, 0, 255]);
    ///         buf.fill_rect(0, 1, 2, 1, [200, 0, 0, 255]);
    ///     })
    ///     .pixel_mode(PixelMode::Braille)
    ///     .pixel_colors(PixelColors::Nearest)
    /// });
    /// let mut driver = app.driver(1, 1);
    /// driver.settle();
    /// let screen = driver.screen();
    /// let cell = screen.cell(0, 0);
    /// // The lit dots in red (the nearer of red and dark red to their
    /// // mean), the rest in black: no blend.
    /// assert_eq!(cell.text(), "⠛");
    /// let style = screen.style(cell.style).unwrap();
    /// assert_eq!(style.color(), Some(&rich::color::Color::from_rgb(200, 0, 0)));
    /// ```
    pub fn pixel_colors(self, colors: PixelColors) -> Node {
        self.widget_mut(|pixels: &mut Pixels| pixels.colors = colors);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;

    /// A buffer of every kind of cell: opaque, half and fully transparent,
    /// flat and noisy.
    fn sample(width: u16, height: u16) -> PixelBuf {
        let mut buf = PixelBuf::new(width, height);
        let mut n = 1u32;
        for y in 0..height as i32 {
            for x in 0..width as i32 {
                n = n.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let [r, g, b, a] = n.to_le_bytes();
                let alpha = match (x + y) % 5 {
                    0 => 0,
                    1 => a,
                    _ => 255,
                };
                buf.set(
                    x,
                    y,
                    if x < 3 {
                        [9, 9, 9, alpha]
                    } else {
                        [r, g, b, alpha]
                    },
                );
            }
        }
        buf
    }

    #[test]
    fn nearest_colours_keep_to_the_cells_own() {
        // 2x4: red, dark red and black, in every Braille cell's dots.
        let (red, dark, black) = ([255, 0, 0, 255], [200, 0, 0, 255], [0, 0, 0, 255]);
        let mut buf = PixelBuf::new(2, 4);
        for (i, p) in [red, dark, red, black, dark, black, red, black]
            .into_iter()
            .enumerate()
        {
            buf.set(i as i32 % 2, i as i32 / 2, p);
        }
        let own: Vec<Rgb> = [red, dark, black].map(|p| [p[0], p[1], p[2]]).to_vec();
        let mut scratch = Vec::new();
        for mode in [PixelMode::Braille, PixelMode::Quadrants] {
            let (_, fg, bg) = cell_at(&buf, (mode, PixelColors::Nearest), 0, 0, &mut scratch);
            assert!(own.contains(&fg.unwrap()), "{mode:?} {fg:?}");
            assert!(own.contains(&bg.unwrap()), "{mode:?} {bg:?}");
            // Averaged, the lit side is a blend of red and dark red.
            let (_, fg, _) = cell_at(&buf, (mode, PixelColors::Average), 0, 0, &mut scratch);
            assert!(!own.contains(&fg.unwrap()), "{mode:?} {fg:?}");
        }
    }

    #[test]
    fn averaged_colours_are_the_default() {
        for mode in [PixelMode::Quadrants, PixelMode::Braille] {
            let screens: Vec<Vec<String>> = [None, Some(PixelColors::Average)]
                .into_iter()
                .map(|colors| {
                    let buf = sample(13, 11);
                    let app = App::new(move || {
                        let buf = buf.clone();
                        let node = pixels(13, 11, move |b| *b = buf.clone()).pixel_mode(mode);
                        match colors {
                            Some(colors) => node.pixel_colors(colors),
                            None => node,
                        }
                    });
                    let mut driver = app.driver(9, 5);
                    driver.settle();
                    let screen = driver.screen();
                    let mut cells = Vec::new();
                    for y in 0..5 {
                        for x in 0..9 {
                            let cell = screen.cell(x, y);
                            cells.push(format!("{}{:?}", cell.text(), screen.style(cell.style)));
                        }
                    }
                    cells
                })
                .collect();
            assert_eq!(screens[0], screens[1], "{mode:?}");
        }
    }

    #[test]
    fn cells_written_directly_match_the_segments_they_replaced() {
        for mode in [
            PixelMode::HalfBlocks,
            PixelMode::Quadrants,
            PixelMode::Braille,
        ] {
            let buf = sample(13, 11);
            let (pw, ph) = mode.cell();
            // A canvas larger than the buffer: the rest is cleared alike.
            let (columns, rows) = (9u16, 5u16);
            let used = (
                columns.min(13u16.div_ceil(pw)),
                rows.min(11u16.div_ceil(ph)),
            );
            let lines = render_lines(&buf, mode, used.0, used.1);
            let fill = buf.clone();
            let app = App::new(move || {
                let buf = fill.clone();
                pixels(13, 11, move |b| *b = buf.clone()).pixel_mode(mode)
            });
            let mut driver = app.driver(columns, rows);
            driver.settle();
            let mut expected = crate::screen::Screen::new(columns, rows);
            expected.write_lines(expected.area(), &lines);
            let got = driver.screen();
            for y in 0..rows {
                for x in 0..columns {
                    let (a, b) = (got.cell(x, y), expected.cell(x, y));
                    assert_eq!(
                        (a.text(), got.style(a.style)),
                        (b.text(), expected.style(b.style)),
                        "{mode:?} ({x}, {y})"
                    );
                }
            }
        }
    }
}

//! Screenshots and video frames as pixels: PNG and GIF.
//!
//! Glyphs come from DejaVu Sans Mono, embedded in the crate (Bitstream Vera
//! licence, `fonts/LICENSE-DejaVu`), so output is identical on every machine.
//! [`Fonts::load`] takes another font instead. Italic is drawn upright:
//! only the regular and bold faces are embedded. Emoji come from the embedded
//! Twemoji (CC BY 4.0, `fonts/LICENSE-Twemoji`), in colour. Box-drawing,
//! block and braille characters are drawn as geometry over the whole cell, so
//! borders join and images and bars have no seams whatever the line height.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

use fontdue::{Font, FontSettings, Metrics};
use unicode_width::UnicodeWidthChar;

use crate::render::emoji::Emoji;
use crate::screen::{Rgb, Snapshot, Theme};

const REGULAR: &[u8] = include_bytes!("../../fonts/DejaVuSansMono.ttf");
const BOLD: &[u8] = include_bytes!("../../fonts/DejaVuSansMono-Bold.ttf");
const CHROME: Rgb = (30, 30, 30);
const OUTLINE: Rgb = (70, 70, 70);

/// An RGB image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}

impl Canvas {
    pub fn new(width: usize, height: usize, fill: Rgb) -> Canvas {
        let mut pixels = Vec::with_capacity(width * height * 3);
        for _ in 0..width * height {
            pixels.extend_from_slice(&[fill.0, fill.1, fill.2]);
        }
        Canvas {
            width,
            height,
            pixels,
        }
    }

    /// Blend `colour` into pixel (x, y) with coverage `alpha` (0–255).
    fn blend(&mut self, x: i64, y: i64, colour: Rgb, alpha: u8) {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height || alpha == 0 {
            return;
        }
        let i = (y as usize * self.width + x as usize) * 3;
        if alpha == 255 {
            self.pixels[i..i + 3].copy_from_slice(&[colour.0, colour.1, colour.2]);
            return;
        }
        let a = alpha as u32;
        for (offset, c) in [colour.0, colour.1, colour.2].into_iter().enumerate() {
            let old = self.pixels[i + offset] as u32;
            self.pixels[i + offset] = ((c as u32 * a + old * (255 - a)) / 255) as u8;
        }
    }

    pub fn fill_rect(&mut self, x: i64, y: i64, width: i64, height: i64, colour: Rgb) {
        let x0 = x.clamp(0, self.width as i64) as usize;
        let x1 = (x + width).clamp(0, self.width as i64) as usize;
        let rgb = [colour.0, colour.1, colour.2];
        for py in y.max(0)..(y + height).min(self.height as i64) {
            let row = py as usize * self.width * 3;
            for pixel in self.pixels[row + x0 * 3..row + x1 * 3]
                .as_chunks_mut::<3>()
                .0
            {
                pixel.copy_from_slice(&rgb);
            }
        }
    }

    /// A filled rectangle with rounded corners, antialiased.
    pub fn fill_rounded(
        &mut self,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        colour: Rgb,
    ) {
        let (x1, y1) = (x + width, y + height);
        let (left, right) = (x.floor() as i64, x1.ceil() as i64);
        for py in y.floor() as i64..y1.ceil() as i64 {
            let cy = py as f64 + 0.5;
            // The part of the row the shape covers fully: clear of the
            // antialiased edges, and of the corners unless the row is
            // between them. Filled directly; only the rest needs coverage.
            // (A radius under half a pixel never reaches full coverage.)
            let (span_lo, span_hi) = if radius < 0.5 || cy - y < 1.5 || y1 - cy < 1.5 {
                (right, right)
            } else if cy - y > radius + 1.0 && y1 - cy > radius + 1.0 {
                ((x + 1.5).ceil() as i64, (x1 - 1.5).floor() as i64)
            } else {
                (
                    (x + radius + 1.0).ceil() as i64,
                    (x1 - radius - 1.0).floor() as i64,
                )
            };
            if span_hi > span_lo {
                self.fill_rect(span_lo, py, span_hi - span_lo, 1, colour);
            }
            for px in (left..right).filter(|px| *px < span_lo || *px >= span_hi.max(span_lo)) {
                let cx = px as f64 + 0.5;
                // Distance outside the rounded shape, in pixels.
                let dx = (x + radius - cx).max(cx - (x1 - radius)).max(0.0);
                let dy = (y + radius - cy).max(cy - (y1 - radius)).max(0.0);
                let outside = (dx * dx + dy * dy).sqrt() - radius;
                let edge = (cx - x).min(x1 - cx).min(cy - y).min(y1 - cy);
                let coverage = (0.5 - outside).clamp(0.0, 1.0) * (edge + 0.5).clamp(0.0, 1.0);
                self.blend(px, py, colour, (coverage * 255.0) as u8);
            }
        }
    }

    pub fn fill_circle(&mut self, cx: f64, cy: f64, radius: f64, colour: Rgb) {
        self.fill_rounded(
            cx - radius,
            cy - radius,
            radius * 2.0,
            radius * 2.0,
            radius,
            colour,
        );
    }

    /// Copy `other` onto this canvas with its top-left at (x, y).
    pub fn paste(&mut self, other: &Canvas, x: usize, y: usize) {
        for row in 0..other.height.min(self.height.saturating_sub(y)) {
            let width = other.width.min(self.width.saturating_sub(x));
            let from = row * other.width * 3;
            let to = ((y + row) * self.width + x) * 3;
            self.pixels[to..to + width * 3].copy_from_slice(&other.pixels[from..from + width * 3]);
        }
    }

    /// Encode as PNG.
    pub fn png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, self.width as u32, self.height as u32);
            encoder.set_color(png::ColorType::Rgb);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.set_compression(png::Compression::High);
            let mut writer = encoder.write_header().expect("PNG header");
            writer.write_image_data(&self.pixels).expect("PNG data");
        }
        out
    }
}

type GlyphKey = (char, bool, u32);

/// Line weights (up, down, left, right) of a box-drawing character: 0 none,
/// 1 light, 2 heavy, 3 double (drawn heavy). Drawn as geometry across the
/// whole cell, so borders join between rows whatever the line height.
fn box_lines(c: char) -> Option<[u8; 4]> {
    Some(match c {
        '─' | '╌' | '┄' | '┈' => [0, 0, 1, 1],
        '━' | '╍' | '┅' | '┉' => [0, 0, 2, 2],
        '│' | '╎' | '┆' | '┊' => [1, 1, 0, 0],
        '┃' | '╏' | '┇' | '┋' => [2, 2, 0, 0],
        '┌' | '╭' => [0, 1, 0, 1],
        '┐' | '╮' => [0, 1, 1, 0],
        '└' | '╰' => [1, 0, 0, 1],
        '┘' | '╯' => [1, 0, 1, 0],
        '┏' => [0, 2, 0, 2],
        '┓' => [0, 2, 2, 0],
        '┗' => [2, 0, 0, 2],
        '┛' => [2, 0, 2, 0],
        '├' => [1, 1, 0, 1],
        '┤' => [1, 1, 1, 0],
        '┬' => [0, 1, 1, 1],
        '┴' => [1, 0, 1, 1],
        '┼' => [1, 1, 1, 1],
        '┣' => [2, 2, 0, 2],
        '┫' => [2, 2, 2, 0],
        '┳' => [0, 2, 2, 2],
        '┻' => [2, 0, 2, 2],
        '╋' => [2, 2, 2, 2],
        '┡' => [2, 1, 0, 2],
        '┩' => [2, 1, 2, 0],
        '╇' => [2, 1, 2, 2],
        '┢' => [1, 2, 0, 2],
        '┪' => [1, 2, 2, 0],
        '╈' => [1, 2, 2, 2],
        '═' => [0, 0, 3, 3],
        '║' => [3, 3, 0, 0],
        '╔' => [0, 3, 0, 3],
        '╗' => [0, 3, 3, 0],
        '╚' => [3, 0, 0, 3],
        '╝' => [3, 0, 3, 0],
        '╠' => [3, 3, 0, 3],
        '╣' => [3, 3, 3, 0],
        '╦' => [0, 3, 3, 3],
        '╩' => [3, 0, 3, 3],
        '╬' => [3, 3, 3, 3],
        _ => return None,
    })
}

/// Draw a box-drawing character over the cell at (left, top).
#[allow(clippy::too_many_arguments)]
fn draw_box(
    canvas: &mut Canvas,
    lines: [u8; 4],
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    size: f32,
    colour: Rgb,
) {
    let light = (size as f64 / 14.0).round().max(1.0);
    let thickness = |weight: u8| if weight >= 2 { light * 2.0 } else { light };
    let (cx, cy) = ((left + width / 2.0).floor(), (top + height / 2.0).floor());
    let [up, down, l, r] = lines;
    // Half-lines from the centre to each edge; the centre square is covered
    // by the widest line through it.
    let centre = [up, down, l, r]
        .into_iter()
        .map(thickness)
        .fold(0.0, f64::max);
    let half = |t: f64| (t / 2.0).floor();
    if up > 0 {
        let t = thickness(up);
        canvas.fill_rect(
            (cx - half(t)) as i64,
            top.floor() as i64,
            t as i64,
            (cy - top.floor() + half(centre) + 1.0) as i64,
            colour,
        );
    }
    if down > 0 {
        let t = thickness(down);
        canvas.fill_rect(
            (cx - half(t)) as i64,
            (cy - half(centre)) as i64,
            t as i64,
            ((top + height).ceil() - cy + half(centre)) as i64,
            colour,
        );
    }
    if l > 0 {
        let t = thickness(l);
        canvas.fill_rect(
            left.floor() as i64,
            (cy - half(t)) as i64,
            (cx - left.floor() + half(centre) + 1.0) as i64,
            t as i64,
            colour,
        );
    }
    if r > 0 {
        let t = thickness(r);
        canvas.fill_rect(
            (cx - half(centre)) as i64,
            (cy - half(t)) as i64,
            ((left + width).ceil() - cx + half(centre)) as i64,
            t as i64,
            colour,
        );
    }
}

/// A block element (U+2580–U+259F) as rectangles in eighths of the cell,
/// (x0, y0, x1, y1), and the coverage they are filled with: full, or a
/// shade for ░ ▒ ▓.
fn blocks(c: char) -> Option<(Vec<[u8; 4]>, u8)> {
    const QUADRANTS: [[u8; 4]; 4] = [[0, 0, 4, 4], [4, 0, 8, 4], [0, 4, 4, 8], [4, 4, 8, 8]];
    let code = c as u32;
    let rects = match code {
        0x2580 => vec![[0, 0, 8, 4]],
        0x2581..=0x2588 => vec![[0, (0x2588 - code) as u8, 8, 8]],
        0x2589..=0x258f => vec![[0, 0, (0x2590 - code) as u8, 8]],
        0x2590 => vec![[4, 0, 8, 8]],
        0x2591..=0x2593 => {
            return Some((vec![[0, 0, 8, 8]], [64, 128, 191][(code - 0x2591) as usize]))
        }
        0x2594 => vec![[0, 0, 8, 1]],
        0x2595 => vec![[7, 0, 8, 8]],
        0x2596..=0x259f => {
            // Upper left, upper right, lower left, lower right.
            let bits: u8 = [4, 8, 1, 13, 9, 7, 11, 2, 6, 14][(code - 0x2596) as usize];
            (0..4)
                .filter(|q| bits & (1 << q) != 0)
                .map(|q| QUADRANTS[q])
                .collect()
        }
        _ => return None,
    };
    Some((rects, 255))
}

/// Draw a block element over the cell at (left, top). Edges are rounded
/// to the same pixels as cell backgrounds, so neighbours meet exactly.
#[allow(clippy::too_many_arguments)]
fn draw_blocks(
    canvas: &mut Canvas,
    rects: &[[u8; 4]],
    alpha: u8,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    colour: Rgb,
) {
    let x = |eighths: u8| (left + width * eighths as f64 / 8.0).round() as i64;
    let y = |eighths: u8| (top + height * eighths as f64 / 8.0).round() as i64;
    for &[x0, y0, x1, y1] in rects {
        if alpha == 255 {
            canvas.fill_rect(x(x0), y(y0), x(x1) - x(x0), y(y1) - y(y0), colour);
        } else {
            for py in y(y0)..y(y1) {
                for px in x(x0)..x(x1) {
                    canvas.blend(px, py, colour, alpha);
                }
            }
        }
    }
}

/// Draw a braille pattern (U+2800–U+28FF): up to eight dots in two columns
/// of four.
fn draw_braille(
    canvas: &mut Canvas,
    dots: u8,
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    colour: Rgb,
) {
    // Bit order: dots 1–3 down the left, 4–6 down the right, then 7 and 8.
    const PLACES: [(u8, u8); 8] = [
        (0, 0),
        (0, 1),
        (0, 2),
        (1, 0),
        (1, 1),
        (1, 2),
        (0, 3),
        (1, 3),
    ];
    let radius = (width / 4.0).min(height / 8.0) * 0.7;
    for (bit, (column, row)) in PLACES.into_iter().enumerate() {
        if dots & (1 << bit) != 0 {
            let cx = left + width * (column as f64 * 2.0 + 1.0) / 4.0;
            let cy = top + height * (row as f64 * 2.0 + 1.0) / 8.0;
            canvas.fill_circle(cx, cy, radius, colour);
        }
    }
}

/// The faces used for text, with a glyph cache.
pub struct Fonts {
    regular: Font,
    bold: Font,
    emoji: Emoji,
    cache: Mutex<HashMap<GlyphKey, (Metrics, Vec<u8>)>>,
}

impl Default for Fonts {
    fn default() -> Self {
        Fonts::embedded()
    }
}

impl Fonts {
    /// DejaVu Sans Mono, regular and bold.
    pub fn embedded() -> Fonts {
        let load = |bytes: &'static [u8]| {
            Font::from_bytes(bytes, FontSettings::default()).expect("embedded font")
        };
        Fonts {
            regular: load(REGULAR),
            bold: load(BOLD),
            emoji: Emoji::default(),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// A font file for both regular and bold text.
    pub fn load(bytes: Vec<u8>) -> Result<Fonts, String> {
        let font = Font::from_bytes(bytes, FontSettings::default()).map_err(str::to_string)?;
        Ok(Fonts {
            regular: font.clone(),
            bold: font,
            emoji: Emoji::default(),
            cache: Mutex::new(HashMap::new()),
        })
    }

    fn face(&self, bold: bool) -> &Font {
        if bold {
            &self.bold
        } else {
            &self.regular
        }
    }

    /// The width of one cell and the ascent, at `size` pixels.
    fn metrics(&self, size: f32) -> (f64, f64) {
        let advance = self.regular.metrics('M', size).advance_width as f64;
        let ascent = self
            .regular
            .horizontal_line_metrics(size)
            .map_or(size * 0.8, |m| m.ascent) as f64;
        (advance, ascent)
    }

    /// Draw `c` with its origin at (x, baseline). A character the font does
    /// not have is drawn as an outlined box (tofu), `cells` wide.
    #[allow(clippy::too_many_arguments)]
    fn draw(
        &self,
        canvas: &mut Canvas,
        c: char,
        bold: bool,
        size: f32,
        x: f64,
        baseline: f64,
        cells: f64,
        colour: Rgb,
    ) {
        let face = self.face(bold);
        if !face.has_glyph(c) {
            let (advance, ascent) = self.metrics(size);
            let w = (advance * cells - 2.0).max(1.0);
            let top = baseline - ascent * 0.8;
            let h = ascent * 0.8;
            let (xi, yi) = ((x + 1.0) as i64, top as i64);
            canvas.fill_rect(xi, yi, w as i64, 1, colour);
            canvas.fill_rect(xi, (top + h) as i64, w as i64, 1, colour);
            canvas.fill_rect(xi, yi, 1, h as i64, colour);
            canvas.fill_rect((x + w) as i64, yi, 1, h as i64 + 1, colour);
            return;
        }
        let key = (c, bold, size.to_bits());
        let mut cache = self.cache.lock().expect("glyph cache");
        let (metrics, bitmap) = cache.entry(key).or_insert_with(|| face.rasterize(c, size));
        let left = x.round() as i64 + metrics.xmin as i64;
        let top = baseline.round() as i64 - metrics.ymin as i64 - metrics.height as i64;
        for gy in 0..metrics.height {
            for gx in 0..metrics.width {
                let alpha = bitmap[gy * metrics.width + gx];
                canvas.blend(left + gx as i64, top + gy as i64, colour, alpha);
            }
        }
    }

    /// Whether a cell's text is drawn as a colour emoji: a wide emoji, one
    /// with an emoji variation selector, ZWJ or skin tone, or one the text
    /// face lacks. Digits, `#` and `*` only as keycaps.
    fn is_emoji(&self, text: &str, wide: bool) -> bool {
        let Some(first) = text.chars().next() else {
            return false;
        };
        if first.is_ascii() {
            return text.contains('\u{20e3}') && self.emoji.has(first);
        }
        self.emoji.has(first)
            && (wide
                || !self.regular.has_glyph(first)
                || text
                    .chars()
                    .any(|c| matches!(c, '\u{fe0f}' | '\u{200d}' | '\u{1f3fb}'..='\u{1f3ff}')))
    }

    /// Draw a colour emoji centred in the box at (left, top).
    #[allow(clippy::too_many_arguments)]
    fn draw_emoji(
        &self,
        canvas: &mut Canvas,
        text: &str,
        left: f64,
        top: f64,
        width: f64,
        height: f64,
    ) -> bool {
        let side = (width.min(height) * 0.95).round().max(1.0) as usize;
        let Some(bitmap) = self.emoji.render(text, side) else {
            return false;
        };
        let x0 = (left + (width - side as f64) / 2.0).round() as i64;
        let y0 = (top + (height - side as f64) / 2.0).round() as i64;
        for (i, [r, g, b, a]) in bitmap.rgba.iter().copied().enumerate() {
            canvas.blend(x0 + (i % side) as i64, y0 + (i / side) as i64, (r, g, b), a);
        }
        true
    }

    fn text_width(&self, text: &str, size: f32) -> f64 {
        text.chars()
            .map(|c| self.bold.metrics(c, size).advance_width as f64)
            .sum()
    }
}

/// Options for [`render`].
#[derive(Clone, Debug)]
pub struct Frame<'a> {
    pub title: &'a str,
    /// The key-overlay label, when a key was just pressed.
    pub key: Option<&'a str>,
    /// Text size in pixels: 28 for screenshots, 16 for video.
    pub size: f32,
    /// Draw the window frame (title bar and buttons).
    pub window: bool,
    /// A line of text under the terminal.
    pub caption: Option<&'a str>,
}

/// The most pixels [`render`] is asked to draw for one image: 300 MB of RGB.
/// Callers check [`size`] against it first, so a huge terminal or font is an
/// error rather than an allocation that fails.
pub const MAX_PIXELS: usize = 100_000_000;

/// The line height, padding and title-bar height of a frame, in pixels.
fn layout(options: &Frame<'_>) -> (f64, f64, f64) {
    let size = options.size as f64;
    let bar = if options.window {
        (size * 2.2).round()
    } else {
        0.0
    };
    ((size * 1.3).round(), (size * 1.2).round(), bar)
}

/// The height of the caption bar, in pixels: none without a caption.
fn caption_height(options: &Frame<'_>) -> f64 {
    if options.caption.is_some() {
        (options.size as f64 * 2.2).round()
    } else {
        0.0
    }
}

/// The width and height, in pixels, of [`render`]'s image of a terminal of
/// `columns` x `rows`.
pub fn size(columns: usize, rows: usize, fonts: &Fonts, options: &Frame<'_>) -> (usize, usize) {
    let (cell, _) = fonts.metrics(options.size);
    let (line, pad, bar) = layout(options);
    (
        (columns as f64 * cell + 2.0 * pad).round() as usize,
        (rows as f64 * line + 2.0 * pad + bar + caption_height(options)).round() as usize,
    )
}

/// Draw `snapshot` as pixels.
pub fn render(snapshot: &Snapshot, theme: &Theme, fonts: &Fonts, options: &Frame<'_>) -> Canvas {
    let size = options.size;
    let (cell, ascent) = fonts.metrics(size);
    let (line, pad, bar) = layout(options);
    let (width, height) = self::size(snapshot.columns(), snapshot.rows.len(), fonts, options);
    let mut canvas = Canvas::new(
        width,
        height,
        if options.window {
            CHROME
        } else {
            theme.background
        },
    );
    if options.window {
        let (w, h, radius) = (width as f64, height as f64, pad / 2.0);
        canvas = Canvas::new(width, height, CHROME);
        canvas.fill_rounded(0.0, 0.0, w, h, radius, OUTLINE);
        canvas.fill_rounded(1.0, 1.0, w - 2.0, h - 2.0, radius, theme.background);
        canvas.fill_rounded(1.0, 1.0, w - 2.0, bar, radius, CHROME);
        canvas.fill_rect(
            1,
            (bar / 2.0) as i64,
            width as i64 - 2,
            (bar / 2.0) as i64,
            CHROME,
        );
        for (index, dot) in [(255, 95, 86), (255, 189, 46), (39, 201, 63)]
            .into_iter()
            .enumerate()
        {
            let r = size as f64 * 0.4;
            canvas.fill_circle(pad + index as f64 * size as f64 * 1.4, bar / 2.0, r, dot);
        }
        if !options.title.is_empty() {
            let title_size = size * 0.8;
            let tw = fonts.text_width(options.title, title_size);
            let mut x = (w - tw) / 2.0;
            let baseline = bar / 2.0 + title_size as f64 * 0.35;
            for c in options.title.chars() {
                fonts.draw(
                    &mut canvas,
                    c,
                    true,
                    title_size,
                    x,
                    baseline,
                    1.0,
                    (150, 150, 150),
                );
                x += fonts.bold.metrics(c, title_size).advance_width as f64;
            }
        }
    }
    let caption = caption_height(options);
    if let Some(text) = options.caption {
        let (w, h) = (width as f64, height as f64);
        let top = h - caption - if options.window { 1.0 } else { 0.0 };
        let colour = if options.window {
            let radius = pad / 2.0;
            canvas.fill_rounded(1.0, top, w - 2.0, caption, radius, CHROME);
            canvas.fill_rect(
                1,
                top as i64,
                width as i64 - 2,
                (caption / 2.0) as i64,
                CHROME,
            );
            (200, 200, 200)
        } else {
            canvas.fill_rect(
                0,
                top as i64,
                width as i64,
                caption as i64,
                theme.background,
            );
            theme.foreground
        };
        let text_size = size * 0.8;
        let tw = fonts.text_width(text, text_size);
        let mut x = ((w - tw) / 2.0).max(pad);
        let baseline = top + caption / 2.0 + text_size as f64 * 0.35;
        for c in text.chars() {
            fonts.draw(&mut canvas, c, false, text_size, x, baseline, 1.0, colour);
            x += fonts.bold.metrics(c, text_size).advance_width as f64;
        }
    }
    let top = bar + pad;
    let baseline_offset = (line - size as f64 * 1.17) / 2.0 + ascent;
    for (y, row) in snapshot.rows.iter().enumerate() {
        let row_top = top + y as f64 * line;
        for (x, cell_info) in row.iter().enumerate() {
            if cell_info.is_continuation() {
                continue;
            }
            let left = pad + x as f64 * cell;
            let span = cell_info.width.max(1) as f64;
            let mut fg = cell_info.fg;
            if cell_info.bg != theme.background {
                canvas.fill_rect(
                    left.round() as i64,
                    row_top.round() as i64,
                    (left + span * cell).round() as i64 - left.round() as i64,
                    (row_top + line).round() as i64 - row_top.round() as i64,
                    cell_info.bg,
                );
            }
            if snapshot.cursor == Some((x as u16, y as u16)) {
                canvas.fill_rect(
                    left.round() as i64,
                    row_top.round() as i64,
                    cell.round() as i64,
                    line as i64,
                    theme.foreground,
                );
                fg = theme.background;
            }
            let baseline = row_top + baseline_offset;
            let text = cell_info.text.as_str();
            let mut chars = text.chars();
            let only = match (chars.next(), chars.next()) {
                (Some(c), None) => Some(c),
                _ => None,
            };
            let (w, h) = (span * cell, line);
            if let Some(lines) = only.and_then(box_lines) {
                draw_box(&mut canvas, lines, left, row_top, cell, line, size, fg);
            } else if let Some((rects, alpha)) = only.and_then(blocks) {
                draw_blocks(&mut canvas, &rects, alpha, left, row_top, w, h, fg);
            } else if let Some(dots) = only
                .filter(|c| ('\u{2800}'..='\u{28ff}').contains(c))
                .map(|c| (c as u32 - 0x2800) as u8)
            {
                draw_braille(&mut canvas, dots, left, row_top, w, h, fg);
            } else if !(fonts.is_emoji(text, cell_info.width >= 2) && {
                // A narrow emoji (❤️, 1️⃣) spreads into a blank cell after
                // it, as terminals draw it.
                let spread = cell_info.width == 1
                    && row.get(x + 1).is_some_and(|next| {
                        next.text == " " && next.bg == cell_info.bg && !next.underline
                    });
                let w = if spread { 2.0 * cell } else { w };
                fonts.draw_emoji(&mut canvas, text, left, row_top, w, h)
            }) {
                // Joiners, selectors and tags the face lacks are
                // invisible, not tofu.
                let face = fonts.face(cell_info.bold);
                for c in text
                    .chars()
                    .filter(|c| *c != ' ' && (c.width() != Some(0) || face.has_glyph(*c)))
                {
                    fonts.draw(
                        &mut canvas,
                        c,
                        cell_info.bold,
                        size,
                        left,
                        baseline,
                        span,
                        fg,
                    );
                }
            }
            if cell_info.underline {
                canvas.fill_rect(
                    left.round() as i64,
                    (row_top + line - 3.0) as i64,
                    (span * cell).round() as i64,
                    1.max((size / 16.0) as i64),
                    fg,
                );
            }
        }
    }
    if let Some(key) = options.key {
        let label_size = size;
        let label: String = format!(" {key} ");
        let w = fonts.text_width(&label, label_size) + size as f64;
        let h = size as f64 * 2.0;
        let (x1, y1) = (width as f64 - pad, height as f64 - pad - caption);
        canvas.fill_rounded(
            x1 - w - 1.0,
            y1 - h - 1.0,
            w + 2.0,
            h + 2.0,
            h / 3.0,
            (120, 120, 120),
        );
        canvas.fill_rounded(x1 - w, y1 - h, w, h, h / 3.0, (20, 20, 20));
        let mut x = x1 - w + size as f64 / 2.0;
        let baseline = y1 - h / 2.0 + label_size as f64 * 0.35;
        for c in label.chars() {
            fonts.draw(
                &mut canvas,
                c,
                true,
                label_size,
                x,
                baseline,
                1.0,
                (240, 240, 240),
            );
            x += fonts.bold.metrics(c, label_size).advance_width as f64;
        }
    }
    canvas
}

/// The smallest rectangle (x, y, width, height) where `a` and `b` differ.
fn changed(a: &Canvas, b: &Canvas) -> Option<(usize, usize, usize, usize)> {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for y in 0..a.height {
        let row = y * a.width * 3;
        for x in 0..a.width {
            let i = row + x * 3;
            if a.pixels[i..i + 3] != b.pixels[i..i + 3] {
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    (x0 != usize::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

/// A hasher for packed RGB keys: one multiply, where SipHash was most of the
/// palette's cost.
#[derive(Default)]
struct RgbHasher(u64);

impl std::hash::Hasher for RgbHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 = (self.0 << 8 | *byte as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        }
    }
    fn write_u32(&mut self, value: u32) {
        self.0 = (value as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    }
}

type RgbMap<V> = HashMap<u32, V, std::hash::BuildHasherDefault<RgbHasher>>;

fn rgb(pixel: &[u8]) -> u32 {
    (pixel[0] as u32) << 16 | (pixel[1] as u32) << 8 | pixel[2] as u32
}

/// The pixels of each frame that the GIF stores: all of the first frame, then
/// the rectangle that changed (None when nothing did).
type Region = Option<(usize, usize, usize, usize)>;

/// One palette for a whole GIF, and the index of every colour in it, from
/// how often each colour is stored.
///
/// Terminal frames use few colours, so when the stored pixels together have
/// 256 or fewer the palette is exact. Otherwise NeuQuant is trained once, on
/// the distinct colours weighted by use, and each distinct colour is mapped
/// to its nearest entry once: quantising every frame was the slow part of
/// GIF encoding.
fn palette(colours: RgbMap<u32>) -> (Vec<u8>, RgbMap<u8>) {
    let unpack = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
    if colours.len() <= 256 {
        let mut sorted: Vec<u32> = colours.into_keys().collect();
        sorted.sort_unstable();
        let palette = sorted.iter().flat_map(|c| unpack(*c)).collect();
        let index = sorted
            .into_iter()
            .enumerate()
            .map(|(i, c)| (c, i as u8))
            .collect();
        return (palette, index);
    }
    // Weighted by use (capped, so flat backgrounds cannot crowd out text),
    // and at most ~100k samples whatever the recording's length.
    let total: u64 = colours.values().map(|n| (*n).min(32) as u64).sum();
    let scale = (100_000.0 / total as f64).min(1.0);
    let mut sample = Vec::new();
    for (colour, count) in &colours {
        let repeats = (((*count).min(32) as f64 * scale).ceil() as usize).max(1);
        let [r, g, b] = unpack(*colour);
        for _ in 0..repeats {
            sample.extend_from_slice(&[r, g, b, 255]);
        }
    }
    let quantizer = color_quant::NeuQuant::new(10, 256, &sample);
    let palette = quantizer.color_map_rgb();
    let index = colours
        .into_keys()
        .map(|c| {
            let [r, g, b] = unpack(c);
            (c, quantizer.index_of(&[r, g, b, 255]) as u8)
        })
        .collect();
    (palette, index)
}

/// `canvas` centred on a `width` x `height` one, unless it is that size.
fn fit(canvas: Canvas, width: usize, height: usize) -> Canvas {
    if (canvas.width, canvas.height) == (width, height) {
        return canvas;
    }
    let mut padded = Canvas::new(width, height, CHROME);
    padded.paste(
        &canvas,
        width.saturating_sub(canvas.width) / 2,
        height.saturating_sub(canvas.height) / 2,
    );
    padded
}

/// The pixels of `canvas` that the GIF stores, given the frame before it.
fn region(previous: Option<&Canvas>, canvas: &Canvas) -> Region {
    match previous {
        None => Some((0, 0, canvas.width, canvas.height)),
        Some(previous) => changed(previous, canvas),
    }
}

/// The rows of `region` in `canvas`, as packed RGB pixels.
fn pixels(
    canvas: &Canvas,
    (x, y, w, h): (usize, usize, usize, usize),
) -> impl Iterator<Item = u32> + '_ {
    (y..y + h).flat_map(move |row| {
        let start = (row * canvas.width + x) * 3;
        canvas.pixels[start..start + w * 3]
            .as_chunks::<3>()
            .0
            .iter()
            .map(|pixel| rgb(pixel))
    })
}

fn gif_error(error: gif::EncodingError) -> std::io::Error {
    match error {
        gif::EncodingError::Io(error) => error,
        other => std::io::Error::other(other),
    }
}

/// Frames for [`gif_streamed`]: each call hands every frame, in order, with
/// how long it lasts in seconds, to the sink it is given.
pub type Sink<'a> = dyn FnMut(Canvas, f64) -> std::io::Result<()> + 'a;

/// Encode frames as a looping `width` x `height` GIF with one global
/// palette, holding at most two frames in memory. `frames` is called twice,
/// and must hand the same frames both times: once to choose the palette, once
/// to encode. Smaller frames are centred. After the first, each frame stores
/// only the rectangle that changed, drawn over the previous one.
pub fn gif_streamed(
    width: usize,
    height: usize,
    mut frames: impl FnMut(&mut Sink<'_>) -> std::io::Result<()>,
    out: impl Write,
) -> std::io::Result<()> {
    let limit = u16::MAX as usize;
    if width == 0 || height == 0 || width > limit || height > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("a GIF of {width}x{height} pixels is larger than GIF allows ({limit}x{limit})"),
        ));
    }
    // Pass one: how often each stored colour is used.
    let mut colours: RgbMap<u32> = RgbMap::default();
    let mut previous: Option<Canvas> = None;
    frames(&mut |canvas, _| {
        let canvas = fit(canvas, width, height);
        if let Some(area) = region(previous.as_ref(), &canvas) {
            for colour in pixels(&canvas, area) {
                *colours.entry(colour).or_default() += 1;
            }
        }
        previous = Some(canvas);
        Ok(())
    })?;
    let (palette, index) = palette(colours);
    // Pass two: encode.
    let mut encoder =
        gif::Encoder::new(out, width as u16, height as u16, &palette).map_err(gif_error)?;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(gif_error)?;
    let delay = |seconds: f64| (seconds * 100.0).round().clamp(2.0, 65535.0) as u16;
    // A frame and the delay it has accumulated, waiting to be written.
    let mut pending: Option<gif::Frame<'static>> = None;
    let mut previous: Option<Canvas> = None;
    frames(&mut |canvas, seconds| {
        let canvas = fit(canvas, width, height);
        let area = region(previous.as_ref(), &canvas);
        let Some((x, y, w, h)) = area else {
            // Nothing changed: the frame on hold simply lasts longer.
            if let Some(frame) = pending.as_mut() {
                frame.delay = frame.delay.saturating_add(delay(seconds));
            }
            return Ok(());
        };
        if let Some(frame) = pending.take() {
            encoder.write_frame(&frame).map_err(gif_error)?;
        }
        let mut indices = Vec::with_capacity(w * h);
        for colour in pixels(&canvas, (x, y, w, h)) {
            // Pass two hands a colour pass one did not: `frames` changed.
            let Some(i) = index.get(&colour) else {
                return Err(std::io::Error::other("GIF frames differ between passes"));
            };
            indices.push(*i);
        }
        let mut frame = gif::Frame::from_indexed_pixels(w as u16, h as u16, indices, None);
        frame.left = x as u16;
        frame.top = y as u16;
        frame.dispose = gif::DisposalMethod::Keep;
        frame.delay = delay(seconds);
        pending = Some(frame);
        previous = Some(canvas);
        Ok(())
    })?;
    if let Some(frame) = pending {
        encoder.write_frame(&frame).map_err(gif_error)?;
    }
    Ok(())
}

/// [`gif_streamed`] for frames already drawn, on a canvas the size of the
/// largest.
pub fn gif(frames: &[(Canvas, f64)], out: impl Write) -> std::io::Result<()> {
    let width = frames.iter().map(|(c, _)| c.width).max().unwrap_or(1);
    let height = frames.iter().map(|(c, _)| c.height).max().unwrap_or(1);
    gif_streamed(
        width,
        height,
        |sink| {
            frames
                .iter()
                .try_for_each(|(canvas, seconds)| sink(canvas.clone(), *seconds))
        },
        out,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(bytes: &[u8]) -> Snapshot {
        let mut parser = vt100::Parser::new(3, 12, 0);
        parser.process(bytes);
        Snapshot::from_screen(parser.screen(), &Theme::default())
    }

    #[test]
    fn draws_text_in_a_window() {
        let fonts = Fonts::embedded();
        let theme = Theme::default();
        let frame = Frame {
            title: "t",
            key: Some("⏎"),
            size: 16.0,
            window: true,
            caption: None,
        };
        let canvas = render(&shot(b"\x1b[31mhello\x1b[0m"), &theme, &fonts, &frame);
        assert!(canvas.width > 100 && canvas.height > 50);
        // Red text pixels exist; the corner is the window chrome.
        let red = theme.ansi[1];
        let has_red = canvas.pixels.chunks(3).any(|p| (p[0], p[1], p[2]) == red);
        assert!(has_red);
        assert_eq!(&canvas.pixels[..3], &[CHROME.0, CHROME.1, CHROME.2]);
        let png = canvas.png();
        assert_eq!(&png[1..4], b"PNG");
    }

    /// The original per-pixel `fill_rounded`, before the row spans.
    fn reference_rounded(
        canvas: &mut Canvas,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        radius: f64,
        colour: Rgb,
    ) {
        let (x1, y1) = (x + width, y + height);
        for py in y.floor() as i64..y1.ceil() as i64 {
            for px in x.floor() as i64..x1.ceil() as i64 {
                let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
                let dx = (x + radius - cx).max(cx - (x1 - radius)).max(0.0);
                let dy = (y + radius - cy).max(cy - (y1 - radius)).max(0.0);
                let outside = (dx * dx + dy * dy).sqrt() - radius;
                let edge = (cx - x).min(x1 - cx).min(cy - y).min(y1 - cy);
                let coverage = (0.5 - outside).clamp(0.0, 1.0) * (edge + 0.5).clamp(0.0, 1.0);
                canvas.blend(px, py, colour, (coverage * 255.0) as u8);
            }
        }
    }

    #[test]
    fn rounded_fill_matches_the_per_pixel_reference() {
        let shapes = [
            (0.0, 0.0, 40.0, 30.0, 6.0),
            (1.0, 1.0, 38.0, 12.0, 6.0),
            (3.5, 2.25, 20.5, 25.75, 4.5),
            (5.0, 5.0, 10.0, 10.0, 5.0),
            (0.0, 0.0, 40.0, 40.0, 0.0),
            (-3.0, -2.0, 30.0, 8.0, 10.0),
        ];
        for (x, y, w, h, r) in shapes {
            let mut fast = Canvas::new(48, 48, (10, 20, 30));
            let mut slow = fast.clone();
            fast.fill_rounded(x, y, w, h, r, (200, 100, 50));
            reference_rounded(&mut slow, x, y, w, h, r, (200, 100, 50));
            assert!(fast == slow, "shape {:?} differs", (x, y, w, h, r));
        }
    }

    #[test]
    fn gif_pads_frames_to_one_size() {
        let a = Canvas::new(10, 10, (255, 0, 0));
        let b = Canvas::new(6, 4, (0, 0, 255));
        let mut out = Vec::new();
        gif(&[(a, 0.5), (b, 1.0)], &mut out).unwrap();
        assert_eq!(&out[..6], b"GIF89a");
        assert_eq!(u16::from_le_bytes([out[6], out[7]]), 10);
    }

    #[test]
    fn gif_stores_only_what_changed() {
        let a = Canvas::new(20, 20, (0, 0, 0));
        let mut b = a.clone();
        b.fill_rect(5, 6, 3, 2, (255, 255, 255));
        assert_eq!(changed(&a, &b), Some((5, 6, 3, 2)));
        assert_eq!(changed(&a, &a), None);
        let mut out = Vec::new();
        gif(&[(a.clone(), 0.5), (b, 0.5), (a, 0.5)], &mut out).unwrap();
        let mut decoder = gif::DecodeOptions::new().read_info(&out[..]).unwrap();
        let mut sizes = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            sizes.push((frame.left, frame.top, frame.width, frame.height));
        }
        assert_eq!(sizes, [(0, 0, 20, 20), (5, 6, 3, 2), (5, 6, 3, 2)]);
    }

    #[test]
    fn gif_larger_than_the_format_allows_is_an_error() {
        let mut calls = 0;
        let error = gif_streamed(
            70_000,
            10,
            |_| {
                calls += 1;
                Ok(())
            },
            Vec::new(),
        )
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error.to_string().contains("70000x10"), "{error}");
        // Refused before a frame is drawn.
        assert_eq!(calls, 0);
    }

    #[test]
    fn gif_frames_are_streamed_twice() {
        let a = Canvas::new(8, 8, (0, 0, 0));
        let mut b = a.clone();
        b.fill_rect(1, 1, 2, 2, (200, 10, 10));
        let mut passes = 0;
        let mut streamed = Vec::new();
        gif_streamed(
            8,
            8,
            |sink| {
                passes += 1;
                sink(a.clone(), 0.5)?;
                sink(b.clone(), 0.5)
            },
            &mut streamed,
        )
        .unwrap();
        assert_eq!(passes, 2);
        let mut whole = Vec::new();
        gif(&[(a, 0.5), (b, 0.5)], &mut whole).unwrap();
        assert_eq!(streamed, whole);
    }

    #[test]
    fn size_matches_what_render_draws() {
        let fonts = Fonts::embedded();
        let options = Frame {
            title: "t",
            key: None,
            size: 16.0,
            window: true,
            caption: None,
        };
        let canvas = render(&shot(b"hi"), &Theme::default(), &fonts, &options);
        assert_eq!(size(12, 3, &fonts, &options), (canvas.width, canvas.height));
    }

    #[test]
    fn a_caption_adds_a_bar_under_the_terminal() {
        let fonts = Fonts::embedded();
        let theme = Theme::default();
        for window in [true, false] {
            let plain = Frame {
                title: "t",
                key: None,
                size: 16.0,
                window,
                caption: None,
            };
            let captioned = Frame {
                caption: Some("Saved"),
                ..plain.clone()
            };
            let (_, h) = size(12, 3, &fonts, &plain);
            let canvas = render(&shot(b"hi"), &theme, &fonts, &captioned);
            assert_eq!(
                size(12, 3, &fonts, &captioned),
                (canvas.width, canvas.height)
            );
            assert!(canvas.height > h, "window={window}");
            // Caption text is drawn in the new bar.
            let bar = &canvas.pixels[h * canvas.width * 3..];
            let background = if window { CHROME } else { theme.background };
            assert!(
                bar.chunks(3)
                    .any(|p| (p[0], p[1], p[2]) != background && (p[0], p[1], p[2]) != OUTLINE),
                "window={window}"
            );
        }
    }
}

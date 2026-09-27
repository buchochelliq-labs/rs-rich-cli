//! Screenshots and video frames as pixels: PNG and GIF.
//!
//! Glyphs come from DejaVu Sans Mono, embedded in the crate (Bitstream Vera
//! licence, `fonts/LICENSE-DejaVu`), so output is identical on every machine.
//! [`Fonts::load`] takes another font instead. Italic is drawn upright:
//! only the regular and bold faces are embedded.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;

use fontdue::{Font, FontSettings, Metrics};

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
        let a = alpha as u32;
        for (offset, c) in [colour.0, colour.1, colour.2].into_iter().enumerate() {
            let old = self.pixels[i + offset] as u32;
            self.pixels[i + offset] = ((c as u32 * a + old * (255 - a)) / 255) as u8;
        }
    }

    pub fn fill_rect(&mut self, x: i64, y: i64, width: i64, height: i64, colour: Rgb) {
        for py in y.max(0)..(y + height).min(self.height as i64) {
            for px in x.max(0)..(x + width).min(self.width as i64) {
                self.blend(px, py, colour, 255);
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
        for py in y.floor() as i64..y1.ceil() as i64 {
            for px in x.floor() as i64..x1.ceil() as i64 {
                let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
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

/// The faces used for text, with a glyph cache.
pub struct Fonts {
    regular: Font,
    bold: Font,
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
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// A font file for both regular and bold text.
    pub fn load(bytes: Vec<u8>) -> Result<Fonts, String> {
        let font = Font::from_bytes(bytes, FontSettings::default()).map_err(str::to_string)?;
        Ok(Fonts {
            regular: font.clone(),
            bold: font,
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
}

/// Draw `snapshot` as pixels.
pub fn render(snapshot: &Snapshot, theme: &Theme, fonts: &Fonts, options: &Frame<'_>) -> Canvas {
    let size = options.size;
    let (cell, ascent) = fonts.metrics(size);
    let line = (size as f64 * 1.3).round();
    let pad = (size as f64 * 1.2).round();
    let bar = if options.window {
        (size as f64 * 2.2).round()
    } else {
        0.0
    };
    let columns = snapshot.columns() as f64;
    let width = (columns * cell + 2.0 * pad).round() as usize;
    let height = (snapshot.rows.len() as f64 * line + 2.0 * pad + bar).round() as usize;
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
            let mut chars = cell_info.text.chars();
            match (chars.next().and_then(box_lines), chars.next()) {
                (Some(lines), None) => {
                    draw_box(&mut canvas, lines, left, row_top, cell, line, size, fg)
                }
                _ => {
                    for c in cell_info.text.chars().filter(|c| *c != ' ') {
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
        let (x1, y1) = (width as f64 - pad, height as f64 - pad);
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

/// Encode frames as a looping GIF; frames of different sizes are centred on
/// one canvas. After the first, each frame stores only the rectangle that
/// changed, drawn over the previous one. `delays` are in seconds.
pub fn gif(frames: &[(Canvas, f64)], out: impl Write) -> Result<(), gif::EncodingError> {
    let width = frames.iter().map(|(c, _)| c.width).max().unwrap_or(1);
    let height = frames.iter().map(|(c, _)| c.height).max().unwrap_or(1);
    let mut encoder = gif::Encoder::new(out, width as u16, height as u16, &[])?;
    encoder.set_repeat(gif::Repeat::Infinite)?;
    let mut previous: Option<Canvas> = None;
    // A frame and the delay it has accumulated, waiting to be written.
    let mut pending: Option<gif::Frame<'static>> = None;
    for (canvas, seconds) in frames {
        let full = if (canvas.width, canvas.height) == (width, height) {
            canvas.clone()
        } else {
            let mut padded = Canvas::new(width, height, CHROME);
            padded.paste(
                canvas,
                (width - canvas.width) / 2,
                (height - canvas.height) / 2,
            );
            padded
        };
        let delay = |seconds: f64| (seconds * 100.0).round().clamp(2.0, 65535.0) as u16;
        let rect = match &previous {
            None => Some((0, 0, width, height)),
            Some(before) => changed(before, &full),
        };
        match rect {
            // Nothing changed: the frame on hold simply lasts longer.
            None => {
                if let Some(frame) = pending.as_mut() {
                    frame.delay = frame.delay.saturating_add(delay(*seconds));
                }
            }
            Some((x, y, w, h)) => {
                if let Some(frame) = pending.take() {
                    encoder.write_frame(&frame)?;
                }
                let mut pixels = Vec::with_capacity(w * h * 3);
                for row in y..y + h {
                    let start = (row * width + x) * 3;
                    pixels.extend_from_slice(&full.pixels[start..start + w * 3]);
                }
                let mut frame = gif::Frame::from_rgb_speed(w as u16, h as u16, &pixels, 20);
                frame.left = x as u16;
                frame.top = y as u16;
                frame.dispose = gif::DisposalMethod::Keep;
                frame.delay = delay(*seconds);
                pending = Some(frame);
            }
        }
        previous = Some(full);
    }
    if let Some(frame) = pending {
        encoder.write_frame(&frame)?;
    }
    Ok(())
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
}

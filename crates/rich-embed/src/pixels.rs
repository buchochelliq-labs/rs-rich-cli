//! Pixel frames drawn in cells: each cell is an upper half block (`▀`)
//! whose foreground is the upper pixel and whose background the lower one,
//! so a pane of `columns` x `rows` cells shows `columns` x `2 * rows`
//! pixels, averaged down from the frame.

use rich::{Color, Segment, Style};

/// A pixel's red, green and blue.
type Rgb = (u8, u8, u8);

/// A frame of pixels: `width` x `height`, three bytes (red, green, blue)
/// per pixel, rows top to bottom.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

impl Pixels {
    /// A frame from its RGB bytes. `None` when there are not
    /// `width * height * 3` of them.
    pub fn new(width: u32, height: u32, rgb: Vec<u8>) -> Option<Pixels> {
        (rgb.len() as u64 == width as u64 * height as u64 * 3).then_some(Pixels {
            width,
            height,
            rgb,
        })
    }

    /// The `width` x `height` pixels from (`x`, `y`), clipped to the
    /// frame. `None` when nothing of it is in the frame.
    pub fn crop(&self, x: u32, y: u32, width: u32, height: u32) -> Option<Pixels> {
        let x1 = x.saturating_add(width).min(self.width);
        let y1 = y.saturating_add(height).min(self.height);
        if x >= x1 || y >= y1 {
            return None;
        }
        let mut rgb = Vec::with_capacity(((x1 - x) * (y1 - y) * 3) as usize);
        for row in y..y1 {
            let start = ((row * self.width + x) * 3) as usize;
            let end = ((row * self.width + x1) * 3) as usize;
            rgb.extend_from_slice(&self.rgb[start..end]);
        }
        Pixels::new(x1 - x, y1 - y, rgb)
    }

    /// The pixels as red, green, blue and alpha (opaque), for encoders that
    /// take RGBA.
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgb.len() / 3 * 4);
        for pixel in self.rgb.as_chunks::<3>().0 {
            out.extend_from_slice(pixel);
            out.push(255);
        }
        out
    }

    /// The average colour of the pixels in `x0..x1` x `y0..y1`.
    fn average(&self, x0: u32, x1: u32, y0: u32, y1: u32) -> (u8, u8, u8) {
        let mut sum = [0u64; 3];
        let mut n = 0u64;
        for y in y0..y1 {
            let row = (y * self.width) as usize * 3;
            for x in x0..x1 {
                let at = row + x as usize * 3;
                sum[0] += self.rgb[at] as u64;
                sum[1] += self.rgb[at + 1] as u64;
                sum[2] += self.rgb[at + 2] as u64;
                n += 1;
            }
        }
        let n = n.max(1);
        ((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8)
    }
}

/// The part of `0..size` that output cell `i` of `count` covers: at least
/// one pixel.
fn span(i: u32, count: u32, size: u32) -> (u32, u32) {
    let start = (i as u64 * size as u64 / count as u64) as u32;
    let end = ((i as u64 + 1) * size as u64 / count as u64) as u32;
    let start = start.min(size.saturating_sub(1));
    (start, end.max(start + 1).min(size))
}

/// `pixels` scaled to `columns` x `rows` cells of half blocks.
pub fn half_blocks(pixels: &Pixels, columns: u16, rows: u16) -> Vec<Vec<Segment>> {
    if pixels.width == 0 || pixels.height == 0 || columns == 0 || rows == 0 {
        return Vec::new();
    }
    let (columns, rows) = (columns as u32, rows as u32);
    let xs: Vec<(u32, u32)> = (0..columns)
        .map(|x| span(x, columns, pixels.width))
        .collect();
    let mut lines = Vec::with_capacity(rows as usize);
    for row in 0..rows {
        let (top0, top1) = span(row * 2, rows * 2, pixels.height);
        let (bottom0, bottom1) = span(row * 2 + 1, rows * 2, pixels.height);
        let mut line: Vec<Segment> = Vec::new();
        let mut run = 0usize;
        let mut current: Option<(Rgb, Rgb)> = None;
        for &(x0, x1) in &xs {
            let colours = (
                pixels.average(x0, x1, top0, top1),
                pixels.average(x0, x1, bottom0, bottom1),
            );
            if current.is_some_and(|c| c != colours) {
                line.push(block(run, current.expect("colours")));
                run = 0;
            }
            current = Some(colours);
            run += 1;
        }
        if let Some(colours) = current {
            line.push(block(run, colours));
        }
        lines.push(line);
    }
    lines
}

fn block(run: usize, (top, bottom): (Rgb, Rgb)) -> Segment {
    let style = Style::from_color(
        Some(Color::from_rgb(top.0, top.1, top.2)),
        Some(Color::from_rgb(bottom.0, bottom.1, bottom.2)),
    );
    Segment::new("▀".repeat(run), Some(style))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cell_is_two_pixels_averaged_down() {
        // 4 x 2: the left half red over blue, the right half white over black.
        let mut rgb = Vec::new();
        for row in [[255, 0, 0, 255, 255, 255], [0, 0, 255, 0, 0, 0]] {
            for half in [&row[..3], &row[..3], &row[3..], &row[3..]] {
                rgb.extend_from_slice(half);
            }
        }
        let pixels = Pixels::new(4, 2, rgb).unwrap();
        let lines = half_blocks(&pixels, 2, 1);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0].text, "▀");
        let style = lines[0][0].style.as_ref().unwrap();
        assert_eq!(style.color(), Some(&Color::from_rgb(255, 0, 0)));
        assert_eq!(style.bgcolor(), Some(&Color::from_rgb(0, 0, 255)));
        // Scaled up, neighbours of one colour merge into one run.
        let wide = half_blocks(&pixels, 8, 3);
        assert_eq!(wide.len(), 3);
        assert_eq!(wide[0][0].text, "▀▀▀▀");
        assert!(Pixels::new(2, 2, vec![0; 3]).is_none());
    }

    #[test]
    fn crops_clip_to_the_frame() {
        // 3 x 2, each pixel's red its index.
        let rgb = (0..6u8).flat_map(|i| [i, 0, 0]).collect();
        let pixels = Pixels::new(3, 2, rgb).unwrap();
        let part = pixels.crop(1, 0, 5, 1).unwrap();
        assert_eq!((part.width, part.height), (2, 1));
        assert_eq!(part.rgb, vec![1, 0, 0, 2, 0, 0]);
        let lower = pixels.crop(0, 1, 1, 9).unwrap();
        assert_eq!(lower.rgb, vec![3, 0, 0]);
        assert!(pixels.crop(3, 0, 1, 1).is_none());
        assert_eq!(part.to_rgba(), vec![1, 0, 0, 255, 2, 0, 0, 255]);
    }
}

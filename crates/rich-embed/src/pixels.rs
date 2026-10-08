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
}

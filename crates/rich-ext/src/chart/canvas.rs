//! A Braille dot canvas: 2×4 dots per cell.

/// The bit for the dot at column `x` (0–1) and row `y` (0–3) of a Braille
/// cell, in Unicode's dot numbering.
const DOT_BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// The longest side of a [`DotCanvas`], in cells.
const MAX_SIDE: usize = u16::MAX as usize;

/// A grid of Braille cells you set dots on, 2 dots wide and 4 tall per cell.
///
/// Each cell remembers which layer (series) last set a dot in it, so a chart
/// can colour it. Dots outside the canvas are ignored.
///
/// ```
/// use rich_ext::chart::DotCanvas;
///
/// let mut canvas = DotCanvas::new(2, 1);
/// canvas.line((0, 3), (3, 0), 0);
/// assert_eq!(canvas.row_string(0), "⡠⠊");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DotCanvas {
    width: usize,
    height: usize,
    bits: Vec<u8>,
    layer: Vec<Option<usize>>,
}

impl DotCanvas {
    /// A blank canvas `width` × `height` cells (`2 * width` × `4 * height`
    /// dots). Each side is at most 65535 cells; larger sizes are capped.
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.min(MAX_SIDE);
        let height = height.min(MAX_SIDE);
        DotCanvas {
            width,
            height,
            bits: vec![0; width * height],
            layer: vec![None; width * height],
        }
    }

    /// Width in dots.
    pub fn dot_width(&self) -> usize {
        self.width * 2
    }

    /// Height in dots.
    pub fn dot_height(&self) -> usize {
        self.height * 4
    }

    /// Width in cells.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Height in cells.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Set the dot at (`x`, `y`), counted from the top left, for `layer`.
    pub fn set(&mut self, x: i64, y: i64, layer: usize) {
        if x < 0 || y < 0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.dot_width() || y >= self.dot_height() {
            return;
        }
        let cell = (y / 4) * self.width + x / 2;
        self.bits[cell] |= DOT_BITS[x % 2][y % 4];
        self.layer[cell] = Some(layer);
    }

    /// Set every dot on the straight line from `a` to `b` (Bresenham).
    pub fn line(&mut self, a: (i64, i64), b: (i64, i64), layer: usize) {
        for (x, y) in bresenham(a, b) {
            self.set(x, y, layer);
        }
    }

    /// The Braille character of a cell (`⠀`, U+2800, when empty) and the
    /// layer that last drew in it.
    pub fn cell(&self, col: usize, row: usize) -> (char, Option<usize>) {
        let i = row * self.width + col;
        let c = char::from_u32(0x2800 + u32::from(self.bits[i])).unwrap_or(' ');
        (c, self.layer[i])
    }

    /// Whether a cell has any dot set.
    pub fn is_set(&self, col: usize, row: usize) -> bool {
        self.bits[row * self.width + col] != 0
    }

    /// A row of cells as text, empty cells as spaces.
    pub fn row_string(&self, row: usize) -> String {
        (0..self.width)
            .map(|col| {
                if self.is_set(col, row) {
                    self.cell(col, row).0
                } else {
                    ' '
                }
            })
            .collect()
    }
}

/// The points on the line from `a` to `b`, both ends included.
pub(crate) fn bresenham(a: (i64, i64), b: (i64, i64)) -> Vec<(i64, i64)> {
    let (mut x0, mut y0) = a;
    let (x1, y1) = b;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut out = Vec::with_capacity((dx.max(-dy) + 1) as usize);
    loop {
        out.push((x0, y0));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dots_map_to_unicode_braille_bits() {
        let mut c = DotCanvas::new(1, 1);
        for y in 0..4 {
            c.set(0, y, 0);
        }
        assert_eq!(c.row_string(0), "⡇");
        for y in 0..4 {
            c.set(1, y, 1);
        }
        assert_eq!(c.cell(0, 0), ('⣿', Some(1)));
        // Out of range is ignored.
        c.set(-1, 0, 0);
        c.set(2, 0, 0);
        c.set(0, 4, 0);
        assert_eq!(c.cell(0, 0).0, '⣿');
    }

    #[test]
    fn lines_include_both_ends() {
        assert_eq!(bresenham((0, 0), (3, 0)).len(), 4);
        assert_eq!(bresenham((0, 0), (0, 0)), vec![(0, 0)]);
        assert_eq!(bresenham((2, 2), (0, 0)), vec![(2, 2), (1, 1), (0, 0)]);
        let mut c = DotCanvas::new(2, 1);
        c.line((0, 3), (3, 0), 0);
        assert_eq!(c.row_string(0), "⡠⠊");
    }

    #[test]
    fn zero_sized_canvas_is_harmless() {
        let mut c = DotCanvas::new(0, 0);
        c.set(0, 0, 0);
        c.line((0, 0), (5, 5), 0);
        assert_eq!(c.dot_width(), 0);
    }
}

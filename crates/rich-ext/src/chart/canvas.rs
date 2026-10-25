//! A Braille dot canvas: 2×4 dots per cell.

/// The bit for the dot at column `x` (0–1) and row `y` (0–3) of a Braille
/// cell, in Unicode's dot numbering.
const DOT_BITS: [[u8; 4]; 2] = [[0x01, 0x02, 0x04, 0x40], [0x08, 0x10, 0x20, 0x80]];

/// [`DotCanvas::layer`] of a cell no layer drew in.
const NO_LAYER: u32 = u32::MAX;

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
    /// The layer that last set a dot in each cell, [`NO_LAYER`] for none:
    /// four bytes a cell, not an `Option<usize>`'s sixteen.
    layer: Vec<u32>,
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
            layer: vec![NO_LAYER; width * height],
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
        self.layer[cell] = u32::try_from(layer)
            .unwrap_or(NO_LAYER - 1)
            .min(NO_LAYER - 1);
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
        let layer = self.layer[i];
        (c, (layer != NO_LAYER).then_some(layer as usize))
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

/// The block-element character for a 2×2 pattern of quarters. Bit
/// `x + 2 * y` is the quarter at column `x` and row `y`, from the top left.
pub(crate) fn quadrant(bits: u8) -> char {
    // Unicode's Block Elements, which hold every pattern of quarters.
    const BY_PATTERN: [char; 16] = [
        ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
    ];
    BY_PATTERN[usize::from(bits & 0x0f)]
}

/// The character for a 2×3 pattern of sixths, bit `x + 2 * y` set for the
/// sixth at column `x` and row `y`: Symbols for Legacy Computing (Unicode
/// 13), which encode the patterns in order from U+1FB00 and leave out the
/// four that Block Elements already has (blank, the two halves, full).
pub(crate) fn sextant(bits: u8) -> char {
    const LEFT: u8 = 0b01_0101;
    const RIGHT: u8 = 0b10_1010;
    match bits & 0x3f {
        0 => ' ',
        LEFT => '▌',
        RIGHT => '▐',
        0x3f => '█',
        pattern => {
            let before = 1 + u32::from(pattern > LEFT) + u32::from(pattern > RIGHT);
            char::from_u32(0x1FB00 + u32::from(pattern) - before).unwrap_or(' ')
        }
    }
}

/// Octant patterns that Unicode 16 did not encode again, because a block
/// character already draws them, sorted by pattern (bit `x + 2 * y` for the
/// eighth at column `x` and row `y`).
const SHARED_OCTANTS: [(u8, char); 26] = [
    (0x00, ' '),
    (0x01, '\u{1CEA8}'),
    (0x02, '\u{1CEAB}'),
    (0x03, '\u{1FB82}'),
    (0x05, '▘'),
    (0x0a, '▝'),
    (0x0f, '▀'),
    (0x14, '\u{1FBE6}'),
    (0x28, '\u{1FBE7}'),
    (0x3f, '\u{1FB85}'),
    (0x40, '\u{1CEA3}'),
    (0x50, '▖'),
    (0x55, '▌'),
    (0x5a, '▞'),
    (0x5f, '▛'),
    (0x80, '\u{1CEA0}'),
    (0xa0, '▗'),
    (0xa5, '▚'),
    (0xaa, '▐'),
    (0xaf, '▜'),
    (0xc0, '▂'),
    (0xf0, '▄'),
    (0xf5, '▙'),
    (0xfa, '▟'),
    (0xfc, '▆'),
    (0xff, '█'),
];

/// The character for a 2×4 pattern of eighths, bit `x + 2 * y` set for the
/// eighth at column `x` and row `y`: the block octants of Unicode 16
/// (U+1CD00 to U+1CDE5, in pattern order), or the older block character
/// that already draws the pattern.
pub(crate) fn octant(bits: u8) -> char {
    match SHARED_OCTANTS.binary_search_by_key(&bits, |(pattern, _)| *pattern) {
        Ok(i) => SHARED_OCTANTS[i].1,
        Err(before) => char::from_u32(0x1CD00 + u32::from(bits) - before as u32).unwrap_or(' '),
    }
}

/// The points on the line from `a` to `b`, both ends included.
pub(crate) fn bresenham(a: (i64, i64), b: (i64, i64)) -> Bresenham {
    let (dx, dy) = ((b.0 - a.0).abs(), -(b.1 - a.1).abs());
    Bresenham {
        at: Some(a),
        end: b,
        dx,
        dy,
        sx: if a.0 < b.0 { 1 } else { -1 },
        sy: if a.1 < b.1 { 1 } else { -1 },
        err: dx + dy,
    }
}

/// The points of a [`bresenham`] line, one at a time, without a buffer.
pub(crate) struct Bresenham {
    at: Option<(i64, i64)>,
    end: (i64, i64),
    dx: i64,
    dy: i64,
    sx: i64,
    sy: i64,
    err: i64,
}

impl Iterator for Bresenham {
    type Item = (i64, i64);

    fn next(&mut self) -> Option<(i64, i64)> {
        let (mut x0, mut y0) = self.at?;
        let point = (x0, y0);
        if point == self.end {
            self.at = None;
            return Some(point);
        }
        let e2 = 2 * self.err;
        if e2 >= self.dy {
            self.err += self.dy;
            x0 += self.sx;
        }
        if e2 <= self.dx {
            self.err += self.dx;
            y0 += self.sy;
        }
        self.at = Some((x0, y0));
        Some(point)
    }
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
        assert_eq!(bresenham((0, 0), (3, 0)).count(), 4);
        assert_eq!(bresenham((0, 0), (0, 0)).collect::<Vec<_>>(), vec![(0, 0)]);
        assert_eq!(
            bresenham((2, 2), (0, 0)).collect::<Vec<_>>(),
            vec![(2, 2), (1, 1), (0, 0)]
        );
        let mut c = DotCanvas::new(2, 1);
        c.line((0, 3), (3, 0), 0);
        assert_eq!(c.row_string(0), "⡠⠊");
    }

    #[test]
    fn pattern_glyphs_follow_unicode() {
        // Quadrants: each corner, the halves, full.
        assert_eq!(quadrant(0b0001), '▘');
        assert_eq!(quadrant(0b1000), '▗');
        assert_eq!(quadrant(0b0101), '▌');
        assert_eq!(quadrant(0b1100), '▄');
        assert_eq!(quadrant(0b1111), '█');
        // Sextants: U+1FB00 is the top left sixth, U+1FB3B the last.
        assert_eq!(sextant(0b00_0001), '\u{1FB00}');
        assert_eq!(sextant(0b01_0100), '\u{1FB13}');
        assert_eq!(sextant(0b01_0110), '\u{1FB14}');
        assert_eq!(sextant(0b11_1110), '\u{1FB3B}');
        assert_eq!(sextant(0b01_0101), '▌');
        assert_eq!(sextant(0b11_1111), '█');
        // Octants: U+1CD00 is the first pattern without an older glyph,
        // U+1CDE5 the last; 230 in all, each once.
        assert_eq!(octant(0b0000_0100), '\u{1CD00}');
        assert_eq!(octant(0b1111_1110), '\u{1CDE5}');
        assert_eq!(octant(0b0000_1111), '▀');
        assert_eq!(octant(0b1100_0000), '▂');
        let glyphs: std::collections::BTreeSet<char> = (0..=255u8).map(octant).collect();
        assert_eq!(glyphs.len(), 256);
        let new = glyphs
            .iter()
            .filter(|c| ('\u{1CD00}'..='\u{1CDE5}').contains(*c))
            .count();
        assert_eq!(new, 230);
        // Every glyph is one cell wide.
        for bits in 0..=255u8 {
            for c in [quadrant(bits), sextant(bits), octant(bits)] {
                assert_eq!(rich::cells::char_cell_width(c), 1, "{c:?}");
            }
        }
    }

    #[test]
    fn zero_sized_canvas_is_harmless() {
        let mut c = DotCanvas::new(0, 0);
        c.set(0, 0, 0);
        c.line((0, 0), (5, 5), 0);
        assert_eq!(c.dot_width(), 0);
    }
}

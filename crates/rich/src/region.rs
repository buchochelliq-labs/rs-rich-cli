//! Port of upstream `rich/region.py`: a rectangular area of the screen.

/// A rectangular region, as `(x, y, width, height)`. Ordered as upstream's
/// `NamedTuple` compares: by `x`, then `y`, `width` and `height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Region {
    /// A region at `(x, y)` of `width` by `height` cells.
    pub fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Region {
            x,
            y,
            width,
            height,
        }
    }
}

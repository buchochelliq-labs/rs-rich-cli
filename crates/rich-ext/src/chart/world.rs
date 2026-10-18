//! The world's coastlines, for [`CanvasPainter::map`](super::CanvasPainter::map).
//!
//! Made with Natural Earth (<https://www.naturalearthdata.com>), whose data
//! is in the public domain. `scripts/world_map.py` makes the two files from
//! Natural Earth's 1:110m and 1:50m coastlines (release 5.1.2): it
//! simplifies each line to a budget of points (Ramer–Douglas–Peucker),
//! drops lines smaller than the tolerance, and rounds what is left to a
//! hundredth of a degree. The files are little-endian `i16` pairs,
//! longitude then latitude in hundredths of a degree, with `(-32768,
//! -32768)` between two lines.

/// How detailed a world map is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MapResolution {
    /// Natural Earth's 1:110m coastlines, simplified to about 1,200 points:
    /// enough for a map up to about 80 cells wide.
    #[default]
    Low,
    /// Natural Earth's 1:50m coastlines, simplified to about 5,000 points,
    /// with more islands.
    High,
}

const LOW: &[u8] = include_bytes!("world_110m.bin");
const HIGH: &[u8] = include_bytes!("world_50m.bin");

/// The pair between two lines.
const BREAK: i16 = i16::MIN;

impl MapResolution {
    fn data(self) -> &'static [u8] {
        match self {
            MapResolution::Low => LOW,
            MapResolution::High => HIGH,
        }
    }

    /// The coastlines, each a run of `(longitude, latitude)` points in
    /// degrees, to be joined by straight lines.
    pub fn lines(self) -> Vec<Vec<(f64, f64)>> {
        let mut lines = Vec::new();
        let mut line = Vec::new();
        for &[x0, x1, y0, y1] in self.data().as_chunks::<4>().0 {
            let (x, y) = (i16::from_le_bytes([x0, x1]), i16::from_le_bytes([y0, y1]));
            if x == BREAK && y == BREAK {
                lines.push(std::mem::take(&mut line));
            } else {
                line.push((f64::from(x) / 100.0, f64::from(y) / 100.0));
            }
        }
        lines.push(line);
        lines.retain(|l| !l.is_empty());
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_coastlines_are_in_range_and_sized_to_their_budget() {
        for (resolution, budget) in [(MapResolution::Low, 1200), (MapResolution::High, 5000)] {
            let lines = resolution.lines();
            let points: usize = lines.iter().map(Vec::len).sum();
            assert!(points > budget * 9 / 10 && points <= budget, "{points}");
            for line in &lines {
                assert!(line.len() >= 2);
                for &(x, y) in line {
                    assert!((-180.0..=180.0).contains(&x) && (-90.0..=90.0).contains(&y));
                }
            }
        }
        // Each file is about 4 bytes a point, so the package stays small.
        assert!(LOW.len() < 8 * 1024 && HIGH.len() < 32 * 1024);
    }
}

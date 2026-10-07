//! Layout: how a container shares its space among its children.
//!
//! A child asks for a [`Size`] along its parent's axis, optionally clamped
//! with [`Node::min_size`](crate::Node::min_size) and
//! [`Node::max_size`](crate::Node::max_size). The solver gives every child
//! its fixed, percentage or content size first, then shares what is left
//! among the flexible children by weight. A flexible child that hits its
//! minimum or maximum keeps it, and the rest is shared again among the
//! others, as CSS flexbox resolves flexible lengths. When the children ask for more than there is, they shrink from
//! the end: down to their minimums first, then past them.
//!
//! [`grid`](crate::grid) places children in rows and columns, each a track
//! solved the same way, with children spanning several tracks.

/// How much of its parent's axis a child takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Size {
    /// Exactly this many cells.
    Fixed(u16),
    /// This percentage of the parent.
    Percent(u16),
    /// A share, by weight, of what the other children leave.
    Flex(u16),
    /// As much as its content needs: the lines a text renders to, a
    /// panel's content and border, a list's rows. Measured again when a
    /// signal the content reads changes.
    Auto,
}

/// One track to solve: a size and its clamps.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Track {
    pub size: Size,
    pub min: u16,
    pub max: u16,
    /// The measured content, for [`Size::Auto`].
    pub content: u16,
}

impl Track {
    pub fn new(size: Size) -> Track {
        Track {
            size,
            min: 0,
            max: u16::MAX,
            content: 0,
        }
    }

    fn clamp(&self, value: u32) -> u32 {
        // A minimum above the maximum wins, as in CSS.
        value.min(self.max as u32).max(self.min as u32)
    }
}

/// The length of each track when they share `total` cells with `gap`
/// cells between neighbours.
pub(crate) fn solve(total: u16, gap: u16, tracks: &[Track]) -> Vec<u16> {
    let n = tracks.len();
    if n == 0 {
        return Vec::new();
    }
    let total = (total as u32).saturating_sub(gap as u32 * (n as u32 - 1));
    let mut sizes: Vec<u32> = tracks
        .iter()
        .map(|t| match t.size {
            Size::Fixed(cells) => t.clamp(cells as u32),
            Size::Percent(p) => t.clamp(total * p.min(100) as u32 / 100),
            Size::Auto => t.clamp(t.content as u32),
            Size::Flex(_) => 0,
        })
        .collect();
    let mut frozen: Vec<bool> = tracks
        .iter()
        .map(|t| !matches!(t.size, Size::Flex(_)))
        .collect();
    // Share what is left by weight. A share outside its clamps is fixed at
    // the clamp, and the rest shared again among the others.
    loop {
        let open: Vec<usize> = (0..n).filter(|&i| !frozen[i]).collect();
        if open.is_empty() {
            break;
        }
        let used: u32 = (0..n).filter(|&i| frozen[i]).map(|i| sizes[i]).sum();
        let free = total.saturating_sub(used);
        let weight = |i: usize| match tracks[i].size {
            Size::Flex(w) => w as u32,
            _ => 0,
        };
        let weights: u32 = open.iter().map(|&i| weight(i)).sum();
        let mut shares = vec![0u32; n];
        let mut left = free;
        // The remainder goes to the last open track with a weight.
        let last = open.iter().rev().copied().find(|&i| weight(i) > 0);
        for &i in &open {
            if weight(i) == 0 {
                continue;
            }
            let share = if Some(i) == last {
                left
            } else {
                free * weight(i) / weights
            };
            left -= share;
            shares[i] = share;
        }
        // As CSS flexbox does: if clamping grows the shares overall, fix
        // only the tracks that hit a minimum; if it shrinks them, only those
        // that hit a maximum; otherwise all of them.
        let fitted: Vec<u32> = (0..n).map(|i| tracks[i].clamp(shares[i])).collect();
        let net: i64 = open
            .iter()
            .map(|&i| fitted[i] as i64 - shares[i] as i64)
            .sum();
        let mut clamped = false;
        for &i in &open {
            let hit = match net.signum() {
                1 => fitted[i] > shares[i],
                -1 => fitted[i] < shares[i],
                _ => fitted[i] != shares[i],
            };
            if hit {
                sizes[i] = fitted[i];
                frozen[i] = true;
                clamped = true;
            }
        }
        if !clamped {
            for &i in &open {
                sizes[i] = shares[i];
            }
            break;
        }
    }
    // Too much asked for: shrink from the end, to the minimums first.
    let mut over = sizes.iter().sum::<u32>().saturating_sub(total);
    for floor in [true, false] {
        for i in (0..n).rev() {
            if over == 0 {
                break;
            }
            let keep = if floor { tracks[i].min as u32 } else { 0 };
            let cut = sizes[i].saturating_sub(keep).min(over);
            sizes[i] -= cut;
            over -= cut;
        }
    }
    sizes.into_iter().map(|s| s as u16).collect()
}

/// Grow the content ([`Size::Auto`]) tracks among `tracks[from..from + n]`
/// so that, with the gaps between them, they hold `need` cells: what a
/// child spanning those tracks needs. The shortfall is shared evenly among
/// the content tracks it spans (the remainder to the last); a span with
/// none has its size fixed by the others.
pub(crate) fn grow_for_span(
    tracks: &mut [Track],
    total: u16,
    gap: u16,
    from: usize,
    n: usize,
    need: u16,
) {
    let span = from..(from + n).min(tracks.len());
    let held: u32 = tracks[span.clone()]
        .iter()
        .map(|t| match t.size {
            Size::Auto => t.content as u32,
            Size::Fixed(cells) => cells as u32,
            Size::Percent(p) => total as u32 * p.min(100) as u32 / 100,
            Size::Flex(_) => 0,
        })
        .sum::<u32>()
        + gap as u32 * (span.len().saturating_sub(1)) as u32;
    let short = (need as u32).saturating_sub(held);
    let content: Vec<usize> = span.filter(|&i| tracks[i].size == Size::Auto).collect();
    if short == 0 || content.is_empty() {
        return;
    }
    let each = short / content.len() as u32;
    let last = *content.last().expect("not empty");
    for &i in &content {
        let extra = if i == last {
            short - each * (content.len() as u32 - 1)
        } else {
            each
        };
        tracks[i].content = (tracks[i].content as u32 + extra).min(u16::MAX as u32) as u16;
    }
}

/// Where each track starts, from `start`, with `gap` between them.
pub(crate) fn offsets(start: u16, gap: u16, sizes: &[u16]) -> Vec<u16> {
    let mut at = start;
    sizes
        .iter()
        .map(|&size| {
            let here = at;
            at = at.saturating_add(size).saturating_add(gap);
            here
        })
        .collect()
}

/// A child's place in a grid: its first column and row, and how many of
/// each it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cellspan {
    pub column: usize,
    pub row: usize,
    pub columns: usize,
    pub rows: usize,
}

/// Place children with `spans` (columns, rows) in a grid `columns` wide,
/// row by row, each in the first free place at or after the previous
/// child's. Returns the places and the number of rows used.
pub(crate) fn place(columns: usize, spans: &[(u16, u16)]) -> (Vec<Cellspan>, usize) {
    let columns = columns.max(1);
    let mut taken: Vec<Vec<bool>> = Vec::new();
    let (mut row, mut column) = (0usize, 0usize);
    let mut places = Vec::with_capacity(spans.len());
    for &(wide, tall) in spans {
        let wide = (wide.max(1) as usize).min(columns);
        let tall = tall.max(1) as usize;
        loop {
            if column + wide > columns {
                row += 1;
                column = 0;
                continue;
            }
            let fits = (row..row + tall).all(|r| {
                (column..column + wide).all(|c| !taken.get(r).is_some_and(|cells| cells[c]))
            });
            if fits {
                break;
            }
            column += 1;
        }
        while taken.len() < row + tall {
            taken.push(vec![false; columns]);
        }
        for cells in &mut taken[row..row + tall] {
            for cell in &mut cells[column..column + wide] {
                *cell = true;
            }
        }
        places.push(Cellspan {
            column,
            row,
            columns: wide,
            rows: tall,
        });
        column += wide;
    }
    (places, taken.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(size: Size) -> Track {
        Track::new(size)
    }

    #[test]
    fn flex_shares_what_fixed_and_percent_leave() {
        let tracks = [
            track(Size::Fixed(10)),
            track(Size::Percent(50)),
            track(Size::Flex(1)),
            track(Size::Flex(3)),
        ];
        assert_eq!(solve(100, 0, &tracks), vec![10, 50, 10, 30]);
    }

    #[test]
    fn gaps_come_out_of_the_total() {
        let tracks = [track(Size::Flex(1)), track(Size::Flex(1))];
        assert_eq!(solve(21, 1, &tracks), vec![10, 10]);
    }

    #[test]
    fn a_clamped_flex_track_gives_its_share_to_the_others() {
        let mut capped = track(Size::Flex(1));
        capped.max = 5;
        let tracks = [capped, track(Size::Flex(1))];
        assert_eq!(solve(30, 0, &tracks), vec![5, 25]);
        let mut floor = track(Size::Flex(1));
        floor.min = 20;
        let tracks = [floor, track(Size::Flex(1))];
        assert_eq!(solve(30, 0, &tracks), vec![20, 10]);
    }

    #[test]
    fn overflow_shrinks_from_the_end_to_the_minimums_first() {
        let mut first = track(Size::Fixed(10));
        first.min = 4;
        let mut second = track(Size::Fixed(10));
        second.min = 8;
        assert_eq!(solve(16, 0, &[first, second]), vec![8, 8]);
        assert_eq!(solve(10, 0, &[first, second]), vec![4, 6]);
        assert_eq!(solve(4, 0, &[first, second]), vec![4, 0]);
    }

    #[test]
    fn auto_takes_its_content() {
        let mut auto = track(Size::Auto);
        auto.content = 3;
        assert_eq!(solve(10, 0, &[auto, track(Size::Flex(1))]), vec![3, 7]);
    }

    #[test]
    fn a_span_grows_the_content_tracks_it_covers() {
        let mut tracks = [track(Size::Auto), track(Size::Fixed(2)), track(Size::Auto)];
        tracks[0].content = 1;
        // Holds 1 + 2 + 0 + two gaps = 5; needs 10: 5 more, shared 2 and 3.
        grow_for_span(&mut tracks, 20, 1, 0, 3, 10);
        assert_eq!((tracks[0].content, tracks[2].content), (3, 3));
        // Already enough: nothing changes.
        grow_for_span(&mut tracks, 20, 1, 0, 2, 4);
        assert_eq!(tracks[0].content, 3);
    }

    #[test]
    fn places_flow_row_by_row_around_spans() {
        let (places, rows) = place(3, &[(2, 1), (1, 2), (1, 1), (1, 1), (3, 1)]);
        let at: Vec<(usize, usize)> = places.iter().map(|p| (p.column, p.row)).collect();
        assert_eq!(at, vec![(0, 0), (2, 0), (0, 1), (1, 1), (0, 2)]);
        assert_eq!(rows, 3);
    }
}

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
//!
//! The solver is also a function of its own: [`split`] lays [`Track`]s out
//! in a rectangle, as a column or row does its children, and
//! [`split_with_spacers`] gives the room between them too, for a widget
//! that places its own parts (a pop-up beside a cell, a gauge's ticks).

use crate::node::Axis;
use crate::screen::Rect;

/// How much of its parent's axis a child takes.
///
/// More kinds may come: match with a wildcard arm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    /// `a / b` of the parent, as a percentage is: ratatui's
    /// `Constraint::Ratio`. Of the room after gaps, rounded down; the cells
    /// that rounding leaves go to the last ratio of the parent, so three
    /// `Ratio(1, 3)` fill it exactly. A zero `b` is no room at all, and a
    /// ratio over one is the whole parent.
    Ratio(u32, u32),
}

/// `total × a / b` cells, rounded down: none when `b` is 0, at most
/// `total`.
pub(crate) fn ratio_of(total: u32, a: u32, b: u32) -> u32 {
    if b == 0 {
        return 0;
    }
    (u64::from(total) * u64::from(a) / u64::from(b)).min(u64::from(total)) as u32
}

/// The cells a group of ratios takes of `total` together, rounded down
/// once: `total × Σ a / b`, worked out exactly (each ratio at most one).
fn ratios_of(total: u32, ratios: &[(u32, u32)]) -> u32 {
    fn gcd(mut a: u128, mut b: u128) -> u128 {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a.max(1)
    }
    let (mut num, mut den) = (0u128, 1u128);
    for &(a, b) in ratios.iter().filter(|(_, b)| *b != 0) {
        let (a, b) = (u128::from(a.min(b)), u128::from(b));
        let sum = num
            .checked_mul(b)
            .and_then(|n| a.checked_mul(den).and_then(|m| n.checked_add(m)));
        match (sum, den.checked_mul(b)) {
            (Some(n), Some(d)) => {
                let g = gcd(n, d);
                (num, den) = (n / g, d / g);
            }
            // Denominators too large to add exactly (many large primes):
            // near enough.
            _ => {
                let exact: f64 = ratios
                    .iter()
                    .filter(|(_, b)| *b != 0)
                    .map(|&(a, b)| f64::from(a.min(b)) / f64::from(b))
                    .sum();
                return (f64::from(total) * exact).floor().min(f64::from(total)) as u32;
            }
        }
    }
    (u128::from(total) * num / den).min(u128::from(total)) as u32
}

/// How the lines of a text sit across its width: CSS's `text-align`, and
/// ratatui's `Alignment` on a paragraph.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAlign {
    /// At the left: the default.
    #[default]
    Left,
    /// Each line centred.
    Center,
    /// Each line at the right.
    Right,
    /// Each line but the last spread to the whole width, the words spaced
    /// out evenly.
    Justify,
}

/// Where a child sits across its parent's axis: across a
/// [`column`](crate::column)'s width, or a [`row`](crate::row)'s height.
/// CSS's `align-items` and `align-self`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    /// At the start (left in a column, top in a row), as wide or tall as
    /// its content.
    Start,
    /// Centred, as wide or tall as its content.
    Center,
    /// At the end, as wide or tall as its content.
    End,
    /// The whole width or height: the default.
    #[default]
    Stretch,
}

/// Where children sit along their parent's axis when they leave room over:
/// CSS's `justify-content`. A container with a flexible child has none left
/// over (the child takes it), as in CSS.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Justify {
    /// Packed at the start: the default.
    #[default]
    Start,
    /// Packed in the middle.
    Center,
    /// Packed at the end.
    End,
    /// The first at the start, the last at the end, the room shared
    /// evenly between them.
    SpaceBetween,
    /// The room shared evenly around each child: half as much at the ends
    /// as between two children.
    SpaceAround,
    /// The room shared evenly, the ends included.
    SpaceEvenly,
}

/// Where the lines of a [wrapping](crate::Node::wrap) row or column sit
/// across its axis when they leave room over: CSS's `align-content`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignContent {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    /// The lines grow to share the room: the default.
    #[default]
    Stretch,
}

/// How a [`column`](crate::column) or [`row`](crate::row) places its
/// children: the gap between them, alignment across and along its axis,
/// and wrapping. [`MeasureCx::stack_flow`](crate::widget::MeasureCx::stack_flow)
/// takes one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackLayout {
    pub gap: u16,
    pub align_items: Align,
    pub justify: Justify,
    pub wrap: bool,
    pub align_content: AlignContent,
}

/// One track to lay out with [`split`]: a [`Size`] and its clamps, as a
/// child of a column or row has them.
///
/// ```
/// use intuituive::layout::Track;
/// use intuituive::Size;
///
/// let sidebar = Track::new(Size::Percent(25)).min(20).max(40);
/// let status = Track::new(Size::Auto).content(3); // three rows of content
/// # let _ = (sidebar, status);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Track {
    pub(crate) size: Size,
    pub(crate) min: u16,
    pub(crate) max: u16,
    /// The measured content, for [`Size::Auto`].
    pub(crate) content: u16,
}

impl Track {
    /// A track of `size`, unclamped, with no content.
    pub fn new(size: Size) -> Track {
        Track {
            size,
            min: 0,
            max: u16::MAX,
            content: 0,
        }
    }

    /// Never less than `cells`, while there is room (CSS's `min-width`).
    pub fn min(mut self, cells: u16) -> Track {
        self.min = cells;
        self
    }

    /// Never more than `cells`.
    pub fn max(mut self, cells: u16) -> Track {
        self.max = cells;
        self
    }

    /// For [`Size::Auto`]: the cells its content needs.
    pub fn content(mut self, cells: u16) -> Track {
        self.content = cells;
        self
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
    // Ratios are rounded down each, and what that loses of the group's
    // exact total goes to the last of them, as a flexible share's
    // remainder does.
    let ratios: Vec<(u32, u32)> = tracks
        .iter()
        .filter_map(|t| match t.size {
            Size::Ratio(a, b) => Some((a, b)),
            _ => None,
        })
        .collect();
    let mut ratio_rest = ratios_of(total, &ratios).saturating_sub(
        ratios
            .iter()
            .map(|&(a, b)| ratio_of(total, a, b))
            .sum::<u32>(),
    );
    let last_ratio = tracks
        .iter()
        .rposition(|t| matches!(t.size, Size::Ratio(..)));
    let mut sizes: Vec<u32> = tracks
        .iter()
        .enumerate()
        .map(|(i, t)| match t.size {
            Size::Fixed(cells) => t.clamp(cells as u32),
            Size::Percent(p) => t.clamp(total * p.min(100) as u32 / 100),
            Size::Ratio(a, b) => {
                let rest = if Some(i) == last_ratio {
                    std::mem::take(&mut ratio_rest)
                } else {
                    0
                };
                t.clamp(ratio_of(total, a, b) + rest)
            }
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
            Size::Ratio(a, b) => ratio_of(total as u32, a, b),
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

/// Room shared out by `justify` before item `i` of `k`, when `free` cells
/// are left over. Each share is computed from the total, so rounding never
/// drifts: the last item of [`Justify::End`] or [`Justify::SpaceBetween`]
/// ends exactly at the end.
fn spread(justify: Justify, free: u32, i: u32, k: u32) -> u32 {
    // In u64: tens of thousands of items times the free room overflow u32.
    let (free, i, k) = (free as u64, i as u64, k as u64);
    let share = match justify {
        Justify::Start => 0,
        Justify::Center => free / 2,
        Justify::End => free,
        Justify::SpaceBetween if k > 1 => free * i / (k - 1),
        Justify::SpaceBetween => 0,
        Justify::SpaceAround => free * (2 * i + 1) / (2 * k),
        Justify::SpaceEvenly => free * (i + 1) / (k + 1),
    };
    share as u32
}

/// Where each of `sizes` starts along `total` cells from `start`, `gap`
/// apart, with the room they leave placed by `justify`.
pub(crate) fn justify_offsets(
    start: u16,
    total: u16,
    gap: u16,
    sizes: &[u16],
    justify: Justify,
) -> Vec<u16> {
    let k = sizes.len() as u32;
    let used: u64 =
        sizes.iter().map(|&n| n as u64).sum::<u64>() + gap as u64 * k.saturating_sub(1) as u64;
    let free = (total as u64).saturating_sub(used) as u32;
    let mut at = start as u64;
    sizes
        .iter()
        .enumerate()
        .map(|(i, &size)| {
            let here = at + spread(justify, free, i as u32, k) as u64;
            at += size as u64 + gap as u64;
            here.min(u16::MAX as u64) as u16
        })
        .collect()
}

/// Where an item `content` cells across sits in a line of `line` cells at
/// `at`, and how many cells it takes: all of them for [`Align::Stretch`].
pub(crate) fn align_cross(at: u16, line: u16, content: u16, align: Align) -> (u16, u16) {
    let size = content.min(line);
    let free = line - size;
    match align {
        Align::Stretch => (at, line),
        Align::Start => (at, size),
        Align::Center => (at.saturating_add(free / 2), size),
        Align::End => (at.saturating_add(free), size),
    }
}

/// Break items whose main sizes are `bases` into lines of at most `total`
/// cells with `gap` between items, as CSS wraps flex items: each line takes
/// items while they fit, and always at least one.
pub(crate) fn break_lines(total: u16, gap: u16, bases: &[u16]) -> Vec<std::ops::Range<usize>> {
    let mut lines = Vec::new();
    let mut first = 0;
    let mut used = 0u32;
    for (i, &base) in bases.iter().enumerate() {
        let need = if i == first {
            base as u32
        } else {
            used + gap as u32 + base as u32
        };
        if i > first && need > total as u32 {
            lines.push(first..i);
            first = i;
            used = base as u32;
        } else {
            used = need;
        }
    }
    if first < bases.len() {
        lines.push(first..bases.len());
    }
    lines
}

/// Where each line of a wrapped container starts across its axis, and how
/// many cells it takes, given each line's own size (its tallest or widest
/// item) and `total` cells from `start` with `gap` between lines.
pub(crate) fn place_lines(
    start: u16,
    total: u16,
    gap: u16,
    lines: &[u16],
    align: AlignContent,
) -> Vec<(u16, u16)> {
    let k = lines.len() as u32;
    let used: u64 =
        lines.iter().map(|&n| n as u64).sum::<u64>() + gap as u64 * k.saturating_sub(1) as u64;
    let free = (total as u64).saturating_sub(used) as u32;
    let justify = match align {
        AlignContent::Start | AlignContent::Stretch => Justify::Start,
        AlignContent::Center => Justify::Center,
        AlignContent::End => Justify::End,
        AlignContent::SpaceBetween => Justify::SpaceBetween,
        AlignContent::SpaceAround => Justify::SpaceAround,
        AlignContent::SpaceEvenly => Justify::SpaceEvenly,
    };
    let mut at = start as u64;
    lines
        .iter()
        .enumerate()
        .map(|(i, &size)| {
            let i = i as u32;
            // Stretch: line `i` grows by its share of the room, so the
            // lines fill it exactly.
            let grow = if align == AlignContent::Stretch {
                let (free, i, k) = (free as u64, i as u64, k as u64);
                (free * (i + 1) / k - free * i / k) as u32
            } else {
                0
            };
            let here = at + spread(justify, free, i, k) as u64;
            let size = size as u32 + grow;
            at += size as u64 + gap as u64;
            (
                here.min(u16::MAX as u64) as u16,
                size.min(u16::MAX as u32) as u16,
            )
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

/// Lay `tracks` out along `axis` in `rect`, as a [`column`](crate::column)
/// or [`row`](crate::row) lays out its children: each by its [`Size`] within
/// its clamps, the flexible ones sharing what is left, `flow.gap` cells
/// apart, and what they leave over placed by `flow.justify`. Each takes the
/// whole of `rect` across the axis (tracks have no content across it, so
/// `flow.align_items` and wrapping do not apply). ratatui's `Layout::split`.
///
/// ```
/// use intuituive::layout::{split, Track};
/// use intuituive::node::Axis;
/// use intuituive::screen::Rect;
/// use intuituive::{Size, StackLayout};
///
/// let tracks = [Track::new(Size::Fixed(3)), Track::new(Size::Flex(1))];
/// let areas = split(Rect::new(0, 0, 20, 10), Axis::Vertical, &tracks, StackLayout::default());
/// assert_eq!(areas, [Rect::new(0, 0, 20, 3), Rect::new(0, 3, 20, 7)]);
/// ```
pub fn split(rect: Rect, axis: Axis, tracks: &[Track], flow: StackLayout) -> Vec<Rect> {
    split_with_spacers(rect, axis, tracks, flow).0
}

/// [`split`], and the room around the tracks: one spacer before the first,
/// one between each two, and one after the last (`tracks.len() + 1` in all,
/// some of them empty), as ratatui's `Layout::split_with_spacers`. With
/// [`Justify::SpaceBetween`] the spacers between are the gaps and the ends
/// are empty; with [`Justify::Center`] the ends hold the room left over.
///
/// ```
/// use intuituive::layout::{split_with_spacers, Track};
/// use intuituive::node::Axis;
/// use intuituive::screen::Rect;
/// use intuituive::{Justify, Size, StackLayout};
///
/// let flow = StackLayout { justify: Justify::Center, ..StackLayout::default() };
/// let tracks = [Track::new(Size::Fixed(4)), Track::new(Size::Fixed(4))];
/// let (areas, spacers) = split_with_spacers(Rect::new(0, 0, 20, 1), Axis::Horizontal, &tracks, flow);
/// assert_eq!(areas, [Rect::new(6, 0, 4, 1), Rect::new(10, 0, 4, 1)]);
/// assert_eq!(spacers[0], Rect::new(0, 0, 6, 1));
/// assert_eq!(spacers[2], Rect::new(14, 0, 6, 1));
/// ```
pub fn split_with_spacers(
    rect: Rect,
    axis: Axis,
    tracks: &[Track],
    flow: StackLayout,
) -> (Vec<Rect>, Vec<Rect>) {
    let (start, total, across, breadth) = match axis {
        Axis::Vertical => (rect.y, rect.height, rect.x, rect.width),
        Axis::Horizontal => (rect.x, rect.width, rect.y, rect.height),
    };
    let piece = |at: u16, length: u16| match axis {
        Axis::Vertical => Rect::new(across, at, breadth, length),
        Axis::Horizontal => Rect::new(at, across, length, breadth),
    };
    let sizes = solve(total, flow.gap, tracks);
    let at = justify_offsets(start, total, flow.gap, &sizes, flow.justify);
    let end = start.saturating_add(total);
    let areas: Vec<Rect> = at
        .iter()
        .zip(&sizes)
        .map(|(&at, &size)| {
            let at = at.min(end);
            piece(at, size.min(end - at))
        })
        .collect();
    // Between the end of each area (or the start) and the start of the
    // next (or the end).
    let mut spacers = Vec::with_capacity(areas.len() + 1);
    let mut from = start;
    for (&at, &size) in at.iter().zip(&sizes) {
        let to = at.clamp(from, end);
        spacers.push(piece(from, to - from));
        from = at.saturating_add(size).clamp(to, end);
    }
    spacers.push(piece(from, end - from));
    (areas, spacers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(size: Size) -> Track {
        Track::new(size)
    }

    #[test]
    fn three_thirds_fill_the_room_exactly() {
        let third = track(Size::Ratio(1, 3));
        assert_eq!(solve(100, 0, &[third, third, third]), vec![33, 33, 34]);
        // Of the room after gaps, as a percentage is: 98 cells.
        assert_eq!(solve(100, 1, &[third, third, third]), vec![32, 32, 34]);
        // A zero denominator is no room; over one is the whole parent.
        assert_eq!(solve(10, 0, &[track(Size::Ratio(1, 0))]), vec![0]);
        assert_eq!(solve(10, 0, &[track(Size::Ratio(3, 2))]), vec![10]);
    }

    #[test]
    fn ratios_mix_with_fixed_and_flexible_tracks() {
        let tracks = [
            track(Size::Fixed(10)),
            track(Size::Ratio(1, 4)),
            track(Size::Flex(1)),
            track(Size::Ratio(1, 4)),
        ];
        // 25 each of 100, and the flexible track takes what is left.
        assert_eq!(solve(100, 0, &tracks), vec![10, 25, 40, 25]);
        // 1/3 and 1/6 of 50: 16 and 8, and the half-cell lost to rounding
        // goes to the last ratio (25 together).
        let tracks = [track(Size::Ratio(1, 3)), track(Size::Ratio(1, 6))];
        assert_eq!(solve(50, 0, &tracks), vec![16, 9]);
        let mut capped = track(Size::Ratio(1, 2));
        capped.max = 10;
        assert_eq!(solve(40, 0, &[capped, track(Size::Flex(1))]), vec![10, 30]);
    }

    #[test]
    fn spacers_are_the_room_between_and_around() {
        let rect = Rect::new(0, 0, 20, 1);
        let tracks = [track(Size::Fixed(4)), track(Size::Fixed(6))];
        let flow = |justify| StackLayout {
            justify,
            ..StackLayout::default()
        };
        let (areas, spacers) =
            split_with_spacers(rect, Axis::Horizontal, &tracks, flow(Justify::SpaceBetween));
        assert_eq!(areas, [Rect::new(0, 0, 4, 1), Rect::new(14, 0, 6, 1)]);
        // The ends are empty, and the gap is the room between.
        assert_eq!(
            spacers,
            [
                Rect::new(0, 0, 0, 1),
                Rect::new(4, 0, 10, 1),
                Rect::new(20, 0, 0, 1)
            ]
        );
        let (areas, spacers) =
            split_with_spacers(rect, Axis::Horizontal, &tracks, flow(Justify::Center));
        assert_eq!(areas, [Rect::new(5, 0, 4, 1), Rect::new(9, 0, 6, 1)]);
        assert_eq!(
            spacers,
            [
                Rect::new(0, 0, 5, 1),
                Rect::new(9, 0, 0, 1),
                Rect::new(15, 0, 5, 1)
            ]
        );
        // A gap is a spacer too, and down the axis the spacers are rows.
        let flow = StackLayout {
            gap: 1,
            ..StackLayout::default()
        };
        let (areas, spacers) = split_with_spacers(
            Rect::new(2, 1, 5, 10),
            Axis::Vertical,
            &[track(Size::Fixed(2)), track(Size::Flex(1))],
            flow,
        );
        assert_eq!(areas, [Rect::new(2, 1, 5, 2), Rect::new(2, 4, 5, 7)]);
        assert_eq!(spacers[1], Rect::new(2, 3, 5, 1));
        assert_eq!(spacers[2], Rect::new(2, 11, 5, 0));
        // No tracks: one spacer, the whole rectangle.
        assert_eq!(
            split_with_spacers(rect, Axis::Horizontal, &[], flow).1,
            [rect]
        );
    }

    #[test]
    fn a_grid_of_ratio_tracks_shares_its_width() {
        let third = track(Size::Ratio(1, 3));
        // Three columns, a gap of 2 between: 56 of 60, 18, 18 and 20.
        assert_eq!(solve(60, 2, &[third, third, third]), vec![18, 18, 20]);
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

    // The expected positions are those CSS flexbox computes for the same
    // boxes, rounded down to whole cells: a 20-cell container, items 4 and 6
    // cells wide.

    #[test]
    fn justify_places_what_the_items_leave() {
        let at = |j| justify_offsets(0, 20, 0, &[4, 6], j);
        assert_eq!(at(Justify::Start), [0, 4]);
        assert_eq!(at(Justify::Center), [5, 9]);
        assert_eq!(at(Justify::End), [10, 14]);
        assert_eq!(at(Justify::SpaceBetween), [0, 14]);
        // 10 free: 2.5 at each end, 5 between (CSS: 2.5 and 11.5).
        assert_eq!(at(Justify::SpaceAround), [2, 11]);
        // 10 free: 3.33 at each end and between (CSS: 3.33 and 10.67).
        assert_eq!(at(Justify::SpaceEvenly), [3, 10]);
    }

    #[test]
    fn justify_counts_gaps_and_never_overflows() {
        assert_eq!(
            justify_offsets(2, 20, 1, &[4, 6], Justify::End),
            [11, 16],
            "ends at 2 + 20"
        );
        // No room left: every justify packs at the start.
        for j in [Justify::Center, Justify::End, Justify::SpaceEvenly] {
            assert_eq!(justify_offsets(0, 10, 0, &[4, 6], j), [0, 4]);
        }
        // One item between: CSS puts it at the start.
        assert_eq!(justify_offsets(0, 10, 0, &[4], Justify::SpaceBetween), [0]);
    }

    #[test]
    fn items_align_across_the_line() {
        assert_eq!(align_cross(1, 10, 4, Align::Start), (1, 4));
        assert_eq!(align_cross(1, 10, 4, Align::Center), (4, 4));
        assert_eq!(align_cross(1, 10, 4, Align::End), (7, 4));
        assert_eq!(align_cross(1, 10, 4, Align::Stretch), (1, 10));
        // Wider than the line: cut to it.
        assert_eq!(align_cross(0, 3, 8, Align::Center), (0, 3));
    }

    #[test]
    fn lines_break_where_the_next_item_does_not_fit() {
        assert_eq!(
            break_lines(10, 1, &[4, 4, 4, 9, 11]),
            [0..2, 2..3, 3..4, 4..5]
        );
        // Exactly full: 4 + 1 + 5 = 10.
        assert_eq!(break_lines(10, 1, &[4, 5, 1]), [0..2, 2..3]);
        assert!(break_lines(10, 1, &[]).is_empty());
    }

    #[test]
    fn align_content_places_the_lines() {
        // Two lines 2 and 3 tall in 10 cells, gap 1: 4 left over.
        let lines = |a| place_lines(0, 10, 1, &[2, 3], a);
        assert_eq!(lines(AlignContent::Start), [(0, 2), (3, 3)]);
        assert_eq!(lines(AlignContent::End), [(4, 2), (7, 3)]);
        assert_eq!(lines(AlignContent::Center), [(2, 2), (5, 3)]);
        assert_eq!(lines(AlignContent::SpaceBetween), [(0, 2), (7, 3)]);
        // Stretch: each line grows by 2 and they fill the 10 cells.
        assert_eq!(lines(AlignContent::Stretch), [(0, 4), (5, 5)]);
    }

    #[test]
    fn places_flow_row_by_row_around_spans() {
        let (places, rows) = place(3, &[(2, 1), (1, 2), (1, 1), (1, 1), (3, 1)]);
        let at: Vec<(usize, usize)> = places.iter().map(|p| (p.column, p.row)).collect();
        assert_eq!(at, vec![(0, 0), (2, 0), (0, 1), (1, 1), (0, 2)]);
        assert_eq!(rows, 3);
    }
}

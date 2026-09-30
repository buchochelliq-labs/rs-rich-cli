//! The graphics side channel: images placed on a frame's cells.
//!
//! A [`Frame`] holds text; control segments are dropped. Terminal graphics
//! (Kitty, iTerm2 and Sixel images) travel beside the cells instead, as
//! [`Placement`]s: a row, a column, a size in cells, the [`Graphic`] and
//! which of its frames shows. Whatever paints the frame writes the cell
//! diff first, then works out from [`plan`] what graphics to redraw:
//!
//! - an overlay (iTerm2, Sixel) is redrawn when it is new, moved, on another
//!   animation frame, or when a cell under it was repainted (which wiped
//!   it);
//! - an overlay that went away has its cells repainted, which wipes it;
//! - a graphic that lives in its cells (Kitty's Unicode placeholders) goes
//!   and comes with those cells, so it is only ever *drawn* to make sure the
//!   terminal holds the image.
//!
//! The cells under a placement carry the layout (a micro asset's
//! placeholder cells), so the cell diff stays exact whatever the graphics
//! do, and a painter without graphics shows those cells and nothing leaks.
//!
//! A [`PlacementSource`] finds the placements in a frame's cells (for micro
//! assets, the cells tagged `rich.micro`), may swap cells for others of the
//! same width before the frame is built ([`PlacementSource::prepare`]), and
//! releases what the terminal holds when the view closes.

use std::fmt;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use rich::Segment;

use super::{Change, Frame};

/// Something a terminal graphics protocol draws on a frame's cells.
pub trait Graphic: Send + Sync + fmt::Debug {
    /// What is drawn: two graphics with one key draw the same pixels.
    fn key(&self) -> u64;

    /// The escape that draws animation frame `frame` with the cursor on the
    /// placement's top-left cell. It must leave the cursor where it found it
    /// (save and restore it around anything that moves it).
    fn draw(&self, frame: usize) -> String;

    /// Whether the image lives in the cells themselves (Kitty's Unicode
    /// placeholders): repainting the cells repaints it, and [`draw`](Self::draw)
    /// only makes sure the terminal holds the image.
    fn in_cells(&self) -> bool {
        false
    }
}

/// A graphic placed on a frame: its top-left cell, its size in cells, and
/// the animation frame that shows.
#[derive(Clone, Debug)]
pub struct Placement {
    pub row: usize,
    pub column: usize,
    pub cols: usize,
    pub rows: usize,
    pub graphic: Arc<dyn Graphic>,
    pub frame: usize,
}

impl Placement {
    /// Whether `self` and `other` show the same thing in the same place.
    pub fn same(&self, other: &Placement) -> bool {
        self.row == other.row
            && self.column == other.column
            && self.cols == other.cols
            && self.rows == other.rows
            && self.frame == other.frame
            && self.graphic.key() == other.graphic.key()
    }

    /// The columns it covers.
    pub fn columns(&self) -> Range<usize> {
        self.column..self.column + self.cols
    }

    /// Whether `change` repainted any of its cells.
    pub fn touched_by(&self, change: &Change) -> bool {
        (self.row..self.row + self.rows).contains(&change.row)
            && change.columns.start < self.column + self.cols
            && self.column < change.columns.end
    }
}

/// Finds the graphics placed on a frame, and speaks for them to painters.
pub trait PlacementSource: Send + Sync {
    /// Swap cells of one painted line for others of the same width before
    /// the frame is built (for example, a micro asset's fallback cells for
    /// Kitty placeholder cells). The default keeps the line.
    fn prepare(&self, line: Vec<Segment>) -> Vec<Segment> {
        line
    }

    /// The placements on `frame`'s cells.
    fn placements(&self, frame: &Frame) -> Vec<Placement>;

    /// How long until an animation among the placements shows another frame,
    /// so an event loop knows when to repaint. `None` when nothing moves.
    fn next_change(&self) -> Option<Duration> {
        None
    }

    /// The escape that releases what the terminal holds for graphics this
    /// source placed, once the view closes, except those in `retained` (still
    /// on screen). Kitty images are deleted by id here.
    fn release(&self, retained: &[Placement]) -> String {
        let _ = retained;
        String::new()
    }
}

/// What a painter writes for graphics after the cell diff.
#[derive(Clone, Debug, Default)]
pub struct GraphicsPlan {
    /// Cells to repaint because the overlay drawn on them went away:
    /// `(row, columns)`.
    pub repaint: Vec<(usize, Range<usize>)>,
    /// Placements to draw, in order.
    pub draw: Vec<Placement>,
}

impl GraphicsPlan {
    pub fn is_empty(&self) -> bool {
        self.repaint.is_empty() && self.draw.is_empty()
    }
}

/// What to redraw going from `previous` placements to `next`, after the
/// cell `changes` were painted.
pub fn plan(previous: &[Placement], next: &[Placement], changes: &[Change]) -> GraphicsPlan {
    let mut out = GraphicsPlan::default();
    for old in previous {
        if old.graphic.in_cells() || next.iter().any(|new| new.same(old)) {
            continue;
        }
        // Repaint what the cell diff did not already.
        for row in old.row..old.row + old.rows {
            let mut open = None;
            for column in old.columns() {
                let painted = changes
                    .iter()
                    .any(|change| change.row == row && change.columns.contains(&column));
                match (painted, open) {
                    (false, None) => open = Some(column),
                    (true, Some(start)) => {
                        out.repaint.push((row, start..column));
                        open = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = open {
                out.repaint.push((row, start..old.column + old.cols));
            }
        }
    }
    for new in next {
        let kept = previous.iter().any(|old| old.same(new));
        let wiped = !new.graphic.in_cells()
            && (changes.iter().any(|change| new.touched_by(change))
                || out.repaint.iter().any(|(row, columns)| {
                    (new.row..new.row + new.rows).contains(row)
                        && columns.start < new.column + new.cols
                        && new.column < columns.end
                }));
        if !kept || wiped {
            out.draw.push(new.clone());
        }
    }
    out
}

impl Frame {
    /// This frame with `placements` beside its cells.
    pub fn with_placements(mut self, placements: Vec<Placement>) -> Frame {
        self.placements = placements;
        self
    }

    /// Replace the placements beside this frame's cells.
    pub fn set_placements(&mut self, placements: Vec<Placement>) {
        self.placements = placements;
    }

    /// The graphics placed on this frame's cells.
    pub fn placements(&self) -> &[Placement] {
        &self.placements
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Image(u64, bool);

    impl Graphic for Image {
        fn key(&self) -> u64 {
            self.0
        }
        fn draw(&self, frame: usize) -> String {
            format!("<img{}:{frame}>", self.0)
        }
        fn in_cells(&self) -> bool {
            self.1
        }
    }

    fn at(row: usize, column: usize, key: u64, frame: usize) -> Placement {
        Placement {
            row,
            column,
            cols: 2,
            rows: 1,
            graphic: Arc::new(Image(key, false)),
            frame,
        }
    }

    fn change(row: usize, columns: Range<usize>) -> Change {
        Change { row, columns }
    }

    #[test]
    fn unchanged_placements_draw_nothing() {
        let placed = [at(0, 4, 1, 0)];
        assert!(plan(&placed, &placed, &[]).is_empty());
        // A change elsewhere leaves it.
        assert!(plan(&placed, &placed, &[change(0, 0..2)]).is_empty());
    }

    #[test]
    fn new_moved_animated_and_wiped_placements_are_drawn() {
        let old = [at(0, 4, 1, 0)];
        assert_eq!(plan(&[], &old, &[]).draw.len(), 1);
        let moved = plan(&old, &[at(0, 6, 1, 0)], &[]);
        assert_eq!(moved.draw.len(), 1);
        // Its old cells 4..6 are repainted; 6..8 are where it is now.
        assert_eq!(moved.repaint, vec![(0, 4..6)]);
        assert_eq!(plan(&old, &[at(0, 4, 1, 1)], &[]).draw.len(), 1);
        let wiped = plan(&old, &old, &[change(0, 5..9)]);
        assert_eq!(wiped.draw.len(), 1);
        assert!(wiped.repaint.is_empty());
    }

    #[test]
    fn a_removed_overlay_repaints_only_cells_the_diff_did_not() {
        let old = [at(1, 4, 1, 0)];
        let out = plan(&old, &[], &[change(1, 3..5)]);
        assert_eq!(out.repaint, vec![(1, 5..6)]);
        assert!(out.draw.is_empty());
    }

    #[test]
    fn graphics_in_cells_are_drawn_once_and_never_repainted() {
        let kitty = Placement {
            graphic: Arc::new(Image(9, true)),
            ..at(0, 0, 9, 0)
        };
        assert_eq!(plan(&[], std::slice::from_ref(&kitty), &[]).draw.len(), 1);
        let again = plan(
            std::slice::from_ref(&kitty),
            std::slice::from_ref(&kitty),
            &[change(0, 0..2)],
        );
        assert!(again.is_empty());
        assert!(plan(std::slice::from_ref(&kitty), &[], &[]).is_empty());
    }
}

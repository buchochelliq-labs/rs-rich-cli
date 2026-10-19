//! Where the focus is, as state, and where it goes next by geometry: the
//! reactive reads [`focused`] and [`has_focus`], [`Direction`] for
//! [`Ctx::focus_direction`](crate::Ctx::focus_direction), and the pure
//! [`neighbour`] search under it, for widgets that place parts of their
//! own.

use crate::reactive::{NodeId, Runtime};
use crate::screen::Rect;

/// The node that has the focus on the top screen, read so that what reads
/// it (a text closure, a memo, a [`watch`](crate::watch), a size) follows
/// the focus as it moves. `None` with nothing focused, or outside an app.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::focus::focused;
///
/// let app = App::new(|| {
///     let list = label("list").focusable();
///     let id = list.id();
///     column([
///         label("menu").focusable(),
///         list,
///         text(move || match focused() == Some(id) {
///             true => "on the list".into(),
///             false => "elsewhere".into(),
///         }),
///     ])
/// });
/// let mut driver = app.driver(12, 3);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[2].trim_end(), "elsewhere");
/// driver.press("tab");
/// assert_eq!(driver.screen().plain()[2].trim_end(), "on the list");
/// ```
pub fn focused() -> Option<NodeId> {
    let path = Runtime::try_current().and_then(|runtime| runtime.focus.get())?;
    path.with(|path| path.last().copied())
}

/// Whether the focus is on node `id` or on a node inside it, read as
/// [`focused`] is: a pane's title lit while the focus is anywhere in the
/// pane, a diff that follows the list last focused.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::focus::has_focus;
///
/// let app = App::new(|| {
///     let pane = column([label("a").focusable(), label("b").focusable()]);
///     let id = pane.id();
///     column([
///         text(move || format!("in pane: {}", has_focus(id))).fixed(1),
///         label("top").focusable().fixed(1),
///         pane,
///     ])
/// });
/// let mut driver = app.driver(16, 4);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "in pane: false");
/// driver.press("tab tab");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "in pane: true");
/// ```
pub fn has_focus(id: NodeId) -> bool {
    Runtime::try_current()
        .and_then(|runtime| runtime.focus.get())
        .is_some_and(|path| path.with(|path| path.contains(&id)))
}

/// A side of the screen to move toward: [`Ctx::focus_direction`](crate::Ctx::focus_direction),
/// [`neighbour`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Of `rects`, the neighbour of `from` toward `direction`: the nearest one
/// wholly on that side, the ones sharing more of `from`'s edge first
/// (a pane beside it before one only diagonally there), then the closest
/// by their centres across the way. `None` with nothing on that side.
/// Empty rectangles are left out. Pure geometry, for any set of
/// rectangles: panes, tiles, cards, keys of a keyboard on screen.
///
/// ```
/// use intuituive::focus::{neighbour, Direction};
/// use intuituive::screen::Rect;
///
/// // A wide pane on top, two below it.
/// let rects = [Rect::new(0, 0, 20, 5), Rect::new(0, 5, 10, 5), Rect::new(10, 5, 10, 5)];
/// assert_eq!(neighbour(&rects, rects[2], Direction::Left), Some(1));
/// assert_eq!(neighbour(&rects, rects[1], Direction::Up), Some(0));
/// assert_eq!(neighbour(&rects, rects[0], Direction::Down), Some(1));
/// assert_eq!(neighbour(&rects, rects[1], Direction::Down), None);
/// ```
pub fn neighbour(rects: &[Rect], from: Rect, direction: Direction) -> Option<usize> {
    // Along the way: the gap from `from`'s edge to the candidate's, which
    // must not be negative; across it: the span each takes.
    let along = |r: Rect| -> Option<u32> {
        let (start, end) = match direction {
            Direction::Left => (r.right() as i32, from.x as i32),
            Direction::Right => (from.right() as i32, r.x as i32),
            Direction::Up => (r.bottom() as i32, from.y as i32),
            Direction::Down => (from.bottom() as i32, r.y as i32),
        };
        (end >= start).then_some((end - start) as u32)
    };
    let across = |r: Rect| -> (u32, u32) {
        match direction {
            Direction::Left | Direction::Right => (r.y as u32, r.bottom() as u32),
            Direction::Up | Direction::Down => (r.x as u32, r.right() as u32),
        }
    };
    let (from_start, from_end) = across(from);
    let from_mid = from_start + from_end;
    rects
        .iter()
        .enumerate()
        .filter(|(_, r)| !r.is_empty() && **r != from)
        .filter_map(|(i, r)| {
            let gap = along(*r)?;
            let (start, end) = across(*r);
            let shared = end.min(from_end).saturating_sub(start.max(from_start));
            let off = (start + end).abs_diff(from_mid);
            Some(((shared == 0, gap, u32::MAX - shared, off), i))
        })
        .min()
        .map(|(_, i)| i)
}

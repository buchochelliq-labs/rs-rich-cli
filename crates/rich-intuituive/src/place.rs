//! Where things go on the screen: pop-ups anchored to the screen
//! ([`Ctx::popup_at`](crate::Ctx::popup_at)), and a node's rectangle as
//! the last frame laid it out ([`Node::rect_handle`]).

use std::cell::Cell;

use rich::Console;

use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;

/// The point of the screen a [pop-up](crate::Ctx::popup_at) is placed
/// from, before its [`Offset`] moves it: a corner, the middle of an edge,
/// the centre, or a rectangle (a node's, from [`Node::rect_handle`]), below
/// which it goes, or above when there is no room below. Yazi's `Origin`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    TopLeft,
    TopCenter,
    TopRight,
    Center,
    BottomLeft,
    BottomCenter,
    BottomRight,
    /// Below this rectangle, from its left edge; above it when there is no
    /// room below.
    At(Rect),
}

/// How a [pop-up](crate::Ctx::popup_at) is moved from its [`Origin`], and
/// its size: `x` and `y` cells (negative: left and up), `w` x `h` as a
/// modal sizes itself ([`Size::Fixed`] cells, a [`Size::Percent`] of the
/// screen, [`Size::Auto`] the content's, [`Size::Flex`] the whole
/// screen). Whatever the offset, the pop-up stays on the screen: it is
/// moved back inside, and made smaller if it is larger.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offset {
    pub x: i16,
    pub y: i16,
    pub w: Size,
    pub h: Size,
}

impl Offset {
    /// `x`, `y` cells from the origin, `w` x `h` cells: Yazi's `Offset`.
    pub const fn new(x: i16, y: i16, w: u16, h: u16) -> Offset {
        Offset {
            x,
            y,
            w: Size::Fixed(w),
            h: Size::Fixed(h),
        }
    }

    /// At the origin, sized `w` x `h`.
    pub const fn sized(w: Size, h: Size) -> Offset {
        Offset { x: 0, y: 0, w, h }
    }
}

/// `size` of `total` cells, as a modal's width or height: `measure` gives
/// the content's.
pub(crate) fn resolve(size: Size, total: u16, measure: &dyn Fn() -> u16) -> u16 {
    match size {
        Size::Fixed(n) => n,
        Size::Percent(p) => (total as u32 * p.min(100) as u32 / 100) as u16,
        Size::Ratio(a, b) => crate::layout::ratio_of(total as u32, a, b) as u16,
        Size::Auto => measure(),
        Size::Flex(_) => total,
    }
    .min(total)
}

/// Where a box from `origin`, moved and sized by `offset`, goes inside
/// `area`; `root` is measured for [`Size::Auto`].
pub(crate) fn place(
    console: &Console,
    root: Option<&Node>,
    area: Rect,
    origin: Origin,
    offset: Offset,
) -> Rect {
    let measure = |axis: Axis, width: u16| match root {
        Some(root) => match axis {
            Axis::Horizontal => root.measure(console, axis, width, area.height),
            Axis::Vertical => root.measure(console, axis, width, 0),
        },
        None => 0,
    };
    let w = resolve(offset.w, area.width, &|| {
        measure(Axis::Horizontal, area.width)
    });
    let h = resolve(offset.h, area.height, &|| measure(Axis::Vertical, w));
    let middle = |start: u16, room: u16, size: u16| start + (room - size) / 2;
    let (x, y) = match origin {
        Origin::TopLeft => (area.x, area.y),
        Origin::TopCenter => (middle(area.x, area.width, w), area.y),
        Origin::TopRight => (area.right() - w, area.y),
        Origin::Center => (
            middle(area.x, area.width, w),
            middle(area.y, area.height, h),
        ),
        Origin::BottomLeft => (area.x, area.bottom() - h),
        Origin::BottomCenter => (middle(area.x, area.width, w), area.bottom() - h),
        Origin::BottomRight => (area.right() - w, area.bottom() - h),
        Origin::At(rect) => {
            let below = rect.bottom();
            let y = if below.saturating_add(h) <= area.bottom() {
                below
            } else {
                rect.y.saturating_sub(h)
            };
            (rect.x, y)
        }
    };
    let clamp = |at: u16, by: i16, start: u16, room: u16, size: u16| -> u16 {
        let last = (start + room - size) as i32;
        (at as i32 + by as i32).clamp(start as i32, last) as u16
    };
    Rect::new(
        clamp(x, offset.x, area.x, area.width, w),
        clamp(y, offset.y, area.y, area.height, h),
        w,
        h,
    )
}

/// Where a node is on the screen, as the last frame laid it out:
/// [`Node::rect_handle`], or [`RectHandle::new`] given to a node later with
/// [`Node::rect_into`]. Reading it with [`get`](Self::get) subscribes, so
/// what reads it draws again when the node moves. Two handles are equal
/// when they are the same handle, so one can be kept in a signal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RectHandle {
    rect: Signal<Option<Rect>>,
}

impl RectHandle {
    /// A handle for a node not made yet: app state can hold it before the
    /// view is built, and [`Node::rect_into`] gives it to the node. `None`
    /// until then.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::screen::Rect;
    /// use intuituive::RectHandle;
    ///
    /// let mut kept = None;
    /// let app = App::new(|| {
    ///     let handle = RectHandle::new();
    ///     kept = Some(handle);
    ///     column([label("body"), label("status").fixed(1).rect_into(&handle)])
    /// });
    /// let mut driver = app.driver(10, 4);
    /// driver.settle();
    /// assert_eq!(kept.unwrap().get_untracked(), Some(Rect::new(0, 3, 10, 1)));
    /// ```
    ///
    /// # Panics
    /// Outside a running or building app.
    #[allow(clippy::new_without_default)]
    pub fn new() -> RectHandle {
        HANDLES.with(|count| count.set(count.get() + 1));
        RectHandle {
            rect: crate::reactive::signal(None).untraced(),
        }
    }

    /// Its rectangle on the screen (inside a scrolling view, the part in
    /// view); `None` before its first frame, while it is hidden, or
    /// scrolled out of view.
    pub fn get(&self) -> Option<Rect> {
        self.rect.get()
    }

    /// [`get`](Self::get) without subscribing: in a handler.
    pub fn get_untracked(&self) -> Option<Rect> {
        self.rect.get_untracked()
    }
}

thread_local! {
    /// How many handles were made: frames look for them only when some
    /// were.
    static HANDLES: Cell<usize> = const { Cell::new(0) };
}

/// Whether any node on this thread has a [`RectHandle`].
pub(crate) fn any_handles() -> bool {
    HANDLES.with(Cell::get) > 0
}

impl Node {
    /// A handle to where this node is on the screen, set after each frame
    /// lays it out: to place a pop-up next to it ([`Origin::At`]), or below
    /// one of its rows. Call it before the node is moved into its parent.
    ///
    /// ```
    /// use intuituive::prelude::*;
    /// use intuituive::screen::Rect;
    ///
    /// let mut handle = None;
    /// let app = App::new(|| {
    ///     let status = label("status");
    ///     handle = Some(status.rect_handle());
    ///     column([label("body"), status.fixed(1)])
    /// });
    /// let mut driver = app.driver(10, 4);
    /// driver.settle();
    /// assert_eq!(handle.unwrap().get_untracked(), Some(Rect::new(0, 3, 10, 1)));
    /// ```
    ///
    /// # Panics
    /// Outside `App::new`'s closure or a screen's.
    pub fn rect_handle(&self) -> RectHandle {
        let mut slot = self.rect_slot.borrow_mut();
        let rect = *slot.get_or_insert_with(|| RectHandle::new().rect);
        RectHandle { rect }
    }

    /// Set `handle` (made with [`RectHandle::new`], before this node) to
    /// where this node is after each frame, as [`rect_handle`](Self::rect_handle)
    /// would. A node follows one handle: this replaces the one it had, and
    /// a handle is for one node.
    pub fn rect_into(self, handle: &RectHandle) -> Node {
        *self.rect_slot.borrow_mut() = Some(handle.rect);
        self
    }
}

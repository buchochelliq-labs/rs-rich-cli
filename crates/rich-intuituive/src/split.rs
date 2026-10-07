//! Two panes with a divider between them that the mouse drags.

use std::cell::Cell;

use rich_interact::{Button, KeyCode, MouseKind};

use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::screen::Rect;
use crate::widget::{widget, Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

struct Split {
    axis: Axis,
    children: Vec<Node>,
    ratio: Signal<f64>,
    min: u16,
    /// Whether the divider is being dragged, and whether the pointer was
    /// last seen on it.
    dragging: bool,
    over: Cell<bool>,
}

/// Two panes side by side (along [`Axis::Horizontal`]) or one above the
/// other (along [`Axis::Vertical`]), with a one-cell divider between them:
/// `│` or `─`. `ratio` (0 to 1) is the first pane's share; each pane keeps
/// at least 3 cells while there is room.
///
/// Dragging the divider with the mouse sets `ratio`. Alt+←/→ (Alt+↑/↓ when
/// stacked) nudge it by 5%; the split takes no focus, so the keys reach it
/// from the focused node inside. The divider is drawn in the theme's
/// `split.divider` style, and `split.divider.hover` while the pointer is on
/// it or drags it.
///
/// ```
/// use intuituive::node::Axis;
/// use intuituive::prelude::*;
/// use intuituive::widgets::split;
///
/// let app = App::new(|| {
///     let ratio = signal(0.5);
///     split(Axis::Horizontal, label("left"), label("right"), ratio).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 13, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "left  │right");
/// ```
pub fn split(axis: Axis, first: Node, second: Node, ratio: Signal<f64>) -> Node {
    split_with(axis, first, second, ratio, 3)
}

/// A [`split`] whose panes each keep at least `min` cells while there is
/// room.
///
/// ```
/// use intuituive::node::Axis;
/// use intuituive::prelude::*;
/// use intuituive::widgets::split_with;
///
/// let app = App::new(|| {
///     let ratio = signal(0.0);
///     split_with(Axis::Horizontal, label("a"), label("b"), ratio, 1).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 6, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "a│b");
/// ```
pub fn split_with(axis: Axis, first: Node, second: Node, ratio: Signal<f64>, min: u16) -> Node {
    widget(Split {
        axis,
        children: vec![first, second],
        ratio,
        min,
        dragging: false,
        over: Cell::new(false),
    })
}

/// A [`split`] with the panes side by side.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::hsplit;
///
/// let app = App::new(|| hsplit(label("a"), label("b"), signal(0.5)).on_key("q", |cx| cx.quit()));
/// let screen = app.render_with(&["q"], 9, 1).unwrap();
/// assert_eq!(screen[0].trim_end(), "a   │b");
/// ```
pub fn hsplit(first: Node, second: Node, ratio: Signal<f64>) -> Node {
    split(Axis::Horizontal, first, second, ratio)
}

/// A [`split`] with the panes one above the other.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::widgets::vsplit;
///
/// let app = App::new(|| vsplit(label("top"), label("bottom"), signal(0.5)).on_key("q", |cx| cx.quit()));
/// let screen = app.render_with(&["q"], 6, 7).unwrap();
/// assert_eq!(screen[0].trim_end(), "top");
/// assert_eq!(screen[3].trim_end(), "──────");
/// assert_eq!(screen[4].trim_end(), "bottom");
/// ```
pub fn vsplit(first: Node, second: Node, ratio: Signal<f64>) -> Node {
    split(Axis::Vertical, first, second, ratio)
}

impl Split {
    /// The cells along the axis, of `width` x `height`.
    fn extent(&self, width: u16, height: u16) -> u16 {
        match self.axis {
            Axis::Horizontal => width,
            Axis::Vertical => height,
        }
    }

    /// The range the first pane's size may take, of `extent` cells.
    fn bounds(&self, extent: u16) -> (u16, u16) {
        let room = extent.saturating_sub(1);
        let low = self.min.min(room / 2);
        (low, room - low)
    }

    /// The first pane's size (where the divider is) for `ratio`, of
    /// `extent` cells.
    fn first(&self, ratio: f64, extent: u16) -> u16 {
        let room = extent.saturating_sub(1);
        let (low, high) = self.bounds(extent);
        let cells = (ratio.clamp(0.0, 1.0) * room as f64).round() as u16;
        cells.clamp(low, high)
    }

    /// Put the divider at `at`, of `extent` cells.
    fn place(&self, at: i32, extent: u16) {
        let room = extent.saturating_sub(1);
        if room == 0 {
            return;
        }
        let (low, high) = self.bounds(extent);
        let at = at.clamp(low as i32, high as i32);
        self.ratio.set(at as f64 / room as f64);
    }

    /// The local coordinate along the axis of a mouse event.
    fn along(&self, column: u16, row: u16) -> u16 {
        match self.axis {
            Axis::Horizontal => column,
            Axis::Vertical => row,
        }
    }
}

impl Widget for Split {
    fn name(&self) -> &'static str {
        "split"
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let extent = self.extent(rect.width, rect.height);
        if extent == 0 {
            return Vec::new();
        }
        let first = self.first(self.ratio.get(), extent);
        let second = extent - first - 1;
        match self.axis {
            Axis::Horizontal => vec![
                Rect::new(rect.x, rect.y, first, rect.height),
                Rect::new(rect.x + first + 1, rect.y, second, rect.height),
            ],
            Axis::Vertical => vec![
                Rect::new(rect.x, rect.y, rect.width, first),
                Rect::new(rect.x, rect.y + first + 1, rect.width, second),
            ],
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let extent = self.extent(width, height);
        if extent == 0 {
            return;
        }
        let at = self.first(self.ratio.get(), extent);
        // Asked every time, so the pointer leaving the split redraws it.
        let hovered = cx.hovered() && self.over.get();
        let style = if self.dragging || hovered {
            cx.style("split.divider.hover", "bold")
        } else {
            cx.style("split.divider", "bright_black")
        };
        match self.axis {
            Axis::Horizontal => {
                for y in 0..height {
                    canvas.set(at, y, '│', Some(&style));
                }
            }
            Axis::Vertical => canvas.print(0, at, &"─".repeat(width as usize), Some(&style)),
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let (width, height) = cx.size();
        let extent = self.extent(width, height);
        match event {
            WidgetEvent::Key(key) => {
                if !key.modifiers.alt || key.modifiers.ctrl {
                    return Used::No;
                }
                let by = match (self.axis, key.code) {
                    (Axis::Horizontal, KeyCode::Left) | (Axis::Vertical, KeyCode::Up) => -0.05,
                    (Axis::Horizontal, KeyCode::Right) | (Axis::Vertical, KeyCode::Down) => 0.05,
                    _ => return Used::No,
                };
                let room = extent.saturating_sub(1).max(1) as f64;
                let (low, high) = self.bounds(extent);
                let now = self.first(self.ratio.get_untracked(), extent) as f64 / room;
                let next = (now + by).clamp(low as f64 / room, high as f64 / room);
                self.ratio.set(next);
                Used::Yes
            }
            WidgetEvent::Mouse(mouse) => {
                let at = self.along(mouse.column, mouse.row);
                let divider = self.first(self.ratio.get_untracked(), extent);
                match mouse.kind {
                    MouseKind::Down(Button::Left) if at == divider => {
                        self.dragging = true;
                        cx.capture_mouse();
                        cx.redraw();
                    }
                    MouseKind::Drag(_) if self.dragging => self.place(at as i32, extent),
                    MouseKind::Up(_) if self.dragging => {
                        self.dragging = false;
                        cx.release_mouse();
                        cx.redraw();
                    }
                    MouseKind::Moved => {
                        let over = at == divider;
                        if self.over.replace(over) != over {
                            cx.redraw();
                        }
                        return Used::No;
                    }
                    _ => return Used::No,
                }
                Used::Yes
            }
        }
    }
}

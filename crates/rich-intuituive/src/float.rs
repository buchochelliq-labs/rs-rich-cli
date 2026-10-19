//! Floats: what a widget draws over its neighbours without taking the
//! keys, as an input's completion list under it. A widget sets its float
//! while it draws ([`DrawCx::float`](crate::widget::DrawCx)); the app
//! paints each screen's floats after the screen, every frame, and draws
//! everything again when one shrinks or goes, so the cells it covered are
//! their owners' again.

use rich::Segment;

use crate::node::Node;
use crate::reactive::NodeId;
use crate::reactive::Runtime;
use crate::screen::{Rect, Screen};

/// A widget's float: `lines` (each `width` cells, in their own styles)
/// below the screen rectangle `anchor` from column `x`, or above it when
/// there is no room below.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Float {
    pub anchor: Rect,
    pub x: u16,
    pub width: u16,
    pub lines: Vec<Vec<Segment>>,
}

impl Float {
    /// Where it goes in `area`.
    fn rect(&self, area: Rect) -> Rect {
        let height = (self.lines.len() as u16).min(area.height);
        let width = self.width.min(area.width);
        let below = self.anchor.bottom();
        let y = if below.saturating_add(height) <= area.bottom() {
            below
        } else {
            self.anchor.y.saturating_sub(height).max(area.y)
        };
        let x = self.x.min(area.right().saturating_sub(width)).max(area.x);
        Rect::new(x, y, width, height)
    }
}

/// Paint the floats of the nodes shown in `root` (a screen just drawn)
/// within `area`; where each went, and the nodes they belong to.
pub(crate) fn paint(
    runtime: &Runtime,
    root: &Node,
    screen: &mut Screen,
    area: Rect,
) -> Vec<(NodeId, Rect)> {
    let floats = runtime.floats.borrow();
    if floats.is_empty() {
        return Vec::new();
    }
    let mut painted = Vec::new();
    crate::node::walk_screen(root, &mut |node, _, _, _| {
        if let Some(float) = floats.get(&node.id()) {
            let rect = float.rect(area);
            if !rect.is_empty() {
                screen.write_lines(rect, &float.lines);
                painted.push((node.id(), rect));
            }
        }
    });
    painted
}

/// Whether every cell of `old` is in one of `now`'s rectangles.
pub(crate) fn covered(old: Rect, now: &[Rect]) -> bool {
    (old.y..old.bottom())
        .all(|y| (old.x..old.right()).all(|x| now.iter().any(|r| r.contains(x, y))))
}

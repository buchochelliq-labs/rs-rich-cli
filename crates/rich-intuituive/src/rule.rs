//! A horizontal line: [`rule`]. Inside a [panel](crate::Node::panel),
//! running from its left side to its right, it joins the panel's border as
//! `├──┤`, the help box's divider.

use crate::border::Border;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

/// A line across, one row tall, in the theme's `border` style: a divider
/// between the parts of a column. Directly inside a
/// [panel](crate::Node::panel) whose sides it reaches (no padding between),
/// it is drawn in the panel's border style and kind and joins its sides:
/// `├────┤`. [`Node::border`] picks the kind of a rule on its own
/// ([`Border::Rounded`]'s light line by default).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::rule;
///
/// let app = App::new(|| column([label("Keys"), rule(), label("q quit")]).panel("Help"));
/// let mut driver = app.driver(12, 5);
/// driver.settle();
/// assert_eq!(
///     driver.screen().plain(),
///     ["╭─ Help ───╮", "│Keys      │", "├──────────┤", "│q quit    │", "╰──────────╯"]
/// );
/// ```
pub fn rule() -> Node {
    let mut node = Node::from_widget(Box::new(Rule { kind: None }), "rule");
    // One row in a column: not a flexible share of it.
    node.size = Size::Fixed(1);
    node.access_hidden(true)
}

/// A [`rule`]'s widget.
pub(crate) struct Rule {
    /// Its kind, when code set one ([`Node::border`]).
    pub kind: Option<Border>,
}

/// The glyph a `kind` of line draws for a cell with `arms` (up, right,
/// down, left).
pub(crate) fn glyph(kind: Border, arms: [bool; 4]) -> &'static str {
    let weight = kind.weight();
    let arms = arms.map(|on| if on { weight } else { 0 });
    crate::border::lattice_glyph(kind, arms, [false; 4])
}

impl Widget for Rule {
    fn name(&self) -> &'static str {
        "rule"
    }

    fn measure(&mut self, _cx: &MeasureCx, _axis: Axis, _width: u16, _height: u16) -> u16 {
        1
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let kind = self.kind.unwrap_or(Border::Rounded);
        let line = glyph(kind, [false, true, false, true]);
        let style = cx.theme().border(false);
        for x in 0..canvas.width() {
            canvas.set_cell(x, 0, line, Some(&style));
        }
    }
}

/// The rules directly inside `node` (not inside a scrolling view), where
/// they were laid out.
pub(crate) fn rules_in(node: &Node, out: &mut Vec<Rect>) {
    if node.hidden() {
        return;
    }
    let body = node.body.borrow();
    let widget: &dyn std::any::Any = &*body.widget;
    if widget.is::<Rule>() {
        out.push(node.rect());
        return;
    }
    if body.widget.viewport() {
        return;
    }
    for child in body.widget.children() {
        rules_in(child, out);
    }
}

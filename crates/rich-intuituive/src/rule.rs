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
/// [panel](crate::Node::panel) whose sides it reaches, it is drawn in the
/// panel's border style and kind and joins its sides: `├────┤`. Padding
/// between the two (the panel's, or a [`padding`](crate::Node::padding)
/// round the column) does not keep it from them: the rule runs across it. [`Node::border`] picks the kind of a rule on its own
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
///
/// With the column padded, the rule still reaches the sides:
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::rule;
///
/// let app = App::new(|| column([label("Keys"), rule(), label("q quit")]).padding(0, 1).panel("Help"));
/// let mut driver = app.driver(12, 5);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[1], "│ Keys     │");
/// assert_eq!(driver.screen().plain()[2], "├──────────┤");
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
/// they were laid out, each reaching through the [padding](Node::padding)
/// it touches: a rule at the inside edge of a padded node runs on to that
/// node's own edge.
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
    let from = out.len();
    for child in body.widget.children() {
        rules_in(child, out);
    }
    if let Some(pad) = widget.downcast_ref::<crate::builtin::Pad>() {
        let [_, right, _, left] = pad.edges;
        reach(&mut out[from..], node.rect(), left, right);
    }
}

/// Run each of `rules` that starts `left` cells inside `outer` (or ends
/// `right` cells inside it) on to its edge.
pub(crate) fn reach(rules: &mut [Rect], outer: Rect, left: u16, right: u16) {
    for rule in rules {
        if left > 0 && rule.x == outer.x.saturating_add(left) {
            rule.x = outer.x;
            rule.width = rule.width.saturating_add(left);
        }
        if right > 0 && rule.right().saturating_add(right) == outer.right() {
            rule.width = rule.width.saturating_add(right);
        }
    }
}

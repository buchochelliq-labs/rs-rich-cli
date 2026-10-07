//! The inspector: a panel docked on the right of a running app that shows
//! the node tree as it is, which nodes drew in the last frame, where the
//! focus is, and what the frame cost.
//!
//! Turn it on with [`App::inspector`](crate::App::inspector) or by setting
//! `INTUITUIVE_INSPECT=1` before starting any app; F12 shows and hides it.
//! The app is laid out in the space to its left, so everything stays
//! visible. Each line of the tree is one node:
//!
//! - its [name](crate::Node::name), or the builder that made it, and what it
//!   holds (a panel's title, a list's length, a grid's shape);
//! - its size, or `hidden` when it has no room or is not shown;
//! - yellow if it drew in the last frame, reversed if it has the focus.

use rich::{Console, Segment};

use crate::app::FrameStats;
use crate::node::Node;
use crate::reactive::NodeId;
use crate::screen::Rect;

/// The inspector's state.
pub(crate) struct Inspector {
    pub open: bool,
    /// Frames drawn since the app started.
    pub frames: u64,
}

impl Inspector {
    pub fn new(open: bool) -> Inspector {
        Inspector { open, frames: 0 }
    }

    /// The columns the panel takes from a screen `width` wide, or `None`
    /// if the screen is too narrow to share.
    pub fn width(&self, width: u16) -> Option<u16> {
        (self.open && width >= 40).then(|| (width * 2 / 5).clamp(24, 56))
    }
}

/// What the last frame did, for the panel.
pub(crate) struct Report<'a> {
    /// The screens drawn, bottom first, with whether each is a modal.
    pub layers: Vec<(&'a Node, bool)>,
    pub drawn: &'a [NodeId],
    pub dirty: usize,
    pub damage: &'a [Rect],
    pub focus: Option<NodeId>,
    /// The frame before this one (this one's bytes are not known yet).
    pub stats: FrameStats,
    pub frame: u64,
    pub theme: Option<String>,
}

/// The panel's lines, rendered at `width` x `height` with its border.
pub(crate) fn render(
    console: &Console,
    report: &Report,
    width: u16,
    height: u16,
) -> Vec<Vec<Segment>> {
    let cells: u32 = report
        .damage
        .iter()
        .map(|r| r.width as u32 * r.height as u32)
        .sum();
    let mut lines = vec![
        "[b]inspector[/] [dim]· F12 hides".to_string(),
        format!(
            "frame {} · [yellow]drew {}[/] of {} dirty",
            report.frame,
            report.drawn.len(),
            report.dirty
        ),
        format!("damage {} rects, {cells} cells", report.damage.len()),
        format!("sent {} B last frame", report.stats.bytes),
    ];
    if let Some(theme) = &report.theme {
        lines.push(format!("theme {}", rich::markup::escape(theme)));
    }
    lines.push(String::new());
    let room = (height as usize).saturating_sub(2 + lines.len());
    let mut tree = Vec::new();
    for (index, (root, modal)) in report.layers.iter().enumerate() {
        let kind = if *modal { "modal" } else { "screen" };
        tree.push(format!("[dim]{kind} {}[/]", index + 1));
        root.walk(&mut |node, path| {
            let depth = path.len();
            let rect = node.rect();
            let size = if rect.is_empty() {
                "[dim]hidden[/]".to_string()
            } else {
                format!("[dim]{}x{}[/]", rect.width, rect.height)
            };
            let what = rich::markup::escape(&node.describe());
            let what = if report.focus == Some(node.id()) {
                format!("[reverse]{what}[/]")
            } else if report.drawn.contains(&node.id()) {
                format!("[yellow]{what}[/]")
            } else {
                what
            };
            tree.push(format!("{}{what} {size}", "  ".repeat(depth.min(12))));
        });
    }
    if tree.len() > room {
        let more = tree.len() - room + 1;
        tree.truncate(room.saturating_sub(1));
        tree.push(format!("[dim]… {more} more"));
    }
    lines.extend(tree);
    let inner = width.saturating_sub(2).max(1) as usize;
    let text = rich::Text::from_markup(&lines.join("\n"))
        .unwrap_or_else(|_| rich::Text::new(lines.join("\n")));
    let mut options = console.options().update_width(inner);
    options.no_wrap = Some(true);
    options.overflow = Some(rich::Overflow::Ellipsis);
    let body = console.render_lines(&text, &options, true);
    let edge = Some(rich::Style::parse("magenta").expect("a built-in style parses"));
    let mut out = Vec::with_capacity(height as usize);
    out.push(vec![Segment::new(
        format!("╭{}╮", "─".repeat(inner)),
        edge.clone(),
    )]);
    for row in 0..(height as usize).saturating_sub(2) {
        let mut line = vec![Segment::new("│", edge.clone())];
        match body.get(row) {
            Some(cells) => line.extend(cells.iter().cloned()),
            None => line.push(Segment::new(" ".repeat(inner), None)),
        }
        line.push(Segment::new("│", edge.clone()));
        out.push(line);
    }
    out.push(vec![Segment::new(format!("╰{}╯", "─".repeat(inner)), edge)]);
    out
}

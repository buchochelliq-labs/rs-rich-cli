//! The built-in nodes, each a [`Widget`]: text and other leaves, columns
//! and rows, grids, panels, padding, keyed lists, switches, hosted
//! components and scrolls. They use only what the trait gives any widget,
//! which is the test that it is enough.

use std::collections::HashMap;
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Overflow, Segment, Style};
use rich_interact::{Component, Context, Event, Flow, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::border::{Border, Edge, Lattice, Shadow, Sides, Title};
use crate::layout::{
    grow_for_span, offsets, place, solve, Align, AlignContent, Justify, Size, StackLayout,
    TextAlign, Track,
};
use crate::node::{Axis, Handler, Node};
use crate::reactive::{NodeId, Runtime, Signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, ScrollCx, Used, Widget, WidgetEvent};

pub(crate) type Draw = Box<dyn Fn(&Console, u16, u16) -> Vec<Vec<Segment>>>;

/// The most rows a [`scroll`](crate::scroll) lays its child out in.
pub(crate) const MAX_SCROLL_ROWS: u16 = 4000;
/// The widest content a view that scrolls across lays out.
pub(crate) const MAX_SCROLL_COLUMNS: u16 = 2000;

/// How far rendered `lines` reach along `axis`.
fn extent(lines: &[Vec<Segment>], axis: Axis) -> u16 {
    match axis {
        Axis::Vertical => lines.len(),
        Axis::Horizontal => lines
            .iter()
            .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
            .max()
            .unwrap_or(0),
    }
    .min(u16::MAX as usize) as u16
}

/// A node drawn from rendered lines ([`leaf`](crate::leaf), text, labels,
/// renderables, lists).
pub(crate) struct Leaf(pub Draw);

impl Widget for Leaf {
    fn name(&self) -> &'static str {
        "leaf"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        extent(&(self.0)(cx.console(), width.max(1), height), axis)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let lines = (self.0)(cx.console(), canvas.width(), canvas.height());
        canvas.lines(&lines);
    }
}

/// Children along an axis: [`column`](crate::column) and [`row`](crate::row).
pub(crate) struct Stack {
    pub axis: Axis,
    /// The gap, alignment and wrapping.
    pub flow: StackLayout,
    pub children: Vec<Node>,
    /// Its children's borders collapsed into one frame, when on.
    pub collapse: Collapse,
}

/// A container's [collapsed borders](crate::Node::collapse_borders): one
/// frame round all its children, which share their edges.
#[derive(Default)]
pub(crate) struct Collapse {
    pub on: bool,
    /// Whether code turned it on or off (else a stylesheet may).
    pub code: bool,
    /// Code's kind of line, and the focused child's.
    pub border: Option<Border>,
    pub focus_border: Option<Border>,
    /// A style or theme style name for the lines while the focus is
    /// elsewhere.
    pub border_style: Option<String>,
    /// The stylesheet's `border` on the container: its kind and colour.
    pub sheet: Option<(Border, Style)>,
    /// Each shown child's box, its lines included (screen coordinates),
    /// with its index, as last laid out.
    boxes: Vec<(usize, Rect)>,
    /// The child whose edges were last drawn lit, once drawn.
    lit: Option<Option<usize>>,
    /// Inside a container that collapses its children's borders too: the
    /// cells of that container's lines on this one's outline (screen
    /// coordinates), with their arms and whether they are lit, joined
    /// with this one's own where they meet; and that container's kind of
    /// line, its focused child's and its lines' style, which this one
    /// takes unless it has its own.
    inherited: Vec<((u16, u16), [u8; 4], bool)>,
    inherited_titles: Vec<TitlePiece>,
    look: Option<(Border, Border, Style)>,
}

/// Whether `child` collapses its own children's borders: inside a
/// container that does too, it shares that container's lines.
fn nested(child: &Node) -> bool {
    child
        .widget_ref(|stack: &Stack| stack.collapse.on)
        .or_else(|| child.widget_ref(|grid: &Grid| grid.collapse.on))
        .unwrap_or(false)
}

impl Collapse {
    /// Lay its children's boxes out from `boxes` (the box of each shown
    /// child, by index); the children's areas are the boxes less their
    /// lines, but a child that collapses its own children's borders gets
    /// its whole box, its outline on this one's lines.
    fn settle(&mut self, children: &[Node], boxes: Vec<(usize, Rect)>) -> Vec<Rect> {
        let mut areas = vec![Rect::default(); children.len()];
        for (i, rect) in &boxes {
            areas[*i] = if nested(&children[*i]) {
                *rect
            } else {
                rect.inner(1)
            };
        }
        self.boxes = boxes;
        areas
    }

    /// Draw the frame and the children's titles, when it changed: the
    /// lines round the focused child lit, in its kind. A child that
    /// collapses its own children's borders lights its own.
    fn draw(&mut self, children: &[Node], cx: &mut DrawCx, canvas: &mut Canvas) {
        let lit = cx.focus_path.with(|path| {
            self.boxes
                .iter()
                .find(|(i, _)| path.contains(&children[*i].id))
                .map(|(i, _)| *i)
                .filter(|i| !nested(&children[*i]))
        });
        if !cx.repaint() && self.lit == Some(lit) {
            return;
        }
        self.lit = Some(lit);
        // Its own look, else a container round it's, else the theme's.
        let around = self.look.clone();
        let kind = self
            .border
            .or(self.sheet.as_ref().map(|(kind, _)| *kind))
            .or(around.as_ref().map(|look| look.0))
            .unwrap_or_default();
        let focus_kind = self
            .focus_border
            .or(around.as_ref().map(|look| look.1))
            .unwrap_or(kind);
        let sheet_style = self.sheet.as_ref().map(|(_, style)| style.clone());
        let style = match (&self.border_style, sheet_style, around) {
            (Some(name), _, _) => cx.style(name, ""),
            (None, Some(style), _) if !style.is_null() => style,
            (None, _, Some(look)) => look.2,
            _ => cx.theme().border(false),
        };
        let lit_style = cx.theme().border(true);
        let origin = (cx.rect.x, cx.rect.y);
        let local = |rect: Rect| {
            Rect::new(
                rect.x.saturating_sub(origin.0),
                rect.y.saturating_sub(origin.1),
                rect.width,
                rect.height,
            )
        };
        let mut lattice = Lattice::default();
        // The lines of a container round this one, where they meet its
        // outline.
        for &((x, y), arms, on) in &self.inherited {
            if x >= origin.0 && y >= origin.1 {
                lattice.join(x - origin.0, y - origin.1, arms, on);
            }
        }
        if kind != Border::None {
            for (i, rect) in &self.boxes {
                let on = Some(*i) == lit;
                let weight = if on { focus_kind } else { kind }.weight();
                lattice.outline(local(*rect), weight, on);
            }
        }
        let outer = self
            .boxes
            .iter()
            .fold(Rect::default(), |all, (_, rect)| all.union(local(*rect)));
        // Row by row, a segment for each run of cells side by side.
        let cells = lattice.cells();
        let mut row: Vec<Segment> = Vec::new();
        let mut start: Option<(u16, u16)> = None;
        let mut next = (0u16, 0u16);
        let flush = |canvas: &mut Canvas, row: &mut Vec<Segment>, start: Option<(u16, u16)>| {
            if let Some((x, y)) = start {
                let width: usize = row.iter().map(Segment::cell_length).sum();
                canvas.lines_at(x, y, width as u16, 1, &[std::mem::take(row)]);
            }
        };
        for ((x, y), arms, on) in cells {
            if start.is_some() && next != (x, y) {
                flush(canvas, &mut row, start);
                start = None;
            }
            start = start.or(Some((x, y)));
            next = (x + 1, y);
            let edges = [
                y == outer.y,
                x + 1 == outer.right(),
                y + 1 == outer.bottom(),
                x == outer.x,
            ];
            let glyph =
                crate::border::lattice_glyph(if on { focus_kind } else { kind }, arms, edges);
            let style = if on { &lit_style } else { &style };
            row.push(Segment::new(glyph, Some(style.clone())));
        }
        flush(canvas, &mut row, start);
        // Titles over the lines: a container round this one's on its
        // outline, then each child's on its own edges.
        let mut pieces: Vec<TitlePiece> = self
            .inherited_titles
            .iter()
            .filter(|(x, y, _, _)| *x >= origin.0 && *y >= origin.1)
            .map(|(x, y, segments, width)| (x - origin.0, y - origin.1, segments.clone(), *width))
            .collect();
        for (i, rect) in &self.boxes {
            let Some(titles) = children[*i].widget_ref(|panel: &Panel| {
                let title_style = panel
                    .code
                    .title_style
                    .as_ref()
                    .map_or_else(|| cx.theme().title.clone(), |name| cx.style(name, ""));
                (panel.titles(), title_style)
            }) else {
                continue;
            };
            let (titles, title_style) = titles;
            let base = if Some(*i) == lit { &lit_style } else { &style };
            let title_style = base.combine(&title_style);
            let rect = local(*rect);
            if rect.width < 5 {
                continue;
            }
            for (edge, y) in [(Edge::Top, rect.y), (Edge::Bottom, rect.bottom() - 1)] {
                let on_edge: Vec<&Title> = titles.iter().filter(|t| t.edge == edge).collect();
                let area = (rect.x + 2, rect.right() - 2);
                let line = kind != Border::None;
                for (x, segments, width) in
                    crate::border::title_pieces(cx.console(), &on_edge, area, line, &title_style)
                {
                    pieces.push((x, y, segments, width));
                }
            }
        }
        for (x, y, segments, width) in &pieces {
            canvas.lines_at(*x, *y, *width, 1, std::slice::from_ref(segments));
        }
        // A child that collapses its own children's borders joins its
        // lines to these, and draws the titles on its outline again over
        // them.
        for (i, rect) in &self.boxes {
            if !nested(&children[*i]) {
                continue;
            }
            let rect = local(*rect);
            let screen = |(x, y): (u16, u16)| (x + origin.0, y + origin.1);
            let shared: Vec<((u16, u16), [u8; 4], bool)> = lattice
                .on_outline(rect)
                .into_iter()
                .map(|(at, arms, on)| (screen(at), arms, on))
                .collect();
            let titles: Vec<TitlePiece> = pieces
                .iter()
                .filter(|(x, y, _, width)| {
                    (*y == rect.y || *y + 1 == rect.bottom())
                        && *x < rect.right()
                        && x + width > rect.x
                })
                .map(|(x, y, segments, width)| {
                    let (x, y) = screen((*x, *y));
                    (x, y, segments.clone(), *width)
                })
                .collect();
            let look = (kind, focus_kind, style.clone());
            let give = |collapse: &mut Collapse| {
                collapse.inherited = shared.clone();
                collapse.inherited_titles = titles.clone();
                collapse.look = Some(look.clone());
            };
            children[*i].widget_mut(|stack: &mut Stack| give(&mut stack.collapse));
            children[*i].widget_mut(|grid: &mut Grid| give(&mut grid.collapse));
        }
    }
}

/// A title drawn on a line: where it starts, its segments and its width.
type TitlePiece = (u16, u16, Vec<Segment>, u16);

impl Widget for Stack {
    fn name(&self) -> &'static str {
        match self.axis {
            Axis::Vertical => "column",
            Axis::Horizontal => "row",
        }
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        if self.collapse.on {
            return collapsed_measure(cx, self.axis, &self.children, axis, width, height);
        }
        let gap = self.flow.gap;
        // Wrapped, the size across the axis is the lines' sizes: a row as
        // wide as `width`, or a column as tall as `height` (when bounded).
        let bounded = match self.axis {
            Axis::Horizontal => true,
            Axis::Vertical => height > 0,
        };
        if self.flow.wrap && self.axis != axis && bounded {
            let (total, room) = match self.axis {
                Axis::Horizontal => (width, height),
                Axis::Vertical => (height, width),
            };
            let lines = cx.wrap_lines(self.axis, gap, &self.children, total, room);
            let gaps = gap.saturating_mul(lines.len().saturating_sub(1) as u16);
            return lines
                .iter()
                .fold(gaps, |sum, (_, _, cross)| sum.saturating_add(*cross));
        }
        let shown = self.children.iter().filter(|c| !c.hidden()).count();
        let gaps = gap.saturating_mul(shown.saturating_sub(1) as u16);
        if self.axis == axis {
            self.children.iter().fold(gaps, |sum, child| {
                sum.saturating_add(cx.extent(child, axis, width, height))
            })
        } else {
            self.children
                .iter()
                .map(|child| cx.measure(child, axis, width, height))
                .max()
                .unwrap_or(0)
        }
    }

    fn describe(&self) -> Option<String> {
        let flow = self.flow;
        let mut parts = Vec::new();
        if self.collapse.on {
            parts.push("border-collapse".to_string());
        }
        if flow.align_items != Align::Stretch {
            parts.push(format!("align-items:{}", css_align(flow.align_items)));
        }
        if flow.justify != Justify::Start {
            parts.push(format!("justify:{}", css_justify(flow.justify)));
        }
        if flow.wrap {
            parts.push("wrap".to_string());
            if flow.align_content != AlignContent::Stretch {
                parts.push(format!(
                    "align-content:{}",
                    css_align_content(flow.align_content)
                ));
            }
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        mark_collapsed(&self.children, self.collapse.on);
        if !self.collapse.on {
            return cx.stack_flow(self.axis, self.flow, &self.children, rect);
        }
        let boxes = collapsed_boxes(cx, self.axis, &self.children, rect);
        self.collapse.settle(&self.children, boxes)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if self.collapse.on {
            self.collapse.draw(&self.children, cx, canvas);
        }
    }

    fn retained(&self) -> bool {
        true
    }
}

/// Tell each child panel whether its container collapses their borders.
fn mark_collapsed(children: &[Node], on: bool) {
    for child in children {
        child.widget_mut(|panel: &mut Panel| panel.collapsed = on);
    }
}

/// The cells `child` takes along `axis` as a box with lines round it: a
/// panel's size counts its border already, as a container that collapses
/// its children's borders counts its outline; another node's content gets
/// the two lines added.
fn boxed(child: &Node, cells: u16, content: bool) -> u16 {
    let framed = child.widget_ref(|_: &Panel| ()).is_some() || nested(child);
    if content && !framed {
        cells.saturating_add(2)
    } else {
        cells
    }
}

/// Whether `child` is sized by its content along its parent's axis.
fn by_content(child: &Node) -> bool {
    matches!(child.size_hint(), Size::Auto | Size::Flex(_))
}

/// The boxes of a collapsed row's or column's children, each sharing its
/// edge with the next: with `n` shown, their sizes add up to the room plus
/// `n - 1`, the lines they share.
fn collapsed_boxes(
    cx: &MeasureCx,
    axis: Axis,
    children: &[Node],
    rect: Rect,
) -> Vec<(usize, Rect)> {
    let shown: Vec<usize> = (0..children.len())
        .filter(|&i| !children[i].hidden())
        .collect();
    if shown.is_empty() {
        return Vec::new();
    }
    let (start, total, across, breadth) = match axis {
        Axis::Vertical => (rect.y, rect.height, rect.x, rect.width),
        Axis::Horizontal => (rect.x, rect.width, rect.y, rect.height),
    };
    let tracks: Vec<Track> = shown
        .iter()
        .map(|&i| {
            let child = &children[i];
            let mut track = cx.main_track(child, axis, rect);
            if track.size == Size::Auto {
                track.content = boxed(child, track.content, true);
            }
            track
        })
        .collect();
    let room = total.saturating_add(shown.len() as u16 - 1);
    let sizes = solve(room, 0, &tracks);
    let mut at = start;
    shown
        .iter()
        .zip(sizes)
        .map(|(&i, size)| {
            let rect = match axis {
                Axis::Vertical => Rect::new(across, at, breadth, size),
                Axis::Horizontal => Rect::new(at, across, size, breadth),
            };
            at = at.saturating_add(size.saturating_sub(1));
            (i, rect)
        })
        .collect()
}

/// What a collapsed row or column measures along `axis`: its children's
/// boxes, less the lines they share.
fn collapsed_measure(
    cx: &MeasureCx,
    stack: Axis,
    children: &[Node],
    axis: Axis,
    width: u16,
    height: u16,
) -> u16 {
    let shown: Vec<&Node> = children.iter().filter(|c| !c.hidden()).collect();
    if stack == axis {
        let sum = shown.iter().fold(0u16, |sum, child| {
            let cells = cx.extent(child, axis, width, height);
            sum.saturating_add(boxed(child, cells, by_content(child)))
        });
        sum.saturating_sub(shown.len().saturating_sub(1) as u16)
    } else {
        shown
            .iter()
            .map(|child| boxed(child, cx.measure(child, axis, width, height), true))
            .max()
            .unwrap_or(0)
    }
}

/// The stylesheet's name for `align`.
pub(crate) fn css_align(align: Align) -> &'static str {
    match align {
        Align::Start => "flex-start",
        Align::Center => "center",
        Align::End => "flex-end",
        Align::Stretch => "stretch",
    }
}

fn css_justify(justify: Justify) -> &'static str {
    match justify {
        Justify::Start => "flex-start",
        Justify::Center => "center",
        Justify::End => "flex-end",
        Justify::SpaceBetween => "space-between",
        Justify::SpaceAround => "space-around",
        Justify::SpaceEvenly => "space-evenly",
    }
}

fn css_align_content(align: AlignContent) -> &'static str {
    match align {
        AlignContent::Start => "flex-start",
        AlignContent::Center => "center",
        AlignContent::End => "flex-end",
        AlignContent::SpaceBetween => "space-between",
        AlignContent::SpaceAround => "space-around",
        AlignContent::SpaceEvenly => "space-evenly",
        AlignContent::Stretch => "stretch",
    }
}

/// Children in rows and columns: [`grid`](crate::grid).
pub(crate) struct Grid {
    pub columns: Vec<Size>,
    /// Whether code gave the columns (else a stylesheet may).
    pub code_columns: bool,
    pub rows: Vec<Size>,
    /// Between rows, and between columns.
    pub gap: (u16, u16),
    pub children: Vec<Node>,
    /// Its children's borders collapsed into one frame, when on.
    pub collapse: Collapse,
}

impl Widget for Grid {
    fn name(&self) -> &'static str {
        "grid"
    }

    fn describe(&self) -> Option<String> {
        let columns = self.columns.len().max(1);
        let collapse = if self.collapse.on {
            " border-collapse"
        } else {
            ""
        };
        Some(format!(
            "{columns}x{}{collapse}",
            self.children.len().div_ceil(columns)
        ))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        grid_areas(cx, self, Rect::new(0, 0, width, height), axis)
            .iter()
            .map(|area| match axis {
                Axis::Vertical => area.bottom(),
                Axis::Horizontal => area.right(),
            })
            .max()
            .unwrap_or(0)
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        mark_collapsed(&self.children, self.collapse.on);
        let areas = grid_areas(cx, self, rect, Axis::Vertical);
        if !self.collapse.on {
            return areas;
        }
        let boxes = areas
            .into_iter()
            .enumerate()
            .filter(|(i, _)| !self.children[*i].hidden())
            .collect();
        self.collapse.settle(&self.children, boxes)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        if self.collapse.on {
            self.collapse.draw(&self.children, cx, canvas);
        }
    }

    fn retained(&self) -> bool {
        true
    }
}

/// Each child's rectangle in a grid. Columns are solved first, then rows,
/// so a row of [`Size::Auto`] knows how wide its children are. `axis`
/// is [`Axis::Horizontal`] when measuring the grid's own width, which
/// leaves flexible columns at their content.
fn grid_areas(cx: &MeasureCx, grid: &Grid, rect: Rect, axis: Axis) -> Vec<Rect> {
    // Children a stylesheet hides take no cell and get an empty rectangle.
    let children: Vec<&Node> = grid.children.iter().filter(|c| !c.hidden()).collect();
    let mut shown = shown_areas(cx, grid, &children, rect, axis).into_iter();
    grid.children
        .iter()
        .map(|c| match c.hidden() {
            true => Rect::default(),
            false => shown.next().unwrap_or_default(),
        })
        .collect()
}

fn shown_areas(
    cx: &MeasureCx,
    grid: &Grid,
    children: &[&Node],
    rect: Rect,
    axis: Axis,
) -> Vec<Rect> {
    let columns = grid.columns.len().max(1);
    let spans: Vec<(u16, u16)> = children.iter().map(|c| c.span).collect();
    let (places, rows) = place(columns, &spans);
    // Collapsed, neighbouring boxes share their lines: no gaps, and each
    // track overlaps the next by a cell.
    let collapsed = grid.collapse.on;
    let (row_gap, column_gap) = if collapsed { (0, 0) } else { grid.gap };
    let span_length = |sizes: &[u16], from: usize, n: usize, gap: u16| -> u16 {
        let sizes = &sizes[from..(from + n).min(sizes.len())];
        let sum = sizes.iter().fold(0u16, |sum, s| sum.saturating_add(*s));
        let between = n.saturating_sub(1) as u16;
        if collapsed {
            sum.saturating_sub(between)
        } else {
            sum.saturating_add(gap.saturating_mul(between))
        }
    };
    let starts = |start: u16, gap: u16, sizes: &[u16]| -> Vec<u16> {
        if !collapsed {
            return offsets(start, gap, sizes);
        }
        let mut at = start;
        sizes
            .iter()
            .map(|&size| {
                let here = at;
                at = at.saturating_add(size.saturating_sub(1));
                here
            })
            .collect()
    };
    let shared = |total: u16, n: usize| match collapsed {
        true => total.saturating_add(n.saturating_sub(1) as u16),
        false => total,
    };
    let measure = |child: &Node, axis: Axis, width: u16, height: u16| {
        let cells = cx.measure(child, axis, width, height);
        if collapsed {
            boxed(child, cells, true)
        } else {
            cells
        }
    };
    // Columns.
    let mut column_tracks: Vec<Track> = (0..columns)
        .map(|c| {
            let size = grid.columns.get(c).copied().unwrap_or(Size::Flex(1));
            let mut track = Track::new(size);
            let content = matches!(size, Size::Auto)
                || (axis == Axis::Horizontal && matches!(size, Size::Flex(_)));
            if content {
                track.size = Size::Auto;
                track.content = places
                    .iter()
                    .zip(children)
                    .filter(|(p, _)| p.column == c && p.columns == 1)
                    .map(|(_, child)| measure(child, Axis::Horizontal, rect.width, rect.height))
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    // Children spanning several columns grow the content columns they span.
    for (p, child) in places.iter().zip(children) {
        if p.columns > 1 {
            let need = measure(child, Axis::Horizontal, rect.width, rect.height)
                .saturating_add(if collapsed { p.columns as u16 - 1 } else { 0 });
            grow_for_span(
                &mut column_tracks,
                rect.width,
                column_gap,
                p.column,
                p.columns,
                need,
            );
        }
    }
    let widths = solve(shared(rect.width, columns), column_gap, &column_tracks);
    let xs = starts(rect.x, column_gap, &widths);
    // Rows.
    let row_size = |r: usize| match grid.rows.get(r).or(grid.rows.last()) {
        Some(size) => *size,
        None => Size::Flex(1),
    };
    let mut row_tracks: Vec<Track> = (0..rows)
        .map(|r| {
            let size = row_size(r);
            let mut track = Track::new(size);
            // A grid measured for its height sizes flexible rows by content.
            let content =
                matches!(size, Size::Auto) || (rect.height == 0 && matches!(size, Size::Flex(_)));
            if content {
                track.size = Size::Auto;
                track.content = places
                    .iter()
                    .zip(children)
                    .filter(|(p, _)| p.row == r && p.rows == 1)
                    .map(|(p, child)| {
                        let width = span_length(&widths, p.column, p.columns, column_gap);
                        measure(child, Axis::Vertical, width, 0)
                    })
                    .max()
                    .unwrap_or(0);
            }
            track
        })
        .collect();
    for (p, child) in places.iter().zip(children) {
        if p.rows > 1 {
            let width = span_length(&widths, p.column, p.columns, column_gap);
            let need = measure(child, Axis::Vertical, width, 0).saturating_add(if collapsed {
                p.rows as u16 - 1
            } else {
                0
            });
            grow_for_span(&mut row_tracks, rect.height, row_gap, p.row, p.rows, need);
        }
    }
    let total_height = if rect.height == 0 {
        u16::MAX
    } else {
        shared(rect.height, rows)
    };
    let heights = solve(total_height, row_gap, &row_tracks);
    let ys = starts(rect.y, row_gap, &heights);
    places
        .iter()
        .map(|p| {
            let width = span_length(&widths, p.column, p.columns, column_gap);
            let height = span_length(&heights, p.row, p.rows, row_gap);
            let area = Rect::new(xs[p.column], ys[p.row], width, height);
            if rect.height == 0 {
                area
            } else {
                area.intersection(rect)
            }
        })
        .collect()
}

/// A border with titles round one child: [`Node::panel`].
pub(crate) struct Panel {
    pub title: String,
    pub child: Node,
    /// Whether the border was last drawn as focused.
    shown_focus: Option<bool>,
    /// What a stylesheet says about it.
    pub look: PanelLook,
    /// What code says about it, which wins.
    pub code: PanelCode,
    /// In [error](crate::Node::error_when): its border in the theme's
    /// `border.error` style.
    pub invalid: bool,
    /// In a container that [collapses](crate::Node::collapse_borders) its
    /// children's borders: the container draws its frame and its titles,
    /// and the panel only holds its child, between the container's lines.
    pub collapsed: bool,
}

/// A stylesheet's border, titles and padding for a [`Panel`].
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelLook {
    pub border: Option<(Border, Style)>,
    /// `border: none`: no box, no titles, and no cells kept for either.
    pub no_border: bool,
    pub title: Option<String>,
    pub title_align: Option<Align>,
    pub subtitle: Option<String>,
    pub subtitle_align: Option<Align>,
    pub sides: Option<Sides>,
    /// Inside the border: top, right, bottom, left.
    pub padding: [u16; 4],
}

/// What code set on a [`Panel`]: [`Node::title_align`] and the rest.
#[derive(Clone, Debug, Default)]
pub(crate) struct PanelCode {
    pub title_align: Option<Align>,
    /// More titles, after the panel's own.
    pub titles: Vec<Title>,
    pub border: Option<Border>,
    pub sides: Option<Sides>,
    /// A style or a theme style's name, for the border while the focus is
    /// elsewhere, and for the titles.
    pub border_style: Option<String>,
    pub title_style: Option<String>,
    pub shadow: Option<Shadow>,
}

impl Panel {
    pub fn new(title: String, child: Node) -> Panel {
        Panel {
            title,
            child,
            shown_focus: None,
            look: PanelLook::default(),
            code: PanelCode::default(),
            invalid: false,
            collapsed: false,
        }
    }

    /// Its first title on the top edge, as plain text, shown or not: its
    /// accessible name.
    pub fn shown_title(&self) -> String {
        self.all_titles()
            .into_iter()
            .find(|title| title.edge == Edge::Top)
            .map(|title| plain(&title.markup))
            .unwrap_or_default()
    }

    /// The title: the one code gave, else the stylesheet's.
    fn title(&self) -> &str {
        match (&self.title[..], &self.look.title) {
            ("", Some(title)) => title,
            (title, _) => title,
        }
    }

    /// The kind of box: code's, else the stylesheet's, else rounded;
    /// `None` when a stylesheet took the frame away (`border: none`) and
    /// code set none.
    fn kind(&self) -> Option<Border> {
        match self.code.border {
            Some(kind) => Some(kind),
            None if self.look.no_border => None,
            None => Some(
                self.look
                    .border
                    .as_ref()
                    .map_or(Border::Rounded, |(kind, _)| *kind),
            ),
        }
    }

    fn sides(&self) -> Sides {
        self.code.sides.or(self.look.sides).unwrap_or_default()
    }

    /// Its titles: its own (`.panel`'s, else the stylesheet's
    /// `border-title`) and code's on the top edge, then the stylesheet's
    /// `border-subtitle` and code's on the bottom. None without a frame.
    pub fn titles(&self) -> Vec<Title> {
        match self.kind() {
            Some(_) => self.all_titles(),
            None => Vec::new(),
        }
    }

    /// [`titles`](Self::titles), with a frame or not.
    fn all_titles(&self) -> Vec<Title> {
        let mut titles = Vec::new();
        let own = self.title();
        if !own.is_empty() {
            let align = self.code.title_align.or(self.look.title_align);
            titles.push(Title::new(Edge::Top, align.unwrap_or(Align::Start), own));
        }
        let code = |edge| self.code.titles.iter().filter(move |t| t.edge == edge);
        titles.extend(code(Edge::Top).cloned());
        if let Some(subtitle) = self.look.subtitle.as_ref().filter(|s| !s.is_empty()) {
            let align = self.look.subtitle_align.unwrap_or(Align::Start);
            titles.push(Title::new(Edge::Bottom, align, subtitle.clone()));
        }
        titles.extend(code(Edge::Bottom).cloned());
        titles
    }

    /// The cells its frame takes, top, right, bottom and left, as it
    /// measures: a collapsed panel counts the lines it shares.
    fn insets(&self) -> [u16; 4] {
        if self.collapsed {
            return [1; 4];
        }
        match self.kind() {
            Some(kind) => crate::border::insets(kind, self.sides(), &self.titles()),
            None => [0; 4],
        }
    }

    /// The cells its shadow takes, right and below.
    fn shadow(&self) -> u16 {
        u16::from(self.code.shadow.is_some() && !self.collapsed && self.kind().is_some())
    }
}

/// `markup` as plain text.
pub(crate) fn plain(markup: &str) -> String {
    rich::Text::from_markup(markup)
        .map(|text| text.plain().to_string())
        .unwrap_or_else(|_| markup.to_string())
}

impl Widget for Panel {
    fn name(&self) -> &'static str {
        "panel"
    }

    fn role(&self) -> crate::a11y::Role {
        crate::a11y::Role::Region
    }

    fn describe(&self) -> Option<String> {
        let title = self.shown_title();
        (!title.is_empty()).then(|| format!("\"{title}\""))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let [top, right, bottom, left] = self.look.padding;
        let [frame_top, frame_right, frame_bottom, frame_left] = self.insets();
        let shadow = self.shadow();
        let across = left
            .saturating_add(right)
            .saturating_add(frame_left + frame_right + shadow);
        let down = top
            .saturating_add(bottom)
            .saturating_add(frame_top + frame_bottom + shadow);
        let height = if height == 0 {
            0
        } else {
            height.saturating_sub(down)
        };
        cx.measure(&self.child, axis, width.saturating_sub(across), height)
            .saturating_add(match axis {
                Axis::Vertical => down,
                Axis::Horizontal => across,
            })
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let shadow = self.shadow();
        let rect = Rect::new(
            rect.x,
            rect.y,
            rect.width.saturating_sub(shadow),
            rect.height.saturating_sub(shadow),
        );
        // Collapsed, the container's lines are outside the panel.
        let [frame_top, frame_right, frame_bottom, frame_left] = match self.collapsed {
            true => [0; 4],
            false => self.insets(),
        };
        let [top, right, bottom, left] = self.look.padding;
        vec![rect.inset(
            top.saturating_add(frame_top),
            right.saturating_add(frame_right),
            bottom.saturating_add(frame_bottom),
            left.saturating_add(frame_left),
        )]
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let Some(kind) = self.kind().filter(|_| !self.collapsed) else {
            return;
        };
        // The focus is all a panel reads: when it moves elsewhere in the
        // app, the panel stays as it is, and only its edges change colour
        // when it comes or goes.
        let focused = cx.focus_within();
        if cx.repaint() || self.shown_focus != Some(focused) {
            // Code's, else a stylesheet's, colour while the focus is
            // elsewhere.
            let sheet = self.look.border.as_ref().map(|(_, style)| style);
            let mut style = match (&self.code.border_style, sheet) {
                (Some(name), _) if !focused => cx.style(name, ""),
                (None, Some(style)) if !focused && !style.is_null() => style.clone(),
                _ => cx.theme().border(focused),
            };
            if self.invalid {
                style = style.combine(&cx.theme().error_style("border.error"));
            }
            let title = match &self.code.title_style {
                Some(name) => cx.style(name, ""),
                None => cx.theme().title.clone(),
            };
            let title_style = style.combine(&title);
            let titles = self.titles();
            let frame = crate::border::Frame {
                border: kind,
                sides: self.sides(),
                style: &style,
                title_style: &title_style,
                titles: &titles,
            };
            let shadow = match self.code.shadow {
                Some(shadow) if cx.repaint() => {
                    Some((shadow, crate::border::shadow_style(cx.console(), shadow)))
                }
                _ => None,
            };
            canvas.frame(cx.console(), &frame, self.shadow() == 1, shadow);
        }
        self.shown_focus = Some(focused);
    }

    fn retained(&self) -> bool {
        true
    }
}

/// Console markup, with how its lines sit, wrap and are cut:
/// [`text`](crate::text) and [`label`](crate::label).
pub(crate) struct TextNode {
    pub markup: Box<dyn Fn() -> String>,
    pub align: Option<TextAlign>,
    pub no_wrap: Option<bool>,
    pub overflow: Option<Overflow>,
    /// Which of the three code set, which a stylesheet leaves alone.
    pub code: [bool; 3],
}

impl TextNode {
    pub fn new(markup: Box<dyn Fn() -> String>) -> TextNode {
        TextNode {
            markup,
            align: None,
            no_wrap: None,
            overflow: None,
            code: [false; 3],
        }
    }

    /// Its lines at `width`, aligned or not: alignment pads lines out to
    /// the width, and a text's natural width is its lines' own.
    fn lines(&self, console: &Console, width: u16, aligned: bool) -> Vec<Vec<Segment>> {
        let markup = (self.markup)();
        let mut text = rich::Text::from_markup(&markup).unwrap_or_else(|_| rich::Text::new(markup));
        let justify = match self.align {
            Some(TextAlign::Center) => Some(rich::Justify::Center),
            Some(TextAlign::Right) => Some(rich::Justify::Right),
            Some(TextAlign::Justify) => Some(rich::Justify::Full),
            Some(TextAlign::Left) | None => None,
        };
        if let (true, Some(justify)) = (aligned, justify) {
            text.set_justify(justify);
        }
        text.set_no_wrap(self.no_wrap);
        text.set_overflow(self.overflow);
        let options = console.options().update_width(width.max(1) as usize);
        console.render_lines(&text, &options, false)
    }
}

impl Widget for TextNode {
    fn name(&self) -> &'static str {
        "text"
    }

    fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        match self.align {
            Some(TextAlign::Center) => parts.push("text-align:center"),
            Some(TextAlign::Right) => parts.push("text-align:right"),
            Some(TextAlign::Justify) => parts.push("text-align:justify"),
            _ => {}
        }
        if self.no_wrap == Some(true) {
            parts.push("nowrap");
        }
        match self.overflow {
            Some(Overflow::Ellipsis) => parts.push("text-overflow:ellipsis"),
            Some(Overflow::Crop) => parts.push("text-overflow:clip"),
            _ => {}
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        extent(&self.lines(cx.console(), width.max(1), false), axis)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let lines = self.lines(cx.console(), canvas.width(), true);
        canvas.lines(&lines);
    }
}

/// Children stacked on one rectangle, later ones over earlier ones:
/// [`layers`](crate::layers).
pub(crate) struct Layers {
    pub children: Vec<Node>,
}

impl Layers {
    /// Where `child` goes in `rect`: inside its inset, the whole of it or,
    /// placed, as big as its content.
    fn place(cx: &MeasureCx, child: &Node, rect: Rect) -> Rect {
        let rect = match child.inset {
            Some([top, right, bottom, left]) => rect.inset(top, right, bottom, left),
            None => rect,
        };
        let Some((across, down)) = child.place else {
            return rect;
        };
        let width = match across {
            Align::Stretch => rect.width,
            _ => cx
                .measure(child, Axis::Horizontal, rect.width, rect.height)
                .min(rect.width),
        };
        let height = match down {
            Align::Stretch => rect.height,
            _ => cx.measure(child, Axis::Vertical, width, 0).min(rect.height),
        };
        let (x, width) = crate::layout::align_cross(rect.x, rect.width, width, across);
        let (y, height) = crate::layout::align_cross(rect.y, rect.height, height, down);
        Rect::new(x, y, width, height)
    }
}

impl Widget for Layers {
    fn name(&self) -> &'static str {
        "layers"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("[{}]", self.children.len()))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        // As big as the biggest layer, with its inset.
        self.children
            .iter()
            .filter(|child| !child.hidden())
            .map(|child| {
                let [top, right, bottom, left] = child.inset.unwrap_or_default();
                let (across, down) = (left.saturating_add(right), top.saturating_add(bottom));
                let inner = cx.measure(
                    child,
                    axis,
                    width.saturating_sub(across),
                    if height == 0 {
                        0
                    } else {
                        height.saturating_sub(down)
                    },
                );
                inner.saturating_add(match axis {
                    Axis::Vertical => down,
                    Axis::Horizontal => across,
                })
            })
            .max()
            .unwrap_or(0)
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.children
            .iter()
            .map(|child| match child.hidden() {
                true => Rect::default(),
                false => Layers::place(cx, child, rect),
            })
            .collect()
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }

    fn overlapping(&self) -> bool {
        true
    }
}

/// Space round one child: [`Node::padding`].
pub(crate) struct Pad {
    pub child: Node,
    /// Top, right, bottom, left.
    pub edges: [u16; 4],
}

impl Widget for Pad {
    fn name(&self) -> &'static str {
        "padding"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let [top, right, bottom, left] = self.edges;
        let (across, down) = (left.saturating_add(right), top.saturating_add(bottom));
        let inner = cx.measure(
            &self.child,
            axis,
            width.saturating_sub(across),
            height.saturating_sub(down),
        );
        match axis {
            Axis::Vertical => inner.saturating_add(down),
            Axis::Horizontal => inner.saturating_add(across),
        }
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        let [top, right, bottom, left] = self.edges;
        vec![Rect::new(
            rect.x.saturating_add(left),
            rect.y.saturating_add(top),
            rect.width.saturating_sub(left.saturating_add(right)),
            rect.height.saturating_sub(top.saturating_add(bottom)),
        )]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// A row's height in an [`each`](crate::each) list: its fixed size, its
/// content for [`Size::Auto`], otherwise one row; none while it is hidden.
fn row_height(cx: &MeasureCx, child: &Node, width: u16) -> u16 {
    if child.hidden() {
        return 0;
    }
    let height = match child.size {
        Size::Fixed(n) => n,
        Size::Auto => cx.measure(child, Axis::Vertical, width, 0),
        _ => 1,
    };
    height.min(child.max).max(child.min)
}

/// One child per key, kept by key: [`each`](crate::each).
pub(crate) struct Each<K, F> {
    pub keys: Rc<dyn Fn() -> Vec<K>>,
    pub build: F,
    pub order: Vec<K>,
    pub children: Vec<Node>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Each<K, F> {
    /// Bring the children in line with the keys.
    fn reconcile(&mut self) {
        let keys = (self.keys)();
        if keys == self.order {
            return;
        }
        let mut old: HashMap<K, Node> = self.order.drain(..).zip(self.children.drain(..)).collect();
        let runtime = Runtime::current();
        for key in &keys {
            // Built untracked: what a new row reads is the row's, not the
            // list's.
            let node = old
                .remove(key)
                .unwrap_or_else(|| runtime.untracked(|| (self.build)(key.clone())));
            self.children.push(node);
        }
        for (_, gone) in old {
            gone.forget(&runtime);
        }
        self.order = keys;
    }
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node + 'static> Widget for Each<K, F> {
    fn name(&self) -> &'static str {
        "each"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("[{}]", self.children.len()))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        self.reconcile();
        match axis {
            Axis::Vertical => self.children.iter().fold(0u16, |sum, child| {
                sum.saturating_add(row_height(cx, child, width))
            }),
            Axis::Horizontal => self
                .children
                .iter()
                .map(|child| cx.measure(child, axis, width, height))
                .max()
                .unwrap_or(0),
        }
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.reconcile();
        let mut y = rect.y;
        self.children
            .iter()
            .map(|child| {
                if y >= rect.bottom() {
                    // Below the viewport: not drawn, so it draws afresh
                    // whenever it comes into view.
                    return Rect::default();
                }
                let height = row_height(cx, child, rect.width).min(rect.bottom() - y);
                let area = Rect::new(rect.x, y, rect.width, height);
                y += height;
                area
            })
            .collect()
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// One child at a time, chosen by a key, the others kept:
/// [`switch`](crate::switch).
pub(crate) struct Switch<K, F> {
    pub key: Box<dyn Fn() -> K>,
    pub build: F,
    pub built: HashMap<K, Node>,
    pub current: Option<K>,
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node> Switch<K, F> {
    /// Read the key (subscribing to what it reads), and build its child,
    /// untracked, if it is new.
    fn settle(&mut self) {
        let key = (self.key)();
        if !self.built.contains_key(&key) {
            let node = Runtime::current().untracked(|| (self.build)(key.clone()));
            self.built.insert(key.clone(), node);
        }
        self.current = Some(key);
    }

    fn shown(&self) -> Option<&Node> {
        self.current.as_ref().and_then(|key| self.built.get(key))
    }
}

impl<K: Clone + Eq + Hash + 'static, F: Fn(K) -> Node + 'static> Widget for Switch<K, F> {
    fn name(&self) -> &'static str {
        "switch"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        self.settle();
        self.shown()
            .map_or(0, |child| cx.measure(child, axis, width, height))
    }

    fn children(&self) -> &[Node] {
        self.shown().map(std::slice::from_ref).unwrap_or(&[])
    }

    fn hidden_children(&self) -> Vec<&Node> {
        let shown = self.shown().map(|node| node.id());
        self.built
            .values()
            .filter(|node| Some(node.id()) != shown)
            .collect()
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.settle();
        vec![rect]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// A `rich-interact` component living in the tree.
pub(crate) trait Hosted {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used;
    fn render(&self, context: &Context<'_>) -> rich_interact::View;
    fn set_cancel(&mut self, cancel: Handler);
}

type OnDone<T> = Box<dyn FnMut(T, &mut Ctx)>;

pub(crate) struct Host<C: Component> {
    pub component: C,
    /// Makes a fresh component after each answer ([`repeating`](crate::repeating)).
    pub make: Option<Box<dyn Fn() -> C>>,
    pub on_done: OnDone<C::Output>,
    pub on_cancel: Option<Handler>,
}

impl<C: Component> Hosted for Host<C> {
    fn handle(&mut self, event: &Event, context: &Context<'_>, cx: &mut Ctx) -> Used {
        match self.component.handle(event, context) {
            Flow::Ignored => Used::No,
            Flow::Continue => Used::Yes,
            Flow::Done(value) => {
                if let Some(make) = &self.make {
                    self.component = make();
                }
                (self.on_done)(value, cx);
                Used::Yes
            }
            // With no cancel handler, the key that cancelled (Esc) is the
            // app's: it bubbles, so an app-wide Esc binding still works.
            Flow::Cancel => match &mut self.on_cancel {
                Some(cancel) => {
                    cancel(cx);
                    Used::Yes
                }
                None => Used::No,
            },
            // Handing the terminal to another program is not supported
            // inside an app yet; the component carries on.
            Flow::Handoff(_) => Used::Yes,
        }
    }

    fn render(&self, context: &Context<'_>) -> rich_interact::View {
        self.component.render(context)
    }

    fn set_cancel(&mut self, cancel: Handler) {
        self.on_cancel = Some(cancel);
    }
}

/// A hosted component as a widget: [`component`](crate::component).
pub(crate) struct HostWidget {
    pub host: Box<dyn Hosted>,
    caret: Option<(u16, u16)>,
    role: crate::a11y::Role,
}

impl HostWidget {
    /// `C` hosted: an `Input` or a `TextArea` is a text box.
    pub fn new<C: Component + 'static>(host: Box<dyn Hosted>) -> HostWidget {
        use std::any::TypeId;
        let text = [
            TypeId::of::<rich_interact::Input>(),
            TypeId::of::<rich_interact::TextArea>(),
        ];
        let role = if text.contains(&TypeId::of::<C>()) {
            crate::a11y::Role::TextBox
        } else {
            crate::a11y::Role::Group
        };
        HostWidget {
            host,
            caret: None,
            role,
        }
    }
}

impl Widget for HostWidget {
    fn name(&self) -> &'static str {
        "component"
    }

    fn role(&self) -> crate::a11y::Role {
        self.role
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        let context = Context {
            console: cx.console(),
            width: width.max(1) as usize,
            height: if height == 0 {
                u16::MAX as usize
            } else {
                height as usize
            },
        };
        extent(&self.host.render(&context).lines, axis)
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let (width, height) = (canvas.width(), canvas.height());
        let context = Context {
            console: cx.console(),
            width: width as usize,
            height: height as usize,
        };
        // A component that reads signals while it renders (a ratatui
        // widget drawing app state) draws again when they change.
        let view = self.host.render(&context);
        canvas.lines(&view.lines);
        self.caret = view.cursor.and_then(|(row, column)| {
            let (row, column) = (row as u16, column as u16);
            (row < height && column < width).then_some((column, row))
        });
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        let event = match event {
            WidgetEvent::Key(key) => Event::Key(*key),
            WidgetEvent::KeyUp(key) => Event::KeyUp(*key),
            WidgetEvent::Paste(text) => Event::Paste(text.clone()),
            // A component sees the mouse only while it has the focus (a
            // press focuses it first).
            WidgetEvent::Mouse(mouse) if cx.focused() => Event::Mouse(*mouse),
            _ => return Used::No,
        };
        let console = cx.console().clone();
        let (width, height) = cx.size();
        let context = Context {
            console: &console,
            width: width as usize,
            height: height as usize,
        };
        let used = self.host.handle(&event, &context, cx.app());
        if used == Used::Yes {
            cx.redraw();
        }
        used
    }

    fn focusable(&self) -> bool {
        true
    }

    fn caret(&self) -> Option<(u16, u16)> {
        self.caret
    }
}

/// A viewport onto one child laid out at its full height:
/// [`scroll`](crate::scroll).
pub(crate) struct ScrollView {
    pub child: Node,
    /// The first row in view.
    pub offset: Signal<u16>,
    /// The first column in view.
    pub x_offset: Signal<u16>,
    /// Which ways it scrolls: down (the content is as wide as the view and
    /// as tall as it needs) and across (as wide as it needs).
    vertical: bool,
    horizontal: bool,
    /// The content's width and height.
    content: (u16, u16),
    /// The view's columns and rows, less the scrollbars.
    view: (u16, u16),
    /// The focused node inside when it was last scrolled into view.
    focus: Option<NodeId>,
    /// The bar down and the bar across, and where each was last drawn.
    down: crate::scrollbar::Bar,
    across_bar: crate::scrollbar::Bar,
    bars: (Option<Rect>, Option<Rect>),
    /// Its own size, as last laid out.
    size: (u16, u16),
}

impl ScrollView {
    pub fn new(child: Node, offset: Signal<u16>) -> ScrollView {
        ScrollView::both(child, crate::reactive::signal(0), offset, false, true)
    }

    pub fn both(
        child: Node,
        x_offset: Signal<u16>,
        offset: Signal<u16>,
        horizontal: bool,
        vertical: bool,
    ) -> ScrollView {
        ScrollView {
            child,
            offset,
            x_offset,
            vertical,
            horizontal,
            content: (0, 0),
            view: (1, 1),
            focus: None,
            down: crate::scrollbar::Bar::new(Some(crate::scrollbar::Scrollbar::new())),
            across_bar: crate::scrollbar::Bar::new(Some(
                crate::scrollbar::Scrollbar::new().side(crate::scrollbar::Side::Bottom),
            )),
            bars: (None, None),
            size: (0, 0),
        }
    }

    /// Draw the bar down (a bar on the left or right) or the bar across (on
    /// the top or bottom) as `bar` says.
    pub(crate) fn set_bar(&mut self, bar: crate::scrollbar::Scrollbar) {
        if bar.is_vertical() {
            self.down = crate::scrollbar::Bar::new(Some(bar));
        } else {
            self.across_bar = crate::scrollbar::Bar::new(Some(bar));
        }
    }

    fn down_look(&self) -> &crate::scrollbar::Scrollbar {
        self.down.look.as_ref().expect("a scroll has a bar down")
    }

    fn across_look(&self) -> &crate::scrollbar::Scrollbar {
        self.across_bar
            .look
            .as_ref()
            .expect("a scroll has a bar across")
    }

    /// Where the window starts: after a bar on the left or the top.
    fn window_origin(&self, width: u16, height: u16) -> (u16, u16) {
        use crate::scrollbar::Side;
        let x =
            u16::from(self.vertical && self.view.0 < width && self.down_look().side == Side::Left);
        let y = u16::from(
            self.horizontal && self.view.1 < height && self.across_look().side == Side::Top,
        );
        (x, y)
    }

    fn by(&self, rows: i32) {
        let max = self.content.1.saturating_sub(self.view.1) as i32;
        self.offset
            .update(|o| *o = (*o as i32 + rows).clamp(0, max.max(0)) as u16);
    }

    fn across(&self, columns: i32) {
        let max = self.content.0.saturating_sub(self.view.0) as i32;
        self.x_offset
            .update(|o| *o = (*o as i32 + columns).clamp(0, max.max(0)) as u16);
    }

    fn top(&self) -> u16 {
        self.offset
            .get_untracked()
            .min(self.content.1.saturating_sub(self.view.1))
    }

    fn left(&self) -> u16 {
        self.x_offset
            .get_untracked()
            .min(self.content.0.saturating_sub(self.view.0))
    }
}

/// The first of `start..start + len` to show so that `from..to` is in
/// view: unchanged if it already is.
fn into_view(start: u16, len: u16, from: u16, to: u16) -> u16 {
    if from < start {
        from
    } else if to > start.saturating_add(len) {
        to.saturating_sub(len)
    } else {
        start
    }
}

impl Widget for ScrollView {
    fn name(&self) -> &'static str {
        "scroll"
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        cx.measure(&self.child, axis, width, height)
    }

    fn children(&self) -> &[Node] {
        std::slice::from_ref(&self.child)
    }

    fn viewport(&self) -> bool {
        true
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        // Across, the content is as wide as it asks (at most
        // MAX_SCROLL_COLUMNS); down, as tall as it needs at that width.
        // Each scrollbar takes a line when the content overflows that way:
        // the last column for down, the last row for across.
        let wide = |width: u16| {
            if self.horizontal {
                // A viewport wider than the cap is the content's width.
                cx.measure(&self.child, Axis::Horizontal, MAX_SCROLL_COLUMNS, 0)
                    .min(MAX_SCROLL_COLUMNS)
                    .max(width)
            } else {
                width
            }
        };
        let tall = |width: u16| {
            if self.vertical {
                cx.measure(&self.child, Axis::Vertical, width, 0)
            } else {
                rect.height
            }
        };
        let (mut view_w, mut view_h) = (rect.width, rect.height);
        let mut w = wide(view_w);
        let mut h = tall(w);
        // The two bars can each make room the other needs: settle in two
        // passes.
        let (down, across) = (self.down_look().clone(), self.across_look().clone());
        for _ in 0..2 {
            if self.vertical && down.wanted(h as usize, view_h as usize) && rect.width > 1 {
                view_w = rect.width - 1;
            }
            if self.horizontal && across.wanted(w as usize, view_w as usize) && rect.height > 1 {
                view_h = rect.height - 1;
            }
            w = wide(view_w);
            h = tall(w);
        }
        self.offset.get();
        self.x_offset.get();
        self.view = (view_w, view_h);
        self.size = (rect.width, rect.height);
        self.content = (
            w.max(view_w),
            if self.vertical {
                h.max(view_h).min(MAX_SCROLL_ROWS)
            } else {
                view_h
            },
        );
        vec![Rect::new(0, 0, self.content.0, self.content.1)]
    }

    fn scroll(&mut self, cx: &ScrollCx) -> (Rect, (u16, u16)) {
        // Keep a newly focused node inside in view, once it is laid out.
        let focused = cx.focused();
        if focused.map(|(id, _)| id) != self.focus {
            self.focus = focused.map(|(id, _)| id);
            if let Some((_, area)) = focused.filter(|(_, area)| !area.is_empty()) {
                let (left, top) = (self.left(), self.top());
                let next_top = into_view(top, self.view.1, area.y, area.bottom());
                let next_left = into_view(left, self.view.0, area.x, area.right());
                if self.vertical && next_top != top {
                    Runtime::current().untracked(|| self.offset.set(next_top));
                }
                if self.horizontal && next_left != left {
                    Runtime::current().untracked(|| self.x_offset.set(next_left));
                }
            }
        }
        let (x, y) = self.window_origin(self.size.0, self.size.1);
        (
            Rect::new(x, y, self.view.0, self.view.1),
            (self.left(), self.top()),
        )
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        use crate::scrollbar::Side;
        // The focus moving inside may scroll the view.
        cx.focus_within();
        let (content_w, content_h) = self.content;
        let (view_w, view_h) = self.view;
        let (width, height) = (canvas.width(), canvas.height());
        let (x, y) = self.window_origin(width, height);
        self.bars = (None, None);
        if self.vertical && view_w < width {
            let look = self.down_look().clone();
            let column = if look.side == Side::Left {
                0
            } else {
                width - 1
            };
            let rect = Rect::new(column, y, 1, view_h);
            let (top, content) = (self.top() as usize, content_h as usize);
            look.paint(cx.console(), canvas, rect, top, content, view_h as usize);
            self.bars.0 = Some(rect);
        }
        if self.horizontal && view_h < height {
            let look = self.across_look().clone();
            let line = if look.side == Side::Top {
                0
            } else {
                height - 1
            };
            let rect = Rect::new(x, line, view_w, 1);
            let (left, content) = (self.left() as usize, content_w as usize);
            look.paint(cx.console(), canvas, rect, left, content, view_w as usize);
            self.bars.1 = Some(rect);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        // A press or a drag on a bar scrolls by it.
        if let WidgetEvent::Mouse(mouse) = event {
            let (content_w, content_h) = self.content;
            let (view_w, view_h) = self.view;
            if let Some(rect) = self.bars.0 {
                let (top, content) = (self.top() as usize, content_h as usize);
                match self
                    .down
                    .mouse(cx, mouse, rect, top, content, view_h as usize)
                {
                    Some(Some(to)) => {
                        self.offset.set(to.min(u16::MAX as usize) as u16);
                        return Used::Yes;
                    }
                    Some(None) => return Used::Yes,
                    None => {}
                }
            }
            if let Some(rect) = self.bars.1 {
                let (left, content) = (self.left() as usize, content_w as usize);
                match self
                    .across_bar
                    .mouse(cx, mouse, rect, left, content, view_w as usize)
                {
                    Some(Some(to)) => {
                        self.x_offset.set(to.min(u16::MAX as usize) as u16);
                        return Used::Yes;
                    }
                    Some(None) => return Used::Yes,
                    None => {}
                }
            }
        }
        let page = self.view.1.max(2) as i32 - 1;
        let page_across = self.view.0.max(2) as i32 - 1;
        match event {
            WidgetEvent::Key(key) if key.modifiers == rich_interact::Modifiers::NONE => {
                match key.code {
                    KeyCode::Up if self.vertical => self.by(-1),
                    KeyCode::Down if self.vertical => self.by(1),
                    KeyCode::PageUp if self.vertical => self.by(-page),
                    KeyCode::PageDown if self.vertical => self.by(page),
                    KeyCode::Left if self.horizontal => self.across(-1),
                    KeyCode::Right if self.horizontal => self.across(1),
                    KeyCode::Home => {
                        self.offset.set(0);
                        self.x_offset.set(0);
                    }
                    KeyCode::End if self.vertical => self.offset.set(MAX_SCROLL_ROWS),
                    KeyCode::End => self.x_offset.set(MAX_SCROLL_COLUMNS),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Key(key)
                if key.modifiers.shift
                    && !key.modifiers.ctrl
                    && !key.modifiers.alt
                    && self.horizontal =>
            {
                match key.code {
                    KeyCode::PageUp => self.across(-page_across),
                    KeyCode::PageDown => self.across(page_across),
                    _ => return Used::No,
                }
            }
            WidgetEvent::Mouse(mouse) => {
                // Shift with the wheel, or the wheel in a view that only
                // scrolls across, scrolls across.
                let across = self.horizontal && (mouse.modifiers.shift || !self.vertical);
                match (mouse.kind, across) {
                    (MouseKind::ScrollUp, false) => self.by(-3),
                    (MouseKind::ScrollDown, false) => self.by(3),
                    (MouseKind::ScrollUp, true) => self.across(-6),
                    (MouseKind::ScrollDown, true) => self.across(6),
                    _ => return Used::No,
                }
            }
            _ => return Used::No,
        }
        Used::Yes
    }

    fn focusable(&self) -> bool {
        true
    }
}

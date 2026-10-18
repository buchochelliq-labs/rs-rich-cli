//! Borders: the kinds of box a [panel](crate::Node::panel) draws, the sides
//! it draws, titles on its edges and its shadow; and the table of glyphs
//! that join lines where they meet, for
//! [collapsed borders](crate::Node::collapse_borders).
//!
//! ```
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     label("3 open, 1 merged")
//!         .panel("[b]Pull requests[/]")
//!         .title_align(Align::Center)
//!         .title_bottom("[dim]q quits")
//!         .border(Border::Thick)
//!         .borders(Sides::TOP | Sides::BOTTOM)
//! });
//! let mut driver = app.driver(25, 3);
//! driver.settle();
//! assert_eq!(driver.screen().plain()[0], "━━━━━ Pull requests ━━━━━");
//! assert_eq!(driver.screen().plain()[1].trim_end(), "3 open, 1 merged");
//! assert_eq!(driver.screen().plain()[2], "━━ q quits ━━━━━━━━━━━━━━");
//! ```

use std::collections::HashMap;
use std::fmt;
use std::ops::{BitOr, BitOrAssign};

use rich::{Console, Segment, Style};

use crate::layout::Align;
use crate::screen::{Rect, Screen};

/// The glyphs of a box: its four corners and its four sides.
///
/// ```
/// use intuituive::border::{Border, BorderSet};
///
/// // Plain corners with a wavy top and bottom.
/// let wavy = Border::Custom(BorderSet { top: "~", bottom: "~", ..BorderSet::PLAIN });
/// assert_eq!(wavy.set().unwrap().top_left, "┌");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BorderSet {
    pub top_left: &'static str,
    pub top: &'static str,
    pub top_right: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub bottom_left: &'static str,
    pub bottom: &'static str,
    pub bottom_right: &'static str,
}

impl BorderSet {
    const fn lines(
        corners: [&'static str; 4],
        horizontal: &'static str,
        vertical: &'static str,
    ) -> BorderSet {
        let [top_left, top_right, bottom_left, bottom_right] = corners;
        BorderSet {
            top_left,
            top: horizontal,
            top_right,
            left: vertical,
            right: vertical,
            bottom_left,
            bottom: horizontal,
            bottom_right,
        }
    }

    pub const ROUNDED: BorderSet = BorderSet::lines(["╭", "╮", "╰", "╯"], "─", "│");
    pub const PLAIN: BorderSet = BorderSet::lines(["┌", "┐", "└", "┘"], "─", "│");
    pub const THICK: BorderSet = BorderSet::lines(["┏", "┓", "┗", "┛"], "━", "┃");
    pub const DOUBLE: BorderSet = BorderSet::lines(["╔", "╗", "╚", "╝"], "═", "║");
    pub const ASCII: BorderSet = BorderSet::lines(["+", "+", "+", "+"], "-", "|");
    pub const LIGHT_DOUBLE_DASHED: BorderSet = BorderSet::lines(["┌", "┐", "└", "┘"], "╌", "╎");
    pub const HEAVY_DOUBLE_DASHED: BorderSet = BorderSet::lines(["┏", "┓", "┗", "┛"], "╍", "╏");
    pub const LIGHT_TRIPLE_DASHED: BorderSet = BorderSet::lines(["┌", "┐", "└", "┘"], "┄", "┆");
    pub const HEAVY_TRIPLE_DASHED: BorderSet = BorderSet::lines(["┏", "┓", "┗", "┛"], "┅", "┇");
    pub const LIGHT_QUADRUPLE_DASHED: BorderSet = BorderSet::lines(["┌", "┐", "└", "┘"], "┈", "┊");
    pub const HEAVY_QUADRUPLE_DASHED: BorderSet = BorderSet::lines(["┏", "┓", "┗", "┛"], "┉", "┋");
    /// Half blocks on the inside of the box's cells.
    pub const QUADRANT_INSIDE: BorderSet = BorderSet {
        top_left: "▗",
        top: "▄",
        top_right: "▖",
        left: "▐",
        right: "▌",
        bottom_left: "▝",
        bottom: "▀",
        bottom_right: "▘",
    };
    /// Half blocks on the outside of the box's cells.
    pub const QUADRANT_OUTSIDE: BorderSet = BorderSet {
        top_left: "▛",
        top: "▀",
        top_right: "▜",
        left: "▌",
        right: "▐",
        bottom_left: "▙",
        bottom: "▄",
        bottom_right: "▟",
    };
}

/// The kind of box a panel is drawn with: ratatui's `BorderType`.
/// [`Rounded`](Self::Rounded) is the default; [`None`](Self::None) draws no
/// lines, but a panel's titles keep their rows.
///
/// A stylesheet's `border` names them: `round`, `square` (or `plain`),
/// `heavy` (or `thick`), `double`, `ascii`, `light-double-dashed` to
/// `heavy-quadruple-dashed` (`dashed` is the light double one),
/// `quadrant-inside` and `quadrant-outside`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Border {
    #[default]
    Rounded,
    Plain,
    Thick,
    Double,
    Ascii,
    LightDoubleDashed,
    HeavyDoubleDashed,
    LightTripleDashed,
    HeavyTripleDashed,
    LightQuadrupleDashed,
    HeavyQuadrupleDashed,
    QuadrantInside,
    QuadrantOutside,
    /// Glyphs of your own.
    Custom(BorderSet),
    /// No lines.
    None,
}

impl Border {
    /// Its glyphs; `None` for [`Border::None`].
    pub fn set(self) -> Option<BorderSet> {
        Some(match self {
            Border::Rounded => BorderSet::ROUNDED,
            Border::Plain => BorderSet::PLAIN,
            Border::Thick => BorderSet::THICK,
            Border::Double => BorderSet::DOUBLE,
            Border::Ascii => BorderSet::ASCII,
            Border::LightDoubleDashed => BorderSet::LIGHT_DOUBLE_DASHED,
            Border::HeavyDoubleDashed => BorderSet::HEAVY_DOUBLE_DASHED,
            Border::LightTripleDashed => BorderSet::LIGHT_TRIPLE_DASHED,
            Border::HeavyTripleDashed => BorderSet::HEAVY_TRIPLE_DASHED,
            Border::LightQuadrupleDashed => BorderSet::LIGHT_QUADRUPLE_DASHED,
            Border::HeavyQuadrupleDashed => BorderSet::HEAVY_QUADRUPLE_DASHED,
            Border::QuadrantInside => BorderSet::QUADRANT_INSIDE,
            Border::QuadrantOutside => BorderSet::QUADRANT_OUTSIDE,
            Border::Custom(set) => set,
            Border::None => return None,
        })
    }

    /// Its name in a stylesheet (and the inspector).
    pub(crate) fn css(self) -> &'static str {
        match self {
            Border::Rounded => "round",
            Border::Plain => "square",
            Border::Thick => "heavy",
            Border::Double => "double",
            Border::Ascii => "ascii",
            Border::LightDoubleDashed => "light-double-dashed",
            Border::HeavyDoubleDashed => "heavy-double-dashed",
            Border::LightTripleDashed => "light-triple-dashed",
            Border::HeavyTripleDashed => "heavy-triple-dashed",
            Border::LightQuadrupleDashed => "light-quadruple-dashed",
            Border::HeavyQuadrupleDashed => "heavy-quadruple-dashed",
            Border::QuadrantInside => "quadrant-inside",
            Border::QuadrantOutside => "quadrant-outside",
            Border::Custom(_) => "custom",
            Border::None => "none",
        }
    }

    /// The kind a stylesheet's `border` names (not `none`, which is a
    /// panel's whole frame taken away).
    pub(crate) fn parse(name: &str) -> Option<Border> {
        Some(match name {
            "round" | "rounded" => Border::Rounded,
            "square" | "solid" | "plain" => Border::Plain,
            "heavy" | "thick" => Border::Thick,
            "double" => Border::Double,
            "ascii" => Border::Ascii,
            "dashed" | "light-double-dashed" => Border::LightDoubleDashed,
            "heavy-double-dashed" => Border::HeavyDoubleDashed,
            "light-triple-dashed" => Border::LightTripleDashed,
            "heavy-triple-dashed" => Border::HeavyTripleDashed,
            "dotted" | "light-quadruple-dashed" => Border::LightQuadrupleDashed,
            "heavy-quadruple-dashed" => Border::HeavyQuadrupleDashed,
            "quadrant-inside" | "inner" => Border::QuadrantInside,
            "quadrant-outside" | "outer" => Border::QuadrantOutside,
            _ => return None,
        })
    }

    /// How heavy its lines are where they join others: 1 light, 2 heavy,
    /// 3 double.
    pub(crate) fn weight(self) -> u8 {
        match self {
            Border::Thick
            | Border::HeavyDoubleDashed
            | Border::HeavyTripleDashed
            | Border::HeavyQuadrupleDashed => HEAVY,
            Border::Double => DOUBLE,
            _ => LIGHT,
        }
    }
}

/// Which sides of a box are drawn: [`Sides::ALL`] (the default), or any of
/// [`TOP`](Self::TOP), [`RIGHT`](Self::RIGHT), [`BOTTOM`](Self::BOTTOM) and
/// [`LEFT`](Self::LEFT) joined with `|`: ratatui's `Borders`.
///
/// ```
/// use intuituive::Sides;
///
/// let sides = Sides::TOP | Sides::LEFT;
/// assert!(sides.contains(Sides::TOP) && !sides.contains(Sides::RIGHT));
/// assert_eq!(format!("{sides:?}"), "Sides(TOP | LEFT)");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Sides(u8);

impl Sides {
    pub const NONE: Sides = Sides(0);
    pub const TOP: Sides = Sides(1);
    pub const RIGHT: Sides = Sides(2);
    pub const BOTTOM: Sides = Sides(4);
    pub const LEFT: Sides = Sides(8);
    pub const ALL: Sides = Sides(15);

    /// Whether every side of `other` is in this.
    pub const fn contains(self, other: Sides) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no side is.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl Default for Sides {
    fn default() -> Sides {
        Sides::ALL
    }
}

impl BitOr for Sides {
    type Output = Sides;

    fn bitor(self, other: Sides) -> Sides {
        Sides(self.0 | other.0)
    }
}

impl BitOrAssign for Sides {
    fn bitor_assign(&mut self, other: Sides) {
        self.0 |= other.0;
    }
}

impl fmt::Debug for Sides {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&str> = [
            (Sides::TOP, "TOP"),
            (Sides::RIGHT, "RIGHT"),
            (Sides::BOTTOM, "BOTTOM"),
            (Sides::LEFT, "LEFT"),
        ]
        .into_iter()
        .filter(|(side, _)| self.contains(*side))
        .map(|(_, name)| name)
        .collect();
        match names.len() {
            0 => write!(f, "Sides(NONE)"),
            4 => write!(f, "Sides(ALL)"),
            _ => write!(f, "Sides({})", names.join(" | ")),
        }
    }
}

/// The edge of a box a title sits on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Edge {
    #[default]
    Top,
    Bottom,
}

/// A panel's shadow: a strip one cell wide down its right and along its
/// bottom, inside the panel's own rectangle (the box is a cell narrower
/// and shorter for it). In the theme's `shadow` style, else a dim grey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Shadow {
    /// `░`
    LightShade,
    /// `▒`
    MediumShade,
    /// `▓`
    DarkShade,
    /// `█`
    Block,
    /// Blank cells on a dark background.
    Dim,
    /// A glyph of your own.
    Symbol(char),
}

impl Shadow {
    /// Its glyph, and the style it takes without a theme's `shadow`.
    fn look(self) -> (char, &'static str) {
        match self {
            Shadow::LightShade => ('░', "grey35"),
            Shadow::MediumShade => ('▒', "grey35"),
            Shadow::DarkShade => ('▓', "grey35"),
            Shadow::Block => ('█', "grey35"),
            Shadow::Dim => (' ', "on grey15"),
            Shadow::Symbol(c) => (c, "grey35"),
        }
    }
}

/// A title on a box's edge: console markup, placed by `align`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Title {
    pub edge: Edge,
    pub align: Align,
    pub markup: String,
}

impl Title {
    pub fn new(edge: Edge, align: Align, markup: impl Into<String>) -> Title {
        Title {
            edge,
            align,
            markup: markup.into(),
        }
    }
}

/// Everything a box needs to be drawn.
pub(crate) struct Frame<'a> {
    pub border: Border,
    pub sides: Sides,
    pub style: &'a Style,
    pub title_style: &'a Style,
    pub titles: &'a [Title],
}

/// The cells a box takes on each side, top, right, bottom and left: a side
/// that is drawn, and an edge with a title (whether or not its line is).
pub(crate) fn insets(border: Border, sides: Sides, titles: &[Title]) -> [u16; 4] {
    let lines = border.set().is_some();
    let titled = |edge: Edge| titles.iter().any(|t| t.edge == edge);
    [
        (lines && sides.contains(Sides::TOP)) || titled(Edge::Top),
        lines && sides.contains(Sides::RIGHT),
        (lines && sides.contains(Sides::BOTTOM)) || titled(Edge::Bottom),
        lines && sides.contains(Sides::LEFT),
    ]
    .map(u16::from)
}

/// Draw `frame` round `rect` on `screen`, the inside left alone; the
/// rectangles written. A box too small for its lines draws none.
pub(crate) fn draw_frame(
    screen: &mut Screen,
    console: &Console,
    frame: &Frame,
    rect: Rect,
) -> Vec<Rect> {
    let (w, h) = (rect.width, rect.height);
    let [top, right, bottom, left] = insets(frame.border, frame.sides, frame.titles);
    if w == 0 || h == 0 || w < left + right || h < top + bottom {
        return Vec::new();
    }
    let set = frame.border.set();
    let line = |side: Sides| set.is_some() && frame.sides.contains(side);
    let (top_line, bottom_line) = (line(Sides::TOP), line(Sides::BOTTOM));
    let (left_line, right_line) = (left == 1, right == 1);
    // In text mode a box is blank: a screen reader reads no lines, and the
    // titles stay.
    let text_mode = crate::a11y::text_mode();
    let glyph = |pick: fn(&BorderSet) -> &'static str| -> &'static str {
        match set {
            Some(set) if !text_mode => pick(&set),
            _ => " ",
        }
    };
    let mut written = Vec::new();
    for (edge, y, drawn) in [
        (Edge::Top, rect.y, top == 1),
        (Edge::Bottom, rect.bottom() - 1, bottom == 1),
    ] {
        // One row for both: a box one row tall shows its top.
        if !drawn || (edge == Edge::Bottom && top == 1 && h < 2) {
            continue;
        }
        let has_line = match edge {
            Edge::Top => top_line,
            Edge::Bottom => bottom_line,
        };
        let cells: Vec<(&'static str, bool)> = (0..w)
            .map(|x| {
                if !has_line {
                    let side = (x == 0 && left_line)
                        .then(|| glyph(|s| s.left))
                        .or_else(|| (x == w - 1 && right_line).then(|| glyph(|s| s.right)));
                    return side.map_or((" ", false), |g| (g, true));
                }
                let pick: fn(&BorderSet) -> &'static str = match (edge, x) {
                    (Edge::Top, 0) if left_line => |s| s.top_left,
                    (Edge::Top, _) if x == w - 1 && right_line => |s| s.top_right,
                    (Edge::Top, _) => |s| s.top,
                    (Edge::Bottom, 0) if left_line => |s| s.bottom_left,
                    (Edge::Bottom, _) if x == w - 1 && right_line => |s| s.bottom_right,
                    (Edge::Bottom, _) => |s| s.bottom,
                };
                (glyph(pick), true)
            })
            .collect();
        // Titles go between the corner and one glyph of line, ` Title `,
        // or, with no line on this edge, between the sides.
        let area = if has_line {
            (2.min(w), w.saturating_sub(2))
        } else {
            (u16::from(left_line), w - u16::from(right_line))
        };
        let titles: Vec<&Title> = frame.titles.iter().filter(|t| t.edge == edge).collect();
        let pieces = title_pieces(console, &titles, area, has_line, frame.title_style);
        let row = edge_row(&cells, frame.style, &pieces);
        let at = Rect::new(rect.x, y, w, 1);
        screen.write_lines(at, &[row]);
        written.push(at);
    }
    let from = rect.y + u16::from(top_line);
    let to = rect.bottom() - u16::from(bottom_line && (h >= 2 || !top_line));
    for (x, drawn, pick) in [
        (
            rect.x,
            left_line,
            (|s: &BorderSet| s.left) as fn(&BorderSet) -> &'static str,
        ),
        (rect.right() - 1, right_line, |s: &BorderSet| s.right),
    ] {
        if !drawn || to <= from {
            continue;
        }
        let at = Rect::new(x, from, 1, to - from);
        let g = glyph(pick);
        let lines: Vec<Vec<Segment>> = (from..to)
            .map(|_| vec![Segment::new(g, Some(frame.style.clone()))])
            .collect();
        screen.write_lines(at, &lines);
        written.push(at);
    }
    written
}

/// A title rendered to its cells: where it starts on the edge, its
/// segments and its width.
pub(crate) type Piece = (u16, Vec<Segment>, u16);

/// An edge's row: `cells` (each a glyph, and whether it is a line's, in
/// the line's `style`) with `pieces` laid over them.
pub(crate) fn edge_row(cells: &[(&str, bool)], style: &Style, pieces: &[Piece]) -> Vec<Segment> {
    let mut row: Vec<Segment> = Vec::new();
    let mut run = String::new();
    let mut run_line = false;
    let flush = |row: &mut Vec<Segment>, run: &mut String, line: bool| {
        if !run.is_empty() {
            let style = line.then(|| style.clone());
            row.push(Segment::new(std::mem::take(run), style));
        }
    };
    let mut x = 0u16;
    while (x as usize) < cells.len() {
        if let Some((_, segments, width)) = pieces.iter().find(|(at, _, _)| *at == x) {
            flush(&mut row, &mut run, run_line);
            row.extend(segments.iter().cloned());
            x += (*width).max(1);
            continue;
        }
        let (glyph, line) = cells[x as usize];
        if line != run_line {
            flush(&mut row, &mut run, run_line);
            run_line = line;
        }
        run.push_str(glyph);
        x += 1;
    }
    flush(&mut row, &mut run, run_line);
    row
}

/// Lay `titles` out between `area`'s ends (`from..to` on the edge): the
/// ones aligned left from the start, the right ones to the end and the
/// centred ones in the middle, one cell apart. When they do not fit, the
/// right ones give way first (the one nearest the middle first), then the
/// centred ones (the last first), and the left ones are cut with `…`; a
/// group alone on its edge is cut rather than given up. On a line, each
/// is ` Title `.
pub(crate) fn title_pieces(
    console: &Console,
    titles: &[&Title],
    (from, to): (u16, u16),
    padded: bool,
    style: &Style,
) -> Vec<Piece> {
    if titles.is_empty() || to <= from {
        return Vec::new();
    }
    let room = to - from;
    let pad = if padded { 2 } else { 0 };
    let width = |t: &Title| crate::widget::markup_width(&t.markup).saturating_add(pad);
    let group = |align: fn(Align) -> bool| -> Vec<&Title> {
        titles.iter().copied().filter(|t| align(t.align)).collect()
    };
    let left = group(|a| matches!(a, Align::Start | Align::Stretch));
    let centred = group(|a| a == Align::Center);
    let righted = group(|a| a == Align::End);
    let length = |group: &[&Title]| -> u32 {
        let cells: u32 = group.iter().map(|t| u32::from(width(t))).sum();
        cells + group.len().saturating_sub(1) as u32
    };
    let room32 = u32::from(room);
    // Where each group starts: (left end, centre start, right start).
    let places = |left: &[&Title], center: &[&Title], right: &[&Title]| {
        let left_end = u32::from(from) + length(left).min(room32);
        let right_start = u32::from(to).saturating_sub(length(right));
        let center_start = u32::from(from) + room32.saturating_sub(length(center)) / 2;
        (left_end, center_start, right_start)
    };
    let fits = |left: &[&Title], center: &[&Title], right: &[&Title]| {
        let (left_end, center_start, right_start) = places(left, center, right);
        let gap_after_left = u32::from(!left.is_empty());
        let right_ok = right.is_empty()
            || (length(right) <= room32 && right_start >= left_end + gap_after_left);
        let center_end = center_start + length(center);
        let center_ok = center.is_empty()
            || (length(center) <= room32
                && center_start >= left_end + gap_after_left
                && (right.is_empty() || center_end < right_start));
        right_ok && center_ok
    };
    // The left titles first, then as many centred ones as fit beside
    // them, then as many right ones as fit beside both.
    let mut center = centred.clone();
    while !center.is_empty() && !fits(&left, &center, &[]) {
        center.pop();
    }
    let mut right = righted.clone();
    while !right.is_empty() && !fits(&left, &center, &right) {
        right.remove(0);
    }
    // Alone on the edge, a group is cut, not given up: a centred title
    // too wide for the edge takes all of it.
    if left.is_empty() && center.is_empty() {
        if let Some(first) = centred.first() {
            center.push(first);
            right.clear();
        } else if let (true, Some(last)) = (right.is_empty(), righted.last()) {
            right.push(last);
        }
    }
    let (_, center_start, right_start) = places(&left, &center, &right);
    let mut pieces = Vec::new();
    let mut lay = |group: &[&Title], start: u32| {
        let mut at = start.max(u32::from(from));
        for title in group {
            if at >= u32::from(to) {
                break;
            }
            let space = (u32::from(to) - at).min(u32::from(width(title))) as u16;
            if let Some(piece) = render_title(console, title, space, padded, style) {
                pieces.push((at as u16, piece, space));
            }
            at += u32::from(space) + 1;
        }
    };
    lay(&left, u32::from(from));
    lay(&center, center_start);
    lay(&right, right_start);
    pieces
}

/// A title's markup on one row `space` cells wide (cut with `…`), over
/// `style`; with `padded`, a space either side. `None` when there is no
/// room for any of it.
fn render_title(
    console: &Console,
    title: &Title,
    space: u16,
    padded: bool,
    style: &Style,
) -> Option<Vec<Segment>> {
    let inner = if padded { space.checked_sub(2)? } else { space };
    if inner == 0 {
        return None;
    }
    let mut line: Vec<Segment> = crate::widget::markup_line(console, &title.markup, inner)
        .into_iter()
        .map(|s| {
            let combined = match &s.style {
                Some(own) => style.combine(own),
                None => style.clone(),
            };
            Segment::new(s.text, Some(combined))
        })
        .collect();
    let used: usize = line.iter().map(Segment::cell_length).sum();
    if padded {
        line.insert(0, Segment::new(" ", Some(style.clone())));
        line.push(Segment::new(" ", Some(style.clone())));
    }
    // A title narrower than its markup said (a wide glyph cut) is padded
    // out, so what follows stays where it was placed.
    let short = (inner as usize).saturating_sub(used);
    if short > 0 {
        line.push(Segment::new(" ".repeat(short), Some(style.clone())));
    }
    Some(line)
}

/// Draw `shadow` in its strip of `rect`, the right column and the bottom
/// row less their first cells; the rectangles written.
pub(crate) fn draw_shadow(
    screen: &mut Screen,
    shadow: Shadow,
    style: &Style,
    rect: Rect,
) -> Vec<Rect> {
    if rect.width < 2 || rect.height < 2 {
        return Vec::new();
    }
    let (glyph, _) = shadow.look();
    let glyph = if crate::a11y::text_mode() { ' ' } else { glyph };
    let cell = |n: u16| Segment::new(glyph.to_string().repeat(n as usize), Some(style.clone()));
    let column = Rect::new(rect.right() - 1, rect.y + 1, 1, rect.height - 1);
    let row = Rect::new(rect.x + 1, rect.bottom() - 1, rect.width - 2, 1);
    let lines: Vec<Vec<Segment>> = (0..column.height).map(|_| vec![cell(1)]).collect();
    screen.write_lines(column, &lines);
    let mut written = vec![column];
    if !row.is_empty() {
        screen.write_lines(row, &[vec![cell(row.width)]]);
        written.push(row);
    }
    written
}

/// The style a shadow takes: the theme's `shadow`, else its own grey.
pub(crate) fn shadow_style(console: &Console, shadow: Shadow) -> Style {
    crate::widget::theme_style(console, "shadow", shadow.look().1)
}

// -- Joining lines ------------------------------------------------------

pub(crate) const LIGHT: u8 = 1;
pub(crate) const HEAVY: u8 = 2;
pub(crate) const DOUBLE: u8 = 3;

/// Box-drawing glyphs by their arms: up, right, down and left, each 0
/// (none), 1 (light), 2 (heavy) or 3 (double), from the Unicode names of
/// U+2500 to U+256C.
const JOINS: [(&str, [u8; 4]); 97] = [
    ("─", [0, 1, 0, 1]),
    ("━", [0, 2, 0, 2]),
    ("│", [1, 0, 1, 0]),
    ("┃", [2, 0, 2, 0]),
    ("┌", [0, 1, 1, 0]),
    ("┍", [0, 2, 1, 0]),
    ("┎", [0, 1, 2, 0]),
    ("┏", [0, 2, 2, 0]),
    ("┐", [0, 0, 1, 1]),
    ("┑", [0, 0, 1, 2]),
    ("┒", [0, 0, 2, 1]),
    ("┓", [0, 0, 2, 2]),
    ("└", [1, 1, 0, 0]),
    ("┕", [1, 2, 0, 0]),
    ("┖", [2, 1, 0, 0]),
    ("┗", [2, 2, 0, 0]),
    ("┘", [1, 0, 0, 1]),
    ("┙", [1, 0, 0, 2]),
    ("┚", [2, 0, 0, 1]),
    ("┛", [2, 0, 0, 2]),
    ("├", [1, 1, 1, 0]),
    ("┝", [1, 2, 1, 0]),
    ("┞", [2, 1, 1, 0]),
    ("┟", [1, 1, 2, 0]),
    ("┠", [2, 1, 2, 0]),
    ("┡", [2, 2, 1, 0]),
    ("┢", [1, 2, 2, 0]),
    ("┣", [2, 2, 2, 0]),
    ("┤", [1, 0, 1, 1]),
    ("┥", [1, 0, 1, 2]),
    ("┦", [2, 0, 1, 1]),
    ("┧", [1, 0, 2, 1]),
    ("┨", [2, 0, 2, 1]),
    ("┩", [2, 0, 1, 2]),
    ("┪", [1, 0, 2, 2]),
    ("┫", [2, 0, 2, 2]),
    ("┬", [0, 1, 1, 1]),
    ("┭", [0, 1, 1, 2]),
    ("┮", [0, 2, 1, 1]),
    ("┯", [0, 2, 1, 2]),
    ("┰", [0, 1, 2, 1]),
    ("┱", [0, 1, 2, 2]),
    ("┲", [0, 2, 2, 1]),
    ("┳", [0, 2, 2, 2]),
    ("┴", [1, 1, 0, 1]),
    ("┵", [1, 1, 0, 2]),
    ("┶", [1, 2, 0, 1]),
    ("┷", [1, 2, 0, 2]),
    ("┸", [2, 1, 0, 1]),
    ("┹", [2, 1, 0, 2]),
    ("┺", [2, 2, 0, 1]),
    ("┻", [2, 2, 0, 2]),
    ("┼", [1, 1, 1, 1]),
    ("┽", [1, 1, 1, 2]),
    ("┾", [1, 2, 1, 1]),
    ("┿", [1, 2, 1, 2]),
    ("╀", [2, 1, 1, 1]),
    ("╁", [1, 1, 2, 1]),
    ("╂", [2, 1, 2, 1]),
    ("╃", [2, 1, 1, 2]),
    ("╄", [2, 2, 1, 1]),
    ("╅", [1, 1, 2, 2]),
    ("╆", [1, 2, 2, 1]),
    ("╇", [2, 2, 1, 2]),
    ("╈", [1, 2, 2, 2]),
    ("╉", [2, 1, 2, 2]),
    ("╊", [2, 2, 2, 1]),
    ("╋", [2, 2, 2, 2]),
    ("═", [0, 3, 0, 3]),
    ("║", [3, 0, 3, 0]),
    ("╒", [0, 3, 1, 0]),
    ("╓", [0, 1, 3, 0]),
    ("╔", [0, 3, 3, 0]),
    ("╕", [0, 0, 1, 3]),
    ("╖", [0, 0, 3, 1]),
    ("╗", [0, 0, 3, 3]),
    ("╘", [1, 3, 0, 0]),
    ("╙", [3, 1, 0, 0]),
    ("╚", [3, 3, 0, 0]),
    ("╛", [1, 0, 0, 3]),
    ("╜", [3, 0, 0, 1]),
    ("╝", [3, 0, 0, 3]),
    ("╞", [1, 3, 1, 0]),
    ("╟", [3, 1, 3, 0]),
    ("╠", [3, 3, 3, 0]),
    ("╡", [1, 0, 1, 3]),
    ("╢", [3, 0, 3, 1]),
    ("╣", [3, 0, 3, 3]),
    ("╤", [0, 3, 1, 3]),
    ("╥", [0, 1, 3, 1]),
    ("╦", [0, 3, 3, 3]),
    ("╧", [1, 3, 0, 3]),
    ("╨", [3, 1, 0, 1]),
    ("╩", [3, 3, 0, 3]),
    ("╪", [1, 3, 1, 3]),
    ("╫", [3, 1, 3, 1]),
    ("╬", [3, 3, 3, 3]),
];

/// The glyph whose arms are `arms` (up, right, down, left). A mix Unicode
/// has no glyph for (double with heavy, a lone arm) is drawn lighter:
/// heavy arms first become light, then double ones.
pub(crate) fn join(arms: [u8; 4]) -> &'static str {
    let find = |arms: [u8; 4]| JOINS.iter().find(|(_, a)| *a == arms).map(|(g, _)| *g);
    if let Some(glyph) = find(arms) {
        return glyph;
    }
    let lighter = |from: u8, arms: [u8; 4]| arms.map(|w| if w == from { LIGHT } else { w });
    let arms = lighter(HEAVY, arms);
    if let Some(glyph) = find(arms) {
        return glyph;
    }
    let arms = lighter(DOUBLE, arms);
    if let Some(glyph) = find(arms) {
        return glyph;
    }
    // A lone arm: a line through the cell, the way it points.
    if arms[1] > 0 || arms[3] > 0 {
        "─"
    } else if arms[0] > 0 || arms[2] > 0 {
        "│"
    } else {
        " "
    }
}

/// The lines of boxes that share their edges, cell by cell: each cell's
/// arms (up, right, down, left, by weight) and whether a lit box drew one.
#[derive(Default)]
pub(crate) struct Lattice {
    cells: HashMap<(u16, u16), ([u8; 4], bool)>,
}

impl Lattice {
    /// Add the outline of `rect` (its edge cells), its lines `weight`
    /// heavy, `lit` or not. A box narrower or shorter than two cells has
    /// no outline.
    pub fn outline(&mut self, rect: Rect, weight: u8, lit: bool) {
        if rect.width < 2 || rect.height < 2 {
            return;
        }
        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.right() - 1, rect.bottom() - 1);
        let mut arm = |x: u16, y: u16, arm: usize| {
            let (arms, on) = self.cells.entry((x, y)).or_default();
            arms[arm] = arms[arm].max(weight);
            *on |= lit;
        };
        for x in x0..=x1 {
            for y in [y0, y1] {
                if x > x0 {
                    arm(x, y, 3);
                }
                if x < x1 {
                    arm(x, y, 1);
                }
            }
        }
        for y in y0..=y1 {
            for x in [x0, x1] {
                if y > y0 {
                    arm(x, y, 0);
                }
                if y < y1 {
                    arm(x, y, 2);
                }
            }
        }
    }

    /// Join `arms` (and `lit`) to the cell at `x`, `y`.
    pub fn join(&mut self, x: u16, y: u16, arms: [u8; 4], lit: bool) {
        let (own, on) = self.cells.entry((x, y)).or_default();
        for (own, arm) in own.iter_mut().zip(arms) {
            *own = (*own).max(arm);
        }
        *on |= lit;
    }

    /// The cells on `rect`'s outline, row by row.
    pub fn on_outline(&self, rect: Rect) -> Vec<((u16, u16), [u8; 4], bool)> {
        if rect.is_empty() {
            return Vec::new();
        }
        let (x1, y1) = (rect.right() - 1, rect.bottom() - 1);
        self.cells()
            .into_iter()
            .filter(|((x, y), _, _)| {
                let inside = (rect.x..=x1).contains(x) && (rect.y..=y1).contains(y);
                inside && (*x == rect.x || *x == x1 || *y == rect.y || *y == y1)
            })
            .collect()
    }

    /// Each cell with its arms and whether it is lit, row by row.
    pub fn cells(&self) -> Vec<((u16, u16), [u8; 4], bool)> {
        let mut cells: Vec<_> = self
            .cells
            .iter()
            .map(|(&(x, y), &(arms, lit))| ((x, y), arms, lit))
            .collect();
        cells.sort_by_key(|((x, y), _, _)| (*y, *x));
        cells
    }
}

/// The glyph a `border` kind draws for a cell of a lattice with `arms`,
/// `outer` saying which edges of the whole lattice the cell is on (top,
/// right, bottom, left).
pub(crate) fn lattice_glyph(border: Border, arms: [u8; 4], outer: [bool; 4]) -> &'static str {
    if crate::a11y::text_mode() {
        return " ";
    }
    let [up, right, down, left] = arms.map(|w| w > 0);
    let horizontal = (left || right) && !up && !down;
    let vertical = (up || down) && !left && !right;
    let corner = arms.iter().filter(|w| **w > 0).count() == 2 && !horizontal && !vertical;
    let light = arms.iter().all(|w| *w <= LIGHT);
    let own = arms.iter().all(|w| *w == 0 || *w == border.weight());
    let Some(set) = border.set() else {
        return " ";
    };
    match border {
        Border::Ascii => {
            return if horizontal {
                "-"
            } else if vertical {
                "|"
            } else {
                "+"
            };
        }
        Border::Rounded if corner && light => {
            return match (up, right, down, left) {
                (false, true, true, false) => "╭",
                (false, false, true, true) => "╮",
                (true, true, false, false) => "╰",
                _ => "╯",
            };
        }
        Border::Rounded | Border::Plain | Border::Thick | Border::Double => {}
        // Dashed lines and glyphs of their own: the set's along its own
        // lines, plain joins.
        _ if own && horizontal => return if outer[2] { set.bottom } else { set.top },
        _ if own && vertical => return if outer[1] { set.right } else { set.left },
        Border::QuadrantInside | Border::QuadrantOutside | Border::Custom(_) if own && corner => {
            return match (up, right, down, left) {
                (false, true, true, false) => set.top_left,
                (false, false, true, true) => set.top_right,
                (true, true, false, false) => set.bottom_left,
                _ => set.bottom_right,
            };
        }
        _ => {}
    }
    join(arms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_join_is_found_and_mixes_fall_back() {
        assert_eq!(join([0, 1, 1, 1]), "┬");
        assert_eq!(join([1, 1, 1, 1]), "┼");
        // A thick pane's corner meeting a light neighbour.
        assert_eq!(join([0, 2, 2, 1]), "┲");
        assert_eq!(join([2, 0, 2, 0]), "┃");
        // Double with heavy has no glyph: the heavy arm goes light.
        assert_eq!(join([3, 2, 3, 0]), "╟");
        // A lone arm is a line.
        assert_eq!(join([0, 1, 0, 0]), "─");
    }

    #[test]
    fn a_lattice_joins_two_boxes_that_share_an_edge() {
        let mut lattice = Lattice::default();
        lattice.outline(Rect::new(0, 0, 4, 3), LIGHT, false);
        lattice.outline(Rect::new(3, 0, 4, 3), HEAVY, true);
        let row: String = lattice
            .cells()
            .iter()
            .filter(|((_, y), _, _)| *y == 0)
            .map(|(_, arms, _)| join(*arms))
            .collect();
        // The shared column is heavy down, so the junction is ┲.
        assert_eq!(row, "┌──┲━━┓");
        let lit: Vec<bool> = lattice
            .cells()
            .iter()
            .filter(|((_, y), _, _)| *y == 0)
            .map(|(_, _, lit)| *lit)
            .collect();
        assert_eq!(lit, [false, false, false, true, true, true, true]);
    }

    #[test]
    fn rounded_lattices_round_their_outer_corners_only() {
        let glyph = |arms| lattice_glyph(Border::Rounded, arms, [false; 4]);
        assert_eq!(glyph([0, 1, 1, 0]), "╭");
        assert_eq!(glyph([0, 1, 1, 1]), "┬");
        assert_eq!(glyph([0, 2, 2, 0]), "┏");
        let dashed = |arms, outer| lattice_glyph(Border::LightTripleDashed, arms, outer);
        assert_eq!(dashed([0, 1, 0, 1], [false; 4]), "┄");
        assert_eq!(dashed([1, 1, 1, 0], [false; 4]), "├");
    }
}

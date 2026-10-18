//! A canvas in world coordinates, as ratatui's `Canvas` widget: points,
//! lines, rectangles, circles, filled areas, a world map, your own shapes
//! and text, in layers, drawn with Braille dots, block patterns, half
//! blocks or one glyph a cell.

use rich::cells::char_cell_width;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use super::canvas::{bresenham, octant, quadrant, sextant};
use super::world::MapResolution;
use super::{
    cell_units, cells, lines_to_segments, theme_style, user_style, Charset, DotCanvas, Line,
};
use crate::fidelity::ascii_text;

/// Rows a [`Canvas`] takes unless [`Canvas::height`] says otherwise.
const DEFAULT_HEIGHT: usize = 10;
/// The longest side drawn, in cells.
const MAX_SIDE: usize = u16::MAX as usize;

/// How a [`Canvas`] layer, or a [`Dataset`](super::Dataset), marks what it
/// draws.
///
/// On an ASCII-only console (see [`Charset`]) every marker but
/// [`Ascii`](Self::Ascii) draws one glyph a cell: `#` for the blocks, `*`
/// for the others.
///
/// [`Quadrant`](Self::Quadrant), [`Bar`](Self::Bar) and
/// [`HalfBlock`](Self::HalfBlock) draw with Block Elements, which every
/// terminal font has. The sextants are in Symbols for Legacy Computing
/// (Unicode 13) and the octants in its supplement (Unicode 16): a font
/// without them shows a box or a blank in each cell, so check the fonts
/// your users have before choosing them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Marker {
    /// Braille dots, 2×4 a cell.
    #[default]
    Braille,
    /// Half blocks (`▀`, `▄`, `█`), 1×2 a cell. A cell whose halves differ
    /// in colour shows the upper colour on the lower one.
    HalfBlock,
    /// A full block, `█`, a cell.
    Block,
    /// A lower half block, `▄`, a cell.
    Bar,
    /// Quarters of a cell (`▘`, `▚`, `▙`, ..), 2×2 a cell.
    Quadrant,
    /// Sixths of a cell (`🬀` to `🬻`), 2×3 a cell.
    Sextant,
    /// Eighths of a cell (U+1CD00 to U+1CDE5), 2×4 a cell: as fine as
    /// Braille, and solid.
    Octant,
    /// A dot, `•`, a cell.
    Dot,
    /// The given character, a cell. One that is not one cell wide, or not
    /// printable ASCII on an ASCII console, becomes `*`.
    Ascii(char),
}

impl Marker {
    /// The marker actually drawn, on an ASCII-only console or not.
    pub(crate) fn resolve(self, ascii: bool) -> Marker {
        match self {
            Marker::Ascii(c)
                if char_cell_width(c) == 1 && (!ascii || c.is_ascii_graphic() || c == ' ') =>
            {
                self
            }
            Marker::Ascii(_) => Marker::Ascii('*'),
            _ if !ascii => self,
            Marker::Block
            | Marker::HalfBlock
            | Marker::Bar
            | Marker::Quadrant
            | Marker::Sextant
            | Marker::Octant => Marker::Ascii('#'),
            _ => Marker::Ascii('*'),
        }
    }

    /// Dots across and down one cell.
    pub(crate) fn per_cell(self) -> (usize, usize) {
        match self {
            Marker::Braille | Marker::Octant => (2, 4),
            Marker::Sextant => (2, 3),
            Marker::Quadrant => (2, 2),
            Marker::HalfBlock => (1, 2),
            _ => (1, 1),
        }
    }

    /// The glyph for a cell of a pattern marker, from its set dots (bit
    /// `x + 2 * y`).
    fn pattern(self, bits: u8) -> char {
        match self {
            Marker::Quadrant => quadrant(bits),
            Marker::Sextant => sextant(bits),
            _ => octant(bits),
        }
    }
}

/// One recorded drawing step, in world coordinates. Styles are specs:
/// theme keys or definitions.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Op {
    Points(Vec<(f64, f64)>, String),
    Line((f64, f64), (f64, f64), String),
    /// A line, and the area between it and a level `y`.
    Area((f64, f64), (f64, f64), f64, String),
    Circle((f64, f64), f64, String),
    Map(MapResolution, String),
    Print((f64, f64), String, String),
    Layer,
    Marker(Marker),
}

/// Something a [`CanvasPainter`] can draw, such as a wireframe, a sprite or
/// a chart of your own. [`CanvasPainter::shape`] calls
/// [`draw`](Self::draw), which draws with the painter's points, lines and
/// text, in world coordinates.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Canvas, CanvasPainter, Marker, Shape};
///
/// /// A cross centred on a point.
/// struct Cross(f64, f64);
///
/// impl Shape for Cross {
///     fn draw(&self, painter: &mut CanvasPainter) {
///         let Cross(x, y) = *self;
///         painter.line(x - 1.0, y, x + 1.0, y, "red");
///         painter.line(x, y - 1.0, x, y + 1.0, "red");
///     }
/// }
///
/// let canvas = Canvas::new()
///     .x_bounds(0.0, 4.0)
///     .y_bounds(0.0, 2.0)
///     .width(5)
///     .height(3)
///     .marker(Marker::Ascii('+'))
///     .paint(|p| {
///         p.shape(&Cross(2.0, 1.0));
///     });
/// let console = Console::builder().width(5).color_system(None).build();
/// assert_eq!(console.render_export(&canvas), "  +  \n +++ \n  +  \n");
/// ```
pub trait Shape {
    /// Draw the shape with `painter`.
    fn draw(&self, painter: &mut CanvasPainter);
}

/// What a [`Canvas::paint`] closure draws with.
///
/// Coordinates are world coordinates, `y` growing upwards, as the canvas's
/// bounds give them. Anything outside the bounds is clipped, and NaN or
/// infinite coordinates are skipped. Styles are theme keys or definitions
/// (`"bold red"`); an empty one draws unstyled.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CanvasPainter {
    ops: Vec<Op>,
}

impl CanvasPainter {
    /// A dot at each point.
    pub fn points(&mut self, points: &[(f64, f64)], style: &str) -> &mut Self {
        self.ops
            .push(Op::Points(points.to_vec(), style.to_string()));
        self
    }

    /// A dot at (`x`, `y`).
    pub fn point(&mut self, x: f64, y: f64, style: &str) -> &mut Self {
        self.points(&[(x, y)], style)
    }

    /// Draw a [`Shape`] of your own.
    pub fn shape(&mut self, shape: &dyn Shape) -> &mut Self {
        shape.draw(self);
        self
    }

    /// The world's coastlines, in longitude (`x`, -180 to 180) and latitude
    /// (`y`, -90 to 90): give the canvas those bounds for the whole world,
    /// or narrower ones for a region. The lines are Natural Earth's, in the
    /// public domain, simplified: see [`MapResolution`].
    pub fn map(&mut self, resolution: MapResolution, style: &str) -> &mut Self {
        self.ops.push(Op::Map(resolution, style.to_string()));
        self
    }

    /// A straight line from (`x1`, `y1`) to (`x2`, `y2`), with the area
    /// between it and the level `y = fill_to` filled in: a strip of an area
    /// chart.
    pub fn area(
        &mut self,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        fill_to: f64,
        style: &str,
    ) -> &mut Self {
        self.ops
            .push(Op::Area((x1, y1), (x2, y2), fill_to, style.to_string()));
        self
    }

    /// A straight line from (`x1`, `y1`) to (`x2`, `y2`).
    pub fn line(&mut self, x1: f64, y1: f64, x2: f64, y2: f64, style: &str) -> &mut Self {
        self.ops
            .push(Op::Line((x1, y1), (x2, y2), style.to_string()));
        self
    }

    /// Lines joining `points` in order. A NaN or infinite point breaks the
    /// line; a point alone between two breaks is drawn as a dot.
    pub fn polyline(&mut self, points: &[(f64, f64)], style: &str) -> &mut Self {
        let finite = |p: &(f64, f64)| p.0.is_finite() && p.1.is_finite();
        for run in points.split(|p| !finite(p)) {
            match run {
                [] => {}
                [p] => self.ops.push(Op::Points(vec![*p], style.to_string())),
                _ => {
                    for pair in run.windows(2) {
                        self.ops.push(Op::Line(pair[0], pair[1], style.to_string()));
                    }
                }
            }
        }
        self
    }

    /// The outline of a rectangle whose bottom left corner is (`x`, `y`).
    pub fn rect(&mut self, x: f64, y: f64, width: f64, height: f64, style: &str) -> &mut Self {
        let (r, t) = (x + width, y + height);
        self.line(x, y, r, y, style)
            .line(x, t, r, t, style)
            .line(x, y, x, t, style)
            .line(r, y, r, t, style)
    }

    /// The outline of a circle: a dot every degree.
    pub fn circle(&mut self, x: f64, y: f64, radius: f64, style: &str) -> &mut Self {
        self.ops.push(Op::Circle((x, y), radius, style.to_string()));
        self
    }

    /// `text` starting at the cell that holds (`x`, `y`), cut at the right
    /// edge. Text is drawn after every layer, over whatever is there.
    pub fn print(&mut self, x: f64, y: f64, text: &str, style: &str) -> &mut Self {
        self.ops
            .push(Op::Print((x, y), text.to_string(), style.to_string()));
        self
    }

    /// Start a new layer. Where a later layer draws in a cell, it replaces
    /// what earlier layers drew there; within a layer, Braille dots add up
    /// and the cell takes the colour of the last shape that drew in it.
    pub fn layer(&mut self) -> &mut Self {
        self.ops.push(Op::Layer);
        self
    }

    /// Draw what follows with `marker`. A marker different from the current
    /// one starts a new layer.
    pub fn marker(&mut self, marker: Marker) -> &mut Self {
        self.ops.push(Op::Marker(marker));
        self
    }
}

/// Shapes and text drawn in world coordinates, as ratatui's `Canvas`.
///
/// - The bounds ([`x_bounds`](Self::x_bounds), [`y_bounds`](Self::y_bounds))
///   are the world's edges: `x` grows to the right and `y` upwards. They
///   default to `0..1`; empty, reversed or infinite bounds draw nothing.
/// - It fills the height it is given (a `Layout` region, an intuiTUIve
///   node), or is [`height`](Self::height) rows when that is set, or 10
///   rows; and it is as wide as the width it is given (or
///   [`width`](Self::width)).
/// - [`background`](Self::background) styles the cells nothing drew in,
///   and goes under what is drawn.
/// - [`paint`](Self::paint) runs a closure once and records what it draws,
///   so the canvas renders the same every time and can be cloned.
/// - [`Marker::Braille`] (the default) draws 2×4 dots a cell. An ASCII-only
///   console gets one glyph a cell instead.
///
/// ```
/// use rich::Console;
/// use rich_ext::chart::{Canvas, Marker};
///
/// let console = Console::builder().width(12).color_system(None).build();
/// let canvas = Canvas::new()
///     .x_bounds(0.0, 11.0)
///     .y_bounds(0.0, 3.0)
///     .height(4)
///     .marker(Marker::Ascii('#'))
///     .paint(|p| {
///         p.rect(0.0, 0.0, 11.0, 3.0, "blue");
///         p.line(0.0, 0.0, 11.0, 3.0, "red");
///         p.print(2.0, 2.0, "hi", "bold");
///     });
/// assert_eq!(
///     console.render_export(&canvas),
///     concat!(
///         "############\n",
///         "# hi  #### #\n",
///         "# ####     #\n",
///         "############\n",
///     )
/// );
///
/// // Braille: 2×4 dots a cell.
/// let braille = Canvas::new()
///     .x_bounds(0.0, 3.0)
///     .y_bounds(0.0, 3.0)
///     .height(1)
///     .width(2)
///     .paint(|p| {
///         p.line(0.0, 0.0, 3.0, 3.0, "");
///     });
/// assert_eq!(console.render_export(&braille), "⡠⠊\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Canvas {
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    width: Option<usize>,
    height: Option<usize>,
    marker: Marker,
    background: Option<String>,
    ops: Vec<Op>,
}

impl Default for Canvas {
    fn default() -> Self {
        Self::new()
    }
}

impl Canvas {
    /// A blank canvas over `0..1` × `0..1`.
    pub fn new() -> Self {
        Canvas {
            x_bounds: [0.0, 1.0],
            y_bounds: [0.0, 1.0],
            width: None,
            height: None,
            marker: Marker::Braille,
            background: None,
            ops: Vec::new(),
        }
    }

    /// The world's left and right edges.
    pub fn x_bounds(mut self, left: f64, right: f64) -> Self {
        self.x_bounds = [left, right];
        self
    }

    /// The world's bottom and top edges.
    pub fn y_bounds(mut self, bottom: f64, top: f64) -> Self {
        self.y_bounds = [bottom, top];
        self
    }

    /// Width in cells (default: the width it is given).
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width.min(MAX_SIDE));
        self
    }

    /// Height in rows (at most 65535), whatever height it is given. Without
    /// it, the canvas fills the height it is given, or takes 10 rows.
    pub fn height(mut self, rows: usize) -> Self {
        self.height = Some(rows.min(MAX_SIDE));
        self
    }

    /// The style of the cells nothing drew in, and under what is drawn: a
    /// theme key or a definition. A style with only a foreground colour
    /// (`"navy_blue"`) is taken as that background (`"on navy_blue"`).
    pub fn background(mut self, style: impl Into<String>) -> Self {
        self.background = Some(style.into());
        self
    }

    /// The marker the first layer draws with (default
    /// [`Marker::Braille`]); [`CanvasPainter::marker`] changes it.
    pub fn marker(mut self, marker: Marker) -> Self {
        self.marker = marker;
        self
    }

    /// Run `draw` once and keep what it draws. Each call adds to what
    /// earlier calls drew, in a new layer.
    pub fn paint(mut self, draw: impl FnOnce(&mut CanvasPainter)) -> Self {
        let mut painter = CanvasPainter::default();
        draw(&mut painter);
        if !self.ops.is_empty() {
            self.ops.push(Op::Layer);
        }
        self.ops.extend(painter.ops);
        self
    }

    fn grid(&self, console: &Console, options: &ConsoleOptions) -> CellGrid {
        let width = self
            .width
            .map_or(options.max_width, |w| w.min(options.max_width));
        let view = View {
            x: self.x_bounds,
            y: self.y_bounds,
            width,
            height: self
                .height
                .or(options.height)
                .unwrap_or(DEFAULT_HEIGHT)
                .min(MAX_SIDE),
            marker: self.marker,
            ascii: is_ascii(console, options),
        };
        let mut grid = raster(&self.ops, &view, console);
        if let Some(style) = self
            .background
            .as_deref()
            .and_then(|spec| spec_style(console, spec))
        {
            grid.underlay(&background_style(style));
        }
        grid
    }
}

/// A background style: one with a foreground colour and no background
/// colour has the colour moved to the background.
fn background_style(style: Style) -> Style {
    match (style.color(), style.bgcolor()) {
        (Some(colour), None) => {
            let colour = colour.clone();
            style.without_color().with_bgcolor(colour)
        }
        _ => style,
    }
}

impl Renderable for Canvas {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        lines_to_segments(self.grid(console, options).into_lines(), options.max_width)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let max = self.width.unwrap_or(options.max_width);
        Measurement::new(max.min(12), max)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `Canvas, x from -180 to 180, y from -90 to 90: a world map, 4 points, 3
/// lines`, then the text printed on it (`Text: London, Paris`). A canvas
/// holds shapes rather than data, so it says what it drew.
impl crate::a11y::AccessibleText for Canvas {
    fn accessible_text(&self, _width: usize) -> String {
        use super::describe::counted;
        let format = super::ValueFormat::Compact;
        let axis = |[a, b]: [f64; 2]| super::describe::range(a.min(b), a.max(b), format);
        let mut head = format!(
            "Canvas, x {}, y {}",
            axis(self.x_bounds),
            axis(self.y_bounds)
        );
        let (mut points, mut lines, mut areas, mut circles) = (0, 0, 0, 0);
        let mut map = false;
        let mut text = Vec::new();
        for op in &self.ops {
            match op {
                Op::Points(p, _) => points += p.len(),
                Op::Line(..) => lines += 1,
                Op::Area(..) => areas += 1,
                Op::Circle(..) => circles += 1,
                Op::Map(..) => map = true,
                Op::Print(_, t, _) if !t.trim().is_empty() => text.push(t.trim().to_string()),
                _ => {}
            }
        }
        let mut drawn = Vec::new();
        if map {
            drawn.push("a world map".to_string());
        }
        for (n, one, many) in [
            (points, "point", "points"),
            (lines, "line", "lines"),
            (areas, "area segment", "area segments"),
            (circles, "circle", "circles"),
        ] {
            if n > 0 {
                drawn.push(counted(n, one, many));
            }
        }
        if drawn.is_empty() {
            head.push_str(": empty");
        } else {
            head.push_str(&format!(": {}", drawn.join(", ")));
        }
        if !text.is_empty() {
            head.push_str(&format!("\nText: {}", text.join(", ")));
        }
        head
    }
}

/// Whether to draw in ASCII only on `console`.
pub(crate) fn is_ascii(console: &Console, options: &ConsoleOptions) -> bool {
    Charset::Auto.resolve(console, options, Charset::Braille) == Charset::Ascii
}

/// A style spec: a `chart.*` theme key (falling back to the chart
/// defaults), another theme key or a definition. Empty is no style.
pub(crate) fn spec_style(console: &Console, spec: &str) -> Option<Style> {
    let spec = spec.trim();
    if spec.is_empty() {
        None
    } else if spec.starts_with("chart.") {
        Some(theme_style(console, spec))
    } else {
        Some(user_style(console, spec))
    }
}

/// Where and how [`raster`] draws.
pub(crate) struct View {
    pub(crate) x: [f64; 2],
    pub(crate) y: [f64; 2],
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) marker: Marker,
    pub(crate) ascii: bool,
}

/// The world's edges, checked to be finite and not empty.
#[derive(Clone, Copy)]
struct Bounds {
    left: f64,
    right: f64,
    bottom: f64,
    top: f64,
}

impl Bounds {
    fn new(x: [f64; 2], y: [f64; 2]) -> Option<Bounds> {
        let b = Bounds {
            left: x[0],
            right: x[1],
            bottom: y[0],
            top: y[1],
        };
        let (w, h) = (b.right - b.left, b.top - b.bottom);
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some(b)
    }

    fn contains(&self, (x, y): (f64, f64)) -> bool {
        (self.left..=self.right).contains(&x) && (self.bottom..=self.top).contains(&y)
    }

    /// The cell of a `columns` × `rows` grid that holds `p`, counted from
    /// the top left, as ratatui maps a point: scaled to `size - 1` and
    /// truncated.
    fn cell(&self, p: (f64, f64), columns: usize, rows: usize) -> Option<(usize, usize)> {
        if !self.contains(p) || columns == 0 || rows == 0 {
            return None;
        }
        let x = (p.0 - self.left) * (columns - 1) as f64 / (self.right - self.left);
        let y = (self.top - p.1) * (rows - 1) as f64 / (self.top - self.bottom);
        Some(((x as usize).min(columns - 1), (y as usize).min(rows - 1)))
    }

    /// The part of the segment `a`–`b` inside the bounds (Liang–Barsky).
    fn clip(&self, a: (f64, f64), b: (f64, f64)) -> Option<((f64, f64), (f64, f64))> {
        if ![a.0, a.1, b.0, b.1].iter().all(|v| v.is_finite()) {
            return None;
        }
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        for (p, q) in [
            (-dx, a.0 - self.left),
            (dx, self.right - a.0),
            (-dy, a.1 - self.bottom),
            (dy, self.top - a.1),
        ] {
            if !(p.is_finite() && q.is_finite()) {
                return None;
            }
            if p == 0.0 {
                if q < 0.0 {
                    return None;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if t0 > t1 {
            return None;
        }
        let at = |t: f64| {
            (
                (a.0 + t * dx).clamp(self.left, self.right),
                (a.1 + t * dy).clamp(self.bottom, self.top),
            )
        };
        Some((at(t0), at(t1)))
    }
}

/// Style specs seen so far, resolved once each.
struct Styles<'a> {
    console: &'a Console,
    specs: Vec<String>,
    resolved: Vec<Option<Style>>,
}

impl<'a> Styles<'a> {
    fn new(console: &'a Console) -> Self {
        Styles {
            console,
            specs: Vec::new(),
            resolved: Vec::new(),
        }
    }

    fn id(&mut self, spec: &str) -> usize {
        if let Some(i) = self.specs.iter().position(|s| s == spec) {
            return i;
        }
        self.specs.push(spec.to_string());
        self.resolved.push(spec_style(self.console, spec));
        self.specs.len() - 1
    }

    fn get(&self, id: usize) -> Option<Style> {
        self.resolved.get(id).cloned().flatten()
    }
}

/// The dots one layer has set: Braille on a [`DotCanvas`]; quadrants,
/// sextants and octants as a pattern of dots a cell, with the style that
/// last set a dot in it; anything else as a grid of pixels holding the
/// style that last set each.
enum Pixels {
    Braille(DotCanvas),
    Pattern {
        bits: Vec<u8>,
        styles: Vec<Option<usize>>,
    },
    Grid(Vec<Option<usize>>),
}

struct Layer {
    marker: Marker,
    width: usize,
    height: usize,
    pixels: Pixels,
}

impl Layer {
    fn new(marker: Marker, width: usize, height: usize) -> Layer {
        let pixels = match marker {
            Marker::Braille => Pixels::Braille(DotCanvas::new(width, height)),
            Marker::Quadrant | Marker::Sextant | Marker::Octant => Pixels::Pattern {
                bits: vec![0; width * height],
                styles: vec![None; width * height],
            },
            _ => {
                let (px, py) = marker.per_cell();
                Pixels::Grid(vec![None; width * px * height * py])
            }
        };
        Layer {
            marker,
            width,
            height,
            pixels,
        }
    }

    /// Pixels across and down.
    fn resolution(&self) -> (usize, usize) {
        let (px, py) = self.marker.per_cell();
        (self.width * px, self.height * py)
    }

    fn set(&mut self, (x, y): (usize, usize), style: usize) {
        let (columns, rows) = self.resolution();
        if x >= columns || y >= rows {
            return;
        }
        match &mut self.pixels {
            Pixels::Braille(canvas) => canvas.set(x as i64, y as i64, style),
            Pixels::Pattern { bits, styles } => {
                let (px, py) = self.marker.per_cell();
                let cell = (y / py) * self.width + x / px;
                bits[cell] |= 1 << (x % px + px * (y % py));
                styles[cell] = Some(style);
            }
            Pixels::Grid(grid) => grid[y * columns + x] = Some(style),
        }
    }

    fn point(&mut self, bounds: &Bounds, p: (f64, f64), style: usize) {
        let (columns, rows) = self.resolution();
        if let Some(at) = bounds.cell(p, columns, rows) {
            self.set(at, style);
        }
    }

    fn line(&mut self, bounds: &Bounds, a: (f64, f64), b: (f64, f64), style: usize) {
        let Some((a, b)) = bounds.clip(a, b) else {
            return;
        };
        let (columns, rows) = self.resolution();
        let (Some(a), Some(b)) = (bounds.cell(a, columns, rows), bounds.cell(b, columns, rows))
        else {
            return;
        };
        let to_i = |(x, y): (usize, usize)| (x as i64, y as i64);
        for (x, y) in bresenham(to_i(a), to_i(b)) {
            self.set((x as usize, y as usize), style);
        }
    }

    /// The line from `a` to `b`, and the pixels between it and the level
    /// `fill` (moved into the bounds), column by column.
    fn area(&mut self, bounds: &Bounds, a: (f64, f64), b: (f64, f64), fill: f64, style: usize) {
        if !fill.is_finite() {
            return self.line(bounds, a, b, style);
        }
        let Some((a, b)) = bounds.clip(a, b) else {
            return;
        };
        let (columns, rows) = self.resolution();
        let fill = fill.clamp(bounds.bottom, bounds.top);
        let (Some(start), Some(end), Some((_, level))) = (
            bounds.cell(a, columns, rows),
            bounds.cell(b, columns, rows),
            bounds.cell((a.0, fill), columns, rows),
        ) else {
            return;
        };
        let to_i = |(x, y): (usize, usize)| (x as i64, y as i64);
        for (x, y) in bresenham(to_i(start), to_i(end)) {
            let (x, y) = (x as usize, y as usize);
            for row in y.min(level)..=y.max(level) {
                self.set((x, row), style);
            }
        }
    }

    /// Copy the cells this layer drew in onto `out`.
    fn finish(&self, out: &mut CellGrid, styles: &Styles) {
        let columns = self.resolution().0;
        for row in 0..self.height {
            for col in 0..self.width {
                let cell = match (&self.pixels, self.marker) {
                    (Pixels::Braille(canvas), _) => match canvas.cell(col, row) {
                        (c, Some(style)) if canvas.is_set(col, row) => Some((c, styles.get(style))),
                        _ => None,
                    },
                    (Pixels::Pattern { bits, styles: set }, marker) => {
                        let i = row * self.width + col;
                        (bits[i] != 0).then(|| {
                            let style = set[i].and_then(|s| styles.get(s));
                            (marker.pattern(bits[i]), style)
                        })
                    }
                    (Pixels::Grid(grid), Marker::HalfBlock) => {
                        let upper = grid[2 * row * columns + col];
                        let lower = grid[(2 * row + 1) * columns + col];
                        half_block(upper, lower, styles)
                    }
                    (Pixels::Grid(grid), marker) => grid[row * columns + col].map(|s| {
                        let glyph = match marker {
                            Marker::Block => '█',
                            Marker::Bar => '▄',
                            Marker::Ascii(c) => c,
                            _ => '•',
                        };
                        (glyph, styles.get(s))
                    }),
                };
                if let Some((c, style)) = cell {
                    out.set(row, col, c.to_string(), style);
                }
            }
        }
    }
}

/// A half-block cell from the styles of its upper and lower pixels.
fn half_block(
    upper: Option<usize>,
    lower: Option<usize>,
    styles: &Styles,
) -> Option<(char, Option<Style>)> {
    match (upper, lower) {
        (Some(a), Some(b)) if a == b => Some(('█', styles.get(a))),
        (Some(a), Some(b)) => {
            let style = match styles.get(b).as_ref().and_then(|s| s.color()) {
                Some(colour) => Some(
                    styles
                        .get(a)
                        .unwrap_or_default()
                        .with_bgcolor(colour.clone()),
                ),
                None => styles.get(a),
            };
            Some(('▀', style))
        }
        (Some(a), None) => Some(('▀', styles.get(a))),
        (None, Some(b)) => Some(('▄', styles.get(b))),
        (None, None) => None,
    }
}

/// Draw `ops` onto a `view.width` × `view.height` grid. Cells nothing drew
/// in are left empty.
pub(crate) fn raster(ops: &[Op], view: &View, console: &Console) -> CellGrid {
    let (width, height) = (view.width.min(MAX_SIDE), view.height.min(MAX_SIDE));
    let mut out = CellGrid::new(width, height);
    let Some(bounds) = Bounds::new(view.x, view.y) else {
        return out;
    };
    if width == 0 || height == 0 {
        return out;
    }
    let mut styles = Styles::new(console);
    let mut layer = Layer::new(view.marker.resolve(view.ascii), width, height);
    let mut labels = Vec::new();
    for op in ops {
        match op {
            Op::Layer => {
                layer.finish(&mut out, &styles);
                layer = Layer::new(layer.marker, width, height);
            }
            Op::Marker(marker) => {
                let marker = marker.resolve(view.ascii);
                if marker != layer.marker {
                    layer.finish(&mut out, &styles);
                    layer = Layer::new(marker, width, height);
                }
            }
            Op::Points(points, style) => {
                let style = styles.id(style);
                for p in points {
                    layer.point(&bounds, *p, style);
                }
            }
            Op::Line(a, b, style) => {
                let style = styles.id(style);
                layer.line(&bounds, *a, *b, style);
            }
            Op::Area(a, b, fill, style) => {
                let style = styles.id(style);
                layer.area(&bounds, *a, *b, *fill, style);
            }
            Op::Map(resolution, style) => {
                let style = styles.id(style);
                for line in resolution.lines() {
                    for pair in line.windows(2) {
                        layer.line(&bounds, pair[0], pair[1], style);
                    }
                }
            }
            Op::Circle((x, y), radius, style) => {
                let style = styles.id(style);
                if radius.is_finite() {
                    for degree in 0..360 {
                        let (sin, cos) = f64::from(degree).to_radians().sin_cos();
                        layer.point(&bounds, (x + radius * cos, y + radius * sin), style);
                    }
                }
            }
            Op::Print(at, text, style) => labels.push((*at, text, style)),
        }
    }
    layer.finish(&mut out, &styles);
    for (at, text, style) in labels {
        if let Some((col, row)) = bounds.cell(at, width, height) {
            let text = if view.ascii && !text.is_ascii() {
                ascii_text(text)
            } else {
                text.clone()
            };
            out.put_text(row, col, &text, spec_style(console, style), width);
        }
    }
    out
}

/// A grid of terminal cells: each empty, or a character (with any marks
/// after it) and a style. The second cell of a wide character holds an
/// empty string.
#[derive(Clone, Debug)]
pub(crate) struct CellGrid {
    width: usize,
    height: usize,
    cells: Vec<Option<(String, Option<Style>)>>,
}

impl CellGrid {
    pub(crate) fn new(width: usize, height: usize) -> Self {
        CellGrid {
            width,
            height,
            cells: vec![None; width * height],
        }
    }

    /// Set one cell, blanking the other half of any wide character it
    /// cuts in two. Out of range is ignored.
    pub(crate) fn set(&mut self, row: usize, col: usize, text: String, style: Option<Style>) {
        if row >= self.height || col >= self.width {
            return;
        }
        let i = row * self.width + col;
        let blank = || Some((" ".to_string(), None));
        if !text.is_empty() && col > 0 && matches!(&self.cells[i], Some((t, _)) if t.is_empty()) {
            self.cells[i - 1] = blank();
        }
        if col + 1 < self.width && matches!(&self.cells[i], Some((t, _)) if cells(t) > 1) {
            self.cells[i + 1] = blank();
        }
        self.cells[i] = Some((text, style));
    }

    /// Write `text` from `col`, stopping before column `end` (and the
    /// grid's edge). A wide character that would cross it becomes a space.
    pub(crate) fn put_text(
        &mut self,
        row: usize,
        col: usize,
        text: &str,
        style: Option<Style>,
        end: usize,
    ) {
        let end = end.min(self.width);
        let units = cell_units(text);
        let mut at = col;
        let mut i = 0;
        while i < units.len() && at < end {
            let unit = &units[i];
            let wide = i + 1 < units.len() && units[i + 1].is_empty() && !unit.is_empty();
            if wide && at + 1 >= end {
                self.set(row, at, " ".to_string(), style.clone());
                break;
            }
            let unit = if unit.is_empty() && i == 0 {
                " ".to_string()
            } else {
                unit.clone()
            };
            self.set(row, at, unit, style.clone());
            at += 1;
            i += 1;
        }
    }

    /// Put `style` under every cell: empty cells become spaces in it, and
    /// drawn cells take it under their own.
    pub(crate) fn underlay(&mut self, style: &Style) {
        for cell in &mut self.cells {
            *cell = Some(match cell.take() {
                None => (" ".to_string(), Some(style.clone())),
                Some((text, own)) => {
                    let combined = match own {
                        Some(own) => style.combine(&own),
                        None => style.clone(),
                    };
                    (text, Some(combined))
                }
            });
        }
    }

    /// Give a drawn cell `style` in place of its own; an empty cell stays
    /// empty. Out of range is ignored.
    pub(crate) fn restyle(&mut self, row: usize, col: usize, style: Option<Style>) {
        if row < self.height && col < self.width {
            if let Some((_, own)) = &mut self.cells[row * self.width + col] {
                *own = style;
            }
        }
    }

    /// Copy the non-empty cells of `other` here, its top left at (`row`,
    /// `col`).
    pub(crate) fn blit(&mut self, other: &CellGrid, row: usize, col: usize) {
        for r in 0..other.height {
            for c in 0..other.width {
                if let Some((text, style)) = &other.cells[r * other.width + c] {
                    self.set(row + r, col + c, text.clone(), style.clone());
                }
            }
        }
    }

    /// The text of a row, empty cells as spaces.
    #[cfg(test)]
    pub(crate) fn row_string(&self, row: usize) -> String {
        (0..self.width)
            .map(|c| match &self.cells[row * self.width + c] {
                Some((t, _)) => t.clone(),
                None => " ".to_string(),
            })
            .collect()
    }

    /// The style of a cell.
    #[cfg(test)]
    pub(crate) fn style_at(&self, row: usize, col: usize) -> Option<Style> {
        self.cells[row * self.width + col]
            .as_ref()
            .and_then(|(_, s)| s.clone())
    }

    pub(crate) fn into_lines(self) -> Vec<Line> {
        let width = self.width;
        let mut cells = self.cells.into_iter();
        (0..self.height)
            .map(|_| {
                let mut line = Line::new();
                for cell in cells.by_ref().take(width) {
                    match cell {
                        Some((text, style)) => line.push(&text, style),
                        None => line.push(" ", None),
                    }
                }
                line
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accessible_text_says_what_was_drawn() {
        use crate::a11y::AccessibleText;
        let canvas = Canvas::new()
            .x_bounds(-1.0, 1.0)
            .y_bounds(-1.0, 1.0)
            .paint(|p| {
                p.circle(0.0, 0.0, 0.8, "cyan");
                p.line(-1.0, -1.0, 1.0, 1.0, "red");
                p.points(&[(0.0, 0.0), (0.5, 0.5)], "");
                p.layer();
                p.print(-0.2, 0.0, "hi", "bold");
            });
        assert_eq!(
            canvas.accessible_text(80),
            "Canvas, x from -1 to 1, y from -1 to 1: 2 points, 1 line, 1 circle\nText: hi"
        );
        let world = Canvas::new()
            .x_bounds(-180.0, 180.0)
            .y_bounds(-90.0, 90.0)
            .paint(|p| {
                p.map(MapResolution::Low, "green");
            });
        assert_eq!(
            world.accessible_text(80),
            "Canvas, x from -180 to 180, y from -90 to 90: a world map"
        );
        assert_eq!(
            Canvas::new().accessible_text(80),
            "Canvas, x from 0 to 1, y from 0 to 1: empty"
        );
    }

    fn console(width: usize) -> Console {
        Console::builder().width(width).color_system(None).build()
    }

    fn grid(canvas: &Canvas, width: usize) -> CellGrid {
        let console = console(width);
        let options = console.options();
        canvas.grid(&console, &options)
    }

    fn rows(canvas: &Canvas, width: usize) -> Vec<String> {
        let g = grid(canvas, width);
        (0..g.height).map(|r| g.row_string(r)).collect()
    }

    fn ascii(c: char) -> Canvas {
        Canvas::new()
            .x_bounds(0.0, 9.0)
            .y_bounds(0.0, 4.0)
            .height(5)
            .width(10)
            .marker(Marker::Ascii(c))
    }

    #[test]
    fn lines_crossing_the_bounds_are_clipped() {
        let b = Bounds::new([0.0, 10.0], [0.0, 10.0]).unwrap();
        assert_eq!(
            b.clip((-5.0, 5.0), (15.0, 5.0)),
            Some(((0.0, 5.0), (10.0, 5.0)))
        );
        assert_eq!(
            b.clip((5.0, -5.0), (5.0, 20.0)),
            Some(((5.0, 0.0), (5.0, 10.0)))
        );
        assert_eq!(
            b.clip((-10.0, -10.0), (20.0, 20.0)),
            Some(((0.0, 0.0), (10.0, 10.0)))
        );
        // Fully outside, beside and parallel.
        assert_eq!(b.clip((-5.0, 0.0), (-1.0, 10.0)), None);
        assert_eq!(b.clip((0.0, 11.0), (10.0, 11.0)), None);
        assert_eq!(b.clip((11.0, 0.0), (11.0, 10.0)), None);
        // Crossing a corner's outside.
        assert_eq!(b.clip((-5.0, 8.0), (2.0, 15.0)), None);
        // Non-finite.
        assert_eq!(b.clip((f64::NAN, 0.0), (1.0, 1.0)), None);
        assert_eq!(b.clip((f64::INFINITY, 0.0), (1.0, 1.0)), None);
    }

    #[test]
    fn horizontal_and_vertical_lines_cover_the_canvas() {
        let canvas = ascii('#').paint(|p| {
            p.line(-100.0, 2.0, 100.0, 2.0, "");
            p.line(4.0, -1e6, 4.0, 1e6, "");
        });
        assert_eq!(
            rows(&canvas, 10),
            vec![
                "    #     ",
                "    #     ",
                "##########",
                "    #     ",
                "    #     ",
            ]
        );
        // A huge line outside draws nothing and does not loop.
        let outside = ascii('#').paint(|p| {
            p.line(-1e300, -1e300, -1e299, 1e300, "");
        });
        assert!(rows(&outside, 10).iter().all(|r| r.trim().is_empty()));
    }

    #[test]
    fn non_finite_points_are_skipped() {
        let canvas = ascii('o').paint(|p| {
            p.points(&[(f64::NAN, 1.0), (1.0, f64::INFINITY), (0.0, 0.0)], "");
            p.polyline(&[(9.0, 4.0), (f64::NAN, 0.0), (9.0, 0.0)], "");
            p.circle(5.0, 2.0, f64::NAN, "");
            p.print(f64::NAN, 1.0, "x", "");
        });
        assert_eq!(
            rows(&canvas, 10),
            vec![
                "         o",
                "          ",
                "          ",
                "          ",
                "o        o",
            ]
        );
    }

    #[test]
    fn later_layers_win_a_cell() {
        let canvas = Canvas::new()
            .x_bounds(0.0, 1.0)
            .y_bounds(0.0, 1.0)
            .height(1)
            .width(1)
            .paint(|p| {
                p.points(&[(0.0, 1.0)], "red");
                p.points(&[(1.0, 1.0)], "blue");
            });
        let g = grid(&canvas, 1);
        // Within a layer the dots add up; the last writer's colour wins.
        assert_eq!(g.row_string(0), "⠉");
        assert_eq!(g.style_at(0, 0), Style::parse("blue").ok());

        let layered = canvas.clone().paint(|p| {
            p.points(&[(0.0, 0.0)], "green");
        });
        let g = grid(&layered, 1);
        assert_eq!(g.row_string(0), "⡀");
        assert_eq!(g.style_at(0, 0), Style::parse("green").ok());

        // A layer that draws nothing in a cell leaves it alone.
        let empty = canvas.paint(|p| {
            p.layer();
        });
        assert_eq!(grid(&empty, 1).row_string(0), "⠉");
    }

    #[test]
    fn markers_draw_at_their_resolution() {
        let draw = |marker| {
            let c = Canvas::new()
                .x_bounds(0.0, 1.0)
                .y_bounds(0.0, 1.0)
                .height(1)
                .width(2)
                .marker(marker)
                .paint(|p| {
                    p.points(&[(0.0, 1.0), (1.0, 0.0)], "red");
                });
            rows(&c, 2)[0].clone()
        };
        assert_eq!(draw(Marker::Braille), "⠁⢀");
        assert_eq!(draw(Marker::HalfBlock), "▀▄");
        assert_eq!(draw(Marker::Block), "██");
        assert_eq!(draw(Marker::Dot), "••");
        assert_eq!(draw(Marker::Ascii('x')), "xx");
        assert_eq!(draw(Marker::Ascii('日')), "**");
    }

    #[test]
    fn half_blocks_of_two_colours_share_a_cell() {
        let canvas = Canvas::new()
            .height(1)
            .width(1)
            .marker(Marker::HalfBlock)
            .paint(|p| {
                p.points(&[(0.0, 1.0)], "red");
                p.points(&[(0.0, 0.0)], "blue");
            });
        let g = grid(&canvas, 1);
        assert_eq!(g.row_string(0), "▀");
        assert_eq!(g.style_at(0, 0), Style::parse("red on blue").ok());
    }

    #[test]
    fn ascii_consoles_get_ascii_markers_and_text() {
        let console = Console::builder()
            .width(4)
            .color_system(None)
            .ascii_only(true)
            .build();
        let canvas = Canvas::new().height(1).paint(|p| {
            p.line(0.0, 0.5, 1.0, 0.5, "");
            p.print(0.0, 0.5, "日", "");
        });
        assert_eq!(console.render_export(&canvas), "??**\n");
        let blocks = canvas.marker(Marker::HalfBlock);
        assert_eq!(console.render_export(&blocks), "??##\n");
    }

    #[test]
    fn text_is_drawn_last_and_cut_at_the_edge() {
        let canvas = ascii('#').paint(|p| {
            p.print(7.0, 4.0, "label", "");
            p.layer();
            p.line(0.0, 4.0, 9.0, 4.0, "");
            p.print(0.0, 0.0, "日本", "");
            p.print(9.0, 0.0, "日", "");
        });
        assert_eq!(
            rows(&canvas, 10),
            vec![
                "#######lab",
                "          ",
                "          ",
                "          ",
                "日本      "
            ]
        );
    }

    #[test]
    fn circles_and_rectangles() {
        let canvas = Canvas::new()
            .x_bounds(-2.0, 2.0)
            .y_bounds(-2.0, 2.0)
            .height(5)
            .width(5)
            .marker(Marker::Ascii('o'))
            .paint(|p| {
                p.circle(0.0, 0.0, 2.0, "");
            });
        let out = rows(&canvas, 5);
        // Points map as ratatui maps them, truncated.
        assert_eq!(out[2], "o  oo");
        assert!(out[0].contains('o') && out[4].contains('o'));
    }

    #[test]
    fn every_marker_draws_a_diagonal() {
        let diagonal = |marker| {
            let canvas = Canvas::new().width(6).height(3).marker(marker).paint(|p| {
                p.line(0.0, 0.0, 1.0, 1.0, "");
            });
            rows(&canvas, 6)
        };
        let expect = |a: &str, b: &str| {
            vec![
                format!("    {a}{b}"),
                format!("  {a}{b}  "),
                format!("{a}{b}    "),
            ]
        };
        assert_eq!(diagonal(Marker::Braille), expect("⡠", "⠊"));
        assert_eq!(diagonal(Marker::HalfBlock), expect("▄", "▀"));
        assert_eq!(diagonal(Marker::Block), expect("█", "█"));
        assert_eq!(diagonal(Marker::Bar), expect("▄", "▄"));
        assert_eq!(diagonal(Marker::Quadrant), expect("▄", "▀"));
        // Sextants: the bottom left and middle right sixths, then the
        // middle left and top right ones.
        assert_eq!(diagonal(Marker::Sextant), expect("\u{1FB16}", "\u{1FB05}"));
        assert_eq!(diagonal(Marker::Octant), expect("\u{1CD51}", "\u{1CD01}"));
        assert_eq!(diagonal(Marker::Dot), expect("•", "•"));
        // ASCII consoles: `#` for the block markers.
        let console = Console::builder()
            .width(6)
            .color_system(None)
            .ascii_only(true)
            .build();
        for marker in [
            Marker::Bar,
            Marker::Quadrant,
            Marker::Sextant,
            Marker::Octant,
        ] {
            let canvas = Canvas::new().width(6).height(1).marker(marker).paint(|p| {
                p.line(0.0, 0.5, 1.0, 0.5, "");
            });
            assert_eq!(console.render_export(&canvas), "######\n", "{marker:?}");
        }
    }

    #[test]
    fn pattern_cells_take_the_last_style_drawn_in_them() {
        let canvas = Canvas::new()
            .width(1)
            .height(1)
            .marker(Marker::Quadrant)
            .paint(|p| {
                p.point(0.0, 1.0, "red");
                p.point(1.0, 0.0, "blue");
            });
        let g = grid(&canvas, 1);
        assert_eq!(g.row_string(0), "▚");
        assert_eq!(g.style_at(0, 0), Style::parse("blue").ok());
    }

    /// Whether any cell within a cell of (`col`, `row`) is drawn.
    fn near(rows: &[String], col: usize, row: usize) -> bool {
        (row.saturating_sub(1)..=row + 1).any(|r| {
            rows.get(r).is_some_and(|line| {
                line.chars()
                    .enumerate()
                    .any(|(c, ch)| c + 1 >= col && c <= col + 1 && ch != ' ')
            })
        })
    }

    #[test]
    fn the_map_lights_known_coasts() {
        for resolution in [MapResolution::Low, MapResolution::High] {
            let world = Canvas::new()
                .x_bounds(-180.0, 180.0)
                .y_bounds(-90.0, 90.0)
                .width(80)
                .height(20)
                .paint(|p| {
                    p.map(resolution, "green");
                });
            let out = rows(&world, 80);
            // The cell a longitude and latitude fall in, as the canvas maps
            // them (2 × 4 dots a cell).
            let cell = |lon: f64, lat: f64| {
                let x = ((lon + 180.0) * 159.0 / 360.0) as usize / 2;
                let y = ((90.0 - lat) * 79.0 / 180.0) as usize / 4;
                (x, y)
            };
            for (place, lon, lat) in [
                ("Sydney", 151.2, -33.9),
                ("Perth", 115.9, -31.9),
                ("Darwin", 130.8, -12.5),
                ("Cape Town", 18.4, -33.9),
                ("Lisbon", -9.1, 38.7),
                ("Rio de Janeiro", -43.2, -22.9),
                ("San Francisco", -122.4, 37.8),
            ] {
                let (x, y) = cell(lon, lat);
                assert!(near(&out, x, y), "{resolution:?}: {place} at {x}, {y}");
            }
            // The open Pacific and the middle of Australia are blank.
            for (lon, lat) in [(-140.0, -10.0), (-140.0, 20.0), (134.0, -25.0)] {
                let (x, y) = cell(lon, lat);
                assert_eq!(
                    out[y].chars().nth(x),
                    Some(' '),
                    "{resolution:?} {lon} {lat}"
                );
            }
            // The map draws in its style.
            let g = grid(&world, 80);
            let (x, y) = cell(151.2, -33.9);
            let drawn = (y - 1..=y + 1)
                .flat_map(|r| (x - 1..=x + 1).map(move |c| (r, c)))
                .find(|&(r, c)| g.row_string(r).chars().nth(c) != Some(' '))
                .unwrap();
            assert_eq!(g.style_at(drawn.0, drawn.1), Style::parse("green").ok());
        }
    }

    #[test]
    fn a_background_goes_under_everything() {
        let canvas = Canvas::new()
            .width(3)
            .height(1)
            .marker(Marker::Block)
            .background("navy_blue")
            .paint(|p| {
                p.point(0.0, 0.5, "red");
            });
        let g = grid(&canvas, 3);
        assert_eq!(g.row_string(0), "█  ");
        assert_eq!(g.style_at(0, 0), Style::parse("red on navy_blue").ok());
        assert_eq!(g.style_at(0, 1), Style::parse("on navy_blue").ok());
        // In a styled export, every cell carries the background.
        let console = Console::builder()
            .width(3)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .no_color(false)
            .build();
        let canvas = canvas.background("on blue");
        assert_eq!(
            console.render_export(&canvas),
            "\u{1b}[31;44m█\u{1b}[0m\u{1b}[44m  \u{1b}[0m\n"
        );
    }

    #[test]
    fn areas_fill_down_to_their_level() {
        let canvas = Canvas::new()
            .x_bounds(0.0, 4.0)
            .y_bounds(0.0, 4.0)
            .width(5)
            .height(5)
            .marker(Marker::Ascii('#'))
            .paint(|p| {
                p.area(0.0, 1.0, 4.0, 3.0, 0.0, "");
            });
        assert_eq!(
            rows(&canvas, 5),
            vec!["     ", "   ##", " ####", "#####", "#####"]
        );
        // Above the level the fill goes up to it.
        let above = Canvas::new()
            .x_bounds(0.0, 4.0)
            .y_bounds(0.0, 4.0)
            .width(5)
            .height(5)
            .marker(Marker::Ascii('#'))
            .paint(|p| {
                p.area(0.0, 1.0, 4.0, 1.0, 3.0, "");
            });
        assert_eq!(
            rows(&above, 5),
            vec!["     ", "#####", "#####", "#####", "     "]
        );
    }

    #[test]
    fn custom_shapes_draw_with_the_painter() {
        struct Square(f64);
        impl Shape for Square {
            fn draw(&self, painter: &mut CanvasPainter) {
                let s = self.0;
                painter
                    .rect(-s, -s, 2.0 * s, 2.0 * s, "yellow")
                    .point(0.0, 0.0, "red");
            }
        }
        let canvas = Canvas::new()
            .x_bounds(-2.0, 2.0)
            .y_bounds(-2.0, 2.0)
            .width(5)
            .height(5)
            .marker(Marker::Ascii('o'))
            .paint(|p| {
                p.shape(&Square(1.0));
            });
        assert_eq!(
            rows(&canvas, 5),
            vec!["     ", " ooo ", " ooo ", " ooo ", "     "]
        );
        let g = grid(&canvas, 5);
        assert_eq!(g.style_at(2, 2), Style::parse("red").ok());
        assert_eq!(g.style_at(1, 1), Style::parse("yellow").ok());
    }

    #[test]
    fn a_canvas_fills_the_height_it_is_given() {
        let console = console(10);
        let canvas = Canvas::new().paint(|p| {
            p.line(0.0, 0.0, 1.0, 1.0, "");
        });
        let lines = console.render_lines(&canvas, &console.options().update_height(7), false);
        assert_eq!(lines.len(), 7);
        assert!(lines.iter().all(|l| !l.is_empty()));
        // Its own height wins; with neither, ten rows.
        let short = canvas.clone().height(3);
        let lines = console.render_lines(&short, &console.options().update_height(7), false);
        assert_eq!(lines.iter().filter(|l| !l.is_empty()).count(), 3);
        assert_eq!(console.render_export(&canvas).lines().count(), 10);
    }

    #[test]
    fn tiny_and_empty_canvases_do_not_panic() {
        let paint = |p: &mut CanvasPainter| {
            p.line(0.0, 0.0, 1.0, 1.0, "red");
            p.circle(0.5, 0.5, 0.5, "red");
            p.print(0.5, 0.5, "text", "");
        };
        for (w, h) in [(0, 0), (1, 0), (0, 1), (1, 1), (2, 1)] {
            for marker in [Marker::Braille, Marker::HalfBlock, Marker::Dot] {
                let c = Canvas::new().width(w).height(h).marker(marker).paint(paint);
                let console = console(10);
                let _ = console.render_export(&c);
            }
        }
        // Empty, reversed and infinite bounds draw nothing.
        for (x, y) in [
            ([0.0, 0.0], [0.0, 1.0]),
            ([1.0, 0.0], [0.0, 1.0]),
            ([0.0, f64::INFINITY], [0.0, 1.0]),
            ([-f64::MAX, f64::MAX], [0.0, 1.0]),
        ] {
            let c = Canvas::new()
                .x_bounds(x[0], x[1])
                .y_bounds(y[0], y[1])
                .height(2)
                .paint(paint);
            assert!(rows(&c, 4).iter().all(|r| r.trim().is_empty()));
        }
    }
}

//! ratatui interop, both ways.
//!
//! - **rich → ratatui**: [`RichWidget`] is a ratatui [`Widget`] that draws
//!   any [`rich::Renderable`] (a `Table`, a `Panel`, markup, a syntax
//!   block) into a ratatui [`Buffer`], so a ratatui app can adopt rich's
//!   renderables one widget at a time.
//! - **ratatui → rich-interact**: [`RatatuiComponent`] is a
//!   [`rich_interact::Component`] whose view is drawn by ratatui widgets, so
//!   an existing ratatui widget runs unchanged inside a rich-interact event
//!   loop (and under the headless driver, for tests).
//!
//! Underneath both sit two converters, useful on their own:
//! [`lines_to_buffer`] (rich lines into buffer cells) and
//! [`buffer_to_lines`] (buffer cells into rich lines), and the style mapping
//! [`to_ratatui_style`] / [`to_rich_style`].
//!
//! # What does not survive the trip
//!
//! The mapping is lossless for the common subset: no colour, the terminal
//! default (`Color::Reset` ⇄ rich's `default`), the 16 named colours, the
//! 256-colour palette, 24-bit RGB, and nine attributes (bold, dim, italic,
//! underline, blink, rapid blink, reverse, conceal/hidden, strike), each
//! tri-state (on, explicitly off, unset). It loses:
//!
//! - rich's **colour names**: `grey0` comes back as `color(16)`, `#FF0000`
//!   as `#ff0000`. The colour (kind, number, RGB) is the same; only
//!   `Color::name` differs.
//! - rich's legacy **Windows** colours, which come back as the standard
//!   colour with the same number.
//! - rich's **`underline2`, `frame`, `encircle`, `overline`**: ratatui has
//!   no modifier for them; they are dropped going to ratatui.
//! - rich's **hyperlinks** (OSC 8) and **meta**: a ratatui cell holds a
//!   symbol and a style, nothing else. Smuggling the escape sequence into
//!   the symbol (with `CellDiffOption::ForcedWidth`) is possible but
//!   fragile, so links are dropped and their text kept.
//! - ratatui's **underline colour** (the `underline-color` feature): rich
//!   has no such colour; it is dropped going to rich.
//! - In a *buffer*, `Color::Reset` and "no colour" are the same thing (a
//!   cell always has a colour; a fresh one has `Reset`), so
//!   [`buffer_to_lines`] reads `Reset` as "unset", not as rich's `default`.

#![allow(dead_code)]

use std::cell::OnceCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use ratatui::buffer::{Buffer, CellWidth};
use ratatui::layout::Rect;
use ratatui::style::{Color as RColor, Modifier, Style as RStyle};
use ratatui::widgets::Widget;
use rich::color::ColorType;
use rich::{Color, ColorSystem, Console, Renderable, Segment, Style};
use rich_interact::{Component, Context, Event, Flow, View};

// ---------------------------------------------------------------------------
// Style mapping
// ---------------------------------------------------------------------------

/// rich's attribute indices (see `Style::attr`) and the ratatui modifier
/// each maps to. The four rich attributes missing here (`underline2`,
/// `frame`, `encircle`, `overline`: indices 9–12) have no ratatui modifier.
const ATTRIBUTES: [(usize, &str, Modifier); 9] = [
    (0, "bold", Modifier::BOLD),
    (1, "dim", Modifier::DIM),
    (2, "italic", Modifier::ITALIC),
    (3, "underline", Modifier::UNDERLINED),
    (4, "blink", Modifier::SLOW_BLINK),
    (5, "blink2", Modifier::RAPID_BLINK),
    (6, "reverse", Modifier::REVERSED),
    (7, "conceal", Modifier::HIDDEN),
    (8, "strike", Modifier::CROSSED_OUT),
];

/// The 16 standard colours in ANSI number order, as ratatui names them.
const NAMED: [RColor; 16] = [
    RColor::Black,
    RColor::Red,
    RColor::Green,
    RColor::Yellow,
    RColor::Blue,
    RColor::Magenta,
    RColor::Cyan,
    RColor::Gray,
    RColor::DarkGray,
    RColor::LightRed,
    RColor::LightGreen,
    RColor::LightYellow,
    RColor::LightBlue,
    RColor::LightMagenta,
    RColor::LightCyan,
    RColor::White,
];

/// The same 16, as rich names them (`rich/color.py`'s `ANSI_COLOR_NAMES`).
const RICH_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "bright_black",
    "bright_red",
    "bright_green",
    "bright_yellow",
    "bright_blue",
    "bright_magenta",
    "bright_cyan",
    "bright_white",
];

/// Map a rich colour to ratatui's. rich's `default` is ratatui's `Reset`;
/// a standard (or legacy Windows) colour is ratatui's named colour with the
/// same number; the 256 palette is `Indexed`, even below 16, so an indexed
/// colour round-trips as indexed; truecolor is `Rgb`.
pub fn to_ratatui_color(color: &Color) -> RColor {
    match (color.kind, color.number, color.triplet) {
        (ColorType::Default, _, _) => RColor::Reset,
        (ColorType::Standard | ColorType::Windows, Some(n), _) => NAMED[usize::from(n & 15)],
        (ColorType::EightBit, Some(n), _) => RColor::Indexed(n),
        (_, _, Some(t)) => RColor::Rgb(t.red, t.green, t.blue),
        // A colour with neither a number nor a triplet is malformed; the
        // terminal default is the harmless reading.
        _ => RColor::Reset,
    }
}

/// Map a ratatui colour to rich's: the inverse of [`to_ratatui_color`].
/// Names are rich's canonical ones (`bright_red`, `color(16)`, `#rrggbb`).
pub fn to_rich_color(color: RColor) -> Color {
    match color {
        RColor::Reset => Color::default_color(),
        RColor::Indexed(n) => Color {
            name: format!("color({n})"),
            kind: ColorType::EightBit,
            number: Some(n),
            triplet: None,
        },
        RColor::Rgb(r, g, b) => Color::from_rgb(r, g, b),
        named => {
            let n = NAMED.iter().position(|c| *c == named).unwrap_or(0);
            Color {
                name: RICH_NAMES[n].to_string(),
                kind: ColorType::Standard,
                number: Some(n as u8),
                triplet: None,
            }
        }
    }
}

/// Map a rich style to a ratatui style. Unset stays unset (`None` colour,
/// modifier in neither set), so the result *patches* whatever a cell had,
/// as rich's own `combine` does. An attribute explicitly off (`not bold`)
/// goes to `sub_modifier`. Links, meta and the four attributes ratatui
/// lacks are dropped (see the [module docs](self)).
pub fn to_ratatui_style(style: &Style) -> RStyle {
    let mut out = RStyle {
        fg: style.color().map(to_ratatui_color),
        bg: style.bgcolor().map(to_ratatui_color),
        ..RStyle::default()
    };
    for (index, _, modifier) in ATTRIBUTES {
        match style.attr(index) {
            Some(true) => out.add_modifier |= modifier,
            Some(false) => out.sub_modifier |= modifier,
            None => {}
        }
    }
    out
}

/// Map a ratatui style to a rich style, or `None` when it sets nothing.
/// The inverse of [`to_ratatui_style`]; ratatui's underline colour is
/// dropped.
pub fn to_rich_style(style: RStyle) -> Option<Style> {
    let mut words = Vec::new();
    for (_, name, modifier) in ATTRIBUTES {
        if style.add_modifier.contains(modifier) {
            words.push(name.to_string());
        } else if style.sub_modifier.contains(modifier) {
            words.push(format!("not {name}"));
        }
    }
    if words.is_empty() && style.fg.is_none() && style.bg.is_none() {
        return None;
    }
    // rich has no public per-attribute setter, so the attributes go through
    // its parser (cached by `buffer_to_lines`, so this is off the hot path).
    let mut out = Style::parse(&words.join(" ")).unwrap_or_default();
    if let Some(fg) = style.fg {
        out = out.with_color(to_rich_color(fg));
    }
    if let Some(bg) = style.bg {
        out = out.with_bgcolor(to_rich_color(bg));
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// rich lines → ratatui buffer
// ---------------------------------------------------------------------------

/// Write rich lines into `area` of `buffer`: line *i* on row `area.y + i`,
/// at most `area.height` lines, each cropped to `area.width` columns.
///
/// Each segment is placed at the column rich measured for it
/// ([`rich::cells::cell_len`]), not where ratatui's own width table would
/// put it, so the two disagreeing about a character (some emoji) shifts
/// nothing after that segment. A wide character that would straddle the
/// right edge is replaced by a space, as rich's own cropping does, and
/// cells left over in a segment are filled with spaces in its style.
///
/// Styles *patch* the cells (ratatui's convention, as `Paragraph` does):
/// an unstyled segment keeps the background a `Block` painted beneath it.
/// Cells right of a short line are left untouched. Control segments are
/// skipped.
pub fn lines_to_buffer(lines: &[Vec<Segment>], area: Rect, buffer: &mut Buffer) {
    let area = area.intersection(buffer.area);
    let right = area.right();
    for (line, y) in lines.iter().zip(area.top()..area.bottom()) {
        let mut x = area.x;
        for segment in line {
            if x >= right {
                break;
            }
            if segment.control || segment.text.is_empty() {
                continue;
            }
            let style = segment
                .style
                .as_ref()
                .map(to_ratatui_style)
                .unwrap_or_default();
            let width = rich::cells::cell_len(&segment.text);
            let end = right.min(x.saturating_add(width.min(usize::from(u16::MAX)) as u16));
            let (written, _) = buffer.set_stringn(x, y, &segment.text, usize::from(end - x), style);
            // Pad what ratatui did not fill: a wide char cut at the edge, or
            // a character ratatui measures narrower than rich does.
            for pad in written..end {
                buffer[(pad, y)].set_symbol(" ").set_style(style);
            }
            x = end;
        }
    }
}

thread_local! {
    /// The console a [`RichWidget`] renders with when given none. Built
    /// once per thread with everything set explicitly, so building it reads
    /// no environment (`NO_COLOR`, terminal detection): the colours a
    /// ratatui app shows are the app's business, decided by its backend.
    static DEFAULT_CONSOLE: OnceCell<Console> = const { OnceCell::new() };
}

fn with_default_console<R>(f: impl FnOnce(&Console) -> R) -> R {
    DEFAULT_CONSOLE.with(|cell| {
        f(cell.get_or_init(|| {
            Console::builder()
                .width(80)
                .height(25)
                .force_terminal(true)
                .color_system(Some(ColorSystem::Truecolor))
                .no_color(false)
                .build()
        }))
    })
}

/// What a [`RichWidget`] draws: a borrowed renderable or one it owns.
enum Content<'a> {
    Borrowed(&'a dyn Renderable),
    Owned(Box<dyn Renderable + 'a>),
}

/// A ratatui widget drawing a rich renderable.
///
/// ```ignore
/// let table = rich::Table::new();
/// frame.render_widget(&RichWidget::new(&table), area);
/// frame.render_widget(RichWidget::markup("[bold]hi[/] there"), area);
/// ```
///
/// The renderable is rendered at the area's width (with the given console,
/// or a default one; see [`RichWidget::console`]); the first `area.height`
/// lines are drawn, and the rest cut off, as ratatui's `Paragraph` does.
pub struct RichWidget<'a> {
    content: Content<'a>,
    console: Option<&'a Console>,
    fill_height: bool,
}

impl<'a> RichWidget<'a> {
    /// Draw a borrowed renderable.
    pub fn new(renderable: &'a dyn Renderable) -> RichWidget<'a> {
        RichWidget {
            content: Content::Borrowed(renderable),
            console: None,
            fill_height: false,
        }
    }

    /// Draw a renderable the widget owns.
    pub fn owned(renderable: impl Renderable + 'a) -> RichWidget<'a> {
        RichWidget {
            content: Content::Owned(Box::new(renderable)),
            console: None,
            fill_height: false,
        }
    }

    /// Draw console markup (`[bold red]hi[/]`). Markup that does not parse
    /// is drawn as plain text, as rich-interact's `Context::markup` does.
    pub fn markup(markup: &str) -> RichWidget<'static> {
        let text = rich::Text::from_markup(markup).unwrap_or_else(|_| rich::Text::new(markup));
        RichWidget::owned(text)
    }

    /// Render with `console`: its theme, highlighting, emoji and box
    /// settings. Its width is ignored (the area's is used). Without one, a
    /// per-thread default console is used: default theme, truecolor,
    /// highlighting on, as upstream's `Console()` defaults.
    pub fn console(mut self, console: &'a Console) -> RichWidget<'a> {
        self.console = Some(console);
        self
    }

    /// Also tell the renderable the area's height, so renderables that use
    /// it (`Panel`, `Layout`) fill the area instead of taking their natural
    /// height. Off by default, matching rich-interact's `Context::lines`.
    pub fn fill_height(mut self, fill: bool) -> RichWidget<'a> {
        self.fill_height = fill;
        self
    }

    fn renderable(&self) -> &dyn Renderable {
        match &self.content {
            Content::Borrowed(r) => *r,
            Content::Owned(r) => r.as_ref(),
        }
    }

    /// The rich lines this widget draws into `area`, at most `area.height`.
    pub fn lines(&self, area: Rect) -> Vec<Vec<Segment>> {
        let render = |console: &Console| {
            let width = usize::from(area.width).max(1);
            let options = if self.fill_height {
                console
                    .options()
                    .update_dimensions(width, usize::from(area.height))
            } else {
                console.options().update_width(width)
            };
            let mut lines = console.render_lines(self.renderable(), &options, false);
            lines.truncate(usize::from(area.height));
            lines
        };
        match self.console {
            Some(console) => render(console),
            None => with_default_console(render),
        }
    }
}

impl Widget for &RichWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        lines_to_buffer(&self.lines(area), area, buf);
    }
}

impl Widget for RichWidget<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        (&self).render(area, buf);
    }
}

// ---------------------------------------------------------------------------
// ratatui buffer → rich lines
// ---------------------------------------------------------------------------

/// A cell's style, as `buffer_to_lines` groups cells by it.
type CellKey = (RColor, RColor, Modifier);

/// A cell's style as rich sees it. `Reset` is "unset" here, not rich's
/// `default`: every cell has a colour, and a fresh one has `Reset`, so
/// reading it as `default` would style every blank cell. A cell with
/// nothing set gets no style at all.
fn cell_style((fg, bg, modifier): CellKey) -> Option<Style> {
    let unset = |c: RColor| (c != RColor::Reset).then_some(c);
    to_rich_style(RStyle {
        fg: unset(fg),
        bg: unset(bg),
        add_modifier: modifier,
        ..RStyle::default()
    })
}

/// Convert every row of `buffer` into a rich line, full width (nothing is
/// trimmed, so a view keeps a stable layout).
///
/// Runs of cells with the same style become one [`Segment`]. A wide
/// symbol's trailing cells are skipped: ratatui does not mark them (in
/// 0.30 `set_stringn` just resets them to blank cells), so they are found
/// as ratatui's own diff finds them, by the symbol's width. A wide symbol
/// that would run past the right edge becomes spaces, and a zero-width
/// symbol a space, so every line is exactly `area.width` cells. The
/// underline colour is dropped.
pub fn buffer_to_lines(buffer: &Buffer) -> Vec<Vec<Segment>> {
    let width = usize::from(buffer.area.width);
    let mut styles: HashMap<CellKey, Option<Style>> = HashMap::new();
    let mut lines = Vec::with_capacity(usize::from(buffer.area.height));
    if width == 0 {
        lines.resize(usize::from(buffer.area.height), Vec::new());
        return lines;
    }
    for row in buffer.content.chunks(width) {
        let mut line = Vec::new();
        let mut text = String::with_capacity(width);
        let mut current: Option<CellKey> = None;
        let mut x = 0;
        while x < width {
            let cell = &row[x];
            let key = (cell.fg, cell.bg, cell.modifier);
            if current != Some(key) {
                if let Some(previous) = current {
                    let style = styles
                        .entry(previous)
                        .or_insert_with(|| cell_style(previous));
                    line.push(Segment::new(std::mem::take(&mut text), style.clone()));
                }
                current = Some(key);
            }
            let symbol = cell.symbol();
            let cells = usize::from(symbol.cell_width());
            if cells == 0 {
                text.push(' ');
                x += 1;
            } else if x + cells > width {
                text.extend(std::iter::repeat_n(' ', width - x));
                x = width;
            } else {
                text.push_str(symbol);
                x += cells;
            }
        }
        if let Some(previous) = current {
            let style = styles
                .entry(previous)
                .or_insert_with(|| cell_style(previous));
            line.push(Segment::new(text, style.clone()));
        }
        lines.push(line);
    }
    lines
}

// ---------------------------------------------------------------------------
// ratatui widgets inside rich-interact
// ---------------------------------------------------------------------------

type Draw<S> = Box<dyn Fn(&S, Rect, &mut Buffer)>;
type Handler<S, T> = Box<dyn FnMut(&mut S, &Event) -> Flow<T>>;

/// A rich-interact component whose view is drawn by ratatui.
///
/// It holds a state `S`, a draw function that renders it with ratatui
/// widgets into a buffer, and an optional event handler that updates it,
/// so the state lives in the component and not behind an `Rc<RefCell>`:
///
/// ```ignore
/// let counter = RatatuiComponent::with_state(0u32, |n, area, buf| {
///     Paragraph::new(format!("{n}")).block(Block::bordered()).render(area, buf);
/// })
/// .on_event(|n, event| match event {
///     Event::Key(key) if key.code == KeyCode::Enter => Flow::Done(*n),
///     Event::Key(_) => { *n += 1; Flow::Continue }
///     _ => Flow::Ignored,
/// });
/// ```
///
/// On render the draw function gets a fresh `Buffer` the size of the
/// context (or [`height`](Self::height) rows), and the buffer becomes the
/// view through [`buffer_to_lines`]. Without a handler every event is
/// [`Flow::Ignored`], so a container can use it.
pub struct RatatuiComponent<S, T> {
    state: S,
    draw: Draw<S>,
    handler: Option<Handler<S, T>>,
    height: Option<u16>,
}

impl<T> RatatuiComponent<(), T> {
    /// A stateless component: `draw` gets only the area and buffer.
    pub fn new(draw: impl Fn(Rect, &mut Buffer) + 'static) -> RatatuiComponent<(), T> {
        RatatuiComponent::with_state((), move |_, area, buf| draw(area, buf))
    }
}

impl<S, T> RatatuiComponent<S, T> {
    /// A component drawing `state` with `draw`.
    pub fn with_state(
        state: S,
        draw: impl Fn(&S, Rect, &mut Buffer) + 'static,
    ) -> RatatuiComponent<S, T> {
        RatatuiComponent {
            state,
            draw: Box::new(draw),
            handler: None,
            height: None,
        }
    }

    /// Handle events with `handler`, which may update the state.
    pub fn on_event(
        mut self,
        handler: impl FnMut(&mut S, &Event) -> Flow<T> + 'static,
    ) -> RatatuiComponent<S, T> {
        self.handler = Some(Box::new(handler));
        self
    }

    /// Draw `rows` rows instead of the context's full height: for an inline
    /// widget that should not take the whole terminal.
    pub fn height(mut self, rows: u16) -> RatatuiComponent<S, T> {
        self.height = Some(rows);
        self
    }

    pub fn state(&self) -> &S {
        &self.state
    }

    /// Draw into a fresh buffer of `width` × `height` and return it.
    pub fn draw(&self, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        (self.draw)(&self.state, area, &mut buffer);
        buffer
    }
}

/// Clamp a context dimension to ratatui's `u16`.
fn dimension(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl<S, T> Component for RatatuiComponent<S, T> {
    type Output = T;

    fn handle(&mut self, event: &Event, _context: &Context<'_>) -> Flow<T> {
        match &mut self.handler {
            Some(handler) => handler(&mut self.state, event),
            None => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let height = self.height.unwrap_or(dimension(context.height));
        View::new(buffer_to_lines(
            &self.draw(dimension(context.width), height),
        ))
    }
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

/// A 60-row table, the bench's rich side.
fn bench_table() -> rich::Table {
    let mut table = rich::Table::new().title("Inventory");
    table.add_column("id");
    table.add_column("name");
    table.add_column("status");
    table.add_column("amount");
    for i in 0..60 {
        let id = i.to_string();
        let name = format!("item number {i}");
        let status = ["[green]ok[/]", "[bold red]failed[/]", "[yellow]pending[/]"][i % 3];
        let amount = format!("{:.2}", i as f64 * 3.17);
        table.add_row(&[&id, &name, status, &amount]);
    }
    table
}

/// A 200×50 buffer with a style change every few cells and some wide
/// characters, the bench's ratatui side.
fn bench_buffer() -> Buffer {
    let area = Rect::new(0, 0, 200, 50);
    let mut buffer = Buffer::empty(area);
    for y in 0..area.height {
        let mut x = 0;
        let mut run = 0u8;
        while x < area.width {
            let style = RStyle::default()
                .fg(RColor::Indexed(run.wrapping_mul(7)))
                .add_modifier(if run.is_multiple_of(2) {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                });
            let text = if run.is_multiple_of(5) {
                "日本語"
            } else {
                "word "
            };
            let (end, _) = buffer.set_stringn(x, y, text, usize::from(area.width - x), style);
            x = end.max(x + 1);
            run = run.wrapping_add(1);
        }
    }
    buffer
}

/// Average time of (a) drawing a 60-row rich table into a 100×40 buffer
/// through [`RichWidget`], and (b) converting a 200×50 buffer with
/// [`buffer_to_lines`], over `iterations` runs each.
pub fn measure(iterations: u32) -> (Duration, Duration) {
    let table = bench_table();
    let widget = RichWidget::new(&table);
    let area = Rect::new(0, 0, 100, 40);
    let start = Instant::now();
    for _ in 0..iterations {
        let mut buffer = Buffer::empty(area);
        (&widget).render(area, &mut buffer);
        std::hint::black_box(&buffer);
    }
    let draw = start.elapsed() / iterations;

    let buffer = bench_buffer();
    let start = Instant::now();
    for _ in 0..iterations {
        std::hint::black_box(buffer_to_lines(std::hint::black_box(&buffer)));
    }
    let convert = start.elapsed() / iterations;
    (draw, convert)
}

/// Print [`measure`] over 200 iterations. Build with `--release` for
/// meaningful numbers.
pub fn bench() {
    let (draw, convert) = measure(200);
    println!("RichWidget: 60-row Table into a 100x40 Buffer: {draw:?} per frame");
    println!("buffer_to_lines: 200x50 Buffer to lines:       {convert:?} per frame");
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::widgets::{Block, Paragraph};
    use rich::Panel;
    use rich_interact::headless::{self, Script};
    use rich_interact::{KeyCode, Outcome};

    /// A console like the default one, but fixed at `width` for the
    /// reference renderings.
    fn console(width: usize) -> Console {
        Console::builder()
            .width(width)
            .height(25)
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .no_color(false)
            .build()
    }

    fn plain(lines: &[Vec<Segment>]) -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect()
    }

    fn buffer_rows(buffer: &Buffer) -> Vec<String> {
        plain(&buffer_to_lines(buffer))
    }

    /// Lines as (text, SGR) runs, adjacent runs with the same SGR merged:
    /// what a terminal would show, whatever the segmentation and colour
    /// names.
    fn normalise(lines: &[Vec<Segment>]) -> Vec<Vec<(String, String)>> {
        lines
            .iter()
            .map(|line| {
                let mut runs: Vec<(String, String)> = Vec::new();
                for segment in line.iter().filter(|s| !s.control && !s.text.is_empty()) {
                    let sgr = segment
                        .style
                        .as_ref()
                        .map(|s| s.ansi_codes(ColorSystem::Truecolor))
                        .unwrap_or_default();
                    match runs.last_mut() {
                        Some((text, last)) if *last == sgr => text.push_str(&segment.text),
                        _ => runs.push((segment.text.clone(), sgr)),
                    }
                }
                runs
            })
            .collect()
    }

    /// Two rich colours are the same colour (names aside).
    fn same_color(a: &Color, b: &Color) -> bool {
        let kind = |c: &Color| match c.kind {
            ColorType::Windows => ColorType::Standard,
            k => k,
        };
        kind(a) == kind(b) && a.number == b.number && a.triplet == b.triplet
    }

    #[test]
    fn colours_round_trip() {
        let mut colours = vec![RColor::Reset];
        colours.extend(NAMED);
        colours.extend((0..=255).map(RColor::Indexed));
        colours.extend([RColor::Rgb(0, 0, 0), RColor::Rgb(255, 128, 1)]);
        for colour in colours {
            assert_eq!(to_ratatui_color(&to_rich_color(colour)), colour);
            let style = RStyle::default().fg(colour).bg(colour);
            assert_eq!(to_ratatui_style(&to_rich_style(style).unwrap()), style);
        }
        // rich → ratatui → rich, from rich's own parser.
        for spec in [
            "default",
            "red",
            "bright_cyan",
            "grey0",
            "color(200)",
            "#ff8001",
        ] {
            let colour = Color::parse(spec).unwrap();
            let back = to_rich_color(to_ratatui_color(&colour));
            assert!(same_color(&colour, &back), "{spec}: {colour:?} vs {back:?}");
        }
    }

    #[test]
    fn modifiers_round_trip() {
        for (index, name, modifier) in ATTRIBUTES {
            let on = Style::parse(name).unwrap();
            assert_eq!(on.attr(index), Some(true));
            let ratatui = to_ratatui_style(&on);
            assert_eq!(ratatui, RStyle::default().add_modifier(modifier));
            assert_eq!(to_rich_style(ratatui), Some(on));

            let off = Style::parse(&format!("not {name}")).unwrap();
            let ratatui = to_ratatui_style(&off);
            assert_eq!(ratatui, RStyle::default().remove_modifier(modifier));
            assert_eq!(to_rich_style(ratatui), Some(off));
        }
        // All at once, with colours.
        let all = Style::parse(
            "bold dim italic underline blink blink2 reverse conceal strike red on #102030",
        )
        .unwrap();
        let back = to_rich_style(to_ratatui_style(&all)).unwrap();
        assert_eq!(to_ratatui_style(&back), to_ratatui_style(&all));
        assert_eq!(
            back.ansi_codes(ColorSystem::Truecolor),
            all.ansi_codes(ColorSystem::Truecolor)
        );
        // Nothing set is no style.
        assert_eq!(to_rich_style(RStyle::default()), None);
        // The lossy ones: dropped, not mistranslated.
        let lossy = Style::parse("underline2 frame encircle overline").unwrap();
        assert_eq!(to_ratatui_style(&lossy), RStyle::default());
    }

    #[test]
    fn rich_widget_matches_richs_plain_rendering() {
        let mut table = rich::Table::new();
        table.add_column("name");
        table.add_column("qty");
        table.add_row(&["apples", "3"]);
        table.add_row(&["pears", "12"]);
        let area = Rect::new(0, 0, 30, 6);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&table).render(area, &mut buffer);

        let expected = console(30).export_text(|c| c.print(&table));
        let expected: Vec<&str> = expected.lines().collect();
        assert_eq!(expected.len(), 6, "{expected:?}");
        // The table is not expanded, so rich's lines are 16 cells and the
        // buffer's other 14 columns are untouched blanks.
        let rows = buffer_rows(&buffer);
        assert_eq!(
            rows.iter().map(|r| r.trim_end()).collect::<Vec<_>>(),
            expected
        );

        // A panel expands: full-width lines, compared as they are.
        let panel = Panel::new(Box::new(table)).title("stock");
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&panel).render(area, &mut buffer);
        let expected = console(30).export_text(|c| c.print(&panel));
        let expected: Vec<&str> = expected.lines().take(6).collect();
        assert_eq!(buffer_rows(&buffer), expected);
    }

    #[test]
    fn rich_widget_crops_to_the_area_and_patches_styles() {
        let panel = Panel::new(Box::new(rich::Text::new("one\ntwo\nthree\nfour")));
        // Offset area inside a larger buffer with a blue background: the
        // panel shows its first 3 lines only, and keeps the background.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 5));
        buffer.set_style(buffer.area, RStyle::default().bg(RColor::Blue));
        RichWidget::new(&panel).render(Rect::new(1, 1, 10, 3), &mut buffer);
        let rows = buffer_rows(&buffer);
        assert_eq!(rows[0], " ".repeat(12));
        assert_eq!(rows[1], " ╭────────╮ ");
        assert_eq!(rows[3], " │ two    │ ");
        assert_eq!(rows[4], " ".repeat(12));
        assert_eq!(buffer[(5, 2)].bg, RColor::Blue);
    }

    #[test]
    fn wide_characters_round_trip() {
        let text = rich::Text::new("日本語 ok 🎉");
        let area = Rect::new(0, 0, 20, 1);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&text).render(area, &mut buffer);
        // The wide chars' trailing cells are blank in the buffer...
        assert_eq!(buffer[(0, 0)].symbol(), "日");
        assert_eq!(buffer[(1, 0)].symbol(), " ");
        // ...and skipped coming back: same text, exactly 20 cells.
        let lines = buffer_to_lines(&buffer);
        let row = &plain(&lines)[0];
        assert_eq!(row.trim_end(), "日本語 ok 🎉");
        assert_eq!(rich::cells::cell_len(row), 20);
    }

    #[test]
    fn wide_character_at_the_last_column_does_not_overflow() {
        // Through lines_to_buffer: rich's line is wider than the area.
        let area = Rect::new(0, 0, 4, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 1));
        buffer.set_string(4, 0, "xy", RStyle::default());
        let line = vec![Segment::new("abc日", Some(Style::parse("bold").unwrap()))];
        lines_to_buffer(&[line], area, &mut buffer);
        assert_eq!(buffer_rows(&buffer), ["abc xy"]);
        assert!(buffer[(3, 0)].modifier.contains(Modifier::BOLD));

        // Through buffer_to_lines: a wide symbol set by hand in the last
        // column becomes a space, so the line stays 3 cells.
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        buffer[(2, 0)].set_symbol("日");
        assert_eq!(buffer_rows(&buffer), ["   "]);
    }

    #[test]
    fn panel_round_trips_with_styles() {
        let body = rich::Text::from_markup(
            "[bold red]alert[/] [italic on blue]note[/] [color(200)]日本[/] [#ff8001 u]rgb[/]",
        )
        .unwrap();
        let panel = Panel::new(Box::new(body))
            .title("[b]demo[/]")
            .border_style("green");
        let console = console(30);
        let options = console.options().update_width(30);
        let original = console.render_lines(&panel, &options, false);

        let area = Rect::new(0, 0, 30, original.len() as u16);
        let mut buffer = Buffer::empty(area);
        RichWidget::new(&panel)
            .console(&console)
            .render(area, &mut buffer);
        let back = buffer_to_lines(&buffer);
        let expected = normalise(&original);
        // The comparison has teeth: styled runs, borders included.
        let styled: Vec<&str> = expected
            .iter()
            .flatten()
            .filter(|(_, sgr)| !sgr.is_empty())
            .map(|(_, sgr)| sgr.as_str())
            .collect();
        for sgr in ["1;31", "3;44", "38;5;200", "4;38;2;255;128;1", "32"] {
            assert!(styled.contains(&sgr), "{sgr} missing from {styled:?}");
        }
        assert_eq!(normalise(&back), expected);
    }

    #[test]
    fn ratatui_paragraph_runs_headless() {
        let component = RatatuiComponent::new(|area, buf| {
            Paragraph::new("hello ratatui")
                .block(Block::bordered().title("demo"))
                .render(area, buf);
        })
        .on_event(|_, event| match event {
            Event::Key(key) if key.code == KeyCode::Enter => Flow::Done("done"),
            _ => Flow::Ignored,
        });
        let (outcome, record) = headless::run(component, Script::new().keys("x enter"), 30, 4);
        assert!(matches!(outcome.unwrap(), Outcome::Done("done")));
        let frame = record.last_frame();
        assert!(frame.contains("hello ratatui"), "{frame}");
        assert!(frame.contains("┌demo"), "{frame}");
        assert!(frame.contains('┘'), "{frame}");
    }

    #[test]
    fn stateful_component_updates() {
        let component = RatatuiComponent::with_state(0u32, |n, area, buf| {
            Paragraph::new(format!("count {n}")).render(area, buf);
        })
        .on_event(|n, event| match event {
            Event::Key(key) if key.code == KeyCode::Enter => Flow::Done(*n),
            Event::Key(_) => {
                *n += 1;
                Flow::Continue
            }
            _ => Flow::Ignored,
        })
        .height(1);
        let (outcome, record) = headless::run(component, Script::new().keys("a b c enter"), 20, 5);
        assert!(matches!(outcome.unwrap(), Outcome::Done(3)));
        assert!(record.last_frame().contains("count 3"));
    }

    #[test]
    fn buffer_groups_cells_by_style() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 8, 1));
        buffer.set_string(0, 0, "ab", RStyle::default().fg(RColor::Red));
        buffer.set_string(2, 0, "cd", RStyle::default().fg(RColor::Red));
        let lines = buffer_to_lines(&buffer);
        assert_eq!(lines[0].len(), 2);
        assert_eq!(lines[0][0].text, "abcd");
        assert_eq!(lines[0][1].text, "    ");
        assert_eq!(lines[0][1].style, None);
    }

    /// `cargo test --release -- --ignored --nocapture bench`
    #[test]
    #[ignore]
    fn bench_interop() {
        bench();
    }
}

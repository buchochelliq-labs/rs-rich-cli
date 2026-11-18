//! A colour picker: [`color_picker`], a saturation and value grid and a
//! hue strip drawn in half blocks, a `#rrggbb` field that takes typing,
//! and a swatch of the colour chosen.

use rich::color::{ColorSystem, ColorTriplet, ColorType};
use rich::{Color, Segment, Style};
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::a11y::Role;
use crate::layout::Size;
use crate::node::{Axis, Node};
use crate::reactive::Signal;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent, widget};

/// The grid's rows when nothing sizes the picker: two values a row.
const GRID_ROWS: u16 = 8;

/// The hue strip's columns, and the column between it and the grid.
const STRIP: u16 = 2;

/// The part of a [`color_picker`] the keys go to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Part {
    Grid,
    Hue,
    Hex,
}

/// A [`color_picker`]'s widget.
struct ColorPicker {
    value: Signal<Color>,
    /// The colour as hue (degrees), saturation and value: kept here, so a
    /// grey keeps the hue it was picked on.
    hsv: (f64, f64, f64),
    /// The value last read or set, to tell the app's changes from ours.
    seen: Color,
    part: Part,
    /// The hex field's digits while they are typed (after the `#`), and
    /// whether the next digit starts them again.
    typed: Option<(String, bool)>,
    /// The part of the mouse's press, while it drags.
    dragging: Option<Part>,
    /// Where the last draw put things: the grid's width and rows, the
    /// strip's first column, and the hex field's first column and row.
    grid: (u16, u16),
    strip: u16,
    field: (u16, u16),
}

/// A colour picker over `value`: a grid of saturation (across) and value
/// (down) at the colour's hue, a strip of hues at its right, both drawn
/// in half blocks (two shades a cell), and under them a swatch of the
/// colour, its `#rrggbb` and its name when it has one. The grid takes
/// the width it is given less the strip, and eight rows unless it is
/// sized taller; `value` is set as the colour changes, and the picker
/// follows when the app sets it.
///
/// With the focus, ← → ↑ ↓ move in the grid (PageUp and PageDown to the
/// top and bottom, Home and End to the sides), Tab goes on to the hue
/// strip, where ↑ and ↓ (or ← and →) turn the hue, and on to the hex
/// field, where hex digits are typed (the first one replaces the field;
/// six digits, or three, and Enter set it; Esc puts it back), and
/// Shift+Tab goes back; Tab from the field, or Shift+Tab from the grid,
/// moves the focus on as usual. A click or a drag in the grid or the
/// strip picks there; a click on the field types into it.
///
/// On a terminal of 256 colours or fewer the console shows each colour as
/// the nearest it has, the value kept exact, and the name says which
/// (`≈ color(67)`). In [text mode](crate::App::accessible) it is the hex
/// field alone. The grid's mark is in the theme's `color_picker.mark`
/// style (black or white, whichever shows), the field's caret in
/// `color_picker.caret` (`reverse`). Its role is a
/// [`Slider`](crate::a11y::Role::Slider), its value the hex and the name
/// (`#ff0000 red`).
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::color_picker;
/// use rich::Color;
///
/// let app = App::new(|| {
///     let colour = signal(Color::parse("#ff0000").unwrap());
///     column([color_picker(colour), text(move || colour.get().name.to_string())])
/// });
/// let mut driver = app.driver(24, 10);
/// driver.settle();
/// assert!(driver.screen().plain()[8].contains("#ff0000 red"));
/// // Down in the grid: a darker red.
/// driver.press("down");
/// assert_eq!(driver.screen().plain()[9].trim_end(), "#ee0000");
/// // To the hex field, and typed.
/// driver.press("tab tab");
/// driver.type_text("00f");
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[9].trim_end(), "#0000ff");
/// ```
pub fn color_picker(value: Signal<Color>) -> Node {
    let seen = value.get_untracked();
    let mut node = widget(ColorPicker {
        value,
        hsv: hsv_of(&seen),
        seen,
        part: Part::Grid,
        typed: None,
        dragging: None,
        grid: (0, 0),
        strip: 0,
        field: (0, 0),
    })
    .what("color_picker")
    .focusable();
    node.size = Size::Auto;
    node
}

/// A colour's red, green and blue, a system colour's as the default
/// terminal theme shows it.
fn rgb_of(color: &Color) -> ColorTriplet {
    color
        .get_truecolor()
        .unwrap_or_else(|| color.get_truecolor_for(None, true))
}

/// A colour's hue in degrees, saturation and value.
fn hsv_of(color: &Color) -> (f64, f64, f64) {
    let ColorTriplet { red, green, blue } = rgb_of(color);
    let (r, g, b) = (
        red as f64 / 255.0,
        green as f64 / 255.0,
        blue as f64 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    let saturation = if max == 0.0 { 0.0 } else { delta / max };
    (hue, saturation, max)
}

fn from_hsv((h, s, v): (f64, f64, f64)) -> Color {
    rich_ext::palette::hsv(h, s, v)
}

/// The colours with names a colour picker says: CSS's sixteen basic
/// colours, and orange.
const NAMED: [(&str, (u8, u8, u8)); 17] = [
    ("black", (0, 0, 0)),
    ("white", (255, 255, 255)),
    ("red", (255, 0, 0)),
    ("lime", (0, 255, 0)),
    ("blue", (0, 0, 255)),
    ("yellow", (255, 255, 0)),
    ("cyan", (0, 255, 255)),
    ("magenta", (255, 0, 255)),
    ("silver", (192, 192, 192)),
    ("gray", (128, 128, 128)),
    ("maroon", (128, 0, 0)),
    ("olive", (128, 128, 0)),
    ("green", (0, 128, 0)),
    ("purple", (128, 0, 128)),
    ("teal", (0, 128, 128)),
    ("navy", (0, 0, 128)),
    ("orange", (255, 165, 0)),
];

/// A colour's name: the console's own (`bright_red`, `grey50`), else one
/// of [`NAMED`] it is exactly.
fn name_of(color: &Color) -> Option<String> {
    if matches!(color.kind, ColorType::Standard | ColorType::EightBit)
        && !color.name.starts_with("color(")
    {
        return Some(color.name.to_string());
    }
    let ColorTriplet { red, green, blue } = rgb_of(color);
    NAMED
        .iter()
        .find(|(_, rgb)| *rgb == (red, green, blue))
        .map(|(name, _)| name.to_string())
}

/// Black or white, whichever shows on `color`.
fn contrast(color: &Color) -> Color {
    let ColorTriplet { red, green, blue } = rgb_of(color);
    let light = 0.299 * red as f64 + 0.587 * green as f64 + 0.114 * blue as f64;
    if light > 140.0 {
        Color::from_rgb(0, 0, 0)
    } else {
        Color::from_rgb(255, 255, 255)
    }
}

/// `color` written as a hex field shows it: `#rrggbb`.
fn hex_of(color: &Color) -> String {
    rgb_of(color).hex()
}

/// Hex digits typed into the field as a colour: six, or three doubled.
fn parse_typed(digits: &str) -> Option<Color> {
    let full = match digits.len() {
        3 => digits.chars().flat_map(|c| [c, c]).collect(),
        6 => digits.to_string(),
        _ => return None,
    };
    Color::parse(&format!("#{full}")).ok()
}

impl ColorPicker {
    /// The value changed by the app: the picker follows it.
    fn follow(&mut self) {
        let now = self.value.get();
        if now != self.seen {
            let (h, s, v) = hsv_of(&now);
            // A grey or black keeps the hue it had.
            let h = if s == 0.0 || v == 0.0 { self.hsv.0 } else { h };
            self.hsv = (h, s, v);
            self.seen = now;
            self.typed = None;
        }
    }

    /// Set the colour from `hsv`.
    fn set(&mut self, hsv: (f64, f64, f64)) {
        self.hsv = (
            hsv.0.rem_euclid(360.0),
            hsv.1.clamp(0.0, 1.0),
            hsv.2.clamp(0.0, 1.0),
        );
        let color = from_hsv(self.hsv);
        self.seen = color.clone();
        self.value.set(color);
    }

    /// Set the colour typed in the field, if it is one.
    fn set_typed(&mut self) -> bool {
        let Some(color) = self.typed.as_ref().and_then(|(d, _)| parse_typed(d)) else {
            return false;
        };
        let (h, s, v) = hsv_of(&color);
        let h = if s == 0.0 || v == 0.0 { self.hsv.0 } else { h };
        self.hsv = (h, s, v);
        self.seen = color.clone();
        self.value.set(color);
        self.typed = None;
        true
    }

    /// A saturation step and a value step in the grid.
    fn steps(&self) -> (f64, f64) {
        let (w, rows) = self.grid;
        (
            1.0 / (w.max(2) - 1) as f64,
            1.0 / (rows.max(1) * 2 - 1).max(1) as f64,
        )
    }

    /// The colour at the grid's column `x` and half row `half`.
    fn grid_hsv(&self, x: u16, half: u16) -> (f64, f64, f64) {
        let (saturation, value) = self.steps();
        (
            self.hsv.0,
            (x as f64 * saturation).min(1.0),
            (1.0 - half as f64 * value).max(0.0),
        )
    }

    /// The hue at the strip's half row `half`.
    fn strip_hue(&self, half: u16) -> f64 {
        let halves = (self.grid.1.max(1) * 2) as f64;
        half as f64 * 360.0 / halves
    }

    fn pick_at(&mut self, part: Part, column: u16, row: u16) {
        let (w, rows) = self.grid;
        let row = row.min(rows.saturating_sub(1));
        match part {
            Part::Grid => {
                let hsv = self.grid_hsv(column.min(w.saturating_sub(1)), row * 2);
                self.set(hsv);
            }
            Part::Hue => {
                let hue = self.strip_hue(row * 2);
                self.set((hue, self.hsv.1, self.hsv.2));
            }
            Part::Hex => {}
        }
    }

    /// What a key does in the part with the keys.
    fn key(&mut self, key: &rich_interact::Key) -> bool {
        let (ds, dv) = self.steps();
        let (h, s, v) = self.hsv;
        let hue_step = 360.0 / (self.grid.1.max(1) * 2) as f64;
        match (self.part, key.code) {
            (Part::Grid, KeyCode::Left) => self.set((h, s - ds, v)),
            (Part::Grid, KeyCode::Right) => self.set((h, s + ds, v)),
            (Part::Grid, KeyCode::Up) => self.set((h, s, v + dv)),
            (Part::Grid, KeyCode::Down) => self.set((h, s, v - dv)),
            (Part::Grid, KeyCode::Home) => self.set((h, 0.0, v)),
            (Part::Grid, KeyCode::End) => self.set((h, 1.0, v)),
            (Part::Grid, KeyCode::PageUp) => self.set((h, s, 1.0)),
            (Part::Grid, KeyCode::PageDown) => self.set((h, s, 0.0)),
            (Part::Hue, KeyCode::Up | KeyCode::Left) => self.set((h - hue_step, s, v)),
            (Part::Hue, KeyCode::Down | KeyCode::Right) => self.set((h + hue_step, s, v)),
            (Part::Hex, KeyCode::Char(c)) if c.is_ascii_hexdigit() => {
                let (digits, fresh) = self
                    .typed
                    .get_or_insert_with(|| (hex_of(&self.seen)[1..].to_string(), true));
                if *fresh {
                    digits.clear();
                    *fresh = false;
                }
                if digits.len() < 6 {
                    digits.push(c.to_ascii_lowercase());
                }
                if digits.len() == 6 {
                    self.set_typed();
                }
            }
            (Part::Hex, KeyCode::Backspace) => {
                let (digits, fresh) = self
                    .typed
                    .get_or_insert_with(|| (hex_of(&self.seen)[1..].to_string(), true));
                *fresh = false;
                digits.pop();
            }
            (Part::Hex, KeyCode::Enter) => return self.set_typed(),
            (Part::Hex, KeyCode::Escape) if self.typed.is_some() => self.typed = None,
            _ => return false,
        }
        true
    }

    /// The part a press at `column`, `row` is on.
    fn part_at(&self, column: u16, row: u16) -> Option<Part> {
        if crate::a11y::text_mode() {
            return Some(Part::Hex);
        }
        let (w, rows) = self.grid;
        if row < rows {
            if column < w {
                return Some(Part::Grid);
            }
            if column >= self.strip {
                return Some(Part::Hue);
            }
            return None;
        }
        (row == self.field.1 && column >= self.field.0 && column < self.field.0 + 7)
            .then_some(Part::Hex)
    }

    /// The hex field's text and where its caret is, if it shows one.
    fn field_text(&self, focused: bool) -> (String, Option<u16>) {
        match &self.typed {
            Some((digits, fresh)) => {
                let caret = (!*fresh).then_some(1 + digits.len() as u16);
                let shown = if *fresh {
                    format!("#{digits}")
                } else {
                    format!("#{digits:_<6}")
                };
                (shown, caret.filter(|_| focused))
            }
            None => (hex_of(&self.seen), None),
        }
    }

    /// The name shown after the hex: the colour's own, or, where the
    /// console has fewer colours, the one it shows (`≈ color(67)`).
    fn name_shown(&self, system: Option<ColorSystem>) -> Option<String> {
        let name = name_of(&self.seen);
        match system {
            Some(ColorSystem::Truecolor) | None => name,
            Some(system) if self.seen.kind == ColorType::Truecolor => {
                let near = self.seen.downgrade(system);
                let near = match near.number {
                    Some(number) => format!("color({number})"),
                    None => near.name.to_string(),
                };
                Some(match name {
                    Some(name) => format!("{name} ≈ {near}"),
                    None => format!("≈ {near}"),
                })
            }
            Some(_) => name,
        }
    }
}

impl Widget for ColorPicker {
    fn name(&self) -> &'static str {
        "color_picker"
    }

    fn role(&self) -> Role {
        Role::Slider
    }

    fn access_text(&self) -> Option<String> {
        Some("Colour".into())
    }

    fn access_value(&self) -> Option<String> {
        let hex = hex_of(&self.seen);
        Some(match name_of(&self.seen) {
            Some(name) => format!("{hex} {name}"),
            None => hex,
        })
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        match axis {
            Axis::Vertical if crate::a11y::text_mode() => 1,
            Axis::Vertical => GRID_ROWS + 1,
            Axis::Horizontal if width == 0 => 24,
            Axis::Horizontal => width,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        self.follow();
        let focused = cx.focused();
        let (width, height) = (canvas.width(), canvas.height());
        let caret_style = cx.style("color_picker.caret", "reverse");
        let system = cx.console().color_system();
        if crate::a11y::text_mode() {
            let (field, caret) = self.field_text(focused && self.part == Part::Hex);
            let mut line = field;
            if let Some(name) = self.name_shown(system) {
                line.push(' ');
                line.push_str(&name);
            }
            canvas.print(0, 0, &line, None);
            if let Some(x) = caret {
                canvas.restyle(x, 0, 1, 1, &caret_style);
            }
            self.field = (0, 0);
            self.grid = (0, 0);
            return;
        }
        let rows = height.saturating_sub(1).max(1);
        let grid_w = width.saturating_sub(STRIP + 1).max(1);
        self.grid = (grid_w, rows);
        self.strip = grid_w + 1;
        let shade = |top: Color, bottom: Color| {
            Segment::new("▀", Some(Style::from_color(Some(top), Some(bottom))))
        };
        let mut lines: Vec<Vec<Segment>> = Vec::with_capacity(height as usize);
        for row in 0..rows {
            let mut line = Vec::with_capacity(width as usize);
            for x in 0..grid_w {
                let top = from_hsv(self.grid_hsv(x, row * 2));
                let bottom = from_hsv(self.grid_hsv(x, row * 2 + 1));
                line.push(shade(top, bottom));
            }
            line.push(Segment::new(" ", None));
            let top = from_hsv((self.strip_hue(row * 2), 1.0, 1.0));
            let bottom = from_hsv((self.strip_hue(row * 2 + 1), 1.0, 1.0));
            for _ in 0..STRIP {
                line.push(shade(top.clone(), bottom.clone()));
            }
            lines.push(line);
        }
        // The swatch, the field and the name.
        let (field, caret) = self.field_text(focused && self.part == Part::Hex);
        let swatch = Style::from_color(None, Some(self.seen.clone()));
        let mut last = vec![
            Segment::new("    ", Some(swatch)),
            Segment::new(" ", None),
            Segment::new(field, None),
        ];
        if let Some(name) = self.name_shown(system) {
            last.push(Segment::new(format!(" {name}"), None));
        }
        lines.push(last);
        canvas.lines(&lines);
        self.field = (5, rows);
        if let Some(x) = caret {
            canvas.restyle(5 + x, rows, 1, 1, &caret_style);
        }
        // The grid's mark, on the colour, and the strip's.
        let (ds, dv) = self.steps();
        let mx = ((self.hsv.1 / ds).round() as u16).min(grid_w - 1);
        let half = (((1.0 - self.hsv.2) / dv).round() as u16).min(rows * 2 - 1);
        let mark = cx.style("color_picker.mark", "");
        let mark =
            Style::from_color(Some(contrast(&self.seen)), Some(self.seen.clone())).combine(&mark);
        let grid_mark = if focused && self.part == Part::Grid {
            '◆'
        } else {
            '◇'
        };
        canvas.set(mx, half / 2, grid_mark, Some(&mark));
        let hue_half = ((self.hsv.0 / 360.0 * (rows * 2) as f64).round() as u16) % (rows * 2);
        let pointer = if focused && self.part == Part::Hue {
            cx.style("color_picker.focus", "bold")
        } else {
            Style::default()
        };
        canvas.set(grid_w, hue_half / 2, '▸', Some(&pointer));
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(MouseButton::Left) => {
                    let Some(part) = self.part_at(mouse.column, mouse.row) else {
                        return Used::No;
                    };
                    self.part = part;
                    if part != Part::Hex {
                        self.typed = None;
                        self.dragging = Some(part);
                        cx.capture_mouse();
                        self.pick_at(part, mouse.column, mouse.row);
                    }
                    cx.redraw();
                    Used::Yes
                }
                MouseKind::Drag(MouseButton::Left) => match self.dragging {
                    Some(Part::Hue) => {
                        self.pick_at(Part::Hue, self.strip, mouse.row);
                        Used::Yes
                    }
                    Some(part) => {
                        self.pick_at(part, mouse.column, mouse.row);
                        Used::Yes
                    }
                    None => Used::No,
                },
                MouseKind::Up(MouseButton::Left) if self.dragging.is_some() => {
                    self.dragging = None;
                    Used::Yes
                }
                _ => Used::No,
            },
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => {
                let text_mode = crate::a11y::text_mode();
                match key.code {
                    KeyCode::Tab if !text_mode && self.part != Part::Hex => {
                        self.part = if self.part == Part::Grid {
                            Part::Hue
                        } else {
                            Part::Hex
                        };
                    }
                    KeyCode::BackTab if !text_mode && self.part != Part::Grid => {
                        self.typed = None;
                        self.part = if self.part == Part::Hex {
                            Part::Hue
                        } else {
                            Part::Grid
                        };
                    }
                    _ => {
                        if text_mode {
                            self.part = Part::Hex;
                        }
                        if !self.key(key) {
                            return Used::No;
                        }
                    }
                }
                cx.redraw();
                Used::Yes
            }
            WidgetEvent::Paste(text) if self.part == Part::Hex => {
                let digits: String = text
                    .trim()
                    .trim_start_matches('#')
                    .chars()
                    .filter(char::is_ascii_hexdigit)
                    .take(6)
                    .collect::<String>()
                    .to_ascii_lowercase();
                self.typed = Some((digits, false));
                self.set_typed();
                cx.redraw();
                Used::Yes
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_goes_there_and_back() {
        for hex in ["#ff0000", "#3a7bd5", "#808080", "#000000", "#00ff7f"] {
            let color = Color::parse(hex).unwrap();
            assert_eq!(hex_of(&from_hsv(hsv_of(&color))), hex);
        }
    }

    #[test]
    fn names_are_the_consoles_or_css_basic_ones() {
        assert_eq!(
            name_of(&Color::parse("bright_red").unwrap()).as_deref(),
            Some("bright_red")
        );
        assert_eq!(
            name_of(&Color::parse("#ffa500").unwrap()).as_deref(),
            Some("orange")
        );
        assert_eq!(name_of(&Color::parse("#3a7bd5").unwrap()), None);
        assert_eq!(name_of(&Color::parse("color(67)").unwrap()), None);
    }

    #[test]
    fn three_digits_double() {
        assert_eq!(hex_of(&parse_typed("0af").unwrap()), "#00aaff");
        assert!(parse_typed("0a").is_none());
    }
}

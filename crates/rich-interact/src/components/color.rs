//! A colour picker (#459, `rich color`): rich's named colours, the 256
//! palette, or a colour typed as hex or RGB, with a live swatch.
//!
//! Typing filters the named colours; text that is itself a colour
//! (`#ff8800`, `rgb(255,136,0)`, `color(208)`) is offered first. Tab
//! switches to the palette, a 16 by 16 grid moved with the arrows. Enter
//! picks. The answer is a colour string rich parses, as hex (`#ff8800`),
//! rgb (`rgb(255,136,0)`) or a name (`dark_orange`, or `color(N)` or hex
//! for a colour without one): see [`ColorFormat`].

use std::cell::Cell;

use rich::color::ColorType;
use rich::{Color, ColorTriplet, Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, highlight, plain, question, text, Theme};
use crate::event::{Button, Event, KeyCode, MouseKind};
use crate::fuzzy::rank;
use crate::keymap::{keys, Keymap};
use crate::names::COLORS;
use crate::policy::{LineIo, NotInteractive};

/// How a picked colour is written.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColorFormat {
    /// `#rrggbb`.
    #[default]
    Hex,
    /// A rich colour name when it has one (`dark_orange`), else
    /// `color(N)` for a palette colour, else hex.
    Name,
    /// `rgb(r,g,b)`.
    Rgb,
}

impl ColorFormat {
    pub fn parse(name: &str) -> Option<ColorFormat> {
        Some(match name {
            "hex" => ColorFormat::Hex,
            "name" => ColorFormat::Name,
            "rgb" => ColorFormat::Rgb,
            _ => return None,
        })
    }
}

/// A colour and the name it was picked by, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Picked {
    color: Color,
    name: Option<String>,
}

impl Picked {
    fn named(name: &str) -> Option<Picked> {
        Some(Picked {
            color: Color::parse(name).ok()?,
            name: Some(name.to_string()),
        })
    }

    /// `text` as a colour: a name, hex, `rgb(…)` or `color(N)`.
    fn parse(text: &str) -> Option<Picked> {
        let text = text.trim();
        let lower = text.to_lowercase();
        if COLORS.contains(&lower.as_str()) {
            return Picked::named(&lower);
        }
        let color = Color::parse(&lower).ok()?;
        if color.kind == ColorType::Default {
            return None;
        }
        Some(Picked { color, name: None })
    }

    fn palette(number: u8) -> Picked {
        Picked {
            color: Color::from_ansi(number),
            name: None,
        }
    }

    fn triplet(&self) -> ColorTriplet {
        self.color
            .get_truecolor()
            .unwrap_or(ColorTriplet::new(0, 0, 0))
    }

    /// The first name rich has for this palette colour.
    fn palette_name(&self) -> Option<&'static str> {
        let number = match self.color.kind {
            ColorType::Standard | ColorType::EightBit => self.color.number?,
            _ => return None,
        };
        COLORS
            .iter()
            .copied()
            .find(|name| Color::parse(name).is_ok_and(|color| color.number == Some(number)))
    }

    fn format(&self, format: ColorFormat) -> String {
        let triplet = self.triplet();
        match format {
            ColorFormat::Hex => triplet.hex(),
            ColorFormat::Rgb => format!("rgb({},{},{})", triplet.red, triplet.green, triplet.blue),
            ColorFormat::Name => match (&self.name, self.palette_name(), self.color.number) {
                (Some(name), _, _) => name.clone(),
                (None, Some(name), _) => name.to_string(),
                (None, None, Some(number)) if self.color.kind == ColorType::EightBit => {
                    format!("color({number})")
                }
                _ => triplet.hex(),
            },
        }
    }

    /// A background of this colour.
    fn swatch(&self) -> Style {
        Style::parse(&format!("on {}", self.triplet().hex())).expect("a hex colour")
    }
}

/// Pick a colour. Returns it as a string in the [`ColorFormat`] chosen.
pub struct ColorPicker {
    prompt: String,
    query: String,
    /// Indices into [`COLORS`] that match, with the characters matched.
    matches: Vec<(usize, Vec<usize>)>,
    /// The query as a colour, when it is one and not a listed name.
    custom: Option<Picked>,
    focus: usize,
    offset: usize,
    height: usize,
    grid: bool,
    cell: u8,
    format: ColorFormat,
    default: Option<String>,
    theme: Theme,
    mouse: bool,
    /// The row (a grid cell, or a list position) the last click focused,
    /// until a key: a click on it again picks it.
    clicked: Option<(bool, usize)>,
    /// Rows on screen, from the last context: the list is cut to fit.
    space: Cell<usize>,
    answer: Option<Option<String>>,
}

/// Rows above the list or grid: the question and the swatch.
const TOP: usize = 2;

impl ColorPicker {
    pub fn new(prompt: impl Into<String>) -> ColorPicker {
        let mut picker = ColorPicker {
            prompt: prompt.into(),
            query: String::new(),
            matches: Vec::new(),
            custom: None,
            focus: 0,
            offset: 0,
            height: 10,
            grid: false,
            cell: 0,
            format: ColorFormat::Hex,
            default: None,
            theme: Theme::default(),
            mouse: false,
            clicked: None,
            space: Cell::new(usize::MAX),
            answer: None,
        };
        picker.refilter();
        picker
    }

    pub fn format(mut self, format: ColorFormat) -> Self {
        self.format = format;
        self
    }

    /// Start with this typed: a name to filter by, or a colour.
    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.query = value.into();
        self.refilter();
        self
    }

    /// The colour returned without a terminal, and for an empty line at a
    /// line prompt. One that is not a colour ([`ColorPicker::is_color`])
    /// is reported when the line prompt starts, and is no default.
    pub fn default(mut self, color: impl Into<String>) -> Self {
        self.default = Some(color.into());
        self
    }

    /// Whether `text` is a colour the picker takes: a rich name, `#rrggbb`,
    /// `rgb(r,g,b)` or `color(N)`. Check a default with it up front.
    pub fn is_color(text: &str) -> bool {
        Picked::parse(text).is_some()
    }

    /// The default, when it is a colour, in the picker's format.
    fn valid_default(&self) -> Result<Option<String>, NotInteractive> {
        let Some(default) = self.default.as_deref() else {
            return Ok(None);
        };
        Picked::parse(default)
            .map(|picked| Some(picked.format(self.format)))
            .ok_or_else(|| {
                NotInteractive::Invalid(format!("the default is not a colour: {default:?}"))
            })
    }

    /// Show at most `rows` colours at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.height = rows.max(1);
        self
    }

    /// Start on the palette grid.
    pub fn palette(mut self, on: bool) -> Self {
        self.grid = on;
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Report the mouse: a click focuses a colour, a second click picks it.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    fn refilter(&mut self) {
        self.matches = rank(&self.query, COLORS.iter().copied())
            .into_iter()
            .map(|(index, found)| (index, found.positions))
            .collect();
        let exact = COLORS.contains(&self.query.trim().to_lowercase().as_str());
        self.custom = Picked::parse(&self.query).filter(|_| !exact);
        self.focus = 0;
        self.offset = 0;
    }

    fn count(&self) -> usize {
        self.matches.len() + usize::from(self.custom.is_some())
    }

    /// Rows the list takes: the height asked for, cut to what fits on
    /// screen beside the question, the swatch and the hint.
    fn rows(&self) -> usize {
        self.height
            .min(self.space.get().saturating_sub(TOP + 1))
            .max(1)
    }

    /// The colour on list row `position`.
    fn at(&self, position: usize) -> Option<Picked> {
        match (&self.custom, position) {
            (Some(custom), 0) => Some(custom.clone()),
            (Some(_), position) => Picked::named(COLORS[self.matches.get(position - 1)?.0]),
            (None, position) => Picked::named(COLORS[self.matches.get(position)?.0]),
        }
    }

    fn focused(&self) -> Option<Picked> {
        if self.grid {
            Some(Picked::palette(self.cell))
        } else {
            self.at(self.focus)
        }
    }

    fn step(&mut self, delta: isize) {
        let count = self.count();
        if count == 0 {
            return;
        }
        self.focus = self.focus.saturating_add_signed(delta).min(count - 1);
        if self.focus < self.offset {
            self.offset = self.focus;
        } else if self.focus >= self.offset + self.rows() {
            self.offset = self.focus + 1 - self.rows();
        }
    }

    fn pick(&mut self) -> Flow<String> {
        match self.focused() {
            Some(picked) => {
                let answer = picked.format(self.format);
                self.answer = Some(Some(answer.clone()));
                Flow::Done(answer)
            }
            None => Flow::Continue,
        }
    }

    fn move_cell(&mut self, columns: i16, rows: i16) {
        let column = (i16::from(self.cell % 16) + columns).clamp(0, 15);
        let row = (i16::from(self.cell / 16) + rows).clamp(0, 15);
        self.cell = (row * 16 + column) as u8;
    }

    fn click(&mut self, column: usize, row: usize) -> Flow<String> {
        let Some(row) = row.checked_sub(TOP) else {
            return Flow::Continue;
        };
        if self.grid {
            let column = column.saturating_sub(2) / 2;
            if row < 16 && column < 16 {
                let cell = (row * 16 + column) as u8;
                if self.clicked == Some((true, cell.into())) && cell == self.cell {
                    return self.pick();
                }
                self.cell = cell;
                self.clicked = Some((true, cell.into()));
            }
        } else if row < self.rows() {
            let position = self.offset + row;
            if position < self.count() {
                if self.clicked == Some((false, position)) && position == self.focus {
                    return self.pick();
                }
                self.focus = position;
                self.clicked = Some((false, position));
            }
        }
        Flow::Continue
    }

    fn swatch_line(&self, width: usize) -> Vec<Segment> {
        let theme = &self.theme;
        let mut line = vec![plain("  ")];
        match self.focused() {
            Some(picked) => {
                line.push(Segment::new("        ", Some(picked.swatch())));
                let triplet = picked.triplet();
                let mut about = format!(
                    "  {}  rgb({},{},{})",
                    triplet.hex(),
                    triplet.red,
                    triplet.green,
                    triplet.blue
                );
                if let Some(name) = picked.name.as_deref().or(picked.palette_name()) {
                    about.push_str(&format!("  {name}"));
                }
                if let Some(number) = picked.color.number {
                    about.push_str(&format!("  color({number})"));
                }
                line.push(text(about, &theme.hint));
            }
            None => line.push(text("no colour", &theme.hint)),
        }
        fit(line, width)
    }

    fn list(&self, width: usize) -> Vec<Vec<Segment>> {
        let theme = &self.theme;
        let mut lines = Vec::new();
        for position in (self.offset..self.count()).take(self.rows()) {
            let Some(picked) = self.at(position) else {
                break;
            };
            let focused = position == self.focus;
            let mut line = if focused {
                vec![text(format!("{} ", theme.pointer), &theme.pointer_style)]
            } else {
                vec![plain("  ")]
            };
            line.push(Segment::new("    ", Some(picked.swatch())));
            line.push(plain(" "));
            let base = focused.then_some(&theme.focused);
            match (&picked.name, self.custom.is_some() && position == 0) {
                (_, true) => line.push(text(self.query.trim().to_string(), &theme.answer)),
                (Some(name), false) => {
                    let positions = &self.matches[position - usize::from(self.custom.is_some())].1;
                    line.extend(highlight(name, positions, base, &theme.matched));
                }
                (None, false) => {}
            }
            line.push(text(format!("  {}", picked.triplet().hex()), &theme.hint));
            lines.push(fit(line, width));
        }
        if self.count() == 0 {
            lines.push(vec![text("  no matches", &theme.hint)]);
        }
        lines.resize_with(self.rows(), Vec::new);
        lines
    }

    fn palette_grid(&self, width: usize) -> Vec<Vec<Segment>> {
        let mut lines = Vec::with_capacity(16);
        for row in 0..16u8 {
            let mut line = vec![plain("  ")];
            for column in 0..16u8 {
                let number = row * 16 + column;
                let picked = Picked::palette(number);
                let mark = if number == self.cell {
                    let triplet = picked.triplet();
                    let light = u32::from(triplet.red) * 299
                        + u32::from(triplet.green) * 587
                        + u32::from(triplet.blue) * 114
                        > 128_000;
                    let ink = if light { "black" } else { "white" };
                    let style = Style::parse(&format!("bold {ink}"))
                        .expect("style")
                        .combine(&picked.swatch());
                    Segment::new("<>", Some(style))
                } else {
                    Segment::new("  ", Some(picked.swatch()))
                };
                line.push(mark);
            }
            lines.push(fit(line, width));
        }
        lines
    }
}

impl Component for ColorPicker {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        self.space.set(context.height);
        if let Event::Mouse(mouse) = event {
            match mouse.kind {
                MouseKind::Down(Button::Left) => {
                    return self.click(mouse.column as usize, mouse.row as usize)
                }
                MouseKind::ScrollUp if self.grid => self.move_cell(0, -1),
                MouseKind::ScrollDown if self.grid => self.move_cell(0, 1),
                MouseKind::ScrollUp => self.step(-1),
                MouseKind::ScrollDown => self.step(1),
                _ => {}
            }
            return Flow::Continue;
        }
        if let Event::Paste(pasted) = event {
            self.clicked = None;
            self.grid = false;
            self.query.push_str(&crate::components::pasted(pasted, " "));
            self.refilter();
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            return Flow::Continue;
        };
        let ctrl = key.modifiers.ctrl;
        // A click after a key is a first click again.
        self.clicked = None;
        match key.code {
            KeyCode::Enter => return self.pick(),
            KeyCode::Escape => {
                self.answer = Some(None);
                return Flow::Cancel;
            }
            KeyCode::Tab | KeyCode::BackTab => self.grid = !self.grid,
            KeyCode::Up if self.grid => self.move_cell(0, -1),
            KeyCode::Down if self.grid => self.move_cell(0, 1),
            KeyCode::Left if self.grid => self.move_cell(-1, 0),
            KeyCode::Right if self.grid => self.move_cell(1, 0),
            KeyCode::Home if self.grid => self.cell = 0,
            KeyCode::End if self.grid => self.cell = 255,
            KeyCode::Up => self.step(-1),
            KeyCode::Down => self.step(1),
            KeyCode::PageUp => self.step(-(self.rows() as isize)),
            KeyCode::PageDown => self.step(self.rows() as isize),
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                self.refilter();
            }
            KeyCode::Backspace => {
                self.grid = false;
                if self.query.pop().is_some() {
                    self.refilter();
                }
            }
            KeyCode::Char(c) if !ctrl && !key.modifiers.alt => {
                self.grid = false;
                self.query.push(c);
                self.refilter();
            }
            _ => return Flow::Ignored,
        }
        Flow::Continue
    }

    fn keymap(&self) -> Keymap {
        Keymap::new("color")
            .bind("pick", keys("enter"), "pick")
            .bind("cancel", keys("escape"), "cancel")
            .bind(
                "grid",
                keys("tab shift+tab"),
                "switch between the list and the grid",
            )
            .bind("up", keys("up"), "move up")
            .bind("down", keys("down"), "move down")
            .bind("left", keys("left"), "move left (grid)")
            .bind("right", keys("right"), "move right (grid)")
            .bind("page-up", keys("pageup"), "page up")
            .bind("page-down", keys("pagedown"), "page down")
            .bind("first", keys("home"), "first cell (grid)")
            .bind("last", keys("end"), "last cell (grid)")
            .bind("clear", keys("ctrl+u"), "clear the filter")
            .bind("delete", keys("backspace"), "delete a character")
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.space.set(context.height);
        let width = context.width;
        let theme = &self.theme;
        let mut header = question(theme, &self.prompt);
        if let Some(answer) = &self.answer {
            match (answer, self.focused()) {
                (Some(answer), Some(picked)) => {
                    header.push(Segment::new("  ", Some(picked.swatch())));
                    header.push(plain(" "));
                    header.push(text(answer.clone(), &theme.answer));
                }
                (Some(answer), None) => header.push(text(answer.clone(), &theme.answer)),
                (None, _) => header.push(text("cancelled", &theme.hint)),
            }
            return View::new(vec![fit(header, width)]);
        }
        let column = crate::components::width(&header) + rich::cells::cell_len(&self.query);
        header.push(plain(self.query.clone()));
        let mut lines = vec![fit(header, width), self.swatch_line(width)];
        if self.grid {
            lines.extend(self.palette_grid(width));
        } else {
            lines.extend(self.list(width));
        }
        let hint = if self.grid {
            "  ←↑↓→ move · tab names · enter pick · esc cancel".to_string()
        } else {
            format!(
                "  {}/{} · type a name, #hex or rgb(r,g,b) · ↑↓ move · tab palette · enter pick · \
                 esc cancel",
                self.count(),
                COLORS.len()
            )
        };
        lines.push(fit(vec![text(hint, &theme.hint)], width));
        View::new(lines).with_cursor(0, column.min(width.saturating_sub(1)))
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn default_value(&self) -> Option<String> {
        self.valid_default().ok().flatten()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        // A bad default fails before anything is asked.
        let default = self.valid_default()?;
        let mut prompt = format!("{} (a name, #rrggbb, rgb(r,g,b) or color(N))", self.prompt);
        if let Some(default) = &self.default {
            prompt.push_str(&format!(" [{default}]"));
        }
        io.write(&format!("{prompt}: "));
        let line = io.read_line().unwrap_or_default();
        let line = line.trim();
        // End of input or an empty line: the default, or no answer.
        if line.is_empty() {
            return default.map(Some).ok_or(NotInteractive::Ended);
        }
        Picked::parse(line)
            .map(|picked| Some(picked.format(self.format)))
            .ok_or_else(|| NotInteractive::Invalid(format!("not a colour: {line:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_colours() {
        let red = Picked::parse("red").unwrap();
        assert_eq!(red.format(ColorFormat::Name), "red");
        assert_eq!(red.format(ColorFormat::Hex), "#800000");
        let orange = Picked::parse("#FF8800").unwrap();
        assert_eq!(orange.format(ColorFormat::Hex), "#ff8800");
        assert_eq!(orange.format(ColorFormat::Rgb), "rgb(255,136,0)");
        assert_eq!(orange.format(ColorFormat::Name), "#ff8800");
        let palette = Picked::palette(208);
        assert_eq!(palette.format(ColorFormat::Name), "dark_orange");
        assert_eq!(
            Picked::parse("rgb(1, 2, 3)")
                .unwrap()
                .format(ColorFormat::Hex),
            "#010203"
        );
        assert!(Picked::parse("not a colour").is_none());
        assert!(Picked::parse("default").is_none());
    }

    #[test]
    fn every_listed_name_is_a_colour() {
        for name in COLORS {
            assert!(Color::parse(name).is_ok(), "{name}");
        }
    }
}

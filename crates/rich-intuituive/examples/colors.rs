//! A port of ratatui's `color-explorer`, `colors-rgb` and `modifiers`
//! examples (<https://github.com/ratatui/ratatui/tree/main/examples/apps>),
//! rebuilt on intuiTUIve as one app with a tab each. It reuses none of
//! ratatui's code: it rebuilds the behaviour and the look with intuiTUIve's
//! API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The
//! palettes are rs-rich-ext's: Tailwind CSS's (MIT) and Material Design's
//! (Apache-2.0); the swatch is Björn Ottosson's Okhsv.
//!
//!     cargo run -p rs-rich-intuituive --example colors
//!     cargo run -p rs-rich-intuituive --example colors -- --frozen
//!
//! ←/→ (or h/l, Tab) change tabs · 1 to 5 pick one · ? lists the keys ·
//! Ctrl+P finds a command · q quits. A click picks a tab.
//!
//! - **Named:** the sixteen named colours as a foreground on five
//!   backgrounds, and as a background under five foregrounds.
//! - **Indexed:** the 256 indexed colours as `color-explorer` lays them
//!   out: the sixteen named ones, the 6×6×6 cube in six squares, and the
//!   greys.
//! - **RGB:** a swatch of every hue, turning at 60 frames a second, with
//!   the frames a second it draws. `--frozen` stops it.
//! - **Modifiers:** each text modifier, on five colours.
//! - **Palettes:** Tailwind's 22 palettes, and Material's.
//!
//! What intuiTUIve adds: the swatch is one widget that draws every frame,
//! and the read-out above it draws once a second; the other tabs draw once
//! and never again (`tests/colors.rs` checks it). Each block has a label,
//! so linear mode (`--linear`) says what it shows rather than reading out
//! colour names cell by cell.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::{Segment, Style};
use intuituive::widget::{Canvas as WidgetCanvas, DrawCx};
use intuituive::widgets::tabs;
use rich_ext::palette::{material, okhsv, tailwind};

/// The tabs, in order.
pub const TABS: [&str; 5] = ["Named", "Indexed", "RGB", "Modifiers", "Palettes"];

/// The sixteen named colours.
pub const NAMED: [&str; 16] = [
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

/// The backgrounds the named colours are shown on, and the foregrounds
/// they are shown under.
pub const BACKGROUNDS: [&str; 5] = ["default", "black", "white", "blue", "bright_black"];
pub const FOREGROUNDS: [&str; 5] = ["default", "black", "white", "yellow", "bright_white"];

/// The text modifiers, as rich names them, and what the tab calls them.
pub const MODIFIERS: [(&str, &str); 9] = [
    ("bold", "Bold"),
    ("dim", "Dim"),
    ("italic", "Italic"),
    ("underline", "Underlined"),
    ("blink", "Slow blink"),
    ("blink2", "Rapid blink"),
    ("reverse", "Reversed"),
    ("conceal", "Hidden"),
    ("strike", "Crossed out"),
];

/// The colours the modifiers are shown on.
pub const MODIFIER_COLOURS: [&str; 5] = ["default", "red", "green", "yellow", "blue"];

/// How often the swatch turns: 60 frames a second.
pub const FRAME: Duration = Duration::from_micros(16_667);

/// How far the hue turns a frame, in degrees.
const TURN: f64 = 1.5;

/// How the app runs: its command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// Stop the swatch, for screenshots that do not move.
    pub frozen: bool,
    /// Run in linear mode.
    pub linear: bool,
}

impl Options {
    /// `--frozen` and `--linear`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        for arg in args {
            match arg.as_str() {
                "--frozen" => options.frozen = true,
                "--linear" => options.linear = true,
                _ => return Err(format!("unknown argument {arg}")),
            }
        }
        Ok(options)
    }
}

/// `text` padded to `width` cells, centred.
fn centred(text: &str, width: usize) -> String {
    format!("{text:^width$}")
}

/// The Named tab: each named colour on each background, then under each
/// foreground.
fn named() -> Node {
    const CELL: usize = 8;
    let heads = |colours: &[&str]| {
        colours
            .iter()
            .map(|c| centred(&short(c), CELL))
            .collect::<String>()
    };
    let mut lines = vec![
        format!(
            "[bold]{:<15}{:<42}{}[/]",
            "", "as a foreground on", "as a background under"
        ),
        format!(
            "[dim]{:<15}{}  {}[/]",
            "",
            heads(&BACKGROUNDS),
            heads(&FOREGROUNDS)
        ),
    ];
    for colour in NAMED {
        let mut line = format!("{colour:<15}");
        for back in BACKGROUNDS {
            line.push_str(&format!("[{colour} on {back}]{}[/]", centred("text", CELL)));
        }
        line.push_str("  ");
        for fore in FOREGROUNDS {
            line.push_str(&format!("[{fore} on {colour}]{}[/]", centred("text", CELL)));
        }
        lines.push(line);
    }
    label(lines.join("\n"))
        .no_wrap()
        .name("named")
        .label("The sixteen named colours, as a foreground on five backgrounds and as a background under five foregrounds")
}

/// A colour's name, short enough for a column heading.
fn short(colour: &str) -> String {
    colour.replace("bright_", "br.")
}

/// Whether text on indexed colour `i` reads better in black.
fn light(i: u16) -> bool {
    match i {
        0..=15 => matches!(i, 2 | 3 | 6 | 7 | 10 | 11 | 14 | 15),
        16..=231 => {
            let n = i - 16;
            let (r, g, b) = (n / 36, n / 6 % 6, n % 6);
            r * 299 + g * 587 + b * 114 >= 2500
        }
        _ => i >= 244,
    }
}

/// An indexed colour's cell: its number on it.
fn indexed_cell(i: u16) -> String {
    let fore = if light(i) { "black" } else { "white" };
    format!("[{fore} on color({i})]{i:>3} [/]")
}

/// The Indexed tab: the sixteen named colours, the cube in six squares (three
/// across, two down), and the greys.
fn indexed() -> Node {
    let named = (0..16).map(indexed_cell).collect::<String>();
    let mut cube = Vec::new();
    for half in 0..2u16 {
        for g in 0..6u16 {
            let mut line = String::new();
            for square in 0..3u16 {
                let r = half * 3 + square;
                for b in 0..6u16 {
                    line.push_str(&indexed_cell(16 + r * 36 + g * 6 + b));
                }
                line.push_str("  ");
            }
            cube.push(line);
        }
        if half == 0 {
            cube.push(String::new());
        }
    }
    let greys = [232u16, 244]
        .iter()
        .map(|from| (*from..from + 12).map(indexed_cell).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    column([
        label("[bold]0 to 15[/], the named colours").fixed(1),
        label(named)
            .no_wrap()
            .fixed(1)
            .label("Indexed colours 0 to 15"),
        label("").fixed(1),
        label("[bold]16 to 231[/], a cube of six reds, greens and blues").fixed(1),
        label(cube.join("\n"))
            .no_wrap()
            .fixed(13)
            .label("Indexed colours 16 to 231, the colour cube"),
        label("").fixed(1),
        label("[bold]232 to 255[/], the greys").fixed(1),
        label(greys)
            .no_wrap()
            .fixed(2)
            .label("Indexed colours 232 to 255, the greys"),
    ])
    .name("indexed")
}

/// The swatch's cell at `x`, `y` of `width` x `height`, turned by
/// `phase` frames: its top half's colour and its bottom half's. The hue
/// goes round across, the value falls down.
pub fn swatch_cell(x: u16, y: u16, width: u16, height: u16, phase: u64) -> (Style, &'static str) {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)) * 2.0);
    let hue = f64::from(x) * 360.0 / w + phase as f64 * TURN;
    let value = |row: f64| 1.0 - row / h;
    let top = okhsv(hue, 1.0, value(f64::from(y) * 2.0));
    let bottom = okhsv(hue, 1.0, value(f64::from(y) * 2.0 + 1.0));
    (Style::from_color(Some(top), Some(bottom)), "▀")
}

/// The RGB tab's swatch: every cell, every frame. It counts the frames it
/// draws, for the read-out.
struct Swatch {
    phase: Signal<u64>,
    drawn: Rc<Cell<u64>>,
}

impl Widget for Swatch {
    fn name(&self) -> &'static str {
        "swatch"
    }

    fn draw(&mut self, _cx: &mut DrawCx, canvas: &mut WidgetCanvas) {
        let phase = self.phase.get();
        self.drawn.set(self.drawn.get() + 1);
        // Text mode shows no colours: a blank swatch.
        if intuituive::a11y::text_mode() {
            return;
        }
        let (width, height) = (canvas.width(), canvas.height());
        let lines: Vec<Vec<Segment>> = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| {
                        let (style, glyph) = swatch_cell(x, y, width, height, phase);
                        Segment::new(glyph, Some(style))
                    })
                    .collect()
            })
            .collect();
        canvas.lines(&lines);
    }
}

/// The RGB tab: the frames a second, over the swatch.
fn rgb(phase: Signal<u64>, fps: Signal<Option<u64>>, drawn: Rc<Cell<u64>>) -> Node {
    let readout = text(move || match fps.get() {
        Some(fps) => format!("[bold]{fps}[/] frames a second"),
        None => "[dim]stopped (--frozen)[/]".into(),
    })
    .name("fps")
    .fixed(1);
    let swatch = widget(Swatch { phase, drawn })
        .name("swatch")
        .label("Every hue, across, from light to dark, down");
    column([readout, swatch.flex(1)])
}

/// The Modifiers tab: each modifier on each colour.
fn modifiers() -> Node {
    const CELL: usize = 13;
    let mut lines = vec![format!(
        "[bold]{:<13}{}[/]",
        "",
        MODIFIER_COLOURS
            .iter()
            .map(|c| centred(c, CELL))
            .collect::<String>()
    )];
    for (modifier, name) in MODIFIERS {
        let mut line = format!("{name:<13}");
        for colour in MODIFIER_COLOURS {
            line.push_str(&format!("[{colour} {modifier}]{}[/]", centred(name, CELL)));
        }
        lines.push(line);
        lines.push(String::new());
    }
    label(lines.join("\n"))
        .no_wrap()
        .name("modifiers")
        .label("Each text modifier, on five colours")
}

/// A palette's row: its name, and a two-cell block of each shade.
fn palette_row(name: &str, width: usize, shades: &[&str]) -> String {
    let mut line = format!("{name:<width$}");
    for shade in shades {
        line.push_str(&format!("[on {shade}]  [/]"));
    }
    line
}

/// The Palettes tab: Tailwind's beside Material's.
fn palettes() -> Node {
    let mut wind = vec!["[bold]Tailwind[/] [dim]50 to 950[/]".to_string()];
    for (name, palette) in tailwind::ALL {
        wind.push(palette_row(name, 8, &palette.shades()));
    }
    let mut material =
        vec!["[bold]Material[/]    [dim]50 to 900, then A100 to A700[/]".to_string()];
    for (name, palette) in material::ACCENTED {
        let mut shades = palette.shades().to_vec();
        shades.extend(palette.accents());
        material.push(palette_row(name, 12, &shades));
    }
    for (name, palette) in material::PLAIN {
        material.push(palette_row(name, 12, &palette.shades()));
    }
    row([
        label(wind.join("\n"))
            .no_wrap()
            .name("tailwind")
            .label("Tailwind's 22 palettes, eleven shades each")
            .fixed(32),
        label(material.join("\n"))
            .no_wrap()
            .name("material")
            .label("Material Design's palettes, ten shades and four accents each")
            .flex(1),
    ])
    .gap(2)
}

/// The app, as `options` says.
pub fn colors_app(options: Options) -> App {
    App::new(move || {
        let tab = signal(0usize);
        let phase = signal(0u64);
        let fps = signal(None::<u64>);
        let drawn = Rc::new(Cell::new(0u64));
        if !options.frozen {
            fps.set(Some(0));
            every(FRAME, move |_| phase.update(|p| *p += 1));
            // The frames the swatch drew in the last second: none while
            // another tab shows.
            let drawn = drawn.clone();
            let last = Cell::new(0u64);
            every(Duration::from_secs(1), move |_| {
                let now = drawn.get();
                let count = now - last.replace(now);
                if fps.get_untracked() != Some(count) {
                    fps.set(Some(count));
                }
            });
        }
        let strip = tabs(|| TABS.iter().map(|t| t.to_string()).collect(), tab)
            .no_focus()
            .name("tabs")
            .label("Tabs")
            .panel("Colours")
            .fixed(3);
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => named().panel("Named colours"),
                1 => indexed().panel("Indexed colours"),
                2 => rgb(phase, fps, drawn.clone()).panel("RGB"),
                3 => modifiers().panel("Modifiers"),
                _ => palettes().panel("Palettes"),
            },
        );
        let mut root = column([strip, body.flex(1)])
            .bind("left h shift+tab", "the tab before", move |_| {
                tab.update(|t| *t = (*t + TABS.len() - 1) % TABS.len())
            })
            .bind("right l tab", "the next tab", move |_| {
                tab.update(|t| *t = (*t + 1) % TABS.len())
            })
            .bind("q", "quit", |cx| cx.quit());
        for (i, name) in TABS.iter().enumerate() {
            let key = (i + 1).to_string();
            root = root.bind(&key, &format!("the {name} tab"), move |_| tab.set(i));
        }
        root
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    colors_app(options).run()
}

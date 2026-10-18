//! A port of ratatui's `canvas` and `mouse-drawing` examples
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps>), rebuilt
//! on intuiTUIve as one app. It reuses none of ratatui's code: it rebuilds
//! the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs. The map
//! is rs-rich-ext's, from Natural Earth's public-domain coastlines.
//!
//!     cargo run -p rs-rich-intuituive --example canvas
//!     cargo run -p rs-rich-intuituive --example canvas -- --frozen
//!
//! Four canvases, each a `Canvas` from rs-rich-ext in a node:
//!
//! - **World:** the map, with "You are here", which h/j/k/l (or the
//!   arrows) move.
//! - **Pong:** a ball bouncing round its box at 60 frames a second.
//! - **Draw:** press and drag the mouse to draw; Space changes the pen's
//!   colour, and c clears the pad.
//! - **Boxes:** squares growing along a line, with numbers along the axes.
//!
//! Enter changes the marker every canvas draws with, round all eight
//! kinds: Braille, half blocks, quadrants, sextants, octants, blocks, bars
//! and dots. ? lists the keys, Ctrl+P finds a command, q quits.
//! `--frozen` moves the ball on a little and stops the clock, for
//! screenshots that do not move.
//!
//! What intuiTUIve adds: each canvas reads only its own signals, so a frame
//! of pong draws the pong panel and nothing else, and a stroke draws the
//! pad alone (`tests/canvas.rs` checks it). The canvases have labels, so
//! linear mode (`--linear`) names them rather than reading out Braille.
//!
//! ratatui's `volatility-surface`, a Surface tab in the plan, is left for
//! later: a wireframe needs nothing the canvas lacks, only the time.

use std::time::Duration;

use intuituive::interact::{Button, MouseKind};
use intuituive::prelude::*;
use rich_ext::chart::{Canvas, MapResolution, Marker};

/// Every marker, in the order Enter goes round them, with a name for
/// each.
pub const MARKERS: [(Marker, &str); 8] = [
    (Marker::Braille, "Braille"),
    (Marker::HalfBlock, "half blocks"),
    (Marker::Quadrant, "quadrants"),
    (Marker::Sextant, "sextants"),
    (Marker::Octant, "octants"),
    (Marker::Block, "blocks"),
    (Marker::Bar, "bars"),
    (Marker::Dot, "dots"),
];

/// The pen's colours, which Space goes round.
pub const PENS: [&str; 6] = ["yellow", "cyan", "magenta", "green", "red", "white"];

/// How often the ball moves: 60 frames a second.
pub const FRAME: Duration = Duration::from_micros(16_667);

/// How far `--frozen` moves the ball before it stops the clock.
pub const FROZEN_FRAMES: u32 = 40;

/// The pong court, in world units.
pub const COURT: (f64, f64) = (100.0, 50.0);

/// The ball's radius, in world units.
const BALL: f64 = 4.0;

/// How far a key moves "You are here", in degrees.
const STEP: f64 = 5.0;

/// How the app runs: its command line.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Options {
    /// Move the ball on [`FROZEN_FRAMES`] frames, then stop the clock.
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

/// The ball: where it is and where it is going, in world units a frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ball {
    pub x: f64,
    pub y: f64,
    pub dx: f64,
    pub dy: f64,
}

impl Default for Ball {
    fn default() -> Ball {
        Ball {
            x: 20.0,
            y: 30.0,
            dx: 1.1,
            dy: 0.7,
        }
    }
}

impl Ball {
    /// A frame on: it moves, and turns back off the court's sides.
    pub fn step(&mut self) {
        let (width, height) = COURT;
        if self.x + self.dx < BALL || self.x + self.dx > width - BALL {
            self.dx = -self.dx;
        }
        if self.y + self.dy < BALL || self.y + self.dy > height - BALL {
            self.dy = -self.dy;
        }
        self.x += self.dx;
        self.y += self.dy;
    }
}

/// A line drawn on the pad: the cells it went through, in the pad's own
/// coordinates, and its pen.
#[derive(Clone, Debug, PartialEq)]
pub struct Stroke {
    pub cells: Vec<(u16, u16)>,
    pub pen: usize,
}

/// The marker a canvas draws with now.
fn marker(index: Signal<usize>) -> Marker {
    MARKERS[index.get() % MARKERS.len()].0
}

/// The World panel's canvas: the map, and "You are here".
fn world(index: Signal<usize>, here: Signal<(f64, f64)>) -> Node {
    renderable(move || {
        let (lon, lat) = here.get();
        Canvas::new()
            .x_bounds(-180.0, 180.0)
            .y_bounds(-90.0, 90.0)
            .marker(marker(index))
            .paint(|p| {
                p.map(MapResolution::Low, "green");
            })
            .paint(move |p| {
                p.print(lon, lat, "You are here", "bold yellow");
            })
    })
    .name("world")
    .label("A map of the world, with you on it")
}

/// The Pong panel's canvas: the ball in its court.
fn pong(index: Signal<usize>, ball: Signal<Ball>) -> Node {
    renderable(move || {
        let Ball { x, y, .. } = ball.get();
        let (width, height) = COURT;
        Canvas::new()
            .x_bounds(0.0, width)
            .y_bounds(0.0, height)
            .marker(marker(index))
            .paint(move |p| {
                p.circle(x, y, BALL, "yellow");
            })
    })
    .name("pong")
    .label("A ball bouncing round a box")
}

/// The Draw panel: what the mouse drew. The canvas's world is the pad's
/// cells, so a cell the mouse went through is drawn where it is.
fn pad(index: Signal<usize>, strokes: Signal<Vec<Stroke>>, pen: Signal<usize>) -> Node {
    leaf(move |console, width, height| {
        let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
        let at = |(x, y): (u16, u16)| (f64::from(x) + 0.5, h - f64::from(y) - 0.5);
        let canvas = strokes.with(|strokes| {
            Canvas::new()
                .x_bounds(0.0, w)
                .y_bounds(0.0, h)
                .width(width as usize)
                .height(height.max(1) as usize)
                .marker(marker(index))
                .paint(|p| {
                    for stroke in strokes {
                        let style = PENS[stroke.pen % PENS.len()];
                        let points: Vec<(f64, f64)> = stroke.cells.iter().map(|c| at(*c)).collect();
                        if points.len() == 1 {
                            p.points(&points, style);
                        } else {
                            p.polyline(&points, style);
                        }
                    }
                })
        });
        let options = console.options().update_width(width.max(1) as usize);
        console.render_lines(
            &canvas,
            &options.update_height(height.max(1) as usize),
            false,
        )
    })
    .on_mouse(move |_, mouse| {
        let cell = (mouse.column, mouse.row);
        match mouse.kind {
            MouseKind::Down(Button::Left) => {
                let pen = pen.get_untracked();
                strokes.update(|s| {
                    s.push(Stroke {
                        cells: vec![cell],
                        pen,
                    })
                });
                true
            }
            MouseKind::Drag(Button::Left) => {
                strokes.update(|s| match s.last_mut() {
                    Some(stroke) if stroke.cells.last() != Some(&cell) => stroke.cells.push(cell),
                    Some(_) => {}
                    None => s.push(Stroke {
                        cells: vec![cell],
                        pen: pen.get_untracked(),
                    }),
                });
                true
            }
            _ => false,
        }
    })
    .name("pad")
    .label("A pad to draw on with the mouse")
}

/// The Boxes panel: squares growing along a line, the axes numbered.
fn boxes(index: Signal<usize>) -> Node {
    renderable(move || {
        Canvas::new()
            .x_bounds(0.0, 100.0)
            .y_bounds(0.0, 50.0)
            .marker(marker(index))
            .paint(|p| {
                let mut x = 8.0;
                for side in [2.0, 4.0, 6.0, 9.0, 12.0, 16.0, 20.0] {
                    p.rect(x, 10.0, side, side, "cyan");
                    x += side + 3.0;
                }
            })
            .paint(|p| {
                for n in (0..=80).step_by(20) {
                    p.print(f64::from(n) + 6.0, 3.0, &n.to_string(), "dim");
                }
                for n in (20..=40).step_by(10) {
                    p.print(1.0, f64::from(n), &n.to_string(), "dim");
                }
            })
    })
    .name("boxes")
    .label("Squares of growing sizes, with the axes numbered")
}

/// The app, as `options` says.
pub fn canvas_app(options: Options) -> App {
    App::new(move || {
        let index = signal(0usize);
        let here = signal((-9.14, 38.72));
        let ball = signal(Ball::default());
        let strokes = signal(Vec::<Stroke>::new());
        let pen = signal(0usize);
        if options.frozen {
            ball.update(|b| (0..FROZEN_FRAMES).for_each(|_| b.step()));
        } else {
            every(FRAME, move |_| ball.update(Ball::step));
        }
        let status = text(move || {
            let (_, name) = MARKERS[index.get() % MARKERS.len()];
            let colour = PENS[pen.get() % PENS.len()];
            format!(
                " Marker: [bold]{name}[/] [dim](Enter)[/] · pen: [{colour}]■ {colour}[/] [dim](Space)[/] · [dim]? keys · q quits[/]"
            )
        })
        .no_wrap()
        .name("status")
        .fixed(1);
        let moved = move |dx: f64, dy: f64| {
            move |_: &mut Ctx| {
                here.update(|(lon, lat)| {
                    *lon = (*lon + dx).clamp(-180.0, 180.0);
                    *lat = (*lat + dy).clamp(-90.0, 90.0);
                })
            }
        };
        column([
            row([
                world(index, here).panel("World").percent(50),
                pong(index, ball).panel("Pong").flex(1),
            ])
            .percent(50),
            row([
                pad(index, strokes, pen).panel("Draw").percent(50),
                boxes(index).panel("Boxes").flex(1),
            ])
            .flex(1),
            status,
        ])
        .bind("enter", "the next marker", move |_| {
            index.update(|i| *i = (*i + 1) % MARKERS.len())
        })
        .bind("space", "the next pen colour", move |_| {
            pen.update(|p| *p = (*p + 1) % PENS.len())
        })
        .bind("c", "clear the pad", move |_| strokes.set(Vec::new()))
        .bind("left h", "move west", moved(-STEP, 0.0))
        .bind("right l", "move east", moved(STEP, 0.0))
        .bind("up k", "move north", moved(0.0, STEP))
        .bind("down j", "move south", moved(0.0, -STEP))
        .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
    .linear(options.linear)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    canvas_app(options).run()
}

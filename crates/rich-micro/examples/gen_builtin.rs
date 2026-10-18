//! The generator of rs-rich-micro's built-in library (#581).
//!
//! Every built-in asset is drawn here, in code, for this project: simple
//! shapes (discs, rings, strokes, polygons) on a grid of 8×16 units a cell
//! (16×16 for the usual 2×1 asset), rendered at 4 pixels a unit with 4×4
//! supersampling, then run through the micro-asset
//! [pipeline](rich_micro::pipeline) to its cells (16×16 pixels at the
//! default 8×16 cell for 2×1; 16×32 for the 2×2 trophy and 24×32 for the
//! 3×2 campfire, #711). No third-party logos, and no names that are emoji
//! codes: every asset lives in a namespace (`status/`, `dev/`, `fun/`).
//!
//! It writes, under `crates/rich-micro/builtin/`, one pack per set (a
//! `pack.json` and a package folder per asset: `manifest.json`,
//! `static.png`, and `animation.gif` for animations), and `files.rs`, the
//! table the crate embeds with `include_bytes!`. The output is committed;
//! `tests/builtin.rs` regenerates it and checks the committed files match.
//!
//! ```sh
//! cargo run -p rs-rich-micro --example gen_builtin
//! ```

use std::path::{Path, PathBuf};
use std::time::Duration;

use rich_art::graphics::AnimationFrame;
use rich_art::image::{Rgba, RgbaImage};
use rich_micro::create::{package_files, PackageSpec};
use rich_micro::pipeline::Pipeline;
use rich_micro::CellSize;

/// Every built-in asset is under this licence: the project's own.
pub const LICENSE: &str = "MIT";
pub const AUTHOR: &str = "rs-rich-cli contributors";
pub const VERSION: &str = "1.0.0";

/// The drawing grid is 8 units a column and 16 a row (16 units square for
/// 2×1 cells); the canvas is `SCALE` pixels a unit.
const SCALE: u32 = 4;
/// Samples per pixel side.
const SAMPLES: u32 = 4;

type Color = [u8; 4];

const fn rgb(hex: u32) -> Color {
    [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8, 255]
}

const WHITE: Color = rgb(0xffffff);
const INK: Color = rgb(0x1f2328);

/// A shape in grid units.
#[derive(Clone, Debug)]
pub enum Shape {
    Disc {
        x: f32,
        y: f32,
        r: f32,
    },
    Ring {
        x: f32,
        y: f32,
        r: f32,
        width: f32,
    },
    Ellipse {
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
    },
    /// A stroke with round caps.
    Line {
        from: (f32, f32),
        to: (f32, f32),
        width: f32,
    },
    /// A rectangle with rounded corners.
    Rect {
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        radius: f32,
    },
    Polygon(Vec<(f32, f32)>),
}

impl Shape {
    fn contains(&self, px: f32, py: f32) -> bool {
        match self {
            Shape::Disc { x, y, r } => (px - x).hypot(py - y) <= *r,
            Shape::Ring { x, y, r, width } => ((px - x).hypot(py - y) - r).abs() <= width / 2.0,
            Shape::Ellipse { x, y, rx, ry } => {
                ((px - x) / rx).powi(2) + ((py - y) / ry).powi(2) <= 1.0
            }
            Shape::Line { from, to, width } => {
                let (dx, dy) = (to.0 - from.0, to.1 - from.1);
                let length = dx * dx + dy * dy;
                let t = if length == 0.0 {
                    0.0
                } else {
                    (((px - from.0) * dx + (py - from.1) * dy) / length).clamp(0.0, 1.0)
                };
                let (cx, cy) = (from.0 + t * dx, from.1 + t * dy);
                (px - cx).hypot(py - cy) <= width / 2.0
            }
            Shape::Rect {
                x0,
                y0,
                x1,
                y1,
                radius,
            } => {
                let cx = px.clamp(x0 + radius, x1 - radius);
                let cy = py.clamp(y0 + radius, y1 - radius);
                px >= *x0
                    && px <= *x1
                    && py >= *y0
                    && py <= *y1
                    && (px - cx).hypot(py - cy) <= *radius
            }
            Shape::Polygon(points) => {
                // Even-odd rule.
                let mut inside = false;
                let mut j = points.len() - 1;
                for i in 0..points.len() {
                    let (xi, yi) = points[i];
                    let (xj, yj) = points[j];
                    if (yi > py) != (yj > py) && px < (xj - xi) * (py - yi) / (yj - yi) + xi {
                        inside = !inside;
                    }
                    j = i;
                }
                inside
            }
        }
    }
}

fn disc(x: f32, y: f32, r: f32) -> Shape {
    Shape::Disc { x, y, r }
}

fn line(from: (f32, f32), to: (f32, f32), width: f32) -> Shape {
    Shape::Line { from, to, width }
}

fn rect(x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> Shape {
    Shape::Rect {
        x0,
        y0,
        x1,
        y1,
        radius,
    }
}

/// Shapes painted in order, each over the ones before it.
pub type Layers = Vec<(Shape, Color)>;

/// The drawing grid of `size` cells, in units: 8 a column, 16 a row.
fn grid(size: CellSize) -> (u32, u32) {
    (8 * size.cols() as u32, 16 * size.rows() as u32)
}

/// Render `layers` to a canvas of `SCALE` pixels a grid unit (64×64 for
/// 2×1 cells) with supersampled edges.
fn render(layers: &Layers, size: CellSize) -> RgbaImage {
    let (width, height) = grid(size);
    let unit = 1.0 / SCALE as f32;
    RgbaImage::from_fn(width * SCALE, height * SCALE, |x, y| {
        let mut sum = [0u32; 4];
        for sy in 0..SAMPLES {
            for sx in 0..SAMPLES {
                let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * unit;
                let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * unit;
                let color = layers
                    .iter()
                    .rev()
                    .find(|(shape, _)| shape.contains(px, py))
                    .map_or([0; 4], |(_, color)| *color);
                // Premultiplied, so edges against nothing do not darken.
                let alpha = u32::from(color[3]);
                for c in 0..3 {
                    sum[c] += u32::from(color[c]) * alpha / 255;
                }
                sum[3] += alpha;
            }
        }
        let n = SAMPLES * SAMPLES;
        let alpha = sum[3] / n;
        if alpha == 0 {
            return Rgba([0, 0, 0, 0]);
        }
        let channel = |c: usize| ((sum[c] * 255 / n) / alpha.max(1)).min(255) as u8;
        Rgba([channel(0), channel(1), channel(2), alpha as u8])
    })
}

/// A disc badge: `fill` with a darker `edge`, the symbol on top.
fn badge(fill: Color, edge: Color, symbol: Layers) -> Layers {
    let mut layers = vec![(disc(8.0, 8.0, 7.6), edge), (disc(8.0, 8.0, 6.7), fill)];
    layers.extend(symbol);
    layers
}

/// One built-in asset: its metadata and its frames (one for a still).
pub struct Drawing {
    pub name: &'static str,
    pub alt: &'static str,
    pub emoji: &'static str,
    pub text: &'static str,
    /// Its cells: 2×1 for all but the two-row examples.
    pub size: CellSize,
    /// `(layers, milliseconds)` per frame.
    pub frames: Vec<(Layers, u64)>,
}

fn still(
    name: &'static str,
    alt: &'static str,
    emoji: &'static str,
    text: &'static str,
    layers: Layers,
) -> Drawing {
    Drawing {
        name,
        alt,
        emoji,
        text,
        size: CellSize::default(),
        frames: vec![(layers, 100)],
    }
}

fn status() -> Vec<Drawing> {
    let green = rgb(0x2da44e);
    let red = rgb(0xcf222e);
    let blue = rgb(0x0969da);
    let amber = rgb(0xe3b341);
    let loading = (0..8)
        .map(|frame| {
            let mut layers = Layers::new();
            for dot in 0..8 {
                let angle = dot as f32 * std::f32::consts::TAU / 8.0 - std::f32::consts::FRAC_PI_2;
                // The head is `frame`; the dots behind it fade.
                let age = (frame + 8 - dot) % 8;
                let shade = match age {
                    0 => rgb(0x54aeff),
                    1 => rgb(0x218bff),
                    2 => rgb(0x0969da),
                    _ => rgb(0xafb8c1),
                };
                let r = if age == 0 { 1.8 } else { 1.4 };
                layers.push((
                    disc(8.0 + 5.4 * angle.cos(), 8.0 + 5.4 * angle.sin(), r),
                    shade,
                ));
            }
            (layers, 100)
        })
        .collect();
    vec![
        still(
            "status/success",
            "green circle with a white check mark",
            "✅",
            "OK",
            badge(
                green,
                rgb(0x1a7f37),
                vec![
                    (line((4.3, 8.4), (7.0, 11.0), 2.0), WHITE),
                    (line((7.0, 11.0), (11.8, 5.2), 2.0), WHITE),
                ],
            ),
        ),
        still(
            "status/warning",
            "amber triangle with an exclamation mark",
            "⚠",
            "!!",
            vec![
                (
                    Shape::Polygon(vec![(8.0, 0.6), (15.6, 14.6), (0.4, 14.6)]),
                    rgb(0x9a6700),
                ),
                (
                    Shape::Polygon(vec![(8.0, 2.4), (14.1, 13.6), (1.9, 13.6)]),
                    amber,
                ),
                (line((8.0, 5.6), (8.0, 9.6), 1.8), INK),
                (disc(8.0, 11.9, 1.0), INK),
            ],
        ),
        still(
            "status/error",
            "red circle with a white cross",
            "❌",
            "XX",
            badge(
                red,
                rgb(0xa40e26),
                vec![
                    (line((5.3, 5.3), (10.7, 10.7), 2.0), WHITE),
                    (line((10.7, 5.3), (5.3, 10.7), 2.0), WHITE),
                ],
            ),
        ),
        still(
            "status/info",
            "blue circle with a white letter i",
            "ℹ",
            "i",
            badge(
                blue,
                rgb(0x0550ae),
                vec![
                    (disc(8.0, 4.6, 1.2), WHITE),
                    (line((8.0, 7.2), (8.0, 11.4), 2.0), WHITE),
                ],
            ),
        ),
        Drawing {
            name: "status/loading",
            alt: "ring of dots turning, for work in progress",
            emoji: "⏳",
            text: "..",
            size: CellSize::default(),
            frames: loading,
        },
    ]
}

fn dev() -> Vec<Drawing> {
    let bug = {
        let body = rgb(0x1a7f37);
        let leg = rgb(0x24292f);
        let mut layers = Layers::new();
        for y in [7.0, 9.5, 12.0] {
            layers.push((line((2.2, y - 1.0), (13.8, y + 1.0), 1.0), leg));
            layers.push((line((2.2, y + 1.0), (13.8, y - 1.0), 1.0), leg));
        }
        layers.push((line((6.6, 3.2), (5.0, 0.8), 0.9), leg));
        layers.push((line((9.4, 3.2), (11.0, 0.8), 0.9), leg));
        layers.push((disc(8.0, 4.4, 2.6), leg));
        layers.push((
            Shape::Ellipse {
                x: 8.0,
                y: 9.6,
                rx: 4.4,
                ry: 5.4,
            },
            body,
        ));
        layers.push((line((8.0, 5.0), (8.0, 14.6), 0.9), rgb(0x116329)));
        layers.push((disc(6.3, 8.4, 0.9), rgb(0x4ac26b)));
        layers.push((disc(9.7, 11.2, 0.9), rgb(0x4ac26b)));
        layers
    };
    let branch = {
        let stem = rgb(0x57606a);
        vec![
            (line((4.5, 2.5), (4.5, 13.5), 1.6), stem),
            (line((4.5, 10.5), (11.5, 6.0), 1.6), stem),
            (line((11.5, 6.0), (11.5, 3.2), 1.6), stem),
            (disc(4.5, 13.0, 2.2), rgb(0x0969da)),
            (disc(4.5, 3.0, 2.2), rgb(0x0969da)),
            (disc(11.5, 3.2, 2.2), rgb(0x8250df)),
        ]
    };
    let terminal = vec![
        (rect(0.6, 1.8, 15.4, 14.2, 2.0), rgb(0x57606a)),
        (rect(1.6, 4.2, 14.4, 13.2, 1.0), rgb(0x24292f)),
        (disc(2.9, 3.0, 0.6), rgb(0xff8182)),
        (disc(4.6, 3.0, 0.6), rgb(0xeac54f)),
        (disc(6.3, 3.0, 0.6), rgb(0x4ac26b)),
        (line((3.8, 6.4), (6.4, 8.6), 1.3), rgb(0x4ac26b)),
        (line((6.4, 8.6), (3.8, 10.8), 1.3), rgb(0x4ac26b)),
        (line((7.8, 11.0), (11.6, 11.0), 1.3), WHITE),
    ];
    let package = vec![
        (
            Shape::Polygon(vec![(8.0, 1.0), (15.0, 4.2), (8.0, 7.4), (1.0, 4.2)]),
            rgb(0xd4a72c),
        ),
        (
            Shape::Polygon(vec![(1.0, 4.2), (8.0, 7.4), (8.0, 15.2), (1.0, 12.0)]),
            rgb(0xbf8700),
        ),
        (
            Shape::Polygon(vec![(15.0, 4.2), (8.0, 7.4), (8.0, 15.2), (15.0, 12.0)]),
            rgb(0x9a6700),
        ),
        (line((4.4, 2.6), (11.4, 5.8), 1.2), rgb(0xfff8c5)),
        (line((11.4, 5.8), (11.4, 9.0), 1.2), rgb(0xfff8c5)),
    ];
    vec![
        still("dev/bug", "green bug with six legs", "🐛", "#!", bug),
        still(
            "dev/branch",
            "branching line with three commit dots",
            "🔀",
            "Y",
            branch,
        ),
        still(
            "dev/terminal",
            "terminal window with a prompt",
            "💻",
            ">_",
            terminal,
        ),
        still("dev/package", "closed cardboard box", "📦", "[]", package),
    ]
}

/// A heart of `size` (1 is the full 16-unit heart), centred.
fn heart(size: f32, color: Color) -> Layers {
    let s = |v: f32| 8.0 + (v - 8.0) * size;
    let r = 3.7 * size;
    vec![
        (disc(s(4.6), s(5.6), r), color),
        (disc(s(11.4), s(5.6), r), color),
        (
            Shape::Polygon(vec![(s(1.1), s(6.8)), (s(14.9), s(6.8)), (s(8.0), s(14.6))]),
            color,
        ),
        (disc(s(4.4), s(4.6), 1.1 * size), rgb(0xffaba8)),
    ]
}

fn fun() -> Vec<Drawing> {
    let red = rgb(0xe5534b);
    let star = {
        let points: Vec<(f32, f32)> = (0..10)
            .map(|i| {
                let angle = i as f32 * std::f32::consts::PI / 5.0 - std::f32::consts::FRAC_PI_2;
                let r = if i % 2 == 0 { 7.6 } else { 3.3 };
                (8.0 + r * angle.cos(), 8.6 + r * angle.sin())
            })
            .collect();
        let inner: Vec<(f32, f32)> = points
            .iter()
            .map(|(x, y)| (8.0 + (x - 8.0) * 0.72, 8.6 + (y - 8.6) * 0.72))
            .collect();
        vec![
            (Shape::Polygon(points), rgb(0xbf8700)),
            (Shape::Polygon(inner), rgb(0xf2cc60)),
        ]
    };
    let cat = {
        let fur = rgb(0xf0883e);
        let dark = rgb(0x953800);
        vec![
            (
                Shape::Polygon(vec![(1.4, 1.2), (6.6, 4.6), (2.2, 8.0)]),
                dark,
            ),
            (
                Shape::Polygon(vec![(14.6, 1.2), (9.4, 4.6), (13.8, 8.0)]),
                dark,
            ),
            (
                Shape::Ellipse {
                    x: 8.0,
                    y: 9.2,
                    rx: 6.6,
                    ry: 5.8,
                },
                fur,
            ),
            (disc(5.4, 8.4, 1.1), INK),
            (disc(10.6, 8.4, 1.1), INK),
            (
                Shape::Polygon(vec![(7.0, 10.6), (9.0, 10.6), (8.0, 11.8)]),
                rgb(0xff8182),
            ),
            (line((1.4, 11.2), (5.0, 11.8), 0.6), dark),
            (line((14.6, 11.2), (11.0, 11.8), 0.6), dark),
        ]
    };
    let coffee = (0..3)
        .map(|frame| {
            let steam = rgb(0xafb8c1);
            let shift = frame as f32 * 0.8;
            let mut layers = vec![
                (
                    Shape::Ring {
                        x: 12.6,
                        y: 10.0,
                        r: 2.1,
                        width: 1.2,
                    },
                    rgb(0x6e7781),
                ),
                (rect(2.0, 6.4, 12.4, 15.0, 1.8), rgb(0x6e7781)),
                (rect(3.0, 7.0, 11.4, 9.0, 0.6), rgb(0x6f3b18)),
            ];
            for x in [4.8, 7.2, 9.6] {
                layers.push((
                    line((x, 5.2 - shift / 2.0), (x + 0.8, 3.0 - shift / 2.0), 0.9),
                    steam,
                ));
                layers.push((
                    line((x + 0.8, 3.0 - shift / 2.0), (x, 0.8 - shift / 2.0), 0.9),
                    steam,
                ));
            }
            (layers, 250)
        })
        .collect();
    vec![
        Drawing {
            name: "fun/heart",
            alt: "red heart, beating",
            emoji: "💖",
            text: "<3",
            size: CellSize::default(),
            frames: vec![
                (heart(1.0, red), 500),
                (heart(0.84, red), 180),
                (heart(1.0, red), 160),
                (heart(0.9, red), 160),
            ],
        },
        still("fun/star", "yellow five-pointed star", "⭐", "*", star),
        still("fun/cat", "orange cat face", "🐱", "=^", cat),
        Drawing {
            name: "fun/coffee",
            alt: "steaming cup of coffee",
            emoji: "☕",
            text: "c[",
            size: CellSize::default(),
            frames: coffee,
        },
        // Two rows high (#711): a 2×2 icon and a 3×2 animation.
        Drawing {
            name: "fun/trophy",
            alt: "gold trophy cup on a dark stand",
            emoji: "🏆",
            text: "#1",
            size: CellSize::new(2, 2).expect("2x2"),
            frames: vec![(trophy(), 100)],
        },
        Drawing {
            name: "fun/campfire",
            alt: "campfire of crossed logs with flickering flames",
            emoji: "🔥",
            text: "(^)",
            size: CellSize::new(3, 2).expect("3x2"),
            frames: (0..4).map(|frame| (campfire(frame), 150)).collect(),
        },
    ]
}

/// A gold cup with two handles on a stem and a dark stand, on the 16×32
/// grid of a 2×2 asset.
fn trophy() -> Layers {
    let gold = rgb(0xd4a72c);
    let light = rgb(0xf2cc60);
    let dark = rgb(0x9a6700);
    let stand = rgb(0x57606a);
    vec![
        (
            Shape::Ring {
                x: 3.4,
                y: 8.6,
                r: 2.6,
                width: 1.4,
            },
            dark,
        ),
        (
            Shape::Ring {
                x: 12.6,
                y: 8.6,
                r: 2.6,
                width: 1.4,
            },
            dark,
        ),
        (
            Shape::Polygon(vec![
                (2.6, 3.0),
                (13.4, 3.0),
                (12.6, 11.0),
                (10.2, 15.6),
                (5.8, 15.6),
                (3.4, 11.0),
            ]),
            dark,
        ),
        (
            Shape::Polygon(vec![
                (3.6, 3.8),
                (12.4, 3.8),
                (11.7, 10.8),
                (9.6, 14.6),
                (6.4, 14.6),
                (4.3, 10.8),
            ]),
            gold,
        ),
        (line((5.6, 5.4), (6.0, 11.0), 1.0), light),
        (rect(6.8, 15.0, 9.2, 21.0, 0.4), dark),
        (rect(4.6, 20.6, 11.4, 23.4, 0.8), gold),
        (rect(2.8, 23.2, 13.2, 29.4, 1.0), stand),
        (rect(5.6, 25.2, 10.4, 27.4, 0.4), light),
    ]
}

/// Crossed logs under three tongues of flame, frame `frame` of four: the
/// tips sway and a spark rises, on the 24×32 grid of a 3×2 asset.
fn campfire(frame: usize) -> Layers {
    let sway = [0.0, 1.2, 0.4, -1.0][frame % 4];
    let lift = [0.0, -1.6, -0.6, -2.2][frame % 4];
    let flame = |x: f32, width: f32, top: f32, lean: f32| {
        Shape::Polygon(vec![
            (x - width, 24.0),
            (x - width * 0.8, 17.0),
            (x + lean, top),
            (x + width * 0.8, 17.0),
            (x + width, 24.0),
        ])
    };
    let mut layers = vec![
        (flame(12.0, 7.4, 2.6 + lift, sway), rgb(0xcf222e)),
        (flame(12.0, 5.6, 6.4 + lift / 2.0, -sway), rgb(0xf0883e)),
        (
            flame(12.0, 3.6, 11.0 - lift / 3.0, sway / 2.0),
            rgb(0xf2cc60),
        ),
        (
            Shape::Ellipse {
                x: 12.0,
                y: 22.4,
                rx: 2.6,
                ry: 2.2,
            },
            rgb(0xfff8c5),
        ),
        (line((2.6, 29.4), (21.4, 24.6), 2.8), rgb(0x6f3b18)),
        (line((2.6, 24.6), (21.4, 29.4), 2.8), rgb(0x8a4b20)),
        (disc(2.6, 24.6, 1.3), rgb(0xd4a72c)),
        (disc(21.4, 24.6, 1.3), rgb(0xd4a72c)),
    ];
    // A spark rising beside the flames.
    let (x, y) = [(6.0, 12.0), (5.2, 8.0), (18.4, 10.0), (19.0, 5.6)][frame % 4];
    layers.push((disc(x, y, 0.9), rgb(0xffd33d)));
    layers
}

/// The sets, as packs: `(pack name, description, drawings)`.
pub fn sets() -> Vec<(&'static str, &'static str, Vec<Drawing>)> {
    vec![
        (
            "status",
            "Status badges: success, warning, error, info, loading",
            status(),
        ),
        (
            "dev",
            "Developer icons: bug, branch, terminal, package",
            dev(),
        ),
        (
            "fun",
            "Fun icons: heart, star, cat, coffee, and two rows high, trophy and campfire",
            fun(),
        ),
    ]
}

/// Render and fit one drawing, then turn it into a package's files.
pub fn package(drawing: &Drawing) -> Vec<(&'static str, Vec<u8>)> {
    let frames: Vec<AnimationFrame> = drawing
        .frames
        .iter()
        .map(|(layers, ms)| AnimationFrame {
            image: render(layers, drawing.size),
            delay: Duration::from_millis(*ms),
        })
        .collect();
    let processed = Pipeline::new(drawing.size).process_frames(&frames);
    let spec = PackageSpec::new(drawing.name, drawing.alt)
        .emoji(drawing.emoji)
        .text(drawing.text)
        .version(VERSION)
        .license(LICENSE)
        .author(AUTHOR);
    package_files(&spec, &processed).expect("a built-in asset makes a valid package")
}

/// Write every set under `root`: `<pack>/pack.json`,
/// `<pack>/<asset>/<files>`, and `files.rs`. Returns the files written,
/// relative to `root`.
pub fn generate(root: &Path) -> Vec<PathBuf> {
    let mut written = Vec::new();
    let write = |written: &mut Vec<PathBuf>, relative: String, bytes: &[u8]| {
        let path = root.join(&relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        written.push(PathBuf::from(relative));
    };
    for (pack, description, drawings) in sets() {
        let packages: Vec<String> = drawings
            .iter()
            .map(|d| d.name.rsplit('/').next().unwrap().to_string())
            .collect();
        let index = serde_json::json!({
            "schema_version": 1,
            "name": pack,
            "version": VERSION,
            "description": description,
            "license": LICENSE,
            "author": AUTHOR,
            "packages": packages,
        });
        let mut json = serde_json::to_vec_pretty(&index).unwrap();
        json.push(b'\n');
        write(&mut written, format!("{pack}/pack.json"), &json);
        for (drawing, folder) in drawings.iter().zip(&packages) {
            for (file, mut bytes) in package(drawing) {
                if file.ends_with(".json") {
                    bytes.push(b'\n');
                }
                write(&mut written, format!("{pack}/{folder}/{file}"), &bytes);
            }
        }
    }
    let mut table = String::from(
        "// Generated by `cargo run -p rs-rich-micro --example gen_builtin`. Do not edit.\n\
         // The built-in library's files, embedded: (path under builtin/, bytes).\n&[\n",
    );
    let mut paths: Vec<String> = written
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    for path in &paths {
        table.push_str(&format!(
            "    (\"{path}\", include_bytes!(concat!(env!(\"CARGO_MANIFEST_DIR\"), \"/builtin/{path}\"))),\n"
        ));
    }
    table.push_str("]\n");
    write(&mut written, "files.rs".to_string(), table.as_bytes());
    written
}

/// Where the committed output lives.
pub fn builtin_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin")
}

fn main() {
    let root = builtin_dir();
    let written = generate(&root);
    println!("wrote {} files under {}", written.len(), root.display());
}

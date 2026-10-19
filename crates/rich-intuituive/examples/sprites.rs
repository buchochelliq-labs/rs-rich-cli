//! Sprites on a stage: a few animated sprites drifting over a backdrop,
//! with a pixel buffer beside them.
//!
//!     cargo run -p rs-rich-intuituive --example sprites
//!
//! Click a sprite to select it (or Tab through them); the arrows move the
//! selected one, which stops drifting while you steer it, and Space lets
//! it go again. f flips it. The panel on the right is a pixel buffer: a
//! ball bouncing in half blocks, drawn a pixel at a time. q quits.
//!
//! What it shows: `Sprite` frames from markup (a ship's flame, a ghost's
//! eyes) and from pixel art (a slime), each animated on the app's clock;
//! a `stage_with` over a `fill` backdrop, its transparent cells showing
//! the dots beneath; clicks found by a sprite's opaque cells; z order (the
//! selected sprite on top, under its marker); and `pixels` with `blit`.

use std::time::Duration;

use intuituive::pixels::{pixels, PixelSprite};
use intuituive::prelude::*;
use intuituive::sprite::{stage_with, Placed, Sprite, StageOptions};

const TICK: Duration = Duration::from_millis(50);

/// One sprite and how it moves.
#[derive(Clone, Copy, PartialEq)]
struct Critter {
    x: f64,
    y: f64,
    dx: f64,
    dy: f64,
    flip: bool,
    steered: bool,
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// The cast: a ship with a flickering flame, a ghost that looks about,
/// and a slime that squashes.
fn cast() -> Vec<Sprite> {
    let ship = Sprite::from_frames([
        Sprite::from_markup(
            &[".[cyan]/\\[/].", "[cyan]<[b red]##[/]>", ".[yellow]''[/]."],
            '.',
        ),
        Sprite::from_markup(
            &[".[cyan]/\\[/].", "[cyan]<[b red]##[/]>", ".[red]\"\"[/]."],
            '.',
        ),
    ])
    .period(ms(120));
    let ghost = Sprite::from_frames([
        Sprite::from_markup(&["[white].--.", "|[b blue]o o[/]|", "|/\\/|"], ' '),
        Sprite::from_markup(&["[white].--.", "|[b blue] oo[/]|", "|\\/\\|"], ' '),
    ])
    .period(ms(400));
    let green = [80, 200, 90, 255];
    let dark = [20, 90, 40, 255];
    let palette = [('g', green), ('d', dark)];
    let slime = PixelSprite::from_art(&["..gggg..", ".gggggg.", "gdggggdg", "gggggggg"], &palette)
        .with_frame(
            PixelSprite::from_art(&["........", "..gggg..", "gdggggdg", "gggggggg"], &palette)
                .frame(0)
                .to_vec(),
        )
        .period(ms(300));
    vec![ship, ghost, Sprite::from_pixels(&slime)]
}

const NAMES: [&str; 3] = ["ship", "ghost", "slime"];

pub fn app() -> App {
    App::new(|| {
        let sprites = cast();
        let critters = signal(vec![
            Critter {
                x: 4.0,
                y: 3.0,
                dx: 0.6,
                dy: 0.25,
                flip: false,
                steered: false,
            },
            Critter {
                x: 30.0,
                y: 8.0,
                dx: -0.4,
                dy: 0.2,
                flip: false,
                steered: false,
            },
            Critter {
                x: 15.0,
                y: 12.0,
                dx: 0.3,
                dy: -0.15,
                flip: false,
                steered: false,
            },
        ]);
        let selected = signal(None::<usize>);
        let size = viewport();
        // The stage's size: the screen less the ball's panel and the
        // status line.
        let bounds = move || {
            let (w, h) = size.get();
            (
                f64::from(w.saturating_sub(24)),
                f64::from(h.saturating_sub(1)),
            )
        };
        every(TICK, move |_| {
            let (w, h) = bounds();
            critters.update(|all| {
                for c in all.iter_mut().filter(|c| !c.steered) {
                    c.x += c.dx;
                    c.y += c.dy;
                    if c.x < 0.0 || c.x > w - 5.0 {
                        c.dx = -c.dx;
                        c.flip = c.dx < 0.0;
                    }
                    if c.y < 0.0 || c.y > h - 3.0 {
                        c.dy = -c.dy;
                    }
                    c.x = c.x.clamp(0.0, (w - 5.0).max(0.0));
                    c.y = c.y.clamp(0.0, (h - 3.0).max(0.0));
                }
            })
        });
        let marker = Sprite::from_markup(&["[b yellow]▼"], ' ');
        let steer = move |dx: f64, dy: f64| {
            move |_: &mut Ctx| {
                let Some(i) = selected.get_untracked() else {
                    return;
                };
                critters.update(|all| {
                    let c = &mut all[i];
                    c.steered = true;
                    c.x += dx;
                    c.y += dy;
                    if dx != 0.0 {
                        c.flip = dx < 0.0;
                    }
                });
            }
        };
        let field = stage_with(
            move || {
                let chosen = selected.get();
                let mut placed: Vec<Placed> = critters
                    .get()
                    .iter()
                    .enumerate()
                    .map(|(i, c)| {
                        Placed::new(i, sprites[i].clone())
                            .at(c.x, c.y)
                            .flip(c.flip)
                            .z(if chosen == Some(i) { 1 } else { 0 })
                    })
                    .collect();
                if let Some(i) = chosen {
                    let c = critters.get()[i];
                    placed.push(
                        Placed::new(usize::MAX, marker.clone())
                            .at(c.x + 1.0, c.y - 1.0)
                            .z(2),
                    );
                }
                placed
            },
            StageOptions::new()
                .backdrop(fill("·", "grey30 on #0b1020"))
                .on_sprite_click(move |id: usize, _| {
                    if id != usize::MAX {
                        selected.set(Some(id));
                    }
                }),
        );
        let ball = PixelSprite::from_art(
            &[".ww.", "wwww", "wwww", ".ww."],
            &[('w', [250, 120, 60, 255])],
        );
        let frame = signal(0u32);
        every(Duration::from_millis(33), move |_| {
            frame.update(|f| *f += 1)
        });
        let bouncing = pixels(22, 30, move |buf| {
            let t = frame.get() as i32;
            buf.clear([16, 16, 40, 255]);
            buf.line(0, 29, 21, 29, [90, 90, 140, 255]);
            // Back and forth across, up and down a bounce.
            let x = (t % 36 - 18).abs();
            let y = 25 - ((t % 24 - 12).pow(2) * 25 / 144);
            buf.blit(&ball, x, y, 1.0, false);
        });
        column([
            row([field, bouncing.panel("pixels").fixed(24)]),
            text(move || {
                let who = selected.get().map_or("nothing", |i| NAMES[i]);
                format!("[b]{who}[/] selected · click a sprite · ←↑↓→ move · f flip · space lets go · q quits")
            })
            .fixed(1),
        ])
        .on_key("left", steer(-1.0, 0.0))
        .on_key("right", steer(1.0, 0.0))
        .on_key("up", steer(0.0, -1.0))
        .on_key("down", steer(0.0, 1.0))
        .on_key("tab", move |_| selected.update(|s| *s = Some(s.map_or(0, |i| (i + 1) % 3))))
        .on_key("f", move |_| {
            if let Some(i) = selected.get_untracked() {
                critters.update(|all| all[i].flip = !all[i].flip);
            }
        })
        .on_key("space", move |_| {
            if let Some(i) = selected.get_untracked() {
                critters.update(|all| all[i].steered = false);
            }
        })
        .on_key("q", |cx| cx.quit())
    })
}

fn main() -> std::io::Result<()> {
    app().run()
}

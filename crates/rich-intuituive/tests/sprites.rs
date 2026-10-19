//! Sprites, stages and pixel buffers: transparent cells over a backdrop,
//! z order, clipping, clicks and hovers by cell, collisions, frames and
//! tweens on the app's clock, half-cell positions, and pixel buffers in
//! half blocks, quadrants and Braille with blits that blend, scale and
//! flip. Nothing in them is about one game.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use intuituive::interact::{Button, Event, Mouse, MouseKind};
use intuituive::pixels::{pixels, PixelBuf, PixelMode, PixelSprite, Rgba};
use intuituive::prelude::*;
use intuituive::sprite::{
    bounds_overlap, overlaps, overlaps_at, stage, stage_with, tween, Placed, Sprite, SpriteCell,
    StageOptions,
};
use intuituive::{Driver, Easing};

const RED: Rgba = [255, 0, 0, 255];
const BLUE: Rgba = [0, 0, 255, 255];
const GREEN: Rgba = [0, 255, 0, 255];

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

fn rows(driver: &Driver) -> Vec<String> {
    driver.screen().plain()
}

fn color(c: Option<&intuituive::rich::color::Color>) -> Option<(u8, u8, u8)> {
    c.and_then(|c| c.get_truecolor())
        .map(|t| (t.red, t.green, t.blue))
}

/// The foreground and background at `x`, `y`, as RGB.
type Colors = (Option<(u8, u8, u8)>, Option<(u8, u8, u8)>);

fn colors(driver: &Driver, x: u16, y: u16) -> Colors {
    let screen = driver.screen();
    match screen.style(screen.cell(x, y).style) {
        Some(style) => (color(style.color()), color(style.bgcolor())),
        None => (None, None),
    }
}

fn mouse(driver: &mut Driver, kind: MouseKind, column: u16, row: u16) {
    driver.event(Event::Mouse(Mouse::new(kind, column, row)));
    driver.settle();
}

// Composition and transparency.

#[test]
fn transparent_cells_show_the_backdrop_and_cells_with_no_background_take_its_colour() {
    let app = App::new(|| {
        let ship = Sprite::from_markup(
            &[".[#ff0000]A[/].", "[#ffffff on #00ff00]=[/][#ff0000]=[/]="],
            '.',
        );
        stage_with(
            move || vec![Placed::new(0, ship.clone()).at(1.0, 1.0)],
            StageOptions::new().backdrop(fill(":", "on #0000ff")),
        )
    });
    let mut driver = app.driver(5, 3);
    driver.settle();
    assert_eq!(rows(&driver), [":::::", "::A::", ":===:"]);
    // `A` is red on the backdrop's blue; the cell beside it is backdrop.
    assert_eq!(
        colors(&driver, 2, 1),
        (Some((255, 0, 0)), Some((0, 0, 255)))
    );
    assert_eq!(colors(&driver, 1, 1).1, Some((0, 0, 255)));
    // A cell with its own background keeps it.
    assert_eq!(colors(&driver, 1, 2).1, Some((0, 255, 0)));
    assert_eq!(
        colors(&driver, 2, 2),
        (Some((255, 0, 0)), Some((0, 0, 255)))
    );
}

#[test]
fn a_stage_background_goes_under_sprites_and_cells_build_sprites_too() {
    let app = App::new(|| {
        let wall = SpriteCell::styled("#", "yellow");
        let sprite = Sprite::from_cells(vec![vec![wall.clone(), SpriteCell::transparent(), wall]]);
        stage(move || vec![Placed::new(0, sprite.clone())]).bg("#102030")
    });
    let mut driver = app.driver(4, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["# # "]);
    assert_eq!(colors(&driver, 0, 0).1, Some((0x10, 0x20, 0x30)));
    assert_eq!(colors(&driver, 1, 0).1, Some((0x10, 0x20, 0x30)));
}

#[test]
fn moving_a_sprite_restores_what_was_beneath() {
    let app = App::new(|| {
        let x = signal(0.0);
        let block = Sprite::from_markup(&["@@"], '.');
        stage_with(
            move || vec![Placed::new(0, block.clone()).at(x.get(), 0.0)],
            StageOptions::new().backdrop(fill("-", "")),
        )
        .on_key("right", move |_| x.update(|x| *x += 2.0))
    });
    let mut driver = app.driver(6, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["@@----"]);
    driver.press("right");
    assert_eq!(rows(&driver), ["--@@--"]);
}

// Z order.

#[test]
fn higher_z_draws_over_lower_and_equal_z_keeps_the_order_given() {
    let app = App::new(|| {
        let a = Sprite::from_markup(&["AAA"], '.');
        let b = Sprite::from_markup(&["BBB"], '.');
        let c = Sprite::from_markup(&["C"], '.');
        let raised = signal(false);
        stage(move || {
            vec![
                Placed::new(1, a.clone())
                    .at(0.0, 0.0)
                    .z(if raised.get() { 5 } else { 0 }),
                Placed::new(2, b.clone()).at(1.0, 0.0),
                Placed::new(3, c.clone()).at(2.0, 0.0),
            ]
        })
        .on_key("r", move |_| raised.set(true))
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["ABCB "]);
    driver.press("r");
    assert_eq!(rows(&driver), ["AAAB "]);
}

// Clipping.

#[test]
fn sprites_are_clipped_to_the_stage_on_every_side() {
    let app = App::new(|| {
        let square = Sprite::from_markup(&["123", "456", "789"], '.');
        column([
            label("top"),
            row([
                label("|"),
                stage(move || {
                    vec![
                        Placed::new(0, square.clone()).at(-1.0, -1.0),
                        Placed::new(1, square.clone()).at(3.0, 1.0),
                    ]
                })
                .fixed(4),
                label("|"),
            ])
            .fixed(3),
            label("end"),
        ])
    });
    let mut driver = app.driver(6, 5);
    driver.settle();
    assert_eq!(
        rows(&driver),
        ["top   ", "|56  |", " 89 1 ", "    4 ", "end   "]
    );
}

// Hit testing.

#[test]
fn a_click_finds_the_topmost_sprite_by_its_opaque_cells() {
    let app = App::new(|| {
        let clicked = signal(String::from("-"));
        let ring = Sprite::from_markup(&["ooo", "o.o", "ooo"], '.');
        let dot = Sprite::from_markup(&["*"], '.');
        column([
            stage_with(
                move || {
                    vec![
                        Placed::new("dot", dot.clone()).at(1.0, 1.0).z(-1),
                        Placed::new("ring", ring.clone()).at(0.0, 0.0),
                        Placed::new("far", dot.clone()).at(6.0, 0.0),
                    ]
                },
                StageOptions::new().on_sprite_click(move |id: &str, _| clicked.set(id.to_string())),
            )
            .fixed(3)
            .on_click(move |_| clicked.set("stage".into())),
            text!("{clicked}"),
        ])
    });
    let mut driver = app.driver(8, 4);
    driver.settle();
    mouse(&mut driver, MouseKind::Down(Button::Left), 0, 0);
    assert_eq!(rows(&driver)[3].trim_end(), "ring");
    // The ring's hole shows the dot under it: the dot is hit.
    mouse(&mut driver, MouseKind::Down(Button::Left), 1, 1);
    assert_eq!(rows(&driver)[3].trim_end(), "dot");
    mouse(&mut driver, MouseKind::Down(Button::Left), 6, 0);
    assert_eq!(rows(&driver)[3].trim_end(), "far");
    // No sprite: the click goes on to the stage's own handler.
    mouse(&mut driver, MouseKind::Down(Button::Left), 4, 2);
    assert_eq!(rows(&driver)[3].trim_end(), "stage");
}

#[test]
fn hovering_reports_the_sprite_under_the_pointer_and_leaving_it() {
    let app = App::new(|| {
        let over = signal(String::from("none"));
        let a = Sprite::from_markup(&["AA"], '.');
        column([
            stage_with(
                move || vec![Placed::new(1u8, a.clone()).at(2.0, 0.0)],
                StageOptions::new().on_sprite_hover(move |id: Option<u8>, _| {
                    over.set(id.map_or("none".into(), |id| format!("sprite {id}")))
                }),
            )
            .fixed(1),
            text!("{over}"),
        ])
    });
    let mut driver = app.driver(10, 2);
    driver.settle();
    mouse(&mut driver, MouseKind::Moved, 0, 0);
    assert_eq!(rows(&driver)[1].trim_end(), "none");
    mouse(&mut driver, MouseKind::Moved, 3, 0);
    assert_eq!(rows(&driver)[1].trim_end(), "sprite 1");
    mouse(&mut driver, MouseKind::Moved, 5, 0);
    assert_eq!(rows(&driver)[1].trim_end(), "none");
    mouse(&mut driver, MouseKind::Moved, 2, 0);
    assert_eq!(rows(&driver)[1].trim_end(), "sprite 1");
    // Off the stage: the pointer left the sprite.
    mouse(&mut driver, MouseKind::Moved, 2, 1);
    assert_eq!(rows(&driver)[1].trim_end(), "none");
}

// Collisions.

#[test]
fn overlaps_tests_cells_and_bounds_overlap_tests_boxes() {
    let l = Sprite::from_markup(&["#.", "##"], '.');
    let dot = Sprite::from_markup(&["o"], '.');
    let a = Placed::new(1, l.clone()).at(0.0, 0.0);
    let corner = Placed::new(2, dot.clone()).at(1.0, 0.0);
    let away = Placed::new(3, dot.clone()).at(5.0, 5.0);
    assert!(bounds_overlap(&a, &corner) && !overlaps(&a, &corner));
    assert!(!bounds_overlap(&a, &away) && !overlaps(&a, &away));
    assert!(overlaps(&a, &corner.clone().at(0.6, 1.4)));
    // Anchors move the cells checked; a flip mirrors them.
    let anchored = Placed::new(4, l.clone().anchor(1, 1)).at(1.0, 1.0);
    assert!(overlaps(
        &anchored,
        &Placed::new(5, dot.clone()).at(0.0, 0.0)
    ));
    let flipped = a.clone().flip(true);
    assert!(overlaps(&flipped, &corner));
    // At a time on the clock: a tween's position then.
    let moving = Placed::new(6, dot.clone()).tween(intuituive::sprite::Tween::new(
        (5.0, 1.0),
        (0.0, 1.0),
        ms(0),
        ms(100),
        Easing::Linear,
    ));
    assert!(!overlaps_at(&a, &moving, ms(0)));
    assert!(overlaps_at(&a, &moving, ms(100)));
}

#[test]
fn a_flipped_sprite_mirrors_its_cells_and_symbols() {
    let app = App::new(|| {
        let arrow = Sprite::from_markup(&["<=(", "/.."], '.');
        stage(move || {
            vec![
                Placed::new(0, arrow.clone()),
                Placed::new(1, arrow.clone()).at(4.0, 0.0).flip(true),
            ]
        })
    });
    let mut driver = app.driver(7, 2);
    driver.settle();
    assert_eq!(rows(&driver), ["<=( )=>", "/     \\"]);
}

// Animation on the app's clock.

#[test]
fn frames_change_on_the_app_clock_looping_or_once() {
    let app = App::new(|| {
        let frames = || ["1", "2", "3"].map(|s| Sprite::from_markup(&[s], '.'));
        let looping = Sprite::from_frames(frames()).period(ms(100));
        let once = Sprite::from_frames(frames()).period(ms(100)).once();
        let timed = Sprite::from_frames(frames()).durations(&[ms(50), ms(200)]);
        stage(move || {
            vec![
                Placed::new(0, looping.clone()),
                Placed::new(1, once.clone()).at(2.0, 0.0),
                Placed::new(2, timed.clone()).at(4.0, 0.0),
                Placed::new(3, looping.clone()).at(6.0, 0.0).frame(2),
            ]
        })
    });
    let mut driver = app.driver(7, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["1 1 1 3"]);
    driver.advance(ms(60));
    assert_eq!(rows(&driver), ["1 1 2 3"]);
    driver.advance(ms(60));
    assert_eq!(rows(&driver), ["2 2 2 3"]);
    driver.advance(ms(100));
    assert_eq!(rows(&driver), ["3 3 2 3"]);
    driver.advance(ms(100));
    // 320 ms: the loop is back at its first frame; once stays on its last.
    assert_eq!(rows(&driver), ["1 3 3 3"]);
    let stats = driver.stats();
    assert!(stats.frames > 0);
}

#[test]
fn an_animation_started_later_plays_from_its_start() {
    let app = App::new(|| {
        let pop = Sprite::from_frames(["a", "b", "c"].map(|s| Sprite::from_markup(&[s], '.')))
            .period(ms(100))
            .once();
        let since = signal(None::<Duration>);
        stage(move || {
            since
                .get()
                .map(|t| Placed::new(0, pop.clone()).started(t))
                .into_iter()
                .collect()
        })
        .on_key("x", move |cx| since.set(Some(cx.now())))
    });
    let mut driver = app.driver(1, 1);
    driver.advance(ms(1000));
    assert_eq!(rows(&driver), [" "]);
    driver.press("x");
    assert_eq!(rows(&driver), ["a"]);
    driver.advance(ms(150));
    assert_eq!(rows(&driver), ["b"]);
    driver.advance(ms(500));
    assert_eq!(rows(&driver), ["c"]);
}

#[test]
fn a_tween_moves_a_sprite_frame_by_frame_and_stops() {
    let app = App::new(|| {
        let ball = Sprite::from_markup(&["o"], '.');
        let path = signal(None);
        stage(move || {
            let place = Placed::new(0, ball.clone());
            vec![match path.get() {
                Some(t) => place.tween(t),
                None => place,
            }]
        })
        .on_key("g", move |_| {
            path.set(Some(tween((0.0, 0.0), (8.0, 0.0), ms(800), Easing::Linear)))
        })
    });
    let mut driver = app.driver(9, 1);
    driver.settle();
    driver.press("g");
    assert_eq!(rows(&driver), ["o        "]);
    driver.advance(ms(400));
    assert_eq!(rows(&driver), ["    o    "]);
    driver.advance(ms(500));
    assert_eq!(rows(&driver), ["        o"]);
    // Arrived: no more frames are asked for.
    let before = driver.stats().frames;
    driver.advance(ms(500));
    assert_eq!(driver.stats().frames, before);
}

#[test]
fn ctx_animate_moves_a_sprite_too() {
    let app = App::new(|| {
        let x = signal(0.0);
        let ball = Sprite::from_markup(&["o"], '.');
        stage(move || vec![Placed::new(0, ball.clone()).at(x.get(), 0.0)])
            .on_key("g", move |cx| cx.animate(x, 4.0, ms(400), Easing::Linear))
    });
    let mut driver = app.driver(5, 1);
    driver.press("g");
    driver.advance(ms(200));
    assert_eq!(rows(&driver), ["  o  "]);
    driver.advance(ms(300));
    assert_eq!(rows(&driver), ["    o"]);
}

// Pixel art as sprites.

#[test]
fn pixel_sprites_draw_in_half_blocks_and_move_by_half_cells() {
    let dot = PixelSprite::from_art(&["r"], &[('r', RED)]);
    let app = App::new(move || {
        let sprite = Sprite::from_pixels(&dot);
        let y = signal(0.0);
        stage_with(
            move || vec![Placed::new(0, sprite.clone()).at(0.0, y.get())],
            StageOptions::new()
                .half_cells(true)
                .backdrop(fill(" ", "on #0000ff")),
        )
        .on_key("down", move |_| y.update(|y| *y += 0.5))
    });
    let mut driver = app.driver(1, 2);
    driver.settle();
    assert_eq!(rows(&driver), ["▀", " "]);
    // The empty half takes the backdrop's blue.
    assert_eq!(
        colors(&driver, 0, 0),
        (Some((255, 0, 0)), Some((0, 0, 255)))
    );
    driver.press("down");
    assert_eq!(rows(&driver), ["▄", " "]);
    driver.press("down");
    assert_eq!(rows(&driver), [" ", "▀"]);
}

// Pixel buffers.

#[test]
fn a_pixel_buffer_draws_in_half_blocks_two_pixels_a_cell() {
    let app = App::new(|| {
        pixels(3, 3, |buf| {
            buf.set(0, 0, RED);
            buf.set(0, 1, BLUE);
            buf.set(1, 1, GREEN);
            buf.set(2, 2, RED);
        })
    });
    let mut driver = app.driver(4, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["▀▄  ", "  ▀ ", "    "]);
    assert_eq!(
        colors(&driver, 0, 0),
        (Some((255, 0, 0)), Some((0, 0, 255)))
    );
    assert_eq!(colors(&driver, 1, 0), (Some((0, 255, 0)), None));
    assert_eq!(colors(&driver, 2, 1), (Some((255, 0, 0)), None));
}

#[test]
fn a_pixel_buffer_redraws_when_a_signal_it_read_changes() {
    let app = App::new(|| {
        let x = signal(0);
        pixels(4, 2, move |buf| {
            buf.clear([0, 0, 0, 0]);
            buf.fill_rect(x.get(), 0, 1, 2, RED);
        })
        .on_key("right", move |_| x.update(|x| *x += 1))
    });
    let mut driver = app.driver(4, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["▀   "]);
    driver.press("right right");
    assert_eq!(rows(&driver), ["  ▀ "]);
}

#[test]
fn quadrants_and_braille_hold_more_pixels_a_cell() {
    let shape = |buf: &mut PixelBuf| {
        buf.set(0, 0, RED);
        buf.set(1, 1, RED);
        buf.set(2, 0, RED);
        buf.set(3, 0, RED);
        buf.set(2, 1, RED);
        buf.set(3, 1, RED);
    };
    let app = App::new(move || pixels(4, 4, shape).pixel_mode(PixelMode::Quadrants));
    let mut driver = app.driver(2, 2);
    driver.settle();
    assert_eq!(rows(&driver), ["▚█", "  "]);

    let app = App::new(|| {
        pixels(2, 4, |buf| {
            for y in 0..4 {
                buf.set(0, y, BLUE);
            }
            buf.set(1, 3, BLUE);
        })
        .pixel_mode(PixelMode::Braille)
    });
    let mut driver = app.driver(1, 1);
    driver.settle();
    // Dots 1, 2, 3, 7 and 8.
    assert_eq!(rows(&driver), ["\u{28C7}"]);
}

#[test]
fn quadrants_of_two_colours_split_by_brightness() {
    let app = App::new(|| {
        pixels(2, 2, |buf| {
            buf.clear([20, 20, 20, 255]);
            buf.set(1, 0, [250, 250, 250, 255]);
        })
        .pixel_mode(PixelMode::Quadrants)
    });
    let mut driver = app.driver(1, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["▝"]);
    assert_eq!(
        colors(&driver, 0, 0),
        (Some((250, 250, 250)), Some((20, 20, 20)))
    );
}

#[test]
fn blits_blend_by_alpha_scale_and_flip() {
    let sprite = PixelSprite::from_art(&["rb", "g."], &[('r', RED), ('b', BLUE), ('g', GREEN)]);
    let mut buf = PixelBuf::new(6, 4);
    buf.clear([0, 0, 0, 255]);
    buf.blit(&sprite, 0, 0, 1.0, false);
    assert_eq!(buf.get(0, 0), Some(RED));
    assert_eq!(buf.get(1, 0), Some(BLUE));
    assert_eq!(
        buf.get(1, 1),
        Some([0, 0, 0, 255]),
        "a transparent pixel leaves what is there"
    );
    // Flipped: blue on the left.
    buf.blit(&sprite, 2, 0, 1.0, true);
    assert_eq!(buf.get(2, 0), Some(BLUE));
    assert_eq!(buf.get(3, 0), Some(RED));
    assert_eq!(buf.get(3, 1), Some(GREEN));
    // Scaled twice: each pixel is 2x2, clipped at the edge.
    let mut big = PixelBuf::new(3, 3);
    big.blit(&sprite, 0, 0, 2.0, false);
    assert_eq!(big.get(1, 1), Some(RED));
    assert_eq!(big.get(2, 1), Some(BLUE));
    assert_eq!(big.get(0, 2), Some(GREEN));
    assert_eq!(big.get(2, 2), Some([0, 0, 0, 0]));
    // Half alpha over white.
    let ghost = PixelSprite::new(1, 1, vec![[0, 0, 0, 128]]);
    let mut white = PixelBuf::new(1, 1);
    white.clear([255, 255, 255, 255]);
    white.blit(&ghost, 0, 0, 1.0, false);
    assert_eq!(white.get(0, 0), Some([127, 127, 127, 255]));
    // Off the buffer: nothing, no panic.
    white.blit(&sprite, -5, 9, 3.0, true);
    // Scaled down: one pixel of the four.
    let mut small = PixelBuf::new(2, 2);
    small.blit(&sprite, 0, 0, 0.5, false);
    assert_eq!(small.get(0, 0), Some(RED));
    assert_eq!(small.get(1, 0), Some([0, 0, 0, 0]));
}

#[test]
fn pixel_sprite_frames_follow_the_clock() {
    let walk = PixelSprite::from_art(&["r"], &[('r', RED)])
        .with_frame(vec![BLUE])
        .with_frame(vec![GREEN])
        .period(ms(100));
    assert_eq!(walk.frame_count(), 3);
    assert_eq!(walk.frame_at(ms(150)), 1);
    assert_eq!(walk.frame_at(ms(350)), 0);
    let mut buf = PixelBuf::new(1, 1);
    buf.blit_frame(&walk, walk.frame_at(ms(250)), 0, 0, 1.0, false);
    assert_eq!(buf.get(0, 0), Some(GREEN));
    // As a sprite, the same frames at the same times.
    let sprite = Sprite::from_pixels(&walk);
    assert_eq!((sprite.frame_count(), sprite.frame_at(ms(150))), (3, 1));
}

#[test]
fn lines_and_rectangles_clip() {
    let mut buf = PixelBuf::new(4, 4);
    buf.line(-2, -2, 5, 5, RED);
    for i in 0..4 {
        assert_eq!(buf.get(i, i), Some(RED));
    }
    assert_eq!(buf.get(1, 0), Some([0, 0, 0, 0]));
    buf.fill_rect(-10, 3, 100, 9, BLUE);
    assert_eq!(buf.pixels()[12..], [BLUE; 4]);
    assert_eq!(buf.to_rgba8().len(), 64);
}

// Cost: a few hundred moving sprites on a large screen.

#[test]
fn three_hundred_moving_sprites_on_a_200_by_60_stage() {
    let frame = Rc::new(RefCell::new(0u32));
    let tick = frame.clone();
    let app = App::new(move || {
        let t = signal(0u32);
        let ship = Sprite::from_markup(&[".[cyan]/\\[/].", "[cyan]<[bold red]##[/]>[/]"], '.');
        let blob = Sprite::from_pixels(&PixelSprite::from_art(
            &[".gg.", "gggg", "gggg", ".gg."],
            &[('g', GREEN)],
        ));
        let sprites: Vec<(Sprite, f64, f64)> = (0..300)
            .map(|i| {
                let sprite = if i % 2 == 0 {
                    ship.clone()
                } else {
                    blob.clone()
                };
                (sprite, (i * 37 % 200) as f64, (i * 11 % 60) as f64)
            })
            .collect();
        let tick = tick.clone();
        stage_with(
            move || {
                let t = t.get() as f64;
                sprites
                    .iter()
                    .enumerate()
                    .map(|(i, (sprite, x, y))| {
                        Placed::new(i, sprite.clone())
                            .at((x + t * 0.7) % 204.0 - 2.0, (y + t * 0.3) % 62.0 - 1.0)
                            .z(i as i32 % 3)
                    })
                    .collect()
            },
            StageOptions::new().backdrop(fill("·", "grey30 on #101820")),
        )
        .on_key("n", move |_| {
            *tick.borrow_mut() += 1;
            t.update(|t| *t += 1);
        })
    });
    let mut driver = app.driver(200, 60);
    driver.settle();
    let frames = 60;
    let start = Instant::now();
    for _ in 0..frames {
        driver.press("n");
    }
    let per_frame = start.elapsed() / frames;
    assert_eq!(*frame.borrow(), frames);
    eprintln!("300 sprites at 200x60: {per_frame:?} a frame");
    // Debug builds are several times slower than release; this is a
    // guard against a quadratic mistake, not the budget (16 ms in release).
    assert!(per_frame < ms(250), "{per_frame:?} a frame");
}

#[test]
fn a_full_screen_pixel_buffer_at_200_by_60() {
    let app = App::new(|| {
        let t = signal(0i32);
        intuituive::pixels::pixels_fit(move |buf| {
            let t = t.get();
            let (w, h) = (buf.width() as i32, buf.height() as i32);
            for y in 0..h {
                for x in 0..w {
                    let v = ((x + t) ^ y) as u8;
                    buf.set(x, y, [v, v.wrapping_mul(3), 255 - v, 255]);
                }
            }
        })
        .on_key("n", move |_| t.update(|t| *t += 1))
    });
    let mut driver = app.driver(200, 60);
    driver.settle();
    let frames = 30;
    let start = Instant::now();
    for _ in 0..frames {
        driver.press("n");
    }
    let per_frame = start.elapsed() / frames;
    eprintln!("a 200x120 pixel buffer: {per_frame:?} a frame");
    assert!(per_frame < ms(500), "{per_frame:?} a frame");
}

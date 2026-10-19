//! rs-rich raycaster (examples/raycast): the levels and rules on the model,
//! the framebuffer the ray caster draws, and the app driven by keys and the
//! app's clock, a tic at a time, so every run is the same game.

#[path = "../examples/raycast/main.rs"]
#[allow(dead_code)]
mod raycast;

use std::time::Duration;

use intuituive::interact::{Event, Key};
use intuituive::Driver;
use raycast::*;

const W: u16 = 80;
const H: u16 = 24;

fn options() -> Options {
    Options {
        start: true,
        ..Options::default()
    }
}

fn start(options: Options, width: u16, height: u16) -> (Driver, Probe) {
    let (app, probe) = raycast_app(options);
    let mut driver = app.driver(width, height);
    driver.settle();
    driver.advance(TIC);
    (driver, probe)
}

fn playing() -> (Driver, Probe) {
    start(options(), W, H)
}

fn tics(n: u32) -> Duration {
    TIC * n
}

fn rows(driver: &Driver) -> Vec<String> {
    driver
        .screen()
        .plain()
        .iter()
        .map(|r| r.trim_end().to_string())
        .collect()
}

fn has(driver: &Driver, needle: &str) -> bool {
    rows(driver).iter().any(|r| r.contains(needle))
}

fn dump(driver: &Driver) -> String {
    rows(driver).join("\n")
}

/// Put the player at `(x, y)` facing `angle` (0 east, π/2 south).
fn place(probe: &Probe, x: f64, y: f64, angle: f64) {
    let mut game = probe.game.borrow_mut();
    game.player.x = x;
    game.player.y = y;
    game.player.angle = angle;
}

/// The centre column's wall in the last frame drawn.
fn centre(probe: &Probe) -> Option<(Wall, f64)> {
    probe.frame.borrow().centre
}

fn key_up(driver: &mut Driver, name: &str) {
    driver.event(Event::KeyUp(Key::parse(name).expect("a key")));
    driver.settle();
}

// The levels.

#[test]
fn the_levels_are_closed_with_a_start_an_exit_and_doors_between_walls() {
    for (n, level) in LEVELS.iter().enumerate() {
        let width = level.rows[0].len();
        assert!(level.rows.iter().all(|r| r.len() == width), "level {n}");
        let game = Game::new(n, 1);
        for y in 0..game.height {
            for x in 0..game.width {
                let edge = x == 0 || y == 0 || x == game.width - 1 || y == game.height - 1;
                if edge {
                    assert!(
                        matches!(game.map[y][x], Tile::Wall(_)),
                        "level {n} is open at {x},{y}"
                    );
                }
            }
        }
        let starts = level
            .rows
            .iter()
            .flat_map(|r| r.chars())
            .filter(|c| "<>^v".contains(*c))
            .count();
        assert_eq!(starts, 1, "level {n}");
        assert!(
            level.rows.iter().any(|r| r.contains('X')),
            "level {n} has no exit"
        );
        for door in &game.doors {
            let wall = |x: usize, y: usize| matches!(game.map[y][x], Tile::Wall(_));
            let between = if door.across_x {
                wall(door.x, door.y - 1) && wall(door.x, door.y + 1)
            } else {
                wall(door.x - 1, door.y) && wall(door.x + 1, door.y)
            };
            assert!(between, "level {n}: the door at {},{}", door.x, door.y);
        }
        assert!(game.enemies() > 0 && game.pickups() > 0);
    }
}

// The screens.

#[test]
fn the_title_starts_the_game_on_enter() {
    let (mut driver, probe) = start(Options::default(), W, H);
    assert!(has(&driver, "Enter start"), "{}", dump(&driver));
    assert_eq!(probe.game.borrow().state, State::Title);
    driver.press("enter");
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().state, State::Playing);
    assert!(!has(&driver, "Enter start"));
    assert!(has(&driver, "E1 Hangar"), "{}", dump(&driver));
    assert!(has(&driver, "Kills 0/4  Items 0/6"), "{}", dump(&driver));
}

#[test]
fn the_title_picks_the_second_level() {
    let (mut driver, probe) = start(Options::default(), W, H);
    driver.press("2");
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().level_name(), "E2 Depot");
    assert!(has(&driver, "E2 Depot"));
}

#[test]
fn the_status_bar_has_digits_a_face_and_the_frame_rate() {
    let (mut driver, _) = playing();
    driver.advance(Duration::from_secs(2));
    let text = dump(&driver);
    for title in ["Ammo", "Health", "Armour", "rs-rich raycaster"] {
        assert!(text.contains(title), "{text}");
    }
    // `digits` draws 50 in box glyphs, and reads 100% aloud.
    assert!(text.contains("┌─╴┌─┐"), "{text}");
    let names: Vec<String> = driver
        .accessibility()
        .into_iter()
        .map(|n| n.name)
        .collect();
    assert!(names.iter().any(|n| n.contains("100%")), "{names:?}");
    // One frame a tic: 35 a second on the app's clock.
    assert!(text.contains("35 fps · half blocks"), "{text}");
}

// Moving: held keys.

#[test]
fn a_press_without_a_release_walks_for_a_few_tics() {
    let (mut driver, probe) = playing();
    let x0 = probe.game.borrow().player.x;
    driver.press("w");
    driver.advance(tics(20));
    let walked = probe.game.borrow().player.x - x0;
    // Latched for FIRST_LATCH tics, at 0.085 a tic.
    assert!((walked - 0.085 * 8.0).abs() < 1e-6, "{walked}");
    // An autorepeat soon after keeps it going.
    driver.press("w");
    driver.advance(tics(3));
    driver.press("w");
    driver.advance(tics(20));
    let walked = probe.game.borrow().player.x - x0;
    assert!(walked > 0.085 * 14.0, "{walked}");
}

#[test]
fn a_key_seen_let_go_is_held_until_it_is_released() {
    let (mut driver, probe) = playing();
    // The terminal reports releases (the kitty keyboard protocol): once
    // one arrives for ↑, ↑ is held by press and release.
    key_up(&mut driver, "up");
    let x0 = probe.game.borrow().player.x;
    driver.press("up");
    driver.advance(tics(30));
    let walked = probe.game.borrow().player.x - x0;
    assert!(walked > 0.085 * 29.0, "held, not latched: {walked}");
    key_up(&mut driver, "up");
    let x1 = probe.game.borrow().player.x;
    driver.advance(tics(10));
    assert_eq!(probe.game.borrow().player.x, x1);
    // `w` never sent a release: it is still latched.
    driver.press("w");
    driver.advance(tics(30));
    let walked = probe.game.borrow().player.x - x1;
    assert!((walked - 0.085 * 8.0).abs() < 1e-6, "{walked}");
}

#[test]
fn turning_strafing_and_walls() {
    let (mut driver, probe) = playing();
    driver.press("d");
    driver.advance(tics(10));
    let angle = probe.game.borrow().player.angle;
    assert!((angle - 0.065 * 8.0).abs() < 1e-9, "{angle}");
    probe.game.borrow_mut().things.clear();
    // Strafing left facing east goes north, until the wall stops it.
    place(&probe, 5.5, 1.5, 0.0);
    driver.press("q");
    driver.advance(tics(10));
    let game = probe.game.borrow();
    assert!(
        game.player.y >= 1.24 && game.player.y < 1.5,
        "{}",
        game.player.y
    );
    assert_eq!(game.player.x, 5.5);
}

// The ray caster.

#[test]
fn the_centre_column_sees_brick_then_the_door_after_moving() {
    let (mut driver, probe) = playing();
    // From the start room's top row, facing east: the brick wall five and
    // a half away.
    place(&probe, 1.5, 1.5, 0.0);
    driver.advance(TIC);
    let (wall, dist) = centre(&probe).expect("a wall ahead");
    assert_eq!(wall, Wall::Brick);
    assert!((dist - 5.5).abs() < 0.01, "{dist}");
    // Brick red at the centre of the frame.
    let frame = probe.frame.borrow().clone();
    let [r, g, b] = frame.pixel(frame.width / 2, frame.height / 2);
    assert!(r > g && r > b, "{r},{g},{b}");
    // Back at the start, in the door's row, facing it.
    place(&probe, 1.5, 3.5, 0.0);
    driver.advance(TIC);
    let (wall, dist) = centre(&probe).unwrap();
    assert_eq!(wall, Wall::Door);
    assert!((dist - 6.0).abs() < 0.01, "{dist}");
}

#[test]
fn space_opens_a_door_and_the_ray_goes_through() {
    let (mut driver, probe) = playing();
    // No enemy to walk into the doorway and hold it open.
    probe.game.borrow_mut().things.clear();
    place(&probe, 6.3, 3.5, 0.0);
    driver.advance(TIC);
    assert_eq!(centre(&probe).unwrap().0, Wall::Door);
    assert!(probe.game.borrow().blocks_at(7.5, 3.5));
    driver.press("space");
    driver.advance(tics(31));
    let door = probe.game.borrow().doors[0].clone();
    assert_eq!((door.x, door.y), (7, 3));
    assert!(matches!(door.state, DoorState::Open(_)), "{door:?}");
    assert!(!probe.game.borrow().blocks_at(7.5, 3.5));
    // Through it: the next door, across the middle room.
    let (wall, dist) = centre(&probe).unwrap();
    assert_eq!(wall, Wall::Door);
    assert!(dist > 8.0, "{dist}");
    // It closes by itself once the player is out of it.
    driver.advance(tics(140 + 40));
    assert_eq!(probe.game.borrow().doors[0].state, DoorState::Closed);
}

#[test]
fn walls_beside_a_door_are_drawn_as_its_frame() {
    let (mut driver, probe) = playing();
    {
        let mut game = probe.game.borrow_mut();
        game.doors[0].open = 1.0;
        game.doors[0].state = DoorState::Open(1000);
    }
    // In the open doorway, looking north at the jamb.
    place(&probe, 7.5, 3.5, -std::f64::consts::FRAC_PI_2);
    driver.advance(TIC);
    assert_eq!(centre(&probe).unwrap().0, Wall::Frame);
}

#[test]
fn sprites_are_drawn_in_front_and_hidden_behind_walls() {
    let (mut driver, probe) = playing();
    // The column at (3,1), seen from the south: grey stone at the centre.
    place(&probe, 3.5, 4.0, -std::f64::consts::FRAC_PI_2);
    driver.advance(TIC);
    let frame = probe.frame.borrow().clone();
    let column = frame.pixel(frame.width / 2, frame.height / 2 - 2);
    let spread = column.iter().max().unwrap() - column.iter().min().unwrap();
    assert!(spread < 40, "{column:?}");
    let (_, wall) = centre(&probe).unwrap();
    assert!(wall > 1.5, "the column is not a wall: {wall}");
    // The same column behind the brick wall, seen from the middle room:
    // the wall hides it.
    place(&probe, 9.5, 1.5, std::f64::consts::PI);
    driver.advance(TIC);
    assert_eq!(centre(&probe).unwrap().0, Wall::Brick);
    let frame = probe.frame.borrow().clone();
    assert_ne!(frame.pixel(frame.width / 2, frame.height / 2 - 2), column);
}

#[test]
fn the_flat_floor_differs_from_the_cast_one() {
    let (_, cast) = playing();
    let (_, flat) = start(
        Options {
            floor_cast: false,
            ..options()
        },
        W,
        H,
    );
    let (a, b) = (cast.frame.borrow().clone(), flat.frame.borrow().clone());
    assert_eq!((a.width, a.height), (80, 38));
    let differ = (0..a.width)
        .filter(|&x| a.pixel(x, a.height - 1) != b.pixel(x, a.height - 1))
        .count();
    assert!(differ > 20, "{differ}");
    // The walls are the same.
    assert_eq!(
        a.pixel(a.width / 2, a.height / 2),
        b.pixel(b.width / 2, b.height / 2)
    );
}

#[test]
fn the_same_keys_give_the_same_frames() {
    let run = || {
        let (mut driver, probe) = playing();
        for key in ["w", "d", "w", "w", "f", "e", "w"] {
            driver.press(key);
            driver.advance(tics(6));
        }
        let frame = probe.frame.borrow().clone();
        frame
            .rgb
            .iter()
            .fold(0u64, |h, b| h.wrapping_mul(31).wrapping_add(*b as u64))
    };
    assert_eq!(run(), run());
}

#[test]
fn the_framebuffer_follows_the_view_and_the_mode() {
    let (_, probe) = start(options(), 160, 50);
    let frame = probe.frame.borrow().clone();
    assert_eq!((frame.width, frame.height), (160, 90));
    let (_, probe) = start(
        Options {
            mode: Some(Mode::Braille),
            ..options()
        },
        W,
        H,
    );
    let frame = probe.frame.borrow().clone();
    assert_eq!((frame.width, frame.height), (160, 76));
}

// Fighting and picking up.

#[test]
fn firing_spends_ammo_flashes_and_kills_an_enemy_in_line() {
    let (mut driver, probe) = playing();
    // In the middle room, facing the enemy at (11, 2).
    place(&probe, 8.5, 2.5, 0.0);
    driver.press("f");
    assert_eq!(probe.game.borrow().player.ammo, 49);
    assert!(probe.game.borrow().flash > 0);
    let killed = (0..4).any(|_| {
        driver.advance(tics(13));
        driver.press("f");
        probe.game.borrow().kills == 1
    });
    assert!(killed, "{:?}", probe.game.borrow().things);
    driver.advance(tics(30));
    assert!(has(&driver, "Kills 1/4"), "{}", dump(&driver));
    let game = probe.game.borrow();
    let dead = game
        .things
        .iter()
        .find(|t| t.kind == Kind::Enemy && !t.alive())
        .unwrap();
    assert!(matches!(dead.mode, EnemyMode::Dead(_)));
}

#[test]
fn a_shot_into_a_wall_hits_nothing() {
    let (mut driver, probe) = playing();
    driver.press("f");
    driver.advance(tics(20));
    let game = probe.game.borrow();
    assert_eq!(game.player.ammo, 49);
    assert!(game
        .things
        .iter()
        .all(|t| t.kind != Kind::Enemy || t.health == 30));
}

#[test]
fn an_enemy_that_sees_the_player_comes_and_shoots() {
    let (mut driver, probe) = playing();
    place(&probe, 8.5, 4.5, 0.0);
    let enemy = |probe: &Probe| {
        let game = probe.game.borrow();
        let t = game
            .things
            .iter()
            .find(|t| t.kind == Kind::Enemy)
            .unwrap()
            .clone();
        (t.x, t.y, t.mode)
    };
    let (x0, y0, _) = enemy(&probe);
    driver.advance(tics(70));
    let (x1, y1, mode) = enemy(&probe);
    let (px, py) = (8.5, 4.5);
    assert!(
        (x1 - px).hypot(y1 - py) < (x0 - px).hypot(y0 - py),
        "{x0},{y0} -> {x1},{y1} {mode:?}"
    );
    driver.advance(Duration::from_secs(6));
    assert!(probe.game.borrow().player.health < 100);
}

#[test]
fn pickups_add_ammo_health_and_armour_once() {
    let (mut driver, probe) = playing();
    // No enemies to spoil the count.
    probe
        .game
        .borrow_mut()
        .things
        .retain(|t| t.kind != Kind::Enemy);
    place(&probe, 19.5, 2.5, 0.0);
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().player.ammo, 60);
    assert!(has(&driver, "Picked up a clip"), "{}", dump(&driver));
    driver.advance(tics(5));
    assert_eq!(probe.game.borrow().player.ammo, 60);
    // A medikit only when hurt.
    place(&probe, 3.5, 5.5, 0.0);
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().items, 1);
    probe.game.borrow_mut().player.health = 50;
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().player.health, 75);
    // Armour soaks a third of a hit.
    place(&probe, 3.5, 11.5, 0.0);
    driver.advance(TIC);
    let mut game = probe.game.borrow_mut();
    assert_eq!(game.player.armour, 100);
    game.damage(9);
    assert_eq!((game.player.health, game.player.armour), (69, 97));
}

// The rest of the screens.

#[test]
fn the_exit_switch_ends_the_level_and_enter_goes_on() {
    let (mut driver, probe) = playing();
    place(&probe, 21.5, 11.5, 0.0);
    driver.advance(TIC);
    driver.press("space");
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().state, State::LevelDone);
    assert!(has(&driver, "Level complete"), "{}", dump(&driver));
    driver.press("enter");
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().level_name(), "E2 Depot");
    assert!(has(&driver, "E2 Depot"));
    // The last level's exit is the end.
    place(&probe, 21.5, 13.5, 0.0);
    driver.press("space");
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().state, State::Won);
    assert!(has(&driver, "All levels cleared"), "{}", dump(&driver));
}

#[test]
fn dying_shows_the_tally_and_enter_tries_again() {
    let (mut driver, probe) = playing();
    probe.game.borrow_mut().damage(150);
    driver.advance(TIC);
    assert_eq!(probe.game.borrow().state, State::Dead);
    assert!(has(&driver, "You died"), "{}", dump(&driver));
    driver.press("enter");
    driver.advance(TIC);
    let game = probe.game.borrow();
    assert_eq!((game.state, game.player.health), (State::Playing, 100));
}

#[test]
fn escape_pauses_the_game_under_a_menu() {
    let (mut driver, probe) = playing();
    driver.press("esc");
    assert!(has(&driver, "Paused"), "{}", dump(&driver));
    let tics_before = probe.game.borrow().level_tics;
    let frames = probe.frames.get();
    driver.press("w");
    driver.advance(tics(20));
    assert_eq!(probe.game.borrow().level_tics, tics_before);
    assert_eq!(probe.frames.get(), frames, "no frames while paused");
    assert_eq!(probe.game.borrow().player.x, 1.5);
    driver.press("esc");
    driver.advance(tics(2));
    assert!(!has(&driver, "Paused"));
    assert!(probe.game.borrow().level_tics > tics_before);
}

#[test]
fn tab_shows_the_map_with_the_player() {
    let (mut driver, _) = playing();
    driver.press("tab");
    driver.advance(TIC);
    assert!(has(&driver, "Map"), "{}", dump(&driver));
    assert!(has(&driver, "→"), "{}", dump(&driver));
    driver.press("tab");
    driver.advance(TIC);
    assert!(!has(&driver, "→"));
}

#[test]
fn help_lists_the_keys() {
    let (mut driver, _) = start(options(), 100, 50);
    driver.press("?");
    let text = dump(&driver);
    for line in ["Walk forward", "Strafe left", "Fire", "Show or hide the map"] {
        assert!(text.contains(line), "{text}");
    }
}

// Real pixels.

#[test]
fn kitty_and_iterm_modes_send_the_frame_as_an_image() {
    for (mode, escape) in [
        (Mode::Kitty, "\x1b_Ga=T,f=100"),
        (Mode::Iterm, "\x1b]1337;File="),
    ] {
        let (app, probe) = raycast_app(Options {
            mode: Some(mode),
            ..options()
        });
        let mut driver = app.driver(W, H);
        driver.update(Duration::ZERO);
        driver.render();
        driver.update(TIC);
        let out = driver.render().expect("a frame");
        assert!(out.contains(escape), "{mode:?}");
        // Four by eight pixels a cell.
        let frame = probe.frame.borrow().clone();
        assert_eq!((frame.width, frame.height), (320, 152), "{mode:?}");
        // The next tic sends the next frame.
        driver.update(TIC * 2);
        assert!(driver.render().expect("a frame").contains(escape));
    }
}

#[test]
fn a_covered_pixel_view_draws_cells_instead() {
    let (app, probe) = raycast_app(Options {
        mode: Some(Mode::Kitty),
        ..options()
    });
    let mut driver = app.driver(W, H);
    driver.settle();
    driver.press("tab");
    driver.advance(tics(2));
    let frame = probe.frame.borrow().clone();
    assert_eq!((frame.width, frame.height), (80, 38));
    assert!(rows(&driver)[5].contains('▀'), "{}", dump(&driver));
}

#[test]
fn options_parse() {
    let o =
        Options::parse(["--mode", "braille", "--level", "2", "--flat"].map(String::from)).unwrap();
    assert_eq!(
        (o.mode, o.level, o.floor_cast),
        (Some(Mode::Braille), 1, false)
    );
    assert!(Options::parse(["--level", "3"].map(String::from)).is_err());
    assert!(Options::parse(["--mode", "sixel"].map(String::from)).is_err());
}


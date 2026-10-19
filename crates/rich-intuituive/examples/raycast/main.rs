//! rs-rich raycaster: a first-person shooter in the terminal, in the style
//! of the early-90s ray casters, on intuiTUIve. Everything in it is
//! original: the levels, and the textures, sprites and face, which are
//! generated in code. The ray casting follows Lode Vandevenne's tutorial
//! (<https://lodev.org/cgtutor/raycasting.html>).
//!
//!     cargo run -p rs-rich-intuituive --example raycast --features art --release
//!     cargo run ... -- --mode braille        (auto, blocks, quadrants, braille, kitty, iterm)
//!     cargo run ... -- --flat --level 2      (flat fogged floor; start on level 2)
//!
//! The 3D view is drawn into an RGB framebuffer each tic (35 a second, on
//! the app's clock) and put on screen by the `View` widget: half blocks by
//! default, quadrants or Braille, or real pixels through Kitty's or
//! iTerm2's image protocol. Everything round it (the status bar's digits
//! and face, the map, the messages, the title, the pause menu, help) is
//! ordinary intuiTUIve nodes.
//!
//! Keys: W/↑ S/↓ walk, A/← D/→ turn, Q/E (or Alt+←/→, `,` `.`) strafe,
//! capitals run; F (or Ctrl+F) fires; Space opens doors and presses the
//! exit switch; Tab shows the map; Esc pauses; `?` lists the keys.

mod art;
mod game;
mod render;
mod view;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use intuituive::interact::Key;
use intuituive::prelude::*;
use intuituive::{big_text, digits, rate, RectHandle};
use rich_art::image::{DynamicImage, RgbImage};
use rich_art::{ImageArt, ImageMode};

pub use art::Textures;
pub use game::{Action, DoorState, Game, Kind, Mode as EnemyMode, State, Tile, Wall, LEVELS, TIC};
pub use render::{render, Frame, Look};
pub use view::Mode;

/// What the status bar and the overlays show; a tic that changes none of
/// it draws none of them.
#[derive(Clone, PartialEq, Debug)]
pub struct Hud {
    pub state: State,
    pub paused: bool,
    pub health: i32,
    pub armour: i32,
    pub ammo: i32,
    pub kills: usize,
    pub enemies: usize,
    pub items: usize,
    pub pickups: usize,
    pub level: &'static str,
    pub seconds: u64,
    pub message: Option<String>,
    pub hurt: bool,
    pub grin: bool,
    pub look: i32,
}

impl Hud {
    fn of(game: &Game) -> Hud {
        Hud {
            state: game.state,
            paused: game.paused,
            health: game.player.health,
            armour: game.player.armour,
            ammo: game.player.ammo,
            kills: game.kills,
            enemies: game.enemies(),
            items: game.items,
            pickups: game.pickups(),
            level: game.level_name(),
            seconds: game.level_tics / 35,
            message: game.message.as_ref().map(|(m, _)| m.clone()),
            hurt: game.hurt > 0,
            grin: game.bonus > 0,
            look: [0, -1, 0, 1][(game.tic / 52 % 4) as usize],
        }
    }
}

pub struct Options {
    /// The level to start on, from 0.
    pub level: usize,
    pub seed: u64,
    /// `None`: detect.
    pub mode: Option<Mode>,
    pub floor_cast: bool,
    /// Skip the title screen.
    pub start: bool,
    /// Draw as fast as frames allow (the game still runs at 35 tics a
    /// second of the wall clock); else one frame a tic.
    pub uncapped: bool,
    /// Keep the game to the wall clock: a frame that came late runs the
    /// tics it owes (at most five). Off, each timer tick is one tic, which
    /// a test's clock steps exactly (a timer handler is not told the app's
    /// clock, so catching up needs the wall clock).
    pub realtime: bool,
    /// Walk round by itself for this long and quit, counting frames.
    pub bench: Option<Duration>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            level: 0,
            seed: 1,
            mode: Some(Mode::Blocks),
            floor_cast: true,
            start: false,
            uncapped: false,
            realtime: false,
            bench: None,
        }
    }
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options {
            mode: None,
            realtime: true,
            ..Options::default()
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            let mut value = |name: &str| args.next().ok_or(format!("{name} takes a value"));
            match arg.as_str() {
                "--mode" => options.mode = Mode::parse(&value("--mode")?)?,
                "--level" => {
                    let n: usize = value("--level")?.parse().map_err(|_| "bad level")?;
                    if n == 0 || n > LEVELS.len() {
                        return Err(format!("levels are 1 to {}", LEVELS.len()));
                    }
                    options.level = n - 1;
                }
                "--seed" => options.seed = value("--seed")?.parse().map_err(|_| "bad seed")?,
                "--flat" => options.floor_cast = false,
                "--start" => options.start = true,
                "--uncapped" => options.uncapped = true,
                "--bench" => {
                    let secs: f64 = value("--bench")?.parse().map_err(|_| "bad seconds")?;
                    options.bench = Some(Duration::from_secs_f64(secs));
                    options.start = true;
                }
                "-h" | "--help" => {
                    return Err(format!(
                        "raycast [--mode {}] [--level N] [--flat] [--start] [--uncapped] [--bench SECS] [--seed N]",
                        Mode::NAMES
                    ))
                }
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(options)
    }
}

/// What a test (or the benchmark) sees of a running game.
#[derive(Clone)]
pub struct Probe {
    pub game: Rc<RefCell<Game>>,
    /// The last frame drawn.
    pub frame: Rc<RefCell<Rc<Frame>>>,
    /// Frames drawn.
    pub frames: Rc<Cell<u64>>,
    /// When the first was (the benchmark's clock starts there).
    pub first: Rc<Cell<Option<Instant>>>,
    /// The mode the view draws in.
    pub mode: Mode,
}

/// Each key, the actions it holds, and its line in the help (the first
/// key of an action describes it; the others are its alternatives).
const KEYS: &[(&str, &[Action], &str)] = &[
    ("w", &[Action::Forward], "Walk forward (also ↑, W runs)"),
    ("up", &[Action::Forward], ""),
    ("W", &[Action::Forward, Action::Run], ""),
    ("s", &[Action::Back], "Walk back (also ↓, S runs)"),
    ("down", &[Action::Back], ""),
    ("S", &[Action::Back, Action::Run], ""),
    ("a", &[Action::TurnLeft], "Turn left (also ←)"),
    ("left", &[Action::TurnLeft], ""),
    ("A", &[Action::TurnLeft, Action::Run], ""),
    ("d", &[Action::TurnRight], "Turn right (also →)"),
    ("right", &[Action::TurnRight], ""),
    ("D", &[Action::TurnRight, Action::Run], ""),
    (
        "q",
        &[Action::StrafeLeft],
        "Strafe left (also Alt+←, comma)",
    ),
    ("alt+left", &[Action::StrafeLeft], ""),
    ("comma", &[Action::StrafeLeft], ""),
    (
        "e",
        &[Action::StrafeRight],
        "Strafe right (also Alt+→, period)",
    ),
    ("alt+right", &[Action::StrafeRight], ""),
    ("period", &[Action::StrafeRight], ""),
];

/// The game as an app, and what a test sees of it.
pub fn raycast_app(options: Options) -> (App, Probe) {
    let mut game = Game::new(options.level, options.seed);
    if options.start {
        game.state = State::Playing;
    }
    let mode = options.mode.unwrap_or_else(Mode::detect);
    let probe = Probe {
        game: Rc::new(RefCell::new(game)),
        frame: Rc::new(RefCell::new(Rc::new(Frame::blank(1, 1)))),
        frames: Rc::new(Cell::new(0)),
        first: Rc::new(Cell::new(None)),
        mode,
    };
    let placed = mode
        .is_pixels()
        .then(|| Arc::new(Mutex::new(view::Placed::default())));
    let shared = probe.clone();
    let source = placed.clone();
    let app = App::new(move || build(shared, options, placed))
        .help_key("?")
        .palette_key("ctrl+p");
    let app = match source {
        Some(placed) => app.graphics(Arc::new(view::Pixels(placed))),
        None => app,
    };
    (app, probe)
}

/// The keys that have been seen let go: from then on they are held by
/// press and release, not latched.
type Releasing = Rc<RefCell<HashSet<&'static str>>>;

fn build(probe: Probe, options: Options, placed: Option<Arc<Mutex<view::Placed>>>) -> Node {
    let textures = Rc::new(Textures::generate());
    let game = probe.game.clone();
    let hud = signal(Hud::of(&game.borrow()));
    let frame = signal(probe.frame.borrow().clone());
    let drawn = signal(0u64);
    let fps = rate(drawn);
    let show_map = signal(false);
    let map = signal(String::new());
    let size = Rc::new(Cell::new((80u16, 19u16)));
    let releasing: Releasing = Rc::new(RefCell::new(HashSet::new()));
    let covered = memo(move || {
        let (state, paused) = hud.with(|h| (h.state, h.paused));
        show_map.get() || paused || state != State::Playing
    });

    // The loop: tics of the game, then a frame.
    let look = Look {
        floor_cast: options.floor_cast,
        aspect: 1.0,
    };
    let mut started: Option<Instant> = None;
    let mut run = 0u64;
    let interval = if options.uncapped {
        Duration::from_millis(1)
    } else {
        TIC
    };
    let tick_game = game.clone();
    let tick_size = size.clone();
    let tick_probe = probe.clone();
    let bench = options.bench.is_some();
    let mode = probe.mode;
    every(interval, move |_| {
        let mut game = tick_game.borrow_mut();
        let due = if options.uncapped || options.realtime {
            // The clock starts at the first tick (after the terminal is set
            // up), and runs the tics owed since, five at most a frame.
            let since = *started.get_or_insert_with(Instant::now);
            let should = 1 + (since.elapsed().as_nanos() / TIC.as_nanos()) as u64;
            // Further behind than that, the rest is dropped.
            let due = should.saturating_sub(run).min(5);
            run = should;
            due
        } else {
            1
        };
        for _ in 0..due {
            if bench {
                // The benchmark walks round on its own.
                game.press(Action::Forward, false);
                if game.tic % 140 < 40 {
                    game.press(Action::TurnRight, false);
                }
                if game.tic.is_multiple_of(35) {
                    game.use_key();
                }
            }
            game.step();
        }
        if game.state == State::Title {
            // The title's backdrop: the level, turning slowly.
            game.player.angle += 0.004;
        }
        hud.set(Hud::of(&game));
        if show_map.get_untracked() {
            map.set(minimap(&game));
        }
        // Capped, a frame is drawn for each tic (none when none is due);
        // uncapped, one for every turn of the loop, to measure the cost.
        if game.paused || (due == 0 && !options.uncapped && game.state != State::Title) {
            return;
        }
        let (w, h) = tick_size.get();
        let (sx, sy, aspect) = mode.pixels(covered.get_untracked());
        let mut next = render(
            &game,
            &textures,
            usize::from(w) * sx,
            usize::from(h) * sy,
            Look { aspect, ..look },
        );
        if bench && tick_probe.first.get().is_none() {
            tick_probe.first.set(Some(Instant::now()));
        }
        let n = tick_probe.frames.get() + 1;
        next.number = n;
        tick_probe.frames.set(n);
        let next = Rc::new(next);
        *tick_probe.frame.borrow_mut() = next.clone();
        frame.replace(next);
        drawn.replace(n);
    });
    if let Some(after_secs) = options.bench {
        after(after_secs, |cx| cx.quit());
    }

    // The view, and what goes over it.
    let rect = RectHandle::new();
    let release_game = game.clone();
    let release_keys = releasing.clone();
    let view = widget(view::View {
        frame,
        mode,
        covered,
        size,
        rect,
        placed,
        released: Box::new(move |key: &Key| {
            for (name, actions, _) in KEYS {
                let bound = Key::parse(name).expect("the key names parse");
                if key.code == bound.code {
                    release_keys.borrow_mut().insert(name);
                    for action in *actions {
                        release_game.borrow_mut().release(*action);
                    }
                }
            }
        }),
    })
    .autofocus()
    .rect_into(&rect)
    .name("view");
    let message = text(move || {
        hud.with(|h| match &h.message {
            Some(m) => format!("[bold #ffd75f on #101018] {m} [/]"),
            None => String::new(),
        })
    })
    .no_wrap()
    .place(Align::Start, Align::Start);
    let map_panel = switch(
        move || show_map.get(),
        move |on| {
            if on {
                text(move || map.get())
                    .no_wrap()
                    .style("on #101018")
                    .panel("Map")
            } else {
                label("")
            }
        },
    )
    .place(Align::End, Align::Start);
    let screens = switch(
        move || hud.with(|h| h.state),
        move |state| match state {
            State::Playing => label(""),
            State::Title => title_panel(),
            State::LevelDone => end_panel(
                hud,
                "Level complete",
                "[bold #ffd75f]Level complete[/]",
                "Enter: next level",
            ),
            State::Dead => end_panel(hud, "Dead", "[bold red]You died[/]", "Enter: try again"),
            State::Won => end_panel(
                hud,
                "Cleared",
                "[bold green]All levels cleared[/]",
                "Enter: back to the title",
            ),
        },
    )
    .place(Align::Center, Align::Center);
    let stage = layers([view, message, map_panel, screens]);
    let root = column([stage.flex(1), status_bar(hud, fps, mode).fixed(5)]).style("on #101018");
    keys(root, game, releasing, show_map, map)
}

/// The keys: movement held down, the rest pressed.
fn keys(
    root: Node,
    game: Rc<RefCell<Game>>,
    releasing: Releasing,
    show_map: Signal<bool>,
    map: Signal<String>,
) -> Node {
    let mut root = root;
    for (name, actions, help) in KEYS {
        let game = game.clone();
        let releasing = releasing.clone();
        let press = move |_: &mut Ctx| {
            let releases = releasing.borrow().contains(name);
            let mut game = game.borrow_mut();
            if game.state != State::Playing {
                return;
            }
            for action in *actions {
                game.press(*action, releases);
            }
        };
        root = if help.is_empty() {
            root.on_key(name, press)
        } else {
            root.bind_in("Move", name, help, press)
        };
    }
    let act = |f: fn(&mut Game)| {
        let game = game.clone();
        move |_: &mut Ctx| f(&mut game.borrow_mut())
    };
    let fire = act(Game::fire);
    let pause_game = game.clone();
    let map_game = game.clone();
    let level_game = game.clone();
    root.bind_in("Act", "f", "Fire (also Ctrl+F)", act(Game::fire))
        .on_key("ctrl+f", fire)
        .bind_in(
            "Act",
            "space",
            "Open a door, press the exit switch",
            act(Game::use_key),
        )
        .bind_in(
            "Act",
            "enter",
            "Start, next level, try again",
            act(Game::enter),
        )
        .bind_in("Game", "tab", "Show or hide the map", move |_| {
            show_map.update(|on| *on = !*on);
            if show_map.get_untracked() {
                map.set(minimap(&map_game.borrow()));
            }
        })
        .bind_in("Game", "1", "On the title: start on level 1", move |_| {
            let mut game = level_game.borrow_mut();
            if game.state == State::Title {
                game.load(0);
                game.state = State::Playing;
            }
        })
        .bind_in("Game", "2", "On the title: start on level 2", {
            let game = game.clone();
            move |_| {
                let mut game = game.borrow_mut();
                if game.state == State::Title {
                    game.load(1);
                    game.state = State::Playing;
                }
            }
        })
        .bind_in("Game", "esc", "Pause menu", move |cx| {
            {
                let mut game = pause_game.borrow_mut();
                if game.state != State::Playing {
                    return;
                }
                game.paused = true;
                game.release_all();
            }
            let game = pause_game.clone();
            cx.modal(Size::Fixed(34), Size::Auto, move || pause_menu(game));
        })
        .bind_in("Game", "ctrl+q", "Quit", |cx| cx.quit())
}

/// The pause menu, over the stopped game.
fn pause_menu(game: Rc<RefCell<Game>>) -> Node {
    let resume = {
        let game = game.clone();
        move |cx: &mut Ctx| {
            game.borrow_mut().paused = false;
            cx.pop();
        }
    };
    let restart = {
        let game = game.clone();
        move |cx: &mut Ctx| {
            let mut game = game.borrow_mut();
            game.restart();
            game.paused = false;
            cx.pop();
        }
    };
    label("[b]Esc[/]  resume\n[b]r[/]    restart the level\n[b]?[/]    keys\n[b]q[/]    quit")
        .padding(1, 2)
        .style("on #181828")
        .panel("Paused")
        .bind("esc enter", "Resume", resume)
        .bind("r", "Restart the level", restart)
        .bind("q", "Quit", |cx| cx.quit())
        .focusable()
        .autofocus()
}

/// The title, over the first level turning slowly behind it.
fn title_panel() -> Node {
    column([
        big_text(|| "[bold #e0603c]RAYCASTER[/]".to_string()).fixed(6),
        label("[dim]rs-rich raycaster · a first-person maze on intuiTUIve[/]").fixed(2),
        label(
            "[b]Enter[/] start   [b]1[/] [b]2[/] pick a level   [b]?[/] keys   [b]Ctrl+Q[/] quit",
        )
        .fixed(1),
    ])
    .padding(0, 2)
    .style("on #101018")
    .panel("rs-rich raycaster")
}

/// The box at the end of a level, or of the player.
fn end_panel(
    hud: Signal<Hud>,
    title: &'static str,
    heading: &'static str,
    next: &'static str,
) -> Node {
    text(move || {
        hud.with(|h| {
            format!(
                "{heading}\n\n{}\nKills  {}/{}\nItems  {}/{}\nTime   {}:{:02}\n\n[b]{next}[/]",
                h.level,
                h.kills,
                h.enemies,
                h.items,
                h.pickups,
                h.seconds / 60,
                h.seconds % 60,
            )
        })
    })
    .padding(1, 3)
    .style("on #181828")
    .panel(title)
}

/// The status bar: ammunition, health, the face, armour, and the rest.
fn status_bar(hud: Signal<Hud>, fps: Signal<u32>, mode: Mode) -> Node {
    let face = renderable(move || {
        let (health, hurt, grin, look) = hud.with(|h| (h.health, h.hurt, h.grin, h.look));
        let (w, h, px) = art::face(health, hurt, grin, look);
        let image = RgbImage::from_raw(w as u32, h as u32, px).expect("the face's size");
        ImageArt::new(DynamicImage::ImageRgb8(image))
            .mode(ImageMode::Blocks)
            .width(w)
            .height(h / 2)
    })
    .label("Face");
    let info = text(move || {
        hud.with(|h| {
            format!(
                "[b]{}[/]  {}:{:02}\nKills {}/{}  Items {}/{}\n[dim]{:>3} fps · {}[/]",
                h.level,
                h.seconds / 60,
                h.seconds % 60,
                h.kills,
                h.enemies,
                h.items,
                h.pickups,
                fps.get(),
                mode.name(),
            )
        })
    })
    .no_wrap()
    .padding(0, 1)
    .panel("rs-rich raycaster");
    let number = |f: fn(&Hud) -> String, colour: &'static str| {
        digits(move || format!("[{colour}]{}[/]", hud.with(f)))
    };
    row([
        number(|h| h.ammo.to_string(), "#ffd75f")
            .panel("Ammo")
            .fixed(11),
        number(|h| format!("{}%", h.health.max(0)), "#ff5f5f")
            .panel("Health")
            .fixed(14),
        face.padding(0, 0).panel("").fixed(12),
        number(|h| format!("{}%", h.armour), "#5fd75f")
            .panel("Armour")
            .fixed(14),
        info.flex(1),
    ])
}

/// The map as markup: a cell a character, the player an arrow.
pub fn minimap(game: &Game) -> String {
    let p = &game.player;
    let arrows = ['→', '↘', '↓', '↙', '←', '↖', '↑', '↗'];
    let octant = ((p.angle / std::f64::consts::FRAC_PI_4).round() as i64).rem_euclid(8) as usize;
    let mut out = String::new();
    for y in 0..game.height {
        if y > 0 {
            out.push('\n');
        }
        for x in 0..game.width {
            let thing = game.things.iter().find(|t| {
                !t.gone && t.x as usize == x && t.y as usize == y && t.kind != Kind::Lamp
            });
            let cell = if p.x as usize == x && p.y as usize == y {
                format!("[bold #5fff5f]{}[/]", arrows[octant])
            } else if let Some(t) = thing {
                match t.kind {
                    Kind::Enemy if t.alive() => "[bold #ff5f5f]e[/]".into(),
                    Kind::Enemy => "[#802020]x[/]".into(),
                    k if k.is_pickup() => "[#5fd7ff]+[/]".into(),
                    _ => "[#a0a0a0]o[/]".into(),
                }
            } else {
                match game.map[y][x] {
                    Tile::Floor => " ".into(),
                    Tile::Wall(wall) => format!("[{}]█[/]", wall.map_colour()),
                    Tile::Door(i) => match game.doors[i].state {
                        DoorState::Closed => "[#c8b040]▒[/]".into(),
                        _ => "[#c8b040]░[/]".into(),
                    },
                }
            };
            out.push_str(&cell);
        }
    }
    out
}

fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let bench = options.bench;
    let (app, probe) = raycast_app(options);
    app.run()?;
    if let Some(first) = probe.first.get().filter(|_| bench.is_some()) {
        let seconds = first.elapsed().as_secs_f64();
        let frames = probe.frames.get();
        let (w, h) = (probe.frame.borrow().width, probe.frame.borrow().height);
        println!(
            "raycast bench: {frames} frames in {seconds:.2} s = {:.1} fps ({} mode, {w}x{h} pixels)",
            frames as f64 / seconds,
            probe.mode.name(),
        );
    }
    Ok(())
}

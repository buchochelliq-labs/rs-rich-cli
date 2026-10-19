//! The game: the levels, the player, doors, things and enemies, stepped
//! one tic (1/35 s, as the classic shooters count time) at a time. No
//! framework in here, so a test drives it directly.
//!
//! The ray caster's walk through the grid ([`cast`]) is Lode Vandevenne's
//! DDA (<https://lodev.org/cgtutor/raycasting.html>), with Wolfenstein-style
//! doors: a door is a thin plane across the middle of its cell that slides
//! into the wall beside it.

use std::time::Duration;

/// One tic: 35 a second.
pub const TIC: Duration = Duration::from_micros(28_571);

/// Walking speed in cells a tic, and running's.
const WALK: f64 = 0.085;
const RUN: f64 = 0.15;
/// Turning speed in radians a tic, and running's.
const TURN: f64 = 0.065;
const RUN_TURN: f64 = 0.1;
/// The player's radius, for collisions.
const RADIUS: f64 = 0.24;
/// The camera plane's half-width for a 1:1 view: a 66° field of view.
pub const PLANE: f64 = 0.66;

/// How long a press without a release (a plain letter, or a terminal
/// without the kitty keyboard protocol) counts as held: the first press,
/// then each autorepeat after it.
pub const FIRST_LATCH: u64 = 8;
pub const REPEAT_LATCH: u64 = 5;

/// A wall's material.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Wall {
    Brick,
    Stone,
    Wood,
    Tech,
    /// The exit switch: `Space` in front of it ends the level.
    Exit,
    /// A door's face.
    Door,
    /// The jamb beside a door (drawn on the walls a door sits between).
    Frame,
}

impl Wall {
    fn from_char(c: char) -> Option<Wall> {
        Some(match c {
            '#' => Wall::Brick,
            'S' => Wall::Stone,
            'W' => Wall::Wood,
            'T' => Wall::Tech,
            'X' => Wall::Exit,
            _ => return None,
        })
    }

    /// The minimap's glyph colour.
    pub fn map_colour(self) -> &'static str {
        match self {
            Wall::Brick => "#a0503c",
            Wall::Stone => "#8c8c87",
            Wall::Wood => "#8a5a32",
            Wall::Tech => "#5a6e8c",
            Wall::Exit => "#e04040",
            Wall::Door | Wall::Frame => "#c8b040",
        }
    }
}

/// A cell of the map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Floor,
    Wall(Wall),
    /// A door, by its index in [`Game::doors`].
    Door(usize),
}

/// A hand-made level: its name and its grid.
///
/// `#` brick, `S` stone, `W` wood, `T` tech panels, `X` the exit switch,
/// `D` a door, `.` floor; `>` `<` `^` `v` the player's start and facing;
/// `B` a barrel, `C` a column, `L` a hanging lamp, `E` an enemy, `a`
/// ammunition, `h` a medikit, `r` armour.
pub struct Level {
    pub name: &'static str,
    pub rows: &'static [&'static str],
}

pub const LEVELS: [Level; 2] = [
    Level {
        name: "E1 Hangar",
        rows: &[
            "SSSSSSSSSSSSSSSSSSSSSSSS",
            "S..C...#.......W.......S",
            "S......#...E...W...a...S",
            "S>.....D.......D...E...S",
            "S......#...B...W.......S",
            "S..h...#...L...W...C...S",
            "S#####D########WWWWDWWWS",
            "S......#.......T.......S",
            "S..L...#...E...T...h...S",
            "S......D.......D...E...S",
            "S..a...#...C...T.......S",
            "S..r...#..a....T......XS",
            "SSSSSSSSSSSSSSSSSSSSSSSS",
        ],
    },
    Level {
        name: "E2 Depot",
        rows: &[
            "TTTTTTTTTTTTTTTTTTTTTTTT",
            "Tv.....T.....a.....T...T",
            "T......T..C.....C..T.a.T",
            "T..B...D...........D...T",
            "T......T....E..L...T.E.T",
            "TTTDTTTT...........TTDTT",
            "W......W..C.....C..W...W",
            "W..E...W.....B.....W...W",
            "W..L...D....h......D.r.W",
            "W..a...W...........W.E.W",
            "WWWWWWWWWWWWDWWWWWWWWDWW",
            "S..........S.......S...S",
            "S..C...E...S...E...D...S",
            "S..........D.......S..XS",
            "S..h...B...S..a....S...S",
            "SSSSSSSSSSSSSSSSSSSSSSSS",
        ],
    },
];

/// What a thing is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Kind {
    Barrel,
    Column,
    Lamp,
    Ammo,
    Health,
    Armour,
    Enemy,
}

impl Kind {
    fn from_char(c: char) -> Option<Kind> {
        Some(match c {
            'B' => Kind::Barrel,
            'C' => Kind::Column,
            'L' => Kind::Lamp,
            'a' => Kind::Ammo,
            'h' => Kind::Health,
            'r' => Kind::Armour,
            'E' => Kind::Enemy,
            _ => return None,
        })
    }

    /// Whether the player and enemies walk round it.
    pub fn blocks(self) -> bool {
        matches!(self, Kind::Barrel | Kind::Column | Kind::Enemy)
    }

    pub fn is_pickup(self) -> bool {
        matches!(self, Kind::Ammo | Kind::Health | Kind::Armour)
    }
}

/// What an enemy is doing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Waiting: it has not seen the player.
    Idle,
    /// Walking at the player.
    Chase,
    /// Firing: the frames left of the shot.
    Attack(u32),
    /// Hit: the frames left of the flinch.
    Pain(u32),
    /// Dead, for this many tics (the fall's frames).
    Dead(u32),
}

/// A thing on the map: scenery, a pickup or an enemy.
#[derive(Clone, Debug)]
pub struct Thing {
    pub kind: Kind,
    pub x: f64,
    pub y: f64,
    /// A pickup taken.
    pub gone: bool,
    pub health: i32,
    pub mode: Mode,
    /// Tics before an enemy may fire again.
    pub cooldown: u32,
    /// Tics walked, for the walking frames.
    pub steps: u32,
}

impl Thing {
    pub fn alive(&self) -> bool {
        self.kind == Kind::Enemy && !matches!(self.mode, Mode::Dead(_))
    }

    /// Whether it stops a walker now.
    fn solid(&self) -> bool {
        match self.kind {
            Kind::Enemy => self.alive(),
            kind => kind.blocks(),
        }
    }
}

/// A door's state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoorState {
    Closed,
    Opening,
    /// Open, closing again after this many tics.
    Open(u32),
    Closing,
}

/// A door: its cell, which way its plane runs, and how far it is open.
#[derive(Clone, Debug)]
pub struct Door {
    pub x: usize,
    pub y: usize,
    /// The plane is `x = x + 0.5` (the passage runs east-west), else
    /// `y = y + 0.5`.
    pub across_x: bool,
    /// 0 closed, 1 open.
    pub open: f64,
    pub state: DoorState,
}

/// Tics a door takes to open, and stays open.
const DOOR_TICS: f64 = 30.0;
const DOOR_WAIT: u32 = 140;

/// What the player can hold down.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Action {
    Forward,
    Back,
    TurnLeft,
    TurnRight,
    StrafeLeft,
    StrafeRight,
    Run,
}

const ACTIONS: usize = 7;

/// Keys held down: by press and release where the terminal reports
/// releases, else latched for a few tics after each press.
#[derive(Clone, Debug, Default)]
pub struct Held {
    down: [bool; ACTIONS],
    until: [u64; ACTIONS],
}

impl Held {
    pub fn is_held(&self, action: Action, tic: u64) -> bool {
        let i = action as usize;
        self.down[i] || self.until[i] > tic
    }
}

/// The game's screens.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    Title,
    Playing,
    LevelDone,
    Dead,
    Won,
}

/// A seeded generator (SplitMix64), so a test sees the same fight.
#[derive(Clone, Debug)]
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `lo..=hi`.
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        lo + (self.next() % u64::from(hi - lo + 1)) as u32
    }
}

/// The player.
#[derive(Clone, Debug)]
pub struct Player {
    pub x: f64,
    pub y: f64,
    /// Facing, in radians: 0 east, π/2 south (the map's y grows down).
    pub angle: f64,
    pub health: i32,
    pub armour: i32,
    pub ammo: i32,
}

impl Player {
    pub fn dir(&self) -> (f64, f64) {
        (self.angle.cos(), self.angle.sin())
    }

    /// The camera plane, to the right of the direction.
    pub fn plane(&self, half_width: f64) -> (f64, f64) {
        (
            -self.angle.sin() * half_width,
            self.angle.cos() * half_width,
        )
    }
}

/// Where a ray stopped.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// The distance along the view direction (no fish-eye).
    pub dist: f64,
    /// It hit a face running north-south (`false`) or east-west (`true`).
    pub side: bool,
    pub wall: Wall,
    /// Where along the face, 0 to 1, left to right as seen.
    pub u: f64,
    pub cell: (usize, usize),
}

/// The whole game.
#[derive(Clone, Debug)]
pub struct Game {
    pub level: usize,
    pub map: Vec<Vec<Tile>>,
    pub width: usize,
    pub height: usize,
    pub doors: Vec<Door>,
    pub things: Vec<Thing>,
    pub player: Player,
    pub state: State,
    pub paused: bool,
    pub held: Held,
    pub tic: u64,
    /// Tics before the pistol may fire again.
    pub cooldown: u32,
    /// Tics left of the muzzle flash.
    pub flash: u32,
    /// Tics left of the red (hurt) and gold (pickup) tints.
    pub hurt: u32,
    pub bonus: u32,
    /// The message at the top left, and its tics left.
    pub message: Option<(String, u32)>,
    pub kills: usize,
    pub items: usize,
    /// Distance walked, for the weapon's bob.
    pub walked: f64,
    /// Tics the level has run.
    pub level_tics: u64,
    rng: Rng,
}

impl Game {
    /// Level `level` (0-based), the player fresh.
    pub fn new(level: usize, seed: u64) -> Game {
        let player = Player {
            x: 1.5,
            y: 1.5,
            angle: 0.0,
            health: 100,
            armour: 0,
            ammo: 50,
        };
        let mut game = Game {
            level: 0,
            map: Vec::new(),
            width: 0,
            height: 0,
            doors: Vec::new(),
            things: Vec::new(),
            player,
            state: State::Title,
            paused: false,
            held: Held::default(),
            tic: 0,
            cooldown: 0,
            flash: 0,
            hurt: 0,
            bonus: 0,
            message: None,
            kills: 0,
            items: 0,
            walked: 0.0,
            level_tics: 0,
            rng: Rng(seed),
        };
        game.load(level);
        game
    }

    /// Load level `level`, keeping the player's health, armour and ammo.
    pub fn load(&mut self, level: usize) {
        let level = level.min(LEVELS.len() - 1);
        let rows = LEVELS[level].rows;
        self.level = level;
        self.height = rows.len();
        self.width = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        self.map = vec![vec![Tile::Floor; self.width]; self.height];
        self.doors.clear();
        self.things.clear();
        for (y, row) in rows.iter().enumerate() {
            for (x, c) in row.chars().enumerate() {
                let (cx, cy) = (x as f64 + 0.5, y as f64 + 0.5);
                if let Some(wall) = Wall::from_char(c) {
                    self.map[y][x] = Tile::Wall(wall);
                } else if c == 'D' {
                    self.map[y][x] = Tile::Door(self.doors.len());
                    let wall_at = |dx: i32, dy: i32| {
                        let row = rows[(y as i32 + dy) as usize].as_bytes();
                        Wall::from_char(row[(x as i32 + dx) as usize] as char).is_some()
                    };
                    self.doors.push(Door {
                        x,
                        y,
                        across_x: wall_at(0, -1) && wall_at(0, 1),
                        open: 0.0,
                        state: DoorState::Closed,
                    });
                } else if let Some(kind) = Kind::from_char(c) {
                    self.things.push(Thing {
                        kind,
                        x: cx,
                        y: cy,
                        gone: false,
                        health: 30,
                        mode: Mode::Idle,
                        cooldown: 0,
                        steps: 0,
                    });
                } else if let Some(angle) = match c {
                    '>' => Some(0.0),
                    'v' => Some(std::f64::consts::FRAC_PI_2),
                    '<' => Some(std::f64::consts::PI),
                    '^' => Some(-std::f64::consts::FRAC_PI_2),
                    _ => None,
                } {
                    self.player.x = cx;
                    self.player.y = cy;
                    self.player.angle = angle;
                }
            }
        }
        self.kills = 0;
        self.items = 0;
        self.level_tics = 0;
        self.held = Held::default();
        self.message = Some((LEVELS[level].name.to_string(), 105));
    }

    pub fn level_name(&self) -> &'static str {
        LEVELS[self.level].name
    }

    pub fn enemies(&self) -> usize {
        self.things.iter().filter(|t| t.kind == Kind::Enemy).count()
    }

    pub fn pickups(&self) -> usize {
        self.things.iter().filter(|t| t.kind.is_pickup()).count()
    }

    pub fn tile(&self, x: i64, y: i64) -> Tile {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return Tile::Wall(Wall::Stone);
        }
        self.map[y as usize][x as usize]
    }

    /// Enter: start from the title, go on after a level, try again after
    /// dying, back to the title after the last level.
    pub fn enter(&mut self) {
        match self.state {
            State::Title => {
                // The title turned the camera round: start where the level
                // says.
                self.load(self.level);
                self.state = State::Playing;
            }
            State::LevelDone => {
                self.load(self.level + 1);
                self.state = State::Playing;
            }
            State::Dead => self.restart(),
            State::Won => {
                let seed = self.rng.next();
                *self = Game::new(0, seed);
            }
            State::Playing => {}
        }
    }

    /// The level again, from a fresh player.
    pub fn restart(&mut self) {
        self.player.health = 100;
        self.player.armour = 0;
        self.player.ammo = 50;
        self.hurt = 0;
        self.bonus = 0;
        self.flash = 0;
        self.load(self.level);
        self.state = State::Playing;
    }

    fn live(&self) -> bool {
        self.state == State::Playing && !self.paused
    }

    /// A movement key went down: held until its release when `releases`
    /// (the terminal reports them for this key), else for a few tics.
    pub fn press(&mut self, action: Action, releases: bool) {
        let i = action as usize;
        if releases {
            self.held.down[i] = true;
        } else {
            let latch = if self.held.until[i] > self.tic {
                REPEAT_LATCH
            } else {
                FIRST_LATCH
            };
            self.held.until[i] = self.held.until[i].max(self.tic + latch + 1);
        }
    }

    /// A movement key came up.
    pub fn release(&mut self, action: Action) {
        let i = action as usize;
        self.held.down[i] = false;
        self.held.until[i] = 0;
    }

    /// Let go of everything (pausing, a screen over the game).
    pub fn release_all(&mut self) {
        self.held = Held::default();
    }

    /// Fire the pistol: hitscan along the centre of the view.
    pub fn fire(&mut self) {
        if !self.live() || self.cooldown > 0 {
            return;
        }
        if self.player.ammo <= 0 {
            self.say("Out of ammo");
            return;
        }
        self.player.ammo -= 1;
        self.cooldown = 12;
        self.flash = 5;
        let (dx, dy) = self.player.dir();
        let wall = self
            .cast(self.player.x, self.player.y, dx, dy)
            .map_or(f64::MAX, |h| h.dist);
        // The nearest live enemy whose body crosses the line of fire.
        let target = self
            .things
            .iter()
            .enumerate()
            .filter(|(_, t)| t.alive())
            .filter_map(|(i, t)| {
                let (vx, vy) = (t.x - self.player.x, t.y - self.player.y);
                let ahead = vx * dx + vy * dy;
                let aside = (vx * dy - vy * dx).abs();
                (ahead > 0.0 && ahead < wall && aside < 0.3).then_some((i, ahead))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = target {
            let damage = self.rng.range(9, 16) as i32;
            let enemy = &mut self.things[i];
            enemy.health -= damage;
            if enemy.health <= 0 {
                enemy.mode = Mode::Dead(0);
                self.kills += 1;
            } else {
                enemy.mode = Mode::Pain(6);
            }
        }
    }

    /// Space: open the door in front, or press the exit switch.
    pub fn use_key(&mut self) {
        if !self.live() {
            return;
        }
        let (dx, dy) = self.player.dir();
        let mut target = None;
        for step in [0.5, 1.0, 1.5] {
            let (x, y) = (self.player.x + dx * step, self.player.y + dy * step);
            match self.tile(x.floor() as i64, y.floor() as i64) {
                Tile::Floor => {}
                tile => {
                    target = Some(tile);
                    break;
                }
            }
        }
        // Not straight ahead: the nearest door or switch within reach and
        // half a right angle of the view, so a keyboard player who stopped
        // a little off need not line up exactly.
        if !matches!(target, Some(Tile::Door(_) | Tile::Wall(Wall::Exit))) {
            let (px, py) = (self.player.x, self.player.y);
            let mut best = f64::MAX;
            for y in (py as i64 - 2)..=(py as i64 + 2) {
                for x in (px as i64 - 2)..=(px as i64 + 2) {
                    let tile = self.tile(x, y);
                    if !matches!(tile, Tile::Door(_) | Tile::Wall(Wall::Exit)) {
                        continue;
                    }
                    let (vx, vy) = (x as f64 + 0.5 - px, y as f64 + 0.5 - py);
                    let dist = vx.hypot(vy);
                    let cos = (vx * dx + vy * dy) / dist;
                    if dist < 1.6 && cos > std::f64::consts::FRAC_PI_4.cos() && dist < best {
                        best = dist;
                        target = Some(tile);
                    }
                }
            }
        }
        match target {
            Some(Tile::Door(i)) => {
                let door = &mut self.doors[i];
                door.state = match door.state {
                    DoorState::Closed | DoorState::Closing => DoorState::Opening,
                    DoorState::Open(_) | DoorState::Opening => DoorState::Closing,
                };
            }
            Some(Tile::Wall(Wall::Exit)) => {
                self.state = if self.level + 1 == LEVELS.len() {
                    State::Won
                } else {
                    State::LevelDone
                };
                self.release_all();
            }
            _ => {}
        }
    }

    fn say(&mut self, text: &str) {
        self.message = Some((text.to_string(), 70));
    }

    /// One tic of the game.
    pub fn step(&mut self) {
        self.tic += 1;
        if !self.live() {
            return;
        }
        self.level_tics += 1;
        self.cooldown = self.cooldown.saturating_sub(1);
        self.flash = self.flash.saturating_sub(1);
        self.hurt = self.hurt.saturating_sub(1);
        self.bonus = self.bonus.saturating_sub(1);
        if let Some((_, left)) = &mut self.message {
            *left = left.saturating_sub(1);
            if *left == 0 {
                self.message = None;
            }
        }
        self.walk();
        self.pick_up();
        self.doors();
        self.enemies_think();
        if self.player.health <= 0 {
            self.player.health = 0;
            self.state = State::Dead;
            self.release_all();
        }
    }

    fn walk(&mut self) {
        let tic = self.tic;
        let held = |a| self.held.is_held(a, tic);
        let run = held(Action::Run);
        let (speed, turn) = if run { (RUN, RUN_TURN) } else { (WALK, TURN) };
        let mut angle = self.player.angle;
        if held(Action::TurnLeft) {
            angle -= turn;
        }
        if held(Action::TurnRight) {
            angle += turn;
        }
        let forward = f64::from(i8::from(held(Action::Forward)) - i8::from(held(Action::Back)));
        let strafe =
            f64::from(i8::from(held(Action::StrafeRight)) - i8::from(held(Action::StrafeLeft)));
        self.player.angle = angle.rem_euclid(std::f64::consts::TAU);
        let (dx, dy) = self.player.dir();
        let (mx, my) = (
            (dx * forward - dy * strafe) * speed,
            (dy * forward + dx * strafe) * speed,
        );
        if mx == 0.0 && my == 0.0 {
            return;
        }
        let (x, y) = (self.player.x, self.player.y);
        if !self.blocked(x + mx, y, None) {
            self.player.x += mx;
        }
        if !self.blocked(self.player.x, y + my, None) {
            self.player.y += my;
        }
        self.walked += (self.player.x - x).hypot(self.player.y - y);
    }

    /// Whether the player could stand at `(x, y)`.
    pub fn blocks_at(&self, x: f64, y: f64) -> bool {
        self.blocked(x, y, None)
    }

    /// Whether a walker of [`RADIUS`] at `(x, y)` hits a wall, a closed
    /// door or a solid thing (other than thing `me`).
    fn blocked(&self, x: f64, y: f64, me: Option<usize>) -> bool {
        for (cx, cy) in [
            (x - RADIUS, y - RADIUS),
            (x + RADIUS, y - RADIUS),
            (x - RADIUS, y + RADIUS),
            (x + RADIUS, y + RADIUS),
        ] {
            match self.tile(cx.floor() as i64, cy.floor() as i64) {
                Tile::Floor => {}
                Tile::Door(i) if self.doors[i].open > 0.85 => {}
                _ => return true,
            }
        }
        let player_check = me.is_some();
        if player_check && (self.player.x - x).hypot(self.player.y - y) < RADIUS * 2.5 {
            return true;
        }
        self.things
            .iter()
            .enumerate()
            .any(|(i, t)| Some(i) != me && t.solid() && (t.x - x).hypot(t.y - y) < RADIUS + 0.3)
    }

    fn pick_up(&mut self) {
        let (px, py) = (self.player.x, self.player.y);
        let mut said = None;
        for thing in &mut self.things {
            if thing.gone || !thing.kind.is_pickup() || (thing.x - px).hypot(thing.y - py) > 0.55 {
                continue;
            }
            let p = &mut self.player;
            let took = match thing.kind {
                Kind::Ammo if p.ammo < 200 => {
                    p.ammo = (p.ammo + 10).min(200);
                    "Picked up a clip"
                }
                Kind::Health if p.health < 100 => {
                    p.health = (p.health + 25).min(100);
                    "Picked up a medikit"
                }
                Kind::Armour if p.armour < 100 => {
                    p.armour = 100;
                    "Picked up the armour"
                }
                _ => continue,
            };
            thing.gone = true;
            said = Some(took);
        }
        if let Some(text) = said {
            self.items += 1;
            self.bonus = 6;
            self.say(text);
        }
    }

    fn doors(&mut self) {
        let (px, py) = (self.player.x, self.player.y);
        let occupied: Vec<(usize, usize)> = std::iter::once((px, py))
            .chain(self.things.iter().filter(|t| t.alive()).map(|t| (t.x, t.y)))
            .map(|(x, y)| (x as usize, y as usize))
            .collect();
        for door in &mut self.doors {
            door.state = match door.state {
                DoorState::Opening => {
                    door.open = (door.open + 1.0 / DOOR_TICS).min(1.0);
                    if door.open >= 1.0 {
                        DoorState::Open(DOOR_WAIT)
                    } else {
                        DoorState::Opening
                    }
                }
                DoorState::Open(0) if occupied.contains(&(door.x, door.y)) => DoorState::Open(35),
                DoorState::Open(0) => DoorState::Closing,
                DoorState::Open(left) => DoorState::Open(left - 1),
                DoorState::Closing if occupied.contains(&(door.x, door.y)) => DoorState::Opening,
                DoorState::Closing => {
                    door.open = (door.open - 1.0 / DOOR_TICS).max(0.0);
                    if door.open <= 0.0 {
                        DoorState::Closed
                    } else {
                        DoorState::Closing
                    }
                }
                DoorState::Closed => DoorState::Closed,
            };
        }
    }

    /// Whether `(x, y)` sees the player: no wall or closed door between.
    fn sees_player(&self, x: f64, y: f64) -> bool {
        let (vx, vy) = (self.player.x - x, self.player.y - y);
        let dist = vx.hypot(vy);
        if dist > 14.0 {
            return false;
        }
        let (dx, dy) = (vx / dist, vy / dist);
        self.cast(x, y, dx, dy).is_none_or(|hit| {
            // `cast` measures along the ray here (direction of length 1).
            hit.dist > dist
        })
    }

    fn enemies_think(&mut self) {
        let (px, py) = (self.player.x, self.player.y);
        for i in 0..self.things.len() {
            let t = &self.things[i];
            if t.kind != Kind::Enemy {
                continue;
            }
            let (x, y) = (t.x, t.y);
            let mode = t.mode;
            let dist = (px - x).hypot(py - y);
            let sees = !matches!(mode, Mode::Dead(_)) && self.sees_player(x, y);
            let t = &mut self.things[i];
            t.cooldown = t.cooldown.saturating_sub(1);
            let first_sight = mode == Mode::Idle && sees;
            t.mode = match mode {
                Mode::Dead(n) => Mode::Dead(n.saturating_add(1)),
                Mode::Pain(0) => Mode::Chase,
                Mode::Pain(n) => Mode::Pain(n - 1),
                Mode::Attack(0) => Mode::Chase,
                Mode::Attack(n) => Mode::Attack(n - 1),
                Mode::Idle if sees => Mode::Chase,
                Mode::Idle => Mode::Idle,
                Mode::Chase => Mode::Chase,
            };
            if first_sight {
                // A moment to react before the first shot.
                t.cooldown = 35 + (self.rng.next() % 35) as u32;
            }
            if t.mode != Mode::Chase {
                continue;
            }
            if sees && dist < 9.0 && t.cooldown == 0 {
                // Fire: likelier to hit up close.
                t.mode = Mode::Attack(8);
                t.cooldown = 45 + (self.rng.next() % 40) as u32;
                let hit = self.rng.next() % 100 < (90.0 - dist * 8.0).max(20.0) as u64;
                if hit {
                    let damage = self.rng.range(4, 12) as i32;
                    self.damage(damage);
                }
                continue;
            }
            if dist > 1.3 {
                let step = 0.045;
                let (mx, my) = ((px - x) / dist * step, (py - y) / dist * step);
                if !self.blocked(x + mx, y, Some(i)) {
                    self.things[i].x += mx;
                }
                let x = self.things[i].x;
                if !self.blocked(x, y + my, Some(i)) {
                    self.things[i].y += my;
                }
                self.things[i].steps += 1;
            }
        }
    }

    /// The player takes `damage`, armour soaking up a third.
    pub fn damage(&mut self, damage: i32) {
        let p = &mut self.player;
        let soaked = if p.armour > 0 {
            (damage / 3).min(p.armour)
        } else {
            0
        };
        p.armour -= soaked;
        p.health -= damage - soaked;
        self.hurt = 8;
    }

    /// Walk the grid from `(ox, oy)` along `(rx, ry)` to the first wall or
    /// door (Lode Vandevenne's DDA). `dist` is in units of the ray's
    /// length: along the view direction for a camera ray (`dir + plane *
    /// x`), so the walls do not bulge.
    pub fn cast(&self, ox: f64, oy: f64, rx: f64, ry: f64) -> Option<Hit> {
        let (mut mx, mut my) = (ox.floor() as i64, oy.floor() as i64);
        let ddx = if rx == 0.0 {
            f64::MAX
        } else {
            (1.0 / rx).abs()
        };
        let ddy = if ry == 0.0 {
            f64::MAX
        } else {
            (1.0 / ry).abs()
        };
        let (sx, mut side_x) = if rx < 0.0 {
            (-1, (ox - mx as f64) * ddx)
        } else {
            (1, (mx as f64 + 1.0 - ox) * ddx)
        };
        let (sy, mut side_y) = if ry < 0.0 {
            (-1, (oy - my as f64) * ddy)
        } else {
            (1, (my as f64 + 1.0 - oy) * ddy)
        };
        // A wall seen from a doorway is the door's frame.
        let mut from_door = matches!(self.tile(mx, my), Tile::Door(_));
        for _ in 0..128 {
            let side = if side_x < side_y {
                side_x += ddx;
                mx += sx;
                false
            } else {
                side_y += ddy;
                my += sy;
                true
            };
            match self.tile(mx, my) {
                Tile::Floor => from_door = false,
                Tile::Wall(wall) => {
                    let dist = if side { side_y - ddy } else { side_x - ddx };
                    let u = face_u(ox, oy, rx, ry, dist, side);
                    let wall = if from_door { Wall::Frame } else { wall };
                    return Some(Hit {
                        dist,
                        side,
                        wall,
                        u,
                        cell: (mx as usize, my as usize),
                    });
                }
                Tile::Door(i) => {
                    from_door = true;
                    let door = &self.doors[i];
                    // The door's plane is half a step further in.
                    if side == door.across_x {
                        continue;
                    }
                    let dist = if side {
                        side_y - ddy * 0.5
                    } else {
                        side_x - ddx * 0.5
                    };
                    let (hx, hy) = (ox + rx * dist, oy + ry * dist);
                    let inside = if side {
                        hx.floor() as i64 == mx
                    } else {
                        hy.floor() as i64 == my
                    };
                    if !inside {
                        continue;
                    }
                    // The gap is on the same side of the door from either
                    // side, so the door's own coordinate, not mirrored.
                    let along = if side { hx } else { hy };
                    let u = along - along.floor();
                    if u < door.open {
                        continue;
                    }
                    return Some(Hit {
                        dist,
                        side,
                        wall: Wall::Door,
                        u: u - door.open,
                        cell: (mx as usize, my as usize),
                    });
                }
            }
        }
        None
    }
}

/// Where along a face the hit is, 0 to 1, mirrored so a texture reads
/// left to right from whichever side it is seen.
fn face_u(ox: f64, oy: f64, rx: f64, ry: f64, dist: f64, side: bool) -> f64 {
    let along = if side { ox + rx * dist } else { oy + ry * dist };
    let u = along - along.floor();
    if (!side && rx < 0.0) || (side && ry > 0.0) {
        1.0 - u
    } else {
        u
    }
}

//! A rebuild of rata-tetris (<https://github.com/nw-wood/rata-tetris>), a
//! falling-block game on ratatui, on intuiTUIve. It reuses none of its
//! code: it rebuilds the behaviour and the look with intuiTUIve's API.
//!
//! rata-tetris is by Nathanyel Wood, MIT licence. The design, the layout,
//! the keys, the level palettes and the block lettering are theirs; the
//! credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example blocks
//!     cargo run -p rs-rich-intuituive --example blocks -- --seed 7
//!
//! The game is NES-style, as the original: a 10x20 playfield, the seven
//! pieces from a shuffled bag of seven, rotation without kicks, gravity in
//! NES frames (48 at level 0 down to 1), a level every ten lines, 40, 100,
//! 300 and 1200 points times the level plus one, a ghost piece, a pause
//! that hides the board, a statistics panel counting each piece, and a top
//! score kept in a file. Added here: a hold panel (`c`), and the cleared
//! rows flash before they go (the original shows them still).
//!
//! Keys, as the original's: Space starts, pauses, resumes and starts again
//! after a game over; ← → move; ↓ drops a row (soft drop); ↑ slams the
//! piece down (hard drop); PageUp and PageDown rotate left and right; `c`
//! holds; `q` quits.
//!
//! The game runs on the app's clock: a `fixed_step` of one NES frame (17 ms,
//! as the original's timer), paused off the playing screen, so a test seeds
//! the bag, presses keys and advances the clock, and sees the same game on
//! every run. ← → and ↓ repeat at the NES's own rates while held (DAS and
//! ARR) where the terminal reports releases, else at the terminal's.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::sprite::{stage_with, Placed, Sprite, StageOptions};
use intuituive::{state_file, write_atomic, Ctx};

// The model: no framework in it.

/// The playfield's width and height, in cells.
pub const WIDTH: usize = 10;
pub const HEIGHT: usize = 20;

/// One NES frame on the original's timer: gravity counts these.
pub const FRAME: Duration = Duration::from_millis(17);

/// Held ← →: the first repeat after 16 frames, then one every 6 (the
/// NES's DAS and ARR); held ↓ drops a row every 2 frames.
const SHIFT_DELAY: Duration = Duration::from_millis(17 * 16);
const SHIFT_RATE: Duration = Duration::from_millis(17 * 6);
const DROP_RATE: Duration = Duration::from_millis(17 * 2);

/// Frames a row takes to fall, by level (NES's table, as the original's).
const GRAVITY: [u32; 15] = [48, 43, 38, 33, 28, 23, 18, 13, 8, 6, 5, 4, 3, 2, 1];

/// Points for clearing 0 to 4 rows at once, times the level plus one.
const BASE_SCORES: [u32; 5] = [0, 40, 100, 300, 1200];

/// The seven pieces, in the original's order (its statistics panel's).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    J,
    Z,
    O,
    S,
    T,
    L,
    I,
}

pub const KINDS: [Kind; 7] = [
    Kind::J,
    Kind::Z,
    Kind::O,
    Kind::S,
    Kind::T,
    Kind::L,
    Kind::I,
];

impl Kind {
    fn index(self) -> usize {
        self as usize
    }

    /// The piece's rotations, as the original's matrices (`#` filled).
    fn rotations(self) -> &'static [[&'static str; 4]] {
        match self {
            Kind::T => &[
                ["...", "###", ".#.", ""],
                [".#.", "##.", ".#.", ""],
                [".#.", "###", "...", ""],
                [".#.", ".##", ".#.", ""],
            ],
            Kind::J => &[
                ["...", "###", "..#", ""],
                [".#.", ".#.", "##.", ""],
                ["#..", "###", "...", ""],
                [".##", ".#.", ".#.", ""],
            ],
            Kind::Z => &[["...", ".##", "##.", ""], [".#.", ".##", "..#", ""]],
            Kind::O => &[["##", "##", "", ""]],
            Kind::S => &[["...", "##.", ".##", ""], ["..#", ".##", ".#.", ""]],
            Kind::L => &[
                ["...", "###", "#..", ""],
                ["##.", ".#.", ".#.", ""],
                ["..#", "###", "...", ""],
                [".#.", ".#.", ".##", ""],
            ],
            Kind::I => &[
                ["....", "....", "####", "...."],
                ["..#.", "..#.", "..#.", "..#."],
            ],
        }
    }

    /// Where the matrix's top left starts on the board (column, row).
    fn spawn(self) -> (i32, i32) {
        match self {
            Kind::O => (4, 0),
            Kind::I => (3, -2),
            _ => (4, -1),
        }
    }

    /// The piece as the side panels draw it, two rows (the original's art).
    fn art(self) -> [&'static str; 2] {
        match self {
            Kind::O => ["████", "████"],
            Kind::L => ["██████", "██"],
            Kind::J => ["██", "██████"],
            Kind::T => ["  ██", "██████"],
            Kind::S => ["████", "  ████"],
            Kind::Z => ["  ████", "████"],
            Kind::I => ["████████", ""],
        }
    }
}

/// The falling piece.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Piece {
    pub kind: Kind,
    pub rotation: usize,
    pub col: i32,
    pub row: i32,
}

impl Piece {
    fn new(kind: Kind) -> Piece {
        let (col, row) = kind.spawn();
        Piece {
            kind,
            rotation: 0,
            col,
            row,
        }
    }

    /// The board cells it covers.
    fn cells(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        let shape = &self.kind.rotations()[self.rotation];
        shape.iter().enumerate().flat_map(move |(y, line)| {
            line.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'#')
                .map(move |(x, _)| (self.col + x as i32, self.row + y as i32))
        })
    }
}

/// A seeded generator (SplitMix64), so a test gets the same bag each run.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A shuffled bag of the seven pieces; the game takes from its end.
    fn bag(&mut self) -> Vec<Kind> {
        let mut bag = KINDS.to_vec();
        for i in (1..bag.len()).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            bag.swap(i, j);
        }
        bag
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum State {
    Start,
    Playing,
    Paused,
    GameOver,
}

/// Rows being cleared: they flash for a while, then go.
#[derive(Clone, Debug)]
struct Clearing {
    rows: Vec<usize>,
    frames_left: u32,
}

/// How many frames the cleared rows stay: the original pauses
/// 300 ms plus 100 ms a row.
fn clear_frames(rows: usize) -> u32 {
    let ms = 300 + 100 * rows as u32;
    ms.div_ceil(FRAME.as_millis() as u32)
}

/// The flash: lit for four frames, dark for four.
const FLASH_FRAMES: u32 = 4;

/// The whole game.
#[derive(Clone, Debug)]
pub struct Game {
    board: [[Option<Kind>; WIDTH]; HEIGHT],
    piece: Piece,
    next: Kind,
    hold: Option<Kind>,
    held: bool,
    bag: Vec<Kind>,
    rng: Rng,
    pub lines: u16,
    pub level: u8,
    pub score: u32,
    pub top: u32,
    pub stats: [u16; 7],
    pub state: State,
    gravity: u32,
    clearing: Option<Clearing>,
    /// Where the top score is kept; `None` keeps it in memory only.
    score_file: Option<PathBuf>,
}

impl Game {
    pub fn new(seed: u64, score_file: Option<PathBuf>) -> Game {
        let top = score_file.as_ref().map_or(0, load_top);
        let mut rng = Rng(seed);
        let mut bag = rng.bag();
        let first = bag.pop().expect("a bag has seven pieces");
        let next = bag.pop().expect("a bag has seven pieces");
        let mut stats = [0; 7];
        stats[first.index()] += 1;
        Game {
            board: [[None; WIDTH]; HEIGHT],
            piece: Piece::new(first),
            next,
            hold: None,
            held: false,
            bag,
            rng,
            lines: 0,
            level: 0,
            score: 0,
            top,
            stats,
            state: State::Start,
            gravity: 0,
            clearing: None,
            score_file,
        }
    }

    pub fn piece(&self) -> Piece {
        self.piece
    }

    pub fn next(&self) -> Kind {
        self.next
    }

    pub fn held(&self) -> Option<Kind> {
        self.hold
    }

    /// The board's cell, `None` when empty.
    pub fn at(&self, col: usize, row: usize) -> Option<Kind> {
        self.board[row][col]
    }

    /// Fill the board's bottom rows from text, `#` filled (for tests).
    pub fn fill_rows(&mut self, rows: &[&str]) {
        let top = HEIGHT - rows.len();
        for (i, line) in rows.iter().enumerate() {
            for (x, b) in line.bytes().take(WIDTH).enumerate() {
                self.board[top + i][x] = (b == b'#').then_some(Kind::O);
            }
        }
    }

    fn collides(&self, piece: &Piece) -> bool {
        piece.cells().any(|(x, y)| {
            x < 0
                || x >= WIDTH as i32
                || y >= HEIGHT as i32
                || (y >= 0 && self.board[y as usize][x as usize].is_some())
        })
    }

    /// How far the piece would fall: the ghost's and the slam's distance.
    pub fn drop_distance(&self) -> i32 {
        let mut piece = self.piece;
        let mut distance = 0;
        loop {
            piece.row += 1;
            if self.collides(&piece) {
                return distance;
            }
            distance += 1;
        }
    }

    fn live(&self) -> bool {
        self.state == State::Playing && self.clearing.is_none()
    }

    /// Space: start, pause, resume, or a new game after one ends.
    pub fn space(&mut self) {
        match self.state {
            State::Start => self.state = State::Playing,
            State::Playing => self.state = State::Paused,
            State::Paused => self.state = State::Playing,
            State::GameOver => self.new_game(),
        }
    }

    fn new_game(&mut self) {
        let top = self.top.max(self.score);
        let seed = self.rng.next();
        let file = self.score_file.take();
        *self = Game::new(seed, None);
        self.top = top;
        self.score_file = file;
        self.state = State::Playing;
    }

    pub fn shift(&mut self, dx: i32) {
        if !self.live() {
            return;
        }
        let mut moved = self.piece;
        moved.col += dx;
        if !self.collides(&moved) {
            self.piece = moved;
        }
    }

    /// Down one row; on the floor the piece locks.
    pub fn soft_drop(&mut self) {
        if self.live() {
            self.fall();
        }
    }

    fn fall(&mut self) {
        let mut moved = self.piece;
        moved.row += 1;
        if self.collides(&moved) {
            self.lock();
        } else {
            self.piece = moved;
        }
    }

    /// Slam: to the ghost, and lock there.
    pub fn hard_drop(&mut self) {
        if !self.live() {
            return;
        }
        self.piece.row += self.drop_distance();
        self.lock();
    }

    /// Rotate: +1 right (PageDown), -1 left (PageUp). No kicks, as the
    /// original: a rotation that collides does nothing.
    pub fn rotate(&mut self, direction: i32) {
        if !self.live() {
            return;
        }
        let count = self.piece.kind.rotations().len() as i32;
        let mut turned = self.piece;
        turned.rotation = (self.piece.rotation as i32 + direction).rem_euclid(count) as usize;
        if !self.collides(&turned) {
            self.piece = turned;
        }
    }

    /// Hold: swap the piece with the held one (or the next), once a piece.
    pub fn hold(&mut self) {
        if !self.live() || self.held {
            return;
        }
        let kind = self.piece.kind;
        match self.hold.replace(kind) {
            Some(held) => {
                self.piece = Piece::new(held);
                self.gravity = 0;
            }
            None => self.spawn(),
        }
        self.held = true;
    }

    fn lock(&mut self) {
        for (x, y) in self.piece.cells().collect::<Vec<_>>() {
            if y >= 0 {
                self.board[y as usize][x as usize] = Some(self.piece.kind);
            }
        }
        let rows: Vec<usize> = (0..HEIGHT)
            .filter(|&y| self.board[y].iter().all(Option::is_some))
            .collect();
        if rows.is_empty() {
            self.spawn();
        } else {
            self.score += (self.level as u32 + 1) * BASE_SCORES[rows.len()];
            self.lines += rows.len() as u16;
            if self.lines / 10 != self.level as u16 {
                self.level += 1;
            }
            self.clearing = Some(Clearing {
                frames_left: clear_frames(rows.len()),
                rows,
            });
        }
    }

    /// The next piece comes in; when it cannot fall at once, the game is
    /// over (the original's rule).
    fn spawn(&mut self) {
        self.piece = Piece::new(self.next);
        // A new piece starts its fall from a full count, as the original's
        // does, rather than inheriting the last one's.
        self.gravity = 0;
        self.held = false;
        self.stats[self.next.index()] += 1;
        if self.bag.is_empty() {
            self.bag = self.rng.bag();
        }
        self.next = self.bag.pop().expect("a bag has seven pieces");
        let mut below = self.piece;
        below.row += 1;
        if self.collides(&below) {
            self.game_over();
        }
    }

    fn game_over(&mut self) {
        self.state = State::GameOver;
        self.board = [[None; WIDTH]; HEIGHT];
        if self.score > self.top {
            if let Some(file) = &self.score_file {
                let _ = save_top(file, self.score);
            }
        }
    }

    /// A NES frame went by. Returns whether anything shown changed.
    pub fn frame(&mut self) -> bool {
        if self.state != State::Playing {
            return false;
        }
        if let Some(clearing) = &mut self.clearing {
            clearing.frames_left = clearing.frames_left.saturating_sub(1);
            if clearing.frames_left == 0 {
                let rows = self.clearing.take().expect("rows are clearing").rows;
                for &y in &rows {
                    for row in (1..=y).rev() {
                        self.board[row] = self.board[row - 1];
                    }
                    self.board[0] = [None; WIDTH];
                }
                self.spawn();
            }
            return true;
        }
        self.gravity += 1;
        if self.gravity >= gravity_frames(self.level) {
            self.gravity = 0;
            self.fall();
            return true;
        }
        false
    }

    /// What the screen shows.
    pub fn view(&self) -> View {
        let mut cells = [[Cell::Empty; WIDTH]; HEIGHT];
        for (y, row) in self.board.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                if let Some(kind) = cell {
                    cells[y][x] = Cell::Block(*kind);
                }
            }
        }
        if let Some(clearing) = &self.clearing {
            let elapsed = clear_frames(clearing.rows.len()) - clearing.frames_left;
            let lit = (elapsed / FLASH_FRAMES).is_multiple_of(2);
            for &y in &clearing.rows {
                cells[y] = [Cell::Clear(lit); WIDTH];
            }
        }
        let falling = (self.state == State::Playing && self.clearing.is_none())
            .then(|| (self.piece, self.drop_distance()));
        View {
            state: self.state,
            cells,
            falling,
            lines: self.lines,
            level: self.level,
            score: self.score,
            top: self.top,
            stats: self.stats,
            next: self.next,
            hold: self.hold,
        }
    }
}

fn gravity_frames(level: u8) -> u32 {
    let at = match level {
        0..=9 => level as usize,
        10..=12 => 10,
        13..=15 => 11,
        16..=18 => 12,
        19..=28 => 13,
        _ => 14,
    };
    GRAVITY[at]
}

fn load_top(file: &PathBuf) -> u32 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

fn save_top(file: &std::path::Path, score: u32) -> std::io::Result<()> {
    write_atomic(file, score.to_string().as_bytes())
}

/// A cell of the board as drawn, under the falling piece.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cell {
    Empty,
    Block(Kind),
    Clear(bool),
}

/// What the screen shows: the nodes read this, so a frame where nothing
/// shown changed draws nothing.
#[derive(Clone, PartialEq, Debug)]
pub struct View {
    pub state: State,
    pub cells: [[Cell; WIDTH]; HEIGHT],
    /// The piece in play and how far its ghost is below it.
    pub falling: Option<(Piece, i32)>,
    pub lines: u16,
    pub level: u8,
    pub score: u32,
    pub top: u32,
    pub stats: [u16; 7],
    pub next: Kind,
    pub hold: Option<Kind>,
}

// The look.

/// The screen's size: the original draws a fixed 64x28 screen, centred.
pub const SCREEN_WIDTH: u16 = 64;
pub const SCREEN_HEIGHT: u16 = 28;

/// The screen's background (xterm colour 234).
const BG: &str = "color(234)";

/// Each level's colours: the seven pieces in `KINDS`' order, then the
/// cleared rows' (the original's ten palettes).
const PALETTES: [[u8; 8]; 10] = [
    [69, 63, 57, 69, 75, 33, 39, 81],
    [40, 46, 47, 34, 28, 22, 82, 84],
    [219, 213, 207, 201, 200, 199, 206, 205],
    [33, 63, 27, 39, 41, 47, 46, 48],
    [85, 120, 48, 199, 200, 201, 207, 209],
    [69, 63, 57, 85, 79, 120, 115, 117],
    [242, 244, 249, 196, 160, 124, 202, 204],
    [52, 88, 89, 91, 93, 141, 129, 131],
    [196, 160, 124, 20, 27, 111, 75, 214],
    [222, 216, 221, 202, 196, 160, 228, 230],
];

fn colour(level: u8, slot: usize) -> String {
    format!("color({}) on {BG}", PALETTES[level as usize % 10][slot])
}

const TITLE: &str = "  ██████  ████  ██████  ████    ██    ████    ██
    ██    ██      ██    ██  ██  ██  ██        ██
    ██    ████    ██    ████    ██    ████    ██
    ██    ██      ██    ████    ██        ██
    ██    ████    ██    ██  ██  ██    ████    ██";

const PAUSED: &str = " ██████  ██████  ██  ██    ████    ██████  ████
 ██  ██  ██  ██  ██  ██  ██        ██      ██  ██
 ██████  ██████  ██  ██    ████    ████    ██  ██
 ██      ██  ██  ██  ██        ██  ██      ██  ██
 ██      ██  ██  ██████    ████    ██████  ████";

const GAME_OVER: &str = "   ████      ██      ██  ██    ██████
 ██        ██  ██  ██  ██  ██  ██
 ██  ████  ██████  ██      ██  ████
 ██    ██  ██  ██  ██      ██  ██
   ████    ██  ██  ██      ██  ██████

               ██    ██  ██  ██████  ██████    ██
             ██  ██  ██  ██  ██      ██    ██  ██
             ██  ██  ██  ██  ████    ██    ██  ██
             ██  ██  ██  ██  ██      ██████
               ██      ██    ██████  ██    ██  ██";

const CONTROLS: &str = " start: space quit: q rot: pgup/dn move: ←→ slam: ↑ drop: ↓";

const ZOOM_TIP: &str = "Tip: On many systems you can adjust the zoom. You can try
this by holding down the Ctrl key, and pressing +, -, or
scrolling the mouse wheel.";

/// A node at the original's absolute rectangle inside the 64x28 screen:
/// `layers` with an inset from all four sides.
fn at(node: Node, x: u16, y: u16, width: u16, height: u16) -> Node {
    node.inset(y, SCREEN_WIDTH - x - width, SCREEN_HEIGHT - y - height, x)
}

/// A box in the original's look: square corners, white on the background.
fn boxed(node: Node) -> Node {
    node.borders(Sides::ALL)
        .border(Border::Plain)
        .border_style(&format!("white on {BG}"))
        .style(&format!("white on {BG}"))
}

/// Text that is never wrapped.
fn plain(markup: impl Fn() -> String + 'static) -> Node {
    text(markup).no_wrap()
}

/// A piece's art in the level's colour, or nothing.
fn art_markup(kind: Option<Kind>, level: u8) -> String {
    match kind {
        Some(kind) => {
            let [a, b] = kind.art();
            let style = colour(level, kind.index());
            format!("[{style}]{a}\n{b}[/]")
        }
        None => String::new(),
    }
}

/// A piece as a sprite, two columns a cell, its matrix's blanks holes:
/// `██` for the piece, `┌┐` for its ghost (the original's bordered block).
fn piece_sprite(piece: &Piece, level: u8, glyph: &str) -> Sprite {
    let style = colour(level, piece.kind.index());
    let lines: Vec<String> = piece.kind.rotations()[piece.rotation]
        .iter()
        .map(|line| {
            line.chars()
                .map(|c| match c {
                    '#' => format!("[{style}]{glyph}[/]"),
                    _ => "..".to_string(),
                })
                .collect()
        })
        .collect();
    Sprite::from_markup(&lines.iter().map(String::as_str).collect::<Vec<_>>(), '.')
}

/// The playfield: the piece and its ghost are sprites on a stage, over the
/// board, which is twenty rows of markup (two columns a cell, and the
/// cleared rows' flash).
fn playfield(view: Signal<View>) -> Node {
    let sprites = move || {
        view.with(|v| match &v.falling {
            Some((piece, distance)) => {
                let (x, y) = (2.0 * piece.col as f64, piece.row as f64);
                vec![
                    Placed::new(0, piece_sprite(piece, v.level, "┌┐")).at(x, y + *distance as f64),
                    Placed::new(1, piece_sprite(piece, v.level, "██"))
                        .at(x, y)
                        .z(1),
                ]
            }
            None => Vec::new(),
        })
    };
    stage_with(sprites, StageOptions::new().backdrop(board(view)))
}

/// The board under the piece.
fn board(view: Signal<View>) -> Node {
    plain(move || {
        view.with(|v| {
            let mut out = String::new();
            for (y, row) in v.cells.iter().enumerate() {
                if y > 0 {
                    out.push('\n');
                }
                for cell in row {
                    let (glyph, style) = match *cell {
                        Cell::Empty => ("  ", format!("on {BG}")),
                        Cell::Block(kind) => ("██", colour(v.level, kind.index())),
                        Cell::Clear(lit) => (if lit { "██" } else { "░░" }, colour(v.level, 7)),
                    };
                    out.push_str(&format!("[{style}]{glyph}[/]"));
                }
            }
            out
        })
    })
}

/// The playing screen: statistics, lines, the board, scores, next, level
/// and hold, at the original's places.
fn playing(view: Signal<View>) -> Node {
    let stats = KINDS.map(|kind| {
        row([
            plain(move || art_markup(Some(kind), view.with(|v| v.level))).percent(60),
            plain(move || view.with(|v| format!(" {:03}", v.stats[kind.index()]))).percent(40),
        ])
        .fixed(3)
    });
    let statistics = column([
        label("    STATISTICS").fixed(2),
        column(stats).padding(0, 2),
    ]);
    let lines = plain(move || view.with(|v| format!("     LINES - {:03}", v.lines)));
    let scores = plain(move || {
        view.with(|v| format!("\n TOP\n     {:06}\n\n SCORE\n     {:06}", v.top, v.score))
    });
    let next = column([
        label("   NEXT").fixed(2),
        plain(move || view.with(|v| art_markup(Some(v.next), v.level))).padding(0, 1),
    ]);
    let level = plain(move || view.with(|v| format!("  LEVEL {:02}", v.level)));
    let hold = column([
        label(" HOLD (c)").fixed(1),
        plain(move || view.with(|v| art_markup(v.hold, v.level))).padding(0, 1),
    ]);
    layers([
        at(boxed(statistics), 4, 1, 20, 24),
        at(boxed(lines), 24, 1, 22, 3),
        at(boxed(playfield(view)), 24, 4, 22, 22),
        at(boxed(scores), 46, 1, 14, 9),
        at(boxed(next), 46, 12, 12, 7),
        at(boxed(level), 46, 19, 14, 3),
        at(boxed(hold), 46, 22, 12, 5),
    ])
}

fn start_screen() -> Node {
    layers([
        at(boxed(label(TITLE).no_wrap()), 7, 4, 52, 7),
        at(
            boxed(label(ZOOM_TIP).no_wrap()).style(&format!("bright_black on {BG}")),
            4,
            19,
            57,
            5,
        ),
    ])
}

fn paused_screen() -> Node {
    layers([at(boxed(label(PAUSED).no_wrap()), 7, 4, 52, 7)])
}

/// The top score is the one from before the game (the original's): it
/// takes the new one when the next game starts.
fn game_over_screen(view: Signal<View>) -> Node {
    let width = (SCREEN_WIDTH - 9) as usize;
    let message = move || {
        let (score, top) = view.with(|v| (v.score, v.top));
        let text = if score > top {
            format!("New top score set at {score}!")
        } else {
            format!("You scored {score} points.")
        };
        format!("{text:^width$}")
    };
    let stats = move || {
        let (level, lines) = view.with(|v| (v.level, v.lines));
        let text = format!("Reach level {level}, and cleared {lines} lines.");
        format!("{text:^width$}")
    };
    layers([
        at(boxed(label(GAME_OVER).no_wrap()), 7, 4, 52, 13),
        at(boxed(plain(stats)), 4, 19, 57, 3),
        at(boxed(plain(message)), 4, 23, 57, 3),
    ])
}

// The app.

pub struct Options {
    /// The bag's seed; a test fixes it, a game takes the time.
    pub seed: u64,
    /// Where the top score is kept, `None` for nowhere.
    pub score_file: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Options {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |d| d.as_nanos() as u64);
        Options {
            seed,
            score_file: default_score_file(),
        }
    }
}

/// `top_score` in the app's state directory (the original keeps
/// `~/top_score`).
fn default_score_file() -> Option<PathBuf> {
    state_file("rs-rich-blocks", "top_score")
}

impl Options {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
        let mut options = Options::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--seed" => {
                    let value = args.next().ok_or("--seed takes a number")?;
                    options.seed = value.parse().map_err(|_| format!("bad seed {value}"))?;
                }
                "--score-file" => {
                    options.score_file =
                        Some(args.next().ok_or("--score-file takes a path")?.into())
                }
                "--no-score-file" => options.score_file = None,
                other => return Err(format!("unknown argument {other}")),
            }
        }
        Ok(options)
    }
}

/// The game, shared by the handlers, what it shows, and its clock.
#[derive(Clone)]
struct Shared {
    game: Rc<RefCell<Game>>,
    view: Signal<View>,
    clock: Timer,
}

impl Shared {
    /// Change the game, then show it (an equal view draws nothing); the
    /// clock runs only while the game is played.
    fn act(&self, f: impl FnOnce(&mut Game)) {
        let mut game = self.game.borrow_mut();
        f(&mut game);
        self.view.set(game.view());
        match game.state {
            State::Playing => self.clock.resume(),
            _ => self.clock.pause(),
        }
    }

    fn key(&self, f: impl Fn(&mut Game) + 'static) -> impl FnMut(&mut Ctx) + 'static {
        let shared = self.clone();
        move |_| shared.act(&f)
    }
}

pub fn blocks_app(options: Options) -> App {
    game_app(Game::new(options.seed, options.score_file))
}

/// The app on a game already set up: a test's board.
pub fn game_app(game: Game) -> App {
    App::new(move || {
        let view = signal(game.view());
        let game = Rc::new(RefCell::new(game));
        // A NES frame a step, every frame owed after a stall (half a
        // second's at most), so a slow turn of the loop does not slow the
        // game down.
        let ticking = game.clone();
        let clock = fixed_step(FRAME, move |_, steps| {
            let mut game = ticking.borrow_mut();
            let mut changed = false;
            for _ in 0..steps {
                changed |= game.frame();
            }
            if changed {
                view.set(game.view());
            }
        })
        .max_catch_up(30);
        let shared = Shared { game, view, clock };
        shared.act(|_| ());

        let state = memo(move || view.with(|v| v.state));
        // The screen's colour covers the room between its boxes; the
        // backdrop is a `fill` in a layer below, showing round it. The
        // original tiles a pattern of grey blocks; `fill` takes one glyph.
        let screen = layers([
            switch(
                move || state.get(),
                move |state| match state {
                    State::Start => start_screen(),
                    State::Playing => playing(view),
                    State::Paused => paused_screen(),
                    State::GameOver => game_over_screen(view),
                },
            ),
            at(label(CONTROLS).no_wrap(), 2, 27, 60, 1).style(&format!("white on {BG}")),
        ])
        .style(&format!("on {BG}"));
        layers([
            fill("▓", "color(236) on color(235)"),
            column([row([screen.fixed(SCREEN_WIDTH)])
                .justify(Justify::Center)
                .fixed(SCREEN_HEIGHT)])
            .justify(Justify::Center),
        ])
        .on_key("left", shared.key(|g| g.shift(-1)))
        .repeat(SHIFT_DELAY, SHIFT_RATE)
        .on_key("right", shared.key(|g| g.shift(1)))
        .repeat(SHIFT_DELAY, SHIFT_RATE)
        .on_key("down", shared.key(Game::soft_drop))
        .repeat(DROP_RATE, DROP_RATE)
        .on_key("up", shared.key(Game::hard_drop))
        .on_key("pageup", shared.key(|g| g.rotate(-1)))
        .on_key("pagedown", shared.key(|g| g.rotate(1)))
        .on_key("c", shared.key(Game::hold))
        .on_key("space", shared.key(Game::space))
        .on_key("q", |cx| cx.quit())
    })
    .min_size(
        SCREEN_WIDTH,
        SCREEN_HEIGHT,
        format!(
            "[b]Terminal must be at least {SCREEN_WIDTH} x {SCREEN_HEIGHT}![/]\n\n{}",
            ZOOM_TIP.replace('\n', " ")
        ),
    )
}

fn main() -> std::io::Result<()> {
    let options = Options::parse(std::env::args().skip(1))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    blocks_app(options).run()
}

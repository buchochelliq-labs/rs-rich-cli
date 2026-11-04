//! Animated FIGlet banners: text, an effect and a time give a frame.
//!
//! [`AnimatedBanner`] lays text out in FIGlet letters once, then
//! [`frame`](AnimatedBanner::frame) turns a time into a [`BannerFrame`]: a
//! grid of characters, each with a true-colour foreground. It is a pure
//! function of the banner and the time, with no clock and no terminal, so
//! a CLI can print frames on a timer, a TUI can draw them on its clock, and
//! a test can snapshot any instant. Past the banner's
//! [`duration`](AnimatedBanner::duration) every frame is the still final
//! one ([`still`](AnimatedBanner::still)), which is also what to draw with
//! no colour, with reduced motion, or when output is not a terminal.
//!
//! The effects ([`Effect`]):
//!
//! * [`Glow`](Effect::Glow), the default: the letters start dark, and a
//!   soft band of blue-green light travels left to right through them,
//!   leaving them lit, like plankton glowing where the water moves.
//! * [`Shimmer`](Effect::Shimmer): lit letters with a narrow highlight
//!   sweeping across on a slant.
//! * [`Typewriter`](Effect::Typewriter): the letters appear a column at a
//!   time.
//! * [`Fade`](Effect::Fade): the letters brighten from dark.
//! * [`Still`](Effect::Still): no animation.
//!
//! Two optional decoration layers, both deterministic from a
//! [`seed`](AnimatedBanner::seed): a [wave line](AnimatedBanner::wave)
//! under the title, and [specks of light](AnimatedBanner::specks) (`·∘°`)
//! rising through the blank cells around the letters and in a row above
//! them.
//!
//! Colours come from a [`BannerPalette`]; rendered through a `rich`
//! console, they are reduced to 256 or 16 colours as the console's colour
//! system needs, and dropped with no colour.
//!
//! ```
//! use std::time::Duration;
//!
//! use rich_art::animate::{AnimatedBanner, Effect};
//! use rich_art::FigletFont;
//!
//! let banner = AnimatedBanner::new("Hi", &FigletFont::standard()).effect(Effect::Glow);
//! // Halfway, the band is in the letters; the still frame has them all lit.
//! let middle = banner.frame(Duration::from_secs(1));
//! let still = banner.still();
//! assert_eq!(middle.plain(), still.plain());
//! assert_ne!(middle, still);
//! assert_eq!(still.plain()[1].trim_end(), "| | | (_)");
//! ```

use std::time::Duration;

use rich::color::Color;
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::style::Style;

use crate::figlet::{self, FigletFont, Justify};

/// A colour as red, green and blue.
pub type Rgb = [u8; 3];

/// How long an animation runs by default: two seconds.
pub const DEFAULT_DURATION: Duration = Duration::from_secs(2);

/// The characters specks are drawn with.
const SPECKS: [char; 5] = ['·', '∘', '°', '˚', '.'];

/// The wave line's characters, from trough to crest.
const WAVE: [char; 5] = ['_', '.', '-', '~', '\''];

/// What happens to the letters over the animation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Effect {
    /// Dark letters lit by a soft band of light travelling left to right.
    #[default]
    Glow,
    /// Lit letters with a narrow highlight sweeping across.
    Shimmer,
    /// The letters appear a column at a time.
    Typewriter,
    /// The letters brighten from dark.
    Fade,
    /// Every frame is the still one.
    Still,
}

impl Effect {
    /// Every effect, in the order help lists them.
    pub const ALL: [Effect; 5] = [
        Effect::Glow,
        Effect::Shimmer,
        Effect::Typewriter,
        Effect::Fade,
        Effect::Still,
    ];

    /// Its name: `glow`, `shimmer`, `typewriter`, `fade`, `still`.
    pub fn name(self) -> &'static str {
        match self {
            Effect::Glow => "glow",
            Effect::Shimmer => "shimmer",
            Effect::Typewriter => "typewriter",
            Effect::Fade => "fade",
            Effect::Still => "still",
        }
    }

    /// The effect called `name` (any case).
    pub fn from_name(name: &str) -> Option<Effect> {
        Effect::ALL
            .into_iter()
            .find(|e| e.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// The colours a banner is drawn in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BannerPalette {
    /// Letters not yet lit.
    pub dark: Rgb,
    /// Lit letters at their left end.
    pub lit: Rgb,
    /// Lit letters at their right end: the still frame runs from `lit` to
    /// this across the title.
    pub lit_end: Rgb,
    /// The brightest light: the band's core, a highlight.
    pub crest: Rgb,
    /// The wave line.
    pub wave: Rgb,
    /// The specks.
    pub speck: Rgb,
}

impl Default for BannerPalette {
    /// Blue-green light in dark water.
    fn default() -> Self {
        BannerPalette {
            dark: [0x1b, 0x36, 0x4a],
            lit: [0x3f, 0xf0, 0xe0],
            lit_end: [0x2a, 0x7f, 0xd8],
            crest: [0xea, 0xff, 0xfd],
            wave: [0x1d, 0x5f, 0xae],
            speck: [0x9f, 0xe6, 0xff],
        }
    }
}

/// One cell of a frame: a character and its colour (`None`: the
/// terminal's own, for blanks).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BannerCell {
    /// The character drawn.
    pub ch: char,
    /// Its colour; `None` is the terminal's own.
    pub fg: Option<Rgb>,
}

impl BannerCell {
    const BLANK: BannerCell = BannerCell { ch: ' ', fg: None };
}

/// A frame of an [`AnimatedBanner`]: rows of cells, every row as wide as
/// the banner.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BannerFrame {
    rows: Vec<Vec<BannerCell>>,
}

impl BannerFrame {
    /// Its rows of cells.
    pub fn rows(&self) -> &[Vec<BannerCell>] {
        &self.rows
    }

    /// The cell at column `x` of row `y`.
    pub fn cell(&self, x: usize, y: usize) -> Option<BannerCell> {
        self.rows.get(y).and_then(|row| row.get(x)).copied()
    }

    /// Its characters, a string a row.
    pub fn plain(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| row.iter().map(|c| c.ch).collect())
            .collect()
    }

    /// Columns.
    pub fn width(&self) -> usize {
        self.rows.first().map_or(0, Vec::len)
    }

    /// Rows.
    pub fn height(&self) -> usize {
        self.rows.len()
    }
}

impl Renderable for BannerFrame {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        let mut out = Vec::new();
        for (y, row) in self.rows.iter().enumerate() {
            if y > 0 {
                out.push(Segment::line());
            }
            for cell in row.iter().take(width) {
                let style = cell
                    .fg
                    .map(|[r, g, b]| Style::new().with_color(Color::from_rgb(r, g, b)));
                out.push(Segment::new(cell.ch.to_string(), style));
            }
        }
        Segment::simplify(&out)
    }
}

/// A speck of light: where it starts, how fast it rises, how it looks.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Speck {
    x: usize,
    y: f64,
    speed: f64,
    ch: char,
    phase: f64,
}

/// FIGlet text with an effect: a frame for any time. See the
/// [module docs](self).
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until a frame is taken"]
pub struct AnimatedBanner {
    lines: Vec<Vec<char>>,
    width: usize,
    effect: Effect,
    duration: Duration,
    wave: bool,
    specks: bool,
    seed: u64,
    palette: BannerPalette,
}

impl AnimatedBanner {
    /// `text` in `font`'s letters, laid out to its natural width (a
    /// newline in `text` starts another row of letters).
    pub fn new(text: &str, font: &FigletFont) -> AnimatedBanner {
        let drawn = figlet::render(text, font, usize::MAX / 4, Justify::Left);
        AnimatedBanner::from_lines(drawn.lines())
    }

    /// Letters already laid out, a string a row (FIGlet output after a
    /// caller's own fitting and alignment). Trailing blank rows are
    /// dropped.
    pub fn from_lines<S: AsRef<str>>(lines: impl IntoIterator<Item = S>) -> AnimatedBanner {
        let mut lines: Vec<Vec<char>> = lines
            .into_iter()
            .map(|l| l.as_ref().trim_end().chars().collect())
            .collect();
        while lines.last().is_some_and(Vec::is_empty) {
            lines.pop();
        }
        let width = lines.iter().map(Vec::len).max().unwrap_or(0);
        for line in &mut lines {
            line.resize(width, ' ');
        }
        AnimatedBanner {
            lines,
            width,
            effect: Effect::default(),
            duration: DEFAULT_DURATION,
            wave: false,
            specks: false,
            seed: 0,
            palette: BannerPalette::default(),
        }
    }

    /// The effect (default [`Effect::Glow`]).
    pub fn effect(mut self, effect: Effect) -> AnimatedBanner {
        self.effect = effect;
        self
    }

    /// How long the animation runs (default two seconds).
    pub fn duration(mut self, duration: Duration) -> AnimatedBanner {
        self.duration = duration;
        self
    }

    /// A wave line under the title.
    pub fn wave(mut self, on: bool) -> AnimatedBanner {
        self.wave = on;
        self
    }

    /// Specks of light rising around the title, and a row above it for
    /// them.
    pub fn specks(mut self, on: bool) -> AnimatedBanner {
        self.specks = on;
        self
    }

    /// Where the specks start (default 0): the same seed, the same specks.
    pub fn seed(mut self, seed: u64) -> AnimatedBanner {
        self.seed = seed;
        self
    }

    /// The colours (default [`BannerPalette::default`]).
    pub fn palette(mut self, palette: BannerPalette) -> AnimatedBanner {
        self.palette = palette;
        self
    }

    /// Its effect.
    pub fn effect_of(&self) -> Effect {
        self.effect
    }

    /// How long its animation runs.
    pub fn duration_of(&self) -> Duration {
        self.duration
    }

    /// Its colours.
    pub fn palette_of(&self) -> BannerPalette {
        self.palette
    }

    /// Columns and rows of every frame, decoration included.
    pub fn size(&self) -> (usize, usize) {
        (
            self.width,
            self.lines.len() + usize::from(self.specks) + usize::from(self.wave),
        )
    }

    /// How far through the animation `t` is: 0 at the start, 1 at its end
    /// and after.
    pub fn progress(&self, t: Duration) -> f64 {
        if self.duration.is_zero() || self.effect == Effect::Still {
            return 1.0;
        }
        (t.as_secs_f64() / self.duration.as_secs_f64()).clamp(0.0, 1.0)
    }

    /// How brightly a light that keeps time with the banner shines at `t`,
    /// from 0 to 1: it pulses while the animation runs and is steady at 1
    /// once it is still. For something drawn beside the banner (a lamp, a
    /// glowing icon).
    pub fn pulse(&self, t: Duration) -> f64 {
        let p = self.progress(t);
        if p >= 1.0 {
            return 1.0;
        }
        // Three pulses over the run, rising to full at the end.
        let wave = 0.5 - 0.5 * (p * 3.0 * std::f64::consts::TAU).cos();
        (0.25 + 0.75 * wave * (0.4 + 0.6 * p)).clamp(0.0, 1.0)
    }

    /// The still final frame: what every frame at or after the end is.
    pub fn still(&self) -> BannerFrame {
        self.frame(self.duration)
    }

    /// The frame at `t` since the animation started.
    pub fn frame(&self, t: Duration) -> BannerFrame {
        let t = t.min(self.duration);
        let p = self.progress(t);
        let secs = t.as_secs_f64();
        let (width, height) = self.size();
        let top = usize::from(self.specks);
        let mut rows = vec![vec![BannerCell::BLANK; width]; height];
        // The band's centre and half width, in columns.
        let sigma = (width as f64 / 8.0).max(4.0);
        let eased = smoothstep(0.0, 1.0, p);
        for (y, line) in self.lines.iter().enumerate() {
            for (x, &ch) in line.iter().enumerate() {
                if ch == ' ' {
                    continue;
                }
                let fg = self.letter(x, y, p, eased, sigma);
                rows[top + y][x] = match fg {
                    Some(fg) => BannerCell { ch, fg: Some(fg) },
                    None => BannerCell::BLANK,
                };
            }
        }
        if self.wave {
            let y = height - 1;
            for (x, cell) in rows[y].iter_mut().enumerate() {
                let level = (x as f64 * 0.45 - secs * 5.0).sin();
                let index = (((level + 1.0) / 2.0) * (WAVE.len() - 1) as f64).round() as usize;
                let mut fg = mix(
                    self.palette.dark,
                    self.palette.wave,
                    0.6 + 0.4 * (level + 1.0) / 2.0,
                );
                if self.effect == Effect::Glow && p < 1.0 {
                    let d = x as f64 - band_centre(eased, width, sigma);
                    fg = mix(fg, self.palette.crest, 0.6 * gauss(d, sigma));
                }
                *cell = BannerCell {
                    ch: WAVE[index.min(WAVE.len() - 1)],
                    fg: Some(fg),
                };
            }
        }
        if self.specks {
            let region = top + self.lines.len();
            for speck in self.speck_set() {
                let y = (speck.y - speck.speed * secs).rem_euclid(region as f64) as usize;
                let cell = &mut rows[y.min(region - 1)][speck.x];
                if cell.ch != ' ' {
                    continue;
                }
                let twinkle = 0.5 + 0.5 * (secs * 3.0 + speck.phase).sin();
                *cell = BannerCell {
                    ch: speck.ch,
                    fg: Some(mix(
                        self.palette.dark,
                        self.palette.speck,
                        0.35 + 0.65 * twinkle,
                    )),
                };
            }
        }
        BannerFrame { rows }
    }

    /// A letter's colour at column `x` of row `y` (`None`: not shown yet).
    fn letter(&self, x: usize, y: usize, p: f64, eased: f64, sigma: f64) -> Option<Rgb> {
        let palette = &self.palette;
        let across = if self.width <= 1 {
            0.0
        } else {
            x as f64 / (self.width - 1) as f64
        };
        let rest = mix(palette.lit, palette.lit_end, across);
        if p >= 1.0 {
            return Some(rest);
        }
        Some(match self.effect {
            Effect::Glow => {
                let wobble = 1.5 * (y as f64 * 0.9 + p * 12.0).sin();
                let d = x as f64 + wobble - band_centre(eased, self.width, sigma);
                // Behind the band the letters stay lit; ahead they are dark.
                let after = smoothstep(0.0, 2.0 * sigma, -d);
                let base = mix(palette.dark, rest, after);
                mix(base, palette.crest, 0.85 * gauss(d, sigma))
            }
            Effect::Shimmer => {
                let centre = -6.0 + (self.width as f64 + self.lines.len() as f64 + 12.0) * eased;
                let d = x as f64 + y as f64 - centre;
                mix(rest, palette.crest, 0.9 * gauss(d, 2.0))
            }
            Effect::Typewriter => {
                let reveal = p * (self.width as f64 + 1.0);
                let x = x as f64;
                if x >= reveal {
                    return None;
                }
                if x + 1.0 >= reveal {
                    palette.crest
                } else {
                    rest
                }
            }
            Effect::Fade => mix(palette.dark, rest, eased),
            Effect::Still => rest,
        })
    }

    /// The specks, from the seed: about one for every 24 cells.
    fn speck_set(&self) -> Vec<Speck> {
        let region = usize::from(self.specks) + self.lines.len();
        if self.width == 0 || region == 0 {
            return Vec::new();
        }
        let count = (self.width * region / 24).max(3);
        let mut state = self.seed ^ 0x9e37_79b9_7f4a_7c15;
        let mut next = move || {
            // splitmix64
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = state;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        };
        let unit = |n: u64| (n >> 11) as f64 / (1u64 << 53) as f64;
        (0..count)
            .map(|_| Speck {
                x: (next() % self.width as u64) as usize,
                y: unit(next()) * region as f64,
                speed: 0.6 + unit(next()),
                ch: SPECKS[(next() % SPECKS.len() as u64) as usize],
                phase: unit(next()) * std::f64::consts::TAU,
            })
            .collect()
    }
}

/// Where the glow band's centre is at eased progress `eased`: from well
/// before the first column to well past the last, so it enters and leaves
/// softly.
fn band_centre(eased: f64, width: usize, sigma: f64) -> f64 {
    let start = -3.0 * sigma;
    let end = width as f64 + 3.0 * sigma + 2.0;
    start + (end - start) * eased
}

fn gauss(d: f64, sigma: f64) -> f64 {
    (-(d / sigma).powi(2)).exp()
}

fn smoothstep(from: f64, to: f64, x: f64) -> f64 {
    if to <= from {
        return if x >= to { 1.0 } else { 0.0 };
    }
    let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `a` towards `b` by `t` (0 to 1).
pub fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let lerp = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
    [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hi() -> AnimatedBanner {
        AnimatedBanner::new("Hi", &FigletFont::standard())
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn the_still_frame_is_the_letters_lit_left_to_right() {
        let still = hi().still();
        assert_eq!(
            still.plain(),
            [
                " _   _ _",
                "| | | (_)",
                "| |_| | |",
                "|  _  | |",
                "|_| |_|_|",
            ]
            .map(|r| format!("{r:<9}"))
        );
        let palette = BannerPalette::default();
        assert_eq!(still.cell(0, 1).unwrap().fg, Some(palette.lit));
        assert_eq!(still.cell(8, 1).unwrap().fg, Some(palette.lit_end));
        assert_eq!(still.cell(3, 1).unwrap().fg, None);
        // Every frame at or after the end is the still one.
        assert_eq!(hi().frame(ms(2000)), still);
        assert_eq!(hi().frame(ms(9000)), still);
    }

    #[test]
    fn the_glow_band_travels_left_to_right() {
        let banner = hi();
        let palette = banner.palette_of();
        let start = banner.frame(Duration::ZERO);
        // At the start every letter is dark.
        assert!(
            start
                .rows()
                .iter()
                .flatten()
                .filter_map(|c| c.fg)
                .all(|fg| fg == palette.dark)
        );
        let brightness = |frame: &BannerFrame, x: usize| {
            let fg = frame.cell(x, 1).unwrap().fg.unwrap();
            fg.iter().map(|&c| c as u32).sum::<u32>()
        };
        // Early on the left is brighter than the right; the band moves on.
        let early = banner.frame(ms(700));
        assert!(brightness(&early, 0) > brightness(&early, 8), "{early:?}");
        let late = banner.frame(ms(1500));
        assert!(brightness(&late, 8) > brightness(&early, 8));
        // Deterministic.
        assert_eq!(banner.frame(ms(700)), early);
    }

    #[test]
    fn other_effects() {
        let typed = hi().effect(Effect::Typewriter).frame(ms(1000));
        assert_eq!(typed.plain()[1].trim_end(), "| | |");
        let palette = BannerPalette::default();
        let faded = hi().effect(Effect::Fade).frame(Duration::ZERO);
        assert_eq!(faded.cell(0, 1).unwrap().fg, Some(palette.dark));
        let still = hi().effect(Effect::Still);
        assert_eq!(still.frame(Duration::ZERO), still.still());
        assert_eq!(still.progress(Duration::ZERO), 1.0);
        let shimmer = hi().effect(Effect::Shimmer);
        assert_ne!(shimmer.frame(ms(1000)), shimmer.still());
        assert_eq!(Effect::from_name("GLOW"), Some(Effect::Glow));
        assert_eq!(Effect::from_name("nope"), None);
    }

    #[test]
    fn the_wave_and_specks_are_deterministic_from_the_seed() {
        let banner = hi().wave(true).specks(true).seed(7);
        assert_eq!(banner.size(), (9, 7));
        let frame = banner.frame(ms(500));
        assert_eq!(frame.height(), 7);
        let wave = &frame.plain()[6];
        assert!(wave.chars().all(|c| WAVE.contains(&c)), "{wave:?}");
        // The letters keep their cells: specks only fill blanks.
        assert_eq!(&frame.plain()[2][..5], "| | |");
        let specks = |f: &BannerFrame| {
            f.plain()[..6]
                .iter()
                .flat_map(|r| r.chars())
                .filter(|c| SPECKS.contains(c))
                .count()
        };
        assert!(specks(&frame) > 0, "{:#?}", frame.plain());
        assert_eq!(banner.frame(ms(500)), frame);
        // The specks rise: another time, other cells.
        assert_ne!(banner.frame(ms(1500)).plain(), frame.plain());
        assert_ne!(banner.clone().seed(8).frame(ms(500)), frame);
    }

    #[test]
    fn the_pulse_settles_at_full() {
        let banner = hi();
        assert_eq!(banner.pulse(ms(2000)), 1.0);
        assert!(banner.pulse(Duration::ZERO) < 0.5);
        let pulses: Vec<f64> = (0..20).map(|n| banner.pulse(ms(n * 100))).collect();
        assert!(pulses.iter().all(|p| (0.0..=1.0).contains(p)));
    }

    #[test]
    fn it_renders_through_a_console_and_degrades() {
        let truecolor = Console::builder()
            .width(20)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .build();
        let out = truecolor.render_export(&hi().still());
        assert!(out.contains("38;2;63;240;224"), "{out:?}");
        let standard = Console::builder()
            .width(20)
            .force_terminal(true)
            .color_system(Some(rich::ColorSystem::Standard))
            .build();
        assert!(!standard.render_export(&hi().still()).contains("38;2"));
        let plain = Console::builder().width(20).color_system(None).build();
        assert!(!plain.render_export(&hi().still()).contains('\u{1b}'));
    }
}

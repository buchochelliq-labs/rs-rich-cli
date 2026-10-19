//! Sprites and stages: cell art placed, moved and animated over a
//! backdrop.
//!
//! A [`Sprite`] is one or more frames of cell art: rows of styled cells,
//! some of them transparent, so what is beneath shows through. Make one
//! from markup lines with a transparent character
//! ([`Sprite::from_markup`]), from cells ([`Sprite::from_cells`]), or from
//! pixel art in half blocks ([`Sprite::from_pixels`]). Frames change on
//! the app's clock ([`Sprite::period`]), looping or [once](Sprite::once).
//!
//! A [`stage`] is a node that draws [`Placed`] sprites (a sprite, an id,
//! a position in cells, a z order, a flip) from a closure that may read
//! signals: when they change, the stage draws again. Sprites are clipped
//! to the stage, drawn in z order over its [backdrop](StageOptions::backdrop)
//! (any node) or its background ([`Node::bg`]), and a cell that sets no
//! background colour takes the colour beneath it. Clicks and hovers find
//! the sprite under the pointer by its cells ([`StageOptions`]);
//! [`overlaps`] tests two placed sprites for a collision, cell by cell.
//! Movement is a [`Tween`] on the app's clock, or any signal moved with
//! [`Ctx::animate`](crate::Ctx::animate).
//!
//! ```
//! use intuituive::prelude::*;
//! use intuituive::sprite::{stage_with, Placed, Sprite, StageOptions};
//!
//! let app = App::new(|| {
//!     let ship = Sprite::from_markup(&[".^.", "/#\\"], '.');
//!     let x = signal(1.0);
//!     let picked = signal(String::from("none"));
//!     column([
//!         stage_with(
//!             move || vec![Placed::new(7, ship.clone()).at(x.get(), 0.0)],
//!             StageOptions::new()
//!                 .backdrop(fill("·", ""))
//!                 .on_sprite_click(move |id: i32, _| picked.set(format!("sprite {id}"))),
//!         )
//!         .fixed(2),
//!         text!("{picked}"),
//!     ])
//!     .on_key("right", move |_| x.update(|x| *x += 1.0))
//! });
//! let mut driver = app.driver(6, 3);
//! driver.press("right");
//! // The transparent corners show the backdrop.
//! assert_eq!(driver.screen().plain()[..2], ["···^··", "··/#\\·"]);
//! ```

use std::cell::{Cell as StdCell, OnceCell};
use std::rc::Rc;
use std::time::Duration;

use rich::{Console, Segment, Style};
use rich_interact::MouseKind;

use crate::app::{Ctx, Easing};
use crate::node::{Axis, Node};
use crate::pixels::{half_block, PixelSprite, Rgba};
use crate::reactive::{signal, Runtime, Signal};
use crate::screen::{Rect, Screen};
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

/// How long a frame shows when nothing says.
const DEFAULT_PERIOD: Duration = Duration::from_millis(100);

/// How often a stage draws while a [`Tween`] on it moves.
pub const FRAME: Duration = Duration::from_millis(16);

/// The app's clock (see [`Ctx::now`](crate::Ctx::now)) while an app is
/// built or running; zero outside one.
pub fn now() -> Duration {
    Runtime::try_current().map_or(Duration::ZERO, |runtime| runtime.now.get())
}

/// When each frame of an animation shows.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Timing {
    durations: Vec<Duration>,
    pub(crate) looping: bool,
}

impl Timing {
    pub(crate) fn still() -> Timing {
        Timing {
            durations: vec![DEFAULT_PERIOD],
            looping: true,
        }
    }

    /// One more frame, as long as the last.
    pub(crate) fn push(&mut self) {
        let last = self.durations.last().copied().unwrap_or(DEFAULT_PERIOD);
        self.durations.push(last);
    }

    pub(crate) fn set_period(&mut self, period: Duration, frames: usize) {
        self.durations = vec![period; frames.max(1)];
    }

    pub(crate) fn set_durations(&mut self, durations: &[Duration], frames: usize) {
        let last = durations.last().copied().unwrap_or(DEFAULT_PERIOD);
        self.durations = (0..frames.max(1))
            .map(|i| durations.get(i).copied().unwrap_or(last))
            .collect();
    }

    fn extend(&mut self, other: &Timing) {
        self.durations.extend_from_slice(&other.durations);
    }

    fn total(&self) -> Duration {
        self.durations.iter().sum()
    }

    /// Where in the frames `elapsed` falls: the frame, and the time left
    /// on it (`None` when it never changes again).
    fn locate(&self, elapsed: Duration) -> (usize, Option<Duration>) {
        let total = self.total();
        if self.durations.len() <= 1 || total.is_zero() {
            return (0, None);
        }
        if !self.looping && elapsed >= total {
            return (self.durations.len() - 1, None);
        }
        let total_ns = total.as_nanos();
        let mut at = Duration::from_nanos((elapsed.as_nanos() % total_ns) as u64);
        for (i, d) in self.durations.iter().enumerate() {
            if at < *d {
                return (i, Some(*d - at));
            }
            at -= *d;
        }
        (self.durations.len() - 1, None)
    }

    pub(crate) fn frame_at(&self, elapsed: Duration) -> usize {
        self.locate(elapsed).0
    }

    pub(crate) fn next_change(&self, elapsed: Duration) -> Option<Duration> {
        self.locate(elapsed).1
    }
}

/// One cell of a sprite's art: a grapheme (two columns when it is wide)
/// in a style, or a transparent cell, through which what is beneath the
/// sprite shows.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SpriteCell {
    symbol: String,
    style: Option<Style>,
}

impl SpriteCell {
    /// `symbol` (one grapheme) in `style`.
    pub fn new(symbol: impl Into<String>, style: Option<Style>) -> SpriteCell {
        let symbol = symbol.into();
        SpriteCell {
            symbol: if symbol.is_empty() {
                " ".into()
            } else {
                symbol
            },
            style,
        }
    }

    /// `symbol` in a style definition (`"bold red on blue"`); an invalid
    /// one is no style.
    pub fn styled(symbol: impl Into<String>, style: &str) -> SpriteCell {
        SpriteCell::new(symbol, Style::parse(style).ok().filter(|s| !s.is_null()))
    }

    /// A transparent cell.
    pub fn transparent() -> SpriteCell {
        SpriteCell::default()
    }

    pub fn is_transparent(&self) -> bool {
        self.symbol.is_empty()
    }

    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    pub fn style(&self) -> Option<&Style> {
        self.style.as_ref()
    }

    /// The columns it takes: 2 for a wide grapheme, else 1.
    fn columns(&self) -> u16 {
        if self.symbol.is_empty() {
            1
        } else {
            rich::cells::cell_len(&self.symbol).clamp(1, 2) as u16
        }
    }
}

/// A symbol's mirror image, for a sprite flipped left to right.
fn mirror(symbol: &str) -> Option<&'static str> {
    const PAIRS: [(&str, &str); 22] = [
        ("(", ")"),
        ("[", "]"),
        ("{", "}"),
        ("<", ">"),
        ("/", "\\"),
        ("▌", "▐"),
        ("▖", "▗"),
        ("▘", "▝"),
        ("▙", "▟"),
        ("▛", "▜"),
        ("▚", "▞"),
        ("◀", "▶"),
        ("◢", "◣"),
        ("◤", "◥"),
        ("┌", "┐"),
        ("└", "┘"),
        ("├", "┤"),
        ("╭", "╮"),
        ("╰", "╯"),
        ("⌐", "¬"),
        ("«", "»"),
        ("◖", "◗"),
    ];
    PAIRS.iter().find_map(|(a, b)| {
        if *a == symbol {
            Some(*b)
        } else if *b == symbol {
            Some(*a)
        } else {
            None
        }
    })
}

/// A grapheme of a raster's row, its columns and style; `None` is a
/// transparent cell.
type Glyph = Option<(String, u16, Option<Style>)>;

/// A frame drawn into cells, ready to copy onto the screen.
pub(crate) struct Raster {
    width: u16,
    height: u16,
    /// The cells, transparent ones blank.
    cells: Screen,
    /// Which cells are opaque, row by row.
    mask: Vec<bool>,
    /// Which opaque cells set no background, and take what is beneath.
    under: Vec<bool>,
    /// The opaque runs: row, first column, length.
    runs: Vec<(u16, u16, u16)>,
}

impl Raster {
    /// Cell rows, each a run of graphemes (`None` transparent).
    fn new(rows: Vec<Vec<Glyph>>) -> Raster {
        let width = rows
            .iter()
            .map(|row| {
                row.iter()
                    .map(|g| g.as_ref().map_or(1, |(_, w, _)| *w))
                    .sum::<u16>()
            })
            .max()
            .unwrap_or(0);
        let height = rows.len() as u16;
        let mut cells = Screen::new(width, height);
        let mut mask = vec![false; width as usize * height as usize];
        let mut under = mask.clone();
        for (y, row) in rows.iter().enumerate() {
            let mut line = Vec::with_capacity(row.len());
            let mut x = 0usize;
            for glyph in row {
                match glyph {
                    None => {
                        line.push(Segment::new(" ", None));
                        x += 1;
                    }
                    Some((symbol, w, style)) => {
                        line.push(Segment::new(symbol.clone(), style.clone()));
                        let takes = style.as_ref().is_none_or(|s| s.bgcolor().is_none());
                        for i in 0..*w as usize {
                            mask[y * width as usize + x + i] = true;
                            under[y * width as usize + x + i] = takes;
                        }
                        x += *w as usize;
                    }
                }
            }
            cells.write_lines(Rect::new(0, y as u16, width, 1), &[line]);
        }
        let mut runs = Vec::new();
        for y in 0..height {
            let mut x = 0;
            while x < width {
                if mask[y as usize * width as usize + x as usize] {
                    let start = x;
                    while x < width && mask[y as usize * width as usize + x as usize] {
                        x += 1;
                    }
                    runs.push((y, start, x - start));
                } else {
                    x += 1;
                }
            }
        }
        Raster {
            width,
            height,
            cells,
            mask,
            under,
            runs,
        }
    }

    fn opaque(&self, x: i32, y: i32) -> bool {
        x >= 0
            && y >= 0
            && x < self.width as i32
            && y < self.height as i32
            && self.mask[y as usize * self.width as usize + x as usize]
    }
}

/// Where a frame's art comes from.
#[derive(Clone)]
enum Source {
    Cells(Rc<Vec<Vec<SpriteCell>>>),
    Pixels {
        width: u16,
        height: u16,
        pixels: Rc<[Rgba]>,
    },
}

impl Source {
    /// Columns and rows.
    fn size(&self) -> (u16, u16) {
        match self {
            Source::Cells(rows) => (
                rows.iter()
                    .map(|r| r.iter().map(SpriteCell::columns).sum::<u16>())
                    .max()
                    .unwrap_or(0),
                rows.len() as u16,
            ),
            Source::Pixels { width, height, .. } => (*width, height.div_ceil(2)),
        }
    }

    /// The frame in cells, flipped left to right, and for pixel art a
    /// pixel lower (`shift`: for half-cell positions).
    fn raster(&self, flip: bool, shift: bool) -> Raster {
        match self {
            Source::Cells(rows) => Raster::new(
                rows.iter()
                    .map(|row| {
                        let glyph = |cell: &SpriteCell| {
                            (!cell.is_transparent()).then(|| {
                                let symbol = if flip {
                                    mirror(&cell.symbol).map_or(cell.symbol.clone(), str::to_string)
                                } else {
                                    cell.symbol.clone()
                                };
                                (symbol, cell.columns(), cell.style.clone())
                            })
                        };
                        if flip {
                            // A row shorter than the widest is padded, so
                            // it flips about the sprite's middle.
                            let pad =
                                self.size().0 - row.iter().map(SpriteCell::columns).sum::<u16>();
                            let mut out: Vec<_> = (0..pad).map(|_| None).collect();
                            out.extend(row.iter().rev().map(glyph));
                            out
                        } else {
                            row.iter().map(glyph).collect()
                        }
                    })
                    .collect(),
            ),
            Source::Pixels {
                width,
                height,
                pixels,
            } => {
                let shift = i32::from(shift);
                let rows = (*height as i32 + shift + 1) / 2;
                let px = |x: i32, y: i32| {
                    let x = if flip { *width as i32 - 1 - x } else { x };
                    PixelSprite::pixel(pixels, *width, x, y - shift, *height)
                };
                Raster::new(
                    (0..rows)
                        .map(|row| {
                            (0..*width as i32)
                                .map(|x| {
                                    half_block(px(x, row * 2), px(x, row * 2 + 1)).map(
                                        |(glyph, fg, bg)| {
                                            let color = |c: Option<[u8; 3]>| {
                                                c.map(|[r, g, b]| {
                                                    rich::color::Color::from_rgb(r, g, b)
                                                })
                                            };
                                            (
                                                glyph.to_string(),
                                                1,
                                                Some(Style::from_color(color(fg), color(bg))),
                                            )
                                        },
                                    )
                                })
                                .collect()
                        })
                        .collect(),
                )
            }
        }
    }
}

/// A frame and its rasters, made when first drawn: plain, flipped, a
/// pixel lower, both.
#[derive(Clone)]
struct Frame {
    source: Source,
    rasters: [OnceCell<Rc<Raster>>; 4],
}

impl Frame {
    fn new(source: Source) -> Frame {
        Frame {
            source,
            rasters: Default::default(),
        }
    }
}

#[derive(Clone)]
struct SpriteData {
    frames: Vec<Frame>,
    timing: Timing,
    anchor: (u16, u16),
}

/// Cell art: one or more frames of styled cells, some transparent, with
/// the time each shows and an anchor (the cell placed at a sprite's
/// position). Cheap to clone: the frames are shared, and each is drawn
/// into cells once, when first shown.
///
/// ```
/// use intuituive::sprite::Sprite;
/// use std::time::Duration;
///
/// let blink = Sprite::from_markup(&["[yellow]*"], ' ')
///     .with_frame(Sprite::from_markup(&["[dim]*"], ' '))
///     .period(Duration::from_millis(250));
/// assert_eq!(blink.size(), (1, 1));
/// assert_eq!(blink.frame_at(Duration::from_millis(300)), 1);
/// assert_eq!(blink.frame_at(Duration::from_millis(500)), 0);
/// ```
#[derive(Clone)]
pub struct Sprite(Rc<SpriteData>);

impl std::fmt::Debug for Sprite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sprite")
            .field("size", &self.size())
            .field("frames", &self.frame_count())
            .field("anchor", &self.0.anchor)
            .finish()
    }
}

impl Sprite {
    fn of(frames: Vec<Frame>, timing: Timing) -> Sprite {
        Sprite(Rc::new(SpriteData {
            frames,
            timing,
            anchor: (0, 0),
        }))
    }

    /// One frame from console markup lines, a row each. Every `transparent`
    /// character (unstyled or styled) is a transparent cell: `'.'`, or `' '`
    /// for art whose spaces are holes. Styles resolve without the app's
    /// theme: use colours and attributes, not theme names.
    pub fn from_markup(lines: &[&str], transparent: char) -> Sprite {
        let console = Console::builder()
            .width(4096)
            .color_system(Some(rich::ColorSystem::Truecolor))
            .force_terminal(true)
            .build();
        let mut hole = [0u8; 4];
        let hole: &str = transparent.encode_utf8(&mut hole);
        let rows = lines
            .iter()
            .map(|line| {
                let segments = crate::widget::markup_line(&console, line, 4096);
                let mut row = Vec::new();
                for segment in segments {
                    let (spans, _) = rich::cells::split_graphemes(&segment.text);
                    for (start, end, width) in spans {
                        let symbol = &segment.text[start..end];
                        if width == 0 {
                            continue;
                        }
                        row.push(if symbol == hole {
                            SpriteCell::transparent()
                        } else {
                            SpriteCell::new(symbol, segment.style.clone().filter(|s| !s.is_null()))
                        });
                    }
                }
                // A line that ends in holes keeps them: the width counts.
                row
            })
            .collect();
        Sprite::from_cells(rows)
    }

    /// One frame from rows of cells, left to right; a wide grapheme takes
    /// two columns.
    ///
    /// ```
    /// use intuituive::sprite::{Sprite, SpriteCell};
    ///
    /// let wall = SpriteCell::styled("█", "grey50");
    /// let sprite = Sprite::from_cells(vec![
    ///     vec![wall.clone(), SpriteCell::transparent(), wall.clone()],
    ///     vec![wall.clone(), wall.clone(), wall],
    /// ]);
    /// assert_eq!(sprite.size(), (3, 2));
    /// ```
    pub fn from_cells(rows: Vec<Vec<SpriteCell>>) -> Sprite {
        Sprite::of(
            vec![Frame::new(Source::Cells(Rc::new(rows)))],
            Timing::still(),
        )
    }

    /// Pixel art in half blocks: a cell for each column and two rows of
    /// pixels, every frame of `sprite`, with its timing. Pixels with alpha
    /// under half are transparent. On a [half-cell](StageOptions::half_cells)
    /// stage it moves a pixel at a time up and down.
    pub fn from_pixels(sprite: &PixelSprite) -> Sprite {
        let frames = sprite
            .frames()
            .iter()
            .map(|pixels| {
                Frame::new(Source::Pixels {
                    width: sprite.width(),
                    height: sprite.height(),
                    pixels: pixels.clone(),
                })
            })
            .collect();
        Sprite::of(frames, sprite.timing().clone())
    }

    /// The frames of `frames`, one after another: an animation made of
    /// sprites. Each frame keeps its time; set them all with
    /// [`period`](Self::period).
    ///
    /// # Panics
    /// With no sprites.
    pub fn from_frames(frames: impl IntoIterator<Item = Sprite>) -> Sprite {
        let mut frames = frames.into_iter();
        let first = frames.next().expect("Sprite::from_frames needs a frame");
        frames.fold(first, Sprite::with_frame)
    }

    fn data(&mut self) -> &mut SpriteData {
        Rc::make_mut(&mut self.0)
    }

    /// The same with `next`'s frames after its own.
    pub fn with_frame(mut self, next: Sprite) -> Sprite {
        let data = self.data();
        data.frames.extend(next.0.frames.iter().cloned());
        data.timing.extend(&next.0.timing);
        self
    }

    /// Each frame shows for `period`.
    pub fn period(mut self, period: Duration) -> Sprite {
        let frames = self.frame_count();
        self.data().timing.set_period(period, frames);
        self
    }

    /// Each frame shows for its own time (the last given for the rest).
    pub fn durations(mut self, durations: &[Duration]) -> Sprite {
        let frames = self.frame_count();
        self.data().timing.set_durations(durations, frames);
        self
    }

    /// Play once and stay on the last frame, instead of looping.
    pub fn once(mut self) -> Sprite {
        self.data().timing.looping = false;
        self
    }

    /// Place the sprite by its cell `x`, `y` (from its top left) instead
    /// of its top left corner: a ball by its middle, a character by its
    /// feet.
    pub fn anchor(mut self, x: u16, y: u16) -> Sprite {
        self.data().anchor = (x, y);
        self
    }

    /// Anchor at the middle cell.
    pub fn centered(self) -> Sprite {
        let (w, h) = self.size();
        self.anchor(w / 2, h / 2)
    }

    /// Columns and rows: the largest frame's.
    pub fn size(&self) -> (u16, u16) {
        self.0.frames.iter().fold((0, 0), |(w, h), frame| {
            let (fw, fh) = frame.source.size();
            (w.max(fw), h.max(fh))
        })
    }

    pub fn frame_count(&self) -> usize {
        self.0.frames.len()
    }

    /// The frame that shows `elapsed` after the animation started (the
    /// app's start, for a sprite [placed](Placed) without
    /// [`started`](Placed::started)).
    pub fn frame_at(&self, elapsed: Duration) -> usize {
        self.0.timing.frame_at(elapsed)
    }

    /// How long after `elapsed` the frame changes; `None` for one frame,
    /// or an animation played [once](Self::once) to its end.
    pub fn next_change(&self, elapsed: Duration) -> Option<Duration> {
        self.0.timing.next_change(elapsed)
    }

    /// Whether its art is pixels (it can move by half cells).
    fn is_pixels(&self) -> bool {
        matches!(
            self.0.frames.first().map(|f| &f.source),
            Some(Source::Pixels { .. })
        )
    }

    fn raster(&self, frame: usize, flip: bool, shift: bool) -> Rc<Raster> {
        let frame = &self.0.frames[frame % self.0.frames.len()];
        let shift = shift && matches!(frame.source, Source::Pixels { .. });
        frame.rasters[usize::from(flip) + 2 * usize::from(shift)]
            .get_or_init(|| Rc::new(frame.source.raster(flip, shift)))
            .clone()
    }
}

/// A movement from one position to another over a time on the app's
/// clock, eased.
///
/// ```
/// use intuituive::sprite::Tween;
/// use intuituive::Easing;
/// use std::time::Duration;
///
/// let ms = Duration::from_millis;
/// let hop = Tween::new((0.0, 0.0), (10.0, 4.0), ms(100), ms(200), Easing::Linear);
/// assert_eq!(hop.at(ms(0)), (0.0, 0.0));
/// assert_eq!(hop.at(ms(200)), (5.0, 2.0));
/// assert!(hop.done(ms(300)));
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tween {
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// When it starts, on the app's clock.
    pub start: Duration,
    pub duration: Duration,
    pub easing: Easing,
}

impl Tween {
    pub fn new(
        from: (f64, f64),
        to: (f64, f64),
        start: Duration,
        duration: Duration,
        easing: Easing,
    ) -> Tween {
        Tween {
            from,
            to,
            start,
            duration,
            easing,
        }
    }

    /// Where it is at `now`: `from` before it starts, `to` once done.
    pub fn at(&self, now: Duration) -> (f64, f64) {
        let t = if self.duration.is_zero() {
            1.0
        } else {
            now.saturating_sub(self.start).as_secs_f64() / self.duration.as_secs_f64()
        };
        let k = self.easing.at(t);
        (
            self.from.0 + (self.to.0 - self.from.0) * k,
            self.from.1 + (self.to.1 - self.from.1) * k,
        )
    }

    /// Whether it has arrived by `now`.
    pub fn done(&self, now: Duration) -> bool {
        now >= self.start + self.duration
    }
}

/// A [`Tween`] from `from` to `to` over `duration`, starting now on the
/// app's clock. Give it to [`Placed::tween`]; the stage draws each frame
/// while it moves.
pub fn tween(from: (f64, f64), to: (f64, f64), duration: Duration, easing: Easing) -> Tween {
    Tween::new(from, to, now(), duration, easing)
}

/// A sprite on a [`stage`]: an id for clicks and collisions, a position
/// in cells (fractions are rounded; the sprite's [anchor](Sprite::anchor)
/// goes there), a z order (higher over lower; equal ones in the order
/// given), a flip, and the frame (by the clock, unless fixed).
#[derive(Clone, Debug)]
pub struct Placed<K = usize> {
    id: K,
    sprite: Sprite,
    x: f64,
    y: f64,
    z: i32,
    flip: bool,
    frame: Option<usize>,
    start: Duration,
    tween: Option<Tween>,
}

/// Where a placed sprite is drawn, and what.
pub(crate) struct Resolved {
    x: i32,
    y: i32,
    raster: Rc<Raster>,
}

impl<K> Placed<K> {
    /// `sprite` at 0, 0.
    pub fn new(id: K, sprite: Sprite) -> Placed<K> {
        Placed {
            id,
            sprite,
            x: 0.0,
            y: 0.0,
            z: 0,
            flip: false,
            frame: None,
            start: Duration::ZERO,
            tween: None,
        }
    }

    /// At `x`, `y` cells from the stage's top left (negative, or past the
    /// stage, is clipped).
    pub fn at(mut self, x: f64, y: f64) -> Placed<K> {
        self.x = x;
        self.y = y;
        self
    }

    /// Where `tween` is, on the app's clock, instead of a fixed place.
    pub fn tween(mut self, tween: Tween) -> Placed<K> {
        self.tween = Some(tween);
        self
    }

    pub fn z(mut self, z: i32) -> Placed<K> {
        self.z = z;
        self
    }

    /// Mirrored left to right (brackets, slashes and half blocks swap).
    pub fn flip(mut self, flip: bool) -> Placed<K> {
        self.flip = flip;
        self
    }

    /// Always frame `frame`, not the clock's.
    pub fn frame(mut self, frame: usize) -> Placed<K> {
        self.frame = Some(frame);
        self
    }

    /// Its animation started at `start` on the app's clock (a sprite played
    /// [once](Sprite::once) from when it appeared).
    pub fn started(mut self, start: Duration) -> Placed<K> {
        self.start = start;
        self
    }

    pub fn id(&self) -> &K {
        &self.id
    }

    pub fn sprite(&self) -> &Sprite {
        &self.sprite
    }

    /// Where it is at `now`, in cells.
    pub fn position(&self, now: Duration) -> (f64, f64) {
        match &self.tween {
            Some(tween) => tween.at(now),
            None => (self.x, self.y),
        }
    }

    /// The frame it shows at `now`.
    pub fn frame_index(&self, now: Duration) -> usize {
        self.frame
            .unwrap_or_else(|| self.sprite.frame_at(now.saturating_sub(self.start)))
    }

    pub(crate) fn resolve(&self, now: Duration, half: bool) -> Resolved {
        let (x, y) = self.position(now);
        let frame = self.frame_index(now);
        let (ax, ay) = self.sprite.0.anchor;
        let (row, shift) = if half && self.sprite.is_pixels() {
            let halves = (y * 2.0).round() as i64;
            (halves.div_euclid(2), halves.rem_euclid(2) == 1)
        } else {
            (y.round() as i64, false)
        };
        Resolved {
            x: (x.round() as i64 - ax as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            y: (row - ay as i64).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            raster: self.sprite.raster(frame, self.flip, shift),
        }
    }

    /// The cells it covers at `now`: its top left and its frame's size.
    pub fn bounds(&self, now: Duration) -> Rect32 {
        let r = self.resolve(now, false);
        Rect32 {
            x: r.x,
            y: r.y,
            width: r.raster.width,
            height: r.raster.height,
        }
    }

    /// Whether an opaque cell of it is at `x`, `y` at `now`.
    pub fn contains(&self, x: i32, y: i32, now: Duration) -> bool {
        let r = self.resolve(now, false);
        r.raster.opaque(x - r.x, y - r.y)
    }
}

/// A rectangle of cells that may start off the stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rect32 {
    pub x: i32,
    pub y: i32,
    pub width: u16,
    pub height: u16,
}

impl Rect32 {
    pub fn intersects(&self, other: &Rect32) -> bool {
        self.x < other.x + other.width as i32
            && other.x < self.x + self.width as i32
            && self.y < other.y + other.height as i32
            && other.y < self.y + self.height as i32
    }
}

/// Whether `a` and `b` share an opaque cell, on the app's clock (zero
/// outside an app): a collision. Positions are whole cells, as a stage
/// without [half cells](StageOptions::half_cells) draws them.
///
/// ```
/// use intuituive::sprite::{overlaps, bounds_overlap, Placed, Sprite};
///
/// // An L and a dot in the L's empty corner: their boxes meet, their
/// // cells do not.
/// let l = Sprite::from_markup(&["#.", "##"], '.');
/// let dot = Sprite::from_markup(&["o"], '.');
/// let a = Placed::new(1, l).at(0.0, 0.0);
/// let b = Placed::new(2, dot).at(1.0, 0.0);
/// assert!(bounds_overlap(&a, &b));
/// assert!(!overlaps(&a, &b));
/// assert!(overlaps(&a, &b.clone().at(1.0, 1.0)));
/// ```
pub fn overlaps<A, B>(a: &Placed<A>, b: &Placed<B>) -> bool {
    overlaps_at(a, b, now())
}

/// [`overlaps`] at `now`.
pub fn overlaps_at<A, B>(a: &Placed<A>, b: &Placed<B>, now: Duration) -> bool {
    let (ra, rb) = (a.resolve(now, false), b.resolve(now, false));
    let x0 = ra.x.max(rb.x);
    let y0 = ra.y.max(rb.y);
    let x1 = (ra.x + ra.raster.width as i32).min(rb.x + rb.raster.width as i32);
    let y1 = (ra.y + ra.raster.height as i32).min(rb.y + rb.raster.height as i32);
    (y0..y1).any(|y| {
        (x0..x1)
            .any(|x| ra.raster.opaque(x - ra.x, y - ra.y) && rb.raster.opaque(x - rb.x, y - rb.y))
    })
}

/// Whether the rectangles `a` and `b` cover meet, on the app's clock
/// (cheaper than [`overlaps`], and blind to transparent cells).
pub fn bounds_overlap<A, B>(a: &Placed<A>, b: &Placed<B>) -> bool {
    let now = now();
    a.bounds(now).intersects(&b.bounds(now))
}

type Click<K> = Box<dyn FnMut(K, &mut Ctx)>;
type Hover<K> = Box<dyn FnMut(Option<K>, &mut Ctx)>;

/// How a [`stage_with`] draws, and what its sprites do when clicked or
/// pointed at.
pub struct StageOptions<K> {
    backdrop: Option<Node>,
    half_cells: bool,
    on_click: Option<Click<K>>,
    on_hover: Option<Hover<K>>,
}

impl<K> Default for StageOptions<K> {
    fn default() -> Self {
        StageOptions {
            backdrop: None,
            half_cells: false,
            on_click: None,
            on_hover: None,
        }
    }
}

impl<K> StageOptions<K> {
    pub fn new() -> StageOptions<K> {
        StageOptions::default()
    }

    /// Draw the sprites over `node`, laid out over the whole stage: a
    /// [`fill`](crate::fill), a picture, a map, a board. Transparent cells
    /// show it.
    pub fn backdrop(mut self, node: Node) -> StageOptions<K> {
        self.backdrop = Some(node);
        self
    }

    /// Place pixel-art sprites ([`Sprite::from_pixels`]) by half cells up
    /// and down: a y of 2.5 is a pixel lower than 2. Cell art still goes
    /// to the nearest cell.
    pub fn half_cells(mut self, on: bool) -> StageOptions<K> {
        self.half_cells = on;
        self
    }

    /// Call `f` with the id of the sprite clicked: the topmost with an
    /// opaque cell under the pointer. A click on no sprite goes on to the
    /// stage's ancestors ([`Node::on_click`]).
    pub fn on_sprite_click(mut self, f: impl FnMut(K, &mut Ctx) + 'static) -> StageOptions<K> {
        self.on_click = Some(Box::new(f));
        self
    }

    /// Call `f` when the pointer comes over another sprite (its id) or off
    /// every sprite (`None`). Turns on the terminal's reports of pointer
    /// movement.
    pub fn on_sprite_hover(
        mut self,
        f: impl FnMut(Option<K>, &mut Ctx) + 'static,
    ) -> StageOptions<K> {
        self.on_hover = Some(Box::new(f));
        self
    }
}

type Sprites<K> = Box<dyn Fn() -> Vec<Placed<K>>>;

/// What [`stage`] makes.
struct Stage<K> {
    sprites: Sprites<K>,
    backdrop: Vec<Node>,
    half_cells: bool,
    on_click: Option<Click<K>>,
    on_hover: Option<Hover<K>>,
    hovered: Option<K>,
    /// What the last draw put where, bottom first.
    drawn: Vec<(K, i32, i32, Rc<Raster>)>,
    /// Written when a frame or a tween is due, to draw again.
    tick: Signal<u64>,
    /// When the stage is due to draw again, already asked for.
    due: Rc<StdCell<Option<Duration>>>,
    /// Backgrounds read from beneath a run, reused.
    beneath: Vec<(u16, Style)>,
}

impl<K: Clone + PartialEq + 'static> Stage<K> {
    /// Copy `raster`'s opaque cells onto the canvas with its top left at
    /// `x`, `y`; a cell with no background takes the one beneath.
    fn composite(&mut self, canvas: &mut Canvas, x: i32, y: i32, raster: &Raster) {
        let (cw, ch) = (canvas.rect.width as i32, canvas.rect.height as i32);
        let (ox, oy) = (canvas.rect.x as i32, canvas.rect.y as i32);
        let (mut left, mut top, mut right, mut bottom) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        let screen = &mut *canvas.screen;
        for &(row, x0, len) in &raster.runs {
            let ty = y + row as i32;
            if ty < 0 || ty >= ch {
                continue;
            }
            let s0 = (x0 as i32).max(-x);
            let s1 = (x0 as i32 + len as i32).min(cw - x);
            if s0 >= s1 {
                continue;
            }
            let (ax, ay) = ((ox + x + s0) as u16, (oy + ty) as u16);
            self.beneath.clear();
            let base = row as usize * raster.width as usize;
            for s in s0..s1 {
                if raster.under[base + s as usize] {
                    let column = ax + (s - s0) as u16;
                    let bg = screen
                        .get(column, ay)
                        .and_then(|cell| screen.style(cell.style))
                        .and_then(|style| style.bgcolor())
                        .cloned();
                    if let Some(bg) = bg {
                        self.beneath
                            .push((column, Style::from_color(None, Some(bg))));
                    }
                }
            }
            screen.blit(
                &raster.cells,
                Rect::new(s0 as u16, row, (s1 - s0) as u16, 1),
                ax,
                ay,
            );
            for (column, style) in &self.beneath {
                screen.underlay(*column, ay, style);
            }
            left = left.min(ax as i32);
            right = right.max(ax as i32 + s1 - s0);
            top = top.min(ay as i32);
            bottom = bottom.max(ay as i32 + 1);
        }
        if left < right {
            canvas.written.push(Rect::new(
                left as u16,
                top as u16,
                (right - left) as u16,
                (bottom - top) as u16,
            ));
        }
    }

    /// The topmost sprite with an opaque cell at `x`, `y` of the stage.
    fn hit(&self, x: u16, y: u16) -> Option<K> {
        self.drawn
            .iter()
            .rev()
            .find(|(_, px, py, raster)| raster.opaque(x as i32 - px, y as i32 - py))
            .map(|(id, ..)| id.clone())
    }

    /// Ask to draw again at `due`, unless an earlier draw is asked for.
    fn wake_at(&self, due: Duration, now: Duration) {
        if self
            .due
            .get()
            .is_some_and(|pending| pending <= due && pending > now)
        {
            return;
        }
        self.due.set(Some(due));
        let (pending, tick) = (self.due.clone(), self.tick);
        crate::app::after(due - now, move |_| {
            if pending.get() == Some(due) {
                pending.set(None);
            }
            tick.update(|t| *t = t.wrapping_add(1));
        });
    }
}

impl<K: Clone + PartialEq + 'static> Widget for Stage<K> {
    fn name(&self) -> &'static str {
        "stage"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{} sprites", self.drawn.len()))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        match self.backdrop.first() {
            Some(backdrop) => cx.measure(backdrop, axis, width, height),
            None => match axis {
                Axis::Horizontal => width,
                Axis::Vertical => height,
            },
        }
    }

    fn children(&self) -> &[Node] {
        &self.backdrop
    }

    fn layout(&mut self, _cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        vec![rect; self.backdrop.len()]
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn draw_over(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        self.tick.get();
        if self.on_hover.is_some() {
            cx.report_movement();
        }
        let now = cx.now();
        let mut placed = (self.sprites)();
        placed.sort_by_key(|p| p.z);
        self.drawn.clear();
        let mut wake: Option<Duration> = None;
        for p in &placed {
            let r = p.resolve(now, self.half_cells);
            self.composite(canvas, r.x, r.y, &r.raster);
            if p.tween.is_some_and(|t| !t.done(now)) {
                wake = Some(wake.map_or(now + FRAME, |w| w.min(now + FRAME)));
            }
            if p.frame.is_none() {
                if let Some(left) = p.sprite.next_change(now.saturating_sub(p.start)) {
                    let at = now.max(p.start) + left;
                    wake = Some(wake.map_or(at, |w| w.min(at)));
                }
            }
            self.drawn.push((p.id.clone(), r.x, r.y, r.raster));
        }
        if let Some(due) = wake {
            self.wake_at(due, now);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(_) => {
                    let Some(id) = self.hit(mouse.column, mouse.row) else {
                        return Used::No;
                    };
                    match self.on_click.as_mut() {
                        Some(f) => {
                            f(id, cx.app());
                            Used::Yes
                        }
                        None => Used::No,
                    }
                }
                MouseKind::Moved => {
                    let over = self.hit(mouse.column, mouse.row);
                    if over != self.hovered {
                        self.hovered = over.clone();
                        if let Some(f) = self.on_hover.as_mut() {
                            f(over, cx.app());
                        }
                    }
                    Used::No
                }
                _ => Used::No,
            },
            WidgetEvent::Hover(false) if self.hovered.is_some() => {
                self.hovered = None;
                if let Some(f) = self.on_hover.as_mut() {
                    f(None, cx.app());
                }
                Used::No
            }
            _ => Used::No,
        }
    }
}

/// A node that draws the sprites `sprites` places, clipped to it, in z
/// order, over a blank stage (give it a background with [`Node::bg`]).
/// `sprites` may read signals: the stage draws again when they change,
/// and on the app's clock while an animation or a [`Tween`] runs. Call it
/// while the app is built or running. [`stage_with`] adds a backdrop,
/// half cells, and clicks and hovers on sprites.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::sprite::{stage, Placed, Sprite};
/// use std::time::Duration;
///
/// let app = App::new(|| {
///     let spinner = Sprite::from_frames(["|", "/", "-", "+"].map(|s| Sprite::from_markup(&[s], ' ')))
///         .period(Duration::from_millis(100));
///     stage(move || vec![Placed::new(0, spinner.clone()).at(1.0, 0.0)])
/// });
/// let mut driver = app.driver(3, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0], " | ");
/// driver.advance(Duration::from_millis(250));
/// assert_eq!(driver.screen().plain()[0], " - ");
/// ```
pub fn stage<K: Clone + PartialEq + 'static>(
    sprites: impl Fn() -> Vec<Placed<K>> + 'static,
) -> Node {
    stage_with(sprites, StageOptions::new())
}

/// [`stage`] with `options`: a backdrop under the sprites, half-cell
/// positions, and handlers for clicks and hovers on sprites.
pub fn stage_with<K: Clone + PartialEq + 'static>(
    sprites: impl Fn() -> Vec<Placed<K>> + 'static,
    options: StageOptions<K>,
) -> Node {
    Node::from_widget(
        Box::new(Stage {
            sprites: Box::new(sprites),
            backdrop: options.backdrop.into_iter().collect(),
            half_cells: options.half_cells,
            on_click: options.on_click,
            on_hover: options.on_hover,
            hovered: None,
            drawn: Vec::new(),
            tick: signal(0),
            due: Rc::default(),
            beneath: Vec::new(),
        }),
        "stage",
    )
}

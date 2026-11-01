//! An animated banner: rs-rich-art's [`AnimatedBanner`] played on the
//! app's clock.
//!
//! A [`Play`] is the clock of one run of an animation: it starts when
//! something first reads it, runs for the animation's length (once, or
//! [looping](Play::looping)), and then holds still. [`banner`] draws an
//! [`AnimatedBanner`]'s frame at the play's time; anything else that
//! should keep time with it (a light that pulses with the glow, a sprite
//! that swims in) reads the same play's [`elapsed`](Play::elapsed) or
//! [`progress`](Play::progress), and draws again with each frame. While it
//! runs, a frame is asked for every [`FRAME`]; once still, nothing wakes
//! the app for it.
//!
//! There is no motion, only the still final frame, when
//! [`a11y::motion_allowed`](crate::a11y::motion_allowed) says so (reduced
//! motion or no animation in `RICH_A11Y`, `RICH_ANIMATION=0`, `NO_COLOR`)
//! and in text mode. [`Play::skip_on_key`] makes any key jump to the end.
//!
//! ```
//! use std::time::Duration;
//!
//! use intuituive::prelude::*;
//! use intuituive::banner::{banner, Play};
//! use rich_art::animate::AnimatedBanner;
//! use rich_art::FigletFont;
//!
//! let app = App::new(|| {
//!     let art = AnimatedBanner::new("Hi", &FigletFont::standard());
//!     let play = Play::new(art.duration_of()).motion(true);
//!     play.skip_on_key(banner(art, play.clone()))
//! });
//! let mut driver = app.driver(12, 5);
//! driver.settle();
//! assert_eq!(driver.screen().plain()[1].trim_end(), "| | | (_)");
//! ```

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use rich::Style;
use rich::color::Color;
use rich_art::animate::{AnimatedBanner, BannerPalette, Rgb};

use crate::node::{Axis, Node};
use crate::reactive::{Signal, signal};
use crate::widget::{Canvas, DrawCx, MeasureCx, Widget};

/// How often a running play asks for a frame: thirty a second.
pub const FRAME: Duration = Duration::from_millis(33);

/// How long a looping play holds its last frame before it starts again.
pub const LOOP_HOLD: Duration = Duration::from_secs(1);

struct PlayState {
    duration: Duration,
    looping: Cell<bool>,
    motion: Cell<bool>,
    /// The app's clock when it was first read.
    start: Cell<Option<Duration>>,
    /// Written when the next frame is due.
    tick: Signal<u64>,
    skipped: Signal<bool>,
    /// A timer for the next frame is set.
    waiting: Cell<bool>,
}

/// The clock of one run of an animation. Cloning it shares the run. See
/// the [module docs](self).
#[derive(Clone)]
pub struct Play(Rc<PlayState>);

impl Play {
    /// A run of `duration`, played once, with motion as
    /// [`a11y::motion_allowed`](crate::a11y::motion_allowed) says.
    ///
    /// # Panics
    /// Outside a running or building app.
    pub fn new(duration: Duration) -> Play {
        Play(Rc::new(PlayState {
            duration,
            looping: Cell::new(false),
            motion: Cell::new(crate::a11y::motion_allowed()),
            start: Cell::new(None),
            tick: signal(0),
            skipped: signal(false),
            waiting: Cell::new(false),
        }))
    }

    /// Whether it plays again after holding its last frame for
    /// [`LOOP_HOLD`], until [skipped](Self::skip).
    #[must_use]
    pub fn looping(self, on: bool) -> Play {
        self.0.looping.set(on);
        self
    }

    /// Whether it moves at all, whatever the environment says: `false`
    /// holds the still frame from the start.
    #[must_use]
    pub fn motion(self, on: bool) -> Play {
        self.0.motion.set(on);
        self
    }

    /// The run's length.
    pub fn duration(&self) -> Duration {
        self.0.duration
    }

    /// Time into the run, from zero to its length (the length once still).
    /// Reading it in a draw draws again with the next frame.
    pub fn elapsed(&self) -> Duration {
        let state = &self.0;
        if state.skipped.get() || !state.motion.get() || crate::a11y::text_mode() {
            return state.duration;
        }
        state.tick.get();
        let now = crate::sprite::now();
        let start = state.start.get().unwrap_or(now);
        state.start.set(Some(start));
        let mut at = now.saturating_sub(start);
        if state.looping.get() {
            let cycle = (state.duration + LOOP_HOLD).as_nanos().max(1);
            at = Duration::from_nanos((at.as_nanos() % cycle) as u64);
        } else if at >= state.duration {
            return state.duration;
        }
        if !state.waiting.replace(true) {
            let this = self.clone();
            crate::app::after(FRAME, move |_| {
                this.0.waiting.set(false);
                this.0.tick.update(|t| *t = t.wrapping_add(1));
            });
        }
        at.min(state.duration)
    }

    /// How far into the run, from 0 to 1 (1 once still).
    pub fn progress(&self) -> f64 {
        let length = self.0.duration.as_secs_f64();
        if length <= 0.0 {
            return 1.0;
        }
        (self.elapsed().as_secs_f64() / length).clamp(0.0, 1.0)
    }

    /// Whether it holds still now: done, skipped, or with no motion.
    pub fn is_still(&self) -> bool {
        self.elapsed() >= self.0.duration && !self.0.looping.get()
            || self.0.skipped.get()
            || !self.0.motion.get()
    }

    /// Jump to the end and hold still (a loop stops).
    pub fn skip(&self) {
        if !self.0.skipped.get_untracked() {
            self.0.skipped.set(true);
        }
    }

    /// Play from the start again.
    pub fn restart(&self) {
        self.0.start.set(None);
        self.0.skipped.set(false);
        self.0.tick.update(|t| *t = t.wrapping_add(1));
    }

    /// `node`, where any key first [skips](Self::skip) to the end; the key
    /// then goes on to do what it does.
    pub fn skip_on_key(&self, node: Node) -> Node {
        let play = self.clone();
        node.previews_keys().on_any_key(move |_, _| {
            play.skip();
            false
        })
    }
}

/// What [`banner`] draws.
struct Banner {
    art: AnimatedBanner,
    play: Play,
    themed: Option<AnimatedBanner>,
}

/// `art` at `play`'s time: its frame while the play runs, its still frame
/// after. Colours come from the art's palette, each replaced by the
/// theme's `banner.dark`, `banner.lit`, `banner.lit-end`, `banner.crest`,
/// `banner.wave` or `banner.speck` style's colour where the theme has one.
/// Sized [`Auto`](crate::Size::Auto), it is as big as the art; what does
/// not fit is cut. Its accessible name is `name` given with
/// [`label`](Node::label), else nothing: label it with the text it shows.
pub fn banner(art: AnimatedBanner, play: Play) -> Node {
    crate::widget::widget(Banner {
        art,
        play,
        themed: None,
    })
    .auto()
}

/// The theme's colour for `name`, if it sets one.
fn theme_colour(cx: &DrawCx, name: &str) -> Option<Rgb> {
    let style = cx.style(name, "");
    let t = style.color()?.get_truecolor()?;
    Some([t.red, t.green, t.blue])
}

impl Widget for Banner {
    fn name(&self) -> &'static str {
        "banner"
    }

    fn measure(&mut self, _cx: &MeasureCx, axis: Axis, _width: u16, _height: u16) -> u16 {
        let (w, h) = self.art.size();
        let n = match axis {
            Axis::Horizontal => w,
            Axis::Vertical => h,
        };
        n.min(u16::MAX as usize) as u16
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let art = self.themed.get_or_insert_with(|| {
            let mut palette: BannerPalette = self.art.palette_of();
            for (name, slot) in [
                ("banner.dark", &mut palette.dark),
                ("banner.lit", &mut palette.lit),
                ("banner.lit-end", &mut palette.lit_end),
                ("banner.crest", &mut palette.crest),
                ("banner.wave", &mut palette.wave),
                ("banner.speck", &mut palette.speck),
            ] {
                if let Some(rgb) = theme_colour(cx, name) {
                    *slot = rgb;
                }
            }
            self.art.clone().palette(palette)
        });
        let frame = art.frame(self.play.elapsed());
        let text = crate::a11y::text_mode();
        for (y, row) in frame.rows().iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                if cell.ch == ' ' {
                    continue;
                }
                let style = match cell.fg {
                    Some([r, g, b]) if !text => {
                        Some(Style::new().with_color(Color::from_rgb(r, g, b)))
                    }
                    _ => None,
                };
                canvas.set(x as u16, y as u16, cell.ch, style.as_ref());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use rich_art::FigletFont;

    fn art() -> AnimatedBanner {
        AnimatedBanner::new("Hi", &FigletFont::standard())
    }

    fn fg(driver: &crate::Driver, x: u16, y: u16) -> Option<Color> {
        let screen = driver.screen();
        screen
            .style(screen.cell(x, y).style)
            .and_then(|s| s.color().cloned())
    }

    fn rgb(c: Rgb) -> Option<Color> {
        Some(Color::from_rgb(c[0], c[1], c[2]))
    }

    #[test]
    fn it_plays_on_the_clock_then_holds_the_still_frame() {
        let app = App::new(|| {
            let play = Play::new(Duration::from_secs(2)).motion(true);
            banner(art(), play)
        });
        let mut driver = app.driver(12, 5);
        driver.settle();
        let palette = BannerPalette::default();
        // At the start the letters are dark.
        assert_eq!(fg(&driver, 0, 1), rgb(palette.dark));
        assert_eq!(driver.screen().plain()[1].trim_end(), "| | | (_)");
        driver.advance(Duration::from_millis(1000));
        let middle = fg(&driver, 0, 1);
        assert_ne!(middle, rgb(palette.dark));
        driver.advance(Duration::from_millis(1100));
        assert_eq!(fg(&driver, 0, 1), rgb(palette.lit));
        // Still: no more frames are asked for.
        let frames = driver.stats().frames;
        driver.advance(Duration::from_secs(1));
        assert_eq!(driver.stats().frames, frames);
    }

    #[test]
    fn any_key_skips_to_the_still_frame() {
        let app = App::new(|| {
            let play = Play::new(Duration::from_secs(2)).motion(true);
            play.skip_on_key(banner(art(), play.clone()))
        });
        let mut driver = app.driver(12, 5);
        driver.settle();
        assert_eq!(fg(&driver, 0, 1), rgb(BannerPalette::default().dark));
        driver.press("x");
        assert_eq!(fg(&driver, 0, 1), rgb(BannerPalette::default().lit));
    }

    #[test]
    fn with_no_motion_it_is_still_from_the_start() {
        let app = App::new(|| banner(art(), Play::new(Duration::from_secs(2)).motion(false)));
        let mut driver = app.driver(12, 5);
        driver.settle();
        assert_eq!(fg(&driver, 0, 1), rgb(BannerPalette::default().lit));
        // In text mode too, with no colour.
        let app = App::new(|| banner(art(), Play::new(Duration::from_secs(2)).motion(true)))
            .accessible(true);
        let mut driver = app.driver(12, 5);
        driver.settle();
        assert_eq!(driver.screen().plain()[1].trim_end(), "| | | (_)");
        assert_eq!(fg(&driver, 0, 1), None);
    }

    #[test]
    fn a_looping_play_starts_again() {
        let app = App::new(|| {
            banner(
                art(),
                Play::new(Duration::from_secs(1)).looping(true).motion(true),
            )
        });
        let mut driver = app.driver(12, 5);
        driver.settle();
        let dark = rgb(BannerPalette::default().dark);
        assert_eq!(fg(&driver, 0, 1), dark);
        driver.advance(Duration::from_millis(1500));
        assert_ne!(fg(&driver, 0, 1), dark);
        driver.advance(Duration::from_millis(520));
        assert_eq!(fg(&driver, 0, 1), dark);
    }
}

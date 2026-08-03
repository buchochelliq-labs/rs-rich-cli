//! Several animations playing at once.
//!
//! A [`Stage`] holds any number of [`AnimatedArt`]s side by side and plays them
//! **simultaneously**, each on its own clock — a GIF with 40ms frames and one
//! with 200ms frames both run at their own speed, and the stage redraws only
//! when something actually changed.
//!
//! Requires the non-default `gif` feature.
//!
//! ```no_run
//! use rich::Console;
//! use rich_art::{AnimatedArt, Stage};
//!
//! // A GIF embedded in the binary as an asset — nothing read at runtime.
//! static CAT: &[u8] = include_bytes!("../examples/assets/cat.gif");
//!
//! let stage = Stage::new()
//!     .with(AnimatedArt::from_bytes(CAT)?.width(30).color(true))
//!     .with(AnimatedArt::from_path("other.gif")?.width(30).color(true));
//! stage.play_stdout(Console::builder().build())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

use std::io::Write;
use std::time::{Duration, Instant};

use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;
use rich::Live;

use crate::ascii::AsciiArt;
use crate::gif::AnimatedArt;

/// How long a stage keeps playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Until {
    /// Stop once every animation has finished its own repeat count. An
    /// animation set to [`Repeat::Forever`] never finishes, so the stage runs
    /// until interrupted.
    #[default]
    AllFinished,
    /// Stop after a fixed wall-clock duration, whatever the animations are
    /// doing.
    Elapsed(Duration),
}

/// One animation on the stage, with its own playback position and clock.
struct Track {
    art: AnimatedArt,
    /// Index of the frame currently displayed.
    frame: usize,
    /// How many complete passes it has made.
    passes: usize,
    /// When the current frame should be replaced.
    next_change: Instant,
    /// Set once the track has played out its repeat count.
    finished: bool,
}

impl Track {
    fn new(art: AnimatedArt, start: Instant) -> Self {
        let delay = art.frame_delay(0).unwrap_or(Duration::from_millis(100));
        Track {
            art,
            frame: 0,
            passes: 0,
            next_change: start + delay,
            finished: false,
        }
    }

    /// Advance if this track's frame is due at `now`. Returns whether the
    /// displayed frame changed.
    fn tick(&mut self, now: Instant) -> bool {
        if self.finished || now < self.next_change {
            return false;
        }
        let count = self.art.frame_count();
        if count == 0 {
            self.finished = true;
            return false;
        }
        self.frame += 1;
        if self.frame >= count {
            self.frame = 0;
            self.passes += 1;
            if let Some(total) = self.art.passes() {
                if self.passes >= total {
                    // Hold the last frame rather than snapping back to the first.
                    self.frame = count - 1;
                    self.finished = true;
                    return false;
                }
            }
        }
        let delay = self
            .art
            .frame_delay(self.frame)
            .unwrap_or(Duration::from_millis(100));
        self.next_change = now + delay;
        true
    }

    fn current(&self) -> Option<AsciiArt> {
        self.art.frame(self.frame)
    }
}

/// Several animations playing together, side by side.
#[derive(Default)]
pub struct Stage {
    items: Vec<AnimatedArt>,
    gap: usize,
    until: Until,
}

impl Stage {
    /// An empty stage.
    pub fn new() -> Self {
        Stage {
            items: Vec::new(),
            gap: 2,
            until: Until::AllFinished,
        }
    }

    /// Add an animation. They are laid out left to right in the order added.
    pub fn with(mut self, art: AnimatedArt) -> Self {
        self.items.push(art);
        self
    }

    /// Columns of blank space between animations (default 2).
    pub fn gap(mut self, gap: usize) -> Self {
        self.gap = gap;
        self
    }

    /// When to stop (default: when every animation has finished).
    pub fn until(mut self, until: Until) -> Self {
        self.until = until;
        self
    }

    /// How many animations are on the stage.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the stage has no animations.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Play every animation at once, redrawing in place.
    ///
    /// Each animation keeps its own frame clock, so mixed frame rates stay
    /// correct. The stage sleeps until the *next* animation is due rather than
    /// polling, and only redraws when a frame actually changed.
    ///
    /// The cursor is hidden during playback and restored on return; see
    /// [`AnimatedArt::play`] for the interrupt caveat.
    pub fn play<W: Write>(&self, console: Console, mut writer: W) -> std::io::Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        let start = Instant::now();
        let mut tracks: Vec<Track> = self
            .items
            .iter()
            .map(|art| Track::new(art.clone(), start))
            .collect();

        let mut live = Live::new(Box::new(self.compose(&tracks)), console, &mut writer);
        live.start();

        loop {
            // Stop conditions.
            match self.until {
                Until::Elapsed(limit) if start.elapsed() >= limit => break,
                Until::AllFinished if tracks.iter().all(|t| t.finished) => break,
                _ => {}
            }

            // Sleep until the next track is due (bounded so an `Elapsed` limit
            // is still honoured promptly when everything has settled).
            let now = Instant::now();
            let next = tracks
                .iter()
                .filter(|t| !t.finished)
                .map(|t| t.next_change)
                .min();
            let mut wait = match next {
                Some(at) => at.saturating_duration_since(now),
                None => Duration::from_millis(20),
            };
            if let Until::Elapsed(limit) = self.until {
                let remaining = limit.saturating_sub(start.elapsed());
                wait = wait.min(remaining);
                if remaining.is_zero() {
                    break;
                }
            }
            if !wait.is_zero() {
                std::thread::sleep(wait);
            }

            let now = Instant::now();
            let mut changed = false;
            for track in &mut tracks {
                changed |= track.tick(now);
            }
            if changed {
                live.update(Box::new(self.compose(&tracks)));
            }
        }

        live.stop();
        writer.flush()
    }

    /// Play to stdout.
    pub fn play_stdout(&self, console: Console) -> std::io::Result<()> {
        let stdout = std::io::stdout();
        let handle = stdout.lock();
        self.play(console, handle)
    }

    /// The stage's current appearance: every track's current frame, side by
    /// side.
    fn compose(&self, tracks: &[Track]) -> Row {
        Row {
            cells: tracks.iter().filter_map(Track::current).collect(),
            gap: self.gap,
        }
    }
}

/// Lays renderables out horizontally, padding each to its own width and each
/// column block to the tallest. Used to place the animations side by side.
struct Row {
    cells: Vec<AsciiArt>,
    gap: usize,
}

impl Renderable for Row {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if self.cells.is_empty() {
            return Vec::new();
        }
        // Render each cell to its own block of lines.
        let blocks: Vec<Vec<Vec<Segment>>> = self
            .cells
            .iter()
            .map(|cell| {
                let width = cell.columns(options.max_width);
                let cell_options = options.update_width(width);
                console.render_lines(cell, &cell_options, true)
            })
            .collect();

        let widths: Vec<usize> = self
            .cells
            .iter()
            .map(|cell| cell.columns(options.max_width))
            .collect();
        let height = blocks.iter().map(Vec::len).max().unwrap_or(0);

        let mut out = Vec::new();
        for row in 0..height {
            for (index, block) in blocks.iter().enumerate() {
                if index > 0 && self.gap > 0 {
                    out.push(Segment::new(" ".repeat(self.gap), None));
                }
                match block.get(row) {
                    Some(line) => out.extend(line.iter().cloned()),
                    // This cell is shorter than the tallest — pad with blanks so
                    // the ones to its right stay aligned.
                    None => out.push(Segment::new(" ".repeat(widths[index]), None)),
                }
            }
            if row + 1 != height {
                out.push(Segment::line());
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif::tests_support::make_gif;
    use crate::gif::Repeat;

    #[test]
    fn a_stage_composes_animations_side_by_side() {
        let a = AnimatedArt::from_bytes(&make_gif(&[[0, 0, 0]], 50))
            .unwrap()
            .width(4)
            .height(1);
        let b = AnimatedArt::from_bytes(&make_gif(&[[255, 255, 255]], 50))
            .unwrap()
            .width(4)
            .height(1);
        let stage = Stage::new().with(a).with(b).gap(1);
        assert_eq!(stage.len(), 2);

        let console = Console::builder().width(40).build();
        let start = Instant::now();
        let tracks: Vec<Track> = stage
            .items
            .iter()
            .map(|art| Track::new(art.clone(), start))
            .collect();
        let text = console.render_to_string(&stage.compose(&tracks));
        // Dark cell, a one-column gap, then the bright cell — on one line.
        let ramp: Vec<char> = crate::ascii::DEFAULT_RAMP.chars().collect();
        let dark = ramp[0];
        let bright = ramp[ramp.len() - 1];
        assert_eq!(
            text.lines().next().unwrap(),
            format!("{dark}{dark}{dark}{dark} {bright}{bright}{bright}{bright}")
        );
    }

    #[test]
    fn tracks_advance_on_their_own_clocks() {
        // 50ms frames vs 200ms frames: the fast one advances four times as often.
        let fast = AnimatedArt::from_bytes(&make_gif(&[[0, 0, 0], [255, 255, 255]], 50))
            .unwrap()
            .repeat(Repeat::Forever);
        let slow = AnimatedArt::from_bytes(&make_gif(&[[0, 0, 0], [255, 255, 255]], 200))
            .unwrap()
            .repeat(Repeat::Forever);

        let start = Instant::now();
        let mut fast_track = Track::new(fast, start);
        let mut slow_track = Track::new(slow, start);

        // Step forward 200ms in 50ms increments.
        let mut fast_changes = 0;
        let mut slow_changes = 0;
        for step in 1..=4 {
            let now = start + Duration::from_millis(50 * step);
            if fast_track.tick(now) {
                fast_changes += 1;
            }
            if slow_track.tick(now) {
                slow_changes += 1;
            }
        }
        assert_eq!(fast_changes, 4, "50ms track should advance every step");
        assert_eq!(slow_changes, 1, "200ms track should advance once");
    }

    #[test]
    fn a_finished_track_holds_its_last_frame() {
        let art = AnimatedArt::from_bytes(&make_gif(&[[0, 0, 0], [255, 255, 255]], 10))
            .unwrap()
            .repeat(Repeat::Times(1));
        let start = Instant::now();
        let mut track = Track::new(art, start);
        // Advance well past the end.
        for step in 1..=10 {
            track.tick(start + Duration::from_millis(10 * step));
        }
        assert!(track.finished);
        assert_eq!(track.frame, 1, "holds the final frame, not frame 0");
    }

    #[test]
    fn an_empty_stage_plays_nothing() {
        let console = Console::builder().width(20).build();
        let mut out = Vec::new();
        Stage::new().play(console, &mut out).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn playback_of_several_animations_terminates_and_restores_the_cursor() {
        let stage = Stage::new()
            .with(
                AnimatedArt::from_bytes(&make_gif(&[[0, 0, 0], [255, 255, 255]], 10))
                    .unwrap()
                    .width(4)
                    .height(1),
            )
            .with(
                AnimatedArt::from_bytes(&make_gif(&[[128, 128, 128], [0, 0, 0]], 10))
                    .unwrap()
                    .width(4)
                    .height(1),
            )
            .until(Until::Elapsed(Duration::from_millis(120)));

        let console = Console::builder().width(40).build();
        let mut out = Vec::new();
        stage.play(console, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("\x1b[?25l"), "hides the cursor");
        assert!(text.ends_with("\x1b[?25h"), "restores the cursor");
    }
}

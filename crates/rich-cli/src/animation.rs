//! `--gif` animation controls: `--speed`, `--fps`, `--max-frames` and
//! `--max-duration`, next to the existing `--loop` (0.0.19, #708). Not
//! upstream: a binary-boundary convenience (see `docs/PORTING.md`) over
//! rs-rich-art's public `AnimatedArt` and `Stage`, which stay as they are.
//!
//! Without `--speed` or `--max-frames`, playback is rs-rich-art's `Stage`
//! (with `--fps` as its frame-rate cap and `--max-duration` as its
//! `Until::Elapsed`). Those two need a clock of their own, so the small
//! player below drives the same frames through `rich::Live` with each delay
//! divided by the speed, and stops each animation after its frame budget.
//! Off a terminal both paths print the first frame once.

// Without the `art` feature there is no GIF to play.
#![cfg_attr(not(feature = "art"), allow(dead_code))]

use std::time::Duration;

/// The default frame-rate cap: colour art is byte-heavy.
pub(crate) const DEFAULT_FPS: f64 = 30.0;

/// The animation flags of the main parser.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct AnimationControls {
    /// `--speed X`: playback speed, 2 = twice as fast.
    pub(crate) speed: Option<f64>,
    /// `--fps N`: the frame-rate cap (default 30).
    pub(crate) fps: Option<f64>,
    /// `--max-frames N`: show at most N frames of each animation.
    pub(crate) max_frames: Option<usize>,
    /// `--max-duration SECS`: stop after this long, whatever is playing.
    pub(crate) max_duration: Option<Duration>,
}

/// The bounds of each number, so every delay derived from them is a
/// `Duration` that exists: `(flag, at least, at most)`.
const RANGES: [(&str, f64, f64); 3] = [
    ("--speed", 0.001, 1000.0),
    ("--fps", 0.01, 1000.0),
    ("--max-duration", 0.001, 1e9),
];

/// A finite number in `flag`'s range.
fn positive(flag: &str, value: &str) -> Result<f64, String> {
    let (_, min, max) = RANGES
        .iter()
        .copied()
        .find(|(name, ..)| *name == flag)
        .unwrap_or((flag, f64::MIN_POSITIVE, 1e9));
    match value.trim().parse::<f64>() {
        Ok(number) if number.is_finite() && number >= min && number <= max => Ok(number),
        _ => Err(format!(
            "{flag} takes a number from {min} to {max}, not {value:?}"
        )),
    }
}

impl AnimationControls {
    /// Record `flag`'s `value`. `flag` is one of the four this owns.
    pub(crate) fn set(&mut self, flag: &str, value: &str) -> Result<(), String> {
        match flag {
            "--speed" => self.speed = Some(positive(flag, value)?),
            "--fps" => self.fps = Some(positive(flag, value)?),
            "--max-frames" => match value.trim().parse::<usize>() {
                Ok(n) if n >= 1 => self.max_frames = Some(n),
                _ => return Err(format!("--max-frames takes a count from 1, not {value:?}")),
            },
            "--max-duration" => {
                self.max_duration = Some(Duration::from_secs_f64(positive(flag, value)?))
            }
            other => return Err(format!("unknown animation option {other}")),
        }
        Ok(())
    }

    /// The frame-rate cap to apply.
    pub(crate) fn fps(&self) -> f64 {
        self.fps.unwrap_or(DEFAULT_FPS)
    }

    /// Whether playback needs the CLI's own player rather than `Stage`.
    pub(crate) fn needs_player(&self) -> bool {
        self.speed.is_some_and(|speed| speed != 1.0) || self.max_frames.is_some()
    }

    /// How long frame `delay` shows: divided by the speed, then held for at
    /// least one frame at the cap.
    pub(crate) fn scaled(&self, delay: Duration) -> Duration {
        // The flags are bounded; a delay that still cannot be one holds.
        let seconds = |value: f64| Duration::try_from_secs_f64(value).unwrap_or(Duration::MAX);
        let speed = self.speed.unwrap_or(1.0);
        let floor = seconds(1.0 / self.fps());
        seconds(delay.as_secs_f64() / speed).max(floor)
    }
}

#[cfg(feature = "art")]
pub(crate) use player::play;

#[cfg(feature = "art")]
mod player {
    use super::AnimationControls;
    use rich::console::{Console, ConsoleOptions};
    use rich::protocol::Renderable;
    use rich::segment::Segment;
    use rich::Live;
    use rich_art::gif::GifFrame;
    use rich_art::AnimatedArt;
    use std::io::Write;
    use std::time::{Duration, Instant};

    /// A GIF's own delay when it has none to give.
    const FALLBACK_DELAY: Duration = Duration::from_millis(100);

    /// One animation and its own clock.
    pub(super) struct Track {
        art: AnimatedArt,
        frame: usize,
        /// Frames shown so far, the first included.
        shown: usize,
        passes: usize,
        next_change: Instant,
        finished: bool,
    }

    impl Track {
        fn delay(&self, controls: &AnimationControls) -> Duration {
            controls.scaled(self.art.frame_delay(self.frame).unwrap_or(FALLBACK_DELAY))
        }

        /// Advance when due; whether the shown frame changed.
        fn tick(
            &mut self,
            now: Instant,
            passes: Option<usize>,
            controls: &AnimationControls,
        ) -> bool {
            if self.finished || now < self.next_change {
                return false;
            }
            let count = self.art.frame_count();
            if count == 0 || controls.max_frames.is_some_and(|limit| self.shown >= limit) {
                self.finished = true;
                return false;
            }
            self.frame += 1;
            if self.frame >= count {
                self.frame = 0;
                self.passes += 1;
                if passes.is_some_and(|total| self.passes >= total) {
                    // Hold the last frame, as `Stage` does.
                    self.frame = count - 1;
                    self.finished = true;
                    return false;
                }
            }
            self.shown += 1;
            self.next_change = now + self.delay(controls);
            true
        }
    }

    /// The animations' current frames side by side, `gap` columns apart,
    /// each block padded to its own width and to the tallest.
    struct Row {
        cells: Vec<GifFrame>,
        gap: usize,
    }

    impl Renderable for Row {
        fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
            let blocks: Vec<Vec<Vec<Segment>>> = self
                .cells
                .iter()
                .map(|cell| console.render_lines(cell, options, false))
                .collect();
            let widths: Vec<usize> = blocks
                .iter()
                .map(|block| {
                    block
                        .iter()
                        .map(|line| line.iter().map(Segment::cell_length).sum::<usize>())
                        .max()
                        .unwrap_or(0)
                })
                .collect();
            let height = blocks.iter().map(Vec::len).max().unwrap_or(0);
            let mut out = Vec::new();
            for row in 0..height {
                for (index, block) in blocks.iter().enumerate() {
                    if index > 0 && self.gap > 0 {
                        out.push(Segment::new(" ".repeat(self.gap), None));
                    }
                    let line = block.get(row);
                    let used = line.map_or(0, |line| {
                        line.iter().map(Segment::cell_length).sum::<usize>()
                    });
                    if let Some(line) = line {
                        out.extend(line.iter().cloned());
                    }
                    if used < widths[index] {
                        out.push(Segment::new(" ".repeat(widths[index] - used), None));
                    }
                }
                if row + 1 != height {
                    out.push(Segment::line());
                }
            }
            out
        }
    }

    fn compose(tracks: &[Track], gap: usize) -> Row {
        Row {
            cells: tracks
                .iter()
                .filter_map(|track| track.art.render_frame(track.frame))
                .collect(),
            gap,
        }
    }

    /// Where the player gets the time and waits: the real clock, or a test's.
    pub(super) trait Clock {
        fn now(&self) -> Instant;
        fn sleep(&mut self, wait: Duration);
    }

    /// The wall clock.
    struct RealClock;

    impl Clock for RealClock {
        fn now(&self) -> Instant {
            Instant::now()
        }

        fn sleep(&mut self, wait: Duration) {
            std::thread::sleep(wait);
        }
    }

    /// Play `arts` together on a terminal console, honouring `controls`;
    /// `passes` is how many times each repeats (`None` = forever, which
    /// `--max-frames` or `--max-duration` still ends).
    pub(crate) fn play(
        arts: Vec<AnimatedArt>,
        gap: usize,
        passes: Option<usize>,
        controls: &AnimationControls,
        console: Console,
    ) -> std::io::Result<()> {
        if arts.is_empty() {
            return Ok(());
        }
        let mut clock = RealClock;
        let mut tracks = start_tracks(arts, controls, clock.now());
        let stdout = std::io::stdout();
        let mut writer = stdout.lock();
        let mut live = Live::new(Box::new(compose(&tracks, gap)), console, &mut writer);
        live.start();
        drive(&mut tracks, passes, controls, &mut clock, |tracks| {
            live.update(Box::new(compose(tracks, gap)));
        });
        live.stop();
        drop(live);
        writer.flush()
    }

    /// Each animation on its first frame, its first change due after that
    /// frame's delay.
    pub(super) fn start_tracks(
        arts: Vec<AnimatedArt>,
        controls: &AnimationControls,
        start: Instant,
    ) -> Vec<Track> {
        arts.into_iter()
            .map(|art| {
                let mut track = Track {
                    art,
                    frame: 0,
                    shown: 1,
                    passes: 0,
                    next_change: start,
                    finished: false,
                };
                track.next_change = start + track.delay(controls);
                track
            })
            .collect()
    }

    /// Advance the tracks until each has finished or `--max-duration` has
    /// passed, waiting on `clock` and calling `redraw` when a frame changes.
    pub(super) fn drive(
        tracks: &mut [Track],
        passes: Option<usize>,
        controls: &AnimationControls,
        clock: &mut dyn Clock,
        mut redraw: impl FnMut(&[Track]),
    ) {
        let start = clock.now();
        loop {
            let elapsed = clock.now().saturating_duration_since(start);
            if tracks.iter().all(|track| track.finished)
                || controls.max_duration.is_some_and(|limit| elapsed >= limit)
            {
                break;
            }
            let now = clock.now();
            let mut wait = tracks
                .iter()
                .filter(|track| !track.finished)
                .map(|track| track.next_change.saturating_duration_since(now))
                .min()
                .unwrap_or(Duration::from_millis(20));
            if let Some(limit) = controls.max_duration {
                wait = wait.min(limit.saturating_sub(elapsed));
            }
            if !wait.is_zero() {
                clock.sleep(wait);
            }
            let now = clock.now();
            let mut changed = false;
            for track in tracks.iter_mut() {
                changed |= track.tick(now, passes, controls);
            }
            if changed {
                redraw(tracks);
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A clock that moves only when the player sleeps, and keeps each
        /// sleep.
        struct TestClock {
            now: Instant,
            sleeps: Vec<Duration>,
        }

        impl Clock for TestClock {
            fn now(&self) -> Instant {
                self.now
            }

            fn sleep(&mut self, wait: Duration) {
                self.sleeps.push(wait);
                self.now += wait;
            }
        }

        const BALL: &str = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../rich-art/examples/assets/ball.gif"
        );

        /// ball.gif played under `flags`: the sleeps, and the frames drawn.
        fn schedule(flags: &[(&str, &str)], passes: Option<usize>) -> (Vec<Duration>, Vec<usize>) {
            let mut controls = AnimationControls::default();
            for (flag, value) in flags {
                controls.set(flag, value).unwrap();
            }
            let art = AnimatedArt::from_path(BALL).unwrap().width(12);
            let mut clock = TestClock {
                now: Instant::now(),
                sleeps: Vec::new(),
            };
            let mut tracks = start_tracks(vec![art], &controls, clock.now);
            let mut drawn = Vec::new();
            drive(&mut tracks, passes, &controls, &mut clock, |tracks| {
                drawn.push(tracks[0].frame)
            });
            (clock.sleeps, drawn)
        }

        fn ms(n: u64) -> Duration {
            Duration::from_millis(n)
        }

        #[test]
        fn speed_divides_each_frame_delay() {
            // ball.gif's frames are 40 ms. Four frames: three changes.
            let (sleeps, drawn) = schedule(&[("--max-frames", "4"), ("--speed", "0.5")], None);
            assert_eq!(sleeps, vec![ms(80); 4]);
            assert_eq!(drawn, vec![1, 2, 3]);
            let (sleeps, _) = schedule(&[("--max-frames", "4"), ("--speed", "0.8")], None);
            assert_eq!(sleeps, vec![ms(50); 4]);
        }

        #[test]
        fn the_frame_rate_cap_holds_fast_frames() {
            // At speed 8 a frame would last 5 ms; 30 fps holds it 33.3 ms.
            let (sleeps, _) = schedule(&[("--max-frames", "2"), ("--speed", "8")], None);
            assert_eq!(sleeps, vec![Duration::from_secs_f64(1.0 / 30.0); 2]);
        }

        #[test]
        fn max_frames_ends_an_endless_loop() {
            let (sleeps, drawn) = schedule(&[("--max-frames", "3")], None);
            assert_eq!(drawn, vec![1, 2]);
            assert_eq!(sleeps.len(), 3);
        }

        #[test]
        fn max_duration_ends_an_endless_loop_on_time() {
            let (sleeps, _) = schedule(&[("--max-duration", "0.5"), ("--speed", "0.9")], None);
            let total: Duration = sleeps.iter().sum();
            assert_eq!(total, ms(500));
        }

        #[test]
        fn a_finite_loop_holds_the_last_frame() {
            let (_, drawn) = schedule(&[("--speed", "2")], Some(1));
            let count = AnimatedArt::from_path(BALL).unwrap().frame_count();
            assert_eq!(drawn, (1..count).collect::<Vec<_>>());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_validates_each_control() {
        let mut controls = AnimationControls::default();
        controls.set("--speed", "2").unwrap();
        controls.set("--fps", "12.5").unwrap();
        controls.set("--max-frames", "3").unwrap();
        controls.set("--max-duration", "1.5").unwrap();
        assert_eq!(controls.speed, Some(2.0));
        assert_eq!(controls.fps(), 12.5);
        assert_eq!(controls.max_frames, Some(3));
        assert_eq!(controls.max_duration, Some(Duration::from_millis(1500)));
        assert!(controls.needs_player());
        for (flag, bad) in [
            ("--speed", "0"),
            ("--speed", "-1"),
            ("--speed", "inf"),
            ("--fps", "NaN"),
            ("--max-frames", "0"),
            ("--max-frames", "1.5"),
            ("--max-duration", "0"),
            ("--max-duration", "1e300"),
            ("--fps", "1e-320"),
            ("--fps", "1e9"),
            ("--speed", "1e-300"),
            ("--speed", "1e300"),
        ] {
            assert!(controls.set(flag, bad).is_err(), "{flag} {bad}");
        }
    }

    #[test]
    fn delays_scale_with_speed_and_respect_the_cap() {
        let mut controls = AnimationControls::default();
        assert!(!controls.needs_player());
        assert_eq!(controls.fps(), DEFAULT_FPS);
        let delay = Duration::from_millis(100);
        assert_eq!(controls.scaled(delay), delay);
        controls.speed = Some(2.0);
        assert_eq!(controls.scaled(delay), Duration::from_millis(50));
        controls.speed = Some(100.0);
        // Never faster than the 30 fps cap.
        assert_eq!(controls.scaled(delay), Duration::from_secs_f64(1.0 / 30.0));
        controls.speed = Some(0.5);
        assert_eq!(controls.scaled(delay), Duration::from_millis(200));
        controls.speed = Some(1.0);
        assert!(!controls.needs_player());
        // At the bounds, and past them, no delay panics.
        controls.speed = Some(0.001);
        controls.fps = Some(0.01);
        assert_eq!(controls.scaled(delay), Duration::from_secs(100));
        assert_eq!(controls.scaled(Duration::MAX), Duration::MAX);
    }
}

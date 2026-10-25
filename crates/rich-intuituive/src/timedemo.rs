//! A timedemo: replay input against an app as fast as it can draw, and
//! report what each frame cost.
//!
//! Doom's `-timedemo` replays a recorded demo (its inputs only) with no
//! waits and reports frames per second; every Doom port has been compared
//! that way since. [`App::timedemo`] does the same for an intuiTUIve app:
//! a [`Script`] of events and waits runs on the headless [`Driver`]'s
//! virtual clock, so a `wait` of a minute fires a minute of timers and
//! animation frames at once, and each frame's build and encode time,
//! bytes and painter counts are kept. Any app becomes a benchmark scene
//! with no code, and a slow case becomes a report anyone can run again.
//!
//! The input comes from a script built in code, or from a rich-record tape
//! ([`script_from_tape`]): its keys, typing, sleeps and resizes, without
//! the shell or the program it would start.
//!
//! ```
//! use intuituive::interact::headless::Script;
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let n = signal(0);
//!     text!("count {n}").on_key("+", move |_| n.update(|n| *n += 1))
//! });
//! let demo = app.timedemo(Script::new().keys("+ + +"), 20, 1);
//! // The first frame and one a key.
//! assert_eq!(demo.frames.len(), 4);
//! assert!(demo.bytes() > 0);
//! println!("{demo}");
//! ```

use std::fmt;
use std::time::Duration;

use rich_interact::headless::{Script, Step};
use rich_interact::{Event, Key};

use crate::app::App;
use crate::clock::Instant;
use crate::screen::PaintStats;

/// One frame of a [`Timedemo`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DemoFrame {
    /// When it was drawn, on the app's virtual clock.
    pub at: Duration,
    /// Real time the turn took: timers and handlers, the draw and the
    /// encode.
    pub time: Duration,
    /// Real time the widgets took to draw.
    pub draw: Duration,
    /// Real time the painter took to encode.
    pub paint: Duration,
    /// Bytes the frame wrote.
    pub bytes: usize,
    /// The painter's counts.
    pub painted: PaintStats,
}

/// What [`App::timedemo`] measured: every frame drawn, in order.
#[derive(Clone, Debug, Default)]
pub struct Timedemo {
    /// The terminal's size the demo ended at.
    pub size: (u16, u16),
    /// Each frame drawn.
    pub frames: Vec<DemoFrame>,
    /// Real time the whole replay took.
    pub elapsed: Duration,
    /// Virtual time the script covered.
    pub covered: Duration,
}

impl Timedemo {
    /// Bytes over every frame.
    pub fn bytes(&self) -> usize {
        self.frames.iter().map(|f| f.bytes).sum()
    }

    /// The painter's counts over every frame.
    pub fn painted(&self) -> PaintStats {
        let mut total = PaintStats::default();
        for frame in &self.frames {
            total.add(&frame.painted);
        }
        total
    }

    /// Real time over every frame: what drawing them took, without the
    /// replay's own bookkeeping.
    pub fn frame_time(&self) -> Duration {
        self.frames.iter().map(|f| f.time).sum()
    }

    /// The mean time a frame took.
    pub fn mean(&self) -> Duration {
        match self.frames.len() {
            0 => Duration::ZERO,
            n => self.frame_time() / n as u32,
        }
    }

    /// The time `share` of the frames (0.5 the median, 0.99 the 99th
    /// percentile) took at most.
    pub fn percentile(&self, share: f64) -> Duration {
        let mut times: Vec<Duration> = self.frames.iter().map(|f| f.time).collect();
        if times.is_empty() {
            return Duration::ZERO;
        }
        times.sort_unstable();
        let at = ((times.len() - 1) as f64 * share.clamp(0.0, 1.0)).round() as usize;
        times[at]
    }

    /// The slowest frame's time.
    pub fn worst(&self) -> Duration {
        self.frames.iter().map(|f| f.time).max().unwrap_or_default()
    }

    /// Frames a second, as fast as they could be drawn.
    pub fn fps(&self) -> f64 {
        let secs = self.frame_time().as_secs_f64();
        if secs > 0.0 {
            self.frames.len() as f64 / secs
        } else {
            0.0
        }
    }
}

impl fmt::Display for Timedemo {
    /// One line, as Doom prints it, then the counts.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let p = self.painted();
        writeln!(
            f,
            "{} frames in {:.1} ms ({:.0} fps); mean {:.1} µs, p50 {:.1}, p99 {:.1}, \
             worst {:.1} µs, at {}x{}",
            self.frames.len(),
            self.frame_time().as_secs_f64() * 1e3,
            self.fps(),
            self.mean().as_secs_f64() * 1e6,
            self.percentile(0.5).as_secs_f64() * 1e6,
            self.percentile(0.99).as_secs_f64() * 1e6,
            self.worst().as_secs_f64() * 1e6,
            self.size.0,
            self.size.1,
        )?;
        write!(
            f,
            "{} bytes: {} moves, {} styles, {} text, {} other; \
             cells {} compared, {} written, {} elided",
            self.bytes(),
            p.move_bytes,
            p.style_bytes,
            p.text_bytes,
            p.other_bytes,
            p.compared,
            p.written,
            p.elided,
        )
    }
}

/// The shortest step the virtual clock takes while something keeps asking
/// to be drawn: a frame at 1,000 fps, so an app that animates without a
/// timer still moves on.
const MIN_STEP: Duration = Duration::from_millis(1);

impl App {
    /// Replay `script` against the app at `width` x `height` on the
    /// headless driver's virtual clock, with no waits, and report every
    /// frame: its time, bytes and painter counts (see the
    /// [module docs](crate::timedemo)). A `wait` runs the timers and
    /// animations that fall inside it, a frame each time one is due. The
    /// replay ends with the script, or when the app quits.
    pub fn timedemo(self, script: Script, width: u16, height: u16) -> Timedemo {
        let mut driver = self.driver(width, height);
        let mut demo = Timedemo {
            size: (width, height),
            ..Timedemo::default()
        };
        let mut now = Duration::ZERO;
        let started = Instant::now();
        let frame = |driver: &mut crate::Driver, now: Duration, demo: &mut Timedemo| {
            let started = Instant::now();
            driver.update(now);
            let out = driver.render();
            let time = started.elapsed();
            if let Some(out) = out {
                let stats = driver.stats();
                demo.frames.push(DemoFrame {
                    at: now,
                    time,
                    draw: stats.draw,
                    paint: stats.paint,
                    bytes: out.len(),
                    painted: stats.painted,
                });
            }
        };
        frame(&mut driver, now, &mut demo);
        for step in script.0 {
            if driver.is_done() {
                break;
            }
            match step {
                Step::Event(event) => {
                    if let Event::Resize { columns, rows } = event {
                        demo.size = (columns, rows);
                    }
                    driver.event(event);
                    frame(&mut driver, now, &mut demo);
                }
                Step::Wait(wait) => {
                    let end = now + wait;
                    while now < end && !driver.is_done() {
                        let next = driver.timeout(now).max(MIN_STEP);
                        now = (now + next).min(end);
                        frame(&mut driver, now, &mut demo);
                    }
                }
            }
        }
        demo.elapsed = started.elapsed();
        demo.covered = now;
        let _ = driver.finish();
        demo
    }
}

/// The input of a rich-record tape as a [`Script`], with the size it asks
/// for (`Set Size`, 100x28 when it does not say): `Type` (a key a
/// character, `Set TypingDelay` apart, 40 ms by default), key steps with
/// their counts (`Down 3`, `Ctrl+C`), `KeyDown`, `KeyUp` and `Hold`,
/// `Sleep` and `Resize`. Steps about the shell, the recording or its
/// output (`Wait`, `Screenshot`, `Exec`, `Output`, other `Set`s) are
/// skipped: a timedemo replays input only.
///
/// ```
/// use std::time::Duration;
/// use intuituive::interact::headless::Step;
/// use intuituive::timedemo::script_from_tape;
///
/// let (script, size) = script_from_tape(
///     "Set Size 80x24\nSet TypingDelay 10ms\nType \"hi\"\nDown 2\nSleep 1s\nScreenshot x\n",
/// )
/// .unwrap();
/// assert_eq!(size, (80, 24));
/// // Two characters a wait apart, two downs, the sleep.
/// assert_eq!(script.0.len(), 6);
/// assert_eq!(script.0[5], Step::Wait(Duration::from_secs(1)));
/// ```
///
/// # Errors
/// A line it cannot read, with its number.
pub fn script_from_tape(tape: &str) -> Result<(Script, (u16, u16)), String> {
    let mut script = Script::new();
    let mut size = (100, 28);
    let mut typing = Duration::from_millis(40);
    for (number, line) in tape.lines().enumerate() {
        let fail = |why: &str| format!("line {}: {why}: {line:?}", number + 1);
        let words = words(line).map_err(fail)?;
        let Some((step, args)) = words.split_first() else {
            continue;
        };
        let arg = |i: usize| args.get(i).map(String::as_str);
        match step.as_str() {
            "Set" => match (arg(0), arg(1)) {
                (Some("Size"), Some(value)) => {
                    size = parse_size(value).ok_or_else(|| fail("bad size"))?
                }
                (Some("TypingDelay"), Some(value)) => {
                    typing = parse_duration(value).ok_or_else(|| fail("bad duration"))?;
                }
                _ => {}
            },
            "Type" => {
                let text = arg(0).ok_or_else(|| fail("nothing to type"))?;
                for (i, c) in text.chars().enumerate() {
                    if i > 0 && !typing.is_zero() {
                        script = script.wait(typing);
                    }
                    script = script.event(Event::Key(Key::char(c)));
                }
            }
            "Sleep" => {
                let wait = arg(0)
                    .and_then(parse_duration)
                    .ok_or_else(|| fail("bad duration"))?;
                script = script.wait(wait);
            }
            "Resize" => {
                let (columns, rows) = arg(0)
                    .and_then(parse_size)
                    .ok_or_else(|| fail("bad size"))?;
                script = script.resize(columns, rows);
            }
            "KeyDown" | "KeyUp" | "Hold" => {
                let key = arg(0)
                    .and_then(tape_key)
                    .ok_or_else(|| fail("unknown key"))?;
                if step != "KeyUp" {
                    script = script.event(Event::Key(key));
                }
                if step == "Hold" {
                    let wait = arg(1)
                        .and_then(parse_duration)
                        .ok_or_else(|| fail("bad duration"))?;
                    script = script.wait(wait);
                }
                if step != "KeyDown" {
                    script = script.event(Event::KeyUp(key));
                }
            }
            name => {
                // A key with an optional count; anything else is not input.
                let Some(key) = tape_key(name) else {
                    continue;
                };
                let count = match arg(0) {
                    Some(n) => n.parse::<usize>().map_err(|_| fail("bad count"))?,
                    None => 1,
                };
                for _ in 0..count {
                    script = script.event(Event::Key(key));
                }
            }
        }
    }
    Ok((script, size))
}

/// A tape's key step (`Enter`, `PageDown`, `Ctrl+Shift+Left`, `F5`) as a
/// key; steps that are not keys (`Wait`, `Screenshot`) are `None`.
fn tape_key(name: &str) -> Option<Key> {
    let last = name.rsplit('+').next()?;
    // Key steps are capitalised; a single character is one only after a
    // modifier (`Ctrl+C`) or in `KeyDown w`.
    let named = matches!(
        last.to_ascii_lowercase().as_str(),
        "enter"
            | "tab"
            | "space"
            | "backspace"
            | "escape"
            | "up"
            | "down"
            | "left"
            | "right"
            | "home"
            | "end"
            | "pageup"
            | "pagedown"
            | "delete"
            | "insert"
    ) || last.len() > 1 && last.starts_with('F') && last[1..].parse::<u8>().is_ok()
        || last.chars().count() == 1;
    if !named {
        return None;
    }
    Key::parse(name)
}

/// The words of a tape line, a quoted string (in `"` or `'`) as one,
/// without a trailing `#` comment.
fn words(line: &str) -> Result<Vec<String>, &'static str> {
    let mut words = Vec::new();
    let mut chars = line.trim().chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c == '#' {
            break;
        } else if c == '"' || c == '\'' {
            chars.next();
            let mut word = String::new();
            loop {
                match chars.next() {
                    Some(q) if q == c => break,
                    Some('\\') if c == '"' => match chars.next() {
                        Some('n') => word.push('\n'),
                        Some('t') => word.push('\t'),
                        Some(other) => word.push(other),
                        None => return Err("unterminated string"),
                    },
                    Some(other) => word.push(other),
                    None => return Err("unterminated string"),
                }
            }
            words.push(word);
        } else {
            let mut word = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() {
                    break;
                }
                word.push(c);
                chars.next();
            }
            words.push(word);
        }
    }
    Ok(words)
}

/// `500ms` or `2s`.
fn parse_duration(text: &str) -> Option<Duration> {
    let (number, scale) = match text.strip_suffix("ms") {
        Some(ms) => (ms, 0.001),
        None => (text.strip_suffix('s')?, 1.0),
    };
    let value: f64 = number.parse().ok()?;
    (value.is_finite() && value >= 0.0)
        .then(|| Duration::try_from_secs_f64(value * scale).ok())
        .flatten()
}

/// `80x24`.
fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (columns, rows) = text.split_once('x')?;
    let size = (columns.parse().ok()?, rows.parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tape_keys_and_counts() {
        let (script, size) =
            script_from_tape("# c\nEnter\nCtrl+C\nPageDown 2\nF5\nWait \"x\"\nKeyUp w\n").unwrap();
        assert_eq!(size, (100, 28));
        let keys: Vec<Step> = ["enter", "ctrl+c", "pagedown", "pagedown", "f5"]
            .iter()
            .map(|k| Step::Event(Event::Key(Key::parse(k).unwrap())))
            .chain([Step::Event(Event::KeyUp(Key::char('w')))])
            .collect();
        assert_eq!(Vec::from(script.0), keys);
        assert!(script_from_tape("Sleep forever").is_err());
        assert!(script_from_tape("Type \"open").is_err());
    }
}

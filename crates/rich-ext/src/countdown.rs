//! Retry and rate-limit countdowns: a [`Backoff`] policy, the
//! [`RetryStatus`] and [`RateLimit`] lines, a shrinking [`CountdownBar`], and
//! [`CountdownWait`], which waits while showing the time left.
//!
//! ```text
//! ⚠ warning attempt 3/5 failed: connection reset — retrying in 4s
//! ⚠ warning rate limited — resets in 0:00:42 (0/100 left)
//! ```
//!
//! Every line carries its meaning in words and symbols, so it reads the same
//! without colour; without colour a bar is drawn as `[####....]`. When motion
//! is unwelcome ([`Motion::Static`]: a non-interactive target, or reduced
//! motion or no animation in the [`AccessibilityPolicy`]), a wait prints its
//! status once instead of redrawing it every tick, so a log gets one line per
//! attempt.
//!
//! Nothing sleeps or reads a clock unless you ask: [`Backoff`] is pure
//! arithmetic (its jitter comes from a seed you choose), the renderables are
//! snapshots of the time left, and [`CountdownWait`] takes an injectable
//! sleeper.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::countdown::Backoff;
//!
//! let backoff = Backoff::new(Duration::from_secs(1)).attempts(5);
//! let delays: Vec<u64> = backoff.delays().map(|d| d.as_secs()).collect();
//! assert_eq!(delays, [1, 2, 4, 8]); // four waits between five attempts
//!
//! let status = backoff.status(3, "connection reset").unwrap();
//! let out = Console::builder().width(70).build().render_to_string(&status);
//! assert_eq!(out, "⚠ warning attempt 3/5 failed: connection reset — retrying in 4s");
//! ```

use std::io::Write;
use std::time::Duration;

use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use crate::a11y::{AccessibilityPolicy, Status, SymbolSet};
use crate::cancel::CancelToken;
use crate::format;
use crate::live::{LiveCoordinator, LiveError};
use crate::target::RenderTarget;
use crate::transfer::{bar_segments, effective_symbols, finish_line, keyed_style};

/// The default styles for countdown keys. [`extended_theme`] includes them;
/// renderers fall back to them when a theme lacks a key.
///
/// [`extended_theme`]: crate::theme::extended_theme
pub const STYLES: &[(&str, &str)] = &[
    ("countdown.remaining", "bold cyan"),
    ("countdown.bar", "cyan"),
    ("countdown.attempt", "bold"),
    ("countdown.reason", "none"),
    ("countdown.retry", "yellow"),
    ("countdown.failed", "bold red"),
    ("countdown.limited", "yellow"),
    ("countdown.quota", "dim"),
];

fn style(console: &Console, key: &str) -> Style {
    keyed_style(console, STYLES, key)
}

/// Time left as a countdown reads it: whole seconds rounded up, `4s`, `59s`,
/// then [`format::duration`] from a minute (`1m 05s`, `2h 00m 00s`).
pub fn remaining_label(left: Duration) -> String {
    let secs = left
        .as_secs()
        .saturating_add(u64::from(left.subsec_nanos() > 0));
    if secs < 60 {
        format!("{secs}s")
    } else {
        format::duration(Duration::from_secs(secs))
    }
}

fn dash(console: &Console) -> &'static str {
    if console.ascii_only() {
        " - "
    } else {
        " — "
    }
}

/// When to try again: exponential backoff with a cap, an optional attempt
/// limit and optional deterministic jitter.
///
/// The delay after failed attempt `n` (from 1) is
/// `initial × factorⁿ⁻¹`, capped at `max`, then reduced by up to `jitter` of
/// itself (so `jitter(0.5, seed)` gives between half and all of it).
///
/// ```
/// use std::time::Duration;
/// use rich_ext::countdown::Backoff;
///
/// let secs = Duration::from_secs;
/// let backoff = Backoff::new(secs(1)).factor(3.0).max(secs(20));
/// assert_eq!(backoff.delay(1), Some(secs(1)));
/// assert_eq!(backoff.delay(3), Some(secs(9)));
/// assert_eq!(backoff.delay(4), Some(secs(20)));
///
/// let jittered = backoff.clone().jitter(0.5, 42);
/// let d = jittered.delay(3).unwrap();
/// assert!(d >= secs(9) / 2 && d <= secs(9));
/// assert_eq!(jittered.delay(3), Some(d)); // same seed, same delays
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Backoff {
    initial: Duration,
    factor: f64,
    max: Duration,
    max_attempts: Option<u32>,
    jitter: f64,
    seed: u64,
}

impl Backoff {
    /// Start at `initial`, doubling, capped at one minute, attempts unlimited,
    /// no jitter.
    pub fn new(initial: Duration) -> Self {
        Backoff {
            initial,
            factor: 2.0,
            max: Duration::from_secs(60),
            max_attempts: None,
            jitter: 0.0,
            seed: 0,
        }
    }

    /// Multiply the delay by `factor` after each failure (at least 1).
    pub fn factor(mut self, factor: f64) -> Self {
        self.factor = if factor.is_finite() {
            factor.max(1.0)
        } else {
            1.0
        };
        self
    }

    /// Never wait longer than `max`.
    pub fn max(mut self, max: Duration) -> Self {
        self.max = max;
        self
    }

    /// Give up after `attempts` attempts in all (at least 1).
    pub fn attempts(mut self, attempts: u32) -> Self {
        self.max_attempts = Some(attempts.max(1));
        self
    }

    /// Shorten each delay by a pseudo-random part of up to `fraction`
    /// (0..=1) of it, drawn from `seed`: the same seed always gives the same
    /// delays, so tests stay deterministic. Seed from the clock or process id
    /// in production so clients spread out.
    pub fn jitter(mut self, fraction: f64, seed: u64) -> Self {
        self.jitter = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        self.seed = seed;
        self
    }

    /// The attempt limit, if any.
    pub fn max_attempts(&self) -> Option<u32> {
        self.max_attempts
    }

    /// How long to wait after failed attempt `attempt` (from 1), or `None`
    /// when that was the last attempt allowed.
    pub fn delay(&self, attempt: u32) -> Option<Duration> {
        let attempt = attempt.max(1);
        if self.max_attempts.is_some_and(|max| attempt >= max) {
            return None;
        }
        // factorⁿ may overflow to infinity; zero times it is still zero.
        let exponent = i32::try_from(attempt - 1).unwrap_or(i32::MAX);
        let base = if self.initial.is_zero() {
            0.0
        } else {
            self.initial.as_secs_f64() * self.factor.powi(exponent)
        };
        let capped = base.min(self.max.as_secs_f64());
        let unit = (splitmix64(self.seed ^ u64::from(attempt)) >> 11) as f64 / (1u64 << 53) as f64;
        // Jitter scales by a factor in (0, 1], so it never lengthens the
        // delay; a value too big for a `Duration` is the cap itself.
        let delay = capped * (1.0 - self.jitter * unit);
        let delay = Duration::try_from_secs_f64(delay).unwrap_or(self.max);
        Some(delay.min(self.max))
    }

    /// The delays between attempts, in order; endless without an attempt
    /// limit.
    pub fn delays(&self) -> impl Iterator<Item = Duration> + '_ {
        (1..).map_while(move |attempt| self.delay(attempt))
    }

    /// The status line for failed attempt `attempt`: retrying in its delay,
    /// or giving up after the last attempt. `None` only for attempt 0.
    pub fn status(&self, attempt: u32, reason: impl Into<String>) -> Option<RetryStatus> {
        if attempt == 0 {
            return None;
        }
        let mut status = RetryStatus::new(attempt).reason(reason);
        if let Some(max) = self.max_attempts {
            status = status.max_attempts(max);
        }
        if let Some(delay) = self.delay(attempt) {
            status = status.retrying_in(delay);
        }
        Some(status)
    }
}

/// SplitMix64: a small, well-mixed hash for the jitter.
fn splitmix64(seed: u64) -> u64 {
    let mut z = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// Append a countdown bar (without its time label, which the sentence
/// already has) to `line`, shrunk to the room left, or not at all when fewer
/// than 5 cells remain.
fn append_bar(
    line: &mut Vec<Segment>,
    console: &Console,
    options: &ConsoleOptions,
    total: Duration,
    left: Duration,
    width: usize,
) {
    let used: usize = line.iter().map(Segment::cell_length).sum();
    let width = width.min(options.max_width.saturating_sub(used + 2));
    if width < 5 {
        return;
    }
    line.push(Segment::new("  ", None));
    let mut bar = CountdownBar::new(total, left)
        .width(width)
        .segments(console, options);
    bar.truncate(bar.len().saturating_sub(2));
    line.extend(bar);
}

/// A bar that shrinks as time runs out, followed by the time left.
///
/// ```
/// use std::time::Duration;
/// use rich::Console;
/// use rich_ext::countdown::CountdownBar;
///
/// let bar = CountdownBar::new(Duration::from_secs(10), Duration::from_secs(4)).width(12);
/// let out = Console::builder().width(40).build().render_to_string(&bar);
/// assert_eq!(out, "[####......]  4s");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CountdownBar {
    total: Duration,
    remaining: Duration,
    width: usize,
}

impl CountdownBar {
    /// A countdown of `total` with `remaining` left, 20 cells wide.
    pub fn new(total: Duration, remaining: Duration) -> Self {
        CountdownBar {
            total,
            remaining: remaining.min(total),
            width: 20,
        }
    }

    /// The bar's width in cells (at least 3).
    pub fn width(mut self, width: usize) -> Self {
        self.width = width.max(3);
        self
    }

    fn ratio(&self) -> f64 {
        if self.total.is_zero() {
            0.0
        } else {
            self.remaining.as_secs_f64() / self.total.as_secs_f64()
        }
    }

    fn segments(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut out = bar_segments(
            console,
            options,
            self.width.min(options.max_width),
            Some(self.ratio()),
            0.0,
            Some(style(console, "countdown.bar")),
        );
        out.push(Segment::new("  ", None));
        out.push(Segment::new(
            remaining_label(self.remaining),
            Some(style(console, "countdown.remaining")),
        ));
        out
    }
}

impl Renderable for CountdownBar {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        finish_line(self.segments(console, options), options.max_width)
    }
}

/// One failed attempt: which, why, and when the next one comes (or that
/// there will be none). See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryStatus {
    attempt: u32,
    max_attempts: Option<u32>,
    reason: Option<String>,
    remaining: Option<Duration>,
    total: Option<Duration>,
    bar_width: usize,
    symbols: SymbolSet,
}

impl RetryStatus {
    /// Attempt `attempt` (from 1) failed; no retry scheduled yet, so it reads
    /// as giving up until [`retrying_in`](Self::retrying_in) is set.
    pub fn new(attempt: u32) -> Self {
        RetryStatus {
            attempt,
            max_attempts: None,
            reason: None,
            remaining: None,
            total: None,
            bar_width: 20,
            symbols: SymbolSet::Unicode,
        }
    }

    /// Show the attempt as `n/max`.
    pub fn max_attempts(mut self, max: u32) -> Self {
        self.max_attempts = Some(max);
        self
    }

    /// Why the attempt failed.
    pub fn reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }

    /// The next attempt comes after `left`.
    pub fn retrying_in(mut self, left: Duration) -> Self {
        self.remaining = Some(left);
        self
    }

    /// Also draw a [`CountdownBar`] of the whole `delay` (use with
    /// [`retrying_in`](Self::retrying_in) for the time left).
    pub fn bar(mut self, delay: Duration) -> Self {
        self.total = Some(delay);
        self
    }

    /// The bar's width in cells (default 20).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = width;
        self
    }

    /// How the status is marked (default Unicode; ASCII on an ASCII-only
    /// console).
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = set;
        self
    }

    /// Whether this is the final failure (no retry scheduled).
    pub fn giving_up(&self) -> bool {
        self.remaining.is_none()
    }

    /// The same status with `left` remaining: the frame for one tick.
    pub fn at(&self, left: Duration) -> Self {
        let mut next = self.clone();
        next.remaining = Some(left);
        next
    }

    /// Warning while retrying, error when giving up.
    pub fn status(&self) -> Status {
        if self.giving_up() {
            Status::Error
        } else {
            Status::Warning
        }
    }
}

impl Renderable for RetryStatus {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let set = effective_symbols(console, self.symbols);
        let (marker_key, attempt) = match self.max_attempts {
            Some(max) => (self.status(), format!("attempt {}/{max}", self.attempt)),
            None => (self.status(), format!("attempt {}", self.attempt)),
        };
        let key = if marker_key == Status::Error {
            "countdown.failed"
        } else {
            "countdown.retry"
        };
        let mut line = vec![
            Segment::new(marker_key.symbol(set), Some(style(console, key))),
            Segment::new(" ", None),
            Segment::new(attempt, Some(style(console, "countdown.attempt"))),
            Segment::new(" failed", None),
        ];
        if let Some(reason) = &self.reason {
            line.push(Segment::new(": ", None));
            line.push(Segment::new(
                reason.clone(),
                Some(style(console, "countdown.reason")),
            ));
        }
        line.push(Segment::new(dash(console), None));
        match self.remaining {
            Some(left) => {
                line.push(Segment::new("retrying in ", None));
                line.push(Segment::new(
                    remaining_label(left),
                    Some(style(console, "countdown.remaining")),
                ));
                if let Some(total) = self.total {
                    append_bar(&mut line, console, options, total, left, self.bar_width);
                }
            }
            None => line.push(Segment::new("giving up", Some(style(console, key)))),
        }
        finish_line(line, options.max_width)
    }
}

/// A rate limit: when it resets and, if known, how much of the quota is left.
///
/// ```
/// use std::time::Duration;
/// use rich::Console;
/// use rich_ext::countdown::RateLimit;
///
/// let limit = RateLimit::new(Duration::from_secs(42)).limit(100).remaining(0).scope("search API");
/// let out = Console::builder().width(80).build().render_to_string(&limit);
/// assert_eq!(out, "⚠ warning rate limited (search API) — resets in 0:00:42 (0/100 left)");
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RateLimit {
    resets_in: Duration,
    limit: Option<u64>,
    remaining: Option<u64>,
    scope: Option<String>,
    window: Option<Duration>,
    bar_width: usize,
    symbols: SymbolSet,
}

impl RateLimit {
    /// Limited until `resets_in` from now.
    pub fn new(resets_in: Duration) -> Self {
        RateLimit {
            resets_in,
            limit: None,
            remaining: None,
            scope: None,
            window: None,
            bar_width: 20,
            symbols: SymbolSet::Unicode,
        }
    }

    /// The quota per window.
    pub fn limit(mut self, limit: u64) -> Self {
        self.limit = Some(limit);
        self
    }

    /// How much of the quota is left.
    pub fn remaining(mut self, remaining: u64) -> Self {
        self.remaining = Some(remaining);
        self
    }

    /// What is limited, e.g. an API name.
    pub fn scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    /// Also draw a [`CountdownBar`] over a reset window of `window`.
    pub fn bar(mut self, window: Duration) -> Self {
        self.window = Some(window);
        self
    }

    /// The bar's width in cells (default 20).
    pub fn bar_width(mut self, width: usize) -> Self {
        self.bar_width = width;
        self
    }

    /// How the status is marked (default Unicode; ASCII on an ASCII-only
    /// console).
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.symbols = set;
        self
    }

    /// The same limit with `left` until the reset: the frame for one tick.
    pub fn at(&self, left: Duration) -> Self {
        let mut next = self.clone();
        next.resets_in = left;
        next
    }
}

impl Renderable for RateLimit {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let set = effective_symbols(console, self.symbols);
        let limited = style(console, "countdown.limited");
        let mut line = vec![
            Segment::new(Status::Warning.symbol(set), Some(limited.clone())),
            Segment::new(" ", None),
            Segment::new("rate limited", Some(limited)),
        ];
        if let Some(scope) = &self.scope {
            line.push(Segment::new(format!(" ({scope})"), None));
        }
        line.push(Segment::new(dash(console), None));
        line.push(Segment::new("resets in ", None));
        line.push(Segment::new(
            format::clock(self.resets_in),
            Some(style(console, "countdown.remaining")),
        ));
        let quota = match (self.remaining, self.limit) {
            (Some(left), Some(limit)) => Some(format!("{left}/{limit} left")),
            (Some(left), None) => Some(format!("{left} left")),
            (None, Some(limit)) => Some(format!("limit {limit}")),
            (None, None) => None,
        };
        if let Some(quota) = quota {
            line.push(Segment::new(
                format!(" ({quota})"),
                Some(style(console, "countdown.quota")),
            ));
        }
        if let Some(window) = self.window {
            append_bar(
                &mut line,
                console,
                options,
                window,
                self.resets_in,
                self.bar_width,
            );
        }
        finish_line(line, options.max_width)
    }
}

/// Whether a countdown redraws itself or is printed once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Motion {
    /// Redraw every tick in a live region.
    #[default]
    Animated,
    /// Print once; the wait is silent. For logs, pipes and reduced motion.
    Static,
}

impl Motion {
    /// [`Static`](Motion::Static) for a non-interactive target or a policy
    /// asking for reduced motion or no animation; otherwise animated.
    pub fn for_target(target: &RenderTarget, policy: &AccessibilityPolicy) -> Motion {
        use rich::protocol::RenderEnvironment;
        if !target.capabilities().interactive || policy.no_animation || policy.reduced_motion {
            Motion::Static
        } else {
            Motion::Animated
        }
    }
}

/// How a [`CountdownWait`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome {
    /// The whole time passed.
    Elapsed,
    /// The token was cancelled first.
    Cancelled,
}

/// A blocking wait that reports the time left every tick and stops early
/// when a [`CancelToken`] is cancelled.
///
/// Time is counted from the sleeps themselves, so an injected sleeper makes
/// the wait instant and deterministic:
///
/// ```
/// use std::time::Duration;
/// use rich_ext::countdown::{CountdownWait, WaitOutcome};
///
/// let mut ticks = Vec::new();
/// let outcome = CountdownWait::new(Duration::from_secs(2))
///     .tick(Duration::from_secs(1))
///     .sleeper(|_| {})
///     .run(|left| ticks.push(left.as_secs()));
/// assert_eq!(outcome, WaitOutcome::Elapsed);
/// assert_eq!(ticks, [2, 1, 0]);
/// ```
pub struct CountdownWait<'a> {
    total: Duration,
    tick: Duration,
    cancel: Option<CancelToken>,
    sleep: Box<dyn FnMut(Duration) + 'a>,
}

impl<'a> CountdownWait<'a> {
    /// Wait `total`, ticking every 250 ms with `std::thread::sleep`.
    pub fn new(total: Duration) -> Self {
        CountdownWait {
            total,
            tick: Duration::from_millis(250),
            cancel: None,
            sleep: Box::new(std::thread::sleep),
        }
    }

    /// Report (and check for cancellation) every `tick` (at least 1 ms).
    pub fn tick(mut self, tick: Duration) -> Self {
        self.tick = tick.max(Duration::from_millis(1));
        self
    }

    /// Stop early once `token` is cancelled.
    pub fn cancel(mut self, token: CancelToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Sleep with `sleep` instead of `std::thread::sleep`.
    pub fn sleeper(mut self, sleep: impl FnMut(Duration) + 'a) -> Self {
        self.sleep = Box::new(sleep);
        self
    }

    /// Wait, calling `on_tick` with the time left before each sleep and once
    /// with zero at the end.
    pub fn run(mut self, mut on_tick: impl FnMut(Duration)) -> WaitOutcome {
        let mut left = self.total;
        loop {
            if self.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                return WaitOutcome::Cancelled;
            }
            on_tick(left);
            if left.is_zero() {
                return WaitOutcome::Elapsed;
            }
            let step = self.tick.min(left);
            (self.sleep)(step);
            left -= step;
        }
    }

    /// Wait while showing `frame(time_left)` through `live`.
    ///
    /// [`Motion::Animated`] draws the frame in a new region, updates it every
    /// tick and removes it at the end. [`Motion::Static`] prints the first
    /// frame once as an ordinary line, which a log keeps.
    pub fn run_live<W: Write, R: Renderable>(
        self,
        live: &mut LiveCoordinator<W>,
        target: &RenderTarget,
        motion: Motion,
        frame: impl Fn(Duration) -> R,
    ) -> Result<WaitOutcome, LiveError> {
        if motion == Motion::Static {
            live.print(&target.segments(&frame(self.total)))?;
            return Ok(self.run(|_| {}));
        }
        let region = live.add(target.segments(&frame(self.total)))?;
        let mut error = None;
        let outcome = self.run(|left| {
            if error.is_some() {
                return;
            }
            let result = live
                .update(region.clone(), target.segments(&frame(left)))
                .and_then(|()| live.refresh());
            if let Err(e) = result {
                error = Some(e);
            }
        });
        if let Some(e) = error {
            return Err(e);
        }
        live.remove(region)?;
        live.refresh()?;
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_round_up_to_whole_seconds() {
        assert_eq!(remaining_label(Duration::ZERO), "0s");
        assert_eq!(remaining_label(Duration::from_millis(3100)), "4s");
        assert_eq!(remaining_label(Duration::from_secs(65)), "1m 05s");
    }

    #[test]
    fn jitter_stays_in_range_and_varies_by_attempt() {
        let backoff = Backoff::new(Duration::from_secs(8))
            .factor(1.0)
            .jitter(1.0, 7);
        let delays: Vec<Duration> = (1..=5).filter_map(|a| backoff.delay(a)).collect();
        assert!(delays.iter().all(|d| *d <= Duration::from_secs(8)));
        assert!(delays.windows(2).any(|w| w[0] != w[1]));
    }
}

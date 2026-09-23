//! Render profiling: timings, output size, frame cost and allocations.
//!
//! [`profile`] times `measure()` and `rich_render()` over a number of
//! iterations with [`std::time::Instant`], counts the output (segments,
//! lines, cells, bytes) and, when [`ProfileOptions::frame`] is set, the cost
//! of a Live-like frame: render at a fixed size, shape to exactly that many
//! rows ([`Segment::set_shape`]) and encode to ANSI, as a live display does
//! on every refresh.
//!
//! # Allocations
//!
//! Install [`CountingAllocator`] as the global allocator of a test or bench
//! binary and [`Profile::allocations`] is filled in; otherwise it is `None`.
//! The library never installs it.
//!
//! ```
//! use rich_ext::qa::profile::{AllocStats, CountingAllocator};
//!
//! #[global_allocator]
//! static ALLOC: CountingAllocator = CountingAllocator::system();
//!
//! fn main() {
//!     let _warm = vec![0u8; 16];
//!     assert!(AllocStats::now().is_some());
//! }
//! ```
//!
//! The counters are process-wide: allocations made by other threads while a
//! profile runs (parallel tests) are counted too, so treat them as an upper
//! bound, or run with `--test-threads=1`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use rich::cells::cell_len;
use rich::{Console, ConsoleOptions, Renderable, Segment, Table, Text};
use serde::{Deserialize, Serialize};

use super::{plain_lines, table_then_line};

static INSTALLED: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static DEALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static BYTES: AtomicU64 = AtomicU64::new(0);
static LIVE: AtomicU64 = AtomicU64::new(0);

/// A [`GlobalAlloc`] wrapper that counts allocations, deallocations and
/// bytes allocated, then delegates to `A` (default [`System`]).
#[derive(Debug, Default)]
pub struct CountingAllocator<A = System> {
    inner: A,
}

impl CountingAllocator<System> {
    /// Counting on top of the system allocator.
    pub const fn system() -> Self {
        CountingAllocator { inner: System }
    }
}

impl<A> CountingAllocator<A> {
    /// Counting on top of `inner`.
    pub const fn new(inner: A) -> Self {
        CountingAllocator { inner }
    }
}

// SAFETY: every method forwards to the inner allocator with the caller's
// arguments unchanged; the counters are plain atomics and never allocate.
#[allow(unsafe_code)]
unsafe impl<A: GlobalAlloc> GlobalAlloc for CountingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        INSTALLED.store(true, Ordering::Relaxed);
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        LIVE.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: forwarded unchanged; the caller upholds `alloc`'s contract.
        unsafe { self.inner.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: forwarded unchanged; the caller upholds `dealloc`'s contract.
        unsafe { self.inner.dealloc(ptr, layout) }
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        INSTALLED.store(true, Ordering::Relaxed);
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        LIVE.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: forwarded unchanged; the caller upholds `alloc_zeroed`'s contract.
        unsafe { self.inner.alloc_zeroed(layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        DEALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        LIVE.fetch_add(new_size as u64, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: forwarded unchanged; the caller upholds `realloc`'s contract.
        unsafe { self.inner.realloc(ptr, layout, new_size) }
    }
}

/// Counter readings; subtract two with [`since`](Self::since).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AllocStats {
    /// Allocations (a `realloc` counts as one, and as one deallocation).
    pub allocations: u64,
    pub deallocations: u64,
    /// Bytes requested.
    pub bytes: u64,
    /// Bytes live at the reading (for a delta: the net change).
    pub live_bytes: i64,
}

impl AllocStats {
    /// The current counters, or `None` when [`CountingAllocator`] is not the
    /// global allocator.
    pub fn now() -> Option<Self> {
        INSTALLED.load(Ordering::Relaxed).then(|| AllocStats {
            allocations: ALLOCATIONS.load(Ordering::Relaxed),
            deallocations: DEALLOCATIONS.load(Ordering::Relaxed),
            bytes: BYTES.load(Ordering::Relaxed),
            live_bytes: LIVE.load(Ordering::Relaxed) as i64,
        })
    }
    /// What happened between `earlier` and this reading.
    pub fn since(&self, earlier: &AllocStats) -> AllocStats {
        AllocStats {
            allocations: self.allocations.saturating_sub(earlier.allocations),
            deallocations: self.deallocations.saturating_sub(earlier.deallocations),
            bytes: self.bytes.saturating_sub(earlier.bytes),
            live_bytes: self.live_bytes - earlier.live_bytes,
        }
    }
}

/// Summary statistics of a set of durations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timing {
    pub samples: usize,
    pub mean: Duration,
    pub median: Duration,
    pub p95: Duration,
    pub min: Duration,
    pub max: Duration,
}

impl Timing {
    /// Statistics of `samples` (all zero when empty). p95 is nearest-rank.
    pub fn from_samples(samples: &[Duration]) -> Self {
        if samples.is_empty() {
            return Timing::default();
        }
        let mut sorted = samples.to_vec();
        sorted.sort_unstable();
        let n = sorted.len();
        let total: Duration = sorted.iter().sum();
        let median = if n % 2 == 1 {
            sorted[n / 2]
        } else {
            (sorted[n / 2 - 1] + sorted[n / 2]) / 2
        };
        let rank = ((n as f64) * 0.95).ceil() as usize;
        Timing {
            samples: n,
            mean: total / n as u32,
            median,
            p95: sorted[rank.clamp(1, n) - 1],
            min: sorted[0],
            max: sorted[n - 1],
        }
    }
}

/// What [`profile`] runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileOptions {
    /// Render width (default: the console's width).
    pub width: Option<usize>,
    /// Timed iterations (default 20; at least 1).
    pub iterations: usize,
    /// Untimed iterations first (default 2).
    pub warmup: usize,
    /// `(width, height)` of a Live-like frame to cost, if any.
    pub frame: Option<(usize, usize)>,
}

impl Default for ProfileOptions {
    fn default() -> Self {
        ProfileOptions {
            width: None,
            iterations: 20,
            warmup: 2,
            frame: None,
        }
    }
}

impl ProfileOptions {
    pub fn iterations(mut self, n: usize) -> Self {
        self.iterations = n;
        self
    }
    pub fn width(mut self, width: usize) -> Self {
        self.width = Some(width);
        self
    }
    pub fn frame(mut self, width: usize, height: usize) -> Self {
        self.frame = Some((width, height));
        self
    }
}

/// The cost of one Live-like refresh.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrameCost {
    pub width: usize,
    pub height: usize,
    /// Render + shape + encode, per frame.
    pub time: Timing,
    /// Bytes of ANSI per frame.
    pub bytes: usize,
}

/// What [`profile`] measured.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub width: usize,
    pub iterations: usize,
    pub measure_time: Timing,
    pub render_time: Timing,
    /// Output of one render.
    pub segments: usize,
    pub lines: usize,
    pub cells: usize,
    /// ANSI bytes of one render on the console.
    pub bytes: usize,
    pub frame: Option<FrameCost>,
    /// Per render (the mean over the timed iterations), with
    /// [`CountingAllocator`] installed.
    pub allocations: Option<AllocStats>,
}

/// Profile `renderable` on `console` (see the [module docs](self)).
pub fn profile(
    renderable: &dyn Renderable,
    console: &Console,
    options: &ProfileOptions,
) -> Profile {
    let width = options.width.unwrap_or_else(|| console.width()).max(1);
    let render_options = console.options().update_width(width);
    let iterations = options.iterations.max(1);
    for _ in 0..options.warmup {
        std::hint::black_box(renderable.rich_render(console, &render_options));
    }
    let mut measure = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let start = Instant::now();
        std::hint::black_box(renderable.measure(console, &render_options));
        measure.push(start.elapsed());
    }
    let before = AllocStats::now();
    let mut render = Vec::with_capacity(iterations);
    let mut segments = Vec::new();
    for _ in 0..iterations {
        let start = Instant::now();
        segments = std::hint::black_box(renderable.rich_render(console, &render_options));
        render.push(start.elapsed());
    }
    let allocations = match (before, AllocStats::now()) {
        (Some(before), Some(after)) => {
            let d = after.since(&before);
            let n = iterations as u64;
            Some(AllocStats {
                allocations: d.allocations / n,
                deallocations: d.deallocations / n,
                bytes: d.bytes / n,
                live_bytes: d.live_bytes / n as i64,
            })
        }
        _ => None,
    };
    let lines = plain_lines(&segments);
    let frame = options.frame.map(|(fw, fh)| {
        let mut frame_options = console.options().update_dimensions(fw.max(1), fh);
        frame_options.height = Some(fh);
        let mut times = Vec::with_capacity(iterations);
        let mut bytes = 0;
        for _ in 0..iterations {
            let start = Instant::now();
            let segs = renderable.rich_render(console, &frame_options);
            let shaped = Segment::set_shape(Segment::split_lines(&segs), fw, fh);
            let mut out = String::new();
            for (i, line) in shaped.iter().enumerate() {
                if i > 0 {
                    out.push('\n');
                }
                out.push_str(&console.segments_to_string(line));
            }
            bytes = std::hint::black_box(out).len();
            times.push(start.elapsed());
        }
        FrameCost {
            width: fw,
            height: fh,
            time: Timing::from_samples(&times),
            bytes,
        }
    });
    Profile {
        width,
        iterations,
        measure_time: Timing::from_samples(&measure),
        render_time: Timing::from_samples(&render),
        segments: segments.iter().filter(|s| !s.control).count(),
        cells: lines.iter().map(|l| cell_len(l)).sum(),
        lines: lines.len(),
        bytes: console.segments_to_string(&segments).len(),
        frame,
        allocations,
    }
}

/// `d` in the largest unit that keeps it ≥ 1, e.g. `12.3 µs`.
pub fn format_duration(d: Duration, ascii: bool) -> String {
    format_nanos(d.as_secs_f64() * 1e9, ascii)
}

pub(crate) fn format_nanos(ns: f64, ascii: bool) -> String {
    let micro = if ascii { "us" } else { "µs" };
    if ns >= 1e9 {
        format!("{:.2} s", ns / 1e9)
    } else if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e3 {
        format!("{:.2} {micro}", ns / 1e3)
    } else {
        format!("{ns:.0} ns")
    }
}

/// A [`Profile`] as a table.
pub struct ProfileReport<'a> {
    profile: &'a Profile,
}

impl<'a> ProfileReport<'a> {
    pub fn new(profile: &'a Profile) -> Self {
        ProfileReport { profile }
    }
}

impl Renderable for ProfileReport<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let p = self.profile;
        let ascii = console.ascii_only();
        let fmt = |d: Duration| format_duration(d, ascii);
        let mut table = Table::new();
        for header in ["Phase", "Mean", "Median", "p95", "Min", "Max"] {
            table.add_column(header);
        }
        let mut row = |name: String, t: &Timing| {
            table.add_row_text(vec![
                Text::new(name),
                Text::new(fmt(t.mean)),
                Text::new(fmt(t.median)),
                Text::new(fmt(t.p95)),
                Text::new(fmt(t.min)),
                Text::new(fmt(t.max)),
            ]);
        };
        row("measure".into(), &p.measure_time);
        row("render".into(), &p.render_time);
        if let Some(frame) = &p.frame {
            row(
                format!("frame {}x{}", frame.width, frame.height),
                &frame.time,
            );
        }
        let mut summary = format!(
            "width {}, {} iterations: {} segments, {} lines, {} cells, {} bytes",
            p.width, p.iterations, p.segments, p.lines, p.cells, p.bytes
        );
        if let Some(frame) = &p.frame {
            summary.push_str(&format!("; {} bytes per frame", frame.bytes));
        }
        match &p.allocations {
            Some(a) => summary.push_str(&format!(
                "; {} allocations ({} bytes) per render",
                a.allocations, a.bytes
            )),
            None => summary.push_str("; allocations not counted (no CountingAllocator)"),
        }
        table_then_line(Some(table), summary, console, options)
    }
}

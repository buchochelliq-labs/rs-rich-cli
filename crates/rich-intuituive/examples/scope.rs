//! An oscilloscope, vectorscope and spectroscope for the terminal in the
//! style of [scope-tui](https://github.com/alemidev/scope-tui), a ratatui
//! app, rebuilt on intuiTUIve. It reuses none of scope-tui's code: it is a
//! rebuild of the behaviour, showing how an app that redraws twenty or more
//! times a second maps.
//!
//!     cargo run -p rs-rich-intuituive --example scope              # a test signal
//!     cargo run -p rs-rich-intuituive --example scope -- file PATH # raw PCM
//!     parec --format=s16le --rate=48000 --channels=2 \
//!         | cargo run -p rs-rich-intuituive --example scope -- file -
//!
//! Options, as scope-tui's: `-c N` channels, `-b SIZE` buffer, `-r HZ`
//! sample rate, `-t NOTE` tune the buffer to a note, `-s X` scale,
//! `--scatter`, `--no-reference`, `--no-ui`, `--no-braille`,
//! `--palette-color a,b,…`, `--labels-color c`, `--axis-color c`; `file`
//! takes `-l` to play a file at its sample rate. `--still` makes the test
//! signal stand still.
//!
//! q quits · space pauses · tab changes mode · s scatter · h hides the
//! interface · r reference lines · ↑ ↓ scale · ← → samples · esc resets ·
//! shift ×10, ctrl ×5, alt ×⅕ · ? shows every key.

use std::cell::Cell;
use std::collections::VecDeque;
use std::f64::consts::PI;
use std::io::Read;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use intuituive::prelude::*;
use intuituive::rich::{Console, Segment, Style};
use rich_ext::chart::DotCanvas;

/// One buffer of samples, one row per channel, each in -1 to 1.
pub type Matrix = Vec<Vec<f64>>;

/// What a [`Source`] has for the app.
pub enum Pull {
    /// The next buffer.
    Frame(Matrix),
    /// Nothing new yet.
    Wait,
    /// The input has ended.
    End,
}

/// Where the samples come from.
pub trait Source {
    fn pull(&mut self) -> Pull;
}

/// A test signal: two tones a fifth apart (2 and 3 cycles a buffer), one
/// per channel, with an overtone each, so the vectorscope draws a
/// Lissajous figure. Every tone is a whole number of cycles a buffer;
/// unless `still`, the right channel's phase drifts from one buffer to the
/// next and the figure turns.
pub struct Demo {
    pub channels: usize,
    pub buffer: usize,
    pub still: bool,
    frame: u64,
}

impl Demo {
    pub fn new(channels: usize, buffer: usize, still: bool) -> Demo {
        Demo {
            channels: channels.max(1),
            buffer: buffer.max(1),
            still,
            frame: 0,
        }
    }
}

impl Source for Demo {
    fn pull(&mut self) -> Pull {
        let n = self.buffer as f64;
        let drift = if self.still {
            0.0
        } else {
            self.frame as f64 * 0.05
        };
        self.frame += 1;
        // A tone `cycles` times per buffer, at sample `i`.
        let tone =
            |cycles: f64, i: usize, phase: f64| (2.0 * PI * cycles * i as f64 / n + phase).sin();
        let channel = |c: usize| -> Vec<f64> {
            (0..self.buffer)
                .map(|i| match c {
                    0 => 0.6 * tone(2.0, i, 0.0) + 0.2 * tone(6.0, i, 0.0),
                    1 => 0.6 * tone(3.0, i, drift) + 0.15 * tone(9.0, i, 3.0 * drift),
                    c => 0.5 * tone(c as f64 + 2.0, i, 0.0),
                })
                .collect()
        };
        Pull::Frame((0..self.channels).map(channel).collect())
    }
}

/// Signed 16-bit little-endian PCM, channels interleaved (`L R L R …`), as
/// `parec --format=s16le` writes it: a file, a pipe or stdin.
pub struct Pcm<R> {
    reader: R,
    channels: usize,
    bytes: Vec<u8>,
}

impl<R: Read> Pcm<R> {
    pub fn new(reader: R, channels: usize, buffer: usize) -> Pcm<R> {
        let channels = channels.max(1);
        Pcm {
            reader,
            channels,
            bytes: vec![0; buffer.max(1) * channels * 2],
        }
    }
}

/// Split interleaved samples into channels, each scaled to -1 to 1.
pub fn deinterleave(bytes: &[u8], channels: usize) -> Matrix {
    let mut out = vec![Vec::with_capacity(bytes.len() / 2 / channels.max(1)); channels.max(1)];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        let sample = i16::from_le_bytes(*pair) as f64 / 32768.0;
        out[index % channels.max(1)].push(sample);
    }
    out
}

impl<R: Read> Source for Pcm<R> {
    fn pull(&mut self) -> Pull {
        // Fill the buffer; a short last one is padded with silence.
        let mut filled = 0;
        while filled < self.bytes.len() {
            match self.reader.read(&mut self.bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
        if filled == 0 {
            return Pull::End;
        }
        self.bytes[filled..].fill(0);
        Pull::Frame(deinterleave(&self.bytes, self.channels))
    }
}

/// A source read on a thread of its own, so a slow pipe never holds up the
/// app: the app takes the newest buffer, and one it was too slow for is
/// dropped rather than queued.
pub struct Threaded {
    /// The newest buffer not yet taken, and whether the input has ended.
    shared: Arc<Mutex<(Option<Matrix>, bool)>>,
}

impl Threaded {
    /// Read `source`, waiting `pace` after each buffer if it is set (to play
    /// a file at its sample rate rather than all at once).
    pub fn new(mut source: impl Source + Send + 'static, pace: Option<Duration>) -> Threaded {
        let shared = Arc::new(Mutex::new((None, false)));
        let slot = shared.clone();
        std::thread::spawn(move || loop {
            match source.pull() {
                Pull::Frame(frame) => slot.lock().unwrap().0 = Some(frame),
                // The end never replaces a buffer the app has not taken.
                Pull::End => {
                    slot.lock().unwrap().1 = true;
                    break;
                }
                Pull::Wait => std::thread::sleep(Duration::from_millis(1)),
            }
            if Arc::strong_count(&slot) == 1 {
                break;
            }
            if let Some(pace) = pace {
                std::thread::sleep(pace);
            }
        });
        Threaded { shared }
    }
}

impl Source for Threaded {
    fn pull(&mut self) -> Pull {
        let mut shared = self.shared.lock().unwrap();
        match shared.0.take() {
            Some(frame) => Pull::Frame(frame),
            None if shared.1 => Pull::End,
            None => Pull::Wait,
        }
    }
}

/// The settings every mode shares, as scope-tui's `GraphConfig`.
#[derive(Clone, Debug, PartialEq)]
pub struct Graph {
    /// Samples shown across the oscilloscope (← →).
    pub samples: u32,
    /// The buffer size.
    pub width: u32,
    pub rate: u32,
    /// The vertical range (↑ ↓).
    pub scale: f64,
    pub scatter: bool,
    pub references: bool,
    pub show_ui: bool,
    pub pause: bool,
    pub braille: bool,
    pub palette: Vec<String>,
    pub labels: String,
    pub axis: String,
}

impl Graph {
    /// The colour of series `index`.
    pub fn palette(&self, index: usize) -> &str {
        if self.palette.is_empty() {
            return "white";
        }
        &self.palette[index % self.palette.len()]
    }
}

/// Which scope is showing (Tab cycles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Oscilloscope,
    Vectorscope,
    Spectroscope,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Oscilloscope => "oscillo",
            Mode::Vectorscope => "vector",
            Mode::Spectroscope => "spectro",
        }
    }

    fn next(self) -> Mode {
        match self {
            Mode::Oscilloscope => Mode::Vectorscope,
            Mode::Vectorscope => Mode::Spectroscope,
            Mode::Spectroscope => Mode::Oscilloscope,
        }
    }
}

/// The oscilloscope's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Oscilloscope {
    pub triggering: bool,
    pub falling_edge: bool,
    /// The level the trigger waits for, in -1 to 1. (scope-tui counts it
    /// in raw 16-bit units, which only its PulseAudio source delivers; the
    /// samples here are always scaled.)
    pub threshold: f64,
    /// How many samples past the crossing must stay past it (debounce).
    pub depth: u32,
    pub peaks: bool,
}

impl Default for Oscilloscope {
    fn default() -> Self {
        Oscilloscope {
            triggering: false,
            falling_edge: false,
            threshold: 0.0,
            depth: 1,
            peaks: true,
        }
    }
}

/// The spectroscope's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Spectroscope {
    /// Buffers averaged into each spectrum.
    pub average: u32,
    /// Apply a Hann window first.
    pub window: bool,
    pub log_y: bool,
    pub phase_diff: bool,
}

impl Default for Spectroscope {
    fn default() -> Self {
        Spectroscope {
            average: 1,
            window: false,
            log_y: true,
            phase_diff: false,
        }
    }
}

/// One set of points to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct DataSet {
    /// Shown in the legend, if any.
    pub name: Option<String>,
    pub points: Vec<(f64, f64)>,
    pub scatter: bool,
    pub style: String,
}

impl DataSet {
    fn new(name: Option<&str>, points: Vec<(f64, f64)>, scatter: bool, style: &str) -> DataSet {
        DataSet {
            name: name.map(str::to_string),
            points,
            scatter,
            style: style.to_string(),
        }
    }

    /// A reference line, in the axis colour.
    fn reference(g: &Graph, a: (f64, f64), b: (f64, f64)) -> DataSet {
        DataSet::new(None, vec![a, b], false, &g.axis)
    }
}

/// What a mode draws: data sets over an x and a y range, with axis titles.
#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    pub sets: Vec<DataSet>,
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub x_title: &'static str,
    pub y_title: &'static str,
}

fn channel_name(index: usize) -> String {
    match index {
        0 => "L".into(),
        1 => "R".into(),
        n => n.to_string(),
    }
}

/// Whether the signal crosses `threshold` at `index` and stays past it for
/// `depth` samples, rising (or falling).
pub fn triggered(data: &[f64], index: usize, threshold: f64, depth: u32, falling: bool) -> bool {
    let depth = depth.max(1) as usize;
    if data.len() < index + 1 + depth {
        return false;
    }
    let after = &data[index + 1..=index + depth];
    if falling {
        data[index] >= threshold && after.iter().all(|s| *s < threshold)
    } else {
        data[index] <= threshold && after.iter().all(|s| *s > threshold)
    }
}

/// The oscilloscope: each channel against time, from the trigger point if
/// triggering.
pub fn oscilloscope(g: &Graph, o: &Oscilloscope, data: &Matrix) -> Plot {
    let mut sets = Vec::new();
    if g.references {
        sets.push(DataSet::reference(g, (0.0, 0.0), (g.samples as f64, 0.0)));
    }
    let offset = match data.first() {
        Some(first) if o.triggering => (0..first.len())
            .find(|&i| triggered(first, i, o.threshold, o.depth, o.falling_edge))
            .unwrap_or(first.len()),
        _ => 0,
    };
    if o.triggering {
        sets.push(DataSet::new(
            Some("T"),
            vec![(0.0, o.threshold)],
            true,
            &g.labels,
        ));
    }
    for (n, channel) in data.iter().enumerate().rev() {
        if o.peaks {
            let min = channel.iter().copied().fold(0.0, f64::min);
            let max = channel.iter().copied().fold(0.0, f64::max);
            sets.push(DataSet::new(
                None,
                vec![(0.0, min), (0.0, max)],
                true,
                g.palette(n),
            ));
        }
        let points = channel
            .iter()
            .enumerate()
            .skip(offset)
            .map(|(i, s)| ((i - offset) as f64, *s))
            .collect();
        sets.push(DataSet::new(
            Some(&channel_name(n)),
            points,
            g.scatter,
            g.palette(n),
        ));
    }
    Plot {
        sets,
        x: (0.0, g.samples as f64),
        y: (-g.scale, g.scale),
        x_title: "time -",
        y_title: "| amplitude",
    }
}

/// The oscilloscope's header: the trigger, or `live`.
pub fn oscilloscope_header(o: &Oscilloscope) -> String {
    if !o.triggering {
        return "live".into();
    }
    let depth = if o.depth > 1 {
        format!(":{}", o.depth)
    } else {
        String::new()
    };
    let edge = if o.falling_edge { "v" } else { "^" };
    format!("{edge} {:.2}{depth} trigger", o.threshold)
}

/// The vectorscope: each pair of channels as x and y, the older half of
/// the buffer in one colour and the newer in the next.
pub fn vectorscope(g: &Graph, data: &Matrix) -> Plot {
    let mut sets = Vec::new();
    if g.references {
        sets.push(DataSet::reference(g, (-g.scale, 0.0), (g.scale, 0.0)));
        sets.push(DataSet::reference(g, (0.0, -g.scale), (0.0, g.scale)));
    }
    for (n, pair) in data.chunks(2).enumerate() {
        let points: Vec<(f64, f64)> = match pair {
            [x, y] => x.iter().zip(y).map(|(a, b)| (*a, *b)).collect(),
            [x] => x.iter().enumerate().map(|(i, a)| (*a, i as f64)).collect(),
            _ => continue,
        };
        let points: Vec<(f64, f64)> = points.into_iter().take(g.samples as usize + 1).collect();
        let pivot = points.len() / 2;
        sets.push(DataSet::new(
            Some(&channel_name(n * 2 + 1)),
            points[pivot..].to_vec(),
            g.scatter,
            g.palette(n * 2 + 1),
        ));
        sets.push(DataSet::new(
            Some(&channel_name(n * 2)),
            points[..pivot].to_vec(),
            g.scatter,
            g.palette(n * 2),
        ));
    }
    Plot {
        sets,
        x: (-g.scale, g.scale),
        y: (-g.scale, g.scale),
        x_title: "left -",
        y_title: "| right",
    }
}

/// `samples` under a Hann window.
pub fn hann(samples: &[f64]) -> Vec<f64> {
    let n = samples.len() as f64;
    samples
        .iter()
        .enumerate()
        .map(|(i, s)| s * 0.5 * (1.0 - (2.0 * PI * i as f64 / n).cos()))
        .collect()
}

/// The discrete Fourier transform of `samples`, zero-padded to a power of
/// two (an in-place radix-2 FFT, so the example needs no dependency), as
/// (real, imaginary) pairs.
pub fn fft(samples: &[f64]) -> Vec<(f64, f64)> {
    let n = samples.len().max(1).next_power_of_two();
    let mut data: Vec<(f64, f64)> = samples.iter().map(|s| (*s, 0.0)).collect();
    data.resize(n, (0.0, 0.0));
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits).min(usize::BITS - 1);
        if bits > 0 && i < j {
            data.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (wr, wi) = ((angle * k as f64).cos(), (angle * k as f64).sin());
                let (ar, ai) = data[start + k];
                let (br, bi) = data[start + k + len / 2];
                let (tr, ti) = (br * wr - bi * wi, br * wi + bi * wr);
                data[start + k] = (ar + tr, ai + ti);
                data[start + k + len / 2] = (ar - tr, ai - ti);
            }
        }
        len *= 2;
    }
    data
}

/// The spectroscope: each channel's spectrum, averaged over the last
/// buffers, against a logarithmic frequency axis.
pub fn spectroscope(g: &Graph, s: &Spectroscope, history: &[VecDeque<Vec<f64>>]) -> Plot {
    let upper = g.scale * 7.5;
    let mut sets = Vec::new();
    if g.references {
        sets.push(DataSet::reference(
            g,
            (0.0, 0.0),
            ((g.samples.max(1) as f64).ln(), 0.0),
        ));
        // A line at every 10, 20 … 90 Hz, 100 … 900 Hz, 1 … 10 kHz, and 20 kHz.
        for decade in [10.0, 100.0, 1000.0] {
            for step in 1..=9 {
                let hz = decade * step as f64;
                if hz >= 20.0 {
                    sets.push(DataSet::reference(g, (hz.ln(), 0.0), (hz.ln(), upper)));
                }
            }
        }
        for hz in [10000.0f64, 20000.0] {
            sets.push(DataSet::reference(g, (hz.ln(), 0.0), (hz.ln(), upper)));
        }
    }
    let mut phases = Vec::new();
    for (n, queue) in history.iter().enumerate().rev() {
        let mut chunk: Vec<f64> = queue.iter().flatten().copied().collect();
        if chunk.is_empty() {
            continue;
        }
        if s.window {
            chunk = hann(&chunk);
        }
        let peak = chunk.iter().copied().fold(1.0, f64::max);
        chunk.iter_mut().for_each(|x| *x /= peak);
        let bins = fft(&chunk);
        let resolution = g.rate as f64 / bins.len() as f64;
        let half = &bins[..bins.len() / 2];
        let points = half
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, (re, im))| {
                let magnitude = (re * re + im * im).sqrt();
                let level = if s.log_y { magnitude.ln() } else { magnitude };
                ((i as f64 * resolution).ln(), level)
            })
            .collect();
        if s.phase_diff {
            phases.push((
                resolution,
                half.iter()
                    .map(|(re, im)| re.atan2(*im))
                    .collect::<Vec<_>>(),
            ));
        }
        sets.push(DataSet::new(
            Some(&channel_name(n)),
            points,
            g.scatter,
            g.palette(n),
        ));
    }
    if s.phase_diff && phases.len() >= 2 {
        let (resolution, right) = &phases[0];
        let (_, left) = &phases[1];
        let points = left
            .iter()
            .zip(right)
            .enumerate()
            .skip(1)
            .map(|(i, (l, r))| ((i as f64 * resolution).ln(), (l - r).abs()))
            .collect();
        sets.insert(
            0,
            DataSet::new(Some("phase diff"), points, true, g.palette(history.len())),
        );
    }
    let top = (g.samples as f64 / g.width.max(1) as f64 * 20000.0).max(21.0);
    Plot {
        sets,
        x: (20f64.ln(), top.ln()),
        y: (0.0, upper),
        x_title: "frequency -",
        y_title: if s.log_y { "| level" } else { "| amplitude" },
    }
}

/// The spectroscope's header: the averaging, the window and the bin width.
pub fn spectroscope_header(g: &Graph, s: &Spectroscope) -> String {
    let window = if s.window { "-|-" } else { "---" };
    let samples = (g.width * s.average.max(1)).max(1) as f64;
    let bins = g.rate as f64 / samples;
    if s.average <= 1 {
        format!("live  {window}  {bins:.3}Hz bins")
    } else {
        let seconds = samples / g.rate.max(1) as f64;
        format!(
            "{}x avg ({seconds:.1}s)  {window}  {bins:.3}Hz bins",
            s.average
        )
    }
}

/// Clip the segment `a`–`b` to the box `[0, w] × [0, h]` (Liang–Barsky).
fn clip(a: (f64, f64), b: (f64, f64), w: f64, h: f64) -> Option<((f64, f64), (f64, f64))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for (p, q) in [(-dx, a.0), (dx, w - a.0), (-dy, a.1), (dy, h - a.1)] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then_some((
        (a.0 + t0 * dx, a.1 + t0 * dy),
        (a.0 + t1 * dx, a.1 + t1 * dy),
    ))
}

/// Draw `plot` in `width` x `height` cells as ratatui's `Chart` lays one
/// out: the y axis's title above it, the x axis along the bottom with its
/// title at the right, and a legend at the top right. Braille dots, 2x4 a
/// cell, or one dot a cell without Braille.
pub fn draw(
    plot: &Plot,
    g: &Graph,
    console: &Console,
    width: u16,
    height: u16,
) -> Vec<Vec<Segment>> {
    let (w, h) = (width as usize, height as usize);
    if w < 3 || h < 2 {
        return Vec::new();
    }
    let mut styles: Vec<Option<Style>> = Vec::new();
    let mut style_of = |spec: &str| -> usize {
        styles.push(Style::parse(spec).ok());
        styles.len() - 1
    };
    let mut grid: Vec<Vec<(char, Option<usize>)>> = vec![vec![(' ', None); w]; h];
    let labels = style_of(&g.labels);
    let axis = style_of(&g.axis);
    let top = usize::from(g.show_ui);
    let bottom = h - 1;
    // The axes, and their titles.
    for row in grid.iter_mut().take(bottom).skip(top) {
        row[0] = ('│', Some(axis));
    }
    grid[bottom][0] = ('└', Some(axis));
    for cell in &mut grid[bottom][1..] {
        *cell = ('─', Some(axis));
    }
    if g.show_ui {
        for (i, c) in plot.y_title.chars().take(w).enumerate() {
            grid[0][i] = (c, Some(labels));
        }
        let title: Vec<char> = plot.x_title.chars().collect();
        if title.len() + 2 < w {
            for (i, c) in title.iter().enumerate() {
                grid[bottom][w - title.len() + i] = (*c, Some(labels));
            }
        }
    }
    // The data, on a canvas right of the y axis and above the x axis.
    let (cw, ch) = (w - 1, bottom - top);
    if ch > 0 {
        let (sx, sy) = if g.braille { (2.0, 4.0) } else { (1.0, 1.0) };
        let (dw, dh) = (cw as f64 * sx - 1.0, ch as f64 * sy - 1.0);
        let span_x = (plot.x.1 - plot.x.0).max(f64::EPSILON);
        let span_y = (plot.y.1 - plot.y.0).max(f64::EPSILON);
        let at = |(x, y): (f64, f64)| ((x - plot.x.0) / span_x * dw, (plot.y.1 - y) / span_y * dh);
        let mut canvas = DotCanvas::new(cw, ch);
        let mut cells: Vec<Vec<Option<usize>>> = vec![vec![None; cw]; ch];
        for set in &plot.sets {
            let layer = style_of(&set.style);
            let mut dot = |x: f64, y: f64| {
                let (x, y) = (x.round() as i64, y.round() as i64);
                if g.braille {
                    canvas.set(x, y, layer);
                } else if (0..cw as i64).contains(&x) && (0..ch as i64).contains(&y) {
                    cells[y as usize][x as usize] = Some(layer);
                }
            };
            let finite = set
                .points
                .iter()
                .filter(|(x, y)| x.is_finite() && y.is_finite())
                .map(|p| at(*p));
            if set.scatter {
                finite.for_each(|(x, y)| dot(x, y));
                continue;
            }
            let points: Vec<(f64, f64)> = finite.collect();
            if points.len() == 1 {
                dot(points[0].0, points[0].1);
            }
            for pair in points.windows(2) {
                let Some((a, b)) = clip(pair[0], pair[1], dw, dh) else {
                    continue;
                };
                let steps = (b.0 - a.0).abs().max((b.1 - a.1).abs()).ceil().max(1.0);
                for step in 0..=steps as usize {
                    let t = step as f64 / steps;
                    dot(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                }
            }
        }
        for row in 0..ch {
            for col in 0..cw {
                let cell = if g.braille {
                    match canvas.cell(col, row) {
                        (c, Some(layer)) => Some((c, layer)),
                        _ => None,
                    }
                } else {
                    cells[row][col].map(|layer| ('•', layer))
                };
                if let Some((c, layer)) = cell {
                    grid[top + row][1 + col] = (c, Some(layer));
                }
            }
        }
        // The legend: the named sets, boxed, at the top right.
        let named: Vec<(&str, usize)> = plot
            .sets
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.name.as_deref().map(|n| (n, i)))
            .collect();
        let inner = named
            .iter()
            .map(|(n, _)| n.chars().count())
            .max()
            .unwrap_or(0);
        if g.show_ui && !named.is_empty() && inner + 3 < cw && named.len() + 2 <= ch {
            let left = w - inner - 2;
            let set_styles: Vec<usize> = plot.sets.iter().map(|s| style_of(&s.style)).collect();
            let mut put = |row: usize, col: usize, c: char, style: Option<usize>| {
                grid[top + row][col] = (c, style)
            };
            put(0, left, '┌', Some(axis));
            put(0, w - 1, '┐', Some(axis));
            put(named.len() + 1, left, '└', Some(axis));
            put(named.len() + 1, w - 1, '┘', Some(axis));
            for col in left + 1..w - 1 {
                put(0, col, '─', Some(axis));
                put(named.len() + 1, col, '─', Some(axis));
            }
            for (row, (name, set)) in named.iter().enumerate() {
                put(row + 1, left, '│', Some(axis));
                put(row + 1, w - 1, '│', Some(axis));
                let mut chars = name.chars();
                for col in left + 1..w - 1 {
                    let c = chars.next().unwrap_or(' ');
                    put(row + 1, col, c, Some(set_styles[*set]));
                }
            }
        }
    }
    let _ = console;
    grid.into_iter()
        .map(|row| {
            let mut line: Vec<Segment> = Vec::new();
            let mut run = String::new();
            let mut current: Option<usize> = None;
            for (c, style) in row {
                let style = if c == ' ' { None } else { style };
                if style != current && !run.is_empty() {
                    line.push(Segment::new(
                        std::mem::take(&mut run),
                        current.and_then(|s| styles[s].clone()),
                    ));
                }
                current = style;
                run.push(c);
            }
            line.push(Segment::new(run, current.and_then(|s| styles[s].clone())));
            line
        })
        .collect()
}

/// The options scope-tui takes on its command line.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub channels: usize,
    pub buffer: u32,
    pub rate: u32,
    pub scale: f64,
    pub scatter: bool,
    pub references: bool,
    pub show_ui: bool,
    pub braille: bool,
    pub palette: Vec<String>,
    pub labels: String,
    pub axis: String,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            channels: 2,
            buffer: 2048,
            rate: 48000,
            scale: 1.0,
            scatter: false,
            references: true,
            show_ui: true,
            braille: true,
            palette: ["red", "yellow", "cyan", "magenta"]
                .map(String::from)
                .to_vec(),
            labels: "cyan".into(),
            axis: "bright_black".into(),
        }
    }
}

/// The buffer size whose length is one period of `note` (`A4`, `C#3`,
/// `Eb`), so a tone at that pitch stands still in the oscilloscope.
pub fn tune(note: &str, rate: u32) -> Option<u32> {
    let note = note.trim();
    let split = note
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(note.len());
    let semitone = match &note[..split] {
        "C" => 0,
        "C#" | "Db" => 1,
        "D" => 2,
        "D#" | "Eb" => 3,
        "E" => 4,
        "F" => 5,
        "F#" | "Gb" => 6,
        "G" => 7,
        "G#" | "Ab" => 8,
        "A" => 9,
        "A#" | "Bb" => 10,
        "B" => 11,
        _ => return None,
    };
    let octave: i32 = note[split..].parse().unwrap_or(0);
    let hz = 440.0 * 2f64.powf((semitone - 9) as f64 / 12.0 + (octave - 4) as f64);
    Some((rate as f64 / hz).round().max(1.0) as u32)
}

/// Clamp `value + step * magnitude` to `range`.
fn nudge(value: f64, step: f64, magnitude: f64, range: (f64, f64)) -> f64 {
    (value + step * magnitude).clamp(range.0, range.1)
}

/// The modifiers scope-tui scales a step by: shift ×10, ctrl ×5, alt ×⅕.
const MAGNITUDES: [(&str, f64); 4] = [("", 1.0), ("shift+", 10.0), ("ctrl+", 5.0), ("alt+", 0.2)];

/// The app, drawing what `source` delivers.
pub fn scope_app(options: Options, mut source: Box<dyn Source>, frame_every: Duration) -> App {
    App::new(move || {
        let graph = signal(Graph {
            samples: options.buffer,
            width: options.buffer,
            rate: options.rate,
            scale: options.scale,
            scatter: options.scatter,
            references: options.references,
            show_ui: options.show_ui,
            pause: false,
            braille: options.braille,
            palette: options.palette.clone(),
            labels: options.labels.clone(),
            axis: options.axis.clone(),
        });
        let mode = signal(Mode::Oscilloscope);
        let osc = signal(Oscilloscope::default());
        let spec = signal(Spectroscope::default());
        let frame = signal(Matrix::new());
        let history = signal(Vec::<VecDeque<Vec<f64>>>::new());
        let ended = signal(false);
        let fps = signal(0u32);

        // Take the source's newest buffer at its own pace. A paused scope
        // still reads its source, as scope-tui does, but shows nothing new.
        let shown = Rc::new(Cell::new(0u32));
        let count = shown.clone();
        every(frame_every, move |_| match source.pull() {
            Pull::Frame(data) if !graph.with_untracked(|g| g.pause) => {
                let average = spec.with_untracked(|s| s.average.max(1) as usize);
                history.update(|history| {
                    history.resize_with(data.len(), VecDeque::new);
                    for (queue, channel) in history.iter_mut().zip(&data) {
                        queue.push_back(channel.clone());
                        while queue.len() > average {
                            queue.pop_front();
                        }
                    }
                });
                frame.set(data);
                count.set(count.get() + 1);
            }
            Pull::End => ended.set(true),
            _ => {}
        });
        every(Duration::from_secs(1), move |_| fps.set(shown.replace(0)));

        let plot = leaf(move |console, width, height| {
            let g = graph.get();
            let plot = match mode.get() {
                Mode::Oscilloscope => osc.with(|o| frame.with(|f| oscilloscope(&g, o, f))),
                Mode::Vectorscope => frame.with(|f| vectorscope(&g, f)),
                Mode::Spectroscope => spec.with(|s| history.with(|h| spectroscope(&g, s, h))),
            };
            draw(&plot, &g, console, width, height)
        })
        .name("plot");

        let shown_header = switch(
            move || graph.with(|g| g.show_ui),
            move |show| {
                if show {
                    header(graph, mode, osc, spec, ended, fps)
                } else {
                    column([])
                }
            },
        );

        let mut root = column([shown_header.auto(), plot])
            .on_key("q ctrl+q ctrl+w", |cx| cx.quit())
            .on_key("space", move |_| graph.update(|g| g.pause = !g.pause))
            .on_key("s", move |_| graph.update(|g| g.scatter = !g.scatter))
            .on_key("h", move |_| graph.update(|g| g.show_ui = !g.show_ui))
            .on_key("r", move |_| graph.update(|g| g.references = !g.references))
            .on_key("tab", move |_| mode.update(|m| *m = m.next()))
            .on_key("esc", move |_| {
                graph.update(|g| {
                    g.samples = g.width;
                    g.scale = 1.0;
                });
                osc.update(|o| o.triggering = false);
            })
            .on_key("t", move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.triggering = !o.triggering);
                }
            })
            .on_key("e", move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.falling_edge = !o.falling_edge);
                }
            })
            .on_key("p", move |_| match mode.get_untracked() {
                Mode::Oscilloscope => osc.update(|o| o.peaks = !o.peaks),
                Mode::Spectroscope => spec.update(|s| s.phase_diff = !s.phase_diff),
                Mode::Vectorscope => {}
            })
            .on_key("w", move |_| {
                if mode.get_untracked() == Mode::Spectroscope {
                    spec.update(|s| s.window = !s.window);
                }
            })
            .on_key("l", move |_| {
                if mode.get_untracked() == Mode::Spectroscope {
                    spec.update(|s| s.log_y = !s.log_y);
                }
            })
            .on_key("?", |cx| cx.modal(Size::Auto, Size::Auto, help));
        for (key, step) in [("=", 1), ("-", -1), ("+", 10), ("_", -10)] {
            root = root.on_key(&format!("{key:?}"), move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.depth = (o.depth as i64 + step).clamp(1, 65535) as u32);
                }
            });
        }
        for (modifier, magnitude) in MAGNITUDES {
            root = root
                .on_key(&format!("{modifier}up"), move |_| {
                    graph.update(|g| g.scale = nudge(g.scale, 0.01, magnitude, (0.0, 10.0)))
                })
                .on_key(&format!("{modifier}down"), move |_| {
                    graph.update(|g| g.scale = nudge(g.scale, -0.01, magnitude, (0.0, 10.0)))
                })
                .on_key(&format!("{modifier}right"), move |_| {
                    graph.update(|g| {
                        let top = g.width as f64 * 2.0;
                        g.samples = nudge(g.samples as f64, 25.0, magnitude, (0.0, top)) as u32;
                    })
                })
                .on_key(&format!("{modifier}left"), move |_| {
                    graph.update(|g| {
                        let top = g.width as f64 * 2.0;
                        g.samples = nudge(g.samples as f64, -25.0, magnitude, (0.0, top)) as u32;
                    })
                })
                .on_key(&format!("{modifier}pageup"), move |_| {
                    page(mode, osc, spec, 1.0, magnitude)
                })
                .on_key(&format!("{modifier}pagedown"), move |_| {
                    page(mode, osc, spec, -1.0, magnitude)
                });
        }
        root
    })
}

/// PgUp (`sign` 1) or PgDn (-1): the oscilloscope's trigger threshold, or
/// the spectroscope's averaging.
fn page(
    mode: Signal<Mode>,
    osc: Signal<Oscilloscope>,
    spec: Signal<Spectroscope>,
    sign: f64,
    magnitude: f64,
) {
    match mode.get_untracked() {
        Mode::Oscilloscope => {
            osc.update(|o| o.threshold = nudge(o.threshold, 0.01 * sign, magnitude, (-1.0, 1.0)))
        }
        Mode::Spectroscope => {
            spec.update(|s| s.average = nudge(s.average as f64, sign, 1.0, (1.0, 65535.0)) as u32)
        }
        Mode::Vectorscope => {}
    }
}

/// The header: a row of cells sized as scope-tui's table columns, each
/// redrawn only when what it shows changes.
fn header(
    graph: Signal<Graph>,
    mode: Signal<Mode>,
    osc: Signal<Oscilloscope>,
    spec: Signal<Spectroscope>,
    ended: Signal<bool>,
    fps: Signal<u32>,
) -> Node {
    let labels = memo(move || graph.with(|g| g.labels.clone()));
    let cell = move |percent: u16, markup: Box<dyn Fn() -> String>| {
        text(move || format!("[{}]{}", labels.get(), markup())).percent(percent)
    };
    row([
        text(move || {
            let colour = graph.with(|g| g.palette(0).to_string());
            format!("[bold {colour}]{}::scope-tui[/]", mode.get().name())
        })
        .percent(35),
        cell(
            25,
            Box::new(move || {
                if ended.get() {
                    return "end of input".into();
                }
                match mode.get() {
                    Mode::Oscilloscope => osc.with(oscilloscope_header),
                    Mode::Vectorscope => "live".into(),
                    Mode::Spectroscope => graph.with(|g| spec.with(|s| spectroscope_header(g, s))),
                }
            }),
        ),
        cell(
            7,
            Box::new(move || format!("-{:.2}x+", graph.with(|g| g.scale))),
        ),
        cell(
            13,
            Box::new(move || graph.with(|g| format!("{}/{} spf", g.samples, g.width))),
        ),
        cell(6, Box::new(move || format!("{}fps", fps.get()))),
        cell(
            6,
            Box::new(move || {
                (if graph.with(|g| g.scatter) {
                    "***"
                } else {
                    "---"
                })
                .into()
            }),
        ),
        cell(
            6,
            Box::new(move || (if graph.with(|g| g.pause) { "||" } else { "|>" }).into()),
        ),
    ])
    // ratatui's tables leave a column between cells.
    .gap(1)
    .fixed(1)
}

/// The keys, in a modal.
fn help() -> Node {
    label(
        "[b]q[/]          quit\n\
         [b]space[/]      pause\n\
         [b]tab[/]        oscilloscope, vectorscope, spectroscope\n\
         [b]s[/]          scatter or lines\n\
         [b]h[/]          hide the interface\n\
         [b]r[/]          reference lines\n\
         [b]↑ ↓[/]        scale\n\
         [b]← →[/]        samples shown\n\
         [b]esc[/]        reset the view\n\
         [dim]shift ×10 · ctrl ×5 · alt ×⅕[/]\n\
         \n\
         [b]oscilloscope[/]\n\
         [b]t[/] trigger · [b]e[/] edge · [b]p[/] peaks\n\
         [b]pgup pgdn[/] threshold · [b]- = _ +[/] debounce\n\
         [b]spectroscope[/]\n\
         [b]pgup pgdn[/] averaging · [b]l[/] log · [b]w[/] window\n\
         [b]p[/] phase difference",
    )
    .padding(0, 1)
    .panel("Keys")
    .on_key("esc ? q", |cx| cx.pop())
}

/// What `main` parsed: the options, the source, and whether to play a
/// file at its sample rate.
#[derive(Debug, PartialEq)]
pub struct Command {
    pub options: Options,
    /// `None` for the test signal, `Some("-")` for stdin.
    pub file: Option<String>,
    pub limit_rate: bool,
    pub still: bool,
}

/// Parse scope-tui's command line (without the program's name).
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = Options::default();
    let (mut file, mut limit_rate, mut still, mut note) = (None, false, false, None);
    let mut args = args.iter();
    let number = |name: &str, value: Option<&String>| -> Result<f64, String> {
        value
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} takes a number"))
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--channels" => options.channels = number(arg, args.next())?.max(1.0) as usize,
            "-b" | "--buffer" => options.buffer = number(arg, args.next())?.max(16.0) as u32,
            "-r" | "--sample-rate" => options.rate = number(arg, args.next())?.max(1.0) as u32,
            "-s" | "--scale" => options.scale = number(arg, args.next())?,
            "-t" | "--tune" => note = args.next().cloned(),
            "--scatter" => options.scatter = true,
            "--no-reference" => options.references = false,
            "--no-ui" => options.show_ui = false,
            "--no-braille" => options.braille = false,
            "--palette-color" => {
                let list = args.next().ok_or("--palette-color takes colours")?;
                options.palette = list.split(',').map(|c| c.trim().to_lowercase()).collect();
            }
            "--labels-color" => {
                options.labels = args
                    .next()
                    .ok_or("--labels-color takes a colour")?
                    .to_lowercase()
            }
            "--axis-color" => {
                options.axis = args
                    .next()
                    .ok_or("--axis-color takes a colour")?
                    .to_lowercase()
            }
            "-l" | "--limit-rate" => limit_rate = true,
            "--still" => still = true,
            "demo" => {}
            "file" => {
                file = Some(
                    args.next()
                        .ok_or("file takes a path, or - for stdin")?
                        .clone(),
                )
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    if let Some(note) = note {
        match tune(&note, options.rate) {
            Some(buffer) => options.buffer = buffer,
            None => eprintln!("[!] Unrecognized note '{note}', ignoring option"),
        }
    }
    Ok(Command {
        options,
        file,
        limit_rate,
        still,
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command =
        parse(&args).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let options = command.options.clone();
    let period = Duration::from_secs_f64(options.buffer as f64 / options.rate as f64);
    let (source, every): (Box<dyn Source>, Duration) = match &command.file {
        None => (
            Box::new(Demo::new(
                options.channels,
                options.buffer as usize,
                command.still,
            )),
            period,
        ),
        Some(path) => {
            let reader: Box<dyn Read + Send> = if path == "-" {
                Box::new(std::io::stdin())
            } else {
                Box::new(std::fs::File::open(path)?)
            };
            let pcm = Pcm::new(reader, options.channels, options.buffer as usize);
            let pace = command.limit_rate.then_some(period);
            // Look twice a buffer, so a buffer waits at most half of one.
            (Box::new(Threaded::new(pcm, pace)), period / 2)
        }
    };
    scope_app(options, source, every.max(Duration::from_millis(5))).run()
}

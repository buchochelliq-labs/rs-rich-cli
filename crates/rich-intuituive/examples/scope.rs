//! An oscilloscope, vectorscope and spectroscope for the terminal in the
//! style of [scope-tui](https://github.com/alemidev/scope-tui), a ratatui
//! app, rebuilt on intuiTUIve. It reuses none of scope-tui's code: it is a
//! rebuild of the behaviour, showing how an app that redraws twenty or more
//! times a second maps. The plot is rich-ext's `Chart`, which lays out and
//! draws as ratatui's does; the header is a row of `cell`s laid out by
//! ratatui's solver, as scope-tui's one-row `Table` is.
//!
//! Ported from scope-tui 0.3.5 (<https://github.com/alemidev/scope-tui>),
//! by alemi, MIT licence. Its design, keys, options and display layout are
//! alemi's; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example scope --features pcm              # a test signal
//!     cargo run -p rs-rich-intuituive --example scope --features pcm -- file PATH # raw PCM
//!     parec --format=s16le --rate=48000 --channels=2 \
//!         | cargo run -p rs-rich-intuituive --example scope --features pcm -- file -
//!     cargo run -p rs-rich-intuituive --example scope --features audio -- audio
//!
//! Options, as scope-tui's: `-c N` channels, `-b SIZE` buffer, `-r HZ`
//! sample rate, `-t NOTE` tune the buffer to a note, `-s X` scale,
//! `--scatter`, `--no-reference`, `--no-ui`, `--no-braille`,
//! `--palette-color a,b,…`, `--labels-color c`, `--axis-color c` (colours
//! as ratatui reads them: `red`, `LightRed`, `DarkGray`, `#ff8800`, `208`).
//! Sources, as scope-tui's subcommands: `file PATH [-l]` plays a file or a
//! pipe (at its sample rate with `-l`); with the crate's `audio` feature,
//! `audio [DEVICE] [--list] [--timeout S]` captures a sound card and `pulse
//! [DEVICE]` PulseAudio's default source, through ALSA. Without a source it
//! draws a test signal; `--still` makes it stand still. `--backend NAME`
//! (or `INTUITUIVE_BACKEND=NAME`) drives the terminal with termion or
//! termwiz instead of crossterm, when built with the crate's feature of
//! that name:
//!
//!     cargo run -p rs-rich-intuituive --example scope --features pcm,termwiz -- --backend termwiz
//!
//! q or ctrl+c quits · space pauses · tab changes mode · s scatter · h
//! hides the interface · r reference lines · ↑ ↓ scale · ← → samples ·
//! esc resets · shift ×10, ctrl ×5, alt ×⅕ · ? lists every key.

use std::collections::VecDeque;
use std::f64::consts::PI;
use std::io::Read;
use std::time::{Duration, Instant};

#[cfg(feature = "audio")]
use intuituive::audio;
use intuituive::interact::BackendKind;
use intuituive::prelude::*;
use intuituive::widgets::cell;
use intuituive::{Latest, Solver};
use rich_ext::chart::{
    Axis, AxisScale, Chart, Dataset, GraphType, Labels, Layering, LegendPosition, Marker,
    OutOfBounds,
};
use rich_ext::dsp;
use rich_ratatui::style_from_color_name;

/// One buffer of samples, one row per channel, each in -1 to 1.
pub type Matrix = Vec<Vec<f64>>;

/// A test signal: two tones a fifth apart (2 and 3 cycles a buffer), one
/// per channel, each with overtones, faint high partials and a little
/// noise, so the vectorscope draws a Lissajous figure and the spectroscope
/// peaks along a noise floor. Every tone is a whole number of cycles a
/// buffer; unless `still`, the right channel's phase drifts from one buffer
/// to the next and the figure turns. Still, every buffer is the same, noise
/// included. It never ends.
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

impl Iterator for Demo {
    type Item = Matrix;

    fn next(&mut self) -> Option<Matrix> {
        let n = self.buffer as f64;
        let frame = if self.still { 0 } else { self.frame };
        self.frame += 1;
        let drift = frame as f64 * 0.05;
        // A tone `cycles` times per buffer, at sample `i`.
        let tone =
            |cycles: f64, i: usize, phase: f64| (2.0 * PI * cycles * i as f64 / n + phase).sin();
        // White noise in -1 to 1 from xorshift, seeded by the frame.
        let mut seed = frame.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
        let mut noise = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 52) as f64 - 1.0
        };
        let mut channel = |c: usize| -> Vec<f64> {
            // Noise through a one-pole low-pass (about 300 Hz at 48 kHz):
            // a smooth wander that falls away with frequency, as music's
            // spectrum does.
            let mut low = 0.0;
            (0..self.buffer)
                .map(|i| {
                    let signal = match c {
                        0 => 0.6 * tone(2.0, i, 0.0) + 0.2 * tone(6.0, i, 0.0),
                        1 => 0.6 * tone(3.0, i, drift) + 0.15 * tone(9.0, i, 3.0 * drift),
                        c => 0.5 * tone(c as f64 + 2.0, i, 0.0),
                    };
                    // A fading series of high partials, for the spectroscope.
                    let partials: f64 = (1..=24)
                        .map(|k| 0.012 / k as f64 * tone((17 * k + 5 * c) as f64, i, k as f64))
                        .sum();
                    low += 0.04 * (0.26 * noise() - low);
                    signal + partials + low + 0.01 * noise()
                })
                .collect()
        };
        Some((0..self.channels).map(&mut channel).collect())
    }
}

/// Signed 16-bit little-endian PCM, channels interleaved (`L R L R …`), as
/// `parec --format=s16le` writes it: a file, a pipe or stdin, a buffer at
/// a time. A short last buffer is padded with silence; then it ends.
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

impl<R: Read> Iterator for Pcm<R> {
    type Item = Matrix;

    fn next(&mut self) -> Option<Matrix> {
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
            return None;
        }
        self.bytes[filled..].fill(0);
        Some(deinterleave(&self.bytes, self.channels))
    }
}

/// A buffer as the app gets it from a thread: numbered, so two that hold
/// the same samples (the still test signal, or silence) are still two
/// frames.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub number: u64,
    pub data: Matrix,
}

/// Where the app's buffers come from, started when the app is.
pub enum Input {
    /// Buffers made on the app's own clock, one every period: the test
    /// signal. A timer, not a thread, so a headless test sees the same
    /// frames on every run.
    Clock(Box<dyn Iterator<Item = Matrix>>, Duration),
    /// Buffers read on a thread of their own (see [`play`]).
    Thread(Box<dyn FnOnce(Latest<Frame>) + Send>),
    /// A sound card, through intuiTUIve's `audio` feature (`None` for the
    /// default input).
    #[cfg(feature = "audio")]
    Audio(Option<String>),
}

/// Send `frames` to the app on a thread, one every `pace` if it is set (a
/// file played at its sample rate), else as fast as they come (a pipe). When they end, the scope goes on with
/// `silence`, a buffer every `period`, as scope-tui's file source does.
/// The newest buffer wins: one the app had no time for is dropped.
pub fn play(
    frames: impl Iterator<Item = Matrix> + Send + 'static,
    silence: Matrix,
    pace: Option<Duration>,
    period: Duration,
) -> Input {
    Input::Thread(Box::new(move |tx: Latest<Frame>| {
        let mut due = Instant::now();
        let silent = std::iter::repeat(silence).map(|data| (data, Some(period)));
        let frames = frames.map(|data| (data, pace)).chain(silent);
        for (number, (data, pace)) in (0..).zip(frames) {
            if !tx.send(Frame { number, data }) {
                return;
            }
            if let Some(pace) = pace {
                // Keep to the beat, however long a buffer took to make.
                due = (due + pace).max(Instant::now());
                std::thread::sleep(due.saturating_duration_since(Instant::now()));
            }
        }
    }))
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
    /// Console styles: the series' colours, the axis titles' and the
    /// reference lines'.
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

/// Full scale of a 16-bit sample: scope-tui counts its trigger threshold
/// in these units.
pub const FULL_SCALE: f64 = 32768.0;

/// The oscilloscope's own settings.
#[derive(Clone, Debug, PartialEq)]
pub struct Oscilloscope {
    pub triggering: bool,
    pub falling_edge: bool,
    /// The level the trigger waits for, as scope-tui counts it: in 16-bit
    /// units, 0 to 32768, stepped by 250.
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
    /// A logarithmic x axis (the spectroscope's frequencies).
    pub x_log: bool,
}

/// The oscilloscope's and the spectroscope's names for channels.
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
/// triggering, with each channel's peaks marked at the left edge.
pub fn oscilloscope(g: &Graph, o: &Oscilloscope, data: &Matrix) -> Plot {
    let mut sets = Vec::new();
    if g.references {
        sets.push(DataSet::reference(g, (0.0, 0.0), (g.samples as f64, 0.0)));
    }
    let level = o.threshold / FULL_SCALE;
    // A buffer that never crosses leaves nothing to draw, as in scope-tui.
    let offset = match data.first() {
        Some(first) if o.triggering => (0..first.len())
            .find(|&i| triggered(first, i, level, o.depth, o.falling_edge))
            .unwrap_or(first.len()),
        _ => 0,
    };
    if o.triggering {
        sets.push(DataSet::new(Some("T"), vec![(0.0, level)], true, &g.labels));
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
        x_log: false,
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
    format!("{edge} {:.0}{depth} trigger", o.threshold)
}

/// The vectorscope: each pair of channels as x and y, the older half of
/// the buffer in one colour and the newer in the next. Its legend numbers
/// the channels, as scope-tui's does.
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
        let (left, right) = (n * 2, n * 2 + 1);
        sets.push(DataSet::new(
            Some(&right.to_string()),
            points[pivot..].to_vec(),
            g.scatter,
            g.palette(right),
        ));
        sets.push(DataSet::new(
            Some(&left.to_string()),
            points[..pivot].to_vec(),
            g.scatter,
            g.palette(left),
        ));
    }
    Plot {
        sets,
        x: (-g.scale, g.scale),
        y: (-g.scale, g.scale),
        x_title: "left -",
        y_title: "| right",
        x_log: false,
    }
}

/// Where scope-tui draws its frequency reference lines: every 10 Hz from
/// 20 to 90, every 100 to 900, every 1 kHz to 10 kHz, and 20 kHz.
fn reference_frequencies() -> impl Iterator<Item = f64> {
    [10.0, 100.0, 1000.0]
        .into_iter()
        .flat_map(|decade| (1..=9).map(move |step| decade * step as f64))
        .filter(|hz| *hz >= 20.0)
        .chain([10000.0, 20000.0])
}

/// The spectroscope: each channel's spectrum, averaged over the last
/// buffers, against a logarithmic frequency axis. Every bin is a point,
/// the mirrored half above the Nyquist frequency too, as in scope-tui
/// (it shows once → widens the axis past it).
pub fn spectroscope(g: &Graph, s: &Spectroscope, history: &[VecDeque<Vec<f64>>]) -> Plot {
    let upper = g.scale * 7.5;
    let mut sets = Vec::new();
    if g.references {
        // scope-tui's zero line runs from 1 Hz to `samples` Hz.
        sets.push(DataSet::reference(
            g,
            (1.0, 0.0),
            (g.samples.max(1) as f64, 0.0),
        ));
        for hz in reference_frequencies() {
            sets.push(DataSet::reference(g, (hz, 0.0), (hz, upper)));
        }
    }
    // scope-tui's phase: atan2 with its arguments as it passes them.
    let phase = |(re, im): &dsp::Complex| {
        let p = re.atan2(*im);
        if p.is_normal() {
            p
        } else {
            0.0
        }
    };
    let mut phases = Vec::new();
    for (n, queue) in history.iter().enumerate().rev() {
        let mut chunk: Vec<f64> = queue.iter().flatten().copied().collect();
        if chunk.is_empty() {
            continue;
        }
        if s.window {
            chunk = dsp::hann(&chunk);
        }
        let peak = chunk.iter().copied().fold(1.0, f64::max);
        chunk.iter_mut().for_each(|x| *x /= peak);
        // The whole transform, not `dsp::spectrum`'s half up to the Nyquist
        // frequency: scope-tui plots every bin, the mirrored half too.
        let bins = dsp::fft(&chunk);
        let resolution = g.rate as f64 / bins.len() as f64;
        let points = bins
            .iter()
            .enumerate()
            .map(|(i, bin)| {
                let magnitude = dsp::magnitude(*bin);
                let level = if s.log_y { magnitude.ln() } else { magnitude };
                (i as f64 * resolution, level)
            })
            .collect();
        if s.phase_diff {
            phases.push((resolution, bins.iter().map(phase).collect::<Vec<_>>()));
        }
        sets.push(DataSet::new(
            Some(&channel_name(n)),
            points,
            g.scatter,
            g.palette(n),
        ));
    }
    if s.phase_diff && phases.len() >= 2 {
        // Channels in reverse: the right one first.
        let (resolution, right) = &phases[0];
        let (_, left) = &phases[1];
        let points = right
            .iter()
            .zip(left)
            .enumerate()
            .map(|(i, (r, l))| (i as f64 * resolution, (r - l).abs()))
            .collect();
        sets.insert(
            0,
            DataSet::new(Some("phase diff"), points, true, g.palette(history.len())),
        );
    }
    let top = (g.samples as f64 / g.width.max(1) as f64 * 20000.0).max(21.0);
    Plot {
        sets,
        x: (20.0, top),
        y: (0.0, upper),
        x_title: "frequency -",
        y_title: if s.log_y { "| level" } else { "| amplitude" },
        x_log: true,
    }
}

/// The spectroscope's header: the averaging, the window and the bin width.
pub fn spectroscope_header(g: &Graph, s: &Spectroscope) -> String {
    let window = if s.window { "-|-" } else { "---" };
    let samples = (g.width.max(1) * s.average.max(1)) as f64;
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

/// `plot` as rich-ext's chart, laid out as ratatui's `Chart`, which is
/// what scope-tui draws with: no axis lines (scope-tui's axes have no
/// labels), the y axis's title at the top left, the x axis's at the bottom
/// right, and the legend boxed at the top right. Braille dots, 2x4 a cell,
/// or one dot a cell without Braille.
pub fn chart(plot: &Plot, g: &Graph) -> Chart {
    let marker = if g.braille {
        Marker::Braille
    } else {
        Marker::Dot
    };
    let datasets = plot.sets.iter().map(|set| {
        let kind = if set.scatter {
            GraphType::Scatter
        } else {
            GraphType::Line
        };
        let dataset = Dataset::new(set.points.iter().copied())
            .graph_type(kind)
            .marker(marker)
            .style(set.style.clone());
        match &set.name {
            Some(name) => dataset.name(name.clone()),
            None => dataset,
        }
    });
    // No labels, so no axis lines. Titles only with the interface shown;
    // the legend always.
    let axis = |(min, max): (f64, f64), title: &str, scale: AxisScale| {
        let axis = Axis::default()
            .bounds(min, max)
            .scale(scale)
            .labels(Labels::None);
        if g.show_ui {
            axis.title(title).title_style(g.labels.clone())
        } else {
            axis
        }
    };
    let x_scale = if plot.x_log {
        AxisScale::Log
    } else {
        AxisScale::Linear
    };
    Chart::new(datasets)
        .x_axis(axis(plot.x, plot.x_title, x_scale))
        .y_axis(axis(plot.y, plot.y_title, AxisScale::Linear))
        .legend(LegendPosition::TopRight)
        // As ratatui 0.29 draws: its canvas leaves out a line with an end
        // outside the bounds, draws each dataset over the ones before (so
        // the trace breaks the reference lines it crosses), and draws the
        // legend's box in the chart's own style, the terminal's colours,
        // over what the datasets drew.
        .out_of_bounds(OutOfBounds::Drop)
        .layering(Layering::PerDataset)
        .legend_style("")
        .legend_clear(false)
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
    /// Console styles, read from ratatui's colour names.
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

/// The buffer size scope-tui's `--tune` picks for `note` (`A4`, `C#3`,
/// `Eb`): one period of the note at `rate`, from its table of octave-0
/// pitches doubled per octave, then nudged up while it is a multiple of
/// `channels × 2`.
pub fn tune(note: &str, rate: u32, channels: usize) -> Option<u32> {
    let note = note.trim();
    let split = note
        .find(|c: char| c.is_ascii_digit())
        .unwrap_or(note.len());
    let hz: f32 = match &note[..split] {
        "C" => 16.35,
        "C#" | "Db" => 17.32,
        "D" => 18.35,
        "D#" | "Eb" => 19.45,
        "E" => 20.60,
        "F" => 21.83,
        "F#" | "Gb" => 23.12,
        "G" => 24.50,
        "G#" | "Ab" => 25.96,
        "A" => 27.50,
        "A#" | "Bb" => 29.14,
        "B" => 30.87,
        _ => return None,
    };
    let octave: u32 = note[split..].parse().unwrap_or(0);
    let hz = (0..octave).fold(hz, |hz, _| hz * 2.0);
    let mut buffer = ((rate as f32 / hz).round() as u32).max(1);
    let align = channels as u32 * 2;
    while align > 0 && buffer.is_multiple_of(align) {
        buffer += 1;
    }
    Some(buffer)
}

/// Clamp `value + step * magnitude` to `range`.
fn nudge(value: f64, step: f64, magnitude: f64, range: (f64, f64)) -> f64 {
    (value + step * magnitude).clamp(range.0, range.1)
}

/// The modifiers scope-tui scales a step by: shift ×10, ctrl ×5, alt ×⅕.
const MAGNITUDES: [(&str, f64); 4] = [("", 1.0), ("shift+", 10.0), ("ctrl+", 5.0), ("alt+", 0.2)];

/// The app, drawing the buffers `input` delivers.
pub fn scope_app(options: Options, input: Input) -> App {
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
        let frames = signal(0u32);
        let fps = signal(0u32);

        // Each buffer the source delivers. Paused, the scope still takes
        // them and counts the frames, as scope-tui does, but shows nothing
        // new; an identical buffer (the still test signal) draws nothing.
        let take = move |data: Matrix| {
            frames.update(|n| *n += 1);
            if graph.with_untracked(|g| g.pause) {
                return;
            }
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
        };
        match input {
            Input::Clock(mut frames, period) => every(period, move |_| {
                if let Some(data) = frames.next() {
                    take(data);
                }
            }),
            Input::Thread(run) => {
                let feed = latest(run);
                // Watched by number: a watch runs only when what it reads
                // changes, and two silent buffers are equal.
                watch(
                    move || feed.with(|f| f.as_ref().map(|f| f.number)),
                    move |_, _| {
                        if let Some(data) =
                            feed.with_untracked(|f| f.as_ref().map(|f| f.data.clone()))
                        {
                            take(data);
                        }
                    },
                );
            }
            #[cfg(feature = "audio")]
            Input::Audio(device) => {
                let (channels, buffer) = (options.channels.max(1) as u16, options.buffer as usize);
                match audio::input(device.as_deref(), options.rate, channels, buffer) {
                    Ok(samples) => {
                        // `None` before the first buffer. Watched by number,
                        // as the thread's frames are.
                        watch(
                            move || samples.with(|f| f.as_ref().map(|f| f.number)),
                            move |_, _| {
                                if let Some(data) = samples
                                    .with_untracked(|f| f.as_ref().map(|f| f.samples.clone()))
                                {
                                    take(data);
                                }
                            },
                        );
                    }
                    // The builder cannot fail: end the app on its first turn.
                    Err(error) => watch(|| (), move |_, cx| cx.fail(error.to_string())),
                }
            }
        }
        every(Duration::from_secs(1), move |_| {
            fps.set(frames.get_untracked());
            frames.set(0);
        });

        let plot = renderable(move || {
            let g = graph.get();
            let plot = match mode.get() {
                Mode::Oscilloscope => osc.with(|o| frame.with(|f| oscilloscope(&g, o, f))),
                Mode::Vectorscope => frame.with(|f| vectorscope(&g, f)),
                Mode::Spectroscope => spec.with(|s| history.with(|h| spectroscope(&g, s, h))),
            };
            chart(&plot, &g)
        })
        .name("plot");

        let shown_header = switch(
            move || graph.with(|g| g.show_ui),
            move |show| {
                if show {
                    header(graph, mode, osc, spec, fps)
                } else {
                    column([])
                }
            },
        );

        let mut root = column([shown_header.auto(), plot])
            .bind("q ctrl+q ctrl+w", "quit", |cx| cx.quit())
            .bind("space", "pause", move |_| {
                graph.update(|g| g.pause = !g.pause)
            })
            .bind("s", "scatter or lines", move |_| {
                graph.update(|g| g.scatter = !g.scatter)
            })
            .bind("h", "hide the interface", move |_| {
                graph.update(|g| g.show_ui = !g.show_ui)
            })
            .bind("r", "reference lines", move |_| {
                graph.update(|g| g.references = !g.references)
            })
            .bind(
                "tab",
                "oscilloscope, vectorscope, spectroscope",
                move |_| mode.update(|m| *m = m.next()),
            )
            .bind("esc", "reset the view (and stop triggering)", move |_| {
                graph.update(|g| {
                    g.samples = g.width;
                    g.scale = 1.0;
                });
                // The scope showing gets the key too: the oscilloscope
                // stops triggering.
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.triggering = false);
                }
            })
            .bind("t", "oscilloscope: trigger", move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.triggering = !o.triggering);
                }
            })
            .bind("e", "oscilloscope: rising or falling edge", move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.falling_edge = !o.falling_edge);
                }
            })
            .bind(
                "p",
                "oscilloscope: peaks · spectroscope: phase difference",
                move |_| match mode.get_untracked() {
                    Mode::Oscilloscope => osc.update(|o| o.peaks = !o.peaks),
                    Mode::Spectroscope => spec.update(|s| s.phase_diff = !s.phase_diff),
                    Mode::Vectorscope => {}
                },
            )
            .bind("w", "spectroscope: Hann window", move |_| {
                if mode.get_untracked() == Mode::Spectroscope {
                    spec.update(|s| s.window = !s.window);
                }
            })
            .bind("l", "spectroscope: linear or log level", move |_| {
                if mode.get_untracked() == Mode::Spectroscope {
                    spec.update(|s| s.log_y = !s.log_y);
                }
            })
            // The framework's help lists the described bindings; scope-tui has
            // none.
            .bind("?", "these keys", |cx| cx.help());
        let debounce = [
            ("=", 1, "oscilloscope: debounce up (+ by 10)"),
            ("-", -1, "oscilloscope: debounce down (_ by 10)"),
            ("+", 10, ""),
            ("_", -10, ""),
        ];
        for (key, step, description) in debounce {
            root = root.bind(&format!("{key:?}"), description, move |_| {
                if mode.get_untracked() == Mode::Oscilloscope {
                    osc.update(|o| o.depth = (o.depth as i64 + step).clamp(1, 65535) as u32);
                }
            });
        }
        for (modifier, magnitude) in MAGNITUDES {
            // Described once, without a modifier: the help would list each.
            let describe = |what: &'static str| if modifier.is_empty() { what } else { "" };
            // scope-tui steps the samples by whole cells: alt's fifth of 25 is 5.
            let samples = move |sign: f64| {
                graph.update(|g| {
                    let step = sign * (25.0 * magnitude).trunc();
                    let top = g.width as f64 * 2.0;
                    g.samples = nudge(g.samples as f64, step, 1.0, (0.0, top)) as u32;
                })
            };
            root = root
                .bind(
                    &format!("{modifier}up"),
                    describe("scale up (shift ×10, ctrl ×5, alt ×⅕)"),
                    move |_| {
                        graph.update(|g| g.scale = nudge(g.scale, 0.01, magnitude, (0.0, 10.0)))
                    },
                )
                .bind(
                    &format!("{modifier}down"),
                    describe("scale down"),
                    move |_| {
                        graph.update(|g| g.scale = nudge(g.scale, -0.01, magnitude, (0.0, 10.0)))
                    },
                )
                .bind(
                    &format!("{modifier}right"),
                    describe("more samples"),
                    move |_| samples(1.0),
                )
                .bind(
                    &format!("{modifier}left"),
                    describe("fewer samples"),
                    move |_| samples(-1.0),
                )
                .bind(
                    &format!("{modifier}pageup"),
                    describe("oscilloscope: threshold up · spectroscope: average more"),
                    move |_| page(mode, osc, spec, 1.0, magnitude),
                )
                .bind(
                    &format!("{modifier}pagedown"),
                    describe("oscilloscope: threshold down · spectroscope: average less"),
                    move |_| page(mode, osc, spec, -1.0, magnitude),
                );
        }
        root
    })
}

/// PgUp (`sign` 1) or PgDn (-1): the oscilloscope's trigger threshold, by
/// 250 times the modifier's magnitude, or the spectroscope's averaging, by
/// one whatever the modifier, as in scope-tui.
fn page(
    mode: Signal<Mode>,
    osc: Signal<Oscilloscope>,
    spec: Signal<Spectroscope>,
    sign: f64,
    magnitude: f64,
) {
    match mode.get_untracked() {
        Mode::Oscilloscope => osc.update(|o| {
            o.threshold = nudge(o.threshold, 250.0 * sign, magnitude, (0.0, FULL_SCALE))
        }),
        Mode::Spectroscope => {
            spec.update(|s| s.average = (s.average as i64 + sign as i64).clamp(1, 65535) as u32)
        }
        Mode::Vectorscope => {}
    }
}

/// The header: scope-tui's one-row `Table`, as a row of cells laid out by
/// ratatui's solver (percentages of the whole width, a column between
/// cells), each cut at its edge and drawn again only when what it shows
/// changes. The row's style is the table's: the labels' colour.
fn header(
    graph: Signal<Graph>,
    mode: Signal<Mode>,
    osc: Signal<Oscilloscope>,
    spec: Signal<Spectroscope>,
    fps: Signal<u32>,
) -> Node {
    let flag = move |on: fn(&Graph) -> bool, yes: &'static str, no: &'static str| {
        cell(move || (if graph.with(on) { yes } else { no }).into())
    };
    let labels = graph.with_untracked(|g| g.labels.clone());
    row([
        cell(move || {
            let colour = graph.with(|g| g.palette(0).to_string());
            format!("[bold {colour}]{}::scope-tui", mode.get().name())
        })
        .percent(35),
        cell(move || match mode.get() {
            Mode::Oscilloscope => osc.with(oscilloscope_header),
            Mode::Vectorscope => "live".into(),
            Mode::Spectroscope => graph.with(|g| spec.with(|s| spectroscope_header(g, s))),
        })
        .percent(25),
        cell(move || format!("-{:.2}x+", graph.with(|g| g.scale))).percent(7),
        cell(move || graph.with(|g| format!("{}/{} spf", g.samples, g.width))).percent(13),
        cell(move || format!("{}fps", fps.get())).percent(6),
        flag(|g| g.scatter, "***", "---").percent(6),
        flag(|g| g.pause, "||", "|>").percent(6),
    ])
    .solver(Solver::Ratatui)
    // ratatui's tables leave a column between cells.
    .gap(1)
    .style(&labels)
    .fixed(1)
}

/// Where the samples come from: scope-tui's subcommands, and the test
/// signal.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// No subcommand, or `demo`: the test signal, held still with `--still`.
    Demo { still: bool },
    /// `file PATH [-l]`: PCM from a file, a pipe or stdin (`-`), played at
    /// its sample rate with `-l`.
    File { path: String, limit_rate: bool },
    /// `audio [DEVICE] [--timeout S] [--list]`: a sound card, the default
    /// input without a name; `--list` prints the inputs instead.
    Audio {
        device: Option<String>,
        timeout: u64,
        list: bool,
    },
    /// `pulse [DEVICE] [--server-buffer N]`: PulseAudio, reached through
    /// ALSA's `pulse` device.
    Pulse {
        device: Option<String>,
        server_buffer: u32,
    },
}

/// What `main` parsed: the options and the source.
#[derive(Debug, PartialEq)]
pub struct Command {
    pub options: Options,
    pub source: Source,
    /// The backend `--backend NAME` chose; without it, `INTUITUIVE_BACKEND`
    /// names one, else crossterm drives the terminal.
    pub backend: Option<BackendKind>,
    /// What `-h` or `-V` prints instead of running.
    pub print: Option<String>,
}

/// The backend called `name`, if this build has it: termion and termwiz
/// are this crate's features of those names.
pub fn backend_named(name: &str) -> Result<BackendKind, String> {
    BackendKind::from_name(name).ok_or_else(|| {
        let have: Vec<&str> = BackendKind::ALL.iter().map(|kind| kind.name()).collect();
        format!(
            "no backend {name:?} in this build, which has {} (termion and termwiz \
             are features of rs-rich-intuituive)",
            have.join(", ")
        )
    })
}

/// `--help`, laid out as scope-tui's.
pub const USAGE: &str = "\
scope 0.3.5 -- scope-tui by alemi, rebuilt on intuiTUIve
a simple oscilloscope/vectorscope/spectroscope for your terminal

Usage: scope [OPTIONS] [COMMAND]

Commands:
  pulse  use PulseAudio's default source, through ALSA (the audio feature)
  file   use a file from filesystem and read its content
  audio  capture from a sound card (the audio feature)
  demo   draw a test signal (the default)

Options:
  -c, --channels <N>                   number of channels to open [default: 2]
  -b, --buffer <SIZE>                  size of audio buffer, and width of scope [default: 2048]
  -r, --sample-rate <HZ>               sample rate to use [default: 48000]
  -t, --tune <NOTE>                    tune buffer size to be in tune with given note (overrides buffer option)
  -s, --scale <x>                      floating point vertical scale, from 0 to 1 [default: 1]
      --scatter                        use vintage looking scatter mode instead of line mode
      --no-reference                   don't draw reference line
      --no-ui                          hide UI and only draw waveforms
      --no-braille                     don't use braille dots for drawing lines
      --palette-color <color1,color2>  palette to use for scope signal lines [default: red,yellow,cyan,magenta]
      --labels-color <color>           color to use for axis labels [default: Cyan]
      --axis-color <color>             color to use for axis lines [default: DarkGray]
      --still                          hold the test signal still
      --backend <NAME>                 crossterm, termion or termwiz
  -h, --help                           Print help
  -V, --version                        Print version

file <PATH> [-l]: PATH is a file or pipe of signed 16-bit little-endian PCM,
or - for stdin; -l, --limit-rate limits data flow to match the sample rate.
audio [DEVICE] [--list] [--timeout S]: DEVICE is a name --list prints, or a
part of one; the default input without it.
pulse [DEVICE] [--server-buffer N]: DEVICE is an ALSA device, `pulse` by default.
";

/// Parse scope-tui's command line (without the program's name). Options
/// may come before or after the source.
pub fn parse(args: &[String]) -> Result<Command, String> {
    let mut options = Options::default();
    let (mut source, mut still, mut note) = (None, false, None);
    let (mut backend, mut print) = (None, None);
    let mut args = args.iter().peekable();
    let number = |name: &str, value: Option<&String>| -> Result<f64, String> {
        value
            .and_then(|v| v.parse().ok())
            .ok_or_else(|| format!("{name} takes a number"))
    };
    let colour = |name: &str, value: &str| -> Result<String, String> {
        style_from_color_name(value.trim())
            .ok_or_else(|| format!("invalid colour {value:?} for {name}"))
    };
    while let Some(arg) = args.next() {
        // A device or a path: the next word, unless it is an option.
        let operand = |args: &mut std::iter::Peekable<std::slice::Iter<String>>| {
            args.next_if(|a| !a.starts_with('-') || a.as_str() == "-")
                .cloned()
        };
        match (arg.as_str(), &mut source) {
            ("-c" | "--channels", _) => {
                options.channels = number(arg, args.next())?.max(1.0) as usize
            }
            ("-b" | "--buffer", _) => options.buffer = number(arg, args.next())?.max(16.0) as u32,
            ("-r" | "--sample-rate", _) => options.rate = number(arg, args.next())?.max(1.0) as u32,
            ("-s" | "--scale", _) => options.scale = number(arg, args.next())?.max(0.0),
            ("-t" | "--tune", _) => note = Some(args.next().ok_or("--tune takes a note")?.clone()),
            ("--scatter", _) => options.scatter = true,
            ("--no-reference", _) => options.references = false,
            ("--no-ui", _) => options.show_ui = false,
            ("--no-braille", _) => options.braille = false,
            ("--palette-color", _) => {
                let list = args.next().ok_or("--palette-color takes colours")?;
                options.palette = list
                    .split(',')
                    .map(|c| colour(arg, c))
                    .collect::<Result<_, _>>()?;
            }
            ("--labels-color", _) => {
                options.labels = colour(arg, args.next().ok_or("--labels-color takes a colour")?)?
            }
            ("--axis-color", _) => {
                options.axis = colour(arg, args.next().ok_or("--axis-color takes a colour")?)?
            }
            ("--still", _) => still = true,
            ("--backend", _) => {
                let name = args
                    .next()
                    .ok_or("--backend takes a name: crossterm, termion or termwiz")?;
                backend = Some(backend_named(name)?);
            }
            ("-h" | "--help" | "help", _) => print = Some(USAGE.to_string()),
            ("-V" | "--version", _) => {
                print = Some("scope 0.3.5 (scope-tui on intuiTUIve)\n".into())
            }
            // Each subcommand's own options, after it.
            ("-l" | "--limit-rate", Some(Source::File { limit_rate, .. })) => *limit_rate = true,
            ("--list", Some(Source::Audio { list, .. })) => *list = true,
            ("--timeout", Some(Source::Audio { timeout, .. })) => {
                *timeout = number(arg, args.next())?.max(0.0) as u64
            }
            ("--server-buffer", Some(Source::Pulse { server_buffer, .. })) => {
                *server_buffer = number(arg, args.next())?.max(1.0) as u32
            }
            ("demo" | "file" | "audio" | "pulse", Some(_)) => {
                return Err(format!("unexpected {arg:?}: there is one source"))
            }
            ("demo", None) => source = Some(Source::Demo { still: false }),
            ("file", None) => {
                // -l may come before the path, as clap lets it.
                let limit_rate = args.next_if(|a| matches!(a.as_str(), "-l" | "--limit-rate"));
                let path = operand(&mut args).ok_or("file takes a path, or - for stdin")?;
                source = Some(Source::File {
                    path,
                    limit_rate: limit_rate.is_some(),
                });
            }
            ("audio", None) => {
                source = Some(Source::Audio {
                    device: operand(&mut args),
                    timeout: 5,
                    list: false,
                })
            }
            ("pulse", None) => {
                source = Some(Source::Pulse {
                    device: operand(&mut args),
                    server_buffer: 32,
                })
            }
            (other, _) => return Err(format!("unknown argument {other:?}")),
        }
    }
    if let Some(note) = note {
        match tune(&note, options.rate, options.channels) {
            Some(buffer) => options.buffer = buffer,
            None => eprintln!("[!] Unrecognized note '{note}', ignoring option"),
        }
    }
    let source = match source {
        None | Some(Source::Demo { .. }) => Source::Demo { still },
        Some(other) => other,
    };
    Ok(Command {
        options,
        source,
        backend,
        print,
    })
}

/// The input for `source`: the test signal and files on a thread of their
/// own, a sound card through intuiTUIve's `audio` feature. A buffer's worth
/// of time is scope-tui's frame.
pub fn input(source: &Source, options: &Options) -> std::io::Result<Input> {
    let (channels, buffer) = (options.channels.max(1), options.buffer as usize);
    let period = Duration::from_secs_f64(buffer as f64 / options.rate.max(1) as f64);
    let silence = vec![vec![0.0; buffer]; channels];
    Ok(match source {
        Source::Demo { still } => {
            Input::Clock(Box::new(Demo::new(channels, buffer, *still)), period)
        }
        Source::File { path, limit_rate } => {
            let reader: Box<dyn Read + Send> = if path == "-" {
                Box::new(std::io::stdin())
            } else {
                Box::new(std::fs::File::open(path)?)
            };
            let pcm = Pcm::new(reader, channels, buffer);
            play(pcm, silence, limit_rate.then_some(period), period)
        }
        #[cfg(feature = "audio")]
        Source::Audio { device, .. } => Input::Audio(device.clone()),
        #[cfg(feature = "audio")]
        Source::Pulse { device, .. } => {
            Input::Audio(Some(device.clone().unwrap_or_else(|| "pulse".into())))
        }
        #[cfg(not(feature = "audio"))]
        Source::Audio { .. } | Source::Pulse { .. } => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "audio and pulse need the audio feature: cargo run -p rs-rich-intuituive \
                 --example scope --features audio -- audio; or pipe PCM into `file -`, as in \
                 parec --format=s16le --rate=48000 --channels=2 | scope file -",
            ))
        }
    })
}

/// `audio --list`: the inputs, the default marked, as scope-tui lists them.
#[cfg(feature = "audio")]
pub fn list_devices() -> String {
    let devices = audio::devices();
    if devices.is_empty() {
        return "no audio inputs\n".into();
    }
    devices
        .iter()
        .map(|d| {
            let default = if d.default { " (default)" } else { "" };
            format!("> {}{default}\n", d.name)
        })
        .collect()
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let invalid = |error: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, error);
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = parse(&args).map_err(invalid)?;
    if let Some(text) = &command.print {
        print!("{text}");
        return Ok(());
    }
    #[cfg(feature = "audio")]
    if let Source::Audio { list: true, .. } = command.source {
        print!("{}", list_devices());
        return Ok(());
    }
    let backend = match command.backend {
        Some(kind) => kind,
        None => match std::env::var("INTUITUIVE_BACKEND") {
            Ok(name) if !name.is_empty() => backend_named(&name).map_err(invalid)?,
            _ => BackendKind::default(),
        },
    };
    let input = input(&command.source, &command.options)?;
    scope_app(command.options, input).run_with(backend)
}

//! An oscilloscope, vectorscope and spectroscope for the terminal in the
//! style of [scope-tui](https://github.com/alemidev/scope-tui), a ratatui
//! app, rebuilt on intuiTUIve. It reuses none of scope-tui's code: it is a
//! rebuild of the behaviour, showing how an app that redraws twenty or more
//! times a second maps. The plot is rich-ext's `Chart`, which lays out and
//! draws as ratatui's does; the header is `cells`, ratatui's one-row
//! `Table`; every source is a signal of the newest buffer.
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
//!     cargo run -p rs-rich-intuituive --example scope --features pulse -- pulse
//!
//! Options, as scope-tui's: `-c N` channels, `-b SIZE` buffer, `-r HZ`
//! sample rate, `-t NOTE` tune the buffer to a note, `-s X` scale,
//! `--scatter`, `--no-reference`, `--no-ui`, `--no-braille`,
//! `--palette-color a,b,…`, `--labels-color c`, `--axis-color c` (colours
//! as ratatui reads them: `red`, `LightRed`, `DarkGray`, `#ff8800`, `208`).
//! Sources, as scope-tui's subcommands: `file PATH [-l]` plays a file or a
//! pipe of s16le PCM (at its sample rate with `-l`); with the crate's
//! `audio` feature, `audio [DEVICE] [--list] [--timeout S]` captures a
//! sound card, and `pulse [DEVICE] [--server-buffer N]` a PulseAudio or
//! PipeWire source, a sink's monitor among them, with the `pulse` feature
//! (through ALSA's `pulse` device with `audio` alone). Without a source it
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
use std::io::Read;
use std::time::Duration;

pub use intuituive::audio::Matrix;
use intuituive::audio::{self, Capture, Format, Pcm};
use intuituive::interact::BackendKind;
use intuituive::prelude::*;
use intuituive::widgets::cells;
use intuituive::{latest_iter, on_write_ref, recent, FeedState, Pace};
use rich_ext::chart::{
    Axis, AxisScale, Chart, Dataset, GraphType, Labels, Layering, LegendPosition, Marker,
    OutOfBounds,
};
use rich_ext::dsp;
use rich_ext::palette::style_from_color_name;

/// A test signal: two tones a fifth apart (2 and 3 cycles a buffer), one
/// per channel, each with an overtone, faint high partials and a little
/// noise, so the vectorscope draws a Lissajous figure and the spectroscope
/// peaks along a noise floor. Made of rich-ext's `dsp` generators, each
/// carrying its phase from one buffer to the next. Unless `still`, the
/// right channel is a little sharp, so the figure turns; still, the first
/// buffer comes again and again, noise included. It never ends.
pub struct Demo {
    buffer: usize,
    channels: Vec<dsp::Mix>,
    hold: bool,
    /// The buffer repeated, once made, when holding still.
    held: Option<Matrix>,
}

impl Demo {
    pub fn new(channels: usize, buffer: usize, rate: u32, still: bool) -> Demo {
        let (n, rate) = (buffer.max(1) as f64, f64::from(rate.max(1)));
        // The frequency that is `cycles` periods a buffer.
        let hz = |cycles: f64| cycles * rate / n;
        // How far the right channel drifts a buffer, in cycles.
        let drift = if still {
            0.0
        } else {
            0.05 / std::f64::consts::TAU
        };
        let channel = |c: usize| {
            let note = match c {
                0 => dsp::partials(hz(2.0), rate, &[0.6, 0.0, 0.2]),
                1 => dsp::partials(hz(3.0 + drift), rate, &[0.6, 0.0, 0.15]),
                c => dsp::partials(hz(c as f64 + 2.0), rate, &[0.5]),
            };
            // A fading series of high partials, for the spectroscope.
            let high = (1..=24).fold(dsp::mix(), |mix, k| {
                let tone = dsp::tone(hz((17 * k + 5 * c) as f64), rate).phase(k as f64);
                mix.add(0.012 / k as f64, tone)
            });
            let seed = 2 * c as u64;
            dsp::mix()
                .add(1.0, note)
                .add(1.0, high)
                // A smooth wander that falls away with frequency, as
                // music's spectrum does, and a little white noise.
                .add(0.26, dsp::noise(seed).low_pass(300.0, rate))
                .add(0.01, dsp::noise(seed + 1))
        };
        Demo {
            buffer: buffer.max(1),
            channels: (0..channels.max(1)).map(channel).collect(),
            hold: still,
            held: None,
        }
    }
}

impl Iterator for Demo {
    type Item = Matrix;

    fn next(&mut self) -> Option<Matrix> {
        if let Some(held) = &self.held {
            return Some(held.clone());
        }
        let buffer = self.buffer;
        let made: Matrix = (self.channels.iter_mut())
            .map(|mix| mix.by_ref().take(buffer).collect())
            .collect();
        if self.hold {
            self.held = Some(made.clone());
        }
        Some(made)
    }
}

/// Where the app's buffers come from, started when the app is: each is a
/// signal of the newest buffer.
pub enum Input {
    /// A capture, with a status: a sound card or a PulseAudio source
    /// (`audio::open`), or the test signal played as one
    /// (`Capture::simulated`, see [`demo`]). A stall ends the app, as
    /// scope-tui's `--timeout`; so does a failure.
    Capture(Capture),
    /// A file or a pipe, played when the app starts (see [`play`]).
    Play(Box<dyn FnOnce() -> Signal<Option<Matrix>>>),
}

/// The test signal as a capture with no sound card: a buffer each
/// buffer's worth of time, on the app's clock under a test.
pub fn demo(options: &Options, still: bool) -> Input {
    let (channels, buffer) = (options.channels.max(1), options.buffer as usize);
    let demo = Demo::new(channels, buffer, options.rate, still);
    Input::Capture(Capture::simulated(demo, period(options)))
}

/// A buffer's worth of time: scope-tui's frame.
fn period(options: &Options) -> Duration {
    Duration::from_secs_f64(options.buffer as f64 / options.rate.max(1) as f64)
}

/// Play a file's or a pipe's buffers: at the sample rate with `-l`
/// (`audio::play`), else as fast as they come, the newest winning
/// (`latest_iter`). When they end, the scope goes on with silence, as
/// scope-tui's file source pads with it, a buffer's worth of time apart.
pub fn play<R: Read + Send + 'static>(pcm: Pcm<R>, rate: u32, limit_rate: bool) -> Input {
    Input::Play(Box::new(move || {
        if limit_rate {
            return audio::play(pcm, rate);
        }
        let silence = vec![vec![0.0; pcm.buffer()]; pcm.channels()];
        let period = Duration::from_secs_f64(pcm.buffer() as f64 / f64::from(rate.max(1)));
        latest_iter(pcm, Pace::Free)
            .then_with(move |tx| while tx.send(silence.clone()) && tx.sleep(period) {})
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
    /// Tab's order.
    pub const ALL: [Mode; 3] = [Mode::Oscilloscope, Mode::Vectorscope, Mode::Spectroscope];

    fn name(self) -> &'static str {
        match self {
            Mode::Oscilloscope => "oscillo",
            Mode::Vectorscope => "vector",
            Mode::Spectroscope => "spectro",
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

impl Oscilloscope {
    /// Where the trace starts in `channel`: rich-ext's `dsp::trigger` at
    /// the threshold (scaled to -1 to 1) on the edge chosen, or 0 when not
    /// triggering. `None` when it never crosses.
    pub fn trigger_at(&self, channel: &[f64]) -> Option<usize> {
        if !self.triggering {
            return Some(0);
        }
        let edge = if self.falling_edge {
            dsp::Edge::Falling
        } else {
            dsp::Edge::Rising
        };
        let level = self.threshold / FULL_SCALE;
        dsp::trigger(channel, level, self.depth as usize, edge)
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
}

/// What a mode draws: data sets over an x and a y range, with axis titles,
/// and the reference lines as each axis's grid.
#[derive(Clone, Debug, PartialEq)]
pub struct Plot {
    pub sets: Vec<DataSet>,
    pub x: (f64, f64),
    pub y: (f64, f64),
    pub x_title: &'static str,
    pub y_title: &'static str,
    /// A logarithmic x axis (the spectroscope's frequencies).
    pub x_log: bool,
    /// Reference lines up the plot at these x values, and across it at
    /// these y values.
    pub x_grid: Vec<f64>,
    pub y_grid: Vec<f64>,
}

/// The oscilloscope's and the spectroscope's names for channels.
fn channel_name(index: usize) -> String {
    match index {
        0 => "L".into(),
        1 => "R".into(),
        n => n.to_string(),
    }
}

/// The oscilloscope: each channel against time, from the trigger point if
/// triggering, with each channel's peaks marked at the left edge.
pub fn oscilloscope(g: &Graph, o: &Oscilloscope, data: &Matrix) -> Plot {
    let mut sets = Vec::new();
    // A buffer that never crosses leaves nothing to draw, as in scope-tui.
    let offset = data
        .first()
        .map_or(0, |first| o.trigger_at(first).unwrap_or(first.len()));
    if o.triggering {
        let level = o.threshold / FULL_SCALE;
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
        x_grid: Vec::new(),
        // The zero line.
        y_grid: if g.references { vec![0.0] } else { Vec::new() },
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
    // The cross through the middle.
    let cross = if g.references { vec![0.0] } else { Vec::new() };
    Plot {
        sets,
        x: (-g.scale, g.scale),
        y: (-g.scale, g.scale),
        x_title: "left -",
        y_title: "| right",
        x_log: false,
        x_grid: cross.clone(),
        y_grid: cross,
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

/// The spectroscope: each channel's spectrum over the last buffers shown,
/// oldest first (`history`, the core's `recent`), one after the other,
/// against a logarithmic frequency axis. Every bin is a point, the
/// mirrored half above the Nyquist frequency too, as in scope-tui (it
/// shows once → widens the axis past it): `dsp::spectrum_full`.
pub fn spectroscope(g: &Graph, s: &Spectroscope, history: &VecDeque<Matrix>) -> Plot {
    let upper = g.scale * 7.5;
    let rate = f64::from(g.rate);
    let mut sets = Vec::new();
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
    let channels = history.back().map_or(0, Vec::len);
    for n in (0..channels).rev() {
        let mut chunk: Vec<f64> = (history.iter())
            .filter_map(|buffer| buffer.get(n))
            .flatten()
            .copied()
            .collect();
        if chunk.is_empty() {
            continue;
        }
        if s.window {
            chunk = dsp::hann(&chunk);
        }
        let peak = chunk.iter().copied().fold(1.0, f64::max);
        chunk.iter_mut().for_each(|x| *x /= peak);
        let level = |magnitude: f64| if s.log_y { magnitude.ln() } else { magnitude };
        let points = dsp::spectrum_full(&chunk, rate)
            .into_iter()
            .map(|(hz, magnitude)| (hz, level(magnitude)))
            .collect();
        if s.phase_diff {
            let bins = dsp::fft(&chunk);
            phases.push((
                rate / chunk.len() as f64,
                bins.iter().map(phase).collect::<Vec<_>>(),
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
            DataSet::new(Some("phase diff"), points, true, g.palette(channels)),
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
        x_grid: if g.references {
            reference_frequencies().collect()
        } else {
            Vec::new()
        },
        // scope-tui's zero line starts at 1 Hz, left of the axis's 20 Hz,
        // so ratatui leaves it out.
        y_grid: Vec::new(),
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
/// right, the legend boxed at the top right, and the reference lines as
/// each axis's grid in the axis colour (one layer a line). Braille dots,
/// 2x4 a cell, or one dot a cell without Braille: the grid's too, before
/// the first buffer.
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
    let axis = |(min, max): (f64, f64), title: &str, scale: AxisScale, grid: &[f64]| {
        let axis = Axis::default()
            .bounds(min, max)
            .scale(scale)
            .labels(Labels::None)
            .style(g.axis.clone())
            .grid(grid.iter().copied())
            .grid_style(g.axis.clone());
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
        .marker(marker)
        .x_axis(axis(plot.x, plot.x_title, x_scale, &plot.x_grid))
        .y_axis(axis(plot.y, plot.y_title, AxisScale::Linear, &plot.y_grid))
        .legend(LegendPosition::TopRight)
        // As ratatui 0.29 draws: its canvas leaves out a line with an end
        // outside the bounds, draws each dataset over the ones before (so
        // the trace breaks the reference lines it crosses), and draws the
        // legend's box in the chart's own style (none: the terminal's
        // colours) over what the datasets drew.
        .out_of_bounds(OutOfBounds::Drop)
        .layering(Layering::PerDataset)
        .legend_unstyled()
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
/// `Eb`): one period of the note at `rate` in scope-tui's own table
/// (rich-ext's `dsp::NoteTable::ScopeTui`, octave 0 when it names none),
/// then nudged up while it is a multiple of `channels × 2`.
pub fn tune(note: &str, rate: u32, channels: usize) -> Option<u32> {
    let hz = dsp::NoteTable::ScopeTui.hz(note)?;
    let mut buffer = ((f64::from(rate) / hz).round() as u32).max(1);
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

/// The modifiers scope-tui scales a step by, said as its docs say them:
/// shift ×10, ctrl ×5, alt ⅕.
const MAGNITUDES: [(&str, f64, &str); 4] = [
    ("", 1.0, "×1"),
    ("shift", 10.0, "×10"),
    ("ctrl", 5.0, "×5"),
    ("alt", 0.2, "⅕"),
];

/// What scope-tui's app ends with when its source gives nothing in time.
pub const STALLED: &str = "data source returned null";

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
        // The buffer shown, and the last ones the spectroscope averages.
        let frame = signal(Matrix::new());
        let averaged = memo(move || spec.with(|s| s.average.max(1) as usize));
        let history = recent(frame, averaged);

        // Each buffer the source delivers, lent by `on_write_ref` and
        // copied once, into the buffer shown. Paused, the scope still
        // takes them and counts the frames, as scope-tui does, but shows
        // nothing new.
        let take = move |data: &Matrix| {
            if !graph.with_untracked(|g| g.pause) {
                frame.set(data.clone());
            }
        };
        // `on_write_ref` sees each buffer, two equal ones (silence)
        // included, and `rate` counts them a second: scope-tui's fps.
        let fps = match input {
            Input::Capture(capture) => {
                let samples = capture.samples();
                on_write_ref(samples, move |frame, _| {
                    if let Some(frame) = frame {
                        take(&frame.samples)
                    }
                });
                // `--timeout`: scope-tui ends when no buffer comes in time.
                let status = capture.status();
                watch(
                    move || status.get(),
                    |status, cx| match status {
                        FeedState::Stalled(_) => cx.fail(STALLED),
                        FeedState::Failed(why) => cx.fail(why),
                        FeedState::Running | FeedState::Ended => {}
                    },
                );
                rate(samples)
            }
            Input::Play(start) => {
                let samples = start();
                on_write_ref(samples, move |data, _| data.iter().for_each(take));
                rate(samples)
            }
        };

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

        // The keys every scope takes, then each scope's own: they apply,
        // and the help lists them under the scope's name, only while it
        // shows, so `p` is peaks in one and phase difference in another.
        let reset = move || {
            graph.update(|g| {
                g.samples = g.width;
                g.scale = 1.0;
            })
        };
        let oscilloscope_keys = move |node: Node| {
            // The scope showing gets Esc too: the oscilloscope stops
            // triggering.
            node.bind("esc", "reset the view, stop triggering", move |_| {
                reset();
                osc.update(|o| o.triggering = false);
            })
            .bind_scaled("pageup", &MAGNITUDES, "threshold up", move |_, by| {
                threshold(osc, 1.0, by)
            })
            .bind_scaled("pagedown", &MAGNITUDES, "threshold down", move |_, by| {
                threshold(osc, -1.0, by)
            })
            .bind_toggle("t", "trigger", osc, |o| &mut o.triggering)
            .bind_toggle("e", "rising or falling edge", osc, |o| &mut o.falling_edge)
            .bind_toggle("p", "peaks", osc, |o| &mut o.peaks)
            .bind_step("equals", "debounce up", osc, depth, 1.0, DEPTHS)
            .bind_step("minus", "debounce down", osc, depth, -1.0, DEPTHS)
            .bind_step("plus", "debounce up by 10", osc, depth, 10.0, DEPTHS)
            .bind_step(
                "underscore",
                "debounce down by 10",
                osc,
                depth,
                -10.0,
                DEPTHS,
            )
        };
        let spectroscope_keys = move |node: Node| {
            // One buffer at a time, whatever the modifier, as scope-tui;
            // the help lists the plain key.
            let more = "shift+pageup ctrl+pageup alt+pageup";
            let less = "shift+pagedown ctrl+pagedown alt+pagedown";
            node.bind_step("pageup", "average more", spec, average, 1.0, DEPTHS)
                .bind_step("pagedown", "average less", spec, average, -1.0, DEPTHS)
                .bind_step(more, "", spec, average, 1.0, DEPTHS)
                .bind_step(less, "", spec, average, -1.0, DEPTHS)
                .bind_toggle("w", "Hann window", spec, |s| &mut s.window)
                .bind_toggle("l", "linear or log level", spec, |s| &mut s.log_y)
                .bind_toggle("p", "phase difference", spec, |s| &mut s.phase_diff)
        };
        column([shown_header.auto(), plot])
            .bind("q ctrl+q ctrl+w", "quit", |cx| cx.quit())
            .bind_toggle("space", "pause", graph, |g| &mut g.pause)
            .bind_toggle("s", "scatter or lines", graph, |g| &mut g.scatter)
            .bind_toggle("h", "hide the interface", graph, |g| &mut g.show_ui)
            .bind_toggle("r", "reference lines", graph, |g| &mut g.references)
            .bind_cycle(
                "tab",
                "oscilloscope, vectorscope, spectroscope",
                mode,
                &Mode::ALL,
            )
            .bind_when(
                move || mode.get() != Mode::Oscilloscope,
                "esc",
                "reset the view",
                move |_| reset(),
            )
            .bind_scaled("up", &MAGNITUDES, "scale up", move |_, by| {
                graph.update(|g| g.scale = nudge(g.scale, 0.01, by, (0.0, 10.0)))
            })
            .bind_scaled("down", &MAGNITUDES, "scale down", move |_, by| {
                graph.update(|g| g.scale = nudge(g.scale, -0.01, by, (0.0, 10.0)))
            })
            .bind_scaled("right", &MAGNITUDES, "more samples", move |_, by| {
                samples(graph, 1.0, by)
            })
            .bind_scaled("left", &MAGNITUDES, "fewer samples", move |_, by| {
                samples(graph, -1.0, by)
            })
            .modes(
                mode,
                &[
                    (Mode::Oscilloscope, "oscilloscope", &oscilloscope_keys),
                    (Mode::Spectroscope, "spectroscope", &spectroscope_keys),
                ],
            )
            // The framework's help lists the described bindings; scope-tui
            // has none.
            .bind("?", "these keys", |cx| cx.help())
    })
}

/// The trigger's debounce, and the spectroscope's averaging: from 1 to
/// 65535, as scope-tui keeps them.
const DEPTHS: std::ops::RangeInclusive<u32> = 1..=65535;

/// The fields more than one binding steps (a closure kept in a variable
/// would not do: its return borrows from its argument).
fn depth(o: &mut Oscilloscope) -> &mut u32 {
    &mut o.depth
}

fn average(s: &mut Spectroscope) -> &mut u32 {
    &mut s.average
}

/// ← (`sign` -1) or → (1): the samples shown, by whole steps of 25 times
/// the modifier's magnitude (alt's fifth of 25 is 5), as scope-tui.
fn samples(graph: Signal<Graph>, sign: f64, magnitude: f64) {
    graph.update(|g| {
        let step = sign * (25.0 * magnitude).trunc();
        let top = g.width as f64 * 2.0;
        g.samples = nudge(g.samples as f64, step, 1.0, (0.0, top)) as u32;
    })
}

/// PgUp (`sign` 1) or PgDn (-1) in the oscilloscope: the trigger's
/// threshold, by 250 times the modifier's magnitude, as scope-tui.
fn threshold(osc: Signal<Oscilloscope>, sign: f64, magnitude: f64) {
    osc.update(|o| o.threshold = nudge(o.threshold, 250.0 * sign, magnitude, (0.0, FULL_SCALE)))
}

/// The header: scope-tui's one-row `Table`, as `cells` (percentages of the
/// whole width laid out by ratatui's solver, a column between cells, each
/// cut at its edge), each drawn again only when what it shows changes. The
/// row's style is the table's: the labels' colour.
fn header(
    graph: Signal<Graph>,
    mode: Signal<Mode>,
    osc: Signal<Oscilloscope>,
    spec: Signal<Spectroscope>,
    fps: Signal<u32>,
) -> Node {
    let flag = move |on: fn(&Graph) -> bool, yes: &'static str, no: &'static str| {
        text(move || (if graph.with(on) { yes } else { no }).into())
    };
    let labels = graph.with_untracked(|g| g.labels.clone());
    cells([
        (
            35,
            text(move || {
                let colour = graph.with(|g| g.palette(0).to_string());
                format!("[bold {colour}]{}::scope-tui", mode.get().name())
            }),
        ),
        (
            25,
            text(move || match mode.get() {
                Mode::Oscilloscope => osc.with(oscilloscope_header),
                Mode::Vectorscope => "live".into(),
                Mode::Spectroscope => graph.with(|g| spec.with(|s| spectroscope_header(g, s))),
            }),
        ),
        (
            7,
            text(move || format!("-{:.2}x+", graph.with(|g| g.scale))),
        ),
        (
            13,
            text(move || graph.with(|g| format!("{}/{} spf", g.samples, g.width))),
        ),
        (6, text!("{fps}fps")),
        (6, flag(|g| g.scatter, "***", "---")),
        (6, flag(|g| g.pause, "||", "|>")),
    ])
    .style(&labels)
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
    /// `pulse [DEVICE] [--server-buffer N]`: a PulseAudio (or PipeWire)
    /// source, the default one without a name.
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
  pulse  use a PulseAudio or PipeWire source (the pulse feature)
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
audio [DEVICE] [--list] [--timeout S]: DEVICE is an id --list prints (or a
part of a device's name), the default input without it; it ends after S
seconds (5) without audio.
pulse [DEVICE] [--server-buffer N]: DEVICE is a PulseAudio source, a sink's
monitor among them; N is the server's buffer, in buffers [default: 32].
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

/// The input for `source`, opened before the app, so a file or a device
/// that is not there fails before the terminal is taken: the test signal
/// (a simulated capture), a file read by intuiTUIve's `audio::pcm` (s16le,
/// as scope-tui's), a sound card opened by its `audio` feature. A buffer's
/// worth of time is scope-tui's frame.
pub fn input(source: &Source, options: &Options) -> std::io::Result<Input> {
    let (channels, buffer) = (options.channels.max(1), options.buffer as usize);
    Ok(match source {
        Source::Demo { still } => demo(options, *still),
        Source::File { path, limit_rate } => {
            let reader: Box<dyn Read + Send> = if path == "-" {
                Box::new(std::io::stdin())
            } else {
                Box::new(std::fs::File::open(path)?)
            };
            let pcm = audio::pcm(reader, Format::S16Le, channels, buffer);
            play(pcm, options.rate, *limit_rate)
        }
        #[cfg(feature = "audio")]
        Source::Audio {
            device, timeout, ..
        } => {
            let quiet = Duration::from_secs(*timeout);
            let mut opening = capture_options(options).stall_after(quiet);
            if let Some(device) = device {
                opening = opening.device(device);
            }
            Input::Capture(audio::open(opening).map_err(std::io::Error::other)?)
        }
        #[cfg(feature = "audio")]
        Source::Pulse {
            device,
            server_buffer,
        } => {
            // scope-tui's server buffer is `N` buffers long.
            let mut opening = pulse_options(capture_options(options))
                .latency(server_buffer.saturating_mul(options.buffer));
            if let Some(device) = device {
                opening = opening.device(device);
            }
            Input::Capture(audio::open(opening).map_err(std::io::Error::other)?)
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

/// What every capture asks for: the rate, the channels and the buffer.
#[cfg(feature = "audio")]
fn capture_options(options: &Options) -> audio::AudioOptions {
    audio::AudioOptions::new()
        .rate(options.rate)
        .channels(options.channels.max(1) as u16)
        .buffer(options.buffer as usize)
}

/// `pulse` on PulseAudio's own host with the `pulse` feature, else on
/// ALSA's `pulse` device (the server's default source, by name).
#[cfg(feature = "audio")]
fn pulse_options(opening: audio::AudioOptions) -> audio::AudioOptions {
    #[cfg(feature = "pulse")]
    return opening.host(audio::Host::Pulse);
    #[cfg(not(feature = "pulse"))]
    opening.device("pulse")
}

/// `audio --list` as scope-tui prints it: each input's id after `>`, then
/// each configuration it offers.
#[cfg(feature = "audio")]
pub fn list_devices(devices: &[(audio::Device, Vec<audio::DeviceConfig>)]) -> String {
    let mut out = String::new();
    for (device, configs) in devices {
        out += &format!("> {}\n", device.id);
        for c in configs {
            let (min, max) = c.buffer.unwrap_or((0, 0));
            out += &format!(
                "  + {}ch {}-{}hz {min}-{max}buf ({})\n",
                c.channels, c.min_rate, c.max_rate, c.format
            );
        }
    }
    out
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
        let devices: Vec<_> = (audio::devices().into_iter())
            .map(|device| {
                let configs = device.configs();
                (device, configs)
            })
            .collect();
        print!("{}", list_devices(&devices));
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
    // As scope-tui: a source that stops ends the app, and why goes to
    // stderr once the terminal is back.
    if let Err(error) = scope_app(command.options, input).run_with(backend) {
        eprintln!("[!] Error executing app: {error}");
    }
    Ok(())
}

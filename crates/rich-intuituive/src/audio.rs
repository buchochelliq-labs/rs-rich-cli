//! Capturing sound: a signal that holds the newest buffer of samples from
//! a sound card. Behind the off-by-default `audio` feature, on
//! [cpal](https://docs.rs/cpal) (ALSA on Linux, CoreAudio on macOS, WASAPI
//! on Windows). Building it on Linux needs ALSA's headers
//! (`libasound2-dev` or `alsa-lib-devel`).
//!
//! [`input`] opens a device and returns a [`Signal`] of a [`Matrix`]: one
//! row a channel, `buffer` samples long, each from -1 to 1. Every time
//! the device has captured another `buffer` frames the signal changes, so a
//! node drawing an oscilloscope or a spectrum from it draws again. The app
//! always gets the newest buffer: one that arrives while the last is still
//! waiting for the app's thread replaces it, so a slow frame drops buffers
//! instead of falling behind.
//!
//! ```no_run
//! use intuituive::audio;
//! use intuituive::prelude::*;
//!
//! for device in audio::devices() {
//!     println!("{}{}", device.name, if device.default { " (default)" } else { "" });
//! }
//! App::new(|| {
//!     let samples = audio::input(None, 48_000, 2, 1024).expect("a microphone");
//!     text(move || {
//!         let peak = samples.with(|m| m.iter().flatten().fold(0.0f64, |a, s| a.max(s.abs())));
//!         format!("peak {peak:.2}")
//!     })
//!     .on_key("q", |cx| cx.quit())
//! })
//! .run()
//! # .unwrap();
//! ```

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use crate::reactive::{signal, Runtime, Signal};

/// One buffer of samples: one row a channel, each sample from -1 to 1.
pub type Matrix = Vec<Vec<f64>>;

/// A sound card input, as [`devices`] lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// The name [`input`] takes.
    pub name: String,
    /// Whether it is the system's default input.
    pub default: bool,
}

/// Why [`input`] could not open a device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioError(String);

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AudioError {}

fn error(what: impl fmt::Display) -> AudioError {
    AudioError(what.to_string())
}

/// The input devices of the default host, the default one marked. Empty,
/// never a panic, on a machine without sound (a container, a CI runner) or
/// when the sound system cannot be reached.
pub fn devices() -> Vec<Device> {
    let host = cpal::default_host();
    let default = host.default_input_device().map(|d| d.to_string());
    let Ok(inputs) = host.input_devices() else {
        return Vec::new();
    };
    inputs
        .map(|device| {
            let name = device.to_string();
            Device {
                default: default.as_deref() == Some(name.as_str()),
                name,
            }
        })
        .collect()
}

/// Capture from `device` (a name from [`devices`], matched exactly or else
/// as a case-insensitive part of a name; `None` for the default input) at
/// `rate` samples a second, into a signal holding the newest `buffer`
/// frames as `channels` rows (see the [module docs](self)). Call it while
/// the app is built or running; the capture runs until the app ends. The
/// signal starts as `channels` empty rows.
///
/// When the device offers no configuration with `channels` channels at
/// `rate`, one at `rate` with another count is used: rows past the
/// device's channels repeat its last one (a mono microphone fills both
/// rows of a stereo view), and the device's extra channels are dropped.
///
/// # Errors
/// No such device, no input at `rate`, or the stream would not start.
///
/// # Panics
/// Outside a running or building app.
pub fn input(
    device: Option<&str>,
    rate: u32,
    channels: u16,
    buffer: usize,
) -> Result<Signal<Matrix>, AudioError> {
    let runtime = Runtime::current_for("audio::input()");
    let proxy = runtime.proxy();
    let channels = channels.max(1);
    let buffer = buffer.max(1);
    let samples = signal(vec![Vec::new(); usize::from(channels)]);
    let mailbox = Arc::new(Mailbox::default());
    let wanted = device.map(str::to_string);
    let (opened, result) = mpsc::channel();
    // The stream lives on its own thread: on some platforms it may not
    // move between threads, and it must outlive this call.
    std::thread::Builder::new()
        .name("intuituive-audio".into())
        .spawn(move || {
            let deliver = {
                let mailbox = mailbox.clone();
                let proxy = proxy.clone();
                move |matrix: Matrix| {
                    if mailbox.put(matrix) {
                        let mailbox = mailbox.clone();
                        proxy.run(move || {
                            if let Some(matrix) = mailbox.take() {
                                samples.update(|m| *m = matrix);
                            }
                        });
                    }
                }
            };
            let stream = open(wanted.as_deref(), rate, channels, buffer, deliver);
            let stream = match stream {
                Ok(stream) => {
                    let _ = opened.send(Ok(()));
                    stream
                }
                Err(e) => {
                    let _ = opened.send(Err(e));
                    return;
                }
            };
            // Keep the stream until the app has gone (a job nobody runs
            // fails to send).
            while proxy.run(|| {}) {
                std::thread::sleep(Duration::from_millis(250));
            }
            drop(stream);
        })
        .map_err(error)?;
    result
        .recv()
        .unwrap_or_else(|_| Err(error("the audio thread stopped")))?;
    Ok(samples)
}

/// Open and start the stream, calling `deliver` with each full buffer.
fn open(
    wanted: Option<&str>,
    rate: u32,
    channels: u16,
    buffer: usize,
    deliver: impl FnMut(Matrix) + Send + 'static,
) -> Result<cpal::Stream, AudioError> {
    let host = cpal::default_host();
    let device = match wanted {
        None => host
            .default_input_device()
            .ok_or_else(|| error("no default audio input"))?,
        Some(name) => find(&host, name)?,
    };
    let (config, format) = choose(&device, rate, channels)?;
    let framer = Framer::new(usize::from(config.channels), usize::from(channels), buffer);
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, framer, deliver),
        SampleFormat::F64 => build::<f64>(&device, &config, framer, deliver),
        SampleFormat::I16 => build::<i16>(&device, &config, framer, deliver),
        SampleFormat::I32 => build::<i32>(&device, &config, framer, deliver),
        SampleFormat::I8 => build::<i8>(&device, &config, framer, deliver),
        SampleFormat::U8 => build::<u8>(&device, &config, framer, deliver),
        SampleFormat::U16 => build::<u16>(&device, &config, framer, deliver),
        SampleFormat::U32 => build::<u32>(&device, &config, framer, deliver),
        other => return Err(error(format!("unsupported sample format {other}"))),
    }?;
    stream.play().map_err(error)?;
    Ok(stream)
}

/// The input device named `name`: exactly, or else the first whose name
/// holds it, ignoring case.
fn find(host: &cpal::Host, name: &str) -> Result<cpal::Device, AudioError> {
    let devices: Vec<cpal::Device> = host.input_devices().map_err(error)?.collect();
    let names: Vec<String> = devices.iter().map(|d| d.to_string()).collect();
    let index = pick(&names, name).ok_or_else(|| {
        error(format!(
            "no audio input named {name:?} (there are: {})",
            if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            }
        ))
    })?;
    devices
        .into_iter()
        .nth(index)
        .ok_or_else(|| error("the device went away"))
}

/// Which of `names` `wanted` means: an exact match, else the first that
/// holds it ignoring case.
fn pick(names: &[String], wanted: &str) -> Option<usize> {
    let lower = wanted.to_lowercase();
    names
        .iter()
        .position(|n| n == wanted)
        .or_else(|| names.iter().position(|n| n.to_lowercase().contains(&lower)))
}

/// The sample formats in the order to prefer them.
const FORMATS: [SampleFormat; 8] = [
    SampleFormat::F32,
    SampleFormat::I16,
    SampleFormat::I32,
    SampleFormat::F64,
    SampleFormat::U16,
    SampleFormat::U8,
    SampleFormat::I8,
    SampleFormat::U32,
];

/// A stream configuration at `rate`: `channels` channels if the device
/// has them, else the closest count.
fn choose(
    device: &cpal::Device,
    rate: u32,
    channels: u16,
) -> Result<(cpal::StreamConfig, SampleFormat), AudioError> {
    let ranges: Vec<_> = device
        .supported_input_configs()
        .map_err(error)?
        .filter(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
        .filter(|r| FORMATS.contains(&r.sample_format()))
        .collect();
    let best = ranges
        .iter()
        .min_by_key(|r| {
            let format = FORMATS.iter().position(|f| *f == r.sample_format());
            (r.channels().abs_diff(channels), format)
        })
        .ok_or_else(|| error(format!("the device has no input at {rate} Hz")))?;
    let config = cpal::StreamConfig {
        channels: best.channels(),
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    Ok((config, best.sample_format()))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut framer: Framer,
    mut deliver: impl FnMut(Matrix) + Send + 'static,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + Send + 'static,
    f64: FromSample<T>,
{
    let mut scratch = Vec::new();
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |data: &[T], _| {
                scratch.clear();
                scratch.extend(data.iter().map(|s| f64::from_sample_(*s)));
                if let Some(matrix) = framer.push(&scratch) {
                    deliver(matrix);
                }
            },
            // A stream error (a device unplugged) ends the samples; the
            // signal keeps the last buffer.
            |_| {},
            None,
        )
        .map_err(error)
}

/// Turns interleaved frames from a device into buffers of rows.
#[derive(Debug)]
struct Framer {
    /// The device's channels.
    source: usize,
    /// The rows a buffer has.
    channels: usize,
    /// Frames a buffer.
    buffer: usize,
    /// Samples not yet in a full buffer, interleaved.
    pending: Vec<f64>,
}

impl Framer {
    fn new(source: usize, channels: usize, buffer: usize) -> Framer {
        Framer {
            source: source.max(1),
            channels: channels.max(1),
            buffer: buffer.max(1),
            pending: Vec::new(),
        }
    }

    /// Add interleaved samples; the newest full buffer among them, if one
    /// filled up (older full ones are dropped, as the app shows only the
    /// newest). A partial frame waits for its other samples.
    fn push(&mut self, samples: &[f64]) -> Option<Matrix> {
        self.pending.extend_from_slice(samples);
        let size = self.source * self.buffer;
        let full = self.pending.len() / size;
        if full == 0 {
            return None;
        }
        let start = (full - 1) * size;
        let matrix = self.rows(&self.pending[start..start + size]);
        self.pending.drain(..full * size);
        Some(matrix)
    }

    /// One buffer of interleaved frames as rows, clamped to -1..1.
    fn rows(&self, frames: &[f64]) -> Matrix {
        (0..self.channels)
            .map(|row| {
                let channel = row.min(self.source - 1);
                frames
                    .iter()
                    .skip(channel)
                    .step_by(self.source)
                    .map(|s| s.clamp(-1.0, 1.0))
                    .collect()
            })
            .collect()
    }
}

/// The newest buffer on its way to the app's thread.
#[derive(Default)]
struct Mailbox {
    latest: Mutex<Option<Matrix>>,
    /// A job to take it is already on its way.
    posted: AtomicBool,
}

impl Mailbox {
    /// Leave `matrix` for the app, replacing any not yet taken; whether a
    /// job must be sent to take it (none is on its way).
    fn put(&self, matrix: Matrix) -> bool {
        if let Ok(mut latest) = self.latest.lock() {
            *latest = Some(matrix);
        }
        !self.posted.swap(true, Ordering::SeqCst)
    }

    /// The newest buffer, if one is waiting.
    fn take(&self) -> Option<Matrix> {
        self.posted.store(false, Ordering::SeqCst);
        self.latest.lock().ok().and_then(|mut latest| latest.take())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_become_rows() {
        let mut framer = Framer::new(2, 2, 3);
        // Two frames: not a buffer yet.
        assert_eq!(framer.push(&[0.1, -0.1, 0.2, -0.2]), None);
        // The third frame and half of a fourth.
        assert_eq!(
            framer.push(&[0.3, -0.3, 0.4]),
            Some(vec![vec![0.1, 0.2, 0.3], vec![-0.1, -0.2, -0.3]])
        );
        assert_eq!(framer.pending, [0.4]);
        // Out of range samples are clamped.
        assert_eq!(
            framer.push(&[-0.4, 2.0, 0.0, 0.5, -3.0]),
            Some(vec![vec![0.4, 1.0, 0.5], vec![-0.4, 0.0, -1.0]])
        );
    }

    #[test]
    fn the_newest_full_buffer_wins() {
        let mut framer = Framer::new(1, 1, 2);
        let samples: Vec<f64> = (0..7).map(|i| f64::from(i) / 10.0).collect();
        // Three full buffers and one sample over: the third.
        assert_eq!(framer.push(&samples), Some(vec![vec![0.4, 0.5]]));
        assert_eq!(framer.pending, [0.6]);
    }

    #[test]
    fn channels_are_remixed() {
        // A mono device into two rows: both are the one channel.
        let mut mono = Framer::new(1, 2, 2);
        assert_eq!(
            mono.push(&[0.5, -0.5]),
            Some(vec![vec![0.5, -0.5], vec![0.5, -0.5]])
        );
        // Four device channels into two rows: the first two.
        let mut quad = Framer::new(4, 2, 1);
        assert_eq!(
            quad.push(&[0.1, 0.2, 0.3, 0.4]),
            Some(vec![vec![0.1], vec![0.2]])
        );
    }

    #[test]
    fn the_mailbox_keeps_the_newest_and_posts_once() {
        let mailbox = Mailbox::default();
        assert!(mailbox.put(vec![vec![1.0]]));
        // Already posted: replaced, not posted again.
        assert!(!mailbox.put(vec![vec![2.0]]));
        assert_eq!(mailbox.take(), Some(vec![vec![2.0]]));
        assert_eq!(mailbox.take(), None);
        // Taken: the next one posts again.
        assert!(mailbox.put(vec![vec![3.0]]));
    }

    #[test]
    fn names_match_exactly_then_in_part() {
        let names = [
            "Built-in Microphone".to_string(),
            "USB Audio".to_string(),
            "usb".to_string(),
        ];
        assert_eq!(pick(&names, "usb"), Some(2));
        assert_eq!(pick(&names, "USB"), Some(1));
        assert_eq!(pick(&names, "microphone"), Some(0));
        assert_eq!(pick(&names, "line in"), None);
    }

    #[test]
    fn listing_devices_without_sound_does_not_panic() {
        // In a container or on CI there is usually no sound card at all.
        let devices = devices();
        assert!(devices.iter().filter(|d| d.default).count() <= 1);
    }

    #[test]
    fn a_missing_device_is_an_error() {
        let app = crate::App::new(|| {
            let result = input(Some("no such device for intuituive"), 48_000, 2, 256);
            let shown = match result {
                Ok(_) => "opened".to_string(),
                Err(e) => format!("error: {}", !e.to_string().is_empty()),
            };
            crate::label(shown).on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["q"], 20, 1).unwrap();
        assert_eq!(screen[0].trim_end(), "error: true");
    }
}

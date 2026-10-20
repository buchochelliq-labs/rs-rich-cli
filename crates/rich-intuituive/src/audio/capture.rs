//! Capturing from a sound card into signals, through cpal: the devices,
//! their configurations, and opening a [`Capture`] on one.

use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::live::Shared;
use super::{error, AudioError, Capture, Frame, Matrix};
use crate::reactive::{Runtime, Signal};

/// The sound system a device is reached through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Host {
    /// The platform's default: ALSA on Linux (PulseAudio when the `pulse`
    /// feature is on and a server is running), CoreAudio on macOS, WASAPI
    /// on Windows.
    #[default]
    Default,
    /// ALSA's devices and PCMs, Linux only.
    Alsa,
    /// A PulseAudio server, or PipeWire's (`pipewire-pulse`), with the
    /// `pulse` feature on Linux: every source, a sink's monitor among
    /// them (`….monitor`), what a scope of "what is playing" wants.
    Pulse,
}

impl Host {
    pub(super) fn open(self) -> Result<cpal::Host, AudioError> {
        match self {
            Host::Default => Ok(cpal::default_host()),
            #[cfg(target_os = "linux")]
            Host::Alsa => cpal::host_from_id(cpal::HostId::Alsa).map_err(error),
            #[cfg(not(target_os = "linux"))]
            Host::Alsa => Err(error("ALSA is only on Linux")),
            #[cfg(all(feature = "pulse", target_os = "linux"))]
            Host::Pulse => cpal::host_from_id(cpal::HostId::PulseAudio).map_err(error),
            #[cfg(not(all(feature = "pulse", target_os = "linux")))]
            Host::Pulse => Err(error(
                "PulseAudio needs intuiTUIve's `pulse` feature (on Linux)",
            )),
        }
    }
}

/// A sound card input, as [`devices`] lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// What [`input`] and [`AudioOptions::device`] take: ALSA's PCM name
    /// (`default`, `pulse`, `hw:CARD=PCH,DEV=0`), PulseAudio's source name
    /// (`alsa_input.pci-0000_00_1f.3.analog-stereo`), or the platform's own
    /// id. It stays the same from one run to the next.
    pub id: String,
    /// What to show: the description's first line ("HDA Intel PCH, ALC892
    /// Analog", "Monitor of Built-in Audio"), or the id when there is none.
    pub name: String,
    /// Whether it is the host's default input.
    pub default: bool,
    /// The host it was listed on.
    pub host: Host,
}

impl Device {
    /// What the device can capture: each configuration its driver offers,
    /// as `audio --list` prints them in scope-tui. Empty when the device
    /// cannot be reached (gone, or busy and closed to probing).
    ///
    /// ```no_run
    /// for device in intuituive::audio::devices() {
    ///     for config in device.configs() {
    ///         println!("{}: {config}", device.id);
    ///     }
    /// }
    /// ```
    pub fn configs(&self) -> Vec<DeviceConfig> {
        let _quiet = quiet();
        let Ok(host) = self.host.open() else {
            return Vec::new();
        };
        let Ok(device) = find(&host, &self.id) else {
            return Vec::new();
        };
        let Ok(ranges) = device.supported_input_configs() else {
            return Vec::new();
        };
        ranges
            .map(|range| DeviceConfig {
                channels: range.channels(),
                min_rate: range.min_sample_rate(),
                max_rate: range.max_sample_rate(),
                buffer: match range.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
                    cpal::SupportedBufferSize::Unknown => None,
                },
                format: range.sample_format().to_string(),
            })
            .collect()
    }
}

/// One configuration a [`Device`] can capture in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DeviceConfig {
    /// Channels.
    pub channels: u16,
    /// The lowest sample rate, in Hz.
    pub min_rate: u32,
    /// The highest sample rate, in Hz.
    pub max_rate: u32,
    /// The smallest and largest buffer, in frames, when the driver says.
    pub buffer: Option<(u32, u32)>,
    /// The sample format, as cpal names it (`i16`, `f32`, …).
    pub format: String,
}

impl std::fmt::Display for DeviceConfig {
    /// `2 channels, 8000-192000 Hz, buffer 32-65536, i16`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let channels = if self.channels == 1 {
            "channel"
        } else {
            "channels"
        };
        write!(f, "{} {channels}, ", self.channels)?;
        if self.min_rate == self.max_rate {
            write!(f, "{} Hz", self.min_rate)?;
        } else {
            write!(f, "{}-{} Hz", self.min_rate, self.max_rate)?;
        }
        if let Some((min, max)) = self.buffer {
            write!(f, ", buffer {min}-{max}")?;
        }
        write!(f, ", {}", self.format)
    }
}

/// The input devices of the default host, the default one marked. Empty,
/// never a panic, on a machine without sound (a container, a CI runner) or
/// when the sound system cannot be reached.
pub fn devices() -> Vec<Device> {
    devices_on(Host::Default).unwrap_or_default()
}

/// The input devices of `host`, the default one marked.
///
/// # Errors
/// The host is not there (no PulseAudio server; ALSA off Linux; `Pulse`
/// without the `pulse` feature) or will not list its devices.
pub fn devices_on(host: Host) -> Result<Vec<Device>, AudioError> {
    let _quiet = quiet();
    let opened = host.open()?;
    let default = opened.default_input_device().and_then(|d| id_of(&d));
    let inputs = opened.input_devices().map_err(error)?;
    Ok(inputs
        .filter_map(|device| {
            let id = id_of(&device)?;
            let name = device
                .description()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|_| id.clone());
            Some(Device {
                default: default.as_deref() == Some(id.as_str()),
                name,
                id,
                host,
            })
        })
        .collect())
}

/// A device's id without its host's prefix.
pub(super) fn id_of(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.id().to_string())
}

/// What [`open`] captures, and how.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct AudioOptions {
    host: Host,
    device: Option<String>,
    rate: u32,
    channels: u16,
    buffer: usize,
    latency: Option<u32>,
    stall: Option<Duration>,
}

impl Default for AudioOptions {
    fn default() -> Self {
        AudioOptions::new()
    }
}

impl AudioOptions {
    /// The default host's default input, at 48 kHz, two channels, buffers
    /// of 1024 frames, stalled after a second without one.
    pub fn new() -> Self {
        AudioOptions {
            host: Host::Default,
            device: None,
            rate: 48_000,
            channels: 2,
            buffer: 1024,
            latency: None,
            stall: Some(Duration::from_secs(1)),
        }
    }

    /// The sound system to open the device on (default [`Host::Default`]).
    pub fn host(mut self, host: Host) -> Self {
        self.host = host;
        self
    }

    /// The device: an id from [`devices`] (matched exactly first), else a
    /// name, exactly or as a part of one in any case. Without it, the
    /// host's default input.
    pub fn device(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }

    /// Samples a second (default 48 000).
    pub fn rate(mut self, rate: u32) -> Self {
        self.rate = rate;
        self
    }

    /// Rows a buffer has (default 2, at least 1). When the device offers
    /// no configuration with that many at the rate, one with another
    /// count is used: rows past the device's channels repeat its last one
    /// (a mono microphone fills both rows of a stereo view), and its extra
    /// channels are dropped.
    pub fn channels(mut self, channels: u16) -> Self {
        self.channels = channels.max(1);
        self
    }

    /// Frames a buffer (default 1024, at least 1): how many samples a row
    /// of each [`Frame`] holds.
    pub fn buffer(mut self, frames: usize) -> Self {
        self.buffer = frames.max(1);
        self
    }

    /// The buffer the driver or server keeps, in frames: lower is less
    /// delay, higher fewer dropouts (scope-tui's `pulse --server-buffer`).
    /// Without it, the driver's default; one outside what the device
    /// allows is left at the default.
    pub fn latency(mut self, frames: u32) -> Self {
        self.latency = Some(frames);
        self
    }

    /// How long without a buffer before the [`status`](Capture::status)
    /// says [`FeedState::Stalled`](crate::FeedState::Stalled) (default a
    /// second, on the app's clock): the `--timeout` of a scope. `None`:
    /// never, as scope-tui's `pulse` source, which blocks with no timeout.
    pub fn stall_after(mut self, quiet: impl Into<Option<Duration>>) -> Self {
        self.stall = quiet.into().map(|q| q.max(Duration::from_millis(1)));
        self
    }
}

/// Open a capture (see the [module docs](super)): anywhere, before an app
/// as well as while one is built, so a missing device fails before the
/// terminal is taken. Its [`samples`](Capture::samples) and
/// [`status`](Capture::status) signals are taken inside the app.
///
/// # Errors
/// No such host or device, no input at the rate, or the stream would not
/// start.
pub fn open(options: AudioOptions) -> Result<Capture, AudioError> {
    let (opened, result) = mpsc::channel();
    let stall = options.stall;
    // The stream lives on its own thread: on some platforms it may not
    // move between threads, and it must outlive this call.
    std::thread::Builder::new()
        .name("intuituive-audio".into())
        .spawn(move || {
            // ALSA's complaints while opening are kept, not printed.
            let _quiet = quiet();
            let (stream, shared) = match start(&options, stall) {
                Ok(started) => started,
                Err(e) => {
                    let _ = opened.send(Err(e));
                    return;
                }
            };
            let _ = opened.send(Ok(shared.clone()));
            keep(stream, &shared);
        })
        .map_err(error)?;
    let shared = result
        .recv()
        .unwrap_or_else(|_| Err(error("the audio thread stopped")))?;
    Ok(Capture::from_shared(shared))
}

/// Capture from `device` (an id or a name from [`devices`]; `None` for the
/// default input) at `rate` samples a second, into a signal holding the
/// newest `buffer` frames as `channels` rows: [`open`] and
/// [`Capture::samples`] in one call, inside an app. The capture runs until
/// the app ends.
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
) -> Result<Signal<Option<Frame>>, AudioError> {
    // Checked first, so a call outside an app panics before opening.
    Runtime::current_for("audio::input()");
    let mut options = AudioOptions::new()
        .rate(rate)
        .channels(channels)
        .buffer(buffer);
    if let Some(device) = device {
        options = options.device(device);
    }
    Ok(open(options)?.samples())
}

/// ALSA's messages that a listing or a capture kept off the screen, oldest
/// first (the last 200). Empty off Linux.
pub fn messages() -> Vec<String> {
    #[cfg(target_os = "linux")]
    return super::alsa::messages();
    #[cfg(not(target_os = "linux"))]
    Vec::new()
}

/// While it lives, ALSA's messages on this thread are kept off stderr.
#[cfg(target_os = "linux")]
pub(super) fn quiet() -> super::alsa::Quiet {
    super::alsa::Quiet::new()
}

#[cfg(not(target_os = "linux"))]
pub(super) fn quiet() {}

/// Keep `stream` until the capture stops (or its app ends), passing
/// ALSA's messages on a few times a second.
fn keep(stream: cpal::Stream, shared: &Arc<Shared>) {
    let tick = Duration::from_millis(100);
    #[cfg(target_os = "linux")]
    let mut seen = 0;
    loop {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        match shared.sink() {
            // In an app: until it has gone (a job nobody runs fails to
            // send).
            Some(sink) if !sink.proxy.run(|| {}) => break,
            Some(_) => {}
            None if shared.dropped.load(Ordering::SeqCst) => break,
            None => {}
        }
        #[cfg(target_os = "linux")]
        if let (Some(sink), Some(log)) = (shared.sink(), shared.log.lock().ok().and_then(|l| *l)) {
            let (lines, next) = super::alsa::messages_since(seen);
            seen = next;
            if !lines.is_empty() {
                sink.proxy
                    .run(move || lines.into_iter().for_each(|line| log.push(line)));
            }
        }
        std::thread::sleep(tick);
    }
    // The feed thread ends too.
    shared.halt();
    drop(stream);
}

/// Open and start the stream that feeds a new capture.
fn start(
    options: &AudioOptions,
    stall: Option<Duration>,
) -> Result<(cpal::Stream, Arc<Shared>), AudioError> {
    let host = options.host.open()?;
    let device = match &options.device {
        None => host
            .default_input_device()
            .ok_or_else(|| error("no default audio input"))?,
        Some(wanted) => find(&host, wanted)?,
    };
    let name = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| id_of(&device).unwrap_or_default());
    let (mut config, format, buffers) = choose(&device, options.rate, options.channels)?;
    if let (Some(frames), Some((min, max))) = (options.latency, buffers) {
        if (min..=max).contains(&frames) {
            config.buffer_size = cpal::BufferSize::Fixed(frames);
        }
    }
    let shared = Arc::new(Shared::new(name, stall));
    let framer = Framer::new(
        usize::from(config.channels),
        usize::from(options.channels),
        options.buffer,
    );
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, framer, &shared),
        SampleFormat::F64 => build::<f64>(&device, &config, framer, &shared),
        SampleFormat::I16 => build::<i16>(&device, &config, framer, &shared),
        SampleFormat::I32 => build::<i32>(&device, &config, framer, &shared),
        SampleFormat::I8 => build::<i8>(&device, &config, framer, &shared),
        SampleFormat::U8 => build::<u8>(&device, &config, framer, &shared),
        SampleFormat::U16 => build::<u16>(&device, &config, framer, &shared),
        SampleFormat::U32 => build::<u32>(&device, &config, framer, &shared),
        other => return Err(error(format!("unsupported sample format {other}"))),
    }?;
    stream.play().map_err(error)?;
    Ok((stream, shared))
}

/// The input device `wanted` means (see [`pick`]).
fn find(host: &cpal::Host, wanted: &str) -> Result<cpal::Device, AudioError> {
    let devices: Vec<cpal::Device> = host.input_devices().map_err(error)?.collect();
    let ids: Vec<String> = devices
        .iter()
        .map(|d| id_of(d).unwrap_or_default())
        .collect();
    let names: Vec<String> = devices
        .iter()
        .map(|d| {
            d.description()
                .map(|d| d.name().to_string())
                .unwrap_or_default()
        })
        .collect();
    let index = pick(&ids, &names, wanted).ok_or_else(|| {
        let listed: Vec<String> = ids
            .iter()
            .zip(&names)
            .map(|(id, name)| format!("{id} ({name})"))
            .collect();
        error(format!(
            "no audio input named {wanted:?} (there are: {})",
            if listed.is_empty() {
                "none".to_string()
            } else {
                listed.join(", ")
            }
        ))
    })?;
    devices
        .into_iter()
        .nth(index)
        .ok_or_else(|| error("the device went away"))
}

/// Which device `wanted` means, given their `ids` and `names`: an exact
/// id, an exact name, else the first name, then id, that holds it
/// ignoring case.
fn pick(ids: &[String], names: &[String], wanted: &str) -> Option<usize> {
    let lower = wanted.to_lowercase();
    let holds = |list: &[String]| list.iter().position(|n| n.to_lowercase().contains(&lower));
    ids.iter()
        .position(|n| n == wanted)
        .or_else(|| names.iter().position(|n| n == wanted))
        .or_else(|| holds(names))
        .or_else(|| holds(ids))
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

/// A stream configuration, its sample format and the buffer sizes it
/// allows.
type Chosen = (cpal::StreamConfig, SampleFormat, Option<(u32, u32)>);

/// A stream configuration at `rate`: `channels` channels if the device
/// has them, else the closest count.
fn choose(device: &cpal::Device, rate: u32, channels: u16) -> Result<Chosen, AudioError> {
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
    let buffers = match best.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => Some((*min, *max)),
        cpal::SupportedBufferSize::Unknown => None,
    };
    Ok((config, best.sample_format(), buffers))
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut framer: Framer,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + Send + 'static,
    f64: FromSample<T>,
{
    let mut scratch = Vec::new();
    let (data, errors) = (shared.clone(), shared.clone());
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |samples: &[T], _| {
                scratch.clear();
                scratch.extend(samples.iter().map(|s| f64::from_sample_(*s)));
                if let Some(matrix) = framer.push(&scratch) {
                    data.deliver(matrix);
                }
            },
            // A glitch or a reroute goes on; anything else ends the
            // samples, and the signal keeps the last buffer.
            move |e| match e.kind() {
                cpal::ErrorKind::Xrun
                | cpal::ErrorKind::DeviceChanged
                | cpal::ErrorKind::RealtimeDenied => {}
                _ => errors.fail(e.to_string()),
            },
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
    fn devices_match_by_id_then_name_then_in_part() {
        let ids = [
            "default".to_string(),
            "hw:CARD=USB,DEV=0".into(),
            "null".into(),
        ];
        let names = [
            "Default ALSA Output".to_string(),
            "USB Audio".into(),
            "Discard all samples (playback) or generate zero samples (capture)".into(),
        ];
        assert_eq!(pick(&ids, &names, "null"), Some(2));
        assert_eq!(pick(&ids, &names, "USB Audio"), Some(1));
        assert_eq!(pick(&ids, &names, "usb"), Some(1));
        assert_eq!(pick(&ids, &names, "CARD=USB"), Some(1));
        assert_eq!(pick(&ids, &names, "discard"), Some(2));
        assert_eq!(pick(&ids, &names, "line in"), None);
    }

    #[test]
    fn a_config_reads_as_scope_tui_lists_it() {
        let config = DeviceConfig {
            channels: 2,
            min_rate: 8000,
            max_rate: 192_000,
            buffer: Some((32, 65536)),
            format: "i16".into(),
        };
        assert_eq!(
            config.to_string(),
            "2 channels, 8000-192000 Hz, buffer 32-65536, i16"
        );
        let mono = DeviceConfig {
            channels: 1,
            min_rate: 48_000,
            max_rate: 48_000,
            buffer: None,
            format: "f32".into(),
        };
        assert_eq!(mono.to_string(), "1 channel, 48000 Hz, f32");
    }

    #[test]
    fn listing_devices_without_sound_does_not_panic() {
        // In a container or on CI there is usually no sound card at all.
        let devices = devices();
        assert!(devices.iter().filter(|d| d.default).count() <= 1);
        for device in &devices {
            assert!(!device.id.is_empty());
            let _ = device.configs();
        }
        #[cfg(not(feature = "pulse"))]
        assert!(devices_on(Host::Pulse).is_err());
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
        // Before an app, too.
        let opened = open(AudioOptions::new().device("no such device for intuituive"));
        assert!(opened.is_err());
    }
}

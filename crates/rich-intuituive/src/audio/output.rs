//! Playing sound through a sound card, through cpal: [`output`] opens a
//! device and gives an [`Output`] to write samples to.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};

use super::capture::{id_of, quiet};
use super::{error, AudioError, Device, Host, Matrix};

/// What [`output`] opens, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputOptions {
    host: Host,
    device: Option<String>,
    rate: Option<u32>,
    channels: Option<u16>,
    queue: Duration,
}

impl Default for OutputOptions {
    fn default() -> Self {
        OutputOptions::new()
    }
}

impl OutputOptions {
    /// The default host's default output, at its own rate and channels,
    /// a quarter second queued.
    pub fn new() -> Self {
        OutputOptions {
            host: Host::Default,
            device: None,
            rate: None,
            channels: None,
            queue: Duration::from_millis(250),
        }
    }

    /// The sound system to open the device on.
    pub fn host(mut self, host: Host) -> Self {
        self.host = host;
        self
    }

    /// The device: an id from [`output_devices`] (matched exactly first),
    /// else a name, exactly or as a part of one in any case.
    pub fn device(mut self, device: impl Into<String>) -> Self {
        self.device = Some(device.into());
        self
    }

    /// Samples a second (default: the device's own).
    pub fn rate(mut self, rate: u32) -> Self {
        self.rate = Some(rate.max(1));
        self
    }

    /// Channels (default: the device's own).
    pub fn channels(mut self, channels: u16) -> Self {
        self.channels = Some(channels.max(1));
        self
    }

    /// How much sound is queued ahead of the device (default a quarter
    /// second): lower answers a pause or a seek sooner, higher rides out a
    /// busy machine.
    pub fn queue(mut self, ahead: Duration) -> Self {
        self.queue = ahead.max(Duration::from_millis(10));
        self
    }
}

/// The output devices of `host`, the default one marked.
///
/// # Errors
/// The host is not there or will not list its devices.
pub fn output_devices(host: Host) -> Result<Vec<Device>, AudioError> {
    let _quiet = quiet();
    let opened = host.open()?;
    let default = opened.default_output_device().and_then(|d| id_of(&d));
    let outputs = opened.output_devices().map_err(error)?;
    Ok(outputs
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

/// A sound card output opened by [`output`]: write interleaved samples
/// ([`write`](Self::write)) or buffers ([`write_matrix`](Self::write_matrix))
/// and they play in order, a queue's length ahead. Clones are the same
/// output; it plays until [`stop`](Self::stop)ped or its last clone is
/// dropped. Pausing, the volume and clearing the queue (for a seek) take
/// effect at once, not after what is queued.
#[derive(Clone)]
pub struct Output {
    shared: Arc<Shared>,
    _handle: Arc<Handle>,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output")
            .field("device", &self.shared.device)
            .field("rate", &self.shared.rate)
            .field("channels", &self.shared.channels)
            .finish()
    }
}

/// Dropped with the last clone of an [`Output`].
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.halt();
    }
}

/// What the stream's callback, its thread and the writers share.
struct Shared {
    device: String,
    rate: u32,
    channels: u16,
    /// Interleaved samples waiting to play.
    queue: Mutex<VecDeque<f32>>,
    /// Room was made in the queue, or the output stopped.
    room: Condvar,
    /// The most samples queued.
    capacity: usize,
    paused: AtomicBool,
    /// The volume, an `f32`'s bits.
    volume: AtomicU32,
    stop: AtomicBool,
    /// Why the stream broke, once it has.
    failed: Mutex<Option<String>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn halt(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.room.notify_all();
    }

    /// The callback: fill `out` from the queue (silence while paused or
    /// short), at the volume.
    fn play<T: SizedSample + FromSample<f32>>(&self, out: &mut [T]) {
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let paused = self.paused.load(Ordering::Relaxed);
        let mut queue = lock(&self.queue);
        for slot in out.iter_mut() {
            let sample = if paused {
                0.0
            } else {
                queue.pop_front().unwrap_or(0.0)
            };
            *slot = T::from_sample(sample * volume);
        }
        drop(queue);
        self.room.notify_all();
    }
}

/// Open a sound card output (see [`Output`]), on a thread of its own that
/// keeps the stream until it stops. Anywhere, before an app as well as in
/// one.
///
/// ```no_run
/// # #[cfg(feature = "audio")] {
/// use intuituive::audio::{output, OutputOptions};
///
/// let speaker = output(OutputOptions::new())?;
/// let tone: Vec<f64> = rich_ext::dsp::tone(440.0, f64::from(speaker.rate())).take(speaker.rate() as usize).collect();
/// speaker.write_matrix(&vec![tone]);
/// std::thread::sleep(std::time::Duration::from_secs(1));
/// # }
/// # Ok::<(), intuituive::audio::AudioError>(())
/// ```
///
/// # Errors
/// No such host or device, no output at the rate, or the stream would not
/// start.
pub fn output(options: OutputOptions) -> Result<Output, AudioError> {
    let (opened, result) = mpsc::channel();
    std::thread::Builder::new()
        .name("intuituive-audio-out".into())
        .spawn(move || {
            let _quiet = quiet();
            let (stream, shared) = match start(&options) {
                Ok(started) => started,
                Err(e) => {
                    let _ = opened.send(Err(e));
                    return;
                }
            };
            let _ = opened.send(Ok(shared.clone()));
            while !shared.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
            drop(stream);
        })
        .map_err(error)?;
    let shared = result
        .recv()
        .unwrap_or_else(|_| Err(error("the audio thread stopped")))?;
    Ok(Output {
        _handle: Arc::new(Handle(shared.clone())),
        shared,
    })
}

/// Open and start the stream of a new output.
fn start(options: &OutputOptions) -> Result<(cpal::Stream, Arc<Shared>), AudioError> {
    let host = options.host.open()?;
    let device = match &options.device {
        None => host
            .default_output_device()
            .ok_or_else(|| error("no default audio output"))?,
        Some(wanted) => {
            let devices: Vec<cpal::Device> = host.output_devices().map_err(error)?.collect();
            let lower = wanted.to_lowercase();
            let name = |d: &cpal::Device| {
                d.description()
                    .map(|d| d.name().to_string())
                    .unwrap_or_default()
            };
            let at = devices
                .iter()
                .position(|d| id_of(d).as_deref() == Some(wanted.as_str()))
                .or_else(|| devices.iter().position(|d| name(d) == *wanted))
                .or_else(|| {
                    devices
                        .iter()
                        .position(|d| name(d).to_lowercase().contains(&lower))
                })
                .ok_or_else(|| error(format!("no audio output named {wanted:?}")))?;
            devices
                .into_iter()
                .nth(at)
                .ok_or_else(|| error("the device went away"))?
        }
    };
    let label = device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| id_of(&device).unwrap_or_default());
    let default = device.default_output_config().map_err(error)?;
    let rate = options.rate.unwrap_or(default.sample_rate());
    let channels = options.channels.unwrap_or(default.channels());
    let format = device
        .supported_output_configs()
        .map_err(error)?
        .filter(|r| r.min_sample_rate() <= rate && rate <= r.max_sample_rate())
        .filter(|r| r.channels() == channels)
        .map(|r| r.sample_format())
        .min_by_key(|f| FORMATS.iter().position(|p| p == f).unwrap_or(usize::MAX))
        .ok_or_else(|| {
            error(format!(
                "the device has no output of {channels} channels at {rate} Hz"
            ))
        })?;
    let config = cpal::StreamConfig {
        channels,
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    };
    let capacity = (options.queue.as_secs_f64() * f64::from(rate)) as usize * usize::from(channels);
    let shared = Arc::new(Shared {
        device: label,
        rate,
        channels,
        queue: Mutex::new(VecDeque::with_capacity(capacity)),
        room: Condvar::new(),
        capacity: capacity.max(usize::from(channels)),
        paused: AtomicBool::new(false),
        volume: AtomicU32::new(1.0f32.to_bits()),
        stop: AtomicBool::new(false),
        failed: Mutex::new(None),
    });
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, &shared),
        SampleFormat::I16 => build::<i16>(&device, &config, &shared),
        SampleFormat::I32 => build::<i32>(&device, &config, &shared),
        SampleFormat::F64 => build::<f64>(&device, &config, &shared),
        SampleFormat::U16 => build::<u16>(&device, &config, &shared),
        SampleFormat::U8 => build::<u8>(&device, &config, &shared),
        SampleFormat::I8 => build::<i8>(&device, &config, &shared),
        SampleFormat::U32 => build::<u32>(&device, &config, &shared),
        other => return Err(error(format!("unsupported sample format {other}"))),
    }?;
    stream.play().map_err(error)?;
    Ok((stream, shared))
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

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: &Arc<Shared>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + FromSample<f32> + Send + 'static,
{
    let (data, errors) = (shared.clone(), shared.clone());
    device
        .build_output_stream::<T, _, _>(
            *config,
            move |out: &mut [T], _| data.play(out),
            move |e| match e.kind() {
                cpal::ErrorKind::Xrun
                | cpal::ErrorKind::DeviceChanged
                | cpal::ErrorKind::RealtimeDenied => {}
                _ => {
                    lock(&errors.failed).get_or_insert(e.to_string());
                    errors.halt();
                }
            },
            None,
        )
        .map_err(error)
}

impl Output {
    /// Samples a second.
    pub fn rate(&self) -> u32 {
        self.shared.rate
    }

    /// Channels a frame has.
    pub fn channels(&self) -> u16 {
        self.shared.channels
    }

    /// The device opened, by name.
    pub fn device(&self) -> &str {
        &self.shared.device
    }

    /// Queue interleaved `samples` (`L R L R …`, each from -1 to 1), waiting
    /// while the queue is full: what a player's thread does. Whether they
    /// were queued (not if the output has stopped).
    pub fn write(&self, samples: &[f32]) -> bool {
        let mut rest = samples;
        while !rest.is_empty() {
            let mut queue = lock(&self.shared.queue);
            while queue.len() >= self.shared.capacity && !self.stopped() {
                queue = self
                    .shared
                    .room
                    .wait_timeout(queue, Duration::from_millis(50))
                    .unwrap_or_else(|e| e.into_inner())
                    .0;
            }
            if self.stopped() {
                return false;
            }
            let room = self.shared.capacity - queue.len();
            let (now, later) = rest.split_at(room.min(rest.len()));
            queue.extend(now.iter().map(|s| s.clamp(-1.0, 1.0)));
            rest = later;
        }
        true
    }

    /// Queue a buffer, a row a channel: rows remixed to the output's
    /// channels (the last row repeated, extra rows dropped). At the
    /// output's rate: a buffer at another rate goes through a player.
    pub fn write_matrix(&self, buffer: &Matrix) -> bool {
        let channels = usize::from(self.shared.channels);
        let frames = buffer.iter().map(Vec::len).max().unwrap_or(0);
        if buffer.is_empty() {
            return true;
        }
        let mut out = Vec::with_capacity(frames * channels);
        for i in 0..frames {
            for c in 0..channels {
                let row = &buffer[c.min(buffer.len() - 1)];
                out.push(row.get(i).copied().unwrap_or(0.0) as f32);
            }
        }
        self.write(&out)
    }

    /// Drop what is queued (a seek, a skip): the next write plays at once.
    pub fn clear(&self) {
        lock(&self.shared.queue).clear();
        self.shared.room.notify_all();
    }

    /// How long what is queued plays.
    pub fn queued(&self) -> Duration {
        let samples = lock(&self.shared.queue).len();
        let frames = samples / usize::from(self.shared.channels.max(1));
        Duration::from_secs_f64(frames as f64 / f64::from(self.shared.rate))
    }

    /// Pause (silence, the queue kept) or play on.
    pub fn set_paused(&self, paused: bool) {
        self.shared.paused.store(paused, Ordering::SeqCst);
    }

    /// Whether it is paused.
    pub fn is_paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    /// The volume, from 0 (silent) to 1 (as written) and up to 2.
    pub fn set_volume(&self, volume: f64) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 2.0)
        } else {
            1.0
        };
        self.shared
            .volume
            .store((volume as f32).to_bits(), Ordering::SeqCst);
    }

    /// The volume.
    pub fn volume(&self) -> f64 {
        f64::from(f32::from_bits(self.shared.volume.load(Ordering::SeqCst)))
    }

    /// Why the stream broke, if it has.
    pub fn failed(&self) -> Option<String> {
        lock(&self.shared.failed).clone()
    }

    /// Stop playing, for good.
    pub fn stop(&self) {
        self.shared.halt();
    }

    /// Whether it has stopped (or broken).
    pub fn stopped(&self) -> bool {
        self.shared.stop.load(Ordering::SeqCst)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_and_opening_without_sound_does_not_panic() {
        // In a container or on CI there is usually no sound card at all.
        let devices = output_devices(Host::Default).unwrap_or_default();
        assert!(devices.iter().filter(|d| d.default).count() <= 1);
        let missing = output(OutputOptions::new().device("no such output for intuituive"));
        assert!(missing.is_err());
    }
}

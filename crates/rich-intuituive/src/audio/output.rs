//! Playing sound through a sound card, through cpal: [`output`] opens a
//! device and gives an [`Output`] to write samples to.
//! [`OutputOptions::null`] opens no device at all, but takes the samples
//! at its rate on a clock, and [`Output::tap`] records what an output
//! takes: sound tested, and captured, on a machine with no sound card.
//!
//! Under `rich record --audio` (or a tape's `Set Audio on`) the recorder
//! sets [`RECORD_AUDIO_ENV`] in the program's environment, and an output
//! that opens a device also writes what it plays to the recording; with
//! no device to open, it plays to the recording alone (see [`output`]).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, OutputStreamTimestamp, SampleFormat, SizedSample};

use super::capture::{id_of, quiet, quiet_stream_thread};
use super::{error, AudioError, Device, Host, Matrix};

/// What [`output`] opens, and how.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "options do nothing until they are passed to what takes them"]
pub struct OutputOptions {
    host: Host,
    device: Option<String>,
    rate: Option<u32>,
    channels: Option<u16>,
    queue: Duration,
    null: bool,
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
            null: false,
        }
    }

    /// No device: an output that takes samples at its rate on the real
    /// clock (44,100 a second in two channels unless [`rate`](Self::rate)
    /// and [`channels`](Self::channels) say otherwise) and drops them,
    /// never more than its queue ahead of a writer. What a test of sound
    /// opens on a machine with no sound card, with a [`tap`](Output::tap)
    /// to see what it took.
    ///
    /// ```
    /// use intuituive::audio::{output, OutputOptions};
    /// use std::time::Duration;
    ///
    /// let null = output(OutputOptions::null().rate(8000).channels(1)).unwrap();
    /// let tap = null.tap(Duration::from_secs(1));
    /// null.write(&[0.5; 400]);
    /// std::thread::sleep(Duration::from_millis(200));
    /// assert!(tap.samples().contains(&0.5));
    /// ```
    pub fn null() -> Self {
        OutputOptions {
            null: true,
            ..OutputOptions::new()
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
    /// What renders the samples, for an output made with a renderer
    /// ([`output_with`]), instead of the queue.
    render: Mutex<Option<Render>>,
    /// Made with a renderer: writes are refused.
    rendered: bool,
    /// The samples of a callback, before they become the device's format.
    scratch: Mutex<Vec<f32>>,
    /// The taps recording what the device takes.
    taps: Mutex<Vec<Weak<TapShared>>>,
    /// The recorder's stream, under `rich record --audio`.
    recording: Mutex<Option<Recorder<std::fs::File>>>,
}

/// A renderer: fills interleaved samples for the channels given.
type Render = Box<dyn FnMut(&mut [f32], u16) + Send>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn halt(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.room.notify_all();
    }

    fn new(
        device: String,
        rate: u32,
        channels: u16,
        queue: Duration,
        render: Option<Render>,
    ) -> Shared {
        let capacity = (queue.as_secs_f64() * f64::from(rate)) as usize * usize::from(channels);
        Shared {
            device,
            rate,
            channels,
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            room: Condvar::new(),
            capacity: capacity.max(usize::from(channels)),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            stop: AtomicBool::new(false),
            failed: Mutex::new(None),
            rendered: render.is_some(),
            render: Mutex::new(render),
            scratch: Mutex::new(Vec::new()),
            taps: Mutex::new(Vec::new()),
            recording: Mutex::new(None),
        }
    }

    /// The callback: fill `out` from the queue or the renderer (silence
    /// while paused or short), at the volume.
    fn play<T: SizedSample + FromSample<f32>>(&self, out: &mut [T], at: OutputStreamTimestamp) {
        let mut scratch = lock(&self.scratch);
        scratch.clear();
        scratch.resize(out.len(), 0.0);
        self.fill(&mut scratch, Some(at));
        for (slot, &sample) in out.iter_mut().zip(scratch.iter()) {
            *slot = T::from_sample(sample);
        }
    }

    /// Fill `out`, interleaved, with what plays next, at the volume, and
    /// hand it to the taps and the recording. `at` is when a device's
    /// callback says the block plays; a null output's blocks follow its
    /// clock and have none.
    fn fill(&self, out: &mut [f32], at: Option<OutputStreamTimestamp>) {
        let volume = f32::from_bits(self.volume.load(Ordering::Relaxed));
        let paused = self.paused.load(Ordering::Relaxed);
        out.fill(0.0);
        let mut render = lock(&self.render);
        if !paused {
            if let Some(renderer) = render.as_mut() {
                let channels = self.channels;
                let rendered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    renderer(out, channels)
                }));
                if rendered.is_err() {
                    *render = None;
                    out.fill(0.0);
                    lock(&self.failed).get_or_insert_with(|| "the renderer panicked".to_string());
                    self.halt();
                }
            } else {
                let mut queue = lock(&self.queue);
                for slot in out.iter_mut() {
                    *slot = queue.pop_front().unwrap_or(0.0);
                }
            }
        }
        drop(render);
        if volume != 1.0 {
            out.iter_mut().for_each(|s| *s *= volume);
        }
        let mut taps = lock(&self.taps);
        taps.retain(|tap| match tap.upgrade() {
            Some(tap) => {
                tap.take(out, self.channels);
                true
            }
            None => false,
        });
        drop(taps);
        let mut recording = lock(&self.recording);
        if let Some(recorder) = recording.as_mut() {
            if recorder.block(out, at, SystemTime::now()).is_err() {
                *recording = None;
            }
        }
        drop(recording);
        self.room.notify_all();
    }
}

/// The environment variable `rich record` sets when it records sound
/// (`rich record --audio`, a tape's `Set Audio on`): a directory. An
/// output opened while it is set ([`output`], [`output_with`]) plays on
/// its device as usual and also writes every block the device plays to a
/// file of its own in that directory, timed by the device's callback
/// (its latency counted). With no device to open (CI, a headless
/// machine) it takes its samples on the real clock instead, as
/// [`OutputOptions::null`] does, and writes them there. The recorder puts
/// the file on the recording's clock: into an MP4's sound track, or a
/// WAV beside the other files.
///
/// Each file is a 24-byte header, then the samples as they play,
/// interleaved little-endian `f32`: the magic `RRAUDIO1`, the rate (`u32`
/// LE), the channels (`u16` LE), two zero bytes, and when the first sample
/// played, in nanoseconds since the Unix epoch (`u64` LE). Sample `n`
/// plays `n / rate` seconds after that. Any program may write one, in
/// any name ending `.pcm`.
pub const RECORD_AUDIO_ENV: &str = "RICH_RECORD_AUDIO";

/// The magic a recorded stream starts with.
const RECORD_MAGIC: &[u8; 8] = b"RRAUDIO1";

/// The directory to record into, when a recorder asked for sound.
fn recording_dir() -> Option<PathBuf> {
    std::env::var_os(RECORD_AUDIO_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// A new, empty stream file in `dir`, and its path.
fn recording_file(dir: &std::path::Path) -> Option<(std::fs::File, PathBuf)> {
    static OPENED: AtomicU32 = AtomicU32::new(0);
    let n = OPENED.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{}-{n}.pcm", std::process::id()));
    Some((std::fs::File::create(&path).ok()?, path))
}

/// How far a block may play from where the stream's count puts it before
/// silence is added or samples are dropped to stay in step: a hundredth
/// of a second, more than a callback's jitter.
const RECORD_SLACK: Duration = Duration::from_millis(10);

/// Writes what an output plays as a recorder's stream (see
/// [`RECORD_AUDIO_ENV`]): the header when the first block is known to
/// play, then every block. Blocks timed by a device's callback are kept
/// in step with the device's clock: a gap (an underrun) becomes silence,
/// and a block that plays earlier than the count says loses its overlap.
struct Recorder<W: std::io::Write> {
    out: W,
    rate: u32,
    channels: u16,
    /// When the first sample played: set when the header is written.
    started: Option<SystemTime>,
    /// The first block's playback instant, on the device's clock.
    first: Option<cpal::StreamInstant>,
    /// Frames written.
    frames: u64,
}

impl<W: std::io::Write> Recorder<W> {
    fn new(out: W, rate: u32, channels: u16) -> Recorder<W> {
        Recorder {
            out,
            rate,
            channels: channels.max(1),
            started: None,
            first: None,
            frames: 0,
        }
    }

    /// Write the header: the first sample plays at `started`.
    fn header(&mut self, started: SystemTime) -> std::io::Result<()> {
        let nanos = started
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        let mut header = Vec::with_capacity(24);
        header.extend_from_slice(RECORD_MAGIC);
        header.extend_from_slice(&self.rate.to_le_bytes());
        header.extend_from_slice(&self.channels.to_le_bytes());
        header.extend_from_slice(&[0, 0]);
        header.extend_from_slice(&nanos.to_le_bytes());
        self.out.write_all(&header)?;
        self.started = Some(started);
        Ok(())
    }

    /// Record `block`, interleaved, which plays `at` by a device's
    /// callback (`now` being when the callback ran), or right after the
    /// last one when `at` is `None`.
    fn block(
        &mut self,
        block: &[f32],
        at: Option<OutputStreamTimestamp>,
        now: SystemTime,
    ) -> std::io::Result<()> {
        let channels = usize::from(self.channels);
        let mut skip = 0usize;
        match (at, self.started) {
            (None, None) => self.header(now)?,
            (None, Some(_)) => {}
            (Some(at), None) => {
                // The block is heard after the device's latency.
                self.header(now + at.playback.duration_since(at.callback))?;
                self.first = Some(at.playback);
            }
            (Some(at), Some(_)) => {
                let first = *self.first.get_or_insert(at.playback);
                let due = (at.playback.duration_since(first).as_secs_f64() * f64::from(self.rate))
                    .round() as i64;
                let drift = due - i64::try_from(self.frames).unwrap_or(i64::MAX);
                let slack = (RECORD_SLACK.as_secs_f64() * f64::from(self.rate)) as i64;
                if drift > slack {
                    let silence = vec![0u8; drift as usize * channels * 4];
                    self.out.write_all(&silence)?;
                    self.frames += drift as u64;
                } else if drift < -slack {
                    skip = ((-drift) as usize).min(block.len() / channels);
                }
            }
        }
        let kept = &block[skip * channels..];
        let bytes: Vec<u8> = kept.iter().flat_map(|s| s.to_le_bytes()).collect();
        // Written at once, unbuffered: the recorder may end the program at
        // any moment.
        self.out.write_all(&bytes)?;
        self.frames += (kept.len() / channels) as u64;
        Ok(())
    }
}

/// What a [`Tap`] and its output share.
struct TapShared {
    rate: u32,
    channels: u16,
    /// The newest samples taken, interleaved, at most `keep`.
    samples: Mutex<VecDeque<f32>>,
    keep: usize,
    /// Frames taken since the tap was made.
    frames: AtomicU64,
}

impl TapShared {
    fn take(&self, out: &[f32], channels: u16) {
        let mut samples = lock(&self.samples);
        let skip = out.len().saturating_sub(self.keep);
        samples.extend(&out[skip..]);
        let over = samples.len().saturating_sub(self.keep);
        samples.drain(..over);
        drop(samples);
        let frames = out.len() / usize::from(channels.max(1));
        self.frames.fetch_add(frames as u64, Ordering::Relaxed);
    }
}

/// What an output's device took, from when the tap was made
/// ([`Output::tap`]): the newest samples, up to the length it keeps, and
/// how many frames in all. Sound tested without a sound card (with
/// [`OutputOptions::null`]), or an app's sound recorded for a demo or a
/// bug report. It stops recording when dropped.
#[derive(Clone)]
pub struct Tap {
    shared: Arc<TapShared>,
}

impl std::fmt::Debug for Tap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tap")
            .field("rate", &self.shared.rate)
            .field("channels", &self.shared.channels)
            .field("frames", &self.frames())
            .finish()
    }
}

impl Tap {
    /// Samples a second.
    pub fn rate(&self) -> u32 {
        self.shared.rate
    }

    /// Channels a frame has.
    pub fn channels(&self) -> u16 {
        self.shared.channels
    }

    /// The samples kept, interleaved, oldest first, after the volume.
    pub fn samples(&self) -> Vec<f32> {
        lock(&self.shared.samples).iter().copied().collect()
    }

    /// The frames the device has taken since the tap was made, kept or
    /// not.
    pub fn frames(&self) -> u64 {
        self.shared.frames.load(Ordering::Relaxed)
    }

    /// Drop the samples kept (the count of frames goes on).
    pub fn clear(&self) {
        lock(&self.shared.samples).clear();
    }

    /// Write the samples kept to a WAV file, 32-bit float, at the tap's
    /// rate and channels.
    ///
    /// # Errors
    /// The file cannot be written.
    #[cfg(feature = "wav")]
    pub fn to_wav(&self, path: impl AsRef<std::path::Path>) -> Result<(), AudioError> {
        let spec = hound::WavSpec {
            channels: self.shared.channels,
            sample_rate: self.shared.rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(path, spec).map_err(error)?;
        for sample in self.samples() {
            writer.write_sample(sample).map_err(error)?;
        }
        writer.finalize().map_err(error)
    }
}

/// Open a sound card output (see [`Output`]), on a thread of its own that
/// keeps the stream until it stops. Anywhere, before an app as well as in
/// one.
///
/// While [`RECORD_AUDIO_ENV`] is set (a program run by `rich record
/// --audio`), the device plays as usual and what it plays is also written
/// to the recording, timed by its callbacks. When no device can be
/// opened, the output is a [null one](OutputOptions::null) at the rate
/// and channels asked for (44,100 a second in two channels unless they
/// say), whose sound goes to the recording alone; its
/// [`device`](Output::device) is then `"rich record"`.
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
    open_output(options, None)
}

/// Open a sound card output (as [`output`]) whose device's callback calls
/// `render` for each block it plays: a synthesizer, a tracker, a tone
/// generator, a [`Mixer`](super::Mixer) mixed by hand; anything that
/// makes sound when asked, its timing taken from the samples it makes,
/// with no queue between it and the device. `render` fills interleaved
/// samples (from -1 to 1) for the channels given; what it leaves is
/// silence. Volume, pause, [`tap`](Output::tap) and
/// [`failed`](Output::failed) work as for [`output`]; writes are refused.
/// A renderer that panics stops the output, which reports
/// [`failed`](Output::failed), and the app goes on. It runs on the audio
/// thread: keep it quick, and keep locks out of it where possible.
///
/// ```
/// use intuituive::audio::{output_with, OutputOptions};
/// use std::time::Duration;
///
/// let mut phase = 0.0f32;
/// let sine = output_with(OutputOptions::null().rate(8000).channels(1), move |out, _channels| {
///     for sample in out.iter_mut() {
///         *sample = (phase * std::f32::consts::TAU).sin() * 0.5;
///         phase = (phase + 440.0 / 8000.0).fract();
///     }
/// })
/// .unwrap();
/// let tap = sine.tap(Duration::from_secs(1));
/// std::thread::sleep(Duration::from_millis(100));
/// assert!(tap.samples().iter().any(|s| *s > 0.4));
/// assert!(!sine.write(&[0.0]));
/// ```
///
/// # Errors
/// As for [`output`].
pub fn output_with(
    options: OutputOptions,
    render: impl FnMut(&mut [f32], u16) + Send + 'static,
) -> Result<Output, AudioError> {
    open_output(options, Some(Box::new(render)))
}

/// Open an output that plays `render` or its queue.
fn open_output(options: OutputOptions, render: Option<Render>) -> Result<Output, AudioError> {
    if options.null {
        return Ok(null_output(&options, render, None));
    }
    let recording = recording_dir();
    let (opened, result) = mpsc::channel();
    let (to_device, render_back) = mpsc::channel();
    let fallback = options.clone();
    let dir = recording.clone();
    std::thread::Builder::new()
        .name("intuituive-audio-out".into())
        .spawn(move || {
            let _quiet = quiet();
            let mut render = render;
            let (stream, shared) = match start(&options, &mut render, dir.as_deref()) {
                Ok(started) => started,
                Err(e) => {
                    // The renderer comes back for a recording's null output.
                    let _ = to_device.send(render);
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
    let shared = match result
        .recv()
        .unwrap_or_else(|_| Err(error("the audio thread stopped")))
    {
        Ok(shared) => shared,
        // Recording with no device: the recording alone, on the clock.
        Err(_) if recording.is_some() => {
            let render = render_back.recv().ok().flatten();
            return Ok(null_output(&fallback, render, recording));
        }
        Err(e) => return Err(e),
    };
    Ok(Output {
        _handle: Arc::new(Handle(shared.clone())),
        shared,
    })
}

/// An output with no device: a thread takes its samples at its rate, and
/// writes them to a stream file in `recording`, when given.
fn null_output(
    options: &OutputOptions,
    render: Option<Render>,
    recording: Option<PathBuf>,
) -> Output {
    let rate = options.rate.unwrap_or(44_100);
    let channels = options.channels.unwrap_or(2);
    let name = if recording.is_some() {
        "rich record"
    } else {
        "null"
    };
    let shared = Arc::new(Shared::new(
        name.into(),
        rate,
        channels,
        options.queue,
        render,
    ));
    let taker = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("intuituive-audio-null".into())
        .spawn(move || {
            let start = Instant::now();
            if let Some((file, _)) = recording.and_then(|dir| recording_file(&dir)) {
                let mut recorder = Recorder::new(file, rate, channels);
                if recorder.header(SystemTime::now()).is_ok() {
                    *lock(&taker.recording) = Some(recorder);
                }
            }
            let mut taken = 0u64;
            let mut buffer = Vec::new();
            while !taker.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(2));
                let due = (start.elapsed().as_secs_f64() * f64::from(rate)) as u64;
                if due > taken {
                    buffer.clear();
                    buffer.resize((due - taken) as usize * usize::from(channels), 0.0);
                    taker.fill(&mut buffer, None);
                    taken = due;
                }
            }
        });
    if let Err(e) = spawned {
        lock(&shared.failed).get_or_insert(e.to_string());
        shared.halt();
    }
    Output {
        _handle: Arc::new(Handle(shared.clone())),
        shared,
    }
}

/// Open and start the stream of a new output; with `recording`, what it
/// plays is also written to a stream file there. `render` is taken only
/// once the stream plays: on an error it is still there.
fn start(
    options: &OutputOptions,
    render: &mut Option<Render>,
    recording: Option<&std::path::Path>,
) -> Result<(cpal::Stream, Arc<Shared>), AudioError> {
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
    let shared = Arc::new(Shared::new(
        label,
        rate,
        channels,
        options.queue,
        render.take(),
    ));
    let built = match format {
        SampleFormat::F32 => build::<f32>(&device, &config, &shared),
        SampleFormat::I16 => build::<i16>(&device, &config, &shared),
        SampleFormat::I32 => build::<i32>(&device, &config, &shared),
        SampleFormat::F64 => build::<f64>(&device, &config, &shared),
        SampleFormat::U16 => build::<u16>(&device, &config, &shared),
        SampleFormat::U8 => build::<u8>(&device, &config, &shared),
        SampleFormat::I8 => build::<i8>(&device, &config, &shared),
        SampleFormat::U32 => build::<u32>(&device, &config, &shared),
        other => Err(error(format!("unsupported sample format {other}"))),
    };
    // The stream file, before the stream plays, so its first block is
    // recorded.
    let file = match (&built, recording) {
        (Ok(_), Some(dir)) => recording_file(dir),
        _ => None,
    };
    let path = file.map(|(file, path)| {
        *lock(&shared.recording) = Some(Recorder::new(file, rate, channels));
        path
    });
    let stream = built.and_then(|stream| stream.play().map(|()| stream).map_err(error));
    match stream {
        Ok(stream) => Ok((stream, shared)),
        Err(e) => {
            *render = lock(&shared.render).take();
            lock(&shared.recording).take();
            if let Some(path) = path {
                let _ = std::fs::remove_file(path);
            }
            Err(e)
        }
    }
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
            move |out: &mut [T], info: &cpal::OutputCallbackInfo| {
                quiet_stream_thread();
                data.play(out, info.timestamp());
            },
            move |e| {
                quiet_stream_thread();
                match e.kind() {
                    cpal::ErrorKind::Xrun
                    | cpal::ErrorKind::DeviceChanged
                    | cpal::ErrorKind::RealtimeDenied => {}
                    _ => {
                        lock(&errors.failed).get_or_insert(e.to_string());
                        errors.halt();
                    }
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
    /// were queued (not if the output has stopped, or renders its own sound,
    /// [`output_with`]).
    pub fn write(&self, samples: &[f32]) -> bool {
        if self.shared.rendered {
            return false;
        }
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

    /// Record what the device takes from now on, the newest `keep` of it
    /// (see [`Tap`]).
    pub fn tap(&self, keep: Duration) -> Tap {
        let keep = (keep.as_secs_f64() * f64::from(self.shared.rate)).round() as usize
            * usize::from(self.shared.channels);
        let tap = Arc::new(TapShared {
            rate: self.shared.rate,
            channels: self.shared.channels,
            samples: Mutex::new(VecDeque::with_capacity(keep.min(1 << 22))),
            keep,
            frames: AtomicU64::new(0),
        });
        lock(&self.shared.taps).push(Arc::downgrade(&tap));
        Tap { shared: tap }
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
    use cpal::StreamInstant;

    fn at(callback_ms: u64, playback_ms: u64) -> Option<OutputStreamTimestamp> {
        Some(OutputStreamTimestamp {
            callback: StreamInstant::from_nanos(callback_ms * 1_000_000),
            playback: StreamInstant::from_nanos(playback_ms * 1_000_000),
        })
    }

    fn header_nanos(bytes: &[u8]) -> u64 {
        u64::from_le_bytes(bytes[16..24].try_into().unwrap())
    }

    fn samples(bytes: &[u8]) -> Vec<f32> {
        bytes[24..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect()
    }

    #[test]
    fn a_device_s_blocks_are_recorded_from_when_they_are_heard() {
        let now = UNIX_EPOCH + Duration::from_secs(1_000);
        let mut recorder = Recorder::new(Vec::new(), 1000, 1);
        // Heard 30 ms after the callback: the device's latency.
        recorder.block(&[0.5; 10], at(100, 130), now).unwrap();
        assert_eq!(&recorder.out[..8], RECORD_MAGIC);
        let heard = now + Duration::from_millis(30);
        let nanos = heard.duration_since(UNIX_EPOCH).unwrap().as_nanos() as u64;
        assert_eq!(header_nanos(&recorder.out), nanos);
        // The next block, right after: written as it is (a callback's
        // jitter within the slack changes nothing).
        recorder.block(&[0.25; 10], at(112, 142), now).unwrap();
        assert_eq!(samples(&recorder.out).len(), 20);
        // An underrun: the device plays the next block 50 ms late, so 50
        // ms of silence keep the count on the device's clock.
        recorder.block(&[1.0; 10], at(190, 190), now).unwrap();
        let all = samples(&recorder.out);
        assert_eq!(all.len(), 20 + 40 + 10);
        assert!(all[20..60].iter().all(|s| *s == 0.0));
        assert_eq!(all[60], 1.0);
        // A block that plays 30 ms earlier than the count says loses that
        // much of its start, so what follows stays in step.
        recorder.block(&[0.75; 40], at(170, 170), now).unwrap();
        let all = samples(&recorder.out);
        assert_eq!(all.len(), 70 + 10);
        assert_eq!(recorder.frames, 80);
    }

    #[test]
    fn a_null_output_s_blocks_follow_one_another() {
        let now = UNIX_EPOCH + Duration::from_secs(5);
        let mut recorder = Recorder::new(Vec::new(), 8000, 2);
        recorder.block(&[0.5; 4], None, now).unwrap();
        recorder
            .block(&[0.25; 4], None, now + Duration::from_secs(9))
            .unwrap();
        assert_eq!(header_nanos(&recorder.out), 5_000_000_000);
        assert_eq!(
            samples(&recorder.out),
            [0.5, 0.5, 0.5, 0.5, 0.25, 0.25, 0.25, 0.25]
        );
    }

    #[test]
    fn a_device_output_tees_what_it_plays_into_the_recording() {
        // The device's side of an output, with no device: the callback's
        // `play`, as cpal calls it.
        let dir = std::env::temp_dir().join(format!("intuituive-tee-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shared = Shared::new("card".into(), 1000, 1, Duration::from_secs(1), None);
        *lock(&shared.recording) = Some(Recorder::new(recording_file(&dir).unwrap().0, 1000, 1));
        let tap_free: Vec<f32> = (0..20).map(|i| i as f32 / 40.0).collect();
        lock(&shared.queue).extend(tap_free.iter().copied());
        let mut out = [0i16; 10];
        shared.play(&mut out, at(0, 20).unwrap());
        assert!(out[1] > 0 && out[9] > out[1], "the device plays it");
        shared.play(&mut out, at(10, 30).unwrap());
        drop(lock(&shared.recording).take());
        let file = std::fs::read_dir(&dir)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let bytes = std::fs::read(&file).unwrap();
        assert_eq!(samples(&bytes), tap_free, "and the recording has the same");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_and_opening_without_sound_does_not_panic() {
        // In a container or on CI there is usually no sound card at all.
        let devices = output_devices(Host::Default).unwrap_or_default();
        assert!(devices.iter().filter(|d| d.default).count() <= 1);
        let missing = output(OutputOptions::new().device("no such output for intuituive"));
        assert!(missing.is_err());
    }
}

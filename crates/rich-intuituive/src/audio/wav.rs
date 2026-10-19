//! WAV files in and out, through hound.

use std::cell::RefCell;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use super::{error, AudioError, Frame, Matrix};
use crate::reactive::{watch, Signal};

/// Read a WAV file as buffers of `buffer` frames, a row a channel, each
/// sample from -1 to 1, as [`pcm`](super::pcm) reads raw PCM: an iterator
/// that reads a buffer each time it is asked. Integer samples of 8 to 32
/// bits and 32-bit floats are read; a short last buffer is padded with
/// silence.
///
/// ```no_run
/// let song = intuituive::audio::wav("song.wav", 2048)?;
/// println!("{} Hz, {} channels, {:?}", song.rate(), song.channels(), song.duration());
/// for buffer in song {
///     // one Matrix a buffer
/// }
/// # Ok::<(), intuituive::audio::AudioError>(())
/// ```
///
/// # Errors
/// The file cannot be opened or is not a WAV file hound reads.
pub fn wav(path: impl AsRef<Path>, buffer: usize) -> Result<Wav, AudioError> {
    let reader = hound::WavReader::open(path).map_err(error)?;
    let spec = reader.spec();
    let scale = match spec.sample_format {
        hound::SampleFormat::Int => (1i64 << (spec.bits_per_sample.clamp(1, 32) - 1)) as f64,
        hound::SampleFormat::Float => 1.0,
    };
    Ok(Wav {
        reader,
        spec,
        scale,
        buffer: buffer.max(1),
    })
}

/// A WAV file read a buffer at a time: see [`wav`].
pub struct Wav {
    reader: hound::WavReader<BufReader<File>>,
    spec: hound::WavSpec,
    /// What an integer sample is divided by.
    scale: f64,
    buffer: usize,
}

impl std::fmt::Debug for Wav {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wav").field("spec", &self.spec).finish()
    }
}

impl Wav {
    /// Samples a second.
    pub fn rate(&self) -> u32 {
        self.spec.sample_rate
    }

    /// Channels: the rows of each buffer.
    pub fn channels(&self) -> u16 {
        self.spec.channels
    }

    /// How long the whole file plays.
    pub fn duration(&self) -> Duration {
        let rate = f64::from(self.spec.sample_rate.max(1));
        Duration::from_secs_f64(f64::from(self.reader.duration()) / rate)
    }

    /// Read on from `to` (kept within the file): the next buffer starts
    /// there, as a player's seek.
    ///
    /// # Errors
    /// The file could not be read there.
    pub fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        let frame = (to.as_secs_f64() * f64::from(self.spec.sample_rate)).round() as u64;
        let frame = frame.min(u64::from(self.reader.duration())) as u32;
        self.reader.seek(frame).map_err(error)
    }
}

impl super::Source for Wav {
    fn rate(&self) -> u32 {
        Wav::rate(self)
    }

    fn channels(&self) -> u16 {
        Wav::channels(self)
    }

    fn duration(&self) -> Option<Duration> {
        Some(Wav::duration(self))
    }

    fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        Wav::seek(self, to)
    }

    fn next_buffer(&mut self) -> Option<Result<Matrix, AudioError>> {
        self.next().map(Ok)
    }
}

impl Iterator for Wav {
    type Item = Matrix;

    fn next(&mut self) -> Option<Matrix> {
        let channels = usize::from(self.spec.channels.max(1));
        let wanted = self.buffer * channels;
        let samples: Vec<f64> = match self.spec.sample_format {
            hound::SampleFormat::Float => self
                .reader
                .samples::<f32>()
                .take(wanted)
                .map_while(Result::ok)
                .map(f64::from)
                .collect(),
            hound::SampleFormat::Int => self
                .reader
                .samples::<i32>()
                .take(wanted)
                .map_while(Result::ok)
                .map(|s| f64::from(s) / self.scale)
                .collect(),
        };
        if samples.is_empty() {
            return None;
        }
        let mut rows = vec![Vec::with_capacity(self.buffer); channels];
        for (i, sample) in samples.iter().enumerate() {
            rows[i % channels].push(sample.clamp(-1.0, 1.0));
        }
        for row in &mut rows {
            row.resize(self.buffer, 0.0);
        }
        Some(rows)
    }
}

/// A new 16-bit WAV file at `path` (replacing one there), `channels`
/// channels at `rate` samples a second, to write buffers to.
///
/// ```no_run
/// let mut out = intuituive::audio::wav_writer("tone.wav", 48_000, 1)?;
/// let tone: Vec<f64> = rich_ext::dsp::tone(440.0, 48_000.0).take(48_000).collect();
/// out.write(&vec![tone])?;
/// out.finish()?;
/// # Ok::<(), intuituive::audio::AudioError>(())
/// ```
///
/// # Errors
/// The file cannot be created.
pub fn wav_writer(
    path: impl AsRef<Path>,
    rate: u32,
    channels: u16,
) -> Result<WavWriter, AudioError> {
    let spec = hound::WavSpec {
        channels: channels.max(1),
        sample_rate: rate.max(1),
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let writer = hound::WavWriter::create(path, spec).map_err(error)?;
    Ok(WavWriter { writer, spec })
}

/// A WAV file being written: see [`wav_writer`]. Dropping it finishes the
/// file too, but without saying whether that worked.
pub struct WavWriter {
    writer: hound::WavWriter<BufWriter<File>>,
    spec: hound::WavSpec,
}

impl std::fmt::Debug for WavWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WavWriter")
            .field("spec", &self.spec)
            .finish()
    }
}

impl WavWriter {
    /// Add a buffer, a row a channel: rows past the file's channels are
    /// left out, missing ones are silence, and samples are clamped to -1
    /// to 1. The longest row sets the buffer's length.
    ///
    /// # Errors
    /// The file could not be written.
    pub fn write(&mut self, buffer: &Matrix) -> Result<(), AudioError> {
        let channels = usize::from(self.spec.channels);
        let frames = buffer.iter().map(Vec::len).max().unwrap_or(0);
        for i in 0..frames {
            for c in 0..channels {
                let sample = buffer.get(c).and_then(|row| row.get(i)).copied();
                let sample = sample.unwrap_or(0.0).clamp(-1.0, 1.0);
                let value = (sample * 32767.0).round() as i16;
                self.writer.write_sample(value).map_err(error)?;
            }
        }
        Ok(())
    }

    /// Finish the file: write its header's lengths and flush it.
    ///
    /// # Errors
    /// The file could not be written.
    pub fn finish(self) -> Result<(), AudioError> {
        self.writer.finalize().map_err(error)
    }
}

/// Record what `samples` delivers (a capture's or a reader's frames) to a
/// 16-bit WAV file at `path`, a frame each time its number changes, until
/// [`Recording::stop`] or the app ends. Frames the app had no time for are
/// not in it, so it is what was shown, not every sample captured.
///
/// # Errors
/// The file cannot be created.
///
/// # Panics
/// Outside a running or building app.
pub fn record(
    samples: Signal<Option<Frame>>,
    path: impl AsRef<Path>,
    rate: u32,
    channels: u16,
) -> Result<Recording, AudioError> {
    let writer = Rc::new(RefCell::new(Some(wav_writer(path, rate, channels)?)));
    let error = Rc::new(RefCell::new(None));
    let recording = Recording {
        writer: writer.clone(),
        error: error.clone(),
    };
    watch(
        move || samples.with(|f| f.as_ref().map(|f| f.number)),
        move |_, _| {
            let mut slot = writer.borrow_mut();
            let Some(out) = slot.as_mut() else {
                return;
            };
            let written = samples.with_untracked(|f| match f {
                Some(frame) => out.write(&frame.samples),
                None => Ok(()),
            });
            if let Err(e) = written {
                *error.borrow_mut() = Some(e);
                *slot = None;
            }
        },
    );
    Ok(recording)
}

/// A recording in progress: see [`record`]. Clones are the same recording.
#[derive(Clone)]
pub struct Recording {
    writer: Rc<RefCell<Option<WavWriter>>>,
    error: Rc<RefCell<Option<AudioError>>>,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("recording", &self.is_recording())
            .finish()
    }
}

impl Recording {
    /// Whether frames are still being written.
    pub fn is_recording(&self) -> bool {
        self.writer.borrow().is_some()
    }

    /// Stop and finish the file. Frames after are not written.
    ///
    /// # Errors
    /// Writing a frame failed (which stopped the recording), or finishing
    /// the file did.
    pub fn stop(&self) -> Result<(), AudioError> {
        if let Some(e) = self.error.borrow_mut().take() {
            return Err(e);
        }
        match self.writer.borrow_mut().take() {
            Some(writer) => writer.finish(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::signal;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("intuituive-wav-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn a_written_file_reads_back() {
        let path = scratch("round.wav");
        let mut out = wav_writer(&path, 8000, 2).unwrap();
        out.write(&vec![vec![0.5, -0.5, 1.0], vec![0.25]]).unwrap();
        out.finish().unwrap();
        let file = wav(&path, 2).unwrap();
        assert_eq!((file.rate(), file.channels()), (8000, 2));
        assert_eq!(file.duration(), Duration::from_secs_f64(3.0 / 8000.0));
        let buffers: Vec<Matrix> = file.collect();
        let q = |v: f64| (v * 32767.0).round() / 32768.0;
        assert_eq!(
            buffers,
            vec![
                vec![vec![q(0.5), q(-0.5)], vec![q(0.25), 0.0]],
                // The last padded with silence.
                vec![vec![q(1.0), 0.0], vec![0.0, 0.0]],
            ]
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn a_seek_reads_on_from_there() {
        let path = scratch("seek");
        let mut out = wav_writer(&path, 10, 1).unwrap();
        let ramp: Vec<f64> = (0..20).map(|i| f64::from(i) / 32.0).collect();
        out.write(&vec![ramp]).unwrap();
        out.finish().unwrap();
        let mut song = wav(&path, 2).unwrap();
        assert_eq!(song.duration(), Duration::from_secs(2));
        song.seek(Duration::from_millis(1500)).unwrap();
        let next = song.next().unwrap();
        assert!((next[0][0] - 15.0 / 32.0).abs() < 1e-3, "{next:?}");
        // Past the end: nothing more.
        song.seek(Duration::from_secs(9)).unwrap();
        assert_eq!(song.next(), None);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn floats_are_read_as_they_are() {
        let path = scratch("float.wav");
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 100,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let mut writer = hound::WavWriter::create(&path, spec).unwrap();
        for s in [0.25f32, -0.75] {
            writer.write_sample(s).unwrap();
        }
        writer.finalize().unwrap();
        let buffers: Vec<Matrix> = wav(&path, 4).unwrap().collect();
        assert_eq!(buffers, vec![vec![vec![0.25, -0.75, 0.0, 0.0]]]);
        std::fs::remove_file(path).ok();
        assert!(wav(scratch("missing.wav"), 4).is_err());
    }

    #[test]
    fn a_recording_writes_each_frame_once() {
        let path = scratch("recorded.wav");
        let target = path.clone();
        let app = crate::App::new(move || {
            let frames = signal(None::<Frame>);
            let recording = record(frames, &target, 1000, 1).unwrap();
            let push = move |number: u64| {
                frames.set(Some(Frame {
                    number,
                    samples: vec![vec![0.5; 2]],
                }));
            };
            crate::label("rec")
                .on_key("a", move |_| push(1))
                .on_key("b", move |_| push(2))
                .on_key("s", move |cx| {
                    recording.stop().unwrap();
                    cx.quit();
                })
                .on_key("c", move |_| push(3))
        });
        // The same frame twice is written once; one after the stop never.
        app.render_with(&["a", "a", "b", "s", "c"], 4, 1).unwrap();
        let file = wav(&path, 16).unwrap();
        assert_eq!(file.duration(), Duration::from_secs_f64(4.0 / 1000.0));
        std::fs::remove_file(path).ok();
    }
}

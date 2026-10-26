//! Sources a player reads ([`Source`]): a decoded file or a clip in
//! memory, seekable, and the decoder that turns one track after another
//! into the samples an output plays, at its rate and a speed.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use super::{AudioError, Matrix};

/// Sound a player reads a buffer at a time: a decoded file
/// ([`wav`](super::wav) with the `wav` feature), a [`clip`] in memory, or
/// a decoder of your own (another codec). Buffers are a row a channel,
/// each sample from -1 to 1, as everywhere in the plugin.
pub trait Source: Send {
    /// Samples a second.
    fn rate(&self) -> u32;
    /// Channels: the rows of each buffer.
    fn channels(&self) -> u16;
    /// How long it plays, when it is known (`None`: a stream).
    fn duration(&self) -> Option<Duration>;
    /// Read on from `to`.
    ///
    /// # Errors
    /// It cannot seek there (a stream, a read error).
    fn seek(&mut self, to: Duration) -> Result<(), AudioError>;
    /// The next buffer, `None` at the end, an error when a read failed.
    fn next_buffer(&mut self) -> Option<Result<Matrix, AudioError>>;
}

/// Samples in memory as a [`Source`]: a tone, a test signal, a sound
/// decoded elsewhere. `samples` is a row a channel at `rate` samples a
/// second.
///
/// ```
/// use intuituive::audio::{clip, Source};
/// use std::time::Duration;
///
/// let tone: Vec<f64> = rich_ext::dsp::tone(440.0, 8000.0).take(8000).collect();
/// let mut sound = clip(8000, vec![tone]);
/// assert_eq!(sound.duration(), Some(Duration::from_secs(1)));
/// sound.seek(Duration::from_millis(500)).unwrap();
/// let rest: usize = std::iter::from_fn(|| sound.next_buffer()).map(|b| b.unwrap()[0].len()).sum();
/// assert_eq!(rest, 4000);
/// ```
pub fn clip(rate: u32, samples: Matrix) -> Clip {
    Clip {
        rate: rate.max(1),
        samples,
        at: 0,
    }
}

/// Samples in memory: see [`clip`].
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub(super) rate: u32,
    pub(super) samples: Matrix,
    at: usize,
}

/// Frames a [`Clip`] gives a buffer.
const CLIP_BUFFER: usize = 1024;

impl Clip {
    pub(super) fn len(&self) -> usize {
        self.samples.iter().map(Vec::len).max().unwrap_or(0)
    }
}

impl Source for Clip {
    fn rate(&self) -> u32 {
        self.rate
    }

    fn channels(&self) -> u16 {
        self.samples.len().max(1) as u16
    }

    fn duration(&self) -> Option<Duration> {
        Some(Duration::from_secs_f64(
            self.len() as f64 / f64::from(self.rate),
        ))
    }

    fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        let frame = (to.as_secs_f64() * f64::from(self.rate)).round() as usize;
        self.at = frame.min(self.len());
        Ok(())
    }

    fn next_buffer(&mut self) -> Option<Result<Matrix, AudioError>> {
        let len = self.len();
        if self.at >= len {
            return None;
        }
        let end = (self.at + CLIP_BUFFER).min(len);
        let buffer = self
            .samples
            .iter()
            .map(|row| {
                (self.at..end)
                    .map(|i| row.get(i).copied().unwrap_or(0.0))
                    .collect()
            })
            .collect();
        self.at = end;
        Some(Ok(buffer))
    }
}

/// Open the sound file at `path` as a [`Source`], by its extension: WAV
/// (with the `wav` feature). Other codecs (MP3, FLAC, Ogg, AAC) are later
/// work; a player can be given an opener of its own for them
/// ([`PlayerOptions::opener`](super::PlayerOptions::opener)).
///
/// # Errors
/// The file cannot be read, or no decoder in this build reads its kind.
pub fn open_source(path: &Path) -> Result<Box<dyn Source>, AudioError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        #[cfg(feature = "wav")]
        "wav" | "wave" => Ok(Box::new(super::wav(path, 1024)?)),
        _ => Err(AudioError(format!(
            "no decoder for {:?} files in this build (WAV with the `wav` feature)",
            if ext.is_empty() {
                "unnamed"
            } else {
                ext.as_str()
            }
        ))),
    }
}

/// What a [`Decoder`] did while filling.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub(super) enum Event {
    /// The source ended and the next one (its index) went on without a
    /// gap.
    Advanced(usize),
    /// The last source ended: silence from here.
    Ended,
    /// A read failed.
    Failed(String),
}

/// Turns one track after another into interleaved samples at an output's
/// rate and channels: each source resampled (linearly) from its rate to
/// the output's times the speed, its channels remixed (a mono track fills
/// both of a stereo output; extra channels are dropped), and, gapless,
/// the next track's samples straight after the last of this one.
#[cfg_attr(not(feature = "audio"), allow(dead_code))]
pub(super) struct Decoder {
    rate: u32,
    channels: usize,
    speed: f64,
    track: Option<(usize, Box<dyn Source>)>,
    /// The source's frames not yet played past, a row a channel.
    pending: Vec<VecDeque<f64>>,
    /// Where the next output frame falls, in frames from `pending`'s
    /// first.
    at: f64,
    ended: bool,
}

#[cfg_attr(not(feature = "audio"), allow(dead_code))]
impl Decoder {
    pub(super) fn new(rate: u32, channels: usize) -> Decoder {
        Decoder {
            rate: rate.max(1),
            channels: channels.max(1),
            speed: 1.0,
            track: None,
            pending: Vec::new(),
            at: 0.0,
            ended: true,
        }
    }

    /// Play `source` (track `index`) from where it is now, dropping what
    /// was pending.
    pub(super) fn load(&mut self, index: usize, source: Box<dyn Source>) {
        self.track = Some((index, source));
        self.pending.clear();
        self.at = 0.0;
        self.ended = false;
    }

    /// The track playing, if any.
    pub(super) fn track(&self) -> Option<usize> {
        self.track.as_ref().map(|(i, _)| *i)
    }

    pub(super) fn set_speed(&mut self, speed: f64) {
        if speed.is_finite() && speed > 0.0 {
            self.speed = speed;
        }
    }

    /// Seek the track playing.
    pub(super) fn seek(&mut self, to: Duration) -> Result<(), AudioError> {
        self.pending.clear();
        self.at = 0.0;
        self.ended = false;
        match &mut self.track {
            Some((_, source)) => source.seek(to),
            None => Ok(()),
        }
    }

    /// `frames` interleaved frames, taking the next track from `next`
    /// (given the index after the one ending) when the track runs out, if
    /// it gives one; silence after the last.
    pub(super) fn fill(
        &mut self,
        frames: usize,
        next: &mut dyn FnMut(usize) -> Option<Box<dyn Source>>,
    ) -> (Vec<f32>, Vec<Event>) {
        let mut out = Vec::with_capacity(frames * self.channels);
        let mut events = Vec::new();
        for _ in 0..frames {
            // Two source frames around the output frame.
            while !self.ended
                && self.pending.first().map_or(0, VecDeque::len) < self.at as usize + 2
            {
                if !self.pull(next, &mut events) {
                    break;
                }
            }
            let have = self.pending.first().map_or(0, VecDeque::len);
            let i = self.at as usize;
            if i >= have {
                out.extend(std::iter::repeat_n(0.0, self.channels));
                continue;
            }
            let frac = self.at - i as f64;
            for c in 0..self.channels {
                let row = &self.pending[c.min(self.pending.len() - 1)];
                let a = row[i];
                let b = row.get(i + 1).copied().unwrap_or(a);
                out.push((a + (b - a) * frac) as f32);
            }
            let step = self.step();
            self.at += step;
            let used = (self.at as usize).min(have.saturating_sub(1));
            if used > 0 {
                for row in &mut self.pending {
                    row.drain(..used.min(row.len()));
                }
                self.at -= used as f64;
            }
        }
        (out, events)
    }

    /// Source frames an output frame moves on.
    fn step(&self) -> f64 {
        let source = self.track.as_ref().map_or(self.rate, |(_, s)| s.rate());
        f64::from(source) / f64::from(self.rate) * self.speed
    }

    /// Read the next buffer into `pending` (or the next track); whether
    /// there may be more.
    fn pull(
        &mut self,
        next: &mut dyn FnMut(usize) -> Option<Box<dyn Source>>,
        events: &mut Vec<Event>,
    ) -> bool {
        let Some((index, source)) = &mut self.track else {
            self.ended = true;
            return false;
        };
        match source.next_buffer() {
            Some(Ok(buffer)) => {
                let rows = buffer.len().max(1);
                if self.pending.len() != rows {
                    // A track with other channels: what is pending carries
                    // over into each row.
                    let last = self.pending.last().cloned().unwrap_or_default();
                    self.pending.resize(rows, last);
                }
                for (row, samples) in self.pending.iter_mut().zip(&buffer) {
                    row.extend(samples.iter().map(|s| s.clamp(-1.0, 1.0)));
                }
                true
            }
            Some(Err(error)) => {
                events.push(Event::Failed(error.to_string()));
                self.ended = true;
                false
            }
            None => {
                let following = *index + 1;
                match next(following) {
                    Some(source) => {
                        self.track = Some((following, source));
                        events.push(Event::Advanced(following));
                        true
                    }
                    None => {
                        self.ended = true;
                        events.push(Event::Ended);
                        false
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(rate: u32, from: f64, n: usize) -> Box<dyn Source> {
        Box::new(clip(
            rate,
            vec![(0..n).map(|i| from + i as f64 / 100.0).collect()],
        ))
    }

    #[test]
    fn tracks_follow_each_other_without_a_gap() {
        let mut decoder = Decoder::new(100, 1);
        decoder.load(0, ramp(100, 0.0, 3));
        let mut nexts = vec![ramp(100, 0.5, 3)];
        let (out, events) = decoder.fill(8, &mut |i| (i == 1).then(|| nexts.pop()).flatten());
        let want = [0.0, 0.01, 0.02, 0.5, 0.51, 0.52, 0.0, 0.0];
        for (got, want) in out.iter().zip(want) {
            assert!((f64::from(*got) - want).abs() < 1e-6, "{out:?}");
        }
        assert_eq!(events, [Event::Advanced(1), Event::Ended]);
        assert_eq!(decoder.track(), Some(1));
    }

    #[test]
    fn rates_and_speed_resample_and_mono_fills_both_channels() {
        // 50 Hz into 100 Hz: each source frame twice, halfway between.
        let mut decoder = Decoder::new(100, 2);
        decoder.load(0, ramp(50, 0.0, 3));
        let (out, _) = decoder.fill(4, &mut |_| None);
        let got: Vec<f64> = out
            .iter()
            .map(|s| (f64::from(*s) * 1000.0).round())
            .collect();
        assert_eq!(got, [0.0, 0.0, 5.0, 5.0, 10.0, 10.0, 15.0, 15.0]);
        // Twice as fast at the same rate: every other frame.
        let mut fast = Decoder::new(100, 1);
        fast.set_speed(2.0);
        fast.load(0, ramp(100, 0.0, 10));
        let (out, _) = fast.fill(3, &mut |_| None);
        let got: Vec<f64> = out
            .iter()
            .map(|s| (f64::from(*s) * 100.0).round())
            .collect();
        assert_eq!(got, [0.0, 2.0, 4.0]);
    }

    #[test]
    fn a_seek_reads_on_from_there() {
        let mut decoder = Decoder::new(100, 1);
        decoder.load(0, ramp(100, 0.0, 100));
        decoder.seek(Duration::from_millis(500)).unwrap();
        let (out, _) = decoder.fill(1, &mut |_| None);
        assert!((f64::from(out[0]) - 0.5).abs() < 1e-6, "{out:?}");
    }
}

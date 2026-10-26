//! Sound in recordings: `Set Audio on` in a tape, or `rich record --audio`.
//!
//! The recorder makes a directory and names it in the recorded program's
//! environment as [`ENV`] (`RICH_RECORD_AUDIO`). A program that plays
//! sound writes what it plays there instead of to a sound card:
//! intuiTUIve's audio outputs do it by themselves. Each stream is a file
//! of its own, ending `.pcm`: a 24-byte header, then the samples as they
//! play, interleaved little-endian `f32`.
//!
//! | bytes | what |
//! |-------|------|
//! | 0..8  | the magic `RRAUDIO1` |
//! | 8..12 | samples a second (`u32` LE) |
//! | 12..14 | channels (`u16` LE) |
//! | 14..16 | zero |
//! | 16..24 | when the first sample played, nanoseconds since the Unix epoch (`u64` LE) |
//!
//! Sample `n` plays `n / rate` seconds after the time in the header: a
//! program writes its samples as they play, or all at once. When the tape
//! ends, the recorder puts every stream on the recording's clock
//! ([`compose`]): what played while the recording was hidden (`Hide`) is
//! cut, as the frames are, and the streams are mixed into one [`Audio`]
//! track. An MP4 carries it as AAC; the other outputs get a WAV beside
//! them ([`Audio::wav`]).

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The environment variable the recorded program finds the directory in.
pub const ENV: &str = "RICH_RECORD_AUDIO";

/// The magic a stream starts with.
pub const MAGIC: &[u8; 8] = b"RRAUDIO1";

/// The size of a stream's header, in bytes.
pub const HEADER: usize = 24;

/// One stream a program wrote.
#[derive(Clone, Debug, PartialEq)]
pub struct Stream {
    /// Samples a second.
    pub rate: u32,
    /// Channels a frame has.
    pub channels: u16,
    /// When its first sample played.
    pub start: SystemTime,
    /// Its samples, interleaved.
    pub samples: Vec<f32>,
}

impl Stream {
    /// A stream's header and samples, for a program that writes its own.
    pub fn to_bytes(&self) -> Vec<u8> {
        let nanos = self
            .start
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX));
        let mut out = Vec::with_capacity(HEADER + self.samples.len() * 4);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out.extend_from_slice(&nanos.to_le_bytes());
        for sample in &self.samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }
        out
    }

    /// Read a stream: `None` when the header is not one. A stream cut off
    /// mid-frame (its program ended while writing) keeps its whole frames.
    pub fn parse(bytes: &[u8]) -> Option<Stream> {
        if bytes.len() < HEADER || &bytes[..8] != MAGIC {
            return None;
        }
        let rate = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let channels = u16::from_le_bytes(bytes[12..14].try_into().ok()?);
        let nanos = u64::from_le_bytes(bytes[16..24].try_into().ok()?);
        if rate == 0 || channels == 0 {
            return None;
        }
        let frame = usize::from(channels) * 4;
        let body = &bytes[HEADER..];
        let whole = body.len() / frame * frame;
        let samples = body[..whole]
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .map(|s| if s.is_finite() { s } else { 0.0 })
            .collect();
        Some(Stream {
            rate,
            channels,
            start: UNIX_EPOCH + Duration::from_nanos(nanos),
            samples,
        })
    }

    /// Frames it holds.
    fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }

    /// Channel `channel` (the last one past its channels) at `position`
    /// frames, between frames by straight lines; silence outside.
    fn at(&self, position: f64, channel: usize) -> f32 {
        let channels = usize::from(self.channels);
        let channel = channel.min(channels - 1);
        let frames = self.frames();
        if position < 0.0 || frames == 0 || position > (frames - 1) as f64 {
            return 0.0;
        }
        let i = position.floor() as usize;
        let fraction = (position - i as f64) as f32;
        let a = self.samples[i * channels + channel];
        let b = self
            .samples
            .get((i + 1) * channels + channel)
            .copied()
            .unwrap_or(a);
        a + (b - a) * fraction
    }
}

/// The streams in `dir` (files ending `.pcm`), in name order; files that
/// are not streams are skipped.
pub fn read_dir(dir: &Path) -> Vec<Stream> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "pcm"))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| std::fs::read(p).ok())
        .filter_map(|bytes| Stream::parse(&bytes))
        .collect()
}

/// A stretch of the recording that was shown (from a `Show` to a `Hide`):
/// what the wall clock said at its start, and where it starts and how long
/// it lasts on the recording's timeline, in seconds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    pub wall: SystemTime,
    pub at: f64,
    pub length: f64,
}

/// A recording's sound: one track, on the video's clock.
#[derive(Clone, Debug, PartialEq)]
pub struct Audio {
    /// Samples a second.
    pub rate: u32,
    /// Channels a frame has.
    pub channels: u16,
    /// Where the track starts on the video's clock, in seconds: what an MP4
    /// gives FFmpeg as `-itsoffset`.
    pub offset: f64,
    /// The track, interleaved, from `offset` on.
    pub samples: Vec<f32>,
}

impl Audio {
    /// How long the track plays, from its offset.
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.channels.max(1)) / f64::from(self.rate.max(1))
    }

    /// The track as a 16-bit PCM WAV file. With `from_zero`, silence fills
    /// its [`offset`](Self::offset) first, so it plays in step with a GIF,
    /// an APNG or the page's player started at the same moment.
    pub fn wav(&self, from_zero: bool) -> Vec<u8> {
        let lead = if from_zero {
            (self.offset.max(0.0) * f64::from(self.rate)).round() as usize
                * usize::from(self.channels)
        } else {
            0
        };
        let count = lead + self.samples.len();
        let data = u32::try_from(count * 2).unwrap_or(u32::MAX - 36);
        let block = self.channels * 2;
        let mut out = Vec::with_capacity(44 + count * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&self.channels.to_le_bytes());
        out.extend_from_slice(&self.rate.to_le_bytes());
        out.extend_from_slice(&(self.rate * u32::from(block)).to_le_bytes());
        out.extend_from_slice(&block.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data.to_le_bytes());
        out.resize(44 + lead * 2, 0);
        for sample in &self.samples {
            let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
            out.extend_from_slice(&value.to_le_bytes());
        }
        out
    }
}

/// Seconds from `earlier` to `later`, negative when `later` is earlier.
fn seconds_between(earlier: SystemTime, later: SystemTime) -> f64 {
    match later.duration_since(earlier) {
        Ok(d) => d.as_secs_f64(),
        Err(e) => -e.duration().as_secs_f64(),
    }
}

/// Put `streams` on the recording's clock and mix them into one track.
/// `spans` are the shown stretches of the timeline; sound outside them is
/// cut, as the frames are. `zero` is the timeline's time that is the
/// video's start (its first frame). The track runs at the highest rate and
/// in the most channels of the streams (a stream with fewer channels
/// repeats its last one), from the first moment one of them plays.
/// `None` when no stream plays while the recording is shown.
pub fn compose(streams: &[Stream], spans: &[Span], zero: f64) -> Option<Audio> {
    let rate = streams.iter().map(|s| s.rate).max()?;
    let channels = streams.iter().map(|s| s.channels).max()?;
    let step = 1.0 / f64::from(rate);
    // Where each stream starts and ends on the video's clock, within each
    // span it plays in.
    let mut placed: Vec<(&Stream, &Span, f64, f64)> = Vec::new();
    for stream in streams {
        let length = stream.frames() as f64 / f64::from(stream.rate);
        for span in spans {
            // The stream's time 0 is `lead` seconds into the span.
            let lead = seconds_between(span.wall, stream.start);
            let from = lead.max(0.0);
            let to = (lead + length).min(span.length);
            if to > from {
                placed.push((stream, span, span.at + from - zero, span.at + to - zero));
            }
        }
    }
    let start = placed
        .iter()
        .map(|p| p.2)
        .fold(f64::INFINITY, f64::min)
        .max(0.0);
    let end = placed.iter().map(|p| p.3).fold(f64::NEG_INFINITY, f64::max);
    if !(end > start) {
        return None;
    }
    let frames = ((end - start) * f64::from(rate)).ceil() as usize;
    let width = usize::from(channels);
    let mut samples = vec![0.0f32; frames * width];
    for (stream, span, from, to) in placed {
        let lead = seconds_between(span.wall, stream.start);
        let first = ((from - start) * f64::from(rate)).ceil().max(0.0) as usize;
        let last = (((to - start) * f64::from(rate)).ceil() as usize).min(frames);
        for j in first..last {
            let video = start + j as f64 * step;
            // Back to the stream's own time.
            let own = video + zero - span.at - lead;
            let position = own * f64::from(stream.rate);
            for c in 0..width {
                samples[j * width + c] += stream.at(position, c);
            }
        }
    }
    samples.iter_mut().for_each(|s| *s = s.clamp(-1.0, 1.0));
    Some(Audio {
        rate,
        channels,
        offset: start,
        samples,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: f64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000) + Duration::from_secs_f64(seconds)
    }

    fn ramp(start: f64, rate: u32, frames: usize) -> Stream {
        Stream {
            rate,
            channels: 1,
            start: at(start),
            samples: (0..frames).map(|i| i as f32 / 10_000.0).collect(),
        }
    }

    #[test]
    fn a_stream_round_trips_and_a_cut_frame_is_dropped() {
        let stream = Stream {
            rate: 8000,
            channels: 2,
            start: at(1.5),
            samples: vec![0.25, -0.5, 1.0, 0.0],
        };
        let mut bytes = stream.to_bytes();
        assert_eq!(Stream::parse(&bytes), Some(stream.clone()));
        bytes.extend_from_slice(&0.5f32.to_le_bytes());
        assert_eq!(Stream::parse(&bytes).unwrap().samples.len(), 4);
        assert!(Stream::parse(b"RRAUDIO0").is_none());
        assert!(Stream::parse(&bytes[..20]).is_none());
    }

    #[test]
    fn a_stream_lands_where_it_played_on_the_shown_timeline() {
        // Shown from wall 10 s (timeline 0) on; the video starts at 0.2.
        let spans = [Span {
            wall: at(10.0),
            at: 0.0,
            length: 5.0,
        }];
        let audio = compose(&[ramp(11.0, 1000, 500)], &spans, 0.2).unwrap();
        assert!((audio.offset - 0.8).abs() < 1e-9, "{}", audio.offset);
        assert_eq!(audio.samples.len(), 500);
        assert!((audio.samples[100] - 0.01).abs() < 1e-5);
        assert!((audio.duration() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn hidden_sound_is_cut_as_the_frames_are() {
        // Shown 0..1 s (wall 10..11), hidden 2 s, shown again from wall 13
        // at timeline 1.
        let spans = [
            Span {
                wall: at(10.0),
                at: 0.0,
                length: 1.0,
            },
            Span {
                wall: at(13.0),
                at: 1.0,
                length: 4.0,
            },
        ];
        // From wall 10.5 to 13.5: half a second shown, 2 s hidden, half a
        // second shown.
        let audio = compose(&[ramp(10.5, 1000, 3000)], &spans, 0.0).unwrap();
        assert!((audio.offset - 0.5).abs() < 1e-9);
        assert_eq!(audio.samples.len(), 1000);
        assert!((audio.samples[499] - 0.0499).abs() < 1e-5);
        // Right after the cut: 2.5 s into the stream.
        assert!((audio.samples[500] - 0.25).abs() < 1e-5);
        // A stream entirely while hidden is no sound at all.
        assert!(compose(&[ramp(11.2, 1000, 1000)], &spans, 0.0).is_none());
        assert!(compose(&[], &spans, 0.0).is_none());
    }

    #[test]
    fn streams_are_mixed_at_the_highest_rate_and_most_channels() {
        let spans = [Span {
            wall: at(0.0),
            at: 0.0,
            length: 10.0,
        }];
        let mono = Stream {
            rate: 1000,
            channels: 1,
            start: at(1.0),
            samples: vec![0.25; 1000],
        };
        let stereo = Stream {
            rate: 2000,
            channels: 2,
            start: at(1.0),
            samples: [0.5, -0.5].repeat(2000),
        };
        let audio = compose(&[mono, stereo], &spans, 0.0).unwrap();
        assert_eq!((audio.rate, audio.channels), (2000, 2));
        assert!((audio.samples[200] - 0.75).abs() < 1e-6);
        assert!((audio.samples[201] + 0.25).abs() < 1e-6);
    }

    #[test]
    fn the_wav_is_16_bit_pcm_from_zero_or_from_its_offset() {
        let audio = Audio {
            rate: 1000,
            channels: 1,
            offset: 0.5,
            samples: vec![1.0, -1.0, 0.0],
        };
        let plain = audio.wav(false);
        assert_eq!(&plain[..4], b"RIFF");
        assert_eq!(&plain[8..16], b"WAVEfmt ");
        assert_eq!(plain.len(), 44 + 6);
        assert_eq!(&plain[44..], &[0xff, 0x7f, 0x01, 0x80, 0, 0]);
        let padded = audio.wav(true);
        assert_eq!(padded.len(), 44 + 1000 + 6);
        assert_eq!(u32::from_le_bytes(padded[40..44].try_into().unwrap()), 1006);
        assert!(padded[44..1044].iter().all(|b| *b == 0));
    }
}

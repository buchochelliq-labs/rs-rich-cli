//! Raw PCM: interleaved samples in a byte stream, read a buffer at a time.

use std::io::Read;
use std::time::Duration;

use super::Matrix;

/// How a sample is stored in raw PCM, by the names `parec --format`,
/// `arecord -f` and `ffmpeg -f` use (see [`Format::parse`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Format {
    /// Unsigned 8-bit, 128 the middle.
    U8,
    /// Signed 8-bit.
    S8,
    /// Signed 16-bit, little-endian (the default, scope-tui's and
    /// `parec`'s).
    #[default]
    S16Le,
    /// Signed 16-bit, big-endian.
    S16Be,
    /// Signed 24-bit in three bytes, little-endian.
    S24Le,
    /// Signed 32-bit, little-endian.
    S32Le,
    /// Signed 32-bit, big-endian.
    S32Be,
    /// 32-bit float, little-endian.
    F32Le,
    /// 32-bit float, big-endian.
    F32Be,
    /// 64-bit float, little-endian.
    F64Le,
}

impl Format {
    /// Bytes a sample takes.
    pub fn bytes(self) -> usize {
        match self {
            Format::U8 | Format::S8 => 1,
            Format::S16Le | Format::S16Be => 2,
            Format::S24Le => 3,
            Format::S32Le | Format::S32Be | Format::F32Le | Format::F32Be => 4,
            Format::F64Le => 8,
        }
    }

    /// The format a name means, in any case: `u8`, `s8`, `s16le` (or
    /// `s16`), `s16be`, `s24le` (or `s24_3le`), `s32le` (or `s32`),
    /// `s32be`, `f32le` (or `f32`, `float32le`), `f32be`, `f64le` (or
    /// `f64`). `None` for any other.
    ///
    /// ```
    /// use intuituive::audio::Format;
    ///
    /// assert_eq!(Format::parse("S16_LE"), Some(Format::S16Le));
    /// assert_eq!(Format::parse("float32le"), Some(Format::F32Le));
    /// assert_eq!(Format::parse("mp3"), None);
    /// ```
    pub fn parse(name: &str) -> Option<Format> {
        let name = name.to_ascii_lowercase().replace(['_', '-'], "");
        Some(match name.as_str() {
            "u8" => Format::U8,
            "s8" => Format::S8,
            "s16le" | "s16" => Format::S16Le,
            "s16be" => Format::S16Be,
            "s24le" | "s243le" => Format::S24Le,
            "s32le" | "s32" => Format::S32Le,
            "s32be" => Format::S32Be,
            "f32le" | "f32" | "float32le" | "flt" => Format::F32Le,
            "f32be" | "float32be" => Format::F32Be,
            "f64le" | "f64" | "float64le" | "dbl" => Format::F64Le,
            _ => return None,
        })
    }

    /// One sample from its bytes (exactly [`bytes`](Self::bytes) of them),
    /// scaled to -1 to 1. Floats are taken as they are.
    pub fn decode(self, b: &[u8]) -> f64 {
        let int = |v: i64, bits: u32| v as f64 / (1i64 << (bits - 1)) as f64;
        match self {
            Format::U8 => int(i64::from(b[0]) - 128, 8),
            Format::S8 => int(i64::from(b[0] as i8), 8),
            Format::S16Le => int(i64::from(i16::from_le_bytes([b[0], b[1]])), 16),
            Format::S16Be => int(i64::from(i16::from_be_bytes([b[0], b[1]])), 16),
            Format::S24Le => int(
                i64::from(i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8),
                24,
            ),
            Format::S32Le => int(i64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])), 32),
            Format::S32Be => int(i64::from(i32::from_be_bytes([b[0], b[1], b[2], b[3]])), 32),
            Format::F32Le => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Format::F32Be => f64::from(f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            Format::F64Le => f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]),
        }
    }
}

/// Interleaved PCM bytes (`L R L R …`) split into `channels` rows, each
/// sample scaled to -1 to 1. Bytes past the last whole sample are left
/// out; a last frame missing samples leaves its rows a sample short.
///
/// ```
/// use intuituive::audio::{deinterleave, Format};
///
/// let bytes = [0x00, 0x40, 0x00, 0xc0, 0xff, 0x7f, 0x00, 0x80];
/// assert_eq!(
///     deinterleave(&bytes, Format::S16Le, 2),
///     vec![vec![0.5, 32767.0 / 32768.0], vec![-0.5, -1.0]]
/// );
/// ```
pub fn deinterleave(bytes: &[u8], format: Format, channels: usize) -> Matrix {
    let channels = channels.max(1);
    let size = format.bytes();
    let mut out = vec![Vec::with_capacity(bytes.len() / size / channels); channels];
    for (index, sample) in bytes.chunks_exact(size).enumerate() {
        out[index % channels].push(format.decode(sample));
    }
    out
}

/// Read interleaved PCM from `reader` (a file, a pipe, stdin) as buffers of
/// `buffer` frames, `channels` rows each: an iterator that reads a buffer
/// each time it is asked for one. A short last buffer is padded with
/// silence; then it ends, as it does on a read error, which [`Pcm::error`]
/// then holds (a [`Capture::from_pcm`](super::Capture::from_pcm) fails
/// with it). Run it on a [`latest`](crate::latest) thread to feed an app.
///
/// ```
/// use intuituive::audio::{pcm, Format};
///
/// // Three stereo frames of u8, read two frames at a time.
/// let bytes: &[u8] = &[128, 192, 64, 128, 255, 0];
/// let buffers: Vec<_> = pcm(bytes, Format::U8, 2, 2).collect();
/// assert_eq!(buffers.len(), 2);
/// assert_eq!(buffers[0], vec![vec![0.0, -0.5], vec![0.5, 0.0]]);
/// // The last is padded with silence.
/// assert_eq!(buffers[1], vec![vec![127.0 / 128.0, 0.0], vec![-1.0, 0.0]]);
/// ```
pub fn pcm<R: Read>(reader: R, format: Format, channels: usize, buffer: usize) -> Pcm<R> {
    let channels = channels.max(1);
    Pcm {
        reader,
        format,
        channels,
        bytes: vec![0; buffer.max(1) * channels * format.bytes()],
        error: None,
        failed: false,
    }
}

/// Buffers of raw PCM from a reader: see [`pcm`].
#[derive(Debug)]
pub struct Pcm<R> {
    reader: R,
    format: Format,
    channels: usize,
    bytes: Vec<u8>,
    /// The read error that ended it, if one did, until taken.
    error: Option<std::io::Error>,
    /// A read failed: no more buffers.
    failed: bool,
}

impl<R> Pcm<R> {
    /// The rows each buffer has.
    pub fn channels(&self) -> usize {
        self.channels
    }

    /// The samples each row of a buffer has.
    pub fn buffer(&self) -> usize {
        self.bytes.len() / self.channels / self.format.bytes()
    }

    /// The read error that ended the buffers, if one did (rather than the
    /// end of the stream): scope-tui's "data source returned null".
    ///
    /// ```
    /// use intuituive::audio::{pcm, Format};
    /// use std::io::{self, Read};
    ///
    /// struct Broken;
    /// impl Read for Broken {
    ///     fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
    ///         Err(io::Error::other("the pipe broke"))
    ///     }
    /// }
    /// let mut buffers = pcm(Broken, Format::S16Le, 1, 4);
    /// assert_eq!(buffers.next(), None);
    /// assert_eq!(buffers.error().map(|e| e.to_string()), Some("the pipe broke".into()));
    /// ```
    pub fn error(&self) -> Option<&std::io::Error> {
        self.error.as_ref()
    }

    /// The next buffer, `Ok(None)` at the end of the stream, or the read
    /// error that ended it.
    ///
    /// # Errors
    /// The reader failed (any samples read before it came as a buffer,
    /// padded).
    pub fn try_next(&mut self) -> std::io::Result<Option<Matrix>>
    where
        R: Read,
    {
        match self.next() {
            Some(buffer) => Ok(Some(buffer)),
            None => match self.error.take() {
                Some(error) => Err(error),
                None => Ok(None),
            },
        }
    }
}

/// Play `pcm`'s buffers onto a signal at `rate` samples a second, a
/// buffer's worth of time apart (the first at once), and then silence on
/// the same beat, as a file played to a scope (scope-tui's `file` source
/// pads with silence): the core's [`latest_iter`](crate::latest_iter)
/// with [`Pace::Every`](crate::Pace::Every). For a source with a state
/// (ended, failed on a read error) in the shape of a sound card's, see
/// [`Capture::from_pcm`](super::Capture::from_pcm). In an app that
/// [waits for its tasks](crate::App::wait_for_tasks) it keeps to the
/// app's clock. The reader is read on the signal's thread.
///
/// ```
/// use intuituive::audio::{pcm, play, Format};
/// use intuituive::prelude::*;
/// use std::time::Duration;
///
/// // Two mono u8 buffers of two samples at 100 Hz: 20 ms each.
/// let bytes = std::io::Cursor::new(vec![255u8, 255, 0, 0]);
/// let app = App::new(move || {
///     let samples = play(pcm(bytes, Format::U8, 1, 2), 100);
///     text(move || format!("{:?}", samples.with(|m| m.as_ref().map(|m| m[0][0]))))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(20, 1);
/// driver.advance(Duration::from_millis(10));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(0.9921875)");
/// driver.advance(Duration::from_millis(20));
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(-1.0)");
/// driver.advance(Duration::from_millis(20));
/// // Then silence, buffers of the same shape.
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Some(0.0)");
/// ```
///
/// # Panics
/// Outside a running or building app.
#[track_caller]
pub fn play<R: Read + Send + 'static>(pcm: Pcm<R>, rate: u32) -> crate::Signal<Option<Matrix>> {
    let frames = pcm.buffer() as f64;
    let period = Duration::from_secs_f64(frames / f64::from(rate.max(1)));
    let silence = vec![vec![0.0; pcm.buffer()]; pcm.channels()];
    crate::latest_iter(pcm, crate::Pace::Every(period)).then(std::iter::repeat(silence))
}

impl<R: Read> Iterator for Pcm<R> {
    type Item = Matrix;

    fn next(&mut self) -> Option<Matrix> {
        if self.failed {
            return None;
        }
        let mut filled = 0;
        while filled < self.bytes.len() {
            match self.reader.read(&mut self.bytes[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) => {
                    self.error = Some(error);
                    self.failed = true;
                    break;
                }
            }
        }
        if filled == 0 {
            return None;
        }
        // Silence, in this format: zero, but 128 for unsigned bytes.
        let silence = if self.format == Format::U8 { 128 } else { 0 };
        self.bytes[filled..].fill(silence);
        Some(deinterleave(&self.bytes, self.format, self.channels))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_decodes_full_scale() {
        let cases: [(Format, Vec<u8>, f64); 10] = [
            (Format::U8, vec![0], -1.0),
            (Format::S8, vec![0x80], -1.0),
            (Format::S16Le, vec![0x00, 0x80], -1.0),
            (Format::S16Be, vec![0x80, 0x00], -1.0),
            (Format::S24Le, vec![0x00, 0x00, 0x80], -1.0),
            (Format::S24Le, vec![0x00, 0x00, 0x40], 0.5),
            (Format::S32Le, vec![0, 0, 0, 0x80], -1.0),
            (Format::S32Be, vec![0x40, 0, 0, 0], 0.5),
            (Format::F32Be, 0.25f32.to_be_bytes().to_vec(), 0.25),
            (Format::F64Le, (-0.75f64).to_le_bytes().to_vec(), -0.75),
        ];
        for (format, bytes, want) in cases {
            assert_eq!(bytes.len(), format.bytes(), "{format:?}");
            assert_eq!(format.decode(&bytes), want, "{format:?}");
        }
        assert_eq!(Format::F32Le.decode(&0.5f32.to_le_bytes()), 0.5);
    }

    #[test]
    fn names_parse_as_the_tools_write_them() {
        for (name, format) in [
            ("s16le", Format::S16Le),
            ("S16_LE", Format::S16Le),
            ("s24_3le", Format::S24Le),
            ("FLOAT32LE", Format::F32Le),
            ("f64", Format::F64Le),
            ("u8", Format::U8),
        ] {
            assert_eq!(Format::parse(name), Some(format), "{name}");
        }
        assert_eq!(Format::parse("s24"), None);
    }

    #[test]
    fn a_reader_ends_after_its_last_buffer() {
        // Five mono s16 samples, two a buffer: three buffers, the last
        // padded.
        let bytes: Vec<u8> = (1..=5i16).flat_map(|s| (s * 1024).to_le_bytes()).collect();
        let buffers: Vec<Matrix> = pcm(bytes.as_slice(), Format::S16Le, 1, 2).collect();
        assert_eq!(buffers.len(), 3);
        assert_eq!(buffers[2], vec![vec![5.0 / 32.0, 0.0]]);
        assert_eq!(pcm(&[][..], Format::F32Le, 2, 4).next(), None);
    }

    #[test]
    fn a_partial_sample_is_dropped() {
        assert_eq!(
            deinterleave(&[0, 0x40, 0x12], Format::S16Le, 1),
            vec![vec![0.5]]
        );
    }
}

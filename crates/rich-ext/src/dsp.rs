//! Signal processing for live views of audio and other sampled data: a
//! fast Fourier transform, a Hann window and magnitude spectra; test
//! signals (tones, harmonic partials, noise and a mix of them); trigger
//! detection, as an oscilloscope's; a spectrum's peak and log-spaced
//! bands, with levels that fall under gravity ([`Falloff`]), as a bar
//! spectrum's (cava's); and the frequencies of musical notes, in equal
//! temperament or scope-tui's table ([`NoteTable`]), with the note nearest
//! a frequency, and a [`Tuner`] reading a spectrum's note and cents,
//! smoothed.
//!
//! Not a port of anything upstream: an rs-rich addition, written for
//! intuiTUIve's spectroscope (the `scope` example, after scope-tui), with
//! no dependencies. [`fft`] takes any length: radix 2 for a power of two,
//! and Bluestein's chirp transform (over a power-of-two FFT) otherwise, so
//! a buffer of 1000 or 4410 samples is transformed at its own length.
//!
//! ```
//! use rich_ext::dsp;
//!
//! // A 1 kHz tone, sampled at 8 kHz.
//! let rate = 8000.0;
//! let tone: Vec<f64> = (0..1024)
//!     .map(|i| (2.0 * std::f64::consts::PI * 1000.0 * i as f64 / rate).sin())
//!     .collect();
//! let spectrum = dsp::spectrum(&dsp::hann(&tone), rate);
//! let (peak, _) = spectrum
//!     .iter()
//!     .copied()
//!     .max_by(|a, b| a.1.total_cmp(&b.1))
//!     .unwrap();
//! assert!((peak - 1000.0).abs() < rate / 1024.0);
//! ```

use std::f64::consts::PI;

/// A complex number, as (real, imaginary).
pub type Complex = (f64, f64);

fn mul((a, b): Complex, (c, d): Complex) -> Complex {
    (a * c - b * d, a * d + b * c)
}

/// The magnitude of `c`: `√(re² + im²)`.
pub fn magnitude((re, im): Complex) -> f64 {
    re.hypot(im)
}

/// `samples` under a Hann window, `0.5 (1 − cos(2πi / n))`, which tapers
/// both ends to zero so a buffer that does not hold whole periods leaks
/// less into neighbouring bins. The periodic form, as scope-tui's.
pub fn hann(samples: &[f64]) -> Vec<f64> {
    let n = samples.len() as f64;
    samples
        .iter()
        .enumerate()
        .map(|(i, s)| s * 0.5 * (1.0 - (2.0 * PI * i as f64 / n).cos()))
        .collect()
}

/// An in-place radix-2 FFT; `data`'s length is a power of two.
fn radix2(data: &mut [Complex]) {
    let n = data.len();
    if n < 2 {
        return;
    }
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = i.reverse_bits() >> (usize::BITS - bits);
        if i < j {
            data.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let w = ((angle * k as f64).cos(), (angle * k as f64).sin());
                let (a, b) = (data[start + k], mul(data[start + k + len / 2], w));
                data[start + k] = (a.0 + b.0, a.1 + b.1);
                data[start + k + len / 2] = (a.0 - b.0, a.1 - b.1);
            }
        }
        len *= 2;
    }
}

/// The discrete Fourier transform of real `samples`, of their own length:
/// `X[k] = Σ x[j] e^(−2πijk/n)`, unnormalised. Radix 2 for a power of two,
/// Bluestein's chirp transform otherwise; an empty input gives an empty
/// output.
///
/// ```
/// use rich_ext::dsp::fft;
///
/// let bins = fft(&[1.0, 0.0, 0.0]);
/// assert_eq!(bins.len(), 3);
/// assert!(bins.iter().all(|(re, im)| (re - 1.0).abs() < 1e-12 && im.abs() < 1e-12));
/// ```
pub fn fft(samples: &[f64]) -> Vec<Complex> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    if n.is_power_of_two() {
        let mut data: Vec<Complex> = samples.iter().map(|s| (*s, 0.0)).collect();
        radix2(&mut data);
        return data;
    }
    // X[k] = w[k] Σ x[j] w[j] conj(w[k - j]), with w[j] = e^(-iπj²/n): a
    // convolution, worked with a power-of-two FFT.
    let m = (2 * n).saturating_sub(1).next_power_of_two();
    let chirp: Vec<Complex> = (0..n)
        .map(|j| {
            // j² mod 2n keeps the angle small, so precise.
            let angle = -PI * ((j * j) % (2 * n)) as f64 / n as f64;
            (angle.cos(), angle.sin())
        })
        .collect();
    let mut a = vec![(0.0, 0.0); m];
    for (j, s) in samples.iter().enumerate() {
        a[j] = (s * chirp[j].0, s * chirp[j].1);
    }
    let mut b = vec![(0.0, 0.0); m];
    for (j, w) in chirp.iter().enumerate() {
        b[j] = (w.0, -w.1);
        if j > 0 {
            b[m - j] = (w.0, -w.1);
        }
    }
    radix2(&mut a);
    radix2(&mut b);
    // The inverse FFT: the conjugate of the forward FFT of the conjugate.
    let mut c: Vec<Complex> = a
        .iter()
        .zip(&b)
        .map(|(x, y)| {
            let p = mul(*x, *y);
            (p.0, -p.1)
        })
        .collect();
    radix2(&mut c);
    let scale = m as f64;
    (0..n)
        .map(|k| mul((c[k].0 / scale, -c[k].1 / scale), chirp[k]))
        .collect()
}

/// The magnitude of every bin of `samples`' [`fft`], as `(frequency in Hz,
/// magnitude)`: all `n` of them, `rate / n` Hz apart, the mirrored half
/// above the Nyquist frequency too (up to `rate − rate / n`), as
/// scope-tui plots them. [`spectrum`] stops at the Nyquist frequency.
///
/// ```
/// use rich_ext::dsp::spectrum_full;
///
/// let bins = spectrum_full(&[0.0; 100], 1000.0);
/// assert_eq!(bins.len(), 100);
/// assert_eq!(bins[99].0, 990.0);
/// ```
pub fn spectrum_full(samples: &[f64], rate: f64) -> Vec<(f64, f64)> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let resolution = rate / n as f64;
    fft(samples)
        .into_iter()
        .enumerate()
        .map(|(i, bin)| (i as f64 * resolution, magnitude(bin)))
        .collect()
}

/// The magnitude spectrum of `samples` taken at `rate` samples a second:
/// `(frequency in Hz, magnitude)` for each bin from 0 Hz up to the Nyquist
/// frequency (`rate / 2`), `n / 2 + 1` of them, `rate / n` Hz apart. The
/// magnitudes are the [`fft`]'s, unnormalised: a full-scale sine of `n`
/// samples peaks near `n / 2` (`n / 4` under a [`hann`] window). Window the
/// samples first if they do not hold whole periods.
///
/// ```
/// use rich_ext::dsp::spectrum;
///
/// let bins = spectrum(&[0.0; 100], 1000.0);
/// assert_eq!(bins.len(), 51);
/// assert_eq!(bins[1].0, 10.0);
/// assert_eq!(bins[50].0, 500.0);
/// ```
pub fn spectrum(samples: &[f64], rate: f64) -> Vec<(f64, f64)> {
    let n = samples.len();
    if n == 0 {
        return Vec::new();
    }
    let resolution = rate / n as f64;
    fft(samples)
        .into_iter()
        .take(n / 2 + 1)
        .enumerate()
        .map(|(i, bin)| (i as f64 * resolution, magnitude(bin)))
        .collect()
}

/// A sine wave of `hz` at `rate` samples a second, from -1 to 1 (or
/// ±[`amplitude`](Tone::amplitude)), as an endless iterator of samples. Its
/// phase carries on from one buffer to the next, so `tone.by_ref().take(n)`
/// in a loop is one continuous wave.
///
/// ```
/// use rich_ext::dsp::tone;
///
/// // 1 kHz at 4 kHz: a quarter period a sample.
/// let wave: Vec<f64> = tone(1000.0, 4000.0).take(5).collect();
/// let rounded: Vec<f64> = wave.iter().map(|s| s.round()).collect();
/// assert_eq!(rounded, [0.0, 1.0, 0.0, -1.0, 0.0]);
/// ```
pub fn tone(hz: f64, rate: f64) -> Tone {
    Tone {
        step: if rate > 0.0 { hz / rate } else { 0.0 },
        phase: 0.0,
        amplitude: 1.0,
    }
}

/// A sine wave: see [`tone`].
#[derive(Clone, Debug, PartialEq)]
pub struct Tone {
    /// Cycles a sample.
    step: f64,
    /// Where in its cycle the next sample is, from 0 to 1.
    phase: f64,
    amplitude: f64,
}

impl Tone {
    /// Its peak (default 1).
    pub fn amplitude(mut self, amplitude: f64) -> Self {
        self.amplitude = amplitude;
        self
    }

    /// Where in its cycle it starts, in radians (default 0).
    pub fn phase(mut self, radians: f64) -> Self {
        self.phase = (radians / (2.0 * PI)).rem_euclid(1.0);
        self
    }
}

impl Iterator for Tone {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        let sample = self.amplitude * (2.0 * PI * self.phase).sin();
        self.phase = (self.phase + self.step).rem_euclid(1.0);
        Some(sample)
    }
}

/// A note with overtones: a [`tone`] at `hz` and one at each whole multiple
/// of it, the `k`th (from 1, the note itself) at `amplitudes[k - 1]`.
/// Partials at or above the Nyquist frequency (`rate / 2`) are left out, so
/// nothing folds back as a lower pitch.
///
/// ```
/// use rich_ext::dsp::{partials, spectrum, tone};
///
/// // A note and its octave, half as loud.
/// let samples: Vec<f64> = partials(100.0, 1000.0, &[1.0, 0.5]).take(1000).collect();
/// let bins = spectrum(&samples, 1000.0);
/// assert!((bins[100].1 - 500.0).abs() < 1e-6);
/// assert!((bins[200].1 - 250.0).abs() < 1e-6);
/// ```
pub fn partials(hz: f64, rate: f64, amplitudes: &[f64]) -> Mix {
    let mut mix = Mix::new();
    for (k, amplitude) in amplitudes.iter().enumerate() {
        let partial = hz * (k + 1) as f64;
        if partial < rate / 2.0 {
            mix = mix.add(*amplitude, tone(partial, rate));
        }
    }
    mix
}

/// White noise from -1 to 1 (xorshift), the same for the same `seed`, as
/// an endless iterator. [`Noise::low_pass`] smooths it into a wander that
/// falls away with frequency, as music's spectrum does.
///
/// ```
/// use rich_ext::dsp::noise;
///
/// let a: Vec<f64> = noise(7).take(64).collect();
/// assert_eq!(a, noise(7).take(64).collect::<Vec<_>>());
/// assert!(a.iter().all(|s| (-1.0..1.0).contains(s)));
/// ```
pub fn noise(seed: u64) -> Noise {
    Noise {
        // xorshift never leaves 0, so the seed is mixed and made odd.
        state: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
        smoothing: None,
        low: 0.0,
    }
}

/// White or low-passed noise: see [`noise`].
#[derive(Clone, Debug, PartialEq)]
pub struct Noise {
    state: u64,
    /// The one-pole low-pass's coefficient, when filtered.
    smoothing: Option<f64>,
    /// The filter's last output.
    low: f64,
}

impl Noise {
    /// Through a one-pole low-pass at `hz` (taken at `rate` samples a
    /// second): fast changes are smoothed away, so the noise wanders.
    /// Filtering takes energy out: the result is quieter than -1 to 1.
    pub fn low_pass(mut self, hz: f64, rate: f64) -> Self {
        let coefficient = if rate > 0.0 {
            1.0 - (-2.0 * PI * hz / rate).exp()
        } else {
            1.0
        };
        self.smoothing = Some(coefficient.clamp(0.0, 1.0));
        self
    }

    fn white(&mut self) -> f64 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 7;
        self.state ^= self.state << 17;
        (self.state >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

impl Iterator for Noise {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        let white = self.white();
        Some(match self.smoothing {
            Some(a) => {
                self.low += a * (white - self.low);
                self.low
            }
            None => white,
        })
    }
}

/// Several sources added together, each scaled by its gain: tones, noise,
/// [`partials`], another mix, or any iterator of samples. It ends when
/// every source has ended (never, with a tone in it); a source that ends
/// early adds nothing after.
///
/// ```
/// use rich_ext::dsp::{mix, noise, tone};
///
/// let rate = 48_000.0;
/// let signal = mix()
///     .add(0.6, tone(440.0, rate))
///     .add(0.2, tone(880.0, rate))
///     .add(0.05, noise(1).low_pass(300.0, rate));
/// let buffer: Vec<f64> = signal.take(1024).collect();
/// assert!(buffer.iter().all(|s| s.abs() <= 0.85));
/// ```
pub fn mix() -> Mix {
    Mix::new()
}

/// A sum of sources: see [`mix`].
#[derive(Default)]
pub struct Mix {
    parts: Vec<(f64, Box<dyn Iterator<Item = f64> + Send>)>,
}

impl std::fmt::Debug for Mix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let gains: Vec<f64> = self.parts.iter().map(|(gain, _)| *gain).collect();
        f.debug_struct("Mix").field("gains", &gains).finish()
    }
}

impl Mix {
    /// An empty mix, which ends at once.
    pub fn new() -> Self {
        Mix::default()
    }

    /// Add `source`, scaled by `gain`.
    pub fn add(mut self, gain: f64, source: impl Iterator<Item = f64> + Send + 'static) -> Self {
        self.parts.push((gain, Box::new(source)));
        self
    }
}

impl Iterator for Mix {
    type Item = f64;

    fn next(&mut self) -> Option<f64> {
        let mut sum = 0.0;
        let mut any = false;
        for (gain, source) in &mut self.parts {
            if let Some(sample) = source.next() {
                sum += *gain * sample;
                any = true;
            }
        }
        any.then_some(sum)
    }
}

/// Which way a signal crosses a [`trigger`] level.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Edge {
    /// From at or below the level to above it.
    #[default]
    Rising,
    /// From at or above the level to below it.
    Falling,
}

/// Where `samples` first cross `level` on `edge` and stay past it for
/// `depth` samples (at least 1): the index of the last sample before the
/// crossing, as an oscilloscope's trigger finds where to start its trace
/// so a periodic wave stands still. `None` when it never does. A greater
/// depth skips noise that crosses and comes straight back.
///
/// ```
/// use rich_ext::dsp::{trigger, Edge};
///
/// let wave = [0.5, -0.2, 0.1, 0.3, 0.4, -0.1];
/// assert_eq!(trigger(&wave, 0.0, 1, Edge::Rising), Some(1));
/// assert_eq!(trigger(&wave, 0.0, 1, Edge::Falling), Some(0));
/// // Past the level for three samples: 0.1, 0.3 and 0.4.
/// assert_eq!(trigger(&wave, 0.0, 3, Edge::Rising), Some(1));
/// assert_eq!(trigger(&wave, 0.0, 4, Edge::Rising), None);
/// ```
pub fn trigger(samples: &[f64], level: f64, depth: usize, edge: Edge) -> Option<usize> {
    let depth = depth.max(1);
    let last = samples.len().checked_sub(depth + 1)?;
    (0..=last).find(|&i| {
        let after = &samples[i + 1..=i + depth];
        match edge {
            Edge::Rising => samples[i] <= level && after.iter().all(|s| *s > level),
            Edge::Falling => samples[i] >= level && after.iter().all(|s| *s < level),
        }
    })
}

/// The semitones of the natural notes above C.
const NATURALS: [(char, i32); 7] = [
    ('C', 0),
    ('D', 2),
    ('E', 4),
    ('F', 5),
    ('G', 7),
    ('A', 9),
    ('B', 11),
];

/// The names [`nearest_note`] gives, from C, with sharps.
const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// The frequency in Hz of a note in scientific pitch notation, in equal
/// temperament with A4 at 440 Hz: a letter (either case), any number of
/// sharps (`#`, `♯`) or flats (`b`, `♭`), and an octave (`-1` to `9`;
/// 4, the octave of middle C, when left out). `None` when it is not a
/// note. [`NoteTable::ScopeTui`] reads notes as scope-tui does instead.
///
/// ```
/// use rich_ext::dsp::note_hz;
///
/// assert_eq!(note_hz("A4"), Some(440.0));
/// assert_eq!(note_hz("a"), Some(440.0));
/// assert_eq!(note_hz("A5"), Some(880.0));
/// assert!((note_hz("C4").unwrap() - 261.626).abs() < 0.001);
/// assert_eq!(note_hz("C#3"), note_hz("Db3"));
/// assert!((note_hz("C0").unwrap() - 16.352).abs() < 0.001);
/// assert_eq!(note_hz("H2"), None);
/// ```
pub fn note_hz(note: &str) -> Option<f64> {
    NoteTable::Equal.hz(note)
}

/// Which pitches [`NoteTable::hz`] gives a note.
///
/// ```
/// use rich_ext::dsp::NoteTable;
///
/// // E2: 82.41 Hz in equal temperament; scope-tui's octave-0 E, 20.60 Hz,
/// // doubled twice: 82.4 Hz (as an f32).
/// assert!((NoteTable::Equal.hz("E2").unwrap() - 82.407).abs() < 0.001);
/// assert_eq!(NoteTable::ScopeTui.hz("E2"), Some(f64::from(82.4f32)));
/// // Without an octave: 4 in equal temperament, 0 in scope-tui's table.
/// assert_eq!(NoteTable::Equal.hz("A"), Some(440.0));
/// assert_eq!(NoteTable::ScopeTui.hz("A"), Some(27.5));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NoteTable {
    /// Equal temperament, A4 at 440 Hz, octave 4 by default: exact
    /// ([`note_hz`]).
    #[default]
    Equal,
    /// scope-tui's table (`music.rs`, 0.3.5): the twelve pitches of octave
    /// 0 rounded to the hundredth of a hertz (C0 16.35, …, B0 30.87),
    /// doubled once an octave in `f32`, octave 0 by default and no octave
    /// below it. A clone that divides a sample rate by these pitches picks
    /// the buffer sizes scope-tui's `--tune` does, sample for sample
    /// (about one note in ten is a sample off equal temperament's).
    ScopeTui,
}

/// scope-tui's octave-0 pitches, from C, as it keeps them.
const SCOPE_TUI_OCTAVE_0: [f32; 12] = [
    16.35, 17.32, 18.35, 19.45, 20.60, 21.83, 23.12, 24.50, 25.96, 27.50, 29.14, 30.87,
];

impl NoteTable {
    /// The octave a note without one is in: 4 ([`Equal`](Self::Equal)) or
    /// 0 ([`ScopeTui`](Self::ScopeTui)).
    pub fn default_octave(self) -> i32 {
        match self {
            NoteTable::Equal => 4,
            NoteTable::ScopeTui => 0,
        }
    }

    /// The frequency in Hz of `note`, written as [`note_hz`] reads it, in
    /// this table. `None` when it is not a note (or, in
    /// [`ScopeTui`](Self::ScopeTui), is below octave 0).
    pub fn hz(self, note: &str) -> Option<f64> {
        // Semitones from C0.
        let from_c0 = parse_note(note, self.default_octave())?;
        match self {
            NoteTable::Equal => Some(440.0 * 2f64.powf(f64::from(from_c0 - 57) / 12.0)),
            NoteTable::ScopeTui => {
                let octave = u32::try_from(from_c0.div_euclid(12)).ok()?;
                let mut hz = SCOPE_TUI_OCTAVE_0[from_c0.rem_euclid(12) as usize];
                for _ in 0..octave {
                    hz *= 2.0;
                }
                Some(f64::from(hz))
            }
        }
    }
}

/// `note`'s semitones from C0, its octave `default_octave` when it names
/// none.
fn parse_note(note: &str, default_octave: i32) -> Option<i32> {
    let note = note.trim();
    let mut chars = note.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    let (_, mut semitone) = *NATURALS.iter().find(|(name, _)| *name == letter)?;
    let rest = chars.as_str();
    let octave_at = rest
        .find(|c: char| c.is_ascii_digit() || c == '-')
        .unwrap_or(rest.len());
    for accidental in rest[..octave_at].chars() {
        semitone += match accidental {
            '#' | '♯' => 1,
            'b' | '♭' => -1,
            _ => return None,
        };
    }
    let octave: i32 = match &rest[octave_at..] {
        "" => default_octave,
        digits => digits.parse().ok().filter(|o| (-1..=9).contains(o))?,
    };
    Some(octave * 12 + semitone)
}

/// The note nearest `hz` in equal temperament (A4 at 440 Hz): its name,
/// with sharps (`"A"`, `"C#"`), its octave, and how far `hz` is from it in
/// cents (hundredths of a semitone, from -50 to 50): a tuner's reading.
/// `None` for a frequency that is not positive and finite, or that falls
/// outside octaves -1 to 9.
///
/// ```
/// use rich_ext::dsp::nearest_note;
///
/// assert_eq!(nearest_note(440.0), Some(("A", 4, 0.0)));
/// let (name, octave, cents) = nearest_note(445.0).unwrap();
/// assert_eq!((name, octave), ("A", 4));
/// assert!((cents - 19.56).abs() < 0.01);
/// assert_eq!(nearest_note(130.0).map(|(n, o, _)| (n, o)), Some(("C", 3)));
/// ```
pub fn nearest_note(hz: f64) -> Option<(&'static str, i32, f64)> {
    if !(hz.is_finite() && hz > 0.0) {
        return None;
    }
    let from_a4 = 12.0 * (hz / 440.0).log2();
    let nearest = from_a4.round();
    let cents = ((from_a4 - nearest) * 100.0 * 1e9).round() / 1e9;
    // Semitones from C-1.
    let from_c = nearest as i64 + 9 + 5 * 12;
    if !(0..11 * 12).contains(&from_c) {
        return None;
    }
    let name = NAMES[(from_c % 12) as usize];
    Some((name, (from_c / 12 - 1) as i32, cents))
}

/// The frequency of the loudest bin of `spectrum` (`(Hz, magnitude)`
/// pairs, lowest first, as [`spectrum`] gives them) above 0 Hz, refined
/// between bins by the parabola through its and its two neighbours'
/// logarithms (a [`hann`]-windowed peak's shape), so a
/// tuner reads finer than the bins are apart. `None` when there is no
/// bin above 0 Hz louder than zero (silence).
///
/// ```
/// use rich_ext::dsp::{hann, nearest_note, peak_hz, spectrum, tone};
///
/// // A4, 2048 samples at 48 kHz: bins 23.4 Hz apart.
/// let samples: Vec<f64> = tone(440.0, 48_000.0).take(2048).collect();
/// let hz = peak_hz(&spectrum(&hann(&samples), 48_000.0)).unwrap();
/// assert!((hz - 440.0).abs() < 1.0, "{hz}");
/// let (name, octave, cents) = nearest_note(hz).unwrap();
/// assert_eq!((name, octave), ("A", 4));
/// assert!(cents.abs() < 5.0);
/// assert_eq!(peak_hz(&spectrum(&[0.0; 64], 48_000.0)), None);
/// ```
pub fn peak_hz(spectrum: &[(f64, f64)]) -> Option<f64> {
    let (at, &(hz, level)) = spectrum
        .iter()
        .enumerate()
        .filter(|(_, (hz, level))| *hz > 0.0 && level.is_finite())
        .max_by(|a, b| a.1 .1.total_cmp(&b.1 .1))?;
    if level <= 0.0 {
        return None;
    }
    let (Some(&(before_hz, a)), Some(&(after_hz, c))) = (
        at.checked_sub(1).and_then(|i| spectrum.get(i)),
        spectrum.get(at + 1),
    ) else {
        return Some(hz);
    };
    if before_hz <= 0.0 || a <= 0.0 || c <= 0.0 {
        return Some(hz);
    }
    // On the logarithms of the levels, which a windowed peak's top fits
    // closely.
    let (a, b, c) = (a.ln(), level.ln(), c.ln());
    let curve = a - 2.0 * b + c;
    if !curve.is_finite() || curve >= 0.0 {
        return Some(hz);
    }
    // The vertex, from -½ to ½ a bin from `hz`.
    let shift = (0.5 * (a - c) / curve).clamp(-0.5, 0.5);
    let step = if shift < 0.0 {
        hz - before_hz
    } else {
        after_hz - hz
    };
    Some(hz + shift * step)
}

/// `spectrum` (`(Hz, magnitude)` pairs, lowest first) gathered into `n`
/// bands spaced by the logarithm of frequency from `lo_hz` to `hi_hz`, so
/// each octave gets as many bands as any other, as cava's bars and a
/// graphic equaliser's sliders: each band the loudest bin in it, or, for a
/// band narrower than the bins are apart (the low end), the level between
/// the bins either side of its middle. Empty for no bands; zeros when the
/// range is not positive and increasing, or the spectrum is empty.
///
/// ```
/// use rich_ext::dsp::bands;
///
/// // Bins 10 Hz apart; one loud at 100 Hz, one at 1 kHz.
/// let spectrum: Vec<(f64, f64)> = (0..=200)
///     .map(|i| (i as f64 * 10.0, if i == 10 || i == 100 { 1.0 } else { 0.0 }))
///     .collect();
/// // Three bands a decade each: 10-100 Hz, 100-1000, 1000-10000.
/// assert_eq!(bands(&spectrum, 3, 10.0, 10_000.0), [0.0, 1.0, 1.0]);
/// ```
pub fn bands(spectrum: &[(f64, f64)], n: usize, lo_hz: f64, hi_hz: f64) -> Vec<f64> {
    let mut out = vec![0.0; n];
    let valid = lo_hz.is_finite() && hi_hz.is_finite() && 0.0 < lo_hz && lo_hz < hi_hz;
    if n == 0 || spectrum.is_empty() || !valid {
        return out;
    }
    let edge = |k: usize| lo_hz * (hi_hz / lo_hz).powf(k as f64 / n as f64);
    for (k, band) in out.iter_mut().enumerate() {
        let (from, to) = (edge(k), edge(k + 1));
        let last = k + 1 == n;
        let inside = spectrum
            .iter()
            .filter(|(hz, _)| from <= *hz && (*hz < to || (last && *hz <= to)))
            .map(|(_, level)| *level)
            .filter(|level| !level.is_nan())
            .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.max(v))));
        *band = inside.unwrap_or_else(|| level_at(spectrum, (from * to).sqrt()));
    }
    out
}

/// `spectrum`'s level at `hz`, a straight line between the bins either
/// side; the end bin's beyond the ends.
fn level_at(spectrum: &[(f64, f64)], hz: f64) -> f64 {
    let after = spectrum.partition_point(|(f, _)| *f < hz);
    match (
        after.checked_sub(1).map(|i| spectrum[i]),
        spectrum.get(after),
    ) {
        (Some((f0, a)), Some(&(f1, b))) if f1 > f0 => a + (b - a) * (hz - f0) / (f1 - f0),
        (_, Some(&(_, b))) => b,
        (Some((_, a)), None) => a,
        (None, None) => 0.0,
    }
}

/// Levels that rise at once and fall under gravity, as cava's bars do:
/// a level that drops is not followed down at once, but falls faster and
/// faster, by `g` more each [`update`](Self::update), until it meets the
/// new level. Rises are followed at once. Keep one per view, and update it
/// once a frame with the new levels ([`bands`]).
///
/// ```
/// use rich_ext::dsp::Falloff;
///
/// let mut bars = Falloff::gravity(0.1);
/// assert_eq!(bars.update(&[1.0]), [1.0]);
/// // Silence: down by 0.1, then 0.2, then 0.3 more, then at the floor.
/// assert_eq!(bars.update(&[0.0]), [0.9]);
/// assert!((bars.update(&[0.0])[0] - 0.7).abs() < 1e-12);
/// assert!((bars.update(&[0.0])[0] - 0.4).abs() < 1e-12);
/// assert_eq!(bars.update(&[0.0]), [0.0]);
/// // A rise is shown at once.
/// assert_eq!(bars.update(&[0.8]), [0.8]);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Falloff {
    gravity: f64,
    levels: Vec<f64>,
    speeds: Vec<f64>,
}

impl Falloff {
    /// Levels falling `g` faster each update (in the levels' units per
    /// update, per update); 0 or less follows every drop at once.
    pub fn gravity(g: f64) -> Self {
        Falloff {
            gravity: if g.is_finite() { g.max(0.0) } else { 0.0 },
            levels: Vec::new(),
            speeds: Vec::new(),
        }
    }

    /// Take the newest levels and return those to show. A level the last
    /// update did not have starts where it is; a NaN level is taken as
    /// silence, so the shown level falls.
    pub fn update(&mut self, levels: &[f64]) -> &[f64] {
        self.levels.truncate(levels.len());
        self.speeds.truncate(levels.len());
        for (i, &level) in levels.iter().enumerate() {
            let level = if level.is_nan() {
                f64::NEG_INFINITY
            } else {
                level
            };
            if i == self.levels.len() {
                self.levels.push(level);
                self.speeds.push(0.0);
                continue;
            }
            if level >= self.levels[i] || self.gravity == 0.0 {
                self.levels[i] = level;
                self.speeds[i] = 0.0;
            } else {
                self.speeds[i] += self.gravity;
                self.levels[i] = (self.levels[i] - self.speeds[i]).max(level);
            }
        }
        &self.levels
    }

    /// The levels the last update returned.
    pub fn levels(&self) -> &[f64] {
        &self.levels
    }

    /// Forget the levels: the next update starts afresh.
    pub fn reset(&mut self) {
        self.levels.clear();
        self.speeds.clear();
    }
}

/// What a [`Tuner`] reads: the frequency heard, the note nearest it (in
/// equal temperament, A4 at 440 Hz) and how far off it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reading {
    /// The frequency, smoothed, in Hz.
    pub hz: f64,
    /// The nearest note's name (`"A"`, `"C#"`).
    pub note: &'static str,
    /// Its octave (A4 is octave 4).
    pub octave: i32,
    /// How far the frequency is from the note, in cents (-50 to 50): sharp
    /// above 0, flat below.
    pub cents: f64,
}

impl Reading {
    /// The note with its octave: `A4`, `C#3`.
    pub fn name(&self) -> String {
        format!("{}{}", self.note, self.octave)
    }

    /// Whether it is within `cents` of the note: in tune.
    pub fn in_tune(&self, cents: f64) -> bool {
        self.cents.abs() <= cents
    }
}

/// A tuner's reading of a spectrum, as a guitar tuner shows it: the
/// loudest peak ([`peak_hz`]), the nearest note to it ([`nearest_note`])
/// and the cents between, smoothed from one spectrum to the next so the
/// needle settles rather than jitters. A peak more than half a semitone
/// from the smoothed one is a new note, taken at once; quieter than the
/// [`floor`](Self::floor), the tuner hears nothing, and after
/// [`hold`](Self::hold) spectra of nothing it lets the reading go.
///
/// ```
/// use rich_ext::dsp::{hann, spectrum, tone, Tuner};
///
/// let mut tuner = Tuner::new();
/// // A string a little sharp of A4.
/// let samples: Vec<f64> = tone(442.0, 48_000.0).take(4096).collect();
/// let reading = tuner.listen(&samples, 48_000.0).unwrap();
/// assert_eq!(reading.name(), "A4");
/// assert!((reading.cents - 7.85).abs() < 1.0, "{reading:?}");
/// // Silence: the last reading is held for a while, then let go.
/// let quiet = spectrum(&hann(&[0.0; 4096]), 48_000.0);
/// assert!(tuner.update(&quiet).is_some());
/// for _ in 0..10 {
///     tuner.update(&quiet);
/// }
/// assert_eq!(tuner.reading(), None);
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Tuner {
    smoothing: f64,
    floor: f64,
    hold: usize,
    hz: Option<f64>,
    quiet: usize,
}

impl Default for Tuner {
    fn default() -> Self {
        Tuner::new()
    }
}

impl Tuner {
    /// A tuner smoothing by half, hearing any peak above 0.001, holding a
    /// reading through four quiet spectra.
    pub fn new() -> Self {
        Tuner {
            smoothing: 0.5,
            floor: 0.001,
            hold: 4,
            hz: None,
            quiet: 0,
        }
    }

    /// How much of the last reading each new one keeps, from 0 (none: each
    /// spectrum read alone) to just under 1 (barely moving); default a
    /// half.
    pub fn smoothing(mut self, keep: f64) -> Self {
        if keep.is_finite() {
            self.smoothing = keep.clamp(0.0, 0.99);
        }
        self
    }

    /// The quietest peak heard (a magnitude, as [`spectrum`] gives them);
    /// default 0.001.
    pub fn floor(mut self, level: f64) -> Self {
        if level.is_finite() {
            self.floor = level.max(0.0);
        }
        self
    }

    /// How many spectra with nothing heard keep the last reading (default
    /// 4), so a note that dips does not blink out.
    pub fn hold(mut self, spectra: usize) -> Self {
        self.hold = spectra;
        self
    }

    /// Read `spectrum` (`(Hz, magnitude)` pairs, lowest first, as
    /// [`spectrum`] gives them): the reading now, smoothed.
    pub fn update(&mut self, spectrum: &[(f64, f64)]) -> Option<Reading> {
        let loudest = spectrum
            .iter()
            .filter(|(hz, _)| *hz > 0.0)
            .map(|(_, level)| *level)
            .fold(0.0f64, f64::max);
        let heard = (loudest > self.floor)
            .then(|| peak_hz(spectrum))
            .flatten()
            .filter(|hz| nearest_note(*hz).is_some());
        match heard {
            Some(hz) => {
                self.quiet = 0;
                self.hz = Some(match self.hz {
                    // Within half a semitone: the same note, smoothed in
                    // pitch (on the log of the frequency).
                    Some(old) if (1200.0 * (hz / old).log2()).abs() < 50.0 => {
                        (self.smoothing * old.log2() + (1.0 - self.smoothing) * hz.log2()).exp2()
                    }
                    _ => hz,
                });
            }
            None => {
                self.quiet += 1;
                if self.quiet > self.hold {
                    self.hz = None;
                }
            }
        }
        self.reading()
    }

    /// Read `samples` taken at `rate` samples a second: windowed
    /// ([`hann`]) and transformed ([`spectrum`]), then
    /// [`update`](Self::update).
    pub fn listen(&mut self, samples: &[f64], rate: f64) -> Option<Reading> {
        self.update(&spectrum(&hann(samples), rate))
    }

    /// The reading the last update gave.
    pub fn reading(&self) -> Option<Reading> {
        let hz = self.hz?;
        let (note, octave, cents) = nearest_note(hz)?;
        Some(Reading {
            hz,
            note,
            octave,
            cents,
        })
    }

    /// Forget the reading: the next peak is taken as it is.
    pub fn reset(&mut self) {
        self.hz = None;
        self.quiet = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tuner_smooths_within_a_note_and_jumps_to_another() {
        let rate = 48_000.0;
        let read = |hz: f64| -> Vec<(f64, f64)> {
            let samples: Vec<f64> = tone(hz, rate).take(8192).collect();
            spectrum(&hann(&samples), rate)
        };
        let mut tuner = Tuner::new().smoothing(0.5);
        let first = tuner.update(&read(440.0)).unwrap();
        assert_eq!(first.name(), "A4");
        assert!(first.in_tune(1.0), "{first:?}");
        // Ten cents sharp: half way there after one more spectrum.
        let sharp = 440.0 * 2f64.powf(10.0 / 1200.0);
        let next = tuner.update(&read(sharp)).unwrap();
        assert!((next.cents - 5.0).abs() < 1.0, "{next:?}");
        // E2, a new note: taken at once.
        let low = tuner.update(&read(82.41)).unwrap();
        assert_eq!(low.name(), "E2");
        assert!(low.cents.abs() < 5.0, "{low:?}");
        tuner.reset();
        assert_eq!(tuner.reading(), None);
    }

    /// The DFT by its definition, to check the fast paths against.
    fn naive(samples: &[f64]) -> Vec<Complex> {
        let n = samples.len();
        (0..n)
            .map(|k| {
                samples.iter().enumerate().fold((0.0, 0.0), |acc, (j, s)| {
                    let angle = -2.0 * PI * (j * k % n) as f64 / n as f64;
                    (acc.0 + s * angle.cos(), acc.1 + s * angle.sin())
                })
            })
            .collect()
    }

    fn close(a: &[Complex], b: &[Complex]) -> bool {
        a.len() == b.len()
            && a.iter()
                .zip(b)
                .all(|(x, y)| (x.0 - y.0).abs() < 1e-7 && (x.1 - y.1).abs() < 1e-7)
    }

    fn random(n: usize) -> Vec<f64> {
        // A small LCG: deterministic, no dependency.
        let mut state = 0x2545_f491_u64;
        (0..n)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1);
                (state >> 33) as f64 / (1u64 << 31) as f64 - 1.0
            })
            .collect()
    }

    #[test]
    fn matches_the_definition_at_every_length() {
        for n in [1, 2, 3, 4, 5, 7, 8, 12, 16, 100, 128, 441, 1000] {
            let samples = random(n);
            assert!(close(&fft(&samples), &naive(&samples)), "length {n}");
        }
        assert!(fft(&[]).is_empty());
    }

    #[test]
    fn a_tone_peaks_at_its_frequency() {
        // 4410 samples at 44.1 kHz: 10 Hz bins, a length Bluestein handles.
        let rate = 44_100.0;
        for hz in [440.0, 1000.0, 12_340.0] {
            let tone: Vec<f64> = (0..4410)
                .map(|i| (2.0 * PI * hz * i as f64 / rate).sin())
                .collect();
            let bins = spectrum(&hann(&tone), rate);
            assert_eq!(bins.len(), 2206);
            let (peak, level) = bins
                .iter()
                .copied()
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .unwrap();
            assert!((peak - hz).abs() <= 10.0, "{hz}: peak at {peak}");
            // A Hann window halves a sine's peak: n / 4.
            assert!((level - 4410.0 / 4.0).abs() < 4410.0 * 0.05, "{level}");
        }
    }

    #[test]
    fn the_window_tapers_to_zero() {
        let window = hann(&[1.0; 8]);
        assert_eq!(window[0], 0.0);
        assert!((window[4] - 1.0).abs() < 1e-12);
        assert!((window[2] - 0.5).abs() < 1e-12);
        assert!(hann(&[]).is_empty());
    }

    #[test]
    fn a_constant_is_all_in_the_first_bin() {
        let bins = spectrum(&[0.5; 64], 64.0);
        assert_eq!(bins.len(), 33);
        assert!((bins[0].1 - 32.0).abs() < 1e-9);
        assert!(bins[1..].iter().all(|(_, m)| *m < 1e-9));
        assert!(spectrum(&[], 48_000.0).is_empty());
    }
    #[test]
    fn the_full_spectrum_mirrors_the_half() {
        let samples = noise(9).take(16).collect::<Vec<_>>();
        let full = spectrum_full(&samples, 16.0);
        let half = spectrum(&samples, 16.0);
        assert_eq!(full.len(), 16);
        assert_eq!(&full[..9], &half[..]);
        // A real signal's bins above the Nyquist frequency mirror those below.
        for k in 1..8 {
            assert!((full[k].1 - full[16 - k].1).abs() < 1e-9);
        }
        assert!(spectrum_full(&[], 1.0).is_empty());
    }

    #[test]
    fn a_tone_keeps_its_phase_across_buffers() {
        let mut wave = tone(440.0, 48_000.0).amplitude(0.5);
        let first: Vec<f64> = wave.by_ref().take(100).collect();
        let second: Vec<f64> = wave.take(100).collect();
        let whole: Vec<f64> = tone(440.0, 48_000.0).amplitude(0.5).take(200).collect();
        assert_eq!([first, second].concat(), whole);
        let shifted: Vec<f64> = tone(1.0, 4.0).phase(PI / 2.0).take(2).collect();
        assert!((shifted[0] - 1.0).abs() < 1e-12 && shifted[1].abs() < 1e-12);
    }

    #[test]
    fn partials_above_nyquist_are_left_out() {
        // 300 Hz at 1 kHz: the second partial (600 Hz) would alias.
        let mix = partials(300.0, 1000.0, &[1.0, 1.0, 1.0]);
        assert_eq!(mix.parts.len(), 1);
    }

    #[test]
    fn filtered_noise_is_smoother() {
        let white: Vec<f64> = noise(3).take(4096).collect();
        let low: Vec<f64> = noise(3).low_pass(100.0, 48_000.0).take(4096).collect();
        let roughness = |s: &[f64]| s.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>();
        assert!(roughness(&low) * 50.0 < roughness(&white));
        assert_ne!(noise(1).next(), noise(2).next());
    }

    #[test]
    fn a_mix_ends_when_every_source_has() {
        let a = [1.0, 2.0, 3.0].into_iter();
        let b = [10.0].into_iter();
        let out: Vec<f64> = mix().add(1.0, a).add(0.5, b).collect();
        assert_eq!(out, [6.0, 2.0, 3.0]);
        assert_eq!(Mix::new().next(), None);
    }

    #[test]
    fn triggers_respect_the_depth_and_the_end() {
        let wave = [0.0, 1.0, -1.0, 1.0, 1.0];
        assert_eq!(trigger(&wave, 0.5, 1, Edge::Rising), Some(0));
        assert_eq!(trigger(&wave, 0.5, 2, Edge::Rising), Some(2));
        assert_eq!(trigger(&wave, 0.5, 3, Edge::Rising), None);
        assert_eq!(trigger(&wave, 0.0, 1, Edge::Falling), Some(1));
        // Too short for the depth.
        assert_eq!(trigger(&[0.0], 0.5, 1, Edge::Rising), None);
        assert_eq!(trigger(&[], 0.5, 0, Edge::Rising), None);
    }

    #[test]
    fn notes_round_trip() {
        for name in ["C-1", "A0", "C4", "F#5", "B9", "G#2"] {
            let hz = note_hz(name).unwrap();
            let (letter, octave, cents) = nearest_note(hz).unwrap();
            assert_eq!(format!("{letter}{octave}"), name);
            assert_eq!(cents, 0.0, "{name}");
        }
        assert_eq!(note_hz("Bb3"), note_hz("A#3"));
        assert_eq!(note_hz("B#3"), note_hz("C4"));
        assert_eq!(note_hz("E♭4"), note_hz("D#4"));
        for bad in ["", "X4", "A10", "A-2", "Ax4", "A4.5"] {
            assert_eq!(note_hz(bad), None, "{bad}");
        }
        assert_eq!(nearest_note(0.0), None);
        assert_eq!(nearest_note(f64::NAN), None);
        assert_eq!(nearest_note(1.0), None);
    }

    #[test]
    fn scope_tuis_table_is_its_octave_0_doubled() {
        let t = NoteTable::ScopeTui;
        assert_eq!(t.hz("C"), Some(f64::from(16.35f32)));
        assert_eq!(t.hz("B0"), Some(f64::from(30.87f32)));
        assert_eq!(t.hz("Eb3"), t.hz("D#3"));
        assert_eq!(t.hz("A4"), Some(440.0));
        // B# is the next octave's C, as in equal temperament.
        assert_eq!(t.hz("B#3"), t.hz("C4"));
        // No octave below 0; Cb0 would be B-1.
        assert_eq!(t.hz("C-1"), None);
        assert_eq!(t.hz("Cb0"), None);
        assert_eq!(t.hz("H"), None);
    }

    #[test]
    fn scope_tuis_buffer_sizes_come_out_sample_for_sample() {
        // scope-tui's `--tune`: `(rate as f32 * (1.0 / freq)).round()`.
        // Dividing in f64 by the table's pitch gives the same size for
        // every note it can read at the usual rates.
        for rate in [44_100u32, 48_000, 96_000] {
            for octave in 0..=8 {
                for name in NAMES {
                    let note = format!("{name}{octave}");
                    let hz = NoteTable::ScopeTui.hz(&note).unwrap();
                    let ours = (f64::from(rate) / hz).round() as u32;
                    let theirs = (rate as f32 * (1.0 / hz as f32)).round() as u32;
                    assert_eq!(ours, theirs, "{note} at {rate}");
                }
            }
        }
        // E2 at 48 kHz: 583 samples in scope-tui's table, 582 in equal
        // temperament.
        let size = |table: NoteTable| (48_000.0 / table.hz("E2").unwrap()).round();
        assert_eq!(size(NoteTable::ScopeTui), 583.0);
        assert_eq!(size(NoteTable::Equal), 582.0);
    }

    #[test]
    fn peaks_are_refined_between_bins() {
        // 1003 Hz with 10 Hz bins: between the 1000 and 1010 Hz bins.
        let rate = 10_240.0;
        let samples: Vec<f64> = tone(1003.0, rate).take(1024).collect();
        let bins = spectrum(&hann(&samples), rate);
        let hz = peak_hz(&bins).unwrap();
        assert!((hz - 1003.0).abs() < 1.0, "{hz}");
        // The 0 Hz bin never wins, however loud.
        assert_eq!(peak_hz(&[(0.0, 9.0), (10.0, 1.0)]), Some(10.0));
        assert_eq!(peak_hz(&[]), None);
        assert_eq!(peak_hz(&[(0.0, 9.0)]), None);
    }

    #[test]
    fn narrow_bands_take_the_level_between_bins() {
        // Bins 100 Hz apart rising 1 a bin: a band around 150 Hz holds no
        // bin, so it reads between 100 and 200 Hz.
        let spectrum: Vec<(f64, f64)> = (0..10).map(|i| (i as f64 * 100.0, i as f64)).collect();
        let b = bands(&spectrum, 1, 140.0, 160.0);
        assert!((b[0] - 1.497).abs() < 0.01, "{b:?}");
        assert!(bands(&spectrum, 0, 1.0, 2.0).is_empty());
        assert_eq!(bands(&spectrum, 2, 0.0, 100.0), [0.0, 0.0]);
        assert_eq!(bands(&[], 2, 1.0, 100.0), [0.0, 0.0]);
        // The top edge is in the last band.
        assert_eq!(bands(&spectrum, 2, 100.0, 900.0)[1], 9.0);
    }

    #[test]
    fn a_falloff_follows_more_bands_and_nan() {
        let mut f = Falloff::gravity(0.5);
        assert_eq!(f.update(&[1.0]), [1.0]);
        // A new band starts where it is; NaN falls.
        assert_eq!(f.update(&[f64::NAN, 2.0]), [0.5, 2.0]);
        assert_eq!(f.update(&[0.0]), [0.0]);
        f.reset();
        assert!(f.levels().is_empty());
        let mut instant = Falloff::gravity(0.0);
        instant.update(&[1.0]);
        assert_eq!(instant.update(&[0.25]), [0.25]);
    }
}

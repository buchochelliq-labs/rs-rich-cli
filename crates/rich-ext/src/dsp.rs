//! Signal processing for live views of audio and other sampled data: a
//! fast Fourier transform, a Hann window and magnitude spectra; test
//! signals (tones, harmonic partials, noise and a mix of them); trigger
//! detection, as an oscilloscope's; and the frequencies of musical notes.
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
/// note.
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
        "" => 4,
        digits => digits.parse().ok().filter(|o| (-1..=9).contains(o))?,
    };
    // Semitones from A4.
    let from_a4 = (octave - 4) * 12 + semitone - 9;
    Some(440.0 * 2f64.powf(f64::from(from_a4) / 12.0))
}

/// The note nearest `hz`, with sharps (`A4`, `C#3`), and how far `hz` is
/// from it in cents (hundredths of a semitone, from -50 to 50): a tuner's
/// reading. `None` for a frequency that is not positive and finite, or
/// that falls outside octaves -1 to 9.
///
/// ```
/// use rich_ext::dsp::nearest_note;
///
/// assert_eq!(nearest_note(440.0), Some(("A4".to_string(), 0.0)));
/// let (name, cents) = nearest_note(445.0).unwrap();
/// assert_eq!(name, "A4");
/// assert!((cents - 19.56).abs() < 0.01);
/// ```
pub fn nearest_note(hz: f64) -> Option<(String, f64)> {
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
    Some((format!("{name}{}", from_c / 12 - 1), cents))
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(nearest_note(hz), Some((name.to_string(), 0.0)), "{name}");
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
}

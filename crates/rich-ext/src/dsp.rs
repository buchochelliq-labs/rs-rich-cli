//! Signal processing for live views of audio and other sampled data: a
//! fast Fourier transform, a Hann window and a magnitude spectrum.
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

    fn noise(n: usize) -> Vec<f64> {
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
            let samples = noise(n);
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
}

//! Perceptual image diffing — CIELAB ΔE, denoised, reported as ranked regions.
//!
//! **Not a port.** `rich` has no image diffing; this is our own feature (see
//! `AGENTS.md`). The reference implementation is `scripts/image_diff.py` on the
//! branch of PR #41, which is deliberately never merged and serves as the
//! oracle — the same role `scripts/capture_golden.py` plays for render parity.
//!
//! # Why not a pixel diff
//!
//! Comparing pixels answers "did any byte change", which for anything
//! regenerated — artwork, anti-aliased text, a re-encoded screenshot — is
//! always yes. On the reference pair a naive diff reports 42% of pixels changed
//! and a bounding box covering 77% of the frame: technically true, useless.
//!
//! This instead asks "would a person notice, and where":
//!
//! 1. **Blur** both images, so sub-pixel noise and re-encoding artefacts stop
//!    registering as change.
//! 2. **Convert to CIELAB**, where Euclidean distance approximates perceived
//!    difference — unlike RGB, where a change in dark blue and the same
//!    numeric change in mid-green are nothing alike to the eye.
//! 3. **ΔE per pixel** (CIE76), then threshold.
//! 4. **Morphological open** (erode, then dilate) to drop speckle while
//!    keeping solid areas at their original size.
//! 5. **Label connected components** and rank them by area × severity, so the
//!    output leads with the change a human would point at first.
//!
//! On the reference pair that ranks the halo first at 47% of all change, in a
//! box covering 12% of the frame.

use image::{DynamicImage, GenericImageView};

/// Tuning for [`diff`]. The defaults match the oracle's, and were tuned on
/// regenerated-artwork pairs where noise is heavy — screenshot pairs are far
/// cleaner and tolerate a much lower `threshold`.
#[derive(Debug, Clone, Copy)]
pub struct DiffSettings {
    /// Gaussian blur radius applied to both images before comparison.
    pub blur: f32,
    /// ΔE above which a pixel counts as changed.
    pub threshold: f32,
    /// Side length of the square structuring element for the open.
    pub open_kernel: usize,
    /// Regions smaller than this many pixels are not reported.
    pub min_region: u64,
    /// How many regions to report.
    pub top: usize,
}

impl Default for DiffSettings {
    fn default() -> Self {
        Self {
            blur: 6.0,
            threshold: 60.0,
            open_kernel: 11,
            min_region: 400,
            top: 3,
        }
    }
}

/// One connected area of perceptible change.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Pixels in the region *after* denoising.
    pub area_px: u64,
    /// This region's share of all changed pixels, 0..=1.
    pub share_of_change: f32,
    /// Mean ΔE within the region — how strong the change is, not how big.
    pub mean_delta_e: f32,
}

/// The result of comparing two images.
#[derive(Debug, Clone)]
pub struct DiffReport {
    pub width: u32,
    pub height: u32,
    /// Fraction of pixels whose ΔE exceeds the threshold, measured *before*
    /// the morphological open. The regions are what survives the open; this is
    /// not, and the two deliberately differ.
    pub changed_fraction: f32,
    /// Fraction a plain byte comparison would call changed. Included because
    /// the gap between this and [`Self::changed_fraction`] is the entire
    /// argument for the pipeline.
    pub naive_changed_fraction: f32,
    pub mean_delta_e: f32,
    pub max_delta_e: f32,
    /// Ranked by area × severity, largest first.
    pub regions: Vec<Region>,
    /// Per-pixel ΔE, row-major, `width * height` long. Kept so callers can
    /// render a heatmap without recomputing.
    pub delta_e: Vec<f32>,
}

/// Why a diff could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffError {
    /// Comparing differently-sized images is meaningless — every pixel past
    /// the smaller extent would read as changed. Align them first.
    SizeMismatch {
        before: (u32, u32),
        after: (u32, u32),
    },
}

impl std::fmt::Display for DiffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SizeMismatch { before, after } => write!(
                f,
                "images differ in size ({}x{} vs {}x{}); align them first — \
                 a diff of differently-sized images is meaningless",
                before.0, before.1, after.0, after.1
            ),
        }
    }
}

impl std::error::Error for DiffError {}

/// Compare two images perceptually.
pub fn diff(
    before: &DynamicImage,
    after: &DynamicImage,
    settings: &DiffSettings,
) -> Result<DiffReport, DiffError> {
    let (w, h) = before.dimensions();
    if (w, h) != after.dimensions() {
        return Err(DiffError::SizeMismatch {
            before: (w, h),
            after: after.dimensions(),
        });
    }

    let lab_a = to_lab(&blur_rgb(before, settings.blur));
    let lab_b = to_lab(&blur_rgb(after, settings.blur));

    let n = (w as usize) * (h as usize);
    let mut delta_e = vec![0.0f32; n];
    let mut sum = 0.0f64;
    let mut max = 0.0f32;
    for i in 0..n {
        let d = ((lab_a[i * 3] - lab_b[i * 3]).powi(2)
            + (lab_a[i * 3 + 1] - lab_b[i * 3 + 1]).powi(2)
            + (lab_a[i * 3 + 2] - lab_b[i * 3 + 2]).powi(2))
        .sqrt();
        delta_e[i] = d;
        sum += f64::from(d);
        if d > max {
            max = d;
        }
    }

    // Measured BEFORE the open, matching the oracle: this is "how much of the
    // canvas differs perceptibly", not "how much survived denoising". The
    // regions below are what the open produces; this number is not.
    let changed = delta_e.iter().filter(|d| **d > settings.threshold).count();

    let mask: Vec<bool> = delta_e.iter().map(|d| *d > settings.threshold).collect();
    let opened = binary_open(&mask, w as usize, h as usize, settings.open_kernel);

    let labels = label_components(&opened, w as usize, h as usize);
    let regions = rank_regions(&labels, &delta_e, w, settings);

    Ok(DiffReport {
        width: w,
        height: h,
        changed_fraction: changed as f32 / n as f32,
        naive_changed_fraction: naive_changed_fraction(before, after),
        mean_delta_e: (sum / n as f64) as f32,
        max_delta_e: max,
        regions,
        delta_e,
    })
}

/// A pixel counts as "naively changed" when any channel moves by more than
/// this. The oracle's constant; it is a byte-level comparison, deliberately
/// unrelated to ΔE.
const NAIVE_CHANNEL_DELTA: i16 = 32;

/// The fraction of pixels a plain byte comparison would call changed — on the
/// **unblurred** originals.
///
/// This is the number the perceptual pipeline exists to improve on, and it is
/// only meaningful because it is computed the crude way: on the reference pair
/// it reports 42% of the canvas against the perceptual 12%.
fn naive_changed_fraction(before: &DynamicImage, after: &DynamicImage) -> f32 {
    let (a, b) = (before.to_rgb8(), after.to_rgb8());
    let changed = a
        .pixels()
        .zip(b.pixels())
        .filter(|(p, q)| {
            (0..3)
                .map(|c| (i16::from(p.0[c]) - i16::from(q.0[c])).abs())
                .max()
                .unwrap_or(0)
                > NAIVE_CHANNEL_DELTA
        })
        .count();
    changed as f32 / (a.width() * a.height()) as f32
}

/// Separable Gaussian blur over RGB, in f32.
///
/// **Known parity risk against the oracle.** PIL's `ImageFilter.GaussianBlur`
/// is not a true Gaussian — it approximates one with three box-blur passes.
/// This is a real Gaussian, so per-pixel values differ slightly near edges. The
/// parity test measures whether that survives into the reported region
/// geometry; if it ever does, replace this with PIL's box approximation rather
/// than tuning the radius.
fn blur_rgb(image: &DynamicImage, radius: f32) -> RgbF32 {
    let rgb = image.to_rgb8();
    let (w, h) = (rgb.width() as usize, rgb.height() as usize);
    let mut data: Vec<f32> = rgb.pixels().flat_map(|p| p.0.map(f32::from)).collect();

    if radius <= 0.0 {
        return RgbF32 {
            data,
            width: w,
            height: h,
        };
    }

    let kernel = gaussian_kernel(radius);
    let half = kernel.len() / 2;
    let mut tmp = vec![0.0f32; data.len()];

    // Horizontal, then vertical. Edges clamp — repeating the edge pixel rather
    // than treating outside as black, which would darken the borders and
    // manufacture change there.
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut acc = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let sx = (x + k).saturating_sub(half).min(w - 1);
                    acc += data[(y * w + sx) * 3 + c] * weight;
                }
                tmp[(y * w + x) * 3 + c] = acc;
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let mut acc = 0.0;
                for (k, weight) in kernel.iter().enumerate() {
                    let sy = (y + k).saturating_sub(half).min(h - 1);
                    acc += tmp[(sy * w + x) * 3 + c] * weight;
                }
                data[(y * w + x) * 3 + c] = acc;
            }
        }
    }

    RgbF32 {
        data,
        width: w,
        height: h,
    }
}

struct RgbF32 {
    data: Vec<f32>,
    width: usize,
    height: usize,
}

fn gaussian_kernel(radius: f32) -> Vec<f32> {
    // PIL treats `radius` as the standard deviation, and cuts the kernel off at
    // 3σ either side.
    let sigma = radius.max(1e-6);
    let half = (sigma * 3.0).ceil() as usize;
    let mut kernel: Vec<f32> = (0..=2 * half)
        .map(|i| {
            let d = i as f32 - half as f32;
            (-(d * d) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let total: f32 = kernel.iter().sum();
    for k in &mut kernel {
        *k /= total;
    }
    kernel
}

/// sRGB (0–255) to CIELAB, D65 white point.
fn to_lab(rgb: &RgbF32) -> Vec<f32> {
    const M: [[f32; 3]; 3] = [
        [0.4124, 0.3576, 0.1805],
        [0.2126, 0.7152, 0.0722],
        [0.0193, 0.1192, 0.9505],
    ];
    const WHITE: [f32; 3] = [0.95047, 1.0, 1.08883];

    let mut out = vec![0.0f32; rgb.data.len()];
    for i in 0..(rgb.width * rgb.height) {
        // Undo the sRGB transfer function to get linear light.
        let lin = |v: f32| {
            let v = v / 255.0;
            if v > 0.04045 {
                ((v + 0.055) / 1.055).powf(2.4)
            } else {
                v / 12.92
            }
        };
        let (r, g, b) = (
            lin(rgb.data[i * 3]),
            lin(rgb.data[i * 3 + 1]),
            lin(rgb.data[i * 3 + 2]),
        );

        let mut f = [0.0f32; 3];
        for (j, fj) in f.iter_mut().enumerate() {
            let xyz = (M[j][0] * r + M[j][1] * g + M[j][2] * b) / WHITE[j];
            // The classic ε/κ approximation, matching the oracle.
            *fj = if xyz > 0.008856 {
                xyz.cbrt()
            } else {
                7.787 * xyz + 16.0 / 116.0
            };
        }
        out[i * 3] = 116.0 * f[1] - 16.0;
        out[i * 3 + 1] = 500.0 * (f[0] - f[1]);
        out[i * 3 + 2] = 200.0 * (f[1] - f[2]);
    }
    out
}

/// Erode then dilate with a square structuring element — removes speckle
/// smaller than the kernel while leaving surviving areas their original size.
///
/// Outside the image counts as unset, matching `scipy.ndimage`'s default
/// `border_value=0`, so a blob touching the border erodes from that side too.
fn binary_open(mask: &[bool], w: usize, h: usize, kernel: usize) -> Vec<bool> {
    if kernel <= 1 {
        return mask.to_vec();
    }
    let eroded = morph(mask, w, h, kernel, true);
    morph(&eroded, w, h, kernel, false)
}

/// One morphological pass. `erode` = require every neighbour set; otherwise
/// require any. Separable: a square kernel is a horizontal pass then a vertical
/// one, which turns O(k²) per pixel into O(k).
fn morph(src: &[bool], w: usize, h: usize, kernel: usize, erode: bool) -> Vec<bool> {
    let half = kernel / 2;
    let mut tmp = vec![false; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = erode;
            for k in 0..kernel {
                let sx = (x + k) as isize - half as isize;
                // Out of bounds reads as unset.
                let v = if sx < 0 || sx >= w as isize {
                    false
                } else {
                    src[y * w + sx as usize]
                };
                if erode {
                    acc &= v;
                } else {
                    acc |= v;
                }
            }
            tmp[y * w + x] = acc;
        }
    }
    let mut out = vec![false; src.len()];
    for y in 0..h {
        for x in 0..w {
            let mut acc = erode;
            for k in 0..kernel {
                let sy = (y + k) as isize - half as isize;
                let v = if sy < 0 || sy >= h as isize {
                    false
                } else {
                    tmp[sy as usize * w + x]
                };
                if erode {
                    acc &= v;
                } else {
                    acc |= v;
                }
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// Label connected components with **4-connectivity**, matching
/// `scipy.ndimage.label`'s default structuring element (a cross, not a square).
/// Returns 0 for background and 1..=n for components.
fn label_components(mask: &[bool], w: usize, h: usize) -> Vec<u32> {
    let mut labels = vec![0u32; mask.len()];
    let mut next = 1u32;
    let mut stack: Vec<usize> = Vec::new();

    for start in 0..mask.len() {
        if !mask[start] || labels[start] != 0 {
            continue;
        }
        labels[start] = next;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let visit = |nx: usize, ny: usize, stack: &mut Vec<usize>, labels: &mut Vec<u32>| {
                let j = ny * w + nx;
                if mask[j] && labels[j] == 0 {
                    labels[j] = next;
                    stack.push(j);
                }
            };
            if x > 0 {
                visit(x - 1, y, &mut stack, &mut labels);
            }
            if x + 1 < w {
                visit(x + 1, y, &mut stack, &mut labels);
            }
            if y > 0 {
                visit(x, y - 1, &mut stack, &mut labels);
            }
            if y + 1 < h {
                visit(x, y + 1, &mut stack, &mut labels);
            }
        }
        next += 1;
    }
    labels
}

/// Per-component stats, ranked by area × severity.
fn rank_regions(labels: &[u32], delta_e: &[f32], w: u32, settings: &DiffSettings) -> Vec<Region> {
    let count = labels.iter().copied().max().unwrap_or(0) as usize;
    if count == 0 {
        return Vec::new();
    }

    let mut area = vec![0u64; count];
    let mut sum = vec![0f64; count];
    let mut bbox = vec![(u32::MAX, u32::MAX, 0u32, 0u32); count]; // x0, y0, x1, y1

    for (i, &l) in labels.iter().enumerate() {
        if l == 0 {
            continue;
        }
        let k = l as usize - 1;
        let (x, y) = ((i % w as usize) as u32, (i / w as usize) as u32);
        area[k] += 1;
        sum[k] += f64::from(delta_e[i]);
        let b = &mut bbox[k];
        b.0 = b.0.min(x);
        b.1 = b.1.min(y);
        b.2 = b.2.max(x);
        b.3 = b.3.max(y);
    }

    // The denominator is **every** changed pixel, including components later
    // dropped for being too small. Matching the oracle: a region's share is of
    // all change, not of the change that made the cut.
    let total: u64 = area.iter().sum();
    if total == 0 {
        return Vec::new();
    }

    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by(|&a, &b| {
        let sa = area[a] as f64 * (sum[a] / area[a] as f64);
        let sb = area[b] as f64 * (sum[b] / area[b] as f64);
        sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal)
    });

    order
        .into_iter()
        .filter(|&k| area[k] >= settings.min_region)
        .take(settings.top)
        .map(|k| {
            let (x0, y0, x1, y1) = bbox[k];
            Region {
                x: x0,
                y: y0,
                width: x1 - x0 + 1,
                height: y1 - y0 + 1,
                area_px: area[k],
                share_of_change: (area[k] as f64 / total as f64) as f32,
                mean_delta_e: (sum[k] / area[k] as f64) as f32,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32, c: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb(c)))
    }

    #[test]
    fn identical_images_report_no_change() {
        let a = solid(64, 64, [10, 20, 30]);
        let report = diff(&a, &a, &DiffSettings::default()).unwrap();
        assert_eq!(report.regions, Vec::new());
        assert_eq!(report.changed_fraction, 0.0);
        assert!(
            report.max_delta_e < 1e-3,
            "max ΔE was {}",
            report.max_delta_e
        );
    }

    #[test]
    fn mismatched_sizes_are_refused() {
        let err = diff(
            &solid(8, 8, [0; 3]),
            &solid(9, 8, [0; 3]),
            &DiffSettings::default(),
        )
        .unwrap_err();
        assert_eq!(
            err,
            DiffError::SizeMismatch {
                before: (8, 8),
                after: (9, 8)
            }
        );
    }

    #[test]
    fn a_solid_block_is_found_and_measured() {
        let before = solid(128, 128, [0, 0, 0]);
        let mut after = RgbImage::from_pixel(128, 128, Rgb([0, 0, 0]));
        for y in 40..90 {
            for x in 30..100 {
                after.put_pixel(x, y, Rgb([255, 255, 255]));
            }
        }
        let report = diff(
            &before,
            &DynamicImage::ImageRgb8(after),
            &DiffSettings::default(),
        )
        .unwrap();

        assert_eq!(report.regions.len(), 1, "expected one region");
        let r = &report.regions[0];
        // Blur softens the edges, so the box is close to but not exactly the
        // drawn rectangle. Assert it is in the right place at the right scale.
        assert!((r.x as i32 - 30).abs() <= 8, "x was {}", r.x);
        assert!((r.y as i32 - 40).abs() <= 8, "y was {}", r.y);
        assert!((r.width as i32 - 70).abs() <= 16, "width was {}", r.width);
        assert!(
            (r.height as i32 - 50).abs() <= 16,
            "height was {}",
            r.height
        );
        assert!(
            (r.share_of_change - 1.0).abs() < 1e-6,
            "one region should hold all the change, got {}",
            r.share_of_change
        );
    }

    #[test]
    fn speckle_smaller_than_the_kernel_is_denoised_away() {
        let before = solid(128, 128, [0, 0, 0]);
        let mut after = RgbImage::from_pixel(128, 128, Rgb([0, 0, 0]));
        // Scattered single pixels: real change, but not perceptible change.
        for i in 0..40 {
            after.put_pixel((i * 3) % 128, (i * 7) % 128, Rgb([255, 255, 255]));
        }
        let report = diff(
            &before,
            &DynamicImage::ImageRgb8(after),
            &DiffSettings::default(),
        )
        .unwrap();
        assert_eq!(
            report.regions,
            Vec::new(),
            "speckle should not survive the open"
        );
    }

    #[test]
    fn four_connectivity_keeps_diagonal_blobs_apart() {
        // Two squares touching only at a corner are two regions under
        // 4-connectivity, one under 8. scipy's default is 4.
        let mask: Vec<bool> = {
            let mut m = vec![false; 8 * 8];
            for (x, y) in [
                (1, 1),
                (2, 1),
                (1, 2),
                (2, 2),
                (3, 3),
                (4, 3),
                (3, 4),
                (4, 4),
            ] {
                m[y * 8 + x] = true;
            }
            m
        };
        let labels = label_components(&mask, 8, 8);
        assert_eq!(labels.iter().copied().max().unwrap(), 2);
    }
}

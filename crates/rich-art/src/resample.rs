//! Resampling of 8-bit RGB and RGBA images with `image`'s Lanczos3 and
//! Triangle filters, the crop done while resampling: the same pixels as
//! `image`'s `imageops::resize` followed by a crop, made several times
//! faster.
//!
//! `image` resamples through `get_pixel` and a per-sample conversion, and
//! resamples the whole image before a [`Cover`](crate::ImageFit::Cover) fit
//! crops part of a side away. This does the same arithmetic in the same
//! order (the same weights, computed the same way in `f32`; a vertical pass
//! into `f32`, then a horizontal one; each sum accumulated tap by tap from
//! zero; rounded half away from zero), over contiguous rows the compiler
//! can vectorise, and only for the output pixels the crop keeps and the
//! input columns they read. Each output value goes through exactly the
//! operations `image` gives it, so the result is bit for bit the same, not
//! merely close (the tests compare them).
//!
//! Requires the `image` feature.

use image::{DynamicImage, RgbImage, RgbaImage};
use std::ops::Range;

/// The filters this resamples with, as `image`'s `FilterType` names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter {
    /// `FilterType::Lanczos3`.
    Lanczos3,
    /// `FilterType::Triangle`.
    Triangle,
}

impl Filter {
    /// `image`'s support for the filter.
    fn support(self) -> f32 {
        match self {
            Filter::Lanczos3 => 3.0,
            Filter::Triangle => 1.0,
        }
    }

    /// `image`'s kernel for the filter.
    fn kernel(self, x: f32) -> f32 {
        match self {
            Filter::Lanczos3 => {
                if x.abs() < 3.0 {
                    sinc(x) * sinc(x / 3.0)
                } else {
                    0.0
                }
            }
            Filter::Triangle => {
                if x.abs() < 1.0 {
                    1.0 - x.abs()
                } else {
                    0.0
                }
            }
        }
    }
}

/// `image`'s `sinc`.
fn sinc(t: f32) -> f32 {
    let a = t * std::f32::consts::PI;
    if t == 0.0 { 1.0 } else { a.sin() / a }
}

/// `image` resampled with `filter` to `rw × rh` and cropped to the
/// `width × height` pixels at `(x, y)`: what
/// `image.resize_exact(rw, rh, filter).crop_imm(x, y, width, height)`
/// gives, of the same pixel type. `None` when this does not cover the case
/// (a pixel type other than 8-bit RGB or RGBA, an empty image, a crop
/// outside the resampled size, or a size `image` copies instead of
/// resampling), so the caller takes `image`'s own path.
pub(crate) fn resize_crop(
    image: &DynamicImage,
    filter: Filter,
    (rw, rh): (u32, u32),
    (x, y): (u32, u32),
    (width, height): (u32, u32),
) -> Option<DynamicImage> {
    let (iw, ih) = (image.width(), image.height());
    if iw == 0 || ih == 0 || rw == 0 || rh == 0 || width == 0 || height == 0 {
        return None;
    }
    // `image` copies an image resized to its own size.
    if (rw, rh) == (iw, ih) {
        return None;
    }
    if x.checked_add(width)? > rw || y.checked_add(height)? > rh {
        return None;
    }
    let horizontal = taps(filter, iw, rw, x..x + width);
    let vertical = taps(filter, ih, rh, y..y + height);
    match image {
        DynamicImage::ImageRgb8(src) => {
            let out = resample::<3>(src.as_raw(), iw, &horizontal, &vertical);
            RgbImage::from_raw(width, height, out).map(DynamicImage::ImageRgb8)
        }
        DynamicImage::ImageRgba8(src) => {
            let out = resample::<4>(src.as_raw(), iw, &horizontal, &vertical);
            RgbaImage::from_raw(width, height, out).map(DynamicImage::ImageRgba8)
        }
        _ => None,
    }
}

/// [`resize_crop`] to `width × height` without a crop, as RGBA.
pub(crate) fn resize_rgba(
    image: &DynamicImage,
    filter: Filter,
    width: u32,
    height: u32,
) -> Option<RgbaImage> {
    resize_crop(image, filter, (width, height), (0, 0), (width, height))
        .map(DynamicImage::into_rgba8)
}

/// One output pixel's taps: the first input index and its weights.
struct Taps {
    left: usize,
    weights: Vec<f32>,
}

/// The taps of the outputs `range` of `len_out` resampled from `len_in`,
/// as `image`'s `vertical_sample` and `horizontal_sample` compute them.
fn taps(filter: Filter, len_in: u32, len_out: u32, range: Range<u32>) -> Vec<Taps> {
    let ratio = len_in as f32 / len_out as f32;
    let sratio = if ratio < 1.0 { 1.0 } else { ratio };
    let support = filter.support() * sratio;
    let len_in = i64::from(len_in);
    range
        .map(|out| {
            let input = (out as f32 + 0.5) * ratio;
            let left = ((input - support).floor() as i64).clamp(0, len_in - 1);
            let right = ((input + support).ceil() as i64).clamp(left + 1, len_in);
            let input = input - 0.5;
            let mut weights = Vec::with_capacity((right - left) as usize);
            let mut sum = 0.0;
            for i in left..right {
                let w = filter.kernel((i as f32 - input) / sratio);
                weights.push(w);
                sum += w;
            }
            for w in &mut weights {
                *w /= sum;
            }
            Taps {
                left: left as usize,
                weights,
            }
        })
        .collect()
}

/// `v` clamped to 0 to 255 and rounded half away from zero, as `image`
/// rounds a sum to 8 bits (with `f32::round`, a call to `roundf` on
/// x86-64's baseline). The fraction `v - trunc(v)` is exact for these
/// values, so comparing it with a half decides as `round` does.
fn to_u8(v: f32) -> u8 {
    let v = v.clamp(0.0, 255.0);
    let whole = v as u8;
    if v - f32::from(whole) >= 0.5 {
        whole + 1
    } else {
        whole
    }
}

/// How many output rows [`resample`] sums side by side.
const ROWS: usize = 8;

/// The resample for `C` channels (3: RGB; 4: RGBA) of `src`, `iw` pixels
/// wide, for the kept outputs' taps.
fn resample<const C: usize>(
    src: &[u8],
    iw: u32,
    horizontal: &[Taps],
    vertical: &[Taps],
) -> Vec<u8> {
    let width = horizontal.len();
    // The input columns the kept output columns read.
    let lo = horizontal.iter().map(|t| t.left).min().unwrap_or(0);
    let hi = horizontal
        .iter()
        .map(|t| t.left + t.weights.len())
        .max()
        .unwrap_or(0);
    let span = (hi - lo) * C;
    let stride = iw as usize * C;
    // `ROWS` rows at a time: `image`'s vertical pass for the columns the
    // kept ones read, value for value, a line each; the lines laid out
    // column by column, so its horizontal pass sums every row of the group
    // side by side, in vectors.
    let mut out = vec![0u8; width * vertical.len() * C];
    let mut line = vec![0.0f32; span];
    let mut block = vec![[0.0f32; ROWS]; span];
    for (group, rows) in vertical.chunks(ROWS).zip(out.chunks_mut(width * C * ROWS)) {
        for (r, taps) in group.iter().enumerate() {
            line.fill(0.0);
            for (i, &w) in taps.weights.iter().enumerate() {
                let start = (taps.left + i) * stride + lo * C;
                for (t, &v) in line.iter_mut().zip(&src[start..start + span]) {
                    *t += f32::from(v) * w;
                }
            }
            for (column, &v) in block.iter_mut().zip(&line) {
                column[r] = v;
            }
        }
        for (x, taps) in horizontal.iter().enumerate() {
            let start = taps.left - lo;
            for c in 0..C {
                let mut t = [0.0f32; ROWS];
                for (i, &w) in taps.weights.iter().enumerate() {
                    let column = &block[(start + i) * C + c];
                    for r in 0..ROWS {
                        t[r] += column[r] * w;
                    }
                }
                for (r, row) in rows.chunks_exact_mut(width * C).enumerate() {
                    row[x * C + c] = to_u8(t[r]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::imageops::FilterType;
    use image::{GenericImageView, Rgb, Rgba};

    fn filter(filter: FilterType) -> Filter {
        match filter {
            FilterType::Lanczos3 => Filter::Lanczos3,
            FilterType::Triangle => Filter::Triangle,
            other => panic!("not resampled here: {other:?}"),
        }
    }

    /// A picture with edges, gradients, noise and uneven alpha, so ringing
    /// and clamping are exercised, not only flat colour.
    fn picture(w: u32, h: u32, seed: u32) -> RgbaImage {
        let mut state = seed.wrapping_mul(2_654_435_761).max(1);
        RgbaImage::from_fn(w, h, |x, y| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            let noise = (state >> 24) as u8;
            let edge = if (x / 7 + y / 5) % 2 == 0 { 255 } else { 0 };
            Rgba([
                edge,
                (x * 255 / w.max(1)) as u8,
                noise,
                if (x + y) % 11 == 0 {
                    0
                } else {
                    200 + (y % 56) as u8
                },
            ])
        })
    }

    #[test]
    fn the_same_pixels_as_images_resize_and_crop() {
        // (source, resampled, crop): the video frame's Cover fit and its
        // cells' Cover fit, a contained fit, a stretch, one side kept,
        // upscaling, odd sizes and tiny ones.
        type Case = ((u32, u32), (u32, u32), [u32; 4]);
        let cases: &[Case] = &[
            ((480, 270), (399, 224), [35, 0, 328, 224]),
            ((480, 270), (50, 28), [4, 0, 41, 28]),
            ((480, 270), (328, 185), [0, 0, 328, 185]),
            ((640, 360), (200, 150), [0, 0, 200, 150]),
            ((333, 517), (101, 157), [7, 13, 80, 120]),
            ((300, 200), (300, 120), [10, 0, 280, 120]),
            ((97, 61), (12, 5), [0, 0, 12, 5]),
            ((5, 400), (2, 37), [1, 3, 1, 30]),
            ((16, 9), (40, 23), [3, 1, 33, 20]),
            ((1280, 720), (656, 369), [0, 1, 656, 368]),
        ];
        for (seed, &((iw, ih), (rw, rh), crop)) in cases.iter().enumerate() {
            let rgba = picture(iw, ih, seed as u32 + 1);
            let rgb = RgbImage::from_fn(iw, ih, |x, y| {
                let p = rgba.get_pixel(x, y).0;
                Rgb([p[0], p[1], p[2]])
            });
            for image in [DynamicImage::ImageRgba8(rgba), DynamicImage::ImageRgb8(rgb)] {
                for filter in [FilterType::Lanczos3, FilterType::Triangle] {
                    let fast = resize_crop(
                        &image,
                        self::filter(filter),
                        (rw, rh),
                        (crop[0], crop[1]),
                        (crop[2], crop[3]),
                    )
                    .expect("an RGB(A) resample is covered");
                    let slow = image
                        .resize_exact(rw, rh, filter)
                        .crop_imm(crop[0], crop[1], crop[2], crop[3]);
                    assert_eq!(fast.color(), slow.color());
                    assert_eq!(fast.dimensions(), slow.dimensions());
                    let differing = fast
                        .as_bytes()
                        .iter()
                        .zip(slow.as_bytes())
                        .filter(|(a, b)| a != b)
                        .count();
                    assert_eq!(
                        differing,
                        0,
                        "{iw}x{ih} -> {rw}x{rh} crop {crop:?} {filter:?} ({:?})",
                        image.color()
                    );
                }
            }
        }
    }

    #[test]
    fn rounding_is_roundf() {
        let check = |v: f32| assert_eq!(to_u8(v), v.clamp(0.0, 255.0).round() as u8, "{v}");
        // The 64 `f32`s each side of every half and whole step, and a sweep.
        for step in 0..=512u32 {
            let (mut down, mut up) = (step as f32 / 2.0, step as f32 / 2.0);
            for _ in 0..64 {
                check(down);
                check(up);
                down = down.next_down();
                up = up.next_up();
            }
        }
        for i in -1000..257_000 {
            check(i as f32 / 1000.0);
        }
        assert_eq!(to_u8(-3.0), 0);
        assert_eq!(to_u8(300.0), 255);
    }

    #[test]
    fn cases_it_does_not_cover_go_to_image() {
        let image = DynamicImage::ImageRgba8(picture(40, 30, 3));
        let lanczos = Filter::Lanczos3;
        // The same size: `image` copies.
        assert!(resize_crop(&image, lanczos, (40, 30), (0, 0), (40, 30)).is_none());
        // A crop outside the resampled size.
        assert!(resize_crop(&image, lanczos, (20, 15), (10, 0), (20, 15)).is_none());
        // Another pixel type.
        let luma = DynamicImage::ImageLuma8(image.to_luma8());
        assert!(resize_crop(&luma, lanczos, (20, 15), (0, 0), (20, 15)).is_none());
    }
}

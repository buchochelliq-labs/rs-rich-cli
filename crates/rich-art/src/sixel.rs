//! Real pixels in the terminal, via the Sixel graphics protocol.
//!
//! Block rendering ([`BlockArt`](crate::block::BlockArt)) is bounded by the
//! character grid: one cell can carry two colours, so a photograph arrives
//! posterised no matter how much colour the terminal supports. Sixel sidesteps
//! the grid entirely — the terminal allocates a raster area and paints
//! individual pixels into it.
//!
//! Encoding is [`icy_sixel`], a pure-Rust encoder. The alternative wrappers
//! around `libsixel` pull a C library, which is a build problem on Windows —
//! precisely where this matters most, since Windows Terminal 1.22+ supports
//! Sixel out of the box.
//!
//! # Support is not reliably detectable
//!
//! The correct probe is a DA1 query (`ESC [ c`) whose reply lists `4` when
//! Sixel is available — but that needs a round trip on a tty, which is not
//! available when output is piped, and terminals that ignore the query leave
//! you waiting. [`is_probably_supported`] is therefore a **heuristic over
//! environment variables**, and it will be wrong somewhere. That is exactly why
//! the CLI exposes an explicit picker rather than trusting detection: when the
//! guess is wrong, the user overrides it.

use crate::image_color::{palette_rgb, preprocess};
use crate::{ColorDistance, Dither, ImageColorMode};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;

/// Assumed size of a character cell in pixels, used to convert a width in
/// columns into a width in pixels.
///
/// The terminal knows the real figure and will not tell us without a `CSI 16 t`
/// query, so this is a conventional default (8×16 is the classic VGA cell, and
/// most terminal fonts sit near it). Being slightly off changes how much of the
/// line the image spans, not whether it renders.
pub const DEFAULT_CELL_PX: (u32, u32) = (8, 16);

/// Largest raster, in pixels, that [`SixelArt::encode`] will produce: 16 Mpx,
/// the same cap [`ImageArt`](crate::ImageArt)'s fitting applies. A tall, thin
/// image asked to fill a wide terminal would otherwise scale to hundreds of
/// megapixels (gigabytes of RGBA) before a byte is written.
pub const MAX_PIXELS: u64 = 16 * 1024 * 1024;

/// Largest repeat count (`!n`) a Sixel decoder accepts in one introducer.
const SIXEL_REPEAT_MAX: usize = 65_535;

/// An image rendered as Sixel graphics.
pub struct SixelArt {
    image: DynamicImage,
    /// Target width in terminal columns; pixels are derived from it.
    columns: Option<usize>,
    max_rows: Option<usize>,
    cell_px: (u32, u32),
    max_colors: u16,
    color_mode: ImageColorMode,
    dither: Dither,
    distance: ColorDistance,
    /// Keep transparency as `ImageBackground::TerminalDefault` defines it:
    /// pixels at or over half opacity show their own colour.
    transparent: bool,
}

impl SixelArt {
    pub fn new(image: DynamicImage) -> Self {
        Self {
            image,
            columns: None,
            max_rows: None,
            cell_px: DEFAULT_CELL_PX,
            // 256 is the Sixel maximum and what a photographic image wants.
            max_colors: 256,
            color_mode: ImageColorMode::TrueColor,
            dither: Dither::None,
            distance: ColorDistance::Rgb,
            transparent: false,
        }
    }

    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
        Ok(Self::new(image::open(path)?))
    }

    /// Render this many columns wide instead of filling the console.
    pub fn width(mut self, columns: usize) -> Self {
        self.columns = Some(columns);
        self
    }

    /// Cap the height in character rows, preserving the aspect ratio.
    pub fn height(mut self, rows: usize) -> Self {
        self.max_rows = Some(rows);
        self
    }

    /// Override the assumed character-cell size in pixels.
    pub fn cell_px(mut self, width: u32, height: u32) -> Self {
        self.cell_px = (width.max(1), height.max(1));
        self
    }

    /// Palette size, 2–256. Fewer colours means a shorter escape sequence.
    pub fn max_colors(mut self, colors: u16) -> Self {
        self.max_colors = colors.clamp(2, 256);
        self
    }

    /// Quantize to a fixed palette (ANSI256, ANSI16 or grayscale) instead of
    /// letting the encoder choose up to [`max_colors`](Self::max_colors)
    /// adaptive colours. The raster is snapped to the palette (and dithered)
    /// exactly as the text backends do, then encoded with exactly those
    /// colours; pixels under half opacity stay transparent. Truecolor (the
    /// default) keeps the adaptive encoder and its output unchanged.
    pub(crate) fn color_processing(
        mut self,
        mode: ImageColorMode,
        dither: Dither,
        distance: ColorDistance,
    ) -> Self {
        self.color_mode = mode;
        self.dither = dither;
        self.distance = distance;
        self
    }

    /// Show pixels at or over half opacity in their own colour rather than
    /// darkened by their alpha when quantizing to a reduced palette; those
    /// under half opacity stay transparent either way.
    pub(crate) fn keep_transparency(mut self, transparent: bool) -> Self {
        self.transparent = transparent;
        self
    }

    /// Target size in **pixels** for the given available width in columns, or
    /// `None` when it overflows or exceeds [`MAX_PIXELS`].
    pub(crate) fn checked_pixel_size(&self, available: usize) -> Option<(u32, u32)> {
        let (iw, ih) = self.image.dimensions();
        if iw == 0 || ih == 0 {
            return Some((1, 1));
        }
        let columns = u64::try_from(self.columns.unwrap_or(available).max(1)).ok()?;
        let mut px_w = columns.checked_mul(u64::from(self.cell_px.0))?;
        let mut px_h = px_w.checked_mul(u64::from(ih))? / u64::from(iw);

        if let Some(rows) = self.max_rows {
            let cap = u64::try_from(rows.max(1))
                .ok()?
                .checked_mul(u64::from(self.cell_px.1))?;
            if px_h > cap {
                px_w = px_w.checked_mul(cap)? / px_h;
                px_h = cap;
            }
        }
        let (w, h) = (px_w.max(1), px_h.max(1));
        if w.checked_mul(h)? > MAX_PIXELS {
            return None;
        }
        Some((u32::try_from(w).ok()?, u32::try_from(h).ok()?))
    }

    /// The Sixel escape sequence for this image, or `None` if encoding failed
    /// or the raster would exceed [`MAX_PIXELS`] (checked before resizing).
    ///
    /// Failure is not worth propagating to a renderable: the caller has already
    /// chosen Sixel, and the useful response is to fall back to blocks.
    pub fn encode(&self, available: usize) -> Option<String> {
        let (w, h) = self.checked_pixel_size(available)?;
        let mut scaled = self
            .image
            .resize_exact(w, h, FilterType::Lanczos3)
            .to_rgba8();
        if self.color_mode != ImageColorMode::TrueColor {
            let kept = crate::image_art::clear_mask(&mut scaled, self.transparent);
            let transparent: Vec<bool> = match &kept {
                Some(clear) => clear.clone(),
                None => scaled.pixels().map(|p| p.0[3] < 128).collect(),
            };
            let indices = preprocess(
                &mut scaled,
                self.color_mode,
                self.dither,
                self.distance,
                kept.as_deref(),
            )?;
            let pixels: Vec<Option<u8>> = indices
                .into_iter()
                .zip(transparent)
                .map(|(index, clear)| (!clear).then_some(index))
                .collect();
            return encode_indexed(&pixels, w as usize, h as usize);
        }
        let opts = icy_sixel::EncodeOptions {
            max_colors: self.max_colors,
            ..Default::default()
        };
        icy_sixel::sixel_encode(scaled.as_raw(), w as usize, h as usize, &opts).ok()
    }
}

/// Encode a raster of fixed-palette indices (`None` = transparent) as Sixel,
/// or `None` when `pixels` is not `width * height` long or the raster exceeds
/// [`MAX_PIXELS`].
///
/// Each register is the ANSI palette index itself, defined once with its RGB
/// in whole percent, so the colours are exact up to Sixel's own precision.
/// The header matches the adaptive encoder's: square pixels, a transparent
/// background, and raster attributes for terminals that drop the DCS
/// parameters.
pub(crate) fn encode_indexed(pixels: &[Option<u8>], width: usize, height: usize) -> Option<String> {
    use std::fmt::Write;
    let area = width.checked_mul(height)?;
    if area != pixels.len() || area as u64 > MAX_PIXELS {
        return None;
    }
    let used: std::collections::BTreeSet<u8> = pixels.iter().flatten().copied().collect();
    let mut out = format!("\x1bP9;1;0q\"1;1;{width};{height}");
    let percent = |v: u8| (u32::from(v) * 100 + 127) / 255;
    for &index in &used {
        let [r, g, b] = palette_rgb(index);
        let _ = write!(
            out,
            "#{index};2;{};{};{}",
            percent(r),
            percent(g),
            percent(b)
        );
    }
    // Write `len` columns of `bits` for the current colour. Decoders cap a
    // repeat count at 65 535 (icy_sixel rejects anything larger), so a longer
    // run is written in pieces.
    let write_run = |out: &mut String, bits: u8, len: usize| {
        let glyph = char::from(63 + bits);
        let mut left = len;
        while left > 0 {
            let piece = left.min(SIXEL_REPEAT_MAX);
            if piece > 3 {
                let _ = write!(out, "!{piece}{glyph}");
            } else {
                (0..piece).for_each(|_| out.push(glyph));
            }
            left -= piece;
        }
    };
    // One pass over each band collects, per colour, the columns it touches
    // and their six-pixel bit patterns, so the work is proportional to the
    // pixels rather than to pixels times colours. Storage is bounded by six
    // entries per column.
    let bands = height.div_ceil(6);
    let mut columns: Vec<Vec<(usize, u8)>> = vec![Vec::new(); 256];
    for band in 0..bands {
        columns.iter_mut().for_each(Vec::clear);
        let rows = band * 6..(band * 6 + 6).min(height);
        for x in 0..width {
            let mut cell = [(0u8, 0u8); 6];
            let mut count = 0;
            for (dy, y) in rows.clone().enumerate() {
                let Some(index) = pixels[y * width + x] else {
                    continue;
                };
                match cell[..count].iter_mut().find(|(i, _)| *i == index) {
                    Some((_, bits)) => *bits |= 1 << dy,
                    None => {
                        cell[count] = (index, 1 << dy);
                        count += 1;
                    }
                }
            }
            for &(index, bits) in &cell[..count] {
                columns[index as usize].push((x, bits));
            }
        }
        let mut first = true;
        for &index in &used {
            // Trailing empty columns need no characters at all, and a colour
            // absent from this band needs none either.
            let entries = &columns[index as usize];
            if entries.is_empty() {
                continue;
            }
            if !first {
                out.push('$');
            }
            first = false;
            let _ = write!(out, "#{index}");
            // Merge equal neighbours (including the empty gaps between
            // entries) into maximal runs.
            let (mut run_bits, mut run_len, mut next_x) = (0u8, 0usize, 0usize);
            for &(x, bits) in entries {
                for (segment_bits, segment_len) in [(0, x - next_x), (bits, 1)] {
                    if segment_len == 0 {
                        continue;
                    }
                    if segment_bits == run_bits {
                        run_len += segment_len;
                    } else {
                        write_run(&mut out, run_bits, run_len);
                        (run_bits, run_len) = (segment_bits, segment_len);
                    }
                }
                next_x = x + 1;
            }
            write_run(&mut out, run_bits, run_len);
        }
        if band + 1 < bands {
            out.push('-');
        }
    }
    out.push_str("\x1b\\");
    Some(out)
}

impl Renderable for SixelArt {
    fn rich_render(&self, _console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        match self.encode(options.max_width) {
            // A control segment: the console must neither measure this as text
            // (it is thousands of characters wide) nor wrap it. The terminal
            // advances the cursor itself once it has drawn the raster.
            Some(sixel) => vec![Segment::control(sixel), Segment::line()],
            None => Vec::new(),
        }
    }
}

/// A best-effort guess at whether the terminal renders Sixel.
///
/// **A heuristic, not a probe.** The reliable test is a DA1 query, which needs
/// a tty round trip. Callers should treat this as a default that the user can
/// override, never as a fact.
///
/// Two variables override the guess. `RICH_GRAPHICS` names the graphics
/// protocol (`sixel` forces Sixel on; `none`, `kitty` or `iterm` turn it off)
/// and wins when set to one of those. Otherwise `RICH_SIXEL` (`1`/`true`/`yes`/
/// `on` or `0`/`false`/`no`/`off`) forces it either way. These are the same
/// rules `rich-ext`'s capability detection follows. Unrecognised values are
/// ignored.
pub fn is_probably_supported() -> bool {
    guess_support(
        std::env::var("RICH_GRAPHICS").ok().as_deref(),
        std::env::var("RICH_SIXEL").ok().as_deref(),
        std::env::var_os("WT_SESSION").is_some(),
        std::env::var("TERM").ok().as_deref(),
        std::env::var("TERM_PROGRAM").ok().as_deref(),
    )
}

/// The heuristic itself, over its inputs rather than over the environment —
/// so it can be tested without mutating process state (which is `unsafe` in
/// this edition, and racy across threads besides).
fn guess_support(
    graphics_var: Option<&str>,
    override_var: Option<&str>,
    windows_terminal: bool,
    term: Option<&str>,
    term_program: Option<&str>,
) -> bool {
    // Explicit opt-out/opt-in first: whatever we guess, the user wins.
    let normalized = |value: Option<&str>| value.map(|v| v.trim().to_ascii_lowercase());
    match normalized(graphics_var).as_deref() {
        Some("sixel") => return true,
        Some("none" | "0" | "no" | "off" | "false" | "kitty" | "iterm" | "iterm2") => return false,
        _ => {}
    }
    match normalized(override_var).as_deref() {
        Some("0" | "false" | "no" | "off") => return false,
        Some("1" | "true" | "yes" | "on") => return true,
        _ => {}
    }

    // Windows Terminal has supported Sixel since 1.22 and sets WT_SESSION. It
    // does not publish its version in the environment, so this accepts older
    // 1.x releases too and renders nothing there — the picker is the remedy.
    if windows_terminal {
        return true;
    }

    if let Some(term) = term {
        let term = term.to_ascii_lowercase();
        if term.contains("sixel") || term.contains("mlterm") || term.contains("foot") {
            return true;
        }
    }
    if let Some(program) = term_program {
        let program = program.to_ascii_lowercase();
        if program.contains("wezterm") || program.contains("mintty") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(w: u32, h: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(w, h, Rgb([200, 40, 90])))
    }

    /// Decode a Sixel sequence back to RGBA pixels.
    fn decode(sixel: &str) -> icy_sixel::SixelImage {
        icy_sixel::SixelImage::decode(sixel.as_bytes()).expect("decodes")
    }

    #[test]
    fn indexed_encoding_round_trips_exact_palette_colours() {
        // Two stripes of ANSI16 red (1) and blue (4), a transparent pixel, and
        // an odd height so the last band is partial.
        let (w, h) = (5, 7);
        let pixels: Vec<Option<u8>> = (0..w * h)
            .map(|i| match (i % w, i / w) {
                (0, 0) => None,
                (_, y) if y < 3 => Some(1),
                _ => Some(4),
            })
            .collect();
        let sixel = encode_indexed(&pixels, w, h).expect("valid raster");
        assert!(sixel.starts_with("\x1bP9;1;0q\"1;1;5;7#1;2;67;0;0#4;2;0;0;67#1"));
        assert!(sixel.ends_with("\x1b\\"));
        let image = decode(&sixel);
        // Decoders pad the last band to six rows; the padding is never set.
        assert_eq!((image.width, image.height), (w, h.div_ceil(6) * 6));
        let rgba = |x: usize, y: usize| {
            let i = (y * w + x) * 4;
            [
                image.pixels[i],
                image.pixels[i + 1],
                image.pixels[i + 2],
                image.pixels[i + 3],
            ]
        };
        assert_eq!(rgba(0, 0)[3], 0, "unset pixels stay transparent");
        assert_eq!(rgba(1, 0), [171, 0, 0, 255]);
        assert_eq!(rgba(4, 6), [0, 0, 171, 255]);
        assert!((h..image.height).all(|y| (0..w).all(|x| rgba(x, y)[3] == 0)));
        // A run longer than three uses the repeat introducer.
        assert!(sixel.contains("!5"), "{sixel:?}");
    }

    #[test]
    fn a_reduced_palette_encodes_only_its_own_colours() {
        let gradient = DynamicImage::ImageRgb8(RgbImage::from_fn(40, 24, |x, y| {
            Rgb([(x * 6) as u8, (y * 10) as u8, 128])
        }));
        for (mode, dither) in [
            (ImageColorMode::Ansi16, Dither::None),
            (ImageColorMode::Ansi16, Dither::Atkinson),
            (ImageColorMode::Grayscale, Dither::FloydSteinberg),
            (ImageColorMode::Ansi256, Dither::Bayer4x4),
        ] {
            let art = SixelArt::new(gradient.clone()).width(5).color_processing(
                mode,
                dither,
                ColorDistance::Rgb,
            );
            let sixel = art.encode(5).expect("encodes");
            assert_eq!(sixel, art.encode(5).unwrap(), "deterministic");
            let image = decode(&sixel);
            // Sixel stores whole percents, so compare after the same rounding.
            let allowed: Vec<[u8; 3]> = (0..=255u8)
                .filter(|&i| match mode {
                    ImageColorMode::Ansi16 => i < 16,
                    ImageColorMode::Grayscale => i == 16 || i >= 231,
                    _ => i >= 16,
                })
                .map(|i| {
                    palette_rgb(i).map(|v| {
                        let percent = (u32::from(v) * 100 + 127) / 255;
                        ((percent * 255 + 50) / 100) as u8
                    })
                })
                .collect();
            for pixel in image.pixels.chunks(4) {
                let rgb = [pixel[0], pixel[1], pixel[2]];
                assert!(allowed.contains(&rgb), "{mode:?} {dither:?}: {rgb:?}");
            }
        }
    }

    #[test]
    fn truecolor_keeps_the_adaptive_encoder_byte_for_byte() {
        let art = SixelArt::new(solid(16, 16)).width(2);
        let explicit = SixelArt::new(solid(16, 16)).width(2).color_processing(
            ImageColorMode::TrueColor,
            Dither::None,
            ColorDistance::Rgb,
        );
        assert_eq!(art.encode(2), explicit.encode(2));
    }

    #[test]
    fn a_tall_thin_image_is_refused_before_resizing() {
        // 1x400 at 80 columns would be 640 x 256 000 px (164 Mpx, 650 MB of
        // RGBA): refused up front, so this stays instant and small.
        let tall = DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 400, Rgb([9, 9, 9])));
        let started = std::time::Instant::now();
        assert!(SixelArt::new(tall.clone()).width(80).encode(80).is_none());
        let reduced = SixelArt::new(tall).width(80).color_processing(
            ImageColorMode::Ansi256,
            Dither::None,
            ColorDistance::Rgb,
        );
        assert!(reduced.encode(80).is_none());
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[test]
    fn huge_column_counts_are_refused_not_wrapped() {
        // 536 870 912 * 8 is 2^32: it used to wrap to a 1-pixel-wide raster
        // in release builds (and overflow-panic in debug ones).
        let art = SixelArt::new(solid(2, 1)).width(536_870_912);
        assert!(art.encode(80).is_none());
        let art = SixelArt::new(solid(2, 1)).width(usize::MAX);
        assert!(art.encode(80).is_none());
    }

    #[test]
    fn rasters_up_to_the_cap_still_encode() {
        // 4096 x 4096 is exactly 16 Mpx; one more row is over.
        let art = SixelArt::new(solid(1, 1)).cell_px(4096, 1).height(4096);
        assert_eq!(art.checked_pixel_size(1), Some((4096, 4096)));
        let over = SixelArt::new(solid(4096, 4097)).cell_px(1, 1).width(4096);
        assert_eq!(over.checked_pixel_size(1), None);
    }

    #[test]
    fn runs_longer_than_the_repeat_maximum_are_split() {
        // Decoders cap a repeat count at 65 535; one 70 000-pixel run must be
        // written as two, or the row fails to decode.
        let (w, h) = (70_000, 6);
        let sixel = encode_indexed(&vec![Some(1); w * h], w, h).expect("valid");
        assert!(sixel.contains("!65535~!4465~"), "split runs");
        let image = decode(&sixel);
        assert_eq!((image.width, image.height), (w, h));
        assert!(image
            .pixels
            .chunks(4)
            .all(|p| p == [171, 0, 0, 255].as_slice()));
    }

    /// The per-colour, full-row encoder the single-pass one replaced, kept
    /// as the oracle (with the run splitting both share).
    fn encode_indexed_reference(pixels: &[Option<u8>], width: usize, height: usize) -> String {
        use std::fmt::Write;
        let used: std::collections::BTreeSet<u8> = pixels.iter().flatten().copied().collect();
        let mut out = format!("\x1bP9;1;0q\"1;1;{width};{height}");
        let percent = |v: u8| (u32::from(v) * 100 + 127) / 255;
        for &index in &used {
            let [r, g, b] = palette_rgb(index);
            let _ = write!(
                out,
                "#{index};2;{};{};{}",
                percent(r),
                percent(g),
                percent(b)
            );
        }
        let bands = height.div_ceil(6);
        let mut row = vec![0u8; width];
        for band in 0..bands {
            let mut first = true;
            for &index in &used {
                row.fill(0);
                for (dy, y) in (band * 6..(band * 6 + 6).min(height)).enumerate() {
                    for (x, bits) in row.iter_mut().enumerate() {
                        if pixels[y * width + x] == Some(index) {
                            *bits |= 1 << dy;
                        }
                    }
                }
                let Some(end) = row.iter().rposition(|&bits| bits != 0) else {
                    continue;
                };
                if !first {
                    out.push('$');
                }
                first = false;
                let _ = write!(out, "#{index}");
                let mut x = 0;
                while x <= end {
                    let bits = row[x];
                    let run = row[x..=end].iter().take_while(|&&b| b == bits).count();
                    let glyph = char::from(63 + bits);
                    let mut left = run;
                    while left > 0 {
                        let piece = left.min(SIXEL_REPEAT_MAX);
                        if piece > 3 {
                            let _ = write!(out, "!{piece}{glyph}");
                        } else {
                            (0..piece).for_each(|_| out.push(glyph));
                        }
                        left -= piece;
                    }
                    x += run;
                }
            }
            if band + 1 < bands {
                out.push('-');
            }
        }
        out.push_str("\x1b\\");
        out
    }

    #[test]
    fn single_pass_encoding_matches_the_reference_byte_for_byte() {
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move |n: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % n
        };
        for (w, h, colours, holes) in [
            (1, 1, 1, 0),
            (5, 7, 2, 3),
            (17, 13, 4, 5),
            (40, 25, 240, 10),
            (64, 12, 3, 2),
            (9, 30, 256, 0),
            (70_000, 6, 1, 0),
        ] {
            // Runs of a colour (so repeats occur), with some holes.
            let mut pixels = Vec::with_capacity(w * h);
            let mut current = Some(0u8);
            for _ in 0..w * h {
                if next(8) == 0 {
                    current = if holes > 0 && next(holes + 1) == 0 {
                        None
                    } else {
                        Some(next(colours) as u8)
                    };
                }
                pixels.push(current);
            }
            assert_eq!(
                encode_indexed(&pixels, w, h).expect("valid"),
                encode_indexed_reference(&pixels, w, h),
                "{w}x{h}"
            );
        }
    }

    #[test]
    fn indexed_encoding_validates_its_raster() {
        assert!(encode_indexed(&[Some(1); 6], 3, 2).is_some());
        assert!(encode_indexed(&[Some(1); 5], 3, 2).is_none(), "short");
        assert!(encode_indexed(&[], usize::MAX, 2).is_none(), "overflow");
        assert!(encode_indexed(&[], 4097, 4096).is_none(), "over the cap");
    }

    #[test]
    fn width_in_columns_becomes_width_in_pixels() {
        let art = SixelArt::new(solid(100, 50)).width(40).cell_px(8, 16);
        // 40 columns * 8 px, and the height follows the 2:1 aspect ratio.
        assert_eq!(art.checked_pixel_size(80), Some((320, 160)));
    }

    #[test]
    fn a_row_cap_shrinks_both_dimensions() {
        let art = SixelArt::new(solid(100, 400))
            .width(40)
            .height(10)
            .cell_px(8, 16);
        let (w, h) = art.checked_pixel_size(80).expect("small");
        assert_eq!(h, 160, "capped to 10 rows * 16 px");
        assert!(w < 320, "width must shrink with it, got {w}");
    }

    #[test]
    fn encodes_to_a_sixel_sequence() {
        let art = SixelArt::new(solid(32, 32)).width(8).cell_px(8, 16);
        let sixel = art.encode(80).expect("encoding a solid image should work");
        // DCS introducer and String Terminator bracket every Sixel payload.
        assert!(sixel.starts_with('\u{1b}'), "expected an escape sequence");
        assert!(sixel.contains('q'), "expected the Sixel DCS selector");
        assert!(sixel.ends_with('\\'), "expected a string terminator");
    }

    #[test]
    fn renders_as_a_control_segment_so_it_is_never_wrapped() {
        let console = Console::builder().width(80).build();
        let art = SixelArt::new(solid(32, 32)).width(8);
        // Through the public path, so this also proves the console does not
        // wrap or re-measure the payload on the way out.
        let segments = console.record_output(|c| c.print(&art));
        assert!(!segments.is_empty(), "expected output");
        assert!(
            segments.iter().any(|s| s.control && s.text.contains('q')),
            "the payload must be a control segment, or the console will \
             measure and wrap thousands of columns of escape data"
        );
    }

    #[test]
    fn the_override_beats_every_other_signal() {
        // Even inside Windows Terminal, an explicit "no" must win.
        assert!(!guess_support(None, Some("0"), true, None, None));
        // And an explicit "yes" must win in a terminal we would otherwise
        // assume knows nothing about Sixel.
        assert!(guess_support(None, Some("1"), false, Some("dumb"), None));
    }

    #[test]
    fn rich_graphics_selects_or_rules_out_sixel() {
        // As documented for the CLI: RICH_GRAPHICS=sixel forces it on...
        assert!(guess_support(
            Some("sixel"),
            None,
            false,
            Some("dumb"),
            None
        ));
        assert!(guess_support(Some(" SIXEL "), None, false, None, None));
        // ...and beats RICH_SIXEL, as in rich-ext's capability detection.
        assert!(guess_support(Some("sixel"), Some("0"), false, None, None));
        assert!(!guess_support(Some("kitty"), Some("1"), true, None, None));
        assert!(!guess_support(Some("none"), None, true, None, None));
        // An unrecognised value is ignored, not treated as "off".
        assert!(guess_support(Some("bogus"), Some("1"), false, None, None));
        assert!(guess_support(Some("bogus"), None, true, None, None));
        // RICH_SIXEL accepts the same boolean spellings as rich-ext.
        assert!(guess_support(None, Some("on"), false, None, None));
        assert!(!guess_support(None, Some("OFF"), true, None, None));
    }

    #[test]
    fn recognises_terminals_that_support_sixel() {
        assert!(
            guess_support(None, None, true, None, None),
            "Windows Terminal"
        );
        assert!(guess_support(None, None, false, Some("foot"), None), "foot");
        assert!(
            guess_support(None, None, false, Some("mlterm"), None),
            "mlterm"
        );
        assert!(
            guess_support(None, None, false, None, Some("WezTerm")),
            "WezTerm"
        );
    }

    #[test]
    fn assumes_no_support_when_nothing_says_otherwise() {
        assert!(!guess_support(
            None,
            None,
            false,
            Some("xterm-256color"),
            None
        ));
        assert!(!guess_support(None, None, false, None, None));
    }
}

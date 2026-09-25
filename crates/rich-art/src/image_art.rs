//! A single, reusable picker across the image-art backends.
//!
//! [`AsciiArt`](crate::ascii::AsciiArt), [`BlockArt`](crate::block::BlockArt)
//! and [`SixelArt`](crate::sixel::SixelArt) are three different answers to
//! "how do I put this picture in a terminal", each with its own tradeoffs
//! (see their module docs). A CLI that wants to expose all three — auto-pick
//! the best one, or let the user override it — ends up re-implementing the
//! same picker every time it does so (see `rich-cli`'s `--diff` command).
//!
//! [`ImageArt`] is that picker, lifted out so it can be reused: it owns the
//! decoded image once, wraps [`ImageOptions`] (mode, width, height, colour),
//! and dispatches to whichever backend the resolved [`ImageMode`] names.
//! By default sizing and alpha handling are delegated to the backing renderer.
//! Optional fitting and background compositing preprocess the image consistently
//! across backends before dispatch.
//!
//! `Sixel` is only available when this crate's `sixel` feature is enabled.
//! Selecting it explicitly without that feature — or when a terminal can't
//! actually encode it — is reported as an [`ImageArtError`], not silently
//! swapped for something else: [`ImageArt::render`] is the strict entry point
//! for callers (such as a CLI) that want to know why and say so. The
//! [`Renderable`] impl, which cannot fail, falls back to ASCII in that case
//! since ASCII has no requirements beyond this module's own `image` feature.
//! Invalid fit dimensions produce no segments through this infallible trait;
//! use [`ImageArt::render`] to receive the validation error.

use std::sync::Arc;

use image::{imageops::FilterType, DynamicImage, GenericImageView, Rgb, RgbImage, Rgba, RgbaImage};

use rich::console::{Console, ConsoleOptions};
use rich::protocol::{ConsoleEnvironment, RenderEnvironment, Renderable, Support};
use rich::segment::Segment;

use crate::ascii::AsciiArt;
use crate::block::BlockArt;
use crate::braille::BrailleArt;
use crate::quadrant::QuadrantArt;
use crate::{ColorDistance, Dither, ImageColorMode};

/// Which backend renders the image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageMode {
    /// Pick a backend from what the destination can actually do: ASCII
    /// without colour, half-blocks with colour, or Sixel where it is both
    /// compiled in and looks supported. See [`ImageArt::resolve_mode`].
    #[default]
    Auto,
    /// A character ramp (`jp2a`-style). No colour required.
    Ascii,
    /// Half-block characters — full colour, twice ASCII's vertical detail.
    Blocks,
    /// Unicode Braille cells, two pixels wide by four pixels high.
    Braille,
    /// Unicode quadrant blocks, two by two pixels in two colours per cell.
    Quadrants,
    /// Real pixels via the Sixel graphics protocol. Requires this crate's
    /// `sixel` feature.
    Sixel,
}

/// How an image fills an explicit rectangle of terminal cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFit {
    /// Preserve the entire image, centering it with background padding.
    Contain,
    /// Fill the rectangle, cropping around the selected [`ImageAnchor`].
    Cover,
    /// Fill the rectangle exactly, ignoring the aspect ratio.
    Stretch,
    /// Render at the image's own size: its pixels mapped to cells at the
    /// backend's density (see [`ImageArt::native_grid`]), rounded up to whole
    /// cells. Each pixel lands on one sub-cell pixel, unscaled, and the rest
    /// of the last cell takes the background. Needs no rectangle; the console
    /// width, `width`/`height` and [`ImageArt::max_width`]/
    /// [`ImageArt::max_height`] only ever shrink it, as a [`Contain`] fit
    /// into the capped grid. Never enlarges a small image to fill the width.
    ///
    /// [`Contain`]: ImageFit::Contain
    Native,
}

/// Which part of the image to retain when using [`ImageFit::Cover`].
///
/// Edge anchors center the other axis. Contain fitting and unfitted images
/// ignore this setting. Center crops round down when the excess is odd.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageAnchor {
    /// Keep the center (the default).
    #[default]
    Center,
    /// Keep the top edge, centered horizontally.
    Top,
    /// Keep the bottom edge, centered horizontally.
    Bottom,
    /// Keep the left edge, centered vertically.
    Left,
    /// Keep the right edge, centered vertically.
    Right,
    /// Keep the top-left corner.
    TopLeft,
    /// Keep the top-right corner.
    TopRight,
    /// Keep the bottom-left corner.
    BottomLeft,
    /// Keep the bottom-right corner.
    BottomRight,
}

/// What transparent pixels turn into.
///
/// [`ImageArt::background`] sets [`Color`](Self::Color); the other two keep
/// the image's alpha through fitting instead of flattening it first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageBackground {
    /// Composite onto this RGB colour before resizing. Contain padding takes
    /// the same colour.
    Color([u8; 3]),
    /// Leave transparent cells unpainted so the terminal's own background
    /// shows through: a pixel under half opacity after sampling is a blank
    /// (ASCII), a half or quadrant left out of the glyph (blocks, quadrants),
    /// no dot (Braille) or a transparent Sixel pixel. Pixels at or over half
    /// opacity show their own colour. Contain padding is transparent too.
    TerminalDefault,
    /// Composite onto a two-tone gray checkerboard (#999999 and #666666), the
    /// image-editor convention for showing where an image is transparent.
    /// With fitting, each square is two cells wide and one cell tall, so it
    /// looks square; without, squares are a sixteenth of the image's longer
    /// side and scale with it.
    Checkerboard,
}

/// Light and dark squares of [`ImageBackground::Checkerboard`].
const CHECKER: [[u8; 3]; 2] = [[153, 153, 153], [102, 102, 102]];

/// Composite `image` onto a checkerboard of `square` (width, height) pixels.
fn checkerboard(image: &RgbaImage, square: (u32, u32)) -> RgbImage {
    RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let under = CHECKER[((x / square.0 + y / square.1) % 2) as usize];
        let rgba = image.get_pixel(x, y).0;
        let alpha = u32::from(rgba[3]);
        Rgb(std::array::from_fn(|c| {
            ((u32::from(rgba[c]) * alpha + u32::from(under[c]) * (255 - alpha) + 127) / 255) as u8
        }))
    })
}

/// The most cells any character backend ([`AsciiArt`], [`BlockArt`],
/// [`QuadrantArt`], [`BrailleArt`]) renders: 2^20, about 1024×1024. At up to
/// eight sampled pixels per cell the resampled raster stays well inside
/// [`sixel::MAX_PIXELS`](crate::sixel::MAX_PIXELS)'s 16 megapixels.
///
/// A derived size is bounded before any resampling: a tiny, very tall image
/// would otherwise ask for millions of rows. Its columns are narrowed to keep
/// the aspect ratio (to one column at least) rather than failing.
pub const MAX_CELLS: usize = 1 << 20;

/// Check an explicit `width` × `height` request against [`MAX_CELLS`]; a
/// missing side counts as one cell. Strict entry points (such as
/// [`ImageArt::render`]) report [`ImageArtError::TooLarge`] from this; the
/// infallible renderables clamp to the budget instead.
pub fn check_cell_budget(width: Option<usize>, height: Option<usize>) -> Result<(), ImageArtError> {
    let cells = width.unwrap_or(1).max(1).checked_mul(height.unwrap_or(1).max(1));
    match cells {
        Some(cells) if cells <= MAX_CELLS => Ok(()),
        _ => Err(ImageArtError::TooLarge),
    }
}

/// Bound a backend's `(columns, rows)` grid to [`MAX_CELLS`] before it is
/// resampled.
///
/// `rows_derived` says the rows came from the image's aspect ratio (not an
/// exact request): they are then also capped by `max_rows` (the console's
/// `options.height`, when it fixes one) and shrunk together with the columns,
/// keeping the aspect ratio. Exact rows are kept (up to the budget) and only
/// the columns narrow.
pub(crate) fn bound_grid(
    columns: usize,
    rows: usize,
    rows_derived: bool,
    max_rows: Option<usize>,
) -> (usize, usize) {
    let (mut columns, mut rows) = (columns.clamp(1, MAX_CELLS), rows.max(1));
    if rows_derived {
        let cap = max_rows.unwrap_or(usize::MAX).clamp(1, MAX_CELLS);
        if rows > cap {
            let factor = cap as f64 / rows as f64;
            columns = ((columns as f64 * factor).round() as usize).max(1);
            rows = cap;
        }
        if columns.saturating_mul(rows) > MAX_CELLS {
            let factor = (MAX_CELLS as f64 / (columns as f64 * rows as f64)).sqrt();
            columns = ((columns as f64 * factor).floor() as usize).max(1);
            rows = ((rows as f64 * factor).floor() as usize).max(1);
        }
        // Whatever rounding did, the product stays inside the budget.
        rows = rows.min(MAX_CELLS / columns);
    } else {
        rows = rows.min(MAX_CELLS);
        columns = columns.min(MAX_CELLS / rows);
    }
    (columns, rows)
}

/// Which pixels of a sampled raster count as transparent (under half
/// opacity), or `None` when transparency is not being kept.
///
/// When it is kept, every other pixel is made fully opaque in place: a pixel
/// at or over half opacity shows its own colour, so nothing downstream
/// (palette quantization, luminance, thresholds) may darken it by its alpha.
/// Without it the raster is left untouched.
pub(crate) fn clear_mask(raster: &mut RgbaImage, keep: bool) -> Option<Vec<bool>> {
    keep.then(|| {
        raster
            .pixels_mut()
            .map(|p| {
                let clear = p.0[3] < 128;
                if !clear {
                    p.0[3] = 255;
                }
                clear
            })
            .collect()
    })
}

/// What the destination can actually do, used only to resolve
/// [`ImageMode::Auto`]. Explicit modes ignore this entirely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RenderCapabilities {
    /// The console has a colour system available (and colour hasn't been
    /// disabled, e.g. via `NO_COLOR` or `--no-color`).
    pub color: bool,
    /// Sixel looks supported: a real terminal, and (heuristically) one that
    /// understands the escape sequence. See
    /// [`sixel::is_probably_supported`](crate::sixel::is_probably_supported)
    /// for why this can only ever be a guess.
    pub sixel_supported: bool,
}

impl RenderCapabilities {
    /// Read capabilities off a [`Console`]: colour from its colour system,
    /// and (with the `sixel` feature) Sixel support from the same heuristic
    /// `rich-cli` uses.
    pub fn from_console(console: &Console) -> Self {
        RenderCapabilities {
            // No-colour mode keeps the colour system (as upstream does) but
            // strips every colour on output, so it counts as no colour here.
            color: console.color_system().is_some() && !console.no_color(),
            sixel_supported: Self::sixel_supported(console),
        }
    }

    #[cfg(feature = "sixel")]
    fn sixel_supported(console: &Console) -> bool {
        console.is_terminal() && crate::sixel::is_probably_supported()
    }

    #[cfg(not(feature = "sixel"))]
    fn sixel_supported(_console: &Console) -> bool {
        false
    }
}

/// Sizing and behaviour shared across every backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageOptions {
    /// Which backend to use, or [`ImageMode::Auto`] to pick one.
    pub mode: ImageMode,
    /// Render this many columns (default: the console width).
    pub width: Option<usize>,
    /// Render this many rows (default: derived from the image, per-backend).
    pub height: Option<usize>,
    /// Colour ASCII cells with the sampled pixel (ignored by Blocks and
    /// Sixel, which are always in colour). See
    /// [`AsciiArt::color`](crate::ascii::AsciiArt::color).
    pub color: bool,
}

impl Default for ImageOptions {
    fn default() -> Self {
        ImageOptions {
            mode: ImageMode::Auto,
            width: None,
            height: None,
            color: false,
        }
    }
}

/// Why an [`ImageArt`] could not be rendered as requested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageArtError {
    /// An explicit mode was chosen that this build cannot honour.
    FeatureNotEnabled {
        mode: ImageMode,
        /// The Cargo feature that would enable it.
        feature: &'static str,
    },
    /// Sixel encoding failed for this image/size. The image itself is fine —
    /// this is an encoder limitation, not a decode error.
    SixelEncodeFailed,
    /// Sixel output requires a real terminal destination.
    NonTerminalDestination,
    /// The destination is a terminal, but not one known to understand Sixel.
    /// Setting `RICH_SIXEL=1` or `RICH_GRAPHICS=sixel` overrides the guess
    /// (see [`sixel::is_probably_supported`](crate::sixel::is_probably_supported)).
    SixelNotSupported,
    /// The Sixel raster for this width and height would exceed
    /// [`sixel::MAX_PIXELS`](crate::sixel::MAX_PIXELS) (16 megapixels, at
    /// 8×16 pixels per cell) or its size overflows. A narrower width or a
    /// height cap brings it back in range.
    SixelTooLarge,
    /// Fitting requires positive width and height, a nonempty image and
    /// destination, and raster canvases no larger than 16 megapixels.
    InvalidFitDimensions,
    /// A reduced colour mode was asked of Braille, which draws monochrome
    /// dots and has no colours to quantize; or dithering or a colour distance
    /// was set without a reduced colour mode.
    UnsupportedColorOptions,
    /// Brightness or contrast is negative or not finite, or gamma is not a
    /// finite positive number. See [`ImageTransforms`](crate::ImageTransforms).
    InvalidAdjustment,
    /// An explicit width and height ask for more than [`MAX_CELLS`] cells.
    /// Derived sizes never fail this way: a backend narrows its columns to
    /// keep the aspect ratio inside the budget instead.
    TooLarge,
}

impl std::fmt::Display for ImageArtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedColorOptions => write!(f, "Braille images are monochrome, so they take no color mode; dithering and color distance require ansi256, ansi16 or grayscale"),
            Self::InvalidAdjustment => write!(f, "image brightness and contrast must be finite and non-negative, and gamma finite and positive"),
            Self::FeatureNotEnabled { mode, feature } => write!(
                f,
                "image mode {mode:?} is not available in this build; \
                 rebuild rs-rich-art with the {feature:?} feature enabled"
            ),
            Self::SixelEncodeFailed => {
                write!(f, "could not encode this image as Sixel graphics")
            }
            Self::InvalidFitDimensions => {
                write!(f, "image fit requires positive width and height, a nonempty image and destination, and at most 16 megapixels")
            }
            Self::NonTerminalDestination => {
                write!(f, "Sixel graphics require a terminal destination; use ASCII, Braille, or blocks when redirecting output")
            }
            Self::SixelTooLarge => {
                write!(f, "the Sixel image would exceed 16 megapixels at this size; set a smaller width or a height")
            }
            Self::TooLarge => {
                write!(f, "the image would exceed {MAX_CELLS} cells at this size; set a smaller width or height")
            }
            Self::SixelNotSupported => {
                write!(f, "this terminal is not known to support Sixel graphics; set RICH_SIXEL=1 or RICH_GRAPHICS=sixel to force it, or use ASCII, Braille, blocks or quadrants")
            }
        }
    }
}

impl std::error::Error for ImageArtError {}

/// An image renderable through any of ASCII, half-block, or Sixel backends,
/// picked automatically or pinned explicitly.
///
/// ```no_run
/// use rich::Console;
/// use rich_art::{ImageArt, ImageMode};
///
/// let console = Console::builder().build();
/// let art = ImageArt::from_path("photo.png")
///     .expect("decode")
///     .mode(ImageMode::Auto)
///     .width(60);
/// console.print(&art);
/// ```
pub struct ImageArt {
    image: Arc<DynamicImage>,
    options: ImageOptions,
    fit: Option<ImageFit>,
    anchor: ImageAnchor,
    background: Option<ImageBackground>,
    color_mode: ImageColorMode,
    dither: Dither,
    color_distance: ColorDistance,
    transforms: crate::ImageTransforms,
    max_width: Option<usize>,
    max_height: Option<usize>,
}

impl ImageArt {
    /// Build from an already-decoded image, with [`ImageMode::Auto`] and no
    /// explicit size.
    pub fn new(image: DynamicImage) -> Self {
        Self::from_shared(Arc::new(image))
    }

    pub(crate) fn from_shared(image: Arc<DynamicImage>) -> Self {
        ImageArt {
            image,
            options: ImageOptions::default(),
            fit: None,
            anchor: ImageAnchor::default(),
            background: None,
            color_mode: ImageColorMode::default(),
            dither: Dither::default(),
            color_distance: ColorDistance::default(),
            transforms: crate::ImageTransforms::default(),
            max_width: None,
            max_height: None,
        }
    }

    /// Decode an image from bytes (PNG, JPEG, GIF, ...).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, image::ImageError> {
        Ok(ImageArt::new(image::load_from_memory(bytes)?))
    }

    /// Decode an image from a file.
    pub fn from_path(path: impl AsRef<std::path::Path>) -> Result<Self, image::ImageError> {
        Ok(ImageArt::new(image::open(path)?))
    }

    /// Replace backend options, preserving fit, anchor, background and colour processing.
    pub fn options(mut self, options: ImageOptions) -> Self {
        self.options = options;
        self
    }

    /// Pin an explicit backend, or [`ImageMode::Auto`] to pick one at render
    /// time.
    pub fn mode(mut self, mode: ImageMode) -> Self {
        self.options.mode = mode;
        self
    }

    /// Render this many columns instead of the console's width.
    pub fn width(mut self, width: usize) -> Self {
        self.options.width = Some(width);
        self
    }

    /// Render this many rows instead of the backend's default.
    pub fn height(mut self, height: usize) -> Self {
        self.options.height = Some(height);
        self
    }

    /// Fit into an explicit width and height (or, with [`ImageFit::Native`],
    /// size to the image itself), assuming cells are twice as
    /// tall as they are wide. Width is clamped to the console's available
    /// width. Invalid dimensions are reported by [`Self::render`]. The
    /// output and cover intermediate rasters are limited to 16 megapixels
    /// each (Sixel: 8×16 per cell). Extreme aspect ratios may exceed this limit
    /// even when the final rectangle fits.
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = Some(fit);
        self
    }

    /// Select the part retained by [`ImageFit::Cover`] (default: center).
    /// Has no effect with [`ImageFit::Contain`] or without fitting.
    pub fn anchor(mut self, anchor: ImageAnchor) -> Self {
        self.anchor = anchor;
        self
    }

    /// Composite transparency over this RGB colour before resizing. Also
    /// colours contain padding; fitting without a background uses black.
    /// Without fitting or a background, backend alpha handling is unchanged.
    /// Shorthand for [`Self::background_mode`] with [`ImageBackground::Color`].
    pub fn background(mut self, background: [u8; 3]) -> Self {
        self.background = Some(ImageBackground::Color(background));
        self
    }

    /// Choose what transparent pixels become: a colour, the terminal's own
    /// background, or a checkerboard. See [`ImageBackground`].
    pub fn background_mode(mut self, background: ImageBackground) -> Self {
        self.background = Some(background);
        self
    }

    /// Colour ASCII cells with the sampled pixel. No effect on Blocks or
    /// Sixel, which are always in colour.
    pub fn color(mut self, color: bool) -> Self {
        self.options.color = color;
        self
    }

    /// Select truecolor (default) or a reduced palette (ANSI256, ANSI16 or
    /// grayscale) for the ASCII, half-block, quadrant and Sixel backends.
    /// Processing occurs after fit/background handling and final sampling,
    /// before ASCII luminance normalization and glyph selection. Sixel then
    /// encodes exactly the palette colours instead of adaptive ones. Braille
    /// is monochrome and rejects a reduced palette.
    pub fn color_mode(mut self, mode: ImageColorMode) -> Self {
        self.color_mode = mode;
        self
    }

    /// Select optional dithering (Floyd–Steinberg, Bayer 4×4 or Atkinson).
    /// It needs a reduced palette; unsupported combinations are errors from
    /// [`Self::render`].
    pub fn dither(mut self, dither: Dither) -> Self {
        self.dither = dither;
        self
    }

    /// Measure the nearest palette colour in encoded RGB (the default) or in
    /// perceptual OKLab. It needs a reduced palette, like dithering.
    pub fn color_distance(mut self, distance: ColorDistance) -> Self {
        self.color_distance = distance;
        self
    }

    /// Apply still-image transforms before fitting, sampling and quantization.
    pub fn transforms(mut self, transforms: crate::ImageTransforms) -> Self {
        self.transforms = transforms;
        self
    }

    /// Never render wider than this many columns, whatever the requested or
    /// available width. Fitting clamps its rectangle to the cap too.
    pub fn max_width(mut self, columns: usize) -> Self {
        self.max_width = Some(columns);
        self
    }

    /// Never render taller than this many rows. Without fitting the image
    /// keeps its aspect ratio and narrows to fit; fitting clamps its rectangle.
    pub fn max_height(mut self, rows: usize) -> Self {
        self.max_height = Some(rows);
        self
    }

    /// Whether backends should leave transparent cells unpainted.
    fn keeps_transparency(&self) -> bool {
        self.background == Some(ImageBackground::TerminalDefault)
    }

    /// The requested row count after the `max_height` cap.
    fn rows(&self) -> Option<usize> {
        match (self.options.height, self.max_height) {
            (Some(h), Some(cap)) => Some(h.min(cap)),
            (h, cap) => h.or(cap),
        }
    }

    /// Resolve [`ImageMode::Auto`] into a concrete backend given what the
    /// destination can do. An explicit mode is returned unchanged — this
    /// picker never overrides the caller.
    pub fn resolve_mode(&self, capabilities: RenderCapabilities) -> ImageMode {
        match self.options.mode {
            ImageMode::Auto => {
                if !capabilities.color {
                    ImageMode::Ascii
                } else if capabilities.sixel_supported {
                    ImageMode::Sixel
                } else {
                    ImageMode::Blocks
                }
            }
            explicit => explicit,
        }
    }

    /// Render for a console, resolving [`ImageMode::Auto`] from its actual
    /// capabilities ([`RenderCapabilities::from_console`]).
    ///
    /// This is the strict entry point: an explicit mode this build or this
    /// destination cannot honour is reported as an [`ImageArtError`] rather
    /// than silently swapped for a fallback. Callers that want a fallback
    /// (e.g. a CLI downgrading Sixel to Blocks on stderr) can match on the
    /// error and retry with a different [`ImageMode`].
    pub fn render(
        &self,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        if let Some(environment) = console.render_environment() {
            return self.render_with_environment(console, options, environment);
        }
        let capabilities = RenderCapabilities::from_console(console);
        let mode = self.resolve_mode(capabilities);
        self.render_as(mode, console, options)
    }

    /// Render using supplied capabilities without environment detection.
    pub fn render_with_environment(
        &self,
        console: &Console,
        options: &ConsoleOptions,
        environment: &dyn RenderEnvironment,
    ) -> Result<Vec<Segment>, ImageArtError> {
        let caps = environment.capabilities();
        if caps.width == 0
            || caps.height == 0
            || options.max_width == 0
            || options.height == Some(0)
        {
            return Ok(Vec::new());
        }
        if self.options.mode == ImageMode::Sixel {
            if !caps.interactive {
                return Err(ImageArtError::NonTerminalDestination);
            }
            if caps.sixel == Support::Unsupported {
                return Err(ImageArtError::SixelNotSupported);
            }
        }
        let mode = if self.options.mode == ImageMode::Auto && !caps.unicode {
            ImageMode::Ascii
        } else {
            self.resolve_mode(RenderCapabilities {
                color: caps.color_system.is_some(),
                sixel_supported: caps.interactive && caps.sixel != Support::Unsupported,
            })
        };
        self.render_as(mode, console, options)
    }

    fn prepare_image(
        &self,
        mode: ImageMode,
        available: usize,
    ) -> Result<Arc<DynamicImage>, ImageArtError> {
        // Terminal-default and checkerboard backgrounds keep alpha through
        // fitting; a colour (or none) flattens first, as it always has.
        let keep_alpha = matches!(
            self.background,
            Some(ImageBackground::TerminalDefault | ImageBackground::Checkerboard)
        );
        let color = match self.background {
            Some(ImageBackground::Color(color)) => color,
            _ => [0, 0, 0],
        };
        let (image, background) = if self.transforms == crate::ImageTransforms::default() {
            (Arc::clone(&self.image), color)
        } else {
            let (image, background) = crate::transform::prepare(
                &self.image,
                self.transforms,
                (!keep_alpha).then_some(color),
            );
            (Arc::new(image), background)
        };
        // Native sizing is a rectangle of its own: the image's own pixels in
        // whole cells, placed unscaled; or, when a cap shrinks it, a contain
        // fit into the capped grid.
        let native = self.fit == Some(ImageFit::Native);
        let (native_grid, native_capped) = if native {
            let own = self.native_cells(mode);
            let grid = self.native_grid(mode, available);
            (Some(grid), grid != own)
        } else {
            (None, false)
        };
        let fit = match self.fit {
            Some(ImageFit::Native) if native_capped => Some(ImageFit::Contain),
            Some(ImageFit::Native) => None,
            other => other,
        };
        if fit.is_none() && self.background.is_none() && !native {
            return Ok(image);
        }
        let target = if fit.is_some() || native {
            let (columns, rows) = match native_grid {
                Some(grid) => grid,
                None => (
                    self.options
                        .width
                        .ok_or(ImageArtError::InvalidFitDimensions)?
                        .min(available)
                        .min(self.max_width.unwrap_or(usize::MAX)),
                    self.options
                        .height
                        .ok_or(ImageArtError::InvalidFitDimensions)?
                        .min(self.max_height.unwrap_or(usize::MAX)),
                ),
            };
            let (sx, sy) = cell_pixels(mode);
            let width = columns.checked_mul(sx).and_then(|v| u32::try_from(v).ok());
            let height = rows.checked_mul(sy).and_then(|v| u32::try_from(v).ok());
            match (width, height) {
                (Some(w), Some(h))
                    if w > 0
                        && h > 0
                        && u64::from(w) * u64::from(h) <= 16 * 1024 * 1024
                        && image.width() > 0
                        && image.height() > 0 =>
                {
                    Some((w, h))
                }
                _ => return Err(ImageArtError::InvalidFitDimensions),
            }
        } else {
            None
        };
        let source = if keep_alpha {
            DynamicImage::ImageRgba8(image.to_rgba8())
        } else {
            let mut flattened = RgbImage::new(image.width(), image.height());
            for (x, y, pixel) in flattened.enumerate_pixels_mut() {
                let rgba = image.get_pixel(x, y).0;
                let alpha = u32::from(rgba[3]);
                for channel in 0..3 {
                    pixel.0[channel] = ((u32::from(rgba[channel]) * alpha
                        + u32::from(background[channel]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
            DynamicImage::ImageRgb8(flattened)
        };
        let checkered = self.background == Some(ImageBackground::Checkerboard);
        let Some((width, height)) = target else {
            if checkered {
                let side = source.width().max(source.height()).div_ceil(16).max(1);
                let flat = checkerboard(&source.to_rgba8(), (side, side));
                return Ok(Arc::new(DynamicImage::ImageRgb8(flat)));
            }
            return Ok(Arc::new(source));
        };
        let Some(fit) = fit else {
            // Native at the image's own size: every pixel lands on exactly one
            // sub-cell pixel; the last partial cell takes the background.
            let result = if keep_alpha {
                let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
                image::imageops::replace(&mut canvas, &source.to_rgba8(), 0, 0);
                DynamicImage::ImageRgba8(canvas)
            } else {
                let mut canvas = RgbImage::from_pixel(width, height, Rgb(background));
                image::imageops::replace(&mut canvas, &source.to_rgb8(), 0, 0);
                DynamicImage::ImageRgb8(canvas)
            };
            return Ok(Arc::new(self.checkered(result, mode)));
        };
        let result = match fit {
            ImageFit::Native => unreachable!("native sizing resolves to a fit or a canvas"),
            ImageFit::Stretch => source.resize_exact(width, height, FilterType::Triangle),
            ImageFit::Contain if keep_alpha => {
                let fitted = source
                    .resize(width, height, FilterType::Triangle)
                    .to_rgba8();
                let mut canvas = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
                image::imageops::replace(
                    &mut canvas,
                    &fitted,
                    i64::from((width - fitted.width()) / 2),
                    i64::from((height - fitted.height()) / 2),
                );
                DynamicImage::ImageRgba8(canvas)
            }
            ImageFit::Contain => {
                let fitted = source.resize(width, height, FilterType::Triangle).to_rgb8();
                let mut canvas = RgbImage::from_pixel(width, height, Rgb(background));
                image::imageops::replace(
                    &mut canvas,
                    &fitted,
                    i64::from((width - fitted.width()) / 2),
                    i64::from((height - fitted.height()) / 2),
                );
                DynamicImage::ImageRgb8(canvas)
            }
            ImageFit::Cover => {
                // Resize proportionally before cropping: integer source crops
                // would discard subpixel detail and distort very small images.
                let (sw, sh) = source.dimensions();
                let (rw, rh) =
                    if u64::from(sw) * u64::from(height) > u64::from(sh) * u64::from(width) {
                        (
                            (u64::from(sw) * u64::from(height)).div_ceil(u64::from(sh)),
                            u64::from(height),
                        )
                    } else {
                        (
                            u64::from(width),
                            (u64::from(sh) * u64::from(width)).div_ceil(u64::from(sw)),
                        )
                    };
                if !rw
                    .checked_mul(rh)
                    .is_some_and(|pixels| pixels <= 16 * 1024 * 1024)
                {
                    return Err(ImageArtError::InvalidFitDimensions);
                }
                // The positive pixel-count bound also guarantees u32 dimensions.
                let (rw, rh) = (rw as u32, rh as u32);
                let x = match self.anchor {
                    ImageAnchor::Left | ImageAnchor::TopLeft | ImageAnchor::BottomLeft => 0,
                    ImageAnchor::Right | ImageAnchor::TopRight | ImageAnchor::BottomRight => {
                        rw - width
                    }
                    _ => (rw - width) / 2,
                };
                let y = match self.anchor {
                    ImageAnchor::Top | ImageAnchor::TopLeft | ImageAnchor::TopRight => 0,
                    ImageAnchor::Bottom | ImageAnchor::BottomLeft | ImageAnchor::BottomRight => {
                        rh - height
                    }
                    _ => (rh - height) / 2,
                };
                source
                    .resize_exact(rw, rh, FilterType::Triangle)
                    .crop_imm(x, y, width, height)
            }
        };
        Ok(Arc::new(self.checkered(result, mode)))
    }

    /// Under a checkerboard background, flatten a fitted raster onto squares
    /// two cells wide by one tall: square on screen.
    fn checkered(&self, result: DynamicImage, mode: ImageMode) -> DynamicImage {
        if self.background != Some(ImageBackground::Checkerboard) {
            return result;
        }
        let (sx, sy) = cell_pixels(mode);
        DynamicImage::ImageRgb8(checkerboard(&result.to_rgba8(), (2 * sx as u32, sy as u32)))
    }

    /// Render with an explicit, already-resolved mode (no `Auto` handling).
    fn render_as(
        &self,
        mode: ImageMode,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        let reduced = self.color_mode != ImageColorMode::TrueColor;
        let tuned = self.dither != Dither::None || self.color_distance != ColorDistance::Rgb;
        if (tuned && !reduced) || (reduced && mode == ImageMode::Braille) {
            return Err(ImageArtError::UnsupportedColorOptions);
        }
        if !self.transforms.adjustments_valid() {
            return Err(ImageArtError::InvalidAdjustment);
        }
        let image = self.prepare_image(mode, options.max_width)?;
        let native = self.fit == Some(ImageFit::Native);
        let (width, rows) = if native {
            let (columns, rows) = self.native_grid(mode, options.max_width);
            (columns, Some(rows))
        } else {
            let width = self.options.width.unwrap_or(options.max_width);
            let width = if self.fit.is_some() {
                width.min(options.max_width)
            } else {
                width
            };
            (width.min(self.max_width.unwrap_or(usize::MAX)), self.rows())
        };
        match mode {
            ImageMode::Auto => {
                // `render` always resolves Auto before dispatching here, but
                // a caller invoking this privately would otherwise loop.
                self.render_as(
                    self.resolve_mode(RenderCapabilities::from_console(console)),
                    console,
                    options,
                )
            }
            ImageMode::Ascii => {
                let mut art = AsciiArt::from_shared(Arc::clone(&image))
                    .width(width)
                    .keep_transparency(self.keeps_transparency())
                    .color(self.options.color)
                    .color_processing(self.color_mode, self.dither, self.color_distance);
                match (self.options.height, self.max_height) {
                    _ if native => art = art.height(rows.expect("native sets rows")),
                    (Some(_), _) => art = art.height(self.rows().expect("height is set")),
                    // ASCII's height is an exact row count, not a cap, so an
                    // unfitted max height narrows the width to keep the aspect.
                    (None, Some(cap)) => {
                        let (columns, rows) = art.grid(options.max_width);
                        if rows > cap.max(1) {
                            let cap = cap.max(1);
                            let columns = ((columns * cap) as f64 / rows as f64).round() as usize;
                            art = art.width(columns.max(1)).height(cap);
                        }
                    }
                    (None, None) => {}
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Blocks => {
                let mut art = BlockArt::from_shared(Arc::clone(&image))
                    .width(width)
                    .keep_transparency(self.keeps_transparency())
                    .color_processing(self.color_mode, self.dither, self.color_distance);
                if let Some(height) = rows {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Quadrants => {
                let mut art = QuadrantArt::from_shared(Arc::clone(&image))
                    .width(width)
                    .keep_transparency(self.keeps_transparency())
                    .color_processing(self.color_mode, self.dither, self.color_distance);
                if let Some(height) = rows {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Braille => {
                let mut art = BrailleArt::from_shared(Arc::clone(&image))
                    .width(width)
                    .keep_transparency(self.keeps_transparency());
                if let Some(height) = rows {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Sixel => self.render_sixel(image, width, rows, console, options),
        }
    }

    #[cfg(feature = "sixel")]
    fn render_sixel(
        &self,
        image: Arc<DynamicImage>,
        width: usize,
        rows: Option<usize>,
        console: &Console,
        _options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        use crate::sixel::SixelArt;

        if !console.is_terminal() {
            return Err(ImageArtError::NonTerminalDestination);
        }
        let mut art = SixelArt::new((*image).clone())
            .width(width)
            .keep_transparency(self.keeps_transparency())
            .color_processing(self.color_mode, self.dither, self.color_distance);
        if let Some(height) = rows {
            art = art.height(height);
        }
        if art.checked_pixel_size(width).is_none() {
            return Err(ImageArtError::SixelTooLarge);
        }
        // Encode once and emit it exactly as `SixelArt`'s renderable does
        // (with the width pinned, the console options cannot change it).
        match art.encode(width) {
            Some(sixel) => Ok(vec![Segment::control(sixel), Segment::line()]),
            None => Err(ImageArtError::SixelEncodeFailed),
        }
    }

    #[cfg(not(feature = "sixel"))]
    fn render_sixel(
        &self,
        _image: Arc<DynamicImage>,
        _width: usize,
        _rows: Option<usize>,
        _console: &Console,
        _options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        Err(ImageArtError::FeatureNotEnabled {
            mode: ImageMode::Sixel,
            feature: "sixel",
        })
    }
}

impl ImageArt {
    /// The cell grid [`ImageFit::Native`] renders `mode` at: the image's
    /// pixels at the backend's density, rounded up to whole cells. ASCII and
    /// half-blocks take 1×2 pixels per cell, quadrants and Braille 2×4, and
    /// Sixel 8×16 (its assumed cell size). Every mode keeps the image's
    /// aspect ratio on a cell twice as tall as it is wide.
    ///
    /// The grid is then capped, keeping the aspect ratio, by `available`
    /// columns, by `width`/`height` when set, and by
    /// [`max_width`](Self::max_width)/[`max_height`](Self::max_height). It is
    /// at least one cell each way.
    ///
    /// `mode` should be a resolved backend; [`ImageMode::Auto`] counts as
    /// half-blocks. A quarter-turn [`Rotation`](crate::Rotation) swaps the
    /// image's width and height first.
    pub fn native_grid(&self, mode: ImageMode, available: usize) -> (usize, usize) {
        let (columns, rows) = self.native_cells(mode);
        let max_columns = available
            .min(self.options.width.unwrap_or(usize::MAX))
            .min(self.max_width.unwrap_or(usize::MAX))
            .max(1);
        let max_rows = self
            .options
            .height
            .unwrap_or(usize::MAX)
            .min(self.max_height.unwrap_or(usize::MAX))
            .max(1);
        if columns <= max_columns && rows <= max_rows {
            return (columns, rows);
        }
        let scale = (max_columns as f64 / columns as f64).min(max_rows as f64 / rows as f64);
        let shrink =
            |cells: usize, cap: usize| ((cells as f64 * scale).round() as usize).clamp(1, cap);
        (shrink(columns, max_columns), shrink(rows, max_rows))
    }

    /// The uncapped native grid: the image's pixels (after a quarter-turn)
    /// in whole cells.
    fn native_cells(&self, mode: ImageMode) -> (usize, usize) {
        let (w, h) = self.image.dimensions();
        let (w, h) = match self.transforms.rotation {
            crate::Rotation::Clockwise90 | crate::Rotation::Clockwise270 => (h, w),
            _ => (w, h),
        };
        let (sx, sy) = cell_pixels(mode);
        (
            (w as usize).div_ceil(sx).max(1),
            (h as usize).div_ceil(sy).max(1),
        )
    }
}

/// Raster pixels per terminal cell when fitting for `mode`: square fitting
/// pixels, except that quadrants resample 2×4 down to 2×2.
fn cell_pixels(mode: ImageMode) -> (usize, usize) {
    match mode {
        ImageMode::Braille | ImageMode::Quadrants => (2, 4),
        ImageMode::Sixel => (8, 16),
        _ => (1, 2),
    }
}

impl Renderable for ImageArt {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.render(console, options).unwrap_or_else(|_| {
            // `Renderable` cannot fail, so degrade to the one backend every
            // build of this module can honour. Callers that need to know
            // *why* Sixel (or another explicit mode) was unavailable should
            // call `render` directly instead of going through this trait.
            self.render_as(ImageMode::Ascii, console, options)
                .unwrap_or_default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};
    use rich::color::ColorSystem;

    fn solid(width: u32, height: u32, rgb: [u8; 3]) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb(rgb)))
    }

    fn console(color: bool) -> Console {
        Console::builder()
            .force_terminal(true)
            .color_system(if color {
                Some(ColorSystem::Truecolor)
            } else {
                None
            })
            .width(8)
            .no_color(!color)
            .build()
    }

    const ANCHORS: [(ImageAnchor, u32, u32); 9] = [
        (ImageAnchor::Center, 1, 1),
        (ImageAnchor::Top, 1, 0),
        (ImageAnchor::Bottom, 1, 3),
        (ImageAnchor::Left, 0, 1),
        (ImageAnchor::Right, 3, 1),
        (ImageAnchor::TopLeft, 0, 0),
        (ImageAnchor::TopRight, 3, 0),
        (ImageAnchor::BottomLeft, 0, 3),
        (ImageAnchor::BottomRight, 3, 3),
    ];

    fn asymmetric(width: u32, height: u32) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            Rgb([(x * 31) as u8, (y * 37) as u8, (x + y * width) as u8])
        }))
    }

    #[test]
    fn all_cover_anchors_select_the_expected_source_pixels() {
        // No resampling: a 4x4 crop loses three columns or rows. This also
        // checks that center retains the existing floor rounding for odd excess.
        for (sw, sh) in [(7, 4), (4, 7)] {
            let source = asymmetric(sw, sh);
            for (anchor, x_offset, y_offset) in ANCHORS {
                let pixels = ImageArt::new(source.clone())
                    .width(4)
                    .height(2)
                    .fit(ImageFit::Cover)
                    .anchor(anchor)
                    .prepare_image(ImageMode::Blocks, 8)
                    .unwrap()
                    .to_rgb8();
                assert_eq!(pixels.dimensions(), (4, 4));
                for (x, y, pixel) in pixels.enumerate_pixels() {
                    let sx = x + if sw > 4 { x_offset } else { 0 };
                    let sy = y + if sh > 4 { y_offset } else { 0 };
                    assert_eq!(
                        pixel.0,
                        source.to_rgb8().get_pixel(sx, sy).0,
                        "{anchor:?}, source {sw}x{sh}, pixel ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn anchor_defaults_to_center_and_options_preserve_preprocessing_settings() {
        let source = asymmetric(7, 4);
        let default = ImageArt::new(source.clone())
            .width(4)
            .height(2)
            .fit(ImageFit::Cover);
        assert_eq!(ImageAnchor::default(), ImageAnchor::Center);
        assert_eq!(default.anchor, ImageAnchor::Center);
        let explicit = ImageArt::new(source)
            .anchor(ImageAnchor::Center)
            .fit(ImageFit::Cover)
            .background([12, 34, 56])
            .options(ImageOptions {
                width: Some(4),
                height: Some(2),
                ..ImageOptions::default()
            });
        assert_eq!(explicit.fit, Some(ImageFit::Cover));
        assert_eq!(
            explicit.background,
            Some(ImageBackground::Color([12, 34, 56]))
        );
        assert_eq!(
            default
                .prepare_image(ImageMode::Blocks, 8)
                .unwrap()
                .to_rgb8(),
            explicit
                .prepare_image(ImageMode::Blocks, 8)
                .unwrap()
                .to_rgb8()
        );
        let right = explicit.anchor(ImageAnchor::Right).options(ImageOptions {
            width: Some(4),
            height: Some(2),
            ..ImageOptions::default()
        });
        assert_eq!(right.anchor, ImageAnchor::Right);
        assert_eq!(
            right
                .prepare_image(ImageMode::Blocks, 8)
                .unwrap()
                .to_rgb8()
                .get_pixel(0, 0)
                .0,
            [93, 0, 3]
        );
    }

    #[test]
    fn anchors_leave_contain_padding_and_unfitted_images_unchanged() {
        for (sw, sh) in [(7, 4), (4, 7)] {
            let source = asymmetric(sw, sh);
            let baseline = ImageArt::new(source.clone())
                .width(4)
                .height(2)
                .fit(ImageFit::Contain)
                .background([12, 34, 56])
                .prepare_image(ImageMode::Blocks, 8)
                .unwrap()
                .to_rgb8();
            for (anchor, _, _) in ANCHORS {
                let art = ImageArt::new(source.clone()).anchor(anchor);
                assert!(Arc::ptr_eq(
                    &art.image,
                    &art.prepare_image(ImageMode::Blocks, 8).unwrap()
                ));
                let contained = art
                    .width(4)
                    .height(2)
                    .fit(ImageFit::Contain)
                    .background([12, 34, 56])
                    .prepare_image(ImageMode::Blocks, 8)
                    .unwrap()
                    .to_rgb8();
                assert_eq!(contained, baseline, "{anchor:?}");
            }
        }
    }

    #[test]
    fn contain_letterboxes_and_cover_crops_the_center() {
        let mut source = RgbImage::from_pixel(8, 4, Rgb([255, 0, 0]));
        for y in 0..4 {
            for x in 2..6 {
                source.put_pixel(x, y, Rgb([0, 255, 0]));
            }
        }
        let contain = ImageArt::new(DynamicImage::ImageRgb8(source.clone()))
            .width(4)
            .height(2)
            .fit(ImageFit::Contain);
        let pixels = contain
            .prepare_image(ImageMode::Blocks, 8)
            .unwrap()
            .to_rgb8();
        assert_eq!(pixels.dimensions(), (4, 4));
        assert_eq!(pixels.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(pixels.get_pixel(0, 3).0, [0, 0, 0]);
        assert!(pixels.get_pixel(0, 1).0[0] > 200);
        assert!(pixels.get_pixel(2, 1).0[1] > 200);
        let cover = ImageArt::new(DynamicImage::ImageRgb8(source))
            .width(4)
            .height(2)
            .fit(ImageFit::Cover);
        let pixels = cover.prepare_image(ImageMode::Blocks, 8).unwrap().to_rgb8();
        assert_eq!(pixels.dimensions(), (4, 4));
        assert!(pixels.pixels().all(|p| p.0 == [0, 255, 0]));
    }

    #[test]
    fn cover_preserves_subpixel_detail_in_small_sources() {
        let mut source = RgbImage::from_pixel(2, 2, Rgb([255, 0, 0]));
        for x in 0..2 {
            source.put_pixel(x, 1, Rgb([0, 0, 255]));
        }
        let art = ImageArt::new(DynamicImage::ImageRgb8(source))
            .width(3)
            .height(1)
            .fit(ImageFit::Cover);
        let pixels = art.prepare_image(ImageMode::Blocks, 8).unwrap().to_rgb8();
        assert_eq!(pixels.dimensions(), (3, 2));
        // Proportional upscaling retains both source rows in the centre sample;
        // cropping to an integer source row first loses blue altogether.
        let middle = pixels.get_pixel(1, 1).0;
        assert!(middle[0] > 100 && middle[2] > 100, "{middle:?}");
    }

    #[test]
    fn cover_rejects_an_oversized_intermediate_before_allocating() {
        let art = ImageArt::new(solid(1, 32, [0, 0, 0]))
            .width(10_000)
            .height(1)
            .fit(ImageFit::Cover);
        assert!(matches!(
            art.prepare_image(ImageMode::Blocks, 10_000),
            Err(ImageArtError::InvalidFitDimensions)
        ));
    }

    #[test]
    fn background_blends_alpha_before_resizing() {
        let mut source = image::RgbaImage::new(3, 1);
        source.put_pixel(0, 0, image::Rgba([200, 100, 0, 128]));
        source.put_pixel(1, 0, image::Rgba([255, 0, 255, 0]));
        source.put_pixel(2, 0, image::Rgba([1, 2, 3, 255]));
        let art = ImageArt::new(DynamicImage::ImageRgba8(source)).background([20, 40, 60]);
        let pixels = art.prepare_image(ImageMode::Blocks, 8).unwrap().to_rgb8();
        assert_eq!(pixels.get_pixel(0, 0).0, [110, 70, 30]);
        assert_eq!(pixels.get_pixel(1, 0).0, [20, 40, 60]);
        assert_eq!(pixels.get_pixel(2, 0).0, [1, 2, 3]);
        let source = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 255, 0]));
        let art = ImageArt::new(DynamicImage::ImageRgba8(source))
            .background([20, 40, 60])
            .fit(ImageFit::Contain)
            .width(4)
            .height(1);
        assert!(art
            .prepare_image(ImageMode::Blocks, 8)
            .unwrap()
            .to_rgb8()
            .pixels()
            .all(|p| p.0 == [20, 40, 60]));
    }

    /// Columns and lines of a text-mode render.
    fn rendered_grid(art: &ImageArt, width: usize) -> (usize, usize) {
        let console = Console::builder()
            .force_terminal(true)
            .color_system(Some(ColorSystem::Truecolor))
            .width(width)
            .build();
        let text: String = art
            .render(&console, &console.options())
            .unwrap()
            .iter()
            .filter(|s| !s.control)
            .map(|s| s.text.clone())
            .collect();
        let lines: Vec<&str> = text.lines().collect();
        let columns = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        (columns, lines.len())
    }

    const TEXT_MODES: [ImageMode; 4] = [
        ImageMode::Ascii,
        ImageMode::Blocks,
        ImageMode::Quadrants,
        ImageMode::Braille,
    ];

    #[test]
    fn native_renders_the_images_own_pixels_per_mode() {
        // (image size, [ascii, blocks, quadrants, braille, sixel] grids)
        let cases = [
            ((8, 8), [(8, 4), (8, 4), (4, 2), (4, 2), (1, 1)]),
            // Odd sizes round up to whole cells.
            ((7, 5), [(7, 3), (7, 3), (4, 2), (4, 2), (1, 1)]),
            ((1, 1), [(1, 1), (1, 1), (1, 1), (1, 1), (1, 1)]),
            ((17, 33), [(17, 17), (17, 17), (9, 9), (9, 9), (3, 3)]),
        ];
        for ((w, h), grids) in cases {
            let modes = [
                ImageMode::Ascii,
                ImageMode::Blocks,
                ImageMode::Quadrants,
                ImageMode::Braille,
                ImageMode::Sixel,
            ];
            for (mode, grid) in modes.into_iter().zip(grids) {
                let art = ImageArt::new(solid(w, h, [200, 50, 50]))
                    .mode(mode)
                    .fit(ImageFit::Native);
                assert_eq!(art.native_grid(mode, 100), grid, "{w}x{h} {mode:?}");
                if mode != ImageMode::Sixel {
                    assert_eq!(rendered_grid(&art, 100), grid, "{w}x{h} {mode:?} rendered");
                }
            }
        }
    }

    #[test]
    fn native_never_enlarges_but_the_default_fills_the_width() {
        let tiny = || ImageArt::new(solid(8, 8, [0, 0, 255])).mode(ImageMode::Blocks);
        assert_eq!(rendered_grid(&tiny(), 100), (100, 50));
        assert_eq!(rendered_grid(&tiny().fit(ImageFit::Native), 100), (8, 4));
    }

    #[test]
    fn native_is_capped_by_the_console_and_max_sizes_keeping_the_aspect() {
        let wide = || {
            ImageArt::new(solid(80, 40, [0, 200, 0]))
                .mode(ImageMode::Blocks)
                .fit(ImageFit::Native)
        };
        // Own size: 80×20 cells.
        assert_eq!(rendered_grid(&wide(), 100), (80, 20));
        // The console width halves it.
        assert_eq!(rendered_grid(&wide(), 40), (40, 10));
        assert_eq!(rendered_grid(&wide().max_width(20), 100), (20, 5));
        assert_eq!(rendered_grid(&wide().max_height(5), 100), (20, 5));
        // `width`/`height` cap too; the tighter cap wins.
        assert_eq!(
            rendered_grid(&wide().width(60).max_height(10), 100),
            (40, 10)
        );
        for mode in TEXT_MODES {
            let art = ImageArt::new(solid(80, 40, [0, 200, 0]))
                .mode(mode)
                .fit(ImageFit::Native)
                .max_width(10);
            let (columns, rows) = rendered_grid(&art, 100);
            assert_eq!(columns, 10, "{mode:?}");
            assert_eq!((columns, rows), art.native_grid(mode, 100), "{mode:?}");
        }
    }

    #[test]
    fn native_maps_pixels_one_to_one_and_pads_the_last_cell() {
        // A 3×3 image under quadrants (2×4 pixels a cell): 2×1 cells, the
        // last pixel column and row padded with the background.
        let mut source = RgbImage::from_pixel(3, 3, Rgb([0, 0, 0]));
        source.put_pixel(1, 1, Rgb([255, 255, 255]));
        let art = ImageArt::new(DynamicImage::ImageRgb8(source))
            .mode(ImageMode::Quadrants)
            .fit(ImageFit::Native)
            .background([9, 9, 9]);
        let raster = art
            .prepare_image(ImageMode::Quadrants, 100)
            .unwrap()
            .to_rgb8();
        assert_eq!(raster.dimensions(), (4, 4));
        assert_eq!(raster.get_pixel(1, 1).0, [255, 255, 255]);
        assert_eq!(raster.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(raster.get_pixel(3, 0).0, [9, 9, 9]);
        assert_eq!(raster.get_pixel(0, 3).0, [9, 9, 9]);
    }

    #[test]
    fn native_follows_a_quarter_turn() {
        let art = ImageArt::new(solid(8, 2, [1, 2, 3]))
            .mode(ImageMode::Blocks)
            .fit(ImageFit::Native)
            .transforms(crate::ImageTransforms {
                rotation: crate::Rotation::Clockwise90,
                ..Default::default()
            });
        assert_eq!(art.native_grid(ImageMode::Blocks, 100), (2, 4));
        assert_eq!(rendered_grid(&art, 100), (2, 4));
    }

    #[test]
    fn native_combines_with_alpha_backgrounds() {
        let mut source = image::RgbaImage::from_pixel(3, 3, image::Rgba([0, 0, 0, 0]));
        source.put_pixel(0, 0, image::Rgba([255, 0, 0, 255]));
        let image = DynamicImage::ImageRgba8(source);
        // Terminal default: padding and transparent pixels stay transparent.
        let art = ImageArt::new(image.clone())
            .mode(ImageMode::Blocks)
            .fit(ImageFit::Native)
            .background_mode(ImageBackground::TerminalDefault);
        let raster = art
            .prepare_image(ImageMode::Blocks, 100)
            .unwrap()
            .to_rgba8();
        assert_eq!(raster.dimensions(), (3, 4));
        assert_eq!(raster.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(raster.get_pixel(2, 3).0[3], 0);
        assert_eq!(rendered_grid(&art, 100), (3, 2));
        // A colour composites transparency and fills the padding.
        let art = ImageArt::new(image.clone())
            .mode(ImageMode::Blocks)
            .fit(ImageFit::Native)
            .background([0, 0, 255]);
        let raster = art.prepare_image(ImageMode::Blocks, 100).unwrap().to_rgb8();
        assert_eq!(raster.get_pixel(1, 1).0, [0, 0, 255]);
        assert_eq!(raster.get_pixel(2, 3).0, [0, 0, 255]);
        // A checkerboard covers the whole grid.
        let art = ImageArt::new(image)
            .mode(ImageMode::Blocks)
            .fit(ImageFit::Native)
            .background_mode(ImageBackground::Checkerboard);
        let raster = art.prepare_image(ImageMode::Blocks, 100).unwrap().to_rgb8();
        assert_eq!(raster.dimensions(), (3, 4));
        assert_eq!(rendered_grid(&art, 100), (3, 2));
    }

    #[test]
    fn fitted_text_modes_fill_the_bounded_cell_grid() {
        let console = console(false);
        for mode in [ImageMode::Ascii, ImageMode::Blocks, ImageMode::Braille] {
            for fit in [ImageFit::Contain, ImageFit::Cover] {
                let art = ImageArt::new(solid(10, 30, [255, 255, 255]))
                    .mode(mode)
                    .width(20)
                    .height(3)
                    .fit(fit);
                let segments = art.render(&console, &console.options()).unwrap();
                let text: String = segments.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(text.lines().count(), 3, "{mode:?} {fit:?}");
                assert!(
                    text.lines().all(|line| line.chars().count() == 8),
                    "{mode:?} {fit:?}"
                );
            }
        }
    }

    #[test]
    fn invalid_fit_dimensions_return_an_error_without_panicking_in_renderable() {
        let console = console(false);
        for (width, height) in [
            (None, Some(2)),
            (Some(2), None),
            (Some(0), Some(2)),
            (Some(2), Some(0)),
            (Some(2), Some(usize::MAX)),
            (Some(2), Some(9_000_000)),
        ] {
            let art = ImageArt::new(solid(4, 4, [0, 0, 0]))
                .options(ImageOptions {
                    width,
                    height,
                    ..ImageOptions::default()
                })
                .fit(ImageFit::Contain);
            assert_eq!(
                art.render(&console, &console.options()),
                Err(ImageArtError::InvalidFitDimensions)
            );
            assert!(art.rich_render(&console, &console.options()).is_empty());
        }
    }

    #[test]
    fn fit_rejects_empty_source_or_destination() {
        let art = ImageArt::new(solid(4, 4, [0, 0, 0]))
            .width(4)
            .height(2)
            .fit(ImageFit::Contain);
        assert!(matches!(
            art.prepare_image(ImageMode::Blocks, 0),
            Err(ImageArtError::InvalidFitDimensions)
        ));
        let art = ImageArt::new(solid(0, 0, [0, 0, 0]))
            .width(4)
            .height(2)
            .fit(ImageFit::Cover);
        assert!(matches!(
            art.prepare_image(ImageMode::Blocks, 8),
            Err(ImageArtError::InvalidFitDimensions)
        ));
    }

    #[test]
    fn background_reaches_the_rendered_cells() {
        let console = console(true);
        let source = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 255, 0]));
        let art = ImageArt::new(DynamicImage::ImageRgba8(source))
            .background([20, 40, 60])
            .mode(ImageMode::Blocks)
            .width(2);
        let out = console.render_to_string(&art);
        assert!(out.contains("38;2;20;40;60") && out.contains("48;2;20;40;60"));
    }

    #[cfg(feature = "sixel")]
    #[test]
    fn fitted_sixel_uses_pixel_geometry_for_the_bounded_rectangle() {
        let art = ImageArt::new(solid(16, 16, [255, 255, 255]))
            .mode(ImageMode::Sixel)
            .width(4)
            .height(2)
            .fit(ImageFit::Contain);
        let pixels = art.prepare_image(ImageMode::Sixel, 8).unwrap();
        assert_eq!(pixels.dimensions(), (32, 32));
        let console = console(true);
        let segments = art.render(&console, &console.options()).unwrap();
        let text: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert!(
            text.contains(";32;32"),
            "expected Sixel raster dimensions, got {text:?}"
        );
    }

    #[test]
    fn default_preprocessing_preserves_the_original_shared_image() {
        let art = ImageArt::new(solid(4, 4, [5, 10, 20]));
        let prepared = art.prepare_image(ImageMode::Blocks, 8).unwrap();
        assert!(Arc::ptr_eq(&prepared, &art.image));
    }

    #[test]
    fn auto_picks_ascii_without_colour() {
        let art = ImageArt::new(solid(4, 4, [1, 2, 3]));
        let caps = RenderCapabilities {
            color: false,
            sixel_supported: true, // must be ignored: no colour wins first
        };
        assert_eq!(art.resolve_mode(caps), ImageMode::Ascii);
    }

    #[test]
    fn auto_picks_blocks_with_colour_but_no_sixel() {
        let art = ImageArt::new(solid(4, 4, [1, 2, 3]));
        let caps = RenderCapabilities {
            color: true,
            sixel_supported: false,
        };
        assert_eq!(art.resolve_mode(caps), ImageMode::Blocks);
    }

    #[test]
    fn auto_picks_sixel_when_supported() {
        let art = ImageArt::new(solid(4, 4, [1, 2, 3]));
        let caps = RenderCapabilities {
            color: true,
            sixel_supported: true,
        };
        assert_eq!(art.resolve_mode(caps), ImageMode::Sixel);
    }

    #[test]
    fn an_explicit_mode_is_never_overridden() {
        let art = ImageArt::new(solid(4, 4, [1, 2, 3])).mode(ImageMode::Ascii);
        let caps = RenderCapabilities {
            color: true,
            sixel_supported: true,
        };
        assert_eq!(art.resolve_mode(caps), ImageMode::Ascii);
    }

    #[test]
    fn render_dispatches_to_ascii() {
        let console = console(false);
        let options = console.options();
        let art = ImageArt::new(solid(4, 4, [0, 0, 0]))
            .mode(ImageMode::Ascii)
            .width(4)
            .height(1);
        let segments = art.render(&console, &options).expect("ascii always works");
        let text: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text.chars().count(), 4);
    }

    #[test]
    fn render_dispatches_to_blocks_with_full_colour_cells() {
        let console = console(true);
        let art = ImageArt::new(solid(2, 2, [200, 10, 10]))
            .mode(ImageMode::Blocks)
            .width(2);
        let out = console.render_to_string(&art);
        assert!(
            out.contains("38;2;200;10;10") && out.contains("48;2;200;10;10"),
            "expected a fully-painted red cell, got:\n{out}"
        );
    }

    #[test]
    fn width_and_height_are_forwarded_to_the_backend() {
        let console = console(false);
        let options = console.options();
        let art = ImageArt::new(solid(20, 20, [9, 9, 9]))
            .mode(ImageMode::Ascii)
            .width(5)
            .height(3);
        let segments = art.render(&console, &options).unwrap();
        let text: String = segments.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text.lines().count(), 3);
        assert_eq!(text.lines().next().unwrap().chars().count(), 5);
    }

    #[test]
    fn ascii_color_option_is_delegated_to_ascii_art() {
        let console = console(true);
        let art = ImageArt::new(solid(2, 2, [0, 255, 0]))
            .mode(ImageMode::Ascii)
            .width(2)
            .height(1)
            .color(true);
        let out = console.render_to_string(&art);
        assert!(
            out.contains("38;2;0;255;0"),
            "expected a green fg, got:\n{out}"
        );
    }

    #[cfg(not(feature = "sixel"))]
    #[test]
    fn explicit_sixel_without_the_feature_is_a_clear_error() {
        let console = console(true);
        let options = console.options();
        let art = ImageArt::new(solid(4, 4, [1, 2, 3])).mode(ImageMode::Sixel);
        let err = art.render(&console, &options).unwrap_err();
        assert_eq!(
            err,
            ImageArtError::FeatureNotEnabled {
                mode: ImageMode::Sixel,
                feature: "sixel",
            }
        );
        assert!(err.to_string().contains("sixel"));
    }

    #[cfg(not(feature = "sixel"))]
    #[test]
    fn the_renderable_impl_falls_back_to_ascii_when_sixel_is_unavailable() {
        let console = console(true);
        let art = ImageArt::new(solid(4, 4, [220, 220, 220]))
            .mode(ImageMode::Sixel)
            .width(4)
            .height(1);
        // Must not panic, and must produce plain ASCII (a bright glyph, not
        // Sixel escape data) rather than nothing.
        let out = console.render_to_string(&art);
        assert_eq!(out.chars().count(), 4, "expected a 4-column ASCII fallback");
        assert!(
            !out.contains('\u{1b}'),
            "must not contain a Sixel escape sequence"
        );
    }

    #[cfg(feature = "sixel")]
    #[test]
    fn explicit_sixel_encodes_through_the_renderable_impl() {
        let console = console(true);
        let art = ImageArt::new(solid(32, 32, [10, 20, 30]))
            .mode(ImageMode::Sixel)
            .width(8);
        let out = console.render_to_string(&art);
        assert!(
            out.contains('q'),
            "expected a Sixel DCS selector, got:\n{out}"
        );
    }

    #[cfg(feature = "sixel")]
    #[test]
    fn explicit_sixel_rejects_non_terminal_destinations() {
        let console = Console::builder().force_terminal(false).width(8).build();
        let options = console.options();
        let art = ImageArt::new(solid(8, 8, [10, 20, 30])).mode(ImageMode::Sixel);
        assert_eq!(
            art.render(&console, &options),
            Err(ImageArtError::NonTerminalDestination)
        );
    }

    #[cfg(feature = "sixel")]
    #[test]
    fn explicit_sixel_refuses_rasters_over_the_cap() {
        // 1x400 at 80 columns: 640 x 256 000 Sixel pixels.
        let console = Console::builder().force_terminal(true).width(80).build();
        let options = console.options();
        let art = ImageArt::new(solid(1, 400, [10, 20, 30]))
            .mode(ImageMode::Sixel)
            .width(80);
        assert_eq!(
            art.render(&console, &options),
            Err(ImageArtError::SixelTooLarge)
        );
        // A height cap brings it back in range.
        let capped = ImageArt::new(solid(1, 400, [10, 20, 30]))
            .mode(ImageMode::Sixel)
            .width(80)
            .height(4);
        assert!(capped.render(&console, &options).is_ok());
    }

    #[test]
    fn explicit_sixel_tells_redirection_from_an_unrecognised_terminal() {
        use rich::protocol::{RenderEnvironment, Support, TargetCapabilities};
        struct Target(bool, Support);
        impl RenderEnvironment for Target {
            fn capabilities(&self) -> TargetCapabilities {
                TargetCapabilities {
                    width: 8,
                    height: 8,
                    color_system: Some(ColorSystem::Truecolor),
                    interactive: self.0,
                    unicode: true,
                    hyperlinks: false,
                    sixel: self.1,
                }
            }
        }
        let console = console(true);
        let options = console.options();
        let art = ImageArt::new(solid(8, 8, [10, 20, 30])).mode(ImageMode::Sixel);
        let render = |interactive, sixel| {
            art.render_with_environment(&console, &options, &Target(interactive, sixel))
        };
        assert_eq!(
            render(false, Support::Inferred),
            Err(ImageArtError::NonTerminalDestination)
        );
        // A real terminal that just is not recognised is not "redirected".
        let error = render(true, Support::Unsupported).unwrap_err();
        assert_eq!(error, ImageArtError::SixelNotSupported);
        let message = error.to_string();
        assert!(!message.contains("redirect"), "{message}");
        assert!(message.contains("RICH_SIXEL=1"), "{message}");
        assert!(message.contains("RICH_GRAPHICS=sixel"), "{message}");
    }
}

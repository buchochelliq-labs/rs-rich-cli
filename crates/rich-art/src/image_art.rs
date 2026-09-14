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
//! Sizing and colour/alpha behaviour are never reimplemented here — they are
//! delegated entirely to the backing renderer, so this module's only job is
//! choosing *which* renderer runs and reporting when that choice cannot be
//! honoured.
//!
//! `Sixel` is only available when this crate's `sixel` feature is enabled.
//! Selecting it explicitly without that feature — or when a terminal can't
//! actually encode it — is reported as an [`ImageArtError`], not silently
//! swapped for something else: [`ImageArt::render`] is the strict entry point
//! for callers (such as a CLI) that want to know why and say so. The
//! [`Renderable`] impl, which cannot fail, falls back to ASCII in that case
//! since ASCII has no requirements beyond this module's own `image` feature.

use std::sync::Arc;

use image::DynamicImage;

use rich::console::{Console, ConsoleOptions};
use rich::protocol::Renderable;
use rich::segment::Segment;

use crate::ascii::AsciiArt;
use crate::block::BlockArt;
use crate::braille::BrailleArt;

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
    /// Real pixels via the Sixel graphics protocol. Requires this crate's
    /// `sixel` feature.
    Sixel,
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
            color: console.color_system().is_some(),
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
}

impl std::fmt::Display for ImageArtError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FeatureNotEnabled { mode, feature } => write!(
                f,
                "image mode {mode:?} is not available in this build; \
                 rebuild rs-rich-art with the {feature:?} feature enabled"
            ),
            Self::SixelEncodeFailed => {
                write!(f, "could not encode this image as Sixel graphics")
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

    /// Replace every option at once.
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

    /// Colour ASCII cells with the sampled pixel. No effect on Blocks or
    /// Sixel, which are always in colour.
    pub fn color(mut self, color: bool) -> Self {
        self.options.color = color;
        self
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
        let capabilities = RenderCapabilities::from_console(console);
        let mode = self.resolve_mode(capabilities);
        self.render_as(mode, console, options)
    }

    /// Render with an explicit, already-resolved mode (no `Auto` handling).
    fn render_as(
        &self,
        mode: ImageMode,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        let width = self.options.width.unwrap_or(options.max_width);
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
                let mut art = AsciiArt::from_shared(Arc::clone(&self.image))
                    .width(width)
                    .color(self.options.color);
                if let Some(height) = self.options.height {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Blocks => {
                let mut art = BlockArt::from_shared(Arc::clone(&self.image)).width(width);
                if let Some(height) = self.options.height {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Braille => {
                let mut art = BrailleArt::from_shared(Arc::clone(&self.image)).width(width);
                if let Some(height) = self.options.height {
                    art = art.height(height);
                }
                Ok(art.rich_render(console, options))
            }
            ImageMode::Sixel => self.render_sixel(width, console, options),
        }
    }

    #[cfg(feature = "sixel")]
    fn render_sixel(
        &self,
        width: usize,
        console: &Console,
        options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        use crate::sixel::SixelArt;

        let mut art = SixelArt::new((*self.image).clone()).width(width);
        if let Some(height) = self.options.height {
            art = art.height(height);
        }
        if art.encode(width).is_none() {
            return Err(ImageArtError::SixelEncodeFailed);
        }
        Ok(art.rich_render(console, options))
    }

    #[cfg(not(feature = "sixel"))]
    fn render_sixel(
        &self,
        _width: usize,
        _console: &Console,
        _options: &ConsoleOptions,
    ) -> Result<Vec<Segment>, ImageArtError> {
        Err(ImageArtError::FeatureNotEnabled {
            mode: ImageMode::Sixel,
            feature: "sixel",
        })
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
                .expect("ASCII rendering never fails")
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
}

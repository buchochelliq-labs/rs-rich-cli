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

/// An image rendered as Sixel graphics.
pub struct SixelArt {
    image: DynamicImage,
    /// Target width in terminal columns; pixels are derived from it.
    columns: Option<usize>,
    max_rows: Option<usize>,
    cell_px: (u32, u32),
    max_colors: u16,
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

    /// Target size in **pixels** for the given available width in columns.
    fn pixel_size(&self, available: usize) -> (u32, u32) {
        let (iw, ih) = self.image.dimensions();
        if iw == 0 || ih == 0 {
            return (1, 1);
        }
        let columns = self.columns.unwrap_or(available).max(1) as u32;
        let mut px_w = columns * self.cell_px.0;
        let mut px_h = ((u64::from(px_w) * u64::from(ih)) / u64::from(iw)) as u32;

        if let Some(rows) = self.max_rows {
            let cap = (rows.max(1) as u32) * self.cell_px.1;
            if px_h > cap {
                px_w = ((u64::from(px_w) * u64::from(cap)) / u64::from(px_h)) as u32;
                px_h = cap;
            }
        }
        (px_w.max(1), px_h.max(1))
    }

    /// The Sixel escape sequence for this image, or `None` if encoding failed.
    ///
    /// Failure is not worth propagating to a renderable: the caller has already
    /// chosen Sixel, and the useful response is to fall back to blocks.
    pub fn encode(&self, available: usize) -> Option<String> {
        let (w, h) = self.pixel_size(available);
        let scaled = self
            .image
            .resize_exact(w, h, FilterType::Lanczos3)
            .to_rgba8();
        let opts = icy_sixel::EncodeOptions {
            max_colors: self.max_colors,
            ..Default::default()
        };
        icy_sixel::sixel_encode(scaled.as_raw(), w as usize, h as usize, &opts).ok()
    }
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
pub fn is_probably_supported() -> bool {
    guess_support(
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
    override_var: Option<&str>,
    windows_terminal: bool,
    term: Option<&str>,
    term_program: Option<&str>,
) -> bool {
    // Explicit opt-out/opt-in first: whatever we guess, the user wins.
    match override_var {
        Some("0") | Some("false") | Some("no") => return false,
        Some("1") | Some("true") | Some("yes") => return true,
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

    #[test]
    fn width_in_columns_becomes_width_in_pixels() {
        let art = SixelArt::new(solid(100, 50)).width(40).cell_px(8, 16);
        // 40 columns * 8 px, and the height follows the 2:1 aspect ratio.
        assert_eq!(art.pixel_size(80), (320, 160));
    }

    #[test]
    fn a_row_cap_shrinks_both_dimensions() {
        let art = SixelArt::new(solid(100, 400))
            .width(40)
            .height(10)
            .cell_px(8, 16);
        let (w, h) = art.pixel_size(80);
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
        assert!(!guess_support(Some("0"), true, None, None));
        // And an explicit "yes" must win in a terminal we would otherwise
        // assume knows nothing about Sixel.
        assert!(guess_support(Some("1"), false, Some("dumb"), None));
    }

    #[test]
    fn recognises_terminals_that_support_sixel() {
        assert!(guess_support(None, true, None, None), "Windows Terminal");
        assert!(guess_support(None, false, Some("foot"), None), "foot");
        assert!(guess_support(None, false, Some("mlterm"), None), "mlterm");
        assert!(guess_support(None, false, None, Some("WezTerm")), "WezTerm");
    }

    #[test]
    fn assumes_no_support_when_nothing_says_otherwise() {
        assert!(!guess_support(None, false, Some("xterm-256color"), None));
        assert!(!guess_support(None, false, None, None));
    }
}

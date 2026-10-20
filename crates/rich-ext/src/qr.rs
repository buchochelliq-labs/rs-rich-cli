//! QR codes drawn with text: [`QrCode`] encodes text or bytes and draws
//! the modules two to a cell with half blocks, inside a quiet zone, so a
//! phone can read it off the terminal.
//!
//! Needs the `qr` feature. The encoder is Project Nayuki's
//! [`qrcodegen`](https://crates.io/crates/qrcodegen) (MIT, no
//! dependencies).
//!
//! ```
//! use rich::Console;
//! use rich_ext::qr::{ErrorCorrection, QrCode};
//!
//! let code = QrCode::with_error_correction("hello", ErrorCorrection::Low).unwrap();
//! assert_eq!(code.size(), 21);
//! let console = Console::builder().width(80).color_system(None).build();
//! let out = console.render_export(&code.quiet_zone(1));
//! // 21 modules and a module of quiet zone each side, two rows a line.
//! assert_eq!(out.lines().count(), 12);
//! assert!(out.lines().all(|line| line.chars().count() == 23));
//! ```

use std::fmt;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style};

use crate::chart::{has_colour, user_style, Charset};

/// The quiet zone the QR specification asks for, in modules.
const SPEC_QUIET_ZONE: usize = 4;

/// How much of a [`QrCode`] can be damaged and still read: more
/// correction makes a bigger code for the same data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ErrorCorrection {
    /// About 7% of the code.
    Low,
    /// About 15% (the default).
    #[default]
    Medium,
    /// About 25%.
    Quartile,
    /// About 30%.
    High,
}

impl ErrorCorrection {
    fn ecc(self) -> qrcodegen::QrCodeEcc {
        match self {
            ErrorCorrection::Low => qrcodegen::QrCodeEcc::Low,
            ErrorCorrection::Medium => qrcodegen::QrCodeEcc::Medium,
            ErrorCorrection::Quartile => qrcodegen::QrCodeEcc::Quartile,
            ErrorCorrection::High => qrcodegen::QrCodeEcc::High,
        }
    }

    fn from_ecc(ecc: qrcodegen::QrCodeEcc) -> Self {
        match ecc {
            qrcodegen::QrCodeEcc::Low => ErrorCorrection::Low,
            qrcodegen::QrCodeEcc::Medium => ErrorCorrection::Medium,
            qrcodegen::QrCodeEcc::Quartile => ErrorCorrection::Quartile,
            qrcodegen::QrCodeEcc::High => ErrorCorrection::High,
        }
    }

    /// The level as a word: `low`, `medium`, `quartile`, `high`.
    pub fn word(self) -> &'static str {
        match self {
            ErrorCorrection::Low => "low",
            ErrorCorrection::Medium => "medium",
            ErrorCorrection::Quartile => "quartile",
            ErrorCorrection::High => "high",
        }
    }
}

/// Why a [`QrCode`] could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum QrError {
    /// The data does not fit the largest code (version 40) at the error
    /// correction asked for.
    TooLong {
        /// The data's length in bytes.
        bytes: usize,
    },
}

impl fmt::Display for QrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QrError::TooLong { bytes } => write!(
                f,
                "{bytes} bytes do not fit a QR code at this error correction"
            ),
        }
    }
}

impl std::error::Error for QrError {}

/// A QR code as a renderable.
///
/// - [`Charset::Blocks`] (the default; [`Charset::Braille`] draws the same,
///   as Braille dots are too sparse to scan) draws two modules a cell with
///   half blocks (`▀▄█`), so the code is square and `size + 2 × quiet
///   zone` cells wide.
/// - With colour, every cell is painted: the dark modules in `#000000` and
///   the light ones in `#ffffff`, whatever the terminal's theme makes of its
///   named colours ([`colours`](Self::colours) changes
///   them), so the code reads on a dark or a light terminal.
/// - Without colour, the glyphs draw the light modules in the terminal's
///   text colour, so on a dark background (the common case) the code reads
///   dark on light. [`invert`](Self::invert) draws the dark modules
///   instead, for a terminal with a light background.
/// - [`Charset::Ascii`] (and an ASCII-only console) draws each module as
///   two cells, `##` or spaces, or as painted spaces with colour, so the
///   code is twice as tall.
/// - The quiet zone is 4 modules, as the specification asks
///   ([`quiet_zone`](Self::quiet_zone)). Given less width, the quiet zone
///   narrows to a module; a code that still does not fit is replaced by a
///   note saying how many columns it needs, as a cut code cannot be read.
///
/// ```
/// use rich::Console;
/// use rich_ext::qr::{ErrorCorrection, QrCode};
///
/// let code = QrCode::new("https://example.com").unwrap();
/// assert_eq!((code.size(), code.version()), (25, 2));
/// // Medium was asked for; quartile fits the same size, so it is used.
/// assert_eq!(code.error_correction(), ErrorCorrection::Quartile);
/// // 25 modules and 4 of quiet zone each side.
/// let wide = Console::builder().width(40).color_system(None).build();
/// assert!(wide.render_export(&code).lines().all(|l| l.chars().count() == 33));
/// // The quiet zone narrows to fit, down to a module.
/// let narrow = Console::builder().width(28).color_system(None).build();
/// assert!(narrow.render_export(&code).lines().all(|l| l.chars().count() == 27));
/// // Narrower still, a note.
/// let tiny = Console::builder().width(26).color_system(None).build();
/// assert_eq!(tiny.render_export(&code), "QR code needs 27 columns\n");
/// ```
#[derive(Clone, Debug, PartialEq)]
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct QrCode {
    modules: Vec<bool>,
    size: usize,
    version: u8,
    ecc: ErrorCorrection,
    text: Option<String>,
    quiet: usize,
    charset: Charset,
    invert: bool,
    dark: String,
    light: String,
}

impl QrCode {
    /// `text` at [`ErrorCorrection::Medium`].
    pub fn new(text: &str) -> Result<Self, QrError> {
        Self::with_error_correction(text, ErrorCorrection::Medium)
    }

    /// `text` at `ecc`, or higher when the larger level fits the same
    /// size of code. Digits and upper-case text are packed tighter than
    /// other text.
    pub fn with_error_correction(text: &str, ecc: ErrorCorrection) -> Result<Self, QrError> {
        let code = qrcodegen::QrCode::encode_text(text, ecc.ecc())
            .map_err(|_| QrError::TooLong { bytes: text.len() })?;
        Ok(Self::from_code(&code, Some(text.to_string())))
    }

    /// Bytes at `ecc` (or higher, as [`with_error_correction`](Self::with_error_correction)).
    pub fn from_bytes(data: &[u8], ecc: ErrorCorrection) -> Result<Self, QrError> {
        let code = qrcodegen::QrCode::encode_binary(data, ecc.ecc())
            .map_err(|_| QrError::TooLong { bytes: data.len() })?;
        let text = std::str::from_utf8(data).ok().map(str::to_string);
        Ok(Self::from_code(&code, text))
    }

    fn from_code(code: &qrcodegen::QrCode, text: Option<String>) -> Self {
        let size = usize::try_from(code.size()).unwrap_or(0);
        let mut modules = Vec::with_capacity(size * size);
        for y in 0..code.size() {
            for x in 0..code.size() {
                modules.push(code.get_module(x, y));
            }
        }
        QrCode {
            modules,
            size,
            version: code.version().value(),
            ecc: ErrorCorrection::from_ecc(code.error_correction_level()),
            text,
            quiet: SPEC_QUIET_ZONE,
            charset: Charset::Auto,
            invert: false,
            dark: "#000000".to_string(),
            light: "#ffffff".to_string(),
        }
    }

    /// Modules along a side, without the quiet zone (21 to 177).
    pub fn size(&self) -> usize {
        self.size
    }

    /// The QR version, 1 to 40: the size is `17 + 4 × version`.
    pub fn version(&self) -> u8 {
        self.version
    }

    /// The error correction the code was made with (it may be higher than
    /// asked for).
    pub fn error_correction(&self) -> ErrorCorrection {
        self.ecc
    }

    /// Whether the module at column `x`, row `y` is dark; `false` outside
    /// the code.
    pub fn module(&self, x: usize, y: usize) -> bool {
        x < self.size && y < self.size && self.modules[y * self.size + x]
    }

    /// The light border around the code, in modules (default 4, as the
    /// specification asks; scanners often manage with 2).
    pub fn quiet_zone(mut self, modules: usize) -> Self {
        self.quiet = modules.min(SPEC_QUIET_ZONE * 4);
        self
    }

    /// Glyphs to draw with (default [`Charset::Auto`]: half blocks).
    pub fn charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    /// Without colour, draw the dark modules rather than the light ones,
    /// for a terminal with a light background (default off).
    pub fn invert(mut self, invert: bool) -> Self {
        self.invert = invert;
        self
    }

    /// The dark and light modules' colours with colour on: theme keys or
    /// colour definitions (default `#000000` and `#ffffff`, the
    /// nearest a console has: black and bright white on 16 colours).
    pub fn colours(mut self, dark: impl Into<String>, light: impl Into<String>) -> Self {
        self.dark = dark.into();
        self.light = light.into();
        self
    }

    /// Whether the module at `(x, y)` counted from the quiet zone's corner
    /// is dark (the quiet zone is light).
    fn dark_at(&self, quiet: usize, x: usize, y: usize) -> bool {
        x >= quiet && y >= quiet && self.module(x - quiet, y - quiet)
    }

    /// Cells across for `quiet` modules of quiet zone.
    fn width_for(&self, quiet: usize, ascii: bool) -> usize {
        (self.size + 2 * quiet) * if ascii { 2 } else { 1 }
    }

    /// The quiet zone that fits `width`: the one asked for, or narrower
    /// down to a module.
    fn fit(&self, width: usize, ascii: bool) -> Option<usize> {
        (self.quiet.min(1)..=self.quiet)
            .rev()
            .find(|&q| self.width_for(q, ascii) <= width)
    }

    /// What is written instead of a code too wide for the width given.
    fn note(&self, ascii: bool) -> String {
        format!(
            "QR code needs {} columns",
            self.width_for(self.quiet.min(1), ascii)
        )
    }

    /// The styles with colour on: dark on light for half blocks, and the
    /// two backgrounds for painted spaces.
    fn colour_styles(&self, console: &Console) -> (Style, Style, Style) {
        let colour_of = |spec: &str| user_style(console, spec).color().cloned();
        let (dark, light) = (colour_of(&self.dark), colour_of(&self.light));
        (
            Style::from_color(dark.clone(), light.clone()),
            Style::from_color(None, dark),
            Style::from_color(None, light),
        )
    }

    fn lines(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let charset = self.charset.resolve(console, options, Charset::Blocks);
        let ascii = charset == Charset::Ascii;
        let Some(quiet) = self.fit(options.max_width, ascii) else {
            let note = crate::chart::truncate(&self.note(ascii), options.max_width, ascii);
            return vec![Segment::new(note, None)];
        };
        let colour = has_colour(console);
        let (ink, dark_bg, light_bg) = self.colour_styles(console);
        let side = self.size + 2 * quiet;
        let dark = |x: usize, y: usize| self.dark_at(quiet, x, y);
        // What the glyphs draw: the dark modules with colour (on a light
        // background) or inverted; otherwise the light ones. Past the last
        // row is nothing.
        let inked = |x: usize, y: usize| {
            y < side
                && if colour || self.invert {
                    dark(x, y)
                } else {
                    !dark(x, y)
                }
        };
        let mut out = Vec::new();
        for y in 0..side {
            if ascii {
                if y > 0 {
                    out.push(Segment::line());
                }
                let mut x = 0;
                while x < side {
                    let here = if colour { dark(x, y) } else { inked(x, y) };
                    let run = (x..side)
                        .take_while(|&i| (if colour { dark(i, y) } else { inked(i, y) }) == here)
                        .count();
                    let segment = if colour {
                        let bg = if here { &dark_bg } else { &light_bg };
                        Segment::new(" ".repeat(run * 2), Some(bg.clone()))
                    } else {
                        Segment::new(if here { "##" } else { "  " }.repeat(run), None)
                    };
                    out.push(segment);
                    x += run;
                }
            } else if y % 2 == 0 {
                if y > 0 {
                    out.push(Segment::line());
                }
                let row: String = (0..side)
                    .map(|x| match (inked(x, y), inked(x, y + 1)) {
                        (true, true) => '█',
                        (true, false) => '▀',
                        (false, true) => '▄',
                        (false, false) => ' ',
                    })
                    .collect();
                out.push(Segment::new(row, colour.then(|| ink.clone())));
            }
        }
        out
    }
}

impl Renderable for QrCode {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.lines(console, options)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        let ascii = self.charset.resolve(console, options, Charset::Blocks) == Charset::Ascii;
        let natural = self.width_for(self.quiet, ascii);
        let least = self.width_for(self.quiet.min(1), ascii);
        Measurement::new(least, natural)
            .with_maximum(options.max_width)
            .normalize()
    }
}

/// `QR code, version 2, 25 by 25 modules, medium error correction`, then
/// what it encodes, when that is text.
impl crate::a11y::AccessibleText for QrCode {
    fn accessible_text(&self, _width: usize) -> String {
        let mut out = format!(
            "QR code, version {}, {} by {} modules, {} error correction",
            self.version,
            self.size,
            self.size,
            self.ecc.word()
        );
        match &self.text {
            Some(text) => out.push_str(&format!("\nEncodes: {text}")),
            None => out.push_str("\nEncodes binary data"),
        }
        out
    }
}

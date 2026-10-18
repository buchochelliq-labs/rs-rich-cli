//! PNG of rendered output, without a tape (#723).
//!
//! The screenshot renderer ([`raster::render`]) draws a recorded screen;
//! this draws rendered output instead: a rich-ext [`CellFrame`] or the
//! segments a [`rich::Console`] recorded, resolved through a [`Theme`] as
//! rich's SVG export resolves them. The glyphs are the embedded DejaVu Sans
//! Mono (or [`Fonts::load`]), so the same output, width and options give the
//! same image on every machine.
//!
//! ```
//! use rich::{Console, Text};
//! use rich_record::render::png::{segments_png, PngOptions};
//! use rich_record::render::raster::Fonts;
//!
//! let console = Console::builder().width(20).build();
//! let segments = console.record_output(|c| c.print(&Text::new("hello")));
//! let options = PngOptions { columns: Some(20), ..PngOptions::default() };
//! let png = segments_png(&segments, &Fonts::embedded(), &options)?;
//! assert_eq!(&png[1..4], b"PNG");
//! # Ok::<(), rich_record::render::png::PngError>(())
//! ```

use std::fmt;

use rich::Segment;
use rich_ext::frame::Frame as CellFrame;

use crate::render::raster::{self, Canvas, Fonts, MAX_PIXELS};
use crate::screen::{Snapshot, Theme};

/// How [`frame_png`] draws: the palette, the window frame and its title, a
/// caption, the text size and the width.
#[derive(Clone, Debug)]
pub struct PngOptions<'a> {
    /// The palette (default: rich's SVG export theme, as the screenshots
    /// and `--export-svg` use).
    pub theme: Theme,
    /// The window's title (default none).
    pub title: &'a str,
    /// Draw the window frame: title bar and buttons (default on).
    pub window: bool,
    /// A line of text under the terminal (default none).
    pub caption: Option<&'a str>,
    /// Text size in pixels (default 28, as the screenshots).
    pub size: f32,
    /// The width in cells (default: the frame's widest row). Rows are padded
    /// to it or cropped.
    pub columns: Option<usize>,
}

impl Default for PngOptions<'_> {
    fn default() -> Self {
        PngOptions {
            theme: Theme::default(),
            title: "",
            window: true,
            caption: None,
            size: 28.0,
            columns: None,
        }
    }
}

impl<'a> PngOptions<'a> {
    fn raster(&self) -> raster::Frame<'a> {
        raster::Frame {
            title: self.title,
            key: None,
            size: self.size,
            window: self.window,
            caption: self.caption,
        }
    }
}

/// Why no image was drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PngError {
    /// The image would be larger than [`MAX_PIXELS`].
    TooLarge { width: usize, height: usize },
    /// The text size is not a positive, finite number of pixels.
    BadSize(String),
}

impl fmt::Display for PngError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PngError::TooLarge { width, height } => write!(
                f,
                "a {width}x{height} image is too large to draw (the limit is {MAX_PIXELS} pixels)"
            ),
            PngError::BadSize(size) => write!(f, "text size {size} is not a positive number"),
        }
    }
}

impl std::error::Error for PngError {}

/// The screen [`frame_png`] draws: `frame` at the options' width.
pub fn snapshot(frame: &CellFrame, options: &PngOptions<'_>) -> Snapshot {
    let columns = options.columns.unwrap_or_else(|| frame.width()).max(1);
    Snapshot::from_frame(frame, &options.theme, columns)
}

/// The width and height, in pixels, of [`frame_png`]'s image.
pub fn image_size(frame: &CellFrame, fonts: &Fonts, options: &PngOptions<'_>) -> (usize, usize) {
    let columns = options.columns.unwrap_or_else(|| frame.width()).max(1);
    raster::size(columns, frame.height().max(1), fonts, &options.raster())
}

/// `frame` drawn as pixels.
pub fn frame_canvas(
    frame: &CellFrame,
    fonts: &Fonts,
    options: &PngOptions<'_>,
) -> Result<Canvas, PngError> {
    if !(options.size.is_finite() && options.size > 0.0) {
        return Err(PngError::BadSize(options.size.to_string()));
    }
    let (width, height) = image_size(frame, fonts, options);
    if width.saturating_mul(height) > MAX_PIXELS {
        return Err(PngError::TooLarge { width, height });
    }
    let shot = snapshot(frame, options);
    Ok(raster::render(
        &shot,
        &options.theme,
        fonts,
        &options.raster(),
    ))
}

/// `frame` as a PNG file's bytes.
pub fn frame_png(
    frame: &CellFrame,
    fonts: &Fonts,
    options: &PngOptions<'_>,
) -> Result<Vec<u8>, PngError> {
    frame_canvas(frame, fonts, options).map(|canvas| canvas.png())
}

/// Recorded segments ([`rich::Console::record_output`]) as a PNG file's
/// bytes. Control segments are dropped. Pass the console's width as
/// [`PngOptions::columns`] for an image as wide as the terminal was.
pub fn segments_png(
    segments: &[Segment],
    fonts: &Fonts,
    options: &PngOptions<'_>,
) -> Result<Vec<u8>, PngError> {
    frame_png(&CellFrame::from_segments(segments), fonts, options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich::Style;

    fn decode(png: &[u8]) -> (u32, u32, Vec<u8>) {
        let decoder = ::png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().expect("a PNG");
        let mut pixels = vec![0; reader.output_buffer_size().expect("size")];
        let info = reader.next_frame(&mut pixels).expect("a frame");
        pixels.truncate(info.buffer_size());
        (info.width, info.height, pixels)
    }

    fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> (u8, u8, u8) {
        let i = ((y * width + x) * 3) as usize;
        (pixels[i], pixels[i + 1], pixels[i + 2])
    }

    #[test]
    fn dimensions_follow_columns_rows_and_size() {
        let fonts = Fonts::embedded();
        let segments = [
            Segment::new("hello", None),
            Segment::line(),
            Segment::new("world", None),
            Segment::line(),
        ];
        let options = PngOptions {
            columns: Some(10),
            size: 16.0,
            window: false,
            ..PngOptions::default()
        };
        let png = segments_png(&segments, &fonts, &options).unwrap();
        let (width, height, pixels) = decode(&png);
        // 16 px text: cells about 9.6 px wide (DejaVu's advance), lines
        // 21 px, 19 px padding; no title bar.
        let frame = CellFrame::from_segments(&segments);
        assert_eq!(
            (width as usize, height as usize),
            image_size(&frame, &fonts, &options)
        );
        assert_eq!((width, height), (134, 80));
        // Without a window the corner is the theme's background.
        let theme = Theme::default();
        assert_eq!(pixel(&pixels, width, 0, 0), theme.background);
        // The same input draws the same bytes.
        assert_eq!(png, segments_png(&segments, &fonts, &options).unwrap());
    }

    #[test]
    fn backgrounds_and_the_window_frame() {
        let fonts = Fonts::embedded();
        let red_bg = Style::parse("on #ff0000").unwrap();
        let segments = [Segment::new("    ", Some(red_bg)), Segment::line()];
        let framed = PngOptions {
            title: "demo",
            size: 16.0,
            ..PngOptions::default()
        };
        let png = segments_png(&segments, &fonts, &framed).unwrap();
        let (width, height, pixels) = decode(&png);
        assert_eq!((width, height), (77, 94));
        // The window chrome in the top left; the red cells inside it, at
        // the padding below the title bar.
        assert_eq!(pixel(&pixels, width, 0, 0), (30, 30, 30));
        assert_eq!(pixel(&pixels, width, 22, 60), (255, 0, 0));
    }

    #[test]
    fn refuses_huge_and_bad_sizes() {
        let fonts = Fonts::embedded();
        let frame = CellFrame::from_segments(&[Segment::new("x", None)]);
        let huge = PngOptions {
            columns: Some(1_000_000),
            ..PngOptions::default()
        };
        assert!(matches!(
            frame_png(&frame, &fonts, &huge),
            Err(PngError::TooLarge { .. })
        ));
        let bad = PngOptions {
            size: f32::NAN,
            ..PngOptions::default()
        };
        assert!(matches!(
            frame_png(&frame, &fonts, &bad),
            Err(PngError::BadSize(_))
        ));
    }
}

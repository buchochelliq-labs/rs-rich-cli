//! Pictures for terminal graphics: decoded once, known to the terminal by
//! an id, uploaded once and placed by reference. Behind the `picture`
//! feature.
//!
//! - [`Picture`]: a decoded image with an id and a version. A new frame of
//!   one picture ([`Picture::next`], [`Picture::next_rgba`]: a video, an
//!   emulator's screen) keeps its id, so Kitty replaces the image it holds
//!   rather than keeping one per frame. [`Picture::animated`] carries an
//!   animation Kitty plays on its own clock, and [`Picture::still`] stops
//!   it.
//! - [`Picture::lines`]: the picture as cells, for drawing inside rows of
//!   one's own (a document, a chat line). With [`ImageView::pixels`] other
//!   than [`Pixels::Cells`], the cells carry a [`PictureTag`] in their
//!   style's meta; whatever paints the cells (intuiTUIve's
//!   `image_graphics`) reads the tags back and draws the pixels over them
//!   with [`Picture::encode_rows`], so a picture partly in view is drawn
//!   clipped to its rows shown.
//! - [`picture_bytes`]: what a picture's escapes have cost the terminal,
//!   counted by the painter ([`count_picture_bytes`]).
//! - [`Pacer`] and [`ByteBudget`] (the `frames` feature: video frame
//!   display): a moving picture's frame rate from a byte budget, stepping
//!   down Kitty, iTerm2, Sixel, then cells when the budget is short. With
//!   [`Picture::from_rgba`] and [`Picture::next_rgba`], RGBA frames go to
//!   every protocol with no PNG in between. Decoding video files is not
//!   rs-rich-art's; frames come from whoever has them (a browser's
//!   screencast, an emulator).

use std::collections::BTreeMap;
#[cfg(feature = "frames")]
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use image::{DynamicImage, RgbaImage};
use rich::segment::Segment;
use rich::style::{Meta, MetaValue};
use rich::Style;

use crate::graphics::{encode_png, fit_to_cells, AnimationFrame, CellPixels};
use crate::{ImageAnchor, ImageArt, ImageBackground, ImageFit, ImageMode};

/// The longest side a decoded picture is kept at. Cells hold at most a few
/// pixels each, so a photo is shrunk once, when decoded, not on each draw.
pub const KEEP: u32 = 1536;

/// How a picture reaches the terminal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Pixels {
    /// In cells, as the view's mode draws them: any terminal.
    #[default]
    Cells,
    /// Kitty's graphics protocol with Unicode placeholders (Kitty, Ghostty,
    /// WezTerm): the cells are placeholders, shown as the uploaded picture.
    Kitty,
    /// Sixel, drawn over the picture's half blocks (foot, WezTerm, mlterm,
    /// xterm with `-ti vt340`, Windows Terminal).
    Sixel,
    /// iTerm2's inline images, drawn over the half blocks (iTerm2,
    /// WezTerm).
    Iterm,
}

impl Pixels {
    /// The protocol the terminal is likely to speak, from the environment:
    /// Kitty in Kitty and Ghostty, iTerm2's in iTerm2 and WezTerm, Sixel
    /// where [`sixel::is_probably_supported`](crate::sixel::is_probably_supported)
    /// guesses it works, else cells.
    pub fn detect() -> Pixels {
        let var = |name: &str| std::env::var(name).unwrap_or_default();
        let (term, program) = (var("TERM"), var("TERM_PROGRAM"));
        if std::env::var_os("KITTY_WINDOW_ID").is_some()
            || term.contains("kitty")
            || program.eq_ignore_ascii_case("ghostty")
            || term.contains("ghostty")
        {
            Pixels::Kitty
        } else if program == "iTerm.app" || program.eq_ignore_ascii_case("WezTerm") {
            Pixels::Iterm
        } else if crate::sixel::is_probably_supported() {
            Pixels::Sixel
        } else {
            Pixels::Cells
        }
    }

    /// Its name, as a status line says it: `kitty`, `iterm2`, `sixel`,
    /// `cells`.
    pub fn name(self) -> &'static str {
        match self {
            Pixels::Cells => "cells",
            Pixels::Kitty => "kitty",
            Pixels::Sixel => "sixel",
            Pixels::Iterm => "iterm2",
        }
    }

    /// The step down from it when bytes are short: Kitty, then iTerm2,
    /// then Sixel, then cells.
    pub fn lower(self) -> Option<Pixels> {
        match self {
            Pixels::Kitty => Some(Pixels::Iterm),
            Pixels::Iterm => Some(Pixels::Sixel),
            Pixels::Sixel => Some(Pixels::Cells),
            Pixels::Cells => None,
        }
    }

    fn code(self) -> i64 {
        match self {
            Pixels::Cells => 0,
            Pixels::Kitty => 1,
            Pixels::Sixel => 2,
            Pixels::Iterm => 3,
        }
    }

    fn from_code(code: i64) -> Option<Pixels> {
        Some(match code {
            1 => Pixels::Kitty,
            2 => Pixels::Sixel,
            3 => Pixels::Iterm,
            _ => return None,
        })
    }
}

/// How a picture is drawn in its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use = "builder methods return a new value; it does nothing until it is used"]
pub struct ImageView {
    /// The cell mode: [`ImageMode::Blocks`] (half blocks) by default.
    pub mode: ImageMode,
    /// `None` (the default): as large as fits the rectangle, keeping the
    /// aspect ratio, at its top left. [`ImageFit::Contain`] centres it
    /// instead; [`ImageFit::Cover`] fills the rectangle, cropping around
    /// [`anchor`](Self::anchor); [`ImageFit::Stretch`] fills it exactly.
    pub fit: Option<ImageFit>,
    /// What a cover fit keeps.
    pub anchor: ImageAnchor,
    /// Transparent pixels show the terminal's background (`None`, the
    /// default), or this colour.
    pub background: Option<[u8; 3]>,
    /// Cells only ([`Pixels::Cells`], the default), or real pixels through
    /// a terminal graphics protocol.
    pub pixels: Pixels,
}

impl Default for ImageView {
    fn default() -> Self {
        ImageView {
            mode: ImageMode::Blocks,
            fit: None,
            anchor: ImageAnchor::Center,
            background: None,
            pixels: Pixels::Cells,
        }
    }
}

impl ImageView {
    pub fn new() -> ImageView {
        ImageView::default()
    }

    /// Draw in `mode`: blocks, quadrants, Braille or ASCII.
    pub fn mode(mut self, mode: ImageMode) -> ImageView {
        self.mode = mode;
        self
    }

    /// Fit the rectangle so (see [`ImageView::fit`](Self#structfield.fit)).
    pub fn fit(mut self, fit: ImageFit) -> ImageView {
        self.fit = Some(fit);
        self
    }

    /// What a cover fit keeps.
    pub fn anchor(mut self, anchor: ImageAnchor) -> ImageView {
        self.anchor = anchor;
        self
    }

    /// Paint transparent pixels (and a contained image's margins) this
    /// colour.
    pub fn background(mut self, rgb: [u8; 3]) -> ImageView {
        self.background = Some(rgb);
        self
    }

    /// Draw real pixels through `pixels`' protocol (see [`Pixels`]).
    pub fn pixels(mut self, pixels: Pixels) -> ImageView {
        self.pixels = pixels;
        self
    }
}

/// A decoded picture, with the id terminal graphics know it by.
///
/// Two pictures are equal when they are the same frame of one picture:
/// the same [`id`](Picture::id) and [`version`](Picture::version).
/// Cloning is cheap.
///
/// ```
/// use rich_art::picture::Picture;
/// use rich_art::image::{DynamicImage, Rgb, RgbImage};
///
/// let red = Picture::new(DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, Rgb([255, 0, 0]))));
/// let blue = red.next(DynamicImage::ImageRgb8(RgbImage::from_pixel(4, 4, Rgb([0, 0, 255]))));
/// assert_eq!(red.id(), blue.id());
/// assert_ne!(red, blue);
/// ```
#[derive(Clone)]
pub struct Picture {
    shown: Arc<Shown>,
}

/// One frame of a picture.
struct Shown {
    id: u32,
    version: u32,
    image: Arc<DynamicImage>,
    animation: Option<Arc<[AnimationFrame]>>,
    /// Sent to Kitty as raw RGBA (`f=32`), not PNG: a live frame, where a
    /// PNG encode per frame costs more than the bytes it saves.
    raw: bool,
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        (self.shown.id, self.shown.version) == (other.shown.id, other.shown.version)
    }
}

impl Eq for Picture {}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Picture")
            .field("id", &self.shown.id)
            .field("version", &self.shown.version)
            .field("width", &self.shown.image.width())
            .field("height", &self.shown.image.height())
            .field("animated", &self.is_animated())
            .finish()
    }
}

/// Every picture's frames, by id and version, for [`Picture::find`]
/// (weakly: a frame nothing holds goes).
static PICTURES: Mutex<BTreeMap<(u32, u32), Weak<Shown>>> = Mutex::new(BTreeMap::new());
static NEXT_PICTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
/// The bytes of escapes drawn for each picture id so far.
static DRAWN: Mutex<BTreeMap<u32, u64>> = Mutex::new(BTreeMap::new());

/// A new picture id: 1 to 2^24 - 1, as Kitty's colour-coded ids take.
fn new_id() -> u32 {
    NEXT_PICTURE
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        .wrapping_sub(1)
        % 0x00ff_fffe
        + 1
}

impl Picture {
    /// `image`, under a new id.
    pub fn new(image: DynamicImage) -> Picture {
        Picture::make(new_id(), 0, Arc::new(image), None, false)
    }

    /// The image in `bytes` (any format the `image` crate reads here),
    /// shrunk to at most [`KEEP`] pixels a side, under a new id.
    pub fn decode(bytes: &[u8]) -> Result<Picture, String> {
        let image = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
        Ok(Picture::new(shrink(image)))
    }

    /// The image in `bytes`, keeping an animation: an animated GIF or APNG
    /// of more than one frame becomes an [`animated`](Self::animated)
    /// picture (at most [`ANIMATION_FRAMES`] frames, each shrunk to at
    /// most [`ANIMATION_SIDE`] pixels a side, frames shorter than 20 ms
    /// merged into the next); anything else is [`decode`](Self::decode)d.
    pub fn decode_animated(bytes: &[u8]) -> Result<Picture, String> {
        let animated = bytes.starts_with(b"GIF8") || bytes.starts_with(b"\x89PNG");
        if animated {
            let budget = crate::graphics::FrameBudget {
                max_bytes: 256 << 20,
                max_frames: ANIMATION_FRAMES,
                min_delay: Duration::from_millis(20),
            };
            if let Ok(frames) = crate::graphics::decode_animation(bytes, budget) {
                if frames.len() > 1 {
                    let (w, h) = frames[0].image.dimensions();
                    let scale = (f64::from(ANIMATION_SIDE) / f64::from(w.max(h).max(1))).min(1.0);
                    let width = ((f64::from(w) * scale).round() as u32).max(1);
                    let height = ((f64::from(h) * scale).round() as u32).max(1);
                    let frames = crate::graphics::resample(&frames, width, height, budget);
                    if let Some(picture) = Picture::animated(frames) {
                        return Ok(picture);
                    }
                }
            }
        }
        Picture::decode(bytes)
    }

    /// A frame of raw RGBA pixels, `width` x `height`, under a new id:
    /// no PNG in between, here or on its way to Kitty. `None` when the
    /// pixels are not `width * height * 4` bytes.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Option<Picture> {
        let image = RgbaImage::from_raw(width, height, rgba)?;
        Some(Picture::make(
            new_id(),
            0,
            Arc::new(DynamicImage::ImageRgba8(image)),
            None,
            true,
        ))
    }

    /// An animation (a GIF's frames, from
    /// [`decode_animation`](crate::graphics::decode_animation)): it shows
    /// its first frame, and Kitty plays all of them on its own clock, with
    /// no further bytes. `None` with no frames.
    pub fn animated(frames: Vec<AnimationFrame>) -> Option<Picture> {
        let first = DynamicImage::ImageRgba8(frames.first()?.image.clone());
        let animation = (frames.len() > 1).then(|| frames.into());
        Some(Picture::make(
            new_id(),
            0,
            Arc::new(first),
            animation,
            false,
        ))
    }

    fn make(
        id: u32,
        version: u32,
        image: Arc<DynamicImage>,
        animation: Option<Arc<[AnimationFrame]>>,
        raw: bool,
    ) -> Picture {
        let shown = Arc::new(Shown {
            id,
            version,
            image,
            animation,
            raw,
        });
        let mut pictures = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
        pictures.retain(|_, weak| weak.strong_count() > 0);
        pictures.insert((id, version), Arc::downgrade(&shown));
        Picture { shown }
    }

    /// The next frame of this picture: `image` under the same id, so a
    /// terminal that holds pictures (Kitty) replaces this one with it.
    #[must_use]
    pub fn next(&self, image: DynamicImage) -> Picture {
        Picture::make(self.id(), self.bumped(), Arc::new(image), None, false)
    }

    /// The next frame as raw RGBA pixels (see [`from_rgba`](Self::from_rgba)).
    /// `None` when the pixels are not `width * height * 4` bytes.
    pub fn next_rgba(&self, width: u32, height: u32, rgba: Vec<u8>) -> Option<Picture> {
        let image = RgbaImage::from_raw(width, height, rgba)?;
        Some(Picture::make(
            self.id(),
            self.bumped(),
            Arc::new(DynamicImage::ImageRgba8(image)),
            None,
            true,
        ))
    }

    /// This picture standing still on the frame it shows, under the same
    /// id: an animation stops (Kitty is sent the still frame in its place).
    #[must_use]
    pub fn still(&self) -> Picture {
        Picture::make(
            self.id(),
            self.bumped(),
            self.shown.image.clone(),
            None,
            self.shown.raw,
        )
    }

    fn bumped(&self) -> u32 {
        self.shown.version.wrapping_add(1)
    }

    /// The frame `version` of picture `id`, while something holds it.
    pub fn find(id: u32, version: u32) -> Option<Picture> {
        let pictures = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
        pictures
            .get(&(id, version))
            .and_then(Weak::upgrade)
            .map(|shown| Picture { shown })
    }

    /// The id terminal graphics know it by, the same for every frame.
    pub fn id(&self) -> u32 {
        self.shown.id
    }

    /// Which frame of the picture this is.
    pub fn version(&self) -> u32 {
        self.shown.version
    }

    /// Its pixels (an animation's first frame).
    pub fn image(&self) -> &Arc<DynamicImage> {
        &self.shown.image
    }

    /// Whether it carries an animation for Kitty to play.
    pub fn is_animated(&self) -> bool {
        self.shown.animation.is_some()
    }

    /// The animation's frames, if it has any.
    pub fn frames(&self) -> Option<&[AnimationFrame]> {
        self.shown.animation.as_deref()
    }

    /// The picture as `width` x `height` cells drawn as `view` says (a
    /// height of 0: as tall as it is at `width`), for drawing inside rows
    /// of one's own. With [`ImageView::pixels`] other than cells, the
    /// cells carry [`PictureTag`]s (Kitty's are its placeholders), and any
    /// rows of them can be shown: the painter draws the pixels of the rows
    /// shown.
    pub fn lines(
        &self,
        console: &rich::Console,
        view: ImageView,
        width: u16,
        height: u16,
    ) -> Vec<Vec<Segment>> {
        if width == 0 {
            return Vec::new();
        }
        let mode = match view.mode {
            // Pixel protocols cannot reach the terminal through cells.
            ImageMode::Auto | ImageMode::Sixel => ImageMode::Blocks,
            mode => mode,
        };
        let mut art = ImageArt::new(DynamicImage::clone(&self.shown.image))
            .mode(mode)
            .anchor(view.anchor)
            .background_mode(match view.background {
                Some(rgb) => ImageBackground::Color(rgb),
                None => ImageBackground::TerminalDefault,
            })
            .width(usize::from(width));
        if height > 0 {
            art = match view.fit {
                Some(fit) => art.fit(fit).height(usize::from(height)),
                None => art
                    .max_width(usize::from(width))
                    .max_height(usize::from(height)),
            };
        }
        let options = console.options().update_width(usize::from(width));
        let options = if height == 0 {
            options
        } else {
            options.update_height(usize::from(height))
        };
        let mut lines = console.render_lines(&art, &options, false);
        if height > 0 {
            lines.truncate(usize::from(height));
        }
        if view.pixels != Pixels::Cells && mode == ImageMode::Blocks {
            lines = self.tag(console, view, lines);
        }
        lines
    }

    /// The cells of the picture drawn in pixels: tagged, and for Kitty,
    /// its placeholders.
    fn tag(
        &self,
        console: &rich::Console,
        view: ImageView,
        lines: Vec<Vec<Segment>>,
    ) -> Vec<Vec<Segment>> {
        let pixels = view.pixels;
        let rows = lines.len();
        let cols = lines
            .iter()
            .map(|line| {
                line.iter()
                    .map(|s| rich::cells::cell_len(&s.text))
                    .sum::<usize>()
            })
            .max()
            .unwrap_or(0);
        if rows == 0 || cols == 0 {
            return lines;
        }
        let tag = |row: usize| {
            PictureTag {
                id: self.id(),
                version: self.version(),
                cols,
                rows,
                row,
                pixels,
                fit: view.fit,
                anchor: view.anchor,
            }
            .style()
        };
        if pixels == Pixels::Kitty {
            let truecolor = console.color_system() == Some(rich::color::ColorSystem::Truecolor);
            let Some(colour) = crate::kitty::id_color(self.id(), truecolor) else {
                return lines;
            };
            return (0..rows)
                .map(|row| {
                    let cells = crate::kitty::placeholder_row(row, cols).unwrap_or_default();
                    let style = Style::new().with_color(colour.clone()).combine(&tag(row));
                    vec![Segment::new(cells, Some(style))]
                })
                .collect();
        }
        lines
            .into_iter()
            .enumerate()
            .map(|(row, line)| {
                let tag = tag(row);
                line.into_iter()
                    .map(|mut segment| {
                        segment.style = Some(segment.style.unwrap_or_default().combine(&tag));
                        segment
                    })
                    .collect()
            })
            .collect()
    }

    /// The escape that draws the whole picture in `cols` x `rows` cells
    /// through `pixels`, with the cursor on its top-left cell, leaving the
    /// cursor there. Kitty's uploads it with a virtual placement for its
    /// placeholder cells (an animation's frames too, playing). `None` for
    /// cells, or when it will not encode.
    pub fn encode(&self, pixels: Pixels, cols: usize, rows: usize) -> Option<String> {
        self.encode_fitted(pixels, cols, rows, None, ImageAnchor::Center)
    }

    /// The escape that draws rows `first` to `first + shown` of the cells
    /// `tag` was read from: the picture fitted as those cells were (its
    /// view's fit and anchor), through the tag's protocol. What a painter
    /// draws over a picture's tagged cells.
    pub fn encode_tag(&self, tag: &PictureTag, first: usize, shown: usize) -> Option<String> {
        let (pixels, cols, rows) = (tag.pixels, tag.cols, tag.rows);
        if pixels == Pixels::Kitty || (first == 0 && shown >= rows) {
            return self.encode_fitted(pixels, cols, rows, tag.fit, tag.anchor);
        }
        self.encode_part(
            &self.fitted(&self.shown.image, cols, rows, tag.fit, tag.anchor),
            pixels,
            cols,
            first,
            shown,
        )
    }

    /// `image` in `cols` x `rows` cells as `fit` says (`None`: contained).
    fn fitted(
        &self,
        image: &DynamicImage,
        cols: usize,
        rows: usize,
        fit: Option<ImageFit>,
        anchor: ImageAnchor,
    ) -> RgbaImage {
        let cell = CellPixels::DEFAULT;
        match fit {
            None | Some(ImageFit::Contain) | Some(ImageFit::Native) => {
                fit_to_cells(image, cols, rows, cell)
            }
            Some(fit) => {
                let (width, height) = cell.span(cols, rows);
                crate::graphics::fill_pixels(image, width, height, fit, anchor)
            }
        }
    }

    fn encode_fitted(
        &self,
        pixels: Pixels,
        cols: usize,
        rows: usize,
        fit: Option<ImageFit>,
        anchor: ImageAnchor,
    ) -> Option<String> {
        let fitted = |image: &DynamicImage| self.fitted(image, cols, rows, fit, anchor);
        match pixels {
            Pixels::Cells => None,
            Pixels::Kitty if self.shown.animation.is_some() => {
                let frames: Vec<_> = self
                    .frames()?
                    .iter()
                    .map(|frame| AnimationFrame {
                        image: fitted(&DynamicImage::ImageRgba8(frame.image.clone())),
                        delay: frame.delay,
                    })
                    .collect();
                crate::kitty::transmit_animation(self.id(), &frames, cols, rows)
            }
            Pixels::Kitty => {
                let fitted = fitted(&self.shown.image);
                if self.shown.raw {
                    let (width, height) = fitted.dimensions();
                    return Some(crate::kitty::transmit_rgba(
                        self.id(),
                        fitted.as_raw(),
                        width,
                        height,
                        cols,
                        rows,
                    ));
                }
                let png = encode_png(&fitted)?;
                Some(crate::kitty::transmit(self.id(), &png, cols, rows))
            }
            Pixels::Sixel => {
                let image = DynamicImage::ImageRgba8(fitted(&self.shown.image));
                let sixel = crate::SixelArt::new(image).encode_cells(cols, rows)?;
                Some(format!("\x1b7{sixel}\x1b8"))
            }
            Pixels::Iterm => {
                let png = encode_png(&fitted(&self.shown.image))?;
                Some(format!(
                    "\x1b7{}\x1b8",
                    crate::iterm::inline_image(&png, cols, rows)
                ))
            }
        }
    }

    /// The escape that draws rows `first` to `first + shown` of the
    /// picture fitted to `cols` x `rows` cells: a picture partly in view.
    /// Kitty's placeholders draw row by row, so for Kitty this is
    /// [`encode`](Self::encode). `None` for cells, or when no rows show.
    pub fn encode_rows(
        &self,
        pixels: Pixels,
        cols: usize,
        rows: usize,
        first: usize,
        shown: usize,
    ) -> Option<String> {
        if pixels == Pixels::Kitty || (first == 0 && shown >= rows) {
            return self.encode(pixels, cols, rows);
        }
        let whole = fit_to_cells(&self.shown.image, cols, rows, CellPixels::DEFAULT);
        self.encode_part(&whole, pixels, cols, first, shown)
    }

    /// Rows `first` to `first + shown` of `whole`, the picture already
    /// fitted to all its cells, through Sixel or iTerm2.
    fn encode_part(
        &self,
        whole: &RgbaImage,
        pixels: Pixels,
        cols: usize,
        first: usize,
        shown: usize,
    ) -> Option<String> {
        let cell = CellPixels::DEFAULT;
        let top = (first as u32 * cell.height).min(whole.height());
        let height = (shown as u32 * cell.height).min(whole.height() - top);
        if height == 0 {
            return None;
        }
        let part = image::imageops::crop_imm(whole, 0, top, whole.width(), height).to_image();
        match pixels {
            Pixels::Sixel => {
                let sixel = crate::SixelArt::new(DynamicImage::ImageRgba8(part))
                    .encode_cells(cols, shown)?;
                Some(format!("\x1b7{sixel}\x1b8"))
            }
            Pixels::Iterm => {
                let png = encode_png(&part)?;
                Some(format!(
                    "\x1b7{}\x1b8",
                    crate::iterm::inline_image(&png, cols, shown)
                ))
            }
            Pixels::Kitty | Pixels::Cells => None,
        }
    }

    /// The escape that frees what the terminal holds for picture `id`
    /// (Kitty's image data; Sixel and iTerm2 hold nothing).
    pub fn release(id: u32) -> String {
        crate::kitty::delete(id)
    }
}

/// The most frames [`Picture::decode_animated`] keeps.
pub const ANIMATION_FRAMES: usize = 240;
/// The longest side of an animation's frames as
/// [`Picture::decode_animated`] keeps them.
pub const ANIMATION_SIDE: u32 = 640;

/// An animated picture's frames stepped by the clock, for a terminal that
/// cannot play the animation itself (Sixel, iTerm2, cells; Kitty plays
/// [`Picture::animated`] on its own). Each frame is a [`Picture`] under
/// the animation's id, so a painter makes each frame's escape once and
/// draws it again each loop.
///
/// ```
/// use std::time::{Duration, Instant};
/// use rich_art::graphics::AnimationFrame;
/// use rich_art::image::{Rgba, RgbaImage};
/// use rich_art::picture::{Picture, Player};
///
/// let frames = (0..3u8)
///     .map(|k| AnimationFrame {
///         image: RgbaImage::from_pixel(4, 4, Rgba([k * 80, 0, 0, 255])),
///         delay: Duration::from_millis(100),
///     })
///     .collect();
/// let gif = Picture::animated(frames).unwrap();
/// let start = Instant::now();
/// let mut player = Player::new(&gif, start).unwrap();
/// assert_eq!(player.index(), 0);
/// assert!(player.advance(start + Duration::from_millis(150)));
/// assert_eq!(player.index(), 1);
/// assert_eq!(player.frame().id(), gif.id());
/// ```
#[derive(Clone, Debug)]
pub struct Player {
    frames: Vec<Picture>,
    delays: Vec<Duration>,
    at: usize,
    since: Instant,
}

impl Player {
    /// A player of `picture`'s animation from its first frame at `now`;
    /// `None` for a picture that is not animated.
    pub fn new(picture: &Picture, now: Instant) -> Option<Player> {
        let animation = picture.frames()?;
        let mut frames: Vec<Picture> = Vec::with_capacity(animation.len());
        for frame in animation {
            let image = DynamicImage::ImageRgba8(frame.image.clone());
            let next = match frames.last() {
                Some(last) => last.next(image),
                None => picture.next(image),
            };
            frames.push(next);
        }
        Some(Player {
            delays: animation.iter().map(|f| f.delay).collect(),
            frames,
            at: 0,
            since: now,
        })
    }

    /// The frame showing.
    pub fn frame(&self) -> &Picture {
        &self.frames[self.at]
    }

    /// Which frame shows, from 0.
    pub fn index(&self) -> usize {
        self.at
    }

    /// How many frames it has.
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether it has no frames (never: [`new`](Self::new) refuses that).
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Whether the frame showing has had its time by `now`.
    pub fn due(&self, now: Instant) -> bool {
        now.duration_since(self.since) >= self.delays[self.at]
    }

    /// How long until the frame showing has had its time.
    pub fn wait(&self, now: Instant) -> Duration {
        self.delays[self.at].saturating_sub(now.duration_since(self.since))
    }

    /// Move on to the frame the clock says shows at `now`, skipping those
    /// whose time went by (a terminal too slow for every frame skips
    /// frames rather than slowing the animation down); `true` when the
    /// frame changed.
    pub fn advance(&mut self, now: Instant) -> bool {
        let start = self.at;
        let mut skipped = 0;
        while self.due(now) && skipped < self.frames.len() {
            self.since += self.delays[self.at];
            self.at = (self.at + 1) % self.frames.len();
            skipped += 1;
        }
        // A long pause (frozen, off screen) starts again from now.
        if skipped == self.frames.len() {
            self.since = now;
        }
        self.at != start
    }
}

/// `image` shrunk to at most [`KEEP`] pixels a side.
pub fn shrink(image: DynamicImage) -> DynamicImage {
    if image.width() > KEEP || image.height() > KEEP {
        image.thumbnail(KEEP, KEEP)
    } else {
        image
    }
}

/// What a cell of a picture drawn in pixels says, in its style's meta:
/// the picture's frame, its size in cells, which of its rows, and the
/// protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PictureTag {
    pub id: u32,
    pub version: u32,
    pub cols: usize,
    pub rows: usize,
    pub row: usize,
    pub pixels: Pixels,
    /// How the cells were fitted ([`ImageView::fit`]), so the pixels
    /// drawn over them are fitted the same way.
    pub fit: Option<ImageFit>,
    pub anchor: ImageAnchor,
}

const FITS: [ImageFit; 4] = [
    ImageFit::Contain,
    ImageFit::Cover,
    ImageFit::Stretch,
    ImageFit::Native,
];
const ANCHORS: [ImageAnchor; 9] = [
    ImageAnchor::Center,
    ImageAnchor::Top,
    ImageAnchor::Bottom,
    ImageAnchor::Left,
    ImageAnchor::Right,
    ImageAnchor::TopLeft,
    ImageAnchor::TopRight,
    ImageAnchor::BottomLeft,
    ImageAnchor::BottomRight,
];

impl PictureTag {
    /// The meta key: `[id, cols, rows, row, protocol, version, fit,
    /// anchor]`.
    pub const META_KEY: &'static str = "intuituive.image";

    /// The tag a cell's style carries, if any.
    pub fn of(style: &Style) -> Option<PictureTag> {
        let MetaValue::List(items) = style.meta_ref()?.get(Self::META_KEY)? else {
            return None;
        };
        let int = |i: usize| match items.get(i) {
            Some(MetaValue::Int(v)) => Some(*v),
            _ => None,
        };
        Some(PictureTag {
            id: u32::try_from(int(0)?).ok()?,
            cols: usize::try_from(int(1)?).ok()?,
            rows: usize::try_from(int(2)?).ok()?,
            row: usize::try_from(int(3)?).ok()?,
            pixels: Pixels::from_code(int(4)?)?,
            version: int(5).and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
            fit: int(6)
                .and_then(|v| usize::try_from(v).ok())
                .and_then(|v| v.checked_sub(1))
                .and_then(|v| FITS.get(v).copied()),
            anchor: int(7)
                .and_then(|v| usize::try_from(v).ok())
                .and_then(|v| ANCHORS.get(v).copied())
                .unwrap_or_default(),
        })
    }

    /// The style that carries this tag.
    pub fn style(&self) -> Style {
        let mut meta = Meta::new();
        meta.insert(
            Self::META_KEY,
            MetaValue::List(vec![
                MetaValue::Int(i64::from(self.id)),
                MetaValue::Int(self.cols as i64),
                MetaValue::Int(self.rows as i64),
                MetaValue::Int(self.row as i64),
                MetaValue::Int(self.pixels.code()),
                MetaValue::Int(i64::from(self.version)),
                MetaValue::Int(
                    self.fit
                        .and_then(|f| FITS.iter().position(|x| *x == f))
                        .map_or(0, |at| at as i64 + 1),
                ),
                MetaValue::Int(ANCHORS.iter().position(|a| *a == self.anchor).unwrap_or(0) as i64),
            ]),
        );
        Style::from_meta(meta)
    }

    /// The frame it tags, while something holds it.
    pub fn picture(&self) -> Option<Picture> {
        Picture::find(self.id, self.version)
    }
}

/// The bytes the terminal has been sent to draw picture `id` (every frame
/// of it) so far, as the painter counted them ([`count_picture_bytes`]):
/// what a moving picture costs. A picture drawn in cells costs its cells,
/// which the painter sends as text, and counts nothing here.
pub fn picture_bytes(id: u32) -> u64 {
    DRAWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&id)
        .copied()
        .unwrap_or(0)
}

/// Count `bytes` more sent to the terminal for picture `id`: called by
/// whatever writes a picture's escape.
pub fn count_picture_bytes(id: u32, bytes: u64) {
    *DRAWN
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entry(id)
        .or_default() += bytes;
}

/// A byte budget for moving pictures: how many bytes a second the terminal
/// takes for them, and the frame rates allowed.
#[cfg(feature = "frames")]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ByteBudget {
    /// Bytes a second, shared by every moving picture.
    pub bytes_per_second: u64,
    /// The most frames a second a picture shows: a ceiling, not a target.
    pub max_fps: f64,
    /// Below this rate a picture steps down to a cheaper protocol.
    pub min_fps: f64,
}

#[cfg(feature = "frames")]
impl ByteBudget {
    /// A budget of `bytes_per_second`, at most 8 frames a second, stepping
    /// down below 2.
    pub fn new(bytes_per_second: u64) -> ByteBudget {
        ByteBudget {
            bytes_per_second,
            max_fps: 8.0,
            min_fps: 2.0,
        }
    }

    /// The budget's guess when nothing was measured: 4 MB a second on this
    /// machine, 256 kB a second over SSH (`SSH_CONNECTION` or `SSH_TTY`
    /// set).
    pub fn guess() -> ByteBudget {
        if over_ssh() {
            ByteBudget::new(256 * 1024)
        } else {
            ByteBudget::new(4 * 1024 * 1024)
        }
    }
}

/// Whether the terminal is at the far end of an SSH link.
#[cfg(feature = "frames")]
pub fn over_ssh() -> bool {
    std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some()
}

/// How long a pacer's window of frames sent is.
#[cfg(feature = "frames")]
const WINDOW: Duration = Duration::from_secs(2);

/// One moving picture's frame rate and protocol, kept within its share of
/// a [`ByteBudget`]: it learns what a frame costs from what was sent, gives
/// the rate that share allows (at most the ceiling), and steps down a
/// protocol when that rate is under the budget's floor.
///
/// ```
/// use rich_art::picture::{ByteBudget, Pacer, Pixels};
///
/// // 60 kB a second for a 40 x 12 cell picture in Sixel: too few frames,
/// // so it steps down to cells.
/// let budget = ByteBudget::new(60_000);
/// let mut pacer = Pacer::new(Pixels::Sixel, 40, 12);
/// while pacer.settle(&budget, 1.0) {}
/// assert_eq!(pacer.pixels(), Pixels::Cells);
/// assert!(pacer.fps(&budget, 1.0) >= budget.min_fps);
/// ```
#[cfg(feature = "frames")]
#[derive(Clone, Debug)]
pub struct Pacer {
    pixels: Pixels,
    cols: usize,
    rows: usize,
    /// Frames sent in the last [`WINDOW`]: when, and their bytes.
    sent: VecDeque<(Instant, u64)>,
    /// When it last stepped down: frames sent before are counted against
    /// the window but not taken as what the next frame may cost.
    stepped: Option<Instant>,
    /// What a frame costs at this protocol, measured; else estimated.
    measured: Option<f64>,
}

#[cfg(feature = "frames")]
impl Pacer {
    /// A picture of `cols` x `rows` cells starting at `pixels`.
    pub fn new(pixels: Pixels, cols: usize, rows: usize) -> Pacer {
        Pacer {
            pixels,
            cols: cols.max(1),
            rows: rows.max(1),
            sent: VecDeque::new(),
            stepped: None,
            measured: None,
        }
    }

    /// Its protocol now.
    pub fn pixels(&self) -> Pixels {
        self.pixels
    }

    /// A new size in cells: what a frame costs is estimated again.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        if (cols.max(1), rows.max(1)) != (self.cols, self.rows) {
            self.cols = cols.max(1);
            self.rows = rows.max(1);
            self.measured = None;
        }
    }

    /// What one frame of `cols` x `rows` cells is likely to cost through
    /// `pixels`, before anything is measured: a PNG photo at about a third
    /// of its raw size, base64 for Kitty and iTerm2, about half a byte a
    /// pixel for Sixel, and 40 bytes a cell for half blocks in true colour.
    pub fn estimate(pixels: Pixels, cols: usize, rows: usize) -> u64 {
        let (w, h) = CellPixels::DEFAULT.span(cols, rows);
        let area = u64::from(w) * u64::from(h);
        match pixels {
            Pixels::Kitty | Pixels::Iterm => area * 4 / 3,
            Pixels::Sixel => area / 2,
            Pixels::Cells => (cols * rows) as u64 * 40,
        }
    }

    /// What a frame costs now: measured, else [`estimate`](Self::estimate)d.
    pub fn bytes_per_frame(&self) -> f64 {
        self.measured
            .unwrap_or_else(|| Pacer::estimate(self.pixels, self.cols, self.rows) as f64)
            .max(1.0)
    }

    /// The frames a second its `share` (0 to 1) of `budget` allows, at most
    /// the ceiling.
    pub fn fps(&self, budget: &ByteBudget, share: f64) -> f64 {
        let allowed = budget.bytes_per_second as f64 * share.clamp(0.0, 1.0);
        (allowed / self.bytes_per_frame()).min(budget.max_fps)
    }

    /// Whether a frame may be sent at `now`: a frame's time at that rate
    /// has gone since the last, and the bytes sent in the last two seconds
    /// leave room for one more.
    pub fn due(&mut self, now: Instant, budget: &ByteBudget, share: f64) -> bool {
        self.forget(now);
        let per_second = budget.bytes_per_second as f64 * share.clamp(0.0, 1.0);
        if per_second <= 0.0 {
            return false;
        }
        // Spaced for the frame planned for, so frames that grow do not
        // come in a burst that fills the window and then a gap.
        let next = self.next_frame();
        let fps = (per_second / next).min(budget.max_fps);
        if let Some((last, _)) = self.sent.back() {
            if now.duration_since(*last) < Duration::from_secs_f64(1.0 / fps) {
                return false;
            }
        }
        let used: u64 = self.sent.iter().map(|(_, b)| b).sum();
        used as f64 + next <= per_second * WINDOW.as_secs_f64()
    }

    /// What the next frame may cost, for keeping the window under its
    /// cap: the largest frame of the last two seconds at this protocol or
    /// the average, whichever is more, so one large frame among small ones
    /// cannot push the window over.
    fn next_frame(&self) -> f64 {
        let largest = self
            .sent
            .iter()
            .filter(|(at, _)| self.stepped.is_none_or(|stepped| *at >= stepped))
            .map(|(_, b)| *b)
            .max()
            .unwrap_or(0) as f64;
        largest.max(self.bytes_per_frame())
    }

    /// A frame was sent at `now`, costing `bytes` (counted by the painter,
    /// [`picture_bytes`], or `None` to take the estimate).
    pub fn sent(&mut self, now: Instant, bytes: Option<u64>) {
        let bytes = bytes.unwrap_or_else(|| self.bytes_per_frame() as u64);
        self.measured = Some(match self.measured {
            Some(before) => before * 0.7 + bytes as f64 * 0.3,
            None => bytes as f64,
        });
        self.sent.push_back((now, bytes));
        self.forget(now);
    }

    fn forget(&mut self, now: Instant) {
        while self
            .sent
            .front()
            .is_some_and(|(at, _)| now.duration_since(*at) > WINDOW)
        {
            self.sent.pop_front();
        }
    }

    /// Step down a protocol if the rate allowed is under the floor; `true`
    /// when it stepped.
    pub fn settle(&mut self, budget: &ByteBudget, share: f64) -> bool {
        if self.fps(budget, share) >= budget.min_fps {
            return false;
        }
        match self.pixels.lower() {
            Some(lower) => {
                self.pixels = lower;
                self.measured = None;
                self.stepped = self
                    .sent
                    .back()
                    .map(|(at, _)| *at + Duration::from_nanos(1));
                true
            }
            None => false,
        }
    }

    /// The bytes sent in the last two seconds.
    pub fn recent_bytes(&self) -> u64 {
        self.sent.iter().map(|(_, b)| b).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn solid(r: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([r, 0, 0])))
    }

    #[test]
    fn frames_keep_the_id_and_are_found_while_held() {
        let first = Picture::new(solid(1));
        let second = first.next(solid(2));
        assert_eq!((first.id(), second.version()), (second.id(), 1));
        assert!(Picture::find(first.id(), 1).is_some_and(|p| p == second));
        let id = second.id();
        drop(second);
        assert!(Picture::find(id, 1).is_none());
        assert!(Picture::find(id, 0).is_some());
    }

    #[test]
    fn tags_read_back_and_lines_are_tagged() {
        let picture = Picture::new(solid(3));
        let console = rich::Console::builder().width(4).build();
        let lines = picture.lines(&console, ImageView::new().pixels(Pixels::Sixel), 4, 2);
        let tag = PictureTag::of(lines[0][0].style.as_ref().unwrap()).unwrap();
        assert_eq!(
            (tag.id, tag.cols, tag.rows, tag.row, tag.pixels),
            (picture.id(), 4, 2, 0, Pixels::Sixel)
        );
        assert_eq!(tag.picture(), Some(picture.clone()));
        // Cells only: no tags.
        let plain = picture.lines(&console, ImageView::new(), 4, 2);
        assert!(plain[0]
            .iter()
            .all(|s| s.style.as_ref().and_then(PictureTag::of).is_none()));
    }

    #[test]
    fn encodes_each_protocol_and_rows_shown() {
        let picture = Picture::new(solid(200));
        assert!(picture
            .encode(Pixels::Sixel, 4, 2)
            .unwrap()
            .starts_with("\x1b7\x1bP"));
        assert!(picture
            .encode(Pixels::Iterm, 4, 2)
            .unwrap()
            .contains("height=2;"));
        assert!(picture
            .encode_rows(Pixels::Iterm, 4, 4, 1, 2)
            .unwrap()
            .contains("height=2;"));
        assert!(picture
            .encode(Pixels::Kitty, 4, 2)
            .unwrap()
            .contains("f=100"));
        assert!(picture.encode(Pixels::Cells, 4, 2).is_none());
        // A raw frame goes to Kitty as RGBA, not PNG.
        let raw = Picture::from_rgba(2, 2, vec![255; 16]).unwrap();
        assert!(raw.encode(Pixels::Kitty, 4, 2).unwrap().contains("f=32"));
        assert!(Picture::from_rgba(2, 2, vec![0; 3]).is_none());
    }

    #[test]
    fn an_animated_gif_decodes_with_its_frames_and_plays() {
        let frames: Vec<AnimationFrame> = (0..4u8)
            .map(|k| AnimationFrame {
                image: RgbaImage::from_pixel(1000, 500, image::Rgba([k * 60, 0, 0, 255])),
                delay: Duration::from_millis(100),
            })
            .collect();
        let gif = crate::graphics::encode_gif(&frames).unwrap();
        let picture = Picture::decode_animated(&gif).unwrap();
        assert!(picture.is_animated());
        let kept = picture.frames().unwrap();
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[0].image.dimensions(), (640, 320));
        // A still stays still.
        let png = encode_png(&RgbaImage::new(2, 2)).unwrap();
        assert!(!Picture::decode_animated(&png).unwrap().is_animated());
        // Played: a frame each 100 ms, a slow clock skipping frames.
        let start = Instant::now();
        let mut player = Player::new(&picture, start).unwrap();
        assert!(!player.advance(start + Duration::from_millis(50)));
        assert!(player.advance(start + Duration::from_millis(100)));
        assert_eq!(player.index(), 1);
        assert!(player.advance(start + Duration::from_millis(350)));
        assert_eq!(player.index(), 3);
        assert!(player.wait(start + Duration::from_millis(350)) <= Duration::from_millis(50));
        // Every frame keeps the picture's id, each its own version.
        let versions: std::collections::HashSet<u32> = (0..4)
            .map(|_| {
                let later = player.since + Duration::from_millis(100);
                player.advance(later);
                assert_eq!(player.frame().id(), picture.id());
                player.frame().version()
            })
            .collect();
        assert_eq!(versions.len(), 4);
    }

    #[test]
    fn the_pixels_are_fitted_as_the_cells_were() {
        // A wide picture in square-ish cells: contained, it leaves bands;
        // covered, it fills them, as its half blocks under it do.
        let wide = Picture::new(DynamicImage::ImageRgb8(RgbImage::from_pixel(
            100,
            10,
            Rgb([255, 0, 0]),
        )));
        let console = rich::Console::builder().width(4).build();
        let view = ImageView::new()
            .fit(ImageFit::Cover)
            .anchor(ImageAnchor::Top)
            .pixels(Pixels::Iterm);
        let lines = wide.lines(&console, view, 4, 4);
        let tag = PictureTag::of(lines[0][0].style.as_ref().unwrap()).unwrap();
        assert_eq!(
            (tag.fit, tag.anchor),
            (Some(ImageFit::Cover), ImageAnchor::Top)
        );
        let covered = wide.fitted(wide.image(), 4, 4, tag.fit, tag.anchor);
        assert!(covered.pixels().all(|p| p.0[3] == 255));
        let contained = wide.fitted(wide.image(), 4, 4, None, ImageAnchor::Center);
        assert!(contained.pixels().any(|p| p.0[3] == 0));
        assert_ne!(
            wide.encode_tag(&tag, 0, 4),
            wide.encode(Pixels::Iterm, 4, 4)
        );
        // Part of it, for a picture partly in view.
        assert!(wide.encode_tag(&tag, 1, 2).unwrap().contains("height=2;"));
    }

    #[test]
    fn bytes_are_counted_per_picture() {
        let picture = Picture::new(solid(4));
        assert_eq!(picture_bytes(picture.id()), 0);
        count_picture_bytes(picture.id(), 10);
        count_picture_bytes(picture.id(), 5);
        assert_eq!(picture_bytes(picture.id()), 15);
    }

    #[cfg(feature = "frames")]
    #[test]
    fn a_large_frame_among_small_ones_keeps_the_window_under_its_cap() {
        let budget = ByteBudget::new(100_000);
        let mut pacer = Pacer::new(Pixels::Sixel, 20, 10);
        let start = Instant::now();
        let mut sent: Vec<(Instant, u64)> = Vec::new();
        for tick in 0..1000u64 {
            let now = start + Duration::from_millis(tick * 10);
            if pacer.due(now, &budget, 1.0) {
                // Every seventh frame is five times the others.
                let bytes = if sent.len() % 7 == 6 { 40_000 } else { 8_000 };
                pacer.sent(now, Some(bytes));
                sent.push((now, bytes));
            }
        }
        // No two-second window holds more than its cap.
        for (at, _) in &sent {
            let window: u64 = sent
                .iter()
                .filter(|(t, _)| *t >= *at && t.duration_since(*at) < WINDOW)
                .map(|(_, b)| b)
                .sum();
            assert!(window <= 200_000, "{window} bytes in two seconds");
        }
        assert!(sent.len() > 20, "{} frames", sent.len());
    }

    #[cfg(feature = "frames")]
    #[test]
    fn the_pacer_keeps_to_its_share_and_steps_down() {
        let budget = ByteBudget::new(100_000);
        let mut pacer = Pacer::new(Pixels::Kitty, 20, 10);
        let start = Instant::now();
        // Frames of 25 kB: four a second at most, never above the ceiling.
        let mut sent = 0;
        for tick in 0..200u64 {
            let now = start + Duration::from_millis(tick * 10);
            if pacer.due(now, &budget, 1.0) {
                pacer.sent(now, Some(25_000));
                sent += 1;
            }
        }
        assert!((7..=9).contains(&sent), "{sent} frames in 2 s");
        assert!(pacer.recent_bytes() <= 200_000);
        assert!(!pacer.settle(&budget, 1.0));
        // Half the budget, frames of 100 kB: under the floor, so it steps.
        pacer.sent(start, Some(100_000));
        pacer.sent(start, Some(100_000));
        assert!(pacer.settle(&budget, 0.5));
        assert_eq!(pacer.pixels(), Pixels::Iterm);
        // The ceiling: tiny frames still at most eight a second.
        let mut tiny = Pacer::new(Pixels::Cells, 2, 1);
        assert_eq!(tiny.fps(&budget, 1.0), 8.0);
        let mut count = 0;
        for tick in 0..100u64 {
            let now = start + Duration::from_millis(tick * 10);
            if tiny.due(now, &budget, 1.0) {
                tiny.sent(now, Some(10));
                count += 1;
            }
        }
        assert!(count <= 9, "{count}");
    }

    #[cfg(feature = "frames")]
    #[test]
    fn growing_frames_are_spaced_not_sent_in_a_burst_and_a_gap() {
        // A small first frame, then frames of 60 kB, at 100 kB a second:
        // spaced by what the next may cost, so no long gap follows a burst
        // of frames paced for the small one, and every two seconds stay
        // within the cap.
        let budget = ByteBudget::new(100_000);
        let mut pacer = Pacer::new(Pixels::Kitty, 20, 10);
        let start = Instant::now();
        let mut sent: Vec<(Duration, u64)> = Vec::new();
        for tick in 0..400u64 {
            let at = Duration::from_millis(tick * 10);
            let now = start + at;
            if pacer.due(now, &budget, 1.0) {
                let bytes = if sent.is_empty() { 10_000 } else { 60_000 };
                pacer.sent(now, Some(bytes));
                sent.push((at, bytes));
            }
        }
        let gaps: Vec<Duration> = sent.windows(2).map(|w| w[1].0 - w[0].0).collect();
        let longest = gaps.iter().max().copied().unwrap_or_default();
        assert!(longest <= Duration::from_millis(1000), "{sent:?}");
        for (i, (at, _)) in sent.iter().enumerate() {
            let window: u64 = sent[..=i]
                .iter()
                .filter(|(t, _)| *at - *t <= Duration::from_secs(2))
                .map(|(_, b)| b)
                .sum();
            assert!(window <= 200_000, "{window} by {at:?}");
        }
    }

    #[cfg(feature = "frames")]
    #[test]
    fn after_a_step_down_frames_are_spaced_for_the_lower_protocol() {
        // One Kitty frame of 270 kB at 400 kB a second steps down; the
        // iTerm2 frames of 45 kB after it are spaced for 45 kB, not 270.
        let budget = ByteBudget::new(400_000);
        let mut pacer = Pacer::new(Pixels::Kitty, 40, 10);
        let start = Instant::now();
        pacer.sent(start, Some(270_000));
        assert!(pacer.settle(&budget, 1.0));
        let mut sent = Vec::new();
        for tick in 1..150u64 {
            let now = start + Duration::from_millis(tick * 10);
            if pacer.due(now, &budget, 1.0) {
                pacer.sent(now, Some(45_000));
                sent.push(tick * 10);
            }
        }
        assert!(sent.len() >= 8, "{sent:?}");
        assert!(pacer.recent_bytes() <= 800_000);
    }
}

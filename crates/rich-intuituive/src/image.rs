//! Image files as nodes: a picture drawn in terminal cells by
//! [rs-rich-art](https://docs.rs/rs-rich-art), sized to the node's
//! rectangle. Behind the off-by-default `art` feature.
//!
//! [`image`] shows a file; [`image_of`] follows a path that changes (a
//! file manager's preview of the selected entry); [`image_bytes`] and
//! [`image_bytes_of`] show an image in memory (a cover embedded in a
//! song). It is decoded on a worker thread, never on the app's, and drawn
//! again only when the node's size changes. It draws in half blocks by
//! default (two pixels a cell, full colour, in any terminal with truecolor
//! or 256 colours); quadrants, Braille and ASCII are the other cell modes.
//!
//! Real pixels go through a terminal graphics protocol: give the view
//! [`Pixels::Kitty`], [`Pixels::Sixel`] or [`Pixels::Iterm`] (or
//! [`Pixels::detect`]) and the app [`image_graphics`]
//! ([`App::graphics`](crate::App::graphics)). The node still draws its
//! half blocks, tagged, and the app draws the picture over them after each
//! frame's cells (Sixel, iTerm2), or the cells are Kitty's placeholders,
//! which show the picture the app uploaded. Without the app's graphics
//! source a Sixel or iTerm2 view is half blocks; [`ImageMode::Sixel`] and
//! [`ImageMode::Auto`] draw half blocks too.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use rich::segment::Segment;
use rich::style::{Meta, MetaValue};
use rich::Style;
use rich_art::image::DynamicImage;
pub use rich_art::{ImageAnchor, ImageFit, ImageMode};
use rich_art::{ImageArt, ImageBackground};
use rich_ext::frame::{Frame, Graphic, Placement, PlacementSource};

use crate::node::{leaf, Node};
use crate::reactive::{signal, watch, Signal};
use crate::task::spawn;

/// The longest side an image is kept at after decoding. Cells hold at most
/// a few pixels each, so a photo is shrunk once, on the worker, rather than
/// on every draw.
const KEEP: u32 = 1536;

/// How an [`image`] node draws its picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// a terminal graphics protocol, which the app's [`image_graphics`]
    /// draws.
    pub pixels: Pixels,
}

/// How an image node reaches the terminal: [`ImageView::pixels`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Pixels {
    /// In cells, as the view's mode draws them: any terminal.
    #[default]
    Cells,
    /// Kitty's graphics protocol with Unicode placeholders (Kitty, Ghostty,
    /// WezTerm): the cells are placeholders, shown as the picture the app
    /// uploads ([`image_graphics`]). Without it they show nothing useful.
    Kitty,
    /// Sixel, drawn over the node's half blocks after each frame (foot,
    /// WezTerm, mlterm, xterm with `-ti vt340`, Windows Terminal).
    Sixel,
    /// iTerm2's inline images, drawn over the half blocks (iTerm2,
    /// WezTerm).
    Iterm,
}

impl Pixels {
    /// The protocol the terminal is likely to speak, from the environment:
    /// Kitty in Kitty and Ghostty, iTerm2's in iTerm2 and WezTerm, Sixel
    /// where rs-rich-art guesses it works, else cells.
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
        } else if rich_art::sixel::is_probably_supported() {
            Pixels::Sixel
        } else {
            Pixels::Cells
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

    /// Draw real pixels through `pixels`' protocol (see [`Pixels`]); the
    /// app draws them with [`image_graphics`].
    pub fn pixels(mut self, pixels: Pixels) -> ImageView {
        self.pixels = pixels;
        self
    }
}

/// What an image node has to draw.
#[derive(Clone, Default)]
enum Picture {
    /// No path.
    #[default]
    Empty,
    Loading,
    Ready(Arc<DynamicImage>),
    Failed(String),
}

impl PartialEq for Picture {
    /// The same state; two pictures are the same only if they share one
    /// decoded image.
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Picture::Empty, Picture::Empty) | (Picture::Loading, Picture::Loading) => true,
            (Picture::Ready(a), Picture::Ready(b)) => Arc::ptr_eq(a, b),
            (Picture::Failed(a), Picture::Failed(b)) => a == b,
            _ => false,
        }
    }
}

/// The image file at `path`, drawn in half blocks to fit the node. Shows
/// `loading…` while the file is decoded, and the error if it cannot be.
/// Call it while the app is built or running.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::image;
///
/// // A 4x4 red square.
/// let path = std::env::temp_dir().join(format!("intuituive-image-doc-{}.png", std::process::id()));
/// rich_art::image::RgbImage::from_pixel(4, 4, rich_art::image::Rgb([255, 0, 0]))
///     .save(&path)
///     .unwrap();
/// let shown = path.clone();
/// let app = App::new(move || image(&shown).fixed(2).on_key("q", |cx| cx.quit()))
///     .wait_for_tasks(true);
/// let screen = app.render_with(&["q"], 4, 2).unwrap();
/// assert!(screen[0].starts_with("▀▀"));
/// # std::fs::remove_file(&path).ok();
/// ```
pub fn image(path: impl AsRef<Path>) -> Node {
    image_with(path, ImageView::default())
}

/// [`image`] drawn as `view` says: another cell mode, a fit, a background.
pub fn image_with(path: impl AsRef<Path>, view: ImageView) -> Node {
    let path = path.as_ref().to_path_buf();
    image_of(move || Some(path.clone()), view)
}

/// The lines last drawn, with the picture and the size they were for.
type Drawn = (Arc<DynamicImage>, u16, u16, Vec<Vec<Segment>>);

/// An image node following `path`, a closure that may read signals: when
/// the path it returns changes, the new file is decoded in the background
/// (a result for a path already left behind is dropped). `None` draws
/// nothing.
///
/// ```no_run
/// use intuituive::prelude::*;
/// use intuituive::{image_of, ImageView};
///
/// App::new(|| {
///     let selected = signal(Some(std::path::PathBuf::from("photo.png")));
///     image_of(move || selected.get(), ImageView::new())
/// })
/// .run()
/// # .unwrap();
/// ```
pub fn image_of(path: impl Fn() -> Option<PathBuf> + 'static, view: ImageView) -> Node {
    image_from(move || path().map(Input::Path), view)
}

/// The image in `bytes` (a PNG, a JPEG, any format rs-rich-art reads),
/// drawn as `view` says: a cover embedded in a song, a picture from a
/// network or an archive. Decoded on a worker thread, as a file is.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{image_bytes, ImageView};
/// use std::io::Cursor;
///
/// let mut png = Vec::new();
/// rich_art::image::RgbImage::from_pixel(4, 4, rich_art::image::Rgb([0, 255, 0]))
///     .write_to(&mut Cursor::new(&mut png), rich_art::image::ImageFormat::Png)
///     .unwrap();
/// let app = App::new(move || image_bytes(png.clone(), ImageView::new()).fixed(2)).wait_for_tasks(true);
/// let screen = app.render_with(&[], 4, 2).unwrap();
/// assert!(screen[0].starts_with("▀▀"));
/// ```
pub fn image_bytes(bytes: impl Into<Arc<[u8]>>, view: ImageView) -> Node {
    let bytes: Arc<[u8]> = bytes.into();
    image_from(move || Some(Input::Bytes(bytes.clone())), view)
}

/// An image node following `bytes`, a closure that may read signals (the
/// cover of the track playing): a new picture is decoded when it returns
/// other bytes (another `Arc`), `None` draws nothing.
pub fn image_bytes_of(bytes: impl Fn() -> Option<Arc<[u8]>> + 'static, view: ImageView) -> Node {
    image_from(move || bytes().map(Input::Bytes), view)
}

/// Where an image node's picture comes from.
#[derive(Clone, Debug)]
enum Input {
    Path(PathBuf),
    Bytes(Arc<[u8]>),
}

impl PartialEq for Input {
    /// The same file, or the same bytes in memory (not a copy of them).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Input::Path(a), Input::Path(b)) => a == b,
            (Input::Bytes(a), Input::Bytes(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// An image node over `input`, decoded in the background when it changes.
fn image_from(input: impl Fn() -> Option<Input> + 'static, view: ImageView) -> Node {
    let picture: Signal<Picture> = signal(Picture::Empty);
    let generation = signal(0u64);
    watch(input, move |input, _| {
        generation.update(|g| *g += 1);
        let mine = generation.get_untracked();
        let Some(input) = input else {
            picture.set(Picture::Empty);
            return;
        };
        picture.set(Picture::Loading);
        spawn(
            move || decode(&input),
            move |result, _| {
                if generation.get_untracked() == mine {
                    picture.set(match result {
                        Ok(image) => Picture::Ready(Arc::new(image)),
                        Err(error) => Picture::Failed(error),
                    });
                }
            },
        );
    });
    let drawn: RefCell<Option<Drawn>> = RefCell::new(None);
    leaf(move |console, width, height| {
        let current = picture.get();
        let image = match current {
            Picture::Empty => return Vec::new(),
            Picture::Loading => return note("loading…", "dim"),
            Picture::Failed(error) => return note(&format!("cannot show image: {error}"), "red"),
            Picture::Ready(image) => image,
        };
        if let Some((last, w, h, lines)) = drawn.borrow().as_ref() {
            if Arc::ptr_eq(last, &image) && (*w, *h) == (width, height) {
                return lines.clone();
            }
        }
        let lines = draw(console, &image, view, width, height);
        *drawn.borrow_mut() = Some((image, width, height, lines.clone()));
        lines
    })
    .what("image")
}

/// Decode the picture, shrunk to at most [`KEEP`] pixels a side.
fn decode(input: &Input) -> Result<DynamicImage, String> {
    let image = match input {
        Input::Path(path) => rich_art::image::open(path),
        Input::Bytes(bytes) => rich_art::image::load_from_memory(bytes),
    }
    .map_err(|e| e.to_string())?;
    Ok(if image.width() > KEEP || image.height() > KEEP {
        image.thumbnail(KEEP, KEEP)
    } else {
        image
    })
}

/// One line of `text` in `style`.
fn note(text: &str, style: &str) -> Vec<Vec<Segment>> {
    vec![vec![Segment::new(
        text.to_string(),
        Style::parse(style).ok(),
    )]]
}

/// `image` in `width` × `height` cells (a height of 0 measures: as tall as
/// the image is at `width`).
fn draw(
    console: &rich::Console,
    image: &Arc<DynamicImage>,
    view: ImageView,
    width: u16,
    height: u16,
) -> Vec<Vec<Segment>> {
    if width == 0 {
        return Vec::new();
    }
    let mode = match view.mode {
        // Pixel protocols cannot reach the terminal through the cell screen.
        ImageMode::Auto | ImageMode::Sixel => ImageMode::Blocks,
        mode => mode,
    };
    let mut art = ImageArt::new(DynamicImage::clone(image))
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
        lines = tag(console, image, view.pixels, lines);
    }
    lines
}

/// The key a cell of an image drawn in pixels carries in its style's meta:
/// `[id, cols, rows, row, protocol]`.
const IMAGE_META_KEY: &str = "intuituive.image";

/// The pictures drawn in pixels, by id, for [`image_graphics`] to find
/// (weakly: a picture no node holds goes).
static PICTURES: Mutex<Vec<(u32, Weak<DynamicImage>)>> = Mutex::new(Vec::new());
static NEXT_PICTURE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// `image`'s id among the pictures drawn in pixels: 1 to 2^24 - 1, as
/// Kitty's colour-coded ids take.
fn picture_id(image: &Arc<DynamicImage>) -> u32 {
    let mut pictures = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
    pictures.retain(|(_, weak)| weak.strong_count() > 0);
    if let Some((id, _)) = pictures
        .iter()
        .find(|(_, weak)| weak.upgrade().is_some_and(|p| Arc::ptr_eq(&p, image)))
    {
        return *id;
    }
    let id = NEXT_PICTURE
        .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        .wrapping_sub(1)
        % 0x00ff_fffe
        + 1;
    pictures.push((id, Arc::downgrade(image)));
    id
}

fn picture(id: u32) -> Option<Arc<DynamicImage>> {
    let pictures = PICTURES.lock().unwrap_or_else(|e| e.into_inner());
    pictures
        .iter()
        .find(|(i, _)| *i == id)
        .and_then(|(_, weak)| weak.upgrade())
}

/// The cells of a picture drawn in pixels: tagged for [`image_graphics`],
/// and for Kitty, its placeholders.
fn tag(
    console: &rich::Console,
    image: &Arc<DynamicImage>,
    pixels: Pixels,
    lines: Vec<Vec<Segment>>,
) -> Vec<Vec<Segment>> {
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
    let id = picture_id(image);
    let meta = |row: usize| {
        let mut meta = Meta::new();
        meta.insert(
            IMAGE_META_KEY,
            MetaValue::List(vec![
                MetaValue::Int(i64::from(id)),
                MetaValue::Int(cols as i64),
                MetaValue::Int(rows as i64),
                MetaValue::Int(row as i64),
                MetaValue::Int(pixels.code()),
            ]),
        );
        Style::from_meta(meta)
    };
    if pixels == Pixels::Kitty {
        let truecolor = console.color_system() == Some(rich::color::ColorSystem::Truecolor);
        let Some(colour) = rich_art::kitty::id_color(id, truecolor) else {
            return lines;
        };
        return (0..rows)
            .map(|row| {
                let cells = rich_art::kitty::placeholder_row(row, cols).unwrap_or_default();
                let style = Style::new().with_color(colour.clone()).combine(&meta(row));
                vec![Segment::new(cells, Some(style))]
            })
            .collect();
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(row, line)| {
            let tag = meta(row);
            line.into_iter()
                .map(|mut segment| {
                    segment.style = Some(segment.style.unwrap_or_default().combine(&tag));
                    segment
                })
                .collect()
        })
        .collect()
}

/// What a tagged run of cells says: the picture, its size, which of its
/// rows, and the protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Tag {
    id: u32,
    cols: usize,
    rows: usize,
    row: usize,
    pixels: Pixels,
}

impl Tag {
    fn of(style: &Style) -> Option<Tag> {
        let MetaValue::List(items) = style.meta_ref()?.get(IMAGE_META_KEY)? else {
            return None;
        };
        let int = |i: usize| match items.get(i) {
            Some(MetaValue::Int(v)) => Some(*v),
            _ => None,
        };
        Some(Tag {
            id: u32::try_from(int(0)?).ok()?,
            cols: usize::try_from(int(1)?).ok()?,
            rows: usize::try_from(int(2)?).ok()?,
            row: usize::try_from(int(3)?).ok()?,
            pixels: Pixels::from_code(int(4)?)?,
        })
    }
}

/// The graphics source that draws image nodes' pixels
/// ([`ImageView::pixels`]): give it to
/// [`App::graphics`](crate::App::graphics). After each frame's cells it
/// finds the nodes' tagged cells and draws their pictures over them
/// (Sixel, iTerm2) or uploads them for their placeholders (Kitty), only
/// when one is new, moved or had cells written under it; the Kitty
/// pictures are freed when the app ends.
///
/// ```no_run
/// use intuituive::prelude::*;
/// use intuituive::{image_graphics, image_with, ImageView, Pixels};
///
/// App::new(|| image_with("cover.png", ImageView::new().pixels(Pixels::detect())))
///     .graphics(image_graphics())
///     .run()
/// # .unwrap();
/// ```
pub fn image_graphics() -> Arc<dyn PlacementSource> {
    Arc::new(ImageGraphics {
        made: Mutex::new(HashMap::new()),
        uploaded: Mutex::new(Vec::new()),
    })
}

/// A picture at a size in a protocol: picture, columns, rows, protocol.
type GraphicKey = (u32, usize, usize, Pixels);

/// [`image_graphics`]'s source.
struct ImageGraphics {
    /// The graphics made, by picture, size and protocol.
    made: Mutex<HashMap<GraphicKey, Arc<dyn Graphic>>>,
    /// The Kitty pictures uploaded, to free.
    uploaded: Mutex<Vec<u32>>,
}

impl PlacementSource for ImageGraphics {
    fn placements(&self, frame: &Frame) -> Vec<Placement> {
        // Each tagged run: its row, column and tag.
        let mut found: Vec<(usize, usize, Tag)> = Vec::new();
        for row in 0..frame.height() {
            let mut column = 0;
            let mut open: Option<(usize, Tag)> = None;
            for run in frame.row(row) {
                let tag = frame.styles().get(run.style()).and_then(Tag::of);
                match (&open, tag) {
                    (Some((_, a)), Some(b)) if *a == b => {}
                    _ => {
                        if let Some((start, tag)) = open.take() {
                            found.push((row, start, tag));
                        }
                        open = tag.map(|tag| (column, tag));
                    }
                }
                column += run.cells();
            }
            if let Some((start, tag)) = open {
                found.push((row, start, tag));
            }
        }
        let has = |row: usize, column: usize, tag: &Tag, which: usize| {
            found
                .iter()
                .any(|(r, c, t)| *r == row && *c == column && t.id == tag.id && t.row == which)
        };
        let mut out = Vec::new();
        for (row, column, tag) in &found {
            // From a picture's first row, all its rows there (Kitty's
            // placeholders need only the upload).
            let whole =
                tag.row == 0 && (1..tag.rows).all(|below| has(row + below, *column, tag, below));
            if !whole && !(tag.pixels == Pixels::Kitty && tag.row == 0) {
                continue;
            }
            if let Some(graphic) = self.graphic(tag) {
                out.push(Placement {
                    row: *row,
                    column: *column,
                    cols: tag.cols,
                    rows: if whole { tag.rows } else { 1 },
                    graphic,
                    frame: 0,
                });
            }
        }
        out
    }

    fn release(&self, retained: &[Placement]) -> String {
        let _ = retained;
        let uploaded =
            std::mem::take(&mut *self.uploaded.lock().unwrap_or_else(|e| e.into_inner()));
        uploaded.into_iter().map(rich_art::kitty::delete).collect()
    }
}

impl ImageGraphics {
    /// The graphic of `tag`'s picture at its size, made once.
    fn graphic(&self, tag: &Tag) -> Option<Arc<dyn Graphic>> {
        let key = (tag.id, tag.cols, tag.rows, tag.pixels);
        let mut made = self.made.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(graphic) = made.get(&key) {
            return Some(graphic.clone());
        }
        let image = picture(tag.id)?;
        let escape = encode(&image, tag)?;
        if tag.pixels == Pixels::Kitty {
            self.uploaded
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(tag.id);
        }
        let graphic: Arc<dyn Graphic> = Arc::new(PixelGraphic {
            key: {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut hasher);
                hasher.finish()
            },
            escape,
            in_cells: tag.pixels == Pixels::Kitty,
        });
        made.retain(|(id, ..), _| picture(*id).is_some());
        made.insert(key, graphic.clone());
        Some(graphic)
    }
}

/// The escape that draws `image` in `tag`'s cells.
fn encode(image: &DynamicImage, tag: &Tag) -> Option<String> {
    use rich_art::graphics::{encode_png, fit_to_cells, CellPixels};
    match tag.pixels {
        Pixels::Cells => None,
        Pixels::Kitty => {
            let png = encode_png(&fit_to_cells(
                image,
                tag.cols,
                tag.rows,
                CellPixels::DEFAULT,
            ))?;
            Some(rich_art::kitty::transmit(tag.id, &png, tag.cols, tag.rows))
        }
        Pixels::Sixel => {
            let sixel = rich_art::SixelArt::new(image.clone()).encode_cells(tag.cols, tag.rows)?;
            Some(format!("\x1b7{sixel}\x1b8"))
        }
        Pixels::Iterm => {
            let png = encode_png(&fit_to_cells(
                image,
                tag.cols,
                tag.rows,
                CellPixels::DEFAULT,
            ))?;
            Some(format!(
                "\x1b7{}\x1b8",
                rich_art::iterm::inline_image(&png, tag.cols, tag.rows)
            ))
        }
    }
}

/// A picture drawn by a protocol: its escape, made once.
#[derive(Debug)]
struct PixelGraphic {
    key: u64,
    escape: String,
    in_cells: bool,
}

impl Graphic for PixelGraphic {
    fn key(&self) -> u64 {
        self.key
    }

    fn draw(&self, _frame: usize) -> String {
        self.escape.clone()
    }

    fn in_cells(&self) -> bool {
        self.in_cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use rich_art::image::{Rgb, RgbImage, Rgba, RgbaImage};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("intuituive-image-{}-{name}", std::process::id()))
    }

    fn save(path: &Path, width: u32, height: u32) {
        RgbImage::from_fn(width, height, |x, _| {
            if x < width / 2 {
                Rgb([255, 0, 0])
            } else {
                Rgb([0, 0, 255])
            }
        })
        .save(path)
        .unwrap();
    }

    #[test]
    fn draws_in_half_blocks_within_its_rect() {
        let path = temp("wide.png");
        // 40x10 pixels: at 20 columns (cells twice as tall as wide), 3
        // rows of half blocks.
        save(&path, 40, 10);
        let shown = path.clone();
        let app = App::new(move || image(&shown).on_key("q", |cx| cx.quit())).wait_for_tasks(true);
        let screen = app.render_with(&["q"], 20, 8).unwrap();
        assert!(screen[0].starts_with(&"▀".repeat(20)), "{screen:?}");
        assert!(screen[2].starts_with('▀'), "{screen:?}");
        // Kept to its aspect: nothing below.
        assert_eq!(screen[3].trim_end(), "");
        // Shrunk to fit a short rect, narrower than its width.
        let shown = path.clone();
        let app = App::new(move || image(&shown).on_key("q", |cx| cx.quit())).wait_for_tasks(true);
        let screen = app.render_with(&["q"], 20, 2).unwrap();
        assert!(screen[0].trim_end().chars().count() < 20, "{screen:?}");
        assert!(screen[0].starts_with('▀'));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_cover_fit_fills_the_rect() {
        let path = temp("cover.png");
        save(&path, 10, 40);
        let shown = path.clone();
        let view = ImageView::new().fit(ImageFit::Cover);
        let app = App::new(move || image_with(&shown, view).on_key("q", |cx| cx.quit()))
            .wait_for_tasks(true);
        let screen = app.render_with(&["q"], 12, 3).unwrap();
        for row in &screen {
            assert_eq!(row.chars().filter(|c| *c == '▀').count(), 12, "{screen:?}");
        }
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn other_cell_modes_and_errors() {
        let path = temp("modes.png");
        RgbaImage::from_pixel(8, 8, Rgba([255, 255, 255, 255]))
            .save(&path)
            .unwrap();
        let shown = path.clone();
        let view = ImageView::new().mode(ImageMode::Braille);
        let app = App::new(move || image_with(&shown, view).on_key("q", |cx| cx.quit()))
            .wait_for_tasks(true);
        let screen = app.render_with(&["q"], 4, 2).unwrap();
        assert!(
            screen[0]
                .chars()
                .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
            "{screen:?}"
        );
        // A file that is not an image.
        let bad = temp("bad.png");
        std::fs::write(&bad, b"not a picture").unwrap();
        let shown = bad.clone();
        let app = App::new(move || image(&shown).on_key("q", |cx| cx.quit())).wait_for_tasks(true);
        let screen = app.render_with(&["q"], 40, 1).unwrap();
        assert!(screen[0].starts_with("cannot show image"), "{screen:?}");
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&bad).ok();
    }

    #[test]
    fn a_decode_is_a_task_the_driver_waits_for() {
        // An image node built only once a key is pressed (a preview pane
        // that switches to an image): one press draws it, with no polling.
        let path = temp("later.png");
        RgbImage::from_pixel(4, 4, Rgb([255, 0, 0]))
            .save(&path)
            .unwrap();
        let shown = path.clone();
        let app = App::new(move || {
            let open = signal(false);
            let shown = shown.clone();
            crate::switch(
                move || open.get(),
                move |open| match open {
                    true => image(&shown).fixed(2),
                    false => crate::label("none"),
                },
            )
            .on_key("o", move |_| open.set(true))
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(4, 2);
        driver.press("o");
        assert!(
            driver.screen().plain()[0].starts_with("▀▀"),
            "{:?}",
            driver.screen().plain()
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn follows_a_changing_path() {
        let red = temp("red.png");
        let blue = temp("blue.png");
        RgbImage::from_pixel(4, 4, Rgb([255, 0, 0]))
            .save(&red)
            .unwrap();
        RgbImage::from_pixel(4, 4, Rgb([0, 0, 255]))
            .save(&blue)
            .unwrap();
        let (a, b) = (red.clone(), blue.clone());
        let app = App::new(move || {
            let which = signal(Some(a.clone()));
            let other = b.clone();
            image_of(move || which.get(), ImageView::new())
                .fixed(2)
                .on_key("n", move |_| which.set(Some(other.clone())))
                .on_key("x", move |_| which.set(None))
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(4, 2);
        driver.settle();
        let row = |driver: &crate::Driver| driver.screen().plain()[0].clone();
        assert!(row(&driver).starts_with("▀▀"));
        driver.press("n");
        assert!(row(&driver).starts_with("▀▀"));
        driver.press("x");
        assert_eq!(row(&driver).trim_end(), "");
        std::fs::remove_file(&red).ok();
        std::fs::remove_file(&blue).ok();
    }

    #[test]
    fn bytes_in_memory_are_drawn_as_a_file_is() {
        use std::io::Cursor;
        let mut png = Vec::new();
        RgbImage::from_pixel(4, 4, Rgb([255, 0, 0]))
            .write_to(
                &mut Cursor::new(&mut png),
                rich_art::image::ImageFormat::Png,
            )
            .unwrap();
        let bytes: Arc<[u8]> = png.into();
        let app = App::new(move || {
            let cover = signal(Some(bytes.clone()));
            image_bytes_of(move || cover.get(), ImageView::new())
                .fixed(2)
                .on_key("x", move |_| cover.set(None))
        })
        .wait_for_tasks(true);
        let mut driver = app.driver(4, 2);
        driver.settle();
        assert!(driver.screen().plain()[0].starts_with("▀▀"));
        driver.press("x");
        assert_eq!(driver.screen().plain()[0].trim_end(), "");
        // Bytes that are no picture say so.
        let app =
            App::new(|| image_bytes(b"nothing".to_vec(), ImageView::new())).wait_for_tasks(true);
        let screen = app.render_with(&[], 40, 1).unwrap();
        assert!(screen[0].starts_with("cannot show image"), "{screen:?}");
    }

    #[test]
    fn sixel_and_iterm_pictures_are_drawn_over_their_half_blocks() {
        for (pixels, starts) in [
            (Pixels::Sixel, "\x1b7\x1bP"),
            (Pixels::Iterm, "\x1b7\x1b]1337;"),
        ] {
            let path = temp(&format!("pixels-{pixels:?}.png"));
            save(&path, 8, 8);
            let shown = path.clone();
            let app =
                App::new(move || image_with(&shown, ImageView::new().pixels(pixels)).fixed(2))
                    .wait_for_tasks(true)
                    .graphics(image_graphics());
            let mut driver = app.driver(4, 2);
            // What is written while the picture is decoded and drawn.
            let mut out = String::new();
            for _ in 0..400 {
                driver.update(std::time::Duration::ZERO);
                out.push_str(&driver.render().unwrap_or_default());
                if out.contains(starts) {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(out.contains(starts), "{pixels:?}: {out:?}");
            // The half blocks stay in the cells, for a terminal without it.
            assert!(driver.screen().plain()[0].starts_with("▀▀"));
            std::fs::remove_file(&path).ok();
        }
    }

    #[test]
    fn kitty_cells_are_placeholders_and_the_picture_is_uploaded_and_freed() {
        let path = temp("kitty.png");
        save(&path, 8, 8);
        let shown = path.clone();
        let app =
            App::new(move || image_with(&shown, ImageView::new().pixels(Pixels::Kitty)).fixed(2))
                .wait_for_tasks(true)
                .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        driver.settle();
        let row = driver.screen().plain()[0].clone();
        assert!(row.starts_with(rich_art::kitty::PLACEHOLDER), "{row:?}");
        let _ = driver.render();
        let finished = driver.finish();
        assert!(finished.contains("a=d"), "freed: {finished:?}");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn tags_read_back() {
        let image = Arc::new(DynamicImage::new_rgb8(2, 2));
        let console = rich::Console::builder().width(4).build();
        let lines = vec![vec![Segment::new("▀▀", None)]];
        let tagged = tag(&console, &image, Pixels::Sixel, lines);
        let tag = Tag::of(tagged[0][0].style.as_ref().unwrap()).unwrap();
        assert_eq!(
            (tag.cols, tag.rows, tag.row, tag.pixels),
            (2, 1, 0, Pixels::Sixel)
        );
        assert!(picture(tag.id).is_some_and(|p| Arc::ptr_eq(&p, &image)));
    }
}

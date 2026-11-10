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
//!
//! The pictures themselves are rs-rich-art's [`Picture`]s: decoded once,
//! known to the terminal by an id, uploaded once and placed by reference.
//! [`picture_of`] is a node over a live picture, a new frame of which
//! ([`Picture::next`]: a video, an emulator's screen) is drawn at once
//! under the same id. An app can also draw pictures inside rows of its own
//! with [`Picture::lines`]; [`image_graphics`] finds their cells wherever
//! they are. [`picture_bytes`] says what a picture's escapes have cost.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rich::Style;
use rich::segment::Segment;
pub use rich_art::picture::{
    ImageView, Picture, PictureEncoding, PixelFormats, Pixels, picture_bytes,
};
use rich_art::picture::{PictureTag, count_picture_bytes};
pub use rich_art::{ImageAnchor, ImageFit, ImageMode};
pub use rich_ext::capabilities::KittyFeatures;
use rich_ext::frame::{Frame, Graphic, Placement, PlacementSource};

use crate::node::{Node, leaf};
use crate::reactive::{Signal, signal, watch};
use crate::task::spawn;

/// One terminal's immutable image policy, shared by first paint and workers.
/// Use explicit constructors for known capabilities; protocol names alone do
/// not establish Kitty placeholder or native-animation support.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImagePresentation {
    pixels: Pixels,
    kitty: KittyFeatures,
    formats: PixelFormats,
}

impl ImagePresentation {
    pub fn detect() -> Self {
        Self::from_environment(&rich_ext::capabilities::SystemEnvironment)
    }

    pub fn from_environment(env: &dyn rich_ext::capabilities::Environment) -> Self {
        use rich_ext::capabilities::{Capabilities, Graphics};
        let report = Capabilities::detect(env);
        let features = Capabilities::graphics_features(env);
        let pixels = match report.graphics.value {
            Graphics::None => Pixels::Cells,
            Graphics::Kitty => Pixels::Kitty,
            Graphics::Iterm => Pixels::Iterm,
            Graphics::Sixel => Pixels::Sixel,
        };
        let mut formats = PixelFormats::cells();
        if features.unicode_placeholders.value {
            formats = formats.with(Pixels::Kitty);
        }
        if features.iterm.value {
            formats = formats.with(Pixels::Iterm);
        }
        if features.sixel.value {
            formats = formats.with(Pixels::Sixel);
        }
        Self::new(pixels, features.kitty_features(), formats)
    }

    pub fn new(pixels: Pixels, kitty: KittyFeatures, formats: PixelFormats) -> Self {
        let pixels = if formats.contains(pixels)
            && (pixels != Pixels::Kitty || kitty.unicode_placeholders)
        {
            pixels
        } else {
            Pixels::Cells
        };
        Self {
            pixels,
            kitty,
            formats,
        }
    }
    pub fn pixels(self) -> Pixels {
        self.pixels
    }
    pub fn kitty_features(self) -> KittyFeatures {
        self.kitty
    }
    pub fn formats(self) -> PixelFormats {
        self.formats
    }
    /// Explicitly select a codec. Kitty still requires independent placeholder evidence.
    #[must_use]
    pub fn with_pixels(self, pixels: Pixels) -> Self {
        Self::new(pixels, self.kitty, self.formats.with(pixels))
    }
    #[must_use]
    pub fn with_native_kitty_animation(mut self, allowed: bool) -> Self {
        self.kitty.native_animation = allowed;
        self
    }
    pub fn encoding(self, cell: rich_art::graphics::CellPixels) -> PictureEncoding {
        PictureEncoding::new(cell).native_kitty_animation(self.kitty.native_animation)
    }
    /// Bind a logical picture to independent fitted/versioned resources. Use
    /// the returned handle with stock `picture_of`, `tiles_of` or `Picture::lines`.
    pub fn picture(self, picture: &Picture) -> Picture {
        picture.bound(self.kitty.native_animation)
    }

    pub fn graphics(self) -> Arc<dyn PlacementSource> {
        graphics_with_animation(self.kitty.native_animation)
    }
}

/// What an image node has to draw.
#[derive(Clone, Default, PartialEq)]
enum Decoded {
    /// No path.
    #[default]
    Empty,
    Loading,
    Ready(Picture),
    Failed(String),
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
type Drawn = (
    Picture,
    u16,
    u16,
    ImageView,
    Option<(u16, u16)>,
    Option<rich::ColorSystem>,
    Vec<Vec<Segment>>,
);

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
    let picture: Signal<Decoded> = signal(Decoded::Empty);
    let generation = signal(0u64);
    watch(input, move |input, _| {
        generation.update(|g| *g += 1);
        let mine = generation.get_untracked();
        let Some(input) = input else {
            picture.set(Decoded::Empty);
            return;
        };
        picture.set(Decoded::Loading);
        spawn(
            move || decode(&input),
            move |result, _| {
                if generation.get_untracked() == mine {
                    picture.set(match result {
                        Ok(image) => Decoded::Ready(image),
                        Err(error) => Decoded::Failed(error),
                    });
                }
            },
        );
    });
    let drawn: RefCell<Option<Drawn>> = RefCell::new(None);
    leaf(move |console, width, height| {
        let current = picture.get();
        let image = match current {
            Decoded::Empty => return Vec::new(),
            Decoded::Loading => return note("loading…", "dim"),
            Decoded::Failed(error) => return note(&format!("cannot show image: {error}"), "red"),
            Decoded::Ready(image) => image,
        };
        draw_cached(&drawn, console, &image, view, width, height)
    })
    .what("image")
}

/// `picture` drawn at `width` x `height`, from `drawn` when it was drawn
/// there last time.
fn draw_cached(
    drawn: &RefCell<Option<Drawn>>,
    console: &rich::Console,
    picture: &Picture,
    view: ImageView,
    width: u16,
    height: u16,
) -> Vec<Vec<Segment>> {
    let pixels = crate::cell_pixels().get();
    if let Some((last, w, h, old_view, old_pixels, color, lines)) = drawn.borrow().as_ref()
        && last == picture
        && (*w, *h) == (width, height)
        && *old_view == view
        && *old_pixels == pixels
        && *color == console.color_system()
    {
        return lines.clone();
    }
    let cell = pixels
        .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
        .unwrap_or_default();
    let lines = picture.lines_with_metrics(console, view, width, height, cell);
    *drawn.borrow_mut() = Some((
        picture.clone(),
        width,
        height,
        view,
        pixels,
        console.color_system(),
        lines.clone(),
    ));
    lines
}

/// A node showing the picture `picture` returns, a closure that may read
/// signals: each new frame (a video's, an emulator's screen) is drawn at
/// once, with no worker and no "loading" step, and a frame of the same
/// picture ([`Picture::next`]) keeps its id, so Kitty replaces the image
/// it holds. `None` draws nothing.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{picture_of, ImageView, Picture};
/// use rich_art::image::{DynamicImage, Rgb, RgbImage};
///
/// let app = App::new(|| {
///     let frame = signal(Some(Picture::new(DynamicImage::ImageRgb8(
///         RgbImage::from_pixel(4, 4, Rgb([255, 0, 0])),
///     ))));
///     picture_of(move || frame.get(), ImageView::new()).fixed(2)
/// });
/// let screen = app.render_with(&[], 4, 2).unwrap();
/// assert!(screen[0].starts_with("▀▀"));
/// ```
pub fn picture_of(picture: impl Fn() -> Option<Picture> + 'static, view: ImageView) -> Node {
    let drawn: RefCell<Option<Drawn>> = RefCell::new(None);
    leaf(move |console, width, height| match picture() {
        None => Vec::new(),
        Some(picture) => draw_cached(&drawn, console, &picture, view, width, height),
    })
    .what("image")
}

/// A tile's picture version and the lines drawn of it.
type TileLines = (
    u32,
    usize,
    usize,
    Option<(u16, u16)>,
    Option<rich::ColorSystem>,
    Vec<Vec<Segment>>,
);

/// A node over a whole frame cut into tiles (#818): `tiles` returns them
/// (a closure that may read signals; [`rich_art::tiles::Tiler::tiles`]),
/// each drawn at its place in cells through `pixels`. A tile that did not
/// change keeps its picture's id and version, so with [`image_graphics`]
/// only the tiles that changed reach the terminal again (Kitty's uploads
/// for its placeholders). A tile that does not fit the node is left out.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{Pixels, tiles_of};
/// use rich_art::image::{Rgba, RgbaImage};
/// use rich_art::tiles::Tiler;
///
/// let mut tiler = Tiler::new(2, 1);
/// tiler.update(&RgbaImage::from_pixel(32, 16, Rgba([255, 0, 0, 255])), 4, 1);
/// let tiles = tiler.tiles().to_vec();
/// let app = App::new(move || {
///     let tiles = tiles.clone();
///     tiles_of(move || tiles.clone(), Pixels::Cells).fixed(1)
/// });
/// let screen = app.render_with(&[], 4, 1).unwrap();
/// assert_eq!(screen[0].chars().count(), 4);
/// ```
pub fn tiles_of(tiles: impl Fn() -> Vec<rich_art::tiles::Tile> + 'static, pixels: Pixels) -> Node {
    // Each tile's lines, by picture id, with the version they are of.
    let drawn: RefCell<HashMap<u32, TileLines>> = RefCell::new(HashMap::new());
    leaf(move |console, width, height| {
        let (width, height) = (width as usize, height as usize);
        let metrics = crate::cell_pixels().get();
        let cell = metrics
            .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
            .unwrap_or_default();
        let mut tiles: Vec<_> = tiles()
            .into_iter()
            .filter(|t| t.col + t.cols <= width && t.row + t.rows <= height)
            .collect();
        tiles.sort_by_key(|t| (t.row, t.col));
        let view = ImageView::new().pixels(pixels).fit(ImageFit::Stretch);
        let mut drawn = drawn.borrow_mut();
        drawn.retain(|id, _| tiles.iter().any(|t| t.picture.id() == *id));
        let mut rows: Vec<Vec<Segment>> = vec![Vec::new(); height];
        let mut filled = vec![0usize; height];
        for tile in &tiles {
            let picture = &tile.picture;
            let fresh = drawn.get(&picture.id()).is_none_or(
                |(version, cols, rows, previous_metrics, color, _)| {
                    *version != picture.version()
                        || (*cols, *rows) != (tile.cols, tile.rows)
                        || *previous_metrics != metrics
                        || *color != console.color_system()
                },
            );
            if fresh {
                let lines = picture.lines_with_metrics(
                    console,
                    view,
                    tile.cols as u16,
                    tile.rows as u16,
                    cell,
                );
                drawn.insert(
                    picture.id(),
                    (
                        picture.version(),
                        tile.cols,
                        tile.rows,
                        metrics,
                        console.color_system(),
                        lines,
                    ),
                );
            }
            let (_, _, _, _, _, lines) = &drawn[&picture.id()];
            for (k, line) in lines.iter().enumerate().take(tile.rows) {
                let row = tile.row + k;
                if filled[row] < tile.col {
                    rows[row].push(Segment::new(" ".repeat(tile.col - filled[row]), None));
                }
                rows[row].extend(line.iter().cloned());
                filled[row] = tile.col + tile.cols;
            }
        }
        rows
    })
    .what("tiles")
}

/// Decode the picture, shrunk to at most [`rich_art::picture::KEEP`]
/// pixels a side.
fn decode(input: &Input) -> Result<Picture, String> {
    match input {
        Input::Path(path) => rich_art::image::open(path)
            .map_err(|e| e.to_string())
            .map(|image| Picture::new(rich_art::picture::shrink(image))),
        Input::Bytes(bytes) => Picture::decode(bytes),
    }
}

/// One line of `text` in `style`.
fn note(text: &str, style: &str) -> Vec<Vec<Segment>> {
    vec![vec![Segment::new(
        text.to_string(),
        Style::parse(style).ok(),
    )]]
}

/// The graphics source that draws pictures' pixels
/// ([`ImageView::pixels`]): give it to
/// [`App::graphics`](crate::App::graphics). After each frame's cells it
/// finds the pictures' tagged cells (image nodes', [`picture_of`]'s, and
/// any drawn with [`Picture::lines`]) and draws them over the cells
/// (Sixel, iTerm2) or uploads them for their placeholders (Kitty), only
/// when one is new, moved, a new frame, or had cells written under it.
/// The Kitty pictures are freed when the app ends.
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
    graphics_with_animation(true)
}

fn graphics_with_animation(native_animation: bool) -> Arc<dyn PlacementSource> {
    Arc::new(ImageGraphics {
        native_animation,
        retired: Mutex::new(Vec::new()),
        made: Mutex::new(HashMap::new()),
        uploaded: Mutex::new(Vec::new()),
        owners: Mutex::new(Vec::new()),
    })
}

/// A picture's frame at a size in a protocol, and the rows of it shown:
/// picture, version, columns, rows, protocol, first row shown, rows shown.
type GraphicKey = (
    u32,
    u32,
    usize,
    usize,
    Pixels,
    usize,
    usize,
    Option<ImageFit>,
    ImageAnchor,
    rich_art::graphics::CellPixels,
    (usize, usize, Option<[u8; 3]>, Option<bool>),
);

/// [`image_graphics`]'s source.
struct ImageGraphics {
    native_animation: bool,
    retired: Mutex<Vec<Arc<rich_art::picture::KittyResource>>>,
    /// The graphics made, by picture, size and protocol.
    made: Mutex<HashMap<GraphicKey, Arc<PixelGraphic>>>,
    /// The Kitty pictures uploaded, to free.
    uploaded: Mutex<Vec<(Arc<rich_art::picture::KittyResource>, bool, u32)>>,
    /// Weak identities of Kitty graphics, including graphics evicted from the cache.
    owners: Mutex<Vec<(u32, std::sync::Weak<PixelGraphic>)>>,
}

#[derive(Clone)]
struct ImageRect {
    row: usize,
    column: usize,
    x: usize,
    cols: usize,
    rows: usize,
    tag: PictureTag,
    bound: Option<bool>,
    known: bool,
    background: Option<[u8; 3]>,
}

impl ImageRect {
    fn instance(&self) -> (PictureTag, Option<[u8; 3]>, Option<bool>, i128, i128) {
        let mut tag = self.tag;
        tag.row = 0;
        (
            tag,
            self.background,
            self.bound,
            self.column as i128 - self.x as i128,
            self.row as i128 - self.tag.row as i128,
        )
    }
}

// Kitty already embeds source coordinates in every placeholder. A composed
// screen can coalesce adjacent copies into one style run, so a column restart
// begins another instance. Validate the entire run before accepting any spans;
// malformed Kitty provenance must not become a legacy whole-width fallback.
fn kitty_spans(
    mut chars: impl Iterator<Item = char>,
    tag: &PictureTag,
    cells: usize,
) -> Option<Vec<std::ops::Range<usize>>> {
    use rich_art::kitty::{DIACRITICS, PLACEHOLDER};
    let row = *DIACRITICS.get(tag.row)?;
    if cells == 0 || chars.next()? != PLACEHOLDER || chars.next()? != row {
        return None;
    }
    let column = chars.next()?;
    let first = DIACRITICS.iter().position(|&c| c == column)?;
    if first >= tag.cols {
        return None;
    }
    let mut spans = Vec::new();
    spans.push(first..first + 1);
    let mut count = 1;
    while let Some(c) = chars.next() {
        if count >= cells || c != PLACEHOLDER || chars.next()? != row {
            return None;
        }
        let column = chars.next()?;
        let span = spans.last_mut()?;
        if span.end < tag.cols && DIACRITICS.get(span.end) == Some(&column) {
            span.end += 1;
        } else if let Some(column) = DIACRITICS.iter().position(|&candidate| candidate == column)
            && column < span.end
        {
            // Composed adjacent copies can both start inside a cropped view.
            // Equal/backward restarts retain exact per-cell provenance; forward
            // jumps remain malformed rather than inventing missing source cells.
            spans.push(column..column + 1);
        } else {
            return None;
        }
        count += 1;
    }
    // At most one span per actual composed cell, with one linear scan and no
    // per-cell style or encoded resource. All spans retain the full-view key.
    (count == cells).then_some(spans)
}

impl PlacementSource for ImageGraphics {
    fn placements(&self, frame: &Frame) -> Vec<Placement> {
        self.placements_with_metrics(frame, None)
    }

    fn placements_with_metrics(
        &self,
        frame: &Frame,
        metrics: Option<(u16, u16)>,
    ) -> Vec<Placement> {
        self.placements_from_rows(
            (0..frame.height()).map(|row| {
                frame.row(row).iter().map(|run| ImageRun {
                    style: frame.styles().get(run.style()),
                    cells: run.cells(),
                    chars: frame.run_text(run).chars(),
                })
            }),
            metrics,
        )
    }

    fn placements_from_screen(
        &self,
        screen: &rich_ext::surface::Screen,
        metrics: Option<(u16, u16)>,
    ) -> Vec<Placement> {
        self.placements_from_rows(
            (0..screen.area().height).map(|row| screen_runs(screen, row)),
            metrics,
        )
    }

    fn reconcile(&self, retained: &[Placement]) -> String {
        self.release_images(retained, true)
    }

    fn output_written(&self) {
        self.retired
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    fn release(&self, retained: &[Placement]) -> String {
        self.release_images(retained, false)
    }
}

// Both transports expose the same equal-style row runs. Character iterators
// keep Kitty's source-coordinate validation independent of owned text storage.
struct ImageRun<'a, C> {
    style: Option<&'a rich::Style>,
    cells: usize,
    chars: C,
}

fn screen_runs(
    screen: &rich_ext::surface::Screen,
    row: u16,
) -> impl Iterator<Item = ImageRun<'_, impl Iterator<Item = char>>> {
    let mut x = 0;
    std::iter::from_fn(move || {
        let width = screen.area().width;
        while x < width && screen.cell(x, row).is_continuation() {
            x += 1;
        }
        if x == width {
            return None;
        }
        let start = x;
        let id = screen.cell(x, row).style;
        let mut cells = 0;
        let mut joined = false;
        while x < width {
            let cell = screen.cell(x, row);
            if !cell.is_continuation() {
                if cell.style != id {
                    break;
                }
                cells += usize::from(cell.width);
                joined |= cell.text().contains(['\u{200d}', '\u{fe0f}']);
            }
            x += 1;
        }
        let end = x;
        let chars = move || {
            (start..end)
                .filter_map(move |column| {
                    let cell = screen.cell(column, row);
                    (!cell.is_continuation()).then_some(cell.text())
                })
                .flat_map(str::chars)
        };
        // A joiner or selector may cross a cell boundary after composition.
        // Match Frame's width of the complete exported run in that rare case.
        if joined {
            cells = rich::cells::cell_len(&chars().collect::<String>());
        }
        Some(ImageRun {
            style: screen.style(id),
            cells,
            chars: chars(),
        })
    })
}

impl ImageGraphics {
    fn placements_from_rows<'a, Rows, Runs, Chars>(
        &self,
        rows: Rows,
        metrics: Option<(u16, u16)>,
    ) -> Vec<Placement>
    where
        Rows: IntoIterator<Item = Runs>,
        Runs: IntoIterator<Item = ImageRun<'a, Chars>>,
        Chars: Iterator<Item = char>,
    {
        let cell = metrics
            .filter(|&(w, h)| w > 0 && h > 0)
            .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
            .unwrap_or_default();
        // Recover exact surviving row spans, then merge only equal spans on
        // consecutive source AND screen rows. Never bridge a dialog or a hole.
        let mut rectangles: Vec<ImageRect> = Vec::new();
        let mut previous: HashMap<_, usize> = HashMap::new();
        let mut counts = HashMap::new();
        let mut rejected = std::collections::HashSet::new();
        let mut overlay_count = 0;
        let mut overlay_overflow = false;
        for (row, runs) in rows.into_iter().enumerate() {
            let mut spans: Vec<ImageRect> = Vec::new();
            let mut column = 0;
            for run in runs {
                let style = run.style;
                let tag = style.and_then(PictureTag::of);
                if let Some(tag) = tag.filter(|t| t.cols > 0 && t.rows > t.row) {
                    let background = style.and_then(PictureTag::background_of);
                    let bound = style.and_then(PictureTag::binding_of);
                    let mut append = |column, x, cols, known| {
                        if let Some(last) = spans.last_mut()
                            && last.tag == tag
                            && last.bound == bound
                            && last.known == known
                            && last.background == background
                            && last.column + last.cols == column
                            && (if known { last.x + last.cols == x } else { true })
                        {
                            last.cols += cols;
                        } else {
                            spans.push(ImageRect {
                                row,
                                column,
                                x,
                                cols,
                                rows: 1,
                                tag,
                                bound,
                                known,
                                background,
                            });
                        }
                    };
                    if tag.pixels == Pixels::Kitty {
                        if let Some(ranges) = kitty_spans(run.chars, &tag, run.cells) {
                            let mut offset = 0;
                            for range in ranges {
                                let cols = range.end - range.start;
                                append(column + offset, range.start, cols, true);
                                offset += cols;
                            }
                        }
                    } else if let Some(x) = style.and_then(PictureTag::column_of) {
                        // Native-overlay provenance names the source column
                        // of each cell. Equal columns can coalesce across
                        // adjacent copies (notably 1x1 views), so each cell
                        // starts its own span until contiguous sources merge.
                        if x < tag.cols {
                            for offset in 0..run.cells {
                                append(column + offset, x, 1, true);
                            }
                        }
                    } else if run.cells <= tag.cols {
                        append(column, 0, run.cells, false);
                    }
                }
                column += run.cells;
            }
            let mut current = HashMap::new();
            for span in spans {
                if !span.known && span.cols != span.tag.cols {
                    continue;
                }
                let instance = span.instance();
                if rejected.contains(&instance) {
                    continue;
                }
                let key = (instance, span.x, span.cols);
                if let Some(&index) = previous.get(&key) {
                    rectangles[index].rows += 1;
                    current.insert(key, index);
                    continue;
                }
                if span.tag.pixels != Pixels::Kitty {
                    if overlay_overflow {
                        continue;
                    }
                    let count = counts.entry(instance).or_insert(0);
                    *count += 1;
                    if *count > 32 {
                        rejected.insert(instance);
                        continue;
                    }
                    overlay_count += 1;
                    if overlay_count > 128 {
                        // A pathological frame keeps its original cell fallback.
                        overlay_overflow = true;
                        continue;
                    }
                }
                current.insert(key, rectangles.len());
                rectangles.push(span);
            }
            previous = current;
        }
        let mut out = Vec::new();
        let mut kitty: HashMap<u32, Vec<Arc<PixelGraphic>>> = HashMap::new();
        for rect in rectangles {
            if rejected.contains(&rect.instance())
                || (overlay_overflow && rect.tag.pixels != Pixels::Kitty)
            {
                continue;
            }
            let tag = &rect.tag;
            let (x, cols, first, rows) = if tag.pixels == Pixels::Kitty {
                (0, tag.cols, 0, tag.rows)
            } else {
                (rect.x, rect.cols, tag.row, rect.rows)
            };
            if let Some(graphic) = self.graphic(
                tag,
                x..x + cols,
                first..first + rows,
                cell,
                rect.background,
                rect.bound,
            ) {
                if tag.pixels == Pixels::Kitty {
                    kitty
                        .entry(graphic.resource.as_ref().unwrap().id())
                        .or_default()
                        .push(graphic.clone());
                }
                out.push(Placement {
                    row: rect.row,
                    column: rect.column,
                    cols: rect.cols,
                    rows: rect.rows,
                    graphic,
                    frame: 0,
                });
            }
        }
        // One Kitty id can still alias simultaneous different fitted views.
        // Never infer retained data for an observed competing upload. This flag
        // also reaches previous placements sharing these cached graphics.
        for variants in kitty.values() {
            if variants.iter().any(|g| g.key != variants[0].key) {
                for graphic in variants {
                    graphic
                        .reusable
                        .store(false, std::sync::atomic::Ordering::Relaxed);
                }
            }
        }
        out
    }
}

impl ImageGraphics {
    fn release_images(&self, retained: &[Placement], managed_only: bool) -> String {
        let mut made = self.made.lock().unwrap_or_else(|e| e.into_inner());
        let mut uploaded = self.uploaded.lock().unwrap_or_else(|e| e.into_inner());
        let mut owners = self.owners.lock().unwrap_or_else(|e| e.into_inner());
        let kept: std::collections::HashSet<u32> = owners
            .iter()
            .filter_map(|(id, graphic)| {
                let graphic: Arc<dyn Graphic> = graphic.upgrade()?;
                retained
                    .iter()
                    .any(|p| Arc::ptr_eq(&p.graphic, &graphic))
                    .then_some(*id)
            })
            .collect();
        let released: Vec<_> = uploaded
            .iter()
            .filter(|(r, bound, _)| (!managed_only || *bound) && !kept.contains(&r.id()))
            .map(|(r, _, _)| r.id())
            .collect();
        // A caller may still hold previous placements after release. They no
        // longer prove that data is present; invalidate their reuse eligibility,
        // including graphics already evicted from the strong encoding cache.
        for (id, weak) in owners.iter() {
            if released.contains(id)
                && let Some(graphic) = weak.upgrade()
            {
                graphic
                    .reusable
                    .store(false, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let mut retired = self.retired.lock().unwrap_or_else(|e| e.into_inner());
        let mut cleanup = String::new();
        uploaded.retain(|(resource, _, logical_id)| {
            if released.contains(&resource.id()) {
                let delete = Picture::release(resource.id());
                count_picture_bytes(*logical_id, delete.len() as u64);
                cleanup.push_str(&delete);
                retired.push(resource.clone());
                false
            } else {
                true
            }
        });
        owners.retain(|(id, weak)| !released.contains(id) && weak.strong_count() > 0);
        made.retain(|_, graphic| {
            !graphic
                .resource
                .as_ref()
                .is_some_and(|r| released.contains(&r.id()))
        });
        cleanup
    }
}

impl ImageGraphics {
    /// The graphic of `tag`'s frame at its size, its rows `first` to
    /// `first + shown` only, made once.
    fn graphic(
        &self,
        tag: &PictureTag,
        columns: std::ops::Range<usize>,
        rows: std::ops::Range<usize>,
        cell: rich_art::graphics::CellPixels,
        background: Option<[u8; 3]>,
        bound: Option<bool>,
    ) -> Option<Arc<PixelGraphic>> {
        let (x, cols, first, shown) = (columns.start, columns.len(), rows.start, rows.len());
        let key = (
            tag.id,
            tag.version,
            tag.cols,
            tag.rows,
            tag.pixels,
            first,
            shown,
            tag.fit,
            tag.anchor,
            cell,
            (x, cols, background, bound),
        );
        let mut made = self.made.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(graphic) = made.get(&key) {
            return Some(graphic.clone());
        }
        let picture = tag.picture()?;
        let picture = bound.map_or_else(|| picture.clone(), |native| picture.bound(native));
        let resource = if tag.pixels == Pixels::Kitty {
            Some(picture.kitty_resource(
                ImageView {
                    fit: tag.fit,
                    anchor: tag.anchor,
                    pixels: tag.pixels,
                    ..ImageView::new()
                },
                tag.cols,
                tag.rows,
                true,
            )?)
        } else {
            None
        };
        let mut escape = picture.encode_tag_rect_with_context(
            tag,
            x..x + cols,
            first..first + shown,
            PictureEncoding::new(cell).native_kitty_animation(self.native_animation),
        )?;
        if tag.pixels != Pixels::Kitty {
            // Only accepted, encoded rectangles lose their fallback. Clearing
            // before native alpha blending prevents sampled half-block colors
            // showing through transparent PNG/Sixel pixels. Restore cursor and
            // attributes BEFORE the encoder uses the same save/restore slot.
            let mut clear = String::from("\x1b7\x1b[0m");
            if let Some([r, g, b]) = background {
                clear.push_str(&format!("\x1b[48;2;{r};{g};{b}m"));
            }
            for row in 0..shown {
                if row > 0 {
                    clear.push_str("\x1b[1B");
                }
                clear.push_str(&format!("\x1b[{cols}X"));
            }
            clear.push_str("\x1b8");
            clear.push_str(&escape);
            escape = clear;
        }
        if tag.pixels == Pixels::Kitty {
            let mut uploaded = self.uploaded.lock().unwrap_or_else(|e| e.into_inner());
            let resource = resource.as_ref().unwrap();
            if !uploaded.iter().any(|(r, _, _)| r.id() == resource.id()) {
                uploaded.push((resource.clone(), bound.is_some(), tag.id));
            }
        }
        let graphic = Arc::new(PixelGraphic {
            key: {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                key.hash(&mut hasher);
                hasher.finish()
            },
            escape,
            resource,
            in_cells: tag.pixels == Pixels::Kitty,
            id: tag.id,
            reusable: std::sync::atomic::AtomicBool::new(true),
        });
        made.retain(|(id, version, ..), _| Picture::find(*id, *version).is_some());
        if tag.pixels == Pixels::Kitty {
            let mut owners = self.owners.lock().unwrap_or_else(|e| e.into_inner());
            owners.retain(|(_, weak)| weak.strong_count() > 0);
            owners.push((
                graphic.resource.as_ref().unwrap().id(),
                Arc::downgrade(&graphic),
            ));
        }
        // Bound live-picture crop history as well as old versions. Clearing the
        // encoding cache does not release retained Kitty resources: owners track
        // those independently, including evicted graphics.
        const CACHE_BYTES: usize = 16 * 1024 * 1024;
        if made.len() >= 128
            || made
                .values()
                .map(|g| g.escape.len())
                .sum::<usize>()
                .saturating_add(graphic.escape.len())
                > CACHE_BYTES
        {
            made.clear();
        }
        if graphic.escape.len() <= CACHE_BYTES {
            made.insert(key, graphic.clone());
        }
        Some(graphic)
    }
}

/// A picture drawn by a protocol: its escape, made once.
#[derive(Debug)]
struct PixelGraphic {
    resource: Option<Arc<rich_art::picture::KittyResource>>,
    key: u64,
    escape: String,
    in_cells: bool,
    /// The picture it draws, whose bytes it counts.
    id: u32,
    reusable: std::sync::atomic::AtomicBool,
}

impl Graphic for PixelGraphic {
    fn key(&self) -> u64 {
        self.key
    }

    fn draw(&self, _frame: usize) -> String {
        count_picture_bytes(self.id, self.escape.len() as u64);
        self.escape.clone()
    }

    fn in_cells(&self) -> bool {
        self.in_cells
    }

    fn can_reuse_data(&self) -> bool {
        self.reusable.load(std::sync::atomic::Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use rich_art::image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};

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

    fn tracked_source() -> ImageGraphics {
        ImageGraphics {
            native_animation: true,
            retired: Mutex::new(Vec::new()),
            made: Mutex::new(HashMap::new()),
            uploaded: Mutex::new(Vec::new()),
            owners: Mutex::new(Vec::new()),
        }
    }

    fn kitty_placements(source: &ImageGraphics, picture: &Picture) -> Vec<Placement> {
        let console = rich::Console::builder()
            .width(2)
            .color_system(Some(rich::color::ColorSystem::Truecolor))
            .build();
        let mut segments = Vec::new();
        for line in picture.lines(&console, ImageView::new().pixels(Pixels::Kitty), 2, 2) {
            segments.extend(line);
            segments.push(Segment::line());
        }
        source.placements(&Frame::from_segments(&segments))
    }

    #[test]
    fn managed_cleanup_counts_logical_bytes_and_waits_for_write_confirmation() {
        let source = tracked_source();
        let first = Picture::from_rgba(4, 4, vec![255; 64])
            .unwrap()
            .bound(false);
        let id = first.id();
        let old = kitty_placements(&source, &first);
        assert_eq!(
            picture_bytes(id),
            0,
            "placement/encoding is side-effect free"
        );
        let upload = old[0].graphic.draw(0);
        assert_eq!(picture_bytes(id), upload.len() as u64);
        let second = first.next_rgba(4, 4, vec![60; 64]).unwrap();
        let next = kitty_placements(&source, &second);
        let next_upload = next[0].graphic.draw(0);
        let cleanup = source.reconcile(&next);
        assert!(cleanup.contains("a=d"));
        assert_eq!(
            picture_bytes(id),
            (upload.len() + next_upload.len() + cleanup.len()) as u64
        );
        assert_eq!(source.uploaded.lock().unwrap().len(), 1);
        assert_eq!(source.retired.lock().unwrap().len(), 1);
        assert!(!old[0].graphic.can_reuse_data());
        assert!(source.reconcile(&next).is_empty());
        assert_eq!(
            source.retired.lock().unwrap().len(),
            1,
            "unconfirmed write must retain the reservation"
        );
        source.output_written();
        assert!(source.retired.lock().unwrap().is_empty());
        let finish = source.release(&[]);
        assert_eq!(
            picture_bytes(id),
            (upload.len() + next_upload.len() + cleanup.len() + finish.len()) as u64
        );
        assert_eq!(source.retired.lock().unwrap().len(), 1);
        source.output_written();
        assert!(source.retired.lock().unwrap().is_empty());
    }

    #[test]
    fn encoded_cache_byte_budget_evicts_before_entry_budget() {
        let source = tracked_source();
        // Noise, opaque: raw Kitty frames are zlib-compressed, and a flat
        // picture would shrink far under the sizes this test needs.
        let mut seed = 0x2545_f491_u32;
        let noise: Vec<u8> = (0..640 * 1280)
            .flat_map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let [r, g, b, _] = seed.to_le_bytes();
                [r, g, b, 255]
            })
            .collect();
        let picture = Picture::from_rgba(640, 1280, noise).unwrap();
        for cols in 64..74 {
            let tag = PictureTag {
                id: picture.id(),
                version: picture.version(),
                cols,
                rows: 72,
                row: 0,
                pixels: Pixels::Kitty,
                fit: Some(ImageFit::Stretch),
                anchor: ImageAnchor::Center,
            };
            let graphic = source
                .graphic(
                    &tag,
                    0..cols,
                    0..72,
                    rich_art::graphics::CellPixels::DEFAULT,
                    None,
                    None,
                )
                .unwrap();
            assert!(
                graphic.escape.len() > 2 * 1024 * 1024,
                "{}",
                graphic.escape.len()
            );
            let made = source.made.lock().unwrap();
            assert!(
                made.values()
                    .map(|graphic| graphic.escape.len())
                    .sum::<usize>()
                    <= 16 * 1024 * 1024
            );
        }
        assert!(
            source.made.lock().unwrap().len() < 10,
            "byte budget must evict before 128 entries"
        );
    }

    #[test]
    fn live_picture_crop_cache_is_bounded() {
        let source = tracked_source();
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let tag = PictureTag {
            id: picture.id(),
            version: picture.version(),
            cols: 16,
            rows: 16,
            row: 0,
            pixels: Pixels::Iterm,
            fit: Some(ImageFit::Stretch),
            anchor: ImageAnchor::Center,
        };
        for row in 0..16 {
            for column in 0..16 {
                assert!(
                    source
                        .graphic(
                            &tag,
                            column..column + 1,
                            row..row + 1,
                            rich_art::graphics::CellPixels::DEFAULT,
                            None,
                            None,
                        )
                        .is_some()
                );
                let made = source.made.lock().unwrap();
                assert!(made.len() <= 128);
                assert!(
                    made.values()
                        .map(|graphic| graphic.escape.len())
                        .sum::<usize>()
                        <= 16 * 1024 * 1024
                );
            }
        }
    }

    #[test]
    fn kitty_owner_tracking_stays_bounded_as_old_versions_are_dropped() {
        let source = tracked_source();
        let mut picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        for version in 0..100u8 {
            if version != 0 {
                picture = picture.next_rgba(4, 4, vec![version; 64]).unwrap();
            }
            let placements = kitty_placements(&source, &picture);
            assert_eq!(placements.len(), 1);
            drop(placements);
        }
        assert_eq!(source.made.lock().unwrap().len(), 1);
        assert_eq!(
            source.owners.lock().unwrap().len(),
            1,
            "dead owner records accumulate"
        );
    }

    #[test]
    fn retained_evicted_kitty_graphic_still_prevents_deletion() {
        let source = tracked_source();
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let id = picture.id();
        let old_version = picture.version();
        let retained = kitty_placements(&source, &picture);
        assert_eq!(retained.len(), 1);
        let next = picture.next_rgba(4, 4, vec![1; 64]).unwrap();
        drop(picture);
        let mut picture = next;
        for version in 2..6u8 {
            let placements = kitty_placements(&source, &picture);
            assert_eq!(placements.len(), 1);
            drop(placements);
            picture = picture.next_rgba(4, 4, vec![version; 64]).unwrap();
        }
        let current = kitty_placements(&source, &picture);
        assert_eq!(current.len(), 1);
        assert!(
            !source
                .made
                .lock()
                .unwrap()
                .keys()
                .any(|(key_id, version, ..)| *key_id == id && *version == old_version)
        );
        assert!(source.release(&retained).is_empty());
        assert_eq!(source.release(&[]), Picture::release(id));
    }

    #[test]
    fn release_preserves_retained_kitty_picture_until_later_release() {
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let source = image_graphics();
        let console = rich::Console::builder()
            .width(2)
            .color_system(Some(rich::color::ColorSystem::Truecolor))
            .build();
        let mut segments = Vec::new();
        for line in picture.lines(&console, ImageView::new().pixels(Pixels::Kitty), 2, 2) {
            segments.extend(line);
            segments.push(Segment::line());
        }
        let placements = source.placements(&Frame::from_segments(&segments));
        assert_eq!(placements.len(), 1);
        assert!(source.release(&placements).is_empty());
        assert!(source.release(&[]).contains("a=d"));
    }

    #[test]
    fn graphics_cache_distinguishes_fit_and_anchor() {
        let picture = Picture::new(DynamicImage::ImageRgb8(RgbImage::from_fn(12, 4, |x, _| {
            Rgb([x as u8 * 20, 0, 0])
        })));
        let source = image_graphics();
        let console = rich::Console::builder().width(2).build();
        let view = ImageView::new().pixels(Pixels::Iterm).fit(ImageFit::Cover);
        let placements = |view| {
            let mut segments = Vec::new();
            for line in picture.lines(&console, view, 2, 2) {
                segments.extend(line);
                segments.push(Segment::line());
            }
            source.placements(&Frame::from_segments(&segments))
        };
        let left = placements(view.anchor(ImageAnchor::Left));
        let same = placements(view.anchor(ImageAnchor::Left));
        let right = placements(view.anchor(ImageAnchor::Right));
        assert_eq!(left.len(), 1);
        assert_eq!(right.len(), 1);
        assert!(
            rich_ext::frame::graphics::plan(&left, &same, &[])
                .draw
                .is_empty()
        );
        let update = rich_ext::frame::graphics::plan(&left, &right, &[]);
        assert_eq!(update.draw.len(), 1);
        let raster = |escape: &str| {
            let payload = escape
                .split_once(':')
                .unwrap()
                .1
                .split('\u{7}')
                .next()
                .unwrap();
            let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut bytes = Vec::new();
            let (mut bits, mut count) = (0u32, 0);
            for byte in payload.bytes().take_while(|b| *b != b'=') {
                let value = alphabet.iter().position(|b| *b == byte).unwrap() as u32;
                bits = (bits << 6) | value;
                count += 6;
                if count >= 8 {
                    count -= 8;
                    bytes.push((bits >> count) as u8);
                }
            }
            rich_art::image::load_from_memory(&bytes)
                .unwrap()
                .to_rgba8()
        };
        let left_pixels = raster(&left[0].graphic.draw(0));
        let right_pixels = raster(&update.draw[0].graphic.draw(0));
        assert_eq!(left_pixels.dimensions(), (16, 32));
        assert_eq!(right_pixels.dimensions(), (16, 32));
        assert!(left_pixels.get_pixel(8, 16)[0] < right_pixels.get_pixel(8, 16)[0]);
        let stretch = placements(view.fit(ImageFit::Stretch));
        assert_eq!(
            rich_ext::frame::graphics::plan(&right, &stretch, &[])
                .draw
                .len(),
            1
        );
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
    fn a_picture_partly_in_view_is_drawn_clipped_to_its_rows() {
        let path = temp("partly.png");
        save(&path, 8, 16);
        let shown = path.clone();
        let offset = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = offset.clone();
        let app = App::new(move || {
            let at = crate::reactive::signal(0u16);
            seen.set(Some(at));
            crate::node::scroll_with(
                image_with(&shown, ImageView::new().pixels(Pixels::Iterm)).fixed(4),
                at,
            )
        })
        .wait_for_tasks(true)
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let mut out = String::new();
        for _ in 0..400 {
            driver.update(std::time::Duration::ZERO);
            out.push_str(&driver.render().unwrap_or_default());
            if out.contains("\x1b]1337;") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // Its top two rows of four, then its middle two once scrolled.
        assert!(out.contains("height=2;"), "{out:?}");
        assert!(!out.contains("height=4;"), "{out:?}");
        offset.get().unwrap().set(1);
        driver.update(std::time::Duration::ZERO);
        let scrolled = driver.render().unwrap_or_default();
        assert!(scrolled.contains("height=2;"), "{scrolled:?}");
        std::fs::remove_file(&path).ok();
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

    /// What the driver writes over `rounds` updates, stopping once it
    /// holds `until`.
    fn written(driver: &mut crate::Driver, until: &str, rounds: usize) -> String {
        let mut out = String::new();
        for _ in 0..rounds {
            driver.update(std::time::Duration::ZERO);
            out.push_str(&driver.render().unwrap_or_default());
            if out.contains(until) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        out
    }

    fn solid(r: u8, g: u8, b: u8) -> DynamicImage {
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([r, g, b])))
    }

    #[test]
    fn driver_reuploads_picture_when_only_cell_pixels_change() {
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let shown = picture.clone();
        let app = App::new(move || {
            picture_of(
                move || Some(shown.clone()),
                ImageView::new()
                    .fit(ImageFit::Stretch)
                    .pixels(Pixels::Kitty),
            )
            .fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let first = written(&mut driver, "a=T", 5);
        assert!(first.contains("s=32,v=32"), "{first:?}");
        driver.set_cell_pixels(Some((12, 24)));
        let changed = written(&mut driver, "a=T", 5);
        assert!(changed.contains("s=48,v=48"), "{changed:?}");
        assert!(changed.contains(&format!("i={},", picture.id())));
    }

    #[test]
    fn graphics_uploads_are_inside_synchronized_frame_when_enabled() {
        use rich_interact::headless::{Headless, Script};
        for synchronized in [false, true] {
            let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
            let app = App::new(move || {
                picture_of(
                    {
                        let picture = picture.clone();
                        move || Some(picture.clone())
                    },
                    ImageView::new().pixels(Pixels::Kitty),
                )
                .fixed(2)
                .on_key("q", |cx| cx.quit())
            })
            .graphics(image_graphics());
            let mut backend = Headless::new(Script::new().keys("q"), 4, 2);
            backend.synchronized_output = synchronized;
            let record = backend.record();
            app.run_on(&mut backend).unwrap();
            let writes = &record.borrow().writes;
            let frame = writes.iter().find(|text| text.contains("a=T")).unwrap();
            assert_eq!(frame.starts_with("\x1b[?2026h"), synchronized);
            assert_eq!(frame.ends_with("\x1b[?2026l"), synchronized);
        }
    }

    #[test]
    fn headless_propagates_metric_changes_without_a_cell_resize() {
        use rich_interact::headless::{Headless, Script};
        let picture = Picture::from_rgba(4, 4, vec![255; 64]).unwrap();
        let shown = picture.clone();
        let app = App::new(move || {
            picture_of(
                move || Some(shown.clone()),
                ImageView::new()
                    .fit(ImageFit::Stretch)
                    .pixels(Pixels::Kitty),
            )
            .fixed(2)
            .on_key("q", |cx| cx.quit())
        })
        .graphics(image_graphics());
        use rich_interact::{Backend, Event};
        struct MetricBackend {
            inner: Headless,
            next: std::collections::VecDeque<(u16, u16)>,
        }
        impl Backend for MetricBackend {
            fn size(&self) -> (u16, u16) {
                self.inner.size()
            }
            fn cell_pixels(&self) -> Option<(u16, u16)> {
                self.inner.cell_pixels()
            }
            fn read(
                &mut self,
                timeout: Option<std::time::Duration>,
            ) -> std::io::Result<Option<Event>> {
                let event = self.inner.read(timeout)?;
                if matches!(event, Some(Event::Resize { .. })) {
                    self.inner.set_cell_pixels(self.next.pop_front());
                }
                Ok(event)
            }
            fn write(&mut self, text: &str) -> std::io::Result<()> {
                self.inner.write(text)
            }
            fn elapsed(&self) -> std::time::Duration {
                self.inner.elapsed()
            }
            fn handoff(
                &mut self,
                command: &mut std::process::Command,
            ) -> std::io::Result<Option<i32>> {
                self.inner.handoff(command)
            }
            fn alternate_screen(&self) -> bool {
                self.inner.alternate_screen()
            }
        }
        let script = Script::new().resize(4, 2).resize(4, 2).keys("q");
        let inner = Headless::new(script, 4, 2);
        let record = inner.record();
        let mut backend = MetricBackend {
            inner,
            next: [(10, 20), (12, 24)].into(),
        };
        app.run_on(&mut backend).unwrap();
        let output = record.borrow().output();
        assert!(output.contains("s=32,v=32"));
        assert!(output.contains("s=40,v=40"));
        assert!(output.contains("s=48,v=48"));
    }

    #[test]
    fn live_frames_keep_one_kitty_id_and_are_freed_once() {
        let first = Picture::new(solid(255, 0, 0));
        let id = first.id();
        let shown = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = shown.clone();
        let start = first.clone();
        let app = App::new(move || {
            let frame = signal(Some(start.clone()));
            seen.set(Some(frame));
            picture_of(move || frame.get(), ImageView::new().pixels(Pixels::Kitty)).fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let out = written(&mut driver, "a=T", 50);
        assert!(out.contains(&format!("i={id},")), "{out:?}");
        let frame = shown.get().unwrap();
        let mut last = first;
        for k in 0..20u8 {
            last = last.next(solid(k * 10, 0, 255));
            frame.set(Some(last.clone()));
            let out = written(&mut driver, "a=T", 5);
            // Every frame goes up under the picture's one id.
            assert!(
                out.contains(&format!("a=T,U=1,f=100,t=d,i={id},")),
                "{k}: {out:?}"
            );
        }
        assert!(picture_bytes(id) > 0);
        let finished = driver.finish();
        assert_eq!(finished.matches("a=d").count(), 1, "{finished:?}");
    }

    #[test]
    fn tile_cache_rebuilds_when_cell_geometry_changes() {
        let mut tiler = rich_art::tiles::Tiler::new(2, 1);
        tiler.update(&RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255])), 2, 1);
        let mut wider = tiler.tiles()[0].clone();
        let first = wider.clone();
        wider.cols = 4;
        let handle = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = handle.clone();
        let app = App::new(move || {
            let tile = signal(first.clone());
            seen.set(Some(tile));
            tiles_of(move || vec![tile.get()], Pixels::Kitty).fixed(1)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 1);
        driver.update(std::time::Duration::ZERO);
        let _ = driver.render();
        assert_eq!(
            driver.screen().plain()[0]
                .chars()
                .filter(|&c| c == rich_art::kitty::PLACEHOLDER)
                .count(),
            2
        );
        handle.get().unwrap().set(wider);
        driver.update(std::time::Duration::ZERO);
        let _ = driver.render();
        assert_eq!(
            driver.screen().plain()[0]
                .chars()
                .filter(|&c| c == rich_art::kitty::PLACEHOLDER)
                .count(),
            4
        );
    }

    #[test]
    fn one_changed_tile_sends_one_kitty_upload() {
        use rich_art::tiles::Tiler;
        // 16 x 4 cells in tiles of 8 x 4: two tiles.
        let mut tiler = Tiler::new(8, 4);
        let mut frame = RgbaImage::from_pixel(128, 64, Rgba([255, 255, 255, 255]));
        tiler.update(&frame, 16, 4);
        let shown = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = shown.clone();
        let first = tiler.tiles().to_vec();
        let app = App::new(move || {
            let tiles = signal(first.clone());
            seen.set(Some(tiles));
            tiles_of(move || tiles.get(), Pixels::Kitty).fixed(4)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(16, 4);
        let out = written(&mut driver, "\u{0}", 20);
        assert_eq!(out.matches("a=T").count(), 2, "both tiles go up: {out:?}");
        // A dot in the second tile.
        frame.put_pixel(100, 20, Rgba([0, 0, 0, 255]));
        assert_eq!(tiler.update(&frame, 16, 4), [1]);
        let changed = tiler.tiles()[1].picture.id();
        shown.get().unwrap().set(tiler.tiles().to_vec());
        let out = written(&mut driver, "\u{0}", 20);
        assert_eq!(out.matches("a=T").count(), 1, "one tile goes up: {out:?}");
        assert!(out.contains(&format!("i={changed},")), "{out:?}");
        // Nothing changed: nothing goes up.
        shown.get().unwrap().set(tiler.tiles().to_vec());
        let out = written(&mut driver, "\u{0}", 5);
        assert_eq!(out.matches("a=T").count(), 0, "{out:?}");
    }

    #[test]
    fn sixel_frames_are_drawn_again_and_counted() {
        let first = Picture::new(solid(0, 255, 0));
        let id = first.id();
        let shown = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = shown.clone();
        let start = first.clone();
        let app = App::new(move || {
            let frame = signal(Some(start.clone()));
            seen.set(Some(frame));
            picture_of(move || frame.get(), ImageView::new().pixels(Pixels::Sixel)).fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let out = written(&mut driver, "\x1bP", 50);
        assert!(out.contains("\x1bP"), "{out:?}");
        let before = picture_bytes(id);
        assert!(before > 0);
        shown.get().unwrap().set(Some(first.next(solid(0, 0, 255))));
        let out = written(&mut driver, "\x1bP", 5);
        assert!(out.contains("\x1bP"), "{out:?}");
        assert!(picture_bytes(id) > before);
        // A frame that does not change draws nothing more.
        let still = picture_bytes(id);
        let out = written(&mut driver, "\x1bP", 3);
        assert!(!out.contains("\x1bP"));
        assert_eq!(picture_bytes(id), still);
    }

    #[test]
    fn kitty_uploads_a_picture_whose_first_row_is_out_of_view() {
        let picture = Picture::new(solid(9, 9, 9));
        let id = picture.id();
        let console = rich::Console::builder()
            .width(4)
            .color_system(Some(rich::color::ColorSystem::Truecolor))
            .build();
        let lines = picture.lines(&console, ImageView::new().pixels(Pixels::Kitty), 4, 4);
        assert_eq!(lines.len(), 4);
        // Only its last two rows, as a view scrolled past its top shows.
        let shown = lines[2..].to_vec();
        let app = App::new(move || {
            let rows = shown.clone();
            crate::node::leaf(move |_, _, _| rows.clone()).fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let out = written(&mut driver, "a=T", 20);
        assert!(out.contains(&format!("i={id},")), "{out:?}");
    }

    #[test]
    fn kitty_plays_an_animation_and_a_still_stops_it() {
        let frames: Vec<_> = [255u8, 0]
            .iter()
            .map(|&r| rich_art::graphics::AnimationFrame {
                image: RgbaImage::from_pixel(8, 8, Rgba([r, 0, 0, 255])),
                delay: std::time::Duration::from_millis(100),
            })
            .collect();
        let animated = Picture::animated(frames).unwrap();
        assert!(animated.is_animated());
        let still = animated.still();
        assert!(!still.is_animated());
        assert_eq!(still.id(), animated.id());
        let shown = std::rc::Rc::new(std::cell::Cell::new(None));
        let seen = shown.clone();
        let start = animated.clone();
        let app = App::new(move || {
            let frame = signal(Some(start.clone()));
            seen.set(Some(frame));
            picture_of(move || frame.get(), ImageView::new().pixels(Pixels::Kitty)).fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(4, 2);
        let out = written(&mut driver, "s=3", 50);
        // Its frames go up once, and Kitty loops them.
        assert!(out.contains("a=f,"), "{out:?}");
        assert!(out.contains("s=3,v=1"), "{out:?}");
        let id = animated.id();
        let bytes = picture_bytes(id);
        // Nothing more is sent while it plays.
        let out = written(&mut driver, "a=", 5);
        assert!(!out.contains("a=f,"), "{out:?}");
        assert_eq!(picture_bytes(id), bytes);
        // Frozen: the still frame replaces it, with no animation.
        shown.get().unwrap().set(Some(still));
        let out = written(&mut driver, "a=T", 20);
        assert!(
            out.contains(&format!("a=T,U=1,f=100,t=d,i={id},")),
            "{out:?}"
        );
        assert!(!out.contains("s=3"), "{out:?}");
    }

    #[test]
    fn an_overlay_picture_partly_covered_is_not_drawn_over_what_covers_it() {
        let picture = Picture::new(solid(1, 2, 3));
        let console = rich::Console::builder().width(8).build();
        let view = ImageView::new()
            .fit(ImageFit::Stretch)
            .pixels(Pixels::Sixel);
        let mut covered = picture.lines(&console, view, 8, 2);
        // A modal's cells over the right half of its second row.
        covered[1] = Segment::divide(&covered[1], &[4, 8]).remove(0);
        covered[1].push(Segment::new("│box", None));
        let app = App::new(move || {
            let rows = covered.clone();
            crate::node::leaf(move |_, _, _| rows.clone()).fixed(2)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(8, 2);
        let out = written(&mut driver, "\x1bP", 20);
        // Its first row and the uncovered left half below are native pixels.
        assert!(out.contains("\x1bP"), "{out:?}");
        let id = picture.id();
        assert!(picture_bytes(id) > 0);
        assert!(out.contains("\"1;1;64;16"), "{out:?}");
        assert!(!out.contains("\"1;1;64;32"), "{out:?}");
        assert!(out.contains("\"1;1;32;16"), "{out:?}");
    }

    #[test]
    fn tags_read_back() {
        let image = Picture::new(DynamicImage::new_rgb8(2, 2));
        let console = rich::Console::builder().width(4).build();
        let tagged = image.lines(&console, ImageView::new().pixels(Pixels::Sixel), 2, 1);
        let tag = PictureTag::of(tagged[0][0].style.as_ref().unwrap()).unwrap();
        assert_eq!(
            (tag.cols, tag.rows, tag.row, tag.pixels),
            (2, 1, 0, Pixels::Sixel)
        );
        assert_eq!(tag.picture(), Some(image));
    }
}

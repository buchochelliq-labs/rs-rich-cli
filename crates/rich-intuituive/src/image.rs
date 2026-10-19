//! Image files as nodes: a picture drawn in terminal cells by
//! [rs-rich-art](https://docs.rs/rs-rich-art), sized to the node's
//! rectangle. Behind the off-by-default `art` feature.
//!
//! [`image`] shows a file; [`image_of`] follows a path that changes (a
//! file manager's preview of the selected entry). The file is decoded on a
//! worker thread, never on the app's, and drawn again only when the node's
//! size changes. It draws in half blocks by default (two pixels a cell,
//! full colour, in any terminal with truecolor or 256 colours); quadrants,
//! Braille and ASCII are the other cell modes.
//!
//! Pixel protocols (Kitty, Sixel, iTerm2) are not drawn here: they need
//! escape sequences outside the cells, which intuiTUIve's screen does not
//! yet pass through, so [`ImageMode::Sixel`] and [`ImageMode::Auto`] draw
//! half blocks.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rich::segment::Segment;
use rich::Style;
use rich_art::image::DynamicImage;
pub use rich_art::{ImageAnchor, ImageFit, ImageMode};
use rich_art::{ImageArt, ImageBackground};

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
}

impl Default for ImageView {
    fn default() -> Self {
        ImageView {
            mode: ImageMode::Blocks,
            fit: None,
            anchor: ImageAnchor::Center,
            background: None,
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
    let picture: Signal<Picture> = signal(Picture::Empty);
    let generation = signal(0u64);
    watch(path, move |path, _| {
        generation.update(|g| *g += 1);
        let mine = generation.get_untracked();
        let Some(path) = path else {
            picture.set(Picture::Empty);
            return;
        };
        picture.set(Picture::Loading);
        spawn(
            move || decode(&path),
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

/// Decode the file at `path`, shrunk to at most [`KEEP`] pixels a side.
fn decode(path: &Path) -> Result<DynamicImage, String> {
    let image = rich_art::image::open(path).map_err(|e| e.to_string())?;
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
    lines
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
}

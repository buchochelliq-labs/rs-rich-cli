//! Pictures in the views: each media region's picture drawn over its
//! cells, and animated pictures kept moving within a byte budget.
//!
//! Everything here is drawn by our own stack. The page's pictures are
//! rs-rich-art [`Picture`]s, decoded on the tab's worker (the original file
//! the page loaded, else a still of Chrome's pixels); a row of the
//! projection is its text with the rows of the pictures on it spliced in
//! ([`Picture::lines`]), and intuiTUIve's `image_graphics` draws their
//! pixels (Kitty, Sixel, iTerm2) over those cells, clipped to the rows in
//! view. In a terminal without graphics they are half blocks.
//!
//! **Animated GIFs**: Kitty plays an animated picture on its own clock,
//! with no bytes from us per frame. Elsewhere a [`Player`] steps the
//! frames of each one in view, each frame only when its [`Pacer`] (the
//! picture's share of the [`ByteBudget`], at most `[media] fps` a second) has
//! room, stepping down a protocol when the budget is short. Freezing
//! (`media.freeze`) holds every animation on its frame and stops the
//! bytes: Kitty is sent the still frame in place of the animation.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use rich::segment::Segment;
use rich_art::picture::{ByteBudget, Pacer, PixelFormats, Player, picture_bytes};
use rich_intuituive::{ImageFit, ImageView, Picture, Pixels};

use crate::config::Images;
use crate::engine::NodeId;
use crate::layout::Region;
use crate::model::Page;

/// The protocol pictures are drawn with for `images`.
pub fn pixels(images: Images) -> Pixels {
    match images {
        Images::Auto => Pixels::detect(),
        Images::Kitty => Pixels::Kitty,
        Images::Sixel => Pixels::Sixel,
        Images::Iterm2 => Pixels::Iterm,
        Images::Ansi | Images::None => Pixels::Cells,
    }
}

/// How a region's picture is drawn: filling the region Chrome gave it (as
/// `object-fit: cover` does), through `pixels`.
pub fn view(pixels: Pixels) -> ImageView {
    ImageView::new().fit(ImageFit::Cover).pixels(pixels)
}

/// A moving picture's frame as shown now, and the protocol it is drawn
/// with (it may have stepped down from the page's).
#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    pub picture: Picture,
    pub pixels: Pixels,
}

/// The pictures' cells, made once per frame, size and protocol, shared by
/// the rows that show them.
#[derive(Default)]
pub struct Cells {
    presentation: Option<rich_intuituive::ImagePresentation>,
    made: RefCell<HashMap<CellsKey, Rc<Vec<Vec<Segment>>>>>,
}

/// A picture's frame, its size in cells and its protocol.
type CellsKey = (
    u32,
    u32,
    usize,
    usize,
    Pixels,
    Option<(u16, u16)>,
    Option<rich::ColorSystem>,
);

impl Cells {
    /// Share the app's independent fitted-resource and encoding policy.
    pub fn with_presentation(presentation: rich_intuituive::ImagePresentation) -> Self {
        Self {
            presentation: Some(presentation),
            ..Self::default()
        }
    }

    /// `picture`'s cells at `cols` x `rows` through `pixels`.
    pub fn of(
        &self,
        console: &rich::Console,
        picture: &Picture,
        pixels: Pixels,
        cols: usize,
        rows: usize,
    ) -> Rc<Vec<Vec<Segment>>> {
        self.of_with_metrics(console, picture, pixels, cols, rows, None)
    }

    /// Cells drawn using the physical terminal cell dimensions, when known.
    pub fn of_with_metrics(
        &self,
        console: &rich::Console,
        picture: &Picture,
        pixels: Pixels,
        cols: usize,
        rows: usize,
        metrics: Option<(u16, u16)>,
    ) -> Rc<Vec<Vec<Segment>>> {
        let key = (
            picture.id(),
            picture.version(),
            cols,
            rows,
            pixels,
            metrics,
            console.color_system(),
        );
        if let Some(lines) = self.made.borrow().get(&key) {
            return lines.clone();
        }
        // A small picture in a line (one row: an emote, a badge) keeps
        // all of itself; a region fills its box.
        let view = if rows == 1 {
            ImageView::new().pixels(pixels)
        } else {
            view(pixels)
        };
        let cell = metrics
            .filter(|&(w, h)| w > 0 && h > 0)
            .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
            .unwrap_or_default();
        let bound = self.presentation.map(|policy| policy.picture(picture));
        let picture = bound.as_ref().unwrap_or(picture);
        let lines =
            Rc::new(picture.lines_with_metrics(console, view, cols as u16, rows as u16, cell));
        let mut made = self.made.borrow_mut();
        // Old frames go; what is on screen is made again at once.
        if made.len() > 512 {
            made.clear();
        }
        made.insert(key, lines.clone());
        lines
    }
}

/// Row `row` of the projection drawn: `line` (the row's text as segments,
/// `width` cells), with the rows of the pictures of the `regions` on it
/// spliced in where they cover it. `picture` gives a region's picture and
/// protocol, if it has one yet; a region with none keeps its text (the
/// alt text and its shading).
pub fn splice(
    console: &rich::Console,
    cells: &Cells,
    line: Vec<Segment>,
    width: usize,
    row: usize,
    regions: &[Region],
    picture: impl Fn(&Region) -> Option<(Picture, Pixels)>,
) -> Vec<Segment> {
    splice_with_metrics(console, cells, line, width, row, regions, picture, None)
}

/// Splice picture cells using measured physical cell dimensions.
#[allow(
    clippy::too_many_arguments,
    reason = "Additive metrics API preserves the public splice signature and caller compatibility"
)]
pub fn splice_with_metrics(
    console: &rich::Console,
    cells: &Cells,
    line: Vec<Segment>,
    width: usize,
    row: usize,
    regions: &[Region],
    picture: impl Fn(&Region) -> Option<(Picture, Pixels)>,
    metrics: Option<(u16, u16)>,
) -> Vec<Segment> {
    let mut line = Segment::adjust_line_length(&line, width, None);
    let mut on: Vec<&Region> = regions
        .iter()
        .filter(|r| r.on(row) && r.column < width)
        .collect();
    on.sort_by_key(|r| r.column);
    for region in on {
        let Some((shown, pixels)) = picture(region) else {
            continue;
        };
        let cols = region.cols.min(width - region.column);
        let lines =
            cells.of_with_metrics(console, &shown, pixels, region.cols, region.rows, metrics);
        let Some(part) = lines.get(row - region.row) else {
            continue;
        };
        // A picture cut at the screen's right edge would draw past it.
        if cols < region.cols {
            continue;
        }
        let part = Segment::adjust_line_length(part, cols, None);
        let mut pieces = Segment::divide(&line, &[region.column, region.column + cols, width]);
        pieces.resize(3, Vec::new());
        let mut joined = std::mem::take(&mut pieces[0]);
        joined.extend(part);
        joined.extend(std::mem::take(&mut pieces[2]));
        line = joined;
    }
    line
}

/// What a cell of half blocks under a moving picture costs the terminal
/// when a frame changes it: true-colour foreground and background, the
/// character, and the painter's moves, about 25 bytes (measured).
const UNDERLAY: u64 = 25;

/// `line` as a row of the page on `backdrop` (a rich style, `on …`): padded
/// to `width` cells, and every cell without a background of its own (not a
/// picture's half block) given the backdrop's. The page colours rule off
/// draws the projected page on white (a light terminal) or black.
pub fn on_backdrop(line: Vec<Segment>, width: usize, backdrop: &str) -> Vec<Segment> {
    let Ok(back) = rich::Style::parse(backdrop) else {
        return line;
    };
    Segment::adjust_line_length(&line, width, None)
        .into_iter()
        .map(|mut segment| {
            let style = segment.style.take().unwrap_or_default();
            segment.style = Some(if style.bgcolor().is_some() {
                style
            } else {
                back.combine(&style)
            });
            segment
        })
        .collect()
}

/// The moving pictures: a [`Player`] and a [`Pacer`] per animated picture
/// in view, on a terminal that cannot play animations itself.
pub struct Motion {
    budget: ByteBudget,
    native_animation: bool,
    formats: PixelFormats,
    /// Other moving pictures sharing the budget (videos playing).
    others: usize,
    /// How many animations stepped at the last look.
    moving: usize,
    playing: HashMap<NodeId, Moving>,
    /// Frozen animations' still frames for Kitty, by node.
    stills: HashMap<NodeId, (Picture, Picture)>,
    cell: rich_art::graphics::CellPixels,
}

struct Moving {
    /// The animation it plays: a new one (a new page) starts again.
    of: Picture,
    player: Player,
    pacer: Pacer,
    /// [`picture_bytes`] of its id when it last stepped.
    bytes: u64,
    cell: rich_art::graphics::CellPixels,
    geometry: (usize, usize),
    stale_bytes: bool,
}

impl Default for Motion {
    fn default() -> Self {
        Motion::new(ByteBudget::guess())
    }
}

impl Motion {
    pub fn new(budget: ByteBudget) -> Motion {
        Motion {
            budget,
            native_animation: true,
            formats: PixelFormats::all(),
            others: 0,
            moving: 0,
            playing: HashMap::new(),
            stills: HashMap::new(),
            cell: rich_art::graphics::CellPixels::DEFAULT,
        }
    }

    /// Software fallback and supported formats from the same policy as first paint.
    pub fn with_presentation(
        budget: ByteBudget,
        presentation: rich_intuituive::ImagePresentation,
    ) -> Self {
        Self {
            native_animation: presentation.kitty_features().native_animation,
            formats: presentation.formats(),
            ..Self::new(budget)
        }
    }

    /// The budget it keeps to.
    pub fn budget(&self) -> ByteBudget {
        self.budget
    }

    /// Keep to `budget` from now on, sharing it with `others` moving
    /// pictures (videos playing).
    pub fn share_with(&mut self, budget: ByteBudget, others: usize) {
        self.budget = budget;
        self.others = others;
    }

    /// Select the physical cell size used to estimate animated frame costs.
    pub fn set_cell_pixels(&mut self, cell: rich_art::graphics::CellPixels) {
        self.cell = rich_art::graphics::CellPixels::new(cell.width, cell.height);
    }

    /// The projection is hidden: release unshown reservations while keeping
    /// counted bytes in the rolling windows in case it becomes visible again.
    pub(crate) fn hide(&mut self) {
        for moving in self.playing.values_mut() {
            if moving.pacer.cancel_pending() {
                moving.stale_bytes = true;
            }
        }
        self.moving = 0;
    }

    /// How many animations it stepped at its last look: what it takes of
    /// the shared budget. Kitty plays its own, for no bytes.
    pub fn moving_count(&self) -> usize {
        self.moving
    }

    /// Step the animated pictures of `page` whose regions are on rows
    /// `rows` (those in view) at `now`, drawn with `pixels`. `frozen` holds
    /// them all still. Returns each animated picture's frame to show now,
    /// by node (Kitty's: the animation itself, which Kitty plays, or its
    /// still frame when frozen).
    pub fn step(
        &mut self,
        page: &Page,
        regions: &[Region],
        rows: std::ops::Range<usize>,
        pixels: Pixels,
        frozen: bool,
        now: Instant,
    ) -> HashMap<NodeId, Live> {
        let mut out = HashMap::new();
        let in_view: Vec<&Region> = regions
            .iter()
            .filter(|r| r.row < rows.end && rows.start < r.row + r.rows)
            .filter(|r| {
                page.media
                    .get(r.media)
                    .and_then(|m| m.picture.as_ref())
                    .is_some_and(Picture::is_animated)
            })
            .collect();
        // Kitty plays animations itself: only the others share the bytes.
        self.moving = if (pixels == Pixels::Kitty && self.native_animation) || frozen {
            0
        } else {
            in_view.len()
        };
        let share = 1.0 / (self.moving + self.others).max(1) as f64;
        let budget = self.budget;
        let cell = self.cell;
        let mut seen = Vec::new();
        for region in in_view {
            let media = &page.media[region.media];
            let Some(animation) = media.picture.clone() else {
                continue;
            };
            seen.push(media.node);
            if pixels == Pixels::Kitty && self.native_animation {
                // Kitty plays it; frozen, the still frame stands in.
                let picture = if frozen {
                    self.still(media.node, &animation)
                } else {
                    animation
                };
                out.insert(media.node, Live { picture, pixels });
                continue;
            }
            let moving = self.moving(media.node, &animation, region, pixels, now, cell);
            let metrics_changed = moving.cell != cell;
            let geometry_changed = moving.geometry != (region.cols, region.rows);
            let id = moving.player.frame().id();
            let counted = picture_bytes(id);
            let spent = counted.saturating_sub(moving.bytes);
            moving.bytes = counted;
            if spent > 0 {
                moving.pacer.observe_bytes(
                    now,
                    spent + (region.cols * region.rows) as u64 * UNDERLAY,
                    moving.pacer.pixels() != Pixels::Cells
                        && !metrics_changed
                        && !geometry_changed
                        && !moving.stale_bytes,
                );
                moving.stale_bytes = false;
            }
            if metrics_changed || geometry_changed {
                moving.pacer.cancel_pending();
                moving.pacer.set_cell_pixels(cell);
                moving.cell = cell;
                moving.geometry = (region.cols, region.rows);
                // A frame already shown may be counted after this transition.
                moving.stale_bytes = spent == 0;
            }
            // Short of bytes from the start (its estimate under the floor):
            // step down before sending a frame at all.
            let before = moving.pacer.pixels();
            while moving.pacer.settle(&budget, share) {}
            if moving.pacer.pixels() != before {
                moving.pacer.cancel_pending();
                moving.stale_bytes = true;
            }
            if !frozen && moving.player.due(now) {
                // Retain the displayed frame's reservation until the new
                // frame has actually been admitted as its replacement.
                if moving.pacer.due_replacing_pending(now, &budget, share) {
                    // Motion and the graphics painter run on the same app
                    // thread: the counter was read above, and an unpainted
                    // frame cannot be drawn after this replacement.
                    moving.pacer.cancel_pending();
                    if moving.pacer.pixels() == Pixels::Cells {
                        moving.pacer.sent(now, None);
                    } else {
                        moving.pacer.schedule(now);
                    }
                    moving.player.advance(now);
                    moving.pacer.settle(&budget, share);
                }
            }
            out.insert(
                media.node,
                Live {
                    picture: moving.player.frame().clone(),
                    pixels: moving.pacer.pixels(),
                },
            );
        }
        // Out of view, an animation forgets its place.
        self.playing.retain(|node, _| seen.contains(node));
        self.stills.retain(|node, _| seen.contains(node));
        out
    }

    fn moving(
        &mut self,
        node: NodeId,
        animation: &Picture,
        region: &Region,
        pixels: Pixels,
        now: Instant,
        cell: rich_art::graphics::CellPixels,
    ) -> &mut Moving {
        let stale = self.playing.get(&node).is_none_or(|m| m.of != *animation);
        if stale && let Some(player) = Player::new(animation, now) {
            let mut pacer = Pacer::with_formats(pixels, region.cols, region.rows, self.formats);
            pacer.set_cell_pixels(cell);
            self.playing.insert(
                node,
                Moving {
                    of: animation.clone(),
                    bytes: picture_bytes(animation.id()),
                    cell,
                    geometry: (region.cols, region.rows),
                    stale_bytes: false,
                    player,
                    pacer,
                },
            );
        }
        let moving = self.playing.get_mut(&node).expect("made above");
        moving.pacer.resize(region.cols, region.rows);
        moving
    }

    /// The still frame Kitty is sent for a frozen animation, made once.
    fn still(&mut self, node: NodeId, animation: &Picture) -> Picture {
        match self.stills.get(&node) {
            Some((of, still)) if of == animation => still.clone(),
            _ => {
                let still = animation.still();
                self.stills.insert(node, (animation.clone(), still.clone()));
                still
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_art::graphics::AnimationFrame;
    use rich_art::image::{Rgba, RgbaImage};
    use std::time::Duration;

    #[test]
    fn cells_reuse_matching_metrics_and_rebuild_for_changed_aspect() {
        let picture = Picture::new(rich_art::image::DynamicImage::ImageRgba8(
            RgbaImage::from_fn(12, 8, |x, _| Rgba([(x * 20) as u8, 0, 0, 255])),
        ));
        let console = rich::Console::builder().width(4).build();
        let cache = Cells::default();
        let normal = cache.of_with_metrics(&console, &picture, Pixels::Cells, 4, 4, Some((8, 16)));
        let same = cache.of_with_metrics(&console, &picture, Pixels::Cells, 4, 4, Some((8, 16)));
        let square = cache.of_with_metrics(&console, &picture, Pixels::Cells, 4, 4, Some((12, 12)));
        assert!(Rc::ptr_eq(&normal, &same));
        assert!(!Rc::ptr_eq(&normal, &square));
        assert_ne!(*normal, *square);
    }

    fn gif() -> Picture {
        Picture::animated(
            (0..4u8)
                .map(|k| AnimationFrame {
                    image: RgbaImage::from_pixel(16, 16, Rgba([k * 60, 0, 0, 255])),
                    delay: Duration::from_millis(50),
                })
                .collect(),
        )
        .unwrap()
    }

    fn page_with(picture: Picture) -> Page {
        let mut page = Page::default();
        page.media.push(crate::model::Media {
            node: 7,
            kind: crate::model::MediaKind::Image,
            rect: crate::engine::Rect::new(0.0, 0.0, 80.0, 72.0),
            alt: String::new(),
            src: String::new(),
            still: None,
            picture: Some(picture),
            captioned: false,
            playback: None,
            poster: String::new(),
            controls: false,
            autoplay: false,
            empty_alt: false,
            aria_hidden: false,
            in_label: false,
            revision: 0,
            text: String::new(),
            pending: false,
        });
        page
    }

    const REGION: Region = Region {
        row: 2,
        column: 0,
        cols: 10,
        rows: 4,
        media: 0,
    };

    #[test]
    fn a_gif_steps_within_its_ceiling_and_holds_when_frozen() {
        let page = page_with(gif());
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        let mut versions = Vec::new();
        for tick in 0..100u64 {
            let now = start + Duration::from_millis(tick * 10);
            let shown = motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, now);
            versions.push(shown[&7].picture.version());
        }
        versions.dedup();
        // 50 ms frames, capped at 8 a second: about 8 changes in a second.
        assert!((6..=10).contains(&versions.len()), "{versions:?}");
        // Frozen: nothing moves.
        let held = motion.step(&page, &[REGION], 0..10, Pixels::Sixel, true, start)[&7]
            .picture
            .clone();
        for tick in 0..50u64 {
            let now = start + Duration::from_millis(1000 + tick * 10);
            let shown = motion.step(&page, &[REGION], 0..10, Pixels::Sixel, true, now);
            assert_eq!(shown[&7].picture, held);
        }
        // Out of view: not stepped, not shown.
        assert!(
            motion
                .step(&page, &[REGION], 20..30, Pixels::Sixel, false, start)
                .is_empty()
        );
    }

    #[test]
    fn changed_cell_geometry_keeps_old_bytes_out_of_new_estimate() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        rich_art::picture::count_picture_bytes(id, 12_345);
        let resized = Region { cols: 8, ..REGION };
        motion.step(
            &page,
            &[resized],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(
            pacer.recent_bytes(),
            12_345 + (resized.cols * resized.rows) as u64 * UNDERLAY
        );
        assert_eq!(
            pacer.bytes_per_frame() as u64,
            Pacer::estimate(Pixels::Sixel, resized.cols, resized.rows)
        );
    }

    #[test]
    fn leaving_the_view_cancels_unseen_reservations_without_losing_bytes() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        );
        motion.hide();
        assert!(!motion.playing.get_mut(&7).unwrap().pacer.cancel_pending());
        rich_art::picture::count_picture_bytes(id, 4000);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(130),
        );
        assert_eq!(motion.playing[&7].pacer.recent_bytes(), 5000);
    }

    #[test]
    fn freezing_keeps_unobserved_displayed_frame_reservation() {
        let page = page_with(gif());
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        );
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(130),
        );
        assert!(
            motion.playing.get_mut(&7).unwrap().pacer.cancel_pending(),
            "the frozen frame remains eligible to paint"
        );
    }

    #[test]
    fn driver_paints_replacement_synchronously_and_pacer_learns_each_actual_cost() {
        use rich_intuituive::{App, image_graphics, picture_of, signal};
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        let initial = motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start)[&7]
            .picture
            .clone();
        let handle = std::rc::Rc::new(std::cell::Cell::new(None));
        let stored = handle.clone();
        let app = App::new(move || {
            let shown = signal(Some(initial.clone()));
            stored.set(Some(shown));
            picture_of(move || shown.get(), view(Pixels::Sixel)).fixed(4)
        })
        .graphics(image_graphics());
        let mut driver = app.driver(10, 4);
        let shown = handle.get().unwrap();
        let mut prior_cost = None;
        for cycle in 0..2u64 {
            let base = cycle * 250;
            let a = motion.step(
                &page,
                &[REGION],
                0..10,
                Pixels::Sixel,
                false,
                start + Duration::from_millis(base + 125),
            )[&7]
                .picture
                .clone();
            shown.set(Some(a.clone()));
            // A is superseded before any Driver render can paint it.
            let b = motion.step(
                &page,
                &[REGION],
                0..10,
                Pixels::Sixel,
                false,
                start + Duration::from_millis(base + 250),
            )[&7]
                .picture
                .clone();
            assert_ne!(a.version(), b.version());
            shown.set(Some(b));
            let before = picture_bytes(id);
            driver.update(Duration::from_millis(base + 250));
            let output = driver.render().unwrap_or_default();
            assert!(
                output.contains("\x1bP"),
                "the replacement must actually be painted"
            );
            let actual = picture_bytes(id) - before + (REGION.cols * REGION.rows) as u64 * UNDERLAY;
            assert!(actual > (REGION.cols * REGION.rows) as u64 * UNDERLAY);
            motion.step(
                &page,
                &[REGION],
                0..10,
                Pixels::Sixel,
                false,
                start + Duration::from_millis(base + 260),
            );
            let pacer = &mut motion.playing.get_mut(&7).unwrap().pacer;
            let expected =
                prior_cost.map_or(actual as f64, |old: f64| old * 0.7 + actual as f64 * 0.3);
            assert!((pacer.bytes_per_frame() - expected).abs() < 0.001);
            assert!(
                !pacer.cancel_pending(),
                "the painted replacement must reconcile its reservation"
            );
            prior_cost = Some(expected);
        }
    }

    #[test]
    fn unseen_gif_frame_reservation_is_replaced_before_next_due_check() {
        let page = page_with(gif());
        let region = Region {
            cols: 1,
            rows: 1,
            ..REGION
        };
        let mut motion = Motion::new(ByteBudget::new(1000));
        let start = Instant::now();
        motion.step(&page, &[region], 0..10, Pixels::Sixel, false, start);
        motion
            .playing
            .get_mut(&7)
            .unwrap()
            .pacer
            .observe_bytes(start, 1900, false);
        let first = motion.step(
            &page,
            &[region],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        )[&7]
            .picture
            .version();
        let second = motion.step(
            &page,
            &[region],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(250),
        )[&7]
            .picture
            .version();
        assert_ne!(
            first, second,
            "an unseen superseded frame must not hold the budget"
        );
    }

    #[test]
    fn changed_metrics_at_due_tick_account_old_bytes_without_teaching_new_cost() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        rich_art::picture::count_picture_bytes(id, 43_210);
        motion.set_cell_pixels(rich_art::graphics::CellPixels::new(12, 24));
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(
            pacer.recent_bytes(),
            43_210 + (REGION.cols * REGION.rows) as u64 * UNDERLAY
        );
        assert_eq!(
            pacer.bytes_per_frame() as u64,
            Pacer::estimate_with_metrics(
                Pixels::Sixel,
                REGION.cols,
                REGION.rows,
                rich_art::graphics::CellPixels::new(12, 24)
            )
        );
    }

    #[test]
    fn frozen_metric_change_accounts_late_old_bytes_without_teaching_new_cost() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        motion.set_cell_pixels(rich_art::graphics::CellPixels::new(12, 24));
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(10),
        );
        rich_art::picture::count_picture_bytes(id, 57_890);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(20),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(
            pacer.recent_bytes(),
            57_890 + (REGION.cols * REGION.rows) as u64 * UNDERLAY
        );
        assert_eq!(
            pacer.bytes_per_frame() as u64,
            Pacer::estimate_with_metrics(
                Pixels::Sixel,
                REGION.cols,
                REGION.rows,
                rich_art::graphics::CellPixels::new(12, 24)
            )
        );
        // Once a frame at the new size is drawn, its observed cost can
        // become the measurement without losing the earlier window bytes.
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(125),
        );
        rich_art::picture::count_picture_bytes(id, 9_000);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(130),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(pacer.recent_bytes(), 68_890);
        assert_eq!(pacer.bytes_per_frame(), 10_000.0);
    }

    #[test]
    fn late_old_protocol_bytes_do_not_teach_new_protocol_cost() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Iterm, false, start);
        motion.budget = ByteBudget::new(12_000);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Iterm,
            false,
            start + Duration::from_millis(10),
        );
        assert_eq!(motion.playing[&7].pacer.pixels(), Pixels::Sixel);
        rich_art::picture::count_picture_bytes(id, 4300);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Iterm,
            false,
            start + Duration::from_millis(20),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(pacer.recent_bytes(), 5300);
        assert_eq!(
            pacer.bytes_per_frame() as u64,
            Pacer::estimate(Pixels::Sixel, REGION.cols, REGION.rows)
        );
    }

    #[test]
    fn late_graphics_bytes_are_counted_after_pacer_steps_to_cells() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        motion.budget = ByteBudget::new(1);
        motion.set_cell_pixels(rich_art::graphics::CellPixels::new(12, 24));
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(10),
        );
        assert_eq!(motion.playing[&7].pacer.pixels(), Pixels::Cells);
        rich_art::picture::count_picture_bytes(id, 10_000);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            true,
            start + Duration::from_millis(20),
        );
        assert_eq!(motion.playing[&7].pacer.recent_bytes(), 11_000);
    }

    #[test]
    fn first_frame_at_nondefault_metrics_teaches_its_actual_cost() {
        let animation = gif();
        let id = animation.id();
        let page = page_with(animation);
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        motion.set_cell_pixels(rich_art::graphics::CellPixels::new(12, 24));
        let start = Instant::now();
        motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, start);
        rich_art::picture::count_picture_bytes(id, 7_300);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            start + Duration::from_millis(10),
        );
        let pacer = &motion.playing[&7].pacer;
        assert_eq!(pacer.recent_bytes(), 8_300);
        assert_eq!(pacer.bytes_per_frame(), 8_300.0);
    }

    #[test]
    fn animated_picture_budget_uses_selected_metrics() {
        let page = page_with(gif());
        let mut motion = Motion::new(ByteBudget::new(10_000_000));
        let cell = rich_art::graphics::CellPixels::new(12, 24);
        motion.set_cell_pixels(cell);
        motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Sixel,
            false,
            Instant::now(),
        );
        assert_eq!(
            motion.playing[&7].pacer.bytes_per_frame() as u64,
            Pacer::estimate_with_metrics(Pixels::Sixel, REGION.cols, REGION.rows, cell)
        );
    }

    #[test]
    fn restricted_kitty_uses_software_frames() {
        let animation = gif();
        let page = page_with(animation.clone());
        let presentation = rich_intuituive::ImagePresentation::new(
            Pixels::Kitty,
            rich_intuituive::KittyFeatures {
                unicode_placeholders: true,
                native_animation: false,
            },
            PixelFormats::cells().with(Pixels::Kitty),
        );
        let mut motion = Motion::with_presentation(ByteBudget::new(10_000_000), presentation);
        let now = Instant::now();
        let first = motion.step(&page, &[REGION], 0..10, Pixels::Kitty, false, now);
        let next = motion.step(
            &page,
            &[REGION],
            0..10,
            Pixels::Kitty,
            false,
            now + Duration::from_millis(150),
        );
        assert!(!first[&7].picture.is_animated());
        assert_eq!(first[&7].picture.id(), animation.id());
        assert_ne!(first[&7].picture.image(), next[&7].picture.image());
        assert_eq!(motion.moving, 1);
    }

    #[test]
    fn kitty_plays_it_and_frozen_gets_a_still() {
        let animation = gif();
        let page = page_with(animation.clone());
        let mut motion = Motion::default();
        let now = Instant::now();
        let shown = motion.step(&page, &[REGION], 0..10, Pixels::Kitty, false, now);
        assert_eq!(shown[&7].picture, animation);
        let frozen = motion.step(&page, &[REGION], 0..10, Pixels::Kitty, true, now);
        assert!(!frozen[&7].picture.is_animated());
        assert_eq!(frozen[&7].picture.id(), animation.id());
    }

    #[test]
    fn a_slow_terminal_steps_down_to_cells() {
        let page = page_with(gif());
        // 20 kB a second for a 10 x 4 region of Sixel frames.
        let mut motion = Motion::new(ByteBudget::new(2_000));
        let start = Instant::now();
        let mut last = Pixels::Sixel;
        for tick in 0..300u64 {
            let now = start + Duration::from_millis(tick * 10);
            last = motion.step(&page, &[REGION], 0..10, Pixels::Sixel, false, now)[&7].pixels;
        }
        assert_eq!(last, Pixels::Cells);
    }

    #[test]
    fn the_backdrop_fills_the_row_under_the_text() {
        let line = vec![
            Segment::new("ink", rich::Style::parse("rgb(17,17,17)").ok()),
            Segment::new("▀", rich::Style::parse("red on blue").ok()),
        ];
        let out = on_backdrop(
            line,
            10,
            crate::adapt::backdrop(crate::adapt::Background::Light),
        );
        assert_eq!(
            out.iter()
                .map(|s| rich::cells::cell_len(&s.text))
                .sum::<usize>(),
            10
        );
        let white_style = rich::Style::parse("on rgb(255,255,255)").unwrap();
        let white = white_style.bgcolor();
        let bg = |s: &Segment| s.style.as_ref().and_then(|s| s.bgcolor().cloned());
        // The text and the padding are on white; a picture's cell keeps its own.
        assert_eq!(bg(&out[0]).as_ref(), white);
        assert_eq!(bg(out.last().unwrap()).as_ref(), white);
        assert_ne!(bg(&out[1]).as_ref(), white);
    }

    #[test]
    fn a_picture_is_spliced_into_its_rows() {
        let console = rich::Console::builder().width(20).build();
        let cells = Cells::default();
        let picture = Picture::new(rich_art::image::DynamicImage::ImageRgba8(
            RgbaImage::from_pixel(8, 8, Rgba([255, 0, 0, 255])),
        ));
        let region = Region {
            row: 0,
            column: 4,
            cols: 6,
            rows: 2,
            media: 0,
        };
        let line = vec![Segment::new("abcd░░░░░░efgh", None)];
        let out = splice(&console, &cells, line, 20, 1, &[region], |_| {
            Some((picture.clone(), Pixels::Cells))
        });
        let text: String = out.iter().map(|s| s.text.as_str()).collect();
        assert!(text.starts_with("abcd"), "{text:?}");
        assert_eq!(&text.chars().skip(10).take(4).collect::<String>(), "efgh");
        assert!(!text.contains('░'), "{text:?}");
        assert_eq!(rich::cells::cell_len(&text), 20);
        // A row the region does not cover keeps its text.
        let line = vec![Segment::new("abcd░░░░░░efgh", None)];
        let out = splice(&console, &cells, line, 20, 2, &[region], |_| {
            Some((picture.clone(), Pixels::Cells))
        });
        assert!(out.iter().any(|s| s.text.contains('░')));
    }
}

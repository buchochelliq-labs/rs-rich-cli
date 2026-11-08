//! The pixel view (#818): the page as Chrome paints it, pixel for pixel,
//! for terminals with Kitty graphics (`--pixels`, `view = "pixels"`, or
//! the `view.pixels` action). The projection stays the default: it works
//! in every terminal, and a screen reader can read it.
//!
//! Frames come from Chrome's screencast (`Page.startScreencast`, the one
//! the video regions use), at full scale while the view shows
//! ([`Command::Pixels`]). Each is fitted to the view's cells and cut into
//! tiles of 8 x 4 cells (64 x 64 pixels at the usual cell) by rs-rich-art's
//! [`Tiler`]; only the tiles whose pixels changed get a new picture, and
//! intuiTUIve's `tiles_of` and `image_graphics` send the terminal only
//! those. A frame the byte budget has no room for is dropped, never
//! queued: the next one is cut against what is shown.
//!
//! Input goes to the page as it is drawn: a click at its exact point
//! ([`Command::Point`], to the pixel where the terminal reports pixels),
//! the wheel, and every key but those with Ctrl or Alt and the F keys,
//! which stay Noctiluca's ([`passes`]).
//!
//! Without Kitty graphics the view is refused with a message, and the
//! projection shows: Sixel or half blocks of a whole page would be too
//! slow to follow it.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

use rich_art::image::RgbaImage;
use rich_art::picture::{ByteBudget, Pacer, Pixels};
use rich_art::tiles::{Tile, Tiler};
use rich_intuituive::interact::{Button, Key, KeyCode, MouseKind};
use rich_intuituive::prelude::*;
use rich_intuituive::tiles_of;

use crate::app::{Browser, CHROME_ROWS, byte_budget};
use crate::cdp::CELL;
use crate::tab::Command;

/// Why the pixel view cannot show.
pub const NEEDS_KITTY: &str = "The pixel view needs Kitty graphics and Unicode placeholders: \
     showing the projection.";

/// What the pixel view has sent, for the status bar and tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScreenStats {
    /// Source frames accepted by the tiler (not native displayed frames).
    pub frames: u64,
    /// Frames dropped: the byte budget had no room for them.
    pub dropped: u64,
    /// Tiles sent, over every frame shown.
    pub tiles: u64,
    /// What the tiles sent were estimated to cost.
    pub bytes: u64,
}

/// The pixel view's state: its tiler, the pace frames keep within the
/// byte budget, and what was sent.
#[derive(Debug)]
pub struct Screen {
    tiler: Tiler,
    pacer: Pacer,
    pub stats: ScreenStats,
}

impl Default for Screen {
    fn default() -> Screen {
        Screen::new()
    }
}

impl Screen {
    pub fn new() -> Screen {
        Screen {
            tiler: Tiler::default(),
            pacer: Pacer::new(Pixels::Kitty, 1, 1),
            stats: ScreenStats::default(),
        }
    }

    /// Take `frame`, drawn in `cols` x `rows` cells, at `now`: the tiles
    /// that changed, or `None` when the budget has no room for a frame
    /// now (it is dropped, and the tiles shown stay as they are).
    pub fn take(
        &mut self,
        frame: &RgbaImage,
        cols: usize,
        rows: usize,
        budget: &ByteBudget,
        now: Instant,
    ) -> Option<Vec<usize>> {
        if !self.accept(cols, rows, budget, now) {
            return None;
        }
        Some(self.update_tiles(frame, cols, rows, now))
    }

    fn accept(&mut self, cols: usize, rows: usize, budget: &ByteBudget, now: Instant) -> bool {
        self.pacer.resize(cols, rows);
        if !self.pacer.due(now, budget, 1.0) {
            self.stats.dropped += 1;
            return false;
        }
        true
    }

    fn update_tiles(
        &mut self,
        frame: &RgbaImage,
        cols: usize,
        rows: usize,
        now: Instant,
    ) -> Vec<usize> {
        let changed = self.tiler.update(frame, cols, rows);
        let bytes = self.tiler.estimate(&changed, Pixels::Kitty);
        self.pacer.sent(now, Some(bytes.max(1)));
        self.stats.frames += 1;
        self.stats.tiles += changed.len() as u64;
        self.stats.bytes += bytes;
        changed
    }

    fn take_lazy(
        &mut self,
        cols: usize,
        rows: usize,
        budget: &ByteBudget,
        now: Instant,
        convert: impl FnOnce() -> Option<RgbaImage>,
    ) -> Option<Vec<usize>> {
        if !self.accept(cols, rows, budget, now) {
            return None;
        }
        let frame = convert()?;
        Some(self.update_tiles(&frame, cols, rows, now))
    }

    /// The tiles, as shown.
    pub fn tiles(&self) -> Vec<Tile> {
        self.tiler.tiles().to_vec()
    }
}

/// A screencast frame's pixels as RGBA.
pub fn rgba(frame: &rich_embed::ScreencastFrame) -> Option<RgbaImage> {
    let pixels = &frame.pixels;
    let rgb = rich_art::image::RgbImage::from_raw(pixels.width, pixels.height, pixels.rgb.clone())?;
    Some(rich_art::image::DynamicImage::ImageRgb8(rgb).to_rgba8())
}

/// The point of the viewport, in CSS pixels, at fractional cell `column`,
/// `row` of the view: Chrome lays the page out at [`CELL`] pixels a cell.
pub fn point(column: f32, row: f32) -> (f64, f64) {
    (
        f64::from(column.max(0.0)) * CELL.0 as f64,
        f64::from(row.max(0.0)) * CELL.1 as f64,
    )
}

/// Whether a key pressed in the pixel view goes to the page: all but
/// those with Ctrl or Alt, and the F keys, which stay Noctiluca's (to
/// leave the view, the F1 list, quitting).
pub fn passes(key: Key) -> bool {
    !key.modifiers.ctrl && !key.modifiers.alt && !matches!(key.code, KeyCode::F(_))
}

/// Whether the pixel view can show: Kitty graphics.
pub fn available(b: Browser) -> bool {
    b.pixels.get_untracked() == Pixels::Kitty
}

/// Discard the last document or view's tiles and restart pacing.
pub(crate) fn invalidate(b: Browser) {
    let screen = b.screen.get_untracked();
    *screen.borrow_mut() = Screen::new();
    b.screen_tiles.set(Vec::new());
    b.screen_stats.set(ScreenStats::default());
}

/// A screencast frame for the pixel view: cut into tiles and shown, if
/// the view shows and the budget lets it through.
pub fn frame(b: Browser, frame: &rich_embed::ScreencastFrame) {
    if b.view.get_untracked() != crate::config::View::Pixels {
        return;
    }
    let (cols, rows) = b.viewport.get_untracked();
    let rows = rows.saturating_sub(CHROME_ROWS);
    let budget = byte_budget(b);
    let screen = b.screen.get_untracked();
    let mut screen = screen.borrow_mut();
    if screen
        .take_lazy(cols.into(), rows.into(), &budget, Instant::now(), || {
            rgba(frame)
        })
        .is_some()
    {
        b.screen_tiles.set(screen.tiles());
    }
    b.screen_stats.set(screen.stats);
}

/// The view: the tiles, in Kitty graphics; a click and the wheel go to
/// the page.
pub fn view(b: Browser) -> Node {
    tiles_of(move || b.screen_tiles.get(), Pixels::Kitty)
        .on_mouse(move |_, mouse| match mouse.kind {
            MouseKind::Down(Button::Left) => {
                let (x, y) = mouse.precise();
                let (x, y) = point(x, y);
                b.send(Command::Point { x, y });
                true
            }
            MouseKind::ScrollDown => {
                b.send(Command::Wheel(3.0 * CELL.1 as f64));
                true
            }
            MouseKind::ScrollUp => {
                b.send(Command::Wheel(-3.0 * CELL.1 as f64));
                true
            }
            _ => false,
        })
        .flex(1)
        .name("pixels")
}

/// The state the browser keeps for the view.
pub fn state() -> Rc<RefCell<Screen>> {
    Rc::new(RefCell::new(Screen::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_art::image::Rgba;
    use std::time::Duration;

    #[test]
    fn unavailable_hint_names_the_required_placeholder_support() {
        assert!(NEEDS_KITTY.contains("Unicode placeholders"));
        assert!(!NEEDS_KITTY.contains("WezTerm"));
    }

    #[test]
    fn a_frame_the_budget_has_no_room_for_is_dropped_not_queued() {
        let mut screen = Screen::new();
        let budget = ByteBudget::new(4 << 20);
        let white = RgbaImage::from_pixel(640, 256, Rgba([255, 255, 255, 255]));
        let now = Instant::now();
        assert_eq!(
            screen.take(&white, 80, 16, &budget, now).map(|c| c.len()),
            Some(40)
        );
        // At once again: too soon for the budget's pace.
        assert_eq!(screen.take(&white, 80, 16, &budget, now), None);
        assert_eq!(screen.stats.dropped, 1);
        // Later, the same frame: shown, and nothing changed.
        let later = now + Duration::from_secs(3);
        assert_eq!(screen.take(&white, 80, 16, &budget, later), Some(vec![]));
        assert_eq!(screen.stats.frames, 2);
    }

    #[test]
    fn rejected_frame_does_not_convert_and_invalid_frame_is_not_sent() {
        let mut screen = Screen::new();
        let budget = ByteBudget::new(4 << 20);
        let now = Instant::now();
        let calls = std::cell::Cell::new(0);
        let convert = || {
            calls.set(calls.get() + 1);
            Some(RgbaImage::from_pixel(64, 64, Rgba([255; 4])))
        };
        assert!(screen.take_lazy(8, 4, &budget, now, convert).is_some());
        assert!(screen.take_lazy(8, 4, &budget, now, convert).is_none());
        assert_eq!(calls.get(), 1);
        assert_eq!(screen.stats.frames, 1);
        assert_eq!(screen.stats.dropped, 1);
        assert!(
            screen
                .take_lazy(8, 4, &budget, now + Duration::from_secs(3), || None)
                .is_none()
        );
        assert_eq!(screen.stats.frames, 1);
        assert!(
            screen
                .take_lazy(8, 4, &budget, now + Duration::from_secs(3), convert)
                .is_some()
        );
        assert_eq!(calls.get(), 2);
        assert_eq!(screen.stats.frames, 2);
    }

    #[test]
    fn keys_with_ctrl_or_alt_and_the_f_keys_stay_noctilucas() {
        let key = |name: &str| Key::parse(name).unwrap();
        assert!(passes(key("a")) && passes(key("enter")) && passes(key("shift+tab")));
        assert!(!passes(key("ctrl+q")) && !passes(key("alt+x")) && !passes(key("f1")));
        assert_eq!(point(2.5, 1.0), (2.5 * CELL.0 as f64, CELL.1 as f64));
    }
}

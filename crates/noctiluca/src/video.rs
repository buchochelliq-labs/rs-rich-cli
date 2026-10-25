//! Video in its region: frames of a playing `<video>`, cut from Chrome's
//! screencast, handed to the picture side to draw.
//!
//! - **Only while something plays on screen.** The tab turns Chrome's
//!   screencast on when a video plays in a region the view shows, and off
//!   when none does. It runs on demand: Chrome paints the next frame only
//!   once the last was taken, so it encodes no more than are used.
//! - **Cropped per region.** Each frame is the viewport; each playing
//!   region's rectangle is cut from it by the frame's own scroll position
//!   ([`ScreencastFrame::crop`]). A region Chrome's viewport does not hold
//!   (our scroll is in rows, Chrome's follows when the view rests) is
//!   captured on its own instead.
//! - **At most [`MAX_FPS`] a region.** Frames are taken no faster than
//!   that. How many of them reach the terminal, in which protocol, within
//!   the terminal's byte budget, is rs-rich-art's frame pacing: the
//!   [`FrameSink`] the app gives frames to.
//!
//! Nothing here looks at a site: any `<video>` with a playing state is a
//! moving region.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rich_embed::{Pixels, ScreencastFrame};

use crate::engine::{NodeId, Rect};

/// The most frames a second a region is given: the PoC's rate, a ceiling.
pub const MAX_FPS: u32 = 8;

/// The shortest time between two frames of one region.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1000 / MAX_FPS as u64);

/// A frame of one playing region: its pixels, as Chrome painted them.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    /// The video element.
    pub node: NodeId,
    /// The region's pixels (RGB).
    pub pixels: Pixels,
    /// Counts the region's frames from 1.
    pub seq: u64,
}

/// Where the frames go: the picture side, which fits, encodes and paces
/// them within the terminal's byte budget (rs-rich-art). The app keeps the
/// newest frame of each region and draws it.
pub trait FrameSink {
    /// A new frame of `frame.node`'s region.
    fn frame(&mut self, frame: Arc<VideoFrame>);
    /// The region stopped moving (paused, ended, gone or off screen).
    fn still(&mut self, node: NodeId);
}

/// A region that may move: a video's node and rectangle, whether it plays
/// and whether the view shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub node: NodeId,
    pub rect: Rect,
    pub playing: bool,
    pub on_screen: bool,
}

impl Region {
    /// Whether it should have frames now.
    pub fn moving(&self) -> bool {
        self.playing && self.on_screen && !self.rect.is_empty()
    }
}

/// The frame side of a tab: which regions move, whether the screencast is
/// on, and each region's next frame.
#[derive(Debug, Default)]
pub struct Frames {
    /// Whether the screencast is on.
    pub on: bool,
    /// Each moving region's last frame: when, and its count.
    last: Vec<(NodeId, Instant, u64)>,
}

/// What [`Frames::plan`] asks the tab to do.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// Turn the screencast on or off.
    pub screencast: Option<bool>,
    /// Regions that stopped moving.
    pub stopped: Vec<NodeId>,
    /// Whether a frame is due for some region now.
    pub due: bool,
}

impl Frames {
    /// What to do with `regions` at `now`.
    pub fn plan(&mut self, regions: &[Region], now: Instant) -> Plan {
        let moving: Vec<&Region> = regions.iter().filter(|r| r.moving()).collect();
        let mut plan = Plan::default();
        let want = !moving.is_empty();
        if want != self.on {
            self.on = want;
            plan.screencast = Some(want);
        }
        self.last.retain(|(node, ..)| {
            let keep = moving.iter().any(|r| r.node == *node);
            if !keep {
                plan.stopped.push(*node);
            }
            keep
        });
        plan.due = moving.iter().any(|r| self.due(r.node, now));
        plan
    }

    fn due(&self, node: NodeId, now: Instant) -> bool {
        self.last
            .iter()
            .find(|(n, ..)| *n == node)
            .is_none_or(|(_, at, _)| now.duration_since(*at) >= MIN_INTERVAL)
    }

    /// Cut the due regions out of `frame`. Regions the frame does not hold
    /// come back in the second list, to capture on their own.
    pub fn cut(
        &mut self,
        regions: &[Region],
        frame: Option<&ScreencastFrame>,
        now: Instant,
    ) -> (Vec<VideoFrame>, Vec<Region>) {
        let mut cut = Vec::new();
        let mut missing = Vec::new();
        for region in regions.iter().filter(|r| r.moving()) {
            if !self.due(region.node, now) {
                continue;
            }
            let r = region.rect;
            match frame.and_then(|f| f.crop(r.x, r.y, r.width, r.height)) {
                // Wholly in the frame: cut. Partly in: the rest is not
                // painted there, so it is captured as missing ones are.
                Some(pixels) if whole(&pixels, frame, r) => {
                    cut.push(self.stamp(region.node, pixels, now));
                }
                _ => missing.push(*region),
            }
        }
        (cut, missing)
    }

    /// A frame of `node` taken some other way (a capture of its region).
    pub fn stamp(&mut self, node: NodeId, pixels: Pixels, now: Instant) -> VideoFrame {
        let seq = match self.last.iter_mut().find(|(n, ..)| *n == node) {
            Some(entry) => {
                entry.1 = now;
                entry.2 += 1;
                entry.2
            }
            None => {
                self.last.push((node, now, 1));
                1
            }
        };
        VideoFrame { node, pixels, seq }
    }
}

/// Whether `pixels`, cut from `frame` for `rect`, is all of `rect`: within
/// a pixel of its size at the frame's scale.
fn whole(pixels: &Pixels, frame: Option<&ScreencastFrame>, rect: Rect) -> bool {
    let Some(frame) = frame else {
        return false;
    };
    let device = if frame.meta.device_width > 0.0 {
        frame.meta.device_width
    } else {
        frame.pixels.width as f64
    };
    let scale = frame.pixels.width as f64 / device * frame.meta.page_scale.max(f64::MIN_POSITIVE);
    (pixels.width as f64 + 1.5) >= rect.width * scale
        && (pixels.height as f64 + 1.5) >= rect.height * scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use rich_embed::FrameMeta;

    fn frame(scroll_y: f64) -> ScreencastFrame {
        ScreencastFrame {
            pixels: Pixels::new(100, 100, vec![7; 100 * 100 * 3]).unwrap(),
            meta: FrameMeta {
                scroll_y,
                page_scale: 1.0,
                device_width: 100.0,
                device_height: 100.0,
                ..FrameMeta::default()
            },
        }
    }

    fn region(node: NodeId, y: f64, playing: bool, on_screen: bool) -> Region {
        Region {
            node,
            rect: Rect::new(10.0, y, 40.0, 20.0),
            playing,
            on_screen,
        }
    }

    #[test]
    fn the_screencast_runs_only_while_a_region_plays_on_screen() {
        let mut frames = Frames::default();
        let now = Instant::now();
        let paused = [region(1, 10.0, false, true)];
        assert_eq!(frames.plan(&paused, now).screencast, None);
        let off_screen = [region(1, 10.0, true, false)];
        assert_eq!(frames.plan(&off_screen, now).screencast, None);
        let playing = [region(1, 10.0, true, true)];
        let plan = frames.plan(&playing, now);
        assert_eq!(plan.screencast, Some(true));
        assert!(plan.due);
        // Again: already on.
        assert_eq!(frames.plan(&playing, now).screencast, None);
        frames.cut(&playing, Some(&frame(0.0)), now);
        // Paused: off, and the region is still again.
        let plan = frames.plan(&paused, now);
        assert_eq!(plan.screencast, Some(false));
        assert_eq!(plan.stopped, vec![1]);
    }

    #[test]
    fn regions_are_cut_at_most_eight_times_a_second() {
        let mut frames = Frames::default();
        let start = Instant::now();
        let playing = [region(1, 10.0, true, true)];
        let mut count = 0;
        // Offered a frame every 10 ms for two seconds.
        for tick in 0..200 {
            let now = start + Duration::from_millis(tick * 10);
            let (cut, missing) = frames.cut(&playing, Some(&frame(0.0)), now);
            assert!(missing.is_empty());
            count += cut.len();
        }
        assert!((15..=17).contains(&count), "{count}");
    }

    #[test]
    fn a_region_the_frame_does_not_hold_is_captured_on_its_own() {
        let mut frames = Frames::default();
        let now = Instant::now();
        let regions = [region(1, 10.0, true, true), region(2, 500.0, true, true)];
        let (cut, missing) = frames.cut(&regions, Some(&frame(0.0)), now);
        assert_eq!(cut.len(), 1);
        assert_eq!((cut[0].node, cut[0].seq), (1, 1));
        assert_eq!((cut[0].pixels.width, cut[0].pixels.height), (40, 20));
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].node, 2);
        // Scrolled so it is half out: captured too, not cut short.
        let mut frames = Frames::default();
        let (cut, missing) = frames.cut(&regions[..1], Some(&frame(20.0)), now);
        assert!(cut.is_empty());
        assert_eq!(missing.len(), 1);
    }
}

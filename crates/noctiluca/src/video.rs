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
//!   that, or than the app's pacing asks. How many of them reach the
//!   terminal, in which protocol, within the terminal's byte budget, is
//!   rs-rich-art's frame pacing ([`Pacer`]), which [`Pacing`] runs per
//!   region on the app's side.
//!
//! Nothing here looks at a site: any `<video>` with a playing state is a
//! moving region.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rich_art::picture::{over_ssh, ByteBudget, Pacer, Picture, Pixels};
use rich_embed::ScreencastFrame;

use crate::engine::{NodeId, Rect};

/// The most frames a second a region is given: the PoC's rate, a ceiling.
pub const MAX_FPS: u32 = 8;

/// How far under its share a region is paced: what a frame costs is
/// learned from the frames before it, and one can cost more.
pub const HEADROOM: f64 = 0.1;

/// The shortest time between two frames of one region.
pub const MIN_INTERVAL: Duration = Duration::from_millis(1000 / MAX_FPS as u64);

/// A frame of one playing region: its pixels, as Chrome painted them.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    /// The video element.
    pub node: NodeId,
    /// The region's pixels (RGB).
    pub pixels: rich_embed::Pixels,
    /// Counts the region's frames from 1.
    pub seq: u64,
}

/// A region that may move: a video's node and rectangle, whether it plays
/// and whether the view shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub node: NodeId,
    pub rect: Rect,
    pub playing: bool,
    pub on_screen: bool,
    /// How long the app's pacing wants between two of its frames; never
    /// less than [`MIN_INTERVAL`].
    pub interval: Option<Duration>,
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
        plan.due = moving.iter().any(|r| self.due(r, now));
        plan
    }

    fn due(&self, region: &Region, now: Instant) -> bool {
        let interval = region.interval.unwrap_or(MIN_INTERVAL).max(MIN_INTERVAL);
        self.last
            .iter()
            .find(|(n, ..)| *n == region.node)
            .is_none_or(|(_, at, _)| now.duration_since(*at) >= interval)
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
            if !self.due(region, now) {
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
    pub fn stamp(&mut self, node: NodeId, pixels: rich_embed::Pixels, now: Instant) -> VideoFrame {
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

/// The app's side of the frames: one live [`Picture`] per region (its
/// frames keep one id, so Kitty replaces the image it holds), drawn as
/// rs-rich-art's [`Pacer`] allows within the terminal's [`ByteBudget`].
///
/// - Every moving region gets an equal share of the budget. Regions off
///   screen get no frames (the tab sends none).
/// - The newest frame is offered first; one that is not due is dropped.
/// - What a frame cost is what the painter counted for its picture
///   ([`picture_bytes`]), taken when the next frame comes: a frame not
///   yet painted is counted at the pacer's estimate.
/// - When the share allows fewer than the budget's floor of frames a
///   second, the region steps down: Kitty, iTerm2, Sixel, then cells.
pub struct Pacing {
    pub budget: ByteBudget,
    /// The protocol a new region starts at.
    pub start: Pixels,
    /// Other moving pictures sharing the budget (animated GIFs in view).
    pub others: usize,
    regions: HashMap<NodeId, Paced>,
}

/// One region's pacing.
struct Paced {
    pacer: Pacer,
    picture: Picture,
    /// The last frame given to the painter: when, and its picture's byte
    /// count then; settled at the next frame.
    pending: Option<(Instant, u64)>,
    /// Whether it moves now.
    moving: bool,
    /// The newest frame's count, so an older one is not shown after it.
    seq: u64,
}

/// What counts a picture's bytes: the painter's counter in the app,
/// an encoder in a test.
pub type ByteCounter = fn(u32) -> u64;

impl Pacing {
    /// Pacing within `budget`, starting regions at `start`.
    pub fn new(budget: ByteBudget, start: Pixels) -> Pacing {
        Pacing {
            budget,
            start,
            others: 0,
            regions: HashMap::new(),
        }
    }

    /// The protocol to start video at: character art over SSH unless the
    /// user asked for a protocol (`asked`), else what the terminal speaks.
    pub fn start_for(asked: Option<Pixels>, detected: Pixels) -> Pixels {
        match asked {
            Some(pixels) => pixels,
            None if over_ssh() => Pixels::Cells,
            None => detected,
        }
    }

    /// Each moving region's share of the budget, which the other moving
    /// pictures share too.
    fn share(&self) -> f64 {
        1.0 / (self.moving() + self.others).max(1) as f64
    }

    /// How many regions move now.
    pub fn moving(&self) -> usize {
        self.regions.values().filter(|r| r.moving).count()
    }

    /// The share the pacer keeps to: [`HEADROOM`] under the region's
    /// share, since a frame can cost more than the frames before it.
    fn paced_share(&self) -> f64 {
        self.share() * (1.0 - HEADROOM)
    }

    /// Offer `frame`, for a region of `cols` x `rows` cells, at `now`.
    /// The picture to draw now when it is due, else `None`. `counted`
    /// says what the painter has sent of a picture so far.
    pub fn offer(
        &mut self,
        frame: &VideoFrame,
        cols: usize,
        rows: usize,
        now: Instant,
        counted: ByteCounter,
    ) -> Option<Picture> {
        let (w, h) = (frame.pixels.width, frame.pixels.height);
        let rgba = frame.pixels.to_rgba();
        if !self.regions.contains_key(&frame.node) {
            let picture = Picture::from_rgba(w, h, rgba.clone())?;
            let pacer = Pacer::new(self.start, cols, rows);
            self.regions.insert(
                frame.node,
                Paced {
                    pacer,
                    picture,
                    pending: None,
                    moving: true,
                    seq: 0,
                },
            );
        }
        let share = {
            let region = self.regions.get_mut(&frame.node)?;
            region.moving = true;
            region.pacer.resize(cols, rows);
            if frame.seq <= region.seq {
                return None;
            }
            self.paced_share()
        };
        let budget = self.budget;
        let region = self.regions.get_mut(&frame.node)?;
        // What the last frame cost, now that it was painted.
        if let Some((at, before)) = region.pending.take() {
            let spent = counted(region.picture.id()).saturating_sub(before);
            region.pacer.sent(at, (spent > 0).then_some(spent));
        }
        while region.pacer.settle(&budget, share) {}
        if region.seq > 0 && !region.pacer.due(now, &budget, share) {
            return None;
        }
        region.picture = region.picture.next_rgba(w, h, rgba)?;
        region.seq = frame.seq;
        region.pending = Some((now, counted(region.picture.id())));
        Some(region.picture.clone())
    }

    /// The region stopped moving; its last frame stays.
    pub fn still(&mut self, node: NodeId) {
        if let Some(region) = self.regions.get_mut(&node) {
            region.moving = false;
            region.pending = None;
        }
    }

    /// Forget every region: a new document.
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// Forget a region gone from the page.
    pub fn forget(&mut self, node: NodeId) {
        self.regions.remove(&node);
    }

    /// The protocol `node`'s frames are drawn in now.
    pub fn pixels(&self, node: NodeId) -> Pixels {
        self.regions
            .get(&node)
            .map_or(self.start, |r| r.pacer.pixels())
    }

    /// The bytes `node`'s frames cost in the last two seconds.
    pub fn recent_bytes(&self, node: NodeId) -> u64 {
        self.regions
            .get(&node)
            .map_or(0, |r| r.pacer.recent_bytes())
    }

    /// The bytes a second `node` may spend: its share of the budget.
    pub fn cap(&self) -> f64 {
        self.budget.bytes_per_second as f64 * self.share()
    }

    /// How long the tab should wait between two frames of each moving
    /// region, so it cuts no frame the pacing would drop.
    pub fn intervals(&self) -> Vec<(NodeId, Duration)> {
        let share = self.paced_share();
        let mut out: Vec<(NodeId, Duration)> = self
            .regions
            .iter()
            .filter(|(_, r)| r.moving)
            .map(|(node, r)| {
                let fps = r.pacer.fps(&self.budget, share).max(0.1);
                (*node, Duration::from_secs_f64(1.0 / fps).max(MIN_INTERVAL))
            })
            .collect();
        out.sort_by_key(|(node, _)| *node);
        out
    }
}

/// Whether `pixels`, cut from `frame` for `rect`, is all of `rect`: within
/// a pixel of its size at the frame's scale.
fn whole(pixels: &rich_embed::Pixels, frame: Option<&ScreencastFrame>, rect: Rect) -> bool {
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
    use rich_embed::{FrameMeta, Pixels as Rgb};

    fn frame(scroll_y: f64) -> ScreencastFrame {
        ScreencastFrame {
            pixels: Rgb::new(100, 100, vec![7; 100 * 100 * 3]).unwrap(),
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
            interval: None,
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
        // The pacing asks for two a second: four in two seconds.
        let mut frames = Frames::default();
        let slow = [Region {
            interval: Some(Duration::from_millis(500)),
            ..playing[0]
        }];
        let count: usize = (0..200)
            .map(|tick| {
                let now = start + Duration::from_millis(tick * 10);
                frames.cut(&slow, Some(&frame(0.0)), now).0.len()
            })
            .sum();
        assert_eq!(count, 4);
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

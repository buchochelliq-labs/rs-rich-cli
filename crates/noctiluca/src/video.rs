//! Video in its region: frames of a playing `<video>`, cut from Chrome's
//! screencast, handed to the picture side to draw.
//!
//! - **Only while something plays on screen.** The tab turns Chrome's
//!   screencast on when a video plays in a region the view shows, and off
//!   when none does. It runs on demand: Chrome is kept one frame ahead
//!   of what is taken, so a take finds a fresh frame and Chrome encodes
//!   about as many as are used. Drawn as character art, a region asks for
//!   a scaled screencast: no more pixels than it is drawn in.
//! - **Cropped per region.** Each frame is the viewport; each playing
//!   region's rectangle is cut from it by the frame's own scroll position
//!   ([`ScreencastFrame::crop`]). A region Chrome's viewport does not hold
//!   (our scroll is in rows, Chrome's follows when the view rests) is
//!   captured on its own instead.
//! - **At most [`MAX_FPS`] a region, evenly spaced.** Frames are taken
//!   no faster than that, or than the app's pacing asks, on even slots a
//!   frame's time apart ([`Frames::next_due`] says when the next is due,
//!   so the tab wakes for it rather than on a coarse tick). How many of
//!   them reach the terminal, in which protocol, within the terminal's
//!   byte budget, is rs-rich-art's frame pacing ([`Pacer`]), which
//!   [`Pacing`] runs per region on the app's side; a fast terminal on
//!   this machine gets the full rate, an SSH link fewer frames, or a
//!   cheaper protocol.
//!
//! - **Encoded off the thread that draws.** A frame the pacing lets
//!   through in Kitty, iTerm2 or Sixel is encoded on [`Encoder`]'s thread
//!   before it is shown, so the painter finds its escape made and the keys
//!   never wait on it.
//!
//! Nothing here looks at a site: any `<video>` with a playing state is a
//! moving region.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rich_art::picture::{ByteBudget, Pacer, Picture, Pixels, over_ssh};
use rich_embed::ScreencastFrame;

use crate::engine::{NodeId, Rect};

/// The most frames a second a region is given: a ceiling, not a target.
/// The byte budget decides how many of them a terminal is sent.
pub const MAX_FPS: u32 = 30;

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
    /// Each moving region's last frame.
    last: Vec<Last>,
    /// The last frame's number, over every region and every stop, so a
    /// region that moves again goes on from where it stopped: its painter
    /// side keeps the number it last drew.
    seq: u64,
}

/// A moving region's last frame: the slot it was taken for (frames keep
/// to slots a frame's time apart), when it was taken, and the spacing it
/// was due at.
#[derive(Clone, Copy, Debug)]
struct Last {
    node: NodeId,
    slot: Instant,
    taken: Instant,
    every: Duration,
}

/// The spacing `region`'s frames keep to.
fn every(region: &Region) -> Duration {
    region.interval.unwrap_or(MIN_INTERVAL).max(MIN_INTERVAL)
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
        self.last.retain(|last| {
            let keep = moving.iter().any(|r| r.node == last.node);
            if !keep {
                plan.stopped.push(last.node);
            }
            keep
        });
        plan.due = moving.iter().any(|r| self.due(r, now));
        plan
    }

    /// Whether `region`'s next slot has come (an eighth of a frame early
    /// is on time: the tab wakes for it, give or take its clock), and half
    /// a frame has gone since the last was taken.
    fn due(&self, region: &Region, now: Instant) -> bool {
        self.wait(region, now).is_zero()
    }

    /// How long until `region`'s next frame is due: zero when it is.
    fn wait(&self, region: &Region, now: Instant) -> Duration {
        let every = every(region);
        let Some(last) = self.last.iter().find(|l| l.node == region.node) else {
            return Duration::ZERO;
        };
        let slot = (last.slot + every - every / 8).max(last.taken + every / 2);
        slot.saturating_duration_since(now)
    }

    /// How long until a moving region's next frame is due, if any moves:
    /// how long the tab may sleep before it looks again.
    pub fn next_due(&self, regions: &[Region], now: Instant) -> Option<Duration> {
        regions
            .iter()
            .filter(|r| r.moving())
            .map(|r| self.wait(r, now))
            .min()
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
                    cut.push(self.stamp_every(region.node, every(region), pixels, now));
                }
                _ => missing.push(*region),
            }
        }
        (cut, missing)
    }

    /// The number of a frame taken outside the regions' plan (a live
    /// canvas's capture): after every frame numbered so far, so the
    /// painter side never takes it for an older one.
    pub fn number(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// A frame of `node` taken some other way (a capture of its region).
    pub fn stamp(&mut self, node: NodeId, pixels: rich_embed::Pixels, now: Instant) -> VideoFrame {
        let every = self
            .last
            .iter()
            .find(|l| l.node == node)
            .map_or(MIN_INTERVAL, |l| l.every);
        self.stamp_every(node, every, pixels, now)
    }

    /// A frame of `node`, due every `every`, taken at `now`: on the next
    /// slot when it is near, else (a first frame, a pause) on a new run of
    /// slots from now.
    fn stamp_every(
        &mut self,
        node: NodeId,
        every: Duration,
        pixels: rich_embed::Pixels,
        now: Instant,
    ) -> VideoFrame {
        self.seq += 1;
        let seq = self.seq;
        let at = |slot: Option<Instant>| match slot.map(|s| s + every) {
            Some(next) if now + every / 4 >= next && now < next + every => next,
            _ => now,
        };
        let entry = |slot| Last {
            node,
            slot,
            taken: now,
            every,
        };
        match self.last.iter_mut().find(|l| l.node == node) {
            Some(last) => *last = entry(at(Some(last.slot))),
            None => self.last.push(entry(at(None))),
        }
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
    cell: rich_art::graphics::CellPixels,
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
            cell: rich_art::graphics::CellPixels::DEFAULT,
        }
    }

    /// Select the physical cell size used to estimate new frame costs.
    pub fn set_cell_pixels(&mut self, cell: rich_art::graphics::CellPixels) {
        self.cell = rich_art::graphics::CellPixels::new(cell.width, cell.height);
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
        region.pacer.set_cell_pixels(self.cell);
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

    /// Whether `node` is a region it paces (it has had a frame since the
    /// document opened, and is not gone).
    pub fn has(&self, node: NodeId) -> bool {
        self.regions.contains_key(&node)
    }

    /// Whether this picture belongs to the current incarnation of a region.
    pub(crate) fn owns_picture(&self, node: NodeId, picture: &Picture) -> bool {
        self.regions
            .get(&node)
            .is_some_and(|region| region.picture.id() == picture.id())
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

/// A frame to encode: its region's picture, the protocol and the cells it
/// is drawn in.
#[derive(Clone, Debug)]
pub struct Job {
    pub node: NodeId,
    pub picture: Picture,
    pub pixels: Pixels,
    pub cols: usize,
    pub rows: usize,
}

/// Video frames encoded off the thread that draws: each [`Job`]'s escape
/// is made ahead ([`Picture::prepare`]) on a thread of its own, and the
/// job handed to `done` once it is, so the painter finds it made. Only a
/// region's newest frame waits: one that comes while an older is waiting
/// takes its place.
///
/// A Sixel frame takes tens of milliseconds to encode and a Kitty one
/// several: on the thread that draws, that is time the keys and the rest
/// of the screen wait, every frame.
pub struct Encoder {
    queue: std::sync::Arc<(std::sync::Mutex<Queue>, std::sync::Condvar)>,
}

#[derive(Default)]
struct Queue {
    waiting: Vec<(Job, Option<(u16, u16)>)>,
    closed: bool,
}

impl Encoder {
    /// An encoder whose thread hands each job, encoded, to `done`. The
    /// thread ends when the encoder is dropped.
    pub fn spawn(done: impl Fn(Job) + Send + 'static) -> Encoder {
        let queue = std::sync::Arc::new((
            std::sync::Mutex::new(Queue::default()),
            std::sync::Condvar::new(),
        ));
        let shared = queue.clone();
        std::thread::spawn(move || {
            loop {
                let (job, metrics) = {
                    let (lock, ready) = &*shared;
                    let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while queue.waiting.is_empty() && !queue.closed {
                        queue = ready.wait(queue).unwrap_or_else(|e| e.into_inner());
                    }
                    if queue.closed {
                        return;
                    }
                    queue.waiting.remove(0)
                };
                let cell = metrics
                    .filter(|&(w, h)| w > 0 && h > 0)
                    .map(|(w, h)| rich_art::graphics::CellPixels::new(w.into(), h.into()))
                    .unwrap_or_default();
                job.picture.prepare_with_metrics(
                    crate::pictures::view(job.pixels),
                    job.cols,
                    job.rows,
                    cell,
                );
                done(job);
            }
        });
        Encoder { queue }
    }

    /// Encode `job`, in place of any frame of its region still waiting.
    pub fn encode(&self, job: Job) {
        self.encode_with_metrics(job, None);
    }

    /// Prepare with the terminal's measured physical cell dimensions.
    pub fn encode_with_metrics(&self, job: Job, metrics: Option<(u16, u16)>) {
        let (lock, ready) = &*self.queue;
        let mut queue = lock.lock().unwrap_or_else(|e| e.into_inner());
        queue
            .waiting
            .retain(|(waiting, _)| waiting.node != job.node);
        queue.waiting.push((job, metrics));
        ready.notify_one();
    }
}

impl Drop for Encoder {
    fn drop(&mut self) {
        let (lock, ready) = &*self.queue;
        lock.lock().unwrap_or_else(|e| e.into_inner()).closed = true;
        ready.notify_one();
    }
}

impl std::fmt::Debug for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Encoder").finish_non_exhaustive()
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
    fn video_budget_uses_selected_metrics_before_the_first_write() {
        let mut pacing = Pacing::new(ByteBudget::new(10_000_000), Pixels::Kitty);
        let cell = rich_art::graphics::CellPixels::new(12, 24);
        pacing.set_cell_pixels(cell);
        let frame = VideoFrame {
            node: 1,
            seq: 1,
            pixels: Rgb::new(4, 4, vec![255; 48]).unwrap(),
        };
        let picture = pacing.offer(&frame, 4, 3, Instant::now(), |_| 0).unwrap();
        assert_eq!(
            pacing.regions[&1].pacer.bytes_per_frame() as u64,
            Pacer::estimate_with_metrics(Pixels::Kitty, 4, 3, cell)
        );
        let view = crate::pictures::view(Pixels::Kitty);
        assert!(picture.prepare_with_metrics(view, 4, 3, cell));
        let tag = rich_art::picture::PictureTag {
            id: picture.id(),
            version: picture.version(),
            cols: 4,
            rows: 3,
            row: 0,
            pixels: Pixels::Kitty,
            fit: view.fit,
            anchor: view.anchor,
        };
        let bytes = picture
            .encode_tag_with_metrics(&tag, 0, 3, cell)
            .unwrap()
            .len() as u64;
        let pacer = &mut pacing.regions.get_mut(&1).unwrap().pacer;
        pacer.sent(Instant::now(), Some(bytes));
        assert_eq!(pacer.bytes_per_frame() as u64, bytes);
        assert!(picture.is_prepared_with_metrics(view, 4, 3, cell));
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
    fn regions_are_cut_at_most_thirty_times_a_second() {
        let mut frames = Frames::default();
        let start = Instant::now();
        let playing = [region(1, 10.0, true, true)];
        let mut count = 0;
        // Offered a frame every 5 ms for two seconds.
        for tick in 0..400 {
            let now = start + Duration::from_millis(tick * 5);
            let (cut, missing) = frames.cut(&playing, Some(&frame(0.0)), now);
            assert!(missing.is_empty());
            count += cut.len();
        }
        assert!((59..=61).contains(&count), "{count}");
        // The pacing asks for two a second: four in 1.9 seconds (the fifth
        // is due at two, an eighth of a frame early at most).
        let mut frames = Frames::default();
        let slow = [Region {
            interval: Some(Duration::from_millis(500)),
            ..playing[0]
        }];
        let count: usize = (0..190)
            .map(|tick| {
                let now = start + Duration::from_millis(tick * 10);
                frames.cut(&slow, Some(&frame(0.0)), now).0.len()
            })
            .sum();
        assert_eq!(count, 4);
    }

    #[test]
    fn the_next_frame_is_due_on_its_slot() {
        let mut frames = Frames::default();
        let start = Instant::now();
        let playing = [region(1, 10.0, true, true)];
        let paused = [region(1, 10.0, false, true)];
        // Nothing moves: no wake. A new region is due at once.
        assert_eq!(frames.next_due(&paused, start), None);
        assert_eq!(frames.next_due(&playing, start), Some(Duration::ZERO));
        frames.cut(&playing, Some(&frame(0.0)), start);
        // Then a frame's time on, give or take an eighth of one.
        let wait = frames.next_due(&playing, start).unwrap();
        assert!(
            wait <= MIN_INTERVAL && wait >= MIN_INTERVAL * 7 / 8,
            "{wait:?}"
        );
        // Taken a little late, the next keeps to the slot: no drift.
        let late = start + MIN_INTERVAL + Duration::from_millis(3);
        assert_eq!(frames.cut(&playing, Some(&frame(0.0)), late).0.len(), 1);
        let wait = frames.next_due(&playing, late).unwrap();
        assert!(wait < MIN_INTERVAL - Duration::from_millis(2), "{wait:?}");
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

    #[test]
    fn encoder_prepares_at_the_measured_cell_size() {
        let (send, done) = std::sync::mpsc::channel();
        let encoder = Encoder::spawn(move |job| drop(send.send(job)));
        let picture = Picture::from_rgba(8, 8, vec![255; 8 * 8 * 4]).unwrap();
        let view = crate::pictures::view(Pixels::Kitty);
        encoder.encode_with_metrics(
            Job {
                node: 1,
                picture: picture.clone(),
                pixels: Pixels::Kitty,
                cols: 4,
                rows: 2,
            },
            Some((10, 20)),
        );
        done.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(picture.is_prepared_with_metrics(
            view,
            4,
            2,
            rich_art::graphics::CellPixels::new(10, 20)
        ));
    }

    #[test]
    fn the_encoder_makes_the_escape_ahead_and_keeps_the_newest_waiting() {
        let (send, done) = std::sync::mpsc::channel();
        let encoder = Encoder::spawn(move |job| drop(send.send(job)));
        let first = Picture::from_rgba(8, 8, vec![9; 8 * 8 * 4]).unwrap();
        let job = |picture: &Picture| Job {
            node: 1,
            picture: picture.clone(),
            pixels: Pixels::Sixel,
            cols: 4,
            rows: 2,
        };
        encoder.encode(job(&first));
        let made = done.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(made.picture, first);
        // Made with the region's view: what the painter asks for is it.
        let tag = rich_art::picture::PictureTag {
            id: first.id(),
            version: first.version(),
            cols: 4,
            rows: 2,
            row: 0,
            pixels: Pixels::Sixel,
            fit: Some(rich_art::ImageFit::Cover),
            anchor: rich_art::ImageAnchor::Center,
        };
        assert!(first.encode_tag(&tag, 0, 2).is_some());
        // Frames of one region queued faster than they encode: the
        // newest reaches `done`, and the last one handed back is it.
        let frames: Vec<Picture> = (0..20u8)
            .scan(first.clone(), |last, k| {
                *last = last.next_rgba(8, 8, vec![k; 8 * 8 * 4])?;
                Some(last.clone())
            })
            .collect();
        for frame in &frames {
            encoder.encode(job(frame));
        }
        let mut last = None;
        while let Ok(job) = done.recv_timeout(Duration::from_millis(500)) {
            last = Some(job.picture);
        }
        assert_eq!(last.as_ref(), frames.last());
    }

    #[test]
    fn a_region_that_moves_again_goes_on_numbering() {
        let mut frames = Frames::default();
        let now = Instant::now();
        let playing = [region(1, 10.0, true, true)];
        let paused = [region(1, 10.0, false, true)];
        let mut last = 0;
        for _ in 0..3 {
            frames.plan(&playing, now);
            let (cut, _) = frames.cut(&playing, Some(&frame(0.0)), now);
            last = cut[0].seq;
            frames.plan(&paused, now);
        }
        // Paused and played again: the numbers go on, so the painter side,
        // which keeps the last it drew, takes the next frame.
        frames.plan(&playing, now);
        let (cut, _) = frames.cut(&playing, Some(&frame(0.0)), now);
        assert!(cut[0].seq > last, "{} after {last}", cut[0].seq);
    }
}

//! A tab: an [`Engine`] on a thread of its own, which keeps the page model
//! up to date and runs actions with their guards.
//!
//! - **Reading again**: the isolated world's ping (an
//!   [`EngineEvent::Changed`]) starts a [`DEBOUNCE`]; more pings within it
//!   fold into one new snapshot. A navigation, the end of a load and an
//!   action of ours read again at once. So a page that changes every frame
//!   costs one snapshot per debounce, not one per frame.
//! - **Guarded actions**: an action carries its [`Binding`]. Before it
//!   runs, the tab takes a fresh snapshot and checks the document, the node
//!   and its meaning revision ([`Page::check`]); a disabled control, and a
//!   value the page changed under a draft (a commit's base), refuse it.
//!   Nothing is sent to the page then.
//! - **Live logs**: a page's live logs (`role=log`, `aria-live`, a scroll
//!   box it appends to) are followed through the live ping
//!   ([`EngineEvent::Live`]): only their new and changed entries are read
//!   ([`Engine::read_live`]), never the whole page, and the page is
//!   published again with them.
//! - **Pictures**: each image's bytes are read after its page is
//!   published, so text never waits for pictures: the original file the
//!   page loaded (`Page.getResourceContent`, or a `data:` address's own
//!   bytes), else a still of Chrome's pixels. A decoder thread makes them
//!   [`Picture`]s (an animated GIF keeps its frames), and the page is
//!   published again with them.
//! - **Frames**: while a video plays in a region the view shows, the
//!   screencast runs and each playing region's frames are cut from it
//!   ([`crate::video`]), at most eight a second, or as the app's pacing
//!   asks ([`Command::Regions`]). A video's still is its poster.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rich_embed::PageState;

use crate::engine::{
    Engine, EngineEvent, Format, HistoryStep, MediaOp, Metrics, NodeId, PageAction, Rect, TabId,
};
use crate::model::{
    build_with, live_runs, Binding, Control, Entry, MediaKind, Page, Revisions, Role, Run,
};
use crate::video::{Frames, Region, VideoFrame};
use rich_intuituive::Picture;

/// How long after the first change ping the page is read again.
pub const DEBOUNCE: Duration = Duration::from_millis(100);
/// How often the tab looks at its engine when nothing happens.
const TICK: Duration = Duration::from_millis(25);
/// At most this many stills are captured per reading.
const STILLS_PER_READ: usize = 8;
/// How often a playing video's or audio's time is read again.
const PLAYBACK: Duration = Duration::from_millis(500);
/// How long without a screencast frame before playing regions are
/// captured on their own.
const QUIET: Duration = Duration::from_millis(400);

/// What an action does to its control.
#[derive(Clone, Debug, PartialEq)]
pub enum Intent {
    /// Click it: a button, a link, a check box, a radio.
    Click,
    Focus,
    /// Replace a field's value. `base` is the value the edit started from;
    /// if the page's value is no longer that, the commit is a conflict.
    SetValue {
        value: String,
        base: Option<String>,
    },
    /// Choose an option, by its key.
    Select {
        option: NodeId,
    },
    SetRange(f64),
}

/// What the tab is asked to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Open(String),
    History(HistoryStep),
    /// An action on a control, guarded by its binding.
    Act {
        binding: Binding,
        intent: Intent,
    },
    /// Activate the `nth` control (from 0) with this role and accessible
    /// name, looked up now: a site key's target.
    Activate {
        role: String,
        name: String,
        nth: usize,
    },
    /// Scroll the page so this many CSS pixels are above its viewport: a
    /// jump. `f64::MAX` is the bottom.
    Scroll(f64),
    /// Turn the wheel by this many CSS pixels: the page sees the user
    /// scroll, so its lazy loading and infinite feeds go on.
    Wheel(f64),
    /// The reader is near the end of what was read: scroll the page to its
    /// bottom, so an infinite feed loads more. The page is read again on
    /// its change ping, as always.
    LoadMore,
    /// Play, pause, mute or set the volume of the video or audio element
    /// `node` of the document `document`.
    Media {
        document: u64,
        node: NodeId,
        op: MediaOp,
    },
    Resize(u16, u16),
    /// Read the page again now.
    Refresh,
    /// The media regions the view shows now, and how long the app's pacing
    /// wants between two frames of each (none: [`crate::video::MAX_FPS`]).
    Regions {
        on_screen: Vec<NodeId>,
        intervals: Vec<(NodeId, Duration)>,
    },
}

/// Why an action did not run, or failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The target went, changed meaning, or the document changed.
    Stale(String),
    /// The control is disabled, read-only, or not on the page.
    Unavailable(String),
    /// The page changed the value under the edit: this is its value now.
    Conflict { remote: String },
    /// The page or the engine refused it.
    Failed(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Stale(why) | Refusal::Unavailable(why) | Refusal::Failed(why) => {
                f.write_str(why)
            }
            Refusal::Conflict { remote } => write!(
                f,
                "Edit conflict: the page changed this value to {:?}. Reload it before applying.",
                crate::clean::truncate(remote, 60)
            ),
        }
    }
}

/// What the tab tells its owner.
#[derive(Clone, Debug)]
pub enum Update {
    /// A new reading of the page.
    Page(Arc<Page>),
    /// The address, title, loading state or history moved.
    State(PageState),
    /// The command with this id finished.
    Done(u64, Result<(), Refusal>),
    /// Where the page is scrolled and how tall it is, read after a scroll.
    Metrics(Metrics),
    /// A download began, and waits in the quarantine.
    Download {
        guid: String,
        name: String,
        url: String,
    },
    DownloadDone {
        guid: String,
        path: String,
    },
    /// A new frame of a playing region.
    Frame(Arc<VideoFrame>),
    /// The region stopped moving: paused, ended, gone or off screen.
    Still(NodeId),
    /// The about page was asked for ([`crate::about::ADDRESS`]): the
    /// browser's answers to `canPlayType` for each of
    /// [`crate::about::CODECS`], in order, and its name and version.
    About {
        answers: Vec<String>,
        product: String,
    },
}

/// The owner's end of a tab.
#[derive(Clone, Debug)]
pub struct Tab {
    commands: Sender<(u64, Command)>,
    next: Arc<std::sync::atomic::AtomicU64>,
}

impl Tab {
    /// Run `engine` on a thread of its own. `update` is called on that
    /// thread with each [`Update`].
    pub fn spawn(engine: Box<dyn Engine>, update: impl Fn(Update) + Send + 'static) -> Tab {
        let (commands, receive) = mpsc::channel();
        thread::spawn(move || run(engine, receive, &update));
        Tab {
            commands,
            next: Arc::default(),
        }
    }

    /// Send `command`; its id comes back in [`Update::Done`].
    pub fn send(&self, command: Command) -> u64 {
        let id = self.next.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
        let _ = self.commands.send((id, command));
        id
    }
}

/// Whether a refusal comes from `control` as it is: disabled, read-only,
/// or a value the page moved under an edit.
fn guard(control: &Control, intent: &Intent) -> Result<(), Refusal> {
    if control.disabled {
        return Err(Refusal::Unavailable(format!(
            "{} is disabled.",
            describe(control)
        )));
    }
    match intent {
        Intent::SetValue { base, .. } => {
            if control.readonly {
                return Err(Refusal::Unavailable(format!(
                    "{} is read-only.",
                    describe(control)
                )));
            }
            // A password's value is never read, so it cannot be checked:
            // it is replaced as typed.
            if let (Some(base), Some(now)) = (base, &control.value) {
                if base != now {
                    return Err(Refusal::Conflict {
                        remote: now.clone(),
                    });
                }
            }
        }
        Intent::Select { option } => {
            let choice = control
                .options
                .iter()
                .find(|o| o.key == *option)
                .ok_or_else(|| Refusal::Stale("Unknown or stale option key.".into()))?;
            if choice.disabled {
                return Err(Refusal::Unavailable("That option is disabled.".into()));
            }
        }
        _ => {}
    }
    Ok(())
}

/// A control as a reason says it: `The "Send" button`.
pub fn describe(control: &Control) -> String {
    let role = match control.role {
        Role::Password => "password field",
        Role::TextBox | Role::SearchBox => "field",
        Role::CheckBox => "check box",
        Role::ComboBox => "select",
        ref other => other.name(),
    };
    if control.name.is_empty() {
        format!("This {role}")
    } else {
        format!("The {:?} {role}", crate::clean::truncate(&control.name, 40))
    }
}

/// The page action for an intent on `control`.
fn action(control: &Control, intent: &Intent) -> PageAction {
    match intent {
        Intent::Click => PageAction::Click(control.node),
        Intent::Focus => PageAction::Focus(control.node),
        Intent::SetValue { value, .. } => PageAction::SetValue {
            node: control.node,
            value: value.clone(),
        },
        Intent::Select { option } => PageAction::SelectOption {
            select: control.node,
            option: *option,
        },
        Intent::SetRange(value) => PageAction::SetRange {
            node: control.node,
            value: *value,
        },
    }
}

struct Worker<'a> {
    engine: Box<dyn Engine>,
    update: &'a dyn Fn(Update),
    revisions: Revisions,
    page: Option<Arc<Page>>,
    state: PageState,
    /// When the first change ping since the last reading came.
    dirty: Option<Instant>,
    stills: HashMap<StillKey, Arc<[u8]>>,
    /// The pictures decoded from the stills.
    pictures: HashMap<StillKey, Picture>,
    /// Stills to decode, to the decoder thread.
    decode: Sender<Job>,
    /// Pictures decoded, from the decoder thread.
    decoded: Receiver<(StillKey, Picture)>,
    /// The small pictures in lines, decoded by source, and those asked for.
    inline: HashMap<String, Picture>,
    inline_asked: HashSet<String>,
    decode_inline: Sender<(String, Arc<[u8]>)>,
    decoded_inline: Receiver<(String, Picture)>,
    /// Where Chrome's view was last seen: images near it are captured
    /// first.
    metrics: Option<Metrics>,
    frames: Frames,
    /// The regions the view shows, and the pacing's intervals.
    on_screen: HashSet<NodeId>,
    intervals: HashMap<NodeId, Duration>,
    /// When the last screencast frame came.
    last_frame: Option<Instant>,
    /// A paused region's time when its frame was last taken: a seek while
    /// paused takes one more.
    shown_time: HashMap<NodeId, f64>,
    /// Each live log's entries as read, by log and entry key.
    live: HashMap<NodeId, HashMap<u64, Vec<Run>>>,
    /// The scroll boxes the page appends to, as the engine last said.
    appended: HashSet<NodeId>,
}

impl Worker<'_> {
    /// Read the page and publish it; `None` if the engine refused (a
    /// navigation swapping the page's process, say).
    fn read(&mut self) -> Option<Arc<Page>> {
        let snapshot = self.engine.snapshot(TabId(0)).ok()?;
        if let Ok(boxes) = self.engine.appended(TabId(0)) {
            self.appended = boxes.into_iter().collect();
        }
        let mut page = build_with(&snapshot, &mut self.revisions, &self.appended);
        if self.page.as_ref().map(|p| p.document) != Some(page.document) {
            self.live.clear();
        }
        self.follow_logs(&mut page);
        self.dress_inline(&mut page);
        for media in &mut page.media {
            let key = still_key(media.node, picture_source(media), media.rect);
            if let Some(still) = self.stills.get(&key) {
                media.still = Some(still.clone());
            }
            media.picture = self.pictures.get(&key).cloned();
        }
        self.probe(&mut page);
        let page = Arc::new(page);
        if self.page.as_deref() != Some(&*page) {
            (self.update)(Update::Page(page.clone()));
        }
        self.page = Some(page.clone());
        self.dirty = None;
        Some(page)
    }

    /// Read the live logs' new and changed entries into `page`. An engine
    /// that cannot read them leaves the snapshot's entries.
    fn follow_logs(&mut self, page: &mut Page) {
        let base = page.url.clone();
        for log in &mut page.logs {
            let Ok(mut read) = self.engine.read_live(TabId(0), log.node, false) else {
                continue;
            };
            let cache = self.live.entry(log.node).or_default();
            let missing = |cache: &HashMap<u64, Vec<Run>>, read: &crate::engine::LiveRead| {
                read.order
                    .iter()
                    .any(|k| !cache.contains_key(k) && !read.fresh.iter().any(|(f, _)| f == k))
            };
            if missing(cache, &read) {
                // Out of step (a new reading of ours, say): all of it again.
                match self.engine.read_live(TabId(0), log.node, true) {
                    Ok(again) => read = again,
                    Err(_) => continue,
                }
            }
            let cache = self.live.entry(log.node).or_default();
            for (key, runs) in &read.fresh {
                cache.insert(*key, live_runs(runs, &base));
            }
            let keep: HashSet<u64> = read.order.iter().copied().collect();
            cache.retain(|k, _| keep.contains(k));
            log.entries = read
                .order
                .iter()
                .filter_map(|k| {
                    let runs = cache.get(k)?;
                    (!runs.is_empty()).then(|| Entry {
                        key: *k,
                        runs: runs.clone(),
                    })
                })
                .collect();
            if log.entries.len() > crate::model::MAX_ENTRIES {
                log.entries
                    .drain(..log.entries.len() - crate::model::MAX_ENTRIES);
            }
            log.first_shown = read.first_shown;
        }
    }

    /// Only live logs changed: read their entries, not the page, and
    /// publish the page with them.
    fn read_live(&mut self) {
        let Some(page) = self.page.clone() else {
            return;
        };
        if page.logs.is_empty() {
            return;
        }
        let mut next = (*page).clone();
        self.follow_logs(&mut next);
        self.dress_inline(&mut next);
        if next != *page {
            let next = Arc::new(next);
            self.page = Some(next.clone());
            (self.update)(Update::Page(next));
        }
    }

    /// Give `page` the small pictures of its lines that are decoded, and
    /// ask for those not yet asked for: each source's original bytes (or a
    /// `data:` address's), decoded once whatever number of lines show it.
    fn dress_inline(&mut self, page: &mut Page) {
        let mut sources: Vec<String> = Vec::new();
        let mut note = |run: &Run| {
            if let Some(picture) = run.picture.as_ref().filter(|p| !p.src.is_empty()) {
                if !sources.contains(&picture.src) {
                    sources.push(picture.src.clone());
                }
            }
        };
        for block in &page.blocks {
            block.runs.iter().for_each(&mut note);
        }
        for log in &page.logs {
            for entry in &log.entries {
                entry.runs.iter().for_each(&mut note);
            }
        }
        page.inline_pictures.clear();
        for src in sources {
            if let Some(picture) = self.inline.get(&src) {
                page.inline_pictures.insert(src, picture.clone());
                continue;
            }
            if self.inline_asked.len() >= MAX_INLINE || !self.inline_asked.insert(src.clone()) {
                continue;
            }
            let bytes = if let Some(data) = src.strip_prefix("data:") {
                data_bytes(data)
            } else {
                self.engine.resource(TabId(0), &src).ok()
            };
            if let Some(bytes) = bytes.filter(|b| decodable(b)) {
                let _ = self.decode_inline.send((src, bytes.into()));
            }
        }
    }

    /// Read each video's and audio's playback from the element.
    fn probe(&mut self, page: &mut Page) {
        for media in &mut page.media {
            if matches!(media.kind, MediaKind::Video | MediaKind::Audio) {
                media.playback = self.engine.playback(TabId(0), media.node).ok();
            }
        }
    }

    /// While something plays, its time moves with no page change: read
    /// the playback again, and publish it if it moved.
    fn follow_playback(&mut self) {
        let Some(page) = self.page.clone() else {
            return;
        };
        if !page
            .media
            .iter()
            .any(|m| m.playback.is_some_and(|p| !p.paused))
        {
            return;
        }
        let mut next = (*page).clone();
        self.probe(&mut next);
        if next != *page {
            let next = Arc::new(next);
            self.page = Some(next.clone());
            (self.update)(Update::Page(next));
        }
    }

    /// Capture the images that have no still yet, and publish the page
    /// again with them.
    fn capture_stills(&mut self) {
        let Some(page) = self.page.clone() else {
            return;
        };
        // Nearest Chrome's view first; an image with no picture yet (a lazy
        // one far from the view) waits: its load pings, and it is captured
        // at the reading after.
        let view = self.metrics.map_or(0.0, |m| m.scroll_y);
        // A video with no poster to fetch shows what Chrome paints in its
        // region: its poster, or its first frame.
        let mut candidates: Vec<_> = page
            .media
            .iter()
            .filter(|m| matches!(m.kind, MediaKind::Image | MediaKind::Video) && m.still.is_none())
            .filter(|m| m.rect.width >= 4.0 && m.rect.height >= 4.0)
            .collect();
        candidates.sort_by(|a, b| (a.rect.y - view).abs().total_cmp(&(b.rect.y - view).abs()));
        let mut wanted = Vec::new();
        for m in candidates {
            if wanted.len() == STILLS_PER_READ {
                break;
            }
            if self.engine.image_ready(TabId(0), m.node).unwrap_or(true) {
                wanted.push((m.node, picture_source(m).to_string(), m.rect));
            }
        }
        if wanted.is_empty() {
            return;
        }
        let mut page = (*page).clone();
        let mut any = false;
        for (node, src, rect) in wanted {
            if let Some(bytes) = self.picture_bytes(&src, rect) {
                let still: Arc<[u8]> = bytes.into();
                let key = still_key(node, &src, rect);
                self.stills.insert(key.clone(), still.clone());
                let _ = self.decode.send((key, still.clone()));
                for media in page.media.iter_mut().filter(|m| m.node == node) {
                    media.still = Some(still.clone());
                }
                any = true;
            }
        }
        if any {
            let page = Arc::new(page);
            self.page = Some(page.clone());
            (self.update)(Update::Page(page));
        }
    }

    /// An image's bytes: the original file the page loaded (from the
    /// browser's cache, so animated GIFs keep their frames), a `data:`
    /// address's own bytes, else a still of Chrome's pixels of `rect` (an
    /// SVG, or a picture no longer in the cache).
    fn picture_bytes(&mut self, src: &str, rect: Rect) -> Option<Vec<u8>> {
        let original = if let Some(data) = src.strip_prefix("data:") {
            data_bytes(data)
        } else if src.is_empty() {
            None
        } else {
            self.engine.resource(TabId(0), src).ok()
        };
        if let Some(bytes) = original.filter(|b| decodable(b)) {
            return Some(bytes);
        }
        let region = Rect::new(
            rect.x,
            rect.y,
            rect.width.min(4000.0),
            rect.height.min(4000.0),
        );
        self.engine
            .capture(TabId(0), region, Format::Png)
            .ok()
            .map(|frame| frame.bytes)
    }

    /// Put the pictures decoded since the last look on the page, and
    /// publish it again.
    fn take_decoded(&mut self) {
        let mut any = false;
        while let Ok((key, picture)) = self.decoded.try_recv() {
            self.pictures.insert(key, picture);
            any = true;
        }
        while let Ok((src, picture)) = self.decoded_inline.try_recv() {
            self.inline.insert(src, picture);
            any = true;
        }
        let Some(page) = self.page.clone().filter(|_| any) else {
            return;
        };
        let mut next = (*page).clone();
        for media in &mut next.media {
            let key = still_key(media.node, picture_source(media), media.rect);
            if let Some(picture) = self.pictures.get(&key) {
                media.picture = Some(picture.clone());
            }
        }
        self.dress_inline(&mut next);
        if next != *page {
            let next = Arc::new(next);
            self.page = Some(next.clone());
            (self.update)(Update::Page(next));
        }
    }

    /// Scroll the page, then say where it is. A document taller than the
    /// one read (a feed that loaded more without a ping yet) is read again
    /// soon.
    fn scroll(&mut self, action: PageAction) -> std::io::Result<()> {
        self.engine.act(TabId(0), &action)?;
        if let Ok(metrics) = self.engine.metrics(TabId(0)) {
            let read = self.page.as_ref().map_or(0.0, |p| p.height);
            if metrics.content_height > read + 1.0 {
                self.dirty.get_or_insert_with(Instant::now);
            }
            self.metrics = Some(metrics);
            (self.update)(Update::Metrics(metrics));
        }
        Ok(())
    }

    /// The video regions now.
    fn regions(&self) -> Vec<Region> {
        let Some(page) = &self.page else {
            return Vec::new();
        };
        page.media
            .iter()
            .filter(|m| m.kind == MediaKind::Video)
            .map(|m| Region {
                node: m.node,
                rect: m.rect,
                playing: m.playback.is_some_and(|p| p.playing()),
                on_screen: self.on_screen.contains(&m.node),
                interval: self.intervals.get(&m.node).copied(),
            })
            .collect()
    }

    /// Run the screencast while a region plays on screen, and send each
    /// region's frames when they are due.
    fn video(&mut self) {
        let regions = self.regions();
        let now = Instant::now();
        let plan = self.frames.plan(&regions, now);
        if let Some(on) = plan.screencast {
            let _ = self.engine.set_screencast(TabId(0), on);
            self.last_frame = on.then_some(now);
        }
        for node in plan.stopped {
            (self.update)(Update::Still(node));
        }
        self.refresh_paused(&regions);
        if !plan.due {
            return;
        }
        let frame = self.engine.screencast_frame(TabId(0));
        if frame.is_some() {
            self.last_frame = Some(now);
        }
        // No new frame for a while: what Chrome's viewport does not hold
        // is not painted there, so it is captured on its own.
        let quiet = self
            .last_frame
            .is_none_or(|at| now.duration_since(at) >= QUIET);
        if frame.is_none() && !quiet {
            return;
        }
        let (cut, missing) = self.frames.cut(&regions, frame.as_ref(), now);
        for video in cut {
            self.shown_time.remove(&video.node);
            (self.update)(Update::Frame(Arc::new(video)));
        }
        if frame.is_some() && !quiet {
            // The frame came; regions out of it wait for the quiet spell
            // before a capture, unless Chrome's view is far from them.
            let view = self
                .metrics
                .map(|m| (m.scroll_y, m.scroll_y + m.viewport_height));
            let missing: Vec<Region> = missing
                .into_iter()
                .filter(|r| {
                    view.is_some_and(|(top, bottom)| r.rect.bottom() < top || r.rect.y > bottom)
                })
                .collect();
            self.capture_regions(&missing, now);
        } else {
            self.capture_regions(&missing, now);
        }
    }

    /// Frames of regions taken with a capture of each.
    fn capture_regions(&mut self, regions: &[Region], now: Instant) {
        for region in regions {
            if let Some(pixels) = self.capture_pixels(region.rect) {
                let video = self.frames.stamp(region.node, pixels, now);
                (self.update)(Update::Frame(Arc::new(video)));
            }
        }
    }

    fn capture_pixels(&mut self, rect: Rect) -> Option<rich_embed::Pixels> {
        let region = Rect::new(
            rect.x,
            rect.y,
            rect.width.min(4000.0),
            rect.height.min(4000.0),
        );
        let frame = self.engine.capture(TabId(0), region, Format::Jpeg).ok()?;
        let image = image::load_from_memory(&frame.bytes).ok()?.to_rgb8();
        rich_embed::Pixels::new(image.width(), image.height(), image.into_raw())
    }

    /// A paused region on screen whose time moved (a seek) gets one new
    /// frame, so the picture shows where it went.
    fn refresh_paused(&mut self, regions: &[Region]) {
        let Some(page) = self.page.clone() else {
            return;
        };
        for region in regions.iter().filter(|r| r.on_screen && !r.playing) {
            let Some(time) = page
                .media
                .iter()
                .find(|m| m.node == region.node)
                .and_then(|m| m.playback)
                .map(|p| p.time)
            else {
                continue;
            };
            let shown = *self.shown_time.entry(region.node).or_insert(time);
            if (shown - time).abs() > 0.01 {
                self.shown_time.insert(region.node, time);
                self.capture_regions(std::slice::from_ref(region), Instant::now());
            }
        }
    }

    fn run_command(&mut self, command: Command) -> Result<(), Refusal> {
        let failed = |e: std::io::Error| Refusal::Failed(crate::clean::clean(&e.to_string()));
        match command {
            Command::Open(url) if url == crate::about::ADDRESS => {
                // The about page, with the browser's own codec answers.
                let answers = self
                    .engine
                    .can_play(TabId(0), &crate::about::types())
                    .map_err(failed)?;
                let product = self.engine.product(TabId(0));
                (self.update)(Update::About { answers, product });
                Ok(())
            }
            Command::Open(url) => self.engine.open(TabId(0), &url).map_err(failed),
            Command::History(step) => self.engine.history(TabId(0), step).map_err(failed),
            Command::Resize(columns, rows) => self.engine.resize(columns, rows).map_err(failed),
            Command::Scroll(y) => self.scroll(PageAction::ScrollTo(y)).map_err(failed),
            Command::Wheel(delta) => self.scroll(PageAction::Wheel(delta)).map_err(failed),
            Command::LoadMore => self.scroll(PageAction::ScrollTo(f64::MAX)).map_err(failed),
            Command::Refresh => {
                self.read();
                Ok(())
            }
            Command::Regions {
                on_screen,
                intervals,
            } => {
                self.on_screen = on_screen.into_iter().collect();
                self.intervals = intervals.into_iter().collect();
                Ok(())
            }

            Command::Media { document, node, op } => {
                let page = self
                    .read()
                    .ok_or_else(|| Refusal::Stale("The page is not ready.".into()))?;
                if page.document != document {
                    return Err(Refusal::Stale(
                        "Stale document: the tab went to another page.".into(),
                    ));
                }
                if !page
                    .media
                    .iter()
                    .any(|m| m.node == node && m.playback.is_some())
                {
                    return Err(Refusal::Stale(
                        "That video or audio is gone from the page.".into(),
                    ));
                }
                self.engine
                    .act(TabId(0), &PageAction::Media { node, op })
                    .map_err(failed)
            }
            Command::Act { binding, intent } => {
                let page = self
                    .read()
                    .ok_or_else(|| Refusal::Stale("The page is not ready.".into()))?;
                let control = page.check(&binding).map_err(Refusal::Stale)?;
                guard(control, &intent)?;
                self.engine
                    .act(TabId(0), &action(control, &intent))
                    .map_err(failed)
            }
            Command::Activate { role, name, nth } => {
                let page = self
                    .read()
                    .ok_or_else(|| Refusal::Stale("The page is not ready.".into()))?;
                let found = page.find(&role, &name);
                let control = found.get(nth).ok_or_else(|| {
                    Refusal::Unavailable(if found.is_empty() {
                        format!("No {role} named {name:?} on this page.")
                    } else {
                        format!("Only {} {role}s named {name:?} on this page.", found.len())
                    })
                })?;
                let intent = if control.role.is_text() {
                    Intent::Focus
                } else {
                    Intent::Click
                };
                guard(control, &intent)?;
                self.engine
                    .act(TabId(0), &action(control, &intent))
                    .map_err(failed)
            }
        }
    }
}

/// At most this many sources of small pictures in lines are read.
const MAX_INLINE: usize = 2000;

/// What a still is kept by: the node, its address and its size.
type StillKey = (NodeId, String, u64, u64);
/// A still for the decoder thread.
type Job = (StillKey, Arc<[u8]>);

/// Whether rs-rich-art can decode `bytes` (an SVG cannot: it gets a
/// still of Chrome's pixels instead).
fn decodable(bytes: &[u8]) -> bool {
    image::guess_format(bytes).is_ok()
}

/// The bytes of a `data:` address's body (after `data:`), when it is
/// base64; at most 16 MiB.
fn data_bytes(data: &str) -> Option<Vec<u8>> {
    let (head, body) = data.split_once(',')?;
    if !head.ends_with(";base64") || body.len() > 22 << 20 {
        return None;
    }
    crate::cdp::base64_decode(body)
}

/// The decoder thread: each still into a picture, an animated GIF with its
/// frames. It ends when the tab does.
fn decoder<K: Send + 'static>() -> (Sender<(K, Arc<[u8]>)>, Receiver<(K, Picture)>) {
    let (send, jobs) = mpsc::channel::<(K, Arc<[u8]>)>();
    let (done, decoded) = mpsc::channel();
    thread::spawn(move || {
        while let Ok((key, bytes)) = jobs.recv() {
            if let Ok(picture) = Picture::decode_animated(&bytes) {
                if done.send((key, picture)).is_err() {
                    return;
                }
            }
        }
    });
    (send, decoded)
}

/// Where a media region's picture comes from: a video's poster when it has
/// one (else what Chrome paints in its region), an image's source.
fn picture_source(media: &crate::model::Media) -> &str {
    if media.kind == MediaKind::Video {
        &media.poster
    } else {
        &media.src
    }
}

fn still_key(node: NodeId, src: &str, rect: Rect) -> StillKey {
    (
        node,
        src.to_string(),
        rect.width.round() as u64,
        rect.height.round() as u64,
    )
}

fn run(engine: Box<dyn Engine>, commands: Receiver<(u64, Command)>, update: &dyn Fn(Update)) {
    let (decode, decoded) = decoder();
    let (decode_inline, decoded_inline) = decoder();
    let mut worker = Worker {
        engine,
        update,
        revisions: Revisions::new(),
        page: None,
        state: PageState::default(),
        dirty: None,
        stills: HashMap::new(),
        pictures: HashMap::new(),
        decode,
        decoded,
        inline: HashMap::new(),
        inline_asked: HashSet::new(),
        decode_inline,
        decoded_inline,
        metrics: None,
        frames: Frames::default(),
        on_screen: HashSet::new(),
        intervals: HashMap::new(),
        last_frame: None,
        shown_time: HashMap::new(),
        live: HashMap::new(),
        appended: HashSet::new(),
    };
    let mut live_due = false;
    // Read once at the start, then whenever something says to.
    let mut read_now = true;
    let mut probed = Instant::now();
    loop {
        match commands.recv_timeout(TICK) {
            Ok((id, command)) => {
                let acts = matches!(
                    command,
                    Command::Act { .. }
                        | Command::Activate { .. }
                        | Command::Scroll(_)
                        | Command::Media { .. }
                );
                let result = worker.run_command(command);
                update(Update::Done(id, result));
                // An action of ours reads again at once.
                read_now |= acts;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        for event in worker.engine.events() {
            match event {
                EngineEvent::Changed => {
                    worker.dirty.get_or_insert_with(Instant::now);
                }
                EngineEvent::Live => live_due = true,
                EngineEvent::Navigated(_) | EngineEvent::Loaded => read_now = true,
                EngineEvent::DownloadBegan { guid, name, url } => {
                    update(Update::Download { guid, name, url })
                }
                EngineEvent::DownloadDone { guid, path } => {
                    update(Update::DownloadDone { guid, path })
                }
            }
        }
        let state = worker.engine.state(TabId(0));
        if state != worker.state {
            if worker.state.loading && !state.loading {
                read_now = true;
            }
            worker.state = state.clone();
            update(Update::State(state));
        }
        worker.take_decoded();
        if probed.elapsed() >= PLAYBACK {
            probed = Instant::now();
            worker.follow_playback();
        }
        worker.video();
        let due = worker
            .dirty
            .is_some_and(|since| since.elapsed() >= DEBOUNCE);
        if read_now || due {
            // A refused reading stays due, and is tried at the next tick.
            if worker.read().is_some() {
                read_now = false;
                live_due = false;
                worker.capture_stills();
            } else {
                worker.dirty.get_or_insert_with(Instant::now);
            }
        } else if live_due && worker.dirty.is_none() {
            // The isolated world debounced it already: read the logs now.
            live_due = false;
            worker.read_live();
        }
    }
}

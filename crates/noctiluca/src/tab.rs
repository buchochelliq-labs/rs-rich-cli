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
//! - **Stills**: images get a still, captured from Chrome's own pixels,
//!   after their page is published, so text never waits for pictures. A
//!   video's still is its poster, as the bytes the page loaded, else a
//!   capture of its region.
//! - **Frames**: while a video plays in a region the view shows, the
//!   screencast runs and each playing region's frames are cut from it
//!   ([`crate::video`]), at most eight a second, or as the app's pacing
//!   asks ([`Command::Regions`]).

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use rich_embed::PageState;

use crate::engine::{
    Engine, EngineEvent, Format, HistoryStep, MediaOp, Metrics, NodeId, PageAction, Rect, TabId,
};
use crate::model::{build, Binding, Control, MediaKind, Page, Revisions, Role};
use crate::video::{Frames, Region, VideoFrame};

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
    stills: HashMap<(NodeId, String, u64, u64), Arc<[u8]>>,
    /// Where Chrome's view was last seen: images near it are captured
    /// first.
    metrics: Option<Metrics>,
    /// Posters by address: the bytes, or `None` when they could not be had.
    posters: HashMap<String, Option<Arc<[u8]>>>,
    frames: Frames,
    /// The regions the view shows, and the pacing's intervals.
    on_screen: HashSet<NodeId>,
    intervals: HashMap<NodeId, Duration>,
    /// When the last screencast frame came.
    last_frame: Option<Instant>,
    /// A paused region's time when its frame was last taken: a seek while
    /// paused takes one more.
    shown_time: HashMap<NodeId, f64>,
}

impl Worker<'_> {
    /// Read the page and publish it; `None` if the engine refused (a
    /// navigation swapping the page's process, say).
    fn read(&mut self) -> Option<Arc<Page>> {
        let snapshot = self.engine.snapshot(TabId(0)).ok()?;
        let mut page = build(&snapshot, &mut self.revisions);
        for media in &mut page.media {
            if let Some(still) = self
                .stills
                .get(&still_key(media.node, &media.src, media.rect))
            {
                media.still = Some(still.clone());
            }
            if media.kind == MediaKind::Video && !media.poster.is_empty() {
                let poster = media.poster.clone();
                let bytes = self
                    .posters
                    .entry(poster.clone())
                    .or_insert_with(|| {
                        self.engine
                            .resource(TabId(0), &poster)
                            .ok()
                            .filter(|b| !b.is_empty())
                            .map(Into::into)
                    })
                    .clone();
                if bytes.is_some() {
                    media.still = bytes;
                }
            }
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
                wanted.push((m.node, m.src.clone(), m.rect));
            }
        }
        if wanted.is_empty() {
            return;
        }
        let mut page = (*page).clone();
        let mut any = false;
        for (node, src, rect) in wanted {
            let region = Rect::new(
                rect.x,
                rect.y,
                rect.width.min(4000.0),
                rect.height.min(4000.0),
            );
            if let Ok(frame) = self.engine.capture(TabId(0), region, Format::Png) {
                let still: Arc<[u8]> = frame.bytes.into();
                self.stills
                    .insert(still_key(node, &src, rect), still.clone());
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
                let html = crate::about::page(&answers, &product);
                self.engine
                    .open(TabId(0), &crate::about::data_url(&html))
                    .map_err(failed)
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

fn still_key(node: NodeId, src: &str, rect: Rect) -> (NodeId, String, u64, u64) {
    (
        node,
        src.to_string(),
        rect.width.round() as u64,
        rect.height.round() as u64,
    )
}

fn run(engine: Box<dyn Engine>, commands: Receiver<(u64, Command)>, update: &dyn Fn(Update)) {
    let mut worker = Worker {
        engine,
        update,
        revisions: Revisions::new(),
        page: None,
        state: PageState::default(),
        dirty: None,
        stills: HashMap::new(),
        metrics: None,
        posters: HashMap::new(),
        frames: Frames::default(),
        on_screen: HashSet::new(),
        intervals: HashMap::new(),
        last_frame: None,
        shown_time: HashMap::new(),
    };
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
                worker.capture_stills();
            } else {
                worker.dirty.get_or_insert_with(Instant::now);
            }
        }
    }
}

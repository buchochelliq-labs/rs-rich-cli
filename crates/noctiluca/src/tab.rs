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
//!   ([`crate::video`]), at most thirty a second, or as the app's pacing
//!   asks ([`Command::Regions`]), each when its slot comes, at the scale
//!   the regions are drawn in. A video's still is its poster.
//! - **Live regions**: a drawing the page changes is drawn again. An
//!   inline `<svg>`'s still is kept by its content's revision
//!   ([`Media::revision`](crate::model::Media::revision)), so a reading
//!   that sees it changed captures it again; already accepted SVG pixels
//!   remain visible while the same drawing's new revision decodes. Late
//!   old-revision completions cannot supply that fallback. A `<canvas>` changes
//!   no DOM: while the view shows it, it is captured at a low rate ([`CANVAS_POLL`], [`CANVAS_IDLE`]
//!   once it holds still), and each capture that changed goes out as a
//!   frame, paced within the byte budget as a video's are. At most
//!   [`MAX_LIVE_CANVASES`] are watched at once, nearest the view; one
//!   that changes at every capture for [`CANVAS_BUSY`] and is bigger than
//!   a quarter of the viewport is cut from the screencast, as a playing
//!   video is. Off screen, or frozen, none is captured.
//! - **Failed navigations**: Chrome shows its own error page at
//!   [`ERROR_PAGE`]'s address. The tab keeps the address that was asked
//!   for in the page and its state instead, and puts a notice over
//!   Chrome's text saying what failed, where, and how to try again
//!   ([`ERROR_NOTICE`] is its node).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use rich_embed::PageState;

use crate::engine::{
    Engine, EngineEvent, Format, HistoryStep, MediaOp, Metrics, NodeId, PageAction, Rect, TabId,
};
use crate::model::{
    Binding, Block, BlockKind, Control, Entry, Media, MediaKind, Page, Revisions, Role, Run,
    SourcePage, build_source_with, build_with, live_runs,
};
use crate::video::{Frames, Region, VideoFrame};
use rich_intuituive::Picture;

/// How long after the first change ping the page is read again.
pub const DEBOUNCE: Duration = Duration::from_millis(100);
/// How long after a new document's reading it is read again, to see
/// whether its layout has settled (web fonts, a sticky header put in
/// place, a script's rendering move boxes without changing the DOM).
pub const SETTLE_STEP: Duration = Duration::from_millis(250);
/// How long a new document is watched for its layout to settle.
pub const SETTLE_MAX: Duration = Duration::from_secs(3);
/// How long a new document is watched at least: a quiet step sooner does
/// not settle it, since a script's first move can come after one.
pub const SETTLE_MIN: Duration = Duration::from_secs(1);
/// A reading the page pinged for waits this many times the last
/// reading's cost after it ended, so pinged readings take at most a
/// quarter of the tab's time however large the page is (a large page's
/// snapshot costs hundreds of milliseconds). A reading for an action of
/// ours, a navigation or a load never waits.
pub const READ_DUTY: u32 = 3;
/// While the screencast runs for a video, a reading the page pinged for
/// waits at least this long after the last: a player's clock and progress
/// bar change the page several times a second.
pub const READ_WHILE_PLAYING: Duration = Duration::from_secs(1);

/// How long after a reading that cost `cost` a reading the page pinged
/// for may run: [`DEBOUNCE`], [`READ_DUTY`] times the cost, and
/// [`READ_WHILE_PLAYING`] while a video plays, whichever is longest.
pub fn ping_read_gap(cost: Duration, playing: bool) -> Duration {
    let gap = DEBOUNCE.max(cost * READ_DUTY);
    if playing {
        gap.max(READ_WHILE_PLAYING)
    } else {
        gap
    }
}

/// How often the tab looks at its engine when nothing happens.
const TICK: Duration = Duration::from_millis(25);
/// At most this many stills are captured per reading.
const STILLS_PER_READ: usize = 8;
/// How often a playing video's or audio's time is read again.
const PLAYBACK: Duration = Duration::from_millis(500);
/// How long without a screencast frame before playing regions are
/// captured on their own.
const QUIET: Duration = Duration::from_millis(400);
/// How often the tab looks for the browser's next frame while one is due
/// and has not come.
const FRAME_POLL: Duration = Duration::from_millis(3);
/// The pixel view's frames are looked for this often (30 a second at
/// most; the app's byte budget drops what the terminal cannot take).
const SCREEN_EVERY: Duration = Duration::from_millis(33);
/// A live canvas on screen is captured this often while it changes, or
/// as the app's pacing asks when that is slower.
pub const CANVAS_POLL: Duration = Duration::from_millis(500);
/// A live canvas that held still for [`CANVAS_QUIET`] captures in a row
/// is captured this often, until it changes again.
pub const CANVAS_IDLE: Duration = Duration::from_secs(2);
/// How many unchanged captures in a row slow a canvas to [`CANVAS_IDLE`].
pub const CANVAS_QUIET: u32 = 10;
/// A canvas that changed at every capture this long, and is bigger than a
/// quarter of the viewport, is cut from the screencast, as a playing video
/// is, rather than captured on its own.
pub const CANVAS_BUSY: Duration = Duration::from_secs(2);
/// At most this many canvases are live at once: those the view shows,
/// nearest Chrome's view (a page of animated adverts costs no more).
pub const MAX_LIVE_CANVASES: usize = 4;

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
    /// Click the media box `node` (a picture, a canvas, a frame) of the
    /// document `document` at `x`, `y` CSS pixels from its top left
    /// corner ([`PageAction::ClickAt`]).
    ClickAt {
        document: u64,
        node: NodeId,
        x: f64,
        y: f64,
    },
    /// Scroll the box `node` of the document `document` sideways by `by`
    /// CSS pixels ([`PageAction::ScrollBox`]): a shelf paged. The page is
    /// read again, as a sideways scroll changes no DOM.
    ScrollBox {
        document: u64,
        node: NodeId,
        by: f64,
    },
    Resize(u16, u16),
    /// Read the page again now.
    Refresh,
    /// The pixel view (#818) is shown, or left: while it is, the
    /// screencast runs at full scale and every frame comes as an
    /// [`Update::Screen`] (the video regions get none, as none are drawn).
    Pixels(bool),
    /// A left click at `x`, `y` CSS pixels of the viewport
    /// ([`PageAction::ClickPoint`]): the pixel view's pointer.
    Point {
        x: f64,
        y: f64,
    },
    /// A key pressed in the page ([`PageAction::Key`]).
    Key(rich_intuituive::interact::Key),
    /// Text typed into what has the page's focus
    /// ([`PageAction::InsertText`]).
    Type(String),
    /// The media regions the view shows now, how long the app's pacing
    /// wants between two frames of each (none: [`crate::video::MAX_FPS`]),
    /// and the screencast's scale: the fraction of the page's pixels the
    /// regions are drawn in (character art needs two pixels a cell).
    Regions {
        on_screen: Vec<NodeId>,
        intervals: Vec<(NodeId, Duration)>,
        scale: f64,
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
    /// A frame of the whole viewport, for the pixel view.
    Screen(Arc<rich_embed::ScreencastFrame>),
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

/// A tab's thread, as [`stop_all`] stops it: a flag it reads, and a
/// channel that closes when the thread has ended and its engine is gone.
struct Running {
    stop: Arc<AtomicBool>,
    ended: Receiver<()>,
}

/// Every tab spawned and not yet stopped. A tab whose thread has ended is
/// pruned when the next one is spawned, so the list holds live tabs only.
static RUNNING: Mutex<Vec<Running>> = Mutex::new(Vec::new());

/// Drop the entries whose thread has ended: their channel is closed.
fn prune(running: &mut Vec<Running>) {
    running.retain(|r| !matches!(r.ended.try_recv(), Err(TryRecvError::Disconnected)));
}

/// Stop every tab's thread and wait, up to `timeout` in all, until each
/// has ended. Ending drops its engine: a browser it started stops, and its
/// temporary profile is removed. The thread does not wait for its owner to
/// let go of the [`Tab`], since an app's state may live until the process
/// exits, when no thread's `Drop` runs. Call it before the process exits,
/// on every way out.
pub fn stop_all(timeout: Duration) {
    let all = std::mem::take(&mut *RUNNING.lock().unwrap_or_else(|e| e.into_inner()));
    for running in &all {
        running.stop.store(true, Ordering::SeqCst);
    }
    let deadline = Instant::now() + timeout;
    for running in all {
        let left = deadline.saturating_duration_since(Instant::now());
        // A message never comes: the channel closes when the thread ends.
        let _ = running.ended.recv_timeout(left);
    }
}

impl Tab {
    /// Run `engine` on a thread of its own. `update` is called on that
    /// thread with each [`Update`]. The thread ends, dropping the engine,
    /// when every [`Tab`] is dropped or [`stop_all`] is called.
    pub fn spawn(engine: Box<dyn Engine>, update: impl Fn(Update) + Send + 'static) -> Tab {
        Self::spawn_with_source(engine, move |event, _| update(event))
    }

    /// Run the same worker, retaining source relationships. Every Page update
    /// carries Some whose page is the exact same Arc; other updates carry None.
    pub fn spawn_with_source(
        engine: Box<dyn Engine>,
        update: impl Fn(Update, Option<SourcePage>) + Send + 'static,
    ) -> Tab {
        let (commands, receive) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let (ended, done) = mpsc::channel::<()>();
        {
            let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
            prune(&mut running);
            running.push(Running {
                stop: stop.clone(),
                ended: done,
            });
        }
        thread::spawn(move || {
            // Dropped last, after the engine, even when the thread panics.
            let _ended = ended;
            run_with_source(
                engine,
                receive,
                &stop,
                &|event| update(event, None),
                Some(&update),
            );
        });
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
            if let (Some(base), Some(now)) = (base, &control.value)
                && base != now
            {
                return Err(Refusal::Conflict {
                    remote: now.clone(),
                });
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
    source_update: Option<&'a dyn Fn(Update, Option<SourcePage>)>,
    source: Option<SourcePage>,
    revisions: Revisions,
    page: Option<Arc<Page>>,
    state: PageState,
    /// When the first change ping since the last reading came.
    dirty: Option<Instant>,
    /// When the last reading ended and what it cost: a pinged reading
    /// waits [`ping_read_gap`] after it.
    last_read: Option<(Instant, Duration)>,
    /// The last reading was refused (a navigation swapping the page's
    /// process): no frame is cut with the old model until one succeeds.
    refused: bool,
    stills: HashMap<StillKey, Arc<[u8]>>,
    /// The pictures decoded from the stills.
    pictures: HashMap<StillKey, Picture>,
    /// Stills to decode, to the decoder thread.
    decode: Sender<Job>,
    /// Pictures decoded, from the decoder thread.
    decoded: Receiver<StillCompletion>,
    /// Bounded to live keys; transport IDs remain until stale jobs complete.
    recovery: HashMap<StillKey, StillRecovery>,
    /// Already accepted SVG pixels, with their original owner, during revisions.
    svg_accepted: HashMap<NodeId, AcceptedSvg>,
    outstanding: HashSet<u64>,
    next_attempt: u64,
    /// The small pictures in lines, decoded by source, and those asked for.
    inline: HashMap<String, Picture>,
    inline_asked: HashSet<String>,
    inline_loader: Option<crate::inline_resources::InlineResources>,
    decode_inline: ToDecode<(u64, String)>,
    decoded_inline: Receiver<((u64, String), Picture)>,
    /// Where Chrome's view was last seen: images near it are captured
    /// first.
    metrics: Option<Metrics>,
    frames: Frames,
    /// The regions the view shows, and the pacing's intervals.
    on_screen: HashSet<NodeId>,
    intervals: HashMap<NodeId, Duration>,
    /// When the last screencast frame came.
    last_frame: Option<Instant>,
    /// A frame was due and the browser had none yet: look again soon.
    starved: bool,
    /// The screencast's scale the app asks for, and the one the engine
    /// was last given.
    scale: f64,
    scale_given: f64,
    /// The part of the document the engine was last asked to screencast
    /// (`None`: the viewport), and Chrome's viewport's width in CSS pixels
    /// (from the pane's size).
    area_given: Option<Rect>,
    view_width: Option<f64>,
    /// When Chrome's view was last looked at while only an area is cast:
    /// a page that scrolls itself moves its viewport off the area.
    view_at: Option<Instant>,
    /// A paused region's time when its frame was last taken: a seek while
    /// paused takes one more.
    shown_time: HashMap<NodeId, f64>,
    /// Each live log's entries as read, by log and entry key.
    live: HashMap<NodeId, HashMap<u64, Vec<Run>>>,
    /// The scroll boxes the page appends to, as the engine last said.
    appended: HashSet<NodeId>,
    /// A new document whose layout may still move: when to read it
    /// again, when to stop looking, and its last layout's key.
    settle: Option<(Instant, Instant, u64)>,
    /// Pictures are still waiting for their stills: more were ready than
    /// one batch takes, or Chrome's view moved. The next tick takes the
    /// next batch, with no new reading.
    stills_pending: bool,
    /// The pixel view is shown: the whole viewport's frames go to the
    /// app, and whether the screencast was started for it.
    pixels: bool,
    pixels_cast: bool,
    /// When the pixel view's last frame was looked for.
    screen_at: Option<Instant>,
    /// The canvases watched as live regions, by node.
    canvases: HashMap<NodeId, Watch>,
    /// The address last opened, until a page other than Chrome's error
    /// page is read: what the address bar keeps when it fails.
    asked: Option<String>,
}

/// A canvas watched while the view shows it.
#[derive(Clone, Copy, Debug, Default)]
struct Watch {
    /// Its pixels' hash at the last capture.
    hash: Option<u64>,
    /// When it is captured next.
    due: Option<Instant>,
    /// Captures in a row that found it unchanged.
    unchanged: u32,
    /// Since when every capture found it changed.
    changing_since: Option<Instant>,
    /// It changes at every capture and is big: the screencast cuts it.
    busy: bool,
    /// It was sent frames since it last stopped.
    moving: bool,
}

/// The start of the address Chrome gives its own error page, the page it
/// shows when a navigation fails (`chrome-error://chromewebdata/`).
pub const ERROR_PAGE: &str = "chrome-error://";

/// The node of the notice the tab puts over Chrome's error page: no node
/// of the page's (the snapshot's are never negative).
pub const ERROR_NOTICE: NodeId = -1;

/// The error code on Chrome's error page (`ERR_NAME_NOT_RESOLVED`), if
/// its text has one.
fn error_code(page: &Page) -> Option<String> {
    page.blocks.iter().find_map(|block| {
        block.text().split_whitespace().find_map(|word| {
            let word = word.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_');
            let code = word.strip_prefix("ERR_")?;
            (!code.is_empty() && code.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
                .then(|| word.to_string())
        })
    })
}

/// A key of where the page's boxes are: it changes when its layout moves.
fn layout_key(page: &Page) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let rect = |r: &Rect| [r.x, r.y, r.width, r.height].map(|v| v.round() as i64);
    for block in &page.blocks {
        std::hash::Hash::hash(&(block.node, rect(&block.rect)), &mut hasher);
    }
    for media in &page.media {
        std::hash::Hash::hash(&(media.node, rect(&media.rect)), &mut hasher);
    }
    std::hash::Hash::hash(&(page.height.round() as i64), &mut hasher);
    std::hash::Hasher::finish(&hasher)
}

impl Worker<'_> {
    fn poll_events(&mut self, read_now: &mut bool, live_due: &mut bool) {
        for event in self.engine.events() {
            match event {
                EngineEvent::Changed => {
                    self.dirty.get_or_insert_with(Instant::now);
                }
                EngineEvent::Live => *live_due = true,
                EngineEvent::Navigated(_) | EngineEvent::Loaded => *read_now = true,
                EngineEvent::DownloadBegan { guid, name, url } => {
                    (self.update)(Update::Download { guid, name, url })
                }
                EngineEvent::DownloadDone { guid, path } => {
                    (self.update)(Update::DownloadDone { guid, path })
                }
            }
        }
    }

    /// Every Page leaves through this helper, with the exact immutable pair.
    fn publish(&mut self, page: Arc<Page>, source: Option<SourcePage>) -> Arc<Page> {
        let mut source = source
            .or_else(|| self.source.clone())
            .map(|s| s.republish(page.clone()));
        if let (Some(next), Some(previous)) = (&mut source, &self.source) {
            next.reuse_relations(previous);
        }
        let same_source = match (&self.source, &source) {
            (Some(old), Some(new)) => old.same_relations(new),
            (None, None) => true,
            _ => false,
        };
        let changed = self.page.as_deref() != Some(page.as_ref()) || !same_source;
        self.page = Some(page.clone());
        self.source = source.clone();
        if changed {
            if let Some(update) = self.source_update {
                update(Update::Page(page.clone()), source);
            } else {
                (self.update)(Update::Page(page.clone()));
            }
        }
        page
    }

    /// Read the page and publish it; `None` if the engine refused (a
    /// navigation swapping the page's process, say).
    fn read(&mut self) -> Option<Arc<Page>> {
        if crate::trace::on() {
            crate::trace::media(r#"{"stage":"model_snapshot_attempt"}"#);
        }
        let at = crate::trace::start();
        let snapshot = self.engine.snapshot(TabId(0)).ok()?;
        let snapshot_ms = crate::trace::ms(at);
        if crate::trace::on() {
            crate::trace::media(r#"{"stage":"model_read"}"#);
        }
        if let Ok(boxes) = self.engine.appended(TabId(0)) {
            self.appended = boxes.into_iter().collect();
        }
        let built = crate::trace::start();
        let source = self
            .source_update
            .map(|_| build_source_with(&snapshot, &mut self.revisions, &self.appended));
        let mut page = match &source {
            Some(source) => source.page().as_ref().clone(),
            None => build_with(&snapshot, &mut self.revisions, &self.appended),
        };
        let build_ms = crate::trace::ms(built);
        self.dress_error(&mut page);
        if self.page.as_ref().map(|p| p.document) != Some(page.document) {
            self.live.clear();
            self.stills.clear();
            self.pictures.clear();
            self.recovery.clear();
            self.svg_accepted.clear();
            self.inline.clear();
            self.inline_asked.clear();
            self.canvases.clear();
        }
        let live: HashMap<_, _> = page.media.iter().map(|m| (key_of(m), m.kind)).collect();
        self.recovery
            .retain(|key, state| live.get(key) == Some(&state.kind));
        self.stills.retain(|key, _| self.recovery.contains_key(key));
        self.pictures
            .retain(|key, _| self.recovery.contains_key(key));
        let drawings: HashMap<_, _> = page
            .media
            .iter()
            .filter(|m| m.kind == MediaKind::Svg)
            .map(|m| (m.node, key_of(m)))
            .collect();
        self.svg_accepted.retain(|node, accepted| {
            drawings
                .get(node)
                .is_some_and(|key| same_svg_identity(&accepted.owner.1, key))
        });
        self.follow_logs(&mut page);
        self.dress_inline(&mut page);
        for media in &mut page.media {
            let (still, picture) = self.shown(media);
            media.still = still;
            media.picture = picture;
        }
        self.probe(&mut page);
        // A new document is read again until its layout holds still: the
        // first reading can come from a layout still moving, and nothing
        // in the DOM may say when it stops.
        let now = Instant::now();
        let key = layout_key(&page);
        let new_document = self.page.as_ref().map(|p| p.document) != Some(page.document);
        self.settle = match self.settle {
            _ if new_document => Some((now + SETTLE_STEP, now + SETTLE_MAX, key)),
            // It moved: look again a step later.
            Some((_, end, last)) if last != key && now < end => Some((now + SETTLE_STEP, end, key)),
            // Read early (a ping): the step still counts from the move.
            Some((next, end, last)) if now < next && now < end => Some((next, end, last)),
            // A step with no move, but too soon after the document came.
            Some((_, end, _)) if now + SETTLE_MAX < end + SETTLE_MIN => {
                Some((now + SETTLE_STEP, end, key))
            }
            // A step with no move: settled.
            _ => None,
        };
        let page = self.publish(Arc::new(page), source);
        if crate::trace::on() {
            crate::trace::event(
                "read",
                serde_json::json!({"snapshot_ms": snapshot_ms, "build_ms": build_ms,
                    "total_ms": crate::trace::ms(at), "blocks": page.blocks.len(),
                    "media": page.media.len()}),
            );
        }
        self.dirty = None;
        self.stills_pending = true;
        Some(page)
    }

    /// The address to show for `url` when it is Chrome's error page: the
    /// one last opened, else the address Chrome's history keeps for the
    /// entry; `None` when `url` is not the error page or nothing better
    /// is known.
    fn error_address(&self, url: &str) -> Option<String> {
        if !url.starts_with(ERROR_PAGE) {
            return None;
        }
        self.asked.clone().or_else(|| {
            let kept = self.engine.state(TabId(0)).url;
            (!kept.is_empty() && !kept.starts_with(ERROR_PAGE)).then_some(kept)
        })
    }

    /// A page read from Chrome's error page keeps the address that was
    /// asked for, and gets a notice above Chrome's own text: what could
    /// not be opened, the error code, and the key that tries again. A page
    /// that is not the error page ends the asking.
    fn dress_error(&mut self, page: &mut Page) {
        if !page.url.starts_with(ERROR_PAGE) {
            if self.page.as_ref().map(|p| p.document) != Some(page.document) {
                self.asked = None;
            }
            return;
        }
        let shown = self.error_address(&page.url);
        // Chrome's history keeps the address that failed: after a
        // redirect, not the one asked for.
        let failed = Some(self.engine.state(TabId(0)).url)
            .filter(|u| !u.is_empty() && !u.starts_with(ERROR_PAGE) && Some(u) != shown.as_ref());
        let address = shown.as_deref().unwrap_or("the page");
        let mut runs = vec![
            Run {
                text: "Could not open ".into(),
                bold: true,
                ..Run::default()
            },
            Run {
                text: crate::clean::clean(address),
                bold: true,
                ..Run::default()
            },
        ];
        let mut rest = String::new();
        if let Some(failed) = failed {
            rest.push_str(&format!(
                " (it went on to {})",
                crate::clean::clean(&failed)
            ));
        }
        if let Some(code) = error_code(page) {
            rest.push_str(&format!(": {code}"));
        }
        rest.push_str(". Reload (R) tries again.");
        runs.push(Run {
            text: rest,
            ..Run::default()
        });
        // Above Chrome's own text, at its left edge.
        let left = page
            .blocks
            .iter()
            .map(|b| b.rect.x)
            .fold(f64::NAN, f64::min);
        let left = if left.is_finite() { left } else { 8.0 };
        let top = page
            .blocks
            .iter()
            .map(|b| b.rect.y)
            .fold(f64::NAN, f64::min);
        let height = 32.0;
        let y = if top.is_finite() && top >= height + 8.0 {
            8.0
        } else {
            0.0
        };
        let rect = Rect::new(left, y, (page.width - 2.0 * left).max(320.0), height);
        page.blocks.insert(
            0,
            Block {
                kind: BlockKind::Paragraph,
                runs,
                quote: 0,
                rect,
                frame: rect,
                node: ERROR_NOTICE,
                region: None,
                stack: Default::default(),
            },
        );
        if let Some(address) = shown {
            page.url = address;
        }
    }

    /// Current-key pixels, or an already accepted SVG drawing while a new
    /// content revision decodes. The latter keeps its original ownership record.
    fn shown(&self, media: &Media) -> (Option<Arc<[u8]>>, Option<Picture>) {
        let key = key_of(media);
        if let Some(picture) = self.pictures.get(&key) {
            return (self.stills.get(&key).cloned(), Some(picture.clone()));
        }
        if media.kind == MediaKind::Svg
            && let Some(accepted) = self
                .svg_accepted
                .get(&media.node)
                .filter(|accepted| same_svg_identity(&accepted.owner.1, &key))
        {
            return (Some(accepted.bytes.clone()), Some(accepted.picture.clone()));
        }
        (self.stills.get(&key).cloned(), None)
    }

    /// Read the live logs' new and changed entries into `page`. An engine
    /// that cannot read them, or a log with controls, leaves the
    /// snapshot's entries, whose controls are bound. Whether a log with
    /// controls was seen: then only a snapshot reads it right.
    fn follow_logs(&mut self, page: &mut Page) -> bool {
        let base = page.url.clone();
        let mut controls = false;
        for log in &mut page.logs {
            let Ok(mut read) = self.engine.read_live(TabId(0), log.node, false) else {
                continue;
            };
            if read.interactive {
                self.live.remove(&log.node);
                controls = true;
                continue;
            }
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
        controls
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
        if self.follow_logs(&mut next) {
            // A log with controls changed: a snapshot binds them.
            self.read();
            return;
        }
        self.dress_inline(&mut next);
        if next != *page {
            self.publish(Arc::new(next), None);
        }
    }

    /// Give `page` the small pictures of its lines that are decoded, and
    /// ask for those not yet asked for: each source's original bytes (or a
    /// `data:` address's), decoded once whatever number of lines show it.
    fn dress_inline(&mut self, page: &mut Page) {
        let mut sources: Vec<String> = Vec::new();
        let mut note = |run: &Run| {
            if let Some(picture) = run.picture.as_ref().filter(|p| !p.src.is_empty())
                && !sources.contains(&picture.src)
            {
                sources.push(picture.src.clone());
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
        sources.truncate(MAX_INLINE);
        let needed: HashSet<_> = sources.iter().cloned().collect();
        self.inline.retain(|src, _| needed.contains(src));
        self.inline_asked.retain(|src| needed.contains(src));
        if let Some(loader) = &self.inline_loader {
            loader.retain(page.document, &needed);
        }
        page.inline_pictures.clear();
        for src in sources {
            if let Some(picture) = self.inline.get(&src) {
                page.inline_pictures.insert(src, picture.clone());
                continue;
            }
            if self.inline_asked.len() >= MAX_INLINE || self.inline_asked.contains(&src) {
                continue;
            }
            if let Some(loader) = &self.inline_loader {
                if loader.submit(page.document, src.clone()) {
                    self.inline_asked.insert(src);
                }
                continue;
            }
            self.inline_asked.insert(src.clone());
            let bytes = if let Some(data) = src.strip_prefix("data:") {
                data_bytes(data)
            } else {
                self.engine.resource(TabId(0), &src).ok()
            };
            if let Some(bytes) = bytes.filter(|b| decodable(b)) {
                let _ = self
                    .decode_inline
                    .send(((page.document, src), bytes.into()));
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
            self.publish(Arc::new(next), None);
        }
    }

    /// Capture new or due sources after the current model's ownership checks.
    fn capture_stills(&mut self) {
        self.capture_stills_at(Instant::now());
    }

    fn still_retry_due(&self, now: Instant) -> bool {
        self.recovery.values().any(|state| state.due(now))
    }

    fn capture_stills_at(&mut self, now: Instant) {
        let Some(page) = self.page.clone() else {
            return;
        };
        let view = self.metrics.map_or(0.0, |m| m.scroll_y);
        let mut candidates: Vec<_> = page
            .media
            .iter()
            .filter(|m| {
                matches!(
                    m.kind,
                    MediaKind::Image | MediaKind::Video | MediaKind::Svg | MediaKind::Canvas
                ) && m.rect.width >= 4.0
                    && m.rect.height >= 4.0
                    && self
                        .recovery
                        .get(&key_of(m))
                        .is_none_or(|state| state.due(now))
            })
            .collect();
        candidates.sort_by(|a, b| (a.rect.y - view).abs().total_cmp(&(b.rect.y - view).abs()));
        self.stills_pending = false;
        let mut admitted = 0;
        for media in candidates {
            if admitted == STILLS_PER_READ || self.outstanding.len() == STILLS_PER_READ {
                self.stills_pending = true;
                break;
            }
            if !self
                .engine
                .image_ready(TabId(0), media.node)
                .unwrap_or(true)
            {
                continue;
            }
            admitted += 1;
            let key = key_of(media);
            let mut state = self
                .recovery
                .remove(&key)
                .unwrap_or_else(|| StillRecovery::new(media.kind));
            let original = !state.started || state.original_due.is_some_and(|due| due <= now);
            state.started = true;
            let bytes = if original {
                state.original_due = None;
                let src = picture_source(media);
                let original = if let Some(data) = src.strip_prefix("data:") {
                    data_bytes(data)
                } else if src.is_empty() {
                    None
                } else {
                    match self.engine.resource(TabId(0), src) {
                        Ok(bytes) => Some(bytes),
                        Err(error) => {
                            if error.kind() != std::io::ErrorKind::Unsupported {
                                state.original_due =
                                    Some(now + retry_delay(&mut state.original_failures));
                            }
                            None
                        }
                    }
                };
                match original.filter(|bytes| decodable(bytes)) {
                    Some(bytes) => Some((bytes, StillOrigin::Original)),
                    // A good screenshot need not be captured on each cache miss.
                    None if state.original_due.is_some() && self.pictures.contains_key(&key) => {
                        None
                    }
                    // Resource and screenshot failures have independent clocks.
                    // An earlier original retry cannot defeat fallback backoff.
                    None if state.screenshot_due.is_some_and(|due| due > now) => None,
                    None => self.screenshot_bytes(media.rect, &mut state, now),
                }
            } else {
                self.screenshot_bytes(media.rect, &mut state, now)
            };
            if let Some((bytes, origin)) = bytes {
                self.next_attempt += 1;
                let attempt = self.next_attempt;
                let bytes: Arc<[u8]> = bytes.into();
                let job = (page.document, key.clone(), attempt, origin);
                if self.decode.send((job, bytes.clone())).is_ok() {
                    self.outstanding.insert(attempt);
                    state.pending = Some((attempt, origin, bytes.clone()));
                    // Retain the last successful bytes/picture until replacement
                    // actually decodes. First bytes still publish asynchronously.
                    if !self.pictures.contains_key(&key) {
                        self.stills.insert(key.clone(), bytes);
                    }
                } else {
                    state.screenshot_due = Some(now + retry_delay(&mut state.screenshot_failures));
                }
            }
            self.recovery.insert(key, state);
        }
        let mut next = (*page).clone();
        for media in &mut next.media {
            let (still, picture) = self.shown(media);
            media.still = still;
            media.picture = picture;
        }
        self.publish(Arc::new(next), None);
    }

    fn screenshot_bytes(
        &mut self,
        rect: Rect,
        state: &mut StillRecovery,
        now: Instant,
    ) -> Option<(Vec<u8>, StillOrigin)> {
        state.screenshot_due = None;
        let region = Rect::new(
            rect.x,
            rect.y,
            rect.width.min(4000.0),
            rect.height.min(4000.0),
        );
        match self.engine.capture_model(TabId(0), region, Format::Png) {
            Ok(frame) => Some((frame.bytes, StillOrigin::Screenshot)),
            Err(_) => {
                state.screenshot_due = Some(now + retry_delay(&mut state.screenshot_failures));
                None
            }
        }
    }

    /// Put the pictures decoded since the last look on the page, and
    /// publish it again.
    fn take_decoded(&mut self) {
        self.take_decoded_at(Instant::now());
    }

    fn take_decoded_at(&mut self, now: Instant) {
        let mut any = false;
        let mut inline_progress = false;
        while let Ok(((document, key, attempt, origin), picture)) = self.decoded.try_recv() {
            if !self.outstanding.remove(&attempt)
                || self.page.as_ref().map(|p| p.document) != Some(document)
            {
                continue;
            }
            let Some(state) = self.recovery.get_mut(&key) else {
                continue;
            };
            if state.pending.as_ref().map(|(id, source, _)| (*id, *source))
                != Some((attempt, origin))
            {
                continue;
            }
            let (_, _, bytes) = state.pending.take().unwrap();
            if let Some(picture) = picture {
                if state.kind == MediaKind::Svg {
                    self.svg_accepted.insert(
                        key.0,
                        AcceptedSvg {
                            owner: (document, key.clone(), attempt, origin),
                            bytes: bytes.clone(),
                            picture: picture.clone(),
                        },
                    );
                }
                self.stills.insert(key.clone(), bytes);
                state.screenshot_due = None;
                if origin == StillOrigin::Original {
                    state.original_due = None;
                }
                self.pictures.insert(key, picture);
                any = true;
            } else if origin == StillOrigin::Original {
                // Header recognition did not guarantee valid pixels. Use the
                // guarded screenshot once; this original is not polled again.
                state.original_due = None;
                state.screenshot_due = Some(now);
            } else {
                state.screenshot_due = Some(now + retry_delay(&mut state.screenshot_failures));
            }
        }
        loop {
            let decoded = self
                .inline_loader
                .as_ref()
                .and_then(|loader| loader.take())
                .or_else(|| {
                    self.decoded_inline
                        .try_recv()
                        .ok()
                        .map(|(key, picture)| (key, Some(picture)))
                });
            let Some(((document, src), picture)) = decoded else {
                break;
            };
            inline_progress = true;
            if self.page.as_ref().map(|p| p.document) != Some(document)
                || !self.inline_asked.contains(&src)
            {
                continue;
            }
            let Some(picture) = picture else {
                continue;
            };
            let used: usize = self
                .inline
                .values()
                .map(crate::inline_resources::retained_bytes)
                .sum();
            if used + crate::inline_resources::retained_bytes(&picture) > 32 << 20 {
                continue;
            }
            self.inline.insert(src, picture);
            any = true;
        }
        // Refill admission after every completed attempt, including failures,
        // stale results and pictures rejected by the cache budget. dress_inline
        // retains attempted URLs; progress never makes failures retry in a loop.
        let Some(page) = self.page.clone().filter(|_| any || inline_progress) else {
            return;
        };
        let mut next = (*page).clone();
        for media in &mut next.media {
            let (still, picture) = self.shown(media);
            media.still = still;
            media.picture = picture;
        }
        self.dress_inline(&mut next);
        if next != *page {
            self.publish(Arc::new(next), None);
        }
    }

    /// Scroll the page, then say where it is. A document taller than the
    /// one read (a feed that loaded more without a ping yet) is read again
    /// soon.
    fn scroll(&mut self, action: PageAction) -> std::io::Result<()> {
        self.engine.act(TabId(0), &action)?;
        // Pictures near the new view, loaded already, are captured first.
        self.stills_pending = true;
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

    /// The video regions now, with the busy live canvases (cut from the
    /// screencast as a playing video is).
    fn regions(&self) -> Vec<Region> {
        let Some(page) = &self.page else {
            return Vec::new();
        };
        let live = self.live_canvases(page);
        page.media
            .iter()
            .filter(|m| {
                m.kind == MediaKind::Video
                    || m.kind == MediaKind::Canvas
                        && self.canvases.get(&m.node).is_some_and(|w| w.busy)
            })
            .map(|m| Region {
                node: m.node,
                rect: m.rect,
                playing: if m.kind == MediaKind::Video {
                    m.playback.is_some_and(|p| p.playing())
                } else {
                    live.iter().any(|(node, _)| *node == m.node)
                },
                on_screen: self.on_screen.contains(&m.node),
                interval: self.intervals.get(&m.node).copied(),
            })
            .collect()
    }

    /// The canvases live now: those the view shows, at most
    /// [`MAX_LIVE_CANVASES`], nearest Chrome's view first.
    fn live_canvases(&self, page: &Page) -> Vec<(NodeId, Rect)> {
        let view = self.metrics.map_or(0.0, |m| m.scroll_y);
        let mut live: Vec<(NodeId, Rect)> = page
            .media
            .iter()
            .filter(|m| m.kind == MediaKind::Canvas && self.on_screen.contains(&m.node))
            .filter(|m| m.rect.width >= 4.0 && m.rect.height >= 4.0)
            .map(|m| (m.node, m.rect))
            .collect();
        live.sort_by(|a, b| (a.1.y - view).abs().total_cmp(&(b.1.y - view).abs()));
        live.truncate(MAX_LIVE_CANVASES);
        live
    }

    /// Capture each live canvas whose time has come, and send its pixels
    /// as a frame when they changed. A canvas no longer live stops.
    fn poll_canvases(&mut self) {
        let Some(page) = self.page.clone() else {
            return;
        };
        let live = self.live_canvases(&page);
        let stopped: Vec<NodeId> = self
            .canvases
            .iter()
            .filter(|(node, _)| !live.iter().any(|(l, _)| l == *node))
            .map(|(node, _)| *node)
            .collect();
        for node in stopped {
            if let Some(watch) = self.canvases.remove(&node)
                && watch.moving
            {
                (self.update)(Update::Still(node));
            }
        }
        let now = Instant::now();
        for (node, rect) in live {
            let watch = self.canvases.entry(node).or_default();
            if watch.busy || watch.due.is_some_and(|due| now < due) {
                continue;
            }
            watch.due = Some(now + CANVAS_POLL);
            let Some(pixels) = self.capture_pixels(rect, Format::Png) else {
                continue;
            };
            if self.observe(node, rect, &pixels, now) {
                let seq = self.frames.number();
                (self.update)(Update::Frame(Arc::new(VideoFrame { node, pixels, seq })));
            }
        }
    }

    /// Note a capture of canvas `node` at `now`: whether its pixels
    /// changed since the last, and when it is captured next. One that
    /// changed at every capture for [`CANVAS_BUSY`] and is big turns busy;
    /// one that held still for [`CANVAS_QUIET`] captures slows down, and
    /// stops being busy.
    fn observe(
        &mut self,
        node: NodeId,
        rect: Rect,
        pixels: &rich_embed::Pixels,
        now: Instant,
    ) -> bool {
        let hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&(pixels.width, pixels.height, &pixels.rgb), &mut hasher);
            std::hash::Hasher::finish(&hasher)
        };
        let interval = self.intervals.get(&node).copied().unwrap_or_default();
        let watch = self.canvases.entry(node).or_default();
        let first = watch.hash.is_none();
        let changed = watch.hash != Some(hash);
        watch.hash = Some(hash);
        let mut busy = false;
        if changed && !first {
            watch.unchanged = 0;
            let since = *watch.changing_since.get_or_insert(now);
            busy = !watch.busy && now.duration_since(since) >= CANVAS_BUSY;
        } else if !changed {
            watch.unchanged = watch.unchanged.saturating_add(1);
            watch.changing_since = None;
            if watch.unchanged >= CANVAS_QUIET {
                watch.busy = false;
            }
        }
        let busy = busy && self.big(rect);
        let watch = self.canvases.entry(node).or_default();
        watch.busy |= busy;
        let every = if watch.unchanged >= CANVAS_QUIET {
            CANVAS_IDLE
        } else {
            CANVAS_POLL
        };
        watch.due = Some(now + every.max(interval));
        watch.moving |= changed;
        changed
    }

    /// Whether `rect` is bigger than a quarter of Chrome's viewport (read
    /// when it was not yet).
    fn big(&mut self, rect: Rect) -> bool {
        if self.metrics.is_none() {
            self.metrics = self.engine.metrics(TabId(0)).ok();
        }
        let width = self.page.as_ref().map_or(0.0, |p| p.width);
        let Some(height) = self.metrics.map(|m| m.viewport_height) else {
            return false;
        };
        rect.width * rect.height * 4.0 >= width * height
    }

    /// Hand a frame cut or captured for a region to the app: a canvas's
    /// only when its pixels changed.
    fn deliver(&mut self, video: VideoFrame, now: Instant) {
        let canvas = self.page.as_ref().and_then(|p| {
            p.media
                .iter()
                .find(|m| m.node == video.node && m.kind == MediaKind::Canvas)
                .map(|m| m.rect)
        });
        if let Some(rect) = canvas
            && !self.observe(video.node, rect, &video.pixels, now)
        {
            return;
        }
        if crate::trace::on() {
            crate::trace::event(
                "frame",
                serde_json::json!({"node": video.node, "seq": video.seq,
                    "w": video.pixels.width, "h": video.pixels.height}),
            );
        }
        (self.update)(Update::Frame(Arc::new(video)));
    }

    /// Show or leave the pixel view: the screencast runs for it at full
    /// scale, or stops, and the video regions' pacing starts again.
    fn set_pixels(&mut self, on: bool) {
        if on == self.pixels {
            return;
        }
        self.pixels = on;
        if on {
            // The pixel view shows the whole viewport.
            self.set_area(None);
            let _ = self.engine.set_screencast_scale(TabId(0), 1.0);
            self.pixels_cast = self.engine.set_screencast(TabId(0), true).is_ok();
            self.scale_given = 1.0;
            self.screen_at = None;
        } else {
            if self.pixels_cast {
                let _ = self.engine.set_screencast(TabId(0), false);
            }
            self.pixels_cast = false;
            // The regions' screencast is planned afresh.
            self.frames = Frames::default();
            self.last_frame = None;
        }
    }

    /// The pixel view's frame, when one is due and the browser has one.
    fn screen(&mut self) {
        let now = Instant::now();
        if self
            .screen_at
            .is_some_and(|at| now.duration_since(at) < SCREEN_EVERY)
        {
            return;
        }
        self.screen_at = Some(now);
        if let Some(frame) = self.engine.screencast_frame(TabId(0)) {
            (self.update)(Update::Screen(Arc::new(frame)));
        }
    }

    /// Run the screencast while a region plays on screen, and send each
    /// region's frames when they are due.
    fn video(&mut self) {
        let regions = self.regions();
        let now = Instant::now();
        let plan = self.frames.plan(&regions, now);
        // The scale and the area first, so the screencast starts at them.
        let casting = self.frames.on || plan.screencast == Some(true);
        if self.scale != self.scale_given && casting {
            self.scale_given = self.scale;
            let _ = self.engine.set_screencast_scale(TabId(0), self.scale);
        }
        let area = if casting && plan.screencast != Some(false) {
            crate::video::screencast_area(&regions, self.chrome_view(now))
        } else {
            None
        };
        self.set_area(area);
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
        let at = crate::trace::start();
        let frame = self.engine.screencast_frame(TabId(0));
        if frame.is_some() {
            self.last_frame = Some(now);
        }
        if let Some(f) = frame.as_ref().filter(|_| crate::trace::on()) {
            crate::trace::event(
                "cast",
                serde_json::json!({"take_ms": crate::trace::ms(at),
                    "w": f.pixels.width, "h": f.pixels.height}),
            );
        }
        // No new frame for a while: what Chrome's viewport does not hold
        // is not painted there, so it is captured on its own.
        let quiet = self
            .last_frame
            .is_none_or(|at| now.duration_since(at) >= QUIET);
        self.starved = frame.is_none() && !quiet;
        if self.starved {
            return;
        }
        let (cut, missing) = self.frames.cut(&regions, frame.as_ref(), now);
        for video in cut {
            self.shown_time.remove(&video.node);
            self.deliver(video, now);
        }
        // A fresh viewport frame can still hold only part of a region the
        // adapted view shows. Capture that whole region through the model's
        // guarded path; waiting for a quiet screencast would starve it forever.
        self.capture_regions(&missing, now);
    }

    /// Chrome's viewport in the document's CSS pixels, if known. While the
    /// screencast runs it is looked at again every [`PLAYBACK`]: a page
    /// that scrolls itself moves Chrome's viewport off the area, or onto it.
    fn chrome_view(&mut self, now: Instant) -> Option<Rect> {
        let stale = self
            .view_at
            .is_none_or(|at| now.duration_since(at) >= PLAYBACK);
        if stale {
            self.view_at = Some(now);
            if let Ok(metrics) = self.engine.metrics(TabId(0)) {
                self.metrics = Some(metrics);
            }
        }
        let m = self.metrics?;
        let width = self
            .view_width
            .or_else(|| self.page.as_ref().map(|p| p.width))?;
        Some(Rect::new(0.0, m.scroll_y, width, m.viewport_height))
    }

    /// Ask the engine to screencast `area` (`None`: the viewport) when it
    /// is not what it was last asked; a box that moved less than a pixel
    /// keeps its area, so a reading's rounding never restarts the cast.
    fn set_area(&mut self, area: Option<Rect>) {
        let same = match (area, self.area_given) {
            (Some(a), Some(b)) => {
                (a.x - b.x).abs() < 1.0
                    && (a.y - b.y).abs() < 1.0
                    && (a.width - b.width).abs() < 1.0
                    && (a.height - b.height).abs() < 1.0
            }
            (a, b) => a.is_none() && b.is_none(),
        };
        if same {
            return;
        }
        if crate::trace::on() {
            crate::trace::event(
                "area",
                serde_json::json!({"area": area.map(|r| [r.x, r.y, r.width, r.height])}),
            );
        }
        self.area_given = area;
        let _ = self.engine.set_screencast_area(TabId(0), area);
    }

    /// Whether a reading the page pinged for may run now (see
    /// [`ping_read_gap`]).
    fn ping_due(&self, now: Instant) -> bool {
        let Some(since) = self.dirty else {
            return false;
        };
        let held = self
            .last_read
            .is_some_and(|(end, cost)| now < end + ping_read_gap(cost, self.frames.on));
        now.duration_since(since) >= DEBOUNCE && !held
    }

    /// Whether video frames may be cut and captured now: with a model of
    /// this document (none asked for or refused). A change the page pinged
    /// waits its turn ([`ping_read_gap`]) without stopping the frames: a
    /// player's own clock pings several times a second, and the frames
    /// would stop with it. The engine still checks each frame's document
    /// and viewport.
    fn video_may_run(&self, read_now: bool) -> bool {
        !read_now && !self.refused && self.page.is_some() && !self.pixels
    }

    /// How long the tab may wait for a command before it looks at its
    /// engine again: until the next video frame is due (so frames are
    /// taken on their slots, not on the tick after), a moment while a due
    /// frame has not come, else the tick.
    fn wait(&self) -> Duration {
        if self.pixels {
            return SCREEN_EVERY.min(TICK);
        }
        if !self.frames.on {
            return TICK;
        }
        if self.starved {
            return FRAME_POLL;
        }
        self.frames
            .next_due(&self.regions(), Instant::now())
            .map_or(TICK, |due| due.min(TICK))
    }

    /// Frames of regions taken with a capture of each.
    fn capture_regions(&mut self, regions: &[Region], now: Instant) {
        for region in regions {
            let at = crate::trace::start();
            let pixels = self.capture_pixels(region.rect, Format::Jpeg);
            if crate::trace::on() {
                crate::trace::event(
                    "capture",
                    serde_json::json!({"node": region.node, "ms": crate::trace::ms(at),
                        "ok": pixels.is_some()}),
                );
            }
            if let Some(pixels) = pixels {
                let video = self.frames.stamp(region.node, pixels, now);
                self.deliver(video, now);
            }
        }
    }

    /// Chrome's pixels of `rect`, as RGB: JPEG for a video's frame, PNG
    /// for a canvas's (its edges and text kept, and its bytes the same
    /// while its pixels are).
    fn capture_pixels(&mut self, rect: Rect, format: Format) -> Option<rich_embed::Pixels> {
        let region = Rect::new(
            rect.x,
            rect.y,
            rect.width.min(4000.0),
            rect.height.min(4000.0),
        );
        let frame = self.engine.capture_model(TabId(0), region, format).ok()?;
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
            Command::Open(url) => {
                self.asked = Some(url.clone());
                self.engine.open(TabId(0), &url).map_err(failed)
            }
            Command::History(step) => {
                // Back and forward go to entries Chrome keeps the address
                // of; a reload tries the same address again.
                if step != HistoryStep::Reload {
                    self.asked = None;
                }
                self.engine.history(TabId(0), step).map_err(failed)
            }
            Command::Resize(columns, rows) => {
                self.view_width = Some(f64::from(columns) * f64::from(crate::cdp::CELL.0));
                self.engine.resize(columns, rows).map_err(failed)
            }
            Command::Scroll(y) => self.scroll(PageAction::ScrollTo(y)).map_err(failed),
            Command::Wheel(delta) => self.scroll(PageAction::Wheel(delta)).map_err(failed),
            Command::LoadMore => self.scroll(PageAction::ScrollTo(f64::MAX)).map_err(failed),
            Command::Refresh => {
                self.read();
                Ok(())
            }
            Command::ScrollBox { document, node, by } => {
                let page = self
                    .read()
                    .ok_or_else(|| Refusal::Stale("The page is not ready.".into()))?;
                if page.document != document {
                    return Err(Refusal::Stale(
                        "Stale document: the tab went to another page.".into(),
                    ));
                }
                if !page.shelves.iter().any(|s| s.node == node) {
                    return Err(Refusal::Stale("That shelf is gone from the page.".into()));
                }
                self.engine
                    .act(TabId(0), &PageAction::ScrollBox { node, by })
                    .map_err(failed)?;
                // Its items moved, with no change to the DOM: read it
                // again, and capture the pictures that came into view.
                self.stills_pending = true;
                self.read();
                Ok(())
            }
            Command::Pixels(on) => {
                self.set_pixels(on);
                Ok(())
            }
            Command::Point { x, y } => self
                .engine
                .act(TabId(0), &PageAction::ClickPoint { x, y })
                .map_err(failed),
            Command::Key(key) => self
                .engine
                .act(TabId(0), &PageAction::Key(key))
                .map_err(failed),
            Command::Type(text) => self
                .engine
                .act(TabId(0), &PageAction::InsertText(text))
                .map_err(failed),
            Command::Regions {
                on_screen,
                intervals,
                scale,
            } => {
                self.on_screen = on_screen.into_iter().collect();
                self.intervals = intervals.into_iter().collect();
                self.scale = scale;
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
            Command::ClickAt {
                document,
                node,
                x,
                y,
            } => {
                let page = self
                    .read()
                    .ok_or_else(|| Refusal::Stale("The page is not ready.".into()))?;
                if page.document != document {
                    return Err(Refusal::Stale(
                        "Stale document: the tab went to another page.".into(),
                    ));
                }
                if !page.media.iter().any(|m| m.node == node) {
                    return Err(Refusal::Stale("That picture is gone from the page.".into()));
                }
                self.engine
                    .act(TabId(0), &PageAction::ClickAt { node, x, y })
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
const MAX_INLINE: usize = 512;

/// What a still is kept by: the node, its address, its size and its
/// content's revision (a drawing's).
type StillKey = (NodeId, String, u64, u64, u64);
/// Bytes to decode, by what asks for them.
type ToDecode<K> = Sender<(K, Arc<[u8]>)>;
/// A still for the decoder thread.
type StillAttempt = (u64, StillKey, u64, StillOrigin);
type Job = (StillAttempt, Arc<[u8]>);
type StillCompletion = (StillAttempt, Option<Picture>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StillOrigin {
    Original,
    Screenshot,
}

struct AcceptedSvg {
    owner: StillAttempt,
    bytes: Arc<[u8]>,
    picture: Picture,
}

fn same_svg_identity(a: &StillKey, b: &StillKey) -> bool {
    (a.0, &a.1, a.2, a.3) == (b.0, &b.1, b.2, b.3)
}

struct StillRecovery {
    kind: MediaKind,
    started: bool,
    pending: Option<(u64, StillOrigin, Arc<[u8]>)>,
    original_due: Option<Instant>,
    screenshot_due: Option<Instant>,
    original_failures: u8,
    screenshot_failures: u8,
}

impl StillRecovery {
    fn new(kind: MediaKind) -> Self {
        Self {
            kind,
            started: false,
            pending: None,
            original_due: None,
            screenshot_due: None,
            original_failures: 0,
            screenshot_failures: 0,
        }
    }

    fn due(&self, now: Instant) -> bool {
        self.pending.is_none()
            && (self.original_due.is_some_and(|due| due <= now)
                || self.screenshot_due.is_some_and(|due| due <= now))
    }
}

fn retry_delay(failures: &mut u8) -> Duration {
    let delay = [
        Duration::from_millis(500),
        Duration::from_secs(2),
        Duration::from_secs(8),
        Duration::from_secs(30),
    ][usize::from((*failures).min(3))];
    *failures = failures.saturating_add(1);
    delay
}

/// Whether rs-rich-art can decode `bytes`: a format it recognises *and*
/// reads (PNG, JPEG, GIF, WebP). An SVG or an AVIF (which Chrome asks for,
/// and sites such as The Guardian serve it) gets a still of Chrome's pixels
/// instead; taken as decodable, it failed on the decoder thread and the
/// picture never came.
fn decodable(bytes: &[u8]) -> bool {
    // `image` decodes AVIF with `avif-native` (the `avif` feature), but its
    // `reading_enabled` answers for its encoder's feature: with the feature,
    // an AVIF file is decodable when its decoder opens it (a file it cannot
    // read still falls back to Chrome's pixels).
    image::guess_format(bytes).is_ok_and(|format| {
        format.reading_enabled()
            || cfg!(feature = "avif")
                && format == image::ImageFormat::Avif
                && image::ImageReader::with_format(std::io::Cursor::new(bytes), format)
                    .into_dimensions()
                    .is_ok()
    })
}

/// The bytes of a `data:` address's body (after `data:`), when it is
/// base64; at most 16 MiB.
pub(crate) fn data_bytes(data: &str) -> Option<Vec<u8>> {
    let (head, body) = data.split_once(',')?;
    if !head.ends_with(";base64") || body.len() > 22 << 20 {
        return None;
    }
    crate::cdp::base64_decode(body)
}

/// Unlike the legacy inline channel, every still job releases admission.
fn still_decoder() -> (Sender<Job>, Receiver<StillCompletion>) {
    let (send, jobs) = mpsc::channel::<Job>();
    let (done, decoded) = mpsc::channel();
    thread::spawn(move || {
        while let Ok((attempt, bytes)) = jobs.recv() {
            let picture = Picture::decode_animated(&bytes).ok();
            if done.send((attempt, picture)).is_err() {
                return;
            }
        }
    });
    (send, decoded)
}

/// Where a media region's picture comes from: a video's poster when it has
/// one (else what Chrome paints in its region), an image's source.
fn picture_source(media: &Media) -> &str {
    if media.kind == MediaKind::Video {
        &media.poster
    } else {
        &media.src
    }
}

/// The key of `media`'s still as it is now.
fn key_of(media: &Media) -> StillKey {
    (
        media.node,
        picture_source(media).to_string(),
        media.rect.width.round() as u64,
        media.rect.height.round() as u64,
        media.revision,
    )
}

#[cfg(test)]
fn run(
    engine: Box<dyn Engine>,
    commands: Receiver<(u64, Command)>,
    stop: &AtomicBool,
    update: &dyn Fn(Update),
) {
    run_with_source(engine, commands, stop, update, None);
}

fn run_with_source(
    mut engine: Box<dyn Engine>,
    commands: Receiver<(u64, Command)>,
    stop: &AtomicBool,
    update: &dyn Fn(Update),
    source_update: Option<&dyn Fn(Update, Option<SourcePage>)>,
) {
    let (decode, decoded) = still_decoder();
    // The optional worker owns actual inline decoding. These channels keep
    // the controlled completion seam used by the document-lifecycle tests.
    let (decode_inline, _unused_inline_jobs) = mpsc::channel();
    let (_unused_inline_done, decoded_inline) = mpsc::channel();
    let inline_loader =
        crate::inline_resources::InlineResources::spawn(engine.optional_resources());
    let mut worker = Worker {
        engine,
        update,
        source_update,
        source: None,
        revisions: Revisions::new(),
        page: None,
        state: PageState::default(),
        dirty: None,
        last_read: None,
        refused: false,
        stills: HashMap::new(),
        pictures: HashMap::new(),
        decode,
        decoded,
        recovery: HashMap::new(),
        svg_accepted: HashMap::new(),
        outstanding: HashSet::new(),
        next_attempt: 0,
        inline: HashMap::new(),
        inline_asked: HashSet::new(),
        inline_loader: Some(inline_loader),
        decode_inline,
        decoded_inline,
        metrics: None,
        frames: Frames::default(),
        on_screen: HashSet::new(),
        intervals: HashMap::new(),
        last_frame: None,
        starved: false,
        scale: 1.0,
        scale_given: 1.0,
        area_given: None,
        view_width: None,
        view_at: None,
        shown_time: HashMap::new(),
        live: HashMap::new(),
        appended: HashSet::new(),
        settle: None,
        stills_pending: false,
        pixels: false,
        pixels_cast: false,
        screen_at: None,
        canvases: HashMap::new(),
        asked: None,
    };
    let mut live_due = false;
    // Read once at the start, then whenever something says to.
    let mut read_now = true;
    let mut probed = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match commands.recv_timeout(worker.wait()) {
            Ok((id, command)) => {
                let acts = matches!(
                    command,
                    Command::Act { .. }
                        | Command::Activate { .. }
                        | Command::Scroll(_)
                        | Command::Media { .. }
                        | Command::ClickAt { .. }
                        | Command::Point { .. }
                        | Command::Key(_)
                        | Command::Type(_)
                        | Command::Resize(_, _)
                );
                let result = worker.run_command(command);
                update(Update::Done(id, result));
                // An action of ours reads again at once.
                read_now |= acts;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        worker.poll_events(&mut read_now, &mut live_due);
        let mut state = worker.engine.state(TabId(0));
        if let Some(address) = worker.error_address(&state.url) {
            state.url = address;
        }
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
        let due = worker.ping_due(Instant::now())
            || worker
                .settle
                .is_some_and(|(next, _, _)| Instant::now() >= next);
        if read_now || due {
            if crate::trace::on() {
                let why = if read_now {
                    "asked"
                } else if worker.dirty.is_some() {
                    "changed"
                } else {
                    "settling"
                };
                crate::trace::event("read_why", serde_json::json!({ "why": why }));
            }
            // A refused reading stays due, and is tried at the next tick.
            let began = Instant::now();
            let read = worker.read();
            worker.last_read = Some((Instant::now(), began.elapsed()));
            worker.refused = read.is_none();
            if read.is_some() {
                read_now = false;
                live_due = false;
            } else {
                worker.dirty.get_or_insert_with(Instant::now);
            }
        } else if live_due && worker.dirty.is_none() {
            // The isolated world debounced it already: read the logs now.
            live_due = false;
            worker.read_live();
        }
        // Events can arrive during the synchronous model read too. Backends
        // also guard the final frame take atomically against their model scope.
        worker.poll_events(&mut read_now, &mut live_due);
        // A canvas can keep changing without a useful quiet period (for
        // example, a speed-test dial also updates its HTML readout every
        // animation frame). Keep polling canvases against the last published
        // layout while it is dirty; capture_model checks document and
        // viewport ownership atomically. Do not do this after a navigation
        // read was refused or is still due.
        if !read_now && worker.page.is_some() && !worker.pixels {
            worker.poll_canvases();
        }
        // Never capture old-model video or still media while a known
        // invalidation or refused snapshot waits.
        if worker.video_may_run(read_now) {
            worker.video();
        }
        // Stills wait for a model that no known change outdates.
        if !read_now
            && worker.dirty.is_none()
            && worker.page.is_some()
            && (worker.stills_pending || worker.still_retry_due(Instant::now()))
        {
            worker.capture_stills();
        }
        // A sole fresh frame belongs to the model published for its document.
        // Keep it in the engine while an initial/navigation snapshot is refused.
        if worker.pixels && !read_now && worker.page.is_some() {
            worker.screen();
        }
        // Legacy engines cannot lend a concurrent reader. Preserve their
        // existing resource behavior, but only after text is published and
        // with one bounded fetch per turn. Chrome uses its independent CDP
        // reader and never takes this path.
        if let Some(((document, src), reply)) = worker
            .inline_loader
            .as_ref()
            .and_then(|l| l.take_resource_request())
        {
            let current = worker.page.as_ref().is_some_and(|p| p.document == document)
                && worker.inline_asked.contains(&src);
            let bytes = current
                .then(|| worker.engine.resource(TabId(0), &src).ok())
                .flatten();
            let _ = reply.send(bytes);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::FakeEngine;

    fn recovery_worker<'a>(
        engine: FakeEngine,
        update: &'a dyn Fn(Update),
    ) -> (Worker<'a>, Receiver<Job>, Sender<StillCompletion>) {
        let engine: Box<dyn Engine> = Box::new(engine);
        let (decode, jobs) = mpsc::channel();
        let (done, decoded) = mpsc::channel();
        let (decode_inline, _inline_jobs) = mpsc::channel();
        let (_inline_done, decoded_inline) = mpsc::channel();
        let worker = Worker {
            engine,
            update,
            source_update: None,
            source: None,
            revisions: Revisions::new(),
            page: None,
            state: PageState::default(),
            dirty: None,
            last_read: None,
            refused: false,
            stills: HashMap::new(),
            pictures: HashMap::new(),
            decode,
            decoded,
            recovery: HashMap::new(),
            svg_accepted: HashMap::new(),
            outstanding: HashSet::new(),
            next_attempt: 0,
            inline: HashMap::new(),
            inline_asked: HashSet::new(),
            inline_loader: None,
            decode_inline,
            decoded_inline,
            metrics: None,
            frames: Frames::default(),
            on_screen: HashSet::new(),
            intervals: HashMap::new(),
            last_frame: None,
            starved: false,
            scale: 1.0,
            scale_given: 1.0,
            area_given: None,
            view_width: None,
            view_at: None,
            shown_time: HashMap::new(),
            live: HashMap::new(),
            appended: HashSet::new(),
            settle: None,
            stills_pending: false,
            pixels: false,
            pixels_cast: false,
            screen_at: None,
            canvases: HashMap::new(),
            asked: None,
        };
        (worker, jobs, done)
    }

    fn recovery_engine(count: usize) -> FakeEngine {
        use crate::testing::PageBuilder;
        let engine = FakeEngine::from_dir(std::path::Path::new("tests/fixtures")).unwrap();
        let mut builder =
            PageBuilder::new("https://example.test/recovery", "recovery", 400.0, 800.0);
        for n in 0..count {
            builder.image(
                builder.body(),
                &format!("https://example.test/{n}.png"),
                "image",
                Rect::new(0.0, n as f64 * 20.0, 16.0, 16.0),
            );
        }
        {
            let mut state = engine.state.lock().unwrap();
            state.pages.insert("recovery".into(), builder.snapshot());
            state.current = Some("recovery".into());
        }
        engine
    }

    fn recovery_png() -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn recovery_animation() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 255, 0, 255]] {
                encoder
                    .encode_frame(image::Frame::from_parts(
                        image::RgbaImage::from_pixel(2, 2, image::Rgba(color)),
                        0,
                        0,
                        image::Delay::from_numer_denom_ms(100, 1),
                    ))
                    .unwrap();
            }
        }
        bytes
    }

    fn complete_recovery(jobs: &Receiver<Job>, done: &Sender<StillCompletion>) -> StillAttempt {
        let (id, bytes) = jobs.try_recv().unwrap();
        done.send((id.clone(), Picture::decode_animated(&bytes).ok()))
            .unwrap();
        id
    }

    fn assert_image_only(before: &Page, after: &Page) {
        assert_eq!(layout_key(before), layout_key(after));
        let clear = |page: &Page| {
            let mut page = page.clone();
            for media in &mut page.media {
                media.still = None;
                media.picture = None;
            }
            page
        };
        assert_eq!(
            clear(before),
            clear(after),
            "pixels must not alter geometry, actions or document"
        );
    }

    #[test]
    fn recovery_corrupt_accepted_png_has_decoder_completion() {
        let bytes: Arc<[u8]> = recovery_png()[..41].into();
        assert!(decodable(&bytes));
        assert!(Picture::decode_animated(&bytes).is_err());
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        state
            .lock()
            .unwrap()
            .resources
            .insert("https://example.test/0.png".into(), bytes.to_vec());
        let update = |_: Update| {};
        let (mut worker, _jobs, done) = recovery_worker(engine, &update);
        let (decode, actual) = still_decoder();
        worker.decode = decode;
        let before = worker.read().unwrap();
        let now = Instant::now();
        worker.capture_stills_at(now);
        let completion = actual.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(completion.0.3, StillOrigin::Original);
        assert!(
            completion.1.is_none(),
            "actual decoder must complete rejected pixels too"
        );
        done.send(completion).unwrap();
        worker.take_decoded_at(now);
        assert!(worker.outstanding.is_empty());
        assert!(state.lock().unwrap().captures.is_empty());
        worker.capture_stills_at(now);
        let completion = actual.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(completion.0.3, StillOrigin::Screenshot);
        assert!(completion.1.is_some());
        done.send(completion).unwrap();
        worker.take_decoded_at(now);
        assert!(worker.page.as_ref().unwrap().media[0].picture.is_some());
        assert_image_only(&before, worker.page.as_ref().unwrap());
        worker.capture_stills_at(now + Duration::from_secs(1000));
        let state = state.lock().unwrap();
        assert_eq!(
            state.resource_reads.len(),
            1,
            "corrupt originals do not poll"
        );
        assert_eq!(state.captures.len(), 1);
    }

    #[test]
    fn recovery_transient_original_upgrades_same_key_animation() {
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        state.lock().unwrap().resource_steps.insert(
            "https://example.test/0.png".into(),
            [None, None, None, None, None, Some(recovery_animation())].into(),
        );
        let update = |_: Update| {};
        let pairs = std::cell::RefCell::new(Vec::new());
        let paired = |update: Update, source: Option<SourcePage>| {
            if let Update::Page(page) = update {
                let source = source.unwrap();
                assert!(Arc::ptr_eq(&page, source.page()));
                pairs.borrow_mut().push(source);
            }
        };
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.source_update = Some(&paired);
        let before = worker.read().unwrap();
        let mut now = Instant::now();
        worker.capture_stills_at(now);
        complete_recovery(&jobs, &done);
        worker.take_decoded_at(now);
        let good = worker.page.clone().unwrap();
        assert!(good.media[0].picture.is_some());
        for (n, millis) in [500, 2000, 8000, 30000, 30000].into_iter().enumerate() {
            let due = now + Duration::from_millis(millis);
            assert_eq!(
                worker.recovery.values().next().unwrap().original_due,
                Some(due)
            );
            // Repeated snapshot reads and scroll-triggered batches never reset due.
            worker.read().unwrap();
            worker.capture_stills_at(due - Duration::from_nanos(1));
            assert_eq!(state.lock().unwrap().resource_reads.len(), n + 1);
            assert!(jobs.try_recv().is_err());
            assert!(!worker.still_retry_due(due - Duration::from_nanos(1)));
            assert!(worker.still_retry_due(due));
            worker.capture_stills_at(due);
            assert_eq!(state.lock().unwrap().resource_reads.len(), n + 2);
            assert_eq!(
                worker.page.as_ref().unwrap().media[0].picture,
                good.media[0].picture
            );
            assert_eq!(
                worker.page.as_ref().unwrap().media[0].still,
                good.media[0].still
            );
            now = due;
        }
        assert_eq!(
            state.lock().unwrap().captures.len(),
            1,
            "good fallback stays visible"
        );
        // A due original that is decoding owns the key; a later tick cannot duplicate it.
        worker.capture_stills_at(now + Duration::from_secs(1000));
        assert_eq!(state.lock().unwrap().resource_reads.len(), 6);
        let id = complete_recovery(&jobs, &done);
        assert_eq!(id.3, StillOrigin::Original);
        worker.take_decoded_at(now);
        let after = worker.page.as_ref().unwrap();
        assert_eq!(
            after.media[0]
                .picture
                .as_ref()
                .unwrap()
                .frames()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            after.media[0].still.as_deref(),
            Some(recovery_animation().as_slice())
        );
        assert_image_only(&before, after);
        for pair in pairs.borrow().iter() {
            assert!(Arc::ptr_eq(&pairs.borrow()[0].relations, &pair.relations));
        }
        worker.capture_stills_at(now + Duration::from_secs(2000));
        assert!(jobs.try_recv().is_err());
        assert!(!worker.still_retry_due(now + Duration::from_secs(2000)));
        assert_eq!(
            state.lock().unwrap().resource_reads.len(),
            6,
            "successful original is final"
        );
    }

    #[test]
    fn recovery_failed_screenshot_capture_and_decode_back_off_without_original_polling() {
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        {
            let mut state = state.lock().unwrap();
            state
                .resources
                .insert("https://example.test/0.png".into(), b"<svg/>".to_vec());
            state.capture_steps = [
                None,
                Some(recovery_png()[..41].to_vec()),
                None,
                Some(recovery_png()),
            ]
            .into();
        }
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let mut now = Instant::now();
        worker.capture_stills_at(now);
        assert!(jobs.try_recv().is_err());
        for (n, millis) in [500, 2000, 8000].into_iter().enumerate() {
            let due = now + Duration::from_millis(millis);
            assert_eq!(
                worker.recovery.values().next().unwrap().screenshot_due,
                Some(due)
            );
            worker.read().unwrap();
            worker.capture_stills_at(due - Duration::from_nanos(1));
            assert_eq!(state.lock().unwrap().captures.len(), n + 1);
            worker.capture_stills_at(due);
            assert_eq!(state.lock().unwrap().captures.len(), n + 2);
            if n != 1 {
                complete_recovery(&jobs, &done);
                worker.take_decoded_at(due);
            }
            now = due;
        }
        assert!(worker.page.as_ref().unwrap().media[0].picture.is_some());
        worker.capture_stills_at(now + Duration::from_secs(1000));
        assert_eq!(state.lock().unwrap().resource_reads.len(), 1);
        assert_eq!(state.lock().unwrap().captures.len(), 4);
    }

    #[test]
    fn recovery_batch_failure_and_not_ready_sources_do_not_starve_later_media() {
        let engine = recovery_engine(20);
        let state = engine.state.clone();
        {
            let mut state = state.lock().unwrap();
            state.capture_steps = (0..8).map(|_| None).collect();
            state
                .resources
                .insert("https://example.test/16.png".into(), recovery_animation());
        }
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let now = Instant::now();
        let first = worker.page.as_ref().unwrap().media[0].node;
        state.lock().unwrap().not_loaded.insert(first);
        worker.capture_stills_at(now);
        assert_eq!(state.lock().unwrap().resource_reads.len(), 8);
        assert!(
            !state
                .lock()
                .unwrap()
                .resource_reads
                .iter()
                .any(|src| src.ends_with("/0.png"))
        );
        assert!(jobs.try_recv().is_err());
        assert!(worker.stills_pending);
        worker.capture_stills_at(now);
        assert_eq!(worker.outstanding.len(), 8);
        assert_eq!(state.lock().unwrap().resource_reads.len(), 16);
        worker.capture_stills_at(now);
        assert_eq!(
            state.lock().unwrap().resource_reads.len(),
            16,
            "full transport admits no work"
        );
        for _ in 0..8 {
            complete_recovery(&jobs, &done);
        }
        worker.take_decoded_at(now);
        worker.capture_stills_at(now);
        assert_eq!(state.lock().unwrap().resource_reads.len(), 19);
        for _ in 0..3 {
            complete_recovery(&jobs, &done);
        }
        worker.take_decoded_at(now);
        let last = worker.page.as_ref().unwrap().media.last().unwrap();
        assert!(last.picture.is_some(), "later ready source was reached");
        assert_eq!(worker.recovery.len(), 19);
        state.lock().unwrap().not_loaded.remove(&first);
        worker.capture_stills_at(now);
        complete_recovery(&jobs, &done);
        worker.take_decoded_at(now);
        assert_eq!(state.lock().unwrap().resource_reads.len(), 20);
        worker.capture_stills_at(now);
        assert!(jobs.try_recv().is_err());
        assert!(!worker.stills_pending, "nothing is immediately runnable");
        assert!(worker.outstanding.is_empty());
    }

    #[test]
    fn recovery_original_retry_honors_later_failed_screenshot_due() {
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        state
            .lock()
            .unwrap()
            .capture_steps
            .push_back(Some(recovery_png()[..41].to_vec()));
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let now = Instant::now();
        worker.capture_stills_at(now);
        complete_recovery(&jobs, &done);
        worker.take_decoded_at(now + Duration::from_millis(100));
        worker.capture_stills_at(now + Duration::from_millis(500));
        assert_eq!(state.lock().unwrap().resource_reads.len(), 2);
        assert_eq!(
            state.lock().unwrap().captures.len(),
            1,
            "original retry cannot pull a failed fallback forward"
        );
        assert!(jobs.try_recv().is_err());
        worker.capture_stills_at(now + Duration::from_millis(599));
        assert_eq!(state.lock().unwrap().captures.len(), 1);
        worker.capture_stills_at(now + Duration::from_millis(600));
        assert_eq!(state.lock().unwrap().captures.len(), 2);
        assert_eq!(complete_recovery(&jobs, &done).3, StillOrigin::Screenshot);
        worker.take_decoded_at(now + Duration::from_millis(600));
    }

    #[test]
    fn recovery_unsupported_data_and_drawing_sources_keep_stable_fallbacks() {
        use crate::testing::PageBuilder;
        let engine = recovery_engine(0);
        let state = engine.state.clone();
        let mut builder = PageBuilder::new("https://example.test/recovery", "test", 400.0, 800.0);
        builder.image(
            builder.body(),
            "data:image/svg+xml;base64,PHN2Zy8+",
            "data",
            Rect::new(0.0, 0.0, 16.0, 16.0),
        );
        builder.image(
            builder.body(),
            "https://example.test/unsupported",
            "unsupported",
            Rect::new(0.0, 20.0, 16.0, 16.0),
        );
        builder.element(
            builder.body(),
            "SVG",
            &[],
            &[],
            Some(Rect::new(0.0, 40.0, 16.0, 16.0)),
        );
        {
            let mut state = state.lock().unwrap();
            state.pages.insert("recovery".into(), builder.snapshot());
            state.resources.insert(
                "https://example.test/unsupported".into(),
                b"<svg/>".to_vec(),
            );
        }
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let now = Instant::now();
        worker.capture_stills_at(now);
        for _ in 0..3 {
            assert_eq!(complete_recovery(&jobs, &done).3, StillOrigin::Screenshot);
        }
        worker.take_decoded_at(now);
        for millis in [0, 500, 2500, 10500, 40500, 70500] {
            worker.read().unwrap();
            worker.capture_stills_at(now + Duration::from_millis(millis));
            assert!(jobs.try_recv().is_err());
        }
        assert!(
            worker
                .page
                .as_ref()
                .unwrap()
                .media
                .iter()
                .all(|m| m.picture.is_some())
        );
        assert_eq!(state.lock().unwrap().resource_reads.len(), 1);
        assert_eq!(state.lock().unwrap().captures.len(), 3);
    }

    #[test]
    fn recovery_failed_replacement_keeps_same_key_good_pixels() {
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        state.lock().unwrap().resource_steps.insert(
            "https://example.test/0.png".into(),
            [None, Some(recovery_png()[..41].to_vec())].into(),
        );
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let now = Instant::now();
        worker.capture_stills_at(now);
        complete_recovery(&jobs, &done);
        worker.take_decoded_at(now);
        let good = worker.page.clone().unwrap();
        let due = now + Duration::from_millis(500);
        worker.capture_stills_at(due);
        complete_recovery(&jobs, &done); // actual decode rejects corrupt replacement
        worker.take_decoded_at(due);
        assert_eq!(worker.page.as_ref(), Some(&good));
        state.lock().unwrap().capture_steps.push_back(None);
        worker.capture_stills_at(due); // guarded fallback itself fails
        assert_eq!(worker.page.as_ref(), Some(&good));
        worker.read().unwrap();
        assert_eq!(worker.page.as_ref(), Some(&good));
        worker.capture_stills_at(due + Duration::from_millis(499));
        assert!(jobs.try_recv().is_err());
        worker.capture_stills_at(due + Duration::from_millis(500));
        complete_recovery(&jobs, &done);
        worker.take_decoded_at(due + Duration::from_millis(500));
        assert_image_only(&good, worker.page.as_ref().unwrap());
        assert_eq!(state.lock().unwrap().resource_reads.len(), 2);
    }

    fn recovery_svg_snapshot(change: &str, fill: &str) -> crate::engine::PageSnapshot {
        use crate::testing::PageBuilder;
        let url = if change == "document" {
            "https://other.test/page"
        } else {
            "https://example.test/recovery"
        };
        let mut builder = PageBuilder::new(url, "drawing", 400.0, 800.0);
        if change == "node" {
            builder.element(builder.body(), "BR", &[], &[], None);
        }
        if change != "removed" {
            let src = if change == "source" {
                "https://example.test/changed.png"
            } else {
                "https://example.test/drawing.png"
            };
            let kind = if change == "kind" { "IMG" } else { "SVG" };
            builder.element(
                builder.body(),
                kind,
                &[("src", src), ("fill", fill)],
                &[],
                Some(Rect::new(
                    0.0,
                    0.0,
                    if change == "width" { 20.0 } else { 16.0 },
                    if change == "height" { 20.0 } else { 16.0 },
                )),
            );
        }
        builder.snapshot()
    }

    #[test]
    fn recovery_accepted_svg_continuity_is_pruned_on_every_identity_change() {
        for change in [
            "revision", "source", "width", "height", "kind", "node", "document", "removed",
        ] {
            let engine = recovery_engine(0);
            let state = engine.state.clone();
            {
                let mut state = state.lock().unwrap();
                state
                    .pages
                    .insert("recovery".into(), recovery_svg_snapshot("", "red"));
                state
                    .resources
                    .insert("https://example.test/drawing.png".into(), recovery_png());
            }
            let update = |_: Update| {};
            let (mut worker, jobs, done) = recovery_worker(engine, &update);
            worker.read().unwrap();
            let now = Instant::now();
            worker.capture_stills_at(now);
            let owner = complete_recovery(&jobs, &done);
            worker.take_decoded_at(now);
            let good = worker.page.clone().unwrap();
            assert!(good.media[0].picture.is_some());
            assert_eq!(worker.svg_accepted[&owner.1.0].owner, owner);
            state
                .lock()
                .unwrap()
                .pages
                .insert("recovery".into(), recovery_svg_snapshot(change, "blue"));
            let next = worker.read().unwrap();
            if change == "revision" {
                assert_eq!(next.media[0].picture, good.media[0].picture);
                assert_eq!(next.media[0].still, good.media[0].still);
                assert_eq!(
                    worker.svg_accepted[&owner.1.0].owner, owner,
                    "display continuity keeps original provenance, not new-key success"
                );
            } else {
                assert!(
                    next.media
                        .iter()
                        .all(|m| m.picture.is_none() && m.still.is_none()),
                    "{change}"
                );
                assert!(worker.svg_accepted.is_empty(), "{change}");
            }
            assert!(worker.pictures.is_empty());
            assert!(worker.recovery.is_empty());
        }
    }

    #[test]
    fn recovery_svg_late_revision_cannot_change_accepted_fallback_or_current_pending() {
        let engine = recovery_engine(0);
        let state = engine.state.clone();
        {
            let mut state = state.lock().unwrap();
            state
                .pages
                .insert("recovery".into(), recovery_svg_snapshot("", "red"));
            state
                .resources
                .insert("https://example.test/drawing.png".into(), recovery_png());
        }
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        worker.read().unwrap();
        let now = Instant::now();
        worker.capture_stills_at(now);
        let first = complete_recovery(&jobs, &done);
        worker.take_decoded_at(now);
        let good = worker.page.clone().unwrap();
        state.lock().unwrap().pages.insert(
            "recovery".into(),
            recovery_svg_snapshot("revision", "green"),
        );
        worker.read().unwrap();
        worker.capture_stills_at(now);
        let (late, bytes) = jobs.try_recv().unwrap();
        state
            .lock()
            .unwrap()
            .pages
            .insert("recovery".into(), recovery_svg_snapshot("revision", "blue"));
        worker.read().unwrap();
        worker.capture_stills_at(now);
        let (current, current_bytes) = jobs.try_recv().unwrap();
        done.send((late, Picture::decode_animated(&bytes).ok()))
            .unwrap();
        worker.take_decoded_at(now);
        assert_eq!(
            worker.page.as_ref().unwrap().media[0].picture,
            good.media[0].picture
        );
        assert_eq!(
            worker.page.as_ref().unwrap().media[0].still,
            good.media[0].still
        );
        assert_eq!(worker.svg_accepted[&first.1.0].owner, first);
        assert!(
            worker.pictures.is_empty(),
            "old data never becomes current-key success"
        );
        assert_eq!(
            worker.recovery[&current.1].pending.as_ref().unwrap().0,
            current.2
        );
        assert_eq!(worker.outstanding.len(), 1);
        done.send((
            current.clone(),
            Picture::decode_animated(&current_bytes).ok(),
        ))
        .unwrap();
        worker.take_decoded_at(now);
        assert_eq!(worker.svg_accepted[&first.1.0].owner, current);
        assert!(worker.pictures.contains_key(&current.1));
        assert_eq!(worker.svg_accepted.len(), 1);
    }

    #[test]
    fn recovery_late_same_key_completions_never_clear_a_new_attempt() {
        for newest_first in [false, true] {
            let engine = recovery_engine(1);
            let state = engine.state.clone();
            let original = state.lock().unwrap().pages["recovery"].clone();
            let update = |_: Update| {};
            let (mut worker, jobs, done) = recovery_worker(engine, &update);
            worker.read().unwrap();
            let now = Instant::now();
            worker.capture_stills_at(now);
            let (old, bytes) = jobs.try_recv().unwrap();
            // The same key reappears while its pruned old attempt is still in flight.
            state
                .lock()
                .unwrap()
                .pages
                .insert("recovery".into(), recovery_svg_snapshot("", "red"));
            worker.read().unwrap();
            state
                .lock()
                .unwrap()
                .pages
                .insert("recovery".into(), original);
            worker.read().unwrap();
            worker.capture_stills_at(now);
            let (new, _) = jobs.try_recv().unwrap();
            assert_eq!(old.1, new.1);
            assert_ne!(old.2, new.2);
            let picture = Picture::decode_animated(&bytes).unwrap();
            if newest_first {
                done.send((new.clone(), Some(picture.clone()))).unwrap();
            }
            done.send((
                old,
                Some(Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap()),
            ))
            .unwrap();
            worker.take_decoded_at(now);
            if !newest_first {
                assert_eq!(
                    worker.outstanding.len(),
                    1,
                    "old completion releases no newer slot"
                );
                assert_eq!(
                    worker
                        .recovery
                        .values()
                        .next()
                        .unwrap()
                        .pending
                        .as_ref()
                        .unwrap()
                        .0,
                    new.2
                );
                assert!(worker.page.as_ref().unwrap().media[0].picture.is_none());
                done.send((new, Some(picture.clone()))).unwrap();
                worker.take_decoded_at(now);
            }
            assert_eq!(
                worker.page.as_ref().unwrap().media[0].picture.as_ref(),
                Some(&picture)
            );
            assert!(worker.outstanding.is_empty());
        }
    }

    #[test]
    fn recovery_churn_prunes_live_state_but_late_jobs_release_transport_once() {
        use crate::testing::PageBuilder;
        let engine = recovery_engine(1);
        let state = engine.state.clone();
        let update = |_: Update| {};
        let (mut worker, jobs, done) = recovery_worker(engine, &update);
        let now = Instant::now();
        let mut pending = Vec::new();
        let mut previous = None;
        for n in 0..12 {
            // Same node with a changed source/size; navigation reuses node IDs too.
            let url = if n < 6 {
                "https://example.test/recovery"
            } else {
                "https://example.test/navigation"
            };
            let mut builder = PageBuilder::new(url, "recovery", 400.0, 800.0);
            builder.image(
                builder.body(),
                &format!("https://example.test/{n}.png"),
                "image",
                Rect::new(0.0, 0.0, 16.0 + n as f64, 16.0),
            );
            state
                .lock()
                .unwrap()
                .pages
                .insert("recovery".into(), builder.snapshot());
            let page = worker.read().unwrap();
            assert!(page.media[0].picture.is_none());
            assert!(page.media[0].still.is_none());
            let key = key_of(&page.media[0]);
            if let Some(old) = previous {
                assert_ne!(old, key);
            }
            previous = Some(key);
            worker.capture_stills_at(now);
            pending.extend(jobs.try_iter());
            assert!(worker.recovery.len() <= 1);
            assert!(worker.stills.len() <= 1);
            assert!(worker.pictures.is_empty());
            assert!(worker.outstanding.len() <= 8);
        }
        assert_eq!(pending.len(), 8, "churn cannot queue unbounded old jobs");
        let (old, bytes) = pending.pop().unwrap();
        done.send((old.clone(), Picture::decode_animated(&bytes).ok()))
            .unwrap();
        worker.take_decoded_at(now);
        assert_eq!(worker.outstanding.len(), 7);
        worker.capture_stills_at(now);
        let (new, bytes) = jobs.try_recv().unwrap();
        assert_eq!(worker.outstanding.len(), 8);
        // A duplicate completion cannot make room a second time.
        done.send((old, None)).unwrap();
        worker.take_decoded_at(now);
        assert_eq!(worker.outstanding.len(), 8);
        done.send((new, Picture::decode_animated(&bytes).ok()))
            .unwrap();
        worker.take_decoded_at(now);
        assert!(worker.page.as_ref().unwrap().media[0].picture.is_some());
        for (id, bytes) in pending {
            done.send((id, Picture::decode_animated(&bytes).ok()))
                .unwrap();
        }
        worker.take_decoded_at(now);
        assert!(worker.outstanding.is_empty());
        state
            .lock()
            .unwrap()
            .pages
            .insert("recovery".into(), recovery_svg_snapshot("", "red"));
        worker.read().unwrap();
        worker.capture_stills_at(now);
        let (removed, bytes) = jobs.try_recv().unwrap();
        let builder = PageBuilder::new("https://example.test/recovery", "empty", 400.0, 800.0);
        state
            .lock()
            .unwrap()
            .pages
            .insert("recovery".into(), builder.snapshot());
        worker.read().unwrap();
        assert!(worker.recovery.is_empty());
        assert!(worker.stills.is_empty());
        assert!(worker.pictures.is_empty());
        done.send((removed, Picture::decode_animated(&bytes).ok()))
            .unwrap();
        worker.take_decoded_at(now);
        assert!(worker.outstanding.is_empty());
        assert!(worker.page.as_ref().unwrap().media.is_empty());
        assert!(worker.svg_accepted.is_empty());
    }

    #[test]
    fn source_publication_pairs_all_five_paths_and_reuses_passive_relations() {
        use crate::engine::{Frame, LiveRead, PageSnapshot, Playback};
        use crate::testing::PageBuilder;
        use std::{cell::RefCell, io};
        struct State {
            snapshot: PageSnapshot,
            text: &'static str,
            time: f64,
            interactive: bool,
            snapshots: usize,
        }
        struct Source(Arc<Mutex<State>>);
        impl Engine for Source {
            fn resize(&mut self, _: u16, _: u16) -> io::Result<()> {
                Ok(())
            }
            fn open(&mut self, _: TabId, _: &str) -> io::Result<()> {
                Ok(())
            }
            fn history(&mut self, _: TabId, _: HistoryStep) -> io::Result<()> {
                Ok(())
            }
            fn state(&self, _: TabId) -> PageState {
                PageState::default()
            }
            fn snapshot(&mut self, _: TabId) -> io::Result<PageSnapshot> {
                let mut state = self.0.lock().unwrap();
                state.snapshots += 1;
                Ok(state.snapshot.clone())
            }
            fn capture(&mut self, _: TabId, _: Rect, _: Format) -> io::Result<Frame> {
                Err(io::Error::other("no capture"))
            }
            fn act(&mut self, _: TabId, _: &PageAction) -> io::Result<()> {
                Ok(())
            }
            fn events(&mut self) -> Vec<EngineEvent> {
                Vec::new()
            }
            fn resource(&mut self, _: TabId, _: &str) -> io::Result<Vec<u8>> {
                Ok(include_bytes!("../tests/fixtures/glow.png").to_vec())
            }
            fn playback(&mut self, _: TabId, _: NodeId) -> io::Result<Playback> {
                Ok(Playback {
                    paused: false,
                    time: self.0.lock().unwrap().time,
                    ready: 4,
                    ..Default::default()
                })
            }
            fn read_live(&mut self, _: TabId, _: NodeId, _: bool) -> io::Result<LiveRead> {
                let state = self.0.lock().unwrap();
                Ok(LiveRead {
                    order: vec![7],
                    fresh: vec![(7, serde_json::json!([{"t": state.text}]))],
                    first_shown: Some(7),
                    interactive: state.interactive,
                })
            }
        }
        let mut builder = PageBuilder::new("https://example.test/a", "source", 400.0, 400.0);
        let left = builder.element(builder.body(), "DIV", &[], &[], None);
        let right = builder.element(builder.body(), "DIV", &[], &[], None);
        let text = builder.text(left, "Words", Rect::new(0.0, 0.0, 100.0, 18.0));
        builder.image(
            builder.body(),
            "https://example.test/a.png",
            "image",
            Rect::new(0.0, 30.0, 100.0, 100.0),
        );
        builder.element(
            builder.body(),
            "VIDEO",
            &[("src", "https://example.test/a.webm")],
            &[],
            Some(Rect::new(0.0, 140.0, 100.0, 100.0)),
        );
        let log = builder.element(
            builder.body(),
            "DIV",
            &[("role", "log")],
            &[],
            Some(Rect::new(120.0, 30.0, 100.0, 100.0)),
        );
        let entry = builder.element(
            log,
            "DIV",
            &[],
            &[],
            Some(Rect::new(120.0, 30.0, 100.0, 18.0)),
        );
        builder.text(entry, "first", Rect::new(120.0, 30.0, 100.0, 18.0));
        let state = Arc::new(Mutex::new(State {
            snapshot: builder.snapshot(),
            text: "first",
            time: 0.0,
            interactive: false,
            snapshots: 0,
        }));
        let engine: Box<dyn Engine> = Box::new(Source(state.clone()));
        let updates = RefCell::new(Vec::new());
        let paired = |update: Update, source: Option<SourcePage>| {
            if let Update::Page(page) = update {
                let source = source.expect("every Page has its source");
                assert!(Arc::ptr_eq(&page, source.page()));
                updates.borrow_mut().push(source);
            } else {
                assert!(source.is_none());
            }
        };
        let legacy = |_: Update| panic!("source publication must use paired callback");
        let (decode, jobs) = mpsc::channel();
        let (done, decoded) = mpsc::channel();
        let (decode_inline, _inline_jobs) = mpsc::channel();
        let (_inline_done, decoded_inline) = mpsc::channel();
        let mut worker = Worker {
            engine,
            update: &legacy,
            source_update: Some(&paired),
            source: None,
            revisions: Revisions::new(),
            page: None,
            state: PageState::default(),
            dirty: None,
            last_read: None,
            refused: false,
            stills: HashMap::new(),
            pictures: HashMap::new(),
            decode,
            decoded,
            recovery: HashMap::new(),
            svg_accepted: HashMap::new(),
            outstanding: HashSet::new(),
            next_attempt: 0,
            inline: HashMap::new(),
            inline_asked: HashSet::new(),
            inline_loader: None,
            decode_inline,
            decoded_inline,
            metrics: None,
            frames: Frames::default(),
            on_screen: HashSet::new(),
            intervals: HashMap::new(),
            last_frame: None,
            starved: false,
            scale: 1.0,
            scale_given: 1.0,
            area_given: None,
            view_width: None,
            view_at: None,
            shown_time: HashMap::new(),
            live: HashMap::new(),
            appended: HashSet::new(),
            settle: None,
            stills_pending: false,
            pixels: false,
            pixels_cast: false,
            screen_at: None,
            canvases: HashMap::new(),
            asked: None,
        };
        worker.read().unwrap();
        let first = updates.borrow()[0].clone();
        worker.read().unwrap();
        assert_eq!(updates.borrow().len(), 1, "identical read is silent");
        state.lock().unwrap().text = "changed live text";
        worker.read_live();
        assert_eq!(updates.borrow().len(), 2);
        assert_eq!(
            worker.page.as_ref().unwrap().logs[0].entries[0].text(),
            "changed live text"
        );
        state.lock().unwrap().time = 1.0;
        worker.follow_playback();
        assert_eq!(updates.borrow().len(), 3);
        worker.capture_stills();
        assert_eq!(updates.borrow().len(), 4);
        let (job, _) = jobs.try_recv().unwrap();
        done.send((
            job.clone(),
            Some(Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap()),
        ))
        .unwrap();
        worker.take_decoded();
        assert_eq!(updates.borrow().len(), 5);
        for source in updates.borrow().iter() {
            assert!(Arc::ptr_eq(&first.relations, &source.relations));
        }
        state.lock().unwrap().snapshot.dom["documents"][0]["nodes"]["parentIndex"][text] =
            serde_json::json!(right);
        let before = worker.page.clone().unwrap();
        worker.read().unwrap();
        assert_eq!(
            updates.borrow().len(),
            6,
            "relationship-only read publishes"
        );
        let changed = updates.borrow().last().unwrap().clone();
        assert_eq!(changed.page(), &before);
        assert!(!first.same_relations(&changed));
        let control = builder.element(
            entry,
            "A",
            &[("href", "/guarded")],
            &[("display", "inline")],
            Some(Rect::new(120.0, 50.0, 100.0, 18.0)),
        );
        builder.text(
            control,
            "Guarded live action",
            Rect::new(120.0, 50.0, 100.0, 18.0),
        );
        let before_reads = {
            let mut state = state.lock().unwrap();
            state.snapshot = builder.snapshot();
            state.interactive = true;
            state.snapshots
        };
        worker.read_live();
        assert_eq!(
            state.lock().unwrap().snapshots,
            before_reads + 1,
            "interactive log takes a complete source snapshot"
        );
        assert_eq!(updates.borrow().len(), 7);
        let active = worker.page.as_ref().unwrap();
        assert_eq!(active.controls.len(), 1);
        assert!(active.check(&active.controls[0].binding(active)).is_ok());
        state.lock().unwrap().snapshot =
            PageBuilder::new("https://example.test/b", "new", 400.0, 400.0).snapshot();
        worker.read().unwrap();
        let nav = updates.borrow().last().unwrap().clone();
        assert_ne!(nav.page().document, first.page().document);
        done.send((
            job,
            Some(Picture::from_rgba(1, 1, vec![0, 255, 0, 255]).unwrap()),
        ))
        .unwrap();
        worker.take_decoded();
        assert_eq!(
            updates.borrow().len(),
            8,
            "stale decode cannot republish an old pair"
        );
    }

    #[test]
    fn video_captures_due_whole_regions_missing_from_fresh_or_quiet_screencasts() {
        use crate::{engine::Playback, testing::PageBuilder};
        use rich_embed::{FrameMeta, Pixels, ScreencastFrame};
        use std::cell::RefCell;

        for (name, rect, fresh, visible, capture) in [
            (
                "contained",
                Rect::new(10.0, 10.0, 20.0, 20.0),
                true,
                true,
                false,
            ),
            (
                "below edge",
                Rect::new(10.0, 90.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "above edge",
                Rect::new(10.0, -10.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "right edge",
                Rect::new(90.0, 10.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "left edge",
                Rect::new(-10.0, 10.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "below viewport",
                Rect::new(10.0, 150.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "right of viewport",
                Rect::new(150.0, 10.0, 20.0, 20.0),
                true,
                true,
                true,
            ),
            (
                "quiet",
                Rect::new(10.0, 10.0, 20.0, 20.0),
                false,
                true,
                true,
            ),
            (
                "not presented",
                Rect::new(10.0, 90.0, 20.0, 20.0),
                true,
                false,
                false,
            ),
        ] {
            let dir = std::env::temp_dir().join(format!("tab-video-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let engine = FakeEngine::from_dir(&dir).unwrap();
            let state = engine.state.clone();
            let mut source = PageBuilder::new("https://example.test", "Video", 200.0, 200.0);
            source.element(
                source.body(),
                "VIDEO",
                &[("src", "movie.webm")],
                &[],
                Some(rect),
            );
            let mut page = crate::model::build(&source.snapshot(), &mut Revisions::new());
            assert_eq!(page.media.len(), 1, "{name}");
            let node = page.media[0].node;
            page.media[0].playback = Some(Playback {
                ready: 4,
                ..Playback::default()
            });
            let frame = ScreencastFrame {
                pixels: Pixels::new(100, 100, vec![7; 100 * 100 * 3]).unwrap(),
                meta: FrameMeta {
                    page_scale: 1.0,
                    device_width: 100.0,
                    device_height: 100.0,
                    ..FrameMeta::default()
                },
            };
            {
                let mut state = state.lock().unwrap();
                state.screencast = true;
                if fresh {
                    state.frames.push(frame.clone());
                }
            }
            let delivered = RefCell::new(Vec::new());
            let update = |update| {
                if let Update::Frame(frame) = update {
                    delivered.borrow_mut().push(frame);
                }
            };
            let (decode, _jobs) = mpsc::channel();
            let (_done, decoded) = mpsc::channel();
            let (decode_inline, _inline_jobs) = mpsc::channel();
            let (_inline_done, decoded_inline) = mpsc::channel();
            let mut worker = Worker {
                engine: Box::new(engine),
                update: &update,
                source_update: None,
                source: None,
                revisions: Revisions::new(),
                page: Some(Arc::new(page)),
                state: PageState::default(),
                dirty: None,
                last_read: None,
                refused: false,
                stills: HashMap::new(),
                pictures: HashMap::new(),
                decode,
                decoded,
                recovery: HashMap::new(),
                svg_accepted: HashMap::new(),
                outstanding: HashSet::new(),
                next_attempt: 0,
                inline: HashMap::new(),
                inline_asked: HashSet::new(),
                inline_loader: None,
                decode_inline,
                decoded_inline,
                metrics: Some(Metrics {
                    scroll_y: 0.0,
                    viewport_height: 100.0,
                    content_height: 200.0,
                }),
                frames: Frames::default(),
                on_screen: if visible {
                    HashSet::from([node])
                } else {
                    HashSet::new()
                },
                intervals: HashMap::new(),
                last_frame: Some(Instant::now() - QUIET),
                starved: false,
                scale: 1.0,
                scale_given: 1.0,
                area_given: None,
                view_width: None,
                view_at: None,
                shown_time: HashMap::new(),
                live: HashMap::new(),
                appended: HashSet::new(),
                settle: None,
                stills_pending: false,
                pixels: false,
                pixels_cast: false,
                screen_at: None,
                canvases: HashMap::new(),
                asked: None,
            };
            worker.frames.on = true;
            worker.video();
            assert_eq!(
                state.lock().unwrap().captures,
                if capture { vec![rect] } else { vec![] },
                "{name}"
            );
            assert_eq!(delivered.borrow().len(), usize::from(visible), "{name}");
            if visible {
                let frames = delivered.borrow();
                let pixels = &frames[0].pixels;
                assert_eq!((pixels.width, pixels.height), (20, 20), "{name}");
                let expected = if capture { [30, 90, 220] } else { [7; 3] };
                assert!(
                    pixels.rgb.as_chunks::<3>().0.iter().all(|p| *p == expected),
                    "{name}"
                );
            }
            // A fresh cast before the next slot cannot duplicate either path.
            worker.intervals.insert(node, Duration::from_secs(60));
            state.lock().unwrap().frames.push(frame);
            worker.video();
            assert_eq!(
                state.lock().unwrap().captures.len(),
                usize::from(capture),
                "{name}"
            );
            assert_eq!(delivered.borrow().len(), usize::from(visible), "{name}");
        }
    }

    #[test]
    fn the_screencast_is_of_the_playing_region_while_chrome_shows_all_of_it() {
        use crate::{engine::Playback, testing::PageBuilder};
        let dir = std::env::temp_dir().join(format!("tab-area-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = FakeEngine::from_dir(&dir).unwrap();
        let state = engine.state.clone();
        {
            let mut state = state.lock().unwrap();
            state.scroll_y = 0.0;
            state.viewport_height = 600.0;
        }
        let rect = Rect::new(20.0, 100.0, 400.0, 225.0);
        let mut source = PageBuilder::new("https://example.test", "Video", 960.0, 2000.0);
        source.element(
            source.body(),
            "VIDEO",
            &[("src", "m.webm")],
            &[],
            Some(rect),
        );
        let mut page = crate::model::build(&source.snapshot(), &mut Revisions::new());
        let node = page.media[0].node;
        page.media[0].playback = Some(Playback {
            ready: 4,
            ..Playback::default()
        });
        let update = |_| {};
        let (mut worker, _jobs, _done) = recovery_worker(engine, &update);
        worker.page = Some(Arc::new(page));
        worker.on_screen.insert(node);
        worker.view_width = Some(960.0);
        // It plays wholly in Chrome's viewport: only its box is cast.
        worker.video();
        assert!(state.lock().unwrap().screencast);
        assert_eq!(state.lock().unwrap().screencast_area, Some(rect));
        // Again with nothing changed: the area is not asked for again.
        worker.video();
        assert_eq!(state.lock().unwrap().screencast_areas.len(), 1);
        // The page scrolled itself half past it: the whole viewport again,
        // once Chrome's view is looked at.
        state.lock().unwrap().scroll_y = 200.0;
        worker.view_at = Some(Instant::now() - PLAYBACK);
        worker.video();
        assert_eq!(state.lock().unwrap().screencast_area, None);
        // The pixel view shows the whole viewport.
        state.lock().unwrap().scroll_y = 0.0;
        worker.view_at = Some(Instant::now() - PLAYBACK);
        worker.video();
        assert_eq!(state.lock().unwrap().screencast_area, Some(rect));
        worker.set_pixels(true);
        assert_eq!(state.lock().unwrap().screencast_area, None);
        worker.set_pixels(false);
        // It stops: the screencast stops, of the viewport.
        worker.video();
        assert_eq!(state.lock().unwrap().screencast_area, Some(rect));
        worker.on_screen.clear();
        worker.video();
        assert!(!state.lock().unwrap().screencast);
        assert_eq!(state.lock().unwrap().screencast_area, None);
    }

    #[test]
    fn model_media_capture_waits_for_navigation_and_refused_snapshot_retry() {
        use crate::{
            engine::{Frame, HistoryStep, PageSnapshot},
            testing::PageBuilder,
        };
        use std::{io, sync::Arc};
        for refused in [false, true] {
            let current = Arc::new(AtomicBool::new(false));
            let published = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let mut pages = Vec::new();
            let mut nodes = Vec::new();
            for host in ["a", "b"] {
                let mut p =
                    PageBuilder::new(&format!("https://{host}.test"), "Capture", 800.0, 640.0);
                let canvas = p.element(
                    p.body(),
                    "CANVAS",
                    &[],
                    &[],
                    Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
                );
                nodes.push(canvas as i64 + 1);
                pages.push(p.snapshot());
            }
            struct Source {
                current: Arc<AtomicBool>,
                published: Arc<std::sync::atomic::AtomicU64>,
                pages: Vec<PageSnapshot>,
                event: bool,
                refuse: bool,
                bytes: Vec<u8>,
            }
            impl Engine for Source {
                fn resize(&mut self, _: u16, _: u16) -> io::Result<()> {
                    Ok(())
                }
                fn open(&mut self, _: TabId, _: &str) -> io::Result<()> {
                    Ok(())
                }
                fn history(&mut self, _: TabId, _: HistoryStep) -> io::Result<()> {
                    Ok(())
                }
                fn state(&self, _: TabId) -> PageState {
                    PageState::default()
                }
                fn snapshot(&mut self, _: TabId) -> io::Result<PageSnapshot> {
                    let b = self.current.load(Ordering::SeqCst);
                    if b && std::mem::take(&mut self.refuse) {
                        return Err(io::Error::other("renderer swapping"));
                    }
                    Ok(self.pages[usize::from(b)].clone())
                }
                fn capture(&mut self, _: TabId, _: Rect, format: Format) -> io::Result<Frame> {
                    if self.current.load(Ordering::SeqCst) {
                        assert_eq!(
                            self.published.load(Ordering::SeqCst),
                            2,
                            "B pixels captured under old model A"
                        );
                    }
                    Ok(Frame {
                        format,
                        bytes: self.bytes.clone(),
                    })
                }
                fn act(&mut self, _: TabId, _: &PageAction) -> io::Result<()> {
                    Ok(())
                }
                fn events(&mut self) -> Vec<EngineEvent> {
                    if self.current.load(Ordering::SeqCst) && !self.event {
                        self.event = true;
                        vec![EngineEvent::Navigated("https://b.test".into())]
                    } else {
                        Vec::new()
                    }
                }
            }
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::new_rgb8(1, 1)
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            let source = Source {
                current: current.clone(),
                published: published.clone(),
                pages,
                event: false,
                refuse: refused,
                bytes: bytes.into_inner(),
            };
            let (send, receive) = mpsc::channel();
            send.send((
                1,
                Command::Regions {
                    on_screen: nodes,
                    intervals: Vec::new(),
                    scale: 1.0,
                },
            ))
            .unwrap();
            let stop = AtomicBool::new(false);
            run(Box::new(source), receive, &stop, &|update| match update {
                Update::Page(page) => {
                    if page.url == "https://a.test" {
                        published.store(1, Ordering::SeqCst);
                        current.store(true, Ordering::SeqCst);
                    } else if page.url == "https://b.test" {
                        published.store(2, Ordering::SeqCst);
                    }
                }
                Update::Frame(_) => {
                    assert_eq!(published.load(Ordering::SeqCst), 2);
                    stop.store(true, Ordering::SeqCst);
                }
                _ => {}
            });
            assert_eq!(published.load(Ordering::SeqCst), 2);
        }
    }

    /// A producer with a sole fresh B frame, including one refused model read.
    /// No Update callbacks are fabricated: the actual run loop emits all of them.
    #[test]
    fn fresh_screen_waits_for_initial_or_navigation_model() {
        use crate::engine::{Frame, HistoryStep, PageSnapshot};
        use crate::testing::feed_snapshot;
        use rich_embed::{Pixels, ScreencastFrame};
        use std::io;
        struct Source {
            old: bool,
            refused: bool,
            on: bool,
            frame: Option<ScreencastFrame>,
            navigation: bool,
            during_read: bool,
        }
        impl Engine for Source {
            fn resize(&mut self, _: u16, _: u16) -> io::Result<()> {
                Ok(())
            }
            fn open(&mut self, _: TabId, _: &str) -> io::Result<()> {
                Ok(())
            }
            fn history(&mut self, _: TabId, _: HistoryStep) -> io::Result<()> {
                Ok(())
            }
            fn state(&self, _: TabId) -> PageState {
                PageState::default()
            }
            fn snapshot(&mut self, _: TabId) -> io::Result<PageSnapshot> {
                if std::mem::take(&mut self.during_read) {
                    self.navigation = true;
                    return Ok(feed_snapshot("https://a.test", 1));
                }
                if self.old && !self.on {
                    return Ok(feed_snapshot("https://a.test", 1));
                }
                if self.refused {
                    self.refused = false;
                    return Err(io::Error::other("renderer swapping"));
                }
                Ok(feed_snapshot("https://b.test", 1))
            }
            fn capture(&mut self, _: TabId, _: Rect, _: Format) -> io::Result<Frame> {
                Err(io::Error::other("no regions"))
            }
            fn act(&mut self, _: TabId, _: &PageAction) -> io::Result<()> {
                Ok(())
            }
            fn set_screencast(&mut self, _: TabId, on: bool) -> io::Result<()> {
                self.on = on;
                self.navigation = on && self.old;
                Ok(())
            }
            fn screencast_frame(&mut self, _: TabId) -> Option<ScreencastFrame> {
                self.on.then(|| self.frame.take()).flatten()
            }
            fn events(&mut self) -> Vec<EngineEvent> {
                if std::mem::take(&mut self.navigation) {
                    vec![EngineEvent::Navigated("https://b.test".into())]
                } else {
                    vec![]
                }
            }
        }
        for (old, during_read) in [(false, false), (true, false), (false, true)] {
            for refused in [false, true] {
                let engine = Source {
                    old,
                    refused,
                    on: false,
                    navigation: false,
                    during_read,
                    frame: Some(ScreencastFrame {
                        pixels: Pixels::new(1, 1, vec![0, 0, 255]).unwrap(),
                        meta: Default::default(),
                    }),
                };
                let (send, receive) = mpsc::channel();
                // Existing A is genuinely read before the navigation command.
                send.send((1, Command::Pixels(!old))).unwrap();
                let stop = AtomicBool::new(false);
                let updates = Mutex::new(Vec::new());
                run(Box::new(engine), receive, &stop, &|update| match update {
                    Update::Page(page) => {
                        updates.lock().unwrap().push(page.url.clone());
                        if page.url == "https://a.test" {
                            send.send((2, Command::Pixels(true))).unwrap();
                        }
                    }
                    Update::Screen(frame) => {
                        assert_eq!(frame.pixels.rgb, [0, 0, 255]);
                        updates.lock().unwrap().push("screen B".into());
                        stop.store(true, Ordering::SeqCst);
                    }
                    _ => {}
                });
                let updates = updates.into_inner().unwrap();
                let expected = if old || during_read {
                    vec!["https://a.test", "https://b.test", "screen B"]
                } else {
                    vec!["https://b.test", "screen B"]
                };
                assert_eq!(
                    updates, expected,
                    "old={old}, refused={refused}, during_read={during_read}"
                );
            }
        }
    }

    #[test]
    fn old_decoder_completion_cannot_replace_new_document_image() {
        use crate::testing::PageBuilder;
        let dir = std::env::temp_dir().join(format!("tab-decoder-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = FakeEngine::from_dir(&dir).unwrap();
        let state = engine.state.clone();
        for name in ["a", "b"] {
            let mut page = PageBuilder::new(
                &format!("https://example.test/{name}"),
                "test",
                400.0,
                400.0,
            );
            page.image(
                page.body(),
                "https://example.test/image.png",
                "image",
                Rect::new(0.0, 0.0, 100.0, 100.0),
            );
            state
                .lock()
                .unwrap()
                .pages
                .insert(name.into(), page.snapshot());
        }
        state.lock().unwrap().current = Some("a".into());
        let engine: Box<dyn Engine> = Box::new(engine);
        let update = |_: Update| {};
        let (decode, jobs) = mpsc::channel();
        let (done, decoded) = mpsc::channel();
        let (decode_inline, _inline_jobs) = mpsc::channel();
        let (_inline_done, decoded_inline) = mpsc::channel();
        let mut worker = Worker {
            engine,
            update: &update,
            source_update: None,
            source: None,
            revisions: Revisions::new(),
            page: None,
            state: PageState::default(),
            dirty: None,
            last_read: None,
            refused: false,
            stills: HashMap::new(),
            pictures: HashMap::new(),
            decode,
            decoded,
            recovery: HashMap::new(),
            svg_accepted: HashMap::new(),
            outstanding: HashSet::new(),
            next_attempt: 0,
            inline: HashMap::new(),
            inline_asked: HashSet::new(),
            inline_loader: None,
            decode_inline,
            decoded_inline,
            metrics: None,
            frames: Frames::default(),
            on_screen: HashSet::new(),
            intervals: HashMap::new(),
            last_frame: None,
            starved: false,
            scale: 1.0,
            scale_given: 1.0,
            area_given: None,
            view_width: None,
            view_at: None,
            shown_time: HashMap::new(),
            live: HashMap::new(),
            appended: HashSet::new(),
            settle: None,
            stills_pending: false,
            pixels: false,
            pixels_cast: false,
            screen_at: None,
            canvases: HashMap::new(),
            asked: None,
        };
        worker.read().unwrap();
        worker.capture_stills();
        let (old_key, _) = jobs.try_recv().unwrap();
        state.lock().unwrap().current = Some("b".into());
        let page = worker.read().unwrap();
        assert!(page.media[0].picture.is_none());
        assert!(page.media[0].still.is_none());
        worker.capture_stills();
        let (new_key, _) = jobs.try_recv().unwrap();
        let old = Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
        done.send((old_key, Some(old))).unwrap();
        worker.take_decoded();
        assert!(worker.page.as_ref().unwrap().media[0].picture.is_none());
        let new = Picture::from_rgba(1, 1, vec![0, 255, 0, 255]).unwrap();
        done.send((new_key, Some(new.clone()))).unwrap();
        worker.take_decoded();
        assert_eq!(
            worker.page.as_ref().unwrap().media[0].picture.as_ref(),
            Some(&new)
        );
    }

    #[test]
    fn same_document_svg_revisions_keep_only_previously_accepted_fallback() {
        use crate::testing::PageBuilder;
        for newest_first in [false, true] {
            let dir = std::env::temp_dir().join(format!("tab-revisions-{}", std::process::id()));
            std::fs::create_dir_all(&dir).unwrap();
            let engine = FakeEngine::from_dir(&dir).unwrap();
            let state = engine.state.clone();
            for (name, color) in [("one", "red"), ("two", "green"), ("three", "blue")] {
                let mut page = PageBuilder::new("https://example.test/page", "test", 400.0, 400.0);
                page.element(
                    page.body(),
                    "SVG",
                    &[("fill", color)],
                    &[],
                    Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
                );
                state
                    .lock()
                    .unwrap()
                    .pages
                    .insert(name.into(), page.snapshot());
            }
            state.lock().unwrap().current = Some("one".into());
            let engine: Box<dyn Engine> = Box::new(engine);
            let update = |_: Update| {};
            let (decode, jobs) = mpsc::channel();
            let (done, decoded) = mpsc::channel();
            let (decode_inline, _inline_jobs) = mpsc::channel();
            let (_inline_done, decoded_inline) = mpsc::channel();
            let mut worker = Worker {
                engine,
                update: &update,
                source_update: None,
                source: None,
                revisions: Revisions::new(),
                page: None,
                state: PageState::default(),
                dirty: None,
                last_read: None,
                refused: false,
                stills: HashMap::new(),
                pictures: HashMap::new(),
                decode,
                decoded,
                recovery: HashMap::new(),
                svg_accepted: HashMap::new(),
                outstanding: HashSet::new(),
                next_attempt: 0,
                inline: HashMap::new(),
                inline_asked: HashSet::new(),
                inline_loader: None,
                decode_inline,
                decoded_inline,
                metrics: None,
                frames: Frames::default(),
                on_screen: HashSet::new(),
                intervals: HashMap::new(),
                last_frame: None,
                starved: false,
                scale: 1.0,
                scale_given: 1.0,
                area_given: None,
                view_width: None,
                view_at: None,
                shown_time: HashMap::new(),
                live: HashMap::new(),
                appended: HashSet::new(),
                settle: None,
                stills_pending: false,
                pixels: false,
                pixels_cast: false,
                screen_at: None,
                canvases: HashMap::new(),
                asked: None,
            };
            worker.read().unwrap();
            worker.capture_stills();
            let (key_one, _) = jobs.try_recv().unwrap();
            state.lock().unwrap().current = Some("two".into());
            let page_two = worker.read().unwrap();
            assert!(page_two.media[0].picture.is_none());
            worker.capture_stills();
            let (key_two, _) = jobs.try_recv().unwrap();
            assert_ne!(key_one, key_two);
            let one = Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
            let two = Picture::from_rgba(1, 1, vec![0, 255, 0, 255]).unwrap();
            if newest_first {
                done.send((key_two, Some(two.clone()))).unwrap();
                worker.take_decoded();
                done.send((key_one, Some(one))).unwrap();
                worker.take_decoded();
            } else {
                done.send((key_one, Some(one.clone()))).unwrap();
                worker.take_decoded();
                assert_eq!(
                    worker.page.as_ref().unwrap().media[0].picture.as_ref(),
                    None,
                    "an older revision cannot publish into the current key"
                );
                done.send((key_two, Some(two.clone()))).unwrap();
                worker.take_decoded();
            }
            assert_eq!(
                worker.page.as_ref().unwrap().media[0].picture.as_ref(),
                Some(&two)
            );
            state.lock().unwrap().current = Some("three".into());
            let page_three = worker.read().unwrap();
            assert_eq!(
                page_three.media[0].picture.as_ref(),
                Some(&two),
                "accepted SVG pixels remain visible during a content revision"
            );
        }
    }

    #[test]
    fn inline_decodes_are_scoped_to_the_document() {
        use crate::testing::PageBuilder;
        let dir = std::env::temp_dir().join(format!("tab-inline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = FakeEngine::from_dir(&dir).unwrap();
        let state = engine.state.clone();
        let src = "https://example.test/emote.png";
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        state
            .lock()
            .unwrap()
            .resources
            .insert(src.into(), png.into_inner());
        for name in ["a", "b"] {
            let mut page = PageBuilder::new(
                &format!("https://example.test/{name}"),
                "test",
                400.0,
                400.0,
            );
            let log = page.element(
                page.body(),
                "DIV",
                &[("role", "log")],
                &[],
                Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            );
            let entry = page.element(log, "DIV", &[], &[], Some(Rect::new(0.0, 0.0, 100.0, 20.0)));
            page.image(entry, src, "emote", Rect::new(0.0, 0.0, 16.0, 16.0));
            state
                .lock()
                .unwrap()
                .pages
                .insert(name.into(), page.snapshot());
        }
        state.lock().unwrap().current = Some("a".into());
        let engine: Box<dyn Engine> = Box::new(engine);
        let update = |_: Update| {};
        let (decode, _jobs) = mpsc::channel();
        let (_done, decoded) = mpsc::channel();
        let (decode_inline, inline_jobs) = mpsc::channel();
        let (inline_done, decoded_inline) = mpsc::channel();
        let mut worker = Worker {
            engine,
            update: &update,
            source_update: None,
            source: None,
            revisions: Revisions::new(),
            page: None,
            state: PageState::default(),
            dirty: None,
            last_read: None,
            refused: false,
            stills: HashMap::new(),
            pictures: HashMap::new(),
            decode,
            decoded,
            recovery: HashMap::new(),
            svg_accepted: HashMap::new(),
            outstanding: HashSet::new(),
            next_attempt: 0,
            inline: HashMap::new(),
            inline_asked: HashSet::new(),
            inline_loader: None,
            decode_inline,
            decoded_inline,
            metrics: None,
            frames: Frames::default(),
            on_screen: HashSet::new(),
            intervals: HashMap::new(),
            last_frame: None,
            starved: false,
            scale: 1.0,
            scale_given: 1.0,
            area_given: None,
            view_width: None,
            view_at: None,
            shown_time: HashMap::new(),
            live: HashMap::new(),
            appended: HashSet::new(),
            settle: None,
            stills_pending: false,
            pixels: false,
            pixels_cast: false,
            screen_at: None,
            canvases: HashMap::new(),
            asked: None,
        };
        worker.read().unwrap();
        let (old_key, _) = inline_jobs.try_recv().expect("first inline job");
        state.lock().unwrap().current = Some("b".into());
        worker.read().unwrap();
        let (new_key, _) = inline_jobs
            .try_recv()
            .expect("new document requests same URL");
        let old = Picture::from_rgba(1, 1, vec![255, 0, 0, 255]).unwrap();
        inline_done.send((old_key, old)).unwrap();
        worker.take_decoded();
        assert!(
            !worker
                .page
                .as_ref()
                .unwrap()
                .inline_pictures
                .contains_key(src)
        );
        let new = Picture::from_rgba(1, 1, vec![0, 255, 0, 255]).unwrap();
        inline_done.send((new_key, new.clone())).unwrap();
        worker.take_decoded();
        assert_eq!(
            worker.page.as_ref().unwrap().inline_pictures.get(src),
            Some(&new)
        );
        state.lock().unwrap().current = Some("a".into());
        worker.read().unwrap();
        assert!(
            !worker
                .page
                .as_ref()
                .unwrap()
                .inline_pictures
                .contains_key(src)
        );
        assert!(inline_jobs.try_recv().is_ok());
        // Exercise the actual concurrent loader too. It can complete the
        // old document only after B is already authoritative. Green B
        // cannot arrive until take_decoded has consumed the stale result.
        let (started, entered) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let png = |rgba| {
            let mut bytes = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(2, 2, image::Rgba(rgba)))
                .write_to(&mut bytes, image::ImageFormat::Png)
                .unwrap();
            bytes.into_inner()
        };
        let red = png([255, 0, 0, 255]);
        let green = png([0, 255, 0, 255]);
        let mut first = true;
        worker.inline_loader = Some(crate::inline_resources::InlineResources::spawn(Some(
            Box::new(move |_| {
                if first {
                    first = false;
                    started.send(()).unwrap();
                    gate.recv().unwrap();
                    Ok(red.clone())
                } else {
                    Ok(green.clone())
                }
            }),
        )));
        worker.inline_asked.clear();
        worker.inline.clear();
        worker.read().unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        state.lock().unwrap().current = Some("b".into());
        let document = worker.read().unwrap().document;
        release.send(()).unwrap();
        let end = Instant::now() + Duration::from_secs(2);
        loop {
            worker.take_decoded();
            let page = worker.page.as_ref().unwrap();
            assert_eq!(page.document, document);
            if let Some(p) = page.inline_pictures.get(src) {
                assert_eq!(p.image().to_rgba8().get_pixel(0, 0).0, [0, 255, 0, 255]);
                break;
            }
            assert!(
                Instant::now() < end,
                "new document inline completion missing"
            );
            std::thread::yield_now();
        }
        // A long sequence of unique emotes must not turn the asked/source
        // maps into a history of every URL the document has ever used.
        worker.inline_loader = Some(crate::inline_resources::InlineResources::spawn(None));
        let mut page = (**worker.page.as_ref().unwrap()).clone();
        for n in 0..1024 {
            let next = format!("https://example.test/emote-{n}.png");
            for run in page.blocks.iter_mut().flat_map(|b| &mut b.runs).chain(
                page.logs
                    .iter_mut()
                    .flat_map(|l| l.entries.iter_mut().flat_map(|e| &mut e.runs)),
            ) {
                if let Some(picture) = &mut run.picture {
                    picture.src = next.clone();
                }
            }
            worker.dress_inline(&mut page);
            assert!(worker.inline.len() <= 1);
            assert!(worker.inline_asked.len() <= 1);
            assert!(worker.inline.keys().all(|src| src == &next));
            worker.inline.insert(
                next,
                Picture::from_rgba(1, 1, vec![0, 255, 0, 255]).unwrap(),
            );
        }
    }

    #[test]
    fn failed_inline_attempts_refill_unadmitted_sources_without_a_page_refresh() {
        assert_failed_inline_attempts_refill(false);
    }

    #[test]
    fn failed_legacy_inline_attempts_refill_unadmitted_sources_without_a_page_refresh() {
        assert_failed_inline_attempts_refill(true);
    }

    fn assert_failed_inline_attempts_refill(legacy: bool) {
        use crate::testing::PageBuilder;
        let dir = std::env::temp_dir().join(format!("tab-inline-refill-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let engine = FakeEngine::from_dir(&dir).unwrap();
        let mut builder = PageBuilder::new("https://example.test/static", "test", 400.0, 400.0);
        let log = builder.element(
            builder.body(),
            "DIV",
            &[("role", "log")],
            &[],
            Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
        );
        for n in 0..200 {
            let entry =
                builder.element(log, "DIV", &[], &[], Some(Rect::new(0.0, 0.0, 100.0, 20.0)));
            builder.image(
                entry,
                &format!("https://example.test/{n}"),
                "emote",
                Rect::new(0.0, 0.0, 16.0, 16.0),
            );
        }
        {
            let mut state = engine.state.lock().unwrap();
            state.pages.insert("static".into(), builder.snapshot());
            state.current = Some("static".into());
        }
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([1, 2, 3, 255]),
        ))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
        let png = png.into_inner();
        // IHDR and the IDAT header expose dimensions without pixel data.
        let truncated = png[..41].to_vec();
        assert!(
            image::ImageReader::new(std::io::Cursor::new(&truncated))
                .with_guessed_format()
                .unwrap()
                .into_dimensions()
                .is_ok()
        );
        assert!(Picture::decode_animated(&truncated).is_err());
        let attempts = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let observed = attempts.clone();
        let (started, entered) = mpsc::sync_channel(1);
        let (release, gate) = mpsc::sync_channel(1);
        let mut first = true;
        let mut read: Option<crate::engine::ResourceReader> = Some(Box::new(move |src| {
            *observed.lock().unwrap().entry(src.into()).or_default() += 1;
            if first && !legacy {
                first = false;
                started.send(()).unwrap();
                gate.recv().unwrap();
            }
            let n: usize = src.rsplit('/').next().unwrap().parse().unwrap();
            match n {
                199 => Ok(png.clone()),
                n if n % 3 == 0 => Err(std::io::Error::other("missing fixture")),
                n if n % 3 == 1 => Ok(b"invalid image format".to_vec()),
                _ => Ok(truncated.clone()),
            }
        }));
        let loader = crate::inline_resources::InlineResources::spawn(if legacy {
            None
        } else {
            read.take()
        });
        let engine: Box<dyn Engine> = Box::new(engine);
        let published = Mutex::new(Vec::new());
        let update = |update| {
            if let Update::Page(page) = update {
                published.lock().unwrap().push(page);
            }
        };
        let (decode, _jobs) = mpsc::channel();
        let (_done, decoded) = mpsc::channel();
        let (decode_inline, _inline_jobs) = mpsc::channel();
        let (_inline_done, decoded_inline) = mpsc::channel();
        let mut worker = Worker {
            engine,
            update: &update,
            source_update: None,
            source: None,
            revisions: Revisions::new(),
            page: None,
            state: PageState::default(),
            dirty: None,
            last_read: None,
            refused: false,
            stills: HashMap::new(),
            pictures: HashMap::new(),
            decode,
            decoded,
            recovery: HashMap::new(),
            svg_accepted: HashMap::new(),
            outstanding: HashSet::new(),
            next_attempt: 0,
            inline: HashMap::new(),
            inline_asked: HashSet::new(),
            inline_loader: Some(loader),
            decode_inline,
            decoded_inline,
            metrics: None,
            frames: Frames::default(),
            on_screen: HashSet::new(),
            intervals: HashMap::new(),
            last_frame: None,
            starved: false,
            scale: 1.0,
            scale_given: 1.0,
            area_given: None,
            view_width: None,
            view_at: None,
            shown_time: HashMap::new(),
            live: HashMap::new(),
            appended: HashSet::new(),
            settle: None,
            stills_pending: false,
            pixels: false,
            pixels_cast: false,
            screen_at: None,
            canvases: HashMap::new(),
            asked: None,
        };
        worker.read().unwrap();
        if !legacy {
            entered.recv_timeout(Duration::from_secs(2)).unwrap();
        }
        let last = "https://example.test/199";
        assert!((128..=129).contains(&worker.inline_asked.len()));
        assert!(
            !worker.inline_asked.contains(last),
            "last source must initially be rejected"
        );
        assert_eq!(worker.page.as_ref().unwrap().logs[0].entries.len(), 200);
        let initial_publications = published.lock().unwrap().len();
        if !legacy {
            release.send(()).unwrap();
        }
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            // Like run(), service one legacy request on the owner. No read(),
            // live log refresh, settling pass or other decode can refill here.
            if let Some(((document, src), reply)) = worker
                .inline_loader
                .as_ref()
                .unwrap()
                .take_resource_request()
            {
                assert_eq!(document, worker.page.as_ref().unwrap().document);
                assert!(worker.inline_asked.contains(&src));
                reply.send(read.as_mut().unwrap()(&src).ok()).unwrap();
            }
            worker.take_decoded();
            if let Some(picture) = worker.page.as_ref().unwrap().inline_pictures.get(last) {
                assert_eq!(picture.image().to_rgba8().get_pixel(0, 0).0, [1, 2, 3, 255]);
                break;
            }
            assert!(
                Instant::now() < end,
                "unadmitted valid source stranded (legacy={legacy}); attempts={}",
                attempts.lock().unwrap().len()
            );
            thread::yield_now();
        }
        assert_eq!(worker.inline_asked.len(), 200);
        let attempts = attempts.lock().unwrap();
        assert_eq!(attempts.len(), 200);
        assert!(
            attempts.values().all(|&n| n == 1),
            "failed sources are not retried"
        );
        let pages = published.lock().unwrap();
        assert_eq!(
            pages.len(),
            initial_publications + 1,
            "failure progress must not publish unchanged pages"
        );
        assert!(pages.last().unwrap().inline_pictures.contains_key(last));
        assert_eq!(pages.last().unwrap().logs[0].entries.len(), 200);
    }

    /// An engine whose page pings a change at every look and whose
    /// snapshot takes `cost`, counting the snapshots and their time.
    struct Busy {
        inner: FakeEngine,
        cost: Duration,
        spent: Arc<std::sync::Mutex<(u32, Duration)>>,
    }

    impl Engine for Busy {
        fn resize(&mut self, columns: u16, rows: u16) -> std::io::Result<()> {
            self.inner.resize(columns, rows)
        }
        fn open(&mut self, tab: TabId, url: &str) -> std::io::Result<()> {
            self.inner.open(tab, url)
        }
        fn history(&mut self, tab: TabId, step: HistoryStep) -> std::io::Result<()> {
            self.inner.history(tab, step)
        }
        fn state(&self, tab: TabId) -> PageState {
            self.inner.state(tab)
        }
        fn snapshot(&mut self, tab: TabId) -> std::io::Result<crate::engine::PageSnapshot> {
            thread::sleep(self.cost);
            let mut spent = self.spent.lock().unwrap();
            spent.0 += 1;
            spent.1 += self.cost;
            self.inner.snapshot(tab)
        }
        fn capture(
            &mut self,
            tab: TabId,
            region: Rect,
            format: Format,
        ) -> std::io::Result<crate::engine::Frame> {
            self.inner.capture(tab, region, format)
        }
        fn act(&mut self, tab: TabId, action: &PageAction) -> std::io::Result<()> {
            self.inner.act(tab, action)
        }
        fn events(&mut self) -> Vec<EngineEvent> {
            let mut events = self.inner.events();
            events.push(EngineEvent::Changed);
            events
        }
    }

    #[test]
    fn a_page_that_keeps_changing_is_read_at_most_a_quarter_of_the_time() {
        let snapshots = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
        let spent = Arc::new(std::sync::Mutex::new((0, Duration::ZERO)));
        let engine = Busy {
            inner: FakeEngine::from_dir(&snapshots).unwrap(),
            cost: Duration::from_millis(120),
            spent: spent.clone(),
        };
        let tab = Tab::spawn(Box::new(engine), |_| {});
        tab.send(Command::Open("https://fixture.example/second.html".into()));
        // Past the new document's settling, then a span of pings alone.
        thread::sleep(SETTLE_MAX + Duration::from_millis(200));
        let before = *spent.lock().unwrap();
        let span = Duration::from_secs(3);
        thread::sleep(span);
        let after = *spent.lock().unwrap();
        drop(tab);
        let reads = after.0 - before.0;
        let share = (after.1 - before.1).as_secs_f64() / span.as_secs_f64();
        // Read again and again (a ping is never dropped), but each reading
        // waits three times its cost: about a quarter of the time. Without
        // the wait it reads after each debounce, over half the time.
        assert!(reads >= 2, "{reads} readings");
        assert!(
            share < 0.4,
            "reading took {:.0}% of the time",
            share * 100.0
        );
    }

    #[test]
    fn pinged_readings_wait_for_their_cost_and_for_playing_video() {
        assert_eq!(ping_read_gap(Duration::from_millis(5), false), DEBOUNCE);
        assert_eq!(
            ping_read_gap(Duration::from_millis(300), false),
            Duration::from_millis(900)
        );
        assert_eq!(
            ping_read_gap(Duration::from_millis(50), true),
            READ_WHILE_PLAYING
        );
        assert_eq!(
            ping_read_gap(Duration::from_millis(500), true),
            Duration::from_millis(1500)
        );
        let dir = std::env::temp_dir().join(format!("tab-ping-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let update = |_| {};
        let (mut worker, _jobs, _done) =
            recovery_worker(FakeEngine::from_dir(&dir).unwrap(), &update);
        let now = Instant::now();
        worker.dirty = Some(now - DEBOUNCE);
        assert!(worker.ping_due(now));
        // Just read at a cost of 200 ms: the ping waits 600 ms, a second
        // while the video's screencast runs.
        worker.last_read = Some((now, Duration::from_millis(200)));
        assert!(!worker.ping_due(now + Duration::from_millis(500)));
        assert!(worker.ping_due(now + Duration::from_millis(600)));
        worker.frames.on = true;
        assert!(!worker.ping_due(now + Duration::from_millis(900)));
        assert!(worker.ping_due(now + READ_WHILE_PLAYING));
    }

    #[test]
    fn video_frames_go_on_while_a_pinged_reading_waits() {
        let dir = std::env::temp_dir().join(format!("tab-ping-video-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let update = |_| {};
        let (mut worker, _jobs, _done) =
            recovery_worker(FakeEngine::from_dir(&dir).unwrap(), &update);
        assert!(!worker.video_may_run(false), "no model yet");
        worker.page = Some(Arc::new(Page::default()));
        assert!(worker.video_may_run(false));
        // The page pinged a change (a player's clock): frames go on.
        worker.dirty = Some(Instant::now());
        assert!(worker.video_may_run(false));
        // A reading asked for (navigation, an action) or refused: none.
        assert!(!worker.video_may_run(true));
        worker.refused = true;
        assert!(!worker.video_may_run(false));
        worker.refused = false;
        worker.pixels = true;
        assert!(!worker.video_may_run(false));
    }

    #[test]
    fn ended_tabs_leave_the_registry() {
        let dir = std::env::temp_dir().join(format!("noctiluca-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let spawn = || Tab::spawn(Box::new(FakeEngine::from_dir(&dir).unwrap()), |_| {});
        // Fifty tabs made and dropped: each thread ends once its tab goes.
        for _ in 0..50 {
            drop(spawn());
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let live = spawn();
            let ended = RUNNING
                .lock()
                .unwrap()
                .iter()
                .filter(|r| matches!(r.ended.try_recv(), Err(TryRecvError::Disconnected)))
                .count();
            drop(live);
            // The new spawn pruned every tab that had ended by then; give
            // the rest time to end, and look again.
            if ended == 0 && RUNNING.lock().unwrap().len() <= 8 {
                break;
            }
            assert!(Instant::now() < deadline, "ended tabs stay registered");
            thread::sleep(Duration::from_millis(20));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

//! The engine trait: what Noctiluca needs from a browser, so the browser
//! behind it can change (decision 1 of `docs/design/browser.md`). Its
//! first backend is [`ChromeBackend`](crate::cdp::ChromeBackend), over
//! rs-rich-embed's `ChromeEngine`; tests use their own.

use std::io;

use rich_embed::{PageState, ScreencastFrame};
use serde_json::Value;

/// A tab. Only one for now: rs-rich-embed drives one page target.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TabId(pub u32);

/// A step through a tab's history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryStep {
    Back,
    Forward,
    Reload,
}

/// A rectangle in CSS pixels of the document.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }
}

/// An image format for [`Engine::capture`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Png,
    Jpeg,
}

/// Pixels of a region of the page, encoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub format: Format,
    pub bytes: Vec<u8>,
}

/// The page as laid out, as the engine reports it: for the CDP backend, a
/// `DOMSnapshot.captureSnapshot` and an `Accessibility.getFullAXTree`.
/// Untrusted: the [model](crate::model) parses it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PageSnapshot {
    pub dom: Value,
    pub ax: Value,
}

/// A node of the page, by the engine's stable id for it (a CDP
/// `backendNodeId`).
pub type NodeId = i64;

/// A media element's playback, read from the element itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Playback {
    pub paused: bool,
    pub muted: bool,
    /// 0 to 1.
    pub volume: f64,
    /// Seconds.
    pub time: f64,
    /// Seconds; `None` while unknown, or for a live stream.
    pub duration: Option<f64>,
    pub ended: bool,
    /// The element's `readyState`, 0 to 4: below 3 a playing element
    /// waits for data.
    pub ready: u8,
    /// Whether the element failed to load its media (`error` is set).
    pub failed: bool,
}

impl Playback {
    /// What it is doing, as the status line says it: `PLAYING`, `PAUSED`,
    /// `ENDED`, `BUFFERING` or `ERROR`.
    pub fn label(&self) -> &'static str {
        if self.failed {
            "ERROR"
        } else if self.ended {
            "ENDED"
        } else if self.paused {
            "PAUSED"
        } else if self.ready < 3 {
            "BUFFERING"
        } else {
            "PLAYING"
        }
    }

    /// Whether it is playing: not paused, not ended, not failed.
    pub fn playing(&self) -> bool {
        !self.paused && !self.ended && !self.failed
    }
}

/// What to do to a media element.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MediaOp {
    Play,
    Pause,
    Mute(bool),
    /// 0 to 1.
    Volume(f64),
    /// Move this many seconds on (back when negative), kept within what
    /// can be sought. Paused stays paused.
    Seek(f64),
}

/// Something done to the page, as real input where the page can tell.
#[derive(Clone, Debug, PartialEq)]
pub enum PageAction {
    /// A real mouse click at the middle of the node, scrolled into view.
    Click(NodeId),
    /// Move the page's focus to the node.
    Focus(NodeId),
    /// Type `text` into what has the focus (`Input.insertText`).
    InsertText(String),
    /// Replace a field's value: focus it, select what it holds, type the
    /// new value; an empty value is deleted. `input` and `change` fire.
    SetValue { node: NodeId, value: String },
    /// Choose one option of a select, by the option's own node: two
    /// options with one value stay apart. Refused if it is disabled.
    SelectOption { select: NodeId, option: NodeId },
    /// Set a range input's number, firing `input` and `change`.
    SetRange { node: NodeId, value: f64 },
    /// Scroll the page so this many CSS pixels are above the viewport (a
    /// jump: Home, End, find). `f64::MAX` is the bottom.
    ScrollTo(f64),
    /// Turn the mouse wheel by this many CSS pixels down (up when
    /// negative), as a user would: the page sees real scrolling, so its
    /// lazy images load and its infinite feed's sentinels fire.
    Wheel(f64),
    /// Play, pause, mute or set the volume of a video or audio element,
    /// as the user's own gesture.
    Media { node: NodeId, op: MediaOp },
}

/// Where the page is scrolled and how tall it is, in CSS pixels: for the
/// CDP backend, `Page.getLayoutMetrics`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Metrics {
    /// The pixels above the viewport.
    pub scroll_y: f64,
    /// The document's height.
    pub content_height: f64,
    /// The viewport's height.
    pub viewport_height: f64,
}

/// A live log's entries, as the engine read them without a snapshot.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveRead {
    /// Its entries' keys, oldest first (the newest [`MAX_ENTRIES`] at
    /// most).
    ///
    /// [`MAX_ENTRIES`]: crate::model::MAX_ENTRIES
    pub order: Vec<u64>,
    /// The entries that are new or changed since the last read of this
    /// log (all of them when the read was `full`): each key and its runs,
    /// untrusted, for [`crate::model::live_runs`].
    pub fresh: Vec<(u64, Value)>,
    /// The first entry Chrome shows in the box, at its own scroll.
    pub first_shown: Option<u64>,
}

/// What happened in the engine since it was last asked.
#[derive(Clone, Debug, PartialEq)]
pub enum EngineEvent {
    /// Something on the page changed: the isolated world's ping.
    Changed,
    /// Only live logs the tab has read changed (entries appended, edited
    /// or removed): their entries are read again, not the whole page.
    Live,
    /// The main frame went to a new document.
    Navigated(String),
    /// The page finished loading.
    Loaded,
    /// A download began; it waits in the quarantine for the user.
    DownloadBegan {
        guid: String,
        name: String,
        url: String,
    },
    /// A download finished in the quarantine.
    DownloadDone { guid: String, path: String },
}

/// A browser engine behind Noctiluca.
pub trait Engine: Send {
    /// Lay pages out at `columns` x `rows` cells.
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()>;
    fn open(&mut self, tab: TabId, url: &str) -> io::Result<()>;
    /// A new tab; backends with one page refuse it.
    fn new_tab(&mut self, _url: &str) -> io::Result<TabId> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this engine has one tab",
        ))
    }
    fn close_tab(&mut self, _tab: TabId) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "this engine has one tab",
        ))
    }
    fn history(&mut self, tab: TabId, step: HistoryStep) -> io::Result<()>;
    /// The tab's address, title, loading state and history.
    fn state(&self, tab: TabId) -> PageState;
    /// The page as laid out, with its accessibility tree.
    fn snapshot(&mut self, tab: TabId) -> io::Result<PageSnapshot>;
    /// Pixels of `region`, in CSS pixels of the document.
    fn capture(&mut self, tab: TabId, region: Rect, format: Format) -> io::Result<Frame>;
    fn act(&mut self, tab: TabId, action: &PageAction) -> io::Result<()>;
    /// A video or audio element's playback now.
    fn playback(&mut self, _tab: TabId, _node: NodeId) -> io::Result<Playback> {
        Err(io::Error::new(io::ErrorKind::Unsupported, "no media here"))
    }
    /// The bytes of a resource the page loaded (an image's original file),
    /// from the browser's cache: no new request. Fails for what the page
    /// did not load.
    fn resource(&mut self, _tab: TabId, _url: &str) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no resources here",
        ))
    }
    /// Whether an image element has its picture yet: a lazy image below
    /// the fold has none until the page scrolls near it. A still of one
    /// that has none would be blank.
    fn image_ready(&mut self, _tab: TabId, _node: NodeId) -> io::Result<bool> {
        Ok(true)
    }
    /// Where the page is scrolled and how tall it is now.
    fn metrics(&mut self, _tab: TabId) -> io::Result<Metrics> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no metrics here",
        ))
    }
    /// Start or stop the page's screencast: frames of the viewport as it
    /// paints, for media that moves. Only while something plays.
    fn set_screencast(&mut self, _tab: TabId, _on: bool) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no screencast here",
        ))
    }
    /// The newest screencast frame since the last call, with where it was
    /// taken. Taking it lets the browser send the next.
    fn screencast_frame(&mut self, _tab: TabId) -> Option<ScreencastFrame> {
        None
    }
    /// What the browser says of each media type: `canPlayType`'s answer
    /// (`"probably"`, `"maybe"` or `""`).
    fn can_play(&mut self, _tab: TabId, types: &[&str]) -> io::Result<Vec<String>> {
        Ok(vec![String::new(); types.len()])
    }
    /// The browser's name and version, for the about page.
    fn product(&mut self, _tab: TabId) -> String {
        "unknown".into()
    }
    /// The entries of the live log on `node`, read incrementally: only
    /// the new and changed ones come with their runs, unless `full`.
    fn read_live(&mut self, _tab: TabId, _node: NodeId, _full: bool) -> io::Result<LiveRead> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no live reads here",
        ))
    }
    /// The scroll boxes the page has appended entries to: live logs too.
    fn appended(&mut self, _tab: TabId) -> io::Result<Vec<NodeId>> {
        Ok(Vec::new())
    }
    /// What happened since the last call.
    fn events(&mut self) -> Vec<EngineEvent>;
}

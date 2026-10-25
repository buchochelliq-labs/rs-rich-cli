//! A scripted [`Engine`] for tests: pages from saved snapshots, actions
//! recorded, and a hook to change the page when an action runs. No
//! browser.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use rich_embed::{PageState, ScreencastFrame};
use serde_json::Value;

use crate::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, MediaOp, Metrics, PageAction, PageSnapshot,
    Playback, Rect, TabId,
};

/// What a [`FakeEngine`] shows, shared with the test.
#[derive(Default)]
pub struct FakeState {
    /// Snapshots by name (`"reader"`); a URL opens the one whose name its
    /// last path segment starts with.
    pub pages: HashMap<String, PageSnapshot>,
    pub current: Option<String>,
    pub history: Vec<String>,
    pub at: usize,
    /// Every action the engine was asked for, in order.
    pub actions: Vec<PageAction>,
    pub opened: Vec<String>,
    pub events: Vec<EngineEvent>,
    /// Media elements' playback, by node; media ops change it.
    pub media: HashMap<i64, Playback>,
    /// The CSS pixels above the viewport: scrolling and the wheel move it,
    /// within the document.
    pub scroll_y: f64,
    /// The viewport's height in CSS pixels.
    pub viewport_height: f64,
    /// Image nodes whose picture has not loaded (lazy, below the fold).
    pub not_loaded: std::collections::HashSet<i64>,
    /// Whether the screencast is on.
    pub screencast: bool,
    /// Screencast frames waiting to be taken, oldest first: a test pushes
    /// them.
    pub frames: Vec<ScreencastFrame>,
    /// `canPlayType`'s answers by media type.
    pub codecs: HashMap<String, String>,
    /// Resources the page loaded, by address: an image's original bytes.
    pub resources: HashMap<String, Vec<u8>>,
    /// Where resources not in [`resources`](Self::resources) are read
    /// from: a fixture directory, by the address's path after its last
    /// `/fixtures/` (else its last segment). Saved snapshots name their
    /// fixtures `file:///noctiluca/fixtures/…`.
    pub resource_root: Option<std::path::PathBuf>,
}

impl FakeState {
    /// The bytes of the resource at `url`, if the test has them.
    pub fn resource(&self, url: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.resources.get(url) {
            return Some(bytes.clone());
        }
        let root = self.resource_root.as_ref()?;
        let path = url.split(['?', '#']).next().unwrap_or(url);
        let relative = match path.rfind("/fixtures/") {
            Some(at) => &path[at + "/fixtures/".len()..],
            None => path.rsplit('/').next().unwrap_or(path),
        };
        // Never above the root.
        if relative
            .split('/')
            .any(|part| part == ".." || part.is_empty())
        {
            return None;
        }
        std::fs::read(root.join(relative)).ok()
    }
}

impl FakeState {
    /// The current page's height in CSS pixels.
    pub fn content_height(&self) -> f64 {
        self.current
            .as_ref()
            .and_then(|name| self.pages.get(name))
            .and_then(|s| s.dom["documents"][0]["contentHeight"].as_f64())
            .unwrap_or(0.0)
    }
}

/// A snapshot of a feed page at `url`: a heading, then `posts` paragraphs
/// ("Post 1", "Post 2", …), each 36 CSS pixels below the last, laid out
/// as Chrome would report them, with no accessibility tree. A test grows
/// it to stand for a page whose IntersectionObserver appended more.
pub fn feed_snapshot(url: &str, posts: usize) -> PageSnapshot {
    feed_snapshot_of(url, 1, posts as i64)
}

/// [`feed_snapshot`] with posts `first` to `last`: posts before 1 stand
/// for posts the page inserted above. Each post's nodes keep their
/// `backendNodeId` whatever is around them, as Chrome's do.
pub fn feed_snapshot_of(url: &str, first: i64, last: i64) -> PageSnapshot {
    let posts = (last - first + 1).max(0) as usize;
    use serde_json::json;
    // Strings: 0 url, 1 title, then names and style values.
    let mut strings: Vec<String> = [
        url,
        "Feed",
        "#document",
        "HTML",
        "BODY",
        "H1",
        "P",
        "#text",
        "block",
        "visible",
        "rgb(0, 0, 0)",
        "rgba(0, 0, 0, 0)",
        "400",
        "normal",
        "none",
        "1",
        "700",
        "inline",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let block = json!([8, 9, 10, 11, 12, 13, 14, 13, 15]);
    let bold = json!([8, 9, 10, 11, 16, 13, 14, 13, 15]);
    let inline = json!([17, 9, 10, 11, 12, 13, 14, 13, 15]);
    let (mut parent, mut kind, mut name, mut value, mut backend) = (
        vec![-1, 0, 1],
        vec![9, 1, 1],
        vec![2, 3, 4],
        vec![-1, -1, -1],
        vec![1, 2, 3],
    );
    let height = 60.0 + 36.0 * posts as f64;
    let mut layout_node = vec![0, 1, 2];
    let mut styles = vec![json!([]), block.clone(), block.clone()];
    let mut bounds = vec![
        json!([0, 0, 800, height]),
        json!([0, 0, 800, height]),
        json!([8, 8, 784, height - 16.0]),
    ];
    let mut text = vec![-1i64, -1, -1];
    let mut push = |tag: i64, id: i64, words: String, y: f64, style: &serde_json::Value| {
        let element = parent.len() as i64;
        parent.push(2);
        kind.push(1);
        name.push(tag);
        value.push(-1);
        backend.push(id);
        let at = strings.len() as i64;
        strings.push(words);
        parent.push(element);
        kind.push(3);
        name.push(7);
        value.push(at);
        backend.push(id + 1);
        layout_node.push(element);
        styles.push(style.clone());
        bounds.push(json!([8, y, 784, 18]));
        text.push(-1);
        layout_node.push(element + 1);
        styles.push(inline.clone());
        bounds.push(json!([8, y, 60, 18]));
        text.push(at);
    };
    push(5, 10, "Feed".into(), 8.0, &bold);
    for (k, post) in (first..=last).enumerate() {
        let id = 100_000 + 2 * post;
        push(
            6,
            id,
            format!("Post {post}"),
            60.0 + 36.0 * k as f64,
            &block,
        );
    }
    let n = parent.len();
    let empty = || json!({"index": [], "value": []});
    let dom = json!({
        "documents": [{
            "documentURL": 0, "title": 1, "baseURL": 0,
            "contentWidth": 800, "contentHeight": height,
            "scrollOffsetX": 0, "scrollOffsetY": 0,
            "nodes": {
                "parentIndex": parent, "nodeType": kind, "nodeName": name,
                "nodeValue": value, "backendNodeId": backend,
                "attributes": vec![json!([]); n],
                "textValue": empty(), "inputValue": empty(),
                "inputChecked": {"index": []}, "optionSelected": {"index": []},
                "currentSourceURL": empty(),
            },
            "layout": {
                "nodeIndex": layout_node, "styles": styles, "bounds": bounds, "text": text,
            },
        }],
        "strings": strings,
    });
    PageSnapshot {
        dom,
        ax: Value::Null,
    }
}

/// What a test does when an action runs.
pub type OnAct = Box<dyn FnMut(&PageAction, &mut FakeState) + Send>;

/// A browser that is not one.
pub struct FakeEngine {
    pub state: Arc<Mutex<FakeState>>,
    on_act: Option<OnAct>,
}

/// A saved snapshot (`{"dom": …, "ax": …}`) from a JSON file.
pub fn snapshot_file(path: &std::path::Path) -> io::Result<PageSnapshot> {
    let all: Value = serde_json::from_str(&std::fs::read_to_string(path)?)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    Ok(PageSnapshot {
        dom: all["dom"].clone(),
        ax: all["ax"].clone(),
    })
}

impl FakeEngine {
    /// An engine over the snapshots in `dir` (`NAME.json` each), showing
    /// none yet. Resources come from the `fixtures` directory beside it,
    /// when there is one.
    pub fn from_dir(dir: &std::path::Path) -> io::Result<FakeEngine> {
        let mut state = FakeState::default();
        let fixtures = dir.join("..").join("fixtures");
        if fixtures.is_dir() {
            state.resource_root = Some(fixtures);
        }
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                let name = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                state.pages.insert(name, snapshot_file(&path)?);
            }
        }
        Ok(FakeEngine {
            state: Arc::new(Mutex::new(state)),
            on_act: None,
        })
    }

    /// Run `hook` on each action, to change the page as a browser would.
    #[must_use]
    pub fn on_act(
        mut self,
        hook: impl FnMut(&PageAction, &mut FakeState) + Send + 'static,
    ) -> FakeEngine {
        self.on_act = Some(Box::new(hook));
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl FakeState {
    /// Go to the page `url` names, as a new history entry.
    pub fn go(&mut self, url: &str) {
        let name = url
            .rsplit('/')
            .next()
            .unwrap_or(url)
            .split(['.', '?', '#'])
            .next()
            .unwrap_or_default()
            .to_string();
        self.history
            .truncate(self.at + usize::from(!self.history.is_empty()));
        self.history.push(name.clone());
        self.at = self.history.len() - 1;
        self.current = Some(name);
        self.events.push(EngineEvent::Navigated(url.to_string()));
    }
}

/// What a click does when a test says nothing else: a link goes where
/// it points, to the saved page its address's last segment names.
pub fn follow(action: &PageAction, state: &mut FakeState) {
    let PageAction::Click(node) = action else {
        return;
    };
    let Some(page) = state
        .current
        .as_ref()
        .and_then(|name| state.pages.get(name))
    else {
        return;
    };
    let model = crate::model::build(page, &mut crate::model::Revisions::new());
    if let Some(href) = model.control(*node).and_then(|c| c.href.clone()) {
        let target = href.rsplit('/').next().unwrap_or_default().to_string();
        state.go(&target);
    }
}

impl Engine for FakeEngine {
    fn resize(&mut self, _columns: u16, rows: u16) -> io::Result<()> {
        self.lock().viewport_height = rows as f64 * crate::cdp::CELL.1 as f64;
        Ok(())
    }

    fn open(&mut self, _tab: TabId, url: &str) -> io::Result<()> {
        let mut state = self.lock();
        state.opened.push(url.to_string());
        state.go(url);
        Ok(())
    }

    fn history(&mut self, _tab: TabId, step: HistoryStep) -> io::Result<()> {
        let mut state = self.lock();
        match step {
            HistoryStep::Back if state.at > 0 => state.at -= 1,
            HistoryStep::Forward if state.at + 1 < state.history.len() => state.at += 1,
            _ => {}
        }
        let at = state.at;
        state.current = state.history.get(at).cloned();
        state.events.push(EngineEvent::Loaded);
        Ok(())
    }

    fn state(&self, _tab: TabId) -> PageState {
        let state = self.lock();
        let url = state
            .current
            .as_ref()
            .and_then(|name| state.pages.get(name))
            .map(|s| {
                let strings = &s.dom["strings"];
                s.dom["documents"][0]["documentURL"]
                    .as_u64()
                    .and_then(|i| strings[i as usize].as_str())
                    .unwrap_or_default()
                    .to_string()
            })
            .unwrap_or_default();
        PageState {
            url,
            can_go_back: state.at > 0,
            can_go_forward: state.at + 1 < state.history.len(),
            ..PageState::default()
        }
    }

    fn snapshot(&mut self, _tab: TabId) -> io::Result<PageSnapshot> {
        let state = self.lock();
        state
            .current
            .as_ref()
            .and_then(|name| state.pages.get(name))
            .cloned()
            .ok_or_else(|| io::Error::other("no page"))
    }

    fn capture(&mut self, _tab: TabId, region: Rect, format: Format) -> io::Result<Frame> {
        // A blue picture of the region's size, as PNG.
        let (w, h) = (region.width.max(1.0) as u32, region.height.max(1.0) as u32);
        let image = image::RgbImage::from_pixel(w, h, image::Rgb([30, 90, 220]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .map_err(io::Error::other)?;
        Ok(Frame { format, bytes })
    }

    fn act(&mut self, _tab: TabId, action: &PageAction) -> io::Result<()> {
        let mut hook = self.on_act.take();
        {
            let mut state = self.lock();
            state.actions.push(action.clone());
            // Scrolling moves within the document, as a browser's does.
            let bottom = (state.content_height() - state.viewport_height).max(0.0);
            match action {
                PageAction::ScrollTo(y) => state.scroll_y = y.clamp(0.0, bottom),
                PageAction::Wheel(d) => state.scroll_y = (state.scroll_y + d).clamp(0.0, bottom),
                _ => {}
            }
            if let PageAction::Media { node, op } = action {
                if let Some(p) = state.media.get_mut(node) {
                    match op {
                        MediaOp::Play => p.paused = false,
                        MediaOp::Pause => p.paused = true,
                        MediaOp::Mute(on) => p.muted = *on,
                        MediaOp::Volume(v) => p.volume = *v,
                        MediaOp::Seek(d) => {
                            let end = p.duration.unwrap_or(p.time);
                            p.time = (p.time + d).clamp(0.0, end);
                        }
                    }
                }
                state.events.push(EngineEvent::Changed);
            }
            match hook.as_mut() {
                Some(hook) => hook(action, &mut state),
                None => follow(action, &mut state),
            }
        }
        self.on_act = hook;
        Ok(())
    }

    fn playback(&mut self, _tab: TabId, node: i64) -> io::Result<Playback> {
        self.lock()
            .media
            .get(&node)
            .copied()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no media"))
    }

    fn image_ready(&mut self, _tab: TabId, node: i64) -> io::Result<bool> {
        Ok(!self.lock().not_loaded.contains(&node))
    }

    fn resource(&mut self, _tab: TabId, url: &str) -> io::Result<Vec<u8>> {
        self.lock()
            .resource(url)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no such resource"))
    }

    fn metrics(&mut self, _tab: TabId) -> io::Result<Metrics> {
        let state = self.lock();
        Ok(Metrics {
            scroll_y: state.scroll_y,
            content_height: state.content_height(),
            viewport_height: state.viewport_height,
        })
    }

    fn set_screencast(&mut self, _tab: TabId, on: bool) -> io::Result<()> {
        self.lock().screencast = on;
        Ok(())
    }

    fn screencast_frame(&mut self, _tab: TabId) -> Option<ScreencastFrame> {
        let mut state = self.lock();
        if !state.screencast || state.frames.is_empty() {
            return None;
        }
        // The newest, as a browser that paints faster than it is asked.
        let last = state.frames.pop();
        state.frames.clear();
        last
    }

    fn can_play(&mut self, _tab: TabId, types: &[&str]) -> io::Result<Vec<String>> {
        let state = self.lock();
        Ok(types
            .iter()
            .map(|t| state.codecs.get(*t).cloned().unwrap_or_default())
            .collect())
    }

    fn events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.lock().events)
    }
}

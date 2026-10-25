//! A scripted [`Engine`] for tests: pages from saved snapshots, actions
//! recorded, and a hook to change the page when an action runs. No
//! browser.

use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};

use rich_embed::PageState;
use serde_json::Value;

use crate::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, PageAction, PageSnapshot, Rect, TabId,
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
    /// none yet.
    pub fn from_dir(dir: &std::path::Path) -> io::Result<FakeEngine> {
        let mut state = FakeState::default();
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
    fn resize(&mut self, _columns: u16, _rows: u16) -> io::Result<()> {
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
            match hook.as_mut() {
                Some(hook) => hook(action, &mut state),
                None => follow(action, &mut state),
            }
        }
        self.on_act = hook;
        Ok(())
    }

    fn events(&mut self) -> Vec<EngineEvent> {
        std::mem::take(&mut self.lock().events)
    }
}

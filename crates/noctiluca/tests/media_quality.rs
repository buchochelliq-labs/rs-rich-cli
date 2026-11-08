//! Deterministic projection baselines over saved Chrome snapshots. These do
//! not exercise terminal graphics placement or asynchronous browser reads.
use noctiluca::layout::{Line, LogScroll, Marks, log_lines, scroll_log};
use noctiluca::model::{Entry, Page, Revisions, Run, build};
use noctiluca::testing::snapshot_file;

fn page(name: &str) -> Page {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("tests/snapshots/{name}.json"));
    build(&snapshot_file(&path).unwrap(), &mut Revisions::new())
}

fn lines(lines: &[Line]) -> Vec<String> {
    lines.iter().map(Line::plain).collect()
}

#[test]
fn chat_scrollback_survives_media_updates() {
    let mut page = page("stream-with-log");
    let log = &mut page.logs[0];
    log.entries = (1..=20)
        .map(|key| Entry {
            key,
            runs: vec![Run::plain(format!("message #{key}"))],
        })
        .collect();
    let paused = scroll_log(log, LogScroll::Follow, -5);
    let mut marks = Marks::default();
    marks.logs.insert(log.node, paused);
    let before = lines(&log_lines(&page.logs[0], 40, 4, &page, &marks));
    assert_eq!(
        before,
        ["message #12", "message #13", "message #14", "message #15"]
    );
    page.logs[0].entries.push(Entry {
        key: 21,
        runs: vec![Run::plain("new :glow: :glow:")],
    });
    if let Some(media) = page.media.first_mut() {
        media.revision += 1;
    }
    assert_eq!(
        lines(&log_lines(&page.logs[0], 40, 4, &page, &marks)),
        before
    );
}

// Gate resource delivery so navigation can be queued before the old image
// is handed to the independent decode worker.
struct GatedEngine {
    inner: noctiluca::testing::FakeEngine,
    entered: std::sync::mpsc::SyncSender<()>,
    release: Option<std::sync::mpsc::Receiver<()>>,
    async_gate: bool,
    gated: bool,
}

impl noctiluca::engine::Engine for GatedEngine {
    fn resize(&mut self, c: u16, r: u16) -> std::io::Result<()> {
        self.inner.resize(c, r)
    }
    fn open(&mut self, t: noctiluca::engine::TabId, u: &str) -> std::io::Result<()> {
        self.inner.open(t, u)
    }
    fn history(
        &mut self,
        t: noctiluca::engine::TabId,
        h: noctiluca::engine::HistoryStep,
    ) -> std::io::Result<()> {
        self.inner.history(t, h)
    }
    fn state(&self, t: noctiluca::engine::TabId) -> rich_embed::PageState {
        self.inner.state(t)
    }
    fn snapshot(
        &mut self,
        t: noctiluca::engine::TabId,
    ) -> std::io::Result<noctiluca::engine::PageSnapshot> {
        self.inner.snapshot(t)
    }
    fn capture(
        &mut self,
        t: noctiluca::engine::TabId,
        r: noctiluca::engine::Rect,
        f: noctiluca::engine::Format,
    ) -> std::io::Result<noctiluca::engine::Frame> {
        self.inner.capture(t, r, f)
    }
    fn act(
        &mut self,
        t: noctiluca::engine::TabId,
        a: &noctiluca::engine::PageAction,
    ) -> std::io::Result<()> {
        self.inner.act(t, a)
    }
    fn resource(&mut self, t: noctiluca::engine::TabId, u: &str) -> std::io::Result<Vec<u8>> {
        if !self.gated {
            self.gated = true;
            self.entered.send(()).unwrap();
            self.release
                .as_ref()
                .unwrap()
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("resource gate released");
        }
        self.inner.resource(t, u)
    }
    fn optional_resources(&mut self) -> Option<noctiluca::engine::ResourceReader> {
        if !self.async_gate {
            return None;
        }
        self.gated = true;
        let entered = self.entered.clone();
        let release = self.release.take().unwrap();
        let state = self.inner.state.clone();
        let mut first = true;
        Some(Box::new(move |url| {
            if first {
                first = false;
                entered.send(()).unwrap();
                release
                    .recv_timeout(std::time::Duration::from_secs(10))
                    .unwrap();
            }
            state
                .lock()
                .unwrap()
                .resource(url)
                .ok_or_else(|| std::io::Error::other("no resource"))
        }))
    }
    fn image_ready(&mut self, t: noctiluca::engine::TabId, n: i64) -> std::io::Result<bool> {
        self.inner.image_ready(t, n)
    }
    fn events(&mut self) -> Vec<noctiluca::engine::EngineEvent> {
        self.inner.events()
    }
}

#[test]
fn navigation_queued_during_resource_load_keeps_new_document() {
    use noctiluca::tab::{Command, Tab, Update};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let engine = noctiluca::testing::FakeEngine::from_dir(&root.join("snapshots")).unwrap();
    engine.state.lock().unwrap().resource_root = Some(root.join("fixtures"));
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (tx, rx) = mpsc::channel();
    let tab = Tab::spawn(
        Box::new(GatedEngine {
            inner: engine,
            entered: entered_tx,
            release: Some(release_rx),
            async_gate: false,
            gated: false,
        }),
        move |u| {
            let _ = tx.send(u);
        },
    );
    tab.send(Command::Open("reader.html".into()));
    entered_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("old image requested");
    tab.send(Command::Open("echo.html".into()));
    release_tx.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut reached = false;
    while Instant::now() < deadline {
        if let Ok(Update::Page(p)) = rx.recv_timeout(Duration::from_millis(50))
            && p.url.contains("echo.html")
        {
            assert!(p.media.is_empty(), "new document has unexpected media");
            reached = true;
            break;
        }
    }
    assert!(reached, "new document was not published");
    // Allow the queued decoder job to publish or be discarded. This bounded
    // observation does not prove native graphics placement cleanup.
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        if let Ok(Update::Page(p)) = rx.recv_timeout(Duration::from_millis(20)) {
            assert!(p.url.contains("echo.html"), "old page returned: {}", p.url);
        }
    }
}

#[test]
fn media_lifecycle_matches_clean_render() {
    use noctiluca::app::{Options, app};
    use noctiluca::bookmarks::Bookmarks;
    use noctiluca::config::{Config, Images, View};
    use noctiluca::testing::FakeEngine;
    use rich_intuituive::Driver;
    use std::time::{Duration, Instant};
    fn wait(driver: &mut Driver, predicate: impl Fn(&Driver) -> bool) {
        let start = Instant::now();
        loop {
            driver.settle();
            if predicate(driver) {
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{}",
                driver.screen().plain().join("\n")
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let engine = FakeEngine::from_dir(&root.join("snapshots")).unwrap();
    engine.state.lock().unwrap().resource_root = Some(root.join("fixtures"));
    let options = Options {
        config: Config::default(),
        bookmarks: Bookmarks::default(),
        url: Some("reader.html".into()),
        view: Some(View::Projection),
        images: Some(Images::Ansi),
        tunnel: None,
        control: None,
    };
    let mut d = app(Box::new(engine), options).driver(80, 30);
    wait(&mut d, |d| d.screen().plain().join("\n").contains("▀▀▀▀▀"));
    let baseline = d.screen().plain();
    d.press("f1");
    wait(&mut d, |d| {
        d.screen().plain().join("\n").contains("Actions")
    });
    d.press("esc");
    d.press("j j k k");
    d.resize(100, 35);
    d.settle();
    d.resize(80, 30);
    wait(&mut d, |d| d.screen().plain()[2..28] == baseline[2..28]);
    assert_eq!(&d.screen().plain()[2..28], &baseline[2..28]);
}

#[test]
fn slow_optional_inline_load_allows_text_and_navigation_before_completion() {
    use noctiluca::tab::{Command, Tab, Update};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let engine = noctiluca::testing::FakeEngine::from_dir(&root.join("snapshots")).unwrap();
    let (entered, started) = mpsc::sync_channel(1);
    let (release, gate) = mpsc::sync_channel(1);
    let (send, receive) = mpsc::channel();
    let tab = Tab::spawn(
        Box::new(GatedEngine {
            inner: engine,
            entered,
            release: Some(gate),
            gated: false,
            async_gate: true,
        }),
        move |u| {
            let _ = send.send(u);
        },
    );
    tab.send(Command::Open("stream-with-log.html".into()));
    started
        .recv_timeout(Duration::from_secs(2))
        .expect("optional read started");
    let until = Instant::now() + Duration::from_secs(2);
    let mut text = false;
    while Instant::now() < until {
        if let Ok(Update::Page(p)) = receive.recv_timeout(Duration::from_millis(20))
            && !p.logs.is_empty()
        {
            text = true;
            break;
        }
    }
    assert!(text, "useful text was held behind optional resource");
    tab.send(Command::Open("echo.html".into()));
    let until = Instant::now() + Duration::from_secs(2);
    let mut navigated = false;
    while Instant::now() < until {
        if let Ok(Update::Page(p)) = receive.recv_timeout(Duration::from_millis(20))
            && p.url.contains("echo.html")
        {
            navigated = true;
            break;
        }
    }
    release.send(()).unwrap();
    assert!(navigated, "navigation waited for optional resource");
}

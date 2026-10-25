//! The way out stops every tab: its engine is dropped (for Chrome, the
//! browser ends and its temporary profile is removed) even while the app
//! still holds the tab, as an app's state does until the process exits.
//! A test file of its own, since `stop_all` stops every tab of the
//! process.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use noctiluca::app::{app, Options};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, View};
use noctiluca::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, PageAction, PageSnapshot, Rect, TabId,
};
use noctiluca::testing::FakeEngine;
use rich_embed::PageState;

/// The fake engine, with a folder standing for a temporary profile that
/// its `Drop` removes, as Chrome's engine does.
struct WithProfile {
    inner: FakeEngine,
    profile: PathBuf,
}

impl Drop for WithProfile {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.profile);
    }
}

impl Engine for WithProfile {
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
    fn snapshot(&mut self, tab: TabId) -> std::io::Result<PageSnapshot> {
        self.inner.snapshot(tab)
    }
    fn capture(&mut self, tab: TabId, region: Rect, format: Format) -> std::io::Result<Frame> {
        self.inner.capture(tab, region, format)
    }
    fn act(&mut self, tab: TabId, action: &PageAction) -> std::io::Result<()> {
        self.inner.act(tab, action)
    }
    fn events(&mut self) -> Vec<EngineEvent> {
        self.inner.events()
    }
}

#[test]
fn stopping_the_tabs_drops_their_engines_while_the_app_still_holds_them() {
    let profile = std::env::temp_dir().join(format!("noctiluca-stop-{}", std::process::id()));
    std::fs::create_dir_all(profile.join("Default")).unwrap();
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let engine = WithProfile {
        inner: FakeEngine::from_dir(&snapshots).unwrap(),
        profile: profile.clone(),
    };
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("second.html".into()),
            view: Some(View::Reader),
            images: None,
        },
    )
    .driver(80, 20);
    let start = Instant::now();
    while !driver.screen().plain().join("\n").contains("You followed") {
        assert!(start.elapsed() < Duration::from_secs(10), "no page");
        driver.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.press("q");
    // The app is done, but its state (and the tab in it) is still here.
    assert!(profile.exists());
    noctiluca::tab::stop_all(Duration::from_secs(5));
    assert!(!profile.exists(), "the engine was not dropped");
    drop(driver);
}

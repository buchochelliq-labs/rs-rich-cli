//! Live logs against a real browser: new entries come through the live
//! ping without a new snapshot, a sent message reads back, and a log
//! built from `<ul aria-live>` reads as one built from `<div role=log>`.
//! Runs only when `RICH_EMBED_CHROME` names a browser (as a normal user:
//! Chrome keeps its sandbox on); without it each test says it skipped.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, LiveRead, NodeId, PageAction, PageSnapshot,
    Rect, TabId,
};
use noctiluca::file_url;
use noctiluca::layout::{project, Line, Marks};
use noctiluca::model::{Entry, Page, Role};
use noctiluca::tab::{Command, Intent, Tab, Update};
use rich_embed::{DevTools, PageState};
use serde_json::json;

/// How soon a new entry must show: the isolated world's 50 ms debounce,
/// the tab's 25 ms tick and one read, with room for a slow machine.
const BUDGET: Duration = Duration::from_millis(600);

fn chrome() -> Option<PathBuf> {
    let binary = std::env::var_os("RICH_EMBED_CHROME");
    if binary.is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
    }
    binary.map(PathBuf::from)
}

fn fixture(name: &str) -> String {
    let (file, query) = name.split_once('?').unwrap_or((name, ""));
    let url = file_url(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file),
    )
    .unwrap();
    if query.is_empty() {
        url
    } else {
        format!("{url}?{query}")
    }
}

/// The Chrome backend, counting its full snapshots.
struct Counting {
    inner: ChromeBackend,
    snapshots: Arc<AtomicUsize>,
}

impl Engine for Counting {
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.inner.resize(columns, rows)
    }
    fn open(&mut self, tab: TabId, url: &str) -> io::Result<()> {
        self.inner.open(tab, url)
    }
    fn history(&mut self, tab: TabId, step: HistoryStep) -> io::Result<()> {
        self.inner.history(tab, step)
    }
    fn state(&self, tab: TabId) -> PageState {
        self.inner.state(tab)
    }
    fn snapshot(&mut self, tab: TabId) -> io::Result<PageSnapshot> {
        self.snapshots.fetch_add(1, Ordering::SeqCst);
        self.inner.snapshot(tab)
    }
    fn capture(&mut self, tab: TabId, region: Rect, format: Format) -> io::Result<Frame> {
        self.inner.capture(tab, region, format)
    }
    fn act(&mut self, tab: TabId, action: &PageAction) -> io::Result<()> {
        self.inner.act(tab, action)
    }
    fn read_live(&mut self, tab: TabId, node: NodeId, full: bool) -> io::Result<LiveRead> {
        self.inner.read_live(tab, node, full)
    }
    fn appended(&mut self, tab: TabId) -> io::Result<Vec<NodeId>> {
        self.inner.appended(tab)
    }
    fn events(&mut self) -> Vec<EngineEvent> {
        self.inner.events()
    }
}

struct Live {
    tab: Tab,
    updates: Receiver<Update>,
    page: Option<Arc<Page>>,
    devtools: DevTools,
    snapshots: Arc<AtomicUsize>,
}

impl Live {
    fn start(binary: PathBuf) -> Live {
        let inner = ChromeBackend::start(ChromeOptions {
            binary: Some(binary),
            columns: 100,
            rows: 40,
            ..ChromeOptions::default()
        })
        .unwrap();
        let devtools = inner.devtools().clone();
        let snapshots = Arc::new(AtomicUsize::new(0));
        let engine = Counting {
            inner,
            snapshots: snapshots.clone(),
        };
        let (send, updates) = mpsc::channel();
        let tab = Tab::spawn(Box::new(engine), move |u| {
            let _ = send.send(u);
        });
        Live {
            tab,
            updates,
            page: None,
            devtools,
            snapshots,
        }
    }

    /// Wait up to `limit` for a page that `ok` accepts.
    fn wait_for(
        &mut self,
        what: &str,
        limit: Duration,
        ok: impl Fn(&Page) -> bool,
    ) -> Option<Arc<Page>> {
        let start = Instant::now();
        loop {
            if let Some(page) = &self.page {
                if ok(page) {
                    return Some(page.clone());
                }
            }
            if start.elapsed() > limit {
                eprintln!("timed out waiting for {what}");
                return None;
            }
            if let Ok(Update::Page(page)) = self.updates.recv_timeout(Duration::from_millis(5)) {
                self.page = Some(page);
            }
        }
    }

    fn wait(&mut self, what: &str, ok: impl Fn(&Page) -> bool) -> Arc<Page> {
        self.wait_for(what, Duration::from_secs(30), ok)
            .unwrap_or_else(|| panic!("timed out waiting for {what}"))
    }

    fn open(&mut self, name: &str) -> Arc<Page> {
        let url = fixture(name);
        self.tab.send(Command::Open(url.clone()));
        let file = url.split('?').next().unwrap().to_string();
        self.wait(name, |p| {
            p.url.starts_with(&file) && p.logs.first().is_some_and(|l| !l.entries.is_empty())
        })
    }

    /// Run `script` in the page's own world, as the page's script would.
    fn page_script(&self, script: &str) {
        self.devtools
            .call(
                "Runtime.evaluate",
                json!({"expression": script}),
                Duration::from_secs(10),
            )
            .unwrap();
    }
}

fn entries(page: &Page) -> Vec<String> {
    page.logs[0].entries.iter().map(Entry::text).collect()
}

#[test]
fn new_entries_come_within_the_budget_without_a_full_read() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open("stream-with-log.html?quiet");
    assert_eq!(page.logs[0].label, "Chat messages");
    assert_eq!(entries(&page).len(), 6);
    // Let the first readings settle (load, stills, the first live read).
    std::thread::sleep(Duration::from_millis(1500));
    while let Ok(Update::Page(p)) = live.updates.try_recv() {
        live.page = Some(p);
    }
    let before = live.snapshots.load(Ordering::SeqCst);
    for k in 0..5 {
        let words = format!("budget check {k}");
        let sent = Instant::now();
        live.page_script(&format!("post('dee', '#ffd700', '{words} :glow:', true)"));
        let page = live
            .wait_for(&words, BUDGET * 4, |p| {
                entries(p).iter().any(|e| e.contains(&words))
            })
            .expect("the entry came");
        let took = sent.elapsed();
        assert!(took < BUDGET, "{words} took {took:?}");
        let last = page.logs[0].entries.last().unwrap();
        assert_eq!(last.text(), format!("moderator dee: {words} :glow:"));
        // The author keeps the colour the page gave it.
        assert!(last
            .runs
            .iter()
            .any(|r| r.text == "dee" && r.color.as_deref() == Some("rgb(255, 215, 0)")));
        assert!(last.runs.iter().any(|r| r
            .picture
            .as_ref()
            .is_some_and(|p| p.alt == ":glow:" && !p.decorative)));
    }
    assert_eq!(
        live.snapshots.load(Ordering::SeqCst),
        before,
        "a live entry must not take a full snapshot"
    );
}

#[test]
fn the_pages_own_timer_fills_the_log_and_it_projects_its_tail() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    live.open("stream-with-log.html");
    // The timer posts every 1.5 s; more at once, as a busy chat does.
    live.page_script("for (let i = 0; i < 30; i++) next()");
    let page = live.wait("the timer's entries", |p| entries(p).len() >= 37);
    let newest = entries(&page).last().unwrap().clone();
    let lines: Vec<String> = project(&page, 100, (8.0, 18.0), &Marks::default())
        .lines
        .iter()
        .map(Line::plain)
        .collect();
    let tail = newest.split(": ").last().unwrap();
    assert!(
        lines.iter().any(|l| l.contains(tail)),
        "{newest:?} not in {lines:#?}"
    );
    // The first entries scrolled out of the log's box.
    assert!(!lines.iter().any(|l| l.contains("#1 ") || l.ends_with("#1")));
}

#[test]
fn a_sent_message_reads_back() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open("stream-with-log.html?quiet");
    let field = page
        .controls
        .iter()
        .find(|c| c.role == Role::TextBox && c.name == "Send a message")
        .unwrap()
        .clone();
    live.tab.send(Command::Act {
        binding: field.binding(&page),
        intent: Intent::SetValue {
            value: "hello from the terminal".into(),
            base: None,
        },
    });
    let page = live.wait("the typed value", |p| {
        p.controls
            .iter()
            .any(|c| c.value.as_deref() == Some("hello from the terminal"))
    });
    let send = page
        .controls
        .iter()
        .find(|c| c.role == Role::Button && c.name == "Send")
        .unwrap();
    live.tab.send(Command::Act {
        binding: send.binding(&page),
        intent: Intent::Click,
    });
    let page = live.wait("the message in the log", |p| {
        entries(p)
            .last()
            .is_some_and(|e| e == "you: hello from the terminal")
    });
    let mine = page.logs[0].entries.last().unwrap();
    assert_eq!(mine.runs[0].color.as_deref(), Some("rgb(53, 208, 255)"));
}

#[test]
fn a_list_log_and_a_div_log_read_the_same_live() {
    let Some(binary) = chrome() else { return };
    let mut projections = Vec::new();
    for name in ["log-ul.html", "log-div.html"] {
        let mut live = Live::start(binary.clone());
        live.open(name);
        live.page_script("post('cy', 'appended live')");
        let page = live.wait("the appended entry", |p| {
            entries(p).last().is_some_and(|e| e == "cy: appended live")
        });
        projections.push((
            entries(&page),
            project(&page, 100, (8.0, 18.0), &Marks::default()).lines,
        ));
    }
    assert_eq!(projections[0].0, projections[1].0);
    assert_eq!(projections[0].1, projections[1].1);
}

#[test]
fn a_scroll_box_the_page_appends_to_is_a_log_too() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let url = fixture("echo.html");
    live.tab.send(Command::Open(url.clone()));
    live.wait("the page", |p| p.url == url);
    // A plain scroll box, no ARIA: the page appends rows to it.
    live.page_script(
        "const box = document.createElement('div'); box.id = 'feed'; \
         box.style.cssText = 'height: 72px; overflow-y: auto'; document.body.append(box); \
         window.add = t => { const r = document.createElement('div'); r.textContent = t; box.append(r); }; \
         add('row one'); add('row two');",
    );
    live.page_script("add('row three')");
    let page = live.wait("the scroll box as a log", |p| {
        p.logs
            .iter()
            .any(|l| l.entries.iter().any(|e| e.text() == "row three"))
    });
    let log = page.logs.iter().find(|l| !l.entries.is_empty()).unwrap();
    assert_eq!(log.kind, noctiluca::model::LogKind::Appended);
    // Let the readings the new box set off finish.
    std::thread::sleep(Duration::from_millis(800));
    let before = live.snapshots.load(Ordering::SeqCst);
    live.page_script("add('row four')");
    live.wait("row four", |p| {
        p.logs
            .iter()
            .any(|l| l.entries.iter().any(|e| e.text() == "row four"))
    });
    assert_eq!(live.snapshots.load(Ordering::SeqCst), before);
}

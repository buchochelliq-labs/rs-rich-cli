//! The tab against a real browser and the fixture pages. Runs only when
//! `RICH_EMBED_CHROME` names a Chrome or Chromium binary, as
//! rs-rich-embed's own browser tests do; without it each test says it
//! skipped and passes. The browser keeps its sandbox on, so these run as
//! a normal user (CI runs them as one; as root Chrome refuses to start).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};

use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::HistoryStep;
use noctiluca::file_url;
use noctiluca::model::{BlockKind, Check, Page, Role};
use noctiluca::tab::{Command, Intent, Refusal, Tab, Update};

fn chrome() -> Option<PathBuf> {
    let binary = std::env::var_os("RICH_EMBED_CHROME");
    if binary.is_none() {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
    }
    binary.map(PathBuf::from)
}

fn fixture(name: &str) -> String {
    file_url(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

struct Live {
    tab: Tab,
    updates: Receiver<Update>,
    page: Option<Arc<Page>>,
    done: Vec<(u64, Result<(), Refusal>)>,
}

impl Live {
    fn start(binary: PathBuf) -> Live {
        let backend = ChromeBackend::start(ChromeOptions {
            binary: Some(binary),
            columns: 100,
            rows: 40,
            ..ChromeOptions::default()
        })
        .unwrap();
        let (send, updates) = mpsc::channel();
        let tab = Tab::spawn(Box::new(backend), move |u| {
            let _ = send.send(u);
        });
        Live {
            tab,
            updates,
            page: None,
            done: Vec::new(),
        }
    }

    /// Wait up to 30 s for a page that `ok` accepts.
    fn wait(&mut self, what: &str, ok: impl Fn(&Page) -> bool) -> Arc<Page> {
        let start = Instant::now();
        loop {
            if let Some(page) = &self.page {
                if ok(page) {
                    return page.clone();
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "timed out waiting for {what}: {:?}",
                self.page.as_ref().map(|p| (p.url.clone(), p.text()))
            );
            match self.updates.recv_timeout(Duration::from_millis(100)) {
                Ok(Update::Page(page)) => self.page = Some(page),
                Ok(Update::Done(id, result)) => self.done.push((id, result)),
                _ => {}
            }
        }
    }

    fn result(&mut self, id: u64) -> Result<(), Refusal> {
        let start = Instant::now();
        loop {
            if let Some(at) = self.done.iter().position(|(n, _)| *n == id) {
                return self.done.remove(at).1;
            }
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "no answer to {id}"
            );
            match self.updates.recv_timeout(Duration::from_millis(100)) {
                Ok(Update::Page(page)) => self.page = Some(page),
                Ok(Update::Done(n, result)) => self.done.push((n, result)),
                _ => {}
            }
        }
    }

    fn open(&mut self, name: &str) -> Arc<Page> {
        let url = fixture(name);
        self.tab.send(Command::Open(url.clone()));
        self.wait(name, |p| p.url == url && !p.blocks.is_empty())
    }

    /// Opens a fixture with a form, and waits for its controls too: a page
    /// can arrive with its text before its controls.
    fn open_form(&mut self, name: &str) -> Arc<Page> {
        let url = fixture(name);
        self.tab.send(Command::Open(url.clone()));
        self.wait(name, |p| p.url == url && !p.controls.is_empty())
    }
}

#[test]
fn a_page_reads_links_follow_and_history_returns() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open("reader.html");
    assert_eq!(page.title, "Reader fixture");
    assert_eq!(page.blocks[0].kind, BlockKind::Heading(1));
    assert!(!page.text().contains("Hidden by"));
    // The image gets a still of Chrome's own pixels.
    let page = live.wait("a still", |p| {
        p.media.first().is_some_and(|m| m.still.is_some())
    });
    assert!(page.media[0]
        .still
        .as_ref()
        .unwrap()
        .starts_with(b"\x89PNG"));
    // Follow the first link by its binding, as a hint does.
    let link = page.controls.iter().find(|c| c.role == Role::Link).unwrap();
    let id = live.tab.send(Command::Act {
        binding: link.binding(&page),
        intent: Intent::Click,
    });
    assert_eq!(live.result(id), Ok(()));
    let second = fixture("second.html");
    live.wait("the second page", |p| {
        p.url == second && p.title == "Second page"
    });
    live.tab.send(Command::History(HistoryStep::Back));
    let start = fixture("reader.html");
    live.wait("back", |p| p.url == start);
    live.tab.send(Command::History(HistoryStep::Forward));
    live.wait("forward", |p| p.url == second);
}

#[test]
fn a_form_fills_submits_and_its_echo_reads_back() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open_form("form.html");
    let by_name = |page: &Page, name: &str| {
        page.controls
            .iter()
            .find(|c| c.name == name)
            .cloned()
            .unwrap()
    };
    let name = by_name(&page, "Name");
    let id = live.tab.send(Command::Act {
        binding: name.binding(&page),
        intent: Intent::SetValue {
            value: "Grace Hopper".into(),
            base: Some("Ada".into()),
        },
    });
    assert_eq!(live.result(id), Ok(()));
    let page = live.wait("the typed name", |p| {
        by_name(p, "Name").value.as_deref() == Some("Grace Hopper")
    });
    let news = by_name(&page, "Send news");
    let id = live.tab.send(Command::Act {
        binding: news.binding(&page),
        intent: Intent::Click,
    });
    assert_eq!(live.result(id), Ok(()));
    let page = live.wait("ticked", |p| {
        by_name(p, "Send news").checked == Some(Check::On)
    });
    // A select's option by key: the third, whose value differs from the
    // first two's shared one; the disabled one is refused.
    let mirror = by_name(&page, "Mirror");
    let off = mirror.options[1].key;
    let id = live.tab.send(Command::Act {
        binding: mirror.binding(&page),
        intent: Intent::Select { option: off },
    });
    assert!(matches!(live.result(id), Err(Refusal::Unavailable(_))));
    let second = mirror.options[2].key;
    let id = live.tab.send(Command::Act {
        binding: mirror.binding(&page),
        intent: Intent::Select { option: second },
    });
    assert_eq!(live.result(id), Ok(()));
    let page = live.wait("chosen", |p| {
        by_name(p, "Mirror").value.as_deref() == Some("Second mirror")
    });
    // The password is typed, and still never read.
    let secret = by_name(&page, "Secret");
    let id = live.tab.send(Command::Act {
        binding: secret.binding(&page),
        intent: Intent::SetValue {
            value: "s3cret".into(),
            base: None,
        },
    });
    assert_eq!(live.result(id), Ok(()));
    let page = live.wait("still masked", |p| by_name(p, "Secret").value.is_none());
    assert!(!format!("{page:?}").contains("s3cret"));
    // A disabled button refuses, and nothing is sent.
    let off = by_name(&page, "Unavailable");
    let id = live.tab.send(Command::Act {
        binding: off.binding(&page),
        intent: Intent::Click,
    });
    assert!(matches!(live.result(id), Err(Refusal::Unavailable(_))));
    let send = by_name(&page, "Send");
    let id = live.tab.send(Command::Act {
        binding: send.binding(&page),
        intent: Intent::Click,
    });
    assert_eq!(live.result(id), Ok(()));
    let page = live.wait("the echo", |p| {
        p.title == "Echo" && p.text().contains("Query: name=")
    });
    let echo = page.text();
    assert!(echo.contains("name=Grace Hopper"), "{echo}");
    assert!(echo.contains("news=yes"), "{echo}");
    assert!(echo.contains("mirror=same2"), "{echo}");
    assert!(echo.contains("size=l"), "{echo}");
}

#[test]
fn a_value_the_page_sets_without_an_event_is_seen_and_conflicts() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open_form("form.html");
    // A page can arrive mid-load without its controls, so waits look up
    // with `value` and only a settled page unwraps.
    let control = |page: &Page, name: &str| page.controls.iter().find(|c| c.name == name).cloned();
    let find = |page: &Page, name: &str| control(page, name).unwrap();
    let value = |page: &Page, name: &str| control(page, name).and_then(|c| c.value);
    let remote = find(&page, "Schedule remote update");
    let id = live.tab.send(Command::Act {
        binding: remote.binding(&page),
        intent: Intent::Click,
    });
    assert_eq!(live.result(id), Ok(()));
    // The page sets the value 300 ms later with no event; the isolated
    // world's sample pings, and the model follows.
    let page = live.wait("the remote value", |p| {
        value(p, "Name").as_deref() == Some("remote value")
    });
    // An edit based on the old value is a conflict, and is not applied.
    let name = find(&page, "Name");
    let id = live.tab.send(Command::Act {
        binding: name.binding(&page),
        intent: Intent::SetValue {
            value: "mine".into(),
            base: Some("Ada".into()),
        },
    });
    assert_eq!(
        live.result(id),
        Err(Refusal::Conflict {
            remote: "remote value".into()
        })
    );
    live.tab.send(Command::Refresh);
    let page = live.wait("unchanged", |p| {
        value(p, "Name").as_deref() == Some("remote value")
    });
    assert_eq!(find(&page, "Name").value.as_deref(), Some("remote value"));
}

#[test]
fn a_role_and_name_activate_the_right_element_or_say_why_not() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    live.open("reader.html");
    let id = live.tab.send(Command::Activate {
        role: "link".into(),
        name: "The second page".into(),
        nth: 0,
    });
    assert_eq!(live.result(id), Ok(()));
    let second = fixture("second.html");
    live.wait("followed", |p| p.url == second);
    let id = live.tab.send(Command::Activate {
        role: "button".into(),
        name: "Next page".into(),
        nth: 0,
    });
    assert_eq!(
        live.result(id),
        Err(Refusal::Unavailable(
            "No button named \"Next page\" on this page.".into()
        ))
    );
}

#[test]
fn escape_sequences_in_a_page_reach_nothing() {
    let Some(binary) = chrome() else { return };
    let mut live = Live::start(binary);
    let page = live.open("inject.html");
    let all = format!("{page:?}");
    for bad in ["\\u{1b}", "\\u{9b}", "\\u{7f}", "\\u{202e}", "\\u{7}"] {
        assert!(!all.contains(bad), "{bad} reached the model");
    }
}

#[test]
fn the_isolated_world_pings_when_the_page_changes() {
    use noctiluca::engine::{Engine, EngineEvent, TabId};
    let Some(binary) = chrome() else { return };
    let mut engine = ChromeBackend::start(ChromeOptions {
        binary: Some(binary),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    engine.open(TabId(0), &fixture("second.html")).unwrap();
    std::thread::sleep(Duration::from_secs(1));
    engine.events();
    // A change the page makes itself, with no event of ours.
    engine
        .devtools()
        .call(
            "Runtime.evaluate",
            serde_json::json!({"expression": "document.body.append('more')"}),
            Duration::from_secs(10),
        )
        .unwrap();
    let start = Instant::now();
    while !engine.events().contains(&EngineEvent::Changed) {
        assert!(start.elapsed() < Duration::from_secs(5), "no change ping");
        std::thread::sleep(Duration::from_millis(20));
    }
}

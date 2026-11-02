//! Agent control (#816) under the fake engine: the app answers its
//! control socket while intuiTUIve's driver runs it, as a script or an
//! agent would drive a running Noctiluca. Open, snapshot, click by id,
//! type into a form and submit, back, find; a read-only session; the
//! status bar; and Ctrl+G's "send to agent".
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use noctiluca::app::{Options, app};
use noctiluca::bookmarks::Bookmarks;
use noctiluca::config::{Config, View};
use noctiluca::control::ControlLink;
use noctiluca::engine::PageAction;
use noctiluca::testing::{FakeEngine, FakeState};
use rich_interact::control::{self, Request};
use rich_intuituive::Driver;
use serde_json::{Value, json};

fn engine() -> FakeEngine {
    let snapshots = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let engine = FakeEngine::from_dir(&snapshots).unwrap();
    {
        let mut state = engine.state.lock().unwrap();
        for (name, snapshot) in state.pages.iter_mut() {
            let doc = &snapshot.dom["documents"][0];
            let at = doc["documentURL"].as_u64().unwrap() as usize;
            snapshot.dom["strings"][at] = format!("https://fixture.example/{name}.html").into();
        }
    }
    engine
}

/// A private directory for one test's socket.
fn socket(name: &str) -> PathBuf {
    let base =
        std::env::temp_dir().join(format!("noctiluca-control-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    let dir = base.join("run");
    control::private_dir(&dir).unwrap();
    dir.join("noctiluca-test.sock")
}

struct Session {
    driver: Driver,
    path: PathBuf,
    state: Arc<Mutex<FakeState>>,
}

impl Session {
    fn start(name: &str, url: &str, read_only: bool) -> Session {
        let path = socket(name);
        let link = ControlLink::bind_at(&path, read_only).unwrap();
        let engine = engine();
        let state = engine.state.clone();
        let options = Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some(url.into()),
            view: Some(View::Projection),
            images: Some(noctiluca::config::Images::Ansi),
            tunnel: None,
            control: Some(link),
        };
        let driver = app(Box::new(engine), options).driver(100, 40);
        Session {
            driver,
            path,
            state,
        }
    }

    /// One request, answered while the driver runs the app.
    fn ask(&mut self, verb: &str, args: Value) -> Result<Value, String> {
        let path = self.path.clone();
        let request = Request::new(verb, args);
        let call =
            std::thread::spawn(move || control::call(&path, &request, Duration::from_secs(10)));
        let start = Instant::now();
        while !call.is_finished() {
            self.driver.settle();
            assert!(
                start.elapsed() < Duration::from_secs(15),
                "{verb}: no answer"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        call.join().unwrap().unwrap()
    }

    /// Ask `info` until `ready` says the page at `url` is read.
    fn wait_for_page(&mut self, url: &str) -> Value {
        let start = Instant::now();
        loop {
            let info = self.ask("info", json!({})).unwrap();
            if info["ready"] == true && info["url"].as_str().is_some_and(|u| u.ends_with(url)) {
                return info;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{url} never read: {info}"
            );
        }
    }

    fn snapshot(&mut self) -> Value {
        self.ask("snapshot", json!({})).unwrap()
    }

    fn actions(&self) -> Vec<PageAction> {
        self.state.lock().unwrap().actions.clone()
    }

    fn screen(&self) -> String {
        self.driver.screen().plain().join("\n")
    }
}

/// The id of the control named `name` in a snapshot.
fn id_of(shot: &Value, name: &str) -> usize {
    shot["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == name)
        .unwrap_or_else(|| panic!("no {name:?} in {shot}"))["id"]
        .as_u64()
        .unwrap() as usize
}

#[test]
fn an_agent_opens_reads_clicks_by_id_goes_back_and_finds() {
    let mut s = Session::start("drive", "reader.html", false);
    let info = s.wait_for_page("reader.html");
    assert_eq!(info["pid"], std::process::id());
    assert_eq!(info["read_only"], false);
    let shot = s.snapshot();
    let text = shot["text"].as_str().unwrap();
    assert!(text.contains("Tiny drifters of the [0]open sea"), "{text}");
    // Each control is marked with its id where it is.
    let second = id_of(&shot, "the second page");
    assert!(text.contains(&format!("[{second}]")), "{text}");
    // Reader mode too.
    let reader = s.ask("snapshot", json!({"reader": true})).unwrap();
    assert_eq!(reader["view"], "reader");
    assert!(reader["text"].as_str().unwrap().contains("Dinoflagellates"));

    // Click by id: the link goes where it points.
    s.ask("click", json!({"target": second.to_string()}))
        .unwrap();
    s.wait_for_page("second.html");
    assert!(s.screen().contains("agent:"), "{}", s.screen());
    let shot = s.snapshot();
    assert!(
        shot["text"]
            .as_str()
            .unwrap()
            .contains("You followed the link.")
    );

    // Back, then open by address.
    s.ask("back", json!({})).unwrap();
    s.wait_for_page("reader.html");
    s.ask("open", json!({"url": "second.html"})).unwrap();
    s.wait_for_page("second.html");
    assert!(
        s.state
            .lock()
            .unwrap()
            .opened
            .iter()
            .any(|u| u.ends_with("second.html"))
    );

    // Find, and click by name.
    let found = s.ask("find", json!({"text": "followed"})).unwrap();
    let matches = found["matches"].as_array().unwrap();
    assert_eq!(matches.len(), 1, "{found}");
    assert!(
        matches[0]["text"]
            .as_str()
            .unwrap()
            .contains("You followed the link.")
    );
    s.ask("click", json!({"target": "back to the start"}))
        .unwrap();
    s.wait_for_page("reader.html");
    // What is not there says so.
    assert!(
        s.ask("click", json!({"target": "99"}))
            .unwrap_err()
            .contains("no control 99")
    );
    assert!(
        s.ask("click", json!({"target": "nowhere"}))
            .unwrap_err()
            .contains("no control named")
    );
}

#[test]
fn an_agent_fills_a_form_and_submits_it() {
    let mut s = Session::start("form", "form.html", false);
    s.wait_for_page("form.html");
    let shot = s.snapshot();
    let name = id_of(&shot, "Name");
    assert_eq!(shot["controls"][name]["value"], "Ada");
    // A disabled control has no id in the text, and cannot be a target.
    let disabled = id_of(&shot, "Unavailable");
    assert_eq!(shot["controls"][disabled]["disabled"], true);
    assert!(
        !shot["text"]
            .as_str()
            .unwrap()
            .contains(&format!("[{disabled}]"))
    );
    assert!(
        s.ask("click", json!({"target": disabled.to_string()}))
            .is_err()
    );
    // Type into the field, then press Send.
    s.ask(
        "type",
        json!({"text": "Ada Lovelace", "into": name.to_string()}),
    )
    .unwrap();
    s.ask("click", json!({"target": "Send"})).unwrap();
    // A field is not a button: typing into the button is refused.
    assert!(
        s.ask("type", json!({"text": "x", "into": "Send"}))
            .unwrap_err()
            .contains("not a field")
    );
    let send_node = {
        let state = s.state.lock().unwrap();
        let page = noctiluca::model::build(
            &state.pages["form"],
            &mut noctiluca::model::Revisions::new(),
        );
        page.controls
            .iter()
            .find(|c| c.name == "Send")
            .unwrap()
            .node
    };
    let start = Instant::now();
    loop {
        s.driver.settle();
        let actions = s.actions();
        let typed = actions.iter().position(
            |a| matches!(a, PageAction::SetValue { value, .. } if value == "Ada Lovelace"),
        );
        let sent = actions
            .iter()
            .position(|a| *a == PageAction::Click(send_node));
        if let (Some(typed), Some(sent)) = (typed, sent) {
            assert!(typed < sent, "{actions:?}");
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{actions:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Keys and text where the focus is.
    s.ask("key", json!({"keys": "enter"})).unwrap();
    s.ask("type", json!({"text": "hi"})).unwrap();
    let start = Instant::now();
    while !s
        .actions()
        .iter()
        .any(|a| *a == PageAction::InsertText("hi".into()))
    {
        s.driver.settle();
        assert!(start.elapsed() < Duration::from_secs(10));
    }
    assert!(s.actions().iter().any(|a| matches!(a, PageAction::Key(_))));
    assert!(s.ask("key", json!({"keys": "nokey"})).is_err());
}

#[test]
fn a_read_only_session_answers_reads_and_refuses_every_change() {
    let mut s = Session::start("read-only", "reader.html", true);
    let info = s.wait_for_page("reader.html");
    assert_eq!(info["read_only"], true);
    assert!(
        s.snapshot()["text"]
            .as_str()
            .unwrap()
            .contains("Tiny drifters")
    );
    assert!(s.ask("find", json!({"text": "sea"})).is_ok());
    let opened = s.state.lock().unwrap().opened.len();
    let actions = s.actions().len();
    for (verb, args) in [
        ("open", json!({"url": "second.html"})),
        ("click", json!({"target": "0"})),
        ("type", json!({"text": "x"})),
        ("key", json!({"keys": "enter"})),
        ("scroll", json!({"to": "down"})),
        ("back", json!({})),
        ("forward", json!({})),
    ] {
        let refusal = s.ask(verb, args).unwrap_err();
        assert!(refusal.contains("read-only"), "{verb}: {refusal}");
    }
    for _ in 0..20 {
        s.driver.settle();
    }
    assert_eq!(s.state.lock().unwrap().opened.len(), opened);
    assert_eq!(s.actions().len(), actions);
}

#[test]
fn ctrl_g_sends_the_picked_element_and_its_page_to_the_clipboard() {
    let mut s = Session::start("send", "second.html", false);
    s.wait_for_page("second.html");
    s.driver.press("ctrl+g");
    s.driver.settle();
    assert!(s.screen().contains("send to agent"), "{}", s.screen());
    // The one link's hint.
    s.driver.press("a");
    s.driver.settle();
    let copies = s.driver.take_copies();
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(
        copies[0].starts_with("[Noctiluca] link \"Back to the start\" -> "),
        "{}",
        copies[0]
    );
    assert!(
        copies[0].contains("on https://fixture.example/second.html"),
        "{}",
        copies[0]
    );
    // It did not follow the link.
    assert!(
        !s.actions()
            .iter()
            .any(|a| matches!(a, PageAction::Click(_)))
    );
}

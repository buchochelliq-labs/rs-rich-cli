//! Configurable names through the real app and shared control path.
use noctiluca::{
    adapt::{Adapt, Rule},
    app::{Options, app},
    bookmarks::Bookmarks,
    config::{Config, Images, View},
    engine::{PageAction, Rect},
    testing::{FakeEngine, PageBuilder},
};
use std::time::{Duration, Instant};

#[test]
fn wrapping_is_a_configurable_adaptation_and_details_keeps_f1() {
    let rule = Rule::parse("wrap-action-labels").expect("registered adaptation");
    assert!(Adapt::default().on(rule));
    assert!(!Adapt::none().on(rule));
    let config = Config::parse("[adapt]\nwrap-action-labels = false\n").unwrap();
    assert!(!config.adapt.on(rule));
    for preset in [
        noctiluca::config::Preset::Vim,
        noctiluca::config::Preset::Plain,
    ] {
        let (keys, warnings) = noctiluca::actions::keymap(preset, &[]);
        assert!(warnings.is_empty());
        assert_eq!(
            noctiluca::actions::keys_of(&keys, "page.labels").unwrap().0,
            "shift+f1"
        );
        assert_eq!(
            noctiluca::actions::keys_of(&keys, "actions").unwrap().0,
            "f1"
        );
    }
}

fn fixture() -> (
    FakeEngine,
    std::sync::Arc<std::sync::Mutex<noctiluca::testing::FakeState>>,
    Vec<i64>,
) {
    let mut p = PageBuilder::new("https://example.test/labels.html", "Labels", 640.0, 630.0);
    p.view(0.0, 630.0);
    let dialog = p.element(
        p.body(),
        "DIV",
        &[("role", "dialog"), ("aria-modal", "true")],
        &[("position", "fixed")],
        Some(Rect::new(0.0, 0.0, 304.0, 270.0)),
    );
    p.paint(dialog, 100);
    let mut nodes = Vec::new();
    for (i, name) in [
        "A long action with complete readable final suffix ALPHA",
        "Another complete action ending in BETA",
        "Disabled choice with a complete final suffix GAMMA",
    ]
    .iter()
    .enumerate()
    {
        let attrs = if i == 2 {
            vec![("disabled", "")]
        } else {
            vec![]
        };
        let button = p.element(
            dialog,
            "BUTTON",
            &attrs,
            &[],
            Some(Rect::new(8.0, 18.0 + i as f64 * 36.0, 280.0, 30.0)),
        );
        p.text(
            button,
            name,
            Rect::new(8.0, 18.0 + i as f64 * 36.0, 280.0, 18.0),
        );
        nodes.push(button);
    }
    let model = noctiluca::model::build(&p.snapshot(), &mut noctiluca::model::Revisions::new());
    let nodes = model.controls.iter().map(|c| c.node).collect();
    let engine = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("labels".into(), p.snapshot());
    (engine, state, nodes)
}

#[test]
fn narrow_source_popup_has_complete_names_and_one_enabled_target_per_action() {
    let (engine, state, nodes) = fixture();
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("labels.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        driver.settle();
        if driver.find_text("ALPHA").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "full label missing: {:?}",
            driver.screen().plain()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(driver.find_text("BETA").is_some());
    assert!(driver.find_text("GAMMA").is_some());
    let first = format!("popup-control-{}", nodes[0]);
    let second = format!("popup-control-{}", nodes[1]);
    for _ in 0..8 {
        if driver.focused_name().as_deref() == Some(&first) {
            break;
        }
        driver.press("tab");
    }
    assert_eq!(driver.focused_name().as_deref(), Some(first.as_str()));
    driver.press("tab");
    assert_eq!(driver.focused_name().as_deref(), Some(second.as_str()));
    driver.press("tab");
    assert_eq!(
        driver.focused_name().as_deref(),
        Some(first.as_str()),
        "{:?}",
        driver.accessibility()
    );
    let focused = driver.focused();
    driver.resize(28, 24);
    driver.settle();
    assert_eq!(
        driver.focused(),
        focused,
        "resize must retain the actual stock button"
    );
    driver.press("a");
    driver.settle();
    assert_eq!(
        driver.focused_name().as_deref(),
        Some(first.as_str()),
        "mode change keeps semantic action focus"
    );
    driver.press("a");
    driver.settle();
    assert_eq!(driver.focused_name().as_deref(), Some(first.as_str()));
    driver.press("shift+f1");
    driver.settle();
    assert!(driver.find_text("Action labels").is_some());
    driver.press("esc");
    driver.settle();
    assert_eq!(driver.focused_name().as_deref(), Some(first.as_str()));
    assert!(
        !state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Key(_))),
        "local details Escape must not reach source"
    );
    driver.press("enter");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a,PageAction::Click(n) if *n==nodes[0]))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn main_projection_wraps_complete_action_faces_only_when_enabled() {
    let mut p = PageBuilder::new("https://example.test/action.html", "Action", 640.0, 630.0);
    let button = p.element(
        p.body(),
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(0.0, 0.0, 160.0, 18.0)),
    );
    p.text(
        button,
        "Read the complete action label through its final OMEGA",
        Rect::new(0.0, 0.0, 160.0, 18.0),
    );
    let page = noctiluca::model::build(&p.snapshot(), &mut noctiluca::model::Revisions::new());
    for (adapt, full) in [(Adapt::default(), true), (Adapt::none(), false)] {
        let projection = noctiluca::layout::project(
            &page,
            20,
            (8.0, 18.0),
            &noctiluca::layout::Marks {
                adapt,
                ..Default::default()
            },
        );
        let text = projection
            .lines
            .iter()
            .map(noctiluca::layout::Line::plain)
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(text.contains("OMEGA"), full, "{text}");
    }
}

#[test]
fn large_popup_can_reach_and_activate_an_action_beyond_4000_rows() {
    let mut p = PageBuilder::new("https://example.test/deep.html", "Deep", 640.0, 80000.0);
    p.view(0.0, 630.0);
    let panel = p.element(
        p.body(),
        "DIV",
        &[("role", "dialog"), ("aria-modal", "true")],
        &[("position", "fixed")],
        Some(Rect::new(0.0, 0.0, 304.0, 80000.0)),
    );
    p.paint(panel, 100);
    for i in 0..4010 {
        let row = p.element(
            panel,
            "P",
            &[],
            &[],
            Some(Rect::new(0.0, i as f64 * 18.0, 280.0, 18.0)),
        );
        p.text(
            row,
            &format!("Content row {i}"),
            Rect::new(0.0, i as f64 * 18.0, 280.0, 18.0),
        );
    }
    let action = p.element(
        panel,
        "BUTTON",
        &[],
        &[],
        Some(Rect::new(0.0, 72200.0, 280.0, 18.0)),
    );
    p.text(
        action,
        "DEEPEST-ACTION",
        Rect::new(0.0, 72200.0, 280.0, 18.0),
    );
    let model = noctiluca::model::build(&p.snapshot(), &mut noctiluca::model::Revisions::new());
    let target = model.controls[0].node;
    let engine = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    let state = engine.state.clone();
    state
        .lock()
        .unwrap()
        .pages
        .insert("deep".into(), p.snapshot());
    let mut driver = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("deep.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        driver.settle();
        if driver.find_text("Content row 0").is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    driver.press("end");
    driver.settle();
    assert!(
        driver.find_text("DEEPEST-ACTION").is_some(),
        "content past4000 was lost: {:?}",
        driver.screen().plain()
    );
    for _ in 0..4 {
        if driver.focused_name().as_deref() == Some(&format!("popup-control-{target}")) {
            break;
        }
        driver.press("tab");
    }
    driver.press("enter");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        driver.settle();
        if state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a,PageAction::Click(n) if *n==target))
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn compact_details_follow_live_names_and_retire_with_the_source_owner() {
    let (engine, state, nodes) = fixture();
    let config = Config::parse("[adapt]\nwrap-action-labels = false\n").unwrap();
    let mut driver = app(
        Box::new(engine),
        Options {
            config,
            bookmarks: Bookmarks::default(),
            url: Some("labels.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        driver.settle();
        if driver.find_text("A long action").is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        driver.find_text("ALPHA").is_none(),
        "compact mode remains dense"
    );
    let first = format!("popup-control-{}", nodes[0]);
    for _ in 0..8 {
        if driver.focused_name().as_deref() == Some(first.as_str()) {
            break;
        }
        driver.press("tab");
    }
    fn rename(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::String(s)
                if s == "A long action with complete readable final suffix ALPHA" =>
            {
                *s = "Changed action with final LIVE-SUFFIX".into()
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(rename),
            serde_json::Value::Object(o) => o.values_mut().for_each(rename),
            _ => {}
        }
    }
    {
        let mut s = state.lock().unwrap();
        let page = s.pages.get_mut("labels").unwrap();
        rename(&mut page.dom);
        rename(&mut page.ax);
        s.events.push(noctiluca::engine::EngineEvent::Changed);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        driver.settle();
        if driver.find_text("Changed action").is_some() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        driver.focused_name().as_deref(),
        Some(first.as_str()),
        "live semantic name keeps focus"
    );
    driver.press("shift+f1");
    driver.settle();
    assert!(driver.find_text("LIVE-SUFFIX").is_some());
    driver.press("end");
    driver.settle();
    assert!(
        driver.find_text("GAMMA").is_some(),
        "disabled name remains readable"
    );
    let mut closed = PageBuilder::new("https://example.test/labels.html", "Closed", 640.0, 630.0);
    let body = closed.body();
    closed.text(body, "OWNER-CLOSED", Rect::new(0.0, 0.0, 280.0, 18.0));
    {
        let mut s = state.lock().unwrap();
        s.pages.insert("labels".into(), closed.snapshot());
        s.events.push(noctiluca::engine::EngineEvent::Changed);
    }
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        driver.settle();
        if driver.find_text("OWNER-CLOSED").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "local details survived owner retirement: {:?}",
            driver.screen().plain()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(driver.find_text("Action labels").is_none());
    assert!(
        !state
            .lock()
            .unwrap()
            .actions
            .iter()
            .any(|a| matches!(a, PageAction::Click(_) | PageAction::Key(_)))
    );
}

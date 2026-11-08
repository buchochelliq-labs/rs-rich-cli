//! Opt-in fixture checks against a sandboxed Chrome and a local HTTP server.
//! Set NOCTILUCA_FIXTURE_HTTP and RICH_EMBED_CHROME to run.
use noctiluca::cdp::{ChromeBackend, ChromeOptions};
use noctiluca::engine::{Engine, TabId};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn browser(page: &str) -> Option<ChromeBackend> {
    let (Ok(base), Some(binary)) = (
        std::env::var("NOCTILUCA_FIXTURE_HTTP"),
        std::env::var_os("RICH_EMBED_CHROME"),
    ) else {
        eprintln!(
            "skipped: set NOCTILUCA_FIXTURE_HTTP and RICH_EMBED_CHROME for live fixture checks"
        );
        return None;
    };
    let url = format!("{}/{}", base.trim_end_matches('/'), page);
    let mut engine = ChromeBackend::start(ChromeOptions {
        binary: Some(binary.into()),
        columns: 100,
        rows: 40,
        ..ChromeOptions::default()
    })
    .unwrap();
    engine.open(TabId(0), &url).unwrap();
    let start = Instant::now();
    loop {
        let state = engine.state(TabId(0));
        if state.url == url && !state.loading {
            break;
        }
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "fixture did not load: {state:?}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    Some(engine)
}

fn evaluate(engine: &ChromeBackend, expression: &str) -> Value {
    let result = engine
        .devtools()
        .call(
            "Runtime.evaluate",
            json!({"expression": expression, "returnByValue": true, "awaitPromise": true}),
            Duration::from_secs(10),
        )
        .unwrap();
    assert!(result.get("exceptionDetails").is_none(), "{result}");
    result["result"]["value"].clone()
}

#[test]
fn delayed_product_images_have_distinct_geometry_transitions() {
    let Some(engine) = browser("product-grid.html?quality") else {
        return;
    };
    let values = evaluate(
        &engine,
        r#"(async () => {
      const reserved = document.getElementById('quality-reserved');
      const intrinsic = document.getElementById('quality-intrinsic');
      const size = image => { const r = image.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
      const before = [size(reserved), size(intrinsic)];
      revealQualityProducts();
      await Promise.all([reserved.decode(), intrinsic.decode()]);
      return {before, after: [size(reserved), size(intrinsic)]};
    })()"#,
    );
    eprintln!("product geometry: {values}");
    assert_eq!(
        values["before"][0], values["after"][0],
        "reserved box changed"
    );
    assert_ne!(
        values["before"][1][2], values["after"][1][2],
        "intrinsic width did not change"
    );
    assert_ne!(
        values["before"][1][3], values["after"][1][3],
        "intrinsic height did not change"
    );
    assert_eq!(values["after"][0][2], 220);
    assert_eq!(values["after"][0][3], 160);
    assert_eq!(values["after"][1][2], 320);
    assert_eq!(values["after"][1][3], 180);
}

#[test]
fn keyed_chat_edits_replace_body_and_preserve_identity() {
    let Some(engine) = browser("stream-with-log.html?quiet") else {
        return;
    };
    let values = evaluate(
        &engine,
        r#"(() => {
      mediaQualityChat.append('test-key', 'hello :glow: again :glow: end');
      const row = document.querySelector('[data-key="test-key"]');
      const author = row.querySelector('.author');
      const read = () => ({
        text: (function text(n) { return n.nodeType === Node.TEXT_NODE ? n.textContent : n.nodeType === Node.ELEMENT_NODE && n.classList.contains('emote') ? n.alt : [...n.childNodes].map(text).join(''); })(row),
        emotes: row.querySelectorAll('.emote').length,
        sameRow: row === document.querySelector('[data-key="test-key"]'),
        sameAuthor: author === row.querySelector('.author')
      });
      const initial = read();
      mediaQualityChat.edit('test-key', 'replacement');
      const plain = read();
      mediaQualityChat.edit('test-key', 'new :glow: and :glow: done');
      return [initial, plain, read()];
    })()"#,
    );
    eprintln!("chat edits: {values}");
    assert_eq!(
        values,
        json!([
          {"text":"fixture: hello :glow: again :glow: end","emotes":2,"sameRow":true,"sameAuthor":true},
          {"text":"fixture: replacement","emotes":0,"sameRow":true,"sameAuthor":true},
          {"text":"fixture: new :glow: and :glow: done","emotes":2,"sameRow":true,"sameAuthor":true}
        ])
    );
}

#[test]
fn source_pane_reflow_keeps_real_browser_draft_focus_and_action_target() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
    };
    let Some(engine) = browser("stream-with-log.html?quiet") else {
        return;
    };
    evaluate(&engine, "window.beforeAppNavigation = true");
    let devtools = engine.devtools().clone();
    let eval = |expression: &str| {
        let r = devtools
            .call(
                "Runtime.evaluate",
                json!({"expression":expression,"returnByValue":true}),
                Duration::from_secs(10),
            )
            .unwrap();
        assert!(r.get("exceptionDetails").is_none(), "{r}");
        r["result"]["value"].clone()
    };
    let url = engine.state(TabId(0)).url;
    let mut d = app(
        Box::new(engine),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some(url),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(160, 50);
    fn wait(d: &mut rich_intuituive::Driver, ok: impl Fn(&rich_intuituive::Driver) -> bool) {
        let until = Instant::now() + Duration::from_secs(15);
        loop {
            d.settle();
            if ok(d) {
                break;
            }
            assert!(Instant::now() < until, "{}", d.screen().plain().join("\n"));
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    wait(&mut d, |_| {
        eval("window.beforeAppNavigation !== true") == json!(true)
    });
    wait(&mut d, |d| {
        d.screen().plain().join("\n").contains("Lanternfish")
    });
    eval(
        "window.savedInput=document.querySelector('#message');savedInput.value='draftseed';savedInput.dispatchEvent(new Event('input',{bubbles:true}));true",
    );
    wait(&mut d, |d| d.find_text("draftseed").is_some());
    let (x, y) = d.find_text("draftseed").unwrap();
    d.click(x, y);
    wait(&mut d, |d| d.focused_name().is_some_and(|n| n != "page"));
    d.press("end");
    d.type_text(" 海の光");
    let focus = d.focused();
    for (w, h) in [(80, 24), (120, 40), (160, 50), (40, 50)] {
        d.resize(w, h);
        d.settle();
        let expected_width = u32::from(w.max(80)) * 8;
        wait(&mut d, |_| eval("innerWidth") == json!(expected_width));
        assert_eq!(
            eval(
                "document.querySelector('video').getBoundingClientRect().width > 0 && document.querySelector('video').videoWidth > 0"
            ),
            json!(true)
        );
        assert_eq!(d.screen().area().width, w);
        eval(
            "mediaQualityChat.append('adaptive-new','passive update');mediaQualityChat.edit('adaptive-new','edited update');true",
        );
        wait(&mut d, |d| {
            d.screen().plain().join("\n").contains("draftseed 海の光")
        });
        assert_eq!(d.focused(), focus, "editor focus changed at {w}");
        assert_eq!(
            eval("savedInput===document.querySelector('#message')"),
            json!(true)
        );
    }
    d.press("enter");
    wait(&mut d, |_| {
        eval("savedInput.value") == json!("draftseed 海の光")
    });
    d.press("esc");
    d.assert_focused("page");
    d.press("end");
    wait(&mut d, |d| d.find_text("[Send]").is_some());
    let (x, y) = d.find_text("[Send]").unwrap();
    d.click(x + 2, y);
    wait(&mut d, |_| {
        eval("document.querySelector('#log').textContent.includes('draftseed 海の光')")
            == json!(true)
    });
    assert_eq!(eval("savedInput.value"), json!(""));
}

#[test]
fn gauge_updates_and_fixed_action_survive_all_projection_widths() {
    use noctiluca::{
        engine::PageAction,
        layout::{Marks, control_at, project},
        model::{Revisions, build},
    };
    for fixture in ["gauge-canvas.html?quiet", "gauge-svg.html?quiet"] {
        let Some(mut engine) = browser(fixture) else {
            return;
        };
        evaluate(
            &engine,
            "window.probeHits=0;const b=document.createElement('button');b.textContent='Fixed probe';b.style='position:fixed;bottom:0;left:0;width:160px;height:24px;z-index:999';b.onclick=()=>++window.probeHits;document.body.append(b);true",
        );
        let mut revisions = Revisions::new();
        let mut original = None;
        for (w, h) in [(40, 24), (80, 24), (120, 40), (160, 50)] {
            engine.resize(w.max(80), h).unwrap();
            for (shown, semantic) in [(123.45, 67.8), (234.56, 89.1)] {
                evaluate(&engine, &format!("setGauge({shown},{semantic});true"));
                let p = build(&engine.snapshot(TabId(0)).unwrap(), &mut revisions);
                assert!(
                    p.indicators.iter().any(|i| i.value == Some(semantic)),
                    "{fixture}: {:?}",
                    p.indicators
                );
                let projection = project(&p, w, (8., 18.), &Marks::default());
                assert!(projection.lines.iter().all(|l| l.width() <= usize::from(w)));
                let c = p.find("button", "Fixed probe")[0];
                assert_eq!(*original.get_or_insert(c.node), c.node);
                let index = p.controls.iter().position(|v| v.node == c.node).unwrap();
                let docked = projection
                    .docked
                    .as_ref()
                    .expect("fixed bottom action is docked");
                assert!(
                    docked
                        .lines
                        .iter()
                        .any(|l| (0..l.width()).any(|x| control_at(l, x) == Some(index)))
                );
                assert_eq!(p.check(&c.binding(&p)).unwrap().node, c.node);
            }
            engine
                .act(TabId(0), &PageAction::Click(original.unwrap()))
                .unwrap();
        }
        assert_eq!(evaluate(&engine, "window.probeHits"), json!(4));
    }
}

#[test]
fn narrow_original_chat_is_readable_and_send_is_reachable_after_scrolling() {
    use noctiluca::{
        app::{Options, app},
        bookmarks::Bookmarks,
        config::{Config, Images, View},
        layout::{Marks, project},
        model::{Revisions, build},
        testing::FakeEngine,
    };
    let Some(mut engine) = browser("stream-with-log.html?quiet") else {
        return;
    };
    engine.resize(80, 20).unwrap();
    let snapshot = engine.snapshot(TabId(0)).unwrap();
    if let Ok(path) = std::env::var("NOCTILUCA_CAPTURE_SNAPSHOT") {
        std::fs::write(
            path,
            serde_json::to_vec(&json!({"dom":snapshot.dom,"ax":snapshot.ax})).unwrap(),
        )
        .unwrap();
    }
    let page = build(&snapshot, &mut Revisions::new());
    eprintln!("source log={:?} grids={:?}", page.logs[0].rect, page.grids);
    let p = project(&page, 40, (8., 18.), &Marks::default());
    eprintln!(
        "projected log={:?}\n{}",
        p.logs,
        p.lines
            .iter()
            .map(|l| l.plain())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let fake = FakeEngine::from_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
    )
    .unwrap();
    fake.state
        .lock()
        .unwrap()
        .pages
        .insert("stream-with-log".into(), snapshot);
    let mut d = app(
        Box::new(fake),
        Options {
            config: Config::default(),
            bookmarks: Bookmarks::default(),
            url: Some("stream-with-log.html".into()),
            view: Some(View::Projection),
            images: Some(Images::None),
            tunnel: None,
            control: None,
        },
    )
    .driver(40, 24);
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        d.settle();
        if d.find_text("Lanternfish").is_some() {
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(10));
    }
    d.press("pagedown");
    d.settle();
    let text = d.screen().plain().join("\n");
    assert!(
        text.contains("tide is coming"),
        "chat must contain readable words after PageDown:\n{text}"
    );
    d.press("G");
    d.settle();
    assert!(
        d.find_text("Send a message").is_some(),
        "{}",
        d.screen().plain().join("\n")
    );
    assert!(
        d.find_text("[Send]").is_some(),
        "{}",
        d.screen().plain().join("\n")
    );
}

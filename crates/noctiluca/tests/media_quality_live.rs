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

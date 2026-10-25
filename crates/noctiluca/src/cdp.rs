//! [`ChromeBackend`]: the [`Engine`] over rs-rich-embed's `ChromeEngine`,
//! speaking the DevTools protocol through `DevTools::call` and
//! `DevTools::listen`.
//!
//! - The page is read with `DOMSnapshot.captureSnapshot` and
//!   `Accessibility.getFullAXTree`, which the page cannot observe.
//! - One small script runs in an **isolated world** of our own
//!   ([`WORLD`]), never in the page's: a `MutationObserver`, a few event
//!   listeners and a slow sample of the form fields' values, which call a
//!   binding ([`BINDING`]) to say "something changed". The page cannot see
//!   the isolated world's globals or the binding.
//! - Clicks are real mouse events at the middle of the node's content
//!   quad. Text goes in as `Input.insertText`. Choosing a select's option
//!   and setting a range run in an isolated world, on the node itself.

use std::io::{self, ErrorKind};
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use rich_embed::{ChromeEngine, DevTools, DevToolsEvent, Downloads, PageState, WebEngine};
use serde_json::{json, Value};

use crate::engine::{
    Engine, EngineEvent, Format, Frame, HistoryStep, MediaOp, Metrics, PageAction, PageSnapshot,
    Playback, Rect, TabId,
};

/// How long one DevTools call may take.
const CALL: Duration = Duration::from_secs(30);

/// The isolated world Noctiluca's script runs in.
pub const WORLD: &str = "noctiluca";
/// The binding the script calls when something changed.
pub const BINDING: &str = "__noctilucaChanged";

/// The computed styles a snapshot asks for, in this order: the
/// [model](crate::model) reads them by position.
pub const SNAPSHOT_STYLES: &[&str] = &[
    "display",
    "visibility",
    "color",
    "background-color",
    "font-weight",
    "font-style",
    "text-decoration-line",
    "white-space",
    "opacity",
    "background-image",
];

/// The script of the isolated world. It reports only that something
/// changed, at most every 50 ms, and never what. The field sample catches
/// a value a page script sets with no event (a password's value is left
/// out of it).
pub const PING_SCRIPT: &str = r#"(function start(tries) {
  const ping = globalThis.__noctilucaChanged;
  if (globalThis.__noctilucaWatching) return;
  // The binding can arrive a moment after the world is made.
  if (typeof ping !== 'function') {
    if (tries < 100) setTimeout(() => start(tries + 1), 20);
    return;
  }
  globalThis.__noctilucaWatching = true;
  let queued = false;
  const changed = () => {
    if (queued) return;
    queued = true;
    setTimeout(() => { queued = false; try { ping(''); } catch (e) {} }, 50);
  };
  new MutationObserver(changed).observe(document,
    { subtree: true, childList: true, attributes: true, characterData: true });
  for (const type of ['input', 'change', 'load', 'toggle', 'play', 'playing', 'pause', 'ended',
                      'volumechange', 'seeked', 'loadedmetadata'])
    document.addEventListener(type, changed, true);
  let last = '';
  setInterval(() => {
    const fields = document.querySelectorAll('input,select,textarea');
    let sample = '';
    for (let i = 0; i < fields.length && i < 2000; i++) {
      const f = fields[i];
      if (f.type === 'password') continue;
      sample += (f.checked ? '1' : '0') + (f.selectedIndex ?? '') + ':' + f.value + '\u0000';
    }
    if (sample !== last) { last = sample; changed(); }
  }, 500);
  changed();
})(0)"#;

/// The CSS pixels one cell stands for: the page is laid out at the
/// terminal's size times this. Narrower than rs-rich-embed's 10 x 20,
/// so a proportional 16 px font's average glyph is about one cell, and a
/// line of Chrome's is about one row.
pub const CELL: (u32, u32) = (8, 18);

/// How the browser is started.
#[derive(Clone, Debug, Default)]
pub struct ChromeOptions {
    /// The browser to run; else the one found ([`find_chrome`]).
    pub binary: Option<PathBuf>,
    /// Let pages play sound. Off by default.
    pub audio: bool,
    /// A kept profile directory, opt-in. `None`: a temporary one.
    pub profile: Option<PathBuf>,
    /// Where downloads wait; `None` denies them.
    pub quarantine: Option<PathBuf>,
    /// Turn the page's script off.
    pub no_js: bool,
    /// The page's size in cells.
    pub columns: u16,
    pub rows: u16,
}

/// The browser to use when none is named: a system Chrome first, for its
/// codecs (decision 5), then rs-rich-embed's search (Chromium, Chrome for
/// Testing on `PATH`, Edge).
pub fn find_chrome() -> Option<PathBuf> {
    const SYSTEM: &[&str] = &[
        "/usr/bin/google-chrome-stable",
        "/usr/bin/google-chrome",
        "/opt/google/chrome/chrome",
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    SYSTEM
        .iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .or_else(ChromeEngine::find)
}

/// The [`Engine`] over a headless Chrome, sandboxed, through rs-rich-embed.
pub struct ChromeBackend {
    engine: ChromeEngine,
    devtools: DevTools,
    events: Receiver<DevToolsEvent>,
}

impl ChromeBackend {
    /// Start the browser and set up the isolated world.
    pub fn start(options: ChromeOptions) -> io::Result<ChromeBackend> {
        // Noctiluca reads the page, not its pixels: no screencast, so
        // Chrome encodes no frame and the engine decodes none.
        let mut engine = ChromeEngine::new()
            .audio(options.audio)
            .cell_pixels(CELL.0, CELL.1)
            .screencast(false);
        if let Some(binary) = options.binary.clone().or_else(find_chrome) {
            engine = engine.binary(binary);
        }
        if let Some(dir) = options.profile {
            engine = engine.profile(dir);
        }
        if let Some(dir) = options.quarantine {
            engine = engine.downloads(Downloads::Quarantine(dir));
        }
        engine.resize(options.columns.max(20), options.rows.max(5))?;
        let devtools = engine.devtools()?;
        let events = devtools.listen(&[
            "Runtime.bindingCalled",
            "Page.frameNavigated",
            "Page.loadEventFired",
            "Browser.downloadWillBegin",
            "Browser.downloadProgress",
        ]);
        // A binding's calls are reported only while the Runtime domain is
        // on (measured: without it, no ping ever arrives).
        devtools.call("Runtime.enable", json!({}), CALL)?;
        devtools.call(
            "Runtime.addBinding",
            json!({"name": BINDING, "executionContextName": WORLD}),
            CALL,
        )?;
        devtools.call(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": PING_SCRIPT, "worldName": WORLD, "runImmediately": true}),
            CALL,
        )?;
        if options.no_js {
            devtools.call(
                "Emulation.setScriptExecutionDisabled",
                json!({"value": true}),
                CALL,
            )?;
        }
        Ok(ChromeBackend {
            engine,
            devtools,
            events,
        })
    }

    /// The page's DevTools, for calls of one's own.
    pub fn devtools(&self) -> &DevTools {
        &self.devtools
    }

    fn call(&self, method: &str, params: Value) -> io::Result<Value> {
        self.devtools.call(method, params, CALL)
    }

    /// A fresh isolated world in the main frame, for one action.
    fn world(&self) -> io::Result<i64> {
        let tree = self.call("Page.getFrameTree", json!({}))?;
        let frame = tree["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("no main frame"))?
            .to_string();
        let world = self.call(
            "Page.createIsolatedWorld",
            json!({"frameId": frame, "worldName": format!("{WORLD}-act")}),
        )?;
        world["executionContextId"]
            .as_i64()
            .ok_or_else(|| io::Error::other("no isolated world"))
    }

    /// The node as an object of `context`.
    fn resolve(&self, node: i64, context: i64) -> io::Result<String> {
        let object = self.call(
            "DOM.resolveNode",
            json!({"backendNodeId": node, "executionContextId": context}),
        )?;
        object["object"]["objectId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "the node is gone"))
    }

    /// Run `function` with the node as `this`, in an isolated world, with
    /// `arguments` (CDP call arguments). A thrown error is an error.
    fn call_on(&self, node: i64, function: &str, arguments: Vec<Value>) -> io::Result<Value> {
        let context = self.world()?;
        let object = self.resolve(node, context)?;
        self.call_on_object(&object, function, arguments)
    }

    fn call_on_object(
        &self,
        object: &str,
        function: &str,
        arguments: Vec<Value>,
    ) -> io::Result<Value> {
        let result = self.call(
            "Runtime.callFunctionOn",
            // As the user's own gesture, so a play the user asked for is
            // not refused as autoplay; promises are awaited.
            json!({"objectId": object, "functionDeclaration": function,
                   "arguments": arguments, "returnByValue": true,
                   "userGesture": true, "awaitPromise": true}),
        )?;
        if let Some(details) = result.get("exceptionDetails") {
            let text = details["exception"]["description"]
                .as_str()
                .or_else(|| details["text"].as_str())
                .unwrap_or("the page refused");
            let first = text.lines().next().unwrap_or(text);
            return Err(io::Error::other(crate::clean::clean(
                first.trim_start_matches("Error: "),
            )));
        }
        Ok(result["result"]["value"].clone())
    }

    fn click(&self, node: i64) -> io::Result<()> {
        // Not every node can scroll (an inline box can); a refusal is not
        // fatal, the quads say whether it can be clicked.
        let _ = self.call("DOM.scrollIntoViewIfNeeded", json!({"backendNodeId": node}));
        let quads = self.call("DOM.getContentQuads", json!({"backendNodeId": node}))?;
        let quad = quads["quads"][0]
            .as_array()
            .filter(|q| q.len() == 8)
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "the node is not on screen"))?;
        let at = |i: usize| quad[i].as_f64().unwrap_or(0.0);
        let x = (at(0) + at(2) + at(4) + at(6)) / 4.0;
        let y = (at(1) + at(3) + at(5) + at(7)) / 4.0;
        for (kind, buttons) in [("mouseMoved", 0), ("mousePressed", 1), ("mouseReleased", 0)] {
            let mut event = json!({"type": kind, "x": x, "y": y, "modifiers": 0});
            if kind != "mouseMoved" {
                event["button"] = json!("left");
                event["clickCount"] = json!(1);
            }
            event["buttons"] = json!(buttons);
            self.call("Input.dispatchMouseEvent", event)?;
        }
        Ok(())
    }

    fn set_value(&self, node: i64, value: &str) -> io::Result<()> {
        self.call("DOM.focus", json!({"backendNodeId": node}))?;
        self.call_on(
            node,
            "function() { if (this.select) { this.select(); } else { \
               const r = document.createRange(); r.selectNodeContents(this); \
               const s = getSelection(); s.removeAllRanges(); s.addRange(r); } }",
            vec![],
        )?;
        if value.is_empty() {
            for kind in ["rawKeyDown", "keyUp"] {
                self.call(
                    "Input.dispatchKeyEvent",
                    json!({"type": kind, "key": "Delete", "windowsVirtualKeyCode": 46}),
                )?;
            }
        } else {
            self.call("Input.insertText", json!({"text": value}))?;
        }
        self.call_on(
            node,
            "function() { this.dispatchEvent(new Event('change', {bubbles: true})); }",
            vec![],
        )?;
        Ok(())
    }

    fn select_option(&self, select: i64, option: i64) -> io::Result<()> {
        let context = self.world()?;
        let select = self.resolve(select, context)?;
        let option = self.resolve(option, context)?;
        self.call_on_object(
            &option,
            "function(select) { \
               if (this.closest('select') !== select) throw new Error('That option is not in that select.'); \
               if (this.disabled || (this.parentElement && this.parentElement.disabled)) \
                 throw new Error('That option is disabled.'); \
               if (select.disabled) throw new Error('That select is disabled.'); \
               this.selected = true; \
               select.dispatchEvent(new Event('input', {bubbles: true})); \
               select.dispatchEvent(new Event('change', {bubbles: true})); }",
            vec![json!({"objectId": select})],
        )?;
        Ok(())
    }
}

/// Standard base64, as DevTools sends captures.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut buffer, mut bits) = (0u32, 0);
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\r' | b'\n' => continue,
            _ => return None,
        };
        buffer = buffer << 6 | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(out)
}

impl Engine for ChromeBackend {
    fn resize(&mut self, columns: u16, rows: u16) -> io::Result<()> {
        self.engine.resize(columns.max(20), rows.max(5))
    }

    fn open(&mut self, _tab: TabId, url: &str) -> io::Result<()> {
        self.engine.open(url)
    }

    fn history(&mut self, _tab: TabId, step: HistoryStep) -> io::Result<()> {
        match step {
            HistoryStep::Back => self.engine.back(),
            HistoryStep::Forward => self.engine.forward(),
            HistoryStep::Reload => self.engine.reload(),
        }
    }

    fn state(&self, _tab: TabId) -> PageState {
        self.engine.state()
    }

    fn snapshot(&mut self, _tab: TabId) -> io::Result<PageSnapshot> {
        let dom = self.call(
            "DOMSnapshot.captureSnapshot",
            json!({"computedStyles": SNAPSHOT_STYLES, "includeDOMRects": true,
                   "includePaintOrder": true}),
        )?;
        // The page reads without its AX tree when Chrome refuses one.
        let ax = self
            .call("Accessibility.getFullAXTree", json!({}))
            .unwrap_or(Value::Null);
        Ok(PageSnapshot { dom, ax })
    }

    fn capture(&mut self, _tab: TabId, region: Rect, format: Format) -> io::Result<Frame> {
        let result = self.call(
            "Page.captureScreenshot",
            json!({
                "format": match format { Format::Png => "png", Format::Jpeg => "jpeg" },
                "clip": {"x": region.x, "y": region.y, "width": region.width,
                         "height": region.height, "scale": 1},
                "captureBeyondViewport": true,
            }),
        )?;
        let bytes = result["data"]
            .as_str()
            .and_then(base64_decode)
            .ok_or_else(|| io::Error::other("a capture that is not base64"))?;
        Ok(Frame { format, bytes })
    }

    fn act(&mut self, _tab: TabId, action: &PageAction) -> io::Result<()> {
        match action {
            PageAction::Click(node) => self.click(*node),
            PageAction::Focus(node) => self
                .call("DOM.focus", json!({"backendNodeId": node}))
                .map(drop),
            PageAction::InsertText(text) => self
                .call("Input.insertText", json!({"text": text}))
                .map(drop),
            PageAction::SetValue { node, value } => self.set_value(*node, value),
            PageAction::SelectOption { select, option } => self.select_option(*select, *option),
            PageAction::SetRange { node, value } => self
                .call_on(
                    *node,
                    "function(v) { this.value = String(v); \
                       this.dispatchEvent(new Event('input', {bubbles: true})); \
                       this.dispatchEvent(new Event('change', {bubbles: true})); }",
                    vec![json!({"value": value})],
                )
                .map(drop),
            PageAction::Media { node, op } => {
                let (function, argument) = match op {
                    MediaOp::Play => (
                        "function() { return this.play().then(() => true); }",
                        json!(null),
                    ),
                    MediaOp::Pause => ("function() { this.pause(); }", json!(null)),
                    MediaOp::Mute(on) => ("function(on) { this.muted = on; }", json!(on)),
                    MediaOp::Volume(v) => (
                        "function(v) { this.volume = Math.min(1, Math.max(0, v)); }",
                        json!(v),
                    ),
                };
                self.call_on(*node, function, vec![json!({"value": argument})])
                    .map(drop)
            }
            PageAction::ScrollTo(y) => {
                let context = self.world()?;
                // Past the end is the end: the document's own height.
                let y = if y.is_finite() && *y < 1e12 {
                    format!("{y}")
                } else {
                    "document.scrollingElement.scrollHeight".to_string()
                };
                self.call(
                    "Runtime.evaluate",
                    json!({"expression": format!("window.scrollTo(0, {y})"),
                           "contextId": context}),
                )
                .map(drop)
            }
            PageAction::Wheel(delta) => {
                // At the viewport's top left corner, inside it whatever its
                // size: the wheel scrolls what is under the pointer, and
                // the document when nothing under it scrolls.
                self.call(
                    "Input.dispatchMouseEvent",
                    json!({"type": "mouseWheel", "x": 4, "y": 4,
                           "deltaX": 0, "deltaY": delta, "modifiers": 0}),
                )
                .map(drop)
            }
        }
    }

    fn playback(&mut self, _tab: TabId, node: i64) -> io::Result<Playback> {
        let value = self.call_on(
            node,
            "function() { return [this.paused, this.muted, this.volume, this.currentTime, \
               isFinite(this.duration) ? this.duration : -1, this.ended]; }",
            vec![],
        )?;
        let at = |i: usize| value.get(i).cloned().unwrap_or(Value::Null);
        Ok(Playback {
            paused: at(0).as_bool().unwrap_or(true),
            muted: at(1).as_bool().unwrap_or(false),
            volume: at(2).as_f64().unwrap_or(1.0),
            time: at(3).as_f64().unwrap_or(0.0),
            duration: at(4).as_f64().filter(|d| *d >= 0.0),
            ended: at(5).as_bool().unwrap_or(false),
        })
    }

    fn resource(&mut self, _tab: TabId, url: &str) -> io::Result<Vec<u8>> {
        let tree = self.call("Page.getFrameTree", json!({}))?;
        let frame = tree["frameTree"]["frame"]["id"]
            .as_str()
            .ok_or_else(|| io::Error::other("no main frame"))?
            .to_string();
        let result = self.call(
            "Page.getResourceContent",
            json!({"frameId": frame, "url": url}),
        )?;
        let content = result["content"].as_str().unwrap_or_default();
        let bytes = if result["base64Encoded"].as_bool() == Some(true) {
            base64_decode(content)
                .ok_or_else(|| io::Error::other("a resource that is not base64"))?
        } else {
            content.as_bytes().to_vec()
        };
        if bytes.is_empty() {
            return Err(io::Error::new(ErrorKind::NotFound, "an empty resource"));
        }
        Ok(bytes)
    }

    fn image_ready(&mut self, _tab: TabId, node: i64) -> io::Result<bool> {
        let value = self.call_on(
            node,
            "function() { return !(this instanceof HTMLImageElement) || \
               (this.complete && this.naturalWidth > 0); }",
            vec![],
        )?;
        Ok(value.as_bool().unwrap_or(true))
    }

    fn metrics(&mut self, _tab: TabId) -> io::Result<Metrics> {
        let metrics = self.call("Page.getLayoutMetrics", json!({}))?;
        // The `css…` sizes are CSS pixels; the older names are device
        // pixels, the same at our scale factor of one.
        let pick = |css: &str, old: &str, key: &str| {
            metrics[css][key]
                .as_f64()
                .or_else(|| metrics[old][key].as_f64())
                .unwrap_or(0.0)
        };
        Ok(Metrics {
            scroll_y: pick("cssVisualViewport", "visualViewport", "pageY"),
            content_height: pick("cssContentSize", "contentSize", "height"),
            viewport_height: pick("cssVisualViewport", "visualViewport", "clientHeight"),
        })
    }

    fn events(&mut self) -> Vec<EngineEvent> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            let params = &event.params;
            let text = |key: &str| crate::clean::clean(params[key].as_str().unwrap_or_default());
            match event.method.as_str() {
                "Runtime.bindingCalled" if params["name"] == BINDING => {
                    if out.last() != Some(&EngineEvent::Changed) {
                        out.push(EngineEvent::Changed);
                    }
                }
                "Page.frameNavigated" if params["frame"].get("parentId").is_none() => {
                    let url = params["frame"]["url"].as_str().unwrap_or_default();
                    out.push(EngineEvent::Navigated(crate::clean::clean(url)));
                }
                "Page.loadEventFired" => out.push(EngineEvent::Loaded),
                "Browser.downloadWillBegin" => out.push(EngineEvent::DownloadBegan {
                    guid: text("guid"),
                    name: text("suggestedFilename"),
                    url: text("url"),
                }),
                "Browser.downloadProgress" if params["state"] == "completed" => {
                    out.push(EngineEvent::DownloadDone {
                        guid: text("guid"),
                        path: params["filePath"].as_str().unwrap_or_default().to_string(),
                    })
                }
                _ => {}
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_decode_from_base64() {
        assert_eq!(base64_decode("aGk=").unwrap(), b"hi");
        assert_eq!(base64_decode("no!"), None);
    }

    #[test]
    fn the_isolated_script_only_pings() {
        // It calls the binding with an empty string: it never reports what
        // changed, so nothing of the page's leaves through it.
        assert!(PING_SCRIPT.contains("ping('')"));
        assert!(!PING_SCRIPT.contains("ping(sample"));
        assert!(PING_SCRIPT.contains("'password'"));
    }
}

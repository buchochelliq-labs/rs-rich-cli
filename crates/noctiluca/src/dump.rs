//! `noctiluca --dump`: a page read once and printed, for scripts and CI.
//! Every string in it comes from the model, so it is cleaned already.

use std::io;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::args::Dump;
use crate::engine::Engine;
use crate::model::{BlockKind, Check, Page};
use crate::tab::{Command, Tab, Update};

/// How long the page must stay unchanged to count as read.
const QUIET: Duration = Duration::from_millis(400);

/// Open `url` in `engine` and wait, up to `timeout`, until the page has
/// loaded and stayed unchanged for a moment; the page then.
pub fn read_page(engine: Box<dyn Engine>, url: &str, timeout: Duration) -> io::Result<Arc<Page>> {
    let (send, updates) = mpsc::channel();
    let tab = Tab::spawn(engine, move |u| {
        let _ = send.send(u);
    });
    tab.send(Command::Open(url.to_string()));
    let start = Instant::now();
    let (mut page, mut loading, mut changed) = (None::<Arc<Page>>, true, Instant::now());
    loop {
        match updates.recv_timeout(Duration::from_millis(50)) {
            Ok(Update::Page(p)) => {
                page = Some(p);
                changed = Instant::now();
            }
            Ok(Update::State(state)) => {
                loading = state.loading;
                if let Some(error) = state.error {
                    return Err(io::Error::other(crate::clean::clean(&error)));
                }
            }
            Ok(Update::Done(_, Err(refusal))) => return Err(io::Error::other(refusal.to_string())),
            _ => {}
        }
        if let Some(p) = &page {
            let real = p.url != "about:blank" && !p.url.is_empty();
            if real && !loading && changed.elapsed() >= QUIET {
                return Ok(p.clone());
            }
        }
        if start.elapsed() > timeout {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{url} did not load in time"),
            ));
        }
    }
}

/// The page as `--dump` prints it.
pub fn dump(page: &Page, how: Dump) -> String {
    match how {
        Dump::Text => page.text(),
        Dump::Ax => page
            .controls
            .iter()
            .map(|c| {
                let mut line = format!("{}\t{}", c.role.name(), c.name);
                if let Some(v) = &c.value {
                    line.push_str(&format!("\tvalue={v}"));
                }
                if let Some(check) = c.checked {
                    line.push_str(match check {
                        Check::On => "\tchecked",
                        Check::Off => "\tunchecked",
                        Check::Mixed => "\tmixed",
                    });
                }
                if c.disabled {
                    line.push_str("\tdisabled");
                }
                if let Some(href) = &c.href {
                    line.push_str(&format!("\t{href}"));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Dump::Json => serde_json::to_string_pretty(&json_of(page)).unwrap_or_default(),
    }
}

/// The model as JSON: blocks, controls and media.
pub fn json_of(page: &Page) -> Value {
    let rect = |r: &crate::engine::Rect| json!([r.x, r.y, r.width, r.height]);
    let blocks: Vec<Value> = page
        .blocks
        .iter()
        .map(|b| {
            let (kind, extra) = match &b.kind {
                BlockKind::Heading(level) => ("heading", json!({"level": level})),
                BlockKind::Paragraph => ("paragraph", json!({})),
                BlockKind::ListItem { depth, marker } => ("item", json!({"depth": depth, "marker": marker})),
                BlockKind::Code => ("code", json!({})),
                BlockKind::Table(t) => (
                    "table",
                    json!({"caption": t.caption,
                           "rows": t.rows.iter().map(|r| r.iter().map(|c| c.text()).collect::<Vec<_>>()).collect::<Vec<_>>()}),
                ),
                BlockKind::Media(i) => ("media", json!({"media": i})),
                BlockKind::Control(i) => ("control", json!({"control": i})),
                BlockKind::Log(i) => (
                    "log",
                    json!({"log": i, "label": page.logs[*i].label,
                           "entries": page.logs[*i].entries.iter().map(|e| e.text()).collect::<Vec<_>>()}),
                ),
            };
            let mut value = json!({"kind": kind, "text": b.text(), "quote": b.quote, "rect": rect(&b.rect)});
            if let (Value::Object(v), Value::Object(e)) = (&mut value, extra) {
                v.extend(e);
            }
            value
        })
        .collect();
    let controls: Vec<Value> = page
        .controls
        .iter()
        .map(|c| {
            json!({"role": c.role.name(), "name": c.name, "value": c.value,
                   "checked": c.checked.map(|k| format!("{k:?}").to_lowercase()),
                   "disabled": c.disabled, "href": c.href, "group": c.group,
                   "options": c.options.iter().map(|o| json!({"label": o.label, "disabled": o.disabled, "selected": o.selected})).collect::<Vec<_>>(),
                   "rect": rect(&c.rect)})
        })
        .collect();
    let media: Vec<Value> = page
        .media
        .iter()
        .map(|m| json!({"kind": format!("{:?}", m.kind).to_lowercase(), "alt": m.alt, "src": m.src, "rect": rect(&m.rect)}))
        .collect();
    json!({"url": page.url, "title": page.title, "blocks": blocks, "controls": controls, "media": media})
}

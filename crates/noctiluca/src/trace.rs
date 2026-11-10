//! Timings of the pipeline, for measuring it: with `NOCTILUCA_TRACE` set
//! to a file's path, each reading of the page (the snapshot's time and
//! size, the model's build), each video frame (where it came from, its
//! encode), each change of the screencast's area and, once a second, the
//! painter's counters are written to that file as JSON lines,
//! `{"t": ms, "k": kind, …}`, `t` counted from the first event. Unset,
//! nothing is measured and nothing is written: each site pays one cached
//! check. Counts and timings only, never an address or a page's text.
//! `docs/design/noctiluca-youtube-perf.md` reads these.
//!
//! The media path's stages (`{"k": "media", "stage": …}`: model reads,
//! delivery and prepared queues, encoder and inline-resource work) go here
//! too: there is one trace, and nothing is written to stderr, which the
//! terminal shows while Noctiluca owns it.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::Value;

/// The variable that names the trace file.
pub(crate) const VAR: &str = "NOCTILUCA_TRACE";

struct Sink {
    start: Instant,
    out: Mutex<BufWriter<File>>,
}

fn sink() -> Option<&'static Sink> {
    static SINK: OnceLock<Option<Sink>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var_os(VAR).filter(|p| !p.is_empty())?;
        let file = File::create(path).ok()?;
        Some(Sink {
            start: Instant::now(),
            out: Mutex::new(BufWriter::new(file)),
        })
    })
    .as_ref()
}

/// Whether a trace is being written.
pub(crate) fn on() -> bool {
    sink().is_some()
}

/// The time now when tracing, to measure a span from.
pub(crate) fn start() -> Option<Instant> {
    on().then(Instant::now)
}

/// Milliseconds since `since`, to three places; zero when not tracing.
pub(crate) fn ms(since: Option<Instant>) -> f64 {
    since.map_or(0.0, |t| (t.elapsed().as_secs_f64() * 1e6).round() / 1e3)
}

/// The line written for an event of `kind` with `fields` (an object, or
/// any value, kept as `v`) at `t` milliseconds.
fn line(kind: &str, fields: Value, t: f64) -> Value {
    let mut line = match fields {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("v".into(), other);
            map
        }
    };
    line.insert("t".into(), t.into());
    line.insert("k".into(), kind.into());
    Value::Object(line)
}

/// Write one event of `kind` with `fields`. Each line is flushed, so a
/// trace read while Noctiluca runs is whole.
pub(crate) fn event(kind: &str, fields: Value) {
    let Some(sink) = sink() else {
        return;
    };
    let t = (sink.start.elapsed().as_secs_f64() * 1e6).round() / 1e3;
    let mut out = sink.out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(out, "{}", line(kind, fields, t));
    let _ = out.flush();
}

/// Write one media-path event: `json` is an object's text (`{"stage":
/// …}`), kept whole under kind `media`; text that is not JSON is kept as
/// `v`.
pub(crate) fn media(json: &str) {
    if !on() {
        return;
    }
    let fields = serde_json::from_str(json).unwrap_or_else(|_| Value::from(json));
    event("media", fields);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_are_one_json_object_a_line_with_time_and_kind() {
        let line = line("read", serde_json::json!({"snapshot_ms": 1.5}), 12.25);
        assert_eq!(line["k"], "read");
        assert_eq!(line["t"], 12.25);
        assert_eq!(line["snapshot_ms"], 1.5);
        let bare = super::line("note", Value::from(3), 0.0);
        assert_eq!(bare["v"], 3);
        let stage: Value = serde_json::from_str(r#"{"stage":"model_read"}"#).unwrap();
        assert_eq!(super::line("media", stage, 1.0)["stage"], "model_read");
        // Unset in tests: nothing is measured.
        if std::env::var_os(VAR).is_none() {
            assert!(!on());
            assert_eq!(ms(start()), 0.0);
        }
    }

    #[test]
    fn nothing_of_the_media_path_writes_to_stderr() {
        // stderr is the terminal while Noctiluca owns it: the media path's
        // stages go to the one file trace, from here and from the library
        // crates it draws with.
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files: Vec<_> = std::fs::read_dir(root.join("src"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n != "main.rs" && n != "trace.rs")
            })
            .collect();
        files.push(root.join("../rich-art/src/picture.rs"));
        files.push(root.join("../rich-ext/src/frame/graphics.rs"));
        for file in files {
            let Ok(text) = std::fs::read_to_string(&file) else {
                continue;
            };
            let old = ["RICH_MEDIA", "_TRACE"].concat();
            assert!(!text.contains(&old), "{} reads {old}", file.display());
            assert!(
                !text.contains("eprintln!(\"RICH_MEDIA") && !text.contains("\"RICH_MEDIA {"),
                "{} prints the media trace",
                file.display()
            );
        }
    }
}

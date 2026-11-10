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
//! `RICH_MEDIA_TRACE` ([`crate::diagnostics`]) is the other trace: the
//! media path's stages, to stderr, for the qualification harness. This
//! one goes to a file, so it can be on while Noctiluca owns the terminal.

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
        // Unset in tests: nothing is measured.
        if std::env::var_os(VAR).is_none() {
            assert!(!on());
            assert_eq!(ms(start()), 0.0);
        }
    }
}

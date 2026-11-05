//! Timings of the pipeline, for measuring it: with `NOCTILUCA_TRACE` set
//! to a file's path, each reading of the page (the snapshot's time and
//! size, the model's build), each projection, each video frame (where it
//! came from, its encode) and, once a second, the painter's counters are
//! written to that file as JSON lines, `{"t": ms, "k": kind, …}`, `t`
//! counted from the first event. Unset, nothing is measured and nothing
//! is written. `docs/design/noctiluca-youtube-perf.md` reads these.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::Value;

/// The variable that names the trace file.
pub const VAR: &str = "NOCTILUCA_TRACE";

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
pub fn on() -> bool {
    sink().is_some()
}

/// The time now when tracing, to measure a span from.
pub fn start() -> Option<Instant> {
    on().then(Instant::now)
}

/// Milliseconds since `since`, to three places; zero when not tracing.
pub fn ms(since: Option<Instant>) -> f64 {
    since.map_or(0.0, |t| (t.elapsed().as_secs_f64() * 1e6).round() / 1e3)
}

/// Write one event of `kind` with `fields` (an object). Each line is
/// flushed, so a trace read while Noctiluca runs is whole.
pub fn event(kind: &str, fields: Value) {
    let Some(sink) = sink() else {
        return;
    };
    let mut line = match fields {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("v".into(), other);
            map
        }
    };
    let t = (sink.start.elapsed().as_secs_f64() * 1e6).round() / 1e3;
    line.insert("t".into(), t.into());
    line.insert("k".into(), kind.into());
    let mut out = sink.out.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(out, "{}", Value::Object(line));
    let _ = out.flush();
}

//! Building a tree or a data explorer is linear in the document, in time
//! and in memory (0.0.14 release-test audit B8): a wide array once took
//! quadratic time (each node's parent found by scanning back over every
//! earlier sibling), and a deep one memory proportional to nodes × depth
//! (a path, a JSONPath and a guide prefix kept for every node).
//!
//! The sizes are chosen so the quadratic code misses the time limit by a
//! wide margin even in an optimised build, while the linear code meets it
//! in a debug one. One test, so the process's peak memory is its own.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use rich_ext::data::{parse, Format};
use rich_interact::components::json_path;
use rich_interact::headless::{self, Script};
use rich_interact::{DataExplorer, TreeSelect};

/// The time each build gets.
const LIMIT: Duration = Duration::from_secs(10);

/// `f`'s result, failing if it takes longer than [`LIMIT`].
fn within<T: Send + 'static>(what: &str, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = mpsc::channel();
    let start = Instant::now();
    std::thread::spawn(move || {
        let _ = send.send(f());
    });
    match receive.recv_timeout(LIMIT) {
        Ok(value) => {
            eprintln!("{what}: {:?}", start.elapsed());
            value
        }
        Err(_) => panic!("{what} took longer than {LIMIT:?}: not linear"),
    }
}

/// The process's peak resident memory, in KiB (Linux only).
fn peak_kib() -> Option<usize> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

#[test]
fn trees_and_explorers_build_in_linear_time_and_memory() {
    // 100 nested arrays around 20,000 leaves: every leaf 101 levels deep.
    let depth = 100;
    let leaves = 20_000;
    let mut doc = "[".repeat(depth);
    doc.push_str(&vec!["0"; leaves].join(","));
    doc.push_str(&"]".repeat(depth));
    let node = parse(Format::Json, &doc).unwrap();
    let before = peak_kib();
    let explorer = within("nested DataExplorer::new", move || {
        DataExplorer::new("deep.json", node)
    });
    if let (Some(before), Some(after)) = (before, peak_kib()) {
        // About 12 KiB a node when each kept its path; a few hundred bytes
        // now.
        let per_node = (after.saturating_sub(before) * 1024) / (depth + leaves);
        eprintln!("nested DataExplorer::new: about {per_node} bytes a node");
        assert!(per_node < 2048, "{per_node} bytes a node: not linear");
    }
    // The deepest leaf's path still comes out right, built when asked.
    let script = Script::new().keys("end ctrl+y enter");
    let (outcome, record) = within("nested explorer, to the last leaf", move || {
        let explorer = explorer.fold_below(usize::MAX);
        headless::run(explorer, script, 80, 12)
    });
    let path = outcome.unwrap().value().unwrap();
    let expected = format!("${}[{}]", "[0]".repeat(depth - 1), leaves - 1);
    assert_eq!(json_path(&path), expected);
    assert_eq!(record.copies, [expected]);

    // A flat array of 200,000 numbers (400 KB of JSON).
    let flat = format!("[{}]", vec!["0"; 200_000].join(","));
    let node = parse(Format::Json, &flat).unwrap();
    let explorer = within("flat DataExplorer::new", move || {
        DataExplorer::new("flat.json", node)
    });
    let (outcome, _) = within("flat explorer, end and pick", move || {
        headless::run(explorer, Script::new().keys("end enter"), 80, 12)
    });
    assert_eq!(json_path(&outcome.unwrap().value().unwrap()), "$[199999]");

    // The same shape straight into a tree: a root and 200,000 children.
    let tree = within("flat TreeSelect::new", || {
        TreeSelect::<String>::new(
            "t",
            (0..=200_000usize).map(|i| (usize::from(i > 0), format!("{i}"))),
        )
    });
    assert_eq!(tree.parents()[200_000], Some(0));
}

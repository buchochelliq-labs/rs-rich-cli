//! Searching below a directory, by name or by content, its results
//! streaming into a listing Miller's columns show (Yazi's `s` and `S`).

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::clock::Instant;

use rich_ext::listing::{smart_contains, walk, Entry};

use crate::reactive::{signal, Signal};
use crate::task::{spawn_stream, Stream, Task};

/// The most results a search collects.
const LIMIT: usize = 100_000;
/// How often found results reach the app while a search goes on.
const EVERY: Duration = Duration::from_millis(50);
/// The largest file the built-in content search reads.
const READ_AT_MOST: u64 = 4 * 1024 * 1024;

/// What a [`search`] matches.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchKind {
    /// Paths whose name matches (Yazi's `s`, through `fd`).
    #[default]
    Name,
    /// Files whose content matches (Yazi's `S`, through `rg`).
    Grep,
}

/// A search running (or done) below a directory: what it has found so
/// far, as entries named by their path from the root, and whether it goes
/// on. `Copy`; made by [`search`], shown in Miller's columns with
/// [`Miller::show_search`](crate::files::Miller::show_search).
#[derive(Clone, Copy)]
pub struct Search {
    root: Signal<PathBuf>,
    query: Signal<String>,
    kind: SearchKind,
    found: Signal<Vec<Entry>>,
    running: Signal<bool>,
    error: Signal<Option<String>>,
    task: Signal<Option<Task>>,
}

/// Search below `root` for `query`, by name or by content, on a thread of
/// its own, the results reaching [`found`](Search::found) in batches while
/// it goes on: with `fd` (or `fdfind`) and `rg` where they are installed,
/// as Yazi does (each skips hidden and ignored files, and takes `query`
/// as a regular expression in smart case), else with a walk of the tree
/// of its own (hidden files left out) matching the name, or each line of
/// a file under 4 MiB, as a substring in smart case. Call it while the
/// app is built or from a handler.
///
/// ```
/// use intuituive::files::{search, SearchKind};
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-search-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("src")).unwrap();
/// std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
/// std::fs::write(dir.join("README.md"), "no code here").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     let found = search(&root, "fn main", SearchKind::Grep);
///     text(move || found.paths().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(","))
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(40, 1);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end().replace('\\', "/"), "src/main.rs");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn search(root: impl AsRef<Path>, query: &str, kind: SearchKind) -> Search {
    let root = root.as_ref().to_path_buf();
    let state = Search {
        root: signal(root.clone()),
        query: signal(query.to_string()),
        kind,
        found: signal(Vec::new()),
        running: signal(true),
        error: signal(None),
        task: signal(None),
    };
    let query = query.to_string();
    let (found, running, error) = (state.found, state.running, state.error);
    let task = spawn_stream(move |stream| {
        let mut out = Batches::new(&stream, &root, found);
        let failed = if query.is_empty() {
            None
        } else {
            match run_tool(kind, &root, &query, &mut out, &stream) {
                Some(result) => result.err(),
                None => {
                    built_in(kind, &root, &query, &mut out);
                    None
                }
            }
        };
        out.flush();
        let proxy = stream.proxy().clone();
        proxy.run(move || {
            error.set(failed);
            running.set(false);
        });
    });
    state.task.replace(Some(task));
    state
}

impl Search {
    /// What it has found so far, each entry named by its path from the
    /// root (`src/main.rs`), subscribing.
    pub fn found(&self) -> Signal<Vec<Entry>> {
        self.found
    }

    /// The paths found so far, from the root, subscribing.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.found
            .with(|f| f.iter().map(|e| PathBuf::from(&e.name)).collect())
    }

    /// Whether it goes on, subscribing.
    pub fn running(&self) -> bool {
        self.running.get()
    }

    /// What the search tool said when it failed (a bad regular
    /// expression), subscribing.
    pub fn error(&self) -> Option<String> {
        self.error.get()
    }

    /// The directory searched.
    pub fn root(&self) -> PathBuf {
        self.root.get_untracked()
    }

    /// What it looks for.
    pub fn query(&self) -> String {
        self.query.get_untracked()
    }

    pub fn kind(&self) -> SearchKind {
        self.kind
    }

    /// Stop it; what it found stays.
    pub fn cancel(&self) {
        if let Some(task) = self.task.with_untracked(Clone::clone) {
            task.cancel();
        }
    }
}

/// Results on their way to the app, a batch every 50 ms.
struct Batches<'a> {
    stream: &'a Stream,
    root: &'a Path,
    found: Signal<Vec<Entry>>,
    batch: Vec<Entry>,
    last: Instant,
    count: usize,
}

impl<'a> Batches<'a> {
    fn new(stream: &'a Stream, root: &'a Path, found: Signal<Vec<Entry>>) -> Batches<'a> {
        Batches {
            stream,
            root,
            found,
            batch: Vec::new(),
            last: Instant::now(),
            count: 0,
        }
    }

    /// Add the path `rel` (from the root); false once the search should
    /// stop: cancelled, or at its limit.
    fn push(&mut self, rel: &str) -> bool {
        if self.stream.cancelled() || self.count >= LIMIT {
            return false;
        }
        let rel = rel.trim_end_matches(['\r', '\n']);
        let rel = rel.strip_prefix("./").unwrap_or(rel);
        if rel.is_empty() {
            return true;
        }
        self.batch.push(Entry::read(self.root, rel));
        self.count += 1;
        if self.last.elapsed() >= EVERY {
            self.last = Instant::now();
            self.flush();
        }
        !self.stream.cancelled()
    }

    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.batch);
        let found = self.found;
        self.stream
            .proxy()
            .run(move || found.update(|f| f.extend(batch)));
    }
}

/// Run `fd` (or `fdfind`) or `rg`, sending each line it prints; `None`
/// when neither is installed, else whether it worked.
fn run_tool(
    kind: SearchKind,
    root: &Path,
    query: &str,
    out: &mut Batches,
    stream: &Stream,
) -> Option<Result<(), String>> {
    let commands: Vec<Command> = match kind {
        SearchKind::Name => ["fd", "fdfind"]
            .iter()
            .map(|program| {
                let mut command = Command::new(program);
                command.args(["--color=never", "--"]).arg(query).arg(".");
                command
            })
            .collect(),
        SearchKind::Grep => {
            let mut command = Command::new("rg");
            command
                .args([
                    "--files-with-matches",
                    "--smart-case",
                    "--color=never",
                    "--",
                ])
                .arg(query)
                // A path, so `rg` never reads its input instead.
                .arg(".");
            vec![command]
        }
    };
    for mut command in commands {
        command
            .current_dir(root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Some(Err(error.to_string())),
        };
        let stdout = child.stdout.take().expect("piped");
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if !out.push(&line) {
                let _ = child.kill();
                break;
            }
        }
        let mut said = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut said);
        }
        let status = child.wait();
        if stream.cancelled() {
            return Some(Ok(()));
        }
        // `rg` and `fd` say 1 for nothing found.
        return Some(match status {
            Ok(s) if s.success() || s.code() == Some(1) => Ok(()),
            Ok(s) => Err(said
                .lines()
                .find(|l| !l.trim().is_empty())
                .map_or_else(|| s.to_string(), str::to_string)),
            Err(error) => Err(error.to_string()),
        });
    }
    None
}

/// The search without `fd` or `rg`: a walk, matching names or lines.
fn built_in(kind: SearchKind, root: &Path, query: &str, out: &mut Batches) {
    for path in walk(root).hidden(false) {
        let Ok(rel) = path.strip_prefix(root) else {
            continue;
        };
        let rel = rel.to_string_lossy();
        let hit = match kind {
            SearchKind::Name => {
                let name = path.file_name().map(|n| n.to_string_lossy());
                name.is_some_and(|n| smart_contains(&n, query))
            }
            SearchKind::Grep => contains(&path, query),
        };
        if hit && !out.push(&rel) {
            return;
        }
        if out.stream.cancelled() {
            return;
        }
    }
}

/// Whether a text file under 4 MiB has `query` in a line (smart case).
fn contains(path: &Path, query: &str) -> bool {
    let small = std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= READ_AT_MOST);
    if !small {
        return false;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    if bytes.iter().take(8 * 1024).any(|b| *b == 0) {
        return false;
    }
    String::from_utf8_lossy(&bytes)
        .lines()
        .any(|line| smart_contains(line, query))
}

#[cfg(test)]
mod tests {
    use super::contains;

    #[test]
    fn the_built_in_grep_reads_text_in_smart_case() {
        let dir = std::env::temp_dir().join(format!("intuituive-grep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\nFind Me\n").unwrap();
        std::fs::write(dir.join("b.bin"), [0u8, b'f', b'i', b'n', b'd']).unwrap();
        assert!(contains(&dir.join("a.txt"), "find me"));
        assert!(!contains(&dir.join("a.txt"), "find Me"));
        assert!(!contains(&dir.join("b.bin"), "find"));
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! State an app keeps between sessions, in small files of its own:
//! [`state_dir`] (where, by platform), [`write_state`] and [`read_state`]
//! (written whole and renamed into place, so a crash never leaves half of
//! one), [`update_state`] (read, change and write under a lock, so two
//! sessions do not clobber each other), and a [`FrecencyStore`] of keys
//! ranked by how often and how lately they were used. Generic: keyed by
//! an app's name and a file name, holding bytes or strings.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::reactive::{signal, Signal};

/// Where app `app` keeps its state between sessions:
/// `$XDG_STATE_HOME/app` (else `~/.local/state/app`) on Linux and the
/// BSDs, `~/Library/Application Support/app` on macOS,
/// `%LOCALAPPDATA%\app` on Windows; `None` where there is no such place
/// (no home directory). Nothing is made until something is written.
///
/// ```
/// use intuituive::state_dir;
///
/// if let Some(dir) = state_dir("notes") {
///     assert!(dir.ends_with("notes"));
/// }
/// ```
pub fn state_dir(app: &str) -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let base = if cfg!(windows) {
        var("LOCALAPPDATA").map(PathBuf::from)
    } else {
        let home = var("HOME").map(PathBuf::from);
        if cfg!(target_os = "macos") {
            home.map(|h| h.join("Library/Application Support"))
        } else {
            var("XDG_STATE_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .or_else(|| home.map(|h| h.join(".local/state")))
        }
    };
    base.map(|dir| dir.join(app))
}

/// The file `name` in app `app`'s [`state_dir`].
pub fn state_file(app: &str, name: &str) -> Option<PathBuf> {
    state_dir(app).map(|dir| dir.join(name))
}

/// The bytes of state file `name` of app `app`; `None` when it has not
/// been written (or there is no state directory).
pub fn read_state(app: &str, name: &str) -> io::Result<Option<Vec<u8>>> {
    match state_file(app, name) {
        Some(file) => read_file(&file),
        None => Ok(None),
    }
}

/// Write `bytes` as state file `name` of app `app`, whole: to a
/// temporary file beside it, renamed over it ([`write_atomic`]). An error
/// of kind `NotFound` where there is no state directory.
pub fn write_state(app: &str, name: &str, bytes: &[u8]) -> io::Result<()> {
    write_atomic(&state_file(app, name).ok_or_else(no_state_dir)?, bytes)
}

/// Change state file `name` of app `app`: `change` gets what is there now
/// (`None` before the first write) and returns what to write, under a
/// lock another session waits for, so each session's change is made to
/// the other's (a history, a frecency store kept by two windows). What
/// was written.
pub fn update_state(
    app: &str,
    name: &str,
    change: impl FnOnce(Option<Vec<u8>>) -> Vec<u8>,
) -> io::Result<Vec<u8>> {
    update_file(&state_file(app, name).ok_or_else(no_state_dir)?, change)
}

fn no_state_dir() -> io::Error {
    io::Error::new(io::ErrorKind::NotFound, "no state directory")
}

/// `file`'s bytes, `None` if it is not there.
fn read_file(file: &Path) -> io::Result<Option<Vec<u8>>> {
    match std::fs::read(file) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Write `bytes` to `file` through a temporary file beside it, synced and
/// renamed over it, making its directory first: readers see the old file
/// or the new, never half of one.
///
/// ```
/// use intuituive::write_atomic;
///
/// let file = std::env::temp_dir().join(format!("intuituive-atomic-doc-{}", std::process::id()));
/// write_atomic(&file, b"one").unwrap();
/// write_atomic(&file, b"two").unwrap();
/// assert_eq!(std::fs::read(&file).unwrap(), b"two");
/// # std::fs::remove_file(&file).ok();
/// ```
pub fn write_atomic(file: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let name = file
        .file_name()
        .map_or_else(|| "state".into(), |n| n.to_string_lossy().into_owned());
    let temp = file.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let written = std::fs::File::create(&temp).and_then(|mut out| {
        out.write_all(bytes)?;
        out.sync_all()
    });
    match written.and_then(|_| std::fs::rename(&temp, file)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// How long [`update_file`] waits for another session's lock.
const LOCK_WAIT: Duration = Duration::from_secs(2);
/// A lock older than this was left by a session that died: taken over.
const LOCK_STALE: Duration = Duration::from_secs(10);

/// [`update_state`] on any `file`: read, change and
/// [write](write_atomic) it, holding `file.lock` meanwhile.
pub fn update_file(
    file: &Path,
    change: impl FnOnce(Option<Vec<u8>>) -> Vec<u8>,
) -> io::Result<Vec<u8>> {
    if let Some(dir) = file.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut lock_name = file.as_os_str().to_owned();
    lock_name.push(".lock");
    let lock = PathBuf::from(lock_name);
    let _held = Lock::take(&lock)?;
    let old = read_file(file)?;
    let new = change(old);
    write_atomic(file, &new)?;
    Ok(new)
}

/// A lock file, removed when dropped.
struct Lock(PathBuf);

impl Lock {
    fn take(path: &Path) -> io::Result<Lock> {
        let start = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(_) => return Ok(Lock(path.to_path_buf())),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(path)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|at| at.elapsed().ok())
                        .is_some_and(|age| age > LOCK_STALE);
                    if stale || start.elapsed() > LOCK_WAIT {
                        // Left by a session that died, or held too long:
                        // taken over.
                        let _ = std::fs::remove_file(path);
                        continue;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => return Err(error),
            }
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A key in a [`FrecencyStore`]: how often it was used (aged), and when
/// last, in seconds since 1970.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    pub key: String,
    pub rank: f64,
    pub last: u64,
}

impl Visit {
    /// Its score at `now` (zoxide's): the rank, four times over within the
    /// hour, twice within the day, halved within the week, else quartered.
    pub fn score(&self, now: u64) -> f64 {
        let age = now.saturating_sub(self.last);
        let factor = match age {
            0..3_600 => 4.0,
            3_600..86_400 => 2.0,
            86_400..604_800 => 0.5,
            _ => 0.25,
        };
        self.rank * factor
    }
}

/// How a [`FrecencyStore`] ages and caps its keys.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Limits {
    /// The most keys kept; past it the least used go.
    keep: usize,
    /// When the ranks add up to more than this, they all age.
    max_age: f64,
}

/// Keys ranked by frecency (how often and how lately each was used), as
/// zoxide ranks directories: a launcher's apps, a palette's recent
/// commands, a picker's recent files. Each [`visit`](Self::visit) adds one
/// to a key's rank and stamps the time; [`ranked`](Self::ranked) lists
/// the keys best first; the ranks age once they add up to more than
/// [`max_age`](Self::max_age), and at most [`keep`](Self::keep) keys are
/// kept. Kept in a file (read, changed and written under a lock on each
/// visit, so two sessions add up rather than clobber each other), or for
/// the session only. `Copy`; made by [`frecency_store`] or
/// [`frecency_in`]. Call those while the app is built.
#[derive(Clone, Copy)]
pub struct FrecencyStore {
    visits: Signal<Vec<Visit>>,
    file: Signal<Option<PathBuf>>,
    limits: Signal<Limits>,
}

/// A frecency store kept in `file` (made on the first visit), read now.
///
/// ```
/// use intuituive::frecency_store;
/// use intuituive::prelude::*;
///
/// let file = std::env::temp_dir().join(format!("intuituive-frecency-store-doc-{}.tsv", std::process::id()));
/// let _ = std::fs::remove_file(&file);
/// let path = file.clone();
/// let app = App::new(move || {
///     let store = frecency_store(path.clone());
///     store.visit("build");
///     store.visit("test");
///     store.visit("test");
///     label(format!("{:?}", store.ranked()))
/// });
/// assert_eq!(app.render_with(&[], 30, 1).unwrap()[0].trim_end(), r#"["test", "build"]"#);
/// // The next session reads what this one wrote.
/// let again = App::new(move || label(format!("{:?}", frecency_store(file.clone()).ranked())));
/// assert_eq!(again.render_with(&[], 30, 1).unwrap()[0].trim_end(), r#"["test", "build"]"#);
/// ```
pub fn frecency_store(file: impl Into<PathBuf>) -> FrecencyStore {
    store(Some(file.into()))
}

/// The frecency store `name` of app `app`, in its [`state_dir`]; for the
/// session only where there is none.
pub fn frecency_in(app: &str, name: &str) -> FrecencyStore {
    store(state_file(app, name))
}

/// A frecency store kept for the session only.
pub fn frecency_session() -> FrecencyStore {
    store(None)
}

fn store(file: Option<PathBuf>) -> FrecencyStore {
    let visits = file
        .as_deref()
        .and_then(|file| read_file(file).ok().flatten())
        .map(|bytes| parse(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    FrecencyStore {
        visits: signal(visits),
        file: signal(file),
        limits: signal(Limits {
            keep: 500,
            max_age: 10_000.0,
        }),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl FrecencyStore {
    /// Keep at most `keys` keys (500 by default): past it, the least used
    /// go.
    // A handle: the change is made through shared state, so the
    // returned copy is only for chaining and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn keep(self, keys: usize) -> FrecencyStore {
        self.limits.update(|l| l.keep = keys.max(1));
        self
    }

    /// Age every rank once they add up to more than `total` (10 000 by
    /// default, zoxide's `_ZO_MAXAGE`): they are scaled down to 90% of
    /// it, and a key under one visit goes.
    // A handle: the change is made through shared state, so the
    // returned copy is only for chaining and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn max_age(self, total: f64) -> FrecencyStore {
        self.limits.update(|l| l.max_age = total.max(1.0));
        self
    }

    /// Count a use of `key` now, and write the store.
    pub fn visit(&self, key: &str) {
        let (key, now) = (key.to_string(), now());
        self.change(move |visits, limits| {
            match visits.iter_mut().find(|v| v.key == key) {
                Some(visit) => {
                    visit.rank += 1.0;
                    visit.last = now;
                }
                None => visits.push(Visit {
                    key: key.clone(),
                    rank: 1.0,
                    last: now,
                }),
            }
            age(visits, limits, Some(&key));
        });
    }

    /// Forget `key` (one that is gone), and write the store.
    pub fn remove(&self, key: &str) {
        let key = key.to_string();
        self.change(move |visits, _| visits.retain(|v| v.key != key));
    }

    /// The keys, best first, subscribing.
    pub fn ranked(&self) -> Vec<String> {
        let now = now();
        self.visits.with(|visits| {
            let mut scored: Vec<(f64, &Visit)> = visits.iter().map(|v| (v.score(now), v)).collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.last.cmp(&a.1.last)));
            scored.into_iter().map(|(_, v)| v.key.clone()).collect()
        })
    }

    /// Every key kept, with its rank and last use, subscribing.
    pub fn visits(&self) -> Vec<Visit> {
        self.visits.get()
    }

    /// The file it is kept in (`None`: for the session only).
    pub fn file(&self) -> Option<PathBuf> {
        self.file.get_untracked()
    }

    /// Read the file again: what other sessions wrote since.
    pub fn reload(&self) {
        if let Some(file) = self.file.get_untracked() {
            if let Ok(Some(bytes)) = read_file(&file) {
                self.visits.set(parse(&String::from_utf8_lossy(&bytes)));
            }
        }
    }

    /// Make `change` to the store: to the file's latest (read under its
    /// lock, so another session's visits are kept), else to the session's.
    fn change(&self, change: impl FnOnce(&mut Vec<Visit>, Limits)) {
        let limits = self.limits.get_untracked();
        let Some(file) = self.file.get_untracked() else {
            self.visits.update(|visits| change(visits, limits));
            return;
        };
        let mut change = Some(change);
        let mut merged = None;
        let _ = update_file(&file, |old| {
            let mut visits = old
                .map(|bytes| parse(&String::from_utf8_lossy(&bytes)))
                .unwrap_or_default();
            if let Some(change) = change.take() {
                change(&mut visits, limits);
            }
            let body = format_visits(&visits);
            merged = Some(visits);
            body.into_bytes()
        });
        match (merged, change) {
            // Written, or at least changed: what the file has (or would).
            (Some(visits), _) => self.visits.set(visits),
            // Not even read: a store that cannot be written lasts the
            // session.
            (None, Some(change)) => self.visits.update(|visits| change(visits, limits)),
            (None, None) => {}
        }
    }
}

/// The visits as lines: `rank`, `last`, `key`, by tabs; keys with a tab
/// or a line break are left out.
fn format_visits(visits: &[Visit]) -> String {
    visits
        .iter()
        .filter(|v| !v.key.contains(['\t', '\n', '\r']))
        .map(|v| format!("{}\t{}\t{}\n", v.rank, v.last, v.key))
        .collect()
}

/// The visits in `text`; lines that do not read are left out.
fn parse(text: &str) -> Vec<Visit> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let rank = parts
                .next()?
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite())?;
            let last = parts.next()?.parse().ok()?;
            let key = parts.next().filter(|k| !k.is_empty())?.to_string();
            Some(Visit { key, rank, last })
        })
        .collect()
}

/// Age the ranks once they add up past `limits.max_age`, dropping what
/// falls under one visit; keep at most `limits.keep`, never dropping
/// `fresh`, the key just visited.
fn age(visits: &mut Vec<Visit>, limits: Limits, fresh: Option<&str>) {
    let total: f64 = visits.iter().map(|v| v.rank).sum();
    if total > limits.max_age {
        let factor = 0.9 * limits.max_age / total;
        for visit in visits.iter_mut() {
            visit.rank *= factor;
        }
        visits.retain(|v| v.rank >= 1.0);
    }
    if visits.len() > limits.keep {
        let is_fresh = |v: &Visit| Some(v.key.as_str()) == fresh;
        visits.sort_by(|a, b| {
            is_fresh(b)
                .cmp(&is_fresh(a))
                .then(b.rank.total_cmp(&a.rank))
                .then(b.last.cmp(&a.last))
        });
        visits.truncate(limits.keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_favour_recent_visits() {
        let visit = |rank, last| Visit {
            key: "x".into(),
            rank,
            last,
        };
        let now: u64 = 10_000_000;
        assert_eq!(visit(2.0, now - 60).score(now), 8.0);
        assert_eq!(visit(2.0, now - 7_200).score(now), 4.0);
        assert_eq!(visit(2.0, now - 2 * 86_400).score(now), 1.0);
        assert_eq!(visit(2.0, now - 30 * 86_400).score(now), 0.5);
    }

    #[test]
    fn ranks_age_keys_are_capped_and_the_file_round_trips() {
        let mut visits: Vec<Visit> = [6_000.0, 6_000.0, 0.5]
            .iter()
            .enumerate()
            .map(|(i, rank)| Visit {
                key: format!("k{i}"),
                rank: *rank,
                last: 5,
            })
            .collect();
        let limits = Limits {
            keep: 500,
            max_age: 10_000.0,
        };
        age(&mut visits, limits, None);
        // 12 000.5 in all: aged to 9 000, the half visit dropped.
        assert_eq!(visits.len(), 2);
        age(&mut visits, Limits { keep: 1, ..limits }, None);
        assert_eq!(visits.len(), 1);
        let text = format!("{}junk\n", format_visits(&visits));
        assert_eq!(parse(&text), visits);
    }

    #[test]
    fn an_update_holds_a_lock_and_writes_what_the_change_returns() {
        let file =
            std::env::temp_dir().join(format!("intuituive-update-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&file);
        update_file(&file, |old| {
            assert!(old.is_none());
            b"1".to_vec()
        })
        .unwrap();
        let written = update_file(&file, |old| {
            let mut bytes = old.unwrap();
            bytes.push(b'2');
            bytes
        })
        .unwrap();
        assert_eq!(written, b"12");
        let mut lock = file.as_os_str().to_owned();
        lock.push(".lock");
        assert!(!PathBuf::from(lock).exists());
        std::fs::remove_file(&file).ok();
    }
}

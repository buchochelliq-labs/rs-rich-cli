//! Directories ranked by how often and how lately they were visited, kept
//! across sessions: what zoxide keeps for Yazi's `Z`. The core's
//! [`FrecencyStore`] keyed by paths.

use std::path::{Path, PathBuf};

use super::miller::Miller;
use crate::reactive::watch;
use crate::state::{frecency_session, frecency_store, FrecencyStore};

/// The app name intuiTUIve's own state is kept under ([`state_dir`]).
const APP: &str = "intuituive";

/// A directory in a [`Frecency`] store: how often it was visited (aged),
/// and when last, in seconds since 1970.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    pub path: PathBuf,
    pub rank: f64,
    pub last: u64,
}

impl Visit {
    /// Its score at `now` (zoxide's): the rank, four times over within the
    /// hour, twice within the day, halved within the week, else quartered.
    pub fn score(&self, now: u64) -> f64 {
        crate::state::Visit {
            key: String::new(),
            rank: self.rank,
            last: self.last,
        }
        .score(now)
    }
}

/// Directories ranked by frecency, as zoxide ranks them for Yazi's `Z`:
/// each [`visit`](Self::visit) adds one to a directory's rank and stamps
/// the time, [`ranked`](Self::ranked) lists them best first. The core's
/// [`FrecencyStore`] keyed by paths: kept in a small file, read, changed
/// and written under a lock on each visit (a whole new file renamed over
/// the old), so two sessions' visits add up rather than the last one's
/// winning, and read by the next session. `Copy`; made by [`frecency`]
/// (the user's state directory) or [`frecency_at`] (a file you name).
#[derive(Clone, Copy)]
pub struct Frecency {
    store: FrecencyStore,
}

/// The user's frecency store: `frecency.tsv` in intuiTUIve's state
/// directory ([`state_dir`]), read now; for the session only where there
/// is none. Call it while the app is built.
pub fn frecency() -> Frecency {
    Frecency {
        store: match state_dir() {
            Some(dir) => frecency_store(dir.join("frecency.tsv")),
            None => frecency_session(),
        },
    }
}

/// A frecency store in `file` (made on the first visit), read now.
///
/// ```
/// use intuituive::files::frecency_at;
/// use intuituive::prelude::*;
///
/// let file = std::env::temp_dir().join(format!("intuituive-frecency-doc-{}.tsv", std::process::id()));
/// let _ = std::fs::remove_file(&file);
/// let path = file.clone();
/// let app = App::new(move || {
///     let store = frecency_at(path.clone());
///     store.visit("/srv".as_ref());
///     store.visit("/home".as_ref());
///     store.visit("/home".as_ref());
///     label(format!("{:?}", store.ranked()))
/// });
/// assert_eq!(app.render_with(&[], 30, 1).unwrap()[0].trim_end(), r#"["/home", "/srv"]"#);
/// // The next session reads what this one wrote.
/// let again = App::new(move || label(format!("{:?}", frecency_at(file.clone()).ranked())));
/// assert_eq!(again.render_with(&[], 30, 1).unwrap()[0].trim_end(), r#"["/home", "/srv"]"#);
/// ```
pub fn frecency_at(file: impl Into<PathBuf>) -> Frecency {
    Frecency {
        store: frecency_store(file),
    }
}

/// Where intuiTUIve keeps state between sessions: the core's
/// [`state_dir`](crate::state_dir) for `intuituive`, so
/// `$XDG_STATE_HOME/intuituive` (else `~/.local/state/intuituive`),
/// `~/Library/Application Support/intuituive` on macOS,
/// `%LOCALAPPDATA%\intuituive` on Windows.
pub fn state_dir() -> Option<PathBuf> {
    crate::state::state_dir(APP)
}

/// A path as a store's key; `None` for one that is not UTF-8 or holds a
/// tab or a line break (it would not read back).
fn key(dir: &Path) -> Option<&str> {
    dir.to_str().filter(|k| !k.contains(['\t', '\n', '\r']))
}

impl Frecency {
    /// Count a visit to `dir` now, and write the store.
    pub fn visit(&self, dir: &Path) {
        if let Some(key) = key(dir) {
            self.store.visit(key);
        }
    }

    /// Forget `dir` (one that is gone), and write the store.
    pub fn remove(&self, dir: &Path) {
        if let Some(key) = key(dir) {
            self.store.remove(key);
        }
    }

    /// The directories, best first, subscribing.
    pub fn ranked(&self) -> Vec<PathBuf> {
        self.store.ranked().into_iter().map(PathBuf::from).collect()
    }

    /// Every visit kept, subscribing.
    pub fn visits(&self) -> Vec<Visit> {
        self.store
            .visits()
            .into_iter()
            .map(|v| Visit {
                path: PathBuf::from(v.key),
                rank: v.rank,
                last: v.last,
            })
            .collect()
    }

    /// The file it is kept in (`None` where there is no state directory:
    /// then it lasts the session).
    pub fn file(&self) -> Option<PathBuf> {
        self.store.file()
    }

    /// Read the file again: the visits other sessions wrote since.
    pub fn reload(&self) {
        self.store.reload();
    }

    /// The core store underneath.
    pub fn store(&self) -> FrecencyStore {
        self.store
    }

    /// Count a visit to each directory `miller`'s columns show, as Yazi
    /// tells zoxide.
    pub fn follow(self, miller: Miller) -> Frecency {
        let store = self;
        watch(move || miller.cwd.get(), move |dir, _| store.visit(&dir));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_favour_recent_visits() {
        let visit = |rank, last| Visit {
            path: PathBuf::from("/x"),
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
    fn two_sessions_add_up() {
        let file = std::env::temp_dir().join(format!(
            "intuituive-frecency-two-{}.tsv",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&file);
        let (one, two) = (file.clone(), file.clone());
        // Two windows open at once, each visiting a directory of its own.
        let app = crate::App::new(move || {
            let first = frecency_at(one.clone());
            let second = frecency_at(two.clone());
            first.visit(Path::new("/a"));
            second.visit(Path::new("/b"));
            first.visit(Path::new("/a"));
            crate::label(format!("{:?}", first.ranked()))
        });
        let screen = app.render_with(&[], 30, 1).unwrap();
        // The first saw the second's visit when it wrote.
        assert_eq!(screen[0].trim_end(), r#"["/a", "/b"]"#);
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text.lines().count(), 2, "{text}");
        std::fs::remove_file(&file).ok();
    }
}

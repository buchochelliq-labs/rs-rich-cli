//! Directories ranked by how often and how lately they were visited, kept
//! across sessions: what zoxide keeps for Yazi's `Z`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::miller::Miller;
use crate::reactive::{signal, watch, Signal};

/// The most a store keeps; past it the least visited go.
const KEEP: usize = 500;
/// When the ranks add up to more than this, they all age (zoxide's
/// `_ZO_MAXAGE`).
const MAX_AGE: f64 = 10_000.0;

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

/// Directories ranked by frecency, as zoxide ranks them for Yazi's `Z`:
/// each [`visit`](Self::visit) adds one to a directory's rank and stamps
/// the time, [`ranked`](Self::ranked) lists them best first, and the
/// store is written to a small file after each visit (a whole new file
/// renamed over the old, so a crash never leaves half of one), read again
/// by the next session. `Copy`; made by [`frecency`] (the user's state
/// directory) or [`frecency_at`] (a file you name).
#[derive(Clone, Copy)]
pub struct Frecency {
    visits: Signal<Vec<Visit>>,
    file: Signal<Option<PathBuf>>,
}

/// The user's frecency store: `frecency.tsv` in intuiTUIve's state
/// directory ([`state_dir`]), read now. Call it while the app is built.
pub fn frecency() -> Frecency {
    store(state_dir().map(|dir| dir.join("frecency.tsv")))
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
    store(Some(file.into()))
}

fn store(file: Option<PathBuf>) -> Frecency {
    let visits = file.as_deref().map(read).unwrap_or_default();
    Frecency {
        visits: signal(visits),
        file: signal(file),
    }
}

/// Where intuiTUIve keeps state between sessions: `$XDG_STATE_HOME/intuituive`
/// (else `~/.local/state/intuituive`), `~/Library/Application
/// Support/intuituive` on macOS, `%LOCALAPPDATA%\intuituive` on Windows.
pub fn state_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if cfg!(windows) {
        return var("LOCALAPPDATA").map(|d| PathBuf::from(d).join("intuituive"));
    }
    let home = var("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        return home.map(|h| h.join("Library/Application Support/intuituive"));
    }
    var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| home.map(|h| h.join(".local/state")))
        .map(|dir| dir.join("intuituive"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Frecency {
    /// Count a visit to `dir` now, and write the store.
    pub fn visit(&self, dir: &Path) {
        let (dir, now) = (dir.to_path_buf(), now());
        self.visits.update(|visits| {
            match visits.iter_mut().find(|v| v.path == dir) {
                Some(visit) => {
                    visit.rank += 1.0;
                    visit.last = now;
                }
                None => visits.push(Visit {
                    path: dir,
                    rank: 1.0,
                    last: now,
                }),
            }
            age(visits);
        });
        self.save();
    }

    /// Forget `dir` (one that is gone), and write the store.
    pub fn remove(&self, dir: &Path) {
        self.visits
            .update(|visits| visits.retain(|v| v.path != dir));
        self.save();
    }

    /// The directories, best first, subscribing.
    pub fn ranked(&self) -> Vec<PathBuf> {
        let now = now();
        self.visits.with(|visits| {
            let mut scored: Vec<(f64, &Visit)> = visits.iter().map(|v| (v.score(now), v)).collect();
            scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(b.1.last.cmp(&a.1.last)));
            scored.into_iter().map(|(_, v)| v.path.clone()).collect()
        })
    }

    /// Every visit kept, subscribing.
    pub fn visits(&self) -> Vec<Visit> {
        self.visits.get()
    }

    /// The file it is kept in (`None` where there is no state directory:
    /// then it lasts the session).
    pub fn file(&self) -> Option<PathBuf> {
        self.file.get_untracked()
    }

    /// Count a visit to each directory `miller`'s columns show, as Yazi
    /// tells zoxide.
    pub fn follow(self, miller: Miller) -> Frecency {
        let store = self;
        watch(move || miller.cwd.get(), move |dir, _| store.visit(&dir));
        self
    }

    /// Write the store: a new file beside the old, renamed over it.
    fn save(&self) {
        let Some(file) = self.file.get_untracked() else {
            return;
        };
        let body = self.visits.with_untracked(|visits| {
            visits
                .iter()
                .filter_map(|v| {
                    let path = v.path.to_str()?;
                    (!path.contains(['\t', '\n']))
                        .then(|| format!("{}\t{}\t{path}\n", v.rank, v.last))
                })
                .collect::<String>()
        });
        // A store that cannot be written lasts the session.
        let _ = write_atomic(&file, body.as_bytes());
    }
}

/// Write `bytes` to `file` through a temporary file renamed over it.
fn write_atomic(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let name = file
        .file_name()
        .map_or_else(|| "store".into(), |n| n.to_string_lossy().into_owned());
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

/// The visits in `file`, a line each: `rank`, `last`, `path`, by tabs.
/// Lines that do not read are left out.
fn read(file: &Path) -> Vec<Visit> {
    let Ok(text) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let rank = parts
                .next()?
                .parse::<f64>()
                .ok()
                .filter(|r| r.is_finite())?;
            let last = parts.next()?.parse().ok()?;
            let path = PathBuf::from(parts.next().filter(|p| !p.is_empty())?);
            Some(Visit { path, rank, last })
        })
        .collect()
}

/// Age the ranks once they add up past [`MAX_AGE`], dropping what falls
/// under one visit; keep at most [`KEEP`].
fn age(visits: &mut Vec<Visit>) {
    let total: f64 = visits.iter().map(|v| v.rank).sum();
    if total > MAX_AGE {
        let factor = 0.9 * MAX_AGE / total;
        for visit in visits.iter_mut() {
            visit.rank *= factor;
        }
        visits.retain(|v| v.rank >= 1.0);
    }
    if visits.len() > KEEP {
        visits.sort_by(|a, b| b.rank.total_cmp(&a.rank).then(b.last.cmp(&a.last)));
        visits.truncate(KEEP);
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
    fn ranks_age_and_the_store_round_trips() {
        let mut visits: Vec<Visit> = [6_000.0, 6_000.0, 0.5]
            .iter()
            .enumerate()
            .map(|(i, rank)| Visit {
                path: PathBuf::from(format!("/d{i}")),
                rank: *rank,
                last: 5,
            })
            .collect();
        age(&mut visits);
        // 12 000.5 in all: aged to 9 000, the half visit dropped.
        assert_eq!(visits.len(), 2);
        let file =
            std::env::temp_dir().join(format!("intuituive-frecency-{}.tsv", std::process::id()));
        let body: String = visits
            .iter()
            .map(|v| format!("{}\t{}\t{}\n", v.rank, v.last, v.path.display()))
            .collect();
        write_atomic(&file, format!("{body}junk\n").as_bytes()).unwrap();
        assert_eq!(read(&file), visits);
        std::fs::remove_file(&file).ok();
    }
}

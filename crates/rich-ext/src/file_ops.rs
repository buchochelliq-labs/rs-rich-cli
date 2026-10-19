//! File operations as a file manager runs them: copy, move, delete and
//! trash, reporting progress as they go and stopping when cancelled, with
//! Yazi's rules for names that are taken (a copy gets `_1`, `_2`… before
//! its extension) and for directories pasted into themselves (refused).
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies. [`run`] blocks; call it on a worker thread and send the
//! [`Progress`] it reports wherever it is shown (intuiTUIve's `files`
//! tasks do, with a task panel).
//!
//! ```
//! use rich_ext::cancel::CancelToken;
//! use rich_ext::file_ops::{run, Conflict, Op};
//!
//! let base = std::env::temp_dir().join(format!("rich-ext-ops-doc-{}", std::process::id()));
//! std::fs::create_dir_all(base.join("into")).unwrap();
//! std::fs::write(base.join("a.txt"), "hello").unwrap();
//!
//! let op = Op::Copy { from: vec![base.join("a.txt")], into: base.join("into"), conflict: Conflict::Rename };
//! let mut seen = 0;
//! let report = run(&op, &CancelToken::new(), &mut |p| seen = p.bytes);
//! assert!(report.errors.is_empty());
//! assert_eq!(seen, 5);
//! assert_eq!(std::fs::read_to_string(base.join("into/a.txt")).unwrap(), "hello");
//! // Again: the name is taken, so the copy is a_1.txt.
//! run(&op, &CancelToken::new(), &mut |_| {});
//! assert!(base.join("into/a_1.txt").exists());
//! # std::fs::remove_dir_all(&base).ok();
//! ```

use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::cancel::CancelToken;

/// What to do when the target's name is taken.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Conflict {
    /// Take a free name: `name_1.ext`, `name_2.ext`… (Yazi's default).
    #[default]
    Rename,
    /// Replace what is there (Yazi's `paste --force`).
    Overwrite,
    /// Leave it, and skip this one.
    Skip,
}

/// A file operation over some paths.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    /// Copy each of `from` into the directory `into`: files, links as
    /// links, directories with everything in them.
    Copy {
        from: Vec<PathBuf>,
        into: PathBuf,
        conflict: Conflict,
    },
    /// Move each of `from` into `into`: a rename where it can be, else a
    /// copy and a delete (across file systems).
    Move {
        from: Vec<PathBuf>,
        into: PathBuf,
        conflict: Conflict,
    },
    /// Delete for good: a link itself (never what it points to), a file,
    /// a directory and everything in it.
    Delete(Vec<PathBuf>),
    /// Move to the desktop trash ([`crate::trash::put`]).
    Trash(Vec<PathBuf>),
}

impl Op {
    /// What it does, for a task list: `Copy 3 items to docs`.
    pub fn title(&self) -> String {
        let count = |paths: &[PathBuf]| match paths {
            [one] => one.file_name().map_or_else(
                || one.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            ),
            many => format!("{} items", many.len()),
        };
        let name = |dir: &Path| {
            dir.file_name().map_or_else(
                || dir.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            )
        };
        match self {
            Op::Copy { from, into, .. } => format!("Copy {} to {}", count(from), name(into)),
            Op::Move { from, into, .. } => format!("Move {} to {}", count(from), name(into)),
            Op::Delete(paths) => format!("Delete {}", count(paths)),
            Op::Trash(paths) => format!("Trash {}", count(paths)),
        }
    }

    /// The paths it works on.
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Op::Copy { from, .. } | Op::Move { from, .. } => from,
            Op::Delete(paths) | Op::Trash(paths) => paths,
        }
    }
}

/// How far an operation has got. Totals are counted before it starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Progress {
    /// Bytes copied (copies and moves across file systems).
    pub bytes: u64,
    pub total_bytes: u64,
    /// Files, links and directories done.
    pub items: usize,
    pub total_items: usize,
    /// What it is working on.
    pub current: Option<PathBuf>,
}

impl Progress {
    /// Done, from 0 to 1: by bytes when there are any, else by items.
    pub fn fraction(&self) -> f64 {
        if self.total_bytes > 0 {
            self.bytes as f64 / self.total_bytes as f64
        } else if self.total_items > 0 {
            self.items as f64 / self.total_items as f64
        } else {
            1.0
        }
    }
}

/// How an operation ended.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Report {
    /// What it made: each copy or moved item at its new path.
    pub made: Vec<PathBuf>,
    /// What went wrong, one line each (`name: error`).
    pub errors: Vec<String>,
    /// It was cancelled before the end. A file half copied is removed;
    /// what was done before stays.
    pub cancelled: bool,
}

/// Bytes copied a read.
const CHUNK: usize = 256 * 1024;

/// Run `op`, calling `progress` as it goes (after each chunk of 256 KiB
/// and each item), and stopping at the next of those when `cancel` is
/// cancelled. Blocks until done.
pub fn run(op: &Op, cancel: &CancelToken, progress: &mut dyn FnMut(&Progress)) -> Report {
    let mut runner = Runner {
        cancel,
        progress: Progress::default(),
        emit: progress,
        report: Report::default(),
    };
    for path in op.paths() {
        let (items, bytes) = match op {
            Op::Trash(_) => (1, 0),
            Op::Delete(_) => (count(path).0, 0),
            _ => count(path),
        };
        runner.progress.total_items += items;
        runner.progress.total_bytes += bytes;
    }
    runner.tell();
    for path in op.paths() {
        if runner.stopped() {
            break;
        }
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let done = match op {
            Op::Copy { into, conflict, .. } => runner.paste(path, into, *conflict, false),
            Op::Move { into, conflict, .. } => runner.paste(path, into, *conflict, true),
            Op::Delete(_) => runner.delete(path),
            Op::Trash(_) => crate::trash::put(path).map(|_| {
                runner.progress.items += 1;
                runner.tell();
            }),
        };
        if let Err(error) = done {
            if error.kind() != io::ErrorKind::Interrupted {
                runner.report.errors.push(format!("{name}: {error}"));
            }
        }
    }
    runner.progress.current = None;
    runner.tell();
    runner.report
}

/// Files, links and directories at `path`, and their bytes (files only).
fn count(path: &Path) -> (usize, u64) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if !meta.is_dir() {
        return (1, if meta.is_file() { meta.len() } else { 0 });
    }
    let mut total = (1, 0);
    if let Ok(read) = std::fs::read_dir(path) {
        for entry in read.filter_map(Result::ok) {
            let (items, bytes) = count(&entry.path());
            total.0 += items;
            total.1 += bytes;
        }
    }
    total
}

struct Runner<'a> {
    cancel: &'a CancelToken,
    progress: Progress,
    emit: &'a mut dyn FnMut(&Progress),
    report: Report,
}

fn interrupted() -> io::Error {
    io::Error::new(io::ErrorKind::Interrupted, "cancelled")
}

impl Runner<'_> {
    fn tell(&mut self) {
        (self.emit)(&self.progress);
    }

    fn stopped(&mut self) -> bool {
        if self.cancel.is_cancelled() {
            self.report.cancelled = true;
        }
        self.report.cancelled
    }

    /// Copy or move `from` into the directory `into`.
    fn paste(&mut self, from: &Path, into: &Path, conflict: Conflict, cut: bool) -> io::Result<()> {
        let Some(name) = from.file_name() else {
            return Ok(());
        };
        let meta = std::fs::symlink_metadata(from)?;
        if meta.is_dir() && into.starts_with(from) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "can't go inside itself",
            ));
        }
        let mut target = into.join(name);
        if target == from {
            if cut {
                // A cut pasted where it is: nothing to do.
                let (items, bytes) = count(from);
                self.progress.items += items;
                self.progress.bytes += bytes;
                self.tell();
                return Ok(());
            }
            target = unique(&target);
        } else if std::fs::symlink_metadata(&target).is_ok() {
            match conflict {
                Conflict::Rename => target = unique(&target),
                Conflict::Skip => {
                    let (items, bytes) = count(from);
                    self.progress.items += items;
                    self.progress.bytes += bytes;
                    self.tell();
                    return Ok(());
                }
                Conflict::Overwrite => remove_all(&target)?,
            }
        }
        if cut && std::fs::rename(from, &target).is_ok() {
            let (items, bytes) = count(&target);
            self.progress.items += items;
            self.progress.bytes += bytes;
            self.tell();
            self.report.made.push(target);
            return Ok(());
        }
        self.copy(from, &target)?;
        if cut {
            remove_all(from)?;
        }
        self.report.made.push(target);
        Ok(())
    }

    fn copy(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        if self.stopped() {
            return Err(interrupted());
        }
        self.progress.current = Some(from.to_path_buf());
        let meta = std::fs::symlink_metadata(from)?;
        if meta.file_type().is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(std::fs::read_link(from)?, to)?;
            #[cfg(not(unix))]
            std::fs::copy(from, to)?;
        } else if meta.is_dir() {
            std::fs::create_dir(to)?;
            let mut names: Vec<_> = std::fs::read_dir(from)?
                .filter_map(Result::ok)
                .map(|e| e.file_name())
                .collect();
            names.sort();
            self.progress.items += 1;
            self.tell();
            for name in names {
                self.copy(&from.join(&name), &to.join(&name))?;
            }
            std::fs::set_permissions(to, meta.permissions())?;
            return Ok(());
        } else {
            self.copy_file(from, to, &meta)?;
        }
        self.progress.items += 1;
        self.tell();
        Ok(())
    }

    fn copy_file(&mut self, from: &Path, to: &Path, meta: &std::fs::Metadata) -> io::Result<()> {
        let mut reader = std::fs::File::open(from)?;
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(to)?;
        let mut buffer = vec![0u8; CHUNK];
        let copied = (|| -> io::Result<()> {
            loop {
                if self.stopped() {
                    return Err(interrupted());
                }
                let n = reader.read(&mut buffer)?;
                if n == 0 {
                    return Ok(());
                }
                writer.write_all(&buffer[..n])?;
                self.progress.bytes += n as u64;
                self.tell();
            }
        })();
        drop(writer);
        if let Err(error) = copied {
            let _ = std::fs::remove_file(to);
            return Err(error);
        }
        std::fs::set_permissions(to, meta.permissions())?;
        if let Ok(modified) = meta.modified() {
            let _ = std::fs::File::options()
                .write(true)
                .open(to)
                .and_then(|f| f.set_modified(modified));
        }
        Ok(())
    }

    fn delete(&mut self, path: &Path) -> io::Result<()> {
        if self.stopped() {
            return Err(interrupted());
        }
        self.progress.current = Some(path.to_path_buf());
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_dir() {
            for entry in std::fs::read_dir(path)? {
                self.delete(&entry?.path())?;
            }
            std::fs::remove_dir(path)?;
        } else {
            std::fs::remove_file(path)?;
        }
        self.progress.items += 1;
        self.tell();
        Ok(())
    }
}

/// Remove `path`: a link itself (never what it points to), a file, or a
/// directory and everything in it.
pub fn remove_all(path: &Path) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// `path`, or `path` with `_1`, `_2`… before its extension, whichever does
/// not exist yet: how Yazi pastes over a name that is taken.
pub fn unique(path: &Path) -> PathBuf {
    if std::fs::symlink_metadata(path).is_err() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..)
        .map(|n| path.with_file_name(format!("{stem}_{n}{ext}")))
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .expect("a free name")
}

/// A name typed into a prompt as a path under `dir`: relative, never
/// climbing out of it (`..`, a root or a drive), trailing slashes dropped.
///
/// ```
/// use std::path::Path;
/// use rich_ext::file_ops::under;
///
/// assert_eq!(under(Path::new("/home/a"), "notes/new.md").unwrap(), Path::new("/home/a/notes/new.md"));
/// assert!(under(Path::new("/home/a"), "../b").is_err());
/// assert!(under(Path::new("/home/a"), "/etc/passwd").is_err());
/// ```
pub fn under(dir: &Path, typed: &str) -> Result<PathBuf, String> {
    let typed = typed.trim_end_matches(['/', '\\']);
    let rel = Path::new(typed);
    if typed.is_empty() {
        return Err("No name given".into());
    }
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("{typed:?} must stay inside this directory"));
    }
    Ok(dir.join(rel))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rich-ext-ops-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src/inner")).unwrap();
        std::fs::create_dir_all(dir.join("dst")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "aaaa").unwrap();
        std::fs::write(dir.join("src/inner/b.txt"), "bb").unwrap();
        dir
    }

    fn quiet(op: &Op) -> Report {
        run(op, &CancelToken::new(), &mut |_| {})
    }

    #[test]
    fn copies_trees_and_counts_progress() {
        let dir = base("copy");
        let op = Op::Copy {
            from: vec![dir.join("src")],
            into: dir.join("dst"),
            conflict: Conflict::Rename,
        };
        let mut last = Progress::default();
        let report = run(&op, &CancelToken::new(), &mut |p| last = p.clone());
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(report.made, [dir.join("dst/src")]);
        assert_eq!((last.items, last.total_items), (4, 4));
        assert_eq!((last.bytes, last.total_bytes), (6, 6));
        assert_eq!(last.fraction(), 1.0);
        assert_eq!(
            std::fs::read_to_string(dir.join("dst/src/inner/b.txt")).unwrap(),
            "bb"
        );
        // Copied again: a free name.
        quiet(&op);
        assert!(dir.join("dst/src_1/a.txt").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn conflicts_skip_overwrite_and_never_into_itself() {
        let dir = base("conflict");
        std::fs::write(dir.join("dst/a.txt"), "old").unwrap();
        let op = |conflict| Op::Copy {
            from: vec![dir.join("src/a.txt")],
            into: dir.join("dst"),
            conflict,
        };
        quiet(&op(Conflict::Skip));
        assert_eq!(
            std::fs::read_to_string(dir.join("dst/a.txt")).unwrap(),
            "old"
        );
        quiet(&op(Conflict::Overwrite));
        assert_eq!(
            std::fs::read_to_string(dir.join("dst/a.txt")).unwrap(),
            "aaaa"
        );
        let into_itself = quiet(&Op::Copy {
            from: vec![dir.join("src")],
            into: dir.join("src/inner"),
            conflict: Conflict::Rename,
        });
        assert_eq!(into_itself.errors, ["src: can't go inside itself"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn moves_and_deletes() {
        let dir = base("move");
        let report = quiet(&Op::Move {
            from: vec![dir.join("src/inner")],
            into: dir.join("dst"),
            conflict: Conflict::Rename,
        });
        assert!(report.errors.is_empty(), "{report:?}");
        assert!(!dir.join("src/inner").exists());
        assert!(dir.join("dst/inner/b.txt").exists());
        // A cut pasted where it already is stays.
        quiet(&Op::Move {
            from: vec![dir.join("dst/inner")],
            into: dir.join("dst"),
            conflict: Conflict::Rename,
        });
        assert!(dir.join("dst/inner/b.txt").exists());
        let mut items = 0;
        let report = run(
            &Op::Delete(vec![dir.join("dst")]),
            &CancelToken::new(),
            &mut |p| items = p.items,
        );
        assert!(report.errors.is_empty());
        assert_eq!(items, 3);
        assert!(!dir.join("dst").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cancelling_stops_and_removes_the_half_copied_file() {
        let dir = base("cancel");
        std::fs::write(dir.join("src/big.bin"), vec![7u8; CHUNK * 3]).unwrap();
        let cancel = CancelToken::new();
        let stop = cancel.clone();
        let report = run(
            &Op::Copy {
                from: vec![dir.join("src/big.bin")],
                into: dir.join("dst"),
                conflict: Conflict::Rename,
            },
            &cancel,
            &mut |p| {
                if p.bytes >= CHUNK as u64 {
                    stop.cancel();
                }
            },
        );
        assert!(report.cancelled);
        assert!(report.errors.is_empty(), "{report:?}");
        assert!(!dir.join("dst/big.bin").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn titles() {
        let op = Op::Copy {
            from: vec![PathBuf::from("/a/x.txt")],
            into: PathBuf::from("/b/docs"),
            conflict: Conflict::Rename,
        };
        assert_eq!(op.title(), "Copy x.txt to docs");
        assert_eq!(
            Op::Delete(vec![PathBuf::from("a"), PathBuf::from("b")]).title(),
            "Delete 2 items"
        );
    }
}

//! Git's view of a working tree, file by file: what `git status` says of
//! each path (modified, added, untracked, ignored, …), read from `git
//! status --porcelain=v1 -z` in a subprocess, with no git library. A file
//! manager shows it as a sign beside each name, as Yazi's `git` plugin
//! does; a directory takes the most pressing change inside it.
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies (it runs the `git` on the `PATH`).
//!
//! ```no_run
//! use std::path::Path;
//! use rich_ext::git;
//!
//! if let Some(status) = git::status(Path::new("."))? {
//!     for (path, file) in &status.files {
//!         println!("{} {}", file.change().letter(), path.display());
//!     }
//! }
//! # Ok::<(), std::io::Error>(())
//! ```
//!
//! [`parse_porcelain`] reads the output on its own, for a status taken
//! some other way.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What happened to a path, as a file manager shows it. Ordered from the
/// least pressing to the most: a directory shows the greatest change
/// inside it (ignored files say nothing about their directory).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Change {
    /// Ignored by a `.gitignore`.
    Ignored,
    /// Not tracked.
    Untracked,
    /// Added, renamed or copied (staged).
    Added,
    /// Changed, staged or not, or its type changed.
    Modified,
    /// Deleted, staged or not.
    Deleted,
    /// Unmerged: a conflict to resolve.
    Conflicted,
}

impl Change {
    /// A letter for it: `!`, `?`, `A`, `M`, `D`, `U`.
    pub fn letter(self) -> char {
        match self {
            Change::Ignored => '!',
            Change::Untracked => '?',
            Change::Added => 'A',
            Change::Modified => 'M',
            Change::Deleted => 'D',
            Change::Conflicted => 'U',
        }
    }

    /// The Nerd Font sign Yazi's `git` plugin shows for it.
    pub fn glyph(self) -> &'static str {
        match self {
            Change::Ignored => "\u{f2d1}",
            Change::Untracked => "\u{f128}",
            Change::Added => "\u{f457}",
            Change::Modified => "\u{f040}",
            Change::Deleted => "\u{f458}",
            Change::Conflicted => "\u{f071}",
        }
    }

    /// The colour Yazi's `git` plugin gives it, as a rich style.
    pub fn style(self) -> &'static str {
        match self {
            Change::Ignored => "bright_black",
            Change::Untracked => "magenta",
            Change::Added => "green",
            Change::Modified => "yellow",
            Change::Deleted => "red",
            Change::Conflicted => "yellow",
        }
    }
}

/// One path's two status letters: the index's (`X`) and the work tree's
/// (`Y`), as `git status --porcelain` writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FileStatus {
    pub index: char,
    pub worktree: char,
}

impl FileStatus {
    /// What the two letters come to.
    pub fn change(self) -> Change {
        let (x, y) = (self.index, self.worktree);
        match (x, y) {
            ('!', '!') => Change::Ignored,
            ('?', '?') => Change::Untracked,
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => Change::Conflicted,
            ('D', _) | (_, 'D') => Change::Deleted,
            (_, 'M') | (_, 'T') | ('M', _) | ('T', _) => Change::Modified,
            _ => Change::Added,
        }
    }

    /// Whether something is staged.
    pub fn staged(self) -> bool {
        !matches!(self.index, ' ' | '?' | '!')
    }
}

/// The status of a working tree: its top directory and the paths `git
/// status` lists under it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    /// The work tree's top directory.
    pub root: PathBuf,
    /// Each path git listed, absolute. An untracked or ignored directory
    /// is listed once, for everything in it.
    pub files: HashMap<PathBuf, FileStatus>,
    /// Each directory with a listed path under it, and the greatest
    /// [`Change`] there (ignored files left out).
    dirs: HashMap<PathBuf, Change>,
}

impl GitStatus {
    /// The status of `files` (absolute paths) in the tree at `root`.
    pub fn new(root: PathBuf, files: HashMap<PathBuf, FileStatus>) -> GitStatus {
        let mut dirs: HashMap<PathBuf, Change> = HashMap::new();
        for (path, status) in &files {
            let change = status.change();
            if change == Change::Ignored {
                continue;
            }
            for dir in path.ancestors().skip(1) {
                if !dir.starts_with(&root) {
                    break;
                }
                let slot = dirs.entry(dir.to_path_buf()).or_insert(change);
                *slot = (*slot).max(change);
            }
        }
        GitStatus { root, files, dirs }
    }

    /// What happened to `path` (absolute): its own status; else, for a
    /// directory, the greatest change inside it; else that of an untracked
    /// or ignored directory it is in. `None` for a clean path, or one
    /// outside the tree.
    pub fn of(&self, path: &Path) -> Option<Change> {
        if let Some(status) = self.files.get(path) {
            return Some(status.change());
        }
        if let Some(change) = self.dirs.get(path) {
            return Some(*change);
        }
        path.ancestors()
            .skip(1)
            .take_while(|dir| dir.starts_with(&self.root))
            .find_map(|dir| {
                self.files
                    .get(dir)
                    .map(|s| s.change())
                    .filter(|c| matches!(c, Change::Untracked | Change::Ignored))
            })
    }
}

/// The status of the work tree `dir` is in, ignored files included, or
/// `None` when it is in none (or git is not installed). Runs `git
/// rev-parse --show-toplevel` and `git status --porcelain=v1 -z
/// --ignored=matching`; takes as long as git does, so run it off a UI's
/// thread.
pub fn status(dir: &Path) -> io::Result<Option<GitStatus>> {
    let Some(root) = toplevel(dir)? else {
        return Ok(None);
    };
    let output = git(dir)
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--ignored=matching",
            "--untracked-files=normal",
        ])
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    let files = parse_porcelain(&root, &output.stdout);
    Ok(Some(GitStatus::new(root, files)))
}

/// The top directory of the work tree `dir` is in, if any.
pub fn toplevel(dir: &Path) -> io::Result<Option<PathBuf>> {
    let output = match git(dir).args(["rev-parse", "--show-toplevel"]).output() {
        Ok(output) => output,
        // No git installed: no status, as outside a repository.
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !output.status.success() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Ok((!text.is_empty()).then(|| PathBuf::from(text)))
}

fn git(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        // Never wait for a lock or a pager, never ask anything.
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

/// The paths in `git status --porcelain=v1 -z` output, absolute under
/// `root`: `XY PATH\0`, a rename or copy followed by its old path
/// (`R  NEW\0OLD\0`), whose new path is the one kept. A directory's path
/// (untracked or ignored as a whole) loses its trailing `/`.
///
/// ```
/// use std::path::Path;
/// use rich_ext::git::{parse_porcelain, Change};
///
/// let files = parse_porcelain(Path::new("/repo"), b" M src/lib.rs\0?? new/\0R  b.rs\0a.rs\0");
/// assert_eq!(files[Path::new("/repo/src/lib.rs")].change(), Change::Modified);
/// assert_eq!(files[Path::new("/repo/new")].change(), Change::Untracked);
/// assert_eq!(files[Path::new("/repo/b.rs")].change(), Change::Added);
/// assert_eq!(files.len(), 3);
/// ```
pub fn parse_porcelain(root: &Path, output: &[u8]) -> HashMap<PathBuf, FileStatus> {
    let mut files = HashMap::new();
    let mut records = output.split(|b| *b == 0).filter(|r| !r.is_empty());
    while let Some(record) = records.next() {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let status = FileStatus {
            index: record[0] as char,
            worktree: record[1] as char,
        };
        if matches!(status.index, 'R' | 'C') {
            // The old path follows as a record of its own.
            records.next();
        }
        let path = String::from_utf8_lossy(&record[3..]);
        let path = path.trim_end_matches('/');
        files.insert(root.join(path), status);
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(x: char, y: char) -> FileStatus {
        FileStatus {
            index: x,
            worktree: y,
        }
    }

    #[test]
    fn letters_come_to_changes() {
        assert_eq!(st(' ', 'M').change(), Change::Modified);
        assert_eq!(st('M', ' ').change(), Change::Modified);
        assert_eq!(st('A', 'M').change(), Change::Modified);
        assert_eq!(st('A', ' ').change(), Change::Added);
        assert_eq!(st('R', ' ').change(), Change::Added);
        assert_eq!(st(' ', 'D').change(), Change::Deleted);
        assert_eq!(st('U', 'U').change(), Change::Conflicted);
        assert_eq!(st('A', 'A').change(), Change::Conflicted);
        assert_eq!(st('?', '?').change(), Change::Untracked);
        assert_eq!(st('!', '!').change(), Change::Ignored);
        assert!(st('M', ' ').staged());
        assert!(!st('?', '?').staged());
    }

    #[test]
    fn directories_take_the_greatest_change_inside() {
        let root = Path::new("/r");
        let files = parse_porcelain(
            root,
            b" M a/x.rs\0A  a/b/y.rs\0 D a/b/z.rs\0!! target/\0?? tmp/\0",
        );
        let status = GitStatus::new(root.to_path_buf(), files);
        assert_eq!(status.of(Path::new("/r/a/x.rs")), Some(Change::Modified));
        assert_eq!(status.of(Path::new("/r/a/b")), Some(Change::Deleted));
        assert_eq!(status.of(Path::new("/r/a")), Some(Change::Deleted));
        assert_eq!(status.of(Path::new("/r")), Some(Change::Deleted));
        assert_eq!(
            status.of(Path::new("/r/target/debug/x")),
            Some(Change::Ignored)
        );
        assert_eq!(
            status.of(Path::new("/r/tmp/notes.txt")),
            Some(Change::Untracked)
        );
        assert_eq!(status.of(Path::new("/r/clean.rs")), None);
        assert_eq!(status.of(Path::new("/elsewhere")), None);
    }

    #[test]
    fn a_real_repository() {
        let dir = std::env::temp_dir().join(format!("rich-ext-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let run = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
        };
        // No git on this machine: nothing to test against.
        if !run(&["init", "-q"]).is_ok_and(|o| o.status.success()) {
            return;
        }
        std::fs::write(dir.join("kept.txt"), "a").unwrap();
        std::fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
        run(&["add", "."]).unwrap();
        run(&["commit", "-qm", "first"]).unwrap();
        std::fs::write(dir.join("kept.txt"), "b").unwrap();
        std::fs::write(dir.join("new.txt"), "c").unwrap();
        std::fs::write(dir.join("debug.log"), "d").unwrap();
        let status = status(&dir).unwrap().expect("a repository");
        let root = &status.root;
        assert_eq!(status.of(&root.join("kept.txt")), Some(Change::Modified));
        assert_eq!(status.of(&root.join("new.txt")), Some(Change::Untracked));
        assert_eq!(status.of(&root.join("debug.log")), Some(Change::Ignored));
        assert_eq!(status.of(&root.join(".gitignore")), None);
        let outside = std::env::temp_dir();
        if toplevel(&outside).unwrap().is_none() {
            assert!(super::status(&outside).unwrap().is_none());
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}

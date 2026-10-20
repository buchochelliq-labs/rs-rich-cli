//! Directory listings as a file manager shows them: the entries of a
//! directory with what a row needs (size, mode, times, owner, where a link
//! points), sorted as [Yazi](https://github.com/sxyazi/yazi) sorts them,
//! and the cells its line modes write; plus a directory [`walk`] for
//! finders.
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies. intuiTUIve's `files` components are built on it, but it
//! needs no TUI: a `ls`-like command can print the same rows.
//!
//! - [`Entry`] is one entry, read with [`Entry::read`]; [`read_dir`] reads
//!   and sorts a directory, hidden files left out unless asked for.
//! - [`Sort`] and [`SortBy`] are Yazi's `sort_by` orders (natural,
//!   modified, created, extension, size, random), directories first;
//!   [`natural_cmp`] (the table module's) compares names with their digits
//!   as numbers.
//! - [`readable_size`] (`1.4K`), [`permissions`] (`drwxr-xr-x`) and
//!   [`owner`] (`user:group`) are the line modes' cells; [`find_ranges`]
//!   is where a smart-case search matches a name.
//! - [`walk`] goes through a tree depth first, lazily, as an iterator, so
//!   a finder can show what it found while it goes on.
//!
//! ```
//! use rich_ext::listing::{natural_cmp, permissions, readable_size};
//! use std::cmp::Ordering;
//!
//! assert_eq!(readable_size(1_500), "1.5K");
//! assert_eq!(permissions(0o040755), "drwxr-xr-x");
//! assert_eq!(natural_cmp("a2", "a10"), Ordering::Less);
//! ```

use std::cmp::Ordering;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::filetype::{self, FileFlags};
/// Names compared with their runs of digits as numbers (`a2` before
/// `a10`), case ignored: the table module's natural order.
pub use crate::table::sort::natural_cmp;

/// One entry of a directory: its name and what a listing shows of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    /// A directory, or a link to one.
    pub dir: bool,
    /// The length its metadata gives (a directory's too, as Yazi shows).
    pub size: u64,
    /// Where a symbolic link points, as written in it.
    pub link: Option<PathBuf>,
    /// A link whose target is missing.
    pub orphan: bool,
    /// Executable (a file whose target has an `x` bit; Unix).
    pub exec: bool,
    /// The Unix mode bits of the entry itself (a link's are `l…`).
    pub mode: u32,
    pub mtime: Option<SystemTime>,
    pub btime: Option<SystemTime>,
    pub uid: u32,
    pub gid: u32,
}

impl Entry {
    /// The entry `name` of `dir`, read from the disk. Links are followed
    /// for whether it is a directory, its size and times; what cannot be
    /// read is left at zero.
    pub fn read(dir: &Path, name: impl Into<String>) -> Entry {
        let name = name.into();
        let path = dir.join(&name);
        let own = std::fs::symlink_metadata(&path).ok();
        let is_link = own.as_ref().is_some_and(|m| m.file_type().is_symlink());
        // Follow links, so a link to a directory opens like one.
        let followed = std::fs::metadata(&path).ok();
        let meta = followed.as_ref().or(own.as_ref());
        let dir_ = followed.as_ref().is_some_and(|m| m.is_dir());
        #[cfg(unix)]
        let (mode, followed_mode, uid, gid) = {
            use std::os::unix::fs::MetadataExt;
            let (mode, uid, gid) = own
                .as_ref()
                .map_or((0, 0, 0), |m| (m.mode(), m.uid(), m.gid()));
            (mode, followed.as_ref().map_or(mode, |m| m.mode()), uid, gid)
        };
        #[cfg(not(unix))]
        let (mode, followed_mode, uid, gid) = {
            let mode = if dir_ { 0o040755 } else { 0o100644 };
            (mode, mode, 0, 0)
        };
        Entry {
            name,
            dir: dir_,
            size: meta.map_or(0, |m| m.len()),
            link: is_link.then(|| std::fs::read_link(&path).unwrap_or_default()),
            orphan: is_link && followed.is_none(),
            exec: !dir_ && followed.is_some() && followed_mode & 0o111 != 0,
            mode,
            mtime: meta.and_then(|m| m.modified().ok()),
            btime: meta.and_then(|m| m.created().ok()),
            uid,
            gid,
        }
    }

    /// Whether the name starts with a dot.
    pub fn is_hidden(&self) -> bool {
        self.name.starts_with('.')
    }

    /// What Yazi's icon and colour rules test, `hovered` for the entry
    /// under the cursor.
    pub fn flags(&self, hovered: bool) -> FileFlags {
        FileFlags {
            dir: self.dir,
            link: self.link.is_some(),
            orphan: self.orphan,
            exec: self.exec,
            hovered,
            ..FileFlags::default()
        }
    }

    /// Its MIME type as far as its name tells: `inode/directory` for a
    /// directory, else by the extension, else `""`. A sniffed type
    /// ([`filetype::mime_for`]) is better, and costs a read.
    pub fn mime_by_name(&self) -> &'static str {
        if self.dir {
            "inode/directory"
        } else {
            filetype::mime_for_path(Path::new(&self.name)).unwrap_or("")
        }
    }
}

/// What a directory is sorted by, as Yazi's `sort_by` names them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SortBy {
    /// By name, ignoring case (Yazi's default).
    #[default]
    Alphabetical,
    /// By name, ignoring case, runs of digits compared as numbers.
    Natural,
    /// Oldest modified first.
    Mtime,
    /// Oldest created first.
    Btime,
    /// By extension, then by name.
    Extension,
    /// Smallest first.
    Size,
    /// Shuffled, the same way for the whole process.
    Random,
}

/// A sort order: what by, whether reversed, and whether directories come
/// first (whichever way the rest goes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sort {
    pub by: SortBy,
    pub reverse: bool,
    pub dirs_first: bool,
}

impl Default for Sort {
    /// Alphabetical, directories first: Yazi's default.
    fn default() -> Self {
        Sort {
            by: SortBy::Alphabetical,
            reverse: false,
            dirs_first: true,
        }
    }
}

impl Sort {
    /// By `by`, not reversed, directories first.
    pub fn by(by: SortBy) -> Sort {
        Sort {
            by,
            ..Sort::default()
        }
    }

    /// Reversed (or not).
    #[must_use]
    pub fn reverse(mut self, on: bool) -> Sort {
        self.reverse = on;
        self
    }
}

/// The entries of `dir`, sorted by `sort`, with the ones whose name starts
/// with a dot only if `hidden`. Entries that vanish while it reads are
/// left out.
pub fn read_dir(dir: &Path, hidden: bool, sort: Sort) -> std::io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| hidden || !name.starts_with('.'))
        .map(|name| Entry::read(dir, name))
        .collect();
    sort_entries(&mut entries, sort);
    Ok(entries)
}

/// Sort `entries` by `sort`; ties go by name.
pub fn sort_entries(entries: &mut [Entry], sort: Sort) {
    let ext = |e: &Entry| -> String {
        Path::new(&e.name)
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    };
    let alpha = |a: &Entry, b: &Entry| a.name.to_lowercase().cmp(&b.name.to_lowercase());
    entries.sort_by(|a, b| {
        let by = match sort.by {
            SortBy::Alphabetical => alpha(a, b),
            SortBy::Natural => natural_cmp(&a.name, &b.name),
            SortBy::Mtime => a.mtime.cmp(&b.mtime),
            SortBy::Btime => a.btime.cmp(&b.btime),
            SortBy::Extension => ext(a).cmp(&ext(b)),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Random => scramble(&a.name).cmp(&scramble(&b.name)),
        }
        .then_with(|| alpha(a, b));
        let by = if sort.reverse { by.reverse() } else { by };
        match (a.dir, b.dir) {
            (true, false) if sort.dirs_first => Ordering::Less,
            (false, true) if sort.dirs_first => Ordering::Greater,
            _ => by,
        }
    });
}

/// A stable shuffle key for the random sort (FNV-1a of the name, salted
/// once a process).
fn scramble(name: &str) -> u64 {
    static SALT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    let salt = *SALT.get_or_init(|| {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64)
    });
    name.bytes().fold(0xcbf29ce484222325 ^ salt, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

/// A size as Yazi's `readable_size` writes it: `289B`, `1.4K`, `2K`, `3.1G`.
pub fn readable_size(size: u64) -> String {
    let units = ["B", "K", "M", "G", "T", "P", "E"];
    let (mut value, mut unit) = (size as f64, 0);
    while value > 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    let text = format!("{value:.1}{}", units[unit]);
    text.replacen(".0", "", 1)
}

/// The mode bits as `ls -l` writes them: `drwxr-xr-x`, `-rwsr-xr-x`,
/// `lrwxrwxrwx`.
pub fn permissions(mode: u32) -> String {
    let kind = match mode & 0o170000 {
        0o040000 => 'd',
        0o120000 => 'l',
        0o060000 => 'b',
        0o020000 => 'c',
        0o010000 => 'p',
        0o140000 => 's',
        _ => '-',
    };
    let mut out = String::from(kind);
    for (shift, special, set) in [(6, 0o4000, 's'), (3, 0o2000, 's'), (0, 0o1000, 't')] {
        let bits = (mode >> shift) & 0o7;
        out.push(if bits & 4 != 0 { 'r' } else { '-' });
        out.push(if bits & 2 != 0 { 'w' } else { '-' });
        out.push(match (bits & 1 != 0, mode & special != 0) {
            (true, true) => set,
            (false, true) => set.to_ascii_uppercase(),
            (true, false) => 'x',
            (false, false) => '-',
        });
    }
    out
}

/// `user:group`, by name where the system knows them (Yazi's owner line
/// mode, through [`crate::fs::user_name`]), else by number.
pub fn owner(uid: u32, gid: u32) -> String {
    let user = crate::fs::user_name(uid).unwrap_or_else(|| uid.to_string());
    let group = crate::fs::group_name(gid).unwrap_or_else(|| gid.to_string());
    format!("{user}:{group}")
}

/// Where `query` occurs in `name`, smart-case (case counts only when the
/// query has a capital), as byte ranges: what a file manager's find
/// highlights. Empty for an empty query.
///
/// ```
/// use rich_ext::listing::find_ranges;
///
/// assert_eq!(find_ranges("Notes.md", "no"), vec![(0, 2)]);
/// assert!(find_ranges("Notes.md", "No").len() == 1);
/// assert!(find_ranges("notes.md", "No").is_empty());
/// ```
pub fn find_ranges(name: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() {
        return Vec::new();
    }
    let sensitive = query.chars().any(char::is_uppercase);
    let (hay, needle) = if sensitive {
        (name.to_string(), query.to_string())
    } else {
        (name.to_lowercase(), query.to_lowercase())
    };
    // Lower-casing can change lengths; then match only where it didn't.
    if hay.len() != name.len() {
        return Vec::new();
    }
    hay.match_indices(&needle)
        .map(|(at, m)| (at, at + m.len()))
        .collect()
}

/// Whether `name` matches `query` as [`find_ranges`] does; an empty query
/// matches everything.
pub fn smart_contains(name: &str, query: &str) -> bool {
    query.is_empty() || !find_ranges(name, query).is_empty()
}

/// A depth-first walk of a directory tree, as an iterator of paths under
/// the root (the root itself left out). Lazy: each directory is read when
/// the walk reaches it, so the first paths come at once however big the
/// tree. Links are listed but not followed; unreadable directories are
/// skipped.
///
/// ```
/// use rich_ext::listing::walk;
///
/// let root = std::env::temp_dir().join(format!("rich-ext-walk-doc-{}", std::process::id()));
/// std::fs::create_dir_all(root.join("a/b")).unwrap();
/// std::fs::write(root.join("a/b/c.txt"), "").unwrap();
/// std::fs::write(root.join(".hidden"), "").unwrap();
/// let mut found: Vec<String> = walk(&root)
///     .map(|p| p.strip_prefix(&root).unwrap().display().to_string())
///     .collect();
/// found.sort();
/// assert_eq!(found, ["a", "a/b", "a/b/c.txt"].map(|p| p.replace('/', std::path::MAIN_SEPARATOR_STR)));
/// # std::fs::remove_dir_all(&root).ok();
/// ```
#[derive(Debug)]
pub struct Walk {
    root: PathBuf,
    stack: Vec<PathBuf>,
    pending: std::vec::IntoIter<PathBuf>,
    hidden: bool,
    max_depth: Option<usize>,
    root_depth: usize,
    skip: Vec<String>,
}

/// A [`Walk`] of the tree under `root`, hidden entries left out.
pub fn walk(root: impl Into<PathBuf>) -> Walk {
    let root = root.into();
    let root_depth = root.components().count();
    Walk {
        stack: vec![root.clone()],
        root,
        pending: Vec::new().into_iter(),
        hidden: false,
        max_depth: None,
        root_depth,
        skip: Vec::new(),
    }
}

impl Walk {
    /// The directory walked.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Include the entries whose name starts with a dot (and walk into
    /// such directories).
    #[must_use]
    pub fn hidden(mut self, on: bool) -> Walk {
        self.hidden = on;
        self
    }

    /// Go at most `depth` levels down: 1 is the root's own entries.
    #[must_use]
    pub fn max_depth(mut self, depth: usize) -> Walk {
        self.max_depth = Some(depth);
        self
    }

    /// List but never walk into directories called `name` (`.git`,
    /// `target`, `node_modules`).
    #[must_use]
    pub fn skip_dir(mut self, name: impl Into<String>) -> Walk {
        self.skip.push(name.into());
        self
    }
}

impl Iterator for Walk {
    type Item = PathBuf;

    fn next(&mut self) -> Option<PathBuf> {
        loop {
            if let Some(path) = self.pending.next() {
                return Some(path);
            }
            let dir = self.stack.pop()?;
            let depth = dir.components().count() - self.root_depth;
            if self.max_depth.is_some_and(|max| depth >= max) {
                continue;
            }
            let Ok(read) = std::fs::read_dir(&dir) else {
                continue;
            };
            let mut found = Vec::new();
            let mut dirs = Vec::new();
            for entry in read.filter_map(Result::ok) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !self.hidden && name.starts_with('.') {
                    continue;
                }
                let path = entry.path();
                // `file_type` does not follow links: a link to a directory
                // is listed, never walked into (no cycles).
                if entry.file_type().is_ok_and(|t| t.is_dir()) && !self.skip.contains(&name) {
                    dirs.push(path.clone());
                }
                found.push(path);
            }
            found.sort();
            dirs.sort();
            // Popped from the end: the first directory is walked first.
            self.stack.extend(dirs.into_iter().rev());
            self.pending = found.into_iter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rich-ext-listing-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("b-dir")).unwrap();
        std::fs::write(root.join("a10.txt"), "1234").unwrap();
        std::fs::write(root.join("a2.txt"), "12").unwrap();
        std::fs::write(root.join(".dot"), "").unwrap();
        std::fs::write(root.join("Z.md"), "123456").unwrap();
        root
    }

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn read_dir_sorts_directories_first_and_hides_dot_files() {
        let root = tree("sort");
        let all = read_dir(&root, false, Sort::default()).unwrap();
        assert_eq!(names(&all), ["b-dir", "a10.txt", "a2.txt", "Z.md"]);
        assert!(all[0].dir);
        let natural = read_dir(&root, false, Sort::by(SortBy::Natural)).unwrap();
        assert_eq!(names(&natural), ["b-dir", "a2.txt", "a10.txt", "Z.md"]);
        let by_size = read_dir(&root, true, Sort::by(SortBy::Size).reverse(true)).unwrap();
        assert_eq!(
            names(&by_size),
            ["b-dir", "Z.md", "a10.txt", "a2.txt", ".dot"]
        );
        let mixed = read_dir(
            &root,
            false,
            Sort {
                dirs_first: false,
                ..Sort::default()
            },
        )
        .unwrap();
        assert_eq!(names(&mixed), ["a10.txt", "a2.txt", "b-dir", "Z.md"]);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn line_mode_cells() {
        assert_eq!(readable_size(289), "289B");
        assert_eq!(readable_size(2048), "2K");
        assert_eq!(readable_size(1_450), "1.4K");
        assert_eq!(permissions(0o100644), "-rw-r--r--");
        assert_eq!(permissions(0o104755), "-rwsr-xr-x");
        assert_eq!(permissions(0o041777), "drwxrwxrwt");
        assert_eq!(permissions(0o120777), "lrwxrwxrwx");
    }

    #[test]
    fn natural_order_and_smart_case_find() {
        assert_eq!(natural_cmp("file9", "file10"), Ordering::Less);
        assert_eq!(natural_cmp("Img12", "img3"), Ordering::Greater);
        assert_eq!(find_ranges("abcabc", "bc"), vec![(1, 3), (4, 6)]);
        assert!(smart_contains("README", "read"));
        assert!(!smart_contains("readme", "READ"));
    }

    #[test]
    fn walk_is_depth_first_and_bounded() {
        let root = tree("walk");
        std::fs::create_dir_all(root.join("b-dir/deep/deeper")).unwrap();
        std::fs::create_dir_all(root.join(".git/objects")).unwrap();
        let rel = |w: Walk| -> Vec<String> {
            w.map(|p| {
                p.strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
        };
        let shallow = rel(walk(&root).max_depth(1));
        assert_eq!(shallow, ["Z.md", "a10.txt", "a2.txt", "b-dir"]);
        let all = rel(walk(&root));
        assert!(all.contains(&"b-dir/deep/deeper".to_string()), "{all:?}");
        assert!(!all.iter().any(|p| p.starts_with(".git")), "{all:?}");
        let hidden = rel(walk(&root).hidden(true).skip_dir(".git"));
        assert!(hidden.contains(&".git".to_string()), "{hidden:?}");
        assert!(!hidden.contains(&".git/objects".to_string()), "{hidden:?}");
        std::fs::remove_dir_all(&root).ok();
    }
}

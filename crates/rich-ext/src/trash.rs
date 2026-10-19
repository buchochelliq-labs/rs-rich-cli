//! The desktop trash, as the [FreeDesktop.org trash
//! specification](https://specifications.freedesktop.org/trash-spec/latest/)
//! keeps it: [`put`] moves a file or directory to the trash with a note of
//! where it was and when, [`list`] reads what is there, [`restore`] puts an
//! item back and [`purge`] deletes it for good. File managers on Linux and
//! the BSDs (GNOME Files, Dolphin, Thunar, Yazi through `trash-cli`) share
//! this trash, so what is put here shows up in theirs.
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies.
//!
//! - A file on the home directory's file system goes to the home trash,
//!   `$XDG_DATA_HOME/Trash` (`~/.local/share/Trash`). A file on another
//!   mount goes to that mount's `.Trash/$uid` (when an administrator made
//!   `.Trash` sticky and it is not a link) or `.Trash-$uid`, as the
//!   specification says: a trashed file is never copied across devices.
//! - Each item is `files/NAME` with `info/NAME.trashinfo` beside it, which
//!   holds the original path (percent-encoded) and the local deletion time.
//!   A name already in the trash gets `.2`, `.3`… after it.
//! - On macOS and Windows, whose bins are kept by the Finder and the
//!   shell, [`put`], [`list`] and [`home`] return an error of kind
//!   [`Unsupported`](std::io::ErrorKind::Unsupported) that says so, rather
//!   than writing a trash nothing else reads. [`Trash::at`] works anywhere:
//!   it is a trash directory you name.
//!
//! ```
//! use rich_ext::trash::Trash;
//!
//! let base = std::env::temp_dir().join(format!("rich-ext-trash-doc-{}", std::process::id()));
//! std::fs::create_dir_all(&base).unwrap();
//! let file = base.join("draft.txt");
//! std::fs::write(&file, "old").unwrap();
//!
//! let trash = Trash::at(base.join("Trash"));
//! let item = trash.put(&file).unwrap();
//! assert!(!file.exists());
//! assert_eq!(trash.list().unwrap()[0].original, file);
//! item.restore().unwrap();
//! assert!(file.exists());
//! # std::fs::remove_dir_all(&base).ok();
//! ```

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Something in a trash: its name under `files/`, where it was, and when
/// it was trashed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrashItem {
    /// The trash directory it is in (the one with `files/` and `info/`).
    pub trash: PathBuf,
    /// Its name in `files/` (and, with `.trashinfo`, in `info/`).
    pub name: String,
    /// Where it was, absolute.
    pub original: PathBuf,
    /// When it was trashed, as the info file has it: local time,
    /// `YYYY-MM-DDThh:mm:ss`.
    pub deleted: String,
}

impl TrashItem {
    /// Where the item is now.
    pub fn path(&self) -> PathBuf {
        self.trash.join("files").join(&self.name)
    }

    fn info_path(&self) -> PathBuf {
        self.trash
            .join("info")
            .join(format!("{}.trashinfo", self.name))
    }

    /// Put it back where it was (making its directory again if that went
    /// too), and return that path. An error of kind
    /// [`AlreadyExists`](io::ErrorKind::AlreadyExists) if something has
    /// taken its place since.
    pub fn restore(&self) -> io::Result<PathBuf> {
        if std::fs::symlink_metadata(&self.original).is_ok() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} exists already", self.original.display()),
            ));
        }
        if let Some(parent) = self.original.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(self.path(), &self.original)?;
        std::fs::remove_file(self.info_path())?;
        Ok(self.original.clone())
    }

    /// Delete it for good.
    pub fn purge(&self) -> io::Result<()> {
        let path = self.path();
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&path)?,
            Ok(_) => std::fs::remove_file(&path)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        std::fs::remove_file(self.info_path())
    }
}

/// A trash directory: `files/` and `info/` under one directory.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Trash {
    dir: PathBuf,
    /// For a mount's trash, the mount: its info files hold paths relative
    /// to it.
    topdir: Option<PathBuf>,
}

impl Trash {
    /// The trash at `dir` (made when something is first put in it), with
    /// absolute paths in its info files, as the home trash keeps them.
    pub fn at(dir: impl Into<PathBuf>) -> Trash {
        Trash {
            dir: dir.into(),
            topdir: None,
        }
    }

    /// The trash of the mount `topdir` at `dir`: its info files hold paths
    /// relative to `topdir`.
    pub fn of_mount(dir: impl Into<PathBuf>, topdir: impl Into<PathBuf>) -> Trash {
        Trash {
            dir: dir.into(),
            topdir: Some(topdir.into()),
        }
    }

    /// The trash directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Move `path` into this trash. It must be on the trash's file system
    /// (a rename); an error of kind
    /// [`CrossesDevices`](io::ErrorKind::CrossesDevices) if not.
    pub fn put(&self, path: &Path) -> io::Result<TrashItem> {
        let original = std::path::absolute(path)?;
        std::fs::symlink_metadata(&original)?;
        if self.dir.starts_with(&original) || original.starts_with(&self.dir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the trash can't go in the trash",
            ));
        }
        let files = self.dir.join("files");
        let info = self.dir.join("info");
        std::fs::create_dir_all(&files)?;
        std::fs::create_dir_all(&info)?;
        let base = original
            .file_name()
            .map_or_else(|| "item".to_string(), |n| n.to_string_lossy().into_owned());
        let recorded = match &self.topdir {
            Some(top) => original
                .strip_prefix(top)
                .unwrap_or(&original)
                .to_path_buf(),
            None => original.clone(),
        };
        let deleted = timestamp(SystemTime::now());
        // Reserve a name by making its info file (O_EXCL), then move.
        for n in 1u32.. {
            let name = if n == 1 {
                base.clone()
            } else {
                format!("{base}.{n}")
            };
            let info_file = info.join(format!("{name}.trashinfo"));
            let mut file = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&info_file)
            {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            if std::fs::symlink_metadata(files.join(&name)).is_ok() {
                // A file without its info: leave both alone.
                drop(file);
                std::fs::remove_file(&info_file)?;
                continue;
            }
            let body = format!(
                "[Trash Info]\nPath={}\nDeletionDate={deleted}\n",
                encode(&recorded)
            );
            let written = file
                .write_all(body.as_bytes())
                .and_then(|_| file.sync_all());
            let moved = written.and_then(|_| std::fs::rename(&original, files.join(&name)));
            if let Err(error) = moved {
                let _ = std::fs::remove_file(&info_file);
                return Err(error);
            }
            return Ok(TrashItem {
                trash: self.dir.clone(),
                name,
                original,
                deleted,
            });
        }
        unreachable!("a free name")
    }

    /// What is in this trash, oldest first; items whose info file cannot
    /// be read are left out. An empty list if the trash was never made.
    pub fn list(&self) -> io::Result<Vec<TrashItem>> {
        let read = match std::fs::read_dir(self.dir.join("info")) {
            Ok(read) => read,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut items: Vec<TrashItem> = read
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file = entry.file_name().to_string_lossy().into_owned();
                let name = file.strip_suffix(".trashinfo")?.to_string();
                let text = std::fs::read_to_string(entry.path()).ok()?;
                let (path, deleted) = parse_info(&text)?;
                let original = match &self.topdir {
                    Some(top) if path.is_relative() => top.join(path),
                    _ => path,
                };
                Some(TrashItem {
                    trash: self.dir.clone(),
                    name,
                    original,
                    deleted,
                })
            })
            .collect();
        items.sort_by(|a, b| a.deleted.cmp(&b.deleted).then(a.name.cmp(&b.name)));
        Ok(items)
    }
}

/// The `Path` and `DeletionDate` of a `.trashinfo` file.
fn parse_info(text: &str) -> Option<(PathBuf, String)> {
    let mut lines = text.lines().map(str::trim);
    lines.by_ref().find(|l| *l == "[Trash Info]")?;
    let (mut path, mut deleted) = (None, String::new());
    for line in lines {
        if line.starts_with('[') {
            break;
        }
        if let Some(value) = line.strip_prefix("Path=") {
            path = Some(decode(value)?);
        } else if let Some(value) = line.strip_prefix("DeletionDate=") {
            deleted = value.to_string();
        }
    }
    Some((path?, deleted))
}

/// A path percent-encoded as the specification asks (RFC 2396: all but
/// unreserved characters and `/`).
fn encode(path: &Path) -> String {
    let bytes = path_bytes(path);
    let mut out = String::new();
    for b in bytes {
        if b.is_ascii_alphanumeric() || b"-_.~!*'()/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(text: &str) -> Option<PathBuf> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Some(path_from_bytes(out))
}

#[cfg(unix)]
fn path_bytes(path: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn path_bytes(path: &Path) -> Vec<u8> {
    path.to_string_lossy().replace('\\', "/").into_bytes()
}

#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

/// `time` as `YYYY-MM-DDThh:mm:ss` in the local time zone (UTC off Unix).
fn timestamp(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let (y, mo, d, h, mi, s) = local_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}")
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn local_parts(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let t = secs as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::zeroed();
    // SAFETY: both pointers are valid for the call; `localtime_r` fills
    // `tm` and is thread-safe.
    let filled = unsafe { !libc::localtime_r(&t, tm.as_mut_ptr()).is_null() };
    if !filled {
        return utc_parts(secs);
    }
    // SAFETY: `localtime_r` succeeded, so `tm` is initialised (and it was
    // zeroed before).
    let tm = unsafe { tm.assume_init() };
    (
        tm.tm_year as i64 + 1900,
        (tm.tm_mon + 1) as u32,
        tm.tm_mday as u32,
        tm.tm_hour as u32,
        tm.tm_min as u32,
        tm.tm_sec as u32,
    )
}

#[cfg(not(unix))]
fn local_parts(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    utc_parts(secs)
}

/// The UTC date and time of a Unix time (Howard Hinnant's
/// `civil_from_days`).
fn utc_parts(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// Why there is no trash here.
#[cfg(any(not(unix), target_os = "macos"))]
fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the FreeDesktop trash is for Linux and the BSDs; macOS and Windows keep \
         their own bins, which this does not write to",
    )
}

/// The home trash: `$XDG_DATA_HOME/Trash`, else `~/.local/share/Trash`.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn home() -> io::Result<Trash> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no home directory"))?;
    Ok(Trash::at(data.join("Trash")))
}

/// The home trash: not on this platform.
#[cfg(any(not(unix), target_os = "macos"))]
pub fn home() -> io::Result<Trash> {
    Err(unsupported())
}

/// Move `path` to the trash: the home trash if it is on the same file
/// system, else its mount's (see the [module docs](self)).
#[cfg(all(unix, not(target_os = "macos")))]
pub fn put(path: &Path) -> io::Result<TrashItem> {
    use std::os::unix::fs::MetadataExt;
    let home = home()?;
    let original = std::path::absolute(path)?;
    let device = std::fs::symlink_metadata(&original)?.dev();
    // The home trash's device: of the trash, or of the nearest directory
    // that exists on the way to it.
    let home_device = home
        .dir()
        .ancestors()
        .find_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.dev());
    if home_device == Some(device) {
        return home.put(&original);
    }
    mount_trash(&original, device)?.put(&original)
}

/// Move `path` to the trash: not on this platform.
#[cfg(any(not(unix), target_os = "macos"))]
pub fn put(path: &Path) -> io::Result<TrashItem> {
    let _ = path;
    Err(unsupported())
}

/// The trash of the mount `original` (on `device`) is on.
#[cfg(all(unix, not(target_os = "macos")))]
fn mount_trash(original: &Path, device: u64) -> io::Result<Trash> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mut top = original.parent().unwrap_or(original).to_path_buf();
    while let Some(up) = top.parent() {
        match std::fs::metadata(up) {
            Ok(meta) if meta.dev() == device => top = up.to_path_buf(),
            _ => break,
        }
    }
    let uid = rustix::process::getuid().as_raw();
    // An administrator's `.Trash`: sticky, and not a link.
    let shared = top.join(".Trash");
    if let Ok(meta) = std::fs::symlink_metadata(&shared) {
        if meta.is_dir() && meta.permissions().mode() & 0o1000 != 0 {
            return Ok(Trash::of_mount(shared.join(uid.to_string()), top));
        }
    }
    Ok(Trash::of_mount(top.join(format!(".Trash-{uid}")), top))
}

/// What is in the home trash, oldest first.
pub fn list() -> io::Result<Vec<TrashItem>> {
    home()?.list()
}

/// Put `item` back where it was; see [`TrashItem::restore`].
pub fn restore(item: &TrashItem) -> io::Result<PathBuf> {
    item.restore()
}

/// Delete `item` for good.
pub fn purge(item: &TrashItem) -> io::Result<()> {
    item.purge()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rich-ext-trash-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn put_list_restore_and_purge() {
        let dir = base("cycle");
        let trash = Trash::at(dir.join("Trash"));
        let a = dir.join("a file.txt");
        std::fs::write(&a, "one").unwrap();
        let first = trash.put(&a).unwrap();
        std::fs::write(&a, "two").unwrap();
        let second = trash.put(&a).unwrap();
        assert_eq!(first.name, "a file.txt");
        assert_eq!(second.name, "a file.txt.2");
        let info = std::fs::read_to_string(dir.join("Trash/info/a file.txt.trashinfo")).unwrap();
        assert!(info.starts_with("[Trash Info]\nPath=/"), "{info}");
        assert!(info.contains("a%20file.txt\n"), "{info}");
        let items = trash.list().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.original == a));
        assert_eq!(items[0].deleted.len(), "2026-10-19T14:05:00".len());
        // Restoring over a file that came back is refused.
        std::fs::write(&a, "three").unwrap();
        let refused = first.restore().unwrap_err();
        assert_eq!(refused.kind(), io::ErrorKind::AlreadyExists);
        std::fs::remove_file(&a).unwrap();
        assert_eq!(first.restore().unwrap(), a);
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "one");
        second.purge().unwrap();
        assert!(trash.list().unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn directories_and_mount_relative_paths() {
        let dir = base("mount");
        std::fs::create_dir_all(dir.join("proj/src")).unwrap();
        std::fs::write(dir.join("proj/src/main.rs"), "fn main() {}").unwrap();
        let trash = Trash::of_mount(dir.join(".Trash-1000"), &dir);
        let item = trash.put(&dir.join("proj")).unwrap();
        let info = std::fs::read_to_string(dir.join(".Trash-1000/info/proj.trashinfo")).unwrap();
        assert!(info.contains("\nPath=proj\n"), "{info}");
        assert_eq!(trash.list().unwrap()[0].original, dir.join("proj"));
        assert!(item.path().join("src/main.rs").exists());
        // Restoring makes a missing parent again.
        item.restore().unwrap();
        assert!(dir.join("proj/src/main.rs").exists());
        assert!(trash.put(&dir).is_err(), "the trash itself");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn info_files_round_trip() {
        let path = Path::new("/home/ada/notes 100%/ü.md");
        let encoded = encode(path);
        assert_eq!(encoded, "/home/ada/notes%20100%25/%C3%BC.md");
        assert_eq!(decode(&encoded).unwrap(), path);
        let (read, when) =
            parse_info("[Trash Info]\nPath=%2Ftmp%2Fx\nDeletionDate=2004-08-31T22:32:08\n")
                .unwrap();
        assert_eq!(read, Path::new("/tmp/x"));
        assert_eq!(when, "2004-08-31T22:32:08");
        assert!(parse_info("[Other]\nPath=/x\n").is_none());
        assert_eq!(utc_parts(1_093_991_528), (2004, 8, 31, 22, 32, 8));
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn the_home_trash_follows_xdg_data_home() {
        // Read only: a test must not trash into the real home.
        let trash = home().unwrap();
        assert!(trash.dir().ends_with("Trash"));
    }
}

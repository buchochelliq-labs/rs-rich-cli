//! What is inside an archive, and taking it out: the entries of a tar,
//! gzipped tar or zip file ([`entries`]), and extraction into a directory
//! ([`extract`]) that never writes outside it. A file manager previews an
//! archive with the listing and extracts it with Yazi's `extract` opener.
//!
//! Not a port of anything upstream: an rs-rich addition, behind this
//! crate's off-by-default `archive` feature, which brings in `zip` (its
//! deflate method only) and `flate2` (the pure-Rust backend). Tar is read
//! here: ustar, GNU long names and pax `path` records.
//!
//! - The format comes from the first bytes (zip's `PK`, gzip's `1f 8b`,
//!   tar's `ustar`), else from the name (`.tar`, `.tar.gz`, `.tgz`,
//!   `.zip`); [`Format::of`] says which, or `None`.
//! - Extraction is careful with what an archive says: an entry whose path
//!   is absolute or climbs out with `..` is refused, links (symbolic and
//!   hard) are not made, nothing that exists is overwritten, and set-id
//!   bits are dropped. What was left out is listed in [`Extracted::skipped`].
//! - [`extract_with`] reports [`Progress`] and stops when cancelled, as
//!   [`crate::file_ops::run`] does.
//!
//! ```
//! # fn main() -> std::io::Result<()> {
//! use rich_ext::archive::{entries, extract};
//!
//! let dir = std::env::temp_dir().join(format!("rich-ext-archive-doc-{}", std::process::id()));
//! std::fs::create_dir_all(&dir)?;
//! // A zip with one file, written with the `zip` crate.
//! let path = dir.join("docs.zip");
//! let mut zip = zip::ZipWriter::new(std::fs::File::create(&path)?);
//! zip.start_file("readme.txt", zip::write::SimpleFileOptions::default())?;
//! std::io::Write::write_all(&mut zip, b"hello")?;
//! zip.finish()?;
//!
//! let listed = entries(&path)?;
//! assert_eq!((listed[0].path.as_str(), listed[0].size), ("readme.txt", 5));
//! let out = extract(&path, &dir.join("out"))?;
//! assert_eq!(out.files, 1);
//! assert_eq!(std::fs::read_to_string(dir.join("out/readme.txt"))?, "hello");
//! # std::fs::remove_dir_all(&dir).ok();
//! # Ok(())
//! # }
//! ```

use std::io::{self, BufReader, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::cancel::CancelToken;
pub use crate::file_ops::Progress;

/// An archive format this module reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Tar,
    /// A tar compressed with gzip (`.tar.gz`, `.tgz`).
    TarGz,
    Zip,
}

impl Format {
    /// The format of the file at `path`, by its first bytes, else its
    /// name; `None` for anything else (a lone `.gz` that is not a tar
    /// included).
    pub fn of(path: &Path) -> io::Result<Option<Format>> {
        let mut head = Vec::with_capacity(512);
        std::fs::File::open(path)?
            .take(512)
            .read_to_end(&mut head)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        Ok(
            if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
                Some(Format::Zip)
            } else if head.starts_with(b"\x1f\x8b") {
                let tar_named = name.ends_with(".tar.gz") || name.ends_with(".tgz");
                (tar_named || gz_holds_tar(path)).then_some(Format::TarGz)
            } else if head.get(257..262) == Some(b"ustar") || name.ends_with(".tar") {
                Some(Format::Tar)
            } else {
                None
            },
        )
    }
}

/// Whether the gzip file at `path` decompresses to a tar.
fn gz_holds_tar(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = Vec::with_capacity(512);
    let read = flate2::read::GzDecoder::new(file)
        .take(512)
        .read_to_end(&mut head);
    read.is_ok() && head.get(257..262) == Some(b"ustar")
}

/// One entry of an archive.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ArchiveEntry {
    /// Its path in the archive, `/`-separated, as written (a directory's
    /// without its trailing `/`).
    pub path: String,
    /// Its size, uncompressed.
    pub size: u64,
    pub dir: bool,
    /// Where a link points: a symbolic link's target, or a hard link's
    /// other path.
    pub link: Option<String>,
    /// Unix mode bits, when the archive keeps them.
    pub mode: Option<u32>,
    /// When it was last modified (a zip's time is read as UTC: zip keeps
    /// no time zone).
    pub mtime: Option<SystemTime>,
}

/// The entries of the archive at `path`, in the order they are stored.
/// An error of kind [`InvalidData`](io::ErrorKind::InvalidData) for a
/// file that is not one of the [formats](Format).
pub fn entries(path: &Path) -> io::Result<Vec<ArchiveEntry>> {
    match Format::of(path)? {
        Some(Format::Zip) => zip_entries(path),
        Some(Format::Tar) => tar_entries(BufReader::new(std::fs::File::open(path)?)),
        Some(Format::TarGz) => tar_entries(flate2::read::GzDecoder::new(BufReader::new(
            std::fs::File::open(path)?,
        ))),
        None => Err(not_an_archive(path)),
    }
}

fn not_an_archive(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{} is not a tar, tar.gz or zip archive", path.display()),
    )
}

/// What an extraction did.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Extracted {
    /// Files written, and directories made.
    pub files: usize,
    pub dirs: usize,
    /// Entries left out, and why: unsafe paths, links, names taken.
    pub skipped: Vec<(String, String)>,
    /// It was cancelled before the end (the file being written removed).
    pub cancelled: bool,
}

/// Extract the archive at `path` into `dest` (made if missing). See the
/// [module docs](self) for what is left out.
pub fn extract(path: &Path, dest: &Path) -> io::Result<Extracted> {
    extract_with(path, dest, &CancelToken::new(), &mut |_| {})
}

/// [`extract`], reporting progress (bytes and entries, counted from a
/// listing first) and stopping when `cancel` is cancelled.
pub fn extract_with(
    path: &Path,
    dest: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(&Progress),
) -> io::Result<Extracted> {
    let listed = entries(path)?;
    let mut out = Out {
        dest: dest.to_path_buf(),
        cancel,
        emit: progress,
        progress: Progress {
            total_items: listed.len(),
            total_bytes: listed.iter().filter(|e| !e.dir).map(|e| e.size).sum(),
            ..Progress::default()
        },
        done: Extracted::default(),
    };
    std::fs::create_dir_all(dest)?;
    (out.emit)(&out.progress);
    match Format::of(path)? {
        Some(Format::Zip) => zip_extract(path, &mut out)?,
        Some(Format::Tar) => tar_walk(
            BufReader::new(std::fs::File::open(path)?),
            &mut |entry, data| out.write(entry, data),
        )?,
        Some(Format::TarGz) => tar_walk(
            flate2::read::GzDecoder::new(BufReader::new(std::fs::File::open(path)?)),
            &mut |entry, data| out.write(entry, data),
        )?,
        None => return Err(not_an_archive(path)),
    }
    out.progress.current = None;
    (out.emit)(&out.progress);
    Ok(out.done)
}

/// Where an archive extracts by default, as Yazi's `extract` does: a
/// directory beside it named after it (`docs.tar.gz` → `docs`), with
/// `_1`, `_2`… if that name is taken.
pub fn default_dest(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "archive".into());
    let lower = name.to_lowercase();
    let stem = [".tar.gz", ".tgz", ".tar", ".zip"]
        .iter()
        .find(|ext| lower.ends_with(*ext) && lower.len() > ext.len())
        .map_or(name.as_str(), |ext| &name[..name.len() - ext.len()]);
    crate::file_ops::unique(&path.with_file_name(stem))
}

/// Where an extraction is.
struct Out<'a> {
    dest: PathBuf,
    cancel: &'a CancelToken,
    emit: &'a mut dyn FnMut(&Progress),
    progress: Progress,
    done: Extracted,
}

impl Out<'_> {
    /// Write one entry, `data` its contents. `Err` only to stop.
    fn write(&mut self, entry: &ArchiveEntry, data: &mut dyn Read) -> io::Result<()> {
        if self.cancel.is_cancelled() {
            self.done.cancelled = true;
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        self.progress.items += 1;
        let skip = |done: &mut Extracted, why: &str| {
            done.skipped.push((entry.path.clone(), why.to_string()));
        };
        let Some(rel) = safe_path(&entry.path) else {
            skip(&mut self.done, "outside the destination");
            (self.emit)(&self.progress);
            return Ok(());
        };
        let target = self.dest.join(&rel);
        self.progress.current = Some(target.clone());
        if entry.link.is_some() {
            skip(&mut self.done, "a link");
        } else if entry.dir {
            match std::fs::create_dir_all(&target) {
                Ok(()) => self.done.dirs += 1,
                Err(error) => skip(&mut self.done, &error.to_string()),
            }
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)?;
            }
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&target)
            {
                Ok(mut file) => {
                    if let Err(error) = self.copy(data, &mut file) {
                        drop(file);
                        let _ = std::fs::remove_file(&target);
                        if error.kind() == io::ErrorKind::Interrupted {
                            self.done.cancelled = true;
                            return Err(error);
                        }
                        skip(&mut self.done, &error.to_string());
                    } else {
                        #[cfg(unix)]
                        if let Some(mode) = entry.mode {
                            use std::os::unix::fs::PermissionsExt;
                            let perms = std::fs::Permissions::from_mode(mode & 0o777);
                            let _ = std::fs::set_permissions(&target, perms);
                        }
                        if let Some(mtime) = entry.mtime {
                            let _ = file.set_modified(mtime);
                        }
                        self.done.files += 1;
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    skip(&mut self.done, "exists already");
                }
                Err(error) => skip(&mut self.done, &error.to_string()),
            }
        }
        (self.emit)(&self.progress);
        Ok(())
    }

    fn copy(&mut self, data: &mut dyn Read, file: &mut std::fs::File) -> io::Result<()> {
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            if self.cancel.is_cancelled() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = data.read(&mut buffer)?;
            if n == 0 {
                return Ok(());
            }
            io::Write::write_all(file, &buffer[..n])?;
            self.progress.bytes += n as u64;
            (self.emit)(&self.progress);
        }
    }
}

/// An entry's path as a relative path with only plain names, or `None`
/// when it is absolute or climbs out.
fn safe_path(path: &str) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for part in Path::new(path).components() {
        match part {
            Component::Normal(name) => out.push(name),
            Component::CurDir => {}
            _ => return None,
        }
    }
    // A Windows drive or root written with backslashes in a Unix archive
    // is a plain name on Unix; refuse it on any system.
    let text = out.to_string_lossy();
    if out.as_os_str().is_empty() || text.contains(':') || text.starts_with('\\') {
        return None;
    }
    Some(out)
}

fn zip_error(error: zip::result::ZipError) -> io::Error {
    match error {
        zip::result::ZipError::Io(error) => error,
        other => io::Error::new(io::ErrorKind::InvalidData, other.to_string()),
    }
}

fn zip_entries(path: &Path) -> io::Result<Vec<ArchiveEntry>> {
    let mut archive =
        zip::ZipArchive::new(BufReader::new(std::fs::File::open(path)?)).map_err(zip_error)?;
    (0..archive.len())
        .map(|i| {
            let file = archive.by_index_raw(i).map_err(zip_error)?;
            Ok(zip_entry(&file))
        })
        .collect()
}

fn zip_entry<R: Read>(file: &zip::read::ZipFile<'_, R>) -> ArchiveEntry {
    ArchiveEntry {
        path: file.name().trim_end_matches('/').to_string(),
        size: file.size(),
        dir: file.is_dir(),
        link: file.is_symlink().then(String::new),
        mode: file.unix_mode(),
        mtime: file.last_modified().and_then(|t| {
            let days = days_from_civil(t.year() as i64, t.month() as i64, t.day() as i64);
            let secs =
                days * 86_400 + t.hour() as i64 * 3600 + t.minute() as i64 * 60 + t.second() as i64;
            u64::try_from(secs)
                .ok()
                .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
        }),
    }
}

fn zip_extract(path: &Path, out: &mut Out<'_>) -> io::Result<()> {
    let mut archive =
        zip::ZipArchive::new(BufReader::new(std::fs::File::open(path)?)).map_err(zip_error)?;
    for i in 0..archive.len() {
        let mut file = match archive.by_index(i) {
            Ok(file) => file,
            Err(error) => {
                out.done.skipped.push((format!("#{i}"), error.to_string()));
                continue;
            }
        };
        let mut entry = zip_entry(&file);
        // zip's own check agrees with ours; an entry it refuses is skipped.
        if file.enclosed_name().is_none() {
            entry.path = format!("/{}", entry.path);
        }
        match out.write(&entry, &mut file) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(()),
            other => other?,
        }
    }
    Ok(())
}

/// Days from 1970-01-01 to a date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn tar_entries(reader: impl Read) -> io::Result<Vec<ArchiveEntry>> {
    let mut found = Vec::new();
    tar_walk(reader, &mut |entry, _| {
        found.push(entry.clone());
        Ok(())
    })?;
    Ok(found)
}

/// A number in a tar header: octal text, or base-256 when the first
/// byte's high bit is set (GNU, for big sizes).
fn tar_number(field: &[u8]) -> u64 {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return field[1..]
            .iter()
            .fold(u64::from(field[0] & 0x7f), |n, b| (n << 8) | u64::from(*b));
    }
    let text: String = field
        .iter()
        .take_while(|b| **b != 0)
        .map(|b| *b as char)
        .collect();
    u64::from_str_radix(text.trim(), 8).unwrap_or(0)
}

fn tar_text(field: &[u8]) -> String {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// Go through a tar stream, calling `each` with every entry and a reader
/// of its data (read or not: the rest is skipped). Stops at the end
/// marker, the end of the stream, or when `each` returns
/// [`Interrupted`](io::ErrorKind::Interrupted) (not an error).
fn tar_walk(
    mut reader: impl Read,
    each: &mut dyn FnMut(&ArchiveEntry, &mut dyn Read) -> io::Result<()>,
) -> io::Result<()> {
    let mut header = [0u8; 512];
    let (mut long_name, mut long_link): (Option<String>, Option<String>) = (None, None);
    loop {
        match read_block(&mut reader, &mut header)? {
            false => return Ok(()),
            true if header.iter().all(|b| *b == 0) => return Ok(()),
            true => {}
        }
        let size = tar_number(&header[124..136]);
        let kind = header[156];
        let padded = size.div_ceil(512) * 512;
        // Extensions that name the next entry.
        if matches!(kind, b'L' | b'K' | b'x') {
            let mut data = Vec::new();
            (&mut reader).take(size).read_to_end(&mut data)?;
            io::copy(&mut (&mut reader).take(padded - size), &mut io::sink())?;
            match kind {
                b'L' => long_name = Some(tar_text(&data)),
                b'K' => long_link = Some(tar_text(&data)),
                _ => {
                    for (key, value) in pax_records(&data) {
                        match key.as_str() {
                            "path" => long_name = Some(value),
                            "linkpath" => long_link = Some(value),
                            _ => {}
                        }
                    }
                }
            }
            continue;
        }
        let ustar = &header[257..262] == b"ustar";
        let mut path = tar_text(&header[0..100]);
        if ustar {
            let prefix = tar_text(&header[345..500]);
            if !prefix.is_empty() {
                path = format!("{prefix}/{path}");
            }
        }
        let path = long_name.take().unwrap_or(path);
        let link = long_link
            .take()
            .unwrap_or_else(|| tar_text(&header[157..257]));
        let dir = kind == b'5' || path.ends_with('/');
        let is_link = matches!(kind, b'1' | b'2');
        let entry = ArchiveEntry {
            path: path.trim_end_matches('/').to_string(),
            size: if dir || is_link { 0 } else { size },
            dir,
            link: is_link.then_some(link),
            mode: Some(tar_number(&header[100..108]) as u32),
            mtime: Some(
                SystemTime::UNIX_EPOCH + Duration::from_secs(tar_number(&header[136..148])),
            ),
        };
        let mut data = (&mut reader).take(size);
        // Files, directories and links; devices, FIFOs, a global pax
        // header, GNU sparse files and unknown types are skipped.
        let result = if matches!(kind, b'0' | 0 | b'7' | b'5' | b'1' | b'2') {
            each(&entry, &mut data)
        } else {
            Ok(())
        };
        io::copy(&mut data, &mut io::sink())?;
        io::copy(&mut (&mut reader).take(padded - size), &mut io::sink())?;
        match result {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(()),
            other => other?,
        }
    }
}

/// Fill `block`; `false` at a clean end of the stream.
fn read_block(reader: &mut impl Read, block: &mut [u8; 512]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < block.len() {
        match reader.read(&mut block[filled..])? {
            0 if filled == 0 => return Ok(false),
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "a tar header cut short",
                ))
            }
            n => filled += n,
        }
    }
    Ok(true)
}

/// The `key=value` records of a pax header: `LEN key=value\n` each.
fn pax_records(data: &[u8]) -> Vec<(String, String)> {
    let mut records = Vec::new();
    let mut rest = data;
    while !rest.is_empty() {
        let Some(space) = rest.iter().position(|b| *b == b' ') else {
            break;
        };
        let Some(len) = std::str::from_utf8(&rest[..space])
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
        else {
            break;
        };
        if len <= space || len > rest.len() {
            break;
        }
        let record = String::from_utf8_lossy(&rest[space + 1..len]);
        if let Some((key, value)) = record.trim_end_matches('\n').split_once('=') {
            records.push((key.to_string(), value.to_string()));
        }
        rest = &rest[len..];
    }
    records
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn base(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rich-ext-archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A ustar header for `path`.
    fn header(path: &str, kind: u8, size: usize, link: &str) -> Vec<u8> {
        let mut h = vec![0u8; 512];
        h[..path.len()].copy_from_slice(path.as_bytes());
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[136..147].copy_from_slice(b"14707072000");
        h[156] = kind;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        h
    }

    fn tar(entries: &[(&str, u8, &[u8], &str)]) -> Vec<u8> {
        let mut out = Vec::new();
        for (path, kind, data, link) in entries {
            out.extend(header(path, *kind, data.len(), link));
            out.extend_from_slice(data);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        out.extend([0u8; 1024]);
        out
    }

    #[test]
    fn lists_and_extracts_a_tar_safely() {
        let dir = base("tar");
        let long = format!("{}/deep.txt", "d".repeat(120));
        let mut pax = Vec::new();
        let record = format!("path={long}\n");
        let len = record.len() + 4;
        pax.extend(format!("{len} {record}").as_bytes());
        let bytes = tar(&[
            ("docs/", b'5', b"", ""),
            ("docs/a.txt", b'0', b"alpha", ""),
            ("../escape.txt", b'0', b"no", ""),
            ("docs/link", b'2', b"", "/etc/passwd"),
            ("PaxHeader", b'x', &pax, ""),
            ("ignored-name", b'0', b"deep", ""),
        ]);
        let path = dir.join("t.tar");
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(Format::of(&path).unwrap(), Some(Format::Tar));
        let listed = entries(&path).unwrap();
        let paths: Vec<&str> = listed.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "docs",
                "docs/a.txt",
                "../escape.txt",
                "docs/link",
                long.as_str()
            ]
        );
        assert!(listed[0].dir);
        assert_eq!(listed[1].size, 5);
        assert_eq!(listed[3].link.as_deref(), Some("/etc/passwd"));
        let out = extract(&path, &dir.join("out")).unwrap();
        assert_eq!((out.files, out.dirs), (2, 1));
        assert_eq!(
            out.skipped,
            [
                (
                    "../escape.txt".to_string(),
                    "outside the destination".to_string()
                ),
                ("docs/link".to_string(), "a link".to_string()),
            ]
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("out/docs/a.txt")).unwrap(),
            "alpha"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("out").join(&long)).unwrap(),
            "deep"
        );
        assert!(!dir.join("escape.txt").exists());
        // Nothing is overwritten.
        let again = extract(&path, &dir.join("out")).unwrap();
        assert_eq!(again.files, 0);
        assert!(again.skipped.iter().any(|(_, why)| why == "exists already"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_gzipped_tar_and_a_zip() {
        let dir = base("gz");
        let path = dir.join("t.tgz");
        let mut gz = flate2::write::GzEncoder::new(
            std::fs::File::create(&path).unwrap(),
            flate2::Compression::fast(),
        );
        gz.write_all(&tar(&[("x.txt", b'0', b"xx", "")])).unwrap();
        gz.finish().unwrap();
        assert_eq!(Format::of(&path).unwrap(), Some(Format::TarGz));
        assert_eq!(entries(&path).unwrap()[0].path, "x.txt");
        let mut progress = Progress::default();
        let out = extract_with(&path, &dir.join("o"), &CancelToken::new(), &mut |p| {
            progress = p.clone()
        })
        .unwrap();
        assert_eq!(out.files, 1);
        assert_eq!((progress.bytes, progress.items), (2, 1));

        let zip_path = dir.join("z.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.add_directory("sub/", options).unwrap();
        zip.start_file("sub/b.txt", options).unwrap();
        zip.write_all(b"beta").unwrap();
        zip.finish().unwrap();
        let listed = entries(&zip_path).unwrap();
        assert_eq!(listed.len(), 2);
        assert!(listed[0].dir);
        assert_eq!((listed[1].path.as_str(), listed[1].size), ("sub/b.txt", 4));
        extract(&zip_path, &dir.join("z")).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.join("z/sub/b.txt")).unwrap(),
            "beta"
        );
        // Not an archive.
        std::fs::write(dir.join("plain.txt"), "hi").unwrap();
        let error = entries(&dir.join("plain.txt")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn destinations_and_numbers() {
        let dir = base("dest");
        assert_eq!(default_dest(&dir.join("docs.tar.gz")), dir.join("docs"));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        assert_eq!(default_dest(&dir.join("docs.zip")), dir.join("docs_1"));
        assert_eq!(tar_number(b"0000644\0"), 0o644);
        assert_eq!(tar_number(&[0x80, 0, 0, 1, 0]), 256);
        assert!(safe_path("/abs").is_none());
        assert!(safe_path("a/../../b").is_none());
        assert!(safe_path("C:/x").is_none());
        assert_eq!(safe_path("./a/b").unwrap(), Path::new("a/b"));
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2004, 8, 31), 12_661);
        std::fs::remove_dir_all(&dir).ok();
    }
}

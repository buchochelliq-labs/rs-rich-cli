//! File-type icons, colours and MIME types, for file managers and
//! pickers: the glyph and colour [Yazi](https://github.com/sxyazi/yazi)
//! shows by default for a file, a MIME type guessed from a name or from the
//! first bytes, Yazi's type colours, and whether a Nerd Font is likely in
//! use.
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies. The icon tables are Yazi's default presets (its
//! `theme-dark.toml` and `theme-light.toml`), whose glyphs and colours come
//! from [nvim-web-devicons](https://github.com/nvim-tree/nvim-web-devicons);
//! both MIT-licensed. The glyphs are Nerd Font symbols: a terminal without
//! a Nerd Font shows them as boxes, so check [`nerd_font`] first.
//!
//! ```
//! use std::path::Path;
//! use rich_ext::filetype::{icon_for, mime_for_path, style_for, FileFlags};
//!
//! let (glyph, colour) = icon_for(Path::new("main.rs"), false);
//! assert_eq!((glyph, colour), ("\u{E68B}", "#dea584"));
//! // A markup tag for a file list: `[#dea584]\u{E68B}[/] main.rs`.
//! let line = format!("[{colour}]{glyph}[/] main.rs");
//! # let _ = line;
//!
//! assert_eq!(mime_for_path(Path::new("photo.JPG")), Some("image/jpeg"));
//! assert_eq!(style_for("image/jpeg", FileFlags::default()), Some("yellow"));
//! assert_eq!(style_for("inode/directory", FileFlags::directory()), Some("blue"));
//! ```

mod icons;

use std::io::Read;
use std::path::{Path, PathBuf};

/// Which of Yazi's presets an icon's colour comes from: most glyphs have a
/// darker colour for a light terminal background.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Appearance {
    /// `theme-dark.toml`: for a dark background (the default).
    #[default]
    Dark,
    /// `theme-light.toml`: for a light background.
    Light,
}

/// A glyph and the colour to draw it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Icon {
    /// One Nerd Font symbol.
    pub glyph: &'static str,
    /// A `#rrggbb` colour, usable as a rich style.
    pub color: &'static str,
}

/// What kind of file an entry is, beyond its name: what Yazi's icon
/// conditions and type colours test. [`FileFlags::of`] reads them from the
/// file system; build one by hand for entries that are not on disk.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileFlags {
    /// A directory (or a link to one).
    pub dir: bool,
    /// A symbolic link.
    pub link: bool,
    /// A symbolic link whose target is missing.
    pub orphan: bool,
    /// Executable by its owner (Unix).
    pub exec: bool,
    /// A block device.
    pub block: bool,
    /// A character device.
    pub char: bool,
    /// A named pipe.
    pub fifo: bool,
    /// A socket.
    pub socket: bool,
    /// The sticky bit is set.
    pub sticky: bool,
    /// Under the cursor: Yazi shows an open folder for a hovered directory.
    pub hovered: bool,
}

impl FileFlags {
    /// A plain directory.
    pub fn directory() -> FileFlags {
        FileFlags {
            dir: true,
            ..FileFlags::default()
        }
    }

    /// The flags of the file at `path`, read without following a final
    /// link (whose target says only whether it is a directory or missing).
    pub fn of(path: &Path) -> std::io::Result<FileFlags> {
        let meta = std::fs::symlink_metadata(path)?;
        let mut flags = FileFlags::default();
        let mut target = Some(meta.clone());
        if meta.file_type().is_symlink() {
            flags.link = true;
            target = std::fs::metadata(path).ok();
            flags.orphan = target.is_none();
        }
        flags.dir = target.as_ref().is_some_and(|m| m.is_dir());
        #[cfg(unix)]
        {
            use std::os::unix::fs::{FileTypeExt, PermissionsExt};
            let kind = meta.file_type();
            flags.block = kind.is_block_device();
            flags.char = kind.is_char_device();
            flags.fifo = kind.is_fifo();
            flags.socket = kind.is_socket();
            let mode = meta.permissions().mode();
            flags.sticky = mode & 0o1000 != 0;
            // A link's own mode is 0777: whether it runs is its target's.
            let target_mode = target.as_ref().map_or(0, |m| m.permissions().mode());
            flags.exec = !flags.dir && target_mode & 0o100 != 0;
        }
        Ok(flags)
    }

    /// Whether Yazi's condition `name` holds (`dir`, `!dir`,
    /// `dir & hovered`, …).
    fn holds(&self, condition: &str) -> bool {
        condition.split('&').all(|term| {
            let term = term.trim();
            let (negated, name) = match term.strip_prefix('!') {
                Some(name) => (true, name),
                None => (false, term),
            };
            let value = match name {
                "dir" => self.dir,
                "link" => self.link,
                "orphan" => self.orphan,
                "exec" => self.exec,
                "block" => self.block,
                "char" => self.char,
                "fifo" => self.fifo,
                "sock" => self.socket,
                "sticky" => self.sticky,
                "hovered" => self.hovered,
                // Yazi's virtual "dummy" entries do not exist here.
                _ => false,
            };
            value != negated
        })
    }
}

/// An entry in one of the name tables: exactly, then in lower case, as
/// Yazi looks names up.
fn lookup(
    table: &'static [(&str, &str, &str, &str)],
    name: &str,
    appearance: Appearance,
) -> Option<Icon> {
    let find = |key: &str| {
        table
            .binary_search_by(|(entry, ..)| entry.as_bytes().cmp(key.as_bytes()))
            .ok()
            .map(|i| table[i])
    };
    let (_, glyph, dark, light) = find(name).or_else(|| find(&name.to_ascii_lowercase()))?;
    Some(Icon {
        glyph,
        color: match appearance {
            Appearance::Dark => dark,
            Appearance::Light => light,
        },
    })
}

/// The glyph and colour Yazi shows by default for the entry at `path`
/// (only its name is read), for a dark background: `("\u{E68B}", "#dea584")`
/// for `main.rs`, a folder for a directory.
///
/// ```
/// use std::path::Path;
/// use rich_ext::filetype::icon_for;
///
/// assert_eq!(icon_for(Path::new(".git"), true).1, "#00bcd4");
/// assert_eq!(icon_for(Path::new("unknown.zzz"), false), ("\u{F15B}", "#ffffff"));
/// ```
pub fn icon_for(path: &Path, is_dir: bool) -> (&'static str, &'static str) {
    let flags = FileFlags {
        dir: is_dir,
        ..FileFlags::default()
    };
    let icon = icon_with(path, flags, Appearance::Dark);
    (icon.glyph, icon.color)
}

/// [`icon_for`] with every condition Yazi tests (links, devices,
/// executables, the hovered directory) and either preset's colours.
///
/// As Yazi, a directory is matched by its name in the directory table, a
/// file by its name and then its extension (the last one: `gz` for
/// `a.tar.gz`), each exactly and then in lower case; only an entry no name
/// matches falls through to the conditions (an orphan link, a link, a
/// device, a directory, an executable, any other file).
///
/// ```
/// use std::path::Path;
/// use rich_ext::filetype::{icon_with, Appearance, FileFlags};
///
/// let exec = FileFlags { exec: true, ..FileFlags::default() };
/// assert_eq!(icon_with(Path::new("runme"), exec, Appearance::Dark).color, "#8bc34a");
/// assert_eq!(icon_with(Path::new("x.rs"), exec, Appearance::Light).color, "#6f5242");
/// ```
pub fn icon_with(path: &Path, flags: FileFlags, appearance: Appearance) -> Icon {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let named = if name.is_empty() {
        None
    } else if flags.dir {
        lookup(icons::DIRS, &name, appearance)
    } else {
        lookup(icons::FILES, &name, appearance).or_else(|| {
            let ext = path.extension()?.to_string_lossy();
            lookup(icons::EXTS, &ext, appearance)
        })
    };
    named.unwrap_or_else(|| {
        let (_, glyph, dark, light) = icons::CONDS
            .iter()
            .copied()
            .find(|(condition, ..)| flags.holds(condition))
            .unwrap_or(icons::CONDS[icons::CONDS.len() - 1]);
        Icon {
            glyph,
            color: match appearance {
                Appearance::Dark => dark,
                Appearance::Light => light,
            },
        }
    })
}

/// The style Yazi's default `[filetype]` rules give an entry's name, as a
/// rich style: images `yellow`, audio and video `magenta`, archives `red`,
/// documents (PDF, Word, RTF) `cyan`, an orphan link `on red`, an
/// executable `green` and a directory `blue`; `None` for anything else
/// (the default style). `mime` is the entry's MIME type (see
/// [`mime_for`]).
///
/// ```
/// use rich_ext::filetype::{style_for, FileFlags};
///
/// assert_eq!(style_for("application/x-tar", FileFlags::default()), Some("red"));
/// assert_eq!(style_for("video/mp4", FileFlags::default()), Some("magenta"));
/// assert_eq!(style_for("text/plain", FileFlags::default()), None);
/// ```
pub fn style_for(mime: &str, flags: FileFlags) -> Option<&'static str> {
    let (kind, sub) = mime.split_once('/').unwrap_or((mime, ""));
    let sub = sub.strip_prefix("x-").unwrap_or(sub);
    let archive = [
        "zip", "rar", "vnd.rar", "tar", "gzip", "xz", "zstd", "lzma", "compress", "archive",
        "cpio", "arj", "xar",
    ];
    let archive_prefix = ["7z", "bzip", "ms-cab", "vnd.ms-cab"];
    Some(match kind {
        "image" => "yellow",
        "audio" | "video" => "magenta",
        "application" if archive.contains(&sub) => "red",
        "application" if archive_prefix.iter().any(|p| sub.starts_with(p)) => "red",
        "application" if matches!(sub, "pdf" | "doc" | "msword" | "rtf") => "cyan",
        _ if flags.orphan => "on red",
        _ if flags.exec => "green",
        _ if flags.dir => "blue",
        _ => return None,
    })
}

/// MIME types by extension (lower case), sorted for a binary search.
const BY_EXTENSION: &[(&str, &str)] = &[
    ("7z", "application/x-7z-compressed"),
    ("aac", "audio/aac"),
    ("aiff", "audio/aiff"),
    ("apng", "image/apng"),
    ("avi", "video/x-msvideo"),
    ("avif", "image/avif"),
    ("bash", "text/x-shellscript"),
    ("bmp", "image/bmp"),
    ("bz2", "application/x-bzip2"),
    ("c", "text/x-c"),
    ("cab", "application/vnd.ms-cab-compressed"),
    ("cc", "text/x-c++"),
    ("cpio", "application/x-cpio"),
    ("cpp", "text/x-c++"),
    ("cs", "text/x-csharp"),
    ("css", "text/css"),
    ("csv", "text/csv"),
    ("deb", "application/vnd.debian.binary-package"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("epub", "application/epub+zip"),
    ("flac", "audio/flac"),
    ("gif", "image/gif"),
    ("go", "text/x-go"),
    ("gz", "application/gzip"),
    ("h", "text/x-c"),
    ("heic", "image/heic"),
    ("hpp", "text/x-c++"),
    ("htm", "text/html"),
    ("html", "text/html"),
    ("ico", "image/vnd.microsoft.icon"),
    ("ini", "text/plain"),
    ("iso", "application/x-iso9660-image"),
    ("jar", "application/java-archive"),
    ("java", "text/x-java"),
    ("jpeg", "image/jpeg"),
    ("jpg", "image/jpeg"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("jsonl", "application/jsonl"),
    ("jxl", "image/jxl"),
    ("log", "text/plain"),
    ("lua", "text/x-lua"),
    ("lz", "application/x-lzip"),
    ("lzma", "application/x-lzma"),
    ("m4a", "audio/mp4"),
    ("md", "text/markdown"),
    ("mid", "audio/midi"),
    ("mjs", "text/javascript"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
    ("mp3", "audio/mpeg"),
    ("mp4", "video/mp4"),
    ("mpeg", "video/mpeg"),
    ("mpg", "video/mpeg"),
    ("odp", "application/vnd.oasis.opendocument.presentation"),
    ("ods", "application/vnd.oasis.opendocument.spreadsheet"),
    ("odt", "application/vnd.oasis.opendocument.text"),
    ("oga", "audio/ogg"),
    ("ogg", "audio/ogg"),
    ("ogv", "video/ogg"),
    ("opus", "audio/opus"),
    ("otf", "font/otf"),
    ("pdf", "application/pdf"),
    ("php", "text/x-php"),
    ("pl", "text/x-perl"),
    ("png", "image/png"),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("psd", "image/vnd.adobe.photoshop"),
    ("py", "text/x-python"),
    ("rar", "application/vnd.rar"),
    ("rb", "text/x-ruby"),
    ("rpm", "application/x-rpm"),
    ("rs", "text/x-rust"),
    ("rtf", "application/rtf"),
    ("sh", "text/x-shellscript"),
    ("sql", "application/sql"),
    ("sqlite", "application/vnd.sqlite3"),
    ("svg", "image/svg+xml"),
    ("swift", "text/x-swift"),
    ("tar", "application/x-tar"),
    ("tgz", "application/gzip"),
    ("tif", "image/tiff"),
    ("tiff", "image/tiff"),
    ("toml", "application/toml"),
    ("ts", "text/x-typescript"),
    ("tsv", "text/tab-separated-values"),
    ("ttf", "font/ttf"),
    ("txt", "text/plain"),
    ("wasm", "application/wasm"),
    ("wav", "audio/wav"),
    ("webm", "video/webm"),
    ("webp", "image/webp"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("xml", "application/xml"),
    ("xz", "application/x-xz"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("zip", "application/zip"),
    ("zsh", "text/x-shellscript"),
    ("zst", "application/zstd"),
];

/// The MIME type of a file named like `path`, by its extension (in any
/// case), or `None` for an extension not in the table (about a hundred
/// common ones: images, audio, video, archives, documents, fonts, source
/// code and data formats).
pub fn mime_for_path(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    BY_EXTENSION
        .binary_search_by(|(entry, _)| entry.cmp(&ext.as_str()))
        .ok()
        .map(|i| BY_EXTENSION[i].1)
}

/// The MIME type that the first bytes of a file announce: the magic
/// numbers of common images, audio and video containers, archives and
/// compressors, PDF, fonts, SQLite, WebAssembly and executables (ELF,
/// Mach-O, PE). `None` when nothing matches; [`mime_for`] then falls back
/// to the name and to telling text from binary. 512 bytes are enough for
/// every format here (a tar header's `ustar` is at 257).
///
/// ```
/// use rich_ext::filetype::mime_from_bytes;
///
/// assert_eq!(mime_from_bytes(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
/// assert_eq!(mime_from_bytes(b"%PDF-1.7"), Some("application/pdf"));
/// assert_eq!(mime_from_bytes(b"hello"), None);
/// ```
pub fn mime_from_bytes(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    let at = |offset: usize, magic: &[u8]| bytes.get(offset..offset + magic.len()) == Some(magic);
    Some(if starts(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if starts(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        "image/gif"
    } else if starts(b"RIFF") && at(8, b"WEBP") {
        "image/webp"
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        "audio/wav"
    } else if starts(b"RIFF") && at(8, b"AVI ") {
        "video/x-msvideo"
    } else if starts(b"BM") && bytes.len() >= 14 && at(6, b"\0\0\0\0") {
        "image/bmp"
    } else if starts(b"\0\0\x01\0") {
        "image/vnd.microsoft.icon"
    } else if starts(b"II*\0") || starts(b"MM\0*") {
        "image/tiff"
    } else if starts(b"8BPS") {
        "image/vnd.adobe.photoshop"
    } else if starts(b"\xff\x0a") || starts(b"\0\0\0\x0cJXL \r\n\x87\n") {
        "image/jxl"
    } else if at(4, b"ftyp") {
        match bytes.get(8..12) {
            Some(b"avif") | Some(b"avis") => "image/avif",
            Some(b"heic") | Some(b"heix") | Some(b"mif1") => "image/heic",
            Some(b"qt  ") => "video/quicktime",
            Some(b"M4A ") => "audio/mp4",
            _ => "video/mp4",
        }
    } else if starts(b"\x1a\x45\xdf\xa3") {
        if bytes.windows(4).take(64).any(|w| w == b"webm") {
            "video/webm"
        } else {
            "video/x-matroska"
        }
    } else if starts(b"OggS") {
        "audio/ogg"
    } else if starts(b"fLaC") {
        "audio/flac"
    } else if starts(b"ID3") || (bytes.len() >= 2 && bytes[0] == 0xff && bytes[1] & 0xe6 == 0xe2) {
        "audio/mpeg"
    } else if starts(b"MThd") {
        "audio/midi"
    } else if starts(b"%PDF-") {
        "application/pdf"
    } else if starts(b"{\\rtf") {
        "application/rtf"
    } else if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") {
        "application/zip"
    } else if starts(b"\x1f\x8b") {
        "application/gzip"
    } else if starts(b"\xfd7zXZ\0") {
        "application/x-xz"
    } else if starts(b"\x28\xb5\x2f\xfd") {
        "application/zstd"
    } else if starts(b"BZh") {
        "application/x-bzip2"
    } else if starts(b"7z\xbc\xaf\x27\x1c") {
        "application/x-7z-compressed"
    } else if starts(b"Rar!\x1a\x07") {
        "application/vnd.rar"
    } else if starts(b"MSCF") {
        "application/vnd.ms-cab-compressed"
    } else if starts(b"LZIP") {
        "application/x-lzip"
    } else if at(257, b"ustar") {
        "application/x-tar"
    } else if starts(b"!<arch>\ndebian") {
        "application/vnd.debian.binary-package"
    } else if starts(b"\xed\xab\xee\xdb") {
        "application/x-rpm"
    } else if starts(b"SQLite format 3\0") {
        "application/vnd.sqlite3"
    } else if starts(b"\0asm") {
        "application/wasm"
    } else if starts(b"wOFF") {
        "font/woff"
    } else if starts(b"wOF2") {
        "font/woff2"
    } else if starts(b"OTTO") {
        "font/otf"
    } else if starts(b"\0\x01\0\0\0") {
        "font/ttf"
    } else if starts(b"\x7fELF") {
        "application/x-executable"
    } else if starts(b"\xcf\xfa\xed\xfe")
        || starts(b"\xce\xfa\xed\xfe")
        || starts(b"\xca\xfe\xba\xbe")
    {
        "application/x-mach-binary"
    } else if starts(b"MZ") {
        "application/vnd.microsoft.portable-executable"
    } else {
        return None;
    })
}

/// Whether `bytes` (a file's start) read as text: valid UTF-8, a character
/// cut at the end allowed, with no NUL and few other controls.
fn looks_like_text(bytes: &[u8]) -> bool {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        // A character cut at the end of the sample is still text.
        Err(e) if e.error_len().is_none() => {
            std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => return false,
    };
    let controls = text
        .chars()
        .filter(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\x0c' | '\x1b'))
        .count();
    !text.contains('\0') && controls * 100 <= text.len().max(1)
}

/// The MIME type of the file at `path`, as a file manager wants it:
/// `inode/directory` for a directory, `inode/x-empty` for an empty file,
/// then the [magic bytes](mime_from_bytes) of its first 512 bytes, then
/// its [extension](mime_for_path), then `text/plain` if those bytes read as
/// text and `application/octet-stream` if not. Reads at most 512 bytes.
///
/// ```
/// let dir = std::env::temp_dir();
/// assert_eq!(rich_ext::filetype::mime_for(&dir).unwrap(), "inode/directory");
/// ```
pub fn mime_for(path: &Path) -> std::io::Result<&'static str> {
    let meta = std::fs::metadata(path)?;
    if meta.is_dir() {
        return Ok("inode/directory");
    }
    if !meta.is_file() {
        return Ok(mime_for_path(path).unwrap_or("inode/x-special"));
    }
    if meta.len() == 0 {
        return Ok("inode/x-empty");
    }
    let mut head = Vec::with_capacity(512);
    std::fs::File::open(path)?
        .take(512)
        .read_to_end(&mut head)?;
    // A container's magic (a ZIP's `PK`) says less than a known extension
    // does: .docx, .xlsx, .epub and .jar are all ZIPs. Anything more
    // specific, a PNG named .txt say, wins over the name.
    let magic = mime_from_bytes(&head);
    let named = mime_for_path(path);
    let generic = matches!(magic, Some("application/zip"));
    Ok(if generic {
        named.or(magic)
    } else {
        magic.or(named)
    }
    .unwrap_or(if looks_like_text(&head) {
        "text/plain"
    } else {
        "application/octet-stream"
    }))
}

/// The MIME types of `paths`, each as [`mime_for`] sniffs it (at most 512
/// bytes read a file), `None` where a file cannot be read. A listing's
/// types from names alone miss a PNG called `.txt` or a script with no
/// extension; this is the sniffing Yazi's fetchers do, to run off a UI's
/// thread and merge into the listing when it is done.
///
/// ```
/// let dir = std::env::temp_dir().join(format!("rich-ext-mime-batch-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("photo.txt"), b"\x89PNG\r\n\x1a\n....").unwrap();
/// std::fs::write(dir.join("run"), "#!/bin/sh\necho hi\n").unwrap();
/// let types = rich_ext::filetype::mime_batch(&[dir.join("photo.txt"), dir.join("run"), dir.join("gone")]);
/// assert_eq!(types, [Some("image/png"), Some("text/plain"), None]);
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn mime_batch<P: AsRef<Path>>(paths: &[P]) -> Vec<Option<&'static str>> {
    paths.iter().map(|p| mime_for(p.as_ref()).ok()).collect()
}

/// Whether a Nerd Font is likely in use, and why we think so.
///
/// No terminal reports its font, and Nerd Font glyphs sit in the Private
/// Use Area, so a cursor-position probe measures them as one cell either
/// way: nothing can *detect* the font. [`nerd_font`] goes by what can be
/// known: an explicit setting, or a Nerd Font installed on this machine
/// (which a terminal may still not use, and which a remote session over
/// SSH cannot see at all).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NerdFont {
    /// `RICH_NERD_FONT` (or `NERD_FONT`) says so: `1`, `true`, `yes`, `on`;
    /// or `0`, `false`, `no`, `off`.
    Configured(bool),
    /// A Nerd Font file is installed: the first one found.
    Installed(PathBuf),
    /// No setting, and no Nerd Font in the usual font directories.
    NotFound,
}

impl NerdFont {
    /// Whether to show Nerd Font glyphs: as configured, or when one is
    /// installed.
    pub fn likely(&self) -> bool {
        match self {
            NerdFont::Configured(on) => *on,
            NerdFont::Installed(_) => true,
            NerdFont::NotFound => false,
        }
    }
}

/// Whether a Nerd Font is likely in use (see [`NerdFont`] for what can and
/// cannot be known). Reads `RICH_NERD_FONT`, then `NERD_FONT`; without
/// either, looks for a Nerd Font file (`*NerdFont*`, `*Nerd Font*`, or a
/// `NF`/`NFM`/`NFP` family suffix) in the user's and the system's font
/// directories, a bounded walk. Call it once and keep the answer.
///
/// ```
/// let icons = rich_ext::filetype::nerd_font().likely();
/// # let _ = icons;
/// ```
pub fn nerd_font() -> NerdFont {
    for name in ["RICH_NERD_FONT", "NERD_FONT"] {
        if let Some(on) = std::env::var(name).ok().and_then(|v| parse_switch(&v)) {
            return NerdFont::Configured(on);
        }
    }
    let mut budget = 20_000usize;
    font_dirs()
        .into_iter()
        .find_map(|dir| find_nerd_font(&dir, 4, &mut budget))
        .map_or(NerdFont::NotFound, NerdFont::Installed)
}

fn parse_switch(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Where fonts are installed on this platform.
fn font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(target_os = "macos") {
        dirs.extend(home.iter().map(|h| h.join("Library/Fonts")));
        dirs.push("/Library/Fonts".into());
    } else if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
        if let Some(windir) = std::env::var_os("WINDIR") {
            dirs.push(PathBuf::from(windir).join("Fonts"));
        }
    } else {
        if let Some(data) = std::env::var_os("XDG_DATA_HOME") {
            dirs.push(PathBuf::from(data).join("fonts"));
        }
        dirs.extend(home.iter().map(|h| h.join(".local/share/fonts")));
        dirs.extend(home.iter().map(|h| h.join(".fonts")));
        dirs.push("/usr/local/share/fonts".into());
        dirs.push("/usr/share/fonts".into());
    }
    dirs
}

/// Whether a font file's name says it is a Nerd Font.
fn is_nerd_font_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let Some(stem) = [".ttf", ".otf", ".ttc", ".woff2"]
        .iter()
        .find_map(|ext| lower.strip_suffix(ext))
    else {
        return false;
    };
    if stem.contains("nerdfont") || stem.contains("nerd font") || stem.contains("nerd_font") {
        return true;
    }
    // `JetBrainsMonoNLNFM-Regular`, `Hack NF Bold`: the family's suffix.
    let original = &name[..stem.len()];
    let family = original.split(['-', ' ', '_']).collect::<Vec<_>>();
    family
        .iter()
        .any(|part| matches!(*part, "NF" | "NFM" | "NFP"))
        || family.first().is_some_and(|f| {
            f.len() > 3 && (f.ends_with("NF") || f.ends_with("NFM") || f.ends_with("NFP"))
        })
}

/// The first Nerd Font file under `dir`, at most `depth` levels down,
/// looking at no more than `budget` entries in all.
fn find_nerd_font(dir: &Path, depth: usize, budget: &mut usize) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut subdirs = Vec::new();
    for entry in entries.flatten() {
        if *budget == 0 {
            return None;
        }
        *budget -= 1;
        let path = entry.path();
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        if is_dir {
            subdirs.push(path);
        } else if entry.file_name().to_str().is_some_and(is_nerd_font_file) {
            return Some(path);
        }
    }
    if depth == 0 {
        return None;
    }
    subdirs
        .into_iter()
        .find_map(|sub| find_nerd_font(&sub, depth - 1, budget))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tables_are_sorted_for_their_binary_searches() {
        for table in [icons::DIRS, icons::FILES, icons::EXTS] {
            assert!(table
                .windows(2)
                .all(|w| w[0].0.as_bytes() < w[1].0.as_bytes()));
            assert!(table
                .iter()
                .all(|(_, _, dark, light)| dark.len() == 7 && light.len() == 7));
        }
        assert!(BY_EXTENSION.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(BY_EXTENSION
            .iter()
            .all(|(e, _)| *e == e.to_ascii_lowercase()));
        // Every colour is a style rich reads.
        for (_, _, dark, light) in icons::EXTS.iter().chain(icons::CONDS) {
            assert!(rich::Style::parse(dark).is_ok() && rich::Style::parse(light).is_ok());
        }
    }

    #[test]
    fn names_then_extensions_then_conditions() {
        let dark = Appearance::Dark;
        let file = FileFlags::default();
        // A file's own name wins over its extension.
        let make = icon_with(Path::new("Makefile"), file, dark);
        assert_eq!(make, lookup(icons::FILES, "makefile", dark).unwrap());
        let ignore = icon_with(Path::new(".gitignore"), file, dark);
        assert_eq!(ignore.color, "#f54d27");
        let lock = icon_with(Path::new("Cargo.lock"), file, dark);
        assert_eq!(lock, lookup(icons::EXTS, "lock", dark).unwrap());
        // In any case, exact first: `R` and `r` are different entries.
        assert_eq!(
            icon_with(Path::new("A.RS"), file, dark),
            icon_with(Path::new("a.rs"), file, dark)
        );
        assert_eq!(
            icon_with(Path::new("x.R"), file, dark),
            lookup(icons::EXTS, "R", dark).unwrap()
        );
        // The last extension.
        assert_eq!(
            icon_with(Path::new("a.tar.gz"), file, dark),
            lookup(icons::EXTS, "gz", dark).unwrap()
        );
        // A directory goes by the directory table, not the file one.
        assert_eq!(
            icon_for(Path::new("Downloads"), true),
            ("\u{F498}", "#00bcd4")
        );
        assert_eq!(icon_for(Path::new("x.rs"), true), ("\u{E5FF}", "#03a9f4"));
        let hovered = FileFlags {
            hovered: true,
            ..FileFlags::directory()
        };
        assert_eq!(icon_with(Path::new("src"), hovered, dark).glyph, "\u{E5FE}");
        // The conditions, in Yazi's order: an orphan link before a link.
        let orphan = FileFlags {
            link: true,
            orphan: true,
            ..file
        };
        assert_eq!(icon_with(Path::new("gone"), orphan, dark).glyph, "\u{F127}");
        let link = FileFlags { link: true, ..file };
        assert_eq!(icon_with(Path::new("here"), link, dark).glyph, "\u{F481}");
        assert_eq!(
            icon_for(Path::new("README"), false).0,
            lookup(icons::FILES, "readme", dark).unwrap().glyph
        );
        // A light background has its own colours.
        assert_eq!(
            icon_with(Path::new("x.zzz"), file, Appearance::Light).color,
            "#000000"
        );
        assert_eq!(icon_for(Path::new(""), false), ("\u{F15B}", "#ffffff"));
    }

    #[test]
    fn flags_from_the_file_system() {
        let dir = std::env::temp_dir().join(format!("rich-ext-filetype-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.txt");
        std::fs::write(&file, "hello\n").unwrap();
        assert_eq!(FileFlags::of(&dir).unwrap(), FileFlags::directory());
        assert!(!FileFlags::of(&file).unwrap().dir);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = dir.join("run");
            std::fs::write(&script, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
            assert!(FileFlags::of(&script).unwrap().exec);
            let link = dir.join("link");
            let _ = std::fs::remove_file(&link);
            std::os::unix::fs::symlink(dir.join("missing"), &link).unwrap();
            let flags = FileFlags::of(&link).unwrap();
            assert!(flags.link && flags.orphan && !flags.dir);
            assert_eq!(style_for("inode/x-empty", flags), Some("on red"));
            // A link runs only if what it points to does (its own mode is
            // 0777 on Linux).
            let to_text = dir.join("to-text");
            let _ = std::fs::remove_file(&to_text);
            std::os::unix::fs::symlink(&file, &to_text).unwrap();
            assert!(!FileFlags::of(&to_text).unwrap().exec);
            let to_script = dir.join("to-script");
            let _ = std::fs::remove_file(&to_script);
            std::os::unix::fs::symlink(&script, &to_script).unwrap();
            assert!(FileFlags::of(&to_script).unwrap().exec);
        }
        assert_eq!(mime_for(&file).unwrap(), "text/plain");
        let png = dir.join("picture.dat");
        std::fs::write(&png, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(mime_for(&png).unwrap(), "image/png");
        // A ZIP's magic gives way to a known extension; a PNG's does not.
        let document = dir.join("report.docx");
        std::fs::write(&document, b"PK\x03\x04\x14\0\0\0").unwrap();
        assert_eq!(
            mime_for(&document).unwrap(),
            mime_for_path(&document).unwrap()
        );
        assert_ne!(mime_for(&document).unwrap(), "application/zip");
        let archive = dir.join("bundle.bin");
        std::fs::write(&archive, b"PK\x03\x04\x14\0\0\0").unwrap();
        assert_eq!(mime_for(&archive).unwrap(), "application/zip");
        let disguised = dir.join("notes.txt");
        std::fs::write(&disguised, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(mime_for(&disguised).unwrap(), "image/png");
        let empty = dir.join("empty.rs");
        std::fs::write(&empty, b"").unwrap();
        assert_eq!(mime_for(&empty).unwrap(), "inode/x-empty");
        let binary = dir.join("blob");
        std::fs::write(&binary, [0u8, 1, 2, 3, 200, 201]).unwrap();
        assert_eq!(mime_for(&binary).unwrap(), "application/octet-stream");
        let source = dir.join("lib.rs");
        std::fs::write(&source, "fn main() {}\n").unwrap();
        assert_eq!(mime_for(&source).unwrap(), "text/x-rust");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn magic_numbers() {
        let mut tar = vec![0u8; 512];
        tar[257..262].copy_from_slice(b"ustar");
        let cases: &[(&[u8], &str)] = &[
            (b"\xff\xd8\xff\xe0", "image/jpeg"),
            (b"GIF89a", "image/gif"),
            (b"RIFF\0\0\0\0WEBPVP8 ", "image/webp"),
            (b"RIFF\0\0\0\0WAVEfmt ", "audio/wav"),
            (b"\0\0\0\x20ftypisom", "video/mp4"),
            (b"\0\0\0\x1cftypavif", "image/avif"),
            (
                b"\x1a\x45\xdf\xa3\x01\0\0\0\0\0\0\x1fB\x82\x84webm",
                "video/webm",
            ),
            (b"OggS\0", "audio/ogg"),
            (b"ID3\x04", "audio/mpeg"),
            (b"PK\x03\x04", "application/zip"),
            (b"\x1f\x8b\x08", "application/gzip"),
            (b"\xfd7zXZ\0", "application/x-xz"),
            (b"\x28\xb5\x2f\xfd", "application/zstd"),
            (b"7z\xbc\xaf\x27\x1c", "application/x-7z-compressed"),
            (&tar, "application/x-tar"),
            (b"SQLite format 3\0", "application/vnd.sqlite3"),
            (b"\0asm\x01\0\0\0", "application/wasm"),
            (b"\x7fELF\x02\x01", "application/x-executable"),
            (b"MZ\x90\0", "application/vnd.microsoft.portable-executable"),
            (b"wOF2", "font/woff2"),
        ];
        for (bytes, mime) in cases {
            assert_eq!(mime_from_bytes(bytes), Some(*mime), "{mime}");
        }
        assert_eq!(mime_from_bytes(b""), None);
        assert_eq!(mime_from_bytes(b"plain words"), None);
        assert!(looks_like_text("héllo wörld\n".as_bytes()));
        // A character cut by the sample's end.
        assert!(looks_like_text(&"aé".as_bytes()[..2]));
        assert!(!looks_like_text(b"a\0b"));
    }

    #[test]
    fn type_colours() {
        let file = FileFlags::default();
        for (mime, style) in [
            ("image/png", Some("yellow")),
            ("audio/flac", Some("magenta")),
            ("application/zip", Some("red")),
            ("application/x-7z-compressed", Some("red")),
            ("application/x-bzip2", Some("red")),
            ("application/vnd.rar", Some("red")),
            ("application/pdf", Some("cyan")),
            ("application/msword", Some("cyan")),
            ("application/json", None),
        ] {
            assert_eq!(style_for(mime, file), style, "{mime}");
        }
        let exec = FileFlags { exec: true, ..file };
        assert_eq!(style_for("application/x-executable", exec), Some("green"));
        // A type colour wins over the kind, as Yazi's rules are ordered.
        assert_eq!(style_for("image/png", exec), Some("yellow"));
    }

    #[test]
    fn nerd_font_names() {
        for name in [
            "JetBrainsMonoNerdFont-Regular.ttf",
            "Fira Code Nerd Font Complete.otf",
            "SymbolsNerdFontMono-Regular.ttf",
            "HackNerdFont-Bold.ttf",
            "JetBrainsMonoNLNFM-Regular.ttf",
            "Hack NF Bold.ttf",
            "CaskaydiaCoveNF-Regular.ttf",
        ] {
            assert!(is_nerd_font_file(name), "{name}");
        }
        for name in [
            "DejaVuSansMono.ttf",
            "NFL.ttf",
            "Inter-Regular.otf",
            "nerd.txt",
        ] {
            assert!(!is_nerd_font_file(name), "{name}");
        }
        assert_eq!(parse_switch(" Yes "), Some(true));
        assert_eq!(parse_switch("off"), Some(false));
        assert_eq!(parse_switch("maybe"), None);
        assert!(NerdFont::Installed(PathBuf::from("a.ttf")).likely());
        assert!(!NerdFont::NotFound.likely());
        // Whatever this machine has, the check finishes.
        let _ = nerd_font();
    }
}

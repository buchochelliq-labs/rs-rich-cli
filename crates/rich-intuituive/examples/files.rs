//! A file manager in the style of [Yazi](https://github.com/sxyazi/yazi),
//! the most-starred app built on ratatui, rebuilt on intuiTUIve from its
//! ready-made components: the parent directory, the current one and a
//! preview of the hovered entry, loaded in the background.
//!
//! Ported from Yazi (<https://github.com/sxyazi/yazi>), by sxyazi and its
//! contributors, MIT licence. Its layout, keys, theme and behaviour are
//! theirs; this rebuild reuses none of its code. The icons and type colours
//! are Yazi's defaults, from rs-rich-ext's `filetype` (whose glyphs come
//! from nvim-web-devicons, MIT).
//!
//!     cargo run -p rs-rich-intuituive --example files [-- DIR] [--icons|--no-icons]
//!     cargo run -p rs-rich-intuituive --features watch,art --example files
//!
//! Icons need a Nerd Font: they are on with `--icons`, or when
//! `RICH_NERD_FONT=1` says one is in use; without them the rows and the
//! status line are plain text, as Yazi with its icons turned off.
//! `--backend NAME` (or `INTUITUIVE_BACKEND=NAME`) drives the terminal with
//! termion or termwiz instead of crossterm, when built with the crate's
//! feature of that name. Two of the crate's features add to it: `watch`
//! follows changes on disk as Yazi does (without it the view changes only
//! with what the app itself does), and `art` draws images in the preview
//! (without it an image is described).
//!
//! Yazi's default keys: j/k move, h/l leave and enter, gg/G top and
//! bottom, H/L back and forward, Enter or o opens with `$EDITOR` or the
//! system's opener, O asks which, Space selects, v visual mode, y/x yank
//! and cut, p paste, d trash, D delete, a create, r rename, / and ? find,
//! n/N the next match, f filter, . hidden files, the `,` keys sort, `m`
//! sets the line mode, `c` copies paths, `g` goes places, z jumps to a
//! file, Z to a directory visited, Tab shows a file's details, `t t` opens
//! a tab and 1-9, [ and ] switch, ~ or F1 shows the help, q quits. After
//! the first key of a sequence (g , c m t) a which-key pop-up shows what
//! can follow. Ctrl+P finds any command (not in Yazi).
//!
//! What changes files is real, and asked first: trashing moves to a
//! `.trash` folder in the directory the app started in (Yazi uses the
//! system's trash, which is outside it), deleting and overwriting say what
//! they will do and wait for Y.
//!
//! The mouse: a click hovers an entry (in the parent column it goes there,
//! in a directory's preview it enters it), the wheel moves the cursor or
//! scrolls the preview, a right click opens a menu, the column rails drag
//! and a click on a tab switches to it.
//!
//! Built on the framework's components: the columns are lazy `list`s in
//! `hsplit` panes with a marker column and the line mode trailing each
//! row, the preview a `scroll` or an `image`; key sequences and the
//! which-key pop-up are the app's; prompts are `input`s in pop-ups placed
//! as Yazi places them (`popup_at`), confirmations `cx.confirm`, errors
//! `cx.notify`, the help a `modal`, the jump the `Jump` component of
//! rs-rich-interact, the tab strip `tabs`, the menu a `context_menu`, and
//! an editor gets the terminal through `cx.handoff`.

use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use intuituive::interact::components::{Jump, JumpSource};
use intuituive::interact::{BackendKind, Button, MouseKind};
use intuituive::menu::{context_menu, MenuItem};
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::{Console, Segment, Style, Syntax};
use intuituive::widgets::{hsplit, list_lazy, tabs, RowRects, Scrollbar, Show};
use intuituive::{rule, Caret, Corner, Level, Offset, Origin, RectHandle};
use rich_ext::filetype::{self, Appearance, FileFlags};

// ---------------------------------------------------------------------
// The file system.
// ---------------------------------------------------------------------

/// One entry of a directory.
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
    pub exec: bool,
    /// The Unix mode bits of the entry itself (a link's are `l…`).
    pub mode: u32,
    pub mtime: Option<SystemTime>,
    pub btime: Option<SystemTime>,
    pub uid: u32,
    pub gid: u32,
}

impl Entry {
    /// What Yazi's icon and colour rules test.
    fn flags(&self, hovered: bool) -> FileFlags {
        FileFlags {
            dir: self.dir,
            link: self.link.is_some(),
            orphan: self.orphan,
            exec: self.exec,
            hovered,
            ..FileFlags::default()
        }
    }

    /// Its MIME type as far as its name tells (Yazi reads the first bytes
    /// too, in the background).
    fn mime(&self) -> &'static str {
        if self.dir {
            "inode/directory"
        } else {
            filetype::mime_for_path(Path::new(&self.name)).unwrap_or("")
        }
    }
}

/// What a directory is sorted by, as Yazi's `sort` command names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortBy {
    Alphabetical,
    Natural,
    Mtime,
    Btime,
    Extension,
    Size,
    Random,
}

/// A sort order: what by, and whether reversed. Directories come first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sort {
    pub by: SortBy,
    pub reverse: bool,
}

/// What the current column shows at the right of each entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Linemode {
    None,
    Size,
    Permissions,
    Mtime,
    Btime,
    Owner,
}

fn entry_of(dir: &Path, name: String) -> Entry {
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

/// The entries of `dir`, sorted, or why they can't be read.
pub fn read_dir(dir: &Path, hidden: bool, sort: Sort) -> Result<Vec<Entry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| e.to_string())?;
    let mut entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| hidden || !name.starts_with('.'))
        .map(|name| entry_of(dir, name))
        .collect();
    sort_entries(&mut entries, sort);
    Ok(entries)
}

fn sort_entries(entries: &mut [Entry], sort: Sort) {
    use std::cmp::Ordering;
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
            SortBy::Natural => natural(&a.name.to_lowercase(), &b.name.to_lowercase()),
            SortBy::Mtime => a.mtime.cmp(&b.mtime),
            SortBy::Btime => a.btime.cmp(&b.btime),
            SortBy::Extension => ext(a).cmp(&ext(b)),
            SortBy::Size => a.size.cmp(&b.size),
            SortBy::Random => scramble(&a.name).cmp(&scramble(&b.name)),
        }
        .then_with(|| alpha(a, b));
        let by = if sort.reverse { by.reverse() } else { by };
        // Directories first, whichever way the rest goes.
        match (a.dir, b.dir) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => by,
        }
    });
}

/// Names compared with their runs of digits as numbers: `a2` before `a10`.
fn natural(a: &str, b: &str) -> std::cmp::Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, _) => return std::cmp::Ordering::Less,
            (_, None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().filter(|c| c.is_ascii_digit()) {
                        n.push(*c);
                        it.next();
                    }
                    n
                };
                let (x, y) = (take(&mut a), take(&mut b));
                let (tx, ty) = (x.trim_start_matches('0'), y.trim_start_matches('0'));
                let order = tx.len().cmp(&ty.len()).then_with(|| tx.cmp(ty));
                if order.is_ne() {
                    return order;
                }
            }
            (Some(x), Some(y)) => {
                if x != y {
                    return x.cmp(&y);
                }
                a.next();
                b.next();
            }
        }
    }
}

/// A stable shuffle key for the random sort (FNV-1a of the name, salted
/// by the session).
fn scramble(name: &str) -> u64 {
    thread_local!(static SALT: u64 = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64));
    SALT.with(|salt| {
        name.bytes().fold(0xcbf29ce484222325 ^ salt, |h, b| {
            (h ^ b as u64).wrapping_mul(0x100000001b3)
        })
    })
}

/// A size as Yazi's `readable_size` writes it: `289B`, `1.4K`, `2K`.
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

/// `drwxr-xr-x`, as `ls` writes it.
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

/// A time as Yazi's line mode writes it, in the local time zone: `10/18
/// 14:05` this year, else `03/02  2024`.
pub fn short_time(time: SystemTime) -> String {
    rich_ext::fs::short_local_time(time)
}

/// `user:group`, by name where the system knows them (Yazi's owner line
/// mode), else by number.
pub fn owner(uid: u32, gid: u32) -> String {
    let user = rich_ext::fs::user_name(uid).unwrap_or_else(|| uid.to_string());
    let group = rich_ext::fs::group_name(gid).unwrap_or_else(|| gid.to_string());
    format!("{user}:{group}")
}

/// Where `query` occurs in `name`, smart-case (sensitive only when the
/// query has a capital), as byte ranges.
fn matches(name: &str, query: &str) -> Vec<(usize, usize)> {
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

fn smart_contains(name: &str, query: &str) -> bool {
    query.is_empty() || !matches(name, query).is_empty()
}

// ---------------------------------------------------------------------
// How an entry looks: Yazi's default theme.
// ---------------------------------------------------------------------

/// A list row's style for `entry`: the colour Yazi's file-type rules give
/// it, across the row, so the hovered row's reversed bar takes it.
fn row_style(entry: Option<&Entry>) -> Option<String> {
    let entry = entry?;
    filetype::style_for(entry.mime(), entry.flags(false)).map(str::to_string)
}

/// The marker column's style for an entry: selected, in the visual
/// range, or yanked (Yazi's marker colours).
fn marker_style(marker: Marker) -> Option<String> {
    let style = match marker {
        Marker::None => return None,
        Marker::Selected => "on bright_yellow",
        Marker::Marked => "on bright_cyan",
        Marker::Copied => "on bright_green",
        Marker::Cut => "on bright_red",
    };
    Some(style.into())
}

/// The mark at the left of an entry: selected, in the visual range, or
/// yanked.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Marker {
    None,
    Selected,
    Marked,
    Copied,
    Cut,
}

/// What an entry row says, as Yazi's entity draws it: the icon, the name
/// with what `find` matches picked out, the match's place on the hovered
/// row, and where a link points. The type's colour is the row's (see
/// [`row_style`]); a hovered row's icon takes it too, as Yazi's does.
fn entity(
    entry: &Entry,
    icons: bool,
    hovered: bool,
    find: Option<&str>,
    found: Option<(usize, usize)>,
) -> String {
    let mut out = String::new();
    if icons {
        let icon = filetype::icon_with(
            Path::new(&entry.name),
            entry.flags(hovered),
            Appearance::Dark,
        );
        if hovered {
            out.push_str(&format!("{} ", icon.glyph));
        } else {
            out.push_str(&format!("[{}]{}[/] ", icon.color, icon.glyph));
        }
    }
    let ranges = find.map(|q| matches(&entry.name, q)).unwrap_or_default();
    let mut last = 0;
    for (from, to) in ranges {
        out.push_str(&escape(&entry.name[last..from]));
        out.push_str(&format!(
            "[bold italic underline yellow]{}[/]",
            escape(&entry.name[from..to])
        ));
        last = to;
    }
    out.push_str(&escape(&entry.name[last..]));
    if let Some((at, of)) = found {
        out.push_str(&format!("  [bold italic magenta]\\[{at}/{of}][/]"));
    }
    if let Some(to) = &entry.link {
        out.push_str(&format!(
            "[italic] -> {}[/]",
            escape(&to.display().to_string())
        ));
    }
    out
}

/// The line mode's text for an entry, with Yazi's padding cell after it.
fn linemode_cell(entry: &Entry, mode: Linemode, path: &Path) -> String {
    let cell = match mode {
        Linemode::None => return String::new(),
        Linemode::Size if entry.dir => std::fs::read_dir(path)
            .map(|r| r.count().to_string())
            .unwrap_or_default(),
        Linemode::Size => readable_size(entry.size),
        Linemode::Permissions => permissions(entry.mode),
        Linemode::Mtime => entry.mtime.map(short_time).unwrap_or_default(),
        Linemode::Btime => entry.btime.map(short_time).unwrap_or_default(),
        Linemode::Owner => owner(entry.uid, entry.gid),
    };
    format!("{} ", escape(&cell))
}

/// The permissions coloured as Yazi's status line does.
fn coloured_permissions(mode: u32) -> String {
    permissions(mode)
        .chars()
        .enumerate()
        .map(|(i, c)| {
            let style = match c {
                '-' | '?' => "bright_black",
                'r' => "yellow",
                'w' => "red",
                'x' | 's' | 'S' | 't' | 'T' if i > 0 => "cyan",
                _ => "green",
            };
            format!("[{style}]{c}[/]")
        })
        .collect()
}

// ---------------------------------------------------------------------
// Previews.
// ---------------------------------------------------------------------

/// What the preview column shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    Nothing,
    /// A directory's entries, or why they can't be read.
    Dir(Result<Vec<Entry>, String>),
    /// The start of a text file, highlighted.
    Code(Vec<Vec<Segment>>),
    /// An image file, and what it is (shown when it is not drawn).
    Image(PathBuf, String),
    /// A line about the file (a binary, an error), in markup.
    Note(String),
}

/// The most lines a preview highlights (J and K seek through them).
const PREVIEW_LINES: usize = 500;

/// Load the preview of `path` (on a worker thread): a directory's entries,
/// an image, the start of a text file highlighted for `width` columns, or
/// a note on what the file is. Highlighting is the slow part, so it
/// happens here, off the app's thread, as Yazi does.
pub fn load_preview(path: &Path, width: u16, hidden: bool, sort: Sort) -> Preview {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return Preview::Note(format!("[red]{}", escape(&error.to_string()))),
    };
    if meta.is_dir() {
        return Preview::Dir(read_dir(path, hidden, sort));
    }
    use std::io::Read;
    let mut head = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(256 * 1024).read_to_end(&mut head));
    if let Err(error) = read {
        return Preview::Note(format!("[red]{}", escape(&error.to_string())));
    }
    if head.is_empty() {
        return Preview::Nothing;
    }
    let size = readable_size(meta.len());
    match filetype::mime_from_bytes(&head) {
        Some(mime) if mime.starts_with("image/") => {
            return Preview::Image(path.to_path_buf(), format!("[yellow]{mime}[/], {size}"));
        }
        _ => {}
    }
    if head.iter().take(8 * 1024).any(|b| *b == 0) {
        return Preview::Note(format!("[bright_black]Binary data, {size}"));
    }
    let language = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "txt".into());
    let text = String::from_utf8_lossy(&head);
    let shown: String = text
        .lines()
        .take(PREVIEW_LINES)
        .collect::<Vec<_>>()
        .join("\n");
    let width = width.max(1) as usize;
    let console = Console::builder().width(width).force_terminal(true).build();
    let mut options = console.options().update_width(width);
    options.no_wrap = Some(true);
    // Yazi's code preview: two-space tabs, no wrapping, and the theme's
    // colours on the terminal's own background.
    let syntax = Syntax::new(shown, language.as_str())
        .tab_size(2)
        .background_color("default");
    Preview::Code(console.render_lines(&syntax, &options, false))
}

// ---------------------------------------------------------------------
// Opening files: Yazi's openers and its rules for which to use.
// ---------------------------------------------------------------------

/// A way to open a file, as Yazi's `[opener]` defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Opener {
    /// `$EDITOR`, which takes the terminal until it ends.
    Edit,
    /// The system's opener (`$OPENER`, else `xdg-open`, `open` or
    /// `start`), in the background.
    Open,
    /// The same for a media file.
    Play,
    /// The system's opener on the file's directory.
    Reveal,
}

impl Opener {
    fn describe(self) -> &'static str {
        match self {
            Opener::Edit => "$EDITOR",
            Opener::Open => "Open",
            Opener::Play => "Play",
            Opener::Reveal => "Reveal",
        }
    }
}

/// The openers Yazi's `[open]` rules give a MIME type, the first one what
/// Enter uses.
pub fn openers_for(mime: &str) -> &'static [Opener] {
    let text = mime.starts_with("text/")
        || matches!(
            mime,
            "inode/x-empty"
                | "application/json"
                | "application/x-ndjson"
                | "application/javascript"
                | "application/toml"
                | "application/yaml"
                | "application/xml"
        );
    if mime == "inode/directory" {
        &[Opener::Edit, Opener::Open, Opener::Reveal]
    } else if text {
        &[Opener::Edit, Opener::Reveal]
    } else if mime.starts_with("audio/") || mime.starts_with("video/") {
        &[Opener::Play, Opener::Reveal]
    } else {
        &[Opener::Open, Opener::Reveal]
    }
}

/// The program and arguments in `$VAR`, else `fallback`.
fn command_from(var: &str, fallback: &str) -> std::process::Command {
    let line = std::env::var(var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string());
    let mut words = line.split_whitespace();
    let mut command = std::process::Command::new(words.next().unwrap_or(fallback));
    command.args(words);
    command
}

/// The system's opener: `$OPENER`, else the platform's.
fn system_opener() -> std::process::Command {
    if cfg!(windows) {
        let mut command = command_from("OPENER", "cmd");
        if std::env::var_os("OPENER").is_none() {
            command.args(["/C", "start", ""]);
        }
        command
    } else if cfg!(target_os = "macos") {
        command_from("OPENER", "open")
    } else {
        command_from("OPENER", "xdg-open")
    }
}

/// `$EDITOR`, else vi (Notepad on Windows).
pub fn editor() -> std::process::Command {
    command_from("EDITOR", if cfg!(windows) { "notepad" } else { "vi" })
}

/// Open `paths` with `opener`: the editor gets the terminal (Yazi's
/// `block = true`), the others run in the background.
fn open_with(fm: Fm, opener: Opener, paths: Vec<PathBuf>, cx: &mut Ctx) {
    if paths.is_empty() {
        return;
    }
    if opener == Opener::Edit {
        let mut command = editor();
        command.args(&paths).current_dir(fm.cwd.get_untracked());
        cx.handoff_then(command, move |ended, cx| {
            fm.refresh();
            if let Err(error) = ended {
                cx.notify(Level::Error, "Open", escape(&error.to_string()));
            }
        });
        return;
    }
    for path in paths {
        let target = match opener {
            Opener::Reveal => path.parent().map_or(path.clone(), Path::to_path_buf),
            _ => path,
        };
        spawn(
            move || {
                system_opener()
                    .arg(&target)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map_err(|e| e.to_string())
                    .and_then(|s| s.success().then_some(()).ok_or(s.to_string()))
            },
            |done: Result<(), String>, cx| {
                if let Err(error) = done {
                    cx.notify(Level::Error, "Open", escape(&error));
                }
            },
        );
    }
}

/// What the targets' types say to open them with: Enter's first opener of
/// each, or (interactively) the openers all of them share.
fn targets_openers(paths: &[PathBuf]) -> Vec<Opener> {
    let mut shared: Option<Vec<Opener>> = None;
    for path in paths {
        let mime = filetype::mime_for(path).unwrap_or("application/octet-stream");
        let these = openers_for(mime);
        shared = Some(match shared {
            None => these.to_vec(),
            Some(before) => before.into_iter().filter(|o| these.contains(o)).collect(),
        });
    }
    shared.unwrap_or_default()
}

// ---------------------------------------------------------------------
// The state.
// ---------------------------------------------------------------------

/// A tab: its directory, where it has been, and a name given to it.
#[derive(Clone, Debug, PartialEq)]
pub struct Tab {
    pub cwd: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    name: Option<String>,
}

impl Tab {
    fn new(cwd: PathBuf) -> Tab {
        Tab {
            cwd,
            back: Vec::new(),
            forward: Vec::new(),
            name: None,
        }
    }

    /// What the tab strip calls it.
    fn title(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.cwd
                .file_name()
                .map_or("/".into(), |n| n.to_string_lossy().into_owned())
        })
    }
}

/// Files yanked for pasting, copied or cut.
#[derive(Clone, Debug, PartialEq)]
struct Yank {
    paths: Vec<PathBuf>,
    cut: bool,
}

/// The whole app's state: signals and the memos read from them. All of it
/// is `Copy`, so any handler takes a copy.
#[derive(Clone, Copy)]
struct Fm {
    /// The directory the app started in: trash goes there.
    start: Signal<PathBuf>,
    icons: bool,
    tabs: Signal<Vec<Tab>>,
    active: Signal<usize>,
    cwd: Memo<PathBuf>,
    hidden: Signal<bool>,
    sort: Signal<Sort>,
    linemode: Signal<Linemode>,
    /// Bumped to read the directories again: after the app changed them,
    /// or when a watcher saw them change.
    tick: Signal<u64>,
    /// The current directory, unfiltered.
    listing: Memo<Result<Vec<Entry>, String>>,
    filter: Signal<String>,
    find: Signal<Option<String>>,
    /// What the current column lists: the listing, filtered.
    files: Memo<Vec<Entry>>,
    cursor: Signal<usize>,
    hovered: Memo<Option<Entry>>,
    parent: Memo<Option<Result<Vec<Entry>, String>>>,
    parent_cursor: Signal<usize>,
    /// The entry last hovered in each directory visited.
    remember: Signal<HashMap<PathBuf, String>>,
    selected: Signal<BTreeSet<PathBuf>>,
    /// Visual mode: where it started, and whether it unselects.
    visual: Signal<Option<(usize, bool)>>,
    yanked: Signal<Option<Yank>>,
    preview: Signal<Preview>,
    preview_top: Signal<u16>,
    preview_cursor: Signal<usize>,
    /// The first rows in view of each column's list, for the mouse.
    first: Signal<usize>,
    parent_first: Signal<usize>,
    preview_first: Signal<usize>,
    /// Where the current column's rows and the column itself are on the
    /// screen: the rename prompt opens below the hovered row, and Ctrl+D
    /// moves by half the column.
    rows: Signal<RowRects>,
    column: Signal<Option<RectHandle>>,
    /// The panes' shares of the width, which the rails drag.
    outer: Signal<f64>,
    inner: Signal<f64>,
}

impl Fm {
    fn new(start: PathBuf, icons: bool) -> Fm {
        let tabs = signal(vec![Tab::new(start.clone())]);
        let active = signal(0usize);
        let cwd = memo(move || tabs.with(|t| t[active.get().min(t.len() - 1)].cwd.clone()));
        let hidden = signal(false);
        let sort = signal(Sort {
            by: SortBy::Alphabetical,
            reverse: false,
        });
        let tick = signal(0u64);
        let listing = memo(move || {
            tick.get();
            let (hidden, sort) = (hidden.get(), sort.get());
            cwd.with(|dir| read_dir(dir, hidden, sort))
        });
        let filter = signal(String::new());
        let files = memo(move || {
            let filter = filter.get();
            listing.with(|l| match l {
                Ok(all) => all
                    .iter()
                    .filter(|e| smart_contains(&e.name, &filter))
                    .cloned()
                    .collect(),
                Err(_) => Vec::new(),
            })
        });
        let cursor = signal(0usize);
        let hovered = memo(move || {
            let at = cursor.get();
            files.with(|f| f.get(at.min(f.len().saturating_sub(1))).cloned())
        });
        let parent = memo(move || {
            tick.get();
            let (hidden, sort) = (hidden.get(), sort.get());
            cwd.with(|dir| dir.parent().map(|p| read_dir(p, hidden, sort)))
        });
        Fm {
            start: signal(start),
            icons,
            tabs,
            active,
            cwd,
            hidden,
            sort,
            linemode: signal(Linemode::None),
            tick,
            listing,
            filter,
            find: signal(None),
            files,
            cursor,
            hovered,
            parent,
            parent_cursor: signal(0),
            remember: signal(HashMap::new()),
            selected: signal(BTreeSet::new()),
            visual: signal(None),
            yanked: signal(None),
            preview: signal(Preview::Nothing),
            preview_top: signal(0),
            preview_cursor: signal(0),
            first: signal(0),
            parent_first: signal(0),
            preview_first: signal(0),
            rows: signal(RowRects::new()),
            column: signal(None),
            // Yazi's 1:4:3 columns.
            outer: signal(1.0 / 8.0),
            inner: signal(4.0 / 7.0),
        }
    }

    fn path_of(&self, entry: &Entry) -> PathBuf {
        self.cwd.get_untracked().join(&entry.name)
    }

    fn len(&self) -> usize {
        self.files.with_untracked(Vec::len)
    }

    /// The index of the entry called `name`, or 0.
    fn position(&self, name: Option<&str>) -> usize {
        name.and_then(|name| {
            self.files
                .with_untracked(|f| f.iter().position(|e| e.name == name))
        })
        .unwrap_or(0)
    }

    /// Move the cursor to `to`, kept in the listing; in visual mode the
    /// range follows it.
    fn move_to(&self, to: isize) {
        let len = self.len();
        let to = to.clamp(0, len.saturating_sub(1) as isize) as usize;
        self.cursor.set(to);
    }

    fn arrow(&self, by: isize) {
        self.move_to(self.cursor.get_untracked() as isize + by);
    }

    /// Move by a share of the column's height (Ctrl+D is half of it).
    fn page(&self, share: f64) {
        let rows = self
            .column
            .get_untracked()
            .and_then(|c| c.get_untracked())
            .map_or(1, |r| r.height.max(1)) as f64;
        let by = (rows * share).round() as isize;
        self.arrow(if by == 0 { share.signum() as isize } else { by });
    }

    /// Show `dir`, hovering `hover` there (else where it was last), and
    /// remember where this tab was for `H`.
    fn cd(&self, dir: PathBuf, hover: Option<String>) {
        let old = self.cwd.get_untracked();
        if dir != old {
            let i = self.active.get_untracked();
            self.tabs.update(|tabs| {
                let tab = &mut tabs[i];
                tab.back.push(old);
                tab.forward.clear();
                tab.cwd = dir;
            });
        }
        self.arrive(hover);
    }

    /// After the directory changed: a fresh view of it.
    fn arrive(&self, hover: Option<String>) {
        self.filter.set(String::new());
        self.find.set(None);
        self.visual.set(None);
        self.preview_top.set(0);
        let hover = hover.or_else(|| {
            let dir = self.cwd.get_untracked();
            self.remember.with_untracked(|r| r.get(&dir).cloned())
        });
        self.cursor.set(self.position(hover.as_deref()));
    }

    fn enter(&self) {
        if let Some(entry) = self.hovered.get_untracked().filter(|e| e.dir) {
            self.cd(self.path_of(&entry), None);
        }
    }

    fn leave(&self) {
        let dir = self.cwd.get_untracked();
        if let Some(up) = dir.parent() {
            let came_from = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            self.cd(up.to_path_buf(), came_from);
        }
    }

    /// Back (`H`) or forward (`L`) through this tab's history.
    fn history(&self, forward: bool) {
        let i = self.active.get_untracked();
        let mut moved = false;
        self.tabs.update(|tabs| {
            let tab = &mut tabs[i];
            let (from, to) = if forward {
                (&mut tab.forward, &mut tab.back)
            } else {
                (&mut tab.back, &mut tab.forward)
            };
            if let Some(dir) = from.pop() {
                to.push(std::mem::replace(&mut tab.cwd, dir));
                moved = true;
            }
        });
        if moved {
            self.arrive(None);
        }
    }

    /// The range visual mode covers, if it is on.
    fn visual_range(&self) -> Option<(usize, usize, bool)> {
        let (anchor, unset) = self.visual.get()?;
        let at = self.cursor.get();
        Some((anchor.min(at), anchor.max(at), unset))
    }

    /// End visual mode, keeping what it selected (or unselected).
    fn end_visual(&self) -> bool {
        let Some((anchor, unset)) = self.visual.get_untracked() else {
            return false;
        };
        let at = self.cursor.get_untracked();
        // A filter may have left fewer entries than the range, or none.
        let paths: Vec<PathBuf> = self.files.with_untracked(|f| {
            let (from, to) = (anchor.min(at), anchor.max(at));
            if from >= f.len() {
                return Vec::new();
            }
            f[from..=to.min(f.len() - 1)]
                .iter()
                .map(|e| self.path_of(e))
                .collect()
        });
        self.selected.update(|s| {
            for path in paths {
                if unset {
                    s.remove(&path);
                } else {
                    s.insert(path);
                }
            }
        });
        self.visual.set(None);
        true
    }

    /// What an action applies to: the selection, else the hovered entry.
    fn targets(&self) -> Vec<PathBuf> {
        self.end_visual();
        let chosen: Vec<PathBuf> = self
            .selected
            .with_untracked(|s| s.iter().cloned().collect());
        if !chosen.is_empty() {
            return chosen;
        }
        self.hovered
            .get_untracked()
            .map(|e| vec![self.path_of(&e)])
            .unwrap_or_default()
    }

    /// The marker for the entry at `index` in the current column.
    fn marker(&self, index: usize, path: &Path) -> Marker {
        if let Some((from, to, unset)) = self.visual_range() {
            if (from..=to).contains(&index) {
                return if unset { Marker::None } else { Marker::Marked };
            }
        }
        self.plain_marker(path)
    }

    /// The marker for `path` outside visual mode.
    fn plain_marker(&self, path: &Path) -> Marker {
        if self.selected.with(|s| s.contains(path)) {
            return Marker::Selected;
        }
        self.yanked.with(|y| match y {
            Some(y) if y.paths.iter().any(|p| p == path) => {
                if y.cut {
                    Marker::Cut
                } else {
                    Marker::Copied
                }
            }
            _ => Marker::None,
        })
    }

    /// Find the next (or previous) entry matching the find query, from
    /// the one after the cursor (or before), wrapping; `here` counts the
    /// cursor's own entry first.
    fn find_arrow(&self, previous: bool, here: bool) {
        let Some(query) = self.find.get_untracked().filter(|q| !q.is_empty()) else {
            return;
        };
        let names: Vec<String> = self
            .files
            .with_untracked(|f| f.iter().map(|e| e.name.clone()).collect());
        let len = names.len();
        if len == 0 {
            return;
        }
        let at = self.cursor.get_untracked();
        let start = if here { 0 } else { 1 };
        for step in start..=len {
            let i = if previous {
                (at + len * 2 - step) % len
            } else {
                (at + step) % len
            };
            if smart_contains(&names[i], &query) {
                self.cursor.set(i);
                return;
            }
        }
    }

    /// The hovered entry's place among the find query's matches.
    fn found(&self) -> Option<(usize, usize)> {
        let query = self.find.get().filter(|q| !q.is_empty())?;
        let at = self.cursor.get();
        self.files.with(|f| {
            let hits: Vec<usize> = (0..f.len())
                .filter(|&i| smart_contains(&f[i].name, &query))
                .collect();
            let place = hits.iter().position(|&i| i == at)?;
            Some((place + 1, hits.len()))
        })
    }

    /// Keep the hovered entry while `change` re-sorts or re-filters.
    fn keep(&self, change: impl FnOnce()) {
        let name = self.hovered.get_untracked().map(|e| e.name);
        change();
        self.cursor.set(self.position(name.as_deref()));
    }

    fn refresh(&self) {
        self.tick.update(|t| *t += 1);
    }

    fn new_tab(&self) {
        let here = self.cwd.get_untracked();
        let at = self.active.get_untracked() + 1;
        self.tabs.update(|tabs| tabs.insert(at, Tab::new(here)));
        self.switch_tab(at);
    }

    fn switch_tab(&self, to: usize) {
        let count = self.tabs.with_untracked(Vec::len);
        if to < count && to != self.active.get_untracked() {
            self.end_visual();
            self.active.set(to);
            self.arrive(None);
        }
    }

    /// Close this tab, or quit if it is the last.
    fn close_tab(&self, cx: &mut Ctx) {
        let count = self.tabs.with_untracked(Vec::len);
        if count <= 1 {
            cx.quit();
            return;
        }
        let i = self.active.get_untracked();
        self.tabs.update(|tabs| {
            tabs.remove(i);
        });
        self.active.set(i.min(count - 2));
        self.arrive(None);
    }

    fn swap_tab(&self, by: isize) {
        let count = self.tabs.with_untracked(Vec::len) as isize;
        let i = self.active.get_untracked() as isize;
        let j = (i + by).rem_euclid(count);
        if count > 1 {
            self.tabs.update(|tabs| tabs.swap(i as usize, j as usize));
            self.active.set(j as usize);
        }
    }
}

// ---------------------------------------------------------------------
// Changing files: always inside what the user is looking at, and only
// after they said so.
// ---------------------------------------------------------------------

/// A name typed into a prompt as a path under `dir`: relative, and never
/// climbing out of it.
fn under(dir: &Path, typed: &str) -> Result<PathBuf, String> {
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

/// `path`, or `path` with `_1`, `_2`… before its extension, whichever
/// does not exist yet: how Yazi pastes over a name that is taken.
fn unique(path: &Path) -> PathBuf {
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

/// Copy `from` to `to`: files, links as links, directories recursively.
fn copy_all(from: &Path, to: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(from)?;
    if meta.file_type().is_symlink() {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(std::fs::read_link(from)?, to);
        #[cfg(not(unix))]
        return std::fs::copy(from, to).map(|_| ());
    }
    if meta.is_dir() {
        std::fs::create_dir(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(from, to).map(|_| ())
}

/// Remove `path`: a link itself (never what it points to), a file, or a
/// directory and everything in it.
fn remove(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Move `from` to `to`, copying across file systems. Never into itself:
/// the copy would find its own destination and nest without end.
fn move_to(from: &Path, to: &Path) -> std::io::Result<()> {
    if to.starts_with(from) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "can't go inside itself",
        ));
    }
    std::fs::rename(from, to).or_else(|_| {
        copy_all(from, to)?;
        remove(from)
    })
}

/// Paste `yank` into `dir`: copies take a free name, a cut moves, and
/// `force` replaces what has the same name. What went wrong, if anything.
fn paste(yank: &Yank, dir: &Path, force: bool) -> Vec<String> {
    let mut errors = Vec::new();
    for from in &yank.paths {
        let Some(name) = from.file_name() else {
            continue;
        };
        let target = dir.join(name);
        if from.is_dir() && dir.starts_with(from) {
            errors.push(format!("{} can't go inside itself", name.to_string_lossy()));
            continue;
        }
        if &target == from && yank.cut {
            continue;
        }
        let target = if force && &target != from {
            let _ = remove(&target);
            target
        } else {
            unique(&target)
        };
        let done = if yank.cut {
            move_to(from, &target)
        } else {
            copy_all(from, &target)
        };
        if let Err(error) = done {
            errors.push(format!("{}: {error}", name.to_string_lossy()));
        }
    }
    errors
}

/// Run file work off the app's thread, then read the directories again;
/// what went wrong is a notification titled `what`, as Yazi's tasks
/// report a failure.
fn work(fm: Fm, what: &'static str, job: impl FnOnce() -> Vec<String> + Send + 'static) {
    spawn(job, move |errors: Vec<String>, cx| {
        fm.refresh();
        if !errors.is_empty() {
            cx.notify(Level::Error, what, escape(&errors.join("\n")));
        }
    });
}

// ---------------------------------------------------------------------
// Keys: Yazi's default keymap.
// ---------------------------------------------------------------------

/// What a key does.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Act {
    Escape,
    Quit,
    Close,
    Arrow(isize),
    Top,
    Bottom,
    Page(f64),
    Leave,
    Enter,
    Back,
    Forward,
    Toggle,
    SelectAll,
    Invert,
    Visual(bool),
    Seek(i16),
    Spot,
    Open(bool),
    Yank(bool),
    Paste(bool),
    Unyank,
    Remove(bool),
    Create,
    Rename,
    Hidden,
    Jump,
    Zoxide,
    Linemode(Linemode),
    CopyPath,
    CopyDir,
    CopyName,
    CopyStem,
    Filter,
    Find(bool),
    FindArrow(bool),
    Sort(SortBy, bool),
    Home,
    Go(&'static str),
    GoInteractive,
    Follow,
    TabNew,
    TabRename,
    TabSwitch(usize),
    TabStep(isize),
    TabSwap(isize),
    Help,
}

/// Keys (as bindings name them), how Yazi's help writes them, what they
/// do.
type Binding = (&'static str, &'static str, &'static str, Act);

/// The single keys of the manager, each name in `keys` one way to press
/// it.
#[rustfmt::skip]
const KEYS: &[Binding] = &[
    ("esc", "<Esc>", "Exit visual mode, clear selection, or cancel search", Act::Escape),
    ("q", "q", "Quit the process", Act::Quit),
    ("ctrl+c", "<C-c>", "Close the current tab, or quit if it's last", Act::Close),
    ("k up", "k", "Previous file", Act::Arrow(-1)),
    ("j down", "j", "Next file", Act::Arrow(1)),
    ("G end", "G", "Go to bottom", Act::Bottom),
    ("home", "<Home>", "Go to top", Act::Top),
    ("ctrl+u shift+pageup", "<C-u>", "Move cursor up half page", Act::Page(-0.5)),
    ("ctrl+d shift+pagedown", "<C-d>", "Move cursor down half page", Act::Page(0.5)),
    ("ctrl+b pageup", "<C-b>", "Move cursor up one page", Act::Page(-1.0)),
    ("ctrl+f pagedown", "<C-f>", "Move cursor down one page", Act::Page(1.0)),
    ("h left", "h", "Back to the parent directory", Act::Leave),
    ("l right", "l", "Enter the child directory", Act::Enter),
    ("H", "H", "Back to previous directory", Act::Back),
    ("L", "L", "Forward to next directory", Act::Forward),
    ("space", "<Space>", "Toggle the current selection state", Act::Toggle),
    ("ctrl+a", "<C-a>", "Select all files", Act::SelectAll),
    ("ctrl+r", "<C-r>", "Invert selection of all files", Act::Invert),
    ("v", "v", "Enter visual mode (selection mode)", Act::Visual(false)),
    ("V", "V", "Enter visual mode (unset mode)", Act::Visual(true)),
    ("K", "K", "Seek up 5 units in the preview", Act::Seek(-5)),
    ("J", "J", "Seek down 5 units in the preview", Act::Seek(5)),
    ("tab", "<Tab>", "Spot hovered file", Act::Spot),
    ("o enter", "o", "Open selected files", Act::Open(false)),
    ("O shift+enter", "O", "Open selected files interactively", Act::Open(true)),
    ("y", "y", "Yank selected files (copy)", Act::Yank(false)),
    ("x", "x", "Yank selected files (cut)", Act::Yank(true)),
    ("p", "p", "Paste yanked files", Act::Paste(false)),
    ("P", "P", "Paste yanked files (overwrite if the destination exists)", Act::Paste(true)),
    ("Y X", "Y", "Cancel the yank status", Act::Unyank),
    ("d", "d", "Trash selected files", Act::Remove(false)),
    ("D", "D", "Permanently delete selected files", Act::Remove(true)),
    ("a", "a", "Create a file (ends with / for directories)", Act::Create),
    ("r", "r", "Rename selected file", Act::Rename),
    (".", ".", "Toggle the visibility of hidden files", Act::Hidden),
    ("z", "z", "Jump to a file or directory below here", Act::Jump),
    ("Z", "Z", "Jump to a directory visited", Act::Zoxide),
    ("f", "f", "Filter files", Act::Filter),
    ("/", "/", "Find next file", Act::Find(false)),
    ("?", "?", "Find previous file", Act::Find(true)),
    ("n", "n", "Next found", Act::FindArrow(false)),
    ("N", "N", "Previous found", Act::FindArrow(true)),
    ("[", "[", "Switch to previous tab", Act::TabStep(-1)),
    ("]", "]", "Switch to next tab", Act::TabStep(1)),
    ("{", "{", "Swap current tab with previous tab", Act::TabSwap(-1)),
    ("}", "}", "Swap current tab with next tab", Act::TabSwap(1)),
    ("~ f1", "~", "Open help", Act::Help),
];

/// The key sequences: keys pressed one after another (`,` is written in
/// quotes, as in any binding).
#[rustfmt::skip]
const SEQUENCES: &[Binding] = &[
    ("g g", "gg", "Go to top", Act::Top),
    ("g h", "gh", "Go home", Act::Home),
    ("g c", "gc", "Go to ~/.config", Act::Go(".config")),
    ("g d", "gd", "Go to ~/Downloads", Act::Go("Downloads")),
    ("g space", "g<Space>", "Jump interactively", Act::GoInteractive),
    ("g f", "gf", "Follow hovered symlink", Act::Follow),
    ("\",\" m", ",m", "Sort by modified time", Act::Sort(SortBy::Mtime, false)),
    ("\",\" M", ",M", "Sort by modified time (reverse)", Act::Sort(SortBy::Mtime, true)),
    ("\",\" b", ",b", "Sort by birth time", Act::Sort(SortBy::Btime, false)),
    ("\",\" B", ",B", "Sort by birth time (reverse)", Act::Sort(SortBy::Btime, true)),
    ("\",\" e", ",e", "Sort by extension", Act::Sort(SortBy::Extension, false)),
    ("\",\" E", ",E", "Sort by extension (reverse)", Act::Sort(SortBy::Extension, true)),
    ("\",\" a", ",a", "Sort alphabetically", Act::Sort(SortBy::Alphabetical, false)),
    ("\",\" A", ",A", "Sort alphabetically (reverse)", Act::Sort(SortBy::Alphabetical, true)),
    ("\",\" n", ",n", "Sort naturally", Act::Sort(SortBy::Natural, false)),
    ("\",\" N", ",N", "Sort naturally (reverse)", Act::Sort(SortBy::Natural, true)),
    ("\",\" s", ",s", "Sort by size", Act::Sort(SortBy::Size, false)),
    ("\",\" S", ",S", "Sort by size (reverse)", Act::Sort(SortBy::Size, true)),
    ("\",\" r", ",r", "Sort randomly", Act::Sort(SortBy::Random, false)),
    ("m s", "ms", "Linemode: size", Act::Linemode(Linemode::Size)),
    ("m p", "mp", "Linemode: permissions", Act::Linemode(Linemode::Permissions)),
    ("m b", "mb", "Linemode: btime", Act::Linemode(Linemode::Btime)),
    ("m m", "mm", "Linemode: mtime", Act::Linemode(Linemode::Mtime)),
    ("m o", "mo", "Linemode: owner", Act::Linemode(Linemode::Owner)),
    ("m n", "mn", "Linemode: none", Act::Linemode(Linemode::None)),
    ("c c", "cc", "Copy file path", Act::CopyPath),
    ("c d", "cd", "Copy directory path", Act::CopyDir),
    ("c f", "cf", "Copy filename", Act::CopyName),
    ("c n", "cn", "Copy filename without extension", Act::CopyStem),
    ("t t", "tt", "Create a new tab in CWD", Act::TabNew),
    ("t r", "tr", "Rename current tab", Act::TabRename),
];

/// Do `act`.
fn run(fm: Fm, act: Act, cx: &mut Ctx) {
    match act {
        Act::Escape => {
            // Yazi's order: visual mode, then the selection, the filter,
            // the find.
            if fm.end_visual() {
            } else if !fm.selected.with_untracked(BTreeSet::is_empty) {
                fm.selected.set(BTreeSet::new());
            } else if !fm.filter.with_untracked(String::is_empty) {
                fm.keep(|| fm.filter.set(String::new()));
            } else {
                fm.find.set(None);
            }
        }
        Act::Quit => cx.quit(),
        Act::Close => fm.close_tab(cx),
        Act::Arrow(by) => fm.arrow(by),
        Act::Top => fm.move_to(0),
        Act::Bottom => fm.move_to(isize::MAX),
        Act::Page(share) => fm.page(share),
        Act::Leave => fm.leave(),
        Act::Enter => fm.enter(),
        Act::Back => fm.history(false),
        Act::Forward => fm.history(true),
        Act::Toggle => {
            if let Some(entry) = fm.hovered.get_untracked() {
                let path = fm.path_of(&entry);
                fm.selected.update(|s| {
                    if !s.remove(&path) {
                        s.insert(path);
                    }
                });
                fm.arrow(1);
            }
        }
        Act::SelectAll | Act::Invert => {
            let paths: Vec<PathBuf> = fm
                .files
                .with_untracked(|f| f.iter().map(|e| fm.path_of(e)).collect());
            fm.selected.update(|s| {
                for path in paths {
                    if act == Act::SelectAll || !s.remove(&path) {
                        s.insert(path);
                    }
                }
            });
        }
        Act::Visual(unset) => {
            if fm.visual.get_untracked().is_none() && fm.len() > 0 {
                fm.visual.set(Some((fm.cursor.get_untracked(), unset)));
            } else {
                fm.end_visual();
            }
        }
        Act::Seek(by) => {
            fm.preview_top
                .update(|t| *t = (*t as i32 + by as i32).max(0) as u16);
        }
        Act::Spot => spot(fm, cx),
        Act::Open(interactive) => open(fm, interactive, cx),
        Act::Yank(cut) => {
            let paths = fm.targets();
            if !paths.is_empty() {
                fm.yanked.set(Some(Yank { paths, cut }));
                fm.selected.set(BTreeSet::new());
            }
        }
        Act::Unyank => fm.yanked.set(None),
        Act::Paste(force) => {
            let Some(yank) = fm.yanked.get_untracked() else {
                return;
            };
            let dir = fm.cwd.get_untracked();
            // P replaces what has the same name here: say which first.
            let clashes: Vec<String> = yank
                .paths
                .iter()
                .filter_map(|p| p.file_name().map(|n| dir.join(n)))
                .filter(|p| force && std::fs::symlink_metadata(p).is_ok())
                .map(|p| readable_path(&p))
                .collect();
            let start = move |_: &mut Ctx| {
                if yank.cut {
                    fm.yanked.set(None);
                }
                work(fm, "Paste", move || paste(&yank, &dir, force));
            };
            if clashes.is_empty() {
                start(cx);
            } else {
                cx.confirm(
                    "Overwrite file?",
                    "Will overwrite the following file:",
                    clashes,
                    start,
                );
            }
        }
        Act::Remove(permanently) => {
            let paths = fm.targets();
            if paths.is_empty() {
                return;
            }
            let n = paths.len();
            let s = if n == 1 { "" } else { "s" };
            let title = if permanently {
                format!("Permanently delete {n} selected file{s}?")
            } else {
                format!("Trash {n} selected file{s}?")
            };
            let shown = paths.iter().map(|p| readable_path(p)).collect();
            cx.confirm(title, "", shown, move |_| {
                fm.selected.set(BTreeSet::new());
                if permanently {
                    work(fm, "Delete", move || {
                        paths
                            .iter()
                            .filter_map(|p| {
                                remove(p).err().map(|e| format!("{}: {e}", p.display()))
                            })
                            .collect()
                    });
                } else {
                    let trash = fm.start.get_untracked().join(".trash");
                    work(fm, "Trash", move || {
                        if let Err(e) = std::fs::create_dir_all(&trash) {
                            return vec![e.to_string()];
                        }
                        paths
                            .iter()
                            // Not the trash, nor anything already in it.
                            .filter(|p| !p.starts_with(&trash))
                            .filter_map(|p| {
                                let to = unique(&trash.join(p.file_name()?));
                                move_to(p, &to)
                                    .err()
                                    .map(|e| format!("{}: {e}", p.display()))
                            })
                            .collect()
                    });
                }
            });
        }
        Act::Create => {
            let dir = fm.cwd.get_untracked();
            Prompt::new("Create:").open(cx, move |typed, cx| {
                let path = match under(&dir, typed) {
                    Ok(path) => path,
                    Err(e) => return cx.notify(Level::Warn, "Create", escape(&e)),
                };
                if path.exists() {
                    let body = format!("{} already exists", escape(typed));
                    return cx.notify(Level::Warn, "Create", body);
                }
                let made = if typed.ends_with('/') || typed.ends_with('\\') {
                    std::fs::create_dir_all(&path)
                } else {
                    path.parent()
                        .map_or(Ok(()), std::fs::create_dir_all)
                        .and_then(|_| std::fs::File::create(&path).map(|_| ()))
                };
                match made {
                    Ok(()) => {
                        fm.refresh();
                        let first = Path::new(typed.trim_end_matches('/'))
                            .components()
                            .next()
                            .map(|c| c.as_os_str().to_string_lossy().into_owned());
                        fm.cursor.set(fm.position(first.as_deref()));
                    }
                    Err(e) => cx.notify(Level::Error, "Create", escape(&e.to_string())),
                }
            });
        }
        Act::Rename => {
            let Some(entry) = fm.hovered.get_untracked() else {
                return;
            };
            let dir = fm.cwd.get_untracked();
            let old = entry.name.clone();
            // Yazi's `rename --cursor=before_ext`: the caret before the
            // extension (a directory's name has none).
            let stem = match Path::new(&old).extension() {
                Some(ext) if !entry.dir => {
                    old.chars().count() - ext.to_string_lossy().chars().count() - 1
                }
                _ => old.chars().count(),
            };
            // Below the hovered row, where it was drawn.
            let row = fm.rows.with_untracked(|r| r.get(fm.cursor.get_untracked()));
            let origin = row.map_or(Origin::TopCenter, Origin::At);
            Prompt::new("Rename:")
                .at(origin, Offset::new(0, 0, 80, 3))
                .text(old.clone())
                .caret(Caret::At(stem))
                .open(cx, move |typed, cx| {
                    if typed == old {
                        return;
                    }
                    let to = match under(&dir, typed) {
                        Ok(to) => to,
                        Err(e) => return cx.notify(Level::Warn, "Rename", escape(&e)),
                    };
                    if std::fs::symlink_metadata(&to).is_ok() {
                        let body = format!("{} already exists", escape(typed));
                        return cx.notify(Level::Warn, "Rename", body);
                    }
                    match std::fs::rename(dir.join(&old), &to) {
                        Ok(()) => {
                            fm.refresh();
                            fm.cursor.set(fm.position(Some(typed)));
                        }
                        Err(e) => cx.notify(Level::Error, "Rename", escape(&e.to_string())),
                    }
                });
        }
        Act::Hidden => fm.keep(|| fm.hidden.update(|h| *h = !*h)),
        Act::Jump => jump(fm, cx),
        Act::Zoxide => zoxide(fm, cx),
        Act::Linemode(mode) => fm.linemode.set(mode),
        Act::CopyPath | Act::CopyDir | Act::CopyName | Act::CopyStem => {
            let Some(entry) = fm.hovered.get_untracked() else {
                return;
            };
            let path = fm.path_of(&entry);
            let text = match act {
                Act::CopyPath => path.display().to_string(),
                Act::CopyDir => fm.cwd.get_untracked().display().to_string(),
                Act::CopyName => entry.name,
                _ => path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            };
            cx.copy(text);
        }
        Act::Filter => {
            let before = fm.filter.get_untracked();
            Prompt::new("Filter:")
                .text(before.clone())
                .on_change(move |typed| fm.keep(|| fm.filter.set(typed.to_string())))
                .on_cancel(move || fm.keep(|| fm.filter.set(before.clone())))
                .open(cx, |_, _| {});
        }
        Act::Find(previous) => {
            let title = if previous {
                "Find previous:"
            } else {
                "Find next:"
            };
            let from = fm.cursor.get_untracked();
            Prompt::new(title)
                .on_change(move |typed| {
                    fm.cursor.set(from);
                    fm.find.set((!typed.is_empty()).then(|| typed.to_string()));
                    fm.find_arrow(previous, true);
                })
                .on_cancel(move || {
                    fm.find.set(None);
                    fm.cursor.set(from);
                })
                .open(cx, |_, _| {});
        }
        Act::FindArrow(previous) => fm.find_arrow(previous, false),
        Act::Sort(by, reverse) => {
            fm.keep(|| fm.sort.set(Sort { by, reverse }));
            // As Yazi's keymap: sorting by size or time shows it.
            match by {
                SortBy::Size => fm.linemode.set(Linemode::Size),
                SortBy::Mtime => fm.linemode.set(Linemode::Mtime),
                SortBy::Btime => fm.linemode.set(Linemode::Btime),
                _ => {}
            }
        }
        Act::Home => {
            if let Some(home) = home() {
                fm.cd(home, None);
            }
        }
        Act::Go(rel) => {
            if let Some(dir) = home().map(|h| h.join(rel)).filter(|d| d.is_dir()) {
                fm.cd(dir, None);
            } else {
                cx.notify(Level::Warn, "Cd", format!("~/{rel} does not exist"));
            }
        }
        Act::GoInteractive => {
            let here = fm.cwd.get_untracked();
            Prompt::new("Change directory:").open(cx, move |typed, cx| {
                let typed = typed.trim();
                let target = match typed.strip_prefix('~') {
                    Some(rest) => home().map(|h| h.join(rest.trim_start_matches('/'))),
                    None => Some(here.join(typed)),
                };
                match target.filter(|t| t.is_dir()) {
                    Some(dir) => fm.cd(absolute(dir), None),
                    None => cx.notify(Level::Warn, "Cd", format!("No directory {}", escape(typed))),
                }
            });
        }
        Act::Follow => {
            if let Some(entry) = fm.hovered.get_untracked() {
                if let Some(to) = entry.link {
                    let target = absolute(fm.cwd.get_untracked().join(to));
                    if let Some(dir) = target.parent() {
                        let name = target.file_name().map(|n| n.to_string_lossy().into_owned());
                        fm.cd(dir.to_path_buf(), name);
                    }
                }
            }
        }
        Act::TabNew => fm.new_tab(),
        Act::TabRename => {
            let i = fm.active.get_untracked();
            let now = fm.tabs.with_untracked(|t| t[i].title());
            Prompt::new("Rename tab:")
                .text(now)
                .open(cx, move |typed, _| {
                    let name = (!typed.trim().is_empty()).then(|| typed.trim().to_string());
                    fm.tabs.update(|t| t[i].name = name);
                });
        }
        Act::TabSwitch(to) => fm.switch_tab(to),
        Act::TabStep(by) => {
            let count = fm.tabs.with_untracked(Vec::len) as isize;
            let to = (fm.active.get_untracked() as isize + by).rem_euclid(count);
            fm.switch_tab(to as usize);
        }
        Act::TabSwap(by) => fm.swap_tab(by),
        Act::Help => help(cx),
    }
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Enter and `o` open the selection (or the hovered entry) with the first
/// opener Yazi's rules give its type; `O` asks which, in Yazi's "Open
/// with:" pick below the hovered row.
fn open(fm: Fm, interactive: bool, cx: &mut Ctx) {
    let paths = fm.targets();
    if !interactive {
        // Each file with its own first opener; the editor once for all.
        let mut by: Vec<(Opener, Vec<PathBuf>)> = Vec::new();
        for path in paths {
            let Some(&first) = targets_openers(std::slice::from_ref(&path)).first() else {
                continue;
            };
            match by.iter_mut().find(|(o, _)| *o == first) {
                Some((_, all)) => all.push(path),
                None => by.push((first, vec![path])),
            }
        }
        for (opener, paths) in by {
            open_with(fm, opener, paths, cx);
        }
        return;
    }
    let openers = targets_openers(&paths);
    if openers.is_empty() {
        return;
    }
    let row = fm.rows.with_untracked(|r| r.get(fm.cursor.get_untracked()));
    let origin = row.map_or(Origin::Center, Origin::At);
    let height = openers.len() as u16 + 2;
    cx.popup_at(origin, Offset::new(0, 0, 50, height.min(7)), move || {
        let picked = signal(0usize);
        let names: Vec<String> = openers.iter().map(|o| o.describe().to_string()).collect();
        list(move || names.clone(), picked)
            .autofocus()
            .on_key("enter", move |cx| {
                cx.pop();
                let opener = openers[picked.get_untracked().min(openers.len() - 1)];
                open_with(fm, opener, paths.clone(), cx);
            })
            .on_key("esc q", |cx| cx.pop())
            .panel("Open with:")
    });
}

// ---------------------------------------------------------------------
// Pop-ups: prompts, help, spot, jumps.
// ---------------------------------------------------------------------

/// A one-line prompt in a rounded box, as Yazi's input: where it opens
/// (the top centre, two rows down, 80 wide, unless [`at`](Self::at) says
/// otherwise), its text and caret, and what hears each edit and Esc.
struct Prompt {
    title: &'static str,
    origin: Origin,
    offset: Offset,
    text: String,
    caret: Caret,
    change: Box<dyn Fn(&str)>,
    cancel: Box<dyn Fn()>,
}

impl Prompt {
    fn new(title: &'static str) -> Prompt {
        Prompt {
            title,
            origin: Origin::TopCenter,
            offset: Offset::new(0, 2, 80, 3),
            text: String::new(),
            caret: Caret::End,
            change: Box::new(|_| {}),
            cancel: Box::new(|| {}),
        }
    }

    fn at(mut self, origin: Origin, offset: Offset) -> Prompt {
        (self.origin, self.offset) = (origin, offset);
        self
    }

    fn text(mut self, text: String) -> Prompt {
        self.text = text;
        self
    }

    fn caret(mut self, caret: Caret) -> Prompt {
        self.caret = caret;
        self
    }

    fn on_change(mut self, change: impl Fn(&str) + 'static) -> Prompt {
        self.change = Box::new(change);
        self
    }

    fn on_cancel(mut self, cancel: impl Fn() + 'static) -> Prompt {
        self.cancel = Box::new(cancel);
        self
    }

    /// Open it: Enter closes it and gives the text to `submit`.
    fn open(self, cx: &mut Ctx, submit: impl Fn(&str, &mut Ctx) + 'static) {
        let Prompt {
            title,
            origin,
            offset,
            text,
            caret,
            change,
            cancel,
        } = self;
        cx.popup_at(origin, offset, move || {
            let value = signal(text);
            watch(move || value.get(), move |text, _| change(&text));
            input(value)
                .caret(caret)
                .on_submit(move |text, cx| {
                    cx.pop();
                    submit(text, cx);
                })
                .on_key("esc", move |cx| {
                    cx.pop();
                    cancel();
                })
                .autofocus()
                .panel(title)
                // Yazi's input titles are plain; its confirm's are blue.
                .title_style("default")
        });
    }
}

/// Yazi's help: every key and what it does, in a box in the middle; `/`
/// filters, j/k move, Esc closes.
fn help(cx: &mut Ctx) {
    let rows: Vec<(String, String)> = KEYS
        .iter()
        .chain(SEQUENCES)
        .map(|(_, on, desc, _)| (on.to_string(), desc.to_string()))
        .chain((1..=9).map(|n| (n.to_string(), format!("Switch to tab {n}"))))
        .chain([(
            "<C-p>".to_string(),
            "Find a command (not in Yazi)".to_string(),
        )])
        .collect();
    cx.modal(Size::Fixed(80), Size::Fixed(25), move || {
        let query = signal(String::new());
        let at = signal(0usize);
        let shown = memo(move || {
            let q = query.get();
            rows.iter()
                .filter(|(on, desc)| smart_contains(on, &q) || smart_contains(desc, &q))
                .map(|(on, desc)| {
                    let pad = " ".repeat(20usize.saturating_sub(on.chars().count()));
                    format!("[cyan]{}[/]{pad}{}", escape(on), escape(desc))
                })
                .collect::<Vec<_>>()
        });
        watch(move || query.get(), move |_, _| at.set(0));
        let field = input(query).placeholder("Type / to filter");
        let field_id = field.id();
        let keys = list(move || shown.get(), at)
            .theme_styles([("selected", "reverse bold")])
            .autofocus();
        let keys_id = keys.id();
        // The rule reaches the panel's sides, so it joins them (`├─┤`);
        // the field and the keys keep a cell from them each.
        column([
            field
                .on_submit(move |_, cx| cx.focus(keys_id))
                .on_key("esc", move |cx| cx.focus(keys_id))
                .padding(0, 1)
                .fixed(1),
            rule(),
            keys.on_key("/", move |cx| cx.focus(field_id))
                .on_key("esc q ~ f1", |cx| cx.pop())
                .padding(0, 1)
                .flex(1),
        ])
        .panel("mgr.help")
        .title_align(Align::Center)
    });
}

/// Tab: what Yazi's spotter shows about the hovered file.
fn spot(fm: Fm, cx: &mut Ctx) {
    let Some(entry) = fm.hovered.get_untracked() else {
        return;
    };
    let path = fm.path_of(&entry);
    let mime = filetype::mime_for(&path).unwrap_or("");
    let mut facts = vec![
        ("Name", entry.name.clone()),
        ("Path", path.display().to_string()),
        ("Mime", mime.to_string()),
        ("Size", readable_size(entry.size)),
        ("Mode", permissions(entry.mode)),
        ("Owner", owner(entry.uid, entry.gid)),
    ];
    let full = |t: SystemTime| rich_ext::fs::local_time(t, "%Y-%m-%d %H:%M:%S");
    if let Some(t) = entry.mtime {
        facts.push(("Modified", full(t)));
    }
    if let Some(t) = entry.btime {
        facts.push(("Created", full(t)));
    }
    if let Some(to) = &entry.link {
        facts.push(("Target", to.display().to_string()));
    }
    let height = facts.len() as u16 + 2;
    cx.modal(Size::Fixed(60), Size::Fixed(height), move || {
        let lines: Vec<String> = facts
            .iter()
            .map(|(k, v)| format!("[blue]{k:<10}[/][yellow]{}[/]", escape(v)))
            .collect();
        label(lines.join("\n"))
            .no_wrap()
            .overflow(Overflow::Ellipsis)
            .padding(0, 1)
            .panel("Spot")
            .title_align(Align::Center)
            .on_key("esc q tab enter", |cx| cx.pop())
    });
}

/// z: jump to a file or directory under this one (Yazi asks fzf), with
/// rs-rich-interact's `Jump`: type to narrow, Enter goes there.
fn jump(fm: Fm, cx: &mut Ctx) {
    let root = fm.cwd.get_untracked();
    let hidden = fm.hidden.get_untracked();
    let mut found = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read.filter_map(Result::ok) {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                continue;
            }
            let path = entry.path();
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(path.clone());
            }
            if let Ok(rel) = path.strip_prefix(&root) {
                found.push(rel.display().to_string());
            }
            if found.len() >= 20_000 {
                break;
            }
        }
    }
    found.sort();
    let picker = Jump::new("Jump").source(JumpSource::new("files", "Files").items(found));
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        component(picker, move |jumped, cx| {
            cx.pop();
            reveal(fm, &root.join(&jumped.value));
        })
        .on_cancel(|cx| cx.pop())
        .panel("")
    });
}

/// Z: jump to a directory this session has been in (Yazi asks zoxide).
fn zoxide(fm: Fm, cx: &mut Ctx) {
    let mut seen: Vec<String> = fm.tabs.with_untracked(|tabs| {
        tabs.iter()
            .flat_map(|t| t.back.iter().chain(&t.forward).chain([&t.cwd]))
            .map(|p| p.display().to_string())
            .collect()
    });
    seen.extend(fm.remember.with_untracked(|r| {
        r.keys()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
    }));
    seen.sort();
    seen.dedup();
    let picker = Jump::new("Directory").source(JumpSource::new("dirs", "Visited").items(seen));
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        component(picker, move |jumped, cx| {
            cx.pop();
            fm.cd(PathBuf::from(&jumped.value), None);
        })
        .on_cancel(|cx| cx.pop())
        .panel("")
    });
}

/// Go to `path`'s directory, hovering it.
fn reveal(fm: Fm, path: &Path) {
    if let Some(dir) = path.parent() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
        fm.cd(dir.to_path_buf(), name);
    }
}

// ---------------------------------------------------------------------
// The screen.
// ---------------------------------------------------------------------

/// `path` as Yazi writes it: `~` for the home directory.
fn readable_path(path: &Path) -> String {
    match home().as_ref().and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// The top row: the directory (cut from the left when it is long) with
/// the filter and find in brackets, and at the right how many files are
/// selected or yanked.
fn header(fm: Fm) -> Node {
    let count = move || {
        let selected = fm.selected.with(BTreeSet::len);
        match fm.yanked.get() {
            _ if selected > 0 => Some(("black on yellow", selected)),
            Some(y) if y.cut => Some(("white on red", y.paths.len())),
            Some(y) => Some(("white on green", y.paths.len())),
            None => None,
        }
    };
    let right = text(move || match count() {
        Some((style, n)) => format!("[{style}] {n} [/] "),
        None => String::new(),
    })
    .no_wrap()
    .auto();
    let size = viewport();
    let cwd = text(move || {
        let mut flags = Vec::new();
        let filter = fm.filter.get();
        if !filter.is_empty() {
            flags.push(format!("filter: {filter}"));
        }
        if let Some(find) = fm.find.get() {
            flags.push(format!("find: {find}"));
        }
        let mut shown = readable_path(&fm.cwd.get());
        if !flags.is_empty() {
            shown.push_str(&format!(" ({})", flags.join(", ")));
        }
        // As Yazi's: a path too long loses its start, not where you are.
        // (Text overflows only at the end, so the cut is made here.)
        let taken = count().map_or(0, |(_, n)| n.to_string().len() + 3);
        let room = (size.get().0 as usize).saturating_sub(taken).max(1);
        let chars: Vec<char> = shown.chars().collect();
        if chars.len() > room {
            let tail: String = chars[chars.len() - (room - 1)..].iter().collect();
            shown = format!("…{tail}");
        }
        format!("[cyan]{}[/]", escape(&shown))
    })
    .no_wrap()
    .flex(1);
    row([cwd, right]).fixed(1)
}

/// The tab strip, shown once there is more than one tab: Yazi's, the
/// active tab on blue.
fn tab_strip(fm: Fm) -> Node {
    // A click picks a tab through `switch_tab`, as the keys do, so visual
    // mode, the filter and the find end the same way.
    let clicked = signal(fm.active.get_untracked());
    watch(move || fm.active.get(), move |at, _| clicked.set(at));
    watch(move || clicked.get(), move |to, _| fm.switch_tab(to));
    let size = viewport();
    switch(
        move || fm.tabs.with(Vec::len) > 1,
        move |many| {
            if !many {
                return column([]);
            }
            let titles = move || {
                let width = size.get().0;
                let active = fm.active.get();
                fm.tabs.with(|tabs| {
                    let max = (width as usize / tabs.len()).max(4);
                    tabs.iter()
                        .enumerate()
                        .map(|(i, tab)| {
                            let mut title = format!(" {} {} ", i + 1, tab.title());
                            if title.chars().count() > max {
                                title = title.chars().take(max - 1).collect::<String>() + "…";
                            }
                            if i == active {
                                escape(&title)
                            } else {
                                format!("[blue on white]{}[/]", escape(&title))
                            }
                        })
                        .collect()
                })
            };
            tabs(titles, clicked)
                .divider("")
                .tab_padding("", "")
                .no_focus()
                .fixed(1)
        },
    )
    .auto()
}

/// The status line, as Yazi's: the mode, the hovered entry's size and
/// name on the left; its permissions, where the cursor is as a share and
/// as a count on the right.
fn status(fm: Fm) -> Node {
    let style = move || match fm.visual.get() {
        Some((_, false)) => ("bold on red", "red on white", "SEL"),
        Some((_, true)) => ("bold on red", "red on white", "UNS"),
        None => ("bold on blue", "blue on white", "NOR"),
    };
    let (open, close) = if fm.icons {
        ("\u{e0b6}", "\u{e0b4}")
    } else {
        ("", "")
    };
    let cap = move |text: &str, bg: &str| -> String {
        if text.is_empty() {
            String::new()
        } else {
            format!("[{bg}]{text}[/]")
        }
    };
    let left = text(move || {
        let (main, alt, mode) = style();
        let main_bg = main.rsplit(' ').next().unwrap_or("blue");
        let hovered = fm.hovered.get();
        let size = readable_size(hovered.as_ref().map_or(0, |e| e.size));
        let name = hovered.map_or(String::new(), |e| escape(&e.name));
        format!(
            "{}[{main}] {mode} [/]{}[{alt}] {size} [/]{} {name}",
            cap(open, main_bg),
            cap(close, &format!("{main_bg} on white")),
            cap(close, "white"),
        )
    })
    .no_wrap()
    .overflow(Overflow::Ellipsis)
    .flex(1);
    let right = text(move || {
        let (main, alt, _) = style();
        let main_bg = main.rsplit(' ').next().unwrap_or("blue");
        let len = fm.files.with(Vec::len);
        let cursor = fm.cursor.get().min(len.saturating_sub(1));
        let perm = fm
            .hovered
            .get()
            .map_or(String::new(), |e| coloured_permissions(e.mode));
        let percent = match (cursor, len) {
            (0, _) | (_, 0) => " Top ".to_string(),
            _ => match (cursor + 1) * 100 / len {
                100 => " Bot ".to_string(),
                p => format!(" {p:>2}% "),
            },
        };
        let at = format!(" {:>2}/{:<2} ", (cursor + 1).min(len), len);
        format!(
            "{perm} {}[{alt}]{percent}[/]{}[{main}]{at}[/]{}",
            cap(open, "white"),
            cap(open, &format!("{main_bg} on white")),
            cap(close, main_bg),
        )
    })
    .no_wrap()
    .auto();
    row([left, right]).fixed(1)
}

/// A column's message when it has nothing to list, centred as Yazi's.
fn message(markup: impl Fn() -> String + 'static) -> Node {
    text(markup).text_align(TextAlign::Center)
}

/// The middle column: the current directory's entries, one row each,
/// built only for the rows in view; a marker column at the left, the
/// hovered row reversed across the column in its type's colour, and the
/// line mode at the right of each row.
fn current_column(fm: Fm) -> Node {
    let empty = move || fm.files.with(Vec::is_empty);
    switch(empty, move |empty| {
        if empty {
            return message(move || {
                if !fm.filter.get().is_empty() {
                    "No filter results".into()
                } else {
                    match fm.listing.get() {
                        Err(e) => format!("Error: {}", escape(&e)),
                        Ok(_) => "No items".into(),
                    }
                }
            });
        }
        let item = move |i: usize| {
            let find = fm.find.get();
            let at = fm.cursor.get();
            let found = (i == at).then(|| fm.found()).flatten();
            fm.files.with(|f| {
                f.get(i).map_or(String::new(), |e| {
                    entity(e, fm.icons, i == at, find.as_deref(), found)
                })
            })
        };
        let trailing = move |i: usize| {
            let mode = fm.linemode.get();
            let dir = fm.cwd.get();
            fm.files.with(|f| {
                f.get(i).map_or(String::new(), |e| {
                    linemode_cell(e, mode, &dir.join(&e.name))
                })
            })
        };
        let marker = move |i: usize| {
            let dir = fm.cwd.get();
            fm.files.with(|f| {
                let path = dir.join(&f.get(i)?.name);
                marker_style(fm.marker(i, &path))
            })
        };
        let rows = fm.rows.get_untracked();
        let node = list_lazy(move || fm.files.with(Vec::len), item, fm.cursor)
            .trailing(trailing)
            .marker(marker)
            .row_style(move |i| fm.files.with(|f| row_style(f.get(i))))
            .scroll_padding(5)
            .offset(fm.first)
            .row_rects(&rows)
            .name("current")
            .autofocus()
            .on_mouse(move |cx, mouse| {
                if mouse.kind != MouseKind::Down(Button::Right) {
                    return false;
                }
                let i = fm.first.get_untracked() + mouse.row as usize;
                if i >= fm.len() {
                    return true;
                }
                fm.cursor.set(i);
                context_menu(cx, menu(fm));
                true
            });
        // `update`, not `set`: a handle is not `PartialEq`.
        let handle = node.rect_handle();
        fm.column.update(|c| *c = Some(handle));
        node
    })
}

/// What a right click on an entry offers.
fn menu(fm: Fm) -> Vec<MenuItem> {
    let entry = fm.hovered.get_untracked();
    let mut items = Vec::new();
    let item = |label: &str, keys: &str, act: Act| {
        MenuItem::new(label, move |cx| run(fm, act, cx)).hint(keys)
    };
    if entry.as_ref().is_some_and(|e| e.dir) {
        items.push(item("Enter", "l", Act::Enter));
    }
    items.push(item("Open", "o", Act::Open(false)));
    items.push(item("Open with…", "O", Act::Open(true)));
    items.push(item("Yank", "y", Act::Yank(false)));
    items.push(item("Cut", "x", Act::Yank(true)));
    if fm.yanked.with_untracked(Option::is_some) {
        items.push(item("Paste", "p", Act::Paste(false)));
    }
    items.push(item("Copy path", "cc", Act::CopyPath));
    items.push(item("Rename", "r", Act::Rename));
    items.push(MenuItem::separator());
    items.push(item("Trash", "d", Act::Remove(false)));
    items.push(item("Delete", "D", Act::Remove(true)));
    items.push(MenuItem::separator());
    items.push(item("Create", "a", Act::Create));
    let shown = if fm.hidden.get_untracked() {
        "Hide hidden files"
    } else {
        "Show hidden files"
    };
    items.push(item(shown, ".", Act::Hidden));
    items.push(item("New tab here", "tt", Act::TabNew));
    items
}

/// The left column: the parent directory, this one hovered, with the
/// parent's markers. A click on an entry goes there; below them, up a
/// level.
fn parent_column(fm: Fm) -> Node {
    watch(
        move || (fm.cwd.get(), fm.parent.get()),
        move |(dir, parent), _| {
            let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            let at = match parent {
                Some(Ok(entries)) => name
                    .and_then(|n| entries.iter().position(|e| e.name == n))
                    .unwrap_or(0),
                _ => 0,
            };
            fm.parent_cursor.set(at);
        },
    );
    let with_entries = move |f: &dyn Fn(&[Entry], &Path) -> Option<String>| {
        let dir = fm.cwd.get();
        let up = dir.parent()?;
        fm.parent.with(|p| match p {
            Some(Ok(entries)) => f(entries, up),
            _ => None,
        })
    };
    let len = move || {
        fm.parent.with(|p| match p {
            Some(Ok(entries)) => entries.len(),
            _ => 0,
        })
    };
    let item = move |i: usize| {
        let at = fm.parent_cursor.get();
        with_entries(&|entries, _| {
            // Yazi's entity starts with a padding cell.
            Some(format!(
                " {}",
                entity(entries.get(i)?, fm.icons, i == at, None, None)
            ))
        })
        .unwrap_or_default()
    };
    list_lazy(len, item, fm.parent_cursor)
        .marker(move |i| {
            with_entries(&|entries, up| {
                marker_style(fm.plain_marker(&up.join(&entries.get(i)?.name)))
            })
        })
        .row_style(move |i| with_entries(&|entries, _| row_style(entries.get(i))))
        .scroll_padding(5)
        .offset(fm.parent_first)
        .no_focus()
        .name("parent")
        .on_mouse(move |_, mouse| {
            match mouse.kind {
                MouseKind::Down(Button::Left) => {}
                MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                _ => return false,
            }
            let i = fm.parent_first.get_untracked() + mouse.row as usize;
            let dir = fm.cwd.get_untracked();
            let Some(up) = dir.parent() else {
                return true;
            };
            let name = fm.parent.with_untracked(|p| match p {
                Some(Ok(entries)) => entries.get(i).map(|e| e.name.clone()),
                _ => None,
            });
            match name {
                Some(name) => fm.cd(up.to_path_buf(), Some(name)),
                None => fm.leave(),
            }
            true
        })
}

/// The right column: a directory's entries, a file highlighted (J and K
/// and the wheel scroll it), an image, or a note on what it is.
fn preview_column(fm: Fm) -> Node {
    let generation = signal(0u64);
    let pane = switch(
        move || {
            fm.preview.with(|p| match p {
                Preview::Dir(Ok(e)) if !e.is_empty() => 1,
                Preview::Code(_) => 2,
                Preview::Image(..) => 3,
                Preview::Note(_) => 4,
                _ => 0,
            })
        },
        move |kind| match kind {
            1 => {
                let entries = move |f: &dyn Fn(&[Entry]) -> Option<String>| {
                    fm.preview.with(|p| match p {
                        Preview::Dir(Ok(entries)) => f(entries),
                        _ => None,
                    })
                };
                let len = move || {
                    fm.preview.with(|p| match p {
                        Preview::Dir(Ok(entries)) => entries.len(),
                        _ => 0,
                    })
                };
                let item = move |i: usize| {
                    let at = fm.preview_cursor.get();
                    entries(&|e| Some(entity(e.get(i)?, fm.icons, i == at, None, None)))
                        .unwrap_or_default()
                };
                list_lazy(len, item, fm.preview_cursor)
                    .row_style(move |i| entries(&|e| row_style(e.get(i))))
                    // Yazi underlines the entry the preview would hover.
                    .selected_style_for(|_| Some("underline".into()))
                    .scroll_padding(5)
                    .offset(fm.preview_first)
                    .no_focus()
                    .on_mouse(move |_, mouse| {
                        match mouse.kind {
                            MouseKind::Down(Button::Left) => {}
                            MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                            _ => return false,
                        }
                        let i = fm.preview_first.get_untracked() + mouse.row as usize;
                        let name = fm.preview.with_untracked(|p| match p {
                            Preview::Dir(Ok(entries)) => entries.get(i).map(|e| e.name.clone()),
                            _ => None,
                        });
                        if let Some(entry) = fm.hovered.get_untracked().filter(|e| e.dir) {
                            fm.cd(fm.path_of(&entry), name);
                        }
                        true
                    })
            }
            2 => scroll_with(
                leaf(move |_, _, _| {
                    fm.preview.with(|p| match p {
                        Preview::Code(lines) => lines.clone(),
                        _ => Vec::new(),
                    })
                }),
                fm.preview_top,
            )
            // Yazi's preview has no scrollbar.
            .scrollbar(Scrollbar::new().show(Show::Never))
            .no_focus(),
            3 => image_pane(fm),
            // What a file is, at the top left, as Yazi's `file` previewer.
            4 => text(move || {
                fm.preview.with(|p| match p {
                    Preview::Note(note) => note.clone(),
                    _ => String::new(),
                })
            }),
            _ => message(move || {
                fm.preview.with(|p| match p {
                    Preview::Dir(Ok(_)) => "No items".into(),
                    Preview::Dir(Err(e)) => format!("Error: {}", escape(e)),
                    _ => String::new(),
                })
            }),
        },
    )
    .name("preview");
    // The preview loads off the app's thread whenever the hovered entry
    // or the pane's width changes; a result for an earlier one is
    // dropped.
    let area = pane.rect_handle();
    watch(
        move || {
            let width = area.get().map_or(0, |r| r.width);
            let entry = fm.hovered.get();
            let path = entry.as_ref().map(|e| fm.cwd.get().join(&e.name));
            (
                path,
                entry,
                width,
                fm.hidden.get(),
                fm.sort.get(),
                fm.tick.get(),
            )
        },
        move |(path, _, width, hidden, sort, _), _| {
            generation.update(|g| *g += 1);
            fm.preview_top.set(0);
            let mine = generation.get_untracked();
            let Some(path) = path else {
                fm.preview.set(Preview::Nothing);
                return;
            };
            // A slow preview does not leave the last file's on show: after
            // a moment the pane clears.
            spawn(
                || std::thread::sleep(Duration::from_millis(150)),
                move |_, _| {
                    if generation.get_untracked() == mine {
                        fm.preview.set(Preview::Nothing);
                    }
                },
            );
            spawn(
                move || load_preview(&path, width, hidden, sort),
                move |loaded, _| {
                    if generation.get_untracked() != mine {
                        return;
                    }
                    // Done: the clearing above has nothing left to do.
                    generation.update(|g| *g += 1);
                    if let Preview::Dir(Ok(entries)) = &loaded {
                        let dir = fm.hovered.get_untracked().map(|e| fm.path_of(&e));
                        let name =
                            dir.and_then(|d| fm.remember.with_untracked(|r| r.get(&d).cloned()));
                        let at = name
                            .and_then(|n| entries.iter().position(|e| e.name == n))
                            .unwrap_or(0);
                        fm.preview_cursor.set(at);
                    }
                    fm.preview.set(loaded);
                },
            );
        },
    );
    // Yazi pads the preview column by a cell on the right.
    row([pane.flex(1), label("").fixed(1)])
}

/// An image preview: drawn in half blocks with the `art` feature, else
/// described.
#[cfg(feature = "art")]
fn image_pane(fm: Fm) -> Node {
    let path = move || {
        fm.preview.with(|p| match p {
            Preview::Image(path, _) => Some(path.clone()),
            _ => None,
        })
    };
    intuituive::image_of(path, intuituive::ImageView::default())
}

#[cfg(not(feature = "art"))]
fn image_pane(fm: Fm) -> Node {
    text(move || {
        fm.preview.with(|p| match p {
            Preview::Image(_, note) => {
                format!("{note}\n[bright_black]Images are drawn with the `art` feature.")
            }
            _ => String::new(),
        })
    })
}

/// Follow the disk, as Yazi does: watch the current directory and its
/// parent, and read them again when either changes. Watches move with the
/// current directory.
#[cfg(feature = "watch")]
fn follow_disk(fm: Fm) {
    let watching = signal(Vec::<intuituive::PathWatch>::new());
    watch(
        move || fm.cwd.get(),
        move |dir, _| {
            watching.update(|old| old.drain(..).for_each(|w| w.stop()));
            let dirs = std::iter::once(dir.as_path()).chain(dir.parent());
            let started: Vec<_> = dirs
                .filter_map(|d| intuituive::watch_path(d, move |_, _| fm.refresh()).ok())
                .collect();
            watching.update(|w| *w = started);
        },
    );
}

/// Without the `watch` feature, the view changes only with what the app
/// does.
#[cfg(not(feature = "watch"))]
fn follow_disk(_fm: Fm) {}

/// `path` made absolute, as a person would write it: on Windows,
/// `canonicalize` gives a verbatim path (`\\?\C:\…`), whose prefix is
/// dropped when what follows is an ordinary drive path.
fn absolute(path: PathBuf) -> PathBuf {
    let full = path.canonicalize().unwrap_or(path);
    let text = full.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => full,
    }
}

/// Yazi's default dark theme, as the framework's named styles: the rails
/// gray, the active tab on blue, the hovered row reversed, pop-up borders
/// and the confirm's title blue, notification titles by level.
fn theme() -> Theme {
    let parse = |s: &str| Style::parse(s).expect("a style");
    let mut theme = Theme::dark()
        .style("selected", parse("reverse"))
        .style("split.divider", parse("white"))
        .style("split.divider.hover", parse("bold bright_white"))
        .style("tabs.selected", parse("bold on blue"))
        .style("tabs.selected.blur", parse("bold on blue"))
        .style("popup.title", parse("blue"))
        .style("which_key.key", parse("bright_cyan"))
        .style("notify.info", parse("green"))
        .style("notify.warn", parse("yellow"))
        .style("notify.error", parse("red"));
    theme.border = parse("blue");
    theme.border_focused = parse("blue");
    theme.title = Style::default();
    theme
}

/// The app, starting in `start`, with Nerd Font icons if `icons`.
pub fn files_app_with(start: PathBuf, icons: bool) -> App {
    App::new(move || {
        let fm = Fm::new(absolute(start), icons);
        follow_disk(fm);
        // Remember the entry hovered in each directory, for coming back.
        watch(
            move || (fm.cwd.get(), fm.hovered.get().map(|e| e.name)),
            move |(dir, name), _| {
                if let Some(name) = name {
                    fm.remember.update(|r| {
                        r.insert(dir, name);
                    });
                }
            },
        );
        // The cursor stays on the listing when it shrinks.
        watch(
            move || fm.files.with(Vec::len),
            move |len, _| {
                if fm.cursor.get_untracked() >= len {
                    fm.cursor.set(len.saturating_sub(1));
                }
            },
        );
        let columns = hsplit(
            parent_column(fm),
            hsplit(current_column(fm), preview_column(fm), fm.inner),
            fm.outer,
        );
        // The column holds Yazi's keymap and sees each key before the
        // focused list, whose own j/k/g it overrides.
        let mut root =
            column([header(fm), tab_strip(fm), columns.flex(1), status(fm)]).previews_keys();
        for &(keys, _, desc, act) in KEYS {
            root = root.bind(keys, desc, move |cx| run(fm, act, cx));
        }
        for &(keys, _, desc, act) in SEQUENCES {
            root = root.bind_sequence(keys, desc, move |cx| run(fm, act, cx));
        }
        for n in 1..=9usize {
            let desc = format!("Switch to tab {n}");
            root = root.bind(&n.to_string(), &desc, move |cx| {
                run(fm, Act::TabSwitch(n - 1), cx)
            });
        }
        root
    })
    .theme(theme())
    .which_key(true)
    .toasts_at(Corner::TopRight)
}

/// The app, starting in `start`, in plain text.
pub fn files_app(start: PathBuf) -> App {
    files_app_with(start, false)
}

/// The backend called `name`, if this build has it: termion and termwiz
/// are this crate's features of those names.
#[allow(dead_code)]
fn backend_named(name: &str) -> Result<BackendKind, String> {
    BackendKind::from_name(name).ok_or_else(|| {
        let have: Vec<&str> = BackendKind::ALL.iter().map(|kind| kind.name()).collect();
        format!(
            "no backend {name:?} in this build, which has {} (termion and termwiz \
             are features of rs-rich-intuituive)",
            have.join(", ")
        )
    })
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let invalid = |error: String| std::io::Error::new(std::io::ErrorKind::InvalidInput, error);
    // `--backend NAME` (or `--backend=NAME`), else INTUITUIVE_BACKEND, else
    // crossterm; `--icons` or `--no-icons`; the other argument is the
    // directory to start in.
    let mut backend = std::env::var("INTUITUIVE_BACKEND")
        .ok()
        .filter(|name| !name.is_empty());
    let (mut start, mut icons) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--backend" {
            let name = args.next().ok_or_else(|| {
                invalid("--backend takes a name: crossterm, termion or termwiz".into())
            })?;
            backend = Some(name);
        } else if let Some(name) = arg.strip_prefix("--backend=") {
            backend = Some(name.to_string());
        } else if arg == "--icons" || arg == "--no-icons" {
            icons = Some(arg == "--icons");
        } else {
            start = Some(PathBuf::from(arg));
        }
    }
    let backend = match backend {
        Some(name) => backend_named(&name).map_err(invalid)?,
        None => BackendKind::default(),
    };
    // Only a setting turns icons on by itself: an installed Nerd Font may
    // not be the terminal's.
    let icons = icons
        .unwrap_or_else(|| matches!(filetype::nerd_font(), filetype::NerdFont::Configured(true)));
    let start =
        start.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    // The grammars a first preview is likely to need, warmed on a worker
    // so it does not stall.
    rich_ext::syntax::preload(&["markdown", "rust", "toml", "json", "python", "sh"]);
    files_app_with(start, icons)
        .palette_key("ctrl+p")
        .run_with(backend)
}

//! A terminal music player in the style of
//! [termusic](https://github.com/tramhao/termusic), a ratatui
//! (tui-realm) app, rebuilt on intuiTUIve: its tree view, the library tree
//! on the left, the playlist, the progress gauge and the lyrics on the
//! right, the album cover over the playlist's corner and the key footer
//! below. It reuses none of termusic's code.
//!
//! Ported from termusic (<https://github.com/tramhao/termusic>), by tramhao
//! and its contributors, MIT or GPLv3. Its layout, strings, default keys and
//! behaviour are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example music --features wav,art [-- DIR]
//!
//! It needs the crate's `wav` feature (WAV files through hound) and `art`
//! (the cover, in half blocks). The library is `DIR` (else `~/Music`, else
//! the current directory).
//!
//! There is no sound: intuiTUIve's `audio` plugin captures but has no
//! output, so playback is a `transport` play head on the app's clock. A WAV file is opened with
//! `audio::wav` for its length and checked to decode; other formats are
//! listed and queued, but there is no decoder for them. Tags are read from
//! a WAV's `LIST`/`INFO` chunk, else from the file name (`Artist - Title`).
//! Lyrics come from a `.lrc` file beside the track, and the cover from the
//! first `jpg`, `jpeg` or `png` in its folder, as termusic looks for them.
//!
//! termusic's default keys. Global: q or Esc quits (asked first), Tab
//! moves the focus, Space pauses, n/N next and previous, +/- the volume,
//! 0 restarts the track, f/b seek 5 s, Ctrl+F/Ctrl+B the speed, Ctrl+G
//! gapless, F/B move the lyrics 1 s, Ctrl+S saves the playlist as m3u,
//! Ctrl+H the help, Ctrl+Shift+arrows move the cover, Ctrl+Shift+PgUp/PgDn
//! size it, Ctrl+Shift+End hides it. Library: l adds a track (or opens a
//! folder), L adds a folder, d deletes, y/p yank and paste, Enter opens a
//! folder as the root and Backspace goes up, / searches, o a, A cycle, add
//! and remove roots. Playlist: l plays, d/D delete one or all, r shuffles,
//! m cycles the loop mode, K/J move a track, s/S add random tracks or an
//! album, / searches.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use intuituive::audio;
use intuituive::prelude::*;
use intuituive::rich::Style;
use intuituive::widgets::{
    table_with, tree_keyed, tree_reveal, Column, TableOptions, TreeItem, TreeKeys,
};
use intuituive::{image_of, transport, HelpOptions, ImageFit, ImageView, Level, Transport};
use rich_ext::chart::FillGauge;

/// termusic's seek step for short tracks.
pub const SEEK: Duration = Duration::from_secs(5);
/// How far F and B move the lyrics, in milliseconds.
pub const LYRIC_STEP: i64 = 1000;
/// termusic's `random_track_quantity`: how many tracks `s` adds.
pub const RANDOM_TRACKS: usize = 20;
pub const UNKNOWN_ARTIST: &str = "Unknown Artist";
pub const UNKNOWN_TITLE: &str = "Unknown Title";
pub const UNKNOWN_ALBUM: &str = "Unknown Album";

// ---------------------------------------------------------------------------
// The library: folders and the tracks termusic can play.
// ---------------------------------------------------------------------------

/// The extensions termusic lists (`filetype_supported`).
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mkv", "mka", "mp3", "aiff", "aif", "aifc", "flac", "m4a", "mp4", "aac", "opus", "ogg", "wav",
    "webm",
];

/// Whether termusic lists `path` as a track.
pub fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Names in alphanumeric order, as termusic sorts them: runs of digits
/// compare as numbers, the rest without case.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let number = |it: &mut std::iter::Peekable<std::str::Chars>| {
                    let mut n = 0u64;
                    while let Some(d) = it.peek().and_then(|c| c.to_digit(10)) {
                        n = n.saturating_mul(10).saturating_add(u64::from(d));
                        it.next();
                    }
                    n
                };
                match number(&mut a).cmp(&number(&mut b)) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(x), Some(y)) => {
                match x.to_lowercase().cmp(y.to_lowercase()) {
                    Ordering::Equal => {}
                    other => return other,
                }
                a.next();
                b.next();
            }
        }
    }
}

/// A folder or a track of the library.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub children: Vec<Entry>,
}

/// The library under `path`: its folders and tracks, hidden files left
/// out, in alphanumeric order with folders and files mixed, as termusic's
/// scanner lists them.
pub fn scan(path: &Path) -> Entry {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    );
    let is_dir = path.is_dir();
    let mut children = Vec::new();
    if is_dir {
        if let Ok(read) = std::fs::read_dir(path) {
            for entry in read.flatten() {
                let child = entry.path();
                let hidden = entry.file_name().to_string_lossy().starts_with('.');
                if !hidden && (child.is_dir() || is_audio(&child)) {
                    children.push(scan(&child));
                }
            }
        }
        children.sort_by(|a: &Entry, b: &Entry| natural_cmp(&a.name, &b.name));
    }
    Entry {
        name,
        path: path.to_path_buf(),
        is_dir,
        children,
    }
}

/// A library entry's key in the tree: its path.
fn key(path: &Path) -> String {
    path.display().to_string()
}

impl Entry {
    /// The entry for `file`, this one or one under it.
    pub fn find(&self, file: &Path) -> Option<&Entry> {
        if self.path == file {
            return Some(self);
        }
        self.children.iter().find_map(|child| child.find(file))
    }

    /// Every track under this entry, in order.
    pub fn tracks(&self) -> Vec<PathBuf> {
        if !self.is_dir {
            return vec![self.path.clone()];
        }
        self.children.iter().flat_map(Entry::tracks).collect()
    }

    /// Every folder with tracks directly in it, in order: the albums.
    pub fn albums(&self) -> Vec<&Entry> {
        let mut out = Vec::new();
        if self.children.iter().any(|c| !c.is_dir) {
            out.push(self);
        }
        for child in self.children.iter().filter(|c| c.is_dir) {
            out.extend(child.albums());
        }
        out
    }

    /// The tree's items: this entry, keyed by its path.
    fn item(&self) -> TreeItem {
        let name = rich::markup::escape(&self.name);
        TreeItem::keyed(key(&self.path), name).children(self.children.iter().map(Entry::item))
    }
}

// ---------------------------------------------------------------------------
// Tracks and their tags.
// ---------------------------------------------------------------------------

/// What a track's tags say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
}

/// The tags in a WAV file's `LIST`/`INFO` chunk (`INAM` the title, `IART`
/// the artist, `IPRD` the album), read with std: hound skips the chunk.
pub fn riff_info(path: &Path) -> Tags {
    let mut tags = Tags::default();
    let Ok(bytes) = std::fs::read(path) else {
        return tags;
    };
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return tags;
    }
    let chunk = |at: usize| -> Option<(&[u8], &[u8])> {
        let id = bytes.get(at..at + 4)?;
        let len = u32::from_le_bytes(bytes.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8 + len).min(bytes.len()))?;
        Some((id, body))
    };
    let text = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let s = s.trim_end_matches('\0').trim();
        (!s.is_empty()).then(|| s.to_string())
    };
    let mut at = 12;
    while let Some((id, body)) = chunk(at) {
        if id == b"LIST" && body.starts_with(b"INFO") {
            let mut sub = 4;
            while sub + 8 <= body.len() {
                let field = &body[sub..sub + 4];
                let len = u32::from_le_bytes(body[sub + 4..sub + 8].try_into().unwrap()) as usize;
                let value = body
                    .get(sub + 8..(sub + 8 + len).min(body.len()))
                    .unwrap_or(&[]);
                match field {
                    b"INAM" => tags.title = text(value),
                    b"IART" => tags.artist = text(value),
                    b"IPRD" => tags.album = text(value),
                    _ => {}
                }
                sub += 8 + len + (len & 1);
            }
        }
        at += 8 + body.len() + (body.len() & 1);
    }
    tags
}

/// A track of the playlist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub path: PathBuf,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// Known for the files a decoder reads: WAV only.
    pub duration: Option<Duration>,
}

impl Track {
    /// The track at `path`: a WAV file's length and tags, else what its
    /// name says (`Artist - Title.ext`, or the stem as the title).
    pub fn read(path: &Path) -> Track {
        let is_wav = path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("wav"));
        let (mut tags, duration) = if is_wav {
            let duration = audio::wav(path, 1024).ok().map(|w| w.duration());
            (riff_info(path), duration)
        } else {
            (Tags::default(), None)
        };
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        if tags.title.is_none() {
            match stem.split_once(" - ") {
                Some((artist, title)) => {
                    tags.artist.get_or_insert_with(|| artist.trim().to_string());
                    tags.title = Some(title.trim().to_string());
                }
                None => tags.title = Some(stem),
            }
        }
        Track {
            path: path.to_path_buf(),
            title: tags.title,
            artist: tags.artist,
            album: tags.album,
            duration,
        }
    }

    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or(UNKNOWN_TITLE)
    }

    pub fn artist(&self) -> &str {
        self.artist.as_deref().unwrap_or(UNKNOWN_ARTIST)
    }

    pub fn album(&self) -> &str {
        self.album.as_deref().unwrap_or(UNKNOWN_ALBUM)
    }

    /// The playlist's duration cell: `[ 03:25 ]`, or `[--:--]`.
    pub fn duration_cell(&self) -> String {
        match self.duration {
            Some(d) => format!("[{:^7.7}]", fmt_short(d)),
            None => "[--:--]".into(),
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    }
}

/// termusic's short duration: `03:25`, or `1:02:03` past an hour.
pub fn fmt_short(d: Duration) -> String {
    let s = d.as_secs();
    let (h, m, s) = (s / 3600, (s % 3600) / 60, s % 60);
    if h == 0 {
        format!("{m:02}:{s:02}")
    } else {
        format!("{h}:{m:02}:{s:02}")
    }
}

/// The cover termusic shows for a track: the first `jpg`, `jpeg` or `png`
/// in its folder that is not `artist.*` (embedded covers need a tag reader
/// this clone does not have). Sorted, so the pick does not depend on the
/// directory's order.
pub fn cover_for(track: &Path) -> Option<PathBuf> {
    let dir = track.parent()?;
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("");
            let stem = p.file_stem().and_then(|e| e.to_str()).unwrap_or("");
            ["jpg", "jpeg", "png"]
                .iter()
                .any(|x| ext.eq_ignore_ascii_case(x))
                && !stem.eq_ignore_ascii_case("artist")
        })
        .collect();
    found.sort();
    found.into_iter().next()
}

// ---------------------------------------------------------------------------
// Lyrics: an `.lrc` file.
// ---------------------------------------------------------------------------

/// A track's synced lyrics: captions by time, and an offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lyrics {
    /// Milliseconds added to the playback time (`[offset:…]`, F and B).
    pub offset: i64,
    /// Each caption's time in milliseconds and its text, in time order.
    pub captions: Vec<(i64, String)>,
}

/// Parse an `.lrc` file: `[mm:ss.xx]text` lines (several stamps on one
/// line give the text each time), and `[offset:+/-ms]`.
pub fn parse_lrc(text: &str) -> Lyrics {
    let mut lyrics = Lyrics::default();
    for line in text.lines() {
        let mut rest = line.trim();
        let mut times = Vec::new();
        while let Some(body) = rest.strip_prefix('[') {
            let Some(end) = body.find(']') else { break };
            let tag = &body[..end];
            rest = &body[end + 1..];
            if let Some(offset) = tag.strip_prefix("offset:") {
                lyrics.offset = offset.trim().parse().unwrap_or(0);
            } else if let Some(ms) = lrc_time(tag) {
                times.push(ms);
            }
        }
        for ms in times {
            lyrics.captions.push((ms, rest.trim().to_string()));
        }
    }
    lyrics.captions.sort_by_key(|(ms, _)| *ms);
    lyrics
}

/// `mm:ss`, `mm:ss.xx` or `mm:ss.xxx` as milliseconds.
fn lrc_time(tag: &str) -> Option<i64> {
    let (m, s) = tag.split_once(':')?;
    let m: i64 = m.trim().parse().ok()?;
    let (s, frac) = s.split_once('.').unwrap_or((s, "0"));
    let s: i64 = s.trim().parse().ok()?;
    let digits = frac.len().clamp(1, 3) as u32;
    let frac: i64 = frac.get(..digits as usize)?.parse().ok()?;
    Some(m * 60_000 + s * 1000 + frac * 10i64.pow(3 - digits))
}

impl Lyrics {
    /// The caption shown at `pos`, as termusic picks it: two seconds
    /// ahead (its allowance for the client's delay) plus the offset, the
    /// first caption before any.
    pub fn index_at(&self, pos: Duration) -> Option<usize> {
        if self.captions.is_empty() {
            return None;
        }
        let time = (pos.as_millis() as i64 + 2000 + self.offset).max(0);
        Some(
            self.captions
                .iter()
                .rposition(|(ms, _)| time >= *ms)
                .unwrap_or(0),
        )
    }
}

// ---------------------------------------------------------------------------
// The player's state.
// ---------------------------------------------------------------------------

/// termusic's loop modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LoopMode {
    Track,
    Playlist,
    Random,
    PlaylistOnce,
}

impl LoopMode {
    /// The next mode, in termusic's order (`m`).
    pub fn cycle(self) -> LoopMode {
        match self {
            LoopMode::Random => LoopMode::Playlist,
            LoopMode::Playlist => LoopMode::PlaylistOnce,
            LoopMode::PlaylistOnce => LoopMode::Track,
            LoopMode::Track => LoopMode::Random,
        }
    }

    /// termusic's default symbols for the playlist's title.
    pub fn symbol(self) -> &'static str {
        match self {
            LoopMode::Track => "🔂",
            LoopMode::Playlist => "🔁",
            LoopMode::Random => "🔀",
            LoopMode::PlaylistOnce => "⮕",
        }
    }

    pub fn text(self) -> &'static str {
        match self {
            LoopMode::Track => "track",
            LoopMode::Playlist => "playlist",
            LoopMode::Random => "random",
            LoopMode::PlaylistOnce => "playlist once",
        }
    }
}

/// Whether a track is playing: none (stopped), or the play head moving or
/// not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Stopped,
    Running,
    Paused,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Status::Stopped => "Stopped",
            Status::Running => "Running",
            Status::Paused => "Paused",
        })
    }
}

/// A small deterministic generator for shuffles and random picks.
pub fn next_random(seed: &mut u64) -> u64 {
    *seed = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *seed >> 33
}

/// termusic's search pattern: `*` any run, `?` one character, without
/// case, the query wrapped in `*…*`.
pub fn wildcard(query: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let p: Vec<char> = format!("*{}*", query.to_lowercase()).chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    go(&p, &t)
}

/// Where the cover sits: shown, its width in cells, and how far it is
/// moved left and up from the playlist's bottom right corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Art {
    pub shown: bool,
    pub width: u16,
    pub left: u16,
    pub up: u16,
}

/// The app's state, every part a signal: `Copy`, so each handler takes it.
#[derive(Clone, Copy)]
pub struct Player {
    /// The library's roots (`o`, `a`, `A`) and the one shown.
    pub roots: Signal<Vec<PathBuf>>,
    pub root: Signal<usize>,
    /// The folder the tree shows, and what is in it; the tree's selection
    /// and open folders, by path.
    pub top: Signal<PathBuf>,
    pub library: Signal<Entry>,
    pub selected: Signal<Option<String>>,
    pub expanded: Signal<HashSet<String>>,
    pub yanked: Signal<Option<PathBuf>>,
    pub playlist: Signal<Vec<Track>>,
    pub cursor: Signal<usize>,
    pub current: Signal<Option<usize>>,
    /// The play head through the current track.
    pub head: Transport,
    pub volume: Signal<u16>,
    /// Tenths: 10 is 1.0.
    pub speed: Signal<i32>,
    pub gapless: Signal<bool>,
    pub mode: Signal<LoopMode>,
    pub lyrics: Signal<Option<Lyrics>>,
    pub lyric_at: Signal<usize>,
    pub art: Signal<Art>,
    pub seed: Signal<u64>,
}

impl Player {
    pub fn new(root: PathBuf, seed: u64) -> Player {
        let library = scan(&root);
        Player {
            roots: signal(vec![root.clone()]),
            root: signal(0),
            selected: signal(Some(key(&root))),
            expanded: signal(HashSet::from([key(&root)])),
            top: signal(root),
            library: signal(library),
            yanked: signal(None),
            playlist: signal(Vec::new()),
            cursor: signal(0),
            current: signal(None),
            head: transport(None),
            volume: signal(30),
            speed: signal(10),
            gapless: signal(true),
            mode: signal(LoopMode::Playlist),
            lyrics: signal(None),
            lyric_at: signal(0),
            art: signal(Art {
                shown: true,
                width: 20,
                left: 0,
                up: 0,
            }),
            seed: signal(seed),
        }
    }

    fn random(self, below: usize) -> usize {
        let mut seed = self.seed.get_untracked();
        let n = next_random(&mut seed);
        self.seed.set(seed);
        (n % below.max(1) as u64) as usize
    }

    /// Stopped, running or paused.
    pub fn status(self) -> Status {
        match (self.current.get(), self.head.is_playing()) {
            (None, _) => Status::Stopped,
            (Some(_), true) => Status::Running,
            (Some(_), false) => Status::Paused,
        }
    }

    fn status_untracked(self) -> Status {
        match (
            self.current.get_untracked(),
            self.head.is_playing_untracked(),
        ) {
            (None, _) => Status::Stopped,
            (Some(_), true) => Status::Running,
            (Some(_), false) => Status::Paused,
        }
    }

    /// The track playing (or paused).
    pub fn playing(self) -> Option<Track> {
        let at = self.current.get()?;
        self.playlist.with(|p| p.get(at).cloned())
    }

    // -- the library ------------------------------------------------------

    /// Show `dir` in the tree, read again, with `focus` selected if given.
    pub fn show_dir(self, dir: PathBuf, focus: Option<&Path>) {
        self.library.set(scan(&dir));
        self.expanded.set(HashSet::from([key(&dir)]));
        self.selected.set(Some(key(&dir)));
        self.top.set(dir);
        if let Some(focus) = focus {
            self.locate(focus);
        }
    }

    /// Select `file` in the tree, its folders opened.
    pub fn locate(self, file: &Path) {
        let items = [self.library.with_untracked(Entry::item)];
        tree_reveal(&items, &key(file), self.selected, self.expanded);
    }

    /// Read the library again: the selection stays on its entry, or, gone,
    /// on the row now in its place.
    pub fn rescan(self) {
        self.library.set(scan(&self.top.get_untracked()));
    }

    /// The selected entry of the tree.
    pub fn selected(self) -> Option<Entry> {
        let at = PathBuf::from(self.selected.get_untracked()?);
        self.library.with_untracked(|l| l.find(&at).cloned())
    }

    /// Whether the tree's top folder is selected.
    fn top_selected(self) -> bool {
        self.selected.get_untracked() == Some(key(&self.top.get_untracked()))
    }

    // -- the playlist -----------------------------------------------------

    pub fn queue(self, paths: Vec<PathBuf>) -> usize {
        let tracks: Vec<Track> = paths.iter().map(|p| Track::read(p)).collect();
        let n = tracks.len();
        self.playlist.update(|p| p.extend(tracks));
        n
    }

    /// Remove track `at`, keeping the one playing.
    pub fn remove(self, at: usize) {
        let len = self.playlist.with(Vec::len);
        if at >= len {
            return;
        }
        self.playlist.update(|p| {
            p.remove(at);
        });
        match self.current.get_untracked() {
            Some(c) if c == at => self.stop(),
            Some(c) if c > at => self.current.set(Some(c - 1)),
            _ => {}
        }
        self.cursor
            .set(self.cursor.get_untracked().min(len.saturating_sub(2)));
    }

    pub fn clear(self) {
        self.stop();
        self.playlist.set(Vec::new());
        self.cursor.set(0);
    }

    /// Move the track under the cursor by `by` (K/J), the playing one
    /// followed.
    pub fn swap(self, by: isize) {
        let at = self.cursor.get_untracked();
        let len = self.playlist.with(Vec::len);
        let Some(to) = at.checked_add_signed(by).filter(|&t| t < len) else {
            return;
        };
        self.playlist.update(|p| p.swap(at, to));
        self.current.update(|current| match *current {
            Some(c) if c == at => *current = Some(to),
            Some(c) if c == to => *current = Some(at),
            _ => {}
        });
        self.cursor.set(to);
    }

    /// Shuffle the playlist (r), the playing track followed.
    pub fn shuffle(self) {
        let mut tracks = self.playlist.get_untracked();
        let playing = self.current.get_untracked().map(|c| tracks[c].path.clone());
        for i in (1..tracks.len()).rev() {
            let j = self.random(i + 1);
            tracks.swap(i, j);
        }
        let current = playing.and_then(|p| tracks.iter().position(|t| t.path == p));
        self.playlist.set(tracks);
        self.current.set(current);
    }

    // -- playback ---------------------------------------------------------

    pub fn stop(self) {
        self.head.pause();
        self.head.seek(Duration::ZERO);
        self.current.set(None);
        self.lyrics.set(None);
    }

    /// Play track `at` from the start.
    pub fn play(self, at: usize, cx: &mut Ctx) {
        let Some(track) = self.playlist.with(|p| p.get(at).cloned()) else {
            return;
        };
        if track.duration.is_none() {
            let ext = track
                .path
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            cx.notify(
                Level::Error,
                "Error",
                format!("No decoder for .{ext} files: {}", track.file_name()),
            );
            return;
        }
        // The WAV is opened, as a player would to decode it; there is no
        // output to send its samples to.
        if let Err(error) = audio::wav(&track.path, 4096) {
            cx.notify(
                Level::Error,
                "Error",
                format!("{}: {error}", track.file_name()),
            );
            return;
        }
        let lyrics = std::fs::read_to_string(track.path.with_extension("lrc"))
            .ok()
            .map(|text| parse_lrc(&text))
            .filter(|l| !l.captions.is_empty());
        self.current.set(Some(at));
        self.lyrics.set(lyrics);
        self.head.set_length(track.duration);
        self.head.seek(Duration::ZERO);
        self.head.play();
    }

    /// Space: pause, resume, or start the playlist.
    pub fn toggle(self, cx: &mut Ctx) {
        match self.status_untracked() {
            Status::Running | Status::Paused => self.head.toggle(),
            Status::Stopped => {
                if self.playlist.with(|p| !p.is_empty()) {
                    self.play(self.cursor.get_untracked(), cx);
                }
            }
        }
    }

    /// The next track (n, and the end of one), as the loop mode says.
    pub fn next(self, cx: &mut Ctx, ended: bool) {
        let len = self.playlist.with(Vec::len);
        let Some(at) = self.current.get_untracked() else {
            return;
        };
        if len == 0 {
            return self.stop();
        }
        let to = match self.mode.get_untracked() {
            LoopMode::Track if ended => at,
            LoopMode::Random => self.random(len),
            LoopMode::PlaylistOnce if at + 1 >= len => return self.stop(),
            _ => (at + 1) % len,
        };
        self.play(to, cx);
    }

    pub fn previous(self, cx: &mut Ctx) {
        let len = self.playlist.with(Vec::len);
        if let Some(at) = self.current.get_untracked().filter(|_| len > 0) {
            self.play((at + len - 1) % len, cx);
        }
    }

    pub fn seek(self, forward: bool) {
        if self.status_untracked() != Status::Stopped {
            let by = SEEK.as_secs_f64();
            self.head.seek_by(if forward { by } else { -by });
        }
    }

    /// The playback speed, in tenths (Ctrl+F, Ctrl+B).
    pub fn set_speed(self, tenths: i32) {
        let tenths = tenths.clamp(1, 30);
        self.speed.set(tenths);
        self.head.set_rate(f64::from(tenths) / 10.0);
    }

    /// The text the progress gauge shows over its bar.
    pub fn progress_label(self) -> String {
        if self.status() == Status::Stopped {
            return "--:--".into();
        }
        let pos = fmt_short(self.head.position());
        match self.playing().and_then(|t| t.duration) {
            Some(total) if !total.is_zero() => format!("{pos}    -    {}", fmt_short(total)),
            _ => pos,
        }
    }

    pub fn progress_ratio(self) -> f64 {
        match self.status() {
            Status::Stopped => 0.0,
            _ => self.head.progress(),
        }
    }

    /// The gauge's title, termusic's.
    pub fn progress_title(self) -> String {
        format!(
            "Status: {} | Volume: {} | Speed: {:^.1} | Gapless: {}",
            self.status(),
            self.volume.get(),
            self.speed.get() as f32 / 10.0,
            if self.gapless.get() { "True" } else { "False" },
        )
    }

    /// The playlist's title, termusic's: its count, length and loop mode.
    pub fn playlist_title(self) -> String {
        let (n, total) = self.playlist.with(|p| {
            let total: Duration = p.iter().filter_map(|t| t.duration).sum();
            (p.len(), total)
        });
        format!(
            "Playlist ──┤ Total {n} tracks | {} | Mode: {} ├─",
            fmt_short(total),
            self.mode.get().symbol()
        )
    }

    pub fn lyric_title(self) -> String {
        match self.playing() {
            Some(track) if self.status() != Status::Stopped => {
                format!("Lyrics of {:^.20} - {:^.20}", track.artist(), track.title())
            }
            _ => "No track is playing".into(),
        }
    }

    /// The lines of the lyrics pane: the captions, or what stands in.
    pub fn lyric_lines(self) -> Vec<String> {
        if self.status() == Status::Stopped {
            return vec!["Stopped.".into()];
        }
        self.lyrics.with(|l| match l {
            Some(l) => l
                .captions
                .iter()
                .map(|(_, text)| rich::markup::escape(text))
                .collect(),
            None => vec!["No lyrics available.".into()],
        })
    }

    pub fn playlist_rows(self) -> Vec<Vec<String>> {
        self.playlist.with(|p| {
            p.iter()
                .map(|t| {
                    vec![
                        t.duration_cell(),
                        format!("[b]{}[/]", rich::markup::escape(t.title())),
                        rich::markup::escape(t.artist()),
                        rich::markup::escape(t.album()),
                    ]
                })
                .collect()
        })
    }

    /// Write the playlist as an m3u file in the library's root.
    pub fn save_playlist(self, name: &str) -> std::io::Result<PathBuf> {
        let root = self.roots.with(|r| r[self.root.get_untracked()].clone());
        let file = root.join(format!("{name}.m3u"));
        let body: String = self.playlist.with(|p| {
            p.iter()
                .map(|t| format!("{}\n", t.path.display()))
                .collect()
        });
        std::fs::write(&file, body)?;
        Ok(file)
    }
}

// ---------------------------------------------------------------------------
// The view.
// ---------------------------------------------------------------------------

fn library_view(m: Player) -> Node {
    // Enter and Space go to the bindings: open a folder as the root, and
    // pause.
    let tree = tree_keyed(
        move || vec![m.library.with(Entry::item)],
        m.selected,
        m.expanded,
    )
    .tree_keys(TreeKeys::default().no_toggle())
    .symbol("🦄")
    .theme_styles([
        ("selected", "bold bright_yellow"),
        ("tree.selected", "bright_yellow"),
    ])
    .name("Library")
    .section("Library", |node| {
        node.bind("l", "Add one track to playlist", move |_| {
            if let Some(entry) = m.selected().filter(|e| !e.is_dir) {
                m.queue(vec![entry.path]);
            }
        })
        .bind("L", "Add all tracks to playlist", move |cx| {
            if let Some(entry) = m.selected() {
                let n = m.queue(entry.tracks());
                cx.toast(format!("{n} tracks added"));
            }
        })
        .bind("d", "Delete track or folder", move |cx| delete(m, cx))
        .bind("y", "Yank file", move |_| {
            m.yanked.set(m.selected().map(|e| e.path));
        })
        .bind("p", "Paste file", move |cx| paste(m, cx))
        .bind("/", "Search in library", move |cx| {
            search(m, cx, Source::Library)
        })
        .bind("o", "Switch among several root folders", move |_| {
            let n = m.roots.with(Vec::len);
            let at = (m.root.get_untracked() + 1) % n;
            m.root.set(at);
            m.show_dir(m.roots.with(|r| r[at].clone()), None);
        })
        .bind("a", "Add new root folder", move |cx| {
            if let Some(dir) = m.selected().filter(|e| e.is_dir) {
                if m.roots.with(|r| !r.contains(&dir.path)) {
                    m.roots.update(|r| r.push(dir.path.clone()));
                    cx.toast(format!("Added root {}", dir.path.display()));
                }
            }
        })
        .bind(
            "A",
            "Remove current root from root folder list",
            move |cx| {
                if m.roots.with(Vec::len) < 2 {
                    cx.notify(Level::Warn, "Library", "The last root cannot be removed");
                    return;
                }
                let at = m.root.get_untracked();
                m.roots.update(|r| {
                    r.remove(at);
                });
                m.root.set(0);
                m.show_dir(m.roots.with(|r| r[0].clone()), None);
            },
        )
        .bind("s", "Search or download track from youtube", |cx| {
            cx.notify(
                Level::Warn,
                "Not in this clone",
                "YouTube search needs the network",
            );
        })
        .bind("t", "Open tag editor for tag and lyric download", |cx| {
            cx.notify(
                Level::Warn,
                "Not in this clone",
                "The tag editor needs a tag writer",
            );
        })
        .bind("enter", "Open sub directory as root", move |_| {
            match m.selected() {
                Some(e) if e.is_dir && !m.top_selected() => m.show_dir(e.path, None),
                Some(e) if !e.is_dir => {
                    m.queue(vec![e.path]);
                }
                _ => {}
            }
        })
        .bind("backspace", "Go back to parent directory", move |_| {
            let top = m.top.get_untracked();
            if let Some(parent) = top.parent() {
                m.show_dir(parent.to_path_buf(), Some(&top));
            }
        })
    });
    tree.panel("Library")
}

fn delete(m: Player, cx: &mut Ctx) {
    let Some(entry) = m.selected() else { return };
    if m.top_selected() {
        return;
    }
    let what = entry.path.clone();
    cx.confirm(
        "Are you sure you want to delete?",
        "",
        vec![entry.name.clone()],
        move |cx| {
            let result = if what.is_dir() {
                std::fs::remove_dir_all(&what)
            } else {
                std::fs::remove_file(&what)
            };
            match result {
                Ok(()) => {
                    let gone: Vec<usize> = m.playlist.with(|p| {
                        (0..p.len())
                            .rev()
                            .filter(|&i| p[i].path.starts_with(&what))
                            .collect()
                    });
                    for i in gone {
                        m.remove(i);
                    }
                    m.rescan();
                }
                Err(e) => cx.notify(Level::Error, "Error", format!("Delete failed: {e}")),
            }
        },
    );
}

fn paste(m: Player, cx: &mut Ctx) {
    let (Some(from), Some(at)) = (m.yanked.get_untracked(), m.selected()) else {
        return;
    };
    let into = if at.is_dir {
        at.path
    } else {
        at.path.parent().map(Path::to_path_buf).unwrap_or(at.path)
    };
    let Some(name) = from.file_name() else { return };
    let to = into.join(name);
    match std::fs::rename(&from, &to) {
        Ok(()) => {
            m.yanked.set(None);
            m.playlist.update(|p| {
                for t in p.iter_mut().filter(|t| t.path == from) {
                    t.path = to.clone();
                }
            });
            m.rescan();
            m.locate(&to);
        }
        Err(e) => cx.notify(Level::Error, "Error", format!("Paste failed: {e}")),
    }
}

fn playlist_view(m: Player) -> Node {
    let columns = vec![
        Column::new("Duration", Size::Fixed(9)),
        Column::new("Title", Size::Flex(1)).min(10),
        Column::new("Artist", Size::Flex(1)).min(10),
        Column::new("Album", Size::Flex(1)).min(10),
    ];
    // termusic's first column: the highlight symbol on the cursor, over
    // the playing track's ►.
    let symbols = TableOptions::default().symbol_for(3, move |i, selected| match selected {
        true => Some("🚀".into()),
        false => (m.current.get() == Some(i)).then(|| "►".into()),
    });
    let table = table_with(columns, move || m.playlist_rows(), m.cursor, symbols)
        .theme_styles([("selected", "bold bright_yellow"), ("table.header", "bold")])
        .name("Playlist")
        .section("Playlist", |node| {
            node.bind("l", "Play selected", move |cx| {
                m.play(m.cursor.get_untracked(), cx)
            })
            .bind("d", "Delete one track from playlist", move |_| {
                m.remove(m.cursor.get_untracked())
            })
            .bind("D", "Delete all tracks from playlist", move |_| m.clear())
            .bind("r", "Randomize playlist", move |_| m.shuffle())
            .bind("m", "Loop mode cycle", move |cx| {
                m.mode.update(|mode| *mode = mode.cycle());
                cx.toast(format!("Loop mode: {}", m.mode.get_untracked().text()));
            })
            .bind("/", "Search in playlist", move |cx| {
                search(m, cx, Source::Playlist)
            })
            .bind("K", "Swap track up in playlist", move |_| m.swap(-1))
            .bind("J", "Swap track down in playlist", move |_| m.swap(1))
            .bind("s", "Select random tracks to playlist", move |cx| {
                let mut all = m.library.with(Entry::tracks);
                let mut picked = Vec::new();
                while !all.is_empty() && picked.len() < RANDOM_TRACKS {
                    picked.push(all.remove(m.random(all.len())));
                }
                let n = m.queue(picked);
                cx.toast(format!("{n} random tracks added"));
            })
            .bind("S", "Select random album to playlist", move |cx| {
                let albums: Vec<Vec<PathBuf>> = m
                    .library
                    .with(|l| l.albums().iter().map(|a| a.tracks()).collect());
                if !albums.is_empty() {
                    let n = m.queue(albums[m.random(albums.len())].clone());
                    cx.toast(format!("{n} tracks of an album added"));
                }
            })
            .bind("comma", "Open sort popup menu", |cx| {
                cx.notify(
                    Level::Warn,
                    "Not in this clone",
                    "The sort menu sorts by tags",
                );
            })
        });
    table.panel_with(move || rich::markup::escape(&m.playlist_title()))
}

/// The cover, over the playlist's bottom right corner as termusic draws it
/// (its default place: the right edge, three quarters down).
fn art_view(m: Player) -> Node {
    keyed(
        move || m.art.get(),
        move |art| {
            let view = ImageView::new().fit(ImageFit::Contain);
            let cover = image_of(
                move || {
                    if m.status() == Status::Stopped {
                        return None;
                    }
                    m.playing().and_then(|t| cover_for(&t.path))
                },
                view,
            );
            // The image's box: `width` cells and a square picture's height
            // in half blocks, `left` and `up` from the corner.
            column([
                row([cover.fixed(art.width), label("").fixed(art.left)]).fixed(art.width / 2),
                label("").fixed(art.up),
            ])
        },
    )
    .shown_when(move || m.art.with(|a| a.shown))
    // Inside the playlist's border, at its bottom right.
    .place(Align::End, Align::End)
    .inset(1, 1, 1, 1)
}

fn progress_view(m: Player) -> Node {
    let gauge = renderable(move || {
        FillGauge::new(m.progress_ratio())
            .label(m.progress_label())
            .style("white")
            .height(1)
    });
    gauge
        .panel_with(move || rich::markup::escape(&m.progress_title()))
        .title_align(Align::Center)
}

fn lyric_view(m: Player) -> Node {
    let lines = list(move || m.lyric_lines(), m.lyric_at)
        .theme_styles([("selected", "bold bright_yellow")])
        .name("Lyrics");
    lines
        .panel_with(move || rich::markup::escape(&m.lyric_title()))
        .title_align(Align::Center)
}

fn footer() -> Node {
    let k = |key: &str| format!("[bold bright_yellow]<{key}>[/]");
    label(format!(
        "[bold] Help: [/]{} [bold]Config: [/]{} [bold]Library: [/]{} [bold]Database: [/]{} \
         [bold]Podcasts: [/]{} [bold]Version: [/]{}",
        k("control+h"),
        k("shift+C"),
        k("1"),
        k("2"),
        k("3"),
        "[bold bright_yellow]rs-rich[/]",
    ))
    .no_wrap()
}

// ---------------------------------------------------------------------------
// Search.
// ---------------------------------------------------------------------------

/// What `/` searches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Library,
    Playlist,
}

/// termusic's general search: a query box over a results table, in a box
/// 65% by 68% of the screen. Enter in the box goes to the results; Enter
/// there locates the item, `l` adds it to the playlist (or plays it).
fn search(m: Player, cx: &mut Ctx, source: Source) {
    cx.modal(Size::Percent(65), Size::Percent(68), move || {
        let query = signal(String::new());
        let at = signal(0usize);
        // Each result: what it points at, and its cells.
        let results = memo(move || -> Vec<(usize, PathBuf, Vec<String>)> {
            let q = query.get();
            match source {
                Source::Library => m
                    .library
                    .with(Entry::tracks)
                    .into_iter()
                    .filter(|p| {
                        let name = p.file_name().unwrap_or_default().to_string_lossy();
                        wildcard(&q, &name)
                    })
                    .enumerate()
                    .map(|(i, p)| {
                        let name = p.file_name().unwrap_or_default().to_string_lossy();
                        let cells = vec![i.to_string(), rich::markup::escape(&name)];
                        (i, p, cells)
                    })
                    .collect(),
                Source::Playlist => m.playlist.with(|list| {
                    list.iter()
                        .enumerate()
                        .filter(|(_, t)| {
                            wildcard(
                                &q,
                                &format!("{} {} {}", t.file_name(), t.artist(), t.title()),
                            )
                        })
                        .map(|(i, t)| {
                            let cells = vec![
                                t.duration_cell(),
                                rich::markup::escape(t.artist()),
                                rich::markup::escape(t.title()),
                            ];
                            (i, t.path.clone(), cells)
                        })
                        .collect()
                }),
            }
        });
        let columns = match source {
            Source::Library => vec![
                Column::new("idx", Size::Fixed(5)),
                Column::new("File name", Size::Flex(1)),
            ],
            Source::Playlist => vec![
                Column::new("Duration", Size::Fixed(9)),
                Column::new("Artist", Size::Flex(1)),
                Column::new("Title", Size::Flex(1)),
            ],
        };
        let picked = move || results.with(|r| r.get(at.get_untracked()).cloned());
        let table = table_with(
            columns,
            move || results.with(|r| r.iter().map(|(_, _, c)| c.clone()).collect()),
            at,
            TableOptions::default(),
        )
        .theme_styles([("selected", "bold bright_yellow")])
        .name("Results")
        .bind("enter", "Locate", move |cx| {
            if let Some((i, path, _)) = picked() {
                match source {
                    Source::Library => m.locate(&path),
                    Source::Playlist => m.cursor.set(i),
                }
                cx.pop();
            }
        })
        .bind("l", "Load to playlist", move |cx| {
            if let Some((i, path, _)) = picked() {
                match source {
                    Source::Library => {
                        m.queue(vec![path]);
                    }
                    Source::Playlist => m.play(i, cx),
                }
            }
        });
        let results_title = match source {
            Source::Library => "Results: (Enter: locate/l: load to playlist)",
            Source::Playlist => "Results: (Enter: locate/l: play selected)",
        };
        column([
            input(query)
                .on_submit(|_, cx| cx.focus_next())
                .autofocus()
                .name("Search")
                .panel("Search for: (support * and ?)")
                .fixed(3),
            table.panel(results_title),
        ])
        .on_key("esc", |cx| cx.pop())
    });
}

// ---------------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------------

fn theme() -> Theme {
    let parse = |s: &str| Style::parse(s).expect("a style");
    let mut theme = Theme::dark()
        .style("popup.title", parse("bright_yellow"))
        .style("table.header", parse("bold"))
        .style("notify.info", parse("green"))
        .style("notify.warn", parse("yellow"))
        .style("notify.error", parse("red"));
    theme.border = parse("default");
    theme.border_focused = parse("blue");
    theme.title = Style::default();
    theme
}

/// Move or size the cover (Ctrl+Shift with the arrows, PgUp/PgDn, End).
fn art_keys(m: Player, node: Node) -> Node {
    let change = move |f: fn(&mut Art)| move |_: &mut Ctx| m.art.update(f);
    node.section("Cover art", |node| {
        node.bind(
            "ctrl+shift+left",
            "Move album cover left",
            change(|a| a.left = (a.left + 1).min(200)),
        )
        .bind(
            "ctrl+shift+right",
            "Move album cover right",
            change(|a| a.left = a.left.saturating_sub(1)),
        )
        .bind(
            "ctrl+shift+up",
            "Move album cover up",
            change(|a| a.up = (a.up + 1).min(100)),
        )
        .bind(
            "ctrl+shift+down",
            "Move album cover down",
            change(|a| a.up = a.up.saturating_sub(1)),
        )
        .bind(
            "ctrl+shift+pageup",
            "Zoom in album cover",
            change(|a| a.width = (a.width + 2).min(120)),
        )
        .bind(
            "ctrl+shift+pagedown",
            "Zoom out album cover",
            change(|a| a.width = a.width.saturating_sub(2).max(4)),
        )
        .bind(
            "ctrl+shift+end",
            "Hide/Show album cover",
            change(|a| a.shown = !a.shown),
        )
    })
}

fn global_keys(m: Player, node: Node) -> Node {
    node.section("Global", |node| {
        node.bind("q esc", "Exit", |cx| {
            cx.confirm("Are sure you want to quit?", "", Vec::new(), |cx| cx.quit())
        })
        .bind("ctrl+h", "Help", |cx| {
            cx.help_with(HelpOptions::new().all_panes(true))
        })
        .bind("space", "Pause current track", move |cx| m.toggle(cx))
        .bind("n", "Next track", move |cx| m.next(cx, false))
        .bind("N", "Previous track", move |cx| m.previous(cx))
        .bind("plus equals", "Increase volume", move |_| {
            m.volume.update(|v| *v = (*v + 5).min(100))
        })
        .bind("minus", "Decrease volume", move |_| {
            m.volume.update(|v| *v = v.saturating_sub(5))
        })
        .bind("0", "Restart current track", move |_| {
            m.head.seek(Duration::ZERO)
        })
        .bind("f", "Seek forward 5 seconds", move |_| m.seek(true))
        .bind("b", "Seek backward 5 seconds", move |_| m.seek(false))
        .bind("ctrl+f", "Playback speed up 10 percent", move |_| {
            m.set_speed(m.speed.get_untracked() + 1)
        })
        .bind("ctrl+b", "Playback speed down 10 percent", move |_| {
            m.set_speed(m.speed.get_untracked() - 1)
        })
        .bind("ctrl+g", "Toggle gapless playback", move |_| {
            m.gapless.update(|g| *g = !*g)
        })
        .bind("F", "Adjust offset of lyrics forward 1 second", move |cx| {
            lyric_offset(m, cx, LYRIC_STEP)
        })
        .bind(
            "B",
            "Adjust offset of lyrics backward 1 second",
            move |cx| lyric_offset(m, cx, -LYRIC_STEP),
        )
        .bind("T", "Switch lyrics if more than 1 available", |cx| {
            cx.toast("Only one lyric is available")
        })
        .bind("ctrl+s", "Save Playlist to m3u", move |cx| {
            cx.prompt(
                "Save Playlist as: (Enter to confirm)",
                "",
                move |name, cx| {
                    if name.trim().is_empty() {
                        return;
                    }
                    match m.save_playlist(name.trim()) {
                        Ok(file) => cx.toast(format!("Playlist saved to {}", file.display())),
                        Err(e) => cx.notify(Level::Error, "Error", format!("Save failed: {e}")),
                    }
                },
            )
        })
        .bind("1", "Switch layout to treeview", |_| {})
        .bind("2", "Switch layout to database", |cx| {
            cx.notify(
                Level::Warn,
                "Not in this clone",
                "The database view needs a tag index",
            );
        })
        .bind("3", "Switch layout to podcast", |cx| {
            cx.notify(
                Level::Warn,
                "Not in this clone",
                "Podcasts need the network",
            );
        })
        .bind("C", "Open Config Editor(all configuration)", |cx| {
            cx.notify(Level::Warn, "Not in this clone", "There is no config file");
        })
    })
}

fn lyric_offset(m: Player, cx: &mut Ctx, by: i64) {
    if m.lyrics.with(Option::is_none) {
        return;
    }
    m.lyrics.update(|l| {
        if let Some(l) = l {
            l.offset += by;
        }
    });
    let offset = m.lyrics.with(|l| l.as_ref().map_or(0, |l| l.offset));
    cx.toast(format!("Lyric offset: {offset} ms"));
}

/// The app on the library at `root`, random picks from `seed`.
pub fn music_app_seeded(root: PathBuf, seed: u64) -> App {
    App::new(move || {
        let m = Player::new(root, seed);
        // The end of a track goes on by the loop mode.
        m.head.on_end(move |cx| m.next(cx, true));
        // The lyric line follows playback.
        let line = memo(move || {
            let pos = m.head.position();
            m.lyrics.with(|l| l.as_ref().and_then(|l| l.index_at(pos)))
        });
        watch(
            move || line.get(),
            move |at, _| m.lyric_at.set(at.unwrap_or(0)),
        );
        let right = column([
            layers([playlist_view(m), art_view(m)]).flex(1),
            progress_view(m).fixed(3),
            lyric_view(m).fixed(4),
        ]);
        let body = row([library_view(m).ratio(1, 3), right.ratio(2, 3)]);
        let root = column([body.flex(1), footer().fixed(1)]).previews_keys();
        art_keys(m, global_keys(m, root))
    })
    .theme(theme())
}

pub fn music_app(root: PathBuf) -> App {
    music_app_seeded(root, 0x5eed)
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let root = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .or_else(|| {
            let home = std::env::var_os("HOME")?;
            let music = PathBuf::from(home).join("Music");
            music.is_dir().then_some(music)
        })
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let root = root.canonicalize().unwrap_or(root);
    music_app(root).run()
}

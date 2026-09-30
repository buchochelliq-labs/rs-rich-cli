//! A file picker (#493, `rich file`): browse directories from a root.
//!
//! The listing is a [`Select`], so typing filters it fuzzily and the
//! focused entry previews beside it: a text file's first lines,
//! highlighted by its extension, or a directory's entries. Enter opens a
//! directory (or picks it, when directories may be picked) and picks a
//! file; Right opens, Left and Backspace on an empty filter go up to `..`,
//! and Ctrl+T shows or hides hidden files. Actions on entries are
//! [`TargetKind::File`] actions.
//!
//! With a root jail ([`FilePicker::jail`]), nothing outside the root is
//! listed, opened or previewed: `..` stops at the root, and a symbolic link
//! whose target is outside it is left out. Names that are not valid UTF-8
//! show with each such byte as `\xNN`, as the `rich` command spells such
//! paths; the path returned is the real one.

use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Component as PathPart, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use rich::{Console, ConsoleOptions, Renderable, Segment, Text};

use crate::component::{Component, Context, Flow, View};
use crate::components::{text, PreviewLayout, Select, Theme};
use crate::event::{Event, Key, KeyCode};
use crate::item::{Actions, Item, Preview, TargetKind};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};

/// The most entries listed from one directory.
const MAX_ENTRIES: usize = 50_000;
/// The most bytes of a file read for its preview.
const PREVIEW_BYTES: u64 = 64 * 1024;
/// The most lines of a preview.
const PREVIEW_LINES: usize = 200;
/// How long drawing a preview waits for its read, on a thread of its own:
/// a read that blocks (a stalled network mount, `/proc/kmsg`) must not
/// hold up the keys.
const PREVIEW_WAIT: Duration = Duration::from_millis(250);

/// What may be picked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FileMode {
    /// Files; directories are only opened (the default).
    #[default]
    File,
    /// Directories only; files are not listed.
    Directory,
    /// Either: Enter picks a directory, Right opens it.
    Both,
}

/// One listed entry: a name in the current directory, or `..`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    /// `None` for `..`.
    name: Option<OsString>,
    dir: bool,
}

/// `name` as text: itself when it is valid UTF-8, else with each byte
/// that is not as `\xNN` (and `\` as `\x5C`, so no two names share a
/// spelling), the way the `rich` command spells such paths.
pub fn display_name(name: &OsStr) -> String {
    if let Some(text) = name.to_str() {
        return text.to_string();
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut out = String::new();
        for chunk in name.as_bytes().utf8_chunks() {
            out.push_str(&chunk.valid().replace('\\', "\\x5C"));
            for byte in chunk.invalid() {
                out.push_str(&format!("\\x{byte:02X}"));
            }
        }
        out
    }
    #[cfg(not(unix))]
    name.to_string_lossy().into_owned()
}

/// A path as text, component by component: see [`display_name`].
pub fn display_path(path: &Path) -> String {
    match path.to_str() {
        Some(text) => text.to_string(),
        None => display_name(path.as_os_str()),
    }
}

fn hidden_name(name: &OsStr) -> bool {
    name.as_encoded_bytes().first() == Some(&b'.')
}

/// Browse from a root and pick a path. Returns the path, joined to the
/// root as given (a root of `.` gives paths relative to it).
pub struct FilePicker {
    root: PathBuf,
    /// The canonical root, when jailed.
    jail: Option<PathBuf>,
    /// The directory shown, relative to the root.
    rel: PathBuf,
    mode: FileMode,
    extensions: Vec<String>,
    show_hidden: bool,
    select: Select<Entry>,
    default: Option<PathBuf>,
    /// Why the directory could not be read, or that it was cut short.
    note: Option<String>,
    theme: Theme,
    /// Preview reads still going, or finished and not yet drawn.
    wake: Arc<PreviewWake>,
}

/// Shared by a picker and its previews: a read that outlasts
/// [`PREVIEW_WAIT`] finishes on its own thread, and the picker ticks until
/// its result has been drawn, so it shows without a key press.
#[derive(Default)]
struct PreviewWake {
    running: AtomicUsize,
    ready: AtomicBool,
}

impl PreviewWake {
    fn wants_tick(&self) -> bool {
        self.running.load(Ordering::SeqCst) > 0 || self.ready.load(Ordering::SeqCst)
    }
}

impl FilePicker {
    pub fn new(prompt: impl Into<String>, root: impl Into<PathBuf>) -> FilePicker {
        let root = root.into();
        let root = if root.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            root
        };
        let mut select = Select::new(prompt, Vec::<Item<Entry>>::new());
        select.set_kind(TargetKind::File);
        // One height for every directory, so the view does not jump.
        select.steady = true;
        let mut picker = FilePicker {
            root,
            jail: None,
            rel: PathBuf::new(),
            mode: FileMode::File,
            extensions: Vec::new(),
            show_hidden: false,
            select,
            default: None,
            note: None,
            theme: Theme::default(),
            wake: Arc::default(),
        };
        picker.load();
        picker
    }

    /// What may be picked (default: files).
    pub fn mode(mut self, mode: FileMode) -> Self {
        self.mode = mode;
        self.load();
        self
    }

    /// List only files with one of these extensions (without the dot, any
    /// case); directories are still listed, to open.
    pub fn extensions<I: IntoIterator<Item = S>, S: AsRef<str>>(mut self, extensions: I) -> Self {
        self.extensions = extensions
            .into_iter()
            .map(|e| e.as_ref().trim_start_matches('.').to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        self.load();
        self
    }

    /// Show hidden files (names starting with `.`) to begin with; Ctrl+T
    /// toggles them.
    pub fn show_hidden(mut self, on: bool) -> Self {
        self.show_hidden = on;
        self.load();
        self
    }

    /// Keep to the root: nothing outside it is listed, opened or read, and
    /// symbolic links out of it are left out. A root that cannot be
    /// resolved lists nothing.
    pub fn jail(mut self, on: bool) -> Self {
        self.jail = if on {
            Some(
                self.root
                    .canonicalize()
                    .unwrap_or_else(|_| PathBuf::from("\0")),
            )
        } else {
            None
        };
        self.load();
        self
    }

    /// The path returned without a terminal when the policy asks for
    /// defaults.
    pub fn default(mut self, path: impl Into<PathBuf>) -> Self {
        self.default = Some(path.into());
        self
    }

    /// Show at most `rows` entries at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.select = self.select.height(rows);
        self
    }

    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.select = self.select.preview(layout);
        self
    }

    /// Start with this filter text.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.select = self.select.query(query);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme.clone();
        self.select = self.select.theme(theme);
        self.load();
        self
    }

    /// Actions offered on every entry (#491).
    pub fn actions(mut self, actions: Actions) -> Self {
        self.select = self.select.actions(actions);
        self
    }

    /// The key that opens the action menu (default Ctrl+K).
    pub fn menu_key(mut self, key: Key) -> Self {
        self.select = self.select.menu_key(key);
        self
    }

    /// Report the mouse: a click focuses, a second click opens or picks,
    /// and the border beside the preview drags.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.select = self.select.with_mouse(on);
        self
    }

    /// The id of the action that picked the entry, if one did.
    pub fn action(&self) -> Option<&str> {
        self.select.action()
    }

    /// The directory shown.
    pub fn directory(&self) -> PathBuf {
        self.path_of(None)
    }

    /// The labels listed, in order (`..` first when it is there).
    pub fn labels(&self) -> Vec<String> {
        self.select
            .items()
            .iter()
            .map(|item| item.label.clone())
            .collect()
    }

    /// The path of `name` in the directory shown, or of the directory.
    fn path_of(&self, name: Option<&OsStr>) -> PathBuf {
        let mut rel = self.rel.clone();
        if let Some(name) = name {
            rel.push(name);
        }
        if self.root == Path::new(".") {
            if rel.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                rel
            }
        } else {
            self.root.join(rel)
        }
    }

    /// Whether `path` is inside the jail (always, without one).
    fn allowed(&self, path: &Path) -> bool {
        match &self.jail {
            None => true,
            Some(jail) => path.canonicalize().is_ok_and(|real| real.starts_with(jail)),
        }
    }

    fn at_root(&self) -> bool {
        self.rel.as_os_str().is_empty()
    }

    /// Read the directory shown into the list.
    fn load(&mut self) {
        let dir = self.root.join(&self.rel);
        self.note = None;
        let mut entries: Vec<(Entry, String)> = Vec::new();
        if !self.allowed(&dir) {
            self.note = Some("outside the root".into());
        } else {
            match std::fs::read_dir(&dir) {
                Err(error) => self.note = Some(format!("cannot read: {error}")),
                Ok(listing) => {
                    for entry in listing.flatten() {
                        if entries.len() >= MAX_ENTRIES {
                            self.note = Some(format!("first {MAX_ENTRIES} entries"));
                            break;
                        }
                        let name = entry.file_name();
                        if hidden_name(&name) && !self.show_hidden {
                            continue;
                        }
                        let path = entry.path();
                        let Ok(kind) = entry.file_type() else {
                            continue;
                        };
                        // A link counts as what it points to, and only when
                        // that is allowed.
                        let dir = if kind.is_symlink() {
                            if !self.allowed(&path) {
                                continue;
                            }
                            path.is_dir()
                        } else {
                            kind.is_dir()
                        };
                        if !dir && !self.file_listed(&name) {
                            continue;
                        }
                        let label = display_name(&name);
                        entries.push((
                            Entry {
                                name: Some(name),
                                dir,
                            },
                            label,
                        ));
                    }
                }
            }
        }
        entries.sort_by(|(a, x), (b, y)| {
            b.dir
                .cmp(&a.dir)
                .then_with(|| x.to_lowercase().cmp(&y.to_lowercase()))
                .then_with(|| x.cmp(y))
        });
        let up = if self.jail.is_some() {
            !self.at_root()
        } else {
            true
        };
        let mut items = Vec::with_capacity(entries.len() + 1);
        let mut values = Vec::with_capacity(entries.len() + 1);
        if up {
            items.push(Item::new(
                Entry {
                    name: None,
                    dir: true,
                },
                "..",
            ));
            values.push(display_path(&self.path_of(Some(OsStr::new("..")))));
        }
        for (entry, label) in entries {
            let path = self.path_of(entry.name.as_deref());
            values.push(display_path(&path));
            let full = self
                .root
                .join(&self.rel)
                .join(entry.name.as_deref().unwrap_or_default());
            let preview = FilePreview::new(full, self.jail.clone(), Arc::clone(&self.wake));
            let preview = Preview::Renderable(Arc::new(preview));
            let item = if entry.dir {
                Item::new(entry, format!("{label}/")).preview(preview)
            } else {
                Item::new(entry, label).preview(preview)
            };
            items.push(item);
        }
        let entries = items.len();
        self.select.replace_items(items);
        self.select.set_values(values);
        // The first entry, rather than `..`, is focused in a directory.
        if up && entries > 1 {
            self.select.focus_item(1);
        }
        self.select.heading = Some(vec![text(
            display_path(&self.directory()),
            &self.theme.hint,
        )]);
        let mut hints = String::from("→ open · ← up · ctrl+t hidden");
        if let Some(note) = &self.note {
            hints.push_str(&format!(" · {note}"));
        }
        self.select.hints = Some(hints);
    }

    fn file_listed(&self, name: &OsStr) -> bool {
        if self.mode == FileMode::Directory {
            return false;
        }
        if self.extensions.is_empty() {
            return true;
        }
        // The name's end rather than its last extension, so `tar.gz`
        // matches `a.tar.gz`; a name that is only the suffix does not.
        let name = name.to_string_lossy().to_lowercase();
        self.extensions
            .iter()
            .any(|ext| name.len() > ext.len() + 1 && name.ends_with(&format!(".{ext}")))
    }

    fn open(&mut self, name: &OsStr) {
        let target = self.root.join(&self.rel).join(name);
        if self.allowed(&target) {
            self.rel.push(name);
            self.load();
        }
    }

    fn up(&mut self) {
        if self.jail.is_some() && self.at_root() {
            return;
        }
        let from = self.rel.file_name().map(OsStr::to_os_string);
        match self.rel.components().next_back() {
            Some(PathPart::Normal(_)) => {
                self.rel.pop();
            }
            _ => self.rel.push(".."),
        }
        self.load();
        // Focus the directory just left.
        if let Some(from) = from {
            let index = self
                .select
                .items()
                .iter()
                .position(|item| item.value.name.as_deref() == Some(from.as_os_str()));
            if let Some(index) = index {
                self.select.focus_item(index);
            }
        }
    }

    fn entry(&self, index: usize) -> Entry {
        self.select.items()[index].value.clone()
    }

    fn finish(&mut self, path: PathBuf) -> Flow<PathBuf> {
        self.select.set_answer(Some(display_path(&path)));
        Flow::Done(path)
    }

    /// Enter (or a second click) on entry `index`.
    fn activate(&mut self, index: usize) -> Flow<PathBuf> {
        let entry = self.entry(index);
        self.select.reopen();
        match entry.name {
            None => self.up(),
            Some(name) if entry.dir => {
                if self.mode == FileMode::File {
                    self.open(&name);
                } else {
                    let path = self.path_of(Some(&name));
                    return self.finish(path);
                }
            }
            Some(name) => {
                if self.mode != FileMode::Directory {
                    let path = self.path_of(Some(&name));
                    return self.finish(path);
                }
            }
        }
        Flow::Continue
    }

    fn key(&mut self, key: Key) -> bool {
        let plain = key.modifiers == Default::default();
        let focused = self.select.focused().map(|index| self.entry(index));
        match key.code {
            KeyCode::Right if plain => {
                if let Some(Entry {
                    name: Some(name),
                    dir: true,
                }) = focused
                {
                    self.open(&name);
                } else if let Some(Entry { name: None, .. }) = focused {
                    self.up();
                }
            }
            KeyCode::Left if plain => self.up(),
            KeyCode::Backspace if self.select.query_text().is_empty() => self.up(),
            KeyCode::Char('t') if key.modifiers.ctrl => {
                self.show_hidden = !self.show_hidden;
                let focused = focused.and_then(|entry| entry.name);
                self.load();
                if let Some(name) = focused {
                    let index = self
                        .select
                        .items()
                        .iter()
                        .position(|item| item.value.name.as_ref() == Some(&name));
                    if let Some(index) = index {
                        self.select.focus_item(index);
                    }
                }
            }
            _ => return false,
        }
        true
    }
}

impl Component for FilePicker {
    type Output = PathBuf;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<PathBuf> {
        if let Some(key) = event.key() {
            if !self.select.menu_open() && self.key(key) {
                return Flow::Continue;
            }
        }
        match self.select.event(event, context) {
            Some(Flow::Done(indices)) => {
                let index = indices[0];
                if self.select.action().is_some() {
                    let entry = self.entry(index);
                    let path = match entry.name {
                        Some(name) => self.path_of(Some(&name)),
                        None => self.path_of(Some(OsStr::new(".."))),
                    };
                    return self.finish(path);
                }
                self.activate(index)
            }
            Some(Flow::Cancel) => Flow::Cancel,
            Some(Flow::Ignored) => Flow::Ignored,
            _ => Flow::Continue,
        }
    }

    fn keymap(&self) -> Keymap {
        let mut keymap = Keymap::new("file")
            .bind("open", keys("right"), "open the directory")
            .bind("up", keys("left"), "go up (Backspace with no filter)")
            .bind("hidden", keys("ctrl+t"), "show or hide hidden files");
        keymap.extend(self.select.visible_keymap());
        keymap
    }

    fn render(&self, context: &Context<'_>) -> View {
        // Drawing takes any preview that has finished since.
        self.wake.ready.store(false, Ordering::SeqCst);
        self.select.render(context)
    }

    fn mouse(&self) -> bool {
        Component::mouse(&self.select)
    }

    fn tick(&self) -> Option<Duration> {
        self.wake.wants_tick().then_some(PREVIEW_WAIT)
    }

    fn default_value(&self) -> Option<PathBuf> {
        self.default.clone()
    }

    fn prompt(&mut self, _: &mut dyn LineIo) -> Result<Option<PathBuf>, NotInteractive> {
        Err(NotInteractive::NoPrompt)
    }
}

/// What a preview shows, read once.
enum Shown {
    Code(String, String),
    Lines(Vec<String>),
    Note(String),
}

/// The preview of an entry: a text file's first lines, highlighted by its
/// extension; a directory's entries; or why there is nothing to show. Read
/// on a thread the first time it is drawn, waiting at most
/// [`PREVIEW_WAIT`]; a read still going shows as unavailable until it
/// finishes. Only a regular file is read (a FIFO would block), opened
/// without blocking and checked once open, within the jail.
struct FilePreview {
    path: PathBuf,
    jail: Option<PathBuf>,
    shown: OnceLock<Shown>,
    /// The read under way, once started.
    pending: Mutex<Option<Receiver<Shown>>>,
    /// [`read`], or a stand-in in tests.
    reader: fn(&Path, Option<&Path>) -> Shown,
    wake: Arc<PreviewWake>,
}

impl FilePreview {
    fn new(path: PathBuf, jail: Option<PathBuf>, wake: Arc<PreviewWake>) -> FilePreview {
        FilePreview {
            path,
            jail,
            shown: OnceLock::new(),
            pending: Mutex::new(None),
            reader: read,
            wake,
        }
    }

    /// What to show: the read, or `None` while it is still going.
    fn shown(&self) -> Option<&Shown> {
        if let Some(shown) = self.shown.get() {
            return Some(shown);
        }
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        let wait = if pending.is_some() {
            Duration::ZERO
        } else {
            let (sender, receiver) = mpsc::channel();
            let (path, jail, reader) = (self.path.clone(), self.jail.clone(), self.reader);
            let wake = Arc::clone(&self.wake);
            wake.running.fetch_add(1, Ordering::SeqCst);
            let spawned = std::thread::Builder::new()
                .name("rich-file-preview".into())
                .spawn(move || {
                    let _ = sender.send(reader(&path, jail.as_deref()));
                    // Ready before no longer running, so the picker never
                    // sees neither while the result waits to be drawn.
                    wake.ready.store(true, Ordering::SeqCst);
                    wake.running.fetch_sub(1, Ordering::SeqCst);
                });
            if let Err(error) = spawned {
                self.wake.running.fetch_sub(1, Ordering::SeqCst);
                return Some(
                    self.shown
                        .get_or_init(|| Shown::Note(format!("preview unavailable: {error}"))),
                );
            }
            *pending = Some(receiver);
            PREVIEW_WAIT
        };
        let receiver = pending.as_ref()?;
        let shown = match receiver.recv_timeout(wait) {
            Ok(shown) => shown,
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => Shown::Note("cannot read".into()),
        };
        *pending = None;
        Some(self.shown.get_or_init(|| shown))
    }
}

/// `path` opened for reading without blocking: a FIFO swapped in after
/// the listing opens at once instead of waiting for a writer, and a
/// device such as `/proc/kmsg` reads nothing rather than waiting.
fn open_nonblocking(path: &Path) -> std::io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    options.open(path)
}

/// Read what [`FilePreview`] shows for `path`.
fn read(path: &Path, jail: Option<&Path>) -> Shown {
    if let Some(jail) = jail {
        if !path.canonicalize().is_ok_and(|real| real.starts_with(jail)) {
            return Shown::Note("outside the root".into());
        }
    }
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => return Shown::Note(format!("cannot read: {error}")),
    };
    if metadata.is_dir() {
        let Ok(listing) = std::fs::read_dir(path) else {
            return Shown::Note("cannot read the directory".into());
        };
        let mut names: Vec<String> = listing
            .flatten()
            .take(MAX_ENTRIES)
            .map(|entry| {
                let dir = entry.file_type().is_ok_and(|kind| kind.is_dir());
                let name = display_name(&entry.file_name());
                if dir {
                    format!("{name}/")
                } else {
                    name
                }
            })
            .collect();
        names.sort_by_key(|name| name.to_lowercase());
        names.truncate(PREVIEW_LINES);
        if names.is_empty() {
            return Shown::Note("empty directory".into());
        }
        return Shown::Lines(names);
    }
    if !metadata.is_file() {
        return Shown::Note("not a regular file".into());
    }
    // What was opened is checked, not what the path named a moment ago.
    let file = open_nonblocking(path).and_then(|file| {
        let metadata = file.metadata()?;
        Ok((file, metadata))
    });
    let (file, metadata) = match file {
        Ok(opened) => opened,
        Err(error) => return Shown::Note(format!("cannot read: {error}")),
    };
    if !metadata.is_file() {
        return Shown::Note("not a regular file".into());
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(PREVIEW_BYTES).read_to_end(&mut bytes) {
        return Shown::Note(format!("cannot read: {error}"));
    }
    if bytes.contains(&0) {
        return Shown::Note(format!(
            "binary file, {}",
            rich::filesize::decimal(metadata.len())
        ));
    }
    let text = String::from_utf8_lossy(&bytes);
    let head: Vec<&str> = text.lines().take(PREVIEW_LINES).collect();
    let language = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("text")
        .to_lowercase();
    Shown::Code(head.join("\n"), language)
}

impl Renderable for FilePreview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let waiting = Shown::Note("preview unavailable: the read has not finished".into());
        match self.shown().unwrap_or(&waiting) {
            Shown::Code(code, language) => {
                rich::syntax::Syntax::new(code.clone(), language.clone())
                    .rich_render(console, options)
            }
            Shown::Lines(lines) => Text::new(lines.join("\n")).rich_render(console, options),
            Shown::Note(note) => {
                let style = rich::Style::parse("dim italic").expect("style");
                Text::styled(note.clone(), style).rich_render(console, options)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(preview: &FilePreview) -> Option<String> {
        match preview.shown()? {
            Shown::Note(note) => Some(note.clone()),
            Shown::Code(..) | Shown::Lines(_) => Some(String::new()),
        }
    }

    /// Drawing a preview whose read never ends comes back, with nothing to
    /// show yet; once the read ends, the next drawing shows it.
    #[test]
    fn a_read_that_blocks_does_not_hold_up_drawing() {
        let wake = Arc::new(PreviewWake::default());
        let mut preview = FilePreview::new(PathBuf::from("slow"), None, Arc::clone(&wake));
        preview.reader = |_, _| {
            std::thread::sleep(Duration::from_millis(600));
            Shown::Note("done".into())
        };
        assert!(!wake.wants_tick());
        let started = std::time::Instant::now();
        assert_eq!(note(&preview), None);
        assert!(started.elapsed() < Duration::from_millis(500));
        // The picker ticks while the read runs, and after it finishes until
        // a drawing has shown it: no key press is needed.
        assert!(wake.wants_tick());
        std::thread::sleep(Duration::from_millis(800));
        assert!(wake.wants_tick());
        assert_eq!(note(&preview).as_deref(), Some("done"));
    }

    /// A picker whose preview read outlasts the wait asks for ticks, and
    /// stops once a drawing has taken the result.
    #[test]
    fn a_picker_ticks_until_a_slow_preview_is_drawn() {
        let dir = std::env::temp_dir().join(format!("rich-file-wake-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let picker = FilePicker::new("Open", &dir);
        assert_eq!(picker.tick(), None);
        picker.wake.running.fetch_add(1, Ordering::SeqCst);
        assert_eq!(picker.tick(), Some(PREVIEW_WAIT));
        picker.wake.ready.store(true, Ordering::SeqCst);
        picker.wake.running.fetch_sub(1, Ordering::SeqCst);
        assert_eq!(picker.tick(), Some(PREVIEW_WAIT));
        let console = rich::Console::builder().width(40).build();
        let context = Context {
            console: &console,
            width: 40,
            height: 10,
        };
        let _ = picker.render(&context);
        assert_eq!(picker.tick(), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `/proc/kmsg` is a regular file whose read waits for the kernel's
    /// next message: opened without blocking, it reads nothing at once.
    #[test]
    fn a_file_whose_read_waits_is_not_waited_for() {
        let path = Path::new("/proc/kmsg");
        if !path.exists() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            read(path, None);
            let _ = sender.send(());
        });
        assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
    }
}

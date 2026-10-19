//! A preview pane that picks its renderer by the file's type: the core's
//! generic [`preview_of`](crate::preview_of) keyed by a path.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use rich::markup::escape;
use rich::{Console, Segment, Syntax};
use rich_ext::filetype;
use rich_ext::hex::HexView;
use rich_ext::listing::{read_dir, readable_size, Entry, Sort};

use super::model::dir_model_of;
use super::row::{file_list, FileListOptions};
use crate::app::Ctx;
use crate::layout::TextAlign;
use crate::node::{leaf, scroll_with, switch, text, Node};
use crate::preview::{preview_of_with, PreviewOfOptions};
use crate::reactive::{signal, Signal};
use crate::scrollbar::{Scrollbar, Show};

/// What a preview pane shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Preview {
    /// Nothing hovered, an empty file, or a preview still loading.
    #[default]
    Nothing,
    /// A directory's entries, or why they cannot be read.
    Dir(Result<Vec<Entry>, String>),
    /// The start of a text file, highlighted, one line a row.
    Code(Vec<Vec<Segment>>),
    /// The start of a binary file as a hex dump.
    Hex(Vec<Vec<Segment>>),
    /// An archive's entries, a line each (with the `archive` feature).
    Archive(Vec<Vec<Segment>>),
    /// An image file, and what it is (shown when it is not drawn: without
    /// the `art` feature).
    Image { path: PathBuf, note: String },
    /// A line about the file (an error, say), in markup.
    Note(String),
}

/// What a preview is loaded for: the pane's width, and how a directory's
/// entries are listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PreviewSettings {
    /// Columns to highlight and lay a hex dump out for.
    pub width: u16,
    pub hidden: bool,
    pub sort: Sort,
    /// The most lines of a text file highlighted (Yazi scrolls through
    /// them with J and K).
    pub max_lines: usize,
    /// The most bytes of a binary file dumped.
    pub hex_bytes: usize,
    /// How a binary file is shown: a hex dump, or what `file` says it is.
    pub binary: BinaryPreview,
}

/// How a [`preview`] shows a binary file (one that is not text, an image
/// or an archive this build reads): [`PreviewOptions::binary`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BinaryPreview {
    /// Its start as a hex dump (the default).
    #[default]
    Hex,
    /// What it is, as Yazi's `file` previewer says: `file -bL`'s
    /// classification (`ELF 64-bit LSB pie executable, …`, `data`), or a
    /// description of its MIME type where there is no `file` command
    /// ([`Previewer::file_type`]).
    FileType,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        PreviewSettings {
            width: 80,
            hidden: false,
            sort: Sort::default(),
            max_lines: 500,
            hex_bytes: 4096,
            binary: BinaryPreview::Hex,
        }
    }
}

/// A previewer: given the path, its MIME type and the settings, a
/// [`Preview`] for the files it takes, or `None` to leave a file to the
/// others. It runs on a worker thread. Make one of your own with
/// [`new`](Self::new), or take a built-in one
/// ([`file_type`](Self::file_type)).
#[derive(Clone)]
pub struct Previewer(PreviewFn);

type PreviewFn = Arc<dyn Fn(&Path, &str, &PreviewSettings) -> Option<Preview> + Send + Sync>;

impl Previewer {
    /// A previewer of your own.
    pub fn new(
        preview: impl Fn(&Path, &str, &PreviewSettings) -> Option<Preview> + Send + Sync + 'static,
    ) -> Previewer {
        Previewer(Arc::new(preview))
    }

    /// Yazi's `file` previewer: what a file is, as `file -bL` classifies
    /// it (`ELF 64-bit LSB pie executable, x86-64, …`, `data`), or, where
    /// there is no `file` command, a description of its MIME type. It
    /// takes every file but directories; put it in
    /// [`PreviewOptions::previewer`] to describe every file that way, or
    /// let [`BinaryPreview::FileType`] use it for binary files only.
    ///
    /// ```
    /// use intuituive::files::{Preview, PreviewSettings, Previewer};
    ///
    /// let path = std::env::temp_dir().join(format!("intuituive-file-type-doc-{}", std::process::id()));
    /// std::fs::write(&path, [0u8, 1, 2, 0xff]).unwrap();
    /// let shown = Previewer::file_type().preview(&path, "application/octet-stream", &PreviewSettings::default());
    /// assert!(matches!(shown, Some(Preview::Note(_))), "{shown:?}");
    /// # std::fs::remove_file(&path).ok();
    /// ```
    pub fn file_type() -> Previewer {
        Previewer::new(|path, mime, _| {
            if path.is_dir() {
                return None;
            }
            Some(Preview::Note(escape(&file_type(path, mime))))
        })
    }

    /// An external previewer, as Yazi runs `pdftoppm` and
    /// `ffmpegthumbnailer`: for files whose MIME type matches `mime` (a
    /// glob: `application/pdf`, `video/*`, `*`), run `argv` (the program,
    /// then its arguments) and show what it writes. In `argv`, `%s` is the
    /// file's path (put last when no argument has it), `%o` a file for the
    /// command to write its output to (else its standard output is read),
    /// and `%w` the pane's width in cells. Output that is an image (PNG,
    /// JPEG, GIF, BMP, WebP or PPM) is shown as one (drawn with the `art`
    /// feature), anything else as plain text. A program that is not there
    /// leaves the file to the other previewers; one that fails says why.
    ///
    /// ```
    /// use intuituive::files::{Preview, PreviewSettings, Previewer};
    ///
    /// // A PDF's first page as a PNG, or a video's thumbnail, as Yazi:
    /// let pdf = Previewer::command("application/pdf", &["pdftoppm", "-png", "-singlefile", "-f", "1", "-l", "1", "-scale-to", "800", "%s"]);
    /// let video = Previewer::command("video/*", &["ffmpegthumbnailer", "-i", "%s", "-o", "%o", "-c", "png", "-s", "800"]);
    /// # let _ = (pdf, video);
    /// # #[cfg(unix)]
    /// # {
    /// let path = std::env::temp_dir().join(format!("intuituive-command-doc-{}.txt", std::process::id()));
    /// std::fs::write(&path, "b\na\n").unwrap();
    /// let sorted = Previewer::command("text/*", &["sort", "%s"]);
    /// let shown = sorted.preview(&path, "text/plain", &PreviewSettings::default());
    /// let Some(Preview::Code(lines)) = shown else { panic!("{shown:?}") };
    /// assert_eq!(lines[0][0].text, "a");
    /// // Another type is left to the other previewers.
    /// assert_eq!(sorted.preview(&path, "image/png", &PreviewSettings::default()), None);
    /// # std::fs::remove_file(&path).ok();
    /// # }
    /// ```
    pub fn command(mime: &str, argv: &[&str]) -> Previewer {
        let glob = mime.to_string();
        let argv: Vec<String> = argv.iter().map(|a| a.to_string()).collect();
        Previewer::new(move |path, mime, settings| {
            if argv.is_empty() || !mime_matches(&glob, mime) || path.is_dir() {
                return None;
            }
            run_previewer(&argv, path, mime, settings)
        })
    }

    /// Ask it about `path`, of type `mime`.
    pub fn preview(&self, path: &Path, mime: &str, settings: &PreviewSettings) -> Option<Preview> {
        (self.0)(path, mime, settings)
    }
}

/// Whether `mime` matches `glob`, whose `*`s match any run of
/// characters.
fn mime_matches(glob: &str, mime: &str) -> bool {
    fn matches(glob: &[u8], text: &[u8]) -> bool {
        match glob.split_first() {
            None => text.is_empty(),
            Some((b'*', rest)) => (0..=text.len()).any(|i| matches(rest, &text[i..])),
            Some((c, rest)) => text
                .split_first()
                .is_some_and(|(t, text)| t.eq_ignore_ascii_case(c) && matches(rest, text)),
        }
    }
    matches(glob.as_bytes(), mime.as_bytes())
}

/// Run a [`Previewer::command`]'s `argv` on `path`: its output as an image
/// or as text; `None` when the program is not there.
fn run_previewer(
    argv: &[String],
    path: &Path,
    mime: &str,
    settings: &PreviewSettings,
) -> Option<Preview> {
    use std::process::{Command, Stdio};
    let out_file = argv
        .iter()
        .any(|a| a.contains("%o"))
        .then(|| previews_dir().join(format!("out-{}", key_of(path, settings))));
    let fill = |arg: &str| {
        let mut arg = arg.replace("%s", &path.to_string_lossy());
        arg = arg.replace("%w", &settings.width.to_string());
        if let Some(out) = &out_file {
            arg = arg.replace("%o", &out.to_string_lossy());
        }
        arg
    };
    let mut command = Command::new(&argv[0]);
    command.args(argv[1..].iter().map(|a| fill(a)));
    if !argv.iter().any(|a| a.contains("%s")) {
        command.arg(path);
    }
    if let Some(out) = &out_file {
        let _ = std::fs::create_dir_all(previews_dir());
        let _ = std::fs::remove_file(out);
    }
    let ran = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    let ran = match ran {
        Ok(ran) => ran,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            return Some(Preview::Note(format!(
                "[red]{}",
                escape(&error.to_string())
            )))
        }
    };
    let bytes = match &out_file {
        Some(out) => {
            let bytes = std::fs::read(out).unwrap_or_default();
            let _ = std::fs::remove_file(out);
            bytes
        }
        None => ran.stdout,
    };
    if !ran.status.success() || bytes.is_empty() {
        let said = String::from_utf8_lossy(&ran.stderr);
        let why = said
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .map_or_else(|| ran.status.to_string(), str::to_string);
        return Some(Preview::Note(format!(
            "[red]{}: {}",
            escape(&argv[0]),
            escape(&why)
        )));
    }
    if let Some(ext) = image_kind(&bytes) {
        let image = previews_dir().join(format!("{}.{ext}", key_of(path, settings)));
        let size = readable_size(bytes.len() as u64);
        return Some(match crate::state::write_atomic(&image, &bytes) {
            Ok(()) => Preview::Image {
                path: image,
                note: format!("[yellow]{mime}[/] by {}, {size}", escape(&argv[0])),
            },
            Err(error) => Preview::Note(format!("[red]{}", escape(&error.to_string()))),
        });
    }
    let text = String::from_utf8_lossy(&bytes);
    let lines = text
        .lines()
        .take(settings.max_lines)
        .map(|line| vec![Segment::new(line.replace('\t', "  "), None)])
        .collect();
    Some(Preview::Code(lines))
}

/// Where command previewers' images are kept: this process's folder in
/// the temporary directory.
fn previews_dir() -> PathBuf {
    std::env::temp_dir().join(format!("intuituive-previews-{}", std::process::id()))
}

/// A name for `path`'s preview at these settings, changing with the file.
fn key_of(path: &Path, settings: &PreviewSettings) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut hasher);
    settings.width.hash(&mut hasher);
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The extension of the image `bytes` are, by their first bytes.
fn image_kind(bytes: &[u8]) -> Option<&'static str> {
    Some(match bytes {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xff, 0xd8, 0xff, ..] => "jpg",
        [b'G', b'I', b'F', b'8', ..] => "gif",
        [b'B', b'M', ..] => "bmp",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        [b'P', b'1'..=b'6', b'\n' | b' ' | b'\r' | b'\t', ..] => "ppm",
        _ => return None,
    })
}

/// What `path` is: `file -bL`'s classification, else a description of
/// `mime`.
fn file_type(path: &Path, mime: &str) -> String {
    let said = std::process::Command::new("file")
        .arg("-bL")
        .arg("--")
        .arg(path)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_string())
        .filter(|said| !said.is_empty());
    said.unwrap_or_else(|| describe_mime(mime))
}

/// A few words for a MIME type, as `file` would put them where it can.
fn describe_mime(mime: &str) -> String {
    let known = match mime {
        "application/octet-stream" | "" => "data",
        "application/x-executable" | "application/x-pie-executable" => "executable",
        "application/x-sharedlib" => "shared library",
        "application/zip" => "Zip archive data",
        "application/gzip" | "application/x-gzip" => "gzip compressed data",
        "application/x-tar" => "POSIX tar archive",
        "application/pdf" => "PDF document",
        "application/x-sqlite3" | "application/vnd.sqlite3" => "SQLite 3.x database",
        _ => return mime.to_string(),
    };
    known.to_string()
}

/// Bytes read from a file's start to preview it.
const HEAD: u64 = 256 * 1024;

/// Load the preview of `path` (on a worker thread: it reads the disk and
/// highlights): a directory's entries; an image; an archive's listing
/// (with the `archive` feature); the start of a text file highlighted for
/// the width; the start of anything else as a hex dump, or what it is
/// ([`BinaryPreview`]). `previewers` are asked first, in order.
pub fn load_preview(path: &Path, settings: &PreviewSettings, previewers: &[Previewer]) -> Preview {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return Preview::Note(format!("[red]{}", escape(&error.to_string()))),
    };
    let mime = filetype::mime_for(path).unwrap_or("application/octet-stream");
    for previewer in previewers {
        if let Some(preview) = previewer.preview(path, mime, settings) {
            return preview;
        }
    }
    if meta.is_dir() {
        return Preview::Dir(
            read_dir(path, settings.hidden, settings.sort).map_err(|e| e.to_string()),
        );
    }
    let mut head = Vec::new();
    let read = std::fs::File::open(path).and_then(|file| file.take(HEAD).read_to_end(&mut head));
    if let Err(error) = read {
        return Preview::Note(format!("[red]{}", escape(&error.to_string())));
    }
    if head.is_empty() {
        return Preview::Nothing;
    }
    let size = readable_size(meta.len());
    if mime.starts_with("image/") {
        return Preview::Image {
            path: path.to_path_buf(),
            note: format!("[yellow]{mime}[/], {size}"),
        };
    }
    #[cfg(feature = "archive")]
    if let Some(preview) = archive_preview(path) {
        return preview;
    }
    let width = settings.width.max(1) as usize;
    let console = Console::builder().width(width).force_terminal(true).build();
    let mut options = console.options().update_width(width);
    options.no_wrap = Some(true);
    if head.iter().take(8 * 1024).any(|b| *b == 0) || !looks_textual(mime) {
        if settings.binary == BinaryPreview::FileType {
            return Preview::Note(escape(&file_type(path, mime)));
        }
        head.truncate(settings.hex_bytes);
        let view = HexView::new(head).collapse(true);
        return Preview::Hex(console.render_lines(&view, &options, false));
    }
    let language = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "txt".into());
    let text = String::from_utf8_lossy(&head);
    let shown: String = text
        .lines()
        .take(settings.max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    // Yazi's code preview: two-space tabs, no wrapping, and the theme's
    // colours on the terminal's own background.
    let syntax = Syntax::new(shown, language.as_str())
        .tab_size(2)
        .background_color("default");
    Preview::Code(console.render_lines(&syntax, &options, false))
}

/// Whether a type is shown as text (else as a hex dump).
fn looks_textual(mime: &str) -> bool {
    mime.starts_with("text/")
        || mime == "inode/x-empty"
        || matches!(
            mime,
            "application/json"
                | "application/x-ndjson"
                | "application/javascript"
                | "application/toml"
                | "application/yaml"
                | "application/xml"
                | "application/rtf"
        )
}

/// An archive's listing, if `path` is one this build reads.
#[cfg(feature = "archive")]
fn archive_preview(path: &Path) -> Option<Preview> {
    use rich::Style;
    use rich_ext::archive;
    archive::Format::of(path).ok()??;
    let entries = match archive::entries(path) {
        Ok(entries) => entries,
        Err(error) => {
            return Some(Preview::Note(format!(
                "[red]{}",
                escape(&error.to_string())
            )))
        }
    };
    let size_style = Style::parse("bright_black").ok();
    let dir_style = Style::parse("blue").ok();
    let lines = entries
        .iter()
        .map(|e| {
            let size = if e.dir {
                String::new()
            } else {
                readable_size(e.size)
            };
            let mut line = vec![Segment::new(format!("{size:>6} "), size_style.clone())];
            if e.dir {
                line.push(Segment::new(format!("{}/", e.path), dir_style.clone()));
            } else {
                line.push(Segment::new(e.path.clone(), None));
            }
            if let Some(to) = e.link.as_ref().filter(|t| !t.is_empty()) {
                line.push(Segment::new(format!(" -> {to}"), None));
            }
            line
        })
        .collect();
    Some(Preview::Archive(lines))
}

/// What a preview is loaded for: the path, the settings, and the refresh
/// tick.
type PreviewKey = (PathBuf, PreviewSettings, Option<u64>);

type DirClick = Rc<dyn Fn(Option<String>, &mut Ctx)>;
type Remembered = Rc<dyn Fn(&Path) -> Option<String>>;

/// How a [`preview`] pane behaves, and the signals it shares.
#[derive(Clone, Default)]
pub struct PreviewOptions {
    pub(crate) hidden: Option<Signal<bool>>,
    pub(crate) sort: Option<Signal<Sort>>,
    pub(crate) state: Option<Signal<Preview>>,
    pub(crate) top: Option<Signal<u16>>,
    pub(crate) dir_cursor: Option<Signal<usize>>,
    pub(crate) dir_offset: Option<Signal<usize>>,
    pub(crate) refresh: Option<Signal<u64>>,
    pub(crate) icons: bool,
    pub(crate) previewers: Vec<Previewer>,
    pub(crate) clear_after: Option<Duration>,
    pub(crate) on_dir_click: Option<DirClick>,
    pub(crate) remembered: Option<Remembered>,
    pub(crate) max_lines: Option<usize>,
    pub(crate) binary: BinaryPreview,
}

impl PreviewOptions {
    pub fn new() -> PreviewOptions {
        PreviewOptions::default()
    }

    /// List a directory's hidden entries as this says.
    pub fn hidden(mut self, hidden: Signal<bool>) -> PreviewOptions {
        self.hidden = Some(hidden);
        self
    }

    /// Sort a directory's entries so.
    pub fn sort(mut self, sort: Signal<Sort>) -> PreviewOptions {
        self.sort = Some(sort);
        self
    }

    /// Keep what is shown in `state` (to read it elsewhere).
    pub fn state(mut self, state: Signal<Preview>) -> PreviewOptions {
        self.state = Some(state);
        self
    }

    /// The first line shown of a text, hex or archive preview (Yazi's J
    /// and K move it); back to 0 for each new preview.
    pub fn scroll(mut self, top: Signal<u16>) -> PreviewOptions {
        self.top = Some(top);
        self
    }

    /// The entry hovered in a directory's preview, and its first row in
    /// view.
    pub fn dir_cursor(mut self, cursor: Signal<usize>, first: Signal<usize>) -> PreviewOptions {
        self.dir_cursor = Some(cursor);
        self.dir_offset = Some(first);
        self
    }

    /// Load again whenever this changes (after the files changed).
    pub fn refresh(mut self, tick: Signal<u64>) -> PreviewOptions {
        self.refresh = Some(tick);
        self
    }

    /// Nerd Font icons in a directory's preview.
    pub fn icons(mut self, on: bool) -> PreviewOptions {
        self.icons = on;
        self
    }

    /// Ask `previewer` before the built-in ones.
    pub fn previewer(
        mut self,
        previewer: impl Fn(&Path, &str, &PreviewSettings) -> Option<Preview> + Send + Sync + 'static,
    ) -> PreviewOptions {
        self.previewers.push(Previewer::new(previewer));
        self
    }

    /// Ask a [`Previewer`] (a built-in one: [`Previewer::file_type`])
    /// before the built-in ones.
    pub fn previewer_of(mut self, previewer: Previewer) -> PreviewOptions {
        self.previewers.push(previewer);
        self
    }

    /// Show a binary file as a hex dump (the default) or as what it is
    /// (Yazi's `file` previewer): [`BinaryPreview`].
    ///
    /// ```
    /// use intuituive::files::{preview, BinaryPreview, PreviewOptions};
    /// use intuituive::prelude::*;
    ///
    /// let path = std::env::temp_dir().join(format!("intuituive-binary-doc-{}.bin", std::process::id()));
    /// std::fs::write(&path, [0u8, 1, 2, 0xff]).unwrap();
    /// let shown = path.clone();
    /// let app = App::new(move || {
    ///     let shown = shown.clone();
    ///     preview(move || Some(shown.clone()), PreviewOptions::new().binary(BinaryPreview::FileType))
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(40, 2);
    /// driver.settle();
    /// // `data`, or `application/octet-stream` with no `file` command.
    /// assert!(!driver.screen().plain()[0].starts_with("00000000"));
    /// # std::fs::remove_file(&path).ok();
    /// ```
    pub fn binary(mut self, binary: BinaryPreview) -> PreviewOptions {
        self.binary = binary;
        self
    }

    /// Clear the pane when a preview takes longer than `delay` (150 ms by
    /// default), so a slow one does not leave the last file's on show.
    pub fn clear_after(mut self, delay: Duration) -> PreviewOptions {
        self.clear_after = Some(delay);
        self
    }

    /// The most lines of a text file highlighted (500 by default).
    pub fn max_lines(mut self, lines: usize) -> PreviewOptions {
        self.max_lines = Some(lines);
        self
    }

    /// Called with the entry clicked in a directory's preview (`None`
    /// below the entries).
    pub fn on_dir_click(
        mut self,
        click: impl Fn(Option<String>, &mut Ctx) + 'static,
    ) -> PreviewOptions {
        self.on_dir_click = Some(Rc::new(click));
        self
    }

    /// The entry to hover in a directory's preview: where the cursor was
    /// when that directory was last shown.
    pub fn remembered(
        mut self,
        entry: impl Fn(&Path) -> Option<String> + 'static,
    ) -> PreviewOptions {
        self.remembered = Some(Rc::new(entry));
        self
    }
}

/// A pane previewing the file or directory `path` gives (which may read
/// signals, as a hovered entry does): loaded off the app's thread whenever
/// the path, the pane's width or a setting changes, and shown by type (see
/// [`load_preview`]): the core's [`preview_of_with`] keyed by the path and
/// the settings. A result for an earlier path is dropped. Text, hex
/// and archive previews scroll; a directory's is a [`file_list`]; an image
/// is drawn with the `art` feature and described without it.
///
/// ```
/// use intuituive::files::{preview, PreviewOptions};
/// use intuituive::prelude::*;
///
/// let path = std::env::temp_dir().join(format!("intuituive-preview-doc-{}.bin", std::process::id()));
/// std::fs::write(&path, [0u8, 1, 2, 0xff]).unwrap();
/// let shown = path.clone();
/// let app = App::new(move || {
///     let shown = shown.clone();
///     preview(move || Some(shown.clone()), PreviewOptions::new())
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(80, 2);
/// driver.settle();
/// let screen = driver.screen().plain();
/// assert!(screen[0].starts_with("00000000  00 01 02 ff"), "{screen:?}");
/// # std::fs::remove_file(&path).ok();
/// ```
pub fn preview(path: impl Fn() -> Option<PathBuf> + 'static, options: PreviewOptions) -> Node {
    let state = options.state.unwrap_or_else(|| signal(Preview::Nothing));
    let top = options.top.unwrap_or_else(|| signal(0u16));
    let dir_cursor = options.dir_cursor.unwrap_or_else(|| signal(0usize));
    let dir_offset = options.dir_offset.unwrap_or_else(|| signal(0usize));
    let hidden = options.hidden.unwrap_or_else(|| signal(false));
    let sort = options.sort.unwrap_or_else(|| signal(Sort::default()));
    let refresh = options.refresh;
    let icons = options.icons;
    let on_dir_click = options.on_dir_click.clone();
    let shown_path = signal(None::<PathBuf>);
    let render = move |state: Signal<Preview>| {
        switch(
            move || {
                state.with(|p| match p {
                    Preview::Dir(Ok(e)) if !e.is_empty() => 1,
                    Preview::Code(_) | Preview::Hex(_) | Preview::Archive(_) => 2,
                    Preview::Image { .. } => 3,
                    Preview::Note(_) => 4,
                    _ => 0,
                })
            },
            move |kind| match kind {
                1 => {
                    // The directory's entries as loaded on the worker, through
                    // a model that never reads the disk itself.
                    let dir = dir_model_of(
                        move || shown_path.get(),
                        move || {
                            state.with(|p| match p {
                                Preview::Dir(listing) => listing.clone(),
                                _ => Ok(Vec::new()),
                            })
                        },
                        hidden,
                        sort,
                    );
                    let list = file_list(
                        dir,
                        dir_cursor,
                        FileListOptions::new()
                            .icons(icons)
                            .offset(dir_offset)
                            // Yazi underlines the entry the preview would hover.
                            .selected_style("underline")
                            .no_focus(),
                    );
                    let click = on_dir_click.clone();
                    list.on_mouse(move |cx, mouse| {
                        use rich_interact::{Button, MouseKind};
                        match mouse.kind {
                            MouseKind::Down(Button::Left) => {}
                            MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                            _ => return false,
                        }
                        let Some(click) = &click else {
                            return false;
                        };
                        let i = dir_offset.get_untracked() + mouse.row as usize;
                        let name = state.with_untracked(|p| match p {
                            Preview::Dir(Ok(entries)) => entries.get(i).map(|e| e.name.clone()),
                            _ => None,
                        });
                        click(name, cx);
                        true
                    })
                }
                2 => scroll_with(
                    leaf(move |_, _, _| {
                        state.with(|p| match p {
                            Preview::Code(lines)
                            | Preview::Hex(lines)
                            | Preview::Archive(lines) => lines.clone(),
                            _ => Vec::new(),
                        })
                    }),
                    top,
                )
                // Yazi's preview has no scrollbar.
                .scrollbar(Scrollbar::new().show(Show::Never))
                .no_focus(),
                3 => image_pane(state),
                // What a file is, at the top left, as Yazi's `file` previewer.
                4 => text(move || {
                    state.with(|p| match p {
                        Preview::Note(note) => note.clone(),
                        _ => String::new(),
                    })
                }),
                _ => text(move || {
                    state.with(|p| match p {
                        Preview::Dir(Ok(_)) => "No items".into(),
                        Preview::Dir(Err(e)) => format!("Error: {}", escape(e)),
                        _ => String::new(),
                    })
                })
                .text_align(TextAlign::Center),
            },
        )
        .name("preview")
    };
    let previewers = Arc::new(options.previewers);
    let remembered = options.remembered.clone();
    let clear_after = options.clear_after.unwrap_or(Duration::from_millis(150));
    let max_lines = options.max_lines.unwrap_or(500);
    let binary = options.binary;
    // The key: the path, and what its preview is loaded for.
    let key = move |width: u16| {
        let path = path()?;
        let settings = PreviewSettings {
            width,
            hidden: hidden.get(),
            sort: sort.get(),
            max_lines,
            binary,
            ..PreviewSettings::default()
        };
        Some((path, settings, refresh.map(|r| r.get())))
    };
    let load = move |(path, settings, _): &PreviewKey| load_preview(path, settings, &previewers);
    preview_of_with(
        key,
        load,
        render,
        PreviewOfOptions::new()
            .state(state)
            .clear_after(clear_after)
            .on_change(move |key| {
                top.set(0);
                if key.is_none() {
                    shown_path.set(None);
                }
            })
            .on_loaded(move |key: &PreviewKey, loaded| {
                let path = &key.0;
                if let Preview::Dir(Ok(entries)) = loaded {
                    // The entry hovered when the directory was last shown.
                    let hover = remembered.as_ref().and_then(|r| r(path));
                    let at = hover
                        .and_then(|n| entries.iter().position(|e| e.name == n))
                        .unwrap_or(0);
                    dir_cursor.set(at);
                }
                shown_path.set(Some(path.clone()));
            }),
    )
}

/// An image preview: drawn in half blocks with the `art` feature.
#[cfg(feature = "art")]
fn image_pane(state: Signal<Preview>) -> Node {
    let path = move || {
        state.with(|p| match p {
            Preview::Image { path, .. } => Some(path.clone()),
            _ => None,
        })
    };
    crate::image::image_of(path, crate::image::ImageView::default())
}

/// An image preview, described: images are drawn with the `art` feature.
#[cfg(not(feature = "art"))]
fn image_pane(state: Signal<Preview>) -> Node {
    text(move || {
        state.with(|p| match p {
            Preview::Image { note, .. } => {
                format!("{note}\n[bright_black]Images are drawn with the `art` feature.")
            }
            _ => String::new(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_globs_and_image_kinds() {
        assert!(mime_matches("video/*", "video/mp4"));
        assert!(mime_matches("*", "anything"));
        assert!(mime_matches("application/pdf", "application/PDF"));
        assert!(!mime_matches("video/*", "audio/ogg"));
        assert!(!mime_matches("text/plain", "text/plainer"));
        assert_eq!(image_kind(b"\x89PNG\r\n"), Some("png"));
        assert_eq!(image_kind(b"P6\n4 4\n255\n"), Some("ppm"));
        assert_eq!(image_kind(b"plain text"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_command_writing_an_image_is_shown_as_one() {
        let path = std::env::temp_dir().join(format!(
            "intuituive-command-image-{}.pdf",
            std::process::id()
        ));
        std::fs::write(&path, "%PDF").unwrap();
        // A stand-in for pdftoppm: a tiny PPM to the output file.
        let previewer = Previewer::command(
            "application/pdf",
            &[
                "sh",
                "-c",
                "printf 'P3\\n1 1\\n255\\n255 0 0\\n' > \"$1\"",
                "sh",
                "%o",
            ],
        );
        let shown = previewer.preview(&path, "application/pdf", &PreviewSettings::default());
        let Some(Preview::Image { path: image, note }) = shown else {
            panic!("{shown:?}")
        };
        assert!(image.extension().is_some_and(|e| e == "ppm"), "{image:?}");
        assert!(note.contains("by sh"), "{note}");
        // A failure says why; a missing program is left to the others.
        let failing = Previewer::command("*", &["sh", "-c", "echo no pages >&2; exit 1", "sh"]);
        let said = failing.preview(&path, "application/pdf", &PreviewSettings::default());
        assert_eq!(said, Some(Preview::Note("[red]sh: no pages".into())));
        let missing = Previewer::command("*", &["intuituive-no-such-previewer"]);
        assert_eq!(
            missing.preview(&path, "application/pdf", &PreviewSettings::default()),
            None
        );
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&image).ok();
    }
}

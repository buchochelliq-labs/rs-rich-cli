//! A three-column file manager in the style of [Yazi](https://github.com/sxyazi/yazi),
//! the most-starred app built on ratatui, rebuilt on intuiTUIve: the parent
//! directory, the current one, and a preview of the selection, loaded in
//! the background.
//!
//! Ported from Yazi (<https://github.com/sxyazi/yazi>), by sxyazi and its
//! contributors, MIT licence. Its three-column design, keys and behaviour
//! are theirs; this rebuild reuses none of its code.
//!
//!     cargo run -p rs-rich-intuituive --example files [-- DIR]
//!
//! j/k or ↑/↓ move · l, → or Enter opens · h or ← goes up · . shows hidden
//! files · s sorts by name or size · / filters · Esc clears the filter ·
//! ~ goes home · ? shows the keys · q quits.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::Duration;

use intuituive::interact::Input;
use intuituive::prelude::*;
use intuituive::rich::markup::escape;
use intuituive::rich::{Console, Segment, Syntax};

/// One entry of a directory.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    pub link: bool,
    pub exec: bool,
}

/// How a directory is sorted (directories always come first).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sort {
    Name,
    Size,
}

/// The entries of `dir`, directories first.
pub fn read_dir(dir: &Path, hidden: bool, sort: Sort) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<Entry> = read
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !hidden && name.starts_with('.') {
                return None;
            }
            let link = entry.file_type().is_ok_and(|t| t.is_symlink());
            // Follow links, so a link to a directory opens like one.
            let meta = std::fs::metadata(entry.path()).ok();
            let dir = meta.as_ref().is_some_and(|m| m.is_dir());
            #[cfg(unix)]
            let exec = {
                use std::os::unix::fs::PermissionsExt;
                !dir && meta
                    .as_ref()
                    .is_some_and(|m| m.permissions().mode() & 0o111 != 0)
            };
            #[cfg(not(unix))]
            let exec = false;
            Some(Entry {
                name,
                dir,
                size: meta.map_or(0, |m| m.len()),
                link,
                exec,
            })
        })
        .collect();
    entries.sort_by(|a, b| {
        b.dir.cmp(&a.dir).then_with(|| match sort {
            Sort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            Sort::Size => b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)),
        })
    });
    entries
}

/// An entry as a list row: directories blue with a slash, links cyan with
/// an arrow, executables green.
fn entry_row(entry: &Entry) -> String {
    let name = escape(&entry.name);
    match entry {
        Entry { dir: true, .. } => format!("[bold blue]{name}/[/]"),
        Entry { link: true, .. } => format!("[cyan]{name} →[/]"),
        Entry { exec: true, .. } => format!("[green]{name}*[/]"),
        _ => name,
    }
}

/// A size as people read it.
fn human(size: u64) -> String {
    let units = ["B", "K", "M", "G", "T"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size}B")
    } else {
        format!("{value:.1}{}", units[unit])
    }
}

/// What the preview column shows.
#[derive(Clone, Debug, PartialEq)]
pub enum Preview {
    Nothing,
    Loading,
    Dir(Vec<Entry>),
    /// The start of a text file, highlighted.
    Code(Vec<Vec<Segment>>),
    Binary(u64),
    Error(String),
}

/// Load the preview of `path` (on a worker thread): a directory's entries,
/// the start of a text file highlighted for `width` x `height`, or a note
/// that the file is binary. Highlighting is the slow part (a language's
/// grammar is compiled the first time it is used), so it happens here, off
/// the app's thread, as Yazi does.
pub fn load_preview(path: &Path, width: u16, height: u16) -> Preview {
    let meta = match std::fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) => return Preview::Error(error.to_string()),
    };
    if meta.is_dir() {
        return Preview::Dir(read_dir(path, false, Sort::Name));
    }
    use std::io::Read;
    let mut head = Vec::new();
    let read =
        std::fs::File::open(path).and_then(|file| file.take(64 * 1024).read_to_end(&mut head));
    if let Err(error) = read {
        return Preview::Error(error.to_string());
    }
    if head.iter().take(8 * 1024).any(|b| *b == 0) {
        return Preview::Binary(meta.len());
    }
    let language = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "txt".into());
    let text = String::from_utf8_lossy(&head);
    let shown: String = text
        .lines()
        .take(height.max(1) as usize)
        .collect::<Vec<_>>()
        .join("\n");
    let console = Console::builder()
        .width(width.max(1) as usize)
        .force_terminal(true)
        .build();
    let options = console
        .options()
        .update_width(width.max(1) as usize)
        .update_height(height.max(1) as usize);
    Preview::Code(console.render_lines(&Syntax::new(shown, language.as_str()), &options, false))
}

/// Draw `preview` in `width` x `height`.
fn draw_preview(
    preview: &Preview,
    console: &Console,
    width: u16,
    height: u16,
) -> Vec<Vec<Segment>> {
    let options = console
        .options()
        .update_width(width.max(1) as usize)
        .update_height(height.max(1) as usize);
    let markup = |m: &str| {
        let text = intuituive::rich::Text::from_markup(m)
            .unwrap_or_else(|_| intuituive::rich::Text::new(m.to_string()));
        console.render_lines(&text, &options, false)
    };
    match preview {
        Preview::Nothing => Vec::new(),
        Preview::Loading => markup("[muted]loading…"),
        Preview::Error(error) => markup(&format!("[bad]{}", escape(error))),
        Preview::Binary(size) => markup(&format!("[muted]binary file, {}", human(*size))),
        Preview::Dir(entries) if entries.is_empty() => markup("[muted]empty"),
        Preview::Dir(entries) => {
            let rows: Vec<String> = entries
                .iter()
                .take(height as usize)
                .map(entry_row)
                .collect();
            markup(&rows.join("\n"))
        }
        Preview::Code(lines) => lines.clone(),
    }
}

/// The index of the entry called `name`, or 0.
fn position(entries: &[Entry], name: Option<&str>) -> usize {
    name.and_then(|name| entries.iter().position(|e| e.name == name))
        .unwrap_or(0)
}

/// The app, starting in `start`.
pub fn files_app(start: PathBuf) -> App {
    App::new(move || {
        let cwd = signal(start.canonicalize().unwrap_or(start));
        let hidden = signal(false);
        let sort = signal(Sort::Name);
        let filter = signal(String::new());
        let selected = signal(0usize);
        // Directories are read again every two seconds, so the view follows
        // the disk; unchanged listings draw nothing.
        let tick = signal(0u64);
        every(Duration::from_secs(2), move |_| tick.update(|t| *t += 1));

        let entries = memo(move || {
            tick.get();
            let needle = filter.get().to_lowercase();
            cwd.with(|dir| read_dir(dir, hidden.get(), sort.get()))
                .into_iter()
                .filter(|e| needle.is_empty() || e.name.to_lowercase().contains(&needle))
                .collect::<Vec<_>>()
        });
        let parent = memo(move || {
            tick.get();
            cwd.with(|dir| dir.parent().map(|p| read_dir(p, hidden.get(), sort.get())))
                .unwrap_or_default()
        });
        let current = memo(move || entries.with(|e| e.get(selected.get()).cloned()));

        // The parent column follows the current directory.
        let parent_selected = signal(0usize);
        watch(
            move || (cwd.get(), parent.get()),
            move |(dir, parent), _| {
                let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                parent_selected.set(position(&parent, name.as_deref()));
            },
        );

        // The preview loads off the app's thread whenever the selection
        // moves; a result for an earlier selection is dropped.
        let preview = signal(Preview::Nothing);
        let generation = signal(0u64);
        // The pane's size when it last drew, for the worker to highlight at.
        let pane = Arc::new((AtomicU16::new(40), AtomicU16::new(20)));
        let size = pane.clone();
        watch(
            move || current.get().map(|e| cwd.with(|dir| dir.join(&e.name))),
            move |path, _| {
                generation.update(|g| *g += 1);
                let mine = generation.get_untracked();
                match path {
                    None => preview.set(Preview::Nothing),
                    Some(path) => {
                        preview.set(Preview::Loading);
                        let (width, height) = (
                            size.0.load(Ordering::Relaxed),
                            size.1.load(Ordering::Relaxed),
                        );
                        spawn(
                            move || load_preview(&path, width, height),
                            move |loaded, _| {
                                if generation.get_untracked() == mine {
                                    preview.set(loaded);
                                }
                            },
                        );
                    }
                }
            },
        );

        // Change directory, selecting `name` there (or the first entry).
        let go = move |dir: PathBuf, name: Option<String>| {
            filter.set(String::new());
            cwd.set(dir);
            selected.set(entries.with_untracked(|e| position(e, name.as_deref())));
        };
        // Re-sort or re-filter, keeping the same entry selected.
        let keep = move |change: &dyn Fn()| {
            let name = current.get_untracked().map(|e| e.name);
            change();
            selected.set(entries.with_untracked(|e| position(e, name.as_deref())));
        };

        // One row, as Yazi's: a path too long for it loses its start, not
        // the directory you are in.
        let header = leaf(move |console, width, _| {
            let dir = cwd.get();
            let home = std::env::var_os("HOME").map(PathBuf::from);
            let shown = match home.as_ref().and_then(|h| dir.strip_prefix(h).ok()) {
                Some(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                Some(rest) => format!("~/{}", rest.display()),
                None => dir.display().to_string(),
            };
            let room = (width as usize).max(2);
            let chars: Vec<char> = shown.chars().collect();
            let shown = if chars.len() > room {
                let tail: String = chars[chars.len() - (room - 1)..].iter().collect();
                format!("…{tail}")
            } else {
                shown
            };
            let markup = format!("[bold accent]{}[/]", escape(&shown));
            let text = intuituive::rich::Text::from_markup(&markup)
                .unwrap_or_else(|_| intuituive::rich::Text::new(shown.clone()));
            let mut options = console.options().update_width(room);
            options.no_wrap = Some(true);
            console.render_lines(&text, &options, false)
        })
        .fixed(1);

        let status = text(move || {
            let count = entries.with(Vec::len);
            let at = if count == 0 { 0 } else { selected.get() + 1 };
            let info = current.get().map_or(String::new(), |e| {
                if e.dir {
                    "dir".to_string()
                } else {
                    human(e.size)
                }
            });
            let sort = match sort.get() {
                Sort::Name => "name",
                Sort::Size => "size",
            };
            let mut parts = vec![format!("{at}/{count}"), info, format!("sort {sort}")];
            if hidden.get() {
                parts.push("hidden shown".into());
            }
            let filter = filter.get();
            if !filter.is_empty() {
                parts.push(format!("filter “{}”", escape(&filter)));
            }
            format!("[muted]{}[/]  [dim]? keys[/]", parts.join(" · "))
        })
        .auto();

        let parent_list = list(
            move || parent.with(|p| p.iter().map(entry_row).collect()),
            parent_selected,
        )
        .no_focus()
        .flex(1);
        let current_list = list(
            move || entries.with(|e| e.iter().map(entry_row).collect()),
            selected,
        )
        .name("current")
        .flex(4);
        let preview_pane = leaf(move |console, width, height| {
            pane.0.store(width, Ordering::Relaxed);
            pane.1.store(height, Ordering::Relaxed);
            preview.with(|p| draw_preview(p, console, width, height))
        })
        .name("preview")
        .flex(3);

        column([
            header,
            row([parent_list, current_list, preview_pane]).gap(1),
            status,
        ])
        .on_key("l right enter", move |_| {
            if let Some(entry) = current.get_untracked().filter(|e| e.dir) {
                go(cwd.get_untracked().join(&entry.name), None);
            }
        })
        .on_key("h left", move |_| {
            let dir = cwd.get_untracked();
            if let Some(up) = dir.parent() {
                let came_from = dir.file_name().map(|n| n.to_string_lossy().into_owned());
                go(up.to_path_buf(), came_from);
            }
        })
        .on_key("~", move |_| {
            if let Some(home) = std::env::var_os("HOME") {
                go(PathBuf::from(home), None);
            }
        })
        .on_key(".", move |_| keep(&|| hidden.update(|h| *h = !*h)))
        .on_key("s", move |_| {
            keep(&|| {
                sort.update(|s| {
                    *s = match s {
                        Sort::Name => Sort::Size,
                        Sort::Size => Sort::Name,
                    }
                })
            })
        })
        .on_key("/", move |cx| {
            cx.modal(Size::Percent(50), Size::Fixed(3), move || {
                component(Input::new("Filter"), move |text: String, cx| {
                    filter.set(text);
                    selected.set(0);
                    cx.pop();
                })
                .on_cancel(|cx| cx.pop())
                .panel("Filter")
            })
        })
        .on_key("esc", move |_| keep(&|| filter.set(String::new())))
        .on_key("?", |cx| cx.modal(Size::Auto, Size::Auto, help))
        .on_key("q", |cx| cx.quit())
    })
}

/// The keys, in a modal.
fn help() -> Node {
    label(
        "[b]j k[/] ↑ ↓   move\n\
         [b]l → ⏎[/]     open\n\
         [b]h ←[/]       up\n\
         [b]g G[/]       first, last\n\
         [b].[/]         hidden files\n\
         [b]s[/]         sort by name or size\n\
         [b]/[/]         filter · [b]esc[/] clears it\n\
         [b]~[/]         home\n\
         [b]q[/]         quit",
    )
    .padding(0, 1)
    .panel("Keys")
    .on_key("esc ? q", |cx| cx.pop())
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let start = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    files_app(start).run()
}

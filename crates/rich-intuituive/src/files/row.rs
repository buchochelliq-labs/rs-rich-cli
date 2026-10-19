//! A file list's rows as Yazi draws them.

use std::path::Path;
use std::rc::Rc;

use rich::markup::escape;
use rich_ext::filetype::{self, Appearance};
use rich_ext::git::{Change, GitStatus};
use rich_ext::listing::{find_ranges, owner, permissions, readable_size, smart_contains, Entry};

use super::model::DirModel;
use crate::layout::TextAlign;
use crate::list::{list_lazy, RowRects};
use crate::node::{switch, text, Node};
use crate::reactive::Signal;

/// What a file list shows at the right of each entry: Yazi's line modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Linemode {
    /// Nothing (Yazi's default).
    #[default]
    None,
    /// A file's size (`1.4K`), a directory's number of entries.
    Size,
    /// `-rw-r--r--`.
    Permissions,
    /// When it was modified, in the local time zone: `10/18 14:05`.
    Mtime,
    /// When it was created.
    Btime,
    /// `user:group`.
    Owner,
}

/// The mark in the one-cell column at the left of an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Marker {
    #[default]
    None,
    /// Selected (Space).
    Selected,
    /// In the range of visual mode.
    Marked,
    /// Yanked to copy.
    Copied,
    /// Yanked to move.
    Cut,
}

impl Marker {
    /// Yazi's marker colours: yellow, cyan, green, red.
    pub fn style(self) -> Option<&'static str> {
        Some(match self {
            Marker::None => return None,
            Marker::Selected => "on bright_yellow",
            Marker::Marked => "on bright_cyan",
            Marker::Copied => "on bright_green",
            Marker::Cut => "on bright_red",
        })
    }
}

/// How [`entry_markup`] draws an entry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntryLook<'a> {
    /// Nerd Font icons before names.
    pub icons: bool,
    /// Under the cursor: the icon takes the row's colour, and a hovered
    /// directory's icon is an open folder.
    pub hovered: bool,
    /// What a find highlights in the name.
    pub find: Option<&'a str>,
    /// The hovered entry's place among the find's matches: `[2/5]`.
    pub found: Option<(usize, usize)>,
}

/// An entry as Yazi's entity draws it, in console markup: the icon, the
/// name with a find's matches picked out, the place among them, and where
/// a link points. The type's colour is the row's, not the markup's (see
/// [`file_list`]), so the hovered row's bar takes it.
///
/// ```
/// use intuituive::files::{entry_markup, Entry, EntryLook};
///
/// let entry = Entry::read(&std::env::temp_dir(), "no-such-file.md");
/// let look = EntryLook { find: Some("such"), ..EntryLook::default() };
/// assert_eq!(entry_markup(&entry, &look), "no-[bold italic underline yellow]such[/]-file.md");
/// ```
pub fn entry_markup(entry: &Entry, look: &EntryLook) -> String {
    let mut out = String::new();
    if look.icons {
        let icon = filetype::icon_with(
            Path::new(&entry.name),
            entry.flags(look.hovered),
            Appearance::Dark,
        );
        if look.hovered {
            out.push_str(&format!("{} ", icon.glyph));
        } else {
            out.push_str(&format!("[{}]{}[/] ", icon.color, icon.glyph));
        }
    }
    let ranges = look
        .find
        .map(|q| find_ranges(&entry.name, q))
        .unwrap_or_default();
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
    if let Some((at, of)) = look.found {
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

/// The line mode's text for `entry` (at `path`), with Yazi's padding cell
/// after it; empty for [`Linemode::None`]. The size of a directory is how
/// many entries it has (read now).
pub fn linemode_cell(entry: &Entry, mode: Linemode, path: &Path) -> String {
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

fn short_time(time: std::time::SystemTime) -> String {
    rich_ext::fs::short_local_time(time)
}

/// The permissions coloured as Yazi's status line does: `r` yellow, `w`
/// red, `x` cyan, `-` grey.
pub fn coloured_permissions(mode: u32) -> String {
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

/// The marker of the entry at an index (`None` in a column without a
/// cursor of its own, such as the parent's) and path.
pub(crate) type MarkerFn = Rc<dyn Fn(Option<usize>, &Path) -> Marker>;

/// How a [`file_list`] looks and what it reads besides its entries.
#[derive(Clone, Default)]
pub struct FileListOptions {
    pub(crate) icons: bool,
    pub(crate) linemode: Option<Signal<Linemode>>,
    pub(crate) markers: Option<MarkerFn>,
    pub(crate) find: Option<Signal<Option<String>>>,
    pub(crate) git: Option<Signal<Option<GitStatus>>>,
    pub(crate) offset: Option<Signal<usize>>,
    pub(crate) rects: Option<RowRects>,
    pub(crate) lead: bool,
    pub(crate) selected_style: Option<String>,
    pub(crate) focus: bool,
}

impl FileListOptions {
    /// Plain text, no line mode, no markers; the list takes the focus.
    pub fn new() -> FileListOptions {
        FileListOptions {
            focus: true,
            ..FileListOptions::default()
        }
    }

    /// Nerd Font icons before names (check
    /// [`nerd_font`](rich_ext::filetype::nerd_font) first).
    pub fn icons(mut self, on: bool) -> FileListOptions {
        self.icons = on;
        self
    }

    /// The line mode at the right of each row.
    pub fn linemode(mut self, mode: Signal<Linemode>) -> FileListOptions {
        self.linemode = Some(mode);
        self
    }

    /// The marker of each entry, by its index and path.
    pub fn markers(mut self, marker: impl Fn(usize, &Path) -> Marker + 'static) -> FileListOptions {
        self.markers = Some(Rc::new(move |i, path| match i {
            Some(i) => marker(i, path),
            None => Marker::None,
        }));
        self
    }

    pub(crate) fn marker_fn(mut self, marker: Option<MarkerFn>) -> FileListOptions {
        self.markers = marker;
        self
    }

    /// Pick out what the find query matches in each name (smart case),
    /// and show the hovered entry's place among the matches.
    pub fn find(mut self, query: Signal<Option<String>>) -> FileListOptions {
        self.find = Some(query);
        self
    }

    /// A git sign before the line mode: `M`, `A`, `?`… in Yazi's git
    /// colours (its glyphs with icons on). See
    /// [`git_status`](crate::files::git_status).
    pub fn git(mut self, status: Signal<Option<GitStatus>>) -> FileListOptions {
        self.git = Some(status);
        self
    }

    /// Keep the first row in view in `first` (both ways), for the mouse.
    pub fn offset(mut self, first: Signal<usize>) -> FileListOptions {
        self.offset = Some(first);
        self
    }

    /// Record where each row in view is drawn (a rename prompt opens
    /// below the hovered row).
    pub fn row_rects(mut self, rects: &RowRects) -> FileListOptions {
        self.rects = Some(rects.clone());
        self
    }

    /// A padding cell before each entry, as Yazi's parent column has.
    pub fn lead(mut self, on: bool) -> FileListOptions {
        self.lead = on;
        self
    }

    /// The hovered row in this style instead of the theme's `selected`
    /// (Yazi underlines a directory preview's entry).
    pub fn selected_style(mut self, style: &str) -> FileListOptions {
        self.selected_style = Some(style.to_string());
        self
    }

    /// Never take the focus (a column beside the one the keys move).
    pub fn no_focus(mut self) -> FileListOptions {
        self.focus = false;
        self
    }
}

/// The git sign for `path`, with the padding cell after it.
fn git_cell(status: &GitStatus, path: &Path, icons: bool) -> String {
    match status.of(path) {
        Some(Change::Ignored) | None => String::new(),
        Some(change) => {
            let sign = if icons {
                change.glyph().to_string()
            } else {
                change.letter().to_string()
            };
            format!("[{}]{}[/] ", change.style(), escape(&sign))
        }
    }
}

/// The entries of `model` as a lazy list, one row each, built only for the
/// rows in view, `cursor` the hovered one, as Yazi's columns draw them:
/// the type's colour across each row ([`style_for`](filetype::style_for)
/// of the [sniffed](DirModel::sniff) or named type), so the hovered row's
/// reversed bar takes it; a marker column at the left; the entry
/// ([`entry_markup`]); a git sign and the line mode at the right. An empty
/// directory says `No items`, a filter that leaves nothing `No filter
/// results`, and one that cannot be read why.
///
/// ```
/// use intuituive::files::{dir_model, file_list, FileListOptions, Linemode};
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-file-list-doc-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("a.txt"), "12345").unwrap();
/// let shown = dir.clone();
/// let app = App::new(move || {
///     let shown = shown.clone();
///     let model = dir_model(move || Some(shown.clone()));
///     let options = FileListOptions::new().linemode(signal(Linemode::Size));
///     file_list(model, signal(0), options).on_key("q", |cx| cx.quit())
/// });
/// assert_eq!(app.render_with(&["q"], 12, 1).unwrap()[0], "a.txt    5B ");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn file_list(model: DirModel, cursor: Signal<usize>, options: FileListOptions) -> Node {
    let options = Rc::new(options);
    switch(
        move || model.is_empty(),
        move |empty| {
            if empty {
                return text(move || {
                    if !model.filter.get().is_empty() {
                        "No filter results".into()
                    } else {
                        match model.listing().get() {
                            Err(e) => format!("Error: {}", escape(&e)),
                            Ok(_) => "No items".into(),
                        }
                    }
                })
                .text_align(TextAlign::Center);
            }
            build(model, cursor, options.clone())
        },
    )
}

fn build(model: DirModel, cursor: Signal<usize>, options: Rc<FileListOptions>) -> Node {
    let entries = model.entries();
    let find = options.find;
    // The hovered entry's place among the find's matches.
    let found = move || -> Option<(usize, usize)> {
        let query = find?.get().filter(|q| !q.is_empty())?;
        let at = cursor.get();
        entries.with(|f| {
            let hits: Vec<usize> = (0..f.len())
                .filter(|&i| smart_contains(&f[i].name, &query))
                .collect();
            let place = hits.iter().position(|&i| i == at)?;
            Some((place + 1, hits.len()))
        })
    };
    let item = {
        let options = options.clone();
        move |i: usize| {
            let query = find.and_then(|f| f.get());
            let at = cursor.get();
            let place = if i == at { found() } else { None };
            let lead = if options.lead { " " } else { "" };
            entries.with(|f| {
                f.get(i).map_or(String::new(), |e| {
                    let look = EntryLook {
                        icons: options.icons,
                        hovered: i == at,
                        find: query.as_deref(),
                        found: place,
                    };
                    format!("{lead}{}", entry_markup(e, &look))
                })
            })
        }
    };
    let mut node = list_lazy(move || entries.with(Vec::len), item, cursor)
        .row_style(move |i| {
            let entry = entries.with(|f| f.get(i).cloned())?;
            filetype::style_for(model.mime(&entry), entry.flags(false)).map(str::to_string)
        })
        .scroll_padding(5);
    if options.linemode.is_some() || options.git.is_some() {
        let (linemode, git, icons) = (options.linemode, options.git, options.icons);
        node = node.trailing(move |i| {
            let mode = linemode.map_or(Linemode::None, |m| m.get());
            let dir = model.dir();
            entries.with(|f| {
                let (Some(e), Some(dir)) = (f.get(i), dir.as_ref()) else {
                    return String::new();
                };
                let path = dir.join(&e.name);
                let sign = git
                    .and_then(|g| g.with(|s| s.as_ref().map(|s| git_cell(s, &path, icons))))
                    .unwrap_or_default();
                format!("{sign}{}", linemode_cell(e, mode, &path))
            })
        });
    }
    if let Some(marker) = options.markers.clone() {
        node = node.marker(move |i| {
            let dir = model.dir()?;
            let path = entries.with(|f| f.get(i).map(|e| dir.join(&e.name)))?;
            marker(Some(i), &path).style().map(str::to_string)
        });
    }
    if let Some(style) = options.selected_style.clone() {
        node = node.selected_style_for(move |_| Some(style.clone()));
    }
    if let Some(first) = options.offset {
        node = node.offset(first);
    }
    if let Some(rects) = &options.rects {
        node = node.row_rects(rects);
    }
    if !options.focus {
        node = node.no_focus();
    }
    node
}

//! Miller columns: the parent directory, the current one and a preview,
//! over one cursor: the core's generic [`Miller`](crate::miller::Miller)
//! over a directory tree, with Yazi's rows and previews.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use rich_ext::git::GitStatus;
use rich_ext::listing::{read_dir, Entry, Sort};

use super::model::{dir_model_with, DirModel};
use super::preview::{preview, Preview, PreviewOptions, PreviewSettings};
use super::row::{file_list, FileListOptions, Linemode, Marker, MarkerFn};
use crate::app::Ctx;
use crate::list::RowRects;
use crate::miller::{miller_in, MillerSource};
use crate::node::{label, row, Node};
use crate::reactive::{memo, signal, Memo, Signal};
use crate::screen::Rect;

/// A directory tree as the core's Miller columns browse it: the current
/// directory's and the parent's items come from their models (sorted,
/// filtered, followed on disk), any other directory is read.
#[derive(Clone, Copy)]
struct DirTree {
    cwd: Signal<PathBuf>,
    current: DirModel,
    parent: DirModel,
    find: Signal<Option<String>>,
    preview_top: Signal<u16>,
}

impl DirTree {
    /// The model listing `at`, if one does (subscribing to the current
    /// directory when `track`).
    fn model(&self, at: &Path, track: bool) -> Option<DirModel> {
        let is = |cwd: &PathBuf| {
            if cwd == at {
                Some(self.current)
            } else if cwd.parent() == Some(at) {
                Some(self.parent)
            } else {
                None
            }
        };
        if track {
            self.cwd.with(is)
        } else {
            self.cwd.with_untracked(is)
        }
    }
}

impl MillerSource for DirTree {
    type Key = PathBuf;

    fn items(&self, at: &PathBuf) -> Vec<String> {
        let names = |entries: &Vec<Entry>| entries.iter().map(|e| e.name.clone()).collect();
        match self.model(at, true) {
            Some(model) => model.entries().with(names),
            None => {
                let (hidden, sort) = (self.current.hidden.get(), self.current.sort.get());
                read_dir(at, hidden, sort)
                    .map(|e| names(&e))
                    .unwrap_or_default()
            }
        }
    }

    fn child(&self, at: &PathBuf, name: &str) -> Option<PathBuf> {
        let dir = match self.model(at, false) {
            Some(model) => model
                .entries()
                .with_untracked(|e| e.iter().find(|e| e.name == name).map(|e| e.dir))?,
            None => at.join(name).is_dir(),
        };
        dir.then(|| at.join(name))
    }

    fn parent(&self, at: &PathBuf) -> Option<(PathBuf, String)> {
        let up = at.parent()?;
        let name = at.file_name()?.to_string_lossy().into_owned();
        Some((up.to_path_buf(), name))
    }

    /// A fresh view: the find cleared, the filter too, the preview
    /// scrolled back.
    fn arrive(&self, _at: &PathBuf) {
        self.find.set(None);
        self.current.filter.set(String::new());
        self.preview_top.set(0);
    }
}

/// Miller columns over a directory tree, as Yazi lays them out: the
/// parent directory (the current one hovered in it), the current
/// directory (the cursor's column) and a preview of the hovered entry,
/// side by side in panes whose rails the mouse drags: the core's generic
/// [`Miller`](crate::miller::Miller) over the directory tree, with
/// [`file_list`] columns and a [`preview`] by type. All of it is signals,
/// and the struct is `Copy`, so any handler takes one. Made by
/// [`miller`]; drawn by [`view`](Self::view).
///
/// - Moving: [`arrow`](Self::arrow), [`move_to`](Self::move_to),
///   [`page`](Self::page), [`enter`](Self::enter),
///   [`leave`](Self::leave), [`cd`](Self::cd); or bind Yazi's keys with
///   [`navigation`](Self::navigation).
/// - Each directory remembers the entry last hovered in it: going back
///   up, or into it again, hovers that entry, and its preview underlines
///   it.
/// - The mouse: a click in the current column hovers an entry and the
///   wheel moves the cursor; a click in the parent column goes to that
///   entry's directory (below the entries, up a level), one in a
///   directory's preview enters it there.
/// - With the `watch` feature both directories are followed on disk; the
///   current one's file types are [sniffed](DirModel::sniff) in the
///   background.
#[derive(Clone, Copy)]
pub struct Miller {
    /// The current directory.
    pub cwd: Signal<PathBuf>,
    /// Its entries.
    pub current: DirModel,
    /// The parent's entries (none at the root).
    pub parent: DirModel,
    /// The hovered entry's index in `current`.
    pub cursor: Signal<usize>,
    /// The hovered entry.
    pub hovered: Memo<Option<Entry>>,
    /// Show hidden files, in all three columns.
    pub hidden: Signal<bool>,
    pub sort: Signal<Sort>,
    /// The current column's line mode.
    pub linemode: Signal<Linemode>,
    /// The find query the current column highlights.
    pub find: Signal<Option<String>>,
    /// What the preview shows, and its first line in view (J and K).
    pub preview: Signal<Preview>,
    pub preview_top: Signal<u16>,
    columns: crate::miller::Miller<DirTree>,
    preview_cursor: Signal<usize>,
    preview_first: Signal<usize>,
    icons: bool,
}

/// What a [`Miller::view_with`] draws besides the entries.
#[derive(Clone, Default)]
pub struct MillerOptions {
    markers: Option<MarkerFn>,
    git: Option<Signal<Option<GitStatus>>>,
    previewers: PreviewOptions,
}

impl MillerOptions {
    pub fn new() -> MillerOptions {
        MillerOptions::default()
    }

    /// The marker of each entry: by its index in the current column, or
    /// `None` in the parent column (which has no visual range).
    pub fn markers(
        mut self,
        marker: impl Fn(Option<usize>, &Path) -> Marker + 'static,
    ) -> MillerOptions {
        self.markers = Some(Rc::new(marker));
        self
    }

    /// Git signs in the current column (see
    /// [`git_status`](crate::files::git_status)).
    pub fn git(mut self, status: Signal<Option<GitStatus>>) -> MillerOptions {
        self.git = Some(status);
        self
    }

    /// A previewer of your own, asked before the built-in ones (see
    /// [`PreviewOptions::previewer`]).
    pub fn previewer(
        mut self,
        previewer: impl Fn(&Path, &str, &PreviewSettings) -> Option<Preview> + Send + Sync + 'static,
    ) -> MillerOptions {
        self.previewers = self.previewers.previewer(previewer);
        self
    }
}

/// [`Miller`] columns starting in `start`, hidden files left out, sorted as
/// Yazi does by default, in Yazi's 1:4:3 shares. Call it while the app is
/// built.
///
/// ```
/// use intuituive::files::miller;
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-miller-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("inner")).unwrap();
/// std::fs::write(dir.join("inner/deep.txt"), "").unwrap();
/// let start = dir.clone();
/// let app = App::new(move || {
///     let columns = miller(start.clone());
///     columns.navigation(columns.view()).on_key("q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// // `inner` is hovered; its entries are previewed; `l` enters it.
/// let screen = app.render_with(&["l", "q"], 60, 3).unwrap();
/// assert!(screen[0].contains(" inner") && screen[0].contains("deep.txt"), "{screen:?}");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn miller(start: impl Into<PathBuf>) -> Miller {
    let cwd = signal(start.into());
    let hidden = signal(false);
    let sort = signal(Sort::default());
    let current = dir_model_with(move || Some(cwd.get()), hidden, sort).sniff();
    let parent = dir_model_with(
        move || cwd.with(|d| d.parent().map(Path::to_path_buf)),
        hidden,
        sort,
    );
    #[cfg(feature = "watch")]
    let (current, parent) = (current.watch(), parent.watch());
    let find = signal(None);
    let preview_top = signal(0);
    let tree = DirTree {
        cwd,
        current,
        parent,
        find,
        preview_top,
    };
    let columns = miller_in(tree, cwd);
    let cursor = columns.cursor;
    let entries = current.entries();
    let hovered = memo(move || {
        let at = cursor.get();
        entries.with(|f| f.get(at.min(f.len().saturating_sub(1))).cloned())
    });
    Miller {
        cwd,
        current,
        parent,
        cursor,
        hovered,
        hidden,
        sort,
        linemode: signal(Linemode::None),
        find,
        preview: signal(Preview::Nothing),
        preview_top,
        columns,
        preview_cursor: signal(0),
        preview_first: signal(0),
        icons: false,
    }
}

impl Miller {
    /// With Nerd Font icons (or not).
    pub fn icons(mut self, on: bool) -> Miller {
        self.icons = on;
        self
    }

    /// The columns' shares of the width: Yazi's `ratio`, `[1, 4, 3]` by
    /// default. A share of 0 hides that column's pane down to its rail.
    pub fn ratios(self, parent: u16, current: u16, preview: u16) -> Miller {
        self.columns.ratios(parent, current, preview);
        self
    }

    /// The path of `entry` in the current directory.
    pub fn path_of(&self, entry: &Entry) -> PathBuf {
        self.cwd.get_untracked().join(&entry.name)
    }

    /// The hovered entry's path, without subscribing.
    pub fn hovered_path(&self) -> Option<PathBuf> {
        self.hovered.get_untracked().map(|e| self.path_of(&e))
    }

    /// Where the current column's rows in view are on the screen.
    pub fn row_rects(&self) -> RowRects {
        self.columns.row_rects()
    }

    /// Where the current column is on the screen.
    pub fn column_rect(&self) -> Option<Rect> {
        self.columns.column_rect()
    }

    /// The entry last hovered in `dir`, if it has been shown.
    pub fn remembered(&self, dir: &Path) -> Option<String> {
        self.columns.remembered(&dir.to_path_buf())
    }

    /// Move the cursor to `to`, kept in the listing.
    pub fn move_to(&self, to: isize) {
        self.columns.move_to(to);
    }

    /// Move the cursor by `by` entries.
    pub fn arrow(&self, by: isize) {
        self.columns.arrow(by);
    }

    /// Move by a share of the column's height (Yazi's Ctrl+D is 0.5).
    pub fn page(&self, share: f64) {
        self.columns.page(share);
    }

    /// Show `dir`, hovering `hover` there, else the entry hovered when it
    /// was last shown. The find is cleared and the preview scrolled back.
    pub fn cd(&self, dir: PathBuf, hover: Option<String>) {
        self.columns.cd(dir, hover);
    }

    /// Enter the hovered directory.
    pub fn enter(&self) {
        self.columns.enter();
    }

    /// Go up to the parent, hovering the directory left.
    pub fn leave(&self) {
        self.columns.leave();
    }

    /// Read both directories again (after changing files), keeping the
    /// hovered entry where it is if it is still there.
    pub fn refresh(&self) {
        let (current, parent) = (self.current, self.parent);
        self.columns.keep(|| {
            current.refresh();
            parent.refresh();
        });
    }

    /// Bind Yazi's movement keys on `node` (which should hold the view):
    /// the core's [`navigation`](crate::miller::Miller::navigation)
    /// (`j`/`k`, `h`/`l`, `g g`/`G`, Ctrl+D/U, PgDn/PgUp), and `J`/`K`
    /// scroll the preview. The node sees the keys before the focused list.
    pub fn navigation(self, node: Node) -> Node {
        let m = self;
        self.columns
            .navigation(node)
            .bind("J", "Scroll the preview down", move |_| {
                m.preview_top.update(|t| *t = t.saturating_add(5))
            })
            .bind("K", "Scroll the preview up", move |_| {
                m.preview_top.update(|t| *t = t.saturating_sub(5))
            })
    }

    /// The three columns, with default options.
    pub fn view(self) -> Node {
        self.view_with(MillerOptions::new())
    }

    /// The three columns: markers and git signs as `options` says.
    pub fn view_with(self, options: MillerOptions) -> Node {
        self.columns.layout(
            self.parent_column(&options),
            self.current_column(&options),
            self.preview_column(&options),
        )
    }

    fn current_column(self, options: &MillerOptions) -> Node {
        let mut list = FileListOptions::new()
            .icons(self.icons)
            .linemode(self.linemode)
            .find(self.find)
            .offset(self.columns.first)
            .row_rects(&self.columns.row_rects())
            .marker_fn(options.markers.clone());
        if let Some(git) = options.git {
            list = list.git(git);
        }
        // The list's own wheel and clicks move the cursor.
        let node = file_list(self.current, self.cursor, list)
            .name("current")
            .autofocus();
        self.columns.track_current(node)
    }

    fn parent_column(self, options: &MillerOptions) -> Node {
        let m = self;
        let markers: Option<MarkerFn> = options
            .markers
            .clone()
            .map(|f| Rc::new(move |_, path: &Path| f(None, path)) as MarkerFn);
        let list = FileListOptions::new()
            .icons(self.icons)
            .offset(self.columns.parent_first)
            .lead(true)
            .marker_fn(markers)
            .no_focus();
        file_list(self.parent, self.columns.parent_cursor, list)
            .name("parent")
            .on_mouse(move |_, mouse| {
                use rich_interact::{Button, MouseKind};
                match mouse.kind {
                    MouseKind::Down(Button::Left) => {}
                    MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                    _ => return false,
                }
                m.columns.click_parent(mouse.row as usize);
                true
            })
    }

    fn preview_column(self, options: &MillerOptions) -> Node {
        let m = self;
        let hovered = self.hovered;
        let path = move || {
            let entry = hovered.get()?;
            Some(m.cwd.get().join(entry.name))
        };
        let pane = preview(
            path,
            options
                .previewers
                .clone()
                .hidden(self.hidden)
                .sort(self.sort)
                .state(self.preview)
                .scroll(self.preview_top)
                .dir_cursor(self.preview_cursor, self.preview_first)
                .refresh(self.current.tick_signal())
                .icons(self.icons)
                .remembered(move |dir| m.remembered(dir))
                .on_dir_click(move |name, _cx: &mut Ctx| m.columns.enter_at(name)),
        );
        // Yazi pads the preview column by a cell on the right.
        row([pane.flex(1), label("").fixed(1)])
    }
}

//! Miller columns: the parent directory, the current one and a preview,
//! over one cursor: the core's generic [`Miller`](crate::miller::Miller)
//! over a directory tree, with Yazi's rows and previews.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use rich::markup::escape;
use rich_ext::git::GitStatus;
use rich_ext::listing::{read_dir, Entry, Sort};

use super::model::{dir_model_with, DirModel};
use super::preview::{preview, BinaryPreview, Preview, PreviewOptions, PreviewSettings, Previewer};
use super::row::{file_list, FileListOptions, Linemode, Marker, MarkerFn};
use super::search::Search;
use crate::app::Ctx;
use crate::list::RowRects;
use crate::miller::{miller_in, MillerSource};
use crate::node::{label, row, Node};
use crate::reactive::{memo, signal, Memo, Signal};
use crate::screen::Rect;
use crate::select::{list_search, MultiSelect};

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
    search: Signal<Option<Search>>,
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
        // Anywhere else, a search's listing ends.
        if let Some(search) = self.search.replace(None) {
            search.cancel();
            self.current.results.replace(None);
        }
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
    /// The selection [`selection`](crate::files::selection) made for it.
    pub(crate) selection: Signal<Option<MultiSelect<PathBuf>>>,
    /// The search whose results the current column lists.
    search: Signal<Option<Search>>,
}

/// What [`MillerOptions::on_context`] runs.
type OnEntryContext = Rc<std::cell::RefCell<dyn FnMut(&Entry, &mut Ctx)>>;

/// What a [`Miller::view_with`] draws besides the entries, and what it
/// gives the columns' lists.
#[derive(Clone, Default)]
pub struct MillerOptions {
    markers: Option<MarkerFn>,
    git: Option<Signal<Option<GitStatus>>>,
    previewers: PreviewOptions,
    caps: Option<(String, String)>,
    context: Option<OnEntryContext>,
    hooks: crate::miller::MillerOptions,
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

    /// A [`Previewer`] (a built-in one: [`Previewer::file_type`]), asked
    /// before the built-in ones.
    pub fn previewer_of(mut self, previewer: Previewer) -> MillerOptions {
        self.previewers = self.previewers.previewer_of(previewer);
        self
    }

    /// How the preview shows a binary file: a hex dump, or what `file`
    /// says it is (see [`PreviewOptions::binary`]).
    pub fn binary(mut self, binary: BinaryPreview) -> MillerOptions {
        self.previewers = self.previewers.binary(binary);
        self
    }

    /// Round the current column's hovered row with `open` and `close`
    /// (Yazi's `"\u{e0b6}"` and `"\u{e0b4}"` with a Nerd Font), in the
    /// row's colour: [`FileListOptions::hovered_caps`].
    pub fn hovered_caps(mut self, open: &str, close: &str) -> MillerOptions {
        self.caps = Some((open.to_string(), close.to_string()));
        self
    }

    /// Run `handler` with an entry of the current column pressed with the
    /// right button (it is hovered first), or hovered when the menu key
    /// (or Shift+F10) is pressed: a context menu on a file, opened at
    /// `cx.pointer()` (`None` from the keyboard; then below the hovered
    /// row, [`Miller::row_rects`]). The list's
    /// [`on_context`](Node::on_context).
    pub fn on_context(mut self, handler: impl FnMut(&Entry, &mut Ctx) + 'static) -> MillerOptions {
        self.context = Some(Rc::new(std::cell::RefCell::new(handler)));
        self
    }

    /// Give the current column's list to `hook`, for any list option
    /// (`.current(|list| list.wheel_step(1))`): the core's
    /// [`MillerOptions::current`](crate::miller::MillerOptions::current).
    pub fn current(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.hooks = self.hooks.current(hook);
        self
    }

    /// Give the parent column's list to `hook`.
    pub fn parent(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.hooks = self.hooks.parent(hook);
        self
    }

    /// Give the preview pane to `hook`.
    pub fn preview(mut self, hook: impl Fn(Node) -> Node + 'static) -> MillerOptions {
        self.hooks = self.hooks.preview(hook);
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
    let search = signal(None);
    let tree = DirTree {
        cwd,
        current,
        parent,
        find,
        preview_top,
        search,
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
        selection: signal(None),
        search,
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

    /// The directories shown so far, the most recent last (the current one
    /// among them), a signal: Z's choices in a session, and what a
    /// [`frecency`](crate::files::frecency) store
    /// [follows](crate::files::Frecency::follow).
    pub fn visited(&self) -> Signal<Vec<PathBuf>> {
        self.columns.visited()
    }

    /// What an action acts on (Yazi's yank, cut, delete, open): the
    /// [`selection`](crate::files::selection)'s paths (a visual range
    /// counted), else the hovered entry's; subscribing, so a status line
    /// can count them.
    pub fn targets(&self) -> Vec<PathBuf> {
        let cwd = self.cwd;
        let hovered = self.hovered.get().map(|e| cwd.with(|d| d.join(e.name)));
        match self.selection.get_untracked() {
            Some(selection) => selection.targets(hovered),
            None => hovered.into_iter().collect(),
        }
    }

    /// Move to the next entry the find query matches (smart case), after
    /// the cursor, wrapping; with `previous`, the one before (Yazi's `n`
    /// and `N`). The index it went to, or `None` with no match. The
    /// core's [`list_search`] over the current column's names.
    pub fn find_next(&self, previous: bool) -> Option<usize> {
        let (find, entries) = (self.find, self.current.entries());
        let search = list_search(
            move || find.get().unwrap_or_default(),
            move || entries.with(|e| e.iter().map(|e| escape(&e.name)).collect()),
            self.cursor,
        );
        if previous {
            search.previous()
        } else {
            search.next()
        }
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

    /// Read both directories again (after changing files). The cursor
    /// stays on the hovered entry while it is there, as it does through a
    /// re-sort, a filter or hidden files shown (it is kept by name).
    pub fn refresh(&self) {
        self.current.refresh();
        self.parent.refresh();
    }

    /// List `search`'s results in the current column, in place of its
    /// root's entries (Yazi's search listing): the columns go to the root,
    /// each result named by its path from there, and its results arrive
    /// while it runs. Entering a directory, going up or anywhere else ends
    /// it, as [`end_search`](Self::end_search) does.
    ///
    /// ```
    /// use intuituive::files::{miller, search, SearchKind};
    /// use intuituive::prelude::*;
    ///
    /// let dir = std::env::temp_dir().join(format!("intuituive-show-search-doc-{}", std::process::id()));
    /// std::fs::create_dir_all(dir.join("src")).unwrap();
    /// std::fs::write(dir.join("src/lib.rs"), "").unwrap();
    /// std::fs::write(dir.join("notes.md"), "").unwrap();
    /// let start = dir.clone();
    /// let app = App::new(move || {
    ///     let m = miller(start.clone());
    ///     let root = start.clone();
    ///     m.navigation(m.view()).on_key("s", move |_| m.show_search(search(&root, "lib", SearchKind::Name)))
    /// })
    /// .wait_for_tasks(true);
    /// let mut driver = app.driver(60, 3);
    /// driver.press("s");
    /// driver.settle();
    /// let row = driver.screen().plain()[0].replace('\\', "/");
    /// assert!(row.contains("src/lib.rs") && !row.contains("notes.md"), "{row}");
    /// # std::fs::remove_dir_all(&dir).ok();
    /// ```
    pub fn show_search(&self, search: Search) {
        self.cd(search.root(), None);
        self.search.replace(Some(search));
        self.current.results.replace(Some(search.found()));
        self.cursor.set(0);
    }

    /// The search the current column lists, if any (subscribing): for a
    /// header that says `search: lib`.
    pub fn searching(&self) -> Option<Search> {
        self.search.with(|s| *s)
    }

    /// Stop listing a search's results: the directory's own entries again.
    pub fn end_search(&self) {
        if let Some(search) = self.search.replace(None) {
            search.cancel();
            self.current.results.replace(None);
        }
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

    /// The three columns: markers, git signs, the hovered row's caps, a
    /// context handler and list options as `options` says. A
    /// [`selection`](crate::files::selection) made for these columns is
    /// marked in them.
    pub fn view_with(self, options: MillerOptions) -> Node {
        let hooks = &options.hooks;
        self.columns.layout(
            hooks.wrap_parent(self.parent_column(&options)),
            hooks.wrap_current(self.current_column(&options)),
            hooks.wrap_preview(self.preview_column(&options)),
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
        if let Some((open, close)) = &options.caps {
            list = list.hovered_caps(open, close);
        }
        let (cwd, entries) = (self.cwd, self.current.entries());
        // An entry's key: its path, so a cursor or a mark never follows a
        // name into another directory.
        let path_at = move |i: usize| {
            let name = entries.with(|e| e.get(i).map(|e| e.name.clone()));
            cwd.with(|dir| name.map(|n| dir.join(n)).unwrap_or_default())
        };
        // The list's own wheel and clicks move the cursor; a re-sort, a
        // filter or a new listing keeps it on its entry.
        let mut node = file_list(self.current, self.cursor, list)
            .cursor_by(path_at)
            .name("current")
            .autofocus();
        if let Some(selection) = self.selection.get_untracked() {
            node = node.selection_markers(selection, path_at);
        }
        if let Some(context) = options.context.clone() {
            node = node.on_context(move |i, cx| {
                if let Some(entry) = entries.with_untracked(|e| e.get(i).cloned()) {
                    (context.borrow_mut())(&entry, cx);
                }
            });
        }
        self.columns.track_current(node)
    }

    fn parent_column(self, options: &MillerOptions) -> Node {
        let m = self;
        let selection = self.selection.get_untracked();
        let markers: Option<MarkerFn> = match (options.markers.clone(), selection) {
            (None, None) => None,
            (marker, selection) => Some(Rc::new(move |_, path: &Path| {
                // The selection's mark first, as the current column draws
                // it (the parent has no visual range).
                let key = path.to_path_buf();
                if selection.is_some_and(|s| s.mark(&key) == crate::select::Mark::Selected) {
                    return Marker::Selected;
                }
                marker.as_ref().map_or(Marker::None, |f| f(None, path))
            })),
        };
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

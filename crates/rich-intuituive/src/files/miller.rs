//! Miller columns: the parent directory, the current one and a preview,
//! over one cursor.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rich_ext::git::GitStatus;
use rich_ext::listing::{Entry, Sort};

use super::model::{dir_model_with, DirModel};
use super::preview::{preview, Preview, PreviewOptions, PreviewSettings};
use super::row::{file_list, FileListOptions, Linemode, Marker, MarkerFn};
use crate::app::Ctx;
use crate::list::RowRects;
use crate::node::{label, row, Node};
use crate::place::RectHandle;
use crate::reactive::{memo, signal, watch, Memo, Signal};
use crate::screen::Rect;
use crate::split::hsplit;

/// Miller columns over a directory tree, as Yazi lays them out: the
/// parent directory (the current one hovered in it), the current
/// directory (the cursor's column) and a preview of the hovered entry,
/// side by side in panes whose rails the mouse drags. All of it is
/// signals, and the struct is `Copy`, so any handler takes one. Made by
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
    parent_cursor: Signal<usize>,
    preview_cursor: Signal<usize>,
    first: Signal<usize>,
    parent_first: Signal<usize>,
    preview_first: Signal<usize>,
    /// The entry last hovered in each directory visited.
    remember: Signal<HashMap<PathBuf, String>>,
    rows: Signal<RowRects>,
    column: Signal<Option<RectHandle>>,
    /// The panes' shares of the width, which the rails drag.
    outer: Signal<f64>,
    inner: Signal<f64>,
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
    let cursor = signal(0usize);
    let entries = current.entries();
    let hovered = memo(move || {
        let at = cursor.get();
        entries.with(|f| f.get(at.min(f.len().saturating_sub(1))).cloned())
    });
    let miller = Miller {
        cwd,
        current,
        parent,
        cursor,
        hovered,
        hidden,
        sort,
        linemode: signal(Linemode::None),
        find: signal(None),
        preview: signal(Preview::Nothing),
        preview_top: signal(0),
        parent_cursor: signal(0),
        preview_cursor: signal(0),
        first: signal(0),
        parent_first: signal(0),
        preview_first: signal(0),
        remember: signal(HashMap::new()),
        rows: signal(RowRects::new()),
        column: signal(None),
        outer: signal(1.0 / 8.0),
        inner: signal(4.0 / 7.0),
        icons: false,
    };
    // Remember the entry hovered in each directory, for coming back.
    watch(
        move || (cwd.get(), hovered.get().map(|e| e.name)),
        move |(dir, name), _| {
            if let Some(name) = name {
                miller.remember.update(|r| {
                    r.insert(dir, name);
                });
            }
        },
    );
    // The cursor stays on the listing when it shrinks.
    watch(
        move || entries.with(Vec::len),
        move |len, _| {
            if cursor.get_untracked() >= len {
                cursor.set(len.saturating_sub(1));
            }
        },
    );
    // The parent column hovers the current directory.
    let parent_entries = parent.entries();
    watch(
        move || {
            let name = cwd.with(|d| d.file_name().map(|n| n.to_string_lossy().into_owned()));
            (name, parent_entries.with(Vec::len))
        },
        move |(name, _), _| {
            let at = name.and_then(|n| parent.position(&n)).unwrap_or(0);
            miller.parent_cursor.set(at);
        },
    );
    miller
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
        let total = f64::from(parent + current + preview).max(1.0);
        self.outer.set(f64::from(parent) / total);
        let rest = f64::from(current + preview).max(1.0);
        self.inner.set(f64::from(current) / rest);
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
        self.rows.get_untracked()
    }

    /// Where the current column is on the screen.
    pub fn column_rect(&self) -> Option<Rect> {
        self.column.get_untracked().and_then(|c| c.get_untracked())
    }

    /// The entry last hovered in `dir`, if it has been shown.
    pub fn remembered(&self, dir: &Path) -> Option<String> {
        self.remember.with_untracked(|r| r.get(dir).cloned())
    }

    /// Move the cursor to `to`, kept in the listing.
    pub fn move_to(&self, to: isize) {
        let len = self.current.entries().with_untracked(Vec::len);
        let to = to.clamp(0, len.saturating_sub(1) as isize) as usize;
        self.cursor.set(to);
    }

    /// Move the cursor by `by` entries.
    pub fn arrow(&self, by: isize) {
        self.move_to(self.cursor.get_untracked() as isize + by);
    }

    /// Move by a share of the column's height (Yazi's Ctrl+D is 0.5).
    pub fn page(&self, share: f64) {
        let rows = self.column_rect().map_or(1, |r| r.height.max(1)) as f64;
        let by = (rows * share).round() as isize;
        self.arrow(if by == 0 { share.signum() as isize } else { by });
    }

    /// Show `dir`, hovering `hover` there, else the entry hovered when it
    /// was last shown. The find is cleared and the preview scrolled back.
    pub fn cd(&self, dir: PathBuf, hover: Option<String>) {
        self.cwd.set(dir);
        self.arrive(hover);
    }

    /// After the directory changed: a fresh view of it.
    fn arrive(&self, hover: Option<String>) {
        self.find.set(None);
        self.current.filter.set(String::new());
        self.preview_top.set(0);
        let dir = self.cwd.get_untracked();
        let hover = hover.or_else(|| self.remembered(&dir));
        let at = hover.and_then(|n| self.current.position(&n)).unwrap_or(0);
        self.cursor.set(at);
    }

    /// Enter the hovered directory.
    pub fn enter(&self) {
        if let Some(entry) = self.hovered.get_untracked().filter(|e| e.dir) {
            self.cd(self.path_of(&entry), None);
        }
    }

    /// Go up to the parent, hovering the directory left.
    pub fn leave(&self) {
        let dir = self.cwd.get_untracked();
        if let Some(up) = dir.parent() {
            let came_from = dir.file_name().map(|n| n.to_string_lossy().into_owned());
            self.cd(up.to_path_buf(), came_from);
        }
    }

    /// Read both directories again (after changing files), keeping the
    /// hovered entry where it is if it is still there.
    pub fn refresh(&self) {
        let name = self.hovered.get_untracked().map(|e| e.name);
        self.current.refresh();
        self.parent.refresh();
        if let Some(at) = name.and_then(|n| self.current.position(&n)) {
            self.cursor.set(at);
        }
    }

    /// Bind Yazi's movement keys on `node` (which should hold the view):
    /// `j`/`k` (and the arrows) move, `h`/`l` (and ←/→) leave and enter,
    /// `g g`/`G` (and Home/End) go to the top and the bottom, Ctrl+D/U
    /// and PgDn/PgUp move by half and whole columns, `J`/`K` scroll the
    /// preview. The node sees the keys before the focused list.
    pub fn navigation(self, node: Node) -> Node {
        let m = self;
        node.previews_keys()
            .bind("k up", "Move up", move |_| m.arrow(-1))
            .bind("j down", "Move down", move |_| m.arrow(1))
            .bind("h left", "Go up a directory", move |_| m.leave())
            .bind("l right", "Enter the directory", move |_| m.enter())
            .bind_sequence("g g", "Go to the top", move |_| m.move_to(0))
            .bind("home", "Go to the top", move |_| m.move_to(0))
            .bind("G end", "Go to the bottom", move |_| m.move_to(isize::MAX))
            .bind("ctrl+d", "Half a page down", move |_| m.page(0.5))
            .bind("ctrl+u", "Half a page up", move |_| m.page(-0.5))
            .bind("pagedown ctrl+f", "A page down", move |_| m.page(1.0))
            .bind("pageup ctrl+b", "A page up", move |_| m.page(-1.0))
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
        hsplit(
            self.parent_column(&options),
            hsplit(
                self.current_column(&options),
                self.preview_column(&options),
                self.inner,
            ),
            self.outer,
        )
    }

    fn current_column(self, options: &MillerOptions) -> Node {
        let mut list = FileListOptions::new()
            .icons(self.icons)
            .linemode(self.linemode)
            .find(self.find)
            .offset(self.first)
            .row_rects(&self.rows.get_untracked())
            .marker_fn(options.markers.clone());
        if let Some(git) = options.git {
            list = list.git(git);
        }
        // The list's own wheel and clicks move the cursor.
        let node = file_list(self.current, self.cursor, list)
            .name("current")
            .autofocus();
        let handle = node.rect_handle();
        // `update`, not `set`: a handle is not `PartialEq`.
        self.column.update(|c| *c = Some(handle));
        node
    }

    fn parent_column(self, options: &MillerOptions) -> Node {
        let m = self;
        let markers: Option<MarkerFn> = options
            .markers
            .clone()
            .map(|f| Rc::new(move |_, path: &Path| f(None, path)) as MarkerFn);
        let list = FileListOptions::new()
            .icons(self.icons)
            .offset(self.parent_first)
            .lead(true)
            .marker_fn(markers)
            .no_focus();
        file_list(self.parent, self.parent_cursor, list)
            .name("parent")
            .on_mouse(move |_, mouse| {
                use rich_interact::{Button, MouseKind};
                match mouse.kind {
                    MouseKind::Down(Button::Left) => {}
                    MouseKind::ScrollUp | MouseKind::ScrollDown => return true,
                    _ => return false,
                }
                let i = m.parent_first.get_untracked() + mouse.row as usize;
                let dir = m.cwd.get_untracked();
                let Some(up) = dir.parent() else {
                    return true;
                };
                match m.parent.entries().with_untracked(|e| e.get(i).cloned()) {
                    Some(entry) if entry.dir => m.cd(up.join(&entry.name), None),
                    Some(entry) => m.cd(up.to_path_buf(), Some(entry.name)),
                    None => m.leave(),
                }
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
                .on_dir_click(move |name, _cx: &mut Ctx| {
                    if let Some(dir) = m
                        .hovered_path()
                        .filter(|_| m.hovered.get_untracked().is_some_and(|e| e.dir))
                    {
                        m.cd(dir, name);
                    }
                }),
        );
        // Yazi pads the preview column by a cell on the right.
        row([pane.flex(1), label("").fixed(1)])
    }
}

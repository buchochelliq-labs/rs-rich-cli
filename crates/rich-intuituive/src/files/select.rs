//! What a file manager acts on: a [`selection`] over Miller's current
//! column, and files yanked to paste ([`Yank`]).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rich_ext::file_ops::{Conflict, LinkKind, Op};

use super::miller::Miller;
use super::row::Marker;
use crate::reactive::{watch, Signal};
use crate::select::{multi_select, MultiSelect};

/// A selection of files for `miller`'s columns, by path: the core's
/// [`multi_select`] over the current column's entries, its visual range
/// following the cursor (Yazi's Space, `v`, `V`, Ctrl+A, Ctrl+R). The
/// columns mark it ([`Miller::view_with`], made after this; the parent
/// column too, by path) and act on it ([`Miller::targets`]): selected
/// files stay selected in other directories. A visual range is over the
/// current directory's rows, so it is dropped when the directory changes;
/// end it first ([`end_visual`](MultiSelect::end_visual)) to keep it.
/// With [`tabs`](crate::files::tabs) over the same columns (made before
/// or after this), it is one selection per tab, as Yazi's: the handle
/// holds the active tab's, and each tab keeps its own visual mode
/// ([`MultiSelect::scoped`]). Call it while the app is built.
///
/// ```
/// use intuituive::files::{miller, selection};
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-selection-doc-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// for name in ["a.txt", "b.txt", "c.txt"] {
///     std::fs::write(dir.join(name), "").unwrap();
/// }
/// let start = dir.clone();
/// let app = App::new(move || {
///     let m = miller(start.clone());
///     let picked = selection(m);
///     let names = move || {
///         m.targets().iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>().join(" ")
///     };
///     column([m.navigation(m.view()).flex(1), text(names).fixed(1)])
///         .on_key("v", move |_| picked.visual(false))
/// });
/// let mut driver = app.driver(60, 4);
/// driver.press("v");
/// driver.press("j");
/// assert_eq!(driver.screen().plain()[3].trim_end(), "a.txt b.txt");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn selection(miller: Miller) -> MultiSelect<PathBuf> {
    let (cwd, entries) = (miller.cwd, miller.current.entries());
    let selection = multi_select::<PathBuf>()
        .over(move || {
            let dir = cwd.get();
            entries.with(|e| e.iter().map(|e| dir.join(&e.name)).collect())
        })
        .range_from(miller.cursor);
    // A visual range ends when its tab's directory changes: with tabs,
    // the active tab's own, so switching tabs keeps each tab's mode.
    let tabs = miller.tabs;
    let place = move || match tabs.get() {
        Some(set) => (Some(set.active_id()), set.with_active(|t| t.cwd.get())),
        None => (None, cwd.get()),
    };
    let last = Rc::new(Cell::new(crate::reactive::untracked(|| place().0)));
    watch(place, move |(tab, _), _| {
        if last.replace(tab) == tab {
            selection.cancel_visual();
        }
    });
    if let Some(set) = tabs.get_untracked() {
        let _ = selection.scoped(&set);
    }
    miller.selection.replace(Some(selection));
    selection
}

/// Files yanked to paste: copied (Yazi's `y`) or cut (`x`). Keep one in a
/// signal (`Signal<Option<Yank>>`); [`Yank::markers`] marks its files in
/// Miller's columns and [`paste`](Self::paste) makes the operation that
/// pastes them, for [`Tasks::run`](crate::files::Tasks::run).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Yank {
    pub paths: Vec<PathBuf>,
    /// Moved, not copied, when pasted.
    pub cut: bool,
}

impl Yank {
    /// `paths` to copy.
    pub fn copy(paths: Vec<PathBuf>) -> Yank {
        Yank { paths, cut: false }
    }

    /// `paths` to move.
    pub fn cut(paths: Vec<PathBuf>) -> Yank {
        Yank { paths, cut: true }
    }

    /// The mark of `path`: [`Marker::Copied`] or [`Marker::Cut`] if it
    /// is yanked, else none.
    pub fn marker(&self, path: &Path) -> Marker {
        match self.paths.iter().any(|p| p == path) {
            true if self.cut => Marker::Cut,
            true => Marker::Copied,
            false => Marker::None,
        }
    }

    /// A marker function for
    /// [`MillerOptions::markers`](crate::files::MillerOptions::markers):
    /// each file's mark from what `yanked` holds (green copied, red cut),
    /// read as the rows draw.
    ///
    /// ```
    /// use intuituive::files::{Marker, Yank};
    /// use intuituive::prelude::*;
    /// use std::path::Path;
    ///
    /// let app = App::new(|| {
    ///     let yanked = signal(Some(Yank::cut(vec!["/tmp/a".into()])));
    ///     let marker = Yank::markers(yanked);
    ///     label(format!("{:?} {:?}", marker(Some(0), Path::new("/tmp/a")), marker(None, Path::new("/tmp/b"))))
    /// });
    /// assert_eq!(app.render_with(&[], 12, 1).unwrap()[0].trim_end(), "Cut None");
    /// ```
    pub fn markers(
        yanked: Signal<Option<Yank>>,
    ) -> impl Fn(Option<usize>, &Path) -> Marker + 'static {
        move |_, path| yanked.with(|y| y.as_ref().map_or(Marker::None, |y| y.marker(path)))
    }

    /// The operation that pastes the files into the directory `into`:
    /// copies them, or moves them if they were cut, a name taken
    /// meanwhile treated as `conflict` says ([`Conflict::Rename`]: Yazi's
    /// `p`; [`Conflict::Overwrite`]: `P`).
    pub fn paste(&self, into: impl Into<PathBuf>, conflict: Conflict) -> Op {
        let (from, into) = (self.paths.clone(), into.into());
        if self.cut {
            Op::Move {
                from,
                into,
                conflict,
            }
        } else {
            Op::Copy {
                from,
                into,
                conflict,
            }
        }
    }

    /// The operation that links the files into the directory `into`, cut
    /// or copied alike, as `kind` says: Yazi's `-` (a symbolic link to each
    /// absolute path, `LinkKind::Symbolic { relative: false }`), `_` (to
    /// each path from `into`, `relative: true`) and Ctrl+- (hard links,
    /// [`LinkKind::Hard`]). A name taken gets a free one (`_1`), as a paste
    /// does. The files stay yanked.
    ///
    /// ```
    /// use intuituive::files::Yank;
    /// use rich_ext::file_ops::{LinkKind, Op};
    ///
    /// let yank = Yank::cut(vec!["/home/a/notes.md".into()]);
    /// let op = yank.link("/home/a/links", LinkKind::Symbolic { relative: true });
    /// assert!(matches!(op, Op::Link { kind: LinkKind::Symbolic { relative: true }, .. }));
    /// assert_eq!(op.title(), "Link notes.md in links");
    /// ```
    pub fn link(&self, into: impl Into<PathBuf>, kind: LinkKind) -> Op {
        Op::Link {
            from: self.paths.clone(),
            into: into.into(),
            kind,
            conflict: Conflict::Rename,
        }
    }
}

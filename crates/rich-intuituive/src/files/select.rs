//! What a file manager acts on: a [`selection`] over Miller's current
//! column, and files yanked to paste ([`Yank`]).

use std::path::{Path, PathBuf};

use rich_ext::file_ops::{Conflict, Op};

use super::miller::Miller;
use super::row::Marker;
use crate::reactive::{watch, Signal};
use crate::select::{multi_select, MultiSelect};

/// A selection of files for `miller`'s columns, by path: the core's
/// [`multi_select`] over the current column's entries, its visual range
/// following the cursor (Yazi's Space, `v`, `V`, Ctrl+A, Ctrl+R). The
/// columns mark it ([`Miller::view_with`], made after this) and act on it
/// ([`Miller::targets`]): selected files stay selected in other
/// directories. A visual range is over the current directory's rows, so
/// it is dropped when the directory changes; end it first
/// ([`end_visual`](MultiSelect::end_visual)) to keep it. Call it while the
/// app is built.
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
    watch(move || cwd.get(), move |_, _| selection.cancel_visual());
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
}

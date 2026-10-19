//! A fuzzy finder over a directory walk, whose results stream in: the
//! core's generic [`finder_of`](crate::finder_of) over rs-rich-ext's walk.

use std::path::PathBuf;

use rich_ext::listing::Walk;

use crate::app::Ctx;
use crate::finder::finder_of;
use crate::node::Node;

/// The most paths a finder collects from its walk.
const LIMIT: usize = 100_000;

/// The state of a [`finder`]: the core's [`Finder`](crate::finder::Finder)
/// over paths relative to the walk's root.
pub type Finder = crate::finder::Finder<String>;

/// A fuzzy finder over the paths `walk` finds (rs-rich-ext's
/// [`walk`](rich_ext::listing::walk): `walk(root).hidden(false)`), as
/// Yazi's `z` is over fd and fzf: the core's generic
/// [`finder_of`](crate::finder_of) over the walk, each path shown relative
/// to the root, at most 100 000 of them. The walk runs on a thread of its
/// own and its paths reach the list in batches while it goes on, so the
/// first results show at once and typing never waits for a big tree; the
/// walk stops when the finder's screen closes. Up and Down (Ctrl+P, Ctrl+N) move,
/// Enter calls `on_pick` with the hovered path (the root joined to it).
///
/// ```
/// use intuituive::files::finder;
/// use intuituive::prelude::*;
/// use rich_ext::listing::walk;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-finder-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("src")).unwrap();
/// std::fs::write(dir.join("src/main.rs"), "").unwrap();
/// std::fs::write(dir.join("README.md"), "").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     let root = root.clone();
///     let picked = signal(String::new());
///     column([
///         finder(walk(&root), move |path, _| {
///             picked.set(path.strip_prefix(&root).unwrap().display().to_string())
///         })
///         .flex(1),
///         text!("{picked}").fixed(1),
///     ])
///     .on_key("ctrl+q", |cx| cx.quit())
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(40, 6);
/// driver.type_text("mrs");
/// driver.press("enter");
/// let shown = driver.screen().plain()[5].trim_end().replace('\\', "/");
/// assert_eq!(shown, "src/main.rs");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn finder(walk: Walk, on_pick: impl Fn(PathBuf, &mut Ctx) + 'static) -> Node {
    let root = walk.root().to_path_buf();
    let base = root.clone();
    let paths = walk.take(LIMIT).map(move |path| {
        let rel = path.strip_prefix(&base).unwrap_or(&path);
        rel.to_string_lossy().into_owned()
    });
    finder_of(paths, String::clone, move |rel, cx| {
        on_pick(root.join(rel), cx)
    })
}

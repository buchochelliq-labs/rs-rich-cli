//! A fuzzy finder over a directory walk, whose results stream in: the
//! core's generic [`fuzzy_list`](crate::finder::fuzzy_list) over rs-rich-ext's walk.

use std::path::PathBuf;

use rich_ext::listing::Walk;

use crate::app::Ctx;
use crate::finder::{fuzzy_list_with, FuzzyOptions, FuzzySource};
use crate::node::Node;

/// The most paths a finder collects from its walk.
const LIMIT: usize = 100_000;

/// The state of a [`finder`]: the core's [`Finder`](crate::finder::Finder)
/// over paths relative to the walk's root.
pub type Finder = crate::finder::Finder<String>;

/// A fuzzy finder over the paths `walk` finds (rs-rich-ext's
/// [`walk`](rich_ext::listing::walk): `walk(root).hidden(false)`), as
/// Yazi's `z` is over fd and fzf: the core's generic
/// [`fuzzy_list`](crate::finder::fuzzy_list) over the walk, each path shown relative
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
    finder_with(walk, on_pick, FuzzyOptions::new())
}

/// A [`finder`] with a title, a border, a prompt or an Esc of its own
/// (the core's [`FuzzyOptions`]), so it looks as any other
/// [`fuzzy_list`](crate::finder::fuzzy_list) does: Yazi's `z` in a modal, titled, Esc closing it (or
/// running [`on_cancel`](FuzzyOptions::on_cancel)).
///
/// ```
/// use intuituive::files::finder_with;
/// use intuituive::finder::FuzzyOptions;
/// use intuituive::prelude::*;
/// use rich_ext::listing::walk;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-finder-with-doc-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// std::fs::write(dir.join("notes.md"), "").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     let root = root.clone();
///     label("files").on_key("z", move |cx| {
///         let root = root.clone();
///         cx.modal(Size::Fixed(30), Size::Fixed(6), move || {
///             let options = FuzzyOptions::new().title("Find").border(Border::Rounded);
///             finder_with(walk(&root), |_, _| {}, options)
///         });
///     })
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(40, 8);
/// driver.press("z");
/// assert!(driver.screen().plain().join("\n").contains("─ Find "));
/// driver.press("esc");
/// assert!(!driver.screen().plain().join("\n").contains("─ Find "));
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn finder_with(
    walk: Walk,
    on_pick: impl Fn(PathBuf, &mut Ctx) + 'static,
    options: FuzzyOptions,
) -> Node {
    let root = walk.root().to_path_buf();
    let base = root.clone();
    let paths = walk.take(LIMIT).map(move |path| {
        let rel = path.strip_prefix(&base).unwrap_or(&path);
        rel.to_string_lossy().into_owned()
    });
    fuzzy_list_with(
        FuzzySource::iter(paths),
        String::clone,
        move |rel, cx| on_pick(root.join(rel), cx),
        options,
    )
}

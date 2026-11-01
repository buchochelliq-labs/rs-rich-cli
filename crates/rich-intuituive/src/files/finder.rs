//! A fuzzy finder over a directory walk, whose results stream in: the
//! core's generic [`fuzzy_list`](crate::finder::fuzzy_list) over rs-rich-ext's walk.

use std::cell::RefCell;
use std::path::PathBuf;

use rich_ext::listing::Walk;
use rich_interact::fuzzy::fuzzy;

use crate::app::Ctx;
use crate::complete::Completion;
use crate::finder::{FuzzyOptions, FuzzySource, fuzzy_list_with};
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

/// The most paths a [`path_completer`] offers at once.
const OFFERED: usize = 50;

/// A source for [`Node::complete_at`](crate::Node::complete_at) over the
/// paths `walk` finds, relative to its root with `/` between their parts
/// (a directory's ending in `/`): `@` file mentions in a prompt. The
/// paths that fuzzy-match the query (as the [`finder`]'s do), best first
/// and shorter first among equals, fifty at most. The walk runs once, the
/// first time a query comes, and stops at 100 000 paths; leave the big
/// directories out with [`Walk::skip_dir`].
///
/// ```
/// use intuituive::files::path_completer;
/// use intuituive::prelude::*;
/// use rich_ext::listing::walk;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-mention-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("src")).unwrap();
/// std::fs::write(dir.join("src/main.rs"), "").unwrap();
/// std::fs::write(dir.join("README.md"), "").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     let prompt = signal(String::new());
///     text_area(prompt).complete_at('@', path_completer(walk(&root)))
/// });
/// let mut driver = app.driver(30, 4);
/// driver.type_text("see @src/ma");
/// driver.press("enter");
/// driver.type_text("now");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "see @src/main.rs now");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn path_completer(walk: Walk) -> impl Fn(&str) -> Vec<Completion> + 'static {
    let pending = RefCell::new(Some(walk));
    let paths: RefCell<Vec<String>> = RefCell::new(Vec::new());
    move |query| {
        if let Some(walk) = pending.borrow_mut().take() {
            let root = walk.root().to_path_buf();
            let found = walk.take(LIMIT).filter_map(|path| {
                let rel = path.strip_prefix(&root).ok()?;
                let mut shown = rel.to_string_lossy().replace('\\', "/");
                if path.is_dir() {
                    shown.push('/');
                }
                Some(shown)
            });
            *paths.borrow_mut() = found.collect();
        }
        let paths = paths.borrow();
        let mut hits: Vec<(i64, &String)> = paths
            .iter()
            .filter_map(|path| fuzzy(query, path).map(|m| (m.score, path)))
            .collect();
        hits.sort_by(|a, b| {
            (b.0.cmp(&a.0))
                .then(a.1.len().cmp(&b.1.len()))
                .then(a.1.cmp(b.1))
        });
        hits.into_iter()
            .take(OFFERED)
            .map(|(_, path)| Completion::new(0..query.len(), path.clone()))
            .collect()
    }
}

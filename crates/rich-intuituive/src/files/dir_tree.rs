//! A directory as a tree whose folders load as they open: Textual's
//! `DirectoryTree`, on the core's [`tree_lazy`](crate::widgets::tree_lazy).

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use crate::app::Ctx;
use crate::node::Node;
use crate::reactive::{Signal, signal};
use crate::widgets::{LazyItem, tree_lazy};

type Filter = Arc<dyn Fn(&Path) -> bool + Send + Sync>;
type OnOpen = Rc<dyn Fn(PathBuf, &mut Ctx)>;

/// A [`dir_tree`] being set up: [`filter`](Self::filter) and
/// [`on_open`](Self::on_open), then [`view`](Self::view).
pub struct DirTreeView {
    root: PathBuf,
    filter: Option<Filter>,
    on_open: Option<OnOpen>,
    selected: Signal<Option<String>>,
}

/// A tree of the directory `root`, as Textual's `DirectoryTree`: `root`
/// is its one item, and a folder's entries are read when it is first
/// opened, on a worker thread (`loading…` until then), folders first, then
/// by name. Enter on a file runs [`on_open`](DirTreeView::on_open).
///
/// ```
/// use intuituive::files::dir_tree;
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-dir-tree-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("src")).unwrap();
/// std::fs::write(dir.join(".hidden"), "").unwrap();
/// let root = dir.clone();
/// let app = App::new(move || {
///     dir_tree(root.clone())
///         .filter(|path| !path.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
///         .view()
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(40, 3);
/// driver.press("right");
/// assert!(driver.screen().plain()[1].contains("src/"));
/// assert!(!driver.screen().plain().join("\n").contains(".hidden"));
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn dir_tree(root: impl Into<PathBuf>) -> DirTreeView {
    DirTreeView {
        root: root.into(),
        filter: None,
        on_open: None,
        selected: signal(None),
    }
}

impl DirTreeView {
    /// Show only the paths `keep` keeps (it runs on the worker thread that
    /// reads a folder): `|path| !is_hidden(path)`, Textual's
    /// `filter_paths`.
    #[must_use]
    pub fn filter(mut self, keep: impl Fn(&Path) -> bool + Send + Sync + 'static) -> DirTreeView {
        self.filter = Some(Arc::new(keep));
        self
    }

    /// Run `open` with a file's path when Enter is pressed on it.
    #[must_use]
    pub fn on_open(mut self, open: impl Fn(PathBuf, &mut Ctx) + 'static) -> DirTreeView {
        self.on_open = Some(Rc::new(open));
        self
    }

    /// The selected item's path, as the tree keeps it.
    pub fn selected(&self) -> Signal<Option<String>> {
        self.selected
    }

    /// The tree as a node.
    pub fn view(self) -> Node {
        let DirTreeView {
            root,
            filter,
            on_open,
            selected,
        } = self;
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let key = root.to_string_lossy().into_owned();
        let roots = move || vec![LazyItem::branch(key.clone(), format!("{name}/"))];
        let children = move |key: String| read_folder(Path::new(&key), filter.as_deref());
        let tree = tree_lazy(roots, children, selected);
        match on_open {
            None => tree,
            Some(open) => tree.on_key("enter", move |cx| {
                let Some(path) = selected.get_untracked().map(PathBuf::from) else {
                    return cx.pass_key();
                };
                if path.is_dir() {
                    return cx.pass_key();
                }
                open(path, cx);
            }),
        }
    }
}

/// A folder's entries as tree items: folders first, then by name, those
/// `keep` keeps.
fn read_folder(
    dir: &Path,
    keep: Option<&(dyn Fn(&Path) -> bool + Send + Sync)>,
) -> Result<Vec<LazyItem>, std::io::Error> {
    let mut items: Vec<(bool, String, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            let is_dir = path.is_dir();
            (
                is_dir,
                entry.file_name().to_string_lossy().into_owned(),
                path,
            )
        })
        .filter(|(_, _, path)| keep.is_none_or(|keep| keep(path)))
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(items
        .into_iter()
        .map(|(is_dir, name, path)| {
            let key = path.to_string_lossy().into_owned();
            if is_dir {
                LazyItem::branch(key, format!("{name}/"))
            } else {
                LazyItem::leaf(key, name)
            }
        })
        .collect())
}

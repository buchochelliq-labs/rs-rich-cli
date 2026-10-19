//! A file picker: Miller's columns in a modal, as Bubbles' `filepicker`
//! and Textual's `DirectoryTree` ask for a file.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use rich::markup::escape;

use super::crumb::path_crumb;
use super::miller::miller;
use crate::app::Ctx;
use crate::layout::Size;
use crate::node::{column, label, Node};
use crate::notify::Level;

/// What [`pick_file`] runs with the file picked.
type Picked = Rc<RefCell<Option<Box<dyn FnOnce(PathBuf, &mut Ctx)>>>>;

/// Ask for a file: Miller's columns ([`miller`]) starting in `start`, in a
/// modal titled `Pick a file`, the path of the directory shown above them.
/// The usual moves browse (`j`/`k`, `h`/`l`, `g g`, `G`, Ctrl+D/U); Enter
/// on a directory enters it, and on a file `filter` takes (`|_| true` for
/// any) closes the picker and gives `picked` its path. A file `filter`
/// refuses stays, with a warning. Esc closes it with nothing picked. Call
/// it from a handler.
///
/// ```
/// use intuituive::files::pick_file;
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-pick-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("docs")).unwrap();
/// std::fs::write(dir.join("docs/notes.md"), "hello").unwrap();
/// std::fs::write(dir.join("photo.png"), "").unwrap();
/// let start = dir.clone();
/// let app = App::new(move || {
///     let chosen = signal(String::new());
///     let start = start.clone();
///     text!("picked {chosen}").on_key("o", move |cx| {
///         let is_markdown = |path: &std::path::Path| path.extension().is_some_and(|e| e == "md");
///         pick_file(cx, start.clone(), is_markdown, move |path, _| {
///             chosen.set(path.file_name().unwrap().to_string_lossy().into_owned())
///         });
///     })
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(60, 12);
/// driver.press("o");
/// driver.settle();
/// // `docs` is hovered: Enter goes in, and picks notes.md there.
/// driver.press("enter");
/// driver.settle();
/// driver.press("enter");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "picked notes.md");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn pick_file(
    cx: &mut Ctx,
    start: impl Into<PathBuf>,
    filter: impl Fn(&Path) -> bool + 'static,
    picked: impl FnOnce(PathBuf, &mut Ctx) + 'static,
) {
    let start = start.into();
    let filter = Rc::new(filter);
    let picked: Picked = Rc::new(RefCell::new(Some(Box::new(picked))));
    cx.modal(Size::Percent(80), Size::Percent(70), move || {
        let m = miller(start.clone());
        let (filter, picked) = (filter.clone(), picked.clone());
        let cwd = m.cwd;
        let header = path_crumb(move || cwd.get()).fixed(1);
        let body: Node = m.navigation(m.view()).flex(1);
        column([header, body, hint().fixed(1)])
            .panel("Pick a file")
            .previews_keys()
            .bind("enter", "Pick", move |cx| {
                let Some(entry) = m.hovered.get_untracked() else {
                    return;
                };
                if entry.dir {
                    m.enter();
                    return;
                }
                let path = m.path_of(&entry);
                if !filter(&path) {
                    let name = escape(&entry.name);
                    cx.notify(
                        Level::Warn,
                        "Pick a file",
                        format!("{name} cannot be picked here"),
                    );
                    return;
                }
                cx.pop();
                let done = picked.borrow_mut().take();
                if let Some(done) = done {
                    done(path, cx);
                }
            })
            .bind("escape", "Cancel", |cx| cx.pop())
    });
}

/// The keys, at the picker's foot.
fn hint() -> Node {
    label("[bright_black]enter[/] pick  [bright_black]h/l[/] up/into  [bright_black]esc[/] cancel")
}

//! Tabs over Miller's columns, each with its own directory and history:
//! the core's [`tab_set`] and [`history`] with a file system plugged in.

use std::path::PathBuf;

use super::miller::Miller;
use crate::node::Node;
use crate::reactive::{signal, watch, Signal};
use crate::select::MultiSelect;
use crate::signals::{history, History};
use crate::tab_set::{tab_set, tabs_for, TabSet};

/// A tab of [`FileTabs`]: its directory and where it has been.
#[derive(Clone, Copy)]
pub struct FileTab {
    /// The tab's directory: the columns' while it is active.
    pub cwd: Signal<PathBuf>,
    /// Back and forward through the directories it has shown (Yazi's `H`
    /// and `L`).
    pub history: History<PathBuf>,
    /// A name given to it (Yazi's tab rename), shown instead of the
    /// directory's.
    pub name: Signal<Option<String>>,
}

impl FileTab {
    fn at(dir: PathBuf) -> FileTab {
        let cwd = signal(dir);
        FileTab {
            cwd,
            history: history(cwd),
            name: signal(None),
        }
    }

    /// What a tab strip calls it: its name, else its directory's (`/` at
    /// the root), subscribing.
    pub fn title(&self) -> String {
        self.name.get().unwrap_or_else(|| {
            self.cwd.with(|dir| {
                dir.file_name()
                    .map_or("/".into(), |n| n.to_string_lossy().into_owned())
            })
        })
    }
}

/// Tabs over one set of [`Miller`] columns, as Yazi's: each tab keeps its
/// directory, a back/forward history and, with a
/// [`selection`](crate::files::selection) over the columns, its own
/// selection and visual mode; the columns show the active tab's. Wherever the columns go (keys, clicks, a finder), the active tab
/// follows; switching tabs, closing one or going back moves the columns.
/// `Copy`; made by [`tabs`].
#[derive(Clone, Copy)]
pub struct FileTabs {
    set: TabSet<FileTab>,
    miller: Miller,
}

/// [`FileTabs`] over `miller`: one tab, at its directory. Call it while
/// the app is built.
///
/// ```
/// use intuituive::files::{miller, tabs};
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-tabs-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("docs")).unwrap();
/// let start = dir.clone();
/// let app = App::new(move || {
///     let m = miller(start.clone());
///     let t = tabs(m);
///     column([t.strip().fixed(1), m.navigation(m.view()).flex(1)])
///         .on_key("t", move |_| {
///             t.open();
///         })
///         .on_key("1", move |_| t.switch(0))
///         .on_key("H", move |_| {
///             t.back();
///         })
/// });
/// let mut driver = app.driver(60, 4);
/// // A second tab, which goes into `docs`; the first stays where it was.
/// driver.press("t l");
/// assert!(driver.screen().plain()[0].trim_end().ends_with("docs"));
/// driver.press("H");
/// assert!(!driver.screen().plain()[0].trim_end().ends_with("docs"));
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn tabs(miller: Miller) -> FileTabs {
    let cwd = miller.cwd;
    let set = tab_set(move || FileTab::at(cwd.get_untracked()));
    // A selection made for the columns is kept per tab.
    miller.tabs.replace(Some(set));
    if let Some(selection) = miller.selection.get_untracked() {
        selection.scoped(&set);
    }
    // The active tab follows the columns.
    watch(
        move || cwd.get(),
        move |dir, _| {
            let tab = set.with_active(|t| t.cwd);
            if tab.with_untracked(|at| *at != dir) {
                tab.set(dir);
            }
        },
    );
    // The columns show the active tab's directory: after a switch, a
    // close, or a move through its history.
    watch(
        move || set.with_active(|t| t.cwd.get()),
        move |dir, _| {
            if cwd.with_untracked(|at| *at != dir) {
                miller.cd(dir, None);
            }
        },
    );
    FileTabs { set, miller }
}

impl FileTabs {
    /// The core tab set underneath.
    pub fn set(&self) -> TabSet<FileTab> {
        self.set
    }

    /// The active tab's index, a signal.
    pub fn active(&self) -> Signal<usize> {
        self.set.active()
    }

    /// The active tab.
    pub fn current(&self) -> FileTab {
        self.set.with_active(|t| *t)
    }

    /// Open a tab at the columns' directory after the active one, and
    /// switch to it (Yazi's `t`); its index.
    pub fn open(&self) -> usize {
        self.set.open()
    }

    /// Open a tab at `dir` after the active one, and switch to it.
    pub fn open_at(&self, dir: impl Into<PathBuf>) -> usize {
        self.set.open_with(FileTab::at(dir.into()))
    }

    /// Close the active tab; the last one is not closed (Yazi quits
    /// there): whether it was.
    pub fn close_active(&self) -> bool {
        self.set.close_active()
    }

    /// Switch to tab `to` (kept among the tabs).
    pub fn switch(&self, to: usize) {
        self.set.switch(to);
    }

    /// Switch to the tab `by` places away, wrapping (`[` and `]`).
    pub fn cycle(&self, by: isize) {
        self.set.cycle(by);
    }

    /// Move the active tab `by` places (`{` and `}`).
    pub fn swap(&self, by: isize) {
        self.set.swap(by);
    }

    /// Back to the directory the active tab showed before (`H`); whether
    /// there was one.
    pub fn back(&self) -> bool {
        self.current().history.back()
    }

    /// Forward again (`L`); whether there was a directory to go to.
    pub fn forward(&self) -> bool {
        self.current().history.forward()
    }

    /// The selection the columns act on (the active tab's, through one
    /// handle: see [`selection`](crate::files::selection)), if one was
    /// made.
    pub fn selection(&self) -> Option<MultiSelect<PathBuf>> {
        self.miller.selection.get_untracked()
    }

    /// Name the active tab (`None`: its directory's name again).
    pub fn rename(&self, name: Option<String>) {
        self.current().name.set(name);
    }

    /// The columns the tabs move.
    pub fn miller(&self) -> Miller {
        self.miller
    }

    /// A [`tabs`](crate::widgets::tabs) strip of the tabs' titles
    /// ([`FileTab::title`]), the active one selected; a click switches. It
    /// takes the strip's options ([`fit`](Node::fit),
    /// [`auto_hide`](Node::auto_hide), …).
    pub fn strip(&self) -> Node {
        tabs_for(&self.set, FileTab::title)
    }
}

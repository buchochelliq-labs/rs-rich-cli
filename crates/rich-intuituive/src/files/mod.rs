//! A file manager's components, behind the off-by-default `files` feature:
//! what rebuilding [Yazi](https://github.com/sxyazi/yazi) by hand
//! (`examples/files.rs`) took a few hundred lines each to write. A plugin:
//! each component with a generic shape underneath is that core component
//! with a file system plugged in ([`miller`] on
//! [`miller_of`](crate::miller_of), [`tasks`] on [`jobs`](crate::jobs::jobs),
//! [`finder`] on [`finder_of`](crate::finder_of), [`preview`] on
//! [`preview_of`](crate::preview_of), [`path_crumb`] on
//! [`breadcrumb`](crate::widgets::breadcrumb)).
//!
//! - [`dir_model`]: a directory's entries as signals, sorted and filtered,
//!   read again on [`refresh`](DirModel::refresh), followed on disk with
//!   the `watch` feature ([`DirModel::watch`]), and typed by their first
//!   bytes in the background ([`DirModel::sniff`]).
//! - [`file_list`]: a lazy list of entries as Yazi draws them: icon, type
//!   colour, marker, find highlights, a git sign and a line mode
//!   ([`Linemode`]) at the right.
//! - [`preview`]: a pane that loads a preview of a path off the app's
//!   thread and picks how to show it by type: a directory's entries,
//!   highlighted text, an image (`art` feature), an archive's listing
//!   (`archive` feature), a hex dump of anything else. Stale results are
//!   dropped.
//! - [`miller`]: Miller columns, the parent, the current directory and the
//!   preview over one cursor, with remembered positions, clicks that
//!   navigate and Yazi's movement keys ([`Miller::navigation`]).
//! - [`open_with`]: runs one of rs-rich-ext's [openers](rich_ext::open)
//!   (Yazi's `[open]` rules): an editor gets the terminal through a
//!   handoff, the system's opener runs in the background.
//! - [`tasks`] and [`tasks_panel`]: copies, moves, deletes, trashing and
//!   extraction as background jobs with progress, cancellable (Yazi's
//!   `w`).
//! - [`finder`]: a fuzzy finder over a directory walk whose results stream
//!   in while the walk goes on (Yazi's `z` over fd and fzf).
//! - [`path_crumb`]: a path as a header, the home directory as `~`.
//! - [`git_status`]: `git status` per file, loaded in the background.
//! - [`hex_view`]: the core's scrolling hex dump, re-exported.
//!
//! The parts that need no terminal are in rs-rich-ext:
//! [`listing`](rich_ext::listing) (entries and sorting),
//! [`open`](rich_ext::open), [`file_ops`](rich_ext::file_ops),
//! [`trash`](rich_ext::trash), [`git`](rich_ext::git) and `archive`.
//!
//! ```
//! use intuituive::files::miller;
//! use intuituive::prelude::*;
//!
//! let dir = std::env::temp_dir().join(format!("intuituive-files-doc-{}", std::process::id()));
//! std::fs::create_dir_all(dir.join("docs")).unwrap();
//! std::fs::write(dir.join("notes.txt"), "hello").unwrap();
//! let start = dir.clone();
//! let app = App::new(move || {
//!     let columns = miller(start.clone());
//!     columns.navigation(columns.view()).on_key("q", |cx| cx.quit())
//! })
//! .wait_for_tasks(true);
//! let screen = app.render_with(&["j", "q"], 60, 4).unwrap();
//! assert!(screen[1].contains("notes.txt") && screen[0].contains("hello"));
//! # std::fs::remove_dir_all(&dir).ok();
//! ```

mod crumb;
mod finder;
mod frecency;
mod miller;
mod model;
mod open;
mod preview;
mod row;
mod search;
mod select;
mod shell;
mod spot;
mod tabs;
mod tasks;

use std::sync::atomic::Ordering;

/// The core's [`hex_view`](crate::widgets::hex_view), here too.
pub use crate::widgets::hex_view;
pub use crumb::{home_dir, path_crumb, path_crumb_at, path_parts};
pub use finder::{finder, finder_with, Finder};
pub use frecency::{frecency, frecency_at, state_dir, Frecency, Visit};
pub use miller::{miller, Miller, MillerOptions};
pub use model::{dir_model, dir_model_with, git_status, DirModel};
pub use open::{open_with, open_with_tasks, opener_rules};
pub use preview::{
    load_preview, preview, BinaryPreview, Preview, PreviewOptions, PreviewSettings, Previewer,
};
pub use row::{
    coloured_permissions, entry_markup, file_list, linemode_cell, EntryLook, FileListOptions,
    Linemode, Marker,
};
pub use search::{search, Search, SearchKind};
pub use select::{selection, Yank};
pub use shell::{bulk_rename, shell, shell_command, ShellOptions};
pub use spot::{spot, spot_facts};
pub use tabs::{tabs, FileTab, FileTabs};
pub use tasks::{tasks, tasks_panel, tasks_panel_with, Job, JobState, JobsPanelOptions, Tasks};

/// The listing types the components take, from rs-rich-ext.
pub use rich_ext::git::{Change, GitStatus};
pub use rich_ext::listing::{Entry, Sort, SortBy};

use crate::reactive::{Proxy, Runtime};

/// Run `work` on a thread of its own with a [`Proxy`] to deliver what it
/// finds, counted as a [task](crate::spawn) until it returns, so
/// [`App::wait_for_tasks`](crate::App::wait_for_tasks) waits for it.
pub(crate) fn background(what: &str, work: impl FnOnce(Proxy) + Send + 'static) {
    let runtime = Runtime::current_for(what);
    let proxy = runtime.proxy();
    let tasks = runtime.tasks.clone();
    tasks.fetch_add(1, Ordering::SeqCst);
    std::thread::spawn(move || {
        // Counted done however the work ends, a panic included.
        struct Done(std::sync::Arc<std::sync::atomic::AtomicUsize>);
        impl Drop for Done {
            fn drop(&mut self) {
                self.0.fetch_sub(1, Ordering::SeqCst);
            }
        }
        let _done = Done(tasks);
        work(proxy);
    });
}

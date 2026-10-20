//! A directory's entries as signals: Yazi's `Folder`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rich_ext::filetype;
use rich_ext::git::GitStatus;
use rich_ext::listing::{read_dir, smart_contains, sort_entries, Entry, Sort};

use crate::reactive::{memo, signal, Memo, Signal};
use crate::task::spawn;

/// The types sniffed for one directory's files, by name.
#[derive(Clone, Debug, Default, PartialEq)]
struct Sniffed {
    dir: Option<PathBuf>,
    types: HashMap<String, &'static str>,
}

/// A directory's entries, read from the disk into signals: sorted by
/// [`sort`](Self::sort), hidden files shown with [`hidden`](Self::hidden),
/// narrowed by [`filter`](Self::filter) (smart case), and read again
/// whenever the directory changes, a setting does, or
/// [`refresh`](Self::refresh) is called. `Copy`, so any handler takes
/// one. See [`dir_model`].
#[derive(Clone, Copy)]
pub struct DirModel {
    dir: Memo<Option<PathBuf>>,
    /// Show the entries whose name starts with a dot.
    pub hidden: Signal<bool>,
    pub sort: Signal<Sort>,
    /// Keep only the entries whose name contains this (smart case: case
    /// counts only when it has a capital). Empty keeps all.
    pub filter: Signal<String>,
    tick: Signal<u64>,
    listing: Memo<Result<Vec<Entry>, String>>,
    entries: Memo<Vec<Entry>>,
    sniffed: Signal<Sniffed>,
    /// A listing of its own in place of the directory's (a search's
    /// results), its names paths from the directory.
    pub(crate) results: Signal<Option<Signal<Vec<Entry>>>>,
}

/// The entries of the directory `dir` gives (which may read signals, so
/// the model follows it; `None` lists nothing), hidden files left out,
/// sorted as Yazi does by default. Call it while the app is built or
/// running.
///
/// ```
/// use intuituive::files::dir_model;
/// use intuituive::prelude::*;
///
/// let dir = std::env::temp_dir().join(format!("intuituive-dir-model-doc-{}", std::process::id()));
/// std::fs::create_dir_all(dir.join("sub")).unwrap();
/// std::fs::write(dir.join("b.txt"), "").unwrap();
/// std::fs::write(dir.join(".hidden"), "").unwrap();
/// let shown = dir.clone();
/// let app = App::new(move || {
///     let shown = shown.clone();
///     let model = dir_model(move || Some(shown.clone()));
///     text(move || model.entries().with(|e| e.iter().map(|e| e.name.clone()).collect::<Vec<_>>().join(" ")))
///         .on_key(".", move |_| model.hidden.update(|h| *h = !*h))
///         .on_key("q", |cx| cx.quit())
/// });
/// assert_eq!(app.render_with(&["q"], 30, 1).unwrap()[0].trim_end(), "sub b.txt");
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn dir_model(dir: impl Fn() -> Option<PathBuf> + 'static) -> DirModel {
    dir_model_with(dir, signal(false), signal(Sort::default()))
}

/// [`dir_model`] with its hidden-files and sort settings in signals you
/// give, so several models (Miller's columns) share them.
pub fn dir_model_with(
    dir: impl Fn() -> Option<PathBuf> + 'static,
    hidden: Signal<bool>,
    sort: Signal<Sort>,
) -> DirModel {
    let dir = memo(dir);
    let tick = signal(0u64);
    let results: Signal<Option<Signal<Vec<Entry>>>> = signal(None);
    let listing = memo(move || {
        tick.get();
        let (hidden, sort) = (hidden.get(), sort.get());
        if let Some(found) = results.get() {
            let mut found = found.get();
            sort_entries(&mut found, sort);
            return Ok(found);
        }
        match dir.get() {
            Some(dir) => read_dir(&dir, hidden, sort).map_err(|e| e.to_string()),
            None => Ok(Vec::new()),
        }
    });
    let mut model = with_listing(dir, listing, hidden, sort, tick);
    model.results = results;
    model
}

/// A model of the entries `listing` gives for the directory `dir`, read
/// elsewhere (a preview's, loaded on a worker): it never reads the disk.
pub(crate) fn dir_model_of(
    dir: impl Fn() -> Option<PathBuf> + 'static,
    listing: impl Fn() -> Result<Vec<Entry>, String> + 'static,
    hidden: Signal<bool>,
    sort: Signal<Sort>,
) -> DirModel {
    with_listing(memo(dir), memo(listing), hidden, sort, signal(0))
}

fn with_listing(
    dir: Memo<Option<PathBuf>>,
    listing: Memo<Result<Vec<Entry>, String>>,
    hidden: Signal<bool>,
    sort: Signal<Sort>,
    tick: Signal<u64>,
) -> DirModel {
    let filter = signal(String::new());
    let entries = memo(move || {
        let filter = filter.get();
        listing.with(|l| match l {
            Ok(all) if filter.is_empty() => all.clone(),
            Ok(all) => all
                .iter()
                .filter(|e| smart_contains(&e.name, &filter))
                .cloned()
                .collect(),
            Err(_) => Vec::new(),
        })
    });
    DirModel {
        dir,
        hidden,
        sort,
        filter,
        tick,
        listing,
        entries,
        sniffed: signal(Sniffed::default()),
        results: signal(None),
    }
}

impl DirModel {
    /// The directory listed (subscribing).
    pub fn dir(&self) -> Option<PathBuf> {
        self.dir.get()
    }

    /// The directory listed, without subscribing.
    pub fn dir_untracked(&self) -> Option<PathBuf> {
        self.dir.get_untracked()
    }

    /// The entries shown: the listing, filtered.
    pub fn entries(&self) -> Memo<Vec<Entry>> {
        self.entries
    }

    /// Every entry (but hidden ones, unless shown), or why the directory
    /// cannot be read.
    pub fn listing(&self) -> Memo<Result<Vec<Entry>, String>> {
        self.listing
    }

    /// How many entries are shown (subscribing).
    pub fn len(&self) -> usize {
        self.entries.with(Vec::len)
    }

    /// Whether no entry is shown (subscribing).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The entry shown at `index` (subscribing).
    pub fn get(&self, index: usize) -> Option<Entry> {
        self.entries.with(|e| e.get(index).cloned())
    }

    /// Where the entry called `name` is shown, without subscribing.
    pub fn position(&self, name: &str) -> Option<usize> {
        self.entries
            .with_untracked(|e| e.iter().position(|e| e.name == name))
    }

    /// The path of `entry` in the directory, without subscribing.
    pub fn path_of(&self, entry: &Entry) -> Option<PathBuf> {
        self.dir.get_untracked().map(|d| d.join(&entry.name))
    }

    /// Read the directory again (after changing it, say).
    pub fn refresh(&self) {
        self.tick.update(|t| *t += 1);
    }

    /// A count that goes up whenever the directory is read again by
    /// [`refresh`](Self::refresh) (subscribing): for work that should follow
    /// it, such as a git status.
    pub fn generation(&self) -> u64 {
        self.tick.get()
    }

    pub(crate) fn tick_signal(&self) -> Signal<u64> {
        self.tick
    }

    /// The MIME type of `entry` (subscribing): the one sniffed from its
    /// first bytes once [`sniff`](Self::sniff) has got to it, else the one
    /// its name gives (`inode/directory` for a directory, `""` when the
    /// name says nothing).
    pub fn mime(&self, entry: &Entry) -> &'static str {
        if entry.dir {
            return "inode/directory";
        }
        let sniffed = self.sniffed.with(|s| s.types.get(&entry.name).copied());
        sniffed.unwrap_or_else(|| entry.mime_by_name())
    }

    /// Sniff the MIME type of each file from its first bytes, on a worker
    /// thread, whenever the directory is read; [`mime`](Self::mime) (and
    /// so a [`file_list`](crate::files::file_list)'s type colours) take
    /// them as they arrive, 256 files at a time. Yazi types files this
    /// way; by name alone a PNG called `.txt` or a script with no
    /// extension gets the wrong colour. A directory left before its sniff
    /// is done stops it.
    // A handle: the change is made through shared state, so the
    // returned copy is only for chaining and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn sniff(self) -> DirModel {
        let model = self;
        let mut running: Option<Arc<AtomicBool>> = None;
        crate::reactive::watch(
            move || (model.dir.get(), model.tick.get(), model.hidden.get()),
            move |(dir, _, _), _| {
                if let Some(stop) = running.take() {
                    stop.store(true, Ordering::SeqCst);
                }
                let Some(dir) = dir else {
                    return;
                };
                let names: Vec<String> = model.listing.with_untracked(|l| {
                    l.as_ref()
                        .map(|all| {
                            all.iter()
                                .filter(|e| !e.dir)
                                .map(|e| e.name.clone())
                                .collect()
                        })
                        .unwrap_or_default()
                });
                model.sniffed.update(|s| {
                    if s.dir.as_ref() != Some(&dir) {
                        *s = Sniffed {
                            dir: Some(dir.clone()),
                            types: HashMap::new(),
                        };
                    }
                });
                let stop = Arc::new(AtomicBool::new(false));
                running = Some(stop.clone());
                super::background("DirModel::sniff()", move |proxy| {
                    for chunk in names.chunks(256) {
                        if stop.load(Ordering::SeqCst) {
                            return;
                        }
                        let paths: Vec<PathBuf> = chunk.iter().map(|n| dir.join(n)).collect();
                        let found: Vec<(String, &'static str)> = chunk
                            .iter()
                            .cloned()
                            .zip(filetype::mime_batch(&paths))
                            .filter_map(|(name, mime)| Some((name, mime?)))
                            .collect();
                        let dir = dir.clone();
                        let sent = proxy.run(move || {
                            model.sniffed.update(|s| {
                                if s.dir.as_ref() == Some(&dir) {
                                    s.types.extend(found);
                                }
                            })
                        });
                        if !sent {
                            return;
                        }
                    }
                });
            },
        );
        self
    }

    /// Follow the directory on disk: read it again when its entries
    /// change (through [`watch_path`](crate::watch_path); the watch moves
    /// with the directory). With the `watch` feature.
    #[cfg(feature = "watch")]
    // A handle: the change is made through shared state, so the
    // returned copy is only for chaining and may be dropped.
    #[allow(clippy::return_self_not_must_use)]
    pub fn watch(self) -> DirModel {
        let model = self;
        let mut watching: Option<crate::PathWatch> = None;
        crate::reactive::watch(
            move || model.dir.get(),
            move |dir, _| {
                if let Some(old) = watching.take() {
                    old.stop();
                }
                if let Some(dir) = dir {
                    watching = crate::watch_path(&dir, move |_, _| model.refresh()).ok();
                }
            },
        );
        self
    }
}

/// `git status` for the directory `model` lists, loaded on a worker thread
/// whenever the directory changes or is read again: `None` outside a
/// repository (or before the first answer). A
/// [`file_list`](crate::files::file_list) shows it with
/// [`FileListOptions::git`](crate::files::FileListOptions::git).
pub fn git_status(model: DirModel) -> Signal<Option<GitStatus>> {
    let status: Signal<Option<GitStatus>> = signal(None);
    crate::reactive::watch(
        move || (model.dir.get(), model.tick.get()),
        move |(dir, _), _| {
            let Some(dir) = dir else {
                status.update(|s| *s = None);
                return;
            };
            spawn(
                {
                    let dir = dir.clone();
                    move || rich_ext::git::status(&dir).ok().flatten()
                },
                move |found, _| {
                    // A status for a directory left since is dropped.
                    if model.dir.get_untracked().as_ref() == Some(&dir) {
                        status.update(|s| *s = found);
                    }
                },
            );
        },
    );
    status
}

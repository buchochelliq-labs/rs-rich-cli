//! Watching files and directories: wake the app when something on disk
//! changes, without polling. Behind the off-by-default `watch` feature,
//! on [notify](https://docs.rs/notify) (inotify on Linux, FSEvents on
//! macOS, ReadDirectoryChangesW on Windows, kqueue on the BSDs).
//!
//! [`watch_path`] starts a watcher on its own thread. Changes that arrive
//! close together are gathered into one [`PathEvent`] (debounced: a save
//! that writes, truncates and renames is one event, not five), and the
//! handler runs with it on the app's thread, between frames, where it may
//! write signals, as a [`Proxy`](crate::Proxy) closure does.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use notify::{EventKind, RecursiveMode, Watcher};

use crate::app::Ctx;
use crate::reactive::{signal, Runtime};

/// What happened to a path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChangeKind {
    /// It appeared.
    Created,
    /// Its contents or metadata changed.
    Modified,
    /// It went away.
    Removed,
    /// It was renamed, to or from this name.
    Renamed,
    /// Something else the platform reported (an access, a rescan).
    Other,
}

/// One changed path in a [`PathEvent`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathChange {
    pub path: PathBuf,
    pub kind: ChangeKind,
}

/// The changes a watcher gathered over one debounce interval: each path
/// once, in the order it first changed, with what last happened to it (a
/// path created and then written is still [`Created`](ChangeKind::Created)).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathEvent {
    pub changes: Vec<PathChange>,
}

impl PathEvent {
    /// The changed paths.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.changes.iter().map(|change| change.path.as_path())
    }

    /// Whether `path` is among them.
    pub fn touches(&self, path: &Path) -> bool {
        self.paths().any(|p| p == path)
    }
}

/// How [`watch_path_with`] watches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchOptions {
    /// Watch every directory below too (off: the path itself, and a
    /// directory's direct entries).
    pub recursive: bool,
    /// How long to gather changes after the first before the handler
    /// runs (100 ms by default).
    pub debounce: Duration,
}

impl Default for WatchOptions {
    fn default() -> Self {
        WatchOptions {
            recursive: false,
            debounce: Duration::from_millis(100),
        }
    }
}

impl WatchOptions {
    pub fn new() -> WatchOptions {
        WatchOptions::default()
    }

    /// Watch the whole tree below the path.
    pub fn recursive(mut self) -> WatchOptions {
        self.recursive = true;
        self
    }

    /// Gather changes for `debounce` before calling the handler.
    pub fn debounce(mut self, debounce: Duration) -> WatchOptions {
        self.debounce = debounce;
        self
    }
}

/// A running watcher (see [`watch_path`]). It runs as long as the app;
/// [`stop`](Self::stop) ends it sooner. Dropping the handle does not stop
/// it, so an app may ignore the handle. Two handles are equal when they
/// are handles of the same watcher, so they can be kept in a signal.
#[derive(Clone, Debug)]
pub struct PathWatch {
    stop: Arc<AtomicBool>,
}

impl PartialEq for PathWatch {
    /// The same watcher.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.stop, &other.stop)
    }
}

impl Eq for PathWatch {}

impl PathWatch {
    /// Stop watching: no handler runs after the change being delivered,
    /// if any.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }

    /// Whether [`stop`](Self::stop) was called.
    pub fn is_stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }
}

/// Watch `path` (a file, or a directory and its direct entries) and call
/// `on_change` on the app's thread when it changes, with the changes
/// gathered over 100 ms. Call it while the app is built or running. See
/// [`watch_path_with`] for a recursive watch and another interval.
///
/// ```no_run
/// use intuituive::prelude::*;
/// use intuituive::watch_path;
///
/// App::new(|| {
///     let entries = signal(0usize);
///     let count = || std::fs::read_dir(".").map_or(0, |d| d.count());
///     entries.set(count());
///     watch_path(".", move |_event, _cx| entries.set(count()))
///         .expect("a watcher");
///     text!("{entries} entries").on_key("q", |cx| cx.quit())
/// })
/// .run()
/// # .unwrap();
/// ```
///
/// # Errors
/// When the platform's watcher cannot start or cannot watch `path` (it
/// does not exist; Linux's inotify watch limit is reached).
///
/// # Panics
/// Outside a running or building app.
pub fn watch_path(
    path: impl AsRef<Path>,
    on_change: impl FnMut(&PathEvent, &mut Ctx) + 'static,
) -> std::io::Result<PathWatch> {
    watch_path_with(path, WatchOptions::default(), on_change)
}

/// The handler a watcher calls, kept on the app's thread.
type Handler = Rc<RefCell<Box<dyn FnMut(&PathEvent, &mut Ctx)>>>;

/// [`watch_path`] with [`WatchOptions`]: recursive, or another debounce
/// interval.
///
/// ```
/// use std::time::Duration;
/// use intuituive::prelude::*;
/// use intuituive::{watch_path_with, WatchOptions};
///
/// let dir = std::env::temp_dir().join(format!("intuituive-watch-doc-{}", std::process::id()));
/// std::fs::create_dir_all(&dir).unwrap();
/// let watched = dir.clone();
/// let app = App::new(move || {
///     let changed = signal(String::from("waiting"));
///     let options = WatchOptions::new().recursive().debounce(Duration::from_millis(20));
///     watch_path_with(&watched, options, move |event, _| {
///         changed.set(format!("{} changed", event.changes.len()));
///     })
///     .unwrap();
///     text!("{changed}")
/// });
/// let mut driver = app.driver(20, 1);
/// driver.settle();
/// std::fs::write(dir.join("new.txt"), "hello").unwrap();
/// for _ in 0..200 {
///     std::thread::sleep(Duration::from_millis(20));
///     driver.settle();
///     if driver.screen().plain()[0].contains("changed") {
///         break;
///     }
/// }
/// assert!(driver.screen().plain()[0].contains("changed"));
/// # std::fs::remove_dir_all(&dir).ok();
/// ```
pub fn watch_path_with(
    path: impl AsRef<Path>,
    options: WatchOptions,
    on_change: impl FnMut(&PathEvent, &mut Ctx) + 'static,
) -> std::io::Result<PathWatch> {
    let runtime = Runtime::current_for("watch_path()");
    let proxy = runtime.proxy();
    let handler: Handler = Rc::new(RefCell::new(Box::new(on_change)));
    let handler = signal(handler);
    let (events, inbox) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(events).map_err(to_io)?;
    let mode = if options.recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };
    watcher.watch(path.as_ref(), mode).map_err(to_io)?;
    let stop = Arc::new(AtomicBool::new(false));
    let handle = PathWatch { stop: stop.clone() };
    std::thread::Builder::new()
        .name("intuituive-watch".into())
        .spawn(move || {
            // The watcher lives (and watches) as long as this thread.
            let _watcher = watcher;
            let mut gathered = Gathered::default();
            loop {
                let first = match inbox.recv_timeout(Duration::from_millis(500)) {
                    Ok(event) => event,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Is anyone still listening? A job nobody runs
                        // fails to send once the app has finished.
                        if stop.load(Ordering::SeqCst) || !proxy.run(|| {}) {
                            return;
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                };
                gathered.add(first);
                let until = Instant::now() + options.debounce;
                while let Some(left) = until.checked_duration_since(Instant::now()) {
                    match inbox.recv_timeout(left) {
                        Ok(event) => gathered.add(event),
                        Err(_) => break,
                    }
                }
                let event = gathered.take();
                if event.changes.is_empty() {
                    continue;
                }
                if stop.load(Ordering::SeqCst) {
                    return;
                }
                let stopped = stop.clone();
                let delivered = proxy.run_with(move |cx| {
                    if stopped.load(Ordering::SeqCst) {
                        return;
                    }
                    let handler = handler.with_untracked(Rc::clone);
                    (handler.borrow_mut())(&event, cx);
                });
                if !delivered {
                    return;
                }
            }
        })?;
    Ok(handle)
}

fn to_io(error: notify::Error) -> std::io::Error {
    match error.kind {
        notify::ErrorKind::Io(io) => io,
        notify::ErrorKind::PathNotFound => {
            std::io::Error::new(std::io::ErrorKind::NotFound, "no such path to watch")
        }
        _ => std::io::Error::other(error.to_string()),
    }
}

/// What a notify event says happened.
fn kind_of(kind: &EventKind) -> ChangeKind {
    use notify::event::ModifyKind;
    match kind {
        EventKind::Create(_) => ChangeKind::Created,
        EventKind::Modify(ModifyKind::Name(_)) => ChangeKind::Renamed,
        EventKind::Modify(_) => ChangeKind::Modified,
        EventKind::Remove(_) => ChangeKind::Removed,
        _ => ChangeKind::Other,
    }
}

/// Changes gathered over a debounce interval.
#[derive(Default)]
struct Gathered {
    changes: Vec<PathChange>,
    index: HashMap<PathBuf, usize>,
}

impl Gathered {
    /// Note one event from the watcher. Errors carry no path to report,
    /// and are dropped.
    fn add(&mut self, event: notify::Result<notify::Event>) {
        let Ok(event) = event else {
            return;
        };
        let kind = kind_of(&event.kind);
        // Reads and opens are not changes.
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        for path in event.paths {
            self.note(path, kind);
        }
    }

    fn note(&mut self, path: PathBuf, kind: ChangeKind) {
        match self.index.get(&path) {
            Some(&i) => {
                let last = &mut self.changes[i].kind;
                // Created and then written is still new.
                if !(*last == ChangeKind::Created && kind == ChangeKind::Modified) {
                    *last = kind;
                }
            }
            None => {
                self.index.insert(path.clone(), self.changes.len());
                self.changes.push(PathChange { path, kind });
            }
        }
    }

    fn take(&mut self) -> PathEvent {
        self.index.clear();
        PathEvent {
            changes: std::mem::take(&mut self.changes),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, CreateKind, DataChange, ModifyKind, RemoveKind, RenameMode};

    fn event(kind: EventKind, path: &str) -> notify::Result<notify::Event> {
        Ok(notify::Event::new(kind).add_path(PathBuf::from(path)))
    }

    #[test]
    fn handles_are_equal_when_they_are_one_watcher() {
        let one = PathWatch {
            stop: Arc::new(AtomicBool::new(false)),
        };
        let other = PathWatch {
            stop: Arc::new(AtomicBool::new(false)),
        };
        assert_eq!(one, one.clone());
        assert_ne!(one, other);
    }

    #[test]
    fn changes_gather_once_a_path() {
        let mut gathered = Gathered::default();
        gathered.add(event(EventKind::Create(CreateKind::File), "/a"));
        gathered.add(event(
            EventKind::Modify(ModifyKind::Data(DataChange::Content)),
            "/a",
        ));
        gathered.add(event(
            EventKind::Modify(ModifyKind::Data(DataChange::Any)),
            "/b",
        ));
        gathered.add(event(EventKind::Access(AccessKind::Read), "/c"));
        gathered.add(event(
            EventKind::Modify(ModifyKind::Name(RenameMode::From)),
            "/d",
        ));
        gathered.add(event(EventKind::Remove(RemoveKind::File), "/b"));
        gathered.add(Err(notify::Error::generic("lost")));
        let got = gathered.take();
        let kinds: Vec<(&str, ChangeKind)> = got
            .changes
            .iter()
            .map(|c| (c.path.to_str().unwrap(), c.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("/a", ChangeKind::Created),
                ("/b", ChangeKind::Removed),
                ("/d", ChangeKind::Renamed),
            ]
        );
        assert!(got.touches(Path::new("/d")));
        assert!(!got.touches(Path::new("/c")));
        // Taking empties it.
        assert!(gathered.take().changes.is_empty());
    }

    #[test]
    fn a_missing_path_is_an_error() {
        let app = crate::App::new(|| {
            let error = watch_path("/no/such/path/for/intuituive", |_, _| {}).unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
            crate::label("ok").on_key("q", |cx| cx.quit())
        });
        let screen = app.render_with(&["q"], 4, 1).unwrap();
        assert_eq!(screen[0].trim_end(), "ok");
    }

    #[test]
    fn a_change_wakes_the_app_and_stop_ends_it() {
        let dir = std::env::temp_dir().join(format!("intuituive-watch-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let watched = dir.clone();
        let handle = Rc::new(RefCell::new(None));
        let kept = handle.clone();
        let app = crate::App::new(move || {
            let seen = signal(Vec::<String>::new());
            let options = WatchOptions::new()
                .recursive()
                .debounce(Duration::from_millis(30));
            let watch = watch_path_with(&watched, options, move |event, _| {
                seen.update(|seen| {
                    seen.extend(
                        event
                            .paths()
                            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned()),
                    )
                });
            })
            .unwrap();
            *kept.borrow_mut() = Some(watch);
            crate::text(move || seen.get().join(","))
        });
        let mut driver = app.driver(40, 1);
        driver.settle();
        let wait_for = |driver: &mut crate::Driver, what: &str, tries: usize| {
            for _ in 0..tries {
                std::thread::sleep(Duration::from_millis(20));
                driver.settle();
                if driver.screen().plain()[0].contains(what) {
                    return true;
                }
            }
            false
        };
        // Recursive: a file in a subdirectory.
        std::fs::write(dir.join("sub").join("deep.txt"), "x").unwrap();
        assert!(
            wait_for(&mut driver, "deep.txt", 250),
            "{:?}",
            driver.screen().plain()
        );
        // Stopped: later changes are not delivered.
        handle.borrow().as_ref().unwrap().stop();
        assert!(handle.borrow().as_ref().unwrap().is_stopped());
        std::fs::write(dir.join("late.txt"), "x").unwrap();
        assert!(!wait_for(&mut driver, "late.txt", 25));
        std::fs::remove_dir_all(&dir).ok();
    }
}

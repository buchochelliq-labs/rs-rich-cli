//! Running an opener from an app.

use std::path::PathBuf;
use std::process::Stdio;

use rich::markup::escape;
use rich_ext::open::{Launch, Opener, Rules};

use super::tasks::Tasks;
use crate::app::Ctx;
use crate::notify::Level;
use crate::task::spawn;

/// Yazi's default opener rules ([`Rules::yazi`]): what an app usually
/// starts from, adding rules of its own with [`Rules::prepend`].
pub fn opener_rules() -> Rules {
    Rules::yazi()
}

/// Open `paths` with `opener`, from a handler. A blocking opener (the
/// editor, a pager) gets the terminal through
/// [`Ctx::handoff_then`]: the app steps aside until it ends. The others
/// run in the background with no input and their output dropped
/// (`xdg-open` and friends return at once). [`Launch::Extract`] extracts
/// each archive beside itself with the `archive` feature (else it says it
/// cannot). `done` runs on the app's thread when the opener has ended (a
/// blocking one) or started (the others), to read the directory again;
/// failures are notifications titled `Open`.
///
/// ```
/// use intuituive::files::open_with;
/// use intuituive::prelude::*;
/// use rich_ext::open::{Launch, Opener};
///
/// let app = App::new(|| {
///     label("files").on_key("e", |cx| {
///         let pager = Opener::new("view", "View", Launch::Shell("less \"$@\"".into())).block(true);
///         open_with(cx, &pager, vec!["notes.md".into()], |_| {});
///     })
/// });
/// let mut driver = app.driver(10, 1);
/// driver.press("e");
/// // Under the headless driver a handoff is recorded, not run.
/// let handoff = driver.take_handoff().expect("the pager took the terminal");
/// # let _ = handoff;
/// ```
pub fn open_with(
    cx: &mut Ctx,
    opener: &Opener,
    paths: Vec<PathBuf>,
    done: impl FnOnce(&mut Ctx) + Send + 'static,
) {
    if paths.is_empty() {
        return;
    }
    if opener.launch == Launch::Extract {
        extract(paths, done);
        return;
    }
    let mut commands = opener.commands(&paths);
    if opener.block {
        // One program takes the terminal: the editor or the shell command
        // has every file; the system opener only the first.
        let Some(command) = commands.drain(..).next() else {
            return;
        };
        cx.handoff_then(command, move |ended, cx| {
            if let Err(error) = ended {
                cx.notify(Level::Error, "Open", escape(&error.to_string()));
            }
            done(cx);
        });
        return;
    }
    for mut command in commands {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        spawn(
            move || {
                command
                    .status()
                    .map_err(|e| e.to_string())
                    .and_then(|s| s.success().then_some(()).ok_or(s.to_string()))
            },
            |ran: Result<(), String>, cx| {
                if let Err(error) = ran {
                    cx.notify(Level::Error, "Open", escape(&error));
                }
            },
        );
    }
    // `done` after the handler: the openers are on their way.
    spawn(|| (), move |_, cx| done(cx));
}

/// [`open_with`], extraction ([`Launch::Extract`]) run as jobs of `tasks`
/// (Yazi's extract task), one an archive, each beside itself, with
/// progress and a cancel in a [`tasks_panel`](crate::files::tasks_panel);
/// what fails stays in the list (and goes to
/// [`Tasks::on_fail`](crate::files::Tasks::on_fail)). `done` runs once
/// every archive is extracted. Any other opener is [`open_with`]'s.
///
/// ```
/// use intuituive::files::{open_with_tasks, opener_rules, tasks, tasks_panel};
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let jobs = tasks();
///     let said = signal(String::from("waiting"));
///     column([tasks_panel(jobs).fixed(1), text!("{said}")]).on_key("x", move |cx| {
///         let extract = opener_rules().get("extract").unwrap().clone();
///         open_with_tasks(cx, &extract, vec!["/no/such.zip".into()], jobs, move |_| said.set("done".into()));
///     })
/// })
/// .wait_for_tasks(true);
/// let screen = app.render_with(&["x"], 40, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "done");
/// ```
pub fn open_with_tasks(
    cx: &mut Ctx,
    opener: &Opener,
    paths: Vec<PathBuf>,
    tasks: Tasks,
    done: impl FnOnce(&mut Ctx) + Send + 'static,
) {
    if opener.launch != Launch::Extract || paths.is_empty() {
        return open_with(cx, opener, paths, done);
    }
    extract_as_tasks(paths, tasks, done);
}

/// Extract each archive as a job; `done` after the last.
#[cfg(feature = "archive")]
fn extract_as_tasks(
    paths: Vec<PathBuf>,
    tasks: Tasks,
    done: impl FnOnce(&mut Ctx) + Send + 'static,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    type Done = Box<dyn FnOnce(&mut Ctx) + Send>;
    let left = Arc::new(AtomicUsize::new(paths.len()));
    let done: Arc<Mutex<Option<Done>>> = Arc::new(Mutex::new(Some(Box::new(done))));
    for path in paths {
        let dest = rich_ext::archive::default_dest(&path);
        let (left, done) = (left.clone(), done.clone());
        tasks.extract(path, dest, move |_, cx| {
            if left.fetch_sub(1, Ordering::SeqCst) == 1 {
                let last = done.lock().unwrap_or_else(|e| e.into_inner()).take();
                if let Some(last) = last {
                    last(cx);
                }
            }
        });
    }
}

/// Without the `archive` feature there is nothing to extract with.
#[cfg(not(feature = "archive"))]
fn extract_as_tasks(
    paths: Vec<PathBuf>,
    _tasks: Tasks,
    done: impl FnOnce(&mut Ctx) + Send + 'static,
) {
    extract(paths, done);
}

/// Extract each archive beside itself (rs-rich-ext's `archive`).
#[cfg(feature = "archive")]
fn extract(paths: Vec<PathBuf>, done: impl FnOnce(&mut Ctx) + Send + 'static) {
    spawn(
        move || {
            paths
                .iter()
                .filter_map(|path| {
                    let dest = rich_ext::archive::default_dest(path);
                    rich_ext::archive::extract(path, &dest)
                        .err()
                        .map(|e| format!("{}: {e}", path.display()))
                })
                .collect::<Vec<_>>()
        },
        move |errors, cx| {
            if !errors.is_empty() {
                cx.notify(Level::Error, "Extract", escape(&errors.join("\n")));
            }
            done(cx);
        },
    );
}

/// Without the `archive` feature there is nothing to extract with.
#[cfg(not(feature = "archive"))]
fn extract(_paths: Vec<PathBuf>, done: impl FnOnce(&mut Ctx) + Send + 'static) {
    spawn(
        || (),
        move |_, cx| {
            cx.notify(
                Level::Error,
                "Extract",
                "Archives are extracted with the `archive` feature.",
            );
            done(cx);
        },
    );
}

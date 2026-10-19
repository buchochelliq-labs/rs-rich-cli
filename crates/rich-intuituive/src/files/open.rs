//! Running an opener from an app.

use std::path::PathBuf;
use std::process::Stdio;

use rich::markup::escape;
use rich_ext::open::{Launch, Opener, Rules};

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

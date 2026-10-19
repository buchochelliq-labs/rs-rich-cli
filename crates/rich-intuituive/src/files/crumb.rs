//! A path as a breadcrumb: the core's generic
//! [`breadcrumb`](crate::widgets::breadcrumb) over a path's parts, with the
//! home directory as `~`.

use std::path::{Component, Path, PathBuf};

use crate::node::Node;
use crate::widgets::breadcrumb;

/// The home directory: `$HOME`, else `%USERPROFILE%`.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// `path`'s parts as a [`breadcrumb`] shows them with `/` between: `~` for
/// `home` (when `path` is in it), an empty first part for the root, so
/// the trail starts with the separator.
///
/// ```
/// use intuituive::files::path_parts;
/// use std::path::Path;
///
/// let home = Path::new("/home/ada");
/// assert_eq!(path_parts(Path::new("/home/ada/src"), Some(home)), ["~", "src"]);
/// assert_eq!(path_parts(Path::new("/usr/lib"), Some(home)), ["", "usr", "lib"]);
/// assert_eq!(path_parts(Path::new("/"), None), ["", ""]);
/// ```
pub fn path_parts(path: &Path, home: Option<&Path>) -> Vec<String> {
    let (shown, tilde) = match home.map(|home| path.strip_prefix(home)) {
        Some(Ok(rest)) => (rest, true),
        _ => (path, false),
    };
    let mut parts: Vec<String> = Vec::new();
    if tilde {
        parts.push("~".into());
    }
    for part in shown.components() {
        match part {
            Component::RootDir => parts.push(String::new()),
            other => parts.push(other.as_os_str().to_string_lossy().into_owned()),
        }
    }
    // The root alone: one separator.
    if parts.len() == 1 && parts[0].is_empty() {
        parts.push(String::new());
    }
    parts
}

/// A path as a one-row header, Yazi's: the core's [`breadcrumb`] over
/// [`path_parts`], `/` between them, the home directory
/// ([`home_dir`]) as `~`. Too long, it loses its start, so where you are
/// stays in view; [`crumb_suffix`](Node::crumb_suffix) adds markup that is
/// never cut (Yazi's `(filter: x)`). `path` may read signals.
pub fn path_crumb(path: impl Fn() -> PathBuf + 'static) -> Node {
    path_crumb_at(path, home_dir())
}

/// A [`path_crumb`] with `home` shown as `~`, or no `~` with `None`.
///
/// ```
/// use intuituive::files::path_crumb_at;
/// use intuituive::prelude::*;
/// use std::path::PathBuf;
///
/// let shown = |width| {
///     let app = App::new(|| {
///         let cwd = signal(PathBuf::from("/home/ada/projects/rich/src"));
///         path_crumb_at(move || cwd.get(), Some("/home/ada".into()))
///     });
///     let mut driver = app.driver(width, 1);
///     driver.settle();
///     driver.screen().plain()[0].trim_end().to_string()
/// };
/// assert_eq!(shown(30), "~/projects/rich/src");
/// assert_eq!(shown(12), "…ts/rich/src");
/// ```
pub fn path_crumb_at(path: impl Fn() -> PathBuf + 'static, home: Option<PathBuf>) -> Node {
    breadcrumb(move || path_parts(&path(), home.as_deref())).crumb_separator("/")
}

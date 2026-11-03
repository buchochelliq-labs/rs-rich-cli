//! A loader of Markdown files for the core's
//! [`markdown_viewer`](crate::markdown_viewer) (#805): the viewer stays
//! generic, and reading the file system is this plugin's.

use std::path::{Component, PathBuf};

/// A [`markdown_viewer`](crate::markdown_viewer) loader that reads a
/// location as a file under `root`: `guide/index.md` is
/// `root/guide/index.md`. A location that would leave `root` (an absolute
/// path, a `..`) is refused with [`std::io::ErrorKind::PermissionDenied`].
///
/// ```
/// use intuituive::files::markdown_loader;
///
/// let root = std::env::temp_dir().join("intuituive-markdown-loader-doc");
/// std::fs::create_dir_all(&root).unwrap();
/// std::fs::write(root.join("a.md"), "# A").unwrap();
/// let load = markdown_loader(&root);
/// assert_eq!(load("a.md").unwrap(), "# A");
/// assert!(load("../a.md").is_err());
/// assert!(load("missing.md").is_err());
/// ```
pub fn markdown_loader(
    root: impl Into<PathBuf>,
) -> impl Fn(&str) -> std::io::Result<String> + Send + Sync + 'static {
    let root = root.into();
    move |location: &str| {
        let path = std::path::Path::new(location);
        let inside = path
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir));
        if !inside {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("{location} is outside the documents"),
            ));
        }
        std::fs::read_to_string(root.join(path))
    }
}

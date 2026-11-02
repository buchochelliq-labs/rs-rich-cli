//! [`Workspace`]: a session's root set, and the checks that keep the agent's
//! file and terminal requests inside it.
//!
//! The protocol leaves enforcement to the client: "check the resolved path
//! at the time of use, compare paths by component, and fail closed". That
//! is what [`Workspace::check`] does.

use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::RpcError;

/// The directories a session may touch: its working directory first, then
/// any the user added. Every root is kept resolved (symbolic links
/// followed), so a check compares real paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    roots: Vec<PathBuf>,
}

impl Workspace {
    /// A workspace rooted at `cwd`, which must exist.
    pub fn new(cwd: impl AsRef<Path>) -> io::Result<Workspace> {
        Ok(Workspace {
            roots: vec![cwd.as_ref().canonicalize()?],
        })
    }

    /// Adds another root, which must exist.
    pub fn with_root(mut self, dir: impl AsRef<Path>) -> io::Result<Workspace> {
        self.roots.push(dir.as_ref().canonicalize()?);
        Ok(self)
    }

    /// The roots, resolved; the first is the working directory.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The working directory (the first root), resolved.
    pub fn cwd(&self) -> &Path {
        &self.roots[0]
    }

    /// Resolves `path` and checks it is inside a root. The path must be
    /// absolute. An existing path is resolved whole; a path that does not
    /// exist yet (a file to create) is resolved through its nearest
    /// existing ancestor, and the rest may not climb with `..`. Anything
    /// that cannot be resolved, or lands outside every root, is refused
    /// with "invalid params".
    pub fn check(&self, path: &Path) -> Result<PathBuf, RpcError> {
        if !path.is_absolute() {
            return Err(RpcError::invalid_params(format!(
                "the path is not absolute: {}",
                path.display()
            )));
        }
        let resolved = resolve(path).ok_or_else(|| {
            RpcError::invalid_params(format!("the path cannot be resolved: {}", path.display()))
        })?;
        if self.roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(RpcError::invalid_params(format!(
                "the path is outside the workspace: {}",
                path.display()
            )))
        }
    }

    /// Whether `path` resolves to a place inside a root.
    pub fn contains(&self, path: &Path) -> bool {
        self.check(path).is_ok()
    }
}

/// Resolves an absolute path: the longest existing prefix through the file
/// system, the rest as plain names. A name that is there but does not
/// resolve (a dangling symlink, or one this process may not follow) fails
/// closed: following it on a write could create a file anywhere.
fn resolve(path: &Path) -> Option<PathBuf> {
    if let Ok(real) = path.canonicalize() {
        return Some(real);
    }
    if path.symlink_metadata().is_ok() {
        return None;
    }
    let mut rest = Vec::new();
    let mut base = path;
    loop {
        let name = base.file_name()?;
        rest.push(name.to_owned());
        base = base.parent()?;
        let Ok(real) = base.canonicalize() else {
            if base.symlink_metadata().is_ok() {
                return None;
            }
            continue;
        };
        let mut resolved = real;
        for name in rest.iter().rev() {
            resolved.push(name);
        }
        // `file_name` skips `..` and `.` only at the end; refuse any left
        // inside the part that does not exist yet.
        let tail = path.strip_prefix(base).ok()?;
        if tail
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return None;
        }
        return Some(resolved);
    }
}

/// Files whose change can run code or change what the user is asked: version
/// control hooks and settings, editor and agent settings, MCP configuration,
/// environment loaders. A client should ask the user before the agent
/// writes one, even with writes allowed.
pub fn controls_execution(path: &Path) -> bool {
    const DIRS: &[&str] = &[
        ".git", ".hg", ".svn", ".vscode", ".idea", ".zed", ".cursor", ".claude", ".gemini",
        ".codex", ".github",
    ];
    const FILES: &[&str] = &[
        ".mcp.json",
        ".envrc",
        ".gitattributes",
        ".gitmodules",
        ".npmrc",
        ".pre-commit-config.yaml",
        "AGENTS.md",
        "CLAUDE.md",
        "GEMINI.md",
    ];
    let mut components = path.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            continue;
        };
        let Some(name) = name.to_str() else {
            continue;
        };
        if components.peek().is_some() {
            if DIRS.contains(&name) {
                return true;
            }
        } else if FILES.contains(&name) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("rich-acp-ws-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("inside")).unwrap();
        dir
    }

    #[test]
    fn inside_outside_and_new_files() {
        let dir = scratch("basic");
        let workspace = Workspace::new(dir.join("inside")).unwrap();
        let root = workspace.cwd().to_path_buf();
        assert!(workspace.contains(&root.join("new.txt")));
        assert!(workspace.contains(&root.join("a/b/new.txt")));
        assert!(!workspace.contains(&root.join("../outside.txt")));
        assert!(!workspace.contains(&root.join("a/../../outside.txt")));
        assert!(!workspace.contains(Path::new("relative.txt")));
        assert!(!workspace.contains(&dir.join("inside-not")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_out_are_refused() {
        let dir = scratch("link");
        std::fs::create_dir_all(dir.join("secret")).unwrap();
        std::os::unix::fs::symlink(dir.join("secret"), dir.join("inside/escape")).unwrap();
        let workspace = Workspace::new(dir.join("inside")).unwrap();
        assert!(!workspace.contains(&workspace.cwd().join("escape/key")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlinks_are_refused() {
        // A link to a file that does not exist yet, outside the workspace:
        // following it on a write would create that file.
        let dir = scratch("dangling");
        std::fs::create_dir_all(dir.join("outside")).unwrap();
        std::os::unix::fs::symlink(dir.join("outside/new.txt"), dir.join("inside/link")).unwrap();
        std::os::unix::fs::symlink(dir.join("outside/gone"), dir.join("inside/dir")).unwrap();
        let workspace = Workspace::new(dir.join("inside")).unwrap();
        assert!(!workspace.contains(&workspace.cwd().join("link")));
        assert!(!workspace.contains(&workspace.cwd().join("dir/new.txt")));
        assert!(!dir.join("outside/new.txt").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn execution_controlling_files() {
        assert!(controls_execution(Path::new("/w/.git/hooks/pre-commit")));
        assert!(controls_execution(Path::new("/w/.mcp.json")));
        assert!(controls_execution(Path::new("/w/.vscode/settings.json")));
        assert!(!controls_execution(Path::new("/w/src/main.rs")));
        assert!(!controls_execution(Path::new("/w/.git")));
    }
}

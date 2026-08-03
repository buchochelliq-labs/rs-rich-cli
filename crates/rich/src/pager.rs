//! Paging long output through the system pager.
//!
//! Port of `rich/pager.py` plus the pager-selection logic upstream inherits from
//! `pydoc.get_pager` (rich's `SystemPager` simply delegates to `pydoc.pager`).
//!
//! [`Console::page`](crate::console::Console::page) buffers everything printed
//! inside a closure and hands it to a [`Pager`] — the Rust analogue of upstream's
//! `with console.pager():` block, matching this crate's other capture-style
//! methods (`capture`, `export_text`, …).

use std::io::{IsTerminal, Write};
use std::process::{Command, Stdio};

/// Something that can display a block of content a screenful at a time. Mirrors
/// `rich.pager.Pager`.
pub trait Pager {
    /// Show `content`, returning an error only if the content could not be
    /// displayed at all.
    fn show(&self, content: &str) -> std::io::Result<()>;
}

/// Pages through the pager program installed on the system. Mirrors
/// `rich.pager.SystemPager` (which defers to `pydoc.pager`).
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemPager;

/// Write `content` straight to stdout — `pydoc`'s `plain_pager`, used when
/// there's no terminal to page in (piped/redirected output, `TERM=dumb`) or when
/// no pager program could be started.
fn plain(content: &str) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    handle.write_all(content.as_bytes())?;
    if !content.ends_with('\n') {
        handle.write_all(b"\n")?;
    }
    handle.flush()
}

/// The pager command to run, as `(program, args)`. Port of `pydoc.get_pager`'s
/// selection order: `MANPAGER`, then `PAGER`, then a platform default.
fn pager_command() -> Option<(String, Vec<String>)> {
    // An explicit pager wins. It's a command *line*, so split off any arguments
    // (e.g. `PAGER="less -R"`), matching pydoc handing the string to a shell.
    for variable in ["MANPAGER", "PAGER"] {
        if let Ok(value) = std::env::var(variable) {
            let mut parts = value.split_whitespace().map(str::to_string);
            if let Some(program) = parts.next() {
                return Some((program, parts.collect()));
            }
        }
    }
    if cfg!(windows) {
        Some(("more".to_string(), Vec::new()))
    } else {
        // `less -R` keeps ANSI styling readable; pydoc tries `pager` then `less`.
        Some(("less".to_string(), vec!["-R".to_string()]))
    }
}

/// Whether we're attached to a terminal that can host a pager. Port of
/// `pydoc.get_pager`'s isatty + `TERM in (dumb, emacs)` guards.
fn can_page() -> bool {
    if !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
        return false;
    }
    !matches!(
        std::env::var("TERM").unwrap_or_default().as_str(),
        "dumb" | "emacs"
    )
}

impl Pager for SystemPager {
    fn show(&self, content: &str) -> std::io::Result<()> {
        if !can_page() {
            return plain(content);
        }
        let Some((program, args)) = pager_command() else {
            return plain(content);
        };
        // Spawn the pager with our content on its stdin, inheriting stdout/stderr
        // so it can drive the terminal. Any failure (no such program, broken
        // pipe from the user quitting early) falls back to plain output.
        let child = Command::new(&program)
            .args(&args)
            .stdin(Stdio::piped())
            .spawn();
        let mut child = match child {
            Ok(child) => child,
            Err(_) => return plain(content),
        };
        if let Some(mut stdin) = child.stdin.take() {
            // A pager the user quits early closes the pipe; that's not an error.
            let _ = stdin.write_all(content.as_bytes());
            drop(stdin);
        }
        child.wait()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A test pager that records what it was asked to show.
    #[derive(Default)]
    struct RecordingPager {
        shown: Mutex<Vec<String>>,
    }

    impl Pager for RecordingPager {
        fn show(&self, content: &str) -> std::io::Result<()> {
            self.shown.lock().unwrap().push(content.to_string());
            Ok(())
        }
    }

    #[test]
    fn custom_pager_receives_content() {
        let pager = RecordingPager::default();
        pager.show("hello").unwrap();
        assert_eq!(
            pager.shown.lock().unwrap().as_slice(),
            &["hello".to_string()]
        );
    }

    #[test]
    fn explicit_pager_env_var_wins_and_splits_args() {
        // Serialised via the env guard below; MANPAGER takes precedence over PAGER.
        let _guard = EnvGuard::set(&[("MANPAGER", Some("myp --opt")), ("PAGER", Some("other"))]);
        let (program, args) = pager_command().expect("a pager command");
        assert_eq!(program, "myp");
        assert_eq!(args, vec!["--opt".to_string()]);
    }

    #[test]
    fn falls_back_to_a_platform_default() {
        let _guard = EnvGuard::set(&[("MANPAGER", None), ("PAGER", None)]);
        let (program, _) = pager_command().expect("a pager command");
        assert_eq!(program, if cfg!(windows) { "more" } else { "less" });
    }

    /// Set/restore env vars around a test. The two env tests share a lock so
    /// they can't interleave (tests run in parallel threads).
    struct EnvGuard {
        previous: Vec<(String, Option<String>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    impl EnvGuard {
        fn set(vars: &[(&str, Option<&str>)]) -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let previous = vars
                .iter()
                .map(|(key, _)| ((*key).to_string(), std::env::var(key).ok()))
                .collect();
            for (key, value) in vars {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            EnvGuard {
                previous,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.previous {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

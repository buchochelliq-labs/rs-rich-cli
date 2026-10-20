//! Which program opens a file: [Yazi](https://github.com/sxyazi/yazi)'s
//! `[opener]` and `[open]` tables as data. An [`Opener`] says how to open
//! (the editor, the system's opener, a folder view, a shell command, or
//! extracting an archive) and whether it takes the terminal; a [`Rule`]
//! picks openers by MIME type or name glob; [`Rules::matching`] gives the
//! openers for a file, the first one what Enter uses.
//!
//! Not a port of anything upstream: an rs-rich addition, with no
//! dependencies. It builds [`Command`]s and runs nothing itself, so it
//! needs no TUI; intuiTUIve's `files` components run a blocking opener
//! through a handoff of the terminal and the others in the background.
//!
//! ```
//! use std::path::Path;
//! use rich_ext::open::{Launch, Rules};
//!
//! let rules = Rules::yazi();
//! let openers = rules.matching(Path::new("notes.md"), "text/markdown");
//! assert_eq!(openers[0].launch, Launch::Editor);
//! assert!(openers[0].block);
//! let openers = rules.matching(Path::new("photo.jpg"), "image/jpeg");
//! assert_eq!(openers[0].launch, Launch::System);
//! ```
//!
//! The commands: [`editor`] is `$EDITOR` (else `vi`, Notepad on Windows),
//! [`system_opener`] `$OPENER` (else `xdg-open`, `open` on macOS, `cmd /C
//! start` on Windows). A [`Launch::Shell`] command runs as Yazi's do, in
//! `sh -c` with the files as `"$@"` (`cmd /C` with `%*` on Windows).

use std::path::{Path, PathBuf};
use std::process::Command;

/// How an [`Opener`] opens files.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Launch {
    /// `$EDITOR` with the files.
    Editor,
    /// The system's opener, once for each file.
    System,
    /// The system's opener on each file's directory.
    Reveal,
    /// A shell command, the files as `"$@"` (`%*` on Windows): Yazi's
    /// `run`, such as `mpv --force-window "$@"`.
    Shell(String),
    /// Extract each archive beside itself. No program: the caller does it
    /// (rs-rich-ext's `archive` module, behind its `archive` feature).
    Extract,
}

/// A way to open files: Yazi's `[opener]` entries.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Opener {
    /// The name rules use for it (`edit`, `open`, `reveal`, …).
    pub name: String,
    /// What a picker shows (`$EDITOR`, `Open`, `Reveal`).
    pub desc: String,
    pub launch: Launch,
    /// It takes the terminal until it ends (an editor, a pager), rather
    /// than running in the background.
    pub block: bool,
}

impl Opener {
    /// An opener called `name`, shown as `desc`, in the background.
    pub fn new(name: impl Into<String>, desc: impl Into<String>, launch: Launch) -> Opener {
        Opener {
            name: name.into(),
            desc: desc.into(),
            launch,
            block: false,
        }
    }

    /// Taking the terminal (or not).
    #[must_use]
    pub fn block(mut self, on: bool) -> Opener {
        self.block = on;
        self
    }

    /// The commands that open `paths` this way: one for the editor or a
    /// shell command, one a file for the system opener and reveal, none
    /// for [`Launch::Extract`]. None either when `paths` is empty.
    pub fn commands(&self, paths: &[PathBuf]) -> Vec<Command> {
        if paths.is_empty() {
            return Vec::new();
        }
        match &self.launch {
            Launch::Editor => {
                let mut command = editor();
                command.args(paths);
                vec![command]
            }
            Launch::System => paths
                .iter()
                .map(|path| {
                    let mut command = system_opener();
                    command.arg(path);
                    command
                })
                .collect(),
            Launch::Reveal => {
                let mut dirs: Vec<PathBuf> = paths
                    .iter()
                    .map(|p| p.parent().map_or(p.clone(), Path::to_path_buf))
                    .collect();
                dirs.dedup();
                dirs.into_iter()
                    .map(|dir| {
                        let mut command = system_opener();
                        command.arg(dir);
                        command
                    })
                    .collect()
            }
            Launch::Shell(run) => vec![shell(run, paths)],
            Launch::Extract => Vec::new(),
        }
    }
}

/// `run` in the platform's shell with `paths` as its arguments.
fn shell(run: &str, paths: &[PathBuf]) -> Command {
    if cfg!(windows) {
        let quoted: Vec<String> = paths
            .iter()
            .map(|p| format!("\"{}\"", p.display()))
            .collect();
        let mut command = Command::new("cmd");
        command.arg("/C").arg(run.replace("%*", &quoted.join(" ")));
        command
    } else {
        let mut command = Command::new("sh");
        // `$0` is the command's name; the files are `$1…`, `"$@"`.
        command.arg("-c").arg(run).arg("sh").args(paths);
        command
    }
}

/// A rule of Yazi's `[open]` table: files whose MIME type matches `mime`
/// (a glob: `text/*`, `{audio,video}/*`) and whose name matches `name`
/// (a glob; a directory's name ends in `/`, so `*/` is any directory) are
/// opened with the openers called `openers`, in order. A rule without a
/// `mime` or `name` matches any.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Rule {
    pub mime: Option<String>,
    pub name: Option<String>,
    pub openers: Vec<String>,
}

impl Rule {
    /// Files whose MIME type matches `glob`.
    pub fn mime(glob: impl Into<String>, openers: &[&str]) -> Rule {
        Rule {
            mime: Some(glob.into()),
            name: None,
            openers: openers.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Files whose name matches `glob`.
    pub fn name(glob: impl Into<String>, openers: &[&str]) -> Rule {
        Rule {
            mime: None,
            name: Some(glob.into()),
            openers: openers.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// Whether the rule takes a file called `name` (ending in `/` for a
    /// directory) of type `mime`.
    pub fn matches(&self, name: &str, mime: &str) -> bool {
        self.mime.as_deref().is_none_or(|g| glob_match(g, mime))
            && self.name.as_deref().is_none_or(|g| glob_match(g, name))
    }
}

/// Openers and the rules that pick them. The first rule that matches a
/// file decides; [`Rules::prepend`] puts a rule of your own before the
/// defaults, as Yazi's `prepend_rules`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rules {
    pub openers: Vec<Opener>,
    pub rules: Vec<Rule>,
}

/// The MIME types of archives Yazi's `extract` opener takes.
const ARCHIVES: &str = "application/{zip,gzip,x-tar,x-bzip2,x-xz,zstd,x-7z-compressed,vnd.rar,x-lzip,vnd.ms-cab-compressed}";

impl Rules {
    /// No openers and no rules.
    pub fn new() -> Rules {
        Rules::default()
    }

    /// Yazi's defaults: `edit` (`$EDITOR`, blocking), `open` and `play`
    /// (the system's opener), `reveal` (the file's folder) and `extract`;
    /// directories, text, JSON, JavaScript and empty files go to the
    /// editor first, media to `play`, archives to `extract`, anything else
    /// to `open`, each with `reveal` after it.
    pub fn yazi() -> Rules {
        Rules::new()
            .opener(Opener::new("edit", "$EDITOR", Launch::Editor).block(true))
            .opener(Opener::new("open", "Open", Launch::System))
            .opener(Opener::new("play", "Play", Launch::System))
            .opener(Opener::new("reveal", "Reveal", Launch::Reveal))
            .opener(Opener::new("extract", "Extract here", Launch::Extract))
            .rule(Rule::name("*/", &["edit", "open", "reveal"]))
            .rule(Rule::mime("text/*", &["edit", "reveal"]))
            .rule(Rule::mime("image/*", &["open", "reveal"]))
            .rule(Rule::mime("{audio,video}/*", &["play", "reveal"]))
            .rule(Rule::mime(ARCHIVES, &["extract", "reveal"]))
            .rule(Rule::mime(
                "application/{json,x-ndjson,toml,yaml,xml}",
                &["edit", "reveal"],
            ))
            .rule(Rule::mime("*/javascript", &["edit", "reveal"]))
            .rule(Rule::mime("inode/x-empty", &["edit", "reveal"]))
            .rule(Rule::name("*", &["open", "reveal"]))
    }

    /// Add an opener (replacing one of the same name).
    #[must_use]
    pub fn opener(mut self, opener: Opener) -> Rules {
        self.openers.retain(|o| o.name != opener.name);
        self.openers.push(opener);
        self
    }

    /// Add a rule after the others.
    #[must_use]
    pub fn rule(mut self, rule: Rule) -> Rules {
        self.rules.push(rule);
        self
    }

    /// Add a rule before the others.
    #[must_use]
    pub fn prepend(mut self, rule: Rule) -> Rules {
        self.rules.insert(0, rule);
        self
    }

    /// The opener called `name`.
    pub fn get(&self, name: &str) -> Option<&Opener> {
        self.openers.iter().find(|o| o.name == name)
    }

    /// The openers for the file at `path` of type `mime` (pass
    /// `inode/directory` for a directory): the first matching rule's, in
    /// its order, the names it uses that are not openers left out.
    pub fn matching(&self, path: &Path, mime: &str) -> Vec<&Opener> {
        let mut name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        if mime == "inode/directory" {
            name.push('/');
        }
        self.rules
            .iter()
            .find(|rule| rule.matches(&name, mime))
            .map(|rule| rule.openers.iter().filter_map(|n| self.get(n)).collect())
            .unwrap_or_default()
    }

    /// The openers every one of `files` (path and MIME type) can use, in
    /// the first one's order: what "Open with…" offers for a selection.
    pub fn shared(&self, files: &[(PathBuf, &str)]) -> Vec<&Opener> {
        let mut shared: Option<Vec<&Opener>> = None;
        for (path, mime) in files {
            let these = self.matching(path, mime);
            shared = Some(match shared {
                None => these,
                Some(before) => before.into_iter().filter(|o| these.contains(o)).collect(),
            });
        }
        shared.unwrap_or_default()
    }
}

/// Whether `text` matches the glob `pattern`: `*` any run of characters,
/// `?` one, `{a,b}` either (nesting allowed), anything else itself.
///
/// ```
/// use rich_ext::open::glob_match;
///
/// assert!(glob_match("text/*", "text/x-rust"));
/// assert!(glob_match("{audio,video}/*", "video/mp4"));
/// assert!(glob_match("*.tar.?z", "a.tar.gz"));
/// assert!(!glob_match("image/*", "text/plain"));
/// ```
pub fn glob_match(pattern: &str, text: &str) -> bool {
    expand(pattern).iter().any(|p| {
        wildcard(
            &p.chars().collect::<Vec<_>>(),
            &text.chars().collect::<Vec<_>>(),
        )
    })
}

/// The patterns `{a,b}` alternatives stand for.
fn expand(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_string()];
    };
    // The matching close brace, and the commas at this level.
    let mut depth = 0;
    let mut commas = Vec::new();
    let mut close = None;
    for (i, c) in pattern[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(open + i);
                    break;
                }
            }
            ',' if depth == 1 => commas.push(open + i),
            _ => {}
        }
    }
    let Some(close) = close else {
        return vec![pattern.to_string()];
    };
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    let mut bounds = vec![open];
    bounds.extend(commas);
    bounds.push(close);
    bounds
        .windows(2)
        .flat_map(|w| expand(&format!("{head}{}{tail}", &pattern[w[0] + 1..w[1]])))
        .collect()
}

fn wildcard(pattern: &[char], text: &[char]) -> bool {
    // Greedy with backtracking to the last `*`.
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, t));
                p += 1;
            }
            Some('?') => {
                p += 1;
                t += 1;
            }
            Some(&c) if c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// The program and arguments in `$VAR`, split at spaces, else `fallback`.
fn command_from(var: &str, fallback: &str) -> Command {
    let line = std::env::var(var)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| fallback.to_string());
    let mut words = line.split_whitespace();
    let mut command = Command::new(words.next().unwrap_or(fallback));
    command.args(words);
    command
}

/// The system's opener: `$OPENER` (a program and its arguments), else
/// `xdg-open`, `open` on macOS, `cmd /C start ""` on Windows. Add the file.
pub fn system_opener() -> Command {
    if cfg!(windows) {
        let mut command = command_from("OPENER", "cmd");
        if std::env::var_os("OPENER").is_none() {
            command.args(["/C", "start", ""]);
        }
        command
    } else if cfg!(target_os = "macos") {
        command_from("OPENER", "open")
    } else {
        command_from("OPENER", "xdg-open")
    }
}

/// `$EDITOR` (a program and its arguments), else `vi` (Notepad on
/// Windows). Add the files.
pub fn editor() -> Command {
    command_from("EDITOR", if cfg!(windows) { "notepad" } else { "vi" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(openers: &[&Opener]) -> Vec<String> {
        openers.iter().map(|o| o.name.clone()).collect()
    }

    #[test]
    fn yazis_rules_pick_by_type_and_name() {
        let rules = Rules::yazi();
        let pick = |name: &str, mime: &str| names(&rules.matching(Path::new(name), mime));
        assert_eq!(pick("src", "inode/directory"), ["edit", "open", "reveal"]);
        assert_eq!(pick("a.rs", "text/x-rust"), ["edit", "reveal"]);
        assert_eq!(pick("a.mp3", "audio/mpeg"), ["play", "reveal"]);
        assert_eq!(pick("a.tar", "application/x-tar"), ["extract", "reveal"]);
        assert_eq!(pick("a.json", "application/json"), ["edit", "reveal"]);
        assert_eq!(pick("empty", "inode/x-empty"), ["edit", "reveal"]);
        assert_eq!(pick("a.pdf", "application/pdf"), ["open", "reveal"]);
    }

    #[test]
    fn own_rules_come_first_and_selections_share() {
        let rules = Rules::yazi()
            .opener(Opener::new("view", "View", Launch::Shell("less \"$@\"".into())).block(true))
            .prepend(Rule::name("*.log", &["view", "edit"]));
        assert_eq!(
            names(&rules.matching(Path::new("x.log"), "text/plain")),
            ["view", "edit"]
        );
        let shared = rules.shared(&[
            (PathBuf::from("a.txt"), "text/plain"),
            (PathBuf::from("b.png"), "image/png"),
        ]);
        assert_eq!(names(&shared), ["reveal"]);
    }

    #[test]
    fn commands_for_each_launch() {
        let paths = [PathBuf::from("/tmp/a b.txt"), PathBuf::from("/tmp/c.txt")];
        let rules = Rules::yazi();
        assert_eq!(rules.get("edit").unwrap().commands(&paths).len(), 1);
        assert_eq!(rules.get("open").unwrap().commands(&paths).len(), 2);
        // Both files are in /tmp: one folder to show.
        assert_eq!(rules.get("reveal").unwrap().commands(&paths).len(), 1);
        assert!(rules.get("extract").unwrap().commands(&paths).is_empty());
        #[cfg(unix)]
        {
            let shell = Opener::new("count", "Count", Launch::Shell("echo $# \"$@\"".into()));
            let out = shell.commands(&paths)[0].output().unwrap();
            assert_eq!(
                String::from_utf8_lossy(&out.stdout).trim(),
                "2 /tmp/a b.txt /tmp/c.txt"
            );
        }
    }

    #[test]
    fn globs() {
        assert!(glob_match("*", ""));
        assert!(glob_match("*/", "dir/"));
        assert!(!glob_match("*/", "file"));
        assert!(glob_match("a{b,{c,d}}e", "ade"));
        assert!(glob_match("*.{tar.gz,tgz}", "x.tgz"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*a*b", "xxaxxb"));
    }
}

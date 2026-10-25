//! A git client in the style of [gitui](https://github.com/extrawurst/gitui),
//! rebuilt on intuiTUIve: its tabs (Status, Log, Files, Stashing and
//! Stashes), the unstaged and staged file trees beside the diff of the
//! selected file, staging and resetting by file, hunk and line, the commit
//! popup (with `$EDITOR` handed the terminal), the commit log with its
//! branch and tag markers, a commit's details and diff, the branch list,
//! the stashes, the key bar at the bottom and the help.
//!
//! Ported from gitui (<https://github.com/extrawurst/gitui>), by Stephan
//! Dilly and its contributors, MIT licence. Its layout, keys, strings and
//! colours are theirs; this rebuild reuses none of its code. gitui reads
//! the repository through libgit2 on worker threads; this one runs the
//! `git` on the `PATH` (`std::process::Command`) and parses its output,
//! the diffs with rs-rich-ext's `diff::git::parse_unified`.
//!
//!     cargo run -p rs-rich-intuituive --example git [-- DIR]
//!
//! gitui's default keys: 1-5 (or Tab and Shift+Tab) switch tabs, h shows
//! the help, . expands the key bar, q quits. Status: ↑/↓ move, Enter stages (or unstages) the
//! selected file or folder, a stages everything (unstages, in the staged
//! list), D resets a file (asking first), w moves between the unstaged and
//! staged lists, → (or l) moves to the diff and ← back, c commits, b lists
//! the branches. The diff: ↑/↓ move the line cursor, Shift+↑/↓ select
//! lines, n/p the next and previous hunk, Enter stages (unstages) the hunk,
//! D resets it, s stages (unstages) the selected lines, d resets them, y
//! copies them. The commit popup: Ctrl+D commits, Ctrl+A amends the last
//! commit, Ctrl+E opens `$EDITOR`, Esc closes it (the message is kept).
//! Log: Enter shows the commit's details, → inspects it (its files and
//! their diffs), y copies the hash. Files: the tree at HEAD and the
//! selected file's text. Branches: Enter checks out, c creates,
//! r renames, D deletes, m merges, ←/→ local and remote. Stashing: s
//! stashes, u and i toggle its options. Stashes: a applies, p pops, D
//! drops, → (or Enter) inspects.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use intuituive::prelude::*;
use intuituive::reactive::NodeId;
use intuituive::rich::markup::escape;
use intuituive::widgets::{
    key_hints_with, list_rows, tabs, tree_keyed, KeyHintsOptions, ListRow, TreeItem,
};
use intuituive::{facts_of, keyed, HelpOptions, ModalOptions, Offset, Origin, PromptOptions};
use rich_ext::diff::git::{parse_unified, FilePatch, LineKind, Patch};

// ---------------------------------------------------------------------
// The repository, through the git command line.
// ---------------------------------------------------------------------

/// A repository, read and changed by running `git` in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Git {
    pub root: PathBuf,
}

/// How a file changed, as gitui marks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Modified,
    New,
    Deleted,
    Renamed,
    Conflicted,
    Typechange,
    /// A file shown as it is (the Files tab): no letter.
    Unchanged,
}

impl Kind {
    fn of(code: char) -> Option<Kind> {
        Some(match code {
            'M' => Kind::Modified,
            'A' => Kind::New,
            'D' => Kind::Deleted,
            'R' | 'C' => Kind::Renamed,
            'T' => Kind::Typechange,
            _ => return None,
        })
    }

    /// gitui's status letter.
    pub fn letter(self) -> char {
        match self {
            Kind::Modified => 'M',
            Kind::New => '+',
            Kind::Deleted => '-',
            Kind::Renamed => 'R',
            Kind::Conflicted => '!',
            Kind::Typechange | Kind::Unchanged => ' ',
        }
    }

    /// gitui's default theme.
    fn style(self) -> &'static str {
        match self {
            Kind::Modified => "yellow",
            Kind::New => "bright_green",
            Kind::Deleted => "bright_red",
            Kind::Renamed => "bright_magenta",
            Kind::Conflicted => "bold yellow",
            Kind::Typechange | Kind::Unchanged => "default",
        }
    }
}

/// A changed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub kind: Kind,
    pub untracked: bool,
}

/// `git status`: the working tree's changes and the index's.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub unstaged: Vec<FileEntry>,
    pub staged: Vec<FileEntry>,
}

/// A commit (or a stash) as the log lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub short: String,
    pub time: i64,
    pub author: String,
    pub subject: String,
}

/// A ref pointing at a commit: `{local}`, `[remote]` or `<tag>` in the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    Local(String),
    Remote(String),
    Tag(String),
}

/// A commit's details.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub author: String,
    pub committer: String,
    pub time: i64,
    pub message: String,
    pub files: Vec<FileEntry>,
}

/// A branch, local or remote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub local: bool,
    pub head: bool,
    pub hash: String,
    pub subject: String,
    pub upstream: bool,
}

type GitResult<T> = Result<T, String>;

impl Git {
    /// The repository `dir` is in.
    pub fn open(dir: &Path) -> GitResult<Git> {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-parse", "--show-toplevel"])
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(Git { root: root.into() })
    }

    fn output(&self, args: &[&str], input: Option<&str>, ok: &[i32]) -> GitResult<String> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "color.ui=never", "-c", "core.quotepath=off"])
            .args(args)
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null());
        let out = match input {
            None => command.output(),
            Some(input) => (|| {
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                let mut child = command.spawn()?;
                child
                    .stdin
                    .take()
                    .expect("piped")
                    .write_all(input.as_bytes())?;
                child.wait_with_output()
            })(),
        }
        .map_err(|e| format!("git: {e}"))?;
        let code = out.status.code().unwrap_or(-1);
        if code == 0 || ok.contains(&code) {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let error = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if error.is_empty() {
            format!("git {} failed", args.join(" "))
        } else {
            error
        })
    }

    fn run(&self, args: &[&str]) -> GitResult<String> {
        self.output(args, None, &[])
    }

    /// `git status`, the untracked files one by one.
    pub fn status(&self) -> GitResult<Status> {
        let out = self.run(&["status", "--porcelain=v1", "-z", "--untracked-files=all"])?;
        let mut status = Status::default();
        let mut parts = out.split('\0');
        while let Some(entry) = parts.next() {
            let bytes = entry.as_bytes();
            if bytes.len() < 4 {
                continue;
            }
            let (x, y) = (bytes[0] as char, bytes[1] as char);
            let path = entry[3..].to_string();
            if x == 'R' || x == 'C' {
                parts.next(); // the path it came from
            }
            let file = |kind, untracked| FileEntry {
                path: path.clone(),
                kind,
                untracked,
            };
            match (x, y) {
                ('?', _) => status.unstaged.push(file(Kind::New, true)),
                ('!', _) => {}
                ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D') => {
                    status.unstaged.push(file(Kind::Conflicted, false))
                }
                _ => {
                    if let Some(kind) = Kind::of(y) {
                        status.unstaged.push(file(kind, false));
                    }
                    if let Some(kind) = Kind::of(x) {
                        status.staged.push(file(kind, false));
                    }
                }
            }
        }
        status.unstaged.sort_by(|a, b| a.path.cmp(&b.path));
        status.staged.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(status)
    }

    /// The diff of one file: the working tree's against the index, or the
    /// index's against HEAD.
    pub fn diff(&self, file: &FileEntry, staged: bool) -> GitResult<Option<FilePatch>> {
        let path = file.path.as_str();
        let text = if staged {
            self.run(&["diff", "--cached", "--no-ext-diff", "--", path])?
        } else if file.untracked {
            let args = [
                "diff",
                "--no-index",
                "--no-ext-diff",
                "--",
                "/dev/null",
                path,
            ];
            self.output(&args, None, &[1])?
        } else {
            self.run(&["diff", "--no-ext-diff", "--", path])?
        };
        Ok(parse(&text)?.files.into_iter().next())
    }

    /// Stage a file or a folder's files.
    pub fn stage(&self, paths: &[String]) -> GitResult<()> {
        self.with_paths(&["add", "-A", "--"], paths)
    }

    /// Unstage files: back to HEAD's, or out of the index before the first
    /// commit.
    pub fn unstage(&self, paths: &[String]) -> GitResult<()> {
        if self.head().is_some() {
            self.with_paths(&["reset", "-q", "--"], paths)
        } else {
            self.with_paths(&["rm", "--cached", "-r", "-q", "--"], paths)
        }
    }

    /// Throw away a file's changes in the working tree; an untracked file
    /// is deleted.
    pub fn reset(&self, files: &[FileEntry]) -> GitResult<()> {
        for file in files {
            if file.untracked {
                std::fs::remove_file(self.root.join(&file.path)).map_err(|e| e.to_string())?;
            } else {
                self.run(&["checkout", "-q", "--", &file.path])?;
            }
        }
        Ok(())
    }

    fn with_paths(&self, args: &[&str], paths: &[String]) -> GitResult<()> {
        let mut all: Vec<&str> = args.to_vec();
        all.extend(paths.iter().map(String::as_str));
        self.run(&all).map(drop)
    }

    /// `git apply` a patch to the index (`cached`) or the working tree,
    /// backwards when `reverse`.
    pub fn apply(&self, patch: &str, cached: bool, reverse: bool) -> GitResult<()> {
        let mut args = vec!["apply", "--recount", "--whitespace=nowarn"];
        if cached {
            args.push("--cached");
        }
        if reverse {
            args.push("--reverse");
        }
        args.push("-");
        self.output(&args, Some(patch), &[]).map(drop)
    }

    /// Commit the index, or amend HEAD with it.
    pub fn commit(&self, message: &str, amend: bool) -> GitResult<()> {
        let mut args = vec!["commit", "-q", "--cleanup=strip", "-F", "-"];
        if amend {
            args.push("--amend");
        }
        self.output(&args, Some(message), &[]).map(drop)
    }

    /// HEAD's commit, if there is one.
    pub fn head(&self) -> Option<String> {
        let out = self.run(&["rev-parse", "-q", "--verify", "HEAD"]).ok()?;
        Some(out.trim().to_string())
    }

    /// HEAD's full message.
    pub fn head_message(&self) -> String {
        self.run(&["log", "-1", "--format=%B"]).unwrap_or_default()
    }

    /// The branch checked out, or HEAD's short hash when detached.
    pub fn branch_name(&self) -> String {
        match self.run(&["symbolic-ref", "--short", "-q", "HEAD"]) {
            Ok(name) => name.trim().to_string(),
            Err(_) => self
                .run(&["rev-parse", "--short", "HEAD"])
                .map(|h| h.trim().to_string())
                .unwrap_or_default(),
        }
    }

    /// How far HEAD is ahead of its upstream and behind it.
    pub fn ahead_behind(&self) -> Option<(usize, usize)> {
        let out = self
            .run(&["rev-list", "--left-right", "--count", "HEAD...@{u}"])
            .ok()?;
        let mut counts = out.split_whitespace().map(|n| n.parse().unwrap_or(0));
        Some((counts.next()?, counts.next()?))
    }

    /// The newest `limit` commits from HEAD.
    pub fn log(&self, limit: usize) -> Vec<Commit> {
        let max = format!("--max-count={limit}");
        let format = "--format=%H%x1f%h%x1f%at%x1f%an%x1f%s%x1e";
        self.run(&["log", &max, format])
            .map(|out| commits(&out))
            .unwrap_or_default()
    }

    /// The refs pointing at each commit.
    pub fn refs(&self) -> HashMap<String, Vec<Ref>> {
        let mut refs: HashMap<String, Vec<Ref>> = HashMap::new();
        let format = "--format=%(objectname)%1f%(*objectname)%1f%(refname)";
        for line in self
            .run(&["for-each-ref", format])
            .unwrap_or_default()
            .lines()
        {
            let mut parts = line.split('\x1f');
            let (Some(object), Some(peeled), Some(name)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            let target = if peeled.is_empty() { object } else { peeled };
            let named = if let Some(n) = name.strip_prefix("refs/heads/") {
                Ref::Local(n.into())
            } else if let Some(n) = name.strip_prefix("refs/remotes/") {
                if n.ends_with("/HEAD") {
                    continue;
                }
                Ref::Remote(n.into())
            } else if let Some(n) = name.strip_prefix("refs/tags/") {
                Ref::Tag(n.into())
            } else {
                continue;
            };
            refs.entry(target.to_string()).or_default().push(named);
        }
        refs
    }

    /// A commit's author, committer, date, message and files.
    pub fn details(&self, hash: &str) -> Details {
        let format = "--format=%an <%ae>%x1f%cn <%ce>%x1f%at%x1f%B";
        let out = self.run(&["show", "-s", format, hash]).unwrap_or_default();
        let mut parts = out.splitn(4, '\x1f');
        let mut details = Details {
            author: parts.next().unwrap_or_default().to_string(),
            committer: parts.next().unwrap_or_default().to_string(),
            time: parts.next().and_then(|t| t.parse().ok()).unwrap_or(0),
            message: parts.next().unwrap_or_default().trim_end().to_string(),
            files: Vec::new(),
        };
        let names = self
            .run(&[
                "diff-tree",
                "--no-commit-id",
                "-r",
                "--root",
                "--name-status",
                "-z",
                hash,
            ])
            .unwrap_or_default();
        let mut parts = names.split('\0').filter(|p| !p.is_empty());
        while let Some(code) = parts.next() {
            let letter = code.chars().next().unwrap_or('M');
            if letter == 'R' || letter == 'C' {
                parts.next();
            }
            if let Some(path) = parts.next() {
                details.files.push(FileEntry {
                    path: path.to_string(),
                    kind: Kind::of(letter).unwrap_or(Kind::Modified),
                    untracked: false,
                });
            }
        }
        details
    }

    /// A commit's diff, file by file.
    pub fn commit_diff(&self, hash: &str) -> Patch {
        self.run(&[
            "diff-tree",
            "-p",
            "--root",
            "--no-commit-id",
            "--no-ext-diff",
            hash,
        ])
        .ok()
        .and_then(|text| parse(&text).ok())
        .unwrap_or_default()
    }

    /// The local and remote branches.
    pub fn branches(&self) -> Vec<Branch> {
        let format = "--format=%(HEAD)%1f%(refname)%1f%(objectname:short=8)%1f%(upstream)%1f%(contents:subject)";
        let out = self
            .run(&["for-each-ref", format, "refs/heads", "refs/remotes"])
            .unwrap_or_default();
        out.lines()
            .filter_map(|line| {
                let mut parts = line.splitn(5, '\x1f');
                let head = parts.next()? == "*";
                let refname = parts.next()?;
                let hash = parts.next()?.to_string();
                let upstream = !parts.next()?.is_empty();
                let subject = parts.next().unwrap_or_default().to_string();
                let (name, local) = if let Some(n) = refname.strip_prefix("refs/heads/") {
                    (n, true)
                } else {
                    (refname.strip_prefix("refs/remotes/")?, false)
                };
                (!name.ends_with("/HEAD")).then(|| Branch {
                    name: name.to_string(),
                    local,
                    head,
                    hash,
                    subject,
                    upstream,
                })
            })
            .collect()
    }

    /// Check a branch out; a remote one as a new local branch tracking it.
    pub fn checkout(&self, branch: &Branch) -> GitResult<()> {
        if branch.local {
            return self.run(&["checkout", "-q", &branch.name]).map(drop);
        }
        self.run(&["checkout", "-q", "--track", &branch.name])
            .map(drop)
    }

    pub fn create_branch(&self, name: &str) -> GitResult<()> {
        self.run(&["checkout", "-q", "-b", name]).map(drop)
    }

    pub fn rename_branch(&self, from: &str, to: &str) -> GitResult<()> {
        self.run(&["branch", "-m", from, to]).map(drop)
    }

    pub fn delete_branch(&self, name: &str) -> GitResult<()> {
        self.run(&["branch", "-D", name]).map(drop)
    }

    pub fn merge(&self, name: &str) -> GitResult<()> {
        self.run(&["merge", "--no-edit", name]).map(drop)
    }

    /// The stashes, newest first.
    pub fn stashes(&self) -> Vec<Commit> {
        let format = "--format=%H%x1f%h%x1f%at%x1f%an%x1f%gs%x1e";
        self.run(&["stash", "list", format])
            .map(|out| commits(&out))
            .unwrap_or_default()
    }

    /// `git stash push`, with gitui's two options.
    pub fn stash(&self, message: &str, untracked: bool, keep_index: bool) -> GitResult<()> {
        let mut args = vec!["stash", "push", "-q"];
        if untracked {
            args.push("--include-untracked");
        }
        if keep_index {
            args.push("--keep-index");
        }
        if !message.is_empty() {
            args.extend(["-m", message]);
        }
        self.run(&args).map(drop)
    }

    /// `git stash apply`, `pop` or `drop` of the stash at `index`.
    pub fn stash_do(&self, what: &str, index: usize) -> GitResult<()> {
        let name = format!("stash@{{{index}}}");
        self.run(&["stash", what, "-q", &name]).map(drop)
    }

    /// The files at HEAD.
    pub fn files(&self) -> Vec<String> {
        self.run(&["ls-tree", "-r", "-z", "--name-only", "HEAD"])
            .map(|out| {
                out.split('\0')
                    .filter(|p| !p.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// A file's text at HEAD.
    pub fn show_file(&self, path: &str) -> String {
        self.run(&["show", &format!("HEAD:{path}")])
            .unwrap_or_default()
    }

    /// The editor `git commit` would start.
    pub fn editor(&self) -> String {
        if let Ok(editor) = std::env::var("GIT_EDITOR") {
            return editor;
        }
        if let Ok(editor) = self.run(&["config", "core.editor"]) {
            if !editor.trim().is_empty() {
                return editor.trim().to_string();
            }
        }
        ["VISUAL", "EDITOR"]
            .iter()
            .find_map(|name| std::env::var(name).ok())
            .unwrap_or_else(|| "vi".into())
    }

    /// Where the commit message is written for the editor.
    pub fn message_file(&self) -> PathBuf {
        let dir = self
            .run(&["rev-parse", "--absolute-git-dir"])
            .map(|d| PathBuf::from(d.trim()))
            .unwrap_or_else(|_| self.root.join(".git"));
        dir.join("COMMIT_EDITMSG")
    }
}

fn parse(text: &str) -> GitResult<Patch> {
    parse_unified(text).map_err(|e| format!("{e:?}"))
}

fn commits(out: &str) -> Vec<Commit> {
    out.split('\x1e')
        .filter_map(|record| {
            let mut parts = record.trim_start_matches('\n').splitn(5, '\x1f');
            Some(Commit {
                hash: parts.next().filter(|h| !h.is_empty())?.to_string(),
                short: parts.next()?.to_string(),
                time: parts.next()?.parse().unwrap_or(0),
                author: parts.next()?.to_string(),
                subject: parts.next()?.to_string(),
            })
        })
        .collect()
}

/// A patch of one hunk with only the lines `selected` (by index in the
/// hunk) changed, for `git apply`. Applied forwards, a change left out is
/// dropped if it adds and kept as context if it removes; applied in
/// `reverse` (unstaging, resetting), the other way round. `None` when no
/// change is selected.
pub fn hunk_patch(
    file: &FilePatch,
    hunk: usize,
    selected: impl Fn(usize) -> bool,
    reverse: bool,
) -> Option<String> {
    let hunk = file.hunks.get(hunk)?;
    let (mut body, mut old, mut new, mut changes) = (String::new(), 0, 0, 0);
    for (i, line) in hunk.lines.iter().enumerate() {
        let picked = selected(i);
        let mark = match (line.kind, picked, reverse) {
            (LineKind::Context, ..) => ' ',
            (LineKind::Added, true, _) | (LineKind::Removed, true, _) => {
                changes += 1;
                if line.kind == LineKind::Added {
                    '+'
                } else {
                    '-'
                }
            }
            (LineKind::Added, false, false) | (LineKind::Removed, false, true) => continue,
            (LineKind::Added, false, true) | (LineKind::Removed, false, false) => ' ',
        };
        if mark != '+' {
            old += 1;
        }
        if mark != '-' {
            new += 1;
        }
        body.push(mark);
        body.push_str(&line.text);
        body.push('\n');
        if line.no_newline {
            body.push_str("\\ No newline at end of file\n");
        }
    }
    if changes == 0 {
        return None;
    }
    let side = |path: &Option<String>, prefix: &str| match path {
        Some(p) => format!("{prefix}{p}"),
        None => "/dev/null".into(),
    };
    let name = file.path();
    Some(format!(
        "diff --git a/{name} b/{name}\n--- {}\n+++ {}\n@@ -{},{old} +{},{new} @@\n{body}",
        side(&file.old_path, "a/"),
        side(&file.new_path, "b/"),
        hunk.old_start,
        hunk.new_start,
    ))
}

// ---------------------------------------------------------------------
// The file trees: a folder per directory, single-child chains folded.
// ---------------------------------------------------------------------

/// A status list as a tree, and what each row stands for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileTree {
    pub items: Vec<TreeItem>,
    /// Each row's key (its path; a folded folder's `a/b/c`), the file it
    /// is (if a file) and the files under it, in the order shown with
    /// every folder open.
    pub rows: Vec<(String, Option<usize>, Vec<usize>)>,
}

impl FileTree {
    pub fn of(files: &[FileEntry]) -> FileTree {
        enum Child {
            Dir(String, Vec<Child>),
            File(usize),
        }
        fn insert(children: &mut Vec<Child>, parts: &[&str], file: usize) {
            if parts.len() == 1 {
                children.push(Child::File(file));
                return;
            }
            let found = children
                .iter()
                .position(|c| matches!(c, Child::Dir(name, _) if name == parts[0]));
            let at = found.unwrap_or_else(|| {
                children.push(Child::Dir(parts[0].to_string(), Vec::new()));
                children.len() - 1
            });
            if let Child::Dir(_, inner) = &mut children[at] {
                insert(inner, &parts[1..], file);
            }
        }
        fn fold(child: Child) -> Child {
            match child {
                Child::Dir(name, mut inner) => {
                    if inner.len() == 1 && matches!(inner[0], Child::Dir(..)) {
                        if let Some(Child::Dir(next, rest)) = inner.pop() {
                            return fold(Child::Dir(format!("{name}/{next}"), rest));
                        }
                    }
                    Child::Dir(name, inner.into_iter().map(fold).collect())
                }
                file => file,
            }
        }
        // gitui's status letter goes first on a file's row, before the
        // indent; a folder's row starts with as many spaces.
        let lettered = files.iter().any(|f| f.kind != Kind::Unchanged);
        fn walk(
            children: Vec<Child>,
            files: &[FileEntry],
            at: &str,
            lettered: bool,
            rows: &mut Vec<(String, Option<usize>, Vec<usize>)>,
        ) -> (Vec<TreeItem>, Vec<usize>) {
            let (mut items, mut all) = (Vec::new(), Vec::new());
            for child in children {
                match child {
                    Child::File(f) => {
                        let file = &files[f];
                        let name = file.path.rsplit('/').next().unwrap_or(&file.path);
                        let style = file.kind.style();
                        let item = TreeItem::keyed(
                            file.path.clone(),
                            format!("[{style}]{}[/]", escape(name)),
                        );
                        items.push(match lettered {
                            true => item.prefix(format!("[{style}]{}[/] ", file.kind.letter())),
                            false => item,
                        });
                        rows.push((file.path.clone(), Some(f), vec![f]));
                        all.push(f);
                    }
                    Child::Dir(name, inner) => {
                        let key = match at {
                            "" => name.clone(),
                            at => format!("{at}/{name}"),
                        };
                        let row = rows.len();
                        rows.push((key.clone(), None, Vec::new()));
                        let (kids, under) = walk(inner, files, &key, lettered, rows);
                        rows[row].2 = under.clone();
                        all.extend(under);
                        let item = TreeItem::keyed(key, escape(&name)).children(kids);
                        items.push(if lettered { item.prefix("  ") } else { item });
                    }
                }
            }
            (items, all)
        }
        let mut top = Vec::new();
        for (i, file) in files.iter().enumerate() {
            let parts: Vec<&str> = file.path.split('/').collect();
            insert(&mut top, &parts, i);
        }
        let top: Vec<Child> = top.into_iter().map(fold).collect();
        let mut rows = Vec::new();
        let (items, _) = walk(top, files, "", lettered, &mut rows);
        FileTree { items, rows }
    }

    /// Every folder's key.
    pub fn folders(&self) -> HashSet<String> {
        (self.rows.iter())
            .filter(|(_, file, _)| file.is_none())
            .map(|(key, ..)| key.clone())
            .collect()
    }

    fn row(&self, key: &str) -> Option<usize> {
        self.rows.iter().position(|(k, ..)| k == key)
    }
}

/// A file tree over a list of files, its selection and open folders kept
/// by path (`tree_keyed`) as the files change, each new folder opened:
/// gitui's status tree.
#[derive(Clone, Copy)]
struct Files {
    entries: Memo<Vec<FileEntry>>,
    tree: Memo<FileTree>,
    selected: Signal<Option<String>>,
    expanded: Signal<HashSet<String>>,
}

impl Files {
    fn new(entries: impl Fn() -> Vec<FileEntry> + 'static) -> Files {
        let entries = memo(entries);
        let tree = memo(move || entries.with(|e| FileTree::of(e)));
        let expanded = signal(HashSet::new());
        // Folders open when they first show; one the user closed stays so.
        let seen = std::rc::Rc::new(std::cell::RefCell::new(HashSet::new()));
        watch(
            move || tree.with(FileTree::folders),
            move |folders, _| {
                let mut seen = seen.borrow_mut();
                let new: Vec<String> = folders.difference(&seen).cloned().collect();
                if !new.is_empty() {
                    expanded.update(|open| open.extend(new));
                }
                *seen = folders;
            },
        );
        Files {
            entries,
            tree,
            selected: signal(None),
            expanded,
        }
    }

    /// The row the selection names.
    fn row(&self) -> Option<usize> {
        let key = self.selected.get()?;
        self.tree.with(|t| t.row(&key))
    }

    /// The files the selected row stands for: one file, or a folder's.
    fn targets(&self) -> Vec<FileEntry> {
        let under = self.row().map(|r| self.tree.with(|t| t.rows[r].2.clone()));
        let under = under.unwrap_or_default();
        self.entries
            .with(|e| under.iter().filter_map(|&i| e.get(i).cloned()).collect())
    }

    /// The selected file, if the selection is a file.
    fn file(&self) -> Option<FileEntry> {
        let file = self.row().and_then(|r| self.tree.with(|t| t.rows[r].1));
        file.and_then(|i| self.entries.with(|e| e.get(i).cloned()))
    }

    fn view(&self) -> Node {
        let tree = self.tree;
        tree_keyed(
            move || tree.with(|t| t.items.clone()),
            self.selected,
            self.expanded,
        )
    }
}

// ---------------------------------------------------------------------
// The diff view: gitui's hunk markers, a line cursor and a selection.
// ---------------------------------------------------------------------

/// A row of the diff.
#[derive(Clone, Debug, PartialEq)]
struct DiffRow {
    markup: String,
    /// The hunk it is in, and its line in the hunk (`None` on the header).
    hunk: Option<usize>,
    line: Option<usize>,
    end: bool,
}

fn diff_rows(patch: &Option<FilePatch>) -> Vec<DiffRow> {
    let Some(file) = patch else {
        return Vec::new();
    };
    let mut rows = Vec::new();
    if file.binary {
        rows.push(DiffRow {
            markup: "[dim]binary file[/]".into(),
            hunk: None,
            line: None,
            end: false,
        });
    }
    for (h, hunk) in file.hunks.iter().enumerate() {
        rows.push(DiffRow {
            markup: format!("[bold bright_black]{}[/]", escape(&hunk.header())),
            hunk: Some(h),
            line: None,
            end: false,
        });
        let last = hunk.lines.len().saturating_sub(1);
        for (l, line) in hunk.lines.iter().enumerate() {
            let text = escape(&line.text.replace('\t', "  "));
            let markup = match line.kind {
                LineKind::Added => format!("[green]+{text}[/]"),
                LineKind::Removed => format!("[red]-{text}[/]"),
                LineKind::Context => format!(" {text}"),
            };
            rows.push(DiffRow {
                markup,
                hunk: Some(h),
                line: Some(l),
                end: l == last,
            });
        }
    }
    rows
}

/// What a diff view shows and where its cursor is.
#[derive(Clone, Copy)]
struct Diff {
    patch: Signal<Option<FilePatch>>,
    rows: Memo<Vec<DiffRow>>,
    cursor: Signal<usize>,
    /// Where a Shift+↑/↓ selection started.
    anchor: Signal<Option<usize>>,
}

impl Diff {
    fn new() -> Diff {
        let patch = signal(None::<FilePatch>);
        let rows = memo(move || patch.with(diff_rows));
        let cursor = signal(0usize);
        let anchor = signal(None);
        watch(
            move || patch.get(),
            move |_, _| {
                cursor.set(0);
                anchor.set(None);
            },
        );
        Diff {
            patch,
            rows,
            cursor,
            anchor,
        }
    }

    fn range(&self) -> (usize, usize) {
        let cursor = self.cursor.get();
        let anchor = self.anchor.get().unwrap_or(cursor);
        (anchor.min(cursor), anchor.max(cursor))
    }

    /// The hunk under the cursor.
    fn hunk(&self) -> Option<usize> {
        let cursor = self.cursor.get();
        self.rows.with(|r| r.get(cursor).and_then(|r| r.hunk))
    }

    /// The hunk lines the selection covers in the cursor's hunk.
    fn lines(&self) -> (Option<usize>, HashSet<usize>) {
        let hunk = self.hunk();
        let (from, to) = self.range();
        let lines = self.rows.with(|rows| {
            (from..=to)
                .filter_map(|i| rows.get(i))
                .filter(|r| r.hunk == hunk)
                .filter_map(|r| r.line)
                .collect()
        });
        (hunk, lines)
    }

    /// The selected rows' text, for `y`.
    fn selected_text(&self) -> String {
        let (from, to) = self.range();
        let patch = self.patch.get();
        let rows = self.rows.get();
        let Some(file) = patch else {
            return String::new();
        };
        (from..=to)
            .filter_map(|i| rows.get(i))
            .map(|r| match (r.hunk, r.line) {
                (Some(h), Some(l)) => {
                    let line = &file.hunks[h].lines[l];
                    let mark = match line.kind {
                        LineKind::Added => '+',
                        LineKind::Removed => '-',
                        LineKind::Context => ' ',
                    };
                    format!("{mark}{}", line.text)
                }
                (Some(h), None) => file.hunks[h].header(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Move to the next (or the previous) hunk's header.
    fn jump(&self, forward: bool) {
        let cursor = self.cursor.get_untracked();
        let rows = self.rows.get_untracked();
        let headers = rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.hunk.is_some() && r.line.is_none())
            .map(|(i, _)| i);
        let target = if forward {
            headers.filter(|&i| i > cursor).min()
        } else {
            let here = rows.get(cursor).and_then(|r| r.hunk);
            headers
                .filter(|&i| i < cursor && rows[i].hunk != here)
                .max()
        };
        if let Some(target) = target {
            self.anchor.set(None);
            self.cursor.set(target);
        }
    }

    /// The list of rows, with the hunk markers; Shift with the moves
    /// selects lines (the list's range).
    fn view(&self) -> Node {
        let me = *self;
        let rows = self.rows;
        list_rows(
            move || rows.with(Vec::len),
            move |i| {
                let hunk = me.hunk();
                rows.with(|rows| {
                    let row = &rows[i];
                    let marker = match (row.end, row.line) {
                        (true, _) => "└",
                        (false, None) if row.hunk.is_some() => "┌",
                        _ => "│",
                    };
                    let marker = if row.hunk.is_none() { " " } else { marker };
                    let marker_style = if row.hunk.is_some() && row.hunk == hunk {
                        "on blue"
                    } else {
                        "bright_black"
                    };
                    ListRow::new(format!("[{marker_style}]{marker}[/]{}", row.markup))
                })
            },
            self.cursor,
        )
        .range(self.anchor)
        .selected_style_for(|_| Some("white on blue".into()))
        .theme_styles([("selected.range", "white on blue")])
    }
}

// ---------------------------------------------------------------------
// The app's state.
// ---------------------------------------------------------------------

/// Which of the Status tab's panes has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Focus {
    WorkDir,
    Stage,
    Diff,
}

/// Everything the app keeps, each part a signal, so the whole is `Copy`.
#[derive(Clone, Copy)]
struct Gu {
    git: Signal<Git>,
    tab: Signal<usize>,
    status: Signal<Status>,
    branch: Signal<String>,
    ahead: Signal<Option<(usize, usize)>>,
    focus: Signal<Focus>,
    /// The list the diff follows: the unstaged one or the staged one.
    staged_diff: Signal<bool>,
    wd: Files,
    stage: Files,
    diff: Diff,
    message: Signal<String>,
    log: Signal<Vec<Commit>>,
    refs: Signal<HashMap<String, Vec<Ref>>>,
    log_at: Signal<usize>,
    details: Signal<bool>,
    stashes: Signal<Vec<Commit>>,
    stash_at: Signal<usize>,
    stash_files: Files,
    untracked: Signal<bool>,
    keep_index: Signal<bool>,
    /// The command bar expanded (`.`).
    more: Signal<bool>,
}

impl Gu {
    fn new(git: Git) -> Gu {
        let status = signal(Status::default());
        let wd = Files::new(move || status.with(|s| s.unstaged.clone()));
        let stage = Files::new(move || status.with(|s| s.staged.clone()));
        let stash_files = Files::new(move || {
            status.with(|s| {
                let mut all = s.unstaged.clone();
                for file in &s.staged {
                    if !all.iter().any(|f| f.path == file.path) {
                        all.push(file.clone());
                    }
                }
                all.sort_by(|a, b| a.path.cmp(&b.path));
                all
            })
        });
        let gu = Gu {
            git: signal(git),
            tab: signal(0),
            status,
            branch: signal(String::new()),
            ahead: signal(None),
            focus: signal(Focus::WorkDir),
            staged_diff: signal(false),
            wd,
            stage,
            diff: Diff::new(),
            message: signal(String::new()),
            log: signal(Vec::new()),
            refs: signal(HashMap::new()),
            log_at: signal(0),
            details: signal(false),
            stashes: signal(Vec::new()),
            stash_at: signal(0),
            stash_files,
            untracked: signal(true),
            keep_index: signal(false),
            more: signal(false),
        };
        // The diff follows the selected file of the list last focused.
        watch(
            move || {
                let staged = gu.staged_diff.get();
                let file = if staged {
                    gu.stage.file()
                } else {
                    gu.wd.file()
                };
                (staged, file, gu.status.get())
            },
            move |(staged, file, _), cx| {
                let patch = match file {
                    Some(file) => gu.git.with(|g| g.diff(&file, staged)),
                    None => Ok(None),
                };
                match patch {
                    Ok(patch) => gu.diff.patch.set(patch),
                    Err(error) => error_popup(cx, &error),
                }
            },
        );
        gu.refresh();
        gu
    }

    fn git(&self) -> Git {
        self.git.get_untracked()
    }

    /// Read the repository again.
    fn refresh(&self) {
        let git = self.git();
        match git.status() {
            Ok(status) => self.status.set(status),
            Err(_) => self.status.set(Status::default()),
        }
        self.branch.set(git.branch_name());
        self.ahead.set(git.ahead_behind());
        let log = git.log(1000);
        self.log_at
            .update(|at| *at = (*at).min(log.len().saturating_sub(1)));
        self.log.set(log);
        self.refs.set(git.refs());
        let stashes = git.stashes();
        self.stash_at
            .update(|at| *at = (*at).min(stashes.len().saturating_sub(1)));
        self.stashes.set(stashes);
    }

    /// Run a change, then read the repository again; a failure is shown.
    fn change(&self, cx: &mut Ctx, change: impl FnOnce(&Git) -> GitResult<()>) {
        let result = change(&self.git());
        self.refresh();
        if let Err(error) = result {
            error_popup(cx, &error);
        }
    }

    /// `{main}`, with `↑1 ↓0 ` before it when there is an upstream.
    fn branch_label(&self) -> String {
        let ahead = self
            .ahead
            .get()
            .map(|(a, b)| format!("\u{2191}{a} \u{2193}{b} "))
            .unwrap_or_default();
        format!("{ahead}{{{}}}", escape(&self.branch.get()))
    }
}

/// gitui's command bar: `Stage [⏎] Reset [D]`, the keys as symbols, and
/// `more [.]` where they do not fit, `.` expanding it onto more rows.
fn command_bar(more: Signal<bool>) -> Node {
    key_hints_with(
        KeyHintsOptions::new()
            .format(|keys, desc| format!("{desc} [{keys}]"))
            .key_symbols(true)
            .separator(" ")
            .more(more, "more [.]"),
    )
}

/// gitui's error box: the message, closed by Enter or Esc.
fn error_popup(cx: &mut Ctx, error: &str) {
    let error = escape(error);
    cx.modal(Size::Percent(60), Size::Auto, move || {
        column([
            label(&error),
            label("[dim]Close [⏎][/]").text_align(TextAlign::Right),
        ])
        .padding(0, 1)
        .panel("[bold]Error[/]")
        .border_style("red")
        .on_key("enter esc", |cx| cx.pop())
    });
}

/// A panel whose title follows signals: gitui's "Diff: src/main.rs" and
/// "Commit 3/10", flush with the corner as a ratatui `Block`'s is
/// (`┌Staged Changes──`): bold while `focused`, else grey.
fn titled(
    body: Node,
    title: impl Fn() -> String + 'static,
    focused: impl Fn() -> bool + 'static,
) -> Node {
    body.panel_with(move || {
        let style = if focused() {
            "bold default"
        } else {
            "bright_black"
        };
        format!("[{style}]{}[/]", title())
    })
    .border(Border::Plain)
    .border_style("bright_black")
    .title_flush()
}

// ---------------------------------------------------------------------
// The Status tab.
// ---------------------------------------------------------------------

fn status_tab(gu: Gu) -> (Node, [NodeId; 3]) {
    let focused = move |f: Focus| move || gu.focus.get() == f;

    let wd_list = gu.wd.view();
    let stage_list = gu.stage.view();
    let diff_list = gu.diff.view();
    let ids = [wd_list.id(), stage_list.id(), diff_list.id()];

    let wd = titled(
        wd_list,
        || "Unstaged Changes".into(),
        focused(Focus::WorkDir),
    );
    let branch = move || match gu.focus.get() != Focus::Stage {
        true => gu.branch_label(),
        false => String::new(),
    };
    let wd = wd
        .add_title_with(Edge::Bottom, Align::End, branch)
        .name("Unstaged Changes")
        .on_focus(move |_| gu.focus.set(Focus::WorkDir))
        .previews_keys()
        .bind("enter", "Stage", move |cx| {
            let paths: Vec<String> = gu.wd.targets().into_iter().map(|f| f.path).collect();
            if !paths.is_empty() {
                gu.change(cx, |g| g.stage(&paths));
            }
        })
        .bind("a", "Stage All", move |cx| {
            gu.change(cx, |g| g.stage(&[".".into()]))
        })
        .bind("D", "Reset", move |cx| reset_files(gu, cx, gu.wd.targets()))
        .bind("i", "Ignore", move |cx| {
            if let Some(file) = gu.wd.file() {
                let ignore = gu.git().root.join(".gitignore");
                let added = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(ignore)
                    .and_then(|mut f| writeln!(f, "{}", file.path));
                gu.change(cx, |_| added.map_err(|e| e.to_string()));
            }
        });

    let stage = titled(
        stage_list,
        || "Staged Changes".into(),
        focused(Focus::Stage),
    );
    let branch = move || match gu.focus.get() == Focus::Stage {
        true => gu.branch_label(),
        false => String::new(),
    };
    let stage = stage
        .add_title_with(Edge::Top, Align::End, branch)
        .name("Staged Changes")
        .on_focus(move |_| gu.focus.set(Focus::Stage))
        .previews_keys()
        .bind("enter", "Unstage", move |cx| {
            let paths: Vec<String> = gu.stage.targets().into_iter().map(|f| f.path).collect();
            if !paths.is_empty() {
                gu.change(cx, |g| g.unstage(&paths));
            }
        })
        .bind("a", "Unstage all", move |cx| {
            gu.change(cx, |g| {
                let all: Vec<String> = g.status()?.staged.into_iter().map(|f| f.path).collect();
                if all.is_empty() {
                    return Ok(());
                }
                g.unstage(&all)
            })
        });

    let diff_title = move || {
        let path = gu
            .diff
            .patch
            .with(|p| p.as_ref().map(|f| f.path().to_string()));
        format!("Diff: {}", escape(&path.unwrap_or_default()))
    };
    let diff = diff_bindings(gu, titled(diff_list, diff_title, focused(Focus::Diff)))
        .name("Diff")
        .on_focus(move |_| gu.focus.set(Focus::Diff));

    // gitui's layout follows its focus: the lists 60/40, 40/60 while the
    // diff shows the staged list's file, and the diff alone while focused.
    let share =
        move |staged: bool| move || Size::Flex(if gu.staged_diff.get() == staged { 3 } else { 2 });
    let left = column([wd.size_with(share(false)), stage.size_with(share(true))])
        .percent(50)
        .hidden_when(move || gu.focus.get() == Focus::Diff);
    let page = row([left, diff.flex(1)])
        .name("Status")
        .bind_when(
            move || gu.focus.get() != Focus::Diff,
            "right l",
            "Diff",
            move |_| gu.focus.set(Focus::Diff),
        )
        .bind_when(
            move || gu.focus.get() == Focus::WorkDir,
            "w",
            "To stage",
            move |_| gu.focus.set(Focus::Stage),
        )
        .bind_when(
            move || gu.focus.get() == Focus::Stage,
            "w",
            "To unstaged",
            move |_| gu.focus.set(Focus::WorkDir),
        )
        .bind_when(
            move || gu.status.with(|s| !s.staged.is_empty()),
            "c",
            "Commit",
            move |cx| commit_popup(gu, cx, false),
        );
    (page, ids)
}

/// D on a file or a folder: ask, then throw its changes away.
fn reset_files(gu: Gu, cx: &mut Ctx, files: Vec<FileEntry>) {
    if files.is_empty() {
        return;
    }
    let names = files.iter().map(|f| escape(&f.path)).collect();
    cx.confirm("Reset", "confirm file reset?", names, move |cx| {
        gu.change(cx, |g| g.reset(&files));
    });
}

/// The diff's keys, which see keys before its list does.
fn diff_bindings(gu: Gu, view: Node) -> Node {
    let diff = gu.diff;
    let staged = gu.staged_diff;
    let hunk_patch_of = move |whole: bool, reverse: bool| {
        let (hunk, lines) = diff.lines();
        let patch = diff.patch.get();
        hunk.zip(patch).and_then(|(hunk, file)| {
            hunk_patch(&file, hunk, |l| whole || lines.contains(&l), reverse)
        })
    };
    let untracked = move || gu.wd.file().is_some_and(|f| f.untracked) && !gu.staged_diff.get();
    view.previews_keys()
        .bind("left h", "Back", move |_| {
            let back = if staged.get() {
                Focus::Stage
            } else {
                Focus::WorkDir
            };
            gu.focus.set(back);
        })
        .bind("n", "Next hunk", move |_| diff.jump(true))
        .bind("p", "Prev hunk", move |_| diff.jump(false))
        .bind_when(
            move || !staged.get(),
            "enter",
            "Add hunk",
            move |cx| {
                if untracked() {
                    let file = gu.wd.file().map(|f| f.path).into_iter().collect::<Vec<_>>();
                    return gu.change(cx, |g| g.stage(&file));
                }
                if let Some(patch) = hunk_patch_of(true, false) {
                    gu.change(cx, |g| g.apply(&patch, true, false));
                }
            },
        )
        .bind_when(
            move || staged.get(),
            "enter",
            "Remove hunk",
            move |cx| {
                if let Some(patch) = hunk_patch_of(true, true) {
                    gu.change(cx, |g| g.apply(&patch, true, true));
                }
            },
        )
        .bind_when(
            move || !staged.get(),
            "D",
            "Reset hunk",
            move |cx| {
                if untracked() {
                    let files = gu.wd.file().into_iter().collect();
                    return reset_files(gu, cx, files);
                }
                if let Some(patch) = hunk_patch_of(true, true) {
                    cx.confirm("Reset", "confirm reset hunk?", Vec::new(), move |cx| {
                        gu.change(cx, |g| g.apply(&patch, false, true));
                    });
                }
            },
        )
        .bind_when(
            move || !staged.get(),
            "s",
            "Stage lines",
            move |cx| {
                if untracked() {
                    let file = gu.wd.file().map(|f| f.path).into_iter().collect::<Vec<_>>();
                    return gu.change(cx, |g| g.stage(&file));
                }
                if let Some(patch) = hunk_patch_of(false, false) {
                    gu.change(cx, |g| g.apply(&patch, true, false));
                }
            },
        )
        .bind_when(
            move || staged.get(),
            "s",
            "Unstage lines",
            move |cx| {
                if let Some(patch) = hunk_patch_of(false, true) {
                    gu.change(cx, |g| g.apply(&patch, true, true));
                }
            },
        )
        .bind_when(
            move || !staged.get() && !untracked(),
            "d",
            "Reset lines",
            move |cx| {
                if let Some(patch) = hunk_patch_of(false, true) {
                    cx.confirm("Reset", "confirm reset lines?", Vec::new(), move |cx| {
                        gu.change(cx, |g| g.apply(&patch, false, true));
                    });
                }
            },
        )
        .bind("y", "Copy", move |cx| cx.copy(diff.selected_text()))
}

// ---------------------------------------------------------------------
// The commit popup.
// ---------------------------------------------------------------------

/// gitui's first-line limit, past which the popup warns.
const FIRST_LINE_LIMIT: usize = 50;

fn commit_popup(gu: Gu, cx: &mut Ctx, amend: bool) {
    if amend {
        gu.message
            .set(gu.git().head_message().trim_end().to_string());
    }
    let title = if amend { "Commit (Amend)" } else { "Commit" };
    cx.modal(Size::Percent(60), Size::Percent(20), move || {
        let message = gu.message;
        let area = text_area(message).placeholder("type commit message..");
        let branch = move || format!("{{{}}}", escape(&gu.branch.get()));
        let warning = move || {
            let first = message.with(|m| m.lines().next().map_or(0, str::len));
            if first > FIRST_LINE_LIMIT {
                format!("[red]{}[/]", escape(&format!("[subject length: {first}]")))
            } else {
                String::new()
            }
        };
        let title = move || title.to_string();
        titled(area, title, || true)
            .add_title_with(Edge::Top, Align::End, branch)
            .add_title_with(Edge::Bottom, Align::End, warning)
            .name("Commit")
            .previews_keys()
            .bind("ctrl+d", "Do Commit", move |cx| {
                let text = message.get();
                if text.trim().is_empty() {
                    return error_popup(cx, "commit message empty");
                }
                let result = gu.git().commit(&text, amend);
                gu.refresh();
                match result {
                    Ok(()) => {
                        message.set(String::new());
                        cx.pop();
                    }
                    Err(error) => error_popup(cx, &error),
                }
            })
            .bind("ctrl+e", "Open editor", move |cx| open_editor(gu, cx))
            .bind_when(
                move || !amend && gu.git().head().is_some(),
                "ctrl+a",
                "Amend",
                move |cx| {
                    cx.pop();
                    commit_popup(gu, cx, true);
                },
            )
            .bind("esc", "Close", |cx| cx.pop())
    });
}

/// Ctrl+E: the message in `$EDITOR`, with gitui's comment under it, read
/// back when the editor ends.
fn open_editor(gu: Gu, cx: &mut Ctx) {
    let git = gu.git();
    let file = git.message_file();
    let text = format!(
        "{}\n# Edit your commit message\n# Lines starting with '#' will be ignored",
        gu.message.get()
    );
    if let Err(error) = std::fs::write(&file, text) {
        return error_popup(cx, &error.to_string());
    }
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(format!("{} \"$@\"", git.editor()))
        .arg("sh")
        .arg(&file)
        .current_dir(&git.root);
    cx.handoff_then(command, move |result, cx| {
        if let Err(error) = result {
            return error_popup(cx, &error.to_string());
        }
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let kept: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
        gu.message.set(kept.join("\n").trim().to_string());
    });
}

// ---------------------------------------------------------------------
// The Log tab, and a commit's details and diff.
// ---------------------------------------------------------------------

fn time_of(seconds: i64) -> String {
    let time = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds.max(0) as u64);
    let day = |t| rich_ext::fs::local_time(t, "%Y-%m-%d");
    let now = SystemTime::now();
    let ago = now.duration_since(time).unwrap_or_default().as_secs();
    if ago < 60 {
        format!("{:<10}", "<1m ago")
    } else if ago < 30 * 60 {
        format!("{:<10}", format!("{:0>2}m ago", ago / 60))
    } else if day(time) == day(now) {
        rich_ext::fs::local_time(time, "%T  ")
    } else {
        day(time)
    }
}

/// A commit as gitui's log draws it: hash, time, author, refs, subject.
fn commit_row(commit: &Commit, refs: Option<&Vec<Ref>>, width: usize) -> String {
    let author_width = (width.saturating_sub(19) / 3).clamp(3, 20);
    let mut author: String = commit.author.chars().take(author_width).collect();
    if commit.author.chars().count() > author_width {
        author.pop();
        author.push('…');
    }
    let author = format!("{author:<author_width$}");
    let mut row = format!(
        "[magenta]{}[/] [bright_cyan]{}[/] [green]{}[/] ",
        commit.short,
        time_of(commit.time),
        escape(&author)
    );
    let refs = refs.cloned().unwrap_or_default();
    let tags: Vec<String> = (refs.iter())
        .filter_map(|r| match r {
            Ref::Tag(t) => Some(format!("<{t}>")),
            _ => None,
        })
        .collect();
    if !tags.is_empty() {
        row += &format!(" [bold bright_magenta]{}[/]", escape(&tags.join(" ")));
    }
    for (local, open, close) in [(true, '{', '}'), (false, '[', ']')] {
        let names: Vec<String> = (refs.iter())
            .filter_map(|r| match (r, local) {
                (Ref::Local(n), true) | (Ref::Remote(n), false) => {
                    Some(format!("{open}{n}{close}"))
                }
                _ => None,
            })
            .collect();
        if !names.is_empty() {
            row += &format!(" [bright_yellow]{}[/]", escape(&names.join(" ")));
        }
    }
    row + " " + &escape(&commit.subject)
}

/// A list of commits (the log, the stashes), titled with gitui's count.
fn commit_list(
    gu: Gu,
    commits: Signal<Vec<Commit>>,
    at: Signal<usize>,
    title: &'static str,
) -> (Node, NodeId) {
    let width = viewport();
    let list = list_rows(
        move || commits.with(Vec::len),
        move |i| {
            let refs = gu.refs;
            let row = commits.with(|c| {
                refs.with(|r| commit_row(&c[i], r.get(&c[i].hash), width.get().0 as usize))
            });
            ListRow::new(row)
        },
        at,
    )
    .selected_style_for(|_| Some("white on blue".into()));
    let id = list.id();
    let count = move || {
        let len = commits.with(Vec::len);
        format!("{title} {}/{len}", len.saturating_sub(at.get()))
    };
    (titled(list, count, || true), id)
}

/// A commit's details: gitui's Info box and its files.
fn details_view(
    gu: Gu,
    hash: impl Fn() -> Option<String> + 'static,
) -> (Node, Signal<usize>, Memo<Details>) {
    let hash = memo(hash);
    let details = memo(move || {
        hash.get()
            .map(|h| gu.git.with(|g| g.details(&h)))
            .unwrap_or_default()
    });
    let info = facts_of(move || {
        let d = details.get();
        let mut rows = vec![
            ("Author: ".into(), escape(&d.author)),
            (
                "Date: ".into(),
                escape(&rich_ext::fs::local_time(
                    SystemTime::UNIX_EPOCH + Duration::from_secs(d.time.max(0) as u64),
                    "%Y-%m-%d %H:%M:%S",
                )),
            ),
        ];
        if d.committer != d.author {
            rows.push(("Committer: ".into(), escape(&d.committer)));
        }
        rows.push(("Sha: ".into(), hash.get().unwrap_or_default()));
        let refs = hash
            .get()
            .and_then(|h| gu.refs.with(|r| r.get(&h).cloned()));
        let tags: Vec<String> = (refs.iter().flatten())
            .filter_map(|r| match r {
                Ref::Tag(t) => Some(escape(t)),
                _ => None,
            })
            .collect();
        if !tags.is_empty() {
            rows.push(("Tags: ".into(), tags.join(", ")));
        }
        rows.push(("Subject: ".into(), escape(&d.message)));
        rows
    });
    let file_at = signal(0usize);
    let files = list_rows(
        move || details.with(|d| d.files.len()),
        move |i| {
            details.with(|d| {
                let f = &d.files[i];
                ListRow::new(format!(
                    "[{}]{} {}[/]",
                    f.kind.style(),
                    f.kind.letter(),
                    escape(&f.path)
                ))
            })
        },
        file_at,
    )
    .selected_style_for(|_| Some("white on blue".into()));
    let view = column([
        titled(info, || "Info".into(), || false).percent(60),
        titled(files, || "Files".into(), || false).percent(40),
    ]);
    (view, file_at, details)
}

fn log_tab(gu: Gu) -> (Node, NodeId) {
    let (list, id) = commit_list(gu, gu.log, gu.log_at, "Commit");
    let hash = move || {
        gu.log
            .with(|l| l.get(gu.log_at.get()).map(|c| c.hash.clone()))
    };
    let (details, ..) = details_view(gu, hash);
    let view = row([
        list.flex(3),
        details.flex(2).shown_when(move || gu.details.get()),
    ]);
    let view = view
        .name("Log")
        .bind("enter", "Details", move |_| gu.details.update(|d| *d = !*d))
        .bind("right l", "Inspect", move |cx| {
            if let Some(hash) = gu
                .log
                .with(|l| l.get(gu.log_at.get()).map(|c| c.hash.clone()))
            {
                inspect(gu, cx, hash);
            }
        })
        .bind("y", "Copy Hash", move |cx| {
            if let Some(hash) = gu
                .log
                .with(|l| l.get(gu.log_at.get()).map(|c| c.hash.clone()))
            {
                cx.copy(hash);
            }
        });
    (view, id)
}

/// gitui's Inspect: a commit's details and the diff of its selected file,
/// full screen.
fn inspect(gu: Gu, cx: &mut Ctx, hash: String) {
    cx.push(move || {
        let patch = {
            let hash = hash.clone();
            memo(move || gu.git.with(|g| g.commit_diff(&hash)))
        };
        let shown = hash.clone();
        let (details, file_at, info) = details_view(gu, move || Some(shown.clone()));
        let diff = Diff::new();
        watch(
            move || {
                let path = info.with(|d| d.files.get(file_at.get()).map(|f| f.path.clone()));
                patch.with(|p| {
                    path.and_then(|path| p.files.iter().find(|f| f.path() == path).cloned())
                })
            },
            move |file, _| diff.patch.set(file),
        );
        let diff_title = move || {
            let path = diff
                .patch
                .with(|p| p.as_ref().map(|f| f.path().to_string()));
            format!("Diff: {}", escape(&path.unwrap_or_default()))
        };
        let view = diff.view();
        let list = titled(view, diff_title, || true)
            .previews_keys()
            .bind("n", "Next hunk", move |_| diff.jump(true))
            .bind("p", "Prev hunk", move |_| diff.jump(false))
            .bind("y", "Copy", move |cx| cx.copy(diff.selected_text()));
        let page =
            row([details.percent(40), list.percent(60)]).bind("esc left", "Close", |cx| cx.pop());
        column([page.flex(1), command_bar(gu.more).auto()])
    });
}

// ---------------------------------------------------------------------
// The Files tab: the tree at HEAD and a file's text.
// ---------------------------------------------------------------------

fn files_tab(gu: Gu) -> (Node, NodeId) {
    let all = memo(move || {
        gu.log.with(|_| ());
        gu.git.with(|g| g.files())
    });
    let files = Files::new(move || {
        all.with(|paths| {
            (paths.iter())
                .map(|p| FileEntry {
                    path: p.clone(),
                    kind: Kind::Unchanged,
                    untracked: false,
                })
                .collect()
        })
    });
    let text = signal(String::new());
    watch(
        move || files.file(),
        move |file, _| {
            text.set(
                file.map(|f| gu.git.with(|g| g.show_file(&f.path)))
                    .unwrap_or_default(),
            )
        },
    );
    let tree = files.view();
    let id = tree.id();
    // A text area's language is set when it is made: one per extension,
    // on the same text, the last one dropped.
    let extension = memo(move || {
        let path = files.file().map(|f| f.path).unwrap_or_default();
        Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().into_owned())
            .unwrap_or_default()
    });
    let content = keyed(
        move || extension.get(),
        move |extension| {
            let area = text_area(text).read_only().line_numbers();
            if extension.is_empty() {
                area
            } else {
                area.language(&extension)
            }
        },
    );
    let title = move || {
        let file = files.file().map(|f| f.path).unwrap_or_default();
        escape(&file)
    };
    let view = row([
        titled(tree, || "Files at HEAD".into(), || true).percent(50),
        titled(content, title, || false).percent(50),
    ]);
    (view, id)
}

// ---------------------------------------------------------------------
// The Stashing and Stashes tabs.
// ---------------------------------------------------------------------

fn stashing_tab(gu: Gu) -> (Node, NodeId) {
    let tree = gu.stash_files.view();
    let id = tree.id();
    let option = |on: bool, name: &str| {
        let mark = if on {
            "[green]x[/]"
        } else {
            "[bright_black]_[/]"
        };
        format!("[{mark}] {name}")
    };
    let options = text(move || {
        format!(
            "{}\n{}",
            option(gu.untracked.get(), "stash untracked"),
            option(gu.keep_index.get(), "keep index")
        )
    });
    let view = row([
        titled(tree, || "Files to Stash".into(), || true).flex(1),
        column([
            titled(options, || "Options".into(), || false).fixed(4),
            label(""),
        ])
        .fixed(22),
    ])
    .name("Stashing")
    .bind("s", "Stash", move |cx| {
        let options = PromptOptions::new().at(Origin::Center, Offset::new(0, 0, 50, 3));
        cx.prompt_with("Stash", "", options, move |message, cx| {
            let (untracked, keep) = (gu.untracked.get(), gu.keep_index.get());
            gu.change(cx, |g| g.stash(&message, untracked, keep));
        });
    })
    .bind("u", "Toggle Untracked", move |_| {
        gu.untracked.update(|u| *u = !*u)
    })
    .bind("i", "Toggle Staged", move |_| {
        gu.keep_index.update(|k| *k = !*k)
    });
    (view, id)
}

fn stashes_tab(gu: Gu) -> (Node, NodeId) {
    let (list, id) = commit_list(gu, gu.stashes, gu.stash_at, "Stashes");
    let selected = move || {
        gu.stashes
            .with(|s| s.get(gu.stash_at.get()).map(|c| c.hash.clone()))
    };
    let has = move || gu.stashes.with(|s| !s.is_empty());
    let view = list
        .name("Stashes")
        .bind_when(has, "a", "Apply", move |cx| {
            let at = gu.stash_at.get();
            gu.change(cx, |g| g.stash_do("apply", at));
        })
        .bind_when(has, "p", "Pop", move |cx| {
            let at = gu.stash_at.get();
            gu.change(cx, |g| g.stash_do("pop", at));
        })
        .bind_when(has, "D", "Drop", move |cx| {
            let at = gu.stash_at.get();
            cx.confirm("Drop", "confirm stash drop?", Vec::new(), move |cx| {
                gu.change(cx, |g| g.stash_do("drop", at));
            });
        })
        .bind_when(has, "right l enter", "Inspect", move |cx| {
            if let Some(hash) = selected() {
                inspect(gu, cx, hash);
            }
        });
    (view, id)
}

// ---------------------------------------------------------------------
// The branch list.
// ---------------------------------------------------------------------

fn branch_popup(gu: Gu, cx: &mut Ctx) {
    let look = ModalOptions::new();
    cx.modal_with(Size::Percent(80), Size::Percent(50), look, move || {
        let remote = signal(0usize);
        let all = signal(gu.git().branches());
        let shown = memo(move || {
            let local = remote.get() == 0;
            all.with(|b| {
                b.iter()
                    .filter(|b| b.local == local)
                    .cloned()
                    .collect::<Vec<_>>()
            })
        });
        let at = signal(shown.with(|b| b.iter().position(|b| b.head).unwrap_or(0)));
        watch(move || remote.get(), move |_, _| at.set(0));
        let reload = move || all.set(gu.git().branches());
        let width = viewport();
        let list = list_rows(
            move || shown.with(Vec::len),
            move |i| {
                shown.with(|b| {
                    let b = &b[i];
                    let head = if b.head { '*' } else { ' ' };
                    let tracking = if b.upstream { '\u{2191}' } else { ' ' };
                    let name_width = (width.get().0 as usize * 8 / 10 / 3).clamp(10, 40);
                    let mut name: String = b.name.chars().take(name_width).collect();
                    if b.name.chars().count() > name_width {
                        name.truncate(name_width.saturating_sub(3));
                        name += "...";
                    }
                    let style = if b.head {
                        "bold bright_yellow"
                    } else {
                        "bright_yellow"
                    };
                    ListRow::new(format!(
                        "{head}{tracking} [{style}]{:<name_width$}[/] [magenta]{}[/] {}",
                        escape(&name),
                        b.hash,
                        escape(&b.subject)
                    ))
                })
            },
            at,
        )
        .selected_style_for(|_| Some("white on blue".into()));
        let selected = move || shown.with(|b| b.get(at.get()).cloned());
        let local = move || remote.get() == 0;
        column([
            tabs(|| vec!["Local".into(), "Remote".into()], remote)
                .divider(" | ")
                .no_focus()
                .borders(Sides::BOTTOM)
                .fixed(2),
            list.autofocus().flex(1),
        ])
        .panel("Branches")
        .border(Border::Thick)
        .name("Branches")
        .previews_keys()
        .bind("left right", "Local/Remote", move |_| {
            remote.update(|r| *r = 1 - *r)
        })
        .bind("enter", "Checkout", move |cx| {
            if let Some(branch) = selected() {
                gu.change(cx, |g| g.checkout(&branch));
                if gu.git().branch_name() == branch.name || !branch.local {
                    cx.pop();
                }
            }
        })
        .bind_when(local, "c", "Create Branch", move |cx| {
            cx.prompt_with("Branch", "", centered_prompt(), move |name, cx| {
                gu.change(cx, |g| g.create_branch(name.trim()));
                cx.pop();
            });
        })
        .bind_when(local, "r", "Rename Branch", move |cx| {
            if let Some(branch) = selected() {
                let from = branch.name.clone();
                cx.prompt_with(
                    "Rename Branch",
                    branch.name,
                    centered_prompt(),
                    move |to, cx| {
                        gu.change(cx, |g| g.rename_branch(&from, to.trim()));
                        reload();
                    },
                );
            }
        })
        .bind_when(
            move || local() && selected().is_some_and(|b| !b.head),
            "D",
            "Delete",
            move |cx| {
                if let Some(branch) = selected() {
                    let title = "Delete Branch";
                    let body = format!("Confirm deleting branch: '{}' ?", escape(&branch.name));
                    cx.confirm(title, body, Vec::new(), move |cx| {
                        gu.change(cx, |g| g.delete_branch(&branch.name));
                        reload();
                    });
                }
            },
        )
        .bind_when(
            move || selected().is_some_and(|b| !b.head),
            "m",
            "Merge",
            move |cx| {
                if let Some(branch) = selected() {
                    gu.change(cx, |g| g.merge(&branch.name));
                    cx.pop();
                }
            },
        )
        .bind("esc", "Close", |cx| cx.pop())
    });
}

fn centered_prompt() -> PromptOptions {
    PromptOptions::new().at(Origin::Center, Offset::new(0, 0, 50, 3))
}

// ---------------------------------------------------------------------
// The app.
// ---------------------------------------------------------------------

/// gitui's tab titles.
const TABS: [&str; 5] = [
    "Status [1]",
    "Log [2]",
    "Files [3]",
    "Stashing [4]",
    "Stashes [5]",
];

/// The app on the repository `git` names.
pub fn git_app(git: Git) -> App {
    App::new(move || {
        let gu = Gu::new(git.clone());
        let path = gu.git().root.display().to_string();

        let (status, status_ids) = status_tab(gu);
        let (log, log_id) = log_tab(gu);
        let (files, files_id) = files_tab(gu);
        let (stashing, stashing_id) = stashing_tab(gu);
        let (stashes, stashes_id) = stashes_tab(gu);
        let pages = std::cell::RefCell::new(vec![
            Some(status),
            Some(log),
            Some(files),
            Some(stashing),
            Some(stashes),
        ]);
        let first = [status_ids[0], log_id, files_id, stashing_id, stashes_id];

        // The focus follows the tab and, on the Status tab, gitui's focus.
        watch(
            move || (gu.tab.get(), gu.focus.get()),
            move |(tab, focus), cx| {
                let id = match (tab, focus) {
                    (0, Focus::WorkDir) => status_ids[0],
                    (0, Focus::Stage) => status_ids[1],
                    (0, Focus::Diff) => status_ids[2],
                    (tab, _) => first[tab],
                };
                cx.focus(id);
                if focus != Focus::Diff {
                    gu.staged_diff.set(focus == Focus::Stage);
                }
            },
        );

        let top = row([
            tabs(|| TABS.iter().map(|t| t.to_string()).collect(), gu.tab)
                .divider(" | ")
                .no_focus()
                .auto(),
            text(move || format!("[bright_black]{}[/]", escape(&path)))
                .no_wrap()
                .truncate_start()
                .text_align(TextAlign::Right)
                .flex(1),
        ])
        .padding(0, 1)
        .borders(Sides::BOTTOM)
        .border_style("bright_black")
        .fixed(2);

        let body = switch(
            move || gu.tab.get(),
            move |tab| pages.borrow_mut()[tab].take().unwrap_or_else(|| label("")),
        );

        every(Duration::from_secs(5), move |_| gu.refresh());

        column([top, body.flex(1), command_bar(gu.more).auto()])
            .theme_styles([
                ("border.focused", "white"),
                ("key_hints.hint", "white"),
                ("tabs.selected", "underline"),
            ])
            .name("Global")
            .previews_keys()
            .bind("1", "Status", move |_| gu.tab.set(0))
            .bind("2", "Log", move |_| gu.tab.set(1))
            .bind("3", "Files", move |_| gu.tab.set(2))
            .bind("4", "Stashing", move |_| gu.tab.set(3))
            .bind("5", "Stashes", move |_| gu.tab.set(4))
            .bind("tab", "Tab", move |_| gu.tab.update(|t| *t = (*t + 1) % 5))
            .bind("shift+tab", "Tab back", move |_| {
                gu.tab.update(|t| *t = (*t + 4) % 5)
            })
            .bind("b", "Branches", move |cx| branch_popup(gu, cx))
            .bind("period", "More", move |_| gu.more.update(|on| *on = !*on))
            .bind("h", "Help", |cx| {
                cx.help_with(HelpOptions::new().title("Help: all commands"))
            })
            .bind("q", "Quit", |cx| cx.quit())
    })
    .palette_key("ctrl+p")
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let git = Git::open(&dir).map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e))?;
    git_app(git).run()
}

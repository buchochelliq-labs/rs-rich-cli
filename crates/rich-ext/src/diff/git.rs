//! `git diff` output: a parser and a review-style renderer.
//!
//! [`parse_unified`] reads `git diff` (and plain `diff -u`) output: `diff
//! --git` headers, `index` lines, new, deleted, renamed and copied files,
//! mode changes, binary files, `---`/`+++`, hunks and `\ No newline at end of
//! file`. [`PatchView`] renders a [`Patch`] with a file tree summary,
//! syntax-highlighted hunks, inline [`Annotation`]s and links from a
//! [`LinkProvider`].

use std::collections::BTreeMap;
use std::fmt;

use rich::cells::cell_len;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use super::render::{self, Block, Kind, Options, Row};
use super::source::{highlight_lines, language_for_path};
use super::{hunk_header, style, Layout};
use crate::diagnostic::Level;
use crate::hyperlink::Hyperlinker;

/// What happened to a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileStatus {
    Added,
    Deleted,
    Modified,
    Renamed,
    Copied,
    /// A modified binary file (an added, deleted or renamed binary keeps that
    /// status and sets [`FilePatch::binary`]).
    Binary,
}

impl FileStatus {
    fn label(self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Deleted => "deleted",
            FileStatus::Modified => "modified",
            FileStatus::Renamed => "renamed",
            FileStatus::Copied => "copied",
            FileStatus::Binary => "binary",
        }
    }
}

/// A line of a hunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

/// One line of a hunk, with its line numbers on each side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchLine {
    pub kind: LineKind,
    /// The text, without the leading marker or newline.
    pub text: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
    /// Followed by `\ No newline at end of file`.
    pub no_newline: bool,
}

/// A hunk: `@@ -old_start,old_len +new_start,new_len @@ section`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatchHunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    /// The text after the closing `@@` (often a function name).
    pub section: String,
    pub lines: Vec<PatchLine>,
}

impl PatchHunk {
    /// The `@@ … @@` header, with the section text.
    pub fn header(&self) -> String {
        let range = |start: usize, len: usize| {
            // `hunk_header` takes 0-based ranges; an empty range names the
            // line before it, which is what the file said.
            let s = if len == 0 { start } else { start - 1 };
            s..s + len
        };
        let mut header = hunk_header(
            range(self.old_start, self.old_len),
            range(self.new_start, self.new_len),
        );
        if !self.section.is_empty() {
            header.push(' ');
            header.push_str(&self.section);
        }
        header
    }
}

/// One file's changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilePatch {
    /// The old path, `None` for an added file.
    pub old_path: Option<String>,
    /// The new path, `None` for a deleted file.
    pub new_path: Option<String>,
    pub status: FileStatus,
    pub binary: bool,
    pub old_mode: Option<String>,
    pub new_mode: Option<String>,
    /// `similarity index` for renames and copies, in percent.
    pub similarity: Option<u8>,
    pub hunks: Vec<PatchHunk>,
    pub additions: usize,
    pub deletions: usize,
}

impl FilePatch {
    fn empty() -> Self {
        FilePatch {
            old_path: None,
            new_path: None,
            status: FileStatus::Modified,
            binary: false,
            old_mode: None,
            new_mode: None,
            similarity: None,
            hunks: Vec::new(),
            additions: 0,
            deletions: 0,
        }
    }
    /// The path to show: the new path, else the old one.
    pub fn path(&self) -> &str {
        self.new_path
            .as_deref()
            .or(self.old_path.as_deref())
            .unwrap_or("")
    }
    /// Whether the mode changed.
    pub fn mode_changed(&self) -> bool {
        self.old_mode.is_some() && self.new_mode.is_some() && self.old_mode != self.new_mode
    }
}

/// A parsed patch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub files: Vec<FilePatch>,
}

impl Patch {
    /// Total `(additions, deletions)`.
    pub fn stats(&self) -> (usize, usize) {
        self.files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.additions, d + f.deletions))
    }
}

/// Why a patch did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    /// The 1-based input line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}
impl std::error::Error for ParseError {}

/// Undo git's C-style quoting of a path (`"a\tb"`).
fn unquote(path: &str) -> String {
    let Some(inner) = path.strip_prefix('"').and_then(|p| p.strip_suffix('"')) else {
        return path.to_string();
    };
    let mut bytes = Vec::new();
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match chars.next() {
            Some(b'n') => bytes.push(b'\n'),
            Some(b't') => bytes.push(b'\t'),
            Some(b'r') => bytes.push(b'\r'),
            Some(b'a') => bytes.push(7),
            Some(b'b') => bytes.push(8),
            Some(b'f') => bytes.push(12),
            Some(b'v') => bytes.push(11),
            Some(d @ b'0'..=b'7') => {
                let mut value = u32::from(d - b'0');
                for _ in 0..2 {
                    match chars.peek() {
                        Some(&o @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(o - b'0');
                            chars.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(value as u8);
            }
            Some(other) => bytes.push(other),
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// A `---`/`+++` path: `/dev/null` is none, `a/`/`b/` prefixes and a
/// trailing tab-separated timestamp are dropped.
fn header_path(raw: &str, prefix: &str) -> Option<String> {
    let raw = raw.split('\t').next().unwrap_or(raw).trim_end();
    let path = unquote(raw);
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix(prefix)
            .map(str::to_string)
            .unwrap_or(path),
    )
}

/// The two paths of `diff --git a/x b/y`.
fn git_header_paths(rest: &str) -> (Option<String>, Option<String>) {
    if rest.starts_with('"') {
        // Quoted: split after the closing quote of the first path.
        let mut escaped = false;
        for (i, c) in rest.char_indices().skip(1) {
            match c {
                '\\' if !escaped => escaped = true,
                '"' if !escaped => {
                    let (a, b) = rest.split_at(i + 1);
                    return (header_path(a, "a/"), header_path(b.trim_start(), "b/"));
                }
                _ => escaped = false,
            }
        }
        return (None, None);
    }
    // Unquoted and space-ambiguous: prefer the split where both sides name
    // the same file, as git does.
    let candidates: Vec<usize> = rest.match_indices(" b/").map(|(i, _)| i).collect();
    for &i in &candidates {
        let (a, b) = (&rest[..i], &rest[i + 1..]);
        if a.strip_prefix("a/") == b.strip_prefix("b/") {
            return (header_path(a, "a/"), header_path(b, "b/"));
        }
    }
    match candidates.first() {
        Some(&i) => (
            header_path(&rest[..i], "a/"),
            header_path(&rest[i + 1..], "b/"),
        ),
        None => match rest.split_once(' ') {
            Some((a, b)) => (Some(a.to_string()), Some(b.to_string())),
            None => (None, None),
        },
    }
}

fn parse_range(s: &str) -> Option<(usize, usize)> {
    match s.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((s.parse().ok()?, 1)),
    }
}

fn parse_hunk_header(line: &str) -> Option<PatchHunk> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, section) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_len) = parse_range(old)?;
    let (new_start, new_len) = parse_range(new)?;
    Some(PatchHunk {
        old_start,
        old_len,
        new_start,
        new_len,
        section: section.trim().to_string(),
        lines: Vec::new(),
    })
}

/// Parse `git diff` (or `diff -u`) output. Text outside any file, such as a
/// commit message, is skipped.
pub fn parse_unified(input: &str) -> Result<Patch, ParseError> {
    let mut patch = Patch::default();
    let mut file: Option<FilePatch> = None;
    // (hunk, old lines left, new lines left, next old, next new)
    let mut hunk: Option<(PatchHunk, usize, usize, usize, usize)> = None;
    let mut in_binary_literal = false;

    fn finish_hunk(
        file: &mut Option<FilePatch>,
        hunk: &mut Option<(PatchHunk, usize, usize, usize, usize)>,
    ) {
        if let (Some(f), Some((h, ..))) = (file.as_mut(), hunk.take()) {
            f.hunks.push(h);
        }
    }
    fn finish_file(patch: &mut Patch, file: &mut Option<FilePatch>) {
        if let Some(mut f) = file.take() {
            if f.status == FileStatus::Modified && f.binary {
                f.status = FileStatus::Binary;
            }
            patch.files.push(f);
        }
    }

    for (index, line) in input.lines().enumerate() {
        let number = index + 1;
        // Inside a hunk: consume body lines while counts remain.
        if let Some((h, old_left, new_left, next_old, next_new)) = hunk.as_mut() {
            if *old_left > 0 || *new_left > 0 {
                let (kind, text) = match line.as_bytes().first() {
                    Some(b' ') => (LineKind::Context, &line[1..]),
                    Some(b'-') => (LineKind::Removed, &line[1..]),
                    Some(b'+') => (LineKind::Added, &line[1..]),
                    Some(b'\\') => {
                        if let Some(last) = h.lines.last_mut() {
                            last.no_newline = true;
                        }
                        continue;
                    }
                    // Some tools strip the space of an empty context line.
                    None if *old_left > 0 && *new_left > 0 => (LineKind::Context, ""),
                    _ => {
                        return Err(ParseError {
                            line: number,
                            message: format!(
                                "hunk ended early: expected {old_left} more old and {new_left} more new lines"
                            ),
                        })
                    }
                };
                let (old_line, new_line) = match kind {
                    LineKind::Context => {
                        if *old_left == 0 || *new_left == 0 {
                            return Err(ParseError {
                                line: number,
                                message: "context line past the end of the hunk".into(),
                            });
                        }
                        *old_left -= 1;
                        *new_left -= 1;
                        *next_old += 1;
                        *next_new += 1;
                        (Some(*next_old - 1), Some(*next_new - 1))
                    }
                    LineKind::Removed => {
                        if *old_left == 0 {
                            return Err(ParseError {
                                line: number,
                                message: "removed line past the end of the hunk".into(),
                            });
                        }
                        *old_left -= 1;
                        *next_old += 1;
                        (Some(*next_old - 1), None)
                    }
                    LineKind::Added => {
                        if *new_left == 0 {
                            return Err(ParseError {
                                line: number,
                                message: "added line past the end of the hunk".into(),
                            });
                        }
                        *new_left -= 1;
                        *next_new += 1;
                        (None, Some(*next_new - 1))
                    }
                };
                if let Some(f) = file.as_mut() {
                    match kind {
                        LineKind::Added => f.additions += 1,
                        LineKind::Removed => f.deletions += 1,
                        LineKind::Context => {}
                    }
                }
                h.lines.push(PatchLine {
                    kind,
                    text: text.to_string(),
                    old_line,
                    new_line,
                    no_newline: false,
                });
                continue;
            }
            if let Some(rest) = line.strip_prefix('\\') {
                let _ = rest;
                if let Some(last) = h.lines.last_mut() {
                    last.no_newline = true;
                }
                continue;
            }
            finish_hunk(&mut file, &mut hunk);
        }
        if in_binary_literal {
            if line.starts_with("diff --git ") {
                in_binary_literal = false;
            } else {
                continue;
            }
        }
        if let Some(rest) = line.strip_prefix("diff --git ") {
            finish_file(&mut patch, &mut file);
            let (old, new) = git_header_paths(rest);
            let mut f = FilePatch::empty();
            f.old_path = old;
            f.new_path = new;
            file = Some(f);
            continue;
        }
        if line.starts_with("@@ ") {
            let Some(f) = file.as_mut() else {
                return Err(ParseError {
                    line: number,
                    message: "hunk outside a file".into(),
                });
            };
            let Some(h) = parse_hunk_header(line) else {
                return Err(ParseError {
                    line: number,
                    message: format!("malformed hunk header {line:?}"),
                });
            };
            let _ = f;
            let (ol, nl, os, ns) = (h.old_len, h.new_len, h.old_start.max(1), h.new_start.max(1));
            hunk = Some((h, ol, nl, os, ns));
            continue;
        }
        if let Some(rest) = line.strip_prefix("--- ") {
            // A `diff -u` file without a `diff --git` header, or the paths
            // of the current git file.
            let starts_new = match &file {
                None => true,
                Some(f) => !f.hunks.is_empty(),
            };
            if starts_new {
                finish_file(&mut patch, &mut file);
                file = Some(FilePatch::empty());
            }
            if let Some(f) = file.as_mut() {
                f.old_path = header_path(rest, "a/");
                if f.old_path.is_none() {
                    f.status = FileStatus::Added;
                }
            }
            continue;
        }
        let Some(f) = file.as_mut() else {
            continue;
        };
        if let Some(rest) = line.strip_prefix("+++ ") {
            f.new_path = header_path(rest, "b/");
            if f.new_path.is_none() {
                f.status = FileStatus::Deleted;
            }
        } else if let Some(mode) = line.strip_prefix("new file mode ") {
            f.status = FileStatus::Added;
            f.new_mode = Some(mode.to_string());
            f.old_path = None;
        } else if let Some(mode) = line.strip_prefix("deleted file mode ") {
            f.status = FileStatus::Deleted;
            f.old_mode = Some(mode.to_string());
            f.new_path = None;
        } else if let Some(mode) = line.strip_prefix("old mode ") {
            f.old_mode = Some(mode.to_string());
        } else if let Some(mode) = line.strip_prefix("new mode ") {
            f.new_mode = Some(mode.to_string());
        } else if let Some(path) = line.strip_prefix("rename from ") {
            f.status = FileStatus::Renamed;
            f.old_path = Some(unquote(path));
        } else if let Some(path) = line.strip_prefix("rename to ") {
            f.status = FileStatus::Renamed;
            f.new_path = Some(unquote(path));
        } else if let Some(path) = line.strip_prefix("copy from ") {
            f.status = FileStatus::Copied;
            f.old_path = Some(unquote(path));
        } else if let Some(path) = line.strip_prefix("copy to ") {
            f.status = FileStatus::Copied;
            f.new_path = Some(unquote(path));
        } else if let Some(value) = line.strip_prefix("similarity index ") {
            f.similarity = value.trim_end_matches('%').parse().ok();
        } else if let Some(rest) = line.strip_prefix("index ") {
            // `index abc..def 100644`: the trailing mode is both sides'.
            if let Some((_, mode)) = rest.split_once(' ') {
                f.old_mode.get_or_insert_with(|| mode.to_string());
                f.new_mode.get_or_insert_with(|| mode.to_string());
            }
        } else if line.starts_with("Binary files ") && line.ends_with(" differ") {
            f.binary = true;
        } else if line == "GIT binary patch" {
            f.binary = true;
            in_binary_literal = true;
        }
    }
    if let Some((h, old_left, new_left, ..)) = &hunk {
        if *old_left > 0 || *new_left > 0 {
            return Err(ParseError {
                line: input.lines().count(),
                message: format!(
                    "hunk {} ended early: expected {old_left} more old and {new_left} more new lines",
                    h.header()
                ),
            });
        }
    }
    finish_hunk(&mut file, &mut hunk);
    finish_file(&mut patch, &mut file);
    Ok(patch)
}

/// A note attached to a line of the new side, shown under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub path: String,
    /// The 1-based line on the new side.
    pub line: usize,
    pub level: Level,
    pub message: String,
}

impl Annotation {
    pub fn new(
        path: impl Into<String>,
        line: usize,
        level: Level,
        message: impl Into<String>,
    ) -> Self {
        Annotation {
            path: path.into(),
            line,
            level,
            message: message.into(),
        }
    }
}

/// Where file and line links point.
pub trait LinkProvider: Send + Sync {
    /// The URL for a file.
    fn file_url(&self, path: &str) -> Option<String>;
    /// The URL for a line of a file's new version.
    fn line_url(&self, path: &str, line: usize) -> Option<String>;
}

/// Links from URL templates. `{path}` and `{line}` are filled in, and so is
/// every `{name}` given through [`var`](Self::var):
///
/// ```
/// use rich_ext::diff::git::{LinkProvider, TemplateLinks};
///
/// let links = TemplateLinks::new("https://github.com/{owner}/{repo}/blob/{rev}/{path}#L{line}")
///     .file_template("https://github.com/{owner}/{repo}/blob/{rev}/{path}")
///     .var("owner", "octo")
///     .var("repo", "demo")
///     .var("rev", "main");
/// assert_eq!(
///     links.line_url("src/lib.rs", 7).as_deref(),
///     Some("https://github.com/octo/demo/blob/main/src/lib.rs#L7")
/// );
/// ```
#[derive(Clone, Debug, Default)]
pub struct TemplateLinks {
    line: String,
    file: Option<String>,
    vars: BTreeMap<String, String>,
}

impl TemplateLinks {
    /// Links lines through `line_template`. Files get no link until
    /// [`file_template`](Self::file_template) is set.
    pub fn new(line_template: impl Into<String>) -> Self {
        TemplateLinks {
            line: line_template.into(),
            file: None,
            vars: BTreeMap::new(),
        }
    }
    /// The template for whole-file links.
    pub fn file_template(mut self, template: impl Into<String>) -> Self {
        self.file = Some(template.into());
        self
    }
    /// A value for `{name}` in the templates.
    pub fn var(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.vars.insert(name.into(), value.into());
        self
    }
    fn fill(&self, template: &str, path: &str, line: Option<usize>) -> String {
        let mut out = template.replace("{path}", &encode_path(path));
        if let Some(line) = line {
            out = out.replace("{line}", &line.to_string());
        }
        for (name, value) in &self.vars {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}

fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        match c {
            '%' => out.push_str("%25"),
            ' ' => out.push_str("%20"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            _ => out.push(c),
        }
    }
    out
}

impl LinkProvider for TemplateLinks {
    fn file_url(&self, path: &str) -> Option<String> {
        self.file.as_ref().map(|t| self.fill(t, path, None))
    }
    fn line_url(&self, path: &str, line: usize) -> Option<String> {
        Some(self.fill(&self.line, path, Some(line)))
    }
}

/// Local `file://` or editor links through a [`Hyperlinker`].
impl LinkProvider for Hyperlinker {
    fn file_url(&self, path: &str) -> Option<String> {
        Hyperlinker::file_url(self, path, None, None)
    }
    fn line_url(&self, path: &str, line: usize) -> Option<String> {
        Hyperlinker::file_url(self, path, Some(line), None)
    }
}

/// A review-style rendering of a [`Patch`]: a file tree with per-file
/// counts, then each file's header and highlighted hunks, with annotations
/// under their lines and a changed-line summary at the end.
pub struct PatchView {
    patch: Patch,
    annotations: Vec<Annotation>,
    links: Option<Box<dyn LinkProvider>>,
    layout: Layout,
    line_numbers: bool,
    wrap: bool,
    highlight: bool,
    tree: bool,
    emphasis: bool,
}

impl fmt::Debug for PatchView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PatchView")
            .field("files", &self.patch.files.len())
            .field("annotations", &self.annotations.len())
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}

impl PatchView {
    pub fn new(patch: Patch) -> Self {
        PatchView {
            patch,
            annotations: Vec::new(),
            links: None,
            layout: Layout::Unified,
            line_numbers: true,
            wrap: true,
            highlight: true,
            tree: true,
            emphasis: true,
        }
    }
    /// Show `annotation` under its line.
    pub fn annotate(mut self, annotation: Annotation) -> Self {
        self.annotations.push(annotation);
        self
    }
    /// Show every annotation under its line.
    pub fn annotations(mut self, annotations: impl IntoIterator<Item = Annotation>) -> Self {
        self.annotations.extend(annotations);
        self
    }
    /// Link paths and new-side line numbers.
    pub fn links(mut self, provider: impl LinkProvider + 'static) -> Self {
        self.links = Some(Box::new(provider));
        self
    }
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }
    /// Syntax highlighting by path (default on).
    pub fn highlight(mut self, on: bool) -> Self {
        self.highlight = on;
        self
    }
    /// The file tree summary (default on).
    pub fn tree(mut self, show: bool) -> Self {
        self.tree = show;
        self
    }
    /// Word-level emphasis (default on).
    pub fn emphasis(mut self, on: bool) -> Self {
        self.emphasis = on;
        self
    }
    /// The patch shown.
    pub fn patch(&self) -> &Patch {
        &self.patch
    }

    fn blocks(&self, file: &FilePatch) -> Vec<Block> {
        let language = (self.highlight).then(|| language_for_path(file.path()));
        let mut blocks = Vec::new();
        for hunk in &file.hunks {
            // Highlight each side of the hunk as one text.
            let side = |keep: LineKind| {
                let lines: Vec<&str> = hunk
                    .lines
                    .iter()
                    .filter(|l| l.kind == LineKind::Context || l.kind == keep)
                    .map(|l| l.text.as_str())
                    .collect();
                let mut code = lines.join("\n");
                code.push('\n');
                highlight_lines(&code, language.as_deref())
            };
            let (mut old, mut new) = (
                side(LineKind::Removed).into_iter(),
                side(LineKind::Added).into_iter(),
            );
            let mut block = Block {
                header: Some(hunk.header()),
                rows: Vec::new(),
            };
            for line in &hunk.lines {
                let (kind, text) = match line.kind {
                    LineKind::Context => {
                        old.next();
                        (Kind::Context, new.next())
                    }
                    LineKind::Removed => (Kind::Delete, old.next()),
                    LineKind::Added => (Kind::Insert, new.next()),
                };
                let text = text.unwrap_or_else(|| Text::new(&line.text));
                let mut row = Row::new(kind, line.old_line, line.new_line, text);
                row.no_newline = line.no_newline;
                if let (Some(n), Some(path), Some(links)) =
                    (line.new_line, &file.new_path, &self.links)
                {
                    row.link = links.line_url(path, n);
                }
                if let (Some(n), Some(path)) = (line.new_line, &file.new_path) {
                    row.notes = self
                        .annotations
                        .iter()
                        .filter(|a| a.line == n && &a.path == path)
                        .map(|a| (a.level, a.message.clone()))
                        .collect();
                }
                block.rows.push(row);
            }
            if self.emphasis {
                render::emphasize(&mut block);
            }
            blocks.push(block);
        }
        blocks
    }

    fn path_segment(&self, path: &str, style: Style) -> Segment {
        let url = self.links.as_ref().and_then(|l| l.file_url(path));
        Segment::new(
            path.to_string(),
            Some(match url {
                Some(url) => style.with_link(url),
                None => style,
            }),
        )
    }

    fn counts(
        &self,
        console: &Console,
        file: &FilePatch,
        bar: usize,
        scale: usize,
    ) -> Vec<Segment> {
        if file.binary {
            return vec![Segment::new(
                "binary",
                Some(style(console, "diff.line_number")),
            )];
        }
        let mut out = vec![
            Segment::new(
                format!("+{}", file.additions),
                Some(style(console, "diff.added")),
            ),
            Segment::new(" ", None),
            Segment::new(
                format!("-{}", file.deletions),
                Some(style(console, "diff.removed")),
            ),
        ];
        let total = file.additions + file.deletions;
        if bar > 0 && total > 0 {
            // git --stat's bar, scaled to the busiest file.
            let cells = (total * bar).div_ceil(scale.max(1)).clamp(1, bar);
            let plus = (file.additions * cells).div_ceil(total).min(cells);
            let plus = if file.deletions > 0 && plus == cells {
                cells - 1
            } else {
                plus
            };
            out.push(Segment::new(" ", None));
            out.push(Segment::new(
                "+".repeat(plus),
                Some(style(console, "diff.added")),
            ));
            out.push(Segment::new(
                "-".repeat(cells - plus),
                Some(style(console, "diff.removed")),
            ));
        }
        out
    }

    fn tree_rows(&self, console: &Console, width: usize) -> Vec<Vec<Segment>> {
        #[derive(Default)]
        struct Dir<'a> {
            dirs: BTreeMap<String, Dir<'a>>,
            files: Vec<(String, &'a FilePatch)>,
        }
        let mut root = Dir::default();
        for file in &self.patch.files {
            let path = file.path();
            let mut parts: Vec<&str> = path.split('/').collect();
            let name = parts.pop().unwrap_or(path);
            let mut dir = &mut root;
            for part in parts {
                dir = dir.dirs.entry(part.to_string()).or_default();
            }
            let mut label = name.to_string();
            match file.status {
                FileStatus::Renamed | FileStatus::Copied => {
                    if let Some(old) = &file.old_path {
                        label = format!("{name} ({} from {old})", file.status.label());
                    }
                }
                FileStatus::Added | FileStatus::Deleted => {
                    label = format!("{name} ({})", file.status.label());
                }
                _ if file.mode_changed() => {
                    label = format!("{name} (mode {})", file.new_mode.as_deref().unwrap_or(""));
                }
                _ => {}
            }
            dir.files.push((label, file));
        }
        let (branch, last, pipe, blank) = if console.ascii_only() {
            ("|-- ", "`-- ", "|   ", "    ")
        } else {
            ("├── ", "└── ", "│   ", "    ")
        };
        // (prefix, label, file)
        let mut lines: Vec<(String, String, Option<&FilePatch>)> = Vec::new();
        fn walk<'a>(
            dir: &Dir<'a>,
            prefix: &str,
            glyphs: (&str, &str, &str, &str),
            lines: &mut Vec<(String, String, Option<&'a FilePatch>)>,
        ) {
            let count = dir.dirs.len() + dir.files.len();
            let mut i = 0;
            for (name, sub) in &dir.dirs {
                i += 1;
                // Collapse single-child directory chains: `src/diff/`.
                let mut label = format!("{name}/");
                let mut sub = sub;
                while sub.files.is_empty() && sub.dirs.len() == 1 {
                    let (n, s) = sub.dirs.iter().next().expect("one child");
                    label.push_str(&format!("{n}/"));
                    sub = s;
                }
                let is_last = i == count;
                lines.push((
                    format!("{prefix}{}", if is_last { glyphs.1 } else { glyphs.0 }),
                    label,
                    None,
                ));
                let next = format!("{prefix}{}", if is_last { glyphs.3 } else { glyphs.2 });
                walk(sub, &next, glyphs, lines);
            }
            for (label, file) in &dir.files {
                i += 1;
                let is_last = i == count;
                lines.push((
                    format!("{prefix}{}", if is_last { glyphs.1 } else { glyphs.0 }),
                    label.clone(),
                    Some(file),
                ));
            }
        }
        walk(&root, "", (branch, last, pipe, blank), &mut lines);
        let name_width = lines
            .iter()
            .filter(|l| l.2.is_some())
            .map(|(p, l, _)| cell_len(p) + cell_len(l))
            .max()
            .unwrap_or(0);
        let scale = self
            .patch
            .files
            .iter()
            .map(|f| f.additions + f.deletions)
            .max()
            .unwrap_or(0);
        let bar = 10.min(scale);
        let dim = style(console, "diff.line_number");
        let mut out = Vec::new();
        for (prefix, label, file) in lines {
            let mut row = vec![Segment::new(prefix.clone(), Some(dim.clone()))];
            match file {
                None => row.push(Segment::new(label, Some(style(console, "diff.header")))),
                Some(file) => {
                    let st = match file.status {
                        FileStatus::Added => style(console, "diff.added"),
                        FileStatus::Deleted => style(console, "diff.removed"),
                        _ => Style::new(),
                    };
                    let pad = name_width.saturating_sub(cell_len(&prefix) + cell_len(&label));
                    // Linked to the file, labelled with its name and status.
                    let mut seg = self.path_segment(file.path(), st);
                    seg.text = label;
                    row.push(seg);
                    row.push(Segment::new(" ".repeat(pad + 2), None));
                    row.extend(self.counts(console, file, bar, scale));
                }
            }
            out.extend(
                crate::layout::fit_segments(&row, width, crate::layout::OverflowPolicy::Crop)
                    .into_iter()
                    .map(render::trim_end),
            );
        }
        out
    }

    fn file_header(&self, console: &Console, file: &FilePatch, width: usize) -> Vec<Vec<Segment>> {
        let header = style(console, "diff.header");
        let mut row = vec![Segment::new(
            format!("{} ", file.status.label()),
            Some(style(console, "diff.hunk")),
        )];
        match (file.status, &file.old_path, &file.new_path) {
            (FileStatus::Renamed | FileStatus::Copied, Some(old), Some(new)) => {
                row.push(self.path_segment(old, header.clone()));
                row.push(Segment::new(" -> ", Some(header.clone())));
                row.push(self.path_segment(new, header.clone()));
                if let Some(similarity) = file.similarity {
                    row.push(Segment::new(
                        format!(" ({similarity}%)"),
                        Some(style(console, "diff.line_number")),
                    ));
                }
            }
            _ => row.push(self.path_segment(file.path(), header.clone())),
        }
        row.push(Segment::new("  ", None));
        row.extend(self.counts(console, file, 0, 0));
        let mut rows =
            crate::layout::fit_segments(&row, width, crate::layout::OverflowPolicy::Fold);
        if file.mode_changed() {
            rows.push(vec![Segment::new(
                format!(
                    "mode {} -> {}",
                    file.old_mode.as_deref().unwrap_or(""),
                    file.new_mode.as_deref().unwrap_or("")
                ),
                Some(style(console, "diff.line_number")),
            )]);
        }
        if file.binary {
            rows.push(vec![Segment::new(
                "Binary file differs",
                Some(style(console, "diff.line_number")),
            )]);
        }
        rows.into_iter().map(render::trim_end).collect()
    }
}

impl Renderable for PatchView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let width = options.max_width;
        if width == 0 || options.height == Some(0) {
            return Vec::new();
        }
        let mut rows: Vec<Vec<Segment>> = Vec::new();
        if self.tree && !self.patch.files.is_empty() {
            rows.extend(self.tree_rows(console, width));
            rows.push(Vec::new());
        }
        let opts = Options {
            layout: self.layout,
            line_numbers: self.line_numbers,
            wrap: self.wrap,
            titles: None,
        };
        for file in &self.patch.files {
            rows.extend(self.file_header(console, file, width));
            let blocks = self.blocks(file);
            rows.extend(render::render(console, &blocks, &opts, width));
            rows.push(Vec::new());
        }
        let (added, removed) = self.patch.stats();
        let n = self.patch.files.len();
        let summary = format!(
            "{n} file{} changed, {added} insertion{}(+), {removed} deletion{}(-)",
            if n == 1 { "" } else { "s" },
            if added == 1 { "" } else { "s" },
            if removed == 1 { "" } else { "s" },
        );
        rows.extend(render::banner(
            &summary,
            style(console, "diff.header"),
            width,
        ));
        render::join(rows, options.height)
    }
}

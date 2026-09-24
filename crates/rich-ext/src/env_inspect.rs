//! Inspect environment variables and PATH-like lists.
//!
//! [`EnvView`] shows variables as a name/value table, sorted by name. It masks
//! the values of names that look secret (see [`is_secret_name`]), masks the
//! secret part of any other value that looks like a credential (see
//! [`redact_value`]) and splits PATH-like values one entry per line.
//!
//! Masking is **best effort**: it recognises common names and value shapes,
//! not every secret. A secret in a variable with an ordinary name and an
//! ordinary-looking value is shown. Check the output before you share it. [`PathView`] checks one PATH-like
//! variable entry by entry: whether each exists, is a directory, repeats an
//! earlier entry or is empty.
//!
//! Both take the variables as data, so output is deterministic;
//! [`EnvView::from_process`] and [`PathView::from_process`] read `std::env`.
//! Filesystem checks go through [`FsProbe`], so tests can pass a fake.
//!
//! ```
//! use rich_ext::env_inspect::{EnvView, PathKind, PathStatus, PathView};
//! use std::path::Path;
//!
//! let env = EnvView::new([
//!     ("GITHUB_TOKEN".to_string(), "ghp_x".to_string()),
//!     ("AUTHOR".to_string(), "Ann".to_string()),
//! ]);
//! assert!(env.is_redacted("GITHUB_TOKEN"));
//! assert!(!env.is_redacted("AUTHOR"));
//!
//! let path = PathView::new("PATH", "/bin:/nope:/bin/", ':')
//!     .probe(|p: &Path| if p == Path::new("/bin") { PathKind::Directory } else { PathKind::Missing });
//! let statuses: Vec<PathStatus> = path.entries().into_iter().map(|e| e.status).collect();
//! assert_eq!(statuses, [PathStatus::Ok, PathStatus::Missing, PathStatus::Duplicate(1)]);
//! ```
//!
//! Styles come from the theme keys `env.name`, `env.redacted`, `env.ok`,
//! `env.warning` and `env.error`, with built-in fallbacks.

use std::path::Path;

use rich::table::ColumnOptions;
use rich::{Console, ConsoleOptions, Justify, Overflow, Renderable, Segment, Table, Text};

use crate::event::theme_style;
use crate::sanitize::is_bidi_control;
use crate::unicode_inspect::control_picture;

/// The separator between entries of `PATH` on this OS.
pub const OS_PATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

/// Name fragments that mark a variable as secret wherever they appear.
pub const SECRET_FRAGMENTS: &[&str] = &[
    "PASSWORD",
    "PASSWD",
    "SECRET",
    "TOKEN",
    "API_KEY",
    "APIKEY",
    "ACCESS_KEY",
    "PRIVATE_KEY",
    "CREDENTIAL",
    "AUTHORIZATION",
];

/// Name segments (between `_`, `-` or `.`) that mark a variable as secret
/// only as a whole segment, so `AUTHOR`, `KEYBOARD_LAYOUT` and `MONKEY` are
/// not masked but `GH_AUTH`, `STRIPE_KEY` and `DB_PASS` are.
pub const SECRET_SEGMENTS: &[&str] = &[
    "AUTH",
    "KEY",
    "PASS",
    "PWD",
    "DSN",
    "PASSPHRASE",
    "COOKIE",
    "JWT",
    "PRIVATEKEY",
];

/// Name segments that mark a variable as secret only as its *last* segment:
/// `FLASK_SESSION` is masked, `XDG_SESSION_TYPE` is not.
pub const SECRET_LAST_SEGMENTS: &[&str] = &["SESSION"];

/// Well-known variables whose names match a secret segment but whose values
/// are not secret: the shell's working directory and the desktop session name.
pub const PUBLIC_NAMES: &[&str] = &["PWD", "DESKTOP_SESSION"];

/// Whether `name` looks like it holds a secret. Case-insensitive, and `-`
/// counts as `_` (`API-KEY` is `API_KEY`).
///
/// A name is secret when it contains one of [`SECRET_FRAGMENTS`], has one of
/// [`SECRET_SEGMENTS`] as a whole segment, or ends in one of
/// [`SECRET_LAST_SEGMENTS`], and is not one of [`PUBLIC_NAMES`].
pub fn is_secret_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase().replace('-', "_");
    if PUBLIC_NAMES.contains(&upper.as_str()) {
        return false;
    }
    let segments: Vec<&str> = upper.split(['_', '.']).collect();
    SECRET_FRAGMENTS.iter().any(|f| upper.contains(f))
        || segments
            .iter()
            .any(|segment| SECRET_SEGMENTS.contains(segment))
        || segments
            .last()
            .is_some_and(|segment| SECRET_LAST_SEGMENTS.contains(segment))
}

/// `value` with the secret parts of anything that looks like a credential
/// masked as `********`, whatever the variable is called: the password in a
/// URL's `user:password@`, `key=value` pairs with a secret key, `Bearer`
/// tokens, well-known token prefixes (`ghp_`, `sk_live_`, …), AWS access key
/// ids and JWTs. These are [`crate::redact::Redactor::secrets`]' detectors,
/// and share their limits: best effort, not a guarantee.
///
/// ```
/// use rich_ext::env_inspect::redact_value;
///
/// assert_eq!(redact_value("postgres://admin:hunter2@db/x"), "postgres://admin:********@db/x");
/// assert_eq!(redact_value("/usr/bin"), "/usr/bin");
/// ```
pub fn redact_value(value: &str) -> String {
    static SECRETS: std::sync::OnceLock<crate::redact::Redactor> = std::sync::OnceLock::new();
    SECRETS
        .get_or_init(crate::redact::Redactor::secrets)
        .redact_str(value)
}

/// Case-insensitive match of `name` against a filter: a whole-name glob
/// when `pattern` contains `*` or `?`, a substring otherwise.
pub fn name_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let name = name.to_lowercase();
    if pattern.contains(['*', '?']) {
        let p: Vec<char> = pattern.chars().collect();
        let n: Vec<char> = name.chars().collect();
        glob(&p, &n)
    } else {
        name.contains(&pattern)
    }
}

fn glob(p: &[char], n: &[char]) -> bool {
    // Iterative wildcard match with single-star backtracking.
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// Whether a variable's value should be shown split on `separator`: its
/// name ends in `PATH`, or its value holds the separator and every
/// non-empty entry looks like a filesystem path (contains `/` or `\`, and
/// the value is not a URL).
pub fn is_path_like(name: &str, value: &str, separator: char) -> bool {
    if name.to_ascii_uppercase().ends_with("PATH") {
        return true;
    }
    if !value.contains(separator) || value.contains("://") {
        return false;
    }
    let mut entries = value.split(separator).filter(|e| !e.is_empty()).peekable();
    entries.peek().is_some() && entries.all(|e| e.contains(['/', '\\']))
}

/// Replace controls with visible pictures (`^[` when `ascii`), and bidi
/// controls with escapes (`\u{202e}`), so a value cannot reorder the table.
fn visible(s: &str, ascii: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match control_picture(c, ascii) {
            Some(picture) => out.push_str(&picture),
            None if is_bidi_control(c) => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            None => out.push(c),
        }
    }
    out
}

fn mask(value: &str, ascii: bool) -> String {
    let dots = if ascii {
        "******"
    } else {
        "••••••"
    };
    let n = value.chars().count();
    format!("{dots} ({n} char{})", if n == 1 { "" } else { "s" })
}

/// A renderable table of environment variables.
#[derive(Clone, Debug)]
pub struct EnvView {
    vars: Vec<(String, String)>,
    filter: Option<String>,
    redact: bool,
    separator: char,
}

impl EnvView {
    /// Show `vars`, sorted by name.
    pub fn new(vars: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut vars: Vec<(String, String)> = vars.into_iter().collect();
        vars.sort();
        EnvView {
            vars,
            filter: None,
            redact: true,
            separator: OS_PATH_SEPARATOR,
        }
    }

    /// Show this process's environment (non-UTF-8 is decoded lossily).
    pub fn from_process() -> Self {
        Self::new(std::env::vars_os().map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        }))
    }

    /// Show only names matching `pattern` (see [`name_matches`]).
    pub fn filter(mut self, pattern: impl Into<String>) -> Self {
        self.filter = Some(pattern.into());
        self
    }

    /// Mask the values of secret-looking names, and the secret parts of
    /// credential-looking values (default on; see [`is_secret_name`] and
    /// [`redact_value`]). Best effort: check the output before sharing it.
    pub fn redact(mut self, on: bool) -> Self {
        self.redact = on;
        self
    }

    /// The separator PATH-like values are split on (default the OS one).
    pub fn separator(mut self, separator: char) -> Self {
        self.separator = separator;
        self
    }

    /// Whether the value of `name` is masked whole (by its name; a value
    /// can also be masked in part by its content, see [`redact_value`]).
    pub fn is_redacted(&self, name: &str) -> bool {
        self.redact && is_secret_name(name)
    }

    /// The variables shown after filtering, sorted by name.
    pub fn vars(&self) -> Vec<(&str, &str)> {
        self.vars
            .iter()
            .filter(|(name, _)| {
                self.filter
                    .as_deref()
                    .is_none_or(|pattern| name_matches(pattern, name))
            })
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect()
    }

    /// How many variables are shown.
    pub fn len(&self) -> usize {
        self.vars().len()
    }

    /// Whether no variable is shown.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Renderable for EnvView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let ascii = console.ascii_only();
        let vars = self.vars();
        if vars.is_empty() {
            let mut out = Text::styled(
                "no matching environment variables",
                theme_style(console, "env.redacted", "dim"),
            )
            .rich_render(console, options);
            out.push(Segment::line());
            return out;
        }
        let name_style = theme_style(console, "env.name", "bold cyan");
        let redacted_style = theme_style(console, "env.redacted", "dim");
        let mut table = Table::new();
        for header in ["Name", "Value"] {
            table.add_column_with(
                Text::new(header),
                ColumnOptions {
                    overflow: Overflow::Fold,
                    ..ColumnOptions::default()
                },
            );
        }
        for (name, value) in vars {
            let masked_whole = self.is_redacted(name);
            let redacted;
            let value = if self.redact && !masked_whole {
                redacted = redact_value(value);
                redacted.as_str()
            } else {
                value
            };
            let shown = if masked_whole {
                Text::styled(mask(value, ascii), redacted_style.clone())
            } else if is_path_like(name, value, self.separator) && !value.is_empty() {
                let entries: Vec<String> = value
                    .split(self.separator)
                    .map(|e| visible(e, ascii))
                    .collect();
                Text::new(entries.join("\n"))
            } else {
                Text::new(visible(value, ascii))
            };
            table.add_row_text(vec![
                Text::styled(visible(name, ascii), name_style.clone()),
                shown,
            ]);
        }
        table.rich_render(console, options)
    }
}

/// What a path is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    Directory,
    /// A file or anything else that is not a directory.
    NotADirectory,
    Missing,
}

/// Looks paths up on a filesystem.
pub trait FsProbe {
    fn kind(&self, path: &Path) -> PathKind;
}

impl<F: Fn(&Path) -> PathKind> FsProbe for F {
    fn kind(&self, path: &Path) -> PathKind {
        self(path)
    }
}

/// The real filesystem (follows symlinks).
#[derive(Clone, Copy, Debug, Default)]
pub struct RealFs;

impl FsProbe for RealFs {
    fn kind(&self, path: &Path) -> PathKind {
        match std::fs::metadata(path) {
            Ok(meta) if meta.is_dir() => PathKind::Directory,
            Ok(_) => PathKind::NotADirectory,
            Err(_) => PathKind::Missing,
        }
    }
}

/// The verdict on one PATH entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathStatus {
    Ok,
    Missing,
    NotADirectory,
    /// The same as the entry with this 1-based index.
    Duplicate(usize),
    /// An empty entry (the current directory on Unix).
    Empty,
}

impl PathStatus {
    /// The label shown in the table.
    pub fn label(self) -> String {
        match self {
            PathStatus::Ok => "ok".into(),
            PathStatus::Missing => "missing".into(),
            PathStatus::NotADirectory => "not a directory".into(),
            PathStatus::Duplicate(n) => format!("duplicate of #{n}"),
            PathStatus::Empty => "empty".into(),
        }
    }

    /// Whether this is a problem worth reporting.
    pub fn is_problem(self) -> bool {
        self != PathStatus::Ok
    }
}

/// One checked entry of a PATH-like variable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    /// 1-based position.
    pub index: usize,
    pub entry: String,
    pub status: PathStatus,
}

/// A renderable, checked table of one PATH-like variable.
pub struct PathView {
    name: String,
    value: String,
    separator: char,
    case_insensitive: bool,
    probe: Box<dyn FsProbe>,
}

impl std::fmt::Debug for PathView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathView")
            .field("name", &self.name)
            .field("value", &self.value)
            .field("separator", &self.separator)
            .field("case_insensitive", &self.case_insensitive)
            .finish_non_exhaustive()
    }
}

impl PathView {
    /// Check `value`, split on `separator`, against the real filesystem.
    /// Duplicates compare case-insensitively on Windows only.
    pub fn new(name: impl Into<String>, value: impl Into<String>, separator: char) -> Self {
        PathView {
            name: name.into(),
            value: value.into(),
            separator,
            case_insensitive: cfg!(windows),
            probe: Box::new(RealFs),
        }
    }

    /// Check the process variable `name` with the OS separator, or `None`
    /// when it is unset.
    pub fn from_process(name: &str) -> Option<Self> {
        let value = std::env::var_os(name)?;
        Some(Self::new(
            name,
            value.to_string_lossy().into_owned(),
            OS_PATH_SEPARATOR,
        ))
    }

    /// Look entries up with `probe` instead of the real filesystem.
    pub fn probe(mut self, probe: impl FsProbe + 'static) -> Self {
        self.probe = Box::new(probe);
        self
    }

    /// Compare entries for duplicates ignoring case (default: on Windows).
    pub fn case_insensitive(mut self, on: bool) -> Self {
        self.case_insensitive = on;
        self
    }

    /// The variable's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn normalise(&self, entry: &str) -> String {
        let trimmed = entry.trim_end_matches(['/', '\\']);
        let trimmed = if trimmed.is_empty() { entry } else { trimmed };
        if self.case_insensitive {
            trimmed.to_lowercase()
        } else {
            trimmed.to_owned()
        }
    }

    /// Every entry with its status. An empty value has no entries.
    pub fn entries(&self) -> Vec<PathEntry> {
        if self.value.is_empty() {
            return Vec::new();
        }
        let mut seen: Vec<(String, usize)> = Vec::new();
        self.value
            .split(self.separator)
            .enumerate()
            .map(|(i, entry)| {
                let index = i + 1;
                let status = if entry.is_empty() {
                    PathStatus::Empty
                } else {
                    let key = self.normalise(entry);
                    if let Some((_, first)) = seen.iter().find(|(k, _)| *k == key) {
                        PathStatus::Duplicate(*first)
                    } else {
                        seen.push((key, index));
                        match self.probe.kind(Path::new(entry)) {
                            PathKind::Directory => PathStatus::Ok,
                            PathKind::NotADirectory => PathStatus::NotADirectory,
                            PathKind::Missing => PathStatus::Missing,
                        }
                    }
                };
                PathEntry {
                    index,
                    entry: entry.to_owned(),
                    status,
                }
            })
            .collect()
    }
}

impl Renderable for PathView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let ascii = console.ascii_only();
        let ok = theme_style(console, "env.ok", "green");
        let warning = theme_style(console, "env.warning", "yellow");
        let error = theme_style(console, "env.error", "bold red");
        let mut table = Table::new();
        let column = |justify: Justify| ColumnOptions {
            justify,
            overflow: Overflow::Fold,
            ..ColumnOptions::default()
        };
        table.add_column_with(Text::new("#"), column(Justify::Right));
        table.add_column_with(Text::new(visible(&self.name, ascii)), column(Justify::Left));
        table.add_column_with(Text::new("Status"), column(Justify::Left));
        for entry in self.entries() {
            let style = match entry.status {
                PathStatus::Ok => ok.clone(),
                PathStatus::Duplicate(_) | PathStatus::Empty => warning.clone(),
                PathStatus::Missing | PathStatus::NotADirectory => error.clone(),
            };
            table.add_row_text(vec![
                Text::new(entry.index.to_string()),
                Text::new(visible(&entry.entry, ascii)),
                Text::styled(entry.status.label(), style),
            ]);
        }
        table.rich_render(console, options)
    }
}

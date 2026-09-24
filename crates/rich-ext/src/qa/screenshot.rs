//! Screenshot matrices and an approval workflow.
//!
//! [`Screenshot::capture`] renders one renderable across a [`Matrix`] of
//! widths, colour depths and unicode on/off, through explicit capabilities
//! (never the environment). Each [`Shot`] is keyed
//! `name@<width>.<colour>.<unicode|ascii>`, e.g. `table@80.truecolor.unicode`;
//! colours are `none`, `ansi16`, `ansi256` and `truecolor`.
//!
//! # Approval files
//!
//! [`Approvals`] keeps approved shots under `<dir>/<name>/<key>.txt`:
//!
//! * colour `none`: the plain text exactly as rendered, one line per row,
//!   ending in a newline;
//! * any colour: the ANSI output with every control escaped so the file is
//!   printable and diffs line by line: ESC is `\e`, a backslash `\\`, and any
//!   other C0 control (except the newline) or DEL `\xHH`. Nothing else is
//!   touched, so `\e[1;31mred\e[0m` reads as the escape it is.
//!
//! [`Approvals::check`] compares shots to those files. A mismatch or a
//! missing file writes `<key>.new` beside it; approving (the `RICH_APPROVE=1`
//! environment variable, [`Approvals::approving`] or
//! [`Approvals::approve_all`]) turns `.new` files into `.txt` files. A match
//! removes a stale `.new`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rich::Renderable;

use super::{depth_key, NoHeight, Probe};
use crate::capabilities::ColorDepth;
use crate::diff::DiffView;
use crate::testing::RenderSnapshot;

/// Which configurations to render. Every combination is captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    /// Default `[40, 80, 120]`: narrow, standard and wide terminals.
    pub widths: Vec<usize>,
    /// Default `[TrueColor, None]`: full colour and the `NO_COLOR` fallback.
    pub color: Vec<ColorDepth>,
    /// Default `[true, false]`: unicode and ASCII-only terminals.
    pub unicode: Vec<bool>,
    /// Whether OSC 8 links are kept (default `false`: most captures are
    /// read in files and CI logs that do not render them).
    pub hyperlinks: bool,
    /// `options.height` imposed on the renderable. Default `None`, as a
    /// top-level print does (a `Panel` does not grow to fill a screen); the
    /// snapshot then records a height of 25.
    pub height: Option<usize>,
}

impl Default for Matrix {
    fn default() -> Self {
        Matrix {
            widths: vec![40, 80, 120],
            color: vec![ColorDepth::TrueColor, ColorDepth::None],
            unicode: vec![true, false],
            hyperlinks: false,
            height: None,
        }
    }
}

impl Matrix {
    /// One configuration: `width`, truecolor, unicode.
    pub fn single(width: usize) -> Self {
        Matrix {
            widths: vec![width],
            color: vec![ColorDepth::TrueColor],
            unicode: vec![true],
            ..Matrix::default()
        }
    }
    pub fn widths(mut self, widths: impl Into<Vec<usize>>) -> Self {
        self.widths = widths.into();
        self
    }
    pub fn color(mut self, color: impl Into<Vec<ColorDepth>>) -> Self {
        self.color = color.into();
        self
    }
    pub fn unicode(mut self, unicode: impl Into<Vec<bool>>) -> Self {
        self.unicode = unicode.into();
        self
    }
    pub fn hyperlinks(mut self, on: bool) -> Self {
        self.hyperlinks = on;
        self
    }
    pub fn height(mut self, height: Option<usize>) -> Self {
        self.height = height;
        self
    }
    /// Number of shots per capture.
    pub fn len(&self) -> usize {
        self.widths.len() * self.color.len() * self.unicode.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// How a shot is stored in its approval file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Plain text, stored as is.
    Plain,
    /// ANSI text with controls escaped (see the [module docs](self)).
    Escaped,
}

impl Encoding {
    /// `text` as file contents.
    pub fn encode(self, text: &str) -> String {
        let mut out = String::with_capacity(text.len() + 1);
        match self {
            Encoding::Plain => out.push_str(text),
            Encoding::Escaped => {
                for c in text.chars() {
                    match c {
                        '\x1b' => out.push_str("\\e"),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push('\n'),
                        c if (c as u32) < 0x20 || c == '\x7f' => {
                            out.push_str(&format!("\\x{:02x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
            }
        }
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out
    }

    /// File contents back to text; the inverse of [`encode`](Self::encode)
    /// for text without a trailing newline.
    pub fn decode(self, contents: &str) -> String {
        let contents = contents.replace("\r\n", "\n");
        let contents = contents.strip_suffix('\n').unwrap_or(&contents);
        if self == Encoding::Plain {
            return contents.to_owned();
        }
        let mut out = String::with_capacity(contents.len());
        let mut chars = contents.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('e') => out.push('\x1b'),
                Some('\\') => out.push('\\'),
                Some('x') => {
                    let hex: String = chars.by_ref().take(2).collect();
                    match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                        Some(c) => out.push(c),
                        None => {
                            out.push_str("\\x");
                            out.push_str(&hex);
                        }
                    }
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        }
        out
    }
}

/// One rendered configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Shot {
    /// `name@80.truecolor.unicode`.
    pub key: String,
    /// The capture name (the approval subdirectory).
    pub name: String,
    pub width: usize,
    pub color: ColorDepth,
    pub unicode: bool,
    pub encoding: Encoding,
    /// Plain text for colour `none`, ANSI otherwise; no trailing newline.
    pub text: String,
    pub snapshot: RenderSnapshot,
}

impl Shot {
    /// A shot from a finished snapshot: colour `none` keeps the plain text
    /// (stored as is), any other colour the ANSI text (stored escaped).
    pub fn from_snapshot(
        name: &str,
        key: String,
        snapshot: RenderSnapshot,
        color: ColorDepth,
        unicode: bool,
    ) -> Self {
        let ansi = color != ColorDepth::None;
        let text = if ansi {
            snapshot.ansi.clone()
        } else {
            snapshot.plain.clone()
        };
        let text = text.strip_suffix('\n').unwrap_or(&text).to_owned();
        Shot {
            key,
            name: name.to_owned(),
            width: snapshot.width,
            color,
            unicode,
            encoding: if ansi {
                Encoding::Escaped
            } else {
                Encoding::Plain
            },
            text,
            snapshot,
        }
    }

    /// The approval file contents.
    pub fn file_contents(&self) -> String {
        self.encoding.encode(&self.text)
    }
}

/// Captures screenshots.
pub struct Screenshot;

impl Screenshot {
    /// Render `renderable` in every configuration of `matrix`, widths
    /// outermost, then colours, then unicode.
    pub fn capture(name: &str, renderable: &dyn Renderable, matrix: &Matrix) -> Vec<Shot> {
        let mut shots = Vec::with_capacity(matrix.len());
        for &width in &matrix.widths {
            for &color in &matrix.color {
                for &unicode in &matrix.unicode {
                    let mut probe = Probe::new(width);
                    probe.color = color;
                    probe.unicode = unicode;
                    probe.hyperlinks = matrix.hyperlinks;
                    probe.height = matrix.height.or(Some(25));
                    let target = probe.target();
                    let snapshot = match matrix.height {
                        Some(_) => RenderSnapshot::capture(&target, renderable),
                        None => RenderSnapshot::capture(&target, &NoHeight(renderable)),
                    };
                    let key = format!(
                        "{name}@{width}.{}.{}",
                        depth_key(color),
                        if unicode { "unicode" } else { "ascii" }
                    );
                    shots.push(Shot::from_snapshot(name, key, snapshot, color, unicode));
                }
            }
        }
        shots
    }
}

/// A shot that differs from its approved file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub key: String,
    /// The approved file.
    pub path: PathBuf,
    /// Approved text, decoded.
    pub approved: String,
    /// The new render.
    pub actual: String,
    pub encoding: Encoding,
}

impl Mismatch {
    /// A diff of approved → actual; ANSI shots report style-only lines.
    pub fn view(&self) -> DiffView {
        let view = match self.encoding {
            Encoding::Plain => DiffView::new(&self.approved, &self.actual),
            Encoding::Escaped => DiffView::ansi(&self.approved, &self.actual),
        };
        view.titles("approved", "actual")
    }
}

/// What [`Approvals::check`] found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Keys equal to their approved file.
    pub matched: Vec<String>,
    /// Keys written as approved during this check (approving mode).
    pub approved: Vec<String>,
    /// Keys whose approved file differs; a `.new` file was written.
    pub mismatched: Vec<Mismatch>,
    /// Keys with no approved file; a `.new` file was written.
    pub missing: Vec<String>,
}

impl Outcome {
    /// No mismatches and nothing missing.
    pub fn is_ok(&self) -> bool {
        self.mismatched.is_empty() && self.missing.is_empty()
    }

    /// A failure report: a summary, then each mismatch's diff rendered as
    /// the `diff::assert` helpers render theirs.
    pub fn report(&self) -> String {
        let mut out = format!(
            "{} matched, {} mismatched, {} missing",
            self.matched.len() + self.approved.len(),
            self.mismatched.len(),
            self.missing.len()
        );
        for key in &self.missing {
            out.push_str(&format!("\nmissing: {key}"));
        }
        let report = crate::diff::assert::Report::from_env();
        for mismatch in &self.mismatched {
            out.push_str(&format!("\nmismatch: {}\n", mismatch.key));
            out.push_str(&report.render(&mismatch.view()));
        }
        out
    }
}

/// Approved screenshots in a directory.
#[derive(Clone, Debug)]
pub struct Approvals {
    dir: PathBuf,
    approve: bool,
}

/// Whether `RICH_APPROVE` asks for approval (`1`, `true`, `yes`, `on`).
fn approve_from_env() -> bool {
    std::env::var("RICH_APPROVE")
        .ok()
        .and_then(|v| crate::capabilities::parse_bool(&v))
        .unwrap_or(false)
}

/// A file-system-safe directory name for a capture name: one normal path
/// component. A name that is empty or only dots (`.`, `..`) would name the
/// directory itself or its parent, so its dots become `_` too.
fn safe_name(name: &str) -> String {
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '@') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.chars().all(|c| c == '.') {
        "_".repeat(safe.len().max(1))
    } else {
        safe
    }
}

impl Approvals {
    /// Approvals under `dir`, approving when `RICH_APPROVE` is set.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Approvals {
            dir: dir.into(),
            approve: approve_from_env(),
        }
    }

    /// Accept every shot as approved while checking (instead of `RICH_APPROVE`).
    pub fn approving(mut self, approve: bool) -> Self {
        self.approve = approve;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// `<dir>/<name>/<key>.txt`.
    pub fn path(&self, shot: &Shot) -> PathBuf {
        self.dir
            .join(safe_name(&shot.name))
            .join(format!("{}.txt", safe_name(&shot.key)))
    }

    /// `<dir>/<name>/<key>.new`.
    pub fn pending_path(&self, shot: &Shot) -> PathBuf {
        self.path(shot).with_extension("new")
    }

    /// Compare `shots` with their approved files (see the [module docs](self)).
    pub fn check(&self, shots: &[Shot]) -> io::Result<Outcome> {
        let mut outcome = Outcome::default();
        for shot in shots {
            let path = self.path(shot);
            let pending = self.pending_path(shot);
            let contents = shot.file_contents();
            let existing = match fs::read_to_string(&path) {
                Ok(text) => Some(text),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            };
            let same = existing
                .as_deref()
                .is_some_and(|old| shot.encoding.decode(old) == shot.text);
            if same {
                remove_if_exists(&pending)?;
                outcome.matched.push(shot.key.clone());
                continue;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            if self.approve {
                fs::write(&path, &contents)?;
                remove_if_exists(&pending)?;
                outcome.approved.push(shot.key.clone());
                continue;
            }
            fs::write(&pending, &contents)?;
            match existing {
                Some(old) => outcome.mismatched.push(Mismatch {
                    key: shot.key.clone(),
                    path,
                    approved: shot.encoding.decode(&old),
                    actual: shot.text.clone(),
                    encoding: shot.encoding,
                }),
                None => outcome.missing.push(shot.key.clone()),
            }
        }
        Ok(outcome)
    }

    /// Turn every pending `.new` file under the directory into its approved
    /// `.txt`; returns the approved paths, sorted.
    pub fn approve_all(&self) -> io::Result<Vec<PathBuf>> {
        let mut approved = Vec::new();
        if !self.dir.exists() {
            return Ok(approved);
        }
        let mut stack = vec![self.dir.clone()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "new") {
                    let target = path.with_extension("txt");
                    fs::rename(&path, &target)?;
                    approved.push(target);
                }
            }
        }
        approved.sort();
        Ok(approved)
    }
}

fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Where [`assert_screenshots`] keeps approvals: `RICH_SCREENSHOT_DIR`, else
/// `$CARGO_MANIFEST_DIR/tests/screenshots`, else `tests/screenshots`.
pub fn default_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RICH_SCREENSHOT_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("tests")
        .join("screenshots")
}

/// Capture `renderable` over the default [`Matrix`] and check it against
/// [`default_dir`]; panic with the rendered diffs when anything differs or
/// is missing. `RICH_APPROVE=1` accepts the new output instead.
#[track_caller]
pub fn assert_screenshots(name: &str, renderable: &dyn Renderable) {
    assert_screenshots_with(
        &Approvals::new(default_dir()),
        name,
        renderable,
        &Matrix::default(),
    );
}

/// [`assert_screenshots`] with explicit approvals and matrix.
#[track_caller]
pub fn assert_screenshots_with(
    approvals: &Approvals,
    name: &str,
    renderable: &dyn Renderable,
    matrix: &Matrix,
) {
    let shots = Screenshot::capture(name, renderable, matrix);
    let outcome = match approvals.check(&shots) {
        Ok(outcome) => outcome,
        Err(e) => panic!(
            "screenshots `{name}`: cannot use {}: {e}",
            approvals.dir().display()
        ),
    };
    if !outcome.is_ok() {
        panic!(
            "screenshots `{name}` differ from {}: {}\n\
             review the .new files, then rerun with RICH_APPROVE=1 to accept them",
            approvals.dir().display(),
            outcome.report()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::safe_name;
    use std::path::{Component, Path};

    #[test]
    fn safe_names_never_leave_the_directory() {
        for name in ["..", ".", "", "...", "a/../..", "../x", "..\\..", "/", "\0"] {
            let safe = safe_name(name);
            let components: Vec<Component<'_>> = Path::new(&safe).components().collect();
            assert!(
                matches!(components.as_slice(), [Component::Normal(_)]),
                "{name:?} became {safe:?}"
            );
            assert!(!safe.chars().all(|c| c == '.'), "{name:?} became {safe:?}");
        }
        assert_eq!(safe_name("table@80.none"), "table@80.none");
    }
}

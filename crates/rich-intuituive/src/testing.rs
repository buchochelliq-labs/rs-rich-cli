//! Snapshot assertions for headless app tests.
//!
//! [`assert_screen!`](crate::assert_screen!) compares what an app shows
//! with a file kept next to the tests, `tests/snapshots/<name>.txt`: the
//! screen as plain text, one line a row, trailing spaces trimmed. When it
//! differs, the test fails with a unified diff of the two. Run the tests
//! with `UPDATE_SNAPSHOTS=1` (the variable every snapshot test in this
//! repository reads) to write the files instead, then review the change
//! with `git diff`.
//!
//! [`assert_screen_styled!`](crate::assert_screen_styled!) keeps
//! `<name>.styled.txt` as well, each row as console markup
//! (`[bold red]Error[/] 3 fields`), for a test about colours and text
//! styles.
//!
//! ```no_run
//! use intuituive::assert_screen;
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let count = signal(0);
//!     text!("Count: {count}").on_key("+", move |_| count.update(|c| *c += 1))
//! });
//! let mut driver = app.driver(20, 1);
//! driver.press("+ +");
//! assert_screen!(driver, "counter_after_two");
//! ```
//!
//! [`assert_screen_svg!`](crate::assert_screen_svg!) keeps the screen's
//! SVG picture, `<name>.svg`, beside its styled rows, and on a difference
//! writes an HTML report showing both pictures.
//!
//! The screen can be a [`Driver`], a [`Screen`], or the rows
//! [`App::render_with`](crate::App::render_with) returns.
//! [`assert_snapshot`] compares any text with any file.

use std::path::Path;

use crate::app::Driver;
use crate::screen::Screen;

/// The variable that makes the assertions write their files instead of
/// comparing.
pub const UPDATE_VAR: &str = "UPDATE_SNAPSHOTS";

/// Something with rows of plain text: a [`Driver`] (its last frame), a
/// [`Screen`], or rows already taken.
pub trait ScreenRows {
    fn plain_rows(&self) -> Vec<String>;
}

impl ScreenRows for Screen {
    fn plain_rows(&self) -> Vec<String> {
        self.plain()
    }
}

impl ScreenRows for Driver {
    fn plain_rows(&self) -> Vec<String> {
        self.screen().plain()
    }
}

impl ScreenRows for [String] {
    fn plain_rows(&self) -> Vec<String> {
        self.to_vec()
    }
}

impl ScreenRows for Vec<String> {
    fn plain_rows(&self) -> Vec<String> {
        self.clone()
    }
}

impl<T: ScreenRows + ?Sized> ScreenRows for &T {
    fn plain_rows(&self) -> Vec<String> {
        (**self).plain_rows()
    }
}

/// Something with styled cells: a [`Driver`] or a [`Screen`].
pub trait StyledRows {
    fn screen_cells(&self) -> &Screen;
}

impl StyledRows for Screen {
    fn screen_cells(&self) -> &Screen {
        self
    }
}

impl StyledRows for Driver {
    fn screen_cells(&self) -> &Screen {
        self.screen()
    }
}

impl<T: StyledRows + ?Sized> StyledRows for &T {
    fn screen_cells(&self) -> &Screen {
        (**self).screen_cells()
    }
}

/// The rows as a snapshot stores them: each without trailing spaces, one
/// a line, every row kept (blank ones too, so the height shows).
pub fn plain(screen: &impl ScreenRows) -> String {
    let mut out = String::new();
    for row in screen.plain_rows() {
        out.push_str(row.trim_end());
        out.push('\n');
    }
    out
}

/// The rows as console markup: each run of cells in one style as
/// `[style]text[/]`, unstyled text as it is (escaped), and blank unstyled
/// cells at the end of a row trimmed.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| label("[bold]Hi[/] there"));
/// let mut driver = app.driver(12, 1);
/// driver.settle();
/// assert_eq!(intuituive::testing::styled(&driver), "[bold]Hi[/] there\n");
/// ```
pub fn styled(screen: &impl StyledRows) -> String {
    let mut out = String::new();
    for line in screen.screen_cells().lines() {
        let mut row = String::new();
        // Trailing blanks with no style say nothing.
        let mut pending = String::new();
        for segment in &line {
            let style = segment
                .style
                .as_ref()
                .map(rich::Style::definition)
                .filter(|definition| definition != "none");
            let text = rich::markup::escape(&segment.text);
            match style {
                None => {
                    let shown = text.trim_end();
                    if !shown.is_empty() {
                        row.push_str(&std::mem::take(&mut pending));
                        row.push_str(shown);
                    }
                    pending.push_str(&text[shown.len()..]);
                }
                Some(style) => {
                    row.push_str(&std::mem::take(&mut pending));
                    row.push_str(&format!("[{style}]{text}[/]"));
                }
            }
        }
        out.push_str(&row);
        out.push('\n');
    }
    out
}

/// Compare `actual` with the snapshot file at `path`, or write it there
/// when `UPDATE_SNAPSHOTS` is set. The error says what differs, as a
/// unified diff, and how to accept the change. Line endings, trailing
/// spaces and a byte-order mark in the file are not compared, so a
/// checkout that turns `\n` into `\r\n`, or an editor that trims lines or
/// saves with a BOM, changes nothing.
pub fn check_snapshot(path: impl AsRef<Path>, actual: &str) -> Result<(), String> {
    check(
        path.as_ref(),
        actual,
        std::env::var_os(UPDATE_VAR).is_some(),
    )
}

fn check(path: &Path, actual: &str, update: bool) -> Result<(), String> {
    if update {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        return std::fs::write(path, actual)
            .map_err(|e| format!("cannot write {}: {e}", path.display()));
    }
    let expected = std::fs::read_to_string(path).map_err(|e| {
        format!(
            "missing snapshot {} ({e}); run with {UPDATE_VAR}=1 to write it",
            path.display()
        )
    })?;
    let (expected, actual) = (normalize(&expected), normalize(actual));
    if expected == actual {
        return Ok(());
    }
    let diff = rich_ext::diff::TextDiff::new(&expected, &actual).unified("expected", "actual");
    // Where the test runs (the crate's directory under cargo), the path
    // as the repository shows it.
    let here = std::env::current_dir().ok();
    let shown = here
        .as_deref()
        .and_then(|here| path.strip_prefix(here).ok())
        .unwrap_or(path);
    Err(format!(
        "snapshot {} differs; run with {UPDATE_VAR}=1 to accept the change\n{diff}",
        shown.display()
    ))
}

/// Lines without `\r` or trailing spaces, each ending in a line break, and
/// without the byte-order mark an editor may put first.
fn normalize(text: &str) -> String {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    text.lines()
        .map(|line| format!("{}\n", line.trim_end()))
        .collect()
}

/// [`check_snapshot`], panicking with its message.
#[track_caller]
pub fn assert_snapshot(path: impl AsRef<Path>, actual: &str) {
    if let Err(message) = check_snapshot(path, actual) {
        panic!("{message}");
    }
}

/// Where [`check_svg_snapshot`] writes its report when a snapshot differs:
/// `intuituive-snapshots` in `$CARGO_TARGET_DIR`, else in the `target`
/// directory beside the nearest `Cargo.lock` above the current directory
/// (the workspace's, under `cargo test`).
pub fn report_dir() -> std::path::PathBuf {
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let here = std::env::current_dir().unwrap_or_default();
            here.ancestors()
                .find(|dir| dir.join("Cargo.lock").is_file())
                .unwrap_or(&here)
                .join("target")
        });
    target.join("intuituive-snapshots")
}

/// Compare a screen's [styled](styled) rows with `<dir>/<name>.styled.txt`
/// and keep its SVG picture beside it, `<dir>/<name>.svg`, for review.
/// Pass or fail comes from the styled text alone, since an SVG's bytes
/// change with fonts and versions. When it differs, the error names an
/// HTML report in [`report_dir`] showing the snapshot's SVG and the
/// screen's side by side. With `UPDATE_SNAPSHOTS` set, both files are
/// written instead; a missing SVG is written whenever the text matches.
pub fn check_svg_snapshot(dir: &Path, name: &str, screen: &impl StyledRows) -> Result<(), String> {
    let text = styled(screen);
    let svg = screen
        .screen_cells()
        .to_svg(&rich_ext::frame::SvgOptions::default().title(name));
    let (text_path, svg_path) = (
        dir.join(format!("{name}.styled.txt")),
        dir.join(format!("{name}.svg")),
    );
    let update = std::env::var_os(UPDATE_VAR).is_some();
    match check(&text_path, &text, update) {
        Ok(()) => {
            if update || !svg_path.exists() {
                std::fs::write(&svg_path, &svg)
                    .map_err(|e| format!("cannot write {}: {e}", svg_path.display()))?;
            }
            Ok(())
        }
        Err(message) => {
            let old = std::fs::read_to_string(&svg_path).ok();
            match write_report(&report_dir(), name, old.as_deref(), &svg, &message) {
                Ok(report) => Err(format!("{message}\nsee {}", report.display())),
                Err(error) => Err(format!("{message}\n(no report: {error})")),
            }
        }
    }
}

/// [`check_svg_snapshot`], panicking with its message.
#[track_caller]
pub fn assert_svg_snapshot(dir: &Path, name: &str, screen: &impl StyledRows) {
    if let Err(message) = check_svg_snapshot(dir, name, screen) {
        panic!("{message}");
    }
}

/// Write `<name>.part.html` (the two pictures and the difference) into
/// `dir`, and `report.html` again from every part there. The report's path.
fn write_report(
    dir: &Path,
    name: &str,
    old: Option<&str>,
    new: &str,
    message: &str,
) -> std::io::Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let old = old.map_or_else(
        || "<p>no SVG was kept</p>".to_string(),
        |svg| svg.trim_start_matches('\u{feff}').to_string(),
    );
    let part = format!(
        "<section><h2>{}</h2><div class=\"pair\"><figure>{old}<figcaption>expected</figcaption></figure>\
         <figure>{new}<figcaption>actual</figcaption></figure></div><pre>{}</pre></section>\n",
        escape(name),
        escape(message)
    );
    std::fs::write(dir.join(format!("{name}.part.html")), part)?;
    let mut parts: Vec<std::path::PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.to_string_lossy().ends_with(".part.html"))
        .collect();
    parts.sort();
    let mut html = String::from(
        "<!doctype html><meta charset=\"utf-8\"><title>intuiTUIve snapshots</title>\
         <style>body{font-family:sans-serif}.pair{display:flex;gap:1em;flex-wrap:wrap}\
         figure{margin:0}svg{max-width:48vw;height:auto}</style><h1>Snapshots that differ</h1>\n",
    );
    for part in parts {
        html.push_str(&std::fs::read_to_string(part)?);
    }
    let report = dir.join("report.html");
    std::fs::write(&report, html)?;
    Ok(report)
}

/// Assert that a screen matches its snapshot: `tests/snapshots/<name>.txt`
/// under the crate being tested, the screen's [plain](crate::testing::plain)
/// rows. The screen is a [`Driver`](crate::Driver), a
/// [`Screen`](crate::screen::Screen) or the rows
/// [`App::render_with`](crate::App::render_with) returns. Set
/// `UPDATE_SNAPSHOTS=1` to write the file instead. See
/// [`testing`](crate::testing).
#[macro_export]
macro_rules! assert_screen {
    ($screen:expr, $name:expr $(,)?) => {
        $crate::testing::assert_snapshot(
            ::std::path::Path::new(::core::env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("snapshots")
                .join(::std::format!("{}.txt", $name)),
            &$crate::testing::plain(&$screen),
        )
    };
}

/// [`assert_screen!`], and the screen's [styled](crate::testing::styled) rows
/// against `tests/snapshots/<name>.styled.txt`: each row as console markup,
/// so a change of colour or text style fails too. The screen is a
/// [`Driver`](crate::Driver) or a [`Screen`](crate::screen::Screen).
#[macro_export]
macro_rules! assert_screen_styled {
    ($screen:expr, $name:expr $(,)?) => {{
        let dir = ::std::path::Path::new(::core::env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots");
        $crate::testing::assert_snapshot(
            dir.join(::std::format!("{}.txt", $name)),
            &$crate::testing::plain(&$screen),
        );
        $crate::testing::assert_snapshot(
            dir.join(::std::format!("{}.styled.txt", $name)),
            &$crate::testing::styled(&$screen),
        );
    }};
}

/// Assert that a screen's [styled](crate::testing::styled) rows match
/// `tests/snapshots/<name>.styled.txt`, keeping its SVG picture as
/// `<name>.svg` beside it; on a difference, an HTML report shows both
/// pictures (see [`check_svg_snapshot`](crate::testing::check_svg_snapshot)).
/// The screen is a [`Driver`](crate::Driver) or a
/// [`Screen`](crate::screen::Screen). Set `UPDATE_SNAPSHOTS=1` to write both
/// files.
#[macro_export]
macro_rules! assert_screen_svg {
    ($screen:expr, $name:expr $(,)?) => {{
        let dir = ::std::path::Path::new(::core::env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("snapshots");
        $crate::testing::assert_svg_snapshot(&dir, &::std::format!("{}", $name), &$screen);
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_difference_is_a_diff_and_line_endings_are_not() {
        let dir = std::env::temp_dir().join(format!("intuituive-snap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("screen.txt");
        std::fs::write(&path, "one  \r\ntwo\r\n").unwrap();
        assert_eq!(check(&path, "one\ntwo\n", false), Ok(()));
        let error = check(&path, "one\nthree\n", false).unwrap_err();
        assert!(error.contains("-two\n+three\n"), "{error}");
        assert!(error.contains("UPDATE_SNAPSHOTS=1"), "{error}");
        let missing = check(&dir.join("nope.txt"), "x\n", false).unwrap_err();
        assert!(missing.starts_with("missing snapshot"), "{missing}");
        // Updating writes the file, and then it matches.
        assert_eq!(check(&path, "one\nthree\n", true), Ok(()));
        assert_eq!(check(&path, "one\nthree\n", false), Ok(()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plain_rows_keep_the_height_without_trailing_spaces() {
        let rows = vec!["ab  ".to_string(), "    ".to_string(), " c".to_string()];
        assert_eq!(plain(&rows), "ab\n\n c\n");
    }
}

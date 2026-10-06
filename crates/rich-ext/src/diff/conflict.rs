//! Three-way merge conflicts: a parser for the markers `git merge` leaves in
//! a file, and [`ConflictView`], which shows ours, base and theirs.
//!
//! A conflict is the block git writes when it cannot merge two changes:
//!
//! ```text
//! <<<<<<< HEAD
//! ours
//! ||||||| base          (only with merge.conflictStyle diff3 or zdiff3)
//! the common ancestor
//! =======
//! theirs
//! >>>>>>> feature
//! ```
//!
//! A marker is seven marker characters at the start of a line, followed by
//! the end of the line or whitespace and an optional label, as git's own
//! `rerere` reads them. Inside a conflict every marker has the opening
//! marker's length, so a longer one is text, which is how git nests a
//! conflict inside another (it lengthens the inner markers). A path's
//! `conflict-marker-size` attribute makes git write longer markers
//! throughout; a longer opening marker opens a conflict when a separator and
//! a closing marker of the same length follow it, and is text otherwise.
//! Outside a conflict only `<<<<<<<` means anything, so a Markdown heading
//! underlined with `=======` is just text. Inside one, markers out of order,
//! a second `<<<<<<<`, and a conflict the input never closes are errors that
//! name the line; parsing never panics.

use std::fmt;
use std::ops::Range;

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use super::render::{banner, join, pad_to, trim_end, wrap_text};
use super::source::{highlight_lines, language_for_path};
use super::style;

/// The largest input [`ConflictFile::parse`] accepts, in bytes (16 MiB).
pub const MAX_CONFLICT_SOURCE: usize = 16 * 1024 * 1024;

/// The most conflicts one file may hold.
pub const MAX_CONFLICTS: usize = 10_000;

/// The marker length git writes by default (`conflict-marker-size`).
const MARKER_SIZE: usize = 7;

/// Columns narrower than this many cells of text stack instead of sitting
/// side by side, in [`ConflictLayout::Auto`].
const SIDE_BY_SIDE_MIN: usize = 20;

/// Files larger than this (1 MiB) are shown without syntax highlighting:
/// each version is highlighted whole, and that is not worth it for a file
/// whose conflicts are a few screens of it.
const HIGHLIGHT_LIMIT: usize = 1024 * 1024;

/// Why a file's conflict markers did not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictError {
    /// The 1-based line the problem is on, or `None` for the input as a
    /// whole (it is too large).
    pub line: Option<usize>,
    pub message: String,
}

impl ConflictError {
    fn at(line: usize, message: impl Into<String>) -> Self {
        ConflictError {
            line: Some(line),
            message: message.into(),
        }
    }
}

impl fmt::Display for ConflictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ConflictError {}

/// One side of a conflict.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictSide {
    /// The text after the marker (`HEAD`, a branch, a commit), if any.
    pub label: Option<String>,
    /// The 1-based line of the marker that opens this side.
    pub marker_line: usize,
    /// The side's lines, as 0-based indices into [`ConflictFile::lines`].
    pub lines: Range<usize>,
}

/// One conflict block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    /// 1-based, in file order.
    pub number: usize,
    /// Ours: between `<<<<<<<` and `|||||||` or `=======`.
    pub ours: ConflictSide,
    /// The common ancestor, in diff3 style: between `|||||||` and `=======`.
    pub base: Option<ConflictSide>,
    /// Theirs: between `=======` and `>>>>>>>`. Its `label` is the one on
    /// the closing `>>>>>>>` marker.
    pub theirs: ConflictSide,
    /// The 1-based line of the closing `>>>>>>>` marker.
    pub end_line: usize,
}

impl Conflict {
    /// The 1-based line of the opening `<<<<<<<` marker.
    pub fn start_line(&self) -> usize {
        self.ours.marker_line
    }

    /// The block's lines, markers included, as 0-based indices.
    pub fn span(&self) -> Range<usize> {
        self.start_line() - 1..self.end_line
    }

    /// The sides in display order: ours, base (when present), theirs.
    pub fn sides(&self) -> Vec<(Pick, &ConflictSide)> {
        let mut sides = vec![(Pick::Ours, &self.ours)];
        if let Some(base) = &self.base {
            sides.push((Pick::Base, base));
        }
        sides.push((Pick::Theirs, &self.theirs));
        sides
    }
}

/// Which side of a conflict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pick {
    Ours,
    Base,
    Theirs,
}

impl Pick {
    /// `ours`, `base` or `theirs`.
    pub fn name(self) -> &'static str {
        match self {
            Pick::Ours => "ours",
            Pick::Base => "base",
            Pick::Theirs => "theirs",
        }
    }

    fn marker(self) -> char {
        match self {
            Pick::Ours => '<',
            Pick::Base => '|',
            Pick::Theirs => '>',
        }
    }

    fn style_key(self) -> &'static str {
        match self {
            Pick::Ours => "diff.conflict.ours",
            Pick::Base => "diff.conflict.base",
            Pick::Theirs => "diff.conflict.theirs",
        }
    }
}

/// A file with conflict markers, parsed.
///
/// ```
/// use rich_ext::diff::ConflictFile;
///
/// let file = ConflictFile::parse("a\n<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\nd\n").unwrap();
/// let conflict = &file.conflicts()[0];
/// assert_eq!(conflict.ours.label.as_deref(), Some("HEAD"));
/// assert_eq!(file.text(&conflict.ours), "b");
/// assert_eq!(file.text(&conflict.theirs), "c");
/// assert_eq!((conflict.start_line(), conflict.end_line), (2, 6));
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictFile {
    lines: Vec<String>,
    conflicts: Vec<Conflict>,
    crlf: bool,
}

/// A marker on `line`: its character, its length and its label, when the
/// line is one. git writes [`MARKER_SIZE`] characters unless a path's
/// `conflict-marker-size` attribute asks for more, so any run of at least
/// that many counts; [`ConflictFile::parse`] holds a conflict's markers to
/// the length its opening marker has.
fn marker(line: &str) -> Option<(char, usize, Option<&str>)> {
    let first = line.chars().next()?;
    if !matches!(first, '<' | '|' | '=' | '>') {
        return None;
    }
    let length = line.bytes().take_while(|&b| b == first as u8).count();
    if length < MARKER_SIZE {
        return None;
    }
    let rest = &line[length..];
    match rest.chars().next() {
        None => Some((first, length, None)),
        Some(c) if c.is_whitespace() => {
            let label = rest.trim();
            match (first, label.is_empty()) {
                (_, true) => Some((first, length, None)),
                // git writes the separator bare; text after one is content.
                ('=', false) => None,
                (_, false) => Some((first, length, Some(label))),
            }
        }
        Some(_) => None,
    }
}

/// The markers of each length other than git's own: per length, the `<`,
/// `=` and `>` markers in file order, as `(line index, kind)`.
type LongerMarkers = std::collections::HashMap<usize, Vec<(usize, char)>>;

/// Whether the opening marker of `length` at `open` is followed by a
/// separator and then a closing marker of the same length, before another
/// opening marker of that length. Only the markers of that length are
/// walked, and the walk stops at the next opening one, so the walks from
/// all the opening markers of a length cover its markers once between them.
fn closes(longer: &LongerMarkers, open: usize, length: usize) -> bool {
    let Some(markers) = longer.get(&length) else {
        return false;
    };
    let after = markers.partition_point(|&(line, _)| line <= open);
    let mut separated = false;
    for &(_, kind) in &markers[after..] {
        match kind {
            '<' => return false,
            '=' => separated = true,
            '>' if separated => return true,
            _ => {}
        }
    }
    false
}

enum State {
    Outside,
    Ours {
        start: usize,
        label: Option<String>,
    },
    Base {
        ours: ConflictSide,
        start: usize,
        label: Option<String>,
    },
    Theirs {
        ours: ConflictSide,
        base: Option<ConflictSide>,
        start: usize,
    },
}

impl ConflictFile {
    /// Parse `text`. Lines may end in `\n` or `\r\n`; a leading byte-order
    /// mark is dropped. Fails on input over [`MAX_CONFLICT_SOURCE`] bytes,
    /// more than [`MAX_CONFLICTS`] conflicts, markers out of order, and a
    /// conflict left open at the end, naming the line.
    pub fn parse(text: &str) -> Result<ConflictFile, ConflictError> {
        if text.len() > MAX_CONFLICT_SOURCE {
            return Err(ConflictError {
                line: None,
                message: format!(
                    "the input is {} bytes; at most {MAX_CONFLICT_SOURCE} are read",
                    text.len()
                ),
            });
        }
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let mut crlf = false;
        let mut lines: Vec<String> = text
            .split('\n')
            .map(|line| match line.strip_suffix('\r') {
                Some(line) => {
                    crlf = true;
                    line.to_string()
                }
                None => line.to_string(),
            })
            .collect();
        if text.is_empty() || text.ends_with('\n') {
            lines.pop();
        }

        // Every line is measured once, and the markers of other lengths
        // are indexed by length for `closes`.
        let markers: Vec<Option<(char, usize, Option<&str>)>> =
            lines.iter().map(|line| marker(line)).collect();
        let mut longer = LongerMarkers::new();
        for (index, found) in markers.iter().enumerate() {
            if let Some((kind @ ('<' | '=' | '>'), length, _)) = *found {
                if length != MARKER_SIZE {
                    longer.entry(length).or_default().push((index, kind));
                }
            }
        }

        let mut conflicts = Vec::new();
        let mut state = State::Outside;
        // The length of the open conflict's markers: a marker of any other
        // length inside it is content.
        let mut size = MARKER_SIZE;
        for (index, found) in markers.iter().enumerate() {
            let number = index + 1;
            let Some((kind, length, label)) = *found else {
                continue;
            };
            match state {
                // git's own size opens a conflict as before. A longer one
                // (a `conflict-marker-size` attribute) is a conflict only
                // when a separator and a closing marker of the same length
                // follow; otherwise the line is text.
                State::Outside if kind == '<' && length != MARKER_SIZE => {
                    if !closes(&longer, index, length) {
                        continue;
                    }
                    size = length;
                }
                State::Outside if kind == '<' => size = length,
                State::Outside => {}
                _ if length != size => continue,
                _ => {}
            }
            let label = label.map(str::to_string);
            state = match (state, kind) {
                (State::Outside, '<') => {
                    if conflicts.len() == MAX_CONFLICTS {
                        return Err(ConflictError::at(
                            number,
                            format!("more than {MAX_CONFLICTS} conflicts"),
                        ));
                    }
                    State::Ours {
                        start: number,
                        label,
                    }
                }
                // Outside a conflict, only an opening marker is one.
                (State::Outside, _) => State::Outside,
                (
                    State::Ours { start, .. }
                    | State::Base {
                        ours:
                            ConflictSide {
                                marker_line: start, ..
                            },
                        ..
                    }
                    | State::Theirs {
                        ours:
                            ConflictSide {
                                marker_line: start, ..
                            },
                        ..
                    },
                    '<',
                ) => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "`<<<<<<<` inside the conflict opened at line {start}; \
                             it has no `>>>>>>>`"
                        ),
                    ))
                }
                (State::Ours { start, label: ours }, '|') => State::Base {
                    ours: ConflictSide {
                        label: ours,
                        marker_line: start,
                        lines: start..index,
                    },
                    start: number,
                    label,
                },
                (State::Ours { start, label: ours }, '=') => State::Theirs {
                    ours: ConflictSide {
                        label: ours,
                        marker_line: start,
                        lines: start..index,
                    },
                    base: None,
                    start: number,
                },
                (
                    State::Base {
                        ours,
                        start,
                        label: base,
                    },
                    '=',
                ) => State::Theirs {
                    ours,
                    base: Some(ConflictSide {
                        label: base,
                        marker_line: start,
                        lines: start..index,
                    }),
                    start: number,
                },
                (State::Base { ours, .. }, '|') => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "a second `|||||||` in the conflict opened at line {}",
                            ours.marker_line
                        ),
                    ))
                }
                (State::Ours { start, .. }, _) => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "`>>>>>>>` before `=======` in the conflict opened at line {start}"
                        ),
                    ))
                }
                (State::Base { ours, .. }, _) => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "`>>>>>>>` before `=======` in the conflict opened at line {}",
                            ours.marker_line
                        ),
                    ))
                }
                (State::Theirs { ours, base, start }, '>') => {
                    conflicts.push(Conflict {
                        number: conflicts.len() + 1,
                        ours,
                        base,
                        theirs: ConflictSide {
                            label,
                            marker_line: start,
                            lines: start..index,
                        },
                        end_line: number,
                    });
                    State::Outside
                }
                (State::Theirs { ours, .. }, '=') => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "a second `=======` in the conflict opened at line {}",
                            ours.marker_line
                        ),
                    ))
                }
                (State::Theirs { ours, .. }, _) => {
                    return Err(ConflictError::at(
                        number,
                        format!(
                            "`|||||||` after `=======` in the conflict opened at line {}",
                            ours.marker_line
                        ),
                    ))
                }
            };
        }
        match state {
            State::Outside => Ok(ConflictFile {
                lines,
                conflicts,
                crlf,
            }),
            State::Ours { start, .. } => Err(unterminated(start)),
            State::Base { ours, .. } | State::Theirs { ours, .. } => {
                Err(unterminated(ours.marker_line))
            }
        }
    }

    /// Every line of the file, without terminators.
    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    /// The conflicts, in file order.
    pub fn conflicts(&self) -> &[Conflict] {
        &self.conflicts
    }

    /// Whether the file has any conflict.
    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// Whether any line ended in `\r\n`.
    pub fn is_crlf(&self) -> bool {
        self.crlf
    }

    /// A side's lines joined with `\n` (no trailing newline).
    pub fn text(&self, side: &ConflictSide) -> String {
        self.lines[side.lines.clone()].join("\n")
    }
}

/// `row` cut to `width` cells when it is wider.
fn crop(row: Vec<Segment>, width: usize) -> Vec<Segment> {
    if row.iter().map(Segment::cell_length).sum::<usize>() > width {
        Segment::adjust_line_length(&row, width, None)
    } else {
        row
    }
}

fn unterminated(start: usize) -> ConflictError {
    ConflictError::at(
        start,
        "this conflict is never closed: no `>>>>>>>` before the end of the input",
    )
}

/// How a [`ConflictView`] lays out the sides.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConflictLayout {
    /// Side by side when every column gets at least 20 cells of text,
    /// stacked otherwise (the default).
    #[default]
    Auto,
    /// Ours, base and theirs in columns.
    SideBySide,
    /// One side under another.
    Stacked,
}

/// A file's merge conflicts as a renderable: each conflict numbered, with
/// ours, base (when the markers have one) and theirs side by side or
/// stacked, syntax highlighted, between a few lines of surrounding context.
///
/// Line numbers are the file's, so they lead back to the markers. With
/// colour off every line keeps a marker: `<` ours, `|` base, `>` theirs.
/// Styles come from the theme's `diff.conflict.ours`, `diff.conflict.base`
/// and `diff.conflict.theirs` (see [`STYLES`](super::STYLES)).
///
/// ```
/// use rich::Console;
/// use rich_ext::diff::{ConflictLayout, ConflictView};
///
/// let view = ConflictView::parse("<<<<<<< HEAD\nb\n=======\nc\n>>>>>>> topic\n")
///     .unwrap()
///     .layout(ConflictLayout::Stacked);
/// let console = Console::builder().width(40).no_color(true).build();
/// assert_eq!(
///     console.render_to_string(&view),
///     "conflict 1 of 1, lines 1-5\n  ours: HEAD\n2 < b\n  theirs: topic\n4 > c"
/// );
/// ```
#[derive(Clone, Debug)]
pub struct ConflictView {
    file: ConflictFile,
    language: Option<String>,
    path: Option<String>,
    layout: ConflictLayout,
    context: usize,
    line_numbers: bool,
    wrap: bool,
    base: bool,
}

/// One displayed line.
struct Line {
    pick: Option<Pick>,
    number: Option<usize>,
    text: Text,
}

/// What to draw for one conflict.
struct Shown {
    before: Range<usize>,
    after: Range<usize>,
}

impl ConflictView {
    /// View a parsed file.
    pub fn new(file: ConflictFile) -> Self {
        ConflictView {
            file,
            language: None,
            path: None,
            layout: ConflictLayout::Auto,
            context: 3,
            line_numbers: true,
            wrap: true,
            base: true,
        }
    }

    /// Parse `text` and view it.
    pub fn parse(text: &str) -> Result<Self, ConflictError> {
        ConflictFile::parse(text).map(Self::new)
    }

    /// The parsed file.
    pub fn file(&self) -> &ConflictFile {
        &self.file
    }

    /// The language (a name or extension core `Syntax` knows).
    pub fn language(mut self, language: impl Into<String>) -> Self {
        self.language = Some(language.into());
        self
    }

    /// The file's path, for the language when none is set.
    pub fn path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// Side by side, stacked, or chosen by width (default).
    pub fn layout(mut self, layout: ConflictLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Unchanged lines shown before and after each conflict (default 3).
    pub fn context(mut self, lines: usize) -> Self {
        self.context = lines;
        self
    }

    /// Show the file's line numbers (default on).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }

    /// Wrap long lines (default) or truncate them with an ellipsis.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Show the diff3 base when the markers have one (default on).
    pub fn base(mut self, show: bool) -> Self {
        self.base = show;
        self
    }

    fn resolved_language(&self) -> Option<String> {
        self.language
            .clone()
            .or_else(|| self.path.as_deref().map(language_for_path))
    }

    /// The context ranges around each conflict: up to `context` lines on
    /// each side, never overlapping a neighbour or what was already shown.
    fn shown(&self) -> Vec<Shown> {
        let conflicts = self.file.conflicts();
        let mut out = Vec::with_capacity(conflicts.len());
        let mut shown_until = 0;
        for (i, conflict) in conflicts.iter().enumerate() {
            let span = conflict.span();
            let before = span.start.saturating_sub(self.context).max(shown_until)..span.start;
            let limit = conflicts
                .get(i + 1)
                .map_or(self.file.lines.len(), |next| next.span().start);
            let after = span.end..span.end.saturating_add(self.context).min(limit);
            shown_until = after.end;
            out.push(Shown { before, after });
        }
        out
    }

    /// Every line the view shows, highlighted. The file is highlighted as
    /// three whole versions (context with ours, with base, with theirs) so
    /// multi-line constructs colour as they would in each. A file over
    /// `HIGHLIGHT_LIMIT` bytes is shown plain.
    fn texts(&self, console: Option<&Console>) -> Vec<Option<Text>> {
        let size: usize = self.file.lines.iter().map(|l| l.len() + 1).sum();
        let language = self.resolved_language().filter(|_| size <= HIGHLIGHT_LIMIT);
        let lines = &self.file.lines;
        let mut out: Vec<Option<Text>> = vec![None; lines.len()];
        let shown = self.shown();
        let mut wanted = vec![false; lines.len()];
        for (conflict, shown) in self.file.conflicts().iter().zip(&shown) {
            for i in shown.before.clone().chain(shown.after.clone()) {
                wanted[i] = true;
            }
            for (_, side) in conflict.sides() {
                for i in side.lines.clone() {
                    wanted[i] = true;
                }
            }
        }
        for pick in [Pick::Ours, Pick::Base, Pick::Theirs] {
            if pick == Pick::Base && !self.file.conflicts().iter().any(|c| c.base.is_some()) {
                continue;
            }
            // The indices of this version's lines, in order.
            let mut version: Vec<usize> = Vec::new();
            let mut next = 0;
            for conflict in self.file.conflicts() {
                let span = conflict.span();
                version.extend(next..span.start);
                let side = match pick {
                    Pick::Ours => Some(&conflict.ours),
                    Pick::Base => conflict.base.as_ref(),
                    Pick::Theirs => Some(&conflict.theirs),
                };
                if let Some(side) = side {
                    version.extend(side.lines.clone());
                }
                next = span.end;
            }
            version.extend(next..lines.len());
            if !version.iter().any(|&i| wanted[i] && out[i].is_none()) {
                continue;
            }
            let mut code = String::new();
            for &i in &version {
                code.push_str(&lines[i]);
                code.push('\n');
            }
            let highlighted = highlight_lines(&code, language.as_deref(), console);
            for (&i, text) in version.iter().zip(highlighted) {
                if wanted[i] && out[i].is_none() {
                    out[i] = Some(text);
                }
            }
        }
        out
    }

    fn digits(&self) -> usize {
        self.file.lines.len().max(1).to_string().len()
    }

    fn gutter_width(&self) -> usize {
        if self.line_numbers {
            self.digits() + 3
        } else {
            2
        }
    }

    fn sides_shown<'c>(&self, conflict: &'c Conflict) -> Vec<(Pick, &'c ConflictSide)> {
        conflict
            .sides()
            .into_iter()
            .filter(|(pick, _)| self.base || *pick != Pick::Base)
            .collect()
    }

    fn side_by_side(&self, width: usize) -> bool {
        if self.layout == ConflictLayout::Stacked {
            return false;
        }
        let columns = self
            .file
            .conflicts()
            .iter()
            .map(|c| self.sides_shown(c).len())
            .max()
            .unwrap_or(2);
        let per_column = width.saturating_sub(3 * (columns - 1)) / columns;
        match self.layout {
            // Forced columns still fall back to stacked when the dividers,
            // gutters and one cell of text do not fit in `width`.
            ConflictLayout::SideBySide => per_column > self.gutter_width(),
            _ => per_column >= self.gutter_width() + SIDE_BY_SIDE_MIN,
        }
    }

    fn line(&self, texts: &[Option<Text>], pick: Option<Pick>, index: usize) -> Line {
        Line {
            pick,
            number: Some(index + 1),
            text: texts[index]
                .clone()
                .unwrap_or_else(|| Text::new(self.file.lines[index].as_str())),
        }
    }

    /// A line's rows at `width`: gutter, marker, then the wrapped text.
    fn line_rows(&self, console: &Console, line: &Line, width: usize) -> Vec<Vec<Segment>> {
        let digits = self.digits();
        // A column too narrow for the numbers and some text drops them.
        let numbers = self.line_numbers && width > self.gutter_width();
        let gutter = if numbers { self.gutter_width() } else { 2 };
        let (base, marker) = match line.pick {
            Some(pick) => (style(console, pick.style_key()), pick.marker()),
            None => (style(console, "diff.context"), ' '),
        };
        let content = width.saturating_sub(gutter).max(1);
        wrap_text(console, &line.text, &base, content, self.wrap)
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let mut row = Vec::new();
                if numbers {
                    let number = match (i, line.number) {
                        (0, Some(n)) => format!("{n:>digits$} "),
                        _ => " ".repeat(digits + 1),
                    };
                    row.push(Segment::new(
                        number,
                        Some(style(console, "diff.line_number")),
                    ));
                }
                let marker = if i == 0 { marker } else { ' ' };
                row.push(Segment::new(format!("{marker} "), Some(base.clone())));
                row.extend(text);
                crop(row, width)
            })
            .collect()
    }

    /// A side's heading: `ours: HEAD`.
    fn heading(&self, console: &Console, pick: Pick, side: &ConflictSide) -> Text {
        let style =
            style(console, pick.style_key()).combine(&style(console, "diff.conflict.label"));
        let mut heading = pick.name().to_string();
        if let Some(label) = &side.label {
            heading.push_str(": ");
            heading.push_str(&crate::sanitize_terminal_controls(label));
        }
        Text::styled(heading, style)
    }

    fn rows(&self, console: &Console, width: usize) -> Vec<Vec<Segment>> {
        let conflicts = self.file.conflicts();
        if conflicts.is_empty() {
            return banner("no conflicts", style(console, "diff.line_number"), width);
        }
        let texts = self.texts(Some(console));
        let side_by_side = self.side_by_side(width);
        let indent = if self.line_numbers {
            self.digits() + 1
        } else {
            0
        };
        let mut out = Vec::new();
        for (conflict, shown) in conflicts.iter().zip(self.shown()) {
            out.extend(banner(
                &format!(
                    "conflict {} of {}, lines {}-{}",
                    conflict.number,
                    conflicts.len(),
                    conflict.start_line(),
                    conflict.end_line
                ),
                style(console, "diff.hunk"),
                width,
            ));
            for i in shown.before.clone() {
                out.extend(
                    self.line_rows(console, &self.line(&texts, None, i), width)
                        .into_iter()
                        .map(trim_end),
                );
            }
            let sides = self.sides_shown(conflict);
            let empty = |pick: Pick| Line {
                pick: Some(pick),
                number: None,
                text: Text::styled("(empty)", style(console, "diff.line_number")),
            };
            let side_lines = |pick: Pick, side: &ConflictSide| -> Vec<Line> {
                if side.lines.is_empty() {
                    vec![empty(pick)]
                } else {
                    side.lines
                        .clone()
                        .map(|i| self.line(&texts, Some(pick), i))
                        .collect()
                }
            };
            if side_by_side {
                let divider = if console.ascii_only() { " | " } else { " │ " };
                let div_style = style(console, "diff.line_number");
                let inner = width.saturating_sub(3 * (sides.len() - 1));
                let widths: Vec<usize> = (0..sides.len())
                    .map(|k| inner / sides.len() + usize::from(k < inner % sides.len()))
                    .collect();
                let columns: Vec<Vec<Vec<Segment>>> = sides
                    .iter()
                    .zip(&widths)
                    .map(|((pick, side), &w)| {
                        let mut rows: Vec<Vec<Segment>> = Vec::new();
                        let heading = self.heading(console, *pick, side);
                        let mut first = vec![Segment::new(" ".repeat(indent.min(w)), None)];
                        first.extend(
                            wrap_text(
                                console,
                                &heading,
                                &Style::new(),
                                w.saturating_sub(indent).max(1),
                                false,
                            )
                            .into_iter()
                            .next()
                            .unwrap_or_default(),
                        );
                        rows.push(crop(first, w));
                        for line in side_lines(*pick, side) {
                            rows.extend(self.line_rows(console, &line, w));
                        }
                        rows
                    })
                    .collect();
                let height = columns.iter().map(Vec::len).max().unwrap_or(0);
                for r in 0..height {
                    let mut row = Vec::new();
                    for (k, column) in columns.iter().enumerate() {
                        if k > 0 {
                            row.push(Segment::new(divider, Some(div_style.clone())));
                        }
                        let cells = column.get(r).cloned().unwrap_or_default();
                        if k + 1 < columns.len() {
                            row.extend(pad_to(cells, widths[k]));
                        } else {
                            row.extend(cells);
                        }
                    }
                    out.push(trim_end(row));
                }
            } else {
                for (pick, side) in &sides {
                    let heading = self.heading(console, *pick, side);
                    for line in wrap_text(
                        console,
                        &heading,
                        &Style::new(),
                        width.saturating_sub(indent).max(1),
                        false,
                    )
                    .into_iter()
                    .take(1)
                    {
                        let mut row = vec![Segment::new(" ".repeat(indent), None)];
                        row.extend(line);
                        out.push(trim_end(row));
                    }
                    for line in side_lines(*pick, side) {
                        out.extend(
                            self.line_rows(console, &line, width)
                                .into_iter()
                                .map(trim_end),
                        );
                    }
                }
            }
            for i in shown.after.clone() {
                out.extend(
                    self.line_rows(console, &self.line(&texts, None, i), width)
                        .into_iter()
                        .map(trim_end),
                );
            }
        }
        out
    }

    /// The widest line of each kind: `(context, per side shown)`.
    fn natural(&self) -> (usize, usize) {
        let lines = &self.file.lines;
        let width = |i: usize| cell_len(&lines[i]);
        let mut stacked = "no conflicts".len();
        let mut side_by_side = 0;
        for (conflict, shown) in self.file.conflicts().iter().zip(self.shown()) {
            stacked = stacked.max(
                format!(
                    "conflict {} of {}, lines {}-{}",
                    conflict.number,
                    self.file.conflicts().len(),
                    conflict.start_line(),
                    conflict.end_line
                )
                .len(),
            );
            for i in shown.before.chain(shown.after) {
                stacked = stacked.max(self.gutter_width() + width(i));
            }
            let sides = self.sides_shown(conflict);
            let mut total = 3 * (sides.len() - 1);
            for (pick, side) in &sides {
                let label = pick.name().len() + side.label.as_ref().map_or(0, |l| 2 + cell_len(l));
                let widest = side
                    .lines
                    .clone()
                    .map(width)
                    .max()
                    .unwrap_or("(empty)".len());
                let column = (self.gutter_width() + widest).max(self.gutter_width() + label);
                stacked = stacked.max(column);
                total += column;
            }
            side_by_side = side_by_side.max(total);
        }
        (stacked, side_by_side)
    }
}

impl Renderable for ConflictView {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        if options.max_width == 0 || options.height == Some(0) {
            return Vec::new();
        }
        join(self.rows(console, options.max_width), options.height)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let (stacked, side_by_side) = self.natural();
        let maximum = match self.layout {
            ConflictLayout::Stacked => stacked,
            ConflictLayout::SideBySide => side_by_side.max(stacked),
            // Wide enough for columns when they fit; stacked otherwise.
            ConflictLayout::Auto if self.side_by_side(side_by_side) => side_by_side.max(stacked),
            ConflictLayout::Auto => stacked,
        }
        .min(options.max_width);
        let minimum = (self.gutter_width() + 1).min(maximum);
        Measurement::new(minimum, maximum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_need_seven_or_more_characters() {
        assert_eq!(marker("<<<<<<< HEAD"), Some(('<', 7, Some("HEAD"))));
        assert_eq!(marker("<<<<<<<"), Some(('<', 7, None)));
        // Longer runs are markers of that length (`conflict-marker-size`);
        // parsing decides whether one opens a conflict.
        assert_eq!(marker("<<<<<<<< HEAD"), Some(('<', 8, Some("HEAD"))));
        assert_eq!(marker("<<<<<<"), None);
        assert_eq!(marker("<<<<<<<x"), None);
        assert_eq!(marker("======="), Some(('=', 7, None)));
        assert_eq!(marker("=======  "), Some(('=', 7, None)));
        assert_eq!(marker("======= x"), None);
        assert_eq!(
            marker("||||||| merged common ancestors"),
            Some(('|', 7, Some("merged common ancestors")))
        );
        assert_eq!(marker("x<<<<<<<"), None);
        assert_eq!(marker(""), None);
    }
}

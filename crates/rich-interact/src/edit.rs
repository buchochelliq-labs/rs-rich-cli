//! A text buffer for editors: lines of text, a caret on a grapheme
//! boundary, a selection, a character limit, and undo and redo.
//!
//! [`TextArea`](crate::TextArea) edits through a [`Buffer`], and so does
//! `rs-rich-intuituive`'s `text_area`, so a fix to how the caret moves over
//! a grapheme cluster or how pasted text is cleaned lands in both.
//!
//! Every change is one [`replace`](Buffer::replace) of a range with text, so
//! every change can be undone. Typing in a run (characters at the caret,
//! one after another) undoes as one step, and so does deleting in a run;
//! moving the caret ends the run. The history keeps the last
//! [`HISTORY`] steps unless [`set_history_limit`](Buffer::set_history_limit)
//! says otherwise.
//!
//! Text killed ([`kill_to_end`](Buffer::kill_to_end) and the other `kill_`
//! methods) goes on a kill ring, as Emacs and readline keep one:
//! [`yank`](Buffer::yank) puts the last kill back, and
//! [`yank_pop`](Buffer::yank_pop) right after it swaps in the one before.
//! A [`Search`] finds a plain string or a regular expression, line by line.
//!
//! ```
//! use rich_interact::edit::Buffer;
//!
//! let mut buffer = Buffer::new("fn main() {}");
//! buffer.end(false);
//! buffer.left(false);
//! buffer.insert("\n    println!();\n");
//! assert_eq!(buffer.text(), "fn main() {\n    println!();\n}");
//! buffer.undo();
//! assert_eq!(buffer.text(), "fn main() {}");
//! ```

use std::cell::RefCell;
use std::collections::VecDeque;
use std::ops::Range;

use rich::cells::split_graphemes;

/// The cells a tab takes on screen, unless a buffer's
/// [`set_tab_width`](Buffer::set_tab_width) says otherwise.
pub const TAB: usize = 4;

/// The undo steps a buffer keeps unless
/// [`set_history_limit`](Buffer::set_history_limit) says otherwise.
pub const HISTORY: usize = 1000;

/// The kills the kill ring keeps.
const KILLS: usize = 16;

/// The grapheme clusters of `line` as (start, end, cells): rich's, with a
/// tab one of its own, [`TAB`] cells wide, rather than joined to the one
/// before it as a zero-width character is.
pub fn graphemes(line: &str) -> Vec<(usize, usize, usize)> {
    graphemes_with(line, TAB)
}

/// [`graphemes`], a tab `tab` cells wide.
pub fn graphemes_with(line: &str, tab: usize) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for piece in line.split('\t') {
        let (spans, _) = split_graphemes(piece);
        out.extend(spans.into_iter().map(|(s, e, w)| (start + s, start + e, w)));
        start += piece.len();
        if start < line.len() {
            out.push((start, start + 1, tab));
            start += 1;
        }
    }
    out
}

/// The cells `text` (one line) takes on screen.
pub fn cells(text: &str) -> usize {
    cells_with(text, TAB)
}

/// [`cells`], a tab `tab` cells wide.
pub fn cells_with(text: &str, tab: usize) -> usize {
    graphemes_with(text, tab)
        .iter()
        .map(|(_, _, width)| width)
        .sum()
}

/// How a line longer than the room is shown: [`wrap_line`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WrapMode {
    /// One row a line, scrolled sideways.
    None,
    /// Broken at the last grapheme cluster that fits (a character wrap).
    #[default]
    Grapheme,
    /// Broken after the last space that fits, so words stay whole; a word
    /// longer than the room is broken as [`Grapheme`](Self::Grapheme) does.
    Word,
}

/// The rows `line` takes `room` cells wide (a tab `tab` cells), as byte
/// ranges covering it end to end: one for [`WrapMode::None`] or a line that
/// fits. With [`WrapMode::Word`] a row ends after a space, and the one space
/// that does not fit hangs past the room rather than starting the next row.
///
/// ```
/// use rich_interact::edit::{wrap_line, WrapMode};
///
/// let line = "the quick brown fox";
/// let rows = |mode| -> Vec<&str> {
///     wrap_line(line, 8, 4, mode).into_iter().map(|r| &line[r]).collect()
/// };
/// assert_eq!(rows(WrapMode::Word), ["the ", "quick ", "brown ", "fox"]);
/// assert_eq!(rows(WrapMode::Grapheme), ["the quic", "k brown ", "fox"]);
/// assert_eq!(wrap_line("abcdefghijkl", 5, 4, WrapMode::Word), [0..5, 5..10, 10..12]);
/// assert_eq!(wrap_line(line, 8, 4, WrapMode::None).len(), 1);
/// ```
pub fn wrap_line(line: &str, room: usize, tab: usize, mode: WrapMode) -> Vec<Range<usize>> {
    if mode == WrapMode::None {
        return std::iter::once(0..line.len()).collect();
    }
    let room = room.max(1);
    let mut rows = Vec::new();
    let (mut start, mut used) = (0, 0);
    // Where a row may end after a space, and the cells up to it.
    let mut space: Option<(usize, usize)> = None;
    for (at, end, width) in graphemes_with(line, tab) {
        let blank = line[at..end].starts_with(char::is_whitespace);
        if used + width > room && at > start {
            if mode == WrapMode::Word && blank {
                // The space hangs: the row ends after it.
                rows.push(start..end);
                (start, used, space) = (end, 0, None);
                continue;
            }
            match space.filter(|_| mode == WrapMode::Word) {
                Some((after, cells)) if after > start => {
                    rows.push(start..after);
                    (start, used) = (after, used - cells);
                }
                _ => {
                    rows.push(start..at);
                    (start, used) = (at, 0);
                }
            }
            space = None;
        }
        used += width;
        if blank {
            space = Some((end, used));
        }
    }
    rows.push(start..line.len());
    rows
}

/// The characters in `text` as a limit counts them: grapheme clusters,
/// and a line break one.
pub fn length(text: &str) -> usize {
    text.split('\n')
        .map(|line| graphemes(line).len())
        .sum::<usize>()
        + text.matches('\n').count()
}

/// The first `chars` characters of `text` as a limit counts them: grapheme
/// clusters, a line break one. Never cuts a cluster.
pub fn truncate(text: &str, chars: usize) -> &str {
    let mut left = chars;
    let mut start = 0;
    for line in text.split('\n') {
        let clusters = graphemes(line);
        if clusters.len() > left {
            return &text[..start + clusters[left].0];
        }
        left -= clusters.len();
        start += line.len();
        if start == text.len() || left == 0 {
            return &text[..start];
        }
        // The line break.
        left -= 1;
        start += 1;
    }
    text
}

/// Text for an editor: `\r\n` and `\r` as `\n`, tabs kept, and other
/// terminal controls (C0, DEL, C1) dropped.
pub fn clean(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\t' => out.push(c),
            '\0'..='\u{1f}' | '\u{7f}'..='\u{9f}' => {}
            _ => out.push(c),
        }
    }
    out
}

/// What to find in a buffer ([`Buffer::set_search`]): a plain string or a
/// regular expression (fancy-regex's syntax, look-around included), matched
/// within each line, so a match never spans a line break. Empty matches
/// are skipped.
///
/// ```
/// use rich_interact::edit::Search;
///
/// let plain = Search::plain("a.b");
/// assert_eq!(plain.find_in("a.b axb"), [0..3]);
/// let regex = Search::regex(r"a.b").unwrap();
/// assert_eq!(regex.find_in("a.b axb"), [0..3, 4..7]);
/// let any_case = Search::plain("Fox").ignore_case(true);
/// assert_eq!(any_case.find_in("fox FOX"), [0..3, 4..7]);
/// assert!(Search::regex("(").is_err());
/// ```
#[derive(Clone, Debug)]
pub struct Search {
    source: String,
    regex: bool,
    ignore_case: bool,
    compiled: fancy_regex::Regex,
}

impl Search {
    /// Find `text` as it is.
    pub fn plain(text: &str) -> Search {
        Search::build(text, false, false).expect("an escaped pattern compiles")
    }

    /// Find matches of the regular expression `pattern`; the error says
    /// why it does not compile.
    pub fn regex(pattern: &str) -> Result<Search, String> {
        Search::build(pattern, true, false)
    }

    /// Plain when `regex` is false, else a regular expression.
    pub fn new(pattern: &str, regex: bool) -> Result<Search, String> {
        Search::build(pattern, regex, false)
    }

    /// Match letters of either case.
    pub fn ignore_case(self, on: bool) -> Search {
        Search::build(&self.source, self.regex, on).unwrap_or(self)
    }

    fn build(source: &str, regex: bool, ignore_case: bool) -> Result<Search, String> {
        let body = if regex {
            std::borrow::Cow::Borrowed(source)
        } else {
            fancy_regex::escape(source)
        };
        let pattern = if ignore_case {
            format!("(?i:{body})")
        } else {
            body.into_owned()
        };
        let compiled = fancy_regex::Regex::new(&pattern).map_err(|e| e.to_string())?;
        Ok(Search {
            source: source.to_string(),
            regex,
            ignore_case,
            compiled,
        })
    }

    /// The pattern as given.
    pub fn pattern(&self) -> &str {
        &self.source
    }

    pub fn is_regex(&self) -> bool {
        self.regex
    }

    pub fn is_ignore_case(&self) -> bool {
        self.ignore_case
    }

    /// The byte ranges it matches in `line`, in order, none empty.
    pub fn find_in(&self, line: &str) -> Vec<Range<usize>> {
        if self.source.is_empty() {
            return Vec::new();
        }
        self.compiled
            .find_iter(line)
            .map_while(Result::ok)
            .filter(|m| m.start() < m.end())
            .map(|m| m.range())
            .collect()
    }
}

/// A place in the text: a line, and a byte offset into it on a grapheme
/// boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: usize,
    pub at: usize,
}

impl Pos {
    pub const fn new(line: usize, at: usize) -> Pos {
        Pos { line, at }
    }
}

/// One change: `removed` taken from `start`, `inserted` put there.
#[derive(Clone, Debug)]
struct Change {
    start: Pos,
    removed: String,
    inserted: String,
}

/// Changes undone and redone together, and where the caret and the
/// selection's anchor were before and after.
#[derive(Clone, Debug)]
struct Step {
    changes: Vec<Change>,
    before: (Pos, Option<Pos>),
    after: (Pos, Option<Pos>),
    /// What the run is, while it may still grow.
    run: Option<Run>,
}

/// A run of edits that undo as one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Run {
    Typing,
    Deleting,
}

/// Lines of text with a caret, a selection, a limit, and undo and redo.
#[derive(Clone, Debug)]
pub struct Buffer {
    lines: Vec<String>,
    caret: Pos,
    /// Where a selection started; the caret is its other end.
    anchor: Option<Pos>,
    limit: Option<usize>,
    undo: VecDeque<Step>,
    redo: Vec<Step>,
    /// Bumped by every change, so a view knows when to draw again.
    revision: u64,
    /// [`count`](Self::count), kept up to date by each change, so a limit
    /// costs a keystroke only the lines it touches.
    chars: usize,
    /// The undo steps kept; `None` keeps them all.
    history: Option<usize>,
    /// The cells a tab takes, and what an indent is.
    tab: usize,
    hard_tabs: bool,
    /// The kill ring, the newest last.
    kills: Vec<String>,
    /// The revision and caret right after the last kill: a kill straight
    /// after it joins it.
    killed: Option<(u64, Pos)>,
    /// What the last yank put in, while it may be swapped
    /// ([`yank_pop`](Self::yank_pop)).
    yanked: Option<Yanked>,
    search: Option<Search>,
    /// Bumped when the search changes.
    searches: u64,
    /// Every match, for the revision and search they were found for.
    found: RefCell<Option<Found>>,
}

/// Every match in a buffer, for the revision and the search they were
/// found for.
type Found = (u64, u64, Vec<(Pos, Pos)>);

/// A yank that [`Buffer::yank_pop`] may swap for an older kill.
#[derive(Clone, Copy, Debug)]
struct Yanked {
    start: Pos,
    end: Pos,
    /// The kill it put in.
    index: usize,
    revision: u64,
}

impl Default for Buffer {
    fn default() -> Buffer {
        Buffer::new("")
    }
}

impl Buffer {
    /// A buffer holding `text` (cleaned: see [`clean`]), the caret at its
    /// start.
    pub fn new(text: &str) -> Buffer {
        let text = clean(text);
        let lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        Buffer {
            chars: chars_of(&lines),
            lines,
            caret: Pos::default(),
            anchor: None,
            limit: None,
            undo: VecDeque::new(),
            redo: Vec::new(),
            revision: 0,
            history: Some(HISTORY),
            tab: TAB,
            hard_tabs: false,
            kills: Vec::new(),
            killed: None,
            yanked: None,
            search: None,
            searches: 0,
            found: RefCell::new(None),
        }
    }

    /// Replace everything with `text`, the caret at its start; the undo
    /// history is forgotten. The settings (the limit, the tab width, the
    /// history limit), the kill ring and the search stay.
    pub fn set_text(&mut self, text: &str) {
        let fresh = Buffer::new(text);
        self.lines = fresh.lines;
        self.chars = fresh.chars;
        self.caret = Pos::default();
        self.anchor = None;
        self.undo.clear();
        self.redo.clear();
        self.killed = None;
        self.yanked = None;
        self.revision += 1;
        if let Some(limit) = self.limit {
            self.cut_to(limit);
        }
    }

    /// Keep at most `steps` undo steps (the oldest go first); `None` keeps
    /// them all. [`HISTORY`] by default.
    ///
    /// ```
    /// use rich_interact::edit::Buffer;
    ///
    /// let mut buffer = Buffer::new("");
    /// buffer.set_history_limit(Some(2));
    /// for word in ["a", "\n", "b", "\n"] {
    ///     buffer.insert(word);
    /// }
    /// assert!(buffer.undo() && buffer.undo());
    /// assert!(!buffer.undo(), "only two steps were kept");
    /// assert_eq!(buffer.text(), "a\n");
    /// ```
    pub fn set_history_limit(&mut self, steps: Option<usize>) {
        self.history = steps;
        self.trim_history();
    }

    pub fn history_limit(&self) -> Option<usize> {
        self.history
    }

    fn trim_history(&mut self) {
        if let Some(steps) = self.history {
            while self.undo.len() > steps {
                self.undo.pop_front();
            }
            self.redo.truncate(steps);
        }
    }

    /// The cells a tab takes ([`TAB`] by default, at least 1), and the
    /// spaces [`indent`](Self::indent) inserts and
    /// [`outdent`](Self::outdent) takes away.
    pub fn set_tab_width(&mut self, cells: usize) {
        self.tab = cells.max(1);
        self.revision += 1;
    }

    pub fn tab_width(&self) -> usize {
        self.tab
    }

    /// Indent with a tab character rather than spaces.
    pub fn set_hard_tabs(&mut self, on: bool) {
        self.hard_tabs = on;
    }

    pub fn hard_tabs(&self) -> bool {
        self.hard_tabs
    }

    /// What one indent is: a tab, or a tab's width of spaces.
    fn indent_text(&self) -> String {
        if self.hard_tabs {
            "\t".to_string()
        } else {
            " ".repeat(self.tab)
        }
    }

    /// The grapheme clusters of `text` (one line) with this buffer's tab
    /// width: see [`graphemes`].
    pub fn graphemes(&self, text: &str) -> Vec<(usize, usize, usize)> {
        graphemes_with(text, self.tab)
    }

    /// The cells `text` (one line) takes with this buffer's tab width.
    pub fn cells(&self, text: &str) -> usize {
        cells_with(text, self.tab)
    }

    /// At most `chars` characters (grapheme clusters, line breaks one each);
    /// text already there beyond it is cut.
    pub fn set_limit(&mut self, chars: Option<usize>) {
        self.limit = chars;
        if let Some(limit) = chars {
            self.cut_to(limit);
        }
    }

    fn cut_to(&mut self, limit: usize) {
        if self.count() > limit {
            let text = self.text();
            let kept = truncate(&text, limit).to_string();
            self.lines = kept.split('\n').map(str::to_string).collect();
            self.chars = chars_of(&self.lines);
            self.caret = self.clamp(self.caret);
            self.anchor = None;
            self.revision += 1;
        }
    }

    pub fn limit(&self) -> Option<usize> {
        self.limit
    }

    /// The text, lines joined with `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn line(&self, index: usize) -> &str {
        &self.lines[index]
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.len() == 1 && self.lines[0].is_empty()
    }

    /// Changes so far: a view draws again when it moves.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The characters in the text, as the limit counts them.
    pub fn count(&self) -> usize {
        self.chars
    }

    pub fn caret(&self) -> Pos {
        self.caret
    }

    /// The caret's line, and its column in characters.
    pub fn caret_column(&self) -> (usize, usize) {
        let Pos { line, at } = self.caret;
        (line, self.lines[line][..at].chars().count())
    }

    /// The nearest place to `pos` that is in the text and on a grapheme
    /// boundary.
    pub fn clamp(&self, pos: Pos) -> Pos {
        let line = pos.line.min(self.lines.len() - 1);
        let text = &self.lines[line];
        let at = stops(text)
            .into_iter()
            .take_while(|&stop| stop <= pos.at.min(text.len()))
            .last()
            .unwrap_or(0);
        Pos::new(line, at)
    }

    /// `pos`, where an edit put the caret, moved on to the end of the
    /// cluster it falls in. Only a non-ASCII character on either side of
    /// it can join a cluster across it, so typing ASCII stays cheap.
    fn snap(&self, pos: Pos, text: &str) -> Pos {
        let line = &self.lines[pos.line];
        let next = line[pos.at..].chars().next();
        let last = text.chars().next_back();
        if next.is_none_or(|c| c.is_ascii()) && last.is_none_or(|c| c.is_ascii()) {
            return pos;
        }
        let stops = stops(line);
        Pos::new(pos.line, stops[stops.partition_point(|&s| s < pos.at)])
    }

    /// Put the caret at `pos` (clamped), extending the selection from where
    /// the caret was when `select`, else dropping it.
    pub fn set_caret(&mut self, pos: Pos, select: bool) {
        let pos = self.clamp(pos);
        self.move_to(pos, select);
    }

    fn move_to(&mut self, pos: Pos, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = pos;
        self.end_run();
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
    }

    /// The selection, start before end; `None` when nothing is selected.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor?;
        (anchor != self.caret).then(|| {
            if anchor < self.caret {
                (anchor, self.caret)
            } else {
                (self.caret, anchor)
            }
        })
    }

    /// Select from `start` to `end`, the caret at `end`.
    pub fn select(&mut self, start: Pos, end: Pos) {
        let (start, end) = (self.clamp(start), self.clamp(end));
        self.anchor = (start != end).then_some(start);
        self.caret = end;
        self.end_run();
    }

    pub fn select_all(&mut self) {
        let last = self.lines.len() - 1;
        self.select(Pos::default(), Pos::new(last, self.lines[last].len()));
    }

    /// The text between `start` and `end`.
    pub fn slice(&self, start: Pos, end: Pos) -> String {
        if start.line == end.line {
            return self.lines[start.line][start.at..end.at].to_string();
        }
        let mut out = self.lines[start.line][start.at..].to_string();
        for line in &self.lines[start.line + 1..end.line] {
            out.push('\n');
            out.push_str(line);
        }
        out.push('\n');
        out.push_str(&self.lines[end.line][..end.at]);
        out
    }

    pub fn selected_text(&self) -> Option<String> {
        self.selection().map(|(start, end)| self.slice(start, end))
    }

    /// Replace the text from `start` to `end` with `text` (not cleaned: see
    /// [`clean`]), as one change of its own; the caret goes after it. The
    /// primitive every edit is made of.
    pub fn replace(&mut self, start: Pos, end: Pos, text: &str) {
        let (start, end) = (self.clamp(start), self.clamp(end));
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        self.edit(start, end, text, None);
    }

    /// Splice `text` in for `start..end`: where the inserted text ends, and
    /// the text taken out.
    fn splice(&mut self, start: Pos, end: Pos, text: &str) -> (Pos, String) {
        let removed = self.slice(start, end);
        let (pieces, column) = self.pieces(start, end, text);
        let last = pieces.len() - 1;
        self.chars = self.count_with(start, end, &pieces);
        self.lines.splice(start.line..=end.line, pieces);
        self.revision += 1;
        (Pos::new(start.line + last, column), removed)
    }

    /// The lines `text` makes in place of `start..end`'s, and the column
    /// where it ends on the last of them.
    fn pieces(&self, start: Pos, end: Pos, text: &str) -> (Vec<String>, usize) {
        let prefix = &self.lines[start.line][..start.at];
        let suffix = &self.lines[end.line][end.at..];
        let mut pieces: Vec<String> = text.split('\n').map(str::to_string).collect();
        let last = pieces.len() - 1;
        let column = pieces[last].len() + if last == 0 { prefix.len() } else { 0 };
        pieces[0].insert_str(0, prefix);
        pieces[last].push_str(suffix);
        (pieces, column)
    }

    /// The count once `pieces` replace the lines from `start` to `end`.
    fn count_with(&self, start: Pos, end: Pos, pieces: &[String]) -> usize {
        self.chars - chars_of(&self.lines[start.line..=end.line]) + chars_of(pieces)
    }

    /// Insert `text` (cleaned) at the caret, replacing the selection, as
    /// much of it as the limit leaves room for. Typing one character after
    /// another undoes as one step.
    pub fn insert(&mut self, text: &str) {
        let text = clean(text);
        let (start, end) = self.selection().unwrap_or((self.caret, self.caret));
        let mut text = text.as_str();
        if let Some(limit) = self.limit {
            let removed = length(&self.slice(start, end));
            let room = (limit + removed).saturating_sub(self.count());
            // Joining can merge a cluster at each end, so try a little more
            // and give back what does not fit.
            text = truncate(text, room.saturating_add(2));
            while !text.is_empty() {
                let (pieces, _) = self.pieces(start, end, text);
                if self.count_with(start, end, &pieces) <= limit {
                    break;
                }
                text = truncate(text, length(text) - 1);
            }
            if text.is_empty() && start == end {
                return;
            }
        }
        let typing = start == end && !text.contains('\n') && length(text) == 1;
        self.edit(start, end, text, typing.then_some(Run::Typing));
    }

    /// Record and make one edit, joining the last step when it continues
    /// the same run (typing at the caret, deleting at it).
    fn edit(&mut self, start: Pos, end: Pos, text: &str, run: Option<Run>) {
        let before = (self.caret, self.anchor);
        let (after, removed) = self.splice(start, end, text);
        // Text that went in before a combining mark joins its cluster: the
        // caret goes to the cluster's end, never inside it.
        let after = self.snap(after, text);
        self.caret = after;
        self.anchor = None;
        self.redo.clear();
        let change = Change {
            start,
            removed,
            inserted: text.to_string(),
        };
        if let Some(step) = self.undo.back_mut() {
            if run.is_some() && step.run == run && step.after.0 == before.0 {
                step.changes.push(change);
                step.after = (after, None);
                return;
            }
        }
        self.undo.push_back(Step {
            changes: vec![change],
            before,
            after: (after, None),
            run,
        });
        self.trim_history();
    }

    /// The step being typed or deleted is done: the next edit starts a new
    /// one.
    fn end_run(&mut self) {
        if let Some(step) = self.undo.back_mut() {
            step.run = None;
        }
    }

    /// Delete the selection, else the grapheme before the caret (joining
    /// the line to the one above at its start).
    pub fn backspace(&mut self) {
        if let Some((start, end)) = self.selection() {
            self.edit(start, end, "", None);
            return;
        }
        let start = self.before(self.caret);
        if start != self.caret {
            self.edit(start, self.caret, "", Some(Run::Deleting));
        }
    }

    /// Delete the selection, else the grapheme after the caret.
    pub fn delete(&mut self) {
        if let Some((start, end)) = self.selection() {
            self.edit(start, end, "", None);
            return;
        }
        let end = self.after(self.caret);
        if end != self.caret {
            let caret = self.caret;
            self.edit(caret, end, "", Some(Run::Deleting));
            self.caret = caret;
        }
    }

    /// Delete from the line's start to the caret.
    pub fn delete_to_start(&mut self) {
        let start = Pos::new(self.caret.line, 0);
        self.edit(start, self.caret, "", None);
    }

    /// Delete from the caret to the line's end.
    pub fn delete_to_end(&mut self) {
        let caret = self.caret;
        let end = Pos::new(caret.line, self.lines[caret.line].len());
        self.edit(caret, end, "", None);
        self.caret = caret;
    }

    /// Delete the word before the caret, and the spaces after it (Ctrl+W):
    /// back to the previous space on the line.
    pub fn delete_word(&mut self) {
        let Pos { line, at } = self.caret;
        let before = &self.lines[line][..at];
        let trimmed = before.trim_end_matches(' ');
        let start = trimmed.rfind(' ').map_or(0, |space| space + 1);
        self.edit(Pos::new(line, start), self.caret, "", None);
    }

    /// Delete from the caret to the end of the word after it (Alt+D in
    /// Emacs and readline, where it kills: [`kill_next_word`](Self::kill_next_word)).
    pub fn delete_next_word(&mut self) {
        let caret = self.caret;
        let end = self.word_end_after(caret);
        if end != caret {
            self.edit(caret, end, "", None);
            self.caret = caret;
        }
    }

    /// Kill from `start` to `end`: delete it and put it on the kill ring.
    /// A kill straight after a kill joins it, as one entry: after it when
    /// `forward`, before it otherwise, so killing a line piece by piece
    /// yanks back as the line.
    pub fn kill(&mut self, start: Pos, end: Pos, forward: bool) {
        let (start, end) = (self.clamp(start), self.clamp(end));
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        if start == end {
            return;
        }
        let text = self.slice(start, end);
        let joins = self.killed == Some((self.revision, self.caret)) && !self.kills.is_empty();
        self.edit(start, end, "", None);
        match self.kills.last_mut() {
            Some(last) if joins && forward => last.push_str(&text),
            Some(last) if joins => last.insert_str(0, &text),
            _ => self.push_kill(&text),
        }
        self.killed = Some((self.revision, self.caret));
        self.yanked = None;
    }

    /// Put `text` on the kill ring as its newest entry, as a kill would:
    /// what [`yank`](Self::yank) puts in next.
    pub fn push_kill(&mut self, text: &str) {
        self.kills.push(text.to_string());
        if self.kills.len() > KILLS {
            self.kills.remove(0);
        }
        self.killed = None;
    }

    /// The kill ring, the newest last.
    pub fn kills(&self) -> &[String] {
        &self.kills
    }

    /// Kill from the caret to the line's end (Ctrl+K); at the end of a line,
    /// the line break.
    ///
    /// ```
    /// use rich_interact::edit::Buffer;
    ///
    /// let mut buffer = Buffer::new("one\ntwo");
    /// buffer.kill_to_end();
    /// buffer.kill_to_end();
    /// assert_eq!(buffer.text(), "two");
    /// buffer.end(false);
    /// buffer.yank();
    /// assert_eq!(buffer.text(), "twoone\n", "the two kills yank as one");
    /// ```
    pub fn kill_to_end(&mut self) {
        let caret = self.caret;
        let length = self.lines[caret.line].len();
        let end = if caret.at < length {
            Pos::new(caret.line, length)
        } else {
            self.after(caret)
        };
        self.kill(caret, end, true);
    }

    /// Kill from the line's start to the caret (Ctrl+U).
    pub fn kill_to_start(&mut self) {
        let caret = self.caret;
        self.kill(Pos::new(caret.line, 0), caret, false);
    }

    /// Kill the selection, else the word before the caret back to the
    /// previous space, as [`delete_word`](Self::delete_word) deletes it
    /// (Ctrl+W).
    pub fn kill_word(&mut self) {
        if let Some((start, end)) = self.selection() {
            self.kill(start, end, true);
            return;
        }
        let Pos { line, at } = self.caret;
        let before = &self.lines[line][..at];
        let start = before
            .trim_end_matches(' ')
            .rfind(' ')
            .map_or(0, |space| space + 1);
        self.kill(Pos::new(line, start), self.caret, false);
    }

    /// Kill to the end of the word after the caret (Alt+D).
    pub fn kill_next_word(&mut self) {
        let end = self.word_end_after(self.caret);
        self.kill(self.caret, end, true);
    }

    /// Kill to the start of the word before the caret (Alt+Backspace).
    pub fn kill_previous_word(&mut self) {
        let start = self.word_start_before(self.caret);
        self.kill(start, self.caret, false);
    }

    /// Kill the selection, if there is one; whether there was.
    pub fn kill_selection(&mut self) -> bool {
        match self.selection() {
            Some((start, end)) => {
                self.kill(start, end, true);
                true
            }
            None => false,
        }
    }

    /// Put the newest kill in at the caret (replacing the selection), the
    /// caret after it (Ctrl+Y in Emacs); whether there was one.
    pub fn yank(&mut self) -> bool {
        let Some(index) = self.kills.len().checked_sub(1) else {
            return false;
        };
        self.yank_index(index, None);
        true
    }

    /// Right after a [`yank`](Self::yank) (or another `yank_pop`), swap what
    /// it put in for the kill before it on the ring, going round (Alt+Y);
    /// whether it did.
    ///
    /// ```
    /// use rich_interact::edit::Buffer;
    ///
    /// let mut buffer = Buffer::new("");
    /// buffer.push_kill("first");
    /// buffer.push_kill("second");
    /// buffer.yank();
    /// assert_eq!(buffer.text(), "second");
    /// assert!(buffer.yank_pop());
    /// assert_eq!(buffer.text(), "first");
    /// buffer.left(false);
    /// assert!(!buffer.yank_pop(), "only straight after a yank");
    /// ```
    pub fn yank_pop(&mut self) -> bool {
        let Some(yanked) = self.yanked else {
            return false;
        };
        if yanked.revision != self.revision || yanked.end != self.caret || self.kills.is_empty() {
            self.yanked = None;
            return false;
        }
        let count = self.kills.len();
        let index = (yanked.index.min(count - 1) + count - 1) % count;
        self.yank_index(index, Some((yanked.start, yanked.end)));
        true
    }

    /// Put kill `index` in place of `range` (else the selection or the
    /// caret), remembering it for [`yank_pop`](Self::yank_pop).
    fn yank_index(&mut self, index: usize, range: Option<(Pos, Pos)>) {
        let text = self.kills[index].clone();
        if let Some((start, end)) = range {
            self.select(start, end);
        }
        let start = self.selection().map_or(self.caret, |(start, _)| start);
        self.insert(&text);
        self.end_run();
        self.killed = None;
        self.yanked = Some(Yanked {
            start,
            end: self.caret,
            index,
            revision: self.revision,
        });
    }

    /// Whether line `index` holds only spaces.
    fn blank(&self, index: usize) -> bool {
        self.lines[index].trim().is_empty()
    }

    /// To the blank line after this paragraph (Alt+} in Emacs, `}` in Vim),
    /// else the end of the text.
    pub fn paragraph_down(&mut self, select: bool) {
        let count = self.lines.len();
        let mut line = self.caret.line;
        while line + 1 < count && self.blank(line) {
            line += 1;
        }
        while line < count && !self.blank(line) {
            line += 1;
        }
        let to = if line < count {
            Pos::new(line, 0)
        } else {
            Pos::new(count - 1, self.lines[count - 1].len())
        };
        self.move_to(to, select);
    }

    /// To the blank line before this paragraph (Alt+{ in Emacs, `{` in
    /// Vim), else the start of the text.
    pub fn paragraph_up(&mut self, select: bool) {
        let mut line = self.caret.line;
        while line > 0 && self.blank(line) {
            line -= 1;
        }
        while line > 0 && !self.blank(line) {
            line -= 1;
        }
        self.move_to(Pos::new(line, 0), select);
    }

    /// Find `search` from now on (`None` stops): the matches are
    /// [`matches`](Self::matches), and [`search_forward`](Self::search_forward)
    /// and [`search_back`](Self::search_back) go to them.
    ///
    /// ```
    /// use rich_interact::edit::{Buffer, Pos, Search};
    ///
    /// let mut buffer = Buffer::new("one fish\ntwo fish\nred fish");
    /// buffer.set_search(Some(Search::plain("fish")));
    /// assert_eq!(buffer.match_count(), 3);
    /// assert!(buffer.search_forward(false));
    /// assert_eq!(buffer.caret(), Pos::new(0, 4));
    /// assert!(buffer.search_back(false), "round to the last");
    /// assert_eq!(buffer.caret(), Pos::new(2, 4));
    /// assert_eq!(buffer.match_index(), Some(2));
    /// assert!(buffer.select_match(true), "round to the first, selected");
    /// assert_eq!(buffer.selected_text().as_deref(), Some("fish"));
    /// ```
    pub fn set_search(&mut self, search: Option<Search>) {
        self.search = search;
        self.searches += 1;
    }

    pub fn search(&self) -> Option<&Search> {
        self.search.as_ref()
    }

    /// The matches on lines `lines` (as many as there are), in order:
    /// what a view highlights.
    pub fn matches(&self, lines: Range<usize>) -> Vec<(Pos, Pos)> {
        let Some(search) = &self.search else {
            return Vec::new();
        };
        let end = lines.end.min(self.lines.len());
        (lines.start.min(end)..end)
            .flat_map(|line| {
                search
                    .find_in(&self.lines[line])
                    .into_iter()
                    .map(move |r| (Pos::new(line, r.start), Pos::new(line, r.end)))
            })
            .collect()
    }

    /// Every match, found once a revision and search.
    fn all_matches(&self) -> std::cell::Ref<'_, Vec<(Pos, Pos)>> {
        let key = (self.revision, self.searches);
        let fresh = matches!(&*self.found.borrow(), Some((r, s, _)) if (*r, *s) == key);
        if !fresh {
            let all = self.matches(0..self.lines.len());
            *self.found.borrow_mut() = Some((key.0, key.1, all));
        }
        std::cell::Ref::map(self.found.borrow(), |found| {
            &found.as_ref().expect("just found").2
        })
    }

    /// How many matches there are in the whole text.
    pub fn match_count(&self) -> usize {
        self.all_matches().len()
    }

    /// Which match (from 0) starts at the caret, or at the selection's
    /// start: the one [`search_forward`](Self::search_forward) or
    /// [`select_match`](Self::select_match) went to.
    pub fn match_index(&self) -> Option<usize> {
        let at = self.selection().map_or(self.caret, |(start, _)| start);
        let all = self.all_matches();
        all.binary_search_by(|(start, _)| start.cmp(&at)).ok()
    }

    /// The first match starting after `from` (at it too when `inclusive`),
    /// going forward or back, round past the end.
    fn next_match(&self, from: Pos, forward: bool, inclusive: bool) -> Option<(Pos, Pos)> {
        let search = self.search.as_ref()?;
        let count = self.lines.len();
        let hits = |line: usize| -> Vec<(Pos, Pos)> {
            search
                .find_in(&self.lines[line])
                .into_iter()
                .map(|r| (Pos::new(line, r.start), Pos::new(line, r.end)))
                .collect()
        };
        let beyond = |start: Pos| {
            if forward {
                start > from || inclusive && start == from
            } else {
                start < from || inclusive && start == from
            }
        };
        // The rest of the line `from` is on.
        let here = hits(from.line);
        let first = if forward {
            here.iter().find(|m| beyond(m.0))
        } else {
            here.iter().rev().find(|m| beyond(m.0))
        };
        if let Some(hit) = first {
            return Some(*hit);
        }
        // The other lines, round past the end.
        for step in 1..count {
            let line = if forward {
                (from.line + step) % count
            } else {
                (from.line + count - step) % count
            };
            let found = hits(line);
            if let Some(hit) = if forward { found.first() } else { found.last() } {
                return Some(*hit);
            }
        }
        // Back on the first line, from its other end.
        if forward {
            here.first().copied()
        } else {
            here.last().copied()
        }
    }

    /// Move the caret to the start of the next match after it, round past
    /// the end; whether there is one.
    pub fn search_forward(&mut self, select: bool) -> bool {
        self.search_to(true, select)
    }

    /// Move the caret to the start of the match before it, round past the
    /// start; whether there is one.
    pub fn search_back(&mut self, select: bool) -> bool {
        self.search_to(false, select)
    }

    fn search_to(&mut self, forward: bool, select: bool) -> bool {
        match self.next_match(self.caret, forward, false) {
            Some((start, _)) => {
                let start = self.clamp(start);
                self.move_to(start, select);
                true
            }
            None => false,
        }
    }

    /// Select the next match (`forward`) or the one before, the caret at
    /// its end: a find bar's Enter. With nothing selected, a match at the
    /// caret counts as the next; whether there is one.
    pub fn select_match(&mut self, forward: bool) -> bool {
        let (from, inclusive) = match self.selection() {
            Some((start, _)) => (start, false),
            None => (self.caret, forward),
        };
        match self.next_match(from, forward, inclusive) {
            Some((start, end)) => {
                self.select(start, end);
                true
            }
            None => false,
        }
    }

    /// Indent: the selected lines (all of each) by a tab's width of spaces
    /// (four, unless [`set_tab_width`](Self::set_tab_width) says otherwise;
    /// a tab with [`set_hard_tabs`](Self::set_hard_tabs)), else as much at
    /// the caret.
    pub fn indent(&mut self) {
        let indent = self.indent_text();
        match self.selection() {
            Some((start, end)) if start.line != end.line => {
                self.each_line(start.line, end, |_| Some((0, 0, indent.clone())));
            }
            _ => self.insert(&indent),
        }
    }

    /// Outdent: up to a tab's width of leading spaces (or a tab) off the
    /// selected lines, or the caret's.
    pub fn outdent(&mut self) {
        let (first, end) = match self.selection() {
            Some((start, end)) => (start.line, end),
            None => (self.caret.line, self.caret),
        };
        let tab = self.tab;
        self.each_line(first, end, |line| {
            let n = if line.starts_with('\t') {
                1
            } else {
                line.len() - line.trim_start_matches(' ').len()
            }
            .min(tab);
            (n > 0).then_some((0, n, String::new()))
        });
    }

    /// Edit lines `first` to `end.line` (not the last when the selection
    /// ends at its start) with `f`: (start, end, text) to replace in each;
    /// one undo step. The selection moves with the text.
    fn each_line(
        &mut self,
        first: usize,
        end: Pos,
        f: impl Fn(&str) -> Option<(usize, usize, String)>,
    ) {
        let last = if end.at == 0 && end.line > first {
            end.line - 1
        } else {
            end.line
        };
        let before = (self.caret, self.anchor);
        let mut changes = Vec::new();
        let (mut caret, mut anchor) = (self.caret, self.anchor);
        for line in first..=last {
            let Some((from, to, text)) = f(&self.lines[line]) else {
                continue;
            };
            let start = Pos::new(line, from);
            let (_, removed) = self.splice(start, Pos::new(line, to), &text);
            let shift = |pos: &mut Pos| {
                if pos.line == line && pos.at > from {
                    pos.at = (pos.at + text.len()).saturating_sub(to - from).max(from);
                }
            };
            shift(&mut caret);
            if let Some(anchor) = anchor.as_mut() {
                shift(anchor);
            }
            changes.push(Change {
                start,
                removed,
                inserted: text,
            });
        }
        if changes.is_empty() {
            return;
        }
        self.caret = self.clamp(caret);
        self.anchor = anchor.map(|a| self.clamp(a));
        self.redo.clear();
        self.undo.push_back(Step {
            changes,
            before,
            after: (self.caret, self.anchor),
            run: None,
        });
        self.trim_history();
    }

    /// Undo the last step; whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop_back() else {
            return false;
        };
        for change in step.changes.iter().rev() {
            let end = end_of(change.start, &change.inserted);
            self.splice(change.start, end, &change.removed);
        }
        (self.caret, self.anchor) = step.before;
        let mut step = step;
        step.run = None;
        self.redo.push(step);
        true
    }

    /// Redo the last step undone; whether there was one.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else {
            return false;
        };
        for change in &step.changes {
            let end = end_of(change.start, &change.removed);
            self.splice(change.start, end, &change.inserted);
        }
        (self.caret, self.anchor) = step.after;
        self.undo.push_back(step);
        true
    }

    /// The place one grapheme before `pos` (the end of the line above at a
    /// line's start).
    pub fn before(&self, pos: Pos) -> Pos {
        if pos.at == 0 {
            return if pos.line > 0 {
                Pos::new(pos.line - 1, self.lines[pos.line - 1].len())
            } else {
                pos
            };
        }
        Pos::new(pos.line, stop_before(&stops(&self.lines[pos.line]), pos.at))
    }

    /// The place one grapheme after `pos` (the start of the next line at a
    /// line's end).
    pub fn after(&self, pos: Pos) -> Pos {
        let length = self.lines[pos.line].len();
        if pos.at >= length {
            return if pos.line + 1 < self.lines.len() {
                Pos::new(pos.line + 1, 0)
            } else {
                pos
            };
        }
        Pos::new(
            pos.line,
            stop_after(&stops(&self.lines[pos.line]), pos.at, length),
        )
    }

    /// [`before`](Self::before) or [`after`](Self::after), keeping the
    /// stops of the line between calls, so a word move stays linear in the
    /// line's length.
    fn step(&self, pos: Pos, forward: bool, cache: &mut Option<(usize, Vec<usize>)>) -> Pos {
        let length = self.lines[pos.line].len();
        if forward && pos.at >= length || !forward && pos.at == 0 {
            return if forward {
                self.after(pos)
            } else {
                self.before(pos)
            };
        }
        let stops = match cache {
            Some((line, stops)) if *line == pos.line => stops,
            _ => &mut cache.insert((pos.line, stops(&self.lines[pos.line]))).1,
        };
        let at = if forward {
            stop_after(stops, pos.at, length)
        } else {
            stop_before(stops, pos.at)
        };
        Pos::new(pos.line, at)
    }

    /// Left one grapheme; without `select`, a selection collapses to its
    /// start instead.
    pub fn left(&mut self, select: bool) {
        let to = match self.selection() {
            Some((start, _)) if !select => start,
            _ => self.before(self.caret),
        };
        self.move_to(to, select);
    }

    /// Right one grapheme; without `select`, a selection collapses to its
    /// end instead.
    pub fn right(&mut self, select: bool) {
        let to = match self.selection() {
            Some((_, end)) if !select => end,
            _ => self.after(self.caret),
        };
        self.move_to(to, select);
    }

    /// Move `delta` lines, keeping the column in cells as near as it goes.
    pub fn vertical(&mut self, delta: isize, select: bool) {
        let target = self
            .caret
            .line
            .saturating_add_signed(delta)
            .min(self.lines.len() - 1);
        let column = self.cells(&self.lines[self.caret.line][..self.caret.at]);
        let (mut used, mut at) = (0, 0);
        for (_, end, width) in self.graphemes(&self.lines[target]) {
            if used + width > column {
                break;
            }
            used += width;
            at = end;
        }
        self.move_to(Pos::new(target, at), select);
    }

    pub fn home(&mut self, select: bool) {
        self.move_to(Pos::new(self.caret.line, 0), select);
    }

    pub fn end(&mut self, select: bool) {
        let line = self.caret.line;
        self.move_to(Pos::new(line, self.lines[line].len()), select);
    }

    pub fn top(&mut self, select: bool) {
        self.move_to(Pos::default(), select);
    }

    pub fn bottom(&mut self, select: bool) {
        let last = self.lines.len() - 1;
        self.move_to(Pos::new(last, self.lines[last].len()), select);
    }

    /// To the start of the word before the caret (Ctrl+←): over spaces and
    /// punctuation, then over a word, crossing lines.
    pub fn word_left(&mut self, select: bool) {
        let pos = self.word_start_before(self.caret);
        self.move_to(pos, select);
    }

    /// To the end of the word after the caret (Ctrl+→).
    pub fn word_right(&mut self, select: bool) {
        let pos = self.word_end_after(self.caret);
        self.move_to(pos, select);
    }

    /// Where [`word_left`](Self::word_left) goes from `pos`.
    pub fn word_start_before(&self, pos: Pos) -> Pos {
        let mut pos = pos;
        let mut cache = None;
        // Over what is not a word, back to the end of one.
        loop {
            let before = self.step(pos, false, &mut cache);
            if before == pos || self.is_word(before, pos) {
                break;
            }
            pos = before;
        }
        loop {
            let before = self.step(pos, false, &mut cache);
            if before == pos || !self.is_word(before, pos) {
                break;
            }
            pos = before;
        }
        pos
    }

    /// Where [`word_right`](Self::word_right) goes from `pos`.
    pub fn word_end_after(&self, pos: Pos) -> Pos {
        let mut pos = pos;
        let mut cache = None;
        loop {
            let after = self.step(pos, true, &mut cache);
            if after == pos || self.is_word(pos, after) {
                break;
            }
            pos = after;
        }
        loop {
            let after = self.step(pos, true, &mut cache);
            if after == pos || !self.is_word(pos, after) {
                break;
            }
            pos = after;
        }
        pos
    }

    /// Whether the grapheme from `start` to `end` (on one line) is part of
    /// a word: letters, digits and `_`.
    fn is_word(&self, start: Pos, end: Pos) -> bool {
        start.line == end.line
            && self.lines[start.line][start.at..end.at]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
    }

    /// The word at `pos`, or the run of spaces or the single other
    /// character there (a double click): its start and end.
    pub fn word_at(&self, pos: Pos) -> (Pos, Pos) {
        let pos = self.clamp(pos);
        let line = &self.lines[pos.line];
        let stops = stops(line);
        let kind = |at: usize| {
            line[at..]
                .chars()
                .next()
                .map(|c| (c.is_alphanumeric() || c == '_', c.is_whitespace()))
        };
        let Some(here) = kind(pos.at).or_else(|| {
            // At the end of the line: the word before.
            stops.iter().rev().nth(1).and_then(|&at| kind(at))
        }) else {
            return (pos, pos);
        };
        let index = stops
            .iter()
            .position(|&s| s == pos.at)
            .unwrap_or(stops.len() - 1)
            .min(stops.len().saturating_sub(2));
        if !here.0 && !here.1 {
            return (
                Pos::new(pos.line, stops[index]),
                Pos::new(pos.line, stops[index + 1]),
            );
        }
        let mut first = index;
        while first > 0 && kind(stops[first - 1]) == Some(here) {
            first -= 1;
        }
        let mut last = index + 1;
        while last < stops.len() - 1 && kind(stops[last]) == Some(here) {
            last += 1;
        }
        (
            Pos::new(pos.line, stops[first]),
            Pos::new(pos.line, stops[last]),
        )
    }
}

/// Where `text` ends when it starts at `start`.
fn end_of(start: Pos, text: &str) -> Pos {
    let lines = text.split('\n').count() - 1;
    let last = text.rsplit('\n').next().unwrap_or("");
    if lines == 0 {
        Pos::new(start.line, start.at + text.len())
    } else {
        Pos::new(start.line + lines, last.len())
    }
}

/// Byte offsets where a caret may stop in `line`: grapheme starts and the
/// end.
pub fn stops(line: &str) -> Vec<usize> {
    let mut stops: Vec<usize> = graphemes(line).iter().map(|(start, _, _)| *start).collect();
    stops.push(line.len());
    stops
}

/// The last of `stops` before `at` (0 if none).
fn stop_before(stops: &[usize], at: usize) -> usize {
    match stops.partition_point(|&s| s < at) {
        0 => 0,
        i => stops[i - 1],
    }
}

/// The first of `stops` after `at` (`length` if none).
fn stop_after(stops: &[usize], at: usize, length: usize) -> usize {
    stops
        .get(stops.partition_point(|&s| s <= at))
        .copied()
        .unwrap_or(length)
}

/// What [`Buffer::count`] counts, for `lines` alone: their clusters and the
/// breaks between them.
fn chars_of(lines: &[String]) -> usize {
    lines
        .iter()
        .map(|line| graphemes(line).len())
        .sum::<usize>()
        + lines.len().saturating_sub(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_in_a_run_undoes_as_one_step() {
        let mut b = Buffer::new("");
        for c in ["h", "i", " ", "y", "o", "u"] {
            b.insert(c);
        }
        b.insert("\n");
        b.insert("x");
        assert_eq!(b.text(), "hi you\nx");
        assert!(b.undo());
        assert_eq!(b.text(), "hi you\n");
        assert!(b.undo());
        assert_eq!(b.text(), "hi you");
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert!(!b.undo());
        assert!(b.redo() && b.redo());
        assert_eq!(b.text(), "hi you\n");
        assert_eq!(b.caret(), Pos::new(1, 0));
    }

    #[test]
    fn moving_the_caret_ends_a_run() {
        let mut b = Buffer::new("");
        b.insert("a");
        b.insert("b");
        b.left(false);
        b.insert("c");
        assert_eq!(b.text(), "acb");
        b.undo();
        assert_eq!(b.text(), "ab");
    }

    #[test]
    fn the_caret_moves_by_grapheme_cluster() {
        let mut b = Buffer::new("e\u{301}👨‍👩‍👧x");
        b.right(false);
        assert_eq!(b.caret().at, "e\u{301}".len());
        b.right(false);
        assert_eq!(b.caret().at, "e\u{301}👨‍👩‍👧".len());
        b.backspace();
        assert_eq!(b.text(), "e\u{301}x");
    }

    #[test]
    fn typing_replaces_the_selection_and_shift_moves_select() {
        let mut b = Buffer::new("hello world");
        b.word_right(true);
        assert_eq!(b.selected_text().as_deref(), Some("hello"));
        b.insert("bye");
        assert_eq!(b.text(), "bye world");
        b.undo();
        assert_eq!(b.text(), "hello world");
        assert_eq!(
            b.selected_text().as_deref(),
            Some("hello"),
            "undo restores it"
        );
    }

    #[test]
    fn word_moves_skip_spaces_and_cross_lines() {
        let mut b = Buffer::new("one two\n  three");
        b.word_right(false);
        assert_eq!(b.caret(), Pos::new(0, 3));
        b.word_right(false);
        assert_eq!(b.caret(), Pos::new(0, 7));
        b.word_right(false);
        assert_eq!(b.caret(), Pos::new(1, 7));
        b.word_left(false);
        assert_eq!(b.caret(), Pos::new(1, 2));
        b.word_left(false);
        assert_eq!(b.caret(), Pos::new(0, 4));
    }

    #[test]
    fn indent_and_outdent_the_selected_lines_as_one_step() {
        let mut b = Buffer::new("a\nb\nc");
        b.select(Pos::new(0, 0), Pos::new(1, 1));
        b.indent();
        assert_eq!(b.text(), "    a\n    b\nc");
        assert_eq!(b.selected_text().as_deref(), Some("    a\n    b"));
        b.outdent();
        assert_eq!(b.text(), "a\nb\nc");
        b.undo();
        b.undo();
        assert_eq!(b.text(), "a\nb\nc");
        // No selection: indent types four spaces, outdent takes them off.
        b.set_caret(Pos::new(2, 0), false);
        b.indent();
        assert_eq!(b.line(2), "    c");
        b.outdent();
        assert_eq!(b.line(2), "c");
    }

    #[test]
    fn the_limit_counts_clusters_and_line_breaks() {
        let mut b = Buffer::new("");
        b.set_limit(Some(4));
        b.insert("ab\ncdef");
        assert_eq!(b.text(), "ab\nc");
        b.insert("z");
        assert_eq!(b.text(), "ab\nc");
        // A combining accent joins the cluster before it: no room needed.
        b.insert("\u{301}");
        assert_eq!(b.text(), "ab\nc\u{301}");
        assert_eq!(b.count(), 4);
    }

    #[test]
    fn a_double_click_takes_the_word() {
        let b = Buffer::new("let x_1 = foo(bar);");
        assert_eq!(b.word_at(Pos::new(0, 5)), (Pos::new(0, 4), Pos::new(0, 7)));
        assert_eq!(
            b.word_at(Pos::new(0, 13)),
            (Pos::new(0, 13), Pos::new(0, 14))
        );
        let (start, end) = b.word_at(Pos::new(0, 19));
        assert_eq!(b.slice(start, end), ";");
    }

    #[test]
    fn deleting_forward_keeps_the_caret_and_undoes_in_a_run() {
        let mut b = Buffer::new("abcd");
        b.delete();
        b.delete();
        assert_eq!((b.text().as_str(), b.caret()), ("cd", Pos::new(0, 0)));
        b.undo();
        assert_eq!(b.text(), "abcd");
    }

    #[test]
    fn kills_join_and_yank_back_and_cycle() {
        let mut b = Buffer::new("alpha beta gamma");
        b.end(false);
        b.kill_previous_word();
        b.kill_previous_word();
        assert_eq!(b.text(), "alpha ");
        assert_eq!(b.kills(), ["beta gamma"], "backward kills join before");
        b.home(false);
        b.kill_next_word();
        assert_eq!(b.text(), " ");
        assert_eq!(b.kills().len(), 2, "a move between kills starts a new one");
        assert!(b.yank());
        assert_eq!(b.text(), "alpha ");
        assert!(b.yank_pop());
        assert_eq!(b.text(), "beta gamma ");
        assert!(b.yank_pop(), "round the ring");
        assert_eq!(b.text(), "alpha ");
        b.undo();
        assert_eq!(b.text(), "beta gamma ");
    }

    #[test]
    fn a_kill_with_a_selection_takes_it() {
        let mut b = Buffer::new("one two");
        b.word_right(true);
        b.kill_word();
        assert_eq!(b.text(), " two");
        assert_eq!(b.kills(), ["one"]);
        assert!(!b.kill_selection());
    }

    #[test]
    fn delete_next_word_keeps_the_caret_and_the_ring() {
        let mut b = Buffer::new("say hello world");
        b.set_caret(Pos::new(0, 3), false);
        b.delete_next_word();
        assert_eq!(b.text(), "say world");
        assert_eq!(b.caret(), Pos::new(0, 3));
        assert!(b.kills().is_empty());
        assert!(!b.yank());
    }

    #[test]
    fn paragraph_moves_stop_at_blank_lines() {
        let mut b = Buffer::new("a\nb\n\n\nc\nd\n\ne");
        b.paragraph_down(false);
        assert_eq!(b.caret(), Pos::new(2, 0));
        b.paragraph_down(false);
        assert_eq!(b.caret(), Pos::new(6, 0));
        b.paragraph_down(true);
        assert_eq!(b.caret(), Pos::new(7, 1), "the end of the text");
        assert_eq!(b.selected_text().as_deref(), Some("\ne"));
        b.paragraph_up(false);
        assert_eq!(b.caret(), Pos::new(6, 0));
        b.paragraph_up(false);
        assert_eq!(b.caret(), Pos::new(3, 0));
        b.paragraph_up(false);
        assert_eq!(b.caret(), Pos::new(0, 0));
    }

    #[test]
    fn the_history_keeps_the_newest_steps() {
        let mut b = Buffer::new("");
        assert_eq!(b.history_limit(), Some(HISTORY));
        b.set_history_limit(Some(3));
        for i in 0..10 {
            b.insert(&format!("{i}\n"));
        }
        let mut undone = 0;
        while b.undo() {
            undone += 1;
        }
        assert_eq!(undone, 3);
        assert_eq!(b.text(), "0\n1\n2\n3\n4\n5\n6\n");
        b.set_history_limit(None);
        for _ in 0..2000 {
            b.insert("\n");
        }
        let mut undone = 0;
        while b.undo() {
            undone += 1;
        }
        assert_eq!(undone, 2000, "unlimited");
    }

    #[test]
    fn tab_width_and_hard_tabs_set_the_indent() {
        let mut b = Buffer::new("x\ty");
        assert_eq!(b.cells("x\ty"), 6);
        b.set_tab_width(2);
        assert_eq!(b.cells("x\ty"), 4);
        b.indent();
        assert_eq!(b.text(), "  x\ty");
        b.outdent();
        assert_eq!(b.text(), "x\ty");
        b.set_hard_tabs(true);
        b.select_all();
        b.insert("a\nb");
        b.select_all();
        b.indent();
        assert_eq!(b.text(), "\ta\n\tb");
        b.outdent();
        assert_eq!(b.text(), "a\nb");
        // A move down keeps the column in cells, tabs counted at their width.
        let mut b = Buffer::new("\tx\nabcdef");
        b.set_tab_width(2);
        b.set_caret(Pos::new(0, 1), false);
        b.vertical(1, false);
        assert_eq!(b.caret(), Pos::new(1, 2));
    }

    #[test]
    fn set_text_keeps_the_settings_and_the_ring() {
        let mut b = Buffer::new("abc");
        b.set_tab_width(8);
        b.set_history_limit(Some(5));
        b.push_kill("k");
        b.set_search(Some(Search::plain("b")));
        b.set_text("bbb");
        assert_eq!(
            (b.tab_width(), b.history_limit(), b.kills().len()),
            (8, Some(5), 1)
        );
        assert_eq!(b.match_count(), 3);
        assert!(!b.undo());
    }

    #[test]
    fn search_goes_round_both_ways_and_follows_edits() {
        let mut b = Buffer::new("ab ab\nxx\nab");
        b.set_search(Some(Search::plain("ab")));
        assert_eq!(b.match_count(), 3);
        // Forward from the caret skips a match starting there.
        assert!(b.search_forward(false));
        assert_eq!(b.caret(), Pos::new(0, 3));
        assert!(b.search_forward(false));
        assert_eq!(b.caret(), Pos::new(2, 0));
        assert!(b.search_forward(false));
        assert_eq!(b.caret(), Pos::new(0, 0), "round past the end");
        assert!(b.search_back(false));
        assert_eq!(b.caret(), Pos::new(2, 0), "round past the start");
        assert_eq!(b.match_index(), Some(2));
        b.insert("ab");
        assert_eq!(b.match_count(), 4, "counted again after an edit");
        assert_eq!(
            b.matches(1..3),
            [
                (Pos::new(2, 0), Pos::new(2, 2)),
                (Pos::new(2, 2), Pos::new(2, 4))
            ]
        );
        b.set_search(Some(Search::regex("x+").unwrap()));
        assert_eq!(b.match_count(), 1);
        b.set_search(Some(Search::plain("")));
        assert_eq!(b.match_count(), 0);
        assert!(!b.search_forward(false));
        b.set_search(None);
        assert!(b.matches(0..3).is_empty());
    }

    #[test]
    fn select_match_takes_the_one_at_the_caret_then_moves_on() {
        let mut b = Buffer::new("aaaa");
        b.set_search(Some(Search::plain("aa")));
        assert!(b.select_match(true));
        assert_eq!(b.selection(), Some((Pos::new(0, 0), Pos::new(0, 2))));
        assert!(b.select_match(true));
        assert_eq!(b.selection(), Some((Pos::new(0, 2), Pos::new(0, 4))));
        assert!(b.select_match(false));
        assert_eq!(b.selection(), Some((Pos::new(0, 0), Pos::new(0, 2))));
        assert!(b.select_match(false), "round to the last");
        assert_eq!(b.selection(), Some((Pos::new(0, 2), Pos::new(0, 4))));
    }

    #[test]
    fn word_wrap_keeps_words_whole_and_covers_the_line() {
        let line = "a bb ccc dddd";
        for room in 1..15 {
            for mode in [WrapMode::Grapheme, WrapMode::Word] {
                let rows = wrap_line(line, room, 4, mode);
                assert_eq!(rows.first().unwrap().start, 0);
                assert_eq!(rows.last().unwrap().end, line.len());
                assert!(rows.windows(2).all(|w| w[0].end == w[1].start));
            }
        }
        let rows: Vec<&str> = wrap_line(line, 6, 4, WrapMode::Word)
            .into_iter()
            .map(|r| &line[r])
            .collect();
        assert_eq!(rows, ["a bb ", "ccc ", "dddd"]);
        assert_eq!(wrap_line(line, 6, 4, WrapMode::None).len(), 1);
        assert_eq!(wrap_line("", 6, 4, WrapMode::Word)[0], 0..0);
        // A wide character never splits, and a tab counts its width.
        assert_eq!(wrap_line("ab\tcd", 4, 2, WrapMode::Grapheme), [0..3, 3..5]);
    }

    #[test]
    fn pasted_controls_are_dropped() {
        assert_eq!(clean("a\r\nb\rc\td\x1b[2Je\u{9b}"), "a\nb\nc\td[2Je");
    }
}

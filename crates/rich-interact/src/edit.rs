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
//! moving the caret ends the run.
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

use rich::cells::split_graphemes;

/// The cells a tab takes on screen.
pub const TAB: usize = 4;

/// What [`Buffer::indent`] inserts, and [`Buffer::outdent`] takes away.
const INDENT: &str = "    ";

/// The grapheme clusters of `line` as (start, end, cells): rich's, with a
/// tab one of its own, [`TAB`] cells wide, rather than joined to the one
/// before it as a zero-width character is.
pub fn graphemes(line: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for piece in line.split('\t') {
        let (spans, _) = split_graphemes(piece);
        out.extend(spans.into_iter().map(|(s, e, w)| (start + s, start + e, w)));
        start += piece.len();
        if start < line.len() {
            out.push((start, start + 1, TAB));
            start += 1;
        }
    }
    out
}

/// The cells `text` (one line) takes on screen.
pub fn cells(text: &str) -> usize {
    graphemes(text).iter().map(|(_, _, width)| width).sum()
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
    undo: Vec<Step>,
    redo: Vec<Step>,
    /// Bumped by every change, so a view knows when to draw again.
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
        Buffer {
            lines: text.split('\n').map(str::to_string).collect(),
            caret: Pos::default(),
            anchor: None,
            limit: None,
            undo: Vec::new(),
            redo: Vec::new(),
            revision: 0,
        }
    }

    /// Replace everything with `text`, the caret at its start; the undo
    /// history is forgotten.
    pub fn set_text(&mut self, text: &str) {
        let limit = self.limit;
        *self = Buffer {
            revision: self.revision + 1,
            ..Buffer::new(text)
        };
        self.limit = limit;
        if let Some(limit) = limit {
            self.cut_to(limit);
        }
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
        self.lines
            .iter()
            .map(|line| graphemes(line).len())
            .sum::<usize>()
            + self.lines.len()
            - 1
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
        let prefix = self.lines[start.line][..start.at].to_string();
        let suffix = self.lines[end.line][end.at..].to_string();
        let mut pieces: Vec<String> = text.split('\n').map(str::to_string).collect();
        let last = pieces.len() - 1;
        let column = pieces[last].len() + if last == 0 { prefix.len() } else { 0 };
        pieces[0].insert_str(0, &prefix);
        pieces[last].push_str(&suffix);
        self.lines.splice(start.line..=end.line, pieces);
        self.revision += 1;
        (Pos::new(start.line + last, column), removed)
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
                let saved = (self.lines.clone(), self.caret, self.anchor);
                self.splice(start, end, text);
                let fits = self.count() <= limit;
                (self.lines, self.caret, self.anchor) = saved;
                self.revision += 1;
                if fits {
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
        self.caret = after;
        self.anchor = None;
        self.redo.clear();
        let change = Change {
            start,
            removed,
            inserted: text.to_string(),
        };
        if let Some(step) = self.undo.last_mut() {
            if run.is_some() && step.run == run && step.after.0 == before.0 {
                step.changes.push(change);
                step.after = (after, None);
                return;
            }
        }
        self.undo.push(Step {
            changes: vec![change],
            before,
            after: (after, None),
            run,
        });
    }

    /// The step being typed or deleted is done: the next edit starts a new
    /// one.
    fn end_run(&mut self) {
        if let Some(step) = self.undo.last_mut() {
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

    /// Indent: the selected lines (all of each) by four spaces, else four
    /// spaces at the caret.
    pub fn indent(&mut self) {
        match self.selection() {
            Some((start, end)) if start.line != end.line => {
                self.each_line(start.line, end, |_| Some((0, 0, INDENT)));
            }
            _ => self.insert(INDENT),
        }
    }

    /// Outdent: up to four leading spaces (or a tab) off the selected lines,
    /// or the caret's.
    pub fn outdent(&mut self) {
        let (first, end) = match self.selection() {
            Some((start, end)) => (start.line, end),
            None => (self.caret.line, self.caret),
        };
        self.each_line(first, end, |line| {
            let n = if line.starts_with('\t') {
                1
            } else {
                line.len() - line.trim_start_matches(' ').len()
            }
            .min(INDENT.len());
            (n > 0).then_some((0, n, ""))
        });
    }

    /// Edit lines `first` to `end.line` (not the last when the selection
    /// ends at its start) with `f`: (start, end, text) to replace in each;
    /// one undo step. The selection moves with the text.
    fn each_line(
        &mut self,
        first: usize,
        end: Pos,
        f: impl Fn(&str) -> Option<(usize, usize, &'static str)>,
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
            let (_, removed) = self.splice(start, Pos::new(line, to), text);
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
                inserted: text.to_string(),
            });
        }
        if changes.is_empty() {
            return;
        }
        self.caret = self.clamp(caret);
        self.anchor = anchor.map(|a| self.clamp(a));
        self.redo.clear();
        self.undo.push(Step {
            changes,
            before,
            after: (self.caret, self.anchor),
            run: None,
        });
    }

    /// Undo the last step; whether there was one.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else {
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
        self.undo.push(step);
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
        let at = stops(&self.lines[pos.line])
            .into_iter()
            .rev()
            .find(|&s| s < pos.at)
            .unwrap_or(0);
        Pos::new(pos.line, at)
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
        let at = stops(&self.lines[pos.line])
            .into_iter()
            .find(|&s| s > pos.at)
            .unwrap_or(length);
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
        let column = cells(&self.lines[self.caret.line][..self.caret.at]);
        let line = &self.lines[target];
        let at = stops(line)
            .into_iter()
            .take_while(|&stop| cells(&line[..stop]) <= column)
            .last()
            .unwrap_or(0);
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
        let mut pos = self.caret;
        // Over what is not a word, back to the end of one.
        loop {
            let before = self.before(pos);
            if before == pos || self.is_word(before, pos) {
                break;
            }
            pos = before;
        }
        loop {
            let before = self.before(pos);
            if before == pos || !self.is_word(before, pos) {
                break;
            }
            pos = before;
        }
        self.move_to(pos, select);
    }

    /// To the end of the word after the caret (Ctrl+→).
    pub fn word_right(&mut self, select: bool) {
        let mut pos = self.caret;
        loop {
            let after = self.after(pos);
            if after == pos || self.is_word(pos, after) {
                break;
            }
            pos = after;
        }
        loop {
            let after = self.after(pos);
            if after == pos || !self.is_word(pos, after) {
                break;
            }
            pos = after;
        }
        self.move_to(pos, select);
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
    fn pasted_controls_are_dropped() {
        assert_eq!(clean("a\r\nb\rc\td\x1b[2Je\u{9b}"), "a\nb\nc\td[2Je");
    }
}

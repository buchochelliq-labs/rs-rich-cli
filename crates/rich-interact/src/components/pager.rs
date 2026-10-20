//! A pager over rendered output, with search (#291).
//!
//! The content is any renderable, rendered at the terminal's width (and
//! again after a resize), or lines already rendered. It scrolls like the
//! [`Viewport`] it is built on. `/` starts a search: matches are marked in
//! place, `n` and `N` jump between them, and the status line counts them.
//! `q` or Escape closes it.
//!
//! Since 0.0.7 a table's header can stay on top while its rows scroll
//! (#716): the first lines given with [`Pager::sticky_header`], or a
//! table's header found in the content with
//! [`Pager::sticky_table_header`].

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Renderable, Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, text, Theme};
use crate::event::{Event, Key, KeyCode};
use crate::keymap::{keys, Keymap};
use crate::policy::{LineIo, NotInteractive};
use crate::viewport::Viewport;

/// One match: a line, and its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit {
    line: usize,
    start: usize,
    end: usize,
}

/// The keys of a [`Pager`], in context `pager`: `quit`, `search`,
/// `next-match`, `previous-match` and [`ScrollState::keymap`]'s.
///
/// [`ScrollState::keymap`]: crate::kit::ScrollState::keymap
pub fn pager_keymap() -> Keymap {
    let mut keymap = Keymap::new("pager")
        .bind("quit", keys("q escape"), "quit")
        .bind("search", keys("/"), "search")
        .bind("next-match", keys("n"), "next match")
        .bind("previous-match", keys("N"), "previous match");
    keymap.extend(crate::kit::ScrollState::keymap("pager"));
    keymap
}

/// The keys of a [`Pager`] while a search is typed, in context
/// `pager-search`.
pub fn pager_search_keymap() -> Keymap {
    Keymap::new("pager-search")
        .bind("find", keys("enter"), "find")
        .bind("cancel", keys("escape"), "stop searching")
        .bind("delete", keys("backspace"), "delete a character")
}

/// Pages content; returns when closed.
#[must_use = "a component does nothing until it is run or mounted"]
pub struct Pager {
    keymap: Keymap,
    search_keymap: Keymap,
    source: Option<Arc<dyn Renderable + Send + Sync>>,
    rendered_at: Option<usize>,
    viewport: Viewport,
    /// Each line's plain text, for searching.
    plain: Vec<String>,
    /// The search being typed, while `/` is active.
    typing: Option<String>,
    query: Option<String>,
    hits: Vec<Hit>,
    current: usize,
    theme: Theme,
    mouse: bool,
    /// Links clicked (#476), in order.
    clicked: Vec<String>,
    /// Which first lines stay on top (#716).
    sticky: Sticky,
}

/// Which of a pager's first lines stay on top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Sticky {
    Lines(usize),
    /// A table's header, found in the content at each width.
    Table,
}

impl Pager {
    /// Page `renderable`, rendered at the terminal's width.
    pub fn new(renderable: impl Renderable + Send + Sync + 'static) -> Pager {
        let mut pager = Pager::lines(Vec::new());
        pager.source = Some(Arc::new(renderable));
        pager
    }

    /// Page lines already rendered.
    pub fn lines(lines: Vec<Vec<Segment>>) -> Pager {
        let mut pager = Pager {
            keymap: pager_keymap(),
            search_keymap: pager_search_keymap(),
            source: None,
            rendered_at: None,
            viewport: Viewport::default(),
            plain: Vec::new(),
            typing: None,
            query: None,
            hits: Vec::new(),
            current: 0,
            theme: Theme::default(),
            mouse: false,
            clicked: Vec::new(),
            sticky: Sticky::Lines(0),
        };
        pager.set(lines);
        pager
    }

    /// Make `keys` do `action` (see [`pager_keymap`] and
    /// [`pager_search_keymap`]) on this pager only; no keys unbinds it.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        let keys: Vec<Key> = keys.into_iter().collect();
        self.keymap.rebind(action, keys.clone());
        self.search_keymap.rebind(action, keys);
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Report the mouse (#476): the wheel scrolls, and a click on a link
    /// (an OSC 8 hyperlink in the content) records it: see
    /// [`Pager::links`]. The pager never opens a link itself.
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.mouse = on;
        self
    }

    /// The links clicked while paging, in order: for the caller to open,
    /// print or ignore.
    pub fn links(&self) -> &[String] {
        &self.clicked
    }

    /// Keep the first `lines` lines on top while the rest scroll (#716):
    /// a header rendered with the content.
    pub fn sticky_header(mut self, lines: usize) -> Self {
        self.sticky = Sticky::Lines(lines);
        self.stick();
        self
    }

    /// Keep a table's header on top while its rows scroll (#716), when the
    /// content starts with a table: its title, top edge and header rows,
    /// down to the line under the header, as [`table_header_rows`] finds
    /// them at each width. Content that does not start with a table
    /// scrolls as it is.
    pub fn sticky_table_header(mut self, on: bool) -> Self {
        self.sticky = if on { Sticky::Table } else { Sticky::Lines(0) };
        self.stick();
        self
    }

    /// The lines that stay on top now.
    pub fn sticky_rows(&self) -> usize {
        self.viewport.sticky_rows()
    }

    /// Work out the sticky lines for the content.
    fn stick(&mut self) {
        let rows = match self.sticky {
            Sticky::Lines(lines) => lines,
            Sticky::Table => table_header_rows(self.viewport.lines()).unwrap_or(0),
        };
        self.viewport.set_sticky(rows);
    }

    /// Start with this search.
    pub fn search(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self.find();
        self
    }

    fn set(&mut self, mut lines: Vec<Vec<Segment>>) {
        // Controls in the content are shown as text; search what is shown.
        for line in &mut lines {
            crate::paint::sanitize_line(line);
        }
        self.plain = lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect())
            .collect();
        self.viewport.set_lines(lines);
        self.stick();
        self.find();
    }

    /// Render the source at `width` if it has not been yet at that width.
    fn render_at(&mut self, context: &Context<'_>) {
        let width = context.width;
        if let Some(source) = &self.source {
            if self.rendered_at != Some(width) {
                let lines = context.lines_at(&**source, width);
                self.rendered_at = Some(width);
                self.set(lines);
            }
        }
    }

    /// Find every case-insensitive occurrence of the query.
    fn find(&mut self) {
        self.hits.clear();
        self.current = 0;
        let Some(query) = self.query.as_ref().filter(|q| !q.is_empty()) else {
            return;
        };
        let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
        for (line, text) in self.plain.iter().enumerate() {
            for (start, end) in find_folded(text, &needle) {
                self.hits.push(Hit {
                    line,
                    start: cell_len(&text[..start]),
                    end: cell_len(&text[..end]),
                });
            }
        }
    }

    fn page(context: &Context<'_>) -> usize {
        context.height.saturating_sub(1).max(1)
    }

    fn jump(&mut self, delta: isize, page: usize) {
        if self.hits.is_empty() {
            return;
        }
        let count = self.hits.len() as isize;
        self.current = (self.current as isize + delta).rem_euclid(count) as usize;
        self.viewport.show(self.hits[self.current].line, page);
    }

    /// `line` with the cells of `hits` restyled.
    fn mark(&self, line: &[Segment], index: usize) -> Vec<Segment> {
        let hits: Vec<(usize, &Hit)> = self
            .hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| hit.line == index)
            .collect();
        if hits.is_empty() {
            return line.to_vec();
        }
        let other = Style::parse("reverse").expect("style");
        let current = Style::parse("black on yellow").expect("style");
        let mut out = Vec::new();
        let mut column = 0;
        for segment in line {
            for c in segment.text.chars() {
                let width = cell_len(c.encode_utf8(&mut [0; 4]));
                let marked = hits
                    .iter()
                    .find(|(_, hit)| column >= hit.start && column < hit.end)
                    .map(|(n, _)| if *n == self.current { &current } else { &other });
                let style = match (marked, &segment.style) {
                    (Some(mark), Some(style)) => Some(style.combine(mark)),
                    (Some(mark), None) => Some(mark.clone()),
                    (None, style) => style.clone(),
                };
                match out.last_mut() {
                    Some(Segment {
                        text, style: last, ..
                    }) if *last == style => text.push(c),
                    _ => out.push(Segment::new(c.to_string(), style)),
                }
                column += width;
            }
        }
        out
    }
}

/// The lines of a table's top, when `lines` start with one: its title,
/// its top edge and its header rows, down to and including the line under
/// the header (#716). `None` when the content does not start with a table
/// whose header a line of its box marks off, such as rich's default
/// `HEAVY_HEAD` table, or when nothing would be left under it, or when the
/// box draws the lines between rows as it draws that one and the line comes
/// again (a table without a header, or with `show_lines`, in `SQUARE`):
/// pinning a row would be worse than pinning nothing. The text of each line
/// is what is looked at, so it works on output rendered elsewhere
/// (`rich data.csv --force-terminal | rich pager`).
pub fn table_header_rows(lines: &[Vec<Segment>]) -> Option<usize> {
    // A header taller than this is not looked for.
    const LIMIT: usize = 16;
    let plain: Vec<String> = lines
        .iter()
        .take(LIMIT)
        .map(|line| {
            let text: String = line
                .iter()
                .filter(|s| !s.control)
                .map(|s| s.text.as_str())
                .collect();
            text.trim_matches(' ').to_string()
        })
        .collect();
    for under in 1..plain.len() {
        for set in BOXES.iter() {
            if let Some(rows) = header_above(&plain, under, set) {
                return (rows < lines.len()).then_some(rows);
            }
        }
    }
    None
}

/// The boxes whose header rows end with a line of their own: rich's, but
/// those that mark the header off with a plain rule, which a rule in the
/// content would look like.
const BOXES: [rich::r#box::Box; 15] = {
    use rich::r#box::*;
    [
        HEAVY_HEAD,
        SQUARE,
        ROUNDED,
        HEAVY,
        DOUBLE,
        MINIMAL,
        ASCII,
        ASCII2,
        ASCII_DOUBLE_HEAD,
        SQUARE_DOUBLE_HEAD,
        MINIMAL_HEAVY_HEAD,
        MINIMAL_DOUBLE_HEAD,
        HEAVY_EDGE,
        DOUBLE_EDGE,
        MARKDOWN,
    ]
};

/// The lines down to `under` when `plain[under]` is the line under a
/// header drawn with `set`, the header rows are above it, and above those
/// at most a top edge and a title of three lines.
fn header_above(plain: &[String], under: usize, set: &rich::r#box::Box) -> Option<usize> {
    let line = &plain[under];
    let (left, across, cross, right) = (
        set.head_row_left,
        set.head_row_horizontal,
        set.head_row_cross,
        set.head_row_right,
    );
    let ends = |text: &str, first: char, last: char| {
        text.starts_with(first) && text.ends_with(last) && text.chars().count() > 1
    };
    let drawn = !line.is_empty()
        && line.contains(across)
        && line
            .chars()
            .all(|c| [left, across, cross, right].contains(&c) && c != ' ');
    // Its edges, or a crossing, tell it from a rule.
    let edged = left != ' ' && right != ' ' && ends(line, left, right);
    if !drawn || !(edged || (cross != across && line.contains(cross))) {
        return None;
    }
    // A box that draws the line between rows (`show_lines`) as it draws
    // the one under the header (SQUARE, and so a headerless HEAVY_HEAD
    // table): the line comes again in a table without a header, so it
    // marks one only where it is the only such line. Better not to pin a
    // header than to pin a row.
    let between = (
        set.row_left,
        set.row_horizontal,
        set.row_cross,
        set.row_right,
    );
    if between == (left, across, cross, right)
        && plain
            .iter()
            .enumerate()
            .any(|(at, text)| at != under && text == line)
    {
        return None;
    }
    // With its edges, or (`show_edge=False`) its dividers.
    let header = |text: &str| {
        (set.head_left != ' ' && set.head_right != ' ' && ends(text, set.head_left, set.head_right))
            || (set.head_vertical != ' ' && text.contains(set.head_vertical))
    };
    let mut top = under;
    while top > 0 && header(&plain[top - 1]) {
        top -= 1;
    }
    if top == under {
        return None;
    }
    let edge = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| [set.top_left, set.top, set.top_divider, set.top_right, ' '].contains(&c))
    };
    if top > 0 && edge(&plain[top - 1]) {
        top -= 1;
    }
    (top <= 3).then_some(under + 1)
}

/// The byte ranges of `text` whose lower-cased characters are `needle`
/// (already lower-cased), without overlaps. Lower-casing can change a
/// character's length (`ẞ` to `ß`, `İ` to `i̇`), so the text is folded one
/// character at a time and each match mapped back to whole characters of
/// the original: nothing is sliced with an offset into the folded text.
fn find_folded(text: &str, needle: &[char]) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    if needle.is_empty() {
        return hits;
    }
    // Each folded character, and the byte range of the character it came
    // from.
    let mut folded: Vec<(char, usize, usize)> = Vec::with_capacity(text.len());
    for (at, c) in text.char_indices() {
        let end = at + c.len_utf8();
        folded.extend(c.to_lowercase().map(|lower| (lower, at, end)));
    }
    let mut from = 0;
    while from + needle.len() <= folded.len() {
        let window = &folded[from..from + needle.len()];
        if window.iter().map(|(c, _, _)| c).eq(needle.iter()) {
            let (start, end) = (window[0].1, window[needle.len() - 1].2);
            hits.push((start, end));
            // Past the last original character the match touched.
            from += needle.len();
            while from < folded.len() && folded[from].1 < end {
                from += 1;
            }
        } else {
            from += 1;
        }
    }
    hits
}

impl Component for Pager {
    type Output = ();

    /// Render the content at the terminal's width and, for a search given
    /// up front, show its first match.
    fn start(&mut self, context: &Context<'_>) -> Flow<()> {
        self.render_at(context);
        self.jump(0, Self::page(context));
        Flow::Continue
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<()> {
        self.render_at(context);
        let page = Self::page(context);
        if let Event::Link(url) = event {
            self.clicked.push(url.clone());
            return Flow::Continue;
        }
        if self.typing.is_some() {
            let Some(key) = event.key() else {
                return Flow::Continue;
            };
            match self
                .search_keymap
                .action(key)
                .map(str::to_string)
                .as_deref()
            {
                Some("find") => {
                    self.query = self.typing.take();
                    self.find();
                    // The first match at or after the top of the page.
                    let top = match self.viewport.offset() {
                        0 => 0,
                        _ => self.viewport.top(page),
                    };
                    let first = self
                        .hits
                        .iter()
                        .position(|hit| hit.line >= top)
                        .unwrap_or(0);
                    self.current = first;
                    self.jump(0, page);
                }
                Some("cancel") => self.typing = None,
                Some("delete") => {
                    if self.typing.as_mut().and_then(String::pop).is_none() {
                        self.typing = None;
                    }
                }
                _ => {
                    if let (KeyCode::Char(c), Some(typing)) = (key.code, &mut self.typing) {
                        typing.push(c);
                    }
                }
            }
            return Flow::Continue;
        }
        let Some(key) = event.key() else {
            // The wheel.
            self.viewport.handle_scroll(event, page);
            return Flow::Continue;
        };
        match self.keymap.action(key).map(str::to_string).as_deref() {
            Some("quit") => Flow::Done(()),
            Some("search") => {
                self.typing = Some(String::new());
                Flow::Continue
            }
            Some("next-match") => {
                self.jump(1, page);
                Flow::Continue
            }
            Some("previous-match") => {
                self.jump(-1, page);
                Flow::Continue
            }
            Some(action) if self.viewport.act(action, page) => Flow::Continue,
            _ => Flow::Ignored,
        }
    }

    fn keymap(&self) -> Keymap {
        if self.typing.is_some() {
            return self.search_keymap.clone();
        }
        self.keymap.clone()
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let page = Self::page(context);
        let theme = &self.theme;
        // Before the first event a renderable source is not rendered yet.
        let fresh;
        let (rows, plain_lines): (Vec<(usize, &[Segment])>, usize) =
            match (&self.source, self.rendered_at) {
                (Some(source), None) => {
                    fresh = context.lines_at(&**source, width);
                    let rows = fresh.iter().map(Vec::as_slice).enumerate();
                    (rows.take(page).collect(), fresh.len())
                }
                // The sticky lines, then those scrolled to.
                _ => (self.viewport.rows(page), self.viewport.len()),
            };
        let mut out: Vec<Vec<Segment>> = rows
            .into_iter()
            .map(|(index, line)| fit(self.mark(line, index), width))
            .collect();
        out.resize_with(page.min(plain_lines.max(1)), Vec::new);
        let status = if let Some(typing) = &self.typing {
            let line = vec![text("/", &theme.prompt), plain(typing.clone())];
            let column = 1 + cell_len(typing);
            out.push(fit(line, width));
            return View::new(out).with_cursor(page.min(plain_lines.max(1)), column);
        } else {
            let viewport_status = if self.viewport.is_empty() && self.source.is_some() {
                format!("all {plain_lines} lines")
            } else {
                self.viewport.status(page)
            };
            let search = match (&self.query, self.hits.len()) {
                (Some(query), 0) => format!(" · no match for {query:?}"),
                (Some(_), count) => format!(" · match {}/{count} · n/N", self.current + 1),
                (None, _) => String::new(),
            };
            let link = match self.clicked.last() {
                Some(url) => format!(" · link {url}"),
                None => String::new(),
            };
            format!("{viewport_status}{search}{link} · / search · q quit")
        };
        out.push(fit(vec![text(status, &theme.hint)], width));
        View::new(out)
    }

    fn mouse(&self) -> bool {
        self.mouse
    }

    fn default_value(&self) -> Option<()> {
        Some(())
    }

    /// Without a terminal, there is nothing to page: write it all.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<()>, NotInteractive> {
        if let (Some(source), None) = (&self.source, self.rendered_at) {
            let console = rich::Console::builder().force_terminal(false).build();
            let options = console.options();
            let lines = console.render_lines(&**source, &options, false);
            self.set(lines);
        }
        for line in &self.plain {
            io.write(line);
            io.write("\n");
        }
        Ok(Some(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_folds_case_one_character_at_a_time() {
        // `ẞ` lower-cases to `ß`, two bytes shorter: offsets into the
        // folded text are not offsets into the line.
        assert_eq!(find_folded("Ⱥẞ", &['ß']), [(2, 5)]);
        assert_eq!(find_folded("ȺẞxSS", &['ß']), [(2, 5)]);
        // `İ` folds to two characters; lines holding it are searched too.
        let needle: Vec<char> = "x".chars().collect();
        assert_eq!(find_folded("İx", &needle), [(2, 3)]);
        let needle: Vec<char> = "i̇".chars().collect();
        assert_eq!(find_folded("aİb", &needle), [(1, 3)]);
        // Non-overlapping, case-insensitive.
        assert_eq!(find_folded("AaAa", &['a', 'a']), [(0, 2), (2, 4)]);
        assert!(find_folded("abc", &[]).is_empty());
    }

    #[test]
    fn a_search_that_changes_byte_lengths_marks_the_right_cells() {
        let pager = Pager::lines(vec![vec![Segment::new("Ⱥẞ end", None)]]).search("ß");
        assert_eq!(
            pager.hits,
            [Hit {
                line: 0,
                start: 1,
                end: 2
            }]
        );
    }
}

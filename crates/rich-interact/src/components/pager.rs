//! A pager over rendered output, with search (#291).
//!
//! The content is any renderable, rendered at the terminal's width (and
//! again after a resize), or lines already rendered. It scrolls like the
//! [`Viewport`] it is built on. `/` starts a search: matches are marked in
//! place, `n` and `N` jump between them, and the status line counts them.
//! `q` or Escape closes it.

use std::sync::Arc;

use rich::cells::cell_len;
use rich::{Renderable, Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::components::{fit, plain, text, Theme};
use crate::event::{Event, KeyCode};
use crate::policy::{LineIo, NotInteractive};
use crate::viewport::Viewport;

/// One match: a line, and its cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit {
    line: usize,
    start: usize,
    end: usize,
}

/// Pages content; returns when closed.
pub struct Pager {
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
            source: None,
            rendered_at: None,
            viewport: Viewport::default(),
            plain: Vec::new(),
            typing: None,
            query: None,
            hits: Vec::new(),
            current: 0,
            theme: Theme::default(),
        };
        pager.set(lines);
        pager
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
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
        if let Some(typing) = &mut self.typing {
            match event.key().map(|key| key.code) {
                Some(KeyCode::Enter) => {
                    self.query = self.typing.take();
                    self.find();
                    // The first match at or after the top of the page.
                    let top = self.viewport.offset();
                    let first = self
                        .hits
                        .iter()
                        .position(|hit| hit.line >= top)
                        .unwrap_or(0);
                    self.current = first;
                    self.jump(0, page);
                }
                Some(KeyCode::Escape) => self.typing = None,
                Some(KeyCode::Backspace) => {
                    if typing.pop().is_none() {
                        self.typing = None;
                    }
                }
                Some(KeyCode::Char(c)) => typing.push(c),
                _ => {}
            }
            return Flow::Continue;
        }
        if self.viewport.handle_scroll(event, page) {
            return Flow::Continue;
        }
        match event.key().map(|key| key.code) {
            Some(KeyCode::Char('q') | KeyCode::Escape) => Flow::Done(()),
            Some(KeyCode::Char('/')) => {
                self.typing = Some(String::new());
                Flow::Continue
            }
            Some(KeyCode::Char('n')) => {
                self.jump(1, page);
                Flow::Continue
            }
            Some(KeyCode::Char('N')) => {
                self.jump(-1, page);
                Flow::Continue
            }
            _ => Flow::Continue,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let width = context.width;
        let page = Self::page(context);
        let theme = &self.theme;
        // Before the first event a renderable source is not rendered yet.
        let fresh;
        let (lines, offset, plain_lines): (&[Vec<Segment>], usize, usize) =
            match (&self.source, self.rendered_at) {
                (Some(source), None) => {
                    fresh = context.lines_at(&**source, width);
                    (&fresh, 0, fresh.len())
                }
                _ => (
                    self.viewport.lines(),
                    self.viewport.offset(),
                    self.viewport.len(),
                ),
            };
        let mut out: Vec<Vec<Segment>> = lines
            .iter()
            .enumerate()
            .skip(offset)
            .take(page)
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
            format!("{viewport_status}{search} · / search · q quit")
        };
        out.push(fit(vec![text(status, &theme.hint)], width));
        View::new(out)
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

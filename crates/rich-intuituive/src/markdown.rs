//! Markdown that streams in: [`markdown`], a node over a signal holding
//! the document, and [`MarkdownStream`], a handle any thread appends to.
//!
//! The document is kept as blocks, each a node of its own: the runs of
//! lines between blank lines, outside fenced code (a line that starts with
//! whitespace after a blank line stays with the block above, so a list
//! item's second paragraph is not cut off from its list). Each block is
//! rs-rich's [`Markdown`](rich::markdown::Markdown) port, rendered on its
//! own at the node's width and kept until its source or the width changes.
//! When the document grows, it is split again only from the last block,
//! which is the only one whose source can have changed: the blocks above
//! it neither render nor draw again. A fence still open draws as code, as
//! CommonMark reads it, so the text does not jump when it closes.
//!
//! Reference-style links whose definition is in another block are not
//! resolved, and a list whose items are separated by blank lines draws as
//! one list an item.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use rich::{Console, Segment, Style};
use rich_interact::{Button as MouseButton, KeyCode, MouseKind};

use crate::app::Ctx;
use crate::node::{Axis, Node};
use crate::reactive::{Proxy, Runtime, Signal, signal};
use crate::screen::Rect;
use crate::widget::{Canvas, DrawCx, EventCx, MeasureCx, Used, Widget, WidgetEvent};

type OnLink = Box<dyn FnMut(&str, &mut Ctx)>;

/// A block's rendered lines.
type Lines = Rc<Vec<Vec<Segment>>>;

/// What every block of a [`markdown`] node shares: its options.
#[derive(Default)]
struct Shared {
    code_theme: RefCell<Option<String>>,
    on_link: RefCell<Option<OnLink>>,
}

/// Markdown `doc` holds (CommonMark, as rs-rich's
/// [`Markdown`](rich::markdown::Markdown) reads it), drawn at the node's
/// width and as tall as it needs: headings, paragraphs, lists, quotes,
/// tables and fenced code, highlighted. Kept as blocks, so a document that
/// grows at its end (an answer streaming in, through a
/// [`MarkdownStream`]) renders and draws again only its last block. A
/// block with links takes the focus: Tab and Shift+Tab go through its
/// links (then on), the focused link is drawn reversed, and Enter or a
/// click on one calls [`on_link`](Node::on_link) with its URL.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::markdown;
///
/// let app = App::new(|| {
///     let doc = signal("# Plan\n\nFirst **this**, then".to_string());
///     markdown(doc).on_key("a", move |_| doc.update(|d| d.push_str(" that.")))
/// });
/// let mut driver = app.driver(30, 6);
/// driver.press("a");
/// let screen = driver.screen().plain().join("\n");
/// assert!(screen.contains("Plan"));
/// assert!(screen.contains("First this, then that."));
/// ```
pub fn markdown(doc: Signal<String>) -> Node {
    Node::from_widget(
        Box::new(MarkdownView {
            doc,
            shared: Rc::new(Shared::default()),
            seen: String::new(),
            blocks: Vec::new(),
            sources: Vec::new(),
            children: Vec::new(),
        }),
        "markdown",
    )
    .auto()
}

/// A [`markdown`] node's options. On any other node they do nothing.
impl Node {
    /// On a [`markdown`] node: call `on_link` with a link's URL when Enter
    /// or a click picks it.
    pub fn on_link(self, on_link: impl FnMut(&str, &mut Ctx) + 'static) -> Node {
        self.widget_mut(|view: &mut MarkdownView| {
            *view.shared.on_link.borrow_mut() = Some(Box::new(on_link));
        });
        self
    }

    /// On a [`markdown`] node: highlight fenced code with `theme` (the
    /// console's code highlighter's names: syntect's, and `ansi_dark` and
    /// `ansi_light`).
    pub fn code_theme(self, theme: &str) -> Node {
        self.widget_mut(|view: &mut MarkdownView| {
            *view.shared.code_theme.borrow_mut() = Some(theme.to_string());
        });
        self
    }
}

/// The widget behind [`markdown`].
struct MarkdownView {
    doc: Signal<String>,
    shared: Rc<Shared>,
    /// The document as last split, and its blocks' byte ranges.
    seen: String,
    blocks: Vec<Range<usize>>,
    /// Each block's source, read by its node, and the nodes.
    sources: Vec<Signal<String>>,
    children: Vec<Node>,
}

/// Whether a line opens or closes a fence: its character and length.
fn fence(line: &str) -> Option<(char, usize)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let c = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let n = rest.chars().take_while(|x| *x == c).count();
    (n >= 3).then_some((c, n))
}

/// The blocks of `text` from byte `from` (a line's start): the runs of
/// lines between blank lines, outside fences, each without its last line
/// break.
pub(crate) fn split_blocks(text: &str, from: usize) -> Vec<Range<usize>> {
    let mut blocks: Vec<Range<usize>> = Vec::new();
    let mut open: Option<(char, usize)> = None;
    let mut current: Option<Range<usize>> = None;
    let mut blank = false;
    let mut at = from;
    for line in text[from..].split_inclusive('\n') {
        let start = at;
        at += line.len();
        let content = line.trim_end_matches(['\n', '\r']);
        let end = start + content.len();
        if let Some((c, n)) = open {
            if fence(content).is_some_and(|(fc, fnn)| fc == c && fnn >= n)
                && content.trim_start().trim_start_matches(c).trim().is_empty()
            {
                open = None;
            }
            if let Some(block) = &mut current {
                block.end = end;
            }
            continue;
        }
        if content.trim().is_empty() {
            blank = true;
            continue;
        }
        // After a blank line, a line that starts with whitespace stays with
        // the block above (a list item's next paragraph); any other starts
        // a block.
        let continues = !blank || content.starts_with([' ', '\t']);
        match &mut current {
            Some(block) if continues => block.end = end,
            _ => {
                if let Some(done) = current.take() {
                    blocks.push(done);
                }
                current = Some(start..end);
            }
        }
        blank = false;
        open = fence(content);
    }
    blocks.extend(current);
    blocks
}

impl MarkdownView {
    /// Bring the blocks in line with the document: split it again from
    /// the last block when it only grew there, else whole; set the sources
    /// that changed, and make nodes for new blocks.
    fn reconcile(&mut self) {
        let doc = self.doc;
        doc.with(|text| {
            if *text == self.seen {
                return;
            }
            let last = self.blocks.last().map_or(0, |b| b.start);
            let grew = text.len() >= self.seen.len()
                && text.as_bytes()[..last] == self.seen.as_bytes()[..last];
            let keep = if grew {
                self.blocks.len().saturating_sub(1)
            } else {
                0
            };
            let from = self
                .blocks
                .get(keep)
                .map_or(if grew { last } else { 0 }, |b| b.start);
            self.blocks.truncate(keep);
            self.blocks.extend(split_blocks(text, from));
            self.seen.clear();
            self.seen.push_str(text);
        });
        let runtime = Runtime::current();
        let text = &self.seen;
        for (i, range) in self.blocks.iter().enumerate() {
            let source = &text[range.clone()];
            match self.sources.get(i) {
                Some(signal) => {
                    if signal.with_untracked(|s| s != source) {
                        // A block that gains or loses its links takes the
                        // focus or stops: made again.
                        let links = has_links(source);
                        if links != self.children[i].focusable {
                            let fresh = signal_of(source);
                            let node = runtime.untracked(|| block(fresh, self.shared.clone()));
                            std::mem::replace(&mut self.children[i], node).forget(&runtime);
                            self.sources[i] = fresh;
                        } else {
                            let source = source.to_string();
                            runtime.untracked(|| signal.set(source));
                        }
                    }
                }
                None => {
                    let fresh = signal_of(source);
                    self.sources.push(fresh);
                    let node = runtime.untracked(|| block(fresh, self.shared.clone()));
                    self.children.push(node);
                }
            }
        }
        for node in self.children.drain(self.blocks.len()..) {
            node.forget(&runtime);
        }
        self.sources.truncate(self.blocks.len());
    }
}

fn signal_of(source: &str) -> Signal<String> {
    Runtime::current().untracked(|| signal(source.to_string()))
}

/// Whether a block's source may hold a link: it then takes the focus.
fn has_links(source: &str) -> bool {
    source.contains("](") || source.contains("<http") || source.contains("]: ")
}

fn block(source: Signal<String>, shared: Rc<Shared>) -> Node {
    let links = has_links(&source.get_untracked());
    let node = Node::from_widget(
        Box::new(Block {
            source,
            shared,
            cache: None,
            links: Vec::new(),
            selected: 0,
        }),
        "markdown block",
    )
    .auto();
    if links { node.focusable() } else { node }
}

impl Widget for MarkdownView {
    fn name(&self) -> &'static str {
        "markdown"
    }

    fn describe(&self) -> Option<String> {
        Some(format!("{} blocks", self.children.len()))
    }

    fn children(&self) -> &[Node] {
        &self.children
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, height: u16) -> u16 {
        self.reconcile();
        let sizes = self
            .children
            .iter()
            .map(|child| cx.measure(child, axis, width, height));
        match axis {
            Axis::Vertical => {
                let rows: u32 = sizes.map(u32::from).sum();
                let gaps = self.children.len().saturating_sub(1) as u32;
                (rows + gaps).min(u16::MAX as u32) as u16
            }
            Axis::Horizontal => sizes.max().unwrap_or(0),
        }
    }

    fn layout(&mut self, cx: &MeasureCx, rect: Rect) -> Vec<Rect> {
        self.reconcile();
        let mut y = rect.y as u32;
        let bottom = rect.bottom() as u32;
        self.children
            .iter()
            .map(|child| {
                if y >= bottom {
                    return Rect::default();
                }
                let rows = cx.measure(child, Axis::Vertical, rect.width, 0) as u32;
                let rows = rows.min(bottom - y);
                let area = Rect::new(rect.x, y as u16, rect.width, rows as u16);
                y += rows + 1;
                area
            })
            .collect()
    }

    fn draw(&mut self, _cx: &mut DrawCx, _canvas: &mut Canvas) {}

    fn retained(&self) -> bool {
        true
    }
}

/// A link in a block's rendered lines: its row, columns and URL.
#[derive(Clone, Debug, PartialEq)]
struct Link {
    row: u16,
    columns: Range<u16>,
    url: String,
}

/// One block of a [`markdown`] node.
struct Block {
    source: Signal<String>,
    shared: Rc<Shared>,
    /// The width and source last rendered, and the lines.
    cache: Option<(u16, String, Lines)>,
    links: Vec<Link>,
    /// The focused link.
    selected: usize,
}

impl Block {
    /// The block rendered at `width`, rendering it again only when its
    /// source or the width changed.
    fn lines(&mut self, console: &Console, width: u16) -> Lines {
        let fresh = self.source.with(|source| {
            self.cache
                .as_ref()
                .is_some_and(|(w, s, _)| *w == width && s == source)
        });
        if !fresh {
            let source = self.source.get_untracked();
            let lines = render(
                console,
                &source,
                width,
                self.shared.code_theme.borrow().as_deref(),
            );
            self.links = links_in(&lines);
            self.selected = self.selected.min(self.links.len().saturating_sub(1));
            self.cache = Some((width, source, Rc::new(lines)));
        }
        self.cache.as_ref().expect("just rendered").2.clone()
    }

    fn pick(&mut self, cx: &mut EventCx, index: usize) -> Used {
        let Some(link) = self.links.get(index) else {
            return Used::No;
        };
        let url = link.url.clone();
        let mut on_link = self.shared.on_link.borrow_mut().take();
        if let Some(call) = on_link.as_mut() {
            call(&url, cx.app());
        }
        let mut slot = self.shared.on_link.borrow_mut();
        if slot.is_none() {
            *slot = on_link;
        }
        Used::Yes
    }
}

/// `source` rendered by rs-rich's Markdown at `width`, without its blank
/// last lines.
fn render(console: &Console, source: &str, width: u16, theme: Option<&str>) -> Vec<Vec<Segment>> {
    let mut md = rich::markdown::Markdown::new(source);
    if let Some(theme) = theme {
        md = md.code_theme(theme);
    }
    let options = console.options().update_width(width.max(1) as usize);
    let mut lines = console.render_lines(&md, &options, false);
    let blank = |line: &Vec<Segment>| {
        line.iter().all(|s| {
            s.text.trim().is_empty() && s.style.as_ref().is_none_or(|st| st.bgcolor().is_none())
        })
    };
    while lines.last().is_some_and(blank) {
        lines.pop();
    }
    lines
}

/// The links in rendered `lines`: runs of cells styled with one URL.
fn links_in(lines: &[Vec<Segment>]) -> Vec<Link> {
    let mut links: Vec<Link> = Vec::new();
    for (row, line) in lines.iter().enumerate() {
        let mut x = 0u16;
        for segment in line {
            let cells = rich::cells::cell_len(&segment.text).min(u16::MAX as usize) as u16;
            if let Some(url) = segment.style.as_ref().and_then(Style::link) {
                match links.last_mut() {
                    Some(last)
                        if last.row == row as u16 && last.columns.end == x && last.url == url =>
                    {
                        last.columns.end = x + cells;
                    }
                    _ => links.push(Link {
                        row: row as u16,
                        columns: x..x + cells,
                        url: url.to_string(),
                    }),
                }
            }
            x = x.saturating_add(cells);
        }
    }
    links
}

impl Widget for Block {
    fn name(&self) -> &'static str {
        "markdown block"
    }

    fn access_text(&self) -> Option<String> {
        let lines = self.cache.as_ref().map(|(_, _, lines)| lines.clone())?;
        let text: Vec<String> = lines
            .iter()
            .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
            .map(|line| line.trim_end().to_string())
            .filter(|line| !line.is_empty())
            .collect();
        Some(text.join(" "))
    }

    fn measure(&mut self, cx: &MeasureCx, axis: Axis, width: u16, _height: u16) -> u16 {
        let lines = self.lines(cx.console, width.max(1));
        match axis {
            Axis::Vertical => lines.len().min(u16::MAX as usize) as u16,
            Axis::Horizontal => lines
                .iter()
                .map(|line| Segment::get_line_length(line))
                .max()
                .unwrap_or(0)
                .min(u16::MAX as usize) as u16,
        }
    }

    fn draw(&mut self, cx: &mut DrawCx, canvas: &mut Canvas) {
        let lines = self.lines(cx.console(), canvas.width());
        canvas.lines(&lines);
        if cx.focused()
            && let Some(link) = self.links.get(self.selected)
        {
            let style = cx.style("markdown.link.focus", "reverse");
            let width = link.columns.end - link.columns.start;
            canvas.restyle(link.columns.start, link.row, width, 1, &style);
        }
    }

    fn event(&mut self, cx: &mut EventCx, event: &WidgetEvent) -> Used {
        match event {
            WidgetEvent::Focus(true) => {
                self.selected = 0;
                cx.redraw();
                Used::No
            }
            WidgetEvent::Key(key) if !key.modifiers.ctrl && !key.modifiers.alt => match key.code {
                KeyCode::Enter => self.pick(cx, self.selected),
                KeyCode::Tab if self.selected + 1 < self.links.len() => {
                    self.selected += 1;
                    cx.redraw();
                    Used::Yes
                }
                KeyCode::BackTab if self.selected > 0 => {
                    self.selected -= 1;
                    cx.redraw();
                    Used::Yes
                }
                _ => Used::No,
            },
            WidgetEvent::Mouse(mouse) if mouse.kind == MouseKind::Down(MouseButton::Left) => {
                let hit = self
                    .links
                    .iter()
                    .position(|link| link.row == mouse.row && link.columns.contains(&mouse.column));
                match hit {
                    Some(index) => {
                        self.selected = index;
                        cx.redraw();
                        self.pick(cx, index)
                    }
                    None => Used::No,
                }
            }
            _ => Used::No,
        }
    }

    fn focusable(&self) -> bool {
        true
    }
}

/// A handle that appends to a [`markdown`] node's document from anywhere:
/// a task, a thread reading an agent's reply, a [`Proxy`]. `Clone` and
/// `Send`. Fragments that arrive faster than the app turns its loop are
/// joined, and reach the document together, once a turn: the node then
/// splits again only its last block, and renders and draws only the
/// blocks that changed.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{markdown, MarkdownStream};
///
/// let app = App::new(|| {
///     let doc = signal(String::new());
///     let stream = MarkdownStream::new(doc);
///     // Another thread: an agent's reply arriving.
///     std::thread::spawn(move || {
///         for word in ["Here ", "is ", "**bold** ", "text."] {
///             stream.append(word);
///         }
///         stream.finish();
///     })
///     .join()
///     .unwrap();
///     markdown(doc)
/// });
/// let mut driver = app.driver(30, 2);
/// driver.settle();
/// assert_eq!(driver.screen().plain()[0].trim_end(), "Here is bold text.");
/// ```
#[derive(Clone)]
pub struct MarkdownStream {
    doc: Signal<String>,
    proxy: Proxy,
    /// Text appended and not yet delivered, and whether a delivery is on
    /// its way.
    pending: Arc<Mutex<(String, bool)>>,
}

impl MarkdownStream {
    /// A stream into `doc`, made on the app's thread.
    pub fn new(doc: Signal<String>) -> MarkdownStream {
        MarkdownStream {
            doc,
            proxy: Runtime::current().proxy(),
            pending: Arc::new(Mutex::new((String::new(), false))),
        }
    }

    /// Append `fragment` to the document, on the app's next turn. Whether
    /// the app is still running.
    pub fn append(&self, fragment: &str) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        pending.0.push_str(fragment);
        if pending.1 {
            return true;
        }
        pending.1 = true;
        drop(pending);
        let (doc, pending) = (self.doc, self.pending.clone());
        self.proxy.run(move || {
            let text = {
                let mut pending = pending.lock().unwrap_or_else(|e| e.into_inner());
                pending.1 = false;
                std::mem::take(&mut pending.0)
            };
            if !text.is_empty() {
                doc.update(|d| d.push_str(&text));
            }
        })
    }

    /// The stream is done: what is pending is delivered. Dropping every
    /// handle does as much; `finish` says so where it is read.
    pub fn finish(self) {
        drop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::split_blocks;

    fn blocks(text: &str) -> Vec<&str> {
        split_blocks(text, 0)
            .into_iter()
            .map(|r| &text[r])
            .collect()
    }

    #[test]
    fn blank_lines_split_blocks_outside_fences() {
        assert_eq!(
            blocks("# A\n\npara\nmore\n\n\n- x\n- y\n"),
            ["# A", "para\nmore", "- x\n- y"]
        );
        assert_eq!(
            blocks("```rust\nfn a() {}\n\nfn b() {}\n```\nafter\n\nnext"),
            ["```rust\nfn a() {}\n\nfn b() {}\n```\nafter", "next"]
        );
        // An open fence runs to the end.
        assert_eq!(
            blocks("text\n\n~~~\ncode\n\nmore"),
            ["text", "~~~\ncode\n\nmore"]
        );
        // An indented line after a blank line stays with the block above.
        assert_eq!(
            blocks("1. item\n\n   more\n\n2. next"),
            ["1. item\n\n   more", "2. next"]
        );
        assert_eq!(blocks(""), Vec::<&str>::new());
        assert_eq!(blocks("\n\n"), Vec::<&str>::new());
    }

    #[test]
    fn splitting_from_a_block_gives_the_same_blocks() {
        let text = "# A\n\npara\n\n```\nx\n\ny\n```\n\nlast";
        let whole = split_blocks(text, 0);
        for (i, block) in whole.iter().enumerate() {
            assert_eq!(split_blocks(text, block.start), whole[i..]);
        }
    }
}

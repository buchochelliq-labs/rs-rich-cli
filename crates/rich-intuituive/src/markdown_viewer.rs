//! A Markdown viewer (#805), as Textual's `MarkdownViewer`: a [`markdown`]
//! document in a view that scrolls, with a table of contents of its
//! headings (picking one scrolls it to the top), links between documents
//! followed through the app's own loader, `#anchors`, and back and
//! forward through what was read.
//!
//! How a document is loaded is the app's: [`markdown_viewer`] takes a
//! loader, a closure from a location to the text, run off the app's
//! thread. The `files` plugin's `files::markdown_loader(root)` reads files
//! under a directory.

use std::cell::RefCell;
use std::fmt::Display;
use std::rc::Rc;
use std::sync::Arc;

use crate::app::Ctx;
use crate::list::list;
use crate::markdown::{markdown, split_blocks};
use crate::node::{Node, column, row, scroll_with, text};
use crate::reactive::{Signal, signal, untracked};

type Load = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;
type OnLink = Rc<RefCell<Option<Box<dyn FnMut(&str, &mut Ctx)>>>>;

/// A heading of the document shown: its level (1 for `#`), its text, its
/// anchor (`#install`) and the block it starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkdownHeading {
    pub level: u8,
    pub text: String,
    pub anchor: String,
    block: usize,
}

/// A place in what was read: a document and the first row in view.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Place {
    path: String,
    offset: u16,
}

/// Where to scroll once the document is laid out.
#[derive(Clone, Debug)]
enum Pending {
    Anchor(String),
    Offset(u16),
}

struct State {
    load: Load,
    back: Vec<Place>,
    forward: Vec<Place>,
    pending: Option<Pending>,
    /// Bumped by each load, so a slower older one is dropped.
    generation: u64,
}

/// A Markdown viewer: build its node with [`view`](Self::view), and steer
/// it from anywhere on the app's thread with [`go`](Self::go),
/// [`back`](Self::back) and [`forward`](Self::forward). `Clone`: each
/// clone is the same viewer.
///
/// Keys, on the viewer: `t` shows or hides the table of contents,
/// Backspace or Alt+← goes back, Alt+→ forward; Tab goes through a
/// block's links and Enter (or a click) follows one. In the table of
/// contents, Enter scrolls to the heading selected, and the heading in
/// view is marked `▸`.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::markdown_viewer;
///
/// let app = App::new(|| {
///     let viewer = markdown_viewer(|path: &str| match path {
///         "a.md" => Ok("# A\n\nSee [b](b.md#usage).".to_string()),
///         "b.md" => Ok("# B\n\nIntro.\n\n## Usage\n\nRun it.".to_string()),
///         _ => Err("not found"),
///     })
///     .open("a.md");
///     viewer.view()
/// })
/// .wait_for_tasks(true);
/// let mut driver = app.driver(50, 8);
/// driver.settle();
/// assert!(driver.screen().plain().join("\n").contains("See b."));
/// ```
#[derive(Clone)]
pub struct MarkdownViewer {
    doc: Signal<String>,
    error: Signal<Option<String>>,
    location: Signal<Option<String>>,
    headings: Signal<Vec<MarkdownHeading>>,
    /// Each block's first row, as last laid out.
    rows: Signal<Vec<u16>>,
    offset: Signal<u16>,
    toc: Signal<bool>,
    toc_selected: Signal<usize>,
    state: Rc<RefCell<State>>,
    on_link: OnLink,
}

/// A [`MarkdownViewer`] whose documents `load` reads: given a location (a
/// path relative to the documents' root, as links name them, without its
/// `#anchor`), the text, or an error, which the viewer shows in place of
/// the document. It runs off the app's thread. Call it while the app is
/// built or running.
///
/// # Panics
/// Outside a running or building app.
pub fn markdown_viewer<E: Display>(
    load: impl Fn(&str) -> Result<String, E> + Send + Sync + 'static,
) -> MarkdownViewer {
    let load: Load = Arc::new(move |path| load(path).map_err(|e| e.to_string()));
    MarkdownViewer {
        doc: signal(String::new()),
        error: signal(None),
        location: signal(None),
        headings: signal(Vec::new()),
        rows: signal(Vec::new()),
        offset: signal(0),
        toc: signal(true),
        toc_selected: signal(0),
        state: Rc::new(RefCell::new(State {
            load,
            back: Vec::new(),
            forward: Vec::new(),
            pending: None,
            generation: 0,
        })),
        on_link: Rc::new(RefCell::new(None)),
    }
}

impl MarkdownViewer {
    /// Show the table of contents (the default) or not; `t` toggles it.
    #[must_use]
    pub fn toc(self, shown: bool) -> MarkdownViewer {
        untracked(|| self.toc.set(shown));
        self
    }

    /// Open `location` first ([`go`](Self::go)).
    #[must_use]
    pub fn open(self, location: &str) -> MarkdownViewer {
        self.go(location);
        self
    }

    /// Call `on_link` with a link the viewer does not follow itself: one
    /// with a scheme (`https:`, `mailto:`), to open elsewhere.
    #[must_use]
    pub fn on_link(self, on_link: impl FnMut(&str, &mut Ctx) + 'static) -> MarkdownViewer {
        *self.on_link.borrow_mut() = Some(Box::new(on_link));
        self
    }

    /// Go to `target`: a location relative to the document shown
    /// (`other.md`, `../guide/index.md`), with an optional `#anchor`, or
    /// an anchor alone (`#install`) in the document shown. What was shown
    /// goes on the back history, and the forward history is cleared.
    pub fn go(&self, target: &str) {
        let current = self.location.get_untracked();
        let (path, anchor) = resolve(current.as_deref(), target);
        if let Some(current) = current {
            let offset = self.offset.get_untracked();
            let mut state = self.state.borrow_mut();
            state.back.push(Place {
                path: current,
                offset,
            });
            state.forward.clear();
        }
        let pending = Pending::Anchor(anchor.unwrap_or_default());
        self.show(path, pending);
    }

    /// Go back to what was shown before, where it was scrolled to; `false`
    /// when there is nothing to go back to.
    pub fn back(&self) -> bool {
        self.step(true)
    }

    /// Go forward again after [`back`](Self::back); `false` when there is
    /// nothing to go forward to.
    pub fn forward(&self) -> bool {
        self.step(false)
    }

    /// Whether [`back`](Self::back) has somewhere to go.
    pub fn can_go_back(&self) -> bool {
        !self.state.borrow().back.is_empty()
    }

    /// Whether [`forward`](Self::forward) has somewhere to go.
    pub fn can_go_forward(&self) -> bool {
        !self.state.borrow().forward.is_empty()
    }

    /// The document shown (or loading), without an anchor. Read as a
    /// signal is: a node that reads it draws again when it changes.
    pub fn location(&self) -> Option<String> {
        self.location.get()
    }

    /// The headings of the document shown, in order.
    pub fn headings(&self) -> Vec<MarkdownHeading> {
        self.headings.get()
    }

    /// The first row in view: read it, or set it to scroll.
    pub fn offset(&self) -> Signal<u16> {
        self.offset
    }

    /// Scroll heading `index` (of [`headings`](Self::headings)) to the top.
    pub fn scroll_to_heading(&self, index: usize) {
        let block = self
            .headings
            .with_untracked(|headings| headings.get(index).map(|h| h.block));
        let row = block.and_then(|block| self.rows.with_untracked(|rows| rows.get(block).copied()));
        if let Some(row) = row {
            self.offset.set(row);
        }
    }

    fn step(&self, back: bool) -> bool {
        let place = {
            let mut state = self.state.borrow_mut();
            let place = match back {
                true => state.back.pop(),
                false => state.forward.pop(),
            };
            let Some(place) = place else {
                return false;
            };
            if let Some(current) = self.location.get_untracked() {
                let here = Place {
                    path: current,
                    offset: self.offset.get_untracked(),
                };
                match back {
                    true => state.forward.push(here),
                    false => state.back.push(here),
                }
            }
            place
        };
        self.show(place.path, Pending::Offset(place.offset));
        true
    }

    /// Show `path`, scrolled as `pending` says once it is laid out:
    /// loaded again unless it is the document shown.
    fn show(&self, path: String, pending: Pending) {
        let same = self.location.get_untracked().as_deref() == Some(path.as_str())
            && self.error.get_untracked().is_none();
        if same {
            self.state.borrow_mut().pending = Some(pending);
            let rows = self.rows.get_untracked();
            self.apply_pending(&rows);
            return;
        }
        self.location.set(Some(path.clone()));
        let (load, generation) = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.pending = Some(pending);
            (state.load.clone(), state.generation)
        };
        let viewer = self.clone();
        crate::task::spawn_then(
            move || {
                let text = load(&path);
                (path, text)
            },
            move |(path, text), _| {
                if viewer.state.borrow().generation != generation {
                    return;
                }
                match text {
                    Ok(text) => {
                        let (text, headings) = outline(&text);
                        viewer.error.set(None);
                        viewer.headings.set(headings);
                        viewer.toc_selected.set(0);
                        viewer.offset.set(0);
                        viewer.doc.set(text);
                    }
                    Err(error) => {
                        viewer.state.borrow_mut().pending = None;
                        viewer.headings.set(Vec::new());
                        viewer.doc.set(String::new());
                        viewer.offset.set(0);
                        viewer.error.set(Some(format!("{path}: {error}")));
                    }
                }
            },
        );
    }

    /// Follow a link picked in the document.
    fn follow(&self, url: &str, cx: &mut Ctx) {
        if has_scheme(url) {
            if let Some(on_link) = self.on_link.borrow_mut().as_mut() {
                on_link(url, cx);
            }
        } else {
            self.go(url);
        }
    }

    /// The heading in view: the last whose row is at or above the first
    /// row in view.
    fn in_view(&self) -> Option<usize> {
        let offset = self.offset.get();
        let rows = self.rows.get();
        self.headings.with(|headings| {
            headings
                .iter()
                .rposition(|h| rows.get(h.block).is_some_and(|row| *row <= offset))
                .or((!headings.is_empty()).then_some(0))
        })
    }

    /// The viewer's node: the table of contents beside the document.
    pub fn view(&self) -> Node {
        let me = self.clone();
        let items = move || {
            let current = me.in_view();
            me.headings.with(|headings| {
                headings
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        let mark = if Some(i) == current { "▸ " } else { "  " };
                        let indent = "  ".repeat(h.level.saturating_sub(1) as usize);
                        // The heading's text, not markup.
                        format!("{mark}{indent}{}", h.text.replace('[', "\\["))
                    })
                    .collect()
            })
        };
        let me = self.clone();
        let toc = list(items, self.toc_selected)
            .bind("enter", "Go to the heading", move |_| {
                me.scroll_to_heading(me.toc_selected.get_untracked());
            })
            .what("markdown_viewer toc")
            .name("Contents")
            .panel("Contents")
            .fixed(28);
        let toc = {
            let shown = self.toc;
            toc.shown_when(move || shown.get())
        };
        let (me, rows) = (self.clone(), self.rows);
        let document = markdown(self.doc)
            .on_link(move |url, cx| me.follow(url, cx))
            .on_block_rows({
                let me = self.clone();
                move |starts| {
                    if rows.with_untracked(|r| r.as_slice() != starts) {
                        let starts = starts.to_vec();
                        untracked(|| rows.set(starts));
                    }
                    me.apply_pending(starts);
                }
            });
        let error = self.error;
        let failed = text(move || {
            error
                .get()
                .map(|e| format!("[bad]{}[/]", e.replace('[', "\\[")))
                .unwrap_or_default()
        })
        .shown_when(move || error.get().is_some());
        let body = column([failed, scroll_with(document, self.offset).flex(1)]).flex(1);
        let (back, forward, toggle) = (self.clone(), self.clone(), self.toc);
        let back_alt = self.clone();
        row([toc, body])
            .what("markdown_viewer")
            .bind("t", "Contents", move |_| toggle.update(|on| *on = !*on))
            .bind("backspace", "Back", move |_| {
                back.back();
            })
            .on_key("alt+left", move |_| {
                back_alt.back();
            })
            .bind("alt+right", "Forward", move |_| {
                forward.forward();
            })
    }

    /// Scroll as the last navigation asked, now that the blocks are laid
    /// out.
    fn apply_pending(&self, starts: &[u16]) {
        let Some(pending) = self.state.borrow_mut().pending.take() else {
            return;
        };
        let offset = match pending {
            Pending::Offset(offset) => offset,
            Pending::Anchor(anchor) if anchor.is_empty() => 0,
            Pending::Anchor(anchor) => {
                let block = self.headings.with_untracked(|headings| {
                    headings
                        .iter()
                        .find(|h| h.anchor == anchor)
                        .map(|h| h.block)
                });
                match block.and_then(|block| starts.get(block)) {
                    Some(row) => *row,
                    None => return,
                }
            }
        };
        if self.offset.get_untracked() != offset {
            untracked(|| self.offset.set(offset));
        }
    }
}

/// Whether `url` names a scheme (`https:`, `mailto:`), not a document.
fn has_scheme(url: &str) -> bool {
    match url.find(':') {
        Some(at) => {
            let scheme = &url[..at];
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
                && !url[..at].contains('/')
        }
        None => false,
    }
}

/// `target` read from `current`: the location, made relative to the
/// directory `current` is in, and its anchor.
fn resolve(current: Option<&str>, target: &str) -> (String, Option<String>) {
    let (path, anchor) = match target.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor.to_string())),
        None => (target, None),
    };
    if path.is_empty() {
        return (current.unwrap_or_default().to_string(), anchor);
    }
    let joined = match (path.strip_prefix('/'), current) {
        (Some(absolute), _) => absolute.to_string(),
        (None, Some(current)) => match current.rfind('/') {
            Some(at) => format!("{}/{path}", &current[..at]),
            None => path.to_string(),
        },
        (None, None) => path.to_string(),
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    (parts.join("/"), anchor)
}

/// An ATX heading's level and text (`## Usage ##` is 2, `Usage`).
fn atx(line: &str) -> Option<(u8, String)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let level = rest.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let after = &rest[level..];
    if !(after.is_empty() || after.starts_with([' ', '\t'])) {
        return None;
    }
    let text = after.trim();
    let text = text.trim_end_matches('#');
    Some((level as u8, text.trim().to_string()))
}

/// A heading's text without its inline markup (`**`, `` ` ``, links).
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' => {}
            '[' => {}
            ']' if chars.peek() == Some(&'(') => {
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            ']' => {}
            c => out.push(c),
        }
    }
    out
}

/// GitHub's anchor for a heading: lower case, spaces as `-`, and only
/// letters, digits, `-` and `_` kept.
pub(crate) fn slug(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .filter_map(|c| match c {
            ' ' => Some('-'),
            c if c.is_alphanumeric() || c == '-' || c == '_' => Some(c),
            _ => None,
        })
        .collect()
}

/// `text` with a blank line before each heading that has none, so that
/// every heading starts a block of its own, and its headings.
fn outline(text: &str) -> (String, Vec<MarkdownHeading>) {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fence: Option<String> = None;
    let mut previous_blank = true;
    for line in text.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        let trimmed = content.trim_start();
        if let Some(open) = &fence {
            if trimmed.starts_with(open.as_str()) {
                fence = None;
            }
        } else if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(trimmed[..3].to_string());
        } else if !previous_blank && !content.starts_with([' ', '\t']) && atx(content).is_some() {
            out.push('\n');
        }
        out.push_str(line);
        previous_blank = content.trim().is_empty();
    }
    let mut headings: Vec<MarkdownHeading> = Vec::new();
    for (block, range) in split_blocks(&out, 0).into_iter().enumerate() {
        let source = &out[range];
        let mut lines = source.lines();
        let first = lines.next().unwrap_or_default();
        let starts_fence = {
            let t = first.trim_start();
            t.starts_with("```") || t.starts_with("~~~")
        };
        if starts_fence {
            // A block of code: no heading in it.
            continue;
        }
        let found = atx(first).or_else(|| {
            let underline = lines.next()?.trim();
            let level = match underline.chars().next()? {
                '=' => 1,
                '-' => 2,
                _ => return None,
            };
            let c = underline.chars().next()?;
            (underline.chars().all(|x| x == c) && !first.trim().is_empty())
                .then(|| (level, first.trim().to_string()))
        });
        if let Some((level, text)) = found {
            let text = plain(&text);
            let mut anchor = slug(&text);
            let base = anchor.clone();
            let mut n = 1;
            while headings.iter().any(|h| h.anchor == anchor) {
                anchor = format!("{base}-{n}");
                n += 1;
            }
            headings.push(MarkdownHeading {
                level,
                text,
                anchor,
                block,
            });
        }
    }
    (out, headings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_locations() {
        assert_eq!(resolve(None, "a.md"), ("a.md".into(), None));
        assert_eq!(
            resolve(Some("docs/guide/a.md"), "../b.md#usage"),
            ("docs/b.md".into(), Some("usage".into()))
        );
        assert_eq!(
            resolve(Some("docs/a.md"), "#top"),
            ("docs/a.md".into(), Some("top".into()))
        );
        assert_eq!(resolve(Some("docs/a.md"), "/c.md"), ("c.md".into(), None));
        assert_eq!(
            resolve(Some("docs/a.md"), "./sub/c.md"),
            ("docs/sub/c.md".into(), None)
        );
    }

    #[test]
    fn knows_a_scheme() {
        assert!(has_scheme("https://example.com"));
        assert!(has_scheme("mailto:me@example.com"));
        assert!(!has_scheme("b.md#usage"));
        assert!(!has_scheme("dir/a:b.md"));
    }

    #[test]
    fn outlines_the_headings() {
        let (text, headings) = outline(
            "# Intro\nText.\n## Install `it`\n\n```\n# not a heading\n```\n\nSetup\n-----\n\n## Install it\n",
        );
        assert!(text.starts_with("# Intro\nText.\n\n## Install"), "{text:?}");
        let found: Vec<(u8, &str, &str)> = headings
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.anchor.as_str()))
            .collect();
        assert_eq!(
            found,
            vec![
                (1, "Intro", "intro"),
                (2, "Install it", "install-it"),
                (2, "Setup", "setup"),
                (2, "Install it", "install-it-1"),
            ]
        );
    }
}

//! The page model: one per tab, built from each [`PageSnapshot`] (a
//! `DOMSnapshot.captureSnapshot` and an `Accessibility.getFullAXTree`).
//!
//! - **Blocks**: headings, paragraphs, list items, quotes, code, tables,
//!   media and controls, in reading order, each with runs of styled text
//!   and Chrome's rectangles. Only what Chrome laid out is read: a node
//!   with no layout box (in `display: none`), hidden text and transparent
//!   text are left out, with no rules of our own.
//! - **Controls**: every node whose accessibility role is interactive
//!   (button, link, text box, check box, radio, combo box, slider, …),
//!   with its role, name, value and state as Chrome computes them for a
//!   screen reader, joined to the snapshot by `backendDOMNodeId`.
//!
//! What carries over from the proof of concept's `dom.js`, in Rust against
//! snapshot and AX data:
//!
//! - **Option identity**: a select's options are keyed by their own node,
//!   not their value, so two options with one value stay apart and a
//!   reorder keeps the choice.
//! - **Revision guards**: each control has a *meaning* (role, name, type,
//!   destination, options, form, range) and a *state* (value, checked,
//!   choice), each with a revision that moves when it changes, kept across
//!   snapshots of one document by [`Revisions`]. An action is bound to a
//!   document, a node and a meaning revision, and is refused if any moved.
//! - **The password rule**: a password field's value is never read into
//!   the model, whatever the snapshot holds.
//! - **Cleaning**: every string is cleaned ([`crate::clean`]) here, so
//!   nothing in the model can carry a control character to the terminal.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;

use crate::clean::{clean, clean_inline, clean_pre, truncate};
use crate::engine::{NodeId, PageSnapshot, Rect};

/// At most this many blocks are read from one page.
pub const MAX_BLOCKS: usize = 20_000;
/// At most this many characters of one text.
const MAX_TEXT: usize = 8192;
/// At most this many options of one select.
const MAX_OPTIONS: usize = 500;

/// A run of text with one style.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// Cleaned text; `\n` is a line break of the page's.
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    /// The control (a link, mostly) this text belongs to: an index into
    /// [`Page::controls`].
    pub control: Option<usize>,
    /// Its own colour, as a rich colour (`rgb(r,g,b)`), when it differs
    /// from its block's: an author's name in a log, a highlighted word.
    pub color: Option<String>,
    /// A small picture in the line (an emote, a badge): the text is its
    /// alt text, which stands in where pictures are not drawn.
    pub picture: Option<Picture>,
}

/// A small picture inside a line of text.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Picture {
    /// Its source address, cleaned.
    pub src: String,
    pub alt: String,
    /// Its size in CSS pixels.
    pub width: f64,
    pub height: f64,
    /// An empty `alt`, or hidden from assistive technology.
    pub decorative: bool,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Run {
        Run {
            text: text.into(),
            ..Run::default()
        }
    }
}

/// A table cell.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cell {
    pub runs: Vec<Run>,
    pub header: bool,
    pub rect: Rect,
}

impl Cell {
    pub fn text(&self) -> String {
        plain(&self.runs)
    }
}

/// A table: its caption and rows of cells.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub caption: String,
    pub rows: Vec<Vec<Cell>>,
}

/// What a block is.
#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    /// `<h1>` to `<h6>`, or a `heading` role: the level.
    Heading(u8),
    Paragraph,
    /// A list item, `depth` lists deep (from 1), with its marker (`•`,
    /// `1.`); a continuation of an item after a nested list has none.
    ListItem {
        depth: u8,
        marker: String,
    },
    /// Preformatted text: its runs keep their line breaks.
    Code,
    Table(Table),
    /// A media region: an index into [`Page::media`].
    Media(usize),
    /// A form control: an index into [`Page::controls`].
    Control(usize),
    /// A live log: an index into [`Page::logs`].
    Log(usize),
}

/// A block of the page, in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    pub runs: Vec<Run>,
    /// How many quotes deep it is.
    pub quote: u8,
    /// Where its text is, in CSS pixels of the document.
    pub rect: Rect,
    /// The box of the element that holds it (the paragraph's, the cell's).
    pub frame: Rect,
    /// The node that made it.
    pub node: NodeId,
    /// The landmark it is in: an index into [`Page::regions`].
    pub region: Option<usize>,
}

impl Block {
    /// Its text, without styles.
    pub fn text(&self) -> String {
        plain(&self.runs)
    }
}

/// The text of `runs`.
pub fn plain(runs: &[Run]) -> String {
    runs.iter().map(|r| r.text.as_str()).collect()
}

/// A control's role, as the accessibility tree names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Button,
    Link,
    TextBox,
    SearchBox,
    /// A text box whose value is never read.
    Password,
    CheckBox,
    Switch,
    Radio,
    /// A select, or any combo box.
    ComboBox,
    ListBox,
    Slider,
    SpinButton,
    Tab,
    MenuItem,
}

impl Role {
    /// The role from its accessibility name, if it is an interactive one.
    pub fn from_ax(name: &str) -> Option<Role> {
        Some(match name {
            "button" | "toggleButton" | "popUpButton" => Role::Button,
            "link" => Role::Link,
            "textbox" | "textField" => Role::TextBox,
            "searchbox" => Role::SearchBox,
            "checkbox" | "menuitemcheckbox" => Role::CheckBox,
            "switch" => Role::Switch,
            "radio" | "menuitemradio" => Role::Radio,
            "combobox" | "comboBoxMenuButton" | "comboBoxSelect" => Role::ComboBox,
            "listbox" => Role::ListBox,
            "slider" => Role::Slider,
            "spinbutton" => Role::SpinButton,
            "tab" => Role::Tab,
            "menuitem" => Role::MenuItem,
            _ => return None,
        })
    }

    /// Its accessibility name, as config files and the overlay say it.
    pub fn name(&self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Link => "link",
            Role::TextBox => "textbox",
            Role::SearchBox => "searchbox",
            Role::Password => "textbox",
            Role::CheckBox => "checkbox",
            Role::Switch => "switch",
            Role::Radio => "radio",
            Role::ComboBox => "combobox",
            Role::ListBox => "listbox",
            Role::Slider => "slider",
            Role::SpinButton => "spinbutton",
            Role::Tab => "tab",
            Role::MenuItem => "menuitem",
        }
    }

    /// Whether the user edits it as text.
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            Role::TextBox | Role::SearchBox | Role::Password | Role::SpinButton
        )
    }
}

/// A check box's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Check {
    Off,
    On,
    Mixed,
}

/// One option of a select, keyed by its own node.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Choice {
    pub key: NodeId,
    pub label: String,
    pub disabled: bool,
    pub selected: bool,
}

/// A control of the page.
#[derive(Clone, Debug, PartialEq)]
pub struct Control {
    pub node: NodeId,
    pub role: Role,
    /// Its accessible name, cleaned.
    pub name: String,
    /// Its value; always `None` for a password.
    pub value: Option<String>,
    pub checked: Option<Check>,
    pub disabled: bool,
    pub readonly: bool,
    /// A select's options, in order.
    pub options: Vec<Choice>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    /// Where a link goes, resolved and cleaned.
    pub href: Option<String>,
    /// A radio button's group (its `name`).
    pub group: Option<String>,
    pub rect: Rect,
    /// Whether the accessibility tree had it (else it is from the DOM).
    pub from_ax: bool,
    /// A button with no text of its own: its name comes from a label or
    /// a title (an icon button).
    pub icon_only: bool,
    /// The meaning's revision: role, name, type, destination, options.
    pub revision: u32,
    /// The state's revision: value, checked, choice.
    pub state_revision: u32,
}

impl Control {
    /// The chosen option, for a select.
    pub fn selected(&self) -> Option<&Choice> {
        self.options.iter().find(|o| o.selected)
    }

    /// The binding an action on it carries: see [`Page::check`].
    pub fn binding(&self, page: &Page) -> Binding {
        Binding {
            document: page.document,
            node: self.node,
            revision: self.revision,
        }
    }
}

/// What a media region holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
    Audio,
    Canvas,
    Svg,
    Frame,
}

/// An opaque box of the page: an image, a video, a canvas, a frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub node: NodeId,
    pub kind: MediaKind,
    pub rect: Rect,
    /// Its text alternative, cleaned.
    pub alt: String,
    /// Its source address, cleaned.
    pub src: String,
    /// A still of it, as image bytes (PNG), once captured.
    pub still: Option<Arc<[u8]>>,
    /// A video's or an audio's playback, read from the element.
    pub playback: Option<crate::engine::Playback>,
    /// It has an `alt` attribute, and it is empty: decorative by the
    /// author's word.
    pub empty_alt: bool,
    /// It is hidden from assistive technology (`aria-hidden`, or a
    /// `presentation` role), itself or by an ancestor.
    pub aria_hidden: bool,
    /// It is inside a heading, a link, a button or a label: a picture
    /// that stands for words.
    pub in_label: bool,
}

/// What made a box a live log.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogKind {
    /// `role=log`.
    Log,
    /// `aria-live` (`polite` or `assertive`).
    Live,
    /// A scroll box the page appends entries to.
    Appended,
}

/// One entry of a live log: a child of its box, as one line of runs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    /// Stable while the entry's node stays: its `backendNodeId` from a
    /// snapshot, or the isolated world's own number for it.
    pub key: u64,
    pub runs: Vec<Run>,
}

impl Entry {
    pub fn text(&self) -> String {
        plain(&self.runs)
    }
}

/// A live log: a box whose entries the page appends, followed through
/// the live change ping and read incrementally.
#[derive(Clone, Debug, PartialEq)]
pub struct Log {
    pub node: NodeId,
    pub kind: LogKind,
    /// Its box, in CSS pixels of the document.
    pub rect: Rect,
    /// Its accessible name, if it has one.
    pub label: String,
    /// Its entries, oldest first: at most [`MAX_ENTRIES`], the newest.
    pub entries: Vec<Entry>,
    /// The first entry Chrome shows in the box, at its own scroll: what
    /// the faithful projection shows from.
    pub first_shown: Option<u64>,
}

/// At most this many entries of a live log are kept: the newest.
pub const MAX_ENTRIES: usize = 500;

/// What a landmark is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegionKind {
    /// `<nav>` or `role=navigation`.
    Navigation,
    /// `<aside>` or `role=complementary`.
    Complementary,
}

/// A landmark of the page whose blocks can collapse together.
#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub node: NodeId,
    pub kind: RegionKind,
    pub rect: Rect,
    /// Its accessible name, if it has one.
    pub label: String,
    /// No field and no live log in it: it can collapse to its links.
    pub collapsible: bool,
}

/// A grid, or a flex row that wraps: its items' boxes in order.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub node: NodeId,
    pub rect: Rect,
    pub items: Vec<Rect>,
}

/// An action's binding to its target: the document, the node, and the
/// meaning revision it was chosen at.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub document: u64,
    pub node: NodeId,
    pub revision: u32,
}

/// The page, as read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Page {
    pub url: String,
    pub title: String,
    /// Which document this is: it changes when the tab goes to another.
    pub document: u64,
    pub blocks: Vec<Block>,
    pub controls: Vec<Control>,
    pub media: Vec<Media>,
    /// The document's size in CSS pixels.
    pub width: f64,
    pub height: f64,
    /// Whether the page had more than the model reads.
    pub truncated: bool,
    /// Whether the accessibility tree came with the snapshot.
    pub has_ax: bool,
    /// Its live logs.
    pub logs: Vec<Log>,
    /// Its navigation and complementary landmarks.
    pub regions: Vec<Region>,
    /// Its grids and wrapping flex rows.
    pub grids: Vec<Grid>,
}

impl Page {
    /// The control on `node`, if the page has one.
    pub fn control(&self, node: NodeId) -> Option<&Control> {
        self.controls.iter().find(|c| c.node == node)
    }

    /// Whether an action bound by `binding` may run on this page: the same
    /// document, the node still there, and its meaning unchanged.
    /// Otherwise, why not.
    pub fn check(&self, binding: &Binding) -> Result<&Control, String> {
        if binding.document != self.document {
            return Err("Stale document: the tab went to another page.".into());
        }
        let control = self
            .control(binding.node)
            .ok_or("Stale target: removed or replaced.")?;
        if control.revision != binding.revision {
            return Err("Stale control: its meaning changed.".into());
        }
        Ok(control)
    }

    /// The controls whose role and accessible name are `role` and `name`
    /// (both compared without case), in reading order.
    pub fn find(&self, role: &str, name: &str) -> Vec<&Control> {
        self.controls
            .iter()
            .filter(|c| {
                c.role.name().eq_ignore_ascii_case(role) && c.name.eq_ignore_ascii_case(name.trim())
            })
            .collect()
    }

    /// All the page's text, block by block, for find and dumps.
    pub fn text(&self) -> String {
        let mut out = Vec::new();
        for block in &self.blocks {
            match &block.kind {
                BlockKind::Table(table) => {
                    for row in &table.rows {
                        out.push(row.iter().map(Cell::text).collect::<Vec<_>>().join(" | "));
                    }
                }
                BlockKind::Media(i) => out.push(format!("[{}]", self.media[*i].alt)),
                BlockKind::Control(i) => out.push(self.controls[*i].name.clone()),
                BlockKind::Log(i) => out.extend(self.logs[*i].entries.iter().map(Entry::text)),
                _ => out.push(block.text()),
            }
        }
        out.join("\n")
    }
}

/// The revisions of one tab's controls, kept from one snapshot to the
/// next while the document stays the same.
#[derive(Clone, Debug, Default)]
pub struct Revisions {
    document: Option<(String, i64)>,
    generation: u64,
    seen: HashMap<NodeId, Seen>,
}

#[derive(Clone, Copy, Debug)]
struct Seen {
    meaning: u64,
    revision: u32,
    state: u64,
    state_revision: u32,
}

impl Revisions {
    pub fn new() -> Revisions {
        Revisions::default()
    }

    /// The document id for this snapshot's document, a new one when it
    /// changed (its address or its root node).
    fn document(&mut self, url: &str, root: i64) -> u64 {
        let key = (url.to_string(), root);
        if self.document.as_ref() != Some(&key) {
            self.document = Some(key);
            self.generation += 1;
            self.seen.clear();
        }
        self.generation
    }

    fn revise(&mut self, node: NodeId, meaning: u64, state: u64) -> (u32, u32) {
        let seen = self.seen.entry(node).or_insert(Seen {
            meaning,
            revision: 1,
            state,
            state_revision: 1,
        });
        if seen.meaning != meaning {
            seen.meaning = meaning;
            seen.revision += 1;
        }
        if seen.state != state {
            seen.state = state;
            seen.state_revision += 1;
        }
        (seen.revision, seen.state_revision)
    }
}

/// The snapshot's first document, in arrays.
struct Dom {
    parent: Vec<i64>,
    kind: Vec<i64>,
    name: Vec<String>,
    value: Vec<String>,
    backend: Vec<i64>,
    attrs: Vec<Vec<(String, String)>>,
    children: Vec<Vec<usize>>,
    layout: Vec<Option<usize>>,
    styles: Vec<Vec<String>>,
    bounds: Vec<Rect>,
    /// Each node's laid-out text: from the first of its layout objects
    /// that has any (a list marker's is on its second).
    text: Vec<Option<String>>,
    input_value: HashMap<usize, String>,
    text_value: HashMap<usize, String>,
    checked: HashSet<usize>,
    selected: HashSet<usize>,
    source: HashMap<usize, String>,
    url: String,
    title: String,
    width: f64,
    height: f64,
}

impl Dom {
    fn parse(snapshot: &Value) -> Dom {
        let strings: Vec<&str> = snapshot["strings"]
            .as_array()
            .map(|all| all.iter().map(|s| s.as_str().unwrap_or_default()).collect())
            .unwrap_or_default();
        let string = |value: &Value| -> String {
            value
                .as_i64()
                .and_then(|i| strings.get(usize::try_from(i).ok()?).copied())
                .unwrap_or_default()
                .to_string()
        };
        let ints = |value: &Value| -> Vec<i64> {
            value
                .as_array()
                .map(|all| all.iter().map(|n| n.as_i64().unwrap_or(-1)).collect())
                .unwrap_or_default()
        };
        let document = &snapshot["documents"][0];
        let nodes = &document["nodes"];
        let parent = ints(&nodes["parentIndex"]);
        let n = parent.len();
        let column = |key: &str| -> Vec<Value> {
            let mut all = nodes[key].as_array().cloned().unwrap_or_default();
            all.resize(n, Value::Null);
            all
        };
        let name = column("nodeName")
            .iter()
            .map(|v| string(v).to_ascii_uppercase())
            .collect();
        let value = column("nodeValue").iter().map(string).collect();
        let mut kind = ints(&nodes["nodeType"]);
        kind.resize(n, 0);
        let mut backend = ints(&nodes["backendNodeId"]);
        backend.resize(n, -1);
        let attrs = column("attributes")
            .iter()
            .map(|list| {
                let list = list.as_array().cloned().unwrap_or_default();
                list.chunks(2)
                    .map(|pair| {
                        (
                            string(&pair[0]).to_ascii_lowercase(),
                            pair.get(1).map(string).unwrap_or_default(),
                        )
                    })
                    .collect()
            })
            .collect();
        let mut children = vec![Vec::new(); n];
        for (i, &p) in parent.iter().enumerate() {
            if let Some(p) = usize::try_from(p).ok().filter(|&p| p < n) {
                children[p].push(i);
            }
        }
        let rare_strings = |key: &str| -> HashMap<usize, String> {
            let index = ints(&nodes[key]["index"]);
            let values: Vec<String> = nodes[key]["value"]
                .as_array()
                .map(|all| all.iter().map(string).collect())
                .unwrap_or_default();
            index
                .into_iter()
                .zip(values)
                .filter_map(|(i, v)| Some((usize::try_from(i).ok()?, v)))
                .collect()
        };
        let rare_bools = |key: &str| -> HashSet<usize> {
            ints(&nodes[key]["index"])
                .into_iter()
                .filter_map(|i| usize::try_from(i).ok())
                .collect()
        };
        let layout_tree = &document["layout"];
        let layout_text: Vec<Option<String>> = layout_tree["text"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|t| t.as_i64().filter(|&i| i >= 0).map(|_| string(t)))
                    .collect()
            })
            .unwrap_or_default();
        let mut layout = vec![None; n];
        let mut text: Vec<Option<String>> = vec![None; n];
        for (k, index) in ints(&layout_tree["nodeIndex"]).into_iter().enumerate() {
            let Some(i) = usize::try_from(index).ok().filter(|&i| i < n) else {
                continue;
            };
            // A node can have several layout objects (a pseudo's box and
            // its text); the first is its own box.
            layout[i].get_or_insert(k);
            if text[i].is_none() {
                text[i] = layout_text.get(k).cloned().flatten();
            }
        }
        let styles = layout_tree["styles"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|s| {
                        s.as_array()
                            .map(|v| v.iter().map(string).collect())
                            .unwrap_or_default()
                    })
                    .collect()
            })
            .unwrap_or_default();
        let bounds = layout_tree["bounds"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|b| {
                        let at = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                        Rect::new(at(0), at(1), at(2), at(3))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Dom {
            parent,
            kind,
            name,
            value,
            backend,
            attrs,
            children,
            layout,
            styles,
            bounds,
            text,
            input_value: rare_strings("inputValue"),
            text_value: rare_strings("textValue"),
            checked: rare_bools("inputChecked"),
            selected: rare_bools("optionSelected"),
            source: rare_strings("currentSourceURL"),
            url: string(&document["documentURL"]),
            title: string(&document["title"]),
            width: document["contentWidth"].as_f64().unwrap_or(0.0),
            height: document["contentHeight"].as_f64().unwrap_or(0.0),
        }
    }

    fn attr(&self, i: usize, name: &str) -> Option<&str> {
        self.attrs[i]
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    fn style(&self, i: usize, at: usize) -> &str {
        self.layout[i]
            .and_then(|k| self.styles.get(k))
            .and_then(|s| s.get(at))
            .map_or("", String::as_str)
    }

    fn rect(&self, i: usize) -> Option<Rect> {
        self.layout[i].and_then(|k| self.bounds.get(k).copied())
    }

    fn is_element(&self, i: usize) -> bool {
        self.kind[i] == 1
    }

    /// Whether a laid-out node shows: not hidden, not transparent.
    fn shows(&self, i: usize) -> bool {
        self.layout[i].is_some()
            && !matches!(self.style(i, 1), "hidden" | "collapse")
            && self.style(i, 8) != "0"
    }

    /// The text of a node's text descendants, laid out or not (an
    /// option's label is never laid out).
    fn raw_text(&self, i: usize) -> String {
        let mut out = String::new();
        let mut stack = vec![i];
        while let Some(n) = stack.pop() {
            if self.kind[n] == 3 {
                out.push_str(&self.value[n]);
            }
            stack.extend(self.children[n].iter().rev());
        }
        out
    }

    /// Where a live log's entries are: the box itself, or the one box it
    /// wraps them in (a scroller inside a panel), however deep. A box
    /// whose only child holds inline content holds one entry: that child.
    fn entry_root(&self, mut i: usize) -> usize {
        let elements = |n: usize| -> Vec<usize> {
            self.children[n]
                .iter()
                .copied()
                .filter(|&c| {
                    self.is_element(c)
                        && self.layout[c].is_some()
                        && !SKIPPED.contains(&self.name[c].as_str())
                        && !self.name[c].starts_with("::")
                })
                .collect()
        };
        let texty = |n: usize| {
            self.children[n]
                .iter()
                .any(|&c| self.kind[c] == 3 && !self.value[c].trim().is_empty())
        };
        for _ in 0..32 {
            let mine = elements(i);
            if mine.len() != 1 || texty(i) {
                break;
            }
            let only = mine[0];
            let theirs = elements(only);
            if theirs.is_empty()
                || texty(only)
                || theirs.iter().any(|&g| self.style(g, 0).starts_with("inline"))
            {
                break;
            }
            i = only;
        }
        i
    }

    fn descendants(&self, i: usize) -> Vec<usize> {
        let mut out = Vec::new();
        let mut stack: Vec<usize> = self.children[i].iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            out.push(n);
            stack.extend(self.children[n].iter().rev());
        }
        out
    }
}

/// The accessibility tree's nodes, by DOM node.
struct Ax<'a> {
    by_node: HashMap<i64, &'a Value>,
}

impl<'a> Ax<'a> {
    fn parse(ax: &'a Value) -> Ax<'a> {
        let mut by_node = HashMap::new();
        for node in ax["nodes"].as_array().into_iter().flatten() {
            if node["ignored"].as_bool() == Some(true) {
                continue;
            }
            if let Some(id) = node["backendDOMNodeId"].as_i64() {
                by_node.entry(id).or_insert(node);
            }
        }
        Ax { by_node }
    }

    fn get(&self, node: i64) -> Option<&'a Value> {
        self.by_node.get(&node).copied()
    }

    fn role(&self, node: i64) -> Option<Role> {
        self.get(node)
            .and_then(|n| n["role"]["value"].as_str())
            .and_then(Role::from_ax)
    }
}

fn property<'v>(node: &'v Value, name: &str) -> Option<&'v Value> {
    node["properties"]
        .as_array()?
        .iter()
        .find(|p| p["name"] == name)
        .map(|p| &p["value"]["value"])
}

fn ax_string(node: &Value, key: &str) -> Option<String> {
    let text = match &node[key]["value"] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    Some(clean(&text))
}

fn number(text: Option<&str>) -> Option<f64> {
    text.and_then(|t| t.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
}

fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

fn union(a: Option<Rect>, b: Rect) -> Rect {
    match a {
        None => b,
        Some(a) => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            Rect::new(
                x,
                y,
                a.right().max(b.right()) - x,
                a.bottom().max(b.bottom()) - y,
            )
        }
    }
}

/// What the walk carries down the tree.
#[derive(Clone, Debug)]
struct Context {
    kind: BlockKind,
    frame: Rect,
    node: NodeId,
    bold: bool,
    italic: bool,
    code: bool,
    pre: bool,
    quote: u8,
    depth: u8,
    control: Option<usize>,
    /// Inside a label of a control, or a control: its text is the
    /// control's name, not text of its own.
    silent: bool,
    /// The block's own colour: a run in another colour keeps it.
    color: String,
    /// The landmark it is in: an index into [`Page::regions`].
    region: Option<usize>,
    /// Hidden from assistive technology by an ancestor.
    aria_hidden: bool,
}

/// Tags whose content is never page text.
const SKIPPED: &[&str] = &[
    "HEAD", "SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT", "TITLE", "META", "LINK", "OPTION",
    "OPTGROUP", "DATALIST",
];

struct Walker<'a> {
    dom: &'a Dom,
    ax: &'a Ax<'a>,
    page: Page,
    runs: Vec<Run>,
    rect: Option<Rect>,
    marker: Option<String>,
    by_node: HashMap<NodeId, usize>,
    revisions: &'a mut Revisions,
    /// Scroll boxes the page appends entries to: live logs too.
    appended: &'a HashSet<NodeId>,
}

impl Walker<'_> {
    fn push(&mut self, run: Run, rect: Option<Rect>) {
        if let Some(rect) = rect {
            self.rect = Some(union(self.rect, rect));
        }
        match self.runs.last_mut() {
            Some(last)
                if last.bold == run.bold
                    && last.italic == run.italic
                    && last.code == run.code
                    && last.control == run.control
                    && last.color == run.color
                    && last.picture.is_none()
                    && run.picture.is_none() =>
            {
                last.text.push_str(&run.text)
            }
            _ => self.runs.push(run),
        }
    }

    /// End the block being gathered, as `context` says what it is.
    fn flush(&mut self, context: &Context) {
        let mut runs = std::mem::take(&mut self.runs);
        let rect = self.rect.take();
        if !context.pre {
            tidy(&mut runs);
        }
        if runs.iter().all(|r| r.text.trim().is_empty()) {
            return;
        }
        if self.page.blocks.len() >= MAX_BLOCKS {
            self.page.truncated = true;
            return;
        }
        let kind = match &context.kind {
            BlockKind::ListItem { depth, .. } => BlockKind::ListItem {
                depth: *depth,
                marker: self.marker.take().unwrap_or_default(),
            },
            other => other.clone(),
        };
        let rect = rect.unwrap_or(context.frame);
        self.page.blocks.push(Block {
            kind,
            runs,
            quote: context.quote,
            rect,
            frame: context.frame,
            node: context.node,
            region: context.region,
        });
    }

    fn block(&mut self, kind: BlockKind, i: usize, context: &Context) {
        let rect = self.dom.rect(i).unwrap_or_default();
        self.page.blocks.push(Block {
            kind,
            runs: Vec::new(),
            quote: context.quote,
            rect,
            frame: rect,
            node: self.dom.backend[i],
            region: context.region,
        });
    }

    fn walk(&mut self, i: usize, context: &Context) {
        let dom = self.dom;
        match dom.kind[i] {
            3 => {
                if context.silent || !dom.shows(i) {
                    return;
                }
                let Some(text) = dom.text[i].clone() else {
                    return;
                };
                let pre = context.pre || dom.style(i, 7).starts_with("pre");
                let text = if pre {
                    clean_pre(&text)
                } else {
                    clean_inline(&text)
                };
                if text.is_empty() {
                    return;
                }
                let weight = dom.style(i, 4).parse::<u32>().unwrap_or(400);
                let color = dom.style(i, 2);
                let run = Run {
                    text: truncate(&text, MAX_TEXT),
                    bold: context.bold || weight >= 600,
                    italic: context.italic || matches!(dom.style(i, 5), "italic" | "oblique"),
                    code: context.code,
                    control: context.control,
                    color: (context.control.is_none()
                        && !context.color.is_empty()
                        && color != context.color)
                        .then(|| terminal_color(color))
                        .flatten(),
                    picture: None,
                };
                self.push(run, dom.rect(i));
            }
            1 => self.element(i, context),
            // The document, shadow roots, and anything else with children.
            _ => {
                for &child in &dom.children[i] {
                    self.walk(child, context);
                }
            }
        }
    }

    fn element(&mut self, i: usize, context: &Context) {
        let dom = self.dom;
        let name = dom.name[i].as_str();
        if SKIPPED.contains(&name) {
            return;
        }
        if name == "::MARKER" {
            if let Some(text) = &dom.text[i] {
                self.marker = Some(clean(text));
            }
            return;
        }
        if name.starts_with("::") {
            return;
        }
        if name == "BR" {
            if !context.silent {
                self.push(
                    Run {
                        text: "\n".into(),
                        control: context.control,
                        ..Run::default()
                    },
                    None,
                );
            }
            return;
        }
        let backend = dom.backend[i];
        let laid_out = dom.layout[i].is_some();
        // Media regions.
        let media = match name {
            "IMG" => Some(MediaKind::Image),
            "VIDEO" => Some(MediaKind::Video),
            "AUDIO" => Some(MediaKind::Audio),
            "CANVAS" => Some(MediaKind::Canvas),
            "SVG" => Some(MediaKind::Svg),
            "IFRAME" | "FRAME" | "OBJECT" | "EMBED" => Some(MediaKind::Frame),
            _ => None,
        };
        if let Some(kind) = media {
            if laid_out && dom.shows(i) {
                self.media(i, kind, context);
            }
            return;
        }
        if laid_out && dom.shows(i) && !context.silent {
            if let Some(kind) = self.log_kind(i) {
                self.flush(context);
                self.log(i, kind, context);
                return;
            }
        }
        // Controls: an interactive role in the AX tree, or a form element.
        let role = self.ax.role(backend).or_else(|| dom_role(dom, i));
        if let Some(role) = role {
            if !laid_out || !dom.shows(i) && role != Role::Link {
                return;
            }
            let index = self.control(i, role.clone());
            if role == Role::Link {
                let inner = Context {
                    control: Some(index),
                    ..context.clone()
                };
                for &child in &dom.children[i] {
                    self.walk(child, &inner);
                }
                return;
            }
            if context.silent && context.control.is_some() {
                return;
            }
            // A control inside a block's text ends that text, and stands
            // as a block of its own.
            self.flush(context);
            self.block(BlockKind::Control(index), i, context);
            return;
        }
        if name == "TABLE" && dom.style(i, 0).starts_with("table") {
            self.flush(context);
            if let Some(table) = self.table(i, context) {
                let rect = dom.rect(i).unwrap_or_default();
                self.page.blocks.push(Block {
                    kind: BlockKind::Table(table),
                    runs: Vec::new(),
                    quote: context.quote,
                    rect,
                    frame: rect,
                    node: backend,
                    region: context.region,
                });
            }
            return;
        }
        let mut inner = context.clone();
        let role_attr = dom.attr(i, "role").unwrap_or_default().to_ascii_lowercase();
        if dom.attr(i, "aria-hidden").is_some_and(|v| v.eq_ignore_ascii_case("true")) {
            inner.aria_hidden = true;
        }
        // Landmarks: the outermost navigation or complementary one.
        if laid_out && context.region.is_none() {
            let ax_role = self
                .ax
                .get(backend)
                .and_then(|n| n["role"]["value"].as_str())
                .unwrap_or_default();
            let kind = match (name, role_attr.as_str(), ax_role) {
                (_, "navigation", _) | (_, _, "navigation") | ("NAV", "", _) => {
                    Some(RegionKind::Navigation)
                }
                (_, "complementary", _) | (_, _, "complementary") | ("ASIDE", "", _) => {
                    Some(RegionKind::Complementary)
                }
                _ => None,
            };
            if let Some(kind) = kind {
                inner.region = Some(self.page.regions.len());
                self.page.regions.push(Region {
                    node: backend,
                    kind,
                    rect: dom.rect(i).unwrap_or_default(),
                    label: self.label(i),
                    collapsible: true,
                });
            }
        }
        // Grids, and flex rows that wrap: their items, for snapping.
        let display = dom.style(i, 0);
        if laid_out
            && (matches!(display, "grid" | "inline-grid")
                || (matches!(display, "flex" | "inline-flex") && dom.style(i, 10) == "wrap"))
        {
            let items: Vec<Rect> = dom.children[i]
                .iter()
                .filter(|&&c| dom.is_element(c) && dom.shows(c))
                .filter_map(|&c| dom.rect(c))
                .filter(|r| !r.is_empty())
                .collect();
            if items.len() >= 2 {
                self.page.grids.push(Grid {
                    node: backend,
                    rect: dom.rect(i).unwrap_or_default(),
                    items,
                });
            }
        }
        match name {
            "B" | "STRONG" => inner.bold = true,
            "I" | "EM" | "CITE" | "DFN" => inner.italic = true,
            "CODE" | "KBD" | "SAMP" | "TT" => inner.code = true,
            "LABEL" => {
                let labels_a_control = dom.attr(i, "for").is_some()
                    || dom
                        .descendants(i)
                        .into_iter()
                        .any(|d| dom.is_element(d) && dom_role(dom, d).is_some());
                if labels_a_control {
                    inner.silent = true;
                }
            }
            _ => {}
        }
        let is_block = laid_out
            && !display.is_empty()
            && !display.starts_with("inline")
            && display != "contents"
            && display != "none";
        let heading = match name {
            "H1" => Some(1),
            "H2" => Some(2),
            "H3" => Some(3),
            "H4" => Some(4),
            "H5" => Some(5),
            "H6" => Some(6),
            _ => None,
        }
        .or_else(|| {
            let node = self.ax.get(backend)?;
            (node["role"]["value"] == "heading")
                .then(|| property(node, "level").and_then(Value::as_u64).unwrap_or(2) as u8)
        });
        if !is_block && heading.is_none() && !matches!(name, "LI" | "PRE" | "BLOCKQUOTE") {
            for &child in &dom.children[i] {
                self.walk(child, &inner);
            }
            return;
        }
        self.flush(context);
        inner.frame = dom.rect(i).unwrap_or(context.frame);
        inner.node = backend;
        inner.color = dom.style(i, 2).to_string();
        inner.kind = match (heading, name) {
            (Some(level), _) => BlockKind::Heading(level),
            (None, "LI") => {
                inner.depth = context.depth.max(1);
                self.marker = None;
                BlockKind::ListItem {
                    depth: inner.depth,
                    marker: String::new(),
                }
            }
            (None, "PRE") => {
                inner.pre = true;
                inner.code = true;
                BlockKind::Code
            }
            (None, "UL" | "OL" | "MENU" | "DL") => {
                inner.depth = context.depth + 1;
                context.kind.clone()
            }
            (None, "BLOCKQUOTE") => {
                inner.quote = context.quote + 1;
                BlockKind::Paragraph
            }
            _ if matches!(context.kind, BlockKind::ListItem { .. }) => context.kind.clone(),
            _ if inner.pre => BlockKind::Code,
            _ => BlockKind::Paragraph,
        };
        if matches!(name, "UL" | "OL" | "MENU" | "DL") {
            inner.kind = BlockKind::Paragraph;
        }
        for &child in &dom.children[i] {
            self.walk(child, &inner);
        }
        self.flush(&inner);
    }

    fn media(&mut self, i: usize, kind: MediaKind, context: &Context) {
        let dom = self.dom;
        let backend = dom.backend[i];
        let alt = dom
            .attr(i, "alt")
            .map(clean)
            .filter(|a| !a.is_empty())
            .or_else(|| self.ax.get(backend).and_then(|n| ax_string(n, "name")))
            .or_else(|| dom.attr(i, "title").map(clean))
            .or_else(|| dom.attr(i, "aria-label").map(clean))
            .unwrap_or_default();
        let src = dom
            .source
            .get(&i)
            .map(String::as_str)
            .or_else(|| dom.attr(i, "src"))
            .map(|s| clean(&crate::url::resolve(&dom.url, s)))
            .unwrap_or_default();
        self.flush(context);
        let index = self.page.media.len();
        let role = dom.attr(i, "role").unwrap_or_default().to_ascii_lowercase();
        self.page.media.push(Media {
            node: backend,
            kind,
            rect: dom.rect(i).unwrap_or_default(),
            alt: truncate(&alt, 500),
            src: truncate(&src, 2000),
            still: None,
            playback: None,
            empty_alt: dom.attr(i, "alt").is_some_and(|a| a.trim().is_empty()),
            aria_hidden: context.aria_hidden
                || dom
                    .attr(i, "aria-hidden")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                || matches!(role.as_str(), "presentation" | "none"),
            in_label: context.control.is_some()
                || context.silent
                || matches!(context.kind, BlockKind::Heading(_)),
        });
        self.block(BlockKind::Media(index), i, context);
    }

    /// Its accessible name: the AX tree's, else `aria-label`.
    fn label(&self, i: usize) -> String {
        let dom = self.dom;
        self.ax
            .get(dom.backend[i])
            .and_then(|n| ax_string(n, "name"))
            .filter(|n| !n.is_empty())
            .or_else(|| dom.attr(i, "aria-label").map(clean))
            .map(|l| truncate(&l, 200))
            .unwrap_or_default()
    }

    /// Whether node `i` is a live log, and why: `role=log`, `aria-live`,
    /// or a scroll box the page appends to. A live box of plain text (a
    /// status line) is not a log: a log's entries are elements.
    fn log_kind(&self, i: usize) -> Option<LogKind> {
        let dom = self.dom;
        let role = dom.attr(i, "role").unwrap_or_default().to_ascii_lowercase();
        let ax_log = self
            .ax
            .get(dom.backend[i])
            .is_some_and(|n| n["role"]["value"] == "log");
        let live = dom
            .attr(i, "aria-live")
            .is_some_and(|v| matches!(v.trim(), "polite" | "assertive"));
        let kind = if role.split_whitespace().any(|r| r == "log") || ax_log {
            LogKind::Log
        } else if live {
            LogKind::Live
        } else if self.appended.contains(&dom.backend[i]) {
            LogKind::Appended
        } else {
            return None;
        };
        let root = dom.entry_root(i);
        let has_text = dom.children[root]
            .iter()
            .any(|&c| dom.kind[c] == 3 && !dom.value[c].trim().is_empty());
        let has_entries = dom.children[root]
            .iter()
            .any(|&c| dom.is_element(c) && dom.layout[c].is_some());
        (has_entries && !has_text).then_some(kind)
    }

    /// The live log on node `i`: each child of its entry box an entry,
    /// its runs with their colours and small pictures.
    fn log(&mut self, i: usize, kind: LogKind, context: &Context) {
        let dom = self.dom;
        let rect = dom.rect(i).unwrap_or_default();
        let root = dom.entry_root(i);
        let mut entries = Vec::new();
        let mut first_shown = None;
        for &child in &dom.children[root] {
            if !dom.is_element(child)
                || dom.layout[child].is_none()
                || !dom.shows(child)
                || SKIPPED.contains(&dom.name[child].as_str())
            {
                continue;
            }
            let frame = dom.rect(child).unwrap_or(rect);
            let inner = Context {
                kind: BlockKind::Paragraph,
                frame,
                node: dom.backend[child],
                color: dom.style(child, 2).to_string(),
                control: None,
                silent: false,
                ..context.clone()
            };
            let mut runs = self.inline(child, &inner);
            tidy(&mut runs);
            if runs.is_empty() {
                continue;
            }
            let key = dom.backend[child] as u64;
            if first_shown.is_none() && frame.bottom() > rect.y + 1.0 {
                first_shown = Some(key);
            }
            entries.push(Entry { key, runs });
        }
        if entries.len() > MAX_ENTRIES {
            entries.drain(..entries.len() - MAX_ENTRIES);
        }
        let index = self.page.logs.len();
        self.page.logs.push(Log {
            node: dom.backend[i],
            kind,
            rect,
            label: self.label(i),
            entries,
            first_shown,
        });
        self.block(BlockKind::Log(index), i, context);
    }

    /// The control on node `i`, made once.
    fn control(&mut self, i: usize, role: Role) -> usize {
        let dom = self.dom;
        let backend = dom.backend[i];
        if let Some(&index) = self.by_node.get(&backend) {
            return index;
        }
        let ax = self.ax.get(backend);
        let kind = dom.attr(i, "type").unwrap_or_default().to_ascii_lowercase();
        let password = dom.name[i] == "INPUT" && kind == "password";
        let role = if password { Role::Password } else { role };
        let name = ax
            .and_then(|n| ax_string(n, "name"))
            .filter(|n| !n.is_empty())
            .or_else(|| dom.attr(i, "aria-label").map(clean))
            .or_else(|| dom.attr(i, "title").map(clean))
            .or_else(|| dom.attr(i, "placeholder").map(clean))
            .or_else(|| {
                let text = clean(&dom.raw_text(i));
                (!text.is_empty()).then_some(text)
            })
            .or_else(|| dom.attr(i, "name").map(clean))
            .unwrap_or_default();
        let options = if dom.name[i] == "SELECT" {
            dom.descendants(i)
                .into_iter()
                .filter(|&d| dom.name[d] == "OPTION")
                .take(MAX_OPTIONS)
                .map(|d| {
                    let in_disabled_group = usize::try_from(dom.parent[d]).ok().is_some_and(|p| {
                        dom.name[p] == "OPTGROUP" && dom.attr(p, "disabled").is_some()
                    });
                    Choice {
                        key: dom.backend[d],
                        label: truncate(
                            &self
                                .ax
                                .get(dom.backend[d])
                                .and_then(|n| ax_string(n, "name"))
                                .filter(|n| !n.is_empty())
                                .or_else(|| dom.attr(d, "label").map(clean))
                                .unwrap_or_else(|| clean(&dom.raw_text(d))),
                            200,
                        ),
                        disabled: dom.attr(d, "disabled").is_some() || in_disabled_group,
                        selected: dom.selected.contains(&d),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let value = if password {
            None
        } else if dom.name[i] == "SELECT" {
            options.iter().find(|o| o.selected).map(|o| o.label.clone())
        } else if matches!(
            role,
            Role::CheckBox | Role::Radio | Role::Switch | Role::Button | Role::Link
        ) {
            None
        } else {
            dom.input_value
                .get(&i)
                .or_else(|| dom.text_value.get(&i))
                .map(|v| truncate(&clean_pre(v), MAX_TEXT))
                .or_else(|| ax.and_then(|n| ax_string(n, "value")))
        };
        let checked = ax
            .and_then(|n| property(n, "checked"))
            .and_then(|v| match v {
                Value::String(s) if s == "mixed" => Some(Check::Mixed),
                Value::String(s) if s == "true" => Some(Check::On),
                Value::String(s) if s == "false" => Some(Check::Off),
                Value::Bool(true) => Some(Check::On),
                Value::Bool(false) => Some(Check::Off),
                _ => None,
            })
            .or_else(|| {
                matches!(role, Role::CheckBox | Role::Radio | Role::Switch).then(|| {
                    if dom.checked.contains(&i) {
                        Check::On
                    } else {
                        Check::Off
                    }
                })
            });
        let disabled = ax
            .and_then(|n| property(n, "disabled"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || dom.attr(i, "disabled").is_some();
        let readonly = ax
            .and_then(|n| property(n, "readonly"))
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || dom.attr(i, "readonly").is_some();
        let range = |attr: &str, prop: &str| {
            number(dom.attr(i, attr)).or_else(|| ax.and_then(|n| property(n, prop)?.as_f64()))
        };
        let href = (role == Role::Link)
            .then(|| {
                ax.and_then(|n| property(n, "url")?.as_str().map(str::to_string))
                    .or_else(|| {
                        dom.attr(i, "href")
                            .map(|h| crate::url::resolve(&dom.url, h))
                    })
            })
            .flatten()
            .map(|h| truncate(&clean(&h), 2000));
        let form = {
            let mut p = dom.parent[i];
            let mut found = None;
            while let Ok(at) = usize::try_from(p) {
                if dom.name[at] == "FORM" {
                    found = Some(at);
                    break;
                }
                p = dom.parent[at];
            }
            found.map(|f| {
                (
                    dom.attr(f, "action").unwrap_or_default().to_string(),
                    dom.attr(f, "method").unwrap_or_default().to_string(),
                )
            })
        };
        let mut control = Control {
            node: backend,
            role: role.clone(),
            name: truncate(&name, 512),
            value,
            checked,
            disabled,
            readonly,
            options,
            min: range("min", "valuemin"),
            max: range("max", "valuemax"),
            step: number(dom.attr(i, "step")),
            href,
            group: (role == Role::Radio)
                .then(|| dom.attr(i, "name").map(clean))
                .flatten(),
            rect: dom.rect(i).unwrap_or_default(),
            from_ax: ax.is_some(),
            icon_only: role == Role::Button
                && if dom.name[i] == "INPUT" {
                    kind == "image"
                } else {
                    clean(&dom.raw_text(i)).trim().is_empty()
                },
            revision: 0,
            state_revision: 0,
        };
        let mut keys: Vec<i64> = control.options.iter().map(|o| o.key).collect();
        keys.sort_unstable();
        let labels: Vec<&str> = control.options.iter().map(|o| o.label.as_str()).collect();
        let meaning = hash(&(
            role.name(),
            &control.name,
            &kind,
            &control.href,
            keys,
            labels,
            form,
            control.min.map(f64::to_bits),
            control.max.map(f64::to_bits),
            control.step.map(f64::to_bits),
            dom.attr(i, "multiple").is_some(),
        ));
        let state = hash(&(
            &control.value,
            control.checked,
            control.selected().map(|o| o.key),
        ));
        let (revision, state_revision) = self.revisions.revise(backend, meaning, state);
        control.revision = revision;
        control.state_revision = state_revision;
        let index = self.page.controls.len();
        self.page.controls.push(control);
        self.by_node.insert(backend, index);
        index
    }

    fn table(&mut self, i: usize, context: &Context) -> Option<Table> {
        let dom = self.dom;
        let mut table = Table::default();
        let mut stack: Vec<usize> = dom.children[i].iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            match dom.name[n].as_str() {
                "TABLE" => continue,
                "CAPTION" => {
                    table.caption = self
                        .inline(n, context)
                        .iter()
                        .map(|r| r.text.as_str())
                        .collect::<String>()
                        .trim()
                        .to_string()
                }
                "TR" => {
                    let mut row = Vec::new();
                    for &cell in &dom.children[n] {
                        let header = dom.name[cell] == "TH";
                        if !(header || dom.name[cell] == "TD") || dom.layout[cell].is_none() {
                            continue;
                        }
                        let mut runs = self.inline(cell, context);
                        tidy(&mut runs);
                        row.push(Cell {
                            runs,
                            header,
                            rect: dom.rect(cell).unwrap_or_default(),
                        });
                    }
                    if !row.is_empty() {
                        table.rows.push(row);
                    }
                }
                _ => stack.extend(dom.children[n].iter().rev()),
            }
        }
        (!table.rows.is_empty()).then_some(table)
    }

    /// The text runs of node `i`, its blocks run together: a cell's.
    fn inline(&mut self, i: usize, context: &Context) -> Vec<Run> {
        let saved_runs = std::mem::take(&mut self.runs);
        let saved_rect = self.rect.take();
        let blocks = self.page.blocks.len();
        let inner = Context {
            kind: BlockKind::Paragraph,
            ..context.clone()
        };
        for &child in &self.dom.children[i] {
            self.walk(child, &inner);
        }
        // Blocks the cell made (its paragraphs, a control) fold back into
        // its text.
        let made: Vec<Block> = self.page.blocks.drain(blocks..).collect();
        let mut runs = Vec::new();
        for block in made {
            match block.kind {
                BlockKind::Control(index) => runs.push(Run {
                    text: format!("[{}] ", self.page.controls[index].name),
                    control: Some(index),
                    ..Run::default()
                }),
                BlockKind::Media(index) => {
                    let media = &self.page.media[index];
                    runs.push(Run {
                        text: media.alt.clone(),
                        picture: Some(Picture {
                            src: media.src.clone(),
                            alt: media.alt.clone(),
                            width: media.rect.width,
                            height: media.rect.height,
                            decorative: crate::adapt::decorative(media),
                        }),
                        ..Run::default()
                    });
                    runs.push(Run::plain(" "));
                }
                _ => {
                    runs.extend(block.runs);
                    runs.push(Run::plain(" "));
                }
            }
        }
        runs.append(&mut self.runs);
        self.runs = saved_runs;
        self.rect = saved_rect;
        runs
    }
}

/// A page's CSS colour as a rich colour for the terminal (`rgb(r,g,b)`),
/// or `None` for a transparent or unreadable one. A colour too dark to
/// read on a dark terminal is lifted toward white, keeping its hue.
pub fn terminal_color(css: &str) -> Option<String> {
    let inner = css
        .trim()
        .strip_prefix("rgba(")
        .or_else(|| css.trim().strip_prefix("rgb("))?
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split([',', ' ', '/'])
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    if parts.len() < 3 || parts.get(3).is_some_and(|a| *a < 0.1) {
        return None;
    }
    let channel = |v: f64| v.clamp(0.0, 255.0);
    let (mut r, mut g, mut b) = (channel(parts[0]), channel(parts[1]), channel(parts[2]));
    let luma = (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0;
    if luma < 0.35 {
        // Mix toward white until it reads: at most two thirds of the way.
        let t = ((0.35 - luma) / 0.35 * 0.66).min(0.66);
        r += (255.0 - r) * t;
        g += (255.0 - g) * t;
        b += (255.0 - b) * t;
    }
    Some(format!(
        "rgb({},{},{})",
        r.round() as u8,
        g.round() as u8,
        b.round() as u8
    ))
}

/// The runs of a live log's entry as the engine's isolated world reads it
/// (`[{t, c, b, i}, {img: {src, alt, w, h, hidden}}, …]`, untrusted):
/// cleaned, with whitespace collapsed as a browser does.
pub fn live_runs(value: &Value, base: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    for part in value.as_array().into_iter().flatten().take(400) {
        if let Some(img) = part.get("img") {
            let alt = truncate(&clean_inline(img["alt"].as_str().unwrap_or_default()), 200);
            let width = img["w"].as_f64().unwrap_or(0.0);
            let height = img["h"].as_f64().unwrap_or(0.0);
            let hidden = img["hidden"].as_bool().unwrap_or(false);
            let empty_alt = img["alt"].as_str().is_some_and(|a| a.trim().is_empty());
            runs.push(Run {
                text: alt.trim().to_string(),
                picture: Some(Picture {
                    src: truncate(
                        &clean(&crate::url::resolve(
                            base,
                            img["src"].as_str().unwrap_or_default(),
                        )),
                        2000,
                    ),
                    alt: alt.trim().to_string(),
                    width,
                    height,
                    decorative: hidden
                        || empty_alt
                        || width < crate::adapt::DECORATIVE_PX
                        || height < crate::adapt::DECORATIVE_PX,
                }),
                ..Run::default()
            });
            runs.push(Run::plain(" "));
            continue;
        }
        let text = part["t"].as_str().unwrap_or_default();
        let text = if text == "\n" || text == "
" {
            "
".to_string()
        } else {
            clean_inline(text)
        };
        if text.is_empty() {
            continue;
        }
        runs.push(Run {
            text: truncate(&text, MAX_TEXT),
            bold: part["b"].as_bool().unwrap_or(false),
            italic: part["i"].as_bool().unwrap_or(false),
            color: part["c"].as_str().and_then(terminal_color),
            ..Run::default()
        });
    }
    // Merge neighbours of one style, as the walker does.
    let mut merged: Vec<Run> = Vec::new();
    for run in runs {
        match merged.last_mut() {
            Some(last)
                if last.picture.is_none()
                    && run.picture.is_none()
                    && last.bold == run.bold
                    && last.italic == run.italic
                    && last.color == run.color =>
            {
                last.text.push_str(&run.text)
            }
            _ => merged.push(run),
        }
    }
    tidy(&mut merged);
    merged
}

/// A form element's role when the AX tree has none for it.
fn dom_role(dom: &Dom, i: usize) -> Option<Role> {
    match dom.name[i].as_str() {
        "INPUT" => {
            let kind = dom.attr(i, "type").unwrap_or("text").to_ascii_lowercase();
            Some(match kind.as_str() {
                "hidden" => return None,
                "checkbox" => Role::CheckBox,
                "radio" => Role::Radio,
                "range" => Role::Slider,
                "number" => Role::SpinButton,
                "search" => Role::SearchBox,
                "password" => Role::Password,
                "button" | "submit" | "reset" | "image" => Role::Button,
                _ => Role::TextBox,
            })
        }
        "SELECT" => Some(Role::ComboBox),
        "TEXTAREA" => Some(Role::TextBox),
        "BUTTON" => Some(Role::Button),
        "A" if dom.attr(i, "href").is_some() => Some(Role::Link),
        _ => None,
    }
}

/// Collapse whitespace across runs as a browser does: one space between
/// words, none at the ends; line breaks of the page stay.
fn tidy(runs: &mut Vec<Run>) {
    let mut space = true;
    for run in runs.iter_mut() {
        let mut text = String::with_capacity(run.text.len());
        for c in run.text.chars() {
            if c == '\n' {
                while text.ends_with(' ') {
                    text.pop();
                }
                text.push('\n');
                space = true;
            } else if c == ' ' {
                if !space {
                    text.push(' ');
                    space = true;
                }
            } else {
                text.push(c);
                space = false;
            }
        }
        run.text = text;
    }
    while let Some(last) = runs.last_mut() {
        let trimmed = last.text.trim_end_matches([' ', '\n']).len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            runs.pop();
        } else {
            break;
        }
    }
    runs.retain(|r| !r.text.is_empty());
    if let Some(first) = runs.first_mut() {
        first.text = first.text.trim_start_matches(['\n']).to_string();
    }
}

/// The page in `snapshot`, its control revisions kept in `revisions`.
pub fn build(snapshot: &PageSnapshot, revisions: &mut Revisions) -> Page {
    build_with(snapshot, revisions, &HashSet::new())
}

/// [`build`], with the scroll boxes the page has appended entries to,
/// as the engine saw them: they are live logs too.
pub fn build_with(
    snapshot: &PageSnapshot,
    revisions: &mut Revisions,
    appended: &HashSet<NodeId>,
) -> Page {
    let dom = Dom::parse(&snapshot.dom);
    let ax = Ax::parse(&snapshot.ax);
    let root = dom.backend.first().copied().unwrap_or(-1);
    let document = revisions.document(&dom.url, root);
    let mut walker = Walker {
        dom: &dom,
        ax: &ax,
        page: Page {
            url: truncate(&clean(&dom.url), 4096),
            title: truncate(&clean(&dom.title), 500),
            document,
            width: dom.width,
            height: dom.height,
            has_ax: !ax.by_node.is_empty(),
            ..Page::default()
        },
        runs: Vec::new(),
        rect: None,
        marker: None,
        by_node: HashMap::new(),
        revisions,
        appended,
    };
    let top = Context {
        kind: BlockKind::Paragraph,
        frame: Rect::default(),
        node: root,
        bold: false,
        italic: false,
        code: false,
        pre: false,
        quote: 0,
        depth: 0,
        control: None,
        silent: false,
        color: String::new(),
        region: None,
        aria_hidden: false,
    };
    if !dom.parent.is_empty() {
        walker.walk(0, &top);
    }
    walker.flush(&top);
    let mut page = walker.page;
    // A landmark with a field or a live log in it is not only links: it
    // never collapses.
    for block in &page.blocks {
        let busy = match &block.kind {
            BlockKind::Log(_) => true,
            BlockKind::Control(c) => page.controls[*c].role.is_text(),
            _ => false,
        };
        if let (true, Some(r)) = (busy, block.region) {
            page.regions[r].collapsible = false;
        }
    }
    page
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A snapshot as Chrome shapes one, from a list of nodes: (parent,
    /// type, name, value, attributes, layout: (display, bounds), text).
    struct Fake {
        strings: Vec<String>,
        parent: Vec<i64>,
        kind: Vec<i64>,
        name: Vec<i64>,
        value: Vec<i64>,
        backend: Vec<i64>,
        attrs: Vec<Vec<i64>>,
        layout: Vec<i64>,
        styles: Vec<Vec<i64>>,
        bounds: Vec<Vec<f64>>,
        text: Vec<i64>,
        input_value: (Vec<i64>, Vec<i64>),
    }

    impl Fake {
        fn new() -> Fake {
            let mut fake = Fake {
                strings: Vec::new(),
                parent: Vec::new(),
                kind: Vec::new(),
                name: Vec::new(),
                value: Vec::new(),
                backend: Vec::new(),
                attrs: Vec::new(),
                layout: Vec::new(),
                styles: Vec::new(),
                bounds: Vec::new(),
                text: Vec::new(),
                input_value: (Vec::new(), Vec::new()),
            };
            fake.node(-1, 9, "#document", None, &[], Some("block"));
            fake
        }

        fn s(&mut self, text: &str) -> i64 {
            if let Some(i) = self.strings.iter().position(|s| s == text) {
                return i as i64;
            }
            self.strings.push(text.to_string());
            self.strings.len() as i64 - 1
        }

        /// Add a node; `display` `None` means no layout box.
        fn node(
            &mut self,
            parent: i64,
            kind: i64,
            name: &str,
            value: Option<&str>,
            attrs: &[(&str, &str)],
            display: Option<&str>,
        ) -> i64 {
            let index = self.parent.len() as i64;
            self.parent.push(parent);
            self.kind.push(kind);
            let n = self.s(name);
            self.name.push(n);
            let v = value.map_or(-1, |v| self.s(v));
            self.value.push(v);
            self.backend.push(100 + index);
            let a = attrs
                .iter()
                .flat_map(|(k, v)| [*k, *v])
                .collect::<Vec<_>>()
                .into_iter()
                .map(|t| self.s(t))
                .collect();
            self.attrs.push(a);
            if let Some(display) = display {
                self.layout.push(index);
                let style = vec![
                    self.s(display),
                    self.s("visible"),
                    self.s(""),
                    self.s(""),
                    self.s("400"),
                    self.s("normal"),
                    self.s(""),
                    self.s("normal"),
                    self.s("1"),
                ];
                self.styles.push(style);
                self.bounds
                    .push(vec![0.0, index as f64 * 20.0, 100.0, 18.0]);
                let t = if kind == 3 { v } else { -1 };
                self.text.push(t);
            }
            index
        }

        fn el(&mut self, parent: i64, name: &str, attrs: &[(&str, &str)], display: &str) -> i64 {
            self.node(parent, 1, name, None, attrs, Some(display))
        }

        fn text(&mut self, parent: i64, text: &str) -> i64 {
            self.node(parent, 3, "#text", Some(text), &[], Some("inline"))
        }

        fn snapshot(&self) -> Value {
            json!({
                "strings": self.strings,
                "documents": [{
                    "documentURL": 0, "title": 0,
                    "contentWidth": 1000, "contentHeight": 800,
                    "nodes": {
                        "parentIndex": self.parent, "nodeType": self.kind,
                        "nodeName": self.name, "nodeValue": self.value,
                        "backendNodeId": self.backend, "attributes": self.attrs,
                        "inputValue": {"index": self.input_value.0, "value": self.input_value.1},
                        "inputChecked": {"index": []}, "optionSelected": {"index": []},
                    },
                    "layout": {"nodeIndex": self.layout, "styles": self.styles,
                               "bounds": self.bounds, "text": self.text},
                }],
            })
        }

        fn page(&self, ax: Value) -> Page {
            let snapshot = PageSnapshot {
                dom: self.snapshot(),
                ax,
            };
            build(&snapshot, &mut Revisions::new())
        }
    }

    #[test]
    fn headings_paragraphs_and_hidden_text() {
        let mut f = Fake::new();
        f.s("about:x");
        let body = f.el(0, "BODY", &[], "block");
        let h1 = f.el(body, "H1", &[], "block");
        f.text(h1, "Hi");
        let p = f.el(body, "P", &[], "block");
        f.text(p, "One ");
        let b = f.el(p, "B", &[], "inline");
        f.text(b, "two");
        // display: none: no layout box for it or its text.
        let gone = f.node(body, 1, "P", None, &[], None);
        f.node(gone, 3, "#text", Some("hidden"), &[], None);
        let page = f.page(Value::Null);
        let texts: Vec<_> = page
            .blocks
            .iter()
            .map(|b| (b.kind.clone(), b.text()))
            .collect();
        assert_eq!(
            texts,
            [
                (BlockKind::Heading(1), "Hi".to_string()),
                (BlockKind::Paragraph, "One two".to_string())
            ]
        );
        assert!(page.blocks[1].runs[1].bold);
    }

    #[test]
    fn lists_nest_and_keep_their_markers() {
        let mut f = Fake::new();
        let ul = f.el(0, "UL", &[], "block");
        let li = f.el(ul, "LI", &[], "list-item");
        f.node(li, 1, "::marker", Some("• "), &[], Some("inline-block"));
        let marker = f.text.len() - 1;
        let m = f.s("• ");
        f.text[marker] = m;
        f.text(li, "Outer");
        let inner = f.el(li, "UL", &[], "block");
        let li2 = f.el(inner, "LI", &[], "list-item");
        f.text(li2, "Inner");
        let page = f.page(Value::Null);
        assert_eq!(
            page.blocks
                .iter()
                .map(|b| (b.kind.clone(), b.text()))
                .collect::<Vec<_>>(),
            [
                (
                    BlockKind::ListItem {
                        depth: 1,
                        marker: "•".into()
                    },
                    "Outer".into()
                ),
                (
                    BlockKind::ListItem {
                        depth: 2,
                        marker: String::new()
                    },
                    "Inner".into()
                ),
            ]
        );
    }

    #[test]
    fn a_password_value_never_enters_the_model() {
        let mut f = Fake::new();
        let form = f.el(0, "FORM", &[], "block");
        let input = f.el(
            form,
            "INPUT",
            &[("type", "password"), ("value", "hunter2")],
            "inline-block",
        );
        f.input_value = (vec![input], vec![f.s("hunter2")]);
        let page = f.page(json!({"nodes": [
            {"nodeId": "1", "role": {"value": "textbox"}, "name": {"value": "Secret"},
             "value": {"value": "•••••••"}, "backendDOMNodeId": 100 + input}
        ]}));
        assert_eq!(page.controls.len(), 1);
        assert_eq!(page.controls[0].role, Role::Password);
        assert_eq!(page.controls[0].value, None);
        assert!(!format!("{page:?}").contains("hunter2"));
    }

    #[test]
    fn options_are_keyed_by_their_node_not_their_value() {
        let mut f = Fake::new();
        let select = f.el(0, "SELECT", &[("name", "m")], "inline-block");
        let a = f.node(select, 1, "OPTION", None, &[("value", "same")], None);
        f.node(a, 3, "#text", Some("First"), &[], None);
        let b = f.node(
            select,
            1,
            "OPTION",
            None,
            &[("value", "same"), ("disabled", "")],
            None,
        );
        f.node(b, 3, "#text", Some("Off"), &[], None);
        let page = f.page(Value::Null);
        let options = &page.controls[0].options;
        assert_eq!(options.len(), 2);
        assert_ne!(options[0].key, options[1].key);
        assert_eq!(
            (options[0].label.as_str(), options[1].disabled),
            ("First", true)
        );
    }

    #[test]
    fn a_meaning_change_moves_the_revision_and_stales_the_binding() {
        let make = |name: &str| {
            let mut f = Fake::new();
            let button = f.el(0, "BUTTON", &[], "inline-block");
            f.text(button, name);
            PageSnapshot {
                dom: f.snapshot(),
                ax: Value::Null,
            }
        };
        let mut revisions = Revisions::new();
        let first = build(&make("Send"), &mut revisions);
        let binding = first.controls[0].binding(&first);
        let same = build(&make("Send"), &mut revisions);
        assert!(same.check(&binding).is_ok());
        let renamed = build(&make("Delete everything"), &mut revisions);
        assert_eq!(
            renamed.check(&binding).unwrap_err(),
            "Stale control: its meaning changed."
        );
        assert_eq!(renamed.controls[0].revision, 2);
    }

    #[test]
    fn controls_come_from_the_accessibility_tree() {
        let mut f = Fake::new();
        let div = f.el(0, "DIV", &[("role", "button")], "inline-block");
        f.text(div, "ARIA action");
        let page = f.page(json!({"nodes": [
            {"nodeId": "1", "role": {"value": "button"}, "name": {"value": "ARIA\u{1b}[2J action"},
             "backendDOMNodeId": 100 + div}
        ]}));
        assert_eq!(page.controls[0].role, Role::Button);
        assert_eq!(page.controls[0].name, "ARIA[2J action");
        assert!(page.controls[0].from_ax);
        assert_eq!(page.find("BUTTON", "aria[2j action").len(), 1);
        assert!(matches!(page.blocks[0].kind, BlockKind::Control(0)));
    }

    #[test]
    fn whitespace_collapses_across_runs() {
        let mut runs = vec![Run::plain(" a  "), Run::plain("  b\n"), Run::plain(" c ")];
        tidy(&mut runs);
        assert_eq!(plain(&runs), "a b\nc");
    }
}

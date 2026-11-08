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
//! - **Cleaning**: every string is cleaned ([`crate::clean()`]) here, so
//!   nothing in the model can carry a control character to the terminal.

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use serde_json::Value;

pub(crate) mod source;
pub use source::SourcePage;
use source::{Extraction, Origins, RunLocation};

use crate::clean::{clean, clean_inline, clean_pre, truncate};
use crate::engine::{NodeId, PageSnapshot, Rect};

/// At most this many blocks are read from one page.
pub const MAX_BLOCKS: usize = 20_000;
/// At most this many characters of one text.
const MAX_TEXT: usize = 8192;
/// The gap, in CSS pixels, between two text boxes on one line that keeps
/// a space between their texts: half a cell.
const GAP: f64 = 4.0;
/// At most this many options of one select.
const MAX_OPTIONS: usize = 500;

/// Where `overflow-x` is in [`SNAPSHOT_STYLES`](crate::cdp::SNAPSHOT_STYLES).
const SNAPSHOT_OVERFLOW_X: usize = 15;
/// Optional appended fact; older snapshots have no clip-margin proof.
const SNAPSHOT_OVERFLOW_CLIP_MARGIN: usize = 16;

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
    /// The page's colour for it, as CSS (`rgb(255, 0, 0)`): the
    /// [page colours](crate::adapt::Rule::PageColors) rule decides how it
    /// shows (adapted, a link is drawn as a link; off, in its own colour).
    pub color: Option<String>,
    /// A small picture in the line (an emote, a badge): the text is its
    /// alt text, which stands in where pictures are not drawn.
    pub picture: Option<Picture>,
    /// Which of a pair of [screen-reader twins](Twin) it is, if either:
    /// the [screen-reader twins](crate::adapt::Rule::ScreenReaderTwins)
    /// rule draws one of the two.
    pub twin: Option<Twin>,
}

/// One of two sibling runs saying the same thing two ways: the visually
/// hidden text a page writes for screen readers (`$29.99`) and the parts
/// it shows instead, hidden from assistive technology (`$`, `29`, `99`
/// drawn apart, read together as `$2999`). Only a pair whose digits agree
/// is a pair, so a hidden hint ("Use the Right Arrow Key…") never is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Twin {
    /// The visually hidden text, for screen readers.
    Spoken,
    /// The `aria-hidden` parts shown in its place.
    Shown,
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
    /// A value in a range the page states: an index into
    /// [`Page::indicators`].
    Indicator(usize),
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
    /// Where it is in the page's stack of boxes: its fixed or sticky
    /// layer, whether a box painted above it hides it, the picture it lies
    /// on.
    pub stack: Stack,
}

/// A block's place in the page's stack of boxes, from computed `position`
/// and the snapshot's paint orders.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stack {
    /// The fixed or sticky box it is in: an index into [`Page::layers`].
    pub layer: Option<usize>,
    /// An opaque box or a picture painted above it covers its whole box:
    /// Chrome does not show it (a player's hidden error panel under an
    /// interstitial).
    pub covered: bool,
    /// It lies inside a picture's box and is painted above it (a duration
    /// badge on a thumbnail, art over a player): an index into
    /// [`Page::media`].
    pub over: Option<usize>,
    /// The sideways-scrolling shelf it is in (its innermost): an index
    /// into [`Page::shelves`]. Only that shelf's box clips it.
    pub shelf: Option<usize>,
}

/// Where a fixed or sticky layer is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// In the page's flow, at [`Layer::top`]: a sticky box at its static
    /// place, a fixed box too tall to pin where it is on screen.
    Flow,
    /// On the strip above the page (a top bar), at [`Layer::top`] in it.
    Pinned,
    /// On the strip under the page, above the status bar (a cookie
    /// banner, a sign-up bar), at [`Layer::top`] in the viewport.
    Docked,
    /// Wholly outside the viewport (a fixed menu slid away): not drawn.
    Off,
}

/// A box the page takes out of its flow with `position: fixed` or
/// `sticky`, and every block in it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layer {
    pub node: NodeId,
    /// `fixed`; else `sticky`.
    pub fixed: bool,
    /// Its box as the snapshot reports it: where it is on screen now, in
    /// the document's pixels.
    pub rect: Rect,
    /// Where it is drawn: its top in the document ([`Place::Flow`]) or in
    /// the viewport (pinned and docked).
    pub top: f64,
    pub place: Place,
}

/// A source-declared dialog or an opaque, large fixed visual layer.
/// `dialog` and `modal` reflect source semantics, never geometry alone.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceOverlay {
    pub node: NodeId,
    pub layer: usize,
    pub dialog: bool,
    pub modal: bool,
    pub name: String,
    /// Source surface background when it is proven fully opaque.
    pub background: Option<String>,
    /// Chrome paint order for compositing sibling dialogs.
    pub paint_order: i64,
    /// Ordinary source blocks painted above this surface, in paint order.
    pub foreground: Vec<NodeId>,
}

impl Block {
    /// Its text, without styles.
    pub fn text(&self) -> String {
        plain(&self.runs)
    }
}

/// The text of `runs`: of a pair of [twins](Twin), the spoken one.
pub fn plain(runs: &[Run]) -> String {
    runs.iter()
        .filter(|r| r.twin != Some(Twin::Shown))
        .map(|r| r.text.as_str())
        .collect()
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
    /// An icon-only button's face, when its name or its icon's name is a
    /// known one: `⋯` for "More actions", `✕` for "Close", `🔍` for
    /// "Search". Its name stays its name, in hints and lists.
    pub glyph: Option<String>,
    /// Laid out but transparent until hovered (`opacity: 0` on it or an
    /// ancestor: a card's "More actions"): not drawn, as Chrome draws it,
    /// but on the page's lists (site keys) to bind and use.
    pub hover_only: bool,
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
    /// Its picture's bytes once read: the original file the page loaded,
    /// else a still of Chrome's pixels (PNG).
    pub still: Option<Arc<[u8]>>,
    /// Its picture, decoded from [`still`](Self::still) on the tab's
    /// worker: what the projection and reader mode draw. An animated GIF
    /// carries its frames.
    pub picture: Option<rich_intuituive::Picture>,
    /// Whether a source figure caption supplies the complete normalized alt.
    /// Reader view always retains the original alternative text.
    pub captioned: bool,
    /// A video's or an audio's playback, read from the element.
    pub playback: Option<crate::engine::Playback>,
    /// A video's poster picture's address, cleaned; empty when it has
    /// none.
    pub poster: String,
    /// Whether the element asks for the browser's own controls.
    pub controls: bool,
    /// Whether a video or audio plays by itself (`autoplay`).
    pub autoplay: bool,
    /// It has an `alt` attribute, and it is empty: decorative by the
    /// author's word.
    pub empty_alt: bool,
    /// It is hidden from assistive technology (`aria-hidden`, or a
    /// `presentation` role), itself or by an ancestor.
    pub aria_hidden: bool,
    /// It is inside a heading, a link, a button or a label: a picture
    /// that stands for words.
    pub in_label: bool,
    /// What its content was when read, as a hash: for an inline `<svg>`,
    /// of its subtree's element names, attributes and text, so a drawing
    /// a script changes (a dial's arc, needle and number) is captured
    /// again; 0 for every other kind (a canvas changes no DOM: the tab
    /// watches its pixels instead).
    pub revision: u64,
    /// The words it draws: an inline `<svg>`'s `<text>` (with its
    /// `<tspan>`s), in document order, cleaned, so a dial's number reads
    /// as text in reader mode and `--dump`; empty for every other kind.
    pub text: String,
    /// An `<img>` that has no source yet (a lazy thumbnail, transparent
    /// until its picture loads): a placeholder in its box, with its alt
    /// text or title, until it has one.
    pub pending: bool,
}

/// What an [`Indicator`] is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IndicatorKind {
    /// `<meter>` or `role=meter`: a measurement in a known range.
    Meter,
    /// `<progress>` or `role=progressbar` with a value.
    Progress,
    /// A progress with no value: busy, for an unknown time.
    Busy,
    /// A score out of a few (`role=img` named "4.5 out of 5 stars").
    Rating,
}

/// A value in a range the page states, not a control (the user does not
/// change it): a `<meter>`, a `<progress>`, `role=meter` or
/// `role=progressbar` with `aria-value*`, a rating. Read again at every
/// reading, so a moving bar moves.
#[derive(Clone, Debug, PartialEq)]
pub struct Indicator {
    /// The node it is on.
    pub node: NodeId,
    /// What it is.
    pub kind: IndicatorKind,
    /// Its accessible name, or its `<label for>`'s text; cleaned.
    pub label: String,
    /// `value` or `aria-valuenow`, within `min..=max`; `None` when busy.
    pub value: Option<f64>,
    /// The low end of its range: `min`, `aria-valuemin`, else 0.
    pub min: f64,
    /// The high end of its range, never below `min`: `max`,
    /// `aria-valuemax`, else 1 for a `<meter>` or a `<progress>` and 100
    /// for an ARIA one; a rating's scale (5).
    pub max: f64,
    /// `aria-valuetext`, or the text it holds; cleaned.
    pub text: Option<String>,
    /// Its box, in CSS pixels of the document.
    pub rect: Rect,
    /// A `<meter>`'s `low`, `high` and `optimum`: which colour its value
    /// is drawn in, as browsers draw it.
    pub bands: Option<(f64, f64, f64)>,
    /// The media region it also is (a `<canvas>` or `<svg>` given the
    /// role): an index into [`Page::media`]. The
    /// [indicators](crate::adapt::Rule::Indicators) rule draws the
    /// indicator in its place.
    pub media: Option<usize>,
    /// The text just before it on its line ends with its label (`Download
    /// <span role=progressbar aria-label=Download>`): drawn without it.
    pub label_in_text: bool,
    /// Text comes before it on its line, in the same block.
    pub after_text: bool,
}

impl Indicator {
    /// Its value's share of its range, 0 to 1; `None` when busy.
    pub fn share(&self) -> Option<f64> {
        let span = self.max - self.min;
        self.value
            .map(|v| {
                if span > 0.0 {
                    (v - self.min) / span
                } else {
                    1.0
                }
            })
            .map(|s| s.clamp(0.0, 1.0))
    }

    /// What it says, without its label: its value text, else its share as
    /// a percentage (`72%`), `busy` for a busy one, the number of a rating
    /// (`4.5`).
    pub fn readout(&self) -> String {
        if let Some(text) = self.text.as_deref().filter(|t| !t.trim().is_empty()) {
            return text.to_string();
        }
        match (self.kind, self.value) {
            (IndicatorKind::Rating, Some(value)) => number_text(value),
            (_, None) | (IndicatorKind::Busy, _) => "busy".into(),
            _ => format!("{:.0}%", self.share().unwrap_or(0.0) * 100.0),
        }
    }

    /// What a reader says of it: its label, then its [readout](Self::readout)
    /// (a rating's label says its number already).
    pub fn said(&self) -> String {
        if self.kind == IndicatorKind::Rating && !self.label.is_empty() {
            return self.label.clone();
        }
        [self.label.clone(), self.readout()]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A number as people write it: `4.5`, `4`, `287.4`; at most two decimals.
pub fn number_text(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A rating in an accessible name, as (value, out of): "4.5 out of 5
/// stars", "Rated 4 out of 5", "4.5 / 5", "4.5/5 stars". `None` when it
/// is not one: no pair of numbers, the value above the scale, or a scale
/// of more than 10 (a count, not stars).
pub fn rating_of(name: &str) -> Option<(f64, f64)> {
    let lower = name.to_ascii_lowercase();
    let (before, after) = lower
        .split_once(" out of ")
        .or_else(|| lower.split_once('/'))?;
    let last_number = |text: &str| {
        text.split(|c: char| c.is_whitespace() || c == '(')
            .rfind(|w| !w.is_empty())
            .and_then(|w| w.replace(',', ".").parse::<f64>().ok())
    };
    let first_number = |text: &str| {
        text.split(|c: char| c.is_whitespace() || c == ')')
            .find(|w| !w.is_empty())
            .and_then(|w| w.replace(',', ".").parse::<f64>().ok())
    };
    let value = last_number(before.trim())?;
    let scale = first_number(after.trim())?;
    (value.is_finite()
        && scale.is_finite()
        && scale > 0.0
        && scale <= 10.0
        && (0.0..=scale).contains(&value))
    .then_some((value, scale))
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

/// A source grid, wrapping flex row, or two named video/log sections in
/// a flex container: its direct items' boxes in source order.
#[derive(Clone, Debug, PartialEq)]
pub struct Grid {
    pub node: NodeId,
    pub rect: Rect,
    pub items: Vec<Rect>,
}

/// A shelf: a box that scrolls sideways (`overflow-x: auto` or `scroll`,
/// wider inside than out) over three or more alike items side by side,
/// a shop's "Customers also viewed", a row of videos. The projection
/// draws the items in its box, as Chrome shows them, with a strip under
/// it that pages it (`‹ 1–3 of 6 ›`).
#[derive(Clone, Debug, PartialEq)]
pub struct Shelf {
    /// The scrolling box's node.
    pub node: NodeId,
    /// Its box, in CSS pixels of the document.
    pub rect: Rect,
    /// Its items' boxes, in order, where Chrome lays them out at its
    /// scroll now: an item scrolled away is outside [`rect`](Self::rect).
    pub items: Vec<Rect>,
    /// How far it is scrolled sideways, in CSS pixels.
    pub scroll_left: f64,
    /// How wide its content is, in CSS pixels (`scrollWidth`).
    pub scroll_width: f64,
    /// How wide what it shows is, in CSS pixels (`clientWidth`).
    pub client_width: f64,
    /// Its accessible name, if it has one.
    pub label: String,
}

impl Shelf {
    /// Whether item `index` is in view: at least half of it in the box.
    pub fn shows(&self, index: usize) -> bool {
        self.items.get(index).is_some_and(|item| {
            let middle = item.x + item.width / 2.0;
            middle >= self.rect.x && middle <= self.rect.right()
        })
    }

    /// The items in view, as (first, how many); (0, 0) when none is.
    pub fn shown(&self) -> (usize, usize) {
        let shown: Vec<usize> = (0..self.items.len()).filter(|&i| self.shows(i)).collect();
        match (shown.first(), shown.last()) {
            (Some(&first), Some(&last)) => (first, last - first + 1),
            _ => (0, 0),
        }
    }

    /// Whether a box at `rect` is in the shelf (within its rows) but
    /// scrolled out of its box: in an item not in view, or, outside every
    /// item, with its middle outside the box.
    pub fn hides(&self, rect: &Rect) -> bool {
        let (middle_x, middle_y) = (rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
        if middle_y < self.rect.y || middle_y > self.rect.bottom() {
            return false;
        }
        match self
            .items
            .iter()
            .position(|item| middle_x >= item.x && middle_x <= item.right())
        {
            Some(index) => !self.shows(index),
            None => middle_x < self.rect.x || middle_x > self.rect.right(),
        }
    }

    /// The shelf as a screen `right` CSS pixels wide shows it: its box
    /// cut at that edge, so an item past the screen counts as out of view
    /// (a page laid out wider than the terminal), and a page of it is
    /// what the screen shows.
    pub fn clipped(&self, right: f64) -> Shelf {
        let mut shelf = self.clone();
        shelf.rect.width = (right.min(self.rect.right()) - self.rect.x).max(0.0);
        shelf
    }

    /// Whether it can scroll back, and on.
    pub fn can_page(&self) -> (bool, bool) {
        (
            self.scroll_left > 0.5,
            self.scroll_left + self.client_width < self.scroll_width - 0.5,
        )
    }
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
    /// The page's own background colour, as CSS: its root's, else its
    /// body's, as browsers paint the canvas; `None` when both are
    /// transparent (the page sets none).
    pub background: Option<String>,
    /// Its live logs.
    pub logs: Vec<Log>,
    /// The small pictures in its lines ([`Run::picture`]: emotes, badges,
    /// avatars), decoded on the tab's worker, by source address.
    pub inline_pictures: HashMap<String, rich_intuituive::Picture>,
    /// Its navigation and complementary landmarks.
    pub regions: Vec<Region>,
    /// Its grids and wrapping flex rows.
    pub grids: Vec<Grid>,
    /// Its meters, progress bars and ratings.
    pub indicators: Vec<Indicator>,
    /// Its boxes that scroll sideways over alike items ([`Shelf`]).
    pub shelves: Vec<Shelf>,
    /// Its fixed and sticky boxes ([`Stack::layer`]).
    pub layers: Vec<Layer>,
    /// Visible source dialogs and opaque large fixed layers.
    pub overlays: Vec<SourceOverlay>,
    /// How far down the page Chrome was scrolled when it was read, in CSS
    /// pixels.
    pub scroll_y: f64,
    /// The viewport's height in CSS pixels (0 when the snapshot does not
    /// say).
    pub view_height: f64,
}

impl Page {
    /// Source-declared dialogs, distinct from purely visual fixed layers.
    pub fn dialogs(&self) -> impl Iterator<Item = &SourceOverlay> {
        self.overlays.iter().filter(|overlay| overlay.dialog)
    }

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
                BlockKind::Indicator(i) => out.push(self.indicators[*i].said()),
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
    /// Each layout object's scroll (`scrollRects`: left, top, width,
    /// height) and client box (`clientRects`); empty when the snapshot
    /// has none.
    scrolls: Vec<Rect>,
    clients: Vec<Rect>,
    /// Each node's laid-out text: from the first of its layout objects
    /// that has any (a list marker's is on its second).
    text: Vec<Option<String>>,
    input_value: HashMap<usize, String>,
    text_value: HashMap<usize, String>,
    checked: HashSet<usize>,
    selected: HashSet<usize>,
    source: HashMap<usize, String>,
    /// Whether each node is visually hidden by itself or an ancestor
    /// (see [`transparent`] and [`veils`]): its text is not drawn.
    concealed: Vec<bool>,
    /// Concealed by `opacity: 0` on it or an ancestor ([`transparent`]).
    faded: Vec<bool>,
    /// Concealed another way ([`veils`]): clipped or shrunk away.
    veiled: Vec<bool>,
    /// This node alone is proven outside an ordinary overflow clip.
    /// Never inherited: positioned descendants can escape that clip.
    overflow_clipped: Vec<bool>,
    /// The [screen-reader twins](Twin): each element of a pair.
    twins: HashMap<usize, (Twin, usize)>,
    /// Elements by their `id`, the first of each.
    ids: HashMap<String, usize>,
    /// Each layout object's paint order (`paintOrders`): boxes painted
    /// together share one; empty when the snapshot has none.
    paint: Vec<i64>,
    scroll_y: f64,
    /// The viewport's height: the document node's box.
    view_height: f64,
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
        let rects = |key: &str| -> Vec<Rect> {
            layout_tree[key]
                .as_array()
                .map(|all| {
                    all.iter()
                        .map(|b| {
                            let at = |i: usize| b.get(i).and_then(Value::as_f64).unwrap_or(0.0);
                            Rect::new(at(0), at(1), at(2), at(3))
                        })
                        .collect()
                })
                .unwrap_or_default()
        };
        let bounds = rects("bounds");
        let (scrolls, clients) = (rects("scrollRects"), rects("clientRects"));
        let mut dom = Dom {
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
            scrolls,
            clients,
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
            concealed: Vec::new(),
            faded: Vec::new(),
            veiled: Vec::new(),
            overflow_clipped: Vec::new(),
            twins: HashMap::new(),
            ids: HashMap::new(),
            paint: ints(&layout_tree["paintOrders"]),
            scroll_y: document["scrollOffsetY"].as_f64().unwrap_or(0.0),
            view_height: 0.0,
        };
        dom.view_height = dom.rect(0).map_or(0.0, |r| r.height);
        // Parents come before their children in a snapshot's order; a
        // parent after its child is read as not concealing.
        let (mut faded, mut veiled) = (vec![false; n], vec![false; n]);
        for i in 0..n {
            let parent = usize::try_from(dom.parent[i]).ok().filter(|&p| p < i);
            faded[i] = parent.is_some_and(|p| faded[p]) || transparent(&dom, i);
            veiled[i] = parent.is_some_and(|p| veiled[p]) || veils(&dom, i);
        }
        dom.concealed = faded.iter().zip(&veiled).map(|(f, v)| *f || *v).collect();
        dom.faded = faded;
        dom.veiled = veiled;
        dom.overflow_clipped = (0..n).map(|i| overflow_clipped(&dom, i)).collect();
        for i in 0..n {
            if let Some(id) = dom.attr(i, "id").filter(|id| !id.is_empty()) {
                dom.ids.entry(id.to_string()).or_insert(i);
            }
        }
        dom.twins = twins(&dom);
        dom
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

    /// Node `i`'s scroll (left, top, width, height) and client box, when
    /// the snapshot has them.
    fn scroll(&self, i: usize) -> Option<(Rect, Rect)> {
        let k = self.layout[i]?;
        Some((*self.scrolls.get(k)?, *self.clients.get(k)?))
    }

    fn is_element(&self, i: usize) -> bool {
        self.kind[i] == 1
    }

    /// Whether a laid-out node shows: not hidden, transparent, or fully clipped.
    fn shows(&self, i: usize) -> bool {
        self.layout[i].is_some()
            && !self.overflow_clipped[i]
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
                || theirs
                    .iter()
                    .any(|&g| self.style(g, 0).starts_with("inline"))
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

/// Whether a text alternative is a script's value rendered as text, not
/// words: `alt="true"` from a boolean bound to the attribute (a video
/// site's thumbnails), `undefined`, `null`, `NaN`, `[object Object]`. It
/// says nothing, so the picture is taken to have no text alternative.
fn script_value(text: &str) -> bool {
    matches!(
        text.trim(),
        "true" | "false" | "null" | "undefined" | "NaN" | "[object Object]"
    )
}

/// Whether element `i` conceals what is in it from sight, though it is
/// laid out (text for screen readers only, a tooltip waiting for a hover):
/// - it is fully transparent (`opacity: 0`, which its children do not
///   inherit as a computed style);
/// - it is positioned and clipped to no area (`clip: rect(0 0 0 0)`, or
///   `rect(1px, 1px, 1px, 1px)`);
/// - its `clip-path` is an inset of half its size or more
///   (`inset(50%)`), or a circle or ellipse of no radius;
/// - it hides its overflow and is at most a pixel wide or tall (the
///   1 x 1 box of a "visually hidden" class, a collapsed panel); not the
///   root or the body, whose overflow clips the viewport, not their own
///   box (CSS Overflow 3, 3.3): a root 0 pixels tall over an absolutely
///   placed body still shows the whole page (a video site's embedded
///   player).
///
/// `visibility: hidden` needs no rule: it is inherited, so each hidden
/// text node says so itself.
///
/// These are two tests: this one, the first, and [`veils`], the rest. A
/// control hidden only by this one is waiting for a hover.
fn transparent(dom: &Dom, i: usize) -> bool {
    dom.is_element(i)
        && dom.layout[i].is_some()
        && number(Some(dom.style(i, 8))).is_some_and(|o| o <= 0.0)
}

/// The rest of [`transparent`]'s tests: element `i` is clipped or shrunk to
/// nothing.
fn veils(dom: &Dom, i: usize) -> bool {
    if !dom.is_element(i) || dom.layout[i].is_none() {
        return false;
    }
    let positioned = matches!(dom.style(i, 14), "absolute" | "fixed");
    if positioned && clipped_to_nothing(dom.style(i, 12)) {
        return true;
    }
    if clip_path_hides(dom.style(i, 13)) {
        return true;
    }
    let display = dom.style(i, 0);
    let boxed = !display.is_empty() && display != "inline" && display != "contents";
    if boxed
        && !matches!(dom.name[i].as_str(), "HTML" | "BODY")
        && matches!(dom.style(i, 10), "hidden" | "clip")
        && let Some(rect) = dom.rect(i)
        && (rect.width <= 1.0 || rect.height <= 1.0)
    {
        return true;
    }
    false
}

/// Reject only a whole source box outside a known clipped axis. Snapshot
/// bounds already include scrolling. The outer border box is conservative
/// for hidden overflow, or clip overflow with a captured zero clip margin.
/// Missing/unknown or nonzero margins cannot prove that border-box edge.
/// Containing blocks and transforms are not captured, so stop at a nonstatic
/// positioned chain rather than guessing whether it escapes an older clip.
/// Explicit veils keep their existing semantic-twin/name-only policy.
fn overflow_clipped(dom: &Dom, i: usize) -> bool {
    let valid = |r: Rect| {
        [r.x, r.y, r.width, r.height, r.right(), r.bottom()]
            .into_iter()
            .all(f64::is_finite)
            && !r.is_empty()
    };
    let Some(rect) = dom.rect(i).filter(|&r| valid(r)) else {
        return false;
    };
    let mut child = i;
    while let Some(parent) = usize::try_from(dom.parent[child])
        .ok()
        .filter(|&p| p < child)
    {
        if dom.is_element(child) && dom.layout[child].is_some() && dom.style(child, 14) != "static"
        {
            return false;
        }
        let display = dom.style(parent, 0);
        let clips = |axis| match dom.style(parent, axis) {
            "hidden" => true,
            "clip" => matches!(
                dom.style(parent, SNAPSHOT_OVERFLOW_CLIP_MARGIN).trim(),
                "0px" | "border-box 0px" | "padding-box 0px" | "content-box 0px"
            ),
            _ => false,
        };
        if dom.is_element(parent)
            && !veils(dom, parent)
            && !matches!(dom.name[parent].as_str(), "HTML" | "BODY")
            && matches!(
                display,
                "block"
                    | "flow-root"
                    | "inline-block"
                    | "flex"
                    | "inline-flex"
                    | "grid"
                    | "inline-grid"
            )
            && let Some(clip) = dom.rect(parent).filter(|&r| valid(r))
            && ((clips(SNAPSHOT_OVERFLOW_X) && (rect.x >= clip.right() || rect.right() <= clip.x))
                || (clips(10) && (rect.y >= clip.bottom() || rect.bottom() <= clip.y)))
        {
            return true;
        }
        child = parent;
    }
    false
}

/// The digits of `text`, in order.
fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

/// The page's [screen-reader twins](Twin): an element that starts a
/// concealed run (visually hidden itself, its parent not) and a sibling
/// marked `aria-hidden="true"` that shows, whose texts have the same
/// digits, at least one. A hidden hint has no digits its sibling shows,
/// so it stays hidden.
fn twins(dom: &Dom) -> HashMap<usize, (Twin, usize)> {
    let mut out = HashMap::new();
    let hidden = |n: usize| {
        dom.attr(n, "aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    };
    for i in 0..dom.name.len() {
        if !dom.concealed[i] || !dom.is_element(i) {
            continue;
        }
        let Some(parent) = usize::try_from(dom.parent[i]).ok().filter(|&p| p < i) else {
            continue;
        };
        if dom.concealed[parent] {
            continue;
        }
        let said = digits(&dom.raw_text(i));
        if said.is_empty() {
            continue;
        }
        let shown = dom.children[parent].iter().copied().find(|&s| {
            s != i
                && dom.is_element(s)
                && hidden(s)
                && dom.layout[s].is_some()
                && !dom.concealed[s]
                // The hidden representation cannot revive a source pair
                // whose shown counterpart is wholly overflow-clipped.
                && !dom.overflow_clipped[s]
                && digits(&dom.raw_text(s)) == said
        });
        if let Some(shown) = shown {
            out.insert(i, (Twin::Spoken, shown));
            out.insert(shown, (Twin::Shown, i));
        }
    }
    out
}

/// The page's background colour: the root element's, else the body's
/// (the root's or body's background paints the whole canvas), when it is
/// not transparent.
fn page_background(dom: &Dom) -> Option<String> {
    let find = |name: &str| (0..dom.name.len()).find(|&i| dom.is_element(i) && dom.name[i] == name);
    ["HTML", "BODY"]
        .into_iter()
        .filter_map(find)
        .map(|i| dom.style(i, 3).trim().to_ascii_lowercase())
        .find(|css| opaque(css))
        .map(|css| truncate(&clean(&css), 64))
}

/// Whether a computed colour paints anything: not `transparent`, not an
/// alpha of 0, not empty.
fn opaque(css: &str) -> bool {
    if css.is_empty() || css == "transparent" {
        return false;
    }
    match css.strip_prefix("rgba(").and_then(|r| r.strip_suffix(')')) {
        Some(inner) => inner
            .rsplit([',', '/'])
            .next()
            .and_then(|a| a.trim().parse::<f64>().ok())
            .is_none_or(|a| a > 0.0),
        None => true,
    }
}

/// Whether a computed colour is fully opaque, for occlusion rather than
/// merely checking whether it paints anything.
fn fully_opaque_color(css: &str) -> bool {
    let css = css.trim();
    let Some(inner) = css
        .strip_prefix("rgb(")
        .or_else(|| css.strip_prefix("rgba("))
        .and_then(|value| value.strip_suffix(')'))
    else {
        // Unknown computed syntax is not positive evidence of occlusion.
        return false;
    };
    let parts: Vec<_> = inner
        .split([',', ' ', '/'])
        .filter(|part| !part.is_empty())
        .collect();
    if !(3..=4).contains(&parts.len())
        || parts[..3].iter().any(|part| {
            part.trim_end_matches('%')
                .parse::<f64>()
                .ok()
                .is_none_or(|value| !value.is_finite())
        })
    {
        return false;
    }
    parts.get(3).is_none_or(|alpha| {
        let scale = if alpha.ends_with('%') { 100.0 } else { 1.0 };
        alpha
            .trim_end_matches('%')
            .parse::<f64>()
            .ok()
            .is_some_and(|value| value.is_finite() && value >= scale)
    })
}

/// Whether anything in node `i` shows that is not visually hidden: text,
/// or a picture or other media.
fn shows_anything(dom: &Dom, i: usize) -> bool {
    dom.descendants(i)
        .into_iter()
        .any(|d| shows_content(dom, d))
}

fn shows_content(dom: &Dom, i: usize) -> bool {
    dom.shows(i)
        && !dom.concealed[i]
        && match dom.kind[i] {
            3 => dom.text[i].as_deref().is_some_and(|t| !t.trim().is_empty()),
            1 => {
                matches!(
                    dom.name[i].as_str(),
                    "IMG" | "SVG" | "VIDEO" | "CANVAS" | "PICTURE"
                ) || background_picture(dom, i).is_some()
            }
            _ => false,
        }
}

/// A boxless link's footprint comes from its visible content, never an
/// empty wrapper's box or the hidden words of its accessible name.
fn visible_footprint(dom: &Dom, i: usize) -> Option<Rect> {
    dom.descendants(i)
        .into_iter()
        // PICTURE's wrapper alone draws no media, but its CSS background can.
        .filter(|&d| {
            shows_content(dom, d)
                && (dom.name[d] != "PICTURE" || background_picture(dom, d).is_some())
        })
        .filter_map(|d| dom.rect(d))
        .filter(|rect| !rect.is_empty())
        .fold(None, |bounds, rect| Some(union(bounds, rect)))
}

/// Whether a computed `clip` (`rect(top, right, bottom, left)`) has no
/// area.
fn clipped_to_nothing(clip: &str) -> bool {
    let Some(inner) = clip
        .trim()
        .strip_prefix("rect(")
        .and_then(|r| r.strip_suffix(')'))
    else {
        return false;
    };
    let sides: Vec<Option<f64>> = inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p == "auto" {
                None
            } else {
                p.trim_end_matches("px").parse::<f64>().ok()
            }
        })
        .collect();
    let [top, right, bottom, left] = sides.as_slice() else {
        return false;
    };
    // `auto` is the box's own edge: an edge clipped to nothing needs both
    // of its sides given.
    let empty = |a: Option<f64>, b: Option<f64>| matches!((a, b), (Some(a), Some(b)) if b <= a);
    empty(*top, *bottom) || empty(*left, *right)
}

/// Whether a computed `clip-path` leaves nothing to see: an inset of 50%
/// or more from every side, or a circle or ellipse of no radius.
fn clip_path_hides(path: &str) -> bool {
    let path = path.trim();
    let args = |name: &str| {
        path.strip_prefix(name)
            .and_then(|r| r.strip_prefix('('))
            .and_then(|r| r.split(')').next())
            .map(str::to_string)
    };
    if let Some(inset) = args("inset") {
        let lengths: Vec<&str> = inset
            .split(" round ")
            .next()
            .unwrap_or_default()
            .split_whitespace()
            .collect();
        return !lengths.is_empty()
            && lengths.iter().all(|l| {
                l.strip_suffix('%')
                    .and_then(|p| p.parse::<f64>().ok())
                    .is_some_and(|p| p >= 50.0)
            });
    }
    for shape in ["circle", "ellipse"] {
        if let Some(radii) = args(shape) {
            let before_at = radii.split(" at ").next().unwrap_or_default();
            let mut radii = before_at.split_whitespace().peekable();
            return radii.peek().is_some()
                && radii.all(|r| {
                    r.trim_end_matches("px")
                        .trim_end_matches('%')
                        .parse::<f64>()
                        .is_ok_and(|r| r <= 0.0)
                });
        }
    }
    false
}

/// The address of node `i`'s CSS background picture, when it has one and
/// no text of its own and is at least 16 x 16 CSS pixels: a thumbnail, an
/// avatar or a banner drawn with `background-image`. The first `url(…)`
/// of the style; gradients are not pictures.
fn background_picture(dom: &Dom, i: usize) -> Option<String> {
    let style = dom.style(i, 9);
    let start = style.find("url(")? + 4;
    let end = start + style[start..].find(')')?;
    let url = style[start..end]
        .trim()
        .trim_matches(['"', '\''])
        .to_string();
    if url.is_empty() {
        return None;
    }
    let rect = dom.rect(i)?;
    if rect.width < 16.0 || rect.height < 16.0 {
        return None;
    }
    let has_text = dom
        .descendants(i)
        .into_iter()
        .any(|d| dom.kind[d] == 3 && dom.layout[d].is_some() && !dom.value[d].trim().is_empty());
    (!has_text).then_some(url)
}

/// An inline `<svg>`'s content: a hash of its subtree (element names,
/// attributes, text), its `<text>` runs joined, and its own `<title>`.
/// What a reading sees of a drawing a script changes: the arc's dash, the
/// needle's `transform`, the number's text.
fn svg_content(dom: &Dom, svg: usize) -> (u64, String, String) {
    let mut hasher = DefaultHasher::new();
    let mut words: Vec<String> = Vec::new();
    let mut title = String::new();
    // Depth first, in document order: (node, inside a <text>).
    let mut stack = vec![(svg, false)];
    while let Some((i, in_text)) = stack.pop() {
        dom.name[i].hash(&mut hasher);
        dom.attrs[i].hash(&mut hasher);
        if dom.kind[i] == 3 {
            dom.value[i].hash(&mut hasher);
            let word = clean_inline(&dom.value[i]);
            if in_text && !word.trim().is_empty() {
                words.push(word.trim().to_string());
            }
            continue;
        }
        let name = dom.name[i].as_str();
        if name == "TITLE" && usize::try_from(dom.parent[i]).ok() == Some(svg) {
            let all: Vec<&str> = dom.children[i]
                .iter()
                .filter(|&&c| dom.kind[c] == 3)
                .map(|&c| dom.value[c].as_str())
                .collect();
            all.hash(&mut hasher);
            if title.is_empty() {
                title = clean(&all.join(" "));
            }
            continue;
        }
        let in_text = in_text || name == "TEXT";
        for &child in dom.children[i].iter().rev() {
            stack.push((child, in_text));
        }
    }
    (hasher.finish(), clean(&words.join(" ")), title)
}

/// Whether a source figure caption (up to three ancestors) supplies all
/// of the alternative text. Mere proximity or a partial title is insufficient.
fn captioned(dom: &Dom, i: usize, alt: &str) -> bool {
    let normalize = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let alt = normalize(alt);
    if alt.is_empty() {
        return false;
    }
    let mut at = i;
    for _ in 0..3 {
        let Some(parent) = usize::try_from(dom.parent[at])
            .ok()
            .filter(|&p| p < dom.name.len())
        else {
            return false;
        };
        if dom.name[parent] == "FIGURE" {
            return dom
                .descendants(parent)
                .into_iter()
                .filter(|&d| dom.name[d] == "FIGCAPTION" && dom.shows(d) && !dom.concealed[d])
                .any(|d| {
                    // Compare what the text walker can show, not raw DOM text
                    // hidden in a descendant. Missing rendered text is uncertain:
                    // keep the image's alternative instead of assuming equivalence.
                    let text: Option<String> = dom
                        .descendants(d)
                        .into_iter()
                        .filter(|&t| dom.kind[t] == 3 && dom.shows(t) && !dom.concealed[t])
                        .map(|t| dom.text[t].as_deref().map(clean_inline))
                        .collect();
                    text.is_some_and(|text| normalize(&text) == alt)
                });
        }
        at = parent;
    }
    false
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
    /// The landmark it is in: an index into [`Page::regions`].
    region: Option<usize>,
    /// Hidden from assistive technology by an ancestor.
    aria_hidden: bool,
    /// Inside a link with nothing else to show: its visually hidden text
    /// is its name, and is read.
    name_only: bool,
    /// Inside one of a pair of [screen-reader twins](Twin): the spoken
    /// one's visually hidden text is read, marked so.
    twin: Option<Twin>,
}

/// Tags whose content is never page text.
const SKIPPED: &[&str] = &[
    "HEAD", "SCRIPT", "STYLE", "TEMPLATE", "NOSCRIPT", "TITLE", "META", "LINK", "OPTION",
    "OPTGROUP", "DATALIST",
];

struct Walker<'a> {
    source: Option<Extraction>,
    dom: &'a Dom,
    ax: &'a Ax<'a>,
    page: Page,
    runs: Vec<Run>,
    rect: Option<Rect>,
    /// The box of the last text in [`runs`](Self::runs).
    last: Option<Rect>,
    marker: Option<String>,
    by_node: HashMap<NodeId, usize>,
    revisions: &'a mut Revisions,
    /// Scroll boxes the page appends entries to: live logs too.
    appended: &'a HashSet<NodeId>,
}

impl Walker<'_> {
    fn push(&mut self, run: Run, rect: Option<Rect>, node: usize) {
        // A box with no area (Chrome gives some text, an empty inline or
        // an icon's pseudo-element one at the page's origin) says nothing
        // about where the text is: in the union it would stretch the block
        // to the top of the page, and the projection would draw it there.
        let rect = rect.filter(|r| r.width > 0.0 || r.height > 0.0);
        if let Some(rect) = rect {
            // Two boxes on one line with a gap between them (flex items,
            // links spaced by margins) keep a space between their texts.
            if let Some(last) = self.last {
                let one_line = rect.y < last.bottom() - 2.0 && last.y < rect.bottom() - 2.0;
                let ends = self
                    .runs
                    .last()
                    .is_none_or(|r| r.text.ends_with(char::is_whitespace));
                if one_line
                    && rect.x - last.right() >= GAP
                    && !ends
                    && !run.text.starts_with(char::is_whitespace)
                {
                    self.runs.push(Run::plain(" "));
                    if let Some(source) = &mut self.source { source.runs.push(vec![None]); }
                }
            }
            self.last = Some(rect);
            self.rect = Some(union(self.rect, rect));
        }
        match self.runs.last_mut() {
            Some(last)
                if last.bold == run.bold
                    && last.italic == run.italic
                    && last.code == run.code
                    && last.control == run.control
                    && last.color == run.color
                    && last.twin == run.twin
                    && last.picture.is_none()
                    && run.picture.is_none() =>
            {
                last.text.push_str(&run.text);
                if let Some(source) = &mut self.source {
                    source.runs.last_mut().unwrap().extend(run.text.chars().map(|_| Some(node)));
                }
            }
            _ => {
                if let Some(source) = &mut self.source { source.runs.push(run.text.chars().map(|_| Some(node)).collect()); }
                self.runs.push(run);
            },
        }
    }

    /// End the block being gathered, as `context` says what it is.
    fn flush(&mut self, context: &Context) {
        let mut runs = std::mem::take(&mut self.runs);
        let mut origins = self.source.as_mut().map(|s| std::mem::take(&mut s.runs));
        let rect = self.rect.take();
        self.last = None;
        if !context.pre {
            tidy_origins(&mut runs, origins.as_mut());
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
        if let (Some(source), Some(origins)) = (&mut self.source, origins) {
            source.blocks.insert(self.page.blocks.len(), origins);
        }
        self.page.blocks.push(Block {
            kind,
            runs,
            quote: context.quote,
            rect,
            frame: context.frame,
            node: context.node,
            region: context.region,
            stack: Stack::default(),
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
            stack: Stack::default(),
        });
    }

    fn walk(&mut self, i: usize, context: &Context) {
        let dom = self.dom;
        if dom.overflow_clipped[i] {
            // A clipped ancestor is not a subtree veil. Keep looking for
            // escaping descendants, including their original source links.
            let link_with_content = dom.is_element(i)
                && self.ax.role(dom.backend[i]).or_else(|| dom_role(dom, i)) == Some(Role::Link)
                && visible_footprint(dom, i).is_some();
            if !link_with_content {
                for &child in &dom.children[i] {
                    self.walk(child, context);
                }
                return;
            }
        }
        match dom.kind[i] {
            3 => {
                if context.silent
                    || !dom.shows(i)
                    || dom.concealed[i] && !context.name_only && context.twin != Some(Twin::Spoken)
                {
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
                    // The page's own colour, as CSS (a link's too): the
                    // layout's page-colors rule decides what of it shows.
                    color: (!color.is_empty()).then(|| truncate(&clean(color), 64)),
                    picture: None,
                    twin: context.twin,
                };
                self.push(run, dom.rect(i), i);
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
                    i,
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
            if kind == MediaKind::Image
                && let Some(rect) = pending_box(dom, i)
            {
                self.pending(i, rect, context);
            } else if laid_out && dom.shows(i) && !dom.concealed[i] {
                let index = self.page.media.len();
                self.media(i, kind, context);
                // A canvas or a drawing given a meter's or a progress
                // bar's role, or a drawing named as a rating, is an
                // indicator too.
                if let Some(indicator) = self.indicator_kind(i) {
                    self.indicator(i, indicator, Some(index), context);
                }
            }
            return;
        }
        if laid_out
            && dom.shows(i)
            && !context.silent
            && let Some(kind) = self.log_kind(i)
        {
            self.flush(context);
            self.log(i, kind, context);
            return;
        }
        if laid_out
            && dom.shows(i)
            && !dom.concealed[i]
            && let Some(kind) = self.indicator_kind(i)
        {
            self.indicator(i, kind, None, context);
            return;
        }
        // A CSS background picture on a box with no text of its own (a
        // thumbnail, an avatar, a banner): a picture region, under what
        // the box holds. A box with text keeps its text instead.
        if laid_out && dom.shows(i) && background_picture(dom, i).is_some() {
            self.media(i, MediaKind::Image, context);
        }
        // Controls: an interactive role in the AX tree, or a form element.
        let role = self.ax.role(backend).or_else(|| dom_role(dom, i));
        if let Some(role) = role {
            // Transparent until hovered: on the page's lists, not drawn.
            if laid_out
                && dom.faded[i]
                && !dom.veiled[i]
                && !matches!(dom.style(i, 1), "hidden" | "collapse")
            {
                let index = self.control(i, role);
                self.page.controls[index].hover_only = true;
                return;
            }
            let footprint = ((!laid_out || dom.overflow_clipped[i]) && role == Role::Link)
                .then(|| visible_footprint(dom, i))
                .flatten();
            if !laid_out && footprint.is_none()
                || !dom.shows(i) && role != Role::Link
                || dom.concealed[i]
            {
                return;
            }
            let index = self.control(i, role.clone());
            if role == Role::Link {
                if let Some(rect) = footprint {
                    self.page.controls[index].rect = rect;
                }
                let inner = Context {
                    control: Some(index),
                    name_only: !shows_anything(dom, i),
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
        if name == "TABLE" && dom.style(i, 0).starts_with("table") && !layout_table(dom, i) {
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
                    stack: Stack::default(),
                });
            }
            return;
        }
        let mut inner = context.clone();
        if let Some(twin) = dom.twins.get(&i) {
            inner.twin = Some(twin.0);
        }
        let role_attr = dom.attr(i, "role").unwrap_or_default().to_ascii_lowercase();
        if dom
            .attr(i, "aria-hidden")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
        {
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
        // Reflow an actual source container, never nearby independent boxes.
        // A nonwrapping flex row qualifies only for two named source sections,
        // one containing video and the other a live log. This establishes layout
        // containment, not a shared playback/chat state or application meaning.
        let display = dom.style(i, 0);
        let paired_sections = if laid_out && matches!(display, "flex" | "inline-flex") {
            let panes: Vec<_> = dom.children[i]
                .iter()
                .copied()
                .filter(|&c| {
                    dom.is_element(c) && dom.shows(c) && dom.rect(c).is_some_and(|r| !r.is_empty())
                })
                .collect();
            let named_section = |c| {
                (dom.name[c] == "SECTION" || dom.attr(c, "role") == Some("region"))
                    && ["aria-label", "aria-labelledby"]
                        .iter()
                        .any(|a| dom.attr(c, a).is_some_and(|v| !v.trim().is_empty()))
                    && !self.label(c).is_empty()
            };
            let contains = |c, video| {
                dom.descendants(c).into_iter().any(|d| {
                    if video {
                        dom.name[d] == "VIDEO"
                    } else {
                        dom.attr(d, "role") == Some("log")
                    }
                })
            };
            panes.len() == 2
                && panes.iter().all(|&c| named_section(c))
                && ((contains(panes[0], true)
                    && !contains(panes[0], false)
                    && contains(panes[1], false)
                    && !contains(panes[1], true))
                    || (contains(panes[1], true)
                        && !contains(panes[1], false)
                        && contains(panes[0], false)
                        && !contains(panes[0], true)))
        } else {
            false
        };
        // Grids, wrapping flex rows, and the explicit two-pane source layout.
        if laid_out
            && (matches!(display, "grid" | "inline-grid")
                || (matches!(display, "flex" | "inline-flex") && dom.style(i, 11) == "wrap")
                || paired_sections)
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
        if laid_out && let Some(shelf) = self.shelf(i) {
            self.page.shelves.push(shelf);
        }
        match name {
            "B" | "STRONG" => inner.bold = true,
            "I" | "EM" | "CITE" | "DFN" => inner.italic = true,
            "CODE" | "KBD" | "SAMP" | "TT" => inner.code = true,
            "LABEL" => {
                // It names what it labels, which shows the name: a
                // control, a meter or a progress bar that is drawn. A
                // `for` that points at anything else, or at nothing, or
                // at a control that is not drawn (a custom check box's
                // transparent input), leaves it the text it is.
                let labels_a_control = match dom.attr(i, "for") {
                    Some(id) => dom
                        .ids
                        .get(id.trim())
                        .is_some_and(|&t| self.labelable(t) && !dom.concealed[t]),
                    None => dom.descendants(i).into_iter().any(|d| {
                        dom.is_element(d) && dom_role(dom, d).is_some() && !dom.concealed[d]
                    }),
                };
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
            .filter(|a| !script_value(a))
            .unwrap_or_default();
        // A drawing's own words: its `<text>`, and its `<title>` as its
        // name when it has no other (Chrome's AX tree gives it one).
        let (revision, text, title) = if kind == MediaKind::Svg {
            svg_content(dom, i)
        } else {
            (0, String::new(), String::new())
        };
        let alt = if alt.is_empty() && kind == MediaKind::Svg {
            if title.is_empty() {
                text.clone()
            } else {
                title
            }
        } else {
            alt
        };
        let src = dom
            .source
            .get(&i)
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .or_else(|| dom.attr(i, "src").filter(|s| !s.trim().is_empty()))
            .map(str::to_string)
            .or_else(|| background_picture(dom, i))
            .map(|s| clean(&crate::url::resolve(&dom.url, &s)))
            .unwrap_or_default();
        let poster = dom
            .attr(i, "poster")
            .filter(|_| kind == MediaKind::Video)
            .filter(|p| !p.trim().is_empty())
            .map(|p| clean(&crate::url::resolve(&dom.url, p)))
            .unwrap_or_default();
        let controls = dom.attr(i, "controls").is_some();
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
            picture: None,
            captioned: captioned(dom, i, &alt),
            playback: None,
            poster: truncate(&poster, 2000),
            controls,
            autoplay: dom.attr(i, "autoplay").is_some(),
            empty_alt: dom.attr(i, "alt").is_some_and(|a| a.trim().is_empty()),
            aria_hidden: context.aria_hidden
                || dom
                    .attr(i, "aria-hidden")
                    .is_some_and(|v| v.eq_ignore_ascii_case("true"))
                || matches!(role.as_str(), "presentation" | "none"),
            in_label: context.control.is_some()
                || context.silent
                || matches!(context.kind, BlockKind::Heading(_)),
            revision,
            text: truncate(&text, 500),
            pending: false,
        });
        self.block(BlockKind::Media(index), i, context);
    }

    /// Whether a `<label for>` can name node `i`: a control, a meter or
    /// a progress bar.
    fn labelable(&self, i: usize) -> bool {
        let dom = self.dom;
        dom.is_element(i)
            && (dom_role(dom, i).is_some()
                || self.ax.role(dom.backend[i]).is_some()
                || matches!(dom.name[i].as_str(), "METER" | "PROGRESS")
                || matches!(
                    self.indicator_kind(i),
                    Some(IndicatorKind::Meter | IndicatorKind::Progress)
                ))
    }

    /// Whether node `i` is an [`Indicator`], and which kind (a progress
    /// with no value is told apart when it is read).
    fn indicator_kind(&self, i: usize) -> Option<IndicatorKind> {
        let dom = self.dom;
        match dom.name[i].as_str() {
            "METER" => return Some(IndicatorKind::Meter),
            "PROGRESS" => return Some(IndicatorKind::Progress),
            _ => {}
        }
        let role = dom
            .attr(i, "role")
            .and_then(|r| r.split_whitespace().next())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let ax_role = self
            .ax
            .get(dom.backend[i])
            .and_then(|n| n["role"]["value"].as_str())
            .unwrap_or_default();
        match (role.as_str(), ax_role) {
            ("progressbar", _) | (_, "progressbar" | "progressIndicator") => {
                Some(IndicatorKind::Progress)
            }
            ("meter", _) | (_, "meter") => Some(IndicatorKind::Meter),
            ("img" | "image", _) | ("", "img" | "image") if rating_of(&self.label(i)).is_some() => {
                Some(IndicatorKind::Rating)
            }
            _ => None,
        }
    }

    /// The text of the elements `ids` (space-separated) names, as
    /// `aria-labelledby` points at them.
    fn text_of_ids(&self, ids: &str) -> String {
        ids.split_whitespace()
            .filter_map(|id| self.dom.ids.get(id))
            .map(|&t| clean(&self.dom.raw_text(t)))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Node `i`'s indicator, of `kind`, as a block; `media` is the region
    /// it also is.
    fn indicator(
        &mut self,
        i: usize,
        kind: IndicatorKind,
        media: Option<usize>,
        context: &Context,
    ) {
        let dom = self.dom;
        let backend = dom.backend[i];
        let ax = self.ax.get(backend);
        let ax_number = |prop: &str| {
            ax.and_then(|n| property(n, prop)).and_then(|v| {
                v.as_f64()
                    .or_else(|| v.as_str().and_then(|t| number(Some(t))))
            })
        };
        let ax_value = ax.and_then(|n| match &n["value"]["value"] {
            Value::Number(v) => v.as_f64(),
            Value::String(t) => number(Some(t)),
            _ => None,
        });
        let attr = |name: &str| number(dom.attr(i, name));
        let label = Some(self.label(i))
            .filter(|l| !l.is_empty())
            .or_else(|| {
                let id = dom.attr(i, "id")?;
                (0..dom.name.len())
                    .find(|&l| dom.name[l] == "LABEL" && dom.attr(l, "for") == Some(id))
                    .map(|l| clean(&dom.raw_text(l)))
                    .filter(|t| !t.is_empty())
            })
            .or_else(|| {
                dom.attr(i, "aria-labelledby")
                    .map(|ids| self.text_of_ids(ids))
                    .filter(|t| !t.is_empty())
            })
            .or_else(|| dom.attr(i, "title").map(clean))
            .map(|l| truncate(&l, 200))
            .unwrap_or_default();
        let (mut kind, value, min, max) = match (kind, dom.name[i].as_str()) {
            (IndicatorKind::Rating, _) => {
                let (value, scale) = rating_of(&label).unwrap_or((0.0, 5.0));
                (kind, Some(value), 0.0, scale)
            }
            // A meter's value is 0 when it says none.
            (_, "METER") => (
                kind,
                attr("value").or(ax_value).or(Some(0.0)),
                attr("min").unwrap_or(0.0),
                attr("max").unwrap_or(1.0),
            ),
            // A progress with no value is busy (indeterminate).
            (_, "PROGRESS") => (kind, attr("value"), 0.0, attr("max").unwrap_or(1.0)),
            _ => (
                kind,
                attr("aria-valuenow")
                    .or_else(|| ax_number("valuenow"))
                    .or(ax_value),
                attr("aria-valuemin")
                    .or_else(|| ax_number("valuemin"))
                    .unwrap_or(0.0),
                attr("aria-valuemax")
                    .or_else(|| ax_number("valuemax"))
                    .unwrap_or(100.0),
            ),
        };
        let max = if max < min { min } else { max };
        let value = value.map(|v| v.clamp(min, max));
        if kind == IndicatorKind::Progress && value.is_none() {
            kind = IndicatorKind::Busy;
        }
        if kind == IndicatorKind::Meter && value.is_none() {
            kind = IndicatorKind::Busy;
        }
        let text = dom
            .attr(i, "aria-valuetext")
            .map(clean)
            .or_else(|| {
                ax.and_then(|n| property(n, "valuetext"))
                    .and_then(Value::as_str)
                    .map(clean)
            })
            .or_else(|| {
                // What a styled box holds (a signal meter's glyphs); a
                // meter's or a progress's own content is fallback, never
                // shown, and a rating's is its picture.
                if matches!(dom.name[i].as_str(), "METER" | "PROGRESS")
                    || kind == IndicatorKind::Rating
                {
                    return None;
                }
                let text = clean(&dom.raw_text(i));
                (!text.trim().is_empty() && text.trim() != label).then(|| text.trim().to_string())
            })
            .filter(|t| !t.trim().is_empty())
            .map(|t| truncate(&t, 200));
        let bands = (dom.name[i] == "METER"
            && ["low", "high", "optimum"].iter().any(|a| attr(a).is_some()))
        .then(|| {
            let low = attr("low").unwrap_or(min).clamp(min, max);
            let high = attr("high").unwrap_or(max).clamp(low, max);
            let optimum = attr("optimum").unwrap_or((min + max) / 2.0).clamp(min, max);
            (low, high, optimum)
        });
        let before = plain(&self.runs).trim_end().to_lowercase();
        let label_in_text = !label.is_empty() && before.ends_with(&label.to_lowercase());
        let after_text = !before.is_empty() && !before.ends_with('\n');
        self.flush(context);
        let index = self.page.indicators.len();
        self.page.indicators.push(Indicator {
            node: backend,
            kind,
            label,
            value,
            min,
            max,
            text,
            rect: dom.rect(i).unwrap_or_default(),
            bands,
            media,
            label_in_text,
            after_text,
        });
        self.block(BlockKind::Indicator(index), i, context);
    }

    /// A placeholder for the `<img>` on node `i`, which has no picture yet,
    /// in `rect`: its alt text or title, else its link's.
    fn pending(&mut self, i: usize, rect: Rect, context: &Context) {
        let dom = self.dom;
        self.media(i, MediaKind::Image, context);
        let mut alt = self
            .page
            .media
            .last()
            .map(|m| m.alt.clone())
            .unwrap_or_default();
        if alt.is_empty() {
            let mut at = dom.parent[i];
            for _ in 0..6 {
                let Ok(p) = usize::try_from(at) else { break };
                if dom.name[p] == "A" {
                    alt = dom
                        .attr(p, "title")
                        .or_else(|| dom.attr(p, "aria-label"))
                        .map(clean)
                        .filter(|t| !script_value(t))
                        .unwrap_or_default();
                    break;
                }
                at = dom.parent[p];
            }
        }
        if let Some(media) = self.page.media.last_mut() {
            media.pending = true;
            media.rect = rect;
            media.alt = truncate(&alt, 500);
        }
        if let Some(block) = self.page.blocks.last_mut() {
            block.rect = rect;
            block.frame = rect;
        }
    }

    /// Its accessible name: the AX tree's, else `aria-label`.
    /// Node `i` as a [`Shelf`], if it is one: it scrolls sideways
    /// (`overflow-x` `auto` or `scroll`, its content wider than its box,
    /// not taller), and three or more of its children that show are alike
    /// (one tag, widths within a quarter of each other) and side by side
    /// (one top). A list's items may sit one level down (a `<div>` holding
    /// a `<ul>`).
    fn shelf(&self, i: usize) -> Option<Shelf> {
        let dom = self.dom;
        if !matches!(dom.style(i, SNAPSHOT_OVERFLOW_X), "auto" | "scroll") {
            return None;
        }
        let (scroll, client) = dom.scroll(i)?;
        if scroll.width <= client.width + 1.0 || client.width <= 0.0 {
            return None;
        }
        let rect = dom.rect(i)?;
        let alike = |parent: usize| -> Vec<Rect> {
            let children: Vec<(usize, Rect)> = dom.children[parent]
                .iter()
                .filter(|&&c| dom.is_element(c) && dom.shows(c))
                .filter_map(|&c| Some((c, dom.rect(c).filter(|r| !r.is_empty())?)))
                .collect();
            // The most common tag among them.
            let mut counts: HashMap<&str, usize> = HashMap::new();
            for (c, _) in &children {
                *counts.entry(dom.name[*c].as_str()).or_default() += 1;
            }
            let Some((tag, _)) = counts.into_iter().max_by_key(|(_, n)| *n) else {
                return Vec::new();
            };
            let items: Vec<Rect> = children
                .iter()
                .filter(|(c, _)| dom.name[*c] == tag)
                .map(|(_, r)| *r)
                .collect();
            let mut widths: Vec<f64> = items.iter().map(|r| r.width).collect();
            widths.sort_by(f64::total_cmp);
            let median = widths.get(widths.len() / 2).copied().unwrap_or(0.0);
            let top = items.first().map_or(0.0, |r| r.y);
            let ok = items.len() >= 3
                && median > 0.0
                && items.iter().all(|r| {
                    (r.width - median).abs() <= median / 4.0 && (r.y - top).abs() < r.height / 2.0
                });
            if ok { items } else { Vec::new() }
        };
        let mut items = alike(i);
        if items.is_empty()
            && let [only] = dom.children[i]
                .iter()
                .copied()
                .filter(|&c| dom.is_element(c) && dom.shows(c))
                .collect::<Vec<_>>()
                .as_slice()
        {
            items = alike(*only);
        }
        if items.is_empty() {
            return None;
        }
        Some(Shelf {
            node: dom.backend[i],
            rect,
            items,
            scroll_left: scroll.x,
            scroll_width: scroll.width,
            client_width: client.width,
            label: self.label(i),
        })
    }

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
        } else if self.appended.contains(&dom.backend[i])
            // The page's own root scrolls, but it is the page, not a log
            // in it: a script appending to it (The Guardian's do) would
            // turn the whole page into one log's entries.
            && !matches!(dom.name[i].as_str(), "HTML" | "BODY")
        {
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
                control: None,
                silent: false,
                ..context.clone()
            };
            let (mut runs, _) = self.inline(child, &inner);
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
                dom.attr(i, "aria-labelledby")
                    .map(|ids| self.text_of_ids(ids))
                    .filter(|t| !t.is_empty())
            })
            .or_else(|| {
                // Its text, an inner drawing's `<title>` among it.
                let text = clean(&dom.raw_text(i));
                (!text.trim().is_empty()).then(|| text.trim().to_string())
            })
            .or_else(|| dom.attr(i, "name").map(clean))
            .unwrap_or_default();
        let icon_only = role == Role::Button
            && if dom.name[i] == "INPUT" {
                kind == "image"
            } else {
                !shows_text(dom, i)
            };
        // An icon button's face: a glyph for its name, else for its
        // icon's (`<use href="#icon-close">`, `class="icon-search"`),
        // which then names it too.
        let (name, glyph) = if icon_only {
            match glyph_for(&name) {
                Some(glyph) => (name, Some(glyph)),
                None => match icon_name(dom, i) {
                    Some((word, glyph)) if name.is_empty() => (word, Some(glyph)),
                    _ => (name, None),
                },
            }
        } else {
            (name, None)
        };
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
            icon_only,
            glyph: glyph.map(str::to_string),
            hover_only: false,
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
        let block = self.page.blocks.len();
        let mut cells = Vec::new();
        let mut stack: Vec<usize> = dom.children[i].iter().rev().copied().collect();
        while let Some(n) = stack.pop() {
            match dom.name[n].as_str() {
                "TABLE" => continue,
                "CAPTION" => {
                    let (runs, origins) = self.inline(n, context);
                    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
                    let start = text.chars().take_while(|c| c.is_whitespace()).count();
                    table.caption = text.trim().to_string();
                    if let Some(origins) = origins {
                        cells.push((RunLocation::Caption { block }, origins.into_iter().flatten().skip(start).take(table.caption.chars().count()).collect()));
                    }
                }
                "TR" => {
                    let mut row = Vec::new();
                    for &cell in &dom.children[n] {
                        let header = dom.name[cell] == "TH";
                        if !(header || dom.name[cell] == "TD") || dom.layout[cell].is_none() {
                            continue;
                        }
                        let (mut runs, mut origins) = self.inline(cell, context);
                        tidy_origins(&mut runs, origins.as_mut());
                        if let Some(origins) = origins {
                            for (run, origins) in origins.into_iter().enumerate() {
                                cells.push((RunLocation::Cell { block, row: table.rows.len(), cell: row.len(), run }, origins));
                            }
                        }
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
        if !table.rows.is_empty() {
            if let Some(source) = &mut self.source { source.cells.extend(cells); }
            Some(table)
        } else { None }
    }

    /// The text runs of node `i`, its blocks run together: a cell's.
    fn inline(&mut self, i: usize, context: &Context) -> (Vec<Run>, Option<Origins>) {
        let saved_runs = std::mem::take(&mut self.runs);
        let saved_cells = self.source.as_ref().map_or(0, |s| s.cells.len());
        let saved_origins = self.source.as_mut().map(|s| std::mem::take(&mut s.runs));
        let mut origins = self.source.as_ref().map(|_| Origins::new());
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
        for (index, block) in made.into_iter().enumerate() {
            let original = self.source.as_mut().and_then(|s| s.blocks.remove(&(blocks + index)));
            let start = runs.len();
            let node = self.dom.backend.iter().position(|&n| n == block.node);
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
            if let Some(origins) = &mut origins {
                if let Some(original) = original {
                    origins.extend(original);
                    origins.push(vec![None]);
                } else {
                    origins.extend(runs[start..].iter().map(|r| r.text.chars().map(|_| node).collect()));
                }
            }
        }
        if let Some(source) = &mut self.source {
            origins.as_mut().unwrap().append(&mut source.runs);
            source.runs = saved_origins.unwrap();
            // Nested tables folded into inline text have no final table address.
            source.cells.truncate(saved_cells);
        }
        runs.append(&mut self.runs);
        self.runs = saved_runs;
        self.rect = saved_rect;
        (runs, origins)
    }
}

/// The box a placeholder for `<img>` `i` takes when the image has no
/// source yet: a lazy thumbnail, transparent (`opacity: 0`) until its
/// picture loads, in its own box; or one with no size of its own yet, in
/// its parent's box when that holds nothing else. Not one hidden another
/// way, and only a box of at least 16 x 16 CSS pixels.
fn pending_box(dom: &Dom, i: usize) -> Option<Rect> {
    let has_source = dom.source.get(&i).is_some_and(|s| !s.trim().is_empty())
        || dom.attr(i, "src").is_some_and(|s| !s.trim().is_empty());
    if has_source || dom.layout[i].is_none() || dom.veiled[i] {
        return None;
    }
    if matches!(dom.style(i, 1), "hidden" | "collapse") {
        return None;
    }
    let big = |r: &Rect| r.width >= 16.0 && r.height >= 16.0;
    let own = dom.rect(i)?;
    if big(&own) {
        return Some(own);
    }
    let parent = usize::try_from(dom.parent[i]).ok()?;
    let rect = dom.rect(parent).filter(big)?;
    let alone = dom.children[parent].iter().all(|&c| {
        c == i || dom.layout[c].is_none() || dom.kind[c] == 3 && dom.value[c].trim().is_empty()
    });
    alone.then_some(rect)
}

/// Whether table `i` lays a page out rather than holding data: it says
/// so (`role="presentation"` or `"none"`), or a table is nested in it, as
/// browsers' accessibility trees judge. Its contents are then read as the
/// blocks they are, so a page built of tables (Hacker News) keeps its text.
fn layout_table(dom: &Dom, i: usize) -> bool {
    let role = dom.attr(i, "role").unwrap_or_default();
    if role.eq_ignore_ascii_case("presentation") || role.eq_ignore_ascii_case("none") {
        return true;
    }
    let mut stack: Vec<usize> = dom.children[i].clone();
    while let Some(n) = stack.pop() {
        if dom.name[n] == "TABLE" {
            return true;
        }
        stack.extend(dom.children[n].iter().copied());
    }
    false
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
                    decorative: crate::adapt::is_decorative(width, height, hidden || empty_alt),
                }),
                ..Run::default()
            });
            runs.push(Run::plain(" "));
            continue;
        }
        let text = part["t"].as_str().unwrap_or_default();
        let text = if text == "\n" {
            "\n".to_string()
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
            color: part["c"]
                .as_str()
                .filter(|c| !c.is_empty())
                .map(|c| truncate(&clean(c), 64)),
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

/// Whether node `i` shows text of its own: a laid-out, visible text node
/// under it with words. A drawing's `<title>` is never laid out.
fn shows_text(dom: &Dom, i: usize) -> bool {
    dom.descendants(i).into_iter().any(|d| {
        dom.kind[d] == 3
            && dom.layout[d].is_some()
            && dom.shows(d)
            && !dom.concealed[d]
            && dom.text[d].as_deref().is_some_and(|t| !t.trim().is_empty())
    })
}

/// Icon words and the glyph each is drawn as, on an icon-only button.
const GLYPHS: &[(&[&str], &str)] = &[
    (&["more", "ellipsis", "kebab", "overflow", "dots"], "⋯"),
    (&["close", "dismiss", "cancel", "x"], "✕"),
    (&["search", "magnifier"], "🔍"),
    (&["menu", "hamburger", "burger", "navigation"], "≡"),
    (&["settings", "gear", "cog", "preferences"], "⚙"),
    (&["home"], "⌂"),
    (&["notifications", "notification", "bell", "alerts"], "🔔"),
    (&["user", "profile", "account", "avatar"], "👤"),
    (&["play"], "▶"),
    (&["pause"], "⏸"),
    (&["next", "forward"], "›"),
    (&["previous", "prev", "back"], "‹"),
    (&["share"], "⤴"),
    (&["like", "heart", "favorite", "favourite"], "♥"),
    (
        &[
            "chat", "message", "messages", "comment", "comments", "whispers",
        ],
        "💬",
    ),
    (&["cart", "basket"], "🛒"),
    (&["download"], "⤓"),
    (&["upload"], "⤒"),
    (&["edit", "pencil"], "✎"),
    (&["delete", "trash"], "🗑"),
    (&["add", "plus"], "+"),
    (&["remove", "minus"], "−"),
    (&["expand", "chevron", "caret"], "▾"),
    (&["collapse"], "▴"),
    (&["fullscreen", "maximize"], "⛶"),
    (&["mute", "volume", "unmute"], "🔈"),
    (&["refresh", "reload"], "↻"),
    (&["help", "question"], "?"),
    (&["info", "information"], "ⓘ"),
    (&["filter", "filters"], "⏷"),
    (&["copy"], "⧉"),
    (&["link"], "🔗"),
    (&["bookmark", "save"], "🔖"),
];

/// The glyph of an icon word, if it is one.
fn glyph_of_word(word: &str) -> Option<&'static str> {
    GLYPHS
        .iter()
        .find(|(words, _)| words.contains(&word))
        .map(|(_, glyph)| *glyph)
}

/// The glyph for an icon button named `name`: its first word an icon
/// word, in a short name ("More actions", "Close", "Search the shop");
/// not a sentence.
fn glyph_for(name: &str) -> Option<&'static str> {
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() || words.len() > 4 {
        return None;
    }
    glyph_of_word(&words[0])
}

/// The name of the icon a button draws, as the page spells it in its
/// own attributes or its icon's (`data-icon`, `<use href="#icon-close">`,
/// `class="fa-search"`, `data-testid="CloseIcon"`), and its glyph: the
/// first icon word found, capitalised (`Close`).
fn icon_name(dom: &Dom, i: usize) -> Option<(String, &'static str)> {
    let mut nodes = vec![i];
    nodes.extend(
        dom.descendants(i)
            .into_iter()
            .filter(|&d| dom.is_element(d))
            .take(32),
    );
    for n in nodes {
        for (key, value) in &dom.attrs[n] {
            let value = match key.as_str() {
                "data-icon" | "icon" | "data-testid" | "data-test-selector" | "data-a-target"
                | "id" => value.clone(),
                // Of its classes, only an icon's (`icon-search`,
                // `fa-search`): the rest name its look (`…ButtonShapeNext…`).
                "class" => value
                    .split_whitespace()
                    .filter(|c| {
                        let c = c.to_ascii_lowercase();
                        c.contains("icon")
                            || ["fa-", "bi-", "mdi-", "ti-"]
                                .iter()
                                .any(|p| c.starts_with(p))
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
                "href" | "xlink:href" if dom.name[n] == "USE" => {
                    value.rsplit('#').next().unwrap_or_default().to_string()
                }
                _ => continue,
            };
            for word in words_of(&value) {
                if let Some(glyph) = glyph_of_word(&word) {
                    let mut name = word.clone();
                    if let Some(first) = name.get_mut(0..1) {
                        first.make_ascii_uppercase();
                    }
                    return Some((name, glyph));
                }
            }
        }
    }
    None
}

/// The words of an identifier: split at anything not a letter, and
/// between a lower-case letter and an upper-case one (`CloseIcon`), in
/// lower case.
fn words_of(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut lower = false;
    for c in text.chars() {
        if !c.is_ascii_alphabetic() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            lower = false;
            continue;
        }
        if c.is_ascii_uppercase() && lower && !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
        lower = c.is_ascii_lowercase();
        word.push(c.to_ascii_lowercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
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
    tidy_origins(runs, None);
}

fn tidy_origins(runs: &mut Vec<Run>, mut origins: Option<&mut Origins>) {
    let mut space = true;
    for (index, run) in runs.iter_mut().enumerate() {
        let mut text = String::with_capacity(run.text.len());
        let mut kept = origins.as_ref().map(|_| Vec::new());
        for (character, c) in run.text.chars().enumerate() {
            if c == '\n' {
                while text.ends_with(' ') {
                    text.pop();
                    if let Some(kept) = &mut kept { kept.pop(); }
                }
                text.push('\n');
                space = true;
            } else if c == ' ' {
                if space { continue; }
                text.push(' ');
                space = true;
            } else {
                text.push(c);
                space = false;
            }
            if let (Some(kept), Some(origins)) = (&mut kept, &origins) { kept.push(origins[index][character]); }
        }
        run.text = text;
        if let (Some(origins), Some(kept)) = (&mut origins, kept) { origins[index] = kept; }
    }
    while let Some(last) = runs.last_mut() {
        let trimmed = last.text.trim_end_matches([' ', '\n']).len();
        last.text.truncate(trimmed);
        if let Some(origins) = &mut origins { origins.last_mut().unwrap().truncate(last.text.chars().count()); }
        if last.text.is_empty() {
            runs.pop();
            if let Some(origins) = &mut origins { origins.pop(); }
        } else { break; }
    }
    for i in (0..runs.len()).rev() {
        if runs[i].text.is_empty() {
            runs.remove(i);
            if let Some(origins) = &mut origins { origins.remove(i); }
        }
    }
    if let Some(first) = runs.first_mut() {
        let count = first.text.chars().take_while(|&c| c == '\n').count();
        first.text = first.text.trim_start_matches(['\n']).to_string();
        if let Some(origins) = &mut origins { origins[0].drain(..count); }
    }
}

/// How near the viewport's top a fixed box (or the bottom of the boxes
/// pinned above it) may start and still be a top bar: about a row, in CSS
/// pixels.
const TOUCH: f64 = 24.0;
/// The tags whose box is a picture: one covers what lies wholly under it.
const PICTURE_TAGS: &[&str] = &["IMG", "VIDEO", "CANVAS", "SVG", "IFRAME", "EMBED", "OBJECT"];
/// The most covering boxes looked at, the largest first.
const MAX_COVERS: usize = 1024;

/// Whether `outer` holds all of `inner`, to a pixel.
fn holds(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - 1.0
        && inner.y >= outer.y - 1.0
        && inner.right() <= outer.right() + 1.0
        && inner.bottom() <= outer.bottom() + 1.0
}

/// The page's stack of boxes, read into [`Page::layers`] and each block's
/// [`Stack`]:
/// - a block in a `position: fixed` or `sticky` box (its outermost) is in
///   that box's layer. A fixed box touching the viewport's top (or the
///   boxes already pinned) is pinned, others are docked, one wholly off
///   screen is not drawn, and one taller than a quarter of the viewport
///   goes in the flow where it is on screen. A sticky box goes in the flow
///   at its static place (recovered from the box after it, as the
///   snapshot reports a stuck box where it is stuck), pinned instead when
///   that place is the page's top;
/// - a block a box painted above it covers wholly is `covered`: only a
///   box with an opaque background or a picture covers, never one of its
///   ancestors or descendants, nor a fixed or sticky one (they move);
/// - a block wholly inside a picture's box and painted above it is
///   `over` it.
fn stack(dom: &Dom, ax: &Ax<'_>, page: &mut Page) {
    page.scroll_y = dom.scroll_y;
    page.view_height = dom.view_height;
    let n = dom.parent.len();
    let parent = |i: usize| usize::try_from(dom.parent[i]).ok().filter(|&p| p < i);
    let shell = shell_bars(dom);
    // Source semantics establish a surface independently of fixed/opaque boxes.
    // The inherited fixed root still includes the container around a nested dialog.
    let declared: Vec<_> = (0..n)
        .filter(|&i| {
            if !dom.is_element(i)
                || dom.concealed[i]
                || !dom.shows(i)
                || !dom
                    .rect(i)
                    .is_some_and(|rect| rect.width > 0.0 && rect.height > 0.0)
            {
                return false;
            }
            let role = dom.attr(i, "role").unwrap_or_default();
            role.split_whitespace().any(|role| {
                role.eq_ignore_ascii_case("dialog") || role.eq_ignore_ascii_case("alertdialog")
            }) || ax
                .get(dom.backend[i])
                .and_then(|node| node["role"]["value"].as_str())
                .is_some_and(|role| matches!(role, "dialog" | "alertdialog"))
                || dom.name[i] == "DIALOG" && dom.attr(i, "open").is_some()
        })
        .collect();
    let semantic: HashSet<_> = declared.iter().copied().collect();
    let mut root: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        root[i] = parent(i).and_then(|p| root[p]).or_else(|| {
            (dom.is_element(i)
                && dom.layout[i].is_some()
                && (matches!(dom.style(i, 14), "fixed" | "sticky")
                    || shell.contains(&i)
                    || semantic.contains(&i)))
            .then_some(i)
        });
    }
    let fixed_at = |r: usize| dom.style(r, 14) == "fixed" || shell.contains(&r);
    let index: HashMap<i64, usize> = dom
        .backend
        .iter()
        .enumerate()
        .filter(|(_, b)| **b >= 0)
        .map(|(i, &b)| (b, i))
        .collect();
    // The layers, in the order of their tops on screen.
    let mut roots: Vec<(usize, f64)> = Vec::new();
    let mut seen = HashSet::new();
    for block in &page.blocks {
        if let Some(r) = index.get(&block.node).and_then(|&i| root[i])
            && seen.insert(r)
        {
            let rect = dom.rect(r).unwrap_or_default();
            let top = if fixed_at(r) {
                rect.y - dom.scroll_y
            } else if semantic.contains(&r) {
                rect.y
            } else {
                static_top(dom, r, rect)
            };
            roots.push((r, top));
        }
    }
    roots.sort_by(|a, b| a.1.total_cmp(&b.1));
    let view = dom.view_height;
    let quarter = if view > 0.0 {
        view / 4.0
    } else {
        f64::INFINITY
    };
    let mut pinned_to = 0.0f64;
    let mut layer_of = HashMap::new();
    for (r, top) in roots {
        let rect = dom.rect(r).unwrap_or_default();
        let fixed = fixed_at(r);
        let place = if fixed && view > 0.0 && (top >= view || top + rect.height <= 0.0) {
            Place::Off
        } else if (!fixed && semantic.contains(&r)) || rect.height > quarter {
            Place::Flow
        } else if top <= pinned_to + TOUCH {
            pinned_to = pinned_to.max(top + rect.height);
            Place::Pinned
        } else if fixed {
            Place::Docked
        } else {
            Place::Flow
        };
        let top = if fixed && place == Place::Flow {
            top.max(0.0)
        } else {
            top
        };
        layer_of.insert(r, page.layers.len());
        page.layers.push(Layer {
            node: dom.backend[r],
            fixed,
            rect,
            top,
            place,
        });
    }
    // A role belongs to source semantics, never inferred from a rectangle.
    // Keep the fixed root as the composited surface, including descendants
    // around a nested dialog, and avoid duplicate surfaces for one root.
    let mut dialog_roots = HashSet::new();
    for i in declared {
        let ax_node = ax.get(dom.backend[i]);
        let Some(r) = root[i] else { continue };
        let Some(&layer) = layer_of.get(&r) else {
            continue;
        };
        if page.layers[layer].place == Place::Off || !dialog_roots.insert(r) {
            continue;
        }
        let modal = dom
            .attr(i, "aria-modal")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"))
            || ax_node
                .and_then(|n| property(n, "modal"))
                .and_then(Value::as_bool)
                == Some(true);
        let name = ax_node
            .and_then(|n| ax_string(n, "name"))
            .or_else(|| dom.attr(i, "aria-label").map(clean))
            .unwrap_or_default();
        let colour = dom.style(r, 3).trim().to_ascii_lowercase();
        let mut node = Some(r);
        let mut solid = fully_opaque_color(&colour);
        while let Some(at) = node {
            if number(Some(dom.style(at, 8))).is_some_and(|alpha| alpha < 1.0) {
                solid = false;
            }
            node = parent(at);
        }
        let background = solid.then_some(colour);
        let paint_order = dom.layout[r]
            .and_then(|k| dom.paint.get(k))
            .copied()
            .unwrap_or_default();
        page.overlays.push(SourceOverlay {
            node: dom.backend[i],
            layer,
            dialog: true,
            modal,
            name,
            background,
            paint_order,
            foreground: Vec::new(),
        });
    }

    let is_descendant = |ancestor: usize, mut node: usize| {
        while let Some(next) = parent(node) {
            if next == ancestor {
                return true;
            }
            node = next;
        }
        false
    };
    // Large opaque fixed boxes without dialog semantics are visual layers,
    // but never gain modal focus or a dialog accessibility role from geometry.
    for (&r, &layer) in &layer_of {
        if dialog_roots.contains(&r) || !fixed_at(r) || page.layers[layer].place != Place::Flow {
            continue;
        }
        let colour = dom.style(r, 3).trim().to_ascii_lowercase();
        let mut solid = fully_opaque_color(&colour);
        let mut node = Some(r);
        while let Some(at) = node {
            if number(Some(dom.style(at, 8))).is_some_and(|alpha| alpha < 1.0) {
                solid = false;
            }
            node = parent(at);
        }
        if !solid {
            continue;
        }
        let paint_order = dom.layout[r]
            .and_then(|k| dom.paint.get(k))
            .copied()
            .unwrap_or_default();
        let rect = page.layers[layer].rect;
        let overlaps_lower = page.blocks.iter().any(|block| {
            let Some(&i) = index.get(&block.node) else {
                return false;
            };
            let Some(order) = dom.layout[i].and_then(|k| dom.paint.get(k)).copied() else {
                return false;
            };
            !is_descendant(r, i)
                && !is_descendant(i, r)
                && order < paint_order
                && block.rect.x < rect.right()
                && rect.x < block.rect.right()
                && block.rect.y < rect.bottom()
                && rect.y < block.rect.bottom()
        });
        if !overlaps_lower {
            continue;
        }
        page.overlays.push(SourceOverlay {
            node: dom.backend[r],
            layer,
            dialog: false,
            modal: false,
            name: String::new(),
            background: Some(colour),
            paint_order,
            foreground: Vec::new(),
        });
    }
    page.overlays.sort_by_key(|dialog| dialog.paint_order);
    let order = |i: usize| dom.layout[i].and_then(|k| dom.paint.get(k)).copied();
    let under = |a: usize, mut i: usize| {
        while let Some(p) = parent(i) {
            if p == a {
                return true;
            }
            i = p;
        }
        false
    };
    let area = |r: &Rect| r.width * r.height;
    let mut covers: Vec<(usize, Rect, i64)> = (0..n)
        .filter_map(|i| {
            if !dom.is_element(i)
                || root[i].is_some()
                || matches!(dom.name[i].as_str(), "HTML" | "BODY")
                || dom.concealed[i]
                || !dom.shows(i)
            {
                return None;
            }
            let (o, rect) = (order(i)?, dom.rect(i)?);
            let picture = PICTURE_TAGS.contains(&dom.name[i].as_str());
            (rect.width >= 4.0
                && rect.height >= 4.0
                && (picture || opaque(&dom.style(i, 3).trim().to_ascii_lowercase())))
            .then_some((i, rect, o))
        })
        .collect();
    covers.sort_by(|a, b| area(&b.1).total_cmp(&area(&a.1)));
    covers.truncate(MAX_COVERS);
    let media: Vec<(usize, Rect, Option<i64>)> = page
        .media
        .iter()
        .enumerate()
        .filter_map(|(m, media)| {
            let i = *index.get(&media.node)?;
            root[i].is_none().then(|| (m, media.rect, order(i)))
        })
        .collect();
    // Each node's innermost shelf, from the shelves' own nodes.
    let shelf_at: HashMap<usize, usize> = page
        .shelves
        .iter()
        .enumerate()
        .filter_map(|(k, s)| index.get(&s.node).map(|&i| (i, k)))
        .collect();
    let mut shelf_of: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        shelf_of[i] = shelf_at
            .get(&i)
            .copied()
            .or_else(|| parent(i).and_then(|p| shelf_of[p]));
    }
    for block in &mut page.blocks {
        let Some(&i) = index.get(&block.node) else {
            continue;
        };
        block.stack.shelf = shelf_of[i];
        if let Some(r) = root[i] {
            block.stack.layer = layer_of.get(&r).copied();
            continue;
        }
        let (Some(o), rect) = (order(i), block.rect) else {
            continue;
        };
        if rect.width <= 0.0 || rect.height <= 0.0 {
            continue;
        }
        block.stack.covered = covers.iter().any(|&(c, by, co)| {
            co > o && c != i && holds(by, rect) && !under(c, i) && !under(i, c)
        });
        if block.stack.covered {
            continue;
        }
        block.stack.over = media
            .iter()
            .filter(|(m, by, mo)| {
                page.media[*m].node != block.node
                    && mo.is_some_and(|mo| mo <= o)
                    && area(by) > area(&rect)
                    && holds(*by, rect)
            })
            .min_by(|a, b| area(&a.1).total_cmp(&area(&b.1)))
            .map(|(m, _, _)| *m);
    }
    // Preserve ordinary blocks above a fixed surface that can overlap it
    // horizontally while scrolling. They remain in the underlay; this bounded association allows
    // their visible cells and hit targets to be painted in source order.
    let mut foreground = Vec::new();
    for block in &page.blocks {
        let Some(&i) = index.get(&block.node) else {
            continue;
        };
        let Some(paint) = order(i) else { continue };
        if block.stack.layer.is_some() || block.stack.covered {
            continue;
        }
        let rect = block.rect;
        for (owner, overlay) in page.overlays.iter().enumerate() {
            let surface = page.layers[overlay.layer].rect;
            if paint > overlay.paint_order && rect.x < surface.right() && surface.x < rect.right() {
                foreground.push((owner, paint, block.node));
            }
        }
    }
    foreground.sort_by_key(|(_, paint, _)| *paint);
    for (owner, _, node) in foreground {
        page.overlays[owner].foreground.push(node);
    }
}

/// The bars of an app shell: a page that does not scroll itself (its
/// content fits the viewport) but scrolls its main box (`overflow-y: auto`
/// or `scroll`, the largest that overflows, at least a quarter of the
/// viewport) keeps the boxes above and under that box on screen, as if
/// fixed: a site's top bar and its bottom sign-up bar. They are the boxes
/// beside the main box's ancestors, wholly above or wholly under it.
fn shell_bars(dom: &Dom) -> HashSet<usize> {
    let view = dom.view_height;
    let mut bars = HashSet::new();
    let Some(page) = dom.rect(0) else {
        return bars;
    };
    if view <= 0.0 || dom.height > view + 1.0 {
        return bars;
    }
    let area = |r: Rect| r.width * r.height;
    let main = (0..dom.parent.len())
        .filter(|&i| dom.is_element(i) && matches!(dom.style(i, 10), "auto" | "scroll"))
        .filter_map(|i| Some((i, dom.rect(i)?)))
        .filter(|&(_, r)| area(r) >= area(page) / 4.0)
        .filter(|&(i, r)| {
            dom.descendants(i)
                .iter()
                .any(|&d| dom.rect(d).is_some_and(|b| b.bottom() > r.bottom() + 1.0))
        })
        .max_by(|a, b| area(a.1).total_cmp(&area(b.1)));
    let Some((main, rect)) = main else {
        return bars;
    };
    let mut on_chain = main;
    while let Some(up) = usize::try_from(dom.parent[on_chain])
        .ok()
        .filter(|&p| p < on_chain)
    {
        for &c in &dom.children[up] {
            if c == on_chain || !dom.is_element(c) {
                continue;
            }
            if let Some(b) = dom.rect(c)
                && b.height > 0.0
                && (b.bottom() <= rect.y + 1.0 || b.y >= rect.bottom() - 1.0)
            {
                bars.insert(c);
            }
        }
        on_chain = up;
    }
    bars
}

/// Where sticky box `r` sits in the flow when the page is at its top: as
/// reported when the page is not scrolled; else the top of the next box in
/// the flow under it less its height, or the bottom of the one before it,
/// or its parent's top; never under where it is now.
fn static_top(dom: &Dom, r: usize, rect: Rect) -> f64 {
    if dom.scroll_y <= 0.0 {
        return rect.y;
    }
    let Some(p) = usize::try_from(dom.parent[r]).ok().filter(|&p| p < r) else {
        return rect.y;
    };
    let in_flow = |s: usize| {
        s != r
            && dom.is_element(s)
            && !matches!(dom.style(s, 14), "absolute" | "fixed" | "sticky")
            && dom
                .rect(s)
                .is_some_and(|b| b.height > 0.0 && b.x < rect.right() && rect.x < b.right())
    };
    let siblings = &dom.children[p];
    let at = siblings.iter().position(|&s| s == r).unwrap_or(0);
    let top = if let Some(&next) = siblings[at..].iter().find(|&&s| in_flow(s)) {
        dom.rect(next).map_or(rect.y, |b| b.y - rect.height)
    } else if let Some(&before) = siblings[..at].iter().rev().find(|&&s| in_flow(s)) {
        dom.rect(before).map_or(rect.y, |b| b.bottom())
    } else {
        dom.rect(p).map_or(rect.y, |b| b.y)
    };
    top.min(rect.y)
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
    build_parts(snapshot, revisions, appended, false).0
}

/// Build an immutable page paired with its exact source relationships.
pub fn build_source(snapshot: &PageSnapshot, revisions: &mut Revisions) -> SourcePage {
    build_source_with(snapshot, revisions, &HashSet::new())
}

/// [`build_source`], recognizing the engine's appended scroll boxes as live logs.
pub fn build_source_with(snapshot: &PageSnapshot, revisions: &mut Revisions, appended: &HashSet<NodeId>) -> SourcePage {
    let (page, relations) = build_parts(snapshot, revisions, appended, true);
    SourcePage { page: Arc::new(page), relations: Arc::new(relations.unwrap()) }
}

fn build_parts(snapshot: &PageSnapshot, revisions: &mut Revisions, appended: &HashSet<NodeId>, source: bool) -> (Page, Option<source::SourceRelations>) {
    let dom = Dom::parse(&snapshot.dom);
    let ax = Ax::parse(&snapshot.ax);
    let root = dom.backend.first().copied().unwrap_or(-1);
    let document = revisions.document(&dom.url, root);
    let mut walker = Walker {
        source: source.then(Extraction::default),
        dom: &dom,
        ax: &ax,
        page: Page {
            url: truncate(&clean(&dom.url), 4096),
            title: truncate(&clean(&dom.title), 500),
            document,
            width: dom.width,
            height: dom.height,
            has_ax: !ax.by_node.is_empty(),
            background: page_background(&dom),
            ..Page::default()
        },
        runs: Vec::new(),
        rect: None,
        last: None,
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
        region: None,
        aria_hidden: false,
        name_only: false,
        twin: None,
    };
    if !dom.parent.is_empty() {
        walker.walk(0, &top);
    }
    walker.flush(&top);
    let mut page = walker.page;
    stack(&dom, &ax, &mut page);
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
    let relations = walker.source.map(|s| s.finish(&dom, &page));
    (page, relations)
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

    fn clip_style(f: &mut Fake, node: i64, at: usize, value: &str) {
        let k = f.layout.iter().position(|&n| n == node).unwrap();
        let empty = f.s("");
        let value = f.s(value);
        let count = f.styles[k].len().max(16).max(at + 1);
        f.styles[k].resize(count, empty);
        f.styles[k][at] = value;
    }

    fn clip_bounds(f: &mut Fake, node: i64, bounds: [f64; 4]) {
        let k = f.layout.iter().position(|&n| n == node).unwrap();
        f.bounds[k] = bounds.to_vec();
        clip_style(f, node, 14, "static");
    }

    fn clip_link(f: &mut Fake, parent: i64, text: &str, bounds: [f64; 4]) -> (i64, i64) {
        let link = f.el(parent, "A", &[("href", "/story")], "block");
        clip_bounds(f, link, bounds);
        let words = f.text(link, text);
        clip_bounds(f, words, bounds);
        let image = f.el(link, "IMG", &[("src", "/photo.png")], "block");
        clip_bounds(f, image, bounds);
        (100 + link, 100 + image)
    }

    #[test]
    fn overflow_clip_margin_requires_known_zero_before_rejecting_content() {
        for (margin, retained) in [
            (Some("40px"), true),
            (None, true),
            (Some(""), true),
            (Some("unknown"), true),
            (Some("border-box 40px"), true),
            (Some("0px"), false),
            (Some("border-box 0px"), false),
            (Some("padding-box 0px"), false),
            (Some("content-box 0px"), false),
        ] {
            for bounds in [[130.0, 40.0, 20.0, 18.0], [30.0, 100.0, 20.0, 18.0]] {
                for overflow in ["clip", "hidden"] {
                    let mut f = Fake::new();
                    let strip = f.el(0, "DIV", &[], "block");
                    clip_bounds(&mut f, strip, [20.0, 30.0, 100.0, 60.0]);
                    clip_style(&mut f, strip, 10, overflow);
                    clip_style(&mut f, strip, 15, overflow);
                    if let Some(margin) = margin {
                        clip_style(&mut f, strip, 16, margin);
                    }
                    let (link, image) = clip_link(&mut f, strip, "Margin", bounds);
                    let page = f.page(json!({"nodes": []}));
                    let visible = overflow == "clip" && retained;
                    assert_eq!(
                        page.controls.len(),
                        usize::from(visible),
                        "{overflow} {margin:?} {bounds:?}"
                    );
                    assert_eq!(page.media.len(), usize::from(visible));
                    if visible {
                        let c = &page.controls[0];
                        assert_eq!(page.check(&c.binding(&page)).unwrap().node, link);
                        assert_eq!(c.href.as_deref(), Some("/story"));
                        assert_eq!(page.media[0].node, image);
                    }
                    let marks = crate::layout::Marks {
                        hints: HashMap::from([(0, "m".into())]),
                        ..Default::default()
                    };
                    for columns in [40, 80, 120] {
                        let projected = crate::layout::project(&page, columns, (8.0, 18.0), &marks);
                        let spans: Vec<_> = projected.lines.iter().flat_map(|l| &l.spans).collect();
                        assert_eq!(spans.iter().any(|s| s.text.contains("Margin")), visible);
                        assert_eq!(
                            spans
                                .iter()
                                .any(|s| s.control == Some(0) && s.text.contains("[m]")),
                            visible
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn uncertain_clip_margin_does_not_mask_an_independent_hidden_clip() {
        for hidden_axis in [10, 15] {
            let mut f = Fake::new();
            let outer = f.el(0, "DIV", &[], "block");
            clip_bounds(&mut f, outer, [20.0, 30.0, 100.0, 60.0]);
            clip_style(&mut f, outer, hidden_axis, "hidden");
            clip_style(&mut f, outer, 16, "40px");
            let inner = f.el(outer, "DIV", &[], "block");
            clip_bounds(&mut f, inner, [20.0, 30.0, 100.0, 60.0]);
            clip_style(&mut f, inner, 10, "clip");
            clip_style(&mut f, inner, 15, "clip");
            clip_link(&mut f, inner, "Outside hidden", [130.0, 100.0, 20.0, 18.0]);
            let page = f.page(json!({"nodes": []}));
            assert!(page.controls.is_empty());
            assert!(page.media.is_empty());
            assert!(page.blocks.is_empty());
        }
    }

    #[test]
    fn overflow_clipped_content_is_absent_but_partial_and_other_axis_survive() {
        // An outer bound is a conservative clip edge; content touching it
        // from outside is hidden, but even a subpixel intersection survives.
        for (axis, hidden, partial, other_axis) in [
            (
                15,
                [120.0, 40.0, 20.0, 18.0],
                [119.375, 40.0, 20.0, 18.0],
                [30.0, 100.0, 20.0, 18.0],
            ),
            (
                10,
                [30.0, 90.0, 20.0, 18.0],
                [30.0, 89.375, 20.0, 18.0],
                [140.0, 40.0, 20.0, 18.0],
            ),
            (
                15,
                [0.0, 40.0, 20.0, 18.0],
                [0.625, 40.0, 20.0, 18.0],
                [30.0, 100.0, 20.0, 18.0],
            ),
            (
                10,
                [30.0, 12.0, 20.0, 18.0],
                [30.0, 12.625, 20.0, 18.0],
                [140.0, 40.0, 20.0, 18.0],
            ),
        ] {
            for overflow in ["hidden", "clip"] {
                let mut f = Fake::new();
                let strip = f.el(0, "DIV", &[], "block");
                clip_bounds(&mut f, strip, [20.0, 30.0, 100.0, 60.0]);
                clip_style(&mut f, strip, axis, overflow);
                clip_style(&mut f, strip, 16, "0px");
                let wrapper = f.node(strip, 1, "DIV", None, &[], None);
                let (hidden_link, hidden_image) = clip_link(&mut f, wrapper, "Concealed", hidden);
                let (partial_link, _) = clip_link(&mut f, wrapper, "Partial", partial);
                let (other_link, _) = clip_link(&mut f, wrapper, "Other axis", other_axis);
                let page = f.page(json!({"nodes": []}));
                assert_eq!(
                    page.controls.iter().map(|c| c.node).collect::<Vec<_>>(),
                    vec![partial_link, other_link]
                );
                assert_eq!(page.media.len(), 2);
                assert!(page.media.iter().all(|m| m.node != hidden_image));
                assert!(page.blocks.iter().all(|b| !b.text().contains("Concealed")));
                let marks = crate::layout::Marks {
                    hints: HashMap::from([(0, "p".into()), (1, "o".into())]),
                    ..Default::default()
                };
                for columns in [40, 80, 120] {
                    let projection = crate::layout::project(&page, columns, (8.0, 18.0), &marks);
                    let spans: Vec<_> = projection.lines.iter().flat_map(|l| &l.spans).collect();
                    assert!(spans.iter().all(|s| !s.text.contains("Concealed")));
                    assert!(
                        spans
                            .iter()
                            .any(|s| s.control == Some(0) && s.text.contains("Partial"))
                    );
                    assert!(
                        spans
                            .iter()
                            .any(|s| s.control == Some(0) && s.text.contains("[p]"))
                    );
                    assert!(
                        spans
                            .iter()
                            .filter_map(|s| s.control)
                            .all(|c| page.controls[c].node != hidden_link)
                    );
                    assert!(
                        projection
                            .regions
                            .iter()
                            .all(|r| page.media[r.media].node != hidden_image)
                    );
                }
                for c in &page.controls {
                    assert_eq!(c.href.as_deref(), Some("/story"));
                    assert_eq!(c.revision, 1);
                    assert_eq!(page.check(&c.binding(&page)).unwrap().node, c.node);
                }
            }
        }
    }

    #[test]
    fn overflow_clips_use_nested_current_bounds_without_reapplying_scroll() {
        let mut f = Fake::new();
        let outer = f.el(0, "DIV", &[], "block");
        clip_bounds(&mut f, outer, [20.0, 30.0, 100.0, 60.0]);
        clip_style(&mut f, outer, 15, "hidden");
        let inner = f.el(outer, "DIV", &[], "block");
        clip_bounds(&mut f, inner, [0.0, 30.0, 300.0, 60.0]);
        clip_style(&mut f, inner, 10, "hidden");
        clip_link(&mut f, inner, "Outside x", [130.0, 40.0, 20.0, 18.0]);
        clip_link(&mut f, inner, "Outside y", [30.0, 100.0, 20.0, 18.0]);
        let (kept, _) = clip_link(&mut f, inner, "Scrolled visible", [30.0, 40.0, 40.0, 18.0]);
        let mut snapshot = PageSnapshot {
            dom: f.snapshot(),
            ax: json!({"nodes": []}),
        };
        snapshot.dom["documents"][0]["scrollOffsetY"] = json!(200.0);
        snapshot.dom["documents"][0]["layout"]["scrollRects"] =
            json!(vec![[80.0, 200.0, 1000.0, 800.0]; f.layout.len()]);
        snapshot.dom["documents"][0]["layout"]["clientRects"] = json!(f.bounds);
        let page = build(&snapshot, &mut Revisions::new());
        assert_eq!(
            page.controls.iter().map(|c| c.node).collect::<Vec<_>>(),
            vec![kept]
        );
        assert_eq!(page.media.len(), 1);
        assert!(
            page.blocks
                .iter()
                .any(|b| b.text().contains("Scrolled visible"))
        );
    }

    #[test]
    fn overflow_clipped_static_ancestor_preserves_positioned_descendant_actions() {
        for position in ["fixed", "absolute", "relative", ""] {
            for ancestor_is_link in [false, true] {
                let mut f = Fake::new();
                let strip = f.el(0, "DIV", &[], "block");
                clip_bounds(&mut f, strip, [20.0, 30.0, 100.0, 60.0]);
                clip_style(&mut f, strip, 15, "hidden");
                let ancestor = f.el(
                    strip,
                    if ancestor_is_link { "A" } else { "DIV" },
                    &[("href", "/ancestor")],
                    "block",
                );
                clip_bounds(&mut f, ancestor, [200.0, 30.0, 100.0, 60.0]);
                let clipped_text = f.text(ancestor, "Hidden ancestor words");
                clip_bounds(&mut f, clipped_text, [200.0, 30.0, 100.0, 18.0]);
                let positioned = f.el(ancestor, "DIV", &[], "block");
                clip_bounds(&mut f, positioned, [240.0, 40.0, 60.0, 40.0]);
                clip_style(&mut f, positioned, 14, position);
                let (link, image, href) = if ancestor_is_link {
                    let text = f.text(positioned, "Escaped");
                    clip_bounds(&mut f, text, [240.0, 40.0, 60.0, 18.0]);
                    let image = f.el(positioned, "IMG", &[("src", "/photo.png")], "block");
                    clip_bounds(&mut f, image, [240.0, 40.0, 60.0, 18.0]);
                    (100 + ancestor, 100 + image, "/ancestor")
                } else {
                    let (link, image) =
                        clip_link(&mut f, positioned, "Escaped", [240.0, 40.0, 60.0, 18.0]);
                    (link, image, "/story")
                };
                let page = f.page(json!({"nodes": []}));
                assert!(
                    page.blocks
                        .iter()
                        .all(|b| !b.text().contains("Hidden ancestor words"))
                );
                let c = page
                    .controls
                    .iter()
                    .find(|c| c.node == link)
                    .expect("escaped source link");
                assert_eq!(page.check(&c.binding(&page)).unwrap().node, link);
                assert_eq!(c.href.as_deref(), Some(href));
                assert_eq!(c.revision, 1);
                assert!(page.media.iter().any(|m| m.node == image));
                assert!(
                    page.blocks
                        .iter()
                        .flat_map(|b| &b.runs)
                        .any(|r| r.text == "Escaped"
                            && r.control.is_some_and(|c| page.controls[c].node == link))
                );
            }
        }
    }

    #[test]
    fn overflow_on_inline_or_table_row_does_not_hide_descendants() {
        for display in ["inline", "table-row"] {
            let mut f = Fake::new();
            let ancestor = f.el(0, "DIV", &[], display);
            clip_bounds(&mut f, ancestor, [20.0, 30.0, 100.0, 60.0]);
            clip_style(&mut f, ancestor, 10, "hidden");
            clip_style(&mut f, ancestor, 15, "hidden");
            let (link, _) = clip_link(&mut f, ancestor, "Unclipped", [200.0, 200.0, 60.0, 18.0]);
            let page = f.page(json!({"nodes": []}));
            assert_eq!(
                page.controls.iter().map(|c| c.node).collect::<Vec<_>>(),
                vec![link]
            );
            assert_eq!(page.media.len(), 1);
            assert!(page.blocks.iter().any(|b| b.text().contains("Unclipped")));
        }
    }

    #[test]
    fn overflow_clipped_price_pair_stays_absent_with_twins_on_or_off() {
        use crate::adapt::{Adapt, Rule};
        for group_x in [20.0, 200.0] {
            for spoken_x in [group_x, -10000.0] {
                let mut f = Fake::new();
                let strip = f.el(0, "DIV", &[], "block");
                clip_bounds(&mut f, strip, [0.0, 0.0, 120.0, 90.0]);
                clip_style(&mut f, strip, 15, "hidden");
                let row = f.el(strip, "DIV", &[], "block");
                clip_bounds(&mut f, row, [group_x, 30.0, 60.0, 30.0]);
                let spoken = f.el(row, "SPAN", &[], "block");
                clip_bounds(&mut f, spoken, [spoken_x, 30.0, 1.0, 1.0]);
                clip_style(&mut f, spoken, 14, "absolute");
                clip_style(&mut f, spoken, 12, "rect(0px, 0px, 0px, 0px)");
                clip_style(&mut f, spoken, 10, "hidden");
                let words = f.text(spoken, "$29.99");
                clip_bounds(&mut f, words, [spoken_x, 31.0, 40.0, 18.0]);
                let shown = f.el(row, "SPAN", &[("aria-hidden", "true")], "block");
                clip_bounds(&mut f, shown, [group_x, 30.0, 40.0, 18.0]);
                let parts = f.text(shown, "$2999");
                clip_bounds(&mut f, parts, [group_x, 30.0, 40.0, 18.0]);
                let page = f.page(json!({"nodes": []}));
                if group_x == 20.0 {
                    // The original canonical text survives even if its hidden
                    // source representation is positioned offscreen.
                    assert!(page.text().contains("$29.99"));
                }
                for twins in [true, false] {
                    let marks = crate::layout::Marks {
                        adapt: Adapt::default().with(Rule::ScreenReaderTwins, twins),
                        ..Default::default()
                    };
                    for columns in [40, 80, 120] {
                        let projection =
                            crate::layout::project(&page, columns, (8.0, 18.0), &marks);
                        let text = projection
                            .lines
                            .iter()
                            .map(|l| l.plain())
                            .collect::<Vec<_>>()
                            .join("\n");
                        if group_x == 200.0 {
                            assert!(!text.contains("29"), "clipped price: {text}");
                        } else if spoken_x == group_x || !twins {
                            // Relocating an independently blocked offscreen
                            // canonical representation is a separate layout concern.
                            let (wanted, excluded) = if twins {
                                ("$29.99", "$2999")
                            } else {
                                ("$2999", "$29.99")
                            };
                            assert!(
                                text.contains(wanted) && !text.contains(excluded),
                                "visible price: {text}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn overflow_clip_preserves_explicit_veil_semantic_twins() {
        let mut f = Fake::new();
        let row = f.el(0, "DIV", &[], "block");
        clip_bounds(&mut f, row, [20.0, 30.0, 100.0, 60.0]);
        let spoken = f.el(row, "SPAN", &[], "block");
        clip_bounds(&mut f, spoken, [20.0, 30.0, 1.0, 1.0]);
        clip_style(&mut f, spoken, 14, "absolute");
        clip_style(&mut f, spoken, 12, "rect(0px, 0px, 0px, 0px)");
        clip_style(&mut f, spoken, 10, "hidden");
        clip_style(&mut f, spoken, 15, "hidden");
        let words = f.text(spoken, "$29.99");
        clip_bounds(&mut f, words, [20.0, 31.0, 40.0, 18.0]);
        let shown = f.el(row, "SPAN", &[("aria-hidden", "true")], "block");
        clip_bounds(&mut f, shown, [20.0, 30.0, 40.0, 18.0]);
        let parts = f.text(shown, "$2999");
        clip_bounds(&mut f, parts, [20.0, 30.0, 40.0, 18.0]);
        let page = f.page(json!({"nodes": []}));
        assert!(
            page.blocks
                .iter()
                .flat_map(|b| &b.runs)
                .any(|r| { r.text == "$29.99" && r.twin == Some(Twin::Spoken) })
        );
        assert!(
            page.blocks
                .iter()
                .flat_map(|b| &b.runs)
                .any(|r| { r.text == "$2999" && r.twin == Some(Twin::Shown) })
        );
        assert!(page.text().contains("$29.99"));
        assert!(!page.text().contains("$2999"));
    }

    #[test]
    fn overflow_on_root_body_or_boxless_ancestor_does_not_clip_to_viewport() {
        for tag in ["HTML", "BODY", "DIV"] {
            let mut f = Fake::new();
            let ancestor = if tag == "DIV" {
                f.node(0, 1, tag, None, &[], None)
            } else {
                let n = f.el(0, tag, &[], "block");
                clip_bounds(&mut f, n, [0.0, 0.0, 100.0, 60.0]);
                clip_style(&mut f, n, 10, "hidden");
                clip_style(&mut f, n, 15, "hidden");
                n
            };
            let (link, _) = clip_link(&mut f, ancestor, "Adaptable", [200.0, 200.0, 60.0, 18.0]);
            let page = f.page(json!({"nodes": []}));
            assert_eq!(page.controls[0].node, link);
            assert_eq!(page.media.len(), 1);
            assert!(page.blocks.iter().any(|b| b.text().contains("Adaptable")));
        }
    }

    #[test]
    fn boxless_link_retains_visible_descendants_and_source_binding() {
        let mut f = Fake::new();
        let link = f.node(0, 1, "A", None, &[("href", "/story")], None);
        let wrapper = f.node(link, 1, "DIV", None, &[], None);
        let image = f.el(
            wrapper,
            "IMG",
            &[("src", "/photo.png"), ("alt", "Scene")],
            "block",
        );
        *f.bounds.last_mut().unwrap() = vec![16.0, 36.0, 160.0, 90.0];
        let text = f.text(wrapper, "Read the story");
        *f.bounds.last_mut().unwrap() = vec![16.0, 144.0, 160.0, 18.0];
        let hidden = f.text(wrapper, "Hidden extra words");
        let hidden_style = f.s("hidden");
        f.styles.last_mut().unwrap()[1] = hidden_style;
        *f.bounds.last_mut().unwrap() = vec![500.0, 500.0, 160.0, 18.0];
        let url = f.s("https://example.test/news/");
        let mut snapshot = PageSnapshot {
            dom: f.snapshot(),
            ax: json!({"nodes": [{"backendDOMNodeId": 100 + link,
                "role": {"value": "link"}, "name": {"value": "Read the full original story"}}]}),
        };
        snapshot.dom["documents"][0]["documentURL"] = json!(url);
        let mut revisions = Revisions::new();
        let page = build(&snapshot, &mut revisions);
        assert_eq!(page.media.len(), 1);
        assert_eq!(page.media[0].node, 100 + image);
        assert_eq!(page.media[0].src, "https://example.test/photo.png");
        assert!(page.media[0].in_label);
        assert_eq!(page.controls.len(), 1);
        let control = &page.controls[0];
        assert_eq!(control.node, 100 + link);
        assert_eq!(control.role, Role::Link);
        assert_eq!(control.name, "Read the full original story");
        assert_eq!(control.href.as_deref(), Some("https://example.test/story"));
        assert_eq!(control.rect, Rect::new(16.0, 36.0, 160.0, 126.0));
        assert_eq!(control.revision, 1);
        assert!(control.from_ax);
        let binding = control.binding(&page);
        assert_eq!(page.check(&binding).unwrap().node, 100 + link);
        let runs: Vec<_> = page.blocks.iter().flat_map(|b| &b.runs).collect();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "Read the story");
        assert_eq!(runs[0].control, Some(0));
        let marks = crate::layout::Marks {
            hints: HashMap::from([(0, "a".into())]),
            ..Default::default()
        };
        for columns in [40, 80, 120] {
            let projection = crate::layout::project(&page, columns, (8.0, 18.0), &marks);
            let spans: Vec<_> = projection.lines.iter().flat_map(|l| &l.spans).collect();
            assert!(
                spans
                    .iter()
                    .any(|s| s.control == Some(0) && s.text.contains("Read"))
            );
            assert!(
                spans
                    .iter()
                    .any(|s| s.control == Some(0) && s.text.contains("[a]"))
            );
        }
        // A geometry-only change keeps the source action's revision.
        let text_layout = f.layout.iter().position(|&n| n == text).unwrap();
        snapshot.dom["documents"][0]["layout"]["bounds"][text_layout][1] = json!(180.0);
        let moved = build(&snapshot, &mut revisions);
        assert_eq!(moved.check(&binding).unwrap().revision, 1);
        // A new destination invalidates the old action, on the same source node.
        let destination = f.s("/other");
        snapshot.dom["strings"] = json!(f.strings);
        snapshot.dom["documents"][0]["nodes"]["attributes"][link as usize][1] = json!(destination);
        let changed = build(&snapshot, &mut revisions);
        assert_eq!(changed.controls[0].node, 100 + link);
        assert_eq!(changed.controls[0].revision, 2);
        assert!(changed.check(&binding).is_err());
        assert!(page.blocks.iter().all(|b| b.node != 100 + hidden));
        // An image-only link still exposes the original guarded action.
        snapshot.dom["documents"][0]["layout"]["styles"][text_layout][1] = json!(hidden_style);
        let image_only = build(&snapshot, &mut revisions);
        assert_eq!(image_only.media.len(), 1);
        let control = &image_only.controls[0];
        assert_eq!(control.rect, Rect::new(16.0, 36.0, 160.0, 90.0));
        assert_eq!(
            image_only
                .check(&control.binding(&image_only))
                .unwrap()
                .node,
            100 + link
        );
    }

    #[test]
    fn boxless_link_retains_picture_background_and_source_binding() {
        let mut f = Fake::new();
        let link = f.node(0, 1, "A", None, &[("href", "/story")], None);
        let picture = f.el(link, "PICTURE", &[], "block");
        *f.bounds.last_mut().unwrap() = vec![16.0, 36.0, 160.0, 90.0];
        let background = f.s("url(/photo.png)");
        f.styles.last_mut().unwrap().push(background);
        let url = f.s("https://example.test/news/");
        let mut snapshot = PageSnapshot {
            dom: f.snapshot(),
            ax: json!({"nodes": [{"backendDOMNodeId": 100 + link,
                "role": {"value": "link"}, "name": {"value": "Read the original story"}}]}),
        };
        snapshot.dom["documents"][0]["documentURL"] = json!(url);
        let page = build(&snapshot, &mut Revisions::new());
        assert_eq!(page.media.len(), 1);
        assert_eq!(page.media[0].node, 100 + picture);
        assert_eq!(page.media[0].kind, MediaKind::Image);
        assert_eq!(page.media[0].src, "https://example.test/photo.png");
        assert_eq!(page.media[0].rect, Rect::new(16.0, 36.0, 160.0, 90.0));
        assert!(page.media[0].in_label);
        assert_eq!(page.controls.len(), 1);
        let control = &page.controls[0];
        assert_eq!(control.node, 100 + link);
        assert_eq!(control.role, Role::Link);
        assert_eq!(control.name, "Read the original story");
        assert_eq!(control.href.as_deref(), Some("https://example.test/story"));
        assert_eq!(control.rect, Rect::new(16.0, 36.0, 160.0, 90.0));
        assert_eq!(control.revision, 1);
        assert!(control.from_ax);
        assert_eq!(
            page.check(&control.binding(&page)).unwrap().node,
            100 + link
        );
    }

    #[test]
    fn boxless_link_without_visible_content_emits_no_action_or_content() {
        for visibility in [
            "no-layout",
            "hidden",
            "transparent",
            "empty-wrapper",
            "empty-picture",
        ] {
            let mut f = Fake::new();
            let link = f.node(0, 1, "A", None, &[("href", "/story")], None);
            let wrapper = f.el(
                link,
                if visibility == "empty-picture" {
                    "PICTURE"
                } else {
                    "DIV"
                },
                &[],
                "block",
            );
            if !visibility.starts_with("empty-") {
                let display = (visibility != "no-layout").then_some("inline");
                f.node(wrapper, 1, "IMG", None, &[("src", "/photo.png")], display);
                f.node(wrapper, 3, "#text", Some("Not visible"), &[], display);
                if matches!(visibility, "hidden" | "transparent") {
                    let (offset, value) = if visibility == "hidden" {
                        (1, "hidden")
                    } else {
                        (8, "0")
                    };
                    let style = f.s(value);
                    for styles in &mut f.styles[1..] {
                        styles[offset] = style;
                    }
                }
            }
            let page = f.page(Value::Null);
            assert!(page.controls.is_empty(), "{visibility}");
            assert!(page.media.is_empty(), "{visibility}");
            assert!(page.blocks.is_empty(), "{visibility}");
        }
    }

    #[test]
    fn image_source_ignores_empty_selected_urls_and_preserves_real_selections() {
        const TINY: &str =
            "data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7";
        for (selected, declared, expected) in [
            (
                Some(""),
                Some("../photo.png"),
                "https://example.test/photo.png",
            ),
            (
                Some(" \t\n"),
                Some("../photo.png"),
                "https://example.test/photo.png",
            ),
            (None, Some("../photo.png"), "https://example.test/photo.png"),
            (
                Some("https://images.test/responsive.png"),
                Some("../photo.png"),
                "https://images.test/responsive.png",
            ),
            (Some(TINY), Some("../photo.png"), TINY),
            (Some(""), Some(""), ""),
            (Some(" \t"), Some(" \t"), ""),
            (None, None, ""),
        ] {
            let mut f = Fake::new();
            let attrs: Vec<_> = declared.map(|src| ("src", src)).into_iter().collect();
            let image = f.el(0, "IMG", &attrs, "block");
            let url = f.s("https://example.test/news/");
            let selected = selected.map(|src| f.s(src));
            let mut snapshot = PageSnapshot {
                dom: f.snapshot(),
                ax: Value::Null,
            };
            snapshot.dom["documents"][0]["documentURL"] = json!(url);
            if let Some(selected) = selected {
                snapshot.dom["documents"][0]["nodes"]["currentSourceURL"] =
                    json!({"index": [image], "value": [selected]});
            }
            let page = build(&snapshot, &mut Revisions::new());
            assert_eq!(page.media.len(), 1);
            assert_eq!(
                page.media[0].src, expected,
                "selected={selected:?}, declared={declared:?}"
            );
            assert_eq!(page.media[0].pending, expected.is_empty());
        }
    }

    #[test]
    fn small_image_without_a_source_stays_absent_without_a_reservation() {
        for source in [None, Some(""), Some(" \t\n")] {
            let mut f = Fake::new();
            let attrs: Vec<_> = source.map(|src| ("src", src)).into_iter().collect();
            let image = f.el(0, "IMG", &attrs, "inline");
            *f.bounds.last_mut().unwrap() = vec![16.0, 36.0, 1.0, 1.0];
            f.text(0, "Adjacent content");
            let url = f.s("https://example.test/news/");
            let selected = source.map(|src| f.s(src));
            let mut snapshot = PageSnapshot {
                dom: f.snapshot(),
                ax: Value::Null,
            };
            snapshot.dom["documents"][0]["documentURL"] = json!(url);
            if let Some(selected) = selected {
                snapshot.dom["documents"][0]["nodes"]["currentSourceURL"] =
                    json!({"index": [image], "value": [selected]});
            }
            let page = build(&snapshot, &mut Revisions::new());
            assert_eq!(page.media.len(), 1);
            assert_eq!(page.media[0].src, "");
            assert!(!page.media[0].pending);
        }
    }

    #[test]
    fn source_interactive_log_retains_control_and_media_ancestry() {
        let mut f = Fake::new();
        let log = f.el(0, "DIV", &[("role", "log")], "block");
        let entry = f.el(log, "DIV", &[], "block");
        let link = f.el(entry, "A", &[("href", "/guarded")], "inline");
        let image = f.el(link, "IMG", &[("src", "/a.png")], "block");
        let sibling = f.el(log, "DIV", &[], "block");
        f.text(sibling, "another entry");
        let source = build_source(&PageSnapshot { dom: f.snapshot(), ax: Value::Null }, &mut Revisions::new());
        assert_eq!(source.page().controls.len(), 1);
        assert_eq!(source.relations.media_controls, [Some(0)]);
        let index = |node| source.relations.nodes.iter().position(|n| n.node == node).expect("interactive log source remains available");
        assert!(source.relations.contains(index(100 + link), index(100 + image)));
        assert!(source.relations.contains(index(100 + log), index(100 + link)));
    }

    #[test]
    fn source_fixed_log_contents_do_not_change_outer_memberships() {
        let mut f = Fake::new();
        let log = f.el(0, "DIV", &[("role", "log")], "block");
        let entry = f.el(log, "DIV", &[], "block");
        f.text(entry, "first");
        let paragraph = f.el(0, "P", &[], "block");
        f.text(paragraph, "Outside");
        let a = build_source(&PageSnapshot { dom: f.snapshot(), ax: Value::Null }, &mut Revisions::new());
        let next = f.el(log, "DIV", &[], "block");
        f.text(next, "second");
        let b = build_source(&PageSnapshot { dom: f.snapshot(), ax: Value::Null }, &mut Revisions::new());
        assert_ne!(a.page().logs, b.page().logs);
        assert_eq!(a.relations, b.relations, "fixed log contents are an opaque layout unit");
    }

    #[test]
    fn source_table_cell_ranges_follow_inline_folding_and_caption_trimming() {
        let mut f = Fake::new();
        let table = f.el(0, "TABLE", &[], "table");
        let caption = f.el(table, "CAPTION", &[], "table-caption");
        let caption_text = f.node(caption, 3, "#text", Some("  Résumé  "), &[], Some("inline"));
        let row = f.el(table, "TR", &[], "table-row");
        let cell = f.el(row, "TD", &[], "table-cell");
        let p = f.el(cell, "P", &[], "block");
        let first = f.node(p, 3, "#text", Some("  naïve  "), &[], Some("inline"));
        let strong = f.el(p, "STRONG", &[], "inline");
        let second = f.node(strong, 3, "#text", Some(" 世界 "), &[], Some("inline"));
        let p = f.el(cell, "P", &[], "block");
        let third = f.node(p, 3, "#text", Some(" fin "), &[], Some("inline"));
        let snapshot = PageSnapshot { dom: f.snapshot(), ax: Value::Null };
        let source = build_source(&snapshot, &mut Revisions::new());
        assert_eq!(source.page().as_ref(), &build(&snapshot, &mut Revisions::new()));
        let read = |node| source.relations.runs.iter().filter(|r| r.node == node).map(|r| r.text(source.page())).collect::<Vec<_>>();
        assert_eq!(read(caption_text as usize), ["Résumé"]);
        assert_eq!(read(first as usize), ["naïve "]);
        assert_eq!(read(second as usize), ["世界"]);
        assert_eq!(read(third as usize), ["fin"]);
    }

    #[test]
    fn source_tables_folded_inside_logs_leave_no_dangling_cell_addresses() {
        let mut f = Fake::new();
        let log = f.el(0, "DIV", &[("role", "log")], "block");
        let entry = f.el(log, "DIV", &[], "block");
        let table = f.el(entry, "TABLE", &[], "table");
        let row = f.el(table, "TR", &[], "table-row");
        let cell = f.el(row, "TD", &[], "table-cell");
        f.text(cell, "A cell");
        let second = f.el(entry, "TABLE", &[], "table");
        let row = f.el(second, "TR", &[], "table-row");
        let cell = f.el(row, "TD", &[], "table-cell");
        f.text(cell, "Another cell");
        let sibling = f.el(log, "DIV", &[], "block");
        f.text(sibling, "An ordinary entry");
        let snapshot = PageSnapshot { dom: f.snapshot(), ax: Value::Null };
        let source = build_source(&snapshot, &mut Revisions::new());
        assert_eq!(source.page().as_ref(), &build(&snapshot, &mut Revisions::new()));
        assert!(source.relations.runs.is_empty());
    }

    #[test]
    fn source_retains_the_existing_twin_match_and_only_its_exact_run_ranges() {
        let mut f = Fake::new();
        let p = f.el(0, "P", &[], "block");
        f.text(p, "Before ");
        let spoken = f.el(p, "SPAN", &[], "inline");
        clip_style(&mut f, spoken, 8, "0");
        let said = f.node(spoken, 3, "#text", Some("$29.99"), &[], Some("inline"));
        let shown = f.el(p, "SPAN", &[("aria-hidden", "true")], "inline");
        let visual = f.node(shown, 3, "#text", Some("$2999"), &[], Some("inline"));
        f.text(p, " After");
        let source = build_source(&PageSnapshot { dom: f.snapshot(), ax: Value::Null }, &mut Revisions::new());
        assert_eq!(source.relations.twins, [(spoken as usize, shown as usize)]);
        let ranges = |node| source.relations.runs.iter().filter(|r| r.node == node).map(|r| r.text(source.page())).collect::<Vec<_>>();
        assert_eq!(ranges(said as usize), ["$29.99"]);
        assert_eq!(ranges(visual as usize), ["$2999"]);
        assert!(source.page().text().contains("Before"));
        assert!(source.page().text().contains("After"));
    }

    #[test]
    fn source_pieces_survive_unicode_cleaning_merging_and_block_splits() {
        let mut f = Fake::new();
        let figure = f.el(0, "FIGURE", &[], "block");
        let p = f.el(figure, "P", &[], "block");
        let first = f.node(p, 3, "#text", Some("  café\u{1b}  "), &[], Some("inline"));
        let second = f.node(p, 3, "#text", Some(" 世界 "), &[], Some("inline"));
        let image = f.el(p, "IMG", &[("src", "/a.png")], "block");
        let third = f.node(p, 3, "#text", Some(" après "), &[], Some("inline"));
        let caption = f.el(figure, "FIGCAPTION", &[], "block");
        f.text(caption, "Caption");
        let snapshot = PageSnapshot { dom: f.snapshot(), ax: Value::Null };
        let source = build_source(&snapshot, &mut Revisions::new());
        let legacy = build(&snapshot, &mut Revisions::new());
        assert_eq!(source.page().as_ref(), &legacy);
        let clone = source.clone();
        assert!(Arc::ptr_eq(source.page(), clone.page()));
        assert!(Arc::ptr_eq(&source.relations, &clone.relations));
        let pieces = &source.relations.runs;
        let texts = |node| pieces.iter().filter(|p| p.node == node).map(|p| {
            p.text(source.page()).to_string()
        }).collect::<Vec<_>>();
        assert_eq!(texts(first as usize), ["café "]);
        assert_eq!(texts(second as usize), ["世界"]);
        assert_eq!(texts(third as usize), ["après"]);
        let owned = &source.relations.figures[0];
        assert_eq!(owned.node, figure as usize);
        assert_eq!(owned.captions, [caption as usize]);
        assert!(owned.blocks.iter().any(|&b| matches!(source.page().blocks[b].kind, BlockKind::Media(m) if source.page().media[m].node == 100 + image)));
        assert_eq!(source.relations.blocks.iter().filter(|b| b.node == p as usize).count(), 2);
    }

    #[test]
    fn source_relationship_only_changes_are_distinct_and_boxless_ownership_is_exact() {
        let mut f = Fake::new();
        let grid = f.el(0, "DIV", &[], "grid");
        let left = f.node(grid, 1, "DIV", None, &[], None);
        let right = f.node(grid, 1, "DIV", None, &[], None);
        let link = f.node(left, 1, "A", None, &[("href", "/original")], None);
        let figure = f.el(link, "FIGURE", &[], "block");
        let image = f.el(figure, "IMG", &[("src", "/a.png")], "block");
        let nested = f.el(figure, "FIGURE", &[], "block");
        f.el(nested, "IMG", &[("src", "/b.png")], "block");
        let caption = f.el(figure, "FIGCAPTION", &[], "block");
        f.text(caption, "Caption");
        let mut snapshot = PageSnapshot { dom: f.snapshot(), ax: Value::Null };
        let a = build_source(&snapshot, &mut Revisions::new());
        assert_eq!(a.relations.grids[0].node, grid as usize);
        assert_eq!(a.relations.grids[0].items, [left as usize, right as usize]);
        assert_eq!(a.relations.media_controls[0], Some(0));
        assert_eq!(a.page().media[0].node, 100 + image);
        assert_eq!(a.relations.figures[1].parent, Some(figure as usize));
        snapshot.dom["documents"][0]["nodes"]["parentIndex"][link as usize] = json!(right);
        let b = build_source(&snapshot, &mut Revisions::new());
        assert_eq!(a.page(), b.page());
        assert_ne!(a.relations, b.relations);
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
    fn a_table_that_lays_out_tables_is_read_as_blocks() {
        // Hacker News: an outer table whose cell holds the stories' table.
        let mut f = Fake::new();
        let body = f.el(0, "BODY", &[], "block");
        let outer = f.el(body, "TABLE", &[], "table");
        let tr = f.el(outer, "TR", &[], "table-row");
        let td = f.el(tr, "TD", &[], "table-cell");
        let inner = f.el(td, "TABLE", &[], "table");
        let row = f.el(inner, "TR", &[], "table-row");
        let rank = f.el(row, "TD", &[], "table-cell");
        f.text(rank, "1.");
        let title = f.el(row, "TD", &[], "table-cell");
        f.text(title, "A story");
        let tr2 = f.el(outer, "TR", &[], "table-row");
        let foot = f.el(tr2, "TD", &[], "table-cell");
        f.text(foot, "Guidelines");
        // A table that says it lays out is read as blocks too.
        let said = f.el(body, "TABLE", &[("role", "presentation")], "table");
        let tr3 = f.el(said, "TR", &[], "table-row");
        let cell = f.el(tr3, "TD", &[], "table-cell");
        f.text(cell, "Laid out");
        let page = f.page(Value::Null);
        let tables: Vec<&Table> = page
            .blocks
            .iter()
            .filter_map(|b| match &b.kind {
                BlockKind::Table(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(tables.len(), 1, "only the stories' table is a table");
        let cells: Vec<String> = tables[0].rows[0].iter().map(|c| plain(&c.runs)).collect();
        assert_eq!(cells, ["1.", "A story"]);
        let texts: Vec<String> = page
            .blocks
            .iter()
            .filter(|b| !matches!(b.kind, BlockKind::Table(_)))
            .map(|b| b.text())
            .collect();
        assert_eq!(texts, ["Guidelines", "Laid out"]);
    }

    #[test]
    fn a_box_with_no_area_does_not_move_its_block() {
        // A footer link far down the page, with an empty text box at the
        // origin beside its real one (as MDN's and GitHub's have).
        let mut f = Fake::new();
        let body = f.el(0, "BODY", &[], "block");
        let p = f.el(body, "P", &[], "block");
        f.text(p, "About");
        let k = f.bounds.len() - 1;
        f.bounds[k] = vec![10.0, 5000.0, 50.0, 18.0];
        f.text(p, " ");
        let k = f.bounds.len() - 1;
        f.bounds[k] = vec![0.0, 0.0, 0.0, 0.0];
        let page = f.page(Value::Null);
        assert_eq!(page.blocks.len(), 1);
        let rect = page.blocks[0].rect;
        assert_eq!((rect.x, rect.y), (10.0, 5000.0));
    }

    #[test]
    fn the_page_root_a_script_appends_to_is_not_a_log() {
        let mut f = Fake::new();
        let html = f.el(0, "HTML", &[], "block");
        let body = f.el(html, "BODY", &[], "block");
        let p = f.el(body, "P", &[], "block");
        f.text(p, "Headline");
        let chat = f.el(body, "DIV", &[], "block");
        let line = f.el(chat, "DIV", &[], "block");
        f.text(line, "hello");
        let backend = |n: i64| 100 + n;
        let appended: HashSet<NodeId> = [backend(html), backend(body), backend(chat)].into();
        let snapshot = PageSnapshot {
            dom: f.snapshot(),
            ax: Value::Null,
        };
        let page = build_with(&snapshot, &mut Revisions::new(), &appended);
        let kinds: Vec<_> = page
            .blocks
            .iter()
            .map(|b| matches!(b.kind, BlockKind::Log(_)))
            .collect();
        assert_eq!(kinds, [false, true], "the paragraph, then the chat's log");
        assert_eq!(page.logs.len(), 1);
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

//! Frames: a rendered result as rows of styled runs (#226).
//!
//! A [`Frame`] is built from the segment stream core already returns, so no
//! renderable changes. It keeps that stream's shape (one [`Run`] per segment
//! piece) over one text arena, with every style stored once in a
//! [`StyleTable`]. From it:
//!
//! - [`Frame::to_ansi`] writes exactly what `Console::segments_to_string`
//!   writes for the same (control-free) stream, byte for byte;
//! - [`Frame::to_ansi_merged`] merges adjacent runs of one style first, which
//!   is smaller but not upstream's bytes (opt-in, for live repaint and our own
//!   exports);
//! - [`Frame::cells`] derives one cell per grapheme on demand, with core's
//!   widths;
//! - [`Frame::diff`] lists the cells that changed against a previous frame.
//!
//! Control segments are dropped: a frame holds content, and whatever paints
//! it owns cursor movement. See `docs/design/render-tree.md`. Terminal
//! graphics travel beside the cells as [`Placement`]s (see [`graphics`]).
//!
//! A frame can also carry semantic [`Region`]s (see [`regions`]) and export
//! itself as HTML or SVG with links kept ([`Frame::to_html`],
//! [`Frame::to_svg`]).

mod export;
pub mod graphics;
pub mod regions;

pub use export::{HtmlOptions, SvgOptions};
pub use graphics::{plan as plan_graphics, Graphic, GraphicsPlan, Placement, PlacementSource};
pub use regions::{render_frame, role_name, Rect, Region, RegionRecorder, Span};

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};
use std::ops::Range;

use rich::cells::{cell_len, split_graphemes};
use rich::protocol::RegionId;
use rich::{ColorSystem, Console, Segment, Style};

/// An index into a [`StyleTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StyleId(u32);

impl StyleId {
    /// "No style": a segment whose `style` is `None`.
    pub const NONE: StyleId = StyleId(0);

    /// No style's id: what a [`StyleRemap`] gives a dropped style, so a
    /// copy of cells that still holds it differs from every cell.
    pub const GONE: StyleId = StyleId(u32::MAX);

    pub fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) fn from_index(index: usize) -> StyleId {
        StyleId(index as u32)
    }
}

/// A colour as three bytes: red, green and blue.
pub type Rgb = [u8; 3];

/// Styles, stored once each. Runs refer to them by [`StyleId`].
///
/// Interning hashes a style's public parts (colours, attributes and link)
/// with a fast non-cryptographic hash and confirms a match with `==`, so
/// metadata still tells styles apart. A style made of two plain colours can
/// be interned by the colours alone ([`intern_rgb`](Self::intern_rgb)),
/// without building a [`Style`] unless it is new.
///
/// The table only grows while it is used; a table kept between frames (a
/// [`Screen`](crate::surface::Screen)'s) drops what its cells no longer
/// use with [`retain`](Self::retain).
#[derive(Clone, Debug)]
pub struct StyleTable {
    styles: Vec<Option<Style>>,
    /// Each style's key (see [`style_key`]), to rebuild the index.
    keys: Vec<u64>,
    /// Each style's colour pair when it was interned as one (see
    /// [`pair_key`]), else 0.
    pairs: Vec<u64>,
    /// Per id, the next id whose key is the same (0 ends the chain:
    /// [`StyleId::NONE`] is never in one).
    chain: Vec<u32>,
    /// The first id with each key.
    index: HashMap<u64, u32, FxBuild>,
    /// Colour pairs interned with [`intern_rgb`](Self::intern_rgb).
    rgb: HashMap<u64, StyleId, FxBuild>,
    /// The last style interned: consecutive segments usually share one.
    last: StyleId,
    /// How many styles the last [`retain`](Self::retain) kept.
    kept: usize,
}

impl Default for StyleTable {
    fn default() -> Self {
        StyleTable {
            styles: vec![None],
            keys: vec![0],
            pairs: vec![0],
            chain: vec![0],
            index: HashMap::default(),
            rgb: HashMap::default(),
            last: StyleId::NONE,
            kept: 1,
        }
    }
}

impl StyleTable {
    pub fn new() -> Self {
        StyleTable::default()
    }

    /// The id of `style`, adding it if it is new. `None` is [`StyleId::NONE`].
    pub fn intern(&mut self, style: Option<&Style>) -> StyleId {
        let Some(style) = style else {
            return StyleId::NONE;
        };
        if self.styles[self.last.index()].as_ref() == Some(style) {
            return self.last;
        }
        let key = style_key(style);
        let id = match self.find(key, style) {
            Some(id) => id,
            None => self.push(key, style.clone()),
        };
        self.last = id;
        id
    }

    /// The id of a style of foreground `fg` and background `bg` in true
    /// colour (either missing), as [`Style::from_color`] with
    /// [`Color::from_rgb`](rich::Color::from_rgb) would make it; neither is
    /// [`StyleId::NONE`]. A pixel's cell: the style is built only the first
    /// time the pair is seen.
    ///
    /// ```
    /// use rich::{Color, Style};
    /// use rich_ext::frame::StyleTable;
    ///
    /// let mut table = StyleTable::new();
    /// let id = table.intern_rgb(Some([255, 0, 0]), None);
    /// let red = Style::from_color(Some(Color::from_rgb(255, 0, 0)), None);
    /// assert_eq!(table.intern(Some(&red)), id);
    /// assert_eq!(table.rgb(id), Some((Some([255, 0, 0]), None)));
    /// ```
    pub fn intern_rgb(&mut self, fg: Option<Rgb>, bg: Option<Rgb>) -> StyleId {
        let pair = pair_key(fg, bg);
        if pair == 0 {
            return StyleId::NONE;
        }
        if self.pairs[self.last.index()] == pair {
            return self.last;
        }
        let id = match self.rgb.get(&pair) {
            Some(&id) => id,
            None => {
                let style = Style::from_color(
                    fg.map(|[r, g, b]| rich::Color::from_rgb(r, g, b)),
                    bg.map(|[r, g, b]| rich::Color::from_rgb(r, g, b)),
                );
                let key = style_key(&style);
                let id = match self.find(key, &style) {
                    Some(id) => id,
                    None => self.push(key, style),
                };
                self.pairs[id.index()] = pair;
                self.rgb.insert(pair, id);
                id
            }
        };
        self.last = id;
        id
    }

    /// The colours of a style interned with [`intern_rgb`](Self::intern_rgb)
    /// (or found by it): what a painter can write without asking the style.
    pub fn rgb(&self, id: StyleId) -> Option<(Option<Rgb>, Option<Rgb>)> {
        match self.pairs.get(id.index()).copied().unwrap_or(0) {
            0 => None,
            pair => Some((unpack(pair >> 32), unpack(pair))),
        }
    }

    fn find(&self, key: u64, style: &Style) -> Option<StyleId> {
        let mut at = self.index.get(&key).copied().unwrap_or(0);
        while at != 0 {
            if self.styles[at as usize].as_ref() == Some(style) {
                return Some(StyleId(at));
            }
            at = self.chain[at as usize];
        }
        None
    }

    fn push(&mut self, key: u64, style: Style) -> StyleId {
        let id = self.styles.len() as u32;
        self.styles.push(Some(style));
        self.keys.push(key);
        self.pairs.push(0);
        // At the head of its key's chain.
        let next = self.index.insert(key, id).unwrap_or(0);
        self.chain.push(next);
        StyleId(id)
    }

    /// The style behind `id`; `None` for [`StyleId::NONE`].
    pub fn get(&self, id: StyleId) -> Option<&Style> {
        self.styles.get(id.index()).and_then(Option::as_ref)
    }

    /// The number of entries, counting [`StyleId::NONE`].
    pub fn len(&self) -> usize {
        self.styles.len()
    }

    /// Always false: the table always holds [`StyleId::NONE`].
    pub fn is_empty(&self) -> bool {
        false
    }

    /// Whether the table has at least doubled since the last
    /// [`retain`](Self::retain) and holds more than a few hundred styles:
    /// when dropping the unused ones is worth a pass over the cells.
    pub fn piled_up(&self) -> bool {
        self.styles.len() > PILE.max(2 * self.kept)
    }

    /// Keep only the styles `keep` marks (by index; [`StyleId::NONE`] is
    /// always kept, and ids past `keep`'s end are dropped), renumbered in
    /// their order. The result maps the old ids to the new: everything
    /// holding an old id must be moved over with it.
    ///
    /// ```
    /// use rich::Style;
    /// use rich_ext::frame::{StyleId, StyleTable};
    ///
    /// let mut table = StyleTable::new();
    /// let bold = table.intern(Some(&Style::parse("bold").unwrap()));
    /// let red = table.intern(Some(&Style::parse("red").unwrap()));
    /// let mut keep = vec![false; table.len()];
    /// keep[red.index()] = true;
    /// let remap = table.retain(&keep);
    /// assert_eq!(table.len(), 2);
    /// assert_eq!(remap.get(bold), None);
    /// let red = remap.get(red).unwrap();
    /// assert_eq!(table.get(red), Some(&Style::parse("red").unwrap()));
    /// assert_eq!(remap.get(StyleId::NONE), Some(StyleId::NONE));
    /// ```
    pub fn retain(&mut self, keep: &[bool]) -> StyleRemap {
        let mut map = vec![StyleRemap::GONE; self.styles.len()];
        map[0] = 0;
        let mut next = 1usize;
        for old in 1..self.styles.len() {
            if !keep.get(old).copied().unwrap_or(false) {
                continue;
            }
            map[old] = next as u32;
            if old != next {
                self.styles.swap(old, next);
                self.keys[next] = self.keys[old];
                self.pairs[next] = self.pairs[old];
            }
            next += 1;
        }
        self.styles.truncate(next);
        self.keys.truncate(next);
        self.pairs.truncate(next);
        self.chain.clear();
        self.chain.resize(next, 0);
        self.index.clear();
        self.rgb.clear();
        for id in 1..next as u32 {
            let key = self.keys[id as usize];
            self.chain[id as usize] = self.index.insert(key, id).unwrap_or(0);
            if self.pairs[id as usize] != 0 {
                self.rgb.insert(self.pairs[id as usize], StyleId(id));
            }
        }
        self.last = StyleId::NONE;
        self.kept = next;
        StyleRemap { map }
    }
}

/// Below this many styles a table is not worth compacting.
const PILE: usize = 1024;

/// From the ids a [`StyleTable`] had to the ids it has after
/// [`retain`](StyleTable::retain).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleRemap {
    map: Vec<u32>,
}

impl StyleRemap {
    const GONE: u32 = u32::MAX;

    /// The new id of `old`; `None` when its style was dropped (or `old` was
    /// never in the table).
    pub fn get(&self, old: StyleId) -> Option<StyleId> {
        match self.map.get(old.index()).copied() {
            None | Some(StyleRemap::GONE) => None,
            Some(new) => Some(StyleId(new)),
        }
    }

    /// The new id of `old`, or an id no style has (that equals no id the
    /// table gives) when its style was dropped: for a copy of cells that
    /// must read as changed wherever the style went away.
    pub fn get_or_gone(&self, old: StyleId) -> StyleId {
        self.get(old).unwrap_or(StyleId::GONE)
    }
}

/// The key a style is interned by: its colours (without the names they
/// were spelled with), attributes and link.
fn style_key(style: &Style) -> u64 {
    let mut hasher = FxHasher::default();
    hash_color(style.color(), &mut hasher);
    hash_color(style.bgcolor(), &mut hasher);
    let mut attrs = 0u32;
    for index in 0..13 {
        attrs |= match style.attr(index) {
            None => 0,
            Some(false) => 1,
            Some(true) => 2,
        } << (2 * index);
    }
    hasher.write_u32(attrs);
    if let Some(link) = style.link() {
        hasher.write(link.as_bytes());
    }
    hasher.finish()
}

fn hash_color(color: Option<&rich::Color>, hasher: &mut FxHasher) {
    match color {
        None => hasher.write_u32(0),
        Some(color) => {
            let triplet = color
                .triplet
                .map_or(0, |t| u32::from_be_bytes([0, t.red, t.green, t.blue]));
            hasher.write_u64(
                (color.kind as u64 + 1) << 40
                    | u64::from(color.number.map_or(0, |n| n as u32 + 1)) << 24
                    | u64::from(triplet),
            );
        }
    }
}

/// A colour pair as a key: each colour in 25 bits, a bit for "present"
/// above its 24 bits of RGB, the foreground in the high half. 0 is "no
/// colour at all".
fn pair_key(fg: Option<Rgb>, bg: Option<Rgb>) -> u64 {
    let pack = |c: Option<Rgb>| {
        c.map_or(0, |[r, g, b]| {
            1 << 24 | u64::from(u32::from_be_bytes([0, r, g, b]))
        })
    };
    pack(fg) << 32 | pack(bg)
}

fn unpack(half: u64) -> Option<Rgb> {
    let half = half as u32;
    (half & 1 << 24 != 0).then(|| {
        let [_, r, g, b] = half.to_be_bytes();
        [r, g, b]
    })
}

/// The Firefox / rustc hash: a multiply and a rotate a word, much faster
/// than the standard library's SipHash for short keys an attacker does not
/// choose (style keys, colour pairs).
#[derive(Clone, Copy, Default)]
pub(crate) struct FxHasher {
    hash: u64,
}

impl FxHasher {
    const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

    #[inline]
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(FxHasher::SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for chunk in &mut chunks {
            self.add(u64::from_le_bytes(chunk.try_into().expect("eight bytes")));
        }
        let mut rest = [0u8; 8];
        let tail = chunks.remainder();
        rest[..tail.len()].copy_from_slice(tail);
        self.add(u64::from_le_bytes(rest) ^ (tail.len() as u64) << 56);
    }

    #[inline]
    fn write_u8(&mut self, n: u8) {
        self.add(n.into());
    }

    #[inline]
    fn write_u32(&mut self, n: u32) {
        self.add(n.into());
    }

    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }

    #[inline]
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

/// Builds [`FxHasher`]s, for `HashMap`s keyed by trusted values.
pub(crate) type FxBuild = BuildHasherDefault<FxHasher>;

/// The run is followed by a line break that belonged to its segment.
const NEWLINE: u8 = 1;
/// The run continues the previous run's segment after that line break.
const JOIN: u8 = 2;

/// A piece of one segment on one row: a slice of the frame's text, its width
/// in cells and its style.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    text: u32,
    len: u32,
    cells: u32,
    style: StyleId,
    flags: u8,
}

impl Run {
    /// The width in terminal cells.
    pub fn cells(&self) -> usize {
        self.cells as usize
    }

    pub fn style(&self) -> StyleId {
        self.style
    }

    fn range(&self) -> Range<usize> {
        self.text as usize..(self.text + self.len) as usize
    }
}

/// One terminal cell of a row, from [`Frame::cells`]. The cell after a wide
/// grapheme is a continuation: empty text and width 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell<'a> {
    pub text: &'a str,
    pub width: u8,
    pub style: StyleId,
}

impl Cell<'_> {
    pub fn is_continuation(&self) -> bool {
        self.width == 0 && self.text.is_empty()
    }
}

/// Cells `columns` of `row` differ between two frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub row: usize,
    pub columns: Range<usize>,
}

/// Rows of styled runs, with interned styles. See the [module docs](self).
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Each row as a range of `runs`.
    rows: Vec<Range<u32>>,
    runs: Vec<Run>,
    text: String,
    styles: StyleTable,
    /// The stream ended with a line break, so the last row is terminated.
    trailing_newline: bool,
    /// The region tags read off the segments' styles, each once.
    tags: Vec<RegionId>,
    /// Each tag's index in `tags`, so a lookup does not scan them.
    tag_index: HashMap<RegionId, u32>,
    /// Per run, 0 for no tag or an index into `tags` plus one. Empty when no
    /// segment was tagged.
    run_tags: Vec<u32>,
    /// Regions from [`Frame::with_regions`], and each tag's place in them.
    regions: Vec<Region>,
    region_ids: HashMap<RegionId, usize>,
    /// Graphics placed on the cells: the side channel (see [`graphics`]).
    placements: Vec<Placement>,
}

impl Frame {
    /// Build a frame from a segment stream, one run per piece of a segment
    /// between line breaks. Control segments are dropped.
    pub fn from_segments(segments: &[Segment]) -> Frame {
        let mut frame = Frame {
            runs: Vec::with_capacity(segments.len()),
            text: String::with_capacity(segments.iter().map(|s| s.text.len()).sum()),
            ..Frame::default()
        };
        let mut row_start = 0u32;
        for segment in segments.iter().filter(|segment| !segment.control) {
            // A region tag (metadata a region sink asked for) is kept beside
            // the run, not in its style, so tagged and untagged text of one
            // style still share a style.
            let (style, tag) = match segment.style.as_ref() {
                Some(style) if style.meta_ref().is_some() => match regions::split_region(style) {
                    Some((id, bare)) => (frame.styles.intern(Some(&bare)), frame.tag(id)),
                    None => (frame.styles.intern(Some(style)), 0),
                },
                other => (frame.styles.intern(other), 0),
            };
            let mut flags = 0;
            let mut rest = segment.text.as_str();
            loop {
                match rest.find('\n') {
                    Some(at) => {
                        frame.push(&rest[..at], style, flags | NEWLINE, tag);
                        let end = frame.runs.len() as u32;
                        frame.rows.push(row_start..end);
                        row_start = end;
                        rest = &rest[at + 1..];
                        flags = JOIN;
                    }
                    None => {
                        if !rest.is_empty() {
                            frame.push(rest, style, flags, tag);
                        }
                        break;
                    }
                }
            }
        }
        let end = frame.runs.len() as u32;
        if end > row_start {
            frame.rows.push(row_start..end);
        } else {
            frame.trailing_newline = !frame.rows.is_empty();
        }
        frame
    }

    /// The index of `id` in `tags`, plus one.
    fn tag(&mut self, id: RegionId) -> u32 {
        let next = self.tags.len() as u32;
        let at = *self.tag_index.entry(id).or_insert(next);
        if at == next {
            self.tags.push(id);
        }
        at + 1
    }

    fn push(&mut self, text: &str, style: StyleId, flags: u8, tag: u32) {
        if tag != 0 && self.run_tags.len() < self.runs.len() {
            self.run_tags.resize(self.runs.len(), 0);
        }
        if tag != 0 || !self.run_tags.is_empty() {
            self.run_tags.push(tag);
        }
        let offset = self.text.len() as u32;
        self.text.push_str(text);
        self.runs.push(Run {
            text: offset,
            len: text.len() as u32,
            cells: cell_len(text) as u32,
            style,
            flags,
        });
    }

    /// A copy with adjacent runs of one style on a row merged into one, and
    /// empty runs dropped: what shows, not how it was split. Line breaks are
    /// no longer part of any run, so [`Frame::to_ansi`] of the result writes
    /// them unstyled.
    pub fn merged(&self) -> Frame {
        let mut frame = Frame {
            runs: Vec::with_capacity(self.runs.len()),
            text: String::with_capacity(self.text.len()),
            styles: self.styles.clone(),
            trailing_newline: self.trailing_newline,
            tags: self.tags.clone(),
            tag_index: self.tag_index.clone(),
            regions: self.regions.clone(),
            region_ids: self.region_ids.clone(),
            placements: self.placements.clone(),
            ..Frame::default()
        };
        for row in 0..self.height() {
            let start = frame.runs.len() as u32;
            for index in self.row_range(row) {
                let run = &self.runs[index];
                if run.len == 0 {
                    continue;
                }
                let tag = self.run_tags.get(index).copied().unwrap_or(0);
                let text = &self.text[run.range()];
                let last_tag = frame.run_tags.last().copied().unwrap_or(0);
                match frame.runs[start as usize..].last_mut() {
                    Some(last) if last.style == run.style && last_tag == tag => {
                        last.len += run.len;
                        last.cells += run.cells;
                        frame.text.push_str(text);
                    }
                    _ => frame.push(text, run.style, 0, tag),
                }
            }
            frame.rows.push(start..frame.runs.len() as u32);
        }
        frame
    }

    /// The number of rows. A final line break does not start a new row.
    pub fn height(&self) -> usize {
        self.rows.len()
    }

    /// The runs of row `index`. Panics when `index >= height()`.
    pub fn row(&self, index: usize) -> &[Run] {
        &self.runs[self.row_range(index)]
    }

    /// The indices of row `index`'s runs.
    fn row_range(&self, index: usize) -> Range<usize> {
        let range = &self.rows[index];
        range.start as usize..range.end as usize
    }

    /// The region tag of run `index`, if it has one.
    fn run_tag(&self, index: usize) -> Option<RegionId> {
        match self.run_tags.get(index).copied().unwrap_or(0) {
            0 => None,
            tag => self.tags.get(tag as usize - 1).copied(),
        }
    }

    /// The width of row `index` in cells.
    pub fn row_width(&self, index: usize) -> usize {
        self.row(index).iter().map(Run::cells).sum()
    }

    /// The widest row, in cells.
    pub fn width(&self) -> usize {
        (0..self.height())
            .map(|row| self.row_width(row))
            .max()
            .unwrap_or(0)
    }

    /// The text of `run`.
    pub fn run_text(&self, run: &Run) -> &str {
        &self.text[run.range()]
    }

    pub fn styles(&self) -> &StyleTable {
        &self.styles
    }

    /// Whether the stream ended with a line break.
    pub fn ends_with_newline(&self) -> bool {
        self.trailing_newline
    }

    /// The total number of runs.
    pub fn run_count(&self) -> usize {
        self.runs.len()
    }

    /// Plain text: rows joined by line breaks, without styles.
    pub fn plain(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + self.height());
        for row in 0..self.height() {
            if row > 0 {
                out.push('\n');
            }
            for run in self.row(row) {
                out.push_str(self.run_text(run));
            }
        }
        if self.trailing_newline {
            out.push('\n');
        }
        out
    }

    /// Encode as `console` would: the same bytes as
    /// `console.segments_to_string` on the stream this frame was built from,
    /// less its control segments.
    pub fn to_ansi(&self, console: &Console) -> String {
        self.encode(console.color_system(), console.no_color())
    }

    /// Encode with an explicit colour system. `no_color` removes colours but
    /// keeps other attributes, as `Console` does when `no_color` is set.
    pub fn encode(&self, system: Option<ColorSystem>, no_color: bool) -> String {
        let styles = self.styles_for(system, no_color);
        let mut out = String::with_capacity(self.text.len() * 2);
        // A segment's pieces render as one: its line breaks sit inside the
        // same SGR pair, as upstream's `_render_buffer` writes them.
        let mut piece = String::new();
        let mut style = StyleId::NONE;
        for row in 0..self.height() {
            let runs = self.row(row);
            for run in runs {
                if run.flags & JOIN == 0 {
                    flush(&mut out, &piece, styles[style.index()].as_ref(), system);
                    piece.clear();
                    style = run.style;
                }
                piece.push_str(&self.text[run.range()]);
                if run.flags & NEWLINE != 0 {
                    piece.push('\n');
                }
            }
            // A merged frame's line breaks belong to no run: write them bare.
            let breaks = row + 1 < self.height() || self.trailing_newline;
            if breaks && runs.last().is_none_or(|run| run.flags & NEWLINE == 0) {
                flush(&mut out, &piece, styles[style.index()].as_ref(), system);
                piece.clear();
                out.push('\n');
            }
        }
        flush(&mut out, &piece, styles[style.index()].as_ref(), system);
        out
    }

    /// Encode after merging adjacent runs of one style, with line breaks
    /// unstyled. Fewer bytes than [`Frame::to_ansi`], but not upstream's.
    pub fn to_ansi_merged(&self, console: &Console) -> String {
        self.merged()
            .encode(console.color_system(), console.no_color())
    }

    /// Encode the runs covering `columns` of row `row` (clipped to the row),
    /// merging adjacent runs of one style. A run that starts before the range
    /// is cut at the grapheme boundary; a wide grapheme cut in half becomes a
    /// space. This is what a cell-level repaint writes for one [`Change`].
    pub fn encode_span(
        &self,
        row: usize,
        columns: Range<usize>,
        system: Option<ColorSystem>,
        no_color: bool,
    ) -> String {
        let styles = self.styles_for(system, no_color);
        let mut out = String::new();
        let mut piece = String::new();
        let mut style = None;
        for cell in self
            .cells(row)
            .into_iter()
            .skip(columns.start)
            .take(columns.len())
        {
            if style != Some(cell.style) {
                if let Some(id) = style {
                    flush(
                        &mut out,
                        &piece,
                        styles[StyleId::index(id)].as_ref(),
                        system,
                    );
                }
                piece.clear();
                style = Some(cell.style);
            }
            if cell.is_continuation() {
                if piece.is_empty() {
                    piece.push(' ');
                }
                continue;
            }
            piece.push_str(cell.text);
        }
        if let Some(id) = style {
            flush(&mut out, &piece, styles[id.index()].as_ref(), system);
        }
        out
    }

    fn styles_for(&self, system: Option<ColorSystem>, no_color: bool) -> Vec<Option<Style>> {
        if no_color && system.is_some() {
            self.styles
                .styles
                .iter()
                .map(|style| style.as_ref().map(Style::without_color))
                .collect()
        } else {
            self.styles.styles.clone()
        }
    }

    /// One cell per grapheme of row `index`, using core's widths. A wide
    /// grapheme is followed by continuation cells, so a cell's index in the
    /// result is its column. A zero-width grapheme occupies no column: it
    /// joins the cell before it, even across runs, or the first cell of the
    /// row when nothing precedes it.
    pub fn cells(&self, index: usize) -> Vec<Cell<'_>> {
        let row = self.row(index);
        // Arena ranges first: the runs of a row are stored in order in the
        // text arena, so a zero-width grapheme extends its neighbour's range.
        let mut cells: Vec<(Range<usize>, u8, StyleId)> =
            Vec::with_capacity(row.iter().map(Run::cells).sum());
        let mut leading: Option<usize> = None;
        for run in row {
            let base = run.text as usize;
            let (graphemes, _) = split_graphemes(self.run_text(run));
            for (start, end, width) in graphemes {
                let range = base + start..base + end;
                if width == 0 {
                    match cells.iter_mut().rev().find(|cell| cell.1 > 0) {
                        Some(lead) if lead.0.end == range.start => lead.0.end = range.end,
                        Some(_) => {}
                        None => {
                            leading.get_or_insert(range.start);
                        }
                    }
                    continue;
                }
                let start = leading.take().unwrap_or(range.start);
                cells.push((start..range.end, width as u8, run.style));
                for _ in 1..width {
                    cells.push((range.end..range.end, 0, run.style));
                }
            }
        }
        cells
            .into_iter()
            .map(|(range, width, style)| Cell {
                text: &self.text[range],
                width,
                style,
            })
            .collect()
    }

    /// The cells that differ from `previous`, as column ranges per row, in
    /// row order. Styles compare by value, so the two frames need not share a
    /// style table. A row present in only one frame changes across its width.
    pub fn diff(&self, previous: &Frame) -> Vec<Change> {
        let mut changes = Vec::new();
        for row in 0..self.height().max(previous.height()) {
            let new = if row < self.height() {
                self.cells(row)
            } else {
                Vec::new()
            };
            let old = if row < previous.height() {
                previous.cells(row)
            } else {
                Vec::new()
            };
            let width = new.len().max(old.len());
            let mut open = None;
            for column in 0..width {
                let same = match (new.get(column), old.get(column)) {
                    (Some(a), Some(b)) => {
                        a.text == b.text
                            && a.width == b.width
                            && self.styles.get(a.style) == previous.styles.get(b.style)
                    }
                    _ => false,
                };
                match (same, open) {
                    (false, None) => open = Some(column),
                    (true, Some(start)) => {
                        changes.push(Change {
                            row,
                            columns: start..column,
                        });
                        open = None;
                    }
                    _ => {}
                }
            }
            if let Some(start) = open {
                changes.push(Change {
                    row,
                    columns: start..width,
                });
            }
        }
        changes
    }
}

fn flush(out: &mut String, text: &str, style: Option<&Style>, system: Option<ColorSystem>) {
    match (style, system) {
        (Some(style), Some(system)) => out.push_str(&style.render(text, Some(system))),
        _ => out.push_str(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, style: Option<&str>) -> Segment {
        Segment::new(text, style.map(|s| Style::parse(s).unwrap()))
    }

    fn console(system: Option<ColorSystem>, no_color: bool) -> Console {
        Console::builder()
            .width(40)
            .force_terminal(true)
            .color_system(system)
            .no_color(no_color)
            .build()
    }

    #[test]
    fn exact_encoding_keeps_line_breaks_inside_a_segment() {
        let segments = vec![
            seg("a\nb", Some("bold")),
            seg("c", None),
            seg("\n", None),
            seg("d\n", Some("red link https://x.test")),
        ];
        let console = console(Some(ColorSystem::Truecolor), false);
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.height(), 3);
        assert!(frame.ends_with_newline());
        assert_eq!(frame.plain(), "a\nbc\nd\n");
        assert_eq!(
            frame.to_ansi(&console),
            console.segments_to_string(&segments)
        );
    }

    #[test]
    fn control_segments_are_dropped() {
        let segments = vec![seg("x", None), Segment::control("\x1b[2K"), seg("y", None)];
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.plain(), "xy");
        assert_eq!(frame.run_count(), 2);
    }

    #[test]
    fn styles_are_interned_once() {
        let segments = vec![
            seg("a", Some("bold")),
            seg("b", Some("italic")),
            seg("c", Some("bold")),
            seg("d", None),
        ];
        let frame = Frame::from_segments(&segments);
        assert_eq!(frame.styles().len(), 3);
        let row = frame.row(0);
        assert_eq!(row[0].style(), row[2].style());
        assert_eq!(row[3].style(), StyleId::NONE);
    }

    #[test]
    fn no_color_keeps_attributes() {
        let segments = vec![seg("hot", Some("bold red on blue"))];
        let console = console(Some(ColorSystem::Standard), true);
        let frame = Frame::from_segments(&segments);
        assert_eq!(
            frame.to_ansi(&console),
            console.segments_to_string(&segments)
        );
        assert_eq!(frame.to_ansi(&console), "\x1b[1mhot\x1b[0m");
    }

    #[test]
    fn merging_joins_runs_of_one_style() {
        let segments = vec![
            seg("ab", Some("bold")),
            seg("cd", Some("bold")),
            seg("", Some("red")),
            seg("\n", None),
            seg("e", None),
        ];
        let console = console(Some(ColorSystem::Truecolor), false);
        let frame = Frame::from_segments(&segments);
        let merged = frame.merged();
        assert_eq!(merged.row(0).len(), 1);
        assert_eq!(frame.to_ansi_merged(&console), "\x1b[1mabcd\x1b[0m\ne");
        assert_eq!(merged.plain(), frame.plain());
    }

    #[test]
    fn cells_follow_core_widths() {
        let frame = Frame::from_segments(&[seg("a漢b", None)]);
        let cells = frame.cells(0);
        let widths: Vec<u8> = cells.iter().map(|cell| cell.width).collect();
        assert_eq!(widths, [1, 2, 0, 1]);
        assert!(cells[2].is_continuation());
        assert_eq!(frame.width(), 4);
    }

    #[test]
    fn zero_width_graphemes_take_no_column() {
        // A combining mark styled on its own joins the cell before it.
        let frame =
            Frame::from_segments(&[seg("a", None), seg("\u{301}", Some("bold")), seg("x", None)]);
        let cells = frame.cells(0);
        assert_eq!(cells.len(), 2);
        assert_eq!(cells[0].text, "a\u{301}");
        assert_eq!(cells[1].text, "x");
        let changed =
            Frame::from_segments(&[seg("a", None), seg("\u{301}", Some("bold")), seg("y", None)]);
        assert_eq!(
            changed.diff(&frame),
            [Change {
                row: 0,
                columns: 1..2
            }]
        );
        // After a wide grapheme it joins the lead, not the continuation.
        let wide = Frame::from_segments(&[seg("漢", None), seg("\u{301}", None), seg("z", None)]);
        let cells = wide.cells(0);
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[0].text, "漢\u{301}");
        assert!(cells[1].is_continuation());
        // At the start of a row it joins the first cell.
        let leading = Frame::from_segments(&[seg("\u{301}", None), seg("b", None)]);
        let cells = leading.cells(0);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].text, "\u{301}b");
    }

    #[test]
    fn diff_reports_changed_cells() {
        let before = Frame::from_segments(&[seg("hello\n", None), seg("world", None)]);
        let after = Frame::from_segments(&[
            seg("hel", None),
            seg("l", Some("bold")),
            seg("o\n", None),
            seg("world!", None),
            seg("\nnew", None),
        ]);
        assert_eq!(
            after.diff(&before),
            [
                Change {
                    row: 0,
                    columns: 3..4
                },
                Change {
                    row: 1,
                    columns: 5..6
                },
                Change {
                    row: 2,
                    columns: 0..3
                },
            ]
        );
        assert!(before.diff(&before).is_empty());
        // Resegmented but identical output has no changes.
        let split = Frame::from_segments(&[seg("hel", None), seg("lo\nworld", None)]);
        assert!(split.diff(&before).is_empty());
    }

    #[test]
    fn encode_span_repaints_one_change() {
        let frame = Frame::from_segments(&[seg("ab", None), seg("cd", Some("bold"))]);
        let span = frame.encode_span(0, 1..3, Some(ColorSystem::Standard), false);
        assert_eq!(span, "b\x1b[1mc\x1b[0m");
        let wide = Frame::from_segments(&[seg("漢x", None)]);
        assert_eq!(wide.encode_span(0, 1..3, None, false), " x");
    }

    #[test]
    fn styles_that_hash_alike_stay_apart() {
        // Metadata is not in the key: these two share one, and a chain.
        let plain = Style::parse("bold").unwrap();
        let meta: rich::style::Meta = [("k", rich::style::MetaValue::Int(1))]
            .into_iter()
            .collect();
        let tagged = plain.clone().with_meta(meta);
        let mut table = StyleTable::new();
        let a = table.intern(Some(&plain));
        let b = table.intern(Some(&tagged));
        assert_ne!(a, b);
        assert_eq!(table.intern(Some(&plain)), a);
        assert_eq!(table.intern(Some(&tagged)), b);
        let remap = table.retain(&[true, false, true]);
        let b = remap.get(b).unwrap();
        assert_eq!(table.get(b), Some(&tagged));
        assert_eq!(table.intern(Some(&tagged)), b);
        assert_eq!(table.intern(Some(&plain)), StyleId::from_index(2));
    }

    #[test]
    fn colour_pairs_and_styles_share_ids() {
        let mut table = StyleTable::new();
        let style = Style::from_color(None, Some(rich::Color::from_rgb(1, 2, 3)));
        let id = table.intern(Some(&style));
        assert_eq!(table.rgb(id), None, "not interned as a pair yet");
        assert_eq!(table.intern_rgb(None, Some([1, 2, 3])), id);
        assert_eq!(table.rgb(id), Some((None, Some([1, 2, 3]))));
        assert_eq!(table.intern_rgb(None, None), StyleId::NONE);
        let other = table.intern_rgb(Some([0, 0, 0]), Some([1, 2, 3]));
        assert_ne!(other, id);
        // Kept through a compaction, found again by its colours.
        let remap = table.retain(&[true, false, true]);
        assert_eq!(remap.get(id), None);
        let other = remap.get(other).unwrap();
        assert_eq!(table.intern_rgb(Some([0, 0, 0]), Some([1, 2, 3])), other);
        assert!(!table.piled_up());
    }
}

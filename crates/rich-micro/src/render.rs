//! Placeholder cells and the renderer seam (#577, #578).
//!
//! # The placeholder representation
//!
//! An asset in a [`Text`] is **exactly `cols` cells of ordinary text**, under
//! a span whose style carries [`MICRO_META_KEY`] in its metadata (the way
//! core's regions tag segments with `rich.region`). Its value is a list:
//! `[name, cols, rows, id]`, where `id` is unique per occurrence, so two
//! adjacent copies of one asset stay distinguishable (see [`MicroMeta`]).
//!
//! That is the asset's first row. An asset two rows high (#711) also has a
//! second row, which [`MicroRows`](crate::MicroRows) reserves below the line
//! after layout: [`PAD_CELL`] blanks at the same columns, tagged
//! `[name, cols, rows, id, 1]`, the fifth item being the row.
//!
//! The cells hold the asset's **fallback**: its emoji, or its text, padded to
//! `cols` with [`PAD_CELL`] (U+2800, a blank that is not whitespace, so
//! wrapping never splits an asset); failing both, its alt text cut to fit.
//! Printed as is — to a pipe, a log, an export — that fallback is what
//! appears. Metadata never renders, so the bytes are plain text.
//!
//! Because the cells are ordinary text, every layout (measure, wrap, crop,
//! tables, panels) sizes an asset at exactly `cols`, whatever draws it.
//!
//! # Drawing images
//!
//! A [`MicroRenderer`] turns one complete placement into segments that take
//! the same `cols` cells, for example a zero-width control escape followed
//! by placeholder cells. [`MicroView`] wraps any renderable, finds each
//! placement in its rendered segments, and offers it to its renderers in
//! order; the first answer of the right width wins, and without one the
//! fallback cells stay. A placement that was cut (cropped, or split across
//! lines) is never offered: its fallback stays. A two-row asset is offered
//! only when both its rows are there, the second directly below the first
//! at the same column; its rows are offered top first, and the first
//! renderer that draws both at the right width draws it. Otherwise its
//! fallback stays, so an image never spills onto a line that did not make
//! room for it. [`FallbackRenderer`] redraws the fallback; the terminal
//! protocols (Kitty, iTerm2, Sixel) and block cells plug in here through
//! [`crate::graphics::MicroGraphics::renderer`].

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::style::{Meta, MetaValue};
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use crate::model::MicroAsset;
use crate::registry::MicroRegistry;
use crate::rows::runs;

/// The style [`Meta`] key that marks a placeholder's cells.
pub const MICRO_META_KEY: &str = "rich.micro";

/// Pads a fallback to its asset's width: U+2800 BRAILLE PATTERN BLANK, one
/// cell, blank in every font, and not whitespace, so the asset stays one
/// word.
pub const PAD_CELL: char = '\u{2800}';

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// The markup pre-pass stands in for tokens with characters from Supplementary
/// Private Use Area-B while core parses the rest.
pub(crate) const SENTINEL_BASE: u32 = 0x10_0000;
pub(crate) const SENTINEL_LAST: u32 = 0x10_FFFD;

pub(crate) fn is_sentinel(c: char) -> bool {
    (SENTINEL_BASE..=SENTINEL_LAST).contains(&(c as u32))
}

/// What a placeholder's metadata says: which asset, its size, which
/// occurrence, and which of its rows these cells are.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MicroMeta {
    pub name: String,
    pub cols: usize,
    pub rows: usize,
    /// Unique per occurrence (per process).
    pub id: u64,
    /// Which row of the asset these cells are: 0, the row in the text, or
    /// 1, the row a two-row asset reserves below it.
    pub row: usize,
}

impl MicroMeta {
    /// A new occurrence of `asset`: its first row.
    pub fn new(asset: &MicroAsset) -> Self {
        MicroMeta {
            name: asset.name().to_string(),
            cols: asset.cols(),
            rows: asset.size().rows(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            row: 0,
        }
    }

    /// The same occurrence's row `row`.
    #[must_use]
    pub fn with_row(&self, row: usize) -> Self {
        MicroMeta {
            row,
            ..self.clone()
        }
    }

    /// As a style metadata value: `[name, cols, rows, id]` for the first
    /// row, `[name, cols, rows, id, row]` for a row below it.
    pub fn to_value(&self) -> MetaValue {
        let mut items = vec![
            MetaValue::Str(self.name.clone()),
            MetaValue::Int(self.cols as i64),
            MetaValue::Int(self.rows as i64),
            MetaValue::Int(self.id as i64),
        ];
        if self.row > 0 {
            items.push(MetaValue::Int(self.row as i64));
        }
        MetaValue::List(items)
    }

    /// A style holding only this tag.
    pub fn to_style(&self) -> Style {
        let mut meta = Meta::new();
        meta.insert(MICRO_META_KEY, self.to_value());
        Style::from_meta(meta)
    }

    /// The tag on `style`, if any.
    pub fn from_style(style: &Style) -> Option<Self> {
        let MetaValue::List(items) = style.meta_ref()?.get(MICRO_META_KEY)? else {
            return None;
        };
        let (head, row) = match items.as_slice() {
            [head @ .., MetaValue::Int(row)] if items.len() == 5 => (head, *row),
            head if items.len() == 4 => (head, 0),
            _ => return None,
        };
        match head {
            [MetaValue::Str(name), MetaValue::Int(cols), MetaValue::Int(rows), MetaValue::Int(id)]
                if *cols > 0 && *rows > 0 && (0..*rows).contains(&row) =>
            {
                Some(MicroMeta {
                    name: name.clone(),
                    cols: *cols as usize,
                    rows: *rows as usize,
                    id: *id as u64,
                    row: row as usize,
                })
            }
            _ => None,
        }
    }

    /// The tag on `segment`, if any. Control segments are never tagged.
    pub fn from_segment(segment: &Segment) -> Option<Self> {
        if segment.control {
            return None;
        }
        segment.style.as_ref().and_then(Self::from_style)
    }
}

/// Which fallback to prefer where no image is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FallbackPreference {
    /// The emoji, else the text, else the alt text.
    #[default]
    Emoji,
    /// The text, else the alt text: for consoles that cannot show emoji.
    Text,
    /// The alt text, cut to fit.
    Alt,
}

/// `value` padded with [`PAD_CELL`] to exactly `cols` cells.
fn pad(value: &str, cols: usize) -> String {
    let width = cell_len(value);
    let mut out = value.to_string();
    out.extend(std::iter::repeat_n(PAD_CELL, cols.saturating_sub(width)));
    out
}

/// The alt text cut to `cols` cells, whitespace and controls replaced with
/// [`PAD_CELL`], padded to exactly `cols`.
fn alt_cells(alt: &str, cols: usize) -> String {
    let mut out = String::new();
    let mut width = 0;
    for c in alt.chars() {
        let c = if c.is_whitespace() || c.is_control() || is_sentinel(c) {
            PAD_CELL
        } else {
            c
        };
        let w = cell_len(c.encode_utf8(&mut [0; 4]));
        if width + w > cols {
            break;
        }
        out.push(c);
        width += w;
    }
    pad(&out, cols)
}

/// The fallback cells for `asset`'s first row: exactly `asset.cols()` cells.
pub fn fallback_cells(asset: &MicroAsset, preference: FallbackPreference) -> String {
    let cols = asset.cols();
    let fallback = asset.fallback();
    let choice = match preference {
        FallbackPreference::Emoji => fallback.emoji.as_ref().or(fallback.text.as_ref()),
        FallbackPreference::Text => fallback.text.as_ref(),
        FallbackPreference::Alt => None,
    };
    match choice {
        // Checked on the way in, but a manifest may be edited under us.
        Some(value) if cell_len(value) <= cols && cell_len(value) > 0 => pad(value, cols),
        _ => alt_cells(asset.alt(), cols),
    }
}

/// The fallback cells of `asset`'s row `row`: its [first
/// row's](fallback_cells), and [`PAD_CELL`] blanks below it. Exactly
/// `asset.cols()` cells either way.
pub fn fallback_row(asset: &MicroAsset, preference: FallbackPreference, row: usize) -> String {
    if row == 0 {
        fallback_cells(asset, preference)
    } else {
        PAD_CELL.to_string().repeat(asset.cols())
    }
}

/// One occurrence of `asset` as a [`Text`]: its fallback cells, tagged. A
/// two-row asset's second row is not in the text:
/// [`MicroRows`](crate::MicroRows) reserves it below the line after layout.
pub fn placeholder(asset: &MicroAsset, preference: FallbackPreference) -> Text {
    placeholder_with(asset, preference, MicroMeta::new(asset))
}

pub(crate) fn placeholder_with(
    asset: &MicroAsset,
    preference: FallbackPreference,
    meta: MicroMeta,
) -> Text {
    let cells = fallback_cells(asset, preference);
    let mut text = Text::new("");
    text.append(&cells, Some(meta.to_style().into()));
    text
}

/// One complete placement found in rendered output: one row of an asset
/// ([`MicroMeta::row`]).
#[derive(Clone, Debug)]
pub struct Placement<'a> {
    pub asset: &'a MicroAsset,
    pub meta: MicroMeta,
    /// The style of the placeholder's first cell (colours around it, and the
    /// tag itself).
    pub style: Option<Style>,
    /// The fallback text that is there now.
    pub fallback: String,
}

/// Draws a placement. Implementations must return segments whose visible
/// width (control segments count zero) is exactly `placement.meta.cols`, or
/// `None` to pass. Anything else is ignored and the fallback stays.
///
/// A two-row asset is offered one row at a time, top first
/// (`placement.meta.row` is 0, then 1), and drawn only when the same
/// renderer draws both: an image drawn at the cursor goes with the last row,
/// moving the cursor up to the first. The rows are printed on consecutive
/// lines at the same column.
///
/// Return only what the target can show: a renderer is picked per console,
/// and the console's capabilities are the renderer's to check. Keep the tag
/// on visible cells ([`Placement::style`]) so later passes can still find
/// the asset.
pub trait MicroRenderer: Send + Sync {
    /// A short name, for diagnostics (`fallback`, `kitty`, ...).
    fn name(&self) -> &str;

    /// Draw `placement`, or pass.
    fn render(&self, placement: &Placement<'_>, console: &Console) -> Option<Vec<Segment>>;
}

impl<T: MicroRenderer + ?Sized> MicroRenderer for Arc<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn render(&self, placement: &Placement<'_>, console: &Console) -> Option<Vec<Segment>> {
        (**self).render(placement, console)
    }
}

/// Redraws each placement with a chosen fallback: for example
/// [`FallbackPreference::Text`] on a console that cannot show emoji. Never
/// emits control sequences.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FallbackRenderer {
    pub preference: FallbackPreference,
}

impl FallbackRenderer {
    pub fn new(preference: FallbackPreference) -> Self {
        FallbackRenderer { preference }
    }
}

impl MicroRenderer for FallbackRenderer {
    fn name(&self) -> &str {
        "fallback"
    }

    fn render(&self, placement: &Placement<'_>, _console: &Console) -> Option<Vec<Segment>> {
        Some(vec![Segment::new(
            fallback_row(placement.asset, self.preference, placement.meta.row),
            placement.style.clone(),
        )])
    }
}

/// Wraps a renderable and offers every complete placement in its output to
/// `renderers`, in order. Layout is the inner renderable's: this only
/// replaces cells with cells of the same width.
#[must_use = "builder methods return a new value; it does nothing until it is rendered or used"]
pub struct MicroView<R> {
    inner: R,
    registry: Arc<MicroRegistry>,
    renderers: Vec<Arc<dyn MicroRenderer>>,
}

impl<R: Renderable> MicroView<R> {
    pub fn new(inner: R, registry: Arc<MicroRegistry>) -> Self {
        MicroView {
            inner,
            registry,
            renderers: Vec::new(),
        }
    }

    /// Add a renderer after the ones added so far.
    pub fn renderer(mut self, renderer: Arc<dyn MicroRenderer>) -> Self {
        self.renderers.push(renderer);
        self
    }
}

impl<R: Renderable> Renderable for MicroView<R> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let segments = self.inner.rich_render(console, options);
        if self.renderers.is_empty() {
            return segments;
        }
        rewrite(segments, &self.registry, &self.renderers, console)
    }

    fn measure(&self, console: &Console, options: &ConsoleOptions) -> Measurement {
        self.inner.measure(console, options)
    }
}

/// Find each run of segments tagged with one occurrence and, when the run is
/// the asset's whole width (and, for a two-row asset, its second row is
/// right below it), offer it to `renderers`.
pub fn rewrite(
    segments: Vec<Segment>,
    registry: &MicroRegistry,
    renderers: &[Arc<dyn MicroRenderer>],
    console: &Console,
) -> Vec<Segment> {
    rewrite_rows(segments, registry, renderers, console, false)
}

/// The width of `drawn`, control segments counting zero.
fn width_of(drawn: &[Segment]) -> usize {
    drawn.iter().map(Segment::cell_length).sum()
}

/// [`rewrite`]; with `each_row`, every row of a two-row asset is offered on
/// its own, for one line of a frame, where the rows around it are not known
/// (renderers then only swap cells, never draw at the cursor).
pub(crate) fn rewrite_rows(
    segments: Vec<Segment>,
    registry: &MicroRegistry,
    renderers: &[Arc<dyn MicroRenderer>],
    console: &Console,
    each_row: bool,
) -> Vec<Segment> {
    let found = runs(&segments);
    if found.is_empty() {
        return segments;
    }
    // Runs by occurrence and row, for finding a first row's second.
    let mut by_row: HashMap<(u64, usize), Vec<usize>> = HashMap::new();
    for (index, run) in found.iter().enumerate() {
        by_row
            .entry((run.meta.id, run.meta.row))
            .or_default()
            .push(index);
    }
    let placement = |index: usize, asset| {
        let run = &found[index];
        Placement {
            asset,
            meta: run.meta.clone(),
            style: segments[run.start].style.clone(),
            fallback: segments[run.start..run.end]
                .iter()
                .map(|s| s.text.as_str())
                .collect(),
        }
    };
    let mut drawn: Vec<Option<Vec<Segment>>> = vec![None; found.len()];
    for (index, run) in found.iter().enumerate() {
        let meta = &run.meta;
        let asset = registry
            .resolve(&meta.name)
            .filter(|asset| asset.cols() == meta.cols && asset.rows() == meta.rows);
        let Some(asset) = asset.filter(|_| run.width == meta.cols) else {
            continue;
        };
        if meta.rows == 1 || each_row {
            let placement = placement(index, asset);
            drawn[index] = renderers.iter().find_map(|renderer| {
                renderer
                    .render(&placement, console)
                    .filter(|cells| width_of(cells) == meta.cols)
            });
            continue;
        }
        if meta.row != 0 {
            // A lower row: drawn with the first, or not at all.
            continue;
        }
        // Every row, each right below the one before, at the same column.
        let mut group = vec![index];
        for row in 1..meta.rows {
            let below = by_row.get(&(meta.id, row)).and_then(|candidates| {
                candidates.iter().copied().find(|&other| {
                    let other = &found[other];
                    other.line == run.line + row
                        && other.column == run.column
                        && other.width == meta.cols
                })
            });
            match below {
                Some(other) => group.push(other),
                None => break,
            }
        }
        if group.len() != meta.rows {
            continue;
        }
        let placements: Vec<Placement<'_>> = group.iter().map(|&i| placement(i, asset)).collect();
        let rows = renderers.iter().find_map(|renderer| {
            placements
                .iter()
                .map(|placement| {
                    renderer
                        .render(placement, console)
                        .filter(|cells| width_of(cells) == meta.cols)
                })
                .collect::<Option<Vec<_>>>()
        });
        if let Some(rows) = rows {
            for (&other, cells) in group.iter().zip(rows) {
                drawn[other] = Some(cells);
            }
        }
    }
    let mut out = Vec::with_capacity(segments.len());
    let mut rest = segments.into_iter();
    let mut at = 0;
    for (run, cells) in found.iter().zip(drawn) {
        out.extend(rest.by_ref().take(run.start - at));
        let original: Vec<Segment> = rest.by_ref().take(run.end - run.start).collect();
        match cells {
            Some(cells) => out.extend(cells),
            None => out.extend(original),
        }
        at = run.end;
    }
    out.extend(rest);
    out
}

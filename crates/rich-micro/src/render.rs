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
//! lines) is never offered: its fallback stays. Terminal protocols (Kitty,
//! iTerm2, Sixel) plug in here in a later release; this release ships
//! [`FallbackRenderer`] only.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use rich::cells::cell_len;
use rich::measure::Measurement;
use rich::style::{Meta, MetaValue};
use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};

use crate::model::MicroAsset;
use crate::registry::MicroRegistry;

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

/// What a placeholder's metadata says: which asset, its size, and which
/// occurrence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MicroMeta {
    pub name: String,
    pub cols: usize,
    pub rows: usize,
    /// Unique per occurrence (per process).
    pub id: u64,
}

impl MicroMeta {
    /// A new occurrence of `asset`.
    pub fn new(asset: &MicroAsset) -> Self {
        MicroMeta {
            name: asset.name().to_string(),
            cols: asset.cols(),
            rows: asset.size().rows(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// As a style metadata value: `[name, cols, rows, id]`.
    pub fn to_value(&self) -> MetaValue {
        MetaValue::List(vec![
            MetaValue::Str(self.name.clone()),
            MetaValue::Int(self.cols as i64),
            MetaValue::Int(self.rows as i64),
            MetaValue::Int(self.id as i64),
        ])
    }

    /// A style holding only this tag.
    pub fn to_style(&self) -> Style {
        let mut meta = Meta::new();
        meta.insert(MICRO_META_KEY, self.to_value());
        Style::from_meta(meta)
    }

    /// The tag on `style`, if any.
    pub fn from_style(style: &Style) -> Option<Self> {
        match style.meta_ref()?.get(MICRO_META_KEY)? {
            MetaValue::List(items) => match items.as_slice() {
                [MetaValue::Str(name), MetaValue::Int(cols), MetaValue::Int(rows), MetaValue::Int(id)]
                    if *cols > 0 && *rows > 0 =>
                {
                    Some(MicroMeta {
                        name: name.clone(),
                        cols: *cols as usize,
                        rows: *rows as usize,
                        id: *id as u64,
                    })
                }
                _ => None,
            },
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

/// The fallback cells for `asset`: exactly `asset.cols()` cells.
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

/// One occurrence of `asset` as a [`Text`]: its fallback cells, tagged.
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

/// One complete placement found in rendered output.
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
            fallback_cells(placement.asset, self.preference),
            placement.style.clone(),
        )])
    }
}

/// Wraps a renderable and offers every complete placement in its output to
/// `renderers`, in order. Layout is the inner renderable's: this only
/// replaces cells with cells of the same width.
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
/// the asset's whole width, offer it to `renderers`.
pub fn rewrite(
    segments: Vec<Segment>,
    registry: &MicroRegistry,
    renderers: &[Arc<dyn MicroRenderer>],
    console: &Console,
) -> Vec<Segment> {
    let mut out = Vec::with_capacity(segments.len());
    let mut index = 0;
    while index < segments.len() {
        let Some(meta) = MicroMeta::from_segment(&segments[index]) else {
            out.push(segments[index].clone());
            index += 1;
            continue;
        };
        let mut end = index;
        let mut width = 0;
        while end < segments.len()
            && MicroMeta::from_segment(&segments[end]).is_some_and(|m| m.id == meta.id)
        {
            width += segments[end].cell_length();
            end += 1;
        }
        let run = &segments[index..end];
        let asset = registry
            .resolve(&meta.name)
            .filter(|asset| asset.cols() == meta.cols);
        let drawn = match asset {
            Some(asset) if width == meta.cols => {
                let placement = Placement {
                    asset,
                    meta: meta.clone(),
                    style: run[0].style.clone(),
                    fallback: run.iter().map(|s| s.text.as_str()).collect(),
                };
                renderers.iter().find_map(|renderer| {
                    renderer
                        .render(&placement, console)
                        .filter(|drawn| drawn.iter().map(Segment::cell_length).sum::<usize>() == meta.cols)
                })
            }
            _ => None,
        };
        match drawn {
            Some(drawn) => out.extend(drawn),
            None => out.extend_from_slice(run),
        }
        index = end;
    }
    out
}

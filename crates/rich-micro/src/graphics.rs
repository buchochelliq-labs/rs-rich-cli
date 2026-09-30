//! Drawing micro assets on a terminal (#572–#577): choosing a protocol, and
//! the renderers and graphics source for each.
//!
//! # Choosing (#573)
//!
//! [`select`] picks one [`MicroMode`] per terminal, in this order: Kitty,
//! iTerm2, Sixel, half-blocks, then the emoji or text fallback. It reads
//! `rich-ext`'s [`GraphicsEnvironment`] (the protocol found, and the cell
//! size in pixels) and honours `RICH_MICRO=kitty|iterm|sixel|blocks|text`.
//! Nothing is drawn that the terminal was not found (or said by the user)
//! to support, and output that is not a terminal (a pipe, a log, an export)
//! always gets the text fallback, whatever the override says.
//!
//! # Drawing
//!
//! [`MicroGraphics`] holds one terminal's drawing state: the selection, the
//! image cache, the Kitty ids it transmitted, and the animation clock.
//!
//! - For printing, [`MicroGraphics::renderer`] is a [`MicroRenderer`] for
//!   [`MicroView`]: Kitty placeholders (transmitting each image once per
//!   session), an iTerm2 or Sixel image drawn over blank cells with the
//!   cursor saved and restored, block cells, or the fallback.
//! - For `LiveCoordinator` and the interactive painter,
//!   [`MicroGraphics::source`] is a [`PlacementSource`]: it swaps fallback
//!   cells for the mode's cells before a frame is built, and reports iTerm2
//!   and Sixel images (and Kitty uploads) as placements for the graphics side
//!   channel, so diffs stay exact.
//! - Every renderer checks [`Console::is_terminal`] at render time: an
//!   export or a capture keeps the fallback.
//!
//! # Fallback (#577)
//!
//! Where no protocol draws, an asset shows its emoji or text; failing both,
//! a half-block rendering of its image at exactly its cells (when the
//! terminal has colour); failing that, its alt text. `RICH_MICRO=blocks`
//! puts the block rendering first.
//!
//! # Animation (#572)
//!
//! Frames are resampled to the cell size, deduplicated and rate-limited
//! under the package's memory limits. Kitty (its frame protocol) and iTerm2
//! (an animated GIF) play them themselves; Sixel and blocks show the frame
//! the clock is on, like a spinner, whenever the view is redrawn, and the
//! source tells an event loop when the next frame is due.
//! `RICH_A11Y=reduced-motion` and `RICH_ANIMATION=0` show the still frame.
//!
//! ```no_run
//! use std::sync::Arc;
//! use rich::Console;
//! use rich_micro::{render_markup, FallbackPreference, Layer, MicroAsset, MicroGraphics, MicroRegistry};
//!
//! let mut registry = MicroRegistry::new();
//! registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
//! let registry = Arc::new(registry);
//!
//! // Detects the terminal once: Kitty, iTerm2, Sixel, blocks or text.
//! let graphics = MicroGraphics::detect(Arc::clone(&registry));
//! let console = Console::new();
//! let (text, _) = render_markup(&console, "Deploying :micro:ship:", &registry, FallbackPreference::Emoji);
//! console.print(&graphics.view(text));
//! # Ok::<(), rich_micro::MicroError>(())
//! ```

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rich::{ColorSystem, Console, Renderable, Segment, Style};
use rich_art::graphics::{overlay, CellPixels};
use rich_ext::capabilities::{ColorDepth, Environment, Graphics, SystemEnvironment};
use rich_ext::frame::{Frame, Graphic, Placement as FramePlacement, PlacementSource};
use rich_ext::graphics::GraphicsEnvironment;

use crate::cache::{user_cache_dir, ImageCache, Prepared};
use crate::model::MicroAsset;
use crate::package::Limits;
use crate::registry::MicroRegistry;
use crate::render::{
    fallback_cells, FallbackPreference, MicroMeta, MicroRenderer, MicroView, Placement, PAD_CELL,
};

/// How micro assets are drawn on one terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MicroMode {
    /// Kitty's graphics protocol, with Unicode placeholders.
    Kitty,
    /// iTerm2's inline images (also WezTerm).
    Iterm,
    /// Sixel, inline.
    Sixel,
    /// Half-block cells in colour.
    Blocks,
    /// The emoji or text fallback.
    Text,
}

impl MicroMode {
    /// `kitty`, `iterm`, `sixel`, `blocks` or `text`.
    pub fn name(self) -> &'static str {
        match self {
            MicroMode::Kitty => "kitty",
            MicroMode::Iterm => "iterm",
            MicroMode::Sixel => "sixel",
            MicroMode::Blocks => "blocks",
            MicroMode::Text => "text",
        }
    }

    /// Parse a `RICH_MICRO` value.
    pub fn parse(value: &str) -> Option<MicroMode> {
        match value.trim().to_ascii_lowercase().as_str() {
            "kitty" => Some(MicroMode::Kitty),
            "iterm" | "iterm2" => Some(MicroMode::Iterm),
            "sixel" => Some(MicroMode::Sixel),
            "blocks" | "block" | "halfblocks" => Some(MicroMode::Blocks),
            "text" | "emoji" | "none" | "off" | "0" => Some(MicroMode::Text),
            _ => None,
        }
    }

    /// Whether this mode draws with a graphics protocol.
    pub fn is_graphics(self) -> bool {
        matches!(self, MicroMode::Kitty | MicroMode::Iterm | MicroMode::Sixel)
    }
}

impl fmt::Display for MicroMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What [`select`] chose, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub mode: MicroMode,
    /// Why this mode, for `rich doctor`.
    pub reason: String,
    /// The cell size images are fitted to.
    pub cell: CellPixels,
    /// Whether the cell size was reported (otherwise it is 8×16).
    pub cell_known: bool,
    /// Whether animations play.
    pub animate: bool,
    /// Which fallback text to show where nothing is drawn.
    pub preference: FallbackPreference,
    /// Block cells before the emoji or text (`RICH_MICRO=blocks`).
    pub blocks_first: bool,
    /// The colour system block and Kitty cells are written in.
    pub color_system: Option<ColorSystem>,
    /// A `RICH_MICRO` value that was not understood.
    pub warning: Option<String>,
}

impl Selection {
    /// The text fallback, with nothing drawn: what a pipe gets.
    pub fn text(reason: &str) -> Selection {
        Selection {
            mode: MicroMode::Text,
            reason: reason.to_string(),
            cell: CellPixels::DEFAULT,
            cell_known: false,
            animate: false,
            preference: FallbackPreference::Emoji,
            blocks_first: false,
            color_system: None,
            warning: None,
        }
    }

    /// A given mode, for tests and callers that know their terminal. The
    /// caller vouches that the terminal can show it.
    pub fn forced(mode: MicroMode, cell: CellPixels) -> Selection {
        Selection {
            mode,
            reason: "set by the caller".to_string(),
            cell,
            cell_known: true,
            animate: false,
            preference: FallbackPreference::Emoji,
            blocks_first: mode == MicroMode::Blocks,
            color_system: Some(ColorSystem::Truecolor),
            warning: None,
        }
    }

    /// With animation on or off.
    pub fn animate(mut self, animate: bool) -> Selection {
        self.animate = animate;
        self
    }
}

/// Choose how to draw micro assets on the terminal `environment`
/// describes, reading `RICH_MICRO` from `env`. See the [module
/// docs](self).
pub fn select(environment: &GraphicsEnvironment, env: &dyn Environment) -> Selection {
    let cell = environment
        .cell_pixels
        .value
        .map(|c| CellPixels::new(u32::from(c.width), u32::from(c.height)));
    let color_system = environment.color.color_system();
    let preference = if environment.unicode {
        FallbackPreference::Emoji
    } else {
        FallbackPreference::Text
    };
    let base = |mode: MicroMode, reason: String| Selection {
        mode,
        reason,
        cell: cell.unwrap_or_default(),
        cell_known: cell.is_some(),
        animate: environment.animation.value,
        preference,
        blocks_first: false,
        color_system,
        warning: None,
    };
    if !environment.interactive {
        let mut selection = base(MicroMode::Text, "stdout is not a terminal".into());
        selection.animate = false;
        return selection;
    }
    let blocks_ok = environment.color != ColorDepth::None && environment.unicode;
    let mut warning = None;
    if let Some(value) = env.var("RICH_MICRO").filter(|v| !v.trim().is_empty()) {
        match MicroMode::parse(&value) {
            Some(MicroMode::Blocks) if !blocks_ok => {
                return base(
                    MicroMode::Text,
                    format!("RICH_MICRO={} but the terminal has no colour", value.trim()),
                );
            }
            Some(mode) => {
                let mut selection = base(mode, format!("RICH_MICRO={}", value.trim()));
                selection.blocks_first = mode == MicroMode::Blocks;
                return selection;
            }
            None => {
                warning = Some(format!(
                    "RICH_MICRO={value:?} is not kitty, iterm, sixel, blocks or text"
                ))
            }
        }
    }
    let mut selection = match environment.graphics.value {
        Graphics::Kitty => base(MicroMode::Kitty, environment.graphics.reason.clone()),
        Graphics::Iterm => base(MicroMode::Iterm, environment.graphics.reason.clone()),
        _ if environment.sixel.value && cell.is_some() => {
            base(MicroMode::Sixel, environment.sixel.reason.clone())
        }
        _ if blocks_ok => base(
            MicroMode::Blocks,
            if environment.sixel.value {
                "Sixel, but the cell size in pixels is unknown".into()
            } else {
                "no graphics protocol; colour cells".into()
            },
        ),
        _ => base(MicroMode::Text, "no graphics protocol and no colour".into()),
    };
    selection.warning = warning;
    selection
}

/// The animation clock: time since the session started.
type Clock = Arc<dyn Fn() -> Duration + Send + Sync>;

/// Kitty image ids: in the high byte a per-process base, so programs sharing
/// a terminal rarely collide.
fn kitty_base() -> u32 {
    ((std::process::id() % 200) + 32) << 16
}

#[derive(Default)]
struct KittyState {
    /// Image key to id.
    ids: HashMap<u64, u32>,
    /// Ids the terminal holds.
    transmitted: HashSet<u32>,
    next: u32,
}

struct Inner {
    registry: Arc<MicroRegistry>,
    selection: Selection,
    limits: Limits,
    cache: Mutex<ImageCache>,
    kitty: Mutex<KittyState>,
    clock: Clock,
    /// The animations shown lately, and when, for
    /// [`PlacementSource::next_change`].
    moving: Mutex<Vec<(Arc<Prepared>, Duration)>>,
    /// For rendering block cells and the like outside a print.
    console: Console,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// One terminal's micro-asset drawing state. Cheap to clone: clones share
/// it. See the [module docs](self).
#[derive(Clone)]
pub struct MicroGraphics {
    inner: Arc<Inner>,
}

impl fmt::Debug for MicroGraphics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MicroGraphics")
            .field("selection", &self.inner.selection)
            .finish_non_exhaustive()
    }
}

impl MicroGraphics {
    /// Drawing state for `selection`, with an in-memory cache only.
    pub fn new(registry: Arc<MicroRegistry>, selection: Selection) -> MicroGraphics {
        let started = Instant::now();
        let console = Console::builder()
            .force_terminal(true)
            .color_system(selection.color_system)
            .width(80)
            .build();
        MicroGraphics {
            inner: Arc::new(Inner {
                registry,
                selection,
                limits: Limits::default(),
                cache: Mutex::new(ImageCache::new(ImageCache::DEFAULT_BUDGET, None)),
                kitty: Mutex::new(KittyState {
                    next: 0,
                    ..KittyState::default()
                }),
                clock: Arc::new(move || started.elapsed()),
                moving: Mutex::new(Vec::new()),
                console,
            }),
        }
    }

    /// Detect the process's terminal ([`GraphicsEnvironment::system`]),
    /// select a mode, and cache optimised images under the user cache
    /// directory.
    pub fn detect(registry: Arc<MicroRegistry>) -> MicroGraphics {
        let selection = select(&GraphicsEnvironment::system(), &SystemEnvironment);
        MicroGraphics::new(registry, selection).with_disk_cache(user_cache_dir())
    }

    fn rebuild(self, change: impl FnOnce(&mut Inner)) -> MicroGraphics {
        match Arc::try_unwrap(self.inner) {
            Ok(mut inner) => {
                change(&mut inner);
                MicroGraphics {
                    inner: Arc::new(inner),
                }
            }
            // Shared already: settings are fixed.
            Err(inner) => MicroGraphics { inner },
        }
    }

    /// Cache optimised images in `dir` (or not, with `None`). Only before
    /// the state is shared.
    pub fn with_disk_cache(self, dir: Option<std::path::PathBuf>) -> MicroGraphics {
        self.rebuild(|inner| {
            let budget = ImageCache::DEFAULT_BUDGET;
            inner.cache = Mutex::new(ImageCache::new(budget, dir));
        })
    }

    /// Read packages under `limits`. Only before the state is shared.
    pub fn with_limits(self, limits: Limits) -> MicroGraphics {
        self.rebuild(|inner| inner.limits = limits)
    }

    /// Bound the in-memory image cache to `bytes`. Only before the state is
    /// shared.
    pub fn with_cache_budget(self, bytes: usize) -> MicroGraphics {
        self.rebuild(|inner| inner.cache = Mutex::new(ImageCache::new(bytes, None)))
    }

    /// Drive animation from `clock` (time since start) instead of the wall
    /// clock. Only before the state is shared.
    pub fn with_clock(self, clock: impl Fn() -> Duration + Send + Sync + 'static) -> MicroGraphics {
        self.rebuild(|inner| inner.clock = Arc::new(clock))
    }

    /// What was selected.
    pub fn selection(&self) -> &Selection {
        &self.inner.selection
    }

    /// The registry assets resolve in.
    pub fn registry(&self) -> &Arc<MicroRegistry> {
        &self.inner.registry
    }

    /// A renderer for printing: see the [module docs](self).
    pub fn renderer(&self) -> Arc<dyn MicroRenderer> {
        Arc::new(ModeRenderer {
            inner: Arc::clone(&self.inner),
            inline: true,
        })
    }

    /// `inner` with its micro assets drawn by [`renderer`](Self::renderer).
    pub fn view<R: Renderable>(&self, inner: R) -> MicroView<R> {
        MicroView::new(inner, Arc::clone(&self.inner.registry)).renderer(self.renderer())
    }

    /// The graphics source for `LiveCoordinator` and the interactive
    /// painter: see the [module docs](self).
    pub fn source(&self) -> Arc<dyn PlacementSource> {
        Arc::new(Source {
            inner: Arc::clone(&self.inner),
        })
    }

    /// The escape that deletes every Kitty image this session transmitted,
    /// for a program that is done with everything it drew (printed images
    /// disappear with it). Empty unless Kitty is in use.
    pub fn close(&self) -> String {
        self.inner.release(&[])
    }

    /// The Kitty ids the terminal holds for this session.
    pub fn kitty_ids(&self) -> Vec<u32> {
        let mut ids: Vec<u32> = lock(&self.inner.kitty)
            .transmitted
            .iter()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    /// Bytes the in-memory image cache holds.
    pub fn cache_bytes(&self) -> usize {
        lock(&self.inner.cache).used()
    }
}

impl Inner {
    fn prepared(&self, asset: &MicroAsset) -> Option<Arc<Prepared>> {
        let animate = self.selection.animate;
        lock(&self.cache).prepare(asset, self.selection.cell, animate, &self.limits)
    }

    fn now(&self) -> Duration {
        (self.clock)()
    }

    /// Kitty's id for `prepared`, and the transmission it still needs (only
    /// when `transmit`).
    fn kitty(&self, prepared: &Prepared, transmit: bool) -> Option<(u32, Option<String>)> {
        let truecolor = self.selection.color_system == Some(ColorSystem::Truecolor);
        let mut state = lock(&self.kitty);
        let id = match state.ids.get(&prepared.key) {
            Some(id) => *id,
            None => {
                let id = if truecolor {
                    kitty_base() | (state.next + 1)
                } else {
                    16 + state.next
                };
                if !truecolor && id > 255 || truecolor && state.next >= 0xFFFF {
                    return None;
                }
                state.next += 1;
                state.ids.insert(prepared.key, id);
                id
            }
        };
        if !transmit || state.transmitted.contains(&id) {
            return Some((id, None));
        }
        let escape = rich_art::kitty::transmit_animation(
            id,
            &prepared.frames,
            prepared.cols,
            prepared.rows,
        )?;
        state.transmitted.insert(id);
        Some((id, Some(escape)))
    }

    /// Kitty placeholder cells for image `id`, tagged with `style`.
    fn kitty_cells(
        &self,
        prepared: &Prepared,
        id: u32,
        style: Option<&Style>,
    ) -> Option<Vec<Segment>> {
        let truecolor = self.selection.color_system == Some(ColorSystem::Truecolor);
        let color = rich_art::kitty::id_color(id, truecolor)?;
        let style = style
            .cloned()
            .unwrap_or_default()
            .combine(&Style::new().with_color(color));
        // One row this release; the placeholder row is 0.
        let _ = prepared.rows;
        let cells = rich_art::kitty::placeholder_row(0, prepared.cols)?;
        Some(vec![Segment::new(cells, Some(style))])
    }

    /// The protocol escapes of each frame (iTerm2: one, possibly a GIF;
    /// Sixel: one per frame).
    fn escapes<'a>(&self, prepared: &'a Prepared) -> &'a [String] {
        prepared.escapes.get_or_init(|| match self.selection.mode {
            MicroMode::Iterm => {
                let bytes = if prepared.animated() {
                    rich_art::graphics::encode_gif(&prepared.frames)
                } else {
                    rich_art::graphics::encode_png(&prepared.frames[0].image)
                };
                bytes
                    .map(|bytes| {
                        vec![rich_art::iterm::inline_image(
                            &bytes,
                            prepared.cols,
                            prepared.rows,
                        )]
                    })
                    .unwrap_or_default()
            }
            MicroMode::Sixel => prepared
                .frames
                .iter()
                .map_while(|frame| {
                    rich_art::SixelArt::new(rich_art::image::DynamicImage::ImageRgba8(
                        frame.image.clone(),
                    ))
                    .cell_px(prepared.cell.width, prepared.cell.height)
                    .encode_cells(prepared.cols, prepared.rows)
                })
                .collect(),
            _ => Vec::new(),
        })
    }

    /// The overlay frame to show now: iTerm2 plays its GIF itself.
    fn overlay_frame(&self, prepared: &Prepared) -> usize {
        match self.selection.mode {
            MicroMode::Sixel => prepared.frame_at(self.now()).0,
            _ => 0,
        }
    }

    /// Block cells for the frame the clock is on, tagged with `style`.
    fn block_cells(&self, prepared: &Prepared, style: Option<&Style>) -> Option<Vec<Segment>> {
        let frames = prepared.blocks.get_or_init(|| {
            prepared
                .frames
                .iter()
                .map_while(|frame| {
                    blocks(&self.console, &frame.image, prepared.cols, prepared.rows)
                })
                .collect()
        });
        if frames.len() != prepared.frames.len() {
            return None;
        }
        let index = prepared.frame_at(self.now()).0;
        let tag = style.cloned().unwrap_or_default();
        Some(
            frames[index]
                .iter()
                .map(|segment| {
                    let style = segment
                        .style
                        .as_ref()
                        .map_or(tag.clone(), |s| s.combine(&tag));
                    Segment::new(segment.text.clone(), Some(style))
                })
                .collect(),
        )
    }

    /// Whether `asset` shows its emoji or text here rather than blocks.
    fn prefers_fallback(&self, asset: &MicroAsset) -> bool {
        let fallback = asset.fallback();
        let has = match self.selection.preference {
            FallbackPreference::Emoji => fallback.emoji.is_some() || fallback.text.is_some(),
            FallbackPreference::Text => fallback.text.is_some(),
            FallbackPreference::Alt => false,
        };
        has && !self.selection.blocks_first
    }

    /// Cells for `placement` in this mode, and the escapes to write before
    /// and after them when printing inline.
    fn draw(&self, placement: &Placement<'_>, inline: bool) -> Option<Drawn> {
        let style = placement.style.as_ref();
        let asset = placement.asset;
        let fallback = || Drawn {
            before: None,
            cells: vec![Segment::new(
                fallback_cells(asset, self.selection.preference),
                placement.style.clone(),
            )],
            after: None,
        };
        match self.selection.mode {
            MicroMode::Text => Some(fallback()),
            MicroMode::Blocks => {
                if self.prefers_fallback(asset) {
                    return Some(fallback());
                }
                match self.prepared(asset) {
                    Some(prepared) => {
                        self.moving(&prepared);
                        match self.block_cells(&prepared, style) {
                            Some(cells) => Some(Drawn::cells(cells)),
                            None => Some(fallback()),
                        }
                    }
                    None => Some(fallback()),
                }
            }
            MicroMode::Kitty => {
                let prepared = self.prepared(asset)?;
                let (id, transmit) = self.kitty(&prepared, inline)?;
                let cells = self.kitty_cells(&prepared, id, style)?;
                Some(Drawn {
                    before: transmit,
                    cells,
                    after: None,
                })
            }
            MicroMode::Iterm | MicroMode::Sixel => {
                let prepared = self.prepared(asset)?;
                let blank = vec![Segment::new(
                    PAD_CELL.to_string().repeat(asset.cols()),
                    placement.style.clone(),
                )];
                if !inline {
                    self.moving(&prepared);
                    return Some(Drawn::cells(blank));
                }
                let frame = self.overlay_frame(&prepared);
                let escape = self.escapes(&prepared).get(frame)?;
                let (before, after) = overlay(escape, asset.cols());
                Some(Drawn {
                    before: Some(before),
                    cells: blank,
                    after: Some(after),
                })
            }
        }
    }

    /// Note an animation shown, for [`PlacementSource::next_change`].
    fn moving(&self, prepared: &Arc<Prepared>) {
        let animated = prepared.animated()
            && matches!(self.selection.mode, MicroMode::Blocks | MicroMode::Sixel);
        if animated {
            let now = self.now();
            let mut moving = lock(&self.moving);
            match moving.iter_mut().find(|(p, _)| Arc::ptr_eq(p, prepared)) {
                Some(entry) => entry.1 = now,
                None => moving.push((Arc::clone(prepared), now)),
            }
        }
    }

    fn release(&self, retained: &[FramePlacement]) -> String {
        let keep: HashSet<u64> = retained.iter().map(|p| p.graphic.key()).collect();
        let mut state = lock(&self.kitty);
        let mut ids: Vec<u32> = state
            .transmitted
            .iter()
            .copied()
            .filter(|id| !keep.contains(&u64::from(*id)))
            .collect();
        ids.sort_unstable();
        let mut out = String::new();
        for id in ids {
            out.push_str(&rich_art::kitty::delete(id));
            state.transmitted.remove(&id);
            state.ids.retain(|_, v| *v != id);
        }
        out
    }
}

/// Block cells for `image` at exactly `cols × rows` cells, untagged.
fn blocks(
    console: &Console,
    image: &rich_art::image::RgbaImage,
    cols: usize,
    rows: usize,
) -> Option<Vec<Segment>> {
    use rich_art::{ImageArt, ImageBackground, ImageFit, ImageMode};
    let art = ImageArt::new(rich_art::image::DynamicImage::ImageRgba8(image.clone()))
        .mode(ImageMode::Blocks)
        .width(cols)
        .height(rows)
        .fit(ImageFit::Contain)
        .background_mode(ImageBackground::TerminalDefault);
    let options = console.options().update_width(cols);
    let segments = art.render(console, &options).ok()?;
    let line = Segment::split_lines(&segments).into_iter().next()?;
    // Rows beyond the first would be a block, not inline text: one row only.
    let line = Segment::adjust_line_length(&line, cols, None);
    (line.iter().map(Segment::cell_length).sum::<usize>() == cols).then_some(line)
}

/// What a mode draws for one placement.
struct Drawn {
    before: Option<String>,
    cells: Vec<Segment>,
    after: Option<String>,
}

impl Drawn {
    fn cells(cells: Vec<Segment>) -> Drawn {
        Drawn {
            before: None,
            cells,
            after: None,
        }
    }

    fn into_segments(self) -> Vec<Segment> {
        let mut out = Vec::with_capacity(self.cells.len() + 2);
        out.extend(self.before.map(Segment::control));
        out.extend(self.cells);
        out.extend(self.after.map(Segment::control));
        out
    }
}

/// The [`MicroRenderer`] of a mode. `inline` writes escapes into the output
/// (printing); otherwise only cells are swapped (frames).
struct ModeRenderer {
    inner: Arc<Inner>,
    inline: bool,
}

impl MicroRenderer for ModeRenderer {
    fn name(&self) -> &str {
        self.inner.selection.mode.name()
    }

    fn render(&self, placement: &Placement<'_>, console: &Console) -> Option<Vec<Segment>> {
        // An export, a capture or a pipe keeps the fallback.
        if !console.is_terminal() {
            return None;
        }
        let drawn = self.inner.draw(placement, self.inline)?;
        if !self.inline {
            return Some(drawn.cells);
        }
        // Kitty's colour-coded cells need the colour system they were made
        // for.
        if self.inner.selection.mode == MicroMode::Kitty
            && console.color_system() != self.inner.selection.color_system
        {
            return None;
        }
        Some(drawn.into_segments())
    }
}

/// A Kitty image, shown by the placeholder cells themselves: drawing it
/// only uploads it, once.
struct KittyGraphic {
    inner: Arc<Inner>,
    prepared: Arc<Prepared>,
    id: u32,
}

impl fmt::Debug for KittyGraphic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KittyGraphic")
            .field("id", &self.id)
            .finish()
    }
}

impl Graphic for KittyGraphic {
    fn key(&self) -> u64 {
        u64::from(self.id)
    }
    fn draw(&self, _frame: usize) -> String {
        self.inner
            .kitty(&self.prepared, true)
            .and_then(|(_, transmit)| transmit)
            .unwrap_or_default()
    }
    fn in_cells(&self) -> bool {
        true
    }
}

/// An iTerm2 or Sixel image drawn over blank cells.
struct OverlayGraphic {
    inner: Arc<Inner>,
    prepared: Arc<Prepared>,
}

impl fmt::Debug for OverlayGraphic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayGraphic")
            .field("key", &self.prepared.key)
            .finish()
    }
}

impl Graphic for OverlayGraphic {
    fn key(&self) -> u64 {
        self.prepared.key
    }
    fn draw(&self, frame: usize) -> String {
        match self.inner.escapes(&self.prepared).get(frame) {
            Some(escape) => format!("\x1b7{escape}\x1b8"),
            None => String::new(),
        }
    }
}

/// The [`PlacementSource`] of a [`MicroGraphics`].
struct Source {
    inner: Arc<Inner>,
}

impl PlacementSource for Source {
    fn prepare(&self, line: Vec<Segment>) -> Vec<Segment> {
        if self.inner.selection.mode == MicroMode::Text {
            return line;
        }
        let renderer: Arc<dyn MicroRenderer> = Arc::new(ModeRenderer {
            inner: Arc::clone(&self.inner),
            inline: false,
        });
        crate::render::rewrite(line, &self.inner.registry, &[renderer], &self.inner.console)
    }

    fn placements(&self, frame: &Frame) -> Vec<FramePlacement> {
        let mode = self.inner.selection.mode;
        if !mode.is_graphics() {
            return Vec::new();
        }
        let mut out = Vec::new();
        for row in 0..frame.height() {
            let mut column = 0;
            let mut open: Option<(MicroMeta, usize, usize)> = None;
            let mut runs: Vec<(Option<MicroMeta>, usize)> = frame
                .row(row)
                .iter()
                .map(|run| {
                    let meta = frame
                        .styles()
                        .get(run.style())
                        .and_then(MicroMeta::from_style);
                    (meta, run.cells())
                })
                .collect();
            runs.push((None, 0));
            for (meta, cells) in runs {
                let continues =
                    matches!((&open, &meta), (Some((o, _, _)), Some(m)) if o.id == m.id);
                if continues {
                    if let Some(open) = open.as_mut() {
                        open.2 += cells;
                    }
                } else {
                    if let Some((meta, start, width)) = open.take() {
                        if let Some(placement) = self.placement(row, start, width, &meta) {
                            out.push(placement);
                        }
                    }
                    open = meta.map(|meta| (meta, column, cells));
                }
                column += cells;
            }
        }
        out
    }

    fn next_change(&self) -> Option<Duration> {
        if !self.inner.selection.animate {
            return None;
        }
        let now = self.inner.now();
        let mut moving = lock(&self.inner.moving);
        // An animation not drawn for a whole cycle (and a second) has left
        // the view.
        moving.retain(|(prepared, seen)| {
            now.saturating_sub(*seen) <= prepared.duration() + Duration::from_secs(1)
        });
        moving
            .iter()
            .filter_map(|(prepared, _)| prepared.frame_at(now).1)
            .min()
    }

    fn release(&self, retained: &[FramePlacement]) -> String {
        self.inner.release(retained)
    }
}

impl Source {
    fn placement(
        &self,
        row: usize,
        column: usize,
        width: usize,
        meta: &MicroMeta,
    ) -> Option<FramePlacement> {
        let asset = self.inner.registry.resolve(&meta.name)?;
        if asset.cols() != meta.cols || width != meta.cols {
            return None;
        }
        let prepared = self.inner.prepared(asset)?;
        let (graphic, frame): (Arc<dyn Graphic>, usize) = match self.inner.selection.mode {
            MicroMode::Kitty => {
                let (id, _) = self.inner.kitty(&prepared, false)?;
                (
                    Arc::new(KittyGraphic {
                        inner: Arc::clone(&self.inner),
                        prepared,
                        id,
                    }),
                    0,
                )
            }
            MicroMode::Iterm | MicroMode::Sixel => {
                let frame = self.inner.overlay_frame(&prepared);
                self.inner.moving(&prepared);
                (
                    Arc::new(OverlayGraphic {
                        inner: Arc::clone(&self.inner),
                        prepared,
                    }),
                    frame,
                )
            }
            _ => return None,
        };
        Some(FramePlacement {
            row,
            column,
            cols: meta.cols,
            rows: 1,
            graphic,
            frame,
        })
    }
}

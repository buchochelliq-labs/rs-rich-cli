//! An asset picker (#461, `rich asset`): emoji by shortcode name, table and
//! panel box styles, spinners and (with the `micro` feature) micro assets,
//! with a fuzzy search and a preview.
//!
//! Every list comes from core's public API: emoji glyphs through
//! `rich::emoji::replace` (the names are those of core's table), the box
//! styles `rich::box` defines, and `rich::spinner::spinner_names`. The
//! answer is the emoji itself, or the box style's or spinner's name as
//! `rich` options take it (`rounded`, `dots`).

use std::sync::Arc;

use rich::r#box::{self as boxes, Box as BoxSet};
use rich::{Console, ConsoleOptions, Renderable, Segment};

use crate::component::{Component, Context, Flow, View};
use crate::components::{PreviewLayout, Select, Theme};
use crate::event::{Event, Key};
use crate::item::{Actions, Item, Preview};
use crate::keymap::Keymap;
use crate::names::EMOJI;
use crate::policy::{LineIo, NotInteractive};

/// What to pick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetKind {
    #[default]
    Emoji,
    /// A box style for tables and panels.
    Box,
    Spinner,
    /// A micro asset (`rs-rich-micro`), by name: drawn in the list and
    /// magnified in the preview. Needs the `micro` feature.
    #[cfg(feature = "micro")]
    Micro,
}

impl AssetKind {
    pub fn parse(name: &str) -> Option<AssetKind> {
        Some(match name {
            "emoji" => AssetKind::Emoji,
            "box" => AssetKind::Box,
            "spinner" => AssetKind::Spinner,
            #[cfg(feature = "micro")]
            "micro" => AssetKind::Micro,
            _ => return None,
        })
    }
}

/// Every box style core defines, by the name `rich` options use.
pub const BOX_STYLES: &[(&str, BoxSet)] = &[
    ("ascii", boxes::ASCII),
    ("ascii2", boxes::ASCII2),
    ("ascii_double_head", boxes::ASCII_DOUBLE_HEAD),
    ("square", boxes::SQUARE),
    ("square_double_head", boxes::SQUARE_DOUBLE_HEAD),
    ("minimal", boxes::MINIMAL),
    ("minimal_heavy_head", boxes::MINIMAL_HEAVY_HEAD),
    ("minimal_double_head", boxes::MINIMAL_DOUBLE_HEAD),
    ("simple", boxes::SIMPLE),
    ("simple_head", boxes::SIMPLE_HEAD),
    ("simple_heavy", boxes::SIMPLE_HEAVY),
    ("horizontals", boxes::HORIZONTALS),
    ("rounded", boxes::ROUNDED),
    ("heavy", boxes::HEAVY),
    ("heavy_edge", boxes::HEAVY_EDGE),
    ("heavy_head", boxes::HEAVY_HEAD),
    ("double", boxes::DOUBLE),
    ("double_edge", boxes::DOUBLE_EDGE),
    ("markdown", boxes::MARKDOWN),
];

/// The emoji a shortcode name stands for, through core.
pub fn emoji(name: &str) -> Option<String> {
    let code = format!(":{name}:");
    let glyph = rich::emoji::replace(&code);
    (glyph != code).then_some(glyph)
}

/// The cell size micro previews are magnified at: the default guess, so a
/// preview looks the same everywhere.
#[cfg(feature = "micro")]
fn rich_art_cell() -> rich_micro::CellPixels {
    rich_micro::CellPixels::DEFAULT
}

/// A micro asset's preview: the asset inline, its image magnified, and what
/// it says about itself.
#[cfg(feature = "micro")]
struct MicroPreview {
    asset: Arc<rich_micro::MicroAsset>,
    still: Option<rich_micro::pipeline::Magnified>,
}

#[cfg(feature = "micro")]
impl Renderable for MicroPreview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let asset = &self.asset;
        let mut head = rich_micro::placeholder(asset, rich_micro::FallbackPreference::Emoji);
        head.append(
            &format!(" {}", asset.name()),
            Some(rich::Style::parse("bold").unwrap_or_default().into()),
        );
        let mut out = head.rich_render(console, options);
        out.push(Segment::line());
        if let Some(still) = &self.still {
            out.extend(still.rich_render(console, options));
        }
        let fallback = asset.fallback();
        let mut about = format!(
            "{}\n{} {} · emoji {} · text {}",
            asset.alt(),
            asset.size(),
            asset.kind().as_str(),
            fallback.emoji.as_deref().unwrap_or("-"),
            fallback.text.as_deref().unwrap_or("-"),
        );
        if let Some(license) = asset.license() {
            about.push_str(&format!("\nlicence {license}"));
        }
        about.push_str(&format!("\n:micro:{}:", asset.name()));
        out.extend(rich::Text::new(about).rich_render(console, options));
        out
    }
}

/// A small table drawn with a box style.
struct BoxPreview(BoxSet);

impl Renderable for BoxPreview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut table = rich::Table::new().box_set(self.0);
        table.add_column("Name").add_column("Value");
        table.add_row(&["alpha", "1"]).add_row(&["beta", "2"]);
        table.rich_render(console, options)
    }
}

/// Pick an emoji, a box style or a spinner. Returns the emoji, or the
/// style's or spinner's name.
pub struct AssetPicker {
    kind: AssetKind,
    select: Select<String>,
}

impl AssetPicker {
    pub fn new(prompt: impl Into<String>, kind: AssetKind) -> AssetPicker {
        #[cfg(feature = "micro")]
        if kind == AssetKind::Micro {
            return AssetPicker::micro(prompt, &rich_micro::MicroRegistry::builtin());
        }
        let mut prefixes = Vec::new();
        let items: Vec<Item<String>> = match kind {
            AssetKind::Emoji => EMOJI
                .iter()
                .filter_map(|name| {
                    let glyph = emoji(name)?;
                    prefixes.push(format!("{glyph} "));
                    let preview = format!("{glyph}\n\n:{name}:");
                    Some(Item::new(glyph, *name).preview(Preview::Text(preview)))
                })
                .collect(),
            AssetKind::Box => BOX_STYLES
                .iter()
                .map(|(name, style)| {
                    Item::new(name.to_string(), *name)
                        .preview(Preview::Renderable(Arc::new(BoxPreview(*style))))
                })
                .collect(),
            AssetKind::Spinner => rich::spinner::spinner_names()
                .iter()
                .filter_map(|name| {
                    let (interval, frames) = rich::spinner::spinner_frames(name)?;
                    prefixes.push(format!("{} ", frames.first().copied().unwrap_or("")));
                    let preview = format!(
                        "{}\n\n{} frames, every {interval} ms",
                        frames.join("  "),
                        frames.len()
                    );
                    Some(Item::new(name.to_string(), *name).preview(Preview::Text(preview)))
                })
                .collect(),
            #[cfg(feature = "micro")]
            AssetKind::Micro => unreachable!("built by AssetPicker::micro"),
        };
        let mut select = Select::new(prompt, items);
        select.prefixes = prefixes;
        select.prefix_plain = true;
        AssetPicker { kind, select }
    }

    /// Pick one of `registry`'s micro assets; the answer is its name. Each
    /// row shows the asset (its placeholder, which the painter's graphics
    /// draw over where the terminal can; its emoji or text elsewhere) and
    /// its alt text; the preview magnifies its image.
    #[cfg(feature = "micro")]
    pub fn micro(prompt: impl Into<String>, registry: &rich_micro::MicroRegistry) -> AssetPicker {
        let mut cache = rich_micro::ImageCache::new(8 << 20, None);
        let limits = rich_micro::Limits::default();
        let mut icons = Vec::new();
        let items: Vec<Item<String>> = registry
            .assets()
            .into_iter()
            .map(|asset| {
                icons.push(Some(rich_micro::placeholder(
                    asset,
                    rich_micro::FallbackPreference::Emoji,
                )));
                let still = cache
                    .prepare(asset, rich_art_cell(), false, &limits)
                    .map(|prepared| {
                        rich_micro::pipeline::Magnified(prepared.frames[0].image.clone())
                    });
                let preview = MicroPreview {
                    asset: std::sync::Arc::clone(asset),
                    still,
                };
                Item::new(asset.name().to_string(), asset.name())
                    .description(asset.alt())
                    .preview(Preview::Renderable(Arc::new(preview)))
            })
            .collect();
        let mut select = Select::new(prompt, items);
        select.set_icons(icons);
        AssetPicker {
            kind: AssetKind::Micro,
            select,
        }
    }

    pub fn kind(&self) -> AssetKind {
        self.kind
    }

    /// Show at most `rows` at once (default 10).
    pub fn height(mut self, rows: usize) -> Self {
        self.select = self.select.height(rows);
        self
    }

    pub fn preview(mut self, layout: PreviewLayout) -> Self {
        self.select = self.select.preview(layout);
        self
    }

    /// Start with this filter text.
    pub fn query(mut self, query: impl Into<String>) -> Self {
        self.select = self.select.query(query);
        self
    }

    /// The asset with this name (an emoji's shortcode name, a style's or
    /// spinner's name) is focused first, and returned without a terminal.
    pub fn default(mut self, name: &str) -> Self {
        let name = name.trim_matches(':');
        if let Some(index) = self
            .select
            .items()
            .iter()
            .position(|item| item.label == name)
        {
            self.select = self.select.default(index);
        }
        self
    }

    pub fn theme(mut self, theme: Theme) -> Self {
        self.select = self.select.theme(theme);
        self
    }

    /// Actions offered on every asset (#491).
    pub fn actions(mut self, actions: Actions) -> Self {
        self.select = self.select.actions(actions);
        self
    }

    pub fn menu_key(mut self, key: Key) -> Self {
        self.select = self.select.menu_key(key);
        self
    }

    /// Report the mouse: see [`Select::with_mouse`].
    pub fn with_mouse(mut self, on: bool) -> Self {
        self.select = self.select.with_mouse(on);
        self
    }

    pub fn action(&self) -> Option<&str> {
        self.select.action()
    }

    pub fn items(&self) -> &[Item<String>] {
        self.select.items()
    }
}

impl Component for AssetPicker {
    type Output = String;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<String> {
        self.select.handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.select.render(context)
    }

    fn mouse(&self) -> bool {
        Component::mouse(&self.select)
    }

    fn keymap(&self) -> Keymap {
        self.select.visible_keymap()
    }

    fn default_value(&self) -> Option<String> {
        self.select.default_value()
    }

    /// Without a terminal: a name, the best fuzzy match answering. The
    /// list is too long to print, so it is not listed.
    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<String>, NotInteractive> {
        let what = match self.kind {
            AssetKind::Emoji => "Emoji name",
            AssetKind::Box => "Box style",
            AssetKind::Spinner => "Spinner",
            #[cfg(feature = "micro")]
            AssetKind::Micro => "Micro asset",
        };
        let default = self.select.default_value();
        let default_label = self
            .select
            .focused()
            .filter(|_| default.is_some())
            .map(|index| self.select.items()[index].label.clone());
        match &default_label {
            Some(label) => io.write(&format!("{what} [{label}]: ")),
            None => io.write(&format!("{what}: ")),
        }
        let Some(line) = io.read_line() else {
            return default.map(Some).ok_or(NotInteractive::Ended);
        };
        let line = line.trim().trim_matches(':');
        if line.is_empty() {
            return default
                .map(Some)
                .ok_or_else(|| NotInteractive::Invalid("nothing chosen".into()));
        }
        let items = self.select.items();
        if let Some(item) = items.iter().find(|item| item.label == line) {
            return Ok(Some(item.value.clone()));
        }
        let labels = items.iter().map(|item| item.label.as_str());
        crate::fuzzy::rank(line, labels)
            .first()
            .map(|(index, _)| Some(items[*index].value.clone()))
            .ok_or_else(|| NotInteractive::Invalid(format!("nothing matches {line:?}")))
    }
}

/// A text rendering of a preview, for tests.
#[cfg(test)]
fn plain(renderable: &dyn Renderable, width: usize) -> String {
    let console = Console::builder()
        .width(width)
        .force_terminal(false)
        .build();
    let options = console.options();
    console
        .render_lines(renderable, &options, false)
        .iter()
        .map(|line| line.iter().map(|s| s.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_emoji_name_resolves_through_core() {
        for name in EMOJI {
            assert!(emoji(name).is_some(), "{name}");
        }
        assert_eq!(emoji("thumbs_up").as_deref(), Some("👍"));
    }

    #[test]
    fn box_previews_draw_with_their_style() {
        let shown = plain(&BoxPreview(boxes::DOUBLE), 30);
        assert!(shown.contains('╔') && shown.contains("alpha"), "{shown}");
    }

    #[test]
    fn lists_every_kind() {
        let emoji = AssetPicker::new("Emoji", AssetKind::Emoji);
        assert!(emoji.items().len() > 2000);
        let boxes = AssetPicker::new("Box", AssetKind::Box);
        assert_eq!(boxes.items().len(), BOX_STYLES.len());
        let spinners = AssetPicker::new("Spinner", AssetKind::Spinner);
        assert_eq!(spinners.items().len(), rich::spinner::spinner_names().len());
    }

    #[cfg(feature = "micro")]
    #[test]
    fn lists_micro_assets_with_their_icons_and_previews() {
        let picker = AssetPicker::new("Micro", AssetKind::Micro);
        assert!(picker
            .items()
            .iter()
            .any(|item| item.label == "status/success"));
        let item = picker
            .items()
            .iter()
            .find(|item| item.label == "status/loading")
            .unwrap();
        let shown = plain(&*item.preview.as_ref().unwrap().renderable(), 40);
        assert!(
            shown.contains("status/loading") && shown.contains("ring of dots"),
            "{shown}"
        );
        assert!(shown.contains('▀'), "{shown}");
        let (outcome, record) = crate::headless::run(
            AssetPicker::new("Micro", AssetKind::Micro).query("heart"),
            crate::headless::Script::new().keys("enter"),
            60,
            12,
        );
        assert_eq!(outcome.unwrap().value().unwrap(), "fun/heart");
        assert!(record.frames.iter().any(|frame| frame.contains("💖")));
    }
}

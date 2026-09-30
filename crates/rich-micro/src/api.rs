//! The Rust API (#570): an extension trait on core's [`Text`] and a
//! renderable, so micro assets compose with everything else without a core
//! method.

use std::sync::Arc;

use rich::measure::Measurement;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar};

use crate::error::MicroError;
use crate::markup::{expand, Diagnostic, MicroTransform};
use crate::model::MicroAsset;
use crate::registry::MicroRegistry;
use crate::render::{placeholder, FallbackPreference};

/// Micro assets on [`Text`].
///
/// ```
/// use rich::Text;
/// use rich_micro::{Layer, MicroAsset, MicroExt, MicroRegistry};
///
/// let mut registry = MicroRegistry::new();
/// registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
/// let mut text = Text::new("Deploying ");
/// text.append_micro(&registry, "ship")?;
/// assert_eq!(text.plain(), "Deploying 🚀");
/// assert_eq!(text.cell_len(), 12);
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
pub trait MicroExt {
    /// Append the asset `name` resolves to, as its placeholder cells.
    fn append_micro(&mut self, registry: &MicroRegistry, name: &str) -> Result<&mut Self, MicroError>;

    /// Append `asset`'s placeholder cells.
    fn append_micro_asset(&mut self, asset: &MicroAsset) -> &mut Self;

    /// Replace the `:micro:name:` tokens in this text (see
    /// [`markup`](crate::markup) for when to prefer the markup path).
    fn expand_micro(&mut self, registry: &MicroRegistry) -> Vec<Diagnostic>;
}

impl MicroExt for Text {
    fn append_micro(&mut self, registry: &MicroRegistry, name: &str) -> Result<&mut Self, MicroError> {
        let asset = registry.require(name)?;
        Ok(self.append_micro_asset(asset))
    }

    fn append_micro_asset(&mut self, asset: &MicroAsset) -> &mut Self {
        let appended = std::mem::take(self).append_text(&placeholder(asset, FallbackPreference::Emoji));
        *self = appended;
        self
    }

    fn expand_micro(&mut self, registry: &MicroRegistry) -> Vec<Diagnostic> {
        let (text, diagnostics) = expand(self, registry, FallbackPreference::Emoji);
        *self = text;
        diagnostics
    }
}

/// One asset as a renderable: exactly its columns wide, in any layout.
///
/// ```
/// use rich_micro::{MicroAsset, MicroAssetRef};
/// use rich::{Console, Renderable};
///
/// let asset = MicroAsset::new("ok", "check")?.with_text("OK")?;
/// let console = Console::builder().width(20).build();
/// let micro = MicroAssetRef::new(asset);
/// let measured = micro.measure(&console, &console.options());
/// assert_eq!((measured.minimum, measured.maximum), (2, 2));
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug)]
pub struct MicroAssetRef {
    asset: Arc<MicroAsset>,
    preference: FallbackPreference,
}

impl MicroAssetRef {
    pub fn new(asset: impl Into<Arc<MicroAsset>>) -> Self {
        MicroAssetRef {
            asset: asset.into(),
            preference: FallbackPreference::default(),
        }
    }

    /// The asset `name` resolves to in `registry`.
    pub fn from_registry(registry: &MicroRegistry, name: &str) -> Result<Self, MicroError> {
        Ok(MicroAssetRef::new(Arc::clone(registry.require(name)?)))
    }

    pub fn preference(mut self, preference: FallbackPreference) -> Self {
        self.preference = preference;
        self
    }

    pub fn asset(&self) -> &MicroAsset {
        &self.asset
    }

    /// Its placeholder, as a [`Text`]. Each call is a new occurrence.
    pub fn text(&self) -> Text {
        placeholder(&self.asset, self.preference)
    }
}

impl Renderable for MicroAssetRef {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.text().rich_render(console, options)
    }

    fn measure(&self, _console: &Console, options: &ConsoleOptions) -> Measurement {
        let cols = self.asset.cols();
        Measurement::new(cols, cols).clamp(None, Some(options.max_width))
    }

    fn fit_to_measurement(&self) -> bool {
        true
    }
}

/// Registers the substitution as the text transform `micro`, for hosts that
/// build transform pipelines from plugins.
#[derive(Clone, Debug)]
pub struct MicroPlugin {
    registry: Arc<MicroRegistry>,
}

impl MicroPlugin {
    pub fn new(registry: Arc<MicroRegistry>) -> Self {
        MicroPlugin { registry }
    }
}

impl Plugin for MicroPlugin {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("micro", "Micro assets", env!("CARGO_PKG_VERSION"))
            .description("Replaces :micro:name: tokens with inline micro assets")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.transform("micro", Arc::new(MicroTransform::new(Arc::clone(&self.registry))));
        Ok(())
    }
}

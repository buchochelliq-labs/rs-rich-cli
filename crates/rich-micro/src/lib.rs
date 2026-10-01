//! Micro assets for rs-rich (#565): emoji-sized inline images and
//! animations, written `Deploying :micro:rocket:`.
//!
//! Not a port of anything upstream: an rs-rich addition, outside core. Core
//! is unchanged: `:micro:name:` is not an emoji code, so it passes through
//! core untouched, and every seam here goes through core's public API (style
//! metadata, [`Renderable`](rich::Renderable)) or rs-rich-ext's.
//!
//! - [`MicroAsset`]: a name, a kind, a size in cells (2×1 by default, 1×1
//!   allowed, one row only), images, mandatory alt text, an emoji or text
//!   fallback, and where it came from.
//! - [`package`]: `.richmicro` packages and packs, read under hard
//!   [`Limits`].
//! - [`MicroRegistry`]: built-in < user < trusted project < inline, with
//!   aliases, collisions and [`explain`](MicroRegistry::explain).
//! - [`markup`]: `:micro:name:` in markup and text; [`MicroExt`] and
//!   [`MicroAssetRef`] in code.
//! - [`render`]: the placeholder representation, and the [`MicroRenderer`]
//!   seam that terminal graphics plug into.
//! - [`graphics`]: drawing on a terminal: [`select`] picks Kitty, iTerm2,
//!   Sixel, half-blocks or the text fallback, and [`MicroGraphics`] draws
//!   with it, in printed output and through the graphics side channel of
//!   `rich-ext`'s live regions and the interactive painter.
//! - [`cache`]: decoded images fitted to their cells, cached in memory and
//!   on disk.
//! - [`pipeline`]: any picture or animation to an asset's images: fitted,
//!   adjusted, sharpened, with its transparency decided; [`create`] writes
//!   the package.
//! - [`builtin`]: the built-in library (status, dev and fun sets), drawn for
//!   this project; [`MicroRegistry::builtin`] loads it.
//!
//! ```
//! use rich::Console;
//! use rich_micro::{render_markup, FallbackPreference, Layer, MicroAsset, MicroRegistry};
//!
//! let mut registry = MicroRegistry::new();
//! registry.add(Layer::Inline, MicroAsset::new("ship", "rocket")?.with_emoji("🚀")?)?;
//! let console = Console::builder().width(40).build();
//! let (text, diagnostics) =
//!     render_markup(&console, "Deploying :micro:ship: :fire:", &registry, FallbackPreference::Emoji);
//! assert_eq!(text.plain(), "Deploying 🚀 🔥");
//! assert!(diagnostics.is_empty());
//! # Ok::<(), rich_micro::MicroError>(())
//! ```

pub mod api;
pub mod builtin;
pub mod cache;
pub mod create;
pub mod error;
pub mod graphics;
pub mod image;
pub mod markup;
pub mod model;
pub mod name;
pub mod package;
pub mod pipeline;
pub mod registry;
pub mod render;

pub use api::{MicroAssetRef, MicroExt, MicroPlugin};
pub use cache::{ImageCache, Prepared};
pub use error::MicroError;
pub use graphics::{select, MicroGraphics, MicroMode, Selection};
pub use markup::{
    expand, markup_text, render_markup, Diagnostic, DiagnosticKind, MicroTransform, PreparedMarkup,
};
pub use model::{
    AssetKind, CellSize, Fallback, ImageFormat, ImageInfo, ImageRef, Layer, MicroAsset, Origin,
    PackageLocation, Variants,
};
pub use name::is_valid_name;
pub use package::{Limits, Pack};
pub use registry::{Collision, LoadReport, MicroRegistry, MicroRoots, Rejected};
pub use render::{
    fallback_cells, placeholder, FallbackPreference, FallbackRenderer, MicroMeta, MicroRenderer,
    MicroView, Placement, MICRO_META_KEY, PAD_CELL,
};
/// The size of a terminal cell in pixels, as the cache and pipeline take it.
pub use rich_art::graphics::CellPixels;

//! The asset model (#566, #579, #585): what a micro asset is, independent of
//! where it was loaded from or how a terminal will draw it.

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use rich::cells::cell_len;

use crate::error::MicroError;
use crate::name::check_name;

/// The widest asset, in cells. Micro assets are emoji-sized; anything wider
/// is an image, not an inline icon.
pub const MAX_COLS: u16 = 4;
/// The tallest asset, in rows. Only single-row (inline) sizes exist in this
/// release; multi-row sizes are blocks, left for later.
pub const MAX_ROWS: u16 = 1;
/// The longest alt text, in characters.
pub const MAX_ALT_CHARS: usize = 200;

/// Whether an asset is a still image or moves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AssetKind {
    #[default]
    Static,
    Animated,
}

impl AssetKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AssetKind::Static => "static",
            AssetKind::Animated => "animated",
        }
    }
}

impl FromStr for AssetKind {
    type Err = MicroError;
    fn from_str(value: &str) -> Result<Self, MicroError> {
        match value {
            "static" => Ok(AssetKind::Static),
            "animated" => Ok(AssetKind::Animated),
            other => Err(MicroError::Manifest(format!(
                "unknown kind {other:?}: use \"static\" or \"animated\""
            ))),
        }
    }
}

/// An asset's footprint in terminal cells: columns × rows. The default is
/// 2×1, the footprint of an emoji (a cell is about twice as tall as it is
/// wide); 1×1 is allowed. Rows above one are rejected in this release.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellSize {
    cols: u16,
    rows: u16,
}

impl Default for CellSize {
    fn default() -> Self {
        CellSize { cols: 2, rows: 1 }
    }
}

impl CellSize {
    /// `cols` × `rows`, checked: 1..=[`MAX_COLS`] columns and exactly one row.
    pub fn new(cols: u16, rows: u16) -> Result<Self, MicroError> {
        if rows > MAX_ROWS {
            return Err(MicroError::InvalidSize(format!(
                "micro asset size {cols}x{rows}: multi-row sizes are not supported yet; \
                 rows must be 1"
            )));
        }
        if cols == 0 || rows == 0 {
            return Err(MicroError::InvalidSize(format!(
                "micro asset size {cols}x{rows}: columns and rows must be at least 1"
            )));
        }
        if cols > MAX_COLS {
            return Err(MicroError::InvalidSize(format!(
                "micro asset size {cols}x{rows}: at most {MAX_COLS} columns"
            )));
        }
        Ok(CellSize { cols, rows })
    }

    pub fn cols(self) -> usize {
        self.cols as usize
    }

    pub fn rows(self) -> usize {
        self.rows as usize
    }
}

impl fmt::Display for CellSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.cols, self.rows)
    }
}

impl FromStr for CellSize {
    type Err = MicroError;
    /// `"2x1"`, `"1x1"`.
    fn from_str(value: &str) -> Result<Self, MicroError> {
        let bad = || {
            MicroError::InvalidSize(format!(
                "micro asset size {value:?}: write columns x rows, like \"2x1\""
            ))
        };
        let (cols, rows) = value.split_once(['x', 'X']).ok_or_else(bad)?;
        let parse = |part: &str| {
            if part.is_empty() || part.len() > 3 || !part.bytes().all(|b| b.is_ascii_digit()) {
                Err(bad())
            } else {
                part.parse::<u16>().map_err(|_| bad())
            }
        };
        CellSize::new(parse(cols)?, parse(rows)?)
    }
}

/// An image file format a package may hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageFormat {
    Png,
    /// Animated PNG: a PNG with an `acTL` chunk.
    Apng,
    Gif,
    WebP,
}

impl ImageFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Apng => "apng",
            ImageFormat::Gif => "gif",
            ImageFormat::WebP => "webp",
        }
    }
}

/// What an image's header says, read without decoding it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub width: u32,
    pub height: u32,
    /// 1 for a still image.
    pub frames: u32,
}

impl ImageInfo {
    /// The bytes it takes decoded, as RGBA, every frame at canvas size.
    pub fn decoded_bytes(&self) -> u64 {
        self.width as u64 * self.height as u64 * 4 * self.frames.max(1) as u64
    }
}

/// One image in a package: its path inside the package, its header and its
/// size on disk. The bytes are read on demand
/// ([`MicroAsset::read_variant`]), never kept.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageRef {
    pub path: String,
    pub info: ImageInfo,
    pub bytes: u64,
}

/// The images an asset can be drawn from. An asset built in code may have
/// none and render only its fallback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Variants {
    /// The still image; also the static frame of an animation.
    pub static_image: Option<ImageRef>,
    /// A GIF, APNG or WebP animation.
    pub animation: Option<ImageRef>,
    /// An animation as a sequence of still images.
    pub frames: Vec<ImageRef>,
}

impl Variants {
    pub fn is_empty(&self) -> bool {
        self.static_image.is_none() && self.animation.is_none() && self.frames.is_empty()
    }

    /// Every image, static first.
    pub fn iter(&self) -> impl Iterator<Item = &ImageRef> {
        self.static_image
            .iter()
            .chain(self.animation.iter())
            .chain(self.frames.iter())
    }
}

/// What to show where no image can be drawn: an emoji, a short text, or both.
/// Each must fit the asset's columns and hold no whitespace or control
/// characters. With neither, the alt text is used, cut to fit.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Fallback {
    pub emoji: Option<String>,
    pub text: Option<String>,
}

/// A registry layer, in ascending precedence: a later layer wins.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Layer {
    /// Shipped with the library.
    BuiltIn,
    /// The user's own assets (`~/.config/rich/micro/`).
    User,
    /// A trusted project's assets (`.rich/micro/`).
    Project,
    /// Added in code.
    Inline,
}

impl Layer {
    pub const ALL: [Layer; 4] = [Layer::BuiltIn, Layer::User, Layer::Project, Layer::Inline];

    pub fn as_str(self) -> &'static str {
        match self {
            Layer::BuiltIn => "built-in",
            Layer::User => "user",
            Layer::Project => "project",
            Layer::Inline => "inline",
        }
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a package's files live, so its images can be read later.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PackageLocation {
    /// A package directory.
    Directory(PathBuf),
    /// A zip archive, and the package's folder inside it (`""` at its root).
    Archive { path: PathBuf, prefix: String },
    /// The library's own built-in set, compiled in: the package's folder in
    /// it (`status/success`).
    BuiltIn(String),
}

/// Where an asset came from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Origin {
    pub layer: Layer,
    /// The pack it was installed with, if any.
    pub pack: Option<String>,
    /// Its package, when it was loaded from one.
    pub location: Option<PackageLocation>,
}

impl Default for Origin {
    fn default() -> Self {
        Origin {
            layer: Layer::Inline,
            pack: None,
            location: None,
        }
    }
}

impl fmt::Display for Origin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.layer.as_str())?;
        if let Some(pack) = &self.pack {
            write!(f, " pack {pack}")?;
        }
        match &self.location {
            Some(PackageLocation::Directory(path)) => write!(f, " ({})", path.display()),
            Some(PackageLocation::Archive { path, prefix }) if prefix.is_empty() => {
                write!(f, " ({})", path.display())
            }
            Some(PackageLocation::Archive { path, prefix }) => {
                write!(f, " ({}!{prefix})", path.display())
            }
            Some(PackageLocation::BuiltIn(prefix)) => write!(f, " (builtin:{prefix})"),
            None => Ok(()),
        }
    }
}

/// An emoji-sized inline image or animation.
///
/// ```
/// use rich_micro::{CellSize, MicroAsset};
///
/// let asset = MicroAsset::new("status/ok", "green check mark")?
///     .with_emoji("✅")?
///     .with_text("OK")?;
/// assert_eq!(asset.size(), CellSize::default()); // 2x1
/// assert!(MicroAsset::new("status/ok", "check")?.with_size(CellSize::new(1, 1)?)?.with_emoji("✅").is_err());
/// # Ok::<(), rich_micro::MicroError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MicroAsset {
    name: String,
    kind: AssetKind,
    size: CellSize,
    variants: Variants,
    alt: String,
    fallback: Fallback,
    aliases: Vec<String>,
    origin: Origin,
    version: Option<String>,
    license: Option<String>,
    author: Option<String>,
}

impl MicroAsset {
    /// A static 2×1 asset with no images, in the inline layer. `alt` is
    /// mandatory: it is what screen readers, logs and plain exports show.
    pub fn new(name: &str, alt: &str) -> Result<Self, MicroError> {
        check_name(name)?;
        check_alt(alt)?;
        Ok(MicroAsset {
            name: name.to_string(),
            kind: AssetKind::Static,
            size: CellSize::default(),
            variants: Variants::default(),
            alt: alt.trim().to_string(),
            fallback: Fallback::default(),
            aliases: Vec::new(),
            origin: Origin::default(),
            version: None,
            license: None,
            author: None,
        })
    }

    /// Change the size; the fallbacks set so far must still fit.
    pub fn with_size(mut self, size: CellSize) -> Result<Self, MicroError> {
        for value in [&self.fallback.emoji, &self.fallback.text]
            .into_iter()
            .flatten()
        {
            check_fallback(value, size)?;
        }
        self.size = size;
        Ok(self)
    }

    /// An emoji to show where no image can be drawn.
    pub fn with_emoji(mut self, emoji: &str) -> Result<Self, MicroError> {
        check_fallback(emoji, self.size)?;
        self.fallback.emoji = Some(emoji.to_string());
        Ok(self)
    }

    /// A short text to show where neither an image nor emoji can be.
    pub fn with_text(mut self, text: &str) -> Result<Self, MicroError> {
        check_fallback(text, self.size)?;
        self.fallback.text = Some(text.to_string());
        Ok(self)
    }

    pub fn with_kind(mut self, kind: AssetKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn with_variants(mut self, variants: Variants) -> Self {
        self.variants = variants;
        self
    }

    /// Another name for this asset, resolved within its layer.
    pub fn with_alias(mut self, alias: &str) -> Result<Self, MicroError> {
        check_name(alias)?;
        if alias != self.name && !self.aliases.iter().any(|a| a == alias) {
            self.aliases.push(alias.to_string());
        }
        Ok(self)
    }

    pub fn with_origin(mut self, origin: Origin) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_version(mut self, version: &str) -> Self {
        self.version = Some(version.to_string());
        self
    }

    pub fn with_license(mut self, license: &str) -> Self {
        self.license = Some(license.to_string());
        self
    }

    pub fn with_author(mut self, author: &str) -> Self {
        self.author = Some(author.to_string());
        self
    }

    /// Check what the builder methods cannot check one at a time: an animated
    /// asset needs an animation or frames.
    pub fn validate(&self) -> Result<(), MicroError> {
        if self.kind == AssetKind::Animated
            && self.variants.animation.is_none()
            && self.variants.frames.is_empty()
        {
            return Err(MicroError::Manifest(format!(
                "micro asset {:?} is animated but has no animation or frames",
                self.name
            )));
        }
        Ok(())
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> AssetKind {
        self.kind
    }

    pub fn size(&self) -> CellSize {
        self.size
    }

    /// Its width in cells: what it always measures, however it is drawn.
    pub fn cols(&self) -> usize {
        self.size.cols()
    }

    pub fn variants(&self) -> &Variants {
        &self.variants
    }

    pub fn alt(&self) -> &str {
        &self.alt
    }

    pub fn fallback(&self) -> &Fallback {
        &self.fallback
    }

    pub fn aliases(&self) -> &[String] {
        &self.aliases
    }

    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn license(&self) -> Option<&str> {
        self.license.as_deref()
    }

    pub fn author(&self) -> Option<&str> {
        self.author.as_deref()
    }

    /// Read one of this asset's images from its package, under `limits`.
    /// Errors when the asset has no package or the file changed and now
    /// breaks a limit.
    pub fn read_variant(
        &self,
        image: &ImageRef,
        limits: &crate::Limits,
    ) -> Result<Vec<u8>, MicroError> {
        let location =
            self.origin.location.as_ref().ok_or_else(|| {
                MicroError::Io(format!("micro asset {:?} has no package", self.name))
            })?;
        crate::package::read_file(location, &image.path, limits)
    }
}

fn check_alt(alt: &str) -> Result<(), MicroError> {
    let trimmed = alt.trim();
    if trimmed.is_empty() {
        return Err(MicroError::InvalidAlt(
            "micro asset alt text is required".to_string(),
        ));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(MicroError::InvalidAlt(format!(
            "micro asset alt text {trimmed:?} holds control characters"
        )));
    }
    if trimmed.chars().count() > MAX_ALT_CHARS {
        return Err(MicroError::InvalidAlt(format!(
            "micro asset alt text is longer than {MAX_ALT_CHARS} characters"
        )));
    }
    Ok(())
}

/// A fallback must be non-empty, fit `size`, and hold no whitespace (it would
/// let wrapping split the asset) or control characters.
pub(crate) fn check_fallback(value: &str, size: CellSize) -> Result<(), MicroError> {
    if value.is_empty() {
        return Err(MicroError::InvalidFallback(
            "micro asset fallback is empty".to_string(),
        ));
    }
    if value
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || crate::render::is_sentinel(c))
    {
        return Err(MicroError::InvalidFallback(format!(
            "micro asset fallback {value:?} holds whitespace or control characters"
        )));
    }
    let width = cell_len(value);
    if width == 0 || width > size.cols() {
        return Err(MicroError::InvalidFallback(format!(
            "micro asset fallback {value:?} is {width} cells wide; it must fit {} columns",
            size.cols()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!("2x1".parse::<CellSize>().unwrap(), CellSize::default());
        assert_eq!("1x1".parse::<CellSize>().unwrap().cols(), 1);
        let err = "2x2".parse::<CellSize>().unwrap_err().to_string();
        assert!(err.contains("rows must be 1"), "{err}");
        assert!("0x1".parse::<CellSize>().is_err());
        assert!("5x1".parse::<CellSize>().is_err());
        assert!("x1".parse::<CellSize>().is_err());
        assert!("+2x1".parse::<CellSize>().is_err());
        assert!("99999x1".parse::<CellSize>().is_err());
    }

    #[test]
    fn alt_is_mandatory() {
        assert!(matches!(
            MicroAsset::new("a", "  "),
            Err(MicroError::InvalidAlt(_))
        ));
        assert!(MicroAsset::new("a", "bell\u{7}").is_err());
        assert!(MicroAsset::new("A", "x").is_err());
    }

    #[test]
    fn fallbacks_must_fit() {
        let one = CellSize::new(1, 1).unwrap();
        let asset = MicroAsset::new("a", "alt").unwrap();
        assert!(asset.clone().with_emoji("🚀").is_ok());
        assert!(asset.clone().with_text("OK").is_ok());
        assert!(asset.clone().with_text("OKAY").is_err());
        assert!(asset.clone().with_text("o k").is_err());
        assert!(asset.clone().with_text("").is_err());
        assert!(asset.clone().with_text("\u{1b}[31m").is_err());
        let rocket = asset.clone().with_emoji("🚀").unwrap();
        assert!(rocket.with_size(one).is_err());
        assert!(asset.with_size(one).unwrap().with_text("*").is_ok());
    }

    #[test]
    fn animated_needs_motion() {
        let asset = MicroAsset::new("a", "alt")
            .unwrap()
            .with_kind(AssetKind::Animated);
        assert!(asset.validate().is_err());
    }
}

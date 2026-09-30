//! Decoded, fitted images for drawing, cached (#584).
//!
//! An asset's image is read from its package, decoded under the package
//! [`Limits`], fitted to exactly its cells at the terminal's cell size, and
//! (for an animation) resampled: deduplicated and rate-limited under a
//! memory budget. The result, a [`Prepared`], is kept in an in-memory cache
//! keyed by a hash of the source bytes, the size in cells and the cell size,
//! bounded in bytes (the oldest entries go first). A still image fitted to
//! its cells is also written to an on-disk cache of optimised variants, so
//! the next process decodes a tiny PNG instead of the source.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rich::Segment;
use rich_art::graphics::{
    decode_animation, encode_png, fit_to_cells, AnimationFrame, CellPixels, FrameBudget,
};
use rich_art::image::{self, DynamicImage};

use crate::model::{ImageFormat, ImageRef, MicroAsset};
use crate::package::Limits;

/// FNV-1a, 64-bit: stable across builds, so on-disk keys stay valid.
pub(crate) fn fnv1a(parts: &[&[u8]]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for part in parts {
        for byte in *part {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        // Separate the parts, so ("ab", "c") and ("a", "bc") differ.
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// An asset's image, ready to draw at one size.
#[derive(Debug)]
pub struct Prepared {
    /// Hash of the source bytes, the size in cells and the cell size.
    pub key: u64,
    pub cols: usize,
    pub rows: usize,
    pub cell: CellPixels,
    /// Fitted to exactly `cols × rows` cells; one frame for a still image.
    pub frames: Vec<AnimationFrame>,
    /// Per-frame protocol escapes, made on first use.
    pub(crate) escapes: OnceLock<Vec<String>>,
    /// Per-frame block cells, made on first use.
    pub(crate) blocks: OnceLock<Vec<Vec<Segment>>>,
}

impl Prepared {
    fn new(
        key: u64,
        cols: usize,
        rows: usize,
        cell: CellPixels,
        frames: Vec<AnimationFrame>,
    ) -> Self {
        Prepared {
            key,
            cols,
            rows,
            cell,
            frames,
            escapes: OnceLock::new(),
            blocks: OnceLock::new(),
        }
    }

    /// Whether it has more than one frame.
    pub fn animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// One full cycle.
    pub fn duration(&self) -> Duration {
        self.frames.iter().map(|frame| frame.delay).sum()
    }

    /// The frame that shows `elapsed` into playback (looping), and how long
    /// until the next one.
    pub fn frame_at(&self, elapsed: Duration) -> (usize, Option<Duration>) {
        let total = self.duration();
        if !self.animated() || total.is_zero() {
            return (0, None);
        }
        let mut at = Duration::from_nanos((elapsed.as_nanos() % total.as_nanos()) as u64);
        for (index, frame) in self.frames.iter().enumerate() {
            if at < frame.delay {
                return (index, Some(frame.delay - at));
            }
            at -= frame.delay;
        }
        (0, Some(self.frames[0].delay))
    }

    /// Bytes its frames take.
    fn weight(&self) -> usize {
        self.frames
            .iter()
            .map(|frame| frame.image.as_raw().len())
            .sum()
    }
}

/// Where an asset's images come from and what they are drawn as.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Identity {
    name: String,
    origin: String,
    image: String,
    cols: usize,
    rows: usize,
    cell: CellPixels,
    animate: bool,
}

/// The in-memory cache, bounded in bytes, plus an optional on-disk cache.
#[derive(Debug)]
pub struct ImageCache {
    entries: HashMap<Identity, Arc<Prepared>>,
    order: VecDeque<Identity>,
    used: usize,
    budget: usize,
    disk: Option<PathBuf>,
    /// Identities that failed to load, so they are not retried every frame.
    failed: HashMap<Identity, ()>,
}

impl ImageCache {
    /// Default bound on the decoded frames kept in memory: 32 MiB.
    pub const DEFAULT_BUDGET: usize = 32 << 20;

    pub fn new(budget: usize, disk: Option<PathBuf>) -> ImageCache {
        ImageCache {
            entries: HashMap::new(),
            order: VecDeque::new(),
            used: 0,
            budget,
            disk,
            failed: HashMap::new(),
        }
    }

    /// Bytes the cached frames take.
    pub fn used(&self) -> usize {
        self.used
    }

    /// Entries held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `asset` ready to draw at `cell` pixels per cell, animated when
    /// `animate` and it has an animation. `None` when it has no image, or its
    /// image will not load or decode within `limits`.
    pub fn prepare(
        &mut self,
        asset: &MicroAsset,
        cell: CellPixels,
        animate: bool,
        limits: &Limits,
    ) -> Option<Arc<Prepared>> {
        let (image, animated) = pick(asset, animate)?;
        let identity = Identity {
            name: asset.name().to_string(),
            origin: asset.origin().to_string(),
            image: image.path.clone(),
            cols: asset.cols(),
            rows: asset.size().rows(),
            cell,
            animate: animated,
        };
        if let Some(found) = self.entries.get(&identity) {
            return Some(Arc::clone(found));
        }
        if self.failed.contains_key(&identity) {
            return None;
        }
        let prepared = self.load(asset, image, animated, &identity, limits);
        match prepared {
            Some(prepared) => {
                let prepared = Arc::new(prepared);
                self.insert(identity, Arc::clone(&prepared));
                Some(prepared)
            }
            None => {
                self.failed.insert(identity, ());
                None
            }
        }
    }

    fn insert(&mut self, identity: Identity, prepared: Arc<Prepared>) {
        self.used += prepared.weight();
        self.order.push_back(identity.clone());
        self.entries.insert(identity, prepared);
        while self.used > self.budget && self.order.len() > 1 {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.used -= gone.weight();
            }
        }
    }

    fn load(
        &self,
        asset: &MicroAsset,
        image: &ImageRef,
        animated: bool,
        identity: &Identity,
        limits: &Limits,
    ) -> Option<Prepared> {
        let bytes = asset.read_variant(image, limits).ok()?;
        let (cols, rows) = (identity.cols, identity.rows);
        let key = fnv1a(&[
            &bytes,
            &(cols as u64).to_le_bytes(),
            &(rows as u64).to_le_bytes(),
            &identity.cell.width.to_le_bytes(),
            &identity.cell.height.to_le_bytes(),
            &[u8::from(animated)],
        ]);
        if !animated {
            if let Some(fitted) = self.disk_get(key, cols, rows, identity.cell) {
                return Some(Prepared::new(
                    key,
                    cols,
                    rows,
                    identity.cell,
                    vec![still(fitted)],
                ));
            }
        }
        let budget = FrameBudget {
            max_bytes: usize::try_from(limits.max_decoded_bytes).unwrap_or(usize::MAX),
            max_frames: limits.max_frames as usize,
            ..FrameBudget::default()
        };
        let decoded = decode_animation(&bytes, budget).ok()?;
        let frames = if animated {
            let (width, height) = identity.cell.span(cols, rows);
            rich_art::graphics::resample(&decoded, width, height, budget)
        } else {
            let first = decoded.into_iter().next()?;
            let fitted = fit_to_cells(
                &DynamicImage::ImageRgba8(first.image),
                cols,
                rows,
                identity.cell,
            );
            self.disk_put(key, &fitted);
            vec![still(fitted)]
        };
        if frames.is_empty() {
            return None;
        }
        Some(Prepared::new(key, cols, rows, identity.cell, frames))
    }

    fn disk_path(&self, key: u64) -> Option<PathBuf> {
        Some(self.disk.as_ref()?.join(format!("{key:016x}.png")))
    }

    fn disk_get(
        &self,
        key: u64,
        cols: usize,
        rows: usize,
        cell: CellPixels,
    ) -> Option<image::RgbaImage> {
        let path = self.disk_path(key)?;
        let bytes = std::fs::read(&path).ok()?;
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .ok()?
            .to_rgba8();
        // A file that is not what the key says is ignored.
        (image.dimensions() == cell.span(cols, rows)).then_some(image)
    }

    fn disk_put(&self, key: u64, image: &image::RgbaImage) {
        let Some(path) = self.disk_path(key) else {
            return;
        };
        if path.exists() {
            return;
        }
        let Some(png) = encode_png(image) else {
            return;
        };
        let _ = write_atomically(&path, &png);
    }
}

fn still(image: image::RgbaImage) -> AnimationFrame {
    AnimationFrame {
        image,
        delay: Duration::from_millis(100),
    }
}

/// Write `bytes` to `path` through a temporary file in its directory.
fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    std::fs::create_dir_all(dir)?;
    let temporary = dir.join(format!(
        ".{}.{}.tmp",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("micro"),
        std::process::id()
    ));
    std::fs::write(&temporary, bytes)?;
    std::fs::rename(&temporary, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&temporary);
    })
}

/// The image to draw `asset` from: its animation when `animate` and it has
/// one this build decodes (GIF, APNG), else its still image, else the first
/// image of its animation or frame sequence. And whether it is animated.
fn pick(asset: &MicroAsset, animate: bool) -> Option<(&ImageRef, bool)> {
    let variants = asset.variants();
    let decodable = |image: &&ImageRef| {
        matches!(
            image.info.format,
            ImageFormat::Png | ImageFormat::Apng | ImageFormat::Gif
        )
    };
    if animate {
        if let Some(animation) = variants.animation.as_ref().filter(decodable) {
            if animation.info.frames > 1 {
                return Some((animation, true));
            }
        }
    }
    variants
        .static_image
        .as_ref()
        .filter(decodable)
        .or(variants.animation.as_ref().filter(decodable))
        .or(variants.frames.iter().find(decodable))
        .map(|image| (image, false))
}

/// The user cache directory for micro assets: `RICH_CACHE_DIR/micro`, else
/// `$XDG_CACHE_HOME/rich/micro`, `~/Library/Caches/rich/micro` on macOS,
/// `%LOCALAPPDATA%\rich\cache\micro` on Windows, `~/.cache/rich/micro`
/// elsewhere.
pub fn user_cache_dir() -> Option<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = var("RICH_CACHE_DIR") {
        return Some(PathBuf::from(dir).join("micro"));
    }
    if cfg!(windows) {
        return var("LOCALAPPDATA")
            .map(|d| PathBuf::from(d).join("rich").join("cache").join("micro"));
    }
    if let Some(dir) = var("XDG_CACHE_HOME") {
        return Some(PathBuf::from(dir).join("rich").join("micro"));
    }
    let home = PathBuf::from(var("HOME")?);
    if cfg!(target_os = "macos") {
        Some(
            home.join("Library")
                .join("Caches")
                .join("rich")
                .join("micro"),
        )
    } else {
        Some(home.join(".cache").join("rich").join("micro"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_separates_parts() {
        assert_ne!(fnv1a(&[b"ab", b"c"]), fnv1a(&[b"a", b"bc"]));
        assert_eq!(fnv1a(&[b"x"]), fnv1a(&[b"x"]));
    }

    #[test]
    fn frames_are_time_indexed_and_loop() {
        let frame = |ms| AnimationFrame {
            image: image::RgbaImage::new(1, 1),
            delay: Duration::from_millis(ms),
        };
        let prepared = Prepared::new(1, 2, 1, CellPixels::DEFAULT, vec![frame(100), frame(50)]);
        assert_eq!(
            prepared.frame_at(Duration::ZERO),
            (0, Some(Duration::from_millis(100)))
        );
        assert_eq!(
            prepared.frame_at(Duration::from_millis(120)),
            (1, Some(Duration::from_millis(30)))
        );
        assert_eq!(prepared.frame_at(Duration::from_millis(150)).0, 0);
        let still = Prepared::new(1, 2, 1, CellPixels::DEFAULT, vec![frame(100)]);
        assert_eq!(still.frame_at(Duration::from_secs(9)), (0, None));
    }
}

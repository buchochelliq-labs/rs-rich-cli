//! The built-in library (#581), compiled in: the packs under
//! `crates/rich-micro/builtin/`, drawn for this project by
//! `examples/gen_builtin.rs` and read through the same package reader, under
//! the same [`Limits`](crate::Limits), as any other pack.
//!
//! | Pack | Assets |
//! |---|---|
//! | `status` | `status/success`, `status/warning`, `status/error`, `status/info`, `status/loading` (animated) |
//! | `dev` | `dev/bug`, `dev/branch`, `dev/terminal`, `dev/package` |
//! | `fun` | `fun/heart` (animated), `fun/star`, `fun/cat`, `fun/coffee` (animated) |
//!
//! Every asset has alt text, an emoji and a text fallback, and a licence:
//! the project's own (MIT). There are no third-party logos, and every name
//! is in a namespace, so none is an emoji code (`:micro:fun/star:` is never
//! confused with `:star:`).

use crate::error::MicroError;
use crate::model::Layer;
use crate::package::{load_embedded_pack, Limits, Pack};

/// Every file of the built-in library: `(path under builtin/, bytes)`.
static FILES: &[(&str, &[u8])] = include!("../builtin/files.rs");

/// The packs, in load order.
pub const PACKS: &[&str] = &["status", "dev", "fun"];

/// A built-in file by its path (`status/success/static.png`).
pub(crate) fn file(path: &str) -> Option<&'static [u8]> {
    FILES
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Whether some built-in file is under `path/`.
pub(crate) fn is_dir(path: &str) -> bool {
    let prefix = format!("{path}/");
    FILES.iter().any(|(name, _)| name.starts_with(&prefix))
}

/// Every built-in file's path, sorted.
pub fn files() -> impl Iterator<Item = &'static str> {
    FILES.iter().map(|(name, _)| *name)
}

/// The built-in packs, loaded into `layer` (normally [`Layer::BuiltIn`]).
/// Each is `Ok` with its assets, or the error that rejected it; a package
/// that fails is in its pack's `rejected`.
pub fn packs(layer: Layer) -> Vec<Result<Pack, MicroError>> {
    let limits = Limits::default();
    PACKS
        .iter()
        .map(|pack| load_embedded_pack(pack, layer, &limits))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pack_loads_whole() {
        for pack in packs(Layer::BuiltIn) {
            let pack = pack.unwrap();
            assert!(pack.rejected.is_empty(), "{:?}", pack.rejected);
            assert!(!pack.assets.is_empty());
        }
        assert!(file("status/pack.json").is_some());
        assert!(is_dir("status/success"));
        assert!(!is_dir("status/nothing"));
    }
}

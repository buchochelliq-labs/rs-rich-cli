//! The built-in library (#581): every asset has alt text, a fallback and a
//! licence; no name is an emoji code or a third-party logo; and the
//! committed files are what the generator makes.

#[allow(dead_code)]
#[path = "../examples/gen_builtin.rs"]
mod generator;

use rich_art::image;
use rich_micro::{AssetKind, Layer, MicroRegistry};

#[test]
fn every_builtin_asset_has_alt_text_a_fallback_and_a_licence() {
    let registry = MicroRegistry::builtin();
    let assets: Vec<_> = registry.layer(Layer::BuiltIn).collect();
    assert_eq!(assets.len(), 13);
    for asset in assets {
        let name = asset.name();
        assert!(!asset.alt().trim().is_empty(), "{name}: alt text");
        let fallback = asset.fallback();
        assert!(fallback.emoji.is_some(), "{name}: emoji fallback");
        assert!(fallback.text.is_some(), "{name}: text fallback");
        assert_eq!(asset.license(), Some(generator::LICENSE), "{name}: licence");
        assert!(asset.author().is_some(), "{name}: author");
        assert!(asset.variants().static_image.is_some(), "{name}: image");
        assert_eq!(asset.cols(), 2, "{name}: 2x1 by default");
    }
    for animated in ["status/loading", "fun/heart", "fun/coffee"] {
        assert_eq!(
            registry.require(animated).unwrap().kind(),
            AssetKind::Animated,
            "{animated}"
        );
    }
    for name in [
        "status/success",
        "status/warning",
        "status/error",
        "status/info",
        "status/loading",
    ] {
        assert!(registry.resolve(name).is_some(), "{name}");
    }
}

#[test]
fn no_builtin_name_is_an_emoji_code_or_a_logo() {
    let registry = MicroRegistry::builtin();
    // Trademarked logos stay out of the default library.
    let logos = ["git", "github", "rust", "python", "docker", "npm", "linux"];
    for asset in registry.layer(Layer::BuiltIn) {
        for name in std::iter::once(asset.name()).chain(asset.aliases().iter().map(String::as_str))
        {
            let code = format!(":{name}:");
            assert_eq!(rich::emoji::replace(&code), code, "{name} is an emoji code");
            for segment in name.split('/') {
                assert!(!logos.contains(&segment), "{name} names a logo");
            }
        }
        // Namespaced, so no future emoji code can take the name either.
        assert!(asset.name().contains('/'), "{}", asset.name());
    }
}

#[test]
fn builtin_images_draw_through_the_cache() {
    let registry = MicroRegistry::builtin();
    let mut cache = rich_micro::ImageCache::new(1 << 20, None);
    let limits = rich_micro::Limits::default();
    for asset in registry.layer(Layer::BuiltIn) {
        let prepared = cache
            .prepare(
                asset,
                rich_art::graphics::CellPixels::DEFAULT,
                true,
                &limits,
            )
            .unwrap_or_else(|| panic!("{} does not decode", asset.name()));
        assert_eq!(prepared.animated(), asset.kind() == AssetKind::Animated);
    }
}

/// Decoded pixels, allowing a little difference from floating point.
fn same_pixels(a: &[u8], b: &[u8]) -> bool {
    let decode = |bytes: &[u8]| image::load_from_memory(bytes).unwrap().to_rgba8();
    let (a, b) = (decode(a), decode(b));
    a.dimensions() == b.dimensions()
        && a.pixels()
            .zip(b.pixels())
            .all(|(x, y)| x.0.iter().zip(y.0.iter()).all(|(p, q)| p.abs_diff(*q) <= 3))
}

#[test]
fn committed_files_match_the_generator() {
    let dir = tempfile::tempdir().unwrap();
    let written = generator::generate(dir.path());
    let committed = generator::builtin_dir();
    let mut on_disk: Vec<_> = rich_micro::builtin::files().map(String::from).collect();
    on_disk.push("files.rs".into());
    on_disk.sort();
    let mut made: Vec<String> = written
        .iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();
    made.sort();
    assert_eq!(
        made, on_disk,
        "rerun: cargo run -p rs-rich-micro --example gen_builtin"
    );
    for path in made {
        let fresh = std::fs::read(dir.path().join(&path)).unwrap();
        let kept = std::fs::read(committed.join(&path)).unwrap();
        let same = if path.ends_with(".png") {
            same_pixels(&fresh, &kept)
        } else if path.ends_with(".gif") {
            // Frame count and timing come from the manifest check below;
            // the first frame is the still image.
            same_pixels(&fresh, &kept) || fresh == kept
        } else {
            String::from_utf8_lossy(&fresh).replace("\r\n", "\n")
                == String::from_utf8_lossy(&kept).replace("\r\n", "\n")
        };
        assert!(
            same,
            "{path} differs: rerun cargo run -p rs-rich-micro --example gen_builtin"
        );
    }
}

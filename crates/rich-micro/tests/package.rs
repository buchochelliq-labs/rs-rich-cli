//! Package and pack conformance (#580, #586): valid fixtures, malformed
//! manifests and hostile archives.

mod common;

use common::*;
use rich_micro::package::{load, load_pack, load_package, Loaded};
use rich_micro::{AssetKind, ImageFormat, Layer, Limits, MicroError, PackageLocation};
use serde_json::json;

fn limits() -> Limits {
    Limits::default()
}

#[test]
fn fixtures_load() {
    let root = fixtures().join("builtin");
    let check = load_package(&root.join("check.richmicro"), Layer::BuiltIn, &limits()).unwrap();
    assert_eq!(check.name(), "status/check");
    assert_eq!(check.kind(), AssetKind::Static);
    assert_eq!(check.size().to_string(), "2x1");
    assert_eq!(check.alt(), "green check mark");
    assert_eq!(check.fallback().emoji.as_deref(), Some("✅"));
    assert_eq!(check.aliases(), ["check"]);
    assert_eq!(check.license(), Some("CC0-1.0"));
    assert_eq!(check.origin().layer, Layer::BuiltIn);
    let image = check.variants().static_image.as_ref().unwrap();
    assert_eq!((image.info.width, image.info.height), (16, 16));
    // Read on demand, not kept.
    let bytes = check.read_variant(image, &limits()).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"));

    let spark = load_package(&root.join("spark.richmicro"), Layer::BuiltIn, &limits()).unwrap();
    assert_eq!(spark.kind(), AssetKind::Animated);
    let animation = spark.variants().animation.as_ref().unwrap();
    assert_eq!(
        (animation.info.format, animation.info.frames),
        (ImageFormat::Gif, 3)
    );

    let dot = load_package(&root.join("dot.richmicro"), Layer::BuiltIn, &limits()).unwrap();
    assert_eq!(dot.cols(), 1);
}

#[test]
fn zip_package_and_lazy_read() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ok.richmicro");
    zip(
        &path,
        &[
            Entry::File("manifest.json", manifest_bytes(&manifest("ok"))),
            Entry::Deflated("static.png", png(16, 16)),
        ],
    );
    let asset = load_package(&path, Layer::User, &limits()).unwrap();
    assert!(matches!(
        &asset.origin().location,
        Some(PackageLocation::Archive { prefix, .. }) if prefix.is_empty()
    ));
    let image = asset.variants().static_image.clone().unwrap();
    assert_eq!(asset.read_variant(&image, &limits()).unwrap(), png(16, 16));
}

fn rejects(manifest: serde_json::Value, files: &[(&str, Vec<u8>)]) -> MicroError {
    let dir = tempfile::tempdir().unwrap();
    package_dir(dir.path(), &manifest, files);
    load_package(dir.path(), Layer::User, &limits()).unwrap_err()
}

#[test]
fn malformed_manifests() {
    let image = || vec![("static.png", png(16, 16))];
    let with = |key: &str, value: serde_json::Value| {
        let mut m = manifest("ok");
        m[key] = value;
        m
    };
    let without = |key: &str| {
        let mut m = manifest("ok");
        m.as_object_mut().unwrap().remove(key);
        m
    };
    let cases: Vec<(serde_json::Value, &str)> = vec![
        (
            with("schema_version", json!(2)),
            "unsupported schema_version 2",
        ),
        (
            with("schema_version", json!("1")),
            "unsupported schema_version",
        ),
        (without("schema_version"), "missing schema_version"),
        (without("alt"), "missing \"alt\""),
        (with("alt", json!("   ")), "alt text is required"),
        (with("name", json!("Bad Name")), "invalid micro asset name"),
        (with("size", json!("2x2")), "rows must be 1"),
        (
            with("size", json!({"cols": 2, "rows": 3})),
            "rows must be 1",
        ),
        (with("size", json!("9x1")), "at most 4 columns"),
        (with("size", json!(2)), "size must be"),
        (
            with("fallback", json!({"text": "TOOLONG"})),
            "must fit 2 columns",
        ),
        (with("fallback", json!({"text": "a b"})), "whitespace"),
        (
            with("fallback", json!({"glyph": "x"})),
            "unknown field \"glyph\"",
        ),
        (with("kind", json!("moving")), "unknown kind"),
        (
            with("kind", json!("animated")),
            "animated but has no animation",
        ),
        (with("colour", json!("red")), "unknown field \"colour\""),
        (with("aliases", json!("ok")), "list of strings"),
        (without("static"), "missing \"static\""),
        (
            with("static", json!("../static.png")),
            "leads out of the package",
        ),
        (with("static", json!("/etc/passwd")), "absolute"),
        (with("static", json!("missing.png")), "missing.png"),
    ];
    for (manifest, expected) in cases {
        let error = rejects(manifest.clone(), &image()).to_string();
        assert!(error.contains(expected), "{manifest}: {error}");
    }
    // Not JSON, not an object.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("manifest.json"), "{nope").unwrap();
    let error = load_package(dir.path(), Layer::User, &limits()).unwrap_err();
    assert!(error.to_string().contains("invalid JSON"), "{error}");
    std::fs::write(dir.path().join("manifest.json"), "[1]").unwrap();
    assert!(load_package(dir.path(), Layer::User, &limits()).is_err());
}

#[test]
fn images_are_checked_by_header() {
    let error = rejects(manifest("ok"), &[("static.png", b"not an image".to_vec())]);
    assert!(matches!(error, MicroError::Image(_)), "{error}");
    // A still PNG is not an animation, and an animation is not a static image.
    let mut m = manifest("ok");
    m["animation"] = json!("a.png");
    let error = rejects(m, &[("static.png", png(16, 16)), ("a.png", png(16, 16))]);
    assert!(error.to_string().contains("GIF, APNG or WebP"), "{error}");
    let error = rejects(manifest("ok"), &[("static.png", gif(16, 16, 2))]);
    assert!(error.to_string().contains("is animated"), "{error}");
}

#[test]
fn hostile_dimensions_frames_and_decoded_size() {
    // A decompression bomb by its header: a 20000x20000 PNG of a few bytes.
    let error = rejects(manifest("ok"), &[("static.png", png(20_000, 20_000))]);
    assert!(matches!(error, MicroError::Limit(_)), "{error}");
    assert!(error.to_string().contains("20000x20000"), "{error}");

    let mut m = manifest("ok");
    m["animation"] = json!("a.gif");
    let error = rejects(
        m.clone(),
        &[("static.png", png(16, 16)), ("a.gif", gif(16, 16, 500))],
    );
    assert!(
        error.to_string().contains("more than 240 frames"),
        "{error}"
    );

    // Frames in a sequence count too.
    let mut m2 = manifest("ok");
    let frames: Vec<String> = (0..241).map(|i| format!("f/{i}.png")).collect();
    m2["frames"] = json!(frames);
    let error = rejects(m2, &[("static.png", png(16, 16))]);
    assert!(
        error.to_string().contains("more than 240 frames"),
        "{error}"
    );

    // Decoded bytes: 200 frames of 256x256 RGBA is 52 MB.
    let error = rejects(
        m,
        &[("static.png", png(16, 16)), ("a.gif", gif(256, 256, 200))],
    );
    assert!(error.to_string().contains("bytes decoded"), "{error}");
}

#[test]
fn oversized_files_and_manifests() {
    let dir = tempfile::tempdir().unwrap();
    let tight = Limits {
        max_file_bytes: 1000,
        max_manifest_bytes: 100,
        ..Limits::default()
    };
    let mut big = png(16, 16);
    big.resize(5000, 0);
    package_dir(dir.path(), &manifest("ok"), &[("static.png", big)]);
    let error = load_package(dir.path(), Layer::User, &tight).unwrap_err();
    assert!(matches!(error, MicroError::Limit(_)), "{error}");

    let mut m = manifest("ok");
    m["author"] = json!("x".repeat(200));
    package_dir(dir.path(), &m, &[("static.png", png(16, 16))]);
    let error = load_package(dir.path(), Layer::User, &tight).unwrap_err();
    assert!(
        error.to_string().contains("larger than 100 bytes"),
        "{error}"
    );
}

fn zip_rejects(entries: &[Entry<'_>], limits: &Limits) -> MicroError {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("hostile.richmicro");
    zip(&path, entries);
    load_package(&path, Layer::User, limits).unwrap_err()
}

#[test]
fn zip_slip_absolute_and_links_are_refused() {
    let m = || Entry::File("manifest.json", manifest_bytes(&manifest("ok")));
    let image = || Entry::File("static.png", png(16, 16));
    for (name, why) in [
        ("../evil.png", "leads out"),
        ("a/../../evil.png", "leads out"),
        ("/abs.png", "absolute"),
        ("C:/win.png", "colons"),
        ("a\\b.png", "backslashes"),
    ] {
        let error = zip_rejects(&[m(), image(), Entry::File(name, vec![1])], &limits());
        assert!(
            matches!(error, MicroError::UnsafePath(_)),
            "{name}: {error}"
        );
        assert!(error.to_string().contains(why), "{name}: {error}");
    }
    let error = zip_rejects(
        &[m(), image(), Entry::Symlink("link.png", "/etc/passwd")],
        &limits(),
    );
    assert!(error.to_string().contains("symbolic link"), "{error}");
    // One name twice, once as a folder.
    let error = zip_rejects(
        &[m(), image(), Entry::File("x", vec![1]), Entry::Dir("x/")],
        &limits(),
    );
    assert!(error.to_string().contains("appears twice"), "{error}");
}

#[test]
fn zip_bombs_and_entry_counts() {
    let m = || Entry::File("manifest.json", manifest_bytes(&manifest("ok")));
    // 4 MiB of zeros deflates to a few KiB: refused by its declared size.
    let error = zip_rejects(
        &[
            m(),
            Entry::File("static.png", png(16, 16)),
            Entry::Deflated("pad.bin", vec![0; 4 << 20]),
        ],
        &limits(),
    );
    assert!(matches!(error, MicroError::Limit(_)), "{error}");

    // Many entries.
    let names: Vec<String> = (0..20).map(|i| format!("x{i}")).collect();
    let mut entries = vec![m(), Entry::File("static.png", png(16, 16))];
    entries.extend(names.iter().map(|n| Entry::File(n, vec![0])));
    let few = Limits {
        max_entries: 10,
        ..Limits::default()
    };
    let error = zip_rejects(&entries, &few);
    assert!(
        error.to_string().contains("more than 10 entries"),
        "{error}"
    );

    // The archive itself.
    let small = Limits {
        max_archive_bytes: 100,
        ..Limits::default()
    };
    let error = zip_rejects(&[m(), Entry::File("static.png", png(16, 16))], &small);
    assert!(
        error.to_string().contains("larger than 100 bytes"),
        "{error}"
    );

    // Total uncompressed bytes read from one archive.
    let total = Limits {
        max_total_bytes: 150,
        ..Limits::default()
    };
    let error = zip_rejects(&[m(), Entry::File("static.png", png(16, 16))], &total);
    assert!(matches!(error, MicroError::Limit(_)), "{error}");

    // Not a zip at all.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("junk.richmicro");
    std::fs::write(&path, b"PK\x03\x04 but not really").unwrap();
    assert!(load_package(&path, Layer::User, &limits()).is_err());
}

#[cfg(unix)]
#[test]
fn directory_links_out_of_the_package_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let outside = dir.path().join("outside.png");
    std::fs::write(&outside, png(16, 16)).unwrap();
    let package = dir.path().join("pkg");
    package_dir(&package, &manifest("ok"), &[]);
    std::os::unix::fs::symlink(&outside, package.join("static.png")).unwrap();
    let error = load_package(&package, Layer::User, &limits()).unwrap_err();
    assert!(matches!(error, MicroError::UnsafePath(_)), "{error}");

    // A link that stays inside is fine.
    std::fs::remove_file(package.join("static.png")).unwrap();
    std::fs::write(package.join("real.png"), png(16, 16)).unwrap();
    std::os::unix::fs::symlink(package.join("real.png"), package.join("static.png")).unwrap();
    assert!(load_package(&package, Layer::User, &limits()).is_ok());
}

#[test]
fn packs_from_directories_and_archives() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("status");
    simple_package(&pack.join("ok"), "status/ok");
    // A package archive inside a pack directory.
    zip(
        &pack.join("warn.richmicro"),
        &[
            Entry::File("manifest.json", manifest_bytes(&manifest("status/warn"))),
            Entry::File("static.png", png(16, 16)),
        ],
    );
    // A broken one does not sink the pack.
    let mut bad = manifest("status/bad");
    bad["schema_version"] = json!(9);
    package_dir(&pack.join("bad"), &bad, &[("static.png", png(16, 16))]);
    std::fs::write(
        pack.join("pack.json"),
        json!({
            "schema_version": 1, "name": "status", "version": "1.0.0",
            "packages": ["ok", "warn.richmicro", "bad", "../escape", "missing"],
        })
        .to_string(),
    )
    .unwrap();
    let loaded = load_pack(&pack, Layer::User, &limits()).unwrap();
    assert_eq!(loaded.name, "status");
    let names: Vec<&str> = loaded.assets.iter().map(|a| a.name()).collect();
    assert_eq!(names, ["status/ok", "status/warn"]);
    assert!(loaded
        .assets
        .iter()
        .all(|a| a.origin().pack.as_deref() == Some("status")));
    let rejected: Vec<&str> = loaded.rejected.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(rejected, ["bad", "../escape", "missing"]);
    assert!(matches!(loaded.rejected[1].1, MicroError::UnsafePath(_)));

    // The same pack as one archive, packages as folders.
    let archive = dir.path().join("status.zip");
    zip(
        &archive,
        &[
            Entry::File(
                "pack.json",
                json!({"schema_version": 1, "name": "status", "packages": ["ok"]})
                    .to_string()
                    .into_bytes(),
            ),
            Entry::Dir("ok/"),
            Entry::File("ok/manifest.json", manifest_bytes(&manifest("status/ok"))),
            Entry::File("ok/static.png", png(16, 16)),
        ],
    );
    match load(&archive, Layer::User, &limits()).unwrap() {
        Loaded::Pack(pack) => {
            assert_eq!(pack.assets.len(), 1);
            let asset = &pack.assets[0];
            let image = asset.variants().static_image.clone().unwrap();
            assert_eq!(asset.read_variant(&image, &limits()).unwrap(), png(16, 16));
        }
        Loaded::Package(_) => panic!("expected a pack"),
    }

    // A bad pack index.
    std::fs::write(
        pack.join("pack.json"),
        r#"{"schema_version": 1, "name": "Bad"}"#,
    )
    .unwrap();
    assert!(load_pack(&pack, Layer::User, &limits()).is_err());
}

#[test]
fn neither_package_nor_pack() {
    let dir = tempfile::tempdir().unwrap();
    let error = load(dir.path(), Layer::User, &limits()).unwrap_err();
    assert!(
        error.to_string().contains("no manifest.json or pack.json"),
        "{error}"
    );
}

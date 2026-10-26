//! The official marketplace artifacts built here (tests/support/official.rs):
//! the WASM plugins `textcase` and `wordplay`, the built-in micro packs and
//! the official themes. Their bytes are pinned, so a rebuild is checked to be
//! the file the marketplace serves; textcase 0.1.0 runs against this host and
//! the packs read back as micro's built-in set. `official_packages.rs` checks
//! what the 0.0.23 plugins and themes do.
//!
//! `write_official_artifacts` (ignored) writes them out, laid out as the
//! marketplace repository's `artifacts/`:
//!
//! ```bash
//! RICH_OFFICIAL_ARTIFACTS=../rs-rich-marketplace/artifacts cargo test -p rs-rich-cli \
//!     --locked --test official_artifacts -- --ignored write_official_artifacts
//! ```

#[path = "support/official.rs"]
mod official;

use std::path::PathBuf;
#[cfg(feature = "wasm-plugins")]
use std::process::{Command, Stdio};

fn pins() -> Vec<(String, String, usize)> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/official-artifacts.txt");
    std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| {
            let mut parts = l.split_whitespace();
            let sha = parts.next().unwrap().to_string();
            let size = parts.next().unwrap().parse().unwrap();
            let path = parts.next().unwrap().to_string();
            (path, sha, size)
        })
        .collect()
}

/// A rebuild is byte for byte what was pinned (and listed): a change to a
/// plugin's `.wat`, a built-in pack, a theme, or the `wat` or `zip` crate
/// shows here, and needs a new version in the marketplace, never a new hash
/// for an old one.
#[test]
fn every_official_artifact_rebuilds_to_its_pinned_hash() {
    let built: Vec<(String, String, usize)> = official::all_artifacts()
        .into_iter()
        .map(|(path, bytes)| (path, official::sha256(&bytes), bytes.len()))
        .collect();
    assert_eq!(
        built,
        pins(),
        "rebuild the artifacts and pin them in tests/fixtures/official-artifacts.txt \
         (with a new version) if this change is meant"
    );
    // The builds are deterministic within one run, too.
    assert_eq!(
        official::micro_pack("status"),
        official::micro_pack("status")
    );
}

/// The pinned pack versions are the ones each `pack.json` names.
#[test]
fn the_pack_versions_are_the_pack_json_versions() {
    for (name, version) in official::PACKS {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../rich-micro/builtin")
            .join(name)
            .join("pack.json");
        let pack: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(pack["name"], name);
        assert_eq!(pack["version"], version, "{name}");
    }
}

/// Each archive reads back with micro's own reader, whole, as the built-in
/// pack of the same name.
#[cfg(feature = "art")]
#[test]
fn every_micro_pack_archive_is_the_built_in_pack() {
    use rich_micro::model::Layer;
    use rich_micro::package::{load_pack, Limits};
    let dir = tempfile::tempdir().unwrap();
    let builtin = rich_micro::builtin::packs(Layer::BuiltIn);
    for ((name, version), expected) in official::PACKS.into_iter().zip(builtin) {
        let expected = expected.unwrap();
        let path = dir.path().join(format!("{name}.richmicro"));
        std::fs::write(&path, official::micro_pack(name)).unwrap();
        let pack = load_pack(&path, Layer::User, &Limits::default()).unwrap();
        assert!(pack.rejected.is_empty(), "{name}: {:?}", pack.rejected);
        assert_eq!(pack.name, name);
        assert_eq!(pack.version.as_deref(), Some(version));
        let names = |p: &rich_micro::Pack| {
            let mut n: Vec<String> = p.assets.iter().map(|a| a.name().to_string()).collect();
            n.sort();
            n
        };
        assert_eq!(names(&pack), names(&expected), "{name}");
    }
}

#[cfg(feature = "wasm-plugins")]
fn rich(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .env("HOME", dir)
        .env("USERPROFILE", dir)
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .env("RICH_PLUGIN_DIR", dir.join("plugins"))
        .env("COLUMNS", "60")
        .env_remove("NO_COLOR")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

/// textcase loads in the sandbox with the manifest its listing states, and
/// each capability does what it says.
#[cfg(feature = "wasm-plugins")]
#[test]
fn textcase_loads_and_runs_on_this_host() {
    let dir = tempfile::tempdir().unwrap();
    let wasm = dir.path().join("textcase.wasm");
    std::fs::write(&wasm, official::textcase_wasm()).unwrap();
    let wasm = wasm.to_str().unwrap();
    let text = |out: &std::process::Output| String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = |out: &std::process::Output| String::from_utf8_lossy(&out.stderr).into_owned();

    let out = rich(
        dir.path(),
        &["plugins", "info", "textcase", "--plugin", wasm],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    for expected in [
        "wasm",
        "0.1.0",
        "transform \"upcase\"",
        "transform \"downcase\"",
        "fence-ansi \"loud\"",
    ] {
        assert!(text(&out).contains(expected), "{expected}: {}", text(&out));
    }

    let out = rich(
        dir.path(),
        &[
            "--plugin",
            wasm,
            "--transform",
            "upcase",
            "-p",
            "Héllo, world",
        ],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    assert!(text(&out).contains("HéLLO, WORLD"), "{}", text(&out));
    let out = rich(
        dir.path(),
        &[
            "--plugin",
            wasm,
            "--transform",
            "downcase",
            "-p",
            "QUIET Please",
        ],
    );
    assert!(text(&out).contains("quiet please"), "{}", text(&out));

    std::fs::write(dir.path().join("doc.md"), "```loud\nhello there\n```\n").unwrap();
    let out = rich(
        dir.path(),
        &["--plugin", wasm, "--force-terminal", "-m", "doc.md"],
    );
    assert!(out.status.success(), "{}", stderr(&out));
    let stdout = text(&out);
    assert!(stdout.contains("HELLO THERE"), "{stdout:?}");
    // Bold survives the sanitizer.
    assert!(stdout.contains("\u{1b}[1m"), "{stdout:?}");
}

/// Write every official artifact to `$RICH_OFFICIAL_ARTIFACTS/PATH`, as the
/// marketplace repository's `artifacts/` holds them, and print each pin.
#[test]
#[ignore = "writes the official artifacts for the marketplace repository"]
fn write_official_artifacts() {
    let dir = PathBuf::from(
        std::env::var_os("RICH_OFFICIAL_ARTIFACTS")
            .expect("set RICH_OFFICIAL_ARTIFACTS to the marketplace's artifacts directory"),
    );
    for (path, bytes) in official::all_artifacts() {
        let file = dir.join(&path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, &bytes).unwrap();
        println!("{}  {}  {path}", official::sha256(&bytes), bytes.len());
    }
}

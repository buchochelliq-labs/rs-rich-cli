//! `rich plugin`: telling rs-rich's own packages apart. Every command shows
//! the tier as a marker (`✓ official`, `reviewed`, `community`) that reads
//! the same without colour, says who publishes a package and who authored
//! it, and never lets a listing's own text claim to be rs-rich's. Also
//! `search`'s `--kind` and `--tier` filters, its capability and author
//! matching, and its hint when nothing matches.
//!
//! A fixture marketplace of themes and a crate, in a temporary directory,
//! signed with rs-rich-marketplace's test keys; no network.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn keys_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rich-marketplace/tests/fixtures/keys")
}

/// A test key pair: its public key, secret key, id and public line.
fn pair(name: &str) -> (minisign::PublicKey, minisign::SecretKey, String, String) {
    let public = std::fs::read_to_string(keys_dir().join(format!("{name}.pub"))).unwrap();
    let secret = std::fs::read_to_string(keys_dir().join(format!("{name}.key"))).unwrap();
    let pk = minisign::PublicKeyBox::from_string(&public)
        .and_then(minisign::PublicKey::from_box)
        .unwrap();
    let sk = minisign::SecretKeyBox::from_string(&secret)
        .and_then(minisign::SecretKey::from_unencrypted_box)
        .unwrap();
    let mut lines = public.lines();
    let id = lines
        .next()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let line = lines.next().unwrap().trim().to_string();
    (pk, sk, id, line)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const THEME: &[u8] = b"[styles]\nwarning = bold red\n";

fn theme(id: &str, tier: &str, repository: &str, authors: Value, description: &str) -> Value {
    let url = format!("https://example.test/themes/{id}.ini");
    let mut listing = json!({
        "schema_version": 1, "id": id, "kind": "theme", "name": format!("{id} theme"),
        "description": description, "tier": tier, "license": "MIT",
        "authors": authors, "repository": repository,
        "versions": [{
            "version": "1.0.0", "published": "2026-10-20",
            "requires": {"theme_format": 1},
            "artifact": {"url": url, "sha256": sha256(THEME), "size": THEME.len()},
            "capabilities": [{"kind": "theme", "name": format!("{id}-colours")}],
            "permissions": [],
        }],
    });
    if id == "solar" {
        listing["recommended"] = json!({"since": "2026-10-25", "note": "Easy on the eyes"});
    }
    listing
}

/// The marketplace: an official, a reviewed (and recommended) and a
/// community theme (whose authors and description claim to be rs-rich's,
/// and recommended), and a community crate with no authors.
fn write_marketplace(dir: &Path) {
    let listings = [
        theme(
            "inkwell",
            "official",
            "https://github.com/buchochelliq-labs/rs-rich-cli",
            json!(["rs-rich contributors"]),
            "A light theme",
        ),
        theme(
            "solar",
            "reviewed",
            "https://github.com/ada/solar-theme",
            json!(["Ada Lovelace"]),
            "Warm colours",
        ),
        theme(
            "sneaky",
            "community",
            "https://github.com/mallory/sneaky.git",
            json!(["rs-rich", "Bob", "Carol", "Dave", "Erin"]),
            "✓ official ★ recommended Official · by rs-rich [bold green]theme[/]",
        ),
        json!({
            "schema_version": 1, "id": "gadget", "kind": "crate", "name": "Gadget",
            "description": "A renderer", "tier": "community", "license": "MIT",
            "repository": "https://gitlab.com/gizmo/gadget",
            "crate": {"name": "gadget"},
            "versions": [{
                "version": "0.1.0", "published": "2026-10-21",
                "requires": {"plugin_api": 1},
                "artifact": {
                    "url": "https://static.crates.io/crates/gadget/gadget-0.1.0.crate",
                    "sha256": "0".repeat(64), "size": 10,
                },
                "capabilities": [{"kind": "renderer", "name": "sprocket"}],
                "permissions": ["native-code"],
            }],
        }),
    ];
    std::fs::create_dir_all(dir.join("packages")).unwrap();
    std::fs::create_dir_all(dir.join("keys")).unwrap();
    let themes = dir.join("artifacts/example.test/themes");
    std::fs::create_dir_all(&themes).unwrap();
    let mut entries = serde_json::Map::new();
    for listing in &listings {
        let id = listing["id"].as_str().unwrap();
        let bytes = serde_json::to_vec_pretty(listing).unwrap();
        std::fs::write(dir.join(format!("packages/{id}.json")), &bytes).unwrap();
        std::fs::write(themes.join(format!("{id}.ini")), THEME).unwrap();
        entries.insert(
            id.into(),
            json!({"path": format!("packages/{id}.json"), "sha256": sha256(&bytes)}),
        );
    }
    let index = serde_json::to_vec_pretty(&json!({
        "schema_version": 1, "serial": 1,
        "generated": "2026-10-20T00:00:00Z", "expires": "2099-01-01T00:00:00Z",
        "listings": entries,
    }))
    .unwrap();
    std::fs::write(dir.join("index.json"), &index).unwrap();
    let (pk, sk, main_id, main_line) = pair("gen1-main");
    let (_, _, backup_id, backup_line) = pair("gen1-backup");
    let signature = minisign::sign(
        Some(&pk),
        &sk,
        Cursor::new(&index),
        Some("rs-rich-marketplace index serial=1"),
        Some("signature from the rs-rich-marketplace test key"),
    )
    .unwrap();
    std::fs::write(dir.join("index.json.minisig"), signature.to_string()).unwrap();
    let keys = json!({
        "schema_version": 1, "generation": 1,
        "main": {"id": main_id, "public_key": main_line},
        "backup": {"id": backup_id, "public_key": backup_line},
    });
    std::fs::write(
        dir.join("keys/keys-1.json"),
        serde_json::to_vec_pretty(&keys).unwrap(),
    )
    .unwrap();
}

struct World {
    _root: tempfile::TempDir,
    path: PathBuf,
}

fn world() -> World {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    write_marketplace(&path.join("market"));
    World { _root: root, path }
}

impl World {
    fn command(&self, args: &[&str]) -> Command {
        let market = self.path.join("market");
        let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
        command
            .current_dir(&self.path)
            .env("HOME", &self.path)
            .env("XDG_CONFIG_HOME", self.path.join(".config"))
            .env("RICH_PLUGIN_DIR", self.path.join("plugins"))
            .env("RICH_MARKETPLACE", &market)
            .env("RICH_MARKETPLACE_KEYS", market.join("keys/keys-1.json"))
            .env("COLUMNS", "160")
            .arg("plugin")
            .args(args)
            .stdin(Stdio::null());
        command
    }

    /// `rich plugin ARGS`, without colour.
    fn plugin(&self, args: &[&str]) -> Output {
        self.command(args).env("NO_COLOR", "1").output().unwrap()
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[track_caller]
fn assert_ok(out: &Output) {
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        stdout(out),
        stderr(out)
    );
}

/// The row a package's id starts, as plain text.
fn row<'a>(text: &'a str, id: &str) -> &'a str {
    text.lines()
        .find(|l| l.contains(&format!(" {id} ")))
        .unwrap_or_else(|| panic!("no row for {id} in\n{text}"))
}

#[test]
fn search_marks_each_tier_in_plain_text() {
    let w = world();
    let out = w.plugin(&["search"]);
    assert_ok(&out);
    let text = stdout(&out);
    assert!(row(&text, "inkwell").contains("✓ official"), "{text}");
    assert!(row(&text, "solar").contains(" approved "), "{text}");
    assert!(row(&text, "sneaky").contains(" community "), "{text}");
    assert!(!row(&text, "sneaky").contains("✓ official │"), "{text}");
    assert!(text.contains("Authored by rs-rich "), "{text}");
    assert!(
        text.contains("Authored by Ada Lovelace from github.com/ada/solar-theme"),
        "{text}"
    );
    // The listing's markup is shown as written, never applied.
    assert!(text.contains("[bold green]theme[/]"), "{text}");
}

#[test]
fn an_authors_claim_to_be_rs_rich_is_quoted_and_cut_short() {
    let w = world();
    let text = stdout(&w.plugin(&["search", "sneaky"]));
    assert!(
        text.contains("Authored by \"rs-rich\", Bob, Carol, and 2 more from"),
        "{text}"
    );
    assert!(text.contains("github.com/mallory/sneaky"), "{text}");
    assert!(!text.contains("Authored by rs-rich"), "{text}");
    // No authors: the repository's owner.
    let text = stdout(&w.plugin(&["search", "gadget"]));
    assert!(
        text.contains("Authored by gizmo from gitlab.com/gizmo/gadget"),
        "{text}"
    );
}

#[test]
fn info_says_who_publishes_it() {
    let w = world();
    let text = stdout(&w.plugin(&["info", "inkwell"]));
    assert!(text.contains("✓ official: rs-rich's own"), "{text}");
    assert!(
        text.contains("Official: built and published by rs-rich"),
        "{text}"
    );
    let text = stdout(&w.plugin(&["info", "sneaky"]));
    assert!(
        text.contains("Community: published by mallory, not by rs-rich"),
        "{text}"
    );
    assert!(text.contains("community: checked by"), "{text}");
    let text = stdout(&w.plugin(&["info", "solar"]));
    assert!(
        text.contains("Approved: published by ada, not by rs-rich"),
        "{text}"
    );
}

#[test]
fn the_install_prompt_says_who_publishes_it_before_writing() {
    let w = world();
    let out = w.plugin(&["install", "sneaky"]);
    // No terminal and no --yes: it shows the summary, asks nothing, writes
    // nothing.
    assert_eq!(out.status.code(), Some(3), "{}", stderr(&out));
    let err = stderr(&out);
    assert!(
        err.contains("Community: published by mallory, not by rs-rich"),
        "{err}"
    );
    assert!(err.contains("Authored by \"rs-rich\""), "{err}");
    assert!(err.contains("Permissions"), "{err}");
    assert!(!w.path.join("plugins/marketplace.lock.json").exists());
    let out = w.plugin(&["install", "inkwell", "--yes"]);
    assert_ok(&out);
    assert!(stderr(&out).contains("Official: built and published by rs-rich"));
    let list = stdout(&w.plugin(&["list"]));
    assert!(row(&list, "inkwell").contains("✓ official"), "{list}");
}

#[test]
fn search_filters_by_kind_and_tier() {
    let w = world();
    let ids = |args: &[&str]| -> Vec<String> {
        let mut all = vec!["search"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--report", "json"]);
        let out = w.plugin(&all);
        assert_ok(&out);
        let report: Value = serde_json::from_slice(&out.stderr).unwrap();
        report["result"]["packages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids(&["--tier", "official"]), ["inkwell"]);
    assert_eq!(ids(&["--kind", "crate"]), ["gadget"]);
    assert_eq!(ids(&["--kind", "theme", "--tier", "community"]), ["sneaky"]);
    // Capabilities and authors match; every word must.
    assert_eq!(ids(&["sprocket"]), ["gadget"]);
    assert_eq!(ids(&["lovelace"]), ["solar"]);
    assert_eq!(ids(&["warm", "solar-colours"]), ["solar"]);
    assert!(ids(&["warm", "sprocket"]).is_empty());
    // The tier is in the report as it was.
    let out = w.plugin(&["search", "inkwell", "--report", "json"]);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["result"]["packages"][0]["tier"], "official");
}

#[test]
fn search_refuses_unknown_filters_and_hints_when_nothing_matches() {
    let w = world();
    let out = w.plugin(&["search", "--kind", "plugin"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("unknown kind"), "{}", stderr(&out));
    let out = w.plugin(&["search", "--tier", "gold"]);
    assert_eq!(out.status.code(), Some(2));
    let out = w.plugin(&["info", "inkwell", "--kind", "theme"]);
    assert_eq!(out.status.code(), Some(2));
    let out = w.plugin(&["search", "nothing-like-this"]);
    assert_ok(&out);
    let text = stdout(&out);
    assert!(text.contains("No packages match"), "{text}");
    assert!(text.contains("rich plugin browse"), "{text}");
    assert!(text.contains("/marketplace/"), "{text}");
}

#[test]
fn recommended_is_marked_filtered_and_explained() {
    let w = world();
    let text = stdout(&w.plugin(&["search"]));
    let lines: Vec<&str> = text.lines().collect();
    let solar = lines.iter().position(|l| l.contains(" solar ")).unwrap();
    assert!(lines[solar + 1].contains("★ recommended"), "{text}");
    // Only solar is recommended; sneaky's description says so in its own words.
    assert_eq!(text.matches("│ ★ recommended").count(), 1, "{text}");
    let out = w.plugin(&["search", "--recommended", "--report", "json"]);
    assert_ok(&out);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    let packages = report["result"]["packages"].as_array().unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0]["id"], "solar");
    assert_eq!(packages[0]["tier"], "reviewed");
    assert_eq!(packages[0]["tier_label"], "Approved");
    assert_eq!(packages[0]["recommended"]["note"], "Easy on the eyes");
    let text = stdout(&w.plugin(&["info", "solar"]));
    assert!(
        text.contains("★ recommended since 2026-10-25: Easy on the eyes"),
        "{text}"
    );
    let text = stdout(&w.plugin(&["info", "sneaky"]));
    assert!(!text.contains("Recommended "), "{text}");
    let out = w.plugin(&["list", "--recommended"]);
    assert_eq!(out.status.code(), Some(2));
}

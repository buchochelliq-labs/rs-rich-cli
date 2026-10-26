//! `rich plugin info` and `rich plugin search --sort downloads` with the
//! marketplace's `stats.json` (docs/design/marketplace.md, "Stats"): a
//! fixture marketplace signed with rs-rich-marketplace's test key, two
//! official themes hosted as release assets, a crate, and a `stats.json`
//! beside them as stats.yml writes it. The file is unsigned and
//! display-only: shown when it is there, silently left out when it is
//! missing or malformed, and never consulted by an install.

#[path = "support/official.rs"]
mod official;

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

const RELEASES: &str = "https://github.com/buchochelliq-labs/rs-rich-marketplace/releases/download";

fn keys_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rich-marketplace/tests/fixtures/keys")
}

/// The public half of a test key: `(id, base64 key)`, and the key pair.
fn pair(name: &str) -> (String, String, minisign::PublicKey, minisign::SecretKey) {
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
    (id, lines.next().unwrap().trim().to_string(), pk, sk)
}

fn theme_listing(id: &str) -> Value {
    let (_, version) = official::THEMES.iter().find(|(t, _)| *t == id).unwrap();
    let body = official::theme(id);
    json!({
        "schema_version": 1, "id": id, "kind": "theme", "name": format!("{id} theme"),
        "description": format!("The {id} theme"), "tier": "official", "license": "MIT",
        "repository": "https://github.com/buchochelliq-labs/rs-rich-cli",
        "versions": [{
            "version": version, "published": "2026-10-26", "requires": {"theme_format": 1},
            "artifact": {
                "url": format!("{RELEASES}/{id}-v{version}/{id}-{version}.ini"),
                "sha256": official::sha256(&body), "size": body.len(),
            },
            "capabilities": [{"kind": "theme", "name": id}], "permissions": [],
        }],
    })
}

fn crate_listing() -> Value {
    json!({
        "schema_version": 1, "id": "mermaid", "kind": "crate", "name": "Mermaid diagrams",
        "description": "Mermaid in the terminal", "tier": "official", "license": "MIT",
        "repository": "https://github.com/buchochelliq-labs/rs-rich-cli",
        "crate": {"name": "rs-rich-mermaid"},
        "versions": [{
            "version": "0.0.8", "published": "2026-10-26", "requires": {"plugin_api": 1},
            "artifact": {
                "url": "https://static.crates.io/crates/rs-rich-mermaid/rs-rich-mermaid-0.0.8.crate",
                "sha256": "7f7f2d067169c4b4e7507ae5f80abd78f16cfc86b3d58b6758b85caef7dd0a5b",
                "size": 49703,
            },
            "capabilities": [{"kind": "fence-renderer", "name": "mermaid"}],
            "permissions": ["native-code"],
        }],
    })
}

/// stats.json as stats.yml writes it.
fn stats() -> Value {
    json!({
        "schema_version": 1,
        "generated_at": "2026-10-27T03:41:07Z",
        "repository": "buchochelliq-labs/rs-rich-marketplace",
        "packages": {
            "dusk": {
                "downloads": {"total": 1234, "versions": {"1.0.0": 1234}},
                "reviews": {"url": "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7",
                            "up": 12, "down": 1, "comments": 3},
            },
            "paper": {"downloads": {"total": null, "versions": {"1.0.0": null}}, "reviews": null},
            "mermaid": {"downloads": {"total": 20, "versions": {"0.0.8": 20}}, "reviews": null},
        },
    })
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let market = root.join("market");
        let packages = market.join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        std::fs::create_dir_all(market.join("keys")).unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();

        let (main_id, main_public, pk, sk) = pair("gen1-main");
        let (backup_id, backup_public, _, _) = pair("gen1-backup");
        let keys = json!({
            "schema_version": 1, "generation": 1,
            "main": {"id": main_id, "public_key": main_public},
            "backup": {"id": backup_id, "public_key": backup_public},
        });
        std::fs::write(
            market.join("keys/keys-1.json"),
            serde_json::to_vec_pretty(&keys).unwrap(),
        )
        .unwrap();

        let mut entries = serde_json::Map::new();
        for listing in [
            theme_listing("dusk"),
            theme_listing("paper"),
            crate_listing(),
        ] {
            let id = listing["id"].as_str().unwrap().to_string();
            let bytes = serde_json::to_vec_pretty(&listing).unwrap();
            std::fs::write(packages.join(format!("{id}.json")), &bytes).unwrap();
            entries.insert(
                id.clone(),
                json!({"path": format!("packages/{id}.json"), "sha256": official::sha256(&bytes)}),
            );
            if listing["kind"] == "theme" {
                // The artifacts/HOST/PATH mirror a local marketplace serves.
                let url = listing["versions"][0]["artifact"]["url"].as_str().unwrap();
                let file = market
                    .join("artifacts")
                    .join(url.strip_prefix("https://").unwrap());
                std::fs::create_dir_all(file.parent().unwrap()).unwrap();
                std::fs::write(file, official::theme(&id)).unwrap();
            }
        }
        let index = serde_json::to_vec_pretty(&json!({
            "schema_version": 1, "serial": 1,
            "generated": "2026-10-01T00:00:00Z", "expires": "2099-01-01T00:00:00Z",
            "listings": entries,
        }))
        .unwrap();
        let signature = minisign::sign(
            Some(&pk),
            &sk,
            Cursor::new(&index),
            Some("rs-rich-marketplace index serial=1"),
            Some("signature from the rs-rich-marketplace test key"),
        )
        .unwrap()
        .to_string();
        std::fs::write(market.join("index.json"), &index).unwrap();
        std::fs::write(market.join("index.json.minisig"), signature).unwrap();
        World { _dir: dir, root }
    }

    fn market(&self) -> PathBuf {
        self.root.join("market")
    }

    fn write_stats(&self, bytes: &[u8]) {
        std::fs::write(self.market().join("stats.json"), bytes).unwrap();
    }

    fn plugin(&self, args: &[&str]) -> Output {
        let market = self.market();
        let keys = market.join("keys/keys-1.json");
        let home = self.root.join("home");
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .current_dir(&self.root)
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("RICH_PLUGIN_DIR", self.root.join("plugins"))
            .env_remove("RICH_MARKETPLACE")
            .env_remove("RICH_MARKETPLACE_KEYS")
            .env("COLUMNS", "160")
            .env("NO_COLOR", "1")
            .arg("plugin")
            .args(args)
            .args([
                "--marketplace",
                path(&market),
                "--marketplace-keys",
                path(&keys),
            ])
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    /// The `result` of a `--report json` run.
    fn report(&self, args: &[&str]) -> Value {
        let mut all = args.to_vec();
        all.extend(["--report", "json"]);
        let out = self.plugin(&all);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let envelope: Value = serde_json::from_slice(&out.stderr).unwrap();
        envelope["result"].clone()
    }
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn info_shows_downloads_rating_and_reviews() {
    let w = World::new();
    w.write_stats(&serde_json::to_vec(&stats()).unwrap());
    let out = w.plugin(&["info", "dusk"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    for expected in [
        "Downloads     1,234 (1.0.0: 1,234)",
        "Rating        👍 12  👎 1  (3 comments)",
        "Reviews       https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7",
        "Stats         as of 2026-10-27; not signed, shown only",
    ] {
        assert!(shown.contains(expected), "{expected:?} in\n{shown}");
    }
    assert_eq!(text(&out.stderr), "");

    let result = w.report(&["info", "dusk"]);
    assert_eq!(
        result["stats"],
        json!({
            "source": path(&w.market().join("stats.json")),
            "generated_at": "2026-10-27T03:41:07Z",
            "downloads": {"total": 1234, "versions": {"1.0.0": 1234}},
            "reviews": {"url": "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/7",
                        "up": 12, "down": 1, "comments": 3},
        })
    );
    // Not counted: no downloads row, and no rating without a discussion.
    let out = w.plugin(&["info", "paper"]);
    let shown = text(&out.stdout);
    assert!(!shown.contains("Downloads"), "{shown}");
    assert!(!shown.contains("Rating"), "{shown}");
    assert_eq!(
        w.report(&["info", "paper"])["stats"]["downloads"]["total"],
        Value::Null
    );
}

#[test]
fn without_stats_info_says_nothing_about_them() {
    let w = World::new();
    for stats in [
        None,
        Some(&b"not json"[..]),
        Some(br#"{"schema_version": 2}"#),
    ] {
        if let Some(bytes) = stats {
            w.write_stats(bytes);
        }
        let out = w.plugin(&["info", "dusk"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        let shown = text(&out.stdout);
        assert!(
            !shown.contains("Downloads") && !shown.contains("Stats"),
            "{shown}"
        );
        assert_eq!(text(&out.stderr), "");
        assert_eq!(w.report(&["info", "dusk"])["stats"], Value::Null);
    }
}

#[test]
fn a_review_link_is_only_a_github_one() {
    let w = World::new();
    let mut stats = stats();
    stats["packages"]["dusk"]["reviews"]["url"] = json!("https://example.test/phish");
    stats["packages"]["dusk"]["downloads"]["versions"]["1.0.0"] = json!("lots");
    w.write_stats(&serde_json::to_vec(&stats).unwrap());
    let result = w.report(&["info", "dusk"]);
    assert_eq!(result["stats"]["reviews"], Value::Null);
    assert_eq!(
        result["stats"]["downloads"]["versions"]["1.0.0"],
        Value::Null
    );
    assert!(!text(&w.plugin(&["info", "dusk"]).stdout).contains("example.test"));
}

#[test]
fn a_review_link_must_stay_in_the_marketplaces_discussions() {
    let w = World::new();
    let base = "https://github.com/buchochelliq-labs/rs-rich-marketplace/discussions/";
    for hostile in [
        format!("{base}../../../evil/phish"),
        format!("{base}%2e%2e/%2e%2e/%2e%2e/evil/phish"),
        format!("{base}.%2E/%2E./evil/phish"),
        format!("{base}7?redirect=https://evil.test/phish"),
        format!("{base}7#https://evil.test/phish"),
        format!("{base}7\\..\\..\\evil"),
        "https://github.com/evil/rs-rich-marketplace/discussions/7".to_string(),
        "https://github.com/buchochelliq-labs/rs-rich-marketplace/issues/7".to_string(),
    ] {
        let mut stats = stats();
        stats["packages"]["dusk"]["reviews"]["url"] = json!(hostile);
        w.write_stats(&serde_json::to_vec(&stats).unwrap());
        let result = w.report(&["info", "dusk"]);
        assert_eq!(result["stats"]["reviews"], Value::Null, "{hostile}");
        let shown = text(&w.plugin(&["info", "dusk"]).stdout);
        assert!(!shown.contains("Reviews "), "{hostile}: {shown}");
        assert!(
            !shown.contains("evil") && !shown.contains("issues"),
            "{shown}"
        );
    }
}

#[test]
fn search_sorts_by_downloads() {
    let w = World::new();
    w.write_stats(&serde_json::to_vec(&stats()).unwrap());
    let out = w.plugin(&["search", "--sort", "downloads"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    assert!(shown.contains("Downloads"), "{shown}");
    let at = |id: &str| shown.find(&format!("│ {id} ")).unwrap();
    // 1,234, then 20, then not counted.
    assert!(
        at("dusk") < at("mermaid") && at("mermaid") < at("paper"),
        "{shown}"
    );

    let result = w.report(&["search", "--sort", "downloads"]);
    let ids: Vec<&str> = result["packages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["dusk", "mermaid", "paper"]);
    assert_eq!(result["packages"][0]["stats"]["downloads"]["total"], 1234);

    // The default order is by id, with no stats read or shown.
    let result = w.report(&["search"]);
    assert!(result["packages"][0].get("stats").is_none());
    assert!(!text(&w.plugin(&["search"]).stdout).contains("Downloads"));
}

#[test]
fn search_by_downloads_without_stats_says_so_and_sorts_by_id() {
    let w = World::new();
    let out = w.plugin(&["search", "--sort", "downloads"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("stats.json is not available; sorted by id"),
        "{}",
        text(&out.stderr)
    );
    let shown = text(&out.stdout);
    assert!(shown.find("│ dusk ").unwrap() < shown.find("│ mermaid ").unwrap());
}

#[test]
fn sort_is_checked() {
    let w = World::new();
    let out = w.plugin(&["search", "--sort", "stars"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    assert!(text(&out.stderr).contains("unknown --sort"));
    let out = w.plugin(&["info", "dusk", "--sort", "downloads"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
}

/// Stats never decide an install: a hosted theme installs from its
/// release URL the same with or without them.
#[test]
fn an_install_ignores_stats() {
    let w = World::new();
    w.write_stats(&serde_json::to_vec(&stats()).unwrap());
    let out = w.plugin(&["install", "paper", "--yes"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!text(&out.stdout).contains("Downloads"));
    assert!(w.root.join("plugins/paper/paper-1.0.0.ini").is_file());
    let lock: Value = serde_json::from_slice(
        &std::fs::read(w.root.join("plugins/marketplace.lock.json")).unwrap(),
    )
    .unwrap();
    assert!(
        lock.to_string()
            .contains(&format!("{RELEASES}/paper-v1.0.0/paper-1.0.0.ini")),
        "{lock}"
    );
}

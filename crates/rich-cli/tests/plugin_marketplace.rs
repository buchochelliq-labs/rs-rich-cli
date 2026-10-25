//! `rich plugin`: install, update, remove, enable, disable, verify and
//! publish against a fixture marketplace in a temporary directory, signed
//! with rs-rich-marketplace's test keys. No network: a local marketplace
//! serves artifacts from its `artifacts/HOST/PATH` mirror only.
//!
//! The WASM cases need the `wasm-plugins` feature (a default build has it);
//! the rest run in every build.
//!
//! `write_test_marketplace` (ignored) writes the same marketplace, with the
//! official plugins listed at this workspace's versions, for the CI job that
//! installs from it with the built binary:
//!
//! ```bash
//! RICH_TEST_MARKETPLACE=/tmp/market cargo test -p rs-rich-cli \
//!     --test plugin_marketplace -- --ignored write_test_marketplace
//! ```

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::{json, Value};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn keys_dir() -> PathBuf {
    manifest_dir().join("../rich-marketplace/tests/fixtures/keys")
}

/// A test key pair from rs-rich-marketplace's fixtures.
struct Pair {
    pk: minisign::PublicKey,
    sk: minisign::SecretKey,
    id: String,
    public: String,
}

fn pair(name: &str) -> Pair {
    let public = std::fs::read_to_string(keys_dir().join(format!("{name}.pub"))).unwrap();
    let secret = std::fs::read_to_string(keys_dir().join(format!("{name}.key"))).unwrap();
    let pk = minisign::PublicKeyBox::from_string(&public)
        .and_then(minisign::PublicKey::from_box)
        .unwrap();
    let sk = minisign::SecretKeyBox::from_string(&secret)
        .and_then(minisign::SecretKey::from_unencrypted_box)
        .unwrap();
    // "untrusted comment: minisign public key: ID", then the key.
    let mut lines = public.lines();
    let id = lines
        .next()
        .unwrap()
        .rsplit(' ')
        .next()
        .unwrap()
        .to_string();
    let public = lines.next().unwrap().trim().to_string();
    Pair { pk, sk, id, public }
}

impl Pair {
    fn sign(&self, data: &[u8], comment: &str) -> String {
        minisign::sign(
            Some(&self.pk),
            &self.sk,
            Cursor::new(data),
            Some(comment),
            Some("signature from the rs-rich-marketplace test key"),
        )
        .unwrap()
        .to_string()
    }
}

/// `keys-1.json` for the test generation-1 pair.
fn keys_json() -> Vec<u8> {
    let (main, backup) = (pair("gen1-main"), pair("gen1-backup"));
    serde_json::to_vec_pretty(&json!({
        "schema_version": 1,
        "generation": 1,
        "main": {"id": main.id, "public_key": main.public},
        "backup": {"id": backup.id, "public_key": backup.public},
    }))
    .unwrap()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

const SHOUT_WAT: &str = include_str!("../../rich-plugin-api/examples/wasm/shout.wat");
const THEME: &[u8] = b"[styles]\nwarning = bold red\ninfo = dim cyan\n";
const BASE: &str = "https://example.test/releases";

/// The shout module with its manifest's version replaced (same length).
fn shout(version: &str) -> Vec<u8> {
    assert_eq!(version.len(), 5);
    wat::parse_str(SHOUT_WAT.replace("version 0.1.0", &format!("version {version}"))).unwrap()
}

/// The fixture marketplace, written to a directory.
struct Market {
    dir: PathBuf,
    /// `packages/ID.json`, as written.
    listings: Vec<(String, Value)>,
    serial: u64,
}

fn version(number: &str, requires: Value, file: &str, body: &[u8], capabilities: Value) -> Value {
    json!({
        "version": number,
        "published": "2026-10-15",
        "requires": requires,
        "artifact": {"url": format!("{BASE}/{file}"), "sha256": sha256(body), "size": body.len()},
        "capabilities": capabilities,
        "permissions": [],
    })
}

fn listing(id: &str, kind: &str, tier: &str, repository: &str, versions: Vec<Value>) -> Value {
    json!({
        "schema_version": 1, "id": id, "kind": kind, "name": id,
        "description": format!("The {id} fixture."), "tier": tier, "license": "MIT",
        "repository": repository, "versions": versions,
    })
}

fn crate_listing(id: &str, name: &str, version: &str, features: &[&str]) -> Value {
    let mut value = listing(
        id,
        "crate",
        "official",
        "https://github.com/buchochelliq-labs/rs-rich-cli",
        vec![json!({
            "version": version, "published": "2026-10-20",
            "requires": {"plugin_api": rich_ext::plugin::PLUGIN_API_VERSION},
            "artifact": {
                "url": format!("https://static.crates.io/crates/{name}/{name}-{version}.crate"),
                "sha256": "6ca5b3f974d2ead0191b462b81164c750a3e03c6629f9a5783fb819d43a0e9ac",
                "size": 98304
            },
            "capabilities": [{"kind": "fence-renderer", "name": id}],
            "permissions": ["native-code"],
        })],
    );
    value["crate"] = json!({"name": name, "features": features});
    value
}

/// A workspace crate's version, from its manifest.
fn workspace_version(dir: &str) -> String {
    let text =
        std::fs::read_to_string(manifest_dir().join("..").join(dir).join("Cargo.toml")).unwrap();
    let manifest: toml::Value = toml::from_str(&text).unwrap();
    manifest["package"]["version"].as_str().unwrap().to_string()
}

impl Market {
    /// `shout` (0.1.0 good, 0.0.9 yanked), `future` (a WASM plugin for ABI
    /// 2 only), `solarized` (a theme), `broken` (a theme whose file does not
    /// match its hash), and the official crates, listed only.
    fn write(dir: &Path) -> Market {
        let artifacts = dir.join("artifacts/example.test/releases");
        std::fs::create_dir_all(&artifacts).unwrap();
        let put = |file: &str, body: &[u8]| std::fs::write(artifacts.join(file), body).unwrap();
        let caps = json!([{"kind": "transform", "name": "upper"}, {"kind": "fence-markup", "name": "shout"}]);
        let wasm = |number: &str| {
            let body = shout(number);
            put(&format!("shout-{number}.wasm"), &body);
            let mut v = version(
                number,
                json!({"abi": "1.0"}),
                &format!("shout-{number}.wasm"),
                &body,
                caps.clone(),
            );
            v["source_revision"] = json!("e26c737c5e9741d94abfea61e9d2065c6b59f2e5");
            v
        };
        let mut old = wasm("0.0.9");
        old["yanked"] = json!(true);
        old["yank_reason"] = json!("Drops combining marks; use 0.1.0.");
        let shout_listing = listing(
            "shout",
            "wasm-plugin",
            "community",
            "https://github.com/example/rich-shout",
            vec![wasm("0.1.0"), old],
        );

        let future_body = shout("0.1.0");
        put("future-3.0.0.wasm", &future_body);
        let mut future = version(
            "3.0.0",
            json!({"abi": "2.0"}),
            "future-3.0.0.wasm",
            &future_body,
            caps.clone(),
        );
        future["source_revision"] = json!("e26c737c5e9741d94abfea61e9d2065c6b59f2e5");
        let future = listing(
            "future",
            "wasm-plugin",
            "community",
            "https://github.com/example/rich-future",
            vec![future],
        );

        put("solarized.ini", THEME);
        let theme_caps = |id: &str| json!([{"kind": "theme", "name": id}]);
        let solarized = listing(
            "solarized",
            "theme",
            "reviewed",
            "https://github.com/example/rich-solarized",
            vec![version(
                "1.2.0",
                json!({"theme_format": 1}),
                "solarized.ini",
                THEME,
                theme_caps("solarized"),
            )],
        );
        // Listed with the hash of THEME, but the file served differs.
        let other = b"[styles]\nwarning = dim  red\ninfo = dim cyan\n";
        assert_eq!(other.len(), THEME.len());
        put("broken.ini", other);
        let broken = listing(
            "broken",
            "theme",
            "community",
            "https://github.com/example/rich-broken",
            vec![version(
                "1.0.0",
                json!({"theme_format": 1}),
                "broken.ini",
                THEME,
                theme_caps("broken"),
            )],
        );

        let listings = vec![
            ("shout".to_string(), shout_listing),
            ("future".to_string(), future),
            ("solarized".to_string(), solarized),
            ("broken".to_string(), broken),
            (
                "mermaid".to_string(),
                crate_listing(
                    "mermaid",
                    "rs-rich-mermaid",
                    &workspace_version("rich-mermaid"),
                    &[],
                ),
            ),
            (
                "dot".to_string(),
                crate_listing(
                    "dot",
                    "rs-rich-diagram",
                    &workspace_version("rich-diagram"),
                    &["plugin"],
                ),
            ),
            (
                "lumis".to_string(),
                crate_listing(
                    "lumis",
                    "rs-rich-lumis",
                    &workspace_version("rich-lumis"),
                    &[],
                ),
            ),
        ];
        std::fs::create_dir_all(dir.join("keys")).unwrap();
        std::fs::write(dir.join("keys/keys-1.json"), keys_json()).unwrap();
        let mut market = Market {
            dir: dir.to_path_buf(),
            listings,
            serial: 0,
        };
        market.publish(1);
        market
    }

    /// Write every listing and a fresh index at `serial`, signed by the
    /// test main key.
    fn publish(&mut self, serial: u64) {
        let packages = self.dir.join("packages");
        std::fs::create_dir_all(&packages).unwrap();
        let mut entries = serde_json::Map::new();
        for (id, value) in &self.listings {
            let bytes = serde_json::to_vec_pretty(value).unwrap();
            std::fs::write(packages.join(format!("{id}.json")), &bytes).unwrap();
            entries.insert(
                id.clone(),
                json!({"path": format!("packages/{id}.json"), "sha256": sha256(&bytes)}),
            );
        }
        let index = serde_json::to_vec_pretty(&json!({
            "schema_version": 1,
            "serial": serial,
            "generated": "2026-10-01T00:00:00Z",
            "expires": "2099-01-01T00:00:00Z",
            "listings": entries,
        }))
        .unwrap();
        let signature = pair("gen1-main").sign(
            &index,
            &format!("rs-rich-marketplace index serial={serial}"),
        );
        std::fs::write(self.dir.join("index.json"), &index).unwrap();
        std::fs::write(self.dir.join("index.json.minisig"), signature).unwrap();
        self.serial = serial;
    }

    fn listing_mut(&mut self, id: &str) -> &mut Value {
        &mut self.listings.iter_mut().find(|(i, _)| i == id).unwrap().1
    }
}

/// A fixture marketplace, a home and an install directory.
struct World {
    _root: tempfile::TempDir,
    root: PathBuf,
    market: Market,
    plugins: PathBuf,
    home: PathBuf,
}

fn world() -> World {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let market = Market::write(&path.join("market"));
    let home = path.join("home");
    std::fs::create_dir_all(&home).unwrap();
    World {
        _root: root,
        plugins: path.join("plugins"),
        market,
        home,
        root: path,
    }
}

impl World {
    fn keys(&self) -> PathBuf {
        self.market.dir.join("keys/keys-1.json")
    }

    /// `rich plugin ARGS` against the fixture.
    fn plugin(&self, args: &[&str]) -> Output {
        let market = self.market.dir.to_str().unwrap().to_string();
        let keys = self.keys().to_str().unwrap().to_string();
        let mut all = vec!["plugin"];
        all.extend_from_slice(args);
        all.extend_from_slice(&["--marketplace", &market, "--marketplace-keys", &keys]);
        self.rich(&all)
    }

    /// `rich ARGS`, with the install directory in RICH_PLUGIN_DIR.
    fn rich(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .current_dir(&self.root)
            .env("HOME", &self.home)
            .env("USERPROFILE", &self.home)
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("RICH_PLUGIN_DIR", &self.plugins)
            .env_remove("RICH_MARKETPLACE")
            .env_remove("RICH_MARKETPLACE_KEYS")
            .env("COLUMNS", "120")
            .env("NO_COLOR", "1")
            .args(args)
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn lockfile(&self) -> Value {
        match std::fs::read(self.plugins.join("marketplace.lock.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap(),
            Err(_) => json!({"packages": {}}),
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[track_caller]
fn assert_code(out: &Output, code: i32) {
    assert_eq!(
        out.status.code(),
        Some(code),
        "stdout: {}\nstderr: {}",
        text(&out.stdout),
        text(&out.stderr)
    );
}

#[test]
fn search_and_info_show_tier_versions_and_the_crate_line() {
    let w = world();
    let out = w.plugin(&["search"]);
    assert_code(&out, 0);
    let stdout = text(&out.stdout);
    for id in ["shout", "solarized", "mermaid", "dot", "lumis"] {
        assert!(stdout.contains(id), "{id}: {stdout}");
    }
    let out = w.plugin(&["search", "SOLAR"]);
    assert!(text(&out.stdout).contains("solarized"));
    assert!(!text(&out.stdout).contains("shout"));

    let out = w.plugin(&["info", "shout"]);
    assert_code(&out, 0);
    let stdout = text(&out.stdout);
    for expected in [
        "community",
        "0.1.0",
        "0.0.9",
        "Drops combining marks",
        "fence-markup shout",
    ] {
        assert!(stdout.contains(expected), "{expected}: {stdout}");
    }
    let out = w.plugin(&["info", "dot", "--report", "json"]);
    assert_code(&out, 0);
    assert!(out.stdout.is_empty());
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["result"]["kind"], "crate");
    assert_eq!(report["result"]["crate"]["features"][0], "plugin");
    assert_eq!(report["result"]["versions"][0]["compatible"], true);

    let out = w.plugin(&["info", "nope"]);
    assert_code(&out, 2);
}

#[test]
fn a_crate_is_refused_with_its_cargo_add_line() {
    let w = world();
    let out = w.plugin(&["install", "mermaid", "--yes"]);
    assert_code(&out, 2);
    let version = workspace_version("rich-mermaid");
    assert!(
        text(&out.stderr).contains(&format!("cargo add rs-rich-mermaid@={version}")),
        "{}",
        text(&out.stderr)
    );
    assert!(!w.plugins.join("marketplace.lock.json").exists());
}

#[test]
fn install_list_disable_enable_and_remove_a_theme() {
    let w = world();
    // Without a terminal and without --yes, nothing is asked or written.
    let out = w.plugin(&["install", "solarized"]);
    assert_code(&out, 3);
    assert!(text(&out.stderr).contains("--yes"), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("reviewed"),
        "the summary: {}",
        text(&out.stderr)
    );
    assert!(!w.plugins.join("marketplace.lock.json").exists());

    let out = w.plugin(&["install", "solarized", "--yes"]);
    assert_code(&out, 0);
    assert!(text(&out.stdout).contains("Installed solarized 1.2.0"));
    let lock = w.lockfile();
    let entry = &lock["packages"]["solarized"];
    assert_eq!(entry["version"], "1.2.0");
    assert_eq!(entry["sha256"], sha256(THEME));
    assert_eq!(entry["index_serial"], 1);
    assert_eq!(
        std::fs::read(w.plugins.join("solarized/solarized-1.2.0.ini")).unwrap(),
        THEME
    );
    // A second install is a no-op.
    let out = w.plugin(&["install", "solarized", "--yes"]);
    assert_code(&out, 0);
    assert!(text(&out.stdout).contains("already installed"));

    let out = w.plugin(&["list"]);
    assert_code(&out, 0);
    let stdout = text(&out.stdout);
    assert!(stdout.contains("solarized") && stdout.contains("yes") && stdout.contains("ok"));

    assert_code(&w.plugin(&["disable", "solarized"]), 0);
    let out = w.plugin(&["list", "--report", "json"]);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["result"]["packages"][0]["enabled"], false);
    assert_code(&w.plugin(&["enable", "solarized"]), 0);
    let out = w.plugin(&["list", "--report", "json"]);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["result"]["packages"][0]["enabled"], true);
    assert_code(&w.plugin(&["disable", "shout"]), 2);

    let out = w.plugin(&["remove", "solarized"]);
    assert_code(&out, 0);
    assert!(w.lockfile()["packages"].as_object().unwrap().is_empty());
    assert!(!w.plugins.join("solarized").exists());
    assert_code(&w.plugin(&["remove", "solarized"]), 2);
}

#[test]
fn a_bad_hash_is_refused_and_nothing_is_written() {
    let w = world();
    let out = w.plugin(&["install", "broken", "--yes", "--report", "json"]);
    assert_code(&out, 4);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["code"], "data");
    assert!(
        report["error"]["message"]
            .as_str()
            .unwrap()
            .contains("SHA-256"),
        "{report}"
    );
    assert!(!w.plugins.join("broken").exists());
    assert!(!w.plugins.join("marketplace.lock.json").exists());
}

#[test]
fn a_yanked_version_and_an_abi_this_host_cannot_load_are_refused() {
    let w = world();
    let out = w.plugin(&["install", "shout@0.0.9", "--yes"]);
    assert_code(&out, 4);
    assert!(
        text(&out.stderr).contains("yanked: Drops combining marks"),
        "{}",
        text(&out.stderr)
    );
    let out = w.plugin(&["install", "future", "--yes"]);
    assert_code(&out, 4);
    assert!(
        text(&out.stderr).contains("need a newer rs-rich"),
        "{}",
        text(&out.stderr)
    );
    let out = w.plugin(&["install", "shout@9.9.9", "--yes"]);
    assert_code(&out, 2);
    assert!(!w.plugins.join("marketplace.lock.json").exists());
}

#[test]
fn a_bad_signature_and_a_rollback_are_refused() {
    let mut w = world();
    w.market.publish(5);
    assert_code(&w.plugin(&["search"]), 0);
    // An older index, validly signed, after serial 5 was seen.
    w.market.publish(4);
    let out = w.plugin(&["search"]);
    assert_code(&out, 4);
    assert!(
        text(&out.stderr).contains("rollback"),
        "{}",
        text(&out.stderr)
    );
    // A signature by another key.
    let index = std::fs::read(w.market.dir.join("index.json")).unwrap();
    let forged = pair("stranger").sign(&index, "rs-rich-marketplace index serial=4");
    std::fs::write(w.market.dir.join("index.json.minisig"), forged).unwrap();
    let out = w.plugin(&["search"]);
    assert_code(&out, 4);
    assert!(
        text(&out.stderr).contains("signed by key"),
        "{}",
        text(&out.stderr)
    );
    // No trust root at all: the official keys are not compiled in yet.
    let out = w.rich(&[
        "plugin",
        "search",
        "--marketplace",
        w.market.dir.to_str().unwrap(),
    ]);
    assert_code(&out, 2);
    assert!(text(&out.stderr).contains("--marketplace-keys"));
}

#[test]
fn verify_reports_a_changed_file_and_a_version_yanked_since() {
    let mut w = world();
    assert_code(&w.plugin(&["install", "solarized", "--yes"]), 0);
    let out = w.plugin(&["verify"]);
    assert_code(&out, 0);
    assert!(text(&out.stdout).contains("Checked 1 installed package against index serial 1"));

    // Yanked since: reported with its reason, still exit 0, and `list`
    // shows it with no network.
    let solarized = w.market.listing_mut("solarized");
    solarized["versions"][0]["yanked"] = json!(true);
    solarized["versions"][0]["yank_reason"] = json!("Wrong colours; use 1.2.1.");
    w.market.publish(2);
    let out = w.plugin(&["verify", "--report", "json"]);
    assert_code(&out, 0);
    let report: Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(
        report["result"]["yanked"][0]["reason"],
        "Wrong colours; use 1.2.1."
    );
    assert_eq!(report["result"]["index_serial"], 2);
    let out = w.plugin(&["list"]);
    assert!(
        text(&out.stdout).contains("Wrong colours"),
        "{}",
        text(&out.stdout)
    );

    // A file changed on disk.
    std::fs::write(
        w.plugins.join("solarized/solarized-1.2.0.ini"),
        b"[styles]\n",
    )
    .unwrap();
    let out = w.plugin(&["verify"]);
    assert_code(&out, 4);
    assert!(
        text(&out.stdout).contains("solarized"),
        "{}",
        text(&out.stdout)
    );
    let out = w.plugin(&["list"]);
    assert!(text(&out.stdout).contains("changed"));
}

#[test]
fn update_moves_off_a_yanked_version() {
    let mut w = world();
    assert_code(&w.plugin(&["install", "solarized", "--yes"]), 0);
    let out = w.plugin(&["update", "--yes"]);
    assert_code(&out, 0);
    assert!(text(&out.stdout).contains("up to date"));

    let newer = b"[styles]\nwarning = bold red\ninfo = cyan\n".to_vec();
    std::fs::write(
        w.market
            .dir
            .join("artifacts/example.test/releases/solarized-1.3.0.ini"),
        &newer,
    )
    .unwrap();
    let added = version(
        "1.3.0",
        json!({"theme_format": 1}),
        "solarized-1.3.0.ini",
        &newer,
        json!([{"kind": "theme", "name": "solarized"}]),
    );
    let listing = w.market.listing_mut("solarized");
    listing["versions"].as_array_mut().unwrap().insert(0, added);
    w.market.publish(2);
    // Declined without a terminal: exit 3, and nothing changes.
    assert_code(&w.plugin(&["update"]), 3);
    assert_eq!(w.lockfile()["packages"]["solarized"]["version"], "1.2.0");
    let out = w.plugin(&["update", "--yes"]);
    assert_code(&out, 0);
    assert!(
        text(&out.stdout).contains("1.2.0 -> 1.3.0"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(w.lockfile()["packages"]["solarized"]["version"], "1.3.0");
    assert!(!w.plugins.join("solarized/solarized-1.2.0.ini").exists());
}

#[test]
fn publish_writes_a_listing_the_marketplace_accepts() {
    let w = world();
    let theme = w.root.join("nord.ini");
    std::fs::write(&theme, THEME).unwrap();
    let output = w.root.join("out/nord.json");
    let out = w.rich(&[
        "plugin",
        "publish",
        theme.to_str().unwrap(),
        "--url",
        "https://example.test/releases/nord.ini",
        "--version",
        "1.0.0",
        "--license",
        "MIT",
        "--repository",
        "https://github.com/example/nord",
        "--description",
        "Nord colours.",
        "--published",
        "2026-10-25",
        "--output",
        output.to_str().unwrap(),
    ]);
    assert_code(&out, 0);
    let listing: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(listing["id"], "nord");
    assert_eq!(listing["kind"], "theme");
    assert_eq!(listing["tier"], "community");
    let version = &listing["versions"][0];
    assert_eq!(version["artifact"]["sha256"], sha256(THEME));
    assert_eq!(version["artifact"]["size"], THEME.len());
    assert_eq!(version["requires"]["theme_format"], 1);
    assert_eq!(version["published"], "2026-10-25");

    // A second version goes first; the same version again is refused.
    let out = w.rich(&[
        "plugin",
        "publish",
        theme.to_str().unwrap(),
        "--url",
        "https://example.test/releases/nord-2.ini",
        "--version",
        "2.0.0",
        "--listing",
        output.to_str().unwrap(),
    ]);
    assert_code(&out, 0);
    let listing: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(listing["versions"][0]["version"], "2.0.0");
    assert_eq!(listing["versions"][1]["version"], "1.0.0");
    let again = [
        "plugin",
        "publish",
        theme.to_str().unwrap(),
        "--url",
        "https://example.test/releases/nord-2.ini",
        "--version",
        "2.0.0",
        "--listing",
        output.to_str().unwrap(),
    ];
    assert_code(&w.rich(&again), 4);

    // Plain HTTP, a theme that does not parse, and a crate are refused.
    let mut http = again.to_vec();
    http[4] = "http://example.test/releases/nord.ini";
    assert_code(&w.rich(&http), 2);
    std::fs::write(&theme, b"no section\n").unwrap();
    let out = w.rich(&[
        "plugin",
        "publish",
        theme.to_str().unwrap(),
        "--url",
        "https://example.test/releases/nord.ini",
        "--version",
        "3.0.0",
        "--listing",
        output.to_str().unwrap(),
    ]);
    assert_code(&out, 4);
}

/// rs-rich-marketplace restates micro's pack schema so it need not build
/// rs-rich-micro; the two must agree.
#[cfg(feature = "art")]
#[test]
fn the_marketplace_reads_the_micro_pack_schema_micro_writes() {
    assert_eq!(
        rich_micro::package::SCHEMA_VERSION,
        rich_marketplace::MICRO_SCHEMA_VERSION
    );
}

#[cfg(feature = "wasm-plugins")]
mod wasm {
    use super::*;

    fn markdown(w: &World) -> Output {
        std::fs::write(
            w.root.join("doc.md"),
            "# Doc\n\n```shout\nhello there\n```\n",
        )
        .unwrap();
        w.rich(&["-m", "doc.md"])
    }

    fn shouted(out: &Output) -> bool {
        text(&out.stdout)
            .lines()
            .any(|l| l.trim_end() == "hello there")
    }

    #[test]
    fn an_installed_plugin_loads_without_the_flag_until_disabled_or_changed() {
        let w = world();
        assert!(!shouted(&markdown(&w)));
        let out = w.plugin(&["install", "shout", "--yes", "--report", "json"]);
        assert_code(&out, 0);
        let report: Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(report["result"]["installed"][0]["version"], "0.1.0");
        assert_eq!(report["result"]["installed"][0]["kind"], "wasm-plugin");

        let out = markdown(&w);
        assert_code(&out, 0);
        assert!(shouted(&out), "{}", text(&out.stdout));
        let out = w.rich(&["plugins", "list"]);
        assert!(
            text(&out.stdout)
                .lines()
                .any(|l| l.contains("shout") && l.contains("wasm")),
            "{}",
            text(&out.stdout)
        );

        assert_code(&w.plugin(&["disable", "shout"]), 0);
        assert!(!shouted(&markdown(&w)));
        assert_code(&w.plugin(&["enable", "shout"]), 0);
        assert!(shouted(&markdown(&w)));

        // A changed file is refused at load, with a warning; the render
        // still runs.
        let file = w.plugins.join("shout/shout-0.1.0.wasm");
        let mut bytes = std::fs::read(&file).unwrap();
        bytes.push(0);
        std::fs::write(&file, bytes).unwrap();
        let out = markdown(&w);
        assert_code(&out, 0);
        assert!(!shouted(&out));
        assert!(
            text(&out.stderr).contains("shout is not loaded"),
            "{}",
            text(&out.stderr)
        );
        assert_code(&w.plugin(&["verify"]), 4);
    }

    #[test]
    fn the_sandbox_checks_the_manifest_against_the_listing() {
        let mut w = world();
        // The 0.1.0 artifact's manifest says 0.0.9.
        let wrong = shout("0.0.9");
        std::fs::write(
            w.market
                .dir
                .join("artifacts/example.test/releases/shout-0.1.0.wasm"),
            &wrong,
        )
        .unwrap();
        let listing = w.market.listing_mut("shout");
        listing["versions"][0]["artifact"]["sha256"] = json!(sha256(&wrong));
        listing["versions"][0]["artifact"]["size"] = json!(wrong.len());
        w.market.publish(2);
        let out = w.plugin(&["install", "shout", "--yes"]);
        assert_code(&out, 4);
        assert!(
            text(&out.stderr).contains("manifest's version is 0.0.9"),
            "{}",
            text(&out.stderr)
        );
        assert!(!w.plugins.join("shout").exists());
    }

    #[test]
    fn publish_reads_a_wasm_plugins_manifest() {
        let w = world();
        let module = w.root.join("shout.wasm");
        std::fs::write(&module, shout("0.1.0")).unwrap();
        let output = w.root.join("shout.json");
        let out = w.rich(&[
            "plugin",
            "publish",
            module.to_str().unwrap(),
            "--url",
            "https://github.com/example/rich-shout/releases/download/v0.1.0/shout.wasm",
            "--source-revision",
            "e26c737c5e9741d94abfea61e9d2065c6b59f2e5",
            "--license",
            "MIT",
            "--repository",
            "https://github.com/example/rich-shout",
            "--output",
            output.to_str().unwrap(),
        ]);
        assert_code(&out, 0);
        let listing: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(listing["id"], "shout");
        assert_eq!(listing["description"], "Upper-cases text and shouts fences");
        let version = &listing["versions"][0];
        assert_eq!(version["version"], "0.1.0");
        assert_eq!(version["requires"]["abi"], "1.0");
        assert_eq!(version["capabilities"][1]["kind"], "fence-markup");
        assert_eq!(version["permissions"], json!([]));
        // Without the revision, refused.
        let out = w.rich(&[
            "plugin",
            "publish",
            module.to_str().unwrap(),
            "--url",
            "https://example.test/shout.wasm",
            "--listing",
            output.to_str().unwrap(),
        ]);
        assert_code(&out, 2);
    }
}

/// Write the fixture marketplace to `$RICH_TEST_MARKETPLACE`, for the CI
/// job that installs from it with the built binary.
#[test]
#[ignore = "writes the CI compatibility job's marketplace"]
fn write_test_marketplace() {
    let dir = PathBuf::from(
        std::env::var_os("RICH_TEST_MARKETPLACE")
            .expect("set RICH_TEST_MARKETPLACE to a directory"),
    );
    let _ = std::fs::remove_dir_all(&dir);
    Market::write(&dir);
    println!("wrote the test marketplace to {}", dir.display());
}

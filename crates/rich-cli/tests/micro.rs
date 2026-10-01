//! `rich micro` (#568, #582): listing, creating a package the registry
//! accepts, adding and removing in the user and project layers, packs, the
//! project trust rule, and `:micro:` in `--print --emoji`.
#![cfg(feature = "art")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// `rich` in `work`, with `home` as the home directory and no colour.
fn rich(work: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(work)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "100")
        .env("NO_COLOR", "1")
        .env_remove("RICH_MICRO")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn json(out: &Output) -> serde_json::Value {
    assert!(out.status.success(), "{}", text(&out.stderr));
    serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&out.stdout)))
}

fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    (root, work, home)
}

/// A source animation: the built-in heart.
fn source(work: &Path) -> PathBuf {
    let from =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../rich-micro/builtin/fun/heart/animation.gif");
    let to = work.join("heart.gif");
    std::fs::copy(from, &to).unwrap();
    to
}

fn names(list: &serde_json::Value) -> Vec<String> {
    list["assets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["name"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn list_shows_the_builtin_library() {
    let (_root, work, home) = dirs();
    let list = json(&rich(&work, &home, &["micro", "list", "--report", "json"]));
    let names = names(&list);
    for name in ["status/success", "status/loading", "dev/bug", "fun/heart"] {
        assert!(names.iter().any(|n| n == name), "{name}: {names:?}");
    }
    for asset in list["assets"].as_array().unwrap() {
        assert_eq!(asset["license"], "MIT");
        assert!(asset["alt"].as_str().is_some_and(|a| !a.is_empty()));
    }
    let out = rich(&work, &home, &["micro", "list"]);
    let table = text(&out.stdout);
    assert!(
        table.contains("status/success") && table.contains("✅"),
        "{table}"
    );
    let show = json(&rich(
        &work,
        &home,
        &["micro", "show", "status/loading", "--json"],
    ));
    assert_eq!(show["kind"], "animated");
    assert_eq!(show["claims"][0]["layer"], "built-in");
    let out = rich(&work, &home, &["micro", "show", "nope"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn create_writes_a_package_the_registry_accepts() {
    let (_root, work, home) = dirs();
    source(&work);
    let created = json(&rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "team/love",
            "--alt",
            "a beating heart",
            "--text",
            "<3",
            "--fit",
            "cover",
            "--sharpen",
            "0.6",
            "--contrast",
            "1.1",
            "--add",
            "--report",
            "json",
        ],
    ));
    assert_eq!(created["layer"], "user");
    assert_eq!(created["asset"]["kind"], "animated");
    let path = PathBuf::from(created["path"].as_str().unwrap());
    assert!(path.join("manifest.json").is_file());
    // The registry loads it from the user layer.
    let list = json(&rich(
        &work,
        &home,
        &["micro", "list", "--layer", "user", "--json"],
    ));
    assert_eq!(names(&list), ["team/love"]);
    // And as a standalone .richmicro archive, previewed from its path.
    let out = rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "team/zip",
            "--alt",
            "heart",
            "--archive",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(work.join("team.zip.richmicro").is_file());
    let preview = json(&rich(
        &work,
        &home,
        &["micro", "preview", "team.zip.richmicro", "--json"],
    ));
    assert_eq!(preview["asset"]["name"], "team/zip");
    assert_eq!(preview["mode"], "text");
    // Alt text is mandatory; a bad name is refused.
    let out = rich(
        &work,
        &home,
        &["micro", "create", "heart.gif", "--name", "x"],
    );
    assert_eq!(out.status.code(), Some(2));
    let out = rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "Bad Name",
            "--alt",
            "x",
        ],
    );
    assert_eq!(out.status.code(), Some(4), "{}", text(&out.stderr));
}

#[test]
fn add_remove_and_the_project_trust_rule() {
    let (_root, work, home) = dirs();
    source(&work);
    let out = rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "team/love",
            "--alt",
            "heart",
            "--text",
            "<3",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = rich(&work, &home, &["micro", "add", "team.love", "--project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(work.join(".rich/micro/team.love/manifest.json").is_file());

    // Untrusted: not loaded, and said so.
    let out = rich(&work, &home, &["micro", "list", "--layer", "project"]);
    assert!(
        text(&out.stderr).contains("not trusted"),
        "{}",
        text(&out.stderr)
    );
    let list = json(&rich(&work, &home, &["micro", "list", "--json"]));
    assert!(!names(&list).contains(&"team/love".to_string()));
    // A project's own ./rich.toml cannot trust itself.
    std::fs::write(
        work.join("rich.toml"),
        "version = 1\n[defaults]\nmicro_project = true\n",
    )
    .unwrap();
    let out = rich(&work, &home, &["micro", "list"]);
    assert!(
        text(&out.stderr).contains("micro_project = true in ./rich.toml is ignored"),
        "{}",
        text(&out.stderr)
    );
    assert!(!text(&out.stdout).contains("team/love"));
    std::fs::remove_file(work.join("rich.toml")).unwrap();
    // The flag, and the user's own config, can.
    let list = json(&rich(
        &work,
        &home,
        &["micro", "list", "--micro-project", "--json"],
    ));
    assert!(names(&list).contains(&"team/love".to_string()));
    let config = home.join(".config/rich");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(
        config.join("config.toml"),
        "version = 1\n[defaults]\nmicro_project = true\n",
    )
    .unwrap();
    let list = json(&rich(&work, &home, &["micro", "list", "--json"]));
    assert!(names(&list).contains(&"team/love".to_string()));

    // Remove from the project layer; the user layer never had it.
    let out = rich(&work, &home, &["micro", "remove", "team/love"]);
    assert_eq!(out.status.code(), Some(2));
    let out = rich(&work, &home, &["micro", "remove", "team/love", "--project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(!work.join(".rich/micro/team.love").exists());
}

#[test]
fn packs_install_and_uninstall() {
    let (_root, work, home) = dirs();
    let pack = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rich-micro/builtin/fun");
    let out = rich(&work, &home, &["micro", "install", pack.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let packs = json(&rich(&work, &home, &["micro", "packs", "--json"]));
    let user: Vec<_> = packs["packs"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["layer"] == "user")
        .collect();
    assert_eq!(user.len(), 1);
    assert_eq!(user[0]["name"], "fun");
    // Installing it again is refused; a pack is not a package.
    let out = rich(&work, &home, &["micro", "install", pack.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let out = rich(&work, &home, &["micro", "add", pack.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    let out = rich(&work, &home, &["micro", "uninstall", "fun"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let packs = json(&rich(&work, &home, &["micro", "packs", "--json"]));
    assert!(packs["packs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["layer"] == "built-in"));
}

#[test]
fn micro_markup_in_print_with_emoji() {
    let (_root, work, home) = dirs();
    let out = rich(
        &work,
        &home,
        &[
            "-p",
            "--emoji",
            "ok :micro:status/success: :micro:nope: :fire:",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "ok ✅ :micro:nope: 🔥\n");
    // Without --emoji, as upstream: codes stay as typed.
    let out = rich(&work, &home, &["-p", "ok :micro:status/success:"]);
    assert_eq!(text(&out.stdout), "ok :micro:status/success:\n");
}

#[cfg(feature = "interact")]
#[test]
fn asset_picks_micro_assets_by_name() {
    let (_root, work, home) = dirs();
    let out = rich(
        &work,
        &home,
        &["asset", "--kind", "micro", "--selected", "status/success"],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "status/success\n");
    let out = rich(
        &work,
        &home,
        &["asset", "--kind", "micro", "--selected", "nope"],
    );
    assert_eq!(out.status.code(), Some(2));
}

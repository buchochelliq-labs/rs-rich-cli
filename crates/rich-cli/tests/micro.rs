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

#[test]
fn preview_times_only_animation_frames() {
    // 0.0.15 workstream 6: a still image has no frame time to show.
    let (_root, work, home) = dirs();
    let builtin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../rich-micro/builtin");
    std::fs::copy(
        builtin.join("status/success/static.png"),
        work.join("still.png"),
    )
    .unwrap();
    for args in [
        &["micro", "preview", "status/success"][..],
        &["micro", "preview", "still.png"],
    ] {
        let out = rich(&work, &home, args);
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
        let shown = text(&out.stdout);
        assert!(!shown.contains(" ms"), "{args:?}: {shown}");
    }
    // An animation still shows each frame's time.
    let out = rich(&work, &home, &["micro", "preview", "fun/heart"]);
    let shown = text(&out.stdout);
    assert!(shown.contains(" ms"), "{shown}");
}

#[test]
fn micro_markup_in_titles_captions_and_markdown() {
    // 0.0.15 workstream 6: not only `--print --emoji`.
    let (_root, work, home) = dirs();
    // A panel's labels expand `:emoji:` codes always, and micro tokens too.
    let out = rich(
        &work,
        &home,
        &[
            "-p",
            "a body wider than the caption",
            "--panel",
            "rounded",
            "--title",
            "T :micro:status/success:",
            "--caption",
            ":micro:status/error: c :micro:nope:",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    let lines: Vec<&str> = shown.lines().collect();
    assert!(lines[0].contains("─ T ✅ ─"), "{shown}");
    assert!(lines[2].contains("❌ c :micro:nope:"), "{shown}");
    // A CSV table's labels expand where its `:emoji:` codes do: with --emoji.
    std::fs::write(work.join("t.csv"), "a,b\n1,2\n").unwrap();
    let csv = |extra: &[&str]| {
        let mut args = vec![
            "t.csv",
            "--title",
            ":micro:status/success: t",
            "--caption",
            ":micro:status/error:",
        ];
        args.extend_from_slice(extra);
        text(&rich(&work, &home, &args).stdout)
    };
    let shown = csv(&["--emoji"]);
    assert!(shown.contains("✅ t") && shown.contains("❌"), "{shown}");
    let shown = csv(&[]);
    assert!(shown.contains(":micro:"), "{shown}");
    // Markdown, outside code.
    std::fs::write(
        work.join("t.md"),
        "# Status :micro:status/success:\n\nok **:micro:status/success:** \
         `:micro:status/success:`\n",
    )
    .unwrap();
    let out = rich(&work, &home, &["t.md"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let shown = text(&out.stdout);
    assert!(shown.contains("Status ✅"), "{shown}");
    assert!(shown.contains("ok ✅ :micro:status/success:"), "{shown}");
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

/// A PNG header (signature, IHDR, IDAT, IEND): enough for the package
/// reader's header check.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8], data: &[u8]| {
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(kind);
        out.extend(data);
        out.extend([0; 4]);
    };
    let mut ihdr = width.to_be_bytes().to_vec();
    ihdr.extend(height.to_be_bytes());
    ihdr.extend([8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &[]);
    chunk(b"IEND", &[]);
    out
}

/// A package directory at `dir` holding `manifest`.
fn package(dir: &Path, manifest: serde_json::Value) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
    std::fs::write(dir.join("s.png"), png(16, 16)).unwrap();
}

fn simple_manifest(name: &str) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1, "name": name, "alt": "a square",
        "static": "s.png", "fallback": {"text": "ok"},
    })
}

/// No terminal control (ESC, BEL, C1 CSI) reaches the terminal.
fn assert_inert(what: &str, out: &Output) {
    for (stream, bytes) in [("stdout", &out.stdout), ("stderr", &out.stderr)] {
        let shown = text(bytes);
        assert!(
            !shown.contains(['\u{1b}', '\u{7}', '\u{9b}']),
            "{what} {stream}: {shown:?}"
        );
    }
}

#[test]
fn untrusted_package_strings_never_reach_the_terminal_raw() {
    // Release-test audit A, F1: pack entry names, manifest fields and the
    // paths packages live at are untrusted; every message that quotes them
    // shows their controls instead of executing them.
    let (root, work, home) = dirs();
    let evil = "\u{1b}]0;PWNED\u{7}\u{1b}[31mRED";
    // A pack naming a package with controls in it.
    let pack = root.path().join("evilpack");
    package(&pack.join("good"), simple_manifest("evil/good"));
    std::fs::write(
        pack.join("pack.json"),
        serde_json::json!({"schema_version": 1, "name": "evil", "packages": ["good", evil]})
            .to_string(),
    )
    .unwrap();
    let out = rich(&work, &home, &["micro", "install", pack.to_str().unwrap()]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_inert("install", &out);
    assert!(
        text(&out.stderr).contains("␛]0;PWNED␇"),
        "{}",
        text(&out.stderr)
    );
    let out = rich(&work, &home, &["micro", "list"]);
    assert_inert("list", &out);
    let out = rich(&work, &home, &["micro", "packs"]);
    assert_inert("packs", &out);

    // A package whose manifest carries controls is refused, and saying so
    // does not run them.
    let pkg = root.path().join("evilpkg");
    let mut manifest = simple_manifest("evilauthor");
    manifest["author"] = serde_json::json!(evil);
    manifest["version"] = serde_json::json!("1\u{1b}[2J");
    package(&pkg, manifest);
    let out = rich(&work, &home, &["micro", "add", pkg.to_str().unwrap()]);
    assert_inert("add", &out);
    assert!(!out.status.success(), "{}", text(&out.stdout));
    let out = rich(&work, &home, &["micro", "show", "evilauthor"]);
    assert_inert("show", &out);

    // Packages in user-layer folders whose names hold controls: their
    // origins (in `show`, `explain` and the collision warning) are shown.
    let user = home.join(".config/rich/micro");
    package(&user.join(format!("a{evil}")), simple_manifest("team/x"));
    package(&user.join(format!("b{evil}")), simple_manifest("team/x"));
    let out = rich(&work, &home, &["micro", "show", "team/x"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_inert("show (origin)", &out);
    let out = rich(&work, &home, &["micro", "remove", "team/x"]);
    assert_inert("remove", &out);

    // An untrusted project under a directory whose name holds controls.
    let project = root.path().join(format!("p{evil}"));
    std::fs::create_dir_all(project.join(".rich/micro")).unwrap();
    for args in [&["micro", "list"][..], &["micro", "packs"]] {
        let out = rich(&project, &home, args);
        assert_inert("untrusted note", &out);
    }
}

#[cfg(unix)]
#[test]
fn create_and_add_leave_an_existing_link_alone() {
    // Release-test audit A, F5: `create --output PATH` over a dangling
    // symbolic link refused with "File exists" and then deleted the link.
    let (_root, work, home) = dirs();
    source(&work);
    std::os::unix::fs::symlink("/nonexistent/target", work.join("out")).unwrap();
    let out = rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "x",
            "--alt",
            "y",
            "--output",
            "out",
        ],
    );
    assert!(!out.status.success());
    assert!(
        text(&out.stderr).contains("already exists"),
        "{}",
        text(&out.stderr)
    );
    let link = std::fs::symlink_metadata(work.join("out")).expect("the link is kept");
    assert!(link.file_type().is_symlink());

    // `add` checks its destination the same way.
    let out = rich(
        &work,
        &home,
        &[
            "micro",
            "create",
            "heart.gif",
            "--name",
            "team/x",
            "--alt",
            "y",
            "--archive",
        ],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let user = home.join(".config/rich/micro");
    std::fs::create_dir_all(&user).unwrap();
    std::os::unix::fs::symlink("/nonexistent/target", user.join("team.x.richmicro")).unwrap();
    let out = rich(&work, &home, &["micro", "add", "team.x.richmicro"]);
    assert!(!out.status.success());
    assert!(std::fs::symlink_metadata(user.join("team.x.richmicro")).is_ok());
}

/// [`rich`], failing the test when it has not finished within 20 seconds.
#[cfg(unix)]
fn rich_within(work: &Path, home: &Path, args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(work)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("COLUMNS", "100")
        .env("NO_COLOR", "1")
        .env_remove("RICH_MICRO")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while child.try_wait().unwrap().is_none() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            panic!("rich {args:?} hung");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.wait_with_output().unwrap()
}

#[cfg(unix)]
#[test]
fn a_fifo_in_a_layer_is_skipped_with_a_warning() {
    // Release-test audit A, F6: a FIFO named `*.richmicro` in a (cloned,
    // untrusted) project's .rich/micro hung every `--project` command, and
    // every command once the project was trusted.
    let (root, work, home) = dirs();
    let layer = work.join(".rich/micro");
    std::fs::create_dir_all(&layer).unwrap();
    let made = Command::new("mkfifo")
        .arg(layer.join("a.richmicro"))
        .status()
        .unwrap();
    assert!(made.success());
    let pkg = root.path().join("pkg");
    package(&pkg, simple_manifest("team/x"));

    let out = rich_within(&work, &home, &["micro", "remove", "--project", "foo"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    let out = rich_within(
        &work,
        &home,
        &["micro", "add", "--project", pkg.to_str().unwrap()],
    );
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = rich_within(&work, &home, &["micro", "uninstall", "--project", "foo"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    let out = rich_within(&work, &home, &["micro", "packs", "--micro-project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = rich_within(&work, &home, &["micro", "list", "--micro-project"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("not a regular file"),
        "{}",
        text(&out.stderr)
    );
    assert!(
        text(&out.stdout).contains("team/x"),
        "{}",
        text(&out.stdout)
    );

    // Writing to an untrusted project's layer does not trust it: what was
    // added there is not drawn.
    let out = rich_within(&work, &home, &["-p", "--emoji", "a :micro:team/x: b"]);
    assert_eq!(text(&out.stdout), "a :micro:team/x: b\n");
    let out = rich_within(
        &work,
        &home,
        &["-p", "--emoji", "--micro-project", "a :micro:team/x: b"],
    );
    assert_eq!(text(&out.stdout), "a ok b\n");
}

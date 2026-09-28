//! `rich plugins list/info`, `--plugin PATH`, and config trust for runtime
//! plugins: a project's `./rich.toml` never loads one; the user's own config,
//! `--config PATH` and `--plugin` do.

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
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A work directory and an empty home.
fn dirs() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    let home = root.path().join("home");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    (root, work, home)
}

#[test]
fn plugins_list_shows_the_built_ins_with_source_version_and_abi() {
    let (_root, work, home) = dirs();
    let out = rich(&work, &home, &["plugins", "list"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    for expected in [
        "Name",
        "Source",
        "ABI",
        "rich-ext",
        "built-in",
        "plugin API 1",
    ] {
        assert!(stdout.contains(expected), "{expected}: {stdout}");
    }
    if !cfg!(all(feature = "dylib-plugins", feature = "wasm-plugins")) {
        assert!(stdout.contains("this build lacks"), "{stdout}");
    }
    // `rich plugins` alone lists too.
    assert_eq!(text(&rich(&work, &home, &["plugins"]).stdout), stdout);

    let out = rich(&work, &home, &["plugins", "--report", "json"]);
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["plugin_api"], 1);
    assert_eq!(report["abi"], "1.0");
    assert_eq!(
        report["runtime_loading"]["wasm"],
        cfg!(feature = "wasm-plugins")
    );
    let first = &report["plugins"][0];
    assert_eq!(first["id"], "rich-ext");
    assert_eq!(first["source"], "built-in");
    assert_eq!(first["abi"], "plugin API 1");
}

#[test]
fn plugins_info_shows_one_plugin_and_refuses_an_unknown_one() {
    let (_root, work, home) = dirs();
    let out = rich(&work, &home, &["plugins", "info", "rich-ext"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains("Version") && stdout.contains("syntect"),
        "{stdout}"
    );

    let out = rich(&work, &home, &["plugins", "info", "nope"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("unknown plugin \"nope\""));
    let out = rich(&work, &home, &["plugins", "info"]);
    assert_eq!(out.status.code(), Some(2));
    let out = rich(&work, &home, &["plugins", "list", "--bogus"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(rich(&work, &home, &["plugins", "--help"]).status.success());
}

#[test]
fn a_plugin_that_cannot_load_is_a_clear_error() {
    let (_root, work, home) = dirs();
    let out = rich(&work, &home, &["--plugin", "missing.wasm", "-p", "hi"]);
    let stderr = text(&out.stderr);
    if cfg!(feature = "wasm-plugins") {
        assert_eq!(out.status.code(), Some(3), "{stderr}");
        assert!(stderr.contains("missing.wasm"), "{stderr}");
    } else {
        assert_eq!(out.status.code(), Some(2), "{stderr}");
        assert!(stderr.contains("wasm-plugins feature"), "{stderr}");
    }
    assert!(!stderr.contains("panicked"), "{stderr}");

    std::fs::write(work.join("notes.txt"), "x").unwrap();
    let out = rich(&work, &home, &["--plugin", "notes.txt", "-p", "hi"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("not a plugin file"));

    std::fs::write(work.join("junk.wasm"), "not wasm").unwrap();
    let out = rich(
        &work,
        &home,
        &["--plugin", "junk.wasm", "-p", "hi", "--report", "json"],
    );
    assert_eq!(out.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["code"], "usage");
}

#[test]
fn a_project_config_cannot_list_plugins() {
    let (_root, work, home) = dirs();
    std::fs::write(
        work.join("rich.toml"),
        "[defaults]\nplugins = [\"evil.wasm\"]\n",
    )
    .unwrap();
    // Never loaded, so a missing file is no error, only the warning.
    let out = rich(&work, &home, &["plugins", "list"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert!(
        text(&out.stderr).contains("plugins in ./rich.toml are ignored"),
        "{}",
        text(&out.stderr)
    );
    assert!(!text(&out.stdout).contains("evil"));
    let out = rich(&work, &home, &["-p", "hi"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let out = rich(&work, &home, &["config", "explain", "plugins"]);
    assert!(
        text(&out.stdout).contains("ignored"),
        "{}",
        text(&out.stdout)
    );

    // A list of anything but paths is a config error.
    std::fs::write(work.join("rich.toml"), "[defaults]\nplugins = \"x.wasm\"\n").unwrap();
    let out = rich(&work, &home, &["-p", "hi"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
}

#[cfg(feature = "wasm-plugins")]
mod wasm {
    use super::*;

    const SHOUT: &str = include_str!("../../rich-plugin-api/examples/wasm/shout.wat");

    fn shout(dir: &Path) -> PathBuf {
        let path = dir.join("shout.wasm");
        std::fs::write(&path, wat::parse_str(SHOUT).unwrap()).unwrap();
        path
    }

    fn lists_shout(out: &Output) -> bool {
        text(&out.stdout)
            .lines()
            .any(|line| line.contains("shout") && line.contains("wasm"))
    }

    #[test]
    fn untrusted_config_does_not_load_but_trusted_config_and_the_flag_do() {
        let (_root, work, home) = dirs();
        shout(&work);
        let config = "[defaults]\nplugins = [\"shout.wasm\"]\n";
        std::fs::write(work.join("rich.toml"), config).unwrap();

        // ./rich.toml is the project's: ignored, with a warning.
        let out = rich(&work, &home, &["plugins", "list"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(!lists_shout(&out), "{}", text(&out.stdout));
        assert!(text(&out.stderr).contains("are ignored"));

        // The same file named with --config is the user's choice: loaded,
        // relative to the file.
        let out = rich(&work, &home, &["plugins", "list", "--config", "rich.toml"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(lists_shout(&out), "{}", text(&out.stdout));

        // So is the user's own config, from any directory.
        let user = home.join(".config/rich");
        std::fs::create_dir_all(&user).unwrap();
        shout(&user);
        std::fs::write(user.join("config.toml"), config).unwrap();
        let elsewhere = home.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        let out = rich(&elsewhere, &home, &["plugins", "list"]);
        assert!(lists_shout(&out), "{}", text(&out.stderr));

        // A second plugin with the same id, from another file, is refused.
        std::fs::remove_file(work.join("rich.toml")).unwrap();
        let out = rich(&work, &home, &["plugins", "list", "--plugin", "shout.wasm"]);
        assert_eq!(out.status.code(), Some(2));
        assert!(text(&out.stderr).contains("already registered"));

        // And --plugin, which is on the command line.
        std::fs::remove_file(user.join("config.toml")).unwrap();
        let out = rich(&work, &home, &["plugins", "list", "--plugin", "shout.wasm"]);
        assert!(lists_shout(&out), "{}", text(&out.stderr));
        let out = rich(
            &work,
            &home,
            &["plugins", "info", "shout", "--plugin", "shout.wasm"],
        );
        let stdout = text(&out.stdout);
        for expected in [
            "WASM ABI 1.0",
            "wasm",
            "shout.wasm",
            "fence-markup \"shout\"",
        ] {
            assert!(stdout.contains(expected), "{expected}: {stdout}");
        }
    }

    #[test]
    fn a_loaded_plugin_draws_its_fences_in_markdown() {
        let (_root, work, home) = dirs();
        shout(&work);
        std::fs::write(work.join("doc.md"), "# Doc\n\n```shout\nhello there\n```\n").unwrap();
        // Without the plugin the fence is a code block (indented by padding).
        let out = rich(&work, &home, &["-m", "doc.md"]);
        assert!(text(&out.stdout)
            .lines()
            .any(|l| l.trim_end() == " hello there"));
        let out = rich(&work, &home, &["--plugin", "shout.wasm", "-m", "doc.md"]);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(
            text(&out.stdout)
                .lines()
                .any(|l| l.trim_end() == "hello there"),
            "{}",
            text(&out.stdout)
        );
        // The same path twice loads once.
        let out = rich(
            &work,
            &home,
            &[
                "--plugin",
                "shout.wasm",
                "--plugin",
                "./shout.wasm",
                "-p",
                "x",
            ],
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
    }

    #[test]
    fn a_plugin_that_claims_a_built_in_name_is_refused() {
        let (_root, work, home) = dirs();
        // "mermaid" is two bytes longer than "shout", and so is the manifest.
        let wat = SHOUT
            .replace("fence-markup shout", "fence-markup mermaid")
            .replace("(i32.const 149)", "(i32.const 151)");
        std::fs::write(work.join("m.wasm"), wat::parse_str(&wat).unwrap()).unwrap();
        let out = rich(&work, &home, &["--plugin", "m.wasm", "-p", "x"]);
        if cfg!(feature = "mermaid") {
            assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
            assert!(
                text(&out.stderr).contains("mermaid"),
                "{}",
                text(&out.stderr)
            );
        }
    }
}

#[cfg(feature = "dylib-plugins")]
mod dylib {
    use super::*;

    include!("../../rich-plugin-api/examples/dylib-plugin/build_fixture.rs");

    #[test]
    fn a_native_plugin_loads_from_the_command_line() {
        let (root, work, home) = dirs();
        let library = build_example_dylib(root.path(), &[]);
        let path = library.to_str().unwrap();
        let out = rich(
            &work,
            &home,
            &["plugins", "info", "example-dylib", "--plugin", path],
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        for expected in [
            "native",
            "C ABI 1.0",
            "transform \"reverse\"",
            "fence-ansi \"red\"",
        ] {
            assert!(stdout.contains(expected), "{expected}: {stdout}");
        }

        std::fs::write(work.join("doc.md"), "```red\nstop\n```\n").unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_rich"))
            .current_dir(&work)
            .env("HOME", &home)
            .env("COLUMNS", "40")
            .env_remove("NO_COLOR")
            .args(["--plugin", path, "--force-terminal", "-m", "doc.md"])
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        assert!(stdout.contains("stop"), "{stdout:?}");
        // Colour survives the sanitizer; the window-title escape does not.
        assert!(stdout.contains("\u{1b}[31m"), "{stdout:?}");
        assert!(!stdout.contains("\u{1b}]0;"), "{stdout:?}");
    }
}

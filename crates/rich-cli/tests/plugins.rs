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

/// Every render-mode flag, long and short (`mode_flag_alias` in lib.rs).
const MODE_FLAGS: &[&str] = &[
    "--print",
    "-p",
    "--markdown",
    "-m",
    "--json",
    "-j",
    "-J",
    "--syntax",
    "-x",
    "--csv",
    "--ipynb",
    "--rst",
    "--jsonl",
    "--ndjson",
    "--log",
    "--rule",
    "-u",
    "--image",
    "--gif",
    "--diff",
    "--inspect",
    "--ansi-explain",
];

/// After a render-mode flag, `plugins` is the resource, not the subcommand:
/// `rich -p plugins` prints the word.
#[test]
fn a_mode_flag_makes_plugins_a_resource() {
    let (_root, work, home) = dirs();
    for flag in MODE_FLAGS {
        let out = rich(&work, &home, &[flag, "plugins"]);
        let stdout = text(&out.stdout);
        assert!(
            !stdout.contains("Capabilities") && !stdout.contains("plugin API"),
            "{flag}: {stdout}"
        );
    }
    for flag in ["-p", "--print"] {
        let out = rich(&work, &home, &[flag, "plugins"]);
        assert!(out.status.success(), "{flag}: {}", text(&out.stderr));
        assert_eq!(text(&out.stdout).trim_end(), "plugins", "{flag}");
    }
}

/// Whether `output` sets a foreground or background colour (bold alone is
/// not colour: `no_color` keeps it).
fn has_colour(output: &str) -> bool {
    output.split("\u{1b}[").skip(1).any(|sequence| {
        let Some(end) = sequence.find('m') else {
            return false;
        };
        sequence[..end].split(';').any(|param| {
            matches!(
                param.parse::<u8>(),
                Ok(30..=38 | 40..=48 | 90..=97 | 100..=107)
            )
        })
    })
}

/// `rich plugins` takes its colour from the merged command line, config
/// first, as the rest of the binary does: a config's `no_color = true` turns
/// colour off on a terminal, and a trusted config's `no_color = false` beats
/// NO_COLOR. Its console colours only a terminal (FORCE_COLOR is not
/// supported), so it runs on a pseudo-terminal. The list and info tables are
/// bold only, which `no_color` keeps, so the colour shows in its help, which
/// resolves colour the same way.
#[cfg(unix)]
#[test]
fn plugins_colour_follows_config() {
    use portable_pty::{native_pty_system, CommandBuilder, PtySize};
    use std::io::Read;

    let (_root, work, home) = dirs();
    let run = |no_color_env: bool, config: &str, args: &[&str]| {
        let path = home.join("rich-test.toml");
        std::fs::write(&path, format!("[defaults]\n{config}")).unwrap();
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 40,
                cols: 100,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_rich"));
        command.args(args);
        command.arg("--config");
        command.arg(&path);
        command.cwd(&work);
        command.env("HOME", &home);
        command.env("XDG_CONFIG_HOME", home.join(".config"));
        command.env("TERM", "xterm-256color");
        command.env_remove("NO_COLOR");
        if no_color_env {
            command.env("NO_COLOR", "1");
        }
        let mut child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let output = std::thread::spawn(move || {
            let mut output = Vec::new();
            let mut buffer = [0u8; 4096];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                output.extend_from_slice(&buffer[..read]);
            }
            output
        });
        let status = child.wait().unwrap();
        drop(pty.master);
        let output = text(&output.join().unwrap());
        assert!(status.success(), "{output}");
        output
    };
    for args in [&["plugins", "--help"][..], &["plugins", "list", "--help"]] {
        let coloured = |no_color_env: bool, config: &str| {
            let output = run(no_color_env, config, args);
            assert!(output.contains("Usage"), "{output}");
            has_colour(&output)
        };
        // A terminal colours; NO_COLOR does not.
        assert!(coloured(false, ""), "{args:?}");
        assert!(!coloured(true, ""), "{args:?}");
        // The config asks for no colour.
        assert!(!coloured(false, "no_color = true\n"), "{args:?}");
        // A trusted config's `no_color = false` beats NO_COLOR.
        assert!(coloured(true, "no_color = false\n"), "{args:?}");
    }
    // The list itself runs with either config (its tables are bold only).
    for (no_color_env, config) in [(false, "no_color = true\n"), (true, "no_color = false\n")] {
        let output = run(no_color_env, config, &["plugins", "list"]);
        assert!(output.contains("rich-ext"), "{output}");
        assert!(!has_colour(&output), "{output}");
    }
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

    /// In-process runs share the runtime-plugin slot: a later run without
    /// `--plugin` must not keep the previous run's plugins.
    #[test]
    fn a_later_in_process_run_does_not_keep_the_previous_plugins() {
        use std::process::ExitCode;
        let (_root, work, _home) = dirs();
        let plugin = shout(&work);
        let plugin = plugin.to_str().unwrap();
        let out = work.join("out.html");
        let run = |args: &[&str]| {
            let mut all: Vec<std::ffi::OsString> = vec!["--no-config".into()];
            all.extend(args.iter().map(Into::into));
            all.extend(["-o".into(), out.clone().into_os_string()]);
            rich_cli::run_embedded(Vec::new(), all)
        };
        let first = run(&["--plugin", plugin, "--transform", "upper", "-p", "hi"]);
        assert_eq!(first, ExitCode::SUCCESS);
        assert!(std::fs::read_to_string(&out).unwrap().contains("HI"));
        // No plugin now: `upper` is unknown, a usage error.
        assert_eq!(
            run(&["--transform", "upper", "-p", "hi"]),
            ExitCode::from(2)
        );
    }

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

        // The path is absolute, as a native plugin's is, and a long one is
        // folded rather than cut off with "…".
        let deep = work.join("a-rather-long-directory-name".repeat(3));
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::copy(work.join("shout.wasm"), deep.join("shout.wasm")).unwrap();
        let relative = format!("{}/shout.wasm", deep.file_name().unwrap().to_str().unwrap());
        let args = ["plugins", "info", "shout", "--plugin", relative.as_str()];
        let out = rich(&work, &home, &args);
        let stdout = text(&out.stdout);
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(!stdout.contains('…'), "{stdout}");
        let out = rich(&work, &home, &[&args[..], &["--report", "json"]].concat());
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let path = std::fs::canonicalize(deep.join("shout.wasm")).unwrap();
        assert_eq!(report["path"], path.to_str().unwrap(), "{report}");
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

/// `--transform NAME` checks its names once plugins are loaded; with none
/// registered, the message says so. It rewrites text only.
#[test]
fn an_unknown_or_misplaced_transform_is_a_usage_error() {
    let (_root, work, home) = dirs();
    let out = rich(&work, &home, &["--transform", "nope", "-p", "hi"]);
    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("unknown transform \"nope\" for --transform"),
        "{stderr}"
    );
    assert!(stderr.contains("no transform is registered"), "{stderr}");
    let out = rich(&work, &home, &["--transform"]);
    assert_eq!(out.status.code(), Some(2));

    std::fs::write(work.join("doc.md"), "# Doc\n").unwrap();
    let out = rich(&work, &home, &["--transform", "x", "-m", "doc.md"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("--transform only has an effect on text, --print or --syntax"),
        "{}",
        text(&out.stderr)
    );
    std::fs::write(work.join("a.json"), "{}").unwrap();
    let out = rich(&work, &home, &["--transform", "x", "--inspect", "a.json"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(
        text(&out.stderr).contains("not --inspect"),
        "{}",
        text(&out.stderr)
    );
}

#[cfg(feature = "wasm-plugins")]
mod wasm_transforms {
    use super::*;

    const SHOUT: &str = include_str!("../../rich-plugin-api/examples/wasm/shout.wat");

    /// A second plugin, `tag`: its `tag` transform replaces any text with
    /// "made by tag", and its `broken` transform always fails.
    fn tag_wat() -> String {
        let manifest = "rich-plugin-abi 1.0\\nname tag\\nversion 0.1.0\\n\
                        capability transform tag\\ncapability transform broken\\n";
        let len = manifest.replace("\\n", "\n").len();
        format!(
            r#"(module
  (memory (export "memory") 1)
  (data (i32.const 0) "{manifest}")
  (data (i32.const 512) "made by tag")
  (data (i32.const 600) "it broke")
  (func $pack (param $ptr i32) (param $len i32) (result i64)
    (i64.or (i64.shl (i64.extend_i32_u (local.get $ptr)) (i64.const 32))
            (i64.extend_i32_u (local.get $len))))
  (func (export "rich_plugin_alloc") (param $len i32) (result i32) (i32.const 4096))
  (func (export "rich_plugin_manifest") (result i64)
    (call $pack (i32.const 0) (i32.const {len})))
  (func (export "rich_plugin_call")
    (param $cap i32) (param $ptr i32) (param $len i32) (param $width i32) (result i64)
    (if (i32.eqz (local.get $cap))
      (then (return (call $pack (i32.const 512) (i32.const 11)))))
    (i64.or (call $pack (i32.const 600) (i32.const 8)) (i64.const 0x8000000000000000)))
)"#
        )
    }

    /// `shout.wasm` (the `upper` transform) and `tag.wasm` in `dir`.
    fn plugins(dir: &Path) {
        std::fs::write(dir.join("shout.wasm"), wat::parse_str(SHOUT).unwrap()).unwrap();
        std::fs::write(dir.join("tag.wasm"), wat::parse_str(tag_wat()).unwrap()).unwrap();
    }

    fn with_plugins<'a>(args: &[&'a str]) -> Vec<&'a str> {
        let mut all = vec!["--plugin", "shout.wasm", "--plugin", "tag.wasm"];
        all.extend_from_slice(args);
        all
    }

    #[test]
    fn a_plugin_transform_applies_to_text_print_and_syntax() {
        let (_root, work, home) = dirs();
        plugins(&work);
        let out = rich(
            &work,
            &home,
            &[
                "--plugin",
                "shout.wasm",
                "--transform",
                "upper",
                "-p",
                "hello there",
            ],
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert_eq!(text(&out.stdout).trim_end(), "HELLO THERE");

        std::fs::write(work.join("notes.txt"), "quiet words\n").unwrap();
        let out = rich(
            &work,
            &home,
            &with_plugins(&["--transform", "upper", "notes.txt"]),
        );
        assert!(
            text(&out.stdout).contains("QUIET WORDS"),
            "{}",
            text(&out.stdout)
        );

        std::fs::write(work.join("main.py"), "print('hi')\n").unwrap();
        let out = rich(
            &work,
            &home,
            &with_plugins(&["--transform", "upper", "main.py"]),
        );
        assert!(out.status.success(), "{}", text(&out.stderr));
        assert!(
            text(&out.stdout).contains("PRINT('HI')"),
            "{}",
            text(&out.stdout)
        );

        // `rich plugins list` shows the transform's name.
        let out = rich(&work, &home, &with_plugins(&["plugins", "list"]));
        assert!(text(&out.stdout).contains("transform \"upper\""));
    }

    #[test]
    fn an_unknown_transform_lists_the_available_names() {
        let (_root, work, home) = dirs();
        plugins(&work);
        let out = rich(
            &work,
            &home,
            &with_plugins(&["--transform", "lower", "-p", "x"]),
        );
        assert_eq!(out.status.code(), Some(2), "{}", text(&out.stderr));
        let stderr = text(&out.stderr);
        assert!(
            stderr.contains(
                "unknown transform \"lower\" for --transform; transforms: broken, tag, upper"
            ),
            "{stderr}"
        );
        let out = rich(
            &work,
            &home,
            &with_plugins(&["--transform", "lower", "-p", "x", "--report", "json"]),
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
        assert_eq!(report["code"], "usage");
    }

    #[test]
    fn transforms_apply_in_the_order_given() {
        let (_root, work, home) = dirs();
        plugins(&work);
        let run = |order: &[&str]| {
            let mut args = Vec::new();
            for name in order {
                args.extend(["--transform", *name]);
            }
            args.extend(["-p", "anything"]);
            let out = rich(&work, &home, &with_plugins(&args));
            assert!(out.status.success(), "{}", text(&out.stderr));
            text(&out.stdout).trim_end().to_string()
        };
        assert_eq!(run(&["tag", "upper"]), "MADE BY TAG");
        assert_eq!(run(&["upper", "tag"]), "made by tag");
        // The same transform twice runs twice.
        assert_eq!(run(&["upper", "upper"]), "ANYTHING");
    }

    #[test]
    fn a_failing_transform_is_named_like_the_other_transforms() {
        let (_root, work, home) = dirs();
        plugins(&work);
        let out = rich(
            &work,
            &home,
            &with_plugins(&["--transform", "broken", "-p", "x"]),
        );
        assert_eq!(out.status.code(), Some(4), "{}", text(&out.stderr));
        let stderr = text(&out.stderr);
        assert!(stderr.contains("--transform broken failed: "), "{stderr}");
        assert!(stderr.contains("it broke"), "{stderr}");
    }

    /// The documented order is `--filter`, each `--transform`, `--highlight`,
    /// whatever order the flags are given in: the filter matches the text as
    /// read, and the highlight the transformed text.
    #[test]
    fn transforms_run_between_filter_and_highlight() {
        let (_root, work, home) = dirs();
        plugins(&work);
        std::fs::write(work.join("fruit.txt"), "apple\nbanana\ncherry\n").unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_rich"))
            .current_dir(&work)
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", home.join(".config"))
            .env("COLUMNS", "40")
            .env_remove("NO_COLOR")
            .args(with_plugins(&[
                "--highlight",
                "AN",
                "--transform",
                "upper",
                "--filter",
                "an",
                "--force-terminal",
                "fruit.txt",
            ]))
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(out.status.success(), "{}", text(&out.stderr));
        let stdout = text(&out.stdout);
        // `--filter an` saw the lowercase input: only "banana" is left.
        assert!(
            !stdout.contains("APPLE") && !stdout.contains("CHERRY"),
            "{stdout:?}"
        );
        // `--highlight AN` saw the upper-cased text: "AN" is in reverse video.
        assert!(stdout.contains("\u{1b}[7mAN"), "{stdout:?}");
        let plain: String = stdout
            .split('\u{1b}')
            .enumerate()
            .map(|(i, part)| {
                if i == 0 {
                    part
                } else {
                    part.split_once('m').map_or("", |p| p.1)
                }
            })
            .collect();
        assert_eq!(plain.trim_end(), "BANANA");
    }
}

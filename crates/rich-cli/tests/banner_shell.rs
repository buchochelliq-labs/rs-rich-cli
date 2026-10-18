//! `rich banner`, the `--gif` animation controls and `rich shell`
//! (0.0.19, #708 and #715): binary-boundary conveniences, tested through the
//! binary.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn rich(args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("RICH_A11Y")
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn ok(args: &[&str]) -> String {
    let out = rich(args, "", &[]);
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

fn usage_error(args: &[&str]) -> String {
    let out = rich(args, "", &[]);
    assert_eq!(out.status.code(), Some(2), "{args:?}");
    String::from_utf8(out.stderr).unwrap()
}

/// Whether `shell` is installed, for the syntax checks.
fn has(shell: &str) -> bool {
    Command::new(shell)
        .arg("-c")
        .arg("exit 0")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(feature = "art")]
mod banner {
    use super::*;

    #[test]
    fn draws_the_standard_font() {
        let out = ok(&["banner", "Hi"]);
        assert!(out.starts_with(" _   _ _ \n| | | (_)\n"), "{out}");
        assert_eq!(out.lines().count(), 6);
        // Text from stdin, and `-`, are the same banner.
        let piped = rich(&["banner"], "Hi\n", &[]);
        assert_eq!(String::from_utf8(piped.stdout).unwrap(), out);
        let dash = rich(&["banner", "-"], "Hi", &[]);
        assert_eq!(String::from_utf8(dash.stdout).unwrap(), out);
    }

    #[test]
    fn aligns_within_the_width() {
        let left = ok(&["banner", "-w", "40", "Hi"]);
        let center = ok(&["banner", "-w", "40", "--align", "center", "Hi"]);
        let right = ok(&["banner", "-w", "40", "--align", "right", "Hi"]);
        let lead = |text: &str| {
            let line = text.lines().next().unwrap();
            line.len() - line.trim_start().len()
        };
        assert_eq!(lead(&left), 1);
        assert!(lead(&center) > 10, "{center}");
        assert!(lead(&right) > lead(&center), "{right}");
    }

    #[test]
    fn fits_the_width_three_ways() {
        let wrapped = ok(&["banner", "-w", "30", "Hello world"]);
        assert_eq!(wrapped.lines().count(), 12, "{wrapped}");
        let plain = ok(&["banner", "-w", "30", "--fit", "plain", "Hello world"]);
        assert_eq!(plain, "Hello world\n");
        let wide = ok(&["banner", "-w", "30", "--fit", "none", "Hello world"]);
        assert_eq!(wide.lines().count(), 6);
        assert!(wide.lines().any(|line| line.len() > 30), "{wide}");
    }

    #[test]
    fn gradients_and_styles_colour_the_strokes() {
        let env = [("TERM", "xterm-256color"), ("COLORTERM", "truecolor")];
        let run = |args: &[&str]| {
            let out = rich(args, "", &env);
            assert!(out.status.success());
            String::from_utf8(out.stdout).unwrap()
        };
        let plain = run(&["banner", "Hi"]);
        assert!(!plain.contains('\x1b'), "piped output stays plain");
        let forced = run(&[
            "banner",
            "--force-terminal",
            "--gradient",
            "#ff0000,#0000ff",
            "Hi",
        ]);
        assert!(forced.contains("\x1b[38;2;255;0;0m"), "{forced:?}");
        assert!(forced.contains("\x1b[38;2;0;0;255m"), "{forced:?}");
        let preset = run(&["banner", "--force-terminal", "--gradient", "ocean", "Hi"]);
        assert!(preset.contains("38;2;33;147;176"), "{preset:?}");
        let styled = run(&["banner", "--force-terminal", "--style", "bold red", "Hi"]);
        assert!(styled.contains("\x1b[1;31m"), "{styled:?}");
        // --no-color drops the colour but keeps the banner.
        let mono = run(&[
            "banner",
            "--force-terminal",
            "--no-color",
            "--gradient",
            "rainbow",
            "Hi",
        ]);
        assert!(!mono.contains("38;2"), "{mono:?}");
        assert!(mono.contains("|_|"), "{mono:?}");
    }

    #[test]
    fn loads_a_font_file_and_lists_fonts() {
        let dir = tempfile::tempdir().unwrap();
        // A two-row font with `A` and a space: the FIGfont header, then the
        // 95 printable ASCII characters, each `height` rows ending in `@`.
        let mut font = String::from("flf2a$ 2 1 8 0 0\n");
        for code in 32..127u8 {
            let glyph = if code == b'A' {
                ["/\\@", "||@@"]
            } else {
                [" @", " @@"]
            };
            font.push_str(&format!("{}\n{}\n", glyph[0], glyph[1]));
        }
        let path = dir.path().join("tiny.flf");
        std::fs::write(&path, &font).unwrap();
        let out = ok(&["banner", "--font", path.to_str().unwrap(), "A"]);
        assert_eq!(out, "/\\\n||\n");
        // By name, through $FIGLET_FONTDIR.
        let by_name = rich(
            &["banner", "--font", "tiny", "A"],
            "",
            &[("FIGLET_FONTDIR", dir.path().to_str().unwrap())],
        );
        assert_eq!(String::from_utf8(by_name.stdout).unwrap(), out);
        let listed = rich(
            &["banner", "--list-fonts"],
            "",
            &[("FIGLET_FONTDIR", dir.path().to_str().unwrap())],
        );
        let listed = String::from_utf8(listed.stdout).unwrap();
        assert!(listed.starts_with("standard (bundled)\n"), "{listed}");
        assert!(listed.contains("tiny ("), "{listed}");
        // A missing font is an input error; a broken one a data error.
        let missing = rich(&["banner", "--font", "no-such-font", "A"], "", &[]);
        assert_eq!(missing.status.code(), Some(3));
        let broken = dir.path().join("broken.flf");
        std::fs::write(&broken, "not a font").unwrap();
        let broken = rich(
            &["banner", "--font", broken.to_str().unwrap(), "A"],
            "",
            &[],
        );
        assert_eq!(broken.status.code(), Some(4));
        assert!(String::from_utf8_lossy(&broken.stderr).contains("FIGfont"));
    }

    #[test]
    fn rejects_bad_options() {
        for (args, needle) in [
            (&["banner", "--gradient", "red", "x"][..], "two or more"),
            (&["banner", "--gradient", "red,nope", "x"], "not a colour"),
            (&["banner", "--align", "middle", "x"], "--align"),
            (&["banner", "--fit", "squash", "x"], "--fit"),
            (&["banner", "--width", "0", "x"], "--width"),
            (&["banner", "--width", "10001", "x"], "from 1 to 10000"),
            (
                &["banner", "--width", "18446744073709551615", "x"],
                "from 1 to 10000",
            ),
            (&["banner", "--direction", "up", "x"], "--direction"),
            (&["banner", "--bogus", "x"], "unknown option"),
        ] {
            let err = usage_error(args);
            assert!(err.contains(needle), "{args:?}: {err}");
        }
        let empty = rich(&["banner"], "  \n", &[]);
        assert_eq!(empty.status.code(), Some(2));
    }

    #[test]
    fn help_lists_the_options() {
        let help = ok(&["banner", "--help"]);
        for option in [
            "--font",
            "--list-fonts",
            "--gradient",
            "--direction",
            "--align",
            "--width",
            "--fit",
            "--force-terminal",
        ] {
            assert!(help.contains(option), "{option}: {help}");
        }
        // `rich -p banner` still prints the word.
        assert_eq!(ok(&["-p", "banner"]), "banner\n");
    }
}

#[cfg(feature = "art")]
mod gif_controls {
    use super::*;
    use std::time::{Duration, Instant};

    const BALL: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rich-art/examples/assets/ball.gif"
    );

    /// Play on a forced terminal; returns the output and how long it took.
    fn play(extra: &[&str]) -> (Output, Duration) {
        let mut args = vec!["--gif", BALL, "--width", "12", "--no-color"];
        args.extend(extra);
        let start = Instant::now();
        let out = rich(&args, "", &[("RS_RICH_BATCH_TERMINAL", "1")]);
        (out, start.elapsed())
    }

    #[test]
    fn max_frames_and_speed_end_an_endless_loop() {
        let (out, took) = play(&["--loop", "0", "--max-frames", "3", "--speed", "4"]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(took < Duration::from_secs(10), "took {took:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        // Live hides the cursor and shows it again.
        assert!(text.contains("\x1b[?25l") && text.contains("\x1b[?25h"));
    }

    #[test]
    fn max_duration_ends_an_endless_loop() {
        let (out, took) = play(&["--loop", "0", "--max-duration", "0.4", "--fps", "10"]);
        assert!(out.status.success());
        assert!(took < Duration::from_secs(10), "took {took:?}");
        let (out, took) = play(&["--loop", "0", "--max-duration", "0.4", "--speed", "2"]);
        assert!(out.status.success());
        assert!(took < Duration::from_secs(10), "took {took:?}");
    }

    #[test]
    fn a_slower_speed_takes_longer() {
        let (_, fast) = play(&["--max-frames", "4", "--speed", "8"]);
        let (_, slow) = play(&["--max-frames", "4", "--speed", "0.5"]);
        assert!(slow > fast, "fast {fast:?}, slow {slow:?}");
    }

    #[test]
    fn piped_output_is_the_first_frame_whatever_the_controls() {
        let base = ok(&["--gif", BALL, "--width", "12", "--no-color"]);
        let controlled = ok(&[
            "--gif",
            BALL,
            "--width",
            "12",
            "--no-color",
            "--speed",
            "3",
            "--fps",
            "5",
            "--max-frames",
            "2",
            "--max-duration",
            "1",
        ]);
        assert_eq!(base, controlled);
    }

    #[test]
    fn controls_validate_and_need_gif() {
        for args in [
            &["--gif", BALL, "--speed", "0"][..],
            &["--gif", BALL, "--speed", "fast"],
            &["--gif", BALL, "--fps", "-1"],
            &["--gif", BALL, "--max-frames", "0"],
            &["--gif", BALL, "--max-duration", "NaN"],
            &["--gif", BALL, "--max-duration", "1e300"],
            &["--gif", BALL, "--fps", "1e-320"],
            &["--gif", BALL, "--speed", "1e-300"],
            &["--gif", BALL, "--speed"],
        ] {
            usage_error(args);
        }
        for flag in ["--speed", "--fps", "--max-frames", "--max-duration"] {
            let err = usage_error(&["-p", "x", flag, "2"]);
            assert!(err.contains("only has an effect with --gif"), "{err}");
        }
    }
}

mod shell {
    use super::*;

    #[test]
    fn status_decorates_a_finished_command() {
        let failed = ok(&[
            "shell",
            "status",
            "--exit",
            "101",
            "--duration",
            "4.2",
            "--command",
            "cargo test | tee log",
        ]);
        assert_eq!(failed, "✖ error $ cargo test | tee log  exit 101  4.2s\n");
        let passed = ok(&[
            "shell",
            "status",
            "--exit=0",
            "--duration=1500ms",
            "--command=make",
        ]);
        assert_eq!(passed, "✔ ok $ make  1.5s\n");
        let ascii = ok(&[
            "shell",
            "status",
            "--exit",
            "1",
            "--command",
            "false",
            "--symbols",
            "ascii",
        ]);
        assert!(
            ascii.starts_with("[FAIL]") || ascii.starts_with("["),
            "{ascii}"
        );
        // Quick successes stay quiet under --min-duration; failures do not.
        let quiet = ok(&[
            "shell",
            "status",
            "--exit",
            "0",
            "--duration",
            "1",
            "--min-duration",
            "2",
        ]);
        assert_eq!(quiet, "");
        let loud = ok(&[
            "shell",
            "status",
            "--exit",
            "2",
            "--duration",
            "1",
            "--min-duration",
            "2",
        ]);
        assert!(loud.contains("exit 2"), "{loud}");
        // Without --command, the line comes from RICH_SHELL_COMMAND (how
        // the PowerShell hook hands it over, unsplit).
        let out = rich(
            &["shell", "status", "--exit", "1"],
            "",
            &[("RICH_SHELL_COMMAND", "echo \"a b\" 'c'")],
        );
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("$ echo \"a b\" 'c'  exit 1"), "{text}");
        let out = rich(
            &["shell", "status", "--exit", "1", "--command", "given"],
            "",
            &[("RICH_SHELL_COMMAND", "ignored")],
        );
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(text.contains("$ given"), "{text}");
    }

    #[test]
    fn durations_out_of_range_are_usage_errors() {
        for args in [
            &["shell", "status", "--exit", "0", "--duration", "1e300"][..],
            &["shell", "status", "--exit", "0", "--duration", "1e20m"],
            &["shell", "status", "--exit", "0", "--min-duration", "1e10"],
            &["shell", "init", "bash", "--min-duration", "1e300"],
        ] {
            let err = usage_error(args);
            assert!(err.contains("at most 10^9 seconds"), "{args:?}: {err}");
        }
        assert!(ok(&["shell", "status", "--exit", "1", "--duration", "1e9"]).contains("exit 1"));
    }

    #[test]
    fn status_follows_rich_a11y() {
        let out = rich(
            &["shell", "status", "--exit", "0", "--command", "ls"],
            "",
            &[("RICH_A11Y", "word-symbols")],
        );
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains('✔'), "{text}");
    }

    #[test]
    fn link_prints_osc8_or_plain() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.txt");
        std::fs::write(&file, "x").unwrap();
        let path = file.to_str().unwrap();
        assert_eq!(ok(&["shell", "link", path]), format!("{path}\n"));
        let linked = ok(&["shell", "link", "--links", "always", path]);
        assert!(
            linked.contains(&format!("\x1b]8;;file://{path}\x1b\\{path}")),
            "{linked:?}"
        );
        let located = ok(&["shell", "link", "--links", "always", &format!("{path}:3")]);
        assert!(located.contains(&format!("file://{path}#3")), "{located:?}");
        let never = ok(&["shell", "link", "--links", "never", path]);
        assert_eq!(never, format!("{path}\n"));
        let editor = ok(&[
            "shell",
            "link",
            "--links",
            "always",
            "--editor",
            "vscode://file{path}:{line}",
            path,
        ]);
        assert!(
            editor.contains(&format!("vscode://file{path}:1")),
            "{editor:?}"
        );
        // Escaped for a prompt: every sequence is marked zero-width.
        let bash = ok(&[
            "shell", "link", "--links", "always", "--prompt", "bash", path,
        ]);
        assert!(bash.starts_with("\u{1}\x1b]8;;"), "{bash:?}");
        assert_eq!(bash.matches('\u{1}').count(), 2, "{bash:?}");
        assert_eq!(bash.matches('\u{2}').count(), 2, "{bash:?}");
        let zsh = ok(&[
            "shell", "link", "--links", "always", "--prompt", "zsh", path,
        ]);
        assert!(zsh.starts_with("%{\x1b]8;;"), "{zsh:?}");
        // zsh expands `%` inside `%{…%}` as well: a URL's escapes are doubled.
        let spaced = dir.path().join("a b.txt");
        std::fs::write(&spaced, "x").unwrap();
        let zsh = ok(&[
            "shell",
            "link",
            "--links",
            "always",
            "--prompt",
            "zsh",
            spaced.to_str().unwrap(),
        ]);
        assert!(zsh.contains("a%%20b.txt\x1b"), "{zsh:?}");
        assert!(!zsh.contains("a%20b"), "{zsh:?}");
        // With no path, the working directory, under ~ with --tilde.
        let mut cwd = Command::new(env!("CARGO_BIN_EXE_rich"));
        let cwd_out = cwd
            .args(["shell", "link", "--tilde"])
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(cwd_out.stdout).unwrap(), "~\n");
        // A $HOME reached through a symlink (macOS's /var is /private/var).
        #[cfg(unix)]
        {
            let link = dir.path().join("home-link");
            std::os::unix::fs::symlink(dir.path(), &link).unwrap();
            let linked_out = Command::new(env!("CARGO_BIN_EXE_rich"))
                .args(["shell", "link", "--tilde"])
                .current_dir(dir.path())
                .env("HOME", &link)
                .output()
                .unwrap();
            assert_eq!(String::from_utf8(linked_out.stdout).unwrap(), "~\n");
        }
    }

    fn init(shell: &str, extra: &[&str]) -> String {
        let mut args = vec!["shell", "init", shell];
        args.extend(extra);
        ok(&args)
    }

    #[test]
    fn init_prints_a_script_for_each_shell() {
        for shell in ["bash", "zsh", "fish", "powershell", "pwsh"] {
            let script = init(shell, &[]);
            assert!(script.contains("133;A"), "{shell}");
            assert!(script.contains("133;C") || shell.contains("pow") || shell == "pwsh");
            assert!(script.contains("133;D;"), "{shell}");
            assert!(script.contains("7;file://"), "{shell}");
            assert!(script.contains("shell status --exit"), "{shell}");
            assert!(script.contains("--min-duration 2"), "{shell}");
            // The completions are `rich completions SHELL`'s.
            let completions = ok(&["completions", shell]);
            assert!(script.contains(completions.trim_end()), "{shell}");
            let bare = init(shell, &["--no-completions", "--no-marks", "--no-status"]);
            assert!(!bare.contains("133;"), "{shell}: {bare}");
            assert!(!bare.contains("shell status"), "{shell}: {bare}");
            assert!(bare.len() < 2_000, "{shell}: {bare}");
        }
        assert!(init("bash", &["--min-duration", "0.5"]).contains("--min-duration 0.5"));
    }

    #[test]
    fn init_scripts_encode_the_directory_and_pass_the_command_safely() {
        // OSC 7: $PWD is percent-encoded, never printed raw or as a format.
        let bash = init("bash", &["--no-completions"]);
        assert!(bash.contains("__rich_shell_url \"$PWD\""), "{bash}");
        assert!(
            bash.contains("\"${HOSTNAME-}\" \"$__rich_shell_path\""),
            "{bash}"
        );
        let zsh = init("zsh", &["--no-completions"]);
        assert!(zsh.contains("__rich_shell_url \"$PWD\""), "{zsh}");
        assert!(
            zsh.contains("print -rn -- $'\\e]7;file://'\"${HOST}${REPLY}\""),
            "{zsh}"
        );
        assert!(!zsh.contains("print -n \""), "{zsh}");
        let fish = init("fish", &["--no-completions"]);
        assert!(
            fish.contains("(string escape --style=url -- $PWD)"),
            "{fish}"
        );
        let pwsh = init("pwsh", &["--no-completions"]);
        assert!(pwsh.contains("[Uri]::EscapeDataString"), "{pwsh}");
        for (shell, script) in [
            ("bash", &bash),
            ("zsh", &zsh),
            ("fish", &fish),
            ("pwsh", &pwsh),
        ] {
            assert!(!script.contains("${PWD}\\a"), "{shell}: {script}");
            assert!(!script.contains("$PWD\n"), "{shell}: {script}");
        }
        // PowerShell hands the command line over in the environment, so
        // legacy argument passing cannot split it.
        assert!(
            pwsh.contains("$env:RICH_SHELL_COMMAND = $last.CommandLine"),
            "{pwsh}"
        );
        assert!(!pwsh.contains("--command"), "{pwsh}");
        // bash names the command only when history saved it.
        assert!(
            bash.contains("[[ $__rich_last != \"${__rich_shell_last-}\" ]]"),
            "{bash}"
        );
        assert!(!bash.contains("@P}"), "{bash}");
        assert!(bash.contains("if (( __rich_saved )) && [["), "{bash}");
        assert!(bash.contains("\"${__rich_cmd[@]}\""), "{bash}");
        assert!(!bash.contains("--command \"$__rich_cmd\""), "{bash}");
    }

    /// The encoders themselves, run in each shell that is installed.
    #[test]
    fn the_directory_encoders_escape_control_characters() {
        let name = "/tmp/a b\x07%\u{e9}\x1b]52;c;x\x07";
        let want = "/tmp/a%20b%07%25%C3%A9%1B%5D52%3Bc%3Bx%07";
        let run = |program: &str, args: &[&str]| -> Option<String> {
            if !has(program) {
                eprintln!("{program} not installed; skipping");
                return None;
            }
            let out = Command::new(program)
                .args(args)
                .env("LANG", "C.UTF-8")
                .env("RICH_TEST_DIR", name)
                .output()
                .unwrap();
            assert!(out.status.success(), "{program}: {out:?}");
            Some(String::from_utf8(out.stdout).unwrap())
        };
        let bash = init("bash", &["--no-completions", "--no-status"]);
        // The scripts need bash 4.4+; macOS's /bin/bash is 3.2.
        let script = format!(
            "(( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] >= 404 )) || {{ printf old; exit 0; }}\n\
             {bash}\n__rich_shell_url \"$RICH_TEST_DIR\"; printf '%s' \"$__rich_shell_path\""
        );
        if let Some(out) = run("bash", &["--norc", "-c", &script]).filter(|out| out != "old") {
            assert_eq!(out, want);
        }
        let zsh = init("zsh", &["--no-completions", "--no-status"]);
        let script = format!("{zsh}\n__rich_shell_url \"$RICH_TEST_DIR\"; print -rn -- $REPLY");
        if let Some(out) = run("zsh", &["-f", "-c", &script]) {
            assert_eq!(out, want);
        }
        if let Some(out) = run(
            "fish",
            &[
                "--no-config",
                "-c",
                "string escape --style=url -- $RICH_TEST_DIR",
            ],
        ) {
            assert_eq!(out.trim_end(), want);
        }
    }

    #[test]
    fn init_scripts_parse_in_their_shells() {
        let dir = tempfile::tempdir().unwrap();
        for (shell, ext, check) in [
            ("bash", "bash", &["bash", "-n"][..]),
            ("zsh", "zsh", &["zsh", "-n"]),
            ("fish", "fish", &["fish", "--no-execute"]),
        ] {
            if !has(check[0]) {
                eprintln!("{} not installed; skipping its syntax check", check[0]);
                continue;
            }
            for extra in [
                &[][..],
                &["--no-completions", "--no-marks"],
                &["--no-status"],
            ] {
                let path = dir.path().join(format!("init.{ext}"));
                std::fs::write(&path, init(shell, extra)).unwrap();
                let status = Command::new(check[0])
                    .args(&check[1..])
                    .arg(&path)
                    .status()
                    .unwrap();
                assert!(status.success(), "{shell} {extra:?}");
            }
        }
    }

    /// The bash integration end to end: an interactive bash reads commands
    /// from stdin, so PS0 and PROMPT_COMMAND run as at a terminal.
    #[test]
    fn bash_integration_reports_each_command() {
        if !has("bash") {
            return;
        }
        let bin = std::path::Path::new(env!("CARGO_BIN_EXE_rich"))
            .parent()
            .unwrap()
            .to_owned();
        let path = format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let tmp = tempfile::tempdir().unwrap();
        let mut child = Command::new("bash")
            .args(["--norc", "--noprofile", "-i"])
            .env("TMPDIR", tmp.path())
            .env("PATH", path)
            .env("HISTFILE", "/dev/null")
            .env("HISTCONTROL", "ignorespace")
            .env_remove("PROMPT_COMMAND")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(
                b"eval \"$(rich shell init bash --no-completions --min-duration 0)\"\n\
                  true\n\
                  (exit 7)\n\
                  \x20(exit 3)\n\
                  mkdir -p \"$TMPDIR/rich odd\"$'\\a' && cd \"$TMPDIR/rich odd\"$'\\a'\n\
                  exit\n",
            )
            .unwrap();
        let out = child.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.contains("133;A") {
            // A bash older than 4.4, or one without prompts: nothing to check.
            return;
        }
        assert!(stdout.contains("\x1b]133;D;0\x07"), "{stdout:?}");
        assert!(stdout.contains("ok $ true"), "{stdout:?}");
        assert!(stdout.contains("\x1b]133;D;7\x07"), "{stdout:?}");
        assert!(stdout.contains("$ (exit 7)  exit 7"), "{stdout:?}");
        // Left out of history (a leading space): no command named, rather
        // than the one before it.
        assert!(stdout.contains("$ -  exit 3"), "{stdout:?}");
        assert_eq!(stdout.matches("(exit 7)").count(), 1, "{stdout:?}");
        // The directory is percent-encoded: its BEL does not end the OSC.
        assert!(stdout.contains("rich%20odd%07\x07"), "{stdout:?}");
        assert!(stdout.contains("\x1b]7;file://"), "{stdout:?}");
        assert!(stderr.contains("\x1b]133;C\x07"), "{stderr:?}");
    }

    #[test]
    fn misuse_is_a_usage_error() {
        for (args, needle) in [
            (&["shell"][..], "needs init, status or link"),
            (&["shell", "launch"], "unknown shell command"),
            (&["shell", "init"], "takes one shell"),
            (&["shell", "init", "tcsh"], "unknown shell"),
            (&["shell", "status"], "needs --exit"),
            (
                &["shell", "status", "--exit", "0", "--tilde"],
                "does not apply",
            ),
            (&["shell", "link", "--links", "maybe"], "--links"),
        ] {
            let err = usage_error(args);
            assert!(err.contains(needle), "{args:?}: {err}");
        }
    }

    #[test]
    fn help_covers_each_command() {
        let help = ok(&["shell", "--help"]);
        for word in ["init", "status", "link"] {
            assert!(help.contains(word), "{help}");
        }
        assert!(ok(&["shell", "init", "--help"]).contains("--no-marks"));
        assert!(ok(&["shell", "status", "--help"]).contains("--min-duration"));
        assert!(ok(&["shell", "link", "--help"]).contains("--prompt"));
        // `rich -p shell` still prints the word.
        assert_eq!(ok(&["-p", "shell"]), "shell\n");
    }
}

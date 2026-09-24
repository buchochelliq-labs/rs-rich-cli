//! Hardening of the commands 0.0.11 adds (`view`, text `diff`, `capture`,
//! `hex`, `unicode`, `inspect`, `bench`, `doctor`, `config`): terminal
//! controls in input and names, bounded reads, capture limits, closed pipes,
//! the working-directory config's trust, and per-command help. Not upstream.
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

/// Run `rich` in `dir` with `stdin`, failing the test if it runs longer
/// than `timeout`. `config` false passes `--no-config`.
fn run_with(
    dir: &Path,
    args: &[&str],
    stdin: &[u8],
    env: &[(&str, &str)],
    config: bool,
    timeout: Duration,
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    if !config {
        command.arg("--no-config");
    }
    command
        .arg("--no-color")
        .args(args)
        .current_dir(dir)
        .env("HOME", dir)
        .env("COLUMNS", "80")
        .env_remove("NO_COLOR")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    let stdin = stdin.to_vec();
    // The binary may exit, or stop reading, before it takes all of stdin.
    std::thread::spawn(move || {
        let _ = input.write_all(&stdin);
    });
    let pid = child.id();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(child.wait_with_output());
    });
    match receiver.recv_timeout(timeout) {
        Ok(output) => output.unwrap(),
        Err(_) => {
            #[cfg(unix)]
            let _ = Command::new("kill").args(["-9", &pid.to_string()]).status();
            panic!("rich {args:?} still running after {timeout:?}");
        }
    }
}

fn run(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    run_with(dir, args, stdin, &[], false, Duration::from_secs(60))
}

fn has_esc(bytes: &[u8]) -> bool {
    bytes.contains(&0x1b)
}

/// Whether `bytes` holds a UTF-8 encoded C1 control (U+0080–U+009F).
fn has_c1(bytes: &[u8]) -> bool {
    bytes
        .windows(2)
        .any(|w| w[0] == 0xc2 && (0x80..=0x9f).contains(&w[1]))
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

const EVIL_PATCH: &[u8] =
    b"--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\x1b]0;T1\x07\n+b\x1b]8;;http://evil\x1b\\\n";

const EVIL_TEXT: &[u8] = b"hi\x1b]52;c;aGVsbG8=\x07 there\x1b]0;TITLE\x07\n";

// --- L1: `rich diff` honours --sanitize ----------------------------------

#[test]
fn diff_sanitize_neutralises_patches_from_files_and_stdin() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("evil.patch"), EVIL_PATCH).unwrap();
    let file = run(dir.path(), &["--sanitize", "diff", "evil.patch"], b"");
    assert!(file.status.success(), "{file:?}");
    assert!(!has_esc(&file.stdout), "{}", text(&file.stdout));
    assert!(
        text(&file.stdout).contains("␛]0;T1"),
        "{}",
        text(&file.stdout)
    );
    let piped = run(dir.path(), &["diff", "-", "--sanitize"], EVIL_PATCH);
    assert!(piped.status.success(), "{piped:?}");
    assert!(!has_esc(&piped.stdout), "{}", text(&piped.stdout));
}

#[test]
fn diff_sanitize_neutralises_c1_controls_and_keeps_ansi_styles() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("old.txt"), "a\nsame\n").unwrap();
    std::fs::write(dir.path().join("new.txt"), "\u{9b}2Jb\nsame\n").unwrap();
    let out = run(
        dir.path(),
        &["--sanitize", "diff", "old.txt", "new.txt"],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!has_c1(&out.stdout), "{}", text(&out.stdout));
    // ANSI captures still compare by style; OSC strings go, and other
    // controls are shown.
    std::fs::write(dir.path().join("a.ansi"), "\x1b[31mred\x1b[0m\n").unwrap();
    std::fs::write(
        dir.path().join("b.ansi"),
        "\x1b[32mred\x1b[0m\x1b]0;T\x07\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("c.ansi"),
        "\x1b[32mred\x1b[0m\x1b7\u{9b}2J\n",
    )
    .unwrap();
    let out = run(dir.path(), &["--sanitize", "diff", "a.ansi", "b.ansi"], b"");
    assert!(out.status.success(), "{out:?}");
    assert!(!has_esc(&out.stdout), "{:?}", text(&out.stdout));
    assert!(text(&out.stdout).contains('~'), "{}", text(&out.stdout));
    let out = run(dir.path(), &["--sanitize", "diff", "a.ansi", "c.ansi"], b"");
    assert!(out.status.success(), "{out:?}");
    assert!(
        !has_esc(&out.stdout) && !has_c1(&out.stdout),
        "{:?}",
        text(&out.stdout)
    );
    assert!(text(&out.stdout).contains("␛7"), "{}", text(&out.stdout));
}

#[cfg(unix)]
#[test]
fn diff_sanitize_neutralises_file_names() {
    let dir = tempfile::tempdir().unwrap();
    let name = "n\x1b]0;FNAME\x07.txt";
    std::fs::write(dir.path().join(name), "a\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "b\n").unwrap();
    let out = run(dir.path(), &["--sanitize", "diff", name, "b.txt"], b"");
    assert!(out.status.success(), "{out:?}");
    assert!(!has_esc(&out.stdout), "{}", text(&out.stdout));
    assert!(
        text(&out.stdout).contains("n␛]0;FNAME␇.txt"),
        "{}",
        text(&out.stdout)
    );
}

// --- L3: `rich view` and text `rich diff` sanitize by default --------------

#[test]
fn view_and_text_diff_neutralise_controls_by_default() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("evil"), EVIL_TEXT).unwrap();
    std::fs::write(dir.path().join("evil.log"), EVIL_TEXT).unwrap();
    std::fs::write(
        dir.path().join("evil.jsonl"),
        "{\"level\": \"info\", \"message\": \"a\\u001b]52;c;aGk=\\u0007b\\u001b]0;T\\u0007\"}\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("evil.patch"), EVIL_PATCH).unwrap();
    for args in [
        &["view", "evil"][..],
        &["view", "evil.log"],
        &["view", "evil.jsonl"],
        &["view", "-"],
        &["diff", "evil.patch"],
        &["--diff", "evil", "evil.log"],
    ] {
        let out = run(dir.path(), args, EVIL_TEXT);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(!has_esc(&out.stdout), "{args:?}: {:?}", text(&out.stdout));
    }
    // --no-sanitize opts out, and the upstream modes keep their bytes.
    for args in [
        &["view", "--no-sanitize", "evil"][..],
        &["diff", "--no-sanitize", "evil.patch"],
        &["evil"],
        &["--print", "-"],
    ] {
        let out = run(dir.path(), args, EVIL_TEXT);
        assert!(out.status.success(), "{args:?}: {out:?}");
        assert!(has_esc(&out.stdout), "{args:?}: {:?}", text(&out.stdout));
    }
}

#[test]
fn a_project_config_cannot_turn_the_view_sanitizing_off() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("evil"), EVIL_TEXT).unwrap();
    std::fs::write(
        dir.path().join("rich.toml"),
        "[defaults]\nsanitize = false\n",
    )
    .unwrap();
    let timeout = Duration::from_secs(60);
    let out = run_with(dir.path(), &["view", "evil"], b"", &[], true, timeout);
    assert!(out.status.success(), "{out:?}");
    assert!(!has_esc(&out.stdout), "{:?}", text(&out.stdout));
    let explain = run_with(
        dir.path(),
        &["config", "explain", "sanitize"],
        b"",
        &[],
        true,
        timeout,
    );
    assert!(
        text(&explain.stdout).contains("sanitize = false in ./rich.toml is ignored"),
        "{explain:?}"
    );
    // The user's own config still can.
    std::fs::create_dir_all(dir.path().join("cfg")).unwrap();
    std::fs::write(
        dir.path().join("cfg/user.toml"),
        "[defaults]\nsanitize = false\n",
    )
    .unwrap();
    let out = run_with(
        dir.path(),
        &["--config", "cfg/user.toml", "view", "evil"],
        b"",
        &[],
        true,
        timeout,
    );
    assert!(has_esc(&out.stdout), "{:?}", text(&out.stdout));
}

// --- L2: `rich capture` titles and --sanitize ------------------------------

#[cfg(unix)]
#[test]
fn capture_titles_are_always_neutralised_and_sanitize_covers_c1_and_casts() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &[
            "capture",
            "--",
            "sh",
            "-c",
            "printf ok",
            "x\x1b]0;TITLE\x07",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!has_esc(&out.stdout), "{:?}", text(&out.stdout));
    assert!(
        text(&out.stdout).contains("␛]0;TITLE␇"),
        "{}",
        text(&out.stdout)
    );

    let script = "printf 'a\\302\\2332J\\n'; printf 'b\\033]0;T\\007\\033]52;c;aGk=\\007\\n'";
    let out = run(
        dir.path(),
        &[
            "--sanitize",
            "capture",
            "--cast",
            "out.cast",
            "--",
            "sh",
            "-c",
            script,
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!has_c1(&out.stdout), "{:?}", text(&out.stdout));
    assert!(!has_esc(&out.stdout), "{:?}", text(&out.stdout));
    let cast = std::fs::read(dir.path().join("out.cast")).unwrap();
    assert!(!has_c1(&cast), "{}", text(&cast));
    assert!(!text(&cast).contains("\\u001b]"), "{}", text(&cast));
}

// --- A6: error messages show controls in paths -----------------------------

#[test]
fn error_messages_show_controls_in_paths() {
    let dir = tempfile::tempdir().unwrap();
    let name = "x\x1b[31mRED\x1b]0;t\x07.txt";
    let mut cases = vec![vec![name], vec!["view", name], vec!["hex", name]];
    if cfg!(feature = "art") {
        cases.push(vec!["--gif", name]);
        cases.push(vec!["--image", name]);
    }
    for args in cases {
        let out = run(dir.path(), &args, b"");
        assert!(!out.status.success(), "{args:?}");
        assert!(!has_esc(&out.stderr), "{args:?}: {:?}", text(&out.stderr));
        assert!(
            text(&out.stderr).contains("x␛[31mRED"),
            "{args:?}: {}",
            text(&out.stderr)
        );
    }
}

// --- L4: bounded reads --------------------------------------------------------

#[cfg(unix)]
#[test]
fn hex_reads_only_the_window_it_shows() {
    let dir = tempfile::tempdir().unwrap();
    let quick = Duration::from_secs(30);
    let zero = run_with(
        dir.path(),
        &["hex", "/dev/zero", "--length", "16"],
        b"",
        &[],
        false,
        quick,
    );
    assert!(
        text(&zero.stdout).starts_with("00000000  00 00"),
        "{zero:?}"
    );
    let random = run_with(
        dir.path(),
        &[
            "hex",
            "/dev/urandom",
            "--offset",
            "0x1000",
            "--length",
            "16",
        ],
        b"",
        &[],
        false,
        quick,
    );
    assert!(text(&random.stdout).starts_with("00001000  "), "{random:?}");
    // Without --length, a limit: an endless input ends, with a notice.
    let endless = run_with(dir.path(), &["hex", "/dev/zero"], b"", &[], false, quick);
    assert!(endless.status.success(), "{endless:?}");
    assert!(
        text(&endless.stderr).contains("showing the first 64 KiB of /dev/zero"),
        "{}",
        text(&endless.stderr)
    );
    // Stdin skips to the offset without keeping what it skips.
    let piped = run_with(
        dir.path(),
        &["hex", "-", "--offset", "2", "--length", "2"],
        b"abcdef",
        &[],
        false,
        quick,
    );
    assert!(
        text(&piped.stdout).starts_with("00000002  63 64"),
        "{piped:?}"
    );
}

/// A sparse gigabyte costs only the bytes shown: under a 1 GB address-space
/// limit, reading the whole file first could not even allocate it.
#[cfg(target_os = "linux")]
#[test]
fn hex_seeks_into_a_sparse_file_instead_of_reading_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = std::fs::File::create(dir.path().join("sparse.bin")).unwrap();
    file.set_len(1 << 30).unwrap();
    let rich = env!("CARGO_BIN_EXE_rich");
    let out = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "ulimit -v 1000000 && exec '{rich}' --no-config --no-color hex sparse.bin \
             --offset 1073741800 --length 16"
        ))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(text(&out.stdout).starts_with("3fffffe8  00 00"), "{out:?}");
}

#[cfg(unix)]
#[test]
fn view_unicode_and_inspect_end_on_endless_input() {
    let dir = tempfile::tempdir().unwrap();
    let quick = Duration::from_secs(60);
    let view = run_with(dir.path(), &["view", "/dev/zero"], b"", &[], false, quick);
    assert!(view.status.success(), "{view:?}");
    assert!(
        text(&view.stdout).starts_with("00000000  00 00"),
        "{view:?}"
    );
    assert!(
        text(&view.stderr).contains("showing the first 64 KiB"),
        "{}",
        text(&view.stderr)
    );
    let unicode = run_with(
        dir.path(),
        &["unicode", "--limit", "3", "/dev/zero"],
        b"",
        &[],
        false,
        quick,
    );
    assert!(unicode.status.success(), "{unicode:?}");
    let inspect = run_with(
        dir.path(),
        &["inspect", "/dev/zero"],
        b"",
        &[],
        false,
        quick,
    );
    assert_eq!(inspect.status.code(), Some(3), "{inspect:?}");
    assert!(
        text(&inspect.stderr).contains("larger than 64 MiB"),
        "{}",
        text(&inspect.stderr)
    );
}

// --- L6, L7, --cast: `rich capture` ending, width and recording ------------

#[cfg(unix)]
#[test]
fn capture_stops_reading_after_the_command_exits() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_with(
        dir.path(),
        &["capture", "--", "sh", "-c", "sleep 30 & echo started"],
        b"",
        &[],
        false,
        Duration::from_secs(15),
    );
    assert!(out.status.success(), "{out:?}");
    assert!(text(&out.stdout).contains("started"), "{out:?}");
    assert!(
        text(&out.stderr).contains("still holds its output open"),
        "{}",
        text(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn capture_stops_a_command_whose_output_never_ends() {
    let dir = tempfile::tempdir().unwrap();
    let out = run_with(
        dir.path(),
        &["capture", "--", "yes"],
        b"",
        &[],
        false,
        Duration::from_secs(120),
    );
    // Stopped by rich, so a signal ended it.
    assert!(!out.status.success(), "{out:?}");
    assert!(
        text(&out.stderr).contains("the output passed 20000 lines"),
        "{}",
        text(&out.stderr)
    );
}

#[cfg(unix)]
#[test]
fn capture_gives_the_command_the_panel_width() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &[
            "capture",
            "-w",
            "50",
            "--cast",
            "w.cast",
            "--",
            "sh",
            "-c",
            "echo cols=$COLUMNS",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(
        text(&out.stdout).contains("cols=46"),
        "{}",
        text(&out.stdout)
    );
    let cast = std::fs::read_to_string(dir.path().join("w.cast")).unwrap();
    let header: serde_json::Value = serde_json::from_str(cast.lines().next().unwrap()).unwrap();
    assert_eq!(header["width"], 46, "{cast}");
}

#[cfg(unix)]
#[test]
fn capture_checks_the_cast_path_before_running_the_command() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &[
            "capture",
            "--cast",
            "missing/dir/out.cast",
            "--",
            "sh",
            "-c",
            "echo ran > marker",
        ],
        b"",
    );
    assert_eq!(out.status.code(), Some(3), "{out:?}");
    assert!(!dir.path().join("marker").exists(), "the command ran");
}

// --- L8: a closed stdout is not a panic -------------------------------------

#[cfg(unix)]
#[test]
fn closed_pipes_do_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    for args in [
        &["config", "reference"][..],
        &["config", "show"],
        &["config", "explain"],
        &["config", "explain", "--help"],
        &["doctor"],
        &["doctor", "--report", "json"],
        &["doctor", "--help"],
        &["bench", "--help"],
        &["hex", "--help"],
    ] {
        let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
            .arg("--no-config")
            .args(args)
            .current_dir(dir.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Close the read end before rich writes anything.
        drop(child.stdout.take());
        let out = child.wait_with_output().unwrap();
        assert_ne!(out.status.code(), Some(101), "{args:?}: {out:?}");
        assert!(!text(&out.stderr).contains("panicked"), "{args:?}: {out:?}");
    }
}

// --- L10: theme files and the working-directory config ---------------------

#[cfg(unix)]
#[test]
fn a_project_config_theme_file_is_ignored_with_a_warning() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = Command::new("mkfifo")
        .arg(dir.path().join("fifo"))
        .status()
        .unwrap();
    assert!(fifo.success());
    std::fs::write(
        dir.path().join("rich.toml"),
        "[defaults]\ntheme_file = 'fifo'\n",
    )
    .unwrap();
    let quick = Duration::from_secs(30);
    let out = run_with(dir.path(), &["--print", "hi"], b"", &[], true, quick);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(text(&out.stdout), "hi\n");
    assert!(
        text(&out.stderr).contains("theme_file in ./rich.toml is ignored"),
        "{}",
        text(&out.stderr)
    );
    let explain = run_with(
        dir.path(),
        &["config", "explain", "theme_file"],
        b"",
        &[],
        true,
        quick,
    );
    assert!(
        text(&explain.stdout).contains("theme_file in ./rich.toml is ignored"),
        "{explain:?}"
    );
    // Named explicitly, a FIFO or a device is refused, not read.
    for path in ["fifo", "/dev/zero"] {
        let out = run_with(
            dir.path(),
            &["--theme-file", path, "--print", "hi"],
            b"",
            &[],
            false,
            quick,
        );
        assert_eq!(out.status.code(), Some(2), "{path}: {out:?}");
        assert!(
            text(&out.stderr).contains("not a regular file"),
            "{}",
            text(&out.stderr)
        );
    }
}

#[test]
fn theme_files_are_size_limited_and_config_errors_name_the_setting() {
    let dir = tempfile::tempdir().unwrap();
    let mut big = String::from("[styles]\n");
    while big.len() <= 1024 * 1024 {
        big.push_str("# padding padding padding padding padding padding padding\n");
    }
    std::fs::write(dir.path().join("big.ini"), big).unwrap();
    let out = run(
        dir.path(),
        &["--theme-file", "big.ini", "--print", "x"],
        b"",
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(text(&out.stderr).contains("larger than 1 MiB"), "{out:?}");

    std::fs::write(
        dir.path().join("user.toml"),
        "[defaults]\ntheme_file = 'missing.ini'\n",
    )
    .unwrap();
    let out = run_with(
        dir.path(),
        &["--config", "user.toml", "--print", "x"],
        b"",
        &[],
        true,
        Duration::from_secs(60),
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let stderr = text(&out.stderr);
    assert!(stderr.contains("user.toml: theme_file"), "{stderr}");
    assert!(!stderr.contains("--theme-file"), "{stderr}");
}

// --- L11: `rich <command> --help` shows that command -------------------------

#[test]
fn command_help_shows_the_command() {
    let dir = tempfile::tempdir().unwrap();
    for (args, expected) in [
        (&["bench", "--help"][..], "rich bench"),
        (&["bench", "compare", "--help"], "--threshold"),
        (&["doctor", "--help"], "rich doctor"),
        (&["hex", "--help"], "--bytes-per-line"),
        (&["view", "--help"], "--no-line-numbers"),
        (&["unicode", "--help"], "--limit"),
        (&["env", "--help"], "--show-secrets"),
        (&["capture", "--help"], "--cast"),
        (&["inspect", "--help"], "--select"),
        (&["ansi", "explain", "--help"], "--escapes-only"),
    ] {
        let out = run(dir.path(), args, b"");
        assert!(out.status.success(), "{args:?}: {out:?}");
        let help = text(&out.stdout);
        assert!(help.contains(expected), "{args:?}: {help}");
        // Not the whole CLI's help.
        assert!(!help.contains("--demo-section"), "{args:?}: {help}");
    }
    let whole = run(dir.path(), &["--help"], b"");
    assert!(text(&whole.stdout).contains("--demo-section"));
}

// --- L13: bench, hex, inspect -----------------------------------------------

fn bench_run(name: &str, mean: f64) -> String {
    serde_json::json!({
        "schema_version": 1,
        "created": "2026-01-01T00:00:00Z",
        "host": null,
        "measurements": [{
            "name": name, "samples": 10, "mean": mean, "median": mean,
            "stddev": 0.0, "p95": mean, "min": mean, "max": mean, "unit": "ns"
        }]
    })
    .to_string()
}

#[test]
fn bench_compare_reports_success_neutralises_names_and_flags_zero_baselines() {
    let dir = tempfile::tempdir().unwrap();
    let name = "render\x1b]0;T\x07";
    std::fs::write(dir.path().join("a.json"), bench_run(name, 100.0)).unwrap();
    std::fs::write(dir.path().join("b.json"), bench_run(name, 101.0)).unwrap();
    let out = run(
        dir.path(),
        &[
            "bench", "compare", "a.json", "b.json", "--report", "json", "-w", "60",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    assert!(!has_esc(&out.stdout), "{:?}", text(&out.stdout));
    assert!(text(&out.stdout)
        .lines()
        .all(|line| line.chars().count() <= 60));
    let report: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["exit_code"], 0, "{report}");

    std::fs::write(dir.path().join("zero.json"), bench_run("x", 0.0)).unwrap();
    std::fs::write(dir.path().join("five.json"), bench_run("x", 5.0)).unwrap();
    let out = run(
        dir.path(),
        &["bench", "compare", "zero.json", "five.json"],
        b"",
    );
    assert_eq!(out.status.code(), Some(5), "{out:?}");
    let table = text(&out.stdout);
    assert!(
        table.contains("+inf%") && table.contains("regression"),
        "{table}"
    );
}

#[test]
fn hex_rejects_bytes_per_line_outside_its_range() {
    let dir = tempfile::tempdir().unwrap();
    for value in ["0", "4097"] {
        let out = run(dir.path(), &["hex", "--bytes-per-line", value, "-"], b"abc");
        assert_eq!(out.status.code(), Some(2), "{value}: {out:?}");
        assert!(text(&out.stderr).contains("--bytes-per-line"), "{out:?}");
    }
    let out = run(
        dir.path(),
        &["hex", "--bytes-per-line", "4096", "-"],
        b"abc",
    );
    assert!(out.status.success(), "{out:?}");
}

#[test]
fn inspect_selectors_are_usage_errors_and_depth_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["inspect", "--select", "$[", "-"], b"{}");
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        text(&out.stderr).contains("invalid --select expression"),
        "{out:?}"
    );
    let deep = format!("{}{}", "[".repeat(5000), "]".repeat(5000));
    std::fs::write(dir.path().join("deep.json"), deep).unwrap();
    let out = run(dir.path(), &["inspect", "deep.json"], b"");
    assert_eq!(out.status.code(), Some(4), "{out:?}");
    assert!(text(&out.stderr).contains("too deeply nested"), "{out:?}");
}

/// On a terminal that Sixel detection does not recognise, the error says so
/// and how to force Sixel, instead of blaming a redirect.
#[cfg(all(target_os = "linux", feature = "art"))]
#[test]
fn sixel_on_an_unrecognised_terminal_says_how_to_force_it() {
    if Command::new("script").arg("--version").output().is_err() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let image =
        rich_art::image::RgbaImage::from_pixel(4, 4, rich_art::image::Rgba([255, 0, 0, 255]));
    image.save(dir.path().join("red.png")).unwrap();
    let rich = env!("CARGO_BIN_EXE_rich");
    let out = Command::new("script")
        .args(["-qec"])
        .arg(format!(
            "'{rich}' --no-config image red.png --image-mode sixel"
        ))
        .arg("/dev/null")
        .current_dir(dir.path())
        .env("TERM", "xterm")
        .env_remove("RICH_SIXEL")
        .env_remove("RICH_GRAPHICS")
        .env_remove("COLORTERM")
        .env_remove("TERM_PROGRAM")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let transcript = text(&out.stdout);
    assert!(transcript.contains("RICH_SIXEL=1"), "{transcript}");
    assert!(transcript.contains("RICH_GRAPHICS=sixel"), "{transcript}");
    assert!(
        !transcript.contains("when redirecting output"),
        "{transcript}"
    );
}

//! `rich choose`, `filter`, `input`, `confirm` and `pager` (#493, #494): in
//! a real PTY, the way scripts use them, and piped, the way they degrade.
//!
//! The PTY tests run a line of `sh` in the terminal: the command's answer
//! captured with `$(…)` or its stdin piped, then `echo` of what came back
//! and `stty -a`, which shows raw mode left on as `-icanon -echo`.
#![cfg(feature = "interact")]

use std::io::Write;
use std::process::{Command, Stdio};

/// `(stdout, stderr, exit code)` for `rich ARGS` with `stdin` piped: no
/// terminal anywhere.
fn piped(args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("CI")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // `rich` may refuse its arguments and exit before reading stdin; the
    // write then fails with a broken pipe, which is not the test's concern.
    let written = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

/// [`piped`] with bytes in and out.
fn piped_bytes(args: &[&str], stdin: &[u8]) -> (Vec<u8>, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("CI")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let data = stdin.to_vec();
    // On a thread: `rich` may stop reading (a limit) while this writes.
    let writer = std::thread::spawn(move || {
        let _ = input.write_all(&data);
    });
    let output = child.wait_with_output().unwrap();
    writer.join().unwrap();
    (
        output.stdout,
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

#[test]
fn without_a_terminal_the_end_of_input_takes_the_default() {
    assert_eq!(piped(&["confirm", "--default", "yes"], "").2, 0);
    assert_eq!(piped(&["confirm", "--default", "no"], "").2, 1);
    let (out, _, code) = piped(&["input", "--default", "d"], "");
    assert_eq!((out.as_str(), code), ("d\n", 0));
    let (out, _, code) = piped(&["choose", "--selected", "b", "a", "b"], "");
    assert_eq!((out.as_str(), code), ("b\n", 0));
    let (out, _, code) = piped(&["choose", "--selected", "b", "a", "b"], "\n");
    assert_eq!(
        (out.as_str(), code),
        ("b\n", 0),
        "an empty line takes it too"
    );
    // A required input refuses the empty line it is given.
    let (out, err, code) = piped(&["input", "--required"], "\n");
    assert_eq!((out.as_str(), code), ("", 3), "{err}");
    assert!(err.contains("an answer is required"), "{err}");
    // Without a default, no answer (3), not "no" or "cancelled" (1).
    for args in [&["confirm"][..], &["input"], &["choose", "a", "b"]] {
        let (out, err, code) = piped(args, "");
        assert_eq!((out.as_str(), code), ("", 3), "{args:?}: {err}");
        assert!(err.contains("no default"), "{args:?}: {err}");
    }
}

#[test]
fn invalid_utf8_is_read_lossily_and_paged_unchanged() {
    let (out, err, code) = piped_bytes(&["choose", "--selected", "a\u{fffd}b"], b"a\xffb\nc\n");
    assert_eq!(
        (out.as_slice(), code),
        ("a\u{fffd}b\n".as_bytes(), 0),
        "{err}"
    );
    let (out, _, code) = piped_bytes(&["filter", "--value", "c"], b"a\xffb\nc\xfe\n");
    assert_eq!((out.as_slice(), code), ("c\u{fffd}\n".as_bytes(), 0));
    // The pager without a terminal passes the bytes through untouched.
    let latin1 = b"caf\xe9 \x1b[1mbold\x1b[0m\n\xff\n";
    let (out, _, code) = piped_bytes(&["pager"], latin1);
    assert_eq!((out.as_slice(), code), (&latin1[..], 0));
    let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("latin1-pager.txt");
    std::fs::write(&file, latin1).unwrap();
    let (out, _, code) = piped_bytes(&["pager", file.to_str().unwrap()], b"");
    assert_eq!((out.as_slice(), code), (&latin1[..], 0));
}

#[test]
fn standard_input_is_bounded() {
    let lines = "y\n".repeat(1_000_001);
    let (out, err, code) = piped_bytes(&["choose", "--selected", "y"], lines.as_bytes());
    assert_eq!((out.as_slice(), code), (&b""[..], 3), "{err}");
    assert!(err.contains("1000000 line limit"), "{err}");
    let big = vec![b'x'; 64 * 1024 * 1024 + 1];
    let (out, err, code) = piped_bytes(&["filter"], &big);
    assert_eq!((out.as_slice(), code), (&b""[..], 3), "{err}");
    assert!(err.contains("64 MiB limit"), "{err}");
}

#[test]
fn filter_without_a_terminal_prints_the_matching_lines_best_first() {
    let (out, _, code) = piped(
        &["filter", "--value", "bry"],
        "apple\nblueberry\ncherry\nbranberry\n",
    );
    assert_eq!(code, 0);
    assert_eq!(out, "blueberry\nbranberry\n");
    let (out, _, _) = piped(&["filter"], "a\nb\n");
    assert_eq!(out, "a\nb\n", "no query keeps every line, in order");
}

#[test]
fn filter_keeps_blank_lines_as_grep_would() {
    let (out, _, code) = piped(&["filter"], "a\n\n  \nb\n");
    assert_eq!((out.as_str(), code), ("a\n\n  \nb\n", 0));
}

#[test]
fn multi_from_stdin_with_nothing_selected_is_no_answer() {
    let (out, err, code) = piped(&["choose", "--multi"], "apple\nbanana\n");
    assert_eq!((out.as_str(), code), ("", 3));
    assert!(err.contains("no default"), "{err}");
    let (out, _, code) = piped(
        &["choose", "--multi", "--selected", "nope"],
        "apple\nbanana\n",
    );
    assert_eq!(
        (out.as_str(), code),
        ("", 3),
        "an unknown --selected marks nothing"
    );
    let (out, _, code) = piped(
        &["choose", "--multi", "--selected", "banana"],
        "apple\nbanana\n",
    );
    assert_eq!((out.as_str(), code), ("banana\n", 0));
}

#[test]
fn the_global_report_options_are_honoured() {
    for report in [&["--report", "json"][..], &["--machine-json"]] {
        let args: Vec<&str> = report
            .iter()
            .copied()
            .chain(["choose", "--selected", "b"])
            .collect();
        let (out, err, code) = piped(&args, "a\nb\n");
        assert_eq!((out.as_str(), code), ("b\n", 0), "{report:?}: {err}");
        let envelope: serde_json::Value = serde_json::from_str(err.trim()).expect(&err);
        assert_eq!(envelope["ok"], true, "{err}");
    }
    let (_, err, code) = piped(&["--report", "json", "choose"], "a\nb\n");
    assert_eq!(code, 3);
    let envelope: serde_json::Value = serde_json::from_str(err.trim()).expect(&err);
    assert_eq!(envelope["ok"], false, "{err}");
    for report in [&["--report", "xml"][..], &["--report=json"]] {
        let args: Vec<&str> = report.iter().copied().chain(["choose", "a"]).collect();
        let (_, err, code) = piped(&args, "");
        assert_eq!(code, 2, "{report:?}: {err}");
    }
}

#[test]
fn choose_from_stdin_without_a_terminal_answers_with_selected() {
    let (out, _, code) = piped(&["choose", "--selected", "banana"], "apple\nbanana\n");
    assert_eq!((out.as_str(), code), ("banana\n", 0));
    let (out, err, code) = piped(&["choose"], "apple\nbanana\n");
    assert_eq!((out.as_str(), code), ("", 3));
    assert!(err.contains("no default"), "{err}");
}

#[test]
fn choose_from_arguments_asks_line_by_line_without_a_terminal() {
    let (out, err, code) = piped(&["choose", "red", "green"], "2\n");
    assert_eq!((out.as_str(), code), ("green\n", 0));
    assert!(err.contains("1) red"), "the list goes to stderr: {err}");
}

#[test]
fn confirm_input_and_pager_degrade() {
    assert_eq!(piped(&["confirm", "Go?"], "y\n").2, 0);
    assert_eq!(piped(&["confirm", "Go?"], "n\n").2, 1);
    assert_eq!(piped(&["confirm", "--default", "no"], "\n").2, 1);
    let (out, _, code) = piped(&["input", "--prompt", "Name"], "Ada\n");
    assert_eq!((out.as_str(), code), ("Ada\n", 0));
    let (out, _, code) = piped(&["input", "--default", "Bob"], "\n");
    assert_eq!((out.as_str(), code), ("Bob\n", 0));
    let (out, _, code) = piped(&["pager"], "\x1b[1mbold\x1b[0m\nplain\n");
    assert_eq!(code, 0);
    assert_eq!(
        out, "\x1b[1mbold\x1b[0m\nplain\n",
        "the content, as it came"
    );
}

#[test]
fn usage_errors_exit_2() {
    for args in [
        &["choose", "--nope"][..],
        &["input", "extra"],
        &["confirm", "--default", "maybe"],
        &["pager", "a", "b"],
    ] {
        let (_, err, code) = piped(args, "x\n");
        assert_eq!(code, 2, "{args:?}: {err}");
    }
    let (_, err, code) = piped(&["choose"], "");
    assert_eq!(code, 2);
    assert!(err.contains("no items"), "{err}");
}

#[cfg(unix)]
#[test]
fn items_that_are_not_utf8_are_shown_as_text() {
    use std::os::unix::ffi::OsStrExt;
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "choose", "--selected"])
        .arg(std::ffi::OsStr::from_bytes(b"caf\xe9"))
        .arg("tea")
        .arg(std::ffi::OsStr::from_bytes(b"caf\xe9"))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        out,
        "caf\u{fffd}\n",
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!out.contains('\0'));
}

#[test]
fn write_without_a_terminal_prints_standard_input() {
    let (out, _, code) = piped(&["write"], "first\nsecond\n");
    assert_eq!((out.as_str(), code), ("first\nsecond\n", 0));
    let (out, _, code) = piped(&["write", "--char-limit", "7"], "first\nsecond\n");
    assert_eq!((out.as_str(), code), ("first\ns\n", 0));
    let big = vec![b'x'; 64 * 1024 * 1024 + 1];
    let (out, err, code) = piped_bytes(&["write"], &big);
    assert_eq!((out.as_slice(), code), (&b""[..], 3), "{err}");
    assert!(err.contains("64 MiB limit"), "{err}");
}

#[test]
fn write_without_a_terminal_answers_value_and_counts_graphemes() {
    // No input at all: --value is the answer, as a default is elsewhere.
    let (out, err, code) = piped(&["write", "--value", "draft"], "");
    assert_eq!((out.as_str(), code), ("draft\n", 0), "{err}");
    // An empty line is an answer: the empty text.
    let (out, _, code) = piped(&["write", "--value", "draft"], "\n");
    assert_eq!((out.as_str(), code), ("\n", 0));
    // The limit counts grapheme clusters and never cuts one.
    let (out, _, code) = piped(&["write", "--char-limit", "1"], "e\u{301}x\n");
    assert_eq!((out.as_str(), code), ("e\u{301}\n", 0));
    let family = "👨\u{200d}👩\u{200d}👧";
    let (out, _, code) = piped(&["write", "--char-limit", "3"], &family.repeat(4));
    assert_eq!((out, code), (format!("{}\n", family.repeat(3)), 0));
    // A `\r\n` line ending ends the text too.
    let (out, _, code) = piped(&["write"], "a\tb\r\n");
    assert_eq!((out.as_str(), code), ("a\tb\n", 0));
}

#[test]
fn line_prompts_show_controls_in_the_header_as_text() {
    let header = "H\x1b]0;PWNED\x07\x1b[2J";
    for (args, stdin) in [
        (&["color", "--header", header][..], "red\n"),
        (&["write", "--header", header], ""),
        (&["input", "--prompt", header], "x\n"),
        (&["confirm", header], "y\n"),
        (&["choose", "--header", header, "a", "b"], "1\n"),
    ] {
        let (_, err, code) = piped(args, stdin);
        assert_eq!(code, 0, "{args:?}: {err}");
        assert!(
            !err.contains('\x1b') && !err.contains('\x07'),
            "{args:?}: {err:?}"
        );
    }
    let (_, err, _) = piped(&["color", "--header", header], "red\n");
    assert!(err.contains("H␛]0;PWNED␇␛[2J"), "{err:?}");
}

#[test]
fn asset_refuses_a_selected_name_it_does_not_offer() {
    for stdin in ["", "rocket\n"] {
        let (out, err, code) = piped(&["asset", "--selected", "nosuch"], stdin);
        assert_eq!((out.as_str(), code), ("", 2), "{err}");
        assert!(
            err.contains("--selected: \"nosuch\" is not an emoji name"),
            "{err}"
        );
    }
    let (_, err, code) = piped(&["asset", "--kind", "box", "--selected", "dots"], "");
    assert_eq!(code, 2, "{err}");
    let (out, _, code) = piped(&["asset", "--selected", ":rocket:"], "");
    assert_eq!((out.as_str(), code), ("🚀\n", 0));
}

#[test]
fn asset_help_names_what_takes_a_box_style() {
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "asset", "--help"])
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(!help.contains("--box"), "rich has no --box: {help}");
    assert!(help.contains("--panel"), "{help}");
    // Every box name `rich asset --kind box` offers, --panel takes.
    let names = rich_interact::components::BOX_STYLES
        .iter()
        .map(|(name, _)| *name);
    for name in names {
        let (out, err, code) = piped(&["--panel", name, "--print", "x"], "");
        assert!(code == 0 && !out.is_empty(), "--panel {name}: {err}");
    }
    let (_, err, code) = piped(&["--panel", "nope", "--print", "x"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("heavy_head"), "{err}");
}

#[test]
fn file_without_a_terminal_prints_selected_or_has_no_answer() {
    let (out, _, code) = piped(&["file", "--selected", "notes.md"], "");
    assert_eq!((out.as_str(), code), ("notes.md\n", 0));
    let (out, err, code) = piped(&["file"], "");
    assert_eq!((out.as_str(), code), ("", 3), "{err}");
    assert!(err.contains("no default"), "{err}");
    let (_, err, code) = piped(&["file", "no-such-directory"], "");
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("not a directory"), "{err}");
}

#[test]
fn color_and_asset_ask_line_by_line_without_a_terminal() {
    let (out, err, code) = piped(&["color", "--format", "rgb"], "#ff8800\n");
    assert_eq!((out.as_str(), code), ("rgb(255,136,0)\n", 0), "{err}");
    assert!(err.contains("#rrggbb"), "the prompt goes to stderr: {err}");
    let (out, _, code) = piped(&["color", "--format", "name", "--default", "color(18)"], "");
    assert_eq!((out.as_str(), code), ("dark_blue\n", 0));
    let (out, err, code) = piped(&["color"], "not a colour\n");
    assert_eq!((out.as_str(), code), ("", 3), "{err}");
    let (out, _, code) = piped(&["asset"], "rocket\n");
    assert_eq!((out.as_str(), code), ("🚀\n", 0));
    let (out, _, code) = piped(&["asset", "--kind", "spinner", "--selected", "dots"], "\n");
    assert_eq!((out.as_str(), code), ("dots\n", 0));
    let (out, _, code) = piped(&["asset", "--kind", "box"], "");
    assert_eq!((out.as_str(), code), ("", 3));
}

#[test]
fn color_checks_its_default_and_an_empty_line() {
    // A default that is not a colour is a usage error, before anything is
    // asked, whatever standard input holds.
    for stdin in ["", "\n", "blue\n"] {
        let (out, err, code) = piped(&["color", "--default", "definitely-not-a-color"], stdin);
        assert_eq!((out.as_str(), code), ("", 2), "{stdin:?}: {err}");
        assert!(
            err.contains("--default: \"definitely-not-a-color\" is not a colour"),
            "{err}"
        );
    }
    let (out, err, code) = piped(&["color", "--default", ""], "\n");
    assert_eq!((out.as_str(), code), ("", 2), "{err}");
    // An empty line is the default, or no answer without one.
    let (out, err, code) = piped(&["color", "--default", "red"], "\n");
    assert_eq!((out.as_str(), code), ("#800000\n", 0), "{err}");
    let (out, err, code) = piped(&["color"], "\n");
    assert_eq!((out.as_str(), code), ("", 3), "{err}");
    assert!(err.contains("no default"), "{err}");
}

#[test]
fn the_new_commands_check_their_arguments() {
    for args in [
        &["write", "extra"][..],
        &["write", "--char-limit", "0"],
        &["file", "a", "b"],
        &["file", "--multi"],
        &["color", "--format", "hsl"],
        &["asset", "--kind", "icon"],
        &["asset", "extra"],
        &["input", "--mouse"],
    ] {
        let (_, err, code) = piped(args, "");
        assert_eq!(code, 2, "{args:?}: {err}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn file_prints_a_path_that_is_not_utf8_byte_for_byte() {
    use std::os::unix::ffi::OsStrExt;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("file-non-utf8");
    std::fs::create_dir_all(&dir).unwrap();
    let name = std::ffi::OsStr::from_bytes(b"caf\xe9.txt");
    std::fs::write(dir.join(name), "x").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "file", "--selected"])
        .arg(dir.join(name))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    let mut expected = dir.join(name).as_os_str().as_bytes().to_vec();
    expected.push(b'\n');
    assert_eq!(output.stdout, expected);
}

#[test]
fn explore_without_a_terminal_prints_the_tree() {
    let (out, err, code) = piped(&["explore", "--format", "yaml"], "a: 1\nb: [x]\n");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, "stdin\n├── a: 1\n└── b\n    └── [0]: \"x\"\n");
    let (out, err, code) = piped(&["explore", "-"], r#"{"k": true}"#);
    assert_eq!((out.as_str(), code), ("stdin\n└── k: true\n", 0), "{err}");
}

#[test]
fn explore_reports_bad_documents_and_options() {
    let (_, err, code) = piped(&["explore", "--format", "toml"], "{");
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("<stdin>:1:1: invalid toml"), "{err}");
    let (_, err, code) = piped(&["explore", "--print", "both"], "{}");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("--print"), "{err}");
    let (_, err, code) = piped(&["explore", "a.json", "b.json"], "");
    assert_eq!(code, 2, "{err}");
    let (_, err, code) = piped(&["explore", "/no/such/file.json"], "");
    assert_eq!(code, 3, "{err}");
    assert!(err.contains("could not read"), "{err}");
}

#[test]
fn every_command_has_help() {
    for command in [
        "choose", "filter", "input", "confirm", "pager", "write", "file", "color", "asset",
        "explore",
    ] {
        let (out, _, code) = piped(&[command, "--help"], "");
        assert_eq!(code, 0);
        assert!(out.contains(&format!("rich {command}")), "{out}");
    }
}

#[cfg(unix)]
mod pty {
    use std::io::{Read, Write};
    use std::process::Command;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    struct Pty {
        output: Arc<Mutex<Vec<u8>>>,
        writer: Box<dyn Write + Send>,
        child: Box<dyn portable_pty::Child + Send + Sync>,
    }

    impl Pty {
        /// Run `script` in `sh`, `rich` meaning the built binary.
        fn start(script: &str) -> Pty {
            let pty = native_pty_system()
                .openpty(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            let rich = format!("'{}' --no-config", env!("CARGO_BIN_EXE_rich"));
            let script = format!(
                "{}; stty -a; echo STTY-DONE",
                script.replace("rich ", &format!("{rich} "))
            );
            let mut command = CommandBuilder::new("sh");
            command.args(["-c", &script]);
            command.env("TERM", "xterm-256color");
            command.env_remove("CI");
            command.env_remove("NO_COLOR");
            let child = pty.slave.spawn_command(command).unwrap();
            drop(pty.slave);
            let mut reader = pty.master.try_clone_reader().unwrap();
            let output = Arc::new(Mutex::new(Vec::new()));
            let sink = Arc::clone(&output);
            std::thread::spawn(move || {
                let mut buffer = [0u8; 4096];
                while let Ok(read) = reader.read(&mut buffer) {
                    if read == 0 {
                        break;
                    }
                    sink.lock().unwrap().extend_from_slice(&buffer[..read]);
                }
            });
            let writer = pty.master.take_writer().unwrap();
            std::mem::forget(pty.master);
            Pty {
                output,
                writer,
                child,
            }
        }

        fn text(&self) -> String {
            String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
        }

        /// The screen as a terminal shows it: the painter writes only the
        /// cells that change, so the byte stream need not hold `needle`.
        fn screen(&self) -> String {
            let mut parser = vt100::Parser::new(24, 80, 0);
            parser.process(&self.output.lock().unwrap());
            parser.screen().contents()
        }

        fn wait_for(&self, needle: &str) {
            let end = Instant::now() + Duration::from_secs(30);
            while Instant::now() < end {
                if self.text().contains(needle) || self.screen().contains(needle) {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!(
                "timed out waiting for {needle:?}; screen:\n{}\noutput:\n{:?}",
                self.screen(),
                self.text()
            );
        }

        fn send(&mut self, keys: &str) {
            self.writer.write_all(keys.as_bytes()).unwrap();
            self.writer.flush().unwrap();
        }

        /// The process id a script printed as `PID=…`.
        fn pid(&self) -> String {
            self.wait_for("PID=");
            let end = Instant::now() + Duration::from_secs(10);
            loop {
                let text = self.text();
                let rest = &text[text.find("PID=").unwrap() + 4..];
                let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
                if rest.len() > digits.len() || Instant::now() > end {
                    return digits;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        /// The whole output, once the script is done; the terminal must be
        /// back in cooked mode.
        fn finish(mut self) -> String {
            self.wait_for("STTY-DONE");
            let _ = self.child.wait();
            let text = self.text();
            let stty = &text[text.rfind("speed").expect("stty output")..];
            let flags: Vec<&str> = stty.split_whitespace().collect();
            for flag in ["icanon", "echo", "isig"] {
                assert!(flags.contains(&flag), "{flag:?} not restored:\n{stty}");
            }
            text
        }
    }

    #[test]
    fn choose_answers_a_captured_substitution() {
        let mut pty = Pty::start(r#"x=$(rich choose apple banana cherry); echo "code=$? got=$x""#);
        pty.wait_for("cherry");
        pty.send("\x1b[B");
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=banana"), "{out}");
    }

    #[test]
    fn filter_reads_keys_from_the_terminal_while_stdin_is_the_list() {
        let mut pty = Pty::start(
            r#"x=$(printf 'alpha\nbeta\ngamma\n' | rich filter); echo "code=$? got=$x""#,
        );
        pty.wait_for("gamma");
        pty.send("gam");
        pty.wait_for("gam");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=gamma"), "{out}");
    }

    #[test]
    fn escape_cancels_with_exit_1_and_ctrl_c_interrupts_with_130() {
        let mut pty = Pty::start(r#"x=$(rich choose a b); echo "code=$? got=[$x]""#);
        pty.wait_for("Choose");
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out}");

        let mut pty = Pty::start(r#"rich input --prompt Name; echo "code=$?""#);
        pty.wait_for("Name");
        pty.send("\x03");
        let out = pty.finish();
        assert!(out.contains("code=130"), "{out}");
    }

    #[test]
    fn confirm_exits_by_the_answer() {
        let mut pty = Pty::start(r#"rich confirm 'Deploy?'; echo "code=$?""#);
        pty.wait_for("Deploy?");
        pty.send("n");
        let out = pty.finish();
        assert!(out.contains("code=1"), "{out}");

        let mut pty = Pty::start(r#"rich confirm 'Deploy?'; echo "code=$?""#);
        pty.wait_for("Deploy?");
        pty.send("y");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
    }

    #[test]
    fn input_returns_the_typed_line() {
        let mut pty = Pty::start(r#"x=$(rich input --prompt Name); echo "code=$? got=$x""#);
        pty.wait_for("Name");
        pty.send("Ada");
        pty.wait_for("Ada");
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=Ada"), "{out}");
    }

    #[test]
    fn a_required_input_refuses_an_empty_line() {
        let mut pty = Pty::start(
            r#"x=$(rich input --prompt Project --placeholder rs-rich --required); echo "code=$? got=$x""#,
        );
        pty.wait_for("rs-rich");
        pty.send("\r");
        pty.wait_for("an answer is required");
        pty.send("demo");
        pty.wait_for("demo");
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=demo"), "{out}");
    }

    #[test]
    fn the_pager_counts_the_lines_of_the_file() {
        let mut pty = Pty::start(r#"printf 'one\ntwo\nthree\n' | rich pager; echo "code=$?""#);
        pty.wait_for("all 3 lines");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
    }

    #[test]
    fn pager_pages_piped_text_and_quits() {
        let mut pty = Pty::start(r#"seq 1 200 | rich pager; echo "code=$?""#);
        pty.wait_for("23");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
        assert!(!out.contains("\n200\r\n"), "paged, not printed: {out}");
    }

    /// Whether an SGR parameter list sets a foreground or background.
    fn sets_colour(params: &str) -> bool {
        params.split(';').any(|p| {
            matches!(
                p.parse::<u32>(),
                Ok(30..=38 | 40..=48 | 90..=97 | 100..=107)
            )
        })
    }

    /// The parameters of every SGR sequence (`CSI … m`) in `text`.
    fn sgr_params(text: &str) -> Vec<String> {
        let mut params = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("\x1b[") {
            rest = &rest[start + 2..];
            let end = rest
                .find(|c: char| !(c.is_ascii_digit() || c == ';'))
                .unwrap_or(rest.len());
            if rest[end..].starts_with('m') {
                params.push(rest[..end].to_string());
            }
            rest = &rest[end..];
        }
        params
    }

    #[test]
    fn no_color_paints_without_colour() {
        let mut pty = Pty::start(r#"x=$(rich choose apple banana); echo "got=$x""#);
        pty.wait_for("banana");
        pty.send("\r");
        let coloured = pty.finish();
        assert!(
            sgr_params(&coloured).iter().any(|p| sets_colour(p)),
            "the default paints colour: {coloured:?}"
        );

        let mut pty = Pty::start(r#"x=$(rich --no-color choose apple banana); echo "got=$x""#);
        pty.wait_for("banana");
        pty.send("\r");
        let plain = pty.finish();
        assert!(plain.contains("got=apple"), "{plain}");
        for param in sgr_params(&plain) {
            assert!(
                !sets_colour(&param),
                "colour {param:?} despite --no-color: {plain:?}"
            );
        }
    }

    #[test]
    fn item_labels_cannot_drive_the_terminal() {
        let mut pty = Pty::start(
            r#"x=$(printf 'safe\nevil\033]0;pwned\007\n' | rich choose); echo "code=$? got=$x""#,
        );
        pty.wait_for("pwned");
        pty.send("\r");
        let out = pty.finish();
        assert!(
            !out.contains("\x1b]0;pwned"),
            "the title escape reached the terminal: {out:?}"
        );
        assert!(out.contains("␛]0;pwned"), "shown as text: {out:?}");
        assert!(out.contains("code=0 got=safe"), "{out}");
    }

    #[test]
    fn terminating_signals_give_the_terminal_back() {
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("signal-pager.txt");
        std::fs::write(&file, "first page line\n".repeat(100)).unwrap();
        let pager = format!("rich pager '{}'", file.display());
        for (command, signal, shown, code) in [
            (pager.as_str(), "TERM", "first page line", 143),
            ("rich choose apple banana", "HUP", "banana", 129),
        ] {
            let script = format!(r#"{command} & echo "PID=$!"; wait $!; echo "code=$?""#);
            let pty = Pty::start(&script);
            let pid = pty.pid();
            pty.wait_for(shown);
            // Painted: the modes are on.
            std::thread::sleep(Duration::from_millis(200));
            let killed = Command::new("kill")
                .args([&format!("-{signal}"), &pid])
                .status()
                .unwrap();
            assert!(killed.success());
            let out = pty.finish();
            assert!(out.contains(&format!("code={code}")), "{signal}: {out}");
            let after = &out[out.find(shown).unwrap()..];
            assert!(
                after.contains("\x1b[?25h"),
                "{signal}: cursor hidden: {out:?}"
            );
            if command.contains("pager") {
                for mode in ["\x1b[?1049l", "\x1b[?1000l", "\x1b[?1006l"] {
                    assert!(after.contains(mode), "{signal}: {mode:?} left on: {out:?}");
                }
            } else {
                assert!(
                    after.contains("\x1b[?2004l"),
                    "{signal}: paste left on: {out:?}"
                );
            }
        }
    }

    #[test]
    fn a_pager_search_that_folds_to_other_lengths_does_not_panic() {
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fold-pager.txt");
        std::fs::write(&file, "Ⱥẞ\nİx\n").unwrap();
        let mut pty = Pty::start(&format!(
            r#"rich pager --search ß '{}'; echo "code=$?""#,
            file.display()
        ));
        pty.wait_for("match 1/1");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
        assert!(!out.contains("panicked"), "{out}");
    }

    #[test]
    fn controls_in_paged_content_are_shown_not_sent() {
        let mut pty =
            Pty::start(r#"printf 'G\033cx\nF\302\2332Jy\nM\033\n' | rich pager; echo "code=$?""#);
        pty.wait_for("G␛cx");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
        for raw in ["\x1bcx", "\u{9b}2J", "M\x1b\r"] {
            assert!(!out.contains(raw), "{raw:?} reached the terminal: {out:?}");
        }
    }

    #[test]
    fn ctrl_c_is_prompt_while_a_preview_command_runs() {
        let mut pty = Pty::start(
            r#"x=$(rich choose --preview 'sleep 30; echo {}' a b); echo "code=$? got=[$x]""#,
        );
        pty.wait_for("Choose");
        std::thread::sleep(Duration::from_millis(300));
        let sent = Instant::now();
        pty.send("\x03");
        pty.wait_for("code=");
        assert!(
            sent.elapsed() < Duration::from_secs(5),
            "Ctrl+C took {:?}",
            sent.elapsed()
        );
        let out = pty.finish();
        assert!(out.contains("code=130 got=[]"), "{out}");
    }

    #[test]
    fn a_preview_command_that_hangs_times_out() {
        let mut pty = Pty::start(r#"rich choose --preview 'sleep 30' a b; echo "code=$?""#);
        pty.wait_for("preview timed out");
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1"), "{out}");
    }

    #[test]
    fn ctrl_c_at_a_line_prompt_without_echo_exits_130() {
        // Standard output captured: no terminal to paint on, so the
        // password is read as a line with echo off.
        let mut pty = Pty::start(r#"x=$(rich input --password); echo "code=$? got=[$x]""#);
        pty.wait_for("Input");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("abc");
        pty.send("\x03");
        let out = pty.finish();
        assert!(out.contains("code=130 got=[]"), "{out}");
        assert!(!out.contains("abc"), "echoed: {out}");
    }

    #[test]
    fn a_preview_command_renders_the_focused_item() {
        let mut pty = Pty::start(
            r#"x=$(rich choose --preview 'echo preview of {}' one two); echo "code=$? got=$x""#,
        );
        pty.wait_for("preview of one");
        pty.send("\x1b[B");
        pty.wait_for("preview of two");
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=two"), "{out}");
    }

    #[test]
    fn write_returns_the_lines_typed() {
        let mut pty = Pty::start(r#"x=$(rich write --header Notes); echo "code=$? got=[$x]""#);
        pty.wait_for("Notes");
        pty.send("one\rtwo");
        pty.wait_for("two");
        pty.send("\x04");
        let out = pty.finish();
        assert!(out.contains("code=0 got=[one\r\ntwo]"), "{out:?}");
    }

    #[test]
    fn write_starts_from_piped_input_and_escape_cancels() {
        let mut pty = Pty::start(r#"x=$(echo draft | rich write); echo "code=$? got=[$x]""#);
        pty.wait_for("draft");
        pty.send(" more");
        pty.wait_for("more");
        pty.send("\x04");
        let out = pty.finish();
        assert!(out.contains("code=0 got=[draft more]"), "{out:?}");
        let mut pty = Pty::start(r#"x=$(rich write); echo "code=$? got=[$x]""#);
        pty.wait_for("submit");
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out}");
    }

    fn picker_dir() -> std::path::PathBuf {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("file-picker");
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs").join("guide.md"), "# Guide\n").unwrap();
        std::fs::write(dir.join("README.md"), "# Readme\n").unwrap();
        dir
    }

    #[test]
    fn file_browses_and_prints_the_path() {
        let dir = picker_dir();
        let mut pty = Pty::start(&format!(
            r#"x=$(rich file '{}'); echo "code=$? got=$x""#,
            dir.display()
        ));
        pty.wait_for("README.md");
        pty.send("docs");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\x1b[C");
        pty.wait_for("guide.md");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        let want = format!("code=0 got={}", dir.join("docs").join("guide.md").display());
        assert!(out.contains(&want), "{out}");
    }

    #[test]
    fn color_prints_the_colour_typed() {
        let mut pty = Pty::start(r#"x=$(rich color --format rgb); echo "code=$? got=$x""#);
        pty.wait_for("Colour");
        pty.send("#00ff00");
        pty.wait_for("#00ff00");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=rgb(0,255,0)"), "{out}");
    }

    #[test]
    fn asset_prints_the_emoji_picked() {
        let mut pty = Pty::start(r#"x=$(rich asset); echo "code=$? got=$x""#);
        pty.wait_for("Emoji");
        pty.send("rocket");
        pty.wait_for("rocket");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=🚀"), "{out}");
    }

    #[test]
    fn write_keeps_tabs_and_takes_a_crlf_as_one_line_ending() {
        let mut pty = Pty::start(r#"printf 'a\tb\r\n' | rich write | od -An -tx1"#);
        pty.wait_for("submit");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\x04");
        let out = pty.finish();
        // `od` pads its columns differently on GNU and BSD (macOS), so
        // compare the bytes: exactly a, tab, b, newline.
        let dumped = out.lines().any(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let is_byte = |w: &str| w.len() == 2 && w.chars().all(|c| c.is_ascii_hexdigit());
            let bytes: Vec<&str> = words
                .iter()
                .rev()
                .take_while(|w| is_byte(w))
                .copied()
                .collect();
            bytes == ["0a", "62", "09", "61"]
        });
        assert!(dumped, "{out:?}");
    }

    #[test]
    fn a_mouse_report_at_zero_does_not_panic() {
        let mut pty = Pty::start(r#"x=$(rich choose --mouse a b c); echo "code=$? got=[$x]""#);
        pty.wait_for("3/3");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\x1b[<0;0;0M");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out:?}");
    }

    #[test]
    fn one_click_on_the_focused_row_does_not_pick_it() {
        let mut pty = Pty::start(r#"x=$(rich choose --mouse a b c); echo "code=$? got=[$x]""#);
        pty.wait_for("3/3");
        std::thread::sleep(Duration::from_millis(200));
        // `a`, focused from the start, on row 2 of the alternate screen.
        pty.send("\x1b[<0;5;2M\x1b[<0;5;2m");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out:?}");
    }

    /// `/proc/kmsg` reads block until the kernel logs something; its
    /// preview must not freeze the picker (only root may open it).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_preview_whose_read_blocks_leaves_the_keys_working() {
        let mut pty = Pty::start(r#"x=$(rich file /proc --value kmsg); echo "code=$? got=[$x]""#);
        pty.wait_for("kmsg");
        std::thread::sleep(Duration::from_millis(500));
        pty.send("\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out:?}");
    }

    #[test]
    fn with_mouse_a_click_picks_and_the_modes_are_undone() {
        let mut pty =
            Pty::start(r#"x=$(rich choose --mouse apple banana cherry); echo "code=$? got=$x""#);
        pty.wait_for("cherry");
        std::thread::sleep(Duration::from_millis(200));
        // On the alternate screen: the question is row 1, banana row 3
        // (SGR mouse reports count from 1). Two clicks: focus, then pick.
        for _ in 0..2 {
            pty.send("\x1b[<0;5;3M\x1b[<0;5;3m");
            std::thread::sleep(Duration::from_millis(100));
        }
        let out = pty.finish();
        assert!(out.contains("code=0 got=banana"), "{out}");
        assert!(out.contains("\x1b[?1000h"), "mouse reporting on: {out:?}");
        let after = &out[out.find("\x1b[?1000h").unwrap()..];
        for mode in ["\x1b[?1000l", "\x1b[?1006l", "\x1b[?1049l"] {
            assert!(after.contains(mode), "{mode:?} left on: {out:?}");
        }
        // Without --mouse, nothing asks for the mouse.
        let mut pty = Pty::start(r#"x=$(rich choose apple banana); echo "got=$x""#);
        pty.wait_for("banana");
        pty.send("\r");
        let out = pty.finish();
        assert!(!out.contains("\x1b[?1000h"), "{out:?}");
    }

    /// `rich explore` in a real terminal, on every format `--inspect`
    /// reads: open the first container, pick its first child, and print
    /// its path.
    #[test]
    fn explore_picks_a_path_in_every_format() {
        let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("explore-pty");
        std::fs::create_dir_all(&dir).unwrap();
        let documents = [
            ("doc.json", r#"{"server": {"port": 8080}}"#, "$.server.port"),
            ("doc.yaml", "server:\n  port: 8080\n", "$.server.port"),
            ("doc.toml", "[server]\nport = 8080\n", "$.server.port"),
            (
                "doc.xml",
                "<server><port>8080</port></server>",
                "$.server.port",
            ),
            ("doc.ini", "[server]\nport = 8080\n", "$.server.port"),
            (".env", "PORT=8080\n", "$.PORT"),
        ];
        for (name, text, expected) in documents {
            let file = dir.join(name);
            std::fs::write(&file, text).unwrap();
            let mut pty = Pty::start(&format!(
                r#"x=$(rich explore '{}'); echo "code=$? got=$x""#,
                file.display()
            ));
            pty.wait_for("$ {…} 1 key");
            if expected == "$.PORT" {
                pty.send("\x1b[B");
            } else {
                pty.send("\x1b[B\x1b[C\x1b[B");
            }
            pty.wait_for(expected.rsplit('.').next().unwrap());
            std::thread::sleep(Duration::from_millis(200));
            pty.send("\r");
            let out = pty.finish();
            assert!(
                out.contains(&format!("code=0 got={expected}")),
                "{name}: {out}"
            );
        }
    }

    /// Ctrl+Y writes OSC 52 only where the clipboard is on: forced here,
    /// since the test terminal (xterm-256color) is not known to take it.
    #[test]
    fn explore_copies_through_osc_52_only_when_the_clipboard_is_on() {
        let file = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("explore-copy.json");
        std::fs::write(&file, r#"{"name": "demo"}"#).unwrap();
        let script = |env: &str| {
            format!(
                r#"{env} rich explore '{}' >/dev/null; echo "code=$?""#,
                file.display()
            )
        };
        for (env, copies) in [("RICH_CLIPBOARD=1", true), ("RICH_CLIPBOARD=0", false)] {
            let mut pty = Pty::start(&script(env));
            pty.wait_for("name");
            pty.send("\x1b[B");
            pty.send("\x19"); // Ctrl+Y
            pty.wait_for(if copies {
                "copied path"
            } else {
                "cannot copy path"
            });
            pty.send("\x1b");
            let out = pty.finish();
            // `$.name`, base64.
            assert_eq!(
                out.contains("\x1b]52;c;JC5uYW1l\x07"),
                copies,
                "{env}: {out:?}"
            );
        }
    }
}

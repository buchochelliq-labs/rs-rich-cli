//! `rich spin` (0.0.21, after `gum spin`): a binary-boundary convenience,
//! tested through the binary, without a terminal and (on Unix) in a PTY.

use std::process::{Command, Output, Stdio};

fn rich(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(args)
        .env_remove("NO_COLOR")
        .env_remove("RICH_A11Y")
        .env_remove("RICH_ANIMATION")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

/// A command that prints to both streams and exits with `code`, run by the
/// platform's shell.
#[cfg(unix)]
fn command(code: i32) -> Vec<String> {
    vec![
        "sh".into(),
        "-c".into(),
        format!("echo out; echo err >&2; exit {code}"),
    ]
}

#[cfg(windows)]
fn command(code: i32) -> Vec<String> {
    vec![
        "cmd".into(),
        "/C".into(),
        format!("echo out& echo err 1>&2& exit {code}"),
    ]
}

fn spin(options: &[&str], code: i32) -> Output {
    let command = command(code);
    let mut args: Vec<&str> = vec!["--no-config", "spin"];
    args.extend(options);
    args.push("--");
    args.extend(command.iter().map(String::as_str));
    rich(&args)
}

#[test]
fn the_command_exit_status_is_rich_s() {
    for code in [0, 1, 3, 42] {
        let out = spin(&["--title", "Working"], code);
        assert_eq!(out.status.code(), Some(code), "{out:?}");
    }
}

#[test]
fn without_a_terminal_there_is_no_spinner_and_the_output_goes_through() {
    let out = spin(&["--title", "Working", "--spinner", "moon"], 0);
    assert!(out.status.success());
    assert_eq!(text(&out.stdout).trim_end(), "out");
    let errors = text(&out.stderr);
    assert_eq!(errors.trim_end(), "err");
    // Neither the title nor a control sequence was drawn.
    assert!(
        !errors.contains("Working") && !errors.contains('\x1b'),
        "{errors:?}"
    );
    // Once, not again after the command: --show-output adds nothing here.
    let out = spin(&["--show-output"], 0);
    assert_eq!(text(&out.stdout).trim_end(), "out");
}

#[test]
fn usage_errors_exit_two() {
    let out = rich(&["--no-config", "spin", "--title", "x"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("no command to run"));
    let out = rich(&["spin", "--spinner", "nope", "--", "true"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("no spinner named \"nope\""));
}

#[test]
fn a_command_that_cannot_start_exits_127() {
    let out = rich(&["spin", "--", "rich-spin-test-no-such-command"]);
    assert_eq!(out.status.code(), Some(127));
    assert!(text(&out.stderr).contains("cannot run \"rich-spin-test-no-such-command\""));
}

#[test]
fn help_names_the_options() {
    let out = rich(&["spin", "--help"]);
    assert!(out.status.success());
    let help = text(&out.stdout);
    for option in ["--title", "--spinner", "--show-output", "--show-error"] {
        assert!(help.contains(option), "{option}: {help}");
    }
}

#[cfg(unix)]
mod pty {
    use std::io::Read;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    /// Run `rich ARGS` on a terminal; its output and exit code.
    fn on_terminal(args: &[&str], env: &[(&str, &str)]) -> (String, u32) {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_rich"));
        command.args(args);
        command.env("TERM", "xterm-256color");
        command.env_remove("NO_COLOR");
        command.env_remove("RICH_A11Y");
        command.env_remove("RICH_ANIMATION");
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = pty.slave.spawn_command(command).unwrap();
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
        let end = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < end, "rich spin did not finish");
            std::thread::sleep(Duration::from_millis(20));
        };
        // Let the reader take what is left.
        std::thread::sleep(Duration::from_millis(200));
        drop(pty.master);
        let text = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
        (text, status.exit_code())
    }

    /// `text` without its SGR (colour) sequences.
    fn without_colour(text: &str) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(at) = rest.find("\x1b[") {
            out.push_str(&rest[..at]);
            let tail = &rest[at + 2..];
            match tail.find(|c: char| c.is_ascii_alphabetic()) {
                Some(end) if tail.as_bytes()[end] == b'm' => rest = &tail[end + 1..],
                _ => {
                    out.push_str("\x1b[");
                    rest = tail;
                }
            }
        }
        out.push_str(rest);
        out
    }

    #[test]
    fn on_a_terminal_the_spinner_shows_and_the_output_waits() {
        let script = "sleep 0.5; echo the-output; exit 5";
        let (text, code) = on_terminal(
            &[
                "spin",
                "--title",
                "Working",
                "--spinner",
                "line",
                "--",
                "sh",
                "-c",
                script,
            ],
            &[],
        );
        assert_eq!(code, 5);
        assert!(text.contains("Working"), "{text:?}");
        // The cursor is hidden while it spins and shown again after.
        assert!(
            text.contains("\x1b[?25l") && text.contains("\x1b[?25h"),
            "{text:?}"
        );
        // Several frames of `line` were drawn over the half second.
        let frames = ["- Working", "\\ Working", "| Working", "/ Working"];
        let plain = without_colour(&text);
        let drawn = frames.iter().filter(|frame| plain.contains(*frame)).count();
        assert!(drawn >= 2, "{text:?}");
        // Without --show-output the command's output is held back.
        assert!(!text.contains("the-output"), "{text:?}");

        let (text, code) = on_terminal(
            &["spin", "--show-output", "--", "sh", "-c", "echo the-output"],
            &[],
        );
        assert_eq!(code, 0);
        assert!(text.contains("the-output"), "{text:?}");
        // The default title.
        assert!(text.contains("Loading..."), "{text:?}");
    }

    #[test]
    fn show_error_prints_only_a_failure_s_output() {
        let (text, code) = on_terminal(
            &["spin", "--show-error", "--", "sh", "-c", "echo fine"],
            &[],
        );
        assert_eq!(code, 0);
        assert!(!text.contains("fine"), "{text:?}");
        let (text, code) = on_terminal(
            &[
                "spin",
                "--show-error",
                "--",
                "sh",
                "-c",
                "echo broken >&2; exit 2",
            ],
            &[],
        );
        assert_eq!(code, 2);
        assert!(text.contains("broken"), "{text:?}");
    }

    #[test]
    fn reduced_motion_draws_the_title_once() {
        let (text, code) = on_terminal(
            &["spin", "--title", "Still", "--", "sh", "-c", "sleep 0.4"],
            &[("RICH_A11Y", "reduced-motion")],
        );
        assert_eq!(code, 0);
        assert_eq!(text.matches("Still").count(), 1, "{text:?}");
    }
}

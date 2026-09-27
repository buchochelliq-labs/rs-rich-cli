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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
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

#[test]
fn every_command_has_help() {
    for command in ["choose", "filter", "input", "confirm", "pager"] {
        let (out, _, code) = piped(&[command, "--help"], "");
        assert_eq!(code, 0);
        assert!(out.contains(&format!("rich {command}")), "{out}");
    }
}

#[cfg(unix)]
mod pty {
    use std::io::{Read, Write};
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
    fn pager_pages_piped_text_and_quits() {
        let mut pty = Pty::start(r#"seq 1 200 | rich pager; echo "code=$?""#);
        pty.wait_for("23");
        pty.send("q");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
        assert!(!out.contains("\n200\r\n"), "paged, not printed: {out}");
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
}

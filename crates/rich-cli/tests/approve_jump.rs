//! `rich approve` and `rich jump` (#471, #484): piped, the way they
//! degrade, and in a real PTY with keys sent, the way scripts use them.
#![cfg(feature = "interact")]

use std::io::Write;
use std::process::{Command, Stdio};

/// `(stdout, stderr, exit code)` for `rich ARGS` with `stdin` piped.
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

/// A directory of files with headings and symbols to jump to.
fn project(name: &str) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join(".hidden")).unwrap();
    std::fs::write(
        dir.join("README.md"),
        "# Project\n\n## Installation\n\n```sh\n# not a heading\n```\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("src/queue.rs"),
        "use std::fmt;\n\npub struct ApprovalQueue;\n\nfn review_all() {}\n",
    )
    .unwrap();
    std::fs::write(dir.join(".hidden/secret.md"), "# Secret\n").unwrap();
    std::fs::write(
        dir.join("commands"),
        "Run the tests\tcargo test\ncargo build\n",
    )
    .unwrap();
    dir
}

#[test]
fn approve_without_a_terminal_asks_about_arguments_line_by_line() {
    let (out, err, code) = piped(
        &["approve", "one", "two", "three", "four"],
        "a\nr: no way\n\nq\n",
    );
    assert_eq!(code, 0, "{err}");
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(
        lines,
        [
            r#"{"index":0,"decision":"approved","reason":null,"item":"one"}"#,
            r#"{"index":1,"decision":"rejected","reason":"no way","item":"two"}"#,
            r#"{"index":2,"decision":"skipped","reason":null,"item":"three"}"#,
            r#"{"index":3,"decision":"pending","reason":null,"item":"four"}"#,
        ]
    );
    assert!(err.contains("[1/4] one"), "{err}");
    // A required reason is asked for again.
    let (out, err, _) = piped(&["approve", "--require-reason", "x"], "r\nr: late\n");
    assert!(err.contains("A reason is needed to reject"), "{err}");
    assert!(out.contains(r#""reason":"late""#), "{out}");
}

#[test]
fn approve_from_stdin_without_a_terminal_leaves_everything_pending() {
    let input = "{\"id\": 1, \"title\": \"Deploy\"}\n{\"id\": 2, \"title\": \"Drop\"}\n";
    let (out, err, code) = piped(&["approve"], input);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        out.lines().next().unwrap(),
        r#"{"index":0,"decision":"pending","reason":null,"item":{"id":1,"title":"Deploy"}}"#
    );
    // `--print approved` prints nothing then.
    let (out, _, code) = piped(&["approve", "--print", "approved"], input);
    assert_eq!((out.as_str(), code), ("", 0));
    // Lines that are not all JSON objects are lines.
    let (out, _, _) = piped(&["approve"], "{\"a\": 1}\nplain\n");
    assert!(out.contains(r#""item":"{\"a\": 1}""#), "{out}");
    let (_, err, code) = piped(&["approve", "--format", "jsonl"], "{}\nplain\n");
    assert_eq!(code, 3);
    assert!(err.contains("line 2 is not a JSON object"), "{err}");
}

#[test]
fn approve_and_jump_check_their_arguments() {
    for (args, needle) in [
        (&["approve", "--format", "xml", "a"][..], "--format"),
        (&["approve", "--print", "all", "a"], "--print"),
        (&["approve", "--height", "0", "a"], "--height"),
        (&["approve", "--value", "x", "a"], "unknown option --value"),
        (&["jump", "--print", "path"], "--print"),
        (&["jump", "--limit", "x"], "--limit"),
        (&["jump", "--require-reason"], "unknown option"),
    ] {
        let (_, err, code) = piped(args, "");
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains(needle), "{args:?}: {err}");
    }
    let (_, err, code) = piped(&["approve"], "");
    assert_eq!(code, 2, "{err}");
    let (_, err, code) = piped(&["jump", "no/such/dir", "--value", "x"], "");
    assert_eq!(code, 3);
    assert!(err.contains("is not a directory"), "{err}");
    for command in ["approve", "jump"] {
        let (out, _, code) = piped(&[command, "--help"], "");
        assert_eq!(code, 0);
        assert!(out.contains(&format!("rich {command}")), "{out}");
    }
}

#[test]
fn jump_without_a_terminal_prints_the_best_match() {
    let dir = project("jump-piped");
    let root = dir.to_str().unwrap();
    let (out, err, code) = piped(&["jump", "--value", "install", root], "");
    assert_eq!(code, 0, "{err}");
    assert_eq!(out, format!("{root}/README.md:3\n"));
    let (out, _, _) = piped(&["jump", "--value", "@approval", root], "");
    assert_eq!(out, format!("{root}/src/queue.rs:3\n"));
    let (out, _, _) = piped(&["jump", "--value", "queue", "--files", root], "");
    assert_eq!(out, format!("{root}/src/queue.rs\n"));
    // Hidden directories are not searched.
    let (out, _, _) = piped(&["jump", "--value", "Secret", "--headings", root], "");
    assert!(!out.contains(".hidden"), "{out}");
    let commands = dir.join("commands");
    let commands = commands.to_str().unwrap();
    let (out, _, _) = piped(
        &[
            "jump",
            "--commands",
            commands,
            "--value",
            ">tests",
            "--print",
            "json",
        ],
        "",
    );
    assert_eq!(
        out,
        "{\"source\":\"commands\",\"label\":\"Run the tests\",\"location\":\"cargo test\"}\n"
    );
    // A line on stdin is the query when there is no --value.
    let (out, _, code) = piped(&["jump", "--files", root], "readme\n");
    assert_eq!((out, code), (format!("{root}/README.md\n"), 0));
    // With nothing to go on, no answer.
    let (_, _, code) = piped(&["jump", "--files", root], "");
    assert_eq!(code, 3);
}

#[test]
fn jump_reads_entries_from_stdin() {
    let input = "{\"label\":\"Home\",\"location\":\"/home\",\"source\":\"Places\"}\nnotes.txt\n";
    let (out, _, code) = piped(&["jump", "--value", "hom"], input);
    assert_eq!((out.as_str(), code), ("/home\n", 0));
    let (out, _, _) = piped(&["jump", "--value", "notes", "-"], input);
    assert_eq!(out, "notes.txt\n");
    let (_, err, code) = piped(&["jump"], "");
    assert_eq!(code, 3);
    assert!(err.contains("nothing to jump to"), "{err}");
}

#[test]
fn jump_remembers_the_pick_in_the_recent_file() {
    let dir = project("jump-recent");
    let root = dir.to_str().unwrap();
    let recent = dir.join("recent");
    std::fs::write(&recent, "elsewhere\n").unwrap();
    let recent_arg = recent.to_str().unwrap();
    let (out, err, code) = piped(
        &[
            "jump", "--recent", recent_arg, "--value", "queue.rs", "--files", root,
        ],
        "",
    );
    assert_eq!(code, 0, "{err}");
    let picked = out.trim_end();
    assert_eq!(
        std::fs::read_to_string(&recent).unwrap(),
        format!("{picked}\nelsewhere\n")
    );
    // Recent places are searched too.
    let (out, _, _) = piped(
        &["jump", "--recent", recent_arg, "--value", "elsew", root],
        "",
    );
    assert_eq!(out, "elsewhere\n");
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
                    cols: 100,
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

        fn screen(&self) -> String {
            let mut parser = vt100::Parser::new(24, 100, 0);
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

        /// The whole output once the script is done; the terminal must be
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
    fn approve_reads_keys_from_the_terminal_while_stdin_is_the_list() {
        let mut pty = Pty::start(
            r#"x=$(printf 'alpha\nbeta\ngamma\n' | rich approve --header Review); echo "code=$? got=[$x]""#,
        );
        pty.wait_for("0/3 decided");
        pty.send("a");
        pty.wait_for("1/3 decided");
        pty.send("R");
        pty.wait_for("Reason to reject");
        pty.send("too soon\r");
        pty.wait_for("2/3 decided");
        pty.send("s");
        pty.wait_for("3/3 decided");
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0"), "{out}");
        assert!(
            out.contains(r#"{"index":1,"decision":"rejected","reason":"too soon","item":"beta"}"#),
            "{out}"
        );
        assert!(
            out.contains(r#""decision":"skipped","reason":null,"item":"gamma""#),
            "{out}"
        );
    }

    #[test]
    fn approve_marks_in_bulk_and_escape_cancels() {
        let mut pty =
            Pty::start(r#"x=$(rich approve --print approved a b c); echo "code=$? got=[$x]""#);
        pty.wait_for("0/3 decided");
        // Mark every pending item, approve them together.
        pty.send("\x01");
        pty.wait_for("3 marked");
        pty.send("a");
        pty.wait_for("3/3 decided");
        pty.send("\r");
        let out = pty.finish();
        assert!(
            out.contains("code=0 got=[a\r\nb\r\nc]") || out.contains("code=0 got=[a\nb\nc]"),
            "{out}"
        );

        let mut pty = Pty::start(r#"x=$(rich approve a b); echo "code=$? got=[$x]""#);
        pty.wait_for("0/2 decided");
        pty.send("a\x1b");
        let out = pty.finish();
        assert!(out.contains("code=1 got=[]"), "{out}");
    }

    #[test]
    fn jump_narrows_and_prints_the_place() {
        let dir = super::project("jump-pty");
        let root = dir.to_str().unwrap();
        let commands = format!("{root}/commands");
        let mut pty = Pty::start(&format!(
            r#"x=$(rich jump --commands '{commands}' '{root}'); echo "code=$? got=[$x]""#
        ));
        pty.wait_for("results");
        pty.wait_for("Headings");
        pty.send(">test");
        pty.wait_for("Run the tests");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        assert!(out.contains("code=0 got=[cargo test]"), "{out}");

        let mut pty = Pty::start(&format!(
            r#"x=$(rich jump --files --headings '{root}'); echo "code=$? got=[$x]""#
        ));
        pty.wait_for("Files");
        // Tab narrows to Files; then to Headings, where Installation is.
        pty.send("\t\t");
        pty.wait_for("[Headings]");
        pty.send("inst");
        pty.wait_for("inst");
        std::thread::sleep(Duration::from_millis(200));
        pty.send("\r");
        let out = pty.finish();
        assert!(
            out.contains(&format!("code=0 got=[{root}/README.md:3]")),
            "{out}"
        );
    }
}

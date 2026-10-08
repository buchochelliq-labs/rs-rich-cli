//! The PTY harness: run this test binary's `child` test inside a real PTY
//! under `sh`, then `stty -a` in the same terminal. The child learns its
//! mode from `INTERACT_CHILD`, and the backend to drive the terminal with
//! from `INTERACT_BACKEND` (crossterm when unset).
#![allow(dead_code)]

use std::io::{Read, Write};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};

pub struct Pty {
    output: Arc<Mutex<Vec<u8>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// Open until the child is done, and for resizing the terminal.
    master: Option<Box<dyn MasterPty + Send>>,
}

/// The kitty keyboard protocol's query, as crossterm sends it: the flags,
/// then the device attributes.
const KITTY_QUERY: &[u8] = b"\x1b[?u\x1b[c";

impl Pty {
    /// A terminal without the kitty keyboard protocol: it answers the
    /// query's device attributes only, as xterm does.
    pub fn start(mode: &str) -> Pty {
        Pty::start_as(mode, false)
    }

    /// [`start`](Pty::start), with the child driving the terminal with
    /// `backend` (a `BackendKind` name).
    pub fn start_on(backend: &str, mode: &str) -> Pty {
        Pty::start_full(mode, false, "", Some(backend))
    }

    /// [`start_kitty`](Pty::start_kitty), with the child driving the
    /// terminal with `backend`.
    pub fn start_kitty_on(backend: &str, mode: &str) -> Pty {
        Pty::start_full(mode, true, "", Some(backend))
    }

    /// [`start_kitty_piped`](Pty::start_kitty_piped), with the child
    /// driving the terminal with `backend`.
    pub fn start_kitty_piped_on(backend: &str, mode: &str) -> Pty {
        Pty::start_full(mode, true, " | sed -n l", Some(backend))
    }

    /// A terminal with the kitty keyboard protocol: it answers the query
    /// with its flags (none pushed yet) before the device attributes.
    pub fn start_kitty(mode: &str) -> Pty {
        Pty::start_as(mode, true)
    }

    /// A terminal with the kitty keyboard protocol, with the child's
    /// standard output piped through `sed -n l`, which shows its escapes,
    /// as `answer=$(rich write)` captures it.
    pub fn start_kitty_piped(mode: &str) -> Pty {
        Pty::start_with(mode, true, " | sed -n l")
    }

    fn start_as(mode: &str, kitty: bool) -> Pty {
        Pty::start_with(mode, kitty, "")
    }

    fn start_with(mode: &str, kitty: bool, pipe: &str) -> Pty {
        Pty::start_full(mode, kitty, pipe, None)
    }

    fn start_full(mode: &str, kitty: bool, pipe: &str, backend: Option<&str>) -> Pty {
        Pty::start_answering(mode, kitty, false, pipe, backend, &[])
    }

    /// A terminal that knows synchronized output (it answers DECRQM for
    /// mode 2026, reset) and, with `kitty`, the kitty keyboard protocol,
    /// with the child driving it with `backend` and `env` set.
    pub fn start_sync_on(backend: &str, mode: &str, kitty: bool, env: &[(&str, &str)]) -> Pty {
        Pty::start_answering(mode, kitty, true, "", Some(backend), env)
    }

    /// A terminal without synchronized output or the kitty keyboard
    /// protocol, with the child driving it with `backend` and `env` set.
    pub fn start_env_on(backend: &str, mode: &str, env: &[(&str, &str)]) -> Pty {
        Pty::start_answering(mode, false, false, "", Some(backend), env)
    }

    fn start_answering(
        mode: &str,
        kitty: bool,
        sync: bool,
        pipe: &str,
        backend: Option<&str>,
        env: &[(&str, &str)],
    ) -> Pty {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let exe = std::env::current_exe().unwrap();
        let script = format!(
            "'{}' --exact child --nocapture --test-threads=1{pipe}; stty -a; echo STTY-DONE",
            exe.display()
        );
        let mut command = CommandBuilder::new("sh");
        command.args(["-c", &script]);
        command.env("INTERACT_CHILD", mode);
        command.env("TERM", "xterm-256color");
        command.env("RUST_BACKTRACE", "0");
        if let Some(backend) = backend {
            command.env("INTERACT_BACKEND", backend);
        }
        // Detected unless a test forces it.
        command.env_remove("RICH_SYNC_OUTPUT");
        for (name, value) in env {
            command.env(name, value);
        }
        let child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&output);
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(pty.master.take_writer().unwrap()));
        let answers = Arc::clone(&writer);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            let mut answered = 0;
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let mut sink = sink.lock().unwrap();
                sink.extend_from_slice(&buffer[..read]);
                // Answer each new query, so the session does not wait for
                // an answer that never comes.
                let asked = sink
                    .windows(KITTY_QUERY.len())
                    .filter(|w| *w == KITTY_QUERY)
                    .count();
                while answered < asked {
                    // In the order asked: synchronized output (DECRQM)
                    // first, when the query has it.
                    let mut answer = Vec::new();
                    if sync {
                        answer.extend_from_slice(b"\x1b[?2026;2$y");
                    }
                    if kitty {
                        answer.extend_from_slice(b"\x1b[?0u");
                    }
                    answer.extend_from_slice(b"\x1b[?62c");
                    let mut answers = answers.lock().unwrap();
                    let _ = answers.write_all(&answer).and_then(|()| answers.flush());
                    answered += 1;
                }
            }
        });
        Pty {
            output,
            writer,
            child,
            master: Some(pty.master),
        }
    }

    /// Resize the terminal, as a window would be: the child gets SIGWINCH.
    pub fn resize(&self, cols: u16, rows: u16) {
        let master = self.master.as_ref().expect("the terminal is open");
        master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    pub fn wait_for(&self, needle: &str) {
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            if self.text().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {needle:?}; output:\n{}", self.text());
    }

    pub fn send(&mut self, bytes: &str) {
        self.try_send(bytes).unwrap();
    }

    /// Send `bytes` when the child may already have finished: on macOS a
    /// write to the master fails (EIO) once the terminal's last reader is
    /// gone, where Linux buffers it.
    pub fn try_send(&mut self, bytes: &str) -> std::io::Result<()> {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(bytes.as_bytes())?;
        writer.flush()
    }

    /// Finish, and return the whole output and the final screen.
    pub fn finish(mut self) -> (String, vt100::Parser) {
        self.wait_for("STTY-DONE");
        let _ = self.child.wait();
        // Never closed while the reader thread may still read it.
        std::mem::forget(self.master.take());
        let bytes = self.output.lock().unwrap().clone();
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(&bytes);
        (String::from_utf8_lossy(&bytes).into_owned(), parser)
    }
}

/// Raw mode off, the main screen back, the cursor shown, mouse reporting
/// off, and the kitty keyboard protocol's flags popped as often as pushed
/// (crossterm pushes flags 1 and 2, termwiz flag 1), and no synchronized
/// update left open.
pub fn assert_restored(output: &str, parser: &vt100::Parser) {
    assert_eq!(
        output.matches("\x1b[>3u").count() + output.matches("\x1b[>1u").count(),
        output.matches("\x1b[<1u").count(),
        "kitty keyboard flags pushed and popped unevenly:\n{output:?}"
    );
    if let Some(last) = output.rfind("\x1b[?2026h") {
        assert!(
            output[last..].contains("\x1b[?2026l"),
            "a synchronized update left open:\n{output:?}"
        );
    }
    let stty = &output[output.rfind("speed").expect("stty output")..];
    let flags: Vec<&str> = stty.split_whitespace().collect();
    for flag in ["icanon", "echo", "isig", "icrnl", "opost"] {
        assert!(flags.contains(&flag), "{flag:?} not restored:\n{stty}");
    }
    let screen = parser.screen();
    assert!(!screen.alternate_screen(), "still on the alternate screen");
    assert!(!screen.hide_cursor(), "cursor still hidden");
    assert_eq!(
        screen.mouse_protocol_mode(),
        vt100::MouseProtocolMode::None,
        "mouse reporting still on"
    );
    assert!(!screen.bracketed_paste(), "bracketed paste still on");
}

/// The child's process id, from its view.
pub fn child_pid(pty: &Pty) -> String {
    let text = pty.text();
    let start = text.rfind("pid ").expect("the child shows its pid") + 4;
    let end = start + text[start..].find('.').expect("pid ends with a dot");
    text[start..end].to_string()
}

/// After the suspend: every mode left (so the shell has a normal terminal);
/// after `SIGCONT`: every mode on again and the view painted anew.
pub fn suspends_and_resumes(pty: &mut Pty, suspend: impl FnOnce(&mut Pty, &str)) {
    pty.wait_for("child ready");
    let pid = child_pid(pty);
    let before = pty.text().len();
    suspend(pty, &pid);
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let stopped = loop {
        let text = pty.text();
        let after = &text[before..];
        if ["\x1b[?1049l", "\x1b[?1000l", "\x1b[?2004l", "\x1b[?25h"]
            .iter()
            .all(|mode| after.contains(mode))
        {
            break after.len();
        }
        assert!(std::time::Instant::now() < end, "not restored:\n{after:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    // Stopped: `T` in the third field. The session gives the terminal back
    // before it stops itself, so the state can still read `R` for a moment
    // after the restore sequences arrive; wait for the stop.
    let stop_by = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while let Ok(state) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let field = state.rsplit(')').next().unwrap().split_whitespace().next();
        if field == Some("T") {
            break;
        }
        assert!(
            std::time::Instant::now() < stop_by,
            "never stopped: {state}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Command::new("kill").args(["-CONT", &pid]).status().unwrap();
    let end = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let text = pty.text();
        let resumed = &text[before + stopped..];
        if resumed.contains("\x1b[?1049h") && resumed.contains("child ready") {
            break;
        }
        assert!(std::time::Instant::now() < end, "not resumed:\n{resumed:?}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

//! The PTY harness: run this test binary's `child` test inside a real PTY
//! under `sh`, then `stty -a` in the same terminal.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

pub struct Pty {
    output: Arc<Mutex<Vec<u8>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
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

    /// A terminal with the kitty keyboard protocol: it answers the query
    /// with its flags (none pushed yet) before the device attributes.
    pub fn start_kitty(mode: &str) -> Pty {
        Pty::start_as(mode, true)
    }

    fn start_as(mode: &str, kitty: bool) -> Pty {
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
            "'{}' --exact child --nocapture --test-threads=1; stty -a; echo STTY-DONE",
            exe.display()
        );
        let mut command = CommandBuilder::new("sh");
        command.args(["-c", &script]);
        command.env("INTERACT_CHILD", mode);
        command.env("TERM", "xterm-256color");
        command.env("RUST_BACKTRACE", "0");
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
                    let answer: &[u8] = if kitty {
                        b"\x1b[?0u\x1b[?62c"
                    } else {
                        b"\x1b[?62c"
                    };
                    let mut answers = answers.lock().unwrap();
                    let _ = answers.write_all(answer).and_then(|()| answers.flush());
                    answered += 1;
                }
            }
        });
        // Keep the master open until the child is done.
        std::mem::forget(pty.master);
        Pty {
            output,
            writer,
            child,
        }
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
        let bytes = self.output.lock().unwrap().clone();
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(&bytes);
        (String::from_utf8_lossy(&bytes).into_owned(), parser)
    }
}

/// Raw mode off, the main screen back, the cursor shown, mouse reporting
/// off, and the kitty keyboard protocol's flags popped as often as pushed.
pub fn assert_restored(output: &str, parser: &vt100::Parser) {
    assert_eq!(
        output.matches("\x1b[>3u").count(),
        output.matches("\x1b[<1u").count(),
        "kitty keyboard flags pushed and popped unevenly:\n{output:?}"
    );
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

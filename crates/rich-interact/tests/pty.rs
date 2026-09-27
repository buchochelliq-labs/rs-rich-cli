//! In a real PTY: the terminal is restored after every way out (#489) —
//! finishing, Ctrl+C, a panic, and after handing the terminal to another
//! program.
//!
//! The test binary runs itself as the child (`child` below, which does
//! nothing unless `INTERACT_CHILD` is set), inside `sh`, and runs `stty -a`
//! afterwards in the same terminal: raw mode left on would show as
//! `-icanon -echo`. A vt100 emulator follows the screen for the rest (the
//! alternate screen, the cursor, mouse reporting).
#![cfg(unix)]

use std::io::{Read, Write};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rich_interact::policy::Policy;
use rich_interact::{
    run, Component, Context, Event, Flow, KeyCode, RunOptions, SessionOptions, View,
};

/// The child's component: `enter` finishes, `p` panics, `e` hands the
/// terminal to `sh -c 'echo handed-off'`.
struct Child {
    returned: Option<Option<i32>>,
}

impl Component for Child {
    type Output = String;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<String> {
        match event {
            Event::Returned(code) => self.returned = Some(*code),
            Event::Key(key) => match key.code {
                KeyCode::Enter => return Flow::Done("finished".into()),
                KeyCode::Char('p') => panic!("child panics on purpose"),
                KeyCode::Char('e') => {
                    let mut command = Command::new("sh");
                    command.args(["-c", "echo handed-off"]);
                    return Flow::Handoff(command);
                }
                _ => {}
            },
            _ => {}
        }
        Flow::Continue
    }

    fn render(&self, context: &Context<'_>) -> View {
        let text = match self.returned {
            Some(code) => format!("back from handoff {code:?}"),
            None => "child ready".to_string(),
        };
        View::new(context.markup(&text))
    }
}

#[test]
fn child() {
    if std::env::var_os("INTERACT_CHILD").is_none() {
        return;
    }
    let options = RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        session: SessionOptions {
            alternate_screen: true,
            mouse: true,
            bracketed_paste: true,
        },
        ..RunOptions::default()
    };
    let outcome = run(Child { returned: None }, &options);
    println!("OUTCOME {outcome:?}");
}

struct Pty {
    output: Arc<Mutex<Vec<u8>>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Pty {
    fn start(mode: &str) -> Pty {
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
        // Keep the master open until the child is done.
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

    fn wait_for(&self, needle: &str) {
        let end = Instant::now() + Duration::from_secs(30);
        while Instant::now() < end {
            if self.text().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("timed out waiting for {needle:?}; output:\n{}", self.text());
    }

    fn send(&mut self, bytes: &str) {
        self.writer.write_all(bytes.as_bytes()).unwrap();
        self.writer.flush().unwrap();
    }

    /// Finish, and return the whole output and the final screen.
    fn finish(mut self) -> (String, vt100::Parser) {
        self.wait_for("STTY-DONE");
        let _ = self.child.wait();
        let bytes = self.output.lock().unwrap().clone();
        let mut parser = vt100::Parser::new(24, 80, 0);
        parser.process(&bytes);
        (String::from_utf8_lossy(&bytes).into_owned(), parser)
    }
}

/// Raw mode off, the main screen back, the cursor shown, mouse reporting
/// off.
fn assert_restored(output: &str, parser: &vt100::Parser) {
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

#[test]
fn restored_after_finishing() {
    let mut pty = Pty::start("done");
    pty.wait_for("child ready");
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

#[test]
fn restored_after_ctrl_c() {
    let mut pty = Pty::start("interrupt");
    pty.wait_for("child ready");
    pty.send("\x03");
    let (output, parser) = pty.finish();
    assert!(output.contains("OUTCOME Ok(Interrupted)"), "{output}");
    assert_restored(&output, &parser);
}

#[test]
fn restored_after_a_panic() {
    let mut pty = Pty::start("panic");
    pty.wait_for("child ready");
    pty.send("p");
    let (output, parser) = pty.finish();
    assert!(output.contains("child panics on purpose"), "{output}");
    assert!(!output.contains("OUTCOME"), "{output}");
    assert_restored(&output, &parser);
}

#[test]
fn restored_for_a_handoff_and_taken_back() {
    let mut pty = Pty::start("handoff");
    pty.wait_for("child ready");
    pty.send("e");
    pty.wait_for("back from handoff Some(0)");
    pty.send("\r");
    let (output, parser) = pty.finish();
    // The command ran on the normal screen, with the session left: its line
    // is between leaving the alternate screen and entering it again.
    let handed = output.find("handed-off").expect("the command ran");
    let left = output[..handed].rfind("\x1b[?1049l").expect("left before");
    let entered = output[handed..].find("\x1b[?1049h").expect("entered after");
    assert!(left < handed && entered > 0);
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

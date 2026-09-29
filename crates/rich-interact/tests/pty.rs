//! In a real PTY: the terminal is restored after every way out (#489) —
//! finishing, Ctrl+C, a panic, and after handing the terminal to another
//! program. A second session started inside the first is refused and
//! leaves the first one's terminal as it was.
//!
//! The test binary runs itself as the child (`child` below, which does
//! nothing unless `INTERACT_CHILD` is set), inside `sh`, and runs `stty -a`
//! afterwards in the same terminal: raw mode left on would show as
//! `-icanon -echo`. A vt100 emulator follows the screen for the rest (the
//! alternate screen, the cursor, mouse reporting).
#![cfg(unix)]

mod support;

use std::process::Command;

use rich_interact::policy::Policy;
use rich_interact::{
    run, Component, Context, Event, Flow, KeyCode, RunOptions, SessionOptions, View,
};
use support::{assert_restored, Pty};

/// The child's component: `enter` finishes, `p` panics, `e` hands the
/// terminal to `sh -c 'echo handed-off'`, `n` runs another component from
/// inside this one.
struct Child {
    returned: Option<Option<i32>>,
    nested: Option<String>,
}

fn child_options() -> RunOptions {
    RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        session: SessionOptions {
            alternate_screen: true,
            mouse: true,
            bracketed_paste: true,
            ..SessionOptions::default()
        },
        ..RunOptions::default()
    }
}

impl Component for Child {
    type Output = String;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<String> {
        match event {
            Event::Returned(code) => self.returned = Some(*code),
            Event::Key(key) => match key.code {
                KeyCode::Enter => return Flow::Done("finished".into()),
                KeyCode::Char('p') => panic!("child panics on purpose"),
                KeyCode::Char('n') => {
                    let inner = Child {
                        returned: None,
                        nested: None,
                    };
                    let kind = run(inner, &child_options()).map_err(|error| match error {
                        rich_interact::Error::Io(error) => format!("{:?}", error.kind()),
                        other => other.to_string(),
                    });
                    let raw = crossterm::terminal::is_raw_mode_enabled().unwrap();
                    self.nested = Some(format!("nested {kind:?} raw {raw}"));
                }
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
        let text = match (&self.nested, self.returned) {
            (Some(nested), _) => nested.clone(),
            (None, Some(code)) => format!("back from handoff {code:?}"),
            (None, None) => format!("child ready pid {}.", std::process::id()),
        };
        View::new(context.markup(&text))
    }
}

#[test]
fn child() {
    if std::env::var_os("INTERACT_CHILD").is_none() {
        return;
    }
    let child = Child {
        returned: None,
        nested: None,
    };
    let outcome = run(child, &child_options());
    println!("OUTCOME {outcome:?}");
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

#[test]
fn a_session_inside_a_session_is_refused() {
    let mut pty = Pty::start("nested");
    pty.wait_for("child ready");
    pty.send("n");
    // The inner run fails and the outer terminal is still raw.
    pty.wait_for("nested Err(\"ResourceBusy\") raw true");
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

/// The child's process id, from its view.
fn child_pid(pty: &Pty) -> String {
    let text = pty.text();
    let start = text.rfind("pid ").expect("the child shows its pid") + 4;
    let end = start + text[start..].find('.').expect("pid ends with a dot");
    text[start..end].to_string()
}

/// After the suspend: every mode left (so the shell has a normal terminal);
/// after `SIGCONT`: every mode on again and the view painted anew.
fn suspends_and_resumes(pty: &mut Pty, suspend: impl FnOnce(&mut Pty, &str)) {
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
    let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
    if let Some(state) = state {
        // Stopped: `T` in the third field.
        let field = state.rsplit(')').next().unwrap().split_whitespace().next();
        assert_eq!(field, Some("T"), "{state}");
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

#[test]
fn ctrl_z_gives_the_terminal_back_and_fg_takes_it_again() {
    let mut pty = Pty::start("ctrl-z");
    suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1a"));
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

#[test]
fn sigtstp_from_outside_gives_the_terminal_back_too() {
    let mut pty = Pty::start("sigtstp");
    suspends_and_resumes(&mut pty, |_, pid| {
        Command::new("kill").args(["-TSTP", pid]).status().unwrap();
    });
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

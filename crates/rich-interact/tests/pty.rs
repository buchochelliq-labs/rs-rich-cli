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
            (None, None) => "child ready".to_string(),
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

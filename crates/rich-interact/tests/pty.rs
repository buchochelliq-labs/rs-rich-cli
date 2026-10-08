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
    run, Component, Context, Event, Flow, KeyCode, Output, RunOptions, SessionOptions, View,
};
use support::{assert_restored, Pty};

/// The child's component: `enter` (or `d`) finishes, `p` panics, `e` hands the
/// terminal to `sh -c 'echo handed-off'`, `n` runs another component from
/// inside this one.
struct Child {
    returned: Option<Option<i32>>,
    nested: Option<String>,
    /// The last key let go, from a terminal with the kitty protocol.
    released: Option<String>,
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
            Event::KeyUp(key) => self.released = Some(key.to_string()),
            Event::Key(key) => match key.code {
                // `d` too: typed while the process is stopped, in cooked
                // mode, Enter arrives as `\n` (Ctrl+J in raw mode).
                KeyCode::Enter | KeyCode::Char('d') => return Flow::Done("finished".into()),
                KeyCode::Char('p') => panic!("child panics on purpose"),
                KeyCode::Char('n') => {
                    let inner = Child {
                        returned: None,
                        nested: None,
                        released: None,
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
        let text = match &self.released {
            Some(key) => format!("{text} released {key}!"),
            None => text,
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
        released: None,
    };
    let mut options = child_options();
    if std::env::var_os("INTERACT_CHILD").is_some_and(|mode| mode == "piped") {
        options.session.output = Output::Stderr;
    }
    let outcome = run(child, &options);
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

/// In a terminal with the kitty keyboard protocol, the session pushes its
/// flags on the alternate screen and pops them before leaving it, on every
/// way out. Keys come in the protocol's encoding.
#[test]
fn the_kitty_protocol_is_popped_on_every_way_out() {
    let mut pty = Pty::start_kitty("kitty-done");
    pty.wait_for("child ready");
    let text = pty.text();
    let pushed = text.find("\x1b[>3u").expect("the flags are pushed");
    assert!(text[..pushed].contains("\x1b[?1049h"), "{text:?}");
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    let popped = output.rfind("\x1b[<1u").expect("the flags are popped");
    assert!(output[popped..].contains("\x1b[?1049l"), "{output:?}");
    assert_restored(&output, &parser);

    // Ctrl+C as the protocol sends it, and a panic.
    for (mode, key, outcome) in [
        ("kitty-interrupt", "\x1b[99;5u", "OUTCOME Ok(Interrupted)"),
        ("kitty-panic", "p", "child panics on purpose"),
    ] {
        let mut pty = Pty::start_kitty(mode);
        pty.wait_for("child ready");
        pty.send(key);
        let (output, parser) = pty.finish();
        assert!(output.contains(outcome), "{mode}: {output}");
        assert!(output.contains("\x1b[<1u"), "{mode}: {output:?}");
        assert_restored(&output, &parser);
    }

    // Given back for a hand-off and taken again.
    let mut pty = Pty::start_kitty("kitty-handoff");
    pty.wait_for("child ready");
    pty.send("e");
    pty.wait_for("back from handoff Some(0)");
    pty.send("\r");
    let (output, parser) = pty.finish();
    let handed = output.find("handed-off").expect("the command ran");
    assert!(output[..handed].contains("\x1b[<1u"), "{output:?}");
    assert!(output[handed..].contains("\x1b[>3u"), "{output:?}");
    assert_restored(&output, &parser);
}

/// With standard output piped, as in `answer=$(rich write)`, the kitty
/// query is not asked: crossterm would write it to standard output, into
/// the answer, and the terminal's reply would reach the shell afterwards.
#[test]
fn a_piped_answer_holds_no_kitty_query() {
    let mut pty = Pty::start_kitty_piped("piped");
    pty.wait_for("child ready");
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))$"),
        "{output}"
    );
    assert!(!output.contains("\\033[?u"), "{output:?}");
    assert!(!output.contains("\x1b[?u"), "{output:?}");
    assert_restored(&output, &parser);
}

#[test]
fn kitty_releases_reach_the_component_and_repeats_press() {
    let mut pty = Pty::start_kitty("kitty-release");
    pty.wait_for("child ready");
    // `x` let go, then `d` held down until it repeats.
    pty.send("\x1b[120;1:3u");
    pty.wait_for("released x!");
    pty.send("\x1b[100;1:2u");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    assert_restored(&output, &parser);
}

#[test]
fn ctrl_z_pops_the_kitty_protocol_and_fg_pushes_it_again() {
    let mut pty = Pty::start_kitty("kitty-ctrl-z");
    suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1b[122;5u"));
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{output}"
    );
    // Pushed at the start and again on `fg`.
    assert_eq!(output.matches("\x1b[>3u").count(), 2, "{output:?}");
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

/// Enter typed while the process is stopped finishes the component the
/// moment it continues, racing the signal thread's resume: the session
/// must still end restored, not with its modes turned back on after.
#[test]
fn a_session_that_ends_as_it_resumes_stays_restored() {
    for _ in 0..5 {
        let mut pty = Pty::start("resume-race");
        pty.wait_for("child ready");
        let pid = child_pid(&pty);
        Command::new("kill").args(["-TSTP", &pid]).status().unwrap();
        pty.wait_for("\x1b[?1049l");
        pty.send("d");
        std::thread::sleep(std::time::Duration::from_millis(100));
        Command::new("kill").args(["-CONT", &pid]).status().unwrap();
        let (output, parser) = pty.finish();
        assert!(
            output.contains("OUTCOME Ok(Done(\"finished\"))"),
            "{output}"
        );
        assert_restored(&output, &parser);
    }
}

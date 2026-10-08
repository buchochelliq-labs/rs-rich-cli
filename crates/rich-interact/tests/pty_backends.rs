//! Every backend this build has (crossterm, and termion and termwiz behind
//! their features) against one real PTY: the conformance suite (#677,
//! #678).
//!
//! Each test runs the child below once per backend. The child reports
//! every event it receives on standard error, so the same bytes typed at
//! the terminal can be checked to arrive as the same events whichever
//! library read them, and the terminal is checked to be given back
//! (`stty -a` afterwards, and a vt100 emulator) after every way out:
//! finishing, Ctrl+C, a panic, a hand-off, and a suspend and resume.
//! Where a library cannot read something (termion: the kitty keyboard
//! protocol, bracketed paste's modifiers on the mouse), the table says so.
#![cfg(unix)]

mod support;

use std::io::Write;
use std::process::Command;
use std::time::{Duration, Instant};

use rich_interact::policy::Policy;
use rich_interact::{
    run, BackendKind, Button, Component, Context, Event, Flow, Key, Modifiers, Mouse, MouseKind,
    Output, RunOptions, SessionOptions, View,
};
use support::{assert_restored, suspends_and_resumes, Pty};

/// Reports each event on standard error. `d` finishes, `p` panics, `e`
/// hands the terminal to `sh -c 'echo handed-off'`.
#[derive(Default)]
struct Child {
    returned: Option<Option<i32>>,
}

impl Component for Child {
    type Output = String;

    fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<String> {
        let mut err = std::io::stderr().lock();
        let _ = write!(err, "EV<{event:?}>EV\r\n");
        let _ = err.flush();
        match event {
            Event::Returned(code) => self.returned = Some(*code),
            Event::Key(key) if *key == Key::char('d') => return Flow::Done("finished".into()),
            Event::Key(key) if *key == Key::char('p') => panic!("child panics on purpose"),
            Event::Key(key) if *key == Key::char('e') => {
                let mut command = Command::new("sh");
                command.args(["-c", "echo handed-off"]);
                return Flow::Handoff(command);
            }
            _ => {}
        }
        Flow::Continue
    }

    fn mouse(&self) -> bool {
        true
    }

    fn render(&self, context: &Context<'_>) -> View {
        let head = match self.returned {
            Some(code) => format!("back from handoff {code:?}"),
            None => format!("child ready pid {}.", std::process::id()),
        };
        // Five rows, so mouse reports on the first few land in the view.
        View::new(context.markup(&format!("{head}\n-\n-\n-\n-")))
    }
}

fn child_options() -> RunOptions {
    let backend = std::env::var("INTERACT_BACKEND")
        .ok()
        .and_then(|name| BackendKind::from_name(&name))
        .unwrap_or_default();
    RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        session: SessionOptions {
            alternate_screen: true,
            mouse: true,
            bracketed_paste: true,
            backend,
            ..SessionOptions::default()
        },
        ..RunOptions::default()
    }
}

#[test]
fn child() {
    if std::env::var_os("INTERACT_CHILD").is_none() {
        return;
    }
    let mut options = child_options();
    if std::env::var_os("INTERACT_CHILD").is_some_and(|mode| mode == "piped") {
        options.session.output = Output::Stderr;
    }
    if std::env::var_os("INTERACT_CHILD").is_some_and(|mode| mode == "legacy") {
        options.session.legacy_keys = true;
    }
    let outcome = run(Child::default(), &options);
    println!("OUTCOME {outcome:?}");
}

/// Every backend this build has, by name.
fn backends() -> impl Iterator<Item = &'static str> {
    BackendKind::ALL.iter().map(|kind| kind.name())
}

/// Send `bytes`, and return the event the child reports next.
fn event_for(pty: &mut Pty, bytes: &str) -> String {
    let before = pty.text().len();
    pty.send(bytes);
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let text = pty.text();
        if let Some(start) = text.get(before..).and_then(|after| after.find("EV<")) {
            let rest = &text[before + start + 3..];
            if let Some(stop) = rest.find(">EV") {
                return rest[..stop].to_string();
            }
        }
        assert!(
            Instant::now() < end,
            "no event for {bytes:?}:\n{:?}",
            &text[before.min(text.len())..]
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// How many times the kitty keyboard flags were pushed: crossterm's both
/// flags, or termwiz's first.
fn kitty_pushes(text: &str) -> usize {
    text.matches("\x1b[>3u").count() + text.matches("\x1b[>1u").count()
}

fn key(name: &str) -> Event {
    Event::Key(Key::parse(name).unwrap())
}

fn exact(name: &str) -> Event {
    Event::Key(Key::parse(name).unwrap().exact())
}

fn mouse(kind: MouseKind, column: u16, row: u16) -> Event {
    Event::Mouse(Mouse::new(kind, column, row))
}

fn shifted(kind: MouseKind, column: u16, row: u16) -> Event {
    Event::Mouse(Mouse {
        modifiers: Modifiers {
            shift: true,
            ..Modifiers::NONE
        },
        ..Mouse::new(kind, column, row)
    })
}

/// Finish the child with `d`, and check the terminal was given back.
fn finish(mut pty: Pty, backend: &str) {
    pty.send("d");
    let (output, parser) = pty.finish();
    assert!(
        output.contains("OUTCOME Ok(Done(\"finished\"))"),
        "{backend}: {output}"
    );
    assert_restored(&output, &parser);
}

/// What a legacy terminal sends, and the event every backend reads from it.
/// The last column: whether termion reads it.
fn legacy_table() -> Vec<(&'static str, Event, bool)> {
    vec![
        ("a", key("a"), true),
        ("A", key("A"), true),
        ("é", key("é"), true),
        ("\x01", key("ctrl+a"), true),
        ("\t", key("tab"), true),
        ("\r", key("enter"), true),
        ("\x7f", key("backspace"), true),
        ("\x1b", key("esc"), true),
        ("\x1bx", key("alt+x"), true),
        ("\x1b[Z", key("shift+tab"), true),
        ("\x1b[A", key("up"), true),
        ("\x1b[1;5C", key("ctrl+right"), true),
        ("\x1b[3~", key("delete"), true),
        ("\x1b[5~", key("pageup"), true),
        ("\x1bOP", key("f1"), true),
        ("\x1b[15~", key("f5"), true),
        (
            "\x1b[200~pasted text\x1b[201~",
            Event::Paste("pasted text".into()),
            true,
        ),
        (
            "\x1b[<0;5;3M",
            mouse(MouseKind::Down(Button::Left), 4, 2),
            true,
        ),
        (
            "\x1b[<32;6;3M",
            mouse(MouseKind::Drag(Button::Left), 5, 2),
            true,
        ),
        (
            "\x1b[<0;6;3m",
            mouse(MouseKind::Up(Button::Left), 5, 2),
            true,
        ),
        (
            "\x1b[<2;2;2M",
            mouse(MouseKind::Down(Button::Right), 1, 1),
            true,
        ),
        (
            "\x1b[<2;2;2m",
            mouse(MouseKind::Up(Button::Right), 1, 1),
            true,
        ),
        ("\x1b[<64;2;2M", mouse(MouseKind::ScrollUp, 1, 1), true),
        ("\x1b[<65;2;2M", mouse(MouseKind::ScrollDown, 1, 1), true),
        // termion reports no modifiers with the mouse.
        (
            "\x1b[<4;2;2M",
            shifted(MouseKind::Down(Button::Left), 1, 1),
            false,
        ),
        (
            "\x1b[<4;2;2m",
            shifted(MouseKind::Up(Button::Left), 1, 1),
            false,
        ),
    ]
}

#[test]
fn every_backend_reads_a_legacy_terminal_alike() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "events");
        pty.wait_for("child ready");
        // No kitty flags in a terminal without the protocol.
        assert_eq!(kitty_pushes(&pty.text()), 0, "{backend}");
        for (bytes, expected, termion) in legacy_table() {
            if backend == "termion" && !termion {
                continue;
            }
            assert_eq!(
                event_for(&mut pty, bytes),
                format!("{expected:?}"),
                "{backend}: {bytes:?}"
            );
        }
        finish(pty, backend);
    }
}

#[test]
fn every_backend_sees_the_terminal_resized() {
    for backend in backends() {
        let pty = Pty::start_on(backend, "resize");
        pty.wait_for("child ready");
        pty.resize(100, 30);
        pty.wait_for("EV<Resize { columns: 100, rows: 30 }>EV");
        finish(pty, backend);
    }
}

/// With the kitty keyboard protocol, crossterm and termwiz read every key
/// exactly (crossterm releases too); termion never turns it on, and its
/// keys stay as a legacy terminal sends them.
#[test]
fn kitty_keys_are_exact_where_the_backend_reads_them() {
    for backend in backends() {
        let mut pty = Pty::start_kitty_on(backend, "kitty");
        pty.wait_for("child ready");
        let text = pty.text();
        match backend {
            "crossterm" => assert!(text.contains("\x1b[>3u"), "{text:?}"),
            "termwiz" => assert!(text.contains("\x1b[>1u"), "{text:?}"),
            _ => {
                assert_eq!(kitty_pushes(&text), 0, "{backend}: {text:?}");
                let tab = event_for(&mut pty, "\t");
                assert_eq!(tab, format!("{:?}", key("tab")), "{backend}");
                finish(pty, backend);
                continue;
            }
        }
        for (bytes, expected) in [
            ("\x1b[105;5u", exact("ctrl+i")),
            ("\t", exact("tab")),
            ("\x1b[27u", exact("esc")),
            ("\x1b[97;6u", exact("ctrl+shift+a")),
            ("\x1b[13;2u", exact("shift+enter")),
            ("a", exact("a")),
        ] {
            assert_eq!(
                event_for(&mut pty, bytes),
                format!("{expected:?}"),
                "{backend}: {bytes:?}"
            );
        }
        if backend == "crossterm" {
            let released = Event::KeyUp(Key::char('x').exact());
            assert_eq!(
                event_for(&mut pty, "\x1b[120;1:3u"),
                format!("{released:?}")
            );
        }
        // Ctrl+Z as the protocol sends it suspends, and `fg` pushes the
        // flags again.
        let pushed = kitty_pushes(&pty.text());
        suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1b[122;5u"));
        assert_eq!(kitty_pushes(&pty.text()), pushed + 1, "{backend}");
        finish(pty, backend);
    }
}

#[test]
fn every_backend_gives_the_terminal_back_on_ctrl_c_and_a_panic() {
    for backend in backends() {
        for (mode, key, outcome) in [
            ("interrupt", "\x03", "OUTCOME Ok(Interrupted)"),
            ("panic", "p", "child panics on purpose"),
        ] {
            let mut pty = Pty::start_on(backend, mode);
            pty.wait_for("child ready");
            pty.send(key);
            let (output, parser) = pty.finish();
            assert!(output.contains(outcome), "{backend} {mode}: {output}");
            assert_restored(&output, &parser);
        }
    }
}

#[test]
fn every_backend_hands_the_terminal_off_and_takes_it_back() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "handoff");
        pty.wait_for("child ready");
        pty.send("e");
        pty.wait_for("back from handoff Some(0)");
        // The command ran on the normal screen, with the session left.
        let text = pty.text();
        let handed = text.find("handed-off").expect("the command ran");
        assert!(
            text[..handed].contains("\x1b[?1049l"),
            "{backend}: {text:?}"
        );
        assert!(
            text[handed..].contains("\x1b[?1049h"),
            "{backend}: {text:?}"
        );
        // Keys still arrive afterwards.
        assert_eq!(
            event_for(&mut pty, "a"),
            format!("{:?}", key("a")),
            "{backend}"
        );
        finish(pty, backend);
    }
}

#[test]
fn every_backend_suspends_on_ctrl_z_and_sigtstp() {
    for backend in backends() {
        let mut pty = Pty::start_on(backend, "ctrl-z");
        suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1a"));
        assert_eq!(
            event_for(&mut pty, "a"),
            format!("{:?}", key("a")),
            "{backend}"
        );
        finish(pty, backend);

        let mut pty = Pty::start_on(backend, "sigtstp");
        suspends_and_resumes(&mut pty, |_, pid| {
            Command::new("kill").args(["-TSTP", pid]).status().unwrap();
        });
        finish(pty, backend);
    }
}

/// Painting on standard error with standard output piped, as
/// `answer=$(rich choose …)` runs: keys still come from the terminal, the
/// answer holds no kitty query, and the terminal is given back.
#[test]
fn every_backend_paints_on_standard_error() {
    for backend in backends() {
        let mut pty = Pty::start_kitty_piped_on(backend, "piped");
        pty.wait_for("child ready");
        // Inside the session, the answer holds nothing yet.
        assert!(!pty.text().contains("OUTCOME"), "{backend}");
        pty.send("a");
        pty.wait_for("EV<");
        pty.send("d");
        let (output, parser) = pty.finish();
        // `sed -n l` shows the piped answer with its escapes, ending in `$`.
        assert!(
            output.contains("OUTCOME Ok(Done(\"finished\"))$"),
            "{backend}: {output}"
        );
        assert!(!output.contains("\\033[?u"), "{backend}: {output:?}");
        assert!(!output.contains("\x1b[?u"), "{backend}: {output:?}");
        assert_restored(&output, &parser);
    }
}

const BEGIN_SYNC: &str = "\x1b[?2026h";
const END_SYNC: &str = "\x1b[?2026l";
/// DECRQM for mode 2026, in the start-up query.
const ASK_SYNC: &str = "\x1b[?2026$p";

/// Whether the child's first frame went out as one synchronized update:
/// begun before the view's text and ended after it.
fn first_frame_synchronized(text: &str) -> bool {
    let Some(view) = text.find("child ready") else {
        return false;
    };
    text[..view].contains(BEGIN_SYNC) && text[view..].contains(END_SYNC)
}

/// A terminal that answers DECRQM for mode 2026 has every frame written as
/// one synchronized update, by every backend that asks it: the question
/// rides on the kitty keyboard query, which termion does not ask, so
/// termion leaves it off. Ctrl+Z ends any update left open before giving
/// the terminal back.
#[test]
fn every_backend_that_asks_wraps_frames_in_synchronized_updates() {
    for backend in backends() {
        for kitty in [false, true] {
            let mut pty = Pty::start_sync_on(backend, "events", kitty, &[]);
            pty.wait_for("child ready");
            let text = pty.text();
            let asks = backend != "termion";
            assert_eq!(text.contains(ASK_SYNC), asks, "{backend}: {text:?}");
            assert_eq!(
                first_frame_synchronized(&text),
                asks,
                "{backend} kitty {kitty}: {text:?}"
            );
            if asks {
                let before = pty.text().len();
                suspends_and_resumes(&mut pty, |pty, _| pty.send("\x1a"));
                let after = pty.text()[before..].to_string();
                // The restore ends an update once more, whatever the last
                // frame left, before it turns the mouse off; `fg` paints
                // anew inside one.
                let given_back = after.find("\x1b[?1006l").expect("the mouse turned off");
                let restoring = &after[..given_back];
                assert_eq!(
                    restoring.matches(END_SYNC).count(),
                    restoring.matches(BEGIN_SYNC).count() + 1,
                    "{backend}: {after:?}"
                );
                assert!(
                    after[given_back..].contains(BEGIN_SYNC),
                    "{backend}: {after:?}"
                );
            }
            // Keys arrive as ever: the answers were no keys.
            let expected = if kitty && asks { exact("a") } else { key("a") };
            assert_eq!(
                event_for(&mut pty, "a"),
                format!("{expected:?}"),
                "{backend}"
            );
            finish(pty, backend);
        }
    }
}

/// `RICH_SYNC_OUTPUT=0` keeps synchronized output off on a terminal that
/// has it; `=1` turns it on for one that does not answer, on every
/// backend, termion among them.
#[test]
fn the_environment_forces_synchronized_output_either_way() {
    for backend in backends() {
        let pty = Pty::start_sync_on(backend, "events", false, &[("RICH_SYNC_OUTPUT", "0")]);
        pty.wait_for("child ready");
        let text = pty.text();
        assert!(!text.contains(BEGIN_SYNC), "{backend}: {text:?}");
        finish(pty, backend);

        let pty = Pty::start_env_on(backend, "events", &[("RICH_SYNC_OUTPUT", "1")]);
        pty.wait_for("child ready");
        let text = pty.text();
        assert!(first_frame_synchronized(&text), "{backend}: {text:?}");
        finish(pty, backend);
    }
}

/// With legacy keys there is no kitty keyboard query to ride on, so
/// nothing is asked at all: synchronized output stays off unless forced.
#[test]
fn a_legacy_keys_session_asks_nothing() {
    for backend in backends() {
        let pty = Pty::start_sync_on(backend, "legacy", true, &[]);
        pty.wait_for("child ready");
        let text = pty.text();
        for query in [ASK_SYNC, "\x1b[?u", "\x1b[c"] {
            assert!(!text.contains(query), "{backend}: {query:?} in {text:?}");
        }
        assert!(!text.contains(BEGIN_SYNC), "{backend}: {text:?}");
        finish(pty, backend);

        let pty = Pty::start_sync_on(backend, "legacy", true, &[("RICH_SYNC_OUTPUT", "1")]);
        pty.wait_for("child ready");
        let text = pty.text();
        assert!(!text.contains(ASK_SYNC), "{backend}: {text:?}");
        assert!(first_frame_synchronized(&text), "{backend}: {text:?}");
        finish(pty, backend);
    }
}

/// An answer that comes after the session stopped waiting for it reaches
/// the backend's reader, which takes it for no key: crossterm reads the
/// DECRQM report and the device attributes after it as one answer of its
/// own, and termion cuts each out whole. termwiz's parser knows no such
/// answers and would read one as typed characters, so with termwiz only
/// the session's own read of the start-up query takes them.
#[test]
fn a_late_answer_is_no_key() {
    for backend in backends().filter(|backend| *backend != "termwiz") {
        let mut pty = Pty::start_on(backend, "events");
        pty.wait_for("child ready");
        let key = event_for(&mut pty, "\x1b[?2026;2$y\x1b[?62ca");
        assert_eq!(key, format!("{:?}", self::key("a")), "{backend}");
        finish(pty, backend);
    }
}

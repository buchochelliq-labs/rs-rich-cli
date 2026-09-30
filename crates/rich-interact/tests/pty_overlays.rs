//! The overlays example in a real PTY (0.0.14 workstream 2): the palette
//! runs a binding, the help overlay and the region's action menu open and
//! close, the status bar shows, and the terminal is restored afterwards.
//! See `support`.
#![cfg(unix)]

#[allow(dead_code)]
#[path = "../examples/overlays.rs"]
mod example;
mod support;

use rich_interact::policy::Policy;
use rich_interact::{run, RunOptions};
use support::{assert_restored, Pty};

#[test]
fn child() {
    let Some(mode) = std::env::var_os("INTERACT_CHILD") else {
        return;
    };
    let options = RunOptions {
        policy: Policy {
            interactive: Some(true),
            ..Policy::default()
        },
        ..RunOptions::default()
    };
    let outcome = match mode.to_str().unwrap() {
        "app" => format!("{:?}", run(example::app(), &options)),
        other => panic!("unknown child {other}"),
    };
    println!("OUTCOME {outcome}");
}

fn drive(keys: &[&str], expected: &str, shown: &[&str]) {
    let mut pty = Pty::start("app");
    pty.wait_for("FILES");
    for key in keys {
        pty.send(key);
        std::thread::sleep(std::time::Duration::from_millis(80));
    }
    let (output, parser) = pty.finish();
    assert!(output.contains(&format!("OUTCOME {expected}")), "{output}");
    for text in shown {
        assert!(output.contains(text), "no {text:?} in:\n{output}");
    }
    assert_restored(&output, &parser);
}

#[test]
fn overlays_open_and_run_in_a_terminal() {
    // Ctrl+O, "move down", Enter: the palette runs the binding. F1 and
    // Escape; Ctrl+K and Escape; Enter picks the second file.
    drive(
        &[
            "\x0f", "m", "o", "v", "e", " ", "d", "o", "w", "n", "\r", "\x1bOP", "\x1b", "\x0b",
            "\x1b", "\r",
        ],
        "Ok(Done(\"src/overlay.rs\"))",
        &["Commands", "Search keys", "Actions · Files", "indexing"],
    );
}

#[test]
fn a_palette_command_of_the_host_cancels_in_a_terminal() {
    // Ctrl+O, "quit", Enter: the example's own command.
    drive(
        &["\x0f", "q", "u", "i", "t", "\r"],
        "Ok(Cancelled)",
        &["Quit without opening"],
    );
}

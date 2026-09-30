//! The composition example in a real PTY (0.0.14 workstream 1): a custom
//! component in a split, tabs switched and switched back, a modal opened
//! and dismissed, and the terminal restored afterwards. See `support`.
#![cfg(unix)]

#[allow(dead_code)]
#[path = "../examples/custom_component.rs"]
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

fn drive(keys: &[&str], expected: &str) {
    let mut pty = Pty::start("app");
    pty.wait_for("Release checklist");
    for key in keys {
        pty.send(key);
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    let (output, parser) = pty.finish();
    assert!(output.contains(&format!("OUTCOME {expected}")), "{output}");
    assert_restored(&output, &parser);
}

#[test]
fn a_custom_component_composes_in_a_terminal() {
    // Down, tick; the Deploy tab and back (Alt+Right, Alt+Left); F1's
    // modal and Escape; Enter finishes the checklist.
    drive(
        &[
            "\x1b[B",
            " ",
            "\x1b[1;3C",
            "\x1b[1;3D",
            "\x1bOP",
            "\x1b",
            "\r",
        ],
        "Ok(Done(Checked([\"update the changelog\"])))",
    );
}

#[test]
fn a_modal_answer_ends_the_app_in_a_terminal() {
    // Ctrl+Q, then `y` in the quit dialog.
    drive(&["\x11", "y"], "Ok(Done(Quit))");
}

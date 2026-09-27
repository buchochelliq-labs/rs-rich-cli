//! Every component in a real PTY: typed into, answered, and the terminal
//! restored afterwards. See `support` for the harness.
#![cfg(unix)]

mod support;

use rich_interact::policy::{Fallback, Policy};
use rich_interact::{run, Confirm, Form, Input, Item, MultiSelect, Pager, RunOptions, Select};
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
    let files = || ["src/lib.rs", "src/main.rs", "README.md"].map(Item::from);
    let outcome = match mode.to_str().unwrap() {
        "select" => format!(
            "{:?}",
            run(Select::new("Open when ready", files()), &options)
        ),
        "multi" => format!(
            "{:?}",
            run(MultiSelect::new("Stage when ready", files()), &options)
        ),
        "input" => format!(
            "{:?}",
            run(Input::new("Name").placeholder("ready"), &options)
        ),
        "confirm" => format!("{:?}", run(Confirm::new("Proceed? ready"), &options)),
        "form" => format!(
            "{:?}",
            run(
                Form::new("Service ready")
                    .text("name", "Name")
                    .toggle("tls", "TLS", false),
                &options
            )
        ),
        "pager" => format!(
            "{:?}",
            run(
                Pager::new(rich::Text::new("ready\nsecond line\nneedle here")),
                &options
            )
        ),
        // No session: the line fallback, with stdin still the terminal.
        "secret" => {
            let options = RunOptions {
                policy: Policy {
                    interactive: Some(false),
                    fallback: Fallback::Prompt,
                    ..Policy::default()
                },
                ..RunOptions::default()
            };
            // The answer's length only: the test looks for the answer itself.
            match run(Input::masked("Token ready"), &options) {
                Ok(rich_interact::Outcome::Done(token)) => format!("Done({})", token.len()),
                other => format!("{other:?}"),
            }
        }
        other => panic!("unknown child {other}"),
    };
    println!("OUTCOME {outcome}");
}

fn drive(mode: &str, keys: &[&str], expected: &str) {
    let mut pty = Pty::start(mode);
    pty.wait_for("ready");
    for key in keys {
        pty.send(key);
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
    let (output, parser) = pty.finish();
    assert!(output.contains(&format!("OUTCOME {expected}")), "{output}");
    assert_restored(&output, &parser);
}

#[test]
fn select_in_a_terminal() {
    // Down, then filter to README and pick it.
    drive(
        "select",
        &["\x1b[B", "rdm", "\r"],
        "Ok(Done(\"README.md\"))",
    );
}

#[test]
fn multi_select_in_a_terminal() {
    drive(
        "multi",
        &["\t", "\t", "\r"],
        "Ok(Done([\"src/lib.rs\", \"src/main.rs\"]))",
    );
}

#[test]
fn input_in_a_terminal() {
    drive("input", &["héllo", "\x7f", "\r"], "Ok(Done(\"héll\"))");
}

#[test]
fn confirm_in_a_terminal() {
    drive("confirm", &["n"], "Ok(Done(\"no\"))");
}

#[test]
fn form_in_a_terminal() {
    drive(
        "form",
        &["api", "\t", " ", "\r"],
        "Ok(Done(Answers([(\"name\", Text(\"api\")), (\"tls\", Flag(true))])))",
    );
}

#[test]
fn pager_in_a_terminal() {
    drive("pager", &["/needle", "\r", "q"], "Ok(Done(()))");
}

#[test]
fn ctrl_c_interrupts_any_component() {
    drive("form", &["ab", "\x03"], "Ok(Interrupted)");
}

#[test]
fn a_masked_line_prompt_does_not_echo() {
    let mut pty = Pty::start("secret");
    pty.wait_for("Token ready: ");
    pty.send("hunter2");
    std::thread::sleep(std::time::Duration::from_millis(40));
    pty.send("\r");
    let (output, parser) = pty.finish();
    assert!(output.contains("OUTCOME Done(7)"), "{output}");
    assert!(!output.contains("hunter2"), "echoed: {output}");
    assert_restored(&output, &parser);
}

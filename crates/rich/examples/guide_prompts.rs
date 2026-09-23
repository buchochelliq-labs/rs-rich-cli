//! Guide: Prompts — run: cargo run -p rs-rich --example guide_prompts [-- --svg docs/media/guide] [-- --interactive]
//!
//! The snippets in docs/guide/core/prompts.md are cut from this file. The
//! scripted examples run without a keyboard; pass `--interactive` to answer
//! real prompts on stdin.

#[path = "guide_support/mod.rs"]
mod guide_support;

use guide_support::Shots;

// --8<-- [start:imports]
use rich::prompt::{Confirm, FloatPrompt, IntPrompt, Prompt, ScriptedInput};
use rich::{Console, Text};
// --8<-- [end:imports]

fn main() {
    let shots = Shots::from_args("guide_prompts");
    shots.shot("questions", 60, questions);
    shots.shot("session", 60, session);
    validation();
    if !shots.is_svg() {
        scripted();
    }
    if std::env::args().any(|arg| arg == "--interactive") {
        interactive().expect("read from stdin");
    }
}

// --8<-- [start:ask]
fn interactive() -> std::io::Result<()> {
    let console = Console::new();

    let name = Prompt::new("What is your [bold]name[/]").ask(&console, Some("World"))?;
    let colour = Prompt::new("Favourite colour")
        .choices(["red", "green", "blue"])
        .case_sensitive(false)
        .ask(&console, None)?;
    let age = IntPrompt::new("Age").ask(&console, None)?;
    let ok = Confirm::new("Continue?").ask(&console, Some(true))?;

    console.print_str(&format!("{name} likes {colour}, is {age}, continue={ok}"));
    Ok(())
}
// --8<-- [end:ask]

// --8<-- [start:scripted]
fn scripted() {
    let console = Console::new();

    // ScriptedInput feeds canned lines instead of reading the keyboard.
    let mut input = ScriptedInput::new(["", "purple", "GREEN"]);
    let name = Prompt::new("Name")
        .ask_from(&console, &mut input, Some("World"))
        .unwrap();
    assert_eq!(name, "World"); // empty answer → the default

    // "purple" is rejected, the question is asked again, "GREEN" matches.
    let colour = Prompt::new("Colour")
        .choices(["red", "green", "blue"])
        .case_sensitive(false)
        .ask_from(&console, &mut input, None)
        .unwrap();
    assert_eq!(colour, "green"); // the choice as spelled in the list

    let mut input = ScriptedInput::new(["forty", "42", "y", "3.5"]);
    assert_eq!(
        IntPrompt::new("Age")
            .ask_from(&console, &mut input, None)
            .unwrap(),
        42
    );
    assert!(Confirm::new("Sure?")
        .ask_from(&console, &mut input, None)
        .unwrap());
    assert_eq!(
        FloatPrompt::new("Ratio")
            .ask_from(&console, &mut input, None)
            .unwrap(),
        3.5
    );
}
// --8<-- [end:scripted]

// --8<-- [start:validate]
fn validation() {
    // The validation step on its own: no I/O at all.
    assert_eq!(IntPrompt::new("n").process_response(" 7 "), Ok(7));
    let error = IntPrompt::new("n").process_response("seven").unwrap_err();
    assert_eq!(
        error.0,
        "[prompt.invalid]Please enter a valid integer number"
    );
    assert_eq!(Confirm::new("ok?").process_response("Y"), Ok(true));
}
// --8<-- [end:validate]

// --8<-- [start:questions]
fn questions(console: &Console) {
    // make_prompt renders a question exactly as ask() prints it.
    console.print(&Prompt::new("What is your [bold]name[/]").make_prompt(console, Some("World")));
    console.print(
        &Prompt::new("Favourite colour")
            .choices(["red", "green", "blue"])
            .make_prompt(console, None),
    );
    console.print(&IntPrompt::new("Age").make_prompt(console, Some(30)));
    console.print(&Confirm::new("Continue?").make_prompt(console, Some(true)));
    console.print(
        &Prompt::new("Password")
            .choices(["hunter2"])
            .show_choices(false)
            .show_default(false)
            .make_prompt(console, Some("hunter2")),
    );
}
// --8<-- [end:questions]

/// A transcript of one exchange, reconstructed from the public pieces:
/// the question, the typed answer, and any rejection message.
fn session(console: &Console) {
    let typed = |question: Text, answer: &str| {
        console.print(&question.append_text(&Text::styled(answer.to_string(), "bold")));
    };
    let colour = Prompt::new("Favourite colour").choices(["red", "green", "blue"]);
    typed(colour.make_prompt(console, None), "purple");
    if let Err(error) = colour.process_response("purple") {
        console.print_str(&error.0);
    }
    typed(colour.make_prompt(console, None), "green");

    let age = IntPrompt::new("Age");
    typed(age.make_prompt(console, None), "forty");
    if let Err(error) = age.process_response("forty") {
        console.print_str(&error.0);
    }
    typed(age.make_prompt(console, None), "42");
}

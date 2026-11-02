//! Cornacai on intuiTUIve's driver, against rs-rich-acp's fake agent
//! replaying a transcript: no agent, no network, no terminal. Each test
//! also checks that the client sent what the transcript expects.

use std::path::{Path, PathBuf};
use std::time::Duration;

use cornacai::app::{Handle, Launch, Options, app, wait_for};
use rich_acp::replay::Transcript;
use rich_intuituive::Driver;

const WAIT: Duration = Duration::from_secs(10);

fn fixture(name: &str) -> Transcript {
    Transcript::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// A fresh workspace for one test.
fn workspace(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cornacai-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn start(name: &str, dir: &Path) -> (Driver, Handle) {
    start_with(Options::new(
        Launch::Replay {
            transcript: fixture(name),
            timeout: Some(WAIT),
        },
        dir,
    ))
}

fn start_with(options: Options) -> (Driver, Handle) {
    let (app, handle) = app(options);
    (app.driver(100, 30), handle)
}

fn screen(driver: &Driver) -> String {
    driver.screen().plain().join("\n")
}

/// Waits for the transcript to have replayed whole.
fn finish(driver: &mut Driver, handle: &Handle) {
    let start = std::time::Instant::now();
    while !handle.replay_finished() && start.elapsed() < WAIT {
        driver.settle();
        std::thread::sleep(Duration::from_millis(10));
    }
    handle.finish_replay().unwrap().unwrap();
}

/// Type `text` into the prompt and send it.
fn send(driver: &mut Driver, text: &str) {
    driver.type_text(text);
    driver.press("enter");
}

#[test]
fn a_reply_streams_in_as_markdown_with_its_plan_and_title() {
    let dir = workspace("stream");
    let (mut driver, handle) = start("stream.jsonl", &dir);
    wait_for(&mut driver, "replay-agent 0.1", WAIT);
    wait_for(&mut driver, "mode default", WAIT);
    send(&mut driver, "Explain the plan");
    wait_for(&mut driver, "Done", WAIT);
    let text = screen(&driver);
    assert!(text.contains("❯ Explain the plan"), "{text}");
    // Markdown, rendered: the heading's text and bold without their marks.
    assert!(text.contains("Steps"), "{text}");
    assert!(!text.contains("# Steps"), "{text}");
    assert!(text.contains("First read the code, then test."), "{text}");
    assert!(text.contains("fn main() {}"), "{text}");
    // The escape sequence in the agent's text is neutralised.
    assert!(text.contains("Done �[2Jhere."), "{text}");
    // The thought is folded, its last line as its summary.
    assert!(text.contains("▸ Thinking"), "{text}");
    assert!(text.contains("The user wants a short plan."), "{text}");
    // The plan, and the title the agent gave the session.
    assert!(text.contains("✓ Read the code"), "{text}");
    assert!(text.contains("▸ Run the tests"), "{text}");
    wait_for(&mut driver, "1 Planning", WAIT);
    finish(&mut driver, &handle);
}

#[test]
fn a_permission_prompt_offers_the_agents_options_and_sends_the_choice() {
    let dir = workspace("permission");
    let (mut driver, handle) = start("permission.jsonl", &dir);
    wait_for(&mut driver, "Prompt", WAIT);
    send(&mut driver, "Fix the config");
    wait_for(&mut driver, "The agent asks to edit", WAIT);
    let text = screen(&driver);
    assert!(
        text.contains("“Editing config.json” (the agent's words)"),
        "{text}"
    );
    assert!(text.contains("file config.json:3"), "{text}");
    assert!(text.contains("1 Allow once"), "{text}");
    assert!(text.contains("2 Always allow edits"), "{text}");
    assert!(text.contains("3 Reject"), "{text}");
    driver.press("2");
    wait_for(&mut driver, "The config is fixed.", WAIT);
    let text = screen(&driver);
    assert!(!text.contains("The agent asks to edit"), "{text}");
    // The tool call's card: done, its output's last line as the summary.
    assert!(text.contains("edit Editing config.json"), "{text}");
    assert!(text.contains("✓"), "{text}");
    assert!(text.contains("Set debug to true."), "{text}");
    finish(&mut driver, &handle);
}

#[test]
fn an_edit_is_reviewed_as_a_diff_then_written_or_refused() {
    let dir = workspace("edit");
    std::fs::write(dir.join("hello.txt"), "hello\nworld\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    let mut options = Options::new(
        Launch::Replay {
            transcript: fixture("edit.jsonl"),
            timeout: Some(WAIT),
        },
        &dir,
    );
    options.read = false;
    options.terminal = false;
    let (mut driver, handle) = start_with(options);
    wait_for(&mut driver, "Prompt", WAIT);
    send(&mut driver, "Change the greeting");
    wait_for(&mut driver, "Review the edit", WAIT);
    let text = screen(&driver);
    assert!(
        text.contains("The agent wants to change hello.txt"),
        "{text}"
    );
    assert!(text.contains("world"), "{text}");
    assert!(text.contains("there"), "{text}");
    // Accept every hunk: the last decision writes the file.
    driver.press("A");
    // The second write is refused whole, and the file is left alone.
    wait_for(&mut driver, "The agent wants to change notes.txt", WAIT);
    assert_eq!(
        std::fs::read_to_string(dir.join("hello.txt")).unwrap(),
        "hello\nthere\n"
    );
    driver.press("escape");
    wait_for(&mut driver, "One edit made.", WAIT);
    let text = screen(&driver);
    assert!(text.contains("Wrote hello.txt"), "{text}");
    assert!(text.contains("Rejected the write to notes.txt"), "{text}");
    // The tool call's card, closed: its diff's file as its summary.
    assert!(text.contains("▸ edit Edit hello.txt"), "{text}");
    assert!(text.contains("  diff hello.txt"), "{text}");
    assert!(!text.contains("failed"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
        "mine\n"
    );
    finish(&mut driver, &handle);
}

#[test]
fn a_hunk_by_hunk_review_writes_only_what_was_accepted() {
    let dir = workspace("hunks");
    let old: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    let new = old
        .replace("line 2\n", "line two\n")
        .replace("line 11\n", "line eleven\n");
    std::fs::write(dir.join("list.txt"), &old).unwrap();
    let transcript = format!(
        "{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n",
        r#"{"client": {"method": "initialize"}}"#,
        r#"{"agent": {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}}"#,
        r#"{"client": {"method": "session/new"}}"#,
        r#"{"agent": {"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "s"}}}"#,
        r#"{"client": {"method": "session/prompt"}}"#,
        serde_json::json!({"agent": {"jsonrpc": "2.0", "id": 9, "method": "fs/write_text_file",
            "params": {"sessionId": "s", "path": "$WORKSPACE/list.txt", "content": new}}}),
        r#"{"client": {"id": 9, "result": {}}}"#,
        r#"{"agent": {"jsonrpc": "2.0", "id": 2, "result": {"stopReason": "end_turn"}}}"#,
    );
    let (mut driver, handle) = start_with(Options::new(
        Launch::Replay {
            transcript: Transcript::parse(&transcript).unwrap(),
            timeout: Some(WAIT),
        },
        &dir,
    ));
    wait_for(&mut driver, "Prompt", WAIT);
    send(&mut driver, "Number the list");
    wait_for(&mut driver, "Review the edit", WAIT);
    // Accept the first hunk, reject the second: the last decision writes.
    driver.press("n a n r");
    wait_for(&mut driver, "1 of 2 hunks", WAIT);
    assert_eq!(
        std::fs::read_to_string(dir.join("list.txt")).unwrap(),
        old.replace("line 2\n", "line two\n")
    );
    finish(&mut driver, &handle);
}

#[test]
fn an_at_mention_completes_and_goes_as_a_resource_link() {
    let dir = workspace("mention");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
    let (mut driver, handle) = start("mention.jsonl", &dir);
    wait_for(&mut driver, "Prompt", WAIT);
    driver.type_text("Read @src/ma");
    wait_for(&mut driver, "src/main.rs", WAIT);
    // Enter takes the candidate, then the rest of the prompt follows.
    driver.press("enter");
    driver.type_text("please");
    driver.press("enter");
    wait_for(&mut driver, "It prints nothing.", WAIT);
    let text = screen(&driver);
    assert!(text.contains("❯ Read @src/main.rs please"), "{text}");
    assert!(text.contains("with @src/main.rs"), "{text}");
    finish(&mut driver, &handle);
}

#[test]
fn sessions_are_tabs_each_with_its_own_conversation() {
    let dir = workspace("sessions");
    let (mut driver, handle) = start("sessions.jsonl", &dir);
    wait_for(&mut driver, "Prompt", WAIT);
    driver.press("ctrl+n");
    wait_for(&mut driver, "2 Session 2", WAIT);
    wait_for(&mut driver, "Prompt", WAIT);
    send(&mut driver, "Second");
    wait_for(&mut driver, "Answer from two.", WAIT);
    // Back to the first session: its own, empty conversation.
    driver.press("alt+p");
    driver.settle();
    let text = screen(&driver);
    assert!(!text.contains("Answer from two."), "{text}");
    send(&mut driver, "First");
    wait_for(&mut driver, "Answer from one.", WAIT);
    assert!(!screen(&driver).contains("Answer from two."));
    driver.press("alt+2");
    wait_for(&mut driver, "Answer from two.", WAIT);
    assert!(!screen(&driver).contains("Answer from one."));
    finish(&mut driver, &handle);
}

#[test]
fn the_shell_pane_opens_beside_the_conversation_and_is_the_users_own() {
    let dir = workspace("shell");
    let host = rich_embed::ReplayHost::new().output("user@box:~$ ");
    let shell = host.handle();
    let transcript = Transcript::parse(
        r#"{"client": {"method": "initialize"}}
{"agent": {"jsonrpc": "2.0", "id": 0, "result": {"protocolVersion": 1}}}
{"client": {"method": "session/new"}}
{"agent": {"jsonrpc": "2.0", "id": 1, "result": {"sessionId": "s"}}}
{"quiet": 300}"#,
    )
    .unwrap();
    let options = Options::new(
        Launch::Replay {
            transcript,
            timeout: Some(WAIT),
        },
        &dir,
    )
    .shell_pane(move || rich_embed::terminal_with(host));
    let (mut driver, handle) = start_with(options);
    wait_for(&mut driver, "Prompt", WAIT);
    assert!(!screen(&driver).contains("user@box"));
    driver.press("ctrl+t");
    wait_for(&mut driver, "user@box:~$", WAIT);
    // Keys typed there go to the shell, not to the agent (the transcript
    // expects nothing more from the client).
    driver.type_text("ls");
    driver.settle();
    assert_eq!(shell.written(), b"ls");
    driver.press("ctrl+t");
    driver.settle();
    assert!(!screen(&driver).contains("user@box"));
    driver.type_text("x");
    driver.settle();
    assert_eq!(shell.written(), b"ls");
    finish(&mut driver, &handle);
}

#[test]
fn a_command_is_asked_about_then_runs_and_its_output_shows_in_its_card() {
    let dir = workspace("terminal");
    let (mut driver, handle) = start("terminal.jsonl", &dir);
    wait_for(&mut driver, "Prompt", WAIT);
    send(&mut driver, "Run the tests");
    wait_for(&mut driver, "The agent wants to run a command", WAIT);
    assert!(screen(&driver).contains("$ echo tests passed"));
    driver.press("y");
    // The second command is refused.
    wait_for(&mut driver, "$ rm -rf build", WAIT);
    driver.press("escape");
    finish(&mut driver, &handle);
    driver.settle();
    // Open the card: the command and its output.
    let text = screen(&driver);
    assert!(text.contains("▸ execute Run the tests"), "{text}");
    assert!(text.contains("Refused the command rm -rf build"), "{text}");
    assert!(driver.click_text("execute Run the tests"));
    driver.settle();
    let text = screen(&driver);
    assert!(text.contains("▾ execute Run the tests"), "{text}");
    assert!(text.contains("tests passed"), "{text}");
}

/// The docs tape's transcript (`docs/tapes/fixtures/cornacai/demo.jsonl`),
/// driven as the tape drives it, so the tape cannot drift from the app.
#[test]
fn the_docs_tape_replays() {
    let dir = workspace("tape");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(
        dir.join("src/main.rs"),
        "fn main() {\n    println!(\"Hello\");\n}\n",
    )
    .unwrap();
    let transcript = Transcript::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/tapes/fixtures/cornacai/demo.jsonl"),
    )
    .unwrap();
    let (mut driver, handle) = start_with(Options::new(
        Launch::Replay {
            transcript,
            timeout: Some(WAIT),
        },
        &dir,
    ));
    wait_for(&mut driver, "demo-agent 1.0", WAIT);
    wait_for(&mut driver, "mode ask", WAIT);
    driver.type_text("Greet by name in @src/ma");
    wait_for(&mut driver, "src/main.rs", WAIT);
    driver.press("enter");
    driver.type_text("please");
    driver.press("enter");
    wait_for(&mut driver, "Review the edit", WAIT);
    driver.press("A");
    wait_for(&mut driver, "The agent asks to run", WAIT);
    assert!(screen(&driver).contains("$ cargo test"));
    driver.press("1");
    wait_for(&mut driver, "and the tests pass.", WAIT);
    let text = screen(&driver);
    assert!(text.contains("1 Greeting by name"), "{text}");
    assert!(text.contains("✓ Run the tests"), "{text}");
    assert!(
        std::fs::read_to_string(dir.join("src/main.rs"))
            .unwrap()
            .contains("Hello, {name}!")
    );
    finish(&mut driver, &handle);
}

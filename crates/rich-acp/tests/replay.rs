//! The client against recorded transcripts, replayed by a fake agent over
//! in-process pipes: no network, no agent installed.

mod support;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use rich_acp::*;
use support::{Answer, Recorder, client_info, fixture, scratch, start};

fn init(connection: &Connection, capabilities: ClientCapabilities) -> InitializeResponse {
    connection
        .initialize(InitializeRequest::new(capabilities, client_info()))
        .unwrap()
}

#[test]
fn a_prompt_turn_streams_its_updates() {
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")));
    let (agent, replay) = start(&fixture("prompt_turn.jsonl"), None, Arc::clone(&recorder));

    let init = init(&agent, ClientCapabilities::none());
    assert_eq!(init.protocol_version, 1);
    assert!(init.agent_capabilities.load_session);
    assert!(init.agent_capabilities.prompt_capabilities.image);
    assert!(init.agent_capabilities.mcp_capabilities.http);
    assert_eq!(
        init.agent_info.as_ref().unwrap().display_name(),
        "Replay Agent"
    );
    assert!(
        agent
            .agent_capabilities()
            .unwrap()
            .session_capabilities
            .is_some()
    );

    let session = agent
        .new_session(NewSessionRequest::new("/home/user/project"))
        .unwrap();
    assert_eq!(session.session_id.as_str(), "sess_abc123def456");
    let modes = session.modes.unwrap();
    assert_eq!(modes.current_mode_id.as_str(), "ask");
    assert_eq!(
        modes.available_modes[1].description.as_deref(),
        Some("Writes code")
    );

    let prompt = PromptRequest {
        session_id: session.session_id.clone(),
        prompt: vec![
            ContentBlock::text("Can you analyze this code for potential issues?"),
            ContentBlock::Resource(EmbeddedResource {
                resource: ResourceContents::Text(TextResourceContents {
                    uri: "file:///home/user/project/main.py".into(),
                    text: "def process_data(items):\n    for item in items:\n        print(item)"
                        .into(),
                    mime_type: Some("text/x-python".into()),
                }),
                annotations: None,
            }),
        ],
    };
    let done = agent.prompt(prompt).unwrap();
    assert_eq!(done.stop_reason, StopReason::EndTurn);
    replay.finish();

    let updates = recorder.updates();
    let kinds: Vec<&str> = updates.iter().map(SessionUpdate::kind).collect();
    assert_eq!(
        kinds,
        [
            "available_commands_update",
            "plan",
            "agent_thought_chunk",
            "agent_message_chunk",
            "agent_message_chunk",
            "tool_call",
            "tool_call_update",
            "tool_call_update",
            "future_update_kind",
            "usage_update",
        ]
    );
    assert_eq!(
        recorder.message_text(),
        "I'll analyze your code for potential issues."
    );

    let SessionUpdate::AvailableCommandsUpdate(commands) = &updates[0] else {
        unreachable!()
    };
    assert_eq!(commands.available_commands[0].name, "web");
    let SessionUpdate::Plan(plan) = &updates[1] else {
        unreachable!()
    };
    assert_eq!(plan.entries.len(), 2);
    assert_eq!(plan.entries[0].priority, PlanEntryPriority::High);
    assert_eq!(plan.entries[1].status, PlanEntryStatus::Pending);
    let SessionUpdate::AgentMessageChunk(chunk) = &updates[3] else {
        unreachable!()
    };
    assert_eq!(chunk.message_id.as_deref(), Some("msg_agent_c42b9"));

    // The tool call, with its updates applied as an app would.
    let SessionUpdate::ToolCall(call) = &updates[5] else {
        unreachable!()
    };
    let mut call = call.clone();
    assert_eq!(call.kind, ToolKind::Other);
    assert_eq!(call.locations[0].line, Some(1));
    for update in &updates[6..8] {
        let SessionUpdate::ToolCallUpdate(update) = update else {
            unreachable!()
        };
        call.apply(update);
    }
    assert_eq!(call.status, ToolCallStatus::Completed);
    assert_eq!(call.title, "Analyzing Python code");
    assert_eq!(call.content.len(), 3);
    let ToolCallContent::Diff(diff) = &call.content[1] else {
        panic!("{:?}", call.content[1])
    };
    assert_eq!(diff.old_text.as_deref(), Some("print(item)"));
    assert_eq!(diff.new_text, "print(repr(item))");
    assert_eq!(call.content[2].kind(), "hologram");

    let SessionUpdate::Unknown { value, .. } = &updates[8] else {
        unreachable!()
    };
    assert_eq!(value["payload"][2], 3);
    let SessionUpdate::UsageUpdate(usage) = &updates[9] else {
        unreachable!()
    };
    assert_eq!((usage.used, usage.size), (53000, 200000));
    assert_eq!(usage.cost.as_ref().unwrap().currency, "USD");

    // The line that was not JSON was reported and skipped.
    let errors = recorder.errors.lock().unwrap();
    assert_eq!(errors.len(), 1, "{errors:?}");
}

#[test]
fn a_permission_request_is_answered() {
    let recorder = Arc::new(Recorder::new(Answer::Choose("allow-once")));
    let (agent, replay) = start(&fixture("permission.jsonl"), None, Arc::clone(&recorder));
    init(&agent, ClientCapabilities::none());
    let session = agent
        .new_session(NewSessionRequest::new("/home/user/project"))
        .unwrap();
    let done = agent
        .prompt(PromptRequest::text(session.session_id, "Fix the config"))
        .unwrap();
    assert_eq!(done.stop_reason, StopReason::EndTurn);
    replay.finish();

    let permissions = recorder.permissions.lock().unwrap();
    assert_eq!(permissions.len(), 1);
    let request = &permissions[0];
    assert_eq!(request.tool_call.kind, Some(ToolKind::Edit));
    assert_eq!(request.options.len(), 3);
    assert!(request.options[1].kind.allows());
    assert!(!request.options[2].kind.allows());
    assert_eq!(
        request
            .option(&PermissionOptionKind::RejectOnce)
            .unwrap()
            .option_id
            .as_str(),
        "reject-once"
    );
}

#[test]
fn file_reads_and_writes_stay_in_the_workspace() {
    let dir = scratch("files");
    std::fs::write(dir.join("notes.txt"), "first\nsecond\nthird\n").unwrap();
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")).in_workspace(&dir));
    let (agent, replay) = start(&fixture("files.jsonl"), Some(&dir), Arc::clone(&recorder));
    let capabilities = ClientCapabilities::none()
        .read_files(true)
        .write_files(true);
    init(&agent, capabilities);
    assert_eq!(agent.client_capabilities(), capabilities);
    let session = agent.new_session(NewSessionRequest::new(&dir)).unwrap();
    let done = agent
        .prompt(PromptRequest::text(session.session_id, "Edit"))
        .unwrap();
    assert_eq!(done.stop_reason, StopReason::EndTurn);
    replay.finish();
    assert_eq!(
        std::fs::read_to_string(dir.join("out/new.txt")).unwrap(),
        "{\n  \"debug\": true\n}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn capabilities_not_offered_are_refused() {
    let recorder =
        Arc::new(Recorder::new(Answer::Choose("unused")).in_workspace(&scratch("refused")));
    let (agent, replay) = start(&fixture("refused.jsonl"), None, Arc::clone(&recorder));
    init(&agent, ClientCapabilities::none());
    replay.finish();
}

#[test]
fn cancelling_answers_the_open_permission_request() {
    let recorder = Arc::new(Recorder::new(Answer::BlockThen("allow")));
    let asked = recorder.on_ask();
    let (agent, replay) = start(&fixture("cancel.jsonl"), None, Arc::clone(&recorder));
    let agent = Arc::new(agent);
    init(&agent, ClientCapabilities::none());
    let session = agent
        .new_session(NewSessionRequest::new("/home/user/project"))
        .unwrap();
    let id = session.session_id.clone();

    let turn = agent
        .start_prompt(PromptRequest::text(id.clone(), "Run the tests"))
        .unwrap();
    // The user cancels while the permission prompt is open.
    asked.recv_timeout(Duration::from_secs(10)).unwrap();
    let canceller = Arc::clone(&agent);
    thread::spawn(move || canceller.cancel(&id).unwrap())
        .join()
        .unwrap();

    let done = turn.wait().unwrap();
    assert_eq!(done.stop_reason, StopReason::Cancelled);
    // The prompt closes late; its answer must not reach the agent (the
    // transcript ends with a quiet period).
    recorder.release();
    replay.finish();
    let updates = recorder.updates();
    let SessionUpdate::ToolCallUpdate(update) = updates.last().unwrap() else {
        unreachable!()
    };
    assert_eq!(update.status, Some(ToolCallStatus::Failed));
}

#[test]
fn errors_from_the_agent() {
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")));
    let (agent, replay) = start(&fixture("errors.jsonl"), None, Arc::clone(&recorder));
    let init = init(&agent, ClientCapabilities::none());
    assert_eq!(init.auth_methods.len(), 2);
    assert_eq!(init.auth_methods[1].kind.as_deref(), Some("terminal"));

    let refused = agent.new_session(NewSessionRequest::new("/w")).unwrap_err();
    let Error::Rpc(rpc) = &refused else {
        panic!("{refused}")
    };
    assert!(rpc.is_auth_required());
    agent.authenticate("api-key").unwrap();
    let session = agent.new_session(NewSessionRequest::new("/w")).unwrap();

    let failed = agent
        .prompt(PromptRequest::text(session.session_id, "Hi"))
        .unwrap_err();
    let Error::Rpc(rpc) = &failed else {
        panic!("{failed}")
    };
    assert_eq!(rpc.code, RpcError::INTERNAL_ERROR);
    assert_eq!(rpc.data.as_ref().unwrap()["details"], "model overloaded");
    assert!(failed.to_string().contains("Internal error"));
    replay.finish();

    // The transcript is over: the agent closed its end.
    let errors = recorder.errors.lock().unwrap();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(errors[0].contains("unknown request"), "{errors:?}");
    drop(errors);
    for _ in 0..200 {
        if agent.is_closed() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(agent.is_closed());
    assert!(matches!(
        agent.new_session(NewSessionRequest::new("/w")),
        Err(Error::Closed)
    ));
}

#[test]
fn an_unsupported_version_is_refused() {
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")));
    let (agent, replay) = start(&fixture("version.jsonl"), None, recorder);
    let error = agent
        .initialize(InitializeRequest::new(
            ClientCapabilities::none(),
            client_info(),
        ))
        .unwrap_err();
    assert!(
        matches!(
            error,
            Error::UnsupportedVersion {
                offered: 1,
                answered: 2
            }
        ),
        "{error}"
    );
    replay.finish();
}

#[cfg(unix)]
#[test]
fn terminals_run_inside_the_workspace() {
    let dir = scratch("terminal");
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")).in_workspace(&dir));
    let (agent, replay) = start(
        &fixture("terminal.jsonl"),
        Some(&dir),
        Arc::clone(&recorder),
    );
    init(&agent, ClientCapabilities::none().terminals(true));
    replay.finish();
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn terminal_output_holds_both_streams() {
    let dir = scratch("terminal-output");
    let terminals = Terminals::new(Workspace::new(&dir).unwrap());
    let created = terminals
        .create(&CreateTerminalRequest {
            session_id: "s".into(),
            command: "sh".into(),
            args: vec![
                "-c".into(),
                "printf 'hello '; printf 'world' >&2; echo $ACP_TEST".into(),
            ],
            env: vec![EnvVariable::new("ACP_TEST", "set")],
            ..CreateTerminalRequest::default()
        })
        .unwrap();
    let request = TerminalRequest {
        session_id: "s".into(),
        terminal_id: created.terminal_id,
    };
    let exit = terminals.wait_for_exit(&request).unwrap();
    assert_eq!(exit.exit_code, Some(0));
    let output = terminals.output(&request).unwrap();
    for part in ["hello", "world", "set"] {
        assert!(output.output.contains(part), "{:?}", output.output);
    }
    terminals.release(&request).unwrap();
    let _ = std::fs::remove_dir_all(dir);
}

/// A real child process on stdio: a shell script that answers
/// `initialize` and writes a log line to standard error.
#[cfg(unix)]
#[test]
fn a_spawned_agent_speaks_on_stdio() {
    let script = r#"read line
echo "starting up" >&2
printf '%s\n' '{"jsonrpc":"2.0","id":0,"result":{"protocolVersion":1,"agentInfo":{"name":"sh-agent","version":"0.1"}}}'
read rest || exit 0"#;
    let mut command = std::process::Command::new("sh");
    command.arg("-c").arg(script);
    let recorder = Arc::new(Recorder::new(Answer::Choose("unused")));
    let agent = Connection::spawn(command, Arc::clone(&recorder)).unwrap();
    let init = init(&agent, ClientCapabilities::none());
    assert_eq!(init.agent_info.unwrap().name, "sh-agent");
    let status = agent.shutdown(Duration::from_secs(5)).unwrap().unwrap();
    assert!(status.success());
    for _ in 0..200 {
        if !recorder.logs.lock().unwrap().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(*recorder.logs.lock().unwrap(), ["starting up"]);
}

//! Drives a real ACP agent through one prompt turn, if one is installed.
//!
//! ```sh
//! RICH_ACP_AGENT="npx @zed-industries/claude-code-acp" \
//! RICH_ACP_PROMPT="What does this crate do? Answer in two sentences." \
//!     cargo run -p rs-rich-acp --example agent
//! ```
//!
//! - `RICH_ACP_AGENT`: the agent's command line, split on spaces. Unset,
//!   the example says so and exits successfully.
//! - `RICH_ACP_PROMPT`: the prompt (a short default otherwise).
//! - `RICH_ACP_RECORD`: a file to record the conversation in, one step per
//!   line in the format of `tests/fixtures/*.jsonl`, to make a transcript
//!   test from.
//! - `RICH_ACP_ALLOW=1`: answer permission requests "allow once" without
//!   asking (it asks on the terminal otherwise).
//!
//! The session runs in the current directory, with file reads offered and
//! kept inside it; no writes and no terminals.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::process::{Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rich_acp::*;
use serde_json::{Value, json};

struct Cli {
    workspace: Workspace,
    allow: bool,
}

impl Client for Cli {
    fn session_update(&self, notification: SessionNotification) {
        match notification.update {
            SessionUpdate::AgentMessageChunk(chunk) => {
                print!("{}", chunk.content.as_text().unwrap_or(""))
            }
            SessionUpdate::AgentThoughtChunk(chunk) => {
                eprint!("\x1b[2m{}\x1b[0m", chunk.content.as_text().unwrap_or(""))
            }
            SessionUpdate::ToolCall(call) => {
                println!("\n[tool] {} ({}, {})", call.title, call.kind, call.status)
            }
            SessionUpdate::ToolCallUpdate(update) => {
                if let Some(status) = update.status {
                    println!("[tool {}] {status}", update.tool_call_id);
                }
            }
            SessionUpdate::Plan(plan) => {
                println!("\n[plan]");
                for entry in plan.entries {
                    println!("  - [{}] {}", entry.status, entry.content);
                }
            }
            other => eprintln!("\n[{}]", other.kind()),
        }
        let _ = io::stdout().flush();
    }

    fn request_permission(
        &self,
        request: RequestPermissionRequest,
    ) -> Result<RequestPermissionResponse, RpcError> {
        println!(
            "\n[permission] the agent asks: {}",
            request.tool_call.title.as_deref().unwrap_or("(untitled)")
        );
        for (n, option) in request.options.iter().enumerate() {
            println!("  {}. {} ({})", n + 1, option.name, option.kind);
        }
        if self.allow
            && let Some(option) = request.option(&PermissionOptionKind::AllowOnce)
        {
            return Ok(RequestPermissionResponse::selected(
                option.option_id.clone(),
            ));
        }
        print!("choose a number (anything else cancels): ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        let _ = io::stdin().read_line(&mut line);
        Ok(
            match line
                .trim()
                .parse::<usize>()
                .ok()
                .and_then(|n| request.options.get(n.wrapping_sub(1)))
            {
                Some(option) => RequestPermissionResponse::selected(option.option_id.clone()),
                None => RequestPermissionResponse::cancelled(),
            },
        )
    }

    fn read_text_file(
        &self,
        request: ReadTextFileRequest,
    ) -> Result<ReadTextFileResponse, RpcError> {
        println!("\n[read] {}", request.path.display());
        fs::read_text_file(&self.workspace, &request)
    }

    fn agent_log(&self, line: &str) {
        eprintln!("\x1b[2m[agent] {line}\x1b[0m");
    }

    fn protocol_error(&self, error: &Error) {
        eprintln!("[protocol] {error}");
    }
}

/// Records each line that passes as a transcript step.
struct Tee<S> {
    inner: S,
    side: &'static str,
    record: Option<Arc<Mutex<File>>>,
    line: Vec<u8>,
}

impl<S> Tee<S> {
    fn push(&mut self, bytes: &[u8]) {
        let Some(record) = &self.record else { return };
        for &byte in bytes {
            if byte != b'\n' {
                self.line.push(byte);
                continue;
            }
            let text = String::from_utf8_lossy(&self.line).into_owned();
            let step = match serde_json::from_str::<Value>(&text) {
                Ok(message) => json!({ self.side: message }),
                Err(_) => json!({ "raw": text }),
            };
            let _ = writeln!(record.lock().unwrap(), "{step}");
            self.line.clear();
        }
    }
}

impl<R: Read> Read for Tee<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.push(&buf[..n]);
        Ok(n)
    }
}

impl<W: Write> Write for Tee<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.push(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn main() -> ExitCode {
    let Ok(command_line) = std::env::var("RICH_ACP_AGENT") else {
        println!("RICH_ACP_AGENT is not set: no agent to drive, skipping.");
        println!(
            "Set it to an ACP agent's command line, e.g. \"npx @zed-industries/claude-code-acp\"."
        );
        return ExitCode::SUCCESS;
    };
    let mut words = command_line.split_whitespace();
    let Some(program) = words.next() else {
        println!("RICH_ACP_AGENT is empty: skipping.");
        return ExitCode::SUCCESS;
    };
    match run(program, words.collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Error::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            println!("{program} is not installed: skipping.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(program: &str, args: Vec<&str>) -> Result<(), Error> {
    let cwd = std::env::current_dir()?;
    let client = Cli {
        workspace: Workspace::new(&cwd)?,
        allow: std::env::var("RICH_ACP_ALLOW").is_ok_and(|v| v == "1"),
    };
    let record = match std::env::var("RICH_ACP_RECORD") {
        Ok(path) => Some(Arc::new(Mutex::new(File::create(path)?))),
        Err(_) => None,
    };

    // Spawned by hand rather than with `Connection::spawn`, so the streams
    // can be recorded.
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stderr = child.stderr.take().expect("piped");
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            eprintln!("\x1b[2m[agent] {line}\x1b[0m");
        }
    });
    let reader = Tee {
        inner: child.stdout.take().expect("piped"),
        side: "agent",
        record: record.clone(),
        line: Vec::new(),
    };
    let writer = Tee {
        inner: child.stdin.take().expect("piped"),
        side: "client",
        record,
        line: Vec::new(),
    };
    let agent = Connection::from_streams(reader, writer, client)?;

    let init = agent.initialize(InitializeRequest::new(
        ClientCapabilities::none().read_files(true),
        Implementation::new("rich-acp-example", env!("CARGO_PKG_VERSION")),
    ))?;
    match &init.agent_info {
        Some(info) => println!(
            "connected to {} {} (ACP {})",
            info.display_name(),
            info.version,
            init.protocol_version
        ),
        None => println!("connected (ACP {})", init.protocol_version),
    }
    let session = match agent.new_session(NewSessionRequest::new(&cwd)) {
        Err(Error::Rpc(error)) if error.is_auth_required() => {
            let Some(method) = init.auth_methods.first() else {
                return Err(Error::Rpc(error));
            };
            println!("authenticating with {}", method.name);
            agent.authenticate(method.id.clone())?;
            agent.new_session(NewSessionRequest::new(&cwd))?
        }
        other => other?,
    };
    // The session id resumes the conversation (`session/load`): it is not
    // printed, so it stays out of terminal scrollback and CI logs.
    println!("session started");
    let prompt = std::env::var("RICH_ACP_PROMPT").unwrap_or_else(|_| {
        "Say hello and name the files in this directory's top level.".to_owned()
    });
    println!("> {prompt}");
    let done = agent.prompt(PromptRequest::text(session.session_id, prompt))?;
    println!("\n[turn ended: {}]", done.stop_reason);

    drop(agent);
    let _ = child.wait_timeout_compat(Duration::from_secs(3));
    Ok(())
}

/// Waits a moment for the agent to exit after its input closed, then kills
/// it.
trait WaitCompat {
    fn wait_timeout_compat(&mut self, grace: Duration) -> io::Result<()>;
}

impl WaitCompat for std::process::Child {
    fn wait_timeout_compat(&mut self, grace: Duration) -> io::Result<()> {
        let deadline = std::time::Instant::now() + grace;
        while std::time::Instant::now() < deadline {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.kill()?;
        self.wait().map(drop)
    }
}

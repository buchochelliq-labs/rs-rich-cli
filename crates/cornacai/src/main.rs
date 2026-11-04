//! `cornacai`: a terminal client for coding agents. See `cornacai --help`
//! and `docs/design/cornacai.md`.

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use cornacai::app::{Launch, Options, app};
use cornacai::config::{
    AgentChoice, Config, agent_choices, config_path, marketplace_agents, rich_plugin_dir,
};
use cornacai::store::{SessionStore, ago, now};

const USAGE: &str = "\
Usage: cornacai [OPTIONS] [-- AGENT COMMAND…]

A terminal client for coding agents over the Agent Client Protocol.

Options:
  --agent CMD       the agent's launch command (split on spaces); or give it
                    after `--`. Saved as the default the first time.
  --cwd DIR         the workspace (default: the current directory)
  --shell CMD       the shell pane's command (default: $SHELL, else sh)
  --no-read         do not let the agent read files through Cornacai
  --no-write        do not let the agent write files through Cornacai
  --no-terminal     do not let the agent run commands through Cornacai
  --pty             run the agent's commands on a PTY (else as configured;
                    pipes when a PTY cannot be opened)
  --resume ID       reopen session ID (session/load) instead of a new one
  --sessions        list the sessions kept to reopen, newest first
  --agents          list the agents Cornacai knows, and which are installed
  -h, --help        this help
  -V, --version     the version

Keys: Enter sends, Alt+Enter adds a line, @ names a file, Esc cancels the
turn, Ctrl+N opens a session, Ctrl+O reopens an earlier one, Alt+N and
Alt+P switch sessions (Alt+1…9 picks one), Ctrl+T the shell pane, F2 the
agent's log, Ctrl+Q quits.

The configuration is cornacai/config.toml in $XDG_CONFIG_HOME (else
~/.config); the sessions kept are cornacai/sessions.jsonl in
$XDG_STATE_HOME (else ~/.local/state).";

#[derive(Default)]
struct Args {
    agent: Option<Vec<String>>,
    cwd: Option<PathBuf>,
    shell: Option<Vec<String>>,
    no_read: bool,
    no_write: bool,
    no_terminal: bool,
    pty: bool,
    resume: Option<String>,
}

enum Run {
    Help,
    Version,
    Agents,
    Sessions,
    Start(Args),
}

fn parse(args: &[String]) -> Result<Run, String> {
    let mut parsed = Args::default();
    let mut it = args.iter();
    let words = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(Run::Help),
            "-V" | "--version" => return Ok(Run::Version),
            "--agents" => return Ok(Run::Agents),
            "--sessions" => return Ok(Run::Sessions),
            "--pty" => parsed.pty = true,
            "--resume" => parsed.resume = Some(value("--resume")?),
            "--agent" => parsed.agent = Some(words(&value("--agent")?)),
            "--cwd" => parsed.cwd = Some(PathBuf::from(value("--cwd")?)),
            "--shell" => parsed.shell = Some(words(&value("--shell")?)),
            "--no-read" => parsed.no_read = true,
            "--no-write" => parsed.no_write = true,
            "--no-terminal" => parsed.no_terminal = true,
            "--" => {
                parsed.agent = Some(it.by_ref().cloned().collect());
                break;
            }
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    if parsed.agent.as_ref().is_some_and(Vec::is_empty) {
        return Err("the agent command is empty".into());
    }
    Ok(Run::Start(parsed))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Run::Help) => println!("{USAGE}"),
        Ok(Run::Version) => println!("cornacai {}", env!("CARGO_PKG_VERSION")),
        Ok(Run::Agents) => agents(),
        Ok(Run::Sessions) => sessions(),
        Ok(Run::Start(args)) => {
            if let Err(error) = start(args) {
                eprintln!("cornacai: {error}");
                return ExitCode::FAILURE;
            }
        }
        Err(error) => {
            eprintln!("cornacai: {error}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    }
    ExitCode::SUCCESS
}

/// The agents to offer: built in, then those recorded from the
/// marketplace.
fn choices() -> Vec<AgentChoice> {
    let listed = rich_plugin_dir()
        .map(|dir| marketplace_agents(&dir))
        .unwrap_or_default();
    agent_choices(&listed)
}

fn agents() {
    for agent in choices() {
        let mark = if agent.installed() {
            "installed"
        } else {
            "not found"
        };
        let from = match &agent.listing {
            Some((id, tier)) => format!(" (marketplace: {id}, {tier})"),
            None => String::new(),
        };
        println!(
            "{:<28} {:<28} {mark}{from}\n{:<28} install: {}",
            agent.name,
            agent.command.join(" "),
            "",
            agent.install
        );
    }
    println!(
        "\nMore agents are listed in the marketplace: rich plugin search --kind agent; \
         `rich plugin install ID` records one here."
    );
}

fn sessions() {
    let Some(path) = SessionStore::default_path() else {
        println!("No state directory: set HOME or XDG_STATE_HOME.");
        return;
    };
    let kept = SessionStore::new(path).list();
    if kept.is_empty() {
        println!("No sessions are kept yet.");
        return;
    }
    let now = now();
    for session in kept {
        let title = if session.title.is_empty() {
            "untitled"
        } else {
            &session.title
        };
        println!(
            "{}  {title}  ({})\n    {} in {}",
            session.id,
            ago(session.last_used, now),
            session.agent,
            session.cwd.display()
        );
    }
    println!(
        "\nReopen one with: cornacai --resume ID (with the same agent, in the same directory)"
    );
}

fn start(args: Args) -> Result<(), String> {
    let path = config_path();
    let config = match &path {
        Some(path) => Config::load(path)?,
        None => Config::default(),
    };
    let cwd = match args.cwd {
        Some(dir) => dir,
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let cwd = cwd
        .canonicalize()
        .map_err(|e| format!("{}: {e}", cwd.display()))?;
    let launch = match fake_agent() {
        Some(launch) => launch,
        None => Launch::Command(choose_agent(args.agent, &config, path.as_deref())?),
    };
    let shell = args.shell.or(config.shell).unwrap_or_else(|| {
        vec![
            std::env::var("SHELL")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "sh".into()),
        ]
    });
    let mut options = Options::new(launch, cwd)
        .shell_command(shell)
        .pty(args.pty || config.pty);
    if let Some(path) = SessionStore::default_path() {
        options = options.session_store(SessionStore::new(path));
    }
    if let Some(id) = args.resume {
        options = options.resume(id);
    }
    options.read = config.read && !args.no_read;
    options.write = config.write && !args.no_write;
    options.terminal = config.terminal && !args.no_terminal;
    let (app, _handle) = app(options);
    app.title("Cornacai").run().map_err(|e| e.to_string())
}

/// The agent to start: the one given, the configured one, or one picked
/// from those installed (and saved).
fn choose_agent(
    given: Option<Vec<String>>,
    config: &Config,
    path: Option<&std::path::Path>,
) -> Result<Vec<String>, String> {
    if let Some(agent) = given {
        if config.agent.is_none()
            && let Some(path) = path
        {
            let _ = Config::save_agent(path, &agent);
        }
        return Ok(agent);
    }
    if let Some(agent) = &config.agent {
        return Ok(agent.clone());
    }
    let all = choices();
    let found: Vec<&AgentChoice> = all.iter().filter(|a| a.installed()).collect();
    let chosen = match found.len() {
        0 => {
            let mut message = String::from(
                "no agent is configured and none of the known ones is installed. Install one, \
                 or give a command with --agent:\n",
            );
            for agent in &all {
                message.push_str(&format!("  {:<28} {}\n", agent.name, agent.install));
            }
            message
                .push_str("More are listed in the marketplace: rich plugin search --kind agent\n");
            return Err(message);
        }
        1 => found[0],
        _ => {
            println!("Which agent?");
            for (i, agent) in found.iter().enumerate() {
                println!("  {}. {} ({})", i + 1, agent.name, agent.command.join(" "));
            }
            print!("> ");
            let _ = std::io::stdout().flush();
            let mut line = String::new();
            std::io::stdin()
                .lock()
                .read_line(&mut line)
                .map_err(|e| e.to_string())?;
            let n: usize = line.trim().parse().map_err(|_| "no agent chosen")?;
            *found.get(n.wrapping_sub(1)).ok_or("no agent chosen")?
        }
    };
    let agent: Vec<String> = chosen.command.clone();
    if let Some(path) = path {
        let _ = Config::save_agent(path, &agent);
    }
    Ok(agent)
}

/// With the test-only `fake-agent` feature: the transcript
/// `CORNACAI_FAKE_AGENT` names, replayed instead of an agent.
#[cfg(feature = "fake-agent")]
fn fake_agent() -> Option<Launch> {
    let path = std::env::var_os("CORNACAI_FAKE_AGENT")?;
    let transcript = rich_acp::replay::Transcript::read(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", PathBuf::from(&path).display()));
    Some(Launch::Replay {
        transcript,
        timeout: None,
    })
}

#[cfg(not(feature = "fake-agent"))]
fn fake_agent() -> Option<Launch> {
    None
}

//! The configuration file, `cornacai/config.toml` in the user's config
//! directory (`$XDG_CONFIG_HOME`, else `~/.config`), and the agents
//! Cornacai knows how to start: built in, and recorded from the
//! marketplace's `agent` listings (`rich plugin install ID`).
//!
//! ```toml
//! # The agent's launch command: the program and its arguments.
//! agent = ["claude-agent-acp"]
//! # The shell pane's command (default: $SHELL, else sh).
//! shell = ["bash", "-l"]
//!
//! # What the agent may ask the client to do. Writes are always shown as
//! # a diff to accept or reject; each terminal command is asked about.
//! [capabilities]
//! read = true
//! write = true
//! terminal = true
//!
//! # The agent's commands on a PTY (colour, programs that want a
//! # terminal) instead of pipes; pipes when a PTY cannot be opened.
//! [terminal]
//! pty = true
//! ```

use std::path::{Path, PathBuf};

use rich_marketplace::{AgentList, ListedAgent};

/// The settings the app runs with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    /// The agent's launch command, if one was chosen.
    pub agent: Option<Vec<String>>,
    /// The shell pane's command.
    pub shell: Option<Vec<String>>,
    /// Offer `fs/read_text_file`.
    pub read: bool,
    /// Offer `fs/write_text_file` (each write reviewed as a diff).
    pub write: bool,
    /// Offer `terminal/*` (each command asked about).
    pub terminal: bool,
    /// Run the agent's commands on a PTY.
    pub pty: bool,
}

impl Default for Config {
    /// No agent, the default shell, and every capability on: reads
    /// answered, writes and terminals asking.
    fn default() -> Config {
        Config {
            agent: None,
            shell: None,
            read: true,
            write: true,
            terminal: true,
            pty: false,
        }
    }
}

impl Config {
    /// Reads a configuration; unknown keys are ignored.
    pub fn parse(text: &str) -> Result<Config, String> {
        let table: toml::Table = text.parse().map_err(|e: toml::de::Error| e.to_string())?;
        let mut config = Config::default();
        let words = |key: &str, value: &toml::Value| -> Result<Vec<String>, String> {
            match value {
                toml::Value::String(line) => {
                    Ok(line.split_whitespace().map(str::to_owned).collect())
                }
                toml::Value::Array(items) => items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(str::to_owned)
                            .ok_or_else(|| format!("`{key}`: every argument is a string"))
                    })
                    .collect(),
                _ => Err(format!("`{key}`: a command, as a string or a list")),
            }
        };
        if let Some(agent) = table.get("agent") {
            config.agent = Some(words("agent", agent)?).filter(|a| !a.is_empty());
        }
        if let Some(shell) = table.get("shell") {
            config.shell = Some(words("shell", shell)?).filter(|s| !s.is_empty());
        }
        if let Some(caps) = table.get("capabilities").and_then(toml::Value::as_table) {
            for (key, slot) in [
                ("read", &mut config.read),
                ("write", &mut config.write),
                ("terminal", &mut config.terminal),
            ] {
                if let Some(value) = caps.get(key) {
                    *slot = value
                        .as_bool()
                        .ok_or_else(|| format!("`capabilities.{key}`: true or false"))?;
                }
            }
        }
        if let Some(terminal) = table.get("terminal").and_then(toml::Value::as_table)
            && let Some(value) = terminal.get("pty")
        {
            config.pty = value.as_bool().ok_or("`terminal.pty`: true or false")?;
        }
        Ok(config)
    }

    /// Reads the file at `path`; a missing file is the default.
    pub fn load(path: &Path) -> Result<Config, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Config::parse(&text).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Saves `agent` as the agent in the file at `path`, keeping the rest
    /// of the file as it is.
    pub fn save_agent(path: &Path, agent: &[String]) -> std::io::Result<()> {
        let old = std::fs::read_to_string(path).unwrap_or_default();
        let line = format!(
            "agent = [{}]",
            agent
                .iter()
                .map(|a| toml::Value::String(a.clone()).to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let mut replaced = false;
        let mut lines: Vec<String> = old
            .lines()
            .map(|l| {
                if !replaced && l.trim_start().starts_with("agent") && l.contains('=') {
                    replaced = true;
                    line.clone()
                } else {
                    l.to_string()
                }
            })
            .collect();
        if !replaced {
            lines.insert(0, line);
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, lines.join("\n") + "\n")
    }
}

/// `cornacai/config.toml` in the user's config directory.
pub fn config_path() -> Option<PathBuf> {
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(dir.join("cornacai").join("config.toml"))
}

/// An agent Cornacai knows how to start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownAgent {
    /// Its name, for people.
    pub name: &'static str,
    /// Its launch command.
    pub command: &'static [&'static str],
    /// How to install it.
    pub install: &'static str,
}

/// The agents looked for on `PATH` when none is configured. The
/// marketplace's `agent` listings are the longer list
/// (`rich plugin search --kind agent`).
pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        name: "Claude Agent (ACP adapter)",
        command: &["claude-agent-acp"],
        install: "npm install -g @agentclientprotocol/claude-agent-acp",
    },
    // The same adapter under its old name, which still runs; its package
    // is deprecated in favour of the one above.
    KnownAgent {
        name: "Claude Code (old ACP adapter)",
        command: &["claude-code-acp"],
        install: "npm install -g @agentclientprotocol/claude-agent-acp (it replaces \
                  @zed-industries/claude-code-acp)",
    },
    KnownAgent {
        name: "Gemini CLI",
        command: &["gemini", "--experimental-acp"],
        install: "npm install -g @google/gemini-cli",
    },
    KnownAgent {
        name: "Codex (ACP adapter)",
        command: &["codex-acp"],
        install: "see github.com/zed-industries/codex-acp",
    },
];

/// rs-rich-cli's plugin install directory, where `rich plugin install`
/// records the marketplace's agents: `RICH_PLUGIN_DIR`, else
/// `%LOCALAPPDATA%\\rich\\plugins` on Windows, else
/// `$XDG_DATA_HOME/rich/plugins`, else `~/.local/share/rich/plugins`.
pub fn rich_plugin_dir() -> Option<PathBuf> {
    let env = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    if let Some(dir) = env("RICH_PLUGIN_DIR") {
        return Some(PathBuf::from(dir));
    }
    if cfg!(windows)
        && let Some(local) = env("LOCALAPPDATA")
    {
        return Some(PathBuf::from(local).join("rich").join("plugins"));
    }
    if let Some(data) = env("XDG_DATA_HOME").map(PathBuf::from)
        && data.is_absolute()
    {
        return Some(data.join("rich").join("plugins"));
    }
    env("HOME")
        .or_else(|| env("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".local/share/rich/plugins"))
}

/// The agents recorded from the marketplace in `dir`
/// ([`rich_plugin_dir`]): none when there is no record or it does not
/// read.
pub fn marketplace_agents(dir: &Path) -> Vec<ListedAgent> {
    AgentList::load(dir)
        .map(|list| list.agents().to_vec())
        .unwrap_or_default()
}

/// An agent to offer: built in, or recorded from the marketplace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentChoice {
    /// Its name, for people.
    pub name: String,
    /// Its launch command.
    pub command: Vec<String>,
    /// How to install it.
    pub install: String,
    /// The marketplace listing it came from, if one did: its id and tier.
    pub listing: Option<(String, String)>,
}

impl AgentChoice {
    /// Whether its program is installed.
    pub fn installed(&self) -> bool {
        self.command.first().is_some_and(|program| on_path(program))
    }
}

/// Every agent Cornacai can offer: [`KNOWN_AGENTS`], then the marketplace's
/// `listed` agents whose command is not one of those.
pub fn agent_choices(listed: &[ListedAgent]) -> Vec<AgentChoice> {
    let mut choices: Vec<AgentChoice> = KNOWN_AGENTS
        .iter()
        .map(|agent| AgentChoice {
            name: agent.name.to_string(),
            command: agent.command.iter().map(|s| s.to_string()).collect(),
            install: agent.install.to_string(),
            listing: None,
        })
        .collect();
    for agent in listed {
        let command = agent.argv();
        if choices.iter().any(|c| c.command == command) {
            continue;
        }
        choices.push(AgentChoice {
            name: crate::clean::clean(&agent.name),
            command,
            install: crate::clean::clean(&agent.install),
            listing: Some((agent.id.clone(), agent.tier.as_str().to_string())),
        });
    }
    choices
}

/// Whether `program` is an executable file on `PATH` (or a path to one).
pub fn on_path(program: &str) -> bool {
    find_program(program).is_some()
}

/// The file the system would run for `program`: a path as given, else the
/// first match on `PATH`. On Windows only a name with one of `PATHEXT`'s
/// extensions runs (`.exe`, and npm's `.cmd` shims, which sit beside a
/// shell script of the same name that Windows cannot run).
pub fn find_program(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH").unwrap_or_default();
    let pathext = std::env::var_os("PATHEXT");
    find_in(program, &paths, pathext.as_deref(), cfg!(windows), runnable)
}

/// Whether `path` is a file the system would run: on Unix, one with an
/// execute bit, as the shell's own search skips the others.
fn runnable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// [`find_program`] over the given `PATH` and `PATHEXT`, as on Windows
/// when `windows`.
fn find_in(
    program: &str,
    paths: &std::ffi::OsStr,
    pathext: Option<&std::ffi::OsStr>,
    windows: bool,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let candidates = |base: PathBuf| -> Vec<PathBuf> {
        if !windows {
            return vec![base];
        }
        let has_extension = base
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| !e.is_empty());
        let exts = pathext
            .and_then(|p| p.to_str())
            .unwrap_or(".COM;.EXE;.BAT;.CMD")
            .split(';')
            .filter(|e| !e.is_empty())
            .map(|e| e.to_ascii_lowercase())
            .collect::<Vec<_>>();
        let mut out = Vec::new();
        if has_extension {
            out.push(base.clone());
        }
        for ext in exts {
            let mut name = base.as_os_str().to_owned();
            name.push(&ext);
            out.push(PathBuf::from(name));
        }
        out
    };
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    if program.contains(separators) {
        return candidates(PathBuf::from(program))
            .into_iter()
            .find(|p| is_file(p));
    }
    std::env::split_paths(paths)
        .flat_map(|dir| candidates(dir.join(program)))
        .find(|p| is_file(p))
}

/// The command that starts `argv`: its program found as [`find_program`]
/// finds it, so that on Windows an npm-installed agent (a `.cmd` shim)
/// starts; the program as given when it is not found, for the error.
pub fn agent_command(argv: &[String]) -> Option<std::process::Command> {
    let (program, args) = argv.split_first()?;
    let found = find_program(program);
    let mut command = std::process::Command::new(found.as_deref().unwrap_or(Path::new(program)));
    command.args(args);
    Some(command)
}

/// The [`KNOWN_AGENTS`] whose program is installed.
pub fn installed_agents() -> Vec<KnownAgent> {
    KNOWN_AGENTS
        .iter()
        .filter(|agent| on_path(agent.command[0]))
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [`find_in`] over a fake file system holding `files`.
    fn find(program: &str, path: &[&str], windows: bool, files: &[&str]) -> Option<String> {
        let path = std::env::join_paths(path).unwrap();
        find_in(
            program,
            &path,
            Some(std::ffi::OsStr::new(".COM;.EXE;.BAT;.CMD")),
            windows,
            |p: &Path| files.iter().any(|f| Path::new(f) == p),
        )
        .map(|p| p.display().to_string())
    }

    #[test]
    fn on_windows_an_npm_agent_is_found_by_its_cmd_shim_not_its_shell_script() {
        // npm puts `claude-code-acp` (a shell script), `.cmd` and `.ps1`
        // side by side; Windows runs only the `.cmd`.
        let npm = [
            "/npm/claude-code-acp",
            "/npm/claude-code-acp.cmd",
            "/npm/claude-code-acp.ps1",
        ];
        assert_eq!(
            find("claude-code-acp", &["/bin", "/npm"], true, &npm).as_deref(),
            Some("/npm/claude-code-acp.cmd")
        );
        assert_eq!(
            find(
                "claude-code-acp",
                &["/npm"],
                true,
                &["/npm/claude-code-acp"]
            ),
            None
        );
        assert_eq!(
            find("claude-code-acp", &["/npm"], false, &npm).as_deref(),
            Some("/npm/claude-code-acp")
        );
        assert_eq!(
            find("gemini.exe", &["/bin"], true, &["/bin/gemini.exe"]).as_deref(),
            Some("/bin/gemini.exe")
        );
    }

    #[cfg(unix)]
    #[test]
    fn on_unix_a_file_that_cannot_run_is_passed_over_for_a_later_one() {
        use std::os::unix::fs::PermissionsExt;
        let base = std::env::temp_dir().join(format!("cornacai-path-{}", std::process::id()));
        let (first, second) = (base.join("a"), base.join("b"));
        for (dir, mode) in [(&first, 0o644), (&second, 0o755)] {
            std::fs::create_dir_all(dir).unwrap();
            let file = dir.join("some-agent-acp");
            std::fs::write(&file, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        }
        let path = std::env::join_paths([&first, &second]).unwrap();
        let found = find_in("some-agent-acp", &path, None, false, runnable);
        let _ = std::fs::remove_dir_all(&base);
        assert_eq!(found, Some(second.join("some-agent-acp")));
    }

    #[test]
    fn the_renamed_claude_adapter_is_offered_first() {
        assert_eq!(KNOWN_AGENTS[0].command, ["claude-agent-acp"]);
        assert!(
            KNOWN_AGENTS[0]
                .install
                .contains("@agentclientprotocol/claude-agent-acp")
        );
        assert!(
            KNOWN_AGENTS
                .iter()
                .any(|a| a.command == ["claude-code-acp"])
        );
    }

    #[test]
    fn a_config_reads_commands_and_capabilities() {
        let config = Config::parse(
            "agent = \"gemini --experimental-acp\"\nshell = [\"bash\", \"-l\"]\n\
             [capabilities]\nterminal = false\n",
        )
        .unwrap();
        assert_eq!(
            config.agent.unwrap(),
            ["gemini", "--experimental-acp"].map(String::from)
        );
        assert_eq!(config.shell.unwrap(), ["bash", "-l"].map(String::from));
        assert!(config.read && config.write && !config.terminal);
        assert!(!config.pty);
        assert!(Config::parse("[terminal]\npty = true\n").unwrap().pty);
        assert!(Config::parse("[capabilities]\nread = 1\n").is_err());
        assert!(Config::parse("[terminal]\npty = \"yes\"\n").is_err());
    }

    #[test]
    fn the_marketplaces_agents_follow_the_built_in_ones() {
        let dir = std::env::temp_dir().join(format!("cornacai-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(marketplace_agents(&dir).is_empty());
        std::fs::create_dir_all(&dir).unwrap();
        // As `rich plugin install` writes it: one new agent, one that is
        // a built-in one again (left out), and a broken entry (skipped).
        std::fs::write(
            dir.join("agents.json"),
            r#"{"schema_version": 1, "agents": [
                {"id": "echo-agent", "name": "Echo\u001b[2J Agent", "tier": "community",
                 "version": "1.4.0", "command": "echo-agent", "args": ["--acp"],
                 "install": "npm install -g @example/echo-agent"},
                {"id": "claude", "name": "Claude again", "tier": "reviewed", "version": "1.0.0",
                 "command": "claude-code-acp"},
                {"id": "broken"}
            ]}"#,
        )
        .unwrap();
        let listed = marketplace_agents(&dir);
        assert_eq!(listed.len(), 2);
        let choices = agent_choices(&listed);
        assert_eq!(choices.len(), KNOWN_AGENTS.len() + 1);
        let echo = choices.last().unwrap();
        assert_eq!(echo.command, ["echo-agent", "--acp"]);
        assert_eq!(
            echo.listing,
            Some(("echo-agent".to_string(), "community".to_string()))
        );
        // The listing's text is shown, so it is cleaned.
        assert!(!echo.name.contains('\u{1b}'), "{:?}", echo.name);
        assert!(!echo.installed());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn saving_the_agent_keeps_the_rest() {
        let dir = std::env::temp_dir().join(format!("cornacai-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            &path,
            "# mine\nagent = [\"old\"]\n[capabilities]\nwrite = false\n",
        )
        .unwrap();
        Config::save_agent(&path, &["new".into(), "--acp".into()]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# mine\nagent = [\"new\", \"--acp\"]\n"),
            "{text}"
        );
        assert!(!Config::load(&path).unwrap().write);
        std::fs::remove_dir_all(&dir).ok();
    }
}

//! The configuration file, `cornacai/config.toml` in the user's config
//! directory (`$XDG_CONFIG_HOME`, else `~/.config`), and the agents
//! Cornacai knows how to start.
//!
//! ```toml
//! # The agent's launch command: the program and its arguments.
//! agent = ["claude-code-acp"]
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
        name: "Claude Code (ACP adapter)",
        command: &["claude-code-acp"],
        install: "npm install -g @zed-industries/claude-code-acp",
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

/// Whether `program` is an executable file on `PATH` (or a path to one).
pub fn on_path(program: &str) -> bool {
    let runnable = |path: &Path| path.is_file();
    if program.contains(std::path::MAIN_SEPARATOR) {
        return runnable(Path::new(program));
    }
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            runnable(&dir.join(program))
                || (cfg!(windows) && runnable(&dir.join(format!("{program}.exe"))))
        })
    })
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

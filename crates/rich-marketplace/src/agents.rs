//! The agents taken from the marketplace: `agents.json` in the install
//! directory, beside the lockfile.
//!
//! An `agent` listing is never installed: nothing is downloaded and no
//! installer runs. "Installing" one records its verified launch command
//! here, with its install hint for the user to follow; an ACP client
//! (Cornacai) reads the file to offer the agents whose program is on
//! `PATH`. The file holds only what came from a listing whose signature
//! and hash were checked: a client starts the command as written, so it
//! is written by [`AgentList::add`] from a [`Listing`] and nothing else.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::listing::{Kind, ListedVersion, Listing, Tier};
use crate::util::parse_json;

/// The file's name in the install directory.
pub const AGENTS_FILE_NAME: &str = "agents.json";

/// The format this release writes and reads.
pub const AGENTS_SCHEMA_VERSION: u64 = 1;

/// An agent recorded from its listing.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ListedAgent {
    /// The listing's id.
    pub id: String,
    /// Its name, for people.
    pub name: String,
    /// One line on it.
    pub description: String,
    /// The listing's tier.
    pub tier: Tier,
    /// The listed version recorded.
    pub version: String,
    /// The program, a bare name looked for on `PATH`.
    pub command: String,
    /// Its arguments.
    pub args: Vec<String>,
    /// The environment variables it reads (names only).
    pub env: Vec<String>,
    /// How to install the program.
    pub install: String,
    /// Its homepage, else its repository.
    pub homepage: String,
}

impl ListedAgent {
    /// The launch command: the program, then its arguments.
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.command.clone())
            .chain(self.args.iter().cloned())
            .collect()
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "description": self.description,
            "tier": self.tier.as_str(),
            "version": self.version,
            "command": self.command,
            "args": self.args,
            "env": self.env,
            "install": self.install,
            "homepage": self.homepage,
        })
    }

    fn from_json(value: &Value) -> Option<ListedAgent> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let list = |key: &str| -> Option<Vec<String>> {
            match value.get(key) {
                None => Some(Vec::new()),
                Some(Value::Array(items)) => items
                    .iter()
                    .map(|item| item.as_str().map(str::to_owned))
                    .collect(),
                Some(_) => None,
            }
        };
        Some(ListedAgent {
            id: text("id").filter(|id| rich_plugin_api::is_valid_name(id))?,
            name: text("name")?,
            description: text("description").unwrap_or_default(),
            tier: Tier::parse(&text("tier")?)?,
            version: text("version")?,
            command: text("command").filter(|c| !c.is_empty())?,
            args: list("args")?,
            env: list("env")?,
            install: text("install").unwrap_or_default(),
            homepage: text("homepage").unwrap_or_default(),
        })
    }
}

/// The agents recorded in an install directory, in the order added.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AgentList {
    agents: Vec<ListedAgent>,
}

impl AgentList {
    /// An empty list.
    pub fn new() -> AgentList {
        AgentList::default()
    }

    /// The file in `dir`.
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(AGENTS_FILE_NAME)
    }

    /// Reads `dir`'s list; a missing file is an empty list. An entry that
    /// does not read is skipped; a file of another format is refused.
    pub fn load(dir: &Path) -> io::Result<AgentList> {
        let bytes = match std::fs::read(AgentList::path(dir)) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(AgentList::new()),
            Err(e) => return Err(e),
        };
        let invalid = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
        let value = parse_json(&bytes).map_err(|_| invalid("agents.json is not JSON"))?;
        match value.get("schema_version").and_then(Value::as_u64) {
            Some(AGENTS_SCHEMA_VERSION) => {}
            _ => return Err(invalid("agents.json is of another format")),
        }
        let agents = value
            .get("agents")
            .and_then(Value::as_array)
            .map(|items| items.iter().filter_map(ListedAgent::from_json).collect())
            .unwrap_or_default();
        Ok(AgentList { agents })
    }

    /// Writes the list to `dir` (written aside, then renamed).
    pub fn save(&self, dir: &Path) -> io::Result<()> {
        crate::lockfile::create_private_dir_all(dir)?;
        let value = json!({
            "schema_version": AGENTS_SCHEMA_VERSION,
            "agents": self.agents.iter().map(ListedAgent::to_json).collect::<Vec<_>>(),
        });
        let mut text = serde_json::to_string_pretty(&value).map_err(io::Error::other)?;
        text.push('\n');
        let path = AgentList::path(dir);
        let aside = dir.join(format!(".{AGENTS_FILE_NAME}.{}", std::process::id()));
        std::fs::write(&aside, text)?;
        std::fs::rename(&aside, &path)
    }

    /// Every agent recorded.
    pub fn agents(&self) -> &[ListedAgent] {
        &self.agents
    }

    /// The agent `id`, if recorded.
    pub fn get(&self, id: &str) -> Option<&ListedAgent> {
        self.agents.iter().find(|a| a.id == id)
    }

    /// Records `version` of the agent `listing` (read through a verified
    /// index), replacing an earlier record of it. `None` when the listing
    /// is not an agent's.
    pub fn add(&mut self, listing: &Listing, version: &ListedVersion) -> Option<&ListedAgent> {
        if listing.kind != Kind::Agent {
            return None;
        }
        let info = listing.agent.as_ref()?;
        let agent = ListedAgent {
            id: listing.id.clone(),
            name: listing.name.clone(),
            description: listing.description.clone(),
            tier: listing.tier,
            version: version.version.to_string(),
            command: info.command.clone(),
            args: info.args.clone(),
            env: info.env.clone(),
            install: info.install.clone(),
            homepage: listing
                .homepage
                .clone()
                .unwrap_or_else(|| listing.repository.clone()),
        };
        match self.agents.iter().position(|a| a.id == agent.id) {
            Some(at) => {
                self.agents[at] = agent;
                self.agents.get(at)
            }
            None => {
                self.agents.push(agent);
                self.agents.last()
            }
        }
    }

    /// Forgets the agent `id`; whether it was recorded.
    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.agents.len();
        self.agents.retain(|a| a.id != id);
        self.agents.len() != before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example(name: &str) -> Listing {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../schemas/marketplace/examples/valid")
            .join(name);
        Listing::parse(&std::fs::read(path).unwrap()).unwrap()
    }

    #[test]
    fn an_agent_is_recorded_saved_read_back_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("marketplace-agents-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(AgentList::load(&dir).unwrap().agents().is_empty());
        let listing = example("echo-agent.json");
        let mut list = AgentList::new();
        let added = list.add(&listing, &listing.versions[0]).unwrap();
        assert_eq!(added.argv(), ["echo-agent", "--acp"]);
        assert_eq!(added.homepage, "https://example.com/echo-agent");
        assert_eq!(added.version, "1.4.0");
        // Added again: replaced, not doubled.
        list.add(&listing, &listing.versions[0]).unwrap();
        assert_eq!(list.agents().len(), 1);
        // Only an agent's listing is recorded.
        let theme = example("solarized.json");
        assert!(list.add(&theme, &theme.versions[0]).is_none());
        list.save(&dir).unwrap();
        let read = AgentList::load(&dir).unwrap();
        assert_eq!(read, list);
        assert_eq!(read.get("echo-agent").unwrap().tier, Tier::Community);
        let mut read = read;
        assert!(read.remove("echo-agent"));
        assert!(!read.remove("echo-agent"));
        read.save(&dir).unwrap();
        assert!(AgentList::load(&dir).unwrap().agents().is_empty());
        // Another format is refused, not read as empty.
        std::fs::write(AgentList::path(&dir), "{\"schema_version\": 2}").unwrap();
        assert!(AgentList::load(&dir).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}

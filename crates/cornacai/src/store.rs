//! The session store: the sessions Cornacai has opened, so one can be
//! reopened later with `session/load`, in Cornacai's state directory
//! (`cornacai/sessions.jsonl` in `$XDG_STATE_HOME`, else
//! `~/.local/state`).
//!
//! Only what reopening needs is kept, one JSON object a line: the
//! session's id, the agent (its launch command), the workspace, its title
//! and when it was last used. The conversation itself is the agent's to
//! keep: `session/load` replays it. The file is rewritten whole on each
//! change (written aside, then renamed), newest first, and holds at most
//! [`MAX_SESSIONS`]; a line that does not read is skipped.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The most sessions the store keeps; the least recently used go first.
pub const MAX_SESSIONS: usize = 200;

/// A session as the store keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredSession {
    /// The agent's id for it.
    pub id: String,
    /// The agent it belongs to: its launch command, as one line.
    pub agent: String,
    /// Its workspace.
    pub cwd: PathBuf,
    /// Its title when last seen.
    pub title: String,
    /// When it was last used, in seconds since the Unix epoch.
    pub last_used: u64,
}

impl StoredSession {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "agent": self.agent,
            "cwd": self.cwd.to_string_lossy(),
            "title": self.title,
            "lastUsed": self.last_used,
        })
    }

    fn from_json(value: &Value) -> Option<StoredSession> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(StoredSession {
            id: text("id").filter(|id| !id.is_empty())?,
            agent: text("agent")?,
            cwd: PathBuf::from(text("cwd")?),
            title: text("title").unwrap_or_default(),
            last_used: value.get("lastUsed").and_then(Value::as_u64).unwrap_or(0),
        })
    }
}

/// The store, a file of sessions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionStore {
    path: PathBuf,
}

impl SessionStore {
    /// A store in the file at `path` (made when first written).
    pub fn new(path: impl Into<PathBuf>) -> SessionStore {
        SessionStore { path: path.into() }
    }

    /// `cornacai/sessions.jsonl` in the user's state directory
    /// (`$XDG_STATE_HOME`, else `~/.local/state`).
    pub fn default_path() -> Option<PathBuf> {
        let dir = std::env::var_os("XDG_STATE_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("state"))
            })?;
        Some(dir.join("cornacai").join("sessions.jsonl"))
    }

    /// The file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every session kept, most recently used first. A missing file is an
    /// empty store.
    pub fn list(&self) -> Vec<StoredSession> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let mut sessions: Vec<StoredSession> = text
            .lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter_map(|value| StoredSession::from_json(&value))
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_used));
        sessions
    }

    /// The sessions of `agent` in `cwd`, most recently used first: the
    /// ones that can be reopened here.
    pub fn sessions_of(&self, agent: &str, cwd: &Path) -> Vec<StoredSession> {
        self.list()
            .into_iter()
            .filter(|s| s.agent == agent && s.cwd == cwd)
            .collect()
    }

    /// The session `id` of `agent`, if kept.
    pub fn get(&self, agent: &str, id: &str) -> Option<StoredSession> {
        self.list()
            .into_iter()
            .find(|s| s.agent == agent && s.id == id)
    }

    /// Keeps `session`, replacing the one with its id and agent.
    pub fn remember(&self, session: StoredSession) -> io::Result<()> {
        let mut sessions = self.list();
        sessions.retain(|s| !(s.id == session.id && s.agent == session.agent));
        sessions.insert(0, session);
        sessions.sort_by_key(|s| std::cmp::Reverse(s.last_used));
        sessions.truncate(MAX_SESSIONS);
        self.write(&sessions)
    }

    /// Sets the title of session `id` of `agent`, if kept.
    pub fn retitle(&self, agent: &str, id: &str, title: &str) -> io::Result<()> {
        let mut sessions = self.list();
        let Some(session) = sessions.iter_mut().find(|s| s.agent == agent && s.id == id) else {
            return Ok(());
        };
        if session.title == title {
            return Ok(());
        }
        session.title = title.to_owned();
        self.write(&sessions)
    }

    /// Forgets session `id` of `agent`.
    pub fn forget(&self, agent: &str, id: &str) -> io::Result<()> {
        let mut sessions = self.list();
        let before = sessions.len();
        sessions.retain(|s| !(s.id == id && s.agent == agent));
        if sessions.len() == before {
            return Ok(());
        }
        self.write(&sessions)
    }

    fn write(&self, sessions: &[StoredSession]) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            private_dir(dir)?;
        }
        let mut text = String::new();
        for session in sessions {
            text.push_str(&session.to_json().to_string());
            text.push('\n');
        }
        let aside = self
            .path
            .with_extension(format!("jsonl.{}", std::process::id()));
        std::fs::write(&aside, text)?;
        std::fs::rename(&aside, &self.path)
    }
}

/// Makes `dir`, readable by the user only on Unix: session ids name
/// conversations.
fn private_dir(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Now, in seconds since the Unix epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// How long ago `then` was, from `now`, for people: "just now",
/// "5 min ago", "3 h ago", "2 days ago".
pub fn ago(then: u64, now: u64) -> String {
    let seconds = now.saturating_sub(then);
    match seconds {
        0..60 => "just now".to_string(),
        60..3600 => format!("{} min ago", seconds / 60),
        3600..86_400 => format!("{} h ago", seconds / 3600),
        86_400..172_800 => "yesterday".to_string(),
        _ => format!("{} days ago", seconds / 86_400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, agent: &str, cwd: &str, last_used: u64) -> StoredSession {
        StoredSession {
            id: id.into(),
            agent: agent.into(),
            cwd: cwd.into(),
            title: format!("Title {id}"),
            last_used,
        }
    }

    #[test]
    fn the_store_keeps_sessions_newest_first_and_replaces_by_id() {
        let dir = std::env::temp_dir().join(format!("cornacai-store-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SessionStore::new(dir.join("state").join("sessions.jsonl"));
        assert!(store.list().is_empty());
        store.remember(session("a", "agent", "/w", 10)).unwrap();
        store.remember(session("b", "agent", "/w", 20)).unwrap();
        store.remember(session("c", "other", "/w", 30)).unwrap();
        store
            .remember(session("d", "agent", "/elsewhere", 40))
            .unwrap();
        let ids = |list: Vec<StoredSession>| list.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(store.list()), ["d", "c", "b", "a"]);
        assert_eq!(ids(store.sessions_of("agent", Path::new("/w"))), ["b", "a"]);
        // Used again: first, with one entry.
        store.remember(session("a", "agent", "/w", 50)).unwrap();
        assert_eq!(ids(store.sessions_of("agent", Path::new("/w"))), ["a", "b"]);
        store.retitle("agent", "b", "Renamed").unwrap();
        assert_eq!(store.get("agent", "b").unwrap().title, "Renamed");
        store.forget("agent", "b").unwrap();
        assert_eq!(ids(store.sessions_of("agent", Path::new("/w"))), ["a"]);
        // A line that does not read is skipped, not fatal.
        let mut text = std::fs::read_to_string(store.path()).unwrap();
        text.push_str("not json\n{\"id\": \"\"}\n");
        std::fs::write(store.path(), text).unwrap();
        assert_eq!(store.list().len(), 3);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("state"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o700);
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_store_holds_at_most_its_limit() {
        let dir = std::env::temp_dir().join(format!("cornacai-store-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = SessionStore::new(dir.join("sessions.jsonl"));
        let many: Vec<StoredSession> = (0..MAX_SESSIONS as u64 + 5)
            .map(|i| session(&i.to_string(), "agent", "/w", i))
            .collect();
        store.write(&many).unwrap();
        store
            .remember(session("new", "agent", "/w", 10_000))
            .unwrap();
        let list = store.list();
        assert_eq!(list.len(), MAX_SESSIONS);
        assert_eq!(list[0].id, "new");
        assert!(!list.iter().any(|s| s.id == "0"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ages_read_as_people_say_them() {
        assert_eq!(ago(100, 130), "just now");
        assert_eq!(ago(0, 300), "5 min ago");
        assert_eq!(ago(0, 3 * 3600), "3 h ago");
        assert_eq!(ago(0, 90_000), "yesterday");
        assert_eq!(ago(0, 3 * 86_400), "3 days ago");
        assert_eq!(ago(500, 100), "just now");
    }
}

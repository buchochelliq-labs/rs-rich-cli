//! A session's conversation as state on the app's thread: entries in the
//! order they arrived (the user's prompts, the agent's answers and
//! thoughts, tool calls, notices), the plan, the mode and the title.
//!
//! [`Session::apply`] turns one `session/update` into changes of signals:
//! a chunk appends to the open message's document (which the transcript
//! draws with intuiTUIve's streaming `markdown` node, so only its last
//! block renders again), a `tool_call` adds an entry, a `tool_call_update`
//! patches it (`ToolCall::apply`), a `plan` replaces the plan whole.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use rich_acp::{
    ContentBlock, ContentChunk, PlanEntry, SessionId, SessionMode, SessionUpdate, ToolCall,
    ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolKind,
};
use rich_intuituive::prelude::*;

use crate::clean::clean;

/// One entry of a conversation.
#[derive(Clone)]
pub enum Entry {
    /// The user's prompt, as typed, and the files it mentions.
    User {
        /// The prompt.
        text: String,
        /// The `@` mentions sent as resource links.
        mentions: Vec<String>,
    },
    /// The agent's answer (Markdown), growing as chunks arrive.
    Agent {
        /// The message id chunks carry, if any.
        message_id: Option<String>,
        /// The text so far.
        doc: Signal<String>,
    },
    /// The agent's reasoning, folded.
    Thought {
        /// The message id chunks carry, if any.
        message_id: Option<String>,
        /// The text so far.
        doc: Signal<String>,
        /// Whether it is unfolded.
        open: Signal<bool>,
    },
    /// A tool call, folded, its title and status following its updates.
    Tool {
        /// The call, with every update applied.
        call: Signal<ToolCall>,
        /// Whether it is unfolded.
        open: Signal<bool>,
    },
    /// Something Cornacai says: a turn's end, an error, a write applied.
    /// Console markup, already escaped.
    Notice(String),
}

/// One session: its id (once `session/new` answers), its title and its
/// conversation. `Copy`: every field is a signal.
#[derive(Clone, Copy)]
pub struct Session {
    /// A key that never changes, for the views.
    pub key: u64,
    /// The agent's id for it, once known.
    pub id: Signal<Option<SessionId>>,
    /// Its title: the agent's (`session_info_update`), else "Session N".
    pub title: Signal<String>,
    /// The conversation.
    pub entries: Signal<Vec<Entry>>,
    /// The agent's plan, replaced whole.
    pub plan: Signal<Vec<PlanEntry>>,
    /// The current mode's id.
    pub mode: Signal<Option<String>>,
    /// The modes the agent offers for it, from `session/new` or
    /// `session/load`.
    pub modes: Signal<Vec<SessionMode>>,
    /// What the user was asked this session, so a change the agent made
    /// without asking can say so.
    pub(crate) asks: Signal<Asks>,
    /// Whether `session/load` is replaying its history: what arrives then
    /// was decided before.
    pub(crate) replaying: Signal<bool>,
    /// Whether a turn is running.
    pub busy: Signal<bool>,
    /// The prompt being written.
    pub prompt: Signal<String>,
    /// Terminal output by terminal id, as the agent last read it.
    pub terminals: Signal<HashMap<String, String>>,
}

/// What the user was asked in a session, and what they chose.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asks {
    /// The tool calls a permission prompt was shown for.
    pub asked: HashSet<String>,
    /// The files whose write was reviewed (`fs/write_text_file`) since
    /// the user's last prompt.
    pub reviewed: HashSet<PathBuf>,
    /// The "always" options chosen: the tool's kind and the option's name.
    pub always: Vec<(ToolKind, String)>,
    /// For each change made without the user being asked, by tool call id:
    /// why, in words (plain text).
    pub unasked: HashMap<String, String>,
}

impl Session {
    /// A new session, keyed `key`, titled "Session N". Call on the app's
    /// thread.
    pub fn new(key: u64) -> Session {
        Session {
            key,
            id: signal(None),
            title: signal(format!("Session {}", key + 1)),
            entries: signal(Vec::new()),
            plan: signal(Vec::new()),
            mode: signal(None),
            modes: signal(Vec::new()),
            asks: signal(Asks::default()),
            replaying: signal(false),
            busy: signal(false),
            prompt: signal(String::new()),
            terminals: signal(HashMap::new()),
        }
    }

    /// Add an entry at the end.
    pub fn push(&self, entry: Entry) {
        self.entries.update(|entries| entries.push(entry));
    }

    /// Add a notice (console markup).
    pub fn notice(&self, markup: impl Into<String>) {
        self.push(Entry::Notice(markup.into()));
    }

    /// The agent's answers, joined: what a test reads.
    pub fn agent_text(&self) -> String {
        self.entries.with_untracked(|entries| {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    Entry::Agent { doc, .. } => Some(doc.get_untracked()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n\n")
        })
    }

    /// Apply one `session/update`.
    pub fn apply(&self, update: SessionUpdate) {
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => self.chunk(chunk, false),
            SessionUpdate::AgentThoughtChunk(chunk) => self.chunk(chunk, true),
            SessionUpdate::UserMessageChunk(chunk) => {
                let text = chunk_text(&chunk.content);
                let appended = self
                    .entries
                    .with_untracked(|entries| matches!(entries.last(), Some(Entry::User { .. })));
                if appended {
                    self.entries.update(|entries| {
                        if let Some(Entry::User { text: t, .. }) = entries.last_mut() {
                            t.push_str(&text);
                        }
                    });
                } else {
                    self.push(Entry::User {
                        text,
                        mentions: Vec::new(),
                    });
                }
            }
            SessionUpdate::ToolCall(call) => {
                let id = call.tool_call_id.0.clone();
                if let Some(slot) = self.tool(&id) {
                    slot.set(call);
                } else {
                    self.push(Entry::Tool {
                        call: signal(call),
                        open: signal(false),
                    });
                }
                self.note_unasked(&id);
            }
            SessionUpdate::ToolCallUpdate(update) => {
                self.update_tool(&update);
                self.note_unasked(&update.tool_call_id.0);
            }
            SessionUpdate::Plan(plan) => self.plan.set(plan.entries),
            SessionUpdate::CurrentModeUpdate(mode) => {
                self.mode.set(Some(mode.current_mode_id.0));
            }
            SessionUpdate::Unknown { kind, value } if kind == "session_info_update" => {
                if let Some(title) = value.get("title").and_then(|t| t.as_str()) {
                    self.title.set(clean(title).replace('\n', " "));
                }
            }
            _ => {}
        }
    }

    /// Patch the tool call `update` names, or add it if it is new.
    pub fn update_tool(&self, update: &ToolCallUpdate) {
        match self.tool(&update.tool_call_id.0) {
            Some(call) => call.update(|call| call.apply(update)),
            None => {
                let mut call = ToolCall {
                    tool_call_id: update.tool_call_id.clone(),
                    ..ToolCall::default()
                };
                call.apply(update);
                self.push(Entry::Tool {
                    call: signal(call),
                    open: signal(false),
                });
            }
        }
    }

    /// The modes `session/new` or `session/load` gave: the current one,
    /// and the list, kept unless the answer has none.
    pub fn set_modes(&self, modes: rich_acp::SessionModeState) {
        self.mode.set(Some(modes.current_mode_id.0));
        if !modes.available_modes.is_empty() {
            self.modes.set(modes.available_modes);
        }
    }

    /// The current mode, as the agent names it (its id when it gave no
    /// name), subscribing to it.
    pub fn mode_name(&self) -> Option<String> {
        let id = self.mode.get()?;
        Some(self.modes.with(|modes| mode_name(modes, &id)))
    }

    /// When tool call `id` has just finished changing files (it carries a
    /// diff) and the user was not asked about it, note why: the session's
    /// mode, or an "always" chosen earlier. Not for a replayed history.
    fn note_unasked(&self, id: &str) {
        let Some(call) = self.tool(id) else {
            return;
        };
        let (done, paths, kind) = call.with_untracked(|c| {
            let paths: Vec<PathBuf> = c
                .content
                .iter()
                .filter_map(|content| match content {
                    ToolCallContent::Diff(diff) => Some(diff.path.clone()),
                    _ => None,
                })
                .collect();
            (c.status == ToolCallStatus::Completed, paths, c.kind.clone())
        });
        if !done || paths.is_empty() || self.replaying.get_untracked() {
            return;
        }
        let known = self.asks.with_untracked(|a| {
            a.asked.contains(id)
                || a.unasked.contains_key(id)
                || paths.iter().any(|p| a.reviewed.contains(p))
        });
        if known {
            return;
        }
        let always = self.asks.with_untracked(|a| {
            a.always
                .iter()
                .rev()
                .find(|(k, _)| *k == kind)
                .map(|(_, name)| name.clone())
        });
        let why = match (
            always,
            self.mode
                .get_untracked()
                .map(|id| self.modes.with_untracked(|m| mode_name(m, &id))),
        ) {
            (Some(name), _) => format!(
                "applied without asking: you chose \u{201c}{}\u{201d} earlier",
                clean(&name).replace('\n', " ")
            ),
            (None, Some(mode)) => format!("applied without asking, in {mode} mode"),
            (None, None) => "applied without asking".to_string(),
        };
        self.asks
            .update(|a| drop(a.unasked.insert(id.to_string(), why)));
    }

    /// The tool call with id `id`.
    pub fn tool(&self, id: &str) -> Option<Signal<ToolCall>> {
        self.entries.with_untracked(|entries| {
            entries.iter().rev().find_map(|entry| match entry {
                Entry::Tool { call, .. } if call.with_untracked(|c| c.tool_call_id.0 == id) => {
                    Some(*call)
                }
                _ => None,
            })
        })
    }

    fn chunk(&self, chunk: ContentChunk, thought: bool) {
        let text = chunk_text(&chunk.content);
        let id = chunk.message_id;
        let open = self.entries.with_untracked(|entries| match entries.last() {
            Some(Entry::Agent { message_id, doc }) if !thought && *message_id == id => Some(*doc),
            Some(Entry::Thought {
                message_id, doc, ..
            }) if thought && *message_id == id => Some(*doc),
            _ => None,
        });
        match open {
            Some(doc) => doc.update(|doc| doc.push_str(&text)),
            None if thought => self.push(Entry::Thought {
                message_id: id,
                doc: signal(text),
                open: signal(false),
            }),
            None => self.push(Entry::Agent {
                message_id: id,
                doc: signal(text),
            }),
        }
    }
}

/// Mode `id`'s name among `modes`, cleaned: its id when it has none.
pub fn mode_name(modes: &[SessionMode], id: &str) -> String {
    let name = modes
        .iter()
        .find(|m| m.id.0 == id)
        .map(|m| m.name.as_str())
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(id);
    clean(name).replace('\n', " ")
}

/// A content block as text for the conversation: text cleaned of control
/// characters; other kinds named.
pub fn chunk_text(content: &ContentBlock) -> String {
    match content {
        ContentBlock::Text(text) => clean(&text.text),
        ContentBlock::Image(image) => format!("*[an image, {}]*", clean(&image.mime_type)),
        ContentBlock::Audio(audio) => format!("*[audio, {}]*", clean(&audio.mime_type)),
        ContentBlock::ResourceLink(link) => format!("`{}`", clean(&link.uri)),
        ContentBlock::Resource(_) => "*[an embedded resource]*".to_string(),
        other => format!("*[{}]*", clean(other.kind())),
    }
}

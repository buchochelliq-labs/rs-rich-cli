//! What a permission prompt says, in words: what will happen (the change
//! and its size, the command and where it runs, the file read, the page
//! fetched, the mode switched), why (the agent's own last words before
//! the tool call), and what each option means. Generic over ACP: built
//! from the tool call's kind, locations, content and raw input alone.

use std::path::Path;

use rich_acp::{PermissionOptionKind, ToolCall, ToolCallContent, ToolKind};

use crate::bridge::display;
use crate::clean::{clean, markup};
use crate::session::{Entry, Session};

/// The most characters of the agent's words a prompt quotes.
const REASON_CHARS: usize = 360;
/// The most characters of a command in the prompt's first line.
const COMMAND_CHARS: usize = 80;

/// `n` and a noun, singular for one.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// `text` cut to `width` characters on its first line, with `…` when cut.
fn cut(text: &str, width: usize) -> String {
    let first = text.lines().next().unwrap_or("");
    let mut out: String = first.chars().take(width).collect();
    if first.chars().count() > width || text.lines().nth(1).is_some() {
        out.push('…');
    }
    out
}

/// A string field of the tool's raw input.
fn input<'a>(call: &'a ToolCall, key: &str) -> Option<&'a str> {
    call.raw_input
        .as_ref()?
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty())
}

/// The diffs of a call: each file, whether it is new, and the lines added
/// and removed.
pub(crate) fn diffs(call: &ToolCall, cwd: &Path) -> Vec<(String, bool, usize, usize)> {
    call.content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Diff(diff) => {
                let old = diff.old_text.as_deref().unwrap_or("");
                let (added, removed) = rich_ext::diff::TextDiff::new(old, &diff.new_text).stats();
                Some((
                    display(&diff.path, cwd),
                    diff.old_text.is_none(),
                    added,
                    removed,
                ))
            }
            _ => None,
        })
        .collect()
}

/// The files a call names, shown relative to the workspace.
fn files(call: &ToolCall, cwd: &Path) -> String {
    let names: Vec<String> = call
        .locations
        .iter()
        .map(|l| display(&l.path, cwd))
        .filter(|p| !p.is_empty())
        .collect();
    match names.len() {
        0 => String::new(),
        1..=3 => names.join(", "),
        n => format!("{} and {} more", names[..2].join(", "), n - 2),
    }
}

/// What will happen, in words, as console markup (escaped): "Edit
/// src/app.rs: 12 lines added, 3 removed", "Run `cargo test` in the
/// workspace", "Read notes.md", "Fetch URL", "Switch out of Plan mode and
/// start on this plan". `mode` is the session's current mode's name.
pub(crate) fn intent(call: &ToolCall, cwd: &Path, mode: Option<&str>) -> String {
    let changes = diffs(call, cwd);
    if !changes.is_empty() {
        let parts: Vec<String> = changes
            .iter()
            .map(|(path, fresh, added, removed)| {
                let path = markup(&clean(path));
                if *fresh {
                    format!("Create {path}: {}", count(*added, "line", "lines"))
                } else {
                    format!(
                        "Edit {path}: {} added, {} removed",
                        count(*added, "line", "lines"),
                        removed
                    )
                }
            })
            .collect();
        return parts.join("; ");
    }
    let title = markup(&clean(&call.title).replace('\n', " "));
    let files = markup(&clean(&files(call, cwd)));
    let or_title = |verb: &str, what: &str| match (what.is_empty(), title.is_empty()) {
        (false, _) => format!("{verb} {what}"),
        (true, false) => format!("{verb}: “{title}”"),
        (true, true) => verb.to_string(),
    };
    match &call.kind {
        ToolKind::Execute => {
            let command = input(call, "command").unwrap_or(call.title.as_str());
            // The agent's commands run in the workspace unless the input
            // names another directory.
            let dir = input(call, "cwd")
                .map(|d| display(Path::new(d), cwd))
                .filter(|d| !d.is_empty())
                .map_or_else(|| "the workspace".to_string(), |d| markup(&clean(&d)));
            format!(
                "Run `{}` in {dir}",
                markup(&clean(&cut(command, COMMAND_CHARS)))
            )
        }
        ToolKind::Read => or_title("Read", &files),
        ToolKind::Edit => or_title("Edit", &files),
        ToolKind::Delete => or_title("Delete", &files),
        ToolKind::Move => or_title("Move", &files),
        ToolKind::Search => or_title("Search", &files),
        ToolKind::Fetch => {
            let url = input(call, "url").map(|u| markup(&clean(&cut(u, 120))));
            or_title("Fetch", url.as_deref().unwrap_or(""))
        }
        ToolKind::SwitchMode => {
            let from = mode
                .map(|m| format!("out of {} mode", markup(m)))
                .unwrap_or_else(|| "mode".to_string());
            if plan_text(call).is_some() {
                format!("Switch {from} and start on this plan")
            } else {
                format!("Switch {from}")
            }
        }
        ToolKind::Think => "Think it over".to_string(),
        _ => or_title("Use a tool", ""),
    }
}

/// The text a call carries as content (a plan, for a mode switch), or
/// its raw input's `plan`.
pub(crate) fn plan_text(call: &ToolCall) -> Option<String> {
    let text: Vec<&str> = call
        .content
        .iter()
        .filter_map(|content| match content {
            ToolCallContent::Content(wrapped) => wrapped.content.as_text(),
            _ => None,
        })
        .filter(|t| !t.trim().is_empty())
        .collect();
    if !text.is_empty() {
        return Some(clean(&text.join("\n\n")));
    }
    input(call, "plan").map(clean)
}

/// Why, in the agent's words: its last message in this turn before tool
/// call `id` (its last paragraph or two), else a `description` in the
/// tool's input, which several agents send. Plain text, cleaned.
pub(crate) fn reason(session: Option<Session>, call: &ToolCall) -> Option<String> {
    let said = session.and_then(|s| {
        s.entries.with_untracked(|entries| {
            let end = entries
                .iter()
                .rposition(|e| matches!(e, Entry::Tool { call: c, .. } if c.with_untracked(|c| c.tool_call_id == call.tool_call_id)))
                .unwrap_or(entries.len());
            entries[..end]
                .iter()
                .rev()
                .take_while(|e| !matches!(e, Entry::User { .. }))
                .find_map(|e| match e {
                    Entry::Agent { doc, .. } => {
                        Some(doc.get_untracked()).filter(|d| !d.trim().is_empty())
                    }
                    _ => None,
                })
        })
    });
    let text = match said {
        Some(text) => last_paragraphs(&text, 2),
        None => input(call, "description")?.trim().to_string(),
    };
    let text = clean(&text);
    let chars = text.chars().count();
    if chars > REASON_CHARS {
        let kept: String = text.chars().skip(chars - REASON_CHARS).collect();
        return Some(format!("…{}", kept.trim_start()));
    }
    Some(text)
}

/// The last `n` paragraphs of `text` (runs of lines between blank ones).
fn last_paragraphs(text: &str, n: usize) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut open = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if !open.is_empty() {
                paragraphs.push(std::mem::take(&mut open));
            }
        } else {
            if !open.is_empty() {
                open.push('\n');
            }
            open.push_str(line.trim_end());
        }
    }
    if !open.is_empty() {
        paragraphs.push(open);
    }
    let from = paragraphs.len().saturating_sub(n);
    paragraphs[from..].join("\n\n")
}

/// What choosing an option of `kind` does, in words; `change` when the
/// request carries a diff.
pub(crate) fn meaning(kind: &PermissionOptionKind, change: bool) -> &'static str {
    match (kind, change) {
        (PermissionOptionKind::AllowOnce, true) => {
            "accept this change; the agent asks again next time"
        }
        (PermissionOptionKind::AllowOnce, false) => {
            "allow it this time; the agent asks again next time"
        }
        (PermissionOptionKind::AllowAlways, true) => {
            "accept it, and let the agent make such changes without asking (it keeps \
             the choice, usually for this session)"
        }
        (PermissionOptionKind::AllowAlways, false) => {
            "allow it, and let the agent do this without asking (it keeps the choice, \
             usually for this session)"
        }
        (PermissionOptionKind::RejectOnce, true) => {
            "reject this change; the agent is told and may try another way"
        }
        (PermissionOptionKind::RejectOnce, false) => {
            "refuse this time; the agent is told and may try another way"
        }
        (PermissionOptionKind::RejectAlways, _) => {
            "refuse, and have the agent stop asking (it keeps the refusal)"
        }
        _ => "an option of the agent's own",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_paragraphs_are_kept() {
        let text = "One.\n\nTwo,\nstill two.\n\n\nThree.\n";
        assert_eq!(last_paragraphs(text, 2), "Two,\nstill two.\n\nThree.");
        assert_eq!(
            last_paragraphs(text, 5),
            "One.\n\nTwo,\nstill two.\n\nThree."
        );
    }

    #[test]
    fn a_command_is_cut_to_its_first_line() {
        assert_eq!(cut("cargo test\n--all", 80), "cargo test…");
        assert_eq!(cut("abcdef", 3), "abc…");
        assert_eq!(cut("ls", 3), "ls");
    }
}

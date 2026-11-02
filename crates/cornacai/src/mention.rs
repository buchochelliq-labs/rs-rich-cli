//! `@` file mentions: the prompt editor completes them (intuiTUIve's
//! `files::path_completer`), and a prompt is sent as its text followed by
//! a `resource_link` for each mentioned file, which every ACP agent
//! accepts (the protocol's baseline prompt content).

use std::path::{Path, PathBuf};

use rich_acp::{ContentBlock, PromptRequest, ResourceLink, SessionId};

/// A file named in a prompt as `@path`: the path as typed and the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    /// As typed after the `@`, relative to the workspace.
    pub text: String,
    /// The file, absolute.
    pub path: PathBuf,
}

/// The `@path` mentions in `prompt` that name a file or directory under
/// `cwd`, in order, each once. An `@` counts at the start or after
/// whitespace; the path runs to the next whitespace, less trailing
/// punctuation that is not part of an existing path (`see @a.rs.`). Paths
/// that leave `cwd` (`@../x`, `@/etc/passwd`) are not mentions.
pub fn mentions(prompt: &str, cwd: &Path) -> Vec<Mention> {
    let mut found: Vec<Mention> = Vec::new();
    let mut previous = ' ';
    for (at, c) in prompt.char_indices() {
        let starts = c == '@' && previous.is_whitespace();
        previous = c;
        if !starts {
            continue;
        }
        let rest = &prompt[at + 1..];
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let mut text = &rest[..end];
        loop {
            if let Some(path) = resolve(text, cwd) {
                if !found.iter().any(|m| m.path == path) {
                    found.push(Mention {
                        text: text.trim_end_matches('/').to_string(),
                        path,
                    });
                }
                break;
            }
            match text.char_indices().next_back() {
                Some((i, c)) if ".,;:!?)]}'\"".contains(c) => text = &text[..i],
                _ => break,
            }
        }
    }
    found
}

/// `text` under `cwd`, when it exists there and stays there.
fn resolve(text: &str, cwd: &Path) -> Option<PathBuf> {
    if text.is_empty() {
        return None;
    }
    let relative = Path::new(text);
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    let path = cwd.join(relative);
    path.exists().then_some(path)
}

/// A `file://` URI for an absolute path, percent-encoding what a URI
/// cannot hold.
pub fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    let text = path.to_string_lossy().replace('\\', "/");
    if !text.starts_with('/') {
        uri.push('/');
    }
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                uri.push(byte as char)
            }
            _ => uri.push_str(&format!("%{byte:02X}")),
        }
    }
    uri
}

/// The prompt to send: `text` as a text block, then a `resource_link` for
/// each of its [`mentions`].
pub fn prompt_request(session: SessionId, text: &str, cwd: &Path) -> PromptRequest {
    let mut prompt = vec![ContentBlock::text(text)];
    for mention in mentions(text, cwd) {
        let name = mention
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| mention.text.clone());
        prompt.push(ContentBlock::ResourceLink(ResourceLink {
            uri: file_uri(&mention.path),
            name,
            mime_type: None,
            title: Some(mention.text.clone()),
            description: None,
            size: std::fs::metadata(&mention.path)
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len()),
        }));
    }
    PromptRequest {
        session_id: session,
        prompt,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_name_existing_files_once() {
        let dir = std::env::temp_dir().join(format!("cornacai-mention-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "fn main() {}").unwrap();
        let found = mentions(
            "look at @src/main.rs. and @src/ and @missing, me@src/main.rs, @../x @src/main.rs",
            &dir,
        );
        let texts: Vec<&str> = found.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["src/main.rs", "src"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn uris_are_encoded() {
        assert_eq!(
            file_uri(Path::new("/tmp/a b/ü.rs")),
            "file:///tmp/a%20b/%C3%BC.rs"
        );
    }
}

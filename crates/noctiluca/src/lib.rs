//! Noctiluca, a terminal web browser on intuiTUIve.
//!
//! This is the first spike of [the design](../../docs/design/browser.md):
//! a page is opened in a headless Chrome or Chromium through
//! rs-rich-embed's [`ChromeEngine`] (DevTools over a pipe, the sandbox
//! on, a temporary profile), read with one `DOMSnapshot.captureSnapshot`
//! call, which the page cannot observe, and its headings and paragraphs
//! come back as rich [`Text`].
//!
//! No script of ours runs in the page. Everything the page says is
//! untrusted: control characters are stripped ([`clean`]) before any of it
//! reaches the terminal.

use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use rich::Text;
use rich_embed::{ChromeEngine, DevTools, WebEngine};
use serde_json::{json, Value};

/// How long one DevTools call may take.
const CALL: Duration = Duration::from_secs(30);

/// What a block of the page is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockKind {
    /// `<h1>` to `<h6>`: the level.
    Heading(u8),
    Paragraph,
}

/// A heading or a paragraph, with its rendered text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub text: String,
}

/// A browser to read pages with: one Chrome, reused page after page.
pub struct Reader {
    engine: ChromeEngine,
    devtools: DevTools,
}

impl Reader {
    /// Start the browser at `binary`, or the one [`ChromeEngine::find`]
    /// finds. The page is laid out at `columns` x `rows` cells of 10 x 20
    /// CSS pixels.
    pub fn new(binary: Option<PathBuf>, columns: u16, rows: u16) -> io::Result<Reader> {
        let mut engine = ChromeEngine::new();
        if let Some(binary) = binary {
            engine = engine.binary(binary);
        }
        engine.resize(columns, rows)?;
        let devtools = engine.devtools()?;
        Ok(Reader { engine, devtools })
    }

    /// The page's DevTools, for calls of one's own.
    pub fn devtools(&self) -> &DevTools {
        &self.devtools
    }

    /// Open `url` and wait, up to `timeout`, until it has loaded.
    pub fn load(&mut self, url: &str, timeout: Duration) -> io::Result<()> {
        self.engine.open(url)?;
        let start = Instant::now();
        loop {
            let state = self.engine.state();
            if let Some(error) = state.error {
                return Err(io::Error::other(error));
            }
            // While a navigation swaps the page's renderer process, a
            // snapshot can be refused: that is not loaded yet, not a failure.
            if !state.loading
                && self
                    .snapshot()
                    .is_ok_and(|snapshot| document_url(&snapshot) == url)
            {
                return Ok(());
            }
            if start.elapsed() > timeout {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    format!("{url} did not load in time"),
                ));
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    /// The page as `DOMSnapshot.captureSnapshot` sees it: its nodes, their
    /// layout boxes and rectangles, and [`SNAPSHOT_STYLES`].
    pub fn snapshot(&self) -> io::Result<Value> {
        self.devtools.call(
            "DOMSnapshot.captureSnapshot",
            json!({
                "computedStyles": SNAPSHOT_STYLES,
                "includeDOMRects": true,
                "includePaintOrder": true,
            }),
            CALL,
        )
    }

    /// The page's headings and paragraphs, now.
    pub fn blocks(&self) -> io::Result<Vec<Block>> {
        Ok(blocks(&self.snapshot()?))
    }
}

/// The computed styles a snapshot asks for, in this order.
pub const SNAPSHOT_STYLES: &[&str] = &[
    "display",
    "visibility",
    "color",
    "background-color",
    "font-weight",
    "font-style",
    "text-decoration-line",
    "white-space",
];

/// The `file://` address of `path`.
pub fn file_url(path: &Path) -> io::Result<String> {
    let path = path.canonicalize()?;
    let mut url = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    Ok(url)
}

/// The main document's address in a snapshot.
pub fn document_url(snapshot: &Value) -> &str {
    let strings = &snapshot["strings"];
    snapshot["documents"][0]["documentURL"]
        .as_i64()
        .and_then(|i| strings.get(usize::try_from(i).ok()?))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

/// The main document's headings and paragraphs, in document order, with
/// the text Chrome lays out: a text node with no layout box (in
/// `display: none`, or never rendered) is left out, and so is an element
/// with no text that shows.
pub fn blocks(snapshot: &Value) -> Vec<Block> {
    let strings: Vec<&str> = snapshot["strings"]
        .as_array()
        .map(|all| all.iter().map(|s| s.as_str().unwrap_or_default()).collect())
        .unwrap_or_default();
    let string = |value: &Value| -> &str {
        value
            .as_i64()
            .and_then(|i| strings.get(usize::try_from(i).ok()?).copied())
            .unwrap_or_default()
    };
    let document = &snapshot["documents"][0];
    let nodes = &document["nodes"];
    let ints = |value: &Value| -> Vec<i64> {
        value
            .as_array()
            .map(|all| all.iter().map(|n| n.as_i64().unwrap_or(-1)).collect())
            .unwrap_or_default()
    };
    let parents = ints(&nodes["parentIndex"]);
    let types = ints(&nodes["nodeType"]);
    let names: Vec<&str> = nodes["nodeName"]
        .as_array()
        .map(|all| all.iter().map(string).collect())
        .unwrap_or_default();
    let values: Vec<&str> = nodes["nodeValue"]
        .as_array()
        .map(|all| all.iter().map(string).collect())
        .unwrap_or_default();
    let mut laid_out = vec![false; parents.len()];
    for index in ints(&document["layout"]["nodeIndex"]) {
        if let Some(seen) = usize::try_from(index)
            .ok()
            .and_then(|i| laid_out.get_mut(i))
        {
            *seen = true;
        }
    }
    // The block each node belongs to: its nearest heading or paragraph.
    let mut owner: Vec<Option<usize>> = vec![None; parents.len()];
    let mut found: Vec<(usize, BlockKind, String)> = Vec::new();
    for index in 0..parents.len() {
        let parent = usize::try_from(parents[index]).ok();
        let inherited = parent.and_then(|p| owner.get(p).copied().flatten());
        let kind = (types.get(index) == Some(&1))
            .then(|| block_kind(names.get(index).copied().unwrap_or_default()))
            .flatten();
        owner[index] = match kind {
            Some(kind) => {
                found.push((index, kind, String::new()));
                Some(found.len() - 1)
            }
            None => inherited,
        };
        // A line break inside a block reads as a space.
        if kind.is_none()
            && names
                .get(index)
                .is_some_and(|n| n.eq_ignore_ascii_case("BR"))
        {
            if let Some(block) = inherited {
                found[block].2.push(' ');
            }
        }
        if types.get(index) == Some(&3) && laid_out[index] {
            if let Some(block) = inherited {
                found[block]
                    .2
                    .push_str(values.get(index).copied().unwrap_or_default());
            }
        }
    }
    found
        .into_iter()
        .filter_map(|(_, kind, text)| {
            let text = clean(&text);
            (!text.is_empty()).then_some(Block { kind, text })
        })
        .collect()
}

fn block_kind(name: &str) -> Option<BlockKind> {
    match name.to_ascii_uppercase().as_str() {
        "P" => Some(BlockKind::Paragraph),
        "H1" => Some(BlockKind::Heading(1)),
        "H2" => Some(BlockKind::Heading(2)),
        "H3" => Some(BlockKind::Heading(3)),
        "H4" => Some(BlockKind::Heading(4)),
        "H5" => Some(BlockKind::Heading(5)),
        "H6" => Some(BlockKind::Heading(6)),
        _ => None,
    }
}

/// `text` safe for a terminal, its whitespace collapsed: no C0 or C1
/// control character (so no escape sequence), no DEL, and no bidirectional
/// override or isolate, which could make text read in a false order.
pub fn clean(text: &str) -> String {
    let kept: String = text
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|&c| {
            !(c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
        })
        .collect();
    kept.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The blocks as rich text: headings bold (the first level underlined
/// too), a blank line between blocks.
pub fn render(blocks: &[Block]) -> Text {
    let mut text = Text::new("");
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            text.append("\n\n", None);
        }
        let style = match block.kind {
            BlockKind::Heading(1) => Some("bold underline".into()),
            BlockKind::Heading(_) => Some("bold".into()),
            BlockKind::Paragraph => None,
        };
        text.append(&block.text, style);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A snapshot as Chrome shapes one: `<h1>Hi</h1><p>One <b>two</b></p>`
    /// with a hidden `<p>`.
    fn sample() -> Value {
        // Strings: 0 "", 1 "#document", 2 "HTML", 3 "BODY", 4 "H1", 5 "Hi",
        // 6 "P", 7 "One ", 8 "B", 9 "two", 10 "#text", 11 "hidden",
        // 12 "about:x".
        json!({
            "strings": ["", "#document", "HTML", "BODY", "H1", "Hi", "P", "One ",
                        "B", "two", "#text", "hidden", "about:x"],
            "documents": [{
                "documentURL": 12,
                "nodes": {
                    "parentIndex": [-1, 0, 1, 2, 3, 2, 5, 5, 7, 2, 9],
                    "nodeType":    [ 9, 1, 1, 1, 3, 1, 3, 1, 3, 1, 3],
                    "nodeName":    [ 1, 2, 3, 4, 10, 6, 10, 8, 10, 6, 10],
                    "nodeValue":   [-1, -1, -1, -1, 5, -1, 7, -1, 9, -1, 11],
                },
                "layout": {"nodeIndex": [1, 2, 3, 4, 5, 6, 7, 8]},
            }],
        })
    }

    #[test]
    fn headings_and_paragraphs_come_from_laid_out_text() {
        let snapshot = sample();
        assert_eq!(document_url(&snapshot), "about:x");
        assert_eq!(
            blocks(&snapshot),
            [
                Block {
                    kind: BlockKind::Heading(1),
                    text: "Hi".into()
                },
                Block {
                    kind: BlockKind::Paragraph,
                    text: "One two".into()
                },
            ]
        );
    }

    #[test]
    fn page_text_cannot_reach_the_terminal_as_control() {
        assert_eq!(clean("a\x1b[31mb\u{9b}2Jc\u{202e}d\u{7f}"), "a[31mb2Jcd");
        assert_eq!(clean("  two\n\t lines  "), "two lines");
    }

    #[test]
    fn blocks_render_as_rich_text() {
        let text = render(&[
            Block {
                kind: BlockKind::Heading(1),
                text: "Title".into(),
            },
            Block {
                kind: BlockKind::Paragraph,
                text: "Body".into(),
            },
        ]);
        assert_eq!(text.plain(), "Title\n\nBody");
    }

    #[test]
    fn file_addresses_are_escaped() {
        let dir = std::env::temp_dir().join("noctiluca a#b");
        std::fs::create_dir_all(&dir).unwrap();
        let url = file_url(&dir).unwrap();
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.ends_with("/noctiluca%20a%23b"), "{url}");
        std::fs::remove_dir(&dir).ok();
    }
}

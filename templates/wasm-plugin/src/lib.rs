//! A WASM plugin for `rich`, from the rs-rich template.
//!
//!     cargo test     # the unit tests, natively
//!     cargo wasm     # the plugin: target/wasm32-unknown-unknown/release/*.wasm
//!     rich --plugin target/wasm32-unknown-unknown/release/<name>.wasm \
//!         --transform title-case -p "hello there"
//!
//! The host runs the module sandboxed: it imports nothing (no files, network,
//! clock or randomness), every call runs in a fresh instance with a fuel and
//! memory budget, and text is all that crosses the boundary. What the plugin
//! adds is its manifest, [`MANIFEST`], and [`call`] does the work; the four
//! exports the host calls are in `abi.rs` and need no changes.
//!
//! To add a capability, add its line to [`MANIFEST`] and its arm to [`call`],
//! in the same position. The kinds, what each is given and what it returns:
//!
//! | kind | input | output |
//! |---|---|---|
//! | `transform` | plain text | plain text |
//! | `highlighter` | plain text | one `START END STYLE` line per span (byte offsets) |
//! | `fence-markup` | a Markdown fence's code | `rich` console markup |
//! | `fence-ansi` | a Markdown fence's code | text with ANSI SGR styling (its listing declares `ansi-output`) |
//!
//! Every call also gets the width available, in cells.

mod abi;

/// What the plugin is, as the host reads it: the ABI it is written against,
/// its id and version (the package's, so the marketplace listing that
/// `rich plugin publish` writes matches the module), and one line per
/// capability, numbered from 0 in this order.
///
/// `rich-plugin-abi 1.0` is `rich_plugin_api::abi::ABI_MAJOR.ABI_MINOR`: a
/// host loads a plugin whose major equals its own, and refuses a capability
/// kind it does not know.
pub const MANIFEST: &str = concat!(
    "rich-plugin-abi 1.0\n",
    "name ",
    env!("CARGO_PKG_NAME"),
    "\n",
    "version ",
    env!("CARGO_PKG_VERSION"),
    "\n",
    "description ",
    env!("CARGO_PKG_DESCRIPTION"),
    "\n",
    "capability transform title-case\n",
    "capability highlighter todo\n",
    "capability fence-markup callout\n",
);

/// Run capability number `capability` (its line in [`MANIFEST`]) on
/// `input`, with `width` cells available. An `Err` is shown to the user as
/// the plugin's error.
pub fn call(capability: u32, input: &str, width: u32) -> Result<String, String> {
    match capability {
        0 => Ok(title_case(input)),
        1 => Ok(todo_spans(input)),
        2 => Ok(callout(input, width)),
        _ => Err(format!("unknown capability {capability}")),
    }
}

/// `transform title-case`: the first letter of every word upper case, the
/// rest lower case.
pub fn title_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_word = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if in_word {
                out.extend(c.to_lowercase());
            } else {
                out.extend(c.to_uppercase());
            }
            in_word = true;
        } else {
            out.push(c);
            // An apostrophe inside a word keeps it one word: "it's".
            in_word = in_word && c == '\'';
        }
    }
    out
}

/// The words [`todo_spans`] marks, and the style each gets: any style
/// `rich` parses (`Style::parse`). A highlighter only colours: links are
/// dropped.
const MARKERS: [(&str, &str); 3] = [
    ("TODO", "bold black on yellow"),
    ("FIXME", "bold white on red"),
    ("XXX", "bold white on magenta"),
];

/// `highlighter todo`: a `START END STYLE` line for every marker that is a
/// whole word. Offsets are bytes into the text, on character boundaries.
pub fn todo_spans(text: &str) -> String {
    let is_word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
    let mut out = String::new();
    for (marker, style) in MARKERS {
        for (start, _) in text.match_indices(marker) {
            let end = start + marker.len();
            if !is_word(text[..start].chars().next_back()) && !is_word(text[end..].chars().next()) {
                out.push_str(&format!("{start} {end} {style}\n"));
            }
        }
    }
    out
}

/// `fence-markup callout`: a fenced block drawn as a callout, a bar down
/// its left and a rule under it as wide as the space available.
///
/// ````markdown
/// ```callout
/// Back up before you upgrade.
/// ```
/// ````
pub fn callout(code: &str, width: u32) -> String {
    let mut out = String::from("[bold cyan]┃ Note[/]\n");
    for line in code.lines() {
        out.push_str("[cyan]┃[/] ");
        out.push_str(&escape(line));
        out.push('\n');
    }
    let rule = "─".repeat(width.clamp(1, 1024) as usize);
    out.push_str(&format!("[cyan]{rule}[/]"));
    out
}

/// `text` as `rich` markup that prints as it is: every `[` escaped, and the
/// backslashes before a `[` (or at the end, where a closing tag follows)
/// doubled, so they stay backslashes.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut backslashes = 0;
    for c in text.chars() {
        match c {
            '\\' => backslashes += 1,
            '[' => {
                out.push_str(&"\\".repeat(backslashes * 2));
                out.push_str("\\[");
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manifest_lists_one_capability_per_arm() {
        let lines: Vec<&str> = MANIFEST.lines().collect();
        assert_eq!(lines[0], "rich-plugin-abi 1.0");
        assert_eq!(lines[1], concat!("name ", env!("CARGO_PKG_NAME")));
        let capabilities = lines
            .iter()
            .filter(|l| l.starts_with("capability "))
            .count();
        for capability in 0..capabilities as u32 {
            assert!(call(capability, "x", 10).is_ok(), "capability {capability}");
        }
        assert!(call(capabilities as u32, "x", 10).is_err());
    }

    #[test]
    fn title_case_capitalises_each_word() {
        assert_eq!(
            title_case("hello WORLD, it's 'élan'"),
            "Hello World, It's 'Élan'"
        );
    }

    #[test]
    fn todo_spans_are_whole_words_in_bytes() {
        let text = "é TODO: fix TODOS and FIXME";
        assert_eq!(
            todo_spans(text),
            "3 7 bold black on yellow\n23 28 bold white on red\n"
        );
    }

    #[test]
    fn callout_escapes_markup_and_fills_the_width() {
        let out = callout("[red]not a tag\\", 4);
        assert_eq!(
            out,
            "[bold cyan]┃ Note[/]\n[cyan]┃[/] \\[red]not a tag\\\\\n[cyan]────[/]"
        );
    }

    #[test]
    fn escape_keeps_backslashes_before_brackets() {
        assert_eq!(escape(r"a\[b] c\d"), r"a\\\[b] c\d");
    }
}

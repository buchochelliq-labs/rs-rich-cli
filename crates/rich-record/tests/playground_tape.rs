//! A session saved from the docs playground's terminal ("Save as tape",
//! docs/assets/rich-shell/shell.js's `sessionTape`) parses, and its quoting
//! gives back exactly what was typed and the files' exact text. The fixture
//! is the terminal's own output, kept current by
//! scripts/check_playground_blocks.mjs.

use rich_record::tape::{self, Step};

const TAPE: &str = include_str!("fixtures/playground-session.tape");

#[test]
fn a_saved_playground_session_parses() {
    let tape = tape::parse(TAPE).unwrap_or_else(|e| panic!("line {}: {}", e.line, e.message));
    assert_eq!((tape.columns, tape.rows), (100, 28));

    let typed: Vec<&str> = tape
        .steps
        .iter()
        .filter_map(|(_, step)| match step {
            Step::Type(text) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        typed,
        [
            "rich data.json",
            r##"echo "# A \"quoted\" title" | rich - --markdown"##,
            "rich photo.png --width 20",
            "cat tricky.txt",
        ],
        "`help` is the terminal's own and is left out"
    );

    let write = |name: &str| {
        tape.steps.iter().find_map(|(_, step)| match step {
            Step::Write { path, content } if path == name => Some(content.clone()),
            _ => None,
        })
    };
    assert_eq!(
        write("tricky.txt").as_deref(),
        Some("say \"hi\"\\n\tnot a newline: \\ end\n")
    );
    let json = write("data.json").expect("the sample a command names is written");
    assert!(json.starts_with("{\n  \"name\": \"rs-rich\""), "{json}");
    assert!(json.ends_with("}\n"));
    assert_eq!(write("photo.png"), None, "an image is noted, not written");
}

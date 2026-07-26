//! Golden parity tests.
//!
//! Each fixture line is a `(name, markup, expected-ansi)` triple whose expected
//! column was captured from the **real Python `rich`** library (see
//! `scripts/capture_golden.py`). We assert byte-for-byte equality so that any
//! drift from upstream fails loudly. This is the backbone of the "stay in sync"
//! guarantee described in AGENTS.md.

use rich::{ColorSystem, Console};

fn truecolor_console() -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(80)
        .no_color(false)
        .build()
}

/// Turn the human-readable `\x1b` marker in the fixture into the ESC byte.
fn unescape(s: &str) -> String {
    s.replace("\\x1b", "\x1b")
}

#[test]
fn truecolor_parity() {
    let data = include_str!("golden/truecolor.tsv");
    let console = truecolor_console();
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let markup = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing markup", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let got = console.render_str_to_string(markup);
        assert_eq!(
            got,
            expected,
            "golden case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no golden cases were checked");
}

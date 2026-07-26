//! Golden parity tests.
//!
//! Each fixture line is a `(name, markup, expected-ansi)` triple whose expected
//! column was captured from the **real Python `rich`** library (see
//! `scripts/capture_golden.py`). We assert byte-for-byte equality so that any
//! drift from upstream fails loudly. This is the backbone of the "stay in sync"
//! guarantee described in AGENTS.md.

use rich::r#box::SQUARE;
use rich::{ColorSystem, Console, Padding, Panel, Renderable, Rule, Text};

fn truecolor_console(width: usize) -> Console {
    Console::builder()
        .force_terminal(true)
        .color_system(Some(ColorSystem::Truecolor))
        .width(width)
        .no_color(false)
        .build()
}

/// Turn the human-readable `\x1b` / `\n` markers in a fixture into real bytes.
fn unescape(s: &str) -> String {
    s.replace("\\x1b", "\x1b").replace("\\n", "\n")
}

/// Build the renderable matching a fixture `name`. Must stay in sync with
/// `RENDERABLE_CASES` in `scripts/capture_golden.py`.
fn build_renderable(name: &str) -> Box<dyn Renderable> {
    match name {
        "rule_plain" => Box::new(Rule::line()),
        "rule_title" | "rule_title_odd" => Box::new(Rule::new("Hi")),
        "panel_plain" => Box::new(Panel::new(Box::new(Text::new("hello")))),
        "panel_title" => Box::new(Panel::new(Box::new(Text::new("hello"))).title("T")),
        "panel_square" => Box::new(Panel::new(Box::new(Text::new("hi"))).box_set(SQUARE)),
        "padding_1_2" => Box::new(Padding::new(Box::new(Text::new("hi")), (1, 2, 1, 2))),
        "padding_0_1" => Box::new(Padding::new(Box::new(Text::new("hi")), (0, 1, 0, 1))),
        other => panic!("no builder for renderable fixture {other:?}"),
    }
}

#[test]
fn truecolor_parity() {
    let data = include_str!("golden/truecolor.tsv");
    let console = truecolor_console(80);
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

#[test]
fn renderables_parity() {
    let data = include_str!("golden/renderables.tsv");
    let mut checked = 0;
    for (index, raw) in data.lines().enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or("");
        let width: usize = parts
            .next()
            .unwrap_or_else(|| panic!("line {}: missing width", index + 1))
            .parse()
            .unwrap_or_else(|_| panic!("line {}: bad width", index + 1));
        let expected = unescape(
            parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing expected", index + 1)),
        );
        let console = truecolor_console(width);
        let got = console.render_export(build_renderable(name).as_ref());
        assert_eq!(
            got,
            expected,
            "renderable case {name:?} (line {}) diverged from upstream rich",
            index + 1
        );
        checked += 1;
    }
    assert!(checked > 0, "no renderable cases were checked");
}

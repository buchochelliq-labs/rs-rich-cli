//! Byte-parity tests for the FIGlet renderer.
//!
//! `tests/golden/figlet.tsv` is captured from real `pyfiglet` 1.0.4 using the
//! vendored `standard` font (see `scripts/` in the repo root for the capture
//! script). Each row is `name<TAB>width<TAB>justify<TAB>text<TAB>output`, with
//! the text and output backslash-escaped.

use rich_art::figlet::{self, FigletFont};
use rich_art::Justify;

/// Reverse the escaping applied by the capture script.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn justify_of(name: &str) -> Justify {
    match name {
        "center" => Justify::Center,
        "right" => Justify::Right,
        _ => Justify::Left,
    }
}

#[test]
fn matches_pyfiglet() {
    let font = FigletFont::standard();
    let data = include_str!("golden/figlet.tsv").replace("\r\n", "\n");

    let mut checked = 0;
    for line in data.lines().filter(|l| !l.trim().is_empty()) {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 5, "malformed golden row: {line:?}");
        let (name, width, justify, text, expected) = (
            fields[0],
            fields[1].parse::<usize>().expect("numeric width"),
            justify_of(fields[2]),
            unescape(fields[3]),
            unescape(fields[4]),
        );

        let actual = figlet::render(&text, &font, width, justify);
        assert_eq!(
            actual, expected,
            "case {name:?} (width {width}) diverged from pyfiglet\n\
             --- actual ---\n{actual}\n--- expected ---\n{expected}"
        );
        checked += 1;
    }
    assert!(checked >= 15, "expected the full golden set, got {checked}");
}

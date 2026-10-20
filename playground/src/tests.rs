//! Every kind renders natively, with the same functions the page calls.

use crate::render::{csv_line, KINDS};
use crate::{kinds, render, render_html};

fn plain(ansi: &str) -> String {
    // Drop CSI and OSC sequences: what a reader would read.
    let mut out = String::new();
    let mut chars = ansi.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\x07' || (c == '\x1b' && chars.next_if_eq(&'\\').is_some()) {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn sample(kind: &str) -> (&'static str, &'static str) {
    match kind {
        "markup" => ("[bold]Hello[/], [u]world[/]! :rocket:", "Hello, world! 🚀"),
        "panel" => (
            "Panels take [b]any[/b] renderable",
            "Panels take any renderable",
        ),
        "rule" => ("[b]Centred title[/b]", "Centred title"),
        "columns" => ("[b]rs-rich[/]\nrs-rich-ext\nsyntect", "rs-rich-ext"),
        "table" => (
            "Released,Title,Box Office\n\"Dec 20, 2019\",The Rise of Skywalker,\"$952,110,690\"",
            "The Rise of Skywalker",
        ),
        "tree" => ("rs-rich-cli\n  crates\n    rich\n  docs", "└── docs"),
        "json" => (
            r#"{"name": "rs-rich", "tags": ["a"]}"#,
            "\"name\": \"rs-rich\"",
        ),
        "sparkline" => ("1 5 2 8 3", ""),
        "bars" => ("api,412\nweb,268.5", "api"),
        "histogram" => ("1 2 2 3 3 3 4 4 5", ""),
        "diff" => (
            "port = 80\nhost = a\n=======\nport = 8080\nhost = a\n",
            "8080",
        ),
        "data" => ("name: api\nports: [80, 443]\n", "ports"),
        "data-diff" => ("{\"a\": 1}\n=======\n{\"a\": 2}\n", "a"),
        "syntax" => ("fn main() {}", "fn main() {}"),
        "markdown" => ("# Title\n\nSome **bold** text.", "Some bold text."),
        other => panic!("no sample for {other}"),
    }
}

#[test]
fn every_kind_renders_its_sample() {
    let compiled: Vec<serde_json::Value> = serde_json::from_str(&kinds()).unwrap();
    for kind in KINDS {
        if kind.full && !cfg!(feature = "full") {
            assert!(render(kind.name, "x", "")
                .unwrap_err()
                .contains("full module"));
            assert!(!compiled.iter().any(|k| k["name"] == kind.name));
            continue;
        }
        let (input, expect) = sample(kind.name);
        let out = render(kind.name, input, r#"{"width": 60}"#)
            .unwrap_or_else(|e| panic!("{}: {e}", kind.name));
        assert!(!out.is_empty(), "{}", kind.name);
        assert!(
            plain(&out).contains(expect),
            "{}: {:?}",
            kind.name,
            plain(&out)
        );
        let html = render_html(kind.name, input, r#"{"width": 60}"#).unwrap();
        assert!(html.starts_with("<!DOCTYPE html>"), "{}", kind.name);
    }
}

#[test]
fn output_is_truecolor_ansi() {
    let out = render("markup", "[#ff8800]orange[/]", "").unwrap();
    assert!(out.contains("\x1b[38;2;255;136;0m"), "{out:?}");
}

#[test]
fn width_is_the_console_width() {
    let out = render("rule", "", r#"{"width": 33}"#).unwrap();
    assert_eq!(plain(&out).trim_end_matches('\n').chars().count(), 33);
}

#[test]
fn errors_are_reported_not_printed() {
    assert!(render("markup", "[/oops]", "")
        .unwrap_err()
        .contains("closing tag"));
    assert!(render("json", "{nope", "").is_err());
    assert!(render("diff", "no separator", "")
        .unwrap_err()
        .contains("======="));
    assert!(render("table", "a,b", r#"{"box": "WAVY"}"#)
        .unwrap_err()
        .contains("WAVY"));
    assert!(render("nope", "", "").unwrap_err().contains("unknown kind"));
    assert!(render("markup", &"x".repeat(70_000), "").is_err());
}

#[test]
fn table_options_apply() {
    let csv = "Name,Size\nalpha,1.2 kB";
    let out = plain(&render("table", csv, r#"{"box": "ASCII", "title": "Files"}"#).unwrap());
    assert!(out.contains("+-"), "{out}");
    assert!(out.contains("Files"), "{out}");
}

#[test]
fn csv_quotes_group_fields() {
    assert_eq!(
        csv_line(r#"a, "b, c" ,"say ""hi""""#),
        ["a", "b, c", r#"say "hi""#]
    );
    assert_eq!(csv_line("one"), ["one"]);
}

#[test]
fn trees_nest_by_indent() {
    let out = plain(&render("tree", "root\n  a\n    a1\n  b\n\tb1", "").unwrap());
    assert!(
        out.contains("├── a\n│   └── a1\n└── b\n    └── b1"),
        "{out}"
    );
}

//! `rich_ext::rst` against `rich-rst` 1.3.2: each document under
//! `fixtures/rst/` must print exactly what upstream printed, captured by
//! `scripts/capture_rst_golden.py`.

use std::fs;
use std::path::{Path, PathBuf};

use rich::{ColorSystem, Console};
use rich_ext::rst::RestructuredText;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/rst")
}

/// Print `source` as rich-cli's `--rst` does.
fn render(source: &str, width: usize, colour: bool) -> String {
    let console = Console::builder()
        .width(width)
        .force_terminal(colour)
        .color_system(colour.then_some(ColorSystem::Truecolor))
        .emoji(false)
        .legacy_windows(false)
        .build();
    console.capture(|console| console.print(&RestructuredText::new(source)))
}

#[test]
fn documents_print_as_rich_rst_prints_them() {
    let mut checked = 0;
    let mut documents: Vec<PathBuf> = fs::read_dir(fixtures())
        .expect("fixtures/rst")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rst"))
        .collect();
    documents.sort();
    for document in documents {
        let source = fs::read_to_string(&document).expect("document");
        let stem = document.file_stem().unwrap().to_string_lossy().into_owned();
        for width in [80, 40] {
            for (extension, colour) in [("txt", false), ("ansi", true)] {
                let expected = document.with_file_name(format!("{stem}.{width}.{extension}"));
                // Documents with code are captured without colour only.
                let Ok(expected) = fs::read_to_string(&expected) else {
                    assert!(colour, "missing {}", expected.display());
                    continue;
                };
                let actual = render(&source, width, colour);
                assert_eq!(
                    actual, expected,
                    "{stem} at width {width} ({extension}) differs from rich-rst"
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 20, "only {checked} fixtures checked");
}

#[test]
fn the_default_lexer_titles_unlabelled_code() {
    let console = Console::builder().width(30).color_system(None).build();
    let document = RestructuredText::new("::\n\n    x = 1").default_lexer("ruby");
    let out = console.render_to_string(&document);
    assert!(out.starts_with("┌─────────── ruby ─"), "{out}");
}

#[test]
fn a_theme_style_overrides_a_default() {
    let mut theme = rich::Theme::default_theme();
    theme.insert(
        "restructuredtext.strong",
        rich::Style::parse("red").unwrap(),
    );
    let console = Console::builder()
        .width(30)
        .force_terminal(true)
        .color_system(Some(ColorSystem::Standard))
        .theme(theme)
        .build();
    let out = console.render_to_string(&RestructuredText::new("**bold**"));
    assert!(out.contains("\x1b[31mbold"), "{out:?}");
}

/// Arbitrary input never panics: random documents built from the pieces of
/// reStructuredText syntax.
#[test]
fn arbitrary_documents_render() {
    const PIECES: &[&str] = &[
        "*",
        "**",
        "`",
        "``",
        "_",
        "__",
        "|",
        ":",
        "::",
        "..",
        ".. ",
        "[",
        "]",
        "<",
        ">",
        "\\",
        "-",
        "=",
        "+",
        "#",
        "(",
        ")",
        " ",
        "  ",
        "    ",
        "\n",
        "\n\n",
        "\t",
        "a",
        "word",
        "1.",
        "i)",
        "-x",
        "--opt=V",
        ":field:",
        ".. note::",
        ".. code:: rust",
        ".. image:: p.png",
        ".. _t: http://x",
        ".. [1]",
        ".. |s| replace:: r",
        "+--+--+",
        "| c |",
        "=== ===",
        ">>> ",
        "é",
        "漢",
        "http://a.b/c",
        "x@y.z",
        ":sub:",
        ":bogus:",
        "`a <b>`_",
        "-- ",
    ];
    let console = Console::builder().width(24).color_system(None).build();
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let len = (next() % 40) as usize;
        let document: String = (0..len)
            .map(|_| PIECES[(next() % PIECES.len() as u64) as usize])
            .collect();
        let _ = console.render_to_string(&RestructuredText::new(&document));
    }
}

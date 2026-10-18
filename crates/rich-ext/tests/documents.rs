//! `rich_ext::document`: the Org, AsciiDoc and man page renderers.
//!
//! Each sample under `fixtures/documents/` is checked against what it printed
//! when it was reviewed, at two widths, without and with colour. There is no
//! upstream to capture from (upstream `rich` has no renderer for these
//! formats), so these are snapshots of our own output: regenerate them with
//! `UPDATE_SNAPSHOTS=1 cargo test -p rs-rich-ext --test documents` after a
//! deliberate change, and read the diff.
//!
//! The rest of the file throws generated and garbled input at the three
//! parsers: none may panic, hang or grow without bound.

#![cfg(feature = "syntax")]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rich::{ColorSystem, Console};
use rich_ext::document::{DocumentRegistry, DocumentView, RenderOptions};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/documents")
}

fn console(width: usize, colour: bool) -> Console {
    Console::builder()
        .width(width)
        .force_terminal(colour)
        .color_system(colour.then_some(ColorSystem::Truecolor))
        .emoji(false)
        .legacy_windows(false)
        .no_color(false)
        .build()
}

/// The view for a sample, by its extension.
fn view(path: &Path, source: &str) -> DocumentView {
    let view = match path.extension().and_then(|ext| ext.to_str()) {
        Some("org") => DocumentView::org(source),
        Some("adoc") => DocumentView::asciidoc(source),
        _ => DocumentView::man(source),
    };
    // As `rich` prints documents without `-y`: link targets written out.
    view.hyperlinks(false)
}

fn render(view: &DocumentView, width: usize, colour: bool) -> String {
    console(width, colour).capture(|console| console.print(view))
}

#[test]
fn samples_print_as_reviewed() {
    let update = std::env::var_os("UPDATE_SNAPSHOTS").is_some();
    let mut samples: Vec<PathBuf> = fs::read_dir(fixtures())
        .expect("fixtures/documents")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("sample.") && name.matches('.').count() == 1)
        })
        .collect();
    samples.sort();
    assert!(samples.len() >= 4, "samples: {samples:?}");
    let mut checked = 0;
    for sample in samples {
        let source = fs::read_to_string(&sample).expect("sample");
        let view = view(&sample, &source);
        let name = sample.file_name().unwrap().to_string_lossy().into_owned();
        for width in [80, 40] {
            for (kind, colour) in [("txt", false), ("ansi", true)] {
                let expected = sample.with_file_name(format!("{name}.{width}.{kind}"));
                let actual = render(&view, width, colour);
                if update {
                    fs::write(&expected, &actual).expect("write the snapshot");
                    continue;
                }
                let expected_text = fs::read_to_string(&expected)
                    .unwrap_or_else(|_| panic!("missing {}", expected.display()));
                assert_eq!(
                    actual, expected_text,
                    "{name} at width {width} ({kind}) differs from its snapshot"
                );
                checked += 1;
            }
        }
    }
    assert!(update || checked >= 16, "only {checked} snapshots checked");
}

#[test]
fn rendering_is_deterministic_and_fits_the_width() {
    for name in ["sample.org", "sample.adoc", "sample.1", "sample.mdoc"] {
        let path = fixtures().join(name);
        let source = fs::read_to_string(&path).unwrap();
        let view = view(&path, &source);
        for width in [20, 33, 61, 120] {
            let first = render(&view, width, false);
            assert_eq!(first, render(&view, width, false), "{name} at {width}");
            for line in first.lines() {
                assert!(
                    rich::cells::cell_len(line) <= width,
                    "{name} at {width}: {line:?} is wider"
                );
            }
        }
    }
}

#[test]
fn hyperlinks_carry_osc8_targets() {
    let console = console(60, true);
    let view = DocumentView::org("See [[https://orgmode.org][Org]].");
    let out = console.capture(|console| console.print(&view));
    assert!(out.contains("\x1b]8;"), "{out:?}");
    assert!(out.contains("https://orgmode.org"));
    let plain = view.clone().hyperlinks(false);
    let plain = self::console(60, false).capture(|console| console.print(&plain));
    assert!(
        plain.contains("See Org (https://orgmode.org)."),
        "{plain:?}"
    );
}

#[test]
fn the_registry_renders_every_builtin() {
    let registry = DocumentRegistry::with_builtins();
    let options = RenderOptions::new().hyperlinks(false);
    for (format, source, expect) in [
        ("markdown", "# Title\n\n*text*", "text"),
        ("rst", "Title\n=====\n\nSome *text*.", "Some text."),
        ("org", "* Heading\n/text/", "text"),
        ("asciidoc", "== Heading\n\n_text_", "text"),
        ("man", ".TH X 1\n.SH NAME\nx \\- text", "x - text"),
    ] {
        let rendered = registry.render(source, format, &options).unwrap();
        let out = console(40, false).render_to_string(rendered.renderable.as_ref());
        assert!(out.contains(expect), "{format}: {out:?}");
    }
}

#[test]
fn diagnostics_name_what_is_not_shown() {
    let view = DocumentView::asciidoc("include::other.adoc[]\n\n++++\n<b>raw</b>\n++++");
    let messages: Vec<String> = view.diagnostics().iter().map(ToString::to_string).collect();
    assert!(
        messages[0].starts_with("line 1: include::other.adoc[]"),
        "{messages:?}"
    );
    let view = DocumentView::man(".TH X 1\n.so other.1\n.EQ\nx = 1\n.EN");
    assert_eq!(view.diagnostics().len(), 2, "{:?}", view.diagnostics());
}

// --- Robustness -------------------------------------------------------------

/// A small deterministic generator (xorshift), so a failure reproduces.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

const COMMON: &[&str] = &[
    " ",
    " ",
    " ",
    "word",
    "text",
    "\n",
    "\n",
    "\n\n",
    "\t",
    "\r\n",
    "\r",
    "漢字",
    "e\u{301}",
    "😀",
    "\u{200d}",
    "\u{0}",
    "\u{1b}[31m",
    "\\",
    "\"",
    "'",
    "(",
    ")",
    "[",
    "]",
    "<",
    ">",
    "{",
    "}",
    "|",
    ":",
    "::",
    "*",
    "_",
    "`",
    "~",
    "=",
    "+",
    "-",
    ".",
    ",",
    "#",
    "/",
    "https://x.org/a",
];

const ORG: &[&str] = &[
    "* ",
    "** TODO ",
    "*** DONE [#A] ",
    " :tag:x:",
    "#+TITLE: ",
    "#+BEGIN_SRC python\n",
    "#+END_SRC\n",
    "#+BEGIN_QUOTE\n",
    "#+END_QUOTE\n",
    "#+begin_example\n",
    "#+BEGIN_NOTE\n",
    "#+BEGIN: clock\n",
    "#+END:\n",
    "#+CAPTION: c\n",
    "#+TODO: A | B\n",
    ":PROPERTIES:\n",
    ":END:\n",
    ":LOGBOOK:\n",
    "SCHEDULED: <2024-01-01 Mon>",
    "| a | b |\n",
    "|---+---|\n",
    "| <r> |\n",
    "- ",
    "  - ",
    "1. ",
    "2) ",
    "[ ] ",
    "[X] ",
    "[@3] ",
    " :: ",
    "[[",
    "]]",
    "][",
    "[[./a.png]]",
    "[fn:1]",
    "[fn::x]",
    "<<t>>",
    "<2024-01-01>--<2024-01-02>",
    "\\\\\n",
    "\\alpha",
    "@@html:x@@",
    ": fixed\n",
    "-----\n",
    "\\begin{eq}\n",
    "\\end{eq}\n",
    "[1/2]",
    "[50%]",
];

const ASCIIDOC: &[&str] = &[
    "= ",
    "== ",
    "====== ",
    "# ",
    ":attr: value\n",
    ":attr!:\n",
    "{attr}",
    "{nbsp}",
    "\\{x}",
    "----\n",
    "....\n",
    "====\n",
    "****\n",
    "____\n",
    "--\n",
    "++++\n",
    "////\n",
    "|===\n",
    ",===\n",
    "```rust\n",
    "```\n",
    "[source,python]\n",
    "[NOTE]\n",
    "[quote, a, b]\n",
    "[verse]\n",
    "[cols=\"1,>2,^3\"]\n",
    "[%header]\n",
    "[[anchor]]\n",
    ".Title\n",
    "* ",
    "** ",
    "- ",
    ". ",
    ".. ",
    "1. ",
    "a. ",
    "<1> ",
    "term:: ",
    "term;; ",
    "+\n",
    "NOTE: ",
    "TIP: ",
    "'''\n",
    "<<<\n",
    "// comment\n",
    "image::a.png[alt]\n",
    "include::x.adoc[]\n",
    "toc::[]\n",
    "**",
    "__",
    "``",
    "##",
    "+pass+",
    "++pass++",
    "[.line-through]#x#",
    "<<id,label>>",
    "link:a[b]",
    "mailto:a@b[c]",
    "kbd:[Ctrl+C]",
    "btn:[OK]",
    "menu:A[B > C]",
    "footnote:[n]",
    "footnote:id[n]",
    "footnote:id[]",
    "&copy;",
    "&#169;",
    "&#xZZ;",
    "(C)",
    "--",
    "...",
    "->",
    " +\n",
    "2+|",
    "a|",
    "3*|",
    "\\|",
    "<.>",
    "# <1>",
    "ifdef::x[]\n",
    "endif::[]\n",
];

const MAN: &[&str] = &[
    ".TH X 1 \"date\" \"src\" \"manual\"\n",
    ".SH ",
    ".SH\n",
    ".SS ",
    ".PP\n",
    ".TP\n",
    ".TQ\n",
    ".IP \\(bu 2\n",
    ".IP 1.\n",
    ".IP\n",
    ".RS\n",
    ".RE\n",
    ".B ",
    ".I ",
    ".BR a b\n",
    ".BI a b c\n",
    ".nf\n",
    ".fi\n",
    ".EX\n",
    ".EE\n",
    ".br\n",
    ".sp\n",
    ".UR https://x\n",
    ".UE\n",
    ".MT a@b\n",
    ".ME\n",
    ".SY cmd\n",
    ".OP -a b\n",
    ".YS\n",
    ".MR ls 1 .\n",
    ".ds S value\n",
    "\\*S",
    "\\*(lq",
    "\\*[x]",
    ".de M\n",
    "..\n",
    ".ig\n",
    ".if n \\{\\\n",
    ".ie t \\{\\\n",
    ".el \\{\\\n",
    "\\}\n",
    ".if '\\*S'v' .B x\n",
    ".so x\n",
    ".EQ\n",
    ".EN\n",
    ".TS\n",
    "tab(;);\n",
    "l l.\n",
    "a;b\n",
    "_\n",
    "T{\n",
    "T}\n",
    ".TE\n",
    "\\fB",
    "\\fI",
    "\\fR",
    "\\fP",
    "\\f(CW",
    "\\f[B]",
    "\\(em",
    "\\[u2014]",
    "\\[char65]",
    "\\-",
    "\\e",
    "\\&",
    "\\c",
    "\\s+2",
    "\\s(12",
    "\\h'1n'",
    "\\h'\\w'x'u'",
    "\\w'",
    "\\o'e\\''",
    "\\n(.g",
    "\\\"comment",
    ".\\\" comment\n",
    "\\\n",
    ".Dd x\n",
    ".Dt X 1\n",
    ".Sh NAME\n",
    ".Nm x\n",
    ".Nd d\n",
    ".Op Fl a Ar b\n",
    ".Bl -tag\n",
    ".Bl -bullet\n",
    ".Bl -column a b\n",
    ".It ",
    ".El\n",
    ".Bd -literal\n",
    ".Ed\n",
    ".Xr ls 1 ,\n",
    ".Dq a .\n",
    ".Ns\n",
    ".Pf ( Ar x\n",
    ".Ex -std\n",
    ".Rv -std f\n",
    ".Unknown a b\n",
    ".Vb 1\n",
    ".Ve\n",
];

fn garble(rng: &mut Rng, tokens: &[&str], pieces: usize) -> String {
    let mut out = String::new();
    for _ in 0..pieces {
        let token = if rng.below(3) == 0 {
            rng.pick(COMMON)
        } else {
            rng.pick(tokens)
        };
        out.push_str(token);
    }
    out
}

fn check(view: &DocumentView, label: &str) {
    for width in [1, 7, 40, 100] {
        let started = Instant::now();
        let out = console(width, width == 40).capture(|console| console.print(view));
        assert!(
            started.elapsed() < Duration::from_secs(20),
            "{label}: rendering at {width} took {:?}",
            started.elapsed()
        );
        assert!(out.len() < 64 * 1024 * 1024, "{label}: {} bytes", out.len());
    }
}

#[test]
fn garbled_input_never_panics() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for round in 0..120 {
        let pieces = 20 + rng.below(400);
        let org = garble(&mut rng, ORG, pieces);
        check(&DocumentView::org(&org), &format!("org #{round}"));
        let asciidoc = garble(&mut rng, ASCIIDOC, pieces);
        check(
            &DocumentView::asciidoc(&asciidoc),
            &format!("asciidoc #{round}"),
        );
        let man = garble(&mut rng, MAN, pieces);
        check(&DocumentView::man(&man), &format!("man #{round}"));
    }
}

#[test]
fn deep_nesting_and_long_lines_are_bounded() {
    let mut cases: Vec<(&str, String)> = Vec::new();
    // Lists indented ever deeper, quotes in quotes, blocks in blocks.
    let org_lists: String = (0..2000)
        .map(|depth| format!("{}- item\n", " ".repeat(depth)))
        .collect();
    cases.push(("org", org_lists));
    cases.push(("org", "#+BEGIN_QUOTE\n".repeat(3000)));
    cases.push((
        "org",
        format!("{}x{}", "*".repeat(50_000), "/".repeat(50_000)),
    ));
    cases.push(("org", format!("[[{}", "[[a][".repeat(20_000))));
    let adoc_lists: String = (1..800)
        .map(|depth| format!("{} item\n", "*".repeat(depth)))
        .collect();
    cases.push(("asciidoc", adoc_lists));
    // Delimiters that never close, each opening a block in the last.
    let examples: String = (4..600)
        .map(|len| format!("{}\n", "=".repeat(len)))
        .collect();
    cases.push(("asciidoc", examples));
    cases.push(("asciidoc", format!("{}x", "*_`#".repeat(30_000))));
    cases.push(("asciidoc", "* a\n+\n".repeat(5000)));
    cases.push(("man", ".RS\n".repeat(5000)));
    cases.push(("man", ".if n \\{\\\n".repeat(3000)));
    cases.push(("man", format!(".Op {}", "Op ".repeat(20_000))));
    cases.push(("man", format!("{}\n", "\\fB\\(em".repeat(50_000))));
    cases.push(("man", ".TP\n".repeat(20_000)));
    for (format, source) in cases {
        let view = match format {
            "org" => DocumentView::org(&source),
            "asciidoc" => DocumentView::asciidoc(&source),
            _ => DocumentView::man(&source),
        };
        let started = Instant::now();
        let out = console(80, false).capture(|console| console.print(&view));
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "{format}: {} bytes took {:?}",
            source.len(),
            started.elapsed()
        );
        assert!(out.lines().all(|line| rich::cells::cell_len(line) <= 80));
    }
}

fn view_as(format: &str, source: &str) -> DocumentView {
    match format {
        "org" => DocumentView::org(source),
        "asciidoc" => DocumentView::asciidoc(source),
        _ => DocumentView::man(source),
    }
}

/// Parse and print `source`, returning the time taken and the output size.
fn timed(format: &str, source: &str) -> (Duration, usize) {
    let started = Instant::now();
    let view = view_as(format, source);
    let out = console(100, false).capture(|console| console.print(&view));
    (started.elapsed(), out.len())
}

/// A document of `bytes` or so that reads like most do: sections of prose
/// with inline markup and lists, a code block every tenth section and a
/// table every twentieth.
fn typical(format: &str, bytes: usize) -> String {
    let mut out = String::new();
    let mut section = 0;
    while out.len() < bytes {
        section += 1;
        let code = section % 10 == 0;
        let table = section % 20 == 0;
        let piece = match format {
            "org" => {
                let mut piece = format!(
                    "* Section {section} :notes:\n\
                     The parser reads *bold*, /italic/, ~code~ and +struck+ text, and links\n\
                     such as [[https://orgmode.org][the Org site]]. A paragraph runs over a\n\
                     few source lines and wraps to the terminal's width when printed.\n\n\
                     Another paragraph follows it, with =verbatim= words and a footnote[fn:1].\n\n\
                     - [X] a checked item\n- [ ] an open item\n  1. a numbered one inside\n\n"
                );
                if code {
                    piece.push_str(
                        "#+BEGIN_SRC python\ndef greet(name):\n    return name\n#+END_SRC\n\n",
                    );
                }
                if table {
                    piece.push_str("| Kind | Count |\n|------+-------|\n| a    |     1 |\n\n");
                }
                piece
            }
            "asciidoc" => {
                let mut piece = format!(
                    "== Section {section}\n\n\
                     The parser reads *bold*, _italic_, `code` and #marked# text, and links\n\
                     such as https://asciidoc.org[the AsciiDoc site]. A paragraph runs over a\n\
                     few source lines and wraps to the terminal's width when printed.\n\n\
                     Another paragraph follows it, with +passthrough+ words.footnote:[A note.]\n\n\
                     * a bulleted item\n* another one\n** a nested one\n\n"
                );
                if code {
                    piece.push_str(
                        "[source,python]\n----\ndef greet(name):\n    return name\n----\n\n",
                    );
                }
                if table {
                    piece.push_str("|===\n| Kind | Count\n\n| a | 1\n|===\n\n");
                }
                piece
            }
            _ => {
                let mut piece = format!(
                    ".SH SECTION {section}\n\
                     The parser reads \\fBbold\\fR, \\fIitalic\\fR and \\(em dashes, and\n\
                     links such as\n.UR https://man7.org\nthe man pages\n.UE .\n\
                     A paragraph runs over a few source lines and wraps when printed.\n\
                     .PP\nAnother paragraph follows it.\n\
                     .TP\n.B \\-n \\fIcount\\fR\nRepeat each greeting.\n\
                     .IP \\(bu 2\nA bulleted item.\n"
                );
                if code {
                    piece.push_str(".EX\ngreet \\-n 2 world\n.EE\n");
                }
                if table {
                    piece.push_str(".TS\nl l.\nKind\tCount\na\t1\n.TE\n");
                }
                piece
            }
        };
        out.push_str(&piece);
    }
    out
}

/// A megabyte of an ordinary document prints well under a second in a
/// release build (a debug build only has to finish).
#[test]
fn a_megabyte_renders_quickly() {
    let limit = if cfg!(debug_assertions) {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(1)
    };
    for format in ["org", "asciidoc", "man"] {
        let source = typical(format, 1024 * 1024);
        let (elapsed, out) = timed(format, &source);
        assert!(out > source.len() / 2, "{format}: little output");
        assert!(
            elapsed < limit,
            "{format}: {} bytes took {elapsed:?}",
            source.len()
        );
    }
}

/// The samples, which use nearly every construct, cost time in proportion
/// to their size. Code blocks and tables cost what core's `Syntax` and
/// `Table` cost, as they do in `Markdown`: a megabyte of nothing but code
/// blocks takes seconds in either.
#[test]
fn rendering_time_grows_linearly() {
    for (format, name) in [
        ("org", "sample.org"),
        ("asciidoc", "sample.adoc"),
        ("man", "sample.1"),
    ] {
        let unit = fs::read_to_string(fixtures().join(name)).unwrap();
        // Keep one document title and running header; repeat the body.
        let body: String = unit
            .lines()
            .filter(|line| {
                !line.starts_with("#+TITLE") && !line.starts_with(".TH") && !line.starts_with("= ")
            })
            .map(|line| format!("{line}\n"))
            .collect();
        let small = body.repeat(64 * 1024 / body.len() + 1);
        let large = body.repeat(4 * (64 * 1024 / body.len() + 1));
        let (small_time, small_out) = timed(format, &small);
        let (large_time, large_out) = timed(format, &large);
        assert!(large_out >= 3 * small_out, "{format}: output did not grow");
        // Four times the input; linear is about four times the time.
        assert!(
            large_time < small_time * 8 + Duration::from_millis(100),
            "{format}: {} bytes took {small_time:?}, {} bytes {large_time:?}",
            small.len(),
            large.len()
        );
    }
}

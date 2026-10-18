//! Org, AsciiDoc and man pages in the CLI (#595): the `--org`, `--asciidoc`
//! and `--man` modes, their subcommands, the document names `--format`
//! takes, and detection by extension and content. Not upstream: rich-cli
//! renders Markdown and reStructuredText only (see `src/documents.rs`).

use std::io::Write;
use std::process::{Command, Stdio};

/// `(stdout, stderr, exit code)` for `rich ARGS`, with `stdin` piped in.
fn rich(args: &[&str], stdin: &str) -> (String, String, i32) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("--no-config")
        .args(args)
        .env_remove("NO_COLOR")
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let written = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    if let Err(error) = written {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.code().unwrap_or(-1),
    )
}

fn ok(args: &[&str], stdin: &str) -> String {
    let (out, err, code) = rich(args, stdin);
    assert_eq!(code, 0, "rich {args:?} failed: {err}");
    out
}

fn fixture(name: &str) -> String {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rich-ext/tests/fixtures/documents"
    );
    std::fs::read_to_string(format!("{dir}/{name}")).unwrap()
}

/// A directory removed when dropped, holding files for the tests.
struct Dir(std::path::PathBuf);

impl Dir {
    fn new() -> Dir {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rich-cli-documents-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Dir(path)
    }

    fn file(&self, name: &str, content: &str) -> String {
        let path = self.0.join(name);
        std::fs::write(&path, content).unwrap();
        path.to_str().unwrap().to_string()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

const PAGE: &str =
    ".TH GREET 1\n.SH NAME\ngreet \\- say hello\n.SH OPTIONS\n.TP\n.B \\-n\nCount.\n";

/// Each document prints as the renderer's own snapshot of it, found by its
/// extension.
#[test]
fn documents_render_as_their_snapshots() {
    let dir = Dir::new();
    for (name, file) in [
        ("sample.org", "notes.org"),
        ("sample.adoc", "guide.adoc"),
        ("sample.1", "greet.1"),
        ("sample.mdoc", "catalog.mdoc"),
    ] {
        let source = fixture(name);
        let expected = fixture(&format!("{name}.80.txt"));
        let path = dir.file(file, &source);
        let out = ok(&[&path], "");
        assert_eq!(out, expected, "{file}");
    }
}

#[test]
fn flags_and_subcommands_choose_the_format() {
    let org = "* TODO Ship it :work:\nSome /text/.\n";
    let adoc = "== Section\n\nNOTE: Mind the gap.\n";
    for args in [
        &["--org", "-"][..],
        &["org", "-"],
        &["--format", "org", "-"],
    ] {
        let out = ok(args, org);
        assert!(out.contains("TODO Ship it  :work:"), "{args:?}: {out}");
    }
    for args in [
        &["--asciidoc", "-"][..],
        &["--adoc", "-"],
        &["asciidoc", "-"],
        &["adoc", "-"],
        &["--format", "adoc", "-"],
    ] {
        let out = ok(args, adoc);
        assert!(
            out.contains("Note") && out.contains("Mind the gap."),
            "{args:?}: {out}"
        );
    }
    for args in [
        &["--man", "-"][..],
        &["--roff", "-"],
        &["man", "-"],
        &["roff", "-"],
        &["--format", "man", "-"],
    ] {
        let out = ok(args, PAGE);
        assert!(out.contains("greet - say hello"), "{args:?}: {out}");
    }
    // `--format` names Markdown and reStructuredText too.
    assert!(ok(&["--format", "md", "-"], "# Title\n\n**bold**").contains("bold"));
    assert!(!ok(&["--format", "rst", "-"], "Title\n=====\n").contains("====="));
}

#[test]
fn a_document_mode_reads_stdin_without_a_resource() {
    let out = ok(&["--org"], "* Heading\n");
    assert!(out.contains("Heading"), "{out}");
    let out = ok(&["--format", "man"], PAGE);
    assert!(out.contains("GREET(1)"), "{out}");
}

#[test]
fn extensions_route_to_documents() {
    let dir = Dir::new();
    for (name, content, expect) in [
        ("a.org", "#+TITLE: Notes\n* Part\n", "Notes"),
        ("a.adoc", "= Guide\n\nBody.\n", "Guide"),
        ("a.asciidoc", "= Guide\n\nBody.\n", "Guide"),
        ("a.man", PAGE, "GREET(1)"),
        ("a.roff", PAGE, "GREET(1)"),
        (
            "a.mdoc",
            ".Dd May 1, 2026\n.Dt A 1\n.Sh NAME\n.Nm a\n",
            "A(1)",
        ),
        // A manual section, confirmed by the content.
        ("greet.1", PAGE, "GREET(1)"),
        ("Module.3pm", PAGE, "GREET(1)"),
    ] {
        let path = dir.file(name, content);
        let out = ok(&[&path], "");
        assert!(out.contains(expect), "{name}: {out}");
        assert!(
            !out.contains("#+TITLE") && !out.contains(".TH"),
            "{name}: {out}"
        );
    }
}

/// Files that look like documents but are not named as them keep today's
/// rendering: a rotated log, source code quoting roff, plain text.
#[test]
fn source_and_logs_are_never_documents() {
    let dir = Dir::new();
    let log = dir.file("syslog.1", "Jan 1 boot ok\n.TH is not here\n");
    assert!(ok(&[&log], "").contains("Jan 1 boot ok"));
    let c = dir.file("gen.c", ".TH X 1\n.SH NAME\nputs(\"x\");\n");
    let out = ok(&[&c], "");
    assert!(out.contains(".TH X 1") && out.contains(".SH NAME"), "{out}");
    let txt = dir.file("notes.txt", "#+TITLE: x\n* heading\n");
    assert!(ok(&[&txt], "").contains("#+TITLE: x"));
    // An extensionless file that is only Markdown-like stays text.
    let readme = dir.file("README", "# Title\n\nSee [docs](https://x.org).\n");
    assert!(ok(&[&readme], "").contains("# Title"));
}

#[test]
fn an_extensionless_file_renders_when_its_content_is_unmistakable() {
    let dir = Dir::new();
    let page = dir.file("GREET", PAGE);
    let out = ok(&[&page], "");
    assert!(out.contains("GREET(1)") && !out.contains(".TH"), "{out}");
    let org = dir.file("TODO", "#+TITLE: Plan\n* TODO Write\n");
    let out = ok(&[&org], "");
    assert!(out.contains("Plan") && !out.contains("#+TITLE"), "{out}");
}

/// Piped input keeps upstream's plain text unless `--format auto` asks for
/// detection.
#[test]
fn stdin_is_detected_only_with_format_auto() {
    let out = ok(&["-"], PAGE);
    assert!(out.contains(".TH GREET 1"), "{out}");
    let out = ok(&["--format", "auto", "-"], PAGE);
    assert!(out.contains("GREET(1)") && !out.contains(".TH"), "{out}");
    let out = ok(&["--format", "auto"], "= Guide\n:toc:\n\n== Part\n");
    assert!(out.contains("Guide") && !out.contains(":toc:"), "{out}");
    // JSON is still JSON, and text still text.
    assert!(ok(&["--format", "auto"], "{\"a\": 1}").contains("\"a\": 1"));
    assert!(ok(&["--format", "auto"], "just words\n").contains("just words"));
}

/// The document an extension picks can still be shown as written.
#[test]
fn syntax_shows_the_source() {
    let dir = Dir::new();
    let org = dir.file("notes.org", "#+TITLE: Notes\n* Part\n");
    let out = ok(&["--syntax", &org], "");
    assert!(out.contains("#+TITLE: Notes"), "{out}");
    let page = dir.file("greet.1", PAGE);
    assert!(ok(&["--syntax", &page], "").contains(".TH GREET 1"));
    // A source option asks for syntax, as for any automatic file.
    assert!(ok(&["--line-numbers", &page], "").contains(".TH GREET 1"));
}

#[test]
fn conflicting_modes_are_usage_errors() {
    for args in [
        &["--org", "--json", "-"][..],
        &["--man", "--asciidoc", "-"],
        &["--format", "org", "--json", "-"],
        &["--format", "man", "--inspect", "-"],
    ] {
        let (_, err, code) = rich(args, "x");
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(err.contains("rich:"), "{err}");
    }
    let (_, err, code) = rich(&["--format", "textile", "-"], "x");
    assert_eq!(code, 2);
    assert!(err.contains("org, asciidoc or man"), "{err}");
    // `--format` takes the last value given.
    assert!(ok(&["--format", "json", "--format", "org", "-"], "* H\n").contains('H'));
}

#[test]
fn lexer_names_the_language_of_unlabelled_code() {
    let page = ".TH X 1\n.SH EXAMPLE\n.EX\necho hi\n.EE\n";
    let out = ok(&["--man", "--lexer", "bash", "-"], page);
    assert!(out.contains("echo hi"), "{out}");
}

#[test]
fn warnings_name_what_is_not_shown() {
    let (out, err, code) = rich(&["--asciidoc", "-"], "Before.\n\ninclude::part.adoc[]\n");
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("include::part.adoc[]"), "{out}");
    assert!(
        err.contains("rich: warning: <stdin>:3: include::part.adoc[] is not followed"),
        "{err}"
    );
    // A JSON report keeps stderr to its one document.
    let (_, err, _) = rich(&["--asciidoc", "--report", "json", "-"], "include::x[]\n");
    assert_eq!(err.lines().count(), 1, "{err}");
    assert!(err.contains("\"ok\":true"), "{err}");
}

#[test]
fn links_write_their_target_unless_hyperlinks_are_asked_for() {
    let source = "See [[https://orgmode.org][Org]].\n";
    let out = ok(&["--org", "-"], source);
    assert!(out.contains("Org (https://orgmode.org)"), "{out}");
    let out = ok(&["--org", "-y", "--force-terminal", "-"], source);
    assert!(out.contains("\x1b]8;"), "{out:?}");
}

#[test]
fn compressed_man_pages_explain_themselves() {
    let dir = Dir::new();
    let page = dir.file("ls.1.gz", "\u{1f}\u{8b}binary");
    let (out, err, code) = rich(&[&page], "");
    assert_eq!(code, 3, "{out}{err}");
    assert!(err.contains("| rich --man -"), "{err}");
}

#[test]
fn config_accepts_document_modes_and_formats() {
    let dir = Dir::new();
    let config = dir.file("rich.toml", "[defaults]\nmode = \"org\"\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--config", &config, "-"])
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"* Configured\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        out.contains("Configured") && !out.contains("* Configured"),
        "{out}"
    );
    let config = dir.file("format.toml", "[defaults]\nformat = \"man\"\n");
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--config", &config, &dir.file("x.txt", PAGE)])
        .env("COLUMNS", "80")
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&output.stdout);
    assert!(
        out.contains("GREET(1)"),
        "{out}{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

//! rich-cli 1.8.1's rendering options (#542), one test each, checked
//! against the rich-cli oracle for their semantics: `--head`/`--tail`, `-n`,
//! `-g`, `--lexer`, `--emoji`, `--soft`, `--no-wrap`, `-W`, the `--text-*`
//! justifies, `--rule-style`/`--rule-char`, `--force-terminal`, and the short
//! aliases.

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
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
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

/// Each line without its trailing padding.
fn lines(out: &str) -> Vec<&str> {
    out.lines().map(str::trim_end).collect()
}

fn ten_lines() -> String {
    (1..=10).map(|n| format!("line {n}\n")).collect()
}

/// A file in a fresh directory, for options that need a file name.
fn file(name: &str, content: &str) -> (tempdir::Dir, String) {
    let dir = tempdir::Dir::new();
    let path = dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    let path = path.to_str().unwrap().to_string();
    (dir, path)
}

mod tempdir {
    /// A directory removed when dropped.
    pub struct Dir(std::path::PathBuf);

    impl Dir {
        pub fn new() -> Dir {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "rich-cli-options-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Dir(path)
        }

        pub fn path(&self) -> &std::path::Path {
            &self.0
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

#[test]
fn head_shows_the_first_lines_and_h_is_head() {
    let (_dir, path) = file("ten.py", &ten_lines());
    for flag in ["--head", "-h"] {
        let out = ok(&[&path, flag, "2"], "");
        assert_eq!(lines(&out), ["line 1", "line 2"], "{flag}");
    }
}

#[test]
fn tail_keeps_upstreams_arithmetic() {
    // rich-cli's `(num_lines - tail + 2, num_lines + 1)`: `--tail 3` shows
    // the last two lines, then a blank one.
    let (_dir, path) = file("ten.py", &ten_lines());
    for flag in ["--tail", "-t"] {
        let out = ok(&[&path, flag, "3"], "");
        assert_eq!(lines(&out), ["line 9", "line 10", ""], "{flag}");
    }
}

#[test]
fn head_and_tail_check_their_values() {
    let (_dir, path) = file("ten.py", &ten_lines());
    let (_, err, code) = rich(&[&path, "-h", "2", "-t", "2"], "");
    assert_eq!(code, 2);
    assert!(err.contains("cannot specify both head and tail"), "{err}");
    let (_, err, code) = rich(&[&path, "--head", "0"], "");
    assert_eq!(code, 2);
    assert!(err.contains("0 is not in the range x>=1"), "{err}");
    // Markdown has no lines to cut: a flag that does nothing is an error.
    let (_dir, md) = file("notes.md", "# Hi\n");
    let (_, err, code) = rich(&[&md, "--head", "1"], "");
    assert_eq!(code, 2);
    assert!(
        err.contains("--head/--tail only has an effect with"),
        "{err}"
    );
}

#[test]
fn head_and_tail_cut_csv_rows() {
    let (_dir, path) = file("t.csv", "name,qty\na,1\nb,2\nc,3\nd,4\n");
    let rows = |args: &[&str]| -> Vec<String> {
        lines(&ok(args, ""))
            .into_iter()
            .filter(|line| line.starts_with('│'))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    };
    assert_eq!(rows(&[&path, "--head", "2"]), ["│ a │ 1 │", "│ b │ 2 │"]);
    assert_eq!(rows(&[&path, "--tail", "2"]), ["│ c │ 3 │", "│ d │ 4 │"]);
    // `render_csv` takes the head when both are given.
    assert_eq!(rows(&[&path, "-h", "1", "-t", "3"]), ["│ a │ 1 │"]);
}

#[test]
fn line_numbers_number_source() {
    let (_dir, path) = file("ten.py", &ten_lines());
    for flag in ["--line-numbers", "-n"] {
        let out = ok(&[&path, flag, "-h", "2"], "");
        assert_eq!(lines(&out), ["   1 line 1", "   2 line 2"], "{flag}");
    }
}

#[test]
fn guides_draw_indentation_guides() {
    let (_dir, path) = file("nested.py", "def f():\n    if x:\n        return 1\n");
    let plain = ok(&[&path], "");
    assert!(!plain.contains('│'), "{plain}");
    for flag in ["--guides", "-g"] {
        let out = ok(&[&path, flag], "");
        assert!(out.lines().nth(2).unwrap().contains('│'), "{flag}: {out}");
    }
}

#[test]
fn lexer_chooses_the_language() {
    // A `.txt` file is plain text; `--lexer python` highlights it.
    let (_dir, path) = file("code.txt", "def f():\n    return 1\n");
    let plain = ok(&[&path, "--syntax", "--force-terminal"], "");
    let python = ok(&[&path, "--lexer", "python", "--force-terminal"], "");
    assert_ne!(plain, python);
    assert!(python.contains("def"), "{python}");
    // Automatic mode becomes syntax for it, as upstream renders it.
    let auto = ok(&[&path, "--lexer", "python", "--force-terminal"], "");
    assert_eq!(auto, python);
}

#[test]
fn emoji_codes_are_kept_unless_asked() {
    assert_eq!(ok(&["-p", ":smile: hi"], ""), ":smile: hi\n");
    assert_eq!(ok(&["-p", ":smile: hi", "--emoji"], ""), "😄 hi\n");
}

#[test]
fn soft_wrap_leaves_lines_whole() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    let out = ok(&["-p", text, "--soft", "-w", "10"], "");
    assert_eq!(out, format!("{text}\n"));
    // Byte for byte what rich 15.0.0's `print(…, soft_wrap=True)` writes at
    // 80 columns: the line is neither wrapped nor cut.
    let long = format!("[bold]{}[/bold] end", "x".repeat(90));
    let out = ok(&["-p", &long, "--soft", "--force-terminal"], "");
    assert_eq!(out, format!("\x1b[1m{}\x1b[0m end\n", "x".repeat(90)));
    // Without it, the same line is wrapped at the width.
    let wrapped = ok(&["-p", &long, "--force-terminal"], "");
    assert!(wrapped.lines().count() > 1, "{wrapped:?}");
}

#[test]
fn no_wrap_crops_text_and_source() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    assert_eq!(
        ok(&["-p", text, "--no-wrap", "-w", "12"], ""),
        "aaaa bbbb cc\n"
    );
    let (_dir, path) = file("long.py", &format!("x = '{}'\n", "y".repeat(100)));
    let wrapped = ok(&[&path], "");
    let cropped = ok(&[&path, "--no-wrap"], "");
    let rows = |out: &str| out.lines().filter(|line| !line.trim().is_empty()).count();
    assert!(rows(&wrapped) > 1, "{wrapped}");
    assert_eq!(rows(&cropped), 1, "{cropped}");
}

#[test]
fn max_width_narrows_the_print() {
    let text = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    for flag in ["--max-width", "-W"] {
        let out = ok(&["-p", text, flag, "12"], "");
        assert_eq!(
            lines(&out),
            ["aaaa bbbb", "cccc dddd", "eeee ffff", "gggg hhhh"],
            "{flag}"
        );
    }
    // Upstream's default is -1: zero or less means no limit.
    assert_eq!(ok(&["-p", "hi", "-W", "-1"], ""), "hi\n");
}

#[test]
fn text_justify_needs_a_width_as_upstream() {
    // `print`'s `Text.join` drops a bare text's justify; `-w` keeps it.
    assert_eq!(ok(&["-p", "hi", "-C"], ""), "hi\n");
    assert_eq!(ok(&["-p", "hi", "-C", "-w", "10"], ""), "    hi    \n");
    assert_eq!(ok(&["-p", "hi", "-R", "-w", "10"], ""), "        hi\n");
    assert_eq!(
        ok(&["-p", "hi", "--text-left", "-w", "10"], ""),
        "hi        \n"
    );
    // `if text_left … elif text_right …`: left wins whatever the order.
    assert_eq!(
        ok(&["-p", "hi", "-R", "-L", "-w", "10"], ""),
        "hi        \n"
    );
    let full = ok(&["-p", "aa bb cc dd", "-F", "-w", "7"], "");
    assert_eq!(lines(&full), ["aa   bb", "cc dd"]);
}

#[test]
fn rule_options_draw_the_line() {
    let out = ok(&["-u", "hi", "--rule-char", "=", "-w", "20"], "");
    assert_eq!(out, "======== hi ========\n");
    let out = ok(&["--rule", "hi", "-L", "-w", "10"], "");
    assert_eq!(out, "hi ───────\n");
    let styled = ok(
        &["-u", "--rule-style", "red", "-w", "4", "--force-terminal"],
        "",
    );
    assert!(styled.contains("\x1b[31m"), "{styled:?}");
    let (_, err, code) = rich(&["-p", "x", "--rule-char", "="], "");
    assert_eq!(code, 2);
    assert!(
        err.contains("--rule-char only has an effect with --rule"),
        "{err}"
    );
}

#[test]
fn force_terminal_writes_styles_to_a_pipe() {
    assert_eq!(ok(&["-p", "[bold]b[/]"], ""), "b\n");
    let forced = ok(&["-p", "[bold]b[/]", "--force-terminal"], "");
    assert!(forced.contains("\x1b[1m"), "{forced:?}");
}

#[test]
fn short_aliases_match_upstream() {
    // -J (upstream's --json), -u (--rule), -v (--version).
    assert_eq!(
        ok(&["-J", "-"], "{\"a\": 1}"),
        ok(&["--json", "-"], "{\"a\": 1}")
    );
    assert_eq!(ok(&["-u", "-w", "3"], ""), ok(&["--rule", "-w", "3"], ""));
    assert!(ok(&["-v"], "").starts_with("rich (rs-rich-cli) "));
    // -d (--padding), -a (--panel), -l/-c/-r (--left/--center/--right).
    assert_eq!(
        ok(&["-p", "hi", "-d", "1", "-a", "square", "-c"], ""),
        ok(
            &[
                "-p",
                "hi",
                "--padding",
                "1",
                "--panel",
                "square",
                "--center"
            ],
            ""
        )
    );
    assert_eq!(
        ok(&["-p", "hi", "-w", "4", "-r"], ""),
        ok(&["-p", "hi", "-w", "4", "--right"], "")
    );
    assert_eq!(
        ok(&["-p", "hi", "-w", "4", "-l"], ""),
        ok(&["-p", "hi", "-w", "4", "--left"], "")
    );
}

#[test]
fn notebook_code_cells_take_the_source_options() {
    let notebook = r#"{"cells": [{"cell_type": "code", "execution_count": 1,
        "source": ["a = 1\n", "b = 2\n", "c = 3\n"], "outputs": []}],
        "metadata": {"kernelspec": {"language": "python"}}}"#;
    let (_dir, path) = file("n.ipynb", notebook);
    let out = ok(&[&path, "-n", "-h", "2"], "");
    assert!(out.contains("1 a = 1"), "{out}");
    assert!(out.contains("2 b = 2"), "{out}");
    assert!(!out.contains("c = 3"), "{out}");
}

/// `--rst` prints what rich-cli's `rich-rst` prints: the renderer's own
/// parity fixture, byte for byte, found by the `.rst` extension.
#[test]
fn rst_renders_as_upstream_and_is_detected_by_extension() {
    let fixtures = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rich-ext/tests/fixtures/rst"
    );
    let source = std::fs::read_to_string(format!("{fixtures}/sample.rst")).unwrap();
    let expected = std::fs::read_to_string(format!("{fixtures}/sample.80.txt")).unwrap();
    let (_dir, path) = file("doc.rst", &source);
    assert_eq!(ok(&[&path], ""), expected);
    assert_eq!(ok(&["--rst", "-"], &source), expected);
}

/// With `--rst`, `--lexer` is the lexer for code blocks that name none
/// (upstream's `default_lexer=lexer or "python"`).
#[test]
fn rst_lexer_is_the_default_for_unlabelled_code() {
    let source = "Code::\n\n    puts 1\n\n.. code:: sh\n\n   echo 1\n";
    let out = ok(&["--rst", "--lexer", "ruby", "-"], source);
    assert!(out.contains(" ruby "), "{out}");
    assert!(out.contains(" sh "), "{out}");
    let out = ok(&["--rst", "-"], source);
    assert!(out.contains(" python "), "{out}");
}

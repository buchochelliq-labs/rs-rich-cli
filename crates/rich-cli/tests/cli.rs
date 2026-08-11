//! Integration tests for the `rich` CLI: exercise each render mode end to end.
//!
//! These drive the built binary (via `CARGO_BIN_EXE_rich`) and assert on plain
//! (`--no-color`) output, so they check argument routing and library wiring
//! without depending on exact ANSI bytes (that parity lives in the `rich` crate).

use std::io::Write;
use std::process::{Command, Stdio};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rich"))
}

/// Run the CLI with `args`, feeding `stdin`, returning `(stdout, success)`.
///
/// `COLUMNS` is pinned so the console width is the same on every machine: since
/// `--width` stopped shrinking the console, several assertions below are about
/// where output lands *within the terminal*, and a 120-column developer machine
/// would otherwise disagree with CI about the answer.
fn run(args: &[&str], stdin: &str) -> (String, bool) {
    let mut child = bin()
        .args(args)
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rich");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().expect("wait rich");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        output.status.success(),
    )
}

#[test]
fn version_flag() {
    let (out, ok) = run(&["--version"], "");
    assert!(ok);
    // Tracks the crate version, which is independent SemVer — deliberately NOT
    // the upstream rich-cli version (see AGENTS.md → Versioning).
    let expected = format!("rich (rs-rich-cli) {}", env!("CARGO_PKG_VERSION"));
    assert!(out.contains(&expected), "got: {out:?}, want: {expected:?}");
}

#[test]
fn print_mode_renders_markup() {
    let (out, ok) = run(&["--no-color", "-p", "[bold]hi[/] there"], "");
    assert!(ok);
    assert_eq!(out, "hi there\n");
}

#[test]
fn markdown_mode_from_stdin() {
    let (out, ok) = run(
        &["--no-color", "--width", "20", "-m", "-"],
        "# Heading\n\ntext",
    );
    assert!(ok);
    assert!(out.contains("Heading"), "got: {out:?}");
    assert!(out.contains("text"), "got: {out:?}");
}

#[test]
fn json_mode_pretty_prints_from_stdin() {
    let (out, ok) = run(&["--no-color", "-j", "-"], r#"{"a": 1, "b": [2, 3]}"#);
    assert!(ok);
    // Pretty-printed with 2-space indent.
    assert!(out.contains("{\n  \"a\": 1"), "got: {out:?}");
}

#[test]
fn rule_mode_draws_title() {
    let (out, ok) = run(&["--no-color", "--width", "12", "--rule", "hi"], "");
    assert!(ok);
    assert!(out.contains("hi"), "got: {out:?}");
    assert!(out.contains('─'), "expected box rule chars, got: {out:?}");
}

/// `--center` centres within the CONSOLE, and `--width` only bounds the
/// renderable inside it.
///
/// This test used to assert `"   mid   \n"` — the whole line nine cells wide —
/// which is what shrinking the console to `--width` produces. Upstream keeps the
/// console at 80, renders "mid" inside its nine columns, and centres the block
/// that comes out (three cells, because a `--print` text does not pad itself),
/// so "mid" starts at column 38 of 80. Checked against rich 15.0.0 driving
/// rich-cli 1.8.1's own pipeline.
#[test]
fn center_justifies_print_output() {
    let (out, ok) = run(&["--no-color", "-w", "9", "--center", "-p", "mid"], "");
    assert!(ok);
    let line = out.trim_end_matches('\n');
    assert_eq!(line.chars().count(), 80, "got: {out:?}");
    assert_eq!(line.find("mid"), Some(38), "got: {out:?}");
}

#[test]
fn conflicting_modes_error() {
    let (_out, ok) = run(&["-m", "-j", "x"], "");
    assert!(!ok, "two mode flags should fail");
}

// --- Regressions found by UAT ------------------------------------------------
//
// Every test below reproduces a defect that shipped while the whole gate was
// green. They exist because the type checker, the linter and 274 other tests
// could not see any of them.

/// The image pair the diff tests run against, as absolute paths.
#[cfg(feature = "art")]
fn diff_fixtures() -> (String, String) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("rich-art/tests/fixtures");
    (
        dir.join("halo-before.png").display().to_string(),
        dir.join("halo-after.png").display().to_string(),
    )
}

/// `"NaN"` parses as an f32 and every comparison against it is false, so this
/// silently switched the CI gate off and reported a pass. A gate that can be
/// disabled by an empty template variable is worse than no gate at all.
#[cfg(feature = "art")]
#[test]
fn a_nonsense_threshold_is_rejected_rather_than_disabling_the_gate() {
    let (before, after) = diff_fixtures();
    for bad in ["NaN", "inf", "-1", "101", "1e3"] {
        let (_out, ok) = run(
            &[
                "--diff",
                &before,
                &after,
                "--image-mode",
                "none",
                "--threshold",
                bad,
            ],
            "",
        );
        assert!(
            !ok,
            "--threshold {bad} should be rejected, but the run succeeded"
        );
    }
}

/// A threshold inside the range must still gate normally.
#[cfg(feature = "art")]
#[test]
fn a_valid_threshold_still_gates() {
    let (before, after) = diff_fixtures();
    let (_out, over) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "2",
        ],
        "",
    );
    assert!(!over, "5.4% change against a 2% limit must fail");
    let (_out, under) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "90",
        ],
        "",
    );
    assert!(under, "5.4% change against a 90% limit must pass");
}

/// The gate compared full precision against a one-decimal display, so a limit
/// equal to the reported figure printed "FAIL 5.4% changed, limit 5.4%" — a
/// verdict contradicting itself, and unexplainable from the output.
#[cfg(feature = "art")]
#[test]
fn the_threshold_matches_the_percentage_it_prints() {
    let (before, after) = diff_fixtures();
    let (out, ok) = run(
        &[
            "--diff",
            &before,
            &after,
            "--image-mode",
            "none",
            "--threshold",
            "5.4",
        ],
        "",
    );
    assert!(
        ok,
        "a limit equal to the reported figure must not fail; got: {out}"
    );
}

/// Redirected output has no colour, and half-blocks without colour are a
/// rectangle of identical characters — 30 rows carrying no information.
#[cfg(feature = "art")]
#[test]
fn a_colourless_diff_renders_something_readable() {
    let (before, after) = diff_fixtures();
    let (out, ok) = run(&["--diff", &before, &after, "-w", "40", "--no-color"], "");
    assert!(ok);
    let picture: Vec<&str> = out
        .lines()
        .take_while(|line| !line.contains("of the canvas"))
        .filter(|line| !line.trim().is_empty())
        .collect();
    let distinct: std::collections::HashSet<_> = picture.iter().collect();
    assert!(
        distinct.len() > 1,
        "the picture collapsed to {} distinct row(s) — that is a solid block, not an image",
        distinct.len()
    );
}

/// Flags that decorate a single rendered resource were accepted and silently
/// dropped by `--diff`, which reads as the flag having no effect.
#[cfg(feature = "art")]
#[test]
fn decoration_flags_are_refused_with_diff_rather_than_ignored() {
    let (before, after) = diff_fixtures();
    for flag in [
        vec!["--panel", "rounded"],
        vec!["--padding", "2"],
        vec!["--center"],
    ] {
        let mut args = vec![
            "--diff",
            before.as_str(),
            after.as_str(),
            "--image-mode",
            "none",
        ];
        args.extend(flag.iter().copied());
        let (_out, ok) = run(&args, "");
        assert!(
            !ok,
            "{flag:?} with --diff should be an error, not a silent no-op"
        );
    }
}

/// Width 0 rendered nothing at all and exited 0, while a negative width was
/// correctly refused.
#[test]
fn a_zero_width_is_refused() {
    let (_out, ok) = run(&["--width", "0", "-p", "hello"], "");
    assert!(!ok, "--width 0 should be rejected");
}

/// Windows editors write a UTF-8 BOM by default. It made valid JSON fail to
/// parse "at column 1", and Markdown render its first heading as literal text
/// at exit 0 — wrong output with no diagnostic, which is the worse of the two.
#[test]
fn a_utf8_bom_does_not_break_parsing() {
    let dir = std::env::temp_dir().join("rich-bom-test");
    std::fs::create_dir_all(&dir).unwrap();

    let json = dir.join("bom.json");
    std::fs::write(&json, "\u{feff}{\"ok\": true}").unwrap();
    let (_out, ok) = run(&["-j", json.to_str().unwrap()], "");
    assert!(ok, "BOM-prefixed JSON should parse");

    let md = dir.join("bom.md");
    std::fs::write(&md, "\u{feff}# Heading\n").unwrap();
    let (out, ok) = run(&["-m", md.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        !out.contains("# Heading"),
        "the heading rendered literally, so the BOM survived: {out:?}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A directory reached `read_to_string` as "Access is denied" on Windows,
/// sending the reader hunting for a permissions problem.
#[test]
fn a_directory_says_it_is_a_directory() {
    let dir = std::env::temp_dir();
    let (_out, ok) = run(&[dir.to_str().unwrap()], "");
    assert!(!ok, "a directory is not a renderable resource");
}

// --- Regressions found by UAT round 2 ----------------------------------------

/// Python reads text with universal newlines, so upstream never sees a CR;
/// `read_to_string` hands them through. `--syntax` emitted `code` + CR +
/// padding, so a terminal returned to column 0 and the padding overwrote the
/// code — a blank rectangle at exit 0. Piping hid it completely, because the
/// bytes were all present and only a terminal acts on the CR.
#[test]
fn a_crlf_file_does_not_leak_carriage_returns() {
    let dir = std::env::temp_dir().join("rich-crlf-test");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("crlf.py");
    std::fs::write(&file, "import os\r\ndef f():\r\n    return 1\r\n").unwrap();

    for mode in [["-x"], ["-m"]] {
        let (out, ok) = run(&[mode[0], file.to_str().unwrap(), "--no-color"], "");
        assert!(ok, "{mode:?} should render a CRLF file");
        assert!(
            !out.contains('\r'),
            "{mode:?} leaked a carriage return; a terminal would overwrite the line: {out:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// A flag whose mode is absent used to be silently ignored at exit 0. For
/// `--threshold` that meant a CI job which lost its `--diff` became a
/// permanently green gate — the same failure the NaN check closed, from the
/// other side.
///
/// `--title`/`--caption` have left this list: upstream feeds them to the CSV
/// table as well as to the panel, so refusing them without `--panel` made
/// `rich --csv sales.csv --title Sales` impossible. `--panel-style` and
/// `--expand` really do nothing without a box to draw, so they stay.
#[test]
fn a_flag_without_its_mode_is_refused() {
    for args in [
        vec!["--threshold", "5"],
        vec!["--image-mode", "blocks"],
        vec!["--loop", "2"],
        vec!["--panel-style", "dim"],
        vec!["--expand"],
    ] {
        let mut full = args.clone();
        full.push("README.md");
        let (_out, ok) = run(&full, "");
        assert!(
            !ok,
            "{args:?} without its mode should be refused, not ignored"
        );
    }
}

/// A malformed notebook printed an error and exited 0, while malformed JSON
/// exited 1. A script cannot tell that apart from success.
#[test]
fn a_broken_notebook_exits_non_zero() {
    let dir = std::env::temp_dir().join("rich-ipynb-test");
    std::fs::create_dir_all(&dir).unwrap();

    let broken = dir.join("broken.ipynb");
    std::fs::write(&broken, "{ not json").unwrap();
    let (_out, ok) = run(&["--ipynb", broken.to_str().unwrap()], "");
    assert!(!ok, "malformed notebook JSON must exit non-zero");

    // Valid JSON that is not a notebook printed nothing at all, at exit 0.
    let not_nb = dir.join("not-a-notebook.ipynb");
    std::fs::write(&not_nb, "{\"a\": 1}").unwrap();
    let (_out, ok) = run(&["--ipynb", not_nb.to_str().unwrap()], "");
    assert!(!ok, "JSON without a `cells` array is not a notebook");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The no-argument demo built its consoles with `force_terminal(true)`, so it
/// wrote escape sequences into a pipe and ignored `--no-color` — on that path
/// alone, while every other mode honoured both.
#[test]
fn the_demo_emits_no_escapes_when_piped() {
    for args in [vec![], vec!["--no-color"]] {
        let (out, ok) = run(&args, "");
        assert!(ok);
        assert!(!out.is_empty(), "the demo should still produce output");
        assert!(
            !out.contains('\u{1b}'),
            "{args:?}: escape sequences leaked into a pipe"
        );
    }
}

// --- Regressions found by UAT round 4 ----------------------------------------

/// nbformat's `source`/`text` arrays carry their own trailing newlines, but a
/// `traceback` array does not — each element is a line. Joining both the same
/// way fused a whole stack trace onto one run-on line, which is the single
/// output a user opens a failed notebook to read.
#[test]
fn a_notebook_traceback_keeps_its_line_breaks() {
    let dir = std::env::temp_dir().join("rich-tb-test");
    std::fs::create_dir_all(&dir).unwrap();
    let nb = dir.join("tb.ipynb");
    std::fs::write(
        &nb,
        r#"{"cells":[{"cell_type":"code","source":["1/0"],"outputs":[
            {"output_type":"error","ename":"E","evalue":"v",
             "traceback":["LINE_ONE","LINE_TWO","LINE_THREE"]}]}],
            "metadata":{},"nbformat":4,"nbformat_minor":5}"#,
    )
    .unwrap();

    let (out, ok) = run(&["--ipynb", nb.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        !out.contains("LINE_ONELINE_TWO"),
        "traceback lines were fused together:\n{out}"
    );
    for line in ["LINE_ONE", "LINE_TWO", "LINE_THREE"] {
        assert!(out.contains(line), "{line} missing from:\n{out}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

/// Columns were derived from the header alone, so a row carrying more fields
/// than the header names silently lost the surplus — data loss in a tool whose
/// entire job is showing you the file.
#[test]
fn a_csv_row_wider_than_its_header_keeps_every_field() {
    let dir = std::env::temp_dir().join("rich-csv-test");
    std::fs::create_dir_all(&dir).unwrap();
    let csv = dir.join("ragged.csv");
    // Row 2 is short (must be padded) and row 3 is long (must not be dropped).
    std::fs::write(&csv, "a,b,c\n1,2\n3,4,5,SURPLUS\n").unwrap();

    let (out, ok) = run(
        &["--csv", csv.to_str().unwrap(), "--no-color", "-w", "60"],
        "",
    );
    assert!(ok);
    assert!(
        out.contains("SURPLUS"),
        "the field past the header count was dropped:\n{out}"
    );
    // Four columns, not three: `csv.Sniffer` cannot read this file at all
    // (the rows disagree about how many commas a row has), so it takes the
    // excel fallback — and the fourth column still has to exist.
    let header = out.lines().nth(1).unwrap_or_default();
    assert_eq!(
        header.matches('│').count() + header.matches('┃').count(),
        5,
        "expected 4 columns worth of dividers in:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

// --- Regressions found by UAT round 8 ----------------------------------------

/// Write `content` to a temp file called `name` and hand back its path.
fn fixture(name: &str, content: &[u8]) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("rich-uat8");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

/// Nothing beginning with `-` could be printed or opened, because `--` was
/// rejected as an unknown option instead of ending the option list. Click gives
/// upstream this for free, so it was simply missing here. (#77)
#[test]
fn a_double_dash_ends_the_options() {
    let (out, ok) = run(&["--no-color", "-p", "--", "-5 degrees"], "");
    assert!(ok, "`--` should end the options, got: {out:?}");
    assert_eq!(out, "-5 degrees\n");

    // The whole point is that this is unreachable otherwise.
    let (_out, ok) = run(&["--no-color", "-p", "-5 degrees"], "");
    assert!(
        !ok,
        "without `--`, a leading dash is still an unknown option"
    );

    // And `--` must be consumed, not left behind as the resource itself.
    let file = fixture("dashed.md", b"# Heading\n");
    let (out, ok) = run(&["--no-color", "-m", "--", file.to_str().unwrap()], "");
    assert!(ok, "got: {out:?}");
    assert!(out.contains("Heading"), "got: {out:?}");
}

/// `;`, `|` and tab delimiters went undetected — a European-style export was
/// rendered as one giant single-column cell — and the first row was always
/// assumed to be a header. Upstream runs `csv.Sniffer` for both. (#80)
#[test]
fn csv_sniffs_its_delimiter_and_its_header() {
    let semi = fixture("semi.csv", b"name;age;city\nAlice;30;Paris\nBob;25;Lyon\n");
    let (out, ok) = run(&["--csv", semi.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        !out.contains("name;age;city"),
        "the semicolons were never split, so the row is one cell:\n{out}"
    );
    for cell in ["name", "age", "city", "Alice", "Paris"] {
        assert!(out.contains(cell), "{cell} missing from:\n{out}");
    }
    // Three columns => four vertical dividers on the header row.
    let header = out.lines().nth(1).unwrap_or_default();
    assert_eq!(
        header.matches('┃').count(),
        4,
        "expected three sniffed columns in:\n{out}"
    );

    let pipe = fixture("pipe.csv", b"a|b|c\n1|2|3\n4|5|6\n");
    let (out, ok) = run(&["--csv", pipe.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(!out.contains("a|b|c"), "the pipes were never split:\n{out}");

    // All-numeric rows have no header, so upstream draws SQUARE with no header
    // rule and treats row 1 as data. `┏` is HEAVY_HEAD's top-left corner.
    let plain = fixture("plain.csv", b"1,2,3\n4,5,6\n7,8,9\n");
    let (out, ok) = run(&["--csv", plain.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        !out.contains('┏'),
        "row 1 was assumed to be a header, but the sniffer says it is data:\n{out}"
    );

    // …and a file that does have one still gets it, including the case that
    // CPython's *current* typing gets right and its historical typing does not:
    // a column mixing `10` and `9.5`.
    let mixed = fixture("mixed.csv", b"price,note\n10,aa\n9.5,bbb\n");
    let (out, ok) = run(&["--csv", mixed.to_str().unwrap(), "--no-color"], "");
    assert!(ok);
    assert!(
        out.contains('┏'),
        "a real header went undetected (int/float column read as inconsistent?):\n{out}"
    );
}

/// A cp1252 file — what Excel writes on a Windows box — exited 1 with an
/// encoding error. Upstream opens files with `errors="replace"` and renders.
/// Stdin is a different matter: `sys.stdin.read()` really is strict upstream,
/// so it stays strict here. (#81)
#[test]
fn a_non_utf8_file_is_decoded_rather_than_refused() {
    // "José" / "Zürich" in cp1252: 0xE9 and 0xFC are not valid UTF-8.
    let path = fixture("cp1252.csv", b"name,city\nJos\xe9,Z\xfcrich\nBob,Lyon\n");
    let (out, ok) = run(&["--csv", path.to_str().unwrap(), "--no-color"], "");
    assert!(ok, "a cp1252 export must render, not exit 1:\n{out}");
    assert!(
        out.contains('\u{fffd}'),
        "expected replacement chars in:\n{out}"
    );
    assert!(out.contains("rich"), "the row is missing from:\n{out}"); // "Zürich"

    // Stdin has no `errors="replace"` upstream, and must keep failing loudly.
    let mut child = bin()
        .args(["-p", "-"])
        .env("COLUMNS", "80")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn rich");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"\xff\xfe bad")
        .unwrap();
    let output = child.wait_with_output().expect("wait rich");
    assert!(
        !output.status.success(),
        "undecodable stdin must still fail, as it does upstream"
    );
}

/// `--panel` and `--padding` filled the console whatever they contained.
/// Upstream shrinks both to the renderable and only expands on `-e`/`--width`.
/// (#82)
#[test]
fn a_panel_and_padding_shrink_to_their_content() {
    let width = |out: &str| out.lines().next().unwrap_or_default().chars().count();

    let (out, ok) = run(&["--no-color", "--panel", "rounded", "-p", "hello"], "");
    assert!(ok);
    assert_eq!(width(&out), 9, "a panel round `hello` is 9 cells:\n{out}");

    // Every mode, because only `Text` has a tight measurement in the core crate:
    // Table, Json and Syntax inherit "as wide as you like" and would collapse
    // back to the full console if the fit were left to them.
    let json = fixture("fit.json", b"{\"a\": 1}\n");
    let (out, ok) = run(
        &[
            "--no-color",
            "--panel",
            "rounded",
            "-j",
            json.to_str().unwrap(),
        ],
        "",
    );
    assert!(ok);
    assert_eq!(
        width(&out),
        12,
        "a panel round this JSON is 12 cells:\n{out}"
    );

    let code = fixture("fit.py", b"print(\"hi\")\nx = 1\n");
    let (out, ok) = run(
        &[
            "--no-color",
            "--panel",
            "rounded",
            "-x",
            code.to_str().unwrap(),
        ],
        "",
    );
    assert!(ok);
    assert_eq!(
        width(&out),
        15,
        "a panel round this source is 15 cells:\n{out}"
    );

    let csv = fixture("fit.csv", b"name;age;city\nAlice;30;Paris\nBob;25;Lyon\n");
    let (out, ok) = run(
        &[
            "--no-color",
            "--panel",
            "rounded",
            "--csv",
            csv.to_str().unwrap(),
        ],
        "",
    );
    assert!(ok);
    assert_eq!(
        width(&out),
        27,
        "a panel round this table is 27 cells:\n{out}"
    );

    let (out, ok) = run(&["--no-color", "--panel", "rounded", "--rule", "hello"], "");
    assert!(ok);
    assert_eq!(
        width(&out),
        5,
        "a Rule measures 1, so its panel is 5:\n{out}"
    );

    // Padding fits too: 2 above/below and 2 either side of a 5-cell word.
    let (out, ok) = run(&["--no-color", "--padding", "2", "-p", "hello"], "");
    assert!(ok);
    assert_eq!(width(&out), 9, "padding fits its content:\n{out}");

    // `-e` is how upstream asks for the old behaviour back.
    let (out, ok) = run(
        &[
            "--no-color",
            "--panel",
            "rounded",
            "--expand",
            "-p",
            "hello",
        ],
        "",
    );
    assert!(ok);
    assert_eq!(width(&out), 80, "--expand should fill the console:\n{out}");
}

/// Three ways `--title`/`--caption`/`--style` diverged from upstream. (#83)
#[test]
fn title_caption_and_style_are_not_hostage_to_the_panel() {
    // `--title`/`--caption` are the CSV table's title and caption upstream, so
    // demanding `--panel` made those unreachable.
    let csv = fixture("titled.csv", b"name,age\nAlice,30\nBob,25\n");
    let (out, ok) = run(
        &[
            "--csv",
            csv.to_str().unwrap(),
            "--no-color",
            "--title",
            "Sales report",
            "--caption",
            "from sales.csv",
        ],
        "",
    );
    assert!(ok, "--title without --panel must not abort:\n{out}");
    assert!(out.contains("Sales report"), "title missing from:\n{out}");
    assert!(
        out.contains("from sales.csv"),
        "caption missing from:\n{out}"
    );

    // `--style` is the renderable's style (upstream's `-s`), not the panel
    // border (upstream's `-S`), and works with no panel at all. Piped output
    // carries no ANSI, so the styling is read back out of an HTML export.
    let html = std::env::temp_dir().join("rich-uat8").join("style.html");
    let styled = |args: &[&str]| -> String {
        let mut full = args.to_vec();
        full.extend(["--export-html", html.to_str().unwrap()]);
        let (out, ok) = run(&full, "");
        assert!(ok, "{args:?} should succeed, got: {out}");
        std::fs::read_to_string(&html).unwrap()
    };
    assert!(
        styled(&["-s", "bold", "-p", "hello"]).contains("font-weight: bold"),
        "--style did not reach the renderable"
    );
    assert!(
        !styled(&["-p", "hello"]).contains("font-weight: bold"),
        "sanity: unstyled output must not be bold either"
    );
    // The border, meanwhile, answers to -S.
    assert!(
        styled(&["-S", "bold", "--panel", "square", "-p", "hi"]).contains("font-weight: bold"),
        "-S did not reach the panel border"
    );

    // `--panel none` is upstream's default and means NO panel; it used to draw
    // an invisible box.NONE frame, padding the output out of alignment.
    let (out, ok) = run(&["--no-color", "--panel", "none", "-p", "hello"], "");
    assert!(ok);
    assert_eq!(out, "hello\n", "--panel none drew a frame:\n{out:?}");
}

/// `--width N` shrank the CONSOLE, so `--center`/`--right` aligned inside N
/// columns instead of inside the terminal, and an `--export-svg` frame came out
/// N wide. Upstream wraps the renderable in a fixed-width container and leaves
/// the console alone. (#84)
#[test]
fn width_bounds_the_renderable_not_the_console() {
    let (out, ok) = run(&["--no-color", "-w", "20", "--right", "-p", "hello"], "");
    assert!(ok);
    let line = out.trim_end_matches('\n');
    assert_eq!(
        line.chars().count(),
        80,
        "the console is still 80:\n{out:?}"
    );
    assert_eq!(
        line.find("hello"),
        Some(75),
        "`hello` should sit at the terminal's right edge:\n{out:?}"
    );

    // A panel under `-w` fills exactly N and is aligned as one block.
    let (out, ok) = run(
        &[
            "--no-color",
            "-w",
            "20",
            "--right",
            "--panel",
            "rounded",
            "-p",
            "hello",
        ],
        "",
    );
    assert!(ok);
    for line in out.lines() {
        assert_eq!(line.chars().count(), 80, "ragged alignment:\n{out}");
        assert_eq!(
            line.chars().take_while(|c| *c == ' ').count(),
            60,
            "the 20-cell panel is not flush right:\n{out}"
        );
    }

    // The SVG frame follows the console, so `--width` must not change it.
    let frame = |args: &[&str]| -> String {
        let svg = std::env::temp_dir().join("rich-uat8").join("w.svg");
        let mut full = args.to_vec();
        full.extend(["--export-svg", svg.to_str().unwrap()]);
        let (_out, ok) = run(&full, "");
        assert!(ok);
        let written = std::fs::read_to_string(&svg).unwrap();
        let at = written
            .find("viewBox=")
            .expect("an SVG frame has a viewBox");
        written[at..].split('"').nth(1).unwrap().to_string()
    };
    assert_eq!(
        frame(&["-w", "20", "-p", "hello"]),
        frame(&["-p", "hello"]),
        "`--width` narrowed the SVG frame, so it narrowed the console"
    );
}

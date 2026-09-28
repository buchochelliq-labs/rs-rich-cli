//! File names that are not valid Unicode, on Unix: batch globbing, directory
//! walks, planned outputs and workers keep their bytes, so every file is
//! rendered and exported under its own name (0.0.13 workstream 6).
#![cfg(unix)]

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::process::Command;

fn rich(args: &[&OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--no-color"])
        .args(args)
        .output()
        .unwrap()
}

/// `input/` with `caf\xe9.txt` (Latin-1, not UTF-8), `na\xffme.txt` and
/// `plain.txt`; a file system that refuses non-UTF-8 names skips the test.
fn fixture() -> Option<tempfile::TempDir> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input");
    std::fs::create_dir(&input).unwrap();
    for (name, body) in [
        (&b"caf\xe9.txt"[..], "latin one"),
        (b"na\xffme.txt", "invalid byte"),
        (b"plain.txt", "plain"),
    ] {
        if std::fs::write(input.join(OsStr::from_bytes(name)), body).is_err() {
            return None;
        }
    }
    Some(dir)
}

fn exported(out: &Path) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = std::fs::read_dir(out)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().as_bytes().to_vec())
        .collect();
    names.sort();
    names
}

#[test]
fn a_batch_glob_exports_each_file_under_its_own_bytes() {
    let Some(dir) = fixture() else { return };
    for jobs in ["1", "3"] {
        let out = dir.path().join(format!("out-{jobs}"));
        std::fs::create_dir(&out).unwrap();
        let glob = dir.path().join("input/*.txt");
        let result = rich(&[
            OsStr::new("--batch"),
            OsStr::new("--jobs"),
            OsStr::new(jobs),
            OsStr::new("--export-html"),
            out.as_os_str(),
            glob.as_os_str(),
        ]);
        assert!(
            result.status.success(),
            "jobs {jobs}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            exported(&out),
            // A directory target names each export `{stem}-{index}`.
            [
                b"caf\xe9-0".to_vec(),
                b"na\xffme-1".to_vec(),
                b"plain-2".to_vec()
            ],
            "jobs {jobs}"
        );
        let html = std::fs::read_to_string(out.join(OsStr::from_bytes(b"na\xffme-1"))).unwrap();
        assert!(
            html.contains("invalid byte"),
            "rendered its own file: {html}"
        );
    }
}

#[test]
fn a_batch_directory_walk_reads_every_file() {
    let Some(dir) = fixture() else { return };
    let out = dir.path().join("walked");
    std::fs::create_dir(&out).unwrap();
    let input = dir.path().join("input");
    let result = rich(&[
        OsStr::new("--batch"),
        OsStr::new("--export-html"),
        out.as_os_str(),
        input.as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(exported(&out).len(), 3);
}

#[test]
fn a_single_non_utf8_file_renders() {
    let Some(dir) = fixture() else { return };
    let file = dir.path().join(OsStr::from_bytes(b"input/caf\xe9.txt"));
    let result = rich(&[file.as_os_str()]);
    assert!(result.status.success());
    assert!(String::from_utf8_lossy(&result.stdout).contains("latin one"));
}

#[test]
fn an_escaped_spelling_never_names_a_real_file() {
    let Some(dir) = fixture() else { return };
    // A valid name spelled like the invalid one's escape.
    let input = dir.path().join("input");
    std::fs::write(input.join("na\\xFFme.txt"), "literal name").unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let result = rich(&[
        OsStr::new("--batch"),
        OsStr::new("--export-html"),
        out.as_os_str(),
        input.join("*.txt").as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let names = exported(&out);
    assert_eq!(names.len(), 4, "every input exported: {names:?}");
    let bodies: Vec<String> = names
        .iter()
        .map(|name| std::fs::read_to_string(out.join(OsStr::from_bytes(name))).unwrap())
        .collect();
    for body in ["latin one", "invalid byte", "literal name", "plain"] {
        assert_eq!(
            bodies.iter().filter(|html| html.contains(body)).count(),
            1,
            "{body:?} rendered once"
        );
    }
}

#[test]
fn a_glob_in_a_non_utf8_directory_is_scanned() {
    let Some(dir) = fixture() else { return };
    let input = dir.path().join(OsStr::from_bytes(b"in\xfe"));
    std::fs::rename(dir.path().join("input"), &input).unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let result = rich(&[
        OsStr::new("--batch"),
        OsStr::new("--export-html"),
        out.as_os_str(),
        input.join("*.txt").as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(exported(&out).len(), 3);
}

#[test]
fn a_dry_run_checks_a_non_utf8_output_directory() {
    let Some(dir) = fixture() else { return };
    let out = dir.path().join(OsStr::from_bytes(b"out\xfd"));
    std::fs::create_dir(&out).unwrap();
    let result = rich(&[
        OsStr::new("--batch"),
        OsStr::new("--dry-run"),
        OsStr::new("--export-html"),
        out.as_os_str(),
        dir.path().join("input/*.txt").as_os_str(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(exported(&out).is_empty(), "a dry run writes nothing");
}

fn rich_in(dir: &Path, args: &[&OsStr]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .current_dir(dir)
        .args(["--no-config", "--no-color"])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn an_export_never_overwrites_an_input_of_the_same_lossy_spelling() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = OsStr::from_bytes(b"\xff.txt");
    // U+FFFD, the lossy spelling of the byte `\xff`, as valid UTF-8.
    let valid = OsStr::from_bytes(b"\xef\xbf\xbd.txt");
    if std::fs::write(dir.path().join(invalid), "the input").is_err() {
        return;
    }
    std::fs::write(dir.path().join(valid), "the old export").unwrap();
    let result = rich_in(dir.path(), &[invalid, OsStr::new("--export-html"), valid]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join(invalid)).unwrap(),
        "the input",
        "the input was overwritten"
    );
    let export = std::fs::read_to_string(dir.path().join(valid)).unwrap();
    assert!(
        export.contains("<html") && export.contains("the input"),
        "{export}"
    );
}

#[test]
fn text_arguments_that_are_not_utf8_show_lossily() {
    let dir = tempfile::tempdir().unwrap();
    let result = rich_in(
        dir.path(),
        &[
            OsStr::new("-p"),
            OsStr::from_bytes(b"a\xffb"),
            OsStr::new("--title"),
            OsStr::from_bytes(b"t\xfe"),
            OsStr::new("-a"),
            OsStr::new("square"),
        ],
    );
    assert!(result.status.success());
    let out = String::from_utf8(result.stdout).unwrap();
    assert!(out.contains("a\u{fffd}b"), "{out:?}");
    assert!(out.contains("t\u{fffd}"), "{out:?}");
    assert!(!out.contains('\0'), "{out:?}");
}

#[test]
fn a_non_utf8_glob_matches_bytes_and_a_dry_run_shows_names_safely() {
    let dir = tempfile::tempdir().unwrap();
    for name in [&b"a\xfe.txt"[..], b"a\xff.txt", b"e\x1b[31m.txt"] {
        if std::fs::write(dir.path().join(OsStr::from_bytes(name)), "x").is_err() {
            return;
        }
    }
    let result = rich_in(
        dir.path(),
        &[
            OsStr::new("--batch"),
            OsStr::new("--dry-run"),
            OsStr::from_bytes(b"a\xff*"),
        ],
    );
    let out = String::from_utf8_lossy(&result.stdout).into_owned();
    assert!(result.status.success(), "{out}");
    assert!(out.contains("a\\xFF.txt"), "{out:?}");
    assert!(
        !out.contains("a\\xFE"),
        "the lossy spelling matched: {out:?}"
    );
    assert!(!out.contains('\0'), "the escape marker printed: {out:?}");
    let result = rich_in(
        dir.path(),
        &[
            OsStr::new("--batch"),
            OsStr::new("--dry-run"),
            OsStr::new("e*"),
        ],
    );
    let out = String::from_utf8_lossy(&result.stdout).into_owned();
    assert!(result.status.success(), "{out}");
    assert!(
        !out.contains('\x1b') && out.contains("e␛[31m.txt"),
        "{out:?}"
    );
}

#[test]
fn a_non_utf8_theme_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let theme = OsStr::from_bytes(b"th\xff.ini");
    if std::fs::write(dir.path().join(theme), "[styles]\nwarning = red\n").is_err() {
        return;
    }
    let result = rich_in(
        dir.path(),
        &[
            OsStr::new("-p"),
            OsStr::new("[warning]w[/]"),
            OsStr::new("--theme-file"),
            theme,
        ],
    );
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

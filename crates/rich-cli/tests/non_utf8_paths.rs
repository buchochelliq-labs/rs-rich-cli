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

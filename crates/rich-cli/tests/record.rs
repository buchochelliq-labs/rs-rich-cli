//! `rich record`: a tape's name cannot send its output, or the pruning of
//! stale screenshots, outside `--output`.
#![cfg(all(unix, feature = "record"))]

use std::path::{Path, PathBuf};
use std::process::Command;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-cli-record-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn record(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .arg("record")
        .args(args)
        .current_dir(dir)
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

#[test]
fn a_tape_named_dot_dot_is_refused_before_anything_runs() {
    let dir = scratch("dotdot");
    let tapes = dir.join("tapes");
    let output = dir.join("out");
    std::fs::create_dir_all(&tapes).unwrap();
    std::fs::create_dir_all(&output).unwrap();
    // `...tape` has the stem `..`: its recording directory would be
    // `out/..`, and pruning would reach the files beside `out`.
    std::fs::write(tapes.join("...tape"), "Exec \"touch ran\"\nScreenshot a\n").unwrap();
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    for tape in ["tapes/...tape", "tapes/..tape"] {
        let result = record(&dir, &["--output", "out", "--no-video", tape]);
        assert_eq!(result.status.code(), Some(3), "{tape}: {result:?}");
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(
            stderr.contains("cannot name a recording directory"),
            "{stderr}"
        );
    }
    assert!(dir.join("notes.txt").exists());
    assert!(!dir.join("ran").exists());
    assert_eq!(std::fs::read_dir(&output).unwrap().count(), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_bad_name_among_good_ones_stops_them_all() {
    let dir = scratch("mixed");
    std::fs::write(dir.join("good.tape"), "Screenshot a\n").unwrap();
    std::fs::write(dir.join("...tape"), "Screenshot a\n").unwrap();
    let result = record(&dir, &["--output", "out", "good.tape", "...tape"]);
    assert_eq!(result.status.code(), Some(3), "{result:?}");
    assert!(!dir.join("out").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

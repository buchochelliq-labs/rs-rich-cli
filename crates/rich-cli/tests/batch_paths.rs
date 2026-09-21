use std::{path::Path, process::Command};
fn run(root: &Path, out: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--no-config",
            "--no-color",
            "--batch",
            "--batch-preserve-dirs",
            "--batch-input-root",
            root.to_str().unwrap(),
            "--batch-name-template",
            "{index}-{stem}.{output_ext}",
            "--export-html",
            out.to_str().unwrap(),
        ])
        .args(extra)
        .arg(root)
        .output()
        .unwrap()
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for sub in ["a", "b"] {
        std::fs::create_dir_all(t.path().join("input").join(sub)).unwrap();
        std::fs::write(t.path().join("input").join(sub).join("report.txt"), "hello").unwrap();
    }
    t
}
#[test]
fn dry_run_plans_missing_directories_and_execution_matches_across_workers() {
    let t = fixture();
    let root = t.path().join("input");
    let out = t.path().join("preview");
    let preview = run(&root, &out, &["--dry-run", "--report", "json"]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(!out.exists());
    let report: serde_json::Value = serde_json::from_slice(&preview.stderr).unwrap();
    let planned = &report["result"]["resources"];
    assert_eq!(
        Path::new(planned[0]["html"].as_str().unwrap()),
        out.join("a/1-report.html")
    );
    assert_eq!(
        Path::new(planned[1]["html"].as_str().unwrap()),
        out.join("b/2-report.html")
    );
    assert!(report["result"]["directories"].as_array().unwrap().len() >= 3);
    let serial = t.path().join("serial");
    let parallel = t.path().join("parallel");
    let serial_result = run(&root, &serial, &["--jobs", "1"]);
    assert!(serial_result.status.success(), "{serial_result:?}");
    let p = run(&root, &parallel, &["--jobs", "4"]);
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    for path in ["a/1-report.html", "b/2-report.html"] {
        assert_eq!(
            std::fs::read(serial.join(path)).unwrap(),
            std::fs::read(parallel.join(path)).unwrap()
        );
    }
}
#[cfg(unix)]
#[test]
fn symlink_escape_and_input_alias_are_rejected_without_writes() {
    use std::os::unix::fs::symlink;
    let t = fixture();
    let root = t.path().join("input");
    let out = t.path().join("out");
    std::fs::create_dir(&out).unwrap();
    let escape = t.path().join("escape");
    std::fs::create_dir(&escape).unwrap();
    symlink(&escape, out.join("a")).unwrap();
    let result = run(&root, &out, &["--dry-run"]);
    assert!(!result.status.success());
    assert_eq!(std::fs::read_dir(&escape).unwrap().count(), 0);
    std::fs::remove_file(out.join("a")).unwrap();
    std::fs::create_dir(out.join("a")).unwrap();
    std::fs::hard_link(root.join("a/report.txt"), out.join("a/1-report.html")).unwrap();
    let result = run(&root, &out, &["--overwrite"]);
    assert!(!result.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("a/report.txt")).unwrap(),
        "hello"
    );
}

#[test]
fn suffix_collisions_and_invalid_templates_are_settled_before_workers() {
    let t = fixture();
    let root = t.path().join("input");
    let out = t.path().join("out");
    let failed = run(&root, &out, &["--batch-name-template", "../bad"]);
    assert!(!failed.status.success());
    assert!(!out.exists());
    let ok = run(
        &root,
        &out,
        &[
            "--no-batch-preserve-dirs",
            "--batch-name-template",
            "same.{output_ext}",
            "--collision",
            "suffix",
            "--dry-run",
        ],
    );
    // Template-only mode retains the legacy requirement for existing parents.
    assert!(!ok.status.success());
    std::fs::create_dir(&out).unwrap();
    let ok = run(
        &root,
        &out,
        &[
            "--no-batch-preserve-dirs",
            "--batch-name-template",
            "same.{output_ext}",
            "--collision",
            "suffix",
            "--jobs",
            "4",
        ],
    );
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    assert!(out.join("same.html").exists());
    assert!(out.join("same-2.html").exists());
}

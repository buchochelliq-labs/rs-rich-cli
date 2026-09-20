//! Batch 0.0.8 contracts, tested through worker subprocesses.
use std::process::{Command, Output};

fn run(root: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--no-color", "--width", "40"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
}

fn report(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stderr).unwrap_or_else(|_| panic!("{output:?}"))
}

#[test]
fn dry_run_plans_without_creating_or_truncating_outputs() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
    std::fs::write(root.path().join("keep.html"), "keep").unwrap();
    let result = run(
        root.path(),
        &[
            "--batch",
            "--dry-run",
            "--report",
            "json",
            "--overwrite",
            "--export-html",
            "keep.html",
            "--export-svg",
            "absent/nested/a.svg",
            "a.txt",
        ],
    );
    assert_eq!(result.status.code(), Some(3), "{result:?}");
    assert!(result.stdout.is_empty());
    let json = report(&result);
    assert_eq!(json["result"]["dry_run"], true);
    assert_eq!(json["result"]["attempted"], 0);
    assert_eq!(json["result"]["resources"][0]["svg"], "absent/nested/a.svg");
    assert_eq!(
        std::fs::read_to_string(root.path().join("keep.html")).unwrap(),
        "keep"
    );
    assert!(!root.path().join("absent").exists());
}

#[test]
fn parallel_exports_equal_serial_and_terminal_output_stays_ordered() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "FIRST\n").unwrap();
    std::fs::write(root.path().join("b.txt"), "SECOND\n").unwrap();
    std::fs::create_dir(root.path().join("serial")).unwrap();
    std::fs::create_dir(root.path().join("parallel")).unwrap();
    let serial = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "1",
            "--report",
            "json",
            "--export-html",
            "serial/out.html",
            "b.txt",
            "a.txt",
        ],
    );
    let parallel = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "2",
            "--report",
            "json",
            "--export-html",
            "parallel/out.html",
            "b.txt",
            "a.txt",
        ],
    );
    assert!(serial.status.success(), "{serial:?}");
    assert!(parallel.status.success(), "{parallel:?}");
    assert_eq!(serial.stdout, parallel.stdout);
    let text = String::from_utf8(parallel.stdout.clone()).unwrap();
    assert!(text.find("FIRST").unwrap() < text.find("SECOND").unwrap());
    assert_eq!(report(&parallel)["result"]["completed"], 2);
    for name in ["a.html", "b.html"] {
        assert_eq!(
            std::fs::read(root.path().join("serial").join(name)).unwrap(),
            std::fs::read(root.path().join("parallel").join(name)).unwrap()
        );
    }
}

#[test]
fn parallel_overwrite_collisions_fail_before_writing() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("nested")).unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
    std::fs::write(root.path().join("nested/a.txt"), "other alpha").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "2",
            "--collision",
            "overwrite",
            "--export-html",
            "out.html",
            "--report",
            "json",
            "a.txt",
            "nested/a.txt",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(report(&output)["code"], "usage");
    assert!(!root.path().join("a.html").exists());
}

#[test]
fn parallel_invalid_json_preserves_data_class_and_single_report() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.json"), "{invalid}").unwrap();
    std::fs::write(root.path().join("b.json"), "{\"valid\":true}").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "2",
            "--continue-on-error",
            "--json",
            "--export-html",
            "out.html",
            "--report",
            "json",
            "a.json",
            "b.json",
        ],
    );
    assert_eq!(output.status.code(), Some(4));
    let json = report(&output);
    assert_eq!(json["code"], "data");
    assert_eq!(json["result"]["failed"], 1);
    assert_eq!(json["result"]["completed"], 1);
    assert_eq!(json["result"]["failures"][0]["resource"], "a.json");
    assert_eq!(json["result"]["failures"][0]["code"], "data");
}

#[test]
fn dry_run_reports_collision_with_full_resource_plan() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
    std::fs::write(root.path().join("a.html"), "original").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--dry-run",
            "--export-html",
            "a.html",
            "--report",
            "json",
            "a.txt",
        ],
    );
    assert_eq!(output.status.code(), Some(3));
    let json = report(&output);
    assert_eq!(json["result"]["resources"][0]["html"], "a.html");
    assert_eq!(json["result"]["errors"][0]["code"], "input");
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.html")).unwrap(),
        "original"
    );
}

#[test]
fn fail_fast_finishes_active_workers_and_skips_unscheduled_inputs() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.json"), "{invalid}").unwrap();
    std::fs::write(root.path().join("b.json"), "{\"second\":true}").unwrap();
    std::fs::write(root.path().join("c.json"), "{\"third\":true}").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "2",
            "--json",
            "--export-html",
            "out.html",
            "--report",
            "json",
            "a.json",
            "b.json",
            "c.json",
        ],
    );
    assert_eq!(output.status.code(), Some(4));
    let json = report(&output);
    assert_eq!(json["result"]["attempted"], 2);
    assert_eq!(json["result"]["skipped"], 1);
    assert!(root.path().join("b.html").exists());
    assert!(!root.path().join("c.html").exists());
}

#[test]
fn export_alias_collision_is_rejected_even_serially() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--export-html",
            "out",
            "--export-svg",
            "./out",
            "--report",
            "json",
            "a.txt",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert!(!root.path().join("out").exists());
}

#[test]
fn batch_never_overwrites_another_planned_input() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
    std::fs::write(root.path().join("a.html"), "source html").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--jobs",
            "2",
            "--collision",
            "overwrite",
            "--export-html",
            "out.html",
            "--report",
            "json",
            "a.txt",
            "a.html",
        ],
    );
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        std::fs::read_to_string(root.path().join("a.html")).unwrap(),
        "source html"
    );
}

#[test]
fn successful_dry_run_reports_sorted_matches_and_does_not_render() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("b.json"), "not valid JSON").unwrap();
    std::fs::write(root.path().join("a.json"), "not valid JSON either").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--dry-run",
            "--jobs",
            "2",
            "--json",
            "--export-html",
            "out.html",
            "--report",
            "json",
            "*.json",
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let json = report(&output);
    assert_eq!(json["result"]["planned"], 2);
    for (index, name) in ["a.json", "b.json"].iter().enumerate() {
        let resource = json["result"]["resources"][index]["resource"]
            .as_str()
            .unwrap();
        assert_eq!(
            std::path::Path::new(resource),
            std::path::Path::new(".").join(name)
        );
    }
    assert!(output.stdout.is_empty());
    assert!(!root.path().join("a.html").exists());
    assert!(!root.path().join("b.html").exists());
}

#[cfg(unix)]
#[test]
fn symlink_parent_traversal_cannot_overwrite_a_batch_input() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("actual/sub")).unwrap();
    std::os::unix::fs::symlink("actual/sub", root.path().join("link")).unwrap();
    std::fs::write(root.path().join("actual/input.txt"), "protected source").unwrap();
    let output = run(
        root.path(),
        &[
            "--batch",
            "--overwrite",
            "--export-html",
            "link/../input.txt",
            "--report",
            "json",
            "actual/input.txt",
        ],
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert_eq!(report(&output)["code"], "usage");
    assert_eq!(
        std::fs::read_to_string(root.path().join("actual/input.txt")).unwrap(),
        "protected source"
    );
}

#[test]
fn hard_linked_input_exports_are_rejected_before_writing() {
    for dry_run in [true, false] {
        for jobs in ["1", "2"] {
            let root = tempfile::tempdir().unwrap();
            let input = root.path().join("a.json");
            std::fs::write(&input, "{\"keep\":true}").unwrap();
            std::fs::hard_link(&input, root.path().join("out.html")).unwrap();
            let mut args = vec![
                "--batch",
                "--json",
                "a.json",
                "--export-html",
                "out.html",
                "--overwrite",
                "--jobs",
                jobs,
                "--report",
                "json",
            ];
            if dry_run {
                args.push("--dry-run");
            }
            let result = run(root.path(), &args);
            assert_eq!(result.status.code(), Some(2), "{result:?}");
            assert_eq!(std::fs::read_to_string(input).unwrap(), "{\"keep\":true}");
            assert!(result.stdout.is_empty());
        }
    }
}

#[test]
fn hard_linked_parallel_destinations_are_rejected_before_writing() {
    for dry_run in [true, false] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.txt"), "alpha").unwrap();
        std::fs::write(root.path().join("b.txt"), "beta").unwrap();
        std::fs::write(root.path().join("a.html"), "keep").unwrap();
        std::fs::hard_link(root.path().join("a.html"), root.path().join("b.html")).unwrap();
        let mut args = vec![
            "--batch",
            "a.txt",
            "b.txt",
            "--export-html",
            "out.html",
            "--overwrite",
            "--jobs",
            "2",
            "--report",
            "json",
        ];
        if dry_run {
            args.push("--dry-run");
        }
        let result = run(root.path(), &args);
        assert_eq!(result.status.code(), Some(2), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(root.path().join("a.html")).unwrap(),
            "keep"
        );
        assert!(result.stdout.is_empty());
    }
}

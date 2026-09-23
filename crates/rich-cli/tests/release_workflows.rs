//! Cross-feature release contracts exercised through the actual CLI binary.
use std::path::PathBuf;
use std::process::{Command, Output};

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rich-release-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .args(args)
            .current_dir(&self.0)
            .env("HOME", &self.0)
            .env("COLUMNS", "80")
            .env_remove("NO_COLOR")
            .output()
            .unwrap()
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn report(output: &Output) -> serde_json::Value {
    let stderr = std::str::from_utf8(&output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "stderr: {stderr}");
    serde_json::from_str(stderr).unwrap()
}

#[test]
fn redirected_watch_is_one_snapshot_without_viewport_controls() {
    let w = Workspace::new("watch-pipe");
    w.write("state.json", "{\"state\":\"snapshot\"}");
    let output = w.run(&[
        "--no-config",
        "--watch",
        "--watch-interval",
        "0.02",
        "--json",
        "--no-color",
        "state.json",
    ]);
    assert!(output.status.success(), "{output:?}");
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert_eq!(stdout.matches("snapshot").count(), 1);
    assert!(!stdout.contains('\x1b'));
    assert!(output.stderr.is_empty());
}

#[test]
fn batch_uses_profile_defaults_but_explicit_mode_and_collision_win() {
    let w = Workspace::new("config-batch");
    w.write("rich.toml", "[defaults]\nbatch = true\nmode = \"json\"\ncollision = \"error\"\n[profiles.docs]\ncollision = \"suffix\"\njobs = 1\n");
    w.write("a.md", "# First document");
    w.write("b.md", "# Second document");
    w.write("a.html", "keep me");
    let output = w.run(&[
        "--profile",
        "docs",
        "markdown",
        "--no-color",
        "--report",
        "json",
        "-o",
        "out.html",
        "a.md",
        "b.md",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(report(&output)["result"]["completed"], 2);
    assert_eq!(
        std::fs::read_to_string(w.0.join("a.html")).unwrap(),
        "keep me"
    );
    assert!(std::fs::read_to_string(w.0.join("a-2.html"))
        .unwrap()
        .contains("First document"));

    let output = w.run(&[
        "--profile",
        "docs",
        "markdown",
        "--collision",
        "overwrite",
        "--report",
        "json",
        "-o",
        "out.html",
        "a.md",
        "b.md",
    ]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(report(&output)["result"]["completed"], 2);
    assert!(std::fs::read_to_string(w.0.join("a.html"))
        .unwrap()
        .contains("First document"));
    assert!(!w.0.join("a-3.html").exists());
}

#[test]
fn batch_continues_after_export_and_data_failures_with_one_aggregate_report() {
    let w = Workspace::new("mixed-failures");
    w.write("a.json", "{\"export_fails\":true}");
    w.write("b.json", "{broken");
    w.write("c.json", "{\"survived\":true}");
    // A directory at one output path causes a real write failure even as root.
    std::fs::create_dir(w.0.join("a.html")).unwrap();
    let output = w.run(&[
        "--no-config",
        "--batch",
        "--json",
        "--continue-on-error",
        "--overwrite",
        "--report",
        "json",
        "-o",
        "out.html",
        "c.json",
        "b.json",
        "a.json",
    ]);
    let report = report(&output);
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(report["code"], "data");
    let result = &report["result"];
    assert_eq!(result["planned"], 3);
    assert_eq!(result["attempted"], 3);
    assert_eq!(result["completed"], 1);
    assert_eq!(result["failed"], 2);
    assert_eq!(result["skipped"], 0);
    assert_eq!(result["failures"][0]["resource"], "a.json");
    assert_eq!(result["failures"][0]["code"], "input");
    assert_eq!(result["failures"][0]["exit_code"], 3);
    assert_eq!(result["failures"][1]["resource"], "b.json");
    assert_eq!(result["failures"][1]["code"], "data");
    assert_eq!(result["failures"][1]["exit_code"], 4);
    assert!(std::fs::read_to_string(w.0.join("c.html"))
        .unwrap()
        .contains("survived"));
    assert!(!w.0.join("b.html").exists());
}

#[test]
fn batch_collision_preflight_prevents_partial_exports() {
    let w = Workspace::new("preflight");
    w.write("a.md", "# First");
    w.write("z.md", "# Last");
    w.write("z.html", "original");
    let output = w.run(&[
        "--no-config",
        "--batch",
        "--markdown",
        "--continue-on-error",
        "--report",
        "json",
        "-o",
        "out.html",
        "a.md",
        "z.md",
    ]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(report(&output)["code"], "input");
    assert!(output.stdout.is_empty());
    assert!(!w.0.join("a.html").exists());
    assert_eq!(
        std::fs::read_to_string(w.0.join("z.html")).unwrap(),
        "original"
    );
}

#[cfg(feature = "art")]
#[test]
fn redirected_image_auto_falls_back_even_when_sixel_is_forced() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../rich-art/tests/fixtures/halo-before.png");
    let output = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args([
            "--no-config",
            "image",
            fixture.to_str().unwrap(),
            "--image-mode",
            "auto",
            "--width",
            "12",
            "--height",
            "4",
            "--report",
            "json",
        ])
        .env("RICH_SIXEL", "1")
        .env_remove("NO_COLOR")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(report(&output)["ok"], true);
    let stdout = std::str::from_utf8(&output.stdout).unwrap();
    assert!(
        !stdout.contains('\x1b'),
        "redirected image contained terminal control bytes"
    );
    assert!(!stdout.trim().is_empty());
}

//! Multi-file `--watch` contracts that hold without a terminal. The
//! interactive behaviour (debounce, live regions, event backends) is covered
//! on a real PTY by `scripts/test_watch_workflows.py`.
use std::path::PathBuf;
use std::process::{Command, Output};

struct Workspace(PathBuf);

impl Workspace {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rich-watch-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, contents: &str) {
        std::fs::write(self.0.join(name), contents).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_rich"))
            .arg("--no-config")
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

#[test]
fn redirected_multi_file_watch_is_each_snapshot_in_order() {
    let w = Workspace::new("multi-pipe");
    w.write("a.json", "{\"a\":\"FIRST\"}");
    w.write("b.md", "# SECOND\n\nbody");
    let watched = w.run(&["--watch", "--watch-debounce", "0.5", "a.json", "b.md"]);
    assert!(watched.status.success(), "{watched:?}");
    assert!(watched.stderr.is_empty(), "{watched:?}");
    let mut expected = w.run(&["a.json"]).stdout;
    expected.extend(w.run(&["b.md"]).stdout);
    assert_eq!(watched.stdout, expected);
    let stdout = String::from_utf8(watched.stdout).unwrap();
    assert!(stdout.find("FIRST").unwrap() < stdout.find("SECOND").unwrap());
    assert!(!stdout.contains("\x1b[2J"));
}

#[test]
fn redirected_multi_file_watch_reports_the_first_failure() {
    let w = Workspace::new("multi-fail");
    w.write("good.json", "{\"ok\":true}");
    w.write("bad.json", "{ nope");
    let output = w.run(&["--watch", "bad.json", "good.json"]);
    assert!(!output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("\"ok\""), "the valid file still renders");
    assert!(stderr.contains("invalid JSON"), "{stderr}");
}

#[test]
fn several_resources_require_local_files_and_no_exports() {
    let w = Workspace::new("multi-usage");
    w.write("a.md", "a");
    w.write("b.md", "b");
    for (args, needle) in [
        (
            vec!["--watch", "a.md", "https://example.com/x.json"],
            "requires local files",
        ),
        (vec!["--watch", "a.md", "-"], "not stdin"),
        (
            vec!["--watch", "-o", "out.html", "a.md", "b.md"],
            "cannot be combined with watching several files",
        ),
        (vec!["a.md", "b.md"], "only one resource"),
        (vec!["--watch-poll", "a.md"], "requires --watch"),
    ] {
        let output = w.run(&args);
        assert!(!output.status.success(), "{args:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(needle), "{args:?}: {stderr}");
    }
}

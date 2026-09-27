//! End to end: a tape runs in a real PTY and every output is produced.
#![cfg(unix)]

use rich_record::record::{self, Formats, Options, Problem};
use rich_record::render::raster::Fonts;
use rich_record::tape;

const TAPE: &str = r#"
Set Size 40x8
Set Title "Test"
Write greeting.txt "café 👍\n"
Type "cat greeting.txt"
Enter
Wait "café"
Screenshot first
Exec "printf 'second\n' > more.txt"
Hide
Type "clear"
Enter
Show
Type "printf '\033[1;31mred\033[0m\n'; cat more.txt"
Enter
Wait /second\s*\n/
Screenshot second
Resize 30x6
Sleep 300ms
"#;

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rich-record-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn records_writes_and_checks() {
    let tape = tape::parse(TAPE).unwrap();
    let recording = record::record(&tape, "test", &Options::default()).unwrap();
    assert_eq!(recording.shots.len(), 2);
    let first = recording.shots[0].1.text_grid();
    assert!(first.contains("café 👍"), "{first}");
    let second = &recording.shots[1].1;
    assert!(
        second.text_grid().contains("red\nsecond"),
        "{}",
        second.text_grid()
    );
    // `red` was bold red: bright red in the theme.
    let output = second
        .rows
        .iter()
        .find(|row| {
            row.iter()
                .map(|c| c.text.as_str())
                .collect::<String>()
                .starts_with("red")
        })
        .unwrap();
    assert!(output[0].bold);
    assert_eq!(output[0].fg, rich_record::Theme::default().ansi[9]);
    // The resize is in the timeline.
    assert!(recording.timeline.events.iter().any(|(_, e)| *e
        == rich_record::session::Event::Resize {
            columns: 30,
            rows: 6
        }));

    let dir = scratch("write");
    let fonts = Fonts::embedded();
    let formats = Formats {
        gif: true,
        ..Formats::NO_VIDEO
    };
    let written = record::write(
        &recording,
        &dir,
        "test",
        formats,
        &fonts,
        &Default::default(),
        None,
    )
    .unwrap();
    for name in [
        "first.txt",
        "first.png",
        "first.svg",
        "second.png",
        "test.cast",
        "test.gif",
    ] {
        assert!(
            written.iter().any(|p| p.ends_with(name)),
            "{name} missing from {written:?}"
        );
    }
    assert!(record::check(&recording, &dir).is_empty());

    // A stale grid and an orphaned screenshot are both reported.
    std::fs::write(dir.join("first.txt"), "something else\n").unwrap();
    std::fs::write(dir.join("gone.txt"), "old\n").unwrap();
    let problems = record::check(&recording, &dir);
    assert!(problems
        .iter()
        .any(|p| matches!(p, Problem::Differs { name, .. } if name == "first")));
    assert!(problems
        .iter()
        .any(|p| matches!(p, Problem::Orphaned { name } if name == "gone")));
    // Writing again removes the orphan.
    record::write(
        &recording,
        &dir,
        "test",
        Formats::NO_VIDEO,
        &fonts,
        &Default::default(),
        None,
    )
    .unwrap();
    assert!(!dir.join("gone.txt").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_relative_bin_dir_resolves_against_the_current_directory() {
    // The shell runs in a temporary workspace: `bin` must still be found.
    let base = scratch("bin");
    std::fs::create_dir_all(base.join("bin")).unwrap();
    let tool = base.join("bin/tape-tool");
    std::fs::write(&tool, "#!/bin/sh\necho tool-ran\n").unwrap();
    let mut permissions = std::fs::metadata(&tool).unwrap().permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
    std::fs::set_permissions(&tool, permissions).unwrap();
    let relative = pathdiff(&base.join("bin"));
    let options = Options {
        bin_dir: Some(relative),
        ..Options::default()
    };
    let tape =
        tape::parse("Type \"tape-tool\"\nEnter\nWait \"tool-ran\" 5s\nScreenshot x\n").unwrap();
    record::record(&tape, "bin", &options).unwrap();
    let _ = std::fs::remove_dir_all(&base);
}

/// `path` relative to the current directory, climbing with `..` as needed.
fn pathdiff(path: &std::path::Path) -> std::path::PathBuf {
    let cwd = std::env::current_dir().unwrap();
    let mut up = std::path::PathBuf::new();
    let mut base = cwd.as_path();
    loop {
        if let Ok(rest) = path.strip_prefix(base) {
            return up.join(rest);
        }
        up.push("..");
        base = base.parent().expect("a common ancestor");
    }
}

#[test]
fn a_wait_that_never_matches_names_its_line() {
    let tape =
        tape::parse("Type \"echo hi\"\nEnter\nWait \"never\" 500ms\nScreenshot x\n").unwrap();
    let error = record::record(&tape, "timeout", &Options::default()).unwrap_err();
    assert_eq!(error.line, 3);
    assert!(
        error.message.contains("timed out waiting for \"never\""),
        "{error}"
    );
}

#[test]
fn every_installed_shell_records_with_the_same_prompt() {
    for name in ["bash", "zsh", "fish", "sh"] {
        let installed = std::process::Command::new(name)
            .args(["-c", "exit 0"])
            .status()
            .is_ok_and(|s| s.success());
        if !installed {
            eprintln!("skipping {name}: not installed");
            continue;
        }
        let source = format!(
            "Set Shell {name}\nSet Size 40x5\nType \"echo one; echo two\"\nEnter\n\
             Wait /two\\s*\\n/\nScreenshot shot\n"
        );
        let tape = tape::parse(&source).unwrap();
        let recording = record::record(&tape, name, &Options::default())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let grid = recording.shots[0].1.text_grid();
        assert!(
            grid.starts_with("❯ echo one; echo two\none\ntwo\n❯"),
            "{name}:\n{grid}"
        );
    }
}

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

    // A stale grid and an orphaned screenshot are both reported. A
    // screenshot is orphaned only when an earlier write listed it in
    // provenance.json: other files in the directory are never touched.
    let tape_path = std::path::Path::new("test.tape");
    record::write(
        &recording,
        &dir,
        "test",
        Formats::NO_VIDEO,
        &fonts,
        &Default::default(),
        Some((tape_path, TAPE.as_bytes())),
    )
    .unwrap();
    let provenance = std::fs::read_to_string(dir.join("provenance.json")).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&provenance).unwrap();
    assert_eq!(json["screenshots"], serde_json::json!(["first", "second"]));
    // A name that would leave the directory is ignored.
    let escape = format!("rich-record-escape-{}", std::process::id());
    let outside = dir.parent().unwrap().join(format!("{escape}.txt"));
    json["screenshots"] = serde_json::json!(["first", "second", "gone", format!("../{escape}")]);
    std::fs::write(dir.join("provenance.json"), json.to_string()).unwrap();
    std::fs::write(dir.join("first.txt"), "something else\n").unwrap();
    std::fs::write(dir.join("gone.txt"), "old\n").unwrap();
    std::fs::write(dir.join("gone.png"), "old").unwrap();
    std::fs::write(dir.join("notes.txt"), "mine\n").unwrap();
    std::fs::write(&outside, "outside\n").unwrap();
    let problems = record::check(&recording, &dir);
    assert!(problems
        .iter()
        .any(|p| matches!(p, Problem::Differs { name, .. } if name == "first")));
    let orphaned: Vec<_> = problems
        .iter()
        .filter_map(|p| match p {
            Problem::Orphaned { name } => Some(name.as_str()),
            Problem::Differs { .. } => None,
        })
        .collect();
    assert_eq!(orphaned, ["gone"]);
    // Writing again removes the orphan, and only it.
    record::write(
        &recording,
        &dir,
        "test",
        Formats::NO_VIDEO,
        &fonts,
        &Default::default(),
        Some((tape_path, TAPE.as_bytes())),
    )
    .unwrap();
    assert!(!dir.join("gone.txt").exists());
    assert!(!dir.join("gone.png").exists());
    assert!(dir.join("notes.txt").exists());
    assert!(outside.exists());
    let _ = std::fs::remove_file(&outside);
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

/// Every `Set Shell` records with the same prompt. A shell that is not
/// installed is skipped, unless `RICH_RECORD_REQUIRE_SHELLS` names it: CI's
/// tapes job installs all four and requires them.
#[test]
fn every_shell_records_with_the_same_prompt() {
    let required = std::env::var("RICH_RECORD_REQUIRE_SHELLS").unwrap_or_default();
    for name in ["bash", "zsh", "fish", "sh"] {
        let installed = std::process::Command::new(name)
            .args(["-c", "exit 0"])
            .status()
            .is_ok_and(|s| s.success());
        if !installed {
            assert!(
                !required.split(',').any(|shell| shell.trim() == name),
                "{name} is required by RICH_RECORD_REQUIRE_SHELLS but not installed"
            );
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

/// A program that asks the device attributes over and over and never reads
/// its input: the answers fill its input queue, but its output is still
/// read and typing still returns.
#[test]
fn a_program_that_asks_and_never_reads_blocks_nothing() {
    use rich_record::session::Session;
    use std::time::{Duration, Instant};
    let dir = scratch("queries");
    std::fs::create_dir_all(&dir).unwrap();
    // It gets its first answer; then it asks 100,000 times, reading none.
    let script = "stty raw -echo; printf '\\033[c'; dd bs=6 count=1 2>/dev/null | od -An -c; \
                  yes \"$(printf '\\033[c')\" | head -n 100000; echo DONE-QUERIES; exec sleep 30";
    let command = ["sh".to_string(), "-c".to_string(), script.to_string()];
    let env = [
        ("PATH".to_string(), std::env::var("PATH").unwrap()),
        ("TERM".to_string(), "xterm-256color".to_string()),
    ];
    let mut session = Session::start(&command, &dir, 80, 24, &env, Default::default()).unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while !session.seen(|screen| screen.contains("DONE-QUERIES")) {
        assert!(Instant::now() < end, "its output stopped being read");
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(session.seen(|screen| screen.contains("?   6   2   c")));
    let (sent, done) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sent.send(session.send("x", None).is_ok());
        session
    });
    assert_eq!(done.recv_timeout(Duration::from_secs(5)), Ok(true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_flood_of_output_is_recorded_in_bounded_frames() {
    let tape = tape::parse(
        "Set Size 40x8\nSet TypingDelay 1ms\nType \"seq 1 300000; echo done\"\nEnter\n\
         Wait /\\ndone\\s*\\n/\nScreenshot end\n",
    )
    .unwrap();
    let recording = record::record(&tape, "flood", &Options::default()).unwrap();
    let timeline = &recording.timeline;
    let (first, last) = (timeline.frames[0].0, timeline.frames.last().unwrap().0);
    // At most one frame per 1/12 s (plus the ones input forces).
    let inputs = timeline
        .events
        .iter()
        .filter(|(_, e)| matches!(e, rich_record::session::Event::Input(_)))
        .count();
    let allowed = ((last - first) * rich_record::session::FRAME_RATE).ceil() as usize + inputs + 2;
    assert!(
        timeline.frames.len() <= allowed,
        "{} frames over {:.2}s",
        timeline.frames.len(),
        last - first
    );
    // 2 MB of numbers, but no one output event holds more than a screen's
    // repaint or a batch.
    let largest = timeline
        .events
        .iter()
        .filter_map(|(_, e)| match e {
            rich_record::session::Event::Output(text) => Some(text.len()),
            _ => None,
        })
        .max()
        .unwrap();
    assert!(largest < 64 * 1024, "{largest}");
    assert!(!timeline.truncated);
    assert!(recording.shots[0].1.text_grid().contains("done"));
}

#[test]
fn held_keys_repeat_and_their_releases_follow_the_programs_flags() {
    // `cat -v` shows what the keys send. Without the protocol a held key
    // repeats its bytes and its release sends nothing; with it on and the
    // program's flags pushed (3: releases too), arrows report repeats and
    // releases, and letters still type.
    let tape = tape::parse(
        "Set Size 60x8\nSet KeyRepeat 100ms 50ms\nType \"stty -icanon -echo; cat -v\"\nEnter\n\
         Sleep 300ms\nHold w 330ms\nType \".\"\nWait /wwww+\\./\nScreenshot legacy\n",
    )
    .unwrap();
    let recording = record::record(&tape, "held", &Options::default()).unwrap();
    let grid = recording.shots[0].1.text_grid();
    let ws = grid.matches('w').count();
    // The press, then a repeat every 50 ms after the first 100 ms.
    assert!((4..=7).contains(&ws), "{ws} in {grid}");

    let tape = tape::parse(
        "Set Size 60x8\nSet KittyKeyboard on\nSet KeyRepeat 100ms 100ms\n\
         Type \"printf '\\\\033[>3u'; stty -icanon -echo; cat -v\"\nEnter\nSleep 300ms\n\
         KeyDown Up\nSleep 150ms\nKeyUp Up\nType \"w\"\nEscape\n\
         Wait /3A.*w/\nScreenshot kitty\n",
    )
    .unwrap();
    let recording = record::record(&tape, "kitty-keys", &Options::default()).unwrap();
    let grid = recording.shots[0].1.text_grid();
    assert!(
        grid.contains("^[[A^[[1;1:2A^[[1;1:3Aw^[[27u^[[27;1:3u"),
        "{grid}"
    );

    let error = record::record(
        &tape::parse("KeyUp w\nScreenshot x\n").unwrap(),
        "not-held",
        &Options::default(),
    )
    .unwrap_err();
    assert!(
        error.message.contains("not held") && error.line == 1,
        "{error}"
    );
}

#[test]
fn every_settled_update_is_a_frame_at_the_tapes_rate() {
    // 50 updates a second for a second and a half, recorded at 35 frames a
    // second: about 35 frames a second are kept, each a different screen.
    let tape = tape::parse(
        "Set Size 30x4\nSet Framerate 35\nOutput apng gif\n\
         Type \"for i in $(seq 1 75); do printf '\\\\r%03d' $i; sleep 0.02; done; echo; echo done\"\n\
         Enter\nWait /done/ 10s\nScreenshot end\n",
    )
    .unwrap();
    let recording = record::record(&tape, "rate", &Options::default()).unwrap();
    assert_eq!(recording.frame_rate, 35.0);
    let frames = &recording.timeline.frames;
    let counting: Vec<f64> = frames
        .iter()
        .filter(|(_, f)| {
            let text = f.snapshot().text_grid();
            text.lines()
                .any(|l| l.trim().len() == 3 && l.trim().parse::<u32>().is_ok())
        })
        .map(|(t, _)| *t)
        .collect();
    let span = counting.last().unwrap() - counting.first().unwrap();
    let rate = (counting.len() - 1) as f64 / span;
    // At most the tape's rate, and well above the old 12 unless the
    // machine is too busy to draw (loose, for loaded CI machines).
    assert!(
        (8.0..=36.0).contains(&rate),
        "{} frames over {span:.2}s",
        counting.len()
    );
    let dir = scratch("rate");
    let written = record::write_selected(
        &recording,
        &dir,
        "rate",
        Formats::ALL,
        &Fonts::embedded(),
        &Default::default(),
        None,
    )
    .unwrap();
    let apng = std::fs::read(dir.join("rate.apng")).unwrap();
    let reader = png::Decoder::new(std::io::Cursor::new(&apng))
        .read_info()
        .unwrap();
    let frames = reader.info().animation_control.unwrap().num_frames;
    assert!(frames as usize >= counting.len() - 2, "{frames}");
    assert!(written.paths.iter().any(|p| p.ends_with("rate.gif")));
    let _ = std::fs::remove_dir_all(&dir);
}

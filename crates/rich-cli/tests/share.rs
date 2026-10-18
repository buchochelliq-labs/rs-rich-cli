//! Sharing output: `--export-png` (#723) and `--redact-preview` (#722), 0.0.19
//! workstream 19. Not upstream.
use std::io::Write;
use std::process::{Command, Output, Stdio};

fn run(dir: &std::path::Path, args: &[&str], stdin: &[u8]) -> Output {
    run_at(dir, 40, args, stdin)
}

fn run_at(dir: &std::path::Path, width: usize, args: &[&str], stdin: &[u8]) -> Output {
    let width = width.to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_rich"))
        .args(["--no-config", "--no-color", "--width", &width])
        .args(args)
        .current_dir(dir)
        .env_remove("NO_COLOR")
        .env("COLUMNS", &width)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
    }
    child.wait_with_output().unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The image's width, height and RGB pixels.
#[cfg(feature = "png")]
fn decode(path: &std::path::Path) -> (u32, u32, Vec<u8>) {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgb);
    pixels.truncate(info.buffer_size());
    (info.width, info.height, pixels)
}

#[cfg(feature = "png")]
fn pixel(image: &(u32, u32, Vec<u8>), x: u32, y: u32) -> (u8, u8, u8) {
    let i = ((y * image.0 + x) * 3) as usize;
    (image.2[i], image.2[i + 1], image.2[i + 2])
}

#[cfg(feature = "png")]
#[test]
fn export_png_writes_a_deterministic_image_beside_the_terminal_output() {
    let dir = tempfile::tempdir().unwrap();
    let print = |name: &str| {
        run(
            dir.path(),
            &[
                "--print",
                "[on #ff0000]    [/] hello",
                "--export-png",
                name,
                "--export-svg",
                "out.svg",
            ],
            b"",
        )
    };
    let out = print("out.png");
    assert!(out.status.success(), "{out:?}");
    // The terminal still gets the output.
    assert!(text(&out.stdout).contains("hello"));
    assert!(dir.path().join("out.svg").exists());
    let image = decode(&dir.path().join("out.png"));
    // 40 columns and one row at 28 px text, in a window frame: cells about
    // 16.9 px, 36 px lines, 34 px padding and a 62 px title bar.
    assert_eq!((image.0, image.1), (742, 166));
    // The window chrome in the corner; the red cells after the padding,
    // below the title bar; the theme's background beside them.
    assert_eq!(pixel(&image, 0, 0), (30, 30, 30));
    assert_eq!(pixel(&image, 40, 110), (255, 0, 0));
    assert_eq!(pixel(&image, 700, 110), (41, 41, 41));
    // The same run draws the same bytes.
    assert!(print("again.png").status.success());
    assert_eq!(
        std::fs::read(dir.path().join("out.png")).unwrap(),
        std::fs::read(dir.path().join("again.png")).unwrap()
    );
}

#[cfg(all(unix, feature = "png"))]
#[test]
fn capture_exports_a_redacted_png() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &[
            "capture",
            "--redact",
            "--export-png",
            "run.png",
            "--",
            "sh",
            "-c",
            "echo hi",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    let image = decode(&dir.path().join("run.png"));
    assert_eq!(image.0, 742);
    assert!(image.1 > 166, "a panel of three rows or more");
}

#[test]
fn export_png_is_refused_where_it_cannot_apply() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "# a\n").unwrap();
    let batch = run(
        dir.path(),
        &["--batch", "a.md", "--export-png", "out.png"],
        b"",
    );
    assert_eq!(batch.status.code(), Some(2), "{batch:?}");
    let message = text(&batch.stderr);
    if cfg!(feature = "png") {
        assert!(message.contains("--export-png cannot be combined with --batch"));
    } else {
        assert!(message.contains("png feature"));
    }
    let missing = run(dir.path(), &["a.md", "--export-png"], b"");
    assert_eq!(missing.status.code(), Some(2), "{missing:?}");
    assert!(dir.path().read_dir().unwrap().count() == 1);
}

#[cfg(unix)]
#[test]
fn capture_redact_preview_shows_matches_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("script.sh"),
        "echo user=ann; echo 'password=hunter2'\n",
    )
    .unwrap();
    let out = run_at(
        dir.path(),
        80,
        &[
            "capture",
            "--redact",
            "--redact-preview",
            "--cast",
            "run.cast",
            "--export-svg",
            "run.svg",
            "--",
            "sh",
            "script.sh",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("2 │ password=hunter2"), "{stdout}");
    assert!(
        stdout.contains("╰ 2:10 key-value hunter2 → *******"),
        "{stdout}"
    );
    assert!(
        stdout.contains("output: 1 match on 1 of 2 lines (key-value 1)"),
        "{stdout}"
    );
    assert!(stdout.contains("command: no matches in 1 line"), "{stdout}");
    let stderr = text(&out.stderr);
    assert!(
        stderr.contains("dry run, nothing was written (without it: run.cast, run.svg)"),
        "{stderr}"
    );
    assert!(!dir.path().join("run.cast").exists());
    assert!(!dir.path().join("run.svg").exists());
}

#[cfg(unix)]
#[test]
fn capture_redact_preview_reports_json() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &[
            "capture",
            "--redact-pattern",
            r"order (?P<secret>\d{4})",
            "--redact-preview",
            "--report",
            "json",
            "--",
            "echo",
            "order 1234",
        ],
        b"",
    );
    assert!(out.status.success(), "{out:?}");
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let root = &doc["redact_preview"];
    // The command line holds the order too.
    assert_eq!(root["matches"], 2, "{doc}");
    let output = &root["sources"][1];
    assert_eq!(output["source"], "output");
    let finding = &output["findings"][0];
    assert_eq!(finding["kind"], "pattern");
    assert_eq!(finding["text"], "1234");
    assert_eq!(finding["replacement"], "****");
    assert_eq!(
        (finding["line"].as_u64(), finding["column"].as_u64()),
        (Some(1), Some(7))
    );
    assert_eq!(root["written"], serde_json::json!([]));
}

#[test]
fn log_redact_preview_reads_the_log_once() {
    let dir = tempfile::tempdir().unwrap();
    let log = "ok\nAuthorization: Bearer abcdef1234567890\nfine\n";
    let out = run_at(
        dir.path(),
        80,
        &["log", "--redact", "--redact-preview", "-"],
        log.as_bytes(),
    );
    assert!(out.status.success(), "{out:?}");
    let stdout = text(&out.stdout);
    assert!(stdout.starts_with("log\n"), "{stdout}");
    assert!(
        stdout.contains("╰ 2:23 key-value abcdef1234567890 → ********"),
        "{stdout}"
    );
    assert!(
        stdout.contains("log: 1 match on 1 of 3 lines (key-value 1)"),
        "{stdout}"
    );
    // The records themselves are not shown.
    assert!(!stdout.contains("fine"), "{stdout}");
    assert!(text(&out.stderr).contains("dry run, nothing was written"));
}

#[test]
fn redact_preview_is_checked() {
    let dir = tempfile::tempdir().unwrap();
    let alone = run(dir.path(), &["log", "--redact-preview", "-"], b"x\n");
    assert_eq!(alone.status.code(), Some(2), "{alone:?}");
    assert!(text(&alone.stderr).contains("needs --redact or --redact-pattern"));
    let follow = run(
        dir.path(),
        &["log", "--redact", "--redact-preview", "--follow", "-"],
        b"x\n",
    );
    assert_eq!(follow.status.code(), Some(2), "{follow:?}");
    let elsewhere = run(dir.path(), &["--redact-preview", "--print", "x"], b"");
    assert_eq!(elsewhere.status.code(), Some(2), "{elsewhere:?}");
    assert!(text(&elsewhere.stderr).contains("--redact-preview"));
}

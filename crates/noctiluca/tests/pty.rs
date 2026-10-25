//! The `noctiluca` binary on a real pseudo-terminal, its output read
//! through rs-rich-record's VT emulator: it starts, reads a page, quits,
//! and leaves the terminal's modes as it found them. Its engine is the
//! saved snapshots (`NOCTILUCA_FAKE_ENGINE`, read only with the test-only
//! `fake-engine` feature, which this test requires), so no browser is needed;
//! what a page says reaches the terminal only as text.
#![cfg(unix)]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rich_record::terminal::Terminal;

const COLUMNS: u16 = 100;
const ROWS: u16 = 50;

struct Run {
    /// The run's own temporary directory (`TMPDIR`), where a browser's
    /// temporary profile goes.
    tmp: std::path::PathBuf,
    output: Arc<Mutex<Vec<u8>>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl Run {
    fn start(args: &[&str], term: &str) -> Run {
        Run::start_with(args, term, None)
    }

    /// With `chrome`, the real browser instead of the saved snapshots.
    fn start_with(args: &[&str], term: &str, chrome: Option<&std::ffi::OsStr>) -> Run {
        let pty = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLUMNS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_noctiluca"));
        command.args(args);
        command.env_clear();
        let home = std::env::temp_dir().join(format!("noctiluca-pty-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        command.env("HOME", &home);
        command.env("XDG_CONFIG_HOME", home.join("config"));
        command.env("XDG_DATA_HOME", home.join("data"));
        command.env("TERM", term);
        static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let tmp = home.join(format!("tmp-{n}"));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        command.env("TMPDIR", &tmp);
        match chrome {
            Some(chrome) => command.env("RICH_EMBED_CHROME", chrome),
            None => command.env(
                "NOCTILUCA_FAKE_ENGINE",
                Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
            ),
        }
        let child = pty.slave.spawn_command(command).unwrap();
        drop(pty.slave);
        let mut reader = pty.master.try_clone_reader().unwrap();
        let writer = pty.master.take_writer().unwrap();
        let output = Arc::new(Mutex::new(Vec::new()));
        let sink = output.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 65536];
            while let Ok(n) = reader.read(&mut buffer) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buffer[..n]);
            }
        });
        // The master must outlive the child.
        std::mem::forget(pty.master);
        Run {
            tmp,
            output,
            writer,
            child,
        }
    }

    /// The emulator's screen after everything written so far.
    fn terminal(&self) -> Terminal {
        let mut terminal = Terminal::new(ROWS, COLUMNS);
        terminal.process(&self.output.lock().unwrap()).unwrap();
        terminal
    }

    fn wait_for(&self, text: &str) {
        let start = Instant::now();
        while !self.terminal().contents().contains(text) {
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "{text:?} never showed:\n{}",
                self.terminal().contents()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn press(&mut self, bytes: &[u8]) {
        self.writer.write_all(bytes).unwrap();
        self.writer.flush().unwrap();
    }

    fn quit(mut self) -> (Vec<u8>, Terminal) {
        self.press(b"q");
        let start = Instant::now();
        while self.child.try_wait().unwrap().is_none() {
            assert!(start.elapsed() < Duration::from_secs(10), "it did not quit");
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
        let bytes = self.output.lock().unwrap().clone();
        let terminal = self.terminal();
        (bytes, terminal)
    }
}

#[test]
fn it_starts_reads_and_quits_leaving_the_terminal_as_it_was() {
    let mut run = Run::start(
        &["--reader", "https://fixture.example/reader.html"],
        "xterm-256color",
    );
    run.wait_for("Tiny drifters of the open sea");
    run.wait_for("reader");
    // Reader mode is one key away from the projection, and back.
    run.press(b"r");
    run.wait_for("projection");
    run.press(b"r");
    run.wait_for("• Dinoflagellates");
    let (bytes, terminal) = run.quit();
    let screen = terminal.screen();
    assert!(!screen.alternate_screen(), "still in the alternate screen");
    assert!(!screen.hide_cursor(), "the cursor is still hidden");
    assert_eq!(screen.mouse_protocol_mode(), vt100::MouseProtocolMode::None);
    // It did use the alternate screen while it ran.
    assert!(bytes.windows(8).any(|w| w == b"\x1b[?1049h"));
}

#[test]
fn escape_sequences_in_a_page_reach_the_terminal_only_as_text() {
    let run = Run::start(
        &["--reader", "https://fixture.example/inject.html"],
        "xterm-256color",
    );
    run.wait_for("Head[31mred[0m");
    run.wait_for("Markup [bold red]not bold[/]");
    let (bytes, terminal) = run.quit();
    let text = String::from_utf8_lossy(&bytes);
    for bad in [
        "\x1b[31mred",
        "\x1b]0;owned",
        "\x1b]8;;http://evil",
        "\x1b[5m text",
        "\u{9b}",
        "\u{202e}",
        "\u{2066}",
        "\x7f",
    ] {
        assert!(!text.contains(bad), "{bad:?} reached the terminal");
    }
    let _ = terminal;
}

#[test]
fn an_image_is_a_media_region_in_kittys_graphics() {
    let run = Run::start(
        &[
            "--reader",
            "--images",
            "kitty",
            "https://fixture.example/reader.html",
        ],
        "xterm-kitty",
    );
    run.wait_for("image: A glowing wave");
    let start = Instant::now();
    // Kitty's graphics: an image sent with `ESC _ G`, and its cells as
    // Unicode placeholders (U+10EEEE).
    while !(run
        .output
        .lock()
        .unwrap()
        .windows(3)
        .any(|w| w == b"\x1b_G")
        && String::from_utf8_lossy(&run.output.lock().unwrap()).contains('\u{10EEEE}'))
    {
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "no Kitty placeholder bytes"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    run.quit();
}

/// The same through a real Chrome, when `RICH_EMBED_CHROME` names one
/// (run as a normal user: the sandbox stays on).
#[test]
fn it_reads_a_page_through_chrome_and_follows_a_hint() {
    let Some(chrome) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/reader.html");
    let mut run = Run::start_with(
        &["--reader", page.to_str().unwrap()],
        "xterm-256color",
        Some(&chrome),
    );
    run.wait_for("Tiny drifters of the open sea");
    run.press(b"f");
    run.wait_for("[s]the second page");
    run.press(b"s");
    run.wait_for("You followed the link");
    run.press(b"H");
    run.wait_for("Tiny drifters");
    let (_, terminal) = run.quit();
    assert!(!terminal.screen().alternate_screen());
}

/// The temporary profiles a run's browser left in its `TMPDIR`.
fn profiles_left(tmp: &Path) -> Vec<String> {
    std::fs::read_dir(tmp)
        .map(|dir| {
            dir.filter_map(Result::ok)
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|name| name.starts_with("rich-embed-chrome-"))
                .collect()
        })
        .unwrap_or_default()
}

/// Quitting, and SIGTERM, end the browser and remove its temporary
/// profile, through a real Chrome when `RICH_EMBED_CHROME` names one.
#[test]
fn no_temporary_profile_is_left_after_quitting_or_sigterm() {
    let Some(chrome) = std::env::var_os("RICH_EMBED_CHROME") else {
        eprintln!("skipped: set RICH_EMBED_CHROME to a Chrome or Chromium binary to run");
        return;
    };
    let page = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/second.html");
    let page = page.to_str().unwrap();
    let wait_gone = |tmp: &Path, how: &str| {
        let start = Instant::now();
        while !profiles_left(tmp).is_empty() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "{how} left {:?}",
                profiles_left(tmp)
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };

    let run = Run::start_with(&["--reader", page], "xterm-256color", Some(&chrome));
    run.wait_for("You followed the link");
    assert_eq!(profiles_left(&run.tmp).len(), 1, "no profile while it ran");
    let tmp = run.tmp.clone();
    run.quit();
    wait_gone(&tmp, "quitting");

    let mut run = Run::start_with(&["--reader", page], "xterm-256color", Some(&chrome));
    run.wait_for("You followed the link");
    assert_eq!(profiles_left(&run.tmp).len(), 1, "no profile while it ran");
    let pid = run.child.process_id().expect("a pid");
    let status = std::process::Command::new("kill")
        .args(["-TERM", &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
    let start = Instant::now();
    while run.child.try_wait().unwrap().is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "SIGTERM did not end it"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    wait_gone(&run.tmp, "SIGTERM");
}

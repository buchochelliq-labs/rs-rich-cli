//! Micro assets on a real terminal (release-test audit A, F2–F4): what a
//! graphics terminal gets, what the exports and the pager get instead, and
//! that the `CSI 16 t` cell-size query neither swallows typeahead nor stops
//! a background job.
//!
//! Every test runs `rich` in a pseudo-terminal that answers nothing, with
//! the environment of a Kitty or iTerm2 window.
#![cfg(all(unix, feature = "art"))]

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

const RICH: &str = env!("CARGO_BIN_EXE_rich");

/// Kitty's environment (or iTerm2's), with `extra` on top.
fn terminal_env(iterm: bool, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut env = vec![
        ("PATH", "/usr/bin:/bin"),
        ("COLORTERM", "truecolor"),
        ("COLUMNS", "80"),
        ("LINES", "24"),
    ];
    if iterm {
        env.extend([("TERM", "xterm-256color"), ("TERM_PROGRAM", "iTerm.app")]);
    } else {
        env.extend([("TERM", "xterm-kitty"), ("KITTY_WINDOW_ID", "1")]);
    }
    env.extend_from_slice(extra);
    env.into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Run `program args` in a PTY (no pixel size reported) in `cwd` with only
/// `env`, typing `typed` as soon as it starts. Returns everything it wrote
/// once it exits, or after `limit`.
fn in_pty(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    typed: &[u8],
    limit: Duration,
) -> String {
    in_pty_answering(program, args, cwd, env, typed, None, limit)
}

/// [`in_pty`], with the terminal writing `answer.1` once `answer.0` appears
/// in the output.
fn in_pty_answering(
    program: &str,
    args: &[&str],
    cwd: &Path,
    env: &[(String, String)],
    typed: &[u8],
    mut answer: Option<(&str, &[u8])>,
    limit: Duration,
) -> String {
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(program);
    command.args(args);
    command.cwd(cwd);
    command.env_clear();
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = pty.slave.spawn_command(command).unwrap();
    drop(pty.slave);
    let mut writer = pty.master.take_writer().unwrap();
    writer.write_all(typed).unwrap();
    writer.flush().unwrap();
    let mut reader = pty.master.try_clone_reader().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 || send.send(buffer[..read].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + limit;
    let mut out = Vec::new();
    loop {
        while let Ok(bytes) = receive.try_recv() {
            out.extend(bytes);
        }
        if let Some((trigger, reply)) = answer {
            if String::from_utf8_lossy(&out).contains(trigger) {
                writer.write_all(reply).unwrap();
                writer.flush().unwrap();
                answer = None;
            }
        }
        if child.try_wait().unwrap().is_some() || Instant::now() > deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    // What was written just before the exit.
    std::thread::sleep(Duration::from_millis(100));
    while let Ok(bytes) = receive.try_recv() {
        out.extend(bytes);
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn body(html: &str) -> &str {
    let start = html.find("<code").expect("a code block");
    &html[start
        ..html[start..]
            .find("</code>")
            .map_or(html.len(), |e| start + e)]
}

#[test]
fn exports_and_the_pager_get_the_fallback_on_a_graphics_terminal() {
    // F2: on Kitty the asset's cells are Unicode placeholders, and on
    // iTerm2 (or Sixel) blank cells under an image escape. Neither is the
    // asset in an HTML or SVG file, or in a pager; the fallback is.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    for (mode, iterm) in [("kitty", false), ("iterm", true)] {
        let env = terminal_env(
            iterm,
            &[
                ("HOME", home.to_str().unwrap()),
                ("RICH_CELL_PIXELS", "8x16"),
                ("RICH_MICRO", mode),
            ],
        );
        let html = format!("{mode}.html");
        let svg = format!("{mode}.svg");
        let args = [
            "-p",
            "Deploy :micro:status/success: done",
            "--emoji",
            "--export-html",
            html.as_str(),
            "--export-svg",
            svg.as_str(),
        ];
        in_pty(RICH, &args, dir.path(), &env, b"", Duration::from_secs(20));
        let html = std::fs::read_to_string(dir.path().join(&html)).unwrap();
        let shown = body(&html);
        assert!(shown.contains('✅'), "{mode} HTML: {shown:?}");
        assert!(
            !shown.contains(['\u{10eeee}', '\u{2800}']),
            "{mode} HTML: {shown:?}"
        );
        let svg = std::fs::read_to_string(dir.path().join(&svg)).unwrap();
        assert!(svg.contains('✅'), "{mode} SVG");
        assert!(!svg.contains(['\u{10eeee}', '\u{2800}']), "{mode} SVG");

        // The pager (`cat`, so what it was handed shows on the terminal).
        let mut env = env.clone();
        env.push(("PAGER".into(), "cat".into()));
        let paged = in_pty(
            RICH,
            &[
                "-p",
                "Deploy :micro:status/success: done",
                "--emoji",
                "--pager",
            ],
            dir.path(),
            &env,
            b"",
            Duration::from_secs(20),
        );
        assert!(paged.contains('✅'), "{mode} pager: {paged:?}");
        assert!(
            !paged.contains("\u{1b}_G") && !paged.contains("\u{1b}]1337;"),
            "{mode} pager: {paged:?}"
        );
    }
}

#[test]
fn the_cell_size_query_keeps_typeahead() {
    // F3: with no pixel size from the window and no RICH_CELL_PIXELS, rich
    // asks the terminal (`CSI 16 t`). What the user typed before that must
    // reach the next program, not the query's reader.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let env = terminal_env(false, &[("HOME", home.to_str().unwrap())]);
    let script =
        format!("sleep 0.3; '{RICH}' -p 'x :micro:status/success:' --emoji; read x; echo GOT=[$x]");
    let out = in_pty(
        "/bin/sh",
        &["-c", &script],
        dir.path(),
        &env,
        b"typed-ahead\n",
        Duration::from_secs(20),
    );
    assert!(out.contains("GOT=[typed-ahead]"), "{out:?}");
}

#[test]
fn a_background_job_is_not_stopped_by_the_cell_size_query() {
    // F4: a job in the background may not change the terminal's modes; the
    // kernel stops it (SIGTTOU) until `fg`. The query is skipped there.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let env = terminal_env(false, &[("HOME", home.to_str().unwrap())]);
    let script = format!(
        "set -m; '{RICH}' -p 'x :micro:status/success:' --emoji & sleep 3; jobs -l; \
         kill -9 %1 2>/dev/null; echo END"
    );
    let out = in_pty(
        "/bin/sh",
        &["-c", &script],
        dir.path(),
        &env,
        b"",
        Duration::from_secs(20),
    );
    assert!(out.contains("END"), "{out:?}");
    assert!(!out.contains("Stopped"), "{out:?}");
}

#[test]
fn the_cell_size_query_reads_only_the_replies() {
    // F3: a terminal that answers is still asked, and the reader stops at
    // the device-attributes reply. A key pressed while the terminal was
    // answering ("zz" and Return before the replies) is dropped, never
    // pushed back on the input queue (TIOCSTI would replay terminal replies
    // to the shell as typed input), so the next `read` sees nothing.
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let env = terminal_env(false, &[("HOME", home.to_str().unwrap())]);
    let script = format!(
        "'{RICH}' micro preview status/success --report json; \
         read x; echo GOT=[$x]"
    );
    let out = in_pty_answering(
        "/bin/sh",
        &["-c", &script],
        dir.path(),
        &env,
        b"",
        Some(("\u{1b}[16t\u{1b}[c", b"zz\r\x1b[6;20;10t\x1b[?62;22c")),
        Duration::from_secs(20),
    );
    assert!(out.contains("\"cell_pixels\": \"10x20\""), "{out:?}");
    assert!(!out.contains("GOT=[zz]"), "{out:?}");
}

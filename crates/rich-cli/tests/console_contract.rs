//! The binary's own console contract, kept when the core console follows
//! rich 15.0.0: `FORCE_COLOR` and `TTY_COMPATIBLE` do not colour redirected
//! stdout, and a reader closing the pipe early is success.

use std::io::{Read, Write};
use std::process::{Command, Stdio};

fn bin() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rich"));
    command
        .env_remove("NO_COLOR")
        .env_remove("RS_RICH_BATCH_TERMINAL")
        .env("COLUMNS", "80")
        .env("TERM", "xterm-256color")
        .env("COLORTERM", "truecolor");
    command
}

fn run_with(args: &[&str], stdin: &str, env: &[(&str, &str)]) -> (Vec<u8>, bool) {
    let mut command = bin();
    for (key, value) in env {
        command.env(key, value);
    }
    let mut child = command
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rich");
    if let Err(error) = child.stdin.take().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe, "{error}");
    }
    let output = child.wait_with_output().expect("wait rich");
    (output.stdout, output.status.success())
}

#[test]
fn force_color_leaves_redirected_stdout_plain() {
    let cases: [(&[&str], &str); 4] = [
        (&["-p", "[bold red]hello[/] 1.5 True"], ""),
        (&["--markdown", "-"], "# Title\n\n*some* `code`\n"),
        (&["--json", "-"], "{\"a\": [1, true, null]}"),
        (&["--rule", "title"], ""),
    ];
    for env in [
        &[("FORCE_COLOR", "1")][..],
        &[("FORCE_COLOR", "3"), ("TTY_COMPATIBLE", "1")][..],
    ] {
        for (args, stdin) in cases {
            let (out, ok) = run_with(args, stdin, env);
            assert!(ok, "{args:?} with {env:?} failed");
            assert!(!out.is_empty(), "{args:?} printed nothing");
            assert!(
                !out.contains(&0x1b),
                "{args:?} with {env:?} wrote escapes: {:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }
}

/// `rich … | head -c1`: the reader takes one byte and goes away while
/// the CLI is still writing. The core console would exit with status 1
/// there, as upstream does; the binary counts it as success.
#[cfg(unix)]
#[test]
fn a_reader_closing_the_pipe_early_is_success() {
    // Far more than a pipe buffer holds, so the write is still under way
    // when the reader closes its end.
    let input: String = (0..20_000)
        .map(|line| format!("line {line} of a long document\n"))
        .collect();
    let mut child = bin()
        .args(["--markdown", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn rich");
    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(input.as_bytes());
    });
    let mut stdout = child.stdout.take().unwrap();
    let mut byte = [0u8; 1];
    stdout.read_exact(&mut byte).expect("the first byte");
    drop(stdout);
    writer.join().unwrap();
    let output = child.wait_with_output().expect("wait rich");
    assert!(
        output.status.success(),
        "status {:?}, stderr {}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
}

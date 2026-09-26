//! The `mmdc` backend: fallbacks when it is missing, slow or failing, and (when
//! `RICH_MERMAID_MMDC` names a real `mmdc`) rendering every diagram type.
#![cfg(feature = "mmdc")]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rich::color::ColorSystem;
use rich::protocol::{ConsoleEnvironment, RenderEnvironment, Support, TargetCapabilities};
use rich::Console;
use rich_mermaid::mmdc::{render_png, MmdcError, MmdcOptions};
use rich_mermaid::{Backend, Mermaid, MermaidOptions};

const FLOWCHART: &str = "graph LR\n  A --> B";
const SEQUENCE: &str = "sequenceDiagram\n  Alice->>Bob: Hi";

/// A destination that shows Sixel graphics, so the backend runs even for a
/// flowchart.
struct Graphics(bool);

impl RenderEnvironment for Graphics {
    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            width: 160,
            height: 48,
            color_system: Some(ColorSystem::Truecolor),
            interactive: true,
            unicode: true,
            hyperlinks: false,
            sixel: if self.0 {
                Support::Confirmed
            } else {
                Support::Unsupported
            },
        }
    }
}

fn console(sixel: bool) -> Console {
    let mut console = Console::builder()
        .width(160)
        .color_system(Some(ColorSystem::Truecolor))
        .force_terminal(true)
        .build();
    console.set_render_environment(Some(Arc::new(Graphics(sixel))));
    console
}

fn render(source: &str, mmdc: MmdcOptions, sixel: bool) -> String {
    let options = MermaidOptions {
        backend: Backend::Mmdc,
        mmdc,
        ..MermaidOptions::default()
    };
    console(sixel).render_to_string(&Mermaid::new(source).options(options))
}

/// `text` without CSI styling and Sixel (DCS) sequences.
fn plain(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('[') => {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            Some('P') => {
                while let Some(c) = chars.next() {
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// A fake `mmdc`: a shell script with `body`.
#[cfg(unix)]
fn script(name: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rich-mermaid-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_missing_mmdc_falls_back_to_text_or_source() {
    let missing = MmdcOptions {
        program: PathBuf::from("/nonexistent/mmdc"),
        ..MmdcOptions::default()
    };
    assert_eq!(
        render_png(FLOWCHART, &missing),
        Err(MmdcError::NotFound("/nonexistent/mmdc".into()))
    );
    let out = plain(&render(FLOWCHART, missing.clone(), true));
    assert!(out.contains("│ A ├─►│ B │"), "{out}");
    assert!(
        out.contains("Mermaid: `/nonexistent/mmdc` is not installed; drawn as text"),
        "{out}"
    );
    let out = plain(&render(SEQUENCE, missing, true));
    assert!(
        out.starts_with(
            "Mermaid: `/nonexistent/mmdc` is not installed; sequence diagrams are not drawn as text"
        ),
        "{out}"
    );
    assert!(out.contains("Alice->>Bob: Hi"), "{out}");
}

#[test]
fn without_graphics_a_flowchart_prefers_text_and_never_runs_mmdc() {
    // A program that would fail loudly if it ran.
    let never = MmdcOptions {
        program: PathBuf::from("/nonexistent/mmdc"),
        ..MmdcOptions::default()
    };
    let out = plain(&render(FLOWCHART, never, false));
    assert!(out.contains("│ A ├─►│ B │"), "{out}");
    assert!(!out.contains("Mermaid:"), "{out}");
}

#[cfg(unix)]
#[test]
fn a_slow_mmdc_is_stopped_at_the_timeout() {
    let slow = MmdcOptions {
        program: script("slow-mmdc", "sleep 30"),
        timeout: Duration::from_millis(300),
        ..MmdcOptions::default()
    };
    let started = Instant::now();
    assert_eq!(
        render_png(SEQUENCE, &slow),
        Err(MmdcError::Timeout(Duration::from_millis(300)))
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    let out = plain(&render(SEQUENCE, slow, true));
    assert!(
        out.starts_with("Mermaid: mmdc did not finish within 0.3 s;"),
        "{out}"
    );
}

#[cfg(unix)]
#[test]
fn a_failing_mmdc_reports_its_first_line() {
    let failing = MmdcOptions {
        program: script(
            "failing-mmdc",
            "echo '' >&2; echo 'Error: Parse error on line 2' >&2; exit 3",
        ),
        ..MmdcOptions::default()
    };
    let Err(MmdcError::Failed(message)) = render_png(SEQUENCE, &failing) else {
        panic!("expected a failure");
    };
    assert!(
        message.starts_with("Error: Parse error on line 2 ("),
        "{message}"
    );
    let out = plain(&render(FLOWCHART, failing, true));
    assert!(
        out.contains("mmdc failed: Error: Parse error on line 2"),
        "{out}"
    );
    assert!(out.contains("drawn as text"), "{out}");
}

#[cfg(unix)]
#[test]
fn the_source_reaches_mmdc_as_a_file_not_a_command_line() {
    // The fake mmdc copies its input file to the output path and records its
    // arguments; neither may contain the diagram source.
    let copy = MmdcOptions {
        program: script(
            "copying-mmdc",
            "while [ $# -gt 0 ]; do case $1 in --input) in=$2;; --output) out=$2;; esac; echo \"$1\" >> \"$(dirname \"$0\")/args\"; shift; done; cp \"$in\" \"$out\"",
        ),
        ..MmdcOptions::default()
    };
    let source = "sequenceDiagram\n  A->>B: $(touch /tmp/pwned) `id`";
    let bytes = render_png(source, &copy).unwrap();
    assert_eq!(bytes, source.as_bytes());
    let args = std::fs::read_to_string(copy.program.parent().unwrap().join("args")).unwrap();
    assert!(!args.contains("touch") && !args.contains("A->>B"), "{args}");
}

#[test]
fn oversized_sources_are_refused_before_running() {
    let small = MmdcOptions {
        program: PathBuf::from("/nonexistent/mmdc"),
        max_input: 10,
        ..MmdcOptions::default()
    };
    assert_eq!(
        render_png(SEQUENCE, &small),
        Err(MmdcError::TooLarge {
            bytes: SEQUENCE.len(),
            limit: 10
        })
    );
}

/// Runs only where `RICH_MERMAID_MMDC` names a real `mmdc` (CI installs
/// `@mermaid-js/mermaid-cli`); `RICH_MERMAID_PUPPETEER_CONFIG` optionally
/// names a Puppeteer configuration file.
#[test]
fn real_mmdc_renders_every_diagram_type() {
    let Some(program) = std::env::var_os("RICH_MERMAID_MMDC") else {
        eprintln!("skipped: RICH_MERMAID_MMDC is not set");
        return;
    };
    let options = MmdcOptions {
        program: PathBuf::from(program),
        puppeteer_config: std::env::var_os("RICH_MERMAID_PUPPETEER_CONFIG").map(PathBuf::from),
        timeout: Duration::from_secs(60),
        ..MmdcOptions::default()
    };
    let diagrams = [
        (
            "flowchart",
            "flowchart TD\n  A[Start] --> B{Ok?}\n  B -->|yes| C[Done]",
        ),
        ("sequence", SEQUENCE),
        (
            "class",
            "classDiagram\n  Animal <|-- Duck\n  Animal : +int age",
        ),
        (
            "state",
            "stateDiagram-v2\n  [*] --> Still\n  Still --> Moving\n  Moving --> [*]",
        ),
    ];
    for (name, source) in diagrams {
        let png = render_png(source, &options).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "{name}: not a PNG");
        // With Sixel, the image is drawn as a Sixel sequence.
        let out = render(source, options.clone(), true);
        assert!(out.contains("\x1bP"), "{name}: no Sixel output");
        assert!(!plain(&out).contains("Mermaid:"), "{name}: {}", plain(&out));
    }
    // Without Sixel, a sequence diagram is drawn in blocks, with a note.
    let out = render(SEQUENCE, options, false);
    let text = plain(&out);
    if let Some(dump) = std::env::var_os("RICH_MERMAID_DUMP") {
        std::fs::write(dump, &out).unwrap();
    }
    assert!(
        text.contains("Mermaid: drawn with block characters"),
        "{}",
        text.lines().rev().take(4).collect::<Vec<_>>().join("\n")
    );
}

#[cfg(unix)]
#[test]
fn the_temporary_directory_is_removed_after_success_and_failure() {
    for (name, status) in [("ok-mmdc", 0), ("bad-mmdc", 1)] {
        let program = script(
            name,
            &format!(
                "while [ $# -gt 0 ]; do case $1 in --input) in=$2;; --output) out=$2;; esac; shift; done\n\
                 echo \"$in\" > \"$(dirname \"$0\")/{name}.input\"; cp \"$in\" \"$out\"; exit {status}"
            ),
        );
        let options = MmdcOptions {
            program: program.clone(),
            ..MmdcOptions::default()
        };
        assert_eq!(render_png(SEQUENCE, &options).is_ok(), status == 0);
        let input = std::fs::read_to_string(program.with_extension("input")).unwrap();
        let dir = PathBuf::from(input.trim()).parent().unwrap().to_path_buf();
        assert!(!dir.exists(), "{} was left behind", dir.display());
    }
}

/// `render_png` reads only what fits under `max_output`: a 100 MB "PNG" is
/// refused without being read, and a log of any size costs 4 KiB.
#[cfg(unix)]
#[test]
fn oversized_images_and_logs_are_not_read_into_memory() {
    let big = MmdcOptions {
        program: script(
            "big-mmdc",
            "while [ $# -gt 0 ]; do case $1 in --output) out=$2;; esac; shift; done\n\
             printf '\\211PNG\\r\\n\\032\\n' > \"$out\"; truncate -s 100M \"$out\"",
        ),
        ..MmdcOptions::default()
    };
    assert_eq!(
        render_png(SEQUENCE, &big),
        Err(MmdcError::OutputTooLarge {
            bytes: 100 * 1024 * 1024,
            limit: 16 * 1024 * 1024
        })
    );
    let small = MmdcOptions {
        max_output: 7,
        ..big.clone()
    };
    assert!(matches!(
        render_png(SEQUENCE, &small),
        Err(MmdcError::OutputTooLarge { limit: 7, .. })
    ),);
    let chatty = MmdcOptions {
        program: script(
            "chatty-mmdc",
            "head -c 1000000 /dev/zero | tr '\\0' 'x' >&2; exit 1",
        ),
        ..MmdcOptions::default()
    };
    let Err(MmdcError::Failed(message)) = render_png(SEQUENCE, &chatty) else {
        panic!("expected a failure");
    };
    assert!(message.starts_with(&"x".repeat(200)), "{message}");
}

/// Ctrl-C at a terminal sends SIGINT to the foreground process group. `mmdc`
/// must be in that group (so it stops, and Puppeteer closes Chromium), and the
/// private temporary directory holding the source must still be removed
/// although the process that made it was killed.
#[cfg(unix)]
mod interrupt {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use rich_mermaid::mmdc::{render_png, MmdcOptions};

    const CHILD: &str = "RICH_MERMAID_INTERRUPT_CHILD";

    fn wait_for(path: &Path, limit: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if std::fs::read_to_string(path).is_ok_and(|s| s.ends_with('\n')) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// Running, not merely unreaped: where PID 1 does not reap orphans (some
    /// containers), a killed process stays a zombie that `kill -0` still finds.
    fn running(pid: &str) -> bool {
        let found = Command::new("kill")
            .args(["-0", pid])
            .status()
            .is_ok_and(|s| s.success());
        let zombie = Command::new("ps")
            .args(["-o", "stat=", "-p", pid])
            .output()
            .is_ok_and(|out| out.stdout.starts_with(b"Z"));
        found && !zombie
    }

    /// Runs only as the parent test's child process: renders with a fake
    /// mmdc that records its pid and input path, then sleeps.
    #[test]
    fn helper_child_render() {
        let Some(dir) = std::env::var_os(CHILD) else {
            return;
        };
        let dir = PathBuf::from(dir);
        let script = dir.join("mmdc");
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(
                &script,
                format!(
                    "#!/bin/sh\nwhile [ $# -gt 0 ]; do case $1 in --input) echo \"$2\" > '{d}/input';; esac; shift; done\necho $$ > '{d}/pid'\nsleep 3\n",
                    d = dir.display()
                ),
            )
            .unwrap();
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let options = MmdcOptions {
            program: script,
            ..MmdcOptions::default()
        };
        let _ = render_png("sequenceDiagram\n  A->>B: secret", &options);
    }

    #[test]
    fn ctrl_c_stops_mmdc_and_removes_its_temp_dir() {
        if std::env::var_os(CHILD).is_some() {
            return;
        }
        use std::os::unix::process::CommandExt;
        let dir =
            std::env::temp_dir().join(format!("rich-mermaid-interrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "interrupt::helper_child_render",
                "--exact",
                "--test-threads=1",
            ])
            .env(CHILD, &dir)
            .process_group(0) // the "foreground job" a shell would create
            .spawn()
            .unwrap();
        assert!(
            wait_for(&dir.join("pid"), Duration::from_secs(20)),
            "fake mmdc never ran"
        );
        let pid = std::fs::read_to_string(dir.join("pid"))
            .unwrap()
            .trim()
            .to_string();
        let input = PathBuf::from(std::fs::read_to_string(dir.join("input")).unwrap().trim());
        let temp = input.parent().unwrap().to_path_buf();
        // Ctrl-C: SIGINT to the whole foreground group.
        Command::new("kill")
            .args(["-INT", "--", &format!("-{}", child.id())])
            .status()
            .unwrap();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(300));
        let mmdc_survived = running(&pid);
        // Well before the fake mmdc's own 3 s would have run out.
        let started = Instant::now();
        while temp.exists() && started.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let leaked = temp.exists();
        if leaked {
            let _ = std::fs::remove_dir_all(&temp);
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(!mmdc_survived, "mmdc (pid {pid}) kept running after Ctrl-C");
        assert!(!leaked, "{} was left behind", temp.display());
    }
}

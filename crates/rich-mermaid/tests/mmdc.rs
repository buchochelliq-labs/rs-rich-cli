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

//! Render a DOT source with Graphviz's own `dot` binary, for SVG.
//!
//! The native parser ([`crate::dot`]) draws the hand-written subset as
//! text; full Graphviz layout and SVG come from `dot` when it is installed
//! and the caller asks for it. Like rs-rich-mermaid's `mmdc` backend, it only
//! runs where the caller chose it (`rich` lets a command-line flag or a
//! trusted config choose it, never a project's `./rich.toml`).
//!
//! The source goes to `dot` on its standard input, never through a shell or
//! a command line; the process gets a fixed argument list, a timeout, and
//! size caps on what goes in and comes out.

use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How to run `dot`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphvizOptions {
    /// The program to run: `dot` on `PATH` by default.
    pub program: PathBuf,
    /// Kill it after this long (default 20 s).
    pub timeout: Duration,
    /// Refuse sources larger than this many bytes (default 256 KiB).
    pub max_input: usize,
    /// Refuse output larger than this many bytes (default 16 MiB).
    pub max_output: usize,
}

impl Default for GraphvizOptions {
    fn default() -> Self {
        GraphvizOptions {
            program: PathBuf::from("dot"),
            timeout: Duration::from_secs(20),
            max_input: 256 * 1024,
            max_output: 16 * 1024 * 1024,
        }
    }
}

/// Why `dot` produced no SVG.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphvizError {
    /// The program is not installed (or not on `PATH`).
    NotFound(String),
    /// The source is larger than [`GraphvizOptions::max_input`].
    TooLarge { bytes: usize, limit: usize },
    /// The output is larger than [`GraphvizOptions::max_output`].
    OutputTooLarge { limit: usize },
    /// It ran longer than [`GraphvizOptions::timeout`] and was stopped.
    Timeout(Duration),
    /// It exited unsuccessfully; the first line of what it printed.
    Failed(String),
    /// Anything else: starting it, its pipes, output that is not UTF-8.
    Io(String),
}

impl fmt::Display for GraphvizError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphvizError::NotFound(program) => write!(f, "`{program}` is not installed"),
            GraphvizError::TooLarge { bytes, limit } => {
                write!(
                    f,
                    "the graph is {bytes} bytes, more than Graphviz's {limit}"
                )
            }
            GraphvizError::OutputTooLarge { limit } => {
                write!(f, "Graphviz wrote more than {limit} bytes")
            }
            GraphvizError::Timeout(after) => {
                write!(
                    f,
                    "Graphviz did not finish within {} s",
                    after.as_secs_f32()
                )
            }
            GraphvizError::Failed(message) => write!(f, "Graphviz failed: {message}"),
            GraphvizError::Io(message) => write!(f, "Graphviz could not run: {message}"),
        }
    }
}

impl std::error::Error for GraphvizError {}

/// Render `source` to an SVG document with `dot -Tsvg`.
pub fn render_svg(source: &str, options: &GraphvizOptions) -> Result<String, GraphvizError> {
    if source.len() > options.max_input {
        return Err(GraphvizError::TooLarge {
            bytes: source.len(),
            limit: options.max_input,
        });
    }
    let mut child = Command::new(&options.program)
        .arg("-Tsvg")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GraphvizError::NotFound(options.program.display().to_string())
            } else {
                GraphvizError::Io(e.to_string())
            }
        })?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input = source.as_bytes().to_vec();
    let writer = std::thread::spawn(move || {
        // A `dot` that exits early closes the pipe; its status says why.
        let _ = stdin.write_all(&input);
    });
    let limit = options.max_output;
    // Each pipe is drained past its cap, so `dot` never blocks on a full
    // pipe: an oversized output is reported, not mistaken for a timeout.
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stdout).take(limit as u64 + 1).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        bytes
    });
    let mut stderr = child.stderr.take().expect("stderr is piped");
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = (&mut stderr).take(4096).read_to_end(&mut bytes);
        let _ = std::io::copy(&mut stderr, &mut std::io::sink());
        String::from_utf8_lossy(&bytes).into_owned()
    });

    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= options.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GraphvizError::Timeout(options.timeout));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(GraphvizError::Io(e.to_string()));
            }
        }
    };
    let _ = writer.join();
    let svg = reader.join().unwrap_or_default();
    let printed = errors.join().unwrap_or_default();
    if !status.success() {
        let first = printed
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or("no output");
        let first: String = first
            .chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect();
        return Err(GraphvizError::Failed(format!("{first} ({status})")));
    }
    if svg.len() > limit {
        return Err(GraphvizError::OutputTooLarge { limit });
    }
    String::from_utf8(svg).map_err(|_| GraphvizError::Io("the SVG is not UTF-8".into()))
}

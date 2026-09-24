//! Stack traces from Rust, Python, Java and JavaScript, in one shape.
//!
//! [`parse`] recognises a trace's language and normalises it into a
//! [`StackTrace`]: the error kind and message, its [`Frame`]s ordered with the
//! most recent call **last** (Python's order), and any chained cause. Each
//! language is a [`TraceParser`]; [`Parsers`] holds the built-in four and
//! takes more, so other formats plug in without changing this module.
//!
//! A [`StackTrace`] renders with frame locations linked through a
//! [`Hyperlinker`], library frames dimmed and runs of them collapsed, and each
//! cause after the trace it caused. [`capture`] and [`panic_hook`] build one
//! for the current thread from `std::backtrace`.

use fancy_regex::Regex;
use rich::{Console, ConsoleOptions, Renderable, Segment, Text};
use std::sync::OnceLock;

use crate::event::theme_style;
use crate::hyperlink::Hyperlinker;

/// The most causes the built-in parsers keep, and a view renders, below the
/// outermost error. Traces are untrusted input; a longer chain keeps the
/// causes nearest the error and records how many were left out.
pub const MAX_CAUSES: usize = 64;

/// The language a trace came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Language {
    Rust,
    Python,
    Java,
    JavaScript,
    /// A language from a caller-supplied parser.
    Other(String),
}

/// One stack frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// The function, method or symbol, when the trace names one.
    pub function: Option<String>,
    /// The source file.
    pub path: Option<String>,
    /// The 1-based line.
    pub line: Option<usize>,
    /// The 1-based column.
    pub column: Option<usize>,
    /// The source line, when the trace quotes it (Python does).
    pub source: Option<String>,
    /// Language-specific details kept from the trace (`module`, `native`, …).
    pub metadata: Vec<(String, String)>,
    /// Whether the frame is in a standard library, runtime or dependency
    /// rather than the application.
    pub library: bool,
}

/// How a trace relates to its [`StackTrace::cause`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CauseKind {
    /// The cause led to this error (Python's `raise … from`, Java's
    /// `Caused by:`, JavaScript's `[cause]`).
    CausedBy,
    /// This error was raised while handling the cause (Python's implicit
    /// chaining).
    DuringHandling,
}

/// A normalised stack trace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StackTrace {
    /// The source language.
    pub language: Language,
    /// The error type (`ValueError`, `java.io.IOException`, `panic`).
    pub kind: Option<String>,
    /// The error message.
    pub message: Option<String>,
    /// Frames, most recent call last.
    pub frames: Vec<Frame>,
    /// The error this one came from.
    pub cause: Option<Box<StackTrace>>,
    /// How `cause` relates to this error.
    pub cause_kind: CauseKind,
    /// Causes below this one that were left out because the chain was longer
    /// than [`MAX_CAUSES`]. Rendered as `… N more causes`.
    pub omitted_causes: usize,
    /// Where the error was raised, when the trace names it apart from the
    /// frames (a Rust panic's `panicked at` location).
    pub location: Option<Frame>,
}

impl StackTrace {
    /// An empty trace for `language`.
    pub fn new(language: Language) -> Self {
        StackTrace {
            language,
            kind: None,
            message: None,
            frames: Vec::new(),
            cause: None,
            cause_kind: CauseKind::CausedBy,
            omitted_causes: 0,
            location: None,
        }
    }

    /// The traces from this one down its causes.
    pub fn chain(&self) -> impl Iterator<Item = &StackTrace> {
        std::iter::successors(Some(self), |trace| trace.cause.as_deref())
    }

    /// The most recent application (non-library) frame, if any: where to look.
    pub fn origin(&self) -> Option<&Frame> {
        self.frames.iter().rev().find(|frame| !frame.library)
    }

    /// A renderable view with the default options.
    pub fn render_options(&self) -> StackTraceView<'_> {
        StackTraceView {
            trace: self,
            linker: Hyperlinker::new(),
            show_library: false,
        }
    }
}

/// Unlinks the cause chain one trace at a time: the derived drop recurses
/// once per cause and overflows the stack on a long hand-built chain.
impl Drop for StackTrace {
    fn drop(&mut self) {
        let mut next = self.cause.take();
        while let Some(mut trace) = next {
            next = trace.cause.take();
        }
    }
}

/// Link `traces`, outermost first, each the cause of the one before it,
/// keeping at most [`MAX_CAUSES`] causes. The deepest kept trace counts the
/// ones left out.
fn link_causes(mut traces: Vec<StackTrace>) -> Option<StackTrace> {
    let omitted = traces.len().saturating_sub(MAX_CAUSES + 1);
    traces.truncate(MAX_CAUSES + 1);
    if let Some(last) = traces.last_mut() {
        last.omitted_causes += omitted;
    }
    let mut result: Option<StackTrace> = None;
    for mut trace in traces.into_iter().rev() {
        trace.cause = result.take().map(Box::new);
        result = Some(trace);
    }
    result
}

/// Recognises and parses one trace format.
pub trait TraceParser: Send + Sync {
    /// Whether `text` looks like this format.
    fn detect(&self, text: &str) -> bool;
    /// Parse `text`, or `None` when it has no frames or header to keep.
    fn parse(&self, text: &str) -> Option<StackTrace>;
}

/// An ordered set of parsers; the first that detects a trace parses it.
pub struct Parsers {
    parsers: Vec<Box<dyn TraceParser>>,
}

impl Default for Parsers {
    fn default() -> Self {
        Parsers {
            parsers: vec![
                Box::new(PythonParser),
                Box::new(JavaParser),
                Box::new(RustParser),
                Box::new(JavaScriptParser),
            ],
        }
    }
}

impl Parsers {
    /// The built-in Python, Java, Rust and JavaScript parsers.
    pub fn new() -> Self {
        Parsers::default()
    }

    /// Try `parser` before the built-in ones.
    pub fn with_parser(mut self, parser: impl TraceParser + 'static) -> Self {
        self.parsers.insert(0, Box::new(parser));
        self
    }

    /// Parse `text` with the first parser that detects it.
    pub fn parse(&self, text: &str) -> Option<StackTrace> {
        self.parsers
            .iter()
            .find(|parser| parser.detect(text))
            .and_then(|parser| parser.parse(text))
    }
}

/// Parse `text` with the built-in parsers.
pub fn parse(text: &str) -> Option<StackTrace> {
    Parsers::default().parse(text)
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("valid trace pattern"))
}

fn number(value: Option<fancy_regex::Match<'_>>) -> Option<usize> {
    value.and_then(|value| value.as_str().parse().ok())
}

/// `kind: message`, splitting on the first `": "` when the kind looks like a
/// type name.
fn kind_and_message(line: &str) -> (Option<String>, Option<String>) {
    let line = line.trim();
    match line.split_once(": ") {
        Some((kind, message)) if !kind.contains(' ') => {
            (Some(kind.to_string()), Some(message.to_string()))
        }
        _ if !line.contains(' ') && !line.is_empty() => (Some(line.to_string()), None),
        _ => (None, Some(line.to_string()).filter(|line| !line.is_empty())),
    }
}

/// Parses `std::backtrace` output and panic messages.
pub struct RustParser;

impl RustParser {
    fn is_library(function: &str, path: Option<&str>) -> bool {
        const CRATES: [&str; 6] = [
            "std::",
            "core::",
            "alloc::",
            "backtrace::",
            "rust_begin_unwind",
            "__rust",
        ];
        CRATES.iter().any(|prefix| {
            function.starts_with(prefix) || function.starts_with(&format!("<{prefix}"))
        }) || path.is_some_and(|path| {
            path.starts_with("/rustc/")
                || path.contains("/.cargo/registry/")
                || path.contains("\\.cargo\\registry\\")
        })
    }
}

impl TraceParser for RustParser {
    fn detect(&self, text: &str) -> bool {
        static FRAME: OnceLock<Regex> = OnceLock::new();
        text.contains("panicked at")
            || regex(&FRAME, r"(?m)^\s*\d+: \S")
                .is_match(text)
                .unwrap_or(false)
    }

    fn parse(&self, text: &str) -> Option<StackTrace> {
        static FRAME: OnceLock<Regex> = OnceLock::new();
        static AT: OnceLock<Regex> = OnceLock::new();
        static PANIC: OnceLock<Regex> = OnceLock::new();
        static OLD_PANIC: OnceLock<Regex> = OnceLock::new();
        let frame_re = regex(&FRAME, r"^\s*\d+:\s+(.+?)\s*$");
        let at_re = regex(&AT, r"^\s+at\s+(.+?):(\d+)(?::(\d+))?\s*$");
        let mut trace = StackTrace::new(Language::Rust);
        let lines: Vec<&str> = text.lines().collect();

        // `thread 'main' panicked at src/main.rs:4:5:` then the message
        // (Rust 1.73+), or `panicked at 'message', src/main.rs:4:5` before.
        let panic_re = regex(&PANIC, r"panicked at (.+?):(\d+):(\d+):\s*$");
        let old_re = regex(&OLD_PANIC, r"panicked at '(.*)', (.+?):(\d+):(\d+)");
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            if let Ok(Some(caps)) = old_re.captures(line) {
                trace.kind = Some("panic".into());
                trace.message = Some(caps[1].to_string());
                trace.metadata_location(&caps[2], number(caps.get(3)), number(caps.get(4)));
            } else if let Ok(Some(caps)) = panic_re.captures(line) {
                trace.kind = Some("panic".into());
                trace.metadata_location(&caps[1], number(caps.get(2)), number(caps.get(3)));
                let mut message = Vec::new();
                while index + 1 < lines.len()
                    && !lines[index + 1].starts_with("stack backtrace:")
                    && !lines[index + 1].starts_with("note:")
                    && !frame_re.is_match(lines[index + 1]).unwrap_or(false)
                {
                    index += 1;
                    message.push(lines[index]);
                }
                trace.message = Some(message.join("\n")).filter(|message| !message.is_empty());
            } else if let Ok(Some(caps)) = frame_re.captures(line) {
                let mut function = caps[1].to_string();
                // `path::to::fn::h0123456789abcdef`: drop the symbol hash.
                if let Some(stripped) = function.rsplit_once("::h").filter(|(_, hash)| {
                    hash.len() == 16 && hash.chars().all(|c| c.is_ascii_hexdigit())
                }) {
                    function = stripped.0.to_string();
                }
                let mut frame = Frame {
                    function: Some(function),
                    ..Frame::default()
                };
                let next = lines.get(index + 1).copied().unwrap_or("");
                if let Ok(Some(at)) = at_re.captures(next) {
                    frame.path = at.get(1).map(|path| path.as_str().to_string());
                    frame.line = number(at.get(2));
                    frame.column = number(at.get(3));
                    index += 1;
                }
                frame.library = Self::is_library(
                    frame.function.as_deref().unwrap_or(""),
                    frame.path.as_deref(),
                );
                trace.frames.push(frame);
            }
            index += 1;
        }
        // Backtraces list the most recent call first.
        trace.frames.reverse();
        (trace.kind.is_some() || !trace.frames.is_empty()).then_some(trace)
    }
}

impl StackTrace {
    /// Where a panic happened, kept as a frame-less location on the trace.
    fn metadata_location(&mut self, path: &str, line: Option<usize>, column: Option<usize>) {
        self.location = Some(Frame {
            path: Some(path.to_string()),
            line,
            column,
            ..Frame::default()
        });
    }
}

/// Parses CPython tracebacks, including chained exceptions.
pub struct PythonParser;

const PY_CAUSE: &str = "The above exception was the direct cause of the following exception:";
const PY_CONTEXT: &str = "During handling of the above exception, another exception occurred:";

impl PythonParser {
    fn block(text: &str) -> Option<StackTrace> {
        static FILE: OnceLock<Regex> = OnceLock::new();
        let file_re = regex(&FILE, r#"^\s*File "(.+?)", line (\d+)(?:, in (.+))?$"#);
        let mut trace = StackTrace::new(Language::Python);
        let mut last_line = None;
        let lines: Vec<&str> = text.lines().collect();
        let mut index = 0;
        while index < lines.len() {
            let line = lines[index];
            if let Ok(Some(caps)) = file_re.captures(line) {
                let path = caps[1].to_string();
                let library = path.contains("site-packages")
                    || path.contains("/lib/python")
                    || path.starts_with('<');
                let mut frame = Frame {
                    function: caps.get(3).map(|m| m.as_str().to_string()),
                    path: Some(path),
                    line: number(caps.get(2)),
                    library,
                    ..Frame::default()
                };
                // The quoted source line, and any `^^^^` marker row under it.
                while let Some(next) = lines.get(index + 1) {
                    let trimmed = next.trim();
                    if next.starts_with("    ")
                        && !trimmed.is_empty()
                        && !file_re.is_match(next).unwrap_or(false)
                    {
                        if trimmed.chars().all(|c| matches!(c, '^' | '~' | ' ')) {
                            index += 1;
                            continue;
                        }
                        if frame.source.is_none() {
                            frame.source = Some(trimmed.to_string());
                        }
                        index += 1;
                    } else {
                        break;
                    }
                }
                trace.frames.push(frame);
            } else if !line.trim().is_empty()
                && !line.starts_with("Traceback (most recent call last):")
                && !line.starts_with(' ')
            {
                last_line = Some(index);
            }
            index += 1;
        }
        if let Some(last) = last_line {
            // The exception line, plus any message lines after it.
            let rest = lines[last..].join("\n");
            let (kind, message) = kind_and_message(&rest);
            trace.kind = kind;
            trace.message = message;
        }
        (trace.kind.is_some() || !trace.frames.is_empty()).then_some(trace)
    }
}

impl TraceParser for PythonParser {
    fn detect(&self, text: &str) -> bool {
        text.contains("Traceback (most recent call last):")
    }

    fn parse(&self, text: &str) -> Option<StackTrace> {
        // Earlier blocks are causes of later ones; split on the chaining lines.
        let mut blocks: Vec<(String, Option<CauseKind>)> = Vec::new();
        let mut current = String::new();
        let mut kind = None;
        for line in text.lines() {
            let relation = match line.trim() {
                PY_CAUSE => Some(CauseKind::CausedBy),
                PY_CONTEXT => Some(CauseKind::DuringHandling),
                _ => None,
            };
            if let Some(relation) = relation {
                blocks.push((std::mem::take(&mut current), kind));
                kind = Some(relation);
            } else {
                current.push_str(line);
                current.push('\n');
            }
        }
        blocks.push((current, kind));
        // Root cause first; a block with no relation starts a new chain.
        let mut chain: Vec<StackTrace> = Vec::new();
        for (block, relation) in blocks {
            let Some(mut trace) = Self::block(&block) else {
                continue;
            };
            match relation {
                Some(relation) if !chain.is_empty() => trace.cause_kind = relation,
                _ => chain.clear(),
            }
            chain.push(trace);
        }
        chain.reverse();
        link_causes(chain)
    }
}

/// Parses JVM stack traces with `Caused by:` chains.
pub struct JavaParser;

impl TraceParser for JavaParser {
    fn detect(&self, text: &str) -> bool {
        static AT: OnceLock<Regex> = OnceLock::new();
        regex(&AT, r"(?m)^\s+at [\w$.<>]+\(.*\)\s*$")
            .is_match(text)
            .unwrap_or(false)
    }

    fn parse(&self, text: &str) -> Option<StackTrace> {
        static AT: OnceLock<Regex> = OnceLock::new();
        let at_re = regex(&AT, r"^\s+at (?:[\w.]+/)?([\w$.<>]+)\((.*)\)\s*$");
        let mut traces: Vec<StackTrace> = Vec::new();
        let mut header: Option<&str> = None;
        for line in text.lines() {
            if let Ok(Some(caps)) = at_re.captures(line) {
                // The header is the line right above the first frame.
                if traces.is_empty() {
                    let line = header.unwrap_or("").trim();
                    let line = line
                        .strip_prefix("Exception in thread ")
                        .and_then(|rest| rest.split_once("\" ").map(|(_, rest)| rest))
                        .unwrap_or(line);
                    let mut trace = StackTrace::new(Language::Java);
                    (trace.kind, trace.message) = kind_and_message(line);
                    traces.push(trace);
                }
                let Some(trace) = traces.last_mut() else {
                    continue;
                };
                let function = caps[1].to_string();
                let location = &caps[2];
                let mut frame = Frame {
                    library: ["java.", "javax.", "jdk.", "sun.", "com.sun.", "kotlin."]
                        .iter()
                        .any(|prefix| function.starts_with(prefix)),
                    function: Some(function),
                    ..Frame::default()
                };
                match location.rsplit_once(':') {
                    Some((file, line)) if line.parse::<usize>().is_ok() => {
                        frame.path = Some(file.to_string());
                        frame.line = line.parse().ok();
                    }
                    _ if location == "Native Method" => {
                        frame.metadata.push(("native".into(), "true".into()));
                    }
                    _ if location != "Unknown Source" && !location.is_empty() => {
                        frame.path = Some(location.to_string());
                    }
                    _ => {}
                }
                trace.frames.push(frame);
            } else if let Some(rest) = line.trim_start().strip_prefix("Caused by: ") {
                let mut trace = StackTrace::new(Language::Java);
                (trace.kind, trace.message) = kind_and_message(rest);
                traces.push(trace);
            } else if line.trim_start().starts_with("... ") && line.trim_end().ends_with(" more") {
                if let Some(trace) = traces.last_mut() {
                    trace.frames.push(Frame {
                        metadata: vec![("elided".into(), line.trim().to_string())],
                        library: true,
                        ..Frame::default()
                    });
                }
            } else if traces.is_empty() && !line.trim().is_empty() {
                header = Some(line);
            }
        }
        // JVM frames list the most recent call first; each `Caused by` is
        // the cause of the trace before it.
        for trace in &mut traces {
            trace.frames.reverse();
        }
        link_causes(traces)
    }
}

/// Parses V8 (`node`, Chrome) stack traces.
pub struct JavaScriptParser;

impl TraceParser for JavaScriptParser {
    fn detect(&self, text: &str) -> bool {
        static AT: OnceLock<Regex> = OnceLock::new();
        regex(&AT, r"(?m)^\s+at .*:\d+:\d+\)?(?: \{)?\s*$")
            .is_match(text)
            .unwrap_or(false)
    }

    fn parse(&self, text: &str) -> Option<StackTrace> {
        static AT: OnceLock<Regex> = OnceLock::new();
        let at_re = regex(
            &AT,
            r"^\s+at (?:(?:async )?(.+?) \()?(.+?):(\d+):(\d+)\)?(?: \{)?\s*$",
        );
        let mut traces: Vec<StackTrace> = Vec::new();
        let mut header: Option<&str> = None;
        for line in text.lines() {
            if let Ok(Some(caps)) = at_re.captures(line) {
                // The header is the line right above the first frame; Node
                // prints a source excerpt before it.
                if traces.is_empty() {
                    let mut trace = StackTrace::new(Language::JavaScript);
                    (trace.kind, trace.message) = kind_and_message(header.unwrap_or(""));
                    traces.push(trace);
                }
                let Some(trace) = traces.last_mut() else {
                    continue;
                };
                let path = caps[2].to_string();
                trace.frames.push(Frame {
                    library: path.starts_with("node:") || path.contains("node_modules"),
                    function: caps.get(1).map(|m| m.as_str().to_string()),
                    path: Some(path),
                    line: number(caps.get(3)),
                    column: number(caps.get(4)),
                    ..Frame::default()
                });
            } else if let Some(rest) = line.trim_start().strip_prefix("[cause]: ") {
                let mut trace = StackTrace::new(Language::JavaScript);
                (trace.kind, trace.message) = kind_and_message(rest.trim_end_matches(['{', ' ']));
                traces.push(trace);
            } else if line.trim_start().starts_with("... ") && line.trim_end().ends_with(" ...") {
                if let Some(trace) = traces.last_mut() {
                    trace.frames.push(Frame {
                        metadata: vec![("elided".into(), line.trim().to_string())],
                        library: true,
                        ..Frame::default()
                    });
                }
            } else if traces.is_empty() && !line.trim().is_empty() {
                header = Some(line);
            }
        }
        for trace in &mut traces {
            trace.frames.reverse();
        }
        link_causes(traces)
    }
}

/// The current thread's backtrace as a Rust [`StackTrace`] with `message`.
/// Frames come from `std::backtrace::Backtrace::force_capture`, so they need
/// debug info to carry file locations.
pub fn capture(message: impl Into<String>) -> StackTrace {
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let mut trace = RustParser
        .parse(&backtrace)
        .unwrap_or_else(|| StackTrace::new(Language::Rust));
    trace.kind = Some("panic".into());
    trace.message = Some(message.into());
    trace
}

/// A panic hook printing the panic as a [`StackTrace`] to `console`.
/// Install it with `std::panic::set_hook(Box::new(panic_hook(console)))`;
/// this module never installs anything itself.
pub fn panic_hook(
    console: Console,
) -> impl Fn(&std::panic::PanicHookInfo<'_>) + Send + Sync + 'static {
    let console = std::sync::Mutex::new(console);
    move |info| {
        let payload = info.payload();
        let message = payload
            .downcast_ref::<&str>()
            .map(|message| message.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "Box<dyn Any>".to_string());
        let mut trace = capture(message);
        if let Some(location) = info.location() {
            trace.location = Some(Frame {
                path: Some(location.file().to_string()),
                line: Some(location.line() as usize),
                column: Some(location.column() as usize),
                ..Frame::default()
            });
        }
        let console = console.lock().unwrap_or_else(|e| e.into_inner());
        console.print(&trace.render_options());
    }
}

/// Rendering options for a [`StackTrace`].
pub struct StackTraceView<'a> {
    trace: &'a StackTrace,
    linker: Hyperlinker,
    show_library: bool,
}

impl StackTraceView<'_> {
    /// Link frame locations with `linker` (default: `file://` links).
    pub fn hyperlinker(mut self, linker: Hyperlinker) -> Self {
        self.linker = linker;
        self
    }

    /// Show every library frame instead of collapsing runs of them.
    pub fn show_library(mut self, show: bool) -> Self {
        self.show_library = show;
        self
    }

    /// Every trace in the chain, root cause first, each after its cause and
    /// the bridge naming how they relate. Iterative, and capped at
    /// [`MAX_CAUSES`] causes like the parsers, so any chain renders.
    fn append_chain(&self, console: &Console, text: &mut Text) {
        let mut chain: Vec<&StackTrace> = Vec::new();
        let mut omitted = 0usize;
        for trace in self.trace.chain() {
            if chain.len() <= MAX_CAUSES {
                chain.push(trace);
            } else {
                omitted += 1;
            }
        }
        if let Some(last) = chain.last() {
            omitted += last.omitted_causes;
        }
        if omitted > 0 {
            let label = if omitted == 1 { "cause" } else { "causes" };
            text.append(
                &format!("… {omitted} more {label}\n\n"),
                Some(theme_style(console, "stacktrace.library", "dim").into()),
            );
        }
        for (index, trace) in chain.iter().enumerate().rev() {
            self.append_trace(console, text, trace);
            let Some(caused) = index.checked_sub(1).map(|at| chain[at]) else {
                continue;
            };
            let bridge = match caused.cause_kind {
                CauseKind::CausedBy => "The error above caused the following error:",
                CauseKind::DuringHandling => {
                    "The error below happened while handling the error above:"
                }
            };
            text.append("\n", None);
            text.append(
                bridge,
                Some(theme_style(console, "stacktrace.bridge", "italic dim").into()),
            );
            text.append("\n\n", None);
        }
    }

    /// One trace's frames and error line, without its causes.
    fn append_trace(&self, console: &Console, text: &mut Text, trace: &StackTrace) {
        let dim = theme_style(console, "stacktrace.library", "dim");
        let function_style = theme_style(console, "stacktrace.function", "green");
        let location_style = theme_style(console, "stacktrace.location", "magenta");
        let mut hidden = 0usize;
        let flush_hidden = |text: &mut Text, hidden: &mut usize| {
            if *hidden > 0 {
                let label = if *hidden == 1 { "frame" } else { "frames" };
                text.append(
                    &format!("  … {hidden} library {label}\n"),
                    Some(dim.clone().into()),
                );
                *hidden = 0;
            }
        };
        for frame in &trace.frames {
            if frame.library && !self.show_library {
                hidden += 1;
                continue;
            }
            flush_hidden(text, &mut hidden);
            let base = if frame.library {
                Some(dim.clone())
            } else {
                None
            };
            let name = frame.function.as_deref().unwrap_or("<unknown>");
            let elided = frame.metadata.iter().find(|(key, _)| key == "elided");
            text.append("  ", None);
            match elided {
                Some((_, label)) => text.append(label, Some(dim.clone().into())),
                None => text.append(
                    name,
                    Some(base.clone().unwrap_or(function_style.clone()).into()),
                ),
            }
            text.append("\n", None);
            if let Some(path) = &frame.path {
                text.append("    at ", base.clone().map(Into::into));
                let location = self.linker.location(
                    path,
                    frame.line,
                    frame.column,
                    base.clone().unwrap_or(location_style.clone()),
                );
                *text = std::mem::take(text).append_text(&location);
                text.append("\n", None);
            }
            if let Some(source) = &frame.source {
                text.append(&format!("      {source}\n"), base.clone().map(Into::into));
            }
        }
        flush_hidden(text, &mut hidden);
        let error_style = theme_style(console, "stacktrace.error", "bold red");
        let kind = trace.kind.as_deref().unwrap_or("error");
        text.append(kind, Some(error_style.into()));
        if let Some(message) = &trace.message {
            text.append(": ", None);
            text.append(message, None);
        }
        if let Some(location) = &trace.location {
            if let Some(path) = &location.path {
                text.append("\n  at ", None);
                let linked =
                    self.linker
                        .location(path, location.line, location.column, location_style);
                *text = std::mem::take(text).append_text(&linked);
            }
        }
    }
}

impl Renderable for StackTraceView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut text = Text::new("");
        self.append_chain(console, &mut text);
        text.rich_render(console, options)
    }
}

impl Renderable for StackTrace {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.render_options().rich_render(console, options)
    }
}

//! One command run: what ran, what it printed, how it ended — and a view of it.
//!
//! Execution and rendering are separate. A [`CommandRecord`] is plain data
//! (argv, cwd, stdout and stderr lines in arrival order, exit status and
//! duration) that tests build by hand and [`CommandRunner`] fills from a real
//! [`std::process::Command`]. A [`CommandView`] renders a record:
//!
//! ```text
//! ✖ error $ cargo test  exit 101  4.2s
//!   … 38 lines hidden
//!   │ test parse::tests::rejects_empty ... FAILED
//!   ! error: test failed, to rerun pass `--lib`
//! error: `cargo test` exited with code 101
//! caused by: error: test failed, to rerun pass `--lib`
//! ```
//!
//! Output is folded to its last lines with a count of the hidden ones (all of
//! it on failure, by default); stderr lines carry a `!` gutter as well as the
//! `workflow.stderr` style so the stream reads without colour. A failed run
//! ends with a [`Diagnostic`]. While the command runs, the header shows a
//! spinner and the elapsed time, which suits a
//! [`LiveCoordinator`](crate::live::LiveCoordinator) region; under reduced
//! motion the spinner is a static `▶ running` marker.
//!
//! ```
//! use std::time::Duration;
//! use rich::Console;
//! use rich_ext::workflow::{CommandRecord, CommandStatus};
//!
//! let record = CommandRecord::new("cargo", ["build"])
//!     .stdout("Compiling demo v0.1.0")
//!     .stderr("warning: unused variable `x`")
//!     .status(CommandStatus::Exited(0))
//!     .duration(Duration::from_millis(1500));
//! let console = Console::builder().width(50).build();
//! assert_eq!(
//!     console.render_to_string(&record.view()),
//!     "✔ ok $ cargo build  1.5s\n  │ Compiling demo v0.1.0\n  ! warning: unused variable `x`",
//! );
//! ```

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use rich::{AnsiDecoder, Console, ConsoleOptions, Renderable, Segment, Text};

use super::{render_texts, span, style, Look, State};
use crate::a11y::{AccessibilityPolicy, SymbolSet};
use crate::cancel::CancelToken;
use crate::diagnostic::Diagnostic;
use crate::event::EventView;

/// Which output stream a line came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stream {
    /// Standard output.
    Stdout,
    /// Standard error.
    Stderr,
}

/// One line of output, without its line ending.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputLine {
    /// The stream it was written to.
    pub stream: Stream,
    /// The text, which may contain ANSI styling.
    pub text: String,
}

/// How a command ended, or that it has not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandStatus {
    /// Still running.
    Running,
    /// Exited with this code; `0` is success.
    Exited(i32),
    /// Killed by this signal (Unix).
    Signalled(i32),
    /// Stopped through a [`CancelToken`].
    Cancelled,
    /// Could not be started; the reason.
    FailedToStart(String),
}

impl CommandStatus {
    /// The workflow state: exit code 0 succeeded, cancellation is
    /// cancelled, every other ending failed.
    pub fn state(&self) -> State {
        match self {
            CommandStatus::Running => State::Running,
            CommandStatus::Exited(0) => State::Succeeded,
            CommandStatus::Cancelled => State::Cancelled,
            CommandStatus::Exited(_)
            | CommandStatus::Signalled(_)
            | CommandStatus::FailedToStart(_) => State::Failed,
        }
    }

    /// Whether the command exited with code 0.
    pub fn success(&self) -> bool {
        *self == CommandStatus::Exited(0)
    }

    /// The short detail after the marker: `exit 101`, `signal 9`,
    /// `failed to start`; empty for success, running and cancellation.
    pub fn detail(&self) -> String {
        match self {
            CommandStatus::Exited(0) | CommandStatus::Running | CommandStatus::Cancelled => {
                String::new()
            }
            CommandStatus::Exited(code) => format!("exit {code}"),
            CommandStatus::Signalled(signal) => format!("signal {signal}"),
            CommandStatus::FailedToStart(_) => "failed to start".into(),
        }
    }
}

/// A command run as plain data. See the [module docs](self).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandRecord {
    /// The program, as shown in the header.
    pub program: String,
    /// The arguments.
    pub args: Vec<String>,
    /// The working directory, when not inherited.
    pub cwd: Option<PathBuf>,
    /// Output lines from both streams in the order they arrived.
    pub lines: Vec<OutputLine>,
    /// How it ended.
    pub status: CommandStatus,
    /// How long it ran (so far, while running).
    pub duration: Duration,
}

impl CommandRecord {
    /// A running command with no output yet.
    pub fn new<I, S>(program: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        CommandRecord {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
            cwd: None,
            lines: Vec::new(),
            status: CommandStatus::Running,
            duration: Duration::ZERO,
        }
    }

    /// The program, arguments and directory of `command`, not yet run.
    pub fn from_command(command: &Command) -> Self {
        let lossy = |s: &OsStr| s.to_string_lossy().into_owned();
        let mut record =
            CommandRecord::new(lossy(command.get_program()), command.get_args().map(lossy));
        record.cwd = command.get_current_dir().map(PathBuf::from);
        record
    }

    /// Set the working directory shown under the header.
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Append stdout lines (`text` is split on line endings).
    pub fn stdout(mut self, text: &str) -> Self {
        self.push(Stream::Stdout, text);
        self
    }

    /// Append stderr lines (`text` is split on line endings).
    pub fn stderr(mut self, text: &str) -> Self {
        self.push(Stream::Stderr, text);
        self
    }

    /// Set how the command ended.
    pub fn status(mut self, status: CommandStatus) -> Self {
        self.status = status;
        self
    }

    /// Set how long it ran.
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Append `text` to `stream`, one [`OutputLine`] per line.
    pub fn push(&mut self, stream: Stream, text: &str) {
        for line in text.lines() {
            self.lines.push(OutputLine {
                stream,
                text: line.to_string(),
            });
        }
    }

    /// The command line as a shell would show it; arguments with spaces or
    /// shell characters are single-quoted.
    pub fn command_line(&self) -> String {
        std::iter::once(&self.program)
            .chain(&self.args)
            .map(|word| quote(word))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Every line written to `stream`, joined with `\n`.
    pub fn output(&self, stream: Stream) -> String {
        self.lines
            .iter()
            .filter(|line| line.stream == stream)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The workflow state of [`status`](Self::status).
    pub fn state(&self) -> State {
        self.status.state()
    }

    /// An error diagnostic for a failed run — `` `cargo test` exited with code
    /// 101`` — with the last non-empty stderr line as its cause; `None`
    /// unless the run [failed](State::Failed).
    pub fn diagnostic(&self) -> Option<Diagnostic> {
        let command = format!("`{}`", self.command_line());
        let message = match &self.status {
            CommandStatus::Exited(0) | CommandStatus::Running | CommandStatus::Cancelled => {
                return None
            }
            CommandStatus::Exited(code) => format!("{command} exited with code {code}"),
            CommandStatus::Signalled(signal) => {
                format!("{command} was terminated by signal {signal}")
            }
            CommandStatus::FailedToStart(reason) => format!("{command} could not start: {reason}"),
        };
        let mut diagnostic = Diagnostic::error(message);
        let last = self
            .lines
            .iter()
            .rev()
            .filter(|line| line.stream == Stream::Stderr)
            .map(|line| AnsiDecoder::new().decode_line(&line.text))
            .find(|text| !text.plain().trim().is_empty());
        if let Some(text) = last {
            diagnostic = diagnostic.cause(text.plain().trim());
        }
        Some(diagnostic)
    }

    /// A view of this record with the default options.
    pub fn view(&self) -> CommandView<'_> {
        CommandView::new(self)
    }
}

impl Renderable for CommandRecord {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.view().rich_render(console, options)
    }
}

fn quote(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_alphanumeric() || "_-./=:,+@%^".contains(c));
    if plain {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// How a [`CommandRecord`] is rendered. See the [module docs](self).
#[derive(Clone, Debug)]
pub struct CommandView<'a> {
    record: &'a CommandRecord,
    tail: Option<usize>,
    full_on_failure: bool,
    show_cwd: bool,
    show_diagnostic: bool,
    help: Vec<String>,
    look: Look,
}

impl<'a> CommandView<'a> {
    /// The last 10 lines (all of them on failure), the working directory, a
    /// diagnostic on failure, Unicode markers and an animated spinner.
    pub fn new(record: &'a CommandRecord) -> Self {
        CommandView {
            record,
            tail: Some(10),
            full_on_failure: true,
            show_cwd: true,
            show_diagnostic: true,
            help: Vec::new(),
            look: Look::default(),
        }
    }

    /// Show only the last `lines` lines of output (0 hides output).
    pub fn tail(mut self, lines: usize) -> Self {
        self.tail = Some(lines);
        self
    }

    /// Show every output line.
    pub fn show_all(mut self) -> Self {
        self.tail = None;
        self
    }

    /// Whether a failed run shows all of its output (default true).
    pub fn full_on_failure(mut self, full: bool) -> Self {
        self.full_on_failure = full;
        self
    }

    /// Whether the working directory is shown (default true).
    pub fn show_cwd(mut self, show: bool) -> Self {
        self.show_cwd = show;
        self
    }

    /// Whether a failed run ends with a diagnostic (default true).
    pub fn show_diagnostic(mut self, show: bool) -> Self {
        self.show_diagnostic = show;
        self
    }

    /// Add a `help:` line to the failure diagnostic.
    pub fn help(mut self, message: impl Into<String>) -> Self {
        self.help.push(message.into());
        self
    }

    /// Mark status with `set`: `Ascii` and `Words` also switch `…` and `µs`
    /// to ASCII and stop the spinner.
    pub fn symbols(mut self, set: SymbolSet) -> Self {
        self.look.symbols = set;
        self
    }

    /// Whether a running command shows a spinner (default true); `false`
    /// shows the static running marker.
    pub fn animate(mut self, animate: bool) -> Self {
        self.look.animate = animate;
        self
    }

    /// Follow `policy`: its status symbols, and no spinner under reduced
    /// motion, no animation or a screen reader.
    pub fn policy(mut self, policy: &AccessibilityPolicy) -> Self {
        self.look = Look::from_policy(policy);
        self
    }

    fn header(&self, console: &Console) -> Text {
        let record = self.record;
        let state = record.state();
        let mut text = Text::new("");
        text.append(
            &self.look.marker(state, record.duration),
            span(console, state.style_key()),
        );
        text.append(" ", None);
        text.append("$ ", span(console, "workflow.prompt"));
        text.append(&record.command_line(), span(console, "workflow.command"));
        let detail = record.status.detail();
        if !detail.is_empty() {
            text.append("  ", None);
            text.append(&detail, span(console, state.style_key()));
        }
        if !matches!(record.status, CommandStatus::FailedToStart(_)) {
            text.append("  ", None);
            text.append(
                &self.look.duration(record.duration),
                span(console, "workflow.duration"),
            );
        }
        text
    }

    fn output(&self, console: &Console) -> Vec<Text> {
        let lines = &self.record.lines;
        let full = self.full_on_failure && self.record.state() == State::Failed;
        let shown = match self.tail {
            Some(tail) if !full => tail.min(lines.len()),
            _ => lines.len(),
        };
        let hidden = lines.len() - shown;
        let mut out = Vec::new();
        if hidden > 0 {
            let noun = if hidden == 1 { "line" } else { "lines" };
            out.push(Text::styled(
                format!("  {} {hidden} {noun} hidden", self.look.ellipsis()),
                style(console, "workflow.hidden"),
            ));
        }
        // Decode every line so styling carried across lines stays right.
        let (mut stdout, mut stderr) = (AnsiDecoder::new(), AnsiDecoder::new());
        let bar = if self.look.ascii() { "|" } else { "│" };
        for (index, line) in lines.iter().enumerate() {
            let (decoder, gutter, gutter_key, key) = match line.stream {
                Stream::Stdout => (&mut stdout, bar, "workflow.gutter", "workflow.stdout"),
                Stream::Stderr => (&mut stderr, "!", "workflow.stderr", "workflow.stderr"),
            };
            let decoded = decoder.decode_line(&line.text);
            if index < hidden {
                continue;
            }
            let mut body = decoded;
            let base = style(console, key);
            if !base.is_null() {
                body.set_base_style(base);
            }
            let mut text = Text::new("  ");
            text.append(gutter, span(console, gutter_key));
            text.append(" ", None);
            out.push(text.append_text(&body));
        }
        out
    }
}

impl Renderable for CommandView<'_> {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut lines = vec![self.header(console)];
        if self.show_cwd {
            if let Some(cwd) = &self.record.cwd {
                lines.push(Text::styled(
                    format!("  in {}", cwd.display()),
                    style(console, "workflow.cwd"),
                ));
            }
        }
        lines.extend(self.output(console));
        let mut segments = render_texts(console, options, &lines);
        let diagnostic = self
            .record
            .diagnostic()
            .filter(|_| self.show_diagnostic)
            .map(|diagnostic| {
                let expanded = !self.help.is_empty();
                self.help
                    .iter()
                    .fold(diagnostic, |d, help| d.help(help.clone()))
                    .view(if expanded {
                        EventView::Expanded
                    } else {
                        EventView::Compact
                    })
            });
        if let Some(diagnostic) = diagnostic {
            if !segments.is_empty() {
                segments.push(Segment::line());
            }
            segments.extend(diagnostic.rich_render(console, options));
        }
        segments
    }
}

/// Runs a [`Command`] into a [`CommandRecord`], capturing both streams.
///
/// Stdout and stderr are read on two threads and merged in arrival order
/// (the order across streams is as close as the pipes allow, not exact).
/// Stdin is closed. [`on_update`](Self::on_update) sees the record after
/// every line and every [`tick`](Self::tick), so a live view can redraw the
/// spinner and elapsed time; a [`CancelToken`] kills the child.
///
/// ```
/// use std::process::Command;
/// use rich_ext::workflow::{CommandRunner, CommandStatus};
///
/// let record = CommandRunner::new().run(&mut Command::new("rustc").arg("--version"));
/// assert_eq!(record.status, CommandStatus::Exited(0));
/// assert!(record.lines[0].text.starts_with("rustc "));
/// ```
pub struct CommandRunner<'a> {
    cancel: Option<CancelToken>,
    tick: Duration,
    on_update: Option<UpdateFn<'a>>,
}

type UpdateFn<'a> = Box<dyn FnMut(&CommandRecord) + 'a>;

impl Default for CommandRunner<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CommandRunner<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CommandRunner")
            .field("cancel", &self.cancel)
            .field("tick", &self.tick)
            .finish_non_exhaustive()
    }
}

/// How long to keep reading after the child exits while its pipes stay open
/// (a background grandchild can hold them forever).
const GRACE: Duration = Duration::from_secs(1);

impl<'a> CommandRunner<'a> {
    /// A runner that ticks every 100 ms and cannot be cancelled.
    pub fn new() -> Self {
        CommandRunner {
            cancel: None,
            tick: Duration::from_millis(100),
            on_update: None,
        }
    }

    /// Kill the child when `token` is cancelled; the record ends
    /// [`Cancelled`](CommandStatus::Cancelled).
    pub fn cancel(mut self, token: CancelToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// How often `on_update` runs while no output arrives, and how often
    /// cancellation is checked.
    pub fn tick(mut self, every: Duration) -> Self {
        self.tick = every.max(Duration::from_millis(1));
        self
    }

    /// Call `update` with the record so far after each line and each tick.
    pub fn on_update(mut self, update: impl FnMut(&CommandRecord) + 'a) -> Self {
        self.on_update = Some(Box::new(update));
        self
    }

    /// Run `command` to completion (or cancellation). A command that cannot
    /// start returns a record with
    /// [`FailedToStart`](CommandStatus::FailedToStart).
    pub fn run(mut self, command: &mut Command) -> CommandRecord {
        let mut record = CommandRecord::from_command(command);
        let start = Instant::now();
        let mut child = match command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                record.status = CommandStatus::FailedToStart(error.to_string());
                return record;
            }
        };
        let (tx, rx) = mpsc::channel();
        if let Some(out) = child.stdout.take() {
            pump(out, Stream::Stdout, tx.clone());
        }
        if let Some(err) = child.stderr.take() {
            pump(err, Stream::Stderr, tx);
        } else {
            drop(tx);
        }

        let mut exited: Option<(ExitStatus, Instant)> = None;
        let mut killed = false;
        loop {
            match rx.recv_timeout(self.tick) {
                Ok((stream, bytes)) => {
                    let text = String::from_utf8_lossy(&bytes);
                    let text = text.strip_suffix('\n').unwrap_or(&text);
                    let text = text.strip_suffix('\r').unwrap_or(text);
                    record.lines.push(OutputLine {
                        stream,
                        text: text.to_string(),
                    });
                    match &mut exited {
                        Some((_, quiet)) => *quiet = Instant::now(),
                        None => record.duration = start.elapsed(),
                    }
                    self.notify(&record);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if exited.is_none() {
                        record.duration = start.elapsed();
                        self.notify(&record);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if !killed && self.cancel.as_ref().is_some_and(CancelToken::is_cancelled) {
                let _ = child.kill();
                killed = true;
            }
            match exited {
                None => {
                    if let Ok(Some(status)) = child.try_wait() {
                        record.duration = start.elapsed();
                        exited = Some((status, Instant::now()));
                    }
                }
                Some((_, quiet)) if quiet.elapsed() > GRACE => break,
                Some(_) => {}
            }
        }
        let status = match exited {
            Some((status, _)) => Ok(status),
            None => {
                let status = child.wait();
                record.duration = start.elapsed();
                status
            }
        };
        record.status = match status {
            _ if killed => CommandStatus::Cancelled,
            Ok(status) => exit_status(status),
            Err(error) => CommandStatus::FailedToStart(error.to_string()),
        };
        record
    }

    fn notify(&mut self, record: &CommandRecord) {
        if let Some(update) = &mut self.on_update {
            update(record);
        }
    }
}

fn pump(reader: impl Read + Send + 'static, stream: Stream, tx: mpsc::Sender<(Stream, Vec<u8>)>) {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        loop {
            let mut line = Vec::new();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if tx.send((stream, line)).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

fn exit_status(status: ExitStatus) -> CommandStatus {
    if let Some(code) = status.code() {
        return CommandStatus::Exited(code);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return CommandStatus::Signalled(signal);
        }
    }
    CommandStatus::Exited(-1)
}

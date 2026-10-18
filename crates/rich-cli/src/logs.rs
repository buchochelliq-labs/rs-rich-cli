//! `rich log`: filters, follow mode, groups and a live view (#724).
//!
//! Not upstream (`rich log` itself is this port's). A binary-boundary
//! convenience: the records are read, filtered, collapsed and grouped by
//! the public `rich_ext::log_stream` API, redacted by `rich_ext::redact`,
//! and shown live by `rich_interact::LogView`; this module routes the
//! flags, reads files and standard input (and follows a file), and writes
//! what is kept. With none of these options the output is what it was:
//! each JSON line in the presentation `--log-presentation` picks.

use std::fs::File;
use std::io::{self, BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use rich::{Console, Renderable, Text};
use rich_ext::event::EventView;
use rich_ext::log_stream::{
    Assembled, Assembler, Clock, Collapser, Filter, FilterError, GroupBy, GroupHeader, Groups,
    LevelFilter, Parsers, Predicate, Record, Repeat, Summary, TimeMode,
};
use rich_ext::redact::Redactor;

use super::{fail, fs_path, structured_log, success, viewers, Cli, ExitClass, ExitCode};

/// `--output`'s value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LogOutput {
    /// Rendered records (the default).
    #[default]
    Text,
    /// Each kept record as one JSON object per line.
    Jsonl,
}

/// The `--parser` names, as `rich log --help` lists them.
const PARSERS: &[&str] = &["auto", "logfmt", "level", "timestamp", "access", "text"];

/// `rich log`'s options. See `docs/cli.md`.
#[derive(Clone, Debug, Default)]
pub(crate) struct LogOptions {
    levels: Option<LevelFilter>,
    predicates: Vec<Predicate>,
    follow: bool,
    collapse: Option<bool>,
    group: Option<GroupBy>,
    fields: bool,
    parser: Option<String>,
    time: Option<TimeMode>,
    output: Option<LogOutput>,
    summary: Option<bool>,
    interactive: bool,
}

fn level_error(error: FilterError) -> String {
    format!("--level: {error}")
}

impl LogOptions {
    /// Consume one of these options; anything else stays with the main
    /// parser. `--group` with a number is left to `rich hex`.
    pub(crate) fn parse_option(
        &mut self,
        arg: &str,
        rest: &mut std::slice::Iter<'_, String>,
    ) -> Result<bool, String> {
        match arg {
            "--level" => {
                let value = rest
                    .next()
                    .ok_or("--level requires a LEVEL, such as error or warn+")?;
                let levels: LevelFilter = value.parse().map_err(level_error)?;
                self.levels = Some(match self.levels.take() {
                    Some(earlier) => earlier.or(levels),
                    None => levels,
                });
            }
            "--where" => {
                let value = rest
                    .next()
                    .ok_or("--where requires an EXPR, such as service=api")?;
                let predicate =
                    Predicate::parse(value).map_err(|error| format!("--where: {error}"))?;
                self.predicates.push(predicate);
            }
            "-f" | "--follow" => self.follow = true,
            "--collapse" => self.collapse = Some(true),
            "--no-collapse" => self.collapse = Some(false),
            "--group" => {
                let usage =
                    "--group requires trace, span or correlation (with `rich hex`, a number)";
                let value = rest.clone().next().ok_or(usage)?;
                if value.bytes().any(|b| b.is_ascii_digit()) && !value.contains(char::is_alphabetic)
                    || value.starts_with("0x")
                {
                    return Ok(false);
                }
                rest.next();
                self.group = Some(value.parse()?);
            }
            "--fields" => self.fields = true,
            "--parser" => {
                let value = rest
                    .next()
                    .ok_or("--parser requires auto, logfmt, level, timestamp, access or text")?;
                if !PARSERS.contains(&value.as_str()) && !Parsers::new().contains(value) {
                    return Err(format!(
                        "unknown --parser {value:?} (auto, logfmt, level, timestamp, access, text)"
                    ));
                }
                self.parser = Some(value.clone());
            }
            "--time" => {
                let value = rest
                    .next()
                    .ok_or("--time requires absolute, relative or none")?;
                self.time = Some(
                    value
                        .parse()
                        .map_err(|error: String| format!("--time: {error}"))?,
                );
            }
            "--output" => {
                let value = rest.next().ok_or("--output requires text or jsonl")?;
                self.output = Some(match value.as_str() {
                    "text" => LogOutput::Text,
                    "jsonl" => LogOutput::Jsonl,
                    other => {
                        return Err(format!("unknown --output {other:?} (text, jsonl)"));
                    }
                });
            }
            "--summary" => self.summary = Some(true),
            "--no-summary" => self.summary = Some(false),
            "-i" | "--interactive" => self.interactive = true,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Each option given, with the command it applies to, for the "only
    /// has an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [
            ("--level", self.levels.is_some()),
            ("--where", !self.predicates.is_empty()),
            ("--follow", self.follow),
            ("--collapse", self.collapse == Some(true)),
            ("--no-collapse", self.collapse == Some(false)),
            ("--group", self.group.is_some()),
            ("--fields", self.fields),
            ("--parser", self.parser.is_some()),
            ("--time", self.time.is_some()),
            ("--output", self.output.is_some()),
            ("--summary", self.summary == Some(true)),
            ("--no-summary", self.summary == Some(false)),
            ("--interactive", self.interactive),
        ]
        .into_iter()
        .filter(|(_, given)| *given)
        .map(|(flag, _)| (flag, &["log"][..]))
        .collect()
    }

    /// The combinations that cannot work, once the mode is `rich log`.
    pub(crate) fn validate(&self, resource: Option<&str>) -> Result<(), String> {
        if self.output == Some(LogOutput::Jsonl) {
            for (flag, given) in [
                ("--group", self.group.is_some()),
                ("--fields", self.fields),
                ("--time", self.time.is_some()),
                ("--collapse", self.collapse == Some(true)),
                ("--interactive", self.interactive),
            ] {
                if given {
                    return Err(format!("{flag} cannot be combined with --output jsonl"));
                }
            }
        }
        if self.interactive && self.group.is_some() {
            return Err("--group cannot be combined with --interactive".into());
        }
        if self.follow && resource.is_none_or(|resource| resource == "-") {
            return Err(
                "--follow needs a FILE to follow (standard input is read until it ends)".into(),
            );
        }
        Ok(())
    }

    /// Whether `--follow` was given.
    pub(crate) fn follows(&self) -> bool {
        self.follow
    }

    /// The parsers for lines that are not JSON.
    fn parsers(&self) -> Parsers {
        match self.parser.as_deref() {
            None | Some("auto") => Parsers::new(),
            Some("text") => Parsers::new().only(&["json"]),
            Some(name) => Parsers::new().only(&["json", name]),
        }
    }

    fn filter(&self) -> Filter {
        let mut filter = Filter::new();
        if let Some(levels) = &self.levels {
            filter = filter.levels(levels.clone());
        }
        for predicate in &self.predicates {
            filter = filter.predicate(predicate.clone());
        }
        filter
    }

    fn jsonl(&self) -> bool {
        self.output == Some(LogOutput::Jsonl)
    }

    /// Repeats fold with `--collapse`, and by default when following a
    /// file or in the live view, where a flood of them would push
    /// everything else off the screen. Never in JSON output.
    fn collapses(&self) -> bool {
        !self.jsonl() && self.collapse.unwrap_or(self.follow || self.interactive)
    }

    /// A summary at the end with `--summary`, and by default with `--group`.
    fn summarises(&self) -> bool {
        self.summary
            .unwrap_or(self.group.is_some() && !self.jsonl())
    }
}

/// How often a followed file is checked for more.
const POLL: Duration = Duration::from_millis(100);

/// How long standard input may be quiet before the record being read is
/// shown (a live pipe, `tail -f app.log | rich log -`).
const QUIET: Duration = Duration::from_millis(150);

/// Set by Ctrl+C while following.
static STOP: AtomicBool = AtomicBool::new(false);

/// `rich log`: read, filter and write the records of `cli`'s resource.
pub(super) fn run(cli: &Cli, console: &Console) -> ExitCode {
    let options = &cli.logs;
    let redactor = match viewers::capture_redactor(&cli.viewers, cli.data.redact()) {
        Ok(redactor) => redactor.map(|redactor| redactor.preserve_width(false)),
        Err(message) => return fail(cli, ExitClass::Usage, message),
    };
    let path = cli.resource.as_deref().filter(|resource| *resource != "-");
    if let Some(path) = path {
        let file = fs_path(path);
        if file.is_dir() {
            return fail(
                cli,
                ExitClass::Input,
                format!("cannot read {path}: is a directory, not a file"),
            );
        }
        if let Err(error) = File::open(&file) {
            return fail(
                cli,
                ExitClass::Input,
                format!("cannot read {path}: {error}"),
            );
        }
    }
    if viewers::redact_preview(&cli.viewers) {
        return preview(cli, console, path, redactor.as_ref());
    }
    if options.interactive {
        if let Some(code) = interactive(cli, console, redactor.clone()) {
            return code;
        }
    }
    let mut stream = Stream::new(cli, console, redactor);
    let read = match path {
        Some(path) if options.follow => return follow(cli, path, stream),
        Some(path) => read_file(&fs_path(path), &mut stream),
        None => read_stdin(|event| match event {
            Tailed::Line(line) => stream.line(line),
            Tailed::Quiet | Tailed::Notice(_) => stream.idle(),
        }),
    };
    finish(cli, read.and_then(|()| stream.finish()))
}

/// `--redact-preview`: every line of the log read once (to the end, so not
/// with `--follow` or `--interactive`), and what the redactor would mask in
/// it shown instead of the records (#722). Lines are previewed as
/// `--redact` masks a line it does not parse; a parsed record's
/// secret-named fields are the `key-value` matches.
fn preview(
    cli: &Cli,
    console: &Console,
    path: Option<&str>,
    redactor: Option<&Redactor>,
) -> ExitCode {
    let Some(redactor) = redactor else {
        return fail(
            cli,
            ExitClass::Usage,
            "--redact-preview needs --redact or --redact-pattern",
        );
    };
    if cli.logs.follow || cli.logs.interactive {
        return fail(
            cli,
            ExitClass::Usage,
            "--redact-preview reads the log once; it cannot be combined with --follow or \
             --interactive",
        );
    }
    let mut bytes = Vec::new();
    let read = match path {
        Some(path) => File::open(fs_path(path)).and_then(|mut file| file.read_to_end(&mut bytes)),
        None => io::stdin().lock().read_to_end(&mut bytes),
    };
    if let Err(error) = read {
        return fail(
            cli,
            ExitClass::Input,
            format!("cannot read the log: {error}"),
        );
    }
    let preview = redactor
        .preview(&String::from_utf8_lossy(&bytes))
        .title("log");
    super::redact_preview::emit(cli, console, vec![("log", preview)], &[])
}

/// The exit for how the stream ended: a closed stdout is success, as a
/// consumer such as `head` stops reading.
fn finish(cli: &Cli, result: io::Result<()>) -> ExitCode {
    match result {
        Ok(()) => success(cli),
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => success(cli),
        Err(error) if error.kind() == io::ErrorKind::Other => fail(
            cli,
            ExitClass::Input,
            format!("cannot read the log: {error}"),
        ),
        Err(error) => fail(
            cli,
            ExitClass::Input,
            format!("could not write the log: {error}"),
        ),
    }
}

/// A read error, told apart from a write error (both are `io::Error`s).
fn read_error(error: io::Error) -> io::Error {
    io::Error::other(error)
}

/// Each line of `reader` (lossy UTF-8, without its line break) to `each`.
fn lines(mut reader: impl BufRead, mut each: impl FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        if reader.read_until(b'\n', &mut buffer).map_err(read_error)? == 0 {
            return Ok(());
        }
        each(&String::from_utf8_lossy(&buffer))?;
    }
}

fn read_file(path: &Path, stream: &mut Stream<'_>) -> io::Result<()> {
    let file = File::open(path).map_err(read_error)?;
    lines(io::BufReader::with_capacity(1 << 16, file), |line| {
        stream.line(line)
    })
}

/// Standard input, read on a thread so that a quiet pipe says so
/// ([`Tailed::Quiet`]), and the record it is in the middle of is shown
/// instead of held back.
fn read_stdin(mut each: impl FnMut(Tailed<'_>) -> io::Result<()>) -> io::Result<()> {
    use std::io::IsTerminal;
    if io::stdin().is_terminal() {
        let eof = if cfg!(windows) {
            "Ctrl-Z then Enter"
        } else {
            "Ctrl-D"
        };
        eprintln!("rich: reading stdin; finish input with {eof}");
    }
    let (sender, receiver) = mpsc::sync_channel::<io::Result<Vec<String>>>(64);
    std::thread::spawn(move || {
        let stdin = io::stdin();
        let mut reader = io::BufReader::with_capacity(1 << 16, stdin.lock());
        let mut batch = Vec::new();
        let mut buffer = Vec::new();
        loop {
            buffer.clear();
            match reader.read_until(b'\n', &mut buffer) {
                Ok(0) => break,
                Ok(_) => batch.push(String::from_utf8_lossy(&buffer).into_owned()),
                Err(error) => {
                    let _ = sender.send(Err(error));
                    return;
                }
            }
            // Send what was read whenever the next read may wait.
            if (reader.buffer().is_empty() || batch.len() >= 1024)
                && sender.send(Ok(std::mem::take(&mut batch))).is_err()
            {
                return;
            }
        }
        if !batch.is_empty() {
            let _ = sender.send(Ok(batch));
        }
    });
    loop {
        match receiver.recv_timeout(QUIET) {
            Ok(Ok(batch)) => {
                for line in &batch {
                    each(Tailed::Line(line))?;
                }
            }
            Ok(Err(error)) => return Err(read_error(error)),
            Err(mpsc::RecvTimeoutError::Timeout) => each(Tailed::Quiet)?,
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// Follow `path` until Ctrl+C: what it holds, then what is appended to it,
/// starting over when it is truncated or replaced (log rotation).
fn follow(cli: &Cli, path: &str, mut stream: Stream<'_>) -> ExitCode {
    STOP.store(false, Ordering::SeqCst);
    let _ = ctrlc::set_handler(|| STOP.store(true, Ordering::SeqCst));
    let result = tail(
        path,
        |event| match event {
            Tailed::Line(line) => stream.line(line),
            Tailed::Quiet => stream.idle(),
            Tailed::Notice(notice) => {
                stream.idle()?;
                eprintln!("rich: {}", crate::controls::shown(&notice));
                stream.moved_on();
                Ok(())
            }
        },
        || STOP.load(Ordering::SeqCst),
    );
    match result.and_then(|()| stream.finish()) {
        Ok(()) => ExitCode::from(130),
        Err(error) => finish(cli, Err(error)),
    }
}

/// What following a file gives.
enum Tailed<'a> {
    Line(&'a str),
    /// Nothing new since the last look.
    Quiet,
    /// The file was truncated or replaced.
    Notice(String),
}

/// A file being followed: its handle, where reading got to, and the start
/// of a line not finished yet.
struct Tail {
    path: PathBuf,
    file: File,
    identity: Option<same_file::Handle>,
    position: u64,
    partial: Vec<u8>,
}

/// Open `path` for following: on Windows with every sharing flag, so the
/// program writing the log can still rename it (rotate it) while it is open
/// here.
fn open_shared(path: &Path) -> io::Result<File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE.
        options.share_mode(0x1 | 0x2 | 0x4);
    }
    options.open(path)
}

impl Tail {
    fn open(path: PathBuf) -> io::Result<Tail> {
        let file = open_shared(&path)?;
        let identity = same_file::Handle::from_file(file.try_clone()?).ok();
        Ok(Tail {
            path,
            file,
            identity,
            position: 0,
            partial: Vec::new(),
        })
    }

    /// Read what is there, giving each finished line; `true` when anything
    /// was read.
    fn read(&mut self, each: &mut dyn FnMut(Tailed<'_>) -> io::Result<()>) -> io::Result<bool> {
        let mut chunk = vec![0u8; 1 << 16];
        let mut any = false;
        loop {
            let read = self.file.read(&mut chunk).map_err(read_error)?;
            if read == 0 {
                return Ok(any);
            }
            any = true;
            self.position += read as u64;
            self.partial.extend_from_slice(&chunk[..read]);
            let mut start = 0;
            while let Some(end) = self.partial[start..].iter().position(|b| *b == b'\n') {
                let line = String::from_utf8_lossy(&self.partial[start..start + end]).into_owned();
                each(Tailed::Line(&line))?;
                start += end + 1;
            }
            self.partial.drain(..start);
        }
    }

    /// The unfinished last line, as a line: the file ended without one.
    fn flush(&mut self, each: &mut dyn FnMut(Tailed<'_>) -> io::Result<()>) -> io::Result<()> {
        if self.partial.is_empty() {
            return Ok(());
        }
        let line = String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned();
        each(Tailed::Line(&line))
    }

    /// Notice a rotation: the path now names another file (read on from
    /// its start, once the old one is read to its end), or this one got
    /// shorter (read it again from the start). A path that is gone for a
    /// moment, between a rename and a create, is waited for.
    fn check(&mut self, each: &mut dyn FnMut(Tailed<'_>) -> io::Result<()>) -> io::Result<()> {
        let Ok(current) = same_file::Handle::from_path(&self.path) else {
            return Ok(());
        };
        let shown = self.path.display().to_string();
        if self
            .identity
            .as_ref()
            .is_some_and(|identity| *identity != current)
        {
            self.read(each)?;
            self.flush(each)?;
            let Ok(file) = open_shared(&self.path) else {
                return Ok(());
            };
            self.file = file;
            self.identity = Some(current);
            self.position = 0;
            each(Tailed::Notice(format!(
                "{shown} was replaced; following the new file"
            )))?;
            return Ok(());
        }
        let length = self.file.metadata().map_err(read_error)?.len();
        if length < self.position {
            self.flush(each)?;
            self.file.seek(SeekFrom::Start(0)).map_err(read_error)?;
            self.position = 0;
            each(Tailed::Notice(format!(
                "{shown} was truncated; reading it from the start"
            )))?;
        }
        Ok(())
    }
}

/// Follow `path`, giving what happens to `each`, until `stop` says so.
fn tail(
    path: &str,
    mut each: impl FnMut(Tailed<'_>) -> io::Result<()>,
    stop: impl Fn() -> bool,
) -> io::Result<()> {
    let mut tail = Tail::open(fs_path(path)).map_err(read_error)?;
    loop {
        if !tail.read(&mut each)? {
            each(Tailed::Quiet)?;
        }
        // Sleep in slices, so Ctrl+C is answered at once.
        let mut slept = Duration::ZERO;
        while slept < POLL && !stop() {
            let slice = Duration::from_millis(20);
            std::thread::sleep(slice);
            slept += slice;
        }
        if stop() {
            tail.read(&mut each)?;
            return tail.flush(&mut each);
        }
        tail.check(&mut each)?;
    }
}

/// The live view, on a terminal; `None` without one, so the records stream
/// as they would without `--interactive`.
#[cfg(feature = "interact")]
fn interactive(cli: &Cli, console: &Console, redactor: Option<Redactor>) -> Option<ExitCode> {
    use rich_interact::{
        LogEvent, LogView, LoopOptions, Outcome, Output, Policy, RunOptions, SessionOptions,
    };
    let run_options = RunOptions {
        policy: Policy {
            tty_keys: true,
            ..Policy::default()
        },
        session: SessionOptions {
            alternate_screen: true,
            ..SessionOptions::default()
        },
        paint: LoopOptions {
            no_color: cli.no_color || LoopOptions::default().no_color,
            ..LoopOptions::default()
        },
    };
    run_options.policy.detect_for(Output::Stdout).ok()?;
    let path = cli.resource.clone().filter(|resource| resource != "-");
    // Records typed at the terminal the keys come from: no view.
    if path.is_none() && std::io::IsTerminal::is_terminal(&io::stdin()) {
        return None;
    }
    let options = &cli.logs;
    let (sender, receiver) = mpsc::sync_channel::<LogEvent>(4096);
    let mut reader = Reader {
        assembler: Assembler::new(options.parsers()),
        filter: options.filter(),
        redactor,
        sanitize: cli.sanitize,
        kept: false,
        summary: options.summarises().then(|| Summary::new(None)),
    };
    let from_stdin = path.is_none();
    let follow = options.follow;
    let done = std::sync::Arc::new(AtomicBool::new(false));
    let finished = std::sync::Arc::clone(&done);
    let worker = std::thread::spawn(move || {
        let send = |event: LogEvent| {
            sender
                .send(event)
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "the view closed"))
        };
        let mut each = |event: Tailed<'_>| match event {
            Tailed::Line(line) => reader.line(line, &send),
            // Quiet: show the record being read.
            Tailed::Quiet | Tailed::Notice(_) => reader.flush(&send),
        };
        let result = match (&path, follow) {
            (Some(path), true) => tail(path, &mut each, || finished.load(Ordering::SeqCst)),
            (Some(path), false) => File::open(fs_path(path))
                .map_err(read_error)
                .and_then(|file| lines(io::BufReader::new(file), |line| each(Tailed::Line(line)))),
            (None, _) => read_stdin(&mut each),
        };
        let _ = reader.flush(&send);
        (result, reader.summary)
    });
    let view = LogView::new()
        .source(receiver)
        .collapse(options.collapses())
        .time(options.time.unwrap_or_default())
        .expand_all(options.fields);
    let outcome = rich_interact::run(view, &run_options);
    done.store(true, Ordering::SeqCst);
    let code = match outcome {
        Ok(Outcome::Interrupted) => ExitCode::from(130),
        Ok(_) => success(cli),
        Err(error) => fail(cli, ExitClass::Input, error.to_string()),
    };
    // A worker reading standard input may wait on it for good: it is
    // joined only once it has finished. One reading or following a file
    // stops on its own (the view is gone, or `done` is set).
    if !from_stdin || worker.is_finished() {
        if let Ok((_, Some(summary))) = worker.join() {
            console.print(&summary);
        }
    }
    Some(code)
}

#[cfg(not(feature = "interact"))]
fn interactive(cli: &Cli, _: &Console, _: Option<Redactor>) -> Option<ExitCode> {
    Some(fail(
        cli,
        ExitClass::Usage,
        "--interactive needs a build with the interact feature",
    ))
}

/// The live view's reader: records parsed, kept or not, and sent on.
#[cfg(feature = "interact")]
struct Reader {
    assembler: Assembler,
    filter: Filter,
    redactor: Option<Redactor>,
    sanitize: bool,
    kept: bool,
    summary: Option<Summary>,
}

#[cfg(feature = "interact")]
impl Reader {
    fn line(
        &mut self,
        line: &str,
        send: &dyn Fn(rich_interact::LogEvent) -> io::Result<()>,
    ) -> io::Result<()> {
        match self.assembler.push(line) {
            Some(Assembled::Record(record)) => self.record(record, send),
            Some(Assembled::Continuation(line)) if self.kept => {
                let line = clean_line(&line, self.sanitize, self.redactor.as_ref());
                send(rich_interact::LogEvent::Continuation(line))
            }
            _ => Ok(()),
        }
    }

    fn record(
        &mut self,
        mut record: Record,
        send: &dyn Fn(rich_interact::LogEvent) -> io::Result<()>,
    ) -> io::Result<()> {
        if let Some(summary) = &mut self.summary {
            summary.read(1);
        }
        self.kept = keep(
            &mut record,
            &self.filter,
            self.sanitize,
            self.redactor.as_ref(),
        );
        if !self.kept {
            return Ok(());
        }
        if let Some(summary) = &mut self.summary {
            summary.shown(&record);
        }
        send(rich_interact::LogEvent::Record(record))
    }

    fn flush(
        &mut self,
        send: &dyn Fn(rich_interact::LogEvent) -> io::Result<()>,
    ) -> io::Result<()> {
        match self.assembler.flush() {
            Some(record) => self.record(record, send),
            None => Ok(()),
        }
    }
}

/// Whether `record` passes `filter`, sanitized first (so a filter sees
/// what is shown) and redacted after (so a filter can still find what the
/// mask hides).
fn keep(record: &mut Record, filter: &Filter, sanitize: bool, redactor: Option<&Redactor>) -> bool {
    if sanitize {
        record.sanitize();
    }
    if !filter.matches(record) {
        return false;
    }
    if let Some(redactor) = redactor {
        record.redact(redactor);
    }
    true
}

/// A continuation line as a record's would be: sanitized, then redacted.
fn clean_line(line: &str, sanitize: bool, redactor: Option<&Redactor>) -> String {
    let line = if sanitize {
        rich_ext::sanitize_terminal_controls(line)
    } else {
        line.to_string()
    };
    match redactor {
        Some(redactor) => redactor.redact_str(&line),
        None => line,
    }
}

/// The records of one run of `rich log`, read line by line and written as
/// they are kept (or, grouped, at the end).
struct Stream<'a> {
    console: &'a Console,
    out: io::BufWriter<io::Stdout>,
    assembler: Assembler,
    filter: Filter,
    redactor: Option<Redactor>,
    sanitize: bool,
    rich: bool,
    jsonl: bool,
    fields: bool,
    clock: Clock,
    collapser: Option<Collapser>,
    /// The records gathered for `--group`, written at the end.
    groups: Option<Groups>,
    /// Following with `--group`: the id of the last record written, its
    /// header written when it changes.
    live_group: Option<(GroupBy, Option<Option<String>>)>,
    summary: Option<Summary>,
    /// Whether the last record was written, so a line that comes after it
    /// is written under it.
    kept: bool,
    /// On a terminal, following: the count of the repeat run written last,
    /// rewritten in place as it grows.
    live_repeat: Option<usize>,
    terminal: bool,
}

impl<'a> Stream<'a> {
    fn new(cli: &'a Cli, console: &'a Console, redactor: Option<Redactor>) -> Stream<'a> {
        use std::io::IsTerminal;
        let options = &cli.logs;
        let group = options.group.filter(|_| !options.jsonl());
        Stream {
            console,
            out: io::BufWriter::with_capacity(1 << 16, io::stdout()),
            assembler: Assembler::new(options.parsers()),
            filter: options.filter(),
            redactor,
            sanitize: cli.sanitize,
            rich: cli.log_presentation == "rich",
            jsonl: options.jsonl(),
            fields: options.fields,
            clock: Clock::new(options.time.unwrap_or_default()),
            collapser: options.collapses().then(Collapser::new),
            groups: group.filter(|_| !options.follow).map(Groups::new),
            live_group: group.filter(|_| options.follow).map(|by| (by, None)),
            summary: options.summarises().then(|| Summary::new(group)),
            kept: false,
            live_repeat: None,
            terminal: io::stdout().is_terminal(),
        }
    }

    fn line(&mut self, line: &str) -> io::Result<()> {
        match self.assembler.push(line) {
            Some(Assembled::Record(record)) => self.record(record),
            Some(Assembled::Continuation(line)) => self.continuation(line),
            None => Ok(()),
        }
    }

    /// The input went quiet: show the record being read, and what a repeat
    /// run has come to.
    fn idle(&mut self) -> io::Result<()> {
        if let Some(record) = self.assembler.flush() {
            self.record(record)?;
        }
        if self.terminal {
            if let Some(repeat) = self.collapser.as_ref().and_then(Collapser::current) {
                if self.live_repeat != Some(repeat.count) {
                    if self.live_repeat.is_some() {
                        // Up one line and clear it: the count grows in place.
                        self.out.write_all(b"\x1b[1A\x1b[2K")?;
                    }
                    self.live_repeat = Some(repeat.count);
                    self.repeat(&repeat, self.indent())?;
                }
            }
        }
        self.out.flush()
    }

    fn record(&mut self, mut record: Record) -> io::Result<()> {
        if let Some(summary) = &mut self.summary {
            summary.read(1);
        }
        self.kept = keep(
            &mut record,
            &self.filter,
            self.sanitize,
            self.redactor.as_ref(),
        );
        if !self.kept {
            return Ok(());
        }
        if let Some(summary) = &mut self.summary {
            summary.shown(&record);
        }
        if let Some(groups) = &mut self.groups {
            groups.push(record);
            return Ok(());
        }
        if let Some(collapser) = &mut self.collapser {
            let push = collapser.push(&record);
            if let Some(repeat) = push.ended {
                self.end_repeat(&repeat)?;
            }
            if !push.show {
                if let Some(summary) = &mut self.summary {
                    summary.folded(1);
                }
                // Its continuation lines go with it.
                self.kept = false;
                return Ok(());
            }
            self.live_repeat = None;
        }
        if let Some((by, last)) = &mut self.live_group {
            let key = by.key_of(&record);
            if last.as_ref() != Some(&key) {
                let header = GroupHeader::streaming(*by, key.clone());
                *last = Some(key);
                let text = header.text(self.console);
                self.write_text(&text, 0)?;
            }
        }
        let indent = self.indent();
        self.write_record(&record, indent)
    }

    /// The indent of a record: under a group header, two.
    fn indent(&self) -> usize {
        if self.live_group.is_some() {
            2
        } else {
            0
        }
    }

    /// A repeat run ended: its count, unless it was written in place
    /// already with that count.
    fn end_repeat(&mut self, repeat: &Repeat) -> io::Result<()> {
        if self.live_repeat == Some(repeat.count) {
            self.live_repeat = None;
            return Ok(());
        }
        if self.live_repeat.take().is_some() {
            self.out.write_all(b"\x1b[1A\x1b[2K")?;
        }
        self.repeat(repeat, self.indent())
    }

    fn continuation(&mut self, line: String) -> io::Result<()> {
        if !self.kept {
            return Ok(());
        }
        let line = clean_line(&line, self.sanitize, self.redactor.as_ref());
        if let Some(groups) = &mut self.groups {
            groups.push_continuation(line);
            return Ok(());
        }
        if self.jsonl {
            let json = serde_json::json!({ "continuation": line });
            serde_json::to_writer(&mut self.out, &json)?;
            return self.out.write_all(b"\n");
        }
        let indent = self.indent() + 2;
        self.write_continuation(&line, indent)
    }

    /// The end of the input: what is left, the groups and the summary.
    fn finish(&mut self) -> io::Result<()> {
        if let Some(record) = self.assembler.flush() {
            self.record(record)?;
        }
        if let Some(repeat) = self.collapser.as_mut().and_then(Collapser::finish) {
            self.end_repeat(&repeat)?;
        }
        if let Some(groups) = self.groups.take() {
            let by = groups.by();
            for group in groups.into_groups() {
                let header = group.header(by).text(self.console);
                self.write_text(&header, 0)?;
                let mut collapser = self.collapser.as_ref().map(|_| Collapser::new());
                for record in &group.records {
                    if let Some(collapser) = &mut collapser {
                        let push = collapser.push(record);
                        if let Some(repeat) = push.ended {
                            self.repeat(&repeat, 2)?;
                        }
                        if !push.show {
                            if let Some(summary) = &mut self.summary {
                                summary.folded(1);
                            }
                            continue;
                        }
                    }
                    self.write_record(record, 2)?;
                }
                if let Some(repeat) = collapser.as_mut().and_then(Collapser::finish) {
                    self.repeat(&repeat, 2)?;
                }
            }
        }
        if let Some(summary) = self.summary.take() {
            if self.jsonl {
                // Standard output is the records: the summary goes to
                // standard error, as plain text.
                self.out.flush()?;
                let console = Console::builder().no_color(true).build();
                eprint!("{}", console.render_export(&summary));
            } else {
                self.out.write_all(b"\n")?;
                self.render(&summary, 0)?;
            }
        }
        self.out.flush()
    }

    fn write_record(&mut self, record: &Record, indent: usize) -> io::Result<()> {
        if self.jsonl {
            let mut json = structured_log::to_json(record);
            if !record.continuation.is_empty() {
                if let Some(object) = json.as_object_mut() {
                    object.insert("continuation".into(), record.continuation.clone().into());
                }
            }
            serde_json::to_writer(&mut self.out, &json)?;
            return self.out.write_all(b"\n");
        }
        // As written, the time is left to the presentation, unchanged.
        let time = match self.clock.mode() {
            TimeMode::Absolute => None,
            _ => Some(self.clock.show(record)),
        };
        let json = structured_log::to_json(record);
        if self.rich {
            let mut event = structured_log::event(&json);
            if let Some(time) = time {
                event.context.timestamp = time;
            }
            if self.fields {
                event = event.view(EventView::Expanded);
            }
            self.render(&event, indent)?;
        } else {
            let (line, rest) =
                plain_record(&json, time.as_ref().map(Option::as_deref), !self.fields);
            if !line.is_empty() {
                self.write_plain(&line, indent)?;
            }
            if self.fields {
                for (key, value) in rest {
                    let value = match &value {
                        serde_json::Value::Array(items) if !items.is_empty() => {
                            serde_json::to_string_pretty(&value)?
                        }
                        serde_json::Value::Object(entries) if !entries.is_empty() => {
                            serde_json::to_string_pretty(&value)?
                        }
                        other => serde_json::to_string(other)?,
                    };
                    for (index, part) in format!("{key}: {value}").split('\n').enumerate() {
                        let extra = if index == 0 { 0 } else { 2 };
                        self.write_plain(part, indent + 4 + extra)?;
                    }
                }
            }
        }
        for line in &record.continuation {
            self.write_continuation(line, indent + 2)?;
        }
        Ok(())
    }

    fn write_continuation(&mut self, line: &str, indent: usize) -> io::Result<()> {
        if self.rich {
            let style = themed(self.console, "log.continuation", "dim");
            self.render(&Text::styled(line, style), indent)
        } else {
            self.write_plain(line, indent)
        }
    }

    /// `× 12 (last 10:00:05)` under a repeated record.
    fn repeat(&mut self, repeat: &Repeat, indent: usize) -> io::Result<()> {
        let mut label = format!("× {}", repeat.count);
        let last = self
            .clock
            .show_at(repeat.last_time.as_deref(), repeat.last_instant);
        if let Some(last) = last {
            label.push_str(&format!(" (last {last})"));
        }
        let style = themed(self.console, "log.repeat", "dim");
        self.write_text(&Text::styled(label, style), indent + 2)
    }

    /// What was written is no longer the last line on the screen (a notice
    /// went to standard error): a repeat count is written anew.
    fn moved_on(&mut self) {
        self.live_repeat = None;
    }

    /// Plain text at `indent`, wrapped as the console wraps a `Text`.
    fn write_plain(&mut self, line: &str, indent: usize) -> io::Result<()> {
        let width = self.console.width().saturating_sub(indent);
        if width > 0 && wrap_simple(line, width, indent, &mut self.out)? {
            return Ok(());
        }
        self.render(&Text::new(line), indent)
    }

    /// A header or a summary line: without colour, its text wrapped as
    /// plain text is (faster, and the same); else rendered.
    fn write_text(&mut self, text: &Text, indent: usize) -> io::Result<()> {
        if self.console.color_system().is_none() && text.plain().len() < 4096 {
            let width = self.console.width().saturating_sub(indent);
            if width > 0 && wrap_simple(text.plain(), width, indent, &mut self.out)? {
                return Ok(());
            }
        }
        self.render(text, indent)
    }

    /// A renderable at `indent`: lines rendered that much narrower, each
    /// after that many spaces.
    fn render(&mut self, renderable: &dyn Renderable, indent: usize) -> io::Result<()> {
        if indent == 0 {
            let rendered = self.console.render_export(renderable);
            return self.out.write_all(rendered.as_bytes());
        }
        let width = self.console.width().saturating_sub(indent).max(1);
        let options = self.console.options().update_width(width);
        let pad = " ".repeat(indent);
        for line in self.console.render_lines(renderable, &options, false) {
            self.out.write_all(pad.as_bytes())?;
            self.out
                .write_all(self.console.segments_to_string(&line).as_bytes())?;
            self.out.write_all(b"\n")?;
        }
        Ok(())
    }
}

/// The console theme's `key`, or `fallback`.
fn themed(console: &Console, key: &str, fallback: &str) -> rich::Style {
    console
        .theme()
        .get(key)
        .cloned()
        .unwrap_or_else(|| rich::Style::parse(fallback).unwrap_or_default())
}

/// The plain presentation of a JSON record: its time, level (upper-cased
/// when it is text) and message, then the other fields as compact JSON
/// (`rest`), and those fields. `time` replaces the time shown: `Some(None)`
/// drops it, `None` keeps it as written.
///
/// Without `time` and with `rest`, this is the line `rich log` has always
/// printed.
fn plain_record(
    value: &serde_json::Value,
    time: Option<Option<&str>>,
    rest: bool,
) -> (String, serde_json::Map<String, serde_json::Value>) {
    let Some(object) = value.as_object() else {
        let json = serde_json::to_string(value).expect("JSON value serializes");
        return (json, serde_json::Map::new());
    };
    let take_value = |keys: &[&'static str]| {
        keys.iter().find_map(|key| {
            object.get(*key).map(|value| {
                let display = value.as_str().map(str::to_string).unwrap_or_else(|| {
                    serde_json::to_string(value).expect("JSON value serializes")
                });
                (*key, display, value.is_string())
            })
        })
    };
    let timestamp = take_value(&["timestamp", "time", "@timestamp"]);
    let level = take_value(&["level", "severity"]);
    let message = take_value(&["message", "msg"]);
    let mut others = serde_json::Map::new();
    for (key, value) in object {
        let consumed = [&timestamp, &level, &message].iter().any(|taken| {
            taken
                .as_ref()
                .is_some_and(|(consumed, _, _)| consumed == key)
        });
        if !consumed {
            others.insert(key.clone(), value.clone());
        }
    }
    let mut parts = Vec::new();
    match time {
        None => parts.extend(timestamp.as_ref().map(|(_, shown, _)| shown.clone())),
        Some(time) => parts.extend(time.map(str::to_string)),
    }
    if let Some((_, level, is_string)) = level {
        parts.push(if is_string {
            level.to_ascii_uppercase()
        } else {
            level
        });
    }
    if let Some((_, message, _)) = message {
        parts.push(message);
    }
    if rest && !others.is_empty() {
        parts.push(
            serde_json::to_string(&serde_json::Value::Object(others.clone()))
                .expect("JSON value serializes"),
        );
    }
    let line = if parts.is_empty() && time.is_none() {
        serde_json::to_string(value).expect("JSON value serializes")
    } else {
        parts.join(" ")
    };
    (line, others)
}

/// The characters of `line`, when each takes one cell and the only
/// whitespace is the space: what [`wrap_simple`] wraps. `Some(None)` for
/// printable ASCII, whose characters are its bytes.
fn simple_chars(line: &str) -> Option<Option<Vec<(usize, char)>>> {
    if line.is_empty() {
        return None;
    }
    if line.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        return Some(None);
    }
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let single = chars.iter().all(|&(_, c)| {
        c == ' ' || (!c.is_whitespace() && !c.is_control() && rich::cells::char_cell_width(c) == 1)
    });
    // Measured whole too: a cluster can measure otherwise than its parts.
    (single && rich::cells::cell_len(line) == chars.len()).then_some(Some(chars))
}

/// What `Console::render_export(&Text::new(line))` writes for a line of
/// one-cell characters at `width` cells, each line after `indent` spaces,
/// or `false` (nothing written) for a line that is not one of those (see
/// [`simple_chars`]). Core's word wrap (`divide_line` with folding, then
/// `rstrip_end` and truncation to the width), without measuring each
/// character again and again: a record a line is the common case, and
/// measuring was most of the time spent. A test holds it to core's output.
fn wrap_simple(line: &str, width: usize, indent: usize, out: &mut impl Write) -> io::Result<bool> {
    let Some(chars) = simple_chars(line) else {
        return Ok(false);
    };
    let length = chars.as_ref().map_or(line.len(), Vec::len);
    let space = |at: usize| match &chars {
        None => line.as_bytes()[at] == b' ',
        Some(chars) => chars[at].1 == ' ',
    };
    let byte = |at: usize| match &chars {
        None => at,
        Some(chars) => chars.get(at).map_or(line.len(), |(byte, _)| *byte),
    };
    let mut breaks: Vec<usize> = Vec::new();
    let mut offset: isize = 0;
    let room = width as isize;
    let mut at = 0;
    while at < length {
        let start = at;
        while at < length && space(at) {
            at += 1;
        }
        if at == length {
            break;
        }
        while at < length && !space(at) {
            at += 1;
        }
        let word = (at - start) as isize;
        while at < length && space(at) {
            at += 1;
        }
        let whole = (at - start) as isize;
        if room - offset >= word {
            offset += whole;
        } else if word > room {
            // Folded into pieces of the width, the last one carried on.
            let mut piece_start = start;
            let mut left = at - start;
            loop {
                if piece_start != 0 {
                    breaks.push(piece_start);
                }
                if left <= width {
                    offset = left as isize;
                    break;
                }
                piece_start += width;
                left -= width;
            }
        } else if offset != 0 && start != 0 {
            breaks.push(start);
            offset = whole;
        }
    }
    let pad = " ".repeat(indent);
    let mut from = 0;
    for to in breaks.into_iter().chain([length]) {
        let mut end = to;
        if end - from > width {
            // `rstrip_end`: trailing spaces past the width go, then the
            // rest is cut to it.
            let excess = end - from - width;
            let mut spaces = 0;
            while spaces < end - from && space(end - 1 - spaces) {
                spaces += 1;
            }
            end -= spaces.min(excess);
            end = end.min(from + width);
        }
        out.write_all(pad.as_bytes())?;
        out.write_all(&line.as_bytes()[byte(from)..byte(end)])?;
        out.write_all(b"\n")?;
        from = to;
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn console(width: usize) -> Console {
        Console::builder().width(width).no_color(true).build()
    }

    /// The fast path writes exactly what the console writes for the same
    /// text, over random lines of words, runs of spaces and long words, in
    /// ASCII and in other one-cell characters.
    #[test]
    fn simple_wrapping_matches_the_console() {
        let mut random = fastrand::Rng::with_seed(724);
        let alphabet = [
            'a', 'b', ' ', ' ', '-', '{', '"', ':', '7', 'é', '▸', '·', '×',
        ];
        for case in 0..6_000 {
            let length = random.usize(1..160);
            // Half the lines ASCII, half with the others too.
            let letters = if case % 2 == 0 { 9 } else { alphabet.len() };
            let line: String = (0..length)
                .map(|_| alphabet[random.usize(..letters)])
                .collect();
            let width = random.usize(1..90);
            let expected = console(width).render_export(&Text::new(line.as_str()));
            let mut got = Vec::new();
            assert!(wrap_simple(&line, width, 0, &mut got).unwrap());
            assert_eq!(
                String::from_utf8(got).unwrap(),
                expected,
                "case {case}: {line:?} at width {width}"
            );
        }
        // Not simple: left to the console.
        for line in [
            "",
            "tab\there",
            "wide 中文",
            "e\u{301}",
            "nbsp\u{a0}x",
            "bell\u{7}",
        ] {
            assert!(
                !wrap_simple(line, 20, 0, &mut Vec::new()).unwrap(),
                "{line:?}"
            );
        }
        let mut indented = Vec::new();
        assert!(wrap_simple("one two three", 7, 2, &mut indented).unwrap());
        assert_eq!(String::from_utf8(indented).unwrap(), "  one two\n  three\n");
    }

    #[test]
    fn plain_records_keep_the_old_line_and_take_a_time() {
        let value = serde_json::json!({
            "time": "2026-10-18T10:00:00Z", "level": "warn", "msg": "slow", "n": 1
        });
        let (line, rest) = plain_record(&value, None, true);
        assert_eq!(line, r#"2026-10-18T10:00:00Z WARN slow {"n":1}"#);
        assert_eq!(rest.len(), 1);
        assert_eq!(
            plain_record(&value, Some(Some("+0.000s")), true).0,
            r#"+0.000s WARN slow {"n":1}"#
        );
        assert_eq!(plain_record(&value, Some(None), false).0, "WARN slow");
        let bare = serde_json::json!([1, 2]);
        assert_eq!(plain_record(&bare, None, true).0, "[1,2]");
        let only_time = serde_json::json!({ "time": "t" });
        assert_eq!(plain_record(&only_time, Some(None), true).0, "");
    }

    fn parse(args: &[&str]) -> Result<LogOptions, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let mut options = LogOptions::default();
        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            if !options.parse_option(arg, &mut iter)? {
                return Err(format!("not mine: {arg}"));
            }
        }
        Ok(options)
    }

    #[test]
    fn options_parse_and_check() {
        let options = parse(&["--level", "warn+", "--level", "debug", "--where", "a=1"]).unwrap();
        assert_eq!(options.levels.unwrap().to_string(), "warn+,debug");
        assert!(parse(&["--level", "loud"])
            .unwrap_err()
            .contains("unknown level"));
        assert!(parse(&["--where", "=x"])
            .unwrap_err()
            .starts_with("--where:"));
        assert!(parse(&["--group", "host"])
            .unwrap_err()
            .contains("trace, span"));
        // A number is `rich hex`'s.
        assert_eq!(parse(&["--group", "4"]).unwrap_err(), "not mine: --group");
        assert_eq!(
            parse(&["--group", "span"]).unwrap().group,
            Some(GroupBy::Span)
        );
        assert!(parse(&["--parser", "xml"]).is_err());
        assert!(parse(&["--parser", "nginx"]).is_ok());
        assert!(parse(&["--time", "soon"]).is_err());
        assert!(parse(&["--output", "csv"]).is_err());
        let jsonl = parse(&["--output", "jsonl", "--fields"]).unwrap();
        assert!(jsonl
            .validate(Some("a.log"))
            .unwrap_err()
            .contains("--output jsonl"));
        let follow = parse(&["-f"]).unwrap();
        assert!(follow.validate(None).is_err());
        assert!(follow.validate(Some("-")).is_err());
        assert!(follow.validate(Some("app.log")).is_ok());
        assert!(follow.collapses() && !follow.summarises());
        let grouped = parse(&["--group", "trace"]).unwrap();
        assert!(grouped.summarises() && !grouped.collapses());
        assert!(!parse(&["--group", "trace", "--no-summary"])
            .unwrap()
            .summarises());
        assert!(parse(&["-i", "--group", "trace"])
            .unwrap()
            .validate(None)
            .is_err());
        assert_eq!(
            parse(&["--where", "a=1"]).unwrap().given(),
            [("--where", &["log"][..])]
        );
    }

    #[test]
    fn a_followed_file_is_read_on_after_truncation_and_replacement() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        std::fs::write(&path, "one\ntwo\npart").unwrap();
        let mut tail = Tail::open(path.clone()).unwrap();
        let mut seen = Vec::new();
        let mut record = |event: Tailed<'_>| {
            seen.push(match event {
                Tailed::Line(line) => line.to_string(),
                Tailed::Quiet => "<quiet>".into(),
                Tailed::Notice(notice) => format!("<{}>", notice.split(' ').nth(2).unwrap()),
            });
            Ok(())
        };
        assert!(tail.read(&mut record).unwrap());
        assert!(!tail.read(&mut record).unwrap());
        // Appended to: the partial line finishes.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"ial\nthree\n").unwrap();
        drop(file);
        tail.read(&mut record).unwrap();
        // Truncated (copytruncate rotation).
        std::fs::write(&path, "").unwrap();
        tail.check(&mut record).unwrap();
        std::fs::write(&path, "four\n").unwrap();
        tail.read(&mut record).unwrap();
        // Replaced (rename and create).
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(b"last of old\n").unwrap();
        drop(file);
        std::fs::rename(&path, dir.path().join("app.log.1")).unwrap();
        std::fs::write(&path, "five\n").unwrap();
        tail.check(&mut record).unwrap();
        tail.read(&mut record).unwrap();
        assert_eq!(
            seen,
            [
                "one",
                "two",
                "partial",
                "three",
                "<truncated;>",
                "four",
                "last of old",
                "<replaced;>",
                "five"
            ]
        );
    }
}

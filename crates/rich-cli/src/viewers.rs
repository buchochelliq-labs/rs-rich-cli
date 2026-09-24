//! `rich view`, `rich hex`, `rich unicode`, `rich env` and `rich capture`:
//! CLI glue over `rich_ext::source_view`, `rich_ext::hex`,
//! `rich_ext::unicode_inspect` and `rich_ext::env_inspect` (0.0.11 workstream
//! 11).
//!
//! None of these exists upstream. They compose public rich-ext renderables
//! and core's ANSI decoder, so they are binary-boundary conveniences like
//! `inspect.rs` and `tools.rs`.
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rich::ansi::AnsiDecoder;
use rich::text::Text;
use rich::{Panel, Renderable, Style};
use rich_ext::data::Format;
use rich_ext::source_view::SourceView;

/// Options for the viewer commands.
#[derive(Clone, Debug, Default)]
pub(crate) struct ViewerOptions {
    search: Option<String>,
    no_line_numbers: bool,
    offset: Option<u64>,
    length: Option<usize>,
    bytes_per_line: Option<usize>,
    group: Option<usize>,
    limit: Option<usize>,
    show_secrets: bool,
    cast: Option<String>,
    redact_patterns: Vec<String>,
}

fn number<'a, T: std::str::FromStr>(
    flag: &str,
    rest: &mut impl Iterator<Item = &'a String>,
) -> Result<T, String> {
    let value = rest.next().ok_or(format!("{flag} requires a number"))?;
    let digits = value.replace('_', "");
    let parsed = match digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        Some(hex) => u64::from_str_radix(hex, 16)
            .ok()
            .and_then(|n| n.to_string().parse().ok()),
        None => digits.parse().ok(),
    };
    parsed.ok_or(format!("{flag} requires a number, got {value:?}"))
}

impl ViewerOptions {
    /// Consume one of these options; anything else stays with the main parser.
    pub(crate) fn parse_option<'a>(
        &mut self,
        arg: &str,
        rest: &mut impl Iterator<Item = &'a String>,
    ) -> Result<bool, String> {
        match arg {
            "--search" => {
                self.search = Some(rest.next().ok_or("--search requires a pattern")?.clone());
            }
            "--no-line-numbers" => self.no_line_numbers = true,
            "--offset" => self.offset = Some(number(arg, rest)?),
            "--length" => self.length = Some(number(arg, rest)?),
            "--bytes-per-line" => self.bytes_per_line = Some(number(arg, rest)?),
            "--group" => {
                let group: usize = number(arg, rest)?;
                if group == 0 {
                    return Err("--group must be at least 1".into());
                }
                self.group = Some(group);
            }
            "--limit" => self.limit = Some(number(arg, rest)?),
            "--show-secrets" => self.show_secrets = true,
            "--cast" => self.cast = Some(rest.next().ok_or("--cast requires a file")?.clone()),
            "--redact-pattern" => self.redact_patterns.push(
                rest.next()
                    .ok_or("--redact-pattern requires a regular expression")?
                    .clone(),
            ),
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// Each option given with the commands it applies to, for the "only has
    /// an effect" check.
    pub(crate) fn given(&self) -> Vec<(&'static str, &'static [&'static str])> {
        [
            ("--search", self.search.is_some(), &["view", "hex"][..]),
            ("--no-line-numbers", self.no_line_numbers, &["view"]),
            ("--offset", self.offset.is_some(), &["hex"]),
            ("--length", self.length.is_some(), &["hex"]),
            ("--bytes-per-line", self.bytes_per_line.is_some(), &["hex"]),
            ("--group", self.group.is_some(), &["hex"]),
            ("--limit", self.limit.is_some(), &["unicode"]),
            ("--show-secrets", self.show_secrets, &["env"]),
            ("--cast", self.cast.is_some(), &["capture"]),
            (
                "--redact-pattern",
                !self.redact_patterns.is_empty(),
                &["capture"],
            ),
        ]
        .into_iter()
        .filter(|(_, given, _)| *given)
        .map(|(flag, _, commands)| (flag, commands))
        .collect()
    }
}

/// What `rich view` shows a resource as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewAs {
    Markdown,
    Csv,
    Notebook,
    Image,
    Gif,
    /// JSON Lines logs, through the `log` renderer.
    Log,
    /// A unified diff or patch.
    Patch,
    /// Highlighted source (or plain text) with this lexer.
    Source(String),
    /// Not text: a hex dump.
    Binary,
}

/// Whether `bytes` look binary: a NUL byte, or not UTF-8, in the first 8 KiB.
pub(crate) fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.contains(&0) {
        return true;
    }
    match std::str::from_utf8(head) {
        Ok(_) => false,
        // A multi-byte character cut off at the 8 KiB boundary is still text.
        Err(error) => error.error_len().is_some(),
    }
}

/// Decide how to view a resource from its extension, then its content.
pub(crate) fn detect(ext: Option<&str>, name: Option<&str>, bytes: &[u8]) -> ViewAs {
    match ext {
        Some("md" | "markdown") => return ViewAs::Markdown,
        Some("csv" | "tsv") => return ViewAs::Csv,
        Some("ipynb") => return ViewAs::Notebook,
        Some("gif") => return ViewAs::Gif,
        Some("png" | "jpg" | "jpeg" | "bmp" | "webp" | "tif" | "tiff" | "ico") => {
            return ViewAs::Image
        }
        Some("diff" | "patch") => return ViewAs::Patch,
        Some("jsonl" | "ndjson") => return ViewAs::Log,
        _ => {}
    }
    if looks_binary(bytes) {
        return ViewAs::Binary;
    }
    let text = String::from_utf8_lossy(bytes);
    let start = text.trim_start();
    if start.starts_with("diff --git ") || (start.starts_with("--- ") && text.contains("\n+++ ")) {
        return ViewAs::Patch;
    }
    if let Some(ext) = ext {
        return ViewAs::Source(ext.to_string());
    }
    let lexer = match Format::detect(&text, name) {
        Some(Format::Json) => "json",
        Some(Format::Yaml) => "yaml",
        Some(Format::Toml) => "toml",
        Some(Format::Xml) => "xml",
        Some(Format::Ini) => "ini",
        Some(Format::Dotenv) => "sh",
        None => "txt",
    };
    ViewAs::Source(lexer.to_string())
}

/// `rich view`'s source view, with a search summary line when searching.
pub(crate) fn source(options: &ViewerOptions, content: &str, lexer: &str) -> Box<dyn Renderable> {
    let mut view = SourceView::new(content, lexer).line_numbers(!options.no_line_numbers);
    if let Some(pattern) = &options.search {
        view = view.search(pattern.clone());
    }
    Box::new(view)
}

/// The one-line search summary `rich view --search` prints to stderr.
pub(crate) fn search_summary(options: &ViewerOptions, content: &str) -> Option<String> {
    let pattern = options.search.as_ref()?;
    let matches = SourceView::new(content, "txt")
        .search(pattern.clone())
        .matches();
    let total: usize = matches.iter().map(|(_, count)| count).sum();
    let lines: Vec<String> = matches
        .iter()
        .take(10)
        .map(|(line, _)| line.to_string())
        .collect();
    Some(match total {
        0 => format!("no matches for {pattern:?}"),
        _ => format!(
            "{total} match{} for {pattern:?} on line{} {}{}",
            if total == 1 { "" } else { "es" },
            if matches.len() == 1 { "" } else { "s" },
            lines.join(", "),
            if matches.len() > 10 { ", …" } else { "" }
        ),
    })
}

/// Output chunks with the time since the command started.
type Chunks = Vec<(Duration, Vec<u8>)>;

/// One command's captured output.
pub(crate) struct Captured {
    pub command: Vec<String>,
    /// Output chunks from stdout and stderr, in arrival order, with the time
    /// since the command started.
    pub chunks: Chunks,
    pub status: Option<i32>,
    /// The signal that killed the command, where the platform reports one.
    pub signal: Option<i32>,
    pub elapsed: Duration,
}

impl Captured {
    /// The status `rich capture` exits with: the command's own, or
    /// 128 + the signal number when a signal killed it, as a shell reports.
    /// A status that does not fit a byte, or reads as 0 once truncated, is 1.
    pub fn exit_code(&self) -> u8 {
        match (self.status, self.signal) {
            (Some(0), _) => 0,
            (Some(code), _) => u8::try_from(code).ok().filter(|&c| c != 0).unwrap_or(1),
            (None, Some(signal)) => u8::try_from(128 + signal).unwrap_or(1),
            (None, None) => 1,
        }
    }

    /// Mask secrets in the output and the command line, before anything is
    /// shown, exported or recorded. The output is decoded once, so a
    /// character split between pipe reads stays whole; chunks keep their
    /// timing and, when nothing matched, their bytes. A secret split across
    /// chunks is masked where it starts. On the command line, the word after
    /// a secret-named flag (`--token X`) is masked too.
    pub fn redact(&mut self, redactor: &rich_ext::redact::Redactor) {
        let chunks: Vec<&[u8]> = self.chunks.iter().map(|(_, b)| b.as_slice()).collect();
        let redacted = redactor.redact_byte_chunks(&chunks);
        for ((_, bytes), new) in self.chunks.iter_mut().zip(redacted) {
            *bytes = new;
        }
        self.command = redactor.redact_args(&self.command);
    }

    fn output(&self) -> Vec<u8> {
        self.chunks
            .iter()
            .flat_map(|(_, bytes)| bytes.clone())
            .collect()
    }
}

/// Run `command` with its output piped, as a colour-capable terminal `width`
/// columns wide would see it (`FORCE_COLOR`, `CLICOLOR_FORCE`, `COLUMNS`).
/// The command's own exit status is recorded, not returned as an error.
pub(crate) fn capture(command: &[String], width: usize) -> Result<Captured, String> {
    let (program, args) = command
        .split_first()
        .ok_or("capture needs a command after --")?;
    // stdout and stderr share one pipe, as they share a terminal, so their
    // output interleaves in the order it was written.
    let (mut reader, writer) = std::io::pipe().map_err(|e| format!("cannot capture: {e}"))?;
    let stderr = writer
        .try_clone()
        .map_err(|e| format!("cannot capture: {e}"))?;
    let started = Instant::now();
    let mut process = Command::new(program);
    process
        .args(args)
        .env("FORCE_COLOR", "1")
        .env("CLICOLOR_FORCE", "1")
        .env("COLUMNS", width.to_string())
        .env_remove("NO_COLOR")
        .stdin(Stdio::null())
        .stdout(writer)
        .stderr(stderr);
    let spawned = process.spawn();
    // Our copies of the write end must close, or the read never ends.
    drop(process);
    let mut child = spawned.map_err(|e| format!("cannot run {program}: {e}"))?;
    let chunks: Arc<Mutex<Chunks>> = Arc::default();
    let pump = {
        let chunks = chunks.clone();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            while let Ok(read) = reader.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                let at = started.elapsed();
                chunks
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((at, buffer[..read].to_vec()));
            }
        })
    };
    let status = child
        .wait()
        .map_err(|e| format!("waiting for {program}: {e}"))?;
    let _ = pump.join();
    let elapsed = started.elapsed();
    let chunks = std::mem::take(&mut *chunks.lock().unwrap_or_else(|e| e.into_inner()));
    Ok(Captured {
        command: command.to_vec(),
        chunks,
        status: status.code(),
        signal: exit_signal(&status),
        elapsed,
    })
}

#[cfg(unix)]
fn exit_signal(status: &std::process::ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(status)
}

#[cfg(not(unix))]
fn exit_signal(_status: &std::process::ExitStatus) -> Option<i32> {
    None
}

/// A command line as a shell would show it.
fn shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            let plain = !word.is_empty()
                && word
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:,+@%".contains(c));
            if plain {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', r"'\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_elapsed(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 1.0 {
        format!("{:.0}ms", seconds * 1000.0)
    } else {
        format!("{seconds:.2}s")
    }
}

/// The captured output in a panel titled with the command, its exit status
/// and duration below.
pub(crate) fn capture_view(captured: &Captured) -> Box<dyn Renderable> {
    let output = String::from_utf8_lossy(&captured.output()).into_owned();
    let mut lines = AnsiDecoder::new().decode(output.trim_end_matches('\n'));
    let mut text = Text::new("");
    for (index, line) in lines.drain(..).enumerate() {
        if index > 0 {
            text.append("\n", None);
        }
        text = text.append_text(&line);
    }
    let ok = captured.status == Some(0);
    let status = match captured.status {
        Some(code) => format!("exit {code}"),
        None => match captured.signal {
            Some(signal) => format!("killed by signal {signal}"),
            None => "killed by a signal".to_string(),
        },
    };
    let panel = Panel::new(Box::new(text))
        .title(format!(
            "$ {}",
            rich::markup::escape(&shell_words(&captured.command))
        ))
        .subtitle(format!("{status} · {}", format_elapsed(captured.elapsed)))
        .border_style(Style::parse(if ok { "green" } else { "red" }).unwrap_or_default());
    Box::new(panel)
}

/// The `--report json` envelope for a command that failed: `code` is
/// `"command"` and `exit_code` the status `rich capture` exits with.
pub(crate) fn capture_report(captured: &Captured) -> serde_json::Value {
    let message = match (captured.status, captured.signal) {
        (Some(code), _) => format!("command exited with status {code}"),
        (None, Some(signal)) => format!("command was killed by signal {signal}"),
        (None, None) => "command was killed by a signal".to_string(),
    };
    serde_json::json!({
        "ok": false,
        "code": "command",
        "exit_code": captured.exit_code(),
        "message": message,
        "error": { "message": message },
        "result": {
            "command": captured.command,
            "status": captured.status,
            "signal": captured.signal,
        },
    })
}

/// The capture as an asciicast v2 recording (`asciinema play FILE`).
pub(crate) fn asciicast(captured: &Captured, width: usize, height: usize) -> String {
    let header = serde_json::json!({
        "version": 2,
        "width": width,
        "height": height,
        "command": shell_words(&captured.command),
        "title": shell_words(&captured.command),
        "env": { "TERM": "xterm-256color" },
    });
    let mut out = header.to_string();
    out.push('\n');
    let mut event = |at: Duration, bytes: &[u8]| {
        // Terminals expect CRLF; piped output has bare LF.
        let data = String::from_utf8_lossy(bytes)
            .replace("\r\n", "\n")
            .replace('\n', "\r\n");
        out.push_str(&serde_json::json!([at.as_secs_f64(), "o", data]).to_string());
        out.push('\n');
    };
    // A character split between pipe reads goes out whole, in the event
    // where it ends, instead of as two replacement characters.
    let mut pending: Vec<u8> = Vec::new();
    let mut last = Duration::ZERO;
    for (at, bytes) in &captured.chunks {
        pending.extend_from_slice(bytes);
        let tail = pending.split_off(pending.len() - incomplete_utf8_tail(&pending));
        if !pending.is_empty() {
            event(*at, &pending);
        }
        pending = tail;
        last = *at;
    }
    if !pending.is_empty() {
        event(last, &pending);
    }
    out
}

/// How many bytes at the end of `bytes` are the start of a UTF-8
/// character that the next read will finish.
fn incomplete_utf8_tail(bytes: &[u8]) -> usize {
    let from = bytes.len().saturating_sub(3);
    for start in (from..bytes.len()).rev() {
        if bytes[start] & 0xc0 != 0x80 {
            return match std::str::from_utf8(&bytes[start..]) {
                Err(e) if e.error_len().is_none() => bytes.len() - start,
                _ => 0,
            };
        }
    }
    0
}

/// `rich hex`: the bytes from `--offset`, at most `--length` of them.
pub(crate) fn hex(options: &ViewerOptions, bytes: Vec<u8>) -> Result<Box<dyn Renderable>, String> {
    use rich_ext::hex::{parse_needle, HexView};
    let start = options.offset.unwrap_or(0);
    let skip = usize::try_from(start)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    let mut bytes = bytes;
    bytes.drain(..skip);
    if let Some(length) = options.length {
        bytes.truncate(length);
    }
    let mut view = HexView::new(bytes).offset(start);
    if let Some(per_line) = options.bytes_per_line {
        view = view.bytes_per_line(Some(per_line));
    }
    if let Some(group) = options.group {
        view = view.group(group);
    }
    if let Some(needle) = &options.search {
        view = view.highlight(&parse_needle(needle)?);
    }
    Ok(Box::new(view))
}

/// `rich unicode`: every grapheme of the input, invalid UTF-8 included.
pub(crate) fn unicode(
    options: &ViewerOptions,
    bytes: &[u8],
) -> Result<Box<dyn Renderable>, String> {
    use rich_ext::unicode_inspect::UnicodeView;
    // A trailing newline from `echo` or a file end is not what was asked about.
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    let mut view = UnicodeView::from_bytes(bytes);
    if let Some(limit) = options.limit {
        view = view.limit(limit);
    }
    Ok(Box::new(view))
}

/// `rich env [PATTERN…]`: variables whose names match, redacted unless
/// `--show-secrets`; a single name of a PATH-like variable checks its entries.
pub(crate) fn env(options: &ViewerOptions, patterns: &[String]) -> Box<dyn Renderable> {
    use rich_ext::env_inspect::{is_path_like, name_matches, EnvView, PathView, OS_PATH_SEPARATOR};
    if let [name] = patterns {
        if let Ok(value) = std::env::var(name) {
            if is_path_like(name, &value, OS_PATH_SEPARATOR) {
                return Box::new(PathView::new(name.clone(), value, OS_PATH_SEPARATOR));
            }
        }
    }
    let vars = std::env::vars().filter(|(name, _)| {
        patterns.is_empty() || patterns.iter().any(|pattern| name_matches(pattern, name))
    });
    Box::new(EnvView::new(vars).redact(!options.show_secrets))
}

pub(crate) fn cast_path(options: &ViewerOptions) -> Option<&str> {
    options.cast.as_deref()
}

/// The redactor `--redact` (parsed with the `--inspect` options, which share
/// it) and `--redact-pattern` ask for, if any: the built-in detectors, then
/// each pattern. Masks keep their width, so the captured screen keeps its
/// layout.
pub(crate) fn capture_redactor(
    options: &ViewerOptions,
    redact: bool,
) -> Result<Option<rich_ext::redact::Redactor>, String> {
    use rich_ext::redact::Redactor;
    if !redact && options.redact_patterns.is_empty() {
        return Ok(None);
    }
    let mut redactor = if redact {
        Redactor::secrets()
    } else {
        Redactor::new()
    };
    for pattern in &options.redact_patterns {
        redactor = redactor
            .pattern(pattern)
            .map_err(|e| format!("--redact-pattern: {e}"))?;
    }
    Ok(Some(redactor.preserve_width(true)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_prefers_extensions_then_content() {
        assert_eq!(detect(Some("md"), None, b"# x"), ViewAs::Markdown);
        assert_eq!(detect(Some("patch"), None, b"x"), ViewAs::Patch);
        assert_eq!(detect(Some("png"), None, b"\x89PNG"), ViewAs::Image);
        assert_eq!(
            detect(Some("rs"), None, b"fn main() {}"),
            ViewAs::Source("rs".into())
        );
        assert_eq!(detect(Some("bin"), None, b"\x00\x01"), ViewAs::Binary);
        assert_eq!(detect(None, None, b"\xff\xfe\x00"), ViewAs::Binary);
        assert_eq!(
            detect(None, None, b"diff --git a/x b/x\n--- a/x\n+++ b/x\n"),
            ViewAs::Patch
        );
        assert_eq!(
            detect(None, None, b"{\"a\": 1}\n"),
            ViewAs::Source("json".into())
        );
        assert_eq!(
            detect(None, None, b"just words\n"),
            ViewAs::Source("txt".into())
        );
        // A multi-byte character cut at the sniffing boundary is still text.
        let mut long = vec![b'a'; 8191];
        long.extend("é".as_bytes());
        assert!(!looks_binary(&long));
    }

    #[test]
    fn options_parse_numbers_and_report_their_command() {
        let args: Vec<String> = ["0x20", "1_000"].iter().map(|s| s.to_string()).collect();
        let mut rest = args.iter();
        let mut options = ViewerOptions::default();
        assert!(options.parse_option("--offset", &mut rest).unwrap());
        assert!(options.parse_option("--length", &mut rest).unwrap());
        assert_eq!((options.offset, options.length), (Some(32), Some(1000)));
        assert!(!options.parse_option("--width", &mut rest).unwrap());
        assert_eq!(
            options.given(),
            [("--offset", &["hex"][..]), ("--length", &["hex"][..])]
        );
        let mut empty = [].iter();
        assert!(ViewerOptions::default()
            .parse_option("--group", &mut empty)
            .is_err());
    }

    #[test]
    fn search_summaries_count_matches_and_lines() {
        let options = ViewerOptions {
            search: Some("x".into()),
            ..Default::default()
        };
        assert_eq!(
            search_summary(&options, "x\ny\nxx\n").unwrap(),
            "3 matches for \"x\" on lines 1, 3"
        );
        assert_eq!(
            search_summary(&options, "y\n").unwrap(),
            "no matches for \"x\""
        );
        assert!(search_summary(&ViewerOptions::default(), "x").is_none());
    }

    #[test]
    fn shell_words_quote_only_when_needed() {
        let words: Vec<String> = ["ls", "-la", "my file", "it's"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(shell_words(&words), r"ls -la 'my file' 'it'\''s'");
    }

    fn captured(command: &[&str], chunks: &[&[u8]]) -> Captured {
        Captured {
            command: command.iter().map(|s| s.to_string()).collect(),
            chunks: chunks
                .iter()
                .enumerate()
                .map(|(i, bytes)| (Duration::from_millis(i as u64), bytes.to_vec()))
                .collect(),
            status: Some(1),
            signal: None,
            elapsed: Duration::from_millis(chunks.len() as u64),
        }
    }

    fn redactor() -> rich_ext::redact::Redactor {
        capture_redactor(&ViewerOptions::default(), true)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn redaction_keeps_characters_split_between_reads() {
        let text = format!("x{}\n", "é".repeat(10_000));
        let chunks: Vec<&[u8]> = text.as_bytes().chunks(8192).collect();
        let mut capture = captured(&["cat", "big.txt"], &chunks);
        capture.redact(&redactor());
        // Nothing to mask: the bytes are untouched, so nothing is lost.
        let after: Vec<&[u8]> = capture.chunks.iter().map(|(_, b)| b.as_slice()).collect();
        assert_eq!(after, chunks);
        assert_eq!(capture.output(), text.as_bytes());
        // Invalid bytes are not multiplied, and a secret is still masked.
        let mut capture = captured(&["x"], &[b"\xc3", b"\xa9\xff token=ab", b"c\n"]);
        capture.redact(&redactor());
        assert_eq!(capture.output(), b"\xc3\xa9\xff token=***\n");
    }

    #[test]
    fn casts_keep_characters_split_between_reads() {
        let text = format!("x{}\n", "é".repeat(10_000));
        let chunks: Vec<&[u8]> = text.as_bytes().chunks(8192).collect();
        let cast = asciicast(&captured(&["cat"], &chunks), 80, 24);
        assert!(!cast.contains('\u{fffd}'), "a split character was mangled");
        let data: String = cast
            .lines()
            .skip(1)
            .map(|line| {
                let event: serde_json::Value = serde_json::from_str(line).unwrap();
                event[2].as_str().unwrap().to_string()
            })
            .collect();
        assert_eq!(data, text.replace('\n', "\r\n"));
    }

    #[test]
    fn redaction_masks_secret_flag_values_on_the_command_line() {
        let mut capture = captured(
            &[
                "deploy",
                "--token",
                "hunter2",
                "--api-key=abc123",
                "-p",
                "80",
            ],
            &[b"ok\n"],
        );
        capture.redact(&redactor());
        let masked = [
            "deploy",
            "--token",
            "*******",
            "--api-key=******",
            "-p",
            "80",
        ];
        assert_eq!(capture.command, masked);
        let report = capture_report(&capture).to_string();
        assert!(
            !report.contains("hunter2") && !report.contains("abc123"),
            "{report}"
        );
        let cast = asciicast(&capture, 80, 24);
        let header = cast.lines().next().unwrap();
        assert!(
            !header.contains("hunter2") && !header.contains("abc123"),
            "{header}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn capture_records_output_status_and_a_cast() {
        let command: Vec<String> = ["sh", "-c", "printf 'a\\n'; printf 'b\\n' >&2; exit 3"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let captured = capture(&command, 40).unwrap();
        assert_eq!(captured.status, Some(3));
        assert_eq!(captured.exit_code(), 3);
        // One pipe keeps stdout and stderr in the order they were written.
        assert_eq!(String::from_utf8(captured.output()).unwrap(), "a\nb\n");
        let cast = asciicast(&captured, 40, 10);
        let mut lines = cast.lines();
        let header: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(header["version"], 2);
        assert_eq!(header["width"], 40);
        for line in lines {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(event[1], "o");
            assert!(event[2].as_str().unwrap().ends_with("\r\n"));
        }
        let console = rich::Console::builder()
            .width(40)
            .color_system(None)
            .build();
        let out = console.render_to_string(capture_view(&captured).as_ref());
        assert!(out.contains("$ sh -c"), "{out}");
        assert!(out.contains("exit 3"), "{out}");
        assert!(capture(&["definitely-not-a-command-xyz".to_string()], 40).is_err());
    }
}

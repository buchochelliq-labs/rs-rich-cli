//! `rich approve` and `rich jump` (#471, #484): rs-rich-interact's
//! approval queue and jump navigator as shell commands. Not upstream: a CLI
//! convenience over the rs-rich-interact library, with the conventions of
//! the other interactive commands (`interactive.rs`): the component paints
//! on standard error, the answer goes to standard output, keys come from
//! the terminal even when standard input is the list, and the exit code is
//! 0 answered, 1 cancelled, 130 interrupted, 2 a usage error, 3 no answer
//! without a terminal.
//!
//! `rich approve` reads items as lines or JSON Lines and writes one JSON
//! line per item with its decision. `rich jump` searches files, Markdown
//! headings, code symbols, a commands file and a recent-places file (or
//! entries on standard input) in one fuzzy box, and prints where to go.
use super::*;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, OnceLock};

use rich::{Console, ConsoleOptions, Renderable, Segment, Style, Text};
use rich_ext::cli_doc::{ArgSpec, CommandSpec};
use rich_ext::sanitize_terminal_controls;
use rich_interact::components::{ApprovalQueue, Jump, JumpSource, Verdict};
use rich_interact::{Fallback, Item, Outcome, Preview};
use serde_json::Value;

use super::interactive::{
    read_stdin, report, run_options, write_stdout, Answer, EXIT_CANCELLED, EXIT_INTERRUPTED,
    MAX_ITEMS,
};

const COMMANDS: &[&str] = &["approve", "jump"];

/// The command word, as `interactive::requested` finds its own.
pub(super) fn requested(args: &[String]) -> Option<&'static str> {
    let word = super::subcommand_word(args)?;
    COMMANDS.iter().copied().find(|command| word == *command)
}

/// The most files `rich jump` lists, and reads for headings and symbols.
const MAX_FILES: usize = 20_000;
/// The most of one file read for its headings or symbols, or a preview.
const MAX_SCAN_BYTES: u64 = 1024 * 1024;
/// The most entries `rich jump` searches.
const MAX_ENTRIES: usize = 200_000;
/// The most places kept in a `--recent` file.
const MAX_RECENT: usize = 50;
/// Lines of a file a preview shows.
const PREVIEW_LINES: usize = 200;

pub(super) fn commands() -> Vec<CommandSpec> {
    let height = ArgSpec::option("height")
        .value_name("ROWS")
        .help("Show at most ROWS at once, fewer when the terminal is shorter");
    let mouse = ArgSpec::flag("mouse").help(
        "Report the mouse: click rows and buttons, scroll with the wheel (off by default: it \
         takes text selection from the terminal)",
    );
    vec![
        CommandSpec::new("approve")
            .about(
                "Review ITEMs (or lines, or JSON Lines, on standard input) one by one or in bulk: \
                 a approves, r rejects, s skips (A, R, S ask for a reason), Space marks, Enter \
                 submits; prints one JSON line per item with its decision",
            )
            .usage("approve [OPTIONS] [ITEM...]")
            .arg(
                ArgSpec::positional("ITEM")
                    .multiple(true)
                    .help("The items; without any, one per line from standard input"),
            )
            .arg(
                ArgSpec::option("header")
                    .value_name("TEXT")
                    .help("The prompt above the queue (default \"Approve\")"),
            )
            .arg(
                ArgSpec::option("format")
                    .value_name("FORMAT")
                    .choices(["auto", "lines", "jsonl"])
                    .help(
                        "How items are read: one per line, or a JSON object per line (default \
                         auto: JSON Lines when every line is an object)",
                    ),
            )
            .arg(ArgSpec::option("label").value_name("FIELD").help(
                "The JSON field to show as an item's label (default the first of title, name, \
                 label, summary and id)",
            ))
            .arg(
                ArgSpec::flag("require-reason")
                    .help("Ask for a reason on every rejection, and refuse an empty one"),
            )
            .arg(
                ArgSpec::flag("auto-submit")
                    .help("Submit as soon as nothing is pending, without Enter"),
            )
            .arg(
                ArgSpec::option("print")
                    .value_name("WHAT")
                    .choices(["decisions", "approved", "rejected"])
                    .help(
                        "What to print: a JSON line per item with its decision and reason \
                         (default), or the approved (or rejected) items as they were read",
                    ),
            )
            .arg(height.clone())
            .arg(mouse.clone())
            .example(
                "git branch --format='%(refname:short)' | rich approve --print approved | xargs -r git branch -D",
                "Delete the branches approved",
            )
            .example(
                "rich approve --require-reason < requests.jsonl > decisions.jsonl",
                "Review JSON requests, with a reason for each rejection",
            ),
        CommandSpec::new("jump")
            .about(
                "Jump to anything: one fuzzy box over files, Markdown headings (#), code symbols \
                 (@), commands (>) and recent places in DIRs, grouped by source with a preview; \
                 prints the place picked (`path`, `path:line` or the command)",
            )
            .usage("jump [OPTIONS] [DIR...]")
            .arg(ArgSpec::positional("DIR").multiple(true).help(
                "Where to look (default the current directory); `-` reads entries from standard \
                 input instead",
            ))
            .arg(
                ArgSpec::option("header")
                    .value_name("TEXT")
                    .help("The prompt (default \"Jump\")"),
            )
            .arg(ArgSpec::flag("files").help("Search file names"))
            .arg(ArgSpec::flag("headings").help("Search Markdown headings"))
            .arg(ArgSpec::flag("symbols").help(
                "Search code symbols: functions, types, classes and modules, found by their \
                 keywords",
            ))
            .arg(ArgSpec::option("commands").value_name("FILE").help(
                "Search the commands in FILE, one per line (`LABEL<TAB>COMMAND` or just the \
                 command); a pick prints the command",
            ))
            .arg(ArgSpec::option("recent").value_name("FILE").help(
                "Show FILE's places first, as Recent, and add the pick to it (the latest 50 kept)",
            ))
            .arg(
                ArgSpec::option("value")
                    .value_name("QUERY")
                    .help("Start with QUERY typed; without a terminal, print its best match"),
            )
            .arg(
                ArgSpec::option("limit")
                    .value_name("N")
                    .help("Results each source shows until Tab narrows to it (default 5)"),
            )
            .arg(
                ArgSpec::option("print")
                    .value_name("WHAT")
                    .choices(["location", "json"])
                    .help(
                        "What to print: the place (default), or a JSON object with its source, \
                         label and place",
                    ),
            )
            .arg(height)
            .arg(mouse)
            .example(
                "$EDITOR \"$(rich jump --files --headings)\"",
                "Open a file, or a Markdown file at a heading's line (an editor that takes \
                 `path:line`)",
            )
            .example(
                "eval \"$(rich jump --commands ~/.commands --recent ~/.jump-recent src)\"",
                "Run a saved command, or go somewhere recent",
            ),
    ]
}

/// The options both commands take (each its own subset).
#[derive(Default)]
struct Args {
    positionals: Vec<String>,
    header: Option<String>,
    format: Option<String>,
    label: Option<String>,
    require_reason: bool,
    auto_submit: bool,
    print: Option<String>,
    height: Option<usize>,
    mouse: bool,
    files: bool,
    headings: bool,
    symbols: bool,
    commands: Option<String>,
    recent: Option<String>,
    value: Option<String>,
    limit: Option<usize>,
    no_color: bool,
}

fn positive(name: &str, value: &str) -> Result<usize, String> {
    value
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| format!("{name}: {value:?} is not a positive number"))
}

fn parse_args(command: &'static str, args: &[String]) -> Result<Args, String> {
    let mut parsed = Args::default();
    let allowed: &[&str] = match command {
        "approve" => &[
            "--header",
            "--format",
            "--label",
            "--require-reason",
            "--auto-submit",
            "--print",
            "--height",
            "--mouse",
        ],
        _ => &[
            "--header",
            "--files",
            "--headings",
            "--symbols",
            "--commands",
            "--recent",
            "--value",
            "--limit",
            "--print",
            "--height",
            "--mouse",
        ],
    };
    let mut seen_command = false;
    let mut options_done = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if options_done || !arg.starts_with('-') || arg == "-" {
            if !seen_command && arg == command {
                seen_command = true;
            } else {
                parsed.positionals.push(arg.clone());
            }
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        match name {
            "--" => {
                options_done = true;
                continue;
            }
            "--no-config" | "--machine-json" => continue,
            "--no-color" => {
                parsed.no_color = true;
                continue;
            }
            "--report" if inline.is_none() => {
                ReportFormat::Human.apply_option(name, iter.next().map(String::as_str))?;
                continue;
            }
            _ => {}
        }
        if !allowed.contains(&name) {
            return Err(format!("unknown option {name} for `rich {command}`"));
        }
        let mut value = || -> Result<String, String> {
            match inline.clone() {
                Some(value) => Ok(text_arg(&value)),
                None => iter
                    .next()
                    .map(|value| text_arg(value))
                    .ok_or_else(|| format!("{name} needs a value")),
            }
        };
        match name {
            "--header" => parsed.header = Some(value()?),
            "--format" => {
                let format = value()?;
                if !matches!(format.as_str(), "auto" | "lines" | "jsonl") {
                    return Err(format!("--format: {format:?} is not auto, lines or jsonl"));
                }
                parsed.format = Some(format);
            }
            "--label" => parsed.label = Some(value()?),
            "--require-reason" => parsed.require_reason = true,
            "--auto-submit" => parsed.auto_submit = true,
            "--print" => {
                let what = value()?;
                let known: &[&str] = if command == "approve" {
                    &["decisions", "approved", "rejected"]
                } else {
                    &["location", "json"]
                };
                if !known.contains(&what.as_str()) {
                    return Err(format!("--print: {what:?} is not {}", known.join(" or ")));
                }
                parsed.print = Some(what);
            }
            "--height" => parsed.height = Some(positive(name, &value()?)?),
            "--limit" => parsed.limit = Some(positive(name, &value()?)?),
            "--mouse" => parsed.mouse = true,
            "--files" => parsed.files = true,
            "--headings" => parsed.headings = true,
            "--symbols" => parsed.symbols = true,
            "--commands" => parsed.commands = Some(value()?),
            "--recent" => parsed.recent = Some(value()?),
            _ => parsed.value = Some(value()?),
        }
    }
    if command == "approve" {
        for positional in &mut parsed.positionals {
            *positional = text_arg(positional);
        }
    }
    Ok(parsed)
}

pub(super) fn dispatch(command: &'static str, args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    if args
        .iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "--help")
    {
        let no_color = cli_spec::no_color_requested(args);
        if let Some(help) = cli_spec::subcommand_help(&[command], no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    let result = parse_args(command, args)
        .map_err(|message| (ExitClass::Usage, message))
        .and_then(|args| match command {
            "approve" => approve(&args),
            _ => jump(&args),
        });
    match result {
        Ok(code) => report(json, code),
        Err((ExitClass::Usage, message)) => {
            emit_error(json, ExitClass::Usage, &format!("{message} (try --help)"))
        }
        Err((class, message)) => emit_error(json, class, &message),
    }
}

fn exit_for<T>(outcome: Outcome<T>) -> Result<T, ExitCode> {
    match outcome {
        Outcome::Done(value) => Ok(value),
        Outcome::Cancelled => Err(ExitCode::from(EXIT_CANCELLED)),
        Outcome::Interrupted => Err(ExitCode::from(EXIT_INTERRUPTED)),
    }
}

// ---------------------------------------------------------------------------
// rich approve

/// One item to review: how it was read, and its JSON form.
struct Pending {
    raw: String,
    json: Value,
}

/// The label of a JSON item: `field`, or the first of the usual ones.
fn json_label(
    object: &serde_json::Map<String, Value>,
    field: Option<&str>,
) -> Option<(String, String)> {
    let fields: Vec<&str> = match field {
        Some(field) => vec![field],
        None => vec!["title", "name", "label", "summary", "id"],
    };
    fields.into_iter().find_map(|name| match object.get(name)? {
        Value::String(text) => Some((name.to_string(), text.clone())),
        Value::Number(number) => Some((name.to_string(), number.to_string())),
        _ => None,
    })
}

fn scalar(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(_) | Value::Bool(_) => Some(value.to_string()),
        Value::Null => Some("null".into()),
        _ => None,
    }
}

/// `line`'s item: a JSON object's label, description, fields and preview,
/// or the line itself.
fn approve_item(index: usize, pending: &Pending, label_field: Option<&str>) -> Item<usize> {
    let shown = |text: &str| sanitize_terminal_controls(text);
    let Value::Object(object) = &pending.json else {
        return Item::new(index, shown(&pending.raw));
    };
    let (used, label) = json_label(object, label_field)
        .unwrap_or_else(|| (String::new(), pending.json.to_string()));
    let mut item = Item::new(index, shown(&label));
    let mut nested = false;
    for (key, value) in object {
        if *key == used {
            continue;
        }
        match (key.as_str(), scalar(value)) {
            ("description", Some(text)) => item = item.description(shown(&text)),
            (_, Some(text)) => item = item.meta(shown(key), shown(&text)),
            (_, None) => nested = true,
        }
    }
    if nested {
        let pretty = serde_json::to_string_pretty(&pending.json).unwrap_or_default();
        item = item.preview(Preview::Text(shown(&pretty)));
    }
    item
}

/// The items from `lines` as `format` says: a JSON object per line, or
/// the lines as they are.
fn read_pending(lines: Vec<String>, format: &str) -> Result<Vec<Pending>, (ExitClass, String)> {
    let parse = |line: &str| match serde_json::from_str::<Value>(line) {
        Ok(value @ Value::Object(_)) => Some(value),
        _ => None,
    };
    let jsonl = match format {
        "jsonl" => true,
        "lines" => false,
        _ => {
            !lines.is_empty()
                && lines
                    .iter()
                    .all(|line| line.trim_start().starts_with('{') && parse(line).is_some())
        }
    };
    lines
        .into_iter()
        .enumerate()
        .map(|(number, raw)| {
            let json = if jsonl {
                parse(&raw).ok_or_else(|| {
                    (
                        ExitClass::Input,
                        format!("line {} is not a JSON object", number + 1),
                    )
                })?
            } else {
                Value::String(raw.clone())
            };
            Ok(Pending { raw, json })
        })
        .collect()
}

fn approve(args: &Args) -> Answer {
    let from_stdin = args.positionals.is_empty() || args.positionals == ["-"];
    let lines: Vec<String> = if from_stdin {
        if std::io::stdin().is_terminal() {
            return Err((
                ExitClass::Usage,
                "`rich approve` needs ITEMs, or lines on standard input".into(),
            ));
        }
        let text = read_stdin()?;
        if text.lines().nth(MAX_ITEMS).is_some() {
            return Err((
                ExitClass::Input,
                format!("standard input is over the {MAX_ITEMS} line limit"),
            ));
        }
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    } else {
        args.positionals.clone()
    };
    if lines.is_empty() {
        return Err((ExitClass::Usage, "no items to review".into()));
    }
    let pending = read_pending(lines, args.format.as_deref().unwrap_or("auto"))?;
    let items: Vec<Item<usize>> = pending
        .iter()
        .enumerate()
        .map(|(index, pending)| approve_item(index, pending, args.label.as_deref()))
        .collect();
    let mut queue = ApprovalQueue::new(
        args.header.clone().unwrap_or_else(|| "Approve".into()),
        items,
    )
    .with_mouse(args.mouse)
    .auto_submit(args.auto_submit);
    if args.require_reason {
        queue = queue.require_reason(Verdict::Rejected);
    }
    if let Some(rows) = args.height {
        queue = queue.height(rows);
    }
    // Standard input was the list: there is nothing left to ask on, so
    // without a terminal nothing is decided.
    let fallback = if from_stdin {
        Fallback::Default
    } else {
        Fallback::Prompt
    };
    let outcome = rich_interact::run(queue, &run_options(fallback, args.no_color))
        .map_err(|error| (ExitClass::Input, error.to_string()))?;
    let report = match exit_for(outcome) {
        Ok(report) => report,
        Err(code) => return Ok(code),
    };
    let mut out = String::new();
    for decision in &report.decisions {
        let pending = &pending[decision.value];
        let line = match args.print.as_deref() {
            Some("approved") if decision.verdict == Verdict::Approved => pending.raw.clone(),
            Some("rejected") if decision.verdict == Verdict::Rejected => pending.raw.clone(),
            Some(_) => continue,
            None => serde_json::json!({
                "index": decision.index,
                "decision": decision.verdict.as_str(),
                "reason": decision.reason,
                "item": pending.json,
            })
            .to_string(),
        };
        out.push_str(&line);
        out.push('\n');
    }
    write_stdout(&out)
}

// ---------------------------------------------------------------------------
// rich jump

/// Where an entry goes.
struct Place {
    /// What is printed: `path`, `path:line`, or a command.
    location: String,
    /// The file to preview, and the line in it.
    file: Option<(PathBuf, Option<usize>)>,
}

/// A file's lines around one, read when the preview is first drawn.
struct FileExcerpt {
    path: PathBuf,
    line: Option<usize>,
    lines: OnceLock<Result<Vec<String>, String>>,
}

impl FileExcerpt {
    fn new(path: PathBuf, line: Option<usize>) -> FileExcerpt {
        FileExcerpt {
            path,
            line,
            lines: OnceLock::new(),
        }
    }

    fn read(&self) -> Result<Vec<String>, String> {
        let bytes = read_head(&self.path).map_err(|error| error.to_string())?;
        if bytes.contains(&0) {
            return Err("(binary)".into());
        }
        let text = String::from_utf8_lossy(&bytes);
        let from = self.line.map_or(0, |line| line.saturating_sub(4));
        Ok(text
            .lines()
            .skip(from)
            .take(PREVIEW_LINES)
            .map(sanitize_terminal_controls)
            .collect())
    }
}

impl Renderable for FileExcerpt {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let mut text = Text::new("");
        match self.lines.get_or_init(|| self.read()) {
            Err(message) => text.append(
                message.as_str(),
                Some(Style::parse("dim").expect("a style").into()),
            ),
            Ok(lines) => {
                let from = self.line.map_or(1, |line| line.saturating_sub(4).max(1));
                let gutter = Style::parse("dim").expect("a style");
                let target = Style::parse("bold").expect("a style");
                for (at, line) in lines.iter().enumerate() {
                    let number = from + at;
                    if at > 0 {
                        text.append("\n", None);
                    }
                    let here = self.line == Some(number);
                    text.append(
                        format!("{}{number:>4} ", if here { "▶" } else { " " }).as_str(),
                        Some(gutter.clone().into()),
                    );
                    text.append(line.as_str(), here.then(|| target.clone().into()));
                }
            }
        }
        text.set_no_wrap(Some(true));
        text.rich_render(console, options)
    }
}

/// At most [`MAX_SCAN_BYTES`] of a regular file.
fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_SCAN_BYTES).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Every file under `root`, its shown path, skipping hidden entries and
/// build directories, at most `left` of them.
fn walk(root: &Path, shown: &str, out: &mut Vec<(PathBuf, String)>) {
    let mut stack = vec![(root.to_path_buf(), shown.to_string())];
    while let Some((dir, prefix)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());
        let mut dirs = Vec::new();
        for entry in entries {
            if out.len() >= MAX_FILES {
                return;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "target" || name == "node_modules" {
                continue;
            }
            let shown = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            match entry.file_type() {
                Ok(kind) if kind.is_dir() => dirs.push((entry.path(), shown)),
                Ok(kind) if kind.is_file() => out.push((entry.path(), shown)),
                _ => {}
            }
        }
        // Depth first, in name order.
        stack.extend(dirs.into_iter().rev());
    }
}

/// The Markdown headings of `text`: (line, level, title), outside code
/// fences.
fn headings(text: &str) -> Vec<(usize, usize, String)> {
    let mut found = Vec::new();
    let mut fence: Option<&str> = None;
    for (at, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some(open) = fence {
            if trimmed.starts_with(open) {
                fence = None;
            }
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(&trimmed[..3]);
            continue;
        }
        let level = line.chars().take_while(|c| *c == '#').count();
        if (1..=6).contains(&level) && line[level..].starts_with(' ') {
            let title = line[level..].trim().trim_end_matches('#').trim();
            if !title.is_empty() {
                found.push((at + 1, level, title.to_string()));
            }
        }
    }
    found
}

/// Code files whose symbols are looked for.
const CODE: &[&str] = &[
    "rs", "py", "js", "jsx", "ts", "tsx", "go", "java", "kt", "c", "h", "cc", "cpp", "hpp", "cs",
    "rb", "swift", "php", "scala", "sh",
];

/// The symbols of a code file: (line, kind, name), found by their keyword
/// at the start of a line.
fn symbols(text: &str) -> Vec<(usize, &'static str, String)> {
    const MODIFIERS: &[&str] = &[
        "pub(crate) ",
        "pub(super) ",
        "pub ",
        "export ",
        "default ",
        "async ",
        "unsafe ",
        "public ",
        "private ",
        "protected ",
        "static ",
        "abstract ",
        "final ",
        "extern ",
    ];
    const KINDS: &[(&str, &str)] = &[
        ("fn ", "fn"),
        ("struct ", "struct"),
        ("enum ", "enum"),
        ("trait ", "trait"),
        ("mod ", "mod"),
        ("macro_rules! ", "macro"),
        ("def ", "def"),
        ("class ", "class"),
        ("function ", "function"),
        ("func ", "func"),
        ("interface ", "interface"),
    ];
    let mut found = Vec::new();
    for (at, line) in text.lines().enumerate() {
        let mut rest = line.trim_start();
        while let Some(stripped) = MODIFIERS.iter().find_map(|m| rest.strip_prefix(m)) {
            rest = stripped;
        }
        for (keyword, kind) in KINDS {
            if let Some(after) = rest.strip_prefix(keyword) {
                // `func (r Recv) Name`: the receiver comes first.
                let after = match after.strip_prefix('(') {
                    Some(receiver) => receiver
                        .split_once(')')
                        .map_or("", |(_, name)| name.trim_start()),
                    None => after,
                };
                let name: String = after
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    found.push((at + 1, *kind, name));
                }
                break;
            }
        }
    }
    found
}

/// Read `path` as lines for `--commands` or `--recent`.
fn read_list(path: &str, what: &str) -> Result<Vec<String>, (ExitClass, String)> {
    let bytes = read_head(&fs_path(path)).map_err(|error| {
        (
            ExitClass::Input,
            format!("could not read {what} {path}: {error}"),
        )
    })?;
    Ok(String::from_utf8_lossy(&bytes)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .map(str::to_string)
        .collect())
}

/// A place's file and line: `path` or `path:line`, when the file is there.
fn file_of(location: &str) -> Option<(PathBuf, Option<usize>)> {
    let (path, line) = match location.rsplit_once(':') {
        Some((path, line)) if line.parse::<usize>().is_ok() => (path, line.parse().ok()),
        _ => (location, None),
    };
    let path = PathBuf::from(path);
    path.is_file().then_some((path, line))
}

/// What `rich jump` searches: the sources and where each entry goes.
struct Built {
    sources: Vec<JumpSource<usize>>,
    places: Vec<Place>,
}

impl Built {
    fn add(&mut self, source: usize, place: Place, item: impl FnOnce(usize) -> Item<usize>) {
        if self.places.len() >= MAX_ENTRIES {
            return;
        }
        let id = self.places.len();
        let mut item = item(id);
        if let Some((path, line)) = &place.file {
            item = item.preview(Preview::Renderable(Arc::new(FileExcerpt::new(
                path.clone(),
                *line,
            ))));
        }
        self.places.push(place);
        self.sources[source].items.push(item);
    }

    fn source(&mut self, source: JumpSource<usize>) -> usize {
        self.sources.push(source);
        self.sources.len() - 1
    }
}

/// Entries from standard input: a JSON object per line (`label`, and
/// optionally `source`, `location`, `detail`, `preview`), or plain lines.
fn stdin_entries(built: &mut Built, limit: usize) -> Result<(), (ExitClass, String)> {
    let text = read_stdin()?;
    if text.lines().nth(MAX_ITEMS).is_some() {
        return Err((
            ExitClass::Input,
            format!("standard input is over the {MAX_ITEMS} line limit"),
        ));
    }
    let mut by_name: Vec<(String, usize)> = Vec::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let object = match serde_json::from_str::<Value>(line) {
            Ok(Value::Object(object)) => Some(object),
            _ => None,
        };
        let field = |name: &str| -> Option<String> {
            object.as_ref().and_then(|object| scalar(object.get(name)?))
        };
        let label = field("label")
            .or_else(|| field("title"))
            .or_else(|| field("name"))
            .unwrap_or_else(|| line.to_string());
        let location = field("location").unwrap_or_else(|| label.clone());
        let group = field("source").unwrap_or_else(|| "Items".into());
        let source = match by_name.iter().find(|(name, _)| *name == group) {
            Some((_, at)) => *at,
            None => {
                let id = group.to_lowercase();
                let at = built
                    .source(JumpSource::new(id, sanitize_terminal_controls(&group)).limit(limit));
                by_name.push((group, at));
                at
            }
        };
        let detail = field("detail").or_else(|| field("description"));
        let preview = field("preview");
        let file = if preview.is_none() {
            file_of(&location)
        } else {
            None
        };
        built.add(
            source,
            Place {
                location: location.clone(),
                file,
            },
            |id| {
                let mut item = Item::new(id, sanitize_terminal_controls(&label))
                    .keyword(sanitize_terminal_controls(&location));
                if let Some(detail) = detail {
                    item = item.description(sanitize_terminal_controls(&detail));
                }
                if let Some(preview) = preview {
                    item = item.preview(Preview::Text(sanitize_terminal_controls(&preview)));
                }
                item
            },
        );
    }
    Ok(())
}

fn build_sources(args: &Args) -> Result<(Built, bool), (ExitClass, String)> {
    let limit = args.limit.unwrap_or(5);
    let mut built = Built {
        sources: Vec::new(),
        places: Vec::new(),
    };
    // Files, headings and symbols: all three unless some are chosen.
    let chosen = args.files || args.headings || args.symbols;
    let dash = args.positionals.iter().any(|dir| dir == "-");
    let dirs: Vec<&String> = args.positionals.iter().filter(|dir| *dir != "-").collect();
    let from_stdin = dash
        || (dirs.is_empty()
            && !chosen
            && args.commands.is_none()
            && !std::io::stdin().is_terminal());
    if let Some(path) = &args.recent {
        // A recent file that is not there yet is an empty one.
        let places = match fs_path(path).exists() {
            true => read_list(path, "the recent file")?,
            false => Vec::new(),
        };
        let at = built.source(JumpSource::new("recent", "Recent").limit(limit));
        for location in places.into_iter().take(MAX_RECENT) {
            let file = file_of(&location);
            built.add(
                at,
                Place {
                    location: location.clone(),
                    file,
                },
                |id| Item::new(id, sanitize_terminal_controls(&location)),
            );
        }
    }
    if from_stdin {
        stdin_entries(&mut built, limit)?;
    }
    let everything = !chosen && !from_stdin;
    let (files, headings_on, symbols_on) = (
        args.files || everything,
        args.headings || everything,
        args.symbols || everything,
    );
    if files || headings_on || symbols_on {
        let mut found = Vec::new();
        let roots: Vec<String> = if dirs.is_empty() {
            vec![".".into()]
        } else {
            dirs.iter().map(|dir| dir.to_string()).collect()
        };
        for root in &roots {
            let path = fs_path(root);
            if !path.is_dir() {
                return Err((ExitClass::Input, format!("{root} is not a directory")));
            }
            let shown = if root == "." {
                String::new()
            } else {
                text_arg(root).trim_end_matches('/').to_string()
            };
            walk(&path, &shown, &mut found);
        }
        let files_at = files.then(|| built.source(JumpSource::new("files", "Files").limit(limit)));
        let headings_at = headings_on.then(|| {
            built.source(
                JumpSource::new("headings", "Headings")
                    .prefix('#')
                    .limit(limit),
            )
        });
        let symbols_at = symbols_on.then(|| {
            built.source(
                JumpSource::new("symbols", "Symbols")
                    .prefix('@')
                    .limit(limit),
            )
        });
        for (path, shown) in &found {
            let shown_text = sanitize_terminal_controls(shown);
            if let Some(at) = files_at {
                let place = Place {
                    location: shown.clone(),
                    file: Some((path.clone(), None)),
                };
                built.add(at, place, |id| Item::new(id, shown_text.clone()));
            }
            let extension = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let markdown = matches!(extension.as_str(), "md" | "markdown");
            let code = CODE.contains(&extension.as_str());
            let wanted = (markdown && headings_at.is_some()) || (code && symbols_at.is_some());
            if !wanted {
                continue;
            }
            let Ok(bytes) = read_head(path) else { continue };
            if bytes.contains(&0) {
                continue;
            }
            let text = String::from_utf8_lossy(&bytes);
            if let (true, Some(at)) = (markdown, headings_at) {
                for (line, level, title) in headings(&text) {
                    let place = Place {
                        location: format!("{shown}:{line}"),
                        file: Some((path.clone(), Some(line))),
                    };
                    let detail = format!("{} {shown_text}:{line}", "#".repeat(level));
                    built.add(at, place, |id| {
                        Item::new(id, sanitize_terminal_controls(&title))
                            .description(detail)
                            .keyword(shown_text.clone())
                    });
                }
            }
            if let (true, Some(at)) = (code, symbols_at) {
                for (line, kind, name) in symbols(&text) {
                    let place = Place {
                        location: format!("{shown}:{line}"),
                        file: Some((path.clone(), Some(line))),
                    };
                    let detail = format!("{kind} · {shown_text}:{line}");
                    built.add(at, place, |id| {
                        Item::new(id, name)
                            .description(detail)
                            .keyword(shown_text.clone())
                    });
                }
            }
        }
    }
    if let Some(path) = &args.commands {
        let at = built.source(
            JumpSource::new("commands", "Commands")
                .prefix('>')
                .limit(limit),
        );
        for line in read_list(path, "the commands file")? {
            let (label, command) = match line.split_once('\t') {
                Some((label, command)) => (label.trim().to_string(), command.trim().to_string()),
                None => (line.clone(), line.clone()),
            };
            built.add(
                at,
                Place {
                    location: command.clone(),
                    file: None,
                },
                |id| {
                    let item = Item::new(id, sanitize_terminal_controls(&label));
                    if label == command {
                        item
                    } else {
                        item.description(sanitize_terminal_controls(&command))
                    }
                },
            );
        }
    }
    Ok((built, from_stdin))
}

/// Put `location` first in the recent file, keeping the latest.
fn remember(path: &str, location: &str) -> Result<(), (ExitClass, String)> {
    let path_buf = fs_path(path);
    let mut places = if path_buf.exists() {
        read_list(path, "the recent file")?
    } else {
        Vec::new()
    };
    places.retain(|place| place != location);
    places.insert(0, location.to_string());
    places.truncate(MAX_RECENT);
    let mut text = places.join("\n");
    text.push('\n');
    std::fs::write(&path_buf, text).map_err(|error| {
        (
            ExitClass::Input,
            format!("could not write the recent file {path}: {error}"),
        )
    })
}

fn jump(args: &Args) -> Answer {
    let (built, from_stdin) = build_sources(args)?;
    if built.places.is_empty() {
        return Err((ExitClass::Input, "nothing to jump to".into()));
    }
    let Built { sources, places } = built;
    let mut jump =
        Jump::new(args.header.clone().unwrap_or_else(|| "Jump".into())).with_mouse(args.mouse);
    for source in sources
        .into_iter()
        .filter(|source| !source.items.is_empty())
    {
        jump = jump.source(source);
    }
    if let Some(rows) = args.height {
        jump = jump.height(rows);
    }
    if let Some(query) = &args.value {
        jump = jump.query(query.clone());
    }
    // With a query given, or stdin taken by the entries, there is nothing
    // to ask: the best match answers.
    let fallback = if from_stdin || args.value.is_some() {
        Fallback::Default
    } else {
        Fallback::Prompt
    };
    let outcome = rich_interact::run(jump, &run_options(fallback, args.no_color))
        .map_err(|error| (ExitClass::Input, error.to_string()))?;
    let jumped = match exit_for(outcome) {
        Ok(jumped) => jumped,
        Err(code) => return Ok(code),
    };
    let place = &places[jumped.value];
    if let Some(recent) = &args.recent {
        remember(recent, &place.location)?;
    }
    let line = match args.print.as_deref() {
        Some("json") => serde_json::json!({
            "source": jumped.source,
            "label": jumped.label,
            "location": place.location,
        })
        .to_string(),
        _ => place.location.clone(),
    };
    write_stdout(&format!("{line}\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_skip_code_fences() {
        let text = "# Title\n\n```sh\n# not a heading\n```\n## Install ##\n#nospace\n";
        assert_eq!(
            headings(text),
            [(1, 1, "Title".to_string()), (6, 2, "Install".to_string())]
        );
    }

    #[test]
    fn symbols_are_found_by_keyword() {
        let text = "pub fn run() {}\n    async def fetch(x):\nclass Queue:\nfunc (q *Q) Push() {}\nlet fn_x = 1;\npub(crate) struct Built {\n";
        let names: Vec<(usize, &str, String)> = symbols(text);
        assert_eq!(
            names,
            [
                (1, "fn", "run".to_string()),
                (2, "def", "fetch".to_string()),
                (3, "class", "Queue".to_string()),
                (4, "func", "Push".to_string()),
                (6, "struct", "Built".to_string()),
            ]
        );
    }

    #[test]
    fn json_items_take_a_label_and_their_fields() {
        let pending = Pending {
            raw: String::new(),
            json: serde_json::json!({"id": 7, "title": "Deploy", "env": "prod", "steps": [1]}),
        };
        let item = approve_item(0, &pending, None);
        assert_eq!(item.label, "Deploy");
        assert_eq!(
            item.metadata,
            [
                ("id".to_string(), "7".to_string()),
                ("env".to_string(), "prod".to_string())
            ]
        );
        assert!(item.preview.is_some());
        let item = approve_item(0, &pending, Some("env"));
        assert_eq!(item.label, "prod");
    }
}

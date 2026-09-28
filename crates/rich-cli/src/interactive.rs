//! `rich choose`, `rich filter`, `rich input`, `rich confirm` and
//! `rich pager` (#493, #494): the rs-rich-interact components as shell
//! commands. Not upstream: a CLI convenience over the rs-rich-interact
//! library.
//!
//! They are made for scripts. The answer goes to standard output and the
//! component paints on standard error, so `choice=$(rich choose a b c)`
//! works; keys come from the terminal even when standard input is the list
//! (`ls | rich filter`). The exit code reports what happened: 0 answered
//! (for `confirm`, yes), 1 cancelled (for `confirm`, no), 130 interrupted
//! with Ctrl+C, 2 a usage error, 3 no answer without a terminal.
//!
//! Without a terminal they follow rs-rich-interact's degradation policy:
//! `input` and `confirm`, and `choose` from arguments, ask line by line on
//! standard error; `choose` from standard input answers with `--selected`;
//! `filter` prints the lines that match `--value`, so it is a fuzzy `grep`
//! in a pipeline; `pager` writes the content out.
use super::*;

use std::io::{IsTerminal, Read};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rich_ext::cli_doc::{ArgSpec, CommandSpec};
use rich_ext::sanitize_terminal_controls;
use rich_interact::{
    Choice, Confirm, Error as RunError, Fallback, Input, Item, LoopOptions, MultiSelect,
    NotInteractive, Outcome, Output, Pager, Policy, Preview, RunOptions, Select, SessionOptions,
};

/// The commands, in the order help lists them.
const COMMANDS: &[&str] = &["choose", "filter", "input", "confirm", "pager"];

/// The command word, when the first word that is not an option is one.
pub(super) fn requested(args: &[String]) -> Option<&'static str> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            return None;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
            continue;
        }
        if !arg.starts_with('-') || arg == "-" {
            return COMMANDS.iter().copied().find(|command| arg == command);
        }
    }
    None
}

const EXIT_CANCELLED: u8 = 1;
const EXIT_INTERRUPTED: u8 = 130;

/// The most standard input (or a paged file) read, so an endless input such
/// as `yes` ends with an error instead of exhausting memory.
const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// The most items `choose` and `filter` read from standard input.
const MAX_ITEMS: usize = 1_000_000;
/// How long a `--preview` command may run before it is killed.
const PREVIEW_TIMEOUT: Duration = Duration::from_secs(5);
/// The most output kept from a `--preview` command.
const PREVIEW_MAX_BYTES: usize = 1024 * 1024;
/// The most lines of a preview kept for the pane.
const PREVIEW_MAX_LINES: usize = 1000;

/// The commands' help: registered with the root spec in `cli_spec`.
pub(super) fn commands() -> Vec<CommandSpec> {
    let preview = ArgSpec::option("preview").value_name("COMMAND").help(
        "Show COMMAND's output for the focused item beside the list; `{}` is the item, \
         shell-quoted, and $COLUMNS the pane's width (e.g. `rich {} --force-terminal`)",
    );
    let pick = |name: &str, about: &str| {
        CommandSpec::new(name)
            .about(about)
            .usage(format!("{name} [OPTIONS] [ITEM...]"))
            .arg(
                ArgSpec::positional("ITEM")
                    .multiple(true)
                    .help("The items; without any, one per line from standard input"),
            )
            .arg(
                ArgSpec::option("header")
                    .value_name("TEXT")
                    .help("The prompt above the list"),
            )
            .arg(
                ArgSpec::flag("multi")
                    .help("Pick several: Tab marks, Enter returns the marked items, one per line"),
            )
            .arg(
                ArgSpec::option("selected")
                    .value_name("ITEM")
                    .multiple(true)
                    .help("Focus ITEM (with --multi, mark it); the answer without a terminal"),
            )
            .arg(
                ArgSpec::option("height")
                    .value_name("ROWS")
                    .help("Show at most ROWS items at once (default 10)"),
            )
            .arg(preview.clone())
    };
    vec![
        pick(
            "choose",
            "Pick from ITEMs with a fuzzy-filtered list and print the choice; exits 1 when \
             cancelled",
        )
        .example(
            "branch=$(git branch --format='%(refname:short)' | rich choose)",
            "A branch",
        )
        .example(
            "rich choose --preview 'rich {} --force-terminal' *.md",
            "Preview each file as rich renders it",
        ),
        pick(
            "filter",
            "Type to filter ITEMs and print the choice; without a terminal, print the lines \
             that match --value",
        )
        .arg(
            ArgSpec::option("value")
                .value_name("QUERY")
                .help("Start with QUERY typed"),
        )
        .example("ls | rich filter", "Pick a file")
        .example(
            "rich filter --value err < log.txt",
            "Fuzzy grep in a pipeline",
        ),
        CommandSpec::new("input")
            .about("Read one line with editing and history, and print it")
            .usage("input [OPTIONS]")
            .arg(
                ArgSpec::option("prompt")
                    .value_name("TEXT")
                    .help("The prompt (default \"Input\")"),
            )
            .arg(
                ArgSpec::option("placeholder")
                    .value_name("TEXT")
                    .help("Shown while the line is empty"),
            )
            .arg(
                ArgSpec::option("value")
                    .value_name("TEXT")
                    .help("Start with TEXT typed"),
            )
            .arg(
                ArgSpec::option("default")
                    .value_name("TEXT")
                    .help("The answer to an empty line, and without a terminal"),
            )
            .arg(
                ArgSpec::flag("password")
                    .help("Hide what is typed; without a terminal, read with echo off"),
            )
            .arg(
                ArgSpec::flag("required")
                    .help("Refuse an empty answer: Enter shows an error under the line and waits"),
            )
            .example(
                "name=$(rich input --prompt Name --placeholder 'Ada Lovelace')",
                "A name",
            )
            .example(
                "rich input --prompt Project --required",
                "An answer that cannot be empty",
            ),
        CommandSpec::new("confirm")
            .about("Ask a yes-or-no question; exits 0 for yes and 1 for no")
            .usage("confirm [OPTIONS] [QUESTION]")
            .arg(ArgSpec::positional("QUESTION").help("The question (default \"Are you sure?\")"))
            .arg(
                ArgSpec::option("default")
                    .value_name("ANSWER")
                    .choices(["yes", "no"])
                    .help("The focused answer, and the answer without a terminal"),
            )
            .arg(
                ArgSpec::option("affirmative")
                    .value_name("LABEL")
                    .help("The yes label (default Yes)"),
            )
            .arg(
                ArgSpec::option("negative")
                    .value_name("LABEL")
                    .help("The no label (default No)"),
            )
            .example(
                "rich confirm 'Deploy to production?' && ./deploy",
                "Guard a command",
            ),
        CommandSpec::new("pager")
            .about(
                "Page FILE (or standard input), keeping its colours: arrows and Space scroll, / \
                 searches, q quits; without a terminal, write it out",
            )
            .usage("pager [OPTIONS] [FILE]")
            .arg(ArgSpec::positional("FILE").help("The file; `-` or none reads standard input"))
            .arg(
                ArgSpec::option("search")
                    .value_name("QUERY")
                    .help("Start with QUERY searched"),
            )
            .example("git log --color | rich pager", "Page coloured output"),
    ]
}

/// Parsed options, for every command (each takes its own subset).
#[derive(Default)]
struct Args {
    command: &'static str,
    positionals: Vec<String>,
    header: Option<String>,
    multi: bool,
    selected: Vec<String>,
    height: Option<usize>,
    preview: Option<String>,
    value: Option<String>,
    prompt: Option<String>,
    placeholder: Option<String>,
    default: Option<String>,
    password: bool,
    /// `rich input --required`.
    required: bool,
    affirmative: Option<String>,
    negative: Option<String>,
    search: Option<String>,
    /// The global `--no-color`.
    no_color: bool,
}

fn parse_args(command: &'static str, args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        command,
        ..Args::default()
    };
    let allowed: &[&str] = match command {
        "choose" => &["--header", "--multi", "--selected", "--height", "--preview"],
        "filter" => &[
            "--header",
            "--multi",
            "--selected",
            "--height",
            "--preview",
            "--value",
        ],
        "input" => &[
            "--prompt",
            "--placeholder",
            "--value",
            "--default",
            "--password",
            "--required",
        ],
        "confirm" => &["--default", "--affirmative", "--negative"],
        _ => &["--search"],
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
        if name == "--" {
            options_done = true;
            continue;
        }
        // The global options every command takes. `--report` was read by
        // `dispatch`; here it only has to be well formed.
        match name {
            "--no-config" | "--machine-json" => continue,
            "--no-color" => {
                parsed.no_color = true;
                continue;
            }
            // Spelled as the other commands take it: `--report json`.
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
                Some(value) => Ok(value),
                None => iter
                    .next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value")),
            }
        };
        match name {
            "--header" => parsed.header = Some(value()?),
            "--multi" => parsed.multi = true,
            "--password" => parsed.password = true,
            "--required" => parsed.required = true,
            "--selected" => parsed.selected.push(value()?),
            "--height" => {
                let rows = value()?;
                parsed.height = Some(
                    rows.parse::<usize>()
                        .ok()
                        .filter(|rows| *rows > 0)
                        .ok_or_else(|| format!("--height: {rows:?} is not a positive number"))?,
                );
            }
            "--preview" => parsed.preview = Some(value()?),
            "--value" => parsed.value = Some(value()?),
            "--prompt" => parsed.prompt = Some(value()?),
            "--placeholder" => parsed.placeholder = Some(value()?),
            "--default" => parsed.default = Some(value()?),
            "--affirmative" => parsed.affirmative = Some(value()?),
            "--negative" => parsed.negative = Some(value()?),
            _ => parsed.search = Some(value()?),
        }
    }
    // Arguments that were not valid Unicode are text here (items, prompts),
    // shown lossily; only the pager's FILE is a path, read through fs_path.
    if command != "pager" {
        for positional in &mut parsed.positionals {
            *positional = text_arg(positional);
        }
    }
    for text in [
        &mut parsed.header,
        &mut parsed.preview,
        &mut parsed.value,
        &mut parsed.prompt,
        &mut parsed.placeholder,
        &mut parsed.default,
        &mut parsed.affirmative,
        &mut parsed.negative,
        &mut parsed.search,
    ]
    .into_iter()
    .flatten()
    {
        *text = text_arg(text);
    }
    for selected in &mut parsed.selected {
        *selected = text_arg(selected);
    }
    match command {
        "input" if !parsed.positionals.is_empty() => {
            return Err("`rich input` takes no arguments; the prompt is --prompt".into())
        }
        "confirm" if parsed.positionals.len() > 1 => {
            return Err("`rich confirm` takes one QUESTION; quote it".into())
        }
        "confirm" => {
            if let Some(default) = parsed.default.as_deref() {
                if !matches!(default, "yes" | "no") {
                    return Err(format!("--default: {default:?} is not yes or no"));
                }
            }
        }
        "pager" if parsed.positionals.len() > 1 => return Err("`rich pager` pages one FILE".into()),
        _ => {}
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
    let result = parse_args(command, args).map_err(|message| (ExitClass::Usage, message));
    let result = result.and_then(|args| match command {
        "choose" | "filter" => pick(&args),
        "input" => input(&args),
        "confirm" => confirm(&args),
        _ => pager(&args),
    });
    match result {
        Ok(code) => report(json, code),
        Err((ExitClass::Usage, message)) => {
            emit_error(json, ExitClass::Usage, &format!("{message} (try --help)"))
        }
        Err((class, message)) => emit_error(json, class, &message),
    }
}

/// The report envelope, on standard error, for an answered command; an
/// answer of no (exit 1) or an interrupt (130) is not a failure to report.
fn report(json: bool, code: ExitCode) -> ExitCode {
    if json && code == ExitCode::SUCCESS {
        emit_success_report(ReportFormat::Json);
    }
    code
}

type Answer = Result<ExitCode, (ExitClass, String)>;

/// Painted on standard error, inline below the cursor.
fn run_options(fallback: Fallback, no_color: bool) -> RunOptions {
    RunOptions {
        policy: Policy {
            fallback,
            tty_keys: true,
            ..Policy::default()
        },
        session: SessionOptions {
            output: Output::Stderr,
            bracketed_paste: true,
            ..SessionOptions::default()
        },
        paint: LoopOptions {
            no_color: no_color || LoopOptions::default().no_color,
            ..LoopOptions::default()
        },
    }
}

/// Print `lines` and exit 0, or report how the component ended.
fn finish(outcome: Result<Outcome<Vec<String>>, RunError>) -> Answer {
    match outcome {
        Ok(Outcome::Done(lines)) => {
            let mut out = String::new();
            for line in lines {
                out.push_str(&line);
                out.push('\n');
            }
            write_stdout(&out)
        }
        Ok(Outcome::Cancelled) => Ok(ExitCode::from(EXIT_CANCELLED)),
        Ok(Outcome::Interrupted) => Ok(ExitCode::from(EXIT_INTERRUPTED)),
        Err(error) => Err((ExitClass::Input, error.to_string())),
    }
}

fn write_stdout(text: &str) -> Answer {
    use std::io::Write;
    let mut stdout = std::io::stdout().lock();
    match stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
    {
        Ok(()) => Ok(ExitCode::SUCCESS),
        // A consumer such as `head` may stop reading: that is success.
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
        Err(error) => Err((
            ExitClass::Input,
            format!("could not write the answer: {error}"),
        )),
    }
}

/// At most `limit` bytes of `reader`, or an input error (exit 3) naming
/// `what` when there is more.
fn read_bounded(
    reader: impl Read,
    limit: usize,
    what: &str,
) -> Result<Vec<u8>, (ExitClass, String)> {
    let mut bytes = Vec::new();
    reader
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| (ExitClass::Input, format!("could not read {what}: {error}")))?;
    if bytes.len() > limit {
        return Err((
            ExitClass::Input,
            format!("{what} is over the {} MiB limit", limit / (1024 * 1024)),
        ));
    }
    Ok(bytes)
}

/// Standard input, up to [`MAX_INPUT_BYTES`], as text: invalid UTF-8 is
/// replaced rather than failing the command.
fn read_stdin() -> Result<String, (ExitClass, String)> {
    let bytes = read_bounded(std::io::stdin().lock(), MAX_INPUT_BYTES, "standard input")?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `choose` and `filter`.
fn pick(args: &Args) -> Answer {
    let from_stdin = args.positionals.is_empty() || args.positionals == ["-"];
    let items: Vec<String> = if from_stdin {
        if std::io::stdin().is_terminal() {
            return Err((
                ExitClass::Usage,
                format!(
                    "`rich {}` needs ITEMs, or lines on standard input",
                    args.command
                ),
            ));
        }
        let text = read_stdin()?;
        if text.lines().nth(MAX_ITEMS).is_some() {
            return Err((
                ExitClass::Input,
                format!("standard input is over the {MAX_ITEMS} line limit"),
            ));
        }
        // `filter` keeps every line, as grep would; a blank choice is no
        // choice.
        text.lines()
            .filter(|line| args.command == "filter" || !line.trim().is_empty())
            .map(str::to_string)
            .collect()
    } else {
        args.positionals.clone()
    };
    if items.is_empty() {
        return Err((ExitClass::Usage, "no items to choose from".into()));
    }
    let options = run_options(
        if from_stdin {
            // Standard input was the list, so there is nothing left to ask on.
            Fallback::Default
        } else {
            Fallback::Prompt
        },
        args.no_color,
    );
    // Without a terminal, `filter` is a filter: the matching lines, best
    // first.
    if args.command == "filter" && options.policy.detect_for(options.session.output).is_err() {
        let query = args.value.as_deref().unwrap_or("");
        let ranked = rich_interact::fuzzy::rank(query, items.iter().map(String::as_str));
        let out: String = ranked
            .into_iter()
            .map(|(index, _)| format!("{}\n", items[index]))
            .collect();
        return write_stdout(&out);
    }
    let prompt = args.header.clone().unwrap_or_else(|| match args.command {
        "filter" => "Filter".to_string(),
        _ => "Choose".to_string(),
    });
    let entries: Vec<Item<String>> = items
        .iter()
        .map(|item| {
            // Items are often names from elsewhere (files, branches): paint
            // their controls as text, but answer with the item as it came.
            let entry = Item::new(item.clone(), sanitize_terminal_controls(item));
            match &args.preview {
                Some(command) => entry.preview(Preview::Renderable(Arc::new(CommandPreview::new(
                    command.clone(),
                    item.clone(),
                )))),
                None => entry,
            }
        })
        .collect();
    let selected: Vec<usize> = args
        .selected
        .iter()
        .filter_map(|wanted| items.iter().position(|item| item == wanted))
        .collect();
    if args.multi {
        // Nothing marked is no default: without a terminal, say so (exit 3)
        // rather than answer with nothing.
        if selected.is_empty() && options.policy.fallback == Fallback::Default {
            if let Err(reason) = options.policy.detect_for(options.session.output) {
                return Err((
                    ExitClass::Input,
                    RunError::NotInteractive(NotInteractive::NoDefault(reason)).to_string(),
                ));
            }
        }
        let mut select = MultiSelect::new(prompt, entries).marked(selected);
        if args.preview.is_some() {
            select = select.repaint_every(PREVIEW_REPAINT);
        }
        if let Some(rows) = args.height {
            select = select.height(rows);
        }
        if let Some(query) = &args.value {
            select = select.query(query.clone());
        }
        finish(rich_interact::run(select, &options))
    } else {
        let mut select = Select::new(prompt, entries);
        if args.preview.is_some() {
            select = select.repaint_every(PREVIEW_REPAINT);
        }
        if let Some(&index) = selected.first() {
            select = select.default(index);
        }
        if let Some(rows) = args.height {
            select = select.height(rows);
        }
        if let Some(query) = &args.value {
            select = select.query(query.clone());
        }
        finish(
            rich_interact::run(select, &options).map(|outcome| match outcome {
                Outcome::Done(value) => Outcome::Done(vec![value]),
                Outcome::Cancelled => Outcome::Cancelled,
                Outcome::Interrupted => Outcome::Interrupted,
            }),
        )
    }
}

fn input(args: &Args) -> Answer {
    let prompt = args.prompt.clone().unwrap_or_else(|| "Input".into());
    let mut input = if args.password {
        Input::masked(prompt)
    } else {
        Input::new(prompt)
    };
    if let Some(placeholder) = &args.placeholder {
        input = input.placeholder(placeholder.clone());
    }
    if let Some(value) = &args.value {
        input = input.value(value.clone());
    }
    if let Some(default) = &args.default {
        input = input.default(default.clone());
    }
    if args.required {
        input = input.validate(|answer| {
            if answer.trim().is_empty() {
                Err("an answer is required".into())
            } else {
                Ok(())
            }
        });
    }
    let outcome = rich_interact::run(input, &run_options(Fallback::Prompt, args.no_color));
    finish(outcome.map(|outcome| match outcome {
        Outcome::Done(line) => Outcome::Done(vec![line]),
        Outcome::Cancelled => Outcome::Cancelled,
        Outcome::Interrupted => Outcome::Interrupted,
    }))
}

fn confirm(args: &Args) -> Answer {
    let question = args
        .positionals
        .first()
        .cloned()
        .unwrap_or_else(|| "Are you sure?".into());
    let yes = args.affirmative.clone().unwrap_or_else(|| "Yes".into());
    let no = args.negative.clone().unwrap_or_else(|| "No".into());
    let mut sheet =
        Confirm::new(question).choices([Choice::new("yes", yes, 'y'), Choice::new("no", no, 'n')]);
    if let Some(default) = args.default.as_deref() {
        sheet = sheet.default(default);
    }
    match rich_interact::run(sheet, &run_options(Fallback::Prompt, args.no_color)) {
        Ok(Outcome::Done(answer)) if answer == "yes" => Ok(ExitCode::SUCCESS),
        Ok(Outcome::Done(_)) | Ok(Outcome::Cancelled) => Ok(ExitCode::from(EXIT_CANCELLED)),
        Ok(Outcome::Interrupted) => Ok(ExitCode::from(EXIT_INTERRUPTED)),
        Err(error) => Err((ExitClass::Input, error.to_string())),
    }
}

fn pager(args: &Args) -> Answer {
    let path = args
        .positionals
        .first()
        .map(String::as_str)
        .filter(|path| *path != "-");
    if path.is_none() && std::io::stdin().is_terminal() {
        return Err((
            ExitClass::Usage,
            "`rich pager` needs a FILE, or text on standard input".into(),
        ));
    }
    let open = |path: &str| {
        std::fs::File::open(fs_path(path))
            .map_err(|error| (ExitClass::Input, format!("could not read {path}: {error}")))
    };
    // The pager owns standard output: without a terminal there, it is only
    // the content.
    let options = RunOptions {
        policy: Policy {
            tty_keys: true,
            ..Policy::default()
        },
        session: SessionOptions {
            alternate_screen: true,
            mouse: true,
            ..SessionOptions::default()
        },
        paint: LoopOptions {
            no_color: args.no_color || LoopOptions::default().no_color,
            ..LoopOptions::default()
        },
    };
    if options.policy.detect_for(Output::Stdout).is_err() {
        // Byte for byte, whatever the encoding, streamed rather than held.
        let copied = match path {
            Some(path) => std::io::copy(&mut open(path)?, &mut std::io::stdout().lock()),
            None => std::io::copy(&mut std::io::stdin().lock(), &mut std::io::stdout().lock()),
        };
        return match copied {
            Ok(_) => Ok(ExitCode::SUCCESS),
            Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => Ok(ExitCode::SUCCESS),
            Err(error) => Err((
                ExitClass::Input,
                format!("could not copy the content: {error}"),
            )),
        };
    }
    let source = match path {
        Some(path) => {
            String::from_utf8_lossy(&read_bounded(open(path)?, MAX_INPUT_BYTES, path)?).into_owned()
        }
        None => read_stdin()?,
    };
    // A file's last newline ends its last line; it is not an empty line
    // after it, as a pager such as `less` shows it.
    let source = source.strip_suffix('\n').unwrap_or(&source);
    let text = Text::from_ansi(source, rich::style::StyleType::default());
    let mut pager = Pager::new(text);
    if let Some(query) = &args.search {
        pager = pager.search(query.clone());
    }
    match rich_interact::run(pager, &options) {
        Ok(Outcome::Interrupted) => Ok(ExitCode::from(EXIT_INTERRUPTED)),
        Ok(_) => Ok(ExitCode::SUCCESS),
        Err(error) => Err((ExitClass::Input, error.to_string())),
    }
}

/// How often a picker with `--preview` repaints, so a preview shows as
/// soon as its command finishes.
const PREVIEW_REPAINT: Duration = Duration::from_millis(100);

/// Where a preview command has got to.
enum PreviewState {
    /// Not drawn yet.
    Idle,
    Running,
    Done(Text),
}

/// `--preview COMMAND` for one item: the command starts the first time the
/// item's preview is drawn, at the pane's width, on a thread of its own, so
/// a slow command never holds up the keys (Ctrl+C included); the pane says
/// it is running until the output (colours kept) arrives, and reuses it
/// after that. A command still running after [`PREVIEW_TIMEOUT`] is killed,
/// and at most [`PREVIEW_MAX_BYTES`] of its output is kept.
struct CommandPreview {
    command: String,
    item: String,
    state: Arc<Mutex<PreviewState>>,
    /// The running command, killed when the picker is dropped.
    child: Arc<Mutex<Option<std::process::Child>>>,
    /// How long the command may run: [`PREVIEW_TIMEOUT`], shorter in tests.
    timeout: Duration,
}

impl CommandPreview {
    fn new(command: String, item: String) -> CommandPreview {
        CommandPreview {
            command,
            item,
            state: Arc::new(Mutex::new(PreviewState::Idle)),
            child: Arc::new(Mutex::new(None)),
            timeout: PREVIEW_TIMEOUT,
        }
    }

    /// Start the command for a pane `width` columns wide.
    fn start(&self, width: usize) -> Result<(), String> {
        let command = if self.command.contains("{}") {
            self.command
                .replace("{}", &quote_item(&self.item, cfg!(windows))?)
        } else {
            self.command.clone()
        };
        #[cfg(windows)]
        let mut process = {
            use std::os::windows::process::CommandExt;
            // `/S /C "…"`: cmd strips the outer quotes and runs the rest
            // as typed, instead of the C runtime's escaping it ignores.
            let mut process = std::process::Command::new("cmd");
            process
                .args(["/D", "/S", "/C"])
                .raw_arg(format!("\"{command}\""));
            process
        };
        #[cfg(not(windows))]
        let mut process = {
            use std::os::unix::process::CommandExt;
            let mut process = std::process::Command::new("sh");
            // A process group of its own, so a timeout or the picker's end
            // kills what the command started (`a | b`, `x & wait`) too.
            process.arg("-c").arg(&command).process_group(0);
            process
        };
        let mut child = process
            .env("COLUMNS", width.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("preview failed: {error}"))?;
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let readers = [
            child
                .stdout
                .take()
                .map(|pipe| drain(pipe, Arc::clone(&stdout))),
            child
                .stderr
                .take()
                .map(|pipe| drain(pipe, Arc::clone(&stderr))),
        ];
        *self.child.lock().unwrap_or_else(|e| e.into_inner()) = Some(child);
        let (state, child) = (Arc::clone(&self.state), Arc::clone(&self.child));
        let timeout = self.timeout;
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut timed_out = false;
            let status = loop {
                std::thread::sleep(Duration::from_millis(20));
                let mut guard = child.lock().unwrap_or_else(|e| e.into_inner());
                // Taken: the picker is gone.
                let Some(running) = guard.as_mut() else {
                    return;
                };
                match running.try_wait() {
                    Ok(Some(status)) => break Some(status),
                    Ok(None) if started.elapsed() < timeout => {}
                    Ok(None) | Err(_) => {
                        timed_out = true;
                        kill_tree(running);
                        break running.wait().ok();
                    }
                }
            };
            child.lock().unwrap_or_else(|e| e.into_inner()).take();
            // The rest of the output, for a moment: a background process
            // the command left may hold the pipes open.
            let deadline = Instant::now() + Duration::from_millis(200);
            while readers.iter().flatten().any(|reader| !reader.is_finished())
                && Instant::now() < deadline
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            let take = |buffer: &Arc<Mutex<Vec<u8>>>| {
                std::mem::take(&mut *buffer.lock().unwrap_or_else(|e| e.into_inner()))
            };
            let mut bytes = take(&stdout);
            if !status.is_some_and(|status| status.success()) {
                bytes.extend(take(&stderr));
            }
            let cut = bytes.len() > PREVIEW_MAX_BYTES;
            bytes.truncate(PREVIEW_MAX_BYTES);
            let text = String::from_utf8_lossy(&bytes);
            let mut lines: Vec<&str> = text.lines().take(PREVIEW_MAX_LINES).collect();
            while lines.last().is_some_and(|line| line.is_empty()) {
                lines.pop();
            }
            let mut shown = lines.join("\n");
            if timed_out {
                shown.push_str(&format!(
                    "\n(preview timed out after {} s)",
                    timeout.as_secs()
                ));
            } else if cut {
                shown.push_str("\n(preview cut at 1 MiB)");
            }
            let text = Text::from_ansi(
                shown.trim_start_matches('\n'),
                rich::style::StyleType::default(),
            );
            *state.lock().unwrap_or_else(|e| e.into_inner()) = PreviewState::Done(text);
        });
        Ok(())
    }
}

/// Read `pipe` into `sink` until it ends or holds more than
/// [`PREVIEW_MAX_BYTES`]; then the pipe is closed, and a command still
/// writing gets a broken pipe.
fn drain(
    mut pipe: impl Read + Send + 'static,
    sink: Arc<Mutex<Vec<u8>>>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => {
                    let mut sink = sink.lock().unwrap_or_else(|e| e.into_inner());
                    let room = (PREVIEW_MAX_BYTES + 1).saturating_sub(sink.len());
                    sink.extend_from_slice(&buffer[..read.min(room)]);
                    if read >= room {
                        return;
                    }
                }
            }
        }
    })
}

impl Drop for CommandPreview {
    fn drop(&mut self) {
        let running = self.child.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(mut child) = running {
            kill_tree(&mut child);
            let _ = child.wait();
        }
    }
}

/// Kill a preview command and everything it started: on Unix its process
/// group (the command leads one of its own), on Windows its process tree.
#[allow(unsafe_code)]
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        if let Ok(pgid) = libc::pid_t::try_from(child.id()) {
            // SAFETY: `kill` takes plain integers; a negative pid names the
            // process group the child leads, and it has not been reaped
            // yet, so the id cannot have been reused.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &child.id().to_string()])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    let _ = child.kill();
}

impl Renderable for CommandPreview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        let text = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            match &*state {
                PreviewState::Done(text) => text.clone(),
                PreviewState::Running => Text::new("…"),
                PreviewState::Idle => {
                    *state = PreviewState::Running;
                    drop(state);
                    match self.start(options.max_width) {
                        Ok(()) => Text::new("…"),
                        Err(message) => {
                            let text = Text::new(message);
                            *self.state.lock().unwrap_or_else(|e| e.into_inner()) =
                                PreviewState::Done(text.clone());
                            text
                        }
                    }
                }
            }
        };
        text.rich_render(console, options)
    }
}

/// `item` as one word for the shell `--preview` runs: `sh` or, when
/// `windows`, `cmd.exe`. cmd has no quoting that keeps `%`, `!`, `"` and
/// the like literal inside a double-quoted word, so an item holding one of
/// them (or a line break) is refused rather than run.
fn quote_item(item: &str, windows: bool) -> Result<String, String> {
    if !windows {
        return Ok(format!("'{}'", item.replace('\'', "'\\''")));
    }
    let unsafe_for_cmd = |c: char| {
        matches!(
            c,
            '"' | '%' | '!' | '^' | '&' | '|' | '<' | '>' | '(' | ')' | '\n' | '\r'
        ) || c.is_control()
    };
    if item.contains(unsafe_for_cmd) {
        return Err(format!(
            "no preview: {:?} holds characters cmd.exe cannot be given safely",
            sanitize_terminal_controls(item)
        ));
    }
    Ok(format!("\"{item}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn a_command_word_is_found_after_global_options() {
        assert_eq!(
            requested(&args(&["--no-color", "choose", "a"])),
            Some("choose")
        );
        assert_eq!(requested(&args(&["README.md"])), None);
        assert_eq!(requested(&args(&["--", "choose"])), None);
    }

    #[test]
    fn options_are_checked_per_command() {
        let parsed = parse_args("choose", &args(&["choose", "--multi", "a", "b"])).unwrap();
        assert!(parsed.multi);
        assert_eq!(parsed.positionals, ["a", "b"]);
        assert!(parse_args("input", &args(&["input", "--multi"])).is_err());
        assert!(parse_args("confirm", &args(&["confirm", "--default", "maybe"])).is_err());
        assert!(parse_args("choose", &args(&["choose", "--height", "0"])).is_err());
    }

    #[test]
    fn items_are_quoted_as_one_shell_word() {
        assert_eq!(quote_item("it's", false).unwrap(), "'it'\\''s'");
        assert_eq!(
            quote_item("$(touch x) `y` \"z\"", false).unwrap(),
            "'$(touch x) `y` \"z\"'"
        );
    }

    #[test]
    fn items_cmd_cannot_quote_are_refused() {
        assert_eq!(
            quote_item("My Documents\\a b.txt", true).unwrap(),
            "\"My Documents\\a b.txt\""
        );
        for item in [
            "x\" & calc & \"",
            "%PATH%",
            "!x!",
            "a^b",
            "a|b",
            "a<b",
            "a>b",
            "a&b",
            "a\nb",
            "(x)",
        ] {
            assert!(quote_item(item, true).is_err(), "{item:?}");
        }
    }

    /// A preview command that leaves a grandchild (`sleep 30 &`), and the
    /// file the grandchild's pid is written to.
    #[cfg(target_os = "linux")]
    fn preview_with_grandchild(dir: &std::path::Path) -> (CommandPreview, std::path::PathBuf) {
        let pidfile = dir.join("pid");
        let path = quote_item(pidfile.to_str().unwrap(), false).unwrap();
        let command = format!("sleep 30 & echo $! > {path}; wait");
        (CommandPreview::new(command, String::new()), pidfile)
    }

    /// The pid in `pidfile`, once the command has written it.
    #[cfg(target_os = "linux")]
    fn grandchild_pid(pidfile: &std::path::Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(pid) = std::fs::read_to_string(pidfile)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "no pid written");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Whether `pid` is still running: present and not a zombie waiting
    /// for a parent (a container's init may never reap it).
    #[cfg(target_os = "linux")]
    fn running(pid: u32) -> bool {
        std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
            stat.rsplit_once(')')
                .is_some_and(|(_, rest)| !rest.trim_start().starts_with('Z'))
        })
    }

    #[cfg(target_os = "linux")]
    fn wait_until_gone(pid: u32) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while running(pid) {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn preview_timeout_kills_the_commands_background_processes() {
        let dir = tempfile::tempdir().unwrap();
        let (mut preview, pidfile) = preview_with_grandchild(dir.path());
        preview.timeout = Duration::from_millis(300);
        preview.start(80).unwrap();
        let pid = grandchild_pid(&pidfile);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !matches!(
            *preview.state.lock().unwrap(),
            PreviewState::Done(ref text) if text.plain().contains("timed out")
        ) {
            assert!(Instant::now() < deadline, "the preview never timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        let gone = wait_until_gone(pid);
        if !gone {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
        assert!(gone, "the preview's `sleep 30` outlived its timeout");
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn dropping_a_preview_kills_the_commands_background_processes() {
        let dir = tempfile::tempdir().unwrap();
        let (preview, pidfile) = preview_with_grandchild(dir.path());
        preview.start(80).unwrap();
        let pid = grandchild_pid(&pidfile);
        drop(preview);
        let gone = wait_until_gone(pid);
        if !gone {
            let _ = std::process::Command::new("kill")
                .arg(pid.to_string())
                .status();
        }
        assert!(gone, "the preview's `sleep 30` outlived the picker");
    }
}

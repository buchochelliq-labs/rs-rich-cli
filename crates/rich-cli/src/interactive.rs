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
use std::sync::{Arc, OnceLock};

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
            .example(
                "name=$(rich input --prompt Name --placeholder 'Ada Lovelace')",
                "A name",
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

/// Standard input, whole.
fn read_stdin() -> Result<String, (ExitClass, String)> {
    let mut text = String::new();
    std::io::stdin()
        .read_to_string(&mut text)
        .map_err(|error| {
            (
                ExitClass::Input,
                format!("could not read standard input: {error}"),
            )
        })?;
    Ok(text)
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
        // `filter` keeps every line, as grep would; a blank choice is no
        // choice.
        read_stdin()?
            .lines()
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
                Some(command) => entry.preview(Preview::Renderable(Arc::new(CommandPreview {
                    command: command.clone(),
                    item: item.clone(),
                    output: OnceLock::new(),
                }))),
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
        if let Some(rows) = args.height {
            select = select.height(rows);
        }
        if let Some(query) = &args.value {
            select = select.query(query.clone());
        }
        finish(rich_interact::run(select, &options))
    } else {
        let mut select = Select::new(prompt, entries);
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
    let source = match args.positionals.first().map(String::as_str) {
        None | Some("-") => {
            if std::io::stdin().is_terminal() {
                return Err((
                    ExitClass::Usage,
                    "`rich pager` needs a FILE, or text on standard input".into(),
                ));
            }
            read_stdin()?
        }
        Some(path) => std::fs::read(fs_path(path))
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|error| (ExitClass::Input, format!("could not read {path}: {error}")))?,
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
        return write_stdout(&source);
    }
    let text = Text::from_ansi(&source, rich::style::StyleType::default());
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

/// `--preview COMMAND` for one item: the command runs the first time the
/// item's preview is drawn, at the pane's width, and its output (colours
/// kept) is reused after that.
struct CommandPreview {
    command: String,
    item: String,
    output: OnceLock<Text>,
}

impl CommandPreview {
    fn run(&self, width: usize) -> Text {
        let command = self.command.replace("{}", &shell_quote(&self.item));
        #[cfg(windows)]
        let mut process = {
            let mut process = std::process::Command::new("cmd");
            process.arg("/C").arg(&command);
            process
        };
        #[cfg(not(windows))]
        let mut process = {
            let mut process = std::process::Command::new("sh");
            process.arg("-c").arg(&command);
            process
        };
        let output = process
            .env("COLUMNS", width.to_string())
            .stdin(std::process::Stdio::null())
            .output();
        match output {
            Ok(output) => {
                let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
                if !output.status.success() {
                    text.push_str(&String::from_utf8_lossy(&output.stderr));
                }
                Text::from_ansi(
                    text.trim_end_matches('\n'),
                    rich::style::StyleType::default(),
                )
            }
            Err(error) => Text::new(format!("preview failed: {error}")),
        }
    }
}

impl Renderable for CommandPreview {
    fn rich_render(&self, console: &Console, options: &ConsoleOptions) -> Vec<Segment> {
        self.output
            .get_or_init(|| self.run(options.max_width))
            .rich_render(console, options)
    }
}

/// `item` as one shell word.
fn shell_quote(item: &str) -> String {
    if cfg!(windows) {
        format!("\"{}\"", item.replace('"', "\\\""))
    } else {
        format!("'{}'", item.replace('\'', "'\\''"))
    }
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
        if !cfg!(windows) {
            assert_eq!(shell_quote("it's"), "'it'\\''s'");
        }
    }
}

//! The clap adapter (feature `clap`): build a [`CommandSpec`] from a
//! `clap::Command`, map `clap::Error`s to [`CliError`]s, and parse with
//! help, version and errors rendered through rich.
//!
//! clap is used with `default-features = false` and only `std`, `help`,
//! `env`, `error-context` and `suggestions`. rich renders help, usage and
//! colour, but without `help` clap leaves out its `-h`/`--help` flag and
//! `help` subcommand; `usage` and `color` are not needed. `env` exposes
//! `Arg::get_env`; `error-context` and `suggestions` fill `Error::context()`
//! with the invalid argument or value and clap's "did you mean" candidates.
//!
//! ```
//! use clap::{Arg, Command};
//! use rich::Console;
//! use rich_ext::cli_doc::clap::try_parse_with;
//!
//! let cmd = Command::new("tool").arg(Arg::new("color").long("color"));
//! let console = Console::builder().width(60).build();
//! let exit = try_parse_with(&console, cmd, ["tool", "--colr"]).unwrap_err();
//! assert_eq!(exit.code, 2);
//! assert!(exit.output.starts_with("error: unexpected argument '--colr'\n"));
//! assert!(exit.output.contains("help: a similar argument exists: '--color'"));
//! ```

use super::error::{CliError, CliErrorKind};
use super::help::HelpView;
use super::spec::{ArgSpec, Choice, CommandSpec, ValueHint};
use clap::error::{ContextKind, ContextValue, ErrorKind};
use clap::{ArgAction, ArgMatches, Command};
use rich::{Console, Text};
use std::ffi::OsString;
use std::io::{IsTerminal, Write};

impl CommandSpec {
    /// The spec of a clap command and its subcommands, including clap's
    /// generated `--help`/`--version` flags and `help` subcommand.
    ///
    /// Maps long, short and visible aliases, value names, possible values
    /// (with their help), value hints, defaults, environment variables, help
    /// headings, hidden, required and positional arguments; `Count` and
    /// `Append` actions (or more than one value) make an argument `multiple`.
    /// `about`, `long_about`, `version`, an overridden usage and the
    /// subcommand heading carry over; `after_help` (else `after_long_help`)
    /// becomes an untitled section.
    pub fn from_clap(cmd: &clap::Command) -> CommandSpec {
        let mut cmd = cmd.clone();
        cmd.build();
        let mut spec = convert(&cmd);
        spec.bin_name = cmd
            .get_bin_name()
            .filter(|bin| *bin != cmd.get_name())
            .map(str::to_string);
        spec
    }
}

fn convert(cmd: &Command) -> CommandSpec {
    let text = |s: Option<&clap::builder::StyledStr>| s.map(|s| s.to_string());
    let mut spec = CommandSpec::new(cmd.get_name());
    spec.version = cmd.get_version().map(str::to_string);
    spec.about = text(cmd.get_about()).unwrap_or_default();
    spec.long_about = text(cmd.get_long_about());
    spec.aliases = cmd.get_visible_aliases().map(str::to_string).collect();
    if let Some(usage) = cmd.get_overridden_usage() {
        spec.usage = usage
            .to_string()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
    }
    spec.args = cmd.get_arguments().map(convert_arg).collect();
    spec.subcommands = cmd.get_subcommands().map(convert).collect();
    spec.subcommand_heading = cmd.get_subcommand_help_heading().map(str::to_string);
    spec.hidden = cmd.is_hide_set();
    spec.subcommand_required = cmd.is_subcommand_required_set();
    if let Some(after) = text(cmd.get_after_help()).or_else(|| text(cmd.get_after_long_help())) {
        spec = spec.section("", after);
    }
    spec
}

fn convert_arg(arg: &clap::Arg) -> ArgSpec {
    let action = arg.get_action();
    let takes = action.takes_values();
    let mut spec = ArgSpec::new(arg.get_id().as_str());
    spec.long = arg.get_long().map(str::to_string);
    spec.short = arg.get_short();
    spec.aliases = arg
        .get_visible_aliases()
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect();
    spec.positional = arg.is_positional();
    spec.global = arg.is_global_set();
    spec.help = arg.get_help().map(|h| h.to_string()).unwrap_or_default();
    spec.long_help = arg.get_long_help().map(|h| h.to_string());
    spec.required = arg.is_required_set();
    spec.hidden = arg.is_hide_set();
    spec.heading = arg.get_help_heading().map(str::to_string);
    spec.multiple = matches!(action, ArgAction::Count | ArgAction::Append)
        || arg.get_num_args().is_some_and(|n| n.max_values() > 1);
    if !arg.is_hide_env_set() {
        spec.env = arg.get_env().map(|e| e.to_string_lossy().into_owned());
    }
    if takes {
        let names: Vec<String> = arg
            .get_value_names()
            .map(|names| names.iter().map(|n| n.to_string()).collect())
            .unwrap_or_default();
        // clap shows an unnamed value by its id, unchanged: `<name>`.
        spec.value_name = Some(if names.is_empty() {
            arg.get_id().as_str().to_string()
        } else {
            names.join(" ")
        });
        let choices: Vec<Choice> = arg
            .get_possible_values()
            .into_iter()
            .filter(|v| !v.is_hide_set())
            .map(|v| {
                Choice::new(v.get_name())
                    .help(v.get_help().map(|h| h.to_string()).unwrap_or_default())
            })
            .collect();
        spec.value = if !choices.is_empty() && !arg.is_hide_possible_values_set() {
            ValueHint::Choices(choices)
        } else {
            match arg.get_value_hint() {
                clap::ValueHint::FilePath => ValueHint::File,
                clap::ValueHint::DirPath => ValueHint::Dir,
                clap::ValueHint::AnyPath => ValueHint::Path,
                clap::ValueHint::ExecutablePath
                | clap::ValueHint::CommandName
                | clap::ValueHint::CommandString
                | clap::ValueHint::CommandWithArguments => ValueHint::Command,
                clap::ValueHint::Url => ValueHint::Url,
                _ => ValueHint::Any,
            }
        };
        let defaults: Vec<String> = arg
            .get_default_values()
            .iter()
            .map(|d| d.to_string_lossy().into_owned())
            .collect();
        if !defaults.is_empty() && !arg.is_hide_default_value_set() {
            spec.default = Some(defaults.join(", "));
        }
    }
    spec
}

impl CliError {
    /// A [`CliError`] from a clap error. The kind maps to the closest
    /// [`CliErrorKind`]; the invalid argument, value, prior argument (for a
    /// conflict), possible values and suggestions come from the error's
    /// context. With `cmd`, the usage and a `--help` tip come from it.
    pub fn from_clap(err: &clap::Error, cmd: Option<&Command>) -> CliError {
        let get = |kind: ContextKind| -> Option<String> {
            match err.get(kind)? {
                ContextValue::String(s) => Some(s.clone()),
                ContextValue::Strings(v) => Some(v.join(", ")),
                ContextValue::StyledStr(s) => Some(s.to_string()),
                ContextValue::StyledStrs(v) => Some(
                    v.iter()
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                ),
                ContextValue::Number(n) => Some(n.to_string()),
                ContextValue::Bool(b) => Some(b.to_string()),
                _ => None,
            }
        };
        let list = |kind: ContextKind| -> Vec<String> {
            match err.get(kind) {
                Some(ContextValue::String(s)) => vec![s.clone()],
                Some(ContextValue::Strings(v)) => v.clone(),
                _ => Vec::new(),
            }
        };
        let argument = get(ContextKind::InvalidArg).or_else(|| get(ContextKind::InvalidSubcommand));
        let value = get(ContextKind::InvalidValue);
        let (kind, message) = match err.kind() {
            ErrorKind::UnknownArgument => (CliErrorKind::UnknownArgument, String::new()),
            ErrorKind::InvalidSubcommand => (CliErrorKind::UnknownSubcommand, String::new()),
            ErrorKind::InvalidValue if value.as_deref() == Some("") => {
                (CliErrorKind::MissingValue, String::new())
            }
            ErrorKind::InvalidValue => (CliErrorKind::InvalidValue, String::new()),
            ErrorKind::ValueValidation => {
                let reason = std::error::Error::source(err).map(|e| e.to_string());
                let message = format!(
                    "invalid value '{}' for '{}'{}",
                    value.as_deref().unwrap_or_default(),
                    argument.as_deref().unwrap_or_default(),
                    reason.map(|r| format!(": {r}")).unwrap_or_default()
                );
                (CliErrorKind::InvalidValue, message)
            }
            ErrorKind::NoEquals => (
                CliErrorKind::MissingValue,
                format!(
                    "equal sign is needed when assigning values to '{}'",
                    argument.as_deref().unwrap_or_default()
                ),
            ),
            ErrorKind::TooManyValues => (CliErrorKind::UnexpectedValue, String::new()),
            ErrorKind::TooFewValues | ErrorKind::WrongNumberOfValues => {
                let expected = get(ContextKind::ExpectedNumValues)
                    .or_else(|| get(ContextKind::MinValues))
                    .unwrap_or_default();
                let actual = get(ContextKind::ActualNumValues).unwrap_or_default();
                let message = format!(
                    "{expected} values required by '{}'; {actual} provided",
                    argument.as_deref().unwrap_or_default()
                );
                (CliErrorKind::MissingValue, message)
            }
            ErrorKind::ArgumentConflict => (CliErrorKind::Conflict, String::new()),
            ErrorKind::MissingRequiredArgument => (CliErrorKind::MissingRequired, String::new()),
            ErrorKind::MissingSubcommand => {
                let name = cmd.map(|c| c.get_name()).unwrap_or("command");
                let message = format!("'{name}' requires a subcommand but one was not provided");
                (CliErrorKind::MissingRequired, message)
            }
            _ => {
                let rendered = err.to_string();
                let first = rendered.lines().next().unwrap_or_default();
                let message = first.strip_prefix("error: ").unwrap_or(first).to_string();
                (CliErrorKind::Other, message)
            }
        };
        let mut error = CliError::new(kind, message);
        error.argument = argument;
        error.value = match kind {
            CliErrorKind::Conflict => get(ContextKind::PriorArg),
            _ => value.filter(|v| !v.is_empty()),
        };
        for context in [
            ContextKind::SuggestedArg,
            ContextKind::SuggestedSubcommand,
            ContextKind::SuggestedValue,
            ContextKind::SuggestedCommand,
        ] {
            error.suggestions.extend(list(context));
        }
        error.possible_values = list(ContextKind::ValidValue);
        error.usage = get(ContextKind::Usage)
            .map(|u| u.trim_start_matches("Usage:").trim().to_string())
            .or_else(|| cmd.map(|c| CommandSpec::from_clap(c).usage_lines().join("\n")));
        if cmd.is_some_and(|c| !c.is_disable_help_flag_set()) {
            error.help_flag = Some("--help".into());
        }
        error
    }
}

/// Print `cmd`'s help through rich to stdout.
pub fn print_help(cmd: &Command) {
    Console::new().print(&HelpView::new(&CommandSpec::from_clap(cmd)));
}

/// Why parsing stopped without matches: help, a version, or an error,
/// already rendered.
#[derive(Debug)]
pub struct ParseExit {
    /// The clap error that stopped parsing (its kind says which case).
    pub error: clap::Error,
    /// The rendered help, version or diagnostic, ending in a newline.
    pub output: String,
    /// Whether the output belongs on stderr (errors) or stdout.
    pub use_stderr: bool,
    /// The process exit status: 0 for help and version, 2 for errors.
    pub code: i32,
}

impl ParseExit {
    /// Write the output to stdout or stderr.
    pub fn print(&self) {
        let _ = if self.use_stderr {
            std::io::stderr().lock().write_all(self.output.as_bytes())
        } else {
            std::io::stdout().lock().write_all(self.output.as_bytes())
        };
    }

    /// Print the output and exit with [`code`](Self::code).
    pub fn exit(&self) -> ! {
        self.print();
        let _ = std::io::stdout().flush();
        std::process::exit(self.code)
    }
}

impl std::fmt::Display for ParseExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.error.fmt(f)
    }
}

impl std::error::Error for ParseExit {}

/// Parse `std::env::args_os()`; on help, version or an error, print it
/// through rich and exit as clap does (0 on stdout, or 2 on stderr).
pub fn parse_or_exit(cmd: Command) -> ArgMatches {
    let spec = CommandSpec::from_clap(&cmd);
    parse_or_exit_with_spec(cmd, &spec)
}

/// [`parse_or_exit`] with help drawn from `spec`, usually
/// [`CommandSpec::from_clap`] extended with what clap cannot describe:
/// examples, sections, config keys.
pub fn parse_or_exit_with_spec(cmd: Command, spec: &CommandSpec) -> ArgMatches {
    let args: Vec<OsString> = std::env::args_os().collect();
    match cmd.clone().try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(err) => {
            let console = if err.use_stderr() {
                Console::builder()
                    .force_terminal(std::io::stderr().is_terminal())
                    .build()
            } else {
                Console::new()
            };
            render_exit(&console, &cmd, spec, &args, err).exit()
        }
    }
}

/// Parse `args`, rendering help and version for stdout and errors for
/// stderr with consoles detected for those streams. Nothing is printed.
pub fn try_parse_from<I, T>(cmd: Command, args: I) -> Result<ArgMatches, ParseExit>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    match cmd.clone().try_get_matches_from(&args) {
        Ok(matches) => Ok(matches),
        Err(err) => {
            let console = if err.use_stderr() {
                Console::builder()
                    .force_terminal(std::io::stderr().is_terminal())
                    .build()
            } else {
                Console::new()
            };
            let spec = CommandSpec::from_clap(&cmd);
            Err(render_exit(&console, &cmd, &spec, &args, err))
        }
    }
}

/// [`try_parse_from`] rendering with `console`, for tests and for callers
/// that configure their own console. Nothing is printed.
pub fn try_parse_with<I, T>(
    console: &Console,
    cmd: Command,
    args: I,
) -> Result<ArgMatches, ParseExit>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let spec = CommandSpec::from_clap(&cmd);
    try_parse_with_spec(console, cmd, &spec, args)
}

/// [`try_parse_with`] with help drawn from `spec` (see
/// [`parse_or_exit_with_spec`]).
pub fn try_parse_with_spec<I, T>(
    console: &Console,
    cmd: Command,
    spec: &CommandSpec,
    args: I,
) -> Result<ArgMatches, ParseExit>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
    match cmd.clone().try_get_matches_from(&args) {
        Ok(matches) => Ok(matches),
        Err(err) => Err(render_exit(console, &cmd, spec, &args, err)),
    }
}

/// The subcommand path the arguments name, and whether `help` asked for it.
fn subcommand_path(spec: &CommandSpec, args: &[OsString]) -> (Vec<String>, bool) {
    let mut path: Vec<String> = Vec::new();
    let mut current = spec;
    let mut help_command = false;
    for arg in args.iter().skip(1) {
        let arg = arg.to_string_lossy();
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            continue;
        }
        if !help_command && arg == "help" && current.find_subcommand("help").is_some() {
            // `tool help sub`: the names that follow are the path.
            help_command = true;
            continue;
        }
        match current.find_subcommand(&arg) {
            Some(next) => {
                path.push(next.name.clone());
                current = next;
            }
            None => break,
        }
    }
    (path, help_command)
}

fn render_exit(
    console: &Console,
    cmd: &Command,
    spec: &CommandSpec,
    args: &[OsString],
    err: clap::Error,
) -> ParseExit {
    let (path, help_command) = subcommand_path(spec, args);
    let names: Vec<&str> = path.iter().map(String::as_str).collect();
    let mut sub = spec;
    for name in &names {
        sub = sub.find_subcommand(name).unwrap_or(sub);
    }
    let shown = std::iter::once(spec.display_name())
        .chain(names.iter().copied())
        .collect::<Vec<_>>()
        .join(" ");
    let output = match err.kind() {
        ErrorKind::DisplayHelp | ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand => {
            let long = help_command || args.iter().any(|a| a == "--help");
            let view = HelpView::for_path(spec, &names)
                .unwrap_or_else(|| HelpView::new(spec))
                .long(long);
            console.render_export(&view)
        }
        ErrorKind::DisplayVersion => {
            let version = sub
                .version
                .as_deref()
                .or(spec.version.as_deref())
                .unwrap_or_default();
            console.render_export(&Text::new(format!("{shown} {version}")))
        }
        _ => {
            let mut error = CliError::from_clap(&err, Some(cmd));
            error.usage = Some(sub.usage_lines_as(&shown).join("\n"));
            console.render_export(&error.to_diagnostic())
        }
    };
    ParseExit {
        use_stderr: err.use_stderr(),
        code: err.exit_code(),
        error: err,
        output,
    }
}

//! `rich shell init|status|link`: shell integration helpers (0.0.19, #715).
//! Not upstream: a binary-boundary convenience (see `docs/PORTING.md`) that
//! composes public rs-rich-ext APIs only — `cli_doc::generate` for the
//! completions, `workflow::CommandRecord`/`CommandView` for the status line
//! and `hyperlink::Hyperlinker` for OSC 8 file links.
//!
//! - `init SHELL` prints a script to `eval` (or `source`) from a shell's
//!   startup file: completions, OSC 133 prompt marks (prompt start and end,
//!   command start, command end with its exit status), OSC 7 for the working
//!   directory, and a hook that runs `rich shell status` after each command.
//! - `status` decorates a finished command in one line: its status marker,
//!   the command, `exit N` and the duration. Quiet for quick successes under
//!   `--min-duration`.
//! - `link` prints paths (the working directory by default) as OSC 8 file
//!   links, plain text where links are off, escaped for a bash or zsh prompt
//!   on request.
use super::*;

use rich::{Style, Text};
use rich_ext::a11y::{AccessibilityPolicy, SymbolSet};
use rich_ext::capabilities::SystemEnvironment;
use rich_ext::cli_doc::{ArgSpec, CommandSpec, Shell};
use rich_ext::hyperlink::Hyperlinker;
use rich_ext::workflow::{CommandRecord, CommandStatus};
use std::time::Duration;

/// Where `status` finds the command line when `--command` is not given.
const COMMAND_VAR: &str = "RICH_SHELL_COMMAND";

/// `init`'s default for `--min-duration`: successes quicker than this stay
/// quiet.
const DEFAULT_MIN_DURATION: f64 = 2.0;

/// Whether the command line is `rich shell …` (not `rich -p shell`).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("shell")
}

/// The command's help: registered with the root spec in `cli_spec`.
pub(super) fn command() -> CommandSpec {
    CommandSpec::new("shell")
        .about(
            "Shell integration: `init` prints a script with completions, OSC 133 prompt marks, \
             OSC 7 and a status line after each command; `status` and `link` are its helpers",
        )
        .usage("shell init|status|link [OPTIONS]")
        .subcommand_required(true)
        .subcommand(
            CommandSpec::new("init")
                .about("Print the integration script for SHELL, to eval from its startup file")
                .usage(
                    "init SHELL [--no-completions] [--no-marks] [--no-status] \
                     [--min-duration SECS]",
                )
                .arg(
                    ArgSpec::positional("shell")
                        .required(true)
                        .help("The shell (pwsh is accepted for powershell)")
                        .choices(["bash", "zsh", "fish", "powershell"]),
                )
                .arg(ArgSpec::flag("no-completions").help("Leave out the completion script"))
                .arg(
                    ArgSpec::flag("no-marks").help(
                        "Leave out the OSC 133 prompt and command marks and the OSC 7 directory",
                    ),
                )
                .arg(
                    ArgSpec::flag("no-status")
                        .help("Do not print a status line after each command"),
                )
                .arg(
                    ArgSpec::option("min-duration")
                        .value_name("SECS")
                        .help("Stay quiet after a success quicker than SECS (default 2)"),
                )
                .example(
                    "eval \"$(rich shell init bash)\"",
                    "In ~/.bashrc (bash 4.4+)",
                )
                .example("eval \"$(rich shell init zsh)\"", "In ~/.zshrc")
                .example(
                    "rich shell init fish | source",
                    "In ~/.config/fish/config.fish",
                )
                .example(
                    "rich shell init pwsh | Out-String | Invoke-Expression",
                    "In $PROFILE",
                ),
        )
        .subcommand(
            CommandSpec::new("status")
                .about(
                    "One line for a finished command: a status marker, the command, `exit N` \
                     and how long it took",
                )
                .usage(
                    "status --exit CODE [--duration D] [--command TEXT] [--min-duration SECS] \
                     [--symbols unicode|ascii|words]",
                )
                .arg(
                    ArgSpec::option("exit")
                        .value_name("CODE")
                        .required(true)
                        .help("The command's exit status (0 = success)"),
                )
                .arg(
                    ArgSpec::option("duration")
                        .value_name("D")
                        .help("How long it ran: seconds, or with a unit (1500ms, 1.5s, 2m)"),
                )
                .arg(
                    ArgSpec::option("command")
                        .value_name("TEXT")
                        .help("The command line as typed (default $RICH_SHELL_COMMAND)"),
                )
                .arg(
                    ArgSpec::option("min-duration")
                        .value_name("SECS")
                        .help("Print nothing for a success quicker than SECS (default 0)"),
                )
                .arg(
                    ArgSpec::option("symbols")
                        .value_name("SET")
                        .help("Status markers (default unicode, or as RICH_A11Y says)")
                        .choices(["unicode", "ascii", "words"]),
                )
                .example(
                    "rich shell status --exit 101 --duration 4.2 --command 'cargo test'",
                    "✖ error $ cargo test  exit 101  4.2s",
                ),
        )
        .subcommand(
            CommandSpec::new("link")
                .about(
                    "Print paths (default: the working directory) as OSC 8 file links, plain \
                     where links are off",
                )
                .usage(
                    "link [PATH[:LINE[:COL]]]... [--tilde] [--editor TEMPLATE] \
                     [--links auto|always|never] [--prompt bash|zsh]",
                )
                .arg(
                    ArgSpec::positional("path")
                        .multiple(true)
                        .help("Paths to link, one per line; relative ones resolve here"),
                )
                .arg(ArgSpec::flag("tilde").help("Show paths under $HOME as ~/…"))
                .arg(ArgSpec::option("editor").value_name("TEMPLATE").help(
                    "Link through an editor URL instead of file://, e.g. \
                     vscode://file{path}:{line}:{column}",
                ))
                .arg(
                    ArgSpec::option("links")
                        .value_name("WHEN")
                        .help(
                            "auto (links on a terminal, the default), always (as in a prompt's \
                             command substitution) or never",
                        )
                        .choices(["auto", "always", "never"]),
                )
                .arg(
                    ArgSpec::option("prompt")
                        .value_name("SHELL")
                        .help(
                            "Mark escape sequences as zero-width for a bash (\\001…\\002) or \
                             zsh (%{…%}) prompt",
                        )
                        .choices(["bash", "zsh"]),
                )
                .example(
                    "PS1='$(rich shell link --tilde --links always --prompt bash) \\$ '",
                    "A bash prompt with a clickable working directory",
                ),
        )
}

/// A parsed `rich shell` command line.
#[derive(Debug, PartialEq)]
enum Command {
    Help(Vec<&'static str>),
    Init(InitArgs),
    Status(StatusArgs),
    Link(LinkArgs),
}

#[derive(Debug, PartialEq)]
struct InitArgs {
    shell: Shell,
    completions: bool,
    marks: bool,
    status: bool,
    min_duration: f64,
}

#[derive(Debug, PartialEq)]
struct StatusArgs {
    exit: i32,
    duration: Duration,
    command: String,
    min_duration: Duration,
    symbols: Option<SymbolSet>,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum Links {
    Auto,
    Always,
    Never,
}

#[derive(Debug, PartialEq)]
struct LinkArgs {
    paths: Vec<String>,
    tilde: bool,
    editor: Option<String>,
    links: Links,
    prompt: Option<Shell>,
}

/// The longest duration `--duration` and `--min-duration` take.
const MAX_DURATION_SECS: f64 = 1e9;

/// A duration: seconds, or a number with `ms`, `s` or `m`.
fn parse_duration(flag: &str, text: &str) -> Result<Duration, String> {
    let trimmed = text.trim();
    let (number, scale) = if let Some(number) = trimmed.strip_suffix("ms") {
        (number, 0.001)
    } else if let Some(number) = trimmed.strip_suffix('s') {
        (number, 1.0)
    } else if let Some(number) = trimmed.strip_suffix('m') {
        (number, 60.0)
    } else {
        (trimmed, 1.0)
    };
    // At most MAX_DURATION_SECS, so the `Duration` always exists.
    match number.trim().parse::<f64>() {
        Ok(value) if value.is_finite() && value >= 0.0 && value * scale <= MAX_DURATION_SECS => {
            Duration::try_from_secs_f64(value * scale).map_err(|err| format!("{flag}: {err}"))
        }
        _ => Err(format!(
            "{flag} takes a duration such as 1.5, 1500ms or 2m (at most 10^9 seconds), \
             not {text:?}"
        )),
    }
}

fn parse_args(args: &[String]) -> Result<Command, String> {
    let mut words: Vec<String> = Vec::new();
    let mut help = false;
    let mut flags: Vec<&'static str> = Vec::new();
    let mut completions = true;
    let mut marks = true;
    let mut status = true;
    let mut exit = None;
    let mut duration = Duration::ZERO;
    let mut command_text = None;
    let mut min_duration = None;
    let mut symbols = None;
    let mut tilde = false;
    let mut editor = None;
    let mut links = Links::Auto;
    let mut prompt = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            words.push(arg.clone());
            continue;
        }
        let (name, inline) = match arg.split_once('=') {
            Some((name, value)) if name.starts_with("--") => (name, Some(value.to_string())),
            _ => (arg.as_str(), None),
        };
        let mut value = || -> Result<String, String> {
            match inline.clone() {
                Some(value) => Ok(value),
                None => iter
                    .next()
                    .cloned()
                    .ok_or_else(|| format!("{name} needs a value")),
            }
        };
        let flag: &'static str = match name {
            "--" => {
                words.extend(iter.cloned());
                break;
            }
            "--help" => {
                help = true;
                continue;
            }
            "--no-color" | "--color" | "--no-config" => continue,
            "--report" => {
                value()?;
                continue;
            }
            "--no-completions" => {
                completions = false;
                "--no-completions"
            }
            "--no-marks" => {
                marks = false;
                "--no-marks"
            }
            "--no-status" => {
                status = false;
                "--no-status"
            }
            "--min-duration" => {
                min_duration = Some(parse_duration("--min-duration", &value()?)?);
                "--min-duration"
            }
            "--exit" => {
                let text = value()?;
                exit = Some(text.trim().parse::<i32>().map_err(|_| {
                    format!("--exit takes an exit status such as 0 or 101, not {text:?}")
                })?);
                "--exit"
            }
            "--duration" => {
                duration = parse_duration("--duration", &value()?)?;
                "--duration"
            }
            "--command" => {
                command_text = Some(value()?);
                "--command"
            }
            "--symbols" => {
                symbols = Some(match value()?.as_str() {
                    "unicode" => SymbolSet::Unicode,
                    "ascii" => SymbolSet::Ascii,
                    "words" => SymbolSet::Words,
                    other => {
                        return Err(format!(
                            "--symbols takes unicode, ascii or words, not {other:?}"
                        ))
                    }
                });
                "--symbols"
            }
            "--tilde" => {
                tilde = true;
                "--tilde"
            }
            "--editor" => {
                editor = Some(value()?);
                "--editor"
            }
            "--links" => {
                links = match value()?.as_str() {
                    "auto" => Links::Auto,
                    "always" => Links::Always,
                    "never" => Links::Never,
                    other => {
                        return Err(format!(
                            "--links takes auto, always or never, not {other:?}"
                        ))
                    }
                };
                "--links"
            }
            "--prompt" => {
                prompt = Some(match value()?.as_str() {
                    "bash" => Shell::Bash,
                    "zsh" => Shell::Zsh,
                    other => return Err(format!("--prompt takes bash or zsh, not {other:?}")),
                });
                "--prompt"
            }
            other => return Err(format!("unknown option {other} for `rich shell`")),
        };
        flags.push(flag);
    }
    // words[0] is `shell` itself.
    let action = words.get(1).map(String::as_str);
    let path: Vec<&'static str> = match action {
        Some("init") => vec!["shell", "init"],
        Some("status") => vec!["shell", "status"],
        Some("link") => vec!["shell", "link"],
        _ => vec!["shell"],
    };
    if help {
        return Ok(Command::Help(path));
    }
    let allowed: &[&str] = match action {
        Some("init") => &[
            "--no-completions",
            "--no-marks",
            "--no-status",
            "--min-duration",
        ],
        Some("status") => &[
            "--exit",
            "--duration",
            "--command",
            "--min-duration",
            "--symbols",
        ],
        Some("link") => &["--tilde", "--editor", "--links", "--prompt"],
        None => return Err("`rich shell` needs init, status or link".into()),
        Some(other) => {
            return Err(format!(
                "unknown shell command {other:?}; use init, status or link"
            ))
        }
    };
    if let Some(flag) = flags.iter().find(|flag| !allowed.contains(flag)) {
        return Err(format!(
            "{flag} does not apply to `rich shell {}`",
            action.unwrap_or_default()
        ));
    }
    let rest = &words[2..];
    match action {
        Some("init") => {
            let [shell] = rest else {
                return Err(
                    "`rich shell init` takes one shell: bash, zsh, fish or powershell".into(),
                );
            };
            Ok(Command::Init(InitArgs {
                shell: shell.parse()?,
                completions,
                marks,
                status,
                min_duration: min_duration.map_or(DEFAULT_MIN_DURATION, |d| d.as_secs_f64()),
            }))
        }
        Some("status") => {
            if let Some(extra) = rest.first() {
                return Err(format!(
                    "`rich shell status` takes no arguments; unexpected {extra:?} (use --command)"
                ));
            }
            Ok(Command::Status(StatusArgs {
                exit: exit.ok_or("`rich shell status` needs --exit CODE")?,
                duration,
                // PowerShell hands the line over in the environment: an
                // argument would be split by legacy native argument passing.
                command: command_text
                    .or_else(|| std::env::var(COMMAND_VAR).ok())
                    .unwrap_or_default(),
                min_duration: min_duration.unwrap_or(Duration::ZERO),
                symbols,
            }))
        }
        _ => Ok(Command::Link(LinkArgs {
            paths: rest.to_vec(),
            tilde,
            editor,
            links,
            prompt,
        })),
    }
}

/// Stands in for the command in the record, so the line is shown as typed
/// rather than re-quoted word by word (`(exit 7)`, `a | b`).
const PLACEHOLDER: &str = "RICH-SHELL-COMMAND-LINE";

/// The record `status` shows, and the command line to put in its place.
fn status_record(args: &StatusArgs) -> (CommandRecord, String) {
    // Shown, never run: a control character in it must not reach the
    // terminal as one.
    let line = rich_ext::sanitize_terminal_controls(args.command.trim());
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = if line.is_empty() { "-".into() } else { line };
    let record = CommandRecord::new(PLACEHOLDER, Vec::<String>::new())
        .status(CommandStatus::Exited(args.exit))
        .duration(args.duration);
    (record, line)
}

fn run_status(args: &StatusArgs, no_color: bool) -> String {
    if args.exit == 0 && args.duration < args.min_duration {
        return String::new();
    }
    let policy = AccessibilityPolicy::from_env(&SystemEnvironment);
    let (record, line) = status_record(args);
    let mut view = record
        .view()
        .tail(0)
        .show_cwd(false)
        .show_diagnostic(false)
        .policy(&policy);
    if let Some(symbols) = args.symbols {
        view = view.symbols(symbols);
    }
    // Wide enough that the header is never wrapped: it is one line, and the
    // terminal folds it if it must.
    let console = Console::builder()
        .no_color(no_color)
        .theme(rich_ext::extended_theme())
        .width(PLACEHOLDER.len() + 200)
        .build();
    let rendered = console.render_to_string(&view);
    format!("{}\n", rendered.trim_end().replacen(PLACEHOLDER, &line, 1))
}

/// `path` with `$HOME` replaced by `~`.
fn tilde_label(path: &str, home: Option<&Path>) -> String {
    let Some(home) = home.and_then(Path::to_str).filter(|home| !home.is_empty()) else {
        return path.to_string();
    };
    let home = home.trim_end_matches('/');
    if path == home {
        "~".into()
    } else if let Some(rest) = path.strip_prefix(home).filter(|rest| rest.starts_with('/')) {
        format!("~{rest}")
    } else {
        path.to_string()
    }
}

/// `path:line[:column]` split, when the plain path does not exist but the
/// part before the numbers does.
fn location(path: &str) -> (String, Option<usize>, Option<usize>) {
    if Path::new(path).exists() {
        return (path.to_string(), None, None);
    }
    let mut parts = path.rsplitn(3, ':').collect::<Vec<_>>();
    parts.reverse();
    let number = |text: &str| text.parse::<usize>().ok().filter(|n| *n > 0);
    match parts.as_slice() {
        [file, line, column] if number(line).is_some() && number(column).is_some() => {
            (file.to_string(), number(line), number(column))
        }
        [.., file, line] if number(line).is_some() => {
            let file = if parts.len() == 3 {
                format!("{}:{file}", parts[0])
            } else {
                file.to_string()
            };
            (file, number(line), None)
        }
        _ => (path.to_string(), None, None),
    }
}

/// Mark every run of escape sequences as zero-width for `shell`'s prompt.
fn prompt_escape(text: &str, shell: Shell) -> String {
    let (open, close) = match shell {
        Shell::Zsh => ("%{", "%}"),
        _ => ("\u{1}", "\u{2}"),
    };
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '\x1b' {
            if shell == Shell::Zsh && chars[index] == '%' {
                out.push('%');
            }
            out.push(chars[index]);
            index += 1;
            continue;
        }
        out.push_str(open);
        // One or more escape sequences in a row.
        while index < chars.len() && chars[index] == '\x1b' {
            let start = index;
            index += 1;
            match chars.get(index) {
                Some(']') => {
                    // OSC: ends with BEL or ST (ESC \).
                    index += 1;
                    while index < chars.len() {
                        if chars[index] == '\x07' {
                            index += 1;
                            break;
                        }
                        if chars[index] == '\x1b' && chars.get(index + 1) == Some(&'\\') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                }
                Some('[') => {
                    // CSI: parameters, then a final byte.
                    index += 1;
                    while index < chars.len() && !('\x40'..='\x7e').contains(&chars[index]) {
                        index += 1;
                    }
                    index = (index + 1).min(chars.len());
                }
                Some(_) => index += 1,
                None => {}
            }
            // zsh expands `%` inside `%{…%}` too (a URL's `%20`).
            for &c in &chars[start..index] {
                if shell == Shell::Zsh && c == '%' {
                    out.push('%');
                }
                out.push(c);
            }
        }
        out.push_str(close);
    }
    out
}

fn run_link(args: &LinkArgs, no_color: bool) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // The shell's `$PWD` keeps the path as typed (through symlinks), which is
    // what a prompt shows; use it when it names the same directory.
    let shown_cwd = std::env::var_os("PWD")
        .map(PathBuf::from)
        .filter(|pwd| pwd.canonicalize().ok() == cwd.canonicalize().ok())
        .unwrap_or_else(|| cwd.clone());
    let mut linker = Hyperlinker::new()
        .base_dir(&cwd)
        .enabled(args.links != Links::Never);
    if let Some(template) = &args.editor {
        linker = linker.editor(template.clone());
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    // `$HOME` resolved too, for a home reached through a symlink (macOS's
    // /var is /private/var).
    let real_home = home.as_deref().and_then(|home| home.canonicalize().ok());
    let paths = if args.paths.is_empty() {
        vec![shown_cwd.to_string_lossy().into_owned()]
    } else {
        args.paths.clone()
    };
    let mut builder = Console::builder().no_color(no_color);
    if args.links == Links::Always {
        builder = builder.force_terminal(true);
    }
    let console = builder.build();
    let mut out = String::new();
    for path in paths {
        let (file, line, column) = location(&path);
        let mut label = if args.tilde {
            match tilde_label(&path, home.as_deref()) {
                unchanged if unchanged == path => tilde_label(&path, real_home.as_deref()),
                label => label,
            }
        } else {
            path.clone()
        };
        label = rich_ext::sanitize_terminal_controls(&label);
        let mut text = Text::new(label.clone());
        if let Some(url) = linker.file_url(&file, line, column) {
            text.stylize(Style::new().with_link(url), 0, label.chars().count());
        }
        let rendered = console.render_to_string(&text.no_wrap(true));
        let rendered = match args.prompt {
            Some(shell) => prompt_escape(&rendered, shell),
            None => rendered,
        };
        out.push_str(&rendered);
        out.push('\n');
    }
    // A prompt's command substitution drops the last newline anyway.
    out
}

/// The `rich` the scripts call.
const PROGRAM: &str = "rich";

/// bash: `$1` percent-encoded into `__rich_shell_path`, byte by byte, for
/// OSC 7 (a directory name is data: no escape in it reaches the terminal).
const BASH_URL: &str = r#"__rich_shell_url() {
    local LC_ALL=C __rich_i __rich_c
    __rich_shell_path=
    for (( __rich_i = 0; __rich_i < ${#1}; __rich_i++ )); do
        __rich_c=${1:__rich_i:1}
        case $__rich_c in
            [A-Za-z0-9/._~-]) __rich_shell_path+=$__rich_c ;;
            *) printf -v __rich_c '%%%02X' "'$__rich_c"; __rich_shell_path+=$__rich_c ;;
        esac
    done
}
"#;

/// zsh: `$1` percent-encoded into `REPLY`, byte by byte, for OSC 7.
const ZSH_URL: &str = r#"__rich_shell_url() {
    emulate -L zsh
    setopt nomultibyte
    local __rich_c
    REPLY=
    for __rich_c in ${(s::)1}; do
        if [[ $__rich_c == [A-Za-z0-9/._~-] ]]; then
            REPLY+=$__rich_c
        else
            REPLY+=%${(l:2::0:)$(( [##16] #__rich_c ))}
        fi
    done
}
"#;

fn bash_script(args: &InitArgs, completions: &str) -> String {
    let min = args.min_duration;
    let mut s = String::from(
        "# rich shell integration for bash (`rich shell init bash`); needs bash 4.4+.\n\
         if (( BASH_VERSINFO[0] * 100 + BASH_VERSINFO[1] < 404 )); then\n\
         \x20   printf 'rich shell init bash: needs bash 4.4 or newer, not %s\\n' \"$BASH_VERSION\" >&2\n\
         elif [[ -z \"${__rich_shell_loaded-}\" ]]; then\n\
         __rich_shell_loaded=1\n",
    );
    if args.completions {
        s.push_str(completions);
        s.push('\n');
    }
    if args.marks {
        s.push_str(BASH_URL);
    }
    s.push_str(
        "__rich_shell_ran=0 __rich_shell_ps0=\n\
         if [[ -n \"${EPOCHREALTIME-}\" ]]; then\n\
         \x20   __rich_shell_clock='${EPOCHREALTIME/[.,]/}'\n\
         else\n\
         \x20   __rich_shell_clock='$(( SECONDS * 1000000 ))'\n\
         fi\n\
         # PS0 is expanded in this shell just before a command runs: note that\n\
         # one ran, and when it started.\n\
         PS0='${__rich_shell_ps0:0:$(( __rich_shell_ran = 1, __rich_shell_start = '\"$__rich_shell_clock\"', 0 ))}'",
    );
    if args.marks {
        s.push_str("'\\e]133;C\\a'");
    }
    s.push_str("\"${PS0-}\"\n");
    s.push_str(
        "__rich_shell_precmd() {\n\
         \x20   local __rich_code=$?\n",
    );
    if args.status {
        // The last history entry, number and all, is unchanged when the
        // command was not saved (HISTCONTROL, HISTIGNORE): `history 1` would
        // name the one before it. (Not `${x@P}`: macOS's bash is 3.2.)
        s.push_str(
            "    local __rich_last __rich_saved=0\n\
             \x20   __rich_last=$(HISTTIMEFORMAT= builtin history 1)\n\
             \x20   [[ $__rich_last != \"${__rich_shell_last-}\" ]] && __rich_saved=1\n\
             \x20   __rich_shell_last=$__rich_last\n",
        );
    }
    s.push_str(
        "    if [[ \"${__rich_shell_ran-0}\" == 1 ]]; then\n\
         \x20       __rich_shell_ran=0\n",
    );
    if args.marks {
        s.push_str("        printf '\\e]133;D;%s\\a' \"$__rich_code\"\n");
    }
    if args.status {
        s.push_str(&format!(
            "        local __rich_end __rich_cmd=()\n\
             \x20       eval \"__rich_end=$__rich_shell_clock\"\n\
             \x20       if (( __rich_saved )) && [[ $__rich_last =~ ^[[:space:]]*[0-9]+[*]?[[:space:]]+(.*)$ ]]; then\n\
             \x20           __rich_cmd=(--command \"${{BASH_REMATCH[1]}}\")\n\
             \x20       fi\n\
             \x20       command {PROGRAM} shell status --exit \"$__rich_code\" \\\n\
             \x20           --duration \"$(( (__rich_end - __rich_shell_start) / 1000 ))ms\" \\\n\
             \x20           --min-duration {min} \"${{__rich_cmd[@]}}\"\n"
        ));
    }
    s.push_str("    fi\n");
    if args.marks {
        s.push_str(
            "    __rich_shell_url \"$PWD\"\n\
             \x20   printf '\\e]7;file://%s%s\\a' \"${HOSTNAME-}\" \"$__rich_shell_path\"\n",
        );
    }
    s.push_str(
        "    return \"$__rich_code\"\n\
         }\n\
         PROMPT_COMMAND=\"__rich_shell_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}\"\n",
    );
    if args.marks {
        s.push_str("PS1='\\[\\e]133;A\\a\\]'\"$PS1\"'\\[\\e]133;B\\a\\]'\n");
    }
    s.push_str("fi\n");
    s
}

fn zsh_script(args: &InitArgs, completions: &str) -> String {
    let min = args.min_duration;
    let mut s = String::from(
        "# rich shell integration for zsh (`rich shell init zsh`).\n\
         if [[ -z \"${__rich_shell_loaded-}\" ]]; then\n\
         typeset -g __rich_shell_loaded=1\n\
         zmodload zsh/datetime 2>/dev/null\n\
         autoload -Uz add-zsh-hook\n",
    );
    if args.marks {
        s.push_str(ZSH_URL);
    }
    if args.completions {
        s.push_str("# Completions need compinit; run it before this.\n");
        s.push_str("if (( $+functions[compdef] )); then\n");
        s.push_str(completions);
        s.push_str("\nfi\n");
    }
    s.push_str(
        "__rich_shell_preexec() {\n\
         \x20   typeset -g __rich_shell_start=$EPOCHREALTIME __rich_shell_cmd=$1\n",
    );
    if args.marks {
        s.push_str("    print -rn -- $'\\e]133;C\\a'\n");
    }
    s.push_str(
        "}\n\
         __rich_shell_precmd() {\n\
         \x20   local __rich_code=$?\n\
         \x20   if [[ -n \"${__rich_shell_start-}\" ]]; then\n",
    );
    if args.marks {
        s.push_str("        print -rn -- $'\\e]133;D;'\"$__rich_code\"$'\\a'\n");
    }
    if args.status {
        s.push_str(&format!(
            "        local -i __rich_ms=$(( (EPOCHREALTIME - __rich_shell_start) * 1000 ))\n\
             \x20       command {PROGRAM} shell status --exit $__rich_code \\\n\
             \x20           --duration \"${{__rich_ms}}ms\" --min-duration {min} \\\n\
             \x20           --command \"$__rich_shell_cmd\"\n"
        ));
    }
    s.push_str("        unset __rich_shell_start\n    fi\n");
    if args.marks {
        s.push_str(
            "    local REPLY\n\
             \x20   __rich_shell_url \"$PWD\"\n\
             \x20   print -rn -- $'\\e]7;file://'\"${HOST}${REPLY}\"$'\\a'\n",
        );
    }
    s.push_str(
        "    return $__rich_code\n\
         }\n\
         add-zsh-hook preexec __rich_shell_preexec\n\
         add-zsh-hook precmd __rich_shell_precmd\n",
    );
    if args.marks {
        s.push_str("PS1=$'%{\\e]133;A\\a%}'\"$PS1\"$'%{\\e]133;B\\a%}'\n");
    }
    s.push_str("fi\n");
    s
}

fn fish_script(args: &InitArgs, completions: &str) -> String {
    let min = args.min_duration;
    let mut s = String::from(
        "# rich shell integration for fish (`rich shell init fish`).\n\
         if not set -q __rich_shell_loaded\n\
         set -g __rich_shell_loaded 1\n",
    );
    if args.completions {
        s.push_str(completions);
        s.push('\n');
    }
    if args.marks {
        s.push_str(
            "function __rich_shell_preexec --on-event fish_preexec\n\
             \x20   printf '\\e]133;C\\a'\n\
             end\n\
             function __rich_shell_prompt --on-event fish_prompt\n\
             \x20   printf '\\e]133;A\\a'\n\
             \x20   printf '\\e]7;file://%s%s\\a' $hostname (string escape --style=url -- $PWD)\n\
             end\n",
        );
    }
    s.push_str(
        "function __rich_shell_postexec --on-event fish_postexec\n\
         \x20   set -l __rich_code $status\n\
         \x20   test -n \"$argv[1]\"; or return\n",
    );
    if args.marks {
        s.push_str("    printf '\\e]133;D;%s\\a' $__rich_code\n");
    }
    if args.status {
        s.push_str(&format!(
            "    command {PROGRAM} shell status --exit $__rich_code --duration {{$CMD_DURATION}}ms \\\n\
             \x20       --min-duration {min} --command \"$argv[1]\"\n"
        ));
    }
    s.push_str("end\nend\n");
    s
}

fn powershell_script(args: &InitArgs, completions: &str) -> String {
    let min = args.min_duration;
    let mut s = String::from(
        "# rich shell integration for PowerShell (`rich shell init pwsh`).\n\
         if (-not $global:__RichShellLoaded) {\n\
         $global:__RichShellLoaded = $true\n",
    );
    if args.completions {
        s.push_str(completions);
        s.push('\n');
    }
    s.push_str(
        "$global:__RichShellPrompt = $function:prompt\n\
         $global:__RichShellLastId = (Get-History -Count 1).Id\n\
         function global:prompt {\n\
         \x20   $ok = $?\n\
         \x20   $exit = $global:LASTEXITCODE\n\
         \x20   $code = if ($ok) { 0 } elseif ($exit) { $exit } else { 1 }\n\
         \x20   $esc = [char]27; $bel = [char]7\n\
         \x20   $last = Get-History -Count 1\n\
         \x20   if ($last -and $last.Id -ne $global:__RichShellLastId) {\n\
         \x20       $global:__RichShellLastId = $last.Id\n",
    );
    if args.marks {
        s.push_str("        [Console]::Write(\"$esc]133;D;$code$bel\")\n");
    }
    if args.status {
        s.push_str(&format!(
            "        $ms = [long]($last.EndExecutionTime - $last.StartExecutionTime).TotalMilliseconds\n\
             \x20       $env:RICH_SHELL_COMMAND = $last.CommandLine\n\
             \x20       & {PROGRAM} shell status --exit $code --duration \"${{ms}}ms\" --min-duration {min} | Out-Host\n\
             \x20       Remove-Item Env:RICH_SHELL_COMMAND -ErrorAction SilentlyContinue\n"
        ));
    }
    s.push_str("    }\n");
    if args.marks {
        s.push_str(
            "    $dir = $PWD.ProviderPath -replace '\\\\', '/'\n\
             \x20   if (-not $dir.StartsWith('/')) { $dir = '/' + $dir }\n\
             \x20   $dir = (($dir -split '/' | ForEach-Object { [Uri]::EscapeDataString($_) }) -join '/').Replace('%3A', ':')\n\
             \x20   [Console]::Write(\"$esc]7;file://$([Environment]::MachineName)$dir$bel\")\n",
        );
    }
    s.push_str("    $global:LASTEXITCODE = $exit\n");
    if args.marks {
        s.push_str("    \"$esc]133;A$bel\" + (& $global:__RichShellPrompt) + \"$esc]133;B$bel\"\n");
    } else {
        s.push_str("    & $global:__RichShellPrompt\n");
    }
    s.push_str("}\n}\n");
    s
}

/// The `init` script.
fn init_script(args: &InitArgs) -> String {
    let completions = if args.completions {
        rich_ext::cli_doc::generate(&cli_spec::spec(), args.shell)
    } else {
        String::new()
    };
    let completions = completions.trim_end();
    match args.shell {
        Shell::Bash => bash_script(args, completions),
        Shell::Zsh => zsh_script(args, completions),
        Shell::Fish => fish_script(args, completions),
        Shell::PowerShell => powershell_script(args, completions),
    }
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    let no_color = cli_spec::no_color_requested(args);
    let command = match parse_args(args) {
        Ok(command) => command,
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich shell --help`)"),
            )
        }
    };
    let text = match command {
        Command::Help(path) => cli_spec::subcommand_help(&path, no_color)
            .map(|help| format!("{help}\n"))
            .unwrap_or_default(),
        Command::Init(args) => init_script(&args),
        Command::Status(args) => run_status(&args, no_color),
        Command::Link(args) => run_link(&args, no_color),
    };
    authoring::out(&text);
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Command, String> {
        parse_args(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_each_command() {
        assert_eq!(
            parsed(&[
                "shell",
                "init",
                "pwsh",
                "--no-marks",
                "--min-duration",
                "0.5"
            ]),
            Ok(Command::Init(InitArgs {
                shell: Shell::PowerShell,
                completions: true,
                marks: false,
                status: true,
                min_duration: 0.5,
            }))
        );
        assert_eq!(
            parsed(&[
                "shell",
                "status",
                "--exit=101",
                "--duration",
                "1500ms",
                "--command",
                "cargo test"
            ]),
            Ok(Command::Status(StatusArgs {
                exit: 101,
                duration: Duration::from_millis(1500),
                command: "cargo test".into(),
                min_duration: Duration::ZERO,
                symbols: None,
            }))
        );
        assert!(matches!(
            parsed(&["shell", "link", "a", "b", "--links", "never"]),
            Ok(Command::Link(LinkArgs { ref paths, links: Links::Never, .. })) if paths.len() == 2
        ));
        assert_eq!(
            parsed(&["shell", "status", "--help"]),
            Ok(Command::Help(vec!["shell", "status"]))
        );
    }

    #[test]
    fn zsh_prompt_doubles_percent_inside_escapes() {
        let link = "\x1b]8;;file:///a%20b\x1b\\a b\x1b]8;;\x1b\\ 100%";
        assert_eq!(
            prompt_escape(link, Shell::Zsh),
            "%{\x1b]8;;file:///a%%20b\x1b\\%}a b%{\x1b]8;;\x1b\\%} 100%%"
        );
        // bash's markers leave `%` alone.
        assert_eq!(
            prompt_escape(link, Shell::Bash),
            "\u{1}\x1b]8;;file:///a%20b\x1b\\\u{2}a b\u{1}\x1b]8;;\x1b\\\u{2} 100%"
        );
    }

    #[test]
    fn durations_are_bounded() {
        assert!(parse_duration("--duration", "1e9").is_ok());
        for bad in ["1e300", "1e8m", "inf", "-1", "NaN", "1e10s"] {
            assert!(parse_duration("--duration", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_misuse() {
        for bad in [
            &["shell"][..],
            &["shell", "launch"],
            &["shell", "init"],
            &["shell", "init", "tcsh"],
            &["shell", "init", "bash", "zsh"],
            &["shell", "init", "bash", "--exit", "1"],
            &["shell", "status"],
            &["shell", "status", "--exit", "x"],
            &["shell", "status", "--exit", "0", "extra"],
            &["shell", "status", "--exit", "0", "--duration", "soon"],
            &["shell", "link", "--links", "sometimes"],
            &["shell", "link", "--prompt", "fish"],
            &["shell", "link", "--bogus"],
        ] {
            assert!(parsed(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn durations_take_units() {
        let d = |text| parse_duration("--duration", text).unwrap();
        assert_eq!(d("1.5"), Duration::from_millis(1500));
        assert_eq!(d("250ms"), Duration::from_millis(250));
        assert_eq!(d("2s"), Duration::from_secs(2));
        assert_eq!(d("2m"), Duration::from_secs(120));
        assert!(parse_duration("--duration", "-1").is_err());
    }

    #[test]
    fn shows_the_command_as_typed() {
        let status = |command: &str| StatusArgs {
            exit: 7,
            duration: Duration::ZERO,
            command: command.into(),
            min_duration: Duration::ZERO,
            symbols: Some(SymbolSet::Ascii),
        };
        let line = run_status(&status("(exit 7) | cat"), true);
        assert!(line.contains("$ (exit 7) | cat  exit 7"), "{line}");
        assert!(!line.contains(PLACEHOLDER), "{line}");
        let line = run_status(&status("printf '\x1b[2J'\nmore"), true);
        assert!(!line.contains('\x1b'), "{line:?}");
        assert_eq!(line.lines().count(), 1, "{line:?}");
        assert!(run_status(&status(""), true).contains("$ -"));
    }

    #[test]
    fn status_lines_follow_the_exit_code() {
        let status = |exit, millis, min| StatusArgs {
            exit,
            duration: Duration::from_millis(millis),
            command: "cargo test".into(),
            min_duration: Duration::from_millis(min),
            symbols: Some(SymbolSet::Ascii),
        };
        let failed = run_status(&status(101, 4200, 0), true);
        assert!(failed.contains("cargo test"), "{failed}");
        assert!(failed.contains("exit 101"), "{failed}");
        assert!(failed.contains("4.2s"), "{failed}");
        assert_eq!(failed.lines().count(), 1, "{failed}");
        // A quick success under the threshold is quiet; a failure never is.
        assert_eq!(run_status(&status(0, 100, 2000), true), "");
        assert!(!run_status(&status(1, 100, 2000), true).is_empty());
        assert!(!run_status(&status(0, 3000, 2000), true).is_empty());
    }

    #[test]
    fn labels_and_locations() {
        let home = Path::new("/home/me");
        assert_eq!(tilde_label("/home/me", Some(home)), "~");
        assert_eq!(tilde_label("/home/me/src", Some(home)), "~/src");
        assert_eq!(tilde_label("/home/meow", Some(home)), "/home/meow");
        assert_eq!(tilde_label("/tmp", None), "/tmp");
        assert_eq!(
            location("/no/such/file.rs:12:3"),
            ("/no/such/file.rs".to_string(), Some(12), Some(3))
        );
        assert_eq!(
            location("/no/such/file.rs:12"),
            ("/no/such/file.rs".to_string(), Some(12), None)
        );
        assert_eq!(
            location("/no/such/dir"),
            ("/no/such/dir".to_string(), None, None)
        );
    }

    #[test]
    fn prompt_escapes_wrap_sequences() {
        let text = "\x1b]8;;file:///x\x1b\\x\x1b]8;;\x1b\\ 100%";
        assert_eq!(
            prompt_escape(text, Shell::Bash),
            "\u{1}\x1b]8;;file:///x\x1b\\\u{2}x\u{1}\x1b]8;;\x1b\\\u{2} 100%"
        );
        assert_eq!(
            prompt_escape(text, Shell::Zsh),
            "%{\x1b]8;;file:///x\x1b\\%}x%{\x1b]8;;\x1b\\%} 100%%"
        );
        assert_eq!(
            prompt_escape("\x1b[1m\x1b[0mok", Shell::Bash),
            "\u{1}\x1b[1m\x1b[0m\u{2}ok"
        );
    }

    #[test]
    fn scripts_carry_each_part_only_when_asked() {
        for shell in Shell::ALL {
            let all = init_script(&InitArgs {
                shell,
                completions: true,
                marks: true,
                status: true,
                min_duration: 2.0,
            });
            assert!(all.contains("133;A"), "{shell}");
            assert!(all.contains("133;D;"), "{shell}");
            assert!(all.contains("7;file://"), "{shell}");
            assert!(all.contains("shell status --exit"), "{shell}");
            assert!(all.contains("--min-duration 2"), "{shell}");
            let none = init_script(&InitArgs {
                shell,
                completions: false,
                marks: false,
                status: false,
                min_duration: 2.0,
            });
            assert!(!none.contains("133;"), "{shell}: {none}");
            assert!(!none.contains("shell status"), "{shell}: {none}");
            assert!(none.len() < all.len() / 4, "{shell}: completions left in");
        }
    }
}

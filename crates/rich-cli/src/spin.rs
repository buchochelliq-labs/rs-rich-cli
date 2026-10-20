//! `rich spin [--title TEXT] [--spinner NAME] -- COMMAND [ARG]...` (0.0.21,
//! after `gum spin`): run a command with a spinner and a title until it
//! exits, and exit with its status. Not upstream: a binary-boundary
//! convenience (see `docs/PORTING.md`) that composes core's public `Status`
//! and spinner table only.
//!
//! - On a terminal (stderr, where the spinner draws), the command's output
//!   is held back: `--show-output` prints it once the command exits, and
//!   `--show-error` only when it fails.
//! - Without one (a pipe, CI), there is no spinner: the command runs with
//!   the output going straight through.
//! - `RICH_A11Y` asking for reduced motion (or `RICH_ANIMATION=0`) draws
//!   the title once instead of animating it.
use super::*;

use std::io::{IsTerminal, Read, Write};
use std::process::{Command as Process, Stdio};
use std::time::{Duration, Instant};

use rich::spinner::spinner_frames;
use rich_ext::capabilities::{Environment, SystemEnvironment};
use rich_ext::cli_doc::{ArgSpec, CommandSpec};

/// The title when `--title` is not given (gum's).
const DEFAULT_TITLE: &str = "Loading...";

/// The spinner when `--spinner` is not given.
const DEFAULT_SPINNER: &str = "dots";

/// Whether the command line is `rich spin …` (not `rich -p spin`).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("spin")
}

/// The command's help: registered with the root spec in `cli_spec`.
pub(super) fn command() -> CommandSpec {
    CommandSpec::new("spin")
        .about(
            "Run a command with a spinner and a title until it exits, and exit with its status \
             (like gum spin); without a terminal it just runs the command",
        )
        .usage(
            "spin [--title TEXT] [--spinner NAME] [--show-output | --show-error] -- COMMAND \
             [ARG]...",
        )
        .arg(
            ArgSpec::option("title")
                .value_name("TEXT")
                .help("The text beside the spinner (default \"Loading...\")"),
        )
        .arg(
            ArgSpec::option("spinner").value_name("NAME").help(
                "One of rich's spinners (default dots; `rich asset --kind spinner` picks one)",
            ),
        )
        .arg(ArgSpec::flag("show-output").help(
            "Print the command's output (stdout and stderr) once it exits; on a terminal it is \
             otherwise held back",
        ))
        .arg(ArgSpec::flag("show-error").help("Print the command's output only when it fails"))
        .arg(
            ArgSpec::positional("command")
                .required(true)
                .multiple(true)
                .help("The command and its arguments, after `--`"),
        )
        .example(
            "rich spin --title \"Building\" -- cargo build --release",
            "A spinner until the build ends; its exit status is rich's",
        )
        .example(
            "rich spin --spinner moon --show-output -- curl -sS https://example.com",
            "The moon spinner, then the page",
        )
}

/// `rich spin`'s arguments.
#[derive(Debug, PartialEq, Eq)]
struct SpinArgs {
    title: String,
    spinner: String,
    show_output: bool,
    show_error: bool,
    command: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    Help,
    Run(SpinArgs),
}

/// Parse the words after (and the global options before) `spin`.
fn parse_args(args: &[String]) -> Result<Parsed, String> {
    let mut iter = args.iter();
    // The global options before the word, then the word itself.
    while let Some(arg) = iter.next() {
        if arg == "spin" {
            break;
        }
        if VALUE_OPTIONS.contains(&arg.as_str()) {
            iter.next();
        }
    }
    let mut spin = SpinArgs {
        title: DEFAULT_TITLE.into(),
        spinner: DEFAULT_SPINNER.into(),
        show_output: false,
        show_error: false,
        command: Vec::new(),
    };
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            // The first word is the command: the rest are its own.
            spin.command.push(arg.clone());
            spin.command.extend(iter.cloned());
            break;
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
        match name {
            "--" => {
                spin.command.extend(iter.cloned());
                break;
            }
            "--help" | "-h" => return Ok(Parsed::Help),
            "--no-color" | "--color" | "--no-config" => {}
            "--config" | "--profile" | "--report" => {
                value()?;
            }
            "--title" => spin.title = value()?,
            "--spinner" => {
                let name = value()?;
                if spinner_frames(&name).is_none() {
                    return Err(format!(
                        "--spinner: no spinner named {name:?} (`rich asset --kind spinner` lists \
                         them)"
                    ));
                }
                spin.spinner = name;
            }
            "--show-output" => spin.show_output = true,
            "--show-error" => spin.show_error = true,
            other => return Err(format!("unknown option {other}")),
        }
    }
    if spin.command.is_empty() {
        return Err("no command to run: rich spin [OPTIONS] -- COMMAND [ARG]...".into());
    }
    Ok(Parsed::Run(spin))
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    let no_color = cli_spec::no_color_requested(args);
    let spin = match parse_args(args) {
        Ok(Parsed::Run(spin)) => spin,
        Ok(Parsed::Help) => {
            if let Some(help) = cli_spec::subcommand_help(&["spin"], no_color) {
                authoring::out(&format!("{help}\n"));
            }
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich spin --help`)"),
            )
        }
    };
    let env = SystemEnvironment;
    let terminal = std::io::stderr().is_terminal();
    let no_color = no_color || env.var("NO_COLOR").is_some_and(|v| !v.is_empty());
    let animate = animation_allowed(&env);
    match run(&spin, terminal, animate, no_color) {
        Ok(code) => code,
        Err((code, message)) => {
            eprintln!("rich: {}", controls::shown(&message));
            ExitCode::from(code)
        }
    }
}

/// Whether animation may run: `RICH_ANIMATION` when set, else not when
/// `RICH_A11Y` asks for reduced motion, no animation or a screen reader.
fn animation_allowed(env: &dyn Environment) -> bool {
    if let Some(on) = env
        .var("RICH_ANIMATION")
        .and_then(|value| rich_ext::capabilities::parse_bool(&value))
    {
        return on;
    }
    let policy = rich_ext::a11y::AccessibilityPolicy::from_env(env);
    !(policy.no_animation || policy.reduced_motion || policy.screen_reader)
}

/// The exit status a finished command gives: its code, or 128 and the
/// signal that ended it, as a shell reports it.
fn exit_status(status: std::process::ExitStatus) -> u8 {
    if let Some(code) = status.code() {
        return u8::try_from(code).unwrap_or(1);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return u8::try_from(128 + signal).unwrap_or(1);
        }
    }
    1
}

/// Read a pipe to its end on a thread.
fn drain(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// One frame: the spinner's frame at `elapsed` and the title, cut to
/// `width` cells, rendered for `console`.
fn frame(
    status: &rich::Status,
    elapsed: Duration,
    width: usize,
    console: &rich::Console,
) -> String {
    let mut text = status.renderable().render(elapsed.as_secs_f64());
    text.truncate(width.max(1), Some(rich::Overflow::Ellipsis), false);
    console
        .render_to_string(&text)
        .trim_end_matches('\n')
        .to_string()
}

/// Run the command: with the spinner on a terminal, straight through
/// without one. The exit code, or the code and why it could not start.
fn run(
    spin: &SpinArgs,
    terminal: bool,
    animate: bool,
    no_color: bool,
) -> Result<ExitCode, (u8, String)> {
    let mut process = Process::new(&spin.command[0]);
    process.args(&spin.command[1..]);
    if terminal {
        process.stdout(Stdio::piped()).stderr(Stdio::piped());
    }
    let mut child = process.spawn().map_err(|error| {
        let code = match error.kind() {
            std::io::ErrorKind::PermissionDenied => 126,
            _ => 127,
        };
        (code, format!("cannot run {:?}: {error}", spin.command[0]))
    })?;
    if !terminal {
        let status = child
            .wait()
            .map_err(|error| (1, format!("waiting for the command: {error}")))?;
        return Ok(ExitCode::from(exit_status(status)));
    }
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    // Ctrl+C reaches the command too (it shares the terminal's process
    // group): rich stays to clean up the line and report how it ended. In a
    // host process (the Python wheel) the handler would outlive the command
    // and replace the host's own, so the host's handling stands instead.
    if !crate::in_host() {
        let _ = ctrlc::set_handler(|| {});
    }
    let console = rich::Console::builder()
        .force_terminal(true)
        .no_color(no_color)
        .build();
    let width = console.width().saturating_sub(1);
    let status = rich::Status::new(rich::markup::escape(&spin.title)).spinner(&spin.spinner);
    let (interval, _) = spinner_frames(&spin.spinner).unwrap_or((80.0, &[]));
    let interval = Duration::from_secs_f64((interval / 1000.0).max(0.01));
    let start = Instant::now();
    let mut err = std::io::stderr();
    let _ = write!(err, "\x1b[?25l");
    let exit = loop {
        let _ = write!(
            err,
            "\r\x1b[2K{}",
            frame(&status, start.elapsed(), width, &console)
        );
        let _ = err.flush();
        match child.try_wait() {
            Ok(Some(exit)) => break Ok(exit),
            Ok(None) => {}
            Err(error) => break Err(error),
        }
        // Without animation the first frame stays: wait for the end.
        if !animate {
            break child.wait();
        }
        std::thread::sleep(interval);
    };
    let _ = write!(err, "\r\x1b[2K\x1b[?25h");
    let _ = err.flush();
    let exit = exit.map_err(|error| (1, format!("waiting for the command: {error}")))?;
    let out = stdout.join().unwrap_or_default();
    let errors = stderr.join().unwrap_or_default();
    if spin.show_output || (spin.show_error && !exit.success()) {
        let _ = std::io::stdout().write_all(&out);
        let _ = std::io::stdout().flush();
        let _ = err.write_all(&errors);
    }
    Ok(ExitCode::from(exit_status(exit)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Parsed, String> {
        parse_args(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_options_and_the_command() {
        assert_eq!(
            parsed(&[
                "--no-config",
                "spin",
                "--title",
                "Working",
                "--spinner=moon",
                "--show-output",
                "--",
                "sh",
                "-c",
                "exit 3"
            ]),
            Ok(Parsed::Run(SpinArgs {
                title: "Working".into(),
                spinner: "moon".into(),
                show_output: true,
                show_error: false,
                command: vec!["sh".into(), "-c".into(), "exit 3".into()],
            }))
        );
        // Without `--`, the first word starts the command, and its options
        // are its own.
        let Ok(Parsed::Run(spin)) = parsed(&["spin", "ls", "--help"]) else {
            panic!("a command");
        };
        assert_eq!(spin.command, ["ls", "--help"]);
        assert_eq!(spin.title, "Loading...");
        assert_eq!(spin.spinner, "dots");
        assert_eq!(parsed(&["spin", "--help"]), Ok(Parsed::Help));
    }

    #[test]
    fn refuses_a_missing_command_and_an_unknown_spinner() {
        assert!(parsed(&["spin", "--title", "x"])
            .unwrap_err()
            .contains("no command"));
        assert!(parsed(&["spin", "--"]).unwrap_err().contains("no command"));
        assert!(parsed(&["spin", "--spinner", "nope", "--", "true"])
            .unwrap_err()
            .contains("no spinner named \"nope\""));
        assert!(parsed(&["spin", "--title"])
            .unwrap_err()
            .contains("needs a value"));
        assert!(parsed(&["spin", "--bogus", "--", "true"])
            .unwrap_err()
            .contains("unknown option --bogus"));
    }

    #[test]
    fn a_frame_is_the_spinner_and_the_title_cut_to_the_width() {
        let console = rich::Console::builder()
            .force_terminal(true)
            .no_color(true)
            .build();
        let status = rich::Status::new("Working [hard]".replace('[', "\\[")).spinner("line");
        assert_eq!(
            frame(&status, Duration::ZERO, 40, &console),
            "- Working [hard]"
        );
        // `line` turns every 130 ms.
        assert_eq!(
            frame(&status, Duration::from_millis(140), 40, &console),
            "\\ Working [hard]"
        );
        assert_eq!(frame(&status, Duration::ZERO, 8, &console), "- Worki…");
    }

    #[test]
    fn animation_follows_the_accessibility_settings() {
        let env = |pairs: &[(&str, &str)]| {
            pairs.iter().fold(
                rich_ext::capabilities::MapEnvironment::new(),
                |env, (name, value)| env.var(name, value),
            )
        };
        assert!(animation_allowed(&env(&[])));
        assert!(!animation_allowed(&env(&[("RICH_A11Y", "reduced-motion")])));
        assert!(animation_allowed(&env(&[
            ("RICH_A11Y", "reduced-motion"),
            ("RICH_ANIMATION", "1")
        ])));
    }
}

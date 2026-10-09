//! `rich serve [OPTIONS] -- PROGRAM [ARGS...]`: run a terminal program in
//! a web browser, one copy per tab, on a pseudo-terminal of its own
//! (rs-rich-web, 0.0.18 plan, workstream 7). Not upstream: a CLI
//! convenience over the rs-rich-web library, behind the off-by-default
//! `serve` feature, so a default build has no network server.
//!
//! It prints the address to open, with its token, and serves until Ctrl+C,
//! which ends it and every session's program (on Unix SIGTERM and SIGHUP
//! too; each program is hung up on, and killed with its process group if
//! it is still running a second later, before `rich` exits). It listens on
//! 127.0.0.1 unless told otherwise; the token, the `Origin` check and the
//! session cap are rs-rich-web's.
use super::*;

use rich_ext::cli_doc::{ArgSpec, CommandSpec};

/// The port unless `--port` gives one.
const DEFAULT_PORT: u16 = 8080;

/// Whether the first word that is not an option is `serve`, with no render
/// mode flag before it (`rich -p serve` prints the word).
pub(super) fn requested(args: &[String]) -> bool {
    subcommand_word(args) == Some("serve")
}

/// The command's help: registered with the root spec in `cli_spec`.
pub(super) fn command() -> CommandSpec {
    CommandSpec::new("serve")
        .about(
            "Run PROGRAM in a web browser: one copy per tab, each on a pseudo-terminal of its \
             own, drawn by xterm.js. Prints the address to open, with its token; Ctrl+C stops \
             it. Built with the `serve` feature",
        )
        .usage(
            "serve [--bind ADDR] [--port N] [--max-sessions N] [--allow-origin ORIGIN]... \
             -- PROGRAM [ARGS...]",
        )
        .arg(ArgSpec::option("bind").value_name("ADDR").help(
            "The address to listen on (default 127.0.0.1, this computer only). Anything \
                     else lets other machines reach it: put a reverse proxy with TLS and \
                     authentication in front",
        ))
        .arg(
            ArgSpec::option("port")
                .value_name("N")
                .help("The port to listen on (default 8080; 0 picks a free one)"),
        )
        .arg(
            ArgSpec::option("max-sessions")
                .value_name("N")
                .help("Run at most N programs (browser tabs) at once (default 8)"),
        )
        .arg(
            ArgSpec::option("allow-origin")
                .value_name("ORIGIN")
                .multiple(true)
                .help(
                    "Also accept the page from ORIGIN, such as https://term.example.com: where \
                     a reverse proxy serves it. The token is still required",
                ),
        )
        .arg(
            ArgSpec::positional("PROGRAM")
                .multiple(true)
                .help("The program and its arguments, after `--`"),
        )
        .example("rich serve -- htop", "htop in a browser tab")
        .example(
            "rich serve --port 9000 -- bash -l",
            "A login shell on port 9000 (anyone with the address can use it)",
        )
}

#[derive(Debug, PartialEq, Eq)]
struct Args {
    help: bool,
    bind: String,
    port: u16,
    max_sessions: Option<usize>,
    origins: Vec<String>,
    command: Vec<String>,
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut parsed = Args {
        help: false,
        bind: "127.0.0.1".into(),
        port: DEFAULT_PORT,
        max_sessions: None,
        origins: Vec::new(),
        command: Vec::new(),
    };
    let mut seen_command = false;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if !arg.starts_with('-') || arg == "-" {
            if !seen_command && arg == "serve" {
                seen_command = true;
                continue;
            }
            // The program: it and everything after it are its own.
            parsed.command.push(arg.clone());
            parsed.command.extend(iter.cloned());
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
                parsed.command.extend(iter.cloned());
                break;
            }
            "--bind" => parsed.bind = value()?,
            "--port" => {
                let text = value()?;
                parsed.port = text.parse().map_err(|_| {
                    format!("--port takes a port number (0 to 65535), not {text:?}")
                })?;
            }
            "--max-sessions" => {
                let text = value()?;
                match text.parse::<usize>() {
                    Ok(n) if n >= 1 => parsed.max_sessions = Some(n),
                    _ => {
                        return Err(format!(
                            "--max-sessions takes a number from 1, not {text:?}"
                        ))
                    }
                }
            }
            "--allow-origin" => {
                let origin = value()?;
                if !(origin.starts_with("http://") || origin.starts_with("https://")) {
                    return Err(format!(
                        "--allow-origin takes an origin such as https://term.example.com, not \
                         {origin:?}"
                    ));
                }
                parsed.origins.push(origin);
            }
            "--help" => parsed.help = true,
            "--no-color" | "--no-config" => {}
            other => return Err(format!("unknown option {other} for `rich serve`")),
        }
    }
    if parsed.command.is_empty() && !parsed.help {
        return Err("`rich serve` needs a PROGRAM to run, after `--`".into());
    }
    Ok(parsed)
}

/// `host` and `port` as an address to bind: an IPv6 address in brackets.
fn address(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

pub(super) fn dispatch(args: &[String]) -> ExitCode {
    let json = wants_json_report(args);
    let no_color = cli_spec::no_color_requested(args);
    let args = match parse_args(args) {
        Ok(args) => args,
        Err(message) => {
            return emit_error(
                json,
                ExitClass::Usage,
                &format!("{message} (try `rich serve --help`)"),
            )
        }
    };
    // `--help` before the program: the program's own `--help` is its own.
    if args.help {
        if let Some(help) = cli_spec::subcommand_help(&["serve"], no_color) {
            authoring::out(&format!("{help}\n"));
        }
        return ExitCode::SUCCESS;
    }
    let addr = address(&args.bind, args.port);
    // As the shell gave them, even when they are not UTF-8.
    let command: Vec<std::ffi::OsString> = args
        .command
        .iter()
        .map(|arg| fs_path(arg).into_os_string())
        .collect();
    let mut server = match rich_web::Server::bind_command(addr.as_str(), command) {
        Ok(server) => server,
        Err(error) => {
            return emit_error(
                json,
                ExitClass::Input,
                &format!("cannot listen on {addr}: {error}"),
            )
        }
    };
    if let Some(n) = args.max_sessions {
        server = server.max_sessions(n);
    }
    for origin in args.origins {
        server = server.allow_origin(origin);
    }
    // Serves until Ctrl+C. On Unix the signal is waited for, so every
    // program is ended (and killed, if it ignores the hang-up) before
    // `rich` exits; elsewhere Ctrl+C ends the process, and the programs'
    // terminals with it.
    #[cfg(unix)]
    let (served, signal) = {
        let mut signal = None;
        let served = match stop_signals::block() {
            Some(set) => server.run_until(|| signal = Some(stop_signals::wait(&set))),
            None => server.run(),
        };
        (served, signal)
    };
    #[cfg(not(unix))]
    let (served, signal) = (server.run(), None::<i32>);
    match served {
        // As a shell reports a process a signal ended: 128 and its number
        // (SIGINT, SIGTERM and SIGHUP are all small).
        Ok(()) => signal.map_or(ExitCode::SUCCESS, |signal| {
            ExitCode::from(128 + signal as u8)
        }),
        Err(error) => emit_error(json, ExitClass::Input, &format!("serving {addr}: {error}")),
    }
}

/// Ctrl+C, `kill` and a closed terminal, waited for as a request to stop.
#[cfg(unix)]
mod stop_signals {
    /// Block SIGINT, SIGTERM and SIGHUP on this thread, and so on every
    /// thread started from it (a program gets an empty mask from
    /// portable-pty), to [`wait`] for them; `None` when they cannot be.
    #[allow(unsafe_code)]
    pub(super) fn block() -> Option<libc::sigset_t> {
        // SAFETY: the set is initialised by `sigemptyset` before it is
        // used, and `pthread_sigmask` only reads it.
        unsafe {
            let mut set: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut set);
            for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
                libc::sigaddset(&mut set, signal);
            }
            (libc::pthread_sigmask(libc::SIG_BLOCK, &set, std::ptr::null_mut()) == 0).then_some(set)
        }
    }

    /// Wait for one of the signals in `set`: its number.
    #[allow(unsafe_code)]
    pub(super) fn wait(set: &libc::sigset_t) -> i32 {
        let mut signal = 0;
        // SAFETY: `set` is a signal set made by `block`, and `signal` a
        // place to write the one that came. It fails only for a set that
        // is not valid.
        while unsafe { libc::sigwait(set, &mut signal) } != 0 {}
        signal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn arguments_parse() {
        let parsed = parse_args(&args(&[
            "serve",
            "--bind",
            "0.0.0.0",
            "--port=9000",
            "--max-sessions",
            "2",
            "--allow-origin",
            "https://a.example",
            "--allow-origin=https://b.example",
            "--",
            "htop",
            "-d",
            "10",
        ]))
        .unwrap();
        assert_eq!(
            parsed,
            Args {
                help: false,
                bind: "0.0.0.0".into(),
                port: 9000,
                max_sessions: Some(2),
                origins: vec!["https://a.example".into(), "https://b.example".into()],
                command: args(&["htop", "-d", "10"]),
            }
        );
        // The defaults; the program's own options are its own, `--` or not.
        let parsed = parse_args(&args(&["serve", "bash", "-l", "--port", "1"])).unwrap();
        assert_eq!((parsed.bind.as_str(), parsed.port), ("127.0.0.1", 8080));
        assert_eq!(parsed.max_sessions, None);
        assert_eq!(parsed.command, args(&["bash", "-l", "--port", "1"]));
        let parsed = parse_args(&args(&["serve", "--", "--weird-name"])).unwrap();
        assert_eq!(parsed.command, args(&["--weird-name"]));
        // Help needs no program; a program's `--help` is the program's.
        assert!(parse_args(&args(&["serve", "--help"])).unwrap().help);
        let parsed = parse_args(&args(&["serve", "--", "htop", "--help"])).unwrap();
        assert!(!parsed.help);
        assert_eq!(parsed.command, args(&["htop", "--help"]));
    }

    #[test]
    fn bad_arguments_are_refused() {
        for (words, why) in [
            (&["serve"][..], "needs a PROGRAM"),
            (&["serve", "--"][..], "needs a PROGRAM"),
            (&["serve", "--port", "70000", "--", "x"][..], "--port takes"),
            (&["serve", "--port"][..], "--port needs a value"),
            (
                &["serve", "--max-sessions", "0", "--", "x"][..],
                "--max-sessions takes",
            ),
            (
                &["serve", "--allow-origin", "evil", "--", "x"][..],
                "--allow-origin takes",
            ),
            (
                &["serve", "--frobnicate", "--", "x"][..],
                "unknown option --frobnicate",
            ),
        ] {
            let message = parse_args(&args(words)).unwrap_err();
            assert!(message.contains(why), "{words:?}: {message}");
        }
    }

    #[test]
    fn addresses_take_brackets_for_ipv6() {
        assert_eq!(address("127.0.0.1", 80), "127.0.0.1:80");
        assert_eq!(address("localhost", 8080), "localhost:8080");
        assert_eq!(address("::1", 8080), "[::1]:8080");
        assert_eq!(address("[::]", 1), "[::]:1");
    }

    #[test]
    fn the_word_is_claimed_without_a_mode_flag() {
        assert!(requested(&args(&["serve", "--", "htop"])));
        assert!(!requested(&args(&["-p", "serve"])));
        assert!(!requested(&args(&["README.md"])));
    }

    #[test]
    fn help_lists_the_options() {
        let help = cli_spec::subcommand_help(&["serve"], true).expect("serve's help");
        for phrase in [
            "--bind",
            "--port",
            "--max-sessions",
            "--allow-origin",
            "PROGRAM",
            "rich serve -- htop",
        ] {
            assert!(help.contains(phrase), "{phrase} in {help}");
        }
    }
}

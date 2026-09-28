//! When the terminal is not interactive (#492): detect it, and degrade
//! without control sequences and without blocking.
//!
//! A component needs a terminal on both ends: keys from stdin, repaints on
//! stdout. With either redirected, under CI, with `TERM=dumb`, or when the
//! caller says so, [`run`](crate::run) does not start the terminal session.
//! It follows the [`Fallback`] instead: ask line by line, return the
//! component's default, or fail with the reason.

use std::fmt;
use std::io::{BufRead, IsTerminal, Write};

/// Why a session is not interactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    StdinNotTerminal,
    StdoutNotTerminal,
    /// Painting to standard error ([`Output::Stderr`](crate::session::Output)),
    /// which is not a terminal.
    StderrNotTerminal,
    /// Neither standard input nor a controlling terminal to read keys from.
    NoTerminal,
    /// `CI` is set (and not `0` or `false`).
    Ci,
    /// `TERM=dumb`.
    DumbTerminal,
    /// The caller asked for it ([`Policy::interactive`] is `Some(false)`).
    Requested,
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Reason::StdinNotTerminal => "standard input is not a terminal",
            Reason::StdoutNotTerminal => "standard output is not a terminal",
            Reason::StderrNotTerminal => "standard error is not a terminal",
            Reason::NoTerminal => "there is no terminal to read keys from",
            Reason::Ci => "running under CI",
            Reason::DumbTerminal => "TERM is dumb",
            Reason::Requested => "interactive mode is turned off",
        })
    }
}

/// What to do when the terminal is not interactive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fallback {
    /// Ask line by line on stdin and stderr, through
    /// [`Component::prompt`](crate::Component::prompt); a component without
    /// a line form uses its default instead.
    #[default]
    Prompt,
    /// Return [`Component::default_value`](crate::Component::default_value).
    Default,
    /// Fail with [`NotInteractive::Terminal`].
    Error,
}

/// How [`run`](crate::run) decides whether to be interactive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Policy {
    /// `Some(true)` forces interactive mode (when both ends are terminals
    /// anyway: raw mode needs one), `Some(false)` forbids it, `None` detects.
    pub interactive: Option<bool>,
    pub fallback: Fallback,
    /// Read keys from the controlling terminal when standard input is a
    /// pipe, as pagers and pickers do: `ls | app` can still be driven from
    /// the keyboard. Off by default, which degrades whenever standard input
    /// is redirected.
    pub tty_keys: bool,
}

impl Policy {
    /// Detect from the process's own stdin, stdout and environment.
    pub fn detect(&self) -> Result<(), Reason> {
        self.decide(
            std::io::stdin().is_terminal(),
            std::io::stdout().is_terminal(),
            |name| std::env::var(name).ok(),
        )
    }

    /// Detect for a session that paints to `output`: that stream must be a
    /// terminal. With [`tty_keys`](Policy::tty_keys), keys may come from the
    /// controlling terminal while standard input is a pipe
    /// (`ls | rich filter`).
    pub fn detect_for(&self, output: crate::session::Output) -> Result<(), Reason> {
        use std::io::IsTerminal;
        let stdin = std::io::stdin().is_terminal();
        if self.interactive == Some(false) {
            return Err(Reason::Requested);
        }
        if !stdin && !(self.tty_keys && crate::session::keys_available()) {
            return Err(if self.tty_keys {
                Reason::NoTerminal
            } else {
                Reason::StdinNotTerminal
            });
        }
        if !output.is_terminal() {
            return Err(match output {
                crate::session::Output::Stdout => Reason::StdoutNotTerminal,
                crate::session::Output::Stderr => Reason::StderrNotTerminal,
            });
        }
        self.decide(true, true, |name| std::env::var(name).ok())
    }

    /// The decision, from explicit inputs. `Ok` means interactive.
    pub fn decide(
        &self,
        stdin_terminal: bool,
        stdout_terminal: bool,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<(), Reason> {
        if self.interactive == Some(false) {
            return Err(Reason::Requested);
        }
        if !stdin_terminal {
            return Err(Reason::StdinNotTerminal);
        }
        if !stdout_terminal {
            return Err(Reason::StdoutNotTerminal);
        }
        if self.interactive == Some(true) {
            return Ok(());
        }
        if env("TERM").is_some_and(|term| term == "dumb") {
            return Err(Reason::DumbTerminal);
        }
        if env("CI")
            .is_some_and(|ci| !matches!(ci.to_ascii_lowercase().as_str(), "" | "0" | "false"))
        {
            return Err(Reason::Ci);
        }
        Ok(())
    }
}

/// Why a component could not produce a value without a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotInteractive {
    /// The policy says to fail.
    Terminal(Reason),
    /// The policy asked for a default and the component has none.
    NoDefault(Reason),
    /// The component has no line-based form.
    NoPrompt,
    /// The line-based form got something it could not use.
    Invalid(String),
    /// Input ended before an answer and the component has no default.
    /// [`degrade`](crate::degrade) reports it as [`NotInteractive::NoDefault`]
    /// with the reason there is no terminal.
    Ended,
    /// Ctrl+C at a line prompt read without echo.
    /// [`degrade`](crate::degrade) reports it as
    /// [`Outcome::Interrupted`](crate::Outcome::Interrupted).
    Interrupted,
}

impl fmt::Display for NotInteractive {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NotInteractive::Terminal(reason) => write!(f, "not interactive: {reason}"),
            NotInteractive::NoDefault(reason) => {
                write!(f, "not interactive ({reason}) and no default value")
            }
            NotInteractive::NoPrompt => f.write_str("no line-based prompt for this component"),
            NotInteractive::Invalid(message) => f.write_str(message),
            NotInteractive::Ended => f.write_str("input ended without an answer"),
            NotInteractive::Interrupted => f.write_str("interrupted"),
        }
    }
}

impl std::error::Error for NotInteractive {}

/// Line-based input and output for a degraded component: plain text, no
/// control sequences.
pub trait LineIo {
    fn write(&mut self, text: &str);
    /// One line without its line break, or `None` at end of input.
    fn read_line(&mut self) -> Option<String>;
    /// One line for a secret, typed without echo where there is echo to
    /// turn off: `Ok(Some(line))`, `Ok(None)` when the user backed out
    /// (Escape), [`NotInteractive::Ended`] at end of input and
    /// [`NotInteractive::Interrupted`] on Ctrl+C. The default reads a line
    /// as usual.
    fn read_secret(&mut self) -> Result<Option<String>, NotInteractive> {
        self.read_line().map(Some).ok_or(NotInteractive::Ended)
    }
}

/// [`LineIo`] on the process: prompts to stderr (stdout may be the data a
/// script is capturing), answers from stdin.
pub struct StdLineIo;

impl LineIo for StdLineIo {
    fn write(&mut self, text: &str) {
        let mut stderr = std::io::stderr().lock();
        let _ = stderr.write_all(text.as_bytes());
        let _ = stderr.flush();
    }

    fn read_line(&mut self) -> Option<String> {
        let mut line = String::new();
        match std::io::stdin().lock().read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    }

    /// With stdin a terminal (stdout redirected, say), read with echo off,
    /// as Python's `getpass` does: the terminal is in raw mode for the one
    /// line, and given back after it. Backspace and Ctrl+U edit; Escape
    /// backs out, Ctrl+D on an empty line ends the input and Ctrl+C
    /// interrupts. A terminating signal (SIGTERM, SIGHUP, SIGQUIT) while
    /// the line is read gives the terminal back before the process ends.
    /// From a pipe, read a line as usual.
    fn read_secret(&mut self) -> Result<Option<String>, NotInteractive> {
        if !std::io::stdin().is_terminal() {
            return self.read_line().map(Some).ok_or(NotInteractive::Ended);
        }
        let answer = read_hidden();
        // The Enter was not echoed either: end the prompt's line.
        self.write("\n");
        answer
    }
}

/// One line read in raw mode, so nothing typed is echoed.
fn read_hidden() -> Result<Option<String>, NotInteractive> {
    use crate::event::{from_crossterm, Event, KeyCode};

    /// Raw mode off again on every way out, a panic and a terminating
    /// signal included.
    struct Raw;
    impl Drop for Raw {
        fn drop(&mut self) {
            crate::session::raw_line(false);
            let _ = crossterm::terminal::disable_raw_mode();
        }
    }
    crate::session::raw_line(true);
    if crossterm::terminal::enable_raw_mode().is_err() {
        crate::session::raw_line(false);
        return Err(NotInteractive::Ended);
    }
    let _raw = Raw;
    let mut line = String::new();
    loop {
        let event = crossterm::event::read().map_err(|_| NotInteractive::Ended)?;
        match from_crossterm(event) {
            Some(Event::Key(key)) => {
                let ctrl = key.modifiers.ctrl;
                match key.code {
                    KeyCode::Enter => return Ok(Some(line)),
                    KeyCode::Escape => return Ok(None),
                    KeyCode::Char('c') if ctrl => return Err(NotInteractive::Interrupted),
                    KeyCode::Char('d') if ctrl && line.is_empty() => {
                        return Err(NotInteractive::Ended)
                    }
                    KeyCode::Char('u') if ctrl => line.clear(),
                    KeyCode::Backspace => {
                        line.pop();
                    }
                    KeyCode::Char(c) if !ctrl => line.push(c),
                    _ => {}
                }
            }
            Some(Event::Paste(text)) => line.push_str(&crate::components::pasted(&text, "")),
            _ => {}
        }
    }
}

/// [`LineIo`] from a script, for tests: answers in order, prompts kept.
#[derive(Clone, Debug, Default)]
pub struct ScriptedLineIo {
    pub answers: std::collections::VecDeque<String>,
    pub written: String,
    /// How many answers were read as secrets.
    pub secrets: usize,
}

impl ScriptedLineIo {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(answers: I) -> ScriptedLineIo {
        ScriptedLineIo {
            answers: answers.into_iter().map(Into::into).collect(),
            written: String::new(),
            secrets: 0,
        }
    }
}

impl LineIo for ScriptedLineIo {
    fn write(&mut self, text: &str) {
        self.written.push_str(text);
    }

    fn read_line(&mut self) -> Option<String> {
        self.answers.pop_front()
    }

    fn read_secret(&mut self) -> Result<Option<String>, NotInteractive> {
        self.secrets += 1;
        self.read_line().map(Some).ok_or(NotInteractive::Ended)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn decides_from_terminals_and_environment() {
        let auto = Policy::default();
        assert_eq!(auto.decide(true, true, env(&[])), Ok(()));
        assert_eq!(
            auto.decide(false, true, env(&[])),
            Err(Reason::StdinNotTerminal)
        );
        assert_eq!(
            auto.decide(true, false, env(&[])),
            Err(Reason::StdoutNotTerminal)
        );
        assert_eq!(
            auto.decide(true, true, env(&[("TERM", "dumb")])),
            Err(Reason::DumbTerminal)
        );
        assert_eq!(
            auto.decide(true, true, env(&[("CI", "true")])),
            Err(Reason::Ci)
        );
        assert_eq!(auto.decide(true, true, env(&[("CI", "false")])), Ok(()));
        assert_eq!(auto.decide(true, true, env(&[("CI", "")])), Ok(()));
        let forced = Policy {
            interactive: Some(true),
            ..Policy::default()
        };
        assert_eq!(forced.decide(true, true, env(&[("CI", "1")])), Ok(()));
        // Forcing cannot conjure a terminal.
        assert_eq!(
            forced.decide(false, true, env(&[])),
            Err(Reason::StdinNotTerminal)
        );
        let off = Policy {
            interactive: Some(false),
            ..Policy::default()
        };
        assert_eq!(off.decide(true, true, env(&[])), Err(Reason::Requested));
    }

    #[test]
    fn detect_for_honours_a_request_first_whatever_the_output() {
        let off = Policy {
            interactive: Some(false),
            tty_keys: true,
            ..Policy::default()
        };
        for output in [crate::Output::Stdout, crate::Output::Stderr] {
            assert_eq!(off.detect_for(output), Err(Reason::Requested));
        }
    }
}

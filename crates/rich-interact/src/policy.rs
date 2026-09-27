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
    /// The line-based form got something it could not use, or input ended.
    Invalid(String),
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
}

/// [`LineIo`] from a script, for tests: answers in order, prompts kept.
#[derive(Clone, Debug, Default)]
pub struct ScriptedLineIo {
    pub answers: std::collections::VecDeque<String>,
    pub written: String,
}

impl ScriptedLineIo {
    pub fn new<I: IntoIterator<Item = S>, S: Into<String>>(answers: I) -> ScriptedLineIo {
        ScriptedLineIo {
            answers: answers.into_iter().map(Into::into).collect(),
            written: String::new(),
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
}

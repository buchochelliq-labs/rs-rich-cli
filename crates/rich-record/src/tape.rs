//! The tape format: one step per line.
//!
//! ```text
//! # A comment.
//! Set Size 100x28            # columns x rows (default 100x28)
//! Set TypingDelay 40ms       # per character typed by `Type`
//! Set Timeout 15s            # default for `Wait`
//! Set Title "Watching files" # caption for the window frame and the cast
//! Set Env NAME value         # extra environment for the shell and `Exec`
//! Write data.json '{"a": 1}' # create a file in the workspace (\n, \t escapes)
//! Exec "sed -i s/1/2/ data.json"   # run a command outside the terminal
//! Type "rich data.json"      # type into the shell, one character at a time
//! Enter  Tab  Space  Backspace  Escape  Up  Down  Left  Right
//! Home  End  PageUp  PageDown  Ctrl+C   # keys; an optional count repeats
//! Sleep 500ms
//! Wait "text"                # until the screen shows it (or /regex/ [timeout])
//! Screenshot name
//! Hide / Show                # steps between them are not recorded
//! Resize 80x24
//! ```
//!
//! This is the format of the first, Python tape runner (#598), unchanged, so
//! its tapes run under `rich record` as they are.

use std::fmt;
use std::time::Duration;

use fancy_regex::Regex;

/// A parse or run error, with the tape line it came from (0 when none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapeError {
    pub line: usize,
    pub message: String,
}

impl TapeError {
    pub fn new(line: usize, message: impl Into<String>) -> Self {
        TapeError {
            line,
            message: message.into(),
        }
    }
}

impl fmt::Display for TapeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line > 0 {
            write!(f, "line {}: {}", self.line, self.message)
        } else {
            f.write_str(&self.message)
        }
    }
}

impl std::error::Error for TapeError {}

/// A key `Type` cannot send as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Enter,
    Tab,
    Space,
    Backspace,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    /// `Ctrl+` a letter or one of `@[\]^_`.
    Ctrl(char),
}

impl Key {
    fn parse(name: &str) -> Option<Key> {
        Some(match name {
            "Enter" => Key::Enter,
            "Tab" => Key::Tab,
            "Space" => Key::Space,
            "Backspace" => Key::Backspace,
            "Escape" => Key::Escape,
            "Up" => Key::Up,
            "Down" => Key::Down,
            "Left" => Key::Left,
            "Right" => Key::Right,
            "Home" => Key::Home,
            "End" => Key::End,
            "PageUp" => Key::PageUp,
            "PageDown" => Key::PageDown,
            _ => {
                let rest = name.strip_prefix("Ctrl+")?;
                let mut chars = rest.chars();
                let letter = chars.next()?.to_ascii_uppercase();
                if chars.next().is_some() || !('@'..='_').contains(&letter) {
                    return None;
                }
                Key::Ctrl(letter)
            }
        })
    }

    /// The bytes the key sends to the terminal.
    pub fn bytes(self) -> String {
        match self {
            Key::Enter => "\r".into(),
            Key::Tab => "\t".into(),
            Key::Space => " ".into(),
            Key::Backspace => "\x7f".into(),
            Key::Escape => "\x1b".into(),
            Key::Up => "\x1b[A".into(),
            Key::Down => "\x1b[B".into(),
            Key::Right => "\x1b[C".into(),
            Key::Left => "\x1b[D".into(),
            Key::Home => "\x1b[H".into(),
            Key::End => "\x1b[F".into(),
            Key::PageUp => "\x1b[5~".into(),
            Key::PageDown => "\x1b[6~".into(),
            Key::Ctrl(letter) => char::from(letter as u8 - b'@').to_string(),
        }
    }

    /// How the key overlay shows it.
    pub fn label(self) -> String {
        match self {
            Key::Enter => "⏎".into(),
            Key::Tab => "⇥".into(),
            Key::Space => "␣".into(),
            Key::Backspace => "⌫".into(),
            Key::Escape => "Esc".into(),
            Key::Up => "↑".into(),
            Key::Down => "↓".into(),
            Key::Right => "→".into(),
            Key::Left => "←".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PgUp".into(),
            Key::PageDown => "PgDn".into(),
            Key::Ctrl(letter) => format!("Ctrl+{letter}"),
        }
    }
}

/// What `Wait` waits for.
#[derive(Debug, Clone)]
pub enum Pattern {
    Text(String),
    Regex(Regex),
}

impl Pattern {
    pub fn is_match(&self, screen: &str) -> bool {
        match self {
            Pattern::Text(text) => screen.contains(text.as_str()),
            Pattern::Regex(regex) => regex.is_match(screen).unwrap_or(false),
        }
    }
}

impl fmt::Display for Pattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pattern::Text(text) => write!(f, "{text:?}"),
            Pattern::Regex(regex) => write!(f, "/{}/", regex.as_str()),
        }
    }
}

/// One step of a tape.
#[derive(Debug, Clone)]
pub enum Step {
    TypingDelay(Duration),
    Timeout(Duration),
    Type(String),
    Key {
        key: Key,
        count: u32,
    },
    Sleep(Duration),
    Wait {
        pattern: Pattern,
        timeout: Option<Duration>,
    },
    Screenshot(String),
    Hide,
    Show,
    Resize {
        columns: u16,
        rows: u16,
    },
    Write {
        path: String,
        content: String,
    },
    Exec(String),
}

/// A parsed tape: its settings, and its steps with their line numbers.
#[derive(Debug, Clone)]
pub struct Tape {
    pub columns: u16,
    pub rows: u16,
    pub title: Option<String>,
    pub env: Vec<(String, String)>,
    pub steps: Vec<(usize, Step)>,
}

impl Tape {
    /// The names of the screenshots the tape takes, in order.
    pub fn screenshots(&self) -> Vec<&str> {
        self.steps
            .iter()
            .filter_map(|(_, step)| match step {
                Step::Screenshot(name) => Some(name.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// `500ms` or `2s` (fractions allowed).
pub fn parse_duration(text: &str) -> Option<Duration> {
    let (number, scale) = if let Some(ms) = text.strip_suffix("ms") {
        (ms, 0.001)
    } else {
        (text.strip_suffix('s')?, 1.0)
    };
    let value: f64 = number.parse().ok()?;
    (value.is_finite() && value >= 0.0).then(|| Duration::from_secs_f64(value * scale))
}

/// `100x28`.
pub fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (columns, rows) = text.split_once('x')?;
    let (columns, rows) = (columns.parse().ok()?, rows.parse().ok()?);
    (columns > 0 && rows > 0).then_some((columns, rows))
}

/// `Write`'s escapes: `\n`, `\t`, `\\`, `\"` and `\'`. Anything else,
/// Unicode included, is kept as written.
pub fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(&c @ ('\\' | '"' | '\'')) => out.push(c),
            _ => {
                out.push('\\');
                continue;
            }
        }
        chars.next();
    }
    out
}

/// Split a line into words as a POSIX shell would (`shlex`): whitespace
/// separates, quotes group, `\` escapes outside single quotes, and `#`
/// starts a comment at the start of a word.
fn words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '#' if !in_word => break,
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unclosed single quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        // In double quotes a backslash escapes only these.
                        Some('\\') => match chars.peek() {
                            Some(&c @ ('"' | '\\' | '$' | '`')) => {
                                word.push(c);
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        Some(c) => word.push(c),
                        None => return Err("unclosed double quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                if let Some(c) = chars.next() {
                    word.push(c);
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        words.push(word);
    }
    Ok(words)
}

/// Parse a tape.
pub fn parse(source: &str) -> Result<Tape, TapeError> {
    let mut tape = Tape {
        columns: 100,
        rows: 28,
        title: None,
        env: Vec::new(),
        steps: Vec::new(),
    };
    let wait_regex = Regex::new(r"^Wait\s+/(.*)/(?:\s+(\S+))?$").expect("valid regex");
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let error = |message: String| TapeError::new(number, message);
        // A /regex/ is kept whole, backslashes included.
        if let Ok(Some(captures)) = wait_regex.captures(trimmed) {
            let regex = Regex::new(&captures[1])
                .map_err(|e| error(format!("bad regex /{}/: {e}", &captures[1])))?;
            let timeout = match captures.get(2) {
                Some(value) => Some(
                    parse_duration(value.as_str())
                        .ok_or_else(|| error(format!("bad duration {:?}", value.as_str())))?,
                ),
                None => None,
            };
            tape.steps.push((
                number,
                Step::Wait {
                    pattern: Pattern::Regex(regex),
                    timeout,
                },
            ));
            continue;
        }
        let words = words(trimmed).map_err(error)?;
        let (command, args) = words.split_first().expect("a non-empty line");
        let arg = |index: usize| -> Result<&str, TapeError> {
            args.get(index)
                .map(String::as_str)
                .ok_or_else(|| error(format!("{command} needs an argument")))
        };
        let duration = |text: &str| {
            parse_duration(text)
                .ok_or_else(|| error(format!("bad duration {text:?} (use 500ms or 2s)")))
        };
        let size = |text: &str| {
            parse_size(text).ok_or_else(|| error(format!("bad size {text:?} (use 100x28)")))
        };
        let step = match command.as_str() {
            "Set" => {
                let value = arg(1)?;
                match arg(0)? {
                    "Size" => {
                        (tape.columns, tape.rows) = size(value)?;
                        continue;
                    }
                    "Title" => {
                        tape.title = Some(value.to_string());
                        continue;
                    }
                    "Env" => {
                        let value = args.get(2).cloned().unwrap_or_default();
                        tape.env.push((args[1].clone(), value));
                        continue;
                    }
                    "TypingDelay" => Step::TypingDelay(duration(value)?),
                    "Timeout" => Step::Timeout(duration(value)?),
                    other => return Err(error(format!("unknown setting {other}"))),
                }
            }
            "Type" => Step::Type(args.join(" ")),
            "Sleep" => Step::Sleep(duration(arg(0)?)?),
            "Wait" => Step::Wait {
                pattern: Pattern::Text(arg(0)?.to_string()),
                timeout: args.get(1).map(|t| duration(t)).transpose()?,
            },
            "Screenshot" => {
                let name = arg(0)?;
                if name.is_empty()
                    || !name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                {
                    return Err(error(format!(
                        "screenshot name {name:?} must be letters, digits, - or _"
                    )));
                }
                Step::Screenshot(name.to_string())
            }
            "Hide" => Step::Hide,
            "Show" => Step::Show,
            "Resize" => {
                let (columns, rows) = size(arg(0)?)?;
                Step::Resize { columns, rows }
            }
            "Write" => Step::Write {
                path: arg(0)?.to_string(),
                content: unescape(arg(1)?),
            },
            "Exec" => Step::Exec(arg(0)?.to_string()),
            name => match Key::parse(name) {
                Some(key) => {
                    let count = match args.first() {
                        Some(count) => count
                            .parse()
                            .ok()
                            .filter(|&n| n > 0)
                            .ok_or_else(|| error(format!("bad count {count:?}")))?,
                        None => 1,
                    };
                    Step::Key { key, count }
                }
                None => return Err(error(format!("unknown command {name}"))),
            },
        };
        tape.steps.push((number, step));
    }
    Ok(tape)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_step() {
        let tape = parse(
            "# demo\nSet Size 80x20\nSet Title \"A demo\"\nSet Env PAGER less\n\
             Write a.txt \"caf\u{e9}\\n\"\nExec \"echo hi > b\"\nType \"rich a.txt\"\n\
             Enter\nDown 3\nCtrl+C\nSleep 500ms\nWait \"done\" 2s\nWait /x\\s+y/ 3s\n\
             Screenshot shot-1\nHide\nShow\nResize 40x10\nSet TypingDelay 10ms\n",
        )
        .unwrap();
        assert_eq!((tape.columns, tape.rows), (80, 20));
        assert_eq!(tape.title.as_deref(), Some("A demo"));
        assert_eq!(tape.env, [("PAGER".to_string(), "less".to_string())]);
        assert_eq!(tape.screenshots(), ["shot-1"]);
        assert!(matches!(&tape.steps[0].1, Step::Write { content, .. } if content == "café\n"));
        assert!(matches!(
            tape.steps[4].1,
            Step::Key {
                key: Key::Down,
                count: 3
            }
        ));
        assert!(matches!(
            tape.steps[5].1,
            Step::Key {
                key: Key::Ctrl('C'),
                ..
            }
        ));
        match &tape.steps[8].1 {
            Step::Wait { pattern, timeout } => {
                assert!(pattern.is_match("a x  y b"));
                assert_eq!(*timeout, Some(Duration::from_secs(3)));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(tape.steps[8].0, 13);
    }

    #[test]
    fn reports_the_failing_line() {
        let error = parse("Type \"ok\"\nFrobnicate\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert!(error.message.contains("Frobnicate"));
        assert!(parse("Sleep soon")
            .unwrap_err()
            .message
            .contains("duration"));
        assert!(parse("Screenshot ../x").is_err());
        assert!(parse("Type \"open").is_err());
    }

    #[test]
    fn words_follow_shell_quoting() {
        assert_eq!(
            words(r#"Exec "sed -i 's/\"a\": 2/\"a\": 5/' f" # note"#).unwrap(),
            ["Exec", r#"sed -i 's/"a": 2/"a": 5/' f"#]
        );
        assert_eq!(words("Type 'a # b'").unwrap(), ["Type", "a # b"]);
        assert_eq!(unescape(r"x\d\n\\"), "x\\d\n\\");
    }

    #[test]
    fn keys_send_their_bytes() {
        assert_eq!(Key::Ctrl('C').bytes(), "\x03");
        assert_eq!(Key::Down.bytes(), "\x1b[B");
        assert_eq!(Key::Ctrl('C').label(), "Ctrl+C");
    }
}

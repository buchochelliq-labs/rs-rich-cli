//! The tape format: one step per line.
//!
//! ```text
//! # A comment.
//! Set Size 100x28            # columns x rows (default 100x28; 2x2 to 500x200)
//! Set TypingDelay 40ms       # per character typed by `Type`
//! Set Timeout 15s            # default for `Wait`
//! Set Title "Watching files" # title of the window frame and the cast
//! Set WindowFrame off        # draw the window frame around stills and video (on)
//! Set Caption "Save to see"  # a line of text under stills, video and the page
//! Set KeyOverlay off         # show keys as they are pressed, in video (on)
//! Set Env NAME value         # extra environment for the shell and `Exec`
//! Set Shell zsh              # bash (default), zsh, fish or sh
//! Write data.json '{"a": 1}' # create a file in the workspace (\n, \t escapes)
//! Exec "sed -i s/1/2/ data.json"   # run a command outside the terminal
//! Type "rich data.json"      # type into the shell, one character at a time
//! Enter  Tab  Space  Backspace  Escape  Up  Down  Left  Right
//! Home  End  PageUp  PageDown  Ctrl+C   # keys; an optional count repeats
//! Alt+Left  Ctrl+Shift+PageDown  Shift+F5 # with modifiers
//! Sleep 500ms
//! Wait "text"                # until the screen shows it, or has since the last
//!                            # step began (or /regex/ [timeout])
//! Screenshot name
//! Hide / Show                # steps between them are not recorded
//! Resize 80x24
//! Mask /\/tmp\/\S+/ "<tmp>"  # in text grids only: hide output that varies
//! Output gif png             # write only these (png svg cast gif mp4 html)
//! Output demo.html           # ... or name a file: cast, gif, mp4 or html
//! ```
//!
//! This is the format of the first, Python tape runner (#598), extended with
//! presentation settings and `Output`; its tapes run under `rich record` as
//! they are.

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
    /// `Shift+Tab`.
    ShiftTab,
    Delete,
    Insert,
    /// A function key, `F1` to `F24`.
    F(u8),
    /// `Ctrl+` a letter or one of `@[\]^_`.
    Ctrl(char),
    /// `Alt+` a printable character: Esc, then the character, as terminals
    /// send it.
    Alt(char),
    /// A navigation or function key held with modifiers: `Shift+`, `Alt+`
    /// and `Ctrl+`, in any order, before `Left`, `PageDown`, `F5` and the
    /// like. `mods` is xterm's modifier parameter: 1, plus 1 for Shift, 2
    /// for Alt and 4 for Ctrl.
    Modified {
        key: Modifiable,
        mods: u8,
    },
}

/// A key that takes modifiers in xterm's `CSI 1 ; mods X` and
/// `CSI n ; mods ~` forms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modifiable {
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Delete,
    Insert,
    F(u8),
}

impl Modifiable {
    fn parse(name: &str) -> Option<Modifiable> {
        Some(match Key::parse_plain(name)? {
            Key::Up => Modifiable::Up,
            Key::Down => Modifiable::Down,
            Key::Left => Modifiable::Left,
            Key::Right => Modifiable::Right,
            Key::Home => Modifiable::Home,
            Key::End => Modifiable::End,
            Key::PageUp => Modifiable::PageUp,
            Key::PageDown => Modifiable::PageDown,
            Key::Delete => Modifiable::Delete,
            Key::Insert => Modifiable::Insert,
            Key::F(n) => Modifiable::F(n),
            _ => return None,
        })
    }

    /// The key without its modifiers.
    fn plain(self) -> Key {
        match self {
            Modifiable::Up => Key::Up,
            Modifiable::Down => Key::Down,
            Modifiable::Left => Key::Left,
            Modifiable::Right => Key::Right,
            Modifiable::Home => Key::Home,
            Modifiable::End => Key::End,
            Modifiable::PageUp => Key::PageUp,
            Modifiable::PageDown => Key::PageDown,
            Modifiable::Delete => Key::Delete,
            Modifiable::Insert => Key::Insert,
            Modifiable::F(n) => Key::F(n),
        }
    }

    fn bytes(self, mods: u8) -> String {
        let (number, last) = match self {
            Modifiable::Up => (1, 'A'),
            Modifiable::Down => (1, 'B'),
            Modifiable::Right => (1, 'C'),
            Modifiable::Left => (1, 'D'),
            Modifiable::Home => (1, 'H'),
            Modifiable::End => (1, 'F'),
            Modifiable::PageUp => (5, '~'),
            Modifiable::PageDown => (6, '~'),
            Modifiable::Delete => (3, '~'),
            Modifiable::Insert => (2, '~'),
            Modifiable::F(n @ 1..=4) => (1, char::from(b'P' + n - 1)),
            Modifiable::F(n @ 5..=20) => (u32::from(FUNCTION_CODES[usize::from(n - 5)]), '~'),
            Modifiable::F(n) => (57_363 + u32::from(n), 'u'),
        };
        format!("\x1b[{number};{mods}{last}")
    }
}

impl Key {
    fn parse(name: &str) -> Option<Key> {
        if let Some(key) = Key::parse_plain(name) {
            return Some(key);
        }
        // Modifiers, in any order, each once.
        let (mut rest, mut mods) = (name, 0u8);
        loop {
            let (bit, after) = if let Some(after) = rest.strip_prefix("Shift+") {
                (1, after)
            } else if let Some(after) = rest.strip_prefix("Alt+") {
                (2, after)
            } else if let Some(after) = rest.strip_prefix("Ctrl+") {
                (4, after)
            } else {
                break;
            };
            if mods & bit != 0 {
                return None;
            }
            (rest, mods) = (after, mods | bit);
        }
        if mods == 0 {
            return None;
        }
        if let Some(key) = Modifiable::parse(rest) {
            return Some(Key::Modified {
                key,
                mods: mods + 1,
            });
        }
        let mut chars = rest.chars();
        let (c, more) = (chars.next()?, chars.next().is_some());
        match mods {
            2 if !more && !c.is_control() => Some(Key::Alt(c)),
            4 if !more && ('@'..='_').contains(&c.to_ascii_uppercase()) => {
                Some(Key::Ctrl(c.to_ascii_uppercase()))
            }
            _ => None,
        }
    }

    /// A key named without modifiers, and `Shift+Tab`.
    fn parse_plain(name: &str) -> Option<Key> {
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
            "Shift+Tab" => Key::ShiftTab,
            "Delete" => Key::Delete,
            "Insert" => Key::Insert,
            _ => match name.strip_prefix('F')?.parse() {
                Ok(n @ 1..=24) => Key::F(n),
                _ => return None,
            },
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
            Key::ShiftTab => "\x1b[Z".into(),
            Key::Delete => "\x1b[3~".into(),
            Key::Insert => "\x1b[2~".into(),
            Key::F(n) => function_key(n),
            Key::Ctrl(letter) => char::from(letter as u8 - b'@').to_string(),
            Key::Alt(c) => format!("\x1b{c}"),
            Key::Modified { key, mods } => key.bytes(mods),
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
            Key::ShiftTab => "⇤".into(),
            Key::Delete => "Del".into(),
            Key::Insert => "Ins".into(),
            Key::F(n) => format!("F{n}"),
            Key::Ctrl(letter) => format!("Ctrl+{letter}"),
            Key::Alt(c) => format!("Alt+{c}"),
            Key::Modified { key, mods } => {
                let mut label = String::new();
                for (bit, name) in [(4, "Ctrl+"), (2, "Alt+"), (1, "Shift+")] {
                    if (mods - 1) & bit != 0 {
                        label.push_str(name);
                    }
                }
                label + &key.plain().label()
            }
        }
    }
}

/// The numbers xterm sends F5 to F20 as, in `CSI n ~`.
const FUNCTION_CODES: [u8; 16] = [
    15, 17, 18, 19, 20, 21, 23, 24, 25, 26, 28, 29, 31, 32, 33, 34,
];

/// The bytes of function key `n` (1 to 24). F1 to F20 are the sequences
/// xterm sends; F21 to F24 have none there, so they go as the kitty
/// keyboard protocol's codes, which an app reads once it has turned that
/// protocol on.
fn function_key(n: u8) -> String {
    match n {
        1..=4 => format!("\x1bO{}", char::from(b'P' + n - 1)),
        5..=20 => format!("\x1b[{}~", FUNCTION_CODES[usize::from(n - 5)]),
        _ => format!("\x1b[{}u", 57_363 + u32::from(n)),
    }
}

/// A mouse button a tape presses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
    Right,
}

impl Button {
    /// Its number in an SGR mouse report.
    fn code(self) -> u16 {
        match self {
            Button::Left => 0,
            Button::Middle => 1,
            Button::Right => 2,
        }
    }
}

/// What a mouse step does, at a cell (column, row, from 0 at the top left).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// `Click`, `RightClick` or `MiddleClick`: a press and a release.
    Click(Button),
    /// `DoubleClick`: two left clicks in quick succession.
    DoubleClick,
    /// `Drag`: press the left button, move to `to` a cell at a time, release.
    Drag { to: (u16, u16) },
    /// `ScrollUp` or `ScrollDown`: one turn of the wheel.
    Scroll { up: bool },
    /// `MouseMove`: the pointer moves there with no button held.
    Move,
}

/// A mouse step: what, where, and how many times.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mouse {
    pub action: MouseAction,
    pub at: (u16, u16),
    pub count: u32,
}

impl Mouse {
    /// The reports to send, in order, as SGR mouse sequences (the format
    /// terminals use with mouse reporting on, and crossterm reads).
    pub fn reports(&self) -> Vec<String> {
        let sgr = |code: u16, (x, y): (u16, u16), press: bool| {
            format!(
                "\x1b[<{code};{};{}{}",
                u32::from(x) + 1,
                u32::from(y) + 1,
                if press { 'M' } else { 'm' }
            )
        };
        let click =
            |button: Button, at| vec![sgr(button.code(), at, true), sgr(button.code(), at, false)];
        match self.action {
            MouseAction::Click(button) => click(button, self.at),
            MouseAction::DoubleClick => {
                let mut reports = click(Button::Left, self.at);
                reports.extend(click(Button::Left, self.at));
                reports
            }
            MouseAction::Drag { to } => {
                let mut reports = vec![sgr(0, self.at, true)];
                let (mut x, mut y) = self.at;
                while (x, y) != to {
                    x = step_towards(x, to.0);
                    y = step_towards(y, to.1);
                    reports.push(sgr(32, (x, y), true));
                }
                reports.push(sgr(0, to, false));
                reports
            }
            MouseAction::Scroll { up } => vec![sgr(if up { 64 } else { 65 }, self.at, true)],
            MouseAction::Move => vec![sgr(35, self.at, true)],
        }
    }

    /// How the key overlay shows it.
    pub fn label(&self) -> String {
        let what = match self.action {
            MouseAction::Click(Button::Left) => "Click",
            MouseAction::Click(Button::Right) => "Right click",
            MouseAction::Click(Button::Middle) => "Middle click",
            MouseAction::DoubleClick => "Double click",
            MouseAction::Drag { .. } => "Drag",
            MouseAction::Scroll { up: true } => "Scroll ↑",
            MouseAction::Scroll { up: false } => "Scroll ↓",
            MouseAction::Move => "Move",
        };
        format!("🖱 {what}")
    }
}

fn step_towards(from: u16, to: u16) -> u16 {
    match from.cmp(&to) {
        std::cmp::Ordering::Less => from + 1,
        std::cmp::Ordering::Greater => from - 1,
        std::cmp::Ordering::Equal => from,
    }
}

/// An output format `rich record` can write. Text grids are always written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Format {
    /// A PNG per screenshot.
    Png,
    /// An SVG per screenshot.
    Svg,
    /// The asciinema cast.
    Cast,
    Gif,
    /// Needs FFmpeg.
    Mp4,
    /// A self-contained page: a player and the screenshots.
    Html,
}

impl Format {
    pub const ALL: [Format; 6] = [
        Format::Png,
        Format::Svg,
        Format::Cast,
        Format::Gif,
        Format::Mp4,
        Format::Html,
    ];

    /// `png`, `svg`, `cast`, `gif`, `mp4` or `html`.
    pub fn parse(name: &str) -> Option<Format> {
        Format::ALL
            .into_iter()
            .find(|format| format.extension() == name)
    }

    /// The file extension, which is also the name.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Png => "png",
            Format::Svg => "svg",
            Format::Cast => "cast",
            Format::Gif => "gif",
            Format::Mp4 => "mp4",
            Format::Html => "html",
        }
    }

    /// Whether one file holds the whole tape (so `Output` may name it), as
    /// opposed to one file per screenshot.
    pub fn per_tape(self) -> bool {
        !matches!(self, Format::Png | Format::Svg)
    }
}

/// One `Output` word: a format, and the file to write it to when the word
/// was a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub format: Format,
    /// Relative to the tape's output directory.
    pub path: Option<String>,
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
    Mouse(Mouse),
}

/// The shell a tape runs in (`Set Shell`). Each starts without the user's
/// profile or rc files and with the same `❯` prompt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shell {
    #[default]
    Bash,
    Zsh,
    Fish,
    Sh,
}

impl Shell {
    pub fn parse(name: &str) -> Option<Shell> {
        Some(match name {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "sh" => Shell::Sh,
            _ => return None,
        })
    }

    /// The program's name, looked up on `PATH`.
    pub fn name(self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Sh => "sh",
        }
    }
}

/// A parsed tape: its settings, and its steps with their line numbers.
#[derive(Debug, Clone)]
pub struct Tape {
    pub columns: u16,
    pub rows: u16,
    pub title: Option<String>,
    pub shell: Shell,
    pub env: Vec<(String, String)>,
    /// Rewrites applied to text grids (what `--check` compares), for output
    /// that differs on every run: temporary paths, timings. Images and casts
    /// keep what was recorded.
    pub masks: Vec<(Regex, String)>,
    /// `Set WindowFrame`: `None` when the tape does not say.
    pub window_frame: Option<bool>,
    /// `Set Caption`.
    pub caption: Option<String>,
    /// `Set KeyOverlay`: `None` when the tape does not say.
    pub key_overlay: Option<bool>,
    /// `Output`, in order. Empty: every format.
    pub outputs: Vec<Output>,
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

/// Apply `Mask` rewrites in order.
pub fn apply_masks(masks: &[(Regex, String)], text: &str) -> String {
    let mut text = text.to_string();
    for (regex, replacement) in masks {
        text = regex.replace_all(&text, replacement.as_str()).into_owned();
    }
    text
}

/// The longest `Sleep`, `Wait` or `Timeout` a tape may ask for.
pub const MAX_DURATION: Duration = Duration::from_secs(3600);
/// The smallest terminal a tape may ask for: the emulator needs room for a
/// wide character.
pub const MIN_COLUMNS: u16 = 2;
pub const MIN_ROWS: u16 = 2;
/// The largest terminal a tape may ask for (`Set Size`, `Resize`): its
/// screenshots and video frames are drawn in memory, and a 500x200 PNG
/// screenshot is already about 8500x7400 pixels.
pub const MAX_COLUMNS: u16 = 500;
pub const MAX_ROWS: u16 = 200;

/// `500ms` or `2s` (fractions allowed), at most [`MAX_DURATION`].
pub fn parse_duration(text: &str) -> Option<Duration> {
    let (number, scale) = if let Some(ms) = text.strip_suffix("ms") {
        (ms, 0.001)
    } else {
        (text.strip_suffix('s')?, 1.0)
    };
    let value: f64 = number.parse().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    Duration::try_from_secs_f64(value * scale)
        .ok()
        .filter(|duration| *duration <= MAX_DURATION)
}

/// `100x28`: from [`MIN_COLUMNS`]x[`MIN_ROWS`] to [`MAX_COLUMNS`]x[`MAX_ROWS`].
pub fn parse_size(text: &str) -> Option<(u16, u16)> {
    let (columns, rows) = text.split_once('x')?;
    let (columns, rows) = (columns.parse().ok()?, rows.parse().ok()?);
    size_allowed(columns, rows).then_some((columns, rows))
}

/// Whether a terminal of `columns` x `rows` is within the limits.
pub fn size_allowed(columns: u16, rows: u16) -> bool {
    (MIN_COLUMNS..=MAX_COLUMNS).contains(&columns) && (MIN_ROWS..=MAX_ROWS).contains(&rows)
}

/// Whether `name` may name a screenshot: letters, digits, `-` and `_`, so
/// its files stay in the output directory.
pub fn screenshot_name_allowed(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Whether `Write` may create `path`: relative, and inside the workspace
/// (no `..`, no root or drive prefix).
pub fn write_path_allowed(path: &str) -> bool {
    use std::path::Component;
    !path.is_empty()
        && std::path::Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
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

/// One `Output` word: a format name, or a relative path ending in a
/// per-tape format's extension.
fn parse_output(word: &str) -> Result<Output, String> {
    if let Some(format) = Format::parse(word) {
        return Ok(Output { format, path: None });
    }
    let names = "png, svg, cast, gif, mp4 or html";
    let Some(format) = std::path::Path::new(word)
        .extension()
        .and_then(|ext| ext.to_str())
        .and_then(Format::parse)
    else {
        return Err(format!(
            "Output {word:?} is neither a format ({names}) nor a file ending in one"
        ));
    };
    if !format.per_tape() {
        return Err(format!(
            "Output {word:?}: {} files are named by Screenshot; write `Output {}`",
            format.extension(),
            format.extension()
        ));
    }
    if !write_path_allowed(word) {
        return Err(format!(
            "Output {word:?} must be relative and stay in the output directory (no ..)"
        ));
    }
    Ok(Output {
        format,
        path: Some(word.to_string()),
    })
}

/// Parse a tape.
pub fn parse(source: &str) -> Result<Tape, TapeError> {
    let mut tape = Tape {
        columns: 100,
        rows: 28,
        title: None,
        shell: Shell::default(),
        env: Vec::new(),
        masks: Vec::new(),
        window_frame: None,
        caption: None,
        key_overlay: None,
        outputs: Vec::new(),
        steps: Vec::new(),
    };
    let wait_regex = Regex::new(r"^Wait\s+/(.*)/(?:\s+(\S+))?$").expect("valid regex");
    let mask_regex = Regex::new(r"^Mask\s+/(.*)/\s+(.+)$").expect("valid regex");
    for (index, line) in source.lines().enumerate() {
        let number = index + 1;
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let error = |message: String| TapeError::new(number, message);
        if let Ok(Some(captures)) = mask_regex.captures(trimmed) {
            let regex = Regex::new(&captures[1])
                .map_err(|e| error(format!("bad regex /{}/: {e}", &captures[1])))?;
            let replacement = match words(&captures[2]).map_err(error)?.as_slice() {
                [one] => one.clone(),
                _ => return Err(error("Mask needs /regex/ and one replacement".into())),
            };
            tape.masks.push((regex, replacement));
            continue;
        }
        // A /regex/ is kept whole, backslashes included.
        if let Ok(Some(captures)) = wait_regex.captures(trimmed) {
            let regex = Regex::new(&captures[1])
                .map_err(|e| error(format!("bad regex /{}/: {e}", &captures[1])))?;
            let timeout = match captures.get(2) {
                Some(value) => Some(parse_duration(value.as_str()).ok_or_else(|| {
                    error(format!(
                        "bad duration {:?} (use 500ms or 2s, at most {}s)",
                        value.as_str(),
                        MAX_DURATION.as_secs()
                    ))
                })?),
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
            parse_duration(text).ok_or_else(|| {
                error(format!(
                    "bad duration {text:?} (use 500ms or 2s, at most {}s)",
                    MAX_DURATION.as_secs()
                ))
            })
        };
        let switch = |text: &str| match text {
            "on" | "true" => Ok(true),
            "off" | "false" => Ok(false),
            _ => Err(error(format!("{text:?} must be on or off"))),
        };
        let size = |text: &str| {
            parse_size(text).ok_or_else(|| {
                error(format!(
                    "bad size {text:?} (use 100x28: {MIN_COLUMNS} to {MAX_COLUMNS} columns, \
                     {MIN_ROWS} to {MAX_ROWS} rows)"
                ))
            })
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
                    "Caption" => {
                        tape.caption = Some(value.to_string()).filter(|c| !c.is_empty());
                        continue;
                    }
                    "WindowFrame" => {
                        tape.window_frame = Some(switch(value)?);
                        continue;
                    }
                    "KeyOverlay" => {
                        tape.key_overlay = Some(switch(value)?);
                        continue;
                    }
                    "Shell" => {
                        tape.shell = Shell::parse(value).ok_or_else(|| {
                            error(format!(
                                "unknown shell {value:?} (use bash, zsh, fish or sh)"
                            ))
                        })?;
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
                if !screenshot_name_allowed(name) {
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
            "Write" => {
                let path = arg(0)?;
                if !write_path_allowed(path) {
                    return Err(error(format!(
                        "Write path {path:?} must be relative and stay in the workspace (no ..)"
                    )));
                }
                Step::Write {
                    path: path.to_string(),
                    content: unescape(arg(1)?),
                }
            }
            "Exec" => Step::Exec(arg(0)?.to_string()),
            "Output" => {
                if args.is_empty() {
                    return Err(error("Output needs a format or a file".into()));
                }
                for word in args {
                    tape.outputs.push(parse_output(word).map_err(error)?);
                }
                continue;
            }
            "Click" | "RightClick" | "MiddleClick" | "DoubleClick" | "Drag" | "ScrollUp"
            | "ScrollDown" | "MouseMove" => {
                let cell = |at: usize| -> Result<u16, TapeError> {
                    let text = arg(at)?;
                    // SGR counts from 1, so the largest cell is one less
                    // than the largest number it carries.
                    text.parse()
                        .ok()
                        .filter(|&n| n < u16::MAX)
                        .ok_or_else(|| error(format!("bad cell {text:?} (a column or row from 0)")))
                };
                let at = (cell(0)?, cell(1)?);
                let (action, extra) = match command.as_str() {
                    "Click" => (MouseAction::Click(Button::Left), 2),
                    "RightClick" => (MouseAction::Click(Button::Right), 2),
                    "MiddleClick" => (MouseAction::Click(Button::Middle), 2),
                    "DoubleClick" => (MouseAction::DoubleClick, 2),
                    "Drag" => (
                        MouseAction::Drag {
                            to: (cell(2)?, cell(3)?),
                        },
                        4,
                    ),
                    "ScrollUp" => (MouseAction::Scroll { up: true }, 2),
                    "ScrollDown" => (MouseAction::Scroll { up: false }, 2),
                    _ => (MouseAction::Move, 2),
                };
                let count = match args.get(extra) {
                    Some(count) => count
                        .parse()
                        .ok()
                        .filter(|&n| n > 0)
                        .ok_or_else(|| error(format!("bad count {count:?}")))?,
                    None => 1,
                };
                Step::Mouse(Mouse { action, at, count })
            }
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
    fn parses_the_mouse_and_more_keys() {
        let tape = parse(
            "Click 3 1\nRightClick 0 0\nDoubleClick 5 2\nDrag 1 1 3 2\nScrollDown 4 4 3\n\
             MouseMove 9 9\nF1\nF13\nF24\nShift+Tab\nAlt+x\nDelete\n",
        )
        .unwrap();
        let mouse = |i: usize| match tape.steps[i].1 {
            Step::Mouse(mouse) => mouse,
            ref other => panic!("{other:?}"),
        };
        // Cells count from 0; SGR reports from 1.
        assert_eq!(mouse(0).reports(), ["\x1b[<0;4;2M", "\x1b[<0;4;2m"]);
        assert_eq!(mouse(1).reports()[0], "\x1b[<2;1;1M");
        assert_eq!(mouse(2).reports().len(), 4);
        // A drag moves a cell at a time, with the button held.
        assert_eq!(
            mouse(3).reports(),
            [
                "\x1b[<0;2;2M",
                "\x1b[<32;3;3M",
                "\x1b[<32;4;3M",
                "\x1b[<0;4;3m"
            ]
        );
        assert_eq!(mouse(4).count, 3);
        assert_eq!(mouse(4).reports(), ["\x1b[<65;5;5M"]);
        assert_eq!(mouse(5).reports(), ["\x1b[<35;10;10M"]);
        let key = |i: usize| match tape.steps[i].1 {
            Step::Key { key, .. } => key.bytes(),
            ref other => panic!("{other:?}"),
        };
        assert_eq!(key(6), "\x1bOP");
        assert_eq!(key(7), "\x1b[25~");
        assert_eq!(key(8), "\x1b[57387u");
        assert_eq!(key(9), "\x1b[Z");
        assert_eq!(key(10), "\x1bx");
        assert_eq!(key(11), "\x1b[3~");
        assert!(parse("F25\n").is_err());
        assert!(parse("Click 65535 0\n").is_err());
        assert_eq!(
            Mouse {
                action: MouseAction::Click(Button::Left),
                at: (65_534, 0),
                count: 1
            }
            .reports()[0],
            "\x1b[<0;65535;1M"
        );
        assert!(parse("Click 3\n").is_err());
        assert!(parse("Drag 1 1 2\n").is_err());
    }

    #[test]
    fn modifiers_go_on_navigation_and_function_keys() {
        let key = |name: &str| match parse(&format!("{name}\n")).unwrap().steps[0].1 {
            Step::Key { key, .. } => key,
            ref other => panic!("{other:?}"),
        };
        assert_eq!(key("Alt+Left").bytes(), "\x1b[1;3D");
        assert_eq!(key("Shift+Left").bytes(), "\x1b[1;2D");
        assert_eq!(key("Ctrl+PageDown").bytes(), "\x1b[6;5~");
        assert_eq!(key("Shift+Ctrl+PageDown"), key("Ctrl+Shift+PageDown"));
        assert_eq!(key("Ctrl+Shift+PageDown").bytes(), "\x1b[6;6~");
        assert_eq!(key("Shift+F1").bytes(), "\x1b[1;2P");
        assert_eq!(key("Ctrl+F5").bytes(), "\x1b[15;5~");
        assert_eq!(key("Alt+F24").bytes(), "\x1b[57387;3u");
        assert_eq!(key("Ctrl+Alt+Home").label(), "Ctrl+Alt+Home");
        // The older forms still parse as they did.
        assert_eq!(key("Shift+Tab"), Key::ShiftTab);
        assert_eq!(key("Alt+x"), Key::Alt('x'));
        assert_eq!(key("Ctrl+c"), Key::Ctrl('C'));
        for bad in [
            "Alt+Alt+Left",
            "Ctrl+Tab",
            "Shift+x",
            "Ctrl+Alt+x",
            "Alt+",
            "Meta+Left",
        ] {
            assert!(parse(&format!("{bad}\n")).is_err(), "{bad}");
        }
    }

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
    fn masks_rewrite_text() {
        let tape = parse("Mask /\\/tmp\\/\\S+/ \"<tmp>\"\nMask /\\d+ms/ \"Nms\"\n").unwrap();
        assert_eq!(tape.masks.len(), 2);
        assert_eq!(
            apply_masks(&tape.masks, "at /tmp/x1/a.toml in 12ms"),
            "at <tmp> in Nms"
        );
        assert!(parse("Mask /x/").is_err());
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
    fn huge_durations_are_parse_errors() {
        // `Duration::from_secs_f64` panicked on these.
        for step in [
            "Sleep 1e300s",
            "Set Timeout 1e20s",
            "Wait \"x\" 1e300s",
            "Sleep 3601s",
        ] {
            let error = parse(step).unwrap_err();
            assert!(error.message.contains("bad duration"), "{step}: {error}");
        }
        assert!(parse("Wait /x/ 1e300s").is_err());
        assert_eq!(parse_duration("3600s"), Some(MAX_DURATION));
        assert_eq!(parse_duration("1.5ms"), Some(Duration::from_micros(1500)));
    }

    #[test]
    fn sizes_are_bounded() {
        for size in [
            "1x5",
            "5x1",
            "0x0",
            "501x10",
            "10x201",
            "65535x65535",
            "7000x2",
        ] {
            assert!(
                parse(&format!("Set Size {size}")).is_err(),
                "Set Size {size}"
            );
            assert!(parse(&format!("Resize {size}")).is_err(), "Resize {size}");
        }
        let tape = parse("Set Size 500x200\nResize 2x2\n").unwrap();
        assert_eq!((tape.columns, tape.rows), (500, 200));
    }

    #[test]
    fn write_stays_in_the_workspace() {
        for path in ["/tmp/x", "../x", "a/../../x", "a/..", ""] {
            let error = parse(&format!("Write '{path}' hi")).unwrap_err();
            assert!(error.message.contains("Write path"), "{path}: {error}");
        }
        assert!(parse("Write a/b.txt hi\nWrite ./c hi\n").is_ok());
    }

    #[test]
    fn keys_send_their_bytes() {
        assert_eq!(Key::Ctrl('C').bytes(), "\x03");
        assert_eq!(Key::Down.bytes(), "\x1b[B");
        assert_eq!(Key::Ctrl('C').label(), "Ctrl+C");
    }

    #[test]
    fn presentation_and_outputs() {
        let tape = parse(
            "Set WindowFrame off\nSet Caption \"Save, and it redraws\"\nSet KeyOverlay off\n\
             Output gif png\nOutput media/demo.html\n",
        )
        .unwrap();
        assert_eq!(tape.window_frame, Some(false));
        assert_eq!(tape.key_overlay, Some(false));
        assert_eq!(tape.caption.as_deref(), Some("Save, and it redraws"));
        assert_eq!(
            tape.outputs,
            [
                Output {
                    format: Format::Gif,
                    path: None
                },
                Output {
                    format: Format::Png,
                    path: None
                },
                Output {
                    format: Format::Html,
                    path: Some("media/demo.html".into())
                },
            ]
        );
        let defaults = parse("Type x\n").unwrap();
        assert_eq!((defaults.window_frame, defaults.key_overlay), (None, None));
        for (bad, message) in [
            ("Set WindowFrame maybe", "on or off"),
            ("Output webm", "neither a format"),
            ("Output shot.png", "named by Screenshot"),
            ("Output ../x.gif", "stay in the output directory"),
            ("Output", "needs a format"),
        ] {
            let error = parse(bad).unwrap_err();
            assert!(error.message.contains(message), "{bad}: {error}");
        }
    }

    #[test]
    fn shells() {
        assert_eq!(parse("Set Size 10x2\n").unwrap().shell, Shell::Bash);
        let tape = parse("Set Shell fish\n").unwrap();
        assert_eq!(tape.shell, Shell::Fish);
        let error = parse("\nSet Shell pwsh\n").unwrap_err().to_string();
        assert!(
            error.contains("line 2") && error.contains("unknown shell"),
            "{error}"
        );
    }
}

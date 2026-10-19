//! Keys as a terminal sends them: the bytes a legacy terminal sends, or the
//! kitty keyboard protocol's escapes when the program has turned the
//! protocol on (<https://sw.kovidgoyal.net/kitty/keyboard-protocol/>).
//!
//! A tape can hold a key down (`KeyDown`, `Hold`) and let it go (`KeyUp`).
//! What the program then reads depends on the flags it pushed, which the
//! emulator follows ([`Terminal::keyboard_flags`](crate::terminal::Terminal::keyboard_flags)):
//!
//! - no flags (the protocol off, or never reported to the program): each
//!   press and each autorepeat sends the key's legacy bytes, and a release
//!   sends nothing, as on a terminal without the protocol;
//! - [`DISAMBIGUATE`]: Escape, and keys with Ctrl or Alt, and the function
//!   keys, as `CSI … u` escapes;
//! - [`REPORT_EVENTS`] as well: repeats and releases of those keys are
//!   reported (`CSI code;mods:2 u`, `:3`); keys that type text (letters,
//!   Space) and Enter, Tab and Backspace still send their text on press
//!   and repeat, and no release, as kitty does;
//! - [`ALL_AS_ESCAPES`]: every key is an escape, text keys included, with
//!   its repeats and releases when [`REPORT_EVENTS`] is set too.

use crate::tape::Key;

/// Disambiguate escape codes: the protocol's first flag.
pub const DISAMBIGUATE: u8 = 1;
/// Report repeats and releases.
pub const REPORT_EVENTS: u8 = 2;
/// Report every key as an escape code, text keys included.
pub const ALL_AS_ESCAPES: u8 = 8;

/// What happens to a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Press,
    /// An autorepeat while it is held.
    Repeat,
    Release,
}

/// A key a tape presses, holds or lets go: a named key, with or without
/// modifiers, or a character (`KeyDown w`, `Hold W 1s` with Shift).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    Key(Key),
    Char(char),
}

/// How the protocol writes a key: `CSI code u`, `CSI 1 X` (the arrows,
/// Home, End, F1, F2, F4) or `CSI n ~`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Form {
    U(u32),
    Letter(char),
    Tilde(u32),
}

/// Whether a key types text, is Enter, Tab or Backspace (which keep their
/// legacy bytes unless every key is an escape), or neither.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Text,
    Legacy,
    Functional,
}

/// The `CSI n ~` numbers of F5 to F12.
const TILDE_F: [u32; 8] = [15, 17, 18, 19, 20, 21, 23, 24];

impl Press {
    /// A key name a tape uses (`Up`, `Ctrl+C`, `Alt+Left`) or one
    /// character (`w`).
    pub fn parse(name: &str) -> Option<Press> {
        if let Some(key) = Key::parse(name) {
            return Some(Press::Key(key));
        }
        let mut chars = name.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_control() && !c.is_whitespace() => Some(Press::Char(c)),
            _ => None,
        }
    }

    /// How the key overlay shows it.
    pub fn label(self) -> String {
        match self {
            Press::Key(key) => key.label(),
            Press::Char(c) => c.to_string(),
        }
    }

    /// What a legacy terminal sends for it.
    pub fn legacy(self) -> String {
        match self {
            Press::Key(key) => key.bytes(),
            Press::Char(c) => c.to_string(),
        }
    }

    /// The protocol's form, xterm modifier number (1 plus 1 Shift, 2 Alt, 4
    /// Ctrl) and kind of the key.
    fn kitty(self) -> (Form, u8, Kind) {
        let letter = |c: char| {
            if c.is_ascii_uppercase() {
                (c.to_ascii_lowercase() as u32, 1)
            } else {
                (c as u32, 0)
            }
        };
        let key = match self {
            Press::Char(c) => {
                let (code, shift) = letter(c);
                return (Form::U(code), 1 + shift, Kind::Text);
            }
            Press::Key(key) => key,
        };
        let plain = |key: Key| -> (Form, Kind) {
            match key {
                Key::Enter => (Form::U(13), Kind::Legacy),
                Key::Tab => (Form::U(9), Kind::Legacy),
                Key::Backspace => (Form::U(127), Kind::Legacy),
                Key::Space => (Form::U(32), Kind::Text),
                Key::Escape => (Form::U(27), Kind::Functional),
                Key::Up => (Form::Letter('A'), Kind::Functional),
                Key::Down => (Form::Letter('B'), Kind::Functional),
                Key::Right => (Form::Letter('C'), Kind::Functional),
                Key::Left => (Form::Letter('D'), Kind::Functional),
                Key::Home => (Form::Letter('H'), Kind::Functional),
                Key::End => (Form::Letter('F'), Kind::Functional),
                Key::PageUp => (Form::Tilde(5), Kind::Functional),
                Key::PageDown => (Form::Tilde(6), Kind::Functional),
                Key::Delete => (Form::Tilde(3), Kind::Functional),
                Key::Insert => (Form::Tilde(2), Kind::Functional),
                Key::ShiftTab => (Form::U(9), Kind::Functional),
                Key::F(1) => (Form::Letter('P'), Kind::Functional),
                Key::F(2) => (Form::Letter('Q'), Kind::Functional),
                Key::F(3) => (Form::Tilde(13), Kind::Functional),
                Key::F(4) => (Form::Letter('S'), Kind::Functional),
                Key::F(n @ 5..=12) => (Form::Tilde(TILDE_F[usize::from(n - 5)]), Kind::Functional),
                Key::F(n) => (Form::U(57_376 + u32::from(n) - 13), Kind::Functional),
                Key::Ctrl(c) => (Form::U(c.to_ascii_lowercase() as u32), Kind::Functional),
                Key::Alt(c) => (Form::U(letter(c).0), Kind::Functional),
                Key::Modified { key, .. } => plain_form(key),
            }
        };
        let (form, kind) = plain(key);
        let mods = match key {
            Key::ShiftTab => 2,
            Key::Ctrl(_) => 5,
            Key::Alt(c) => 3 + letter(c).1,
            Key::Modified { mods, .. } => mods,
            _ => 1,
        };
        (form, mods, kind)
    }

    /// The bytes the terminal sends for `event` while the program has
    /// `flags` in force; `None` when it sends nothing (a release the
    /// program did not ask to hear).
    pub fn bytes(self, event: Event, flags: u8) -> Option<String> {
        let (form, mods, kind) = self.kitty();
        let escape = flags & ALL_AS_ESCAPES != 0
            || (kind == Kind::Functional && flags & (DISAMBIGUATE | REPORT_EVENTS) != 0);
        let reported = escape && flags & REPORT_EVENTS != 0;
        match event {
            Event::Press if escape => Some(encode(form, mods, None)),
            Event::Repeat if reported => Some(encode(form, mods, Some(2))),
            Event::Release if reported => Some(encode(form, mods, Some(3))),
            Event::Press | Event::Repeat => Some(self.legacy()),
            Event::Release => None,
        }
    }
}

/// [`Form`] of a key that takes modifiers.
fn plain_form(key: crate::tape::Modifiable) -> (Form, Kind) {
    let (form, _, kind) = Press::Key(key.plain()).kitty();
    (form, kind)
}

/// The protocol's escape for a key, with an event type (2 repeat, 3
/// release) when it is not a press.
fn encode(form: Form, mods: u8, event: Option<u8>) -> String {
    let tail = match (mods, event) {
        (1, None) => String::new(),
        (mods, None) => format!(";{mods}"),
        (mods, Some(event)) => format!(";{mods}:{event}"),
    };
    match form {
        Form::U(code) => format!("\x1b[{code}{tail}u"),
        Form::Letter(last) if tail.is_empty() => format!("\x1b[{last}"),
        Form::Letter(last) => format!("\x1b[1{tail}{last}"),
        Form::Tilde(number) => format!("\x1b[{number}{tail}~"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(name: &str) -> Press {
        Press::parse(name).unwrap_or_else(|| panic!("{name}"))
    }

    #[test]
    fn without_the_protocol_keys_send_their_legacy_bytes() {
        for name in ["w", "Up", "Escape", "Enter", "Ctrl+C", "F5", "Alt+x"] {
            let key = press(name);
            assert_eq!(key.bytes(Event::Press, 0), Some(key.legacy()), "{name}");
            assert_eq!(key.bytes(Event::Repeat, 0), Some(key.legacy()), "{name}");
            assert_eq!(key.bytes(Event::Release, 0), None, "{name}");
        }
        assert_eq!(press("w").legacy(), "w");
        assert_eq!(press("Up").legacy(), "\x1b[A");
    }

    #[test]
    fn disambiguated_keys_and_their_releases() {
        let flags = DISAMBIGUATE | REPORT_EVENTS;
        assert_eq!(
            press("Escape").bytes(Event::Press, flags).unwrap(),
            "\x1b[27u"
        );
        assert_eq!(
            press("Escape").bytes(Event::Release, flags).unwrap(),
            "\x1b[27;1:3u"
        );
        assert_eq!(press("Up").bytes(Event::Press, flags).unwrap(), "\x1b[A");
        assert_eq!(
            press("Up").bytes(Event::Repeat, flags).unwrap(),
            "\x1b[1;1:2A"
        );
        assert_eq!(
            press("Up").bytes(Event::Release, flags).unwrap(),
            "\x1b[1;1:3A"
        );
        assert_eq!(
            press("Ctrl+C").bytes(Event::Press, flags).unwrap(),
            "\x1b[99;5u"
        );
        assert_eq!(
            press("Alt+Left").bytes(Event::Release, flags).unwrap(),
            "\x1b[1;3:3D"
        );
        assert_eq!(
            press("PageDown").bytes(Event::Press, flags).unwrap(),
            "\x1b[6~"
        );
        assert_eq!(
            press("F3").bytes(Event::Release, flags).unwrap(),
            "\x1b[13;1:3~"
        );
        assert_eq!(
            press("F13").bytes(Event::Press, flags).unwrap(),
            "\x1b[57376u"
        );
        // Text keys, and Enter, keep their bytes and report no release.
        assert_eq!(press("w").bytes(Event::Press, flags).unwrap(), "w");
        assert_eq!(press("w").bytes(Event::Repeat, flags).unwrap(), "w");
        assert_eq!(press("w").bytes(Event::Release, flags), None);
        assert_eq!(press("Enter").bytes(Event::Release, flags), None);
        // Without the second flag, no releases at all.
        assert_eq!(press("Up").bytes(Event::Release, DISAMBIGUATE), None);
    }

    #[test]
    fn every_key_as_an_escape() {
        let flags = DISAMBIGUATE | REPORT_EVENTS | ALL_AS_ESCAPES;
        assert_eq!(press("w").bytes(Event::Press, flags).unwrap(), "\x1b[119u");
        assert_eq!(
            press("W").bytes(Event::Press, flags).unwrap(),
            "\x1b[119;2u"
        );
        assert_eq!(
            press("w").bytes(Event::Release, flags).unwrap(),
            "\x1b[119;1:3u"
        );
        assert_eq!(
            press("Space").bytes(Event::Repeat, flags).unwrap(),
            "\x1b[32;1:2u"
        );
        assert_eq!(
            press("Enter").bytes(Event::Release, flags).unwrap(),
            "\x1b[13;1:3u"
        );
        assert!(Press::parse("ww").is_none() && Press::parse(" ").is_none());
    }
}

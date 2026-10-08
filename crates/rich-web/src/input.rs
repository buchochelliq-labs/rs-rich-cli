//! The browser's input, decoded into rs-rich-interact [`Event`]s.
//!
//! xterm.js turns keys, the mouse and pastes into the bytes an xterm sends
//! (`onData`): control characters, `ESC [` and `ESC O` sequences, SGR mouse
//! reports and bracketed paste. rs-rich-interact reads a real terminal
//! through crossterm, whose parser is private, so this is a small decoder of
//! its own. It gives the same events crossterm gives for the same bytes, so
//! an app's key bindings work alike in a terminal and in a browser: Ctrl+I
//! is Tab, `0x08` is Ctrl+H, `\n` is Ctrl+J (raw mode), and an upper-case
//! letter carries no Shift.
//!
//! Each WebSocket message holds whole sequences (xterm.js hands one key,
//! one mouse report or one paste to `onData` at a time), so a lone `ESC` at
//! the end of a message is the Escape key, with no timeout to wait out.

use intuituive::interact::{Button, Event, Key, KeyCode, Modifiers, Mouse, MouseKind};

const ESC: u8 = 0x1b;

/// Decode one message of input into events. Sequences this does not know
/// (focus reports, kitty keys, X10 mouse reports) are skipped.
///
/// ```
/// use rich_web::input::decode;
/// use rich_web::intuituive::interact::{Event, Key};
///
/// assert_eq!(decode("a\x1b[A"), vec![
///     Event::Key(Key::char('a')),
///     Event::Key(Key::parse("up").unwrap()),
/// ]);
/// ```
pub fn decode(input: &str) -> Vec<Event> {
    let mut events = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let (event, used) = next(rest);
        events.extend(event);
        // Never stall: every step uses at least one character.
        let used = used.max(rest.chars().next().map_or(1, char::len_utf8));
        rest = rest.get(used..).unwrap_or("");
    }
    events
}

/// The first event in `s` and how many bytes it used.
fn next(s: &str) -> (Option<Event>, usize) {
    if s.as_bytes()[0] == ESC {
        return escape(s);
    }
    let c = s.chars().next().expect("not empty");
    (Some(Event::Key(plain(c))), c.len_utf8())
}

/// A character outside an escape sequence: a control key or a character.
fn plain(c: char) -> Key {
    match c {
        '\r' => Key::new(KeyCode::Enter),
        '\t' => Key::new(KeyCode::Tab),
        '\x7f' => Key::new(KeyCode::Backspace),
        '\0' => Key::ctrl(' '),
        // Ctrl+A to Ctrl+Z, `\n` (Ctrl+J) and 0x08 (Ctrl+H) among them.
        '\x01'..='\x1a' => Key::ctrl((b'a' + (c as u8 - 1)) as char),
        // Ctrl+\ ] ^ _ arrive as these, and are Ctrl+4 to Ctrl+7.
        '\x1c'..='\x1f' => Key::ctrl((b'4' + (c as u8 - 0x1c)) as char),
        c => Key::char(c),
    }
}

/// `s` starts with ESC.
fn escape(s: &str) -> (Option<Event>, usize) {
    let bytes = s.as_bytes();
    match bytes.get(1) {
        None => (Some(Event::Key(Key::new(KeyCode::Escape))), 1),
        Some(b'[') => csi(s),
        Some(b'O') if bytes.len() > 2 => (ss3(bytes[2]).map(Event::Key), 3),
        // Escape pressed twice: one Escape now, the next one after it.
        Some(&ESC) => (Some(Event::Key(Key::new(KeyCode::Escape))), 1),
        // Alt (or Option, as Meta) and a key.
        Some(_) => {
            let (event, used) = next(&s[1..]);
            let event = event.map(|event| match event {
                Event::Key(mut key) => {
                    key.modifiers.alt = true;
                    Event::Key(key)
                }
                other => other,
            });
            (event, used + 1)
        }
    }
}

/// `ESC O` and one byte: the application-mode keys.
fn ss3(byte: u8) -> Option<Key> {
    let code = match byte {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        _ => return None,
    };
    Some(Key::new(code))
}

/// `s` starts with `ESC [`.
fn csi(s: &str) -> (Option<Event>, usize) {
    let bytes = s.as_bytes();
    // Parameters and intermediates, then a final byte in 0x40..=0x7e.
    let Some(end) = bytes[2..]
        .iter()
        .position(|b| (0x40..=0x7e).contains(b))
        .map(|i| i + 2)
    else {
        // Cut short: drop the rest of the message.
        return (None, bytes.len());
    };
    let params = &s[2..end];
    let used = end + 1;
    let last = bytes[end];
    if params == "200" && last == b'~' {
        return paste(s, used);
    }
    if let Some(mouse) = params.strip_prefix('<') {
        if last == b'M' || last == b'm' {
            return (sgr_mouse(mouse, last == b'M').map(Event::Mouse), used);
        }
        return (None, used);
    }
    let numbers: Vec<u16> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
    let first = numbers.first().copied().unwrap_or(0);
    let modifiers = numbers
        .get(1)
        .map_or(Modifiers::NONE, |&m| modifier_bits(m));
    let code = match last {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        b'Z' => {
            // Shift+Tab, as BackTab with no Shift (as crossterm gives it).
            let mut key = Key::new(KeyCode::BackTab);
            key.modifiers = modifiers;
            key.modifiers.shift = false;
            return (Some(Event::Key(key)), used);
        }
        b'~' => match tilde(first) {
            Some(code) => code,
            None => return (None, used),
        },
        // Focus in and out, and anything else: nothing to deliver.
        _ => return (None, used),
    };
    (Some(Event::Key(Key { code, modifiers })), used)
}

/// The keys sent as `ESC [ n ~`.
fn tilde(n: u16) -> Option<KeyCode> {
    Some(match n {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        11..=15 => KeyCode::F((n - 10) as u8),
        17..=21 => KeyCode::F((n - 11) as u8),
        23..=26 => KeyCode::F((n - 12) as u8),
        28 | 29 => KeyCode::F((n - 13) as u8),
        31..=34 => KeyCode::F((n - 14) as u8),
        _ => return None,
    })
}

/// xterm's modifier parameter: one more than a bit set of Shift (1), Alt
/// (2), Ctrl (4) and Meta (8, taken as Alt).
fn modifier_bits(parameter: u16) -> Modifiers {
    let bits = parameter.saturating_sub(1);
    Modifiers {
        shift: bits & 1 != 0,
        alt: bits & 2 != 0 || bits & 8 != 0,
        ctrl: bits & 4 != 0,
    }
}

/// `ESC [ 200 ~` text `ESC [ 201 ~`: one paste. `start` is where the text
/// starts.
fn paste(s: &str, start: usize) -> (Option<Event>, usize) {
    const END: &str = "\x1b[201~";
    let text = &s[start..];
    match text.find(END) {
        Some(at) => (
            Some(Event::Paste(text[..at].to_string())),
            start + at + END.len(),
        ),
        None => (Some(Event::Paste(text.to_string())), s.len()),
    }
}

/// An SGR mouse report, `ESC [ < button ; column ; row` and `M` (pressed,
/// dragged, moved) or `m` (released). Columns and rows are 1-based.
fn sgr_mouse(params: &str, pressed: bool) -> Option<Mouse> {
    let mut numbers = params.split(';').map(|p| p.parse::<u16>().ok());
    let code = numbers.next()??;
    let column = numbers.next()??.saturating_sub(1);
    let row = numbers.next()??.saturating_sub(1);
    let button = match code & 3 {
        0 => Some(Button::Left),
        1 => Some(Button::Middle),
        2 => Some(Button::Right),
        _ => None,
    };
    let kind = if code & 64 != 0 {
        match code & 3 {
            0 => MouseKind::ScrollUp,
            1 => MouseKind::ScrollDown,
            // Sideways scrolling: no event for it.
            _ => return None,
        }
    } else if code & 32 != 0 {
        match button {
            Some(button) => MouseKind::Drag(button),
            None => MouseKind::Moved,
        }
    } else if pressed {
        MouseKind::Down(button?)
    } else {
        MouseKind::Up(button?)
    };
    Some(Mouse {
        kind,
        column,
        row,
        modifiers: Modifiers {
            shift: code & 4 != 0,
            alt: code & 8 != 0,
            ctrl: code & 16 != 0,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Event {
        Event::Key(Key::parse(name).unwrap_or_else(|| panic!("{name} is a key name")))
    }

    /// What xterm.js sends for each key, and the key name it is.
    #[test]
    fn keys_decode_to_their_names() {
        let table: &[(&str, &str)] = &[
            ("a", "a"),
            ("A", "A"),
            ("é", "é"),
            ("+", "+"),
            (" ", "space"),
            ("\r", "enter"),
            ("\t", "tab"),
            ("\x7f", "backspace"),
            ("\x1b", "esc"),
            ("\x03", "ctrl+c"),
            ("\x01", "ctrl+a"),
            ("\x1a", "ctrl+z"),
            ("\x08", "ctrl+h"),
            ("\n", "ctrl+j"),
            ("\x00", "ctrl+space"),
            ("\x1c", "ctrl+\\"),
            ("\x1d", "ctrl+]"),
            ("\x1f", "ctrl+_"),
            ("\x1b[A", "up"),
            ("\x1b[B", "down"),
            ("\x1b[C", "right"),
            ("\x1b[D", "left"),
            ("\x1bOA", "up"),
            ("\x1bOD", "left"),
            ("\x1b[H", "home"),
            ("\x1b[F", "end"),
            ("\x1bOH", "home"),
            ("\x1bOF", "end"),
            ("\x1b[1~", "home"),
            ("\x1b[4~", "end"),
            ("\x1b[2~", "insert"),
            ("\x1b[3~", "delete"),
            ("\x1b[5~", "pageup"),
            ("\x1b[6~", "pagedown"),
            ("\x1b[Z", "shift+tab"),
            ("\x1bOP", "f1"),
            ("\x1bOQ", "f2"),
            ("\x1bOR", "f3"),
            ("\x1bOS", "f4"),
            ("\x1b[15~", "f5"),
            ("\x1b[17~", "f6"),
            ("\x1b[18~", "f7"),
            ("\x1b[19~", "f8"),
            ("\x1b[20~", "f9"),
            ("\x1b[21~", "f10"),
            ("\x1b[23~", "f11"),
            ("\x1b[24~", "f12"),
            ("\x1b[1;2A", "shift+up"),
            ("\x1b[1;5C", "ctrl+right"),
            ("\x1b[1;3D", "alt+left"),
            ("\x1b[1;6B", "ctrl+shift+down"),
            ("\x1b[1;5P", "ctrl+f1"),
            ("\x1b[3;5~", "ctrl+delete"),
            ("\x1b[5;2~", "shift+pageup"),
            ("\x1b[15;3~", "alt+f5"),
            ("\x1bx", "alt+x"),
            ("\x1bX", "alt+X"),
            ("\x1b\r", "alt+enter"),
            ("\x1b\x7f", "alt+backspace"),
            ("\x1b\x01", "ctrl+alt+a"),
        ];
        for (bytes, name) in table {
            assert_eq!(decode(bytes), vec![key(name)], "{bytes:?} is {name}");
        }
    }

    #[test]
    fn several_keys_in_one_message() {
        assert_eq!(
            decode("hi\x1b[A\x1b\x1b"),
            vec![key("h"), key("i"), key("up"), key("esc"), key("esc")]
        );
    }

    #[test]
    fn mouse_reports_decode() {
        let at = |kind, column, row| Event::Mouse(Mouse::new(kind, column, row));
        let table: &[(&str, Event)] = &[
            ("\x1b[<0;1;1M", at(MouseKind::Down(Button::Left), 0, 0)),
            ("\x1b[<0;10;5m", at(MouseKind::Up(Button::Left), 9, 4)),
            ("\x1b[<2;3;4M", at(MouseKind::Down(Button::Right), 2, 3)),
            ("\x1b[<1;3;4M", at(MouseKind::Down(Button::Middle), 2, 3)),
            ("\x1b[<32;7;2M", at(MouseKind::Drag(Button::Left), 6, 1)),
            ("\x1b[<35;7;2M", at(MouseKind::Moved, 6, 1)),
            ("\x1b[<64;5;5M", at(MouseKind::ScrollUp, 4, 4)),
            ("\x1b[<65;5;5M", at(MouseKind::ScrollDown, 4, 4)),
            // A report of 0 does not underflow.
            ("\x1b[<0;0;0M", at(MouseKind::Down(Button::Left), 0, 0)),
        ];
        for (bytes, event) in table {
            assert_eq!(decode(bytes), vec![event.clone()], "{bytes:?}");
        }
        let held = decode("\x1b[<20;2;2M");
        let Event::Mouse(mouse) = &held[0] else {
            panic!("a mouse event")
        };
        assert!(mouse.modifiers.ctrl && mouse.modifiers.shift && !mouse.modifiers.alt);
        // Sideways scrolling has no event.
        assert_eq!(decode("\x1b[<66;5;5M"), vec![]);
    }

    #[test]
    fn pastes_arrive_whole() {
        assert_eq!(
            decode("\x1b[200~hello\r\nworld\x1b[201~x"),
            vec![Event::Paste("hello\r\nworld".into()), key("x")]
        );
        // An unterminated paste takes the rest of the message.
        assert_eq!(decode("\x1b[200~tail"), vec![Event::Paste("tail".into())]);
    }

    #[test]
    fn unknown_and_broken_sequences_are_skipped() {
        // Focus in and out.
        assert_eq!(decode("\x1b[I\x1b[O"), vec![]);
        // A kitty key, an unknown tilde key, a cut-short sequence.
        assert_eq!(decode("\x1b[97;5u"), vec![]);
        assert_eq!(decode("\x1b[99~a"), vec![key("a")]);
        assert_eq!(decode("\x1b[1;5"), vec![]);
        assert_eq!(decode("\x1b[<0;1M"), vec![]);
        // `ESC O` at the very end is Alt+O.
        assert_eq!(decode("\x1bO"), vec![key("alt+O")]);
        assert_eq!(decode(""), vec![]);
    }
}

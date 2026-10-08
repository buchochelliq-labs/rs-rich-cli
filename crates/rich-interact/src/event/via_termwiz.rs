//! termwiz's events (#678), in the crate's own types.

use ::termwiz::input::{
    InputEvent, KeyCode as K, KeyEvent, Modifiers as M, MouseButtons, MouseEvent,
};

use super::{char_key, Button, Event, HeldButton, Key, KeyCode, Modifiers, Mouse, MouseKind};

/// Translate a termwiz event, for a loop of your own that reads the
/// terminal with termwiz, in its legacy mode: a key may be another the
/// terminal sends alike (see [`Key::matches`]). With the kitty keyboard
/// protocol pushed (`CSI > 1 u`), use [`from_termwiz_kitty`].
///
/// termwiz reports the mouse buttons down rather than what changed, so
/// `held` keeps the last report's, which tells a press from a drag and a
/// release from a move. termwiz reports no key releases; wakes and pixel
/// mouse reports translate to `None`.
pub fn from_termwiz(event: InputEvent, held: &mut HeldButton) -> Option<Event> {
    translate(event, held, false)
}

/// [`from_termwiz`] for a terminal with the kitty keyboard protocol's
/// first flag pushed (`CSI > 1 u`, disambiguate): every key is
/// [`exact`](Key::exact). termwiz reads that flag's keys, not the release
/// reports of the second, so push only the first.
pub fn from_termwiz_kitty(event: InputEvent, held: &mut HeldButton) -> Option<Event> {
    translate(event, held, true)
}

fn translate(event: InputEvent, held: &mut HeldButton, exact: bool) -> Option<Event> {
    Some(match event {
        InputEvent::Key(KeyEvent { key, modifiers }) => {
            let mut read = self::key(key, modifiers)?;
            read.exact = exact;
            Event::Key(read)
        }
        InputEvent::Mouse(MouseEvent {
            x,
            y,
            mouse_buttons,
            modifiers,
        }) => {
            let kind = if mouse_buttons.contains(MouseButtons::VERT_WHEEL) {
                if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                    MouseKind::ScrollUp
                } else {
                    MouseKind::ScrollDown
                }
            } else if mouse_buttons.contains(MouseButtons::HORZ_WHEEL) {
                return None;
            } else {
                let down = if mouse_buttons.contains(MouseButtons::LEFT) {
                    Some(Button::Left)
                } else if mouse_buttons.contains(MouseButtons::RIGHT) {
                    Some(Button::Right)
                } else if mouse_buttons.contains(MouseButtons::MIDDLE) {
                    Some(Button::Middle)
                } else {
                    None
                };
                let kind = match (held.0, down) {
                    (Some(was), Some(now)) if was == now => MouseKind::Drag(now),
                    (_, Some(now)) => MouseKind::Down(now),
                    (Some(was), None) => MouseKind::Up(was),
                    (None, None) => MouseKind::Moved,
                };
                held.0 = down;
                kind
            };
            // termwiz's coordinates count from 1.
            Event::Mouse(Mouse {
                kind,
                column: x.saturating_sub(1),
                row: y.saturating_sub(1),
                modifiers: self::modifiers(modifiers),
            })
        }
        InputEvent::Resized { cols, rows } => Event::Resize {
            columns: u16::try_from(cols).unwrap_or(u16::MAX),
            rows: u16::try_from(rows).unwrap_or(u16::MAX),
        },
        InputEvent::Paste(text) => Event::Paste(text),
        InputEvent::PixelMouse(_) | InputEvent::Wake => return None,
    })
}

fn modifiers(held: M) -> Modifiers {
    Modifiers {
        shift: held.contains(M::SHIFT),
        ctrl: held.contains(M::CTRL),
        alt: held.contains(M::ALT),
    }
}

fn key(key: K, held: M) -> Option<Key> {
    // Super is not a modifier here, so Super+A is not A.
    if held.intersects(M::SUPER) {
        return None;
    }
    let mut modifiers = self::modifiers(held);
    let code = match key {
        // A control character: the key that sends it (Esc, from the kitty
        // protocol's `CSI 27 u`, among them).
        K::Char(c) if c.is_control() => return Some(char_key(c, modifiers)),
        K::Char(c) => {
            // An upper-case letter already says Shift; the kitty protocol
            // sends Ctrl+Shift+A as Shift and `a`.
            if modifiers.shift && c.is_ascii_lowercase() {
                modifiers.shift = false;
                KeyCode::Char(c.to_ascii_uppercase())
            } else {
                if c.is_uppercase() {
                    modifiers.shift = false;
                }
                KeyCode::Char(c)
            }
        }
        K::Tab if modifiers.shift => {
            modifiers.shift = false;
            KeyCode::BackTab
        }
        K::Tab => KeyCode::Tab,
        K::Enter => KeyCode::Enter,
        K::Backspace => KeyCode::Backspace,
        K::Escape => KeyCode::Escape,
        K::Delete => KeyCode::Delete,
        K::Insert => KeyCode::Insert,
        K::UpArrow | K::ApplicationUpArrow => KeyCode::Up,
        K::DownArrow | K::ApplicationDownArrow => KeyCode::Down,
        K::LeftArrow | K::ApplicationLeftArrow => KeyCode::Left,
        K::RightArrow | K::ApplicationRightArrow => KeyCode::Right,
        K::Home | K::KeyPadHome => KeyCode::Home,
        K::End | K::KeyPadEnd => KeyCode::End,
        K::PageUp | K::KeyPadPageUp => KeyCode::PageUp,
        K::PageDown | K::KeyPadPageDown => KeyCode::PageDown,
        K::Function(n) => KeyCode::F(n),
        _ => return None,
    };
    Some(Key::with(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::termwiz::input::InputParser;

    /// Every event termwiz's parser reads from `bytes`.
    fn read(bytes: &[u8], exact: bool) -> Vec<Event> {
        let mut held = HeldButton::default();
        InputParser::new()
            .parse_as_vec(bytes, false)
            .into_iter()
            .filter_map(|event| translate(event, &mut held, exact))
            .collect()
    }

    fn key(name: &str) -> Event {
        Event::Key(Key::parse(name).unwrap())
    }

    #[test]
    fn keys_read_as_a_legacy_terminal_sends_them() {
        for (bytes, name) in [
            (&b"a"[..], "a"),
            (b"A", "A"),
            (b"\x01", "ctrl+a"),
            (b"\t", "tab"),
            (b"\r", "enter"),
            (b"\x7f", "backspace"),
            (b"\x1b", "esc"),
            (b"\x1bx", "alt+x"),
            (b"\x1b[Z", "shift+tab"),
            (b"\x1b[A", "up"),
            (b"\x1b[1;5C", "ctrl+right"),
            (b"\x1b[3~", "delete"),
            (b"\x1b[6~", "pagedown"),
            (b"\x1bOP", "f1"),
            (b"\x1b[15~", "f5"),
        ] {
            let read = self::read(bytes, false);
            assert_eq!(read, [key(name)], "{bytes:?}");
            assert!(!read[0].key().unwrap().is_exact(), "{bytes:?}");
        }
        let tab = read(b"\t", false)[0].key().unwrap();
        assert!(tab.matches(&Key::parse("ctrl+i").unwrap()));
        assert_eq!(
            read(b"\x1b[200~pasted\x1b[201~", false),
            [Event::Paste("pasted".into())]
        );
    }

    #[test]
    fn kitty_keys_are_exact() {
        for (bytes, name) in [
            (&b"\x1b[105;5u"[..], "ctrl+i"),
            (b"\t", "tab"),
            (b"\x1b[27u", "esc"),
            (b"\x1b[99;5u", "ctrl+c"),
            (b"\x1b[97;6u", "ctrl+shift+a"),
            (b"\x1b[13;2u", "shift+enter"),
        ] {
            let read = self::read(bytes, true);
            assert_eq!(read, [key(name)], "{bytes:?}");
            assert!(read[0].key().unwrap().is_exact(), "{bytes:?}");
        }
        let ctrl_i = read(b"\x1b[105;5u", true)[0].key().unwrap();
        assert!(!ctrl_i.matches(&Key::parse("tab").unwrap()));
    }

    #[test]
    fn the_mouse_reports_what_changed() {
        let at = |kind, column, row| Event::Mouse(Mouse::new(kind, column, row));
        assert_eq!(
            read(
                b"\x1b[<0;5;3M\x1b[<32;6;3M\x1b[<0;6;3m\x1b[<35;7;3M\x1b[<64;1;1M\x1b[<65;1;1M",
                false
            ),
            [
                at(MouseKind::Down(Button::Left), 4, 2),
                at(MouseKind::Drag(Button::Left), 5, 2),
                at(MouseKind::Up(Button::Left), 5, 2),
                at(MouseKind::Moved, 6, 2),
                at(MouseKind::ScrollUp, 0, 0),
                at(MouseKind::ScrollDown, 0, 0),
            ]
        );
        let Event::Mouse(shifted) = read(b"\x1b[<6;1;1M", false)[0] else {
            panic!("a mouse event");
        };
        assert_eq!(shifted.kind, MouseKind::Down(Button::Right));
        assert!(shifted.modifiers.shift);
    }
}

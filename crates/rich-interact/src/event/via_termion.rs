//! termion's events (#677), in the crate's own types.

use ::termion::event::{Event as T, Key as K, MouseButton as B, MouseEvent as M};

use super::{char_key, Button, Event, HeldButton, Key, KeyCode, Modifiers, Mouse, MouseKind};

/// Translate a termion event, for a loop of your own that reads the
/// terminal with termion. termion reads keys as a legacy terminal sends
/// them, so a key may be another the terminal sends alike (see
/// [`Key::matches`]); it reports no key releases, no modifiers with the
/// mouse, and no movement with no button held, and a sequence it does not
/// know (bracketed paste's markers among them) is
/// [`Unsupported`](termion::event::Event::Unsupported), which translates
/// to `None`. `held` is the button pressed, which termion's releases and
/// drags do not say.
pub fn from_termion(event: T, held: &mut HeldButton) -> Option<Event> {
    match event {
        T::Key(key) => self::key(key).map(Event::Key),
        T::Mouse(mouse) => {
            let (kind, column, row) = match mouse {
                M::Press(button, column, row) => {
                    let kind = match button {
                        B::Left => MouseKind::Down(Button::Left),
                        B::Right => MouseKind::Down(Button::Right),
                        B::Middle => MouseKind::Down(Button::Middle),
                        B::WheelUp => MouseKind::ScrollUp,
                        B::WheelDown => MouseKind::ScrollDown,
                        B::WheelLeft | B::WheelRight => return None,
                    };
                    if let MouseKind::Down(button) = kind {
                        held.0 = Some(button);
                    }
                    (kind, column, row)
                }
                M::Release(column, row) => (
                    MouseKind::Up(held.0.take().unwrap_or(Button::Left)),
                    column,
                    row,
                ),
                M::Hold(column, row) => {
                    (MouseKind::Drag(held.0.unwrap_or(Button::Left)), column, row)
                }
            };
            // termion's coordinates count from 1.
            Some(Event::Mouse(Mouse::new(
                kind,
                column.saturating_sub(1),
                row.saturating_sub(1),
            )))
        }
        T::Unsupported(_) => None,
    }
}

fn key(key: K) -> Option<Key> {
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ctrl: false,
        alt: false,
    };
    const ALT: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: true,
    };
    const CTRL: Modifiers = Modifiers::CTRL;
    let with = |code, modifiers| Some(Key::with(code, modifiers));
    let plain = |code| Some(Key::new(code));
    match key {
        K::Backspace => plain(KeyCode::Backspace),
        K::Left => plain(KeyCode::Left),
        K::ShiftLeft => with(KeyCode::Left, SHIFT),
        K::AltLeft => with(KeyCode::Left, ALT),
        K::CtrlLeft => with(KeyCode::Left, CTRL),
        K::Right => plain(KeyCode::Right),
        K::ShiftRight => with(KeyCode::Right, SHIFT),
        K::AltRight => with(KeyCode::Right, ALT),
        K::CtrlRight => with(KeyCode::Right, CTRL),
        K::Up => plain(KeyCode::Up),
        K::ShiftUp => with(KeyCode::Up, SHIFT),
        K::AltUp => with(KeyCode::Up, ALT),
        K::CtrlUp => with(KeyCode::Up, CTRL),
        K::Down => plain(KeyCode::Down),
        K::ShiftDown => with(KeyCode::Down, SHIFT),
        K::AltDown => with(KeyCode::Down, ALT),
        K::CtrlDown => with(KeyCode::Down, CTRL),
        K::Home => plain(KeyCode::Home),
        K::CtrlHome => with(KeyCode::Home, CTRL),
        K::End => plain(KeyCode::End),
        K::CtrlEnd => with(KeyCode::End, CTRL),
        K::PageUp => plain(KeyCode::PageUp),
        K::PageDown => plain(KeyCode::PageDown),
        K::BackTab => plain(KeyCode::BackTab),
        K::Delete => plain(KeyCode::Delete),
        K::Insert => plain(KeyCode::Insert),
        K::F(n) => plain(KeyCode::F(n)),
        K::Char(c) => Some(char_key(c, Modifiers::NONE)),
        K::Alt(c) => Some(char_key(c, ALT)),
        // Ctrl with `a` to `z` (Ctrl+I, J and M arrive as Tab and Enter),
        // or with `4` to `7` for the bytes after Esc.
        K::Ctrl(c) => with(KeyCode::Char(c), CTRL),
        K::Null => Some(Key::ctrl(' ')),
        K::Esc => plain(KeyCode::Escape),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(bytes: &[u8]) -> Option<Event> {
        let mut rest = bytes[1..].iter().map(|&b| Ok(b));
        let event = ::termion::event::parse_event(bytes[0], &mut rest).ok()?;
        from_termion(event, &mut HeldButton::default())
    }

    fn key(name: &str) -> Option<Event> {
        Some(Event::Key(Key::parse(name).unwrap()))
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
            (b"\x1bx", "alt+x"),
            (b"\x1b\x01", "alt+ctrl+a"),
            (b"\x00", "ctrl+space"),
            (b"\x1c", "ctrl+4"),
            (b"\x1b[Z", "shift+tab"),
            (b"\x1b[A", "up"),
            (b"\x1b[1;5C", "ctrl+right"),
            (b"\x1b[1;2D", "shift+left"),
            (b"\x1b[3~", "delete"),
            (b"\x1b[6~", "pagedown"),
            (b"\x1bOP", "f1"),
            (b"\x1b[15~", "f5"),
        ] {
            let read = self::read(bytes);
            assert_eq!(read, key(name), "{bytes:?}");
            assert!(!read.unwrap().key().unwrap().is_exact(), "{bytes:?}");
        }
        // Tab and Ctrl+I are one byte: either binding fires.
        let Some(Event::Key(tab)) = read(b"\t") else {
            panic!("a key");
        };
        assert!(tab.matches(&Key::parse("ctrl+i").unwrap()));
    }

    #[test]
    fn the_mouse_remembers_the_button_held() {
        let mut held = HeldButton::default();
        let mut read = |bytes: &[u8]| {
            let mut rest = bytes[1..].iter().map(|&b| Ok(b));
            let event = ::termion::event::parse_event(bytes[0], &mut rest).ok()?;
            from_termion(event, &mut held)
        };
        let at = |kind, column, row| Some(Event::Mouse(Mouse::new(kind, column, row)));
        assert_eq!(
            read(b"\x1b[<2;5;3M"),
            at(MouseKind::Down(Button::Right), 4, 2)
        );
        assert_eq!(
            read(b"\x1b[<2;6;3m"),
            at(MouseKind::Up(Button::Right), 5, 2)
        );
        assert_eq!(
            read(b"\x1b[<0;1;1M"),
            at(MouseKind::Down(Button::Left), 0, 0)
        );
        assert_eq!(
            read(b"\x1b[<32;2;1M"),
            at(MouseKind::Drag(Button::Left), 1, 0)
        );
        assert_eq!(read(b"\x1b[<0;2;1m"), at(MouseKind::Up(Button::Left), 1, 0));
        assert_eq!(read(b"\x1b[<64;1;1M"), at(MouseKind::ScrollUp, 0, 0));
        assert_eq!(read(b"\x1b[<65;1;1M"), at(MouseKind::ScrollDown, 0, 0));
        // A sequence termion does not know, and the mouse with a modifier
        // (Shift and the left button).
        assert_eq!(read(b"\x1b[200~"), None);
        assert_eq!(read(b"\x1b[<4;1;1M"), None);
        assert_eq!(
            from_termion(T::Unsupported(b"\x1b[200~".to_vec()), &mut held),
            None
        );
    }
}

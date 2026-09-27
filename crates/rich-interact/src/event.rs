//! Input events: keys, mouse, resizes, pastes and ticks.
//!
//! These are the crate's own types, so a component never sees `crossterm`,
//! and a test can write `Key::parse("ctrl+c")` instead of building one.

use std::fmt;

/// Modifier keys held with a key or mouse event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };
    pub const CTRL: Modifiers = Modifiers {
        shift: false,
        ctrl: true,
        alt: false,
    };
}

/// A key, without its modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Tab,
    /// Shift+Tab, as terminals send it.
    BackTab,
    Backspace,
    Delete,
    Insert,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
}

/// A key press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: Modifiers,
}

impl Key {
    pub const fn new(code: KeyCode) -> Key {
        Key {
            code,
            modifiers: Modifiers::NONE,
        }
    }

    pub const fn ctrl(c: char) -> Key {
        Key {
            code: KeyCode::Char(c),
            modifiers: Modifiers::CTRL,
        }
    }

    pub const fn char(c: char) -> Key {
        Key::new(KeyCode::Char(c))
    }

    /// Parse a key name such as `enter`, `ctrl+c`, `shift+tab`, `pagedown`,
    /// `f5` or a single character. Case does not matter for names.
    pub fn parse(text: &str) -> Option<Key> {
        if text == "+" {
            return Some(Key::char('+'));
        }
        let mut modifiers = Modifiers::NONE;
        let mut parts: Vec<&str> = text.split('+').collect();
        // `ctrl++` is Ctrl and the plus key.
        if text.ends_with("++") {
            parts.truncate(parts.len() - 2);
            parts.push("+");
        }
        let (name, held) = parts.split_last()?;
        for modifier in held {
            match modifier.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "meta" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                _ => return None,
            }
        }
        let mut chars = name.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(if modifiers.ctrl {
                c.to_ascii_lowercase()
            } else {
                c
            }),
            _ => match name.to_ascii_lowercase().as_str() {
                "enter" | "return" => KeyCode::Enter,
                "tab" if modifiers.shift => {
                    modifiers.shift = false;
                    KeyCode::BackTab
                }
                "tab" => KeyCode::Tab,
                "backtab" => KeyCode::BackTab,
                "backspace" => KeyCode::Backspace,
                "delete" | "del" => KeyCode::Delete,
                "insert" => KeyCode::Insert,
                "escape" | "esc" => KeyCode::Escape,
                "space" => KeyCode::Char(' '),
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "home" => KeyCode::Home,
                "end" => KeyCode::End,
                "pageup" => KeyCode::PageUp,
                "pagedown" => KeyCode::PageDown,
                other => KeyCode::F(other.strip_prefix('f')?.parse().ok()?),
            },
        };
        Some(Key { code, modifiers })
    }

    /// Ctrl+C: the event loop ends every component with
    /// [`Outcome::Interrupted`](crate::Outcome::Interrupted) on it.
    pub fn is_interrupt(&self) -> bool {
        self.modifiers.ctrl && self.code == KeyCode::Char('c')
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.ctrl {
            f.write_str("ctrl+")?;
        }
        if self.modifiers.alt {
            f.write_str("alt+")?;
        }
        if self.modifiers.shift {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::F(n) => write!(f, "f{n}"),
            code => f.write_str(&format!("{code:?}").to_ascii_lowercase()),
        }
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Left,
    Right,
    Middle,
}

/// What the mouse did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseKind {
    Down(Button),
    Up(Button),
    Drag(Button),
    Moved,
    ScrollUp,
    ScrollDown,
}

/// A mouse event, at a column and row of the terminal (0-based).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mouse {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: Modifiers,
}

/// One input event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Key(Key),
    Mouse(Mouse),
    /// The terminal is now this size.
    Resize {
        columns: u16,
        rows: u16,
    },
    /// Text pasted with bracketed paste on.
    Paste(String),
    /// A component's tick interval passed (see
    /// [`Component::tick`](crate::Component::tick)).
    Tick,
    /// A command the component handed the terminal to (with
    /// [`Flow::Handoff`](crate::Flow::Handoff)) exited with this code
    /// (`None`: killed by a signal).
    Returned(Option<i32>),
}

impl Event {
    /// The key, when this is a key press.
    pub fn key(&self) -> Option<Key> {
        match self {
            Event::Key(key) => Some(*key),
            _ => None,
        }
    }
}

impl From<Key> for Event {
    fn from(key: Key) -> Event {
        Event::Key(key)
    }
}

fn modifiers(state: crossterm::event::KeyModifiers) -> Modifiers {
    use crossterm::event::KeyModifiers as M;
    Modifiers {
        shift: state.contains(M::SHIFT),
        ctrl: state.contains(M::CONTROL),
        alt: state.contains(M::ALT),
    }
}

fn button(button: crossterm::event::MouseButton) -> Button {
    use crossterm::event::MouseButton as B;
    match button {
        B::Left => Button::Left,
        B::Right => Button::Right,
        B::Middle => Button::Middle,
    }
}

/// Translate a crossterm event. Key releases and repeats (reported only by
/// terminals with the kitty protocol) and focus changes are dropped.
pub(crate) fn from_crossterm(event: crossterm::event::Event) -> Option<Event> {
    use crossterm::event::{Event as E, KeyCode as K, KeyEventKind, MouseEventKind as MK};
    Some(match event {
        E::Key(key) if key.kind == KeyEventKind::Press => {
            let mut held = modifiers(key.modifiers);
            let code = match key.code {
                K::Char(c) => {
                    // An upper-case letter already says Shift.
                    if c.is_uppercase() {
                        held.shift = false;
                    }
                    KeyCode::Char(c)
                }
                K::Enter => KeyCode::Enter,
                K::Tab => KeyCode::Tab,
                K::BackTab => {
                    held.shift = false;
                    KeyCode::BackTab
                }
                K::Backspace => KeyCode::Backspace,
                K::Delete => KeyCode::Delete,
                K::Insert => KeyCode::Insert,
                K::Esc => KeyCode::Escape,
                K::Up => KeyCode::Up,
                K::Down => KeyCode::Down,
                K::Left => KeyCode::Left,
                K::Right => KeyCode::Right,
                K::Home => KeyCode::Home,
                K::End => KeyCode::End,
                K::PageUp => KeyCode::PageUp,
                K::PageDown => KeyCode::PageDown,
                K::F(n) => KeyCode::F(n),
                _ => return None,
            };
            Event::Key(Key {
                code,
                modifiers: held,
            })
        }
        E::Mouse(mouse) => Event::Mouse(Mouse {
            kind: match mouse.kind {
                MK::Down(b) => MouseKind::Down(button(b)),
                MK::Up(b) => MouseKind::Up(button(b)),
                MK::Drag(b) => MouseKind::Drag(button(b)),
                MK::Moved => MouseKind::Moved,
                MK::ScrollUp => MouseKind::ScrollUp,
                MK::ScrollDown => MouseKind::ScrollDown,
                _ => return None,
            },
            column: mouse.column,
            row: mouse.row,
            modifiers: modifiers(mouse.modifiers),
        }),
        E::Resize(columns, rows) => Event::Resize { columns, rows },
        E::Paste(text) => Event::Paste(text),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_names() {
        assert_eq!(Key::parse("enter"), Some(Key::new(KeyCode::Enter)));
        assert_eq!(Key::parse("Ctrl+C"), Some(Key::ctrl('c')));
        assert!(Key::parse("ctrl+c").unwrap().is_interrupt());
        assert_eq!(Key::parse("shift+tab"), Some(Key::new(KeyCode::BackTab)));
        assert_eq!(Key::parse("f12"), Some(Key::new(KeyCode::F(12))));
        assert_eq!(Key::parse("space"), Some(Key::char(' ')));
        assert_eq!(Key::parse("x"), Some(Key::char('x')));
        assert_eq!(Key::parse("ctrl++").unwrap().code, KeyCode::Char('+'));
        assert_eq!(Key::parse("+"), Some(Key::char('+')));
        assert_eq!(Key::parse("hyper+x"), None);
        assert_eq!(Key::parse("fx"), None);
    }

    #[test]
    fn keys_display_as_they_parse() {
        for name in ["ctrl+c", "enter", "pagedown", "space", "f3", "alt+x"] {
            assert_eq!(Key::parse(name).unwrap().to_string(), name);
        }
    }

    #[test]
    fn translates_crossterm_events() {
        use crossterm::event::{
            Event as E, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M,
        };
        let press = E::Key(KeyEvent::new(K::Char('c'), M::CONTROL));
        assert_eq!(from_crossterm(press), Some(Event::Key(Key::ctrl('c'))));
        let mut release = KeyEvent::new(K::Enter, M::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(from_crossterm(E::Key(release)), None);
        let upper = E::Key(KeyEvent::new(K::Char('A'), M::SHIFT));
        assert_eq!(from_crossterm(upper), Some(Event::Key(Key::char('A'))));
        assert_eq!(
            from_crossterm(E::Resize(80, 24)),
            Some(Event::Resize {
                columns: 80,
                rows: 24
            })
        );
    }
}

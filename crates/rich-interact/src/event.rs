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
///
/// Two keys are equal when their code and modifiers are. How the terminal
/// reported one changes only which bindings it [`matches`](Key::matches):
/// a terminal without the kitty keyboard protocol (a legacy terminal) sends
/// one byte for Tab and for Ctrl+I, so its Tab could be either key, while a
/// key marked [`exact`](Key::exact) is only itself.
#[derive(Clone, Copy, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: Modifiers,
    /// Reported by a terminal that tells every key apart.
    exact: bool,
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.code == other.code && self.modifiers == other.modifiers
    }
}

impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.code.hash(state);
        self.modifiers.hash(state);
    }
}

impl Key {
    pub const fn new(code: KeyCode) -> Key {
        Key::with(code, Modifiers::NONE)
    }

    /// A key and the modifiers held with it.
    pub const fn with(code: KeyCode, modifiers: Modifiers) -> Key {
        Key {
            code,
            modifiers,
            exact: false,
        }
    }

    pub const fn ctrl(c: char) -> Key {
        Key::with(KeyCode::Char(c), Modifiers::CTRL)
    }

    pub const fn char(c: char) -> Key {
        Key::new(KeyCode::Char(c))
    }

    /// Parse a key name such as `enter`, `ctrl+c`, `shift+tab`, `pagedown`,
    /// `f5` or a single character. Case does not matter for names.
    ///
    /// A name means the key: `ctrl+i` is Ctrl+I, not Tab, although a legacy
    /// terminal sends the two alike; [`matches`](Key::matches) is what lets
    /// a Tab from such a terminal fire a `ctrl+i` binding.
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
            // Shift with a letter is the capital letter, with Ctrl too
            // (`ctrl+shift+a`), as the key arrives. Shift with anything
            // else (`shift+1`) depends on the keyboard layout, and is not a
            // key name.
            (Some(c), None) if modifiers.shift => {
                if !c.is_alphabetic() {
                    return None;
                }
                modifiers.shift = false;
                KeyCode::Char(c.to_uppercase().next().unwrap_or(c))
            }
            // Ctrl with a letter of either case is the lower-case letter:
            // `ctrl+A` is Ctrl+A, and Ctrl+Shift+A is `ctrl+shift+a`.
            (Some(c), None) if modifiers.ctrl => KeyCode::Char(c.to_ascii_lowercase()),
            (Some(c), None) => KeyCode::Char(c),
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
                other => match other.strip_prefix('f')?.parse().ok()? {
                    // The function keys terminals send.
                    n @ 1..=24 => KeyCode::F(n),
                    _ => return None,
                },
            },
        };
        Some(Key::with(code, modifiers))
    }

    /// This key as a terminal that tells every key apart reports it (the
    /// kitty keyboard protocol): it [`matches`](Key::matches) a binding for
    /// itself only. A [`Session`](crate::Session) with the protocol on
    /// reads keys so, and a test marks them to stand for such a terminal.
    pub const fn exact(self) -> Key {
        Key {
            exact: true,
            ..self
        }
    }

    /// Whether the key came from a terminal that tells every key apart.
    pub const fn is_exact(&self) -> bool {
        self.exact
    }

    /// This key as a legacy terminal sends it: Ctrl+I arrives as Tab,
    /// Ctrl+M as Enter, Ctrl+[ as Esc, Ctrl+\ ] ^ _ as Ctrl+4 to 7, Ctrl+@
    /// as Ctrl+Space, and Ctrl+Shift with a letter as Ctrl and the letter.
    /// Any other key arrives as itself. The result is not
    /// [`exact`](Key::exact).
    pub fn legacy(self) -> Key {
        let mut key = Key {
            exact: false,
            ..self
        };
        let KeyCode::Char(c) = key.code else {
            return key;
        };
        if !key.modifiers.ctrl {
            return key;
        }
        key.code = match c.to_ascii_lowercase() {
            'i' => KeyCode::Tab,
            'm' => KeyCode::Enter,
            '[' => KeyCode::Escape,
            '\\' => KeyCode::Char('4'),
            ']' => KeyCode::Char('5'),
            '^' => KeyCode::Char('6'),
            '_' => KeyCode::Char('7'),
            '@' => KeyCode::Char(' '),
            c => KeyCode::Char(c),
        };
        // Tab, Enter and Esc are bytes of their own, without Ctrl.
        if matches!(key.code, KeyCode::Tab | KeyCode::Enter | KeyCode::Escape) {
            key.modifiers.ctrl = false;
        }
        key
    }

    /// Whether this key, as read, fires a binding for `binding`. A key
    /// fires a binding for itself. One from a legacy terminal also fires a
    /// binding for any key that terminal sends alike (see
    /// [`legacy`](Key::legacy)): its Tab fires `tab` or `ctrl+i`; and its
    /// Ctrl+H, which some terminals send for Backspace, fires `ctrl+h` or
    /// `backspace`. An [`exact`](Key::exact) key fires only its own.
    pub fn matches(&self, binding: &Key) -> bool {
        if self == binding {
            return true;
        }
        if self.exact {
            return false;
        }
        if binding.legacy() == *self {
            return true;
        }
        binding.code == KeyCode::Backspace
            && !binding.modifiers.ctrl
            && self.code == KeyCode::Char('h')
            && self.modifiers
                == Modifiers {
                    ctrl: true,
                    ..binding.modifiers
                }
    }

    /// Whether this key fires a binding for any of `bindings`.
    pub fn matches_any<'a>(&self, bindings: impl IntoIterator<Item = &'a Key>) -> bool {
        bindings.into_iter().any(|binding| self.matches(binding))
    }

    /// Which of `sets` (each the keys of one binding) this key fires: the
    /// first that has it exactly, else the first it
    /// [`matches`](Key::matches). A Tab from a legacy terminal picks a
    /// `tab` binding over an earlier `ctrl+i` one.
    pub fn pick<'a>(&self, sets: impl IntoIterator<Item = &'a [Key]>) -> Option<usize> {
        let sets: Vec<&[Key]> = sets.into_iter().collect();
        sets.iter()
            .position(|keys| keys.contains(self))
            .or_else(|| sets.iter().position(|keys| self.matches_any(*keys)))
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
        // Shift+Tab is written as it is parsed and documented: `shift+tab`,
        // and so is Ctrl with a capital letter: `ctrl+shift+a`.
        let capital =
            matches!(self.code, KeyCode::Char(c) if self.modifiers.ctrl && c.is_ascii_uppercase());
        if self.modifiers.shift || self.code == KeyCode::BackTab || capital {
            f.write_str("shift+")?;
        }
        match self.code {
            KeyCode::BackTab => f.write_str("tab"),
            KeyCode::Char(' ') => f.write_str("space"),
            KeyCode::Char(c) if capital => write!(f, "{}", c.to_ascii_lowercase()),
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

/// A mouse event, at a column and row (0-based). The event loop reports
/// them to a component relative to the top left of its own view (#476), so
/// row 0 is the view's first line wherever the view is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Mouse {
    pub kind: MouseKind,
    pub column: u16,
    pub row: u16,
    pub modifiers: Modifiers,
}

impl Mouse {
    pub const fn new(kind: MouseKind, column: u16, row: u16) -> Mouse {
        Mouse {
            kind,
            column,
            row,
            modifiers: Modifiers::NONE,
        }
    }

    /// A press of the left button: the start of a click.
    pub fn is_click(&self) -> bool {
        self.kind == MouseKind::Down(Button::Left)
    }
}

/// One input event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A key pressed, or held down long enough to repeat.
    Key(Key),
    /// A key let go. Terminals report it with the kitty keyboard protocol
    /// on (see [`SessionOptions::legacy_keys`](crate::SessionOptions)), and
    /// the Windows console always. Nothing binds it: a component that wants
    /// it matches it itself.
    KeyUp(Key),
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
    /// The left button went down on a hyperlink (an OSC 8 region: a
    /// [`Style`](rich::Style) with a link) in the component's view (#476).
    /// Delivered instead of the [`Event::Mouse`]; what to do with the URL
    /// (open it, print it, ignore it) is the component's or its caller's
    /// choice, never the event loop's.
    Link(String),
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

impl Event {
    /// The mouse event, when this is one.
    pub fn mouse(&self) -> Option<Mouse> {
        match self {
            Event::Mouse(mouse) => Some(*mouse),
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

/// Translate a crossterm event, for a loop of your own that reads the
/// terminal with crossterm (or with ratatui, which re-exports it), in its
/// legacy mode: keys may be others the terminal sends alike (see
/// [`Key::matches`]). With the kitty protocol pushed, use
/// [`from_crossterm_kitty`]. Repeats arrive as presses, releases as
/// [`Event::KeyUp`]; focus changes are dropped.
pub fn from_crossterm(event: crossterm::event::Event) -> Option<Event> {
    translate(event, false)
}

/// [`from_crossterm`] for a terminal with the kitty keyboard protocol
/// pushed (`PushKeyboardEnhancementFlags` with at least
/// `DISAMBIGUATE_ESCAPE_CODES`): every key is [`exact`](Key::exact).
pub fn from_crossterm_kitty(event: crossterm::event::Event) -> Option<Event> {
    translate(event, true)
}

fn translate(event: crossterm::event::Event, exact: bool) -> Option<Event> {
    use crossterm::event::{
        Event as E, KeyCode as K, KeyEventKind, KeyModifiers as M, MouseEventKind as MK,
    };
    Some(match event {
        E::Key(key) => {
            // Super and Hyper, which only the kitty protocol reports, are
            // not modifiers here: Super+A is not A.
            if key.modifiers.intersects(M::SUPER | M::HYPER) {
                return None;
            }
            let mut held = modifiers(key.modifiers);
            held.alt |= key.modifiers.contains(M::META);
            let code = match key.code {
                K::Char(c) => {
                    // An upper-case letter already says Shift; the kitty
                    // protocol sends Ctrl+Shift+A as Shift and `a`.
                    if held.shift && c.is_ascii_lowercase() {
                        held.shift = false;
                        KeyCode::Char(c.to_ascii_uppercase())
                    } else {
                        if c.is_uppercase() {
                            held.shift = false;
                        }
                        KeyCode::Char(c)
                    }
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
            let mut read = Key::with(code, held);
            read.exact = exact;
            if key.kind == KeyEventKind::Release {
                Event::KeyUp(read)
            } else {
                Event::Key(read)
            }
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

    fn key(name: &str) -> Key {
        Key::parse(name).expect("a key name")
    }

    #[test]
    fn names_are_the_keys_terminals_send() {
        // Shift and a letter is the capital letter, with Ctrl too.
        assert_eq!(Key::parse("shift+a"), Some(Key::char('A')));
        assert_eq!(Key::parse("alt+shift+a"), Key::parse("alt+A"));
        assert_eq!(
            Key::parse("ctrl+shift+a"),
            Some(Key::with(KeyCode::Char('A'), Modifiers::CTRL))
        );
        assert_eq!(Key::parse("ctrl+A"), Some(Key::ctrl('a')));
        // Shift with a symbol depends on the layout.
        assert_eq!(Key::parse("shift+1"), None);
        // A name means the key, even where a legacy terminal sends another.
        assert_eq!(Key::parse("ctrl+i"), Some(Key::ctrl('i')));
        assert_eq!(Key::parse("ctrl+m"), Some(Key::ctrl('m')));
        assert_eq!(Key::parse("ctrl+["), Some(Key::ctrl('[')));
        assert_eq!(Key::parse("ctrl+]"), Some(Key::ctrl(']')));
        assert_eq!(Key::parse("ctrl+@"), Some(Key::ctrl('@')));
        assert_eq!(Key::parse("ctrl+h"), Some(Key::ctrl('h')));
        // ... and a key from a legacy terminal fires the bindings of every
        // key it could be.
        assert!(key("tab").matches(&key("ctrl+i")));
        assert!(key("enter").matches(&key("ctrl+m")));
        assert!(key("esc").matches(&key("ctrl+[")));
        assert!(Key::ctrl('5').matches(&key("ctrl+]")));
        assert!(key("ctrl+space").matches(&key("ctrl+@")));
        assert!(key("ctrl+a").matches(&key("ctrl+shift+a")));
        // Function keys terminals have.
        assert_eq!(Key::parse("f24"), Some(Key::new(KeyCode::F(24))));
        assert_eq!(Key::parse("f0"), None);
        assert_eq!(Key::parse("f99"), None);
    }

    #[test]
    fn legacy_keys_match_every_key_they_could_be() {
        // What a legacy terminal sends for each name.
        for (name, sent) in [
            ("ctrl+i", "tab"),
            ("ctrl+m", "enter"),
            ("ctrl+[", "esc"),
            ("ctrl+\\", "ctrl+4"),
            ("ctrl+]", "ctrl+5"),
            ("ctrl+^", "ctrl+6"),
            ("ctrl+_", "ctrl+7"),
            ("ctrl+@", "ctrl+space"),
            ("alt+ctrl+i", "alt+tab"),
            ("ctrl+shift+a", "ctrl+a"),
            ("ctrl+h", "ctrl+h"),
            ("tab", "tab"),
            ("f5", "f5"),
        ] {
            assert_eq!(key(name).legacy(), key(sent), "{name}");
            // Its bytes fire a binding for the name and for what was sent.
            assert!(key(sent).matches(&key(name)), "{sent} fires {name}");
            assert!(key(sent).matches(&key(sent)), "{sent} fires {sent}");
        }
        // Some terminals send Ctrl+H for Backspace; Backspace is only itself.
        assert!(key("ctrl+h").matches(&key("backspace")));
        assert!(key("alt+ctrl+h").matches(&key("alt+backspace")));
        assert!(!key("backspace").matches(&key("ctrl+h")));
        // Keys that send bytes of their own never match each other.
        assert!(!key("ctrl+i").matches(&key("tab")));
        assert!(!key("tab").matches(&key("ctrl+j")));
        assert!(!key("ctrl+a").matches(&key("ctrl+b")));
        assert!(key("tab").matches_any(&keys(&["x", "ctrl+i"])));
    }

    fn keys(names: &[&str]) -> Vec<Key> {
        names.iter().map(|name| key(name)).collect()
    }

    #[test]
    fn exact_keys_match_only_themselves() {
        assert!(key("tab").exact().matches(&key("tab")));
        assert!(!key("tab").exact().matches(&key("ctrl+i")));
        assert!(key("ctrl+i").exact().matches(&key("ctrl+i")));
        assert!(!key("ctrl+i").exact().matches(&key("tab")));
        assert!(!key("enter").exact().matches(&key("ctrl+m")));
        assert!(!key("esc").exact().matches(&key("ctrl+[")));
        assert!(!key("ctrl+h").exact().matches(&key("backspace")));
        assert!(!key("ctrl+a").exact().matches(&key("ctrl+shift+a")));
        // How a key was read is not part of what it is.
        assert_eq!(key("tab").exact(), key("tab"));
        assert!(key("tab").exact().is_exact());
        assert!(!key("tab").exact().legacy().is_exact());
        let mut set = std::collections::HashSet::new();
        set.insert(key("q").exact());
        assert!(set.contains(&key("q")));
    }

    #[test]
    fn an_exact_name_wins_over_a_key_it_could_be() {
        let bindings = [keys(&["ctrl+i"]), keys(&["x", "tab"])];
        let sets = || bindings.iter().map(Vec::as_slice);
        assert_eq!(key("tab").pick(sets()), Some(1));
        assert_eq!(key("tab").exact().pick(sets()), Some(1));
        assert_eq!(key("ctrl+i").pick(sets()), Some(0));
        let only = [keys(&["ctrl+i"])];
        assert_eq!(key("tab").pick(only.iter().map(Vec::as_slice)), Some(0));
        assert_eq!(
            key("tab").exact().pick(only.iter().map(Vec::as_slice)),
            None
        );
    }

    #[test]
    fn keys_display_as_they_parse() {
        for name in [
            "ctrl+c",
            "enter",
            "pagedown",
            "space",
            "f3",
            "alt+x",
            "ctrl+i",
            "ctrl+[",
            "ctrl+shift+a",
            "shift+tab",
            "A",
        ] {
            assert_eq!(key(name).to_string(), name);
        }
    }

    #[test]
    fn translates_crossterm_events() {
        use crossterm::event::{Event as E, KeyCode as K, KeyEvent, KeyModifiers as M};
        let press = E::Key(KeyEvent::new(K::Char('c'), M::CONTROL));
        assert_eq!(from_crossterm(press), Some(Event::Key(Key::ctrl('c'))));
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

    #[test]
    fn releases_and_repeats_from_crossterm() {
        use crossterm::event::{
            Event as E, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M,
        };
        let with = |code, modifiers, kind| {
            let mut event = KeyEvent::new(code, modifiers);
            event.kind = kind;
            E::Key(event)
        };
        let release = with(K::Char('q'), M::NONE, KeyEventKind::Release);
        assert_eq!(from_crossterm(release), Some(Event::KeyUp(Key::char('q'))));
        let repeat = with(K::Down, M::NONE, KeyEventKind::Repeat);
        assert_eq!(
            from_crossterm(repeat),
            Some(Event::Key(Key::new(KeyCode::Down)))
        );
        // Released, the key is read as pressed: exact with the protocol.
        let up = with(K::Tab, M::NONE, KeyEventKind::Release);
        let Some(Event::KeyUp(tab)) = from_crossterm_kitty(up) else {
            panic!("a release");
        };
        assert!(tab.is_exact());
    }

    #[test]
    fn kitty_keys_from_crossterm_are_exact() {
        use crossterm::event::{Event as E, KeyCode as K, KeyEvent, KeyModifiers as M};
        let read =
            |code, modifiers| match from_crossterm_kitty(E::Key(KeyEvent::new(code, modifiers))) {
                Some(Event::Key(key)) => Some(key),
                _ => None,
            };
        let ctrl_i = read(K::Char('i'), M::CONTROL).unwrap();
        assert!(ctrl_i.is_exact() && ctrl_i.matches(&key("ctrl+i")));
        let tab = read(K::Tab, M::NONE).unwrap();
        assert!(tab.matches(&key("tab")) && !tab.matches(&key("ctrl+i")));
        // Ctrl+Shift+A arrives as Shift and `a`: the name is ctrl+shift+a.
        assert_eq!(
            read(K::Char('a'), M::CONTROL | M::SHIFT),
            Key::parse("ctrl+shift+a")
        );
        // Super is not a modifier here, so Super+A is not A.
        assert_eq!(read(K::Char('a'), M::SUPER), None);
        // The legacy translation leaves the same keys ambiguous.
        let Some(Event::Key(legacy_tab)) = from_crossterm(E::Key(KeyEvent::new(K::Tab, M::NONE)))
        else {
            panic!("a key");
        };
        assert!(!legacy_tab.is_exact() && legacy_tab.matches(&key("ctrl+i")));
    }
}

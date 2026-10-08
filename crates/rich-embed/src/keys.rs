//! Keys, the mouse and pastes as the bytes an xterm-compatible terminal
//! sends a program.
//!
//! Keys follow xterm: arrows and Home/End in normal or application cursor
//! mode, modifiers as `CSI 1;m X` and `CSI n;m ~`, F1 to F4 as `SS3 P`
//! to `SS3 S`, F13 to F24 as Shift with F1 to F12, Alt as a leading Esc.
//! The mouse is reported the way the program asked for it: which events
//! (presses; releases; drags; all movement) and in which encoding (X10's
//! bytes, UTF-8 or SGR).

use rich_intuituive::interact::{Button, Key, KeyCode, Modifiers, Mouse, MouseKind};
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// The xterm modifier parameter: 1, plus 1 for Shift, 2 for Alt, 4 for
/// Ctrl. `None` without modifiers.
fn modifier_parameter(modifiers: Modifiers) -> Option<u8> {
    let n = modifiers.shift as u8 + 2 * modifiers.alt as u8 + 4 * modifiers.ctrl as u8;
    (n > 0).then_some(n + 1)
}

/// `CSI 1;m final`, or the unmodified form `normal`/`application`.
fn cursor_key(key: Key, last: char, application: bool) -> Vec<u8> {
    match modifier_parameter(key.modifiers) {
        Some(m) => format!("\x1b[1;{m}{last}").into_bytes(),
        None if application => format!("\x1bO{last}").into_bytes(),
        None => format!("\x1b[{last}").into_bytes(),
    }
}

/// `CSI n ~`, or `CSI n;m ~` with modifiers.
fn tilde_key(key: Key, n: u8) -> Vec<u8> {
    match modifier_parameter(key.modifiers) {
        Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
        None => format!("\x1b[{n}~").into_bytes(),
    }
}

/// The control character for Ctrl with `c`, as terminals send it.
fn control(c: char) -> Option<u8> {
    Some(match c {
        'a'..='z' => c as u8 - b'a' + 1,
        'A'..='Z' => c as u8 - b'A' + 1,
        ' ' | '@' | '2' => 0,
        '[' | '3' => 0x1b,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '7' | '/' => 0x1f,
        '8' | '?' => 0x7f,
        _ => return None,
    })
}

/// The bytes for `key`; `application_cursor` is the program's cursor key
/// mode (DECCKM).
pub fn key_bytes(key: Key, application_cursor: bool) -> Vec<u8> {
    let alt = key.modifiers.alt;
    let mut out = Vec::new();
    match key.code {
        KeyCode::Char(c) => {
            if alt {
                out.push(0x1b);
            }
            match control(c).filter(|_| key.modifiers.ctrl) {
                Some(byte) => out.push(byte),
                None => {
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
            }
            out
        }
        KeyCode::Enter | KeyCode::Tab | KeyCode::Backspace | KeyCode::Escape => {
            if alt {
                out.push(0x1b);
            }
            out.push(match key.code {
                KeyCode::Enter => b'\r',
                KeyCode::Tab => b'\t',
                KeyCode::Backspace if key.modifiers.ctrl => 0x08,
                KeyCode::Backspace => 0x7f,
                _ => 0x1b,
            });
            out
        }
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Up => cursor_key(key, 'A', application_cursor),
        KeyCode::Down => cursor_key(key, 'B', application_cursor),
        KeyCode::Right => cursor_key(key, 'C', application_cursor),
        KeyCode::Left => cursor_key(key, 'D', application_cursor),
        KeyCode::Home => cursor_key(key, 'H', application_cursor),
        KeyCode::End => cursor_key(key, 'F', application_cursor),
        KeyCode::Insert => tilde_key(key, 2),
        KeyCode::Delete => tilde_key(key, 3),
        KeyCode::PageUp => tilde_key(key, 5),
        KeyCode::PageDown => tilde_key(key, 6),
        KeyCode::F(n) => {
            // F13 to F24 are Shift with F1 to F12, as xterm sends them.
            let (n, mut key) = if (13..=24).contains(&n) {
                let mut shifted = key;
                shifted.modifiers.shift = true;
                (n - 12, shifted)
            } else {
                (n, key)
            };
            key.code = KeyCode::F(n);
            match n {
                1..=4 => {
                    let last = (b'P' + n - 1) as char;
                    match modifier_parameter(key.modifiers) {
                        Some(m) => format!("\x1b[1;{m}{last}").into_bytes(),
                        None => format!("\x1bO{last}").into_bytes(),
                    }
                }
                5..=12 => {
                    const CODES: [u8; 8] = [15, 17, 18, 19, 20, 21, 23, 24];
                    tilde_key(key, CODES[n as usize - 5])
                }
                _ => Vec::new(),
            }
        }
    }
}

/// Text pasted into the program: wrapped in bracketed-paste markers when
/// the program asked for them (with any end marker inside taken out, so
/// a paste cannot end itself early), else as typed, line breaks as Enter.
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let inner = text.replace("\x1b[201~", "");
        format!("\x1b[200~{inner}\x1b[201~").into_bytes()
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

/// The button number in a mouse report.
fn button_code(button: Button) -> u8 {
    match button {
        Button::Left => 0,
        Button::Middle => 1,
        Button::Right => 2,
    }
}

/// The report for `mouse` (at its column and row in the pane), if the
/// program asked for this kind of event, in the encoding it asked for.
pub fn mouse_bytes(
    mouse: Mouse,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
) -> Option<Vec<u8>> {
    let wanted = match mouse.kind {
        MouseKind::Down(_) | MouseKind::ScrollUp | MouseKind::ScrollDown => {
            mode != MouseProtocolMode::None
        }
        MouseKind::Up(_) => !matches!(mode, MouseProtocolMode::None | MouseProtocolMode::Press),
        MouseKind::Drag(_) => matches!(
            mode,
            MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
        ),
        MouseKind::Moved => mode == MouseProtocolMode::AnyMotion,
    };
    if !wanted {
        return None;
    }
    let sgr = encoding == MouseProtocolEncoding::Sgr;
    let mut code = match mouse.kind {
        MouseKind::Down(button) => button_code(button),
        // X10's encoding cannot say which button went up.
        MouseKind::Up(button) if sgr => button_code(button),
        MouseKind::Up(_) => 3,
        MouseKind::Drag(button) => 32 + button_code(button),
        MouseKind::Moved => 35,
        MouseKind::ScrollUp => 64,
        MouseKind::ScrollDown => 65,
    };
    code += 4 * mouse.modifiers.shift as u8
        + 8 * mouse.modifiers.alt as u8
        + 16 * mouse.modifiers.ctrl as u8;
    let (x, y) = (mouse.column as u32 + 1, mouse.row as u32 + 1);
    if sgr {
        let last = if matches!(mouse.kind, MouseKind::Up(_)) {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{x};{y}{last}").into_bytes());
    }
    let mut out = b"\x1b[M".to_vec();
    out.push(32 + code);
    for value in [x, y] {
        let value = 32 + value;
        match encoding {
            MouseProtocolEncoding::Utf8 => {
                let c = char::from_u32(value).filter(|_| value < 2048)?;
                let mut buffer = [0u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
            }
            // One byte each: a column past 222 cannot be said.
            _ => out.push(u8::try_from(value).ok()?),
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> Vec<u8> {
        key_bytes(Key::parse(name).unwrap(), false)
    }

    #[test]
    fn keys_send_what_xterm_sends() {
        assert_eq!(key("a"), b"a");
        assert_eq!(key("A"), b"A");
        assert_eq!(key("é"), "é".as_bytes());
        assert_eq!(key("ctrl+c"), b"\x03");
        assert_eq!(key("ctrl+space"), b"\x00");
        assert_eq!(key("alt+x"), b"\x1bx");
        assert_eq!(key("enter"), b"\r");
        assert_eq!(key("tab"), b"\t");
        assert_eq!(key("shift+tab"), b"\x1b[Z");
        assert_eq!(key("backspace"), b"\x7f");
        assert_eq!(key("escape"), b"\x1b");
        assert_eq!(key("up"), b"\x1b[A");
        assert_eq!(key_bytes(Key::parse("up").unwrap(), true), b"\x1bOA");
        assert_eq!(key("ctrl+left"), b"\x1b[1;5D");
        assert_eq!(key("shift+up"), b"\x1b[1;2A");
        assert_eq!(key("home"), b"\x1b[H");
        assert_eq!(key("delete"), b"\x1b[3~");
        assert_eq!(key("pageup"), b"\x1b[5~");
        assert_eq!(key("alt+pagedown"), b"\x1b[6;3~");
        assert_eq!(key("f1"), b"\x1bOP");
        assert_eq!(key("ctrl+f2"), b"\x1b[1;5Q");
        assert_eq!(key("f5"), b"\x1b[15~");
        assert_eq!(key("f12"), b"\x1b[24~");
        assert_eq!(key("f13"), b"\x1b[1;2P");
        assert_eq!(key("f24"), b"\x1b[24;2~");
    }

    #[test]
    fn pastes_are_bracketed_when_asked() {
        assert_eq!(paste_bytes("a\nb", false), b"a\rb");
        assert_eq!(
            paste_bytes("x\x1b[201~y", true),
            b"\x1b[200~xy\x1b[201~".to_vec()
        );
    }

    #[test]
    fn the_mouse_is_reported_as_the_program_asked() {
        use MouseProtocolEncoding as E;
        use MouseProtocolMode as M;
        let down = Mouse::new(MouseKind::Down(Button::Left), 4, 2);
        let up = Mouse::new(MouseKind::Up(Button::Left), 4, 2);
        let moved = Mouse::new(MouseKind::Moved, 4, 2);
        assert_eq!(mouse_bytes(down, M::None, E::Default), None);
        assert_eq!(
            mouse_bytes(down, M::PressRelease, E::Default),
            Some(b"\x1b[M\x20\x25\x23".to_vec())
        );
        assert_eq!(
            mouse_bytes(up, M::PressRelease, E::Default),
            Some(b"\x1b[M\x23\x25\x23".to_vec())
        );
        assert_eq!(mouse_bytes(up, M::Press, E::Sgr), None);
        assert_eq!(
            mouse_bytes(up, M::PressRelease, E::Sgr),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
        assert_eq!(mouse_bytes(moved, M::ButtonMotion, E::Sgr), None);
        assert_eq!(
            mouse_bytes(moved, M::AnyMotion, E::Sgr),
            Some(b"\x1b[<35;5;3M".to_vec())
        );
        let wheel = Mouse::new(MouseKind::ScrollDown, 0, 0);
        assert_eq!(
            mouse_bytes(wheel, M::Press, E::Sgr),
            Some(b"\x1b[<65;1;1M".to_vec())
        );
        // Too far right for X10's single bytes, but not for UTF-8.
        let far = Mouse::new(MouseKind::Down(Button::Left), 300, 0);
        assert_eq!(mouse_bytes(far, M::Press, E::Default), None);
        assert!(mouse_bytes(far, M::Press, E::Utf8).is_some());
    }
}

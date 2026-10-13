//! `rich_ext::surface::Surface` against a real VT emulator (vt100, which
//! rs-rich-record depends on): the terminal shows what the surface's model
//! says (the 0.0.19 release test).

use rich::{Style, Text};
use rich_ext::surface::{Rect, Surface, Viewport};

fn term(rows: u16, cols: u16, bytes: &[u8]) -> vt100::Parser {
    let mut p = vt100::Parser::new(rows, cols, 0);
    p.process(bytes);
    p
}

fn rows_of(p: &vt100::Parser) -> Vec<String> {
    p.screen().rows(0, p.screen().size().1).collect()
}

/// A link URL is written raw inside OSC 8: a URL carrying ESC \ ends the
/// OSC early, and what follows reaches the terminal as controls.
#[test]
fn a_link_url_cannot_drive_the_terminal() {
    let mut surface = Surface::new(Vec::new(), 20, 2);
    let evil = Style::new().with_link("https://x\x1b\\\x1b[2;5HPWNED");
    surface
        .draw(|s| {
            s.render(Rect::new(0, 0, 20, 1), &Text::styled("a", evil.clone()));
        })
        .unwrap();
    let out = String::from_utf8(surface.writer().clone()).unwrap();
    eprintln!("{out:?}");
    let p = term(2, 20, out.as_bytes());
    let rows = rows_of(&p);
    eprintln!("terminal: {rows:?}; model: {:?}", surface.screen().plain());
    assert!(
        !rows.concat().contains("PWNED"),
        "a link's URL drove the terminal: {rows:?}"
    );
}

/// Drawing a narrow cell over the second half of a wide one: the screen
/// keeps the wide grapheme, the terminal erases it.
#[test]
fn overlapping_a_wide_grapheme_keeps_screen_and_terminal_in_step() {
    let mut surface = Surface::new(Vec::new(), 6, 1);
    surface
        .draw(|s| s.render(Rect::new(0, 0, 6, 1), &Text::new("日本語")))
        .unwrap();
    surface
        .draw(|s| s.render(Rect::new(1, 0, 1, 1), &Text::new("x")))
        .unwrap();
    let out = String::from_utf8(surface.writer().clone()).unwrap();
    let p = term(1, 6, out.as_bytes());
    let terminal = rows_of(&p)[0].clone();
    let model = surface.screen().plain()[0].clone();
    eprintln!("terminal {terminal:?} model {model:?}");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(model.as_str()),
        6,
        "the screen model's row is not 6 cells: {model:?}"
    );
    assert_eq!(model.trim_end(), terminal.trim_end());
}

/// Same, the rectangle's right edge on the first half of a wide grapheme.
#[test]
fn overlapping_the_first_half_of_a_wide_grapheme() {
    let mut surface = Surface::new(Vec::new(), 6, 1);
    surface
        .draw(|s| s.render(Rect::new(0, 0, 6, 1), &Text::new("日本語")))
        .unwrap();
    surface
        .draw(|s| s.render(Rect::new(2, 0, 1, 1), &Text::new("x")))
        .unwrap();
    let out = String::from_utf8(surface.writer().clone()).unwrap();
    let p = term(1, 6, out.as_bytes());
    let terminal = rows_of(&p)[0].clone();
    let model = surface.screen().plain()[0].clone();
    eprintln!("terminal {terminal:?} model {model:?}");
    assert_eq!(
        unicode_width::UnicodeWidthStr::width(model.as_str()),
        6,
        "the screen model's row is not 6 cells: {model:?}"
    );
}

/// Inline near the bottom of the terminal: the frame scrolls the shell up
/// and the second frame updates the right rows.
#[test]
fn inline_near_the_bottom() {
    let mut bytes = b"$ one\r\n$ two\r\n$ three\r\n$ four".to_vec();
    bytes.extend_from_slice(b"\r\n");
    let mut surface = Surface::new(Vec::new(), 10, 5).viewport(Viewport::Inline(3));
    surface
        .draw(|s| s.render(s.area(), &Text::new("r0\nr1\nr2")))
        .unwrap();
    surface
        .draw(|s| s.render(Rect::new(0, 0, 10, 1), &Text::new("R0")))
        .unwrap();
    surface
        .draw(|s| s.render(Rect::new(0, 2, 10, 1), &Text::new("R2")))
        .unwrap();
    surface.finish().unwrap();
    bytes.extend_from_slice(surface.writer());
    bytes.extend_from_slice(b"$ next");
    let p = term(5, 10, &bytes);
    let rows = rows_of(&p);
    eprintln!("{rows:#?}");
    let rows: Vec<&str> = rows.iter().map(|r| r.trim_end()).collect();
    assert_eq!(rows, ["$ four", "R0", "r1", "R2", "$ next"]);
}

/// An inline viewport taller than the terminal.
#[test]
fn inline_taller_than_the_terminal() {
    let mut surface = Surface::new(Vec::new(), 10, 3).viewport(Viewport::Inline(5));
    surface
        .draw(|s| s.render(s.area(), &Text::new("r0\nr1\nr2\nr3\nr4")))
        .unwrap();
    surface
        .draw(|s| s.render(Rect::new(0, 0, 10, 1), &Text::new("R0")))
        .unwrap();
    let out = surface.writer().clone();
    let p = term(3, 10, &out);
    let rows = rows_of(&p);
    eprintln!("{rows:#?}");
    let rows: Vec<&str> = rows.iter().map(|r| r.trim_end()).collect();
    // Whatever part of the region fits, it must be in order.
    assert!(
        rows == ["R0", "r1", "r2"] || rows == ["r2", "r3", "r4"],
        "rows out of order: {rows:?}"
    );
    // The terminal shrinks under an inline surface: the region must fit.
    let mut shrunk = Surface::new(Vec::new(), 10, 5).viewport(Viewport::Inline(5));
    shrunk.resize(10, 3);
    assert!(shrunk.area().height <= 3, "{:?}", shrunk.area());
}

#[test]
fn zero_sizes_and_resizes() {
    let mut surface = Surface::new(Vec::new(), 0, 0);
    surface
        .draw(|s| {
            s.render(s.area(), &Text::new("x"));
            s.set_cursor(0, 0);
        })
        .unwrap();
    surface.resize(5, 2);
    surface
        .draw(|s| {
            s.render(s.area(), &Text::new("hello\nworld"));
            s.set_cursor(4, 1);
        })
        .unwrap();
    surface.resize(0, 0);
    surface.draw(|s| s.set_cursor(4, 1)).unwrap();
    surface.resize(u16::MAX, 1);
    surface
        .draw(|s| {
            s.render(s.area(), &Text::new("wide"));
            s.set_cursor(u16::MAX - 1, 0);
        })
        .unwrap();
    let mut inline = Surface::new(Vec::new(), 5, 5).viewport(Viewport::Inline(0));
    inline
        .draw(|s| s.render(s.area(), &Text::new("x")))
        .unwrap();
    inline.finish().unwrap();
    let mut sync = Surface::new(Vec::new(), 5, 1).synchronized(true);
    sync.draw(|s| s.set_cursor(1, 0)).unwrap();
    sync.draw(|s| s.set_cursor(1, 0)).unwrap();
    let out = String::from_utf8(sync.writer().clone()).unwrap();
    eprintln!("sync: {out:?}");
}

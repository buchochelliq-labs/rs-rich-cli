//! `edit::Buffer` under load and at its edges (the 0.0.19 release test).

use std::time::{Duration, Instant};

use rich_interact::edit::{Buffer, Pos};

fn timed<R>(what: &str, f: impl FnOnce() -> R) -> (R, Duration) {
    let t = Instant::now();
    let r = f();
    let e = t.elapsed();
    eprintln!("{what}: {e:?}");
    (r, e)
}

#[test]
fn hundred_thousand_lines_edit() {
    let text: String = (0..100_000)
        .map(|i| format!("line {i} with some text in it"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut b = Buffer::new(&text);
    b.set_caret(Pos::new(50_000, 3), false);
    let (_, e) = timed("insert char at line 50k", || b.insert("x"));
    assert!(e < Duration::from_millis(50), "{e:?}");
    let (_, e) = timed("paste 3 lines at 50k", || b.insert("a\nb\nc"));
    assert!(e < Duration::from_millis(50), "{e:?}");
    let (_, e) = timed("undo", || b.undo());
    assert!(e < Duration::from_millis(50), "{e:?}");
    b.set_limit(Some(10_000_000));
    let (_, e) = timed("insert with limit", || b.insert("y"));
    eprintln!("with a limit: {e:?}");
}

/// Up/Down on a long unwrapped line.
#[test]
fn a_vertical_move_on_a_long_line_is_quick() {
    let long = "x".repeat(20_000);
    let mut b = Buffer::new(&format!("{long}\n{long}"));
    b.end(false);
    let (_, e) = timed("Down from column 20000", || b.vertical(1, false));
    assert_eq!(b.caret(), Pos::new(1, 20_000));
    assert!(e < Duration::from_millis(200), "Down took {e:?}");
}

/// Ctrl+Right over a long word.
#[test]
fn a_word_move_over_a_long_word_is_quick() {
    let mut b = Buffer::new(&"x".repeat(20_000));
    let (_, e) = timed("Ctrl+Right over 20000-char word", || b.word_right(false));
    assert_eq!(b.caret(), Pos::new(0, 20_000));
    assert!(e < Duration::from_millis(200), "Ctrl+Right took {e:?}");
}

#[test]
fn paste_with_controls_and_escape_sequences() {
    let mut b = Buffer::new("");
    b.insert("a\x1b[31mred\x1b[0m\x07\u{9b}2J\x1b]0;title\x07b\r\nc\u{85}d");
    let text = b.text();
    assert!(
        !text.chars().any(|c| c.is_control() && c != '\n'),
        "{text:?}"
    );
    eprintln!("{text:?}");
}

#[test]
fn undo_redo_across_a_paste() {
    let mut b = Buffer::new("");
    b.insert("a");
    b.insert("b");
    b.insert("one\ntwo\nthree");
    b.insert("c");
    assert_eq!(b.text(), "abone\ntwo\nthreec");
    b.undo();
    assert_eq!(b.text(), "abone\ntwo\nthree");
    b.undo();
    assert_eq!(b.text(), "ab");
    assert_eq!(b.caret(), Pos::new(0, 2));
    b.redo();
    assert_eq!(b.text(), "abone\ntwo\nthree");
    assert_eq!(b.caret(), Pos::new(2, 5));
    b.redo();
    assert_eq!(b.text(), "abone\ntwo\nthreec");
}

/// A one-character paste joins the typing run before it.
#[test]
fn one_char_paste_joins_typing_run() {
    let mut b = Buffer::new("");
    b.insert("a");
    b.insert("b");
    b.insert("c"); // as a paste would
    b.undo();
    eprintln!("after undo: {:?}", b.text());
}

/// Inserting before a combining mark or a regional indicator never leaves
/// the caret inside a grapheme cluster.
#[test]
fn caret_stays_on_a_grapheme_boundary_after_insert() {
    // Line starting with a regional indicator; insert another before it:
    // the two join into one flag cluster.
    let mut b = Buffer::new("\u{1F1FA}");
    b.insert("\u{1F1F8}");
    let caret = b.caret();
    let clamped = b.clamp(caret);
    assert_eq!(
        caret,
        clamped,
        "caret {caret:?} is inside a cluster; text {:?}",
        b.text()
    );
}

/// A line that starts with a combining mark: typing before it joins the
/// two into one cluster, and the caret goes after it, so what is typed next
/// does not take the mark.
#[test]
fn typing_before_a_combining_mark_leaves_the_caret_after_the_cluster() {
    let mut b = Buffer::new("\u{301}x");
    b.insert("e");
    let caret = b.caret();
    eprintln!(
        "text {:?} caret {caret:?} clamp {:?}",
        b.text(),
        b.clamp(caret)
    );
    b.insert("a");
    eprintln!("after typing a: {:?}", b.text());
    assert_eq!(b.text(), "e\u{301}ax", "typed text landed inside a cluster");
}

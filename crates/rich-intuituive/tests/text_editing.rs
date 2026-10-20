//! The text editors' editing options: rebindable keys and the Emacs
//! preset, search and the find bar, word wrap, the cursor line, the
//! caret's shape (DECSCUSR) and a drawn caret, tabs, the undo limit, and
//! an input's validation message.

use std::time::Duration;

use intuituive::interact::caret::CaretShape;
use intuituive::interact::edit::WrapMode;
use intuituive::interact::{Event, Key};
use intuituive::prelude::*;
use intuituive::{search_bar, Driver, TextSearch};

/// A focused text area of `start`, given `options`, over a line showing
/// the signal's value (its line breaks as `|`); Ctrl+Q, Alt+D and Ctrl+Y
/// bound above it report that they reached the app.
fn editor(start: &'static str, options: fn(Node) -> Node) -> App {
    App::new(move || {
        let value = signal(start.to_string());
        let reached = signal(String::new());
        column([
            options(text_area(value)).autofocus(),
            text(move || format!("={}<", value.get().replace('\n', "|"))).fixed(1),
            text!("app:{reached}").fixed(1),
        ])
        .on_key("alt+d", move |_| reached.set("alt+d".into()))
        .on_key("f3", move |_| reached.set("f3".into()))
    })
}

fn rows(driver: &Driver) -> Vec<String> {
    let plain = driver.screen().plain();
    plain.iter().map(|row| row.trim_end().to_string()).collect()
}

fn value(driver: &Driver) -> String {
    rows(driver)
        .into_iter()
        .find_map(|row| {
            let row = row.strip_prefix('=')?;
            Some(row.strip_suffix('<').unwrap_or(row).to_string())
        })
        .expect("the value line")
}

fn reached(driver: &Driver) -> String {
    rows(driver)
        .into_iter()
        .find_map(|row| row.strip_prefix("app:").map(str::to_string))
        .expect("the app line")
}

#[test]
fn the_default_keys_are_as_they_were() {
    let mut driver = editor("one two", |n| n).driver(20, 4);
    driver.press("end ctrl+w");
    assert_eq!(value(&driver), "one ");
    driver.press("ctrl+z");
    assert_eq!(value(&driver), "one two");
    // Ctrl+Y redoes, Alt+D and F3 (with no search) go on to the app.
    driver.press("ctrl+y");
    assert_eq!(value(&driver), "one ");
    driver.press("alt+d");
    assert_eq!(reached(&driver), "alt+d");
    driver.press("f3");
    assert_eq!(reached(&driver), "f3");
    // Shift selects; Ctrl+Shift selects by word.
    driver.press("home ctrl+shift+right");
    driver.type_text("ONE");
    assert_eq!(value(&driver), "ONE ");
}

#[test]
fn the_emacs_preset_moves_kills_and_yanks() {
    let mut driver = editor("alpha beta\ngamma", Node::emacs_keys).driver(20, 4);
    driver.press("ctrl+e alt+b ctrl+k");
    assert_eq!(value(&driver), "alpha |gamma");
    driver.press("ctrl+n ctrl+a ctrl+y");
    assert_eq!(value(&driver), "alpha |betagamma");
    driver.press("ctrl+p ctrl+a alt+d");
    assert_eq!(value(&driver), " |betagamma");
    driver.press("ctrl+y alt+y");
    assert_eq!(
        value(&driver),
        "beta |betagamma",
        "Alt+Y swaps in the older kill"
    );
    // Ctrl+Space sets the mark; moves select, Alt+W copies onto the ring.
    driver.press("ctrl+a ctrl+space ctrl+f ctrl+f alt+w ctrl+e ctrl+y");
    assert_eq!(value(&driver), "beta be|betagamma");
    // Ctrl+D deletes forward, Ctrl+H back; Alt+D is the editor's now.
    driver.press("ctrl+a ctrl+d ctrl+e ctrl+h");
    assert_eq!(value(&driver), "eta b|betagamma");
    assert_eq!(reached(&driver), "");
}

#[test]
fn a_key_rebound_onto_an_action_leaves_the_one_that_had_it() {
    let mut driver = editor("one two", |n| {
        n.rebind_key("yank", "ctrl+y")
            .rebind_key("delete-to-end", "")
            .rebind_key("kill-to-end", "ctrl+k")
            .rebind_key("paragraph-down", "alt+n")
    })
    .driver(20, 4);
    driver.press("ctrl+k");
    assert_eq!(value(&driver), "");
    driver.press("ctrl+y ctrl+y");
    assert_eq!(
        value(&driver),
        "one twoone two",
        "Ctrl+Y yanks, no longer redoes"
    );
    driver.press("ctrl+z");
    assert_eq!(value(&driver), "one two");
}

#[test]
fn delete_next_word_and_paragraph_moves_can_be_bound() {
    let mut driver = editor("a b\n\nc d", |n| {
        n.rebind_key("delete-next-word", "alt+d")
            .rebind_key("paragraph-down", "alt+n")
    })
    .driver(20, 5);
    driver.press("alt+d");
    assert_eq!(value(&driver), " b||c d");
    driver.press("alt+n");
    driver.type_text("X");
    assert_eq!(value(&driver), " b|X|c d");
    assert_eq!(reached(&driver), "");
}

#[test]
fn search_selects_matches_and_reports_where_it_is() {
    let app = App::new(|| {
        let body = signal("cat\nhat cat\nbat".to_string());
        let search = TextSearch::new();
        column([
            text_area(body).search(search).autofocus(),
            text(move || format!("{:?} {:?}", search.found(), search.error())).fixed(1),
        ])
        .on_key("ctrl+p", move |_| search.previous())
        .on_key("f5", move |_| search.set("cat"))
        .on_key("f6", move |_| {
            search.set_regex(true);
            search.set("[bh]at");
        })
        .on_key("f7", move |_| search.set("("))
    });
    let mut driver = app.driver(20, 4);
    driver.settle();
    assert_eq!(rows(&driver)[3], "None None", "no pattern, no count");
    driver.press("f5");
    assert_eq!(rows(&driver)[3], "Some((1, 2)) None");
    // F3 in the area goes on; round past the end.
    driver.press("f3");
    assert_eq!(rows(&driver)[3], "Some((2, 2)) None");
    driver.press("f3");
    assert_eq!(rows(&driver)[3], "Some((1, 2)) None");
    driver.press("ctrl+p");
    assert_eq!(rows(&driver)[3], "Some((2, 2)) None");
    // Typing replaces the selected match.
    driver.type_text("dog");
    assert_eq!(rows(&driver)[1], "hat dog");
    assert_eq!(rows(&driver)[3], "Some((0, 1)) None");
    // A new pattern selects the first match from the caret on.
    driver.press("f6");
    assert_eq!(rows(&driver)[3], "Some((2, 2)) None");
    driver.press("f7");
    assert!(
        rows(&driver)[3].starts_with("None Some("),
        "{}",
        rows(&driver)[3]
    );
}

#[test]
fn matches_in_view_are_highlighted() {
    let app = App::new(|| {
        let search = TextSearch::new();
        search.set("ab");
        text_area(signal("ab ab".to_string()))
            .search(search)
            .autofocus()
            .theme_styles([
                ("text_area.search", "underline"),
                ("text_area.search.current", "bold"),
                ("text_area.selection", "italic"),
            ])
    });
    let mut driver = app.driver(10, 1);
    driver.settle();
    assert_eq!(
        intuituive::testing::styled(&driver),
        "[bold]ab[/] [underline]ab[/]\n"
    );
}

#[test]
fn the_find_bar_drives_a_search() {
    let app = App::new(|| {
        let text = signal("one two one".to_string());
        let search = TextSearch::new();
        column([
            text_area(text).search(search),
            search_bar(search).fixed(1).autofocus(),
        ])
    });
    let mut driver = app.driver(30, 2);
    driver.type_text("one");
    assert_eq!(rows(&driver)[1], "Find one        1/2");
    driver.press("down");
    assert_eq!(rows(&driver)[1], "Find one        2/2");
    driver.press("up up");
    assert_eq!(rows(&driver)[1], "Find one        2/2");
    driver.press("alt+r");
    assert_eq!(rows(&driver)[1], "Find one        2/2 .*");
    driver.press("ctrl+a");
    driver.type_text("(");
    assert!(rows(&driver)[1].contains("unclosed") || rows(&driver)[1].len() > 9);
    driver.press("ctrl+a");
    driver.type_text("zzz");
    assert_eq!(rows(&driver)[1], "Find zzz        no matches .*");
}

#[test]
fn word_wrap_keeps_words_whole() {
    let app = App::new(|| {
        text_area(signal("the quick brown fox jumps".to_string()))
            .wrap_mode(WrapMode::Word)
            .autofocus()
    });
    let mut driver = app.driver(11, 4);
    driver.settle();
    assert_eq!(rows(&driver), ["the quick", "brown fox", "jumps", ""]);
    // The caret is placed on the rows as they are wrapped.
    driver.press("ctrl+end");
    assert_eq!(driver.cursor(), Some((5, 2)));
    // Back to a character wrap, and to none.
    let app =
        App::new(|| text_area(signal("the quick brown".to_string())).wrap_mode(WrapMode::Grapheme));
    let mut driver = app.driver(8, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["the qui", "ck brow", "n"]);
    let app =
        App::new(|| text_area(signal("the quick brown".to_string())).wrap_mode(WrapMode::None));
    let mut driver = app.driver(8, 2);
    driver.settle();
    assert_eq!(rows(&driver), ["the quic", ""]);
}

#[test]
fn the_cursor_line_is_highlighted_across_the_row() {
    let app = App::new(|| {
        text_area(signal("one\ntwo".to_string()))
            .cursor_line()
            .autofocus()
            .theme_styles([("text_area.cursor_line", "bold")])
    });
    let mut driver = app.driver(6, 2);
    driver.settle();
    assert_eq!(
        intuituive::testing::styled(&driver),
        "[bold]one   [/]\ntwo\n"
    );
    driver.press("down");
    assert_eq!(
        intuituive::testing::styled(&driver),
        "one\n[bold]two   [/]\n"
    );
}

/// Render after each of `keys`, collecting the bytes.
fn bytes(driver: &mut Driver, keys: &str) -> String {
    let mut out = driver.render().unwrap_or_default();
    for key in keys.split_whitespace() {
        driver.event(Event::Key(Key::parse(key).expect("a key")));
        driver.update(Duration::ZERO);
        out.push_str(&driver.render().unwrap_or_default());
    }
    out
}

#[test]
fn the_caret_shape_is_set_on_the_terminal_and_put_back() {
    let app = App::new(|| {
        column([
            input(signal(String::new()))
                .caret_shape(CaretShape::Bar)
                .fixed(1)
                .autofocus(),
            input(signal(String::new())).fixed(1),
        ])
    });
    let mut driver = app.driver(10, 2);
    let first = bytes(&mut driver, "");
    assert!(first.contains("\x1b[6 q"), "{first:?}");
    // Typing does not send it again.
    let typed = bytes(&mut driver, "a");
    assert!(!typed.contains(" q"), "{typed:?}");
    // The focus moves to one with no shape: the user's comes back.
    let moved = bytes(&mut driver, "tab");
    assert!(moved.contains("\x1b[0 q"), "{moved:?}");
    let back = bytes(&mut driver, "backtab");
    assert!(back.contains("\x1b[6 q"), "{back:?}");
    assert!(driver.finish().ends_with("\x1b[0 q"));
    // An app that never sets one never resets it.
    let app = App::new(|| input(signal(String::new())));
    let mut driver = app.driver(10, 1);
    assert!(!bytes(&mut driver, "a").contains(" q"));
    assert!(!driver.finish().contains(" q"));
}

#[test]
fn a_drawn_caret_is_in_the_cells_not_the_terminal() {
    let app = App::new(|| {
        input(signal("ab".to_string()))
            .caret(intuituive::Caret::Start)
            .draw_caret()
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(intuituive::testing::styled(&driver), "[reverse]a[/]b\n");
    assert_eq!(driver.cursor(), None);
    let app = App::new(|| {
        input(signal("ab".to_string()))
            .caret_shape(CaretShape::Underline)
            .draw_caret()
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(intuituive::testing::styled(&driver), "ab[underline] [/]\n");
}

#[test]
fn tabs_take_their_width_and_hard_tabs_indent_with_one() {
    let app = App::new(|| {
        let value = signal("x".to_string());
        column([
            text_area(value).tab_width(2).hard_tabs().autofocus(),
            text(move || format!("={:?}", value.get())).fixed(1),
        ])
    });
    let mut driver = app.driver(12, 2);
    driver.press("tab");
    driver.type_text("y");
    assert_eq!(rows(&driver)[0], "  yx");
    assert_eq!(rows(&driver)[1], "=\"\\tyx\"");
    let app = App::new(|| {
        let value = signal("x".to_string());
        column([
            text_area(value).tab_width(3).autofocus(),
            text(move || format!("={:?}", value.get())).fixed(1),
        ])
    });
    let mut driver = app.driver(12, 2);
    driver.press("tab");
    assert_eq!(rows(&driver)[1], "=\"   x\"");
}

#[test]
fn the_undo_limit_drops_the_oldest_steps() {
    let mut driver = editor("", |n| n.undo_limit(Some(2))).driver(20, 4);
    for word in ["a", "b", "c"] {
        driver.type_text(word);
        driver.press("enter");
    }
    driver.press("ctrl+z ctrl+z ctrl+z ctrl+z ctrl+z");
    assert_eq!(
        value(&driver),
        "a|b|",
        "two steps undone: c and its line break"
    );
}

#[test]
fn a_validation_message_shows_and_is_read_out() {
    let check = |text: &str| -> Option<String> {
        text.parse::<u8>().is_err().then(|| "a number".to_string())
    };
    // With room, under the text.
    let app = App::new(move || {
        let age = signal("x".to_string());
        column([
            input(age).label("Age").validate(check).fixed(2).autofocus(),
            label("end"),
        ])
    })
    .stylesheet(":error { text-style: bold; }");
    let mut driver = app.driver(20, 3);
    driver.settle();
    assert_eq!(rows(&driver), ["x", "a number", "end"]);
    let node = &driver.accessibility()[0];
    assert!(node.state.invalid);
    assert_eq!(node.error.as_deref(), Some("a number"));
    assert_eq!(node.describe(), "Age, text box: x, invalid: a number");
    assert!(node
        .aria_attributes()
        .contains(&("aria-description", "a number".to_string())));
    driver.press("backspace");
    driver.type_text("42");
    assert_eq!(rows(&driver), ["42", "", "end"]);
    let node = &driver.accessibility()[0];
    assert!(!node.state.invalid);
    assert_eq!(node.describe(), "Age, text box: 42");
    // One row: beside it, at the right.
    let app = App::new(move || input(signal("x".to_string())).validate(check).fixed(1));
    let mut driver = app.driver(20, 1);
    driver.settle();
    assert_eq!(rows(&driver), ["x           a number"]);
}

#[test]
fn error_message_works_beside_error_when() {
    let app = App::new(|| {
        let n = signal(0);
        text!("{n}")
            .label("N")
            .error_when(move || n.get() > 5)
            .error_message(move || (n.get() < 0).then(|| "negative".to_string()))
            .on_key("-", move |_| n.set(-1))
            .on_key("+", move |_| n.set(9))
    });
    let mut driver = app.driver(5, 1);
    driver.settle();
    assert_eq!(driver.accessibility()[0].describe(), "N");
    driver.press("+");
    assert_eq!(driver.accessibility()[0].describe(), "N, invalid");
    driver.press("-");
    assert_eq!(driver.accessibility()[0].describe(), "N, invalid: negative");
}

#[cfg(feature = "vim")]
#[test]
fn vim_keys_edit_modally_with_their_caret_shapes() {
    let app = App::new(|| {
        let value = signal("one\ntwo".to_string());
        let status = signal(String::new());
        column([
            text_area(value).vim().mode_status(status).autofocus(),
            text(move || format!("={}", value.get().replace('\n', "|"))).fixed(1),
            text!("{status}").fixed(1),
        ])
        .on_key(":", move |_| status.set("command".into()))
    });
    let mut driver = app.driver(20, 4);
    let first = bytes(&mut driver, "");
    assert!(first.contains("\x1b[2 q"), "a block in normal mode");
    driver.type_text("x");
    assert_eq!(value(&driver), "ne|two", "x deletes, it does not type");
    let inserting = bytes(&mut driver, "i");
    assert!(inserting.contains("\x1b[6 q"), "a bar while inserting");
    driver.type_text("o");
    driver.press("escape");
    driver.type_text("jdd");
    assert_eq!(value(&driver), "one");
    assert_eq!(rows(&driver)[3], "NORMAL");
    driver.type_text("2d");
    assert_eq!(rows(&driver)[3], "NORMAL 2d");
    driver.press("escape");
    driver.type_text(":");
    assert_eq!(rows(&driver)[3], "command", "`:` goes on to the app");
}

#[cfg(feature = "vim")]
#[path = "../examples/textarea.rs"]
#[allow(dead_code)]
mod gallery;

#[cfg(feature = "vim")]
#[test]
fn the_gallery_shows_each_tab() {
    let mut driver = gallery::textarea_app().driver(70, 12);
    driver.settle();
    for (key, shown) in [
        ("f2", "main.rs"),
        ("f3", "not a number"),
        ("f4", "word wrap"),
        ("f5", "emacs keys"),
        ("f6", "NORMAL"),
        ("f7", "password"),
        ("f1", "minimal"),
    ] {
        driver.press(key);
        let screen = rows(&driver).join("\n");
        if key == "f3" {
            driver.press("ctrl+a");
            driver.type_text("x");
        }
        let screen = if key == "f3" {
            rows(&driver).join("\n")
        } else {
            screen
        };
        assert!(screen.contains(shown), "{key}: {screen}");
    }
    // The editor's find bar.
    driver.press("f2 ctrl+f");
    driver.type_text("fish");
    assert!(rows(&driver).join("\n").contains("1/2"));
}

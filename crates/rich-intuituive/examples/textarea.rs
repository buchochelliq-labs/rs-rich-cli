//! ratatui-textarea's examples as one gallery, a tab each: a minimal
//! editor, an editor with a find bar, a validated single line, word wrap,
//! Emacs keys, Vim keys (the `vim` feature) and a password.
//!
//!     cargo run -p rs-rich-intuituive --features vim --example textarea
//!
//! F1 to F7 pick a tab; Ctrl+Q quits. In the editor tab, Ctrl+F opens the
//! find bar (Enter or ↓ next, ↑ previous, Alt+R regex, Alt+C any case,
//! Esc closes it). In the Vim tab, Esc goes to normal mode and `i`, `a`,
//! `o` back to inserting.

use intuituive::interact::caret::CaretShape;
use intuituive::interact::edit::WrapMode;
use intuituive::prelude::*;
use intuituive::{search_bar, TextSearch};

const PROSE: &str = "Text areas wrap long lines. With word wrap a line breaks after a \
space, so the words stay whole; with a character wrap it breaks wherever the room runs out, \
and with none it scrolls sideways to the caret.";

const CODE: &str = "fn main() {\n    let words = [\"one\", \"fish\", \"two\", \"fish\"];\n    \
for word in words {\n        println!(\"{word}\");\n    }\n}\n";

const TABS: [&str; 7] = [
    "minimal",
    "editor",
    "single line",
    "wrap",
    "emacs",
    "vim",
    "password",
];

fn main() -> std::io::Result<()> {
    textarea_app().run()
}

/// The app, for the tests too.
pub fn textarea_app() -> App {
    App::new(|| {
        let tab = signal(0usize);
        let pages = switch(move || tab.get(), page).autofocus();
        let id = pages.id();
        let mut root = column([
            intuituive::widgets::tabs(|| TABS.iter().map(|t| format!(" {t} ")).collect(), tab)
                .fixed(1),
            pages,
        ])
        .on_key("ctrl+q", |cx| cx.quit());
        for (i, key) in ["f1", "f2", "f3", "f4", "f5", "f6", "f7"]
            .iter()
            .enumerate()
        {
            // The page shown is built anew: the focus goes into it.
            root = root.on_key(key, move |cx| {
                tab.set(i);
                cx.focus_within(id);
            });
        }
        root
    })
}

fn page(index: usize) -> Node {
    match index {
        0 => minimal(),
        1 => editor(),
        2 => single_line(),
        3 => wrap(),
        4 => emacs(),
        5 => vim(),
        _ => password(),
    }
}

fn minimal() -> Node {
    text_area(signal(String::new()))
        .placeholder("Type here: the default keys, undo with Ctrl+Z")
        .autofocus()
        .panel("minimal")
}

// --8<-- [start:editor]
fn editor() -> Node {
    let code = signal(CODE.to_string());
    let search = TextSearch::new();
    let finding = signal(false);
    let area = text_area(code)
        .language("rust")
        .line_numbers()
        .cursor_line()
        .tab_width(4)
        .search(search)
        .autofocus()
        .panel("main.rs");
    let bar = switch(
        move || finding.get(),
        move |open| {
            if open {
                search_bar(search).autofocus()
            } else {
                label("[dim]Ctrl+F finds · F3 next match")
            }
        },
    )
    .fixed(1);
    let (area_id, bar_id) = (area.id(), bar.id());
    column([area, bar])
        .on_key("ctrl+f", move |cx| {
            finding.set(true);
            cx.focus_within(bar_id);
        })
        .on_key("escape", move |cx| {
            finding.set(false);
            cx.focus_within(area_id);
        })
}
// --8<-- [end:editor]

// --8<-- [start:single]
fn single_line() -> Node {
    let number = signal("3.14".to_string());
    column([
        input(number)
            .label("A number")
            .validate(|text| {
                text.trim()
                    .parse::<f64>()
                    .is_err()
                    .then(|| "not a number".to_string())
            })
            .autofocus()
            .panel("a number")
            .fixed(4),
        text!("[dim]value: {number}").fixed(1),
    ])
}
// --8<-- [end:single]

fn wrap() -> Node {
    column([
        text_area(signal(PROSE.to_string()))
            .wrap_mode(WrapMode::Word)
            .autofocus()
            .panel("word wrap"),
        text_area(signal(PROSE.to_string()))
            .wrap_mode(WrapMode::Grapheme)
            .panel("character wrap"),
        text_area(signal(PROSE.to_string()))
            .wrap_mode(WrapMode::None)
            .panel("no wrap"),
    ])
}

fn emacs() -> Node {
    text_area(signal("Ctrl+K kills to the end of the line, Ctrl+Y yanks it back.\nAlt+Y swaps in an older kill; Ctrl+Space sets the mark.".to_string()))
        .emacs_keys()
        .caret_shape(CaretShape::Bar)
        .autofocus()
        .panel("emacs keys")
}

// --8<-- [start:vim]
fn vim() -> Node {
    let mode = signal(String::new());
    column([
        text_area(signal(CODE.to_string()))
            .vim()
            .mode_status(mode)
            .line_numbers()
            .autofocus()
            .panel("vim"),
        text!("[reverse] {mode} [/] [dim]dw · 3x · yyp · cw · . repeats").fixed(1),
    ])
}
// --8<-- [end:vim]

fn password() -> Node {
    let secret = signal(String::new());
    column([
        input(secret)
            .mask('•')
            .placeholder("a password")
            .draw_caret()
            .autofocus()
            .panel("password")
            .fixed(3),
        text!("[dim]{} characters", secret.with(|s| s.chars().count())).fixed(1),
    ])
}

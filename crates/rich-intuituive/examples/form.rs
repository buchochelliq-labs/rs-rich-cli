//! A port of ratatui's `input-form` and `user-input` examples
//! (<https://github.com/ratatui/ratatui/tree/main/examples/apps>), rebuilt
//! on intuiTUIve as one app with a tab each. It reuses none of ratatui's
//! code: it rebuilds the behaviour and the look with intuiTUIve's API.
//!
//! Ported from ratatui 0.30 (<https://github.com/ratatui/ratatui>), by
//! Florian Dehau and the Ratatui Developers, MIT licence. The design, the
//! layout and the keys are theirs; the credit for them is theirs.
//!
//!     cargo run -p rs-rich-intuituive --example form
//!
//! F1 and F2 (or a click on the tab strip) change tabs.
//!
//! - **Form:** a first name, a last name and an age, with Tab (or Enter)
//!   between them. ↑/↓ or digits change the age. A field is in error while
//!   it is wrong: a name with a digit or a symbol at once, an empty field
//!   once Enter on the age has tried to send the form. Enter on the age,
//!   with nothing wrong, sends it: the app ends with the form, and `main`
//!   prints it. Esc quits without it.
//! - **Chat:** an input with two modes, as `user-input` has: e starts
//!   editing, Enter records the message, Esc stops editing. The messages
//!   are listed above the input. q quits while not editing.
//!
//! ? lists the keys and Ctrl+P finds a command, whenever the focus is not
//! in a text box.
//!
//! What intuiTUIve adds: each field is an `input` bound to a signal, the
//! errors are `error_when` (a stylesheet's `:error`, red by the theme, and
//! "invalid" to a screen reader), and the form comes back from the app as
//! a value (`App::run_for`) rather than through state the caller shares.

use std::cell::RefCell;

use intuituive::prelude::*;
use intuituive::reactive::NodeId;
use intuituive::widgets::tabs;

/// The tabs, in order.
pub const TABS: [&str; 2] = ["Form", "Chat"];

/// What the form sends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Person {
    pub first_name: String,
    pub last_name: String,
    pub age: i64,
}

impl std::fmt::Display for Person {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {}, {} years old",
            self.first_name, self.last_name, self.age
        )
    }
}

/// What is wrong with a name, if anything: empty (counted only once the
/// form was tried), or holding something other than letters, spaces,
/// hyphens and apostrophes.
pub fn name_problem(name: &str, tried: bool) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        tried.then_some("is empty")
    } else if !name
        .chars()
        .all(|c| c.is_alphabetic() || matches!(c, ' ' | '-' | '\''))
    {
        Some("takes letters, spaces, hyphens and apostrophes")
    } else {
        None
    }
}

/// What is wrong with an age, if anything: none given (0), once the form
/// was tried.
pub fn age_problem(age: i64, tried: bool) -> Option<&'static str> {
    (tried && age == 0).then_some("is not given")
}

/// The form's fields, as signals.
#[derive(Clone, Copy)]
struct Fields {
    first: Signal<String>,
    last: Signal<String>,
    age: Signal<i64>,
    /// Whether Enter on the age tried to send the form.
    tried: Signal<bool>,
}

impl Fields {
    /// Each field's problem, with its name.
    fn problems(&self) -> Vec<String> {
        let tried = self.tried.get();
        let mut out = Vec::new();
        if let Some(p) = self.first.with(|n| name_problem(n, tried)) {
            out.push(format!("The first name {p}."));
        }
        if let Some(p) = self.last.with(|n| name_problem(n, tried)) {
            out.push(format!("The last name {p}."));
        }
        if let Some(p) = age_problem(self.age.get(), tried) {
            out.push(format!("The age {p}."));
        }
        out
    }
}

/// The Form tab.
fn form(fields: Fields) -> Node {
    let Fields {
        first,
        last,
        age,
        tried,
    } = fields;
    let next = |_: &str, cx: &mut Ctx| cx.focus_next();
    let first_name = input(first)
        .placeholder("Ada")
        .on_submit(next)
        .name("first-name")
        .label("First name")
        .autofocus()
        .panel("First name")
        .error_when(move || first.with(|n| name_problem(n, tried.get()).is_some()))
        .fixed(3);
    let last_name = input(last)
        .placeholder("Lovelace")
        .on_submit(next)
        .name("last-name")
        .label("Last name")
        .panel("Last name")
        .error_when(move || last.with(|n| name_problem(n, tried.get()).is_some()))
        .fixed(3);
    let age_field = number_input(age, 0..=130)
        .bind("enter", "send the form", move |cx| {
            tried.set(true);
            if fields.problems().is_empty() {
                cx.quit_with(Person {
                    first_name: first.get_untracked().trim().to_string(),
                    last_name: last.get_untracked().trim().to_string(),
                    age: age.get_untracked(),
                });
            }
        })
        .name("age")
        .label("Age")
        .panel("Age")
        .error_when(move || age_problem(age.get(), tried.get()).is_some())
        .fixed(3);
    let problems = text(move || {
        let problems = fields.problems();
        if problems.is_empty() {
            "[dim]Tab or Enter moves to the next field · ↑/↓ or digits set the age\nEnter on the age sends the form · Esc quits[/]".into()
        } else {
            problems
                .iter()
                .map(|p| format!("[bad]✗ {p}[/]"))
                .collect::<Vec<_>>()
                .join("\n")
        }
    })
    .name("problems")
    .live();
    column([
        row([first_name.flex(1), last_name.flex(1)]).fixed(3),
        row([age_field.fixed(12), label("").flex(1)]).fixed(3),
        problems.flex(1),
    ])
    .name("form")
}

/// The Chat tab: the help line, the messages, the input.
fn chat(
    editing: Signal<bool>,
    message: Signal<String>,
    messages: Signal<Vec<String>>,
) -> (Node, NodeId) {
    let help = text(move || {
        if editing.get() {
            "Press [bold]Esc[/] to stop editing, [bold]Enter[/] to record the message".into()
        } else {
            "Press [bold]q[/] to quit, [bold]e[/] to start editing".into()
        }
    })
    .name("help")
    .fixed(1);
    let list = list_optional(
        move || {
            messages.with(|m| {
                m.iter()
                    .enumerate()
                    .map(|(i, m)| format!("{}: {}", i + 1, intuituive::rich::markup::escape(m)))
                    .collect()
            })
        },
        signal(None),
    )
    .no_focus()
    .name("messages")
    .label("Messages")
    .panel("Messages");
    let field = input(message)
        .on_submit(move |text, _| {
            if !text.trim().is_empty() {
                let text = text.to_string();
                messages.update(|m| m.push(text));
            }
            message.set(String::new());
        })
        .on_key("esc", move |_| editing.set(false))
        .disabled_when(move || !editing.get())
        .name("message")
        .label("Message");
    let id = field.id();
    // The panel's border lights up while editing: the focus is inside it.
    let field = field.panel("Input").fixed(3);
    (column([help, list.flex(1), field]).name("chat"), id)
}

/// The app. It ends with the [`Person`] the form sent, if it was sent.
pub fn form_app() -> App {
    App::new(|| {
        let tab = signal(0usize);
        let fields = Fields {
            first: signal(String::new()),
            last: signal(String::new()),
            age: signal(0i64),
            tried: signal(false),
        };
        let editing = signal(false);
        let message = signal(String::new());
        let messages = signal(Vec::<String>::new());
        let (chat, message_id) = chat(editing, message, messages);
        let chat = RefCell::new(Some(chat));
        let strip = tabs(|| TABS.iter().map(|t| t.to_string()).collect(), tab)
            .no_focus()
            .name("tabs")
            .label("Tabs")
            .fixed(1);
        let body = switch(
            move || tab.get(),
            move |page| match page {
                0 => form(fields),
                _ => chat.borrow_mut().take().unwrap_or_else(|| label("")),
            },
        );
        column([strip, body.flex(1)])
            .bind("f1", "the Form tab", move |_| tab.set(0))
            .bind("f2", "the Chat tab", move |_| tab.set(1))
            .bind("e", "start editing the message", move |cx| {
                if tab.get_untracked() == 1 {
                    editing.set(true);
                    cx.focus(message_id);
                }
            })
            .bind("esc", "quit", move |cx| {
                if tab.get_untracked() == 0 {
                    cx.quit()
                }
            })
            .bind("q", "quit", |cx| cx.quit())
    })
    .help_key("?")
    .palette_key("ctrl+p")
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    if let Some(person) = form_app().run_for::<Person>()? {
        println!("{person}");
    }
    Ok(())
}

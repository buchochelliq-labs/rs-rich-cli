//! The to-do app the tutorial builds (docs/guide/intuituive/tutorial.md).
//!
//!     cargo run -p rs-rich-intuituive --example todo
//!
//! Type a to-do and press Enter to add it. Tab moves to the list, where
//! Space ticks a to-do off and d deletes it (after asking). Ctrl+Q quits.

use intuituive::interact::Input;
use intuituive::prelude::*;
use intuituive::rich::markup::escape;

/// One to-do.
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    pub id: u32,
    pub title: String,
    pub done: bool,
}

impl Todo {
    pub fn new(id: u32, title: &str) -> Todo {
        Todo {
            id,
            title: title.to_string(),
            done: false,
        }
    }
}

/// The app, starting with `start`.
pub fn todo_app(start: Vec<Todo>) -> App {
    App::new(move || {
        let todos = signal(start);
        let left = memo(move || todos.with(|t| t.iter().filter(|t| !t.done).count()));

        let entry = repeating(
            || Input::new("Add"),
            move |title: String, _| {
                let title = title.trim().to_string();
                if !title.is_empty() {
                    todos.update(|t| {
                        let id = t.iter().map(|t| t.id).max().unwrap_or(0) + 1;
                        t.push(Todo {
                            id,
                            title,
                            done: false,
                        });
                    });
                }
            },
        );

        let list = each(
            move || todos.with(|t| t.iter().map(|t| t.id).collect()),
            move |id| todo_row(todos, id),
        );

        column([
            entry.panel("New").fixed(3),
            list.panel("To do"),
            text!("[muted]{left} left · space ticks · d deletes · ctrl+q quits").auto(),
        ])
        .on_key("ctrl+q", |cx| cx.quit())
    })
}

/// One row of the list: the to-do with `id`.
fn todo_row(todos: Signal<Vec<Todo>>, id: u32) -> Node {
    let todo = memo(move || todos.with(|t| t.iter().find(|t| t.id == id).cloned()));
    text(move || match todo.get() {
        Some(todo) if todo.done => format!("[good]✓[/] [muted strike]{}[/]", escape(&todo.title)),
        Some(todo) => format!("[accent]•[/] {}", escape(&todo.title)),
        None => String::new(),
    })
    .focus_style("reverse")
    .on_key("space", move |_| {
        todos.update(|t| {
            if let Some(todo) = t.iter_mut().find(|t| t.id == id) {
                todo.done = !todo.done;
            }
        })
    })
    .on_key("d", move |cx| {
        cx.modal(Size::Auto, Size::Auto, move || confirm_delete(todos, id))
    })
}

/// Ask before deleting the to-do with `id`.
fn confirm_delete(todos: Signal<Vec<Todo>>, id: u32) -> Node {
    let title = todos.with_untracked(|t| {
        t.iter()
            .find(|t| t.id == id)
            .map(|t| t.title.clone())
            .unwrap_or_default()
    });
    label(format!("Delete “{}”? [b]y[/] / [b]n[/]", escape(&title)))
        .padding(0, 1)
        .panel("Delete")
        .on_key("y", move |cx| {
            todos.update(|t| t.retain(|t| t.id != id));
            cx.pop();
        })
        .on_key("n esc", |cx| cx.pop())
}

#[allow(dead_code)]
fn main() -> std::io::Result<()> {
    todo_app(vec![
        Todo::new(1, "Read the intuiTUIve tutorial"),
        Todo::new(2, "Build an app"),
    ])
    .run()
}

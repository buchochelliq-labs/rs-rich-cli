//! intuiTUIve: a reactive, retained terminal UI framework built on rs-rich.
//!
//! You describe the screen once, as a tree of nodes, and keep your state in
//! [signals](signal). When a signal changes, the nodes that read it draw
//! again, and only the cells that changed are sent to the terminal. There
//! is no draw loop to write, no layout pass to call, and no "what changed"
//! to track.
//!
//! ```no_run
//! use intuituive::prelude::*;
//!
//! fn main() -> std::io::Result<()> {
//!     App::new(|| {
//!         let count = signal(0);
//!         column([
//!             text!("[b]Count:[/] {count}").panel("Counter"),
//!             label("[dim]+ adds one · q quits"),
//!         ])
//!         .on_key("+", move |_| count.update(|c| *c += 1))
//!         .on_key("q", |cx| cx.quit())
//!     })
//!     .run()
//! }
//! ```
//!
//! - **Nodes** ([`text`](fn@text), [`label`], [`renderable`], [`column`](fn@column), [`row`],
//!   [`grid`], [`each`], [`switch`], [`Node::panel`]) are built once and
//!   kept. Any rich renderable is a node: a `Table`, `Markdown`, `Syntax`, a
//!   chart.
//! - **Layout** sizes children by [`Size`] (fixed, percentage, flexible or
//!   their content), within minimums and maximums, with gaps and padding.
//! - **State** is [`signal`]s and [`memo`]s. Reading one while drawing
//!   subscribes the node; writing one redraws exactly its readers.
//! - **Input**: [`Node::on_key`] bindings bubble from the focused node up;
//!   Tab moves the focus; [`Node::on_click`] gets clicks, routed by the
//!   layout the last frame kept.
//! - **Screens**: [`Ctx::push`], [`Ctx::modal`] and [`Ctx::pop`] keep a
//!   stack of screens, each with its own state and focus.
//! - **Background work**: [`spawn`], [`spawn_future`] and [`resource`] run
//!   slow work off the app's thread and write signals with the result; a
//!   [`Proxy`] lets any thread do the same.
//! - **Themes**: [`Theme`] styles the framework's parts and names styles
//!   for markup; [`Ctx::set_theme`] switches at run time.
//! - **Inline**: [`App::inline`] runs in a few rows below the prompt
//!   instead of the alternate screen.
//! - **Tests**: [`App::render_with`] and [`App::run_on`] with
//!   `rich-interact`'s headless driver run an app without a terminal.
//!
//! The design, and how it compares with ratatui, is in
//! `docs/design/intuituive.md`.

pub mod a11y;
pub mod app;
mod builtin;
mod calendar;
pub mod inspect;
pub mod layout;
pub mod log;
pub mod menu;
pub mod node;
pub mod reactive;
pub mod screen;
mod sheet;
mod split;
pub mod task;
mod tree;
pub mod widget;
pub mod widgets;

/// rs-rich, for renderables, styles and markup helpers, so an app needs
/// only this crate as a dependency.
pub use rich;
/// rs-rich-interact, for its components (`interact::Input`,
/// `interact::Select`, …) and the headless test driver.
pub use rich_interact as interact;

pub use app::{every, Anchor, App, Ctx, Driver, Easing, FrameStats, Placement, Theme};
pub use layout::Size;
pub use log::Log;
pub use node::{
    column, component, each, grid, label, leaf, list, renderable, repeating, row, scroll,
    scroll_both, scroll_both_with, scroll_with, scroll_x, switch, text, Node,
};
pub use reactive::{memo, signal, watch, Memo, Proxy, Signal};
pub use sheet::{SheetError, Stylesheet};
pub use task::{resource, spawn, spawn_future, Load, Resource, Task};
pub use widget::{widget, Widget};

/// Everything an app usually needs.
pub mod prelude {
    // `text` is both the function and the `text!` macro.
    pub use crate::{
        column, component, each, every, grid, label, leaf, list, memo, renderable, repeating,
        resource, row, scroll, scroll_both, scroll_with, scroll_x, signal, spawn, switch, text,
        watch, widget, App, Ctx, Load, Log, Memo, Node, Proxy, Resource, Signal, Size, Task, Theme,
        Widget,
    };
}

/// Console markup with `format!` arguments, read again whenever a signal it
/// names changes: `text!("{count} items")`. Signals format as their value.
#[macro_export]
macro_rules! text {
    ($($arg:tt)*) => {
        $crate::node::text(move || format!($($arg)*))
    };
}

impl<T: std::fmt::Display + 'static> std::fmt::Display for Signal<T> {
    /// The value, subscribing the node that is drawing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|value| value.fmt(f))
    }
}

impl<T: std::fmt::Display + 'static> std::fmt::Display for Memo<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.with(|value| value.fmt(f))
    }
}

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
//! - **Nodes** ([`text`], [`label`], [`renderable`], [`column`], [`row`],
//!   [`each`], [`Node::panel`]) are built once and kept. Any rich renderable
//!   is a node: a `Table`, `Markdown`, `Syntax`, a chart.
//! - **State** is [`signal`]s and [`memo`]s. Reading one while drawing
//!   subscribes the node; writing one redraws exactly its readers.
//! - **Input**: [`Node::on_key`] bindings bubble from the focused node up;
//!   Tab moves the focus; [`Node::on_click`] gets clicks, routed by the
//!   layout the last frame kept.
//! - **Threads**: a [`Proxy`] runs a closure on the app's thread, where it
//!   can write signals.
//! - **Tests**: [`App::render_with`] and [`App::run_on`] with
//!   `rich-interact`'s headless driver run an app without a terminal.
//!
//! The design, and how it compares with ratatui, is in
//! `docs/design/intuituive.md`.

pub mod app;
pub mod log;
pub mod node;
pub mod reactive;
pub mod screen;

pub use app::{every, App, Ctx, FrameStats, Theme};
pub use log::Log;
pub use node::{column, each, label, leaf, renderable, row, text, Node, Size};
pub use reactive::{memo, signal, Memo, Proxy, Signal};

/// Everything an app usually needs.
pub mod prelude {
    // `text` is both the function and the `text!` macro.
    pub use crate::{
        column, each, every, label, leaf, memo, renderable, row, signal, text, App, Ctx, Log, Memo,
        Node, Proxy, Signal, Size, Theme,
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

//! Vim's keys for the text editors (the off-by-default `vim` feature):
//! [`Node::vim`] gives a [`text_area`](crate::text_area) or an
//! [`input`](crate::input) rs-rich-interact's [`Vim`] preset, a
//! [key mode](Node::key_mode) with normal, insert and visual modes,
//! motions, operators, counts and `.`.
//!
//! The caret is a block in normal mode and a bar while inserting.
//! [`mode_status`](Node::mode_status) writes the mode's name (`NORMAL`,
//! `INSERT`, `VISUAL`, `V-LINE`) and a command's keys so far to a signal
//! for a status bar. `:`, `/`, and keys the preset does not know, go on to
//! the app's bindings, so a command line or a [`search_bar`](crate::search_bar)
//! opens on them.
//!
//! ```
//! use intuituive::prelude::*;
//!
//! let app = App::new(|| {
//!     let status = signal(String::new());
//!     column([
//!         text_area(signal("one two".to_string())).vim().mode_status(status).autofocus(),
//!         text!("{status}").fixed(1),
//!     ])
//! });
//! let mut driver = app.driver(20, 2);
//! driver.type_text("wdwAyes");
//! driver.settle();
//! assert_eq!(driver.screen().plain()[0].trim_end(), "one yes");
//! assert_eq!(driver.screen().plain()[1].trim_end(), "INSERT");
//! driver.press("escape");
//! driver.settle();
//! assert_eq!(driver.screen().plain()[1].trim_end(), "NORMAL");
//! ```

pub use rich_interact::vim::{Vim, VimMode};

use crate::node::Node;

/// Vim's keys, on a [`text_area`](crate::text_area) or an
/// [`input`](crate::input). On any other node it does nothing.
impl Node {
    /// Edit with Vim's keys, starting in normal mode.
    pub fn vim(self) -> Node {
        self.key_mode(Vim::new())
    }

    /// Edit with Vim's keys, starting in insert mode: a field that is typed
    /// into first.
    pub fn vim_inserting(self) -> Node {
        self.key_mode(Vim::inserting())
    }
}

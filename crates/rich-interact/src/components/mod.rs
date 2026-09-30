//! Ready-made components (0.0.13 workstream 3), built from the crate's
//! primitives: [`Item`](crate::Item), [`Viewport`](crate::Viewport) and the
//! [fuzzy](crate::fuzzy) matcher. Each works under both drivers, degrades to
//! a line-based prompt without a terminal, and is styled by one [`Theme`].

mod asset;
mod color;
mod confirm;
mod file;
mod form;
mod input;
mod pager;
mod select;
mod textarea;
mod views;

pub use asset::{emoji, AssetKind, AssetPicker, BOX_STYLES};
pub use color::{ColorFormat, ColorPicker};
pub use confirm::{Choice, Confirm};
pub use file::{display_name, display_path, FileMode, FilePicker};
pub use form::{Answers, Form, Value};
pub use input::{input_keymap, Input, Provider, Suggestion};
pub use pager::Pager;
pub use select::{select_keymap, MultiSelect, PreviewLayout, Select};
pub use textarea::TextArea;
pub use views::{TableSelect, TreeSelect};

use rich::Style;

/// The styles and symbols every component draws with.
#[derive(Clone, Debug)]
pub struct Theme {
    /// The mark before a question (`?`).
    pub question: String,
    pub question_style: Style,
    pub prompt: Style,
    /// The focused row's pointer (`❯`).
    pub pointer: String,
    pub pointer_style: Style,
    pub focused: Style,
    /// Characters a filter matched.
    pub matched: Style,
    pub checked: String,
    pub unchecked: String,
    pub checked_style: Style,
    /// Hints, counts and descriptions.
    pub hint: Style,
    pub error: Style,
    /// A finished component's answer.
    pub answer: Style,
    pub border: Style,
}

fn style(definition: &str) -> Style {
    Style::parse(definition).expect("a built-in style")
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            question: "?".into(),
            question_style: style("bold green"),
            prompt: style("bold"),
            pointer: "❯".into(),
            pointer_style: style("bold magenta"),
            focused: style("bold"),
            matched: style("bold magenta"),
            checked: "◉".into(),
            unchecked: "○".into(),
            checked_style: style("green"),
            hint: style("dim"),
            error: style("red"),
            answer: style("cyan"),
            border: style("dim"),
        }
    }
}

// The line helpers moved to the public kit (0.0.14); the components use
// them from there.
pub(crate) use crate::kit::{fit, highlight, pad, pasted, plain, question, shown, text, width};

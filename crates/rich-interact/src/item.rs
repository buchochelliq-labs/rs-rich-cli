//! The universal item model (#452): one type behind every picker.
//!
//! File pickers, command palettes, plugin browsers and history search all
//! show the same thing: a value with a label, an optional description,
//! details, a preview and actions. [`Item`] carries it, so each selector
//! renders, searches and previews items the same way instead of inventing
//! its own row type.

use std::fmt;
use std::sync::Arc;

use rich::{Renderable, Text};

use crate::event::Key;

/// What a preview pane shows for an item.
#[derive(Clone)]
pub enum Preview {
    /// Plain text.
    Text(String),
    /// Console markup (`[bold]…[/]`).
    Markup(String),
    /// Any renderable: a `Syntax`, a `Table`, a `Panel`.
    Renderable(Arc<dyn Renderable + Send + Sync>),
}

impl Preview {
    /// The preview as something to render.
    pub fn renderable(&self) -> Arc<dyn Renderable + Send + Sync> {
        match self {
            Preview::Text(text) => Arc::new(Text::new(text.clone())),
            Preview::Markup(markup) => {
                Arc::new(Text::from_markup(markup).unwrap_or_else(|_| Text::new(markup.clone())))
            }
            Preview::Renderable(renderable) => Arc::clone(renderable),
        }
    }
}

impl fmt::Debug for Preview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Preview::Text(text) => f.debug_tuple("Text").field(text).finish(),
            Preview::Markup(markup) => f.debug_tuple("Markup").field(markup).finish(),
            Preview::Renderable(_) => f.write_str("Renderable(..)"),
        }
    }
}

/// Something that can be done to an item, bound to a key: "open in
/// $EDITOR" on `ctrl+e`, "delete" on `ctrl+d`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// A stable name the caller matches on.
    pub id: String,
    /// The label shown in a help line.
    pub label: String,
    pub key: Key,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>, key: Key) -> Action {
        Action {
            id: id.into(),
            label: label.into(),
            key,
        }
    }
}

/// One choosable thing: a value of any type, and how to show and find it.
#[derive(Clone, Debug)]
pub struct Item<T> {
    pub value: T,
    /// The main line.
    pub label: String,
    /// A dimmer second line or trailing note.
    pub description: Option<String>,
    /// Details shown as `key: value`, and searched.
    pub metadata: Vec<(String, String)>,
    pub preview: Option<Preview>,
    pub actions: Vec<Action>,
    /// Extra text that matches a search without being shown (aliases,
    /// keywords, a full path behind a short label).
    pub keywords: Vec<String>,
}

impl<T> Item<T> {
    pub fn new(value: T, label: impl Into<String>) -> Item<T> {
        Item {
            value,
            label: label.into(),
            description: None,
            metadata: Vec::new(),
            preview: None,
            actions: Vec::new(),
            keywords: Vec::new(),
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn meta(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.push((key.into(), value.into()));
        self
    }

    pub fn preview(mut self, preview: Preview) -> Self {
        self.preview = Some(preview);
        self
    }

    pub fn action(mut self, action: Action) -> Self {
        self.actions.push(action);
        self
    }

    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.push(keyword.into());
        self
    }

    /// Everything a search matches against, label first, one field per line.
    pub fn search_text(&self) -> String {
        let mut text = self.label.clone();
        for field in self
            .description
            .iter()
            .chain(self.metadata.iter().map(|(_, value)| value))
            .chain(&self.keywords)
        {
            text.push('\n');
            text.push_str(field);
        }
        text
    }

    /// The action bound to `key`, if any.
    pub fn action_for(&self, key: Key) -> Option<&Action> {
        self.actions.iter().find(|action| action.key == key)
    }

    /// The same item with its value mapped.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Item<U> {
        Item {
            value: f(self.value),
            label: self.label,
            description: self.description,
            metadata: self.metadata,
            preview: self.preview,
            actions: self.actions,
            keywords: self.keywords,
        }
    }
}

impl<T: fmt::Display> From<T> for Item<T> {
    /// An item labelled with its value's `Display`.
    fn from(value: T) -> Item<T> {
        let label = value.to_string();
        Item::new(value, label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_and_searches_items() {
        let item = Item::new(std::path::PathBuf::from("src/main.rs"), "main.rs")
            .description("the entry point")
            .meta("size", "2 KB")
            .keyword("src/main.rs")
            .preview(Preview::Text("fn main() {}".into()))
            .action(Action::new("edit", "Open in $EDITOR", Key::ctrl('e')));
        assert_eq!(
            item.search_text(),
            "main.rs\nthe entry point\n2 KB\nsrc/main.rs"
        );
        assert_eq!(item.action_for(Key::ctrl('e')).unwrap().id, "edit");
        assert!(item.action_for(Key::ctrl('d')).is_none());
        let named = item.map(|path| path.display().to_string());
        assert_eq!(named.value, "src/main.rs");
        assert_eq!(Item::from(42).label, "42");
    }
}

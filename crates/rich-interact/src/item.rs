//! The universal item model (#452): one type behind every picker.
//!
//! File pickers, command palettes, plugin browsers and history search all
//! show the same thing: a value with a label, an optional description,
//! details, a preview and actions. [`Item`] carries it, so each selector
//! renders, searches and previews items the same way instead of inventing
//! its own row type.
//!
//! Actions (#491) attach to one item, or to every target of a view through
//! [`Actions`]: list items, table rows, tree nodes and file entries alike,
//! told apart by an [`ActionTarget`]'s [`TargetKind`]. Plugins add actions
//! through `rs-rich-plugin-api`'s `CustomAction`, which
//! [`Actions::from_registry`] turns into these.

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
/// $EDITOR" on `ctrl+e`, "delete" on `ctrl+d`. Every action is also in the
/// view's action menu; one without a key is only there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// A stable name the caller matches on.
    pub id: String,
    /// The label shown in a help line and the action menu.
    pub label: String,
    pub key: Option<Key>,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>, key: Key) -> Action {
        Action {
            id: id.into(),
            label: label.into(),
            key: Some(key),
        }
    }

    /// An action with no key of its own: picked from the action menu.
    pub fn menu(id: impl Into<String>, label: impl Into<String>) -> Action {
        Action {
            id: id.into(),
            label: label.into(),
            key: None,
        }
    }
}

/// What kind of thing an action is done to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// An item of a list ([`Select`](crate::Select)).
    #[default]
    Item,
    /// A row of a table ([`TableSelect`](crate::TableSelect)).
    Row,
    /// A node of a tree ([`TreeSelect`](crate::TreeSelect)).
    Node,
    /// A file or directory ([`FilePicker`](crate::FilePicker)).
    File,
    /// A region of the screen rather than an item in it: a pane, a tab, or
    /// any component wrapped in [`Overlays`](crate::overlay::Overlays)
    /// with a region of its own (#474).
    Region,
}

impl TargetKind {
    /// `item`, `row`, `node`, `file` or `region`: the name plugins see.
    pub fn as_str(self) -> &'static str {
        match self {
            TargetKind::Item => "item",
            TargetKind::Row => "row",
            TargetKind::Node => "node",
            TargetKind::File => "file",
            TargetKind::Region => "region",
        }
    }

    pub fn parse(name: &str) -> Option<TargetKind> {
        Some(match name {
            "item" => TargetKind::Item,
            "row" => TargetKind::Row,
            "node" => TargetKind::Node,
            "file" => TargetKind::File,
            "region" => TargetKind::Region,
            _ => return None,
        })
    }
}

/// The thing an action is about to be done to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionTarget {
    pub kind: TargetKind,
    /// The item's index in the view's items.
    pub index: usize,
    /// Its label, as shown.
    pub label: String,
    /// What identifies it: an item's label, a row's cells joined by tabs, a
    /// node's path of labels joined by `/`, a file's path (with bytes that
    /// are not UTF-8 as `\xNN`).
    pub value: String,
}

/// Whether an action applies to a target.
pub type ActionFilter = Arc<dyn Fn(&ActionTarget) -> bool + Send + Sync>;

/// Actions a view offers on every target, beside each item's own: the
/// caller's, and those plugins registered. Each may be limited to some
/// targets.
#[derive(Clone, Default)]
pub struct Actions {
    entries: Vec<(Action, Option<ActionFilter>)>,
}

impl fmt::Debug for Actions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.entries.iter().map(|(action, _)| action))
            .finish()
    }
}

impl Actions {
    pub fn new() -> Actions {
        Actions::default()
    }

    /// Offer `action` on every target.
    pub fn action(mut self, action: Action) -> Actions {
        self.entries.push((action, None));
        self
    }

    /// Offer `action` on targets of one kind.
    pub fn action_for(self, kind: TargetKind, action: Action) -> Actions {
        self.action_if(action, move |target| target.kind == kind)
    }

    /// Offer `action` where `filter` says it applies.
    pub fn action_if(
        mut self,
        action: Action,
        filter: impl Fn(&ActionTarget) -> bool + Send + Sync + 'static,
    ) -> Actions {
        self.entries.push((action, Some(Arc::new(filter))));
        self
    }

    /// Every action from `other` after these.
    pub fn extend(mut self, other: Actions) -> Actions {
        self.entries.extend(other.entries);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// The actions for `target`: the item's `own` first, then these where
    /// they apply. An id already offered is not offered again.
    pub fn for_target(&self, target: &ActionTarget, own: &[Action]) -> Vec<Action> {
        let mut out: Vec<Action> = Vec::new();
        let applies = self
            .entries
            .iter()
            .filter(|(_, filter)| filter.as_ref().is_none_or(|filter| filter(target)));
        for action in own.iter().chain(applies.map(|(action, _)| action)) {
            if !out.iter().any(|known| known.id == action.id) {
                out.push(action.clone());
            }
        }
        out
    }

    /// The custom actions plugins registered in `registry`
    /// (`PluginRegistrar::action`): each under its registered name, on the
    /// targets it says it applies to. A key name that does not parse leaves
    /// the action in the menu only.
    pub fn from_registry(registry: &rich_ext::registry::ExtensionRegistry) -> Actions {
        let mut actions = Actions::new();
        for (name, _, action) in registry.actions() {
            let entry = Action {
                id: name.to_string(),
                label: action.label(),
                key: action.key().as_deref().and_then(Key::parse),
            };
            actions = actions.action_if(entry, move |target| {
                action.applies(target.kind.as_str(), &target.value)
            });
        }
        actions
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
        self.actions
            .iter()
            .find(|action| action.key.is_some_and(|k| key.matches(&k)))
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

    fn target(kind: TargetKind, value: &str) -> ActionTarget {
        ActionTarget {
            kind,
            index: 0,
            label: value.into(),
            value: value.into(),
        }
    }

    #[test]
    fn view_actions_apply_by_target() {
        let actions = Actions::new()
            .action(Action::menu("copy", "Copy"))
            .action_for(
                TargetKind::File,
                Action::new("edit", "Edit", Key::ctrl('e')),
            )
            .action_if(Action::menu("rust", "Rust only"), |target| {
                target.value.ends_with(".rs")
            });
        let own = [Action::new("open", "Open", Key::ctrl('o'))];
        let ids = |target: &ActionTarget| -> Vec<String> {
            actions
                .for_target(target, &own)
                .into_iter()
                .map(|action| action.id)
                .collect()
        };
        assert_eq!(
            ids(&target(TargetKind::File, "main.rs")),
            ["open", "copy", "edit", "rust"]
        );
        assert_eq!(ids(&target(TargetKind::Row, "a\tb")), ["open", "copy"]);
        // An item's own action wins over a view's of the same id.
        let shadow = [Action::menu("copy", "Copy path")];
        let found = actions.for_target(&target(TargetKind::Item, "x"), &shadow);
        assert_eq!(found[0].label, "Copy path");
        assert_eq!(found.len(), 1);
        assert_eq!(TargetKind::parse("node"), Some(TargetKind::Node));
        assert_eq!(TargetKind::Node.as_str(), "node");
    }

    #[test]
    fn plugin_actions_come_from_the_registry() {
        use rich_plugin_api::{CustomAction, Plugin, PluginError, PluginMetadata, PluginRegistrar};
        struct Reveal;
        impl CustomAction for Reveal {
            fn label(&self) -> String {
                "Reveal".into()
            }
            fn key(&self) -> Option<String> {
                Some("ctrl+r".into())
            }
            fn applies(&self, kind: &str, value: &str) -> bool {
                kind == "file" && !value.starts_with('.')
            }
        }
        struct Files;
        impl Plugin for Files {
            fn metadata(&self) -> PluginMetadata {
                PluginMetadata::new("files", "Files", "0.0.0")
            }
            fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
                registrar.action("reveal", Arc::new(Reveal));
                Ok(())
            }
        }
        let mut registry = rich_ext::registry::ExtensionRegistry::new();
        registry.add_plugin(&Files).unwrap();
        let actions = Actions::from_registry(&registry);
        let found = actions.for_target(&target(TargetKind::File, "a.txt"), &[]);
        assert_eq!(found, [Action::new("reveal", "Reveal", Key::ctrl('r'))]);
        assert!(actions
            .for_target(&target(TargetKind::File, ".env"), &[])
            .is_empty());
        assert!(actions
            .for_target(&target(TargetKind::Item, "a.txt"), &[])
            .is_empty());
    }
}

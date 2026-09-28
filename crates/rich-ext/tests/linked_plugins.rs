//! Compile-time plugins: `export_plugin!` in this test binary, collected at
//! link time and added in a fixed order, sorted by plugin id.

use rich_ext::plugin::{
    linked_plugins, Capability, Plugin, PluginError, PluginMetadata, PluginRegistrar,
};
use rich_ext::ExtensionRegistry;

struct Named(&'static str);

impl Plugin for Named {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new(self.0, self.0, "1.0.0")
    }
    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.theme(&format!("{}-theme", self.0), rich::Theme::new());
        Ok(())
    }
}

// Submitted out of order, from two modules.
rich_ext::plugin::export_plugin!(Named("zeta"));
mod elsewhere {
    rich_ext::plugin::export_plugin!(super::Named("alpha"));
}

#[test]
fn linked_plugins_are_collected_and_added_sorted_by_id() {
    let mut ids: Vec<String> = linked_plugins().map(|p| p.plugin().metadata().id).collect();
    ids.sort();
    assert_eq!(ids, ["alpha", "zeta"]);
    assert!(linked_plugins().any(|p| p.module().ends_with("elsewhere")));

    let registry = ExtensionRegistry::with_linked_plugins().unwrap();
    let order: Vec<&str> = registry
        .plugins()
        .iter()
        .map(|p| p.metadata.id.as_str())
        .collect();
    assert_eq!(order, ["rich-ext", "alpha", "zeta"]);
    assert_eq!(
        registry.provided_by(&Capability::Theme("zeta-theme".into())),
        Some("zeta")
    );
}

#[test]
fn a_linked_plugin_already_added_by_hand_is_a_duplicate() {
    let mut registry = ExtensionRegistry::new();
    registry.add_plugin(&Named("alpha")).unwrap();
    assert_eq!(
        registry.add_linked_plugins(),
        Err(PluginError::DuplicatePlugin { id: "alpha".into() })
    );
}

#[test]
fn a_plugin_set_with_a_duplicate_id_adds_nothing() {
    let mut registry = ExtensionRegistry::new();
    let set: Vec<Box<dyn Plugin>> = vec![
        Box::new(Named("b")),
        Box::new(Named("a")),
        Box::new(Named("b")),
    ];
    assert_eq!(
        registry.add_plugin_set(set),
        Err(PluginError::DuplicatePlugin { id: "b".into() })
    );
    assert!(registry.plugins().is_empty());
    let set: Vec<Box<dyn Plugin>> = vec![Box::new(Named("b")), Box::new(Named("a"))];
    assert_eq!(registry.add_plugin_set(set).unwrap(), ["a", "b"]);
}

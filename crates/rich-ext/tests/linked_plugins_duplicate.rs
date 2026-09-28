//! Two linked plugins with one id: `with_linked_plugins` refuses them both,
//! whatever order the linker put them in. (Its own test binary, since every
//! test in a binary sees the same linked plugins.)

use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
use rich_ext::ExtensionRegistry;

struct Same;

impl Plugin for Same {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("same", "Same", "1.0.0")
    }
    fn register(&self, _: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        Ok(())
    }
}

rich_ext::plugin::export_plugin!(Same);
rich_ext::plugin::export_plugin!(Same);

#[test]
fn duplicate_linked_ids_are_an_error() {
    let error = ExtensionRegistry::with_linked_plugins().err().unwrap();
    assert_eq!(error, PluginError::DuplicatePlugin { id: "same".into() });
    let mut registry = ExtensionRegistry::new();
    assert!(registry.add_linked_plugins().is_err());
    assert!(registry.plugins().is_empty());
}

//! `export_plugin!` as a third-party crate uses it: the plugin is listed by
//! `linked_plugins`, and each call makes a fresh instance.

use rich_plugin_api::{linked_plugins, Plugin, PluginError, PluginMetadata, PluginRegistrar};

struct Greeter {
    greeting: &'static str,
}

impl Plugin for Greeter {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("greeter", self.greeting, "1.0.0")
    }
    fn register(&self, _: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        Ok(())
    }
}

rich_plugin_api::export_plugin!(Greeter { greeting: "hello" });

#[test]
fn an_exported_plugin_is_linked() {
    let linked: Vec<_> = linked_plugins().collect();
    assert_eq!(linked.len(), 1);
    assert_eq!(linked[0].plugin().metadata().name, "hello");
    assert_eq!(linked[0].module(), "linked");
}

//! Compile-time plugins: registered with [`export_plugin!`](crate::export_plugin)
//! and collected at link time.
//!
//! A plugin crate writes `rich_plugin_api::export_plugin!(MyPlugin);` once.
//! Every such plugin in the final binary is listed by [`linked_plugins`], and
//! a host (`rs-rich-ext`'s `ExtensionRegistry::with_linked_plugins`) adds them
//! in a fixed order: sorted by plugin id, with a duplicate id an error.
//!
//! Collection uses [`inventory`], which needs no proc macro and works on every
//! platform `rich` supports. One caveat, shared by every link-time scheme: a
//! crate the binary depends on but never names may be dropped by the linker.
//! Name it once (`use my_plugin as _;`) to keep its plugins.

use crate::Plugin;

/// One plugin submitted with [`export_plugin!`](crate::export_plugin).
pub struct LinkedPlugin {
    make: fn() -> Box<dyn Plugin>,
    module: &'static str,
}

impl LinkedPlugin {
    /// Used by [`export_plugin!`](crate::export_plugin); not called directly.
    #[doc(hidden)]
    pub const fn new(make: fn() -> Box<dyn Plugin>, module: &'static str) -> Self {
        LinkedPlugin { make, module }
    }

    /// A fresh instance of the plugin.
    pub fn plugin(&self) -> Box<dyn Plugin> {
        (self.make)()
    }

    /// The module that exported it (`module_path!()`), for error messages.
    pub fn module(&self) -> &'static str {
        self.module
    }
}

inventory::collect!(LinkedPlugin);

/// Every plugin exported with [`export_plugin!`](crate::export_plugin) in this
/// binary, in no particular order (a host sorts them by id).
pub fn linked_plugins() -> impl Iterator<Item = &'static LinkedPlugin> {
    inventory::iter::<LinkedPlugin>.into_iter()
}

#[doc(hidden)]
pub mod __private {
    pub use inventory;
}

/// Register a plugin for link-time collection.
///
/// The argument is an expression that makes the plugin, evaluated each time
/// a host asks for it. It must not capture anything:
///
/// ```
/// use rich_plugin_api::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
///
/// struct Hello;
/// impl Plugin for Hello {
///     fn metadata(&self) -> PluginMetadata {
///         PluginMetadata::new("hello", "Hello", "1.0.0")
///     }
///     fn register(&self, _: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
///         Ok(())
///     }
/// }
///
/// rich_plugin_api::export_plugin!(Hello);
///
/// # fn main() {
/// assert!(rich_plugin_api::linked_plugins().any(|p| p.plugin().metadata().id == "hello"));
/// # }
/// ```
#[macro_export]
macro_rules! export_plugin {
    ($plugin:expr) => {
        $crate::__private::inventory::submit! {
            $crate::LinkedPlugin::new(
                || -> ::std::boxed::Box<dyn $crate::Plugin> { ::std::boxed::Box::new($plugin) },
                ::core::module_path!(),
            )
        }
    };
}

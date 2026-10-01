//! Components from plugins (0.0.14 workstream 4): a
//! [`PluginComponent`](rich_ext::plugin::PluginComponent) that a plugin
//! registered by name with `PluginRegistrar::component`, mounted as a
//! [`Component`] beside the built-ins.
//!
//! A plugin depends on `rs-rich-plugin-api` and core only, so its components
//! speak that crate's small contract: keys by name, text answers. A
//! [`PluginView`] translates both ways. Its output is the text the plugin
//! answers with; [`map`](crate::ComponentExt::map) it into a container's
//! output like any other child. Its keys are listed under the registered
//! name, so a help overlay shows them and configuration rebinds them
//! (`counter.up = k`).
//!
//! A plugin component that panics does not take the app down: the view
//! shows what went wrong, stops taking focus and ignores events.
//!
//! ```
//! use std::sync::Arc;
//!
//! use rich_ext::plugin::component::{
//!     ComponentContext, ComponentEvent, ComponentFlow, ComponentView, PluginComponent,
//! };
//! use rich_ext::plugin::{Plugin, PluginError, PluginMetadata, PluginRegistrar};
//! use rich_ext::registry::ExtensionRegistry;
//! use rich_interact::compose::{ComponentExt, Split};
//! use rich_interact::plugin::PluginView;
//! use rich_interact::{headless, Flow, Outcome, Select};
//!
//! #[derive(Default)]
//! struct Counter(u32);
//!
//! impl PluginComponent for Counter {
//!     fn handle(&mut self, event: &ComponentEvent, _: &ComponentContext<'_>) -> ComponentFlow {
//!         match event.key() {
//!             Some("up") => self.0 += 1,
//!             Some("enter") => return ComponentFlow::Done(self.0.to_string()),
//!             _ => return ComponentFlow::Ignored,
//!         }
//!         ComponentFlow::Continue
//!     }
//!     fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
//!         ComponentView::new(context.markup(&format!("count {}", self.0)))
//!     }
//! }
//!
//! struct Counters;
//!
//! impl Plugin for Counters {
//!     fn metadata(&self) -> PluginMetadata {
//!         PluginMetadata::new("counters", "Counters", "1.0.0")
//!     }
//!     fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
//!         registrar.component("counter", Arc::new(|| Box::new(Counter::default())));
//!         Ok(())
//!     }
//! }
//!
//! let mut registry = ExtensionRegistry::new();
//! registry.add_plugin(&Counters).unwrap();
//! let counter = PluginView::mount(&registry, "counter").unwrap();
//! let app = Split::horizontal(
//!     counter.map(Flow::Done),
//!     Select::new("Pick", ["a", "b"]).map(|pick| Flow::Done(pick.to_string())),
//! );
//! let script = headless::Script::new().keys("up up enter");
//! let (outcome, _) = headless::run(app, script, 40, 4);
//! assert_eq!(outcome.unwrap(), Outcome::Done("2".to_string()));
//! ```

use std::fmt;
use std::panic::AssertUnwindSafe;
use std::time::Duration;

use rich::Style;
use rich_ext::plugin::component::{
    ComponentBinding, ComponentContext, ComponentEvent, ComponentFlow,
};
use rich_ext::plugin::PluginComponent;
use rich_ext::registry::ExtensionRegistry;

use crate::component::{Component, Context, Flow, View};
use crate::event::{Event, Key, MouseKind};
use crate::keymap::{Binding, Keymap};
use crate::session::catch_panic;

/// No component is registered under a name, or its factory failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownComponent {
    /// The name asked for.
    pub name: String,
    /// Every name that is registered, sorted.
    pub available: Vec<String>,
}

impl fmt::Display for UnknownComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "no plugin component {:?}", self.name)?;
        if self.available.is_empty() {
            write!(f, "; no plugin registered one")
        } else {
            write!(f, "; available: {}", self.available.join(", "))
        }
    }
}

impl std::error::Error for UnknownComponent {}

/// A plugin's component as a [`Component`] whose answer is the plugin's
/// text. See the [module](self).
pub struct PluginView {
    name: String,
    inner: Box<dyn PluginComponent>,
    /// The panic that stopped it, shown instead of its view.
    failed: Option<String>,
    /// Keys rebound on this view, over the plugin's own.
    keymap: Keymap,
}

impl PluginView {
    /// Wrap `component`, listing its keys under `name`.
    pub fn new(name: impl Into<String>, component: Box<dyn PluginComponent>) -> PluginView {
        let name = name.into();
        PluginView {
            keymap: Keymap::new(name.clone()),
            name,
            inner: component,
            failed: None,
        }
    }

    /// A fresh instance of the component registered as `name` in
    /// `registry`.
    pub fn mount(registry: &ExtensionRegistry, name: &str) -> Result<PluginView, UnknownComponent> {
        registry
            .create_component(name)
            .map(|component| PluginView::new(name, component))
            .ok_or_else(|| UnknownComponent {
                name: name.to_string(),
                available: registry
                    .component_names()
                    .into_iter()
                    .map(|(name, _)| name.to_string())
                    .collect(),
            })
    }

    /// The name its keys are listed under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Make `keys` do the plugin's `action` instead of the keys it declared.
    pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
        self.keymap.rebind(action, keys);
        self
    }

    /// Why it stopped, if the plugin panicked.
    pub fn failure(&self) -> Option<&str> {
        self.failed.as_deref()
    }

    /// The bindings the plugin declares, as it declares them.
    fn declared(&self) -> Vec<ComponentBinding> {
        catch_panic(AssertUnwindSafe(|| self.inner.bindings())).unwrap_or_default()
    }

    /// The plugin's bindings, with this view's and the installed rebinds.
    fn bindings(&self) -> Keymap {
        self.keymap_of(self.declared())
    }

    fn keymap_of(&self, declared: Vec<ComponentBinding>) -> Keymap {
        let mut keymap = self.keymap.clone();
        for binding in declared {
            let keys = binding.keys.iter().filter_map(|name| Key::parse(name));
            keymap.add(Binding::new(
                self.name.clone(),
                binding.action,
                keys,
                binding.description,
            ));
        }
        keymap
    }

    /// Run `f` on the plugin, catching a panic.
    fn guard<T>(&mut self, f: impl FnOnce(&mut dyn PluginComponent) -> T) -> Option<T> {
        if self.failed.is_some() {
            return None;
        }
        match catch_panic(AssertUnwindSafe(|| f(&mut *self.inner))) {
            Ok(value) => Some(value),
            Err(panic) => {
                self.failed = Some(panic_message(&*panic));
                None
            }
        }
    }
}

/// The name of `key` for the plugin, through `keymap` (the plugin's
/// bindings with every rebind): a key that now does one of the plugin's
/// actions arrives as the first key the plugin declared for it, spelled as
/// the plugin spelled it (`esc`, `shift+tab`), so a rebound key does what
/// the plugin knows; a declared key rebound away from its action does not
/// arrive at all (`None`); any other key as itself.
fn translate(keymap: &Keymap, declared: &[ComponentBinding], key: Key) -> Option<String> {
    if let Some(action) = keymap.action(key) {
        let name = declared
            .iter()
            .find(|binding| binding.action == action)
            .and_then(|binding| binding.keys.iter().find(|name| Key::parse(name).is_some()));
        return Some(name.cloned().unwrap_or_else(|| key.to_string()));
    }
    if keymap
        .declared()
        .iter()
        .any(|binding| binding.keys.contains(&key))
    {
        return None;
    }
    Some(key.to_string())
}

fn event(value: &Event) -> Option<ComponentEvent> {
    Some(match value {
        Event::Key(key) => ComponentEvent::Key(key.to_string()),
        Event::Paste(text) => ComponentEvent::Paste(text.clone()),
        Event::Resize { columns, rows } => ComponentEvent::Resize {
            columns: *columns,
            rows: *rows,
        },
        Event::Tick => ComponentEvent::Tick,
        Event::Mouse(mouse) => ComponentEvent::Mouse {
            kind: match mouse.kind {
                MouseKind::Down(_) => "down",
                MouseKind::Up(_) => "up",
                MouseKind::Drag(_) => "drag",
                MouseKind::Moved => "moved",
                MouseKind::ScrollUp => "scroll_up",
                MouseKind::ScrollDown => "scroll_down",
            }
            .to_string(),
            column: mouse.column,
            row: mouse.row,
        },
        Event::Link(url) => ComponentEvent::Link(url.clone()),
        // A plugin component cannot hand the terminal off.
        Event::Returned(_) => return None,
    })
}

impl Component for PluginView {
    type Output = String;

    fn handle(&mut self, value: &Event, context: &Context<'_>) -> Flow<String> {
        let event = match value {
            Event::Key(key) => {
                let declared = self.declared();
                match translate(&self.keymap_of(declared.clone()), &declared, *key) {
                    Some(name) => ComponentEvent::Key(name),
                    None => return Flow::Ignored,
                }
            }
            other => match event(other) {
                Some(event) => event,
                None => return Flow::Ignored,
            },
        };
        let inner = ComponentContext::new(context.console, context.width, context.height);
        match self.guard(|component| component.handle(&event, &inner)) {
            Some(ComponentFlow::Continue) => Flow::Continue,
            Some(ComponentFlow::Done(answer)) => Flow::Done(answer),
            Some(ComponentFlow::Cancel) => Flow::Cancel,
            _ => Flow::Ignored,
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        if let Some(message) = &self.failed {
            return self.failure_view(message, context);
        }
        let inner = ComponentContext::new(context.console, context.width, context.height);
        match catch_panic(AssertUnwindSafe(|| self.inner.render(&inner))) {
            Ok(view) => View {
                lines: view.lines,
                cursor: view.cursor,
            },
            // Rendering takes `&self`, so the panic cannot be kept: the
            // next event finds it again, or finds the plugin recovered.
            Err(panic) => self.failure_view(&panic_message(&*panic), context),
        }
    }

    fn tick(&self) -> Option<Duration> {
        if self.failed.is_some() {
            return None;
        }
        catch_panic(AssertUnwindSafe(|| self.inner.tick())).unwrap_or(None)
    }

    fn mouse(&self) -> bool {
        self.failed.is_none()
            && catch_panic(AssertUnwindSafe(|| self.inner.mouse())).unwrap_or(false)
    }

    fn keymap(&self) -> Keymap {
        if self.failed.is_some() {
            return Keymap::default();
        }
        self.bindings()
    }

    fn focusable(&self) -> bool {
        self.failed.is_none()
            && catch_panic(AssertUnwindSafe(|| self.inner.focusable())).unwrap_or(false)
    }
}

impl PluginView {
    fn failure_view(&self, message: &str, context: &Context<'_>) -> View {
        let message = format!("plugin component {} failed: {message}", self.name);
        let mut text = rich::Text::new(&message);
        let length = text.plain().chars().count();
        text.stylize(Style::parse("red").unwrap_or_default(), 0, length);
        View::new(context.lines(&text))
    }
}

/// The text of a caught panic.
fn panic_message(panic: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = panic.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = panic.downcast_ref::<String>() {
        message.clone()
    } else {
        "a panic with no message".to_string()
    }
}

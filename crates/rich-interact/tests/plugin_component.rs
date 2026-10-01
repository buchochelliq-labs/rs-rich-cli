//! A component a plugin registers by name (`PluginRegistrar::component`),
//! linked in with `export_plugin!`, mounted with `PluginView` and composed
//! with built-ins in a split (0.0.14 workstream 4).

use std::sync::Arc;

use rich_ext::registry::ExtensionRegistry;
use rich_interact::compose::{ComponentExt, Split};
use rich_interact::headless::{self, Script};
use rich_interact::keymap::keys;
use rich_interact::plugin::PluginView;
use rich_interact::{Component, Flow, Outcome, Select};
use rich_plugin_api::component::{
    ComponentBinding, ComponentContext, ComponentEvent, ComponentFlow, ComponentView,
};
use rich_plugin_api::{Plugin, PluginComponent, PluginError, PluginMetadata, PluginRegistrar};

/// Counts Up presses; Enter answers with the count; typed text is echoed.
#[derive(Default)]
struct Counter {
    count: u32,
    typed: String,
}

impl PluginComponent for Counter {
    fn handle(&mut self, event: &ComponentEvent, _: &ComponentContext<'_>) -> ComponentFlow {
        match event {
            ComponentEvent::Key(key) if key == "up" => self.count += 1,
            ComponentEvent::Key(key) if key == "enter" => {
                return ComponentFlow::Done(self.count.to_string())
            }
            ComponentEvent::Key(key) if key == "escape" => return ComponentFlow::Cancel,
            ComponentEvent::Paste(text) => self.typed.push_str(text),
            _ => return ComponentFlow::Ignored,
        }
        ComponentFlow::Continue
    }

    fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
        ComponentView::new(context.markup(&format!("count [bold]{}[/] {}", self.count, self.typed)))
    }

    fn bindings(&self) -> Vec<ComponentBinding> {
        vec![
            ComponentBinding::new("up", ["up"], "count up"),
            ComponentBinding::new("done", ["enter"], "answer"),
        ]
    }
}

/// Panics on its first key.
struct Broken;

impl PluginComponent for Broken {
    fn handle(&mut self, _: &ComponentEvent, _: &ComponentContext<'_>) -> ComponentFlow {
        panic!("broken on purpose")
    }

    fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
        ComponentView::new(context.markup("broken"))
    }
}

struct Views;

impl Plugin for Views {
    fn metadata(&self) -> PluginMetadata {
        PluginMetadata::new("test-views", "Test views", "0.0.0")
    }

    fn register(&self, registrar: &mut dyn PluginRegistrar) -> Result<(), PluginError> {
        registrar.component("counter", Arc::new(|| Box::new(Counter::default())));
        registrar.component("broken", Arc::new(|| Box::new(Broken)));
        Ok(())
    }
}

rich_plugin_api::export_plugin!(Views);

/// A registry with the linked plugins, as an app or the CLI builds one.
fn registry() -> ExtensionRegistry {
    let mut registry = ExtensionRegistry::new();
    registry.add_linked_plugins().unwrap();
    registry
}

#[derive(Debug, PartialEq, Eq)]
enum App {
    Count(String),
    File(String),
}

/// The plugin's counter beside a built-in `Select`.
fn app(registry: &ExtensionRegistry) -> impl Component<Output = App> {
    let counter = PluginView::mount(registry, "counter").unwrap();
    Split::horizontal(
        counter.map(|count| Flow::Done(App::Count(count))),
        Select::new("File", ["a.rs", "b.rs"]).map(|file| Flow::Done(App::File(file.to_string()))),
    )
}

#[test]
fn a_linked_plugin_component_composes_with_a_built_in_in_a_split() {
    let registry = registry();
    assert_eq!(
        registry.component_names(),
        [("broken", "test-views"), ("counter", "test-views")]
    );

    // Keys go to the focused pane, the plugin's, first.
    let (outcome, record) = headless::run(app(&registry), Script::new().keys("up up enter"), 50, 5);
    assert_eq!(outcome.unwrap(), Outcome::Done(App::Count("2".into())));
    let frame = &record.frames[record.frames.len() - 1];
    assert!(frame.contains("count 2"), "{frame}");
    assert!(frame.contains("File"), "{frame}");

    // Tab bubbles up from the plugin (it ignores it) and moves focus to the
    // built-in, which answers.
    let script = Script::new().keys("up tab down enter");
    let (outcome, record) = headless::run(app(&registry), script, 50, 5);
    assert_eq!(outcome.unwrap(), Outcome::Done(App::File("b.rs".into())));
    assert!(record.frames.iter().any(|frame| frame.contains("count 1")));

    // A paste reaches the plugin too.
    let script = Script::new()
        .event(rich_interact::Event::Paste("hi".into()))
        .keys("enter");
    let (_, record) = headless::run(app(&registry), script, 50, 5);
    assert!(record
        .frames
        .iter()
        .any(|frame| frame.contains("count 0 hi")));
}

#[test]
fn a_plugin_components_keys_are_listed_and_rebindable_under_its_name() {
    let registry = registry();
    let bindings = app(&registry).keymap().bindings();
    let up = bindings
        .iter()
        .find(|binding| binding.id() == "counter.up")
        .expect("the plugin's binding is listed");
    assert_eq!(up.description, "count up");
    // The focused child's bindings come first, then the split's own.
    assert_eq!(bindings[0].context, "counter");
    assert!(bindings.iter().any(|binding| binding.context == "split"));

    // Rebound, `k` counts and `up` no longer does.
    let counter = PluginView::mount(&registry, "counter")
        .unwrap()
        .rebind("up", keys("k"));
    let (outcome, _) = headless::run(counter, Script::new().keys("k k up enter"), 30, 3);
    assert_eq!(outcome.unwrap(), Outcome::Done("2".into()));
}

#[test]
fn a_plugin_component_that_panics_does_not_take_the_app_down() {
    let registry = registry();
    let broken = PluginView::mount(&registry, "broken").unwrap();
    let app = Split::horizontal(
        broken.map(Flow::Done),
        Select::new("File", ["a.rs", "b.rs"]).map(|file| Flow::Done(file.to_string())),
    );
    // The first key panics in the plugin: its pane says so, and stops
    // taking focus, so the next keys reach the built-in.
    let (outcome, record) = headless::run(app, Script::new().keys("x down enter"), 70, 4);
    assert_eq!(outcome.unwrap(), Outcome::Done("b.rs".to_string()));
    assert!(
        record
            .frames
            .iter()
            .any(|frame| frame.contains("plugin component broken")),
        "{:?}",
        record.frames
    );
}

#[test]
fn an_unknown_component_names_the_ones_there_are() {
    let error = PluginView::mount(&registry(), "nope").err().unwrap();
    assert_eq!(error.available, ["broken", "counter"]);
    assert!(error.to_string().contains("available: broken, counter"));
}

/// Records every key name it is sent; Enter answers.
struct Names(Arc<std::sync::Mutex<Vec<String>>>, Vec<ComponentBinding>);

impl PluginComponent for Names {
    fn handle(&mut self, event: &ComponentEvent, _: &ComponentContext<'_>) -> ComponentFlow {
        let Some(key) = event.key() else {
            return ComponentFlow::Ignored;
        };
        self.0.lock().unwrap().push(key.to_string());
        if key == "enter" {
            return ComponentFlow::Done(String::new());
        }
        ComponentFlow::Continue
    }

    fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
        ComponentView::new(context.markup("names"))
    }

    fn bindings(&self) -> Vec<ComponentBinding> {
        self.1.clone()
    }
}

/// A key bound to one of the plugin's actions arrives as the name the
/// plugin declared for it, alias and all, and Shift+Tab is `shift+tab`
/// as `ComponentEvent::Key` documents (0.0.14 release-test audit B5):
/// they arrived as `backtab` and `escape`.
#[test]
fn a_plugin_sees_the_key_names_it_declared() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let plugin = Names(
        Arc::clone(&seen),
        vec![
            ComponentBinding::new("back", ["shift+tab"], "back"),
            ComponentBinding::new("quit", ["esc", "q"], "quit"),
            ComponentBinding::new("cut", ["Control+X"], "cut"),
        ],
    );
    let view = PluginView::new("names", Box::new(plugin));
    let script = Script::new().keys("shift+tab escape q ctrl+x ctrl+shift+left shift+tab enter");
    let _ = headless::run(view, script, 40, 5);
    assert_eq!(
        *seen.lock().unwrap(),
        [
            "shift+tab",
            "esc",
            "esc",
            "Control+X",
            "ctrl+shift+left",
            "shift+tab",
            "enter"
        ]
    );

    // Undeclared, Shift+Tab is still `shift+tab`.
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let view = PluginView::new("names", Box::new(Names(Arc::clone(&seen), Vec::new())));
    let _ = headless::run(
        view,
        Script::new().keys("shift+tab ctrl+shift+tab enter"),
        40,
        5,
    );
    assert_eq!(
        *seen.lock().unwrap(),
        ["shift+tab", "ctrl+shift+tab", "enter"]
    );
}

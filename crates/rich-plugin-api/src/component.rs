//! Interactive components a plugin registers by name (0.0.14), with
//! [`PluginRegistrar::component`](crate::PluginRegistrar::component).
//!
//! A plugin depends on this crate and core only, never on
//! `rs-rich-interact`, so the contract here is its own small one: events
//! arrive as [`ComponentEvent`]s, with keys by name (`down`, `ctrl+k`); a
//! component answers with a [`ComponentFlow`] and renders a
//! [`ComponentView`] of core [`Segment`]s. A host adapts it to its own
//! component model: `rs-rich-interact`'s `PluginView` mounts one in a split,
//! tabs or a modal beside the built-ins, and runs it through the same
//! drivers, headless included.
//!
//! A component finishes with text ([`ComponentFlow::Done`]): whatever it
//! chose or typed, for the app that mounted it to interpret.
//!
//! ```
//! use rich_plugin_api::component::{
//!     ComponentContext, ComponentEvent, ComponentFlow, ComponentView, PluginComponent,
//! };
//!
//! /// Counts Up presses; Enter answers with the count.
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
//!
//!     fn render(&self, context: &ComponentContext<'_>) -> ComponentView {
//!         ComponentView::new(context.markup(&format!("count: [bold]{}[/]", self.0)))
//!     }
//! }
//!
//! let console = rich::Console::new();
//! let context = ComponentContext::new(&console, 20, 1);
//! let mut counter = Counter::default();
//! counter.handle(&ComponentEvent::Key("up".into()), &context);
//! assert!(matches!(
//!     counter.handle(&ComponentEvent::Key("enter".into()), &context),
//!     ComponentFlow::Done(count) if count == "1"
//! ));
//! ```

use std::sync::Arc;
use std::time::Duration;

use rich::{Console, Renderable, Segment, Text};

/// Makes a fresh component each time a host mounts one, so one
/// registration can be mounted many times.
pub type ComponentFactory = Arc<dyn Fn() -> Box<dyn PluginComponent> + Send + Sync>;

/// An event for a [`PluginComponent`].
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComponentEvent {
    /// A key, by name: `a`, `A`, `enter`, `down`, `space`, `tab`,
    /// `backspace`, `f5`, with `ctrl+`, `alt+` and `shift+` before it, in
    /// that order (`ctrl+shift+left`).
    Key(String),
    /// Text pasted at once.
    Paste(String),
    /// The terminal is now this size.
    Resize { columns: u16, rows: u16 },
    /// The component's [`tick`](PluginComponent::tick) interval passed.
    Tick,
    /// The mouse, in the component's own view (0-based), when it asked for
    /// it with [`mouse`](PluginComponent::mouse).
    Mouse {
        /// `down`, `up`, `drag`, `moved`, `scroll_up` or `scroll_down`.
        kind: String,
        column: u16,
        row: u16,
    },
    /// A click on a hyperlink in the component's view: its URL.
    Link(String),
}

impl ComponentEvent {
    /// The key's name, when this is a key.
    pub fn key(&self) -> Option<&str> {
        match self {
            ComponentEvent::Key(name) => Some(name),
            _ => None,
        }
    }
}

/// What a component wants after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComponentFlow {
    /// Keep going; the view is rendered again.
    Continue,
    /// Finished, with an answer for the app that mounted it.
    Done(String),
    /// The user backed out.
    Cancel,
    /// The event was not for this component: the container it is in may
    /// use it (Tab to move focus, a binding of its own).
    Ignored,
}

/// A rendered view: lines of segments, and where the terminal cursor goes
/// (a text caret), or `None` to hide it.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ComponentView {
    pub lines: Vec<Vec<Segment>>,
    /// (row, column) within the view.
    pub cursor: Option<(usize, usize)>,
}

impl ComponentView {
    pub fn new(lines: Vec<Vec<Segment>>) -> ComponentView {
        ComponentView {
            lines,
            cursor: None,
        }
    }

    /// Put the cursor at `row`, `column` of the view.
    pub fn with_cursor(mut self, row: usize, column: usize) -> ComponentView {
        self.cursor = Some((row, column));
        self
    }
}

/// What a component renders and handles events for: the console to render
/// with, and the space it has.
#[non_exhaustive]
pub struct ComponentContext<'a> {
    pub console: &'a Console,
    /// Columns available.
    pub width: usize,
    /// Rows available; a taller view is cut off at the bottom.
    pub height: usize,
}

impl<'a> ComponentContext<'a> {
    pub fn new(console: &'a Console, width: usize, height: usize) -> ComponentContext<'a> {
        ComponentContext {
            console,
            width,
            height,
        }
    }

    /// Render `renderable` at the context's width into lines.
    pub fn lines(&self, renderable: &dyn Renderable) -> Vec<Vec<Segment>> {
        let options = self.console.options().update_width(self.width.max(1));
        self.console.render_lines(renderable, &options, false)
    }

    /// Render console markup into lines. Markup that does not parse is
    /// shown as it is.
    pub fn markup(&self, markup: &str) -> Vec<Vec<Segment>> {
        let text = Text::from_markup(markup).unwrap_or_else(|_| Text::new(markup));
        self.lines(&text)
    }
}

/// One thing a key does in a component, for a help overlay, a shortcut
/// sheet or status-bar hints. A host lists it under the component's
/// registered name, and configuration can rebind it there.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ComponentBinding {
    /// What it does, for code and configuration: `down`.
    pub action: String,
    /// The keys that do it, by name (see [`ComponentEvent::Key`]); the
    /// first is the one to show.
    pub keys: Vec<String>,
    /// What it does, for people: `move down`.
    pub description: String,
}

impl ComponentBinding {
    pub fn new(
        action: impl Into<String>,
        keys: impl IntoIterator<Item = impl Into<String>>,
        description: impl Into<String>,
    ) -> ComponentBinding {
        ComponentBinding {
            action: action.into(),
            keys: keys.into_iter().map(Into::into).collect(),
            description: description.into(),
        }
    }
}

/// An interactive component: a state machine that handles one event at a
/// time and renders its state. Registered with
/// [`PluginRegistrar::component`](crate::PluginRegistrar::component)
/// through a [`ComponentFactory`].
///
/// Key presses and pastes come as text from the user, and so does anything
/// a component shows of them: treat them as untrusted. The host keeps
/// terminal controls out of what is painted.
pub trait PluginComponent: Send {
    /// Handle one event. Ctrl+C never arrives: the host ends the run.
    fn handle(&mut self, event: &ComponentEvent, context: &ComponentContext<'_>) -> ComponentFlow;

    /// Render the current state.
    fn render(&self, context: &ComponentContext<'_>) -> ComponentView;

    /// The keys it uses now, for help and hints. None, by default.
    fn bindings(&self) -> Vec<ComponentBinding> {
        Vec::new()
    }

    /// Whether it takes focus in a container. Yes, by default; a view that
    /// only shows something says no.
    fn focusable(&self) -> bool {
        true
    }

    /// How often it wants [`ComponentEvent::Tick`]. Never, by default.
    fn tick(&self) -> Option<Duration> {
        None
    }

    /// Whether it wants mouse events. No, by default: reporting the mouse
    /// takes text selection away from the terminal.
    fn mouse(&self) -> bool {
        false
    }
}

//! Interactive terminal components for rs-rich (#451): the layer between
//! printing and a full TUI framework.
//!
//! A [`Component`] is a state machine: it handles one [`Event`] and renders
//! one [`View`]. Two drivers run it:
//!
//! - **blocking:** [`run`] takes the terminal, drives the component to an
//!   [`Outcome`] and gives the terminal back, like `Prompt.ask`;
//! - **event loop:** an [`EventLoop`] runs one or more components at once,
//!   with ticks and timers, and repaints only when something changed.
//!
//! Views are painted through [`rich_ext::frame::Frame::diff`], cell by cell,
//! so an idle loop writes nothing. Around them:
//!
//! - [`session`]: raw mode, the alternate screen, mouse and paste, restored
//!   on every way out, including a panic, Ctrl+C and a hand-off to another
//!   program (#489);
//! - [`event`]: keys, mouse, resizes and pastes, from `crossterm`;
//! - [`viewport`]: a scrollable window over rendered lines (#495);
//! - [`item`]: one item model behind every picker (#452);
//! - [`policy`]: with no terminal, under CI or with `TERM=dumb`, a
//!   component asks line by line, returns its default, or fails, as the
//!   caller chooses, and never blocks on a pipe (#492);
//! - [`headless`]: scripted events in, frames out, for tests;
//! - [`components`]: [`Select`], [`MultiSelect`], [`TableSelect`],
//!   [`TreeSelect`], [`Input`], [`TextArea`], [`Confirm`], [`Form`],
//!   [`Pager`], [`FilePicker`], [`ColorPicker`], [`AssetPicker`] and
//!   [`ThemePicker`], built on the above with a [`fuzzy`] matcher, and with
//!   the `data` feature `DataExplorer`, for JSON, YAML, TOML and the other
//!   formats `rich_ext::data` reads;
//! - [`clipboard`]: copy to the terminal's clipboard with OSC 52, when it
//!   takes it.
//!
//! Components compose (0.0.14): [`compose`] has containers that are
//! components themselves ([`Column`], [`Row`], [`Stack`], [`Split`],
//! [`Tabs`], [`Layers`]), with focus, routing and bubbling
//! ([`Flow::Ignored`]); [`kit`] has the line helpers and state types the
//! built-ins are made of; and [`keymap`] declares, rebinds and lists every
//! component's keys. On top of those, [`overlay`] has a command palette, a
//! help overlay, a shortcut sheet and an action menu read from the keymap,
//! and [`chrome`] a status bar and breadcrumbs; [`Overlays`] wires them all
//! into any component.
//!
//! Mouse support (#476) is opt-in per component ([`Component::mouse`]):
//! clicks, drags and the wheel arrive in the component's own coordinates, a
//! click on a hyperlink arrives as [`Event::Link`], and the border beside a
//! preview drags. Actions (#491) attach to list items, table rows, tree
//! nodes and file entries through [`Actions`] and an [`ActionTarget`], and
//! open in a modal menu (Ctrl+K); a region takes actions too, through
//! [`Overlays::actions`]; plugins register them through
//! `rs-rich-plugin-api`. Plugins register whole components too, and
//! [`PluginView`] mounts one by name beside the built-ins ([`plugin`]).
//!
//! ```
//! use rich_interact::{headless, Component, Context, Event, Flow, KeyCode, Outcome, View};
//!
//! /// Counts Up presses; Enter returns the count.
//! struct Counter(u32);
//!
//! impl Component for Counter {
//!     type Output = u32;
//!     fn handle(&mut self, event: &Event, _: &Context<'_>) -> Flow<u32> {
//!         match event.key().map(|key| key.code) {
//!             Some(KeyCode::Up) => self.0 += 1,
//!             Some(KeyCode::Enter) => return Flow::Done(self.0),
//!             _ => {}
//!         }
//!         Flow::Continue
//!     }
//!     fn render(&self, context: &Context<'_>) -> View {
//!         View::new(context.markup(&format!("count: [bold]{}[/]", self.0)))
//!     }
//! }
//!
//! let script = headless::Script::new().keys("up up enter");
//! let (outcome, record) = headless::run(Counter(0), script, 40, 5);
//! assert_eq!(outcome.unwrap(), Outcome::Done(2));
//! assert_eq!(record.last_frame(), "count: 2");
//! ```

pub mod chrome;
pub mod clipboard;
pub mod component;
pub mod components;
pub mod compose;
pub mod event;
pub mod event_loop;
pub mod fuzzy;
pub mod headless;
pub mod item;
pub mod keymap;
pub mod kit;
mod names;
pub mod overlay;
pub mod paint;
pub mod plugin;
pub mod policy;
pub mod session;
pub mod viewport;

pub use chrome::{Breadcrumbs, StatusBar, StatusItem};
pub use component::{Component, Context, Flow, View};
#[cfg(feature = "data")]
pub use components::DataExplorer;
pub use components::{
    Answers, AssetKind, AssetPicker, Choice, ColorFormat, ColorPicker, Confirm, FileMode,
    FilePicker, Form, Input, MultiSelect, Pager, PreviewLayout, Select, Suggestion, TableSelect,
    TextArea, Theme, ThemePicker, TreeSelect, Value,
};
pub use compose::{
    Axis, Column, ComponentExt, Label, Layer, LayerHandle, Layers, Rect, Row, Size, Split, Stack,
    Tabs,
};
pub use event::{Button, Event, Key, KeyCode, Modifiers, Mouse, MouseKind};
pub use event_loop::{degrade, run, Error, EventLoop, Handle, LoopOptions, Outcome, RunOptions};
pub use item::{Action, ActionFilter, ActionTarget, Actions, Item, Preview, TargetKind};
pub use keymap::{Binding, Keymap};
pub use overlay::{Command, Help, Menu, Overlays, Palette, Shortcuts};
pub use plugin::PluginView;
pub use policy::{Fallback, LineIo, NotInteractive, Policy, Reason};
pub use session::{Backend, Output, Session, SessionOptions};
pub use viewport::Viewport;

/// Forward every [`Component`] method to `(**self)`.
macro_rules! forward_component {
    () => {
        type Output = C::Output;

        fn start(&mut self, context: &Context<'_>) -> Flow<C::Output> {
            (**self).start(context)
        }

        fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<C::Output> {
            (**self).handle(event, context)
        }

        fn render(&self, context: &Context<'_>) -> View {
            (**self).render(context)
        }

        fn tick(&self) -> Option<std::time::Duration> {
            (**self).tick()
        }

        fn mouse(&self) -> bool {
            (**self).mouse()
        }

        fn keymap(&self) -> keymap::Keymap {
            (**self).keymap()
        }

        fn focusable(&self) -> bool {
            (**self).focusable()
        }

        fn focus_step(&mut self, forward: bool) -> bool {
            (**self).focus_step(forward)
        }

        fn focus_enter(&mut self, forward: bool) -> bool {
            (**self).focus_enter(forward)
        }

        fn default_value(&self) -> Option<C::Output> {
            (**self).default_value()
        }

        fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<C::Output>, NotInteractive> {
            (**self).prompt(io)
        }
    };
}

/// A component borrowed mutably is a component, so a caller can mount one
/// and still read its state after the loop.
impl<C: Component + ?Sized> Component for &mut C {
    forward_component!();
}

/// A boxed component is a component: how containers hold children of
/// different types with one output ([`compose::Child`]).
impl<C: Component + ?Sized> Component for Box<C> {
    forward_component!();
}

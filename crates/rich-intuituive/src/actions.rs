//! A live list of actions: the help and the command palette re-read what
//! they list while they are open, and [`action_overlay`] is a palette of
//! actions of your own, from a closure that may read signals.
//!
//! What either lists changes under it as the app does: an action comes or
//! goes as its condition changes ([`bind_when`](crate::Node::bind_when)),
//! one that cannot run now is listed greyed with why
//! ([`bind_unless`](crate::Node::bind_unless), [`Action::unavailable`]),
//! and the focus stays on the same action, by its id, as the list is
//! filtered or reordered. An action is checked again when it is chosen:
//! one that stopped applying after the list was drawn does not run.

use std::cell::RefCell;

use rich_interact::keymap::Binding;
use rich_interact::overlay::{Command, Help, Palette};
use rich_interact::{Component, Context, Event, Flow, Key, Keymap, View};

use crate::app::Ctx;
use crate::node::{component, BindingCondition, BindingReason, Node};
use crate::reactive::untracked;

/// An action an [`action_overlay`] lists: what identifies it (`id`), what
/// it says (`label`), the keys shown as its shortcut, and why it cannot
/// run now, when it cannot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub id: String,
    pub label: String,
    pub keys: Vec<Key>,
    pub unavailable: Option<String>,
}

impl Action {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Action {
        Action {
            id: id.into(),
            label: label.into(),
            keys: Vec::new(),
            unavailable: None,
        }
    }

    /// Its shortcut, shown at the right: space-separated key names
    /// (`"ctrl+s"`, `"up k"`).
    ///
    /// # Panics
    /// On a name that is not a key.
    #[must_use]
    pub fn keys(mut self, keys: &str) -> Action {
        self.keys = crate::node::parse_keys(keys);
        self
    }

    /// Listed greyed, saying `reason`, and not run.
    #[must_use]
    pub fn unavailable(mut self, reason: impl Into<String>) -> Action {
        self.unavailable = Some(reason.into());
        self
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.id, &self.label).keys(self.keys.iter().copied());
        command.unavailable = self.unavailable.clone();
        command
    }
}

/// A palette of the actions `actions` returns, read again whenever a
/// signal it reads changes, so actions come, go and change their labels
/// while it is open, the focus staying on the same id. Choosing one (Enter
/// or a click) closes it and calls `run` with the action's id, after
/// reading `actions` once more: one gone, or unavailable, by then is not
/// run. Escape closes it. Open it as a modal, as
/// [`Ctx::action_palette`](crate::Ctx::action_palette) does, so the focus
/// comes back where it was and no key reaches the screen beneath.
///
/// ```
/// use intuituive::prelude::*;
/// use intuituive::{action_overlay, Action, Size};
///
/// let app = App::new(|| {
///     let (playing, said) = (signal(false), signal(String::new()));
///     let actions = move || {
///         let toggle = match playing.get() {
///             true => Action::new("toggle", "Pause"),
///             false => Action::new("toggle", "Play"),
///         };
///         vec![toggle, Action::new("next", "Next track").unavailable("end of the queue")]
///     };
///     text!("{said}").on_key("ctrl+k", move |cx| {
///         cx.modal(Size::Percent(80), Size::Percent(80), move || {
///             action_overlay(actions, move |id, _| {
///                 if id == "toggle" {
///                     playing.update(|p| *p = !*p);
///                 }
///                 said.set(id.to_string());
///             })
///         })
///     })
/// });
/// let mut driver = app.driver(50, 8);
/// driver.press("ctrl+k");
/// let shown = driver.screen().plain().join("\n");
/// assert!(shown.contains("Play") && shown.contains("Next track (end of the queue)"));
/// // The unavailable action does not run.
/// driver.press("down enter");
/// assert!(driver.screen().plain().join("\n").contains("Play"));
/// driver.press("up enter");
/// assert_eq!(driver.screen().plain()[0].trim_end(), "toggle");
/// ```
pub fn action_overlay(
    actions: impl Fn() -> Vec<Action> + 'static,
    mut run: impl FnMut(&str, &mut Ctx) + 'static,
) -> Node {
    let actions = std::rc::Rc::new(actions);
    let source = actions.clone();
    let palette = LivePalette::new(
        Palette::new(Vec::new()).prompt("Action"),
        Box::new(move || source().iter().map(Action::command).collect()),
    );
    component(palette, move |chosen: Command, cx| {
        cx.pop();
        // Checked again as it runs: it may have gone, or stopped applying.
        let now = untracked(|| actions());
        let still = now
            .iter()
            .any(|action| action.id == chosen.id && action.unavailable.is_none());
        if still {
            run(&chosen.id, cx);
        }
    })
    .on_cancel(|cx| cx.pop())
}

/// A binding as the help or the palette may list it, with what decides,
/// as it is shown, whether it is listed and whether it is available: read
/// again each time the list is drawn.
#[derive(Clone)]
pub(crate) struct Listed {
    /// The node it is bound on, and its place among the node's bindings.
    pub node: crate::reactive::NodeId,
    pub index: usize,
    pub sequence: bool,
    pub binding: Binding,
    pub when: Option<BindingCondition>,
    pub unavailable: Option<BindingReason>,
}

impl Listed {
    /// The binding as it is now: `None` while its condition does not hold,
    /// else with why it is unavailable, if it is. Read so a signal its
    /// condition reads draws the list again.
    pub fn now(&self) -> Option<Binding> {
        if !self.when.as_ref().is_none_or(|when| when()) {
            return None;
        }
        let mut binding = self.binding.clone();
        binding.unavailable = self.unavailable.as_ref().and_then(|why| why());
        Some(binding)
    }
}

/// The bindings of `listed` that are listed now.
pub(crate) fn listed_now(listed: &[Listed]) -> Vec<Binding> {
    listed.iter().filter_map(Listed::now).collect()
}

type Source<T> = Box<dyn Fn() -> Vec<T>>;

/// A [`Palette`] whose commands come from `source`, read again as it is
/// drawn and before each event: the commands as they are now.
pub(crate) struct LivePalette {
    palette: RefCell<Palette>,
    source: Source<Command>,
    last: RefCell<Option<Vec<Command>>>,
}

impl LivePalette {
    pub(crate) fn new(palette: Palette, source: Source<Command>) -> LivePalette {
        let live = LivePalette {
            palette: RefCell::new(palette),
            source,
            last: RefCell::new(None),
        };
        untracked(|| live.refresh());
        live
    }

    fn refresh(&self) {
        let now = (self.source)();
        if self.last.borrow().as_ref() != Some(&now) {
            self.palette.borrow_mut().set_commands(now.clone());
            *self.last.borrow_mut() = Some(now);
        }
    }
}

impl Component for LivePalette {
    type Output = Command;

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<Command> {
        untracked(|| self.refresh());
        self.palette.get_mut().handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.refresh();
        self.palette.borrow().render(context)
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        self.palette.borrow().keymap()
    }
}

/// A [`Help`] whose bindings come from `source`, as [`LivePalette`]'s
/// commands do.
pub(crate) struct LiveHelp {
    help: RefCell<Help>,
    source: Source<Binding>,
    last: RefCell<Option<Vec<Binding>>>,
}

impl LiveHelp {
    pub(crate) fn new(help: Help, source: Source<Binding>) -> LiveHelp {
        let live = LiveHelp {
            help: RefCell::new(help),
            source,
            last: RefCell::new(None),
        };
        untracked(|| live.refresh());
        live
    }

    fn refresh(&self) {
        let now = (self.source)();
        if self.last.borrow().as_ref() != Some(&now) {
            self.help.borrow_mut().set_bindings(now.clone());
            *self.last.borrow_mut() = Some(now);
        }
    }
}

impl Component for LiveHelp {
    type Output = ();

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<()> {
        untracked(|| self.refresh());
        self.help.get_mut().handle(event, context)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.refresh();
        self.help.borrow().render(context)
    }

    fn mouse(&self) -> bool {
        true
    }

    fn keymap(&self) -> Keymap {
        self.help.borrow().keymap()
    }
}

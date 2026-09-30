//! Composition: containers that are themselves components (0.0.14
//! workstream 1, #478, #479, #480).
//!
//! A container holds children, gives each a [`Rect`] of its own space, and
//! is a [`Component`] like any other, so containers nest and run under both
//! drivers and the [headless](crate::headless) one:
//!
//! - [`Stack`] lays children out along an [`Axis`], each [`Size`]d fixed,
//!   flexible or by its content; [`Column`] and [`Row`] are its two axes;
//! - [`Split`] puts two children side by side or one above the other, with
//!   a border the mouse drags and the keyboard nudges, and minimum sizes;
//! - [`Tabs`] shows one child at a time under a bar of titles, and keeps
//!   every child's state while it is hidden;
//! - [`Layers`] puts modal and popover [`Layer`]s over a base: a modal
//!   dims what is under it and traps focus until it is dismissed.
//!
//! Children of one container share an output type `M`, the container's.
//! A child with another output is adapted with
//! [`ComponentExt::map`], which also decides what its answer means for the
//! whole: finish with a value, or carry on. [`Label`] and [`Painted`] show
//! something without taking focus.
//!
//! **Routing.** Keys and pastes go to the focused child. A child that does
//! not use an event returns [`Flow::Ignored`] and it *bubbles*: the
//! container tries its own bindings, then leaves it to its own container.
//! Mouse events go to the child under the pointer (a press holds that child
//! until the release, so a drag stays with it) in the child's own
//! coordinates, and a click focuses the child. Resizes and ticks reach
//! every child; a hand-off's [`Event::Returned`] goes back to the child that
//! handed off.
//!
//! **Focus.** Tab and Shift+Tab (the `focus-next` and `focus-previous`
//! bindings) move focus through every focusable child in order, into and
//! out of nested containers, and wrap round at the top. They bubble like any
//! key, so a child that uses Tab (a multi-select marking, an input
//! completing) keeps it; a container overrides the order by implementing
//! [`Component::focus_step`] and [`Component::focus_enter`], and rebinds
//! the keys with its `rebind`.
//!
//! **Bindings.** Every container declares its keys in a
//! [`Keymap`], and [`Component::keymap`] lists the
//! focused child's then the container's own. `on` adds a binding of yours
//! that runs when a key bubbles up unused; `shortcut` one that runs before
//! the focused child sees the key.
//!
//! ```
//! use rich_interact::compose::{Column, ComponentExt, Label, Split};
//! use rich_interact::{headless, Flow, Input, Outcome, Select};
//!
//! let picker = Select::new("File", ["a.rs", "b.rs"]).map(Flow::Done);
//! let name = Input::new("Name").map(|_| Flow::Continue);
//! let app = Column::new()
//!     .child(Label::new("[bold]Demo[/]"))
//!     .child(Split::horizontal(picker, name).ratio(50));
//! let script = headless::Script::new().keys("down enter");
//! let (outcome, _) = headless::run(app, script, 60, 8);
//! assert_eq!(outcome.unwrap(), Outcome::Done("b.rs"));
//! ```

use std::cell::{Cell, RefCell};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

use rich::{Segment, Style};

use crate::component::{Component, Context, Flow, View};
use crate::event::{Event, Key, Mouse, MouseKind};
use crate::keymap::{keys, Binding, Keymap};
use crate::kit::{self, Divider, Theme};
use crate::policy::{LineIo, NotInteractive};

/// The action that moves focus to the next focusable child (Tab).
pub const FOCUS_NEXT: &str = "focus-next";
/// The action that moves focus to the previous focusable child (Shift+Tab).
pub const FOCUS_PREVIOUS: &str = "focus-previous";

/// A child of a container: any component with the container's output,
/// boxed. [`ComponentExt::boxed`] makes one.
pub type Child<'a, M> = Box<dyn Component<Output = M> + 'a>;

/// A rectangle of cells, from the top left of the container's view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the cell at `column`, `row` is inside.
    pub fn contains(&self, column: usize, row: usize) -> bool {
        column >= self.x
            && column < self.x + self.width
            && row >= self.y
            && row < self.y + self.height
    }

    /// `mouse` in this rectangle's own coordinates (0, 0 at its top left).
    pub fn local(&self, mouse: Mouse) -> Mouse {
        let clamp = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
        Mouse {
            column: clamp((mouse.column as usize).saturating_sub(self.x)),
            row: clamp((mouse.row as usize).saturating_sub(self.y)),
            ..mouse
        }
    }

    /// The context a child in this rectangle renders and handles events
    /// with: the same console, this rectangle's size.
    pub fn context<'c>(&self, context: &Context<'c>) -> Context<'c> {
        Context {
            console: context.console,
            width: self.width.max(1),
            height: self.height.max(1),
        }
    }
}

/// Which way a [`Stack`] or a [`Split`] lays children out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Side by side, left to right.
    Horizontal,
    /// One above the other, top to bottom.
    #[default]
    Vertical,
}

/// How much of a [`Stack`] a child takes along its axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Size {
    /// Exactly this many cells (rows in a column, columns in a row), or
    /// what is left.
    Fixed(usize),
    /// A share of what the fixed and content-sized children leave, by
    /// weight: `Flex(2)` takes twice what `Flex(1)` does.
    Flex(u16),
    /// In a column, the rows the child renders; in a row, the same as
    /// `Flex(1)`.
    #[default]
    Auto,
}

/// Split `total` cells by `weights`, the remainder to the first.
fn share(total: usize, weights: &[u16]) -> Vec<usize> {
    let sum: usize = weights.iter().map(|w| usize::from(*w)).sum();
    if sum == 0 {
        return vec![0; weights.len()];
    }
    let mut out: Vec<usize> = weights
        .iter()
        .map(|w| total * usize::from(*w) / sum)
        .collect();
    let mut left = total - out.iter().sum::<usize>();
    for (index, weight) in weights.iter().enumerate() {
        if left == 0 {
            break;
        }
        if *weight > 0 {
            out[index] += 1;
            left -= 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Adapting leaves.

/// A component whose answer is turned into a container's
/// [`Flow`]: see [`ComponentExt::map`]. Once the component has answered or
/// cancelled, it no longer takes focus or events, and shows its final view.
pub struct Map<'a, C: Component, M> {
    inner: C,
    done: Box<dyn FnMut(C::Output) -> Flow<M> + 'a>,
    cancel: Box<dyn FnMut() -> Flow<M> + 'a>,
    finished: bool,
}

impl<'a, C: Component, M> Map<'a, C, M> {
    /// What cancelling means (default: [`Flow::Cancel`], which cancels the
    /// container too).
    pub fn on_cancel(mut self, cancel: impl FnMut() -> Flow<M> + 'a) -> Self {
        self.cancel = Box::new(cancel);
        self
    }

    /// The component inside.
    pub fn inner(&self) -> &C {
        &self.inner
    }

    pub fn inner_mut(&mut self) -> &mut C {
        &mut self.inner
    }

    /// Whether the component has answered or cancelled.
    pub fn finished(&self) -> bool {
        self.finished
    }

    fn convert(&mut self, flow: Flow<C::Output>) -> Flow<M> {
        match flow {
            Flow::Continue => Flow::Continue,
            Flow::Ignored => Flow::Ignored,
            Flow::Handoff(command) => Flow::Handoff(command),
            Flow::Done(value) => {
                self.finished = true;
                (self.done)(value)
            }
            Flow::Cancel => {
                self.finished = true;
                (self.cancel)()
            }
        }
    }
}

impl<C: Component, M> Component for Map<'_, C, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let flow = self.inner.start(context);
        self.convert(flow)
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        if self.finished {
            return Flow::Ignored;
        }
        let flow = self.inner.handle(event, context);
        self.convert(flow)
    }

    fn render(&self, context: &Context<'_>) -> View {
        self.inner.render(context)
    }

    fn tick(&self) -> Option<Duration> {
        if self.finished {
            None
        } else {
            self.inner.tick()
        }
    }

    fn mouse(&self) -> bool {
        !self.finished && self.inner.mouse()
    }

    fn keymap(&self) -> Keymap {
        if self.finished {
            Keymap::default()
        } else {
            self.inner.keymap()
        }
    }

    fn focusable(&self) -> bool {
        !self.finished && self.inner.focusable()
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        !self.finished && self.inner.focus_step(forward)
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        !self.finished && self.inner.focus_enter(forward)
    }
}

/// Adapters for putting any component in a container.
pub trait ComponentExt: Component + Sized {
    /// Turn this component's answer into `M`'s flow: `Flow::Done(m)` to
    /// finish the whole composition with `m`, `Flow::Continue` to carry on
    /// (the component stays, showing its answer, and focus moves past it).
    /// Cancelling cancels the composition unless
    /// [`on_cancel`](Map::on_cancel) says otherwise.
    fn map<'a, M>(self, done: impl FnMut(Self::Output) -> Flow<M> + 'a) -> Map<'a, Self, M>
    where
        Self: 'a,
    {
        Map {
            inner: self,
            done: Box::new(done),
            cancel: Box::new(|| Flow::Cancel),
            finished: false,
        }
    }

    /// Box this component as a [`Child`].
    fn boxed<'a>(self) -> Child<'a, Self::Output>
    where
        Self: 'a,
    {
        Box::new(self)
    }
}

impl<C: Component> ComponentExt for C {}

/// Console markup that takes no focus and uses no events: a title, a note.
pub struct Label<M> {
    markup: String,
    _output: PhantomData<fn() -> M>,
}

impl<M> Label<M> {
    pub fn new(markup: impl Into<String>) -> Label<M> {
        Label {
            markup: markup.into(),
            _output: PhantomData,
        }
    }

    /// Change what it says.
    pub fn set(&mut self, markup: impl Into<String>) {
        self.markup = markup.into();
    }
}

impl<M> Component for Label<M> {
    type Output = M;

    fn handle(&mut self, _: &Event, _: &Context<'_>) -> Flow<M> {
        Flow::Ignored
    }

    fn render(&self, context: &Context<'_>) -> View {
        View::new(context.markup(&self.markup))
    }

    fn focusable(&self) -> bool {
        false
    }
}

/// A view drawn by a function, taking no focus and using no events: a
/// status line computed from shared state, a preview.
pub struct Painted<F, M> {
    paint: F,
    _output: PhantomData<fn() -> M>,
}

impl<F: Fn(&Context<'_>) -> View, M> Painted<F, M> {
    pub fn new(paint: F) -> Painted<F, M> {
        Painted {
            paint,
            _output: PhantomData,
        }
    }
}

impl<F: Fn(&Context<'_>) -> View, M> Component for Painted<F, M> {
    type Output = M;

    fn handle(&mut self, _: &Event, _: &Context<'_>) -> Flow<M> {
        Flow::Ignored
    }

    fn render(&self, context: &Context<'_>) -> View {
        (self.paint)(context)
    }

    fn focusable(&self) -> bool {
        false
    }
}

// ---------------------------------------------------------------------------
// What every container shares: children with focus, and bindings.

/// Whether a flow ends the event's journey: anything but carrying on.
fn decisive<M>(flow: &Flow<M>) -> bool {
    !matches!(flow, Flow::Continue | Flow::Ignored)
}

/// Children, which one has focus, which one a mouse press holds, and which
/// one handed the terminal off.
struct Group<'a, M> {
    children: Vec<Child<'a, M>>,
    focus: Option<usize>,
    grab: Option<usize>,
    handoff: Option<usize>,
}

impl<'a, M> Group<'a, M> {
    fn new() -> Group<'a, M> {
        Group {
            children: Vec::new(),
            focus: None,
            grab: None,
            handoff: None,
        }
    }

    /// The focused child: the one focused last if it still takes focus,
    /// else the first that does.
    fn focused(&self) -> Option<usize> {
        let takes = |index: usize| self.children[index].focusable();
        self.focus
            .filter(|&index| index < self.children.len() && takes(index))
            .or_else(|| (0..self.children.len()).find(|&index| takes(index)))
    }

    fn settle(&mut self) {
        self.focus = self.focused();
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        let current = self.focused();
        if let Some(index) = current {
            if self.children[index].focus_step(forward) {
                return true;
            }
        }
        let count = self.children.len();
        let order: Vec<usize> = if forward {
            (current.map_or(0, |index| index + 1)..count).collect()
        } else {
            (0..current.unwrap_or(count)).rev().collect()
        };
        for index in order {
            if self.children[index].focus_enter(forward) {
                self.focus = Some(index);
                return true;
            }
        }
        false
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        let count = self.children.len();
        let order: Vec<usize> = if forward {
            (0..count).collect()
        } else {
            (0..count).rev().collect()
        };
        for index in order {
            if self.children[index].focus_enter(forward) {
                self.focus = Some(index);
                return true;
            }
        }
        false
    }

    fn deliver(&mut self, index: usize, event: &Event, context: &Context<'_>) -> Flow<M> {
        let flow = self.children[index].handle(event, context);
        if let Flow::Handoff(_) = flow {
            self.handoff = Some(index);
        }
        flow
    }

    fn start(&mut self, context: &Context<'_>, rect: impl Fn(usize) -> Rect) -> Flow<M> {
        for index in 0..self.children.len() {
            let flow = self.children[index].start(&rect(index).context(context));
            if decisive(&flow) {
                return flow;
            }
        }
        self.settle();
        Flow::Continue
    }

    /// Route `event` to the children laid out at `rects` (child index and
    /// rectangle); children missing from `rects` (hidden tabs) get resizes
    /// and ticks at `hidden`.
    fn route(
        &mut self,
        event: &Event,
        context: &Context<'_>,
        rects: &[(usize, Rect)],
        hidden: Rect,
    ) -> Flow<M> {
        let rect_of = |index: usize| {
            rects
                .iter()
                .find(|(i, _)| *i == index)
                .map_or(hidden, |(_, rect)| *rect)
        };
        match event {
            Event::Mouse(mouse) => self.route_mouse(*mouse, context, rects),
            Event::Tick | Event::Resize { .. } => {
                for index in 0..self.children.len() {
                    if matches!(event, Event::Tick) && self.children[index].tick().is_none() {
                        continue;
                    }
                    let flow = self.deliver(index, event, &rect_of(index).context(context));
                    if decisive(&flow) {
                        return flow;
                    }
                }
                Flow::Continue
            }
            Event::Returned(_) => match self.handoff.take().or_else(|| self.focused()) {
                Some(index) => self.deliver(index, event, &rect_of(index).context(context)),
                None => Flow::Ignored,
            },
            _ => match self.focused() {
                Some(index) => {
                    self.focus = Some(index);
                    self.deliver(index, event, &rect_of(index).context(context))
                }
                None => Flow::Ignored,
            },
        }
    }

    fn route_mouse(
        &mut self,
        mouse: Mouse,
        context: &Context<'_>,
        rects: &[(usize, Rect)],
    ) -> Flow<M> {
        let (column, row) = (mouse.column as usize, mouse.row as usize);
        let under = rects
            .iter()
            .find(|(_, rect)| rect.contains(column, row))
            .map(|(index, _)| *index);
        let mut focused = false;
        let target = match mouse.kind {
            MouseKind::Down(_) => {
                self.grab = under;
                if let Some(index) = under.filter(|&i| self.children[i].focusable()) {
                    focused = self.focused() != Some(index);
                    self.focus = Some(index);
                }
                under
            }
            MouseKind::Drag(_) => self.grab.or(under),
            MouseKind::Up(_) => self.grab.take().or(under),
            _ => under,
        };
        let Some(index) = target else {
            return Flow::Ignored;
        };
        let Some((_, rect)) = rects.iter().find(|(i, _)| *i == index).copied() else {
            return Flow::Ignored;
        };
        if !self.children[index].mouse() {
            return if focused {
                Flow::Continue
            } else {
                Flow::Ignored
            };
        }
        let flow = self.deliver(
            index,
            &Event::Mouse(rect.local(mouse)),
            &rect.context(context),
        );
        match flow {
            Flow::Ignored if focused => Flow::Continue,
            flow => flow,
        }
    }

    fn tick(&self) -> Option<Duration> {
        self.children.iter().filter_map(|child| child.tick()).min()
    }

    fn mouse(&self) -> bool {
        self.children.iter().any(|child| child.mouse())
    }

    fn focusable(&self) -> bool {
        self.children.iter().any(|child| child.focusable())
    }
}

type Handler<'a, M> = Box<dyn FnMut() -> Flow<M> + 'a>;

/// A container's keymap, which of its actions come before the focused
/// child, and the handlers of the bindings added with `on` and `shortcut`.
struct Bindings<'a, M> {
    keymap: Keymap,
    priority: Vec<String>,
    handlers: Vec<(String, Handler<'a, M>)>,
}

impl<'a, M> Bindings<'a, M> {
    fn new(context: &str) -> Bindings<'a, M> {
        Bindings {
            keymap: Keymap::new(context),
            priority: Vec::new(),
            handlers: Vec::new(),
        }
    }

    fn declare(&mut self, action: &str, keys: Vec<Key>, description: &str, priority: bool) {
        let binding = Binding::new(self.keymap.context(), action, keys, description);
        self.keymap.add(binding);
        self.priority.retain(|p| p != action);
        if priority {
            self.priority.push(action.to_string());
        }
    }

    fn focus_keys(mut self) -> Self {
        self.declare(FOCUS_NEXT, keys("tab"), "next field", false);
        self.declare(FOCUS_PREVIOUS, keys("shift+tab"), "previous field", false);
        self
    }

    fn on(
        &mut self,
        action: &str,
        keys: Vec<Key>,
        description: &str,
        priority: bool,
        handler: impl FnMut() -> Flow<M> + 'a,
    ) {
        self.declare(action, keys, description, priority);
        self.handlers.retain(|(name, _)| name != action);
        self.handlers.push((action.to_string(), Box::new(handler)));
    }

    /// The action `key` triggers among the priority (`true`) or the other
    /// bindings.
    fn action(&self, key: Key, priority: bool) -> Option<String> {
        let action = self.keymap.action(key)?;
        (self.priority.iter().any(|p| p == action) == priority).then(|| action.to_string())
    }

    /// Run the handler for `action`, if it has one.
    fn run(&mut self, action: &str) -> Option<Flow<M>> {
        self.handlers
            .iter_mut()
            .find(|(name, _)| name == action)
            .map(|(_, handler)| handler())
    }
}

/// The builder methods every container has for its bindings.
macro_rules! binding_builders {
    () => {
        /// Run `handler` when one of `keys` bubbles up unused by the
        /// focused child: an action of yours, listed in the keymap under
        /// this container's context.
        pub fn on(
            mut self,
            action: &str,
            keys: impl IntoIterator<Item = Key>,
            description: &str,
            handler: impl FnMut() -> Flow<M> + 'a,
        ) -> Self {
            let keys = keys.into_iter().collect();
            self.bindings.on(action, keys, description, false, handler);
            self
        }

        /// Run `handler` when one of `keys` is pressed, before the focused
        /// child sees it.
        pub fn shortcut(
            mut self,
            action: &str,
            keys: impl IntoIterator<Item = Key>,
            description: &str,
            handler: impl FnMut() -> Flow<M> + 'a,
        ) -> Self {
            let keys = keys.into_iter().collect();
            self.bindings.on(action, keys, description, true, handler);
            self
        }

        /// Make `keys` do this container's `action` (`focus-next`, and the
        /// container's own); no keys unbinds it.
        pub fn rebind(mut self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
            self.bindings.keymap.rebind(action, keys);
            self
        }

        /// This container's own bindings, without its children's.
        pub fn own_keymap(&self) -> &Keymap {
            &self.bindings.keymap
        }

        /// Report the mouse even when no child asks for it: for a click to
        /// focus a child, or a border to drag.
        pub fn with_mouse(mut self, on: bool) -> Self {
            self.mouse = on;
            self
        }
    };
}

// ---------------------------------------------------------------------------
// Stack, Column, Row.

/// Children laid out along an axis, each [`Size`]d: a [`Column`] top to
/// bottom, a [`Row`] left to right.
pub struct Stack<'a, M> {
    axis: Axis,
    group: Group<'a, M>,
    sizes: Vec<Size>,
    gap: usize,
    bindings: Bindings<'a, M>,
    mouse: bool,
}

impl<'a, M> Stack<'a, M> {
    pub fn new(axis: Axis) -> Stack<'a, M> {
        Stack {
            axis,
            group: Group::new(),
            sizes: Vec::new(),
            gap: 0,
            bindings: Bindings::new("stack").focus_keys(),
            mouse: false,
        }
    }

    /// Add a child sized [`Size::Auto`].
    pub fn child(self, child: impl Component<Output = M> + 'a) -> Self {
        self.sized(Size::Auto, child)
    }

    /// Add a child sized `size`.
    pub fn sized(mut self, size: Size, child: impl Component<Output = M> + 'a) -> Self {
        self.push(size, Box::new(child));
        self
    }

    /// Add a child to a stack already built.
    pub fn push(&mut self, size: Size, child: Child<'a, M>) {
        self.group.children.push(child);
        self.sizes.push(size);
    }

    /// Leave `cells` empty between children.
    pub fn gap(mut self, cells: usize) -> Self {
        self.gap = cells;
        self
    }

    binding_builders!();

    pub fn axis(&self) -> Axis {
        self.axis
    }

    pub fn len(&self) -> usize {
        self.group.children.len()
    }

    pub fn is_empty(&self) -> bool {
        self.group.children.is_empty()
    }

    /// Child `index`.
    pub fn get(&self, index: usize) -> Option<&(dyn Component<Output = M> + 'a)> {
        self.group.children.get(index).map(|child| &**child)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut (dyn Component<Output = M> + 'a)> {
        self.group.children.get_mut(index).map(|child| &mut **child)
    }

    /// The focused child's index.
    pub fn focused(&self) -> Option<usize> {
        self.group.focused()
    }

    /// Focus child `index`, if it takes focus.
    pub fn focus(&mut self, index: usize) -> bool {
        let takes = self
            .group
            .children
            .get_mut(index)
            .is_some_and(|child| child.focus_enter(true));
        if takes {
            self.group.focus = Some(index);
        }
        takes
    }

    /// Each child's rectangle at `context`'s size. A content-sized child
    /// in a column is rendered to measure it.
    pub fn layout(&self, context: &Context<'_>) -> Vec<Rect> {
        let count = self.len();
        let gaps = self.gap * count.saturating_sub(1);
        let vertical = self.axis == Axis::Vertical;
        let total = if vertical {
            context.height
        } else {
            context.width
        };
        let mut left = total.saturating_sub(gaps);
        let mut extent = vec![0; count];
        for (index, size) in self.sizes.iter().enumerate() {
            if let Size::Fixed(cells) = size {
                extent[index] = (*cells).min(left);
                left -= extent[index];
            }
        }
        if vertical {
            for (index, size) in self.sizes.iter().enumerate() {
                if *size == Size::Auto {
                    let probe = Rect::new(0, 0, context.width, left).context(context);
                    extent[index] = self.group.children[index]
                        .render(&probe)
                        .lines
                        .len()
                        .min(left);
                    left -= extent[index];
                }
            }
        }
        let weights: Vec<u16> = self
            .sizes
            .iter()
            .map(|size| match size {
                Size::Flex(weight) => *weight,
                Size::Auto if !vertical => 1,
                _ => 0,
            })
            .collect();
        for (index, cells) in share(left, &weights).into_iter().enumerate() {
            if weights[index] > 0 {
                extent[index] = cells;
            }
        }
        let mut at = 0;
        extent
            .into_iter()
            .map(|cells| {
                let rect = if vertical {
                    Rect::new(0, at, context.width, cells)
                } else {
                    Rect::new(at, 0, cells, context.height)
                };
                at += cells + self.gap;
                rect
            })
            .collect()
    }

    fn indexed(&self, context: &Context<'_>) -> Vec<(usize, Rect)> {
        self.layout(context).into_iter().enumerate().collect()
    }
}

/// The rows of `view` in `rect`: cut to its height, and padded to it when
/// `fill`.
fn rows_of(view: &View, rect: Rect, fill: bool) -> Vec<Vec<Segment>> {
    let mut lines: Vec<Vec<Segment>> = view
        .lines
        .iter()
        .take(rect.height)
        .map(|line| kit::fit(line.clone(), rect.width))
        .collect();
    if fill {
        lines.resize_with(rect.height, Vec::new);
    }
    lines
}

/// Views side by side at `rects`, each padded to its width but the last;
/// the cursor of view `focus`, if it has one.
fn side_by_side(
    views: &[(View, Rect)],
    rows: usize,
    focus: Option<usize>,
    between: &[Vec<Segment>],
) -> View {
    let mut lines = Vec::with_capacity(rows);
    for row in 0..rows {
        let mut line = Vec::new();
        for (index, (view, rect)) in views.iter().enumerate() {
            let cells = view.lines.get(row).cloned().unwrap_or_default();
            if index + 1 == views.len() {
                line.extend(kit::fit(cells, rect.width));
            } else {
                line.extend(kit::pad(cells, rect.width));
                line.extend(between.get(index).cloned().unwrap_or_default());
            }
        }
        lines.push(line);
    }
    let mut out = View::new(lines);
    if let Some((view, rect)) = focus.and_then(|index| views.get(index)) {
        if let Some((row, column)) = view.cursor {
            if row < rows && column < rect.width.max(1) {
                out.cursor = Some((rect.y + row, rect.x + column));
            }
        }
    }
    out
}

/// The container's keymap: the focused child's, then its own.
fn keymap_with<M>(focused: Option<&Child<'_, M>>, own: &Keymap) -> Keymap {
    let mut keymap = focused.map(|child| child.keymap()).unwrap_or_default();
    keymap.extend(own.clone());
    keymap
}

impl<M> Component for Stack<'_, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let rects = self.layout(context);
        self.group.start(context, |index| rects[index])
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        if let Some(key) = event.key() {
            if let Some(action) = self.bindings.action(key, true) {
                if let Some(flow) = self.bindings.run(&action) {
                    return flow;
                }
            }
        }
        let rects = self.indexed(context);
        let flow = self.group.route(event, context, &rects, Rect::default());
        self.group.settle();
        if !matches!(flow, Flow::Ignored) {
            return flow;
        }
        let Some(action) = event.key().and_then(|key| self.bindings.action(key, false)) else {
            return Flow::Ignored;
        };
        match action.as_str() {
            FOCUS_NEXT | FOCUS_PREVIOUS => {
                if self.group.focus_step(action == FOCUS_NEXT) {
                    Flow::Continue
                } else {
                    Flow::Ignored
                }
            }
            _ => self.bindings.run(&action).unwrap_or(Flow::Ignored),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let rects = self.layout(context);
        let focus = self.group.focused();
        let views: Vec<(View, Rect)> = rects
            .iter()
            .enumerate()
            .map(|(index, rect)| {
                (
                    self.group.children[index].render(&rect.context(context)),
                    *rect,
                )
            })
            .collect();
        match self.axis {
            Axis::Vertical => {
                let mut out = View::default();
                for (index, (view, rect)) in views.iter().enumerate() {
                    if index > 0 {
                        out.lines.extend(std::iter::repeat_n(Vec::new(), self.gap));
                    }
                    let fill = self.sizes[index] != Size::Auto;
                    let top = out.lines.len();
                    out.lines.extend(rows_of(view, *rect, fill));
                    if Some(index) == focus {
                        if let Some((row, column)) = view.cursor {
                            if row < rect.height {
                                out.cursor = Some((top + row, column));
                            }
                        }
                    }
                }
                out
            }
            Axis::Horizontal => {
                let rows = views
                    .iter()
                    .map(|(view, _)| view.lines.len())
                    .max()
                    .unwrap_or(0)
                    .min(context.height);
                let gap = vec![kit::plain(" ".repeat(self.gap))];
                let between = vec![gap; views.len()];
                side_by_side(&views, rows, focus, &between)
            }
        }
    }

    fn tick(&self) -> Option<Duration> {
        self.group.tick()
    }

    fn mouse(&self) -> bool {
        self.mouse || self.group.mouse()
    }

    fn keymap(&self) -> Keymap {
        let focused = self
            .group
            .focused()
            .map(|index| &self.group.children[index]);
        keymap_with(focused, &self.bindings.keymap)
    }

    fn focusable(&self) -> bool {
        self.group.focusable()
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        self.group.focus_step(forward)
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        self.group.focus_enter(forward)
    }
}

/// A [`Stack`] with a fixed axis, under its own name.
macro_rules! axis_stack {
    ($(#[$doc:meta])* $name:ident, $axis:expr) => {
        $(#[$doc])*
        pub struct $name<'a, M>(Stack<'a, M>);

        impl<'a, M> Default for $name<'a, M> {
            fn default() -> Self {
                Self::new()
            }
        }

        impl<'a, M> $name<'a, M> {
            pub fn new() -> Self {
                $name(Stack::new($axis))
            }

            /// Add a child sized [`Size::Auto`].
            pub fn child(self, child: impl Component<Output = M> + 'a) -> Self {
                $name(self.0.child(child))
            }

            /// Add a child sized `size`.
            pub fn sized(self, size: Size, child: impl Component<Output = M> + 'a) -> Self {
                $name(self.0.sized(size, child))
            }

            /// Leave `cells` empty between children.
            pub fn gap(self, cells: usize) -> Self {
                $name(self.0.gap(cells))
            }

            /// See [`Stack::on`].
            pub fn on(
                self,
                action: &str,
                keys: impl IntoIterator<Item = Key>,
                description: &str,
                handler: impl FnMut() -> Flow<M> + 'a,
            ) -> Self {
                $name(self.0.on(action, keys, description, handler))
            }

            /// See [`Stack::shortcut`].
            pub fn shortcut(
                self,
                action: &str,
                keys: impl IntoIterator<Item = Key>,
                description: &str,
                handler: impl FnMut() -> Flow<M> + 'a,
            ) -> Self {
                $name(self.0.shortcut(action, keys, description, handler))
            }

            /// See [`Stack::rebind`].
            pub fn rebind(self, action: &str, keys: impl IntoIterator<Item = Key>) -> Self {
                $name(self.0.rebind(action, keys))
            }

            /// See [`Stack::with_mouse`].
            pub fn with_mouse(self, on: bool) -> Self {
                $name(self.0.with_mouse(on))
            }

            /// The stack inside.
            pub fn stack(&self) -> &Stack<'a, M> {
                &self.0
            }

            pub fn stack_mut(&mut self) -> &mut Stack<'a, M> {
                &mut self.0
            }
        }

        impl<'a, M> Component for $name<'a, M> {
            type Output = M;

            fn start(&mut self, context: &Context<'_>) -> Flow<M> {
                self.0.start(context)
            }

            fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
                self.0.handle(event, context)
            }

            fn render(&self, context: &Context<'_>) -> View {
                self.0.render(context)
            }

            fn tick(&self) -> Option<Duration> {
                self.0.tick()
            }

            fn mouse(&self) -> bool {
                Component::mouse(&self.0)
            }

            fn keymap(&self) -> Keymap {
                self.0.keymap()
            }

            fn focusable(&self) -> bool {
                self.0.focusable()
            }

            fn focus_step(&mut self, forward: bool) -> bool {
                self.0.focus_step(forward)
            }

            fn focus_enter(&mut self, forward: bool) -> bool {
                self.0.focus_enter(forward)
            }
        }
    };
}

axis_stack!(
    /// Children top to bottom: a [`Stack`] on the vertical axis. A child
    /// sized [`Size::Auto`] takes the rows it renders; flexible children
    /// share the rest of the height, and fill it.
    Column,
    Axis::Vertical
);

axis_stack!(
    /// Children left to right: a [`Stack`] on the horizontal axis. Fixed
    /// children take their columns and the rest share what is left.
    Row,
    Axis::Horizontal
);

// ---------------------------------------------------------------------------
// Split.

/// Two children side by side ([`Split::horizontal`]) or one above the other
/// ([`Split::vertical`]), with a border between them. The border drags with
/// the mouse (#476's preview border, generalised, #479) and moves with
/// Alt+H/Alt+L (side by side) or Alt+K/Alt+J (stacked); each pane keeps its
/// minimum size. Tab moves focus between the panes.
pub struct Split<'a, M> {
    axis: Axis,
    group: Group<'a, M>,
    divider: Divider,
    ratio: u8,
    style: Style,
    bindings: Bindings<'a, M>,
    mouse: bool,
}

impl<'a, M> Split<'a, M> {
    fn new(
        axis: Axis,
        first: impl Component<Output = M> + 'a,
        second: impl Component<Output = M> + 'a,
    ) -> Split<'a, M> {
        let mut group = Group::new();
        group.children.push(Box::new(first) as Child<'a, M>);
        group.children.push(Box::new(second) as Child<'a, M>);
        let mut bindings = Bindings::new("split").focus_keys();
        let (shrink, grow) = match axis {
            Axis::Horizontal => ("alt+h", "alt+l"),
            Axis::Vertical => ("alt+k", "alt+j"),
        };
        bindings.declare("shrink", keys(shrink), "move the border back", true);
        bindings.declare("grow", keys(grow), "move the border on", true);
        Split {
            axis,
            group,
            divider: Divider::new(1).min(1),
            ratio: 50,
            style: Theme::default().border,
            bindings,
            mouse: false,
        }
    }

    /// `left` and `right`, side by side.
    pub fn horizontal(
        left: impl Component<Output = M> + 'a,
        right: impl Component<Output = M> + 'a,
    ) -> Split<'a, M> {
        Split::new(Axis::Horizontal, left, right)
    }

    /// `top` above `bottom`. The split takes the whole height.
    pub fn vertical(
        top: impl Component<Output = M> + 'a,
        bottom: impl Component<Output = M> + 'a,
    ) -> Split<'a, M> {
        Split::new(Axis::Vertical, top, bottom)
    }

    /// The first pane's share, in percent, until the border is moved
    /// (default 50).
    pub fn ratio(mut self, percent: u8) -> Self {
        self.ratio = percent.min(100);
        self
    }

    /// Put the border after `cells` of the first pane, until moved.
    pub fn at(mut self, cells: usize) -> Self {
        self.divider.set_position(Some(cells));
        self
    }

    /// Both panes at least `cells` along the axis (default 1).
    pub fn min(mut self, cells: usize) -> Self {
        self.divider.min_before = cells;
        self.divider.min_after = cells;
        self
    }

    /// The first pane at least `first` cells and the second at least
    /// `second`.
    pub fn mins(mut self, first: usize, second: usize) -> Self {
        self.divider.min_before = first;
        self.divider.min_after = second;
        self
    }

    /// The border's style (default: the theme's, dim).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    binding_builders!();

    pub fn axis(&self) -> Axis {
        self.axis
    }

    /// The border: where it is, whether it is being dragged.
    pub fn divider(&self) -> &Divider {
        &self.divider
    }

    pub fn first(&self) -> &(dyn Component<Output = M> + 'a) {
        &*self.group.children[0]
    }

    pub fn second(&self) -> &(dyn Component<Output = M> + 'a) {
        &*self.group.children[1]
    }

    pub fn first_mut(&mut self) -> &mut (dyn Component<Output = M> + 'a) {
        &mut *self.group.children[0]
    }

    pub fn second_mut(&mut self) -> &mut (dyn Component<Output = M> + 'a) {
        &mut *self.group.children[1]
    }

    /// The focused pane: 0 or 1.
    pub fn focused(&self) -> Option<usize> {
        self.group.focused()
    }

    /// Focus pane `index` (0 or 1), if it takes focus.
    pub fn focus(&mut self, index: usize) -> bool {
        let takes = self
            .group
            .children
            .get_mut(index)
            .is_some_and(|child| child.focus_enter(true));
        if takes {
            self.group.focus = Some(index);
        }
        takes
    }

    /// The cells along the axis, and the first pane's.
    fn extent(&self, context: &Context<'_>) -> (usize, usize) {
        let total = match self.axis {
            Axis::Horizontal => context.width,
            Axis::Vertical => context.height,
        };
        let default = self
            .divider
            .clamp(total * usize::from(self.ratio) / 100, total);
        (total, self.divider.resolve(total, default))
    }

    /// The two panes' rectangles at `context`'s size.
    pub fn layout(&self, context: &Context<'_>) -> [Rect; 2] {
        let (total, first) = self.extent(context);
        let second = total.saturating_sub(first + self.divider.thickness);
        match self.axis {
            Axis::Horizontal => [
                Rect::new(0, 0, first, context.height),
                Rect::new(first + self.divider.thickness, 0, second, context.height),
            ],
            Axis::Vertical => [
                Rect::new(0, 0, context.width, first),
                Rect::new(0, first + self.divider.thickness, context.width, second),
            ],
        }
    }
}

impl<M> Component for Split<'_, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let rects = self.layout(context);
        self.group.start(context, |index| rects[index])
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let (total, first) = self.extent(context);
        if let Some(key) = event.key() {
            if let Some(action) = self.bindings.action(key, true) {
                let step = match self.axis {
                    Axis::Horizontal => 2,
                    Axis::Vertical => 1,
                };
                match action.as_str() {
                    "shrink" => self.divider.nudge(-step, first, total),
                    "grow" => self.divider.nudge(step, first, total),
                    _ => {
                        if let Some(flow) = self.bindings.run(&action) {
                            return flow;
                        }
                    }
                }
                return Flow::Continue;
            }
        }
        if let Event::Mouse(mouse) = event {
            let along = match self.axis {
                Axis::Horizontal => mouse.column as usize,
                Axis::Vertical => mouse.row as usize,
            };
            if self.divider.handle_mouse(mouse, along, first, total) {
                self.group.grab = None;
                return Flow::Continue;
            }
        }
        let [a, b] = self.layout(context);
        let flow = self
            .group
            .route(event, context, &[(0, a), (1, b)], Rect::default());
        self.group.settle();
        if !matches!(flow, Flow::Ignored) {
            return flow;
        }
        let Some(action) = event.key().and_then(|key| self.bindings.action(key, false)) else {
            return Flow::Ignored;
        };
        match action.as_str() {
            FOCUS_NEXT | FOCUS_PREVIOUS => {
                if self.group.focus_step(action == FOCUS_NEXT) {
                    Flow::Continue
                } else {
                    Flow::Ignored
                }
            }
            _ => self.bindings.run(&action).unwrap_or(Flow::Ignored),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let [a, b] = self.layout(context);
        let focus = self.group.focused();
        let first = self.group.children[0].render(&a.context(context));
        let second = self.group.children[1].render(&b.context(context));
        match self.axis {
            Axis::Horizontal => {
                let rows = first
                    .lines
                    .len()
                    .max(second.lines.len())
                    .min(context.height)
                    .max(1);
                let border = vec![vec![kit::text("│", &self.style)]];
                side_by_side(&[(first, a), (second, b)], rows, focus, &border)
            }
            Axis::Vertical => {
                let mut out = View::new(rows_of(&first, a, true));
                out.lines
                    .push(vec![kit::text("─".repeat(context.width), &self.style)]);
                out.lines.extend(rows_of(&second, b, true));
                let (view, rect) = match focus {
                    Some(0) => (&first, a),
                    _ => (&second, b),
                };
                if let Some((row, column)) = view.cursor.filter(|(row, _)| *row < rect.height) {
                    out.cursor = Some((rect.y + row, column));
                }
                out
            }
        }
    }

    fn tick(&self) -> Option<Duration> {
        self.group.tick()
    }

    fn mouse(&self) -> bool {
        self.mouse || self.group.mouse()
    }

    fn keymap(&self) -> Keymap {
        let focused = self
            .group
            .focused()
            .map(|index| &self.group.children[index]);
        keymap_with(focused, &self.bindings.keymap)
    }

    fn focusable(&self) -> bool {
        self.group.focusable()
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        self.group.focus_step(forward)
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        self.group.focus_enter(forward)
    }
}

// ---------------------------------------------------------------------------
// Tabs.

/// Children one at a time, under a bar of their titles (#478). Every tab
/// keeps its state while another shows; hidden tabs still get ticks and
/// resizes. Alt+Right or Ctrl+PageDown shows the next tab, Alt+Left or
/// Ctrl+PageUp the previous, Alt+1 to Alt+9 a tab by number, and a click
/// on a title that tab. These come before the tab's own keys; Tab and
/// Shift+Tab move focus inside the tab shown.
pub struct Tabs<'a, M> {
    titles: Vec<String>,
    group: Group<'a, M>,
    active: usize,
    selected: Style,
    unselected: Style,
    bindings: Bindings<'a, M>,
    mouse: bool,
}

impl<'a, M> Default for Tabs<'a, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, M> Tabs<'a, M> {
    pub fn new() -> Tabs<'a, M> {
        let mut bindings = Bindings::new("tabs").focus_keys();
        bindings.declare("next", keys("alt+right ctrl+pagedown"), "next tab", true);
        bindings.declare(
            "previous",
            keys("alt+left ctrl+pageup"),
            "previous tab",
            true,
        );
        Tabs {
            titles: Vec::new(),
            group: Group::new(),
            active: 0,
            selected: Style::parse("bold reverse").expect("a built-in style"),
            unselected: Theme::default().hint,
            bindings,
            mouse: false,
        }
    }

    /// Add a tab.
    pub fn tab(mut self, title: impl Into<String>, child: impl Component<Output = M> + 'a) -> Self {
        self.push(title, Box::new(child));
        self
    }

    /// Add a tab to tabs already built.
    pub fn push(&mut self, title: impl Into<String>, child: Child<'a, M>) {
        self.titles.push(title.into());
        self.group.children.push(child);
        let number = self.titles.len();
        if number <= 9 {
            self.bindings.declare(
                &format!("tab-{number}"),
                keys(&format!("alt+{number}")),
                &format!("show tab {number}"),
                true,
            );
        }
    }

    /// The styles of the shown tab's title and the others'.
    pub fn styles(mut self, selected: Style, unselected: Style) -> Self {
        self.selected = selected;
        self.unselected = unselected;
        self
    }

    /// Show tab `index` first.
    pub fn active(mut self, index: usize) -> Self {
        self.select(index);
        self
    }

    binding_builders!();

    /// Show tab `index`.
    pub fn select(&mut self, index: usize) {
        if index < self.titles.len() {
            self.active = index;
            self.group.focus = Some(index);
        }
    }

    /// The tab shown.
    pub fn selected(&self) -> usize {
        self.active
    }

    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    pub fn len(&self) -> usize {
        self.titles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.titles.is_empty()
    }

    /// Tab `index`'s child.
    pub fn get(&self, index: usize) -> Option<&(dyn Component<Output = M> + 'a)> {
        self.group.children.get(index).map(|child| &**child)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut (dyn Component<Output = M> + 'a)> {
        self.group.children.get_mut(index).map(|child| &mut **child)
    }

    /// The tab bar at `width`, and each title's columns.
    fn bar(&self, width: usize) -> (Vec<Segment>, Vec<(usize, usize)>) {
        let mut line = Vec::new();
        let mut spans = Vec::new();
        let mut at = 0;
        for (index, title) in self.titles.iter().enumerate() {
            if index > 0 {
                line.push(kit::plain(" "));
                at += 1;
            }
            let label = format!(" {title} ");
            let cells = rich::cells::cell_len(&label);
            let style = if index == self.active {
                &self.selected
            } else {
                &self.unselected
            };
            line.push(kit::text(label, style));
            spans.push((at, at + cells));
            at += cells;
        }
        (kit::fit(line, width), spans)
    }

    /// Where the shown tab's child is.
    pub fn content(&self, context: &Context<'_>) -> Rect {
        Rect::new(0, 1, context.width, context.height.saturating_sub(1))
    }
}

impl<M> Component for Tabs<'_, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let rect = self.content(context);
        let flow = self.group.start(context, |_| rect);
        self.group.focus = Some(self.active);
        flow
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let count = self.titles.len();
        if let Some(key) = event.key() {
            if let Some(action) = self.bindings.action(key, true) {
                match action.as_str() {
                    "next" if count > 0 => self.select((self.active + 1) % count),
                    "previous" if count > 0 => self.select((self.active + count - 1) % count),
                    number if number.starts_with("tab-") => {
                        if let Ok(n) = number[4..].parse::<usize>() {
                            self.select(n.saturating_sub(1));
                        }
                    }
                    _ => {
                        if let Some(flow) = self.bindings.run(&action) {
                            return flow;
                        }
                    }
                }
                return Flow::Continue;
            }
        }
        if let Event::Mouse(mouse) = event {
            if mouse.row == 0 && self.group.grab.is_none() {
                if mouse.is_click() {
                    let (_, spans) = self.bar(context.width);
                    let column = mouse.column as usize;
                    if let Some(index) = spans.iter().position(|(a, b)| (*a..*b).contains(&column))
                    {
                        self.select(index);
                        return Flow::Continue;
                    }
                }
                return Flow::Ignored;
            }
        }
        let rect = self.content(context);
        self.group.focus = Some(self.active);
        let flow = self
            .group
            .route(event, context, &[(self.active, rect)], rect);
        if !matches!(flow, Flow::Ignored) {
            return flow;
        }
        let Some(action) = event.key().and_then(|key| self.bindings.action(key, false)) else {
            return Flow::Ignored;
        };
        match action.as_str() {
            FOCUS_NEXT | FOCUS_PREVIOUS => {
                let forward = action == FOCUS_NEXT;
                if self.group.children[self.active].focus_step(forward) {
                    Flow::Continue
                } else {
                    Flow::Ignored
                }
            }
            _ => self.bindings.run(&action).unwrap_or(Flow::Ignored),
        }
    }

    fn render(&self, context: &Context<'_>) -> View {
        let (bar, _) = self.bar(context.width);
        let mut out = View::new(vec![bar]);
        if let Some(child) = self.group.children.get(self.active) {
            let rect = self.content(context);
            let view = child.render(&rect.context(context));
            out.lines.extend(rows_of(&view, rect, false));
            if let Some((row, column)) = view.cursor.filter(|(row, _)| *row < rect.height) {
                out.cursor = Some((rect.y + row, column));
            }
        }
        out
    }

    fn tick(&self) -> Option<Duration> {
        self.group.tick()
    }

    fn mouse(&self) -> bool {
        self.mouse || self.group.mouse()
    }

    fn keymap(&self) -> Keymap {
        keymap_with(self.group.children.get(self.active), &self.bindings.keymap)
    }

    fn focusable(&self) -> bool {
        self.group
            .children
            .get(self.active)
            .is_some_and(|child| child.focusable())
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        let active = self.active;
        self.group
            .children
            .get_mut(active)
            .is_some_and(|child| child.focus_step(forward))
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        let active = self.active;
        self.group
            .children
            .get_mut(active)
            .is_some_and(|child| child.focus_enter(forward))
    }
}

// ---------------------------------------------------------------------------
// Layers.

/// How a [`Layer`] sits over what is under it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum LayerKind {
    /// Traps focus until dismissed, dims what is under it (a backdrop), and
    /// ignores clicks outside it.
    #[default]
    Modal,
    /// Takes the keys while open, but a click outside closes it, and there
    /// is no backdrop: a menu, a completion list.
    Popover,
}

/// Where a [`Layer`] goes, border included, cut to the space there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Placement {
    /// This many cells, centred: horizontally in the width, vertically in
    /// the base's rows (or the layer's own height, if taller).
    Center { width: usize, height: usize },
    /// At this rectangle of the host's view.
    At(Rect),
}

impl Default for Placement {
    fn default() -> Self {
        Placement::Center {
            width: 40,
            height: 10,
        }
    }
}

/// A component shown over a [`Layers`] host's base: a modal dialog or a
/// popover. It closes when it answers (its flow still goes up, so a mapped
/// `Flow::Done` finishes the host), when it cancels or Escape reaches the
/// host unused (it is dismissed, the host carries on), or through a
/// [`LayerHandle`].
pub struct Layer<'a, M> {
    child: Child<'a, M>,
    kind: LayerKind,
    placement: Placement,
    title: Option<String>,
    border: bool,
    backdrop: bool,
    dismissable: bool,
    /// Whether the child took focus when opened: a layer whose child stops
    /// taking focus (it answered) closes; one that never took it (a label)
    /// stays until dismissed.
    took_focus: bool,
}

impl<'a, M> Layer<'a, M> {
    fn new(kind: LayerKind, child: impl Component<Output = M> + 'a) -> Layer<'a, M> {
        Layer {
            child: Box::new(child),
            kind,
            placement: Placement::default(),
            title: None,
            border: true,
            backdrop: kind == LayerKind::Modal,
            dismissable: true,
            took_focus: false,
        }
    }

    /// A modal layer: see [`LayerKind::Modal`].
    pub fn modal(child: impl Component<Output = M> + 'a) -> Layer<'a, M> {
        Layer::new(LayerKind::Modal, child)
    }

    /// A popover: see [`LayerKind::Popover`].
    pub fn popover(child: impl Component<Output = M> + 'a) -> Layer<'a, M> {
        Layer::new(LayerKind::Popover, child)
    }

    /// Centred, `width` × `height` cells with the border (default 40 × 10).
    pub fn size(mut self, width: usize, height: usize) -> Self {
        self.placement = Placement::Center { width, height };
        self
    }

    pub fn placement(mut self, placement: Placement) -> Self {
        self.placement = placement;
        self
    }

    /// A title in the top border.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Draw a border round it (default: yes).
    pub fn border(mut self, on: bool) -> Self {
        self.border = on;
        self
    }

    /// Dim what is under it (default: for a modal).
    pub fn backdrop(mut self, on: bool) -> Self {
        self.backdrop = on;
        self
    }

    /// Whether Escape, reaching the host unused, closes it (default: yes).
    pub fn dismissable(mut self, on: bool) -> Self {
        self.dismissable = on;
        self
    }

    pub fn kind(&self) -> LayerKind {
        self.kind
    }
}

enum Request<'a, M> {
    Open(Layer<'a, M>),
    Close,
}

/// Opens and closes layers on a [`Layers`] host from anywhere: a binding's
/// handler, your own component, a timer. Requests take effect when the host
/// next handles an event (the one being handled, from inside it).
pub struct LayerHandle<'a, M> {
    requests: Rc<RefCell<Vec<Request<'a, M>>>>,
    open: Rc<Cell<usize>>,
}

impl<M> Clone for LayerHandle<'_, M> {
    fn clone(&self) -> Self {
        LayerHandle {
            requests: Rc::clone(&self.requests),
            open: Rc::clone(&self.open),
        }
    }
}

impl<'a, M> LayerHandle<'a, M> {
    /// Open `layer` over the others.
    pub fn open(&self, layer: Layer<'a, M>) {
        self.requests.borrow_mut().push(Request::Open(layer));
    }

    /// Close the top layer.
    pub fn close(&self) {
        self.requests.borrow_mut().push(Request::Close);
    }

    /// How many layers were open when the host last handled an event.
    pub fn open_count(&self) -> usize {
        self.open.get()
    }
}

/// A base component with [`Layer`]s over it (#480): modal dialogs and
/// popovers. The top layer takes the keys; a modal also traps focus (Tab
/// cycles inside it) and dims the base. Escape dismisses the top layer when
/// the layer does not use it, and a layer that cancels is dismissed rather
/// than cancelling the host. With no layer open, the host is its base.
pub struct Layers<'a, M> {
    base: Child<'a, M>,
    layers: Vec<Layer<'a, M>>,
    requests: Rc<RefCell<Vec<Request<'a, M>>>>,
    open: Rc<Cell<usize>>,
    backdrop: Style,
    border: Style,
    bindings: Bindings<'a, M>,
    mouse: bool,
    grab: Option<Option<usize>>,
    handoff: Option<Option<usize>>,
}

impl<'a, M> Layers<'a, M> {
    pub fn new(base: impl Component<Output = M> + 'a) -> Layers<'a, M> {
        let mut bindings = Bindings::new("layers").focus_keys();
        bindings.declare("dismiss", keys("escape"), "close the dialog", false);
        Layers {
            base: Box::new(base),
            layers: Vec::new(),
            requests: Rc::new(RefCell::new(Vec::new())),
            open: Rc::new(Cell::new(0)),
            backdrop: Style::parse("dim").expect("a built-in style"),
            border: Style::default(),
            bindings,
            mouse: false,
            grab: None,
            handoff: None,
        }
    }

    /// A handle that opens and closes layers here.
    pub fn handle(&self) -> LayerHandle<'a, M> {
        LayerHandle {
            requests: Rc::clone(&self.requests),
            open: Rc::clone(&self.open),
        }
    }

    /// Open `layer` now.
    pub fn open(&mut self, mut layer: Layer<'a, M>) {
        layer.took_focus = layer.child.focusable();
        self.layers.push(layer);
        self.open.set(self.layers.len());
    }

    /// Close the top layer; `false` when none is open.
    pub fn close(&mut self) -> bool {
        let closed = self.layers.pop().is_some();
        self.open.set(self.layers.len());
        closed
    }

    /// Open a layer made by `layer` when one of `keys` reaches the host
    /// unused, with no layer open: a help dialog on `?`, a palette on
    /// Ctrl+P.
    pub fn open_on(
        mut self,
        action: &str,
        keys: impl IntoIterator<Item = Key>,
        description: &str,
        mut layer: impl FnMut() -> Layer<'a, M> + 'a,
    ) -> Self
    where
        M: 'a,
    {
        let handle = self.handle();
        let keys = keys.into_iter().collect();
        self.bindings.on(action, keys, description, false, move || {
            handle.open(layer());
            Flow::Continue
        });
        self
    }

    /// The style put over the base under a modal (default: dim).
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop = style;
        self
    }

    /// The style of a layer's border (default: none).
    pub fn border_style(mut self, style: Style) -> Self {
        self.border = style;
        self
    }

    binding_builders!();

    /// How many layers are open.
    pub fn depth(&self) -> usize {
        self.layers.len()
    }

    pub fn base(&self) -> &(dyn Component<Output = M> + 'a) {
        &*self.base
    }

    pub fn base_mut(&mut self) -> &mut (dyn Component<Output = M> + 'a) {
        &mut *self.base
    }

    /// Carry out the handles' requests; new layers start at `context`.
    fn apply(&mut self, context: &Context<'_>) -> Flow<M> {
        let requests: Vec<Request<'a, M>> = self.requests.borrow_mut().drain(..).collect();
        let mut result = Flow::Continue;
        for request in requests {
            match request {
                Request::Open(mut layer) => {
                    layer.took_focus = layer.child.focusable();
                    self.layers.push(layer);
                    let index = self.layers.len() - 1;
                    let inner = self.placements(context)[index].1;
                    let flow = self.layers[index].child.start(&inner.context(context));
                    if let Some(flow) = self.after_layer(index, flow) {
                        result = flow;
                    }
                }
                Request::Close => {
                    self.layers.pop();
                }
            }
        }
        self.open.set(self.layers.len());
        result
    }

    /// Each open layer's outer and inner rectangles at `context`.
    pub fn placements(&self, context: &Context<'_>) -> Vec<(Rect, Rect)> {
        if self.layers.is_empty() {
            return Vec::new();
        }
        let rows = self.base.render(context).lines.len();
        self.layers
            .iter()
            .map(|layer| {
                let outer = match layer.placement {
                    Placement::Center { width, height } => {
                        let width = width.min(context.width);
                        let height = height.min(context.height.max(1));
                        let area = rows.max(height).min(context.height.max(height));
                        Rect::new(
                            (context.width - width) / 2,
                            (area - height) / 2,
                            width,
                            height,
                        )
                    }
                    Placement::At(rect) => {
                        let x = rect.x.min(context.width);
                        Rect::new(x, rect.y, rect.width.min(context.width - x), rect.height)
                    }
                };
                let inner = if layer.border && outer.width >= 2 && outer.height >= 2 {
                    Rect::new(outer.x + 1, outer.y + 1, outer.width - 2, outer.height - 2)
                } else {
                    outer
                };
                (outer, inner)
            })
            .collect()
    }

    /// Close layer `index` if its flow ends it; the flow the host returns.
    fn after_layer(&mut self, index: usize, flow: Flow<M>) -> Option<Flow<M>> {
        let layer = &self.layers[index];
        let finished = layer.took_focus && !layer.child.focusable();
        match flow {
            Flow::Cancel => {
                self.layers.remove(index);
                Some(Flow::Continue)
            }
            Flow::Done(value) => {
                self.layers.remove(index);
                Some(Flow::Done(value))
            }
            Flow::Handoff(command) => {
                self.handoff = Some(Some(index));
                Some(Flow::Handoff(command))
            }
            flow if finished => {
                self.layers.remove(index);
                Some(match flow {
                    Flow::Ignored => Flow::Continue,
                    flow => flow,
                })
            }
            _ => None,
        }
    }

    fn handle_top(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let top = self.layers.len() - 1;
        let places = self.placements(context);
        let (outer, inner) = places[top];
        match event {
            Event::Tick | Event::Resize { .. } => {
                let flow = self.broadcast(event, context, &places);
                return flow;
            }
            Event::Returned(_) => {
                return match self.handoff.take() {
                    Some(None) => self.base.handle(event, context),
                    Some(Some(index)) if index < self.layers.len() => {
                        let inner = places[index].1;
                        let flow = self.layers[index]
                            .child
                            .handle(event, &inner.context(context));
                        self.after_layer(index, flow).unwrap_or(Flow::Continue)
                    }
                    _ => Flow::Continue,
                };
            }
            Event::Mouse(mouse) => {
                let (column, row) = (mouse.column as usize, mouse.row as usize);
                let inside = outer.contains(column, row);
                let held = matches!(mouse.kind, MouseKind::Drag(_) | MouseKind::Up(_))
                    && self.grab == Some(Some(top));
                if let MouseKind::Down(_) = mouse.kind {
                    // Outside: held by nobody, so its release goes nowhere.
                    self.grab = Some(Some(if inside { top } else { usize::MAX }));
                }
                if !inside && !held {
                    if mouse.is_click() && self.layers[top].kind == LayerKind::Popover {
                        self.layers.pop();
                    }
                    if let MouseKind::Up(_) = mouse.kind {
                        self.grab = None;
                    }
                    return Flow::Continue;
                }
                if matches!(mouse.kind, MouseKind::Up(_)) {
                    self.grab = None;
                }
                if !self.layers[top].child.mouse() || !(inner.contains(column, row) || held) {
                    return Flow::Continue;
                }
                let event = Event::Mouse(inner.local(*mouse));
                let flow = self.layers[top]
                    .child
                    .handle(&event, &inner.context(context));
                return self.after_layer(top, flow).unwrap_or(Flow::Continue);
            }
            _ => {}
        }
        let flow = self.layers[top]
            .child
            .handle(event, &inner.context(context));
        let ignored = matches!(flow, Flow::Ignored);
        if let Some(flow) = self.after_layer(top, flow) {
            return flow;
        }
        if !ignored {
            return Flow::Continue;
        }
        // Unused by the layer: dismiss, move focus inside it, or stop here.
        let Some(action) = event.key().and_then(|key| self.bindings.action(key, false)) else {
            return Flow::Continue;
        };
        match action.as_str() {
            "dismiss" if self.layers[top].dismissable => {
                self.layers.pop();
            }
            FOCUS_NEXT | FOCUS_PREVIOUS => {
                let forward = action == FOCUS_NEXT;
                let child = &mut self.layers[top].child;
                if !child.focus_step(forward) {
                    child.focus_enter(forward);
                }
            }
            _ => {}
        }
        Flow::Continue
    }

    fn broadcast(
        &mut self,
        event: &Event,
        context: &Context<'_>,
        places: &[(Rect, Rect)],
    ) -> Flow<M> {
        if !matches!(event, Event::Tick) || self.base.tick().is_some() {
            let flow = self.base.handle(event, context);
            if decisive(&flow) {
                return flow;
            }
        }
        let mut index = 0;
        while index < self.layers.len() {
            if matches!(event, Event::Tick) && self.layers[index].child.tick().is_none() {
                index += 1;
                continue;
            }
            let inner = places
                .get(index)
                .map_or(Rect::default(), |(_, inner)| *inner);
            let flow = self.layers[index]
                .child
                .handle(event, &inner.context(context));
            let count = self.layers.len();
            if let Some(flow) = self.after_layer(index, flow) {
                if decisive(&flow) {
                    return flow;
                }
            }
            if self.layers.len() == count {
                index += 1;
            }
        }
        Flow::Continue
    }

    /// The layer's box: its border and title round its view.
    fn boxed(
        &self,
        layer: &Layer<'a, M>,
        view: &View,
        outer: Rect,
        inner: Rect,
    ) -> Vec<Vec<Segment>> {
        let mut lines = rows_of(view, inner, true)
            .into_iter()
            .map(|line| kit::pad(line, inner.width))
            .collect::<Vec<_>>();
        if !(layer.border && inner != outer) {
            return lines;
        }
        let style = &self.border;
        let span = inner.width;
        let mut top = vec![kit::text("╭", style)];
        match &layer.title {
            Some(title) if span >= 4 => {
                let title = kit::fit(vec![kit::text(format!(" {title} "), style)], span - 1);
                let used = kit::width(&title) + 1;
                top.push(kit::text("─", style));
                top.extend(title);
                top.push(kit::text("─".repeat(span.saturating_sub(used)), style));
            }
            _ => top.push(kit::text("─".repeat(span), style)),
        }
        top.push(kit::text("╮", style));
        for line in &mut lines {
            let mut boxed = vec![kit::text("│", style)];
            boxed.append(line);
            boxed.push(kit::text("│", style));
            *line = boxed;
        }
        lines.insert(0, top);
        lines.push(vec![
            kit::text("╰", style),
            kit::text("─".repeat(span), style),
            kit::text("╯", style),
        ]);
        lines
    }
}

impl<M> Component for Layers<'_, M> {
    type Output = M;

    fn start(&mut self, context: &Context<'_>) -> Flow<M> {
        let flow = self.base.start(context);
        if decisive(&flow) {
            return flow;
        }
        self.apply(context)
    }

    fn handle(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        let before = self.apply(context);
        if decisive(&before) {
            return before;
        }
        let flow = if self.layers.is_empty() {
            self.handle_base(event, context)
        } else {
            self.handle_top(event, context)
        };
        self.open.set(self.layers.len());
        if decisive(&flow) {
            return flow;
        }
        let after = self.apply(context);
        if decisive(&after) {
            return after;
        }
        flow
    }

    fn render(&self, context: &Context<'_>) -> View {
        let base = self.base.render(context);
        if self.layers.is_empty() {
            return base;
        }
        let places = self.placements(context);
        let mut lines = base.lines;
        let mut cursor = None;
        for (index, layer) in self.layers.iter().enumerate() {
            let (outer, inner) = places[index];
            if layer.backdrop {
                for line in &mut lines {
                    *line = kit::restyle(line, &self.backdrop);
                }
            }
            let view = layer.child.render(&inner.context(context));
            let boxed = self.boxed(layer, &view, outer, inner);
            if lines.len() < outer.y + boxed.len() {
                lines.resize_with(outer.y + boxed.len(), Vec::new);
            }
            for (row, top) in boxed.iter().enumerate() {
                let line = &mut lines[outer.y + row];
                *line = kit::overlay(line, outer.x, top);
            }
            cursor = view
                .cursor
                .filter(|(row, column)| *row < inner.height && *column < inner.width.max(1))
                .map(|(row, column)| (inner.y + row, inner.x + column));
        }
        View { lines, cursor }
    }

    fn tick(&self) -> Option<Duration> {
        self.layers
            .iter()
            .filter_map(|layer| layer.child.tick())
            .chain(self.base.tick())
            .min()
    }

    fn mouse(&self) -> bool {
        self.mouse || self.base.mouse() || self.layers.iter().any(|layer| layer.child.mouse())
    }

    fn keymap(&self) -> Keymap {
        match self.layers.last() {
            Some(layer) => keymap_with(Some(&layer.child), &self.bindings.keymap),
            None => keymap_with(Some(&self.base), &self.bindings.keymap),
        }
    }

    fn focusable(&self) -> bool {
        match self.layers.last() {
            Some(_) => true,
            None => self.base.focusable(),
        }
    }

    fn focus_step(&mut self, forward: bool) -> bool {
        match self.layers.last_mut() {
            // Focus stays in a layer: past its last stop, round to its first.
            Some(layer) => {
                if !layer.child.focus_step(forward) {
                    layer.child.focus_enter(forward);
                }
                true
            }
            None => self.base.focus_step(forward),
        }
    }

    fn focus_enter(&mut self, forward: bool) -> bool {
        match self.layers.last_mut() {
            Some(layer) => {
                layer.child.focus_enter(forward);
                true
            }
            None => self.base.focus_enter(forward),
        }
    }

    fn default_value(&self) -> Option<M> {
        self.base.default_value()
    }

    fn prompt(&mut self, io: &mut dyn LineIo) -> Result<Option<M>, NotInteractive> {
        self.base.prompt(io)
    }
}

impl<M> Layers<'_, M> {
    fn handle_base(&mut self, event: &Event, context: &Context<'_>) -> Flow<M> {
        if let Some(key) = event.key() {
            if let Some(action) = self.bindings.action(key, true) {
                if let Some(flow) = self.bindings.run(&action) {
                    return flow;
                }
            }
        }
        if let Event::Mouse(mouse) = event {
            if !self.base.mouse() {
                return Flow::Ignored;
            }
            match mouse.kind {
                MouseKind::Down(_) => self.grab = Some(None),
                // A press that started on a layer (one since closed) is
                // not the base's to finish.
                MouseKind::Drag(_) | MouseKind::Up(_) if matches!(self.grab, Some(Some(_))) => {
                    if matches!(mouse.kind, MouseKind::Up(_)) {
                        self.grab = None;
                    }
                    return Flow::Continue;
                }
                MouseKind::Up(_) => self.grab = None,
                _ => {}
            }
        }
        if let Event::Returned(_) = event {
            self.handoff = None;
        }
        let flow = self.base.handle(event, context);
        if let Flow::Handoff(_) = flow {
            self.handoff = Some(None);
        }
        if !matches!(flow, Flow::Ignored) {
            return flow;
        }
        let Some(action) = event.key().and_then(|key| self.bindings.action(key, false)) else {
            return Flow::Ignored;
        };
        match action.as_str() {
            FOCUS_NEXT | FOCUS_PREVIOUS | "dismiss" => Flow::Ignored,
            _ => self.bindings.run(&action).unwrap_or(Flow::Ignored),
        }
    }
}

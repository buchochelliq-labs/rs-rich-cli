//! Nodes: the retained tree an app is made of.
//!
//! A [`Node`] is built once and kept. It remembers the rectangle it was
//! laid out in, so the app can route a click to it without the author
//! storing anything, and it draws again only when a [signal](crate::signal)
//! it read changed, or it moved. A frame writes the nodes that drew into
//! the [`Screen`] and reports their rectangles as damage.
//!
//! Builders:
//!
//! - [`text`](fn@text) and the [`text!`](crate::text!) macro: console markup that may
//!   read signals; [`label`] for markup that never changes;
//! - [`renderable`]: any rich renderable (a `Table`, `Markdown`, `Syntax`,
//!   a chart), rebuilt when the signals it read change;
//! - [`column`](fn@column) and [`row`]: children laid out along an axis, each sized
//!   [`Size::Fixed`], [`Size::Percent`], [`Size::Flex`] or
//!   [`Size::Auto`], within [`Node::min_size`] and [`Node::max_size`];
//! - [`grid`]: children in rows and columns, spanning several with
//!   [`Node::span`];
//! - [`each`]: one child per key of a list, kept by key;
//! - [`switch`]: one child at a time, chosen by a key, each kept while
//!   hidden (tabs, wizard steps);
//! - [`Node::panel`]: a rounded border with a title, highlighted while the
//!   focus is inside it; [`Node::padding`] for space round a node.
//!
//! and behaviour: [`Node::on_key`] binds keys (they bubble from the focused
//! node to its ancestors), [`Node::on_click`] handles clicks, and
//! [`Node::focusable`] puts a node in the Tab order.
//!
//! Every node is a [`Widget`]: the builders above make built-in ones, and
//! [`widget`](crate::widget()) makes a node of your own.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::rc::Rc;

use rich::{Console, Renderable, Segment, Style};
use rich_interact::{Component, Key, Mouse};

use crate::app::Ctx;
use crate::builtin::{Each, Grid, Host, HostWidget, Leaf, Pad, Panel, ScrollView, Stack, Switch};
pub use crate::layout::Size;
use crate::layout::Track;
use crate::reactive::{next_node, NodeId, Runtime, Signal};
use crate::screen::{Rect, Screen};
use crate::sheet::{BoxKind, Dock, Element, Layout, States, Stylesheet};
use crate::widget::{Canvas, DrawCx, MeasureCx, ScrollCx, Watchers, Widget};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

pub(crate) type Handler = Box<dyn FnMut(&mut Ctx)>;
/// A mouse handler: the event in the node's coordinates; whether it used
/// it.
pub(crate) type MouseHandler = Box<dyn FnMut(&mut Ctx, Mouse) -> bool>;

/// What a drop target does with a value dropped on it.
pub(crate) type DropHandler = Box<dyn FnMut(&dyn Any, &mut Ctx)>;

/// What a node takes when something is dropped on it: the type it accepts,
/// and what it does with the value.
pub(crate) struct DropTarget {
    pub accepts: fn(&dyn Any) -> bool,
    pub handler: RefCell<DropHandler>,
}

/// One node of the tree: a [`Widget`] with a place in the layout, key
/// bindings and handlers.
pub struct Node {
    pub(crate) id: NodeId,
    pub(crate) size: Size,
    pub(crate) min: u16,
    pub(crate) max: u16,
    /// Columns and rows it spans in a [`grid`].
    pub(crate) span: (u16, u16),
    pub(crate) body: RefCell<Body>,
    /// Where it was last laid out.
    pub(crate) rect: Cell<Rect>,
    /// Where it last drew; `None` before its first draw, or while hidden.
    drawn: Cell<Option<Rect>>,
    /// Whether it was last drawn in its focus style.
    highlit: Cell<bool>,
    pub(crate) focusable: bool,
    pub(crate) keys: RefCell<Vec<(Vec<Key>, String, Handler)>>,
    pub(crate) click: RefCell<Option<Handler>>,
    pub(crate) mouse: RefCell<Option<MouseHandler>>,
    /// Where the widget wants the text caret, on its surface.
    pub(crate) caret: Cell<Option<(u16, u16)>>,
    /// What the inspector calls it: its [`name`](Node::name), else the
    /// builder that made it.
    name: Option<String>,
    what: &'static str,
    /// A style (or theme style name) laid over the node while it has the
    /// focus.
    focus_style: Option<String>,
    /// The same, while the pointer is over it.
    hover_style: Option<String>,
    /// Whether it was last drawn in its hover style.
    hover_lit: Cell<bool>,
    /// Markup shown when the pointer rests on it, or F1 is pressed while it
    /// has the focus.
    pub(crate) tooltip: Option<String>,
    /// The value a drag from this node carries.
    pub(crate) drag: Option<Rc<dyn Any>>,
    pub(crate) drop: Option<DropTarget>,
    /// What code set, which a stylesheet leaves alone: the size, the
    /// minimum, the maximum, the gap and a grid's rows.
    set: [bool; 5],
    /// Its classes, for stylesheets: fixed ones, and ones that hold while a
    /// condition does.
    classes: Vec<String>,
    class_when: Vec<(String, Condition)>,
    selected_when: Option<Condition>,
    disabled_when: Option<Condition>,
    /// What the stylesheet decided for it.
    sheet: RefCell<SheetNode>,
    /// Where its widget lays out and draws: its rectangle less the
    /// stylesheet's border and padding.
    pub(crate) inner: Cell<Rect>,
}

/// A condition a node's class or state follows.
type Condition = Box<dyn Fn() -> bool>;

const SET_SIZE: usize = 0;
const SET_MIN: usize = 1;
const SET_MAX: usize = 2;
const SET_GAP: usize = 3;
const SET_ROWS: usize = 4;

/// A node's part of the stylesheet.
#[derive(Default)]
struct SheetNode {
    /// The sheet it was matched against (0: none yet).
    generation: u64,
    layout: Layout,
    /// Whether a rule with a state could match it, and which states.
    watch: (bool, bool, bool),
    /// The border it draws round its widget (a panel draws its own).
    decor: Option<(BoxKind, Style, String)>,
    insets: [u16; 4],
    /// The style it was last drawn in.
    style: Option<Style>,
    /// Its conditional classes that held, and whether it was selected and
    /// disabled, when it last drew.
    active: Vec<String>,
    selected: bool,
    disabled: bool,
}

/// The stylesheet as a frame draws with it.
pub(crate) struct SheetCx<'a> {
    pub sheet: &'a Stylesheet,
    pub generation: u64,
    pub vars: &'a dyn Fn(&str) -> Option<Style>,
}

/// A node's widget, and what the framework keeps for it between frames.
pub(crate) struct Body {
    pub widget: Box<dyn Widget>,
    /// Where its children were laid out last, and which children.
    areas: Vec<Rect>,
    laid: Vec<NodeId>,
    /// A viewport's content, drawn offscreen, and what of it is shown.
    view: Option<View>,
}

/// A [viewport](Widget::viewport)'s offscreen content.
struct View {
    buffer: Screen,
    /// The window on the widget's surface, and the content's point at its
    /// top left, when it was last copied out.
    shown: Option<(Rect, (u16, u16))>,
}

impl Node {
    pub(crate) fn from_widget(widget: Box<dyn Widget>, what: &'static str) -> Node {
        Node {
            id: next_node(),
            size: Size::Flex(1),
            min: 0,
            max: u16::MAX,
            span: (1, 1),
            body: RefCell::new(Body {
                widget,
                areas: Vec::new(),
                laid: Vec::new(),
                view: None,
            }),
            rect: Cell::new(Rect::default()),
            drawn: Cell::new(None),
            highlit: Cell::new(false),
            focusable: false,
            keys: RefCell::new(Vec::new()),
            click: RefCell::new(None),
            mouse: RefCell::new(None),
            caret: Cell::new(None),
            name: None,
            what,
            focus_style: None,
            hover_style: None,
            hover_lit: Cell::new(false),
            tooltip: None,
            drag: None,
            drop: None,
            set: [false; 5],
            classes: Vec::new(),
            class_when: Vec::new(),
            selected_when: None,
            disabled_when: None,
            sheet: RefCell::new(SheetNode::default()),
            inner: Cell::new(Rect::default()),
        }
    }

    fn what(mut self, what: &'static str) -> Node {
        self.what = what;
        self
    }

    /// Name this node for the [inspector](crate::App::inspector).
    pub fn name(mut self, name: &str) -> Node {
        self.name = Some(name.to_string());
        self
    }

    /// Its name, if it was given one: how the command palette and the help
    /// group its bindings.
    pub(crate) fn label(&self) -> String {
        self.name.clone().unwrap_or_default()
    }

    /// How the inspector shows this node: its name or builder, and what it
    /// holds.
    pub(crate) fn describe(&self) -> String {
        let classes: String = self.classes.iter().map(|c| format!(".{c}")).collect();
        let base = match &self.name {
            Some(name) => format!("{name} ({}{classes})", self.what),
            None => format!("{}{classes}", self.what),
        };
        match self.body.borrow().widget.describe() {
            Some(more) => format!("{base} {more}"),
            None => base,
        }
    }

    /// The widget, if it is a `W`.
    fn widget_mut<W: Widget, R>(&self, f: impl FnOnce(&mut W) -> R) -> Option<R> {
        let mut body = self.body.borrow_mut();
        let widget: &mut dyn std::any::Any = &mut *body.widget;
        widget.downcast_mut::<W>().map(f)
    }

    /// Size this node along its parent's axis.
    pub fn size(mut self, size: Size) -> Node {
        self.size = size;
        self.set[SET_SIZE] = true;
        self
    }

    /// Exactly `cells` along the parent's axis.
    pub fn fixed(self, cells: u16) -> Node {
        self.size(Size::Fixed(cells))
    }

    /// A flexible share, by `weight`, of the space left over.
    pub fn flex(self, weight: u16) -> Node {
        self.size(Size::Flex(weight))
    }

    /// `percent` of the parent's axis.
    pub fn percent(self, percent: u16) -> Node {
        self.size(Size::Percent(percent))
    }

    /// As much of the parent's axis as the content needs (see
    /// [`Size::Auto`]).
    pub fn auto(self) -> Node {
        self.size(Size::Auto)
    }

    /// Never less than `cells` along the parent's axis, while there is
    /// room.
    pub fn min_size(mut self, cells: u16) -> Node {
        self.min = cells;
        self.set[SET_MIN] = true;
        self
    }

    /// Never more than `cells` along the parent's axis.
    pub fn max_size(mut self, cells: u16) -> Node {
        self.max = cells;
        self.set[SET_MAX] = true;
        self
    }

    /// In a [`grid`]: span `columns` columns and `rows` rows.
    pub fn span(mut self, columns: u16, rows: u16) -> Node {
        self.span = (columns.max(1), rows.max(1));
        self
    }

    /// For a [`column`](fn@column), [`row`] or [`grid`]: leave `cells` empty
    /// between neighbouring children. Ignored on other nodes.
    pub fn gap(mut self, cells: u16) -> Node {
        self.widget_mut(|stack: &mut Stack| stack.gap = cells);
        self.widget_mut(|grid: &mut Grid| grid.gap = (cells, cells));
        self.set[SET_GAP] = true;
        self
    }

    /// For a [`grid`]: the rows' sizes. Rows past the last one given take
    /// its size, so `.rows([Size::Auto])` makes every row as tall as its
    /// content; without any, rows share the height evenly. Ignored on other
    /// nodes.
    pub fn rows(mut self, rows: impl IntoIterator<Item = Size>) -> Node {
        let rows: Vec<Size> = rows.into_iter().collect();
        self.widget_mut(|grid: &mut Grid| grid.rows = rows);
        self.set[SET_ROWS] = true;
        self
    }

    /// Give this node `classes` (space-separated), for a
    /// [stylesheet](crate::App::stylesheet) to match with `.name`.
    pub fn class(mut self, classes: &str) -> Node {
        self.classes
            .extend(classes.split_whitespace().map(str::to_string));
        self
    }

    /// Give this node `class` while `condition` holds: a stylesheet's
    /// colours and text styles for `.class` follow it (its layout does not;
    /// that comes from fixed classes). `condition` may read signals.
    ///
    /// ```
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let load = signal(95);
    ///     text!("load {load}").class_when("danger", move || load.get() > 90)
    /// })
    /// .stylesheet(".danger { text-style: bold; }");
    /// ```
    pub fn class_when(mut self, class: &str, condition: impl Fn() -> bool + 'static) -> Node {
        self.class_when
            .push((class.to_string(), Box::new(condition)));
        self
    }

    /// Match a stylesheet's `:selected` while `condition` holds.
    pub fn selected_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.selected_when = Some(Box::new(condition));
        self
    }

    /// Match a stylesheet's `:disabled` while `condition` holds. A disabled
    /// node also leaves the Tab order, and its click and key handlers do
    /// not run.
    pub fn disabled_when(mut self, condition: impl Fn() -> bool + 'static) -> Node {
        self.disabled_when = Some(Box::new(condition));
        self
    }

    /// Whether it is [disabled](Self::disabled_when) now. Callers outside a
    /// draw read it untracked.
    pub(crate) fn disabled(&self) -> bool {
        self.disabled_when
            .as_ref()
            .is_some_and(|condition| condition())
    }

    /// Whether it can take the focus now.
    pub(crate) fn takes_focus(&self) -> bool {
        self.focusable && !self.disabled()
    }

    /// Put this node in the Tab order, so it can hold the focus.
    pub fn focusable(mut self) -> Node {
        self.focusable = true;
        self
    }

    /// Take this node out of the Tab order: a [`list`] that only shows, or
    /// a component you drive yourself.
    pub fn no_focus(mut self) -> Node {
        self.focusable = false;
        self
    }

    /// While this node has the focus, draw it in `style` over its own: a
    /// style (`"reverse"`, `"on grey23"`) or a theme style name
    /// (`"accent"`). Its whole rectangle takes the style, its children's
    /// cells included, so a list shows which row is selected even when a
    /// row is a [`row`] of several parts. The node is also made
    /// [focusable](Self::focusable).
    pub fn focus_style(mut self, style: &str) -> Node {
        self.focus_style = Some(style.to_string());
        self.focusable = true;
        self
    }

    /// While the pointer is over this node (or a node inside it), draw it
    /// in `style` over its own, as [`focus_style`](Self::focus_style) does
    /// for the focus. A [stylesheet](crate::App::stylesheet)'s `:hover`
    /// rules do the same from outside the code.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| column([label("one").hover_style("reverse"), label("two")]));
    /// let mut driver = app.driver(10, 2);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0)));
    /// driver.update(Duration::ZERO);
    /// let frame = driver.render().expect("the hover redraws");
    /// assert!(frame.contains("\x1b[0;7m"), "{frame:?}");
    /// ```
    pub fn hover_style(mut self, style: &str) -> Node {
        self.hover_style = Some(style.to_string());
        self
    }

    /// Show `markup` in a small box by the pointer once it has rested on
    /// this node (or a node inside it without a tooltip of its own) for
    /// 600 ms, or below the node at once when F1 is pressed while it has
    /// the focus and nothing binds F1. A key, a click, or the pointer
    /// moving to another node hides it.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| label("save").tooltip("Write the file to disk"));
    /// let mut driver = app.driver(30, 3);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// driver.event(Event::Mouse(Mouse::new(MouseKind::Moved, 1, 0)));
    /// driver.update(Duration::from_millis(700));
    /// let _ = driver.render();
    /// assert!(driver.screen().plain()[1].contains("Write the file to disk"));
    /// ```
    pub fn tooltip(mut self, markup: impl Into<String>) -> Node {
        self.tooltip = Some(markup.into());
        self
    }

    /// Let this node be dragged with the left button, carrying `value` to
    /// a node that takes it with [`on_drop`](Self::on_drop). A press still
    /// focuses and clicks as before; the drag starts once the pointer moves
    /// a cell. While it lasts the node is dimmed and the drop target under
    /// the pointer is highlighted (the theme's `drop.target` style, reverse
    /// unless set); Esc cancels it.
    ///
    /// ```
    /// use std::time::Duration;
    ///
    /// use intuituive::interact::{Button, Event, Mouse, MouseKind};
    /// use intuituive::prelude::*;
    ///
    /// let app = App::new(|| {
    ///     let done = signal(Vec::<String>::new());
    ///     column([
    ///         label("task").draggable("task".to_string()),
    ///         text(move || format!("done: {}", done.get().join(", ")))
    ///             .on_drop(move |task: &String, _| done.update(|d| d.push(task.clone()))),
    ///     ])
    /// });
    /// let mut driver = app.driver(20, 2);
    /// driver.update(Duration::ZERO);
    /// let _ = driver.render();
    /// let mut send = |kind, row| {
    ///     driver.event(Event::Mouse(Mouse::new(kind, 1, row)));
    ///     driver.update(Duration::ZERO);
    ///     let _ = driver.render();
    /// };
    /// send(MouseKind::Down(Button::Left), 0);
    /// send(MouseKind::Drag(Button::Left), 1);
    /// send(MouseKind::Up(Button::Left), 1);
    /// assert_eq!(driver.screen().plain()[1].trim_end(), "done: task");
    /// ```
    pub fn draggable<T: 'static>(mut self, value: T) -> Node {
        self.drag = Some(Rc::new(value));
        self
    }

    /// Take values of type `T` dropped on this node (or on a node inside
    /// it that does not take them itself): `handler` gets the value a
    /// [`draggable`](Self::draggable) node carried. A drag carrying another
    /// type passes this node by: it is not highlighted, and nothing drops.
    pub fn on_drop<T: 'static>(mut self, mut handler: impl FnMut(&T, &mut Ctx) + 'static) -> Node {
        self.drop = Some(DropTarget {
            accepts: |value| value.is::<T>(),
            handler: RefCell::new(Box::new(move |value, cx| {
                if let Some(value) = value.downcast_ref::<T>() {
                    handler(value, cx);
                }
            })),
        });
        self
    }

    /// Run `handler` when one of `keys` (space-separated names: `"q"`,
    /// `"ctrl+s"`, `"up k"`) is pressed while this node or a node inside it
    /// has the focus, and nothing deeper used the key. The binding also
    /// shows in the app's help.
    pub fn on_key(self, keys: &str, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.bind(keys, "", handler)
    }

    /// [`on_key`](Self::on_key) with a description for the help.
    pub fn bind(
        self,
        keys: &str,
        description: &str,
        handler: impl FnMut(&mut Ctx) + 'static,
    ) -> Node {
        let parsed = parse_keys(keys);
        self.keys
            .borrow_mut()
            .push((parsed, description.to_string(), Box::new(handler)));
        self
    }

    /// Run `handler` when this node is clicked. A clickable node is also
    /// focusable, and a click focuses it.
    pub fn on_click(mut self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        *self.click.borrow_mut() = Some(Box::new(handler));
        self.focusable = true;
        self
    }

    /// Run `handler` for every mouse event over this node (presses, releases,
    /// drags, movement, the wheel), in the node's own coordinates, before
    /// its ancestors see it. It returns whether it used the event; one it
    /// did not use bubbles up. A press does not focus the node unless it is
    /// [focusable](Self::focusable).
    pub fn on_mouse(self, handler: impl FnMut(&mut Ctx, Mouse) -> bool + 'static) -> Node {
        *self.mouse.borrow_mut() = Some(Box::new(handler));
        self
    }

    /// For a [`component`] node: run `handler` when the component is
    /// cancelled (Esc). Without one, the cancelling key bubbles to the
    /// node's ancestors like any unused key. Ignored on other nodes.
    pub fn on_cancel(self, handler: impl FnMut(&mut Ctx) + 'static) -> Node {
        self.widget_mut(|host: &mut HostWidget| host.host.set_cancel(Box::new(handler)));
        self
    }

    /// Wrap this node in a rounded border with `title`. The border is
    /// highlighted while the focus is on the node or inside it. The panel
    /// takes the node's size.
    pub fn panel(self, title: &str) -> Node {
        let title = title.to_string();
        self.wrap(|child| Box::new(Panel::new(title, child)), "panel")
    }

    /// Leave `vertical` empty rows above and below this node, and
    /// `horizontal` empty columns either side.
    pub fn padding(self, vertical: u16, horizontal: u16) -> Node {
        self.wrap(
            |child| {
                Box::new(Pad {
                    child,
                    edges: [vertical, horizontal, vertical, horizontal],
                })
            },
            "padding",
        )
    }

    /// A node of `widget` round this one, taking over its size and span.
    fn wrap(self, widget: impl FnOnce(Node) -> Box<dyn Widget>, what: &'static str) -> Node {
        let (size, min, max, span, set) = (self.size, self.min, self.max, self.span, self.set);
        let mut outer = Node::from_widget(widget(self), what);
        outer.size = size;
        outer.min = min;
        outer.max = max;
        outer.span = span;
        outer.set[SET_SIZE] = set[SET_SIZE];
        outer.set[SET_MIN] = set[SET_MIN];
        outer.set[SET_MAX] = set[SET_MAX];
        outer
    }

    /// Where the node was last laid out.
    pub fn rect(&self) -> Rect {
        self.rect.get()
    }

    pub fn id(&self) -> NodeId {
        self.id
    }

    /// How it asks to be sized along its parent's axis: as code set it,
    /// else as the stylesheet does.
    pub fn size_hint(&self) -> Size {
        self.dims().0
    }

    /// Its size, minimum and maximum: what code set, else what the
    /// stylesheet says; nothing at all when the sheet hides it.
    fn dims(&self) -> (Size, u16, u16) {
        let sheet = self.sheet.borrow();
        let layout = &sheet.layout;
        if layout.hidden {
            return (Size::Fixed(0), 0, 0);
        }
        fn pick<T>(set: bool, own: T, from: Option<T>) -> T {
            if set {
                own
            } else {
                from.unwrap_or(own)
            }
        }
        (
            pick(self.set[SET_SIZE], self.size, layout.size),
            pick(self.set[SET_MIN], self.min, layout.min),
            pick(self.set[SET_MAX], self.max, layout.max),
        )
    }

    /// Whether a stylesheet hides it (`display: none`).
    pub(crate) fn hidden(&self) -> bool {
        self.sheet.borrow().layout.hidden
    }

    /// The edge a stylesheet docks it to, if any.
    pub(crate) fn dock(&self) -> Option<Dock> {
        self.sheet.borrow().layout.dock
    }

    pub(crate) fn track(&self) -> Track {
        let (size, min, max) = self.dims();
        Track {
            size,
            min,
            max,
            content: 0,
        }
    }

    /// The node as selectors see it: its fixed classes, then its
    /// conditional ones that held when it last drew.
    fn element(&self) -> Element {
        let sheet = self.sheet.borrow();
        let mut classes = self.classes.clone();
        classes.extend(sheet.active.iter().cloned());
        Element {
            id: self.id,
            kind: self.what,
            name: self.name.clone(),
            fixed: self.classes.len(),
            maybe: self.class_when.iter().map(|(c, _)| c.clone()).collect(),
            classes,
            selected: sheet.selected,
            disabled: sheet.disabled,
        }
    }

    /// Match the stylesheet's rules to this node, the last of `chain`'s
    /// descendants, once per sheet: its layout, the border it draws, what
    /// its widget takes (a stack's gap, a grid's tracks, a panel's look).
    pub(crate) fn resolve(&self, cx: &SheetCx, chain: &mut Vec<Element>) {
        if self.sheet.borrow().generation == cx.generation {
            return;
        }
        chain.push(self.element());
        let layout = cx.sheet.layout(chain, cx.vars);
        let watch = cx.sheet.watches(chain);
        chain.pop();
        if !self.set[SET_GAP] {
            let gap = layout.gap.unwrap_or(0);
            self.widget_mut(|stack: &mut Stack| stack.gap = gap);
            self.widget_mut(|grid: &mut Grid| grid.gap = (gap, gap));
        }
        let rows_set = self.set[SET_ROWS];
        self.widget_mut(|grid: &mut Grid| {
            if !rows_set {
                grid.rows = layout.grid_rows.clone().unwrap_or_default();
            }
            if !grid.code_columns {
                grid.columns = layout.grid_columns.clone().unwrap_or_default();
            }
        });
        let title = layout.border_title.clone().unwrap_or_default();
        let is_panel = self
            .widget_mut(|panel: &mut Panel| {
                panel.look = crate::builtin::PanelLook {
                    border: layout.border.clone(),
                    no_border: layout.no_border,
                    title: layout.border_title.clone(),
                    padding: layout.padding,
                };
            })
            .is_some();
        let (decor, insets) = if is_panel {
            (None, [0; 4])
        } else {
            (
                layout
                    .border
                    .clone()
                    .map(|(kind, style)| (kind, style, title)),
                layout.insets(),
            )
        };
        let mut sheet = self.sheet.borrow_mut();
        sheet.generation = cx.generation;
        sheet.layout = layout;
        sheet.watch = watch;
        sheet.decor = decor;
        sheet.insets = insets;
    }

    /// Its conditional classes that hold now, and whether it is selected
    /// and disabled, reading the signals they read.
    fn conditions(&self) -> (Vec<String>, bool, bool) {
        (
            self.class_when
                .iter()
                .filter(|(_, condition)| condition())
                .map(|(class, _)| class.clone())
                .collect(),
            self.selected_when.as_ref().is_some_and(|c| c()),
            self.disabled_when.as_ref().is_some_and(|c| c()),
        )
    }

    /// Visit this node and every node inside it that is shown, depth first.
    pub(crate) fn walk(&self, f: &mut dyn FnMut(&Node, &[NodeId])) {
        let mut path = Vec::new();
        self.walk_inner(&mut path, f);
    }

    fn walk_inner(&self, path: &mut Vec<NodeId>, f: &mut dyn FnMut(&Node, &[NodeId])) {
        if self.hidden() {
            // A stylesheet's `display: none`: not shown, nor anything inside.
            return;
        }
        path.push(self.id);
        f(self, path);
        self.each_child(&mut |child| child.walk_inner(path, f));
        path.pop();
    }

    /// Call `f` with each child that is shown.
    fn each_child(&self, f: &mut dyn FnMut(&Node)) {
        for child in self.body.borrow().widget.children() {
            f(child);
        }
    }

    /// Not shown this frame: no rectangle (so no clicks reach it or
    /// anything inside it), and it draws afresh when it comes back.
    fn hide(&self) {
        self.rect.set(Rect::default());
        self.inner.set(Rect::default());
        self.drawn.set(None);
        self.highlit.set(false);
        self.hover_lit.set(false);
        // Its sheet style is applied afresh when it comes back.
        self.sheet.borrow_mut().style = None;
        self.each_child(&mut |child| child.hide());
    }

    /// How many cells of `axis` the content needs, given `width` x
    /// `height` to lay out in (a height of 0: as tall as it likes).
    pub(crate) fn measure(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        let (hidden, [top, right, bottom, left]) = {
            let sheet = self.sheet.borrow();
            (sheet.layout.hidden, sheet.insets)
        };
        if hidden {
            return 0;
        }
        let height = if height == 0 {
            0
        } else {
            height.saturating_sub(top + bottom).max(1)
        };
        let width = width.saturating_sub(left + right);
        let inner =
            self.body
                .borrow_mut()
                .widget
                .measure(&MeasureCx { console }, axis, width, height);
        inner.saturating_add(match axis {
            Axis::Vertical => top + bottom,
            Axis::Horizontal => left + right,
        })
    }

    /// Its extent along `axis` as a stack sizes it: its size, within its
    /// minimum and maximum.
    pub(crate) fn extent(&self, console: &Console, axis: Axis, width: u16, height: u16) -> u16 {
        let along = match axis {
            Axis::Vertical => height,
            Axis::Horizontal => width,
        };
        let (size, min, max) = self.dims();
        let cells = match size {
            Size::Fixed(n) => n,
            Size::Percent(p) => (along as u32 * p.min(100) as u32 / 100) as u16,
            Size::Auto | Size::Flex(_) => self.measure(console, axis, width, height),
        };
        cells.min(max).max(min)
    }

    /// Lay out and draw into `screen`. A node draws if it is dirty, has
    /// moved or changed size, or `force` says its area was overwritten;
    /// damage records what was written.
    pub(crate) fn draw(
        &self,
        frame: &mut FrameState,
        rect: Rect,
        screen: &mut Screen,
        force: bool,
    ) {
        self.rect.set(rect);
        let last = self.drawn.get();
        let moved = last != Some(rect);
        let dirty = frame.dirty.contains(&self.id);
        let redraw = moved || force || dirty;
        let id = self.id;
        // Inside the stylesheet's border and padding, the widget's own.
        let (decor, inner) = {
            let sheet = self.sheet.borrow();
            let [top, right, bottom, left] = sheet.insets;
            let inner = Rect::new(
                rect.x.saturating_add(left),
                rect.y.saturating_add(top),
                rect.width.saturating_sub(left + right),
                rect.height.saturating_sub(top + bottom),
            );
            (sheet.decor.clone(), inner)
        };
        self.inner.set(inner);
        let styled = frame.sheet.is_some();
        if styled {
            frame.chain.push(self.element());
        }
        let mut body = self.body.borrow_mut();
        let Body {
            widget,
            areas,
            laid,
            view,
        } = &mut *body;
        let viewport = widget.viewport();
        let console = frame.console;
        let focused = self.focus_style.is_some()
            && frame
                .focus_path
                .with_untracked(|path| path.last() == Some(&id));
        let hovered = self.hover_style.is_some()
            && frame.hover_path.with_untracked(|path| path.contains(&id));
        // Damage from here on is this node's and its children's: where
        // the focus style goes over what they drew.
        let mark = frame.damage.len();
        // What the widget wrote, and whether it painted all of its rectangle.
        let mut written: Vec<Rect> = Vec::new();
        let mut repaint = false;
        let mut cleared = false;
        let mut relaid = false;
        let mut restyled = false;
        if redraw {
            if let Some(last) = last.filter(|last| !last.is_empty()) {
                if (last.width, last.height) != (rect.width, rect.height) {
                    frame.resized.push(id);
                }
            }
            // The children's layout from the stylesheet, before they are
            // laid out.
            if let Some(sheet) = &frame.sheet {
                for child in widget.children() {
                    child.resolve(sheet, &mut frame.chain);
                }
            }
            // Lay out first, subscribing to what that reads; a viewport
            // lays out in its content's coordinates.
            let space = if viewport {
                Rect::new(0, 0, inner.width, inner.height)
            } else {
                inner
            };
            let new_areas = frame
                .runtime
                .observe_node(id, || widget.layout(&MeasureCx { console }, space));
            let ids: Vec<NodeId> = widget.children().iter().map(|c| c.id).collect();
            relaid =
                (!laid.is_empty() || !areas.is_empty()) && (ids != *laid || new_areas != *areas);
            *areas = new_areas;
            *laid = ids;
            // Its classes and states for the stylesheet now, and the style
            // they give it; a change draws it and what is inside again.
            if styled {
                restyled = self.restyle(frame);
            }
            // A widget that keeps what it drew is cleared only when its own
            // children moved: when it moved, or was drawn over, whoever did
            // that cleared the area already.
            // Gaining or losing the focus style redraws the lot; keeping it
            // costs only what is drawn again.
            cleared = !widget.retained()
                || focused != self.highlit.get()
                || hovered != self.hover_lit.get()
                || restyled
                || (relaid && !viewport);
            repaint = moved || force || cleared;
            if cleared {
                screen.clear(rect);
            }
            if let (true, Some((kind, style, title))) = (repaint, &decor) {
                let title_style = style.combine(&frame.theme.title);
                for edge in draw_box(screen, *kind, title, style, &title_style, rect) {
                    frame.damage.push(edge);
                }
            }
        }
        // A viewport's children draw offscreen before it decides where to
        // look; it draws itself after, so its scrollbar matches.
        let mut window = None;
        if viewport {
            window = Some(self.draw_view(frame, widget, areas, view, inner, relaid));
        }
        if redraw {
            let focus_path = frame.focus_path;
            let focus_style = self.focus_style.is_some();
            let hover_path = frame.hover_path;
            let hover_style = self.hover_style.is_some();
            if self.tooltip.is_some() {
                // A tooltip waits for the pointer to rest: its movement
                // must be reported.
                frame.wants_hover.set(true);
            }
            if hover_style {
                // The pointer's movement is reported once something reads
                // it, and this node draws again when the pointer comes or
                // goes.
                frame.wants_hover.set(true);
                frame.watchers.hover.borrow_mut().insert(id);
            }
            let caret = {
                let mut cx = DrawCx {
                    console,
                    theme: frame.theme,
                    id,
                    rect: inner,
                    repaint,
                    focus_path,
                    hover_path: frame.hover_path,
                    pointer: frame.pointer,
                    shift: frame.shift,
                    wants_hover: &frame.wants_hover,
                    watchers: frame.watchers,
                };
                let mut canvas = Canvas {
                    screen: &mut *screen,
                    rect: inner,
                    written: &mut written,
                };
                frame.runtime.observe_node_more(id, || {
                    widget.draw(&mut cx, &mut canvas);
                    // Reading the focus subscribes the node, so it draws
                    // again when the focus comes or goes.
                    if focus_style {
                        focus_path.with(|path| path.last() == Some(&id));
                    }
                    if hover_style {
                        hover_path.with(|path| path.contains(&id));
                    }
                });
                widget.caret()
            };
            self.caret.set(caret.and_then(|(x, y)| {
                (x < inner.width && y < inner.height).then_some((inner.x + x, inner.y + y))
            }));
            if cleared {
                frame.damage.push(rect);
            } else {
                frame.damage.extend(written.iter().copied());
            }
            if cleared || !written.is_empty() {
                frame.drew(id);
            }
        }
        match window {
            Some((window, offset, inner)) => {
                // The rows in view, copied out when they changed.
                let shown = view.as_ref().and_then(|v| v.shown);
                if redraw || inner || shown != Some((window, offset)) {
                    let view = view.as_mut().expect("a viewport has a view");
                    screen.clear(window);
                    screen.blit(
                        &view.buffer,
                        Rect::new(offset.0, offset.1, window.width, window.height),
                        window.x,
                        window.y,
                    );
                    frame.damage.push(window);
                    if !redraw {
                        frame.drew(id);
                    }
                    view.shown = Some((window, offset));
                }
            }
            None => {
                for (i, child) in widget.children().iter().enumerate() {
                    match areas.get(i).map(|area| area.intersection(inner)) {
                        Some(area) if !area.is_empty() => {
                            // A child draws again where the widget drew over
                            // it.
                            let over = written.iter().any(|w| !w.intersection(area).is_empty());
                            child.draw(frame, area, screen, repaint || over);
                        }
                        _ => child.hide(),
                    }
                }
            }
        }
        // The stylesheet's colours go under everything drawn inside the
        // node where nothing deeper set them, once its children have drawn.
        let sheet_style = self.sheet.borrow().style.clone();
        if let Some(style) = sheet_style {
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            if repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.underlay(column, row, &style);
                    }
                }
                frame.damage.push(area);
            }
        }
        if styled {
            frame.chain.pop();
        }
        // The focus style goes over everything drawn inside the node, its
        // children's cells included, once they have drawn.
        if let (true, Some(name)) = (focused, self.focus_style.as_deref()) {
            let style = crate::widget::theme_style(console, name, "reverse");
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            // Newly focused, or drawn in full (a restack clears the whole
            // screen first, before this node's damage is counted): the
            // whole rectangle, gaps and padding included.
            if !self.highlit.get() || repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.restyle(column, row, &style);
                    }
                }
                // Restyled cells are sent even where nothing drew (a
                // container's whole rectangle).
                frame.damage.push(area);
            }
        }
        self.highlit.set(focused);
        // The hover style, the same way, over the focus style.
        if let (true, Some(name)) = (hovered, self.hover_style.as_deref()) {
            let style = crate::widget::theme_style(console, name, "reverse");
            let mut parts: Vec<Rect> = frame.damage[mark..]
                .iter()
                .map(|area| area.intersection(rect))
                .filter(|area| !area.is_empty())
                .collect();
            if !self.hover_lit.get() || repaint {
                parts = vec![rect];
            }
            for area in parts {
                for row in area.y..area.bottom() {
                    for column in area.x..area.right() {
                        screen.restyle(column, row, &style);
                    }
                }
                // Restyled cells are sent even where nothing drew (a
                // container's whole rectangle).
                frame.damage.push(area);
            }
        }
        self.hover_lit.set(hovered);
        self.drawn.set(Some(rect));
    }

    /// Read its conditions and states for the stylesheet (subscribing to
    /// what they read) and work out its style. Whether anything changed
    /// that it, or a node inside it, must draw again for.
    fn restyle(&self, frame: &mut FrameState) -> bool {
        let Some(cx) = &frame.sheet else {
            return false;
        };
        let (any, focus, hover) = self.sheet.borrow().watch;
        let (active, selected, disabled) = frame
            .runtime
            .observe_node_more(self.id, || self.conditions());
        let changed = {
            let sheet = self.sheet.borrow();
            sheet.active != active || sheet.selected != selected || sheet.disabled != disabled
        };
        if changed {
            let mut sheet = self.sheet.borrow_mut();
            sheet.active = active;
            sheet.selected = selected;
            sheet.disabled = disabled;
        }
        if let Some(top) = frame.chain.last_mut() {
            *top = self.element();
        }
        let style = if any {
            if hover {
                frame.wants_hover.set(true);
            }
            let read = |path: Signal<Vec<NodeId>>, track: bool| {
                frame.runtime.observe_node_more(self.id, || {
                    if track {
                        path.get()
                    } else {
                        path.get_untracked()
                    }
                })
            };
            let focus_path = read(frame.focus_path, focus);
            let hover_path = read(frame.hover_path, hover);
            let states = States {
                focus: &focus_path,
                hover: &hover_path,
            };
            cx.sheet.style(&frame.chain, &states, cx.vars)
        } else {
            None
        };
        let mut sheet = self.sheet.borrow_mut();
        let restyled = sheet.style != style;
        sheet.style = style;
        changed || restyled
    }

    /// Draw a viewport's children into its offscreen content and ask it
    /// where to look: the window on the screen, the content's point at its
    /// top left, and whether anything inside drew.
    fn draw_view(
        &self,
        frame: &mut FrameState,
        widget: &mut Box<dyn Widget>,
        areas: &[Rect],
        view: &mut Option<View>,
        rect: Rect,
        relaid: bool,
    ) -> (Rect, (u16, u16), bool) {
        let content = areas.iter().fold((0u16, 0u16), |(w, h), area| {
            (w.max(area.right()), h.max(area.bottom()))
        });
        let view = view.get_or_insert_with(|| View {
            buffer: Screen::new(0, 0),
            shown: None,
        });
        let previous = view.shown;
        let mut force = relaid;
        let size = view.buffer.area();
        if (size.width, size.height) != content {
            view.buffer = Screen::new(content.0, content.1);
            force = true;
        } else if relaid {
            // The children moved: what they leave between them (gaps,
            // padding) must not keep old cells.
            view.buffer.clear(size);
        }
        if force {
            view.shown = None;
        }
        // Inside, the screen's coordinates are shifted by the window and
        // the scroll, as they were last shown.
        let outer = frame.shift;
        if let Some((window, offset)) = previous {
            frame.shift = (
                outer.0 - window.x as i32 + offset.0 as i32,
                outer.1 - window.y as i32 + offset.1 as i32,
            );
        }
        let saved = std::mem::take(&mut frame.damage);
        let all = view.buffer.area();
        for (i, child) in widget.children().iter().enumerate() {
            match areas.get(i).map(|area| area.intersection(all)) {
                Some(area) if !area.is_empty() => child.draw(frame, area, &mut view.buffer, force),
                _ => child.hide(),
            }
        }
        let inner = !std::mem::replace(&mut frame.damage, saved).is_empty();
        frame.shift = outer;
        // Where the focus is inside, for scrolling it into view.
        let focused = frame.focus_path.with_untracked(|path| {
            let last = *path.last()?;
            if !path.contains(&self.id) || last == self.id {
                return None;
            }
            let mut found = None;
            for child in widget.children() {
                with_node(child, last, &mut |node| found = Some(node.rect()));
            }
            found.map(|area| (last, area))
        });
        let cx = ScrollCx { content, focused };
        let (local, offset) = frame.runtime.untracked(|| widget.scroll(&cx));
        let window = Rect::new(
            rect.x.saturating_add(local.x),
            rect.y.saturating_add(local.y),
            local.width,
            local.height,
        )
        .intersection(rect);
        let offset = (
            offset.0.min(content.0.saturating_sub(window.width)),
            offset.1.min(content.1.saturating_sub(window.height)),
        );
        if previous != Some((window, offset)) {
            // Widgets inside that read the pointer drew with the old
            // scroll: they draw again with the new one.
            let watchers = frame.watchers.pointer.borrow();
            if !watchers.is_empty() {
                for child in widget.children() {
                    child.walk(&mut |node, _| {
                        if watchers.contains(&node.id) {
                            frame.runtime.mark_dirty(node.id);
                        }
                    });
                }
            }
        }
        (window, offset, inner)
    }

    /// Forget this subtree's subscriptions (it left the tree), including
    /// the nodes a widget keeps hidden.
    pub(crate) fn forget(&self, runtime: &Runtime) {
        runtime.forget(self.id);
        let body = self.body.borrow();
        for child in body.widget.children() {
            child.forget(runtime);
        }
        for child in body.widget.hidden_children() {
            child.forget(runtime);
        }
    }
}

/// A box's four edges: top and bottom rows, left and right columns between
/// them.
pub(crate) fn edges(rect: Rect) -> [Rect; 4] {
    let inner = rect.height.saturating_sub(2);
    [
        Rect::new(rect.x, rect.y, rect.width, 1),
        Rect::new(rect.x, rect.bottom().saturating_sub(1), rect.width, 1),
        Rect::new(rect.x, rect.y + 1, 1, inner),
        Rect::new(rect.right().saturating_sub(1), rect.y + 1, 1, inner),
    ]
}

/// A box's lines (from [`border`]) as its four edges, in the order of
/// [`edges`]: the top and bottom rows whole, and the first and last
/// segment of each row between them. Whole segments, so a title's
/// grapheme clusters (an emoji sequence) stay intact.
pub(crate) fn edge_lines(lines: &[Vec<Segment>]) -> [Vec<Vec<Segment>>; 4] {
    let middle = &lines[1.min(lines.len())..lines.len().saturating_sub(1).max(1)];
    let side = |pick: fn(&Vec<Segment>) -> Option<&Segment>| -> Vec<Vec<Segment>> {
        middle
            .iter()
            .map(|line| pick(line).cloned().into_iter().collect())
            .collect()
    };
    [
        lines.first().cloned().into_iter().collect(),
        lines.last().cloned().into_iter().collect(),
        side(|line| line.first()),
        side(|line| line.last()),
    ]
}

/// `lines` with `style` laid over every segment, each row filled out to
/// the rectangle's width in it.
fn highlight(lines: Vec<Vec<Segment>>, style: &Style, rect: Rect) -> Vec<Vec<Segment>> {
    lines
        .into_iter()
        .take(rect.height as usize)
        .map(|line| {
            let width: usize = line.iter().map(Segment::cell_length).sum();
            let mut line: Vec<Segment> = line
                .into_iter()
                .map(|segment| {
                    let combined = match &segment.style {
                        Some(own) => own.combine(style),
                        None => style.clone(),
                    };
                    Segment::new(segment.text, Some(combined))
                })
                .collect();
            let fill = (rect.width as usize).saturating_sub(width);
            if fill > 0 {
                line.push(Segment::new(" ".repeat(fill), Some(style.clone())));
            }
            line
        })
        .collect()
}

/// A one-column scrollbar `rows` tall for content `content` rows tall,
/// scrolled to `top`.
pub(crate) fn scrollbar(console: &Console, rows: u16, top: u16, content: u16) -> Vec<Vec<Segment>> {
    let style = |name: &str, fallback: &str| {
        console
            .get_style(&rich::style::StyleType::Name(name.to_string()))
            .ok()
            .or_else(|| Style::parse(fallback).ok())
    };
    let (track, thumb) = (
        style("scrollbar", "bright_black"),
        style("scrollbar.thumb", "white"),
    );
    let (rows_f, content_f) = (rows as f64, content.max(1) as f64);
    let size = (rows_f * rows_f / content_f).round().clamp(1.0, rows_f);
    let start = (top as f64 * rows_f / content_f).round().min(rows_f - size);
    (0..rows)
        .map(|row| {
            let on = (row as f64) >= start && (row as f64) < start + size;
            if on {
                vec![Segment::new("┃", thumb.clone())]
            } else {
                vec![Segment::new("│", track.clone())]
            }
        })
        .collect()
}

/// A scrollbar along the bottom of a view `columns` wide, showing
/// `left..left + columns` of `content` columns.
pub(crate) fn scrollbar_across(
    console: &Console,
    columns: u16,
    left: u16,
    content: u16,
) -> Vec<Segment> {
    let style = |name: &str, fallback: &str| {
        console
            .get_style(&rich::style::StyleType::Name(name.to_string()))
            .ok()
            .or_else(|| Style::parse(fallback).ok())
    };
    let (track, thumb) = (
        style("scrollbar", "bright_black"),
        style("scrollbar.thumb", "white"),
    );
    let (columns_f, content_f) = (columns as f64, content.max(1) as f64);
    let size = (columns_f * columns_f / content_f)
        .round()
        .clamp(1.0, columns_f);
    let start = (left as f64 * columns_f / content_f)
        .round()
        .min(columns_f - size);
    (0..columns)
        .map(|column| {
            let on = (column as f64) >= start && (column as f64) < start + size;
            if on {
                Segment::new("━", thumb.clone())
            } else {
                Segment::new("─", track.clone())
            }
        })
        .collect()
}

/// A visit in [`walk_screen`]: the node, its path, the shift to screen
/// coordinates and the part of the screen it can show in.
pub(crate) type ScreenVisit<'a> = dyn FnMut(&Node, &[NodeId], (i32, i32), Rect) + 'a;

/// Visit every node that is shown, with the translation that takes its
/// coordinates to the screen's and the part of the screen it can show in
/// (both change inside a [`scroll`]).
pub(crate) fn walk_screen(root: &Node, f: &mut ScreenVisit) {
    fn inner(
        node: &Node,
        path: &mut Vec<NodeId>,
        shift: (i32, i32),
        clip: Rect,
        f: &mut ScreenVisit,
    ) {
        path.push(node.id);
        f(node, path, shift, clip);
        let scrolled = node.body.borrow().view.as_ref().and_then(|view| {
            let (window, offset) = view.shown?;
            let window = translate(window, shift);
            Some((
                (
                    window.x as i32 - offset.0 as i32,
                    window.y as i32 - offset.1 as i32,
                ),
                window.intersection(clip),
            ))
        });
        let (shift, clip) = scrolled.unwrap_or((shift, clip));
        node.each_child(&mut |child| inner(child, path, shift, clip, f));
        path.pop();
    }
    let all = Rect::new(0, 0, u16::MAX, u16::MAX);
    inner(root, &mut Vec::new(), (0, 0), all, f);
}

/// `rect` moved by `shift`, clamped to the screen.
pub(crate) fn translate(rect: Rect, (dx, dy): (i32, i32)) -> Rect {
    let x = rect.x as i32 + dx;
    let y = rect.y as i32 + dy;
    let (x0, y0) = (x.max(0), y.max(0));
    let width = (rect.width as i32 - (x0 - x)).max(0);
    let height = (rect.height as i32 - (y0 - y)).max(0);
    Rect::new(
        x0.min(u16::MAX as i32) as u16,
        y0.min(u16::MAX as i32) as u16,
        width.min(u16::MAX as i32) as u16,
        height.min(u16::MAX as i32) as u16,
    )
}

/// What a frame needs while it draws.
pub(crate) struct FrameState<'a> {
    pub console: &'a Console,
    pub runtime: &'a Runtime,
    pub dirty: HashSet<NodeId>,
    pub damage: Vec<Rect>,
    pub focus_path: Signal<Vec<NodeId>>,
    /// The nodes under the mouse pointer, outermost first.
    pub hover_path: Signal<Vec<NodeId>>,
    /// A widget asked whether it is hovered: the app turns on the
    /// terminal's pointer movement reports.
    pub wants_hover: Cell<bool>,
    /// The nodes that asked about the pointer.
    pub watchers: &'a Watchers,
    /// Where the pointer last was, on the screen.
    pub pointer: Option<(u16, u16)>,
    /// From the screen to the surface being drawn on (inside a viewport,
    /// its content).
    pub shift: (i32, i32),
    /// Nodes laid out at a new size this frame.
    pub resized: Vec<NodeId>,
    pub theme: &'a crate::app::Theme,
    /// Nodes drawn this frame.
    pub drawn: usize,
    /// Which, when the inspector is watching.
    pub drawn_ids: Option<Vec<NodeId>>,
    /// The app's stylesheet, if it has one.
    pub sheet: Option<SheetCx<'a>>,
    /// The nodes being drawn, outermost first, as selectors see them.
    pub chain: Vec<Element>,
}

impl FrameState<'_> {
    /// `id` drew this frame.
    pub fn drew(&mut self, id: NodeId) {
        self.drawn += 1;
        if let Some(ids) = &mut self.drawn_ids {
            ids.push(id);
        }
    }
}

/// A rounded border round `rect`, with `title` in the top edge; the inside
/// is left to the child.
pub(crate) fn border(
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Vec<Segment>> {
    border_box(BoxKind::Round, title, style, title_style, rect)
}

/// [`border`] with a `kind` of box.
pub(crate) fn border_box(
    kind: BoxKind,
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Vec<Segment>> {
    let [top_left, across, top_right, side, bottom_left, bottom_right] = kind.chars();
    let (w, h) = (rect.width as usize, rect.height as usize);
    if w < 2 || h < 2 {
        return Vec::new();
    }
    let edge = |s: String| Segment::new(s, Some(style.clone()));
    let title = if title.is_empty() {
        String::new()
    } else {
        format!(" {title} ")
    };
    let room = w.saturating_sub(4);
    let title = rich::cells::set_cell_size(&title, rich::cells::cell_len(&title).min(room));
    let title_len = rich::cells::cell_len(&title);
    let mut lines = Vec::with_capacity(h);
    if w < 4 {
        // Too narrow for a title: a plain box.
        lines.push(vec![edge(format!(
            "{top_left}{}{top_right}",
            across.repeat(w - 2)
        ))]);
    } else {
        // `title_len` is at most `w - 4`, so one dash always follows it.
        lines.push(vec![
            edge(format!("{top_left}{across}")),
            Segment::new(title, Some(title_style.clone())),
            edge(format!("{}{top_right}", across.repeat(w - 3 - title_len))),
        ]);
    }
    for _ in 0..h - 2 {
        lines.push(vec![
            edge(side.into()),
            Segment::new(" ".repeat(w - 2), None),
            edge(side.into()),
        ]);
    }
    lines.push(vec![edge(format!(
        "{bottom_left}{}{bottom_right}",
        across.repeat(w - 2)
    ))]);
    lines
}

/// Draw the edges of a `kind` box round `rect`, with `title` in its top
/// edge; the edges written.
pub(crate) fn draw_box(
    screen: &mut Screen,
    kind: BoxKind,
    title: &str,
    style: &Style,
    title_style: &Style,
    rect: Rect,
) -> Vec<Rect> {
    let lines = border_box(kind, title, style, title_style, rect);
    if lines.is_empty() {
        return Vec::new();
    }
    let mut written = Vec::new();
    for (edge, part) in edges(rect).into_iter().zip(edge_lines(&lines)) {
        if !edge.is_empty() {
            screen.write_lines(edge, &part);
            written.push(edge);
        }
    }
    written
}

// Builders.

/// A node that draws itself from rendered lines; `draw` gets the console,
/// the width and the height. Signals it reads make it draw again. A height
/// of 0 asks how tall the node would like to be: a parent laying out a
/// [`Size::Auto`] child measures it so.
pub fn leaf(draw: impl Fn(&Console, u16, u16) -> Vec<Vec<Segment>> + 'static) -> Node {
    Node::from_widget(Box::new(Leaf(Box::new(draw))), "leaf")
}

/// Console markup, from a closure that may read signals:
/// `text(move || format!("[b]{}[/] items", count.get()))`. The
/// [`text!`](crate::text!) macro writes the closure for you.
pub fn text(markup: impl Fn() -> String + 'static) -> Node {
    let node = leaf(move |console, width, _| {
        let markup = markup();
        let text = rich::Text::from_markup(&markup).unwrap_or_else(|_| rich::Text::new(markup));
        console.render_lines(
            &text,
            &console.options().update_width(width.max(1) as usize),
            false,
        )
    });
    node.what("text")
}

/// Console markup that never changes.
pub fn label(markup: impl Into<String>) -> Node {
    let markup = markup.into();
    text(move || markup.clone()).what("label")
}

/// Any rich renderable, built by `f` (which may read signals) and rendered
/// at the node's size: a `Table`, `Markdown`, `Syntax`, `Panel`, a chart.
pub fn renderable<R: Renderable + 'static>(f: impl Fn() -> R + 'static) -> Node {
    leaf(move |console, width, height| {
        let options = console.options().update_width(width.max(1) as usize);
        // Height 0 is a measurement: as tall as the renderable likes.
        let options = if height == 0 {
            options
        } else {
            options.update_height(height as usize)
        };
        console.render_lines(&f(), &options, false)
    })
    .what("renderable")
}

/// A `rich-interact` component as a node: an `Input`, a `Select`, a
/// `Form`, a `Pager`, or one of your own. It takes keys while it has the
/// focus (keys it does not use bubble on), shows its text caret, and calls
/// `on_done` with its answer (Enter in an `Input`, a pick in a `Select`).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let name = signal(String::new());
///     column([
///         component(Input::new("Name"), move |value, _| name.set(value)),
///         text!("Hello, {name}"),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["A", "d", "a", "enter", "esc"], 30, 3).unwrap();
/// assert!(screen.iter().any(|l| l.contains("Hello, Ada")), "{screen:?}");
/// ```
pub fn component<C: Component + 'static>(
    component: C,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::from_widget(
        Box::new(HostWidget::new(Box::new(Host {
            component,
            make: None,
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A [`component`] that starts again after each answer: `make` builds it,
/// and builds a fresh one each time it is done, so an `Input` clears for
/// the next entry (a chat box, a to-do entry, a command line).
///
/// ```
/// use intuituive::prelude::*;
/// use rich_interact::Input;
///
/// let app = App::new(|| {
///     let added = signal(Vec::<String>::new());
///     column([
///         repeating(|| Input::new("Add"), move |item, _| added.update(|v| v.push(item))),
///         text(move || added.get().join(", ")),
///     ])
///     .on_key("esc", |cx| cx.quit())
/// });
/// let keys = ["a", "enter", "b", "enter", "esc"];
/// let screen = app.render_with(&keys, 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "a, b");
/// ```
pub fn repeating<C: Component + 'static>(
    make: impl Fn() -> C + 'static,
    on_done: impl FnMut(C::Output, &mut Ctx) + 'static,
) -> Node {
    let mut node = Node::from_widget(
        Box::new(HostWidget::new(Box::new(Host {
            component: make(),
            make: Some(Box::new(make)),
            on_done: Box::new(on_done),
            on_cancel: None,
        }))),
        "component",
    );
    node.focusable = true;
    node.what("component")
}

/// A scrolling list of markup rows, one selected: ratatui's `List` and
/// `ListState` in one node. `items` may read signals; `selected` is the
/// index of the selected row, which you read and write like any signal.
///
/// The list keeps the selected row in view, scrolling only as far as it
/// must, and draws it in the theme's `selected` style, filled to the full
/// width. It is focusable, and while it has the focus ↑/k, ↓/j, Home/g,
/// End/G, PageUp and PageDown move the selection; bind Enter (or anything
/// else) yourself with [`on_key`](Node::on_key).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let selected = signal(0usize);
///     let items = || (1..=50).map(|n| format!("item {n}")).collect();
///     list(items, selected).on_key("q", |cx| cx.quit())
/// });
/// // Down 7 times in a 5-row list: rows 4-8 (items 4-8) show, item 8 selected.
/// let keys = ["j", "j", "j", "j", "j", "j", "j", "q"];
/// let screen = app.render_with(&keys, 20, 5).unwrap();
/// assert_eq!(screen[0].trim_end(), "item 4");
/// assert_eq!(screen[4].trim_end(), "item 8");
/// ```
pub fn list(items: impl Fn() -> Vec<String> + 'static, selected: Signal<usize>) -> Node {
    let items = Rc::new(items);
    // The first row shown, and the height last drawn (for a page's size).
    let offset = Rc::new(Cell::new(0usize));
    let rows = Rc::new(Cell::new(1usize));
    let draw = {
        let (items, offset, rows) = (items.clone(), offset.clone(), rows.clone());
        move |console: &Console, width: u16, height: u16| {
            let items = items();
            let len = items.len();
            // Height 0 is a measurement: every row.
            let height = if height == 0 { len } else { height as usize };
            rows.set(height.max(1));
            let selected = selected.get().min(len.saturating_sub(1));
            let mut first = offset.get().min(len.saturating_sub(height));
            if selected < first {
                first = selected;
            } else if selected >= first + height {
                first = selected + 1 - height;
            }
            offset.set(first);
            let mut options = console.options().update_width(width.max(1) as usize);
            options.no_wrap = Some(true);
            options.overflow = Some(rich::Overflow::Ellipsis);
            let style = console
                .get_style(&rich::style::StyleType::Name("selected".into()))
                .unwrap_or_else(|_| Style::parse("reverse").expect("parses"));
            items
                .iter()
                .enumerate()
                .skip(first)
                .take(height)
                .map(|(i, markup)| {
                    let text = rich::Text::from_markup(markup)
                        .unwrap_or_else(|_| rich::Text::new(markup.clone()));
                    let line = console
                        .render_lines(&text, &options, false)
                        .into_iter()
                        .next()
                        .unwrap_or_default();
                    if i == selected {
                        let row = Rect::new(0, 0, width, 1);
                        highlight(vec![line], &style, row).remove(0)
                    } else {
                        line
                    }
                })
                .collect()
        }
    };
    let len = {
        let items = items.clone();
        move || items().len()
    };
    let step = move |by: isize| {
        let len = len();
        let last = len.saturating_sub(1) as isize;
        selected.update(|s| *s = (*s as isize + by).clamp(0, last.max(0)) as usize);
    };
    let (rows_up, rows_down) = (rows.clone(), rows);
    let (up, down, pgup, pgdn) = (step.clone(), step.clone(), step.clone(), step);
    leaf(draw)
        .what("list")
        .focusable()
        .on_key("up k", move |_| up(-1))
        .on_key("down j", move |_| down(1))
        .on_key("pageup", move |_| pgup(-(rows_up.get() as isize)))
        .on_key("pagedown", move |_| pgdn(rows_down.get() as isize))
        .on_key("home g", move |_| selected.set(0))
        .on_key("end G", move |_| {
            let len = items().len();
            selected.set(len.saturating_sub(1));
        })
}

/// A viewport onto `child`, laid out at its full height (up to 4000 rows)
/// and scrolled with the arrow keys, PgUp/PgDn and Home/End (when nothing
/// inside used them) and the mouse wheel, with a scrollbar when it does not
/// fit. Focus moving to a node inside scrolls it into view. Only the nodes
/// inside that changed draw; the rows in view are copied out.
pub fn scroll(child: Node) -> Node {
    scroll_with(child, crate::reactive::signal(0))
}

/// [`scroll`], with the first row in view in `offset`: read it, or set it
/// to scroll from code.
pub fn scroll_with(child: Node, offset: Signal<u16>) -> Node {
    Node::from_widget(Box::new(ScrollView::new(child, offset)), "scroll").focusable()
}

/// `child` in a view that scrolls across: the content is as wide as it
/// asks for (lines are not wrapped to the view) and as tall as the view.
/// ←/→, Shift+PgUp/PgDn, the wheel and Home/End scroll it, and a bar
/// along the bottom shows where it is.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     scroll_x(label("0123456789abcdefghij")).on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["right", "right", "q"], 8, 2).unwrap();
/// assert_eq!(screen[0], "23456789");
/// ```
pub fn scroll_x(child: Node) -> Node {
    scroll_both_with(
        child,
        crate::reactive::signal(0),
        crate::reactive::signal(0),
        true,
        false,
    )
}

/// `child` in a view that scrolls down and across, with a bar for each
/// way it overflows. Shift with the wheel scrolls across.
pub fn scroll_both(child: Node) -> Node {
    scroll_both_with(
        child,
        crate::reactive::signal(0),
        crate::reactive::signal(0),
        true,
        true,
    )
}

/// A scrolling view with its first column and row in view in `x` and `y`,
/// scrolling across, down, or both.
pub fn scroll_both_with(
    child: Node,
    x: Signal<u16>,
    y: Signal<u16>,
    across: bool,
    down: bool,
) -> Node {
    let view = ScrollView::both(child, x, y, across, down);
    Node::from_widget(Box::new(view), "scroll").focusable()
}

/// Children one above the other.
pub fn column(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Vertical, children).what("column")
}

/// Children side by side.
pub fn row(children: impl IntoIterator<Item = Node>) -> Node {
    stack(Axis::Horizontal, children).what("row")
}

fn stack(axis: Axis, children: impl IntoIterator<Item = Node>) -> Node {
    Node::from_widget(
        Box::new(Stack {
            axis,
            gap: 0,
            children: children.into_iter().collect(),
        }),
        "stack",
    )
}

/// Children in a grid with `columns`, filled row by row. A child can span
/// several columns and rows with [`Node::span`]; it takes the first place
/// where it fits. Rows share the height evenly unless sized with
/// [`Node::rows`]; [`Node::gap`] spaces the cells.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     grid(
///         [Size::Flex(1), Size::Flex(1)],
///         [
///             label("CPU").panel("1"),
///             label("Memory").panel("2"),
///             label("Disk").panel("3").span(2, 1),
///         ],
///     )
///     .rows([Size::Fixed(3)])
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["q"], 20, 6).unwrap();
/// assert!(screen[1].contains("CPU") && screen[1].contains("Memory"));
/// assert!(screen[4].contains("Disk"));
/// ```
pub fn grid(
    columns: impl IntoIterator<Item = Size>,
    children: impl IntoIterator<Item = Node>,
) -> Node {
    let columns: Vec<Size> = columns.into_iter().collect();
    Node::from_widget(
        Box::new(Grid {
            code_columns: !columns.is_empty(),
            columns,
            rows: Vec::new(),
            gap: (0, 0),
            children: children.into_iter().collect(),
        }),
        "grid",
    )
}

/// One child per key, top to bottom, each one row tall unless it is
/// [`fixed`](Node::fixed) to more or [`auto`](Node::auto) (as tall as its
/// content). `keys` may read signals; a key that stays
/// keeps its node (its state, its focus, its cached drawing) however the
/// list is reordered, and only new keys are built (`build` gets a clone of
/// the key).
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let items = signal(vec!["a", "b"]);
///     each(move || items.get(), |item| text!("• {item}"))
/// });
/// ```
pub fn each<K, F>(keys: impl Fn() -> Vec<K> + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    Node::from_widget(
        Box::new(Each {
            keys: Rc::new(keys),
            build,
            order: Vec::new(),
            children: Vec::new(),
        }),
        "each",
    )
}

/// The child for the current `key`, built by `build` the first time the key
/// is shown and kept while other keys are: tabs, the steps of a wizard, a
/// detail pane per item. `key` may read signals; when it changes, the
/// switch shows the other child, which keeps its state (and its focus
/// order) from when it was last shown. Only the shown child is in the Tab
/// order and takes clicks.
///
/// ```
/// use intuituive::prelude::*;
///
/// let app = App::new(|| {
///     let tab = signal(0usize);
///     column([
///         label("[b]1[/] Overview · [b]2[/] Logs").fixed(1),
///         switch(move || tab.get(), |tab| match tab {
///             0 => label("All systems go"),
///             _ => label("No logs yet"),
///         }),
///     ])
///     .on_key("1", move |_| tab.set(0))
///     .on_key("2", move |_| tab.set(1))
///     .on_key("q", |cx| cx.quit())
/// });
/// let screen = app.render_with(&["2", "q"], 30, 2).unwrap();
/// assert_eq!(screen[1].trim_end(), "No logs yet");
/// ```
pub fn switch<K, F>(key: impl Fn() -> K + 'static, build: F) -> Node
where
    K: Clone + Eq + Hash + 'static,
    F: Fn(K) -> Node + 'static,
{
    Node::from_widget(
        Box::new(Switch {
            key: Box::new(key),
            build,
            built: HashMap::new(),
            current: None,
        }),
        "switch",
    )
}

/// The nodes under `column`, `row`, outermost first (the path to the
/// deepest node whose shown rectangle holds the point), each with the
/// translation from screen coordinates to its own.
pub(crate) fn hit_path(root: &Node, column: u16, row: u16) -> Vec<(NodeId, (i32, i32))> {
    let mut found: Vec<(NodeId, (i32, i32))> = Vec::new();
    let mut shifts: Vec<(i32, i32)> = Vec::new();
    walk_screen(root, &mut |node, path, shift, clip| {
        shifts.truncate(path.len() - 1);
        shifts.push(shift);
        let shown = translate(node.rect(), shift).intersection(clip);
        if shown.contains(column, row) && path.len() > found.len() {
            found = path
                .iter()
                .zip(&shifts)
                .map(|(id, (dx, dy))| (*id, (-dx, -dy)))
                .collect();
        }
    });
    found
}

/// Every node shown, with the translation from screen coordinates to its
/// own (they differ inside a viewport).
pub(crate) fn shifts(root: &Node) -> Vec<(NodeId, (i32, i32))> {
    let mut out = Vec::new();
    walk_screen(root, &mut |node, _, shift, _| {
        out.push((node.id, (-shift.0, -shift.1)))
    });
    out
}

/// Run `f` on the node with `id`.
/// The keys named in `names`, for a binding.
///
/// # Panics
/// On a name that is not a key, or on no names at all (`""`, or `" "`,
/// which is a separator: the space key is `space`).
pub(crate) fn parse_keys(names: &str) -> Vec<Key> {
    let keys = rich_interact::keymap::keys(names);
    assert!(
        !keys.is_empty(),
        "no keys in {names:?}: name them separated by spaces; the space key is \"space\""
    );
    keys
}

pub(crate) fn with_node(root: &Node, id: NodeId, f: &mut dyn FnMut(&Node)) {
    let mut done = false;
    root.walk(&mut |node, _| {
        if !done && node.id == id {
            f(node);
            done = true;
        }
    });
}
